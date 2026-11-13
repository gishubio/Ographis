//! What the window of Ographis and an assistant connected by `og mcp` tell each other, through
//! files in the folder of the settings: which models are open in the window, with their
//! changes that are not saved yet, and which model the assistant asks the window to open.
//! Nothing goes over the network.

use crate::model::RasterImage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A model open in a window of Ographis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenModel {
    /// The file, or none for a model that is not saved yet.
    pub path: Option<PathBuf>,
    pub name: String,
    /// It has changes that are not saved.
    pub dirty: bool,
    /// The model as it is in the window, with the changes, without passwords and keys.
    pub model: String,
    /// The id of the node selected, if any.
    #[serde(default)]
    pub selected: Option<String>,
    pub process: u32,
}

/// A window tells again that it is open at least this often; one that has been silent for
/// longer has closed or stopped.
pub const HEARTBEAT: Duration = Duration::from_secs(5);

fn folder() -> Option<PathBuf> {
    crate::settings::settings_dir().map(|d| d.join("open"))
}

fn own_file(dir: &Path) -> PathBuf {
    dir.join(format!("window-{}.json", std::process::id()))
}

/// Tells that a model is open in this window, as it is now.
pub fn publish(open: &OpenModel) {
    if let Some(dir) = folder() {
        publish_in(&dir, open);
    }
}

fn publish_in(dir: &Path, open: &OpenModel) {
    let file = own_file(dir);
    let _ = std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")));
    let mut open = open.clone();
    if let Ok(ws) = crate::run::Workspace::from_json(&open.model) {
        open.model = crate::bugreport::without_secrets(&ws).to_json();
    }
    if let Ok(text) = serde_json::to_string(&open) {
        // Written beside and moved, so that a reader never sees half of it.
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
}

/// This window has closed.
pub fn withdraw() {
    if let Some(dir) = folder() {
        let _ = std::fs::remove_file(own_file(&dir));
    }
}

/// The models open in windows of Ographis now, the latest first.
pub fn open_models() -> Vec<OpenModel> {
    folder().map(|d| open_models_in(&d)).unwrap_or_default()
}

fn open_models_in(dir: &Path) -> Vec<OpenModel> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(SystemTime, OpenModel)> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("window-")) || p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(when) = e.metadata().and_then(|m| m.modified()) else { continue };
        if when.elapsed().unwrap_or_default() > HEARTBEAT * 4 {
            // Left by a window that stopped.
            let _ = std::fs::remove_file(&p);
            continue;
        }
        if let Some(m) = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<OpenModel>(&t).ok()) {
            found.push((when, m));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, m)| m).collect()
}

/// Asks the window to open a model, as when an assistant has made one. Gives false when no
/// window is open to ask.
pub fn ask_to_open(path: &Path) -> bool {
    folder().is_some_and(|d| ask_to_open_in(&d, path))
}

fn ask_to_open_in(dir: &Path, path: &Path) -> bool {
    if open_models_in(dir).is_empty() {
        return false;
    }
    let dir = dir.join("requests");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::fs::write(dir.join(format!("{}.txt", stamp)), path.to_string_lossy().as_bytes()).is_ok()
}

/// The models asked to be opened since last time, the oldest first. Each is asked once.
pub fn take_requests() -> Vec<PathBuf> {
    folder().map(|d| take_requests_in(&d)).unwrap_or_default()
}

fn take_requests_in(dir: &Path) -> Vec<PathBuf> {
    let dir = dir.join("requests");
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        // Taken by renaming first, so that two windows do not both open it.
        let taken = f.with_extension("taken");
        if std::fs::rename(&f, &taken).is_ok() {
            if let Ok(text) = std::fs::read_to_string(&taken) {
                out.push(PathBuf::from(text.trim()));
            }
            let _ = std::fs::remove_file(&taken);
        }
    }
    out
}

/// A picture as PNG.
pub fn png(img: &RasterImage) -> crate::model::Result<Vec<u8>> {
    let mut out = Vec::new();
    let pic = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.rgba.clone())
        .ok_or_else(|| crate::model::GbError("The picture has the wrong size.".into()))?;
    pic.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| crate::model::GbError(format!("The picture cannot be made: {}", e)))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_tells_what_is_open_and_takes_requests() {
        let dir = std::env::temp_dir().join(format!("og-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(open_models_in(&dir).is_empty());
        assert!(!ask_to_open_in(&dir, Path::new("/x/a.ographis")), "no window to ask");
        let model = r#"{"format":"ographis/1","nodes":[{"id":"db","type":"Reader","params":{"path":"PG:host=h password=secret"}}]}"#;
        publish_in(
            &dir,
            &OpenModel { path: Some("/x/m.ographis".into()), name: "m".into(), dirty: true, model: model.into(), selected: None, process: std::process::id() },
        );
        let open = open_models_in(&dir);
        assert_eq!(open.len(), 1);
        assert!(open[0].dirty && !open[0].model.contains("secret"), "{}", open[0].model);
        assert!(ask_to_open_in(&dir, Path::new("/x/a.ographis")));
        assert_eq!(take_requests_in(&dir), vec![PathBuf::from("/x/a.ographis")]);
        assert!(take_requests_in(&dir).is_empty(), "asked once");
        let _ = std::fs::remove_file(own_file(&dir));
        assert!(open_models_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
