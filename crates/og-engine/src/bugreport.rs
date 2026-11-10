//! A zip to attach to an issue on GitHub: what the program is and runs on, the model with
//! its secrets taken out, the log of its latest run and the latest faults. `og report` and
//! Help → Report a problem make it.

use crate::model::{GbError, Result};
use crate::run::Workspace;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What is put in the zip, to tell the user.
pub struct Bundle {
    pub file: PathBuf,
    pub parts: Vec<String>,
}

/// Takes out what could let someone in: settings whose name says secret, password=… in
/// connection strings, and keys and tokens in the query of an address.
pub fn without_secrets(ws: &Workspace) -> Workspace {
    let mut ws = ws.clone();
    for n in ws.nodes.iter_mut() {
        for (k, v) in n.params.iter_mut() {
            if crate::run::is_secret(k) {
                if v.as_str().is_some_and(|s| !s.trim().is_empty() && !s.trim().starts_with("$(")) {
                    *v = serde_json::Value::String("(taken out)".into());
                }
            } else if let Some(s) = v.as_str() {
                let clean = scrub(s);
                if clean != s {
                    *v = serde_json::Value::String(clean);
                }
            }
        }
    }
    for var in ws.variables.iter_mut() {
        if crate::run::is_secret(&var.name) && !var.value.is_empty() {
            var.value = "(taken out)".into();
        }
        var.value = scrub(&var.value);
    }
    ws
}

/// password=… in a connection string, and key=…, token=… and the like in an address.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        // The next name=value, where the name says it is a secret.
        let Some(eq) = rest.find('=') else {
            out.push_str(rest);
            break;
        };
        let start = rest[..eq].rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).map_or(0, |i| i + 1);
        let name = &rest[start..eq];
        let secret = !name.is_empty() && (crate::run::is_secret(name) || matches!(name.to_lowercase().as_str(), "key" | "access_token" | "auth" | "pwd"));
        out.push_str(&rest[..=eq]);
        rest = &rest[eq + 1..];
        if secret {
            let end = rest.find(['&', ' ', ';', '"', '\'', '\n']).unwrap_or(rest.len());
            if end > 0 {
                out.push_str("(taken out)");
            }
            rest = &rest[end..];
        }
    }
    out
}

/// The newest file in a folder, by when it was changed.
fn newest(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().filter(|e| e.path().is_file()).max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok()).map(|e| e.path())
}

/// The last part of a text file, at most so many bytes, from the start of a line.
fn tail(path: &Path, most: usize) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.len() <= most {
        return Some(text);
    }
    let mut from = text.len() - most;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let from = text[from..].find('\n').map_or(from, |i| from + i + 1);
    Some(text[from..].to_string())
}

/// Makes the zip. `about` is what the program and GDAL are, such as from `og version`.
pub fn bundle(model: Option<&Path>, out: &Path, about: &str) -> Result<Bundle> {
    let file = std::fs::File::create(out).map_err(|e| GbError(format!("Cannot write {}: {}", out.display(), e)))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut parts = Vec::new();
    let mut put = |zip: &mut zip::ZipWriter<std::fs::File>, name: &str, text: &str, what: &str| -> Result<()> {
        zip.start_file(name, options).map_err(|e| GbError(format!("Cannot write the zip: {}", e)))?;
        zip.write_all(text.as_bytes()).map_err(|e| GbError(format!("Cannot write the zip: {}", e)))?;
        parts.push(what.to_string());
        Ok(())
    };
    let system = format!(
        "{}\nOperating system: {} {}\nMade: {}\n",
        about.trim(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );
    put(&mut zip, "about.txt", &system, "the version of Ographis and GDAL, and the operating system")?;
    if let Some(model) = model {
        let ws = Workspace::load(model)?;
        let clean = without_secrets(&ws);
        let name = model.file_name().map_or("model.ographis".to_string(), |n| n.to_string_lossy().into_owned());
        let json = serde_json::to_string_pretty(&clean).map_err(|e| GbError(format!("Cannot write the model: {}", e)))?;
        put(&mut zip, &name, &json, "the model, with passwords and keys taken out")?;
        if let Some(log) = newest(&crate::history::runs_dir(model).join("logs")) {
            if let Some(text) = tail(&log, 500_000) {
                put(&mut zip, "latest-run.log", &scrub(&text), "the log of its latest run")?;
            }
        }
    }
    if let Some(text) = crate::crash::log_file().and_then(|f| tail(&f, 200_000)) {
        put(&mut zip, "faults.txt", &scrub(&text), "the latest faults inside Ographis")?;
    }
    zip.finish().map_err(|e| GbError(format!("Cannot write the zip: {}", e)))?;
    Ok(Bundle { file: out.to_path_buf(), parts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_taken_out_of_text() {
        assert_eq!(scrub("PG:host=db dbname=og user=me password=hemligt port=5432"), "PG:host=db dbname=og user=me password=(taken out) port=5432");
        assert_eq!(scrub("https://api.x.se/v1?q=a&api_key=123abc&n=2"), "https://api.x.se/v1?q=a&api_key=(taken out)&n=2");
        assert_eq!(scrub("https://x.se/?key=abc"), "https://x.se/?key=(taken out)");
        assert_eq!(scrub("population = 10000"), "population = 10000");
        assert_eq!(scrub("monkey=banana"), "monkey=banana");
    }

    #[test]
    fn a_report_has_the_model_without_its_secrets_and_the_latest_log() {
        let dir = std::env::temp_dir().join(format!("og-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("m.runs/logs")).unwrap();
        let model = dir.join("m.ographis");
        std::fs::write(
            &model,
            r#"{"format": "ographis/1", "name": "M", "variables": [{"name": "token", "value": "abc"}],
                "nodes": [{"id": "pg", "type": "PostgisWriter", "params": {"host": "db", "password": "hemligt", "table": "t"}},
                          {"id": "h", "type": "HttpCaller", "params": {"url": "'https://x.se/?apikey=123'"}}], "edges": []}"#,
        )
        .unwrap();
        std::fs::write(dir.join("m.runs/logs/2026-11-08_100000.log"), "INFO  Run finished\n").unwrap();
        let out = dir.join("report.zip");
        let b = bundle(Some(&model), &out, "Ographis 0.96 · GDAL 3.8.4").unwrap();
        assert!(b.parts.iter().any(|p| p.contains("latest run")), "{:?}", b.parts);
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("m.ographis").unwrap(), &mut text).unwrap();
        assert!(!text.contains("hemligt") && !text.contains("123") && !text.contains("\"abc\""), "{}", text);
        assert!(text.contains("PostgisWriter") && text.contains("(taken out)"));
        let mut about = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("about.txt").unwrap(), &mut about).unwrap();
        assert!(about.contains("GDAL 3.8.4") && about.contains(std::env::consts::OS));
        assert!(zip.by_name("latest-run.log").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
