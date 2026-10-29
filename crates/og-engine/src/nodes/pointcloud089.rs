//! Tools for point clouds added in 0.89: a raster of a point cloud, the height above the
//! ground, and filtering by class, return, height and area. A point cloud is points with
//! heights, as Input reads LAS and LAZ, with classification, intensity and returns.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Grid};
use geo::Geometry;

/// The height of a point, from its z.
fn z_of(f: &Feature) -> Option<f64> {
    f.heights().and_then(|z| z.first().copied())
}

fn xy_of(f: &Feature) -> Option<(f64, f64)> {
    match &f.geom {
        Some(Geometry::Point(p)) => Some((p.x(), p.y())),
        _ => None,
    }
}

/// Empty cells filled from their neighbours with values, a ring at a time, at most `rings`
/// times: holes under buildings and in water, where no ground point is.
pub fn fill_holes(g: &mut Grid, rings: usize) {
    let (w, h) = (g.width, g.height);
    for _ in 0..rings {
        let before = g.values.clone();
        let mut changed = false;
        for r in 0..h {
            for c in 0..w {
                if !before[r * w + c].is_nan() {
                    continue;
                }
                let (mut sum, mut n) = (0f64, 0u32);
                for dr in -1isize..=1 {
                    for dc in -1isize..=1 {
                        let (cc, rr) = (c as isize + dc, r as isize + dr);
                        if cc < 0 || rr < 0 || cc as usize >= w || rr as usize >= h {
                            continue;
                        }
                        let v = before[rr as usize * w + cc as usize];
                        if !v.is_nan() {
                            sum += v as f64;
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    g.values[r * w + c] = (sum / n as f64) as f32;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// A raster of points: for each cell the smallest, largest or mean of their values, how
/// many there are or how many per square unit. The raster covers the points, or `extent`
/// as west, south, east and north when it is given.
pub fn rasterize_points(points: &[(f64, f64, f64)], cell: f64, stat: &str, crs: Option<u32>, extent: Option<[f64; 4]>) -> Result<Grid> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y, _) in points {
        (x0, y0, x1, y1) = (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y));
    }
    if let Some(e) = extent {
        (x0, y0, x1, y1) = (x0.min(e[0]), y0.min(e[1]), x1.max(e[2]), y1.max(e[3]));
    }
    if x0 > x1 {
        return err("There are no points to make a raster of.");
    }
    let left = (x0 / cell).floor() * cell;
    let top = (y1 / cell).floor() * cell + cell;
    let w = (((x1 - left) / cell).floor() as usize + 1).max(1);
    let h = (((top - y0) / cell).floor() as usize + 1).max(1);
    if w * h > GRID_CELLS {
        return err(format!("That would be {} × {} cells, more than a raster may have. Choose larger cells.", fmt_count(w as u64), fmt_count(h as u64)));
    }
    let mut v = vec![f32::NAN; w * h];
    let mut n = vec![0u32; w * h];
    let mut sum = if stat == "mean" { vec![0f64; w * h] } else { Vec::new() };
    for (x, y, z) in points {
        let c = (((x - left) / cell) as usize).min(w - 1);
        let r = (((top - y) / cell) as usize).min(h - 1);
        let i = r * w + c;
        n[i] += 1;
        let cur = &mut v[i];
        match stat {
            "min" => *cur = if cur.is_nan() { *z as f32 } else { cur.min(*z as f32) },
            "mean" => sum[i] += z,
            "count" | "density" => {}
            _ => *cur = if cur.is_nan() { *z as f32 } else { cur.max(*z as f32) },
        }
    }
    let area = cell * cell;
    for i in 0..w * h {
        match stat {
            "mean" if n[i] > 0 => v[i] = (sum[i] / n[i] as f64) as f32,
            "count" if n[i] > 0 => v[i] = n[i] as f32,
            "density" if n[i] > 0 => v[i] = (n[i] as f64 / area) as f32,
            _ => {}
        }
    }
    Ok(Grid { width: w, height: h, values: v, geotransform: [left, cell, 0.0, top, 0.0, -cell], crs, bands: Vec::new() })
}

// ---------- Point cloud to raster (PointCloudRasterizer) ----------

const PC_STATS: &[(&str, &str)] = &[
    ("max", "The highest: a surface model, with trees and roofs"),
    ("min", "The lowest: the ground, from ground points"),
    ("mean", "The mean"),
    ("count", "How many points"),
    ("density", "Points per square metre"),
];

const PC_RASTER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "cell", label: "Cell size", kind: ParamKind::Text, default: "1", hint: "In metres, or the units of the coordinate system" },
    ParamSpec { key: "stat", label: "Each cell gets", kind: ParamKind::Choice(PC_STATS), default: "max", hint: "" },
    ParamSpec {
        key: "value",
        label: "Of",
        kind: ParamKind::Expression("Points"),
        default: "",
        hint: "Empty takes the height of the points. Or an attribute, such as intensity",
    },
    ParamSpec {
        key: "fill",
        label: "Fill holes up to",
        kind: ParamKind::Text,
        default: "0",
        hint: "Cells without points are filled from those around them, as many cells in: holes under buildings in a ground model. 0 fills none",
    },
];

fn run_pc_raster(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let cell = number_param(p, "cell", "Cell size")?.unwrap_or(1.0);
    if cell <= 0.0 {
        return err("The cell size must be more than 0.");
    }
    let stat = p.get("stat").unwrap_or_else(|| "max".into());
    let fill = number_param(p, "fill", "Fill holes up to")?.unwrap_or(0.0).clamp(0.0, 10_000.0) as usize;
    let expr = p.get("value").filter(|v| !v.trim().is_empty());
    let compiled = match &expr {
        Some(e) => Some(crate::expr::compile(e, &t.field_names()).map_err(|e| GbError(format!("Of: {}", e.0)))?),
        None => None,
    };
    let geographic = ctx.geographic(t.crs);
    let mut points: Vec<(f64, f64, f64)> = Vec::with_capacity(t.len());
    let (mut flat, mut not_points) = (0usize, 0usize);
    for (i, f) in t.features.iter().enumerate() {
        let Some((x, y)) = xy_of(f) else {
            not_points += usize::from(f.geom.is_some());
            continue;
        };
        let v = match &compiled {
            Some(e) => match e.eval(&crate::expr::Row { feature: f, index: i, geographic })?.as_f64() {
                Some(v) => v,
                None => continue,
            },
            None if matches!(stat.as_str(), "count" | "density") => 0.0,
            None => match z_of(f) {
                Some(z) => z,
                None => {
                    flat += 1;
                    continue;
                }
            },
        };
        points.push((x, y, v));
    }
    if flat > 0 && points.is_empty() {
        return err("The points have no heights. Read a point cloud (LAS or LAZ), or give the value as an attribute.");
    }
    if flat > 0 {
        ctx.notes.warn(format!("{} points have no height and were left out.", fmt_count(flat as u64)));
    }
    if not_points > 0 {
        ctx.notes.warn(format!("{} features are not points and were left out.", fmt_count(not_points as u64)));
    }
    if geographic == Some(true) {
        ctx.notes.warn("The points are in degrees, so the cell size is in degrees too. Reproject them first to have cells in metres.");
    }
    let mut g = rasterize_points(&points, cell, &stat, t.crs, None)?;
    let empty = g.values.iter().filter(|v| v.is_nan()).count();
    if fill > 0 {
        fill_holes(&mut g, fill);
    }
    let left = g.values.iter().filter(|v| v.is_nan()).count();
    match g.range() {
        Some((lo, hi)) => ctx.notes.info(format!(
            "{} points into {} × {} cells of {}, from {} to {}",
            fmt_count(points.len() as u64),
            g.width,
            g.height,
            fmt_real(cell),
            fmt_real((lo as f64 * 100.0).round() / 100.0),
            fmt_real((hi as f64 * 100.0).round() / 100.0)
        )),
        None => ctx.notes.warn("Every cell is empty."),
    }
    if empty > 0 && fill > 0 {
        ctx.notes.info(format!("{} empty cells filled, {} left empty", fmt_count((empty - left) as u64), fmt_count(left as u64)));
    } else if empty * 10 > g.values.len() && fill == 0 && !matches!(stat.as_str(), "count" | "density") {
        ctx.notes.info(format!("{} cells have no point. Fill holes fills them from those around", fmt_count(empty as u64)));
    }
    Ok(vec![("Raster".into(), g.into_table())])
}

// ---------- Height above ground (HeightAboveGround) ----------

const HAG_REPLACE: &[(&str, &str)] = &[("attribute", "In an attribute"), ("heights", "In an attribute, and as the heights of the points")];

const HAG_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "above_ground", hint: "The height of each point above the ground" },
    ParamSpec {
        key: "ground_classes",
        label: "Ground points",
        kind: ParamKind::Text,
        default: "2",
        hint: "Without a terrain model on Ground, the ground is made of the points of these classes: 2 is ground",
    },
    ParamSpec { key: "cell", label: "Ground cells", kind: ParamKind::Text, default: "1", hint: "The cell size of the ground made of the points" },
    ParamSpec {
        key: "replace",
        label: "The height above ground",
        kind: ParamKind::Choice(HAG_REPLACE),
        default: "attribute",
        hint: "As the heights, trees and roofs stand on flat ground",
    },
];

