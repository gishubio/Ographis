//! The parameter form for the selected node. The fields are built from the description of the node type.

use crate::graph::{self, GNode};
use crate::sources::{guess_pattern, Layers};
use crate::theme::{self, Palette};
use eframe::egui::{self, RichText, TextEdit, Ui};
use og_engine::expr;
use og_engine::model::{parse_crs, KNOWN_CRS};
use og_engine::nodes::{is_url, source_ext, writer_file, ParamKind, ParamSpec, Sources, WRITER_FORMATS};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

mod layers;
mod lists;
mod tables;
use layers::*;
pub(crate) use layers::{colour_button, colour_code, parse_colour_code};
use lists::*;
use tables::*;

pub struct ParamCtx<'a> {
    pub pal: Palette,
    /// The attributes that came in on a given input port in the latest run.
    pub fields: &'a dyn Fn(&str) -> Vec<String>,
    /// The same attributes with the name of the type of each.
    pub field_types: &'a dyn Fn(&str) -> Vec<(String, &'static str)>,
    /// The folder that relative paths start from.
    pub base_dir: &'a Path,
    /// Whether chosen files are stored relative to that folder. False until the workspace
    /// has been saved, since the folder is then only where the program happened to start.
    pub relative: bool,
    /// For a reader: the files its path stands for, when that can be told.
    pub sources: Option<&'a Result<Sources, String>>,
    /// For a reader of one file: the layers in that file.
    pub layers: Option<&'a Layers>,
    /// The values of an attribute on an input port in the latest run, without repeats and in
    /// the order they come: (port, attribute) → values. At most a hundred.
    pub values: &'a dyn Fn(&str, &str) -> Vec<String>,
    /// The variables of the model, put in place of $(name) before an expression is checked.
    pub variables: &'a [og_engine::Variable],
    /// The connections into an input port of the node: (where from, as "node:port", and the
    /// name to show), in the order they were made.
    pub connections: &'a dyn Fn(&str) -> Vec<(String, String)>,
    /// What an expression gives for the first feature on an input port in the latest run,
    /// as text to show, or None when nothing has run.
    pub preview: &'a dyn Fn(&str, &str) -> Option<String>,
    /// The names of the database connections in the settings.
    pub db_connections: &'a [String],
    pub api_connections: &'a [String],
    /// Every coordinate system PROJ knows, to search among.
    pub crs_list: &'a [og_engine::io::CrsEntry],
    /// The attributes of what an output port gave in the latest run, by "node:port", each
    /// with whether it is a number.
    pub stream_fields: &'a dyn Fn(&str) -> Vec<(String, bool)>,
    /// What the raster of an output port in the latest run holds, by "node:port": None for a
    /// stream of features, or one that has not run.
    pub stream_raster: &'a dyn Fn(&str) -> Option<og_engine::rasterlook::RasterInfo>,
    /// Whether expressions get suggestions while they are written.
    pub suggest: bool,
    /// Whether the help of every setting is shown under it, not only when the pointer is on it.
    pub hints_always: bool,
    /// When the latest run that is shown ended, as a time to show.
    pub ran_at: Option<String>,
    /// The classes of the points on an input port in the latest run, with how many of each.
    pub classes: &'a dyn Fn(&str) -> Vec<(i64, u64)>,
}

/// What a form changed.
#[derive(Default, Clone, Copy)]
pub struct Edited {
    /// Something that the workspace file holds.
    pub any: bool,
    /// Something that changes what a run gives.
    pub run: bool,
    /// The PostGIS button of an input was clicked: the window for the database is to open.
    pub postgis: bool,
    /// The settings are to open, to add or change a database connection.
    pub open_settings: bool,
    /// The rectangle of a download is to be chosen on the map.
    pub pick_area: bool,
    /// The server, database, user and password written in the node are to become a
    /// connection in the settings, which the node then names.
    pub save_connection: bool,
    /// The model inside a tool is to be opened on the canvas.
    pub open_tool: bool,
    /// A tool is to be unpacked into the nodes it is made of.
    pub unpack_tool: bool,
}

/// The file extensions of formats that keep their data in sheets.
pub fn is_spreadsheet(ext: &str) -> bool {
    matches!(ext, "xlsx" | "xlsm" | "ods" | "xls")
}

/// The files a reader of a folder stands for. `None` for a reader of one file.
fn folder_of<'a>(cx: &'a ParamCtx) -> Option<&'a Sources> {
    cx.sources.and_then(|s| s.as_ref().ok()).filter(|s| s.is_many())
}

/// The kind of file a reader or writer handles, as a lower case extension. For a folder
/// it is taken from the filter.
fn format_of(node: &GNode, folder: Option<&Sources>) -> String {
    let path = node.params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if node.kind == "Writer" {
        // A writer that has been told a format writes that, whatever the name says.
        return ext_of(&writer_file(path, node.params.get("format").and_then(|v| v.as_str())));
    }
    match folder.and_then(|s| s.folder.as_ref()) {
        Some((_, patterns)) => patterns.iter().map(|p| ext_of(p)).find(|e| !e.is_empty() && !e.contains('*')).unwrap_or_default(),
        None => source_ext(path),
    }
}

fn get(node: &GNode, spec: &ParamSpec) -> String {
    match node.params.get(spec.key) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        Some(Json::Bool(b)) => b.to_string(),
        Some(Json::Array(a)) => a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "),
        _ => spec.default.to_string(),
    }
}

fn set(node: &mut GNode, spec: &ParamSpec, value: String) {
    if value.is_empty() || value == spec.default {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::String(value));
    }
}

