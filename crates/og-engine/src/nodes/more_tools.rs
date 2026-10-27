//! Small tools added in 0.87: the bounds of each feature, coordinates in attributes
//! reprojected, the first and last point of lines, and random numbers.

use super::geom_inspect::column;
use super::*;
use crate::model::{err, fmt_count, Feature, FieldType, GbError, Result, Value};
use geo::{BoundingRect, Geometry};
use std::sync::Arc;

fn rounder(decimals: Option<i32>) -> impl Fn(f64) -> f64 {
    move |v: f64| match decimals {
        Some(d) => {
            let k = 10f64.powi(d);
            (v * k).round() / k
        }
        None => v,
    }
}

// ---------- Extract bounds (BoundsExtractor) ----------

pub(super) const BOUNDS_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "prefix",
        label: "Names begin with",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as bbox_ for bbox_xmin. Empty: xmin, ymin, xmax, ymax",
    },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "", hint: "Empty: as they are" },
    ParamSpec { key: "heights", label: "Heights too", kind: ParamKind::Choice(YES_NO), default: "false", hint: "zmin and zmax, for features with heights" },
];

fn run_bounds(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let prefix = p.exact("prefix").trim().to_string();
    let round = rounder(number_param(p, "decimals", "The decimals")?.map(|d| d.clamp(0.0, 12.0) as i32));
    let heights = p.flag("heights");
    let mut out = table.clone();
    let names: Vec<String> = ["xmin", "ymin", "xmax", "ymax"].iter().map(|n| format!("{}{}", prefix, n)).collect();
    let idx: Vec<usize> = names.iter().map(|n| column(&mut out, n, FieldType::Real)).collect();
    let zidx = if heights {
        Some((column(&mut out, &format!("{}zmin", prefix), FieldType::Real), column(&mut out, &format!("{}zmax", prefix), FieldType::Real)))
    } else {
        None
    };
    let mut none = 0usize;
    for f in out.features.iter_mut() {
        f.attrs.resize(out.fields.len(), Value::Null);
        match f.geom.as_ref().and_then(|g| g.bounding_rect()) {
            Some(r) => {
                for (k, v) in [r.min().x, r.min().y, r.max().x, r.max().y].into_iter().enumerate() {
                    f.attrs[idx[k]] = Value::Real(round(v));
                }
            }
            None => none += 1,
        }
        if let (Some((a, b)), Some(z)) = (zidx, f.heights()) {
            let (lo, hi) = z.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| (l.min(*v), h.max(*v)));
            if lo.is_finite() {
                f.attrs[a] = Value::Real(round(lo));
                f.attrs[b] = Value::Real(round(hi));
            }
        }
    }
    if none > 0 {
        ctx.notes.info(format!("{} features without geometry got no bounds", fmt_count(none as u64)));
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Reproject coordinate attributes (CoordinateReprojector) ----------

pub(super) const XY_REPROJECT_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "x", label: "x from", kind: ParamKind::Attribute("Input"), default: "", hint: "The attribute with x, or the longitude: east" },
    ParamSpec { key: "y", label: "y from", kind: ParamKind::Attribute("Input"), default: "", hint: "The attribute with y, or the latitude: north" },
    ParamSpec { key: "from", label: "From coordinate system", kind: ParamKind::Crs, default: "EPSG:4326", hint: "That of the numbers in the attributes" },
    ParamSpec { key: "to", label: "To coordinate system", kind: ParamKind::Crs, default: "EPSG:3006", hint: "" },
    ParamSpec {
        key: "x_out",
        label: "New x in",
        kind: ParamKind::Text,
        default: "",
        hint: "The attribute that gets the new x. Empty: the one it came from, written over",
    },
    ParamSpec { key: "y_out", label: "New y in", kind: ParamKind::Text, default: "", hint: "As for x" },
    ParamSpec {
        key: "decimals",
        label: "Decimals",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as 3 for millimetres in metres, or 7 in degrees. Empty: as they are",
    },
];

