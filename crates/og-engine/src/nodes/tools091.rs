//! Tools added in 0.91: Create raster, Focal statistics and Cost distance for rasters, Thin
//! point cloud, and Stylize picture, which draws a picture in the style of the Commodore 64 or
//! as a mosaic of LEGO plates.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Grid, RasterImage};
use geo::Geometry;

fn grid_on<'a>(t: &'a Table, port: &str) -> Result<&'a Grid> {
    match t.grid.as_deref() {
        Some(g) if g.is_turned() => err(format!("The raster on {} is turned, which this tool cannot work with yet.", port)),
        Some(g) => Ok(g),
        None => err(format!("The stream on {} has no grid of values. Read a GeoTIFF with Cells as set to Heights or other values on a grid.", port)),
    }
}

fn note_grid(g: &Grid, ctx: &mut NodeCtx) {
    match g.range() {
        Some((lo, hi)) => ctx.notes.info(format!(
            "{} × {} cells of {}, values from {} to {}",
            g.width,
            g.height,
            fmt_real(g.cell_size().0),
            fmt_real((lo as f64 * 1000.0).round() / 1000.0),
            fmt_real((hi as f64 * 1000.0).round() / 1000.0)
        )),
        None => ctx.notes.warn("Every cell of the result is empty."),
    }
}

// ---------- Create raster (RasterCreator) ----------

const CREATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "bbox",
        label: "Rectangle",
        kind: ParamKind::Text,
        default: "",
        hint: "West, south, east, north in the coordinate system below. Not needed when something comes in on Extent, whose extent is used",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "EPSG:3006", hint: "Of the rectangle. With something on Extent, its own" },
    ParamSpec { key: "cell", label: "Cell size", kind: ParamKind::Text, default: "10", hint: "In the units of the coordinate system, such as metres" },
    ParamSpec {
        key: "value",
        label: "Value",
        kind: ParamKind::Text,
        default: "0",
        hint: "Of each cell: a number, or an expression with x and y, the middle of the cell, and row and column, such as rand() * 10 or sqrt((x - 319000)^2 + (y - 6400000)^2)",
    },
];

fn run_create(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let extent = input(inp, "Extent");
    let (rect, crs) = if extent.has_geometry() {
        use geo::BoundingRect;
        let mut r = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for g in extent.features.iter().filter_map(|f| f.geom.as_ref()).filter_map(|g| g.bounding_rect()) {
            r = [r[0].min(g.min().x), r[1].min(g.min().y), r[2].max(g.max().x), r[3].max(g.max().y)];
        }
        (r, extent.crs)
    } else {
        let text = p.need("bbox", "Write the rectangle as west, south, east, north, or connect something to Extent.")?;
        let v: Vec<f64> = text.split([',', ';', ' ']).filter(|t| !t.trim().is_empty()).filter_map(|t| crate::model::parse_number(t.trim())).collect();
        if v.len() != 4 || v[0] >= v[2] || v[1] >= v[3] {
            return err(format!("\"{}\" is not a rectangle. Write west, south, east, north.", text));
        }
        ([v[0], v[1], v[2], v[3]], p.crs("crs")?)
    };
    let size = number_param(p, "cell", "Cell size")?.unwrap_or(10.0);
    if size <= 0.0 {
        return err("The cell size must be more than 0.");
    }
    let left = (rect[0] / size).floor() * size;
    let top = (rect[3] / size).ceil() * size;
    let w = ((rect[2] - left) / size).ceil().max(1.0) as usize;
    let h = ((top - rect[1]) / size).ceil().max(1.0) as usize;
    if w * h > GRID_CELLS {
        return err(format!("That would be {} × {} cells, more than a raster may have. Choose larger cells.", fmt_count(w as u64), fmt_count(h as u64)));
    }
    let text = p.get("value").filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "0".into());
    let names: Vec<String> = ["x", "y", "row", "column"].iter().map(|s| s.to_string()).collect();
    let e = crate::expr::compile(&text, &names).map_err(|e| GbError(format!("Value: {}", e.0)))?;
    let mut values = Vec::with_capacity(w * h);
    let mut f = Feature::new(vec![Value::Null; 4], None);
    for r in 0..h {
        for c in 0..w {
            f.attrs[0] = Value::Real(left + (c as f64 + 0.5) * size);
            f.attrs[1] = Value::Real(top - (r as f64 + 0.5) * size);
            f.attrs[2] = Value::Int(r as i64);
            f.attrs[3] = Value::Int(c as i64);
            let v = e.eval(&crate::expr::Row { feature: &f, index: r * w + c, geographic: None })?;
            values.push(match v {
                Value::Bool(b) => b as u8 as f32,
                v => v.as_f64().map_or(f32::NAN, |x| x as f32),
            });
        }
    }
    let g = Grid { width: w, height: h, values, geotransform: [left, size, 0.0, top, 0.0, -size], crs, bands: Vec::new() };
    note_grid(&g, ctx);
    Ok(vec![("Raster".into(), g.into_table())])
}

