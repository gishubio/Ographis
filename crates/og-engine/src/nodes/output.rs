//! Output and what writes or shows: Output, View, Make zip file and GeoSpatial PDF with layers.

use super::*;
use crate::model::{crs_label, err, fmt_count, GbError, Result, Table};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

// ---------- View (Viewer) ----------

/// The name a stream connected to View is shown by: its number and where it comes from.
pub fn view_name(i: usize, label: &str) -> String {
    format!("{} · {}", i + 1, label)
}

pub(super) fn run_viewer(_p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let parts = std::mem::take(&mut ctx.parts);
    let n = parts.len();
    if n > 0 {
        ctx.notes.info(format!("{} {} to look at", n, if n == 1 { "stream" } else { "streams" }));
    }
    Ok(parts.into_iter().enumerate().map(|(i, p)| (view_name(i, &p.label).into(), Arc::unwrap_or_clone(p.table))).collect())
}

// ---------- Output (Writer) ----------

pub(super) const CSV_GEOMETRY: &[(&str, &str)] =
    &[("none", "None"), ("xy", "x and y, for points"), ("wkt", "WKT"), ("auto", "x and y for points, WKT for the rest")];
/// What an output does when its layer, sheet or file is already there.
pub(super) const WRITE_MODES: &[(&str, &str)] = &[("overwrite", "Overwrite it"), ("append", "Add the features to it")];
/// The fonts a sheet can be written in. Empty leaves the font of the program that opens it.
pub const SHEET_FONTS: &[(&str, &str)] = &[
    ("", "Standard"),
    ("Aptos", "Aptos"),
    ("Arial", "Arial"),
    ("Calibri", "Calibri"),
    ("Consolas", "Consolas"),
    ("Liberation Sans", "Liberation Sans"),
    ("Segoe UI", "Segoe UI"),
    ("Times New Roman", "Times New Roman"),
];
/// The colour schemes of a sheet: a dark top row and every second row tinted.
pub const SHEET_COLORS: &[(&str, &str)] = &[("", "None"), ("grey", "Grey"), ("blue", "Blue"), ("green", "Green"), ("sand", "Sand"), ("magenta", "Magenta")];
pub(super) const OUT_ENCODINGS: &[(&str, &str)] = &[("utf-8", "UTF-8"), ("utf-8-bom", "UTF-8 with BOM (Excel)"), ("windows-1252", "Windows-1252")];
/// The formats a writer can be told to write: (file extension, label).
pub const WRITER_FORMATS: &[(&str, &str)] = &[
    ("", "From the file name"),
    ("csv", "CSV (.csv)"),
    ("xlsx", "Excel (.xlsx)"),
    ("fgb", "FlatGeobuf (.fgb)"),
    ("geojson", "GeoJSON (.geojson)"),
    ("gpkg", "GeoPackage (.gpkg)"),
    ("tif", "GeoTIFF image (.tif)"),
    ("glb", "glTF 3D model (.glb)"),
    ("jpg", "JPEG image (.jpg)"),
    ("ods", "LibreOffice Calc (.ods)"),
    ("obj", "OBJ 3D model (.obj)"),
    ("pdf", "PDF (.pdf)"),
    ("png", "PNG image (.png)"),
    ("shp", "Shapefile (.shp)"),
    ("txt", "Text (.txt)"),
];

/// What an output can write to: the file formats, a GeoSpatial PDF with layers, and a table
/// in PostGIS. The last two are tools of their own, which the program switches to when they
/// are chosen.
pub const OUTPUT_FORMATS: &[(&str, &str)] = &[
    ("", "From the file name"),
    ("csv", "CSV (.csv)"),
    ("xlsx", "Excel (.xlsx)"),
    ("fgb", "FlatGeobuf (.fgb)"),
    ("geojson", "GeoJSON (.geojson)"),
    ("gpkg", "GeoPackage (.gpkg)"),
    ("geopdf", "GeoSpatial PDF with layers (.pdf)"),
    ("tif", "GeoTIFF image (.tif)"),
    ("glb", "glTF 3D model (.glb)"),
    ("jpg", "JPEG image (.jpg)"),
    ("ods", "LibreOffice Calc (.ods)"),
    ("obj", "OBJ 3D model (.obj)"),
    ("pdf", "PDF (.pdf)"),
    ("png", "PNG image (.png)"),
    ("postgis", "PostGIS table"),
    ("pptx", "PowerPoint slides (.pptx)"),
    ("odp", "LibreOffice Impress slides (.odp)"),
    ("shp", "Shapefile (.shp)"),
    ("txt", "Text (.txt)"),
];

