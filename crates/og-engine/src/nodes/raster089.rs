//! Tools for rasters added in 0.89: values from a raster, zonal statistics, slope and aspect,
//! clip raster, raster to areas, features to raster, reclassify, distance and visibility.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Grid, RasterImage};
use geo::Geometry;

// ---------- Shared ----------

/// The grid on a port, or a word on what to connect.
fn grid_on<'a>(t: &'a Table, port: &str) -> Result<&'a Grid> {
    match t.grid.as_deref() {
        Some(g) if g.is_turned() => err(format!("The raster on {} is turned, which this tool cannot work with yet.", port)),
        Some(g) => Ok(g),
        None if t.image.is_some() => {
            err(format!("The raster on {} is a picture of colours, not values. Read it with Cells as set to Heights or other values on a grid.", port))
        }
        None if t.features.is_empty() => err(format!("Nothing came in on {}. Connect a raster, such as a GeoTIFF read as a grid of values.", port)),
        None => err(format!("The stream on {} has no raster. Read a GeoTIFF with Cells as set to Heights or other values on a grid.", port)),
    }
}

/// The geometries of a stream in another coordinate system, for looking them up in a raster.
fn geoms_in(t: &Table, to: Option<u32>, ctx: &NodeCtx) -> Result<Vec<Option<Geometry<f64>>>> {
    let mut geoms: Vec<Option<Geometry<f64>>> = t.features.iter().map(|f| f.geom.clone()).collect();
    if let (Some(from), Some(to)) = (t.crs, to) {
        if from != to {
            let b = ctx.backend("Reprojecting to the raster")?;
            geom::transform_all(geoms.iter_mut().flatten(), |xs, ys| b.transform(from, to, xs, ys))?;
        }
    }
    Ok(geoms)
}

/// The cells of a raster: its size and where it lies, as a grid or a picture has them.
#[derive(Clone, Copy, Debug)]
pub struct Cells {
    pub width: usize,
    pub height: usize,
    pub gt: [f64; 6],
}

impl Cells {
    pub fn of(g: &Grid) -> Cells {
        Cells { width: g.width, height: g.height, gt: g.geotransform }
    }
    fn of_image(i: &RasterImage) -> Cells {
        Cells { width: i.width, height: i.height, gt: i.geotransform }
    }
    /// The column and row a place lies in, as numbers with decimals.
    fn place(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.gt[0]) / self.gt[1], (y - self.gt[3]) / self.gt[5])
    }
    fn cell(&self, x: f64, y: f64) -> Option<usize> {
        let (c, r) = self.place(x, y);
        (c >= 0.0 && r >= 0.0 && (c as usize) < self.width && (r as usize) < self.height).then(|| r as usize * self.width + c as usize)
    }

    /// The cells whose middles lie in the areas of a geometry, each once, row by row. Lines
    /// give the cells they pass through, and points the cell they lie in.
    pub fn of_geometry(&self, g: &Geometry<f64>, out: &mut Vec<usize>) {
        let polys = geom::polygons_of(g);
        if !polys.is_empty() {
            self.fill(&polys, out);
            return;
        }
        let start = out.len();
        let step = self.gt[1].abs().min(self.gt[5].abs()) / 3.0;
        match g {
            Geometry::Point(p) => out.extend(self.cell(p.x(), p.y())),
            Geometry::MultiPoint(m) => out.extend(m.iter().filter_map(|p| self.cell(p.x(), p.y()))),
            _ => {
                let lines: Vec<geo::LineString<f64>> = match g {
                    Geometry::LineString(l) => vec![l.clone()],
                    Geometry::MultiLineString(m) => m.0.clone(),
                    Geometry::Line(l) => vec![geo::LineString::from(vec![l.start, l.end])],
                    Geometry::GeometryCollection(c) => {
                        for g in c {
                            self.of_geometry(g, out);
                        }
                        Vec::new()
                    }
                    _ => Vec::new(),
                };
                for l in lines {
                    for s in l.lines() {
                        let (dx, dy) = (s.end.x - s.start.x, s.end.y - s.start.y);
                        let n = ((dx * dx + dy * dy).sqrt() / step).ceil().max(1.0) as usize;
                        for k in 0..=n {
                            let t = k as f64 / n as f64;
                            out.extend(self.cell(s.start.x + dx * t, s.start.y + dy * t));
                        }
                    }
                }
            }
        }
        let mut found: Vec<usize> = out.split_off(start);
        found.sort_unstable();
        found.dedup();
        out.extend(found);
    }

    /// The cells whose middles lie inside the polygons, holes left out, found row by row
    /// where the rings cross the middle of the row.
    fn fill(&self, polys: &[geo::Polygon<f64>], out: &mut Vec<usize>) {
        use geo::BoundingRect;
        let mut edges: Vec<(geo::Coord<f64>, geo::Coord<f64>)> = Vec::new();
        let (mut ymin, mut ymax) = (f64::MAX, f64::MIN);
        for p in polys {
            if let Some(r) = p.bounding_rect() {
                ymin = ymin.min(r.min().y);
                ymax = ymax.max(r.max().y);
            }
            for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
                edges.extend(ring.lines().map(|l| (l.start, l.end)));
            }
        }
        if edges.is_empty() {
            return;
        }
        let rows = |y: f64| (y - self.gt[3]) / self.gt[5] - 0.5;
        let (ra, rb) = (rows(ymin), rows(ymax));
        let r0 = ra.min(rb).ceil().max(0.0) as usize;
        let r1 = (ra.max(rb).floor() + 1.0).clamp(0.0, self.height as f64) as usize;
        let mut xs: Vec<f64> = Vec::new();
        for r in r0..r1 {
            let y = self.gt[3] + (r as f64 + 0.5) * self.gt[5];
            xs.clear();
            for (a, b) in &edges {
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks_exact(2) {
                let col = |x: f64| (x - self.gt[0]) / self.gt[1] - 0.5;
                let (ca, cb) = (col(pair[0]), col(pair[1]));
                let c0 = ca.min(cb).ceil().max(0.0) as usize;
                let c1 = (ca.max(cb).floor() + 1.0).clamp(0.0, self.width as f64) as usize;
                out.extend((c0..c1.max(c0)).map(|c| r * self.width + c));
            }
        }
    }
}

/// The cells of a new raster: those of a raster connected to Like, or cells of a size
/// over a rectangle, lined up with whole multiples of the size.
fn new_cells(p: &P, like: &Table, rect: Option<[f64; 4]>, crs: Option<u32>, ctx: &mut NodeCtx) -> Result<(Cells, Option<u32>)> {
    if let Some(g) = like.grid.as_deref() {
        if g.is_turned() {
            return err("The raster on Like is turned, which this tool cannot work with yet.");
        }
        return Ok((Cells::of(g), g.crs));
    }
    if let Some(i) = like.image.as_deref() {
        return Ok((Cells::of_image(i), like.crs));
    }
    let size = number_param(p, "cell", "Cell size")?.unwrap_or(10.0);
    if size <= 0.0 {
        return err("The cell size must be more than 0.");
    }
    let Some([x0, y0, x1, y1]) = rect else {
        return err("There is nothing to make a raster of: no feature has a geometry.");
    };
    let left = (x0 / size).floor() * size;
    let top = (y1 / size).ceil() * size;
    let width = ((x1 - left) / size).ceil().max(1.0) as usize;
    let height = ((top - y0) / size).ceil().max(1.0) as usize;
    if width * height > GRID_CELLS {
        return err(format!(
            "That would be {} × {} cells, more than the {} a raster may have. Choose larger cells.",
            fmt_count(width as u64),
            fmt_count(height as u64),
            fmt_count(GRID_CELLS as u64)
        ));
    }
    if ctx.geographic(crs) == Some(true) {
        ctx.notes.warn("The features are in degrees, so the cell size is in degrees too. Reproject them first to have cells in metres.");
    }
    Ok((Cells { width, height, gt: [left, size, 0.0, top, 0.0, -size] }, crs))
}