fn ext_of(path: &str) -> String {
    Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Should the field be shown, given what the node points to? Readers and writers have
/// fields that only apply to some formats, and a reader has some that only apply to folders.
fn relevant(node: &GNode, spec: &ParamSpec, folder: Option<&Sources>, has_layers: bool, base: &Path) -> bool {
    let (reader, writer) = (node.kind == "Reader", node.kind == "Writer");
    if !reader && !writer {
        // The start position only matters when the characters are taken from a position,
        // and the text for other values when that is what they get.
        let param = |k: &str| node.params.get(k).and_then(|v| v.as_str()).map(str::to_string);
        return match (node.kind.as_str(), spec.key) {
            ("SubstringExtractor", "start") => param("part").as_deref() == Some("middle"),
            ("ValueMapper", "default") => param("otherwise").as_deref() == Some("text"),
            ("TerrainAnalyzer", "radius") => param("measure").as_deref() == Some("tpi"),
            ("CustomTool", "each_by") => param("loop").as_deref() == Some("each"),
            ("JsonBuilder", "gather_by") => param("gather").as_deref() == Some("group"),
            ("JsonBuilder", "list_name") => param("gather").is_some_and(|g| g != "each"),
            ("StringConcatenator", "fixed") => node.params.get("attributes").is_some_and(|v| match v {
                serde_json::Value::Array(a) => a.iter().any(|x| x.as_str().is_some_and(|t| t.trim().starts_with('\''))),
                serde_json::Value::String(t) => t.contains('\''),
                _ => false,
            }),
            ("LineLengthInAreas", "layout") => node.params.get("group_by").is_some_and(|v| match v {
                serde_json::Value::Array(a) => !a.is_empty(),
                serde_json::Value::String(t) => !t.trim().is_empty(),
                _ => false,
            }),
            ("CustomTool", "times") => param("loop").as_deref() == Some("repeat"),
            ("PointCloudFilter", "height_of") => param("zmin").is_some_and(|v| !v.is_empty()) || param("zmax").is_some_and(|v| !v.is_empty()),
            // The settings of the connection that models before 0.63 kept in the node are shown
            // only while such a model has them and names no connection.
            ("PostgisReader" | "PostgisWriter", "host" | "port" | "database" | "user" | "password") => {
                param("connection").is_none_or(|c| c.is_empty())
                    && ["host", "database", "user", "password"].iter().any(|k| param(k).is_some_and(|v| !v.is_empty()))
            }
            ("SceneRenderer", "sun_azimuth" | "sun_altitude") => !matches!(param("sun").as_deref(), Some("date" | "study")),
            ("SceneRenderer", "study_dates" | "study_from" | "study_to" | "study_every" | "study_folder") => param("sun").as_deref() == Some("study"),
            ("SceneRenderer", "background_colour") => param("background").as_deref() == Some("colour"),
            ("SceneRenderer", "elevation" | "projection" | "fov") => param("camera").is_none_or(|c| c.is_empty()),
            ("MapRenderer", "width") => param("size_as").as_deref() != Some("print"),
            ("MapRenderer", "grid_every") => !matches!(param("grid").as_deref(), None | Some("none") | Some("")),
            ("MapRenderer", "overview_size" | "overview_zoom") => !matches!(param("overview").as_deref(), None | Some("none") | Some("")),
            ("MapRenderer", "print_mm" | "print_dpi") => param("size_as").as_deref() == Some("print"),
            ("MapRenderer", "background_colour") => param("background").as_deref() == Some("colour"),
            ("MapRenderer", "basemap") => param("background").as_deref() == Some("map"),
            ("MapRenderer", "basemap_own") => param("background").as_deref() == Some("map") && param("basemap").as_deref() == Some("own"),
            ("MapRenderer", "background_picture" | "background_crs") => param("background").as_deref() == Some("picture"),
            ("MapRenderer", "background_fade") => matches!(param("background").as_deref(), Some("map" | "picture")),
            ("SceneRenderer", "water_level" | "waves") => param("water").is_some_and(|w| w != "none"),
            ("PostgisWriter", "key_name") => param("mode").as_deref() != Some("update") && param("key").as_deref() != Some("attribute"),
            ("PostgisWriter", "key_attribute") => param("mode").as_deref() != Some("update") && param("key").as_deref() == Some("attribute"),
            ("PostgisWriter", "commit_rows") => param("mode").as_deref() != Some("update") && param("commit").as_deref() == Some("every"),
            ("PostgisWriter", "update_key" | "delete_missing") => param("mode").as_deref() == Some("update"),
            // An update is one transaction, and does not make a new table's key.
            ("PostgisWriter", "commit" | "key") => param("mode").as_deref() != Some("update"),
            ("LayeredPdfWriter", "pdf_width" | "pdf_height") => param("pdf_size").as_deref() == Some("custom"),
            ("QgisProjectWriter" | "WebMapWriter", "basemap_own") => param("basemap").as_deref() == Some("own"),
            ("SceneRenderer", "focus") => param("blur").is_some_and(|b| b.trim().parse::<f64>().is_ok_and(|v| v > 0.0)),
            // Ready rules of the road take the place of the conditions and speeds of one's own.
            ("ShortestRoute" | "ServiceAreas" | "NetworkAllocation" | "LocationChooser", "mode" | "avoid") => param("rules").is_some_and(|r| r != "own"),
            ("ShortestRoute" | "ServiceAreas" | "NetworkAllocation" | "LocationChooser", "speed" | "speed_against" | "along" | "against") => {
                param("rules").is_none_or(|r| r == "own")
            }
            ("HttpCaller", "folder") => param("answer_as").as_deref() == Some("file"),
            ("SpatialJoiner", "distance") => param("relation").as_deref() == Some("near"),
            ("SpatialJoiner", "numbers" | "count") => param("mode").as_deref() == Some("summary"),
            ("ConvexHuller" | "ConcaveHuller", "group_by" | "count") => param("for").as_deref() != Some("each"),
            ("HttpCaller", "response" | "status" | "paging" | "parallel" | "body" | "content_type" | "per_second")
                if param("answer_as").as_deref() == Some("file") =>
            {
                false
            }
            ("HttpCaller", "next_path") => param("paging").as_deref() == Some("next"),
            ("HttpCaller", "page_param" | "size_param" | "page_size" | "start" | "items_path") => matches!(param("paging").as_deref(), Some("offset" | "page")),
            ("HttpCaller", "max_pages") => matches!(param("paging").as_deref(), Some("next" | "offset" | "page")),
            ("SceneRenderer", "date" | "time") => param("sun").as_deref() == Some("date"),
            ("SceneRenderer", "clock") => matches!(param("sun").as_deref(), Some("date" | "study")),
            ("Rotator", "x" | "y") => param("around").as_deref() == Some("point"),
            ("ZipWriter", "names") => param("outputs").as_deref() == Some("named"),
            ("HttpCaller", "body" | "content_type") => param("method").is_some_and(|m| matches!(m.as_str(), "POST" | "PUT" | "PATCH")),
            ("RangeMapper", "ranges") => param("method").is_none_or(|m| m == "own"),
            ("RangeMapper", "classes" | "decimals") => param("method").is_some_and(|m| m != "own"),
            // A count has nothing to put together, and the second colour needs bars to colour.
            ("ChartDrawer", "combine") => param("value").is_some_and(|v| !v.trim().is_empty()),
            ("ChartDrawer", "accent") => node.params.get("highlight").is_some_and(|v| match v {
                serde_json::Value::Array(a) => !a.is_empty(),
                serde_json::Value::String(s) => !s.trim().is_empty(),
                _ => false,
            }),
            _ => true,
        };
    }
    let path = node.params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let web = reader && is_url(path);
    // A tileset of 3D Tiles has settings of its own, and few of the others apply.
    let web_as = node.params.get("web_as").and_then(|v| v.as_str());
    if reader && !path.is_empty() && og_engine::nodes::is_tileset(path, web_as) {
        let detail = node.params.get("tiles_detail").and_then(|v| v.as_str());
        return match spec.key {
            "path" | "tiles_detail" | "tiles_most" | "tiles_crs" | "tiles_dz" | "within" => true,
            "tiles_error" => detail == Some("error"),
            "web_as" | "connection" => web,
            "min_x" | "min_y" | "max_x" | "max_y" | "within_crs" => node.params.get("within").and_then(|v| v.as_str()) == Some("rectangle"),
            _ => false,
        };
    }
    // Vector tiles from the web, by an address with {z}/{x}/{y}.
    if reader && !path.is_empty() && og_engine::nodes::vector_tiles::is_template(path, web_as) {
        return match spec.key {
            "path" | "vt_zoom" | "vt_pieces" | "tiles_most" | "layer" | "layer_attr" | "web_as" | "connection" => true,
            "within" => true,
            "min_x" | "min_y" | "max_x" | "max_y" | "within_crs" => node.params.get("within").and_then(|v| v.as_str()) == Some("rectangle"),
            _ => false,
        };
    }
    if spec.key.starts_with("tiles_") {
        return false;
    }
    let ext = format_of(node, folder);
    let text = matches!(ext.as_str(), "csv" | "txt" | "tsv");
    // A Google sheet has sheets, as a workbook does.
    let google = og_engine::gsheets::is_sheet(path) || node.params.get("format").and_then(|v| v.as_str()) == Some("gsheet");
    let sheets = is_spreadsheet(&ext) || google;
    let layered = has_layers || sheets || matches!(ext.as_str(), "gpkg" | "gdb" | "sqlite" | "db");
    let filled = |key: &str| node.params.get(key).and_then(|v| v.as_str()).is_some_and(|v| !v.trim().is_empty());
    // A picture has neither attributes nor anything to add to.
    let image = writer && og_engine::nodes::is_image_format(&ext);
    let walk = node.params.get("format").and_then(|v| v.as_str()) == Some("walk");
    let full = base.join(path);
    let tiles_file = matches!(ext.as_str(), "mbtiles" | "pmtiles") || (reader && og_engine::nodes::vector_tiles::is_tile_folder(&full));
    match spec.key {
        "vt_zoom" | "vt_pieces" => reader && tiles_file,
        "zip_file" => reader && (path.to_lowercase().ends_with(".zip") || og_engine::nodes::source_ext(path) == "zip"),
        "mode" | "fields" => {
            !image && !matches!(ext.as_str(), "obj" | "glb" | "gml") && !matches!(node.params.get("format").and_then(|v| v.as_str()), Some("3dtiles" | "walk"))
        }
        "pdf_size" | "pdf_orientation" | "pdf_margin" | "pdf_title" | "pdf_compress" | "pdf_as" => writer && ext == "pdf",
        "pdf_width" | "pdf_height" => writer && ext == "pdf" && node.params.get("pdf_size").and_then(|v| v.as_str()) == Some("custom"),
        "pdf_dpi" => writer && ext == "pdf" && node.params.get("pdf_size").and_then(|v| v.as_str()) == Some("fit"),
        "pdf_quality" => writer && ext == "pdf" && node.params.get("pdf_compress").and_then(|v| v.as_str()) == Some("jpeg"),
        "pm_min" | "pm_max" => writer && ext == "pmtiles",
        "gpkg_version" => writer && ext == "gpkg",
        "geojson_crs" => writer && (ext == "geojson" || (ext == "json" && node.params.get("format").and_then(|v| v.as_str()) != Some("json"))),
        "json_shape" | "json_geometry" => writer && node.params.get("format").and_then(|v| v.as_str()) == Some("json"),
        "xml_root" | "xml_row" | "xml_geometry" => writer && ext == "xml" && node.params.get("format").and_then(|v| v.as_str()) != Some("landxml"),
        "schema_files" => writer && matches!(ext.as_str(), "geojson" | "json"),
        "connection" if writer => node.params.get("format").and_then(|v| v.as_str()) == Some("gsheet") || og_engine::gsheets::is_sheet(path),
        "json_list_name" => {
            writer
                && node.params.get("format").and_then(|v| v.as_str()) == Some("json")
                && node.params.get("json_shape").and_then(|v| v.as_str()).is_none_or(|s| s == "list")
        }
        "img_dpi" => writer && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "tif" | "tiff"),
        "img_quality" => {
            writer
                && (matches!(ext.as_str(), "jpg" | "jpeg")
                    || (matches!(ext.as_str(), "tif" | "tiff") && node.params.get("tif_compress").and_then(|v| v.as_str()) == Some("jpeg")))
        }
        "tif_compress" => writer && matches!(ext.as_str(), "tif" | "tiff"),
        "t3_most" | "t3_dz" | "t3_lod" => writer && node.params.get("format").and_then(|v| v.as_str()) == Some("3dtiles"),
        "citygml_lod" => writer && ext == "gml",
        "gltf_origin" => writer && (matches!(ext.as_str(), "glb" | "obj" | "usda" | "usdz" | "ply" | "spz") || walk),
        "walk_eye" | "walk_speed" | "walk_start" | "walk_utc" => writer && walk,
        "print_scale" => writer && matches!(ext.as_str(), "stl" | "3mf" | "step" | "stp"),
        "usd_layers" | "usd_variants" => writer && matches!(ext.as_str(), "usda" | "usdz"),
        "cad_template" | "cad_keep" | "cad_layer" | "cad_colour" | "cad_linetype" | "cad_block" | "cad_rotation" | "cad_text" | "cad_text_height"
        | "cad_version" => writer && matches!(ext.as_str(), "dxf" | "dwg"),
        "gltf_name" | "gltf_group" | "gltf_material" => writer && (ext == "glb" || walk),
        "gltf_georef" | "gltf_sun" => writer && ext == "glb",
        "gltf_compress" => writer && (ext == "glb" || walk || node.params.get("format").and_then(|v| v.as_str()) == Some("3dtiles")),
        "gltf_when" => writer && ((ext == "glb" && matches!(node.params.get("gltf_sun").and_then(|v| v.as_str()), Some("sun" | "both"))) || walk),
        "key" => writer && ext == "gpkg",
        "key_name" => writer && ext == "gpkg" && node.params.get("key").and_then(|v| v.as_str()) != Some("attribute"),
        "key_attribute" => writer && ext == "gpkg" && node.params.get("key").and_then(|v| v.as_str()) == Some("attribute"),
        "pdf_title_align" => writer && ext == "pdf" && filled("pdf_title"),
        "x" | "y" | "wkt" | "delimiter" | "decimal_comma" => text,
        "geometry" => text || sheets,
        "font" | "bold_header" | "freeze" | "colors" => writer && sheets,
        "raster_as" => reader && og_engine::io::is_raster(&ext),
        // A rectangle is for features, not for the cells of a raster.
        // For a raster, a rectangle is read as a part of the picture or the grid.
        "within" => {
            reader
                && !sheets
                && !matches!(ext.as_str(), "obj" | "gltf" | "glb" | "ifc")
                && (!og_engine::io::is_raster(&ext) || matches!(node.params.get("raster_as").and_then(|v| v.as_str()), Some("image" | "grid")))
        }
        "min_x" | "min_y" | "max_x" | "max_y" | "within_crs" => reader && node.params.get("within").and_then(|v| v.as_str()) == Some("rectangle"),
        "osm_tags" => reader && matches!(ext.as_str(), "pbf" | "osm"),
        "las_classes" | "las_spacing" => reader && matches!(ext.as_str(), "las" | "laz"),
        "las_every" => reader && matches!(ext.as_str(), "las" | "laz" | "e57"),
        "obj_unit" | "obj_up" => reader && matches!(ext.as_str(), "obj" | "gltf" | "glb" | "stl" | "3mf" | "step" | "stp"),
        "obj_east" | "obj_north" | "obj_height" | "obj_rotation" => {
            reader && matches!(ext.as_str(), "obj" | "gltf" | "glb" | "ifc" | "ply" | "spz" | "stl" | "3mf" | "step" | "stp")
        }
        "splat_scale" => reader && matches!(ext.as_str(), "ply" | "spz"),
        "splat_up" => matches!(ext.as_str(), "ply" | "spz"),
        "ifc_openings" | "ifc_spaces" | "ifc_properties" => reader && ext == "ifc",
        "dxf_blocks" | "dxf_closed" => reader && matches!(ext.as_str(), "dxf" | "dwg"),
        "city_lod" => {
            reader && matches!(ext.as_str(), "gml" | "xml" | "json" | "jsonl") && og_engine::citymodel::is_city_path(&base.join(path).to_string_lossy())
        }
        // A file on the web may say what it is only when it has been fetched.
        "json_records" | "json_nested" => reader && (matches!(ext.as_str(), "json" | "ndjson" | "jsonl" | "xml") || web),
        "cell_size" => reader && og_engine::io::is_raster(&ext) && node.params.get("raster_as").and_then(|v| v.as_str()) == Some("grid"),
        "step" => reader && og_engine::io::is_cube(&ext) && matches!(node.params.get("raster_as").and_then(|v| v.as_str()), Some("grid" | "image")),
        "layer" => layered || (writer && ext == "pmtiles") || (reader && (og_engine::io::is_cube(&ext) || ext == "xml")),
        "layer_attr" => layered,
        "header" => sheets,
        "encoding" => text || ext == "shp",
        // An address on the web need not end in a file name that says what the file is.
        "crs" => (!ext.is_empty() || web) && !sheets,
        "web_as" | "connection" => web,
        "keep_download" => web && og_engine::nodes::service_name(path, node.params.get("web_as").and_then(|v| v.as_str())).is_none(),
        // The filter belongs to a folder. A path with * in it is a filter already.
        "pattern" | "recursive" => folder.is_some() && !path.contains('*') && !path.contains('?'),
        "filename_part" => filled("filename_attr"),
        _ => true,
    }
}

