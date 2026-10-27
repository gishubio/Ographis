//! The JSON tools: flatten JSON into attributes, a feature per element of a list, values by
//! their path, JSON-stat as a table, and JSON built from attributes.

use super::*;
use crate::json::{self, Lists, Step};
use crate::model::{err, fmt_count, infer_type, Feature, Field, GbError, Result, Table, Value};
use geo::Geometry;
use serde_json::{Map, Value as Json};

pub(super) const LISTS: &[(&str, &str)] = &[("text", "As JSON text in one attribute"), ("columns", "An attribute per element: name.0, name.1, …")];
pub(super) const GEOMETRY: &[(&str, &str)] = &[("none", "Left out"), ("geojson", "As GeoJSON, in geometry")];

/// The JSON in an attribute of a feature. Empty gives None; text that is not JSON an error.
pub(super) fn json_of(v: &Value) -> std::result::Result<Option<Json>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Text(t) if t.trim().is_empty() => Ok(None),
        Value::Text(t) => json::parse(t).map(Some).map_err(|e| e.0),
        other => Ok(Some(json::from_value(other, false))),
    }
}

/// Puts new attributes on a feature. A name that the stream has already gets the name of the
/// JSON attribute in front: adress.ort becomes svar.adress.ort.
/// The feature a row of a list starts from: without the JSON it came from, when that is not
/// kept. An answer of megabytes would otherwise be copied once for every element of its list,
/// which for every stop of a journey planner is more memory than a computer has.
fn bare(f: &Feature, drop: Option<usize>) -> Feature {
    match drop {
        Some(at) => Feature {
            z: f.z.clone(),
            geom: f.geom.clone(),
            attrs: f.attrs.iter().enumerate().map(|(i, v)| if i == at { Value::Null } else { v.clone() }).collect(),
        },
        None => f.clone(),
    }
}

pub(super) fn add_columns(base: &Table, rows: Vec<(Feature, Vec<(String, Value)>)>, source: &str, drop: Option<usize>) -> Table {
    let mut names: Vec<String> = Vec::new();
    let mut index = std::collections::HashMap::new();
    let taken = |n: &str| base.fields.iter().enumerate().any(|(i, f)| Some(i) != drop && f.name == n);
    let rows: Vec<(Feature, Vec<(String, Value)>)> = rows
        .into_iter()
        .map(|(f, cols)| {
            let cols = cols.into_iter().map(|(k, v)| (if taken(&k) { format!("{}.{}", source, k) } else { k }, v)).collect();
            (f, cols)
        })
        .collect();
    for (_, cols) in &rows {
        for (k, _) in cols {
            if !index.contains_key(k) {
                index.insert(k.clone(), names.len());
                names.push(k.clone());
            }
        }
    }
    let kept: Vec<usize> = (0..base.fields.len()).filter(|i| Some(*i) != drop).collect();
    let mut features = Vec::with_capacity(rows.len());
    for (f, cols) in rows {
        let mut attrs: Vec<Value> = kept.iter().map(|i| f.attrs[*i].clone()).collect();
        let mut extra = vec![Value::Null; names.len()];
        for (k, v) in cols {
            extra[index[&k]] = v;
        }
        attrs.extend(extra);
        features.push(Feature { z: f.z, attrs, geom: f.geom });
    }
    let mut fields: Vec<Field> = kept.iter().map(|i| base.fields[*i].clone()).collect();
    for (k, name) in names.iter().enumerate() {
        let at = kept.len() + k;
        fields.push(Field::new(name.clone(), infer_type(features.iter().map(|f| &f.attrs[at]))));
    }
    Table { fields, features, ..base.like() }
}

// ---------- Flatten JSON ----------

