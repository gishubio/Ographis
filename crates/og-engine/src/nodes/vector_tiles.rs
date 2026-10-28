//! Vector tiles, as Mapbox and MapLibre draw maps from them: MBTiles and PMTiles files, a
//! folder of tiles, and tiles on the web from an address with {z}/{x}/{y}. GDAL reads the
//! tiles; this puts the pieces of a feature that the tiles cut apart together again, and
//! fetches the tiles of a rectangle from the web.

use super::*;
use crate::io::ReadOpts;
use crate::model::{err, fmt_count, Feature, GbError, Result, Table, Value};
use geo::{Geometry, MultiLineString, MultiPoint};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A folder of vector tiles: z/x/y.pbf with a metadata.json, as written by tippecanoe or GDAL.
pub fn is_tile_folder(path: &Path) -> bool {
    path.is_dir()
        && path.join("metadata.json").is_file()
        && std::fs::read_dir(path).is_ok_and(|d| d.flatten().any(|e| e.path().is_dir() && e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit())))
}

/// A file or folder of vector tiles, read by GDAL.
pub fn is_vector_tiles(path: &Path) -> bool {
    matches!(ext_of(path).as_str(), "mbtiles" | "pmtiles") || is_tile_folder(path) || path.to_string_lossy().starts_with("MVT:")
}

/// An address of tiles on the web, such as https://…/{z}/{x}/{y}.pbf.
pub fn is_template(url: &str, web_as: Option<&str>) -> bool {
    let has = url.contains("{z}") && url.contains("{x}") && url.contains("{y}");
    has && (web_as == Some("vectortiles") || {
        let ext = super::input::source_ext(url);
        matches!(ext.as_str(), "pbf" | "mvt") || url.to_lowercase().contains("vector")
    })
}

