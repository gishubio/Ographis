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
            ("SceneRenderer", "water_level" | "waves") => param("water").is_some_and(|w| w != "none"),
            ("PostgisWriter", "key_name") => param("key").as_deref() != Some("attribute"),
            ("PostgisWriter", "key_attribute") => param("key").as_deref() == Some("attribute"),
            ("LayeredPdfWriter", "pdf_width" | "pdf_height") => param("pdf_size").as_deref() == Some("custom"),
            ("SceneRenderer", "focus") => param("blur").is_some_and(|b| b.trim().parse::<f64>().is_ok_and(|v| v > 0.0)),
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
    let sheets = is_spreadsheet(&ext);
    let layered = has_layers || sheets || matches!(ext.as_str(), "gpkg" | "gdb" | "sqlite" | "db");
    let filled = |key: &str| node.params.get(key).and_then(|v| v.as_str()).is_some_and(|v| !v.trim().is_empty());
    // A picture has neither attributes nor anything to add to.
    let image = writer && og_engine::nodes::is_image_format(&ext);
    let full = base.join(path);
    let tiles_file = matches!(ext.as_str(), "mbtiles" | "pmtiles") || (reader && og_engine::nodes::vector_tiles::is_tile_folder(&full));
    match spec.key {
        "vt_zoom" | "vt_pieces" => reader && tiles_file,
        "zip_file" => reader && (path.to_lowercase().ends_with(".zip") || og_engine::nodes::source_ext(path) == "zip"),
        "mode" | "fields" => !image && !matches!(ext.as_str(), "obj" | "glb"),
        "pdf_size" | "pdf_orientation" | "pdf_margin" | "pdf_title" | "pdf_compress" => writer && ext == "pdf",
        "pdf_width" | "pdf_height" => writer && ext == "pdf" && node.params.get("pdf_size").and_then(|v| v.as_str()) == Some("custom"),
        "pdf_dpi" => writer && ext == "pdf" && node.params.get("pdf_size").and_then(|v| v.as_str()) == Some("fit"),
        "pdf_quality" => writer && ext == "pdf" && node.params.get("pdf_compress").and_then(|v| v.as_str()) == Some("jpeg"),
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
        "las_classes" | "las_every" | "las_spacing" => reader && matches!(ext.as_str(), "las" | "laz"),
        "obj_unit" | "obj_up" => reader && matches!(ext.as_str(), "obj" | "gltf" | "glb"),
        "obj_east" | "obj_north" | "obj_height" | "obj_rotation" => reader && matches!(ext.as_str(), "obj" | "gltf" | "glb" | "ifc"),
        "ifc_openings" | "ifc_spaces" | "ifc_properties" => reader && ext == "ifc",
        "dxf_blocks" | "dxf_closed" => reader && matches!(ext.as_str(), "dxf" | "dwg"),
        "city_lod" => {
            reader && matches!(ext.as_str(), "gml" | "xml" | "json" | "jsonl") && og_engine::citymodel::is_city_path(&base.join(path).to_string_lossy())
        }
        // A file on the web may say what it is only when it has been fetched.
        "json_records" | "json_nested" => reader && (matches!(ext.as_str(), "json" | "ndjson" | "jsonl") || web),
        "cell_size" => reader && og_engine::io::is_raster(&ext) && node.params.get("raster_as").and_then(|v| v.as_str()) == Some("grid"),
        "layer" => layered,
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

fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
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
    let specs: Vec<&ParamSpec> = ty.params.iter().filter(|s| relevant(node, s, folder, !layers.is_empty(), cx.base_dir)).collect();
    let sheets = is_spreadsheet(&format_of(node, folder));
    let what = if node.kind == "PostgisReader" {
        "tables"
    } else if sheets {
        "sheets"
    } else {
        "layers"
    };
    for spec in specs {
        ui.add_space(2.0);
        // A tick box carries its own label.
        if !matches!(spec.kind, ParamKind::OptionalName(_)) {
            ui.label(RichText::new(label_of(spec, sheets)).small().color(cx.pal.muted));
        }
        let mut value = get(node, spec);
        let before = value.clone();
        let mut hint = spec.hint.to_string();
        match spec.kind {
            ParamKind::Secret => {
                // A variable such as $(password) is shown, since it says nothing secret.
                let plain = value.trim().starts_with("$(");
                ui.add(TextEdit::singleline(&mut value).password(!plain).desired_width(f32::INFINITY));
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
                                    "gpkg", "shp", "geojson", "json", "fgb", "parquet", "sqlite", "db", "dxf", "dwg", "gml", "xlsx", "ods", "csv", "txt",
                                    "png", "jpg", "tif", "pbf", "zip",
                                ],
                            )
                            .add_filter("3D models, IFC and 3D Tiles", &["obj", "gltf", "glb", "ifc", "json", "b3dm"])
                            .add_filter("Point clouds", &["las", "laz"])
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
                    "LayeredPdfWriter" => "As the first layer",
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
                crate::suggest::expression_field(ui, &cx.pal, &mut value, &fields, cx.suggest, id, &quoted_field);
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
        if !hint.is_empty() {
            ui.label(RichText::new(hint).small().color(cx.pal.muted));
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
    Edited { any: changed || noted, run: changed, postgis, open_settings }
}

/// The type and size of single attributes, as a row for each attribute that comes in.
/// Only the rows that say something are stored, as a JSON object by attribute name.
fn schema(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[(String, &'static str)], pal: &Palette) -> bool {
    const TYPES: [(&str, &str); 4] = [("", "As it is"), ("text", "Text"), ("integer", "Integer"), ("decimal", "Decimal")];
    let stored: serde_json::Map<String, Json> = match node.params.get(spec.key) {
        Some(Json::Object(o)) => o.clone(),
        _ => Default::default(),
    };
    let text = |name: &str, key: &str| match stored.get(name).and_then(|r| r.get(key)) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    // The attributes of the stream, and after them any that have settings but are gone.
    let mut names: Vec<(String, &str)> = fields.to_vec();
    for name in stored.keys() {
        if !names.iter().any(|f| &f.0 == name) {
            names.push((name.clone(), "not in the stream"));
        }
    }
    if names.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes are listed here.").small().color(pal.muted));
        return false;
    }
    let mut after = serde_json::Map::new();
    egui::Grid::new(("schema", spec.key)).num_columns(3).spacing(egui::vec2(6.0, 3.0)).show(ui, |ui| {
        for heading in ["Attribute", "Write as", "Length / decimals"] {
            ui.label(RichText::new(heading).small().color(pal.muted));
        }
        ui.end_row();
        for (name, has) in &names {
            let (mut ty, mut length, mut decimals) = (text(name, "type"), text(name, "length"), text(name, "decimals"));
            ui.add(egui::Label::new(RichText::new(name).font(theme::mono(11.5))).truncate()).on_hover_text(format!("{} ({})", name, has));
            let shown = match TYPES.iter().find(|t| t.0 == ty) {
                Some(("", _)) | None => format!("As it is ({})", has),
                Some((_, label)) => label.to_string(),
            };
            egui::ComboBox::from_id_salt(("schema-type", spec.key, name)).selected_text(shown).width(106.0).show_ui(ui, |ui| {
                for (value, label) in TYPES {
                    ui.selectable_value(&mut ty, value.to_string(), label);
                }
            });
            // What the attribute will be written as decides which size makes sense.
            let becomes = if ty.is_empty() { *has } else { ty.as_str() };
            match becomes {
                "text" => {
                    ui.add(TextEdit::singleline(&mut length).desired_width(44.0).hint_text("any"))
                        .on_hover_text("The largest number of characters. Longer text is cut");
                    decimals.clear();
                }
                "decimal" => {
                    ui.add(TextEdit::singleline(&mut decimals).desired_width(44.0).hint_text("all"))
                        .on_hover_text("The number of decimals. The values are rounded");
                    length.clear();
                }
                _ => {
                    ui.label("");
                    length.clear();
                    decimals.clear();
                }
            }
            ui.end_row();
            let mut rule = serde_json::Map::new();
            if !ty.is_empty() {
                rule.insert("type".to_string(), Json::String(ty));
            }
            for (key, value) in [("length", &length), ("decimals", &decimals)] {
                if let Ok(n) = value.trim().parse::<u32>() {
                    rule.insert(key.to_string(), Json::Number(n.into()));
                }
            }
            if !rule.is_empty() {
                after.insert(name.clone(), Json::Object(rule));
            }
        }
    });
    if after == stored {
        return false;
    }
    if after.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Object(after));
    }
    true
}

/// An attribute name that can be written, or picked from those that come in.
fn attr_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, name: &mut String, fields: &[String], width: f32) {
    ui.spacing_mut().item_spacing.x = 2.0;
    let w = if fields.is_empty() { width } else { (width - 24.0).max(40.0) };
    ui.add(TextEdit::singleline(name).id_salt(("attr-name", &id)).desired_width(w).hint_text("attribute"));
    if !fields.is_empty() {
        egui::ComboBox::from_id_salt(("attr-pick", &id)).selected_text("").width(14.0).show_ui(ui, |ui| {
            for f in fields {
                ui.selectable_value(name, f.clone(), f);
            }
        });
    }
}

