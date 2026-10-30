//! Tools for rasters added in 0.90: interpolate points to a raster, resample a raster to
//! other cells, fill its empty cells, and catchments with the streams and the flow over a
//! terrain model.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Grid};
use geo::Geometry;

fn grid_on<'a>(t: &'a Table, port: &str) -> Result<&'a Grid> {
    match t.grid.as_deref() {
        Some(g) if g.is_turned() => err(format!("The raster on {} is turned, which this tool cannot work with yet.", port)),
        Some(g) => Ok(g),
        None if t.image.is_some() => {
            err(format!("The raster on {} is a picture of colours, not values. Read it with Cells as set to Heights or other values on a grid.", port))
        }
        None => err(format!("The stream on {} has no grid of values. Read a GeoTIFF with Cells as set to Heights or other values on a grid.", port)),
    }
}

fn note_grid(g: &Grid, ctx: &mut NodeCtx, what: &str) {
    let short = |v: f32| fmt_real((v as f64 * 1000.0).round() / 1000.0);
    match g.range() {
        Some((lo, hi)) => {
            ctx.notes.info(format!("{} × {} cells of {}, {} from {} to {}", g.width, g.height, fmt_real(g.cell_size().0), what, short(lo), short(hi)))
        }
        None => ctx.notes.warn("Every cell of the result is empty."),
    }
}

/// Cells of a size over a rectangle, lined up with whole multiples of the size.
fn cells_over(rect: [f64; 4], size: f64) -> Result<(usize, usize, [f64; 6])> {
    let left = (rect[0] / size).floor() * size;
    let top = (rect[3] / size).ceil() * size;
    let w = ((rect[2] - left) / size).ceil().max(1.0) as usize;
    let h = ((top - rect[1]) / size).ceil().max(1.0) as usize;
    if w * h > GRID_CELLS {
        return err(format!(
            "That would be {} × {} cells, more than the {} a raster may have. Choose larger cells.",
            fmt_count(w as u64),
            fmt_count(h as u64),
            fmt_count(GRID_CELLS as u64)
        ));
    }
    Ok((w, h, [left, size, 0.0, top, 0.0, -size]))
}

// ---------- Interpolate points (PointInterpolator) ----------

const INTERPOLATION: &[(&str, &str)] = &[
    ("tin", "Linear in triangles between the points (TIN)"),
    ("natural", "Natural neighbours: smooth, through the points"),
    ("idw", "Inverse distance: the nearest points, weighted"),
    ("nearest", "The nearest point"),
];

const INTERPOLATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "value",
        label: "Value",
        kind: ParamKind::Expression("Points"),
        default: "",
        hint: "Empty takes the height of the points. Or an attribute or expression, such as depth or the noise measured",
    },
    ParamSpec { key: "method", label: "Between the points", kind: ParamKind::Choice(INTERPOLATION), default: "tin", hint: "" },
    ParamSpec {
        key: "cell",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "10",
        hint: "In the units of the coordinate system, such as metres. A raster connected to Like gives its cells instead",
    },
    ParamSpec {
        key: "neighbours",
        label: "Nearest points",
        kind: ParamKind::Text,
        default: "12",
        hint: "For inverse distance: how many of the nearest points each cell takes",
    },
    ParamSpec {
        key: "power",
        label: "Power",
        kind: ParamKind::Text,
        default: "2",
        hint: "For inverse distance: higher lets the nearest points count for more",
    },
    ParamSpec {
        key: "reach",
        label: "As far as",
        kind: ParamKind::Text,
        default: "",
        hint: "Cells farther than this from every point are left empty. Empty: no limit. The triangles reach only between the points",
    },
];

/// A point with a value, for the triangles.
#[derive(Clone, Copy, Debug)]
struct Valued {
    x: f64,
    y: f64,
    v: f64,
}

impl spade::HasPosition for Valued {
    type Scalar = f64;
    fn position(&self) -> spade::Point2<f64> {
        spade::Point2::new(self.x, self.y)
    }
}

