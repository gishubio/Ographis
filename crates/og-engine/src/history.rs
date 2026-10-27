//! What a model has been through: a history of its runs, a log of each run from the command
//! line, and a lock so that a model is not run twice at once. They are kept in a folder
//! beside the model, named after it: house.ographis has house.runs.

use crate::model::{GbError, Result};
use crate::run::{Level, RunResult};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The folder of the runs of a model.
pub fn runs_dir(model: &Path) -> PathBuf {
    let stem = model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
    model.parent().unwrap_or(Path::new(".")).join(format!("{}.runs", stem))
}

/// The user and the computer, as the system names them.
pub fn who() -> (String, String) {
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let computer = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .unwrap_or_default();
    (user, computer)
}

/// One run, as the history keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub started: String,
    pub seconds: f64,
    /// ok, warnings, errors or stopped.
    pub outcome: String,
    pub nodes: usize,
    pub warnings: usize,
    pub errors: usize,
    /// Features that went into outputs, and the files they wrote.
    pub written: u64,
    pub files: usize,
    /// og or program.
    pub by: String,
    pub user: String,
    pub computer: String,
    /// The log of the run, a file in the logs folder, when there is one.
    pub log: String,
    /// The first error, for a quick look.
    pub message: String,
}

const HEADER: &str = "started;seconds;outcome;nodes;warnings;errors;written;files;by;user;computer;log;message";

impl Run {
    /// A run as its result tells it.
    pub fn of(result: &RunResult, started: chrono::NaiveDateTime, by: &str, stopped: bool) -> Run {
        let (user, computer) = who();
        let mut written = 0u64;
        let mut files = 0usize;
        for r in result.nodes.values() {
            if !r.files.is_empty() {
                files += r.files.len();
                written += r.inputs.values().map(|t| t.len() as u64).sum::<u64>();
            }
        }
        let outcome = if stopped {
            "stopped"
        } else if result.errors > 0 {
            "errors"
        } else if result.warnings > 0 {
            "warnings"
        } else {
            "ok"
        };
        let message = result
            .log
            .iter()
            .find(|l| l.level == Level::Error)
            .or_else(|| result.log.iter().find(|l| l.level == Level::Warn))
            .map(|l| l.msg.clone())
            .unwrap_or_default();
        Run {
            started: started.format("%Y-%m-%d %H:%M:%S").to_string(),
            seconds: (result.ms / 100.0).round() / 10.0,
            outcome: outcome.to_string(),
            nodes: result.nodes.len(),
            warnings: result.warnings,
            errors: result.errors,
            written,
            files,
            by: by.to_string(),
            user,
            computer,
            log: String::new(),
            message,
        }
    }

    fn line(&self) -> String {
        let clean = |s: &str| s.replace([';', '\n', '\r'], " ");
        format!(
            "{};{};{};{};{};{};{};{};{};{};{};{};{}",
            self.started,
            self.seconds,
            self.outcome,
            self.nodes,
            self.warnings,
            self.errors,
            self.written,
            self.files,
            clean(&self.by),
            clean(&self.user),
            clean(&self.computer),
            clean(&self.log),
            clean(&self.message.chars().take(300).collect::<String>())
        )
    }

    fn parse(line: &str) -> Option<Run> {
        let p: Vec<&str> = line.split(';').collect();
        if p.len() < 13 {
            return None;
        }
        Some(Run {
            started: p[0].to_string(),
            seconds: p[1].parse().unwrap_or(0.0),
            outcome: p[2].to_string(),
            nodes: p[3].parse().unwrap_or(0),
            warnings: p[4].parse().unwrap_or(0),
            errors: p[5].parse().unwrap_or(0),
            written: p[6].parse().unwrap_or(0),
            files: p[7].parse().unwrap_or(0),
            by: p[8].to_string(),
            user: p[9].to_string(),
            computer: p[10].to_string(),
            log: p[11].to_string(),
            message: p[12..].join(";"),
        })
    }
}

/// Adds a run to the history of a model, a CSV file that a spreadsheet can open.
pub fn record(model: &Path, run: &Run) -> Result<()> {
    let dir = runs_dir(model);
    std::fs::create_dir_all(&dir).map_err(|e| GbError(format!("Cannot keep the history of the runs in {}: {}", dir.display(), e)))?;
    let file = dir.join("history.csv");
    let new = !file.exists();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&file).map_err(|e| GbError(format!("Cannot write {}: {}", file.display(), e)))?;
    if new {
        writeln!(f, "{}", HEADER).map_err(|e| GbError(e.to_string()))?;
    }
    writeln!(f, "{}", run.line()).map_err(|e| GbError(e.to_string()))
}

/// The runs of a model, the latest first.
pub fn history(model: &Path) -> Vec<Run> {
    let file = runs_dir(model).join("history.csv");
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut runs: Vec<Run> = text.lines().skip(1).filter_map(Run::parse).collect();
    runs.reverse();
    runs
}

/// The file for the log of a run that starts now.
pub fn log_file(model: &Path, started: chrono::NaiveDateTime) -> PathBuf {
    let dir = runs_dir(model).join("logs");
    let stamp = started.format("%Y-%m-%d_%H%M%S").to_string();
    // Two runs in the same second get a log each.
    let mut path = dir.join(format!("{}.log", stamp));
    let mut k = 2;
    while path.exists() {
        path = dir.join(format!("{}_{}.log", stamp, k));
        k += 1;
    }
    path
}

