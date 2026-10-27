//! Point clouds: LAS and LAZ, as laser scanning from the air gives them, such as the
//! Laserdata of Lantmäteriet. Each point becomes a feature with its height and what the
//! scanner says about it.

use crate::io::Notes;
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use std::path::Path;
use std::sync::Arc;

/// What to read of a point cloud.
#[derive(Debug, Clone, Default)]
pub struct LasOpts {
    /// Only points of these classes, such as 2 for ground and 6 for buildings.
    pub classes: Option<Vec<u8>>,
    /// Every n-th point, to thin a dense cloud. 1 reads all.
    pub every: usize,
    /// At most this many points.
    pub most: Option<usize>,
    /// Only points within this rectangle, as west, south, east and north in the coordinates
    /// of the file.
    pub within: Option<[f64; 4]>,
    /// The coordinate system, when the file does not say or says wrong.
    pub crs: Option<u32>,
    /// For COPC: only the levels of the octree down to about this distance between points.
    pub spacing: Option<f64>,
}

/// The names of the classes of ASPRS that laser scanning in Sweden uses.
pub fn class_name(c: u8) -> &'static str {
    match c {
        0 => "never classified",
        1 => "unclassified",
        2 => "ground",
        3 => "low vegetation",
        4 => "medium vegetation",
        5 => "high vegetation",
        6 => "building",
        7 => "low point (noise)",
        9 => "water",
        10 => "rail",
        11 => "road surface",
        13 => "wire guard",
        14 => "wire conductor",
        15 => "transmission tower",
        17 => "bridge deck",
        18 => "high noise",
        _ => "other",
    }
}

/// The EPSG code of the projected coordinate system in WKT, or of the geographic one when
/// there is no projection: the code that closes that block.
pub fn epsg_of_wkt(wkt: &str) -> Option<u32> {
    let block = |start: usize| -> Option<&str> {
        let open = start + wkt[start..].find('[')?;
        let mut depth = 0i32;
        for (i, c) in wkt[open..].char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&wkt[start..open + i + 1]);
                    }
                }
                _ => {}
            }
        }
        None
    };
    // The code at the end of a block is the block's own: the last one before its close.
    let last_code = |text: &str| -> Option<u32> {
        let mut found = None;
        for key in ["AUTHORITY[\"EPSG\",\"", "ID[\"EPSG\","] {
            let mut at = 0;
            while let Some(i) = text[at..].find(key) {
                let rest = &text[at + i + key.len()..];
                let digits: String = rest.trim_start_matches('"').chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(code) = digits.parse::<u32>() {
                    found = Some((at + i, code));
                }
                at += i + key.len();
            }
        }
        found.map(|f| f.1)
    };
    for key in ["PROJCRS[", "PROJCS[", "GEOGCRS[", "GEOGCS["] {
        if let Some(start) = wkt.find(key) {
            if let Some(code) = block(start).and_then(last_code) {
                return Some(code);
            }
        }
    }
    None
}

/// The coordinate system a point cloud says it is in, from WKT or GeoTIFF keys.
fn crs_of(header: &las::Header, opts: &LasOpts, notes: &mut Notes) -> Option<u32> {
    opts.crs.or_else(|| {
        let from_keys =
            header.get_geotiff_crs().ok().flatten().and_then(|g| g.get_projected_crs_geo_key_value()).filter(|c| *c >= 1024 && *c < 32767).map(u32::from);
        let from_wkt = header.get_wkt_crs_bytes().and_then(|b| epsg_of_wkt(&String::from_utf8_lossy(b)));
        let found = from_wkt.or(from_keys);
        if found.is_none() {
            notes.warn("The point cloud does not say its coordinate system. Choose it in the input, such as EPSG:3006 for SWEREF 99 TM.");
        }
        found
    })
}

/// The points as features, as they are read: thinned, by class and within the rectangle.
struct Collector<'a> {
    opts: &'a LasOpts,
    fields: Vec<Field>,
    colour: bool,
    /// Colours are 16 bits in the format, but most files keep 8 bits in them: those that go
    /// above 255 are made 8 bits.
    wide_colour: Option<bool>,
    features: Vec<Feature>,
    seen: u64,
    skipped_class: u64,
    cut_short: bool,
}

impl<'a> Collector<'a> {
    fn new(header: &las::Header, opts: &'a LasOpts) -> Self {
        let colour = header.point_format().has_color;
        let mut fields = vec![
            Field::new("classification", FieldType::Int),
            Field::new("class", FieldType::Text),
            Field::new("intensity", FieldType::Int),
            Field::new("return_number", FieldType::Int),
            Field::new("returns", FieldType::Int),
        ];
        if colour {
            for c in ["red", "green", "blue"] {
                fields.push(Field::new(c, FieldType::Int));
            }
        }
        Collector { opts, fields, colour, wide_colour: None, features: Vec::new(), seen: 0, skipped_class: 0, cut_short: false }
    }

