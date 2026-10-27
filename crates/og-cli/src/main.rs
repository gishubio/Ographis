//! Ographis from the command line: run a model, look at a file, test the installation.

use og_engine::csv_io::{self, CsvReadOpts};
use og_engine::io::{Backend, Notes};
use og_engine::model::{crs_label, Table, Value};
use og_engine::nodes::{Group, ParamKind, NODE_TYPES};
use og_engine::{geom, run_with, Level, Progress, Workspace};
use og_gdal::GdalBackend;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "Ographis – spatial ETL from the command line

Usage:
  og run <model.ographis | folder> [--var name=value] [--log file] [--inspect node[:port]] [--rows N]
      Runs a model, or every model in a folder by name, with a summary at the end.
      Paths in a model start from the folder the model is in.
      --var sets a variable of the model, used in its settings as $(name).
      --log adds the log, with date and time, to the end of a file.
      --inspect prints what came out of a node, for example --inspect tester:Passed
      Each run is kept in a folder beside the model, named after it (house.runs):
      a line in history.csv, and its log in the folder logs, where logs older than 30 days
      are removed. While a model runs it is locked, so that it does not start twice.
      --keep-logs N keeps the logs N days (0 keeps them all).
      --no-history keeps neither history nor logs.
      --no-lock runs even when the model is running already.
      --lock-hours H counts a lock older than H hours as left by a crash (24).
      Exit code: 0 all went well, 3 finished with warnings, 1 a node failed,
      4 the model was running already, 2 the command is written wrong.
      For a folder, the worst of the models.
  og history <model.ographis> [--last N] [--unlock]
      Shows the latest runs of a model: when, how long, how it went, what was written.
      --unlock removes a lock left by a run that crashed.
  og info <file>
      Shows the layers, feature counts, coordinate system and attributes of a file.
  og nodes
      Lists the inputs, tools and outputs with their ports and parameters,
      and the functions available in expressions.
  og connections
      Lists the database and API connections of the settings, which models name,
      and where the settings are kept. Passwords are not shown.
  og picture <model.ographis> <file.png | file.jpg | file.pdf>
      Draws the model as on the canvas: its nodes, connections, groups, comments
      and notes, for documentation.
  og formats [--html file]
      Lists the formats Ographis reads and writes, and whether GDAL or Ographis
      itself handles them. --html writes them as a web page.
  og doctor
      Tests that GDAL and PROJ work in this installation.
  og version
";

fn main() -> ExitCode {
    og_engine::crash::install(concat!("og ", env!("CARGO_PKG_VERSION")));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "run" => cmd_run(&args[1..]),
        "info" => cmd_info(&args[1..]),
        "history" => cmd_history(&args[1..]),
        "nodes" => cmd_nodes(),
        "doctor" => cmd_doctor(),
        "picture" => cmd_picture(&args[1..]),
        "formats" => cmd_formats(&args[1..]),
        "connections" => cmd_connections(),
        "version" | "--version" | "-V" => {
            let version = env!("CARGO_PKG_VERSION");
            println!("Ographis {} · {}", version.strip_suffix(".0").unwrap_or(version), GdalBackend::new().describe());
            ExitCode::SUCCESS
        }
        "help" | "--help" | "-h" => {
            print!("{}", HELP);
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("Unknown command: {}\n\n{}", other, HELP);
            ExitCode::from(2)
        }
    }
}

fn cmd_picture(args: &[String]) -> ExitCode {
    let (Some(model), Some(out)) = (args.first(), args.get(1)) else {
        return usage("og picture needs a model and the file to draw it to.");
    };
    let ws = match Workspace::load(Path::new(model)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{}", e.0);
            return ExitCode::from(1);
        }
    };
    match og_engine::modelpic::export(&ws, Path::new(out)) {
        Ok(()) => {
            println!("{} drawn to {}", model, out);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e.0);
            ExitCode::from(1)
        }
    }
}

