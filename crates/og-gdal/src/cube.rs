//! Data cubes: NetCDF and Zarr, variables over a grid with time or other dimensions, such as
//! climate and weather data. Each variable is a layer. Read through the classic rasters of
//! GDAL, where the steps of time are the bands, named after their dates.

use gdal::{Dataset, DatasetOptions, GdalOpenFlags, Metadata};
use og_engine::{GbError, Result};
use std::path::{Path, PathBuf};

/// The folder of a Zarr store, also when one of its own files is chosen.
pub fn zarr_root(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if matches!(name.as_str(), ".zgroup" | ".zattrs" | ".zmetadata" | "zarr.json" | ".zarray") {
        return path.parent().map(Path::to_path_buf);
    }
    (crate::ext_of(path) == "zarr").then(|| path.to_path_buf())
}

/// Whether the file is a data cube that this module reads.
pub fn is_cube(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if s.starts_with("NETCDF:") || s.starts_with("ZARR:") {
        return false;
    }
    og_engine::io::is_cube(&crate::ext_of(path)) || zarr_root(path).is_some()
}

fn open(name: &str) -> Result<Dataset> {
    Dataset::open_ex(name, DatasetOptions { open_flags: GdalOpenFlags::GDAL_OF_RASTER | GdalOpenFlags::GDAL_OF_READONLY, ..Default::default() })
        .map_err(|e| GbError(format!("Cannot open {}: {}", name, e)))
}

/// Coordinates and bounds, which are not variables of their own to read.
fn is_coordinate(name: &str) -> bool {
    let n = name.to_lowercase();
    matches!(n.as_str(), "lat" | "lon" | "latitude" | "longitude" | "x" | "y" | "time" | "crs" | "spatial_ref" | "rlat" | "rlon" | "height" | "depth" | "level")
        || n.ends_with("_bnds")
        || n.ends_with("_bounds")
}

/// The number of dimensions of an array of a Zarr store, by its metadata.
fn zarr_dims(dir: &Path) -> Option<usize> {
    for f in [".zarray", "zarr.json"] {
        if let Ok(text) = std::fs::read_to_string(dir.join(f)) {
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            if f == "zarr.json" && v["node_type"] != "array" {
                return None;
            }
            return v["shape"].as_array().map(|a| a.len());
        }
    }
    None
}

/// The variables of a cube that lie on a grid: their names and how GDAL opens each.
pub fn variables(path: &Path) -> Result<Vec<(String, String)>> {
    if let Some(root) = zarr_root(path) {
        let mut out = Vec::new();
        let mut look = |dir: &Path, prefix: &str| {
            let mut entries: Vec<_> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
            entries.sort();
            for e in entries.into_iter().filter(|e| e.is_dir()) {
                let name = e.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if zarr_dims(&e).is_some_and(|d| d >= 2) && !is_coordinate(&name) {
                    out.push((format!("{}{}", prefix, name), format!("ZARR:\"{}\":/{}{}", root.display(), prefix, name)));
                }
            }
        };
        look(&root, "");
        // The arrays of the groups one level down.
        let groups: Vec<PathBuf> = std::fs::read_dir(&root)
            .map(|d| {
                d.flatten().map(|e| e.path()).filter(|e| e.join(".zgroup").is_file() || (e.join("zarr.json").is_file() && zarr_dims(e).is_none())).collect()
            })
            .unwrap_or_default();
        for g in groups {
            let name = g.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            look(&g, &format!("{}/", name));
        }
        if out.is_empty() {
            return Err(GbError(format!("{} has no variables on a grid of two dimensions or more.", root.display())));
        }
        return Ok(out);
    }
    let p = crate::gdal_path(path).to_string_lossy().into_owned();
    let ds = open(&p)?;
    let subs = ds.metadata_domain("SUBDATASETS").unwrap_or_default();
    let mut out = Vec::new();
    for item in &subs {
        let Some((key, value)) = item.split_once('=') else { continue };
        if !key.ends_with("_NAME") {
            continue;
        }
        let desc = subs.iter().find_map(|d| d.strip_prefix(&format!("{}=", key.replace("_NAME", "_DESC")))).unwrap_or("");
        // [3x10x20] tas (32-bit floating-point): two dimensions or more.
        let dims = desc.split(']').next().map_or(2, |d| d.matches('x').count() + 1);
        let name = value.rsplit(':').next().unwrap_or(value).trim_matches('/').to_string();
        if dims >= 2 && !is_coordinate(&name) {
            out.push((name, value.to_string()));
        }
    }
    if out.is_empty() && ds.raster_count() > 0 {
        let name = ds
            .rasterband(1)
            .ok()
            .and_then(|b| b.metadata_item("NETCDF_VARNAME", ""))
            .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        let open_as = format!("NETCDF:\"{}\":{}", p, name);
        out.push((name, open_as));
    }
    if out.is_empty() {
        return Err(GbError(format!("{} has no variables on a grid.", path.display())));
    }
    Ok(out)
}

