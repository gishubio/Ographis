//! Running the model: in a thread of its own, from or to a node, with what has not changed
//! reused, paused and stopped, and the command line that runs the same.

use super::*;

impl App {
    /// Starts a run in a thread of its own, so that the window does not freeze. The thread
    /// reports node by node, so that the counts on the canvas fill in while it runs.
    pub fn run(&mut self, ctx: &egui::Context) {
        // With the cache on, what has not changed since the latest run is taken from it.
        // Outputs and Run model always run: files are written, and another model may have
        // changed.
        if self.use_cache && self.result.is_some() && !self.live {
            let ws = self.workspace();
            let always: HashSet<String> = ws
                .nodes
                .iter()
                .filter(|n| {
                    og_engine::node_type(&n.kind).is_some_and(|t| t.group == og_engine::nodes::Group::Writer || t.outputs.is_empty()) || n.kind == "ModelRunner"
                })
                .map(|n| n.id.clone())
                .collect();
            // Every node is in the run; those not in `always` come from the cache when they can.
            let all: HashSet<String> = ws.nodes.iter().map(|n| n.id.clone()).collect();
            self.start_run_with(ctx, Some(all), Some(always));
            return;
        }
        self.start_run(ctx, None);
    }

    /// Runs every node again, whatever the cache holds.
    pub fn run_all_again(&mut self, ctx: &egui::Context) {
        self.start_run(ctx, None);
    }

    /// Runs the model from a node: the node and everything after it. What comes before it
    /// is taken from the cache where it has not changed. Needs the cache to be on.
    pub fn run_from(&mut self, ctx: &egui::Context, id: &str) {
        if !self.use_cache {
            self.say("Turn on Model → Use cache to run from a node.".to_string());
            return;
        }
        let ws = self.workspace();
        let mut fresh = og_engine::downstream_of(&ws, id);
        fresh.insert(id.to_string());
        self.start_run(ctx, Some(fresh));
    }

    /// Runs the model up to a node: the node and what it needs, and nothing after it.
    /// With the cache on, what comes before it is taken from the cache where it has not
    /// changed.
    pub fn run_to(&mut self, ctx: &egui::Context, id: &str) {
        let ws = self.workspace();
        let mut fresh: HashSet<String> = [id.to_string()].into_iter().collect();
        if !self.use_cache {
            fresh.extend(og_engine::upstream_of(&ws, &fresh));
        }
        self.start_run(ctx, Some(fresh));
    }

    /// Starts a run in a thread of its own: of the whole model, or of the nodes in `fresh`
    /// and what they need.
    pub(super) fn start_run(&mut self, ctx: &egui::Context, fresh: Option<HashSet<String>>) {
        self.start_run_with(ctx, fresh, None);
    }

    /// As `start_run`; with `must`, the nodes in `fresh` are only in scope, and only those in
    /// `must` are sure to run, the rest coming from the cache when they have not changed.
    pub(super) fn start_run_with(&mut self, ctx: &egui::Context, fresh: Option<HashSet<String>>, must: Option<HashSet<String>>) {
        if self.is_running() || !self.take_lock() {
            return;
        }
        let scope_only = must.is_some();
        let (ws, base, backend, ctx) = (self.workspace(), self.base_dir(), self.backend.clone(), ctx.clone());
        let (tx, rx) = channel();
        // A part of the model is run against what the latest run left.
        let partial = fresh.is_some() && !scope_only;
        let cache = if (partial || scope_only) && self.use_cache { self.result.clone().filter(|_| !self.live) } else { None };
        self.previous = if partial { self.result.clone() } else { None };
        // A whole run with the cache: every node is in it, and those that must run, run.
        let fresh = if scope_only { must } else { fresh };
        self.begin_run();
        if let (Some(f), false) = (&fresh, scope_only) {
            self.progress.1 = f.len() + og_engine::upstream_of(&ws, f).len();
        }
        // Flags of their own for each run, so that a stop never reaches the next one.
        self.stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.pause = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stopping = false;
        self.paused = false;
        let (stop, pause) = (self.stop.clone(), self.pause.clone());
        std::thread::spawn(move || {
            let opts = og_engine::RunOptions { stop: Some(&stop), pause: Some(&pause), fresh, cache: cache.as_ref().map(|c| &c.nodes), everything: scope_only };
            let result = og_engine::run_opts(
                &ws,
                Some(&*backend),
                &base,
                &mut |p| {
                    let msg = match p {
                        Progress::Started { id } => RunMsg::Started(id.to_string()),
                        Progress::Finished { id, result } => RunMsg::Finished(id.to_string(), Box::new(result.clone())),
                        Progress::Log { entry } => RunMsg::Log(entry.clone()),
                    };
                    let _ = tx.send(msg);
                    ctx.request_repaint();
                },
                opts,
            );
            drop(cache);
            let _ = tx.send(RunMsg::Done(Box::new(result)));
            ctx.request_repaint();
        });
        self.running = Some(rx);
    }

