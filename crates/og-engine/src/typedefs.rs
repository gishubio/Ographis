//! What a GeoJSON or JSON file holds, for those who use it in code: a JSON Schema
//! (name.schema.json) and TypeScript types (name.d.ts), made from the attributes, their types
//! and whether they are ever empty, and the kinds of geometry.

use crate::model::{FieldType, Table, Value};
use serde_json::{json, Map, Value as Json};

/// What an attribute holds, as the schema and the types say it.
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Integer,
    Number,
    Text,
    /// JSON within the text, written as JSON: of any shape.
    Any,
}

/// An attribute, or an object of attributes under a name with a dot in it.
#[derive(Debug, Clone)]
enum Node {
    Leaf { kind: Kind, nullable: bool },
    Object(Vec<(String, Node)>),
}

fn kinds_of(table: &Table, parse_json: bool) -> Vec<(String, Kind, bool)> {
    table
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let nullable = table.features.iter().any(|ft| ft.attrs.get(i).is_none_or(|v| v.is_null()));
            let kind = match f.ty {
                FieldType::Int => Kind::Integer,
                FieldType::Real => Kind::Number,
                FieldType::Text => {
                    let json_text = parse_json
                        && table.features.iter().any(|ft| matches!(ft.attrs.get(i), Some(Value::Text(t)) if (t.trim_start().starts_with('{') || t.trim_start().starts_with('[')) && serde_json::from_str::<Json>(t).is_ok()));
                    if json_text {
                        Kind::Any
                    } else {
                        Kind::Text
                    }
                }
            };
            (f.name.clone(), kind, nullable)
        })
        .collect()
}

/// The attributes as a tree: names with a dot as objects within objects, when `nest`.
fn tree(fields: &[(String, Kind, bool)], nest: bool) -> Vec<(String, Node)> {
    let mut out: Vec<(String, Node)> = Vec::new();
    for (name, kind, nullable) in fields {
        let path: Vec<&str> = if nest { name.split('.').filter(|p| !p.is_empty()).collect() } else { vec![name.as_str()] };
        let path = if path.is_empty() { vec![name.as_str()] } else { path };
        insert(&mut out, &path, Node::Leaf { kind: kind.clone(), nullable: *nullable });
    }
    out
}

fn insert(level: &mut Vec<(String, Node)>, path: &[&str], leaf: Node) {
    if path.len() == 1 {
        level.push((path[0].to_string(), leaf));
        return;
    }
    let slot = match level.iter().position(|(n, node)| n == path[0] && matches!(node, Node::Object(_))) {
        Some(k) => k,
        None => {
            level.push((path[0].to_string(), Node::Object(Vec::new())));
            level.len() - 1
        }
    };
    if let Node::Object(inner) = &mut level[slot].1 {
        insert(inner, &path[1..], leaf);
    }
}

fn schema_of(nodes: &[(String, Node)]) -> Json {
    let mut props = Map::new();
    let mut required = Vec::new();
    for (name, node) in nodes {
        let s = match node {
            Node::Leaf { kind, nullable } => {
                let t = match kind {
                    Kind::Integer => Some("integer"),
                    Kind::Number => Some("number"),
                    Kind::Text => Some("string"),
                    Kind::Any => None,
                };
                match (t, nullable) {
                    (Some(t), true) => json!({"type": [t, "null"]}),
                    (Some(t), false) => json!({"type": t}),
                    (None, _) => json!({}),
                }
            }
            Node::Object(inner) => schema_of(inner),
        };
        props.insert(name.clone(), s);
        required.push(Json::String(name.clone()));
    }
    json!({"type": "object", "properties": props, "required": required})
}

fn ts_of(nodes: &[(String, Node)], indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let mut s = String::from("{\n");
    for (name, node) in nodes {
        let key = if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit()) {
            name.clone()
        } else {
            format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
        };
        let t = match node {
            Node::Leaf { kind, nullable } => {
                let base = match kind {
                    Kind::Integer | Kind::Number => "number",
                    Kind::Text => "string",
                    Kind::Any => "unknown",
                };
                let comment = if *kind == Kind::Integer { " // integer" } else { "" };
                format!("{}{};{}", base, if *nullable && *kind != Kind::Any { " | null" } else { "" }, comment)
            }
            Node::Object(inner) => format!("{};", ts_of(inner, indent + 1)),
        };
        s.push_str(&format!("{}  {}: {}\n", pad, key, t));
    }
    s.push_str(&format!("{}}}", pad));
    s
}

/// A name for the types: the name of the file in capitals, as TypeScript names types.
pub fn type_name(stem: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(if up { c.to_ascii_uppercase() } else { c });
            up = false;
        } else {
            up = true;
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "Data");
    }
    out
}

