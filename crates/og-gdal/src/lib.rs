//! GDAL backend: reads everything GDAL can open as vector data, writes GeoPackage,
//! Shapefile, GeoJSON and spreadsheets, and reprojects between coordinate systems with PROJ.

mod db;
mod dxf;
mod pdf;
pub mod raster;
mod services;
pub mod sheets;

use gdal::errors::CplErrType;
use gdal::spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef};
use gdal::vector::{
    geometry_type_has_m, geometry_type_has_z, geometry_type_to_name, FieldDefn, FieldValue, Geometry as OgrGeometry, LayerAccess, LayerOptions, OGRFieldType,
    OGRwkbGeometryType, ToGdal,
};
use gdal::{Dataset, DatasetOptions, DriverManager, GdalOpenFlags};
use og_engine::geom::{self, Family};
use og_engine::io::{is_raster, Backend, CrsInfo, FieldSize, HttpAnswer, LayerInfo, Notes, ReadOpts, WriteOpts};
use og_engine::model::{coerce, Feature, Field, FieldType, GbError, Result, Table, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Once;

thread_local! {
    /// Warnings that GDAL emits while a file is read or written. GDAL reports on the thread
    /// that does the work, so each thread has its own list and a run is not handed the
    /// warnings of something that goes on elsewhere in the program.
    static GDAL_MESSAGES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn gerr(what: impl std::fmt::Display, e: gdal::errors::GdalError) -> GbError {
    // GDAL's own wording, without the numbers of its error classes around it.
    let text = e.to_string();
    let text = match text.find("error msg: '") {
        Some(at) => text[at + 12..].trim_end().trim_end_matches('\'').trim().to_string(),
        None => text,
    };
    GbError(format!("{}: {}", what, text.trim()))
}

pub(crate) fn drain_messages(notes: &mut Notes) {
    for msg in GDAL_MESSAGES.with(|m| std::mem::take(&mut *m.borrow_mut())) {
        notes.warn(format!("GDAL: {}", msg));
    }
}

/// The latest message GDAL gave in this thread, if any.
pub(crate) fn last_gdal_error() -> Option<String> {
    GDAL_MESSAGES.with(|m| m.borrow().last().cloned())
}

pub(crate) fn clear_messages() {
    GDAL_MESSAGES.with(|m| m.borrow_mut().clear());
}

pub struct GdalBackend;

/// One step of the self-test: what was tried and how it went.
pub struct Check {
    pub what: String,
    pub result: std::result::Result<String, String>,
}

impl GdalBackend {
    /// Starts GDAL. Looks for PROJ and GDAL data next to the program, so that a folder
    /// with the program and its data files can be moved without environment variables.
    pub fn new() -> GdalBackend {
        // The error handler and the search paths apply to the whole process and are
        // therefore set only once. Replacing the error handler while another thread is
        // working in GDAL can crash the program.
        static START: Once = Once::new();
        START.call_once(Self::start);
        GdalBackend
    }

    fn start() {
        gdal::config::set_error_handler(|class, _num, msg| {
            if matches!(class, CplErrType::Warning | CplErrType::Failure) {
                GDAL_MESSAGES.with(|m| {
                    let mut m = m.borrow_mut();
                    if m.len() < 20 && !m.iter().any(|x| x == msg) {
                        m.push(msg.to_string());
                    }
                });
            }
        });
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            // A folder made by build.cmd -Package has proj\proj.db next to the program. Its
            // own data then wins over variables set for the whole computer, as PostGIS and
            // QGIS like to set PROJ_LIB, GDAL_DATA and GDAL_DRIVER_PATH to versions of their
            // own that do not fit.
            let packaged = dir.join("proj").join("proj.db").exists();
            if packaged || (std::env::var_os("PROJ_DATA").is_none() && std::env::var_os("PROJ_LIB").is_none()) {
                for cand in ["proj", "share/proj", "proj-data"] {
                    let p = dir.join(cand);
                    if p.join("proj.db").exists() {
                        if let Ok(c) = CString::new(p.to_string_lossy().as_bytes()) {
                            let list = [c.as_ptr(), std::ptr::null()];
                            unsafe { gdal_sys::OSRSetPROJSearchPaths(list.as_ptr()) };
                        }
                        break;
                    }
                }
            }
            if packaged || std::env::var_os("GDAL_DATA").is_none() {
                for cand in ["gdal-data", "share/gdal", "gdal"] {
                    let p = dir.join(cand);
                    if p.join("gdalvrt.xsd").exists() || p.join("gcs.csv").exists() || p.join("ogr_fields_override.schema.json").exists() {
                        let _ = gdal::config::set_config_option("GDAL_DATA", &p.to_string_lossy());
                        break;
                    }
                }
            }
            if packaged {
                // Only the plugins that came with the folder, or none: those of another GDAL
                // can stop the program.
                let plugins = dir.join("gdalplugins");
                let path = if plugins.is_dir() { plugins.to_string_lossy().into_owned() } else { "disable".to_string() };
                let _ = gdal::config::set_config_option("GDAL_DRIVER_PATH", &path);
                // The certificates GDAL's curl checks web addresses against.
                let certs = dir.join("curl-ca-bundle.crt");
                if certs.exists() && std::env::var_os("CURL_CA_BUNDLE").is_none() {
                    let _ = gdal::config::set_config_option("CURL_CA_BUNDLE", &certs.to_string_lossy());
                }
            }
        }
    }

    /// Self-test for a new installation: are the formats there, does PROJ find its
    /// database, and does a known point get the right coordinates?
    pub fn doctor(&self) -> Vec<Check> {
        let mut out = Vec::new();
        out.push(Check { what: "GDAL".into(), result: Ok(gdal::version::VersionInfo::release_name()) });
        for (driver, label) in [
            ("GPKG", "GeoPackage"),
            ("ESRI Shapefile", "Shapefile"),
            ("GeoJSON", "GeoJSON"),
            ("GTiff", "GeoTIFF"),
            ("PNG", "PNG"),
            ("JPEG", "JPEG"),
            ("PDF", "PDF"),
            ("XLSX", "Excel (reading)"),
            ("ODS", "LibreOffice Calc (reading)"),
            ("PostgreSQL", "PostGIS (for later)"),
        ] {
            out.push(Check {
                what: format!("Format: {}", label),
                result: DriverManager::get_driver_by_name(driver).map(|_| "available".to_string()).map_err(|_| "missing from this GDAL build".to_string()),
            });
        }
        clear_messages();
        let proj = SpatialRef::from_epsg(3006);
        let hint = "PROJ cannot find proj.db. Put the proj folder next to the program or set the environment variable PROJ_DATA.";
        out.push(Check {
            what: "PROJ database".into(),
            result: match &proj {
                Ok(s) => Ok(s.name().unwrap_or_else(|| "EPSG:3006".into())),
                Err(_) => Err(hint.to_string()),
            },
        });
        if proj.is_ok() {
            // Gothenburg, roughly at Gustaf Adolfs torg.
            let (mut xs, mut ys) = (vec![11.9746], vec![57.7089]);
            out.push(Check {
                what: "Reprojection WGS 84 → SWEREF 99 TM".into(),
                result: match self.transform(4326, 3006, &mut xs, &mut ys) {
                    Ok(()) if (xs[0] - 319_758.3).abs() < 1.0 && (ys[0] - 6_400_326.0).abs() < 1.0 => Ok(format!("{:.1} E, {:.1} N", xs[0], ys[0])),
                    Ok(()) => Err(format!("wrong answer: {:.1} E, {:.1} N (expected about 319758 E, 6400326 N)", xs[0], ys[0])),
                    Err(e) => Err(e.0),
                },
            });
        }
        clear_messages();
        out
    }
}

impl Default for GdalBackend {
    fn default() -> Self {
        GdalBackend::new()
    }
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_spreadsheet(ext: &str) -> bool {
    matches!(ext, "xlsx" | "xlsm" | "ods")
}

/// Tells GDAL whether the first row of a sheet holds the column names, for as long as the
/// value lives. GDAL also has an open option for this, but it only reaches the first sheet
/// of a workbook, so the setting is made for the thread instead.
struct Headers;

const HEADER_OPTIONS: [&str; 2] = ["OGR_XLSX_HEADERS", "OGR_ODS_HEADERS"];

impl Headers {
    fn set(first_row_has_names: bool) -> Headers {
        for key in HEADER_OPTIONS {
            let _ = gdal::config::set_thread_local_config_option(key, if first_row_has_names { "FORCE" } else { "DISABLE" });
        }
        Headers
    }
}

impl Drop for Headers {
    fn drop(&mut self) {
        for key in HEADER_OPTIONS {
            let _ = gdal::config::clear_thread_local_config_option(key);
        }
    }
}

/// A name that Excel accepts for a sheet: at most 31 characters and none of [ ] : * ? / \.
pub fn sheet_name(wanted: &str) -> String {
    let clean: String = wanted.trim().chars().map(|c| if "[]:*?/\\".contains(c) { '_' } else { c }).take(31).collect();
    let clean = clean.trim_matches('\'').trim().to_string();
    if clean.is_empty() {
        "Sheet1".to_string()
    } else {
        clean
    }
}

/// GDAL's name for a character encoding.
fn gdal_encoding(label: &str) -> String {
    match label.trim().to_lowercase().as_str() {
        "utf-8" | "utf8" | "utf-8-bom" => "UTF-8".into(),
        "windows-1252" | "cp1252" | "1252" | "ansi" => "CP1252".into(),
        "iso-8859-1" | "latin1" | "latin-1" => "ISO-8859-1".into(),
        other => other.to_uppercase(),
    }
}

pub(crate) fn epsg_of(srs: &SpatialRef) -> Option<u32> {
    let code = |s: &SpatialRef| if s.auth_name().as_deref() == Some("EPSG") { s.auth_code().ok().filter(|c| *c > 0).map(|c| c as u32) } else { None };
    if let Some(c) = code(srs) {
        return Some(c);
    }
    let mut copy = srs.clone();
    if copy.auto_identify_epsg().is_ok() {
        if let Some(c) = code(&copy) {
            return Some(c);
        }
    }
    // Last resort: let PROJ look for a system that matches, and accept only confident hits.
    unsafe {
        let mut n: std::ffi::c_int = 0;
        let mut conf: *mut std::ffi::c_int = std::ptr::null_mut();
        let matches = gdal_sys::OSRFindMatches(srs.to_c_hsrs(), std::ptr::null_mut(), &mut n, &mut conf);
        let mut found = None;
        if !matches.is_null() && n > 0 && !conf.is_null() && *conf >= 90 {
            let first = *matches;
            let name = gdal_sys::OSRGetAuthorityName(first, std::ptr::null());
            let c = gdal_sys::OSRGetAuthorityCode(first, std::ptr::null());
            if !name.is_null() && !c.is_null() && CStr::from_ptr(name).to_bytes() == b"EPSG" {
                found = CStr::from_ptr(c).to_str().ok().and_then(|s| s.parse::<u32>().ok());
            }
        }
        if !matches.is_null() {
            gdal_sys::OSRFreeSRSArray(matches);
        }
        if !conf.is_null() {
            gdal_sys::VSIFree(conf.cast());
        }
        found
    }
}

pub(crate) fn field_type(ty: OGRFieldType::Type) -> FieldType {
    match ty {
        OGRFieldType::OFTInteger | OGRFieldType::OFTInteger64 => FieldType::Int,
        OGRFieldType::OFTReal => FieldType::Real,
        _ => FieldType::Text,
    }
}

/// A repaired geometry keeps to the kind it was: a repaired area that GDAL returns as a
/// collection with stray lines or points keeps only its areas, and so on.
fn keep_family(fixed: geo::Geometry<f64>, was: &geo::Geometry<f64>) -> geo::Geometry<f64> {
    let family = geom::family(was);
    match fixed {
        geo::Geometry::GeometryCollection(c) => {
            let parts: Vec<geo::Geometry<f64>> = c.0.into_iter().filter(|p| geom::family(p) == family).collect();
            match family {
                Family::Area => {
                    let polys: Vec<geo::Polygon<f64>> = parts
                        .into_iter()
                        .flat_map(|p| match p {
                            geo::Geometry::Polygon(x) => vec![x],
                            geo::Geometry::MultiPolygon(m) => m.0,
                            _ => vec![],
                        })
                        .collect();
                    if polys.len() == 1 {
                        geo::Geometry::Polygon(polys.into_iter().next().unwrap())
                    } else {
                        geo::Geometry::MultiPolygon(geo::MultiPolygon::new(polys))
                    }
                }
                _ if parts.len() == 1 => parts.into_iter().next().unwrap(),
                _ => geo::Geometry::GeometryCollection(geo::GeometryCollection::new_from(parts)),
            }
        }
        other => other,
    }
}

pub(crate) struct GeomStats {
    pub z: usize,
    pub m: usize,
    pub curves: usize,
    pub failed: usize,
}

/// The heights of an OGR geometry with Z, coordinate by coordinate in the order of
/// `og_engine::geom::coords`. None for a flat one, or one with curves, which are made of other
/// points when they are straightened.
pub(crate) fn ogr_heights(g: &OgrGeometry) -> Option<Vec<f64>> {
    if unsafe { g.c_geometry() }.is_null() || g.is_empty() || !geometry_type_has_z(g.geometry_type()) {
        return None;
    }
    if unsafe { gdal_sys::OGR_G_HasCurveGeometry(g.c_geometry(), 0) != 0 } {
        return None;
    }
    fn walk(g: &OgrGeometry, out: &mut Vec<f64>) {
        let parts = g.geometry_count();
        if parts > 0 {
            for i in 0..parts {
                walk(&g.get_geometry(i), out);
            }
        } else {
            for i in 0..g.point_count() {
                out.push(g.get_point(i as i32).2);
            }
        }
    }
    let mut out = Vec::new();
    walk(g, &mut out);
    Some(out)
}

/// Turns an OGR geometry into 2D and straight lines, and then into `geo`.
pub(crate) fn to_geo(g: &OgrGeometry, stats: &mut GeomStats) -> Option<geo::Geometry<f64>> {
    // A feature without geometry can come as a geometry that points at nothing.
    if unsafe { g.c_geometry() }.is_null() || g.is_empty() {
        return None;
    }
    let mut owned = g.clone();
    let ty = owned.geometry_type();
    if geometry_type_has_m(ty) {
        stats.m += 1;
        unsafe { gdal_sys::OGR_G_SetMeasured(owned.c_geometry(), 0) };
    }
    if geometry_type_has_z(ty) {
        stats.z += 1;
        owned.flatten_to_2d();
    }
    let curved = unsafe { gdal_sys::OGR_G_HasCurveGeometry(owned.c_geometry(), 0) != 0 };
    if curved {
        stats.curves += 1;
        let wkb = unsafe {
            let lin = gdal_sys::OGR_G_GetLinearGeometry(owned.c_geometry(), 0.0, std::ptr::null_mut());
            if lin.is_null() {
                stats.failed += 1;
                return None;
            }
            let size = gdal_sys::OGR_G_WkbSize(lin);
            let mut buf = vec![0u8; size.max(0) as usize];
            let rc = gdal_sys::OGR_G_ExportToWkb(lin, gdal_sys::OGRwkbByteOrder::wkbNDR, buf.as_mut_ptr());
            gdal_sys::OGR_G_DestroyGeometry(lin);
            if rc != gdal_sys::OGRErr::OGRERR_NONE {
                stats.failed += 1;
                return None;
            }
            buf
        };
        owned = match OgrGeometry::from_wkb(&wkb) {
            Ok(g) => g,
            Err(_) => {
                stats.failed += 1;
                return None;
            }
        };
    }
    match owned.to_geo() {
        Ok(g) => Some(geom::normalize(g)),
        Err(_) => {
            stats.failed += 1;
            None
        }
    }
}

/// The name GDAL opens the file by. A zip file is opened without unpacking: the folder in
/// it that holds the Shapefiles, or else the first data file in it.
pub(crate) fn gdal_path(path: &Path) -> PathBuf {
    if ext_of(path) != "zip" || !path.is_file() {
        return path.to_path_buf();
    }
    let root = format!("/vsizip/{}", path.to_string_lossy().replace('\\', "/"));
    let entries = gdal::vsi::read_dir(&root, true).unwrap_or_default();
    let inside = |e: &Path| format!("{}/{}", root, e.to_string_lossy().replace('\\', "/"));
    if let Some(shp) = entries.iter().find(|e| ext_of(e) == "shp") {
        // All the Shapefiles in that folder become layers.
        return match shp.parent().filter(|d| !d.as_os_str().is_empty()) {
            Some(dir) => PathBuf::from(inside(dir)),
            None => PathBuf::from(root),
        };
    }
    const DATA: [&str; 9] = ["gpkg", "geojson", "json", "gml", "kml", "tab", "xlsx", "ods", "csv"];
    match entries.iter().find(|e| DATA.contains(&ext_of(e).as_str())) {
        Some(file) => PathBuf::from(inside(file)),
        None => PathBuf::from(root),
    }
}

/// The file in a zip file to read: the one asked for by name, path or pattern. Without one,
/// as gdal_path chooses, with a note when the zip holds more data files to choose from.
pub(crate) fn zip_member(path: &Path, asked: Option<&str>, notes: &mut Notes) -> Result<Option<PathBuf>> {
    if ext_of(path) != "zip" || !path.is_file() {
        return Ok(None);
    }
    let root = format!("/vsizip/{}", path.to_string_lossy().replace('\\', "/"));
    let entries: Vec<String> = gdal::vsi::read_dir(&root, true).unwrap_or_default().iter().map(|e| e.to_string_lossy().replace('\\', "/")).collect();
    const DATA: [&str; 15] = ["shp", "gpkg", "geojson", "json", "gml", "kml", "tab", "xlsx", "ods", "csv", "fgb", "sqlite", "tif", "parquet", "dxf"];
    let data: Vec<&String> = entries.iter().filter(|e| DATA.contains(&ext_of(Path::new(e)).as_str())).collect();
    let name_of = |e: &str| e.rsplit('/').next().unwrap_or(e).to_string();
    match asked {
        Some(want) => {
            let want = want.replace('\\', "/");
            let found = entries
                .iter()
                .find(|e| **e == want)
                .or_else(|| entries.iter().find(|e| og_engine::nodes::wildcard_match(&want, e) || og_engine::nodes::wildcard_match(&want, &name_of(e))));
            match found {
                Some(e) if ext_of(Path::new(e)) == "shp" && want.contains('*') => {
                    // All the Shapefiles of that folder, as layers.
                    let dir = e.rsplit_once('/').map(|d| d.0).unwrap_or("");
                    Ok(Some(PathBuf::from(if dir.is_empty() { root } else { format!("{}/{}", root, dir) })))
                }
                Some(e) => Ok(Some(PathBuf::from(format!("{}/{}", root, e)))),
                None => Err(GbError(format!(
                    "{} holds no file {}. It holds: {}.",
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    want,
                    if data.is_empty() { entries.join(", ") } else { data.iter().map(|d| d.as_str()).collect::<Vec<_>>().join(", ") }
                ))),
            }
        }
        None => {
            let shps = data.iter().filter(|d| d.ends_with(".shp")).count();
            if data.len() > shps.max(1) {
                let read = gdal_path(path).to_string_lossy().trim_start_matches(&root).trim_start_matches('/').to_string();
                let read = if read.is_empty() || !data.iter().any(|d| **d == read) { "the Shapefiles".to_string() } else { read };
                notes.info(format!(
                    "{} holds {} data files: {}. Read {}; choose another with File in the zip",
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    data.len(),
                    data.iter().map(|d| d.as_str()).collect::<Vec<_>>().join(", "),
                    read
                ));
            }
            Ok(None)
        }
    }
}

pub(crate) fn open(path: &Path, update: bool, open_options: &[String]) -> Result<Dataset> {
    let shown = path;
    let path = &gdal_path(path);
    let refs: Vec<&str> = open_options.iter().map(|s| s.as_str()).collect();
    let flags = GdalOpenFlags::GDAL_OF_VECTOR | if update { GdalOpenFlags::GDAL_OF_UPDATE } else { GdalOpenFlags::GDAL_OF_READONLY };
    Dataset::open_ex(path, DatasetOptions { open_flags: flags, open_options: if refs.is_empty() { None } else { Some(&refs) }, ..Default::default() })
        .map_err(|e| GbError(format!("Cannot open {} as vector data: {}", shown.display(), e)))
}

/// Reads the features of a layer, or of the result of a query, into a table.
pub(crate) fn read_layer<L: LayerAccess>(layer: &mut L, opts: &ReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let has_geom_column = layer.defn().geom_fields().count() > 0;
    let mut crs = None;
    if has_geom_column {
        // GeoPackage marks layers without a coordinate system as "Undefined …". That counts as not stated.
        let declared =
            layer.spatial_ref().filter(|s| !s.is_local() && !s.name().is_some_and(|n| n.starts_with("Undefined") || n.eq_ignore_ascii_case("unknown")));
        crs = match declared {
            Some(srs) => match epsg_of(&srs) {
                Some(c) => Some(c),
                None => match opts.crs {
                    Some(c) => {
                        notes.warn(format!(
                            "The coordinate system of the file ({}) could not be recognised as an EPSG code. The input's setting EPSG:{} is used.",
                            srs.name().unwrap_or_else(|| "unnamed".into()),
                            c
                        ));
                        Some(c)
                    }
                    None => {
                        return Err(GbError(format!(
                            "The coordinate system of the file ({}) could not be recognised as an EPSG code. Set crs in the input.",
                            srs.name().unwrap_or_else(|| "unnamed".into())
                        )))
                    }
                },
            },
            None => match opts.crs {
                Some(c) => Some(c),
                None if opts.allow_no_crs => None,
                None => return Err(GbError("The file states no coordinate system. Set crs in the input, for example EPSG:3006.".into())),
            },
        };
    }
    // Only what reaches into a rectangle: GDAL leaves the rest unread, through the spatial
    // index where the file has one.
    if let (Some((rect, rect_crs)), true) = (opts.within, has_geom_column) {
        let r = match (rect_crs, crs) {
            (Some(from), Some(to)) if from != to => {
                // The corners and the middles of the sides, so that a turned rectangle is covered.
                let (x0, y0, x1, y1) = (rect[0], rect[1], rect[2], rect[3]);
                let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let mut xs = vec![x0, x1, x1, x0, mx, mx, x0, x1];
                let mut ys = vec![y0, y0, y1, y1, y0, y1, my, my];
                let (s, t) = (srs_from_epsg(from)?, srs_from_epsg(to)?);
                let ct = CoordTransform::new(&s, &t).map_err(|e| gerr(format!("Cannot reproject from EPSG:{} to EPSG:{}", from, to), e))?;
                ct.transform_coords(&mut xs, &mut ys, &mut [])
                    .map_err(|_| GbError("The rectangle cannot be reprojected to the coordinate system of the data.".into()))?;
                let fold = |v: &[f64], f: fn(f64, f64) -> f64, s: f64| v.iter().copied().fold(s, f);
                [
                    fold(&xs, f64::min, f64::INFINITY),
                    fold(&ys, f64::min, f64::INFINITY),
                    fold(&xs, f64::max, f64::NEG_INFINITY),
                    fold(&ys, f64::max, f64::NEG_INFINITY),
                ]
            }
            _ => rect,
        };
        layer.set_spatial_filter_rect(r[0], r[1], r[2], r[3]);
    }
    let defs: Vec<(String, OGRFieldType::Type)> = layer.defn().fields().map(|f| (f.name(), f.field_type())).collect();
    let mut fields: Vec<Field> = defs.iter().map(|(n, t)| Field::new(n.clone(), field_type(*t))).collect();
    // A key column of its own name, as when an attribute was made the primary key, is read
    // as the first attribute. The usual fid and ogc_fid are only the numbers of the rows.
    // SAFETY: the layer is valid, and GDAL owns the name it hands back.
    let key_column = unsafe {
        let p = gdal_sys::OGR_L_GetFIDColumn(layer.c_layer());
        if p.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    };
    let own_key = !key_column.is_empty()
        && !["fid", "ogc_fid", "objectid", "rowid"].iter().any(|k| key_column.eq_ignore_ascii_case(k))
        && !defs.iter().any(|(n, _)| n.eq_ignore_ascii_case(&key_column));
    if own_key {
        fields.insert(0, Field::new(key_column.clone(), FieldType::Int));
    }
    let mut stats = GeomStats { z: 0, m: 0, curves: 0, failed: 0 };
    // The number of features, when the format knows it without reading them all.
    let total = layer.try_feature_count().map(|t| opts.limit.map_or(t, |l| t.min(l as u64)));
    let mut features = Vec::with_capacity(total.unwrap_or(0).min(50_000_000) as usize);
    for f in layer.features() {
        if opts.limit.is_some_and(|l| features.len() >= l) {
            break;
        }
        if features.len() % 1_000 == 0 && !progress(features.len() as u64, total) {
            drain_messages(notes);
            return Err(GbError(og_engine::model::STOPPED.into()));
        }
        let mut attrs = Vec::with_capacity(defs.len() + 1);
        if own_key {
            attrs.push(f.fid().map_or(Value::Null, |v| Value::Int(v as i64)));
        }
        let shift = own_key as usize;
        for (i, (_, _)) in defs.iter().enumerate() {
            // Some formats, Excel among them, leave an empty value unset instead of null.
            // The binding only looks for null, and would hand back 0 or an empty text.
            let set = unsafe { gdal_sys::OGR_F_IsFieldSetAndNotNull(f.c_feature(), i as std::ffi::c_int) } != 0;
            let v = match if set { f.field(i).map_err(|e| gerr("Cannot read an attribute value", e))? } else { None } {
                None => Value::Null,
                Some(FieldValue::IntegerValue(v)) => Value::Int(v as i64),
                Some(FieldValue::Integer64Value(v)) => Value::Int(v),
                Some(FieldValue::RealValue(v)) => {
                    if v.is_finite() {
                        Value::Real(v)
                    } else {
                        Value::Null
                    }
                }
                Some(FieldValue::StringValue(s)) => Value::Text(s),
                Some(FieldValue::DateValue(d)) => Value::Text(d.to_string()),
                Some(FieldValue::DateTimeValue(d)) => Value::Text(d.format("%Y-%m-%d %H:%M:%S").to_string()),
                Some(_) => f.field_as_string(i).ok().flatten().map(Value::Text).unwrap_or(Value::Null),
            };
            attrs.push(coerce(v, fields[i + shift].ty));
        }
        let (geom, z) = match f.geometry().filter(|_| has_geom_column) {
            Some(g) => {
                let geom = to_geo(g, &mut stats);
                // The heights are kept when there is one for every coordinate.
                let z = ogr_heights(g).filter(|z| geom.as_ref().is_some_and(|x| og_engine::geom::coord_count(x) == z.len()));
                (geom, z.map(std::sync::Arc::from))
            }
            None => (None, None),
        };
        features.push(Feature { z, attrs, geom });
    }
    if stats.z > 0 {
        notes.info(format!("{} features with heights (Z)", og_engine::model::fmt_count(stats.z as u64)));
    }
    if stats.m > 0 {
        notes.warn(format!("{} features have measures (M). They are dropped.", stats.m));
    }
    if stats.curves > 0 {
        notes.info(format!("{} curved features were turned into straight segments", stats.curves));
    }
    if stats.failed > 0 {
        notes.warn(format!("{} features have a geometry type that is not supported and were read without geometry.", stats.failed));
    }
    progress(features.len() as u64, total);
    drain_messages(notes);
    Ok(Table { mesh: None, fields, crs, features, raster: false, image: None, grid: None })
}

fn srs_from_epsg(code: u32) -> Result<SpatialRef> {
    let mut s = SpatialRef::from_epsg(code).map_err(|e| GbError(format!("Unknown coordinate system EPSG:{} ({})", code, e)))?;
    s.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
    Ok(s)
}

impl Backend for GdalBackend {
    fn describe(&self) -> String {
        format!("GDAL {}", gdal::version::VersionInfo::release_name())
    }

    fn read(&self, path: &Path, opts: &ReadOpts, notes: &mut Notes) -> Result<Table> {
        self.read_with(path, opts, notes, &|_, _| true)
    }

    fn read_with(&self, path: &Path, opts: &ReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
        if services::service(path).is_some() {
            return services::read(path, opts, notes, progress);
        }
        clear_messages();
        let member = zip_member(path, opts.zip_member.as_deref(), notes)?;
        let path = member.as_deref().unwrap_or(path);
        let ext = ext_of(path);
        if is_raster(&ext) {
            let t = raster::read(path, opts, notes);
            drain_messages(notes);
            return t;
        }
        if ext == "dxf" {
            let t = dxf::read(path, opts, notes, progress);
            drain_messages(notes);
            return t;
        }
        let mut open_opts = opts.open_options.clone();
        // A folder of vector tiles is opened by the driver of the tiles.
        let tile_folder;
        let path = if path.is_dir() && path.join("metadata.json").is_file() {
            // GDAL opens one zoom level of a folder of tiles: the one asked for, or the
            // most detailed there is.
            let asked = open_opts.iter().find_map(|o| o.strip_prefix("ZOOM_LEVEL=").map(|z| z.to_string()));
            open_opts.retain(|o| !o.starts_with("ZOOM_LEVEL="));
            let levels: Vec<u32> =
                std::fs::read_dir(path).map(|d| d.flatten().filter_map(|e| e.file_name().to_string_lossy().parse().ok()).collect()).unwrap_or_default();
            let level = match asked {
                Some(z) => {
                    let z: u32 = z.parse().unwrap_or(0);
                    if !levels.contains(&z) {
                        let mut have = levels.clone();
                        have.sort();
                        return Err(GbError(format!(
                            "The folder of tiles has no zoom level {}. It has {}.",
                            z,
                            have.iter().map(|z| z.to_string()).collect::<Vec<_>>().join(", ")
                        )));
                    }
                    z
                }
                None => levels.iter().copied().max().unwrap_or(0),
            };
            tile_folder = path.join(level.to_string());
            tile_folder.as_path()
        } else {
            path
        };
        if let Some(enc) = opts.encoding.as_deref().filter(|e| !e.trim().is_empty()) {
            if ext == "shp" || ext == "dbf" {
                open_opts.push(format!("ENCODING={}", gdal_encoding(enc)));
            }
        }
        // Without this GDAL guesses, and guesses wrong for a sheet where every column is text.
        let _headers = if is_spreadsheet(&ext) { opts.header.map(Headers::set) } else { None };
        let ds = open(path, false, &open_opts)?;
        let count = ds.layer_count();
        if count == 0 {
            return Err(GbError(format!("{} contains no vector layers.", path.display())));
        }
        // A query instead of a layer, such as SQL against a database.
        if let Some(sql) = opts.sql.as_deref().filter(|q| !q.trim().is_empty()) {
            let mut rs = ds
                .execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT)
                .map_err(|e| gerr("The query failed", e))?
                .ok_or_else(|| GbError("The query gave no rows to read.".into()))?;
            let t = read_layer(&mut *rs, opts, notes, progress);
            drain_messages(notes);
            return t;
        }
        // OpenStreetMap is read from start to end, and the features of every layer come in on
        // the way. Unless GDAL is told which layer is wanted, it keeps those of the others
        // until they are asked for, which fills the memory and then stops with "Too many
        // features have accumulated". Only the one layer is read, and the rest let go.
        if ds.driver().short_name() == "OSM" {
            let wanted = match opts.layer.as_deref().filter(|l| !l.trim().is_empty()) {
                Some(name) => name.to_string(),
                None => ds.layer(0).map(|l| l.name()).unwrap_or_default(),
            };
            if ds.layers().any(|l| l.name() == wanted) {
                let _ = ds.execute_sql(format!("SET interest_layers = {}", wanted), None, gdal::vector::sql::Dialect::DEFAULT);
                clear_messages();
            }
        }
        let mut layer = match opts.layer.as_deref().filter(|l| !l.trim().is_empty()) {
            Some(name) => ds.layer_by_name(name).map_err(|_| {
                let names: Vec<String> = ds.layers().map(|l| l.name()).collect();
                GbError(format!("There is no layer \"{}\" in the file. Available: {}.", name, names.join(", ")))
            })?,
            None => {
                let l = ds.layer(0).map_err(|e| gerr("Cannot read the first layer", e))?;
                if count > 1 {
                    notes.info(format!("layer {}, one of {} in the file", l.name(), count));
                }
                l
            }
        };
        read_layer(&mut layer, opts, notes, progress)
    }

    fn run_sql(
        &self,
        path: &Path,
        statements: &[String],
        opts: &ReadOpts,
        notes: &mut Notes,
        going: &dyn Fn(u64, Option<u64>) -> bool,
    ) -> Result<Vec<std::result::Result<Option<Table>, String>>> {
        clear_messages();
        // One connection for all, opened for writing, since a statement can change things.
        let ds = open(path, true, &[])?;
        let mut out = Vec::with_capacity(statements.len());
        let total = statements.len() as u64;
        for (i, sql) in statements.iter().enumerate() {
            if !going(i as u64, Some(total)) {
                return Err(GbError(og_engine::model::STOPPED.into()));
            }
            clear_messages();
            let answer = match ds.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT) {
                Ok(Some(mut rs)) => {
                    let mut quiet = Notes::default();
                    read_layer(&mut *rs, opts, &mut quiet, &|_, _| true).map(Some).map_err(|e| e.0)
                }
                Ok(None) => {
                    // GDAL tells of a failed statement that returns no rows only in its messages.
                    match last_gdal_error() {
                        Some(e) => Err(e),
                        None => Ok(None),
                    }
                }
                Err(e) => Err(gerr("The database answered", e).0),
            };
            out.push(answer);
        }
        let _ = notes;
        clear_messages();
        Ok(out)
    }

    fn write(&self, table: &Table, path: &Path, opts: &WriteOpts, notes: &mut Notes) -> Result<Vec<PathBuf>> {
        clear_messages();
        let ext = ext_of(path);
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "layer".into());
        let files = match ext.as_str() {
            // Values on a grid with no picture, such as from Raster calculator: a GeoTIFF of
            // decimal numbers.
            "tif" | "tiff" if table.image.is_none() && table.grid.is_some() => write_grid(table.grid.as_deref().expect("checked"), path, notes)?,
            "png" | "jpg" | "jpeg" | "tif" | "tiff" | "pdf" => {
                let Some(image) = table.image.as_ref() else {
                    return Err(GbError(format!("There is no image in the stream to write as .{}.", ext)));
                };
                write_image(image, table.crs, path, &ext, &opts.pdf, notes)?
            }
            "gpkg" => {
                let layer = opts.layer.clone().filter(|l| !l.trim().is_empty()).unwrap_or(stem);
                write_gpkg(table, path, &layer, &opts.sizes, opts.append, opts.key.as_ref(), notes)?;
                vec![path.to_path_buf()]
            }
            "shp" => write_shp(table, path, &stem, opts.encoding.as_deref().unwrap_or("utf-8"), &opts.sizes, opts.append, notes)?,
            "fgb" => {
                write_fgb(table, path, &opts.layer.clone().filter(|l| !l.trim().is_empty()).unwrap_or(stem), &opts.sizes, opts.append, notes)?;
                vec![path.to_path_buf()]
            }
            "geojson" | "json" => {
                write_geojson(table, path, &opts.layer.clone().filter(|l| !l.trim().is_empty()).unwrap_or(stem), &opts.sizes, opts.append, notes)?;
                vec![path.to_path_buf()]
            }
            "xlsx" | "ods" => {
                let wanted = opts.layer.clone().filter(|l| !l.trim().is_empty()).unwrap_or(stem);
                let sheet = sheet_name(&wanted);
                if sheet != wanted.trim() {
                    notes.warn(format!("A sheet cannot be called \"{}\". It was named \"{}\".", wanted.trim(), sheet));
                }
                if table.has_geometry() {
                    return Err(GbError("A spreadsheet cannot hold geometry.".into()));
                }
                if ext == "ods" {
                    sheets::write_ods(table, path, &sheet, &opts.sizes, &opts.sheet, opts.append, notes)?;
                } else {
                    sheets::write_xlsx(table, path, &sheet, &opts.sizes, &opts.sheet, opts.append, notes)?;
                }
                vec![path.to_path_buf()]
            }
            other => return Err(GbError(format!("The output cannot write .{}. Use .gpkg, .shp, .fgb, .geojson, .xlsx, .ods, .csv or .txt.", other))),
        };
        drain_messages(notes);
        Ok(files)
    }

    fn http_get(&self, url: &str, headers: &[(String, String)], timeout_s: u64) -> Result<HttpAnswer> {
        http_get(url, headers, timeout_s)
    }

    fn fetch_bytes(&self, url: &str, headers: &[(String, String)]) -> Result<Vec<u8>> {
        let (status, bytes, _) = http_bytes("GET", url, headers, None, 120)?;
        if status >= 400 {
            return Err(GbError(format!("The server answered {} for {}.", status, url)));
        }
        Ok(bytes)
    }

    fn fetch_range(&self, url: &str, headers: &[(String, String)], start: u64, len: u64) -> Result<(Vec<u8>, Option<u64>)> {
        let mut h = headers.to_vec();
        h.push(("Range".into(), format!("bytes={}-{}", start, start + len.max(1) - 1)));
        let (status, bytes, answer) = http_bytes("GET", url, &h, None, 120)?;
        match status {
            // bytes 0-524287/12345678
            206 => {
                let total = answer
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-range"))
                    .and_then(|(_, v)| v.rsplit('/').next())
                    .and_then(|t| t.trim().parse::<u64>().ok());
                Ok((bytes, total))
            }
            // A server that does not give parts sends the whole.
            200 => {
                let total = bytes.len() as u64;
                let from = (start as usize).min(bytes.len());
                let to = ((start + len) as usize).min(bytes.len());
                Ok((bytes[from..to].to_vec(), Some(total)))
            }
            416 => Ok((Vec::new(), None)),
            _ => Err(GbError(format!("The server answered {} for {}.", status, url))),
        }
    }

    fn http_send(&self, method: &str, url: &str, headers: &[(String, String)], body: Option<&str>, timeout_s: u64) -> Result<HttpAnswer> {
        http_send(method, url, headers, body, timeout_s)
    }

    fn write_layered_pdf(
        &self,
        layers: &[og_engine::io::PdfLayer],
        crs: u32,
        page: &og_engine::io::PdfPage,
        margin: f64,
        path: &Path,
        notes: &mut Notes,
    ) -> Result<Vec<PathBuf>> {
        clear_messages();
        let r = pdf::write_layered(layers, crs, page, margin, path, notes);
        drain_messages(notes);
        r
    }

    fn make_valid(&self, geoms: &[geo::Geometry<f64>]) -> Result<Vec<Option<geo::Geometry<f64>>>> {
        use gdal::vector::ToGdal;
        clear_messages();
        let mut stats = GeomStats { z: 0, m: 0, curves: 0, failed: 0 };
        let opts = gdal::cpl::CslStringList::new();
        let out = geoms
            .iter()
            .map(|g| {
                let og = g.to_gdal().ok()?;
                let fixed = og.make_valid(&opts).ok()?;
                to_geo(&fixed, &mut stats).map(|x| keep_family(x, g))
            })
            .collect();
        clear_messages();
        Ok(out)
    }

    fn contours(&self, grid: &og_engine::model::Grid, interval: f64, base: f64, areas: bool, name: &str) -> Result<Table> {
        clear_messages();
        let t = raster::contours(grid, interval, base, areas, name);
        clear_messages();
        let mut t = t?;
        let mut stats = GeomStats { z: 0, m: 0, curves: 0, failed: 0 };
        let _ = &mut stats;
        t.crs = grid.crs;
        Ok(t)
    }

    fn polygonize(&self, grid: &og_engine::model::Grid, name: &str, eight: bool) -> Result<Table> {
        clear_messages();
        let t = raster::polygonize(grid, name, eight);
        clear_messages();
        t
    }

    fn crs_wkt(&self, epsg: u32) -> Option<String> {
        SpatialRef::from_epsg(epsg).ok()?.to_wkt().ok()
    }

    fn warp_grid(&self, grid: &og_engine::model::Grid, to: u32) -> Result<og_engine::model::Grid> {
        raster::warp_grid(grid, to)
    }

    fn warp_image(&self, image: &og_engine::model::RasterImage, from: u32, to: u32) -> Result<og_engine::model::RasterImage> {
        raster::warp_image(image, from, to)
    }

    fn transform(&self, from: u32, to: u32, xs: &mut [f64], ys: &mut [f64]) -> Result<()> {
        if from == to || xs.is_empty() {
            return Ok(());
        }
        let (s, t) = (srs_from_epsg(from)?, srs_from_epsg(to)?);
        let ct = CoordTransform::new(&s, &t).map_err(|e| gerr(format!("Cannot reproject from EPSG:{} to EPSG:{}", from, to), e))?;
        let mut zs: Vec<f64> = Vec::new();
        ct.transform_coords(xs, ys, &mut zs).map_err(|_| {
            GbError(format!("Some coordinates cannot be reprojected from EPSG:{} to EPSG:{}. Is the coordinate system of the stream right?", from, to))
        })?;
        let bad = xs.iter().zip(ys.iter()).filter(|(x, y)| !x.is_finite() || !y.is_finite()).count();
        if bad > 0 {
            return Err(GbError(format!(
                "{} coordinates cannot be reprojected from EPSG:{} to EPSG:{}. Is the coordinate system of the stream right?",
                bad, from, to
            )));
        }
        Ok(())
    }

    fn crs_info(&self, epsg: u32) -> Option<CrsInfo> {
        let s = SpatialRef::from_epsg(epsg).ok()?;
        Some(CrsInfo { name: s.name().unwrap_or_else(|| format!("EPSG:{}", epsg)), geographic: s.is_geographic() })
    }

    fn fetch(&self, url: &str, keep: bool, notes: &mut Notes) -> Result<PathBuf> {
        fetch_file(url, &[], keep, notes)
    }

    fn versions(&self) -> Vec<(String, String)> {
        let (mut a, mut b, mut c) = (0, 0, 0);
        // SAFETY: GDAL writes three numbers.
        unsafe { gdal_sys::OSRGetPROJVersion(&mut a, &mut b, &mut c) };
        vec![
            ("GDAL".into(), gdal::version::VersionInfo::release_name()),
            ("PROJ".into(), format!("{}.{}.{}", a, b, c)),
            ("GEOS".into(), if gdal::version::VersionInfo::has_geos() { "through GDAL".into() } else { "not in this GDAL".into() }),
        ]
    }

    fn crs_list(&self) -> Vec<og_engine::io::CrsEntry> {
        crs_list().to_vec()
    }

    fn fetch_with(&self, url: &str, headers: &[(String, String)], keep: bool, notes: &mut Notes) -> Result<PathBuf> {
        fetch_file(url, headers, keep, notes)
    }

    fn write_postgis(&self, table: &Table, conn: &str, opts: &og_engine::io::DbWrite, notes: &mut Notes) -> Result<u64> {
        clear_messages();
        let r = db::write(table, conn, opts, notes);
        clear_messages();
        r
    }

    fn layers(&self, path: &Path) -> Result<Vec<LayerInfo>> {
        if services::service(path).is_some() {
            return services::layers(path, &[]);
        }
        clear_messages();
        if is_raster(&ext_of(path)) {
            let l = raster::layers(path);
            clear_messages();
            return l;
        }
        // The same reading of the first row as the reader's own default.
        let _headers = if is_spreadsheet(&ext_of(path)) { Some(Headers::set(true)) } else { None };
        // A folder of vector tiles: the layers of its most detailed zoom level.
        let deepest;
        let path = if path.is_dir() && path.join("metadata.json").is_file() {
            let levels: Vec<u32> =
                std::fs::read_dir(path).map(|d| d.flatten().filter_map(|e| e.file_name().to_string_lossy().parse().ok()).collect()).unwrap_or_default();
            deepest = path.join(levels.iter().max().copied().unwrap_or(0).to_string());
            deepest.as_path()
        } else {
            path
        };
        let ds = open(path, false, &[])?;
        let mut out = Vec::new();
        for layer in ds.layers() {
            let has_geom = layer.defn().geom_fields().count() > 0;
            let srs = if has_geom { layer.spatial_ref().filter(|s| !s.is_local() && !s.name().is_some_and(|n| n.starts_with("Undefined"))) } else { None };
            out.push(LayerInfo {
                name: layer.name(),
                count: layer.try_feature_count(),
                crs: srs.as_ref().and_then(epsg_of),
                crs_name: srs.as_ref().and_then(|s| s.name()),
                geometry: if has_geom { geometry_type_to_name(layer.defn().geometry_type()) } else { "no geometry".into() },
                fields: layer.defn().fields().map(|f| Field::new(f.name(), field_type(f.field_type()))).collect(),
            });
        }
        clear_messages();
        Ok(out)
    }
}

