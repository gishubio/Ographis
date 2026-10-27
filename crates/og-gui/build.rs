//! Records when the program was built, and the versions of the libraries it is built with
//! from Cargo.lock, for the About window. It runs again when a file of the crate or the lock
//! file changes, so the date follows the code.

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // From days since 1970 to a date, after Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    println!("cargo:rustc-env=OGRAPHIS_BUILT={:04}-{:02}-{:02} {:02}:{:02} UTC", year, month, day, rest / 3600, rest % 3600 / 60);
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    // name=version;name=version… for each package in the lock file.
    let lock = std::fs::read_to_string("../../Cargo.lock").unwrap_or_default();
    let mut found = Vec::new();
    let mut name = None;
    for line in lock.lines() {
        if let Some(n) = line.strip_prefix("name = ") {
            name = Some(n.trim_matches('"').to_string());
        } else if let (Some(v), Some(n)) = (line.strip_prefix("version = "), name.take()) {
            found.push(format!("{}={}", n, v.trim_matches('"')));
        }
    }
    println!("cargo:rustc-env=OGRAPHIS_CRATES={}", found.join(";"));
    // The version of the compiler, as rustc tells it.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let v = std::process::Command::new(rustc).arg("--version").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let v = v.split_whitespace().nth(1).unwrap_or("unknown").to_string();
    println!("cargo:rustc-env=OGRAPHIS_RUSTC={}", v);
}