/// The tiles of a rectangle in degrees at a zoom level, as x and y ranges.
pub fn tile_range(rect: [f64; 4], z: u32) -> (std::ops::RangeInclusive<u32>, std::ops::RangeInclusive<u32>) {
    let n = 2f64.powi(z as i32);
    let last = (n as u32).saturating_sub(1);
    let x = |lon: f64| (((lon + 180.0) / 360.0 * n).floor().max(0.0) as u32).min(last);
    let y = |lat: f64| {
        let lat = lat.clamp(-85.051_128, 85.051_128).to_radians();
        (((1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n).floor().max(0.0) as u32).min(last)
    };
    (x(rect[0])..=x(rect[2]), y(rect[3])..=y(rect[1]))
}

/// The pieces of a feature that tiles cut apart, put together again: those with the same id
/// (mvt_id, as GDAL and most tile makers keep it) and the same values become one feature.
/// Areas are joined, lines are kept as the parts of one multi line, and a point that is in
/// two tiles is kept once. Gives how many pieces there were.
pub fn join_pieces(t: &mut Table) -> usize {
    let id_at = t.field_index("mvt_id");
    let before = t.len();
    if before < 2 {
        return before;
    }
    // The key of a feature: its id when there is one, and else all of its values.
    let key = |f: &Feature| -> String {
        let vals: Vec<String> = f.attrs.iter().enumerate().filter(|(i, _)| Some(*i) != id_at).map(|(_, v)| v.to_text()).collect();
        match id_at.map(|i| &f.attrs[i]) {
            Some(Value::Null) | None => vals.join("\u{1}"),
            Some(id) => format!("{}\u{2}{}", id.to_text(), vals.join("\u{1}")),
        }
    };
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Feature>> = HashMap::new();
    for f in t.features.drain(..) {
        let k = key(&f);
        if !groups.contains_key(&k) {
            order.push(k.clone());
        }
        groups.entry(k).or_default().push(f);
    }
    for k in order {
        let mut pieces = groups.remove(&k).unwrap_or_default();
        if pieces.len() == 1 {
            t.features.push(pieces.pop().expect("one"));
            continue;
        }
        let mut first = pieces[0].clone();
        first.z = None;
        first.geom = join_geometries(pieces.iter().filter_map(|p| p.geom.clone()).collect());
        t.features.push(first);
    }
    before
}

fn join_geometries(parts: Vec<Geometry<f64>>) -> Option<Geometry<f64>> {
    let mut polys = Vec::new();
    let mut lines = Vec::new();
    let mut points = Vec::new();
    for g in parts {
        match g {
            Geometry::Polygon(p) => polys.push(p),
            Geometry::MultiPolygon(m) => polys.extend(m.0),
            Geometry::LineString(l) => lines.push(l),
            Geometry::MultiLineString(m) => lines.extend(m.0),
            Geometry::Point(p) => points.push(p),
            Geometry::MultiPoint(m) => points.extend(m.0),
            _ => {}
        }
    }
    if !polys.is_empty() {
        let joined = geo::unary_union(polys.iter());
        return Some(if joined.0.len() == 1 { Geometry::Polygon(joined.0.into_iter().next().expect("one")) } else { Geometry::MultiPolygon(joined) });
    }
    if !lines.is_empty() {
        return Some(if lines.len() == 1 { Geometry::LineString(lines.pop().expect("one")) } else { Geometry::MultiLineString(MultiLineString(lines)) });
    }
    let mut kept: Vec<geo::Point<f64>> = Vec::new();
    for p in points {
        if !kept.iter().any(|k| (k.x() - p.x()).abs() < 1e-6 && (k.y() - p.y()).abs() < 1e-6) {
            kept.push(p);
        }
    }
    match kept.len() {
        0 => None,
        1 => Some(Geometry::Point(kept[0])),
        _ => Some(Geometry::MultiPoint(MultiPoint(kept))),
    }
}

/// The open options of GDAL for a zoom level, if one is chosen.
pub fn zoom_options(p: &P) -> Result<Vec<String>> {
    Ok(match zoom_of(p)? {
        Some(z) => vec![format!("ZOOM_LEVEL={}", z)],
        None => Vec::new(),
    })
}

fn zoom_of(p: &P) -> Result<Option<u32>> {
    match p.get("vt_zoom").filter(|z| !z.trim().is_empty()) {
        None => Ok(None),
        Some(z) => match z.trim().parse::<u32>() {
            Ok(z) if z <= 24 => Ok(Some(z)),
            _ => err(format!("The zoom level of the vector tiles must be a whole number from 0 to 24, not \"{}\".", z)),
        },
    }
}

/// Vector tiles from an address with {z}/{x}/{y}: those of the rectangle at the zoom level
/// asked for, each read by GDAL, with the pieces of each feature put together again.
pub(super) fn run_web(p: &P, ctx: &mut NodeCtx, url: &str, headers: &[(String, String)]) -> Result<Outputs> {
    let b = ctx.backend("Vector tiles")?;
    let z = zoom_of(p)?.unwrap_or(14);
    let most = match p.get("tiles_most") {
        None => 500,
        Some(t) => t
            .replace(' ', "")
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| GbError(format!("The largest number of tiles must be a whole number above zero, not \"{}\".", t)))?,
    };
    if p.get("within").as_deref() != Some("rectangle") {
        return err("Vector tiles from the web are read within a rectangle: choose Rectangle under Read, with its sides and their coordinate system.");
    }
    let side = |k: &str, what: &str| -> Result<f64> {
        let v = p.get(k).unwrap_or_default();
        crate::model::parse_number(v.trim()).ok_or_else(|| GbError(format!("{} of the rectangle must be a number, not \"{}\".", what, v)))
    };
    let r = [side("min_x", "The smallest x")?, side("min_y", "The smallest y")?, side("max_x", "The largest x")?, side("max_y", "The largest y")?];
    let from = p.crs("within_crs")?.unwrap_or(4326);
    let rect = if from == 4326 {
        r
    } else {
        let (mut xs, mut ys) = (vec![r[0], r[2], r[2], r[0]], vec![r[1], r[1], r[3], r[3]]);
        b.transform(from, 4326, &mut xs, &mut ys)?;
        let fold = |v: &[f64], f: fn(f64, f64) -> f64, s: f64| v.iter().copied().fold(s, f);
        [fold(&xs, f64::min, f64::INFINITY), fold(&ys, f64::min, f64::INFINITY), fold(&xs, f64::max, f64::NEG_INFINITY), fold(&ys, f64::max, f64::NEG_INFINITY)]
    };
    let (xs, ys) = tile_range(rect, z);
    let count = (xs.end() - xs.start() + 1) as usize * (ys.end() - ys.start() + 1) as usize;
    if count > most {
        return err(format!(
            "The rectangle covers {} tiles at zoom level {}, more than {}. Choose a lower zoom level, a smaller rectangle, or raise the largest number of tiles.",
            fmt_count(count as u64),
            z,
            fmt_count(most as u64)
        ));
    }
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("og-vector-tiles-{}-{}", std::process::id(), run));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| GbError(format!("Cannot make a folder for the tiles: {}", e)))?;
    let wanted: Vec<String> =
        p.get("layer").map(|l| l.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "*").collect()).unwrap_or_default();
    let mut tables: Vec<Arc<Table>> = Vec::new();
    let mut fetched = 0usize;
    let mut done = 0u64;
    let result = (|| -> Result<()> {
        for x in xs.clone() {
            for y in ys.clone() {
                if !ctx.gate.going() {
                    return err(crate::model::STOPPED);
                }
                (ctx.progress)("fetched tiles", done, Some(count as u64));
                done += 1;
                let address = url.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string());
                let bytes = match b.fetch_bytes(&address, headers) {
                    Ok(b) => b,
                    // A tile with nothing in it is often not there at all.
                    Err(e) if e.0.contains("404") || e.0.contains("204") => continue,
                    Err(e) => return Err(e),
                };
                if bytes.is_empty() {
                    continue;
                }
                fetched += 1;
                let file = dir.join(format!("{}_{}_{}.mvt", z, x, y));
                std::fs::write(&file, gunzip(bytes)).map_err(|e| GbError(format!("Cannot keep a tile: {}", e)))?;
                let source = PathBuf::from(format!("MVT:{}", file.display()));
                let names: Vec<String> = if wanted.is_empty() { b.layers(&source)?.into_iter().map(|l| l.name).collect() } else { wanted.clone() };
                let options = vec![format!("X={}", x), format!("Y={}", y), format!("Z={}", z)];
                for name in names {
                    let opts = ReadOpts { layer: Some(name.clone()), open_options: options.clone(), allow_no_crs: true, ..Default::default() };
                    let mut t = match b.read(&source, &opts, &mut ctx.notes) {
                        Ok(t) => t,
                        // A layer that is not in this tile.
                        Err(_) if !wanted.is_empty() => continue,
                        Err(e) => return Err(e),
                    };
                    t.crs = Some(3857);
                    let at = super::geom_inspect::column(&mut t, "layer", crate::model::FieldType::Text);
                    for f in t.features.iter_mut() {
                        f.attrs.resize(t.fields.len(), Value::Null);
                        f.attrs[at] = Value::Text(name.clone());
                    }
                    tables.push(Arc::new(t));
                }
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result?;
    (ctx.progress)("fetched tiles", count as u64, Some(count as u64));
    if tables.is_empty() {
        ctx.notes.warn(format!("None of the {} tiles at zoom level {} had any features.", fmt_count(count as u64), z));
        return Ok(vec![("Output".into(), Table { crs: Some(3857), ..Default::default() })]);
    }
    let merged = merge_tables(tables, Some(b), "the tiles", &mut ctx.notes)?;
    let mut t = Arc::try_unwrap(merged).unwrap_or_else(|a| (*a).clone());
    let pieces = t.len();
    if p.get("vt_pieces").as_deref() != Some("keep") {
        join_pieces(&mut t);
    }
    ctx.notes.info(format!(
        "{} tiles at zoom level {}: {} pieces, {} features",
        fmt_count(fetched as u64),
        z,
        fmt_count(pieces as u64),
        fmt_count(t.len() as u64)
    ));
    Ok(vec![("Output".into(), t)])
}

/// Tiles are often sent compressed with gzip, also without saying so in a header.
fn gunzip(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut out = Vec::new();
        if flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Field, FieldType};
    use geo::polygon;

    #[test]
    fn the_tiles_of_a_rectangle_and_pieces_put_together() {
        // Göteborg at zoom 10: one or two tiles across.
        let (xs, ys) = tile_range([11.9, 57.68, 12.0, 57.72], 10);
        assert_eq!((*xs.start(), *ys.start()), (545, 309));
        assert!(xs.end() - xs.start() <= 1 && ys.end() - ys.start() <= 1);
        let (xs, ys) = tile_range([-180.0, -85.0, 180.0, 85.0], 2);
        assert_eq!((xs, ys), (0..=3, 0..=3));
        // A lake cut in two by a tile edge, and a road that is one piece.
        let sq = |x0: f64, x1: f64| Some(Geometry::Polygon(polygon![(x: x0, y: 0.0), (x: x1, y: 0.0), (x: x1, y: 1.0), (x: x0, y: 1.0), (x: x0, y: 0.0)]));
        let mut t = Table {
            fields: vec![Field::new("mvt_id", FieldType::Int), Field::new("name", FieldType::Text)],
            features: vec![
                Feature { attrs: vec![Value::Int(7), Value::Text("Lake".into())], geom: sq(0.0, 1.0), z: None },
                Feature { attrs: vec![Value::Int(8), Value::Text("Road".into())], geom: sq(5.0, 6.0), z: None },
                Feature { attrs: vec![Value::Int(7), Value::Text("Lake".into())], geom: sq(1.0, 2.0), z: None },
            ],
            ..Default::default()
        };
        assert_eq!(join_pieces(&mut t), 3);
        assert_eq!(t.len(), 2);
        use geo::Area;
        let lake = t.features[0].geom.as_ref().unwrap();
        assert!(matches!(lake, Geometry::Polygon(_)), "{:?}", lake);
        assert!((lake.unsigned_area() - 2.0).abs() < 1e-9);
        assert!(is_template("https://tiles.example.org/data/v3/{z}/{x}/{y}.pbf", None));
        assert!(!is_template("https://tile.openstreetmap.org/{z}/{x}/{y}.png", None));
    }
}