// ---------- Files from the web ----------

/// Where fetched files are kept: a folder per address in the temporary folder of the system.
pub fn download_dir(url: &str) -> PathBuf {
    // FNV-1a. The folder name only has to be the same for the same address.
    let hash = url.trim().bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    std::env::temp_dir().join("ographis-downloads").join(format!("{:016x}", hash))
}

/// The file name to give a fetched file. The server may say it. Otherwise it is the last
/// part of the address, with an extension from the kind of content if it has none.
pub fn download_name(url: &str, disposition: Option<&str>, content_type: Option<&str>) -> String {
    let clean = |s: &str| -> String {
        let s: String = s.chars().map(|c| if c.is_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
        s.trim_matches(|c| c == '_' || c == '.').to_string()
    };
    let said = disposition.and_then(|d| d.split(';').find_map(|part| part.trim().strip_prefix("filename="))).map(|n| clean(n.trim_matches('"')));
    let address = url.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let last = clean(address.rsplit('/').next().filter(|_| address.matches('/').count() > 2).unwrap_or(""));
    let mut name = said.filter(|n| !n.is_empty()).unwrap_or(last);
    if name.is_empty() {
        name = "download".to_string();
    }
    let known = [
        "gpkg", "shp", "zip", "csv", "txt", "tsv", "xlsx", "xls", "ods", "geojson", "json", "gml", "kml", "tab", "sqlite", "db", "dxf", "gdb", "fgb", "parquet",
    ];
    if !known.contains(&ext_of(Path::new(&name)).as_str()) {
        let kind = content_type.unwrap_or("").split(';').next().unwrap_or("").trim().to_lowercase();
        let ext = match kind.as_str() {
            "text/csv" => Some("csv"),
            // Plain JSON is .json: the program looks inside to tell GeoJSON from other JSON.
            "application/json" | "text/json" => Some("json"),
            "application/geo+json" | "application/vnd.geo+json" => Some("geojson"),
            "application/geopackage+sqlite3" | "application/x-sqlite3" => Some("gpkg"),
            "application/zip" | "application/x-zip-compressed" => Some("zip"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
            "application/gml+xml" | "text/xml" | "application/xml" => Some("gml"),
            "application/vnd.google-earth.kml+xml" => Some("kml"),
            "text/plain" => Some("txt"),
            _ => None,
        };
        if let Some(ext) = ext {
            name = format!("{}.{}", name, ext);
        }
    }
    name
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1_048_576 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        b if b >= 1024 => format!("{:.0} kB", b as f64 / 1024.0),
        b => format!("{} bytes", b),
    }
}

/// Sends a GET request and gives back the status and the body as text. Answers with an
/// error status are answers too: the caller decides what they mean.
pub(crate) fn http_get(url: &str, headers: &[(String, String)], timeout_s: u64) -> Result<HttpAnswer> {
    http_send("GET", url, headers, None, timeout_s)
}

fn http_send(method: &str, url: &str, headers: &[(String, String)], body: Option<&str>, timeout_s: u64) -> Result<HttpAnswer> {
    let (status, bytes, headers) = http_bytes(method, url, headers, body, timeout_s)?;
    Ok(HttpAnswer { status, body: String::from_utf8_lossy(&bytes).into_owned(), headers })
}

/// An answer: the status, the body as bytes and the headers.
type Answer = (u16, Vec<u8>, Vec<(String, String)>);

/// Sends a request and gives back the status, the body as bytes and the headers.
fn http_bytes(method: &str, url: &str, headers: &[(String, String)], body: Option<&str>, timeout_s: u64) -> Result<Answer> {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let agent = AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
            .http_status_as_error(false)
            .user_agent(concat!("Ographis/", env!("CARGO_PKG_VERSION"), " (desktop application)"))
            .build()
            .into()
    });
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(GbError(format!("\"{}\" is not a web address. It should start with http:// or https://.", url)));
    }
    let timeout = Some(std::time::Duration::from_secs(timeout_s));
    let sent = match method {
        "POST" | "PUT" | "PATCH" => {
            let mut request = match method {
                "PUT" => agent.put(url),
                "PATCH" => agent.patch(url),
                _ => agent.post(url),
            }
            .config()
            .timeout_global(timeout)
            .build();
            for (k, v) in headers {
                request = request.header(k.as_str(), v.as_str());
            }
            request.send(body.unwrap_or("").as_bytes())
        }
        "DELETE" => {
            let mut request = agent.delete(url).config().timeout_global(timeout).build();
            for (k, v) in headers {
                request = request.header(k.as_str(), v.as_str());
            }
            request.call()
        }
        _ => {
            let mut request = agent.get(url).config().timeout_global(timeout).build();
            for (k, v) in headers {
                request = request.header(k.as_str(), v.as_str());
            }
            request.call()
        }
    };
    let mut response = sent.map_err(|e| match e {
        ureq::Error::Timeout(_) => GbError(format!("No answer from {} within {} seconds.", url, timeout_s)),
        other => GbError(format!("Cannot reach {}: {}", url, other)),
    })?;
    let status = response.status().as_u16();
    let headers: Vec<(String, String)> =
        response.headers().iter().filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string()))).collect();
    let bytes =
        response.body_mut().with_config().limit(50 * 1024 * 1024).read_to_vec().map_err(|e| GbError(format!("The answer from {} was cut off: {}", url, e)))?;
    Ok((status, bytes, headers))
}