    /// Takes the points it wants. False when no more may be taken.
    fn take(&mut self, points: Vec<las::Point>) -> bool {
        let every = self.opts.every.max(1) as u64;
        if self.colour && self.wide_colour.is_none() {
            self.wide_colour = Some(points.iter().filter_map(|p| p.color).any(|c| c.red > 255 || c.green > 255 || c.blue > 255));
        }
        for p in points {
            self.seen += 1;
            if !(self.seen - 1).is_multiple_of(every) {
                continue;
            }
            let class = u8::from(p.classification);
            if let Some(cl) = &self.opts.classes {
                if !cl.contains(&class) {
                    self.skipped_class += 1;
                    continue;
                }
            }
            if let Some(w) = self.opts.within {
                if p.x < w[0] || p.x > w[2] || p.y < w[1] || p.y > w[3] {
                    continue;
                }
            }
            if self.opts.most.is_some_and(|m| self.features.len() >= m) {
                self.cut_short = true;
                return false;
            }
            let mut attrs = vec![
                Value::Int(class as i64),
                Value::Text(class_name(class).to_string()),
                Value::Int(p.intensity as i64),
                Value::Int(p.return_number as i64),
                Value::Int(p.number_of_returns as i64),
            ];
            if self.colour {
                let c = p.color.unwrap_or_default();
                let shift = if self.wide_colour == Some(true) { 8 } else { 0 };
                attrs.extend([c.red, c.green, c.blue].map(|v| Value::Int((v >> shift) as i64)));
            }
            self.features.push(Feature { z: Some(Arc::from(vec![p.z])), attrs, geom: Some(geo::Geometry::Point(geo::Point::new(p.x, p.y))) });
        }
        true
    }

    fn finish(self, total: u64, crs: Option<u32>, notes: &mut Notes) -> Table {
        if self.cut_short {
            notes.warn(format!(
                "Only the first {} points were read, of {} in the file. Read every n-th point, only some classes or within a rectangle, or allow more.",
                fmt_count(self.features.len() as u64),
                fmt_count(total)
            ));
        }
        if self.skipped_class > 0 && self.features.is_empty() {
            notes.warn("No point has the classes asked for.");
        }
        notes.info(format!("{} of {} points", fmt_count(self.features.len() as u64), fmt_count(total)));
        Table { mesh: None, fields: self.fields, crs, features: self.features, raster: false, image: None, grid: None }
    }
}

/// Reads a point cloud. `going` is told how many points have been gone through, and stops
/// the reading when it says false. A COPC file is read through its octree when only part of
/// it is wanted: within a rectangle, or down to a spacing between the points.
pub fn read_las(path: &Path, opts: &LasOpts, notes: &mut Notes, going: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let named = |e: las::Error| GbError(format!("{} cannot be read as a point cloud: {}", path.display(), e));
    let mut reader = las::Reader::from_path(path).map_err(named)?;
    let header = reader.header().clone();
    if header.copc_info_vlr().is_some() && (opts.within.is_some() || opts.spacing.is_some()) {
        let file = std::fs::File::open(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
        return read_copc(std::io::BufReader::new(file), &path.display().to_string(), opts, notes);
    }
    if opts.spacing.is_some() {
        notes.info("The file is not COPC, so every level of detail is read: the spacing applies to COPC only");
    }
    let total = header.number_of_points();
    let crs = crs_of(&header, opts, notes);
    let mut collector = Collector::new(&header, opts);
    let mut data = las::PointDataBuilder::new().for_header(&header).build();
    let mut done = 0u64;
    loop {
        let n = reader.fill_points(100_000, &mut data).map_err(|e| GbError(format!("{}: {}", path.display(), e)))?;
        if n == 0 {
            break;
        }
        if !going(done, Some(total)) {
            return err(crate::model::STOPPED);
        }
        done += n;
        let points: Vec<las::Point> = data.points().collect::<std::result::Result<_, _>>().map_err(|e| GbError(format!("{}: {}", path.display(), e)))?;
        if !collector.take(points) {
            break;
        }
    }
    Ok(collector.finish(total, crs, notes))
}

/// A COPC point cloud read through its octree: only the nodes that reach into the
/// rectangle, and only the levels down to the spacing asked for.
pub fn read_copc<R: std::io::Read + std::io::Seek>(source: R, name: &str, opts: &LasOpts, notes: &mut Notes) -> Result<Table> {
    let mut reader = las::CopcReader::new(source).map_err(|e| GbError(format!("{} cannot be read as COPC: {}", name, e)))?;
    let header = reader.header().clone();
    let total = header.number_of_points();
    let crs = crs_of(&header, opts, notes);
    let levels = match opts.spacing {
        Some(s) if s > 0.0 => las::LodSelection::Resolution(s),
        _ => las::LodSelection::All,
    };
    let bounds = match opts.within {
        Some(w) => {
            las::BoundsSelection::Within(las::Bounds { min: las::Vector { x: w[0], y: w[1], z: f64::MIN }, max: las::Vector { x: w[2], y: w[3], z: f64::MAX } })
        }
        None => las::BoundsSelection::All,
    };
    let data = reader.query(levels, bounds).map_err(|e| GbError(format!("{}: {}", name, e)))?;
    let points: Vec<las::Point> = data.points().collect::<std::result::Result<_, _>>().map_err(|e| GbError(format!("{}: {}", name, e)))?;
    let mut collector = Collector::new(&header, opts);
    collector.take(points);
    Ok(collector.finish(total, crs, notes))
}

/// A file on the web read in pieces as it is asked for, so that only the parts that are
/// needed are fetched. The pieces are kept, so a part is fetched once.
pub struct Ranged<'a> {
    backend: &'a dyn crate::io::Backend,
    url: String,
    headers: Vec<(String, String)>,
    at: u64,
    len: Option<u64>,
    pieces: std::collections::HashMap<u64, Vec<u8>>,
    /// How many bytes were fetched, for the log.
    pub fetched: u64,
}

