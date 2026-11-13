//! `og mcp` as an assistant uses it: the protocol, the typical tasks of an assistant, done
//! with the tools as an assistant would call them, and the limits that keep the user's
//! computer safe. The tasks are in `tests/mcp_tasks.json`, which the evaluation of real
//! assistants in `tools/mcp-eval` uses too.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Session {
    fn start(folder: &Path, extra: &[&str]) -> Session {
        let settings = folder.join(".settings");
        let mut child = Command::new(env!("CARGO_BIN_EXE_og"))
            .arg("mcp")
            .arg("--folder")
            .arg(folder)
            .args(extra)
            .env("OG_MCP_NO_WINDOW", "1")
            .env("OGRAPHIS_SETTINGS_DIR", &settings)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("og mcp starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut s = Session { child, stdin, stdout, next: 0 };
        let init = s.ask("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        s.tell("notifications/initialized");
        s
    }

    fn tell(&mut self, method: &str) {
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "method": method})).unwrap();
    }

    fn ask(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params})).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{}: {}", e, line));
        assert_eq!(v["id"], json!(self.next), "the answer to the question asked");
        v
    }

    /// Calls a tool: its text, and whether it is an error.
    fn tool(&mut self, name: &str, args: Value) -> (String, bool, Value) {
        let v = self.ask("tools/call", json!({"name": name, "arguments": args}));
        let r = &v["result"];
        let text = r["content"][0]["text"].as_str().unwrap_or("").to_string();
        (text, r["isError"].as_bool().unwrap_or(false), r.clone())
    }

    fn ok(&mut self, name: &str, args: Value) -> String {
        let (text, error, _) = self.tool(name, args.clone());
        assert!(!error, "{} {} failed: {}", name, args, text);
        text
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("og-mcp-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for f in ["towns.csv", "blocks.geojson", "municipalities.csv"] {
        std::fs::copy(examples.join(f), dir.join(f)).unwrap();
    }
    dir
}

/// The model of the first task: towns of more than 10 000 inhabitants to a GeoPackage.
fn large_towns() -> Value {
    json!({"format": "ographis/1", "name": "Large towns", "nodes": [
        {"id": "towns", "type": "Reader", "params": {"path": "towns.csv", "x": "lon", "y": "lat", "crs": "EPSG:4326"}},
        {"id": "big", "type": "Tester", "params": {"test": "population > 10000"}},
        {"id": "out", "type": "Writer", "params": {"path": "large_towns.gpkg"}}],
      "edges": [{"from": ["towns", "Output"], "to": ["big", "Input"]}, {"from": ["big", "Passed"], "to": ["out", "Input"]}]})
}