pub(super) const FLATTEN_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attribute",
        label: "JSON in",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The attribute with JSON text, such as the answer from HTTP request or an object that an input kept as JSON",
    },
    ParamSpec { key: "lists", label: "Lists", kind: ParamKind::Choice(LISTS), default: "text", hint: "For a feature per element, use Explode JSON list" },
    ParamSpec {
        key: "prefix",
        label: "Prefix",
        kind: ParamKind::Text,
        default: "",
        hint: "Put before the new names, such as svar. Empty uses the names in the JSON, and a name the stream has already gets the name of the attribute in front",
    },
    ParamSpec { key: "keep", label: "Keep the JSON attribute", kind: ParamKind::Choice(YES_NO), default: "false", hint: "" },
];

pub(super) fn run_flatten(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute that holds the JSON.")?;
    let at = table.need_field(&attr)?;
    let lists = if p.get("lists").as_deref() == Some("columns") { Lists::Columns } else { Lists::Text };
    let prefix = p.get("prefix").unwrap_or_default();
    let prefix = prefix.trim_end_matches('.');
    let drop = if p.flag("keep") { None } else { Some(at) };
    let mut rows = Vec::new();
    let mut failed = table.like();
    failed.fields.push(Field::new("json_error", crate::model::FieldType::Text));
    for f in &table.features {
        match json_of(&f.attrs[at]) {
            Ok(v) => {
                let mut cols = Vec::new();
                if let Some(v) = v {
                    json::flatten(&v, prefix, lists, &mut cols);
                }
                // A plain value gets the name of the attribute it came from.
                if cols.len() == 1 && cols[0].0 == "value" && prefix.is_empty() {
                    cols[0].0 = format!("{}.value", attr);
                }
                rows.push((f.clone(), cols));
            }
            Err(e) => {
                let mut g = f.clone();
                g.attrs.push(Value::Text(e));
                failed.features.push(g);
            }
        }
    }
    let out = add_columns(table, rows, &attr, drop);
    let added = out.fields.len() + usize::from(drop.is_some()) - table.fields.len();
    ctx.notes.info(format!("{} attributes from {}", fmt_count(added as u64), attr));
    Ok(vec![("Output".into(), out), ("Failed".into(), failed)])
}

// ---------- Explode JSON list ----------

pub(super) const EXPLODE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "JSON in", kind: ParamKind::Attribute("Input"), default: "", hint: "The attribute with the list, or with JSON that holds it" },
    ParamSpec {
        key: "path",
        label: "List at",
        kind: ParamKind::Text,
        default: "",
        hint: "Where the list is in the JSON, such as mottagningar or data.items. Empty: the attribute is the list. Several lists of the same length, separated by commas, are taken in step: hourly.time, hourly.temperature_2m",
    },
    ParamSpec {
        key: "prefix",
        label: "Prefix",
        kind: ParamKind::Text,
        default: "",
        hint: "Put before the names of the values of each element. Empty uses the names in the JSON, and a name the stream has already gets the name of the list in front",
    },
    ParamSpec {
        key: "index_attr",
        label: "Number the elements",
        kind: ParamKind::OptionalName("element"),
        default: "",
        hint: "The place of each element in its list, from 0, in an attribute",
    },
    ParamSpec { key: "keep", label: "Keep the JSON attribute", kind: ParamKind::Choice(YES_NO), default: "false", hint: "" },
];

