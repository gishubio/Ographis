//! Spatial statistics, added in 0.96: hot spots by Getis-Ord Gi* (Ord and Getis, 1995), spatial
//! autocorrelation by Moran's I (Moran, 1950) with its local form (Anselin, 1995), clusters of
//! points by DBSCAN (Ester and others, 1996), and values moved from one division of the land to
//! another by the share of their area.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Field, FieldType, Value};
use geo::{Area, BooleanOps, BoundingRect, Centroid, ConvexHull, Coord, Geometry, MultiPoint, MultiPolygon, Polygon};
use rstar::{primitives::GeomWithData, RTree, AABB};

// ---------- Neighbours ----------

const NEIGHBOURS: &[(&str, &str)] = &[("distance", "Within a distance"), ("nearest", "The nearest, so many of them")];

const P_NEIGHBOURS: ParamSpec = ParamSpec {
    key: "neighbours",
    label: "Neighbours",
    kind: ParamKind::Choice(NEIGHBOURS),
    default: "distance",
    hint: "Which features count as near each other: those within a distance, or each feature's nearest few",
};
const P_DISTANCE: ParamSpec = ParamSpec {
    key: "distance",
    label: "Within",
    kind: ParamKind::Text,
    default: "",
    hint: "In metres, between the middles of the features. Empty: as far as needed for every feature to have a neighbour",
};
const P_K: ParamSpec = ParamSpec { key: "k", label: "How many nearest", kind: ParamKind::Text, default: "8", hint: "" };

/// The middle of each feature: the point itself, or the centroid of a line or an area.
fn middles(t: &Table) -> Vec<Option<Coord<f64>>> {
    t.features.iter().map(|f| f.geom.as_ref().and_then(|g| g.centroid()).map(|p| p.0)).collect()
}

type Indexed = GeomWithData<[f64; 2], usize>;

/// The neighbours of each feature, without the feature itself, and the distance used.
fn neighbours(p: &P, at: &[Option<Coord<f64>>], ctx: &NodeCtx, crs: Option<u32>) -> Result<(Vec<Vec<usize>>, String)> {
    if ctx.geographic(crs) == Some(true) {
        return err("The features are in degrees. Reproject them to metres first, such as to EPSG:3006, so that distances mean something.");
    }
    let tree: RTree<Indexed> = RTree::bulk_load(at.iter().enumerate().filter_map(|(i, c)| c.map(|c| GeomWithData::new([c.x, c.y], i))).collect());
    let mut out = vec![Vec::new(); at.len()];
    if p.get("neighbours").as_deref() == Some("nearest") {
        let k = number_param(p, "k", "How many nearest")?.unwrap_or(8.0).max(1.0) as usize;
        for (i, c) in at.iter().enumerate() {
            let Some(c) = c else { continue };
            out[i] = tree.nearest_neighbor_iter(&[c.x, c.y]).filter(|n| n.data != i).take(k).map(|n| n.data).collect();
        }
        return Ok((out, format!("the {} nearest", k)));
    }
    let d = match number_param(p, "distance", "Within")? {
        Some(d) if d > 0.0 => d,
        _ => {
            // As far as the loneliest feature's nearest neighbour.
            let far = at
                .iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    let c = (*c)?;
                    tree.nearest_neighbor_iter(&[c.x, c.y]).find(|n| n.data != i).map(|n| ((n.geom()[0] - c.x).powi(2) + (n.geom()[1] - c.y).powi(2)).sqrt())
                })
                .fold(0.0, f64::max);
            far * 1.0001
        }
    };
    for (i, c) in at.iter().enumerate() {
        let Some(c) = c else { continue };
        out[i] = tree.locate_within_distance([c.x, c.y], d * d).filter(|n| n.data != i).map(|n| n.data).collect();
    }
    Ok((out, format!("within {} m", fmt_real((d * 10.0).round() / 10.0))))
}

/// The numbers of an attribute, None where there is none.
fn numbers(t: &Table, attr: &str) -> Result<Vec<Option<f64>>> {
    let i = t.need_field(attr)?;
    Ok(t.features.iter().map(|f| f.attrs[i].as_f64()).collect())
}

/// The share of a normal distribution beyond |z| on both sides.
fn two_sided_p(z: f64) -> f64 {
    erfc(z.abs() / std::f64::consts::SQRT_2)
}

