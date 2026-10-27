//! JSON that is not GeoJSON: a list of records read as a table, and the paths, flattening
//! and building that the JSON tools use.

use crate::model::{err, infer_type, Feature, Field, GbError, Result, Table, Value};
use serde_json::Value as Json;
use std::path::Path;

/// The byte order mark some servers put first, also in front of the first key of a record.
const BOM: char = '\u{feff}';

fn clean_key(k: &str) -> String {
    k.trim_start_matches(BOM).to_string()
}

/// Parses JSON text. A text of one JSON value per line (newline delimited JSON) becomes a list.
pub fn parse(text: &str) -> Result<Json> {
    let text = text.trim_start_matches(BOM);
    match serde_json::from_str::<Json>(text) {
        Ok(v) => Ok(v),
        Err(e) => {
            let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.len() > 1 {
                if let Ok(list) = lines.iter().map(|l| serde_json::from_str::<Json>(l)).collect::<std::result::Result<Vec<_>, _>>() {
                    return Ok(Json::Array(list));
                }
            }
            err(format!("This is not JSON that can be read: {}", e))
        }
    }
}

pub fn read_file(path: &Path) -> Result<Json> {
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let text = String::from_utf8(bytes).map_err(|_| GbError(format!("{} is not text in UTF-8, as JSON must be.", path.display())))?;
    parse(&text)
}

/// Whether a file with the extension .json holds GeoJSON, which GDAL reads with its geometry.
/// Only the start of the file is looked at.
pub fn looks_like_geojson(path: &Path) -> bool {
    use std::io::Read;
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..n]);
    let squeezed: String = head.chars().filter(|c| !c.is_whitespace()).collect();
    ["\"type\":\"FeatureCollection\"", "\"type\":\"Feature\"", "\"type\":\"Topology\"", "\"features\":["].iter().any(|p| squeezed.contains(p))
}

/// Whether a file starts with { or [, after any byte order mark and white space.
pub fn starts_as_json(path: &Path) -> bool {
    use std::io::Read;
    let mut head = vec![0u8; 256];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..n]);
    head.trim_start_matches(BOM).trim_start().starts_with(['{', '['])
}

// ---------- Paths ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Key(String),
    Index(usize),
    /// The first element of a list whose key has a value: links[rel=next].
    Match(String, String),
}

/// A path such as results, data.items, kontakter[0].telefon or kontakter.0.telefon. A leading
/// $ or $. is allowed, as in JSONPath.
pub fn parse_path(path: &str) -> Result<Vec<Step>> {
    let p = path.trim();
    let p = p.strip_prefix('$').unwrap_or(p);
    let p = p.strip_prefix('.').unwrap_or(p);
    let mut out = Vec::new();
    if p.is_empty() {
        return Ok(out);
    }
    for part in p.split('.') {
        let mut rest = part;
        // The name before any [n].
        let name_end = rest.find('[').unwrap_or(rest.len());
        let name = &rest[..name_end];
        if !name.is_empty() {
            match name.parse::<usize>() {
                Ok(i) if name.chars().all(|c| c.is_ascii_digit()) => out.push(Step::Index(i)),
                _ => out.push(Step::Key(name.to_string())),
            }
        } else if name_end == rest.len() {
            return err(format!("The path {} has an empty part.", path.trim()));
        }
        rest = &rest[name_end..];
        while let Some(r) = rest.strip_prefix('[') {
            let close = r.find(']').ok_or_else(|| GbError(format!("A [ in the path {} is not closed.", path.trim())))?;
            let inside = r[..close].trim().trim_matches(['"', '\'']);
            match (inside.parse::<usize>(), inside.split_once('=')) {
                (Ok(i), _) => out.push(Step::Index(i)),
                (Err(_), Some((k, v))) => out.push(Step::Match(k.trim().to_string(), v.trim().trim_matches(['"', '\'']).to_string())),
                _ => out.push(Step::Key(inside.to_string())),
            }
            rest = &r[close + 1..];
        }
        if !rest.is_empty() {
            return err(format!("The path {} cannot be read near {}.", path.trim(), rest));
        }
    }
    Ok(out)
}

/// The value at a path, if there is one. Keys are matched without a byte order mark.
pub fn get<'a>(v: &'a Json, path: &[Step]) -> Option<&'a Json> {
    let mut at = v;
    for step in path {
        at = match (step, at) {
            (Step::Key(k), Json::Object(o)) => o.get(k).or_else(|| o.iter().find(|(key, _)| key.trim_start_matches(BOM) == k).map(|(_, v)| v))?,
            (Step::Index(i), Json::Array(a)) => a.get(*i)?,
            // A number as a key of an object, such as "2026".
            (Step::Index(i), Json::Object(o)) => o.get(&i.to_string())?,
            (Step::Match(k, v), Json::Array(a)) => a.iter().find(|e| match e.get(k) {
                Some(Json::String(s)) => s == v,
                Some(Json::Number(n)) => v.parse::<f64>().ok() == n.as_f64(),
                Some(Json::Bool(b)) => v.parse::<bool>().ok() == Some(*b),
                Some(_) => false,
                None => false,
            })?,
            _ => return None,
        };
    }
    Some(at)
}

