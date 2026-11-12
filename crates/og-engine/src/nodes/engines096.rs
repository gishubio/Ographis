//! A real place for a game engine: Unreal, Unity or Godot. The terrain model becomes a
//! square heightmap of 16 bits in a size the engine takes, the areas masks for painting the
//! ground, such as forest, water, roads and fields, the points and buildings positions of
//! instances, such as trees, poles and houses, and the roads splines. Each in the units and
//! axes of the engine, from a corner of the heightmap, with a file that tells how to import it.

use super::*;
use crate::model::{err, fmt_count, fmt_real, Feature, Field, FieldType, GbError, Result, Table, Value};
use geo::{Area, Centroid, Geometry};

const ENGINES: &[(&str, &str)] = &[("unreal", "Unreal Engine"), ("unity", "Unity"), ("godot", "Godot")];

/// The sizes of heightmap each engine takes, in pixels on a side.
fn sizes(engine: Engine) -> &'static [usize] {
    match engine {
        // Landscapes of whole components, as the import of Unreal recommends.
        Engine::Unreal => &[127, 253, 505, 1009, 2017, 4033, 8129],
        // A power of two and one.
        Engine::Unity | Engine::Godot => &[129, 257, 513, 1025, 2049, 4097, 8193],
    }
}

const SIZES: &[(&str, &str)] = &[
    ("auto", "As fine as the terrain model, up to the largest"),
    ("small", "About 500 pixels"),
    ("medium", "About 1000 pixels"),
    ("large", "About 2000 pixels"),
    ("huge", "About 4000 pixels"),
];

const PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "engine", label: "Engine", kind: ParamKind::Choice(ENGINES), default: "unreal", hint: "The units, axes and sizes follow the engine" },
    ParamSpec {
        key: "folder",
        label: "Folder",
        kind: ParamKind::Text,
        default: "game",
        hint: "Where the files go, beside the model or a whole path: heightmap.png and .r16, masks, instances.csv, roads.json and IMPORT.txt",
    },
    ParamSpec {
        key: "size",
        label: "Size of the heightmap",
        kind: ParamKind::Choice(SIZES),
        default: "auto",
        hint: "The nearest size the engine takes. The heightmap is square, over the whole terrain model; outside it, the lowest height",
    },
    ParamSpec {
        key: "mask_by",
        label: "Masks by",
        kind: ParamKind::Attribute("Areas"),
        default: "",
        hint: "The attribute of the areas that names their kind, such as forest, water or field: a mask for each kind. Empty: one mask of all the areas",
    },
    ParamSpec {
        key: "kind_by",
        label: "Instances by",
        kind: ParamKind::Attribute("Instances"),
        default: "",
        hint: "The attribute that names what stands there, such as tree, pole or house. Empty: instance",
    },
    ParamSpec {
        key: "height_by",
        label: "Height of instances",
        kind: ParamKind::Attribute("Instances"),
        default: "",
        hint: "Such as the height of a building or a tree, in metres, for its scale. Empty: none",
    },
    ParamSpec {
        key: "road_width",
        label: "Width of roads",
        kind: ParamKind::Expression("Roads"),
        default: "6",
        hint: "In metres: a number, or an attribute such as the width in NVDB. For the road mask and the splines",
    },
    ParamSpec {
        key: "road_name",
        label: "Name of roads",
        kind: ParamKind::Attribute("Roads"),
        default: "",
        hint: "An attribute for the name of each spline. Empty: road and its number",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Unreal,
    Unity,
    Godot,
}

/// Where a heightmap lies: the upper left (north-west) corner, the distance between two of
/// its pixels, and its lowest and highest height.
struct Frame {
    engine: Engine,
    west: f64,
    north: f64,
    cell: f64,
    size: usize,
    zmin: f64,
    zmax: f64,
}

impl Frame {
    /// The side of the heightmap in metres.
    fn side(&self) -> f64 {
        self.cell * (self.size - 1) as f64
    }

    /// A place in the engine: from metres east of the west edge, south of the north edge and
    /// above the lowest height, in its units and axes.
    fn place(&self, x: f64, y: f64, z: f64) -> [f64; 3] {
        let (e, s, h) = (x - self.west, self.north - y, z - self.zmin);
        match self.engine {
            // Centimetres; X east, Y south, Z up.
            Engine::Unreal => [e * 100.0, s * 100.0, h * 100.0],
            // Metres; X east, Y up, Z north, from the south-west corner of the terrain.
            Engine::Unity => [e, h, self.side() - s],
            // Metres; X east, Y up, Z south.
            Engine::Godot => [e, h, s],
        }
    }