/// The label of a field. What applies to a format is said by the field being shown at all,
/// and a workbook has sheets where other files have layers.
fn label_of(spec: &ParamSpec, sheets: bool) -> String {
    let label = [" (CSV, Excel)", " (CSV)", " (Excel)"].iter().find_map(|suffix| spec.label.strip_suffix(suffix)).unwrap_or(spec.label);
    match (sheets, spec.key) {
        (true, "layer") => label.replace("Layers", "Sheets").replace("Layer", "Sheet"),
        (true, "layer_attr") => label.replace("Layer", "Sheet").replace("layer", "sheet"),
        _ => label.to_string(),
    }
}

/// The value of a list parameter: stored as a JSON list, or as text with commas.
pub fn list_of(node: &GNode, key: &str) -> Vec<String> {
    match node.params.get(key) {
        Some(Json::Array(a)) => a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
        Some(Json::String(s)) => split_list(s),
        _ => Vec::new(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Which layers of the file to read, as ticks in a list. Returns true if the choice changed.
fn layer_ticks(ui: &mut Ui, node: &mut GNode, key: &str, layers: &[og_engine::io::LayerInfo], what: &str, pal: &Palette) -> bool {
    let before = list_of(node, key);
    let all = before.iter().any(|l| l == "*");
    let mut chosen: Vec<String> = before.iter().filter(|l| layers.iter().any(|x| &x.name == *l)).cloned().collect();
    let mut every = all;
    ui.checkbox(&mut every, format!("All {} {}", layers.len(), what));
    // A long list can be searched. What is ticked stays ticked when it is not shown.
    let search_id = ui.id().with(("layer-search", key));
    let mut search: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
    if layers.len() > 10 {
        ui.add(TextEdit::singleline(&mut search).desired_width(f32::INFINITY).hint_text(format!("Search the {}", what)));
        ui.data_mut(|d| d.insert_temp(search_id, search.clone()));
    }
    let wanted = search.trim().to_lowercase();
    ui.add_enabled_ui(!every, |ui| {
        egui::ScrollArea::vertical().id_salt(("layers", key)).max_height(190.0).auto_shrink([false, true]).show(ui, |ui| {
            for l in layers.iter().filter(|l| wanted.is_empty() || l.name.to_lowercase().contains(&wanted)) {
                let mut on = every || chosen.contains(&l.name);
                let count = l.count.map_or(String::new(), |c| format!("{} · ", c));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ui.checkbox(&mut on, &l.name).changed() && !every {
                        if on {
                            chosen.push(l.name.clone());
                        } else {
                            chosen.retain(|c| c != &l.name);
                        }
                    }
                    ui.add(
                        egui::Label::new(RichText::new(format!("{}{}", count, l.geometry.to_lowercase())).font(theme::mono(10.5)).color(pal.muted)).truncate(),
                    );
                });
            }
        });
    });
    // The layers are read in the order they have in the file.
    let after: Vec<String> = if every { vec!["*".to_string()] } else { layers.iter().map(|l| l.name.clone()).filter(|n| chosen.contains(n)).collect() };
    if after == before || (after.is_empty() && before.is_empty()) {
        return false;
    }
    if after.is_empty() {
        node.params.remove(key);
    } else {
        node.params.insert(key.to_string(), Json::Array(after.into_iter().map(Json::String).collect()));
    }
    true
}

/// Under the path of a reader of a folder: how many files match, and which.
fn folder_summary(ui: &mut Ui, sources: &Sources, pal: &Palette) {
    let Some((dir, patterns)) = &sources.folder else { return };
    let n = sources.files.len();
    if n == 0 {
        ui.label(RichText::new(format!("No files match {} in this folder.", patterns.join("; "))).small().color(pal.warn));
        return;
    }
    let heading = format!("{} {} read as one stream", n, if n == 1 { "file is" } else { "files are" });
    egui::CollapsingHeader::new(RichText::new(heading).small().color(pal.ink)).id_salt("matched_files").show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for f in sources.files.iter().take(200) {
            // Files in subfolders are shown with the way to them.
            let shown = f.strip_prefix(dir).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| file_name(f));
            ui.add(egui::Label::new(RichText::new(shown).font(theme::mono(11.0)).color(pal.muted)).truncate());
        }
        if n > 200 {
            ui.label(RichText::new(format!("… and {} more", n - 200)).small().color(pal.muted));
        }
    });
}