fn bounds(geoms: &[Option<Geometry<f64>>]) -> Option<[f64; 4]> {
    use geo::BoundingRect;
    let mut r: Option<[f64; 4]> = None;
    for g in geoms.iter().flatten() {
        if let Some(b) = g.bounding_rect() {
            let (lo, hi) = (b.min(), b.max());
            r = Some(match r {
                None => [lo.x, lo.y, hi.x, hi.y],
                Some(a) => [a[0].min(lo.x), a[1].min(lo.y), a[2].max(hi.x), a[3].max(hi.y)],
            });
        }
    }
    r
}

/// A point that lies on a line or in an area.
fn point_on(g: &Geometry<f64>) -> Option<geo::Coord<f64>> {
    match geom::center(g, geom::Center::Inside)? {
        Geometry::Point(p) => Some(p.0),
        _ => None,
    }
}

/// A value of a raster as it is worth showing.
fn short(v: f64) -> String {
    fmt_real((v * 1000.0).round() / 1000.0)
}

fn note_range(g: &Grid, ctx: &mut NodeCtx, what: &str) {
    match g.range() {
        Some((lo, hi)) => ctx.notes.info(format!(
            "{} × {} cells of {}, {} from {} to {}",
            g.width,
            g.height,
            short(g.cell_size().0),
            what,
            short(lo as f64),
            short(hi as f64)
        )),
        None => ctx.notes.warn("Every cell of the result is empty."),
    }
}

// ---------- Values from raster (RasterSampler) ----------

const SAMPLE_METHODS: &[(&str, &str)] =
    &[("cell", "The cell it lies in"), ("bilinear", "Between the four nearest cells (bilinear), for heights and other smooth values")];

const SAMPLE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "name",
        label: "Attribute",
        kind: ParamKind::Text,
        default: "value",
        hint: "Gets the value. A raster with several bands gives name_2, name_3 and so on too",
    },
    ParamSpec {
        key: "method",
        label: "Value",
        kind: ParamKind::Choice(SAMPLE_METHODS),
        default: "cell",
        hint: "The cell it lies in suits classes, such as land cover",
    },
];

fn run_sampler(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Features");
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let name = p.get("name").filter(|n| !n.is_empty()).unwrap_or_else(|| "value".into());
    let smooth = p.get("method").as_deref() == Some("bilinear");
    let geoms = geoms_in(t, g.crs, ctx)?;
    let mut out = t.clone();
    let mut slots = vec![column(&mut out, &name, FieldType::Real)];
    for b in 0..g.bands.len() {
        slots.push(column(&mut out, &format!("{}_{}", name, b + 2), FieldType::Real));
    }
    let bands: Vec<Grid> = g.bands.iter().map(|values| Grid { values: values.clone(), bands: Vec::new(), ..g.clone() }).collect::<Vec<_>>();
    let (mut found, mut outside, mut not_points) = (0usize, 0usize, 0usize);
    for (f, geom) in out.features.iter_mut().zip(&geoms) {
        let Some(geom) = geom else { continue };
        // A point, or for a line or an area a point on it.
        let at = match geom {
            Geometry::Point(p) => p.0,
            other => {
                not_points += 1;
                match point_on(other) {
                    Some(c) => c,
                    None => continue,
                }
            }
        };
        let look = |g: &Grid| if smooth { g.bilinear(at.x, at.y) } else { g.at(at.x, at.y) };
        match look(g) {
            Some(v) => {
                found += 1;
                f.attrs[slots[0]] = Value::Real(v as f64);
            }
            None => outside += 1,
        }
        for (k, b) in bands.iter().enumerate() {
            if let Some(v) = look(b) {
                f.attrs[slots[k + 1]] = Value::Real(v as f64);
            }
        }
    }
    ctx.notes.info(format!("{} features got a value", fmt_count(found as u64)));
    if outside > 0 {
        ctx.notes.warn(format!("{} features lie outside the raster or on empty cells, and got no value.", fmt_count(outside as u64)));
    }
    if not_points > 0 {
        ctx.notes.info(format!(
            "{} lines or areas got the value at a point on them. Zonal statistics gives the mean and more over each area",
            fmt_count(not_points as u64)
        ));
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Zonal statistics (ZonalStatistics) ----------

const ZONAL_CLASSES: &[(&str, &str)] = &[("", "No"), ("share", "The share of each value, in percent"), ("area", "The area of each value")];

const ZONAL_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "stats",
        label: "Statistics",
        kind: ParamKind::Text,
        default: "count, mean, min, max",
        hint: "Any of count, mean, min, max, sum, median, std, range, majority and area, separated by commas",
    },
    ParamSpec {
        key: "classes",
        label: "Each value apart",
        kind: ParamKind::Choice(ZONAL_CLASSES),
        default: "",
        hint: "For classes, such as land cover: an attribute for each value, such as share_3 for the share of the cells with 3",
    },
    ParamSpec { key: "prefix", label: "Before the names", kind: ParamKind::Text, default: "", hint: "Such as dem_, to give dem_mean and dem_max" },
];

const ZONAL_STATS: &[&str] = &["count", "mean", "min", "max", "sum", "median", "std", "range", "majority", "area"];

fn run_zonal(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use rayon::prelude::*;
    let t = input(inp, "Zones");
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let stats: Vec<String> = p.list("stats").into_iter().map(|s| s.to_lowercase()).collect();
    let stats = if stats.is_empty() { vec!["count".to_string(), "mean".into(), "min".into(), "max".into()] } else { stats };
    if let Some(bad) = stats.iter().find(|s| !ZONAL_STATS.contains(&s.as_str())) {
        return err(format!("\"{}\" is not a statistic here. Choose among {}.", bad, ZONAL_STATS.join(", ")));
    }
    let classes = p.get("classes").unwrap_or_default();
    let prefix = p.get("prefix").unwrap_or_default();
    let geoms = geoms_in(t, g.crs, ctx)?;
    let cells = Cells::of(g);
    let cell_area = (g.geotransform[1] * g.geotransform[5]).abs();
    // The values of the cells in each zone.
    let per_zone: Vec<Vec<f32>> = geoms
        .par_iter()
        .map(|geom| {
            let Some(geom) = geom else { return Vec::new() };
            let mut idx = Vec::new();
            cells.of_geometry(geom, &mut idx);
            idx.into_iter().map(|i| g.values[i]).filter(|v| !v.is_nan()).collect()
        })
        .collect();
    let mut out = t.clone();
    let slots: Vec<usize> =
        stats.iter().map(|s| column(&mut out, &format!("{}{}", prefix, s), if s == "count" { FieldType::Int } else { FieldType::Real })).collect();
    for (k, (f, values)) in out.features.iter_mut().zip(&per_zone).enumerate() {
        let _ = k;
        let n = values.len();
        let sum: f64 = values.iter().map(|v| *v as f64).sum();
        let mean = (n > 0).then(|| sum / n as f64);
        let mut sorted: Vec<f32> = Vec::new();
        for (s, slot) in stats.iter().zip(&slots) {
            if sorted.is_empty() && n > 0 && matches!(s.as_str(), "median" | "majority" | "min" | "max" | "range") {
                sorted = values.clone();
                sorted.sort_by(f32::total_cmp);
            }
            let v = match s.as_str() {
                "count" => Value::Int(n as i64),
                "area" => Value::Real(n as f64 * cell_area),
                _ if n == 0 => Value::Null,
                "sum" => Value::Real(sum),
                "mean" => Value::Real(mean.unwrap_or(0.0)),
                "min" => Value::Real(sorted[0] as f64),
                "max" => Value::Real(sorted[n - 1] as f64),
                "range" => Value::Real((sorted[n - 1] - sorted[0]) as f64),
                "median" => Value::Real(if n % 2 == 1 { sorted[n / 2] as f64 } else { (sorted[n / 2 - 1] as f64 + sorted[n / 2] as f64) / 2.0 }),
                "std" => {
                    let m = mean.unwrap_or(0.0);
                    Value::Real((values.iter().map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / n as f64).sqrt())
                }
                "majority" => Value::Real(majority(&sorted) as f64),
                _ => Value::Null,
            };
            f.attrs[*slot] = v;
        }
    }
    if !classes.is_empty() {
        let mut seen: Vec<f32> = per_zone.iter().flatten().copied().collect();
        seen.sort_by(f32::total_cmp);
        seen.dedup();
        if seen.len() > 200 {
            return err(format!(
                "The raster has {} different values within the zones. Each value apart is for classes, such as land cover; reclassify a raster of measures first.",
                fmt_count(seen.len() as u64)
            ));
        }
        let word = if classes == "area" { "area" } else { "share" };
        let names: Vec<usize> = seen.iter().map(|v| column(&mut out, &format!("{}{}_{}", prefix, word, short(*v as f64)), FieldType::Real)).collect();
        for (f, values) in out.features.iter_mut().zip(&per_zone) {
            for (v, slot) in seen.iter().zip(&names) {
                let n = values.iter().filter(|x| *x == v).count();
                f.attrs[*slot] = if classes == "area" {
                    Value::Real(n as f64 * cell_area)
                } else if values.is_empty() {
                    Value::Null
                } else {
                    Value::Real((n as f64 / values.len() as f64 * 10000.0).round() / 100.0)
                };
            }
        }
    }
    let empty = per_zone.iter().filter(|v| v.is_empty()).count();
    ctx.notes.info(format!("{} zones, {} cells counted", fmt_count(t.len() as u64), fmt_count(per_zone.iter().map(|v| v.len() as u64).sum::<u64>())));
    if empty > 0 {
        ctx.notes.warn(format!(
            "{} zones have no cell with a value: they lie outside the raster, or are smaller than a cell, whose middle must lie inside the zone.",
            fmt_count(empty as u64)
        ));
    }
    Ok(vec![("Output".into(), out)])
}