/// Values at the middles of the cells, between the points. `method` is one of INTERPOLATION.
#[allow(clippy::too_many_arguments)]
pub fn interpolate(points: &[(f64, f64, f64)], w: usize, h: usize, gt: [f64; 6], method: &str, k: usize, power: f64, reach: Option<f64>) -> Result<Vec<f32>> {
    use rayon::prelude::*;
    use rstar::{primitives::GeomWithData, RTree};
    let center = |c: usize, r: usize| (gt[0] + (c as f64 + 0.5) * gt[1], gt[3] + (r as f64 + 0.5) * gt[5]);
    let tree: RTree<GeomWithData<[f64; 2], f64>> = RTree::bulk_load(points.iter().map(|(x, y, v)| GeomWithData::new([*x, *y], *v)).collect());
    let near_enough = |x: f64, y: f64| reach.is_none_or(|r| tree.nearest_neighbor(&[x, y]).is_some_and(|n| (n.geom()[0] - x).hypot(n.geom()[1] - y) <= r));
    let values: Vec<f32> = match method {
        "tin" | "natural" => {
            use spade::{DelaunayTriangulation, FloatTriangulation, Triangulation};
            // Loaded all at once, which is much quicker than one by one. Points in the same
            // place become one.
            let tri: DelaunayTriangulation<Valued> = DelaunayTriangulation::bulk_load(points.iter().map(|(x, y, v)| Valued { x: *x, y: *y, v: *v }).collect())
                .map_err(|e| GbError(format!("The points cannot be made into triangles: {:?}", e)))?;
            let natural = method == "natural";
            (0..h)
                .into_par_iter()
                .flat_map_iter(|r| {
                    let bary = tri.barycentric();
                    let nn = tri.natural_neighbor();
                    (0..w)
                        .map(|c| {
                            let (x, y) = center(c, r);
                            if !near_enough(x, y) {
                                return f32::NAN;
                            }
                            let p = spade::Point2::new(x, y);
                            let v = if natural { nn.interpolate(|v| v.data().v, p) } else { bary.interpolate(|v| v.data().v, p) };
                            v.map_or(f32::NAN, |v| v as f32)
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        }
        "nearest" => (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = center(i % w, i / w);
                match tree.nearest_neighbor(&[x, y]) {
                    Some(n) if reach.is_none_or(|r| (n.geom()[0] - x).hypot(n.geom()[1] - y) <= r) => n.data as f32,
                    _ => f32::NAN,
                }
            })
            .collect(),
        _ => (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = center(i % w, i / w);
                let (mut sum, mut weights) = (0.0, 0.0);
                for n in tree.nearest_neighbor_iter(&[x, y]).take(k.max(1)) {
                    let d = (n.geom()[0] - x).hypot(n.geom()[1] - y);
                    if reach.is_some_and(|r| d > r) {
                        break;
                    }
                    if d < 1e-9 {
                        return n.data as f32;
                    }
                    let wt = 1.0 / d.powf(power);
                    sum += wt * n.data;
                    weights += wt;
                }
                if weights > 0.0 {
                    (sum / weights) as f32
                } else {
                    f32::NAN
                }
            })
            .collect(),
    };
    Ok(values)
}

fn run_interpolate(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let like = input(inp, "Like");
    let method = p.get("method").unwrap_or_else(|| "tin".into());
    let expr = p.get("value").filter(|v| !v.trim().is_empty());
    let compiled = match &expr {
        Some(e) => Some(crate::expr::compile(e, &t.field_names()).map_err(|e| GbError(format!("Value: {}", e.0)))?),
        None => None,
    };
    let geographic = ctx.geographic(t.crs);
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    let (mut flat, mut empty) = (0usize, 0usize);
    for (i, f) in t.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        let coords = geom::coords(g);
        let z = f.heights();
        for (k, c) in coords.iter().enumerate() {
            let v = match &compiled {
                Some(e) => e.eval(&crate::expr::Row { feature: f, index: i, geographic })?.as_f64(),
                None => z.and_then(|z| z.get(k).copied()),
            };
            match v {
                Some(v) if v.is_finite() => points.push((c.x, c.y, v)),
                Some(_) => empty += 1,
                None if compiled.is_some() => empty += 1,
                None => flat += 1,
            }
        }
    }
    if points.is_empty() {
        return err(if flat > 0 {
            "The points have no heights. Give the value as an attribute, or set their heights first."
        } else {
            "There are no points with a value to interpolate between."
        });
    }
    if flat + empty > 0 {
        ctx.notes.warn(format!("{} points have no value and were left out.", fmt_count((flat + empty) as u64)));
    }
    // The cells: those of Like, or of a size over the points.
    let (w, h, gt, crs) = match like.grid.as_deref() {
        Some(g) => {
            if g.crs.is_some() && t.crs.is_some() && g.crs != t.crs {
                return err(format!("The raster on Like is in {} and the points in {}. Reproject one of them first.", crs_label(g.crs), crs_label(t.crs)));
            }
            (g.width, g.height, g.geotransform, g.crs.or(t.crs))
        }
        None => {
            let size = number_param(p, "cell", "Cell size")?.unwrap_or(10.0);
            if size <= 0.0 {
                return err("The cell size must be more than 0.");
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for (x, y, _) in &points {
                (x0, y0, x1, y1) = (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y));
            }
            let (w, h, gt) = cells_over([x0, y0, x1, y1], size)?;
            (w, h, gt, t.crs)
        }
    };
    let k = number_param(p, "neighbours", "Nearest points")?.unwrap_or(12.0).clamp(1.0, 1000.0) as usize;
    let power = number_param(p, "power", "Power")?.unwrap_or(2.0);
    let reach = number_param(p, "reach", "As far as")?.filter(|r| *r > 0.0);
    if matches!(method.as_str(), "tin" | "natural") && points.len() < 3 {
        return err("Triangles need at least three points. Choose inverse distance or the nearest point.");
    }
    let values = interpolate(&points, w, h, gt, &method, k, power, reach)?;
    let g = Grid { width: w, height: h, values, geotransform: gt, crs, bands: Vec::new() };
    ctx.notes.info(format!("{} points", fmt_count(points.len() as u64)));
    note_grid(&g, ctx, "values");
    Ok(vec![("Raster".into(), g.into_table())])
}

// ---------- Resample raster (RasterResampler) ----------

const RESAMPLE_METHODS: &[(&str, &str)] = &[
    ("nearest", "The cell under the middle: for classes"),
    ("bilinear", "Between the four nearest cells: for heights and measures"),
    ("mean", "The mean of the cells within: to make it coarser"),
    ("majority", "The most common value within: classes made coarser"),
];

const RESAMPLE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "cell",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "10",
        hint: "The new size of the cells. A raster connected to Like gives its cells instead, so that the two line up cell by cell",
    },
    ParamSpec { key: "method", label: "The new value", kind: ParamKind::Choice(RESAMPLE_METHODS), default: "nearest", hint: "" },
];

