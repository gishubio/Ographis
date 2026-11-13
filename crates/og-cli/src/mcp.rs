//! `og mcp`: Ographis as a server of the Model Context Protocol, over standard input and
//! output, so that an assistant such as Claude can look at data, build a model, check it, try
//! it on a sample, run it and look at what came out, while the user sees the model on the
//! canvas.
//!
//! What it may touch is held in: only the folders it is given; passwords and keys never
//! reach the assistant; a file that is already there is never written over, unless the
//! assistant made it in the same session; nothing is fetched from the web, and nothing
//! written to a database, unless the user allowed it when the server was started.

use og_engine::io::Backend;
use og_engine::model::{crs_label, fmt_count, fmt_duration, Table, Value};
use og_engine::nodes::{Group, NODE_TYPES};
use og_engine::run::{NodeResult, RunResult, Status};
use og_engine::{Level, Progress, Workspace};
use og_gdal::GdalBackend;
use serde_json::{json, Value as J};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// The most text one answer gives; longer is cut, with a word on how to see the rest.
const MOST_TEXT: usize = 24_000;

/// The rows a trial run reads from each input, when nothing else is asked.
const TRIAL: usize = 100;

const AGENTS_MD: &str = include_str!("../../../AGENTS.md");

const INSTRUCTIONS: &str = "Ographis is a desktop program for spatial ETL: a model is a JSON file of nodes (readers, tools, writers) and the connections between their ports. \
Work like this: describe the data first; look up tools with find_nodes and node_help, never guess type names, ports or settings; \
write the model with save_model, which checks it and shows it to the user on the canvas; try it with run_model, which by default reads only the first 100 features of each input and writes nothing; \
look at what came out with inspect and look; run it in full with run_model and full: true only when the trial went well. \
The resource ographis://docs/AGENTS.md has the rules of the model file, and ographis://examples/ has examples to start from. \
Paths are within the folders the user allowed. Files that exist are not written over; choose a new name. Web services and databases need the user's permission, given when the server is started.";

/// What the user allowed when the server was started.
struct Allowed {
    folders: Vec<PathBuf>,
    web: bool,
    overwrite: bool,
    databases: bool,
}

/// A run kept to look at afterwards: what the model was, and how many features each input read.
struct Kept {
    text: String,
    sample: Option<usize>,
    result: RunResult,
}

struct Server {
    allowed: Allowed,
    backend: GdalBackend,
    /// Files and folders made by the assistant in this session, which it may write again.
    made: HashSet<PathBuf>,
    /// The models whose trial went without errors, by their text.
    tried: HashSet<String>,
    /// The latest run of each model.
    runs: HashMap<PathBuf, Kept>,
}

pub fn serve(args: &[String]) -> ExitCode {
    let mut allowed = Allowed { folders: Vec::new(), web: false, overwrite: false, databases: false };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--folder" => match it.next() {
                Some(f) => allowed.folders.push(PathBuf::from(f)),
                None => return wrong("--folder needs a folder."),
            },
            "--allow-web" => allowed.web = true,
            "--allow-overwrite" => allowed.overwrite = true,
            "--allow-databases" => allowed.databases = true,
            other if !other.starts_with("--") => allowed.folders.push(PathBuf::from(other)),
            other => return wrong(&format!("og mcp does not know {}.", other)),
        }
    }
    // As an extension of Claude Desktop sets them, from what the user ticked.
    let yes = |name: &str| std::env::var(name).is_ok_and(|v| matches!(v.trim(), "true" | "1" | "yes"));
    allowed.web |= yes("OG_MCP_ALLOW_WEB");
    allowed.databases |= yes("OG_MCP_ALLOW_DATABASES");
    allowed.overwrite |= yes("OG_MCP_ALLOW_OVERWRITE");
    if let Ok(more) = std::env::var("OG_MCP_FOLDERS") {
        allowed.folders.extend(std::env::split_paths(&more).filter(|p| !p.as_os_str().is_empty()));
    }
    if allowed.folders.is_empty() {
        allowed.folders.push(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    }
    let mut folders = Vec::new();
    for f in &allowed.folders {
        match std::fs::canonicalize(f) {
            Ok(c) if c.is_dir() => folders.push(c),
            _ => return wrong(&format!("The folder {} does not exist.", f.display())),
        }
    }
    allowed.folders = folders;
    eprintln!(
        "Ographis MCP server {} in {}{}{}{}",
        env!("CARGO_PKG_VERSION"),
        allowed.folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", "),
        if allowed.web { ", the web allowed" } else { "" },
        if allowed.databases { ", databases allowed" } else { "" },
        if allowed.overwrite { ", writing over files allowed" } else { "" }
    );
    let mut server = Server { allowed, backend: GdalBackend::new(), made: HashSet::new(), tried: HashSet::new(), runs: HashMap::new() };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answers: Vec<J> = match serde_json::from_str::<J>(&line) {
            Ok(J::Array(batch)) => batch.iter().filter_map(|m| server.handle(m)).collect(),
            Ok(m) => server.handle(&m).into_iter().collect(),
            Err(e) => vec![json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Not JSON: {}", e)}})],
        };
        for a in answers {
            if writeln!(out, "{}", a).and_then(|_| out.flush()).is_err() {
                return ExitCode::SUCCESS;
            }
        }
    }
    ExitCode::SUCCESS
}

fn wrong(msg: &str) -> ExitCode {
    eprintln!("{}\nUsage: og mcp [--folder DIR]... [--allow-web] [--allow-databases] [--allow-overwrite]", msg);
    ExitCode::from(2)
}

/// An answer of a tool: text, perhaps a picture, and whether it is an error.
struct Answer {
    text: String,
    image: Option<Vec<u8>>,
    error: bool,
}

impl Answer {
    fn ok(text: impl Into<String>) -> Answer {
        Answer { text: text.into(), image: None, error: false }
    }
    fn fail(text: impl Into<String>) -> Answer {
        Answer { text: text.into(), image: None, error: true }
    }
}

type Done = std::result::Result<Answer, String>;