/// The paths to every value in a JSON value, objects and lists followed, to offer in a form.
/// Lists show their first element as [0].
pub fn paths(v: &Json, prefix: &str, out: &mut Vec<String>) {
    match v {
        Json::Object(o) => {
            for (k, x) in o {
                let k = clean_key(k);
                let p = if prefix.is_empty() { k } else { format!("{}.{}", prefix, k) };
                if !matches!(x, Json::Object(_) | Json::Array(_)) && !out.contains(&p) {
                    out.push(p.clone());
                }
                paths(x, &p, out);
            }
        }
        Json::Array(a) => {
            if let Some(first) = a.first() {
                let p = format!("{}[0]", prefix);
                if !matches!(first, Json::Object(_) | Json::Array(_)) && !out.contains(&p) {
                    out.push(p.clone());
                }
                paths(first, &p, out);
            }
        }
        _ => {}
    }
}

// ---------- Values ----------

/// A JSON value as a value of an attribute: numbers, text and true/false as they are, null as
/// empty, and objects and lists as compact JSON text.
pub fn to_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => n.as_f64().map_or(Value::Null, Value::Real),
        },
        Json::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}

/// An attribute value as JSON: a text that holds JSON is kept as JSON when `parse_text` is set.
pub fn from_value(v: &Value, parse_text: bool) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::from(*i),
        Value::Real(r) => serde_json::Number::from_f64(*r).map_or(Json::Null, Json::Number),
        Value::Text(t) => {
            let s = t.trim_start();
            if parse_text && (s.starts_with('{') || s.starts_with('[')) {
                if let Ok(j) = serde_json::from_str::<Json>(t) {
                    return j;
                }
            }
            Json::String(t.clone())
        }
    }
}

/// How lists inside a record are written when it is flattened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lists {
    /// As JSON text in one attribute.
    Text,
    /// An attribute per element: name.0, name.1, …
    Columns,
}

/// The values of an object as (name, value), nested objects as name.inner, lists as `lists`
/// says. A value that is not an object gives one pair with the prefix, or "value".
pub fn flatten(v: &Json, prefix: &str, lists: Lists, out: &mut Vec<(String, Value)>) {
    let join = |k: &str| if prefix.is_empty() { k.to_string() } else { format!("{}.{}", prefix, k) };
    match v {
        Json::Object(o) if !o.is_empty() => {
            for (k, x) in o {
                flatten(x, &join(&clean_key(k)), lists, out);
            }
        }
        Json::Array(a) if lists == Lists::Columns && !a.is_empty() => {
            for (i, x) in a.iter().enumerate() {
                flatten(x, &join(&i.to_string()), lists, out);
            }
        }
        other => out.push((if prefix.is_empty() { "value".to_string() } else { prefix.to_string() }, to_value(other))),
    }
}

/// Rows of (name, value) as a table without geometry: the attributes in the order they are
/// first met, each with the narrowest type that holds its values.
pub fn rows_to_table(rows: Vec<Vec<(String, Value)>>) -> Table {
    let mut names: Vec<String> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for row in &rows {
        for (k, _) in row {
            if !index.contains_key(k) {
                index.insert(k.clone(), names.len());
                names.push(k.clone());
            }
        }
    }
    let features: Vec<Feature> = rows
        .into_iter()
        .map(|row| {
            let mut attrs = vec![Value::Null; names.len()];
            for (k, v) in row {
                attrs[index[&k]] = v;
            }
            Feature { z: None, attrs, geom: None }
        })
        .collect();
    let fields = names.iter().enumerate().map(|(i, n)| Field::new(n.clone(), infer_type(features.iter().map(|f| &f.attrs[i])))).collect();
    Table { fields, crs: None, features, ..Default::default() }
}

/// Where the records are: at a path that is given, or else the value itself when it is a
/// list, or else the list of objects in it that is most likely the data, such as results,
/// data or items. A single object is one record. Returns the records and where they were.
pub fn find_records<'a>(v: &'a Json, path: Option<&str>) -> Result<(Vec<&'a Json>, String)> {
    if let Some(p) = path.map(str::trim).filter(|p| !p.is_empty()) {
        let steps = parse_path(p)?;
        let at = get(v, &steps).ok_or_else(|| {
            let mut found = Vec::new();
            if let Json::Object(o) = v {
                found = o.keys().map(|k| clean_key(k)).collect();
            }
            GbError(format!(
                "There is nothing at {} in the JSON. At the top it has: {}.",
                p,
                if found.is_empty() { "no named values".to_string() } else { found.join(", ") }
            ))
        })?;
        return Ok((
            match at {
                Json::Array(a) => a.iter().collect(),
                other => vec![other],
            },
            p.to_string(),
        ));
    }
    match v {
        Json::Array(a) => Ok((a.iter().collect(), "the list at the top".to_string())),
        Json::Object(o) => {
            // A list of objects, by a likely name, or else the longest such list, a level down too.
            let lists: Vec<(String, &Vec<Json>)> = o
                .iter()
                .filter_map(|(k, x)| match x {
                    Json::Array(a) if a.iter().any(|e| e.is_object()) => Some((clean_key(k), a)),
                    _ => None,
                })
                .collect();
            const LIKELY: &[&str] = &["results", "result", "data", "items", "records", "rows", "features", "hits", "values", "entries"];
            let best = lists.iter().find(|(k, _)| LIKELY.contains(&k.to_lowercase().as_str())).or_else(|| lists.iter().max_by_key(|(_, a)| a.len()));
            if let Some((k, a)) = best {
                return Ok((a.iter().collect(), k.clone()));
            }
            for (k, x) in o {
                if let Json::Object(inner) = x {
                    if inner.values().any(|y| matches!(y, Json::Array(a) if a.iter().any(|e| e.is_object()))) {
                        let (recs, at) = find_records(x, None)?;
                        return Ok((recs, format!("{}.{}", clean_key(k), at)));
                    }
                }
            }
            Ok((vec![v], "the object at the top".to_string()))
        }
        other => Ok((vec![other], "the value".to_string())),
    }
}