    /// The command that runs the model from the command line, with its variables. None
    /// until the model has been saved, since the command runs the file.
    pub fn command_line(&self) -> Option<String> {
        let model = self.path.as_ref()?;
        let model = std::fs::canonicalize(model).unwrap_or_else(|_| model.clone());
        // og.cmd sets up GDAL and PROJ before it starts og.exe. It is in the folder the
        // program was built in, a few levels above the program itself.
        let exe = std::env::current_exe().ok();
        let dir = exe.as_ref().and_then(|e| e.parent()).map(|d| d.to_path_buf());
        let launcher = dir.as_ref().and_then(|d| d.ancestors().take(4).map(|a| a.join("og.cmd")).find(|p| p.exists()));
        let gb = launcher.unwrap_or_else(|| {
            let name = if cfg!(windows) { "og.exe" } else { "og" };
            dir.map(|d| d.join(name)).unwrap_or_else(|| PathBuf::from(name))
        });
        let quote = |s: &str| if s.is_empty() || s.contains([' ', '&', '(', ')', '^', ';', ',']) { format!("\"{}\"", s) } else { s.to_string() };
        // On Windows a full path comes back as \\?\C:\…, which the command line does not want.
        let plain = |p: &Path| p.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let mut cmd = format!("{} run {}", quote(&plain(&gb)), quote(&plain(&model)));
        // A password is left out: the model has it, and a command line is easily seen.
        for v in self.variables.iter().filter(|v| !og_engine::run::is_secret(&v.name)) {
            cmd += &format!(" --var {}", quote(&format!("{}={}", v.name, v.value)));
        }
        Some(cmd)
    }

    /// Puts the command line of the model on the clipboard.
    pub fn copy_command_line(&mut self, ctx: &egui::Context) {
        match self.command_line() {
            None => self.say("Save the model first. The command line runs the saved file.".to_string()),
            Some(cmd) => {
                ctx.copy_text(cmd.clone());
                self.last_command = Some(cmd);
                let note = if self.dirty { " Save the model, since it runs what is saved." } else { "" };
                self.say(format!("The command line is copied.{}", note));
            }
        }
    }

    /// With the cache on, the nodes that have a result in it, and whether that result is up
    /// to date. Worked out at most twice a second, since the files that are read can change.
    pub fn cache_state(&mut self, now: f64) -> HashMap<String, bool> {
        if !self.use_cache || self.live {
            return HashMap::new();
        }
        let Some(result) = self.result.clone() else { return HashMap::new() };
        if now - self.cache_state.0 > 0.5 || now < self.cache_state.0 {
            let keys = og_engine::node_keys(&self.workspace(), &self.base_dir());
            let state = result
                .nodes
                .iter()
                .filter(|(_, r)| r.key != 0 && matches!(r.status, Status::Ok | Status::Warn))
                .map(|(id, r)| (id.clone(), keys.get(id) == Some(&r.key)))
                .collect();
            self.cache_state = (now, state);
        }
        self.cache_state.1.clone()
    }

    /// Whether the result of a node is from before its settings, or what it depends on, were
    /// changed. Worked out at most twice a second.
    pub fn result_is_old(&mut self, id: &str, now: f64) -> bool {
        let Some(r) = self.result.as_ref().and_then(|r| r.nodes.get(id)) else { return false };
        if r.key == 0 || self.live {
            return false;
        }
        let key = r.key;
        if now - self.keys_now.0 > 0.5 || now < self.keys_now.0 {
            self.keys_now = (now, og_engine::node_keys(&self.workspace(), &self.base_dir()));
        }
        self.keys_now.1.get(id).is_some_and(|k| *k != key)
    }