/// The complementary error function, to about 1.2e-7 (Press and others, Numerical Recipes).
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t
        * (-z * z - 1.26551223
            + t * (1.00002368
                + t * (0.37409196
                    + t * (0.09678418 + t * (-0.18628806 + t * (0.27886807 + t * (-1.13520398 + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))))
            .exp();
    if x >= 0.0 {
        r
    } else {
        2.0 - r
    }
}

/// Random numbers that are the same at every run, for permutations (SplitMix64).
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

// ---------- Hot spots (HotSpots) ----------

const HOT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "value",
        label: "Value",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "Such as the number of accidents in each square, or the rate of illness in each area",
    },
    P_NEIGHBOURS,
    P_DISTANCE,
    P_K,
];

fn run_hot_spots(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let attr = p.need("value", "Choose the attribute whose hot spots to find.")?;
    let x = numbers(t, &attr)?;
    let at = middles(t);
    let (near, how) = neighbours(p, &at, ctx, t.crs)?;
    let used: Vec<usize> = (0..t.len()).filter(|i| x[*i].is_some() && at[*i].is_some()).collect();
    let n = used.len() as f64;
    if used.len() < 3 {
        return err("At least three features with a value and a place are needed.");
    }
    let mean = used.iter().map(|i| x[*i].unwrap_or(0.0)).sum::<f64>() / n;
    let s = (used.iter().map(|i| x[*i].unwrap_or(0.0).powi(2)).sum::<f64>() / n - mean * mean).max(0.0).sqrt();
    let mut out = t.clone();
    let (zi, pi, ci) = (column(&mut out, "gi_z", FieldType::Real), column(&mut out, "gi_p", FieldType::Real), column(&mut out, "hot_spot", FieldType::Text));
    // -3 to 3, from cold at 99 % to hot at 99 %, to draw with colours from blue to red.
    let ki = column(&mut out, "hot_class", FieldType::Int);
    let (mut hot, mut cold) = (0, 0);
    for i in 0..t.len() {
        let Some(_) = x[i].filter(|_| at[i].is_some()) else { continue };
        // Gi*: the feature with its neighbours, each weighing 1.
        let mut group: Vec<usize> = near[i].iter().copied().filter(|j| x[*j].is_some()).collect();
        group.push(i);
        let w = group.len() as f64;
        let sum: f64 = group.iter().map(|j| x[*j].unwrap_or(0.0)).sum();
        let below = s * ((n * w - w * w) / (n - 1.0)).max(0.0).sqrt();
        if below <= 0.0 {
            continue;
        }
        let z = (sum - mean * w) / below;
        let pv = two_sided_p(z);
        let level = if pv < 0.01 {
            "99 %"
        } else if pv < 0.05 {
            "95 %"
        } else if pv < 0.10 {
            "90 %"
        } else {
            ""
        };
        let step = match level {
            "99 %" => 3,
            "95 %" => 2,
            "90 %" => 1,
            _ => 0,
        } * if z > 0.0 { 1 } else { -1 };
        let class = match (level, z > 0.0) {
            ("", _) => "Not significant".to_string(),
            (l, true) => {
                hot += 1;
                format!("Hot spot, {}", l)
            }
            (l, false) => {
                cold += 1;
                format!("Cold spot, {}", l)
            }
        };
        let f = &mut out.features[i];
        f.attrs[zi] = Value::Real(z);
        f.attrs[pi] = Value::Real(pv);
        f.attrs[ci] = Value::Text(class);
        f.attrs[ki] = Value::Int(step);
    }
    ctx.notes.info(format!("{} features with neighbours {}: {} hot spots and {} cold spots at 90 % or more", fmt_count(n as u64), how, hot, cold));
    Ok(vec![("Output".into(), out)])
}

// ---------- Spatial autocorrelation (SpatialAutocorrelation) ----------

const AUTO_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "value",
        label: "Value",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "Such as the income or the share of elderly in each area",
    },
    P_NEIGHBOURS,
    P_DISTANCE,
    P_K,
    ParamSpec {
        key: "permutations",
        label: "Permutations",
        kind: ParamKind::Text,
        default: "499",
        hint: "How many times the values are shuffled among the places to tell chance from pattern. More is surer and slower",
    },
    ParamSpec { key: "alpha", label: "Significant below", kind: ParamKind::Text, default: "0.05", hint: "The p-value under which a local cluster counts" },
];