fn run_hag(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let given = input(inp, "Ground");
    let name = p.get("name").filter(|n| !n.is_empty()).unwrap_or_else(|| "above_ground".into());
    let made;
    let ground: &Grid = match given.grid.as_deref() {
        Some(g) => {
            if g.crs != t.crs && g.crs.is_some() && t.crs.is_some() {
                return err(format!(
                    "The ground is in {} and the points in {}. Reproject one of them, so that they are in the same system.",
                    crs_label(g.crs),
                    crs_label(t.crs)
                ));
            }
            g
        }
        None => {
            if !given.features.is_empty() {
                return err("The stream on Ground has no grid of heights. Read the terrain model with Cells as set to Heights or other values on a grid.");
            }
            let classes: Vec<i64> = p
                .list("ground_classes")
                .iter()
                .map(|c| c.parse::<i64>().map_err(|_| GbError(format!("Ground points are given by class numbers, such as 2, not \"{}\".", c))))
                .collect::<Result<_>>()?;
            let classes = if classes.is_empty() { vec![2] } else { classes };
            let Some(at) = t.field_index("classification") else {
                return err("The points have no classification to find the ground by. Connect a terrain model to Ground.");
            };
            let cell = number_param(p, "cell", "Ground cells")?.unwrap_or(1.0);
            if cell <= 0.0 {
                return err("The ground cells must be more than 0.");
            }
            let pts: Vec<(f64, f64, f64)> = t
                .features
                .iter()
                .filter(|f| f.attrs[at].as_f64().is_some_and(|c| classes.contains(&(c as i64))))
                .filter_map(|f| Some((xy_of(f)?.0, xy_of(f)?.1, z_of(f)?)))
                .collect();
            if pts.is_empty() {
                return err(format!(
                    "No point is of the class {}, so there is no ground to measure from. Connect a terrain model to Ground, or choose other classes.",
                    classes.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")
                ));
            }
            // Over all the points, so that roofs and trees beyond the ground points have ground.
            let mut all = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for (x, y) in t.features.iter().filter_map(xy_of) {
                all = [all[0].min(x), all[1].min(y), all[2].max(x), all[3].max(y)];
            }
            let mut g = rasterize_points(&pts, cell, "min", t.crs, Some(all))?;
            let rings = g.width.max(g.height);
            fill_holes(&mut g, rings);
            ctx.notes.info(format!("The ground made of {} points, in cells of {}", fmt_count(pts.len() as u64), fmt_real(cell)));
            made = g;
            &made
        }
    };
    let replace = p.get("replace").as_deref() == Some("heights");
    let mut out = t.clone();
    let slot = column(&mut out, &name, FieldType::Real);
    let (mut done, mut outside, mut flat) = (0usize, 0usize, 0usize);
    for f in out.features.iter_mut() {
        let (Some((x, y)), Some(z)) = (xy_of(f), z_of(f)) else {
            flat += usize::from(f.geom.is_some());
            continue;
        };
        let Some(gz) = ground.bilinear(x, y) else {
            outside += 1;
            continue;
        };
        let above = ((z - gz as f64) * 1000.0).round() / 1000.0;
        f.attrs[slot] = Value::Real(above);
        if replace {
            f.z = Some(Arc::from(vec![above]));
        }
        done += 1;
    }
    ctx.notes.info(format!("{} points measured from the ground", fmt_count(done as u64)));
    if outside > 0 {
        ctx.notes.warn(format!("{} points lie outside the ground or over empty cells, and got no height above it.", fmt_count(outside as u64)));
    }
    if flat > 0 {
        ctx.notes.warn(format!("{} features are not points with heights and were left as they are.", fmt_count(flat as u64)));
    }
    Ok(vec![("Points".into(), out)])
}