/// Sort keys: rows of an attribute and an order, with arrows to change which comes first.
/// Stored as a JSON list of [attribute, order]. sort_by and order from before 0.61 become rows.
fn sort_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    let mut rows: Vec<(String, bool)> = og_engine::nodes::sort_keys(&node.params);
    let before = rows.clone();
    let mut remove = None;
    let mut swap = None;
    let n = rows.len();
    for (i, (name, down)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 150.0).max(60.0);
            attr_picker(ui, ("sort-key", i), name, fields, w);
            egui::ComboBox::from_id_salt(("sort-dir", i)).selected_text(if *down { "Descending" } else { "Ascending" }).width(86.0).show_ui(ui, |ui| {
                ui.selectable_value(down, false, "Ascending");
                ui.selectable_value(down, true, "Descending");
            });
            if symbol(ui, "▲", i > 0, "Sort by this before the one above") {
                swap = Some(i - 1);
            }
            if symbol(ui, "▼", i + 1 < n, "Sort by this after the one below") {
                swap = Some(i);
            }
            if symbol(ui, "×", true, "Remove") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = swap {
        rows.swap(i, i + 1);
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    let free = fields.iter().find(|f| !rows.iter().any(|r| &r.0 == *f));
    if ui.add_enabled(free.is_some() || fields.is_empty(), egui::Button::new("Add attribute").small()).clicked() {
        rows.push((free.cloned().unwrap_or_default(), false));
    }
    if fields.is_empty() && rows.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes can be picked here.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    // Rows from sort_by and order, as models before 0.61 have them, are now stored as rows.
    node.params.remove("sort_by");
    node.params.remove("order");
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        let list = rows.into_iter().map(|(a, d)| Json::Array(vec![Json::String(a), Json::String(if d { "descending" } else { "ascending" }.into())])).collect();
        node.params.insert(spec.key.to_string(), Json::Array(list));
    }
    true
}

/// Classify by range: rows of from, to and the value. Stored as a JSON list of [from, to, value].
fn range_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, pal: &Palette) -> bool {
    let text = |v: &Json| match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let mut rows: Vec<[String; 3]> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a
            .iter()
            .filter_map(|r| {
                r.as_array().map(|r| [r.first().map(text).unwrap_or_default(), r.get(1).map(text).unwrap_or_default(), r.get(2).map(text).unwrap_or_default()])
            })
            .collect(),
        _ => Vec::new(),
    };
    let before = rows.clone();
    let mut remove = None;
    if !rows.is_empty() {
        ui.horizontal(|ui| {
            for (head, w) in [("from", 64.0), ("to", 64.0), ("gives", 120.0)] {
                ui.add_sized([w, 14.0], egui::Label::new(RichText::new(head).small().color(pal.muted)));
            }
        });
    }
    for (i, row) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut row[0]).id_salt(("range-from", i)).desired_width(60.0).hint_text("…"));
            ui.add(TextEdit::singleline(&mut row[1]).id_salt(("range-to", i)).desired_width(60.0).hint_text("…"));
            ui.add(TextEdit::singleline(&mut row[2]).id_salt(("range-value", i)).desired_width((ui.available_width() - 30.0).max(60.0)).hint_text("value"));
            if symbol(ui, "×", true, "Remove this range") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.add(egui::Button::new("Add range").small()).clicked() {
        // The next range starts after the last one ends, when that is a whole number.
        let from = rows.last().and_then(|r| r[1].trim().parse::<i64>().ok()).map(|t| (t + 1).to_string()).unwrap_or_default();
        rows.push([from, String::new(), String::new()]);
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(rows.into_iter().map(|r| Json::Array(r.into_iter().map(Json::String).collect())).collect()));
    }
    true
}

/// Check attributes: a row for each attribute, with what it must be. Stored as a JSON list
/// of objects.
fn check_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    const KINDS: [(&str, &str); 5] = [("", "Any type"), ("integer", "Whole number"), ("decimal", "Number"), ("text", "Text"), ("date", "Date")];
    let mut rows: Vec<serde_json::Map<String, Json>> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a.iter().filter_map(|r| r.as_object().cloned()).collect(),
        _ => Vec::new(),
    };
    let before = rows.clone();
    let mut remove = None;
    let text = |o: &serde_json::Map<String, Json>, k: &str| match o.get(k) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    for (i, row) in rows.iter_mut().enumerate() {
        egui::Frame::new().stroke(egui::Stroke::new(1.0, pal.line)).corner_radius(4).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut name = text(row, "attribute");
                attr_picker(ui, ("check-attr", i), &mut name, fields, (ui.available_width() - 40.0).max(80.0));
                row.insert("attribute".into(), Json::String(name));
                if symbol(ui, "×", true, "Remove this check") {
                    remove = Some(i);
                }
            });
            ui.horizontal(|ui| {
                let mut required = row.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
                if ui.checkbox(&mut required, "Must have a value").changed() {
                    row.insert("required".into(), Json::Bool(required));
                }
                let mut kind = text(row, "type");
                let shown = KINDS.iter().find(|k| k.0 == kind).map_or("Any type", |k| k.1);
                egui::ComboBox::from_id_salt(("check-type", i)).selected_text(shown).width(110.0).show_ui(ui, |ui| {
                    for (v, l) in KINDS {
                        ui.selectable_value(&mut kind, v.to_string(), l);
                    }
                });
                row.insert("type".into(), Json::String(kind));
            });
            let mut field = |ui: &mut Ui, key: &str, label: &str, hint: &str, width: f32| {
                ui.label(RichText::new(label).small().color(pal.muted));
                let mut v = text(row, key);
                if ui.add(TextEdit::singleline(&mut v).id_salt(("check", key, i)).desired_width(width).hint_text(hint)).changed() {
                    row.insert(key.into(), Json::String(v));
                }
            };
            ui.horizontal(|ui| {
                field(ui, "max_length", "Longest", "chars", 46.0);
                field(ui, "min", "Smallest", "", 56.0);
                field(ui, "max", "Largest", "", 56.0);
            });
            ui.horizontal(|ui| {
                field(ui, "values", "One of", "A, B, C", (ui.available_width() - 50.0).max(80.0));
            });
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    let free = fields.iter().find(|f| !rows.iter().any(|r| text(r, "attribute") == **f));
    if ui.add_enabled(free.is_some() || fields.is_empty(), egui::Button::new("Add a check").small()).clicked() {
        let mut row = serde_json::Map::new();
        row.insert("attribute".into(), Json::String(free.cloned().unwrap_or_default()));
        row.insert("required".into(), Json::Bool(true));
        rows.push(row);
    }
    if fields.is_empty() && rows.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes can be picked here.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    // Empty settings are left out, so that the file only says what is checked.
    let clean: Vec<Json> = rows
        .into_iter()
        .map(|mut r| {
            r.retain(|_, v| !matches!(v, Json::String(s) if s.trim().is_empty()) && *v != Json::Bool(false));
            Json::Object(r)
        })
        .collect();
    if clean.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(clean));
    }
    true
}