/// The tool that writes a format of OUTPUT_FORMATS.
pub fn output_kind(format: &str) -> &'static str {
    match format {
        "geopdf" => "LayeredPdfWriter",
        "postgis" => "PostgisWriter",
        "pptx" | "odp" => "SlideWriter",
        _ => "Writer",
    }
}

pub(super) const KEYS: &[(&str, &str)] = &[("new", "A new column of numbers"), ("attribute", "An attribute")];

/// The primary key from the parameters key, key_name and key_attribute, with the name of the
/// new column when none is given.
pub(super) fn primary_key(p: &P, default_name: &str) -> Result<Option<crate::io::PrimaryKey>> {
    Ok(match p.get("key").as_deref() {
        Some("attribute") => match p.get("key_attribute").filter(|a| !a.trim().is_empty()) {
            Some(a) => Some(crate::io::PrimaryKey::Attribute(a)),
            None => return err("Choose the attribute that is to be the primary key."),
        },
        _ => {
            let name = p.get("key_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| default_name.to_string());
            Some(crate::io::PrimaryKey::New(name.trim().to_string()))
        }
    })
}

pub(super) const PDF_SIZES: &[(&str, &str)] = &[
    ("a4", "A4"),
    ("a3", "A3"),
    ("a2", "A2"),
    ("a1", "A1"),
    ("a0", "A0"),
    ("a5", "A5"),
    ("letter", "Letter"),
    ("legal", "Legal"),
    ("tabloid", "Tabloid"),
    ("custom", "A size of my own"),
    ("fit", "As large as the picture"),
];
pub(super) const PDF_COMPRESS: &[(&str, &str)] = &[("deflate", "Lossless: every pixel kept"), ("jpeg", "JPEG: smaller, for photos and shaded relief")];

/// The page of a PDF from the parameters pdf_*.
pub(super) fn pdf_page(p: &P) -> Result<crate::io::PdfPage> {
    let d = crate::io::PdfPage::default();
    Ok(crate::io::PdfPage {
        size: p.get("pdf_size").unwrap_or_else(|| "a4".into()),
        custom_mm: (number_param(p, "pdf_width", "The page width")?.unwrap_or(210.0), number_param(p, "pdf_height", "The page height")?.unwrap_or(297.0)),
        dpi: number_param(p, "pdf_dpi", "The pixels to the inch")?.unwrap_or(150.0),
        compress: p.get("pdf_compress").unwrap_or(d.compress),
        quality: number_param(p, "pdf_quality", "The JPEG quality")?.unwrap_or(85.0).clamp(1.0, 100.0) as u8,
        orientation: p.get("pdf_orientation").unwrap_or_else(|| "auto".into()),
        margin_mm: number_param(p, "pdf_margin", "The margin")?.unwrap_or(10.0),
        title: p.get("pdf_title").filter(|t| !t.trim().is_empty()),
        title_align: p.get("pdf_title_align").unwrap_or_else(|| "left".into()),
    })
}
pub(super) const PDF_ORIENTATIONS: &[(&str, &str)] = &[("auto", "After the picture"), ("portrait", "Portrait"), ("landscape", "Landscape")];
pub(super) const PDF_TITLE_ALIGN: &[(&str, &str)] = &[("left", "Left"), ("center", "Center"), ("right", "Right")];

pub(super) const WRITER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "File",
        kind: ParamKind::FileSave,
        default: "",
        hint: "An attribute or an expression in braces gives a file for each value: out/{municipality}.gpkg, or stops_{now('%Y-%m-%d')}.xlsx",
    },
    ParamSpec { key: "format", label: "Format", kind: ParamKind::Choice(OUTPUT_FORMATS), default: "", hint: "The file gets the extension of the format" },
    ParamSpec {
        key: "layer",
        label: "Layer",
        kind: ParamKind::Text,
        default: "",
        hint: "Table in a GeoPackage, sheet in a workbook. Empty uses the file name. Other layers and sheets in the file are kept. {municipality} gives one for each value",
    },
    ParamSpec { key: "encoding", label: "Encoding", kind: ParamKind::Choice(OUT_ENCODINGS), default: "utf-8", hint: "For Shapefile and CSV" },
    ParamSpec { key: "delimiter", label: "Delimiter (CSV)", kind: ParamKind::Text, default: ";", hint: "" },
    ParamSpec { key: "decimal_comma", label: "Decimal comma (CSV)", kind: ParamKind::Choice(YES_NO), default: "false", hint: "" },
    ParamSpec {
        key: "geometry",
        label: "Geometry (CSV, Excel)",
        kind: ParamKind::Choice(CSV_GEOMETRY),
        default: "none",
        hint: "These formats have no geometry of their own, so it is left out. Points can be written as x and y columns, and any geometry as WKT text",
    },
    ParamSpec {
        key: "mode",
        label: "If it is already there",
        kind: ParamKind::Choice(WRITE_MODES),
        default: "overwrite",
        hint: "The table in a GeoPackage, the sheet in a workbook, or the file. New features are matched to the attributes that are there by name",
    },
    ParamSpec { key: "key", label: "Primary key", kind: ParamKind::Choice(KEYS), default: "new", hint: "The column that tells the rows apart" },
    ParamSpec { key: "key_name", label: "Key column", kind: ParamKind::Text, default: "fid", hint: "The name of the new column of numbers" },
    ParamSpec {
        key: "key_attribute",
        label: "Key attribute",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "Its values must all be different and none empty. In GeoPackage whole numbers are the key; other values get an index that keeps them apart",
    },
    ParamSpec { key: "font", label: "Font", kind: ParamKind::Choice(SHEET_FONTS), default: "", hint: "" },
    ParamSpec { key: "bold_header", label: "Bold top row", kind: ParamKind::Choice(YES_NO), default: "false", hint: "" },
    ParamSpec {
        key: "freeze",
        label: "Lock the top row",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "The names stay in view when the sheet is scrolled",
    },
    ParamSpec {
        key: "colors",
        label: "Colours",
        kind: ParamKind::Choice(SHEET_COLORS),
        default: "",
        hint: "The top row filled, and every second row tinted to make the rows easier to follow",
    },
    ParamSpec { key: "pdf_size", label: "Page size", kind: ParamKind::Choice(PDF_SIZES), default: "a4", hint: "" },
    ParamSpec { key: "pdf_width", label: "Page width in mm", kind: ParamKind::Text, default: "210", hint: "" },
    ParamSpec { key: "pdf_height", label: "Page height in mm", kind: ParamKind::Text, default: "297", hint: "" },
    ParamSpec {
        key: "pdf_dpi",
        label: "Pixels to the inch",
        kind: ParamKind::Text,
        default: "150",
        hint: "How large the picture is printed: 300 is sharp in print, 150 enough for a screen",
    },
    ParamSpec {
        key: "pdf_compress",
        label: "Picture packed as",
        kind: ParamKind::Choice(PDF_COMPRESS),
        default: "deflate",
        hint: "Lossless suits maps with lines and text; JPEG makes a large picture much smaller",
    },
    ParamSpec { key: "pdf_quality", label: "JPEG quality", kind: ParamKind::Text, default: "85", hint: "1 to 100: more is sharper and larger" },
    ParamSpec {
        key: "pdf_orientation",
        label: "Orientation",
        kind: ParamKind::Choice(PDF_ORIENTATIONS),
        default: "auto",
        hint: "After the picture: landscape when the picture is wider than it is high",
    },
    ParamSpec { key: "pdf_margin", label: "Margin in mm", kind: ParamKind::Text, default: "10", hint: "" },
    ParamSpec { key: "pdf_title", label: "Title", kind: ParamKind::Text, default: "", hint: "Written above the picture. Empty gives none" },
    ParamSpec {
        key: "pdf_title_align",
        label: "Title placed",
        kind: ParamKind::Choice(PDF_TITLE_ALIGN),
        default: "left",
        hint: "Along the top edge of the picture: at the left, in the middle or at the right",
    },
    ParamSpec {
        key: "fields",
        label: "Data types and sizes",
        kind: ParamKind::Schema("Input"),
        default: "",
        hint: "Write an attribute as another type, cut text at a length, or round to a number of decimals",
    },
];

