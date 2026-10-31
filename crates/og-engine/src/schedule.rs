//! Runs of a model at set times, by the Task Scheduler of Windows: a small command file beside
//! the model starts og run, and schtasks makes a task that starts the file. The command file
//! keeps the line of schtasks short, which may be at most 261 characters, and can be opened
//! to see what is run. On other systems, a line for cron.

use std::path::{Path, PathBuf};

/// How often a model runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Every {
    Day,
    Weekday,
    /// On one day of the week: MON, TUE, WED, THU, FRI, SAT or SUN.
    Week(String),
    /// On a day of the month, 1 to 28.
    Month(u32),
    /// Every so many hours, from the time.
    Hours(u32),
}

/// The days of the week as schtasks names them, with their names.
pub const DAYS: &[(&str, &str)] =
    &[("MON", "Monday"), ("TUE", "Tuesday"), ("WED", "Wednesday"), ("THU", "Thursday"), ("FRI", "Friday"), ("SAT", "Saturday"), ("SUN", "Sunday")];

/// The name of the task of a model, in a folder Ographis of the Task Scheduler.
pub fn task_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
    let clean = clean.trim();
    format!("Ographis\\{}", if clean.is_empty() { "model" } else { clean })
}

/// The command file of a model's scheduled runs, in the folder of its runs.
pub fn script_path(model: &Path) -> PathBuf {
    crate::history::runs_dir(model).join("scheduled run.cmd")
}

/// What the command file holds: the folder of the model, then the command.
pub fn script(model: &Path, command: &str) -> String {
    // By the text, so that a path of Windows is split the same everywhere.
    let full = model.to_string_lossy();
    let full = full.trim_start_matches(r"\\?\");
    let dir = full.rsplit_once(['\\', '/']).map(|(d, _)| d.to_string()).unwrap_or_default();
    format!("@echo off\r\nrem Started by the Task Scheduler of Windows, as Model > Schedule in Ographis set it up.\r\ncd /d \"{}\"\r\n{}\r\nexit /b %ERRORLEVEL%\r\n", dir, command)
}

/// A time as HH:MM, or None.
pub fn clock(time: &str) -> Option<String> {
    let (h, m) = time.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then(|| format!("{:02}:{:02}", h, m))
}

/// The arguments of schtasks that make the task, or what is wrong.
pub fn create_args(name: &str, every: &Every, time: &str, script: &Path) -> Result<Vec<String>, String> {
    let at = clock(time).ok_or_else(|| format!("\"{}\" is not a time. Write it as 06:30.", time))?;
    let mut a: Vec<String> = vec!["/Create".into(), "/F".into(), "/TN".into(), task_name(name), "/TR".into(), format!("\"{}\"", script.display())];
    match every {
        Every::Day => a.extend(["/SC".into(), "DAILY".into()]),
        Every::Weekday => a.extend(["/SC".into(), "WEEKLY".into(), "/D".into(), "MON,TUE,WED,THU,FRI".into()]),
        Every::Week(d) => {
            if !DAYS.iter().any(|x| x.0 == d) {
                return Err(format!("{} is not a day of the week.", d));
            }
            a.extend(["/SC".into(), "WEEKLY".into(), "/D".into(), d.clone()]);
        }
        Every::Month(d) => a.extend(["/SC".into(), "MONTHLY".into(), "/D".into(), (*d).clamp(1, 28).to_string()]),
        Every::Hours(h) => a.extend(["/SC".into(), "HOURLY".into(), "/MO".into(), (*h).clamp(1, 23).to_string()]),
    }
    a.extend(["/ST".into(), at]);
    if a.iter().map(|x| x.len() + 1).sum::<usize>() > 1000 {
        return Err("The name of the task is too long.".into());
    }
    Ok(a)
}

/// The arguments of schtasks that remove the task.
pub fn delete_args(name: &str) -> Vec<String> {
    vec!["/Delete".into(), "/F".into(), "/TN".into(), task_name(name)]
}

/// The arguments of schtasks that ask about the task.
pub fn query_args(name: &str) -> Vec<String> {
    vec!["/Query".into(), "/TN".into(), task_name(name), "/FO".into(), "LIST".into()]
}

/// The same as a line of crontab, for Linux and macOS.
pub fn cron_line(every: &Every, time: &str, command: &str) -> Option<String> {
    let at = clock(time)?;
    let (h, m) = at.split_once(':')?;
    let (h, m) = (h.trim_start_matches('0'), m.trim_start_matches('0'));
    let (h, m) = (if h.is_empty() { "0" } else { h }, if m.is_empty() { "0" } else { m });
    let when = match every {
        Every::Day => format!("{} {} * * *", m, h),
        Every::Weekday => format!("{} {} * * 1-5", m, h),
        Every::Week(d) => format!("{} {} * * {}", m, h, DAYS.iter().position(|x| x.0 == d).map_or(1, |i| (i + 1) % 7)),
        Every::Month(d) => format!("{} {} {} * *", m, h, d.clamp(&1, &28)),
        Every::Hours(n) => format!("{} */{} * * *", m, n.clamp(&1, &23)),
    };
    Some(format!("{} {}", when, command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_for_the_scheduler_of_windows() {
        let s = Path::new(r"C:\GIS\models\roads.runs\scheduled run.cmd");
        let a = create_args("Roads: nightly", &Every::Weekday, "6:05", s).unwrap();
        assert_eq!(a[3], r"Ographis\Roads_ nightly");
        assert_eq!(a[5], r#""C:\GIS\models\roads.runs\scheduled run.cmd""#);
        assert_eq!(&a[6..], ["/SC", "WEEKLY", "/D", "MON,TUE,WED,THU,FRI", "/ST", "06:05"]);
        assert!(create_args("x", &Every::Day, "25:00", s).is_err());
        let h = create_args("x", &Every::Hours(3), "00:30", s).unwrap();
        assert!(h.windows(2).any(|w| w == ["/MO", "3"]));
        assert_eq!(cron_line(&Every::Week("SUN".into()), "07:00", "og run m.ographis").unwrap(), "0 7 * * 0 og run m.ographis");
        let text = script(Path::new(r"C:\GIS\models\roads.ographis"), "og run roads.ographis");
        assert!(text.contains("cd /d \"C:\\GIS\\models\"") && text.contains("\r\nog run roads.ographis\r\n"));
    }
}
