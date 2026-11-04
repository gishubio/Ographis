//! Remote sensing: satellite pictures read from the items of a STAC search onto one grid, a
//! picture for each time, with their clouds masked, made into spectral indices such as NDVI,
//! and put together over time into a picture without clouds.
//!
//! A stream of pictures over time is a series (`Table::series`): a grid for each time, each
//! with the names of its bands and its time, and a feature for each with its outline. The
//! first grid is also the grid of the stream, so that the other raster tools take it.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Grid, GridMeta};
use std::sync::Arc;

// ---------- Series of grids ----------

/// The grids of a stream: its series, or its one grid.
pub fn grids_of(t: &Table) -> Vec<Grid> {
    match (&t.series, &t.grid) {
        (Some(s), _) => s.as_ref().clone(),
        (None, Some(g)) => vec![g.as_ref().clone()],
        _ => Vec::new(),
    }
}

/// A stream of grids over time: the first as the grid of the stream, and a feature for each
/// with its outline, time, size and the share of its cells with a value.
/// Picks `most` of the items, which are in the order of their times: the time is cut into
/// `most` runs of about as many items each, and from each run the one with the least
/// `cloud`. A summer then keeps pictures from all its months, not only the first.
fn spread_over_time(order: &[usize], most: usize, cloud: impl Fn(usize) -> f64) -> Vec<usize> {
    let n = order.len();
    if n <= most {
        return order.to_vec();
    }
    (0..most)
        .filter_map(|k| {
            let run = &order[k * n / most..(k + 1) * n / most];
            run.iter().copied().min_by(|a, b| cloud(*a).total_cmp(&cloud(*b)))
        })
        .collect()
}

pub fn series_table(grids: Vec<Grid>, extra: &[(&str, FieldType, Vec<Value>)]) -> Table {
    let mut fields = vec![
        Field::new("time", FieldType::Text),
        Field::new("bands", FieldType::Text),
        Field::new("columns", FieldType::Int),
        Field::new("rows", FieldType::Int),
        Field::new("cell_size", FieldType::Real),
        Field::new("with_value", FieldType::Real),
    ];
    fields.extend(extra.iter().map(|(n, t, _)| Field::new(*n, *t)));
    let features = grids
        .iter()
        .enumerate()
        .map(|(k, g)| {
            let filled = g.values.iter().filter(|v| !v.is_nan()).count() as f64 / g.values.len().max(1) as f64;
            let mut attrs = vec![
                g.meta.time.clone().map_or(Value::Null, Value::Text),
                Value::Text(g.meta.names.join(", ")),
                Value::Int(g.width as i64),
                Value::Int(g.height as i64),
                Value::Real(g.cell_size().0),
                Value::Real((filled * 1000.0).round() / 10.0),
            ];
            attrs.extend(extra.iter().map(|(_, _, v)| v[k].clone()));
            Feature { z: None, attrs, geom: Some(geo::Geometry::Polygon(g.outline())) }
        })
        .collect();
    let crs = grids.first().and_then(|g| g.crs);
    let first = grids.first().cloned().map(Arc::new);
    Table { fields, crs, features, grid: first, series: Some(Arc::new(grids)), ..Default::default() }
}

/// The names a band goes by in the catalogues: Sentinel-2 at Element 84 and Copernicus,
/// and Landsat Collection 2.
const ALIASES: &[(&str, &[&str])] = &[
    ("coastal", &["coastal", "b01", "b1"]),
    ("blue", &["blue", "b02", "b2"]),
    ("green", &["green", "b03", "b3"]),
    ("red", &["red", "b04", "b4"]),
    ("rededge1", &["rededge1", "b05"]),
    ("rededge2", &["rededge2", "b06"]),
    ("rededge3", &["rededge3", "b07"]),
    ("nir", &["nir", "b08", "nir08", "b5", "b8a", "nir08a"]),
    ("swir16", &["swir16", "b11", "b6"]),
    ("swir22", &["swir22", "b12", "b7"]),
    ("scl", &["scl"]),
    ("qa_pixel", &["qa_pixel", "fmask"]),
];

/// The band of a grid that goes by a common name, such as red or nir, under any of its names.
pub fn band_by_role(g: &Grid, role: &str) -> Option<usize> {
    if let Some(i) = g.band_index(role) {
        return Some(i);
    }
    let names = ALIASES.iter().find(|(r, _)| r.eq_ignore_ascii_case(role))?.1;
    g.meta.names.iter().position(|n| {
        let n = n.to_lowercase();
        let n = n.strip_prefix("asset_").unwrap_or(&n);
        names.contains(&n)
    })
}