/// The file a writer writes, as its path is stored: the path, with the extension of the
/// chosen format. A name with another extension keeps it in front of the new one, unless
/// it is the extension of another format the writer knows.
pub fn writer_file(path: &str, format: Option<&str>) -> String {
    let Some(format) = format.map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty()) else { return path.to_string() };
    let format = if format == "geopdf" { "pdf".to_string() } else { format };
    let ext = ext_of(Path::new(path));
    if ext == format {
        path.to_string()
    } else if ext.is_empty() || WRITER_FORMATS.iter().any(|f| f.0 == ext) {
        Path::new(path).with_extension(&format).to_string_lossy().into_owned()
    } else {
        format!("{}.{}", path, format)
    }
}

/// A name with expressions in braces, such as out/{municipality}.gpkg or stops_{now('%Y')}:
/// the text around them as it is, and each expression for each feature.
pub struct NameTemplate {
    parts: Vec<(String, Option<expr::Compiled>)>,
}

impl NameTemplate {
    /// A template, or None when the text has no expression in braces.
    pub fn parse(text: &str, fields: &[String]) -> Result<Option<NameTemplate>> {
        if !text.contains('{') {
            return Ok(None);
        }
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}').map(|c| open + c) else {
                return err(format!("A {{ in {} is not closed with }}.", text));
            };
            let inside = &rest[open + 1..close];
            let e = expr::compile(inside, fields).map_err(|e| GbError(format!("{{{}}} in the name: {}", inside, e.0)))?;
            parts.push((rest[..open].to_string(), Some(e)));
            rest = &rest[close + 1..];
        }
        parts.push((rest.to_string(), None));
        Ok(Some(NameTemplate { parts }))
    }

    /// The name for a feature. With `safe`, values are made fit for file names: characters
    /// that a file name cannot have become _. An empty value is "empty".
    pub fn name_for(&self, f: &Feature, i: usize, geographic: Option<bool>, safe: bool) -> Result<String> {
        let mut out = String::new();
        for (text, e) in &self.parts {
            out.push_str(text);
            if let Some(e) = e {
                let v = e.eval(&Row { feature: f, index: i, geographic })?.to_text();
                let v = v.trim();
                let v = if v.is_empty() { "empty".to_string() } else { v.to_string() };
                // Braces never come out of a value, so the name is not read as a template again.
                let bad = if safe { "/\\:*?\"<>|{}" } else { "{}" };
                out.extend(v.chars().map(|c| if bad.contains(c) || c.is_control() { '_' } else { c }));
            }
        }
        Ok(out)
    }
}