fn fetch_file(url: &str, headers: &[(String, String)], keep: bool, notes: &mut Notes) -> Result<PathBuf> {
    let url = url.trim();
    let dir = download_dir(url);
    let kept = || std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.is_file() && ext_of(p) != "part");
    if keep {
        if let Some(file) = kept() {
            notes.info("read from the copy that was fetched earlier");
            return Ok(file);
        }
    }
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let agent = AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
            .timeout_connect(Some(std::time::Duration::from_secs(20)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
            .user_agent(concat!("Ographis/", env!("CARGO_PKG_VERSION"), " (desktop application)"))
            .build()
            .into()
    });
    let mut request = agent.get(url);
    for (k, v) in headers {
        request = request.header(k.as_str(), v.as_str());
    }
    let mut response = request.call().map_err(|e| match e {
        ureq::Error::StatusCode(code) => GbError(format!("The server answered {} for {}.", code, url)),
        other => GbError(format!("Cannot fetch {}: {}", url, other)),
    })?;
    let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(|v| v.to_string());
    let name = download_name(url, header("content-disposition").as_deref(), header("content-type").as_deref());
    let io = |what: &str, e: std::io::Error| GbError(format!("{} {}: {}", what, dir.display(), e));
    // What was fetched before goes, so that the folder holds one file.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| io("Cannot create the folder", e))?;
    let part = dir.join("download.part");
    let mut file = std::fs::File::create(&part).map_err(|e| io("Cannot write in", e))?;
    let bytes = std::io::copy(&mut response.body_mut().with_config().limit(u64::MAX).reader(), &mut file)
        .map_err(|e| GbError(format!("The fetching of {} was cut off: {}", url, e)))?;
    drop(file);
    let target = dir.join(&name);
    std::fs::rename(&part, &target).map_err(|e| io("Cannot write in", e))?;
    notes.info(format!("fetched {} ({})", name, size_text(bytes)));
    Ok(target)
}