    /// A turn about the up axis in the engine, in degrees, for a direction on the map in
    /// degrees counterclockwise from east.
    fn yaw(&self, map_degrees: f64) -> f64 {
        let y = match self.engine {
            Engine::Unreal | Engine::Unity => -map_degrees,
            Engine::Godot => map_degrees,
        };
        (y + 540.0).rem_euclid(360.0) - 180.0
    }

    /// Whether the rows of a picture go from the south, as Unity reads a heightmap.
    fn rows_from_south(&self) -> bool {
        self.engine == Engine::Unity
    }
}

fn run_engine(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let heights = input(inp, "Heights");
    let Some(grid) = heights.grid.as_deref() else {
        return err("Nothing came in on Heights. Connect a terrain model, read by an Input with Cells as heights on a grid, such as a GeoTIFF of Lantmäteriet's elevation model.");
    };
    if grid.is_turned() {
        return err("The terrain model is turned. Warp it to rows that run east and west first.");
    }
    if ctx.geographic(grid.crs) == Some(true) {
        return err("The terrain model is in degrees. Reproject it to metres first, such as to SWEREF 99 TM (EPSG:3006).");
    }
    let engine = match p.get("engine").as_deref() {
        Some("unity") => Engine::Unity,
        Some("godot") => Engine::Godot,
        _ => Engine::Unreal,
    };
    let g = grid.geotransform;
    let (width_m, height_m) = (grid.width as f64 * g[1].abs(), grid.height as f64 * g[5].abs());
    let side = width_m.max(height_m);
    let wanted = match p.get("size").as_deref() {
        Some("small") => 500,
        Some("medium") => 1000,
        Some("large") => 2000,
        Some("huge") => 4000,
        _ => (side / g[1].abs().min(g[5].abs())).ceil() as usize + 1,
    };
    let list = sizes(engine);
    let size = list.iter().copied().min_by_key(|s| (*s as i64 - wanted as i64).abs()).expect("sizes");
    // As fine as the terrain model: the smallest that is as fine, or the largest.
    let size =
        if p.get("size").is_none_or(|s| s == "auto") { list.iter().copied().find(|s| *s >= wanted).unwrap_or(*list.last().expect("sizes")) } else { size };
    let west = g[0];
    let north = g[3];
    let cell = side / (size - 1) as f64;
    // The heights at the pixels, from the terrain model between its cells.
    let mut z: Vec<f32> = vec![f32::NAN; size * size];
    {
        use rayon::prelude::*;
        z.par_chunks_mut(size).enumerate().for_each(|(row, line)| {
            for (col, v) in line.iter_mut().enumerate() {
                let (x, y) = (west + col as f64 * cell, north - row as f64 * cell);
                if let Some(h) = grid.bilinear(x, y).filter(|h| h.is_finite()) {
                    *v = h;
                }
            }
        });
    }
    let (zmin, zmax) = z.iter().filter(|v| v.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if !zmin.is_finite() {
        return err("The terrain model has no heights.");
    }
    let empty = z.iter().filter(|v| !v.is_finite()).count();
    let frame = Frame { engine, west, north, cell, size, zmin: zmin as f64, zmax: (zmax as f64).max(zmin as f64 + 0.01) };
    let folder = ctx.resolve(&p.get("folder").filter(|f| !f.trim().is_empty()).unwrap_or_else(|| "game".into()));
    std::fs::create_dir_all(folder.join("masks")).map_err(|e| GbError(format!("Cannot create the folder {}: {}", folder.display(), e)))?;
    let range = frame.zmax - frame.zmin;
    let row_of = |r: usize| if frame.rows_from_south() { size - 1 - r } else { r };

    // The heightmap: 16 bits, the lowest height 0 and the highest 65535.
    let mut pixels = vec![0u16; size * size];
    for r in 0..size {
        for c in 0..size {
            let v = z[r * size + c];
            let v = if v.is_finite() { v } else { zmin };
            pixels[row_of(r) * size + c] = (((v - zmin) as f64 / range) * 65535.0).round().clamp(0.0, 65535.0) as u16;
        }
    }
    let png = folder.join("heightmap.png");
    image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(size as u32, size as u32, pixels.clone())
        .ok_or_else(|| GbError("The heightmap could not be made.".into()))?
        .save(&png)
        .map_err(|e| GbError(format!("Cannot write {}: {}", png.display(), e)))?;
    let raw = folder.join("heightmap.r16");
    let bytes: Vec<u8> = pixels.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(&raw, bytes).map_err(|e| GbError(format!("Cannot write {}: {}", raw.display(), e)))?;
    ctx.files.push(png);
    ctx.files.push(raw);
    ctx.notes.info(format!(
        "a heightmap of {} × {} pixels, {} m apart, over {} × {} m, from {} to {} m",
        size,
        size,
        fmt_real((cell * 100.0).round() / 100.0),
        fmt_real(side.round()),
        fmt_real(side.round()),
        fmt_real((frame.zmin * 10.0).round() / 10.0),
        fmt_real((frame.zmax * 10.0).round() / 10.0)
    ));
    if empty > 0 || (width_m - height_m).abs() > cell {
        ctx.notes.info("the heightmap is square, so where the terrain model does not reach it lies at the lowest height");
    }

    // The masks: white where the kind lies, with soft edges.
    let mut masks: Vec<String> = Vec::new();
    let areas = input(inp, "Areas");
    if !areas.is_empty() {
        let by = p.get("mask_by").filter(|b| !b.trim().is_empty()).map(|b| areas.need_field(&b)).transpose()?;
        let mut kinds: Vec<(String, Vec<&Geometry<f64>>)> = Vec::new();
        for f in &areas.features {
            let Some(geom) = &f.geom else { continue };
            let name = by.map(|i| f.attrs[i].to_text()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "areas".into());
            match kinds.iter_mut().find(|(k, _)| *k == name) {
                Some((_, list)) => list.push(geom),
                None => kinds.push((name, vec![geom])),
            }
        }
        if kinds.len() > 32 {
            ctx.notes.warn(format!("There are {} kinds of area; masks were made for the 32 largest.", kinds.len()));
            kinds.sort_by(|a, b| {
                let area = |l: &Vec<&Geometry<f64>>| l.iter().map(|g| g.unsigned_area()).sum::<f64>();
                area(&b.1).total_cmp(&area(&a.1))
            });
            kinds.truncate(32);
        }
        for (name, geoms) in &kinds {
            let mask = paint(&frame, geoms.iter().map(|g| (*g, None)));
            masks.push(write_mask(&frame, &folder, name, &mask, ctx)?);
        }
    }

    // The roads: a mask, and a spline for each.
    let roads = input(inp, "Roads");
    let mut splines: Vec<serde_json::Value> = Vec::new();
    if !roads.is_empty() {
        let names = roads.field_names();
        let width_text = p.get("road_width").filter(|w| !w.trim().is_empty()).unwrap_or_else(|| "6".into());
        let width = expr::compile(&width_text, &names).map_err(|e| GbError(format!("The width of roads: {}", e.0)))?;
        let name_at = p.get("road_name").filter(|n| !n.trim().is_empty()).map(|n| roads.need_field(&n)).transpose()?;
        let mut painted: Vec<(&Geometry<f64>, Option<f64>)> = Vec::new();
        for (i, f) in roads.features.iter().enumerate() {
            let Some(geom) = &f.geom else { continue };
            let w = width.eval(&expr::Row { feature: f, index: i, geographic: Some(false) }).map_err(|e| at_row(e, i))?;
            let w = w.as_f64().or_else(|| crate::model::parse_number(&w.to_text())).filter(|w| *w > 0.0).unwrap_or(6.0);
            painted.push((geom, Some(w)));
            let lines: Vec<&geo::LineString<f64>> = match geom {
                Geometry::LineString(l) => vec![l],
                Geometry::MultiLineString(m) => m.0.iter().collect(),
                _ => Vec::new(),
            };
            for (k, l) in lines.into_iter().enumerate() {
                let points: Vec<[f64; 3]> =
                    l.0.iter()
                        .map(|c| {
                            let h = grid.bilinear(c.x, c.y).filter(|h| h.is_finite()).map(|h| h as f64).unwrap_or(frame.zmin);
                            frame.place(c.x, c.y, h).map(|v| (v * 1000.0).round() / 1000.0)
                        })
                        .collect();
                if points.len() < 2 {
                    continue;
                }
                let base = name_at.map(|n| f.attrs[n].to_text()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("road_{}", i + 1));
                let name = if k > 0 { format!("{}_{}", base, k + 1) } else { base };
                let units = if engine == Engine::Unreal { w * 100.0 } else { w };
                splines.push(serde_json::json!({"name": name, "width": units, "points": points}));
            }
        }
        let mask = paint(&frame, painted.into_iter());
        masks.push(write_mask(&frame, &folder, "roads", &mask, ctx)?);
        let path = folder.join("roads.json");
        let text = serde_json::to_string_pretty(&serde_json::json!({"units": units_of(engine), "axes": axes_of(engine), "roads": splines}))
            .map_err(|e| GbError(e.to_string()))?;
        std::fs::write(&path, text).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        ctx.files.push(path);
    }
    if !masks.is_empty() {
        ctx.notes.info(format!("masks: {}", masks.join(", ")));
    }

    // The instances: where each stands, how it is turned, and how large it is.
    let things = input(inp, "Instances");
    let mut out = Table {
        fields: vec![
            Field::new("Name", FieldType::Text),
            Field::new("kind", FieldType::Text),
            Field::new("x", FieldType::Real),
            Field::new("y", FieldType::Real),
            Field::new("z", FieldType::Real),
            Field::new("yaw", FieldType::Real),
            Field::new("width", FieldType::Real),
            Field::new("length", FieldType::Real),
            Field::new("height", FieldType::Real),
        ],
        features: Vec::new(),
        crs: things.crs,
        ..Table::default()
    };
    if !things.is_empty() {
        let kind_at = p.get("kind_by").filter(|k| !k.trim().is_empty()).map(|k| things.need_field(&k)).transpose()?;
        let height_at = p.get("height_by").filter(|k| !k.trim().is_empty()).map(|k| things.need_field(&k)).transpose()?;
        let scale = if engine == Engine::Unreal { 100.0 } else { 1.0 };
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut outside = 0usize;
        for f in &things.features {
            let Some(geom) = &f.geom else { continue };
            let Some(c) = geom.centroid() else { continue };
            let (x, y) = (c.x(), c.y());
            if x < west || x > west + frame.side() || y > north || y < north - frame.side() {
                outside += 1;
                continue;
            }
            let ground = grid.bilinear(x, y).filter(|h| h.is_finite()).map(|h| h as f64).unwrap_or(frame.zmin);
            // A building: turned along its longest side, as wide and long as it is.
            let (yaw, wide, long) = match geom {
                Geometry::Polygon(_) | Geometry::MultiPolygon(_) => turn_and_size(geom),
                _ => (0.0, 0.0, 0.0),
            };
            let kind = kind_at.map(|k| f.attrs[k].to_text()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "instance".into());
            let n = counts.entry(kind.clone()).or_insert(0);
            *n += 1;
            let tall = height_at.and_then(|h| f.attrs[h].as_f64().or_else(|| crate::model::parse_number(&f.attrs[h].to_text()))).unwrap_or(0.0);
            let at = frame.place(x, y, ground);
            let round = |v: f64| Value::Real((v * 1000.0).round() / 1000.0);
            out.features.push(Feature {
                z: None,
                attrs: vec![
                    Value::Text(format!("{}_{}", kind, n)),
                    Value::Text(kind),
                    round(at[0]),
                    round(at[1]),
                    round(at[2]),
                    round(frame.yaw(yaw)),
                    round(wide * scale),
                    round(long * scale),
                    round(tall * scale),
                ],
                geom: Some(Geometry::Point(c)),
            });
        }
        if outside > 0 {
            ctx.notes.warn(format!("{} instances lie outside the heightmap and were left out.", fmt_count(outside as u64)));
        }
        let path = folder.join("instances.csv");
        let mut csv = String::from("Name,kind,x,y,z,yaw,width,length,height\n");
        for f in &out.features {
            let cells: Vec<String> = f.attrs.iter().map(|v| v.to_text().replace(',', " ")).collect();
            csv.push_str(&cells.join(","));
            csv.push('\n');
        }
        std::fs::write(&path, csv).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        ctx.files.push(path);
        let mut kinds: Vec<_> = counts.into_iter().collect();
        kinds.sort();
        ctx.notes.info(format!("instances: {}", kinds.iter().map(|(k, n)| format!("{} {}", fmt_count(*n as u64), k)).collect::<Vec<_>>().join(", ")));
    }

    // How to bring it into the engine.
    let path = folder.join("IMPORT.txt");
    std::fs::write(&path, import_notes(&frame, &masks, !splines.is_empty(), !out.is_empty(), grid.crs))
        .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    ctx.files.push(path);
    // The heightmap as a grid, to see on the map and in 3D.
    let mut hm = crate::model::Grid::blank(size, size, [west - cell / 2.0, cell, 0.0, north + cell / 2.0, 0.0, -cell], grid.crs);
    hm.values = z.iter().map(|v| if v.is_finite() { *v } else { zmin }).collect();
    let heightmap = Table { crs: grid.crs, grid: Some(std::sync::Arc::new(hm)), raster: true, ..Table::default() };
    Ok(vec![("Heightmap".into(), heightmap), ("Instances".into(), out)])
}

fn units_of(engine: Engine) -> &'static str {
    match engine {
        Engine::Unreal => "centimetres",
        _ => "metres",
    }
}