// ---------- Focal statistics (FocalStatistics) ----------

const FOCAL_STATS: &[(&str, &str)] = &[
    ("mean", "Mean"),
    ("min", "Smallest"),
    ("max", "Largest"),
    ("sum", "Sum"),
    ("median", "Median"),
    ("std", "Spread (standard deviation)"),
    ("range", "Range: largest less smallest"),
    ("majority", "Most common value"),
    ("count", "How many cells have a value"),
];
const FOCAL_SHAPES: &[(&str, &str)] = &[("square", "Square"), ("circle", "Circle")];

const FOCAL_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "stat", label: "Each cell gets", kind: ParamKind::Choice(FOCAL_STATS), default: "mean", hint: "Of the cells around it, itself included" },
    ParamSpec { key: "radius", label: "Cells around", kind: ParamKind::Text, default: "1", hint: "How many cells out: 1 is a window of 3 × 3, 2 of 5 × 5" },
    ParamSpec { key: "shape", label: "Window", kind: ParamKind::Choice(FOCAL_SHAPES), default: "square", hint: "" },
];

/// Each cell from the cells within `r` of it: a square window, or a circle.
pub fn focal(g: &Grid, r: usize, stat: &str, circle: bool) -> Grid {
    use rayon::prelude::*;
    let (w, h) = (g.width, g.height);
    let ri = r as isize;
    let values: Vec<f32> = (0..w * h)
        .into_par_iter()
        .map_init(Vec::new, |vals: &mut Vec<f32>, i| {
            let (c, row) = ((i % w) as isize, (i / w) as isize);
            vals.clear();
            for dr in -ri..=ri {
                let rr = row + dr;
                if rr < 0 || rr >= h as isize {
                    continue;
                }
                for dc in -ri..=ri {
                    let cc = c + dc;
                    if cc < 0 || cc >= w as isize || (circle && dr * dr + dc * dc > ri * ri) {
                        continue;
                    }
                    let v = g.values[rr as usize * w + cc as usize];
                    if !v.is_nan() {
                        vals.push(v);
                    }
                }
            }
            if vals.is_empty() {
                return if stat == "count" { 0.0 } else { f32::NAN };
            }
            let n = vals.len() as f64;
            let sum: f64 = vals.iter().map(|v| *v as f64).sum();
            match stat {
                "min" => vals.iter().copied().fold(f32::INFINITY, f32::min),
                "max" => vals.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                "sum" => sum as f32,
                "count" => n as f32,
                "range" => vals.iter().copied().fold(f32::NEG_INFINITY, f32::max) - vals.iter().copied().fold(f32::INFINITY, f32::min),
                "std" => {
                    let m = sum / n;
                    (vals.iter().map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / n).sqrt() as f32
                }
                "median" | "majority" => {
                    vals.sort_by(f32::total_cmp);
                    if stat == "median" {
                        let k = vals.len() / 2;
                        if vals.len() % 2 == 1 {
                            vals[k]
                        } else {
                            (vals[k - 1] + vals[k]) / 2.0
                        }
                    } else {
                        let (mut best, mut best_n, mut i) = (vals[0], 0, 0);
                        while i < vals.len() {
                            let run = vals[i..].iter().take_while(|x| **x == vals[i]).count();
                            if run > best_n {
                                (best, best_n) = (vals[i], run);
                            }
                            i += run;
                        }
                        best
                    }
                }
                _ => (sum / n) as f32,
            }
        })
        .collect();
    Grid { values, bands: Vec::new(), ..g.clone() }
}

fn run_focal(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let r = match number_param(p, "radius", "Cells around")? {
        None => 1,
        Some(r) if (1.0..=100.0).contains(&r) => r.round() as usize,
        Some(_) => return err("Cells around must be from 1 to 100."),
    };
    let stat = p.get("stat").unwrap_or_else(|| "mean".into());
    let out = focal(g, r, &stat, p.get("shape").as_deref() == Some("circle"));
    note_grid(&out, ctx);
    Ok(vec![("Raster".into(), out.into_table())])
}

// ---------- Cost distance (CostDistance) ----------

