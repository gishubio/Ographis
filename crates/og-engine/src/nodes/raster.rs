//! Tools for rasters: Merge rasters, Generalize raster, Raster calculator and Contour lines.

use super::*;
use crate::model::{crs_label, err, fmt_count, Grid, RasterImage, Result, Table};

// ---------- Merge rasters ----------

pub(super) const MOSAIC_PARAMS: &[ParamSpec] = &[];

/// Pictures put together into one, in the coordinate system of the first. Where they
/// overlap, the one connected first is on top. The pixels are those of the finest picture,
/// made coarser if there would be more than `max_pixels`.
pub fn mosaic_images(images: &[&RasterImage], max_pixels: usize) -> Option<RasterImage> {
    let first = images.first()?;
    let px = |i: &RasterImage| (i.geotransform[1].abs()).min(i.geotransform[5].abs());
    let mut size = images.iter().map(|i| px(i)).fold(f64::INFINITY, f64::min);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for i in images {
        let t = &i.geotransform;
        let (ax, bx) = (t[0], t[0] + t[1] * i.width as f64);
        let (ay, by) = (t[3], t[3] + t[5] * i.height as f64);
        (x0, x1, y0, y1) = (x0.min(ax.min(bx)), x1.max(ax.max(bx)), y0.min(ay.min(by)), y1.max(ay.max(by)));
    }
    while ((x1 - x0) / size).ceil() * ((y1 - y0) / size).ceil() > max_pixels as f64 {
        size *= 1.25;
    }
    let _ = first;
    let (w, h) = (((x1 - x0) / size).ceil().max(1.0) as usize, ((y1 - y0) / size).ceil().max(1.0) as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        let y = y1 - (row as f64 + 0.5) * size;
        for col in 0..w {
            let x = x0 + (col as f64 + 0.5) * size;
            // The first picture with something here, from the top.
            for i in images {
                let t = &i.geotransform;
                let (c, r) = ((x - t[0]) / t[1], (y - t[3]) / t[5]);
                if c < 0.0 || r < 0.0 || c >= i.width as f64 || r >= i.height as f64 {
                    continue;
                }
                let k = (r as usize * i.width + c as usize) * 4;
                if i.rgba[k + 3] == 0 {
                    continue;
                }
                rgba[(row * w + col) * 4..][..4].copy_from_slice(&i.rgba[k..k + 4]);
                break;
            }
        }
    }
    Some(RasterImage { width: w, height: h, rgba, geotransform: [x0, size, 0.0, y1, 0.0, -size] })
}

pub(super) fn run_mosaic(_p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let parts: Vec<std::sync::Arc<Table>> = ctx.parts.iter().filter(|x| x.port == "Rasters").map(|x| x.table.clone()).collect();
    if parts.is_empty() {
        return err("Connect the rasters to put together to Rasters: pictures, or grids such as terrain models.");
    }
    let grids: Vec<&Grid> = parts.iter().filter_map(|t| t.grid.as_deref()).collect();
    let images: Vec<(&RasterImage, Option<u32>)> = parts.iter().filter_map(|t| t.image.as_deref().map(|i| (i, t.crs))).collect();
    if !grids.is_empty() && !images.is_empty() {
        return err("Pictures and grids cannot be put together. Connect only pictures, or only grids.");
    }
    if grids.len() + images.len() < parts.len() {
        ctx.notes.warn("Streams that are neither a picture nor a grid were left out.".to_string());
    }
    if !grids.is_empty() {
        let crs = grids[0].crs;
        let mut warped: Vec<Grid> = Vec::new();
        for g in &grids {
            if g.crs != crs {
                let to = crs.ok_or_else(|| crate::model::GbError("The first grid has no coordinate system.".into()))?;
                ctx.notes.warn(format!("A grid in {} was reprojected to {}.", crs_label(g.crs), crs_label(crs)));
                warped.push(ctx.backend("Reprojecting a grid")?.warp_grid(g, to)?);
            } else {
                warped.push((*g).clone());
            }
        }
        let refs: Vec<&Grid> = warped.iter().collect();
        let g = Grid::mosaic(&refs, super::GRID_CELLS).ok_or_else(|| crate::model::GbError("There is nothing to put together.".into()))?;
        ctx.notes.info(format!("{} grids became one of {} × {} cells", refs.len(), g.width, g.height));
        return Ok(vec![("Raster".into(), g.into_table())]);
    }
    if images.is_empty() {
        return err("There is no picture or grid on Rasters.");
    }
    let crs = images[0].1;
    let mut warped: Vec<RasterImage> = Vec::new();
    for (img, c) in &images {
        match (c, crs) {
            (Some(a), Some(b)) if *a != b => {
                ctx.notes.warn(format!("A picture in {} was reprojected to {}.", crs_label(Some(*a)), crs_label(Some(b))));
                warped.push(ctx.backend("Reprojecting a picture")?.warp_image(img, *a, b)?);
            }
            _ => warped.push((*img).clone()),
        }
    }
    let refs: Vec<&RasterImage> = warped.iter().collect();
    let img = mosaic_images(&refs, 60_000_000).ok_or_else(|| crate::model::GbError("There is nothing to put together.".into()))?;
    ctx.notes.info(format!("{} pictures became one of {} × {} pixels", refs.len(), img.width, img.height));
    Ok(vec![("Raster".into(), img.into_table(crs))])
}