pub(super) fn run_explode(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute that holds the JSON with the list.")?;
    let at = table.need_field(&attr)?;
    let path_text = p.get("path").unwrap_or_default();
    // Several lists in step, as the time series of SMHI and Open-Meteo: a row for each place.
    if path_text.contains(',') {
        return explode_in_step(p, table, at, &path_text, ctx);
    }
    let path = json::parse_path(&path_text)?;
    // The name the values of a list of plain values get: the last name in the path, or the attribute.
    let list_name = path.iter().rev().find_map(|s| if let Step::Key(k) = s { Some(k.clone()) } else { None }).unwrap_or_else(|| attr.clone());
    let prefix = p.get("prefix").unwrap_or_default();
    let prefix = prefix.trim_end_matches('.').to_string();
    let index_attr = p.get("index_attr").filter(|s| !s.is_empty());
    let drop = if p.flag("keep") { None } else { Some(at) };
    let mut rows = Vec::new();
    let mut other = table.like();
    for f in &table.features {
        let list = match json_of(&f.attrs[at]) {
            Ok(Some(v)) => json::get(&v, &path).cloned(),
            _ => None,
        };
        let Some(Json::Array(items)) = list.filter(|l| matches!(l, Json::Array(a) if !a.is_empty())) else {
            other.features.push(f.clone());
            continue;
        };
        let from = bare(f, drop);
        for (i, item) in items.iter().enumerate() {
            let mut cols = Vec::new();
            if let Some(name) = &index_attr {
                cols.push((name.clone(), Value::Int(i as i64)));
            }
            match item {
                Json::Object(_) => json::flatten(item, &prefix, Lists::Text, &mut cols),
                plain => cols.push((if prefix.is_empty() { list_name.clone() } else { prefix.clone() }, json::to_value(plain))),
            }
            rows.push((from.clone(), cols));
        }
    }
    let n = rows.len();
    let out = add_columns(table, rows, &list_name, drop);
    ctx.notes.info(format!("{} elements", fmt_count(n as u64)));
    if !other.is_empty() {
        ctx.notes.info(format!("{} without a list to Other", fmt_count(other.len() as u64)));
    }
    Ok(vec![("Exploded".into(), out), ("Other".into(), other)])
}

/// Explodes several lists side by side: element 0 of each in the first row, 1 in the next.
/// A shorter list gives empty values at the end.
pub(super) fn explode_in_step(p: &P, table: &Table, at: usize, paths_text: &str, ctx: &mut NodeCtx) -> Result<Outputs> {
    let paths: Vec<(String, Vec<Step>)> =
        paths_text.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()).map(|t| Ok((name_of_path(t), json::parse_path(t)?))).collect::<Result<_>>()?;
    let prefix = p.get("prefix").unwrap_or_default();
    let prefix = prefix.trim_end_matches('.').to_string();
    let index_attr = p.get("index_attr").filter(|s| !s.is_empty());
    let drop = if p.flag("keep") { None } else { Some(at) };
    let attr = table.fields[at].name.clone();
    let (mut rows, mut other) = (Vec::new(), table.like());
    for f in &table.features {
        let v = match json_of(&f.attrs[at]) {
            Ok(Some(v)) => v,
            _ => {
                other.features.push(f.clone());
                continue;
            }
        };
        let lists: Vec<Option<&Vec<Json>>> = paths.iter().map(|(_, path)| json::get(&v, path).and_then(|l| l.as_array())).collect();
        let n = lists.iter().map(|l| l.map_or(0, |l| l.len())).max().unwrap_or(0);
        if n == 0 {
            other.features.push(f.clone());
            continue;
        }
        let from = bare(f, drop);
        for i in 0..n {
            let mut cols = Vec::new();
            if let Some(name) = &index_attr {
                cols.push((name.clone(), Value::Int(i as i64)));
            }
            for ((name, _), list) in paths.iter().zip(&lists) {
                let name = if prefix.is_empty() { name.clone() } else { format!("{}.{}", prefix, name) };
                match list.and_then(|l| l.get(i)) {
                    Some(item @ Json::Object(_)) => json::flatten(item, &name, Lists::Text, &mut cols),
                    Some(plain) => cols.push((name, json::to_value(plain))),
                    None => cols.push((name, Value::Null)),
                }
            }
            rows.push((from.clone(), cols));
        }
    }
    let n = rows.len();
    let out = add_columns(table, rows, &attr, drop);
    ctx.notes.info(format!("{} rows from {} lists side by side", fmt_count(n as u64), paths.len()));
    Ok(vec![("Exploded".into(), out), ("Other".into(), other)])
}

// ---------- JSON-stat to table ----------

