//! Tools made of other tools, added in 0.96: a group of nodes made into one node, with a
//! model of its own inside. Tool input and Tool output nodes in that model are the ports of
//! the tool. It runs once, once for each value of an attribute (For each), or a number of
//! times with what comes out going in again (Repeat).

use super::*;
use crate::model::{err, fmt_count, Value};
use crate::run::{run_opts, RunOptions, Workspace};
use std::sync::Mutex;

/// The streams given to the Tool input nodes of a tool that runs, by a key in their settings.
static GIVEN: Mutex<Option<HashMap<u64, Arc<Table>>>> = Mutex::new(None);
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Port names live as long as the program, as the ports of the other tools do. Each name is
/// kept once.
fn interned(name: &str) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().expect("names");
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let n: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(n);
    n
}

/// The model inside a tool.
pub fn inner_model(params: &Params) -> Option<Workspace> {
    params.get("tool").and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// The name of the port of a Tool input or Tool output node.
fn port_of(n: &crate::run::Node, fallback: &str) -> String {
    n.params.get("port").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(fallback).to_string()
}

/// The input ports of a tool: the names of its Tool input nodes, in the order they lie from
/// the top.
pub fn input_ports(params: &Params) -> Vec<&'static str> {
    let Some(ws) = inner_model(params) else { return vec!["Input"] };
    let mut ins: Vec<&crate::run::Node> = ws.nodes.iter().filter(|n| n.kind == "ToolInput").collect();
    ins.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut out: Vec<&'static str> = Vec::new();
    for n in ins {
        let name = interned(&port_of(n, "Input"));
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn output_ports(p: &P) -> Vec<String> {
    let Some(ws) = inner_model(p.values) else { return vec!["Output".into()] };
    let mut outs: Vec<&crate::run::Node> = ws.nodes.iter().filter(|n| n.kind == "ToolOutput").collect();
    outs.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut names: Vec<String> = Vec::new();
    for n in outs {
        let name = port_of(n, "Output");
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

const TOOL_INPUT_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "port",
    label: "Port",
    kind: ParamKind::Text,
    default: "Input",
    hint: "The name of the port on the tool, which the stream comes in on",
}];
const TOOL_OUTPUT_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "port",
    label: "Port",
    kind: ParamKind::Text,
    default: "Output",
    hint: "The name of the port on the tool, which the stream goes out on",
}];

fn run_tool_input(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let given = p.values.get("_given").and_then(|v| v.as_u64()).and_then(|k| GIVEN.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&k).cloned())));
    match given {
        Some(t) => Ok(vec![("Output".into(), (*t).clone())]),
        None => {
            ctx.notes.info("nothing comes in when the model of a tool runs on its own");
            Ok(vec![("Output".into(), Table::default())])
        }
    }
}

fn run_tool_output(_p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    Ok(vec![("Output".into(), input(inp, "Input").clone())])
}

const LOOPS: &[(&str, &str)] = &[
    ("once", "Once"),
    ("each", "For each value of an attribute: once for the features with each value, and all that comes out together"),
    ("repeat", "Repeat: so many times, what comes out going in again"),
];

const CUSTOM_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "tool", label: "The model inside", kind: ParamKind::Model, default: "", hint: "Open it with a double-click on the node, or Enter" },
    ParamSpec { key: "loop", label: "Runs", kind: ParamKind::Choice(LOOPS), default: "once", hint: "" },
    ParamSpec {
        key: "each_by",
        label: "For each value of",
        kind: ParamKind::Text,
        default: "",
        hint: "An attribute of the stream on the first port. Inside, $(value) is the value of the round",
    },
    ParamSpec {
        key: "times",
        label: "Times",
        kind: ParamKind::Text,
        default: "3",
        hint: "What comes out on a port goes in again on the input port of the same name. Inside, $(round) is the round, from 1",
    },
];