/// An output whose file, layer or sheet has {expressions} in its name writes a file, layer
/// or sheet for each name the features give, such as one GeoPackage per municipality.
pub(super) fn split_by_name(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Option<Outputs>> {
    let table = input(inp, "Input");
    let fields = table.field_names();
    let path_t = NameTemplate::parse(&p.get("path").unwrap_or_default(), &fields)?;
    let layer_t = NameTemplate::parse(&p.get("layer").unwrap_or_default(), &fields)?;
    if path_t.is_none() && layer_t.is_none() {
        return Ok(None);
    }
    let geographic = ctx.geographic(table.crs);
    // The groups in the order their first feature comes.
    type Named = (String, Option<String>);
    let mut groups: Vec<(Named, Vec<usize>)> = Vec::new();
    for (i, f) in table.features.iter().enumerate() {
        let path = match &path_t {
            Some(t) => t.name_for(f, i, geographic, true).map_err(|e| at_row(e, i))?,
            None => p.get("path").unwrap_or_default(),
        };
        let layer = match &layer_t {
            Some(t) => Some(t.name_for(f, i, geographic, false).map_err(|e| at_row(e, i))?),
            None => None,
        };
        let key = (path, layer);
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    if groups.len() > 10_000 {
        return err(format!("The names give {} files or layers. At most 10 000 are written.", fmt_count(groups.len() as u64)));
    }
    for ((path, layer), rows) in &groups {
        let mut params = p.values.clone();
        params.insert("path".into(), serde_json::Value::String(path.clone()));
        if let Some(l) = layer {
            params.insert("layer".into(), serde_json::Value::String(l.clone()));
        }
        let part = Table { features: rows.iter().map(|i| table.features[*i].clone()).collect(), ..table.like() };
        let mut one: Inputs = HashMap::new();
        one.insert("Input", Arc::new(part));
        run_writer(&P::new(&params, WRITER_PARAMS), &one, ctx)?;
    }
    let what = match (&path_t, &layer_t) {
        (Some(_), _) => "files",
        (None, _) if is_spreadsheet(&ext_of(Path::new(&p.get("path").unwrap_or_default()))) => "sheets",
        _ => "layers",
    };
    let names: Vec<String> =
        groups.iter().take(5).map(|((path, layer), _)| layer.clone().filter(|_| path_t.is_none()).unwrap_or_else(|| path.clone())).collect();
    let more = if groups.len() > 5 { ", …" } else { "" };
    ctx.notes.info(format!("{} {}, one per name: {}{}", fmt_count(groups.len() as u64), what, names.join(", "), more));
    Ok(Some(Vec::new()))
}

pub(super) fn run_writer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    if let Some(out) = split_by_name(p, inp, ctx)? {
        return Ok(out);
    }
    match p.get("format").as_deref() {
        Some("postgis") => return err("PostGIS is written by the type PostgisWriter. Choose PostGIS table as the format in the program, and it switches."),
        Some("geopdf") => return err("A GeoSpatial PDF is written by the type LayeredPdfWriter. Choose the format in the program, and it switches."),
        _ => {}
    }
    let stored = p.need("path", "The output has no file. Say where the result should be written.")?;
    let path = ctx.resolve(&writer_file(&stored, p.get("format").as_deref()));
    let ext = ext_of(&path);
    if matches!(ext.as_str(), "pptx" | "odp") {
        return err("Slides are written by the output for slides: choose PowerPoint or Impress as the format, and connect each picture to it.");
    }
    if ext.is_empty() {
        return err(format!(
            "The file name {} has no extension. Choose a format, or use .gpkg, .shp, .geojson, .xlsx, .ods, .csv, .txt, .png, .jpg, .tif, .pdf, .obj or .glb.",
            path.display()
        ));
    }
    if matches!(ext.as_str(), "obj" | "glb") {
        // A 3D model: the one the stream carries, from a 3D file, Extrude or Generate TIN.
        let table = input(inp, "Input");
        if table.mesh.is_none() {
            return err(format!("The stream has no 3D model to write as .{}. Read an OBJ, glTF or IFC file, or make one with Extrude or Generate TIN.", ext));
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        let files = if ext == "obj" { crate::mesh_write::write_obj(table, &path)? } else { crate::mesh_write::write_glb(table, &path)? };
        let o = crate::mesh_write::origin_of(table.mesh.as_deref().expect("a model"));
        ctx.notes.info(format!(
            "{} triangles written around the origin {}, {}, {}, which is kept in the file",
            fmt_count(table.mesh.as_ref().map_or(0, |m| m.tris.len()) as u64),
            o[0],
            o[1],
            o[2]
        ));
        ctx.files.extend(files);
        return Ok(Vec::new());
    }
    if is_image_format(&ext) {
        // A picture: the image the stream carries, such as a map from Draw map.
        let table = input(inp, "Input");
        let values = table.grid.is_some() && matches!(ext.as_str(), "tif" | "tiff");
        if table.image.is_none() && !values && table.mesh.is_some() {
            return err(format!(
                "The stream is a 3D model, not a picture. Put Render 3D scene between them: it makes the picture, which is written as .{}. A 3D model itself is written as .glb or .obj.",
                ext
            ));
        }
        if table.image.is_none() && !values {
            return err(format!("The stream has no image to write as .{}. Draw one with Draw map, or read a raster with Cells as set to an image. A grid of values is written as .tif.", ext));
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        if p.get("mode").as_deref() == Some("append") {
            ctx.notes.warn("A picture cannot be added to, so it was written over.".to_string());
        }
        let b = ctx.backend(&format!("The format .{}", ext))?;
        let pdf = pdf_page(p)?;
        let files = b.write(table, &path, &WriteOpts { pdf, ..Default::default() }, &mut ctx.notes)?;
        ctx.files.extend(files);
        return Ok(Vec::new());
    }
    let (table, sizes) = apply_field_rules(input(inp, "Input"), &p.schema("fields"), &mut ctx.notes);
    let table = &table;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let geometry = match p.get("geometry").as_deref() {
        Some("xy") => GeomMode::Xy,
        Some("wkt") => GeomMode::Wkt,
        Some("auto") => GeomMode::Auto,
        _ => GeomMode::None,
    };
    let append = p.get("mode").as_deref() == Some("append");
    if append && path.exists() {
        ctx.notes.info("added to what was there".to_string());
    }
    if is_text_format(&ext) {
        let opts = CsvWriteOpts {
            delimiter: parse_delimiter(p.get("delimiter"))?.unwrap_or(';'),
            decimal_comma: p.flag("decimal_comma"),
            encoding: p.get("encoding").unwrap_or_else(|| "utf-8".into()),
            geometry,
            geographic: ctx.geographic(table.crs),
        };
        if append {
            csv_io::append_csv(table, &path, &opts, &mut ctx.notes)?;
        } else {
            csv_io::write_csv(table, &path, &opts, &mut ctx.notes)?;
        }
        ctx.files.push(path);
    } else if is_spreadsheet(&ext) {
        // A spreadsheet has no geometry, so it is written as ordinary columns.
        let flat = csv_io::geometry_as_columns(table, geometry, ctx.geographic(table.crs));
        let b = ctx.backend(&format!("The format .{}", ext))?;
        let sheet = SheetStyle {
            font: p.get("font"),
            bold_header: p.flag("bold_header"),
            freeze_header: p.flag("freeze"),
            colors: p.get("colors").filter(|c| SHEET_COLORS.iter().any(|x| x.0 == c)),
        };
        let files = b.write(&flat, &path, &WriteOpts { layer: p.get("layer"), encoding: None, sizes, sheet, append, ..Default::default() }, &mut ctx.notes)?;
        ctx.files.extend(files);
    } else {
        if matches!(ext.as_str(), "geojson" | "json") && table.crs.is_some_and(|c| c != 4326) {
            // Nothing wrong with it, but web maps and many other programs take for granted
            // that GeoJSON is in WGS 84.
            ctx.notes.info(format!("the coordinates are in {}, not in WGS 84 as GeoJSON usually is", crs_label(table.crs)));
        }
        let b = ctx.backend(&format!("The format .{}", ext))?;
        let key = if ext == "gpkg" { primary_key(p, "fid")? } else { None };
        let opts = WriteOpts { layer: p.get("layer"), encoding: p.get("encoding"), sizes, append, key, ..Default::default() };
        let files = b.write(table, &path, &opts, &mut ctx.notes)?;
        ctx.files.extend(files);
    }
    Ok(Vec::new())
}

// ---------- Slides (SlideWriter) ----------

pub(super) const SLIDE_SIZES: &[(&str, &str)] = &[("wide", "Wide, 16:9"), ("standard", "Standard, 4:3")];

pub(super) const SLIDE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "File",
        kind: ParamKind::FileSave,
        default: "",
        hint: "A .pptx file for PowerPoint or an .odp file for LibreOffice Impress",
    },
    ParamSpec { key: "format", label: "Format", kind: ParamKind::Choice(super::OUTPUT_FORMATS), default: "pptx", hint: "" },
    ParamSpec { key: "cover", label: "Title of the first slide", kind: ParamKind::Text, default: "", hint: "Empty: no first slide of its own" },
    ParamSpec { key: "cover_note", label: "Under the title", kind: ParamKind::Text, default: "", hint: "Such as a date, or who made it" },
    ParamSpec {
        key: "titles",
        label: "Titles above the pictures",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "The name of the tool each picture comes from, such as Draw map or Render 3D scene: name the tools as the slides should be called",
    },
    ParamSpec { key: "size", label: "Slides", kind: ParamKind::Choice(SLIDE_SIZES), default: "wide", hint: "" },
    ParamSpec { key: "background", label: "Background colour", kind: ParamKind::Text, default: "", hint: "As #rrggbb. Empty: white" },
];

pub(super) fn run_slides(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let parts: Vec<super::Part> = ctx.parts.iter().filter(|x| x.port == "Pictures").cloned().collect();
    if parts.is_empty() {
        return err("Connect pictures to Pictures, such as maps from Draw map, charts and 3D scenes: each connection is a slide.");
    }
    let stored = p.need("path", "Say where the slides should be written.")?;
    let format = p.get("format").filter(|f| f == "pptx" || f == "odp");
    let path = ctx.resolve(&writer_file(&stored, format.as_deref().or(Some("pptx"))));
    let ext = ext_of(&path);
    let slides: Vec<crate::slides::Slide> =
        parts.iter().filter_map(|x| x.table.image.as_deref().map(|img| crate::slides::Slide { title: x.label.clone(), picture: img })).collect();
    let left = parts.len() - slides.len();
    if left > 0 {
        ctx.notes
            .warn(format!("{} of the streams on Pictures have no picture and were left out. Draw one with Draw map, Draw chart or Render 3D scene.", left));
    }
    if slides.is_empty() {
        return err("None of the streams on Pictures has a picture.");
    }
    let background = match p.get("background").filter(|b| !b.trim().is_empty()) {
        None => None,
        Some(b) => match crate::mapdraw::parse_color(&b) {
            Ok(Some(c)) => Some([c[0], c[1], c[2]]),
            Ok(None) => None,
            Err(e) => return err(format!("The background colour: {}", e)),
        },
    };
    let deck = crate::slides::Deck {
        cover: p.exact("cover").trim().to_string(),
        cover_note: p.exact("cover_note").trim().to_string(),
        wide: p.get("size").as_deref() != Some("standard"),
        titles: p.get("titles").as_deref() != Some("false"),
        background,
    };
    if ext == "odp" {
        crate::slides::write_odp(&path, &slides, &deck)?;
    } else {
        crate::slides::write_pptx(&path, &slides, &deck)?;
    }
    ctx.notes.info(format!("{} slides written to {}", slides.len() + (!deck.cover.is_empty()) as usize, path.display()));
    ctx.files.push(path);
    Ok(Vec::new())
}

// ---------- PDF with layers ----------

pub(super) const LAYERED_PDF_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "path", label: "File", kind: ParamKind::FileSave, default: "", hint: "A .pdf file" },
    ParamSpec { key: "format", label: "Format", kind: ParamKind::Choice(super::OUTPUT_FORMATS), default: "geopdf", hint: "" },
    ParamSpec {
        key: "layers",
        label: "Layers",
        kind: ParamKind::MapLayers("Layers"),
        default: "",
        hint: "The top row is drawn on top. A layer that is not ticked is in the PDF, but off when it is opened",
    },
    ParamSpec {
        key: "crs",
        label: "Coordinate system",
        kind: ParamKind::Crs,
        default: "",
        hint: "Of the map. Empty uses that of the first layer. The others are reprojected to it",
    },
    ParamSpec { key: "margin", label: "Margin in %", kind: ParamKind::Text, default: "5", hint: "Room around the features, as a share of the larger side" },
    ParamSpec { key: "pdf_size", label: "Page size", kind: ParamKind::Choice(super::PDF_SIZES), default: "a4", hint: "" },
    ParamSpec { key: "pdf_width", label: "Page width in mm", kind: ParamKind::Text, default: "210", hint: "" },
    ParamSpec { key: "pdf_height", label: "Page height in mm", kind: ParamKind::Text, default: "297", hint: "" },
    ParamSpec { key: "pdf_orientation", label: "Orientation", kind: ParamKind::Choice(super::PDF_ORIENTATIONS), default: "auto", hint: "" },
    ParamSpec { key: "pdf_margin", label: "Page margin in mm", kind: ParamKind::Text, default: "10", hint: "" },
    ParamSpec { key: "pdf_title", label: "Title", kind: ParamKind::Text, default: "", hint: "Written above the map. Empty gives none" },
    ParamSpec { key: "pdf_title_align", label: "Title placed", kind: ParamKind::Choice(super::PDF_TITLE_ALIGN), default: "left", hint: "" },
];

