//! The workspace format and the execution. A workspace is a JSON file with nodes and connections.

use crate::io::{Backend, Notes};
use crate::model::{fmt_count, fmt_duration, GbError, Result, Table};
use crate::nodes::{self, node_type, Inputs, NodeCtx, Params, P};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub const FORMAT: &str = "ographis/1";
/// The format name from before the program was renamed. Such files are still read.
/// Earlier names of the program, whose model files are read as they are.
const OLD_FORMAT_PREFIXES: &[&str] = &["geobobbo/", "flodesbanken/"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub name: String,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// Named boxes that hold nodes together on the canvas. They do not affect the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupBox>,
    /// Free-standing notes on the canvas. They do not affect the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// The ids of readers and writers in the order the user has put them in the
    /// navigator. Those that are not listed come after, by name. Does not affect the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub navigator: Vec<String>,
    /// Named values that the settings of the nodes use as $(name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// Whether the results of the latest run are kept for running parts of the model.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache: bool,
}

/// A named value of a model. Wherever a setting says $(name), the value is put in its place
/// when the model runs: in paths, expressions, values and names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Variable {
    pub name: String,
    #[serde(default)]
    pub value: String,
}

/// Whether a text can be the name of a variable: letters, digits and _, not starting with a digit.
pub fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Puts the values of the variables in place of $(name) in a text. A name that is not
/// defined is an error that names it.
pub fn expand_variables(text: &str, vars: &[Variable]) -> Result<String> {
    if !text.contains("$(") {
        return Ok(text.to_string());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("$(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        match after.find(')') {
            Some(end) if is_variable_name(after[..end].trim()) => {
                let name = after[..end].trim();
                match vars.iter().find(|v| v.name == name) {
                    Some(v) => out.push_str(&v.value),
                    None => {
                        let known: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
                        let hint = if known.is_empty() { "The model has no variables.".to_string() } else { format!("The model has: {}.", known.join(", ")) };
                        return Err(GbError(format!("The variable {} is not defined. {} Variables are set in the Model menu.", name, hint)));
                    }
                }
                rest = &after[end + 1..];
            }
            // Not a variable, such as a $( in a text: kept as it is.
            _ => {
                out.push_str("$(");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// The settings of a node with the variables put in, everywhere a text is found.
pub fn expand_params(params: &Params, vars: &[Variable]) -> Result<Params> {
    // An input that reads within the rectangle of the variables gets them as its sides.
    if params.get("within").and_then(|v| v.as_str()) == Some("variables") {
        let mut p = params.clone();
        for (k, v) in [("min_x", "xmin"), ("min_y", "ymin"), ("max_x", "xmax"), ("max_y", "ymax")] {
            p.insert(k.to_string(), serde_json::Value::String(format!("$({})", v)));
        }
        return expand_params_as_they_are(&p, vars);
    }
    expand_params_as_they_are(params, vars)
}

fn expand_params_as_they_are(params: &Params, vars: &[Variable]) -> Result<Params> {
    fn walk(v: &serde_json::Value, vars: &[Variable]) -> Result<serde_json::Value> {
        Ok(match v {
            serde_json::Value::String(s) => serde_json::Value::String(expand_variables(s, vars)?),
            serde_json::Value::Array(a) => serde_json::Value::Array(a.iter().map(|x| walk(x, vars)).collect::<Result<_>>()?),
            serde_json::Value::Object(o) => serde_json::Value::Object(o.iter().map(|(k, x)| Ok((k.clone(), walk(x, vars)?))).collect::<Result<_>>()?),
            other => other.clone(),
        })
    }
    params.iter().map(|(k, v)| Ok((k.clone(), walk(v, vars)?))).collect()
}

/// Is a variable by this name a password or something like it, which is not shown in logs?
pub fn is_secret(name: &str) -> bool {
    let n = name.to_lowercase();
    ["password", "passwd", "pwd", "secret", "token", "lösenord", "losenord", "apikey", "api_key"].iter().any(|w| n.contains(w))
}

impl Workspace {
    /// Sets a variable, or adds it if the model does not have it. For values given from
    /// the outside, such as on the command line.
    pub fn set_variable(&mut self, name: &str, value: &str) {
        match self.variables.iter_mut().find(|v| v.name == name) {
            Some(v) => v.value = value.to_string(),
            None => self.variables.push(Variable { name: name.to_string(), value: value.to_string() }),
        }
    }
}

/// A named box on the canvas. Position and size are in canvas coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupBox {
    #[serde(default)]
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// One of the colour names the program knows. Empty means the default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
    /// Whether the nodes inside follow when the box is moved.
    #[serde(default = "yes")]
    pub locked: bool,
}

/// A free-standing note on the canvas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// The width of a note that has been resized, and the height of its text area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

fn yes() -> bool {
    true
}

fn default_format() -> String {
    FORMAT.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    /// Position on the canvas. Does not affect the run.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub params: Params,
    /// A comment about the node, shown with it on the canvas. Does not affect the run.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Where the box with the comment is: x and y from the upper left corner of the node,
    /// width and height. Without it the box stands above the node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_box: Option<[f64; 4]>,
    /// A disabled node does not run and lets nothing through.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Node {
    /// The name of the node, or the name of its tool if it has none of its own.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            crate::nodes::node_type(&self.kind).map_or(&self.kind, |t| t.title)
        } else {
            &self.name
        }
    }
}

/// The name a node was given by the program, in the words used today. Before the tools were
/// named by what they do a new node was called "Sorter" or "Sorter_2", and now "Sort" or
/// "Sort 2". Any other name is one the user chose, and gives None.
pub fn default_name_today(kind: &str, name: &str) -> Option<String> {
    let t = crate::nodes::node_type(kind)?;
    let number = |rest: &str, sep: char| rest.strip_prefix(sep).filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).map(str::to_string);
    // The name of the type with a space is how 0.52 numbered inputs and outputs.
    for (base, sep) in [(t.title, ' '), (t.name, '_'), (t.name, ' ')] {
        let Some(rest) = name.strip_prefix(base) else { continue };
        if rest.is_empty() {
            return Some(t.title.to_string());
        }
        if let Some(k) = number(rest, sep) {
            return Some(format!("{} {}", t.title, k));
        }
    }
    None
}