impl Server {
    fn handle(&mut self, m: &J) -> Option<J> {
        let id = m.get("id").cloned();
        let method = m.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = m.get("params").cloned().unwrap_or(json!({}));
        // A notification gets no answer.
        let id = id?;
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOLS[0]);
                let version = PROTOCOLS.iter().find(|p| **p == asked).copied().unwrap_or(PROTOCOLS[0]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}, "prompts": {"listChanged": false}},
                    "serverInfo": {"name": "ographis", "title": "Ographis", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let a = match self.call(name, &args) {
                    Ok(a) => a,
                    Err(e) => Answer::fail(e),
                };
                let mut content = vec![json!({"type": "text", "text": cut(&a.text)})];
                if let Some(png) = a.image {
                    use base64::Engine;
                    content.push(json!({"type": "image", "mimeType": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(png)}));
                }
                Ok(json!({ "content": content, "isError": a.error }))
            }
            "resources/list" => Ok(self.resources(params.get("cursor").and_then(|c| c.as_str()))),
            "resources/templates/list" => Ok(json!({"resourceTemplates": [
                {"uriTemplate": "ographis://examples/{name}", "name": "example", "title": "An example model", "mimeType": "application/json"},
                {"uriTemplate": "ographis://models/{path}", "name": "model", "title": "A model in the allowed folders", "mimeType": "application/json"}
            ]})),
            "resources/read" => self.read_resource(params.get("uri").and_then(|u| u.as_str()).unwrap_or("")),
            "prompts/list" => Ok(json!({ "prompts": prompts() })),
            "prompts/get" => prompt(params.get("name").and_then(|v| v.as_str()).unwrap_or(""), params.get("arguments").unwrap_or(&json!({}))),
            other => Err((-32601, format!("There is no method {}.", other))),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, msg)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}}),
        })
    }

    fn call(&mut self, name: &str, a: &J) -> Done {
        match name {
            "list_files" => self.list_files(a),
            "describe" => self.describe(a),
            "find_nodes" => find_nodes(a),
            "node_help" => node_help(a),
            "read_model" => self.read_model(a),
            "open_model" => open_model(),
            "check_model" => self.check_model(a),
            "save_model" => self.save_model(a),
            "run_model" => self.run_model(a),
            "inspect" => self.inspect(a),
            "look" => self.look(a),
            other => Err(format!(
                "There is no tool {}. The tools are: list_files, describe, find_nodes, node_help, read_model, open_model, check_model, save_model, run_model, inspect and look.",
                other
            )),
        }
    }

    // ---- Paths ----

    /// A path given by the assistant, within the allowed folders: relative ones start from
    /// the first folder.
    fn path(&self, given: &str) -> std::result::Result<PathBuf, String> {
        let given = given.trim();
        if given.is_empty() {
            return Err("Give a path.".into());
        }
        let p = Path::new(given);
        let p = if p.is_absolute() { p.to_path_buf() } else { self.allowed.folders[0].join(p) };
        // A file that is not there yet is judged by its folder.
        let real = match std::fs::canonicalize(&p) {
            Ok(r) => r,
            Err(_) => {
                let parent = p.parent().ok_or("A path needs a folder.")?;
                let parent = std::fs::canonicalize(parent).map_err(|_| format!("The folder {} does not exist.", parent.display()))?;
                parent.join(p.file_name().ok_or("A path needs a name.")?)
            }
        };
        if self.allowed.folders.iter().any(|f| real.starts_with(f)) {
            Ok(real)
        } else {
            Err(format!(
                "{} is outside the folders Ographis may use here: {}. Ask the user to add its folder when the server is started.",
                given,
                self.folders_text()
            ))
        }
    }

    fn folders_text(&self) -> String {
        self.allowed.folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")
    }

    /// The path as the assistant sees it: from the first folder when it lies there.
    fn shown(&self, p: &Path) -> String {
        for f in &self.allowed.folders {
            if let Ok(r) = p.strip_prefix(f) {
                let r = r.to_string_lossy().replace('\\', "/");
                return if self.allowed.folders.len() == 1 { r } else { p.to_string_lossy().into_owned() };
            }
        }
        p.to_string_lossy().into_owned()
    }

    // ---- Files and data ----

    fn list_files(&self, a: &J) -> Done {
        let dir = match a.get("folder").and_then(|v| v.as_str()) {
            Some(f) => self.path(f)?,
            None => self.allowed.folders[0].clone(),
        };
        if !dir.is_dir() {
            return Err(format!("{} is not a folder.", self.shown(&dir)));
        }
        let page = page_of(a);
        let mut found: Vec<(String, u64, bool)> = Vec::new();
        let mut stack = vec![(dir.clone(), 0)];
        while let Some((d, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if name.starts_with('.') || name.ends_with(".runs") || name == "target" || name == "node_modules" {
                    continue;
                }
                if p.is_dir() {
                    // Folders of files that are read as one, such as a shapefile's, and a GDB.
                    if name.to_lowercase().ends_with(".gdb") {
                        found.push((self.shown(&p), 0, true));
                    } else if depth < 3 {
                        stack.push((p, depth + 1));
                    }
                } else if known_file(&name) {
                    found.push((self.shown(&p), e.metadata().map(|m| m.len()).unwrap_or(0), false));
                }
            }
        }
        found.sort();
        let total = found.len();
        let (from, to) = (page * 100, ((page + 1) * 100).min(total));
        let shown_dir = self.shown(&dir);
        let mut text = format!(
            "{} files of data and models in {}{}:\n",
            total,
            if shown_dir.is_empty() { "." } else { shown_dir.as_str() },
            if total > 100 { format!(", page {} of {}", page + 1, total.div_ceil(100)) } else { String::new() }
        );
        for (p, size, _) in found.iter().take(to).skip(from) {
            text.push_str(&format!("{}  {}\n", p, size_text(*size)));
        }
        if to < total {
            text.push_str(&format!("More with page: {}.", page + 1));
        }
        Ok(Answer::ok(text))
    }

    /// A file or service in short: its layers, attributes, coordinate system, extent, count
    /// and a few rows. The data itself stays in Ographis.
    fn describe(&mut self, a: &J) -> Done {
        let given = a.get("path").and_then(|v| v.as_str()).ok_or("Give the path of a file, or the address of a service.")?;
        let rows = a.get("rows").and_then(|v| v.as_u64()).unwrap_or(5).min(20) as usize;
        let is_url = given.trim().starts_with("http://") || given.trim().starts_with("https://");
        let path_text = if is_url {
            if !self.allowed.web {
                return Ok(Answer::fail(format!(
                    "Reading {} would go to the web, which the user has not allowed. Ask the user to start the server with --allow-web.",
                    given
                )));
            }
            given.trim().to_string()
        } else {
            self.path(given)?.to_string_lossy().into_owned()
        };
        let mut text = String::new();
        if !is_url {
            let p = Path::new(&path_text);
            if !p.exists() {
                return Err(format!("There is no file {}. list_files shows what there is.", given));
            }
            if let Ok(layers) = self.backend.layers(p) {
                if layers.len() > 1 {
                    text.push_str(&format!("{} layers:\n", layers.len()));
                    for l in &layers {
                        text.push_str(&format!(
                            "- {}: {} {} features, {}\n",
                            l.name,
                            l.count.map(fmt_count).unwrap_or_else(|| "an unknown number of".into()),
                            l.geometry,
                            crs_label(l.crs)
                        ));
                    }
                    text.push('\n');
                }
            }
        }
        let mut params = serde_json::Map::new();
        params.insert("path".into(), json!(path_text));
        if let Some(extra) = a.get("params").and_then(|p| p.as_object()) {
            for (k, v) in extra {
                params.insert(k.clone(), v.clone());
            }
        }
        if let Some(l) = a.get("layer").and_then(|v| v.as_str()) {
            params.insert("layer".into(), json!(l));
        }
        let ws: Workspace = serde_json::from_value(json!({"format": "ographis/1", "nodes": [{"id": "data", "type": "Reader", "params": params}]}))
            .map_err(|e| format!("The settings cannot be used: {}", e))?;
        let base = self.allowed.folders[0].clone();
        let most = 10_000;
        let result = og_engine::run_opts(&ws, Some(&self.backend), &base, &mut |_| {}, og_engine::RunOptions { sample: Some(most), ..Default::default() });
        let r = &result.nodes["data"];
        if let Some(e) = &r.error {
            return Ok(Answer::fail(format!("{} could not be read: {}", given, scrub(e))));
        }
        let Some(t) = r.outputs.get("Output") else { return Ok(Answer::fail("Nothing came out.")) };
        let read_all = t.len() < most;
        text.push_str(&format!("{}\n{}", given, summary(t, if read_all { "" } else { " (of the first 10 000 read)" })));
        if t.len() >= most {
            text.push_str("There are more than 10 000 features; the extent and figures are of the first 10 000.\n");
        }
        if rows > 0 && !t.features.is_empty() {
            text.push_str(&format!("\nThe first {} rows:\n{}", rows.min(t.len()), rows_text(t, 0, rows, None)));
        }
        let notes: Vec<String> = result.log.iter().filter(|l| l.level == Level::Warn && !l.msg.starts_with("A trial run")).map(|l| scrub(&l.msg)).collect();
        if !notes.is_empty() {
            text.push_str(&format!("\nWarnings:\n{}\n", notes.join("\n")));
        }
        text.push_str("\nTo read it in a model: a Reader with path set to this path");
        if params.len() > 1 {
            text.push_str(&format!(
                " and {}",
                params.iter().filter(|(k, _)| *k != "path").map(|(k, v)| format!("{} = {}", k, v)).collect::<Vec<_>>().join(", ")
            ));
        }
        text.push('.');
        Ok(Answer::ok(text))
    }

    // ---- Models ----

    /// The text of a model, from a path or given whole.
    fn model_text(&self, a: &J) -> std::result::Result<(Option<PathBuf>, String), String> {
        if let Some(m) = a.get("model") {
            let text = match m {
                J::String(s) => s.clone(),
                other => serde_json::to_string_pretty(other).unwrap_or_default(),
            };
            let path = match a.get("path").and_then(|v| v.as_str()) {
                Some(p) => Some(self.path(p)?),
                None => None,
            };
            return Ok((path, text));
        }
        let p = a.get("path").and_then(|v| v.as_str()).ok_or("Give the path of a model, or the model itself as model.")?;
        let p = self.path(p)?;
        let text = std::fs::read_to_string(&p).map_err(|_| format!("There is no model {}. list_files shows the models there are.", self.shown(&p)))?;
        Ok((Some(p), text))
    }

    fn read_model(&self, a: &J) -> Done {
        let (path, text) = self.model_text(a)?;
        let ws = Workspace::from_json(&text).map_err(|e| format!("The model cannot be read: {}", e))?;
        let clean = og_engine::bugreport::without_secrets(&ws);
        Ok(Answer::ok(format!("{}\n{}", path.map(|p| self.shown(&p)).unwrap_or_default(), clean.to_json())))
    }

    fn check_model(&self, a: &J) -> Done {
        let (_, text) = self.model_text(a)?;
        Ok(findings_answer(&text))
    }

    fn save_model(&mut self, a: &J) -> Done {
        let p = a.get("path").and_then(|v| v.as_str()).ok_or("Give the path to save the model to, ending in .ographis.")?;
        let path = self.path(p)?;
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ographis")) {
            return Err("A model is saved as a file ending in .ographis.".into());
        }
        let m = a.get("model").ok_or("Give the model as model: the JSON of the model file.")?;
        let text = match m {
            J::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_default(),
        };
        let ws = match Workspace::from_json(&text) {
            Ok(ws) => ws,
            Err(e) => return Ok(Answer::fail(format!("Not saved: the model cannot be read: {}. ographis://docs/AGENTS.md shows the shape of a model.", e))),
        };
        if path.exists() && !self.may_write(&path) {
            return Ok(Answer::fail(format!(
                "Not saved: {} is there already, and Ographis does not write over a file it did not make in this session. Choose another name, such as {}.",
                self.shown(&path),
                free_name(&path)
            )));
        }
        // Laid out on the canvas when the assistant gave no places.
        let mut ws = ws;
        lay_out(&mut ws);
        std::fs::write(&path, ws.to_json()).map_err(|e| format!("Not saved: {}", e))?;
        self.made.insert(path.clone());
        let mut found = findings_answer(&ws.to_json());
        found.text = format!("Saved as {}.\n{}", self.shown(&path), found.text);
        if a.get("show").and_then(|v| v.as_bool()).unwrap_or(true) {
            found.text.push('\n');
            found.text.push_str(&show(&path));
        }
        Ok(found)
    }

    fn may_write(&self, p: &Path) -> bool {
        self.allowed.overwrite || self.made.contains(p) || self.made.iter().any(|m| p.starts_with(m))
    }

    // ---- Runs ----

    /// What a model would reach outside the allowed folders: the web, databases, and files
    /// it would write. Empty when it may run.
    fn hindrances(&self, ws: &Workspace, base: &Path, writes: bool) -> Vec<String> {
        let mut out = Vec::new();
        for n in ws.nodes.iter().filter(|n| !n.disabled) {
            let t = og_engine::nodes::node_type(&n.kind);
            let label = format!("{} ({})", n.label(), n.id);
            let text_params: Vec<(&String, String)> =
                n.params.iter().map(|(k, v)| (k, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect();
            let web =
                matches!(n.kind.as_str(), "HttpCaller" | "StacSearch" | "SceneReader" | "CloudMasker" | "SpectralIndex" | "TemporalComposite" | "PlaceFetcher")
                    || (n.kind == "MapRenderer" && n.params.get("background").and_then(|v| v.as_str()) == Some("map"))
                    || text_params.iter().any(|(_, v)| v.contains("http://") || v.contains("https://") || v.contains("/vsicurl") || v.contains("/vsis3"));
            if web && !self.allowed.web {
                out.push(format!("{} would go to the web, which the user has not allowed (start the server with --allow-web).", label));
            }
            let database = matches!(n.kind.as_str(), "PostgisWriter" | "SqlExecutor")
                || (t.is_some_and(|t| t.group == Group::Writer)
                    && text_params.iter().any(|(k, v)| k.as_str() == "path" && v.trim_start().to_uppercase().starts_with("PG:")));
            if database && writes && !self.allowed.databases {
                out.push(format!("{} would write to a database, which the user has not allowed (start the server with --allow-databases).", label));
            }
            if writes && t.is_some_and(|t| t.group == Group::Writer) && !database {
                if let Some(p) = n.params.get("path").and_then(|v| v.as_str()).filter(|p| !p.trim().is_empty() && !p.contains("$(")) {
                    let target = if Path::new(p).is_absolute() { PathBuf::from(p) } else { base.join(p) };
                    let target =
                        target.parent().and_then(|d| std::fs::canonicalize(d).ok()).map(|d| d.join(target.file_name().unwrap_or_default())).unwrap_or(target);
                    if !self.allowed.folders.iter().any(|f| target.starts_with(f)) {
                        out.push(format!("{} would write {}, outside the allowed folders.", label, p));
                    } else if target.exists() && !self.may_write(&target) {
                        out.push(format!(
                            "{} would write over {}, which is there already. Give the output a new name, such as {}.",
                            label,
                            p,
                            free_name(&target)
                        ));
                    }
                }
            }
        }
        out
    }

    fn run_model(&mut self, a: &J) -> Done {
        let (path, text) = self.model_text(a)?;
        let path = path.ok_or("Save the model first with save_model, or give its path.")?;
        let mut ws = Workspace::from_json(&text).map_err(|e| format!("The model cannot be read: {}", e))?;
        if let Some(vars) = a.get("variables").and_then(|v| v.as_object()) {
            for (k, v) in vars {
                ws.set_variable(k, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()).as_str());
            }
        }
        let found = og_engine::reference::check(&ws.to_json());
        if found.iter().any(|f| f.error) {
            let mut ans = findings_answer(&ws.to_json());
            ans.text = format!("Not run: the model has errors.\n{}", ans.text);
            return Ok(ans);
        }
        let full = a.get("full").and_then(|v| v.as_bool()).unwrap_or(false);
        let base = path.parent().map(Path::to_path_buf).unwrap_or_else(|| self.allowed.folders[0].clone());
        let hindrances = self.hindrances(&ws, &base, full);
        if !hindrances.is_empty() {
            return Ok(Answer::fail(format!("Not run:\n- {}", hindrances.join("\n- "))));
        }
        let key = ws.to_json();
        if full && !self.tried.contains(&key) {
            return Ok(Answer::fail(
                "Not run in full: try it first on a sample with run_model without full, so that a mistake is found on a few features and before anything is written.",
            ));
        }
        let sample = if full { None } else { Some(a.get("sample").and_then(|v| v.as_u64()).map(|s| s.max(1) as usize).unwrap_or(TRIAL)) };
        let (run_ws, left_out) = if full { (ws.clone(), Vec::new()) } else { without_writers(&ws) };
        let mut log: Vec<String> = Vec::new();
        let result = og_engine::run_opts(
            &run_ws,
            Some(&self.backend),
            &base,
            &mut |p: Progress| {
                if let Progress::Log { entry } = p {
                    let tag = match entry.level {
                        Level::Info => "",
                        Level::Warn => "WARN ",
                        Level::Error => "ERROR ",
                    };
                    log.push(format!("{}{}", tag, scrub(&entry.msg)));
                }
            },
            og_engine::RunOptions { sample, ..Default::default() },
        );
        if !full && result.errors == 0 {
            self.tried.insert(key);
        }
        let mut text = String::new();
        text.push_str(&if full {
            format!("Ran {} in full in {}: ", self.shown(&path), fmt_duration(result.ms))
        } else {
            format!("Tried {} on the first {} features of each input, in {}: ", self.shown(&path), sample.unwrap_or(0), fmt_duration(result.ms))
        });
        text.push_str(&format!("{} errors, {} warnings.\n", result.errors, result.warnings));
        text.push_str(&nodes_text(&run_ws, &result));
        if !left_out.is_empty() {
            text.push_str(&format!(
                "\nNothing was written in the trial. In full, these would write: {}. Run with full: true when the trial looks right.\n",
                left_out.join("; ")
            ));
        }
        let files: Vec<PathBuf> = result.nodes.values().flat_map(|r| r.files.iter().cloned()).collect();
        for f in &files {
            let real = std::fs::canonicalize(f).unwrap_or_else(|_| f.clone());
            self.made.insert(real);
        }
        // Folder outputs, such as tiles and walk-throughs, are made whole.
        for n in run_ws.nodes.iter().filter(|n| og_engine::nodes::node_type(&n.kind).is_some_and(|t| t.group == Group::Writer)) {
            if let Some(p) = n.params.get("path").and_then(|v| v.as_str()) {
                let t = if Path::new(p).is_absolute() { PathBuf::from(p) } else { base.join(p) };
                if t.is_dir() {
                    if let Ok(real) = std::fs::canonicalize(&t) {
                        self.made.insert(real);
                    }
                }
            }
        }
        if full && !files.is_empty() {
            text.push_str(&format!("\nWritten: {}\n", files.iter().map(|f| self.shown(f)).collect::<Vec<_>>().join(", ")));
        }
        let problems: Vec<&String> = log.iter().filter(|l| l.starts_with("WARN") || l.starts_with("ERROR")).collect();
        if !problems.is_empty() {
            text.push_str("\nThe warnings and errors:\n");
            for l in problems.iter().take(40) {
                text.push_str(l);
                text.push('\n');
            }
        }
        text.push_str("\ninspect shows the rows of a node, and look a map or a picture of them.");
        let failed = result.errors > 0;
        self.runs.insert(path, Kept { text: run_ws.to_json(), sample, result });
        Ok(Answer { text, image: None, error: failed })
    }

    /// The latest run of a model, run as a trial if there is none of it as it is now.
    fn kept(&mut self, a: &J) -> std::result::Result<(PathBuf, String), String> {
        let (path, text) = self.model_text(a)?;
        let path = path.ok_or("Give the path of the model.")?;
        let ws = Workspace::from_json(&text).map_err(|e| format!("The model cannot be read: {}", e))?;
        let fresh = match self.runs.get(&path) {
            Some(k) => k.text != without_writers(&ws).0.to_json() && k.text != ws.to_json(),
            None => true,
        };
        let mut note = String::new();
        if fresh {
            let ans = self.run_model(&json!({"path": self.shown(&path)}))?;
            if !self.runs.contains_key(&path) {
                return Err(ans.text);
            }
            note = format!("(The model was tried first on {} features of each input.)\n", TRIAL);
        }
        Ok((path, note))
    }

    /// The node and port asked for, from the latest run.
    fn port_of<'r>(&self, run: &'r Kept, a: &J) -> std::result::Result<(String, &'r NodeResult, String, &'r Table), String> {
        let ws = Workspace::from_json(&run.text).map_err(|e| e.to_string())?;
        let id = a.get("node").and_then(|v| v.as_str()).ok_or("Give the id of a node as node.")?;
        let node = ws.nodes.iter().find(|n| n.id == id || n.name == id).ok_or_else(|| {
            format!("There is no node {}. The nodes are: {}.", id, ws.nodes.iter().map(|n| format!("{} ({})", n.id, n.label())).collect::<Vec<_>>().join(", "))
        })?;
        let r = run.result.nodes.get(&node.id).ok_or_else(|| format!("{} did not run: it is disabled, or nothing it needs ran.", node.id))?;
        if let Some(e) = &r.error {
            return Err(format!("{} failed: {}", node.label(), scrub(e)));
        }
        let (map, writer) = if r.outputs.is_empty() { (&r.inputs, true) } else { (&r.outputs, false) };
        let mut ports: Vec<&String> = map.keys().collect();
        ports.sort();
        let port = match a.get("port").and_then(|v| v.as_str()) {
            Some(p) => ports.iter().find(|x| x.as_str() == p).map(|x| x.to_string()).ok_or_else(|| {
                format!("{} has no port {} with data. It has: {}.", node.label(), p, ports.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
            })?,
            None => {
                let first = ["Output", "Passed", "Input"].iter().find(|p| ports.iter().any(|x| x == *p)).map(|p| p.to_string());
                first.or_else(|| ports.first().map(|p| p.to_string())).ok_or_else(|| format!("{} has no data.", node.label()))?
            }
        };
        let label =
            if writer { format!("{} ({}), what came in on {}", node.label(), node.id, port) } else { format!("{} ({}), {}", node.label(), node.id, port) };
        Ok((node.id.clone(), r, label, map[&port].as_ref()))
    }

    /// An output is not run in a trial: what would come into it is shown, from the node
    /// before it.
    fn before_writer(&self, a: &J) -> J {
        let mut a = a.clone();
        let Ok((_, text)) = self.model_text(&a) else { return a };
        let Ok(ws) = Workspace::from_json(&text) else { return a };
        let Some(id) = a.get("node").and_then(|v| v.as_str()).map(str::to_string) else { return a };
        let Some(n) = ws.nodes.iter().find(|n| n.id == id || n.name == id) else { return a };
        if !og_engine::nodes::node_type(&n.kind).is_some_and(|t| t.group == Group::Writer) {
            return a;
        }
        let port = a.get("port").and_then(|v| v.as_str()).unwrap_or("Input").to_string();
        if let Some(e) = ws.edges.iter().find(|e| !e.disabled && e.to.0 == n.id && e.to.1 == port) {
            a["node"] = json!(e.from.0);
            a["port"] = json!(e.from.1);
        }
        a
    }

    fn inspect(&mut self, a: &J) -> Done {
        let a = &self.before_writer(a);
        let (path, note) = self.kept(a)?;
        let run = &self.runs[&path];
        let (id, r, label, t) = self.port_of(run, a)?;
        let offset = a.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = a.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).clamp(1, 100) as usize;
        let columns: Option<Vec<String>> =
            a.get("columns").and_then(|c| c.as_array()).map(|c| c.iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
        let mut text = note;
        text.push_str(&format!(
            "{}{}\n",
            label,
            if run.sample.is_some() { format!(", in a trial on {} features of each input", run.sample.unwrap_or(0)) } else { String::new() }
        ));
        text.push_str(&summary(t, ""));
        if !t.features.is_empty() {
            let to = (offset + limit).min(t.len());
            text.push_str(&format!("\nRows {} to {} of {}:\n", offset + 1, to, t.len()));
            text.push_str(&rows_text(t, offset, limit, columns.as_deref()));
            if to < t.len() {
                text.push_str(&format!("More with offset: {}.\n", to));
            }
        }
        if !r.warnings.is_empty() {
            text.push_str(&format!("\nWarnings of {}:\n{}\n", id, r.warnings.iter().map(|w| scrub(w)).collect::<Vec<_>>().join("\n")));
        }
        let lines: Vec<String> = run.result.log.iter().filter(|l| l.node.as_deref() == Some(id.as_str())).map(|l| scrub(&l.msg)).collect();
        if !lines.is_empty() {
            text.push_str(&format!("\nThe log of {}:\n{}\n", id, lines.join("\n")));
        }
        Ok(Answer::ok(text))
    }

    /// A picture of what came out of a node: a map, a 3D view, a chart, or the picture itself.
    fn look(&mut self, a: &J) -> Done {
        let a = &self.before_writer(a);
        let (path, note) = self.kept(a)?;
        let width = a.get("width").and_then(|v| v.as_u64()).unwrap_or(1000).clamp(300, 2000);
        let (source, label, kind, already) = {
            let run = &self.runs[&path];
            let (id, r, label, t) = self.port_of(run, a)?;
            let ws = Workspace::from_json(&run.text).map_err(|e| e.to_string())?;
            // A writer has no output: what came into it is drawn, from the node before.
            let source = if r.outputs.is_empty() {
                let port = a.get("port").and_then(|v| v.as_str()).unwrap_or("Input");
                ws.edges
                    .iter()
                    .filter(|e| !e.disabled)
                    .find(|e| e.to.0 == id && e.to.1 == port)
                    .map(|e| e.from.clone())
                    .ok_or("Nothing comes into that output.")?
            } else {
                let port = r.outputs.keys().find(|p| label.ends_with(&format!(", {}", p))).cloned().unwrap_or_default();
                (id.clone(), port)
            };
            let asked = a.get("as").and_then(|v| v.as_str()).unwrap_or("auto");
            let kind = match asked {
                "auto" if t.image.is_some() => "picture",
                "auto" if t.mesh.is_some() => "3d",
                "auto" if t.features.iter().any(|f| f.geom.is_some()) || t.grid.is_some() || t.raster => "map",
                "auto" if a.get("chart").is_some() => "chart",
                "auto" => {
                    return Ok(Answer::fail(format!(
                        "{} has no geometry to draw. Give as: \"chart\" with chart: {{\"kind\": \"bars\", \"category\": …, \"value\": …}}, or use inspect.",
                        label
                    )))
                }
                "map" | "3d" | "chart" => asked,
                other => return Err(format!("as is auto, map, 3d or chart, not {}.", other)),
            };
            (source, label, kind, t.image.clone())
        };
        if kind == "picture" {
            let img = already.expect("a picture");
            let png = og_engine::session::png(&shrink(&img, width as usize)).map_err(|e| e.0)?;
            return Ok(Answer { text: format!("{}{}, a picture of {} × {} pixels.", note, label, img.width, img.height), image: Some(png), error: false });
        }
        let run = self.runs.remove(&path).expect("a run");
        let mut ws = Workspace::from_json(&run.text).map_err(|e| e.to_string())?;
        let (viewer, params, port_in, port_out) = match kind {
            "3d" => ("SceneRenderer", json!({"width": width.to_string(), "height": (width * 5 / 8).to_string()}), "Models", "Image"),
            "chart" => {
                let mut p = a.get("chart").and_then(|c| c.as_object()).cloned().unwrap_or_default();
                p.insert("width".into(), json!(width.to_string()));
                ("ChartDrawer", J::Object(p), "Input", "Chart")
            }
            _ => {
                let mut p = json!({"width": width.to_string(), "background": "#f4f1ea", "legend": "none", "scale_bar": "bottom-left", "grid": "ticks"});
                if let Some(label) = a.get("label").and_then(|v| v.as_str()) {
                    p["layers"] = json!([{"from": format!("{}:{}", source.0, source.1), "label": label}]);
                }
                ("MapRenderer", p, "Layers", "Map")
            }
        };
        let look_id = "__look";
        ws.nodes.push(serde_json::from_value(json!({"id": look_id, "type": viewer, "params": params})).map_err(|e| e.to_string())?);
        ws.edges.push(serde_json::from_value(json!({"from": [source.0, source.1], "to": [look_id, port_in]})).map_err(|e| e.to_string())?);
        let base = path.parent().map(Path::to_path_buf).unwrap_or_else(|| self.allowed.folders[0].clone());
        let fresh: HashSet<String> = [look_id.to_string()].into_iter().collect();
        let result = og_engine::run_opts(
            &ws,
            Some(&self.backend),
            &base,
            &mut |_| {},
            og_engine::RunOptions { sample: run.sample, cache: Some(&run.result.nodes), fresh: Some(fresh), everything: true, ..Default::default() },
        );
        self.runs.insert(path, run);
        let r = result.nodes.get(look_id).ok_or("The picture could not be made.")?;
        if let Some(e) = &r.error {
            return Ok(Answer::fail(format!("{} could not be drawn: {}", label, scrub(e))));
        }
        let img = r.outputs.get(port_out).and_then(|t| t.image.clone()).ok_or("The picture could not be made.")?;
        let png = og_engine::session::png(&img).map_err(|e| e.0)?;
        let what = match kind {
            "3d" => "a 3D view",
            "chart" => "a chart",
            _ => "a map, without a background map",
        };
        Ok(Answer { text: format!("{}{}, as {}.", note, label, what), image: Some(png), error: false })
    }

    // ---- Resources ----

    fn resources(&self, cursor: Option<&str>) -> J {
        let mut all: Vec<J> = vec![
            json!({"uri": "ographis://docs/AGENTS.md", "name": "AGENTS.md", "title": "How to write an Ographis model", "mimeType": "text/markdown"}),
            json!({"uri": "ographis://reference/nodes.json", "name": "nodes.json", "title": "Every node type with its ports and settings, and the functions of expressions", "mimeType": "application/json"}),
            json!({"uri": "ographis://reference/model.schema.json", "name": "model.schema.json", "title": "JSON Schema of the model file", "mimeType": "application/schema+json"}),
        ];
        for (name, _) in examples() {
            all.push(
                json!({"uri": format!("ographis://examples/{}", name), "name": name, "title": format!("Example: {}", name), "mimeType": "application/json"}),
            );
        }
        for f in &self.allowed.folders {
            for m in models_in(f) {
                let shown = self.shown(&m);
                all.push(
                    json!({"uri": format!("ographis://models/{}", shown), "name": shown, "title": format!("Model: {}", shown), "mimeType": "application/json"}),
                );
            }
        }
        let from: usize = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
        let page: Vec<J> = all.iter().skip(from).take(100).cloned().collect();
        let mut out = json!({ "resources": page });
        if from + 100 < all.len() {
            out["nextCursor"] = json!((from + 100).to_string());
        }
        out
    }

    fn read_resource(&self, uri: &str) -> std::result::Result<J, (i64, String)> {
        let text_of = |mime: &str, text: String| json!({"contents": [{"uri": uri, "mimeType": mime, "text": text}]});
        if uri == "ographis://docs/AGENTS.md" {
            return Ok(text_of("text/markdown", AGENTS_MD.to_string()));
        }
        if uri == "ographis://reference/nodes.json" {
            return Ok(text_of("application/json", serde_json::to_string_pretty(&og_engine::reference::nodes_json()).unwrap_or_default()));
        }
        if uri == "ographis://reference/model.schema.json" {
            return Ok(text_of("application/schema+json", serde_json::to_string_pretty(&og_engine::reference::model_schema()).unwrap_or_default()));
        }
        if let Some(name) = uri.strip_prefix("ographis://examples/") {
            return match examples().into_iter().find(|(n, _)| n == name) {
                Some((_, p)) => Ok(text_of("application/json", std::fs::read_to_string(p).unwrap_or_default())),
                None => Err((-32002, format!("There is no example {}.", name))),
            };
        }
        if let Some(rest) = uri.strip_prefix("ographis://models/") {
            let p = self.path(rest).map_err(|e| (-32002, e))?;
            let text = std::fs::read_to_string(&p).map_err(|_| (-32002, format!("There is no model {}.", rest)))?;
            let ws = Workspace::from_json(&text).map_err(|e| (-32002, e.to_string()))?;
            return Ok(text_of("application/json", og_engine::bugreport::without_secrets(&ws).to_json()));
        }
        Err((-32002, format!("There is no resource {}.", uri)))
    }
}