pub(super) fn run_layered_pdf(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use super::{arrange_layers, map_layers, reproject, writer_file};
    let parts: Vec<super::Part> = ctx.parts.iter().filter(|x| x.port == "Layers").cloned().collect();
    if parts.is_empty() {
        return err("Connect the streams to Layers. Each connection is a layer of the PDF.");
    }
    let stored = p.need("path", "Say where the PDF should be written.")?;
    let path = ctx.resolve(&writer_file(&stored, Some("pdf")));
    let connected: Vec<String> = parts.iter().map(|x| x.from.clone()).collect();
    let rows = arrange_layers(&map_layers(p.values), &connected);
    let crs = match p.get("crs") {
        Some(c) => super::parse_crs(&c)?,
        None => parts.iter().find_map(|x| x.table.crs).ok_or_else(|| crate::model::GbError("No layer has a coordinate system. Set one for the PDF.".into()))?,
    };
    let b = ctx.backend("A PDF with layers")?;
    // From the bottom up, each in the coordinate system of the map.
    let mut tables: Vec<(String, Table, crate::mapdraw::LayerStyle, bool)> = Vec::new();
    for row in rows.iter().rev() {
        let Some(part) = parts.iter().find(|x| x.from == row.from) else { continue };
        // A layer with nothing to draw cannot be in the PDF.
        if part.table.image.is_none() && !part.table.features.iter().any(|f| f.geom.is_some()) {
            ctx.notes.info(format!("{} has nothing to draw and was left out", part.label));
            continue;
        }
        let t = match part.table.crs {
            Some(c) if c != crs => reproject(&part.table, crs, Some(b))?,
            _ => (*part.table).clone(),
        };
        // A name given in the form, or else the name of the stream.
        let name = if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() };
        tables.push((name, t, row.style.clone(), row.show));
    }
    if tables.is_empty() {
        return err("No layer has anything to draw.");
    }
    let layers: Vec<crate::io::PdfLayer> =
        tables.iter().map(|(n, t, s, v)| crate::io::PdfLayer { name: n.clone(), table: t, style: s.clone(), visible: *v }).collect();
    let page = super::pdf_page(p)?;
    let margin = (number_param(p, "margin", "The margin")?.unwrap_or(5.0) / 100.0).clamp(0.0, 1.0);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| crate::model::GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let files = b.write_layered_pdf(&layers, crs, &page, margin, &path, &mut ctx.notes)?;
    let shown: Vec<String> = layers.iter().rev().map(|l| l.name.clone()).collect();
    ctx.notes.info(format!("{} layers, from the top: {}", layers.len(), shown.join(", ")));
    ctx.files.extend(files);
    Ok(Vec::new())
}