/// The most common of sorted values; between equally common, the smallest.
fn majority(sorted: &[f32]) -> f32 {
    let (mut best, mut best_n, mut i) = (sorted[0], 0usize, 0usize);
    while i < sorted.len() {
        let mut j = i;
        while j < sorted.len() && sorted[j] == sorted[i] {
            j += 1;
        }
        if j - i > best_n {
            (best, best_n) = (sorted[i], j - i);
        }
        i = j;
    }
    best
}

// ---------- Slope and aspect (TerrainAnalyzer) ----------

const TERRAIN_MEASURES: &[(&str, &str)] = &[
    ("slope", "Slope in degrees"),
    ("percent", "Slope in percent"),
    ("aspect", "Aspect: the way the slope faces, degrees from north"),
    ("tpi", "Position: above or below the surroundings (TPI)"),
    ("roughness", "Roughness: the largest difference to a neighbour"),
];

const TERRAIN_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "measure", label: "Measure", kind: ParamKind::Choice(TERRAIN_MEASURES), default: "slope", hint: "" },
    ParamSpec {
        key: "radius",
        label: "Surroundings",
        kind: ParamKind::Text,
        default: "5",
        hint: "For the position: how many cells out the surroundings reach. Ridges and valleys show at about their width",
    },
    ParamSpec {
        key: "zfactor",
        label: "Heights times",
        kind: ParamKind::Text,
        default: "1",
        hint: "When the heights are in other units than the coordinates, such as feet on metres: 0.3048",
    },
];

/// Slope, aspect, position or roughness of a grid of heights. Slope by the method of Horn:
/// from the eight neighbours of each cell; a neighbour that is empty or outside counts as
/// the cell itself.
pub fn terrain_measure(g: &Grid, measure: &str, radius: usize, zf: f64) -> Grid {
    use rayon::prelude::*;
    let (w, h) = (g.width, g.height);
    let (cw, ch) = (g.geotransform[1].abs(), g.geotransform[5].abs());
    let at = |c: isize, r: isize, own: f64| -> f64 {
        if c < 0 || r < 0 || c as usize >= w || r as usize >= h {
            return own;
        }
        let v = g.values[r as usize * w + c as usize];
        if v.is_nan() {
            own
        } else {
            v as f64 * zf
        }
    };
    // Sums of the values and of the cells with a value, for the mean over a square.
    let (sums, counts) = if measure == "tpi" {
        let mut s = vec![0f64; (w + 1) * (h + 1)];
        let mut n = vec![0u32; (w + 1) * (h + 1)];
        for r in 0..h {
            for c in 0..w {
                let v = g.values[r * w + c];
                let (a, k) = if v.is_nan() { (0.0, 0) } else { (v as f64 * zf, 1) };
                let i = (r + 1) * (w + 1) + c + 1;
                s[i] = a + s[i - 1] + s[i - (w + 1)] - s[i - (w + 1) - 1];
                n[i] = k + n[i - 1] + n[i - (w + 1)] - n[i - (w + 1) - 1];
            }
        }
        (s, n)
    } else {
        (Vec::new(), Vec::new())
    };
    let values: Vec<f32> = (0..h)
        .into_par_iter()
        .flat_map_iter(|r| {
            let (sums, counts) = (&sums, &counts);
            (0..w).map(move |c| {
                let v = g.values[r * w + c];
                if v.is_nan() {
                    return f32::NAN;
                }
                let z = v as f64 * zf;
                let (ci, ri) = (c as isize, r as isize);
                let n = |dc: isize, dr: isize| at(ci + dc, ri + dr, z);
                match measure {
                    "tpi" => {
                        let (c0, r0) = (c.saturating_sub(radius), r.saturating_sub(radius));
                        let (c1, r1) = ((c + radius + 1).min(w), (r + radius + 1).min(h));
                        let i = |cc: usize, rr: usize| rr * (w + 1) + cc;
                        let s = sums[i(c1, r1)] - sums[i(c0, r1)] - sums[i(c1, r0)] + sums[i(c0, r0)] - z;
                        let k = counts[i(c1, r1)] + counts[i(c0, r0)] - counts[i(c0, r1)] - counts[i(c1, r0)] - 1;
                        if k == 0 {
                            0.0
                        } else {
                            (z - s / k as f64) as f32
                        }
                    }
                    "roughness" => {
                        let mut most = 0f64;
                        for (dc, dr) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                            most = most.max((n(dc, dr) - z).abs());
                        }
                        most as f32
                    }
                    _ => {
                        // Rows go south: dz/dx east, dz/dy north.
                        let dzdx = ((n(1, -1) + 2.0 * n(1, 0) + n(1, 1)) - (n(-1, -1) + 2.0 * n(-1, 0) + n(-1, 1))) / (8.0 * cw);
                        let dzdy = ((n(-1, -1) + 2.0 * n(0, -1) + n(1, -1)) - (n(-1, 1) + 2.0 * n(0, 1) + n(1, 1))) / (8.0 * ch);
                        let rise = (dzdx * dzdx + dzdy * dzdy).sqrt();
                        match measure {
                            "percent" => (rise * 100.0) as f32,
                            "aspect" => {
                                if rise < 1e-9 {
                                    // Flat ground faces nowhere.
                                    -1.0
                                } else {
                                    // The way down: against the rise, as a compass bearing.
                                    let deg = (-dzdx).atan2(-dzdy).to_degrees();
                                    (if deg < 0.0 { deg + 360.0 } else { deg }) as f32
                                }
                            }
                            _ => rise.atan().to_degrees() as f32,
                        }
                    }
                }
            })
        })
        .collect();
    Grid { values, bands: Vec::new(), ..g.clone() }
}