// ---- Tools that need no session ----

fn find_nodes(a: &J) -> Done {
    let query = a.get("query").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let page = page_of(a);
    let mut hits: Vec<(usize, String)> = Vec::new();
    for t in NODE_TYPES.iter().filter(|t| !t.hidden) {
        let hay = format!("{} {} {} {}", t.name, t.title, t.category, t.desc).to_lowercase();
        let score: usize = words
            .iter()
            .map(|w| {
                if t.title.to_lowercase().contains(w) || t.name.to_lowercase().contains(w) {
                    3
                } else if hay.contains(w) {
                    1
                } else {
                    0
                }
            })
            .sum();
        if !words.is_empty() && score == 0 {
            continue;
        }
        let group = match t.group {
            Group::Reader => "input",
            Group::Transformer => "tool",
            Group::Writer => "output",
        };
        let first = t.desc.split(". ").next().unwrap_or(t.desc);
        let ports = format!(
            "in: {}; out: {}",
            if t.inputs.is_empty() { "–".to_string() } else { t.inputs.join(", ") },
            if t.ports.is_some() {
                format!("one per value, and {}", t.outputs.join(", "))
            } else if t.outputs.is_empty() {
                "–".into()
            } else {
                t.outputs.join(", ")
            }
        );
        hits.push((
            score,
            format!(
                "{} · {} ({}, {}) — {}. {}",
                t.name,
                t.title,
                group,
                if t.category.is_empty() { "-" } else { t.category },
                first.trim_end_matches('.'),
                ports
            ),
        ));
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let total = hits.len();
    if total == 0 {
        return Ok(Answer::ok(format!("No node type matches {}. Try other words, or find_nodes without a query for all of them.", query)));
    }
    let (from, to) = (page * 40, ((page + 1) * 40).min(total));
    let mut text = format!("{} node types{}:\n", total, if total > 40 { format!(", page {} of {}", page + 1, total.div_ceil(40)) } else { String::new() });
    for (_, h) in hits.iter().take(to).skip(from) {
        text.push_str(h);
        text.push('\n');
    }
    if to < total {
        text.push_str(&format!("More with page: {}.\n", page + 1));
    }
    text.push_str("node_help gives the settings of one.");
    Ok(Answer::ok(text))
}

fn node_help(a: &J) -> Done {
    let ty = a.get("type").and_then(|v| v.as_str()).ok_or("Give the type, such as Reader or Tester.")?;
    let all = og_engine::reference::nodes_json();
    let types = all["node_types"].as_array().cloned().unwrap_or_default();
    match types
        .iter()
        .find(|t| t["type"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(ty)) || t["title"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(ty)))
    {
        Some(t) => Ok(Answer::ok(serde_json::to_string_pretty(t).unwrap_or_default())),
        None => {
            let close: Vec<String> = types
                .iter()
                .filter_map(|t| t["type"].as_str())
                .filter(|n| n.to_lowercase().contains(&ty.to_lowercase()) || ty.to_lowercase().contains(&n.to_lowercase()))
                .map(str::to_string)
                .collect();
            Err(format!(
                "There is no node type {}.{} find_nodes finds them by what they do.",
                ty,
                if close.is_empty() { String::new() } else { format!(" Perhaps: {}.", close.join(", ")) }
            ))
        }
    }
}

fn open_model() -> Done {
    let open = og_engine::session::open_models();
    if open.is_empty() {
        return Ok(Answer::ok("No model is open in Ographis now. save_model shows a model there, and starts Ographis if it can."));
    }
    let mut text = String::new();
    for m in &open {
        text.push_str(&format!(
            "{}{}{}{}\n{}\n\n",
            m.name,
            m.path.as_ref().map(|p| format!(", {}", p.display())).unwrap_or_else(|| ", not saved yet".into()),
            if m.dirty { ", with changes that are not saved (shown here as they are)" } else { "" },
            m.selected.as_ref().map(|s| format!(", the node {} selected", s)).unwrap_or_default(),
            m.model
        ));
    }
    Ok(Answer::ok(text))
}

/// Shows a model on the canvas: in the window that is open, or in a new one.
fn show(path: &Path) -> String {
    if std::env::var_os("OG_MCP_NO_WINDOW").is_some() {
        return String::new();
    }
    if og_engine::session::ask_to_open(path) {
        return "Shown in Ographis.".into();
    }
    let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let names = if cfg!(windows) { ["ographis.exe", "Ographis.exe"] } else { ["ographis", "og-gui"] };
    if let Some(gui) = exe.and_then(|d| names.iter().map(|n| d.join(n)).find(|p| p.is_file())) {
        if std::process::Command::new(gui)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return "Opened in Ographis.".into();
        }
    }
    "Ographis is not open to show it in.".into()
}