const COST_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "reach",
    label: "As far as",
    kind: ParamKind::Text,
    default: "",
    hint: "The largest cost: cells that cost more to reach are left empty. Empty: no limit",
}];

/// The least cost of going from the cells that are set to every other, over a raster of the
/// cost of crossing each cell per unit of distance (Dijkstra on the eight neighbours). Empty
/// cells cannot be crossed.
pub fn cost_distance(cost: &Grid, sources: &[usize], reach: Option<f64>) -> Vec<f64> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let (w, h) = (cost.width, cost.height);
    let (cw, ch) = (cost.geotransform[1].abs(), cost.geotransform[5].abs());
    let mut best = vec![f64::INFINITY; w * h];
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();
    for &s in sources {
        if !cost.values[s].is_nan() {
            best[s] = 0.0;
            heap.push(Reverse((0, s as u32)));
        }
    }
    let steps: [(isize, isize, f64); 8] =
        [(1, 0, cw), (-1, 0, cw), (0, 1, ch), (0, -1, ch), (1, 1, cw.hypot(ch)), (1, -1, cw.hypot(ch)), (-1, 1, cw.hypot(ch)), (-1, -1, cw.hypot(ch))];
    while let Some(Reverse((key, i))) = heap.pop() {
        let i = i as usize;
        let d = f64::from_bits(key);
        if d > best[i] {
            continue;
        }
        if reach.is_some_and(|r| d > r) {
            continue;
        }
        let (c, r) = ((i % w) as isize, (i / w) as isize);
        let here = cost.values[i] as f64;
        for (dc, dr, len) in steps {
            let (cc, rr) = (c + dc, r + dr);
            if cc < 0 || rr < 0 || cc >= w as isize || rr >= h as isize {
                continue;
            }
            let j = rr as usize * w + cc as usize;
            let there = cost.values[j];
            if there.is_nan() || there < 0.0 {
                continue;
            }
            // Half the step in each cell, at its own cost.
            let nd = d + len * (here + there as f64) / 2.0;
            if nd < best[j] {
                best[j] = nd;
                heap.push(Reverse((nd.to_bits(), j as u32)));
            }
        }
    }
    best
}

fn run_cost(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let cost = grid_on(input(inp, "Cost"), "Cost")?;
    let from = input(inp, "From");
    let mut geoms: Vec<Option<Geometry<f64>>> = from.features.iter().map(|f| f.geom.clone()).collect();
    if let (Some(a), Some(b)) = (from.crs, cost.crs) {
        if a != b {
            let be = ctx.backend("Reprojecting to the raster")?;
            geom::transform_all(geoms.iter_mut().flatten(), |xs, ys| be.transform(a, b, xs, ys))?;
        }
    }
    let cells = super::raster089::Cells::of(cost);
    let mut sources = Vec::new();
    for g in geoms.iter().flatten() {
        cells.of_geometry(g, &mut sources);
    }
    if sources.is_empty() {
        return err("No feature on From lies on the cost raster. Connect the places to start from, within the raster.");
    }
    if cost.values.iter().any(|v| *v < 0.0) {
        ctx.notes.warn("Cells with a cost below 0 cannot be crossed, as empty cells.");
    }
    let reach = number_param(p, "reach", "As far as")?.filter(|r| *r > 0.0);
    let d = cost_distance(cost, &sources, reach);
    let values = d.into_iter().map(|x| if x.is_finite() && reach.is_none_or(|r| x <= r) { x as f32 } else { f32::NAN }).collect();
    let out = Grid { values, bands: Vec::new(), ..cost.clone() };
    note_grid(&out, ctx);
    Ok(vec![("Raster".into(), out.into_table())])
}

// ---------- Thin point cloud (PointThinner) ----------

const THIN_METHODS: &[(&str, &str)] = &[("voxel", "One point in each cube: the nearest to its middle"), ("distance", "No two points closer than the distance")];

const THIN_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "method",
        label: "Thin by",
        kind: ParamKind::Choice(THIN_METHODS),
        default: "voxel",
        hint: "Both keep the cloud even, where every n-th point keeps its patches dense",
    },
    ParamSpec {
        key: "size",
        label: "Size",
        kind: ParamKind::Text,
        default: "1",
        hint: "In metres: the side of the cubes, or the least distance between points",
    },
    ParamSpec {
        key: "per_class",
        label: "Each class on its own",
        kind: ParamKind::Choice(&[("yes", "Yes"), ("no", "No")]),
        default: "yes",
        hint: "So that a few points of a building are not lost among the ground points",
    },
];