/// A grid with the values at other cells. Mean and majority gather every cell whose middle
/// lies in a new cell; a new cell without any takes the cell under its middle.
pub fn resample_to(g: &Grid, w: usize, h: usize, gt: [f64; 6], method: &str) -> Grid {
    use rayon::prelude::*;
    let center = |c: usize, r: usize| (gt[0] + (c as f64 + 0.5) * gt[1], gt[3] + (r as f64 + 0.5) * gt[5]);
    let band = |values: &[f32]| -> Vec<f32> {
        let src = Grid { width: g.width, height: g.height, values: values.to_vec(), geotransform: g.geotransform, crs: g.crs, bands: Vec::new() };
        match method {
            "mean" | "majority" => {
                let mut gathered: Vec<Vec<f32>> = vec![Vec::new(); w * h];
                for r in 0..src.height {
                    for c in 0..src.width {
                        let v = src.values[r * src.width + c];
                        if v.is_nan() {
                            continue;
                        }
                        let (x, y) = src.center(c, r);
                        let (nc, nr) = ((x - gt[0]) / gt[1], (y - gt[3]) / gt[5]);
                        if nc < 0.0 || nr < 0.0 || nc as usize >= w || nr as usize >= h {
                            continue;
                        }
                        gathered[nr as usize * w + nc as usize].push(v);
                    }
                }
                gathered
                    .into_par_iter()
                    .enumerate()
                    .map(|(i, mut vs)| {
                        if vs.is_empty() {
                            let (x, y) = center(i % w, i / w);
                            return src.at(x, y).unwrap_or(f32::NAN);
                        }
                        if method == "mean" {
                            (vs.iter().map(|v| *v as f64).sum::<f64>() / vs.len() as f64) as f32
                        } else {
                            vs.sort_by(f32::total_cmp);
                            let (mut best, mut n, mut i) = (vs[0], 0, 0);
                            while i < vs.len() {
                                let j = vs[i..].iter().take_while(|x| **x == vs[i]).count();
                                if j > n {
                                    (best, n) = (vs[i], j);
                                }
                                i += j;
                            }
                            best
                        }
                    })
                    .collect()
            }
            _ => (0..w * h)
                .into_par_iter()
                .map(|i| {
                    let (x, y) = center(i % w, i / w);
                    let v = if method == "bilinear" { src.bilinear(x, y) } else { src.at(x, y) };
                    v.unwrap_or(f32::NAN)
                })
                .collect(),
        }
    };
    Grid { width: w, height: h, values: band(&g.values), bands: g.bands.iter().map(|b| band(b)).collect(), geotransform: gt, crs: g.crs }
}

fn run_resample(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let like = input(inp, "Like");
    let method = p.get("method").unwrap_or_else(|| "nearest".into());
    let warped;
    let (g, w, h, gt) = match like.grid.as_deref() {
        Some(l) => {
            let g = match (g.crs, l.crs) {
                (Some(a), Some(b)) if a != b => {
                    ctx.notes.info(format!("The raster was reprojected from {} to {}", crs_label(Some(a)), crs_label(Some(b))));
                    warped = ctx.backend("Reprojecting the raster")?.warp_grid(g, b)?;
                    &warped
                }
                _ => g,
            };
            (g, l.width, l.height, l.geotransform)
        }
        None => {
            let size = number_param(p, "cell", "Cell size")?.unwrap_or(10.0);
            if size <= 0.0 {
                return err("The cell size must be more than 0.");
            }
            let o = g.outline();
            use geo::BoundingRect;
            let r = o.bounding_rect().expect("an outline");
            let gt0 = g.geotransform;
            // Lined up with the corner of the raster, not with whole multiples of the size.
            let w = ((r.width()) / size).round().max(1.0) as usize;
            let h = ((r.height()) / size).round().max(1.0) as usize;
            if w * h > GRID_CELLS {
                return err(format!(
                    "That would be {} × {} cells, more than a raster may have. Choose larger cells.",
                    fmt_count(w as u64),
                    fmt_count(h as u64)
                ));
            }
            (g, w, h, [gt0[0], size, 0.0, gt0[3], 0.0, -size])
        }
    };
    let out = resample_to(g, w, h, gt, &method);
    note_grid(&out, ctx, "values");
    Ok(vec![("Raster".into(), out.into_table())])
}