/// Runs the model of a tool once, with the given streams on its Tool input nodes and its own
/// variables and those of the model it is in. Gives what came out of each Tool output node.
fn run_inner(ws: &Workspace, given: &HashMap<&'static str, Arc<Table>>, extra: &[(&str, String)], ctx: &mut NodeCtx) -> Result<HashMap<String, Table>> {
    let mut ws = ws.clone();
    // The variables of the outer model count inside too, unless the tool has its own.
    for v in ctx.variables {
        if !ws.variables.iter().any(|w| w.name == v.name) {
            ws.variables.push(v.clone());
        }
    }
    for (name, value) in extra {
        ws.set_variable(name, value);
    }
    let mut keys = Vec::new();
    {
        let mut g = GIVEN.lock().map_err(|_| crate::model::GbError("The tool could not be started.".into()))?;
        let map = g.get_or_insert_with(HashMap::new);
        for n in ws.nodes.iter_mut().filter(|n| n.kind == "ToolInput") {
            let port = port_of(n, "Input");
            let key = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let table = given.iter().find(|(p, _)| **p == port).map(|(_, t)| t.clone()).unwrap_or_default();
            map.insert(key, table);
            n.params.insert("_given".into(), serde_json::json!(key));
            keys.push(key);
        }
    }
    let r = run_opts(&ws, ctx.backend, ctx.base_dir, &mut |_| {}, RunOptions { stop: Some(ctx.gate.stop), pause: Some(ctx.gate.pause), ..Default::default() });
    if let Ok(mut g) = GIVEN.lock() {
        if let Some(map) = g.as_mut() {
            for k in keys {
                map.remove(&k);
            }
        }
    }
    let label = |id: &str| ws.nodes.iter().find(|n| n.id == id).map_or(id.to_string(), |n| n.label().to_string());
    if let Some((id, e)) = ws
        .nodes
        .iter()
        .find_map(|n| r.nodes.get(&n.id).and_then(|res| res.error.clone().filter(|_| res.status == crate::run::Status::Error).map(|e| (n.id.clone(), e))))
    {
        return err(format!("{} inside the tool: {}", label(&id), e));
    }
    for n in &ws.nodes {
        if let Some(res) = r.nodes.get(&n.id) {
            for w in &res.warnings {
                ctx.notes.warn(format!("{} inside the tool: {}", n.label(), w));
            }
        }
    }
    let mut out = HashMap::new();
    for n in ws.nodes.iter().filter(|n| n.kind == "ToolOutput") {
        let port = port_of(n, "Output");
        if let Some(t) = r.output(&n.id, "Output") {
            match out.get_mut(&port) {
                None => {
                    out.insert(port, (**t).clone());
                }
                Some(have) => {
                    let both = merge_tables(vec![Arc::new(std::mem::take(have)), t.clone()], ctx.backend, "outputs", &mut ctx.notes)?;
                    *have = (*both).clone();
                }
            }
        }
    }
    Ok(out)
}