fn run_thin(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let size = number_param(p, "size", "Size")?.unwrap_or(1.0);
    if size <= 0.0 {
        return err("The size must be more than 0.");
    }
    let per_class = p.get("per_class").as_deref() != Some("no");
    let class_at = t.field_index("classification").filter(|_| per_class);
    let xyz = |f: &Feature| -> Option<[f64; 3]> {
        let Some(Geometry::Point(pt)) = &f.geom else { return None };
        Some([pt.x(), pt.y(), f.heights().and_then(|z| z.first().copied()).unwrap_or(0.0)])
    };
    let class_of = |f: &Feature| class_at.and_then(|i| f.attrs[i].as_f64()).map_or(-1, |c| c as i64);
    let mut keep = vec![false; t.len()];
    if p.get("method").as_deref() == Some("distance") {
        // In a grid of cells as large as the distance: a point is kept when no kept point in
        // its cell or the cells around is too near.
        let mut kept: std::collections::HashMap<(i64, i64, i64, i64), Vec<[f64; 3]>> = std::collections::HashMap::new();
        let d2 = size * size;
        for (i, f) in t.features.iter().enumerate() {
            let Some(p3) = xyz(f) else { continue };
            let k = class_of(f);
            let cell = |v: f64| (v / size).floor() as i64;
            let (cx, cy, cz) = (cell(p3[0]), cell(p3[1]), cell(p3[2]));
            let mut near = false;
            'look: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(list) = kept.get(&(k, cx + dx, cy + dy, cz + dz)) {
                            if list.iter().any(|q| (q[0] - p3[0]).powi(2) + (q[1] - p3[1]).powi(2) + (q[2] - p3[2]).powi(2) < d2) {
                                near = true;
                                break 'look;
                            }
                        }
                    }
                }
            }
            if !near {
                kept.entry((k, cx, cy, cz)).or_default().push(p3);
                keep[i] = true;
            }
        }
    } else {
        // The point nearest the middle of each cube.
        let mut best: std::collections::HashMap<(i64, i64, i64, i64), (f64, usize)> = std::collections::HashMap::new();
        for (i, f) in t.features.iter().enumerate() {
            let Some(p3) = xyz(f) else { continue };
            let cell = |v: f64| (v / size).floor();
            let (cx, cy, cz) = (cell(p3[0]), cell(p3[1]), cell(p3[2]));
            let d = (p3[0] - (cx + 0.5) * size).powi(2) + (p3[1] - (cy + 0.5) * size).powi(2) + (p3[2] - (cz + 0.5) * size).powi(2);
            let key = (class_of(f), cx as i64, cy as i64, cz as i64);
            match best.get(&key) {
                Some((bd, _)) if *bd <= d => {}
                _ => {
                    best.insert(key, (d, i));
                }
            }
        }
        for (_, i) in best.values() {
            keep[*i] = true;
        }
    }
    let mut out = t.like();
    out.features = t.features.iter().zip(&keep).filter(|(_, k)| **k).map(|(f, _)| f.clone()).collect();
    ctx.notes.info(format!("{} of {} points kept", fmt_count(out.len() as u64), fmt_count(t.len() as u64)));
    Ok(vec![("Points".into(), out)])
}

// ---------- Stylize picture (PictureStylizer) ----------

const STYLES: &[(&str, &str)] = &[
    ("c64", "Commodore 64: big pixels in its 16 colours"),
    ("lego", "LEGO: a mosaic of round plates"),
    ("lego_square", "LEGO: a mosaic of square plates with studs"),
    ("pixels", "Big pixels in the colours of the picture"),
];

const STYLE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "style", label: "Style", kind: ParamKind::Choice(STYLES), default: "c64", hint: "" },
    ParamSpec {
        key: "across",
        label: "Pixels or plates across",
        kind: ParamKind::Text,
        default: "160",
        hint: "The Commodore 64 had 160 wide pixels across in colour; a LEGO base plate has 48 studs",
    },
    ParamSpec {
        key: "dither",
        label: "Dithering",
        kind: ParamKind::Choice(&[("no", "No"), ("yes", "Yes, as games of the time did")]),
        default: "no",
        hint: "Mixes the colours of the palette into patterns, for colours it does not have",
    },
    ParamSpec { key: "scale", label: "Each pixel as", kind: ParamKind::Text, default: "8", hint: "How many pixels of the picture each pixel or plate becomes" },
];

