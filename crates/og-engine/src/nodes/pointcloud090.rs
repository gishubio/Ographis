//! Tools for point clouds added in 0.90: ground points found in a cloud without classes, as
//! from a drone, and the heights and roofs of buildings from the points on their footprints.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::pointcloud089::{fill_holes, rasterize_points};
use super::*;
use crate::model::{err, fmt_count, fmt_real, Grid};
use geo::Geometry;

fn z_of(f: &Feature) -> Option<f64> {
    f.heights().and_then(|z| z.first().copied())
}

fn xy_of(f: &Feature) -> Option<(f64, f64)> {
    match &f.geom {
        Some(Geometry::Point(p)) => Some((p.x(), p.y())),
        _ => None,
    }
}

/// The smallest (or largest) value within `r` cells along each row, then each column: a
/// square window, in time that does not grow with its size.
fn min_max_filter(values: &[f32], w: usize, h: usize, r: usize, max: bool) -> Vec<f32> {
    let better = |a: f32, b: f32| if max { a >= b } else { a <= b };
    let line = |get: &dyn Fn(usize) -> f32, n: usize, out: &mut Vec<f32>| {
        use std::collections::VecDeque;
        out.clear();
        let mut q: VecDeque<usize> = VecDeque::new();
        let mut next = 0usize;
        for i in 0..n {
            let hi = (i + r).min(n - 1);
            while next <= hi {
                let v = get(next);
                while let Some(&b) = q.back() {
                    if better(v, get(b)) {
                        q.pop_back();
                    } else {
                        break;
                    }
                }
                q.push_back(next);
                next += 1;
            }
            while q.front().is_some_and(|&f| f + r < i) {
                q.pop_front();
            }
            out.push(q.front().map_or(f32::NAN, |&f| get(f)));
        }
    };
    let mut rows = vec![0f32; w * h];
    let mut buf = Vec::with_capacity(w.max(h));
    for y in 0..h {
        line(&|x| values[y * w + x], w, &mut buf);
        rows[y * w..(y + 1) * w].copy_from_slice(&buf);
    }
    let mut out = vec![0f32; w * h];
    for x in 0..w {
        line(&|y| rows[y * w + x], h, &mut buf);
        for (y, v) in buf.iter().enumerate() {
            out[y * w + x] = *v;
        }
    }
    out
}

/// The ground under a point cloud by a simple morphological filter (after Pingel and others,
/// SMRF): the lowest point in each cell, opened with ever larger windows up to `window`
/// metres; a cell that the opening lowers by more than the slope allows is something on the
/// ground, not the ground. The rest is the ground, with the holes filled from around.
pub fn ground_surface(points: &[(f64, f64, f64)], cell: f64, slope: f64, window: f64, crs: Option<u32>) -> Result<Grid> {
    let mut low = rasterize_points(points, cell, "min", crs, None)?;
    let (w, h) = (low.width, low.height);
    fill_holes(&mut low, w.max(h));
    let mut surface: Vec<f32> = low.values.clone();
    let mut object = vec![false; w * h];
    let most = (window / cell / 2.0).ceil().max(1.0) as usize;
    for r in 1..=most {
        let opened = min_max_filter(&min_max_filter(&surface, w, h, r, false), w, h, r, true);
        let allowed = (slope * r as f64 * cell) as f32;
        for i in 0..w * h {
            if surface[i] - opened[i] > allowed {
                object[i] = true;
            }
        }
        surface = opened;
    }
    let mut ground = low.clone();
    for (v, o) in ground.values.iter_mut().zip(&object) {
        if *o {
            *v = f32::NAN;
        }
    }
    fill_holes(&mut ground, w.max(h));
    Ok(ground)
}

// ---------- Classify ground points (GroundClassifier) ----------