pub(super) const WHICH: &[(&str, &str)] =
    &[("all", "Everything the outputs wrote"), ("named", "What the outputs named below wrote"), ("none", "Nothing, only the files below")];
pub(super) const PLACES: &[(&str, &str)] = &[("folders", "In their folders, as under the folder of the model"), ("flat", "All in the top of the zip file")];

pub(super) const ZIP_PARAMS: &[ParamSpec] = &[
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
pub(super) fn name_in_zip(file: &Path, base: &Path, flat: bool) -> String {
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
pub(super) fn with_companions(f: &Path) -> Vec<PathBuf> {
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

pub(super) fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
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

pub(super) fn run_zip(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
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

pub(super) fn file_name(p: &P) -> String {
    let Some(path) = p.get("path").map(|s| writer_file(&s, p.get("format").as_deref())) else { return "no file".into() };
    Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(path)
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "Viewer",
        title: "View",
        hidden: false,
        group: Group::Writer,
        category: "",
        desc: "A place to look at data: connect as many streams as you like to it, from anywhere in the model. Click it, and each connection is shown under the canvas, in the table and on the map, by its number and name. It writes nothing.",
        inputs: &["Input"],
        outputs: &[],
        ports: None,
        params: &[],
        run: run_viewer,
        summary: |_| "look at the data".into(),
    },
    NodeType {
        name: "ZipWriter",
        title: "Make zip file",
        hidden: false,
        group: Group::Writer,
        category: "",
        desc: "Packs what the outputs of the model wrote in the run into one zip file, all or those named, with other files and folders you choose, such as a description or terms of use. It runs when everything else in the model has run, and needs no connection.",
        inputs: &[],
        outputs: &[],
        ports: None,
        params: ZIP_PARAMS,
        run: run_zip,
        summary: |p| p.get("path").map(|x| x.rsplit(['/', '\\']).next().unwrap_or("").to_string()).unwrap_or_else(|| "no file".into()),
    },
    NodeType {
        name: "Writer",
        title: "Output",
        hidden: false,
        group: Group::Writer,
        category: "",
        desc: "Writes the stream to GeoPackage, Shapefile, GeoJSON, Excel, LibreOffice Calc, CSV or txt. A file that already exists is overwritten, but in a GeoPackage or a workbook only the layer or sheet that is written. A Shapefile holds one geometry type, so mixed streams are split into several files. A workbook can be given a font, a bold and locked top row and colours. A map from Draw map is written as PNG, JPG, GeoTIFF or a PDF page.",
        inputs: &["Input"],
        outputs: &[],
        ports: None,
        params: WRITER_PARAMS,
        run: run_writer,
        summary: file_name,
    },
    NodeType {
        name: "SlideWriter",
        title: "Slides",
        hidden: false,
        group: Group::Writer,
        category: "",
        desc: "Writes the pictures connected to Pictures as slides for PowerPoint (.pptx) or LibreOffice Impress (.odp), a slide for each: maps from Draw map, charts and 3D scenes, as large as fits, with the name of the tool each comes from as its title. A first slide with a title of its own, wide or standard slides and a background colour.",
        inputs: &["Pictures"],
        outputs: &[],
        ports: None,
        params: SLIDE_PARAMS,
        run: run_slides,
        summary: |p| p.get("path").map(|f| f.rsplit(['/', '\\']).next().unwrap_or("").to_string()).unwrap_or_default(),
    },
    NodeType {
        name: "LayeredPdfWriter",
        title: "GeoSpatial PDF output",
        hidden: true,
        group: Group::Writer,
        category: "",
        desc: "Writes the streams connected to Layers as a geospatial PDF where each is a layer that can be turned on and off in the PDF reader, with the attributes of the features. The order of the layers and how each looks are set in the form, as in Draw map. Pictures, such as shaded terrain, can be layers too.",
        inputs: &["Layers"],
        outputs: &[],
        ports: None,
        params: LAYERED_PDF_PARAMS,
        run: run_layered_pdf,
        summary: |p| file_name(p),
    },
];
