//! When something goes wrong inside Ographis itself: a fault in a tool becomes an error on
//! that node, and the run goes on; every fault is written to a log that the user can send in.
//!
//! The log is crash-log.txt in the folder for settings of the user (%APPDATA%\Ographis on
//! Windows), at most about a megabyte: when it grows past that, the older half is dropped.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// The log of faults.
pub fn log_file() -> Option<PathBuf> {
    crate::settings::settings_dir().map(|d| d.join("crash-log.txt"))
}

/// What the last fault was, for the error of the node it happened in.
static LAST: Mutex<Option<String>> = Mutex::new(None);

const MOST: u64 = 1_000_000;

/// Writes every fault to the log, with where in the code it was and how it got there, and
/// remembers it for [`guarded`]. `program` names what is running, such as "Ographis 0.72".
/// The faults are also told on the console, as before.
pub fn install(program: &'static str) {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A fault that is expected, and caught by `quietly`, is no fault of the program.
        if QUIET.with(|q| q.get()) {
            return;
        }
        let msg = message(info.payload());
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_else(|| "an unknown place".into());
        if let Ok(mut last) = LAST.lock() {
            *last = Some(format!("{} (at {})", msg, place));
        }
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let trace = std::backtrace::Backtrace::force_capture();
        let when = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let entry = format!(
            "==== {} · {} · {} {}\n{}\nat {} on the thread {}\n\n{}\n\n",
            when,
            program,
            std::env::consts::OS,
            std::env::consts::ARCH,
            msg,
            place,
            thread,
            trace
        );
        write(&entry);
        before(info);
    }));
}

fn message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "a fault without a message".into()
    }
}

fn write(entry: &str) {
    let Some(file) = log_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A long log keeps its newer half.
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > MOST) {
        if let Ok(old) = std::fs::read_to_string(&file) {
            let cut = old.len() / 2;
            let from = old[cut..].find("\n==== ").map_or(old.len(), |k| cut + k + 1);
            let _ = std::fs::write(&file, &old[from..]);
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(entry.as_bytes());
    }
}

/// Runs `f`, and turns a fault in it into an error that says what happened and where the
/// log is, so that one tool that fails does not end the program.
pub fn guarded<T>(what: &str, f: impl FnOnce() -> crate::Result<T>) -> crate::Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let detail = LAST.lock().ok().and_then(|mut l| l.take()).unwrap_or_else(|| message(payload.as_ref()));
            let log = log_file().map(|p| format!(" It is written in {}.", p.display())).unwrap_or_default();
            Err(crate::GbError(format!(
                "{} stopped on a fault in Ographis itself: {}. The rest of the model ran as usual. Please report it, with the model and the log if you can.{}",
                what, detail, log
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fault_becomes_an_error_and_is_logged() {
        let dir = std::env::temp_dir().join(format!("gb-crash-{}", std::process::id()));
        std::env::set_var("OGRAPHIS_SETTINGS_DIR", &dir);
        install("Ographis test");
        let r: crate::Result<()> = guarded("Buffer", || {
            let v: Vec<u8> = Vec::new();
            #[allow(clippy::useless_vec)]
            let _ = v[3];
            Ok(())
        });
        let e = r.unwrap_err().0;
        assert!(e.starts_with("Buffer stopped on a fault in Ographis itself") && e.contains("crash.rs"), "{e}");
        let log = std::fs::read_to_string(dir.join("crash-log.txt")).unwrap();
        assert!(log.contains("Ographis test") && log.contains("index out of bounds"), "{log}");
        assert_eq!(guarded("Count", || Ok(7)).unwrap(), 7);
        let _ = std::panic::take_hook();
        let _ = std::fs::remove_dir_all(dir);
    }
}

thread_local! {
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs a step that may fail in a library on an awkward case, such as points too close for
/// a triangulation: a fault there gives `None`, without being logged as a fault.
pub fn quietly<R>(f: impl FnOnce() -> R) -> Option<R> {
    let before = QUIET.with(|q| q.replace(true));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok();
    QUIET.with(|q| q.set(before));
    r
}
