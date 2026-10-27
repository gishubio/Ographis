//! JSON-stat: the format of statistics from SCB (PxWeb), Eurostat and many other statistics
//! agencies. A dataset is a cube: dimensions such as region, sex and year, each with its
//! categories, and the values in one long list, the last dimension changing fastest. Here
//! it becomes a plain table, a row for each value with the code and the name of each
//! category, or with the categories of one dimension as columns of their own.
//!
//! Both version 2.0 (a dataset, or a collection of datasets) and the older 1.x (a bundle of
//! named datasets, with id and size inside dimension) are read.

use crate::model::{err, infer_type, Field, FieldType, GbError, Result, Table, Value};
use serde_json::Value as Json;

/// Is this JSON-stat?
pub fn is_jsonstat(v: &Json) -> bool {
    match v {
        Json::Object(o) => {
            let class = o.get("class").and_then(|c| c.as_str());
            (matches!(class, Some("dataset")) && o.contains_key("dimension"))
                || (matches!(class, Some("collection")) && o.get("link").is_some())
                || (o.get("dimension").is_some() && o.get("value").is_some())
                // A 1.x bundle: datasets by name.
                || (!o.is_empty() && o.values().all(|d| d.get("dimension").is_some() && d.get("value").is_some()))
        }
        _ => false,
    }
}

/// How the table is laid out.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// The categories of this dimension become columns, each with its values.
    pub columns_from: Option<String>,
    /// Rows without a value are left out.
    pub skip_missing: bool,
    /// The names of the categories too, in a column next to each code.
    pub labels: bool,
}

/// A row of the wide layout: the places of its dimensions, and its values so far.
type WideRow = (Vec<usize>, Vec<(String, Value)>);

struct Dim {
    id: String,
    /// Codes in their order.
    codes: Vec<String>,
    labels: Vec<String>,
}

/// The datasets in a JSON-stat answer, each as (its label, its dimensions, values, status).
fn datasets(v: &Json) -> Result<Vec<&Json>> {
    let o = v.as_object().ok_or_else(|| GbError("JSON-stat is an object.".into()))?;
    match o.get("class").and_then(|c| c.as_str()) {
        Some("dataset") => Ok(vec![v]),
        Some("collection") => {
            let items = o.get("link").and_then(|l| l.get("item")).and_then(|i| i.as_array()).ok_or_else(|| GbError("The collection has no items.".into()))?;
            let inside: Vec<&Json> =
                items.iter().filter(|i| i.get("class").and_then(|c| c.as_str()) == Some("dataset") && i.get("dimension").is_some()).collect();
            if inside.is_empty() {
                return err("The collection only links to its datasets. Fetch one of them, from the addresses in link.item.");
            }
            Ok(inside)
        }
        _ if o.contains_key("dimension") => Ok(vec![v]),
        _ => Ok(o.values().filter(|d| d.get("dimension").is_some()).collect()),
    }
}

