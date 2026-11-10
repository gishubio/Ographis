//! Making nodes into a tool, and a tool back into nodes, added in 0.96. A tool is a node of
//! the type CustomTool with a model of its own inside, whose Tool input and Tool output nodes
//! are its ports. These are changes to a model as a whole, so that the program and tests
//! make them the same way.

use crate::model::{GbError, Result};
use crate::run::{Edge, GroupBox, Node, Workspace};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// About the size of a node on the canvas, for placing what is new.
const NODE_W: f64 = 190.0;
const NODE_H: f64 = 110.0;
const STEP: f64 = 260.0;

fn node(id: &str, kind: &str, name: &str, x: f64, y: f64, params: Value) -> Node {
    serde_json::from_value(json!({"id": id, "type": kind, "name": name, "x": x, "y": y, "params": params})).expect("a node")
}

/// An id that is not among `taken`, made from `wish`, and then taken.
fn unique(wish: &str, taken: &mut HashSet<String>) -> String {
    let base: String = wish.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    let base = base.trim_matches('_');
    let base = if base.is_empty() { "node" } else { base };
    let mut id = base.to_string();
    let mut k = 2;
    while taken.contains(&id) {
        id = format!("{}_{}", base, k);
        k += 1;
    }
    taken.insert(id.clone());
    id
}

/// A name that is not among `taken`, made from `wish`, and then taken.
fn unique_name(wish: &str, taken: &mut Vec<String>) -> String {
    let mut name = wish.to_string();
    let mut k = 2;
    while taken.contains(&name) {
        name = format!("{} {}", wish, k);
        k += 1;
    }
    taken.push(name.clone());
    name
}

/// The name of a port on the tool for a stream from a node: the name of the node, and the
/// port when it is not the usual one.
fn port_name(n: &Node, port: &str) -> String {
    if port == "Output" || port == "Input" {
        n.label().to_string()
    } else {
        format!("{} {}", n.label(), port)
    }
}