/// Bands that hold classes or bits rather than reflectance, which are not scaled and are
/// read by the nearest cell.
fn is_class_band(name: &str) -> bool {
    let n = name.to_lowercase();
    let n = n.strip_prefix("asset_").unwrap_or(&n);
    matches!(n, "scl" | "qa_pixel" | "fmask" | "qa" | "cloud" | "mask" | "qa_radsat")
}

// ---------- Read satellite bands (SceneReader) ----------

const SCENE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "bands",
        label: "Bands",
        kind: ParamKind::Text,
        default: "red, nir, scl",
        hint:
            "The files of the items to read, by name, separated by commas: red, green, blue, nir, swir16 and scl at Element 84; B04, B08 and SCL at Copernicus",
    },
    ParamSpec {
        key: "cell",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "10",
        hint: "In metres. Sentinel-2 has 10 m for red, green, blue and nir, 20 m for the others",
    },
    ParamSpec {
        key: "crs",
        label: "Coordinate system",
        kind: ParamKind::Crs,
        default: "",
        hint: "Of the grid. Empty: that of Extent, or SWEREF 99 TM when it is in degrees",
    },
    ParamSpec {
        key: "bbox",
        label: "Area in degrees",
        kind: ParamKind::Text,
        default: "",
        hint: "West, south, east and north, when nothing is connected to Extent",
    },
    ParamSpec {
        key: "scale",
        label: "Scale",
        kind: ParamKind::Text,
        default: "0.0001",
        hint: "The values of the bands are multiplied by it, to reflectance from 0 to 1. Classes, such as scl, are not",
    },
    ParamSpec {
        key: "offset",
        label: "Offset",
        kind: ParamKind::Text,
        default: "0",
        hint: "Added after the scale. Sentinel-2 from 2022 at Copernicus has −0.1; Element 84 has already taken it away",
    },
    ParamSpec {
        key: "most",
        label: "At most pictures",
        kind: ParamKind::Text,
        default: "20",
        hint: "Spread over the time, the least cloudy in each part; each is read over the web",
    },
];

/// The area of a grid in `crs`: the extent of what comes in on Extent, or the rectangle in
/// degrees.
fn target_area(p: &P, extent: &Table, ctx: &mut NodeCtx) -> Result<([f64; 4], u32)> {
    use geo::BoundingRect;
    let b = ctx.backend("Reading satellite pictures")?;
    let given = p.crs("crs")?;
    let (rect, from) = if extent.has_geometry() {
        let mut r: Option<[f64; 4]> = None;
        for g in extent.features.iter().filter_map(|f| f.geom.as_ref()).filter_map(|g| g.bounding_rect()) {
            r = Some(match r {
                None => [g.min().x, g.min().y, g.max().x, g.max().y],
                Some(o) => [o[0].min(g.min().x), o[1].min(g.min().y), o[2].max(g.max().x), o[3].max(g.max().y)],
            });
        }
        (r.expect("geometry"), extent.crs.ok_or_else(|| GbError("The stream on Extent has no coordinate system.".into()))?)
    } else {
        let t = p.need("bbox", "Connect an area to Extent, or write the area in degrees: west, south, east and north.")?;
        let n: Vec<f64> = t.split([',', ' ', ';']).filter(|s| !s.trim().is_empty()).filter_map(|s| s.trim().parse().ok()).collect();
        if n.len() != 4 {
            return err("The area must be four numbers: west, south, east and north.");
        }
        ([n[0], n[1], n[2], n[3]], 4326)
    };
    let to = given.unwrap_or(if from == 4326 || ctx.geographic(Some(from)) == Some(true) { 3006 } else { from });
    if to == from {
        return Ok((rect, to));
    }
    // The corners and the middles of the sides, so that the area is covered when it turns.
    let (mut xs, mut ys) = (vec![], vec![]);
    for (fx, fy) in [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)] {
        xs.push(rect[0] + fx * (rect[2] - rect[0]));
        ys.push(rect[1] + fy * (rect[3] - rect[1]));
    }
    b.transform(from, to, &mut xs, &mut ys)?;
    let lo = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Ok(([lo(&xs), lo(&ys), hi(&xs), hi(&ys)], to))
}