fn run_terrain(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Heights"), "Heights")?;
    let measure = p.get("measure").unwrap_or_else(|| "slope".into());
    let radius = match number_param(p, "radius", "Surroundings")? {
        None => 5,
        Some(r) if (1.0..=500.0).contains(&r) => r.round() as usize,
        Some(_) => return err("The surroundings must be from 1 to 500 cells."),
    };
    let zf = number_param(p, "zfactor", "Heights times")?.unwrap_or(1.0);
    if ctx.geographic(g.crs) == Some(true) && matches!(measure.as_str(), "slope" | "percent" | "aspect") {
        return err(
            "The heights are in a coordinate system in degrees, and slopes need metres across as well as up. Reproject the raster first, such as to EPSG:3006.",
        );
    }
    let out = terrain_measure(g, &measure, radius, zf);
    let label = TERRAIN_MEASURES.iter().find(|m| m.0 == measure).map_or("values", |m| m.1);
    note_range(&out, ctx, &label.to_lowercase());
    if measure == "aspect" {
        ctx.notes.info("Flat cells have the aspect -1");
    }
    Ok(vec![("Raster".into(), out.into_table())])
}

// ---------- Clip raster (RasterClipper) ----------

const CLIP_KEEP: &[(&str, &str)] = &[("inside", "Inside the areas"), ("outside", "Outside the areas")];
const CLIP_CROP: &[(&str, &str)] = &[("yes", "Cut it to the areas"), ("no", "Keep the size of the raster")];

const CLIP_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "keep", label: "Keep the cells", kind: ParamKind::Choice(CLIP_KEEP), default: "inside", hint: "A cell is inside when its middle is" },
    ParamSpec { key: "crop", label: "The raster", kind: ParamKind::Choice(CLIP_CROP), default: "yes", hint: "" },
];

fn run_clip_raster(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let r = input(inp, "Raster");
    let areas = input(inp, "Areas");
    let (cells, crs) = match (r.grid.as_deref(), r.image.as_deref()) {
        (Some(g), _) => (Cells::of(grid_on(r, "Raster").map(|_| g)?), g.crs),
        (None, Some(i)) => (Cells::of_image(i), r.crs),
        _ => return err("The stream on Raster has no raster. Read a GeoTIFF, or connect a tool that makes one."),
    };
    if r.image.as_deref().is_some_and(|i| i.geotransform[2] != 0.0 || i.geotransform[4] != 0.0) {
        return err("The picture is turned, which Clip raster cannot work with yet.");
    }
    let geoms = geoms_in(areas, crs, ctx)?;
    if !geoms.iter().flatten().any(|g| !geom::polygons_of(g).is_empty()) {
        return err("No areas came in on Areas. Connect polygons to clip with.");
    }
    let mut inside = vec![false; cells.width * cells.height];
    let mut idx = Vec::new();
    for g in geoms.iter().flatten() {
        if !geom::polygons_of(g).is_empty() {
            idx.clear();
            cells.of_geometry(g, &mut idx);
            for i in &idx {
                inside[*i] = true;
            }
        }
    }
    let keep_inside = p.get("keep").as_deref() != Some("outside");
    let keep = |i: usize| inside[i] == keep_inside;
    // The rows and columns that keep anything.
    let (mut c0, mut r0, mut c1, mut r1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for i in (0..inside.len()).filter(|i| keep(*i)) {
        let (c, rr) = (i % cells.width, i / cells.width);
        (c0, r0, c1, r1) = (c0.min(c), r0.min(rr), c1.max(c + 1), r1.max(rr + 1));
    }
    if c0 == usize::MAX {
        return err("No cell of the raster is left: the areas do not cover the middle of any cell.");
    }
    if p.get("crop").as_deref() == Some("no") || !keep_inside {
        (c0, r0, c1, r1) = (0, 0, cells.width, cells.height);
    }
    let (w, h) = (c1 - c0, r1 - r0);
    let gt = cells.gt;
    let ngt = [gt[0] + c0 as f64 * gt[1], gt[1], 0.0, gt[3] + r0 as f64 * gt[5], 0.0, gt[5]];
    let cut = |values: &[f32]| -> Vec<f32> {
        let mut out = Vec::with_capacity(w * h);
        for rr in r0..r1 {
            for c in c0..c1 {
                let i = rr * cells.width + c;
                out.push(if keep(i) { values[i] } else { f32::NAN });
            }
        }
        out
    };
    let grid = r.grid.as_deref().map(|g| Grid {
        width: w,
        height: h,
        values: cut(&g.values),
        bands: g.bands.iter().map(|b| cut(b)).collect(),
        geotransform: ngt,
        crs: g.crs,
    });
    let kept = (r0..r1).flat_map(|rr| (c0..c1).map(move |c| rr * cells.width + c)).filter(|i| keep(*i)).count();
    ctx.notes.info(format!("{} of {} cells kept, in {} × {}", fmt_count(kept as u64), fmt_count(inside.len() as u64), w, h));
    let out = match r.image.as_deref() {
        Some(img) => {
            let mut rgba = Vec::with_capacity(w * h * 4);
            for rr in r0..r1 {
                for c in c0..c1 {
                    let i = rr * cells.width + c;
                    if keep(i) {
                        rgba.extend_from_slice(&img.rgba[i * 4..i * 4 + 4]);
                    } else {
                        rgba.extend_from_slice(&[0, 0, 0, 0]);
                    }
                }
            }
            RasterImage { width: w, height: h, rgba, geotransform: ngt }.into_table_with(r.crs, grid)
        }
        None => grid.expect("a grid").into_table(),
    };
    Ok(vec![("Raster".into(), out)])
}

// ---------- Raster to areas (RasterPolygonizer) ----------

const CONNECT: &[(&str, &str)] = &[("4", "By their sides"), ("8", "By their sides and corners")];
const MERGE: &[(&str, &str)] = &[("no", "An area for each patch"), ("yes", "One feature for each value")];

const POLYGONIZE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "value", hint: "The value of the cells of each area" },
    ParamSpec {
        key: "connect",
        label: "Cells belong together",
        kind: ParamKind::Choice(CONNECT),
        default: "4",
        hint: "Cells with the same value that touch become one area",
    },
    ParamSpec { key: "merge", label: "Areas", kind: ParamKind::Choice(MERGE), default: "no", hint: "" },
];

fn run_polygonize(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let name = p.get("name").filter(|n| !n.is_empty()).unwrap_or_else(|| "value".into());
    let mut distinct: Vec<f32> = g.values.iter().copied().filter(|v| !v.is_nan()).collect();
    distinct.sort_by(f32::total_cmp);
    distinct.dedup();
    if distinct.len() > 10_000 {
        return err(format!(
            "The raster has {} different values, which would give an area for nearly every cell. Make classes of it first with Reclassify raster.",
            fmt_count(distinct.len() as u64)
        ));
    }
    let mut t = ctx.backend("Raster to areas")?.polygonize(g, &name, p.get("connect").as_deref() == Some("8"))?;
    t.crs = g.crs;
    if p.get("merge").as_deref() == Some("yes") {
        let at = t.field_index(&name).unwrap_or(0);
        let mut groups: Vec<(f64, Vec<geo::Polygon<f64>>)> = Vec::new();
        for f in &t.features {
            let v = f.attrs[at].as_f64().unwrap_or(f64::NAN);
            let polys = f.geom.as_ref().map(geom::polygons_of).unwrap_or_default();
            match groups.iter_mut().find(|(k, _)| *k == v) {
                Some((_, ps)) => ps.extend(polys),
                None => groups.push((v, polys)),
            }
        }
        groups.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged = t.like();
        merged.features =
            groups.into_iter().map(|(v, ps)| Feature::new(vec![Value::Real(v)], Some(Geometry::MultiPolygon(geo::MultiPolygon::new(ps))))).collect();
        t = merged;
    }
    ctx.notes.info(format!("{} areas of {} values", fmt_count(t.len() as u64), fmt_count(distinct.len() as u64)));
    Ok(vec![("Areas".into(), t)])
}

// ---------- Features to raster (Rasterizer) ----------