/// Renames as rows of old and new name. Stored as a JSON object.
fn renames(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    let mut pairs: Vec<(String, String)> = match node.params.get(spec.key) {
        Some(Json::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect(),
        Some(Json::String(s)) => s.lines().filter_map(|l| l.split_once('=')).map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).collect(),
        _ => Vec::new(),
    };
    let before = pairs.clone();
    let mut remove = None;
    for (i, (old, new)) in pairs.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ((ui.available_width() - 110.0) / 2.0).max(50.0);
            egui::ComboBox::from_id_salt(("rename", spec.key, i)).selected_text(old.as_str()).width(w).show_ui(ui, |ui| {
                for f in fields {
                    ui.selectable_value(old, f.clone(), f);
                }
            });
            ui.label(RichText::new("to").small().color(pal.muted));
            ui.add(TextEdit::singleline(new).desired_width(w).hint_text("new name"));
            if ui.small_button("×").on_hover_text("Remove this rename").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        pairs.remove(i);
    }
    let free = fields.iter().find(|f| !pairs.iter().any(|p| &p.0 == *f));
    if ui.add_enabled(free.is_some(), egui::Button::new("Add rename").small()).clicked() {
        if let Some(f) = free {
            pairs.push((f.clone(), String::new()));
        }
    }
    if fields.is_empty() && pairs.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes can be picked here.").small().color(pal.muted));
    }
    if pairs == before {
        return false;
    }
    if pairs.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Object(pairs.into_iter().map(|(k, v)| (k, Json::String(v))).collect()));
    }
    true
}

/// The arrow buttons and other small symbols are drawn in the monospace font, which has them.
fn symbol(ui: &mut Ui, text: &str, enabled: bool, hint: &str) -> bool {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(text).font(theme::mono(11.0))).small().frame(false)).on_hover_text(hint).clicked()
}

/// Manage attributes: every attribute that comes in, with Keep, Rename or Remove, and arrows
/// to change the order. What is stored is the whole list as it is shown, once something in
/// it has been changed.
fn attribute_table(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[(String, &'static str)], pal: &Palette) -> bool {
    use og_engine::nodes::{attribute_rules, manager_from_old, manager_is_old, rules_json, AttrAction};
    let names: Vec<String> = fields.iter().map(|f| f.0.clone()).collect();
    // A node from before 0.53 has lists to keep and remove. They become rows once the
    // attributes that come in are known.
    if manager_is_old(&node.params) {
        if names.is_empty() {
            ui.label(
                RichText::new("Run the model, and the attributes are listed here. The settings this node had in an earlier version are kept until then.")
                    .small()
                    .color(pal.muted),
            );
            return false;
        }
        let (rows, only_listed) = manager_from_old(&node.params, &names);
        for k in ["keep", "remove", "rename"] {
            node.params.remove(k);
        }
        node.params.insert(spec.key.to_string(), rules_json(&rows));
        if only_listed {
            node.params.insert("others".to_string(), Json::String("remove".into()));
        }
        return true;
    }
    let others_go = node.params.get("others").and_then(|v| v.as_str()) == Some("remove");
    let unlisted = if others_go { AttrAction::Remove } else { AttrAction::Keep };
    let stored = attribute_rules(&node.params);
    // The types chosen, by name: "integer", "decimal" or "text". Empty keeps the type.
    let stored_types: Vec<(String, String)> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => {
            a.iter().filter_map(|r| Some((r.get("name")?.as_str()?.to_string(), r.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string()))).collect()
        }
        _ => Vec::new(),
    };
    let mut types: Vec<String> = Vec::new();
    // The rows: what is listed, in its order, and then what has come in since.
    let mut rows: Vec<(String, AttrAction, bool)> = stored.iter().map(|(n, a)| (n.clone(), a.clone(), true)).collect();
    for n in &names {
        if !rows.iter().any(|r| &r.0 == n) {
            rows.push((n.clone(), unlisted.clone(), false));
        }
    }
    if rows.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes are listed here.").small().color(pal.muted));
        return false;
    }
    for r in &rows {
        types.push(stored_types.iter().find(|t| t.0 == r.0).map(|t| t.1.clone()).unwrap_or_default());
    }
    let types_before = types.clone();
    let before: Vec<(String, AttrAction)> = rows.iter().map(|r| (r.0.clone(), r.1.clone())).collect();
    let mut moved: Option<(usize, usize)> = None;
    let mut dropped = None;
    let count = rows.len();
    ui.spacing_mut().item_spacing.y = 2.0;
    for (i, (name, action, listed)) in rows.iter_mut().enumerate() {
        let here = names.contains(name);
        let ty = fields.iter().find(|f| &f.0 == name).map_or("", |f| f.1);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if symbol(ui, "▲", i > 0, "Move up") {
                moved = Some((i, i - 1));
            }
            if symbol(ui, "▼", i + 1 < count, "Move down") {
                moved = Some((i, i + 1));
            }
            ui.add_space(4.0);
            // The name takes what the lists of actions and types leave.
            let width = (ui.available_width() - 176.0).max(40.0);
            ui.allocate_ui_with_layout(egui::vec2(width, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.x = 4.0;
                let text = RichText::new(name.as_str()).color(if here { pal.ink } else { pal.muted });
                let text = if *action == AttrAction::Remove { text.strikethrough() } else { text };
                let hover = match (here, *listed) {
                    (false, _) => "Not in the stream that comes in".to_string(),
                    (true, false) => format!("{}. Not in the list yet, so it is treated as the attributes that are not listed", ty),
                    (true, true) => ty.to_string(),
                };
                ui.add(egui::Label::new(text).truncate()).on_hover_text(hover);
                let note = match (here, *listed) {
                    (false, _) => "gone",
                    (true, false) => "new",
                    (true, true) => "",
                };
                if !note.is_empty() {
                    ui.label(RichText::new(note).font(theme::mono(10.5)).color(pal.muted));
                }
            });
            let label = match action {
                AttrAction::Keep => "Keep",
                AttrAction::Rename(_) => "Rename",
                AttrAction::Remove => "Remove",
            };
            egui::ComboBox::from_id_salt(("attr_action", spec.key, i)).selected_text(label).width(78.0).show_ui(ui, |ui| {
                let renamed = match action {
                    AttrAction::Rename(to) => to.clone(),
                    _ => String::new(),
                };
                ui.selectable_value(action, AttrAction::Keep, "Keep");
                ui.selectable_value(action, AttrAction::Rename(renamed), "Rename");
                ui.selectable_value(action, AttrAction::Remove, "Remove");
            });
            // The type: as it comes in, or another that the values are made into.
            let names_of = [("integer", "integer"), ("decimal", "decimal"), ("text", "text")];
            let current = types[i].clone();
            let shown = if current.is_empty() { ty.to_string() } else { format!("→ {}", current) };
            ui.add_space(4.0);
            egui::ComboBox::from_id_salt(("attr_type", spec.key, i))
                .selected_text(shown)
                .width(76.0)
                .show_ui(ui, |ui| {
                    for (v, l) in names_of {
                        let label = if l == ty { format!("{} (as it is)", l) } else { l.to_string() };
                        if ui.selectable_label(current == v || (current.is_empty() && l == ty), label).clicked() {
                            types[i] = if l == ty { String::new() } else { v.to_string() };
                        }
                    }
                })
                .response
                .on_hover_text(
                    "The type of the attribute. Another type makes the values into it; those that cannot be, such as text that is not a number, are left empty",
                );
            if !here && symbol(ui, "×", true, "Take it out of the list") {
                dropped = Some(i);
            }
        });
        if let AttrAction::Rename(to) = action {
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(RichText::new("to").small().color(pal.muted));
                ui.add(TextEdit::singleline(to).desired_width((ui.available_width() - 4.0).max(60.0)).hint_text("new name"));
            });
        }
    }
    let mut after: Vec<(String, AttrAction)> = rows.iter().map(|r| (r.0.clone(), r.1.clone())).collect();
    if let Some((a, b)) = moved {
        after.swap(a, b);
        types.swap(a, b);
    }
    if let Some(i) = dropped {
        after.remove(i);
        types.remove(i);
    }
    if after == before && types == types_before {
        return false;
    }
    let mut json = rules_json(&after);
    if let Json::Array(list) = &mut json {
        for (row, t) in list.iter_mut().zip(&types) {
            if !t.is_empty() {
                row["type"] = Json::String(t.clone());
            }
        }
    }
    node.params.insert(spec.key.to_string(), json);
    true
}