fn run_scenes(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let items = input(inp, "Items");
    let extent = input(inp, "Extent");
    let cell = number_param(p, "cell", "Cell size")?.unwrap_or(10.0);
    if cell <= 0.0 {
        return err("The cell size must be more than 0.");
    }
    let scale = number_param(p, "scale", "Scale")?.unwrap_or(0.0001) as f32;
    let offset = number_param(p, "offset", "Offset")?.unwrap_or(0.0) as f32;
    let most = number_param(p, "most", "At most pictures")?.unwrap_or(20.0).max(1.0) as usize;
    let bands: Vec<String> = p.list("bands");
    if bands.is_empty() {
        return err("Write the bands to read, such as red, nir, scl.");
    }
    // The column of each band among the attributes of the items.
    let mut columns = Vec::new();
    for b in &bands {
        let name = b.strip_prefix("asset_").unwrap_or(b);
        let at = items
            .field_index(&format!("asset_{}", name))
            .or_else(|| items.field_index(name))
            .or_else(|| items.fields.iter().position(|f| f.name.strip_prefix("asset_").is_some_and(|n| n.eq_ignore_ascii_case(name))));
        let Some(at) = at else {
            let have: Vec<String> = items.fields.iter().filter_map(|f| f.name.strip_prefix("asset_").map(str::to_string)).collect();
            return err(format!(
                "The items have no file {}. They have {}.",
                name,
                if have.is_empty() { "none; connect the Items of STAC search".into() } else { have.join(", ") }
            ));
        };
        columns.push((name.to_string(), at));
    }
    let time_at = items.field_index("datetime");
    let (rect, crs) = target_area(p, extent, ctx)?;
    let (w, h) = (((rect[2] - rect[0]) / cell).ceil().max(1.0) as usize, ((rect[3] - rect[1]) / cell).ceil().max(1.0) as usize);
    if w * h > GRID_CELLS {
        return err(format!(
            "The area would be {} × {} cells, more than a raster may have. Choose a smaller area or larger cells.",
            fmt_count(w as u64),
            fmt_count(h as u64)
        ));
    }
    let gt = [rect[0], cell, 0.0, rect[3], 0.0, -cell];
    // The items in the order of their times.
    let mut order: Vec<usize> = (0..items.len()).collect();
    let time_of = |i: usize| time_at.map(|a| items.features[i].attrs[a].to_text()).unwrap_or_default();
    order.sort_by_key(|i| time_of(*i));
    if order.len() > most {
        let cloud_at = items.field_index("cloud_cover");
        let cloud = |i: usize| cloud_at.and_then(|a| items.features[i].attrs[a].as_f64()).unwrap_or(f64::MAX);
        let n = order.len();
        order = spread_over_time(&order, most, cloud);
        ctx.notes.info(format!(
            "{} of {} pictures, spread over the time{}; raise At most pictures for more",
            most,
            n,
            if cloud_at.is_some() { ", the least cloudy in each part" } else { "" }
        ));
    }
    let b = ctx.backend("Reading satellite pictures")?;
    let (progress, gate) = (ctx.progress, ctx.gate);
    let mut grids = Vec::new();
    let mut ids = Vec::new();
    let (mut failed, mut first_failure) = (0usize, None);
    for (k, i) in order.iter().enumerate() {
        if !gate.going() {
            return err(crate::model::STOPPED);
        }
        progress("pictures", k as u64, Some(order.len() as u64));
        let f = &items.features[*i];
        let mut layers: Vec<Vec<f32>> = Vec::new();
        let mut problem = None;
        for (name, at) in &columns {
            let href = f.attrs[*at].to_text();
            if href.is_empty() {
                problem = Some(format!("has no file {}", name));
                break;
            }
            // A file of its own, as in a catalogue kept beside the model, is found from there.
            let href = if href.contains("://") || href.starts_with("/vsi") { href } else { ctx.resolve(&href).to_string_lossy().into_owned() };
            match b.read_onto(&href, gt, w, h, crs, is_class_band(name)) {
                Ok(mut v) => {
                    if !is_class_band(name) && (scale != 1.0 || offset != 0.0) {
                        for x in v.iter_mut().filter(|x| !x.is_nan()) {
                            *x = *x * scale + offset;
                        }
                    }
                    layers.push(v);
                }
                Err(e) => {
                    problem = Some(e.0);
                    break;
                }
            }
        }
        if let Some(why) = problem {
            failed += 1;
            let note = format!("The picture of {} was left out: {}", time_of(*i), why);
            first_failure.get_or_insert_with(|| note.clone());
            ctx.notes.warn(note);
            continue;
        }
        // A picture that does not reach the area is left out.
        if layers[0].iter().all(|v| v.is_nan()) {
            continue;
        }
        let values = layers.remove(0);
        let meta = GridMeta { names: columns.iter().map(|c| c.0.clone()).collect(), time: Some(time_of(*i)).filter(|t| !t.is_empty()) };
        grids.push(Grid { width: w, height: h, values, geotransform: gt, crs: Some(crs), bands: layers, meta });
        ids.push(items.field_index("id").map_or(Value::Null, |a| f.attrs[a].clone()));
    }
    if grids.is_empty() {
        // The warnings are not shown when the tool fails, so the first one is told here.
        return match first_failure {
            Some(why) if failed > 1 => err(format!("No picture could be read. {} (and {} more)", why, failed - 1)),
            Some(why) => err(format!("No picture could be read. {}", why)),
            None => err("No picture reaches into the area."),
        };
    }
    ctx.notes.info(format!("{} pictures of {} × {} cells of {}, {} bands each, in {}", grids.len(), w, h, fmt_real(cell), columns.len(), crs_label(Some(crs))));
    Ok(vec![("Rasters".into(), series_table(grids, &[("id", FieldType::Text, ids)]))])
}