/// Removes the logs older than so many days. Gives how many were removed.
pub fn clean_logs(model: &Path, keep_days: u32) -> usize {
    let dir = runs_dir(model).join("logs");
    let Ok(entries) = std::fs::read_dir(&dir) else { return 0 };
    let limit = std::time::Duration::from_secs(keep_days as u64 * 86_400);
    let mut removed = 0;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "log") {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
            if old && std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// Who holds the lock of a model, and since when.
#[derive(Debug, Clone, PartialEq)]
pub struct Holder {
    pub pid: u32,
    pub computer: String,
    pub user: String,
    pub since: String,
    pub by: String,
}

impl Holder {
    pub fn describe(&self) -> String {
        format!(
            "by {} as {} on {} since {} (process {})",
            self.by,
            if self.user.is_empty() { "someone" } else { &self.user },
            self.computer,
            self.since,
            self.pid
        )
    }
}

/// The lock of a model while it runs. It is let go when this is dropped, also when the run
/// fails.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Whether a process runs on this computer.
fn alive(pid: u32) -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        Some(Path::new(&format!("/proc/{}", pid)).exists())
    }
    #[cfg(windows)]
    {
        // tasklist answers with the process, or with a line that says there is none.
        let out = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"]).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).contains(&format!("\"{}\"", pid)))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = pid;
        None
    }
}

fn read_holder(path: &Path) -> Option<Holder> {
    let text = std::fs::read_to_string(path).ok()?;
    let p: Vec<&str> = text.trim().split(';').collect();
    Some(Holder {
        pid: p.first()?.parse().ok()?,
        computer: p.get(1).unwrap_or(&"").to_string(),
        user: p.get(2).unwrap_or(&"").to_string(),
        since: p.get(3).unwrap_or(&"").to_string(),
        by: p.get(4).unwrap_or(&"").to_string(),
    })
}

/// Who holds the lock of a model now, if anyone does. A lock left by a process that has
/// ended on this computer, or older than `stale_hours`, counts as let go.
pub fn holder(model: &Path, stale_hours: f64) -> Option<Holder> {
    let path = runs_dir(model).join("lock");
    let h = read_holder(&path)?;
    let (_, computer) = who();
    if h.computer == computer && alive(h.pid) == Some(false) {
        return None;
    }
    let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if age.is_some_and(|a| a.as_secs_f64() > stale_hours * 3600.0) {
        return None;
    }
    Some(h)
}

/// Takes the lock of a model, unless someone holds it.
pub fn lock(model: &Path, by: &str, stale_hours: f64) -> std::result::Result<Lock, Holder> {
    let dir = runs_dir(model);
    let path = dir.join("lock");
    if let Some(h) = holder(model, stale_hours) {
        return Err(h);
    }
    // A lock that has been let go is taken over.
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::create_dir_all(&dir);
    let (user, computer) = who();
    let line = format!("{};{};{};{};{}", std::process::id(), computer, user, chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), by);
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut f) => {
            let _ = f.write_all(line.as_bytes());
            Ok(Lock { path })
        }
        // Someone took it in the same moment.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(read_holder(&path).unwrap_or(Holder { pid: 0, computer: String::new(), user: String::new(), since: String::new(), by: "another run".into() }))
        }
        // The folder cannot be written to: run without a lock rather than not at all.
        Err(_) => Ok(Lock { path: PathBuf::new() }),
    }
}

/// Removes the lock of a model by hand, when it was left by a run that crashed.
pub fn unlock(model: &Path) -> bool {
    std::fs::remove_file(runs_dir(model).join("lock")).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_the_logs_and_the_lock() {
        let dir = std::env::temp_dir().join(format!("og-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("house.ographis");
        assert_eq!(runs_dir(&model), dir.join("house.runs"));
        let result = RunResult { nodes: Default::default(), log: Vec::new(), order: Vec::new(), errors: 1, warnings: 2, ms: 1234.0 };
        let mut run = Run::of(&result, chrono::NaiveDate::from_ymd_opt(2026, 10, 22).unwrap().and_hms_opt(18, 30, 0).unwrap(), "og", false);
        run.message = "Input: a; b".into();
        record(&model, &run).unwrap();
        run.outcome = "ok".into();
        record(&model, &run).unwrap();
        let h = history(&model);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].outcome, "ok", "the latest first");
        assert_eq!(h[1].outcome, "errors");
        assert_eq!((h[1].seconds, h[1].warnings, h[1].message.as_str()), (1.2, 2, "Input: a  b"));
        // The lock: taken, held against a second run, and let go when dropped.
        let l = lock(&model, "og", 24.0).unwrap();
        let held = lock(&model, "program", 24.0).unwrap_err();
        assert_eq!(held.pid, std::process::id());
        assert_eq!(held.by, "og");
        drop(l);
        assert!(holder(&model, 24.0).is_none());
        // A lock older than the limit counts as let go.
        let _l = lock(&model, "og", 24.0).unwrap();
        assert!(holder(&model, 0.0).is_none());
        assert!(unlock(&model));
        // Old logs go.
        let log = log_file(&model, chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap());
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "x").unwrap();
        assert_eq!(clean_logs(&model, 30), 0, "just written");
        assert_eq!(clean_logs(&model, 0), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
