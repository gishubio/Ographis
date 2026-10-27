//! Run model: runs another model from this one, so that work can be split into models of
//! their own, each easy to follow, and run as a chain. By default this model waits until
//! the other has run; what comes after Run model runs after it. The lines of the other
//! model come in the log of this one, under the name of the other.

use super::*;
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use crate::run::{Level, Progress, RunOptions, Status, Workspace};
use std::cell::RefCell;
use std::path::PathBuf;

pub(super) const WAIT: &[(&str, &str)] = &[("true", "Wait until it has run, then go on"), ("false", "Start it and go on at once")];
pub(super) const ON_ERROR: &[(&str, &str)] = &[("stop", "Stop here too"), ("go", "Go on all the same")];

pub(super) const RUNNER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "Model",
        kind: ParamKind::FileOpen,
        default: "",
        hint: "The model to run, an .ographis file (or .json, as before 0.75). Paths in it start from its own folder",
    },
    ParamSpec {
        key: "wait",
        label: "This model",
        kind: ParamKind::Choice(WAIT),
        default: "true",
        hint: "Started without waiting, it runs by itself with og, and writes its log beside it",
    },
    ParamSpec {
        key: "variables",
        label: "Variables",
        kind: ParamKind::Text,
        default: "",
        hint: "Values for variables of the other model: year=2026; county=14. $(name) gives a variable of this model",
    },
    ParamSpec {
        key: "on_error",
        label: "If it fails",
        kind: ParamKind::Choice(ON_ERROR),
        default: "stop",
        hint: "Whether what comes after Run model runs when a node of the other model fails",
    },
];

thread_local! {
    /// The other models a run is inside, so that a model that runs itself, also by a
    /// detour, is caught rather than run without end.
    static RUNNING: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

pub(super) const DEEPEST: usize = 8;

pub(super) fn variables(text: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once('=').ok_or_else(|| GbError(format!("\"{}\" is not a variable and a value. Write them as year=2026.", part)))?;
        let k = k.trim();
        if !crate::run::is_variable_name(k) {
            return err(format!("\"{}\" cannot be the name of a variable: letters, digits and _, not starting with a digit.", k));
        }
        out.push((k.to_string(), v.trim().to_string()));
    }
    Ok(out)
}

pub(super) fn run_model(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let path = p.need("path", "Choose the model to run.")?;
    let file = ctx.resolve(&path);
    let full = file.canonicalize().map_err(|_| GbError(format!("The model {} is not there.", file.display())))?;
    let vars = variables(&p.get("variables").unwrap_or_default())?;
    let mut ws = Workspace::load(&full)?;
    let name = if ws.name.trim().is_empty() { full.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { ws.name.clone() };
    for (k, v) in &vars {
        match ws.variables.iter_mut().find(|x| x.name == *k) {
            Some(x) => x.value = v.clone(),
            None => ctx.notes.warn(format!("{} has no variable {}; it is given to it all the same.", name, k)),
        }
        if !ws.variables.iter().any(|x| x.name == *k) {
            ws.variables.push(crate::run::Variable { name: k.clone(), value: v.clone() });
        }
    }
    let base = full.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| ctx.base_dir.to_path_buf());

    if p.get("wait").as_deref() == Some("false") {
        return start_alone(&full, &name, &vars, ctx);
    }

    // A model that runs itself, also through others, would never end.
    let chain = RUNNING.with(|r| r.borrow().clone());
    if chain.contains(&full) {
        let names: Vec<String> =
            chain.iter().chain(std::iter::once(&full)).map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
        return err(format!("The models run each other in a ring: {}. It would never end.", names.join(" → ")));
    }
    if chain.len() >= DEEPEST {
        return err(format!("Models run models {} deep. That is as deep as it goes.", DEEPEST));
    }

    let say = ctx.say;
    say(Level::Info, &format!("→ {}: runs {}", ctx.node_name, name));
    let total = ws.nodes.len() as u64;
    let done = std::cell::Cell::new(0u64);
    let progress = ctx.progress;
    let depth = chain.len() + 1;
    let indent = "  ".repeat(depth);
    let mut report = |pr: Progress| match pr {
        // Its first and last lines are said here in a line of this model's own.
        Progress::Log { entry } if entry.node.is_none() && (entry.msg.starts_with("Run") || entry.msg.starts_with("Running")) => {}
        Progress::Log { entry } => {
            // The lines of the other model, under its name, a step in.
            let msg = format!("{}{} › {}", indent, name, entry.msg);
            say(entry.level, &msg);
        }
        Progress::Finished { .. } => {
            done.set(done.get() + 1);
            progress("ran nodes of", done.get(), Some(total));
        }
        Progress::Started { .. } => {}
    };
    RUNNING.with(|r| r.borrow_mut().push(full.clone()));
    let started = std::time::Instant::now();
    let result =
        crate::run::run_opts(&ws, ctx.backend, &base, &mut report, RunOptions { stop: Some(ctx.gate.stop), pause: Some(ctx.gate.pause), ..Default::default() });
    RUNNING.with(|r| {
        r.borrow_mut().pop();
    });
    if ctx.stopped() {
        return err(crate::model::STOPPED);
    }

    // A row for each node of the other model: how it went.
    let fields = vec![
        Field::new("model", FieldType::Text),
        Field::new("node", FieldType::Text),
        Field::new("tool", FieldType::Text),
        Field::new("status", FieldType::Text),
        Field::new("features", FieldType::Int),
        Field::new("files", FieldType::Text),
        Field::new("seconds", FieldType::Real),
        Field::new("error", FieldType::Text),
    ];
    let mut out = Table { fields, ..Table::default() };
    for id in &result.order {
        let Some(r) = result.nodes.get(id) else { continue };
        let node = ws.nodes.iter().find(|n| n.id == *id);
        let label = node.map(|n| if n.name.is_empty() { n.id.clone() } else { n.name.clone() }).unwrap_or_else(|| id.clone());
        let status = match r.status {
            Status::Ok => "ok",
            Status::Warn => "warning",
            Status::Error => "error",
            Status::Skipped => "skipped",
            Status::Disabled => "disabled",
        };
        // What came out of the first output, such as Passed of a test.
        let first = node.and_then(|n| super::node_type(&n.kind)).and_then(|t| t.outputs.first().copied());
        let count: usize = match first {
            Some(port) => r.outputs.get(port).map_or(0, |t| t.len()),
            None => r.inputs.values().map(|t| t.len()).sum(),
        };
        let files: Vec<String> = r.files.iter().map(|f| f.display().to_string()).collect();
        out.features.push(Feature {
            attrs: vec![
                Value::Text(name.clone()),
                Value::Text(label),
                Value::Text(node.map(|n| n.kind.clone()).unwrap_or_default()),
                Value::Text(status.into()),
                Value::Int(count as i64),
                if files.is_empty() { Value::Null } else { Value::Text(files.join("; ")) },
                Value::Real((r.ms / 100.0).round() / 10.0),
                r.error.clone().map_or(Value::Null, Value::Text),
            ],
            geom: None,
            z: None,
        });
        // What the other model wrote counts as written by this one, for Make zip file.
        for f in &r.files {
            ctx.written.push((label_of(node, id), f.clone()));
        }
    }
    let secs = started.elapsed().as_secs_f64();
    let summary =
        format!("{} ran: {} nodes, {} errors, {} warnings, {:.1} s", name, fmt_count(result.order.len() as u64), result.errors, result.warnings, secs);
    if result.errors > 0 {
        say(Level::Warn, &format!("← {}: {}", ctx.node_name, summary));
        if p.get("on_error").as_deref() != Some("go") {
            let first = result.nodes.values().find_map(|r| r.error.clone()).unwrap_or_default();
            return err(format!("{} failed: {}", name, first));
        }
        ctx.notes.warn(summary);
    } else {
        say(Level::Info, &format!("← {}: {}", ctx.node_name, summary));
    }
    Ok(vec![("Result".into(), out)])
}