fn run_custom(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let Some(ws) = inner_model(p.values) else { return err("The tool has no model inside. Make one by choosing nodes and Make into a tool.") };
    let ins = input_ports(p.values);
    let outs = output_ports(p);
    if outs.is_empty() {
        return err("The model inside has no Tool output, so nothing comes out of the tool.");
    }
    let given: HashMap<&'static str, Arc<Table>> = ins.iter().map(|port| (*port, inp.get(port).cloned().unwrap_or_default())).collect();
    let result: HashMap<String, Table> = match p.get("loop").as_deref() {
        Some("each") => {
            let first = *ins.first().ok_or_else(|| crate::model::GbError("For each needs a Tool input in the model inside.".into()))?;
            let attr = p.need("each_by", "Say which attribute to run once for each value of.")?;
            let t = given[first].clone();
            let i = t.need_field(&attr)?;
            // The values in the order they first come.
            let mut values: Vec<Value> = Vec::new();
            for f in &t.features {
                if !values.contains(&f.attrs[i]) {
                    values.push(f.attrs[i].clone());
                }
            }
            let mut all: HashMap<String, Vec<Arc<Table>>> = HashMap::new();
            for (k, v) in values.iter().enumerate() {
                (ctx.progress)("rounds", k as u64, Some(values.len() as u64));
                let mut part = t.like();
                part.features = t.features.iter().filter(|f| f.attrs[i] == *v).cloned().collect();
                let mut g = given.clone();
                g.insert(first, Arc::new(part));
                let round = run_inner(&ws, &g, &[("value", v.to_text()), ("round", (k + 1).to_string())], ctx)
                    .map_err(|e| crate::model::GbError(format!("For {} = {}: {}", attr, v.to_text(), e.0)))?;
                for (port, table) in round {
                    all.entry(port).or_default().push(Arc::new(table));
                }
                if !ctx.gate.going() {
                    return err(crate::model::STOPPED);
                }
            }
            ctx.notes.info(format!("ran once for each of {} values of {}", fmt_count(values.len() as u64), attr));
            let mut out = HashMap::new();
            for (port, tables) in all {
                out.insert(port, (*merge_tables(tables, ctx.backend, "rounds", &mut ctx.notes)?).clone());
            }
            out
        }
        Some("repeat") => {
            let times = super::geom_transform::number_param(p, "times", "Times")?.unwrap_or(3.0).clamp(1.0, 10_000.0) as usize;
            let mut g = given.clone();
            let mut last = HashMap::new();
            for k in 0..times {
                (ctx.progress)("rounds", k as u64, Some(times as u64));
                last = run_inner(&ws, &g, &[("round", (k + 1).to_string())], ctx).map_err(|e| crate::model::GbError(format!("In round {}: {}", k + 1, e.0)))?;
                // What came out on a port goes in again on the input of the same name.
                for port in &ins {
                    if let Some(t) = last.get(*port) {
                        g.insert(*port, Arc::new(t.clone()));
                    }
                }
                if !ctx.gate.going() {
                    return err(crate::model::STOPPED);
                }
            }
            if !ins.iter().any(|i| outs.iter().any(|o| o == i)) {
                ctx.notes.warn("No output of the tool has the name of an input, so each round started from the same streams. Name a Tool output as a Tool input to feed it back.");
            }
            ctx.notes.info(format!("ran {} times", times));
            last
        }
        _ => run_inner(&ws, &given, &[], ctx)?,
    };
    Ok(outs
        .into_iter()
        .map(|port| {
            let t = result.get(&port).cloned().unwrap_or_default();
            (std::borrow::Cow::Owned(port), t)
        })
        .collect())
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "CustomTool",
        title: "Tool",
        hidden: false,
        group: Group::Transformer,
        category: "Workflow",
        desc: "A tool of your own: nodes made into one, with a model inside that a double-click opens. Its ports are the Tool input and Tool output nodes inside. It runs once, once for each value of an attribute (For each, with $(value) inside), or a number of times with what comes out going in again (Repeat, with $(round) inside). Unpack it to make it nodes in a group again.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: Some(output_ports),
        params: CUSTOM_PARAMS,
        run: run_custom,
        summary: |p| match p.get("loop").as_deref() {
            Some("each") => format!("for each {}", p.get("each_by").unwrap_or_default()),
            Some("repeat") => format!("{} times", p.get("times").unwrap_or_else(|| "3".into())),
            _ => inner_model(p.values).map(|w| format!("{} nodes inside", w.nodes.iter().filter(|n| n.kind != "ToolInput" && n.kind != "ToolOutput").count())).unwrap_or_default(),
        },
    },
    NodeType {
        name: "ToolInput",
        title: "Tool input",
        hidden: false,
        group: Group::Transformer,
        category: "Workflow",
        desc: "In the model of a tool: a port of the tool that a stream comes in on. Its name is the name of the port.",
        inputs: &[],
        outputs: &["Output"],
        ports: None,
        params: TOOL_INPUT_PARAMS,
        run: run_tool_input,
        summary: |p| p.get("port").unwrap_or_else(|| "Input".into()),
    },
    NodeType {
        name: "ToolOutput",
        title: "Tool output",
        hidden: false,
        group: Group::Transformer,
        category: "Workflow",
        desc: "In the model of a tool: a port of the tool that a stream goes out on. Its name is the name of the port.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: TOOL_OUTPUT_PARAMS,
        run: run_tool_output,
        summary: |p| p.get("port").unwrap_or_else(|| "Output".into()),
    },
];