fn cmd_formats(args: &[String]) -> ExitCode {
    match args.first().map(|s| s.as_str()) {
        Some("--html") => {
            let Some(out) = args.get(1) else { return usage("og formats --html needs the file to write.") };
            let version = env!("CARGO_PKG_VERSION");
            let html = og_engine::formats::html(version.strip_suffix(".0").unwrap_or(version), &GdalBackend::new().describe());
            if let Err(e) = std::fs::write(out, html) {
                eprintln!("Cannot write {}: {}", out, e);
                return ExitCode::from(1);
            }
            println!("The formats written to {}", out);
            ExitCode::SUCCESS
        }
        None => {
            print!("{}", og_engine::formats::text());
            ExitCode::SUCCESS
        }
        Some(other) => usage(&format!("Unknown option for og formats: {}", other)),
    }
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("{}\n\n{}", msg, HELP);
    ExitCode::from(2)
}

/// How a run ended, as the exit code of the program: 0 when all went well, 1 when a node
/// failed, 3 when it finished with warnings. 2 is for a command that is written wrong.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Outcome {
    Ok,
    Warnings,
    Errors,
    Locked,
}

impl Outcome {
    fn code(self) -> ExitCode {
        match self {
            Outcome::Ok => ExitCode::SUCCESS,
            Outcome::Warnings => ExitCode::from(3),
            Outcome::Errors => ExitCode::from(1),
            Outcome::Locked => ExitCode::from(4),
        }
    }
    fn word(self) -> &'static str {
        match self {
            Outcome::Ok => "OK",
            Outcome::Warnings => "WARNINGS",
            Outcome::Errors => "ERRORS",
            Outcome::Locked => "RUNNING",
        }
    }
}

struct RunArgs {
    inspect: Vec<String>,
    rows: usize,
    vars: Vec<(String, String)>,
    log: Option<std::fs::File>,
    /// The log of the run of one model, in its runs folder.
    run_log: Option<std::fs::File>,
    history: bool,
    keep_logs: u32,
    lock: bool,
    lock_hours: f64,
}

impl RunArgs {
    /// Prints a line of the log, and writes it with the date and time to the log file.
    fn say(&mut self, tag: &str, msg: &str) {
        println!("{} {}", tag, msg);
        if let Some(f) = self.log.as_mut() {
            use std::io::Write;
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "{} {} {}", now, tag, msg);
        }
        if let Some(f) = self.run_log.as_mut() {
            use std::io::Write;
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "{} {} {}", now, tag, msg);
        }
    }
}