fn axes_of(engine: Engine) -> &'static str {
    match engine {
        Engine::Unreal => "X east, Y south, Z up, from the north-west corner of the landscape",
        Engine::Unity => "X east, Y up, Z north, from the south-west corner of the terrain",
        Engine::Godot => "X east, Y up, Z south, from the north-west corner of the terrain",
    }
}

/// How a building is turned, in degrees counterclockwise from east along its longest side,
/// and how wide and long it is.
fn turn_and_size(g: &Geometry<f64>) -> (f64, f64, f64) {
    use geo::MinimumRotatedRect;
    let Some(rect) = g.minimum_rotated_rect() else { return (0.0, 0.0, 0.0) };
    let c = &rect.exterior().0;
    if c.len() < 4 {
        return (0.0, 0.0, 0.0);
    }
    let a = ((c[1].x - c[0].x).powi(2) + (c[1].y - c[0].y).powi(2)).sqrt();
    let b = ((c[2].x - c[1].x).powi(2) + (c[2].y - c[1].y).powi(2)).sqrt();
    let (from, to, long, wide) = if a >= b { (c[0], c[1], a, b) } else { (c[1], c[2], b, a) };
    let mut deg = (to.y - from.y).atan2(to.x - from.x).to_degrees();
    // A building looks the same turned half a turn; keep it between -90 and 90.
    if deg > 90.0 {
        deg -= 180.0;
    } else if deg <= -90.0 {
        deg += 180.0;
    }
    (deg, wide, long)
}