/// The 16 colours of the Commodore 64 (the palette of Pepto).
pub const C64: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xff, 0xff, 0xff],
    [0x68, 0x37, 0x2b],
    [0x70, 0xa4, 0xb2],
    [0x6f, 0x3d, 0x86],
    [0x58, 0x8d, 0x43],
    [0x35, 0x28, 0x79],
    [0xb8, 0xc7, 0x6f],
    [0x6f, 0x4f, 0x25],
    [0x43, 0x39, 0x00],
    [0x9a, 0x67, 0x59],
    [0x44, 0x44, 0x44],
    [0x6c, 0x6c, 0x6c],
    [0x9a, 0xd2, 0x84],
    [0x6c, 0x5e, 0xb5],
    [0x95, 0x95, 0x95],
];

/// The colours of LEGO plates that are easy to get, with their names.
pub const LEGO: &[(&str, [u8; 3])] = &[
    ("White", [0xf4, 0xf4, 0xf4]),
    ("Black", [0x1b, 0x2a, 0x34]),
    ("Light bluish grey", [0xa0, 0xa5, 0xa9]),
    ("Dark bluish grey", [0x6c, 0x6e, 0x68]),
    ("Red", [0xc9, 0x1a, 0x09]),
    ("Blue", [0x00, 0x55, 0xbf]),
    ("Medium blue", [0x5a, 0x93, 0xdb]),
    ("Dark azure", [0x07, 0x8b, 0xc9]),
    ("Bright light blue", [0x9f, 0xc3, 0xe9]),
    ("Yellow", [0xf2, 0xcd, 0x37]),
    ("Bright light yellow", [0xff, 0xf0, 0x3a]),
    ("Tan", [0xe4, 0xcd, 0x9e]),
    ("Dark tan", [0x95, 0x8a, 0x73]),
    ("Reddish brown", [0x58, 0x2a, 0x12]),
    ("Green", [0x23, 0x78, 0x41]),
    ("Bright green", [0x4b, 0x9f, 0x4a]),
    ("Lime", [0xbb, 0xe9, 0x0b]),
    ("Olive green", [0x9b, 0x9a, 0x5a]),
    ("Dark green", [0x18, 0x46, 0x32]),
    ("Orange", [0xfe, 0x8a, 0x18]),
    ("Dark orange", [0xa9, 0x55, 0x00]),
    ("Medium nougat", [0xaa, 0x7d, 0x55]),
    ("Sand green", [0xa0, 0xbc, 0xac]),
    ("Sand blue", [0x60, 0x74, 0x8c]),
    ("Dark red", [0x72, 0x0e, 0x0f]),
    ("Dark blue", [0x0a, 0x34, 0x63]),
];

fn nearest(c: [f32; 3], palette: &[[u8; 3]]) -> usize {
    // Distance weighted for how the eye sees red, green and blue.
    let mut best = (f32::MAX, 0);
    for (k, p) in palette.iter().enumerate() {
        let (dr, dg, db) = (c[0] - p[0] as f32, c[1] - p[1] as f32, c[2] - p[2] as f32);
        let rm = (c[0] + p[0] as f32) / 2.0;
        let d = (2.0 + rm / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - rm) / 256.0) * db * db;
        if d < best.0 {
            best = (d, k);
        }
    }
    best.1
}

/// A picture made small, as the mean colour of the pixels in each cell, and the parts that
/// are see-through. The cells keep the shape of the pixels: twice as wide as high for the
/// Commodore 64.
fn shrink(img: &RasterImage, w: usize, h: usize) -> Vec<Option<[f32; 3]>> {
    let mut out = Vec::with_capacity(w * h);
    for r in 0..h {
        for c in 0..w {
            let (x0, x1) = (c * img.width / w, ((c + 1) * img.width / w).max(c * img.width / w + 1).min(img.width));
            let (y0, y1) = (r * img.height / h, ((r + 1) * img.height / h).max(r * img.height / h + 1).min(img.height));
            let (mut s, mut a) = ([0f32; 3], 0f32);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = &img.rgba[(y * img.width + x) * 4..][..4];
                    let alpha = p[3] as f32 / 255.0;
                    for k in 0..3 {
                        s[k] += p[k] as f32 * alpha;
                    }
                    a += alpha;
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as f32;
            out.push((a > n * 0.5).then(|| [s[0] / a, s[1] / a, s[2] / a]));
        }
    }
    out
}