    /// Pauses the run that is going on, or lets it go on. It waits between nodes and in
    /// long steps such as reading.
    pub fn toggle_pause(&mut self) {
        if !self.is_running() || self.stopping {
            return;
        }
        self.paused = !self.paused;
        self.pause.store(self.paused, std::sync::atomic::Ordering::Relaxed);
        self.say(if self.paused { "Paused. Resume with F6 or the button." } else { "Resumed." }.to_string());
    }

    pub fn is_running(&self) -> bool {
        self.live
    }

    /// Stops the run that is going on. The step at work is broken off where it can be, and
    /// what has not run is skipped. The results so far are kept.
    pub fn stop_run(&mut self) {
        if self.is_running() && !self.stopping {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            self.stopping = true;
            self.stop_asked = Some(std::time::Instant::now());
            self.paused = false;
            self.say("Stopping the run…".to_string());
        }
    }

    /// After Stop, a tool that goes on for longer than a few seconds, in a step that cannot be
    /// broken off, is left to finish on its own: the run ends at once with what it has, and
    /// what the tool gives later is not used.
    pub(super) fn give_up_stopping(&mut self) {
        let waited = self.stop_asked.is_some_and(|t| t.elapsed() > std::time::Duration::from_secs(3));
        if !(self.stopping && waited && self.running.is_some()) {
            return;
        }
        self.running = None;
        self.stop_asked = None;
        let mut result =
            self.result.as_deref().cloned().unwrap_or(RunResult { nodes: HashMap::new(), log: Vec::new(), order: Vec::new(), errors: 0, warnings: 0, ms: 0.0 });
        let at_work = self.running_node.take();
        if let Some(id) = &at_work {
            let name = self.snarl.nodes().find(|n| &n.id == id).map_or(id.clone(), |n| n.name.clone());
            result.log.push(og_engine::run::LogEntry {
                level: og_engine::run::Level::Warn,
                node: Some(id.clone()),
                msg: format!("{} was at a step that cannot be broken off. It is left to finish on its own, and what it gives is not used.", name),
            });
            result.warnings += 1;
        }
        // The nodes that did not finish are stopped, as when a run is stopped between nodes.
        let ids: Vec<String> = self.snarl.nodes().filter(|n| !n.is_note()).map(|n| n.id.clone()).collect();
        for id in ids {
            if !result.nodes.contains_key(&id) {
                let why = if at_work.as_deref() == Some(id.as_str()) { "Stopped while it ran." } else { "Stopped before it ran." };
                result.nodes.insert(id.clone(), og_engine::run::NodeResult { status: Status::Skipped, error: Some(why.to_string()), ..Default::default() });
                result.order.push(id);
            }
        }
        let ran = result.nodes.values().filter(|r| r.status != Status::Skipped).count();
        result.log.push(og_engine::run::LogEntry {
            level: og_engine::run::Level::Warn,
            node: None,
            msg: format!(
                "Run stopped: {} {}, {} {}",
                ran,
                if ran == 1 { "node" } else { "nodes" },
                result.warnings,
                if result.warnings == 1 { "warning" } else { "warnings" }
            ),
        });
        self.finish(result);
    }

    /// Clears the result of the latest run and gets ready to take in a new one, node by node.
    pub fn begin_run(&mut self) {
        self.live = true;
        self.stale = false;
        self.running_node = None;
        self.view.feature = None;
        self.view.more.clear();
        self.progress = (0, self.snarl.nodes().filter(|n| !n.is_note()).count());
        // A large result takes a while to free. That is done beside the window.
        let old = self.result.replace(Arc::new(RunResult { nodes: HashMap::new(), log: Vec::new(), order: Vec::new(), errors: 0, warnings: 0, ms: 0.0 }));
        drop_later(old);
    }