pub(super) const STAT_MISSING: &[(&str, &str)] = &[("keep", "Kept, with an empty value"), ("skip", "Left out")];

pub(super) const JSONSTAT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attribute",
        label: "JSON-stat in",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The attribute with the answer from SCB, Eurostat or another service that gives JSON-stat, such as _response from HTTP request",
    },
    ParamSpec {
        key: "labels",
        label: "Names of the categories",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "Such as Borås next to 1490, in Region_label",
    },
    ParamSpec {
        key: "columns_from",
        label: "Columns from",
        kind: ParamKind::Text,
        default: "",
        hint: "A dimension whose categories become columns, such as Tid for a column per year or ContentsCode for one per measure. Empty gives a row per value",
    },
    ParamSpec { key: "missing", label: "Values that are missing", kind: ParamKind::Choice(STAT_MISSING), default: "keep", hint: "" },
];

pub(super) fn run_jsonstat(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute that holds the JSON-stat.")?;
    let at = table.need_field(&attr)?;
    let layout = crate::jsonstat::Layout {
        columns_from: p.get("columns_from").filter(|c| !c.trim().is_empty()),
        skip_missing: p.get("missing").as_deref() == Some("skip"),
        labels: p.get("labels").as_deref() != Some("false"),
    };
    let mut rows = Vec::new();
    let mut failed = table.like();
    failed.fields.push(Field::new("json_error", crate::model::FieldType::Text));
    let mut fail = |f: &Feature, why: String| {
        let mut g = f.clone();
        g.attrs.push(Value::Text(why));
        failed.features.push(g);
    };
    for f in &table.features {
        let v = match json_of(&f.attrs[at]) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => {
                fail(f, e);
                continue;
            }
        };
        if !crate::jsonstat::is_jsonstat(&v) {
            fail(f, "This is JSON, but not JSON-stat. Ask the service for json-stat2 (SCB) or the statistics API (Eurostat).".into());
            continue;
        }
        match crate::jsonstat::to_table(&v, &layout) {
            Ok(t) => {
                let names = t.field_names();
                let from = bare(f, Some(at));
                for g in t.features {
                    rows.push((from.clone(), names.iter().cloned().zip(g.attrs).collect::<Vec<_>>()));
                }
            }
            Err(e) => fail(f, e.0),
        }
    }
    let n = rows.len();
    let out = add_columns(table, rows, &attr, Some(at));
    // Codes, such as 0114 or 2025, stay text.
    let out = keep_codes_as_text(out, table.fields.len().saturating_sub(1));
    ctx.notes.info(format!("{} rows of statistics", fmt_count(n as u64)));
    Ok(vec![("Output".into(), out), ("Failed".into(), failed)])
}

/// The new columns that hold only codes in text are kept as text, though they look like numbers.
pub(super) fn keep_codes_as_text(mut t: Table, from: usize) -> Table {
    for i in from..t.fields.len() {
        let all_text = t.features.iter().all(|f| matches!(f.attrs[i], Value::Text(_) | Value::Null));
        if all_text {
            t.fields[i].ty = crate::model::FieldType::Text;
        }
    }
    t
}

// ---------- Extract JSON values ----------

pub(super) const EXTRACT_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "JSON in", kind: ParamKind::Attribute("Input"), default: "", hint: "The attribute with JSON text" },
    ParamSpec {
        key: "paths",
        label: "Values to take",
        kind: ParamKind::JsonPaths("Input", "attribute"),
        default: "",
        hint: "A path and the attribute it goes to, such as adress.ort or kontakter[0].telefon. An empty name takes the last part of the path. An object or a list is taken as JSON text",
    },
];

/// The rows of path and name, as they are stored: a JSON list of [path, name].
pub fn json_paths_of(params: &Params, key: &str) -> Vec<(String, String)> {
    let Some(Json::Array(rows)) = params.get(key) else { return Vec::new() };
    rows.iter()
        .filter_map(|r| {
            let a = r.as_array()?;
            let path = a.first()?.as_str()?.trim().to_string();
            let name = a.get(1).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            Some((path, name)).filter(|(p, _)| !p.is_empty())
        })
        .collect()
}

