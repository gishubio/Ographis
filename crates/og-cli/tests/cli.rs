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

#[test]
fn a_folder_of_models_is_checked_with_a_report_of_what_went_wrong() {
    let dir = tmp("check");
    std::fs::write(dir.join("towns.csv"), "name;population;e;n\nBorås;75000;377605;6399600\nKinna;16000;362270;6376250\n").unwrap();
    model(&dir, "a_good.json", "population > 20000");
    std::fs::create_dir_all(dir.join("more")).unwrap();
    std::fs::write(dir.join("more/towns.csv"), "name;population;e;n\nBorås;75000;377605;6399600\n").unwrap();
    model(&dir.join("more"), "b_bad.json", "nonsense >");
    std::fs::write(dir.join("more/c_broken.ographis"), "{\"nodes\": [").unwrap();
    // Data beside the models is not a model, nor is a folder of runs or of results.
    std::fs::write(dir.join("more/roads.json"), "{\"type\": \"FeatureCollection\", \"features\": []}").unwrap();
    std::fs::create_dir_all(dir.join("a_good.runs")).unwrap();
    model(&dir.join("a_good.runs"), "never.json", "population > 1");
    let report = dir.join("check.json");
    let (code, out) = gb(&["run", dir.to_str().unwrap(), "--recursive", "--quiet", "--no-history", "--report", report.to_str().unwrap()]);
    assert_eq!(code, 1, "{}", out);
    // A line for each model, and what went wrong, node by node, without the whole logs.
    assert!(out.starts_with("Running 3 models in "), "{}", out);
    assert!(out.contains("1/3") && out.contains("3/3") && !out.contains("never.json"), "{}", out);
    assert!(out.contains("more/b_bad.json") && out.contains("    ERROR Big: "), "{}", out);
    assert!(out.contains("3 models: 1 OK, 0 with warnings, 2 with errors"), "{}", out);
    assert!(!out.contains("INFO "), "{}", out);
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let models = r["models"].as_array().unwrap();
    let outcome: Vec<(&str, &str)> = models.iter().map(|m| (m["model"].as_str().unwrap(), m["outcome"].as_str().unwrap())).collect();
    assert_eq!(outcome, [("a_good.json", "OK"), ("more/b_bad.json", "ERRORS"), ("more/c_broken.ographis", "ERRORS")]);
    assert_eq!(models[1]["problems"][0]["node"], "Big");
    assert!(models[2]["problems"][0]["message"].as_str().unwrap().contains("cannot be read"));
    // Markdown and HTML too.
    for name in ["check.md", "check.html"] {
        let path = dir.join(name);
        gb(&["run", dir.to_str().unwrap(), "-r", "-q", "--no-history", "--report", path.to_str().unwrap()]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("more/b_bad.json") && text.contains("Big"), "{}", text);
    }
    // A report is for a folder.
    assert_eq!(gb(&["run", dir.join("a_good.json").to_str().unwrap(), "--report", report.to_str().unwrap()]).0, 2);
}