// ---------- Contour lines ----------

pub(super) const CONTOUR_KINDS: &[(&str, &str)] = &[("lines", "Lines"), ("areas", "Areas between the lines")];

pub(super) const CONTOUR_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "interval",
        label: "Interval",
        kind: ParamKind::Text,
        default: "10",
        hint: "The height between two lines, such as 5 for every five metres",
    },
    ParamSpec { key: "base", label: "Start at", kind: ParamKind::Text, default: "0", hint: "A height that has a line. The others are whole intervals from it" },
    ParamSpec { key: "kind", label: "Make", kind: ParamKind::Choice(CONTOUR_KINDS), default: "lines", hint: "" },
    ParamSpec {
        key: "name",
        label: "Height attribute",
        kind: ParamKind::Text,
        default: "height",
        hint: "The height of each line. Areas get it with _min and _max after it",
    },
];

pub(super) fn run_contours(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let heights = input(inp, "Heights");
    let Some(grid) = heights.grid.as_deref() else {
        return err("The stream on Heights has no grid. Read the terrain model with Cells as set to Heights or other values on a grid.");
    };
    let interval = number_param(p, "interval", "The interval")?.unwrap_or(10.0);
    if interval <= 0.0 {
        return err("The interval must be more than 0.");
    }
    let base = number_param(p, "base", "Start at")?.unwrap_or(0.0);
    if let Some((lo, hi)) = grid.range() {
        let lines = ((hi - lo) as f64 / interval).ceil();
        if lines > 5000.0 {
            return err(format!("That would be {} levels between {} and {}. Choose a larger interval.", lines, lo, hi));
        }
    }
    let name = p.get("name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "height".into());
    let areas = p.get("kind").as_deref() == Some("areas");
    let b = ctx.backend("Contour lines")?;
    let t = b.contours(grid, interval, base, areas, &name)?;
    ctx.notes.info(format!("{} {}", fmt_count(t.len() as u64), if areas { "areas" } else { "lines" }));
    Ok(vec![("Contours".into(), t)])
}

// ---------- Generalize raster (RasterGeneralizer) ----------

pub(super) const GENERALIZE_METHODS: &[(&str, &str)] = &[
    ("mean", "Mean"),
    ("majority", "Majority: the most common value, for classes"),
    ("median", "Median"),
    ("min", "Smallest"),
    ("max", "Largest"),
    ("sum", "Sum"),
];

pub(super) const GENERALIZE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "factor",
        label: "Cells into one",
        kind: ParamKind::Text,
        default: "2",
        hint: "How many cells across and down become one: 4 makes 1 m cells into 4 m cells, sixteen into one",
    },
    ParamSpec {
        key: "method",
        label: "The new value",
        kind: ParamKind::Choice(GENERALIZE_METHODS),
        default: "mean",
        hint: "Empty cells are left out. A new cell is empty only when all its cells are",
    },
];

/// One value from the values of the cells that become one. None when there are none.
fn combine(values: &mut [f32], method: &str) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    Some(match method {
        "min" => values.iter().copied().fold(f32::INFINITY, f32::min),
        "max" => values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        "sum" => values.iter().map(|v| *v as f64).sum::<f64>() as f32,
        "median" => {
            values.sort_by(|a, b| a.total_cmp(b));
            let m = values.len() / 2;
            if values.len() % 2 == 1 {
                values[m]
            } else {
                (values[m - 1] + values[m]) / 2.0
            }
        }
        "majority" => {
            // The most common; between equally common, the smallest, so that it is the same
            // from run to run.
            values.sort_by(|a, b| a.total_cmp(b));
            let (mut best, mut best_n) = (values[0], 0usize);
            let mut i = 0;
            while i < values.len() {
                let mut j = i;
                while j < values.len() && values[j] == values[i] {
                    j += 1;
                }
                if j - i > best_n {
                    (best, best_n) = (values[i], j - i);
                }
                i = j;
            }
            best
        }
        _ => (values.iter().map(|v| *v as f64).sum::<f64>() / values.len() as f64) as f32,
    })
}