// ---------- Mask clouds (CloudMasker) ----------

const MASK_WHAT: &[(&str, &str)] = &[("cloud", "Clouds"), ("shadow", "Cloud shadows"), ("cirrus", "Thin cirrus"), ("snow", "Snow and ice"), ("water", "Water")];

const MASK_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "mask",
        label: "Mask",
        kind: ParamKind::Picks(MASK_WHAT),
        default: "cloud, shadow, cirrus",
        hint: "Cells of these become empty in every band",
    },
    ParamSpec { key: "grow", label: "Grow the mask by", kind: ParamKind::Text, default: "0", hint: "Cells around each masked cell, for the edges of clouds" },
    ParamSpec {
        key: "least",
        label: "Least clear share",
        kind: ParamKind::Text,
        default: "0",
        hint: "In percent: pictures with less of the area left are left out",
    },
];

/// Whether a cell of the mask band is to be masked: Sentinel-2's scene classes, or the bits of
/// Landsat's QA_PIXEL.
fn masked(v: f32, landsat: bool, what: &[String]) -> bool {
    if v.is_nan() {
        return true;
    }
    let has = |w: &str| what.iter().any(|x| x == w);
    if landsat {
        let q = v as u32;
        let bit = |b: u32| q & (1 << b) != 0;
        bit(0)
            || (has("cloud") && (bit(1) || bit(3)))
            || (has("cirrus") && bit(2))
            || (has("shadow") && bit(4))
            || (has("snow") && bit(5))
            || (has("water") && bit(7))
    } else {
        match v as i32 {
            0 | 1 => true,
            3 => has("shadow"),
            8 | 9 => has("cloud"),
            10 => has("cirrus"),
            11 => has("snow"),
            6 => has("water"),
            _ => false,
        }
    }
}

fn run_mask(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Rasters");
    let grids = grids_of(t);
    if grids.is_empty() {
        return err("Connect satellite pictures to Rasters, such as from Read satellite bands.");
    }
    let what = {
        let l = p.list("mask");
        if l.is_empty() {
            vec!["cloud".to_string(), "shadow".into(), "cirrus".into()]
        } else {
            l
        }
    };
    let grow = number_param(p, "grow", "Grow the mask by")?.unwrap_or(0.0).clamp(0.0, 50.0) as usize;
    let least = number_param(p, "least", "Least clear share")?.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
    let mut out = Vec::new();
    let mut shares = Vec::new();
    let mut dropped = 0usize;
    for mut g in grids {
        let (at, landsat) = match (band_by_role(&g, "scl"), band_by_role(&g, "qa_pixel")) {
            (Some(i), _) => (i, false),
            (None, Some(i)) => (i, true),
            _ => return err("The pictures have no band to find the clouds by: read scl with Sentinel-2, or qa_pixel with Landsat."),
        };
        let qa = g.band(at).expect("band").clone();
        let (w, h) = (g.width, g.height);
        let mut mask: Vec<bool> = qa.iter().map(|v| masked(*v, landsat, &what)).collect();
        // Grown by whole cells, a ring at a time.
        for _ in 0..grow {
            let before = mask.clone();
            for r in 0..h {
                for c in 0..w {
                    if before[r * w + c] {
                        continue;
                    }
                    let near = (r.saturating_sub(1)..=(r + 1).min(h - 1)).any(|rr| (c.saturating_sub(1)..=(c + 1).min(w - 1)).any(|cc| before[rr * w + cc]));
                    if near {
                        mask[r * w + c] = true;
                    }
                }
            }
        }
        let clear = mask.iter().filter(|m| !**m).count() as f64 / mask.len().max(1) as f64;
        if clear < least || clear == 0.0 {
            dropped += 1;
            continue;
        }
        for k in 0..g.band_count() {
            if k == at {
                continue;
            }
            let band = if k == 0 { &mut g.values } else { &mut g.bands[k - 1] };
            for (v, m) in band.iter_mut().zip(&mask) {
                if *m {
                    *v = f32::NAN;
                }
            }
        }
        shares.push(Value::Real((clear * 1000.0).round() / 10.0));
        out.push(g);
    }
    if out.is_empty() {
        return err("Every picture is too cloudy. Lower Least clear share, or search a longer time.");
    }
    ctx.notes.info(format!("{} pictures masked{}", out.len(), if dropped > 0 { format!(", {} too cloudy left out", dropped) } else { String::new() }));
    Ok(vec![("Rasters".into(), series_table(out, &[("clear", FieldType::Real, shares)]))])
}