fn findings_answer(text: &str) -> Answer {
    let found = og_engine::reference::check(text);
    if found.is_empty() {
        return Answer::ok("The model is fine.");
    }
    let errors = found.iter().filter(|f| f.error).count();
    let mut out = format!("{} errors and {} warnings:\n", errors, found.len() - errors);
    for f in &found {
        out.push_str(&format!("- {}{}\n", if f.error { "ERROR " } else { "WARN " }, f.message));
    }
    Answer { text: out, image: None, error: errors > 0 }
}

// ---- Prompts ----

fn prompts() -> J {
    json!([
        {"name": "build_model", "title": "Build a model for a task", "description": "Builds an Ographis model step by step, tries it on a sample and shows the result",
         "arguments": [{"name": "task", "description": "What the model is to do", "required": true}, {"name": "data", "description": "The files or services to use", "required": false}]},
        {"name": "describe_data", "title": "Describe data", "description": "What is in a file or service: layers, attributes, coordinate system and a map",
         "arguments": [{"name": "path", "description": "The file or service", "required": true}]},
        {"name": "fix_model", "title": "Fix a model", "description": "Finds what is wrong in a model, by checking and trying it, and mends it",
         "arguments": [{"name": "path", "description": "The model", "required": true}]},
        {"name": "explain_model", "title": "Explain a model", "description": "Tells in plain words what a model does, step by step",
         "arguments": [{"name": "path", "description": "The model; empty for the one open in Ographis", "required": false}]}
    ])
}

