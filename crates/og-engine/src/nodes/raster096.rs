//! Raster tools added in 0.96: classification of satellite pictures, by k-means without
//! examples (MacQueen, 1967) or by the nearest class mean with examples drawn as areas; and the
//! hollows of a terrain where water gathers in a cloudburst, found by filling the terrain as
//! water would (the priority flood of Barnes and others, 2014).

use super::geom_transform::number_param;
use super::raster089::{geoms_in, grid_on, Cells};
use super::*;
use crate::model::{err, fmt_count, fmt_real, Field, FieldType, Grid, Value};
use geo::Geometry;

// ---------- Classify image (ImageClassifier) ----------

const CLASSIFY_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "method",
        label: "How",
        kind: ParamKind::Choice(&[
            ("kmeans", "By itself: the cells are put in groups of like colours (k-means)"),
            ("examples", "From examples: areas on Examples with their class, and each cell to the class it is most like"),
        ]),
        default: "kmeans",
        hint: "By itself finds the groups, which you then name. From examples gives the classes you drew",
    },
    ParamSpec { key: "classes", label: "Groups", kind: ParamKind::Text, default: "6", hint: "How many groups k-means makes" },
    ParamSpec {
        key: "class",
        label: "Class of each example",
        kind: ParamKind::Attribute("Examples"),
        default: "class",
        hint: "Such as forest, water, field and town, or numbers",
    },
    ParamSpec {
        key: "bands",
        label: "Bands",
        kind: ParamKind::Text,
        default: "",
        hint: "The bands to use by their numbers from 1, such as 2, 3, 4, 8. Empty uses all",
    },
];