// ---------- Spectral index (SpectralIndex) ----------

pub(super) const INDICES: &[(&str, &str)] = &[
    ("ndvi", "NDVI: vegetation, (nir − red) / (nir + red)"),
    ("ndwi", "NDWI: water, (green − nir) / (green + nir)"),
    ("ndmi", "NDMI: moisture, (nir − swir16) / (nir + swir16)"),
    ("ndbi", "NDBI: built-up, (swir16 − nir) / (swir16 + nir)"),
    ("nbr", "NBR: burnt, (nir − swir22) / (nir + swir22)"),
    ("evi", "EVI: vegetation where it is dense"),
    ("savi", "SAVI: vegetation where the soil shows"),
    ("own", "An expression of my own"),
];

const INDEX_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "index",
        label: "Index",
        kind: ParamKind::Choice(INDICES),
        default: "ndvi",
        hint: "The bands are found by their names, for Sentinel-2 and Landsat alike",
    },
    ParamSpec {
        key: "expression",
        label: "Expression",
        kind: ParamKind::Text,
        default: "",
        hint: "With the names of the bands, such as (nir - red) / (nir + red)",
    },
    ParamSpec { key: "name", label: "Name of the band", kind: ParamKind::Text, default: "", hint: "Empty: the name of the index" },
    ParamSpec {
        key: "keep",
        label: "The other bands",
        kind: ParamKind::Choice(&[("keep", "Kept, with the index added"), ("drop", "Left out: only the index")]),
        default: "keep",
        hint: "",
    },
];

/// The value of an index from the bands of a cell, by their roles.
fn index_value(which: &str, b: &dyn Fn(&str) -> f32) -> f32 {
    let nd = |a: f32, c: f32| if a + c == 0.0 { f32::NAN } else { (a - c) / (a + c) };
    match which {
        "ndvi" => nd(b("nir"), b("red")),
        "ndwi" => nd(b("green"), b("nir")),
        "ndmi" => nd(b("nir"), b("swir16")),
        "ndbi" => nd(b("swir16"), b("nir")),
        "nbr" => nd(b("nir"), b("swir22")),
        "evi" => {
            let (n, r, bl) = (b("nir"), b("red"), b("blue"));
            let d = n + 6.0 * r - 7.5 * bl + 1.0;
            if d == 0.0 {
                f32::NAN
            } else {
                2.5 * (n - r) / d
            }
        }
        "savi" => {
            let (n, r) = (b("nir"), b("red"));
            1.5 * (n - r) / (n + r + 0.5)
        }
        _ => f32::NAN,
    }
}

/// The bands an index needs.
fn index_roles(which: &str) -> &'static [&'static str] {
    match which {
        "ndvi" | "savi" => &["nir", "red"],
        "ndwi" => &["green", "nir"],
        "ndmi" | "ndbi" => &["nir", "swir16"],
        "nbr" => &["nir", "swir22"],
        "evi" => &["nir", "red", "blue"],
        _ => &[],
    }
}

fn run_index(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Rasters");
    let grids = grids_of(t);
    if grids.is_empty() {
        return err("Connect satellite pictures to Rasters, such as from Read satellite bands.");
    }
    let which = p.get("index").unwrap_or_else(|| "ndvi".into());
    let name = p.get("name").filter(|n| !n.trim().is_empty()).map(|n| n.trim().to_string()).unwrap_or_else(|| which.clone());
    let keep = p.get("keep").as_deref() != Some("drop");
    let mut out = Vec::new();
    for mut g in grids {
        let n = g.values.len();
        // Every band at once, so that the formula reads a cell's bands by role.
        let values: Vec<f32> = if which == "own" {
            let text = p.need("expression", "Write the expression, such as (nir - red) / (nir + red).")?;
            let names: Vec<String> = (0..g.band_count()).map(|k| g.meta.names.get(k).cloned().unwrap_or_else(|| format!("band_{}", k + 1))).collect();
            let compiled = crate::expr::compile(&text, &names).map_err(|e| GbError(format!("Expression: {}", e.0)))?;
            let mut f = Feature::new(vec![Value::Null; names.len()], None);
            let mut v = Vec::with_capacity(n);
            for i in 0..n {
                for k in 0..names.len() {
                    let x = g.band(k).expect("band")[i];
                    f.attrs[k] = if x.is_nan() { Value::Null } else { Value::Real(x as f64) };
                }
                v.push(compiled.eval(&crate::expr::Row { feature: &f, index: i, geographic: None })?.as_f64().map_or(f32::NAN, |x| x as f32));
            }
            v
        } else {
            let mut at = std::collections::HashMap::new();
            for r in index_roles(&which) {
                let i = band_by_role(&g, r)
                    .ok_or_else(|| GbError(format!("The pictures have no {} band for {}. They have {}.", r, which.to_uppercase(), g.meta.names.join(", "))))?;
                at.insert(*r, i);
            }
            (0..n)
                .map(|i| {
                    let b = |r: &str| at.get(r).map_or(f32::NAN, |k| g.band(*k).expect("band")[i]);
                    index_value(&which, &b)
                })
                .collect()
        };
        if keep {
            let first = std::mem::replace(&mut g.values, values);
            g.bands.insert(0, first);
            let mut names = if g.meta.names.is_empty() { (1..=g.band_count() - 1).map(|k| format!("band_{}", k)).collect() } else { g.meta.names.clone() };
            names.insert(0, name.clone());
            g.meta.names = names;
        } else {
            g.values = values;
            g.bands.clear();
            g.meta.names = vec![name.clone()];
        }
        out.push(g);
    }
    if let Some((lo, hi)) = out[0].range() {
        ctx.notes.info(format!(
            "{} of {} pictures, from {} to {} in the first",
            name,
            out.len(),
            fmt_real((lo as f64 * 100.0).round() / 100.0),
            fmt_real((hi as f64 * 100.0).round() / 100.0)
        ));
    }
    Ok(vec![("Rasters".into(), if out.len() == 1 && t.series.is_none() { out.remove(0).into_table() } else { series_table(out, &[]) })])
}