/// New attributes, each with a name and an expression for the value.
fn new_attribute_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    let types = og_engine::nodes::attribute_types(&node.params, spec.key);
    let type_text = |t: og_engine::model::FieldType| match t {
        og_engine::model::FieldType::Int => "integer",
        og_engine::model::FieldType::Real => "decimal",
        og_engine::model::FieldType::Text => "text",
    };
    let mut rows: Vec<(String, String, String)> = og_engine::nodes::new_attributes(&node.params)
        .into_iter()
        .map(|(n, v)| {
            let t = types.iter().find(|t| t.0 == n).map_or("", |t| type_text(t.1)).to_string();
            (n, v, t)
        })
        .collect();
    let before = rows.clone();
    let mut remove = None;
    for (i, (name, value, ty)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.add(TextEdit::singleline(name).desired_width((w * 0.3).max(60.0)).hint_text("name"));
            ui.label(RichText::new("=").color(pal.muted));
            ui.add(TextEdit::singleline(value).desired_width((ui.available_width() - 132.0).max(60.0)).font(theme::mono(12.5)).hint_text("empty"));
            let shown = if ty.is_empty() { "as it comes" } else { ty.as_str() }.to_string();
            egui::ComboBox::from_id_salt(("new_type", spec.key, i)).selected_text(shown).width(76.0).show_ui(ui, |ui| {
                for (v, l) in [("", "as it comes"), ("integer", "integer"), ("decimal", "decimal"), ("text", "text")] {
                    ui.selectable_value(ty, v.to_string(), l);
                }
            });
            if symbol(ui, "×", true, "Remove this attribute") {
                remove = Some(i);
            }
        });
        if !fields.is_empty() && !value.trim().is_empty() {
            if let Err(e) = expr::compile(value, fields) {
                ui.colored_label(pal.err, RichText::new(e.0).small());
            }
        }
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.add(egui::Button::new("Add attribute").small()).clicked() {
        rows.push((String::new(), String::new(), String::new()));
    }
    if rows.iter().any(|r| r.1.trim().is_empty() && !r.0.trim().is_empty()) {
        ui.label(RichText::new("An attribute without a value is empty, of the type chosen, or text.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        let list = rows
            .into_iter()
            .map(|(n, v, t)| if t.is_empty() { serde_json::json!({ "name": n, "value": v }) } else { serde_json::json!({ "name": n, "value": v, "type": t }) })
            .collect();
        node.params.insert(spec.key.to_string(), Json::Array(list));
    }
    true
}

/// A colour as egui has it.
/// A colour as a code, #rrggbb, or #rrggbbaa when it is not opaque: as QGIS writes them.
pub(crate) fn colour_code(c: [u8; 4]) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

/// A colour from a code: #rrggbb, #rrggbbaa or #rgb, with or without #.
pub(crate) fn parse_colour_code(text: &str) -> Option<[u8; 4]> {
    let t = text.trim().trim_start_matches('#');
    let hex = |s: &str| u8::from_str_radix(s, 16).ok();
    match t.len() {
        3 => {
            let v: Vec<u8> = t.chars().map(|ch| hex(&format!("{}{}", ch, ch))).collect::<Option<_>>()?;
            Some([v[0], v[1], v[2], 255])
        }
        6 | 8 => {
            let v: Vec<u8> = (0..t.len()).step_by(2).map(|k| t.get(k..k + 2).and_then(hex)).collect::<Option<_>>()?;
            Some([v[0], v[1], v[2], v.get(3).copied().unwrap_or(255)])
        }
        _ => None,
    }
}

/// The colour last copied with the secondary button on a colour, to paste on another.
const COPIED_COLOUR: &str = "ographis-copied-colour";

/// A button of a colour. A click opens the colour picker with the code of the colour, which
/// can be typed, such as #80d3ff; the secondary button copies the colour or pastes the one
/// copied. Gives whether the colour changed.
pub(crate) fn colour_button(ui: &mut Ui, c: &mut [u8; 4]) -> bool {
    let before = *c;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(34.0, 18.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let stroke = if r.hovered() { ui.visuals().widgets.hovered.fg_stroke } else { ui.visuals().widgets.inactive.bg_stroke };
        ui.painter().rect(rect, 3.0, to_color(*c), stroke, egui::StrokeKind::Inside);
    }
    let r = r.on_hover_text(format!("{}. Click to choose, or type its code. The secondary button copies it or pastes a copied one", colour_code(*c)));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, colour_code(before)));
    let code_id = r.id.with("code");
    egui::Popup::from_toggle_button_response(&r).id(r.id.with("picker")).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let mut col = to_color(*c);
        if egui::color_picker::color_picker_color32(ui, &mut col, egui::color_picker::Alpha::OnlyBlend) {
            let [r, g, b, a] = col.to_srgba_unmultiplied();
            *c = [r, g, b, a];
            ui.data_mut(|d| d.remove::<String>(code_id));
        }
        ui.horizontal(|ui| {
            ui.label("Code");
            let mut text: String = ui.data(|d| d.get_temp(code_id)).unwrap_or_else(|| colour_code(*c));
            let edit = ui
                .add(TextEdit::singleline(&mut text).desired_width(90.0).hint_text("#80d3ff"))
                .on_hover_text("As in QGIS: #rrggbb, or #rrggbbaa with how opaque it is");
            if edit.changed() {
                if let Some(v) = parse_colour_code(&text) {
                    *c = v;
                }
                ui.data_mut(|d| d.insert_temp(code_id, text.clone()));
            }
            if edit.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(code_id));
            }
            if parse_colour_code(&text).is_none() {
                ui.label(RichText::new("not a colour").small().color(ui.visuals().warn_fg_color));
            }
        });
    });
    let copied_id = egui::Id::new(COPIED_COLOUR);
    r.context_menu(|ui| {
        let copied: Option<[u8; 4]> = ui.ctx().data(|d| d.get_temp(copied_id));
        if ui.button("Copy colour").clicked() {
            let v = *c;
            ui.ctx().data_mut(|d| d.insert_temp(copied_id, v));
            ui.ctx().copy_text(colour_code(v));
            ui.close();
        }
        let paste = copied.map_or("Paste colour".to_string(), |v| format!("Paste colour {}", colour_code(v)));
        if ui.add_enabled(copied.is_some(), egui::Button::new(paste)).clicked() {
            if let Some(v) = copied {
                *c = v;
            }
            ui.close();
        }
    });
    *c != before
}