// ---------- Writing ----------

fn promote(g: &geo::Geometry<f64>) -> geo::Geometry<f64> {
    match g {
        geo::Geometry::Point(p) => geo::Geometry::MultiPoint(geo::MultiPoint::new(vec![*p])),
        geo::Geometry::LineString(l) => geo::Geometry::MultiLineString(geo::MultiLineString::new(vec![l.clone()])),
        geo::Geometry::Polygon(p) => geo::Geometry::MultiPolygon(geo::MultiPolygon::new(vec![p.clone()])),
        other => other.clone(),
    }
}

/// The geometry type of the layer, and whether single geometries are to be promoted to multi.
fn layer_geometry(features: &[&Feature], shp: bool) -> (OGRwkbGeometryType::Type, bool) {
    let mut fams: Vec<Family> = Vec::new();
    let (mut any_multi, mut any_single) = (false, false);
    for g in features.iter().filter_map(|f| f.geom.as_ref()) {
        let fam = geom::family(g);
        if !fams.contains(&fam) {
            fams.push(fam);
        }
        if geom::is_multi(g) {
            any_multi = true
        } else {
            any_single = true
        }
    }
    if fams.is_empty() {
        return (OGRwkbGeometryType::wkbNone, false);
    }
    if fams.len() > 1 || fams[0] == Family::Other {
        return (OGRwkbGeometryType::wkbUnknown, false);
    }
    use OGRwkbGeometryType::*;
    match (fams[0], any_multi) {
        (Family::Point, false) => (wkbPoint, false),
        (Family::Point, true) => (wkbMultiPoint, any_single),
        // A Shapefile does not tell single from multi for lines and areas.
        (Family::Line, m) => {
            if shp || !m {
                (wkbLineString, false)
            } else {
                (wkbMultiLineString, any_single)
            }
        }
        (Family::Area, m) => {
            if shp || !m {
                (wkbPolygon, false)
            } else {
                (wkbMultiPolygon, any_single)
            }
        }
        (Family::Other, _) => (wkbUnknown, false),
    }
}