/// The name a path gives when no name is set: its last name, with [n] left off.
pub fn name_of_path(path: &str) -> String {
    match json::parse_path(path) {
        Ok(steps) => steps.iter().rev().find_map(|s| if let Step::Key(k) = s { Some(k.clone()) } else { None }).unwrap_or_else(|| "value".into()),
        Err(_) => path.to_string(),
    }
}

pub(super) fn run_extract(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute that holds the JSON.")?;
    let at = table.need_field(&attr)?;
    let rows = json_paths_of(p.values, "paths");
    if rows.is_empty() {
        return err("Add a path for each value to take, such as adress.ort.");
    }
    let mut wanted: Vec<(Vec<Step>, String)> = Vec::new();
    for (path, name) in &rows {
        let steps = json::parse_path(path)?;
        let name = if name.is_empty() { name_of_path(path) } else { name.clone() };
        if wanted.iter().any(|w| w.1 == name) {
            return err(format!("Two values go to {}. Give one of them another name.", name));
        }
        wanted.push((steps, name));
    }
    let mut out = table.clone();
    let mut failed = table.like();
    failed.fields.push(Field::new("json_error", crate::model::FieldType::Text));
    let mut values: Vec<Vec<Value>> = vec![Vec::new(); wanted.len()];
    let mut kept = Vec::new();
    let mut missing = 0usize;
    for f in &table.features {
        match json_of(&f.attrs[at]) {
            Ok(v) => {
                for (k, (steps, _)) in wanted.iter().enumerate() {
                    let got = v.as_ref().and_then(|v| json::get(v, steps)).map(json::to_value).unwrap_or(Value::Null);
                    if got.is_null() {
                        missing += 1;
                    }
                    values[k].push(got);
                }
                kept.push(f.clone());
            }
            Err(e) => {
                let mut g = f.clone();
                g.attrs.push(Value::Text(e));
                failed.features.push(g);
            }
        }
    }
    out.features = kept;
    for ((_, name), vals) in wanted.iter().zip(values) {
        let ty = infer_type(vals.iter());
        match out.field_index(name) {
            Some(i) => {
                out.fields[i].ty = ty;
                for (f, v) in out.features.iter_mut().zip(vals) {
                    f.attrs[i] = v;
                }
            }
            None => {
                out.fields.push(Field::new(name.clone(), ty));
                for (f, v) in out.features.iter_mut().zip(vals) {
                    f.attrs.push(v);
                }
            }
        }
    }
    if missing > 0 {
        ctx.notes.info(format!("{} values were not there and are empty", fmt_count(missing as u64)));
    }
    Ok(vec![("Output".into(), out), ("Failed".into(), failed)])
}

// ---------- Build JSON ----------

pub(super) const BUILD_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attributes",
        label: "Attributes",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "The attributes to put in. Empty puts in all of them",
    },
    ParamSpec { key: "result", label: "Write to", kind: ParamKind::Text, default: "json", hint: "The attribute that gets the JSON text" },
    ParamSpec {
        key: "nest",
        label: "Names with a dot as objects",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "adress.gata and adress.ort become \"adress\": {\"gata\": …, \"ort\": …}, as Flatten JSON took them apart",
    },
    ParamSpec {
        key: "parse",
        label: "JSON text as JSON",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "An attribute that holds a JSON object or list is put in as such, not as text",
    },
    ParamSpec { key: "geometry", label: "Geometry", kind: ParamKind::Choice(GEOMETRY), default: "none", hint: "In the coordinates of the stream" },
    ParamSpec { key: "pretty", label: "Over several lines", kind: ParamKind::Choice(YES_NO), default: "false", hint: "Easier to read, longer" },
];