fn to_color(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// The layers of Draw map: one for each stream connected to its port, top first, each with
/// whether it is drawn, its colours and sizes, and arrows for the order. Once something is
/// changed the whole list is stored.
fn map_layer_rows(
    ui: &mut Ui,
    node: &mut GNode,
    spec: &ParamSpec,
    connected: &[(String, String)],
    pal: &Palette,
    stream_fields: &dyn Fn(&str) -> Vec<(String, bool)>,
    stream_raster: &dyn Fn(&str) -> Option<og_engine::rasterlook::RasterInfo>,
) -> bool {
    use og_engine::nodes::{arrange_layers, map_layers, map_layers_json};
    if connected.is_empty() {
        ui.label(RichText::new("Connect the streams to draw to Layers on the node. Each connection is a layer.").small().color(pal.muted));
        return false;
    }
    let keys: Vec<String> = connected.iter().map(|c| c.0.clone()).collect();
    let named = node.kind == "LayeredPdfWriter";
    let stored = map_layers(&node.params);
    let mut rows = arrange_layers(&stored, &keys);
    let before = rows.clone();
    let count = rows.len();
    let mut moved: Option<(usize, usize)> = None;
    // A layer dragged by its handle and let go over another: (from, to).
    let mut dropped: Option<(usize, usize)> = None;
    let copied_id = egui::Id::new(COPIED_COLOUR);
    for (i, row) in rows.iter_mut().enumerate() {
        let label = connected.iter().find(|c| c.0 == row.from).map_or(row.from.as_str(), |c| c.1.as_str());
        let frame = egui::Frame::new().fill(pal.bg).corner_radius(4).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // The handle to drag the layer by, to another place in the list.
                ui.dnd_drag_source(egui::Id::new(("layer-drag", spec.key, i)), i, |ui| {
                    // Six dots, drawn, so that they look the same whatever the font holds.
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                    for k in 0..6 {
                        let (cx, cy) = (r.left() + 3.0 + (k % 2) as f32 * 4.0, r.center().y - 4.0 + (k / 2) as f32 * 4.0);
                        ui.painter().circle_filled(egui::pos2(cx, cy), 1.1, pal.muted);
                    }
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Drag handle"));
                    resp.on_hover_cursor(egui::CursorIcon::Grab);
                })
                .response
                .on_hover_text("Drag to move the layer up or down in the list");
                ui.add_space(4.0);
                if symbol(ui, "▲", i > 0, "Move up: drawn above the layer over it") {
                    moved = Some((i, i - 1));
                }
                if symbol(ui, "▼", i + 1 < count, "Move down: drawn under the layer below it") {
                    moved = Some((i, i + 1));
                }
                ui.add_space(4.0);
                ui.checkbox(&mut row.show, "").on_hover_text(if named { "On or off when the PDF is opened" } else { "Drawn or not" });
                // The name of the layer: in the PDF, or in the legend of the map. The name of
                // the stream until another is given.
                let what = if named { "The name of the layer in the PDF" } else { "The name of the layer in the legend" };
                let colour = if row.show { pal.ink } else { pal.muted };
                ui.add(TextEdit::singleline(&mut row.name).id_salt(("layer-name", i)).hint_text(label).text_color(colour).desired_width(ui.available_width()))
                    .on_hover_text(format!("{}. Empty gives {}", what, label))
                    // Screen readers, and the tests, know the field by the layer it names.
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
            });
            // A raster is drawn by its cells, with settings of its own.
            let raster = if named { None } else { stream_raster(&row.from) };
            if let Some(info) = &raster {
                raster_rows(ui, &mut row.raster, info, &row.from, pal);
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
                // The secondary button on a colour copies it, and pastes a copied one on another.
                let menu = |r: egui::Response, value: &mut Option<[u8; 4]>| {
                    r.context_menu(|ui| {
                        let copied: Option<[u8; 4]> = ui.ctx().data(|d| d.get_temp(copied_id));
                        if ui.add_enabled(value.is_some(), egui::Button::new("Copy colour")).clicked() {
                            if let Some(c) = *value {
                                ui.ctx().data_mut(|d| d.insert_temp(copied_id, c));
                                ui.ctx().copy_text(format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]));
                            }
                            ui.close();
                        }
                        let paste = match copied {
                            Some(c) => format!("Paste colour #{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
                            None => "Paste colour".to_string(),
                        };
                        if ui.add_enabled(copied.is_some(), egui::Button::new(paste)).clicked() {
                            *value = copied;
                            ui.close();
                        }
                    });
                };
                let colour = |ui: &mut Ui, value: &mut Option<[u8; 4]>, name: &str, hint: &str, fallback: [u8; 4]| {
                    let mut on = value.is_some();
                    let r = ui.checkbox(&mut on, name).on_hover_text(hint);
                    if r.changed() {
                        *value = if on { Some(fallback) } else { None };
                    }
                    menu(r, value);
                    if let Some(c) = value {
                        colour_button(ui, c);
                    }
                    ui.add_space(6.0);
                };
                let defaults = og_engine::mapdraw::LayerStyle::default_for(i);
                colour(ui, &mut row.style.fill, "Fill", "The colour of areas and points", defaults.fill.unwrap_or([0, 0, 0, 255]));
                colour(
                    ui,
                    &mut row.style.stroke,
                    "Line",
                    "The colour of lines, and of the outlines of areas and points",
                    defaults.stroke.unwrap_or([0, 0, 0, 255]),
                );
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("width").small().color(pal.muted));
                ui.add(egui::DragValue::new(&mut row.style.width).range(0.0..=50.0).speed(0.1).max_decimals(1))
                    .on_hover_text("Of lines and outlines, in pixels");
                ui.add_space(6.0);
                ui.label(RichText::new("point size").small().color(pal.muted));
                ui.add(egui::DragValue::new(&mut row.style.size).range(0.0..=100.0).speed(0.2).max_decimals(1))
                    .on_hover_text("The diameter of a point, in pixels");
            });
            // Drawing by the values of an attribute: Draw map only, not the PDF.
            if !named {
                let fields = stream_fields(&row.from);
                theme_rows(ui, &mut row.theme, &row.from, &fields, pal);
                label_rows(ui, &mut row.labels, &row.from, &fields, pal);
                icon_rows(ui, &mut row.icon, &row.from, &fields, pal);
            }
        });
        // A layer dragged over this one: a line shows where it goes, above or below.
        let r = &frame.response;
        if let (Some(from), Some(p)) = (r.dnd_hover_payload::<usize>(), ui.ctx().pointer_hover_pos()) {
            if *from != i {
                let below = p.y > r.rect.center().y;
                let y = if below { r.rect.bottom() + 1.0 } else { r.rect.top() - 1.0 };
                ui.painter().hline(r.rect.x_range(), y, egui::Stroke::new(2.0, pal.accent));
            }
        }
        if let (Some(from), Some(p)) = (r.dnd_release_payload::<usize>(), ui.ctx().pointer_interact_pos()) {
            if *from != i {
                let below = p.y > r.rect.center().y;
                // The place in the list without the dragged layer.
                let mut to = if below { i + 1 } else { i };
                if *from < to {
                    to -= 1;
                }
                dropped = Some((*from, to));
            }
        }
        ui.add_space(2.0);
    }
    if let Some((a, b)) = moved {
        rows.swap(a, b);
    }
    if let Some((from, to)) = dropped {
        let layer = rows.remove(from);
        rows.insert(to.min(rows.len()), layer);
    }
    // Nothing changed: the list stays as it is stored, or not stored at all.
    if rows == before {
        return false;
    }
    node.params.insert(spec.key.to_string(), map_layers_json(&rows));
    true
}