struct OutField {
    name: String,
    ty: OGRFieldType::Type,
    width: Option<i32>,
    precision: Option<i32>,
}

fn plan_fields(
    table: &Table,
    features: &[&Feature],
    names: Vec<String>,
    shp_bytes: Option<&dyn Fn(&str) -> usize>,
    sizes: &HashMap<String, FieldSize>,
    notes: &mut Notes,
) -> Vec<OutField> {
    let mut out = Vec::new();
    for (i, f) in table.fields.iter().enumerate() {
        // What the writer was told about this field, if anything.
        let size = sizes.get(&f.name).copied().unwrap_or_default();
        let asked = size.length.map(|l| l.clamp(1, 254) as i32);
        let mut precision = None;
        let (ty, width) = match f.ty {
            FieldType::Real => {
                precision = size.decimals.map(|d| d.min(15) as i32);
                // A dBASE number needs a width when it has a number of decimals.
                (OGRFieldType::OFTReal, asked.or(precision.filter(|_| shp_bytes.is_some()).map(|_| 24)))
            }
            FieldType::Int => {
                let big = features.iter().any(|x| matches!(x.attrs[i], Value::Int(v) if v > i32::MAX as i64 || v < i32::MIN as i64));
                (if big { OGRFieldType::OFTInteger64 } else { OGRFieldType::OFTInteger }, asked)
            }
            FieldType::Text if asked.is_some() => (OGRFieldType::OFTString, asked),
            FieldType::Text => {
                let width = shp_bytes.map(|bytes| {
                    let longest = features.iter().map(|x| if let Value::Text(s) = &x.attrs[i] { bytes(s) } else { 0 }).max().unwrap_or(0);
                    if longest > 254 {
                        notes.warn(format!("The attribute {} has text longer than 254 characters, which is the Shapefile limit. The text is cut.", f.name));
                    }
                    longest.clamp(1, 254) as i32
                });
                (OGRFieldType::OFTString, width)
            }
        };
        out.push(OutField { name: names[i].clone(), ty, width, precision });
    }
    out
}