// ---------- Fill empty cells (RasterFiller) ----------

const FILL_METHODS: &[(&str, &str)] = &[("nearby", "From the cells around, weighted by how near"), ("value", "With one value")];

const FILL_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "method", label: "Fill", kind: ParamKind::Choice(FILL_METHODS), default: "nearby", hint: "" },
    ParamSpec {
        key: "reach",
        label: "As far as",
        kind: ParamKind::Text,
        default: "10",
        hint: "How many cells away the values are taken from. Holes wider than twice this stay empty in the middle",
    },
    ParamSpec { key: "value", label: "Value", kind: ParamKind::Text, default: "0", hint: "For one value: what every empty cell gets" },
];

/// The empty cells filled from the cells with values within `reach` cells, weighted by one
/// over the square of the distance. Gives how many were filled.
pub fn fill_nearby(g: &Grid, reach: usize) -> (Grid, usize) {
    use rayon::prelude::*;
    let (w, h) = (g.width, g.height);
    let r = reach as isize;
    let values: Vec<f32> = (0..w * h)
        .into_par_iter()
        .map(|i| {
            let v = g.values[i];
            if !v.is_nan() {
                return v;
            }
            let (c, row) = ((i % w) as isize, (i / w) as isize);
            let (mut sum, mut weights) = (0f64, 0f64);
            for dr in -r..=r {
                let rr = row + dr;
                if rr < 0 || rr >= h as isize {
                    continue;
                }
                for dc in -r..=r {
                    let cc = c + dc;
                    if cc < 0 || cc >= w as isize || (dc == 0 && dr == 0) {
                        continue;
                    }
                    let d2 = (dc * dc + dr * dr) as f64;
                    if d2 > (r * r) as f64 {
                        continue;
                    }
                    let x = g.values[rr as usize * w + cc as usize];
                    if !x.is_nan() {
                        sum += x as f64 / d2;
                        weights += 1.0 / d2;
                    }
                }
            }
            if weights > 0.0 {
                (sum / weights) as f32
            } else {
                f32::NAN
            }
        })
        .collect();
    let filled = g.values.iter().zip(&values).filter(|(a, b)| a.is_nan() && !b.is_nan()).count();
    (Grid { values, ..g.clone() }, filled)
}

fn run_fill(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let empty = g.values.iter().filter(|v| v.is_nan()).count();
    let (out, filled) = if p.get("method").as_deref() == Some("value") {
        let v = number_param(p, "value", "Value")?.unwrap_or(0.0) as f32;
        let mut out = g.clone();
        out.values.iter_mut().filter(|x| x.is_nan()).for_each(|x| *x = v);
        (out, empty)
    } else {
        let reach = number_param(p, "reach", "As far as")?.unwrap_or(10.0);
        if !(1.0..=500.0).contains(&reach) {
            return err("As far as must be from 1 to 500 cells.");
        }
        fill_nearby(g, reach.round() as usize)
    };
    ctx.notes.info(format!("{} of {} empty cells filled", fmt_count(filled as u64), fmt_count(empty as u64)));
    if filled < empty {
        ctx.notes.warn(format!("{} cells are farther than that from any value, and stay empty.", fmt_count((empty - filled) as u64)));
    }
    Ok(vec![("Raster".into(), out.into_table())])
}

// ---------- Catchments (Watershed) ----------