fn cmd_run(args: &[String]) -> ExitCode {
    let (mut file, mut inspect, mut rows) = (None, Vec::new(), 20usize);
    let mut vars: Vec<(String, String)> = Vec::new();
    let mut log_path: Option<PathBuf> = None;
    let (mut history, mut keep_logs, mut lock, mut lock_hours) = (true, 30u32, true, 24.0f64);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--inspect" | "-i" => {
                i += 1;
                match args.get(i) {
                    Some(v) => inspect.push(v.clone()),
                    None => return usage("--inspect needs a node id."),
                }
            }
            "--rows" | "-n" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(v) => rows = v,
                    None => return usage("--rows needs a whole number."),
                }
            }
            "--var" | "-v" => {
                i += 1;
                match args.get(i).and_then(|v| v.split_once('=')) {
                    Some((name, value)) if og_engine::run::is_variable_name(name.trim()) => vars.push((name.trim().to_string(), value.to_string())),
                    _ => return usage("--var needs a name and a value, as in --var year=2026."),
                }
            }
            "--log" | "-l" => {
                i += 1;
                match args.get(i) {
                    Some(v) => log_path = Some(PathBuf::from(v)),
                    None => return usage("--log needs a file to write the log to."),
                }
            }
            "--no-history" => history = false,
            "--no-lock" => lock = false,
            "--keep-logs" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(v) => keep_logs = v,
                    None => return usage("--keep-logs needs a number of days."),
                }
            }
            "--lock-hours" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()).filter(|v: &f64| *v > 0.0) {
                    Some(v) => lock_hours = v,
                    None => return usage("--lock-hours needs a number of hours."),
                }
            }
            a if a.starts_with('-') => return usage(&format!("Unknown option: {}", a)),
            a => file = Some(PathBuf::from(a)),
        }
        i += 1;
    }
    let file = match file {
        Some(f) => f,
        None => return usage("Say which model to run, or which folder of models."),
    };
    // The log file is added to, so that runs from a schedule are kept one after another.
    let log = match &log_path {
        Some(p) => {
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                let _ = std::fs::create_dir_all(dir);
            }
            match std::fs::OpenOptions::new().create(true).append(true).open(p) {
                Ok(f) => Some(f),
                Err(e) => {
                    eprintln!("ERROR Cannot write the log to {}: {}", p.display(), e);
                    return ExitCode::from(1);
                }
            }
        }
        None => None,
    };
    let mut opts = RunArgs { inspect, rows, vars, log, run_log: None, history, keep_logs, lock, lock_hours };
    if file.is_dir() {
        // Every model in the folder, in the order of their names.
        let mut models: Vec<PathBuf> = std::fs::read_dir(&file)
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ographis") || e.eq_ignore_ascii_case("json")))
                    .collect()
            })
            .unwrap_or_default();
        models.sort();
        if models.is_empty() {
            eprintln!("ERROR There are no models (.ographis or .json) in {}.", file.display());
            return ExitCode::from(1);
        }
        if !opts.inspect.is_empty() {
            return usage("--inspect works with one model, not a folder.");
        }
        let mut summary = Vec::new();
        for m in &models {
            let started = std::time::Instant::now();
            let outcome = run_model(m, &mut opts);
            summary.push((m.clone(), outcome, started.elapsed().as_secs_f64() * 1000.0));
            println!();
        }
        let width = summary.iter().map(|s| s.0.file_name().map_or(0, |n| n.len())).max().unwrap_or(0);
        opts.say("INFO ", &format!("{} models in {}:", summary.len(), file.display()));
        for (m, outcome, ms) in &summary {
            let name = m.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            opts.say("INFO ", &format!("  {:<width$}  {:<8}  {}", name, outcome.word(), og_engine::model::fmt_duration(*ms), width = width));
        }
        return summary.iter().map(|s| s.1).max().unwrap_or(Outcome::Ok).code();
    }
    run_model(&file, &mut opts).code()
}

/// Runs one model and prints its log, with a lock, a log of its own and a line in its
/// history. Says how it ended.
fn run_model(file: &Path, opts: &mut RunArgs) -> Outcome {
    use og_engine::history;
    let started = chrono::Local::now().naive_local();
    // A model that runs already, as from a schedule that comes round before the last run
    // is done, is not started again.
    let _lock = if opts.lock {
        match history::lock(file, "og", opts.lock_hours) {
            Ok(l) => Some(l),
            Err(h) => {
                opts.say(
                    "ERROR",
                    &format!(
                        "{} is running already, {}. It was not started again. If that run has crashed, remove the lock with og history --unlock.",
                        file.display(),
                        h.describe()
                    ),
                );
                return Outcome::Locked;
            }
        }
    } else {
        None
    };
    let log_name = if opts.history {
        let path = history::log_file(file, started);
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
        match std::fs::File::create(&path) {
            Ok(f) => {
                opts.run_log = Some(f);
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            }
            Err(e) => {
                eprintln!("WARN  The log of the run cannot be kept in {}: {}", path.display(), e);
                String::new()
            }
        }
    } else {
        String::new()
    };
    let outcome = run_model_inner(file, opts, started, &log_name);
    opts.run_log = None;
    if opts.history && opts.keep_logs > 0 {
        history::clean_logs(file, opts.keep_logs);
    }
    outcome
}

