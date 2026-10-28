//! What a program needs to know to write a model, and a check of a model before it runs:
//! the node types with their ports and settings as JSON (`og nodes --json`), a JSON Schema
//! of the model file, and `og check`, which says what is wrong and what would be right.

use crate::nodes::{node_type, Group, NodeType, ParamKind, ParamSpec, NODE_TYPES};
use crate::run::{is_variable_name, Workspace};
use serde_json::{json, Map, Value};

/// How a setting is written in the model file, for a program to read.
pub struct Kind {
    /// A short name: text, choice, expression, attribute, list…
    pub name: &'static str,
    /// What the value looks like in the file.
    pub shape: &'static str,
    /// The input port whose attributes the setting is about, if any.
    pub port: Option<&'static str>,
}

pub fn kind_of(kind: &ParamKind) -> Kind {
    let k = |name, shape, port| Kind { name, shape, port };
    match *kind {
        ParamKind::Text => k("text", "a string", None),
        ParamKind::Exact => k("text", "a string, where spaces count", None),
        ParamKind::Secret => k("secret", "a string; better a $(variable) or a connection in the settings than the secret itself", None),
        ParamKind::Connection => k("connection", "the name of a database connection in the settings of the user", None),
        ParamKind::ApiConnection => k("api_connection", "the name of an API connection in the settings of the user", None),
        ParamKind::Choice(_) => k("choice", "one of the values in choices, as a string", None),
        ParamKind::Crs => k("crs", "a coordinate system, such as \"EPSG:3006\"", None),
        ParamKind::Attribute(p) => k("attribute", "the name of one attribute of the stream on the port", Some(p)),
        ParamKind::Attributes(p) => k("attributes", "names of attributes of the stream on the port: a list of strings, or one string separated by commas", Some(p)),
        ParamKind::Expression(p) => k("expression", "an expression over the attributes of the stream on the port; text in 'single quotes', names with spaces in \"double quotes\"", Some(p)),
        ParamKind::Code(p) => k("code", "text over several lines, such as SQL, with {attribute} put in from the stream on the port", Some(p)),
        ParamKind::FileOpen | ParamKind::Source => k("path", "a file, a folder, a pattern with * or a web address; relative paths start at the folder of the model", None),
        ParamKind::FileSave => k("path", "a file to write; relative paths start at the folder of the model", None),
        ParamKind::Layers => k("layers", "names of layers of the file in path, separated by commas; empty reads the first, * all", None),
        ParamKind::OptionalName(_) => k("optional_name", "the name of an attribute to add, or empty for none", None),
        ParamKind::AttributeTable(p) => k("list", "a list of {\"name\": attribute, \"action\": \"keep\" | \"rename\" | \"remove\", \"to\": new name}", Some(p)),
        ParamKind::NewAttributes(p) => k("list", "a list of {\"name\": attribute, \"value\": expression}", Some(p)),
        ParamKind::Values(p, _) => k("list", "a list of strings", Some(p)),
        ParamKind::JsonPaths(p, _) => k("list", "a list of [path in the JSON, attribute name]", Some(p)),
        ParamKind::SortKeys(p) => k("list", "a list of [attribute, \"ascending\" | \"descending\"]", Some(p)),
        ParamKind::Mapping(p, _) => k("list", "a list of [value, what it becomes]", Some(p)),
        ParamKind::Renames(p) => k("object", "an object {\"old name\": \"new name\"}", Some(p)),
        ParamKind::Ranges(p) => k("list", "a list of [from, to, value]; an empty from or to has no end", Some(p)),
        ParamKind::Checks(p) => k(
            "list",
            "a list of {\"attribute\", \"required\": true | false, \"type\": \"integer\" | \"number\" | \"text\" | \"date\", \"max_length\", \"min\", \"max\", \"values\": \"a, b\"}",
            Some(p),
        ),
        ParamKind::Schema(p) => k("object", "an object {attribute: {\"type\": \"text\" | \"integer\" | \"decimal\", \"length\", \"decimals\"}}", Some(p)),
        ParamKind::MapLayers(p) => k(
            "list",
            "a list of layers, one per stream connected to the port: {\"from\": \"node.port\", \"fill\": \"#rrggbb\", \"stroke\", \"width\", \"size\", \"label\": attribute, …}",
            Some(p),
        ),
    }
}