/// A grid with n × n cells made into one.
pub fn generalize_grid(g: &Grid, n: usize, method: &str) -> Grid {
    let (w, h) = (g.width.div_ceil(n), g.height.div_ceil(n));
    let band = |values: &[f32]| -> Vec<f32> {
        use rayon::prelude::*;
        (0..h)
            .into_par_iter()
            .flat_map_iter(|row| {
                let mut cell = Vec::with_capacity(n * n);
                (0..w)
                    .map(|col| {
                        cell.clear();
                        for r in row * n..((row + 1) * n).min(g.height) {
                            for c in col * n..((col + 1) * n).min(g.width) {
                                let v = values[r * g.width + c];
                                if !v.is_nan() {
                                    cell.push(v);
                                }
                            }
                        }
                        combine(&mut cell, method).unwrap_or(f32::NAN)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let t = &g.geotransform;
    let k = n as f64;
    Grid {
        width: w,
        height: h,
        values: band(&g.values),
        bands: g.bands.iter().map(|b| band(b)).collect(),
        geotransform: [t[0], t[1] * k, t[2] * k, t[3], t[4] * k, t[5] * k],
        crs: g.crs,
    }
}

/// A picture with n × n pixels made into one, by their mean colour.
fn generalize_image(img: &RasterImage, n: usize) -> RasterImage {
    let (w, h) = (img.width.div_ceil(n), img.height.div_ceil(n));
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for r in row * n..((row + 1) * n).min(img.height) {
                for c in col * n..((col + 1) * n).min(img.width) {
                    let k = (r * img.width + c) * 4;
                    let a = img.rgba[k + 3] as u64;
                    // Weighted by how opaque, so that empty pixels do not darken the edge.
                    for (i, s) in sum.iter_mut().enumerate().take(3) {
                        *s += img.rgba[k + i] as u64 * a;
                    }
                    sum[3] += a;
                    count += 1;
                }
            }
            let o = (row * w + col) * 4;
            if let Some(r) = sum[0].checked_div(sum[3]) {
                rgba[o] = r as u8;
                rgba[o + 1] = (sum[1] / sum[3]) as u8;
                rgba[o + 2] = (sum[2] / sum[3]) as u8;
                rgba[o + 3] = (sum[3] / count.max(1)) as u8;
            }
        }
    }
    let t = &img.geotransform;
    let k = n as f64;
    RasterImage { width: w, height: h, rgba, geotransform: [t[0], t[1] * k, t[2] * k, t[3], t[4] * k, t[5] * k] }
}

pub(super) fn run_raster_generalizer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Raster");
    let n = match number_param(p, "factor", "Cells into one")? {
        None => 2,
        Some(f) if f >= 1.0 && f.fract() == 0.0 && f <= 10_000.0 => f as usize,
        Some(_) => return err("Cells into one must be a whole number of 1 or more, such as 2 or 4."),
    };
    let method = p.get("method").unwrap_or_else(|| "mean".into());
    let grid = t.grid.as_deref().map(|g| generalize_grid(g, n, &method));
    let image = t.image.as_deref().map(|i| generalize_image(i, n));
    let out = match (grid, image) {
        (None, None) => return err("The stream on Raster has no raster. Read a GeoTIFF or another raster, or connect a tool that makes one."),
        (Some(g), None) => {
            ctx.notes.info(format!("{} × {} cells of {}", g.width, g.height, crate::model::fmt_real(g.cell_size().0)));
            g.into_table()
        }
        (g, Some(i)) => {
            if g.is_none() && method != "mean" {
                ctx.notes.warn("A picture has colours rather than values, so its pixels are put together by their mean colour.");
            }
            i.into_table_with(t.crs, g)
        }
    };
    Ok(vec![("Raster".into(), out)])
}

// ---------- Raster calculator (RasterCalculator) ----------

/// A value of a grid as it is worth showing: f32 holds about seven digits.
fn short(v: f32) -> String {
    crate::model::fmt_real((v as f64 * 1000.0).round() / 1000.0)
}

pub(super) const CALC_EXTENTS: &[(&str, &str)] = &[("overlap", "Where they all overlap"), ("first", "That of A, the first")];
pub(super) const CALC_EMPTY: &[(&str, &str)] = &[("empty", "Make the result empty"), ("zero", "Count as 0")];

pub(super) const CALC_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "expression",
        label: "Expression",
        kind: ParamKind::Text,
        default: "A + B",
        hint: "A is the raster connected first, B the second, and so on. Such as (A - B) / A * 100, if(A > 100, B, null) or max(A, B, C). Band 2 of A is A_2",
    },
    ParamSpec {
        key: "extent",
        label: "The result covers",
        kind: ParamKind::Choice(CALC_EXTENTS),
        default: "overlap",
        hint: "Its cells are those of A. The other rasters are reprojected and read at the middle of each",
    },
    ParamSpec {
        key: "empty",
        label: "An empty cell",
        kind: ParamKind::Choice(CALC_EMPTY),
        default: "empty",
        hint: "Where a raster has no value. coalesce(B, 0) does the same for one of them",
    },
];