const RASTERIZE_MANY: &[(&str, &str)] = &[
    ("last", "The last feature"),
    ("first", "The first feature"),
    ("max", "The largest value"),
    ("min", "The smallest value"),
    ("sum", "The sum"),
    ("count", "How many features"),
];

const RASTERIZE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "value",
        label: "Value",
        kind: ParamKind::Expression("Features"),
        default: "1",
        hint: "The value of the cells of each feature: a number, an attribute or an expression",
    },
    ParamSpec {
        key: "cell",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "10",
        hint: "In the units of the coordinate system, such as metres. A raster connected to Like gives its cells instead",
    },
    ParamSpec { key: "many", label: "Where several features are", kind: ParamKind::Choice(RASTERIZE_MANY), default: "last", hint: "" },
    ParamSpec {
        key: "background",
        label: "Other cells",
        kind: ParamKind::Text,
        default: "",
        hint: "The value of cells no feature reaches. Empty leaves them empty",
    },
];

fn run_rasterize(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Features");
    let like = input(inp, "Like");
    let to = like.grid.as_ref().and_then(|g| g.crs).or(if like.image.is_some() { like.crs } else { None }).or(t.crs);
    let geoms = geoms_in(t, to, ctx)?;
    let (cells, crs) = new_cells(p, like, bounds(&geoms), to, ctx)?;
    let text = p.get("value").filter(|v| !v.is_empty()).unwrap_or_else(|| "1".into());
    let e = crate::expr::compile(&text, &t.field_names()).map_err(|e| GbError(format!("Value: {}", e.0)))?;
    let geographic = ctx.geographic(t.crs);
    let many = p.get("many").unwrap_or_else(|| "last".into());
    let background = number_param(p, "background", "Other cells")?.map_or(f32::NAN, |v| v as f32);
    let mut values = vec![f32::NAN; cells.width * cells.height];
    let mut idx = Vec::new();
    let mut reached = 0usize;
    for (i, (f, g)) in t.features.iter().zip(&geoms).enumerate() {
        let Some(g) = g else { continue };
        let v = match e.eval(&crate::expr::Row { feature: f, index: i, geographic })? {
            Value::Null => continue,
            Value::Bool(b) => b as u8 as f32,
            v => v.as_f64().ok_or_else(|| GbError(format!("The value must be a number. Feature {} gives \"{}\".", i + 1, v.to_text())))? as f32,
        };
        idx.clear();
        cells.of_geometry(g, &mut idx);
        if !idx.is_empty() {
            reached += 1;
        }
        for k in &idx {
            let c = &mut values[*k];
            *c = match (many.as_str(), c.is_nan()) {
                ("count", true) => 1.0,
                ("count", false) => *c + 1.0,
                (_, true) => v,
                ("first", false) => *c,
                ("max", false) => c.max(v),
                ("min", false) => c.min(v),
                ("sum", false) => *c + v,
                _ => v,
            };
        }
    }
    if !background.is_nan() {
        for v in values.iter_mut().filter(|v| v.is_nan()) {
            *v = background;
        }
    }
    let grid = Grid { width: cells.width, height: cells.height, values, geotransform: cells.gt, crs, bands: Vec::new() };
    if reached < t.features.iter().filter(|f| f.geom.is_some()).count() {
        ctx.notes.warn(format!(
            "{} features reached no cell: smaller than a cell, whose middle must lie inside an area, or outside the raster.",
            fmt_count((t.features.iter().filter(|f| f.geom.is_some()).count() - reached) as u64)
        ));
    }
    note_range(&grid, ctx, "values");
    Ok(vec![("Raster".into(), grid.into_table())])
}

// ---------- Reclassify raster (RasterReclassifier) ----------

const RECLASS_OTHER: &[(&str, &str)] = &[("empty", "Make them empty"), ("keep", "Keep their value")];

const RECLASS_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "ranges",
        label: "From, to and new value",
        kind: ParamKind::Ranges("Raster"),
        default: "",
        hint: "From and to are both in the range, and the first range that fits is used: 0 to 5 gives 1, 5 to 15 gives 2. An empty from or to has no end",
    },
    ParamSpec { key: "otherwise", label: "Values in no range", kind: ParamKind::Choice(RECLASS_OTHER), default: "empty", hint: "" },
];

fn run_reclassify(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use rayon::prelude::*;
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let ranges = super::attr_values::ranges_of(p.values, "ranges");
    if ranges.is_empty() {
        return err("Give the ranges: from, to and the new value, such as 0 to 5 gives 1.");
    }
    let mut table: Vec<(f64, f64, f32)> = Vec::new();
    for r in &ranges {
        let v =
            crate::model::parse_number(&r.value).ok_or_else(|| GbError(format!("The new value \"{}\" is not a number. A raster holds numbers.", r.value)))?;
        table.push((r.from.unwrap_or(f64::NEG_INFINITY), r.to.unwrap_or(f64::INFINITY), v as f32));
    }
    let keep = p.get("otherwise").as_deref() == Some("keep");
    let values: Vec<f32> = g
        .values
        .par_iter()
        .map(|v| {
            if v.is_nan() {
                return f32::NAN;
            }
            let x = *v as f64;
            match table.iter().find(|(lo, hi, _)| x >= *lo && x <= *hi) {
                Some((_, _, n)) => *n,
                None if keep => *v,
                None => f32::NAN,
            }
        })
        .collect();
    let missed = g.values.iter().zip(&values).filter(|(a, b)| !a.is_nan() && b.is_nan()).count();
    let out = Grid { values, bands: Vec::new(), ..g.clone() };
    note_range(&out, ctx, "values");
    if missed > 0 {
        ctx.notes.warn(format!("{} cells had a value in no range and were made empty.", fmt_count(missed as u64)));
    }
    Ok(vec![("Raster".into(), out.into_table())])
}

// ---------- Distance raster (DistanceRaster) ----------

const DISTANCE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "cell",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "10",
        hint: "In the units of the coordinate system, such as metres. A raster connected to Like gives its cells instead",
    },
    ParamSpec {
        key: "reach",
        label: "As far as",
        kind: ParamKind::Text,
        default: "1000",
        hint: "How far from the features the raster reaches; cells farther away are empty",
    },
];

/// The distance from the middle of every cell to the nearest cell that is set, in the units
/// of the cells: the exact distance transform of Felzenszwalb and Huttenlocher, first along
/// the rows and then down the columns.
pub fn distance_transform(set: &[bool], w: usize, h: usize, cw: f64, ch: f64) -> Vec<f64> {
    let inf = 1e20;
    let mut d: Vec<f64> = set.iter().map(|s| if *s { 0.0 } else { inf }).collect();
    // One line: squared distances, with the cells `s` apart.
    fn line(f: &[f64], s: f64, out: &mut [f64], v: &mut Vec<usize>, z: &mut Vec<f64>) {
        v.clear();
        z.clear();
        let s2 = s * s;
        let cross = |q: usize, p: usize| ((f[q] + s2 * (q * q) as f64) - (f[p] + s2 * (p * p) as f64)) / (2.0 * s2 * (q as f64 - p as f64));
        for (q, fq) in f.iter().enumerate() {
            if *fq >= 1e19 {
                continue;
            }
            while let Some(&p) = v.last() {
                let x = cross(q, p);
                if x <= *z.last().unwrap() {
                    v.pop();
                    z.pop();
                } else {
                    break;
                }
            }
            let x = match v.last() {
                Some(&p) => cross(q, p),
                None => f64::NEG_INFINITY,
            };
            v.push(q);
            z.push(x);
        }
        if v.is_empty() {
            out.iter_mut().for_each(|o| *o = 1e20);
            return;
        }
        let mut k = 0;
        for (q, o) in out.iter_mut().enumerate() {
            while k + 1 < v.len() && z[k + 1] < q as f64 {
                k += 1;
            }
            let dq = q as f64 - v[k] as f64;
            *o = s2 * dq * dq + f[v[k]];
        }
    }
    let (mut v, mut z) = (Vec::new(), Vec::new());
    let mut out = vec![0.0; w.max(h)];
    for r in 0..h {
        let row = d[r * w..(r + 1) * w].to_vec();
        line(&row, cw, &mut out[..w], &mut v, &mut z);
        d[r * w..(r + 1) * w].copy_from_slice(&out[..w]);
    }
    let mut col = vec![0.0; h];
    for c in 0..w {
        for r in 0..h {
            col[r] = d[r * w + c];
        }
        line(&col, ch, &mut out[..h], &mut v, &mut z);
        for r in 0..h {
            d[r * w + c] = out[r];
        }
    }
    d.into_iter().map(|x| if x >= 1e19 { f64::INFINITY } else { x.sqrt() }).collect()
}