fn group_name(g: Group) -> &'static str {
    match g {
        Group::Reader => "input",
        Group::Transformer => "tool",
        Group::Writer => "output",
    }
}

fn param_json(p: &ParamSpec) -> Value {
    let k = kind_of(&p.kind);
    let mut o = Map::new();
    o.insert("key".into(), json!(p.key));
    o.insert("label".into(), json!(p.label));
    o.insert("kind".into(), json!(k.name));
    o.insert("shape".into(), json!(k.shape));
    if let Some(port) = k.port {
        o.insert("port".into(), json!(port));
    }
    if let ParamKind::Choice(list) = p.kind {
        o.insert("choices".into(), Value::Array(list.iter().map(|(v, l)| json!({"value": v, "label": l})).collect()));
    }
    o.insert("default".into(), json!(p.default));
    if !p.hint.is_empty() {
        o.insert("hint".into(), json!(p.hint));
    }
    Value::Object(o)
}

fn type_json(t: &NodeType) -> Value {
    json!({
        "type": t.name,
        "title": t.title,
        "group": group_name(t.group),
        "category": t.category,
        "description": t.desc,
        "inputs": t.inputs,
        "outputs": t.outputs,
        "outputs_follow_settings": t.ports.is_some(),
        "offered": !t.hidden,
        "params": t.params.iter().map(param_json).collect::<Vec<_>>(),
    })
}

/// Every node type, the functions of expressions and the rules of the model file, as JSON.
pub fn nodes_json() -> Value {
    json!({
        "program": "Ographis",
        "version": env!("CARGO_PKG_VERSION"),
        "model": {
            "format": crate::run::FORMAT,
            "about": "A model is a JSON file (.ographis) with nodes and the connections between their ports. Each node has a type from node_types, an id that is unique in the model, a name that is shown, x and y on the canvas, and params by the keys of its type. A connection goes from [node id, output port] to [node id, input port]. An input port may take several connections: their streams are put together, except on ports that keep them apart, such as Layers of Draw map. Settings that are not given take their default. $(name) in a setting is the value of the variable name. Check a model with og check.",
            "example": {
                "format": crate::run::FORMAT,
                "name": "Towns as points",
                "variables": [{"name": "folder", "value": "out"}],
                "nodes": [
                    {"id": "towns", "type": "Reader", "name": "Towns", "x": 20, "y": 20, "params": {"path": "towns.csv", "x": "lon", "y": "lat", "crs": "EPSG:4326"}},
                    {"id": "big", "type": "Tester", "name": "Large towns", "x": 280, "y": 20, "params": {"test": "population >= 10000"}},
                    {"id": "out", "type": "Writer", "name": "Result", "x": 540, "y": 20, "params": {"path": "$(folder)/towns.gpkg"}}
                ],
                "edges": [
                    {"from": ["towns", "Output"], "to": ["big", "Input"]},
                    {"from": ["big", "Passed"], "to": ["out", "Input"]}
                ]
            }
        },
        "expressions": {
            "about": "As the field calculator of QGIS: attributes by name or in \"double quotes\", text in 'single quotes'. Operators: + - * / // % || = <> < <= > >= AND OR NOT IS NULL IN LIKE ILIKE, CASE WHEN … THEN … ELSE … END. $area, $length and $perimeter in m² and m, $x, $y and $id.",
            "functions": crate::expr::functions().iter().map(|(n, lo, hi, help)| json!({"name": n, "min_args": lo, "max_args": hi, "help": help})).collect::<Vec<_>>(),
        },
        "node_types": NODE_TYPES.iter().map(type_json).collect::<Vec<_>>(),
    })
}