const GROUND_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "cell",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "1",
        hint: "Of the surface the ground is found on, in metres: about the distance between the points, or a little more",
    },
    ParamSpec {
        key: "window",
        label: "Largest object",
        kind: ParamKind::Text,
        default: "20",
        hint: "Metres across: the largest building or thing on the ground that is to be taken away. Larger finds bigger buildings, and may cut hilltops",
    },
    ParamSpec {
        key: "slope",
        label: "Slope of the ground",
        kind: ParamKind::Text,
        default: "0.15",
        hint: "The steepest ground, as rise over run: 0.15 is 15 %. Raise it for hilly land",
    },
    ParamSpec {
        key: "threshold",
        label: "Within",
        kind: ParamKind::Text,
        default: "0.4",
        hint: "Metres: a point this close to the ground surface, above or below, is ground",
    },
];

fn run_ground(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let cell = number_param(p, "cell", "Cell size")?.unwrap_or(1.0);
    let window = number_param(p, "window", "Largest object")?.unwrap_or(20.0);
    let slope = number_param(p, "slope", "Slope of the ground")?.unwrap_or(0.15);
    let threshold = number_param(p, "threshold", "Within")?.unwrap_or(0.4);
    if cell <= 0.0 || window <= 0.0 || threshold < 0.0 {
        return err("The cell size, the largest object and within must be more than 0.");
    }
    if ctx.geographic(t.crs) == Some(true) {
        return err("The points are in a coordinate system in degrees. Reproject them first, such as to EPSG:3006, so that sizes are in metres.");
    }
    let pts: Vec<(f64, f64, f64)> = t.features.iter().filter_map(|f| Some((xy_of(f)?.0, xy_of(f)?.1, z_of(f)?))).collect();
    if pts.is_empty() {
        return err("There are no points with heights. Read a point cloud (LAS or LAZ).");
    }
    let ground = ground_surface(&pts, cell, slope, window, t.crs)?;
    let mut out = t.clone();
    let class_at = column(&mut out, "classification", FieldType::Int);
    let name_at = out.field_index("class");
    let mut grounds = out.like();
    let (mut n_ground, mut n_other) = (0usize, 0usize);
    for f in out.features.iter_mut() {
        let (Some((x, y)), Some(z)) = (xy_of(f), z_of(f)) else { continue };
        let is_ground = ground.bilinear(x, y).is_some_and(|g| (z - g as f64).abs() <= threshold);
        let class: u8 = if is_ground { 2 } else { 1 };
        f.attrs[class_at] = Value::Int(class as i64);
        if let Some(n) = name_at {
            f.attrs[n] = Value::Text(crate::pointcloud::class_name(class).to_string());
        }
        if is_ground {
            n_ground += 1;
            grounds.features.push(f.clone());
        } else {
            n_other += 1;
        }
    }
    grounds.fields = out.fields.clone();
    ctx.notes.info(format!(
        "{} ground points, {} others ({}% ground)",
        fmt_count(n_ground as u64),
        fmt_count(n_other as u64),
        fmt_real(((n_ground as f64 / (n_ground + n_other).max(1) as f64) * 1000.0).round() / 10.0)
    ));
    Ok(vec![("Points".into(), out), ("Ground".into(), grounds)])
}

// ---------- Building heights (BuildingHeights) ----------

const BUILDING_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "classes",
        label: "Roof points",
        kind: ParamKind::Text,
        default: "",
        hint: "The classes of the points on the roofs, such as 6 for buildings. Empty takes every point on the footprint that is not ground",
    },
    ParamSpec {
        key: "ground_classes",
        label: "Ground points",
        kind: ParamKind::Text,
        default: "2",
        hint: "Without a terrain model on Ground, the ground is taken from the points of these classes around the building",
    },
    ParamSpec {
        key: "inset",
        label: "Leave out the edge",
        kind: ParamKind::Text,
        default: "0.5",
        hint: "Metres inside the footprint, so that walls, eaves and trees at the edge do not count",
    },
    ParamSpec { key: "prefix", label: "Before the names", kind: ParamKind::Text, default: "", hint: "Such as las_, to give las_height" },
];