#[test]
fn the_protocol_is_spoken() {
    let dir = folder("protocol");
    let mut s = Session::start(&dir, &[]);
    let tools = s.ask("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    for n in ["list_files", "describe", "find_nodes", "node_help", "read_model", "open_model", "check_model", "save_model", "run_model", "inspect", "look"] {
        assert!(names.contains(&n), "{} is offered", n);
    }
    for t in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
    }
    assert_eq!(s.ask("ping", json!({}))["result"], json!({}));
    let unknown = s.ask("no/such", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    // An older version of the protocol is answered in it.
    let mut old = Session::start(&dir, &[]);
    let again = old.ask("initialize", json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}));
    assert_eq!(again["result"]["protocolVersion"], "2024-11-05");
    // Resources: the guide, the reference, the examples and the models of the folder.
    let res = s.ask("resources/list", json!({}));
    let uris: Vec<String> = res["result"]["resources"].as_array().unwrap().iter().filter_map(|r| r["uri"].as_str().map(str::to_string)).collect();
    assert!(uris.contains(&"ographis://docs/AGENTS.md".to_string()));
    assert!(uris.iter().any(|u| u == "ographis://examples/skaraborg.ographis"), "{:?}", &uris[..uris.len().min(8)]);
    let guide = s.ask("resources/read", json!({"uri": "ographis://docs/AGENTS.md"}));
    assert!(guide["result"]["contents"][0]["text"].as_str().unwrap().contains("Writing a model"));
    let nodes = s.ask("resources/read", json!({"uri": "ographis://reference/nodes.json"}));
    let parsed: Value = serde_json::from_str(nodes["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert!(parsed["node_types"].as_array().unwrap().len() > 100);
    let example = s.ask("resources/read", json!({"uri": "ographis://examples/skaraborg.ographis"}));
    assert!(example["result"]["contents"][0]["text"].as_str().unwrap().contains("\"nodes\""));
    // Prompts for the common tasks.
    let prompts = s.ask("prompts/list", json!({}));
    assert!(prompts["result"]["prompts"].as_array().unwrap().len() >= 4);
    let p = s.ask("prompts/get", json!({"name": "build_model", "arguments": {"task": "towns over 10 000"}}));
    assert!(p["result"]["messages"][0]["content"]["text"].as_str().unwrap().contains("towns over 10 000"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_assistant_describes_builds_tries_looks_and_runs_a_model() {
    let dir = folder("task");
    let mut s = Session::start(&dir, &[]);
    // What there is, and what is in it.
    let files = s.ok("list_files", json!({}));
    assert!(files.contains("towns.csv") && files.contains("blocks.geojson"), "{}", files);
    let d = s.ok("describe", json!({"path": "towns.csv"}));
    assert!(d.contains("39 features") && d.contains("population (integer): 2000 to 600000") && d.contains("Göteborg"), "{}", d);
    let d = s.ok("describe", json!({"path": "blocks.geojson", "rows": 2}));
    assert!(d.contains("12 features") && d.contains("EPSG:3006") && d.contains("Extent:"), "{}", d);
    // The tools, found by what they do, and their settings.
    let found = s.ok("find_nodes", json!({"query": "test split"}));
    assert!(found.lines().nth(1).unwrap_or("").contains("Tester") || found.contains("Tester · Test"), "{}", found);
    let help = s.ok("node_help", json!({"type": "Tester"}));
    assert!(help.contains("\"Passed\"") && help.contains("\"test\""), "{}", help);
    let (wrong, error, _) = s.tool("node_help", json!({"type": "Filter"}));
    assert!(error && wrong.contains("find_nodes"), "{}", wrong);
    // A model with a mistake: told what would be right.
    let mut broken = large_towns();
    broken["edges"][1]["from"][1] = json!("Output");
    let (checked, error, _) = s.tool("check_model", json!({"model": broken}));
    assert!(error && checked.contains("Passed"), "{}", checked);
    // Saved, tried on a sample, writing nothing.
    let saved = s.ok("save_model", json!({"path": "large_towns.ographis", "model": large_towns()}));
    assert!(saved.contains("Saved") && saved.contains("fine"), "{}", saved);
    let saved_model: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("large_towns.ographis")).unwrap()).unwrap();
    assert!(saved_model["nodes"][2]["x"].as_f64().unwrap() > 0.0, "laid out on the canvas");
    let (refused, error, _) = s.tool("run_model", json!({"path": "large_towns.ographis", "full": true}));
    assert!(error && refused.contains("try it first"), "{}", refused);
    let trial = s.ok("run_model", json!({"path": "large_towns.ographis", "sample": 10}));
    assert!(trial.contains("first 10 features") && trial.contains("Nothing was written"), "{}", trial);
    assert!(!dir.join("large_towns.gpkg").exists(), "a trial writes nothing");
    // The rows of a node, a page at a time, and a map of them.
    let rows = s.ok("inspect", json!({"path": "large_towns.ographis", "node": "big", "port": "Passed", "limit": 2}));
    assert!(rows.contains("Rows 1 to 2") && rows.contains("More with offset: 2"), "{}", rows);
    let (text, error, r) = s.tool("look", json!({"path": "large_towns.ographis", "node": "out", "label": "name"}));
    assert!(!error, "{}", text);
    assert_eq!(r["content"][1]["mimeType"], "image/png");
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.decode(r["content"][1]["data"].as_str().unwrap()).unwrap();
    assert!(png.starts_with(b"\x89PNG") && png.len() > 5000, "a map");
    // In full: written, and it may be written again by the same session.
    let full = s.ok("run_model", json!({"path": "large_towns.ographis", "full": true}));
    assert!(full.contains("Passed 9") || full.contains("Written"), "{}", full);
    assert!(dir.join("large_towns.gpkg").exists());
    s.ok("run_model", json!({"path": "large_towns.ographis", "full": true}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_computer_of_the_user_is_kept_safe() {
    let dir = folder("safe");
    std::fs::write(dir.join("theirs.gpkg"), b"not to be written over").unwrap();
    let mut s = Session::start(&dir, &[]);
    // Outside the folders.
    let (t, e, _) = s.tool("describe", json!({"path": "../../etc/passwd"}));
    assert!(e && t.contains("outside the folders"), "{}", t);
    let (t, e, _) = s.tool("save_model", json!({"path": "/tmp/elsewhere.ographis", "model": large_towns()}));
    assert!(e && t.contains("outside"), "{}", t);
    // Not written over: a model, and an output.
    std::fs::write(dir.join("mine.ographis"), "{}").unwrap();
    let (t, e, _) = s.tool("save_model", json!({"path": "mine.ographis", "model": large_towns()}));
    assert!(e && t.contains("mine-2.ographis"), "{}", t);
    let mut m = large_towns();
    m["nodes"][2]["params"]["path"] = json!("theirs.gpkg");
    s.ok("save_model", json!({"path": "over.ographis", "model": m}));
    s.ok("save_model", json!({"path": "over.ographis", "model": m}));
    s.ok("run_model", json!({"path": "over.ographis"}));
    let (t, e, _) = s.tool("run_model", json!({"path": "over.ographis", "full": true}));
    assert!(e && t.contains("write over theirs.gpkg"), "{}", t);
    assert_eq!(std::fs::read(dir.join("theirs.gpkg")).unwrap(), b"not to be written over");
    // Not the web, nor a database, without leave.
    let web = json!({"format": "ographis/1", "nodes": [{"id": "w", "type": "Reader", "params": {"path": "https://example.com/a.geojson"}}]});
    s.ok("save_model", json!({"path": "web.ographis", "model": web}));
    let (t, e, _) = s.tool("run_model", json!({"path": "web.ographis"}));
    assert!(e && t.contains("web"), "{}", t);
    let db = json!({"format": "ographis/1", "nodes": [
        {"id": "towns", "type": "Reader", "params": {"path": "towns.csv", "x": "lon", "y": "lat", "crs": "EPSG:4326"}},
        {"id": "db", "type": "Writer", "params": {"path": "PG:host=h dbname=d password=hemligt", "layer": "towns"}}],
      "edges": [{"from": ["towns", "Output"], "to": ["db", "Input"]}]});
    s.ok("save_model", json!({"path": "db.ographis", "model": db}));
    s.ok("run_model", json!({"path": "db.ographis"}));
    let (t, e, _) = s.tool("run_model", json!({"path": "db.ographis", "full": true}));
    assert!(e && t.contains("database"), "{}", t);
    // Passwords and keys never reach the assistant.
    let read = s.ok("read_model", json!({"path": "db.ographis"}));
    assert!(!read.contains("hemligt") && read.contains("(taken out)"), "{}", read);
    let (t, _, _) = s.tool("run_model", json!({"path": "db.ographis"}));
    assert!(!t.contains("hemligt"), "{}", t);
    let res = s.ask("resources/read", json!({"uri": "ographis://models/db.ographis"}));
    assert!(!res.to_string().contains("hemligt"));
    // With leave, the web is reached.
    let mut allowed = Session::start(&dir, &["--allow-web"]);
    let (t, _, _) = allowed.tool("run_model", json!({"path": "web.ographis"}));
    assert!(!t.contains("not allowed"), "{}", t);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The typical tasks, each solved with the tools as an assistant would; the same checks judge
/// the work of real assistants in tools/mcp-eval.
#[test]
fn the_typical_tasks_can_be_done_with_the_tools() {
    let tasks: Value = serde_json::from_str(include_str!("mcp_tasks.json")).unwrap();
    for task in tasks["tasks"].as_array().unwrap() {
        let name = task["name"].as_str().unwrap();
        let dir = folder(name);
        let mut s = Session::start(&dir, &[]);
        for step in task["solution"].as_array().unwrap() {
            let (tool, args) = (step["tool"].as_str().unwrap(), step["arguments"].clone());
            let text = s.ok(tool, args);
            if let Some(want) = step["expect"].as_str() {
                assert!(text.contains(want), "{}: {} should say {}: {}", name, tool, want, text);
            }
        }
        for check in task["checks"].as_array().unwrap() {
            judge(&dir, check).unwrap_or_else(|e| panic!("{}: {}", name, e));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A check of the outcome of a task: a file there, with so many features.
fn judge(dir: &Path, check: &Value) -> Result<(), String> {
    let file = dir.join(check["file"].as_str().unwrap());
    if !file.exists() {
        return Err(format!("{} was not written", file.display()));
    }
    let info = || String::from_utf8_lossy(&Command::new(env!("CARGO_BIN_EXE_og")).arg("info").arg(&file).output().unwrap().stdout).into_owned();
    if let Some(n) = check["features"].as_u64() {
        let text = info();
        if !text.contains(&format!("{} features", n)) {
            return Err(format!("{} should have {} features: {}", file.display(), n, text));
        }
    }
    if let Some(crs) = check["crs"].as_str() {
        let text = info();
        if !text.contains(crs) {
            return Err(format!("{} should be in {}: {}", file.display(), crs, text));
        }
    }
    if let Some(word) = check["contains"].as_str() {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if !text.contains(word) {
            return Err(format!("{} should contain {}", file.display(), word));
        }
    }
    Ok(())
}

/// The extension of Claude Desktop and the entry of the MCP registry name the tools the
/// server has, and the version of the program.
#[test]
fn the_extension_and_the_registry_entry_match_the_server() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(root.join("mcp/manifest.json")).unwrap()).unwrap();
    let server: Value = serde_json::from_str(&std::fs::read_to_string(root.join("mcp/server.json")).unwrap()).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(manifest["version"], version);
    assert_eq!(server["version"], version);
    let id = server["packages"][0]["identifier"].as_str().unwrap();
    assert!(id.contains(&format!("v{}/Ographis-{}-windows.mcpb", version, version)), "{}", id);
    assert!(server["description"].as_str().unwrap().len() <= 100);
    let dir = folder("manifest");
    let mut s = Session::start(&dir, &[]);
    let tools = s.ask("tools/list", json!({}));
    let mut offered: Vec<String> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    let mut listed: Vec<String> = manifest["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    offered.sort();
    listed.sort();
    assert_eq!(offered, listed);
    let _ = std::fs::remove_dir_all(&dir);
}