/// The values of the chosen bands of each cell, scaled so that each band counts as much,
/// with NaN where a band has none.
fn features_of(g: &Grid, bands: &[usize]) -> (Vec<Vec<f32>>, Vec<(f64, f64)>) {
    let cols: Vec<&Vec<f32>> = bands.iter().filter_map(|b| g.band(*b)).collect();
    let scale: Vec<(f64, f64)> = cols
        .iter()
        .map(|v| {
            let ok: Vec<f64> = v.iter().filter(|x| !x.is_nan()).map(|x| *x as f64).collect();
            let n = ok.len().max(1) as f64;
            let mean = ok.iter().sum::<f64>() / n;
            let sd = (ok.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
            (mean, if sd > 0.0 { sd } else { 1.0 })
        })
        .collect();
    let rows = (0..g.width * g.height).map(|i| cols.iter().zip(&scale).map(|(v, (m, s))| ((v[i] as f64 - m) / s) as f32).collect()).collect();
    (rows, scale)
}

fn distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

fn nearest(x: &[f32], means: &[Vec<f32>]) -> usize {
    means.iter().enumerate().min_by(|a, b| distance(x, a.1).total_cmp(&distance(x, b.1))).map(|(k, _)| k).unwrap_or(0)
}

/// k-means with the first means spread out (k-means++, Arthur and Vassilvitskii, 2007), on a
/// sample of the cells, with the same start every run.
fn kmeans(cells: &[&Vec<f32>], k: usize) -> Vec<Vec<f32>> {
    let mut seed = 0x0596_00c1u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let step = (cells.len() / 50_000).max(1);
    let sample: Vec<&Vec<f32>> = cells.iter().step_by(step).copied().collect();
    let mut means: Vec<Vec<f32>> = vec![sample[(next() % sample.len() as u64) as usize].clone()];
    while means.len() < k {
        let d: Vec<f64> = sample.iter().map(|x| means.iter().map(|m| distance(x, m)).fold(f32::INFINITY, f32::min) as f64).collect();
        let total: f64 = d.iter().sum();
        if total <= 0.0 {
            break;
        }
        let mut r = (next() % 1_000_000) as f64 / 1_000_000.0 * total;
        let mut pick = 0;
        for (i, v) in d.iter().enumerate() {
            r -= v;
            if r <= 0.0 {
                pick = i;
                break;
            }
        }
        means.push(sample[pick].clone());
    }
    for _ in 0..50 {
        let dims = means[0].len();
        let mut sums = vec![vec![0.0f64; dims]; means.len()];
        let mut counts = vec![0usize; means.len()];
        for x in &sample {
            let c = nearest(x, &means);
            counts[c] += 1;
            for (s, v) in sums[c].iter_mut().zip(x.iter()) {
                *s += *v as f64;
            }
        }
        let mut moved = 0.0;
        for (c, m) in means.iter_mut().enumerate() {
            if counts[c] == 0 {
                continue;
            }
            let new: Vec<f32> = sums[c].iter().map(|s| (s / counts[c] as f64) as f32).collect();
            moved += distance(m, &new);
            *m = new;
        }
        if moved < 1e-6 {
            break;
        }
    }
    means
}

fn run_classify(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Raster"), "Raster")?;
    let count = 1 + g.bands.len();
    let bands: Vec<usize> = match p.list("bands") {
        b if b.is_empty() => (0..count).collect(),
        b => b
            .iter()
            .map(|s| match s.trim().parse::<usize>() {
                Ok(n) if n >= 1 && n <= count => Ok(n - 1),
                _ => err(format!("The raster has bands 1 to {}; {} is not one of them.", count, s)),
            })
            .collect::<Result<_>>()?,
    };
    let (cells, scale) = features_of(g, &bands);
    let valid: Vec<usize> = (0..cells.len()).filter(|i| cells[*i].iter().all(|v| !v.is_nan())).collect();
    if valid.is_empty() {
        return err("No cell has a value in all the bands.");
    }
    let examples = input(inp, "Examples");
    let by_examples = p.get("method").as_deref() == Some("examples");
    // The mean of each class, and its name.
    let (means, names): (Vec<Vec<f32>>, Vec<Value>) = if by_examples {
        if examples.is_empty() {
            return err("Draw areas of known classes, such as forest and water, and connect them to Examples.");
        }
        let attr = p.get("class").unwrap_or_else(|| "class".into());
        let ci = examples.need_field(&attr)?;
        let geoms = geoms_in(examples, g.crs, ctx)?;
        let grid_cells = Cells::of(g);
        let mut groups: Vec<(Value, Vec<f64>, usize)> = Vec::new();
        for (f, geom) in examples.features.iter().zip(&geoms) {
            let Some(geom) = geom else { continue };
            let mut idx = Vec::new();
            grid_cells.of_geometry(geom, &mut idx);
            let name = f.attrs[ci].clone();
            let k = match groups.iter().position(|(n, _, _)| *n == name) {
                Some(k) => k,
                None => {
                    groups.push((name, vec![0.0; bands.len()], 0));
                    groups.len() - 1
                }
            };
            for i in idx.into_iter().filter(|i| cells[*i].iter().all(|v| !v.is_nan())) {
                for (s, v) in groups[k].1.iter_mut().zip(&cells[i]) {
                    *s += *v as f64;
                }
                groups[k].2 += 1;
            }
        }
        let empty: Vec<String> = groups.iter().filter(|g| g.2 == 0).map(|g| g.0.to_text()).collect();
        if !empty.is_empty() {
            ctx.notes.warn(format!("No cell of the raster lies in the examples of {}, so they are not a class.", empty.join(", ")));
        }
        let groups: Vec<_> = groups.into_iter().filter(|g| g.2 > 0).collect();
        if groups.len() < 2 {
            return err("The examples must cover cells of at least two classes.");
        }
        groups.into_iter().map(|(n, s, c)| (s.iter().map(|v| (v / c as f64) as f32).collect(), n)).unzip()
    } else {
        let k = number_param(p, "classes", "Groups")?.unwrap_or(6.0).clamp(2.0, 50.0) as usize;
        let refs: Vec<&Vec<f32>> = valid.iter().map(|i| &cells[*i]).collect();
        let means = kmeans(&refs, k);
        // Numbered from the darkest, by the mean of the bands, so that the numbers mean the
        // same from run to run.
        let mut order: Vec<usize> = (0..means.len()).collect();
        let brightness = |m: &Vec<f32>| m.iter().zip(&scale).map(|(v, (mu, sd))| *v as f64 * sd + mu).sum::<f64>();
        order.sort_by(|a, b| brightness(&means[*a]).total_cmp(&brightness(&means[*b])));
        let sorted: Vec<Vec<f32>> = order.iter().map(|i| means[*i].clone()).collect();
        let names = (1..=sorted.len()).map(|n| Value::Int(n as i64)).collect();
        (sorted, names)
    };
    use rayon::prelude::*;
    let class: Vec<f32> = cells.par_iter().map(|x| if x.iter().any(|v| v.is_nan()) { f32::NAN } else { nearest(x, &means) as f32 + 1.0 }).collect();
    let mut counts = vec![0usize; means.len()];
    for c in class.iter().filter(|c| !c.is_nan()) {
        counts[*c as usize - 1] += 1;
    }
    let out_grid = Grid { values: class, bands: Vec::new(), meta: Default::default(), ..g.clone() };
    let mut table = out_grid.into_table();
    table.crs = g.crs;
    // The classes: their number in the raster, their name, how many cells and the mean of each band.
    let cell_area = (g.geotransform[1] * g.geotransform[5]).abs();
    let mut fields = vec![
        Field::new("value", FieldType::Int),
        Field::new("class", FieldType::Text),
        Field::new("cells", FieldType::Int),
        Field::new("area", FieldType::Real),
    ];
    fields.extend(bands.iter().map(|b| Field::new(format!("band_{}", b + 1), FieldType::Real)));
    let classes = Table {
        fields,
        features: means
            .iter()
            .zip(&names)
            .enumerate()
            .map(|(k, (m, n))| {
                let mut attrs =
                    vec![Value::Int(k as i64 + 1), Value::Text(n.to_text()), Value::Int(counts[k] as i64), Value::Real(counts[k] as f64 * cell_area)];
                attrs.extend(m.iter().zip(&scale).map(|(v, (mu, sd))| Value::Real(((*v as f64 * sd + mu) * 1000.0).round() / 1000.0)));
                Feature::new(attrs, None)
            })
            .collect(),
        ..Default::default()
    };
    ctx.notes.info(format!(
        "{} cells in {} classes, by bands {}",
        fmt_count(valid.len() as u64),
        means.len(),
        bands.iter().map(|b| (b + 1).to_string()).collect::<Vec<_>>().join(", ")
    ));
    Ok(vec![("Raster".into(), table), ("Classes".into(), classes)])
}

// ---------- Hollows (Depressions) ----------

const HOLLOW_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "min_depth", label: "At least as deep as", kind: ParamKind::Text, default: "0.1", hint: "Metres. Shallower hollows are left out" },
    ParamSpec { key: "min_area", label: "At least as large as", kind: ParamKind::Text, default: "100", hint: "Square metres. Smaller hollows are left out" },
];