/// The kinds of geometry of a stream, as GeoJSON names them.
fn geometry_kinds(table: &Table) -> (Vec<&'static str>, bool) {
    use geo::Geometry as G;
    let mut kinds: Vec<&'static str> = Vec::new();
    let mut none = false;
    for f in &table.features {
        let k = match &f.geom {
            None => {
                none = true;
                continue;
            }
            Some(G::Point(_)) => "Point",
            Some(G::MultiPoint(_)) => "MultiPoint",
            Some(G::LineString(_)) | Some(G::Line(_)) => "LineString",
            Some(G::MultiLineString(_)) => "MultiLineString",
            Some(G::Polygon(_)) | Some(G::Rect(_)) | Some(G::Triangle(_)) => "Polygon",
            Some(G::MultiPolygon(_)) => "MultiPolygon",
            Some(G::GeometryCollection(_)) => "GeometryCollection",
        };
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    (kinds, none)
}

/// The schema and the types of a GeoJSON file of this stream.
pub fn for_geojson(table: &Table, stem: &str) -> (String, String) {
    let nodes = tree(&kinds_of(table, false), false);
    let (kinds, none) = geometry_kinds(table);
    let mut geometry = json!({"type": "object", "properties": {"type": {"enum": kinds}, "coordinates": {"type": "array"}}, "required": ["type"]});
    if none {
        geometry = json!({"oneOf": [geometry, {"type": "null"}]});
    }
    let name = type_name(stem);
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": name,
        "description": format!("{}.geojson, as Ographis wrote it", stem),
        "type": "object",
        "properties": {
            "type": {"const": "FeatureCollection"},
            "features": {"type": "array", "items": {
                "type": "object",
                "properties": {"type": {"const": "Feature"}, "properties": schema_of(&nodes), "geometry": geometry},
                "required": ["type", "properties", "geometry"]
            }}
        },
        "required": ["type", "features"]
    });
    let ts_geom = match kinds.as_slice() {
        [one] => format!("GeoJSON.{}", one),
        [] => "null".to_string(),
        many => many.iter().map(|k| format!("GeoJSON.{}", k)).collect::<Vec<_>>().join(" | "),
    };
    let ts_geom = if none && !kinds.is_empty() { format!("{} | null", ts_geom) } else { ts_geom };
    let ts = format!(
        "// {stem}.geojson, as Ographis wrote it. The GeoJSON types come from @types/geojson.\n\nexport interface {name}Properties {props}\n\nexport type {name}Feature = GeoJSON.Feature<{geom}, {name}Properties>;\nexport type {name} = GeoJSON.FeatureCollection<{geom}, {name}Properties>;\n",
        stem = stem,
        name = name,
        props = ts_of(&nodes, 0),
        geom = ts_geom
    );
    (serde_json::to_string_pretty(&schema).unwrap_or_default(), ts)
}

/// The schema and the types of a plain JSON file of this stream: a list of objects, or one
/// object, with names with a dot as objects, and the geometry as GeoJSON or WKT if it is in.
pub fn for_json(table: &Table, stem: &str, shape: &str, list_name: &str, geometry: &str) -> (String, String) {
    let mut nodes = tree(&kinds_of(table, true), true);
    let (_, none) = geometry_kinds(table);
    match geometry {
        "geojson" => nodes.push(("geometry".into(), Node::Leaf { kind: Kind::Any, nullable: none })),
        "wkt" => nodes.push(("geometry".into(), Node::Leaf { kind: Kind::Text, nullable: none })),
        _ => {}
    }
    let name = type_name(stem);
    let item = schema_of(&nodes);
    let (body, ts_whole) = match (shape, list_name.trim()) {
        ("first", _) | ("lines", _) => (item.clone(), format!("export type {name}Lines = {name}Item; // one on each line", name = name)),
        (_, "") => (json!({"type": "array", "items": item}), format!("export type {name} = {name}Item[];", name = name)),
        (_, key) => (
            json!({"type": "object", "properties": {key: {"type": "array", "items": item}}, "required": [key]}),
            format!("export interface {name} {{\n  {key}: {name}Item[];\n}}", name = name, key = key),
        ),
    };
    let ts_whole = if shape == "first" { format!("export type {name} = {name}Item;", name = name) } else { ts_whole };
    let mut schema = body;
    if let Json::Object(o) = &mut schema {
        o.insert("$schema".into(), json!("https://json-schema.org/draft/2020-12/schema"));
        o.insert("title".into(), json!(name));
        o.insert(
            "description".into(),
            json!(format!("{}.json, as Ographis wrote it{}", stem, if shape == "lines" { ": each line is one such object" } else { "" })),
        );
    }
    let ts = format!(
        "// {stem}.json, as Ographis wrote it.\n\nexport interface {name}Item {item}\n\n{whole}\n",
        stem = stem,
        name = name,
        item = ts_of(&nodes, 0),
        whole = ts_whole
    );
    (serde_json::to_string_pretty(&schema).unwrap_or_default(), ts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field};

    #[test]
    fn the_schema_and_types_follow_the_attributes() {
        let t = Table {
            fields: vec![Field::new("name", FieldType::Text), Field::new("pupils", FieldType::Int), Field::new("contact.email", FieldType::Text)],
            features: vec![
                Feature {
                    attrs: vec![Value::Text("A".into()), Value::Int(3), Value::Null],
                    geom: Some(geo::Geometry::Point(geo::Point::new(1.0, 2.0))),
                    z: None,
                },
                Feature {
                    attrs: vec![Value::Text("B".into()), Value::Int(4), Value::Text("b@x.se".into())],
                    geom: Some(geo::Geometry::Point(geo::Point::new(3.0, 4.0))),
                    z: None,
                },
            ],
            ..Default::default()
        };
        let (schema, ts) = for_geojson(&t, "skolor");
        let s: Json = serde_json::from_str(&schema).unwrap();
        let props = &s["properties"]["features"]["items"]["properties"]["properties"]["properties"];
        assert_eq!(props["pupils"]["type"], "integer");
        assert_eq!(props["contact.email"]["type"], json!(["string", "null"]));
        assert!(
            ts.contains("export interface SkolorProperties")
                && ts.contains("pupils: number; // integer")
                && ts.contains("GeoJSON.FeatureCollection<GeoJSON.Point, SkolorProperties>"),
            "{}",
            ts
        );
        let (schema, ts) = for_json(&t, "skolor", "list", "items", "none");
        let s: Json = serde_json::from_str(&schema).unwrap();
        assert_eq!(s["properties"]["items"]["items"]["properties"]["contact"]["properties"]["email"]["type"], json!(["string", "null"]));
        assert!(ts.contains("contact: {") && ts.contains("items: SkolorItem[];"), "{}", ts);
    }
}