/// The variable asked for, or the first, with a note of the others.
pub fn choose(path: &Path, layer: Option<&str>, notes: &mut og_engine::io::Notes) -> Result<PathBuf> {
    let vars = variables(path)?;
    let names: Vec<&str> = vars.iter().map(|v| v.0.as_str()).collect();
    let found = match layer.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => vars
            .iter()
            .find(|v| v.0 == l || v.0.eq_ignore_ascii_case(l))
            .ok_or_else(|| GbError(format!("{} has no variable {}. It has {}.", path.display(), l, names.join(", "))))?,
        None => {
            if vars.len() > 1 {
                notes.info(format!("the variable {}; choose another as the layer: {}", vars[0].0, names[1..].join(", ")));
            }
            &vars[0]
        }
    };
    Ok(PathBuf::from(&found.1))
}

/// A date from days since 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A value of time with units such as "days since 2026-01-01" as a date, and the time of
/// day when it is not midnight: 2026_02_01 or 2026_02_01_0630.
pub fn time_label(value: f64, units: &str) -> Option<String> {
    let (unit, since) = units.split_once(" since ")?;
    let secs_per = match unit.trim().to_lowercase().trim_end_matches('s') {
        "second" | "sec" => 1.0,
        "minute" | "min" => 60.0,
        "hour" | "hr" | "h" => 3600.0,
        "day" | "d" => 86_400.0,
        _ => return None,
    };
    let since = since.trim().replace('T', " ");
    let mut parts = since.split(' ');
    let date: Vec<i64> = parts.next()?.split('-').filter_map(|x| x.parse().ok()).collect();
    if date.len() < 3 {
        return None;
    }
    let clock: Vec<f64> = parts.next().map(|t| t.trim_end_matches('Z').split(':').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let start = days_from_civil(date[0], date[1] as u32, date[2] as u32) as f64 * 86_400.0
        + clock.first().unwrap_or(&0.0) * 3600.0
        + clock.get(1).unwrap_or(&0.0) * 60.0
        + clock.get(2).unwrap_or(&0.0);
    let t = (start + value * secs_per).round() as i64;
    let (y, m, d) = civil(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    Some(if s == 0 { format!("{}_{:02}_{:02}", y, m, d) } else { format!("{}_{:02}_{:02}_{:02}{:02}", y, m, d, s / 3600, s % 3600 / 60) })
}

/// The name of a band of a cube from where it lies along the other dimensions, such as
/// time_2026_02_01, or None for a band of an ordinary raster.
pub fn band_label(ds: &Dataset, band: usize) -> Option<String> {
    let b = ds.rasterband(band).ok()?;
    let items = b.metadata_domain("").unwrap_or_default();
    let mut parts = Vec::new();
    for item in &items {
        let Some((key, value)) = item.split_once('=') else { continue };
        // NetCDF: NETCDF_DIM_time=31, with the units on the dataset as time#units.
        let (dim, units) = if let Some(dim) = key.strip_prefix("NETCDF_DIM_") {
            (dim.to_string(), ds.metadata_item(&format!("{}#units", dim), ""))
        // Zarr: DIM_time_VALUE=31 and DIM_time_UNIT.
        } else if let Some(dim) = key.strip_prefix("DIM_").and_then(|k| k.strip_suffix("_VALUE")) {
            (dim.to_string(), b.metadata_item(&format!("DIM_{}_UNIT", dim), ""))
        } else {
            continue;
        };
        let v: f64 = value.trim().parse().ok()?;
        let label = units.as_deref().and_then(|u| time_label(v, u)).unwrap_or_else(|| og_engine::model::fmt_real(v).replace(['.', '-'], "_"));
        parts.push(format!("{}_{}", dim, label));
    }
    (!parts.is_empty()).then(|| parts.join("_"))
}

/// The band to read as a grid or a picture: by its number, or by the start of its time such
/// as 2026-07 or 2026-07-15.
pub fn step_band(ds: &Dataset, step: Option<&str>) -> Result<usize> {
    let Some(step) = step.map(str::trim).filter(|s| !s.is_empty()) else { return Ok(1) };
    let n = ds.raster_count();
    // A number is the step, unless it is a year.
    if let Ok(i) = step.parse::<usize>().ok().filter(|i| (1..=n).contains(i) || step.len() != 4).ok_or(()) {
        return if (1..=n).contains(&i) { Ok(i) } else { Err(GbError(format!("There are {} time steps, so step {} is not among them.", n, i))) };
    }
    let want = step.replace(['-', ' ', ':', 'T'], "_");
    for i in 1..=n {
        if let Some(label) = band_label(ds, i) {
            if label.split_once('_').is_some_and(|(_, t)| t.starts_with(&want)) {
                return Ok(i);
            }
        }
    }
    let first = band_label(ds, 1).unwrap_or_default();
    let last = band_label(ds, n).unwrap_or_default();
    Err(GbError(format!("No time step starts with {}. The steps go from {} to {}.", step, first, last)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_become_dates() {
        assert_eq!(time_label(31.0, "days since 2026-01-01").as_deref(), Some("2026_02_01"));
        assert_eq!(time_label(6.5, "hours since 2026-01-01 00:00:00").as_deref(), Some("2026_01_01_0630"));
        assert_eq!(time_label(0.0, "seconds since 1970-01-01T00:00:00Z").as_deref(), Some("1970_01_01"));
        assert_eq!(time_label(1.0, "metres"), None);
        assert_eq!(civil(days_from_civil(2024, 2, 29)), (2024, 2, 29));
    }
}