fn run_autocorrelation(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let attr = p.need("value", "Choose the attribute to look for a pattern in.")?;
    let raw = numbers(t, &attr)?;
    let at = middles(t);
    let (near, how) = neighbours(p, &at, ctx, t.crs)?;
    let perms = number_param(p, "permutations", "Permutations")?.unwrap_or(499.0).clamp(9.0, 99_999.0) as usize;
    let alpha = number_param(p, "alpha", "Significant below")?.unwrap_or(0.05);
    // Only the features with a value and a place take part.
    let used: Vec<usize> = (0..t.len()).filter(|i| raw[*i].is_some() && at[*i].is_some()).collect();
    if used.len() < 3 {
        return err("At least three features with a value and a place are needed.");
    }
    let pos: HashMap<usize, usize> = used.iter().enumerate().map(|(k, i)| (*i, k)).collect();
    let n = used.len();
    let mean = used.iter().map(|i| raw[*i].unwrap_or(0.0)).sum::<f64>() / n as f64;
    let sd = (used.iter().map(|i| (raw[*i].unwrap_or(0.0) - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
    if sd == 0.0 {
        return err(format!("{} is the same everywhere, so there is no pattern to find.", attr));
    }
    let z: Vec<f64> = used.iter().map(|i| (raw[*i].unwrap_or(0.0) - mean) / sd).collect();
    let nb: Vec<Vec<usize>> = used.iter().map(|i| near[*i].iter().filter_map(|j| pos.get(j).copied()).collect()).collect();
    let lonely = nb.iter().filter(|v| v.is_empty()).count();
    // Global Moran's I with each row of weights summing to 1.
    let moran = |z: &[f64]| -> f64 {
        let mut s = 0.0;
        let mut rows: f64 = 0.0;
        for (k, js) in nb.iter().enumerate() {
            if js.is_empty() {
                continue;
            }
            rows += 1.0;
            s += z[k] * js.iter().map(|j| z[*j]).sum::<f64>() / js.len() as f64;
        }
        (n as f64 / rows.max(1.0)) * s / z.iter().map(|v| v * v).sum::<f64>()
    };
    let observed = moran(&z);
    let expected = -1.0 / (n as f64 - 1.0);
    let mut rng = Random(0x0596_0001);
    let mut shuffled = z.clone();
    let mut sims = Vec::with_capacity(perms);
    for _ in 0..perms {
        for k in (1..n).rev() {
            let j = rng.below(k + 1);
            shuffled.swap(k, j);
        }
        sims.push(moran(&shuffled));
        if !ctx.gate.going() {
            return err(crate::model::STOPPED);
        }
    }
    let sim_mean = sims.iter().sum::<f64>() / perms as f64;
    let sim_sd = (sims.iter().map(|v| (v - sim_mean).powi(2)).sum::<f64>() / (perms as f64 - 1.0)).sqrt();
    let as_far = if observed >= sim_mean { sims.iter().filter(|v| **v >= observed).count() } else { sims.iter().filter(|v| **v <= observed).count() };
    let p_global = (as_far + 1) as f64 / (perms + 1) as f64;
    let z_global = if sim_sd > 0.0 { (observed - sim_mean) / sim_sd } else { 0.0 };
    // Local Moran: each feature against its neighbours, with the others shuffled around it.
    let mut out = t.clone();
    let (li, lp, lc) =
        (column(&mut out, "local_i", FieldType::Real), column(&mut out, "local_p", FieldType::Real), column(&mut out, "cluster", FieldType::Text));
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (k, i) in used.iter().enumerate() {
        let js = &nb[k];
        if js.is_empty() {
            continue;
        }
        let lag = js.iter().map(|j| z[*j]).sum::<f64>() / js.len() as f64;
        let local = z[k] * lag;
        let mut beyond = 0;
        for _ in 0..perms {
            let mut s = 0.0;
            for _ in 0..js.len() {
                let mut r = rng.below(n - 1);
                if r >= k {
                    r += 1;
                }
                s += z[r];
            }
            let sim = z[k] * s / js.len() as f64;
            if (local >= 0.0 && sim >= local) || (local < 0.0 && sim <= local) {
                beyond += 1;
            }
        }
        let pv = (beyond + 1) as f64 / (perms + 1) as f64;
        let class = if pv >= alpha {
            "Not significant"
        } else {
            match (z[k] >= 0.0, lag >= 0.0) {
                (true, true) => "High-High",
                (false, false) => "Low-Low",
                (true, false) => "High-Low",
                (false, true) => "Low-High",
            }
        };
        *counts.entry(class).or_default() += 1;
        let f = &mut out.features[*i];
        f.attrs[li] = Value::Real(local);
        f.attrs[lp] = Value::Real(pv);
        f.attrs[lc] = Value::Text(class.into());
    }
    let pattern = if p_global >= alpha {
        "no pattern beyond chance"
    } else if observed > expected {
        "like values lie together"
    } else {
        "unlike values lie together"
    };
    let summary = Table {
        fields: vec![
            Field::new("attribute", FieldType::Text),
            Field::new("features", FieldType::Int),
            Field::new("neighbours", FieldType::Text),
            Field::new("morans_i", FieldType::Real),
            Field::new("expected", FieldType::Real),
            Field::new("z", FieldType::Real),
            Field::new("p", FieldType::Real),
            Field::new("pattern", FieldType::Text),
        ],
        features: vec![Feature::new(
            vec![
                Value::Text(attr.clone()),
                Value::Int(n as i64),
                Value::Text(how.clone()),
                Value::Real(observed),
                Value::Real(expected),
                Value::Real(z_global),
                Value::Real(p_global),
                Value::Text(pattern.into()),
            ],
            None,
        )],
        ..Default::default()
    };
    ctx.notes.info(format!(
        "Moran's I {} (p {}): {}. Local clusters: {} High-High, {} Low-Low, {} High-Low, {} Low-High",
        fmt_real((observed * 1000.0).round() / 1000.0),
        fmt_real((p_global * 1000.0).round() / 1000.0),
        pattern,
        counts.get("High-High").unwrap_or(&0),
        counts.get("Low-Low").unwrap_or(&0),
        counts.get("High-Low").unwrap_or(&0),
        counts.get("Low-High").unwrap_or(&0)
    ));
    if lonely > 0 {
        ctx.notes.warn(format!("{} features have no neighbour {} and take no part. Make the distance larger.", fmt_count(lonely as u64), how));
    }
    Ok(vec![("Features".into(), out), ("Summary".into(), summary)])
}

// ---------- Point clusters (PointClusters) ----------

const CLUSTER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "distance", label: "Within", kind: ParamKind::Text, default: "100", hint: "In metres: points this close to each other belong together" },
    ParamSpec {
        key: "min_points",
        label: "At least",
        kind: ParamKind::Text,
        default: "5",
        hint: "Points within the distance of a point, itself counted, for it to be the core of a cluster. Points in no cluster get none",
    },
];

fn run_clusters(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let eps = number_param(p, "distance", "Within")?.unwrap_or(100.0);
    let min = number_param(p, "min_points", "At least")?.unwrap_or(5.0).max(1.0) as usize;
    if eps <= 0.0 {
        return err("Within must be more than 0 m.");
    }
    if ctx.geographic(t.crs) == Some(true) {
        return err("The points are in degrees. Reproject them to metres first, such as to EPSG:3006.");
    }
    let at = middles(t);
    let tree: RTree<Indexed> = RTree::bulk_load(at.iter().enumerate().filter_map(|(i, c)| c.map(|c| GeomWithData::new([c.x, c.y], i))).collect());
    let within = |i: usize| -> Vec<usize> { at[i].map(|c| tree.locate_within_distance([c.x, c.y], eps * eps).map(|n| n.data).collect()).unwrap_or_default() };
    // DBSCAN: a core point has enough points near it; a cluster is the cores that reach each
    // other and the points they reach.
    let mut label: Vec<Option<usize>> = vec![None; t.len()];
    let mut seen = vec![false; t.len()];
    let mut clusters = 0usize;
    for i in 0..t.len() {
        if seen[i] || at[i].is_none() {
            continue;
        }
        seen[i] = true;
        let near = within(i);
        if near.len() < min {
            continue;
        }
        let id = clusters;
        clusters += 1;
        label[i] = Some(id);
        let mut queue = near;
        while let Some(j) = queue.pop() {
            if label[j].is_none() {
                label[j] = Some(id);
            }
            if seen[j] {
                continue;
            }
            seen[j] = true;
            let more = within(j);
            if more.len() >= min {
                queue.extend(more.into_iter().filter(|k| !seen[*k] || label[*k].is_none()));
            }
        }
    }
    let mut size = vec![0usize; clusters];
    for l in label.iter().flatten() {
        size[*l] += 1;
    }
    let mut out = t.clone();
    let (ci, si) = (column(&mut out, "cluster", FieldType::Int), column(&mut out, "cluster_size", FieldType::Int));
    for (f, l) in out.features.iter_mut().zip(&label) {
        if let Some(l) = l {
            f.attrs[ci] = Value::Int(*l as i64 + 1);
            f.attrs[si] = Value::Int(size[*l] as i64);
        }
    }
    // The outline of each cluster.
    let mut hulls = Table { fields: vec![Field::new("cluster", FieldType::Int), Field::new("points", FieldType::Int)], crs: t.crs, ..Default::default() };
    for (c, n) in size.iter().enumerate().take(clusters) {
        let pts: Vec<geo::Point<f64>> = (0..t.len()).filter(|i| label[*i] == Some(c)).filter_map(|i| at[i].map(geo::Point)).collect();
        let hull = MultiPoint::from(pts).convex_hull();
        let geom = if hull.unsigned_area() > 0.0 { Some(Geometry::Polygon(hull)) } else { hull.centroid().map(Geometry::Point) };
        hulls.features.push(Feature::new(vec![Value::Int(c as i64 + 1), Value::Int(*n as i64)], geom));
    }
    let noise = label.iter().zip(&at).filter(|(l, a)| l.is_none() && a.is_some()).count();
    ctx.notes.info(format!("{} clusters, and {} points in none", clusters, fmt_count(noise as u64)));
    Ok(vec![("Points".into(), out), ("Clusters".into(), hulls)])
}

// ---------- Reallocate between areas (AreaReallocator) ----------

const REALLOCATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "counts",
        label: "Counts",
        kind: ParamKind::Attributes("From"),
        default: "",
        hint: "Such as people or jobs: shared out by how much of each area of From lies in each area of To, and added up",
    },
    ParamSpec {
        key: "averages",
        label: "Averages",
        kind: ParamKind::Attributes("From"),
        default: "",
        hint: "Such as a rate or a mean income: the mean in each area of To, weighed by how much of it each area of From covers",
    },
];