/// The cells in colours of the palette, mixed into patterns where it is asked for
/// (Floyd–Steinberg). Gives the index in the palette of each cell.
fn quantize(cells: &[Option<[f32; 3]>], w: usize, palette: &[[u8; 3]], dither: bool) -> Vec<Option<usize>> {
    let mut work: Vec<Option<[f32; 3]>> = cells.to_vec();
    let h = cells.len() / w.max(1);
    let mut out = vec![None; cells.len()];
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            let Some(col) = work[i] else { continue };
            let k = nearest(col, palette);
            out[i] = Some(k);
            if dither {
                let p = palette[k];
                let e = [col[0] - p[0] as f32, col[1] - p[1] as f32, col[2] - p[2] as f32];
                let mut spread = |cc: isize, rr: usize, f: f32| {
                    if cc < 0 || cc >= w as isize || rr >= h {
                        return;
                    }
                    if let Some(v) = &mut work[rr * w + cc as usize] {
                        for k in 0..3 {
                            v[k] = (v[k] + e[k] * f).clamp(0.0, 255.0);
                        }
                    }
                };
                spread(c as isize + 1, r, 7.0 / 16.0);
                spread(c as isize - 1, r + 1, 3.0 / 16.0);
                spread(c as isize, r + 1, 5.0 / 16.0);
                spread(c as isize + 1, r + 1, 1.0 / 16.0);
            }
        }
    }
    out
}

/// A plate of LEGO seen from above: a square in its colour with a stud on it, lit from the
/// upper left, or a round plate.
fn plate(pm: &mut tiny_skia::Pixmap, x: f32, y: f32, s: f32, c: [u8; 3], round: bool) {
    use tiny_skia::{Color, FillRule, Paint, PathBuilder, Transform};
    let paint = |c: [u8; 3], a: u8| {
        let mut p = Paint::default();
        p.set_color(Color::from_rgba8(c[0], c[1], c[2], a));
        p.anti_alias = true;
        p
    };
    let shade = |c: [u8; 3], k: f32| [(c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8];
    let mut pb = PathBuilder::new();
    if round {
        pb.push_circle(x + s / 2.0, y + s / 2.0, s * 0.47);
    } else if let Some(r) = tiny_skia::Rect::from_xywh(x + s * 0.02, y + s * 0.02, s * 0.96, s * 0.96) {
        pb.push_rect(r);
    }
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &paint(shade(c, 0.85), 255), FillRule::Winding, Transform::identity(), None);
    }
    // The stud, with its light edge up left and its shadow down right.
    let (cx, cy, r) = (x + s / 2.0, y + s / 2.0, s * 0.3);
    for (dx, dy, k) in [(s * 0.04, s * 0.05, 0.6), (-s * 0.02, -s * 0.02, 1.25), (0.0, 0.0, 1.0)] {
        let mut pb = PathBuilder::new();
        pb.push_circle(cx + dx, cy + dy, r);
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &paint(shade(c, k), 255), FillRule::Winding, Transform::identity(), None);
        }
    }
}

/// The picture in the style asked for, the cells across, and for LEGO the plates of each
/// colour, by its name, with how many.
pub fn stylize(img: &RasterImage, style: &str, across: usize, scale: usize, dither: bool) -> Result<(RasterImage, Vec<(String, usize)>)> {
    let across = across.clamp(4, 2000);
    let scale = scale.clamp(1, 64);
    // The Commodore 64 has pixels twice as wide as high.
    let wide = if style == "c64" { 2 } else { 1 };
    let cell_w = img.width as f64 / across as f64;
    let down = ((img.height as f64 / (cell_w / wide as f64)).round() as usize).max(1);
    let cells = shrink(img, across, down);
    let palette: Vec<[u8; 3]> = match style {
        "c64" => C64.to_vec(),
        "lego" | "lego_square" => LEGO.iter().map(|l| l.1).collect(),
        _ => Vec::new(),
    };
    let chosen: Vec<Option<[u8; 3]>> = if palette.is_empty() {
        cells.iter().map(|c| c.map(|c| [c[0] as u8, c[1] as u8, c[2] as u8])).collect()
    } else {
        quantize(&cells, across, &palette, dither).into_iter().map(|k| k.map(|k| palette[k])).collect()
    };
    let (pw, ph) = (across * scale * wide, down * scale);
    let mut pm =
        tiny_skia::Pixmap::new(pw as u32, ph as u32).ok_or_else(|| GbError("The picture would be too large. Choose fewer cells or a smaller scale.".into()))?;
    let mut parts: Vec<(String, usize)> = Vec::new();
    let lego = style.starts_with("lego");
    if lego {
        // The base plate under the mosaic.
        pm.fill(tiny_skia::Color::from_rgba8(0x40, 0x40, 0x40, 255));
    }
    for r in 0..down {
        for c in 0..across {
            let Some(col) = chosen[r * across + c] else { continue };
            let (x, y, s) = ((c * scale * wide) as f32, (r * scale) as f32, scale as f32);
            if lego {
                plate(&mut pm, x, y, s, col, style == "lego");
                if let Some(name) = LEGO.iter().find(|l| l.1 == col).map(|l| l.0) {
                    match parts.iter_mut().find(|p| p.0 == name) {
                        Some(p) => p.1 += 1,
                        None => parts.push((name.to_string(), 1)),
                    }
                }
            } else if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, (scale * wide) as f32, s) {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color(tiny_skia::Color::from_rgba8(col[0], col[1], col[2], 255));
                pm.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
            }
        }
    }
    parts.sort_by_key(|b| std::cmp::Reverse(b.1));
    let mut rgba = Vec::with_capacity(pw * ph * 4);
    for p in pm.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    // Placed where the picture was, with its pixels of another size.
    let g = img.geotransform;
    let (kx, ky) = (img.width as f64 / pw as f64, img.height as f64 / ph as f64);
    Ok((RasterImage { width: pw, height: ph, rgba, geotransform: [g[0], g[1] * kx, g[2] * ky, g[3], g[4] * kx, g[5] * ky] }, parts))
}

