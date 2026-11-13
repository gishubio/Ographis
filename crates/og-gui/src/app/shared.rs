//! What this window and an assistant connected by `og mcp` tell each other: the model open
//! here, with its changes, for the assistant to read; the models the assistant asks to have
//! opened; and a model file changed outside the program, which is read again when it has no
//! changes here, so that the user sees a model being built.

use super::*;
use std::time::{Instant, SystemTime};

#[derive(Default)]
pub struct Shared {
    /// When the model was told of last, and what it was then.
    told: Option<(Instant, u64)>,
    /// When the requests and the file were looked at last.
    looked: Option<Instant>,
    /// When the file was changed, as it was when it was read or saved here.
    pub disk: Option<SystemTime>,
    /// The user has been told that the file was changed outside while there are changes here.
    pub warned: bool,
    /// The tests run many windows in one program, which would take each other's requests:
    /// a quiet window shares only when a test asks it to.
    pub when_quiet: bool,
}

pub fn changed_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl App {
    /// Called every frame; does its work about once a second.
    pub(super) fn share(&mut self) {
        if self.quiet && !self.shared.when_quiet {
            return;
        }
        let now = Instant::now();
        if self.shared.looked.is_some_and(|t| now.duration_since(t).as_millis() < 1000) {
            return;
        }
        self.shared.looked = Some(now);
        for p in og_engine::session::take_requests() {
            self.request(Pending::OpenPath(p));
        }
        // The file changed outside the program, such as by an assistant.
        if let Some(path) = self.path.clone().filter(|p| !p.as_os_str().is_empty()) {
            let on_disk = shared::changed_at(&path);
            if on_disk.is_some() && self.shared.disk.is_some() && on_disk != self.shared.disk {
                if !self.dirty {
                    match self.load(&path) {
                        Ok(()) => self.say(format!("{} was changed outside Ographis and is read again.", self.name)),
                        Err(e) => {
                            self.shared.disk = on_disk;
                            self.say(e);
                        }
                    }
                } else if !self.shared.warned {
                    self.shared.warned = true;
                    self.say(format!("{} was changed outside Ographis. Your changes here are not saved: save them as another file to keep both.", self.name));
                }
            }
        }
        // What is open here, for an assistant to read.
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.path, self.dirty, self.revision, &self.name, self.focus.map(|f| f.0), self.shared.disk).hash(&mut h);
        let key = h.finish();
        let due = match self.shared.told {
            None => true,
            Some((t, k)) => k != key || now.duration_since(t) >= og_engine::session::HEARTBEAT,
        };
        if due {
            let selected = self.focus.and_then(|f| self.snarl.get_node(f)).map(|n| n.id.clone());
            og_engine::session::publish(&og_engine::session::OpenModel {
                path: self.path.clone().filter(|p| !p.as_os_str().is_empty()),
                name: self.name.clone(),
                dirty: self.dirty,
                model: self.workspace().to_json(),
                selected,
                process: std::process::id(),
            });
            self.shared.told = Some((now, key));
        }
    }
}