pub(crate) fn write_features(
    layer: &gdal::vector::Layer,
    table: &Table,
    features: &[&Feature],
    fields: &[OutField],
    to_multi: bool,
    fids: Option<&[i64]>,
) -> Result<()> {
    let targets: Vec<usize> = (0..fields.len()).collect();
    write_features_to(layer, table, features, fields, &targets, to_multi, fids)
}

/// Writes the features into a layer, with attribute `i` of the table in field `targets[i]`
/// of the layer.
/// With `fids`, each feature gets its number as its primary key.
fn write_features_to(
    layer: &gdal::vector::Layer,
    table: &Table,
    features: &[&Feature],
    fields: &[OutField],
    targets: &[usize],
    to_multi: bool,
    fids: Option<&[i64]>,
) -> Result<()> {
    let w = |e| gerr("Cannot write a feature", e);
    for (n, f) in features.iter().enumerate() {
        let mut feat = gdal::vector::Feature::new(layer.defn()).map_err(w)?;
        if let Some(id) = fids.and_then(|ids| ids.get(n)) {
            // SAFETY: the feature is valid while `feat` lives.
            unsafe { gdal_sys::OGR_F_SetFID(feat.c_feature(), *id) };
        }
        for (k, of) in fields.iter().enumerate() {
            let (i, at) = (k, targets[k]);
            match coerce(f.attrs[i].clone(), table.fields[i].ty) {
                Value::Null => feat.set_field_null(at).map_err(w)?,
                Value::Int(v) => {
                    if of.ty == OGRFieldType::OFTInteger64 {
                        feat.set_field_integer64(at, v).map_err(w)?
                    } else {
                        feat.set_field_integer(at, v as i32).map_err(w)?
                    }
                }
                Value::Bool(b) => feat.set_field_integer(at, b as i32).map_err(w)?,
                Value::Real(v) => {
                    if v.is_finite() {
                        feat.set_field_double(at, v).map_err(w)?
                    } else {
                        feat.set_field_null(at).map_err(w)?
                    }
                }
                Value::Text(s) => feat.set_field_string(at, &s).map_err(w)?,
            }
        }
        if let Some(g) = &f.geom {
            let shape = if to_multi { promote(g) } else { g.clone() };
            // With heights the geometry goes through WKT with Z, which keeps them.
            let og = match f.heights() {
                Some(z) => OgrGeometry::from_wkt(&og_engine::geom::wkt_z(&shape, z)).map_err(|e| gerr("Cannot convert a geometry with heights", e))?,
                None => shape.to_gdal().map_err(|e| gerr("Cannot convert a geometry", e))?,
            };
            feat.set_geometry(og).map_err(w)?;
        }
        feat.create(layer).map_err(w)?;
    }
    Ok(())
}

/// Writes a picture with its coordinates: GeoTIFF holds them itself, PNG and JPEG get a world
/// file next to them (.pgw, .jgw) and the coordinate system in an .aux.xml file. JPEG has no
/// transparency, so see-through parts are laid on white.
/// Text for a PDF content stream: brackets and backslashes escaped, and letters beyond
/// ASCII, such as å, ä and ö, as octal codes in the Windows encoding of the standard fonts.
pub(crate) fn pdf_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            c if (c as u32) >= 0xa0 && (c as u32) <= 0xff => out.push_str(&format!("\\{:03o}", c as u32)),
            '€' => out.push_str("\\200"),
            '–' => out.push_str("\\226"),
            '—' => out.push_str("\\227"),
            _ => out.push('?'),
        }
    }
    out
}

pub(crate) fn write_image(
    image: &og_engine::model::RasterImage,
    crs: Option<u32>,
    path: &Path,
    ext: &str,
    page: &og_engine::io::PdfPage,
    notes: &mut Notes,
) -> Result<Vec<PathBuf>> {
    use gdal::raster::{Buffer, ColorInterpretation};
    let pdf = ext == "pdf";
    // GDAL puts at least one pixel in each point of a PDF page (72 to the inch). A picture
    // printed larger than that is made larger first, each pixel a square of pixels, so that
    // it fills the page.
    let grown;
    let (image, grow) = match pdf.then(|| page.layout(image.width, image.height).scale).filter(|s| *s > 1.0) {
        Some(scale) => {
            let k = scale.ceil() as usize;
            let (w, h) = (image.width * k, image.height * k);
            let mut rgba = vec![0u8; w * h * 4];
            for y in 0..h {
                let src = &image.rgba[(y / k) * image.width * 4..][..image.width * 4];
                let row = &mut rgba[y * w * 4..][..w * 4];
                for x in 0..w {
                    row[x * 4..x * 4 + 4].copy_from_slice(&src[(x / k) * 4..(x / k) * 4 + 4]);
                }
            }
            let mut gt = image.geotransform;
            gt[1] /= k as f64;
            gt[2] /= k as f64;
            gt[4] /= k as f64;
            gt[5] /= k as f64;
            grown = og_engine::model::RasterImage { width: w, height: h, rgba, geotransform: gt };
            (&grown, k)
        }
        None => (image, 1),
    };
    let (w, h) = (image.width, image.height);
    // JPEG and PDF have the picture laid on white.
    let jpeg = matches!(ext, "jpg" | "jpeg") || pdf;
    let tiff = matches!(ext, "tif" | "tiff");
    let bands = if jpeg { 3 } else { 4 };
    let mem = DriverManager::get_driver_by_name("MEM").map_err(|e| gerr("GDAL has no in-memory driver", e))?;
    let mut ds = mem.create_with_band_type::<u8, _>("", w, h, bands).map_err(|e| gerr("Cannot make the picture", e))?;
    // A picture without a coordinate system, such as a 3D view, is not placed on the map.
    if crs.is_some() {
        ds.set_geo_transform(&image.geotransform).map_err(|e| gerr("Cannot place the picture", e))?;
    }
    if let Some(c) = crs {
        let srs = SpatialRef::from_epsg(c).map_err(|e| gerr(format!("EPSG:{} is not known", c), e))?;
        ds.set_spatial_ref(&srs).map_err(|e| gerr("Cannot give the picture its coordinate system", e))?;
    }
    let interp = |b: usize| match b {
        0 => ColorInterpretation::RedBand,
        1 => ColorInterpretation::GreenBand,
        2 => ColorInterpretation::BlueBand,
        _ => ColorInterpretation::AlphaBand,
    };
    for b in 0..bands {
        let data: Vec<u8> = image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| {
                if jpeg {
                    // Laid on white by its opacity.
                    let a = px[3] as u32;
                    ((px[b] as u32 * a + 255 * (255 - a) + 127) / 255) as u8
                } else {
                    px[b]
                }
            })
            .collect();
        let mut band = ds.rasterband(b + 1).map_err(|e| gerr("Cannot write the picture", e))?;
        band.set_color_interpretation(interp(b)).map_err(|e| gerr("Cannot write the picture", e))?;
        let mut buf = Buffer::new((w, h), data);
        band.write((0, 0), (w, h), &mut buf).map_err(|e| gerr("Cannot write the picture", e))?;
    }
    let pdf_options: Vec<String> = if pdf {
        let l = page.layout(w, h);
        // The resolution of the picture as it was drawn, before it was made larger.
        let ppi_drawn = 72.0 / (l.scale * grow as f64);
        let mut o = vec![
            format!("DPI={:.4}", 72.0 / l.scale),
            format!("LEFT_MARGIN={:.0}", l.left),
            format!("RIGHT_MARGIN={:.0}", l.right),
            format!("TOP_MARGIN={:.0}", l.top),
            format!("BOTTOM_MARGIN={:.0}", l.bottom),
            if page.compress == "jpeg" { "COMPRESS=JPEG".to_string() } else { "COMPRESS=DEFLATE".to_string() },
            format!("JPEG_QUALITY={}", page.quality.clamp(1, 100)),
            "GEO_ENCODING=ISO32000".to_string(),
            "CREATOR=Ographis".to_string(),
        ];
        // How sharp the picture comes out on paper.
        let ppi = ppi_drawn;
        let (wmm, hmm) = (w as f64 * l.scale / 72.0 * 25.4, h as f64 * l.scale / 72.0 * 25.4);
        if page.size != "fit" {
            if ppi < 150.0 {
                notes.warn(format!(
                    "The picture is printed {:.0} × {:.0} mm, at {:.0} pixels to the inch, which looks coarse on paper. Make it {} pixels wide in Draw map for 300.",
                    wmm,
                    hmm,
                    ppi,
                    (wmm / 25.4 * 300.0).round()
                ));
            } else {
                notes.info(format!("printed {:.0} × {:.0} mm, at {:.0} pixels to the inch", wmm, hmm, ppi));
            }
        }
        if let Some(title) = page.title.as_deref().filter(|t| !t.trim().is_empty()) {
            o.push(format!(
                "EXTRA_STREAM=BT /FHelveticaBold {:.1} Tf 1 0 0 1 {:.2} {:.2} Tm ({}) Tj ET",
                l.title_size,
                l.title_at.0,
                l.title_at.1,
                pdf_text(title.trim())
            ));
            o.push(format!("TITLE={}", title.trim()));
        }
        o
    } else {
        Vec::new()
    };
    let (driver, options): (&str, Vec<&str>) = if pdf {
        ("PDF", pdf_options.iter().map(|s| s.as_str()).collect())
    } else if tiff {
        ("GTiff", vec!["COMPRESS=DEFLATE", "PHOTOMETRIC=RGB", "ALPHA=YES", "TILED=YES"])
    } else if jpeg {
        ("JPEG", vec!["QUALITY=92"])
    } else {
        ("PNG", vec!["ZLEVEL=6"])
    };
    let target = DriverManager::get_driver_by_name(driver).map_err(|e| gerr(format!("GDAL has no {} support", driver), e))?;
    // A file that is there is written over, with what belongs to it.
    // PNG and JPEG get a world file of their own, named as GIS programs look for it, so
    // that map.png and map.jpg in one folder do not share one.
    let world = path.with_extension(if jpeg { "jgw" } else { "pgw" });
    let aux = PathBuf::from(format!("{}.aux.xml", path.display()));
    let mut stale = vec![path.to_path_buf(), aux.clone()];
    if !tiff && !pdf && crs.is_some() {
        stale.push(world.clone());
    }
    for old in stale {
        if old.exists() {
            std::fs::remove_file(&old).map_err(|e| GbError(format!("Cannot write over {}: {}. Is it open in another program?", old.display(), e)))?;
        }
    }
    let opts: gdal::raster::RasterCreationOptions = options.iter().map(|s| s.to_string()).collect();
    let out = ds.create_copy(&target, path, &opts).map_err(|e| gerr(format!("Cannot write {}", path.display()), e))?;
    out.close().map_err(|e| gerr("Cannot close the picture", e))?;
    drain_messages(notes);
    let mut files = vec![path.to_path_buf()];
    if !tiff && !pdf && crs.is_some() {
        // The world file says where the middle of the upper left pixel is.
        let g = &image.geotransform;
        let text = format!("{}\n{}\n{}\n{}\n{}\n{}\n", g[1], g[4], g[2], g[5], g[0] + g[1] / 2.0 + g[2] / 2.0, g[3] + g[4] / 2.0 + g[5] / 2.0);
        std::fs::write(&world, text).map_err(|e| GbError(format!("Cannot write {}: {}", world.display(), e)))?;
        files.push(world);
    }
    files.extend([aux].into_iter().filter(|f| f.exists()));
    Ok(files)
}