/// A hollow of a terrain: its cells, how deep it is and what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Hollow {
    pub cells: Vec<usize>,
    pub max_depth: f64,
    pub volume: f64,
    pub area: f64,
    /// The height of its bottom and the height at which it runs over.
    pub bottom: f64,
    pub brim: f64,
}

/// The terrain filled as water fills it, the depth of water in each cell when every hollow is
/// full, and the hollows deeper than `min_depth` and larger than `min_area`.
pub fn hollows(g: &Grid, min_depth: f64, min_area: f64) -> (Vec<f32>, Vec<Hollow>) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let (w, h) = (g.width, g.height);
    let n = w * h;
    let z: Vec<f64> = g.values.iter().map(|v| *v as f64).collect();
    let key = |v: f64| {
        let b = v.to_bits();
        if v >= 0.0 {
            b ^ (1 << 63)
        } else {
            !b
        }
    };
    // From the edge and from empty cells inwards, the lowest first; a cell lower than the one
    // it is reached from fills to that level.
    let mut filled = z.clone();
    let mut done = vec![false; n];
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();
    const N8: [(isize, isize); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    for i in 0..n {
        let (c, r) = (i % w, i / w);
        if z[i].is_nan() {
            done[i] = true;
            continue;
        }
        let edge = c == 0 || r == 0 || c == w - 1 || r == h - 1;
        let by_empty = !edge && N8.iter().any(|(dc, dr)| z[((r as isize + dr) as usize) * w + (c as isize + dc) as usize].is_nan());
        if edge || by_empty {
            done[i] = true;
            heap.push(Reverse((key(z[i]), i as u32)));
        }
    }
    while let Some(Reverse((_, i))) = heap.pop() {
        let i = i as usize;
        let (c, r) = ((i % w) as isize, (i / w) as isize);
        for (dc, dr) in N8 {
            let (cc, rr) = (c + dc, r + dr);
            if cc < 0 || rr < 0 || cc >= w as isize || rr >= h as isize {
                continue;
            }
            let j = rr as usize * w + cc as usize;
            if done[j] {
                continue;
            }
            done[j] = true;
            filled[j] = filled[j].max(filled[i]);
            heap.push(Reverse((key(filled[j]), j as u32)));
        }
    }
    let depth: Vec<f32> = (0..n).map(|i| if z[i].is_nan() { f32::NAN } else { (filled[i] - z[i]) as f32 }).collect();
    // The hollows: cells with water joined side by side.
    let cell_area = (g.geotransform[1] * g.geotransform[5]).abs();
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for s in 0..n {
        if seen[s] || depth[s].is_nan() || depth[s] <= 0.0 {
            continue;
        }
        let mut cells = Vec::new();
        let mut stack = vec![s];
        seen[s] = true;
        while let Some(i) = stack.pop() {
            cells.push(i);
            let (c, r) = ((i % w) as isize, (i / w) as isize);
            for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (cc, rr) = (c + dc, r + dr);
                if cc < 0 || rr < 0 || cc >= w as isize || rr >= h as isize {
                    continue;
                }
                let j = rr as usize * w + cc as usize;
                if !seen[j] && depth[j] > 0.0 {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        let max_depth = cells.iter().map(|i| depth[*i] as f64).fold(0.0, f64::max);
        let area = cells.len() as f64 * cell_area;
        if max_depth < min_depth || area < min_area {
            continue;
        }
        let volume = cells.iter().map(|i| depth[*i] as f64).sum::<f64>() * cell_area;
        let bottom = cells.iter().map(|i| z[*i]).fold(f64::INFINITY, f64::min);
        let brim = cells.iter().map(|i| filled[*i]).fold(f64::NEG_INFINITY, f64::max);
        cells.sort_unstable();
        out.push(Hollow { cells, max_depth, volume, area, bottom, brim });
    }
    out.sort_by(|a, b| b.volume.total_cmp(&a.volume));
    (depth, out)
}

fn run_hollows(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Terrain"), "Terrain")?;
    let min_depth = number_param(p, "min_depth", "At least as deep as")?.unwrap_or(0.1).max(0.0);
    let min_area = number_param(p, "min_area", "At least as large as")?.unwrap_or(100.0).max(0.0);
    let (depth, found) = hollows(g, min_depth, min_area);
    // The hollows numbered on a grid, made into areas.
    let mut label = vec![f32::NAN; g.width * g.height];
    for (k, hl) in found.iter().enumerate() {
        for i in &hl.cells {
            label[*i] = (k + 1) as f32;
        }
    }
    let labels = Grid { values: label, bands: Vec::new(), meta: Default::default(), ..g.clone() };
    let mut fields = vec![
        Field::new("hollow", FieldType::Int),
        Field::new("max_depth", FieldType::Real),
        Field::new("volume_m3", FieldType::Real),
        Field::new("area_m2", FieldType::Real),
        Field::new("bottom", FieldType::Real),
        Field::new("brim", FieldType::Real),
    ];
    let mut areas = Table { crs: g.crs, ..Default::default() };
    if !found.is_empty() {
        let polys = ctx.backend("Hollows")?.polygonize(&labels, "hollow", false)?;
        let at = polys.field_index("hollow").unwrap_or(0);
        let mut parts: Vec<Vec<geo::Polygon<f64>>> = vec![Vec::new(); found.len()];
        for f in &polys.features {
            if let (Some(k), Some(geom)) = (f.attrs[at].as_f64(), &f.geom) {
                if k >= 1.0 && (k as usize) <= found.len() {
                    parts[k as usize - 1].extend(crate::geom::polygons_of(geom));
                }
            }
        }
        for (k, (hl, ps)) in found.iter().zip(parts).enumerate() {
            let geom = match ps.len() {
                0 => None,
                1 => Some(Geometry::Polygon(ps.into_iter().next().expect("one"))),
                _ => Some(Geometry::MultiPolygon(geo::MultiPolygon::new(ps))),
            };
            let r = |v: f64, d: f64| Value::Real((v * d).round() / d);
            areas.features.push(Feature::new(
                vec![Value::Int(k as i64 + 1), r(hl.max_depth, 100.0), r(hl.volume, 10.0), r(hl.area, 10.0), r(hl.bottom, 100.0), r(hl.brim, 100.0)],
                geom,
            ));
        }
    }
    areas.fields = std::mem::take(&mut fields);
    let depth_grid = Grid { values: depth, bands: Vec::new(), meta: Default::default(), ..g.clone() };
    let mut depth_t = depth_grid.into_table();
    depth_t.crs = g.crs;
    let total: f64 = found.iter().map(|h| h.volume).sum();
    ctx.notes.info(format!(
        "{} hollows deeper than {} m and larger than {} m², holding {} m³ when full",
        fmt_count(found.len() as u64),
        fmt_real(min_depth),
        fmt_real(min_area),
        fmt_real(total.round())
    ));
    Ok(vec![("Hollows".into(), areas), ("Depth".into(), depth_t)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "ImageClassifier",
        title: "Classify image",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Gives each cell of a picture with several bands, such as from Sentinel-2, a class: by itself, putting the cells in groups of like colours by k-means, numbered from the darkest, for you to name; or from examples, areas you drew of forest, water, field and town, each cell getting the class it is most like. On Classes, each class with its cells, area and mean of each band. Raster to areas then gives the areas of each class.",
        inputs: &["Raster", "Examples"],
        outputs: &["Raster", "Classes"],
        ports: None,
        params: CLASSIFY_PARAMS,
        run: run_classify,
        summary: |p| if p.get("method").as_deref() == Some("examples") { "from examples".into() } else { format!("{} groups", p.get("classes").unwrap_or_else(|| "6".into())) },
    },
    NodeType {
        name: "Depressions",
        title: "Hollows",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "Finds the hollows of a terrain model where water gathers and stays in a cloudburst, as it would if it could not soak away: each with its greatest depth, the water it holds when full, its area, its bottom and the height at which it runs over. On Depth, how deep the water stands in each cell when every hollow is full. Join buildings to Hollows to see which lie where water gathers; Catchments and streams shows the way it runs there.",
        inputs: &["Terrain"],
        outputs: &["Hollows", "Depth"],
        ports: None,
        params: HOLLOW_PARAMS,
        run: run_hollows,
        summary: |p| format!("deeper than {} m", p.get("min_depth").unwrap_or_else(|| "0.1".into())),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::run_for_test;
    use geo::polygon;

    fn grid(w: usize, h: usize, f: impl Fn(usize, usize) -> f32) -> Grid {
        Grid {
            width: w,
            height: h,
            values: (0..w * h).map(|i| f(i % w, i / w)).collect(),
            geotransform: [0.0, 1.0, 0.0, h as f64, 0.0, -1.0],
            crs: Some(3006),
            bands: Vec::new(),
            meta: Default::default(),
        }
    }

    #[test]
    fn a_bowl_in_a_slope_is_a_hollow_that_holds_its_water() {
        // A slope down to the west, with a bowl 2 m deep and 3 × 3 cells wide in it, and a
        // shallow dent of 5 cm that is left out.
        let g = grid(12, 8, |c, r| {
            let base = c as f32 * 0.5 + 10.0;
            if (4..7).contains(&c) && (2..5).contains(&r) {
                base - 2.0 - 1.0
            } else if c == 9 && r == 6 {
                base - 0.55
            } else {
                base
            }
        });
        let (depth, found) = hollows(&g, 0.1, 1.0);
        assert_eq!(found.len(), 1, "{:?}", found);
        let h = &found[0];
        assert_eq!(h.cells.len(), 9);
        assert!(h.max_depth > 2.0 && h.max_depth < 4.5, "{}", h.max_depth);
        assert!((h.volume - depth.iter().filter(|d| **d > 0.0).take(9).map(|d| *d as f64).sum::<f64>()).abs() < 1.0);
        assert!(h.brim > h.bottom);
    }

    #[test]
    fn cells_fall_into_the_groups_of_their_colours() {
        // Two bands: water dark in both, forest dark and bright in the second, a field bright.
        let kinds = |c: usize| match c / 4 {
            0 => (10.0, 5.0),
            1 => (20.0, 80.0),
            _ => (70.0, 60.0),
        };
        let mut g = grid(12, 4, |c, _| kinds(c).0);
        g.bands = vec![(0..48).map(|i| kinds(i % 12).1).collect()];
        let (out, notes) = run_for_test("ImageClassifier", &[("classes", "3")], vec![("Raster", g.clone().into_table())]).unwrap();
        let classes = out[0].1.grid.as_ref().unwrap();
        assert_eq!(classes.values[0], 1.0, "the darkest is 1");
        assert_eq!(classes.values[0], classes.values[3]);
        assert_ne!(classes.values[0], classes.values[4]);
        assert_ne!(classes.values[4], classes.values[8]);
        assert_eq!(out[1].1.len(), 3, "{:?}", notes.info);
        // From examples: an area over the first column called water, and one over the last called field.
        let mut ex = Table { fields: vec![Field::new("class", FieldType::Text)], crs: Some(3006), ..Default::default() };
        for (name, x) in [("water", 0.0), ("field", 11.0), ("forest", 5.0)] {
            let sq = polygon![(x: x, y: 0.0), (x: x + 1.0, y: 0.0), (x: x + 1.0, y: 4.0), (x: x, y: 4.0)];
            ex.features.push(Feature::new(vec![Value::Text(name.into())], Some(Geometry::Polygon(sq))));
        }
        let (out, _) = run_for_test("ImageClassifier", &[("method", "examples")], vec![("Raster", g.into_table()), ("Examples", ex)]).unwrap();
        let c = out[0].1.grid.as_ref().unwrap();
        let names = &out[1].1;
        let name_of = |v: f32| names.features[v as usize - 1].attrs[1].to_text();
        assert_eq!((name_of(c.values[2]), name_of(c.values[6]), name_of(c.values[9])), ("water".into(), "forest".into(), "field".into()));
    }
}