/// Makes the nodes with the ids given into one tool with the name given. What came into them
/// from other nodes comes in on the ports of the tool, and what went out from them to other
/// nodes goes out on its ports. Gives the model and the id of the tool.
pub fn make_tool(ws: &Workspace, ids: &[String], name: &str) -> Result<(Workspace, String)> {
    let inside: HashSet<&str> = ids.iter().map(String::as_str).filter(|id| ws.nodes.iter().any(|n| n.id == *id)).collect();
    if inside.is_empty() {
        return Err(GbError("Select the nodes to make into a tool first.".into()));
    }
    let of = |id: &str| ws.nodes.iter().find(|n| n.id == id);
    let mut inner_nodes: Vec<Node> = ws.nodes.iter().filter(|n| inside.contains(n.id.as_str())).cloned().collect();
    let (min_x, max_x) = inner_nodes.iter().fold((f64::MAX, f64::MIN), |(a, b), n| (a.min(n.x), b.max(n.x)));
    let (min_y, max_y) = inner_nodes.iter().fold((f64::MAX, f64::MIN), |(a, b), n| (a.min(n.y), b.max(n.y)));
    let mut taken: HashSet<String> = inner_nodes.iter().map(|n| n.id.clone()).collect();
    let mut inner_edges = Vec::new();
    let mut outer_edges = Vec::new();
    // One port for each stream that comes in or goes out: by the node and port it is from.
    let mut ins: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    let mut outs: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    let (mut in_names, mut out_names) = (Vec::new(), Vec::new());
    let tool_id = unique(
        if name.trim().is_empty() { "tool" } else { name },
        &mut ws.nodes.iter().filter(|n| !inside.contains(n.id.as_str())).map(|n| n.id.clone()).collect(),
    );
    for e in &ws.edges {
        let (a, b) = (inside.contains(e.from.0.as_str()), inside.contains(e.to.0.as_str()));
        match (a, b) {
            (true, true) => inner_edges.push(e.clone()),
            (false, false) => outer_edges.push(e.clone()),
            (false, true) => {
                let key = (e.from.0.clone(), e.from.1.clone());
                if !ins.contains_key(&key) {
                    let wish = of(&e.from.0).map_or_else(|| "Input".to_string(), |n| port_name(n, &e.from.1));
                    let port = unique_name(&wish, &mut in_names);
                    let id = unique("in", &mut taken);
                    let y = of(&e.to.0).map_or(min_y, |n| n.y);
                    inner_nodes.push(node(&id, "ToolInput", &port, min_x - STEP, y, json!({"port": port})));
                    let mut outer = e.clone();
                    outer.to = (tool_id.clone(), port.clone());
                    outer.points.clear();
                    outer_edges.push(outer);
                    ins.insert(key.clone(), (id, port));
                }
                let (id, _) = &ins[&key];
                inner_edges.push(Edge { from: (id.clone(), "Output".into()), to: e.to.clone(), disabled: e.disabled, points: Vec::new() });
            }
            (true, false) => {
                let key = (e.from.0.clone(), e.from.1.clone());
                if !outs.contains_key(&key) {
                    let wish = of(&e.from.0).map_or_else(|| "Output".to_string(), |n| port_name(n, &e.from.1));
                    let port = unique_name(&wish, &mut out_names);
                    let id = unique("out", &mut taken);
                    let y = of(&e.from.0).map_or(min_y, |n| n.y);
                    inner_nodes.push(node(&id, "ToolOutput", &port, max_x + STEP, y, json!({"port": port})));
                    inner_edges.push(Edge { from: e.from.clone(), to: (id.clone(), "Input".into()), disabled: false, points: Vec::new() });
                    outs.insert(key.clone(), (id, port));
                }
                let (_, port) = &outs[&key];
                let mut outer = e.clone();
                outer.from = (tool_id.clone(), port.clone());
                outer.points.clear();
                outer_edges.push(outer);
            }
        }
    }
    // A tool that nothing comes into or goes out of still has a port of each.
    let mut inner = ws.clone();
    inner.name = name.to_string();
    inner.description = String::new();
    inner.variables = Vec::new();
    inner.groups = ws.groups.iter().filter(|g| inner_nodes.iter().any(|n| inside.contains(n.id.as_str()) && in_box(n, g))).cloned().collect();
    inner.notes = Vec::new();
    inner.navigator = ws.navigator.iter().filter(|id| inside.contains(id.as_str())).cloned().collect();
    inner.nodes = inner_nodes;
    inner.edges = inner_edges;
    let mut outer = ws.clone();
    outer.nodes.retain(|n| !inside.contains(n.id.as_str()));
    outer.navigator.retain(|id| !inside.contains(id.as_str()));
    let keep_groups: Vec<GroupBox> = inner.groups.clone();
    outer.groups.retain(|g| !keep_groups.contains(g));
    let (x, y) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    let label = if name.trim().is_empty() { "Tool" } else { name.trim() };
    outer.nodes.push(node(&tool_id, "CustomTool", label, x, y, json!({"tool": inner_value(&inner), "loop": "once"})));
    outer.edges = outer_edges;
    Ok((outer, tool_id))
}

/// Whether the middle of a node lies in a group box.
fn in_box(n: &Node, g: &GroupBox) -> bool {
    let (cx, cy) = (n.x + NODE_W / 2.0, n.y + NODE_H / 2.0);
    cx >= g.x && cx <= g.x + g.w && cy >= g.y && cy <= g.y + g.h
}

/// The model inside a tool as it is kept in the setting "tool": without what belongs to the
/// model as a whole.
pub fn inner_value(inner: &Workspace) -> Value {
    let mut v = serde_json::to_value(inner).unwrap_or(Value::Null);
    if let Value::Object(o) = &mut v {
        for key in ["format", "variables", "cache", "description"] {
            o.remove(key);
        }
    }
    v
}

/// Puts a changed model into the tool with the id given.
pub fn put_inside(outer: &mut Workspace, id: &str, inner: &Workspace) {
    if let Some(n) = outer.nodes.iter_mut().find(|n| n.id == id) {
        n.params.insert("tool".into(), inner_value(inner));
    }
}