/// A JSON Schema of the model file, with the type of each node and its settings.
pub fn model_schema() -> Value {
    let types: Vec<&str> = NODE_TYPES.iter().map(|t| t.name).collect();
    // Each type gets a branch that names its settings, so that an editor can say which
    // settings a node of that type has.
    let branches: Vec<Value> = NODE_TYPES
        .iter()
        .map(|t| {
            let props: Map<String, Value> = t
                .params
                .iter()
                .map(|p| {
                    let mut s = Map::new();
                    s.insert("title".into(), json!(p.label));
                    let k = kind_of(&p.kind);
                    let hint = if p.hint.is_empty() { String::new() } else { format!(" {}", p.hint) };
                    s.insert("description".into(), json!(format!("{}.{}", k.shape, hint)));
                    if let ParamKind::Choice(list) = p.kind {
                        // A $(variable) may stand for a choice.
                        s.insert("anyOf".into(), json!([{"enum": list.iter().map(|c| c.0).collect::<Vec<_>>()}, {"type": "string", "pattern": "\\$\\("}]));
                    }
                    if !p.default.is_empty() {
                        s.insert("default".into(), json!(p.default));
                    }
                    (p.key.to_string(), Value::Object(s))
                })
                .collect();
            json!({
                "if": {"properties": {"type": {"const": t.name}}},
                "then": {"properties": {"params": {"type": "object", "properties": props}}}
            })
        })
        .collect();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://raw.githubusercontent.com/gishubio/Ographis/main/docs/model.schema.json",
        "title": "Ographis model",
        "description": format!("A model of Ographis {}. Written by og schema.", env!("CARGO_PKG_VERSION")),
        "type": "object",
        "required": ["nodes"],
        "properties": {
            "format": {"type": "string", "const": crate::run::FORMAT},
            "name": {"type": "string"},
            "description": {"type": "string", "description": "What the model does, shown when it is opened."},
            "variables": {"type": "array", "items": {"type": "object", "required": ["name"], "properties": {"name": {"type": "string", "pattern": "^[A-Za-z_][A-Za-z0-9_]*$"}, "value": {"type": "string"}}}},
            "nodes": {"type": "array", "items": {"$ref": "#/$defs/node"}},
            "edges": {"type": "array", "items": {"$ref": "#/$defs/edge"}},
            "groups": {"type": "array"},
            "notes": {"type": "array"},
            "navigator": {"type": "array", "items": {"type": "string"}},
            "cache": {"type": "boolean"}
        },
        "$defs": {
            "node": {
                "type": "object",
                "required": ["id", "type"],
                "properties": {
                    "id": {"type": "string", "description": "Unique in the model; connections name it."},
                    "type": {"enum": types},
                    "name": {"type": "string"},
                    "x": {"type": "number"},
                    "y": {"type": "number"},
                    "params": {"type": "object"},
                    "comment": {"type": "string"},
                    "disabled": {"type": "boolean"}
                },
                "allOf": branches
            },
            "edge": {
                "type": "object",
                "required": ["from", "to"],
                "properties": {
                    "from": {"type": "array", "prefixItems": [{"type": "string", "description": "node id"}, {"type": "string", "description": "output port"}], "minItems": 2, "maxItems": 2},
                    "to": {"type": "array", "prefixItems": [{"type": "string", "description": "node id"}, {"type": "string", "description": "input port"}], "minItems": 2, "maxItems": 2},
                    "disabled": {"type": "boolean"}
                }
            }
        }
    })
}

// ---------- The check ----------

/// Something `og check` found: an error stops the model from running as meant, a warning
/// may be fine.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub error: bool,
    /// The id of the node it is about, if any.
    pub node: Option<String>,
    pub message: String,
}