// ---------- Filter point cloud (PointCloudFilter) ----------

const PC_RETURNS: &[(&str, &str)] = &[
    ("all", "All"),
    ("first", "First returns: the tops of trees and roofs"),
    ("last", "Last returns: closest to the ground"),
    ("single", "Single returns: hard surfaces"),
];

const PC_FILTER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "classes",
        label: "Classes",
        kind: ParamKind::Text,
        default: "",
        hint: "The class numbers to keep, such as 2 for ground or 3, 4, 5 for vegetation and 6 for buildings. Empty keeps all",
    },
    ParamSpec { key: "returns", label: "Returns", kind: ParamKind::Choice(PC_RETURNS), default: "all", hint: "" },
    ParamSpec { key: "zmin", label: "Lowest height", kind: ParamKind::Text, default: "", hint: "Of the points, or of the attribute below. Empty has no limit" },
    ParamSpec { key: "zmax", label: "Highest height", kind: ParamKind::Text, default: "", hint: "" },
    ParamSpec {
        key: "height_of",
        label: "Height from",
        kind: ParamKind::Text,
        default: "",
        hint: "Empty uses the heights of the points. An attribute, such as above_ground from Height above ground, uses that",
    },
];

fn run_pc_filter(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let areas = input(inp, "Areas");
    let classes: Vec<i64> = p
        .list("classes")
        .iter()
        .map(|c| c.parse::<i64>().map_err(|_| GbError(format!("Classes are numbers, such as 2 or 6, not \"{}\".", c))))
        .collect::<Result<_>>()?;
    let class_at = t.field_index("classification");
    if !classes.is_empty() && class_at.is_none() {
        return err("The points have no classification. Read a classified point cloud, or leave Classes empty.");
    }
    let returns = p.get("returns").unwrap_or_else(|| "all".into());
    let (ret_at, rets_at) = (t.field_index("return_number"), t.field_index("returns").or_else(|| t.field_index("number_of_returns")));
    if returns != "all" && (ret_at.is_none() || rets_at.is_none()) {
        return err("The points do not say which return they are. Choose All returns.");
    }
    let zmin = number_param(p, "zmin", "Lowest height")?;
    let zmax = number_param(p, "zmax", "Highest height")?;
    let height_at = match p.get("height_of").filter(|h| !h.is_empty()) {
        Some(h) => Some(t.field_index(&h).ok_or_else(|| GbError(format!("The points have no attribute {}.", h)))?),
        None => None,
    };
    // The areas in the system of the points.
    let index = if areas.features.iter().any(|f| f.geom.is_some()) {
        let mut geoms: Vec<Option<Geometry<f64>>> = areas.features.iter().map(|f| f.geom.clone()).collect();
        if let (Some(from), Some(to)) = (areas.crs, t.crs) {
            if from != to {
                let b = ctx.backend("Reprojecting the areas")?;
                geom::transform_all(geoms.iter_mut().flatten(), |xs, ys| b.transform(from, to, xs, ys))?;
            }
        }
        Some(AreaIndex::new(geoms.iter().map(|g| g.as_ref())))
    } else {
        None
    };
    let num = |f: &Feature, i: Option<usize>| i.and_then(|i| f.attrs[i].as_f64());
    let keep = |f: &Feature| -> bool {
        if let Some(c) = class_at.filter(|_| !classes.is_empty()) {
            if !f.attrs[c].as_f64().is_some_and(|v| classes.contains(&(v as i64))) {
                return false;
            }
        }
        let (r, n) = (num(f, ret_at).unwrap_or(1.0), num(f, rets_at).unwrap_or(1.0));
        let by_return = match returns.as_str() {
            "first" => r == 1.0,
            "last" => r == n,
            "single" => n == 1.0,
            _ => true,
        };
        if !by_return {
            return false;
        }
        if zmin.is_some() || zmax.is_some() {
            let z = match height_at {
                Some(i) => f.attrs[i].as_f64(),
                None => z_of(f),
            };
            let Some(z) = z else { return false };
            if zmin.is_some_and(|m| z < m) || zmax.is_some_and(|m| z > m) {
                return false;
            }
        }
        if let Some(ix) = &index {
            let Some((x, y)) = xy_of(f) else { return false };
            if ix.containing(geo::Coord { x, y }).is_empty() {
                return false;
            }
        }
        true
    };
    let (mut kept, mut other) = (t.like(), t.like());
    for f in &t.features {
        if keep(f) {
            kept.features.push(f.clone());
        } else {
            other.features.push(f.clone());
        }
    }
    ctx.notes.info(format!("{} of {} points kept", fmt_count(kept.len() as u64), fmt_count(t.len() as u64)));
    Ok(vec![("Points".into(), kept), ("Other".into(), other)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "PointCloudRasterizer",
        title: "Point cloud to raster",
        hidden: false,
        group: Group::Transformer,
        category: "Point clouds",
        desc: "Makes a raster of a point cloud: a terrain model of the ground points (the lowest), a surface model with trees and roofs (the highest), the mean, how many points or how many per square metre, or of an attribute such as intensity. Holes without points can be filled from the cells around them.",
        inputs: &["Points"],
        outputs: &["Raster"],
        ports: None,
        params: PC_RASTER_PARAMS,
        run: run_pc_raster,
        summary: |p| format!("{} m, {}", p.get("cell").unwrap_or_else(|| "1".into()), p.get("stat").unwrap_or_else(|| "max".into())),
    },
    NodeType {
        name: "HeightAboveGround",
        title: "Height above ground",
        hidden: false,
        group: Group::Transformer,
        category: "Point clouds",
        desc: "Gives each point its height above the ground: from a terrain model on Ground, or from the ground points of the cloud itself. The height of trees and buildings, as with a canopy height model. Can make it the height of the points too.",
        inputs: &["Points", "Ground"],
        outputs: &["Points"],
        ports: None,
        params: HAG_PARAMS,
        run: run_hag,
        summary: |p| p.get("name").unwrap_or_else(|| "above_ground".into()),
    },
    NodeType {
        name: "PointCloudFilter",
        title: "Filter point cloud",
        hidden: false,
        group: Group::Transformer,
        category: "Point clouds",
        desc: "Keeps the points of some classes, of some returns (first, last or single), between two heights, and inside the areas on Areas when they are connected. The rest go to Other.",
        inputs: &["Points", "Areas"],
        outputs: &["Points", "Other"],
        ports: None,
        params: PC_FILTER_PARAMS,
        run: run_pc_filter,
        summary: |p| {
            let c = p.get("classes").unwrap_or_default();
            if c.is_empty() {
                String::new()
            } else {
                format!("classes {}", c)
            }
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Field, Table};
    use geo::polygon;

    /// Ground at 10 m on a 10 × 10 m square, with a tree of 8 m at 5, 5 and a building
    /// 4 m high over the east part.
    fn cloud() -> Table {
        let mut features = Vec::new();
        let mut add = |x: f64, y: f64, z: f64, class: i64, r: i64, n: i64| {
            features.push(Feature {
                attrs: vec![Value::Int(class), Value::Int(r), Value::Int(n)],
                geom: Some(Geometry::Point(geo::Point::new(x, y))),
                z: Some(Arc::from(vec![z])),
            });
        };
        for i in 0..10 {
            for j in 0..10 {
                let (x, y) = (i as f64 + 0.5, j as f64 + 0.5);
                if i < 7 {
                    add(x, y, 10.0, 2, 1, 1);
                } else {
                    add(x, y, 14.0, 6, 1, 1);
                }
            }
        }
        add(5.2, 5.2, 18.0, 5, 1, 2);
        add(5.3, 5.3, 10.0, 2, 2, 2);
        Table {
            fields: vec![Field::new("classification", FieldType::Int), Field::new("return_number", FieldType::Int), Field::new("returns", FieldType::Int)],
            crs: Some(3006),
            features,
            ..Default::default()
        }
    }

    #[test]
    fn a_surface_a_ground_and_heights_above_it() {
        let (out, _) = run_for_test("PointCloudRasterizer", &[("cell", "1")], vec![("Points", cloud())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!((g.width, g.height), (10, 10));
        assert_eq!(g.at(5.5, 5.5), Some(18.0), "the top of the tree");
        assert_eq!(g.at(8.5, 2.5), Some(14.0), "the roof");
        let (out, _) = run_for_test("PointCloudRasterizer", &[("cell", "2"), ("stat", "count")], vec![("Points", cloud())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        assert_eq!(g.values.iter().map(|v| *v as u32).sum::<u32>(), 102);
        // The ground of the ground points, the building filled from around it.
        let (out, _) = run_for_test("HeightAboveGround", &[], vec![("Points", cloud())]).unwrap();
        let t = &out[0].1;
        let at = t.field_index("above_ground").unwrap();
        assert_eq!(t.features[100].attrs[at], Value::Real(8.0), "the tree");
        assert_eq!(t.features[95].attrs[at], Value::Real(4.0), "the roof");
        assert_eq!(t.features[0].attrs[at], Value::Real(0.0));
        let (out, _) = run_for_test("HeightAboveGround", &[("replace", "heights")], vec![("Points", cloud())]).unwrap();
        assert_eq!(z_of(&out[0].1.features[100]), Some(8.0));
        // Filter: buildings, and the first returns above 2 m.
        let (out, _) = run_for_test("PointCloudFilter", &[("classes", "6")], vec![("Points", cloud())]).unwrap();
        assert_eq!((out[0].1.len(), out[1].1.len()), (30, 72));
        let (out, _) = run_for_test("PointCloudFilter", &[("returns", "last"), ("zmin", "15")], vec![("Points", cloud())]).unwrap();
        assert_eq!(out[0].1.len(), 0, "the top of the tree is not a last return");
        let (out, _) = run_for_test("PointCloudFilter", &[("returns", "first"), ("zmin", "15")], vec![("Points", cloud())]).unwrap();
        assert_eq!(out[0].1.len(), 1);
        let square = Table {
            fields: Vec::new(),
            crs: Some(3006),
            features: vec![Feature::new(Vec::new(), Some(Geometry::Polygon(polygon![(x: 0.0, y: 0.0), (x: 2.0, y: 0.0), (x: 2.0, y: 2.0), (x: 0.0, y: 2.0)])))],
            ..Default::default()
        };
        let (out, _) = run_for_test("PointCloudFilter", &[], vec![("Points", cloud()), ("Areas", square)]).unwrap();
        assert_eq!(out[0].1.len(), 4);
    }

    #[test]
    fn a_point_cloud_is_written_and_read_again() {
        let dir = std::env::temp_dir().join(format!("og-las-{}", std::process::id()));
        let path = dir.join("cloud.laz");
        let (n, flat) = crate::pointcloud::write_las(&cloud(), &path, None).unwrap();
        assert_eq!((n, flat), (102, 0));
        let mut notes = crate::io::Notes::default();
        let opts = crate::pointcloud::LasOpts { crs: Some(3006), ..Default::default() };
        let t = crate::pointcloud::read_las(&path, &opts, &mut notes, &|_, _| true).unwrap();
        assert_eq!(t.len(), 102);
        assert_eq!(z_of(&t.features[100]), Some(18.0));
        assert_eq!(t.features[100].attrs[0], Value::Int(5));
        assert_eq!(t.features[100].attrs[4], Value::Int(2), "two returns");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