fn prompt(name: &str, args: &J) -> std::result::Result<J, (i64, String)> {
    let arg = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let text = match name {
        "build_model" => format!(
            "Build an Ographis model for this task: {}\n{}\nWork in steps: describe each file first; find the tools with find_nodes and read their settings with node_help, as the names must be exact; \
             start from an example in ographis://examples/ when one is close; save the model with save_model under a new name, so that I see it on the canvas; \
             try it with run_model (it reads only the first 100 features of each input and writes nothing), look at the result with inspect and look, and mend it until it is right. \
             Then tell me what it does and ask before you run it in full.",
            arg("task"),
            if arg("data").is_empty() { String::new() } else { format!("The data: {}", arg("data")) }
        ),
        "describe_data" => format!(
            "Describe {} for me with describe: what it holds, its attributes and what they seem to mean, its coordinate system, extent and number of features. \
             Then make a model that only reads it, try it, and show it as a map with look.",
            arg("path")
        ),
        "fix_model" => format!(
            "The model {} does not work as it should. Check it with check_model, try it with run_model, and look at the nodes that fail or give nothing with inspect. \
             Tell me what is wrong, and save a mended model under a new name with save_model.",
            arg("path")
        ),
        "explain_model" => format!(
            "Explain {} in plain words: what it reads, what each step does and why, and what it writes. Read it with {}.",
            if arg("path").is_empty() { "the model open in Ographis".to_string() } else { arg("path") },
            if arg("path").is_empty() { "open_model" } else { "read_model" }
        ),
        other => return Err((-32602, format!("There is no prompt {}.", other))),
    };
    Ok(json!({"messages": [{"role": "user", "content": {"type": "text", "text": text}}]}))
}

