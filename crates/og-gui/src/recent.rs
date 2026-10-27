//! The models that were opened or saved most recently, for File → Open recent. The list is
//! kept in a small file in the folder for settings of the user: %APPDATA%\Ographis on Windows.

use std::path::{Path, PathBuf};

/// How many models the list holds.
pub const RECENT: usize = 5;

/// The folder where the program keeps what it remembers between sessions.
pub use og_engine::settings::settings_dir;

fn file() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("recent.json"))
}

/// The models, the latest first.
#[derive(Debug, Default, Clone)]
pub struct Recent {
    pub list: Vec<PathBuf>,
}

impl Recent {
    /// The list as it was left. A missing or broken file gives an empty list.
    pub fn load() -> Recent {
        let list = file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("recent").and_then(|l| l.as_array()).cloned())
            .map(|l| l.iter().filter_map(|p| p.as_str().map(PathBuf::from)).take(RECENT).collect())
            .unwrap_or_default();
        Recent { list }
    }

    /// Puts a model first in the list.
    pub fn add(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.list.retain(|p| p != &path);
        self.list.insert(0, path);
        self.list.truncate(RECENT);
        self.save();
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.save();
    }

    /// Writes the list. It is a convenience, so a failure is not reported.
    fn save(&self) {
        let Some(f) = file() else { return };
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let list: Vec<String> = self.list.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let _ = std::fs::write(f, serde_json::to_string_pretty(&serde_json::json!({ "recent": list })).unwrap_or_default());
    }
}