/// Puts a value in an object at a dotted name, making the objects on the way.
pub(super) fn insert_nested(obj: &mut Map<String, Json>, name: &str, v: Json) {
    match name.split_once('.') {
        Some((head, rest)) if !head.is_empty() && !rest.is_empty() => {
            let inner = obj.entry(head.to_string()).or_insert_with(|| Json::Object(Map::new()));
            if !inner.is_object() {
                // A plain value is there already under that name: the dotted name is kept whole.
                obj.insert(name.to_string(), v);
                return;
            }
            if let Json::Object(o) = inner {
                insert_nested(o, rest, v);
            }
        }
        _ => {
            obj.insert(name.to_string(), v);
        }
    }
}

/// A geometry as GeoJSON.
pub fn geojson(g: &Geometry<f64>) -> Json {
    use serde_json::json;
    let pt = |c: geo::Coord<f64>| json!([c.x, c.y]);
    let line = |l: &geo::LineString<f64>| Json::Array(l.0.iter().map(|c| pt(*c)).collect());
    let poly = |p: &geo::Polygon<f64>| Json::Array(std::iter::once(p.exterior()).chain(p.interiors()).map(line).collect());
    match g {
        Geometry::Point(p) => json!({"type": "Point", "coordinates": pt(p.0)}),
        Geometry::MultiPoint(m) => json!({"type": "MultiPoint", "coordinates": m.0.iter().map(|p| pt(p.0)).collect::<Vec<_>>()}),
        Geometry::LineString(l) => json!({"type": "LineString", "coordinates": line(l)}),
        Geometry::Line(l) => json!({"type": "LineString", "coordinates": [pt(l.start), pt(l.end)]}),
        Geometry::MultiLineString(m) => json!({"type": "MultiLineString", "coordinates": m.0.iter().map(line).collect::<Vec<_>>()}),
        Geometry::Polygon(p) => json!({"type": "Polygon", "coordinates": poly(p)}),
        Geometry::MultiPolygon(m) => json!({"type": "MultiPolygon", "coordinates": m.0.iter().map(poly).collect::<Vec<_>>()}),
        Geometry::Rect(r) => geojson(&Geometry::Polygon(r.to_polygon())),
        Geometry::Triangle(t) => geojson(&Geometry::Polygon(t.to_polygon())),
        Geometry::GeometryCollection(c) => json!({"type": "GeometryCollection", "geometries": c.0.iter().map(geojson).collect::<Vec<_>>()}),
    }
}