/// How a layer of Draw map draws a raster: a picture as it is; a grid by a colour ramp, a
/// colour for each value, or as relief; and how see-through.
fn raster_rows(ui: &mut Ui, r: &mut og_engine::rasterlook::RasterSpec, info: &og_engine::rasterlook::RasterInfo, from: &str, pal: &Palette) {
    use og_engine::rasterlook::{class_colour, value_text};
    if !info.image {
        const MODES: &[(&str, &str)] =
            &[("", "By its values"), ("ramp", "Colour ramp"), ("classes", "A colour for each value"), ("shade", "Grey relief, as heights")];
        let auto = if info.classes.is_some() { "classes" } else { "ramp" };
        let shown = MODES.iter().find(|m| m.0 == r.mode).map_or("By its values", |m| m.1);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Draw as").small().color(pal.muted));
            egui::ComboBox::from_id_salt(("raster-mode", from)).selected_text(shown).width(190.0).show_ui(ui, |ui| {
                for (v, l) in MODES {
                    ui.selectable_value(&mut r.mode, v.to_string(), *l);
                }
            });
        });
        let mode = if r.mode.is_empty() { auto } else { r.mode.as_str() };
        match mode {
            "classes" => {
                // Each value, with its colour, whether it is drawn, and its name in the legend.
                let mut values: Vec<f64> = info.classes.clone().unwrap_or_default();
                for (v, _, _) in &r.colors {
                    if let Some(v) = og_engine::model::parse_number(v) {
                        if !values.contains(&v) {
                            values.push(v);
                        }
                    }
                }
                values.sort_by(f64::total_cmp);
                if values.is_empty() {
                    ui.label(RichText::new("Run the model, and the values of the raster are listed here.").small().color(pal.muted));
                }
                for (k, v) in values.iter().enumerate() {
                    let text = value_text(*v);
                    let at = r.colors.iter().position(|c| og_engine::model::parse_number(&c.0) == Some(*v));
                    let (colour, label) = match at {
                        Some(i) => (og_engine::mapdraw::parse_color(&r.colors[i].1).ok().unwrap_or(Some(class_colour(k))), r.colors[i].2.clone()),
                        None => (Some(class_colour(k)), String::new()),
                    };
                    let (mut on, mut c, mut name) = (colour.is_some(), colour.unwrap_or(class_colour(k)), label);
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        changed |= ui
                            .checkbox(&mut on, RichText::new(&text).monospace())
                            .on_hover_text("Drawn or not: a value that is not drawn lets the map under it show")
                            .changed();
                        if on {
                            changed |= colour_button(ui, &mut c);
                        }
                        changed |= ui
                            .add(TextEdit::singleline(&mut name).id_salt(("raster-class", from, k)).hint_text("name in the legend").desired_width(120.0))
                            .on_hover_text(format!("What {} stands for, such as Seen. Empty shows the value", text))
                            .changed();
                    });
                    if changed {
                        let colour = if on { og_engine::mapdraw::color_text(Some(c)) } else { "none".to_string() };
                        match at {
                            Some(i) => r.colors[i] = (text.clone(), colour, name),
                            None => r.colors.push((text.clone(), colour, name)),
                        }
                    }
                }
            }
            "ramp" => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Colours").small().color(pal.muted));
                    let shown = og_engine::thematic::RAMPS.iter().find(|x| x.0 == r.ramp).map_or("Yellow to blue", |x| x.1);
                    egui::ComboBox::from_id_salt(("raster-ramp", from)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                        for (v, l, _) in og_engine::thematic::RAMPS {
                            ui.selectable_value(&mut r.ramp, v.to_string(), *l);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (lo, hi) = info.range.unwrap_or((0.0, 1.0));
                    let edit = |ui: &mut Ui, v: &mut Option<f64>, hint: f64, id: &str| {
                        let mut t = v.map(value_text).unwrap_or_default();
                        let resp =
                            ui.add(TextEdit::singleline(&mut t).id_salt((id, from)).hint_text(value_text((hint * 100.0).round() / 100.0)).desired_width(70.0));
                        if resp.changed() {
                            *v = og_engine::model::parse_number(&t);
                        }
                    };
                    ui.label(RichText::new("from").small().color(pal.muted));
                    edit(ui, &mut r.min, lo, "raster-min");
                    ui.label(RichText::new("to").small().color(pal.muted));
                    edit(ui, &mut r.max, hi, "raster-max");
                })
                .response
                .on_hover_text("The values at the ends of the colours. Empty takes the smallest and the largest");
            }
            _ => {}
        }
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Opacity").small().color(pal.muted));
        let mut pct = (r.opacity * 100.0).round();
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix(" %").step_by(5.0)).on_hover_text("How much it covers what is under it").changed() {
            r.opacity = pct / 100.0;
        }
    });
}