/// Unpacks a tool into the nodes it is made of, in a group box with its name, connected as
/// the tool was.
pub fn unpack_tool(ws: &Workspace, id: &str) -> Result<Workspace> {
    let Some(tool) = ws.nodes.iter().find(|n| n.id == id && n.kind == "CustomTool") else {
        return Err(GbError("Only a tool made of other tools can be unpacked.".into()));
    };
    let looping = tool.params.get("loop").and_then(|v| v.as_str()).unwrap_or("once");
    if looping != "once" {
        return Err(GbError(
            "A tool that runs for each value or repeated cannot be unpacked, since its nodes would then run only once. Set it to run once first.".into(),
        ));
    }
    let Some(inner) = crate::nodes::inner_model(&tool.params) else {
        return Err(GbError("The tool has no model inside.".into()));
    };
    let port =
        |n: &Node, fallback: &str| n.params.get("port").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(fallback).to_string();
    let is_port = |n: &Node| n.kind == "ToolInput" || n.kind == "ToolOutput";
    let real: Vec<&Node> = inner.nodes.iter().filter(|n| !is_port(n)).collect();
    let mut out = ws.clone();
    out.nodes.retain(|n| n.id != id);
    let mut taken: HashSet<String> = out.nodes.iter().map(|n| n.id.clone()).collect();
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    for n in &real {
        ids.insert(n.id.clone(), unique(&n.id, &mut taken));
    }
    // The nodes come where the tool was, around its place.
    let (min_x, max_x) = real.iter().fold((f64::MAX, f64::MIN), |(a, b), n| (a.min(n.x), b.max(n.x)));
    let (min_y, max_y) = real.iter().fold((f64::MAX, f64::MIN), |(a, b), n| (a.min(n.y), b.max(n.y)));
    let (dx, dy) = if real.is_empty() { (0.0, 0.0) } else { (tool.x - (min_x + max_x) / 2.0, tool.y - (min_y + max_y) / 2.0) };
    for n in &real {
        let mut m = (*n).clone();
        m.id = ids[&n.id].clone();
        m.x += dx;
        m.y += dy;
        out.nodes.push(m);
    }
    let into_tool: Vec<&Edge> = ws.edges.iter().filter(|e| e.to.0 == id).collect();
    let from_tool: Vec<&Edge> = ws.edges.iter().filter(|e| e.from.0 == id).collect();
    out.edges.retain(|e| e.to.0 != id && e.from.0 != id);
    let kind_of = |nid: &str| inner.nodes.iter().find(|n| n.id == nid);
    for e in &inner.edges {
        let (from, to) = (kind_of(&e.from.0), kind_of(&e.to.0));
        let (Some(from), Some(to)) = (from, to) else { continue };
        match (from.kind.as_str(), to.kind.as_str()) {
            ("ToolInput", "ToolOutput") => {
                // Straight through: what came in on one port goes out on the other.
                let (p_in, p_out) = (port(from, "Input"), port(to, "Output"));
                for a in into_tool.iter().filter(|a| a.to.1 == p_in) {
                    for b in from_tool.iter().filter(|b| b.from.1 == p_out) {
                        out.edges.push(Edge { from: a.from.clone(), to: b.to.clone(), disabled: a.disabled || b.disabled, points: Vec::new() });
                    }
                }
            }
            ("ToolInput", _) => {
                let p = port(from, "Input");
                for a in into_tool.iter().filter(|a| a.to.1 == p) {
                    out.edges.push(Edge {
                        from: a.from.clone(),
                        to: (ids[&to.id].clone(), e.to.1.clone()),
                        disabled: a.disabled || e.disabled,
                        points: Vec::new(),
                    });
                }
            }
            (_, "ToolOutput") => {
                let p = port(to, "Output");
                for b in from_tool.iter().filter(|b| b.from.1 == p) {
                    out.edges.push(Edge {
                        from: (ids[&from.id].clone(), e.from.1.clone()),
                        to: b.to.clone(),
                        disabled: b.disabled || e.disabled,
                        points: Vec::new(),
                    });
                }
            }
            _ => {
                let mut m = e.clone();
                m.from.0 = ids[&from.id].clone();
                m.to.0 = ids[&to.id].clone();
                for p in &mut m.points {
                    p[0] += dx;
                    p[1] += dy;
                }
                out.edges.push(m);
            }
        }
    }
    for g in &inner.groups {
        let mut g = g.clone();
        g.x += dx;
        g.y += dy;
        out.groups.push(g);
    }
    for n in &inner.notes {
        let mut n = n.clone();
        n.x += dx;
        n.y += dy;
        out.notes.push(n);
    }
    for nid in &inner.navigator {
        if let Some(new) = ids.get(nid) {
            out.navigator.push(new.clone());
        }
    }
    // A box with the name of the tool holds the nodes together, only for the eye.
    if !real.is_empty() {
        let pad = 30.0;
        out.groups.push(GroupBox {
            name: tool.label().to_string(),
            x: min_x + dx - pad,
            y: min_y + dy - pad - 24.0,
            w: max_x - min_x + NODE_W + 2.0 * pad,
            h: max_y - min_y + NODE_H + 2.0 * pad + 24.0,
            color: String::new(),
            locked: true,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Workspace {
        Workspace::from_json(
            r#"{"format":"ographis/1","name":"m","nodes":[
            {"id":"a","type":"Reader","name":"Points","x":0,"y":0,"params":{"path":"p.csv"}},
            {"id":"b","type":"AttributeCreator","x":260,"y":0,"params":{"attributes":[["k","1"]]}},
            {"id":"c","type":"Tester","x":520,"y":0,"params":{"test":"k = 1"}},
            {"id":"d","type":"Writer","x":780,"y":0,"params":{"path":"out/x.gpkg"}}],
            "edges":[{"from":["a","Output"],"to":["b","Input"]},{"from":["b","Output"],"to":["c","Input"]},
            {"from":["c","Passed"],"to":["d","Input"]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn nodes_become_a_tool_and_come_back() {
        let ws = model();
        let (made, id) = make_tool(&ws, &["b".into(), "c".into()], "Keep ones").unwrap();
        assert_eq!(made.nodes.len(), 3);
        let tool = made.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(tool.kind, "CustomTool");
        let inner = crate::nodes::inner_model(&tool.params).unwrap();
        assert_eq!(inner.nodes.iter().filter(|n| n.kind == "ToolInput").count(), 1);
        assert_eq!(inner.nodes.iter().filter(|n| n.kind == "ToolOutput").count(), 1);
        assert!(made.edges.iter().any(|e| e.from.0 == "a" && e.to == (id.clone(), "Points".to_string())));
        assert!(made.edges.iter().any(|e| e.from == (id.clone(), "Test Passed".to_string()) && e.to.0 == "d"));
        // The ports of the node follow from the model inside.
        let ty = crate::nodes::node_type("CustomTool").unwrap();
        assert_eq!(ty.inputs_for(&tool.params), vec!["Points"]);
        let back = unpack_tool(&made, &id).unwrap();
        assert_eq!(back.nodes.len(), 4);
        let pairs: HashSet<(String, String)> = back.edges.iter().map(|e| (e.from.0.clone(), e.to.0.clone())).collect();
        assert_eq!(pairs, HashSet::from([("a".into(), "b".into()), ("b".into(), "c".into()), ("c".into(), "d".into())]));
        assert_eq!(back.groups.last().unwrap().name, "Keep ones");
    }

    #[test]
    fn a_tool_that_loops_is_not_unpacked() {
        let (mut made, id) = make_tool(&model(), &["b".into()], "One").unwrap();
        made.nodes.iter_mut().find(|n| n.id == id).unwrap().params.insert("loop".into(), json!("repeat"));
        assert!(unpack_tool(&made, &id).is_err());
    }
}