const PIECE: u64 = 256 * 1024;

impl<'a> Ranged<'a> {
    pub fn new(backend: &'a dyn crate::io::Backend, url: &str, headers: &[(String, String)]) -> Self {
        Ranged { backend, url: url.to_string(), headers: headers.to_vec(), at: 0, len: None, pieces: std::collections::HashMap::new(), fetched: 0 }
    }
    fn piece(&mut self, k: u64) -> std::io::Result<&Vec<u8>> {
        if !self.pieces.contains_key(&k) {
            let (bytes, total) = self.backend.fetch_range(&self.url, &self.headers, k * PIECE, PIECE).map_err(|e| std::io::Error::other(e.0))?;
            if total.is_some() {
                self.len = total;
            }
            self.fetched += bytes.len() as u64;
            self.pieces.insert(k, bytes);
        }
        Ok(&self.pieces[&k])
    }
}

impl std::io::Read for Ranged<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (k, off) = (self.at / PIECE, (self.at % PIECE) as usize);
        let piece = self.piece(k)?;
        if off >= piece.len() {
            return Ok(0);
        }
        let n = (piece.len() - off).min(buf.len());
        buf[..n].copy_from_slice(&piece[off..off + n]);
        self.at += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for Ranged<'_> {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.at = match to {
            std::io::SeekFrom::Start(n) => n,
            std::io::SeekFrom::Current(d) => self.at.saturating_add_signed(d),
            std::io::SeekFrom::End(d) => {
                if self.len.is_none() {
                    self.piece(0)?;
                }
                let len = self.len.ok_or_else(|| std::io::Error::other("The server does not say how large the file is."))?;
                len.saturating_add_signed(d)
            }
        };
        Ok(self.at)
    }
}

/// A COPC point cloud on the web, of which only what is needed is fetched.
pub fn read_copc_url(backend: &dyn crate::io::Backend, url: &str, headers: &[(String, String)], opts: &LasOpts, notes: &mut Notes) -> Result<Table> {
    let mut source = Ranged::new(backend, url, headers);
    let t = read_copc(&mut source, url, opts, notes)?;
    notes.info(format!("{} MB fetched", crate::model::fmt_real((source.fetched as f64 / 1e5).round() / 10.0)));
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_of_the_projection_is_found_in_wkt() {
        let wkt1 = r#"COMPD_CS["SWEREF99 TM + RH2000 height",PROJCS["SWEREF99 TM",GEOGCS["SWEREF99",DATUM["SWEREF99",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6619"]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433],AUTHORITY["EPSG","4619"]],PROJECTION["Transverse_Mercator"],UNIT["metre",1],AUTHORITY["EPSG","3006"]],VERT_CS["RH2000 height",VERT_DATUM["Rikets hojdsystem 2000",2005],UNIT["metre",1],AUTHORITY["EPSG","5613"]],AUTHORITY["EPSG","5845"]]"#;
        assert_eq!(epsg_of_wkt(wkt1), Some(3006));
        let wkt2 = r#"PROJCRS["SWEREF99 TM",BASEGEOGCRS["SWEREF99",DATUM["SWEREF99",ELLIPSOID["GRS 1980",6378137,298.257222101]],ID["EPSG",4619]],CONVERSION["SWEREF99 TM",METHOD["Transverse Mercator"]],CS[Cartesian,2],ID["EPSG",3006]]"#;
        assert_eq!(epsg_of_wkt(wkt2), Some(3006));
        assert_eq!(epsg_of_wkt(r#"GEOGCS["WGS 84",DATUM["WGS_1984"],AUTHORITY["EPSG","4326"]]"#), Some(4326));
        assert_eq!(epsg_of_wkt("nothing"), None);
    }
}