fn run_xy_reproject(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let xa = p.need("x", "Choose the attribute with x.")?;
    let ya = p.need("y", "Choose the attribute with y.")?;
    let (xi, yi) = (table.need_field(&xa)?, table.need_field(&ya)?);
    let from = p.crs("from")?.ok_or_else(|| GbError("Choose the coordinate system the numbers are in.".into()))?;
    let to = p.crs("to")?.ok_or_else(|| GbError("Choose the coordinate system to reproject to.".into()))?;
    let round = rounder(number_param(p, "decimals", "The decimals")?.map(|d| d.clamp(0.0, 12.0) as i32));
    let mut out = table.clone();
    let xo = match p.get("x_out").filter(|n| !n.trim().is_empty()) {
        Some(n) => column(&mut out, n.trim(), FieldType::Real),
        None => {
            out.fields[xi].ty = FieldType::Real;
            xi
        }
    };
    let yo = match p.get("y_out").filter(|n| !n.trim().is_empty()) {
        Some(n) => column(&mut out, n.trim(), FieldType::Real),
        None => {
            out.fields[yi].ty = FieldType::Real;
            yi
        }
    };
    // The rows with both numbers, reprojected together.
    let num = |v: &Value| v.as_f64().or_else(|| crate::model::parse_number(&v.to_text()));
    let rows: Vec<usize> =
        (0..table.features.len()).filter(|&i| num(&table.features[i].attrs[xi]).is_some() && num(&table.features[i].attrs[yi]).is_some()).collect();
    let mut xs: Vec<f64> = rows.iter().map(|&i| num(&table.features[i].attrs[xi]).unwrap_or(0.0)).collect();
    let mut ys: Vec<f64> = rows.iter().map(|&i| num(&table.features[i].attrs[yi]).unwrap_or(0.0)).collect();
    if from != to && !rows.is_empty() {
        ctx.backend("Reprojecting coordinates")?.transform(from, to, &mut xs, &mut ys)?;
    }
    for f in out.features.iter_mut() {
        f.attrs.resize(out.fields.len(), Value::Null);
    }
    for (k, &i) in rows.iter().enumerate() {
        out.features[i].attrs[xo] = Value::Real(round(xs[k]));
        out.features[i].attrs[yo] = Value::Real(round(ys[k]));
    }
    let left = table.features.len() - rows.len();
    if left > 0 {
        ctx.notes.warn(format!("{} features have no x or y that is a number and were not reprojected.", fmt_count(left as u64)));
    }
    ctx.notes.info(format!("{} from {} to {}", fmt_count(rows.len() as u64), crate::model::crs_label(Some(from)), crate::model::crs_label(Some(to))));
    Ok(vec![("Output".into(), out)])
}

// ---------- First and last point (LineEndsExtractor) ----------

/// The first and last coordinate of a line, or of the first and last part of a multi line,
/// with their heights.
/// A corner and its height, if it has one.
type End = (geo::Coord<f64>, Option<f64>);

fn ends(f: &Feature) -> Option<(End, End)> {
    let g = f.geom.as_ref()?;
    let coords = crate::geom::coords(g);
    if coords.is_empty() || !matches!(g, Geometry::LineString(_) | Geometry::MultiLineString(_) | Geometry::Line(_)) {
        return None;
    }
    let z = f.heights();
    let n = coords.len();
    Some(((coords[0], z.map(|z| z[0])), (coords[n - 1], z.map(|z| z[n - 1]))))
}

fn run_line_ends(_p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let (mut first, mut last, mut other) = (table.like(), table.like(), table.like());
    for f in &table.features {
        match ends(f) {
            Some((a, b)) => {
                let point = |(c, z): (geo::Coord<f64>, Option<f64>)| Feature {
                    attrs: f.attrs.clone(),
                    geom: Some(Geometry::Point(c.into())),
                    z: z.map(|z| Arc::from(vec![z])),
                };
                first.features.push(point(a));
                last.features.push(point(b));
            }
            None => other.features.push(f.clone()),
        }
    }
    if !other.features.is_empty() {
        ctx.notes.info(format!("{} features are not lines and went to Other", fmt_count(other.features.len() as u64)));
    }
    Ok(vec![("First".into(), first), ("Last".into(), last), ("Other".into(), other)])
}