pub(super) fn label_of(node: Option<&crate::run::Node>, id: &str) -> String {
    node.map(|n| if n.name.is_empty() { n.id.clone() } else { n.name.clone() }).unwrap_or_else(|| id.to_string())
}

/// Starts the model by itself with og, which lies beside the program, and goes on. Its log
/// goes to a file beside the model.
pub(super) fn start_alone(full: &std::path::Path, name: &str, vars: &[(String, String)], ctx: &mut NodeCtx) -> Result<Outputs> {
    let exe = std::env::current_exe().map_err(|e| GbError(format!("Cannot find the program: {}", e)))?;
    let dir = exe.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let gb = ["og.exe", "og"].iter().map(|n| dir.join(n)).find(|p| p.is_file()).or_else(|| {
        // In the tests and in a build folder, og lies one folder up.
        dir.parent().and_then(|d| ["og.exe", "og"].iter().map(|n| d.join(n)).find(|p| p.is_file()))
    });
    let Some(gb) = gb else {
        return err("To start a model without waiting, the command line og must lie beside the program. Choose to wait instead.");
    };
    let log = full.with_extension("log");
    let mut cmd = std::process::Command::new(&gb);
    cmd.arg("run").arg(full).arg("--log").arg(&log);
    for (k, v) in vars {
        cmd.arg("--var").arg(format!("{}={}", k, v));
    }
    cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().map_err(|e| GbError(format!("Could not start {}: {}", name, e)))?;
    (ctx.say)(Level::Info, &format!("→ {}: started {} by itself; its log goes to {}", ctx.node_name, name, log.display()));
    let mut out = Table { fields: vec![Field::new("model", FieldType::Text), Field::new("log", FieldType::Text)], ..Table::default() };
    out.features.push(Feature { attrs: vec![Value::Text(name.to_string()), Value::Text(log.display().to_string())], geom: None, z: None });
    Ok(vec![("Result".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "ModelRunner",
        title: "Run model",
        hidden: false,
        group: Group::Transformer,
        category: "Workflow",
        desc: "Runs another model, so that work can be split into models of their own and run as a chain: fetch, then prepare, then publish. It waits until the other model has run, and what is connected after it runs after that; or it starts the other model by itself and goes on. Connect a stream to After to run it after that stream is done. The log of the other model comes in this log, and Result tells how each of its nodes went.",
        inputs: &["After"],
        outputs: &["Result"],
        ports: None,
        params: RUNNER_PARAMS,
        run: run_model,
        summary: |p| {
            let m = p.get("path").map(|x| x.rsplit(['/', '\\']).next().unwrap_or("").to_string()).unwrap_or_else(|| "no model".into());
            if p.get("wait").as_deref() == Some("false") { format!("{} · without waiting", m) } else { m }
        },
    },
];