fn run_model_inner(file: &Path, opts: &mut RunArgs, started: chrono::NaiveDateTime, log_name: &str) -> Outcome {
    let mut ws = match Workspace::load(file) {
        Ok(w) => w,
        Err(e) => {
            opts.say("ERROR", &format!("{}: {}", file.display(), e));
            return Outcome::Errors;
        }
    };
    let base = file.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    for (name, value) in &opts.vars {
        ws.set_variable(name, value);
    }
    // A password is not written in the log.
    let shown_vars: Vec<String> =
        ws.variables.iter().map(|v| if og_engine::run::is_secret(&v.name) { format!("{}=…", v.name) } else { format!("{}={}", v.name, v.value) }).collect();
    let head = if shown_vars.is_empty() { format!("Running {}", file.display()) } else { format!("Running {} with {}", file.display(), shown_vars.join(", ")) };
    opts.say("INFO ", &head);
    let backend = GdalBackend::new();
    // The log is printed as it is written, so that a long run shows how far it has come.
    let result = run_with(&ws, Some(&backend), &base, &mut |p| {
        if let Progress::Log { entry } = p {
            let tag = match entry.level {
                Level::Info => "INFO ",
                Level::Warn => "WARN ",
                Level::Error => "ERROR",
            };
            opts.say(tag, &entry.msg);
        }
    });
    let rows = opts.rows;
    for spec in &opts.inspect {
        let (id, port) = match spec.split_once(':') {
            Some((a, b)) => (a, Some(b)),
            None => (spec.as_str(), None),
        };
        println!();
        let node = match ws.nodes.iter().find(|n| n.id == id) {
            Some(n) => n,
            None => {
                println!("There is no node {}. Available: {}.", id, ws.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>().join(", "));
                continue;
            }
        };
        let res = match result.nodes.get(id) {
            Some(r) => r,
            None => continue,
        };
        if let Some(e) = &res.error {
            println!("{}: {}", node.label(), e);
            continue;
        }
        // A writer has no output ports, so what came in is shown there.
        let (map, default_port) = if res.outputs.is_empty() { (&res.inputs, "Input") } else { (&res.outputs, "") };
        let mut ports: Vec<&String> = map.keys().collect();
        ports.sort();
        let chosen = port.map(|p| p.to_string()).or_else(|| {
            if ports.len() == 1 {
                Some(ports[0].clone())
            } else if !default_port.is_empty() {
                Some(default_port.to_string())
            } else {
                None
            }
        });
        let shown: Vec<&String> = match &chosen {
            Some(p) => ports.iter().copied().filter(|x| *x == p).collect(),
            None => ports.clone(),
        };
        if shown.is_empty() {
            println!(
                "There is no port {} on {}. Available: {}.",
                port.unwrap_or(""),
                node.label(),
                ports.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
        for p in shown {
            print_table(&format!("{} · {}", node.label(), p), &map[p], rows);
        }
    }
    if opts.history {
        let mut run = og_engine::history::Run::of(&result, started, "og", false);
        run.log = log_name.to_string();
        if let Err(e) = og_engine::history::record(file, &run) {
            eprintln!("WARN  {}", e);
        }
    }
    if result.errors > 0 {
        Outcome::Errors
    } else if result.warnings > 0 {
        Outcome::Warnings
    } else {
        Outcome::Ok
    }
}

fn cmd_history(args: &[String]) -> ExitCode {
    let (mut file, mut last, mut unlock) = (None, 20usize, false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--last" | "-n" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(v) => last = v,
                    None => return usage("--last needs a whole number."),
                }
            }
            "--unlock" => unlock = true,
            a if a.starts_with('-') => return usage(&format!("Unknown option: {}", a)),
            a => file = Some(PathBuf::from(a)),
        }
        i += 1;
    }
    let Some(file) = file else { return usage("Say which model to show the history of.") };
    if unlock {
        if og_engine::history::unlock(&file) {
            println!("The lock of {} was removed.", file.display());
        } else {
            println!("{} has no lock.", file.display());
        }
        return ExitCode::SUCCESS;
    }
    if let Some(h) = og_engine::history::holder(&file, 24.0) {
        println!("Running now, {}.", h.describe());
    }
    let runs = og_engine::history::history(&file);
    if runs.is_empty() {
        println!("{} has not been run with a history yet.", file.display());
        return ExitCode::SUCCESS;
    }
    println!("{} — the latest {} of {} runs, in {}", file.display(), last.min(runs.len()), runs.len(), og_engine::history::runs_dir(&file).display());
    println!("{:<19}  {:>8}  {:<8}  {:>8}  {:>5}  {:<7}  log / message", "started", "seconds", "outcome", "written", "files", "by");
    for r in runs.iter().take(last) {
        let tail = if r.message.is_empty() { r.log.clone() } else { format!("{}  {}", r.log, r.message) };
        println!("{:<19}  {:>8}  {:<8}  {:>8}  {:>5}  {:<7}  {}", r.started, r.seconds, r.outcome, r.written, r.files, r.by, tail);
    }
    ExitCode::SUCCESS
}