// ---------- Random number (RandomNumber) ----------

pub(super) const RANDOM_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "random", hint: "" },
    ParamSpec { key: "min", label: "From", kind: ParamKind::Text, default: "0", hint: "" },
    ParamSpec { key: "max", label: "To", kind: ParamKind::Text, default: "1", hint: "" },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "2", hint: "0 gives whole numbers, from and to both included" },
    ParamSpec {
        key: "seed",
        label: "Seed",
        kind: ParamKind::Text,
        default: "",
        hint: "A whole number gives the same numbers at every run, to test with. Empty: new ones each time",
    },
];

fn run_random(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let name = p.get("name").unwrap_or_else(|| "random".into());
    let lo = number_param(p, "min", "From")?.unwrap_or(0.0);
    let hi = number_param(p, "max", "To")?.unwrap_or(1.0);
    if hi < lo {
        return err("To must be at least From.");
    }
    let decimals = number_param(p, "decimals", "The decimals")?.unwrap_or(2.0).clamp(0.0, 12.0) as i32;
    let seed = match number_param(p, "seed", "The seed")? {
        Some(s) => s.abs() as u64,
        None => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1),
    };
    let mut rng = super::props::Rng::new(seed);
    let mut out = table.clone();
    let at = column(&mut out, &name, if decimals == 0 { FieldType::Int } else { FieldType::Real });
    let k = 10f64.powi(decimals);
    for f in out.features.iter_mut() {
        f.attrs.resize(out.fields.len(), Value::Null);
        f.attrs[at] = if decimals == 0 {
            // Each whole number from lo to hi as likely as the others.
            let (a, b) = (lo.ceil() as i64, hi.floor() as i64);
            Value::Int(a + ((rng.next() * (b - a + 1) as f64) as i64).min(b - a))
        } else {
            Value::Real(((lo + (hi - lo) * rng.next()) * k).round() / k)
        };
    }
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "BoundsExtractor",
        title: "Extract bounds",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Puts the smallest and largest x and y of each feature, its bounding box, in attributes of their own: xmin, ymin, xmax and ymax, and zmin and zmax for features with heights. For the bounds of all the features together, use Bounding box of all.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: BOUNDS_PARAMS,
        run: run_bounds,
        summary: |p| format!("{}xmin … {}ymax", p.exact("prefix").trim(), p.exact("prefix").trim()),
    },
    NodeType {
        name: "CoordinateReprojector",
        title: "Reproject coordinate attributes",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Reprojects coordinates that are numbers in two attributes, such as lat and lon in a table, from one coordinate system to another, without making geometry of them. The new numbers go into the same attributes or new ones. To reproject geometry, use Reproject.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: XY_REPROJECT_PARAMS,
        run: run_xy_reproject,
        summary: |p| format!("{}, {} · {} → {}", p.get("x").unwrap_or_default(), p.get("y").unwrap_or_default(), p.get("from").unwrap_or_default(), p.get("to").unwrap_or_default()),
    },
    NodeType {
        name: "LineEndsExtractor",
        title: "First and last point",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "The first and the last point of each line, with its attributes and heights: where a route, a pipe or a street begins and ends. A multi line gives the start of its first part and the end of its last. Features that are not lines go to Other.",
        inputs: &["Input"],
        outputs: &["First", "Last", "Other"],
        ports: None,
        params: &[],
        run: run_line_ends,
        summary: |_| String::new(),
    },
    NodeType {
        name: "RandomNumber",
        title: "Random number",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Gives each feature a random number between two values, with the decimals you want, or a whole number. With a seed the numbers are the same at every run.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: RANDOM_PARAMS,
        run: run_random,
        summary: |p| format!("{} from {} to {}", p.get("name").unwrap_or_else(|| "random".into()), p.get("min").unwrap_or_default(), p.get("max").unwrap_or_default()),
    },
];