pub(super) fn run_raster_calculator(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use rayon::prelude::*;
    let parts: Vec<std::sync::Arc<Table>> = ctx.parts.iter().filter(|x| x.port == "Rasters").map(|x| x.table.clone()).collect();
    if parts.is_empty() {
        return err("Connect the rasters to Rasters. The first is A in the expression, the second B, and so on.");
    }
    if parts.len() > 26 {
        return err("At most 26 rasters, A to Z.");
    }
    let mut grids: Vec<Grid> = Vec::new();
    for (k, t) in parts.iter().enumerate() {
        let letter = (b'A' + k as u8) as char;
        let Some(g) = t.grid.as_deref() else {
            return err(format!("{}, the raster connected as number {}, has no values on a grid. A picture of colours cannot be counted with.", letter, k + 1));
        };
        grids.push(g.clone());
    }
    // The others in the coordinate system of A.
    let crs = grids[0].crs;
    for (k, grid) in grids.iter_mut().enumerate().skip(1) {
        if grid.crs != crs {
            let to = crs.ok_or_else(|| crate::model::GbError("A has no coordinate system, so the others cannot be put on it.".into()))?;
            ctx.notes.warn(format!("{} was reprojected from {} to {}.", (b'A' + k as u8) as char, crs_label(grid.crs), crs_label(crs)));
            *grid = ctx.backend("Reprojecting a grid")?.warp_grid(grid, to)?;
        }
    }
    // The names: A, B, … and A_2 for band 2, also in small letters.
    let mut names: Vec<String> = Vec::new();
    let mut sources: Vec<(usize, usize)> = Vec::new();
    for (k, g) in grids.iter().enumerate() {
        let letter = (b'A' + k as u8) as char;
        for band in 0..=g.bands.len() {
            let name = if band == 0 { letter.to_string() } else { format!("{}_{}", letter, band + 1) };
            names.push(name.to_lowercase());
            sources.push((k, band));
            names.push(name);
            sources.push((k, band));
        }
    }
    let src = p.need("expression", "Give an expression, such as A + B.")?;
    let e = crate::expr::compile(&src, &names)?;
    let zero = p.get("empty").as_deref() == Some("zero");
    let a = &grids[0];
    let t = a.geotransform;
    if t[2] != 0.0 || t[4] != 0.0 {
        return err("A is turned, which the raster calculator cannot work with yet.");
    }
    // The cells of A to count: all, or those where every raster is.
    let (mut c0, mut r0, mut c1, mut r1) = (0usize, 0usize, a.width, a.height);
    if p.get("extent").as_deref() != Some("first") {
        for g in &grids[1..] {
            let gt = g.geotransform;
            let (gx0, gx1) = (gt[0].min(gt[0] + gt[1] * g.width as f64), gt[0].max(gt[0] + gt[1] * g.width as f64));
            let (gy0, gy1) = (gt[3].min(gt[3] + gt[5] * g.height as f64), gt[3].max(gt[3] + gt[5] * g.height as f64));
            let col = |x: f64| (x - t[0]) / t[1];
            let row = |y: f64| (y - t[3]) / t[5];
            let (ca, cb) = (col(gx0).min(col(gx1)), col(gx0).max(col(gx1)));
            let (ra, rb) = (row(gy0).min(row(gy1)), row(gy0).max(row(gy1)));
            c0 = c0.max(ca.floor().max(0.0) as usize);
            c1 = c1.min(cb.ceil().max(0.0) as usize);
            r0 = r0.max(ra.floor().max(0.0) as usize);
            r1 = r1.min(rb.ceil().max(0.0) as usize);
        }
        if c0 >= c1 || r0 >= r1 {
            return err("The rasters do not overlap. Choose That of A to count over the whole of A.");
        }
    }
    let (w, h) = (c1 - c0, r1 - r0);
    let values: Vec<std::result::Result<Vec<f32>, crate::model::GbError>> = (r0..r1)
        .into_par_iter()
        .map(|row| {
            let mut f = crate::model::Feature { z: None, attrs: vec![Value::Null; names.len()], geom: None };
            let y = t[3] + (row as f64 + 0.5) * t[5];
            let mut out = Vec::with_capacity(w);
            for col in c0..c1 {
                let x = t[0] + (col as f64 + 0.5) * t[1];
                for (slot, (k, band)) in sources.iter().enumerate() {
                    let g = &grids[*k];
                    let v = if *k == 0 {
                        let i = row * g.width + col;
                        if *band == 0 {
                            g.values[i]
                        } else {
                            g.bands[band - 1][i]
                        }
                    } else {
                        match g.cell_at(x, y) {
                            Some((c, r, _)) => {
                                let i = r * g.width + c;
                                if *band == 0 {
                                    g.values[i]
                                } else {
                                    g.bands[band - 1][i]
                                }
                            }
                            None => f32::NAN,
                        }
                    };
                    f.attrs[slot] = if v.is_nan() {
                        if zero {
                            Value::Real(0.0)
                        } else {
                            Value::Null
                        }
                    } else {
                        Value::Real(v as f64)
                    };
                }
                let index = (row - r0) * w + (col - c0);
                let v = e.eval(&crate::expr::Row { feature: &f, index, geographic: None })?;
                out.push(match v {
                    Value::Bool(b) => b as u8 as f32,
                    other => other.as_f64().map_or(f32::NAN, |x| x as f32),
                });
            }
            Ok(out)
        })
        .collect();
    let mut cells = Vec::with_capacity(w * h);
    for row in values {
        cells.extend(row?);
    }
    let g = Grid {
        width: w,
        height: h,
        values: cells,
        bands: Vec::new(),
        geotransform: [t[0] + c0 as f64 * t[1], t[1], 0.0, t[3] + r0 as f64 * t[5], 0.0, t[5]],
        crs,
    };
    match g.range() {
        Some((lo, hi)) => ctx.notes.info(format!("{} × {} cells from {} to {}", w, h, short(lo), short(hi))),
        None => ctx.notes.warn("Every cell of the result is empty. An empty cell in any raster makes the result empty, unless Count as 0 is chosen."),
    }
    Ok(vec![("Raster".into(), g.into_table())])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "RasterMosaicker",
        title: "Merge rasters",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Puts the rasters connected to Rasters together into one: pictures, such as tiles of an orthophoto, or grids, such as tiles of a terrain model. Where they overlap, the one connected first is on top. Rasters in another coordinate system are reprojected to that of the first.",
        inputs: &["Rasters"],
        outputs: &["Raster"],
        ports: None,
        params: MOSAIC_PARAMS,
        run: run_mosaic,
        summary: |_| String::new(),
    },
    NodeType {
        name: "RasterGeneralizer",
        title: "Generalize raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Makes a raster coarser: a number of cells across and down become one, with their mean, the most common value for classes such as land cover, the median, the smallest, the largest or the sum. Pictures get the mean colour.",
        inputs: &["Raster"],
        outputs: &["Raster"],
        ports: None,
        params: GENERALIZE_PARAMS,
        run: run_raster_generalizer,
        summary: |p| format!("{} × {}, {}", p.get("factor").unwrap_or_else(|| "2".into()), p.get("factor").unwrap_or_else(|| "2".into()), p.get("method").unwrap_or_else(|| "mean".into())),
    },
    NodeType {
        name: "RasterCalculator",
        title: "Raster calculator",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Counts cell by cell with the rasters connected to Rasters, A the first and B the second: A + B, A - B, A * B, A / B, and anything an expression can do, such as if(A > 100, B, null), max(A, B) or sqrt(A). The result has the cells of A, where the rasters overlap.",
        inputs: &["Rasters"],
        outputs: &["Raster"],
        ports: None,
        params: CALC_PARAMS,
        run: run_raster_calculator,
        summary: |p| p.get("expression").unwrap_or_else(|| "A + B".into()),
    },
    NodeType {
        name: "ContourGenerator",
        title: "Contour lines",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "Draws contour lines from the heights on Heights, at an interval such as every 5 metres, with the height in an attribute. Can give the areas between the lines instead.",
        inputs: &["Heights"],
        outputs: &["Contours"],
        ports: None,
        params: CONTOUR_PARAMS,
        run: run_contours,
        summary: |p| format!("every {}", p.get("interval").unwrap_or_else(|| "10".into())),
    },
];
