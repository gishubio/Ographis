//! The coverages of a Web Coverage Service (WCS), read through GDAL's driver as rasters. GDAL
//! asks the service only for the part that is read, at the cell size it is read at, so a
//! coverage is read within a rectangle, like a raster on the web.
//!
//! A service comes as a name: WCS:<address>. The coverage is the layer of the input, or the
//! coverage= the address names.

use gdal::{Dataset, DatasetOptions, GdalOpenFlags, Metadata};
use og_engine::io::{LayerInfo, Notes};
use og_engine::model::{GbError, Result};
use std::path::{Path, PathBuf};

pub(crate) use og_engine::nodes::is_wcs;

/// The address of the service, without WCS: and without the coverage it names. Without a
/// version in the address, version 1.0.0 is asked for, which nearly every server answers
/// and GDAL reads the same from all of them.
fn service(path: &Path) -> String {
    let s = path.to_string_lossy();
    let u = s.strip_prefix("WCS:").unwrap_or(&s);
    let (base, query) = u.split_once('?').unwrap_or((u, ""));
    let mut rest: Vec<String> = query.split('&').filter(|kv| !kv.is_empty() && !kv.to_lowercase().starts_with("coverage=")).map(str::to_string).collect();
    if !rest.iter().any(|kv| kv.to_lowercase().starts_with("version=")) {
        rest.push("version=1.0.0".into());
    }
    format!("{}?{}", base, rest.join("&"))
}

/// The coverage the address names, if it names one.
fn named(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let query = s.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.eq_ignore_ascii_case("coverage") && !v.is_empty()).then(|| v.to_string())
    })
}

/// Does the address name a coverage?
pub(crate) fn coverage_named(path: &Path) -> bool {
    named(path).is_some()
}

/// Is this the name of one coverage, as `coverage` gives it, with its version?
pub(crate) fn resolved(path: &Path) -> bool {
    let s = path.to_string_lossy().to_lowercase();
    named(path).is_some() && s.contains("version=")
}

/// The name GDAL reads one coverage of the service by.
fn of(service: &str, coverage: &str) -> PathBuf {
    let sep = if service.contains('?') { '&' } else { '?' };
    PathBuf::from(format!("WCS:{}{}coverage={}", service, sep, coverage))
}

/// Opens the service or a coverage of it. What GDAL learns of the service is kept in a
/// folder of Ographis, and asked for again each time it is opened, as the service changes.
pub(crate) fn open(path: &Path) -> Result<Dataset> {
    let cache = std::env::temp_dir().join("ographis-wcs");
    let _ = std::fs::create_dir_all(&cache);
    let cache = format!("CACHE={}", cache.to_string_lossy());
    let options = [cache.as_str(), "REFRESH_CACHE=YES"];
    Dataset::open_ex(
        path,
        DatasetOptions { open_flags: GdalOpenFlags::GDAL_OF_RASTER | GdalOpenFlags::GDAL_OF_READONLY, open_options: Some(&options), ..Default::default() },
    )
    .map_err(|e| GbError(format!("Cannot read {} as a coverage service: {}", service(path), crate::last_gdal_error().unwrap_or_else(|| e.to_string()))))
}

/// The coverages of the service: their names and titles.
pub(crate) fn coverages(path: &Path) -> Result<Vec<(String, String)>> {
    let ds = open(&PathBuf::from(format!("WCS:{}", service(path))))?;
    let list = ds.metadata_domain("SUBDATASETS").unwrap_or_default();
    let value = |key: String| list.iter().find_map(|l| l.strip_prefix(&key).map(|v| v.to_string()));
    let mut out = Vec::new();
    for i in 1.. {
        let Some(name) = value(format!("SUBDATASET_{}_NAME=", i)) else { break };
        let Some(id) = named(Path::new(&name)) else { continue };
        let title = value(format!("SUBDATASET_{}_DESC=", i)).unwrap_or_default();
        out.push((id, title));
    }
    if out.is_empty() {
        return Err(GbError(format!("{} has no coverages.", service(path))));
    }
    Ok(out)
}

/// The coverage to read: the one asked for, the one the address names, or the first.
pub(crate) fn coverage(path: &Path, asked: Option<&str>, notes: &mut Notes) -> Result<PathBuf> {
    if let Some(name) = asked.filter(|a| !a.trim().is_empty()) {
        return Ok(of(&service(path), name.trim()));
    }
    if let Some(name) = named(path) {
        return Ok(of(&service(path), &name));
    }
    let all = coverages(path)?;
    let (first, _) = &all[0];
    if all.len() > 1 {
        notes.info(format!("the first of {} coverages, {}; choose another under Layers", all.len(), first));
    }
    Ok(of(&service(path), first))
}

/// The coverages as layers, for the list of layers in the input.
pub(crate) fn layers(path: &Path) -> Result<Vec<LayerInfo>> {
    Ok(coverages(path)?
        .into_iter()
        .map(|(name, title)| LayerInfo {
            geometry: if title.is_empty() || title == name { "raster coverage".into() } else { format!("raster coverage: {}", title) },
            name,
            count: None,
            crs: None,
            crs_name: None,
            fields: Vec::new(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coverage_is_named_in_the_address() {
        let p = Path::new("WCS:https://karta.example.se/geoserver/wcs?service=WCS&version=1.0.0&coverage=dem:heights");
        assert_eq!(service(p), "https://karta.example.se/geoserver/wcs?service=WCS&version=1.0.0");
        assert_eq!(named(p).as_deref(), Some("dem:heights"));
        assert_eq!(of(&service(p), "dem:slope"), PathBuf::from("WCS:https://karta.example.se/geoserver/wcs?service=WCS&version=1.0.0&coverage=dem:slope"));
        assert_eq!(of(&service(Path::new("WCS:https://h/wcs")), "a"), PathBuf::from("WCS:https://h/wcs?version=1.0.0&coverage=a"));
    }
}
