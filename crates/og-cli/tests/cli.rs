//! The command line, run as a program: models and folders of models, the log file and the
//! exit codes.

use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("og-cli-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gb(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_og")).args(args).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
}

fn model(dir: &Path, name: &str, test: &str) {
    std::fs::write(
        dir.join(name),
        format!(
            r#"{{"nodes": [
            {{"id": "a", "type": "Reader", "name": "Towns", "params": {{"path": "towns.csv", "crs": "EPSG:3006"}}}},
            {{"id": "t", "type": "Tester", "name": "Big", "params": {{"test": "{}"}}}},
            {{"id": "w", "type": "Writer", "name": "Big towns", "params": {{"path": "out/$(name).csv"}}}}],
          "edges": [{{"from": ["a", "Output"], "to": ["t", "Input"]}}, {{"from": ["t", "Passed"], "to": ["w", "Input"]}}],
          "variables": [{{"name": "name", "value": "big"}}]}}"#,
            test
        ),
    )
    .unwrap();
}

#[test]
fn models_and_folders_are_run_with_a_log_and_exit_codes() {
    let dir = tmp("run");
    std::fs::write(dir.join("towns.csv"), "name;population;e;n\nBorås;75000;377605;6399600\nKinna;16000;362270;6376250\n").unwrap();
    model(&dir, "a_good.json", "population > 20000");
    model(&dir, "b_bad.json", "nonsense >");
    // A good model gives 0, and a variable can be set.
    let (code, out) = gb(&["run", dir.join("a_good.json").to_str().unwrap(), "--var", "name=chosen"]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("Running") && out.contains("with name=chosen"), "{}", out);
    assert!(dir.join("out/chosen.csv").exists());
    // A failing node gives 1.
    let (code, _) = gb(&["run", dir.join("b_bad.json").to_str().unwrap()]);
    assert_eq!(code, 1);
    // A command written wrong gives 2.
    assert_eq!(gb(&["run", "--nonsense"]).0, 2);
    // A folder runs every model in it by name, and gives the worst code, with a summary.
    let log = dir.join("logs/runs.log");
    let (code, out) = gb(&["run", dir.to_str().unwrap(), "--log", log.to_str().unwrap()]);
    assert_eq!(code, 1, "{}", out);
    let summary: Vec<&str> = out.lines().filter(|l| l.contains("a_good.json  ") || l.contains("b_bad.json  ")).collect();
    assert_eq!(summary.len(), 2, "{}", out);
    assert!(summary[0].contains("a_good.json") && summary[0].contains("OK"));
    assert!(summary[1].contains("b_bad.json") && summary[1].contains("ERRORS"));
    // The log file has every line with the date and time, and grows run by run.
    let text = std::fs::read_to_string(&log).unwrap();
    let first = text.lines().next().unwrap();
    assert!(first.len() > 20 && first.as_bytes()[4] == b'-' && first[11..19].chars().filter(|c| *c == ':').count() == 2, "{}", first);
    let lines = text.lines().count();
    gb(&["run", dir.join("a_good.json").to_str().unwrap(), "--log", log.to_str().unwrap()]);
    assert!(std::fs::read_to_string(&log).unwrap().lines().count() > lines, "the log is added to");
    // Warnings only give 3: moving a stream that has no coordinate system.
    std::fs::write(dir.join("plain.csv"), "name;value\nx;1\n").unwrap();
    let warn = tmp("warn");
    std::fs::copy(dir.join("plain.csv"), warn.join("plain.csv")).unwrap();
    std::fs::write(
        warn.join("warn.json"),
        r#"{"nodes": [{"id": "a", "type": "Reader", "params": {"path": "plain.csv"}}, {"id": "m", "type": "Offsetter", "params": {"dx": "1"}}],
          "edges": [{"from": ["a", "Output"], "to": ["m", "Input"]}]}"#,
    )
    .unwrap();
    let (code, out) = gb(&["run", warn.join("warn.json").to_str().unwrap()]);
    assert_eq!(code, 3, "{}", out);
    assert!(out.contains("WARN"));
}

#[test]
fn runs_are_kept_with_their_logs_and_a_model_runs_once_at_a_time() {
    let dir = tmp("history");
    std::fs::write(dir.join("towns.csv"), "name;population;e;n\nBorås;75000;377605;6399600\nKinna;16000;362270;6376250\n").unwrap();
    model(&dir, "towns.ographis", "population > 20000");
    let file = dir.join("towns.ographis");
    let f = file.to_str().unwrap();
    assert_eq!(gb(&["run", f]).0, 0);
    assert_eq!(gb(&["run", f]).0, 0);
    // A line for each run in history.csv, and a log of each.
    let runs = dir.join("towns.runs");
    let history = std::fs::read_to_string(runs.join("history.csv")).unwrap();
    assert_eq!(history.lines().count(), 3, "{}", history);
    assert!(history.lines().nth(1).unwrap().contains(";ok;"), "{}", history);
    let logs: Vec<_> = std::fs::read_dir(runs.join("logs")).unwrap().flatten().collect();
    assert_eq!(logs.len(), 2);
    let log = std::fs::read_to_string(logs[0].path()).unwrap();
    assert!(log.contains("Run finished"), "{}", log);
    let (code, out) = gb(&["history", f]);
    assert_eq!(code, 0);
    assert!(out.contains("the latest 2 of 2 runs") && out.matches(" ok ").count() == 2, "{}", out);
    // While it runs, it is not started again: a lock held by a process that lives.
    let me = std::process::id();
    let computer = std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_default();
    std::fs::write(runs.join("lock"), format!("{};{};someone;2026-10-22 18:00:00;og", me, computer)).unwrap();
    let (code, out) = gb(&["run", f]);
    assert_eq!(code, 4, "{}", out);
    assert!(out.contains("is running already"), "{}", out);
    assert!(gb(&["history", f]).1.contains("Running now"));
    // --no-lock runs anyway; --unlock removes the lock.
    assert_eq!(gb(&["run", f, "--no-lock", "--no-history"]).0, 0);
    assert!(gb(&["history", f, "--unlock"]).1.contains("was removed"));
    assert_eq!(gb(&["run", f]).0, 0);
    assert_eq!(std::fs::read_to_string(runs.join("history.csv")).unwrap().lines().count(), 4, "no line for the run without history");
}
