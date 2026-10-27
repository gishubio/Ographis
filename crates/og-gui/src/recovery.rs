//! Changes that are not saved are kept now and then in a file of their own, so that they
//! can be had back if the program stops without being closed: a fault, a power cut, a
//! computer that restarts for updates.
//!
//! Each running program has its own file in the folder recovery among the settings of the
//! user. The file is removed when the changes are saved or let go of, and when the program
//! closes as usual. A file that has not been written to for a few minutes belongs to a
//! program that is no longer running, and is offered when Ographis starts.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often changes are kept, at most.
pub const EVERY: Duration = Duration::from_secs(30);
/// How long a file must have been left alone before it is taken to be left behind. A
/// running program with changes writes its file again at least this often.
const LEFT_FOR: Duration = Duration::from_secs(180);

pub fn folder() -> Option<PathBuf> {
    og_engine::settings::settings_dir().map(|d| d.join("recovery"))
}

/// The file of this program, and when it was last written.
pub struct Keeper {
    file: Option<PathBuf>,
    last: Option<Instant>,
    written: bool,
}

impl Default for Keeper {
    fn default() -> Self {
        let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        Keeper { file: folder().map(|d| d.join(format!("{}-{}.json", stamp, std::process::id()))), last: None, written: false }
    }
}

impl Keeper {
    /// Called often. Writes the model when it has changes and it is time, and removes the
    /// file when there are no changes to keep. `model` is called only when it is written.
    pub fn tick(&mut self, dirty: bool, name: &str, path: Option<&Path>, model: impl FnOnce() -> String) {
        let Some(file) = &self.file else { return };
        if !dirty {
            if self.written {
                let _ = std::fs::remove_file(file);
                self.written = false;
            }
            self.last = None;
            return;
        }
        // The first changes wait a little, so that a click by mistake is not kept.
        let now = Instant::now();
        let due = match self.last {
            None => {
                self.last = Some(now);
                false
            }
            Some(t) => now.duration_since(t) >= EVERY,
        };
        if !due {
            return;
        }
        self.last = Some(now);
        let model: Value = serde_json::from_str(&model()).unwrap_or(Value::Null);
        let saved = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let doc = json!({ "name": name, "path": path.map(|p| p.display().to_string()), "saved": saved, "model": model });
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Written beside and then moved, so that a cut in the middle leaves the older one.
        let part = file.with_extension("part");
        if std::fs::write(&part, serde_json::to_string(&doc).unwrap_or_default()).is_ok() && std::fs::rename(&part, file).is_ok() {
            self.written = true;
        }
    }

    /// The program closes as it should: nothing is left to offer.
    pub fn clear(&mut self) {
        if let Some(file) = &self.file {
            if self.written {
                let _ = std::fs::remove_file(file);
            }
        }
        self.written = false;
        self.last = None;
    }

    #[cfg(test)]
    fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }
}

/// Changes left behind by a program that stopped.
#[derive(Clone, Debug)]
pub struct Found {
    pub file: PathBuf,
    pub name: String,
    pub path: Option<PathBuf>,
    pub saved: String,
    pub model: String,
}

/// The changes left behind, the newest first.
pub fn left_behind() -> Vec<Found> {
    let Some(dir) = folder() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut found: Vec<(SystemTime, Found)> = Vec::new();
    for e in entries.flatten() {
        let file = e.path();
        if file.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(modified) = e.metadata().and_then(|m| m.modified()) else { continue };
        if SystemTime::now().duration_since(modified).unwrap_or_default() < LEFT_FOR {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            let _ = std::fs::remove_file(&file);
            continue;
        };
        let s = |k: &str| doc.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
        let model = doc.get("model").map(|m| m.to_string()).unwrap_or_default();
        found.push((
            modified,
            Found { name: s("name").unwrap_or_default(), path: s("path").map(PathBuf::from), saved: s("saved").unwrap_or_default(), model, file },
        ));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, f)| f).collect()
}

/// Lets go of changes that were offered: restored or not wanted.
pub fn forget(found: &Found) {
    let _ = std::fs::remove_file(&found.file);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_kept_and_let_go_of() {
        let dir = std::env::temp_dir().join(format!("gb-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OGRAPHIS_SETTINGS_DIR", &dir);
        let mut k = Keeper::default();
        let file = k.file().unwrap().to_path_buf();
        // Not at once, then when it is time.
        k.tick(true, "Roads", Some(Path::new("roads.json")), || r#"{"nodes": []}"#.into());
        assert!(!file.exists());
        k.last = Some(Instant::now() - EVERY);
        k.tick(true, "Roads", Some(Path::new("roads.json")), || r#"{"nodes": []}"#.into());
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["name"], "Roads");
        assert_eq!(doc["model"]["nodes"], json!([]));
        // A running program's file is not offered.
        assert!(left_behind().is_empty());
        // Saved: the file goes.
        k.tick(false, "Roads", None, String::new);
        assert!(!file.exists());
        k.last = Some(Instant::now() - EVERY);
        k.tick(true, "Roads", None, || "{}".into());
        assert!(file.exists());
        k.clear();
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