/// Writes a field name so that it can be used in an expression.
fn quoted_field(name: &str) -> String {
    let plain = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    let keyword = ["and", "or", "not", "is", "in", "like", "ilike", "null", "true", "false"].contains(&name.to_lowercase().as_str());
    if plain && !keyword {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// The path as it is stored in a node: relative to the workspace folder when the file is
/// inside it, so that the folder can be moved, and absolute otherwise.
pub fn relative_to(base: &Path, path: &Path) -> String {
    let (base, full) = (base.canonicalize().unwrap_or_else(|_| base.to_path_buf()), path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    match full.strip_prefix(&base) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

fn chips(ui: &mut Ui, pal: &Palette, names: &[String], is_on: impl Fn(&str) -> bool) -> Option<String> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for n in names {
            let on = is_on(n);
            let text = RichText::new(n).font(theme::mono(11.5)).color(pal.ink);
            let button = egui::Button::new(text)
                .small()
                .corner_radius(10)
                .fill(if on { pal.accent_soft } else { pal.surface })
                .stroke(egui::Stroke::new(1.0, if on { pal.accent } else { pal.line }));
            if ui.add(button).clicked() {
                clicked = Some(n.clone());
            }
        }
    });
    clicked
}

/// Commas inside single quotes do not split, so that a fixed text such as ', ' in
/// Concatenate attributes stays one entry.
fn split_list(s: &str) -> Vec<String> {
    og_engine::nodes::split_quoted(s)
}

/// Changes the path of a reader or a writer, and what hangs on it. Returns true if
/// anything changed.
pub fn set_path(node: &mut GNode, value: &str) -> bool {
    let before = node.params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if value == before {
        return false;
    }
    if value.is_empty() {
        node.params.remove("path");
    } else {
        node.params.insert("path".to_string(), Json::String(value.to_string()));
    }
    if node.kind == "Reader" {
        // The layers that were chosen belong to the file that was there before.
        node.params.remove("layer");
    }
    if node.kind == "Writer" {
        follow_extension(node);
    }
    true
}

/// A writer that has been told a format follows the file name when the name is given
/// the extension of another format, so that the two never disagree.
fn follow_extension(node: &mut GNode) {
    let ext = ext_of(node.params.get("path").and_then(|v| v.as_str()).unwrap_or(""));
    let chosen = node.params.get("format").and_then(|v| v.as_str()).is_some_and(|f| !f.is_empty());
    if chosen && WRITER_FORMATS.iter().any(|f| !f.0.is_empty() && f.0 == ext) {
        node.params.insert("format".to_string(), Json::String(ext));
    }
}

/// Draws the form. Returns what changed.
pub fn node_form(ui: &mut Ui, node: &mut GNode, cx: &ParamCtx) -> Edited {
    let Some(ty) = node.ty() else {
        ui.colored_label(cx.pal.err, format!("The node type {} does not exist in this version.", node.kind));
        return Edited::default();
    };
    let mut changed = false;
    let mut noted = false;
    let mut postgis = false;
    let mut open_settings = false;
    let mut pick_area = false;
    let mut save_connection = false;
    let (mut open_tool, mut unpack_tool) = (false, false);
    ui.label(RichText::new("Name").small().color(cx.pal.muted));
    noted |= ui.add(TextEdit::singleline(&mut node.name).desired_width(f32::INFINITY)).changed();
    if node.name.trim().is_empty() {
        node.name = graph::title_of(&node.kind).to_string();
    }
    ui.add_space(2.0);
    ui.label(RichText::new("Comment").small().color(cx.pal.muted));
    let comment = TextEdit::multiline(&mut node.comment)
        .desired_width(f32::INFINITY)
        .desired_rows(2)
        .hint_text("Shown in a box next to the node. What it is for, or what to remember");
    noted |= ui.add(comment).changed();

    let folder = folder_of(cx);
    let layers: &[og_engine::io::LayerInfo] = cx.layers.and_then(|l| l.as_ref().ok()).map_or(&[], |l| l.as_slice());
    // The ticks are for one file. A folder holds many, with layers that may differ.
    // A drawing is one layer, entities, to GDAL: its CAD layers are in the attribute layer.
    let layers = if folder.is_some() || matches!(format_of(node, folder).as_str(), "dxf" | "dwg") { &[] } else { layers };
    // Where the paths are taken from only matters with a stream on Paths, which the latest run
    // tells, or once it has been set.
    let paths_in =
        || !(cx.fields)("Paths").is_empty() || node.params.get("path_attribute").and_then(|v| v.as_str()).is_some_and(|v| !v.is_empty() && v != "_file");
    let specs: Vec<&ParamSpec> = ty
        .params
        .iter()
        .filter(|s| relevant(node, s, folder, !layers.is_empty(), cx.base_dir))
        .filter(|s| !(node.kind == "Reader" && s.key == "path_attribute") || paths_in())
        .collect();
    let sheets = is_spreadsheet(&format_of(node, folder))
        || node.params.get("path").and_then(|v| v.as_str()).is_some_and(og_engine::gsheets::is_sheet)
        || node.params.get("format").and_then(|v| v.as_str()) == Some("gsheet");
    let what = if node.kind == "PostgisReader" {
        "tables"
    } else if sheets {
        "sheets"
    } else {
        "layers"
    };
    // What is seldom changed goes last, under More, which is open when any of it is set.
    let (main, more): (Vec<&ParamSpec>, Vec<&ParamSpec>) = specs.into_iter().partition(|s| !is_more(&node.kind, s.key));
    let more_id = egui::Id::new(("more-settings", node.id.clone()));
    let more_set = more.iter().filter(|s| is_set(node, s)).count();
    let mut more_open = ui.data(|d| d.get_temp::<bool>(more_id)).unwrap_or(more_set > 0);
    let more_keys: Vec<&str> = more.iter().map(|s| s.key).collect();
    let specs: Vec<&ParamSpec> = main.into_iter().chain(more).collect();
    let mut header_drawn = false;
    for spec in specs {
        if more_keys.contains(&spec.key) {
            if !header_drawn {
                header_drawn = true;
                ui.add_space(6.0);
                let arrow = if more_open { "▾" } else { "▸" };
                let count = if more_set > 0 { format!(" · {} set", more_set) } else { String::new() };
                let what = more_keys.iter().filter_map(|k| ty.params.iter().find(|p| p.key == *k)).map(|p| p.label).collect::<Vec<_>>().join(", ");
                let clicked = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // The arrow in the monospace font, which has it.
                        let a = ui.add(egui::Button::new(RichText::new(arrow).font(theme::mono(12.0)).color(cx.pal.muted)).frame(false));
                        let b = ui.add(egui::Button::new(RichText::new(format!("More{}", count)).strong()).frame(false)).on_hover_text(&what);
                        a.clicked() || b.clicked()
                    })
                    .inner;
                if clicked {
                    more_open = !more_open;
                    ui.data_mut(|d| d.insert_temp(more_id, more_open));
                }
            }
            if !more_open {
                continue;
            }
        }
        ui.add_space(2.0);
        let top = ui.cursor().min;
        // A tick box carries its own label. The help is on its name for the pointer.
        if !matches!(spec.kind, ParamKind::OptionalName(_)) {
            let name = ui.label(RichText::new(label_of(spec, sheets)).small().color(cx.pal.muted));
            if !spec.hint.is_empty() && !cx.hints_always {
                name.on_hover_text(spec.hint);
            }
        }
        let mut value = get(node, spec);
        let before = value.clone();
        let mut hint = spec.hint.to_string();
        match spec.kind {
            ParamKind::Files => {
                changed |= file_rows(ui, node, spec, cx.base_dir, cx.relative);
                value = before.clone();
            }
            ParamKind::Model => {
                let inner = og_engine::nodes::inner_model(&node.params);
                let count = |k: &str| inner.as_ref().map_or(0, |m| m.nodes.iter().filter(|n| n.kind == k).count());
                let all = inner.as_ref().map_or(0, |m| m.nodes.len());
                let (ins, outs) = (count("ToolInput"), count("ToolOutput"));
                let said = format!("{} nodes inside, {} coming in and {} going out", all - ins - outs, ins, outs);
                ui.label(RichText::new(said).small());
                ui.horizontal(|ui| {
                    open_tool |= ui
                        .button("Open")
                        .on_hover_text("Shows the model inside on the canvas, to change it. A double-click on the node, or Enter, does the same")
                        .clicked();
                    unpack_tool |= ui.button("Unpack").on_hover_text("Puts the nodes back in the model, in a box with the name of the tool").clicked();
                });
                hint.clear();
            }
            ParamKind::Secret => {
                // A variable such as $(password) is shown, since it says nothing secret.
                let plain = value.trim().starts_with("$(");
                ui.add(TextEdit::singleline(&mut value).password(!plain).desired_width(f32::INFINITY));
                if !plain && !value.trim().is_empty() {
                    ui.label(RichText::new("⚠ Written in the model, for anyone it is given to").small().color(cx.pal.warn));
                }
                if matches!(node.kind.as_str(), "PostgisReader" | "PostgisWriter") {
                    save_connection |= ui
                        .button("Save as a connection")
                        .on_hover_text(
                            "Moves the server, database, user and password to a connection in the settings, which this node then names. The password leaves the model",
                        )
                        .clicked();
                }
            }
            ParamKind::OptionalName(suggested) => {
                let mut on = !value.trim().is_empty();
                if ui.checkbox(&mut on, label_of(spec, sheets)).changed() {
                    value = if on {
                        if sheets {
                            suggested.replace("layer", "sheet")
                        } else {
                            suggested.to_string()
                        }
                    } else {
                        String::new()
                    };
                }
                if on {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label(RichText::new("named").small().color(cx.pal.muted));
                        ui.add(TextEdit::singleline(&mut value).desired_width(ui.available_width()));
                    });
                    // An empty name would switch it off. The tick box is for that.
                    if value.trim().is_empty() {
                        value = before.clone();
                    }
                } else {
                    hint.clear();
                }
            }
            ParamKind::Connection => {
                let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    open_settings |= ui.button("Settings…").on_hover_text("Add or change database connections").clicked();
                    let shown = if value.is_empty() { "Choose a connection".to_string() } else { value.clone() };
                    egui::ComboBox::from_id_salt(("connection", spec.key)).selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
                        for name in cx.db_connections {
                            ui.selectable_value(&mut value, name.clone(), name);
                        }
                    });
                });
                if !value.is_empty() && !cx.db_connections.iter().any(|c| c.trim() == value.trim()) {
                    hint = format!("There is no connection called {} on this computer. Add it under Settings, or choose another.", value);
                } else if value.is_empty() && cx.db_connections.is_empty() {
                    hint = "There are no connections yet. Click Settings… to add one.".to_string();
                }
            }
            ParamKind::ApiConnection => {
                let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    open_settings |= ui.button("Settings…").on_hover_text("Add or change API connections").clicked();
                    let shown = if value.is_empty() { "None".to_string() } else { value.clone() };
                    egui::ComboBox::from_id_salt(("api", spec.key)).selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
                        ui.selectable_value(&mut value, String::new(), "None");
                        for name in cx.api_connections {
                            ui.selectable_value(&mut value, name.clone(), name);
                        }
                    });
                });
                if !value.is_empty() && !cx.api_connections.iter().any(|c| c.trim() == value.trim()) {
                    hint = format!("There is no API connection called {} on this computer. Add it under Settings, or choose another.", value);
                }
            }
            // A colour, such as the colour of walls or a background: a swatch to pick it
            // from beside its code.
            ParamKind::Text if spec.key.ends_with("colour") || spec.key.ends_with("_color") => {
                ui.horizontal(|ui| {
                    let shown = if value.trim().is_empty() { spec.default } else { value.as_str() };
                    let mut c = parse_colour_code(shown).unwrap_or([128, 128, 128, 255]);
                    if colour_button(ui, &mut c) {
                        value = colour_code(c);
                    }
                    ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY).hint_text(spec.default));
                });
            }
            ParamKind::Text if spec.key == "bbox" && matches!(node.kind.as_str(), "OsmDownloader" | "OvertureDownloader" | "PostgisReader") => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY));
                pick_area |= ui
                    .button("Choose on the map")
                    .on_hover_text("Click two corners on the map under the canvas, and Enter. Nothing needs to be connected to Extent")
                    .clicked();
            }
            ParamKind::Text if spec.key == "table" && node.kind == "PostgisWriter" => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY));
                db_names(ui, &value, &(cx.fields)("Input"), &cx.pal);
            }
            ParamKind::Text if spec.key == "print_dpi" && node.kind == "MapRenderer" => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY));
                if let Some(line) = print_pixels(node.params.get("print_mm").and_then(|v| v.as_str()).unwrap_or("180"), &value) {
                    hint = line;
                }
            }
            ParamKind::Text if spec.key == "classes" && node.kind == "PointCloudFilter" => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY));
                class_ticks(ui, &mut value, &(cx.classes)("Points"), cx.ran_at.as_deref(), &cx.pal);
            }
            ParamKind::Text => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY));
            }
            ParamKind::Exact => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY).font(theme::mono(12.5)));
                // Spaces count here and cannot be seen in the field, so they are shown.
                if value.contains(' ') {
                    ui.label(RichText::new(format!("Spaces shown as dots: {}", value.replace(' ', "·"))).font(theme::mono(11.0)).color(cx.pal.muted));
                }
            }
            ParamKind::Picks(list) => {
                // A tick for each choice, kept as a list of the values ticked.
                let mut chosen = list_of(node, spec.key);
                if chosen.is_empty() && !spec.default.is_empty() && !node.params.contains_key(spec.key) {
                    chosen = split_list(spec.default);
                }
                let mut changed_here = false;
                for (v, label) in list {
                    let mut on = chosen.iter().any(|c| c == v);
                    if ui.checkbox(&mut on, *label).changed() {
                        changed_here = true;
                        if on {
                            chosen.push(v.to_string());
                        } else {
                            chosen.retain(|c| c != v);
                        }
                    }
                }
                if changed_here {
                    // In the order of the list.
                    let ordered: Vec<Json> = list.iter().filter(|(v, _)| chosen.iter().any(|c| c == v)).map(|(v, _)| Json::String(v.to_string())).collect();
                    node.params.insert(spec.key.to_string(), Json::Array(ordered));
                    changed = true;
                }
                value = before.clone();
            }
            ParamKind::Layers => {
                if layers.is_empty() {
                    ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY).font(theme::mono(12.5)));
                    if let Some(Err(e)) = cx.layers {
                        ui.label(RichText::new(e).small().color(cx.pal.err));
                    }
                } else {
                    changed |= layer_ticks(ui, node, spec.key, layers, what, &cx.pal);
                    value = before.clone();
                    hint = match (list_of(node, spec.key).is_empty(), node.kind == "PostgisReader") {
                        (true, true) => "Tick the tables to read.".to_string(),
                        (true, false) => format!("Nothing ticked reads the first one, {}.", layers[0].name),
                        _ => String::new(),
                    };
                }
            }
            ParamKind::Attribute(port) => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY).font(theme::mono(12.5)));
                let fields = (cx.fields)(port);
                if let Some(name) = chips(ui, &cx.pal, &fields, |n| n == value.trim()) {
                    value = name;
                }
                if fields.is_empty() {
                    hint = "Connect a stream and run, and the attribute can be picked here.".to_string();
                }
            }
            ParamKind::Source => {
                // The path gets the whole width. With the buttons beside it there is no
                // room to see where it leads.
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY));
                let stored = |p: &Path| if cx.relative { relative_to(cx.base_dir, p) } else { p.to_string_lossy().into_owned() };
                ui.horizontal(|ui| {
                    if ui.button("File…").clicked() {
                        let dialog = rfd::FileDialog::new()
                            .set_directory(cx.base_dir)
                            .add_filter(
                                "Spatial data and tables",
                                &[
                                    "gpkg",
                                    "shp",
                                    "geojson",
                                    "json",
                                    "fgb",
                                    "parquet",
                                    "sqlite",
                                    "db",
                                    "dxf",
                                    "dwg",
                                    "gpx",
                                    "gml",
                                    "kml",
                                    "kmz",
                                    "000",
                                    "bag",
                                    "h5",
                                    "xlsx",
                                    "ods",
                                    "csv",
                                    "txt",
                                    "png",
                                    "jpg",
                                    "tif",
                                    "asc",
                                    "hgt",
                                    "dt0",
                                    "dt1",
                                    "dt2",
                                    "xml",
                                    "pbf",
                                    "zip",
                                    "nc",
                                    "zgroup",
                                    "zmetadata",
                                ],
                            )
                            .add_filter("3D models, IFC and 3D Tiles", &["obj", "gltf", "glb", "ifc", "json", "b3dm", "stl", "3mf", "step", "stp"])
                            .add_filter("Point clouds and splats", &["las", "laz", "e57", "ply", "spz"])
                            .add_filter("All files", &["*"]);
                        if let Some(p) = dialog.pick_file() {
                            value = stored(&p);
                        }
                    }
                    if ui.button("Folder…").on_hover_text("Read all the files in a folder that match a filter, as one stream").clicked() {
                        if let Some(p) = rfd::FileDialog::new().set_directory(cx.base_dir).pick_folder() {
                            value = stored(&p);
                            // The filter starts as the kind of file there is most of in the folder.
                            match guess_pattern(&p).filter(|g| g != og_engine::nodes::DEFAULT_PATTERN) {
                                Some(g) => node.params.insert("pattern".to_string(), Json::String(g)),
                                None => node.params.remove("pattern"),
                            };
                        }
                    }
                    if ui.button("PostGIS…").on_hover_text("Read tables from a PostgreSQL database with PostGIS").clicked() {
                        postgis = true;
                    }
                });
                match cx.sources {
                    Some(Ok(sources)) => folder_summary(ui, sources, &cx.pal),
                    Some(Err(e)) if !before.trim().is_empty() => {
                        ui.label(RichText::new(e).small().color(cx.pal.err));
                    }
                    _ => {}
                }
            }
            ParamKind::Choice(list) => {
                let shown = list.iter().find(|c| c.0 == value).map_or(value.as_str(), |c| c.1).to_string();
                egui::ComboBox::from_id_salt(("choice", spec.key)).selected_text(shown).width(ui.available_width()).height(460.0).show_ui(ui, |ui| {
                    for (v, l) in list {
                        ui.selectable_value(&mut value, v.to_string(), *l);
                    }
                });
            }
            ParamKind::Crs => {
                let code = parse_crs(&value).ok();
                let known = code.and_then(|c| KNOWN_CRS.iter().find(|k| k.0 == c));
                let in_proj = code.and_then(|c| cx.crs_list.iter().find(|e| e.code == c));
                // What an empty choice means depends on the tool.
                let as_is = match node.kind.as_str() {
                    "PostgisWriter" => "As the stream is",
                    "LayeredPdfWriter" | "QgisProjectWriter" => "As the first layer",
                    "PostgisReader" => "As stated in the table",
                    _ => "As stated in the file",
                };
                let shown = match (known, in_proj, value.is_empty()) {
                    (Some(k), _, _) => format!("{} · EPSG:{}", k.1, k.0),
                    (None, Some(e), _) => format!("{} · EPSG:{}", e.name, e.code),
                    (None, None, true) => as_is.to_string(),
                    (None, None, false) => "Other EPSG code".to_string(),
                };
                let search_id = egui::Id::new(("crs-search", spec.key, node.id.clone()));
                let mut searching = ui.data(|d| d.get_temp::<bool>(search_id)).unwrap_or(false);
                egui::ComboBox::from_id_salt(("crs", spec.key)).selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
                    // The search first, so that it is seen without scrolling.
                    if !cx.crs_list.is_empty() && ui.selectable_label(searching, format!("Search all {}…", cx.crs_list.len())).clicked() {
                        searching = true;
                    }
                    if spec.default.is_empty() {
                        ui.selectable_value(&mut value, String::new(), as_is);
                    }
                    for k in KNOWN_CRS {
                        ui.selectable_value(&mut value, format!("EPSG:{}", k.0), format!("{} · EPSG:{}", k.1, k.0));
                    }
                    if ui.selectable_label(known.is_none() && in_proj.is_none() && !value.is_empty(), "Write an EPSG code…").clicked() {
                        value = "EPSG:".to_string();
                        searching = false;
                    }
                });
                if known.is_none() && in_proj.is_none() && !value.is_empty() {
                    ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY).font(theme::mono(12.5)));
                    if parse_crs(&value).is_err() {
                        hint = "Write the code as EPSG:3021.".to_string();
                    }
                }
                if searching {
                    // A search among every coordinate system of PROJ: by name, code or area.
                    let text_id = search_id.with("text");
                    let mut find = ui.data(|d| d.get_temp::<String>(text_id)).unwrap_or_default();
                    let field = ui.add(
                        TextEdit::singleline(&mut find).desired_width(f32::INFINITY).hint_text("Search: name, code or area, such as RT90, 3021 or Norway"),
                    );
                    if field.gained_focus() || find.is_empty() {
                        field.request_focus();
                    }
                    let words: Vec<String> = find.to_lowercase().split_whitespace().map(str::to_string).collect();
                    let hits: Vec<&og_engine::io::CrsEntry> = if words.is_empty() {
                        Vec::new()
                    } else {
                        cx.crs_list
                            .iter()
                            .filter(|e| {
                                let all = format!("{} epsg:{} {} {}", e.name, e.code, e.area, e.kind).to_lowercase();
                                words.iter().all(|w| all.contains(w.as_str()))
                            })
                            .take(40)
                            .collect()
                    };
                    egui::ScrollArea::vertical().id_salt(search_id.with("list")).max_height(180.0).show(ui, |ui| {
                        for e in &hits {
                            let kind = match e.kind {
                                "geographic" => " · degrees",
                                "compound" => " · with heights",
                                _ => "",
                            };
                            let label = format!("EPSG:{} · {}{}", e.code, e.name, kind);
                            if ui.selectable_label(false, label).on_hover_text(&e.area).clicked() {
                                value = format!("EPSG:{}", e.code);
                                searching = false;
                                find.clear();
                            }
                        }
                        if !words.is_empty() && hits.is_empty() {
                            ui.label(RichText::new("Nothing found.").color(cx.pal.muted));
                        }
                    });
                    if ui.small_button("Close the search").clicked() {
                        searching = false;
                    }
                    ui.data_mut(|d| d.insert_temp(text_id, find));
                }
                ui.data_mut(|d| d.insert_temp(search_id, searching));
            }
            ParamKind::Attributes(port) => {
                ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY).font(theme::mono(12.5)));
                let fields = (cx.fields)(port);
                let on = split_list(&value);
                if let Some(name) = chips(ui, &cx.pal, &fields, |n| on.iter().any(|o| o == n)) {
                    let mut list = on.clone();
                    match list.iter().position(|x| *x == name) {
                        Some(i) => {
                            list.remove(i);
                        }
                        None => list.push(name),
                    }
                    value = list.join(", ");
                }
                if fields.is_empty() {
                    hint = "Connect a stream and run, and the attributes can be picked here.".to_string();
                }
            }
            ParamKind::Code(port) => {
                ui.add(TextEdit::multiline(&mut value).desired_width(f32::INFINITY).desired_rows(5).font(theme::mono(12.5)).code_editor());
                let fields = (cx.fields)(port);
                if let Some(name) = chips(ui, &cx.pal, &fields, |_| false) {
                    value.push_str(&format!("{{{}}}", quoted_field(&name)));
                }
                if !fields.is_empty() && !value.trim().is_empty() {
                    if let Err(e) = og_engine::nodes::SqlTemplate::parse(&value, &fields) {
                        ui.colored_label(cx.pal.err, RichText::new(e.0).small());
                    }
                }
            }
            ParamKind::Expression(port) => {
                let fields = (cx.fields)(port);
                let id = ui.make_persistent_id(("expression", node.id.as_str(), spec.key));
                crate::suggest::expression_field(ui, &cx.pal, &mut value, &fields, cx.suggest, id, &quoted_field, &|a| (cx.values)(port, a));
                if let Some(name) = chips(ui, &cx.pal, &fields, |_| false) {
                    if !value.is_empty() && !value.ends_with(' ') && !value.ends_with('(') {
                        value.push(' ');
                    }
                    value.push_str(&quoted_field(&name));
                }
                if !fields.is_empty() && !value.trim().is_empty() {
                    let checked = og_engine::run::expand_variables(&value, cx.variables).and_then(|v| expr::compile(&v, &fields).map(|_| ()));
                    // An address written without quotes is taken as it is by HTTP request.
                    let plain_address = node.kind == "HttpCaller" && (value.trim().starts_with("http://") || value.trim().starts_with("https://"));
                    match (checked, plain_address) {
                        (Err(e), false) => {
                            ui.colored_label(cx.pal.err, RichText::new(e.0).small());
                        }
                        (Ok(()), _) => {
                            if let Some(shown) = (cx.preview)(port, &value) {
                                ui.label(RichText::new(shown).small().color(cx.pal.muted));
                            }
                        }
                        _ => {}
                    }
                }
                // The functions and variables, each with a line of help. A click puts it in.
                egui::CollapsingHeader::new(RichText::new("Functions and variables").small()).id_salt((spec.key, "functions")).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let put = |ui: &mut Ui, name: String, help: &str, value: &mut String| {
                        let r = ui.add(egui::Label::new(RichText::new(help).small()).sense(egui::Sense::click()));
                        if r.on_hover_text("Click to put it in the expression").clicked() {
                            if !value.is_empty() && !value.ends_with(' ') && !value.ends_with('(') {
                                value.push(' ');
                            }
                            value.push_str(&name);
                        }
                    };
                    egui::ScrollArea::vertical().max_height(220.0).id_salt((spec.key, "function list")).show(ui, |ui| {
                        for help in expr::variable_help() {
                            let name = help.split_whitespace().next().unwrap_or("").trim_end_matches(',').to_string();
                            put(ui, name, help, &mut value);
                        }
                        ui.add_space(4.0);
                        ui.label(RichText::new("CASE WHEN condition THEN value ELSE value END chooses a value; ^ raises to a power").small());
                        ui.add_space(4.0);
                        for (name, help) in expr::function_names().into_iter().zip(expr::function_help()) {
                            put(ui, format!("{}(", name), help, &mut value);
                        }
                    });
                });
            }
            ParamKind::FileOpen | ParamKind::FileSave => {
                let save = matches!(spec.kind, ParamKind::FileSave);
                // The button is laid out first, from the right, and the text field gets the
                // rest. Otherwise the row becomes wider than the panel and the panel grows.
                let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let browse = ui.button("Browse…").clicked();
                    ui.add(TextEdit::singleline(&mut value).desired_width(ui.available_width()));
                    if browse {
                        let mut dialog = rfd::FileDialog::new().set_directory(cx.base_dir);
                        dialog = if node.kind == "ModelRunner" {
                            dialog.add_filter("Ographis model", &["ographis", "json"])
                        } else {
                            dialog.add_filter(
                                "Spatial data and tables",
                                &["gpkg", "shp", "geojson", "json", "xlsx", "ods", "csv", "txt", "png", "jpg", "tif", "pdf"],
                            )
                        }
                        .add_filter("All files", &["*"]);
                        if !value.is_empty() {
                            if let Some(name) = Path::new(&value).file_name() {
                                dialog = dialog.set_file_name(name.to_string_lossy());
                            }
                        }
                        let picked: Option<PathBuf> = if save { dialog.save_file() } else { dialog.pick_file() };
                        if let Some(p) = picked {
                            value = if cx.relative { relative_to(cx.base_dir, &p) } else { p.to_string_lossy().into_owned() };
                        }
                    }
                });
            }
            ParamKind::Renames(port) => {
                changed |= renames(ui, node, spec, &(cx.fields)(port), &cx.pal);
                value = before.clone();
            }
            ParamKind::SortKeys(port) => {
                changed |= sort_rows(ui, node, spec, &(cx.fields)(port), &cx.pal);
                value = before.clone();
            }
            ParamKind::Ranges(_) => {
                changed |= range_rows(ui, node, spec, &cx.pal);
                value = before.clone();
            }
            ParamKind::Checks(port) => {
                changed |= check_rows(ui, node, spec, &(cx.fields)(port), &cx.pal);
                value = before.clone();
            }
            ParamKind::Schema(port) => {
                changed |= schema(ui, node, spec, &(cx.field_types)(port), &cx.pal);
                value = before.clone();
            }
            ParamKind::AttributeTable(port) => {
                changed |= attribute_table(ui, node, spec, &(cx.field_types)(port), &cx.pal);
                value = before.clone();
            }
            ParamKind::NewAttributes(port) => {
                changed |= new_attribute_rows(ui, node, spec, &(cx.fields)(port), &cx.pal);
                value = before.clone();
            }
            ParamKind::MapLayers(port) => {
                changed |= map_layer_rows(ui, node, spec, &(cx.connections)(port), &cx.pal, cx.stream_fields, cx.stream_raster);
                value = before.clone();
            }
            ParamKind::Values(port, attr_key) => {
                let attr = node.params.get(attr_key).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let found = if attr.is_empty() { Vec::new() } else { (cx.values)(port, &attr) };
                changed |= value_rows(ui, node, spec, &attr, &found, &cx.pal);
                value = before.clone();
            }
            ParamKind::Mapping(port, attr_key) => {
                let attr = node.params.get(attr_key).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let found = if attr.is_empty() { Vec::new() } else { (cx.values)(port, &attr) };
                changed |= mapping_rows(ui, node, spec, &attr, &found, &cx.pal);
                value = before.clone();
            }
            ParamKind::JsonPaths(port, attr_key) => {
                let attr = node.params.get(attr_key).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let found = if attr.is_empty() { Vec::new() } else { (cx.values)(port, &attr) };
                changed |= json_path_rows(ui, node, spec, &found, &cx.pal);
                value = before.clone();
            }
        }
        // The help of a setting is shown when the pointer rests on it, so that the form does not
        // move under the pointer. A message about what is there now, such as a connection that
        // is missing, is always shown.
        if !hint.is_empty() && (cx.hints_always || hint != spec.hint) {
            ui.label(RichText::new(hint).small().color(cx.pal.muted));
        } else if !hint.is_empty() {
            let field = egui::Rect::from_min_max(top, egui::pos2(ui.max_rect().right(), ui.cursor().min.y));
            ui.interact(field, egui::Id::new(("help", node.id.clone(), spec.key)), egui::Sense::hover()).on_hover_text(hint);
        }
        if value != before {
            // A newly chosen file gives the node the name of the file, unless the user has already named it.
            if matches!(spec.kind, ParamKind::FileOpen | ParamKind::FileSave | ParamKind::Source) && graph::has_default_name(node) {
                if let Some(stem) = Path::new(&value).file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty() && !s.contains('*')) {
                    node.name = stem;
                }
            }
            // The layers that were chosen belong to the file that was there before.
            if matches!(spec.kind, ParamKind::Source) {
                node.params.remove("layer");
            }
            set(node, spec, value);
            if node.kind == "Writer" && spec.key == "path" {
                follow_extension(node);
            }
            // A newly chosen format gives the file name its extension, so that the name
            // always says what the file is.
            if node.kind == "Writer" && spec.key == "format" {
                let format = node.params.get("format").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let path = node.params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if !format.is_empty() && format != "postgis" && !path.is_empty() {
                    node.params.insert("path".to_string(), Json::String(writer_file(&path, Some(&format))));
                }
            }
            changed = true;
        }
    }
    Edited { any: changed || noted, run: changed, postgis, open_settings, pick_area, save_connection, open_tool, unpack_tool }
}