/// The areas of a geometry, for the overlay.
fn areas_of(g: &Geometry<f64>) -> Option<MultiPolygon<f64>> {
    let ps: Vec<Polygon<f64>> = crate::geom::polygons_of(g);
    (!ps.is_empty()).then(|| MultiPolygon::new(ps))
}

fn run_reallocate(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use rayon::prelude::*;
    let to = input(inp, "To");
    let from = match (input(inp, "From").crs, to.crs) {
        (Some(a), Some(b)) if a != b => reproject(input(inp, "From"), b, ctx.backend)?,
        _ => input(inp, "From").clone(),
    };
    let counts = p.list("counts");
    let averages = p.list("averages");
    if counts.is_empty() && averages.is_empty() {
        return err("Choose the counts and averages to move from one division to the other.");
    }
    let ci: Vec<usize> = counts.iter().map(|c| from.need_field(c)).collect::<Result<_>>()?;
    let ai: Vec<usize> = averages.iter().map(|c| from.need_field(c)).collect::<Result<_>>()?;
    // The areas of From in an index; points go whole to the area of To they lie in.
    let shapes: Vec<Option<MultiPolygon<f64>>> = from.features.iter().map(|f| f.geom.as_ref().and_then(areas_of)).collect();
    let whole: Vec<f64> = shapes.iter().map(|s| s.as_ref().map_or(0.0, |m| m.unsigned_area())).collect();
    let points: Vec<Option<Coord<f64>>> =
        from.features.iter().zip(&shapes).map(|(f, s)| if s.is_none() { f.geom.as_ref().and_then(|g| g.centroid()).map(|p| p.0) } else { None }).collect();
    type Boxed = GeomWithData<rstar::primitives::Rectangle<[f64; 2]>, usize>;
    let tree: RTree<Boxed> = RTree::bulk_load(
        from.features
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                f.geom
                    .as_ref()
                    .and_then(|g| g.bounding_rect())
                    .map(|r| GeomWithData::new(rstar::primitives::Rectangle::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]), i))
            })
            .collect(),
    );
    // For each area: the sums of the counts, the weighted sums and weights of the averages,
    // and how much of it is covered.
    type Row = (Vec<f64>, Vec<(f64, f64)>, f64);
    let rows: Vec<Row> = to
        .features
        .par_iter()
        .map(|f| {
            let mut sums = vec![0.0; ci.len()];
            let mut means = vec![(0.0, 0.0); ai.len()];
            let mut covered = 0.0;
            let Some(target) = f.geom.as_ref().and_then(areas_of) else { return (sums, means, 0.0) };
            let Some(r) = target.bounding_rect() else { return (sums, means, 0.0) };
            for c in tree.locate_in_envelope_intersecting(&AABB::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y])) {
                let j = c.data;
                let (share, weight) = match (&shapes[j], points[j]) {
                    (Some(src), _) if whole[j] > 0.0 => {
                        let a = target.intersection(src).unsigned_area();
                        (a / whole[j], a)
                    }
                    (None, Some(pt)) if geo::Contains::contains(&target, &geo::Point(pt)) => (1.0, 1.0),
                    _ => continue,
                };
                if share <= 0.0 {
                    continue;
                }
                covered += if shapes[j].is_some() { weight } else { 0.0 };
                for (s, k) in sums.iter_mut().zip(&ci) {
                    *s += from.features[j].attrs[*k].as_f64().unwrap_or(0.0) * share;
                }
                for (m, k) in means.iter_mut().zip(&ai) {
                    if let Some(v) = from.features[j].attrs[*k].as_f64() {
                        m.0 += v * weight;
                        m.1 += weight;
                    }
                }
            }
            let area = target.unsigned_area();
            (sums, means, if area > 0.0 { covered / area } else { 0.0 })
        })
        .collect();
    let mut out = to.clone();
    let cs: Vec<usize> = counts.iter().map(|c| column(&mut out, c, FieldType::Real)).collect();
    let avs: Vec<usize> = averages.iter().map(|c| column(&mut out, c, FieldType::Real)).collect();
    let cov = if shapes.iter().any(|s| s.is_some()) { Some(column(&mut out, "covered", FieldType::Real)) } else { None };
    for (f, (sums, means, covered)) in out.features.iter_mut().zip(rows) {
        for (k, s) in cs.iter().zip(sums) {
            f.attrs[*k] = Value::Real(s);
        }
        for (k, (s, w)) in avs.iter().zip(means) {
            f.attrs[*k] = if w > 0.0 { Value::Real(s / w) } else { Value::Null };
        }
        if let Some(c) = cov {
            f.attrs[c] = Value::Real((covered * 10000.0).round() / 10000.0);
        }
    }
    // What was shared out, against what came in: what lies outside every area of To is lost.
    for (name, k) in counts.iter().zip(&ci) {
        let before: f64 = from.features.iter().map(|f| f.attrs[*k].as_f64().unwrap_or(0.0)).sum();
        let after: f64 = out.features.iter().map(|f| f.attrs[out.field_index(name).expect("added")].as_f64().unwrap_or(0.0)).sum();
        if before > 0.0 && (after - before).abs() / before > 0.005 {
            ctx.notes.warn(format!(
                "{}: {} came in and {} was shared out. The divisions do not cover the same land, or the areas of To overlap.",
                name,
                fmt_real((before * 10.0).round() / 10.0),
                fmt_real((after * 10.0).round() / 10.0)
            ));
        }
    }
    ctx.notes.info(format!("{} areas of From shared among {} areas of To", fmt_count(from.len() as u64), fmt_count(to.len() as u64)));
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "HotSpots",
        title: "Hot spots",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "Finds where high values lie together, and low values: hot and cold spots, by the Getis-Ord Gi* statistic of each feature with its neighbours. Each feature gets gi_z (how far above or below what chance would give), gi_p, hot_spot, such as Hot spot, 99 %, and hot_class from -3 to 3, to colour from blue to red. Accidents counted in squares, illness in areas, prices of sold houses. The features should be in metres.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: HOT_PARAMS,
        run: run_hot_spots,
        summary: |p| p.get("value").map(|v| format!("of {}", v)).unwrap_or_default(),
    },
    NodeType {
        name: "SpatialAutocorrelation",
        title: "Spatial autocorrelation",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "Whether like values lie together, by Moran's I: on Summary the I of all the features, what chance would give, and a p-value from shuffling the values among the places; on Features each feature's local I, its p and its cluster: High-High and Low-Low where like lie together, High-Low and Low-High for one that stands out from its neighbours.",
        inputs: &["Input"],
        outputs: &["Features", "Summary"],
        ports: None,
        params: AUTO_PARAMS,
        run: run_autocorrelation,
        summary: |p| p.get("value").map(|v| format!("of {}", v)).unwrap_or_default(),
    },
    NodeType {
        name: "PointClusters",
        title: "Point clusters",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "Finds clusters of points by DBSCAN: points that have enough others within a distance, and those they reach, form a cluster, of any shape; points far from the rest belong to none. Each point gets its cluster and how many points it has, and the outline of each cluster comes on Clusters. Where the accidents gather, the trees of a forest stand, the visitors of a park stop.",
        inputs: &["Points"],
        outputs: &["Points", "Clusters"],
        ports: None,
        params: CLUSTER_PARAMS,
        run: run_clusters,
        summary: |p| format!("{} within {} m", p.get("min_points").unwrap_or_else(|| "5".into()), p.get("distance").unwrap_or_else(|| "100".into())),
    },
    NodeType {
        name: "AreaReallocator",
        title: "Reallocate between areas",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "Moves values from one division of the land to another that does not follow it, by how much of each area lies in the other: people from the squares of a population grid to the districts, jobs from DeSO to the catchments of the schools. Counts are shared out and added up, averages weighed by area. Points on From, such as addresses with their people, go whole to the area they lie in. Each area of To gets covered, the share of it that From covers.",
        inputs: &["From", "To"],
        outputs: &["Output"],
        ports: None,
        params: REALLOCATE_PARAMS,
        run: run_reallocate,
        summary: |p| p.list("counts").into_iter().chain(p.list("averages")).collect::<Vec<_>>().join(", "),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::run_for_test;
    use geo::polygon;

    /// Squares of 100 m in a grid of 10 × 10, with a value from `f(column, row)`.
    fn squares(f: impl Fn(usize, usize) -> f64) -> Table {
        let mut t = Table { fields: vec![Field::new("v", FieldType::Real)], crs: Some(3006), ..Default::default() };
        for r in 0..10 {
            for c in 0..10 {
                let (x, y) = (c as f64 * 100.0, r as f64 * 100.0);
                let sq = polygon![(x: x, y: y), (x: x + 100.0, y: y), (x: x + 100.0, y: y + 100.0), (x: x, y: y + 100.0)];
                t.features.push(Feature::new(vec![Value::Real(f(c, r))], Some(Geometry::Polygon(sq))));
            }
        }
        t
    }

    fn text_of(t: &Table, name: &str, i: usize) -> String {
        t.features[i].attrs[t.field_index(name).unwrap()].to_text()
    }

    #[test]
    fn a_corner_of_high_values_is_a_hot_spot_and_the_rest_cold_or_nothing() {
        // High in the corner of 3 × 3 squares at the lower left, low and varied elsewhere.
        let t = squares(|c, r| if c < 3 && r < 3 { 50.0 } else { ((c * 7 + r * 3) % 5) as f64 });
        let (out, notes) = run_for_test("HotSpots", &[("value", "v"), ("distance", "150")], vec![("Input", t)]).unwrap();
        let o = &out[0].1;
        assert!(text_of(o, "hot_spot", 11).starts_with("Hot spot, 99"), "{}", text_of(o, "hot_spot", 11));
        assert!(!text_of(o, "hot_spot", 88).starts_with("Hot"), "{}", text_of(o, "hot_spot", 88));
        assert!(notes.info.iter().any(|n| n.contains("hot spots")), "{:?}", notes.info);
    }

    #[test]
    fn like_values_together_give_a_positive_morans_i_and_a_chessboard_a_negative() {
        let halves = squares(|c, _| if c < 5 { 1.0 } else { 10.0 });
        let (out, _) =
            run_for_test("SpatialAutocorrelation", &[("value", "v"), ("distance", "150"), ("permutations", "199")], vec![("Input", halves)]).unwrap();
        let s = &out[1].1;
        let i = s.features[0].attrs[s.field_index("morans_i").unwrap()].as_f64().unwrap();
        let p = s.features[0].attrs[s.field_index("p").unwrap()].as_f64().unwrap();
        assert!(i > 0.7 && p < 0.01, "I {} p {}", i, p);
        assert_eq!(text_of(s, "pattern", 0), "like values lie together");
        // Squares in the middle, with eight neighbours; a corner with three may be so by chance.
        assert_eq!(text_of(&out[0].1, "cluster", 42), "Low-Low");
        assert_eq!(text_of(&out[0].1, "cluster", 47), "High-High");
        let chess = squares(|c, r| ((c + r) % 2) as f64);
        let (out, _) = run_for_test("SpatialAutocorrelation", &[("value", "v"), ("distance", "110"), ("permutations", "99")], vec![("Input", chess)]).unwrap();
        let s = &out[1].1;
        assert!(s.features[0].attrs[s.field_index("morans_i").unwrap()].as_f64().unwrap() < -0.9);
    }

    #[test]
    fn points_close_together_form_clusters_and_lonely_ones_none() {
        let mut t = Table { fields: vec![], crs: Some(3006), ..Default::default() };
        let mut add = |x: f64, y: f64| t.features.push(Feature::new(vec![], Some(Geometry::Point(geo::Point::new(x, y)))));
        for k in 0..10 {
            add(k as f64 * 10.0, 0.0);
            add(1000.0 + k as f64 * 5.0, 1000.0 + (k % 3) as f64 * 5.0);
        }
        add(5000.0, 5000.0);
        let (out, notes) = run_for_test("PointClusters", &[("distance", "20"), ("min_points", "3")], vec![("Points", t)]).unwrap();
        assert_eq!(out[1].1.len(), 2, "{:?}", notes.info);
        let pts = &out[0].1;
        let c = pts.field_index("cluster").unwrap();
        assert_eq!(pts.features[0].attrs[c], pts.features[2].attrs[c]);
        assert_ne!(pts.features[0].attrs[c], pts.features[1].attrs[c]);
        assert_eq!(pts.features[20].attrs[c], Value::Null, "the lonely point is in none");
        assert_eq!(pts.features[0].attrs[pts.field_index("cluster_size").unwrap()], Value::Int(10));
    }

    #[test]
    fn people_in_squares_are_shared_among_districts_by_area() {
        // Four squares of 100 people each, and two districts: the west half and the east half,
        // which splits the middle squares.
        let mut from =
            Table { fields: vec![Field::new("people", FieldType::Int), Field::new("income", FieldType::Real)], crs: Some(3006), ..Default::default() };
        for (k, income) in [100.0, 200.0, 300.0, 400.0].iter().enumerate() {
            let x = k as f64 * 100.0;
            let sq = polygon![(x: x, y: 0.0), (x: x + 100.0, y: 0.0), (x: x + 100.0, y: 100.0), (x: x, y: 100.0)];
            from.features.push(Feature::new(vec![Value::Int(100), Value::Real(*income)], Some(Geometry::Polygon(sq))));
        }
        let mut to = Table { fields: vec![Field::new("district", FieldType::Text)], crs: Some(3006), ..Default::default() };
        for (name, x0, x1) in [("West", 0.0, 150.0), ("East", 150.0, 400.0)] {
            let r = polygon![(x: x0, y: 0.0), (x: x1, y: 0.0), (x: x1, y: 100.0), (x: x0, y: 100.0)];
            to.features.push(Feature::new(vec![Value::Text(name.into())], Some(Geometry::Polygon(r))));
        }
        let (out, notes) = run_for_test("AreaReallocator", &[("counts", "people"), ("averages", "income")], vec![("From", from), ("To", to)]).unwrap();
        let o = &out[0].1;
        let get = |i: usize, n: &str| o.features[i].attrs[o.field_index(n).unwrap()].as_f64().unwrap();
        assert!((get(0, "people") - 150.0).abs() < 1e-6 && (get(1, "people") - 250.0).abs() < 1e-6);
        assert!((get(0, "income") - (100.0 * 100.0 + 200.0 * 50.0) / 150.0).abs() < 1e-6);
        assert!((get(0, "covered") - 1.0).abs() < 1e-6);
        assert!(notes.warnings.is_empty(), "{:?}", notes.warnings);
    }
}