pub(super) fn run_build(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let chosen = p.list("attributes");
    let idx: Vec<usize> =
        if chosen.is_empty() { (0..table.fields.len()).collect() } else { chosen.iter().map(|n| table.need_field(n)).collect::<Result<_>>()? };
    let result = p.get("result").filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "json".into());
    let (nest, parse, pretty) = (p.get("nest").as_deref() != Some("false"), p.get("parse").as_deref() != Some("false"), p.flag("pretty"));
    let with_geom = p.get("geometry").as_deref() == Some("geojson");
    let mut texts = Vec::with_capacity(table.len());
    for f in &table.features {
        let mut obj = Map::new();
        for &i in &idx {
            let v = json::from_value(&f.attrs[i], parse);
            let name = &table.fields[i].name;
            if nest {
                insert_nested(&mut obj, name, v);
            } else {
                obj.insert(name.clone(), v);
            }
        }
        if with_geom {
            obj.insert("geometry".into(), f.geom.as_ref().map_or(Json::Null, geojson));
        }
        let v = Json::Object(obj);
        let text = if pretty { serde_json::to_string_pretty(&v) } else { serde_json::to_string(&v) }.map_err(|e| GbError(e.to_string()))?;
        texts.push(Value::Text(text));
    }
    let mut out = table.clone();
    match out.field_index(&result) {
        Some(i) => {
            out.fields[i].ty = crate::model::FieldType::Text;
            for (f, t) in out.features.iter_mut().zip(texts) {
                f.attrs[i] = t;
            }
        }
        None => {
            out.fields.push(Field::new(result.clone(), crate::model::FieldType::Text));
            for (f, t) in out.features.iter_mut().zip(texts) {
                f.attrs.push(t);
            }
        }
    }
    ctx.notes.info(format!("{} attributes in {}", idx.len(), result));
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "JsonFlattener",
        title: "Flatten JSON",
        hidden: false,
        group: Group::Transformer,
        category: "JSON",
        desc: "Makes the JSON in an attribute into attributes of their own, so that the stream is a plain table: {\"adress\": {\"ort\": \"Borås\"}} gives adress.ort. Lists are kept as JSON text, or get an attribute per element. Features whose attribute is not JSON go to Failed.",
        inputs: &["Input"],
        outputs: &["Output", "Failed"],
        ports: None,
        params: FLATTEN_PARAMS,
        run: run_flatten,
        summary: |p| p.get("attribute").unwrap_or_else(|| "no attribute chosen".into()),
    },
    NodeType {
        name: "JsonExploder",
        title: "Explode JSON list",
        hidden: false,
        group: Group::Transformer,
        category: "JSON",
        desc: "Makes a feature of each element of a list in JSON, with the attributes of the feature it came from and the values of the element, such as one feature per clinic of a hospital. Features without a list, or with an empty one, go to Other.",
        inputs: &["Input"],
        outputs: &["Exploded", "Other"],
        ports: None,
        params: EXPLODE_PARAMS,
        run: run_explode,
        summary: |p| match (p.get("attribute"), p.get("path")) {
            (Some(a), Some(path)) => format!("{} · {}", a, path),
            (Some(a), None) => a,
            _ => "no attribute chosen".into(),
        },
    },
    NodeType {
        name: "JsonExtractor",
        title: "Extract JSON values",
        hidden: false,
        group: Group::Transformer,
        category: "JSON",
        desc: "Takes chosen values out of the JSON in an attribute, each by its path, such as adress.ort or kontakter[0].telefon, into attributes. A value that is not there is left empty. Features whose attribute is not JSON go to Failed.",
        inputs: &["Input"],
        outputs: &["Output", "Failed"],
        ports: None,
        params: EXTRACT_PARAMS,
        run: run_extract,
        summary: |p| {
            let rows = json_paths_of(p.values, "paths");
            match rows.as_slice() {
                [] => "no paths".into(),
                [one] => one.0.clone(),
                many => format!("{} values", many.len()),
            }
        },
    },
    NodeType {
        name: "JsonStatReader",
        title: "JSON-stat to table",
        hidden: false,
        group: Group::Transformer,
        category: "JSON",
        desc: "Makes a table of statistics in JSON-stat, as SCB (PxWeb), Eurostat, Folkhälsomyndigheten and other statistics agencies give them: a row for each value with the code and the name of each category, such as region, sex and year, or with the categories of one dimension as columns. Answers that are not JSON-stat go to Failed.",
        inputs: &["Input"],
        outputs: &["Output", "Failed"],
        ports: None,
        params: JSONSTAT_PARAMS,
        run: run_jsonstat,
        summary: |p| match p.get("columns_from").filter(|c| !c.trim().is_empty()) {
            Some(c) => format!("columns from {}", c),
            None => "a row per value".into(),
        },
    },
    NodeType {
        name: "JsonBuilder",
        title: "Build JSON",
        hidden: false,
        group: Group::Transformer,
        category: "JSON",
        desc: "Writes attributes as a JSON object in an attribute of its own: all of them or the ones chosen, names with a dot as nested objects, and the geometry as GeoJSON if you want it. For sending on to a web service, or for writing as text.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: BUILD_PARAMS,
        run: run_build,
        summary: |p| {
            let what = match p.list("attributes").len() {
                0 => "all attributes".to_string(),
                n => format!("{} attributes", n),
            };
            format!("{} → {}", what, p.get("result").unwrap_or_else(|| "json".into()))
        },
    },
];