fn cell(v: &Value) -> String {
    let s = match v {
        Value::Null => "–".to_string(),
        other => other.to_text(),
    };
    let s = s.replace('\n', " ");
    if s.chars().count() > 30 {
        s.chars().take(29).collect::<String>() + "…"
    } else {
        s
    }
}

fn print_table(title: &str, t: &Table, rows: usize) {
    println!("{} — {} features · {}", title, t.len(), crs_label(t.crs));
    if t.fields.is_empty() && t.is_empty() {
        return;
    }
    let numeric: Vec<bool> = t.fields.iter().map(|f| f.ty != og_engine::FieldType::Text).collect();
    let mut head = vec!["#".to_string()];
    if t.crs.is_some() {
        head.push("geometry".to_string());
    }
    head.extend(t.field_names());
    let mut body: Vec<Vec<String>> = Vec::new();
    for (i, f) in t.features.iter().take(rows).enumerate() {
        let mut r = vec![(i + 1).to_string()];
        if t.crs.is_some() {
            r.push(f.geom.as_ref().map(geom::type_name).unwrap_or("–").to_string());
        }
        r.extend(f.attrs.iter().map(cell));
        body.push(r);
    }
    let width: Vec<usize> = (0..head.len()).map(|c| body.iter().map(|r| r[c].chars().count()).chain([head[c].chars().count()]).max().unwrap_or(0)).collect();
    let offset = head.len() - t.fields.len();
    let line = |r: &[String]| {
        r.iter()
            .enumerate()
            .map(|(c, s)| {
                let pad = " ".repeat(width[c] - s.chars().count());
                let right = c == 0 || (c >= offset && numeric[c - offset]);
                if right {
                    format!("{}{}", pad, s)
                } else {
                    format!("{}{}", s, pad)
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    println!("{}", line(&head));
    for r in &body {
        println!("{}", line(r));
    }
    if t.len() > rows {
        println!("… and {} more. Show more with --rows.", t.len() - rows);
    }
}

fn cmd_info(args: &[String]) -> ExitCode {
    let path = match args.first() {
        Some(p) => Path::new(p),
        None => return usage("Give a file."),
    };
    if !path.exists() {
        eprintln!("ERROR The file does not exist: {}", path.display());
        return ExitCode::from(1);
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let plain_json = og_engine::nodes::is_plain_json(path);
    if plain_json || matches!(ext.as_str(), "csv" | "txt" | "tsv") {
        let mut notes = Notes::default();
        let read = if plain_json {
            og_engine::json::read_table(path, None, og_engine::json::Lists::Text, false, &mut notes)
        } else {
            csv_io::read_csv(path, &CsvReadOpts::default(), &mut notes)
        };
        return match read {
            Ok(t) => {
                println!("{}", path.display());
                println!("  {} rows · {} · {}", t.len(), crs_label(t.crs), notes.info.join(", "));
                for f in &t.fields {
                    println!("    {:<24} {}", f.name, f.ty.label());
                }
                for w in &notes.warnings {
                    println!("  WARN  {}", w);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("ERROR {}", e);
                ExitCode::from(1)
            }
        };
    }
    match GdalBackend::new().layers(path) {
        Ok(layers) => {
            println!("{}", path.display());
            for l in layers {
                let crs = match (l.crs, &l.crs_name) {
                    (Some(c), _) => crs_label(Some(c)),
                    (None, Some(n)) => format!("{} (no EPSG code)", n),
                    (None, None) => "no coordinate system".to_string(),
                };
                println!("  {} — {} features · {} · {}", l.name, l.count.map(|c| c.to_string()).unwrap_or_else(|| "unknown number of".into()), l.geometry, crs);
                for f in &l.fields {
                    println!("    {:<24} {}", f.name, f.ty.label());
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ERROR {}", e);
            ExitCode::from(1)
        }
    }
}

fn cmd_connections() -> ExitCode {
    let file = og_engine::settings::settings_file();
    println!("Settings: {}", file.as_ref().map_or("no folder for settings".to_string(), |f| f.display().to_string()));
    match og_engine::settings::Settings::load() {
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::from(1)
        }
        Ok(s) => {
            if s.connections.is_empty() {
                println!("No database connections. Add them in the program under Edit → Settings.");
            }
            for c in &s.connections {
                let host = if c.host.is_empty() { "localhost" } else { &c.host };
                let port = if c.port.is_empty() { "5432" } else { &c.port };
                println!("{}  ·  {} {} on {}:{} as {}", c.name, c.kind, c.database, host, port, if c.user.is_empty() { "-" } else { &c.user });
            }
            for c in &s.apis {
                let kind = og_engine::web::AUTH_KINDS.iter().find(|k| k.0 == c.auth).map_or(c.auth.as_str(), |k| k.1);
                let only = if c.base_url.trim().is_empty() { String::new() } else { format!(", only for {}", c.base_url.trim()) };
                println!("{}  ·  API: {}{}", c.name, kind, only);
            }
            ExitCode::SUCCESS
        }
    }
}

fn cmd_nodes() -> ExitCode {
    for t in NODE_TYPES.iter() {
        let group = match t.group {
            Group::Reader => "input",
            Group::Transformer => "tool",
            Group::Writer => "output",
        };
        // The name in workspace files, and what the tool is called in the program.
        let shown = if t.title == t.name { String::new() } else { format!(" · {}", t.title) };
        let kept = match (t.hidden, t.name) {
            (true, "PostgisReader") => ", in the program the PostGIS button of Input",
            (true, "PostgisWriter") => ", in the program the PostGIS button of Output",
            (true, "LayeredPdfWriter") => ", in the program the format GeoSpatial PDF of Output",
            (true, _) => ", kept for older models and not offered in the program",
            _ => "",
        };
        println!("{}{} ({}{})", t.name, shown, group, kept);
        println!("  {}", t.desc);
        if !t.inputs.is_empty() {
            println!("  in:  {}", t.inputs.join(", "));
        }
        if t.ports.is_some() {
            println!("  out: one port per value, and {}", t.outputs.join(", "));
        } else if !t.outputs.is_empty() {
            println!("  out: {}", t.outputs.join(", "));
        }
        for p in t.params {
            let kind = match p.kind {
                ParamKind::AttributeTable(_) => " [list of {name, action: keep | rename | remove, to}]".to_string(),
                ParamKind::NewAttributes(_) => " [list of {name, value}]".to_string(),
                ParamKind::Values(..) => " [list of values]".to_string(),
                ParamKind::Choice(list) => format!(" [{}]", list.iter().map(|c| if c.0.is_empty() { "empty" } else { c.0 }).collect::<Vec<_>>().join(" | ")),
                _ => String::new(),
            };
            let default = if p.default.is_empty() { String::new() } else { format!(" = {}", p.default) };
            let hint = if p.hint.is_empty() { String::new() } else { format!(" — {}", p.hint) };
            println!("    {}{}{}  {}{}", p.key, default, kind, p.label, hint);
        }
        println!();
    }
    println!("Expressions follow QGIS: fields are written by name or in \"double quotes\", text in 'single quotes'.");
    println!("Operators: + - * / // % || = <> < <= > >= AND OR NOT IS NULL IN LIKE ILIKE. Variables: $area and $length in m² and m.");
    for h in og_engine::expr::function_help() {
        println!("  {}", h);
    }
    ExitCode::SUCCESS
}

fn cmd_doctor() -> ExitCode {
    let mut ok = true;
    for c in GdalBackend::new().doctor() {
        match c.result {
            Ok(v) => println!("OK    {}: {}", c.what, v),
            Err(e) => {
                // PostGIS is not needed yet, and the program works without reading workbooks.
                let optional = c.what.contains("PostGIS") || c.what.contains("(reading)");
                ok &= optional;
                println!("{} {}: {}", if optional { "–    " } else { "ERROR" }, c.what, e);
            }
        }
    }
    println!("{}", if ok { "The installation works." } else { "The installation needs attention. See docs/BUILD-WINDOWS.md." });
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