fn dims(ds: &Json) -> Result<Vec<Dim>> {
    let dimension = ds.get("dimension").and_then(|d| d.as_object()).ok_or_else(|| GbError("The dataset has no dimensions.".into()))?;
    // 2.0 has id and size on the dataset, 1.x inside dimension.
    let ids = ds
        .get("id")
        .or_else(|| dimension.get("id"))
        .and_then(|i| i.as_array())
        .ok_or_else(|| GbError("The dataset does not list its dimensions (id).".into()))?;
    let sizes =
        ds.get("size").or_else(|| dimension.get("size")).and_then(|s| s.as_array()).ok_or_else(|| GbError("The dataset does not give its size.".into()))?;
    if ids.len() != sizes.len() {
        return err("The dataset has as many sizes as dimensions, but this one does not.");
    }
    let mut out = Vec::new();
    for (id, size) in ids.iter().zip(sizes) {
        let id = id.as_str().unwrap_or_default().to_string();
        let size = size.as_u64().unwrap_or(0) as usize;
        let cat = dimension.get(&id).and_then(|d| d.get("category")).ok_or_else(|| GbError(format!("The dimension {} has no categories.", id)))?;
        let label_of = |code: &str| cat.get("label").and_then(|l| l.get(code)).and_then(|l| l.as_str()).unwrap_or(code).to_string();
        let mut codes = vec![String::new(); size];
        match cat.get("index") {
            Some(Json::Array(a)) => {
                for (k, c) in a.iter().enumerate().take(size) {
                    codes[k] = c.as_str().unwrap_or_default().to_string();
                }
            }
            Some(Json::Object(o)) => {
                for (c, k) in o {
                    if let Some(k) = k.as_u64().map(|k| k as usize).filter(|k| *k < size) {
                        codes[k] = c.clone();
                    }
                }
            }
            // Without an index, the labels give the order, as for a single category.
            _ => {
                if let Some(l) = cat.get("label").and_then(|l| l.as_object()) {
                    for (k, c) in l.keys().enumerate().take(size) {
                        codes[k] = c.clone();
                    }
                }
            }
        }
        let labels = codes.iter().map(|c| label_of(c)).collect();
        out.push(Dim { id, codes, labels });
    }
    Ok(out)
}