/// A mask of the heightmap's size: 0 to 255 by how much of each pixel the geometries cover.
/// Lines are drawn as wide as given, in metres.
fn paint<'g>(frame: &Frame, geoms: impl Iterator<Item = (&'g Geometry<f64>, Option<f64>)>) -> Vec<u8> {
    use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
    let n = frame.size as u32;
    let Some(mut pix) = Pixmap::new(n, n) else { return vec![0; frame.size * frame.size] };
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    // A pixel stands for the place of its middle.
    let to_px = |x: f64, y: f64| (((x - frame.west) / frame.cell + 0.5) as f32, ((frame.north - y) / frame.cell + 0.5) as f32);
    let ring = |pb: &mut PathBuilder, l: &geo::LineString<f64>, close: bool| {
        for (k, c) in l.0.iter().enumerate() {
            let (x, y) = to_px(c.x, c.y);
            if k == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if close {
            pb.close();
        }
    };
    for (g, width) in geoms {
        let mut pb = PathBuilder::new();
        let mut lines = false;
        let polys: Vec<&geo::Polygon<f64>> = match g {
            Geometry::Polygon(p) => vec![p],
            Geometry::MultiPolygon(m) => m.0.iter().collect(),
            Geometry::LineString(l) => {
                ring(&mut pb, l, false);
                lines = true;
                Vec::new()
            }
            Geometry::MultiLineString(m) => {
                for l in &m.0 {
                    ring(&mut pb, l, false);
                }
                lines = true;
                Vec::new()
            }
            _ => Vec::new(),
        };
        for p in polys {
            ring(&mut pb, p.exterior(), true);
            for hole in p.interiors() {
                ring(&mut pb, hole, true);
            }
        }
        let Some(path) = pb.finish() else { continue };
        if lines {
            let w = (width.unwrap_or(6.0) / frame.cell) as f32;
            let stroke = Stroke { width: w.max(0.5), line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
            pix.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        } else {
            pix.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
        }
    }
    pix.pixels().iter().map(|p| p.alpha()).collect()
}

/// Writes a mask as a grey PNG, rows as the engine reads them. Gives its file name.
fn write_mask(frame: &Frame, folder: &std::path::Path, kind: &str, mask: &[u8], ctx: &mut NodeCtx) -> Result<String> {
    let n = frame.size;
    let mut rows = vec![0u8; n * n];
    for r in 0..n {
        let to = if frame.rows_from_south() { n - 1 - r } else { r };
        rows[to * n..(to + 1) * n].copy_from_slice(&mask[r * n..(r + 1) * n]);
    }
    // Plain letters, as some engines take no others in the names of files.
    let safe: String = kind
        .chars()
        .map(|c| match c {
            'å' | 'ä' | 'à' | 'á' => 'a',
            'Å' | 'Ä' => 'A',
            'ö' | 'ø' | 'ó' => 'o',
            'Ö' | 'Ø' => 'O',
            'é' | 'è' => 'e',
            'ü' => 'u',
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => c,
            _ => '_',
        })
        .collect();
    let name = format!("mask_{}.png", safe);
    let path = folder.join("masks").join(&name);
    image::GrayImage::from_raw(n as u32, n as u32, rows)
        .ok_or_else(|| GbError("The mask could not be made.".into()))?
        .save(&path)
        .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    ctx.files.push(path);
    Ok(name)
}

/// What to set when the files are imported, for the engine.
fn import_notes(f: &Frame, masks: &[String], roads: bool, instances: bool, crs: Option<u32>) -> String {
    let side = f.side();
    let range = f.zmax - f.zmin;
    let mut t = String::new();
    let crs = crs.map(|c| format!("EPSG:{}", c)).unwrap_or_else(|| "unknown".into());
    t.push_str(&format!(
        "Made by Ographis {}.\n\nThe heightmap: {} × {} pixels, {} m apart, {} m on a side.\nIts north-west corner lies at x {} and y {} in {}.\nHeights from {} m (black, 0) to {} m (white, 65535): {} m.\n\n",
        env!("CARGO_PKG_VERSION"),
        f.size,
        f.size,
        r3(f.cell),
        r3(side),
        r3(f.west),
        r3(f.north),
        crs,
        r3(f.zmin),
        r3(f.zmax),
        r3(range)
    ));
    match f.engine {
        Engine::Unreal => {
            // Unreal reads 65535 as 512 units of the Z scale above -256: a Z scale of 100 is
            // 512 m from black to white.
            let zscale = range * 100.0 / 512.0;
            t.push_str(&format!(
                "Unreal Engine\n\
                 Landscape mode, Import from File: heightmap.png (or heightmap.r16).\n\
                 Scale: X {xy}, Y {xy}, Z {z}.\n\
                 Location: X 0, Y 0, Z {loc}, so that the lowest height lies at Z 0.\n\
                 Units: centimetres. X runs east, Y south, Z up.\n",
                xy = r3(f.cell * 100.0),
                z = r3(zscale),
                loc = r3(256.0 * zscale)
            ));
            if !masks.is_empty() {
                t.push_str("Masks: in the Paint tab, each layer of the landscape material, Import from file, the mask of that kind.\n");
            }
        }
        Engine::Unity => {
            t.push_str(&format!(
                "Unity\n\
                 Terrain, Terrain Settings, Import Raw: heightmap.r16, Depth 16 bit, Byte Order Windows, Flip Vertically off.\n\
                 Resolution {n}, Terrain Size: width {side}, length {side}, height {range}.\n\
                 Position: 0, 0, 0, so that the lowest height lies at Y 0.\n\
                 Units: metres. X runs east, Y up, Z north. The rows of the files run from the south.\n",
                n = f.size,
                side = r3(side),
                range = r3(range)
            ));
            if !masks.is_empty() {
                t.push_str("Masks: as the alpha map of a terrain layer, by a script or a tool for splat maps.\n");
            }
        }
        Engine::Godot => {
            t.push_str(&format!(
                "Godot\n\
                 Terrain3D: Import heightmap.png, Scale {cell}, Height range {range}, Height offset 0.\n\
                 HTerrain: Import heightmap.r16, Min height 0, Max height {range}, cell size {cell}.\n\
                 Units: metres. X runs east, Y up, Z south.\n",
                cell = r3(f.cell),
                range = r3(range)
            ));
            if !masks.is_empty() {
                t.push_str("Masks: as the control or splat map of the terrain, one kind of texture each.\n");
            }
        }
    }
    if !masks.is_empty() {
        t.push_str(&format!("\nThe masks, in masks/: {}. White where the kind lies, with soft edges.\n", masks.join(", ")));
    }
    if instances {
        t.push_str("\ninstances.csv: Name, kind, x, y, z, yaw in degrees about the up axis, width, length and height, in the units of the engine. A Data Table in Unreal, or read by a script, to place one model of each kind as instances.\n");
    }
    if roads {
        t.push_str("\nroads.json: each road as a spline, with its name, width and points on the ground, in the units of the engine.\n");
    }
    t
}

fn r3(v: f64) -> String {
    fmt_real((v * 1000.0).round() / 1000.0)
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "GameEngineExporter",
    title: "Real place for a game engine",
    hidden: false,
    group: Group::Transformer,
    category: "3D models and views",
    desc: "Makes a real place ready for Unreal Engine, Unity or Godot. The terrain model on Heights becomes a square heightmap of 16 bits, as PNG and RAW, in a size the engine takes. The areas on Areas become masks for painting the ground, one for each kind, such as forest, water and fields. Trees, poles and buildings on Instances become a list of positions, each turned and sized, to place as instances. The roads on Roads become splines on the ground, and a mask of their width. Each in the units and axes of the engine, from a corner of the heightmap, with IMPORT.txt, which tells the scale and place to set when importing.",
    inputs: &["Heights", "Areas", "Instances", "Roads"],
    outputs: &["Heightmap", "Instances"],
    ports: None,
    params: PARAMS,
    run: run_engine,
    summary: |p| format!("for {}", choice_label(ENGINES, &p.get("engine").unwrap_or_else(|| "unreal".into()))),
}];

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry<f64> {
        Geometry::Polygon(geo::Polygon::new(geo::LineString::from(vec![(x0, y0), (x0, y1), (x1, y1), (x1, y0), (x0, y0)]), vec![]))
    }

    #[test]
    fn places_and_turns_follow_the_axes_of_each_engine() {
        let frame = |engine| Frame { engine, west: 1000.0, north: 2000.0, cell: 1.0, size: 101, zmin: 50.0, zmax: 80.0 };
        // 10 m east, 20 m south and 5 m above the lowest.
        assert_eq!(frame(Engine::Unreal).place(1010.0, 1980.0, 55.0), [1000.0, 2000.0, 500.0]);
        assert_eq!(frame(Engine::Unity).place(1010.0, 1980.0, 55.0), [10.0, 5.0, 80.0]);
        assert_eq!(frame(Engine::Godot).place(1010.0, 1980.0, 55.0), [10.0, 5.0, 20.0]);
        // Turned towards the north-east.
        assert_eq!(frame(Engine::Unreal).yaw(45.0), -45.0);
        assert_eq!(frame(Engine::Godot).yaw(45.0), 45.0);
        let house = square(0.0, 0.0, 6.0, 12.0);
        let (deg, wide, long) = turn_and_size(&house);
        assert_eq!(((deg.abs() - 90.0).abs() < 1e-6, (wide - 6.0).abs() < 1e-6, (long - 12.0).abs() < 1e-6), (true, true, true));
    }

    #[test]
    fn masks_cover_what_lies_there() {
        let frame = Frame { engine: Engine::Godot, west: 0.0, north: 100.0, cell: 1.0, size: 101, zmin: 0.0, zmax: 1.0 };
        let field = square(10.0, 10.0, 50.0, 50.0);
        let mask = paint(&frame, std::iter::once((&field, None)));
        // Inside: x 30, y 30 is column 30, row 70. Outside: row 20.
        assert_eq!(mask[70 * 101 + 30], 255);
        assert_eq!(mask[20 * 101 + 30], 0);
    }
}