/// Settings that are seldom changed, shown under More at the end of the form.
fn is_more(kind: &str, key: &str) -> bool {
    match kind {
        "PostgisWriter" => {
            matches!(key, "commit" | "commit_rows" | "geom_column" | "key" | "key_name" | "key_attribute" | "sql_before" | "sql_after" | "analyze")
        }
        "PostgisReader" => matches!(key, "crs" | "limit"),
        "Writer" => matches!(
            key,
            "key"
                | "key_name"
                | "key_attribute"
                | "gpkg_version"
                | "geojson_crs"
                | "img_quality"
                | "tif_compress"
                | "pdf_compress"
                | "pdf_quality"
                | "pdf_as"
                | "t3_most"
                | "t3_dz"
        ),
        "MapRenderer" => matches!(key, "margin" | "background_fade" | "grid_every" | "overview_size" | "overview_zoom"),
        "SceneRenderer" => matches!(key, "tree_height" | "tree_height_by" | "tree_kind" | "tree_kind_by"),
        _ => false,
    }
}

/// A setting given a value of its own, other than the default.
fn is_set(node: &GNode, spec: &ParamSpec) -> bool {
    match node.params.get(spec.key) {
        None | Some(Json::Null) => false,
        Some(Json::String(v)) => !v.trim().is_empty() && v.trim() != spec.default,
        Some(Json::Array(a)) => !a.is_empty(),
        Some(_) => true,
    }
}