/// How a layer of Draw map shows the values of its features: size, colour, order and
/// clusters, folded in under "Values" until it is used.
fn theme_rows(ui: &mut Ui, t: &mut og_engine::thematic::ThemeSpec, from: &str, fields: &[(String, bool)], pal: &Palette) {
    let on = !t.is_off();
    let title = if on { "Values · in use" } else { "Values" };
    egui::CollapsingHeader::new(RichText::new(title).small().color(if on { pal.accent } else { pal.muted })).id_salt(("theme", from)).default_open(on).show(
        ui,
        |ui| {
            if fields.is_empty() {
                ui.label(RichText::new("Run the model, and the attributes of this layer can be chosen.").small().color(pal.muted));
            }
            let pick = |ui: &mut Ui, id: &str, value: &mut String, numbers_only: bool| {
                let shown = if value.is_empty() { "None".to_string() } else { value.clone() };
                egui::ComboBox::from_id_salt((id, from)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                    ui.selectable_value(value, String::new(), "None");
                    for (f, num) in fields {
                        if *num || !numbers_only {
                            ui.selectable_value(value, f.clone(), f);
                        }
                    }
                });
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("Size by").small().color(pal.muted)).on_hover_text("Points get larger with the value: the area of the point follows it");
                pick(ui, "size-by", &mut t.size_by, true);
            });
            if !t.size_by.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("from").small().color(pal.muted));
                    ui.add(egui::DragValue::new(&mut t.size_min).range(1.0..=200.0).speed(0.2).max_decimals(1));
                    ui.label(RichText::new("to").small().color(pal.muted));
                    ui.add(egui::DragValue::new(&mut t.size_max).range(1.0..=300.0).speed(0.2).max_decimals(1));
                    ui.label(RichText::new("pixels").small().color(pal.muted));
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Colour by").small().color(pal.muted))
                    .on_hover_text("A colour after the value: classes of a number, or a colour per text. For areas, a choropleth map");
                pick(ui, "color-by", &mut t.color_by, false);
            });
            if !t.color_by.is_empty() {
                let numeric = fields.iter().find(|f| f.0 == t.color_by).is_none_or(|f| f.1) && t.ramp != "categories";
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let shown = if t.ramp == "categories" {
                        "A colour per value"
                    } else {
                        og_engine::thematic::RAMPS.iter().find(|r| r.0 == t.ramp).map_or("Blues", |r| r.1)
                    };
                    egui::ComboBox::from_id_salt(("ramp", from)).selected_text(shown).width(130.0).show_ui(ui, |ui| {
                        for (v, l, _) in og_engine::thematic::RAMPS {
                            ui.selectable_value(&mut t.ramp, v.to_string(), *l);
                        }
                        ui.selectable_value(&mut t.ramp, "categories".to_string(), "A colour per value");
                    });
                    if numeric {
                        ui.add(egui::DragValue::new(&mut t.classes).range(1..=12).speed(0.1)).on_hover_text("The number of classes");
                        ui.label(RichText::new("classes").small().color(pal.muted));
                        let methods = [("equal", "Equal intervals"), ("quantile", "As many in each"), ("natural", "Natural breaks")];
                        let shown = methods.iter().find(|m| m.0 == t.method).map_or("Equal intervals", |m| m.1);
                        egui::ComboBox::from_id_salt(("method", from)).selected_text(shown).width(130.0).show_ui(ui, |ui| {
                            for (v, l) in methods {
                                ui.selectable_value(&mut t.method, v.to_string(), l);
                            }
                        });
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Order").small().color(pal.muted))
                    .on_hover_text("Which are drawn first, and so lie underneath. By the attribute of the size, or else of the colour");
                let orders = [("", "As they come"), ("large_below", "Largest underneath"), ("small_below", "Smallest underneath")];
                let shown = orders.iter().find(|o| o.0 == t.order).map_or("As they come", |o| o.1);
                egui::ComboBox::from_id_salt(("order", from)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                    for (v, l) in orders {
                        ui.selectable_value(&mut t.order, v.to_string(), l);
                    }
                });
            });
            if !t.order.is_empty() && t.size_by.is_empty() && t.color_by.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("by").small().color(pal.muted));
                    pick(ui, "order-by", &mut t.order_by, true);
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Gather points within").small().color(pal.muted))
                    .on_hover_text("Points closer than this on the map are drawn as one, with their number in it. 0 draws every point");
                ui.add(egui::DragValue::new(&mut t.cluster).range(0.0..=200.0).speed(0.5).max_decimals(0));
                ui.label(RichText::new("pixels").small().color(pal.muted));
            });
        },
    );
}

/// A list of the attributes of a layer to choose one from, or none.
fn attribute_pick(ui: &mut Ui, id: (&str, &str), value: &mut String, fields: &[(String, bool)], numbers_only: bool, width: f32) {
    let shown = if value.is_empty() { "None".to_string() } else { value.clone() };
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(width).show_ui(ui, |ui| {
        ui.selectable_value(value, String::new(), "None");
        for (f, num) in fields {
            if *num || !numbers_only {
                ui.selectable_value(value, f.clone(), f);
            }
        }
    });
}

/// A colour that can be switched off, with a button to choose it.
fn optional_colour(ui: &mut Ui, value: &mut Option<[u8; 4]>, name: &str, hint: &str, fallback: [u8; 4]) {
    let mut on = value.is_some();
    if ui.checkbox(&mut on, name).on_hover_text(hint).changed() {
        *value = if on { Some(fallback) } else { None };
    }
    if let Some(c) = value {
        colour_button(ui, c);
    }
}

/// How a layer of Draw map labels its features, folded in under "Labels" until it is used.
fn label_rows(ui: &mut Ui, l: &mut og_engine::mapdraw::LabelSpec, from: &str, fields: &[(String, bool)], pal: &Palette) {
    let on = !l.text.trim().is_empty();
    let title = if on { "Labels · in use" } else { "Labels" };
    egui::CollapsingHeader::new(RichText::new(title).small().color(if on { pal.accent } else { pal.muted })).id_salt(("labels", from)).default_open(on).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Text").small().color(pal.muted)).on_hover_text("An attribute, or an expression such as name || ' ' || year");
                let mut chosen = if fields.iter().any(|f| f.0 == l.text) { l.text.clone() } else { String::new() };
                let before = chosen.clone();
                attribute_pick(ui, ("label-attr", from), &mut chosen, fields, false, 120.0);
                if chosen != before {
                    l.text = chosen;
                }
            });
            ui.add(
                TextEdit::singleline(&mut l.text).id_salt(("label-text", from)).hint_text("an attribute or an expression").desired_width(ui.available_width()),
            )
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Label text"));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Placed").small().color(pal.muted));
                let places = [("auto", "Beside points, along lines"), ("center", "On the point, in the middle"), ("xy", "At x and y from attributes")];
                let shown = places.iter().find(|p| p.0 == l.place).map_or(places[0].1, |p| p.1);
                egui::ComboBox::from_id_salt(("label-place", from)).selected_text(shown).width(170.0).show_ui(ui, |ui| {
                    for (v, t) in places {
                        ui.selectable_value(&mut l.place, v.to_string(), t);
                    }
                });
            });
            if l.place == "xy" {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("x").small().color(pal.muted));
                    attribute_pick(ui, ("label-x", from), &mut l.x_attr, fields, true, 90.0);
                    ui.label(RichText::new("y").small().color(pal.muted));
                    attribute_pick(ui, ("label-y", from), &mut l.y_attr, fields, true, 90.0);
                });
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut l.all, "All, also where they overlap")
                    .on_hover_text("Otherwise a label that would cover another, or reach outside the map, is left out");
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("First").small().color(pal.muted))
                    .on_hover_text("When there is not room for all: the largest values of this attribute get it first");
                attribute_pick(ui, ("label-priority", from), &mut l.priority_by, fields, true, 120.0);
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("size").small().color(pal.muted));
                ui.add(egui::DragValue::new(&mut l.size).range(4.0..=200.0).speed(0.2).max_decimals(1)).on_hover_text("The height of the letters, in pixels");
                ui.label(RichText::new("by").small().color(pal.muted));
                attribute_pick(ui, ("label-size-by", from), &mut l.size_by, fields, true, 100.0);
                if !l.size_by.is_empty() {
                    ui.label(RichText::new("to").small().color(pal.muted));
                    ui.add(egui::DragValue::new(&mut l.size_max).range(4.0..=300.0).speed(0.2).max_decimals(1)).on_hover_text("The size of the largest value");
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.checkbox(&mut l.bold, "Bold");
                ui.checkbox(&mut l.upper, "CAPITALS");
                let mut c = Some(l.colour);
                optional_colour(ui, &mut c, "Colour", "The colour of the letters", [0x22, 0x22, 0x22, 255]);
                if let Some(c) = c {
                    l.colour = c;
                }
                optional_colour(ui, &mut l.halo, "Halo", "An edge around the letters, so that they can be read on anything", [255, 255, 255, 255]);
                if l.halo.is_some() {
                    ui.add(egui::DragValue::new(&mut l.halo_width).range(0.0..=20.0).speed(0.1).max_decimals(1))
                        .on_hover_text("How wide the halo is, in pixels");
                }
            });
        },
    );
}