// ---------- Composite over time (TemporalComposite) ----------

const METHODS: &[(&str, &str)] = &[
    ("median", "Median: clouds and shadows that slipped through fall away"),
    ("mean", "Mean"),
    ("max", "Highest"),
    ("min", "Lowest"),
    ("greenest", "Greenest: all bands from the time with the highest NDVI"),
];

const PERIODS: &[(&str, &str)] = &[("all", "All of it"), ("month", "Each month"), ("season", "Each season"), ("year", "Each year")];

const COMPOSITE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "method", label: "Each cell gets", kind: ParamKind::Choice(METHODS), default: "median", hint: "Of the values it has over the time" },
    ParamSpec { key: "period", label: "Period", kind: ParamKind::Choice(PERIODS), default: "all", hint: "A picture for each, named by its first day" },
];

/// The period of a time: 2026-07 for a month, 2026-summer for a season, 2026 for a year.
fn period_of(time: &str, period: &str) -> String {
    let y = time.get(..4).unwrap_or("");
    let m: u32 = time.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(1);
    match period {
        "month" => time.get(..7).unwrap_or(time).to_string(),
        "season" => {
            let (yy, s) = match m {
                12 => ((y.parse::<i32>().unwrap_or(0) + 1).to_string(), "winter"),
                1 | 2 => (y.to_string(), "winter"),
                3..=5 => (y.to_string(), "spring"),
                6..=8 => (y.to_string(), "summer"),
                _ => (y.to_string(), "autumn"),
            };
            format!("{}-{}", yy, s)
        }
        "year" => y.to_string(),
        _ => "all".into(),
    }
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// One picture of a group of pictures on the same grid, cell by cell, and how many values
/// each cell had.
fn composite(group: &[&Grid], method: &str) -> Result<(Grid, Vec<f32>)> {
    let first = group[0];
    let n = first.values.len();
    if group.iter().any(|g| g.values.len() != n || g.band_count() != first.band_count()) {
        return err("The pictures are on different grids or have different bands. Read them with the same Read satellite bands.");
    }
    let bands = first.band_count();
    let mut out: Vec<Vec<f32>> = vec![vec![f32::NAN; n]; bands];
    let mut count = vec![0f32; n];
    let ndvi = if method == "greenest" {
        let (r, nir) = (band_by_role(first, "red"), band_by_role(first, "nir"));
        let at = first.meta.names.iter().position(|x| x.eq_ignore_ascii_case("ndvi"));
        match (at, r, nir) {
            (Some(i), _, _) => Some((Some(i), None)),
            (None, Some(r), Some(nir)) => Some((None, Some((r, nir)))),
            _ => return err("Greenest needs NDVI, or the red and nir bands, in the pictures."),
        }
    } else {
        None
    };
    let mut buf = Vec::with_capacity(group.len());
    for i in 0..n {
        if let Some((band, rn)) = ndvi {
            let mut best: Option<(f32, usize)> = None;
            for (k, g) in group.iter().enumerate() {
                let v = match (band, rn) {
                    (Some(b), _) => g.band(b).expect("band")[i],
                    (None, Some((r, nir))) => {
                        let (r, nr) = (g.band(r).expect("band")[i], g.band(nir).expect("band")[i]);
                        (nr - r) / (nr + r)
                    }
                    _ => f32::NAN,
                };
                if !v.is_nan() && best.is_none_or(|(b, _)| v > b) {
                    best = Some((v, k));
                }
            }
            if let Some((_, k)) = best {
                for (b, o) in out.iter_mut().enumerate() {
                    o[i] = group[k].band(b).expect("band")[i];
                }
                count[i] = group.iter().filter(|g| !g.values[i].is_nan()).count() as f32;
            }
            continue;
        }
        for (b, o) in out.iter_mut().enumerate() {
            buf.clear();
            buf.extend(group.iter().map(|g| g.band(b).expect("band")[i]).filter(|v| !v.is_nan()));
            if b == 0 {
                count[i] = buf.len() as f32;
            }
            if buf.is_empty() {
                continue;
            }
            o[i] = match method {
                "mean" => buf.iter().sum::<f32>() / buf.len() as f32,
                "max" => buf.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                "min" => buf.iter().copied().fold(f32::INFINITY, f32::min),
                _ => median(&mut buf),
            };
        }
    }
    let values = out.remove(0);
    Ok((Grid { values, bands: out, ..first.clone() }, count))
}

fn run_composite(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Rasters");
    let grids = grids_of(t);
    if grids.is_empty() {
        return err("Connect satellite pictures over time to Rasters, such as from Mask clouds.");
    }
    let method = p.get("method").unwrap_or_else(|| "median".into());
    let period = p.get("period").unwrap_or_else(|| "all".into());
    let mut groups: Vec<(String, Vec<&Grid>)> = Vec::new();
    for g in &grids {
        let key = period_of(g.meta.time.as_deref().unwrap_or(""), &period);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(g),
            None => groups.push((key, vec![g])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    let gate = ctx.gate;
    let mut out = Vec::new();
    let mut counts = Vec::new();
    for (key, group) in &groups {
        if !gate.going() {
            return err(crate::model::STOPPED);
        }
        let (mut g, count) = composite(group, &method)?;
        g.meta.time = Some(if period == "all" { group[0].meta.time.clone().unwrap_or_default() } else { key.clone() });
        let mut c = g.clone();
        c.values = count;
        c.bands.clear();
        c.meta.names = vec!["count".into()];
        out.push(g);
        counts.push(c);
    }
    let empty = out[0].values.iter().filter(|v| v.is_nan()).count();
    ctx.notes.info(format!(
        "{} of {} pictures, {} each",
        if out.len() == 1 { "one picture".to_string() } else { format!("{} pictures", out.len()) },
        grids.len(),
        method
    ));
    if empty > 0 {
        ctx.notes.warn(format!("{} cells had no clear value at any time, and are empty.", fmt_count(empty as u64)));
    }
    let (composite, count) =
        if out.len() == 1 { (out.remove(0).into_table(), counts.remove(0).into_table()) } else { (series_table(out, &[]), series_table(counts, &[])) };
    Ok(vec![("Composite".into(), composite), ("Count".into(), count)])
}

pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "SceneReader",
        title: "Read satellite bands",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Reads the bands of the satellite pictures that STAC search found, such as Sentinel-2 or Landsat, onto one grid over the area on Extent: a picture for each time, with the names of its bands, in the coordinate system and cell size you choose. Only the part of each file that covers the area is fetched over the web. The values are scaled to reflectance; bands of classes, such as scl, are kept as they are.",
        inputs: &["Items", "Extent"],
        outputs: &["Rasters"],
        ports: None,
        params: SCENE_PARAMS,
        run: run_scenes,
        summary: |p| p.get("bands").unwrap_or_else(|| "red, nir, scl".into()),
    },
    NodeType {
        name: "CloudMasker",
        title: "Mask clouds",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Empties the cells of clouds, their shadows, cirrus, and if asked snow and water, in every band of satellite pictures: by the scene classes (scl) of Sentinel-2 or the bits of QA_PIXEL of Landsat. The mask can be grown at the edges of clouds, and pictures with too little left are left out. Each picture gets the share that is clear.",
        inputs: &["Rasters"],
        outputs: &["Rasters"],
        ports: None,
        params: MASK_PARAMS,
        run: run_mask,
        summary: |p| p.list("mask").join(", "),
    },
    NodeType {
        name: "SpectralIndex",
        title: "Spectral index",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Works out an index of the bands of each picture, such as NDVI for vegetation, NDWI for water, NDMI for moisture, NDBI for built-up land or NBR for burnt land, or an expression of your own with the names of the bands. The bands are found by their names in Sentinel-2 and Landsat alike. The index becomes the first band, and the others are kept or left out.",
        inputs: &["Rasters"],
        outputs: &["Rasters"],
        ports: None,
        params: INDEX_PARAMS,
        run: run_index,
        summary: |p| p.get("index").unwrap_or_else(|| "ndvi".into()).to_uppercase(),
    },
    NodeType {
        name: "TemporalComposite",
        title: "Composite over time",
        hidden: false,
        group: Group::Transformer,
        category: "Raster",
        desc: "Puts satellite pictures of many times together into one, cell by cell: the median, which leaves out clouds that slipped through, the mean, the highest, the lowest, or all bands from the greenest time. For all the time at once, or a picture for each month, season or year. Count tells how many clear values each cell had.",
        inputs: &["Rasters"],
        outputs: &["Composite", "Count"],
        ports: None,
        params: COMPOSITE_PARAMS,
        run: run_composite,
        summary: |p| format!("{} · {}", p.get("method").unwrap_or_else(|| "median".into()), p.get("period").unwrap_or_else(|| "all".into())),
    },
];

#[cfg(test)]
mod tests {

    #[test]
    fn pictures_are_spread_over_the_time_and_the_clear_ones_kept() {
        // Twelve items in time order, two of them clear in each third of the time.
        let order: Vec<usize> = (0..12).collect();
        let cloud = |i: usize| [50.0, 3.0, 40.0, 60.0, 70.0, 80.0, 90.0, 1.0, 20.0, 30.0, 2.0, 99.0][i];
        assert_eq!(spread_over_time(&order, 3, cloud), [1, 7, 10]);
        assert_eq!(spread_over_time(&order, 12, cloud), order);
        assert_eq!(spread_over_time(&order, 1, cloud), [7]);
        // Without cloud cover, one from each part.
        assert_eq!(spread_over_time(&order, 4, |_| f64::MAX), [0, 3, 6, 9]);
    }

    use super::*;

    fn picture(time: &str, red: f32, nir: f32, scl: f32) -> Grid {
        Grid {
            width: 2,
            height: 1,
            values: vec![red, red],
            bands: vec![vec![nir, nir], vec![scl, 4.0]],
            geotransform: [0.0, 10.0, 0.0, 10.0, 0.0, -10.0],
            crs: Some(3006),
            meta: GridMeta { names: vec!["red".into(), "nir".into(), "scl".into()], time: Some(time.into()) },
        }
    }

    #[test]
    fn clouds_are_masked_indices_made_and_put_together() {
        let series =
            series_table(vec![picture("2026-06-01", 0.05, 0.4, 4.0), picture("2026-06-11", 0.3, 0.35, 9.0), picture("2026-07-01", 0.04, 0.5, 4.0)], &[]);
        let (out, _) = run_for_test("CloudMasker", &[], vec![("Rasters", series.clone())]).unwrap();
        let masked = out[0].1.series.clone().unwrap();
        assert!(masked[1].values[0].is_nan() && !masked[1].values[1].is_nan(), "the cloud of the second picture is masked, its clear cell is kept");
        let (out, _) = run_for_test("SpectralIndex", &[("index", "ndvi")], vec![("Rasters", out[0].1.clone())]).unwrap();
        let ndvi = out[0].1.series.clone().unwrap();
        assert_eq!(ndvi[0].meta.names, ["ndvi", "red", "nir", "scl"]);
        assert!((ndvi[0].values[0] - 0.35 / 0.45).abs() < 1e-6);
        let (out, _) = run_for_test("TemporalComposite", &[("method", "median")], vec![("Rasters", out[0].1.clone())]).unwrap();
        let g = out[0].1.grid.clone().unwrap();
        // Two clear values for the first cell, three for the second.
        assert_eq!(out[1].1.grid.as_ref().unwrap().values, vec![2.0, 3.0]);
        assert!((g.values[0] - (0.35 / 0.45 + 0.46 / 0.54) / 2.0).abs() < 1e-5, "{:?}", g.values);
        let (out, _) = run_for_test("TemporalComposite", &[("period", "month")], vec![("Rasters", series)]).unwrap();
        assert_eq!(out[0].1.series.as_ref().unwrap().len(), 2);
        assert_eq!(out[0].1.features[1].attrs[0], Value::Text("2026-07".into()));
        let mut own = run_for_test(
            "SpectralIndex",
            &[("index", "own"), ("expression", "nir / red"), ("keep", "drop"), ("name", "ratio")],
            vec![("Rasters", picture("t", 0.1, 0.5, 4.0).into_table())],
        )
        .unwrap()
        .0;
        let g = own.remove(0).1.grid.unwrap();
        assert_eq!((g.meta.names.clone(), g.values[0]), (vec!["ratio".to_string()], 5.0));
    }

    #[test]
    fn landsat_bits_and_periods() {
        assert!(masked(((1 << 3) | (1 << 6)) as f32, true, &["cloud".into()]));
        assert!(!masked((1 << 6) as f32, true, &["cloud".into()]));
        assert_eq!(period_of("2025-12-15T10:00:00Z", "season"), "2026-winter");
        assert_eq!(period_of("2026-07-02", "month"), "2026-07");
    }
}