/// A plane z = a + b·x + c·y through points, by least squares around their middle, and how
/// far the points lie from it (the root of the mean square).
fn plane(pts: &[(f64, f64, f64)]) -> Option<([f64; 3], f64)> {
    let n = pts.len() as f64;
    if pts.len() < 3 {
        return None;
    }
    let (mx, my, mz) = pts.iter().fold((0.0, 0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n, a.2 + p.2 / n));
    let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (x, y, z) in pts {
        let (dx, dy, dz) = (x - mx, y - my, z - mz);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
        sxz += dx * dz;
        syz += dy * dz;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-9 {
        return None;
    }
    let b = (sxz * syy - syz * sxy) / det;
    let c = (syz * sxx - sxz * sxy) / det;
    let rms = (pts.iter().map(|(x, y, z)| (z - (mz + b * (x - mx) + c * (y - my))).powi(2)).sum::<f64>() / n).sqrt();
    Some(([mz - b * mx - c * my, b, c], rms))
}

/// The shape of a roof from its points: flat, one slope (shed), two slopes meeting at a ridge
/// along the long side or the short side (gable), or something else; with the slope in
/// degrees and the direction of the ridge, degrees from north.
pub fn roof_shape(pts: &[(f64, f64, f64)], axis: f64) -> (String, Option<f64>, Option<f64>) {
    let slope_of = |p: &[f64; 3]| p[1].hypot(p[2]).atan().to_degrees();
    let Some((one, rms1)) = plane(pts) else { return ("unknown".into(), None, None) };
    if rms1 < 0.2 {
        let s = slope_of(&one);
        return if s < 5.0 { ("flat".into(), Some(s), None) } else { ("shed".into(), Some(s), None) };
    }
    // Two halves, each side of a line through the middle, along the axis and across it.
    let n = pts.len() as f64;
    let (mx, my) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
    let mut best: Option<(f64, f64, f64)> = None; // (rms, slope, ridge direction)
    for dir in [axis, axis + 90.0] {
        let (sx, sy) = (dir.to_radians().sin(), dir.to_radians().cos());
        // Points to the left and right of the ridge line through the middle.
        let (left, right): (Vec<_>, Vec<_>) = pts.iter().partition(|(x, y, _)| (x - mx) * sy - (y - my) * sx > 0.0);
        let (Some((a, ra)), Some((b, rb))) = (plane(&left), plane(&right)) else { continue };
        let rms = ((ra * ra * left.len() as f64 + rb * rb * right.len() as f64) / n).sqrt();
        // A gable: both halves slope, up towards the ridge.
        let up = |p: &[f64; 3], side: f64| (p[1] * sy - p[2] * sx) * side < 0.0;
        let (sa, sb) = (slope_of(&a), slope_of(&b));
        if sa > 5.0 && sb > 5.0 && up(&a, 1.0) && up(&b, -1.0) && best.is_none_or(|b| rms < b.0) {
            best = Some((rms, (sa + sb) / 2.0, dir.rem_euclid(180.0)));
        }
    }
    match best {
        Some((rms, s, d)) if rms < 0.25 => ("gable".into(), Some(s), Some(d)),
        // Hipped, with dormers, or more than two planes: one plane says nothing of its slope.
        _ => ("other".into(), None, None),
    }
}

/// The direction of the long side of a polygon, degrees from north, from its smallest
/// rotated rectangle.
fn long_axis(g: &Geometry<f64>) -> f64 {
    use geo::MinimumRotatedRect;
    let Some(r) = g.minimum_rotated_rect() else { return 0.0 };
    let c = &r.exterior().0;
    if c.len() < 4 {
        return 0.0;
    }
    let (a, b) = ((c[1].x - c[0].x, c[1].y - c[0].y), (c[2].x - c[1].x, c[2].y - c[1].y));
    let long = if a.0.hypot(a.1) >= b.0.hypot(b.1) { a } else { b };
    long.0.atan2(long.1).to_degrees().rem_euclid(180.0)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let at = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[at.min(sorted.len() - 1)]
}

fn run_buildings(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let buildings = input(inp, "Buildings");
    let cloud = input(inp, "Points");
    let ground_grid = input(inp, "Ground").grid.clone();
    if cloud.crs.is_some() && buildings.crs.is_some() && cloud.crs != buildings.crs {
        return err(format!(
            "The buildings are in {} and the points in {}. Reproject the buildings to the system of the points first.",
            crs_label(buildings.crs),
            crs_label(cloud.crs)
        ));
    }
    let classes = |key: &str| -> Result<Vec<i64>> {
        p.list(key).iter().map(|c| c.parse::<i64>().map_err(|_| GbError(format!("Classes are numbers, such as 2 or 6, not \"{}\".", c)))).collect()
    };
    let roof_classes = classes("classes")?;
    let ground_classes = {
        let c = classes("ground_classes")?;
        if c.is_empty() {
            vec![2]
        } else {
            c
        }
    };
    let inset = number_param(p, "inset", "Leave out the edge")?.unwrap_or(0.5).max(0.0);
    let prefix = p.get("prefix").unwrap_or_default();
    let class_at = cloud.field_index("classification");
    if class_at.is_none() && ground_grid.is_none() {
        return err("The points have no classes to tell the ground by. Classify the ground points first, or connect a terrain model to Ground.");
    }
    let class_of = |f: &Feature| class_at.and_then(|i| f.attrs[i].as_f64()).map(|c| c as i64);
    // The footprints, made smaller by the edge left out.
    use geo::Buffer;
    let shrunk: Vec<Option<Geometry<f64>>> = buildings
        .features
        .iter()
        .map(|f| {
            let g = f.geom.as_ref()?;
            let polys = geom::polygons_of(g);
            if polys.is_empty() {
                return None;
            }
            let m = geo::MultiPolygon::new(polys);
            let s = if inset > 0.0 { m.buffer(-inset) } else { m.clone() };
            Some(Geometry::MultiPolygon(if s.0.is_empty() { m } else { s }))
        })
        .collect();
    let index = AreaIndex::new(shrunk.iter().map(|g| g.as_ref()));
    let mut roofs: Vec<Vec<(f64, f64, f64)>> = vec![Vec::new(); buildings.len()];
    let mut ground_pts: Vec<rstar::primitives::GeomWithData<[f64; 2], f64>> = Vec::new();
    for f in &cloud.features {
        let (Some((x, y)), Some(z)) = (xy_of(f), z_of(f)) else { continue };
        let c = class_of(f);
        if c.is_some_and(|c| ground_classes.contains(&c)) {
            ground_pts.push(rstar::primitives::GeomWithData::new([x, y], z));
            continue;
        }
        if !roof_classes.is_empty() && !c.is_some_and(|c| roof_classes.contains(&c)) {
            continue;
        }
        for b in index.containing(geo::Coord { x, y }) {
            roofs[b].push((x, y, z));
        }
    }
    let ground_tree = rstar::RTree::bulk_load(ground_pts);
    let mut out = buildings.clone();
    let names = ["ground", "height", "height_max", "roof_low", "points", "roof_shape", "roof_slope", "ridge_direction"];
    let types = [FieldType::Real, FieldType::Real, FieldType::Real, FieldType::Real, FieldType::Int, FieldType::Text, FieldType::Real, FieldType::Real];
    let slots: Vec<usize> = names.iter().zip(types).map(|(n, t)| column(&mut out, &format!("{}{}", prefix, n), t)).collect();
    let round = |v: f64, d: i32| (v * 10f64.powi(d)).round() / 10f64.powi(d);
    let (mut measured, mut without) = (0usize, 0usize);
    use geo::BoundingRect;
    for (k, f) in out.features.iter_mut().enumerate() {
        let Some(g) = &buildings.features[k].geom else { continue };
        let mut pts = std::mem::take(&mut roofs[k]);
        if pts.len() < 3 {
            without += 1;
            f.attrs[slots[4]] = Value::Int(pts.len() as i64);
            continue;
        }
        // The ground: the terrain model at the corners, or the ground points around.
        let ground = match &ground_grid {
            Some(gr) => {
                let mut zs: Vec<f64> = geom::coords(g).iter().filter_map(|c| gr.bilinear(c.x, c.y)).map(|v| v as f64).collect();
                zs.sort_by(f64::total_cmp);
                (!zs.is_empty()).then(|| percentile(&zs, 50.0))
            }
            None => g.bounding_rect().and_then(|r| {
                let env = rstar::AABB::from_corners([r.min().x - 3.0, r.min().y - 3.0], [r.max().x + 3.0, r.max().y + 3.0]);
                let mut zs: Vec<f64> = ground_tree.locate_in_envelope(&env).map(|p| p.data).collect();
                zs.sort_by(f64::total_cmp);
                (!zs.is_empty()).then(|| percentile(&zs, 50.0))
            }),
        };
        pts.sort_by(|a, b| a.2.total_cmp(&b.2));
        let zs: Vec<f64> = pts.iter().map(|p| p.2).collect();
        let base = ground.unwrap_or(zs[0]);
        // The roof: points well above the ground, so that the ground seen between does not count.
        let roof: Vec<(f64, f64, f64)> = pts.iter().copied().filter(|p| p.2 > base + 1.5).collect();
        let (shape, slope, ridge) = if roof.len() >= 6 { roof_shape(&roof, long_axis(g)) } else { ("unknown".into(), None, None) };
        let r_zs: Vec<f64> = if roof.is_empty() { zs.clone() } else { roof.iter().map(|p| p.2).collect() };
        f.attrs[slots[0]] = ground.map_or(Value::Null, |v| Value::Real(round(v, 2)));
        f.attrs[slots[1]] = Value::Real(round(percentile(&r_zs, 90.0) - base, 2));
        f.attrs[slots[2]] = Value::Real(round(r_zs[r_zs.len() - 1] - base, 2));
        f.attrs[slots[3]] = Value::Real(round(percentile(&r_zs, 10.0) - base, 2));
        f.attrs[slots[4]] = Value::Int(pts.len() as i64);
        f.attrs[slots[5]] = Value::Text(shape);
        f.attrs[slots[6]] = slope.map_or(Value::Null, |s| Value::Real(round(s, 1)));
        f.attrs[slots[7]] = ridge.map_or(Value::Null, |d| Value::Real(round(d, 0)));
        measured += 1;
    }
    ctx.notes.info(format!("{} buildings measured", fmt_count(measured as u64)));
    if without > 0 {
        ctx.notes.warn(format!("{} buildings have fewer than three points on them, and were not measured.", fmt_count(without as u64)));
    }
    Ok(vec![("Buildings".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "GroundClassifier",
        title: "Classify ground points",
        hidden: false,
        group: Group::Transformer,
        category: "Point clouds",
        desc: "Finds the ground in a point cloud without classes, as from a drone or a scanner of one's own, by a morphological filter (SMRF): the points on the ground get class 2, the others class 1. The largest object sets how large buildings it takes away. Then Point cloud to raster makes a terrain model of the ground.",
        inputs: &["Points"],
        outputs: &["Points", "Ground"],
        ports: None,
        params: GROUND_PARAMS,
        run: run_ground,
        summary: |p| format!("objects to {} m", p.get("window").unwrap_or_else(|| "20".into())),
    },
    NodeType {
        name: "BuildingHeights",
        title: "Building heights from points",
        hidden: false,
        group: Group::Transformer,
        category: "Point clouds",
        desc: "Measures each building on Buildings from the points of the cloud on Points that lie on its footprint: the ground, the height (90 % of the roof points), the highest point and the low edge of the roof above the ground, and the shape of the roof (flat, shed, gable or other) with its slope and the direction of its ridge. The ground from a terrain model on Ground, or from the ground points around.",
        inputs: &["Buildings", "Points", "Ground"],
        outputs: &["Buildings"],
        ports: None,
        params: BUILDING_PARAMS,
        run: run_buildings,
        summary: |_| String::new(),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Field, Table};

    /// Ground sloping gently, 60 × 60 m at a point every half metre, with a gable house of
    /// 10 × 6 m, ridge along x at 8 m and eaves at 5 m, and a flat-roofed block at 4 m.
    fn town() -> (Table, Table) {
        let mut pts = Vec::new();
        let ground = |x: f64, y: f64| 20.0 + 0.02 * x + 0.01 * y;
        let mut k = 0u64;
        for i in 0..120 {
            for j in 0..120 {
                let (x, y) = (i as f64 * 0.5 + 0.25, j as f64 * 0.5 + 0.25);
                // A little noise, the same each time.
                k = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let noise = ((k >> 40) as f64 / (1u64 << 24) as f64 - 0.5) * 0.06;
                let z = if (10.0..20.0).contains(&x) && (10.0..16.0).contains(&y) {
                    ground(x, y) + 5.0 + 3.0 * (1.0 - (y - 13.0).abs() / 3.0)
                } else if (35.0..50.0).contains(&x) && (30.0..45.0).contains(&y) {
                    ground(x, y) + 4.0
                } else {
                    ground(x, y)
                };
                pts.push(Feature { attrs: vec![], geom: Some(Geometry::Point(geo::Point::new(x, y))), z: Some(Arc::from(vec![z + noise])) });
            }
        }
        let cloud = Table { fields: vec![], crs: Some(3006), features: pts, ..Default::default() };
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
            Feature::new(vec![Value::Int(0)], Some(Geometry::Polygon(geo::Rect::new(geo::coord! {x: x0, y: y0}, geo::coord! {x: x1, y: y1}).to_polygon())))
        };
        let houses = Table {
            fields: vec![Field::new("id", FieldType::Int)],
            crs: Some(3006),
            features: vec![rect(10.0, 10.0, 20.0, 16.0), rect(35.0, 30.0, 50.0, 45.0)],
            ..Default::default()
        };
        (cloud, houses)
    }

    #[test]
    fn the_ground_is_found_and_buildings_measured() {
        let (cloud, houses) = town();
        let (out, notes) = run_for_test("GroundClassifier", &[], vec![("Points", cloud)]).unwrap();
        let t = &out[0].1;
        let c = t.field_index("classification").unwrap();
        let class_at = |x: f64, y: f64| {
            t.features
                .iter()
                .find(|f| matches!(&f.geom, Some(Geometry::Point(p)) if (p.x() - x).abs() < 0.3 && (p.y() - y).abs() < 0.3))
                .map(|f| f.attrs[c].clone())
        };
        assert_eq!(class_at(5.25, 5.25), Some(Value::Int(2)), "open ground: {:?}", notes.info);
        assert_eq!(class_at(15.25, 13.25), Some(Value::Int(1)), "the ridge of the house");
        assert_eq!(class_at(42.25, 37.25), Some(Value::Int(1)), "the middle of the block");
        let grounds = out[1].1.len();
        assert!((13_100..=13_300).contains(&grounds), "{} of 14 400, of which 1 140 are on roofs", grounds);
        let (out, _) = run_for_test("BuildingHeights", &[], vec![("Buildings", houses), ("Points", t.clone())]).unwrap();
        let b = &out[0].1;
        let get = |k: usize, n: &str| b.features[k].attrs[b.field_index(n).unwrap()].clone();
        let num = |k: usize, n: &str| get(k, n).as_f64().unwrap();
        assert_eq!(get(0, "roof_shape"), Value::Text("gable".into()));
        assert!((num(0, "height_max") - 8.0).abs() < 0.3, "{}", num(0, "height_max"));
        assert!((num(0, "roof_slope") - 45.0).abs() < 5.0, "{}", num(0, "roof_slope"));
        assert_eq!(num(0, "ridge_direction"), 90.0, "the ridge runs east and west, along the long side");
        assert_eq!(get(1, "roof_shape"), Value::Text("flat".into()));
        assert!((num(1, "height") - 4.0).abs() < 0.3, "{}", num(1, "height"));
    }
}
