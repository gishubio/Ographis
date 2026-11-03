//! Make zip file: what the outputs wrote, and other files, packed into one.

use super::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(in crate::nodes) const WHICH: &[(&str, &str)] =
    &[("all", "Everything the outputs wrote"), ("named", "What the outputs named below wrote"), ("none", "Nothing, only the files below")];
pub(in crate::nodes) const PLACES: &[(&str, &str)] =
    &[("folders", "In their folders, as under the folder of the model"), ("flat", "All in the top of the zip file")];

pub(in crate::nodes) const ZIP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "Zip file",
        kind: ParamKind::FileSave,
        default: "",
        hint: "Such as out/result.zip. {now('%Y-%m-%d')} puts the date in the name",
    },
    ParamSpec {
        key: "outputs",
        label: "From the outputs",
        kind: ParamKind::Choice(WHICH),
        default: "all",
        hint: "The files they wrote in this run, all of a Shapefile together",
    },
    ParamSpec { key: "names", label: "Outputs by name", kind: ParamKind::Text, default: "", hint: "The names of the outputs, separated by ;" },
    ParamSpec {
        key: "extra",
        label: "Other files and folders",
        kind: ParamKind::Text,
        default: "",
        hint: "Separated by ;, such as README.txt; docs/terms.pdf; styles. A folder goes in with all it holds",
    },
    ParamSpec { key: "places", label: "In the zip file", kind: ParamKind::Choice(PLACES), default: "folders", hint: "" },
];

/// The name a file gets in the zip: its path under the folder of the model, or under the
/// folder it was chosen from, with / between the parts.
pub(in crate::nodes) fn name_in_zip(file: &Path, base: &Path, flat: bool) -> String {
    let name = || file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if flat {
        return name();
    }
    match file.strip_prefix(base) {
        Ok(rest) => rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"),
        Err(_) => name(),
    }
}

/// A file with the files that belong to it: a Shapefile is a .shp with .shx, .dbf, .prj
/// and .cpg beside it, all with the same name.
pub(in crate::nodes) fn with_companions(f: &Path) -> Vec<PathBuf> {
    let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "shp" | "tab") {
        return vec![f.to_path_buf()];
    }
    let (Some(dir), Some(stem)) = (f.parent(), f.file_stem()) else { return vec![f.to_path_buf()] };
    let mut out = vec![f.to_path_buf()];
    let mut others: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p != f && p.is_file() && p.file_stem() == Some(stem)).collect())
        .unwrap_or_default();
    others.retain(|p| !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("part")));
    others.sort();
    out.extend(others);
    out
}

pub(in crate::nodes) fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

pub(in crate::nodes) fn run_zip(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let raw = p.need("path", "Choose where to write the zip file.")?;
    // The name may hold the date of the run, as the names of other outputs can.
    let named = match super::NameTemplate::parse(&raw, &[])? {
        Some(t) => t.name_for(&crate::model::Feature { z: None, attrs: Vec::new(), geom: None }, 0, None, true)?,
        None => raw,
    };
    let path = ctx.resolve(&named);
    if !path.to_string_lossy().to_lowercase().ends_with(".zip") {
        return err("The zip file should end with .zip.");
    }
    let flat = p.get("places").as_deref() == Some("flat");
    // (where it is, what it was chosen as) for each file, in order, each once.
    let mut files: Vec<(PathBuf, PathBuf)> = Vec::new();
    let add = |f: PathBuf, base: PathBuf, files: &mut Vec<(PathBuf, PathBuf)>| {
        if f != path && !files.iter().any(|x| x.0 == f) {
            files.push((f, base));
        }
    };
    let base = ctx.base_dir.to_path_buf();
    match p.get("outputs").as_deref() {
        Some("none") => {}
        which => {
            let names: Vec<String> = p.get("names").unwrap_or_default().split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            if which == Some("named") && names.is_empty() {
                return err("Write the names of the outputs whose files go in, separated by ;.");
            }
            let mut found_names = Vec::new();
            for (label, f) in &ctx.written {
                if which == Some("named") && !names.iter().any(|n| n.eq_ignore_ascii_case(label)) {
                    continue;
                }
                found_names.push(label.clone());
                for part in with_companions(f) {
                    add(part, base.clone(), &mut files);
                }
            }
            for n in &names {
                if which == Some("named") && !found_names.iter().any(|f| f.eq_ignore_ascii_case(n)) {
                    ctx.notes.warn(format!("No output named {} wrote anything in this run.", n));
                }
            }
        }
    }
    for item in p.get("extra").unwrap_or_default().split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let at = ctx.resolve(item);
        if at.is_dir() {
            let mut inside = Vec::new();
            walk(&at, &mut inside).map_err(|e| GbError(format!("Cannot read the folder {}: {}", at.display(), e)))?;
            // A folder goes in under its own name.
            let parent = at.parent().map(Path::to_path_buf).unwrap_or_else(|| base.clone());
            let from = if at.starts_with(&base) && !flat { base.clone() } else { parent };
            for f in inside {
                add(f, from.clone(), &mut files);
            }
        } else if at.is_file() {
            let from = if at.starts_with(&base) { base.clone() } else { at.parent().map(Path::to_path_buf).unwrap_or_default() };
            add(at, from, &mut files);
        } else {
            return err(format!("{} is not there.", at.display()));
        }
    }
    if files.is_empty() {
        return err("There is nothing to put in the zip file: no output wrote anything in this run, and no other files are given.");
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot make the folder {}: {}", dir.display(), e)))?;
    }
    let partial = path.with_extension("zip.part");
    let file = std::fs::File::create(&partial).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let mut names: Vec<String> = Vec::new();
    let mut bytes = 0u64;
    let total = files.len() as u64;
    for (k, (f, from)) in files.iter().enumerate() {
        (ctx.progress)("packed files", k as u64, Some(total));
        if ctx.stopped() {
            drop(zip);
            let _ = std::fs::remove_file(&partial);
            return err(crate::model::STOPPED);
        }
        let mut name = name_in_zip(f, from, flat);
        // Two files with the same name, as can happen when all go in the top: a number.
        if names.contains(&name) {
            let (stem, ext) = match name.rsplit_once('.') {
                Some((s, e)) => (s.to_string(), format!(".{}", e)),
                None => (name.clone(), String::new()),
            };
            let mut i = 2;
            while names.contains(&format!("{}_{}{}", stem, i, ext)) {
                i += 1;
            }
            name = format!("{}_{}{}", stem, i, ext);
        }
        let mut src = std::fs::File::open(f).map_err(|e| GbError(format!("Cannot read {}: {}", f.display(), e)))?;
        zip.start_file(name.as_str(), opts).map_err(|e| GbError(format!("Cannot add {} to the zip file: {}", name, e)))?;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = src.read(&mut buf).map_err(|e| GbError(format!("Cannot read {}: {}", f.display(), e)))?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            zip.write_all(&buf[..n]).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        }
        names.push(name);
    }
    zip.finish().map_err(|e| GbError(format!("Cannot finish {}: {}", path.display(), e)))?;
    std::fs::rename(&partial, &path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    ctx.notes.info(format!(
        "{} files, {} MB packed to {} MB",
        fmt_count(names.len() as u64),
        crate::model::fmt_real((bytes as f64 / 1e6 * 10.0).round() / 10.0),
        crate::model::fmt_real((size as f64 / 1e6 * 10.0).round() / 10.0)
    ));
    ctx.files.push(path);
    Ok(Vec::new())
}