/// A connection from an output port to an input port: `[node id, port name]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub from: (String, String),
    pub to: (String, String),
    /// A disabled connection lets nothing through, as if it were not there, but stays in
    /// the model to be turned on again.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Points the connection is drawn through on the canvas, so that it can go around
    /// things. Only for how it looks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f64; 2]>,
}

impl Edge {
    pub fn new(from: (String, String), to: (String, String)) -> Edge {
        Edge { from, to, disabled: false, points: Vec::new() }
    }
}

impl Workspace {
    pub fn from_json(text: &str) -> Result<Workspace> {
        let ws: Workspace = serde_json::from_str(text).map_err(|e| GbError(format!("The model file cannot be read: {}", e)))?;
        let mut ws = ws;
        if OLD_FORMAT_PREFIXES.iter().any(|p| ws.format.starts_with(p)) {
            ws.format = FORMAT.to_string();
        }
        if !ws.format.starts_with("ographis/") {
            return Err(GbError(format!("Unknown format: {}. Expected {}.", ws.format, FORMAT)));
        }
        for n in &mut ws.nodes {
            if let Some(name) = default_name_today(&n.kind, &n.name) {
                n.name = name;
            }
            // Before 0.75 an input could be bound to the variables xmin, ymin, xmax and ymax
            // by a choice of its own. Now the sides of the rectangle name them.
            // Before 0.75 the tags of OpenStreetMap were all attributes or none.
            if n.params.get("osm_tags").and_then(|v| v.as_str()) == Some("true") {
                n.params.insert("osm_tags".into(), serde_json::json!("common"));
            }
            if n.kind == "Reader" && n.params.get("within").and_then(|v| v.as_str()) == Some("variables") {
                n.params.insert("within".into(), serde_json::json!("rectangle"));
                for (k, v) in [("min_x", "$(xmin)"), ("min_y", "$(ymin)"), ("max_x", "$(xmax)"), ("max_y", "$(ymax)")] {
                    let empty = n.params.get(k).and_then(|x| x.as_str()).is_none_or(|x| x.trim().is_empty());
                    if empty {
                        n.params.insert(k.into(), serde_json::json!(v));
                    }
                }
            }
        }
        // Before 0.77 Join by location took only polygons, on a port of that name, and placed
        // a line or an area by one point on it.
        let joiners: Vec<String> = ws.nodes.iter().filter(|n| n.kind == "SpatialJoiner").map(|n| n.id.clone()).collect();
        for n in ws.nodes.iter_mut().filter(|n| n.kind == "SpatialJoiner") {
            n.params.entry("relation").or_insert_with(|| serde_json::json!("point"));
        }
        for e in &mut ws.edges {
            if e.to.1 == "Polygons" && joiners.contains(&e.to.0) {
                e.to.1 = "Join".into();
            }
        }
        let mut seen = std::collections::HashSet::new();
        for n in &ws.nodes {
            if !seen.insert(n.id.as_str()) {
                return Err(GbError(format!("Two nodes have the same id: {}.", n.id)));
            }
        }
        Ok(ws)
    }
    pub fn load(path: &Path) -> Result<Workspace> {
        let text = std::fs::read_to_string(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
        Workspace::from_json(&text)
    }
    pub fn to_json(&self) -> String {
        // The settings of each node in alphabetical order, so that a model saved twice is
        // the same file, whatever order the settings were made in.
        let mut ws = self.clone();
        for n in &mut ws.nodes {
            n.params.sort_keys();
        }
        serde_json::to_string_pretty(&ws).expect("a model can always be written as JSON")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Ok,
    Warn,
    Error,
    Skipped,
    /// The node is switched off by the user.
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: Level,
    pub node: Option<String>,
    pub msg: String,
}

#[derive(Debug, Clone, Default)]
pub struct NodeResult {
    pub status: Status,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    /// What came out of each output port. Kept so that it can be inspected afterwards.
    pub outputs: HashMap<String, Arc<Table>>,
    /// What came in on each input port, after several connections were merged.
    pub inputs: HashMap<String, Arc<Table>>,
    /// The files a writer wrote.
    pub files: Vec<PathBuf>,
    /// The files a reader read.
    pub sources: Vec<PathBuf>,
    pub ms: f64,
    /// What the node was run with: its settings, what came in, and the date of the file
    /// read. A result with the same key can be used again instead of running the node.
    pub key: u64,
    /// The result was taken from the cache of an earlier run.
    pub cached: bool,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub nodes: HashMap<String, NodeResult>,
    pub log: Vec<LogEntry>,
    pub order: Vec<String>,
    pub errors: usize,
    pub warnings: usize,
    pub ms: f64,
}

impl RunResult {
    pub fn output(&self, node: &str, port: &str) -> Option<&Arc<Table>> {
        self.nodes.get(node).and_then(|r| r.outputs.get(port))
    }
}

fn finish(results: &mut HashMap<String, NodeResult>, report: &RefCell<&mut dyn FnMut(Progress)>, id: &str, result: NodeResult) {
    (report.borrow_mut())(Progress::Finished { id, result: &result });
    results.insert(id.to_string(), result);
}

/// Hands the lines of the log that have not been reported yet to the one who follows the run.
fn flush(log: &[LogEntry], sent: &mut usize, report: &RefCell<&mut dyn FnMut(Progress)>) {
    for entry in &log[(*sent).min(log.len())..] {
        (report.borrow_mut())(Progress::Log { entry });
    }
    *sent = log.len();
}

/// The step at which a long read is written in the log: every tenth when the total is known
/// and large, otherwise every half million.
fn progress_step(verb: &str, done: u64, total: Option<u64>) -> Option<u64> {
    // Reading is quick per feature, so only large reads are worth the lines. Other steps,
    // such as web requests, take long for each one.
    let large = match verb {
        "read" => 100_000,
        // A file read by its size: only one of several megabytes.
        "read the file" => 5_000_000,
        "rendered" => 1,
        _ => 20,
    };
    match total {
        Some(t) if t >= large => Some(done * 10 / t.max(1)),
        Some(_) => None,
        None => Some(done / 500_000),
    }
}

/// What a run reports while it is going on.
pub enum Progress<'a> {
    /// The node is about to run.
    Started { id: &'a str },
    /// The node is done, with or without success.
    Finished { id: &'a str, result: &'a NodeResult },
    /// A line was written in the log. Lines come as they are written, also while a node is
    /// at work, such as how far a large file has been read.
    Log { entry: &'a LogEntry },
}

/// Runs a workspace. An error in a node only stops what is downstream of it.
pub fn run(ws: &Workspace, backend: Option<&dyn Backend>, base_dir: &Path) -> RunResult {
    run_with(ws, backend, base_dir, &mut |_| {})
}

/// Runs a workspace and reports each node as it starts and finishes, so that a user
/// interface can show the counts while the run is going on.
pub fn run_with(ws: &Workspace, backend: Option<&dyn Backend>, base_dir: &Path, report: &mut dyn FnMut(Progress)) -> RunResult {
    run_stoppable(ws, backend, base_dir, report, &std::sync::atomic::AtomicBool::new(false))
}

/// Runs a workspace as `run_with` does, and stops when `stop` is set: the node at work is
/// broken off where it can be, and those that have not run are skipped. What has already
/// been done keeps its results.
pub fn run_stoppable(
    ws: &Workspace,
    backend: Option<&dyn Backend>,
    base_dir: &Path,
    report: &mut dyn FnMut(Progress),
    stop: &std::sync::atomic::AtomicBool,
) -> RunResult {
    run_opts(ws, backend, base_dir, report, RunOptions { stop: Some(stop), ..Default::default() })
}

/// How a run is done. The default runs every node, and cannot be stopped or paused.
#[derive(Default)]
pub struct RunOptions<'a> {
    /// Set to stop the run. The node at work is broken off where it can be.
    pub stop: Option<&'a std::sync::atomic::AtomicBool>,
    /// Set to pause the run, and cleared to go on. It waits between nodes and in long steps.
    pub pause: Option<&'a std::sync::atomic::AtomicBool>,
    /// The nodes to run. Those they need are run too, or taken from the cache. Nodes that
    /// are neither are left out of the result. None runs every node.
    pub fresh: Option<HashSet<String>>,
    /// Results of an earlier run. A node that is not among `fresh`, and has a result here
    /// with the key it has now, is not run again.
    pub cache: Option<&'a HashMap<String, NodeResult>>,
    /// Every node is in the run, not only those `fresh` needs: `fresh` then only says which
    /// must run, and the rest come from the cache when they have not changed.
    pub everything: bool,
}

/// The nodes that the given ones get their data from, directly or further up, not
/// counting themselves.
pub fn upstream_of(ws: &Workspace, of: &HashSet<String>) -> HashSet<String> {
    let mut found: HashSet<String> = HashSet::new();
    let mut todo: Vec<String> = of.iter().cloned().collect();
    while let Some(id) = todo.pop() {
        for e in ws.edges.iter().filter(|e| e.to.0 == id) {
            if !of.contains(&e.from.0) && found.insert(e.from.0.clone()) {
                todo.push(e.from.0.clone());
            }
        }
    }
    found
}

/// The nodes that get data from the given one, directly or further down, not counting
/// itself.
pub fn downstream_of(ws: &Workspace, id: &str) -> HashSet<String> {
    let mut found: HashSet<String> = HashSet::new();
    let mut todo = vec![id.to_string()];
    while let Some(id) = todo.pop() {
        for e in ws.edges.iter().filter(|e| e.from.0 == id) {
            if e.to.0 != id && found.insert(e.to.0.clone()) {
                todo.push(e.to.0.clone());
            }
        }
    }
    found.remove(id);
    found
}

/// A key for each node that changes when anything that decides its result changes: its
/// type and settings, the variables, whether it is switched off, what is connected to it
/// and the keys of those nodes, and for an input the time its file was last changed.
pub fn node_keys(ws: &Workspace, base_dir: &Path) -> HashMap<String, u64> {
    fn key_of(id: &str, ws: &Workspace, base_dir: &Path, keys: &mut HashMap<String, u64>, busy: &mut HashSet<String>) -> u64 {
        if let Some(k) = keys.get(id) {
            return *k;
        }
        let Some(n) = ws.nodes.iter().find(|n| n.id == id) else { return 0 };
        // A loop has no key of its own.
        if !busy.insert(id.to_string()) {
            return 0;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        n.kind.hash(&mut h);
        n.disabled.hash(&mut h);
        let params = expand_params(&n.params, &ws.variables).unwrap_or_else(|_| n.params.clone());
        serde_json::to_string(&params).unwrap_or_default().hash(&mut h);
        if n.kind == "Reader" {
            if let Some(path) = params.get("path").and_then(|p| p.as_str()).filter(|p| !p.contains("://")) {
                let file = base_dir.join(path);
                if let Ok(t) = std::fs::metadata(&file).and_then(|m| m.modified()) {
                    t.hash(&mut h);
                }
            }
        }
        let mut incoming: Vec<(&str, &str, u64)> = Vec::new();
        for e in ws.edges.iter().filter(|e| e.to.0 == id && !e.disabled) {
            let k = key_of(&e.from.0, ws, base_dir, keys, busy);
            incoming.push((e.to.1.as_str(), e.from.1.as_str(), k));
        }
        incoming.sort();
        incoming.hash(&mut h);
        busy.remove(id);
        let k = h.finish();
        keys.insert(id.to_string(), k);
        k
    }
    let mut keys = HashMap::new();
    let mut busy = HashSet::new();
    for n in &ws.nodes {
        key_of(&n.id, ws, base_dir, &mut keys, &mut busy);
    }
    keys
}

/// Runs a workspace with the given options: stopped, paused, only some of the nodes, and
/// with results of an earlier run used again where nothing has changed.
pub fn run_opts(ws: &Workspace, backend: Option<&dyn Backend>, base_dir: &Path, report: &mut dyn FnMut(Progress), opts: RunOptions) -> RunResult {
    let never = std::sync::atomic::AtomicBool::new(false);
    let still = std::sync::atomic::AtomicBool::new(false);
    let stop = opts.stop.unwrap_or(&never);
    let gate = nodes::Gate { stop, pause: opts.pause.unwrap_or(&still) };
    let stopped = || !gate.going();
    let keys = node_keys(ws, base_dir);
    let scope: Option<HashSet<String>> = opts.fresh.as_ref().filter(|_| !opts.everything).map(|f| {
        let mut all = upstream_of(ws, f);
        all.extend(f.iter().cloned());
        all
    });
    let in_scope = |id: &str| scope.as_ref().is_none_or(|s| s.contains(id));
    let must_run = |id: &str| opts.fresh.as_ref().is_none_or(|f| f.contains(id));
    let mut written: Vec<LogEntry> = Vec::new();
    let mut not_run = 0usize;
    let report: RefCell<&mut dyn FnMut(Progress)> = RefCell::new(report);
    let report = &report;
    let mut sent = 0usize;
    let t0 = Instant::now();
    let now = chrono::Local::now().naive_local();
    let mut log: Vec<LogEntry> = Vec::new();
    let mut results: HashMap<String, NodeResult> = HashMap::new();
    let (mut errors, mut warnings) = (0usize, 0usize);
    let ids: HashMap<&str, usize> = ws.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();

    // Connections that point to nodes or ports that do not exist give an error on the target node.
    let mut bad_edges: HashMap<&str, String> = HashMap::new();
    let mut live: Vec<&Edge> = Vec::new();
    for e in ws.edges.iter().filter(|e| !e.disabled) {
        let (from, to) = (ids.get(e.from.0.as_str()), ids.get(e.to.0.as_str()));
        let problem = match (from, to) {
            (None, _) => Some(format!("A connection starts at the node {}, which does not exist.", e.from.0)),
            (_, None) => {
                log.push(LogEntry {
                    level: Level::Warn,
                    node: None,
                    msg: format!("A connection goes to the node {}, which does not exist. It was skipped.", e.to.0),
                });
                warnings += 1;
                continue;
            }
            (Some(&f), Some(&t)) => {
                let (ft, tt) = (node_type(&ws.nodes[f].kind), node_type(&ws.nodes[t].kind));
                if ft.is_some_and(|x| !x.outputs_for(&ws.nodes[f].params).contains(&e.from.1)) {
                    Some(format!("There is no output port {} on {}.", e.from.1, ws.nodes[f].label()))
                } else if tt.is_some_and(|x| !x.inputs.contains(&e.to.1.as_str())) {
                    Some(format!("There is no input port {} on {}.", e.to.1, ws.nodes[t].label()))
                } else {
                    None
                }
            }
        };
        match problem {
            Some(p) => {
                bad_edges.entry(e.to.0.as_str()).or_insert(p);
            }
            None => live.push(e),
        }
    }

    // Topological order. Nodes that are left over are part of a loop.
    let mut indeg: Vec<usize> = vec![0; ws.nodes.len()];
    for e in &live {
        indeg[ids[e.to.0.as_str()]] += 1;
    }
    let mut queue: std::collections::VecDeque<usize> = (0..ws.nodes.len()).filter(|i| indeg[*i] == 0).collect();
    let mut order: Vec<usize> = Vec::new();
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for e in live.iter().filter(|e| ids[e.from.0.as_str()] == i) {
            let t = ids[e.to.0.as_str()];
            indeg[t] -= 1;
            if indeg[t] == 0 {
                queue.push_back(t);
            }
        }
    }
    // Make zip file packs what the others wrote, so it runs after them all.
    let (zips, rest): (Vec<usize>, Vec<usize>) = order.into_iter().partition(|i| ws.nodes[*i].kind == "ZipWriter");
    let mut order = rest;
    order.extend(zips);
    let fail = |msg: String| NodeResult {
        status: Status::Error,
        error: Some(msg),
        warnings: vec![],
        outputs: HashMap::new(),
        inputs: HashMap::new(),
        files: vec![],
        sources: vec![],
        ms: 0.0,
        ..Default::default()
    };
    for (i, n) in ws.nodes.iter().enumerate() {
        if !order.contains(&i) && in_scope(&n.id) {
            let msg = "The node is part of a loop and cannot run.".to_string();
            log.push(LogEntry { level: Level::Error, node: Some(n.id.clone()), msg: format!("{}: {}", n.label(), msg) });
            finish(&mut results, report, &n.id, fail(msg));
            errors += 1;
        }
    }

    // The nodes that are switched off, and those that rest because of them.
    let mut idle: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &i in &order {
        let n = &ws.nodes[i];
        if !in_scope(&n.id) {
            continue;
        }
        let push_error = |log: &mut Vec<LogEntry>, msg: String| {
            log.push(LogEntry { level: Level::Error, node: Some(n.id.clone()), msg: format!("{}: {}", n.label(), msg) });
        };
        let ty = match node_type(&n.kind) {
            Some(t) => t,
            None => {
                let msg = format!("Unknown node type: {}.", n.kind);
                push_error(&mut log, msg.clone());
                finish(&mut results, report, &n.id, fail(msg));
                errors += 1;
                continue;
            }
        };
        if let Some(msg) = bad_edges.get(n.id.as_str()) {
            push_error(&mut log, msg.clone());
            finish(&mut results, report, &n.id, fail(msg.clone()));
            errors += 1;
            continue;
        }
        let nothing = |status: Status, why: &str| NodeResult {
            status,
            error: Some(why.to_string()),
            warnings: vec![],
            outputs: HashMap::new(),
            inputs: HashMap::new(),
            files: vec![],
            sources: vec![],
            ms: 0.0,
            ..Default::default()
        };
        if stopped() {
            finish(&mut results, report, &n.id, nothing(Status::Skipped, "Stopped before it ran."));
            idle.insert(n.id.as_str());
            not_run += 1;
            continue;
        }
        if n.disabled {
            log.push(LogEntry { level: Level::Info, node: Some(n.id.clone()), msg: format!("{}: disabled", n.label()) });
            finish(&mut results, report, &n.id, nothing(Status::Disabled, "The node is disabled."));
            idle.insert(n.id.as_str());
            continue;
        }
        // A result from an earlier run, made with the same settings and the same data.
        let reusable = opts
            .cache
            .filter(|_| !must_run(&n.id))
            .and_then(|c| c.get(&n.id))
            .filter(|r| r.key == keys.get(&n.id).copied().unwrap_or(0) && r.key != 0 && matches!(r.status, Status::Ok | Status::Warn));
        let fed = live.iter().filter(|e| e.to.0 == n.id).all(|e| results.get(&e.from.0).is_some_and(|r| matches!(r.status, Status::Ok | Status::Warn)));
        if let (Some(old), true) = (reusable, fed) {
            let mut r = old.clone();
            r.cached = true;
            let ports = ty.outputs_for(&n.params);
            let counts: Vec<String> = ports
                .iter()
                .map(|p| {
                    let c = fmt_count(r.outputs.get(p).map_or(0, |t| t.len()) as u64);
                    if ports.len() > 1 {
                        format!("{} {}", p, c)
                    } else {
                        c
                    }
                })
                .collect();
            let msg =
                if counts.is_empty() { format!("{}: from the cache", n.label()) } else { format!("{}: {} out, from the cache", n.label(), counts.join(", ")) };
            log.push(LogEntry { level: Level::Info, node: Some(n.id.clone()), msg });
            finish(&mut results, report, &n.id, r);
            continue;
        }
        flush(&log, &mut sent, report);
        (report.borrow_mut())(Progress::Started { id: &n.id });
        // How far a long step has come, written in the log as it goes.
        let steps: RefCell<Vec<LogEntry>> = RefCell::new(Vec::new());
        let last = std::cell::Cell::new((String::new(), u64::MAX));
        let progress = |verb: &str, done: u64, total: Option<u64>| {
            let Some(step) = progress_step(verb, done, total) else { return };
            let (last_verb, last_step) = last.take();
            let same = last_verb == verb && last_step == step;
            last.set((verb.to_string(), step));
            if same || (step == 0 && total.is_some()) || (total.is_none() && done == 0) {
                return;
            }
            let msg = match total {
                // A file read by its size says how much of it is read.
                Some(t) if verb == "read the file" => format!("{}: read {} % of the file", n.label(), (done * 100 / t.max(1)).min(100)),
                Some(t) if verb == "rendered" => format!("{}: rendered {} % of the picture", n.label(), (done * 100 / t.max(1)).min(100)),
                Some(t) => format!("{}: {} {} of {} features ({} %)", n.label(), verb, fmt_count(done), fmt_count(t), (done * 100 / t.max(1)).min(100)),
                None => format!("{}: {} {} features so far", n.label(), verb, fmt_count(done)),
            };
            let entry = LogEntry { level: Level::Info, node: Some(n.id.clone()), msg };
            (report.borrow_mut())(Progress::Log { entry: &entry });
            steps.borrow_mut().push(entry);
        };
        // A line of its own in the log, written at once, such as from a model run by this one.
        let say = |level: Level, msg: &str| {
            let entry = LogEntry { level, node: Some(n.id.clone()), msg: msg.to_string() };
            (report.borrow_mut())(Progress::Log { entry: &entry });
            steps.borrow_mut().push(entry);
        };
        let mut ctx = NodeCtx {
            say: &say,
            backend,
            base_dir,
            node_name: n.label(),
            notes: Notes::default(),
            files: Vec::new(),
            sources: Vec::new(),
            now,
            progress: &progress,
            gate,
            parts: Vec::new(),
            open_ports: ty.inputs.iter().copied().filter(|port| !live.iter().any(|e| e.to.0 == n.id && e.to.1 == *port)).collect(),
            written: if n.kind == "ZipWriter" {
                ws.nodes
                    .iter()
                    .filter(|m| m.kind != "ZipWriter" && node_type(&m.kind).is_some_and(|t| t.group == nodes::Group::Writer))
                    .filter_map(|m| results.get(&m.id).map(|r| (m.label().to_string(), r.files.clone())))
                    .flat_map(|(label, files)| files.into_iter().map(move |f| (label.clone(), f)))
                    .collect()
            } else {
                Vec::new()
            },
        };
        let started = Instant::now();
        let mut inputs: Inputs = HashMap::new();
        let mut blocked = false;
        // A port whose every connection comes from something that is switched off.
        let mut starved: Option<&str> = None;
        // A port of a writer that nothing is connected to.
        let mut unfed: Option<&str> = None;
        let mut failure: Option<String> = None;
        for port in ty.inputs {
            let mut tables = Vec::new();
            let mut off = 0;
            if ty.group == nodes::Group::Writer && !live.iter().any(|e| e.to.0 == n.id && e.to.1 == *port) {
                unfed = Some(port);
            }
            for e in live.iter().filter(|e| e.to.0 == n.id && e.to.1 == *port) {
                match results.get(&e.from.0) {
                    // What is switched off sends nothing, and that is not a failure.
                    Some(_) if idle.contains(e.from.0.as_str()) => off += 1,
                    Some(r) if r.status != Status::Error && r.status != Status::Skipped => {
                        let table = r.outputs.get(&e.from.1).cloned().unwrap_or_default();
                        if nodes::keeps_apart(&n.kind, port) {
                            let source = ws.nodes.iter().find(|x| x.id == e.from.0);
                            let many = source.and_then(|x| node_type(&x.kind)).is_some_and(|t| t.outputs_for(&source.expect("found").params).len() > 1);
                            let name = source.map_or(e.from.0.clone(), |x| x.label().to_string());
                            let label = if many { format!("{} · {}", name, e.from.1) } else { name };
                            ctx.parts.push(nodes::Part { port, from: format!("{}:{}", e.from.0, e.from.1), label, table });
                            continue;
                        }
                        tables.push(table);
                    }
                    _ => blocked = true,
                }
            }
            if tables.is_empty() && off > 0 && !ctx.parts.iter().any(|x| x.port == *port) {
                starved = Some(port);
            }
            match nodes::merge_tables(tables, backend, "incoming streams", &mut ctx.notes) {
                Ok(t) => {
                    inputs.insert(port, t);
                }
                Err(e) => {
                    failure = Some(e.0);
                    inputs.insert(port, Arc::new(Table::default()));
                }
            }
        }
        if blocked {
            log.push(LogEntry { level: Level::Warn, node: Some(n.id.clone()), msg: format!("{}: skipped because a node upstream failed", n.label()) });
            warnings += 1;
            finish(
                &mut results,
                report,
                &n.id,
                NodeResult {
                    status: Status::Skipped,
                    error: Some("Skipped because a node upstream failed.".into()),
                    warnings: vec![],
                    outputs: HashMap::new(),
                    inputs: HashMap::new(),
                    files: vec![],
                    sources: vec![],
                    ms: 0.0,
                    ..Default::default()
                },
            );
            continue;
        }
        if let Some(port) = unfed {
            // A writer with nothing connected would write an empty file over a good one.
            let why = format!("Skipped because nothing is connected to {}.", port);
            log.push(LogEntry { level: Level::Info, node: Some(n.id.clone()), msg: format!("{}: skipped, since nothing is connected to {}", n.label(), port) });
            finish(&mut results, report, &n.id, nothing(Status::Skipped, &why));
            idle.insert(n.id.as_str());
            continue;
        }
        if let Some(port) = starved {
            // Running on nothing would write empty files over good ones. So the node rests.
            let why = format!("Skipped because what feeds {} is disabled.", port);
            log.push(LogEntry { level: Level::Info, node: Some(n.id.clone()), msg: format!("{}: skipped, since what feeds {} is disabled", n.label(), port) });
            finish(&mut results, report, &n.id, nothing(Status::Skipped, &why));
            idle.insert(n.id.as_str());
            continue;
        }
        let outcome = match failure {
            Some(msg) => Err(GbError(msg)),
            None => match expand_params(&n.params, &ws.variables) {
                Ok(params) => crate::crash::guarded(n.label(), || (ty.run)(&P::new(&params, ty.params), &inputs, &mut ctx)),
                Err(e) => Err(e),
            },
        };
        let took = started.elapsed().as_secs_f64() * 1000.0;
        // The steps were reported as they came, so they are not sent again.
        let steps = steps.take();
        let reported_all = sent == log.len();
        log.extend(steps);
        if reported_all {
            sent = log.len();
        }
        let kept_inputs: HashMap<String, Arc<Table>> = inputs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        match outcome {
            // Broken off because the run was stopped. Not an error in the node.
            Err(_) if stopped() => {
                log.push(LogEntry { level: Level::Warn, node: Some(n.id.clone()), msg: format!("{}: stopped after {}", n.label(), fmt_duration(took)) });
                let mut r = nothing(Status::Skipped, "Stopped while it ran.");
                r.inputs = kept_inputs;
                r.ms = took;
                finish(&mut results, report, &n.id, r);
                idle.insert(n.id.as_str());
                not_run += 1;
            }
            Err(e) => {
                push_error(&mut log, e.0.clone());
                errors += 1;
                finish(
                    &mut results,
                    report,
                    &n.id,
                    NodeResult {
                        status: Status::Error,
                        error: Some(e.0),
                        warnings: ctx.notes.warnings,
                        outputs: HashMap::new(),
                        inputs: kept_inputs,
                        files: vec![],
                        sources: ctx.sources,
                        ms: took,
                        ..Default::default()
                    },
                );
            }
            Ok(mut outs) => {
                // Geometry to 2D leaves the 3D model behind on purpose.
                if n.kind != "Force2D" {
                    carry_models(&inputs, &mut outs);
                }
                let outputs: HashMap<String, Arc<Table>> = outs.into_iter().map(|(k, v)| (k.to_string(), Arc::new(v))).collect();
                let parts = &ctx.parts;
                let count = |port: &&str, map: &HashMap<String, Arc<Table>>| {
                    // A port that keeps its streams apart counts what came in on all of them.
                    let apart: usize = parts.iter().filter(|x| x.port == *port).map(|x| x.table.len()).sum();
                    fmt_count((map.get(*port).map_or(0, |t| t.len()) + apart) as u64)
                };
                let named = |ports: &[&str], labels: &[String], map: &HashMap<String, Arc<Table>>| {
                    ports
                        .iter()
                        .zip(labels)
                        .map(|(p, label)| if ports.len() > 1 { format!("{} {}", label, count(p, map)) } else { count(p, map) })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let ports = ty.outputs_for(&n.params);
                // Ports named after values are quoted in the log, so that "1" 6 is not read as 16.
                let shown: Vec<String> =
                    ports.iter().enumerate().map(|(i, p)| if ty.ports.is_some() && i + 1 < ports.len() { format!("\"{}\"", p) } else { p.clone() }).collect();
                let ports: Vec<&str> = ports.iter().map(|p| p.as_str()).collect();
                let mut msg = format!("{}: ", n.label());
                // View writes nothing; it only keeps what comes in to be looked at.
                let writer = ty.group == nodes::Group::Writer && n.kind != "Viewer";
                let mut wrote = String::new();
                if writer {
                    // An output says how much it wrote.
                    let input = kept_inputs.get("Input");
                    let layers = parts.iter().filter(|x| x.port == "Layers").count();
                    wrote = match input {
                        _ if ty.inputs == ["Layers"] => format!("{} {}", layers, if layers == 1 { "layer" } else { "layers" }),
                        _ if ty.inputs == ["Pictures"] => {
                            let n = parts.iter().filter(|x| x.port == "Pictures" && x.table.image.is_some()).count();
                            format!("{} {}", n, if n == 1 { "picture" } else { "pictures" })
                        }
                        Some(t) if t.image.is_some() => "an image".to_string(),
                        Some(t) => format!("{} {}", fmt_count(t.len() as u64), if t.len() == 1 { "feature" } else { "features" }),
                        None => "nothing".to_string(),
                    };
                    msg += &format!("{} written", wrote.trim_start_matches("an "));
                } else if n.kind == "Viewer" {
                    msg = n.label().to_string();
                } else if !ty.inputs.is_empty() {
                    let labels: Vec<String> = ty.inputs.iter().map(|p| p.to_string()).collect();
                    msg += &format!("{} in", named(ty.inputs, &labels, &kept_inputs));
                }
                if !ty.inputs.is_empty() && !ports.is_empty() {
                    msg += " → ";
                }
                if !ports.is_empty() {
                    msg += &format!("{} out", named(&ports, &shown, &outputs));
                }
                // The files that go with another, such as a world file, are not named.
                let companion = |f: &PathBuf| {
                    let name = f.to_string_lossy().to_lowercase();
                    [".aux.xml", ".pgw", ".jgw"].iter().any(|e| name.ends_with(e))
                };
                let names: Vec<String> =
                    ctx.files.iter().filter(|f| !companion(f)).map(|f| f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).collect();
                for f in &names {
                    msg += &format!(" → {}", f);
                }
                if writer {
                    let to = if names.is_empty() { String::new() } else { format!(" to {}", names.join(", ")) };
                    written.push(LogEntry { level: Level::Info, node: Some(n.id.clone()), msg: format!("{} wrote {}{}", n.label(), wrote, to) });
                }
                if !ctx.notes.info.is_empty() {
                    msg += &format!(" · {}", ctx.notes.info.join(", "));
                }
                msg += &format!(" ({})", fmt_duration(took));
                log.push(LogEntry { level: Level::Info, node: Some(n.id.clone()), msg });
                for w in &ctx.notes.warnings {
                    log.push(LogEntry { level: Level::Warn, node: Some(n.id.clone()), msg: format!("{}: {}", n.label(), w) });
                    warnings += 1;
                }
                let status = if ctx.notes.warnings.is_empty() { Status::Ok } else { Status::Warn };
                finish(
                    &mut results,
                    report,
                    &n.id,
                    NodeResult {
                        status,
                        error: None,
                        warnings: ctx.notes.warnings,
                        outputs,
                        inputs: kept_inputs,
                        files: ctx.files,
                        sources: ctx.sources,
                        ms: took,
                        ..Default::default()
                    },
                );
            }
        }
    }

    // Each output and what it wrote, together at the end.
    log.extend(written);
    for (id, r) in results.iter_mut() {
        r.key = keys.get(id).copied().unwrap_or(0);
    }
    let ran = ws.nodes.iter().filter(|n| in_scope(&n.id)).count();
    let total = t0.elapsed().as_secs_f64() * 1000.0;
    let how = if stopped() && not_run > 0 {
        format!("stopped, {} {} did not run", not_run, if not_run == 1 { "node" } else { "nodes" })
    } else if errors > 0 {
        format!("ended with {} {}", errors, if errors == 1 { "error" } else { "errors" })
    } else {
        "finished".to_string()
    };
    log.push(LogEntry {
        level: if errors > 0 {
            Level::Error
        } else if stopped() && not_run > 0 {
            Level::Warn
        } else {
            Level::Info
        },
        node: None,
        msg: format!(
            "Run {}: {} {}, {} {}, {}",
            how,
            ran,
            if ran == 1 { "node" } else { "nodes" },
            warnings,
            if warnings == 1 { "warning" } else { "warnings" },
            fmt_duration(total)
        ),
    });
    flush(&log, &mut sent, report);
    RunResult { nodes: results, log, order: order.iter().map(|i| ws.nodes[*i].id.clone()).collect(), errors, warnings, ms: total }
}

/// A stream that keeps some of the features of one with a 3D model, as they were, as a filter
/// or a sort does, keeps their part of the model too: so that the buildings of a city model
/// that pass a test can be rendered, without every such tool having to know of models.
pub(crate) fn carry_models(inputs: &crate::nodes::Inputs, outs: &mut crate::nodes::Outputs) {
    let sources: Vec<&Table> = inputs
        .values()
        .map(|t| t.as_ref())
        .filter(|t| t.mesh.as_ref().is_some_and(|m| m.objects.len() == t.features.len()) && t.features.len() <= 500_000)
        .collect();
    if sources.is_empty() || outs.iter().all(|(_, o)| o.mesh.is_some() || o.features.is_empty()) {
        return;
    }
    let key = |f: &crate::model::Feature| format!("{:?}|{:?}", f.attrs, f.geom);
    for src in sources {
        let mut rows_of: HashMap<String, std::collections::VecDeque<usize>> = HashMap::new();
        for (i, f) in src.features.iter().enumerate() {
            rows_of.entry(key(f)).or_default().push_back(i);
        }
        let names = src.field_names();
        for (_, out) in outs.iter_mut() {
            if out.mesh.is_some() || out.features.is_empty() || out.field_names() != names {
                continue;
            }
            let mut taken = rows_of.clone();
            let rows: Option<Vec<usize>> = out.features.iter().map(|f| taken.get_mut(&key(f)).and_then(|q| q.pop_front())).collect();
            if let (Some(rows), Some(m)) = (rows, src.mesh.as_ref()) {
                out.mesh = Some(Arc::new(m.pick(&rows)));
            }
        }
    }
}