/// The eight directions, clockwise from east: (column, row) steps.
const D8: [(isize, isize); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// Where water runs from each cell of a terrain model: the neighbour of the steepest descent
/// (D8), after the hollows have been filled so that every cell drains to the edge or to an
/// empty cell. None where it leaves the grid. The filling is the priority flood of Barnes,
/// with a tiny slope over flats so that they drain too.
pub fn flow_directions(g: &Grid) -> Vec<Option<u32>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let (w, h) = (g.width, g.height);
    let n = w * h;
    let mut z: Vec<f64> = g.values.iter().map(|v| *v as f64).collect();
    let mut done = vec![false; n];
    // Ordered by height, then by when they were reached, so that flats drain away from the
    // edge in order.
    let mut heap: BinaryHeap<Reverse<(u64, u64, u32)>> = BinaryHeap::new();
    let key = |v: f64| {
        let b = v.to_bits();
        if v >= 0.0 {
            b ^ (1 << 63)
        } else {
            !b
        }
    };
    let mut order = 0u64;
    let mut down: Vec<Option<u32>> = vec![None; n];
    for i in 0..n {
        let (c, r) = (i % w, i / w);
        let edge = c == 0 || r == 0 || c == w - 1 || r == h - 1;
        let by_empty = !edge && D8.iter().any(|(dc, dr)| z[((r as isize + dr) as usize) * w + (c as isize + dc) as usize].is_nan());
        if z[i].is_nan() {
            done[i] = true;
        } else if edge || by_empty {
            done[i] = true;
            heap.push(Reverse((key(z[i]), order, i as u32)));
            order += 1;
        }
    }
    let (cw, ch) = (g.geotransform[1].abs(), g.geotransform[5].abs());
    let tiny = 1e-5 * cw.min(ch);
    while let Some(Reverse((_, _, i))) = heap.pop() {
        let i = i as usize;
        let (c, r) = ((i % w) as isize, (i / w) as isize);
        for (dc, dr) in D8 {
            let (cc, rr) = (c + dc, r + dr);
            if cc < 0 || rr < 0 || cc >= w as isize || rr >= h as isize {
                continue;
            }
            let j = rr as usize * w + cc as usize;
            if done[j] {
                continue;
            }
            done[j] = true;
            // A cell lower than the one it drains through is raised to just above it.
            if z[j] <= z[i] {
                z[j] = z[i] + tiny;
            }
            down[j] = Some(i as u32);
            heap.push(Reverse((key(z[j]), order, j as u32)));
            order += 1;
        }
    }
    // The steepest way down in the filled terrain, which is the way the flood came where
    // it is flat, and the true steepest descent elsewhere.
    for i in 0..n {
        if z[i].is_nan() {
            down[i] = None;
            continue;
        }
        let (c, r) = ((i % w) as isize, (i / w) as isize);
        let mut best: Option<(f64, usize)> = None;
        for (dc, dr) in D8 {
            let (cc, rr) = (c + dc, r + dr);
            if cc < 0 || rr < 0 || cc >= w as isize || rr >= h as isize {
                continue;
            }
            let j = rr as usize * w + cc as usize;
            if z[j].is_nan() {
                continue;
            }
            let d = ((dc as f64 * cw).powi(2) + (dr as f64 * ch).powi(2)).sqrt();
            let drop = (z[i] - z[j]) / d;
            if drop > 0.0 && best.is_none_or(|b| drop > b.0) {
                best = Some((drop, j));
            }
        }
        if let Some((_, j)) = best {
            down[i] = Some(j as u32);
        }
    }
    down
}

/// How many cells drain through each cell, itself included.
pub fn accumulation(down: &[Option<u32>]) -> Vec<u32> {
    let n = down.len();
    let mut ins = vec![0u32; n];
    for d in down.iter().flatten() {
        ins[*d as usize] += 1;
    }
    let mut acc = vec![1u32; n];
    let mut stack: Vec<usize> = (0..n).filter(|i| ins[*i] == 0).collect();
    while let Some(i) = stack.pop() {
        if let Some(j) = down[i] {
            let j = j as usize;
            acc[j] += acc[i];
            ins[j] -= 1;
            if ins[j] == 0 {
                stack.push(j);
            }
        }
    }
    acc
}

const WATERSHED_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "snap",
        label: "Move outlets",
        kind: ParamKind::Text,
        default: "50",
        hint: "An outlet is moved to the cell with the most water running through it within this distance, so that it lies on the stream",
    },
    ParamSpec {
        key: "stream",
        label: "Streams from",
        kind: ParamKind::Text,
        default: "1",
        hint: "Square kilometres: a cell with water from this much land or more is part of a stream",
    },
    ParamSpec {
        key: "smallest",
        label: "Without outlets, smallest",
        kind: ParamKind::Text,
        default: "1",
        hint: "Square kilometres: without outlets on Outlets, each area that drains to the edge of the terrain this large or larger is a catchment",
    },
];