fn run_stylize(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Picture");
    let img = match (&t.image, &t.grid) {
        (Some(i), _) => (**i).clone(),
        (None, Some(g)) => crate::rasterlook::preview(g, 4000),
        _ => return err("The stream on Picture has no picture. Connect a map from Draw map, a chart, or a raster."),
    };
    let style = p.get("style").unwrap_or_else(|| "c64".into());
    let across = number_param(p, "across", "Pixels or plates across")?.unwrap_or(160.0) as usize;
    let scale = number_param(p, "scale", "Each pixel as")?.unwrap_or(8.0) as usize;
    let (out, parts) = stylize(&img, &style, across, scale, p.get("dither").as_deref() == Some("yes"))?;
    ctx.notes.info(format!("{} × {} pixels", out.width, out.height));
    let mut list = Table { fields: vec![Field::new("colour", FieldType::Text), Field::new("plates", FieldType::Int)], ..Default::default() };
    if !parts.is_empty() {
        let total: usize = parts.iter().map(|p| p.1).sum();
        ctx.notes.info(format!("{} plates of 1 × 1 in {} colours", fmt_count(total as u64), parts.len()));
        for (name, n) in parts {
            list.features.push(Feature::new(vec![Value::Text(name), Value::Int(n as i64)], None));
        }
    }
    let _ = column;
    Ok(vec![("Picture".into(), out.into_table(t.crs)), ("Parts".into(), list)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "RasterCreator",
        title: "Create raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Makes a raster over a rectangle, or the extent of what comes in on Extent, with cells of a size: every cell one value, or the value of an expression of the middle of the cell (x and y), its row and its column. A start for Raster calculator, a cost raster or a grid to count with.",
        inputs: &["Extent"],
        outputs: &["Raster"],
        ports: None,
        params: CREATE_PARAMS,
        run: run_create,
        summary: |p| format!("{} m cells", p.get("cell").unwrap_or_else(|| "10".into())),
    },
    NodeType {
        name: "FocalStatistics",
        title: "Focal statistics",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Gives each cell a statistic of the cells around it, in a square or a circle: the mean to smooth, the largest or smallest, the spread, the most common value to clean up classes, or how many have a value.",
        inputs: &["Raster"],
        outputs: &["Raster"],
        ports: None,
        params: FOCAL_PARAMS,
        run: run_focal,
        summary: |p| format!("{} of {} around", p.get("stat").unwrap_or_else(|| "mean".into()), p.get("radius").unwrap_or_else(|| "1".into())),
    },
    NodeType {
        name: "CostDistance",
        title: "Cost distance",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "The least cost of getting from the places on From to every cell, over a raster of what it costs to cross each cell, such as minutes per metre through forest, bog and fields. Empty cells cannot be crossed. Where there are no roads, how far one comes on foot or in a forest machine.",
        inputs: &["Cost", "From"],
        outputs: &["Raster"],
        ports: None,
        params: COST_PARAMS,
        run: run_cost,
        summary: |_| String::new(),
    },
    NodeType {
        name: "PointThinner",
        title: "Thin point cloud",
        hidden: false,
        group: Group::Transformer,
        category: "Point clouds",
        desc: "Makes a point cloud evenly thinner: one point in each cube of a size, the one nearest its middle, or no two points closer than a distance. Each class on its own if asked, so that few points of one class are kept among many of another.",
        inputs: &["Points"],
        outputs: &["Points"],
        ports: None,
        params: THIN_PARAMS,
        run: run_thin,
        summary: |p| format!("{} m", p.get("size").unwrap_or_else(|| "1".into())),
    },
    NodeType {
        name: "PictureStylizer",
        title: "Stylize picture",
        hidden: false,
        group: Group::Transformer,
        category: "Maps",
        desc: "Draws a picture again in a style: as on a Commodore 64, in big pixels with its 16 colours, as a mosaic of LEGO plates in the colours one can buy, with a list of the plates of each colour to build it, or in big pixels of its own colours. For maps from Draw map, terrain, orthophotos and charts.",
        inputs: &["Picture"],
        outputs: &["Picture", "Parts"],
        ports: None,
        params: STYLE_PARAMS,
        run: run_stylize,
        summary: |p| p.get("style").unwrap_or_else(|| "c64".into()),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> Grid {
        let values = (0..100).map(|i| (i % 10) as f32).collect();
        Grid { width: 10, height: 10, values, geotransform: [0.0, 1.0, 0.0, 10.0, 0.0, -1.0], crs: Some(3006), bands: Vec::new() }
    }

    #[test]
    fn rasters_are_created_smoothed_and_crossed() {
        let (out, _) = run_for_test("RasterCreator", &[("bbox", "0, 0, 100, 50"), ("cell", "10"), ("value", "column + row * 100")], vec![]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!((g.width, g.height, g.crs), (10, 5, Some(3006)));
        assert_eq!((g.get(3, 0), g.get(2, 4)), (Some(3.0), Some(402.0)));
        let f = focal(&ramp(), 1, "mean", false);
        assert_eq!(f.get(5, 5), Some(5.0));
        assert_eq!(f.get(0, 5), Some(0.5), "at the edge only the cells there count");
        assert_eq!(focal(&ramp(), 1, "max", false).get(5, 5), Some(6.0));
        // Crossing a field at cost 1 and a wall at cost 100.
        let mut cost = Grid { values: vec![1.0; 100], ..ramp() };
        for r in 0..9 {
            cost.values[r * 10 + 5] = 100.0;
        }
        let d = cost_distance(&cost, &[0], None);
        assert_eq!(d[4], 4.0);
        assert!(d[6] < 100.0 && d[6] > 15.0, "round the wall rather than through it: {}", d[6]);
    }

    #[test]
    fn points_are_thinned_evenly() {
        // A dense patch and a sparse one.
        let mut t = Table { fields: vec![Field::new("classification", FieldType::Int)], crs: Some(3006), ..Default::default() };
        for i in 0..100 {
            let (x, y) = ((i % 10) as f64 * 0.1, (i / 10) as f64 * 0.1);
            t.features.push(Feature { attrs: vec![Value::Int(2)], geom: Some(Geometry::Point(geo::Point::new(x, y))), z: Some(Arc::from(vec![0.0])) });
        }
        t.features.push(Feature { attrs: vec![Value::Int(6)], geom: Some(Geometry::Point(geo::Point::new(0.5, 0.5))), z: Some(Arc::from(vec![0.0])) });
        let (out, _) = run_for_test("PointThinner", &[("size", "0.5")], vec![("Points", t.clone())]).unwrap();
        assert_eq!(out[0].1.len(), 5, "four cubes of ground, and the building point of its own class");
        let (out, _) = run_for_test("PointThinner", &[("size", "0.5"), ("method", "distance"), ("per_class", "no")], vec![("Points", t)]).unwrap();
        assert!(out[0].1.len() >= 4 && out[0].1.len() <= 9, "{}", out[0].1.len());
    }

    #[test]
    fn a_picture_becomes_c64_or_lego() {
        // A picture half sea blue and half grass green.
        let (w, h) = (64usize, 32usize);
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(if x < 32 { &[0x30, 0x60, 0xc0, 255] } else { &[0x50, 0xa0, 0x40, 255] });
            }
        }
        let img = RasterImage { width: w, height: h, rgba, geotransform: [0.0, 1.0, 0.0, 32.0, 0.0, -1.0] };
        let (c64, _) = stylize(&img, "c64", 16, 4, false).unwrap();
        assert_eq!((c64.width, c64.height), (16 * 4 * 2, 16 * 4));
        let px = |i: &RasterImage, x: usize, y: usize| [i.rgba[(y * i.width + x) * 4], i.rgba[(y * i.width + x) * 4 + 1], i.rgba[(y * i.width + x) * 4 + 2]];
        assert!(C64.contains(&px(&c64, 2, 2)) && C64.contains(&px(&c64, c64.width - 3, 2)));
        assert_ne!(px(&c64, 2, 2), px(&c64, c64.width - 3, 2));
        let (_, parts) = stylize(&img, "lego_square", 8, 10, false).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts.iter().map(|p| p.1).sum::<usize>(), 8 * 4);
    }
}