    /// Takes in what a run reports.
    pub fn on_run_msg(&mut self, msg: RunMsg) {
        match msg {
            RunMsg::Started(id) => {
                self.running_node = Some(id);
                self.running_step = None;
            }
            RunMsg::Finished(id, node) => {
                if self.running_node.as_deref() == Some(id.as_str()) {
                    self.running_node = None;
                }
                self.progress.0 += 1;
                if let Some(result) = &mut self.result {
                    let result = Arc::make_mut(result);
                    result.errors += usize::from(node.status == Status::Error);
                    result.warnings += node.warnings.len();
                    result.order.push(id.clone());
                    result.nodes.insert(id, *node);
                }
            }
            RunMsg::Log(entry) => {
                if let Some((f, _)) = &mut self.run_log {
                    use std::io::Write;
                    let tag = match entry.level {
                        og_engine::run::Level::Info => "INFO ",
                        og_engine::run::Level::Warn => "WARN ",
                        og_engine::run::Level::Error => "ERROR",
                    };
                    let _ = writeln!(f, "{} {} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), tag, entry.msg);
                }
                // How far a long step has come is shown in the status line too.
                let step = entry.node.is_some() && self.running_node == entry.node && (entry.msg.contains(" of ") || entry.msg.ends_with("so far"));
                if step {
                    self.running_step = entry.msg.split_once(": ").map(|x| x.1.to_string());
                }
                if let Some(result) = &mut self.result {
                    Arc::make_mut(result).log.push(entry);
                }
            }
            RunMsg::Done(result) => self.finish(*result),
        }
    }

    /// Runs and waits for the result. For tests.
    pub fn run_blocking(&mut self) {
        if !self.take_lock() {
            return;
        }
        let result = og_engine::run(&self.workspace(), Some(&*self.backend), &self.base_dir());
        self.finish(result);
    }

    /// Takes the lock of the saved model for a run, so that it is not run from the command
    /// line at the same time. Says so, and gives false, when it is running already.
    fn take_lock(&mut self) -> bool {
        self.run_started = Some(chrono::Local::now().naive_local());
        let Some(path) = self.path.clone() else { return true };
        match og_engine::history::lock(&path, "program", 24.0) {
            Ok(l) => {
                self.run_lock = Some(l);
                let started = self.run_started.unwrap_or_else(|| chrono::Local::now().naive_local());
                let file = og_engine::history::log_file(&path, started);
                let _ = std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")));
                self.run_log = std::fs::File::create(&file).ok().map(|mut f| {
                    use std::io::Write;
                    let model = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let _ = writeln!(f, "{} INFO  Running {} in Ographis {}", started.format("%Y-%m-%d %H:%M:%S"), model, env!("CARGO_PKG_VERSION"));
                    (f, file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                });
                true
            }
            Err(h) => {
                self.say(format!("The model is running already, {}. Wait for it to finish, or remove the lock in Model → Run history.", h.describe()));
                false
            }
        }
    }

    pub(super) fn finish(&mut self, mut result: RunResult) {
        // The run goes into the history of the saved model, and the lock is let go.
        let stopped = self.stopping;
        if let (Some(path), Some(started)) = (self.path.clone(), self.run_started.take()) {
            let mut run = og_engine::history::Run::of(&result, started, "program", stopped);
            run.log = self.run_log.take().map(|(_, name)| name).unwrap_or_default();
            let _ = og_engine::history::record(&path, &run);
            og_engine::history::clean_logs(&path, 30);
        }
        self.run_lock = None;
        self.run_log = None;
        // After a run of a part of the model, the results of the other nodes are kept from
        // before, as long as nothing they depend on has changed.
        if let Some(before) = self.previous.take() {
            let keys = og_engine::node_keys(&self.workspace(), &self.base_dir());
            for id in &before.order {
                if result.nodes.contains_key(id) {
                    continue;
                }
                if let Some(r) = before.nodes.get(id).filter(|r| r.key != 0 && keys.get(id) == Some(&r.key)) {
                    result.nodes.insert(id.clone(), r.clone());
                    result.order.insert(0, id.clone());
                }
            }
            drop_later(Some(before));
        }
        self.running = None;
        self.live = false;
        self.stopping = false;
        self.paused = false;
        self.running_node = None;
        self.stale = false;
        self.view.feature = None;
        self.view.more.clear();
        if result.errors > 0 {
            self.tab = Tab::Log;
        }
        self.result = Some(Arc::new(result));
    }
}