/// The pixels a map for print gets: the width on paper at the resolution, as Draw map
/// counts it, or why it cannot be drawn.
pub fn print_pixels(mm: &str, dpi: &str) -> Option<String> {
    let mm: f64 = og_engine::model::parse_number(if mm.trim().is_empty() { "180" } else { mm })?;
    let dpi: f64 = og_engine::model::parse_number(if dpi.trim().is_empty() { "300" } else { dpi })?;
    if mm <= 0.0 || dpi <= 0.0 {
        return None;
    }
    let w = (mm / 25.4 * dpi).round();
    let most = og_engine::mapdraw::MAX_SIDE as f64;
    Some(if w > most {
        format!(
            "{} pixels wide, more than the {} a map can be. Make it narrower or lower the resolution",
            og_engine::model::fmt_count(w as u64),
            og_engine::model::fmt_count(most as u64)
        )
    } else {
        format!(
            "{} mm at {} to the inch gives {} pixels in width; the height follows from the area drawn",
            og_engine::model::fmt_real(mm),
            og_engine::model::fmt_real(dpi),
            og_engine::model::fmt_count(w as u64)
        )
    })
}

/// The names the table and its columns get in the database, where they differ from those
/// given, and those that are too long for PostgreSQL, which cuts names at 63 bytes.
fn db_names(ui: &mut Ui, table: &str, fields: &[String], pal: &Palette) {
    let mut lines: Vec<(String, bool)> = Vec::new();
    let show = |given: &str, lines: &mut Vec<(String, bool)>| {
        let made = og_engine::io::db_name(given);
        if made.len() > 63 {
            lines.push((format!("{} is longer than the 63 characters PostgreSQL keeps of a name", made), true));
        } else if made != given.trim() {
            lines.push((format!("{} → {}", given.trim(), made), false));
        }
    };
    if !table.trim().is_empty() {
        show(table, &mut lines);
    }
    for f in fields {
        show(f, &mut lines);
    }
    if lines.is_empty() {
        return;
    }
    ui.label(RichText::new("Named in the database as").small().color(pal.muted));
    for (line, bad) in lines.iter().take(12) {
        ui.label(RichText::new(line).font(theme::mono(11.0)).color(if *bad { pal.warn } else { pal.muted }));
    }
    if lines.len() > 12 {
        ui.label(RichText::new(format!("and {} more", lines.len() - 12)).small().color(pal.muted));
    }
}