fn run_distance(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Features");
    let like = input(inp, "Like");
    let to = like.grid.as_ref().and_then(|g| g.crs).or(if like.image.is_some() { like.crs } else { None }).or(t.crs);
    let geoms = geoms_in(t, to, ctx)?;
    let reach = number_param(p, "reach", "As far as")?.unwrap_or(1000.0);
    if reach <= 0.0 {
        return err("As far as must be more than 0.");
    }
    let rect = bounds(&geoms).map(|r| [r[0] - reach, r[1] - reach, r[2] + reach, r[3] + reach]);
    let (cells, crs) = new_cells(p, like, rect, to, ctx)?;
    let mut set = vec![false; cells.width * cells.height];
    let mut idx = Vec::new();
    for g in geoms.iter().flatten() {
        idx.clear();
        cells.of_geometry(g, &mut idx);
        for i in &idx {
            set[*i] = true;
        }
    }
    if !set.iter().any(|s| *s) {
        return err("No feature reaches a cell of the raster.");
    }
    let d = distance_transform(&set, cells.width, cells.height, cells.gt[1].abs(), cells.gt[5].abs());
    let values: Vec<f32> = d.into_iter().map(|x| if x > reach { f32::NAN } else { x as f32 }).collect();
    let grid = Grid { width: cells.width, height: cells.height, values, geotransform: cells.gt, crs, bands: Vec::new() };
    note_range(&grid, ctx, "distances");
    Ok(vec![("Raster".into(), grid.into_table())])
}

// ---------- Visible area (Viewshed) ----------

const VIEW_RESULT: &[(&str, &str)] = &[("count", "How many of the places see each cell"), ("visible", "1 where any of them sees, 0 elsewhere")];

const VIEW_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "eye",
        label: "Eye above the ground",
        kind: ParamKind::Expression("Places"),
        default: "1.7",
        hint: "How high the eye is at each place, in metres: a number, or an attribute such as the height of a mast",
    },
    ParamSpec {
        key: "target",
        label: "Seen above the ground",
        kind: ParamKind::Text,
        default: "0",
        hint: "How high what is to be seen is, such as 0 for the ground or 150 for the top of a wind turbine",
    },
    ParamSpec { key: "radius", label: "As far as", kind: ParamKind::Text, default: "5000", hint: "How far the eye reaches, in metres" },
    ParamSpec { key: "result", label: "Each cell", kind: ParamKind::Choice(VIEW_RESULT), default: "count", hint: "" },
    ParamSpec {
        key: "curvature",
        label: "The curve of the Earth",
        kind: ParamKind::Choice(&[("yes", "Counted, with the bending of light"), ("no", "Not counted")]),
        default: "yes",
        hint: "Matters over kilometres: 5 km away the ground is 1.7 m lower",
    },
];

/// The cells one eye sees, in the square around it, by rays from the eye to every cell of
/// the edge of the square: a cell is seen when nothing nearer along the ray rises above the
/// line from the eye to it. `eye` is the column, row and height of the eye.
pub fn viewshed(g: &Grid, eye: (f64, f64, f64), target: f64, radius: f64, curvature: bool) -> (usize, usize, usize, usize, Vec<bool>) {
    let (cw, ch) = (g.geotransform[1].abs(), g.geotransform[5].abs());
    let (oc, or) = (eye.0.floor() as isize, eye.1.floor() as isize);
    let (rc, rr) = ((radius / cw).ceil() as isize, (radius / ch).ceil() as isize);
    let (c0, r0) = ((oc - rc).max(0), (or - rr).max(0));
    let (c1, r1) = ((oc + rc + 1).min(g.width as isize), (or + rr + 1).min(g.height as isize));
    let (w, h) = ((c1 - c0).max(0) as usize, (r1 - r0).max(0) as usize);
    let mut seen = vec![false; w * h];
    if w == 0 || h == 0 {
        return (c0 as usize, r0 as usize, w, h, seen);
    }
    // 0.87 of the radius of the Earth bends: light curves back a seventh.
    let drop = |d: f64| if curvature { d * d / (2.0 * 6_371_000.0) * 0.87 } else { 0.0 };
    let mut edge: Vec<(isize, isize)> = Vec::new();
    for c in c0..c1 {
        edge.push((c, r0));
        edge.push((c, r1 - 1));
    }
    for r in r0..r1 {
        edge.push((c0, r));
        edge.push((c1 - 1, r));
    }
    if let (true, true) = (oc >= c0 && oc < c1, or >= r0 && or < r1) {
        seen[(or - r0) as usize * w + (oc - c0) as usize] = true;
    }
    for (tc, tr) in edge {
        let (dc, dr) = (tc - oc, tr - or);
        let n = dc.abs().max(dr.abs());
        let mut steepest = f64::NEG_INFINITY;
        for i in 1..=n {
            let fc = oc as f64 + dc as f64 * i as f64 / n as f64;
            let fr = or as f64 + dr as f64 * i as f64 / n as f64;
            let (c, r) = (fc.round() as isize, fr.round() as isize);
            if c < c0 || r < r0 || c >= c1 || r >= r1 {
                break;
            }
            let dist = (((c - oc) as f64 * cw).powi(2) + ((r - or) as f64 * ch).powi(2)).sqrt();
            if dist > radius {
                break;
            }
            let Some(z) = g.get(c as usize, r as usize) else { continue };
            let z = z as f64 - drop(dist);
            let slope = (z + target - eye.2) / dist;
            if slope >= steepest {
                seen[(r - r0) as usize * w + (c - c0) as usize] = true;
            }
            steepest = steepest.max((z - eye.2) / dist);
        }
    }
    (c0 as usize, r0 as usize, w, h, seen)
}