/// Values on a grid as a GeoTIFF of 32-bit decimal numbers, a band for each band, with
/// -9999 where there is no value.
pub(crate) fn write_grid(grid: &og_engine::model::Grid, path: &Path, notes: &mut Notes) -> Result<Vec<PathBuf>> {
    use gdal::raster::Buffer;
    const NODATA: f32 = -9999.0;
    let (w, h) = (grid.width, grid.height);
    let count = 1 + grid.bands.len();
    let mem = DriverManager::get_driver_by_name("MEM").map_err(|e| gerr("GDAL has no in-memory driver", e))?;
    let mut ds = mem.create_with_band_type::<f32, _>("", w, h, count).map_err(|e| gerr("Cannot make the raster", e))?;
    ds.set_geo_transform(&grid.geotransform).map_err(|e| gerr("Cannot place the raster", e))?;
    if let Some(c) = grid.crs {
        let srs = SpatialRef::from_epsg(c).map_err(|e| gerr(format!("EPSG:{} is not known", c), e))?;
        ds.set_spatial_ref(&srs).map_err(|e| gerr("Cannot give the raster its coordinate system", e))?;
    }
    for b in 0..count {
        let values = if b == 0 { &grid.values } else { &grid.bands[b - 1] };
        let data: Vec<f32> = values.iter().map(|v| if v.is_nan() { NODATA } else { *v }).collect();
        let mut band = ds.rasterband(b + 1).map_err(|e| gerr("Cannot write the raster", e))?;
        band.set_no_data_value(Some(NODATA as f64)).map_err(|e| gerr("Cannot write the raster", e))?;
        let mut buf = Buffer::new((w, h), data);
        band.write((0, 0), (w, h), &mut buf).map_err(|e| gerr("Cannot write the raster", e))?;
    }
    let aux = PathBuf::from(format!("{}.aux.xml", path.display()));
    for old in [path.to_path_buf(), aux.clone()] {
        if old.exists() {
            std::fs::remove_file(&old).map_err(|e| GbError(format!("Cannot write over {}: {}. Is it open in another program?", old.display(), e)))?;
        }
    }
    let target = DriverManager::get_driver_by_name("GTiff").map_err(|e| gerr("GDAL has no GeoTIFF support", e))?;
    let opts: gdal::raster::RasterCreationOptions = ["COMPRESS=DEFLATE", "PREDICTOR=3", "TILED=YES"].iter().map(|s| s.to_string()).collect();
    let out = ds.create_copy(&target, path, &opts).map_err(|e| gerr(format!("Cannot write {}", path.display()), e))?;
    out.close().map_err(|e| gerr("Cannot close the raster", e))?;
    drain_messages(notes);
    let mut files = vec![path.to_path_buf()];
    files.extend([aux].into_iter().filter(|f| f.exists()));
    Ok(files)
}

/// How a format wants the geometries of a layer.
#[derive(Clone, Copy, PartialEq)]
enum Shapes {
    /// One geometry type per layer, with single and multi told apart: GeoPackage.
    Typed,
    /// One geometry type per layer, single and multi alike: Shapefile.
    Shapefile,
    /// Any geometries side by side, written as they are: GeoJSON.
    Any,
}

#[allow(clippy::too_many_arguments)]
fn create_layer_and_write(
    ds: &mut Dataset,
    table: &Table,
    features: &[&Feature],
    name: &str,
    fields: &[OutField],
    shapes: Shapes,
    layer_options: &[&str],
    fids: Option<&[i64]>,
) -> Result<()> {
    let (ty, to_multi) = if table.crs.is_none() && !features.iter().any(|f| f.geom.is_some()) {
        (OGRwkbGeometryType::wkbNone, false)
    } else if shapes == Shapes::Any {
        (OGRwkbGeometryType::wkbUnknown, false)
    } else {
        layer_geometry(features, shapes == Shapes::Shapefile)
    };
    // A layer with heights is a layer with Z.
    let ty = if ty != OGRwkbGeometryType::wkbNone && features.iter().any(|f| f.heights().is_some()) { unsafe { gdal_sys::OGR_GT_SetZ(ty) } } else { ty };
    let srs = match table.crs {
        Some(c) if ty != OGRwkbGeometryType::wkbNone => Some(srs_from_epsg(c)?),
        _ => None,
    };
    let layer = ds
        .create_layer(LayerOptions { name, srs: srs.as_ref(), ty, options: if layer_options.is_empty() { None } else { Some(layer_options) } })
        .map_err(|e| gerr(format!("Cannot create the layer {}", name), e))?;
    for of in fields {
        let d = FieldDefn::new(&of.name, of.ty).map_err(|e| gerr("Cannot create a field", e))?;
        if let Some(w) = of.width {
            d.set_width(w);
        }
        if let Some(p) = of.precision {
            d.set_precision(p);
        }
        d.add_to_layer(&layer).map_err(|e| gerr(format!("Cannot create the field {}", of.name), e))?;
    }
    write_features(&layer, table, features, fields, to_multi, fids)
}

/// Adds the features to a layer that is there. Attributes are matched to the fields of the
/// layer by name, and those the layer lacks are added to it. A stream in another coordinate
/// system is refused, since the coordinates would land in the wrong place.
fn append_to_layer(layer: &gdal::vector::Layer, table: &Table, features: &[&Feature], fields: &[OutField], notes: &mut Notes) -> Result<()> {
    let name = layer.name();
    let srs = layer.spatial_ref().filter(|s| !s.is_local());
    if let (Some(srs), Some(crs)) = (srs.as_ref(), table.crs) {
        if let Some(there) = epsg_of(srs).filter(|c| *c != crs) {
            return Err(GbError(format!(
                "The layer {} is in EPSG:{} and the stream in EPSG:{}. Put Reproject before the output to add to it.",
                name, there, crs
            )));
        }
    }
    let mut added = Vec::new();
    let mut targets = Vec::new();
    for of in fields {
        let find = |l: &gdal::vector::Layer| l.defn().fields().position(|f| f.name().eq_ignore_ascii_case(&of.name));
        let at = match find(layer) {
            Some(i) => i,
            None => {
                let d = FieldDefn::new(&of.name, of.ty).map_err(|e| gerr("Cannot create a field", e))?;
                if let Some(w) = of.width {
                    d.set_width(w);
                }
                d.add_to_layer(layer).map_err(|e| gerr(format!("Cannot add the field {} to the layer {}", of.name, name), e))?;
                added.push(of.name.clone());
                find(layer).ok_or_else(|| GbError(format!("The field {} could not be added to the layer {}.", of.name, name)))?
            }
        };
        targets.push(at);
    }
    if !added.is_empty() {
        notes.info(format!("added {} to the layer", added.join(", ")));
    }
    let multi = layer.defn().geom_fields().next().is_some_and(|g| {
        let t = g.field_type();
        t == OGRwkbGeometryType::wkbMultiPoint || t == OGRwkbGeometryType::wkbMultiLineString || t == OGRwkbGeometryType::wkbMultiPolygon
    });
    write_features_to(layer, table, features, fields, &targets, multi, None)
}

/// How the primary key of a new table is written.
pub(crate) struct KeyPlan {
    /// The name of the key column, for the layer option FID.
    pub column: String,
    /// The table to write: without the attribute when its values are the key.
    pub table: Table,
    /// The key of each feature, when an attribute of whole numbers is the key.
    pub fids: Option<Vec<i64>>,
    /// An attribute of other values that is the key: it stays, and is made unique.
    pub unique: Option<String>,
}