/// A tick for each class the points of the latest run have, with how many, that writes the
/// classes ticked into the field.
fn class_ticks(ui: &mut Ui, value: &mut String, found: &[(i64, u64)], ran: Option<&str>, pal: &Palette) {
    if found.is_empty() {
        ui.label(RichText::new("Run the model, and the classes in the data can be ticked here.").small().color(pal.muted));
        return;
    }
    let mut chosen: Vec<i64> = value.split([',', ';', ' ']).filter_map(|c| c.trim().parse().ok()).collect();
    let before = chosen.clone();
    for (c, n) in found {
        let mut on = chosen.contains(c);
        let label = format!("{} · {}", og_engine::nodes::class_port(*c), og_engine::model::fmt_count(*n));
        if ui.checkbox(&mut on, label).changed() {
            if on {
                chosen.push(*c);
            } else {
                chosen.retain(|x| x != c);
            }
        }
    }
    if let Some(at) = ran {
        // The classes are those of the run that is shown; new data needs a new run.
        ui.label(RichText::new(format!("From the run at {}", at)).small().color(pal.muted))
            .on_hover_text("Run again after the data has changed, and the classes found then are shown");
    }
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Tick all").small()).clicked() {
            chosen = found.iter().map(|(c, _)| *c).collect();
        }
        if ui.add_enabled(!chosen.is_empty(), egui::Button::new("None").small()).clicked() {
            chosen.clear();
        }
    });
    if chosen != before {
        *value = chosen.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn colour_codes_as_in_qgis() {
        assert_eq!(super::parse_colour_code("#80d3ff"), Some([0x80, 0xd3, 0xff, 255]));
        assert_eq!(super::parse_colour_code("80D3FF80"), Some([0x80, 0xd3, 0xff, 0x80]));
        assert_eq!(super::parse_colour_code("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(super::parse_colour_code("#80d3f"), None);
        assert_eq!(super::colour_code([0x80, 0xd3, 0xff, 255]), "#80d3ff");
    }

    use super::*;

    #[test]
    fn field_names_in_expressions() {
        assert_eq!(quoted_field("population"), "population");
        assert_eq!(quoted_field("län"), "län");
        assert_eq!(quoted_field("area m2"), "\"area m2\"");
        assert_eq!(quoted_field("2024"), "\"2024\"");
        assert_eq!(quoted_field("in"), "\"in\"");
    }

    #[test]
    fn format_specific_fields() {
        let ty = og_engine::node_type("Reader").unwrap();
        let spec = |key: &str| ty.params.iter().find(|s| s.key == key).unwrap();
        let mut n = GNode {
            id: "a".into(),
            kind: "Reader".into(),
            name: "Reader".into(),
            params: Default::default(),
            comment: String::new(),
            comment_box: None,
            disabled: false,
            off_wires: Vec::new(),
            bends: Vec::new(),
        };
        let shown = |n: &GNode, key: &str| relevant(n, spec(key), None, false, Path::new(""));
        assert!(!shown(&n, "x") && !shown(&n, "layer") && shown(&n, "path") && !shown(&n, "pattern") && !shown(&n, "header"));
        n.params.insert("path".into(), Json::String("data/towns.CSV".into()));
        assert!(shown(&n, "x") && shown(&n, "encoding") && shown(&n, "crs") && !shown(&n, "layer"));
        n.params.insert("path".into(), Json::String("areas.gpkg".into()));
        assert!(!shown(&n, "x") && shown(&n, "layer") && shown(&n, "layer_attr") && !shown(&n, "encoding"));
        assert!(!shown(&n, "zip_file") && !shown(&n, "vt_zoom"));
        // A zip file, also on the web, says which file in it; vector tiles their zoom level.
        n.params.insert("path".into(), Json::String("https://example.org/data.zip?x=1".into()));
        assert!(shown(&n, "zip_file") && !shown(&n, "vt_zoom"));
        n.params.insert("path".into(), Json::String("roads.pmtiles".into()));
        assert!(shown(&n, "vt_zoom") && shown(&n, "vt_pieces") && !shown(&n, "zip_file"));
        n.params.insert("path".into(), Json::String("https://tiles.example.org/{z}/{x}/{y}.pbf".into()));
        assert!(shown(&n, "vt_zoom") && shown(&n, "tiles_most") && shown(&n, "within") && !shown(&n, "crs") && !shown(&n, "x"));
        // A workbook has sheets and a first row, and no coordinate system.
        n.params.insert("path".into(), Json::String("book.XLSX".into()));
        assert!(shown(&n, "layer") && shown(&n, "header") && !shown(&n, "crs") && !shown(&n, "x"));
        // What the file name is written as is only asked for when it is written at all.
        assert!(shown(&n, "filename_attr") && !shown(&n, "filename_part"));
        n.params.insert("filename_attr".into(), Json::String("source".into()));
        assert!(shown(&n, "filename_part"));

        // A folder has a filter, and the filter says what kind of files they are.
        n.params.insert("path".into(), Json::String("data".into()));
        n.params.insert("pattern".into(), Json::String("*.csv".into()));
        let folder = Sources { files: Vec::new(), folder: Some((PathBuf::from("data"), vec!["*.csv".to_string()])), url: None };
        let shown = |n: &GNode, key: &str| relevant(n, spec(key), Some(&folder), false, Path::new(""));
        assert!(shown(&n, "pattern") && shown(&n, "recursive") && shown(&n, "x") && !shown(&n, "layer"));
        // A path with a * in it is its own filter.
        n.params.insert("path".into(), Json::String("data/roads_*.shp".into()));
        let folder = Sources { files: Vec::new(), folder: Some((PathBuf::from("data"), vec!["roads_*.shp".to_string()])), url: None };
        assert!(!relevant(&n, spec("pattern"), Some(&folder), false, Path::new("")) && relevant(&n, spec("encoding"), Some(&folder), false, Path::new("")));

        // A writer to a workbook chooses a sheet and how the geometry is written.
        let ty = og_engine::node_type("Writer").unwrap();
        let spec = |key: &str| ty.params.iter().find(|s| s.key == key).unwrap();
        let mut w = GNode {
            id: "w".into(),
            kind: "Writer".into(),
            name: "Writer".into(),
            params: Default::default(),
            comment: String::new(),
            comment_box: None,
            disabled: false,
            off_wires: Vec::new(),
            bends: Vec::new(),
        };
        w.params.insert("path".into(), Json::String("out/result.xlsx".into()));
        assert!(
            relevant(&w, spec("layer"), None, false, Path::new(""))
                && relevant(&w, spec("geometry"), None, false, Path::new(""))
                && !relevant(&w, spec("encoding"), None, false, Path::new(""))
        );
    }
}