fn run_watershed(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Heights"), "Heights")?;
    if ctx.geographic(g.crs) == Some(true) {
        return err(
            "The heights are in a coordinate system in degrees. Reproject the raster first, such as to EPSG:3006, so that distances and areas are in metres.",
        );
    }
    let outlets = input(inp, "Outlets");
    let (w, h) = (g.width, g.height);
    let cell_area = (g.geotransform[1] * g.geotransform[5]).abs();
    let km2 = |cells: u32| cells as f64 * cell_area / 1e6;
    let down = flow_directions(g);
    let acc = accumulation(&down);
    if ctx.stopped() {
        return err(crate::model::STOPPED);
    }
    // The cells upstream of each outlet: the outlets given, moved onto the stream, or each
    // place where water leaves the terrain with enough land behind it.
    let mut label = vec![0u32; w * h];
    let mut sources: Vec<(usize, Option<usize>)> = Vec::new(); // (cell, feature of Outlets)
    let cells = super::raster089::Cells::of(g);
    if outlets.features.iter().any(|f| f.geom.is_some()) {
        let mut geoms: Vec<Option<Geometry<f64>>> = outlets.features.iter().map(|f| f.geom.clone()).collect();
        if let (Some(from), Some(to)) = (outlets.crs, g.crs) {
            if from != to {
                let b = ctx.backend("Reprojecting the outlets")?;
                geom::transform_all(geoms.iter_mut().flatten(), |xs, ys| b.transform(from, to, xs, ys))?;
            }
        }
        let snap = number_param(p, "snap", "Move outlets")?.unwrap_or(50.0).max(0.0);
        let (cw, ch) = (g.geotransform[1].abs(), g.geotransform[5].abs());
        let (sc, sr) = ((snap / cw).ceil() as isize, (snap / ch).ceil() as isize);
        let mut outside = 0usize;
        for (k, geom) in geoms.iter().enumerate() {
            let Some(geom) = geom else { continue };
            let mut idx = Vec::new();
            cells.of_geometry(&Geometry::Point(geo::Point(geom::coords(geom)[0])), &mut idx);
            let Some(&at) = idx.first() else {
                outside += 1;
                continue;
            };
            let (c0, r0) = ((at % w) as isize, (at / w) as isize);
            let mut best = at;
            for dr in -sr..=sr {
                for dc in -sc..=sc {
                    let (c, r) = (c0 + dc, r0 + dr);
                    if c < 0 || r < 0 || c >= w as isize || r >= h as isize || ((dc as f64 * cw).powi(2) + (dr as f64 * ch).powi(2)).sqrt() > snap {
                        continue;
                    }
                    let j = r as usize * w + c as usize;
                    if !g.values[j].is_nan() && acc[j] > acc[best] {
                        best = j;
                    }
                }
            }
            sources.push((best, Some(k)));
        }
        if outside > 0 {
            ctx.notes.warn(format!("{} outlets lie outside the terrain and were left out.", fmt_count(outside as u64)));
        }
    } else {
        let smallest = number_param(p, "smallest", "Without outlets, smallest")?.unwrap_or(1.0);
        for i in 0..w * h {
            if down[i].is_none() && !g.values[i].is_nan() && km2(acc[i]) >= smallest {
                sources.push((i, None));
            }
        }
        sources.sort_by_key(|s| std::cmp::Reverse(acc[s.0]));
    }
    if sources.is_empty() {
        return err("No catchment is that large. Make Without outlets, smallest smaller, or connect outlets to Outlets.");
    }
    // Upstream from each outlet; an outlet inside the catchment of another, larger one takes
    // its own part, as nested catchments are usually drawn.
    let mut ups: Vec<Vec<u32>> = vec![Vec::new(); w * h];
    for (i, d) in down.iter().enumerate() {
        if let Some(j) = d {
            ups[*j as usize].push(i as u32);
        }
    }
    let mut order: Vec<usize> = (0..sources.len()).collect();
    order.sort_by_key(|k| acc[sources[*k].0]);
    let mut sizes = vec![0u32; sources.len()];
    for k in order {
        let start = sources[k].0;
        if label[start] != 0 {
            continue;
        }
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            if label[i] != 0 {
                continue;
            }
            label[i] = k as u32 + 1;
            sizes[k] += 1;
            stack.extend(ups[i].iter().map(|u| *u as usize));
        }
    }
    // The catchments as areas, through the cells that carry their number.
    let labels = Grid { values: label.iter().map(|l| if *l == 0 { f32::NAN } else { *l as f32 }).collect(), bands: Vec::new(), ..g.clone() };
    let polys = ctx.backend("Catchments")?.polygonize(&labels, "catchment", false)?;
    let mut areas = Table {
        fields: vec![Field::new("catchment", FieldType::Int), Field::new("area_km2", FieldType::Real), Field::new("outlet_flow_km2", FieldType::Real)],
        crs: g.crs,
        ..Default::default()
    };
    for f in &outlets.fields {
        if areas.field_index(&f.name).is_none() {
            areas.fields.push(f.clone());
        }
    }
    let mut parts: HashMap<usize, Vec<geo::Polygon<f64>>> = HashMap::new();
    for f in &polys.features {
        let k = f.attrs.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as usize;
        if k == 0 {
            continue;
        }
        parts.entry(k - 1).or_default().extend(f.geom.as_ref().map(geom::polygons_of).unwrap_or_default());
    }
    for (k, (cell, from)) in sources.iter().enumerate() {
        let Some(ps) = parts.remove(&k) else { continue };
        let mut attrs =
            vec![Value::Int(k as i64 + 1), Value::Real((km2(sizes[k]) * 1000.0).round() / 1000.0), Value::Real((km2(acc[*cell]) * 1000.0).round() / 1000.0)];
        attrs.resize(areas.fields.len(), Value::Null);
        if let Some(fi) = from {
            for (f, v) in outlets.fields.iter().zip(&outlets.features[*fi].attrs) {
                if let Some(at) = areas.field_index(&f.name) {
                    if at >= 3 {
                        attrs[at] = v.clone();
                    }
                }
            }
        }
        let geom = if ps.len() == 1 { Geometry::Polygon(ps.into_iter().next().unwrap()) } else { Geometry::MultiPolygon(geo::MultiPolygon::new(ps)) };
        areas.features.push(Feature::new(attrs, Some(geom)));
    }
    // The streams: each stream cell joined to the one it drains to, in lines from a source or
    // a confluence to the next.
    let stream_km2 = number_param(p, "stream", "Streams from")?.unwrap_or(1.0).max(0.0);
    let is_stream = |i: usize| km2(acc[i]) >= stream_km2 && !g.values[i].is_nan();
    let mut stream_ins = vec![0u8; w * h];
    for (i, d) in down.iter().enumerate() {
        if let (true, Some(j)) = (is_stream(i), d) {
            stream_ins[*j as usize] = stream_ins[*j as usize].saturating_add(1);
        }
    }
    let mut streams = Table { fields: vec![Field::new("flow_km2", FieldType::Real), Field::new("order", FieldType::Int)], crs: g.crs, ..Default::default() };
    // Strahler order, from the sources down.
    let mut strahler = vec![0u8; w * h];
    let mut seen_in = vec![(0u8, 0u8); w * h]; // (highest order coming in, how many with it)
    let mut ready: Vec<usize> = (0..w * h).filter(|i| is_stream(*i) && stream_ins[*i] == 0).collect();
    let mut left = stream_ins.clone();
    let order_of = |i: usize, strahler: &mut Vec<u8>, seen_in: &Vec<(u8, u8)>| {
        let (hi, n) = seen_in[i];
        strahler[i] = if hi == 0 {
            1
        } else if n >= 2 {
            hi + 1
        } else {
            hi
        };
    };
    while let Some(i) = ready.pop() {
        order_of(i, &mut strahler, &seen_in);
        if let Some(j) = down[i] {
            let j = j as usize;
            if is_stream(j) {
                let o = strahler[i];
                let s = &mut seen_in[j];
                if o > s.0 {
                    *s = (o, 1);
                } else if o == s.0 {
                    s.1 += 1;
                }
                left[j] -= 1;
                if left[j] == 0 {
                    ready.push(j);
                }
            }
        }
    }
    for i in 0..w * h {
        // A line starts at a source or just below a confluence, and goes on until the next.
        if !is_stream(i) || stream_ins[i] == 1 {
            continue;
        }
        let mut line = vec![geo::Coord::from(g.center(i % w, i / w))];
        let mut at = i;
        while let Some(j) = down[at] {
            let j = j as usize;
            line.push(geo::Coord::from(g.center(j % w, j / w)));
            if !is_stream(j) || stream_ins[j] != 1 || strahler[j] != strahler[i] {
                break;
            }
            at = j;
        }
        if line.len() >= 2 {
            streams.features.push(Feature::new(
                vec![Value::Real((km2(acc[at]) * 1000.0).round() / 1000.0), Value::Int(strahler[i] as i64)],
                Some(Geometry::LineString(geo::LineString::new(line))),
            ));
        }
    }
    let flow = Grid {
        values: acc.iter().zip(&g.values).map(|(a, v)| if v.is_nan() { f32::NAN } else { (*a as f64 * cell_area) as f32 }).collect(),
        bands: Vec::new(),
        ..g.clone()
    };
    ctx.notes.info(format!("{} catchments, {} stream lines", fmt_count(areas.len() as u64), fmt_count(streams.len() as u64)));
    Ok(vec![("Catchments".into(), areas), ("Streams".into(), streams), ("Flow".into(), flow.into_table())])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "PointInterpolator",
        title: "Interpolate points to raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Makes a raster of values between points: heights from measured points, depths from soundings, noise or temperature from where it was measured. Linear in triangles (TIN), smooth by natural neighbours, by inverse distance, or the nearest point, at a cell size or on the cells of a raster on Like.",
        inputs: &["Points", "Like"],
        outputs: &["Raster"],
        ports: None,
        params: INTERPOLATE_PARAMS,
        run: run_interpolate,
        summary: |p| p.get("method").unwrap_or_else(|| "tin".into()),
    },
    NodeType {
        name: "RasterResampler",
        title: "Resample raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Gives a raster other cells: a cell size, or the cells of a raster on Like, so that two rasters line up cell by cell for Raster calculator. The cell under the middle for classes, between the nearest for heights, or the mean or most common value to make it coarser.",
        inputs: &["Raster", "Like"],
        outputs: &["Raster"],
        ports: None,
        params: RESAMPLE_PARAMS,
        run: run_resample,
        summary: |p| p.get("method").unwrap_or_else(|| "nearest".into()),
    },
    NodeType {
        name: "RasterFiller",
        title: "Fill empty cells",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Fills the empty cells of a raster: holes in a terrain model under water or buildings, gaps between scans. From the cells around, weighted by how near they are, or with one value.",
        inputs: &["Raster"],
        outputs: &["Raster"],
        ports: None,
        params: FILL_PARAMS,
        run: run_fill,
        summary: |p| if p.get("method").as_deref() == Some("value") { format!("with {}", p.get("value").unwrap_or_else(|| "0".into())) } else { String::new() },
    },
    NodeType {
        name: "Watershed",
        title: "Catchments and streams",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "Follows the water over a terrain model: the catchment above each outlet on Outlets, or the areas that drain off the terrain, as polygons with their area; the streams, with their order (Strahler) and the land they drain; and the flow, the area upstream of each cell, as a raster. Hollows are filled first, so that all water runs off.",
        inputs: &["Heights", "Outlets"],
        outputs: &["Catchments", "Streams", "Flow"],
        ports: None,
        params: WATERSHED_PARAMS,
        run: run_watershed,
        summary: |_| String::new(),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn points() -> Table {
        // Heights on a plane z = x + 2y, at the corners and the middle of a 100 m square.
        let pts = [(0.0, 0.0), (100.0, 0.0), (0.0, 100.0), (100.0, 100.0), (50.0, 50.0)];
        Table {
            fields: vec![Field::new("depth", FieldType::Real)],
            crs: Some(3006),
            features: pts
                .iter()
                .map(|(x, y)| Feature {
                    attrs: vec![Value::Real(x * 0.1)],
                    geom: Some(Geometry::Point(geo::Point::new(*x, *y))),
                    z: Some(Arc::from(vec![x + 2.0 * y])),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn points_are_interpolated_and_rasters_resampled_and_filled() {
        let (out, _) = run_for_test("PointInterpolator", &[("cell", "10")], vec![("Points", points())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!((g.width, g.height), (10, 10));
        // A plane is given back exactly by the triangles.
        let v = g.at(25.0, 35.0).unwrap();
        assert!((v - (25.0 + 70.0)).abs() < 1e-3, "{}", v);
        for m in ["natural", "idw", "nearest"] {
            let (out, _) = run_for_test("PointInterpolator", &[("cell", "10"), ("method", m), ("value", "depth")], vec![("Points", points())]).unwrap();
            let g = out[0].1.grid.clone().unwrap();
            let v = g.at(95.0, 95.0).unwrap();
            assert!((9.0..=10.0).contains(&v), "{} gives {}", m, v);
        }
        // Resample to 20 m: the mean of four cells, and the cell under the middle.
        let fine = Grid {
            width: 4,
            height: 2,
            values: vec![1.0, 3.0, 5.0, 7.0, 1.0, 3.0, 5.0, 7.0],
            geotransform: [0.0, 10.0, 0.0, 20.0, 0.0, -10.0],
            crs: None,
            bands: Vec::new(),
        };
        let mean = resample_to(&fine, 2, 1, [0.0, 20.0, 0.0, 20.0, 0.0, -20.0], "mean");
        assert_eq!(mean.values, [2.0, 6.0]);
        let (out, _) = run_for_test("RasterResampler", &[("cell", "5"), ("method", "nearest")], vec![("Raster", fine.clone().into_table())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!((g.width, g.height, g.get(2, 0)), (8, 4, Some(3.0)));
        // A hole filled from around it.
        let mut holed = fine.clone();
        holed.values[1] = f32::NAN;
        let (filled, n) = fill_nearby(&holed, 2);
        assert_eq!(n, 1);
        assert!(filled.values[1] > 1.0 && filled.values[1] < 7.0);
    }

    #[test]
    fn water_runs_down_into_catchments_and_streams() {
        // A valley along the middle, falling to the south: two slopes drain into it.
        let (w, h) = (21usize, 30usize);
        let values = (0..w * h).map(|i| ((i % w) as f32 - 10.0).abs() * 2.0 + (h - i / w) as f32 * 0.5).collect();
        let g = Grid { width: w, height: h, values, geotransform: [0.0, 10.0, 0.0, 300.0, 0.0, -10.0], crs: Some(3006), bands: Vec::new() };
        let down = flow_directions(&g);
        let acc = accumulation(&down);
        // The bottom of the valley gathers the most.
        let bottom = (h - 1) * w + 10;
        assert_eq!(acc.iter().max(), Some(&acc[bottom]));
        assert!(acc[bottom] as usize > w * h / 3, "{}", acc[bottom]);
        // A hollow is filled and drains over its rim.
        let mut pit = g.clone();
        pit.values[15 * w + 10] = -5.0;
        let down = flow_directions(&pit);
        let acc2 = accumulation(&down);
        assert!(acc2[bottom] >= acc[bottom] - 1, "{} {}", acc2[bottom], acc[bottom]);
    }
}