/// A JSON file as a table: a feature for each record.
pub fn read_table(path: &Path, records: Option<&str>, lists: Lists, nested_as_text: bool, notes: &mut crate::io::Notes) -> Result<Table> {
    let v = read_file(path)?;
    // Statistics in JSON-stat, as from SCB or Eurostat, become a row per value.
    if records.is_none_or(|r| r.trim().is_empty()) && crate::jsonstat::is_jsonstat(&v) {
        let t = crate::jsonstat::to_table(&v, &crate::jsonstat::Layout { labels: true, ..Default::default() })?;
        notes.info(format!("JSON-stat: {} values", crate::model::fmt_count(t.len() as u64)));
        return Ok(t);
    }
    let (recs, at) = find_records(&v, records)?;
    let rows: Vec<Vec<(String, Value)>> = recs
        .into_iter()
        .map(|r| {
            let mut row = Vec::new();
            match r {
                Json::Object(o) if nested_as_text => {
                    for (k, x) in o {
                        row.push((clean_key(k), to_value(x)));
                    }
                }
                other => flatten(other, "", lists, &mut row),
            }
            row
        })
        .collect();
    let t = rows_to_table(rows);
    notes.info(format!("{} records from {}", crate::model::fmt_count(t.len() as u64), at));
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_found_flattened_and_typed() {
        let text = "\u{feff}{\"resultCount\": 2, \"results\": [{\"\u{feff}id\": \"1\", \"namn\": \"Alingsås lasarett\", \"xkoord\": 6423475.733, \"adress\": {\"ort\": \"Alingsås\", \"nr\": 30}, \"tel\": [\"a\", \"b\"]}, {\"\u{feff}id\": \"5\", \"namn\": \"Dalslands sjukhus\", \"xkoord\": 6521088, \"extra\": true}]}";
        let v = parse(text).unwrap();
        let (recs, at) = find_records(&v, None).unwrap();
        assert_eq!((recs.len(), at.as_str()), (2, "results"));
        let rows = recs.iter().map(|r| {
            let mut row = Vec::new();
            flatten(r, "", Lists::Text, &mut row);
            row
        });
        let t = rows_to_table(rows.collect());
        assert_eq!(t.field_names(), ["id", "namn", "xkoord", "adress.ort", "adress.nr", "tel", "extra"]);
        assert_eq!(t.fields[0].ty, crate::model::FieldType::Text, "a code in quotes stays text");
        assert_eq!(t.fields[2].ty, crate::model::FieldType::Real);
        assert_eq!(t.features[0].attrs[5], Value::Text("[\"a\",\"b\"]".into()));
        assert_eq!(t.features[1].attrs[3], Value::Null);
        let mut cols = Vec::new();
        flatten(recs[0], "", Lists::Columns, &mut cols);
        assert!(cols.iter().any(|(k, v)| k == "tel.1" && *v == Value::Text("b".into())));
    }

    #[test]
    fn paths_reach_into_objects_and_lists() {
        let v: Json = serde_json::from_str(r#"{"data": {"items": [{"k": [{"tel": "031"}]}]}}"#).unwrap();
        for p in ["data.items[0].k[0].tel", "$.data.items.0.k.0.tel", "data['items'][0].k[0].tel"] {
            assert_eq!(get(&v, &parse_path(p).unwrap()), Some(&Json::String("031".into())), "{}", p);
        }
        assert_eq!(get(&v, &parse_path("data.nothing").unwrap()), None);
        let mut all = Vec::new();
        paths(&v, "", &mut all);
        assert_eq!(all, ["data.items[0].k[0].tel"]);
        let (recs, at) = find_records(&v, None).unwrap();
        assert_eq!((recs.len(), at.as_str()), (1, "data.items"));
        assert!(find_records(&v, Some("data.missing")).unwrap_err().0.contains("At the top it has: data"));
        assert_eq!(parse("{\"a\":1}\n{\"a\":2}\n").unwrap().as_array().unwrap().len(), 2);
    }
}