/// Levenshtein distance, for the names that come closest to one that is not known.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.to_lowercase().chars().collect(), b.to_lowercase().chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let here = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = here;
        }
    }
    row[b.len()]
}

/// The known names nearest to `name`, at most three, as a phrase: "Did you mean A or B?".
fn nearest<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> String {
    let mut all: Vec<(usize, &str)> = known
        .map(|k| {
            (
                distance(name, k).min(if k.to_lowercase().contains(&name.to_lowercase()) || name.to_lowercase().contains(&k.to_lowercase()) {
                    2
                } else {
                    usize::MAX
                }),
                k,
            )
        })
        .collect();
    all.sort();
    let limit = (name.chars().count() / 2).max(2);
    let close: Vec<&str> = all.iter().filter(|(d, _)| *d <= limit).take(3).map(|(_, k)| *k).collect();
    match close.as_slice() {
        [] => String::new(),
        [one] => format!(" Did you mean {}?", one),
        [rest @ .., last] => format!(" Did you mean {} or {}?", rest.join(", "), last),
    }
}

/// The names of the variables a text uses as $(name).
fn variables_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("$(") {
        let after = &rest[at + 2..];
        match after.find(')') {
            Some(end) if is_variable_name(after[..end].trim()) => {
                out.push(after[..end].trim().to_string());
                rest = &after[end + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

fn strings_in(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings_in(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings_in(x, out)),
        _ => {}
    }
}

/// Settings a node may hold that are not in the form of its type: kept by the program for
/// how the node is shown, or from earlier versions.
const KEPT: &[&str] = &["hidden", "within", "min_x", "min_y", "max_x", "max_y", "port_order", "open"];

/// Settings of a type from earlier versions, which it still reads.
const OLD: &[(&str, &[&str])] = &[("Sorter", &["sort_by", "order"])];

/// Checks a model without running it. Gives what is wrong with what would be right: node
/// types, ports, connections, settings and their values, variables and how expressions are
/// written. The attributes of the streams are not known before a run, so they are not checked.
pub fn check(text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let err = |node: Option<&str>, message: String| Finding { error: true, node: node.map(String::from), message };
    let warn = |node: Option<&str>, message: String| Finding { error: false, node: node.map(String::from), message };
    let ws = match Workspace::from_json(text) {
        Ok(ws) => ws,
        Err(e) => {
            out.push(err(None, e.0));
            return out;
        }
    };
    let defined: Vec<&str> = ws.variables.iter().map(|v| v.name.as_str()).collect();
    let mut ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for n in &ws.nodes {
        let at = Some(n.id.as_str());
        if n.id.trim().is_empty() {
            out.push(err(None, format!("A node of type {} has no id.", n.kind)));
        } else if !ids.insert(n.id.as_str()) {
            out.push(err(at, format!("The id \"{}\" is used by more than one node. Each node needs an id of its own.", n.id)));
        }
        let Some(t) = node_type(&n.kind) else {
            out.push(err(
                at,
                format!(
                    "The node \"{}\" has the type \"{}\", which is not known.{} og nodes lists the types.",
                    n.id,
                    n.kind,
                    nearest(&n.kind, NODE_TYPES.iter().map(|t| t.name))
                ),
            ));
            continue;
        };
        for (key, value) in &n.params {
            let Some(spec) = t.params.iter().find(|p| p.key == key) else {
                let old = OLD.iter().any(|(ty, keys)| *ty == t.name && keys.contains(&key.as_str()));
                if !KEPT.contains(&key.as_str()) && !old {
                    let known: Vec<&str> = t.params.iter().map(|p| p.key).collect();
                    let hint = nearest(key, known.iter().copied());
                    out.push(err(
                        at,
                        format!("The node \"{}\" ({}) has no setting \"{}\".{} Its settings are: {}.", n.id, t.name, key, hint, known.join(", ")),
                    ));
                }
                continue;
            };
            check_value(&n.id, t, spec, value, &ws, &mut out);
        }
        let mut texts = Vec::new();
        for v in n.params.values() {
            strings_in(v, &mut texts);
        }
        for name in texts.iter().flat_map(|t| variables_in(t)) {
            if !defined.contains(&name.as_str()) && !out.iter().any(|f| f.message.contains(&format!("$({})", name))) {
                out.push(warn(
                    at,
                    format!(
                        "$({}) is used, but the model has no variable {}. Add it to variables, or give it with --var {}=… when the model runs.",
                        name, name, name
                    ),
                ));
            }
        }
    }
    // The connections: both ends must be there, with ports the nodes have.
    for e in &ws.edges {
        let (from, to) = (&e.from, &e.to);
        let ends = [(from, false), (to, true)];
        for ((id, port), input) in ends {
            let Some(n) = ws.nodes.iter().find(|n| &n.id == id) else {
                out.push(err(
                    None,
                    format!(
                        "A connection {} \"{}\", which is not in the model.{}",
                        if input { "goes to" } else { "comes from" },
                        id,
                        nearest(id, ws.nodes.iter().map(|n| n.id.as_str()))
                    ),
                ));

                continue;
            };
            let Some(t) = node_type(&n.kind) else { continue };
            let ports: Vec<String> = if input {
                t.inputs.iter().map(|p| p.to_string()).collect()
            } else if n.params.values().any(|v| v.as_str().is_some_and(|s| s.contains("$("))) && t.ports.is_some() {
                // Ports from settings with variables are known first when the model runs.
                continue;
            } else {
                t.outputs_for(&n.params)
            };
            if !ports.iter().any(|p| p == port) {
                let what = if input { "input" } else { "output" };
                let has = if ports.is_empty() { format!("it has no {} ports", what) } else { format!("it has {}", ports.join(" and ")) };
                out.push(err(Some(id), format!("The node \"{}\" ({}) has no {} port \"{}\"; {}.", id, t.name, what, port, has)));
            }
        }
    }
    // Inputs left open, on tools that need them.
    for n in &ws.nodes {
        let Some(t) = node_type(&n.kind) else { continue };
        if n.disabled || t.group == Group::Reader {
            continue;
        }
        for port in t.inputs {
            let connected = ws.edges.iter().any(|e| e.to.0 == n.id && e.to.1 == *port);
            if !connected
                && t.inputs.len() == 1
                && *port == "Input"
                && !matches!(t.name, "HttpCaller" | "SqlExecutor" | "ModelRunner" | "OsmDownloader" | "Viewer")
            {
                out.push(warn(Some(&n.id), format!("The input \"{}\" of \"{}\" ({}) is not connected, so the node gets nothing.", port, n.id, t.name)));
            }
        }
    }
    out
}

fn check_value(id: &str, t: &NodeType, spec: &ParamSpec, value: &Value, ws: &Workspace, out: &mut Vec<Finding>) {
    let at = Some(id.to_string());
    let bad = |what: String| Finding { error: true, node: at.clone(), message: format!("The setting \"{}\" of \"{}\" ({}) {}", spec.key, id, t.name, what) };
    let k = kind_of(&spec.kind);
    let scalar = matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null);
    match spec.kind {
        ParamKind::Choice(list) => {
            let v = match value {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                _ => {
                    out.push(bad(format!("must be one of {}.", list.iter().map(|c| c.0).collect::<Vec<_>>().join(", "))));
                    return;
                }
            };
            if !v.contains("$(") && !list.iter().any(|c| c.0 == v) {
                let values: Vec<&str> = list.iter().map(|c| c.0).collect();
                out.push(bad(format!("is \"{}\", which is not one of its choices: {}.{}", v, values.join(", "), nearest(&v, values.iter().copied()))));
            }
        }
        ParamKind::Expression(_) => {
            let Some(text) = value.as_str() else {
                if !scalar {
                    out.push(bad(format!("must be {}.", k.shape)));
                }
                return;
            };
            if text.trim().is_empty() {
                return;
            }
            // An address of a web request may be written as it is, without quotes.
            if t.name == "HttpCaller" && (text.trim().starts_with("http://") || text.trim().starts_with("https://")) {
                return;
            }
            let Ok(text) = crate::run::expand_variables(text, &ws.variables) else { return };
            if let Err(e) = crate::expr::check_syntax(&text) {
                out.push(bad(format!("is not a valid expression: {} The expression: {}", e.0, text)));
            }
        }
        ParamKind::AttributeTable(_) | ParamKind::NewAttributes(_) | ParamKind::Checks(_) | ParamKind::MapLayers(_) => {
            let ok = match value {
                Value::Array(a) => a.iter().all(|x| x.is_object()),
                Value::String(s) => s.trim().is_empty(),
                _ => false,
            };
            if !ok {
                out.push(bad(format!("must be {}.", k.shape)));
            }
            if let (ParamKind::NewAttributes(_), Value::Array(a)) = (&spec.kind, value) {
                for row in a {
                    if let Some(v) = row.get("value").and_then(|v| v.as_str()) {
                        if let Ok(v) = crate::run::expand_variables(v, &ws.variables) {
                            if let Err(e) = crate::expr::check_syntax(&v) {
                                out.push(bad(format!(
                                    "has the value \"{}\" for {}, which is not a valid expression: {}",
                                    v,
                                    row.get("name").and_then(|n| n.as_str()).unwrap_or("?"),
                                    e.0
                                )));
                            }
                        }
                    }
                }
            }
        }
        ParamKind::JsonPaths(..) | ParamKind::SortKeys(_) | ParamKind::Ranges(_) => {
            let ok = match value {
                Value::Array(a) => a.iter().all(|x| x.is_array()),
                Value::String(s) => s.trim().is_empty(),
                _ => false,
            };
            if !ok {
                out.push(bad(format!("must be {}.", k.shape)));
            }
        }
        ParamKind::Mapping(..) => {
            if !matches!(value, Value::Array(_) | Value::Object(_) | Value::String(_)) {
                out.push(bad(format!("must be {}.", k.shape)));
            }
        }
        ParamKind::Renames(_) | ParamKind::Schema(_) => {
            if !matches!(value, Value::Object(_) | Value::String(_)) {
                out.push(bad(format!("must be {}.", k.shape)));
            }
        }
        ParamKind::Values(..) | ParamKind::Attributes(_) => {
            let ok = match value {
                Value::Array(a) => a.iter().all(|x| x.is_string()),
                _ => scalar,
            };
            if !ok {
                out.push(bad(format!("must be {}.", k.shape)));
            }
        }
        _ => {
            if !scalar {
                out.push(bad(format!("must be {}, not a list or an object.", k.shape)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_is_described_and_the_schema_names_them_all() {
        let j = nodes_json();
        let types = j["node_types"].as_array().unwrap();
        assert_eq!(types.len(), NODE_TYPES.len());
        let tester = types.iter().find(|t| t["type"] == "Tester").unwrap();
        assert_eq!(tester["outputs"], json!(["Passed", "Failed"]));
        assert_eq!(tester["params"][0]["kind"], "expression");
        assert!(j["expressions"]["functions"].as_array().unwrap().iter().any(|f| f["name"] == "format_time"));
        let s = model_schema();
        assert_eq!(s["$defs"]["node"]["properties"]["type"]["enum"].as_array().unwrap().len(), NODE_TYPES.len());
        // The example in the reference is a model that passes the check.
        let example = serde_json::to_string(&j["model"]["example"]).unwrap();
        assert!(check(&example).iter().all(|f| !f.error), "{:?}", check(&example));
    }

    /// The reference and the schema in docs are those of this version, for assistants that
    /// read the repository without running og.
    #[test]
    fn the_reference_in_docs_is_up_to_date() {
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        for (file, value) in [("nodes.json", nodes_json()), ("model.schema.json", model_schema())] {
            let written = std::fs::read_to_string(docs.join(file)).unwrap_or_default();
            let now = serde_json::to_string_pretty(&value).unwrap() + "\n";
            assert!(written == now, "docs/{} is out of date. Write it again with: og nodes --json > docs/nodes.json, og schema > docs/model.schema.json", file);
        }
    }

    /// Every example model passes the check, so that they can be shown to an assistant as
    /// models that are right.
    #[test]
    fn every_example_passes_the_check() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut files = Vec::new();
        for dir in [root.clone(), root.join("apis")] {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                if e.path().extension().is_some_and(|x| x == "ographis") {
                    files.push(e.path());
                }
            }
        }
        assert!(files.len() > 40, "{}", files.len());
        // And the model in AGENTS.md.
        let agents = std::fs::read_to_string(root.join("../AGENTS.md")).unwrap();
        let model = agents.split("```json").nth(1).and_then(|b| b.split("```").next()).unwrap();
        assert!(check(model).is_empty(), "AGENTS.md: {:?}", check(model));
        for f in files {
            let found = check(&std::fs::read_to_string(&f).unwrap());
            assert!(found.is_empty(), "{}: {:?}", f.display(), found);
        }
    }

    #[test]
    fn the_check_says_what_is_wrong_and_what_would_be_right() {
        let model = r#"{"format": "ographis/1", "variables": [{"name": "min", "value": "10"}],
          "nodes": [
            {"id": "in", "type": "Reader", "params": {"path": "towns.csv"}},
            {"id": "t", "type": "Testr", "params": {}},
            {"id": "big", "type": "Tester", "params": {"tset": "population > $(min)"}},
            {"id": "sort", "type": "Sorter", "params": {"sort_keys": "population"}},
            {"id": "json", "type": "JsonExploder", "params": {"attribute": "_response", "keep": "maybe"}},
            {"id": "calc", "type": "AttributeCreator", "params": {"name": "x", "expression": "round(population / 1000"}},
            {"id": "out", "type": "Writer", "params": {"path": "$(folder)/big.gpkg"}}],
          "edges": [
            {"from": ["in", "Output"], "to": ["big", "Input"]},
            {"from": ["big", "Output"], "to": ["out", "Input"]},
            {"from": ["json", "Output"], "to": ["calc", "Input"]},
            {"from": ["in", "Output"], "to": ["calc", "Input"]},
            {"from": ["nobody", "Output"], "to": ["out", "Input"]}]}"#;
        let found = check(model);
        let all = found.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join("\n");
        let has = |s: &str| assert!(all.contains(s), "no \"{}\" in:\n{}", s, all);
        has("which is not known. Did you mean Tester");
        has("has no setting \"tset\". Did you mean test?");
        has("The setting \"sort_keys\" of \"sort\" (Sorter) must be a list of [attribute");
        has("is \"maybe\", which is not one of its choices: false, true.");
        has("is not a valid expression: A closing parenthesis is missing");
        has("$(folder) is used, but the model has no variable folder");
        has("The node \"big\" (Tester) has no output port \"Output\"; it has Passed and Failed.");
        has("The node \"json\" (JsonExploder) has no output port \"Output\"; it has Exploded and Other.");
        has("A connection comes from \"nobody\", which is not in the model.");
        assert!(!found.iter().any(|f| f.message.contains("$(min)")), "min is defined");
        // A model that cannot be read says so, and nothing more.
        let twice = check(r#"{"nodes": [{"id": "a", "type": "Tester"}, {"id": "a", "type": "Sorter"}]}"#);
        assert_eq!(twice.len(), 1);
        assert!(twice[0].error && twice[0].message.contains("same id"), "{:?}", twice);
    }
}