/// The value and the status at a place in the long list.
fn value_at(ds: &Json, k: usize) -> (Value, Option<String>) {
    let v = match ds.get("value") {
        Some(Json::Array(a)) => a.get(k),
        Some(Json::Object(o)) => o.get(&k.to_string()),
        _ => None,
    };
    let value = match v {
        Some(Json::Number(n)) => n.as_i64().map(Value::Int).unwrap_or_else(|| Value::Real(n.as_f64().unwrap_or(f64::NAN))),
        Some(Json::String(s)) => s.trim().replace(',', ".").parse::<f64>().map(Value::Real).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let status = match ds.get("status") {
        Some(Json::String(s)) => Some(s.clone()),
        Some(Json::Array(a)) if a.len() == 1 => a[0].as_str().map(|s| s.to_string()),
        Some(Json::Array(a)) => a.get(k).and_then(|s| s.as_str()).map(|s| s.to_string()),
        Some(Json::Object(o)) => o.get(&k.to_string()).and_then(|s| s.as_str()).map(|s| s.to_string()),
        _ => None,
    };
    (value, status)
}

/// Makes a table of JSON-stat. With several datasets, the rows of each, with the name of the
/// dataset in an attribute.
pub fn to_table(v: &Json, layout: &Layout) -> Result<Table> {
    let sets = datasets(v)?;
    let several = sets.len() > 1;
    let mut names: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<(String, Value)>> = Vec::new();
    let add_name = |n: &str, names: &mut Vec<String>| {
        if !names.iter().any(|x| x == n) {
            names.push(n.to_string());
        }
    };
    for (key, ds) in sets.iter().enumerate() {
        let dims = dims(ds)?;
        let total: usize = dims.iter().map(|d| d.codes.len()).product();
        let has_status = ds.get("status").is_some();
        let set_name = ds.get("label").and_then(|l| l.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("dataset {}", key + 1));
        let wide = match &layout.columns_from {
            Some(c) => Some(dims.iter().position(|d| d.id.eq_ignore_ascii_case(c.trim())).ok_or_else(|| {
                GbError(format!(
                    "There is no dimension {} to make columns of. There are: {}.",
                    c,
                    dims.iter().map(|d| d.id.as_str()).collect::<Vec<_>>().join(", ")
                ))
            })?),
            None => None,
        };
        // Walks the cube: the place of each dimension, the last one fastest.
        let mut at = vec![0usize; dims.len()];
        let mut current: Option<WideRow> = None;
        for k in 0..total {
            let (value, status) = value_at(ds, k);
            let key_of = |at: &[usize]| at.iter().enumerate().filter(|(i, _)| Some(*i) != wide).map(|(_, x)| *x).collect::<Vec<usize>>();
            match wide {
                None => {
                    if !(layout.skip_missing && value == Value::Null) {
                        let mut row = Vec::new();
                        if several {
                            row.push(("dataset".to_string(), Value::Text(set_name.clone())));
                        }
                        for (d, x) in dims.iter().zip(&at) {
                            row.push((d.id.clone(), Value::Text(d.codes[*x].clone())));
                            if layout.labels {
                                row.push((format!("{}_label", d.id), Value::Text(d.labels[*x].clone())));
                            }
                        }
                        row.push(("value".into(), value));
                        if has_status {
                            row.push(("status".into(), status.map(Value::Text).unwrap_or(Value::Null)));
                        }
                        rows.push(row);
                    }
                }
                Some(w) => {
                    let this = key_of(&at);
                    if current.as_ref().is_none_or(|c| c.0 != this) {
                        if let Some((_, row)) = current.take() {
                            rows.push(row);
                        }
                        let mut row = Vec::new();
                        if several {
                            row.push(("dataset".to_string(), Value::Text(set_name.clone())));
                        }
                        for (i, (d, x)) in dims.iter().zip(&at).enumerate() {
                            if i != w {
                                row.push((d.id.clone(), Value::Text(d.codes[*x].clone())));
                                if layout.labels {
                                    row.push((format!("{}_label", d.id), Value::Text(d.labels[*x].clone())));
                                }
                            }
                        }
                        current = Some((this, row));
                    }
                    let d = &dims[w];
                    let name = if layout.labels { d.labels[at[w]].clone() } else { d.codes[at[w]].clone() };
                    if let Some((_, row)) = current.as_mut() {
                        row.push((name, value));
                    }
                }
            }
            // The next place.
            for i in (0..dims.len()).rev() {
                at[i] += 1;
                if at[i] < dims[i].codes.len() {
                    break;
                }
                at[i] = 0;
            }
        }
        if let Some((_, row)) = current.take() {
            // In the wide layout a row with no value at all is left out when asked.
            rows.push(row);
        }
    }
    if let Some(_w) = &layout.columns_from {
        if layout.skip_missing {
            rows.retain(|r| r.iter().any(|(k, v)| *v != Value::Null && !k.ends_with("_label")));
        }
    }
    for r in &rows {
        for (k, _) in r {
            add_name(k, &mut names);
        }
    }
    let index: std::collections::HashMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    let features: Vec<crate::model::Feature> = rows
        .iter()
        .map(|r| {
            let mut attrs = vec![Value::Null; names.len()];
            for (k, v) in r {
                attrs[index[k.as_str()]] = v.clone();
            }
            crate::model::Feature { attrs, geom: None, z: None }
        })
        .collect();
    let fields = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            // Codes stay text, so that 0114 keeps its zero.
            let t = infer_type(features.iter().map(|f| &f.attrs[i]));
            let is_code = rows.first().is_some_and(|r| r.iter().any(|(k, v)| k == n && matches!(v, Value::Text(_))));
            Field::new(n.clone(), if is_code { FieldType::Text } else { t })
        })
        .collect();
    Ok(Table { fields, features, ..Table::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As SCB answers with json-stat2: population by region, sex and year.
    const SCB: &str = r#"{
      "class": "dataset", "version": "2.0", "label": "Folkmängden efter region, kön och år",
      "source": "SCB", "updated": "2026-02-20T07:00:00Z",
      "id": ["Region", "Kon", "ContentsCode", "Tid"], "size": [2, 2, 1, 2],
      "dimension": {
        "Region": {"label": "region", "category": {"index": {"1490": 0, "1480": 1}, "label": {"1490": "Borås", "1480": "Göteborg"}}},
        "Kon": {"label": "kön", "category": {"index": {"1": 0, "2": 1}, "label": {"1": "män", "2": "kvinnor"}}},
        "ContentsCode": {"label": "tabellinnehåll", "category": {"index": {"BE0101N1": 0}, "label": {"BE0101N1": "Folkmängd"}, "unit": {"BE0101N1": {"base": "antal", "decimals": 0}}}},
        "Tid": {"label": "år", "category": {"index": {"2024": 0, "2025": 1}, "label": {"2024": "2024", "2025": "2025"}}}
      },
      "role": {"time": ["Tid"], "metric": ["ContentsCode"]},
      "value": [57000, 57400, 56800, 57300, 302000, 304100, 298000, 300500]
    }"#;

    #[test]
    fn a_cube_becomes_rows_last_dimension_fastest() {
        let v: Json = serde_json::from_str(SCB).unwrap();
        assert!(is_jsonstat(&v));
        let t = to_table(&v, &Layout { labels: true, ..Default::default() }).unwrap();
        assert_eq!(t.len(), 8);
        assert_eq!(t.field_names(), ["Region", "Region_label", "Kon", "Kon_label", "ContentsCode", "ContentsCode_label", "Tid", "Tid_label", "value"]);
        let row = |i: usize| t.features[i].attrs.clone();
        assert_eq!(row(1)[0], Value::Text("1490".into()));
        assert_eq!(row(1)[6], Value::Text("2025".into()));
        assert_eq!(row(1)[8], Value::Int(57400));
        assert_eq!(row(6)[1], Value::Text("Göteborg".into()));
        assert_eq!(row(6)[3], Value::Text("kvinnor".into()));
        assert_eq!(row(6)[8], Value::Int(298000));
        assert_eq!(t.fields[6].ty, FieldType::Text, "a year as a code stays text");
        // The years as columns.
        let w = to_table(&v, &Layout { columns_from: Some("Tid".into()), ..Default::default() }).unwrap();
        assert_eq!(w.len(), 4);
        assert_eq!(w.field_names(), ["Region", "Kon", "ContentsCode", "2024", "2025"]);
        assert_eq!(w.features[3].attrs[4], Value::Int(300500));
    }

    #[test]
    fn sparse_values_status_and_old_bundles() {
        // As Eurostat: values and status as objects, with gaps.
        let eurostat = r#"{"version": "2.0", "class": "dataset", "label": "Population",
          "id": ["freq", "geo", "time"], "size": [1, 2, 2],
          "dimension": {
            "freq": {"category": {"index": {"A": 0}, "label": {"A": "Annual"}}},
            "geo": {"category": {"index": {"SE232": 0, "SE231": 1}, "label": {"SE232": "Västsverige", "SE231": "Hallands län"}}},
            "time": {"category": {"index": {"2023": 0, "2024": 1}}}
          },
          "value": {"0": 1750000, "1": 1761234.5, "3": 345000},
          "status": {"1": "p"}}"#;
        let v: Json = serde_json::from_str(eurostat).unwrap();
        let t = to_table(&v, &Layout::default()).unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!(t.features[2].attrs[3], Value::Null);
        assert_eq!(t.features[1].attrs[3], Value::Real(1761234.5));
        assert_eq!(t.features[1].attrs[4], Value::Text("p".into()));
        let skip = to_table(&v, &Layout { skip_missing: true, ..Default::default() }).unwrap();
        assert_eq!(skip.len(), 3);
        // JSON-stat 1.x, as PxWeb answers with json-stat: a bundle, id and size in dimension.
        let old = r#"{"dataset": {"label": "x", "dimension": {"id": ["a"], "size": [2],
            "a": {"category": {"index": {"p": 0, "q": 1}, "label": {"p": "P", "q": "Q"}}}}, "value": [1, 2]}}"#;
        let v: Json = serde_json::from_str(old).unwrap();
        assert!(is_jsonstat(&v));
        let t = to_table(&v, &Layout::default()).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.features[1].attrs[1], Value::Int(2));
        assert!(!is_jsonstat(&serde_json::json!({"values": [1, 2]})));
    }
}