fn run_viewshed(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use rayon::prelude::*;
    let g = grid_on(input(inp, "Heights"), "Heights")?;
    if ctx.geographic(g.crs) == Some(true) {
        return err("The heights are in a coordinate system in degrees. Reproject the raster first, such as to EPSG:3006, so that distances are in metres.");
    }
    let places = input(inp, "Places");
    let geoms = geoms_in(places, g.crs, ctx)?;
    let text = p.get("eye").filter(|v| !v.is_empty()).unwrap_or_else(|| "1.7".into());
    let e = crate::expr::compile(&text, &places.field_names()).map_err(|e| GbError(format!("Eye above the ground: {}", e.0)))?;
    let target = number_param(p, "target", "Seen above the ground")?.unwrap_or(0.0);
    let radius = number_param(p, "radius", "As far as")?.unwrap_or(5000.0);
    if radius <= 0.0 {
        return err("As far as must be more than 0.");
    }
    let curvature = p.get("curvature").as_deref() != Some("no");
    let cells = Cells::of(g);
    let mut eyes: Vec<(f64, f64, f64)> = Vec::new();
    let (mut outside, mut not_points) = (0usize, 0usize);
    let geographic = ctx.geographic(places.crs);
    for (i, (f, geom)) in places.features.iter().zip(&geoms).enumerate() {
        let at = match geom {
            Some(Geometry::Point(p)) => p.0,
            Some(other) => {
                not_points += 1;
                match point_on(other) {
                    Some(c) => c,
                    None => continue,
                }
            }
            None => continue,
        };
        let Some(ground) = g.bilinear(at.x, at.y) else {
            outside += 1;
            continue;
        };
        let above = e.eval(&crate::expr::Row { feature: f, index: i, geographic })?.as_f64().unwrap_or(1.7);
        let (c, r) = cells.place(at.x, at.y);
        eyes.push((c, r, ground as f64 + above));
    }
    if outside > 0 {
        ctx.notes.warn(format!("{} places lie outside the heights or on empty cells, and were left out.", fmt_count(outside as u64)));
    }
    if not_points > 0 {
        ctx.notes.info(format!("{} lines or areas look from a point on them", fmt_count(not_points as u64)));
    }
    if eyes.is_empty() {
        return err("No place to look from lies on the heights. Connect points within the terrain model to Places.");
    }
    if eyes.len() > 5000 {
        return err(format!("{} places to look from. At most 5 000 are counted at a time.", fmt_count(eyes.len() as u64)));
    }
    let views: Vec<(usize, usize, usize, usize, Vec<bool>)> = eyes.par_iter().map(|eye| viewshed(g, *eye, target, radius, curvature)).collect();
    let mut count = vec![f32::NAN; g.width * g.height];
    let (cw, ch) = (g.geotransform[1].abs(), g.geotransform[5].abs());
    for (eye, (c0, r0, w, h, seen)) in eyes.iter().zip(&views) {
        for r in 0..*h {
            for c in 0..*w {
                let (gc, gr) = (c0 + c, r0 + r);
                let d = (((gc as f64 + 0.5 - eye.0) * cw).powi(2) + ((gr as f64 + 0.5 - eye.1) * ch).powi(2)).sqrt();
                if d > radius || g.values[gr * g.width + gc].is_nan() {
                    continue;
                }
                let cell = &mut count[gr * g.width + gc];
                let add = seen[r * w + c] as u8 as f32;
                *cell = if cell.is_nan() { add } else { *cell + add };
            }
        }
    }
    if p.get("result").as_deref() == Some("visible") {
        for v in count.iter_mut().filter(|v| !v.is_nan()) {
            *v = (*v > 0.0) as u8 as f32;
        }
    }
    let seen = count.iter().filter(|v| **v > 0.0).count();
    let reached = count.iter().filter(|v| !v.is_nan()).count();
    let grid = Grid { values: count, bands: Vec::new(), ..g.clone() };
    ctx.notes.info(format!(
        "{} of {} cells within reach are seen ({}%), from {} places",
        fmt_count(seen as u64),
        fmt_count(reached as u64),
        fmt_real((seen as f64 / reached.max(1) as f64 * 1000.0).round() / 10.0),
        fmt_count(eyes.len() as u64)
    ));
    Ok(vec![("Raster".into(), grid.into_table())])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "RasterSampler",
        title: "Values from raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Gives each feature the value of the raster where it lies: the height of the terrain at each address, the land cover under each tree, the noise at each school. The value of the cell, or between the four nearest cells for heights. A line or an area gets the value at a point on it; Zonal statistics gives the mean and more over each area.",
        inputs: &["Features", "Raster"],
        outputs: &["Output"],
        ports: None,
        params: SAMPLE_PARAMS,
        run: run_sampler,
        summary: |p| p.get("name").unwrap_or_else(|| "value".into()),
    },
    NodeType {
        name: "ZonalStatistics",
        title: "Zonal statistics",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Counts the cells of the raster within each area: how many, the mean, the smallest and largest, sum, median, spread and the most common value, and for classes, such as land cover, the share or area of each. A cell counts when its middle lies in the area. The share of forest in each municipality, the mean height of each property, the people in each district from a population grid.",
        inputs: &["Zones", "Raster"],
        outputs: &["Output"],
        ports: None,
        params: ZONAL_PARAMS,
        run: run_zonal,
        summary: |p| p.get("stats").unwrap_or_else(|| "count, mean, min, max".into()),
    },
    NodeType {
        name: "TerrainAnalyzer",
        title: "Slope and aspect",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "Measures the terrain from a grid of heights, cell by cell: the slope in degrees or percent, the aspect (which way it faces), the position against the surroundings, which picks out ridges and valleys (TPI), or the roughness.",
        inputs: &["Heights"],
        outputs: &["Raster"],
        ports: None,
        params: TERRAIN_PARAMS,
        run: run_terrain,
        summary: |p| {
            let m = p.get("measure").unwrap_or_else(|| "slope".into());
            TERRAIN_MEASURES.iter().find(|x| x.0 == m).map_or(m.clone(), |x| x.1.split(':').next().unwrap_or(x.1).to_string())
        },
    },
    NodeType {
        name: "RasterClipper",
        title: "Clip raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Keeps the cells of a raster inside the areas on Areas, or outside them, and cuts the raster to them. A grid of values or a picture. To reproject a raster, use Reproject.",
        inputs: &["Raster", "Areas"],
        outputs: &["Raster"],
        ports: None,
        params: CLIP_PARAMS,
        run: run_clip_raster,
        summary: |p| if p.get("keep").as_deref() == Some("outside") { "outside".into() } else { "inside".into() },
    },
    NodeType {
        name: "RasterPolygonizer",
        title: "Raster to areas",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Makes areas of the cells with the same value that touch, with the value as an attribute: land cover, zones of noise or of a viewshed as polygons. For a raster of measures, such as heights, make classes first with Reclassify raster.",
        inputs: &["Raster"],
        outputs: &["Areas"],
        ports: None,
        params: POLYGONIZE_PARAMS,
        run: run_polygonize,
        summary: |p| if p.get("merge").as_deref() == Some("yes") { "one per value".into() } else { String::new() },
    },
    NodeType {
        name: "Rasterizer",
        title: "Features to raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Makes a raster of features: each cell gets the value of the area its middle lies in, the line that passes through it or the point in it. A value from an attribute or expression, or how many features. The cells are of a size, or those of a raster connected to Like, so that it can be counted with that one.",
        inputs: &["Features", "Like"],
        outputs: &["Raster"],
        ports: None,
        params: RASTERIZE_PARAMS,
        run: run_rasterize,
        summary: |p| p.get("value").unwrap_or_else(|| "1".into()),
    },
    NodeType {
        name: "RasterReclassifier",
        title: "Reclassify raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Gives the cells new values by ranges: heights into bands, slopes into classes, land cover codes into groups. From, to and the new value, the first range that fits.",
        inputs: &["Raster"],
        outputs: &["Raster"],
        ports: None,
        params: RECLASS_PARAMS,
        run: run_reclassify,
        summary: |p| {
            let n = super::attr_values::ranges_of(p.values, "ranges").len();
            if n == 0 {
                String::new()
            } else {
                format!("{} ranges", n)
            }
        },
    },
    NodeType {
        name: "DistanceRaster",
        title: "Distance raster",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "A raster of the distance from each cell to the nearest feature, as far as a distance: to the nearest road, school, shore or stop. Exact from the middle of the cells, in the units of the coordinate system.",
        inputs: &["Features", "Like"],
        outputs: &["Raster"],
        ports: None,
        params: DISTANCE_PARAMS,
        run: run_distance,
        summary: |p| format!("as far as {}", p.get("reach").unwrap_or_else(|| "1000".into())),
    },
    NodeType {
        name: "Viewshed",
        title: "Visible area",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "What can be seen from places on the terrain (a viewshed): for each cell, how many of the places on Places see it, with the eye a height above the ground and what is seen a height above it, as far as a distance, with the curve of the Earth. From where a wind turbine can be seen, what a lookout tower shows.",
        inputs: &["Heights", "Places"],
        outputs: &["Raster"],
        ports: None,
        params: VIEW_PARAMS,
        run: run_viewshed,
        summary: |p| format!("{} m", p.get("radius").unwrap_or_else(|| "5000".into())),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{line_string, point, polygon};

    /// A slope rising 1 m per cell to the east, 10 × 10 cells of 1 m, at 1000, 2000.
    fn ramp() -> Grid {
        let values = (0..100).map(|i| (i % 10) as f32).collect();
        Grid { width: 10, height: 10, values, geotransform: [1000.0, 1.0, 0.0, 2010.0, 0.0, -1.0], crs: None, bands: Vec::new() }
    }

    fn table_of(geoms: Vec<Geometry<f64>>) -> Table {
        Table {
            fields: vec![Field::new("id", FieldType::Int)],
            features: geoms.into_iter().enumerate().map(|(i, g)| Feature::new(vec![Value::Int(i as i64)], Some(g))).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn values_are_taken_from_the_raster_and_counted_in_zones() {
        let pts = table_of(vec![
            Geometry::Point(point!(x: 1003.5, y: 2005.5)),
            Geometry::Point(point!(x: 1003.0, y: 2005.5)),
            Geometry::Point(point!(x: 2000.0, y: 0.0)),
        ]);
        let (out, notes) = run_for_test("RasterSampler", &[], vec![("Features", pts.clone()), ("Raster", ramp().into_table())]).unwrap();
        let v: Vec<Value> = out[0].1.features.iter().map(|f| f.attrs[1].clone()).collect();
        assert_eq!(v, [Value::Real(3.0), Value::Real(3.0), Value::Null]);
        assert!(notes.warnings.iter().any(|l| l.contains("outside")), "{:?}", notes.warnings);
        let (out, _) = run_for_test("RasterSampler", &[("method", "bilinear")], vec![("Features", pts), ("Raster", ramp().into_table())]).unwrap();
        assert_eq!(out[0].1.features[1].attrs[1], Value::Real(2.5), "half way between the middles of two cells");

        // A zone over columns 2 to 4 of the ramp, all ten rows.
        let zone = table_of(vec![Geometry::Polygon(polygon![(x: 1002.0, y: 2000.0), (x: 1005.0, y: 2000.0), (x: 1005.0, y: 2010.0), (x: 1002.0, y: 2010.0)])]);
        let (out, _) = run_for_test(
            "ZonalStatistics",
            &[("stats", "count, mean, min, max, sum, majority, area"), ("classes", "share")],
            vec![("Zones", zone), ("Raster", ramp().into_table())],
        )
        .unwrap();
        let t = &out[0].1;
        let get = |n: &str| t.features[0].attrs[t.field_index(n).unwrap()].clone();
        assert_eq!(get("count"), Value::Int(30));
        assert_eq!((get("mean"), get("min"), get("max"), get("sum")), (Value::Real(3.0), Value::Real(2.0), Value::Real(4.0), Value::Real(90.0)));
        assert_eq!((get("majority"), get("area")), (Value::Real(2.0), Value::Real(30.0)));
        assert_eq!(get("share_3"), Value::Real(33.33));
    }

    #[test]
    fn slope_aspect_and_position() {
        let g = ramp();
        let slope = terrain_measure(&g, "slope", 1, 1.0);
        assert!((slope.get(5, 5).unwrap() - 45.0).abs() < 1e-4, "{:?}", slope.get(5, 5));
        // Rising to the east, it faces west.
        let aspect = terrain_measure(&g, "aspect", 1, 1.0);
        assert!((aspect.get(5, 5).unwrap() - 270.0).abs() < 1e-3, "{:?}", aspect.get(5, 5));
        assert!((terrain_measure(&g, "percent", 1, 1.0).get(5, 5).unwrap() - 100.0).abs() < 1e-3);
        // On an even slope nothing sticks up, and a single peak does.
        assert!(terrain_measure(&g, "tpi", 2, 1.0).get(5, 5).unwrap().abs() < 1e-4);
        let mut peak = Grid::blank(5, 5, [0.0, 1.0, 0.0, 5.0, 0.0, -1.0], None);
        peak.values.iter_mut().for_each(|v| *v = 0.0);
        peak.values[12] = 8.0;
        assert_eq!(terrain_measure(&peak, "tpi", 1, 1.0).get(2, 2), Some(8.0));
        assert_eq!(terrain_measure(&peak, "roughness", 1, 1.0).get(1, 1), Some(8.0));
    }

    #[test]
    fn a_raster_is_clipped_reclassified_made_and_measured() {
        // A triangle over the lower left half of the ramp.
        let tri = table_of(vec![Geometry::Polygon(polygon![(x: 1000.0, y: 2000.0), (x: 1010.0, y: 2000.0), (x: 1000.0, y: 2010.0)])]);
        let (out, _) = run_for_test("RasterClipper", &[], vec![("Raster", ramp().into_table()), ("Areas", tri.clone())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        let kept = g.values.iter().filter(|v| !v.is_nan()).count();
        assert_eq!(kept, 55, "the cells whose middles lie on or below the diagonal");
        assert_eq!(g.get(0, 9), Some(0.0));
        assert_eq!(g.get(9, 0), None);
        // Reclassify: 0 to 4 is 1, 5 to 9 is 2.
        let mut params = serde_json::Map::new();
        params.insert("ranges".into(), serde_json::json!([["0", "4", "1"], ["5", "9", "2"]]));
        let (out, _) = run_for_test_with("RasterReclassifier", params, vec![("Raster", ramp().into_table())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!((g.get(4, 0), g.get(5, 0)), (Some(1.0), Some(2.0)));
        // Features to raster: the triangle at 2 m cells, and a line.
        let line = table_of(vec![Geometry::LineString(line_string![(x: 0.0, y: 1.0), (x: 10.0, y: 1.0)])]);
        let (out, _) = run_for_test("Rasterizer", &[("cell", "2"), ("value", "id + 5")], vec![("Features", line)]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!((g.width, g.height), (5, 1));
        assert!(g.values.iter().all(|v| *v == 5.0));
        let (out, _) =
            run_for_test("Rasterizer", &[("value", "1"), ("background", "0")], vec![("Features", tri.clone()), ("Like", ramp().into_table())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!(g.values.iter().filter(|v| **v == 1.0).count(), 55);
        // The distance from a point, in cells of 1 m.
        let pt = table_of(vec![Geometry::Point(point!(x: 0.5, y: 0.5))]);
        let (out, _) = run_for_test("DistanceRaster", &[("cell", "1"), ("reach", "5")], vec![("Features", pt)]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        let at = |x: f64, y: f64| g.at(x, y).unwrap();
        assert_eq!(at(0.5, 0.5), 0.0);
        assert!((at(3.5, 4.5) - 5.0).abs() < 1e-5);
        assert!(g.at(4.5, 4.5).is_none(), "beyond the reach");
    }

    #[test]
    fn a_wall_hides_what_lies_behind_it() {
        // Flat ground of 100 × 1 cells of 10 m, with a wall 20 m high at cell 50.
        let mut g = Grid::blank(100, 1, [0.0, 10.0, 0.0, 10.0, 0.0, -10.0], Some(3006));
        g.values.iter_mut().for_each(|v| *v = 0.0);
        g.values[50] = 20.0;
        let (c0, _, w, _, seen) = viewshed(&g, (10.5, 0.5, 2.0), 0.0, 2000.0, false);
        let at = |c: usize| seen[c - c0];
        assert!(at(30) && at(50), "the ground before the wall, and its top");
        assert!(!at(60) && !at(99), "the ground behind it");
        assert_eq!(w, 100);
        // A target 150 m high is seen behind the wall too.
        let (_, _, _, _, seen) = viewshed(&g, (10.5, 0.5, 2.0), 150.0, 2000.0, false);
        assert!(seen[99]);
        let places = table_of(vec![Geometry::Point(point!(x: 105.0, y: 5.0))]);
        let (out, _) = run_for_test("Viewshed", &[("radius", "300"), ("curvature", "no")], vec![("Heights", g.into_table()), ("Places", places)]).unwrap();
        let v = out[0].1.grid.clone().unwrap();
        assert_eq!(v.get(30, 0), Some(1.0));
        assert_eq!(v.get(45, 0), None, "beyond the reach");
    }
}