/// Plans the primary key: a new column of numbers, or an attribute whose values are all
/// different and none empty.
pub(crate) fn plan_key(table: &Table, key: Option<&og_engine::io::PrimaryKey>, default: &str) -> Result<KeyPlan> {
    use og_engine::io::PrimaryKey;
    match key {
        None => Ok(KeyPlan { column: default.to_string(), table: table.clone(), fids: None, unique: None }),
        Some(PrimaryKey::New(name)) => {
            let name = if name.trim().is_empty() { default } else { name.trim() };
            if let Some(bad) = table.fields.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
                return Err(GbError(format!(
                    "The attribute {} has the name of the new key column. Choose another name for the key, or the attribute as the key.",
                    bad.name
                )));
            }
            Ok(KeyPlan { column: name.to_string(), table: table.clone(), fids: None, unique: None })
        }
        Some(PrimaryKey::Attribute(name)) => {
            let at = table.field_index(name).ok_or_else(|| GbError(format!("There is no attribute {} to be the primary key.", name)))?;
            let mut seen = std::collections::HashSet::new();
            let mut whole = Vec::with_capacity(table.features.len());
            for (i, f) in table.features.iter().enumerate() {
                let v = &f.attrs[at];
                if v.is_null() || v.to_text().trim().is_empty() {
                    return Err(GbError(format!("Row {} has no {}, so it cannot be the primary key.", i + 1, name)));
                }
                if !seen.insert(v.to_text()) {
                    return Err(GbError(format!("{} is {} on more than one row, so it cannot be the primary key.", name, v.to_text())));
                }
                match v {
                    Value::Int(n) => whole.push(Some(*n)),
                    Value::Real(r) if r.fract() == 0.0 && r.abs() < 9e15 => whole.push(Some(*r as i64)),
                    _ => whole.push(None),
                }
            }
            if whole.iter().all(|w| w.is_some()) {
                // Whole numbers are the key itself, in a column of that name.
                let mut t = table.clone();
                t.fields.remove(at);
                for f in t.features.iter_mut() {
                    f.attrs.remove(at);
                }
                Ok(KeyPlan { column: name.clone(), table: t, fids: Some(whole.into_iter().flatten().collect()), unique: None })
            } else {
                Ok(KeyPlan { column: default.to_string(), table: table.clone(), fids: None, unique: Some(name.clone()) })
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_gpkg(
    table: &Table,
    path: &Path,
    layer: &str,
    sizes: &HashMap<String, FieldSize>,
    append: bool,
    key: Option<&og_engine::io::PrimaryKey>,
    notes: &mut Notes,
) -> Result<()> {
    let plan = plan_key(table, key, "fid")?;
    let table = &plan.table;
    let features: Vec<&Feature> = table.features.iter().collect();
    let names = table.field_names();
    if let Some(bad) = names.iter().find(|n| n.eq_ignore_ascii_case(&plan.column) || n.eq_ignore_ascii_case("geom")) {
        return Err(GbError(format!("The attribute {} clashes with a column that GeoPackage creates itself. Rename it with Manage attributes.", bad)));
    }
    let fields = plan_fields(table, &features, names, None, sizes, notes);
    let mut ds = if path.exists() {
        // The file exists: replace only this layer and leave the other layers alone.
        open(path, true, &[])?
    } else {
        DriverManager::get_driver_by_name("GPKG").and_then(|d| d.create_vector_only(path)).map_err(|e| gerr(format!("Cannot create {}", path.display()), e))?
    };
    let mut txn = ds.start_transaction().map_err(|e| gerr("Cannot start writing", e))?;
    let there = append && txn.layer_by_name(layer).is_ok();
    let fid_option = format!("FID={}", plan.column);
    let written = if there {
        if key.is_some_and(|k| !matches!(k, og_engine::io::PrimaryKey::New(n) if n == "fid")) {
            notes.info("the primary key is set when the layer is made, so the layer that was there keeps its own");
        }
        let l = txn.layer_by_name(layer).map_err(|e| gerr(format!("Cannot open the layer {}", layer), e))?;
        append_to_layer(&l, table, &features, &fields, notes)
    } else {
        create_layer_and_write(&mut txn, table, &features, layer, &fields, Shapes::Typed, &["OVERWRITE=YES", &fid_option], plan.fids.as_deref())
    };
    match written {
        Ok(()) => txn.commit().map_err(|e| gerr("Cannot finish writing", e))?,
        Err(e) => {
            let _ = txn.rollback();
            return Err(e);
        }
    }
    if let (Some(u), false) = (&plan.unique, there) {
        // GeoPackage has whole numbers as the key of a layer; other values are kept apart by
        // an index that allows each only once.
        let sql = format!(
            "CREATE UNIQUE INDEX \"{}_{}_key\" ON \"{}\" (\"{}\")",
            layer.replace('"', ""),
            u.replace('"', ""),
            layer.replace('"', "\"\""),
            u.replace('"', "\"\"")
        );
        ds.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map_err(|e| gerr(format!("Cannot make {} unique", u), e))?;
        notes.info(format!("{} is not whole numbers, so the key is {} and {} has an index that keeps its values apart", u, plan.column, u));
    }
    ds.flush_cache().map_err(|e| gerr("Cannot save the file", e))?;
    ds.close().map_err(|e| gerr("Cannot close the file", e))
}

/// Writes the table as a GeoJSON file, which holds one layer. A file that is there is
/// replaced. The geometries are written as they are, single and multi side by side, in the
/// coordinate system of the stream.
fn write_geojson(table: &Table, path: &Path, layer: &str, sizes: &HashMap<String, FieldSize>, append: bool, notes: &mut Notes) -> Result<()> {
    let driver = DriverManager::get_driver_by_name("GeoJSON").map_err(|e| gerr("GDAL has no GeoJSON support", e))?;
    let features: Vec<&Feature> = table.features.iter().collect();
    let fields = plan_fields(table, &features, table.field_names(), None, sizes, notes);
    if append && path.exists() {
        // GeoJSON is one text, so the file is read, the features are added after those
        // that are there, and it is written again. The order then stays as it was.
        let there = GdalBackend::new().read(path, &ReadOpts::default(), &mut Notes::default())?;
        if let (Some(a), Some(b)) = (there.crs, table.crs) {
            if a != b {
                return Err(GbError(format!(
                    "{} is in EPSG:{} and the stream in EPSG:{}. Put Reproject before the output to add to it.",
                    path.display(),
                    a,
                    b
                )));
            }
        }
        let added: Vec<String> = table.fields.iter().filter(|f| there.field_index(&f.name).is_none()).map(|f| f.name.clone()).collect();
        if !added.is_empty() && !there.is_empty() {
            notes.info(format!("added {} to the layer", added.join(", ")));
        }
        let both =
            og_engine::nodes::merge_tables(vec![std::sync::Arc::new(there), std::sync::Arc::new(table.clone())], None, "the file and the stream", notes)?;
        return write_geojson(&both, path, layer, sizes, false, notes);
    }
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| GbError(format!("Cannot overwrite {}: {}", path.display(), e)))?;
    }
    // Millimetres in metres, about a centimetre in degrees. More decimals only make the file larger.
    let geographic = table.crs.and_then(|c| SpatialRef::from_epsg(c).ok()).is_none_or(|s| s.is_geographic());
    let precision = format!("COORDINATE_PRECISION={}", if geographic { 7 } else { 3 });
    let mut ds = driver.create_vector_only(path).map_err(|e| gerr(format!("Cannot create {}", path.display()), e))?;
    create_layer_and_write(&mut ds, table, &features, layer, &fields, Shapes::Any, &[&precision], None)?;
    ds.flush_cache().map_err(|e| gerr("Cannot save the file", e))?;
    ds.close().map_err(|e| gerr("Cannot close the file", e))
}

/// Writes the table as FlatGeobuf, one geometry type and a spatial index. A file that is
/// there is replaced; FlatGeobuf cannot be added to, so with append the file is read and
/// written again with the new features after.
fn write_fgb(table: &Table, path: &Path, layer: &str, sizes: &HashMap<String, FieldSize>, append: bool, notes: &mut Notes) -> Result<()> {
    if append && path.exists() {
        let there = GdalBackend::new().read(path, &ReadOpts { crs: table.crs, ..Default::default() }, &mut Notes::default())?;
        let both =
            og_engine::nodes::merge_tables(vec![std::sync::Arc::new(there), std::sync::Arc::new(table.clone())], None, "the file and the stream", notes)?;
        return write_fgb(&both, path, layer, sizes, false, notes);
    }
    let driver = DriverManager::get_driver_by_name("FlatGeobuf").map_err(|e| gerr("GDAL has no FlatGeobuf support", e))?;
    let features: Vec<&Feature> = table.features.iter().collect();
    let fields = plan_fields(table, &features, table.field_names(), None, sizes, notes);
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| GbError(format!("Cannot overwrite {}: {}", path.display(), e)))?;
    }
    let mut ds = driver.create_vector_only(path).map_err(|e| gerr(format!("Cannot create {}", path.display()), e))?;
    create_layer_and_write(&mut ds, table, &features, layer, &fields, Shapes::Typed, &["SPATIAL_INDEX=YES"], None)?;
    ds.flush_cache().map_err(|e| gerr("Cannot save the file", e))?;
    ds.close().map_err(|e| gerr("Cannot close the file", e))
}

/// Field names in a Shapefile: at most ten bytes in the encoding of the file, and unique.
fn shp_names(table: &Table, bytes: &dyn Fn(&str) -> usize, notes: &mut Notes) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut renamed: Vec<String> = Vec::new();
    for f in &table.fields {
        let cut = |limit: usize| -> String {
            let mut s = String::new();
            for c in f.name.chars() {
                let mut next = s.clone();
                next.push(c);
                if bytes(&next) > limit {
                    break;
                }
                s = next;
            }
            s
        };
        let mut name = cut(10);
        let mut k = 1;
        while name.is_empty() || out.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            let suffix = format!("_{}", k);
            name = cut(10 - suffix.len()) + &suffix;
            k += 1;
        }
        if name != f.name {
            renamed.push(format!("{} → {}", f.name, name));
        }
        out.push(name);
    }
    if !renamed.is_empty() {
        notes.warn(format!("Field names in a Shapefile can have at most ten characters. Shortened: {}.", renamed.join(", ")));
    }
    out
}

fn write_shp(
    table: &Table,
    path: &Path,
    stem: &str,
    encoding: &str,
    sizes: &HashMap<String, FieldSize>,
    append: bool,
    notes: &mut Notes,
) -> Result<Vec<PathBuf>> {
    let enc = gdal_encoding(encoding);
    let single_byte = enc != "UTF-8";
    let bytes = move |s: &str| if single_byte { s.chars().count() } else { s.len() };
    let driver = DriverManager::get_driver_by_name("ESRI Shapefile").map_err(|e| gerr("GDAL has no Shapefile support", e))?;

    // Split by geometry family. Features without geometry go with the first group.
    let mut groups: Vec<(Family, Vec<&Feature>)> = Vec::new();
    let mut no_geom: Vec<&Feature> = Vec::new();
    for f in &table.features {
        match &f.geom {
            None => no_geom.push(f),
            Some(g) => {
                let fam = geom::family(g);
                if fam == Family::Other {
                    return Err(GbError("A Shapefile cannot hold geometry collections. Write to GeoPackage instead.".into()));
                }
                match groups.iter_mut().find(|x| x.0 == fam) {
                    Some(grp) => grp.1.push(f),
                    None => groups.push((fam, vec![f])),
                }
            }
        }
    }
    groups.sort_by_key(|g| g.0);
    if groups.is_empty() {
        groups.push((Family::Other, Vec::new()));
    }
    groups[0].1.extend(no_geom);
    let split = groups.len() > 1;
    if split {
        notes.warn(format!("A Shapefile holds one geometry type, so the stream was split into {} files.", groups.len()));
    }
    let names = shp_names(table, &bytes, notes);
    let enc_opt = format!("ENCODING={}", enc);
    let mut files = Vec::new();
    for (fam, features) in &groups {
        let suffix = match fam {
            _ if !split => "",
            Family::Point => "_point",
            Family::Line => "_line",
            _ => "_area",
        };
        let name = format!("{}{}", stem, suffix);
        let file = path.with_file_name(format!("{}.shp", name));
        let fields = plan_fields(table, features, names.clone(), Some(&bytes), sizes, notes);
        let there = if file.exists() { Some(file.clone()) } else { Some(file.with_extension("dbf")).filter(|d| d.exists()) };
        if let (true, Some(existing)) = (append, there) {
            let ds = open(&existing, true, &[format!("ENCODING={}", enc)])?;
            {
                let l = ds.layer(0).map_err(|e| gerr(format!("Cannot open {}", existing.display()), e))?;
                append_to_layer(&l, table, features, &fields, notes)?;
            }
            ds.close().map_err(|e| gerr("Cannot close the file", e))?;
            files.push(existing);
            continue;
        }
        if file.exists() || file.with_extension("dbf").exists() {
            // Removes .shp, .shx, .dbf, .prj and .cpg from an earlier run.
            driver.delete(&file).map_err(|e| gerr(format!("Cannot overwrite {}", file.display()), e))?;
        }
        let mut ds = driver.create_vector_only(&file).map_err(|e| gerr(format!("Cannot create {}", file.display()), e))?;
        create_layer_and_write(&mut ds, table, features, &name, &fields, Shapes::Shapefile, &[&enc_opt], None)?;
        ds.flush_cache().map_err(|e| gerr("Cannot save the file", e))?;
        ds.close().map_err(|e| gerr("Cannot close the file", e))?;
        // A table without geometry becomes just a .dbf.
        files.push(if file.exists() { file } else { file.with_extension("dbf") });
    }
    Ok(files)
}

/// The EPSG coordinate systems in the database of PROJ, read once.
pub fn crs_list() -> &'static [og_engine::io::CrsEntry] {
    static LIST: std::sync::OnceLock<Vec<og_engine::io::CrsEntry>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        use gdal_sys::OSRCRSType::*;
        let text = |p: *mut std::ffi::c_char| if p.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned() };
        let auth = std::ffi::CString::new("EPSG").expect("no nul");
        let mut n: std::ffi::c_int = 0;
        // SAFETY: the list is read as GDAL describes it, and freed by GDAL once it is copied.
        let list = unsafe { gdal_sys::OSRGetCRSInfoListFromDatabase(auth.as_ptr(), std::ptr::null(), &mut n) };
        if list.is_null() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n.max(0) as usize {
            let info = unsafe { &**list.add(i) };
            let kind = match info.eType {
                OSR_CRS_TYPE_PROJECTED => "projected",
                OSR_CRS_TYPE_GEOGRAPHIC_2D => "geographic",
                OSR_CRS_TYPE_COMPOUND => "compound",
                _ => continue,
            };
            if info.bDeprecated != 0 {
                continue;
            }
            let Ok(code) = text(info.pszCode).parse::<u32>() else { continue };
            out.push(og_engine::io::CrsEntry { code, name: text(info.pszName), kind, area: text(info.pszAreaName) });
        }
        unsafe { gdal_sys::OSRDestroyCRSInfoList(list) };
        out.sort_by_key(|c| c.code);
        // PROJ can list a code twice, from two sources; one is enough.
        out.dedup_by_key(|c| c.code);
        out
    })
}