/// Icons for the points of a layer of Draw map, from Maki, folded in under "Icons".
fn icon_rows(ui: &mut Ui, ic: &mut og_engine::mapdraw::IconSpec, from: &str, fields: &[(String, bool)], pal: &Palette) {
    let on = !ic.is_off();
    let title = if on { "Icons · in use" } else { "Icons" };
    egui::CollapsingHeader::new(RichText::new(title).small().color(if on { pal.accent } else { pal.muted })).id_salt(("icons", from)).default_open(on).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Icon").small().color(pal.muted)).on_hover_text("The same icon for every point. Icons from Maki, by Mapbox (CC0)");
            let shown = if ic.name.is_empty() { "None".to_string() } else { ic.name.clone() };
            egui::ComboBox::from_id_salt(("icon-name", from)).selected_text(shown).width(170.0).height(320.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut ic.name, String::new(), "None");
                for (n, _) in og_engine::icons::MAKI {
                    ui.selectable_value(&mut ic.name, n.to_string(), *n);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("or by").small().color(pal.muted))
                .on_hover_text("An attribute whose values name the icons, such as amenity from OpenStreetMap: hospital, doctors, police, charging_station, hotel. Swedish words such as vårdcentral and apotek work too");
            attribute_pick(ui, ("icon-by", from), &mut ic.by, fields, false, 140.0);
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Drawn").small().color(pal.muted));
            let styles = [("plain", "In the fill colour, edged in the line colour"), ("circle", "White on a circle of the fill colour")];
            let shown = styles.iter().find(|s| s.0 == ic.style).map_or(styles[0].1, |s| s.1);
            egui::ComboBox::from_id_salt(("icon-style", from)).selected_text(shown).width(230.0).show_ui(ui, |ui| {
                for (v, t) in styles {
                    ui.selectable_value(&mut ic.style, v.to_string(), t);
                }
            });
        });
        ui.label(RichText::new("The point size is the size of the icon; below 10 it is 18.").small().color(pal.muted));
    });
}

/// The values of Filter by value, one per row, and a button that takes them from the data.
fn value_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, attr: &str, found: &[String], pal: &Palette) -> bool {
    let mut values: Vec<String> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect(),
        Some(Json::String(s)) => s.lines().map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let before = values.clone();
    let mut remove = None;
    for (i, v) in values.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(v).desired_width((ui.available_width() - 26.0).max(60.0)).hint_text("value"));
            if symbol(ui, "×", true, "Remove this value and its output") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        values.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Add value").small()).clicked() {
            values.push(String::new());
        }
        let button = ui.add_enabled(!found.is_empty(), egui::Button::new("Use the values in the data").small());
        let why_not = if attr.is_empty() { "Choose the attribute first" } else { "Run the model, and the values that come in can be taken from there" };
        if button.on_hover_text(format!("Lists the {} values of {} in the latest run", found.len(), attr)).on_disabled_hover_text(why_not).clicked() {
            values = found.to_vec();
        }
    });
    if found.len() >= 100 {
        ui.label(RichText::new("The data has more values than that. The first hundred are taken.").small().color(pal.muted));
    }
    if values == before {
        return false;
    }
    if values.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(values.into_iter().map(Json::String).collect()));
    }
    true
}

/// Extract JSON values: rows of a path and the attribute it goes to, and the paths that the
/// JSON of the latest run has, to click.
fn json_path_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, found: &[String], pal: &Palette) -> bool {
    let mut rows = og_engine::nodes::json_paths_of(&node.params, spec.key);
    // Rows being typed may have an empty path; those are kept while the form is open.
    if let Some(Json::Array(a)) = node.params.get(spec.key) {
        rows = a
            .iter()
            .filter_map(|r| {
                r.as_array()
                    .map(|r| (r.first().and_then(|v| v.as_str()).unwrap_or("").to_string(), r.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string()))
            })
            .collect();
    }
    let before = rows.clone();
    let mut remove = None;
    for (i, (path, name)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ((ui.available_width() - 34.0) / 2.0).max(60.0);
            ui.add(TextEdit::singleline(path).id_salt(("json-path", i)).desired_width(w).font(theme::mono(12.0)).hint_text("path"));
            let hint = if path.trim().is_empty() { "name".to_string() } else { og_engine::nodes::name_of_path(path) };
            ui.add(TextEdit::singleline(name).id_salt(("json-name", i)).desired_width(w).hint_text(hint));
            if symbol(ui, "×", true, "Remove this value") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.add(egui::Button::new("Add value").small()).clicked() {
        rows.push((String::new(), String::new()));
    }
    // The paths in the JSON that came in, from the first features.
    let mut paths: Vec<String> = Vec::new();
    for text in found.iter().take(20) {
        if let Ok(v) = og_engine::json::parse(text) {
            og_engine::json::paths(&v, "", &mut paths);
        }
        if paths.len() > 60 {
            break;
        }
    }
    if paths.is_empty() {
        ui.label(RichText::new("Run the model, and the paths in the JSON can be clicked here.").small().color(pal.muted));
    } else {
        ui.label(RichText::new("Paths in the JSON, to click:").small().color(pal.muted));
        if let Some(p) = chips(ui, pal, &paths, |p| rows.iter().any(|r| r.0 == p)) {
            match rows.iter().position(|r| r.0 == p) {
                Some(i) => {
                    rows.remove(i);
                }
                None => rows.push((p, String::new())),
            }
        }
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(rows.into_iter().map(|(p, n)| Json::Array(vec![Json::String(p), Json::String(n)])).collect()));
    }
    true
}

/// Map values: rows of a value and what it becomes. Stored as a JSON list of [from, to].
fn mapping_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, attr: &str, found: &[String], pal: &Palette) -> bool {
    let text = |v: &Json| match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let mut rows: Vec<(String, String)> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a.iter().filter_map(|r| r.as_array().filter(|r| r.len() == 2).map(|r| (text(&r[0]), text(&r[1])))).collect(),
        Some(Json::Object(o)) => o.iter().map(|(k, v)| (k.clone(), text(v))).collect(),
        _ => Vec::new(),
    };
    let before = rows.clone();
    let mut remove = None;
    for (i, (from, to)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ((ui.available_width() - 60.0) / 2.0).max(50.0);
            ui.add(TextEdit::singleline(from).id_salt(("map-from", i)).desired_width(w).hint_text("value"));
            ui.label(RichText::new("→").font(theme::mono(11.0)).color(pal.muted));
            ui.add(TextEdit::singleline(to).id_salt(("map-to", i)).desired_width(w).hint_text("becomes"));
            if symbol(ui, "×", true, "Remove this row") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Add value").small()).clicked() {
            rows.push((String::new(), String::new()));
        }
        let button = ui.add_enabled(!found.is_empty(), egui::Button::new("Use the values in the data").small());
        let why_not = if attr.is_empty() { "Choose the attribute first" } else { "Run the model, and the values that come in can be taken from there" };
        if button
            .on_hover_text(format!("Adds a row for each value of {} in the latest run that is not in the list yet", attr))
            .on_disabled_hover_text(why_not)
            .clicked()
        {
            for v in found {
                if !rows.iter().any(|r| r.0.trim() == v.trim()) {
                    rows.push((v.clone(), String::new()));
                }
            }
        }
    });
    if found.len() >= 100 {
        ui.label(RichText::new("The data has more values than that. The first hundred are taken.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(rows.into_iter().map(|(a, b)| Json::Array(vec![Json::String(a), Json::String(b)])).collect()));
    }
    true
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