// ---- The list of tools ----

fn tools() -> J {
    let path = json!({"type": "string", "description": "A path within the allowed folders; relative ones start from the first of them"});
    let model_ref = json!({"type": "object", "properties": {"path": path, "model": {"description": "The model itself, as JSON, instead of a path"}}});
    let node_ref = json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "The model"},
        "node": {"type": "string", "description": "The id of the node"},
        "port": {"type": "string", "description": "The port; by default the first output, or what came into an output"}
    }, "required": ["path", "node"]});
    let mut inspect = node_ref.clone();
    inspect["properties"]["offset"] = json!({"type": "integer", "description": "The first row, from 0"});
    inspect["properties"]["limit"] = json!({"type": "integer", "description": "How many rows, at most 100 (20)"});
    inspect["properties"]["columns"] = json!({"type": "array", "items": {"type": "string"}, "description": "Only these attributes"});
    let mut look = node_ref.clone();
    look["properties"]["as"] = json!({"type": "string", "enum": ["auto", "map", "3d", "chart"], "description": "How to draw it (auto)"});
    look["properties"]["chart"] = json!({"type": "object", "description": "Settings of Draw chart (ChartDrawer), such as {\"kind\": \"bars\", \"category\": \"municipality\", \"value\": \"population\"}"});
    look["properties"]["width"] = json!({"type": "integer", "description": "Width in pixels (1000)"});
    look["properties"]["label"] = json!({"type": "string", "description": "An attribute to write beside the features on a map"});
    let ro = json!({"readOnlyHint": true, "openWorldHint": false});
    json!([
        {"name": "list_files", "title": "List files", "description": "Lists the files of data and the models in an allowed folder and its subfolders.",
         "inputSchema": {"type": "object", "properties": {"folder": path, "page": {"type": "integer"}}}, "annotations": ro},
        {"name": "describe", "title": "Describe data", "description": "A file or service in short: layers, attributes and their types, coordinate system, extent, number of features and a few rows. The data stays in Ographis.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "description": "A file, or the address of a service when the web is allowed"}, "layer": {"type": "string"}, "rows": {"type": "integer", "description": "Rows to show, at most 20 (5)"}, "params": {"type": "object", "description": "More settings of Input, such as x, y and crs for a CSV"}}, "required": ["path"]}, "annotations": ro},
        {"name": "find_nodes", "title": "Find tools", "description": "Finds node types (inputs, tools, outputs) by words of what they do, with their ports.",
         "inputSchema": {"type": "object", "properties": {"query": {"type": "string", "description": "Words, such as buffer, join by location or geopackage"}, "page": {"type": "integer"}}}, "annotations": ro},
        {"name": "node_help", "title": "Settings of a tool", "description": "Everything about one node type: its ports and its settings with their keys, kinds, choices and defaults.",
         "inputSchema": {"type": "object", "properties": {"type": {"type": "string"}}, "required": ["type"]}, "annotations": ro},
        {"name": "read_model", "title": "Read a model", "description": "The JSON of a model, without passwords and keys.", "inputSchema": model_ref, "annotations": ro},
        {"name": "open_model", "title": "The model open in Ographis", "description": "The model the user has open in Ographis now, with changes that are not saved, and the node selected.",
         "inputSchema": {"type": "object", "properties": {}}, "annotations": ro},
        {"name": "check_model", "title": "Check a model", "description": "Checks a model without running it: node types, ports, connections, settings and expressions. Says what is wrong and what would be right.",
         "inputSchema": model_ref, "annotations": ro},
        {"name": "save_model", "title": "Save a model", "description": "Saves a model, checks it and shows it on the canvas of Ographis. A file that is there already is not written over unless this session made it.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "description": "Ending in .ographis"}, "model": {"description": "The model as JSON"}, "show": {"type": "boolean", "description": "Show it in Ographis (true)"}}, "required": ["path", "model"]},
         "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}},
        {"name": "run_model", "title": "Run a model", "description": "Tries a model on the first features of each input, writing nothing (sample, 100); with full: true runs it in full and writes its outputs, once a trial has gone well. Says how each node went.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "sample": {"type": "integer", "description": "Features read from each input in a trial (100)"}, "full": {"type": "boolean"}, "variables": {"type": "object", "description": "Values of variables of the model"}}, "required": ["path"]},
         "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false}},
        {"name": "inspect", "title": "Look at the rows of a node", "description": "The rows, attributes, figures, warnings and log of what came out of a node in the latest run, a page at a time.",
         "inputSchema": inspect, "annotations": ro},
        {"name": "look", "title": "See what came out", "description": "A picture of what came out of a node: a map of features and rasters, a 3D view of models, a chart, or the picture itself.",
         "inputSchema": look, "annotations": ro}
    ])
}

// ---- Helpers ----

fn page_of(a: &J) -> usize {
    a.get("page").and_then(|v| v.as_u64()).unwrap_or(0) as usize
}

fn cut(text: &str) -> String {
    if text.len() <= MOST_TEXT {
        return text.to_string();
    }
    let mut end = MOST_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… cut here; ask for less, such as with limit, columns or page.", &text[..end])
}

fn scrub(text: &str) -> String {
    og_engine::bugreport::scrub(text)
}

fn size_text(n: u64) -> String {
    match n {
        0 => String::new(),
        n if n < 1024 => format!("{} B", n),
        n if n < 1024 * 1024 => format!("{} kB", n / 1024),
        n => format!("{:.1} MB", n as f64 / 1024.0 / 1024.0),
    }
}

fn known_file(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(
        ext.as_str(),
        "ographis"
            | "gpkg"
            | "shp"
            | "geojson"
            | "json"
            | "csv"
            | "txt"
            | "tsv"
            | "xlsx"
            | "kml"
            | "kmz"
            | "gml"
            | "parquet"
            | "fgb"
            | "tif"
            | "tiff"
            | "asc"
            | "hgt"
            | "las"
            | "laz"
            | "ifc"
            | "gltf"
            | "glb"
            | "obj"
            | "dxf"
            | "dwg"
            | "zip"
            | "sqlite"
            | "gpx"
            | "xml"
            | "jp2"
            | "png"
            | "jpg"
            | "stl"
            | "3mf"
            | "step"
            | "stp"
            | "pmtiles"
            | "mbtiles"
    )
}

/// A name beside a file that is not taken: house.gpkg becomes house-2.gpkg.
fn free_name(p: &Path) -> String {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for k in 2..1000 {
        let c = p.with_file_name(format!("{}-{}{}", stem, k, ext));
        if !c.exists() {
            return c.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
    }
    format!("{}-new{}", stem, ext)
}

/// The model without its outputs, for a trial; and what they would have written.
fn without_writers(ws: &Workspace) -> (Workspace, Vec<String>) {
    let mut ws = ws.clone();
    let writers: HashSet<String> = ws
        .nodes
        .iter()
        .filter(|n| !n.disabled && og_engine::nodes::node_type(&n.kind).is_some_and(|t| t.group == Group::Writer))
        .map(|n| n.id.clone())
        .collect();
    let left: Vec<String> = ws
        .nodes
        .iter()
        .filter(|n| writers.contains(&n.id))
        .map(|n| format!("{} ({})", n.label(), scrub(n.params.get("path").and_then(|v| v.as_str()).unwrap_or(n.kind.as_str()))))
        .collect();
    ws.nodes.retain(|n| !writers.contains(&n.id));
    ws.edges.retain(|e| !writers.contains(&e.to.0) && !writers.contains(&e.from.0));
    (ws, left)
}

/// Places the nodes from left to right in the order the data flows, when they all stand at
/// the same place, as when an assistant gives no places.
fn lay_out(ws: &mut Workspace) {
    let placed = ws.nodes.iter().filter(|n| n.x != 0.0 || n.y != 0.0).count();
    if placed > 0 || ws.nodes.len() < 2 {
        return;
    }
    let mut depth: HashMap<String, usize> = ws.nodes.iter().map(|n| (n.id.clone(), 0)).collect();
    for _ in 0..ws.nodes.len() {
        for e in &ws.edges {
            let d = depth.get(&e.from.0).copied().unwrap_or(0) + 1;
            if let Some(t) = depth.get_mut(&e.to.0) {
                *t = (*t).max(d);
            }
        }
    }
    let mut rows: HashMap<usize, usize> = HashMap::new();
    for n in ws.nodes.iter_mut() {
        let d = depth[&n.id];
        let row = rows.entry(d).or_default();
        n.x = 40.0 + 280.0 * d as f64;
        n.y = 40.0 + 170.0 * *row as f64;
        *row += 1;
    }
}

/// How each node of a run went, one line each.
fn nodes_text(ws: &Workspace, r: &RunResult) -> String {
    let mut out = String::new();
    for id in &r.order {
        let Some(n) = ws.nodes.iter().find(|n| &n.id == id) else { continue };
        let Some(res) = r.nodes.get(id) else { continue };
        let status = match res.status {
            Status::Ok => "ok",
            Status::Warn => "warning",
            Status::Error => "FAILED",
            _ => "-",
        };
        let mut ports: Vec<(&String, usize)> = res.outputs.iter().map(|(p, t)| (p, t.len())).collect();
        ports.sort();
        let counts = ports.iter().map(|(p, c)| format!("{} {}", p, fmt_count(*c as u64))).collect::<Vec<_>>().join(", ");
        out.push_str(&format!(
            "- {} ({}, {}): {}{}{}\n",
            n.id,
            n.kind,
            n.label(),
            status,
            if counts.is_empty() { String::new() } else { format!(" · {}", counts) },
            res.error.as_ref().map(|e| format!(" · {}", scrub(e))).unwrap_or_default()
        ));
    }
    out
}

/// A table in short: what it holds, its attributes with figures, its extent.
fn summary(t: &Table, of: &str) -> String {
    let mut out = String::new();
    if let Some(img) = &t.image {
        out.push_str(&format!("A picture of {} × {} pixels, {}.\n", img.width, img.height, crs_label(t.crs)));
    }
    if let Some(g) = &t.grid {
        out.push_str(&format!(
            "A grid of {} × {} cells of {} m, {}.\n",
            g.width,
            g.height,
            og_engine::model::fmt_real(g.geotransform[1].abs()),
            crs_label(t.crs)
        ));
    }
    if let Some(m) = &t.mesh {
        out.push_str(&format!("A 3D model of {} triangles in {} objects.\n", fmt_count(m.tris.len() as u64), fmt_count(m.objects.len() as u64)));
    }
    if t.image.is_none() && t.grid.is_none() {
        let kinds: std::collections::BTreeMap<String, usize> = t.features.iter().fold(Default::default(), |mut m, f| {
            *m.entry(f.geom.as_ref().map(og_engine::geom::type_name).unwrap_or("no geometry").to_string()).or_default() += 1;
            m
        });
        out.push_str(&format!(
            "{} features{} · {} · {}\n",
            fmt_count(t.len() as u64),
            of,
            kinds.iter().map(|(k, c)| format!("{} {}", fmt_count(*c as u64), k)).collect::<Vec<_>>().join(", "),
            crs_label(t.crs)
        ));
        if let Some((lo, hi)) = extent(t) {
            out.push_str(&format!("Extent: {:.6}, {:.6} to {:.6}, {:.6}\n", lo[0], lo[1], hi[0], hi[1]));
        }
    }
    if !t.fields.is_empty() {
        out.push_str("Attributes:\n");
        for (k, f) in t.fields.iter().enumerate() {
            out.push_str(&format!("- {} ({}){}\n", f.name, f.ty.label(), figures(t, k)));
        }
    }
    out
}

fn extent(t: &Table) -> Option<([f64; 2], [f64; 2])> {
    use geo::BoundingRect;
    let mut b: Option<([f64; 2], [f64; 2])> = None;
    for r in t.features.iter().filter_map(|f| f.geom.as_ref()?.bounding_rect()) {
        b = Some(match b {
            None => ([r.min().x, r.min().y], [r.max().x, r.max().y]),
            Some((lo, hi)) => ([lo[0].min(r.min().x), lo[1].min(r.min().y)], [hi[0].max(r.max().x), hi[1].max(r.max().y)]),
        });
    }
    b
}

/// The figures of an attribute: empty values, the range of numbers, the most common texts.
fn figures(t: &Table, k: usize) -> String {
    let vals: Vec<&Value> = t.features.iter().filter_map(|f| f.attrs.get(k)).collect();
    if vals.is_empty() {
        return String::new();
    }
    let empty = vals.iter().filter(|v| matches!(v, Value::Null)).count();
    let nums: Vec<f64> = vals
        .iter()
        .filter_map(|v| match v {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            _ => None,
        })
        .collect();
    let mut parts = Vec::new();
    if empty > 0 {
        parts.push(format!("{} empty", fmt_count(empty as u64)));
    }
    if !nums.is_empty() {
        let (lo, hi) = nums.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
        let mean = nums.iter().sum::<f64>() / nums.len() as f64;
        parts.push(format!(
            "{} to {}, mean {}",
            og_engine::model::fmt_real(lo),
            og_engine::model::fmt_real(hi),
            og_engine::model::fmt_real((mean * 100.0).round() / 100.0)
        ));
    } else {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for v in vals.iter().filter(|v| !matches!(v, Value::Null)) {
            *counts.entry(short(&v.to_text(), 30)).or_default() += 1;
        }
        let mut top: Vec<(String, usize)> = counts.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        if !top.is_empty() {
            parts.push(format!(
                "{} different{}",
                fmt_count(top.len() as u64),
                if top.len() > 1 {
                    format!(", most often {}", top.iter().take(3).map(|(v, c)| format!("'{}' {}", v, c)).collect::<Vec<_>>().join(", "))
                } else {
                    format!(": '{}'", top[0].0)
                }
            ));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(": {}", parts.join("; "))
    }
}

fn short(s: &str, most: usize) -> String {
    let s = s.replace(['\n', '\r', '\t'], " ");
    if s.chars().count() <= most {
        s
    } else {
        format!("{}…", s.chars().take(most).collect::<String>())
    }
}

/// Rows as a table of text, the geometry in short.
fn rows_text(t: &Table, offset: usize, limit: usize, columns: Option<&[String]>) -> String {
    let idx: Vec<usize> = match columns {
        Some(c) => c.iter().filter_map(|n| t.fields.iter().position(|f| &f.name == n)).collect(),
        None => (0..t.fields.len()).collect(),
    };
    let mut out = String::new();
    out.push_str(&format!("# | {} | geometry\n", idx.iter().map(|i| t.fields[*i].name.as_str()).collect::<Vec<_>>().join(" | ")));
    for (k, f) in t.features.iter().enumerate().skip(offset).take(limit) {
        let cells: Vec<String> = idx.iter().map(|i| f.attrs.get(*i).map(|v| short(&scrub(&v.to_text()), 40)).unwrap_or_default()).collect();
        let g = f
            .geom
            .as_ref()
            .map(|g| {
                short(
                    &{
                        use wkt::ToWkt;
                        g.wkt_string()
                    },
                    60,
                )
            })
            .unwrap_or_default();
        out.push_str(&format!("{} | {} | {}\n", k, cells.join(" | "), g));
    }
    out
}

fn shrink(img: &og_engine::model::RasterImage, width: usize) -> og_engine::model::RasterImage {
    if img.width <= width {
        return img.clone();
    }
    let h = (img.height * width / img.width).max(1);
    let mut rgba = vec![0u8; width * h * 4];
    for y in 0..h {
        for x in 0..width {
            let (sx, sy) = (x * img.width / width, y * img.height / h);
            let s = (sy * img.width + sx) * 4;
            rgba[(y * width + x) * 4..][..4].copy_from_slice(&img.rgba[s..s + 4]);
        }
    }
    og_engine::model::RasterImage { width, height: h, rgba, ..img.clone() }
}

/// The example models, by their path from the folder of the examples: beside the program, or
/// where it was built from.
fn examples() -> Vec<(String, PathBuf)> {
    let mut places = Vec::new();
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        places.push(d.join("examples"));
        places.push(d.join("../examples"));
        places.push(d.join("../../examples"));
    }
    places.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples")));
    let Some(root) = places.into_iter().find(|p| p.is_dir() && !models_in(p).is_empty()) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> =
        models_in(&root).into_iter().filter_map(|p| Some((p.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/"), p))).collect();
    out.sort();
    out
}

fn models_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if p.is_dir() && depth < 3 && !name.starts_with('.') && !name.ends_with(".runs") && name != "out" && name != "target" {
                stack.push((p, depth + 1));
            } else if p.extension().is_some_and(|x| x == "ographis") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
