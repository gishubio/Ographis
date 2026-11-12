//! Output and what writes or shows: Output, View, Make zip file and GeoSpatial PDF with layers.

use super::*;
use crate::model::{crs_label, err, fmt_count, GbError, Result, Table};
use std::path::Path;

mod documents;
mod packing;
mod webmap;
use documents::*;
use packing::*;
use webmap::*;

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
pub(super) const CITYGML_LODS: &[(&str, &str)] = &[("2", "LoD2: ground, walls and roofs"), ("1", "LoD1: one solid")];
/// The formats a writer can be told to write: (file extension, label).
pub const WRITER_FORMATS: &[(&str, &str)] = &[
    ("", "From the file name"),
    ("csv", "CSV (.csv)"),
    ("xlsx", "Excel (.xlsx)"),
    ("fgb", "FlatGeobuf (.fgb)"),
    ("geojson", "GeoJSON (.geojson)"),
    ("parquet", "GeoParquet (.parquet)"),
    ("gpkg", "GeoPackage (.gpkg)"),
    ("gml", "GML 3.2 (.gml)"),
    ("gpx", "GPX for GPS units (.gpx)"),
    ("gsheet", "Google Sheets (the address of the sheet)"),
    ("kml", "KML for Google Earth (.kml)"),
    ("kmz", "KMZ for Google Earth (.kmz)"),
    ("tif", "GeoTIFF image (.tif)"),
    ("cog", "Cloud Optimized GeoTIFF (.tif)"),
    ("citygml", "CityGML 2.0 buildings (.gml)"),
    ("glb", "glTF 3D model (.glb)"),
    ("usda", "OpenUSD (.usda)"),
    ("usdz", "OpenUSD package with textures (.usdz)"),
    ("3dtiles", "3D Tiles 1.1 (a folder with tileset.json)"),
    ("jpg", "JPEG image (.jpg)"),
    ("json", "JSON: a list of objects (.json)"),
    ("landxml", "LandXML surface and points (.xml)"),
    ("las", "LAS point cloud (.las)"),
    ("laz", "LAZ point cloud (.laz)"),
    ("ply", "Gaussian splats or points, PLY (.ply)"),
    ("spz", "Gaussian splats, compressed SPZ (.spz)"),
    ("ods", "LibreOffice Calc (.ods)"),
    ("obj", "OBJ 3D model (.obj)"),
    ("pdf", "PDF (.pdf)"),
    ("png", "PNG image (.png)"),
    ("pmtiles", "PMTiles vector tiles (.pmtiles)"),
    ("shp", "Shapefile (.shp)"),
    ("svg", "SVG drawing of a map (.svg)"),
    ("txt", "Text (.txt)"),
    ("xml", "XML (.xml)"),
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
    ("parquet", "GeoParquet (.parquet)"),
    ("gpkg", "GeoPackage (.gpkg)"),
    ("gml", "GML 3.2 (.gml)"),
    ("gpx", "GPX for GPS units (.gpx)"),
    ("gsheet", "Google Sheets (the address of the sheet)"),
    ("kml", "KML for Google Earth (.kml)"),
    ("kmz", "KMZ for Google Earth (.kmz)"),
    ("geopdf", "GeoSpatial PDF with layers (.pdf)"),
    ("tif", "GeoTIFF image (.tif)"),
    ("cog", "Cloud Optimized GeoTIFF (.tif)"),
    ("citygml", "CityGML 2.0 buildings (.gml)"),
    ("glb", "glTF 3D model (.glb)"),
    ("usda", "OpenUSD (.usda)"),
    ("usdz", "OpenUSD package with textures (.usdz)"),
    ("3dtiles", "3D Tiles 1.1 (a folder with tileset.json)"),
    ("jpg", "JPEG image (.jpg)"),
    ("json", "JSON: a list of objects (.json)"),
    ("landxml", "LandXML surface and points (.xml)"),
    ("las", "LAS point cloud (.las)"),
    ("laz", "LAZ point cloud (.laz)"),
    ("ply", "Gaussian splats or points, PLY (.ply)"),
    ("spz", "Gaussian splats, compressed SPZ (.spz)"),
    ("ods", "LibreOffice Calc (.ods)"),
    ("obj", "OBJ 3D model (.obj)"),
    ("pdf", "PDF (.pdf)"),
    ("png", "PNG image (.png)"),
    ("pmtiles", "PMTiles vector tiles (.pmtiles)"),
    ("svg", "SVG drawing of a map (.svg)"),
    ("postgis", "PostGIS table"),
    ("qgis", "QGIS project with its layers (.qgz)"),
    ("webmap", "Web map with Leaflet or MapLibre (a folder)"),
    ("pptx", "PowerPoint slides (.pptx)"),
    ("odp", "LibreOffice Impress slides (.odp)"),
    ("shp", "Shapefile (.shp)"),
    ("txt", "Text (.txt)"),
    ("xml", "XML (.xml)"),
];

/// The tool that writes a format of OUTPUT_FORMATS.
pub fn output_kind(format: &str) -> &'static str {
    match format {
        "geopdf" => "LayeredPdfWriter",
        "qgis" => "QgisProjectWriter",
        "webmap" => "WebMapWriter",
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
/// How a picture is written, from the form.
fn image_opts(p: &P, table: &Table) -> Result<crate::io::ImageOpts> {
    // Given here, or by the picture: a map drawn for print knows its resolution.
    let own = table.field_index("dpi").and_then(|k| table.features.first().and_then(|f| f.attrs.get(k)).and_then(|v| v.as_f64()));
    let dpi = number_param(p, "img_dpi", "Pixels to the inch")?.or(own);
    if dpi.is_some_and(|d| d <= 0.0 || d > 10_000.0) {
        return err("Pixels to the inch is a number from 1 to 10 000, such as 300.");
    }
    let quality = number_param(p, "img_quality", "JPEG quality")?.unwrap_or(92.0).clamp(1.0, 100.0) as u8;
    let compress = p.get("tif_compress").filter(|c| matches!(c.as_str(), "deflate" | "lzw" | "jpeg" | "none")).unwrap_or_else(|| "deflate".into());
    Ok(crate::io::ImageOpts { dpi, quality, compress })
}

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
    ParamSpec {
        key: "gltf_name",
        label: "Object names",
        kind: ParamKind::Expression("Input"),
        default: "",
        hint: "glTF: the name of each object in Blender, such as the attribute name or address. Empty: as the model has them",
    },
    ParamSpec {
        key: "gltf_group",
        label: "Group by",
        kind: ParamKind::Expression("Input"),
        default: "",
        hint: "glTF: the objects of each value under a parent of its own, such as the kind of building, to show and hide together in Blender",
    },
    ParamSpec {
        key: "gltf_material",
        label: "Materials by",
        kind: ParamKind::Expression("Input"),
        default: "",
        hint: "glTF: a material named after each value, such as the material of the roof, to give them all a new look at once in Blender",
    },
    ParamSpec {
        key: "gltf_origin",
        label: "Origin",
        kind: ParamKind::Text,
        default: "",
        hint: "glTF and OBJ: x, y and z the model is written around, the same for files that are to fit together, such as 418300, 6472550, 0. none, or 0, 0, 0: the coordinates as they are, without moving the model. Empty: near its middle",
    },
    ParamSpec {
        key: "gltf_georef",
        label: "Where it lies",
        kind: ParamKind::Choice(&[("true", "Kept for BlenderGIS: the coordinate system and the origin"), ("false", "Not kept")]),
        default: "true",
        hint: "glTF: BlenderGIS can then add maps and terrain in their place",
    },
    ParamSpec {
        key: "gltf_sun",
        label: "Sun and camera",
        kind: ParamKind::Choice(&[("none", "None"), ("sun", "A sun"), ("both", "A sun and a camera"), ("camera", "A camera")]),
        default: "none",
        hint: "glTF: a sun where it stands at the time below, and a camera looking from the south-west",
    },
    ParamSpec {
        key: "gltf_when",
        label: "Time of the sun",
        kind: ParamKind::Text,
        default: "",
        hint: "Swedish time, such as 2026-06-21 15:00. Empty: midsummer at three in the afternoon",
    },
    ParamSpec {
        key: "t3_most",
        label: "Triangles in a tile",
        kind: ParamKind::Text,
        default: "100000",
        hint: "3D Tiles: the objects are split into tiles of at most this many triangles, which load quickly",
    },
    ParamSpec {
        key: "t3_dz",
        label: "Height added",
        kind: ParamKind::Text,
        default: "0",
        hint: "3D Tiles: metres added to every height. The globe has heights above the ellipsoid: in Sweden add about 20 to 40 m to heights above the sea",
    },
    ParamSpec {
        key: "citygml_lod",
        label: "Level of detail",
        kind: ParamKind::Choice(CITYGML_LODS),
        default: "2",
        hint: "CityGML: LoD1 is each building as one solid, LoD2 its ground, walls and roofs",
    },
    ParamSpec {
        key: "pm_min",
        label: "From zoom level",
        kind: ParamKind::Text,
        default: "0",
        hint: "PMTiles: the smallest zoom level with tiles. 0 is the whole world in one tile",
    },
    ParamSpec {
        key: "pm_max",
        label: "To zoom level",
        kind: ParamKind::Text,
        default: "14",
        hint: "PMTiles: the largest, at most 16. 14 has tiles of about 2 km, and maps zoom in beyond it",
    },
    ParamSpec {
        key: "connection",
        label: "API connection",
        kind: ParamKind::ApiConnection,
        default: "",
        hint: "Google Sheets: a connection with the key of a Google service account that the sheet is shared with, as an editor",
    },
    ParamSpec {
        key: "splat_up",
        label: "Upwards in the file",
        kind: ParamKind::Choice(super::input::SPLAT_UP),
        default: "auto",
        hint: "Gaussian splats: y down for PLY and y up for SPZ, as most viewers expect, or z up as on maps",
    },
    ParamSpec {
        key: "usd_layers",
        label: "Layers by",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "OpenUSD: a layer for each value, such as buildings, roads and trees, each a file the main file takes in. Empty: one",
    },
    ParamSpec {
        key: "usd_variants",
        label: "Variants by",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "OpenUSD: a variant for each value, such as today and plan, to switch between in the 3D program. Empty: none",
    },
    ParamSpec { key: "xml_root", label: "Outer element", kind: ParamKind::Text, default: "features", hint: "XML: the element around all of it" },
    ParamSpec { key: "xml_row", label: "Element of a feature", kind: ParamKind::Text, default: "feature", hint: "XML: the element of each feature. Names with a dot become elements within it" },
    ParamSpec {
        key: "xml_geometry",
        label: "Geometry in XML",
        kind: ParamKind::Choice(&[("none", "Left out"), ("wkt", "As WKT, in geometry")]),
        default: "none",
        hint: "",
    },
    ParamSpec {
        key: "json_shape",
        label: "JSON as",
        kind: ParamKind::Choice(&[
            ("list", "A list of objects, one for each feature"),
            ("lines", "JSON Lines: an object on each line"),
            ("first", "The object of the first feature alone"),
        ]),
        default: "list",
        hint: "Names with a dot become objects within the object, and attributes that hold JSON, such as from Build JSON, are written as JSON",
    },
    ParamSpec {
        key: "json_list_name",
        label: "List under the name",
        kind: ParamKind::Text,
        default: "",
        hint: "Puts the list in an object under this name, such as {\"items\": [...]}. Empty writes the list as it is",
    },
    ParamSpec {
        key: "json_geometry",
        label: "Geometry in JSON",
        kind: ParamKind::Choice(&[("none", "Left out"), ("geojson", "As GeoJSON, in geometry"), ("wkt", "As WKT, in geometry")]),
        default: "none",
        hint: "In the coordinates of the stream",
    },
    ParamSpec {
        key: "schema_files",
        label: "Schema and TypeScript types",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "Writes name.schema.json (JSON Schema) and name.d.ts (TypeScript) beside the file, with each attribute, its type and whether it can be empty, for those who use the file in code",
    },
    ParamSpec {
        key: "geojson_crs",
        label: "Coordinates",
        kind: ParamKind::Choice(&[("wgs84", "In WGS 84, as GeoJSON is to be"), ("keep", "In the coordinate system of the stream")]),
        default: "wgs84",
        hint: "GeoJSON is in degrees in WGS 84 (RFC 7946), which web maps take for granted. Other coordinate systems are named in the file in the older way",
    },
    ParamSpec {
        key: "gpkg_version",
        label: "GeoPackage version",
        kind: ParamKind::Choice(&[("", "As GDAL chooses"), ("1.4", "1.4"), ("1.3", "1.3"), ("1.2", "1.2, for older programs")]),
        default: "",
        hint: "Of a new GeoPackage. GDAL chooses the oldest version that holds the data. A file that is there keeps its own",
    },
    ParamSpec {
        key: "img_dpi",
        label: "Pixels to the inch",
        kind: ParamKind::Text,
        default: "",
        hint: "PNG, JPEG and GeoTIFF: how large the picture prints, kept in the file, such as 300. It does not change the picture, and makes it no sharper: for that, draw it for print in Draw map, which then gives its resolution here by itself. Empty takes that of the picture, or none",
    },
    ParamSpec {
        key: "img_quality",
        label: "JPEG quality",
        kind: ParamKind::Text,
        default: "92",
        hint: "1 to 100: more is sharper and larger. For JPEG, and GeoTIFF packed as JPEG",
    },
    ParamSpec {
        key: "tif_compress",
        label: "GeoTIFF packed as",
        kind: ParamKind::Choice(&[("deflate", "Deflate, lossless"), ("lzw", "LZW, lossless, read by the most programs"), ("jpeg", "JPEG, small but lossy"), ("none", "Not packed")]),
        default: "deflate",
        hint: "For pictures, also Cloud Optimized. JPEG suits aerial photos, not maps with lines and text",
    },
    ParamSpec {
        key: "pdf_as",
        label: "A map as",
        kind: ParamKind::Choice(&[
            ("shapes", "Lines, areas and text as shapes, sharp at any size, placed on the map"),
            ("picture", "A picture of pixels, placed on the map"),
        ]),
        default: "shapes",
        hint: "For a map from Draw map or a chart. As shapes it prints sharp and can be edited in Illustrator or Inkscape; as a picture a GIS can place it",
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
    let format = match format.as_str() {
        // A Google sheet is an address, not a file.
        "gsheet" => return path.to_string(),
        "landxml" => "xml".to_string(),
        "geopdf" => "pdf".to_string(),
        "qgis" => "qgz".to_string(),
        "cog" => "tif".to_string(),
        "citygml" => "gml".to_string(),
        // A web map is a folder, or a page that ends in .html.
        "webmap" => {
            let ext = ext_of(Path::new(path));
            return if matches!(ext.as_str(), "html" | "htm") {
                path.to_string()
            } else if WRITER_FORMATS.iter().any(|f| !f.0.is_empty() && f.0 == ext) || ext == "gml" {
                Path::new(path).with_extension("").to_string_lossy().into_owned()
            } else {
                path.to_string()
            };
        }
        // A tileset is a folder: the name without an extension.
        "3dtiles" => {
            let ext = ext_of(Path::new(path));
            return if WRITER_FORMATS.iter().any(|f| !f.0.is_empty() && f.0 == ext) || ext == "gml" {
                Path::new(path).with_extension("").to_string_lossy().into_owned()
            } else {
                path.to_string()
            };
        }
        _ => format,
    };
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

/// The settings of a glTF file for Blender, with the values of the features evaluated.
fn gltf_opts(p: &P, table: &Table, ctx: &NodeCtx) -> Result<crate::mesh_write::GltfOpts> {
    let values = |key: &str, label: &str| -> Result<Option<Vec<String>>> {
        let Some(text) = p.get(key).filter(|t| !t.trim().is_empty()) else { return Ok(None) };
        let e = crate::expr::compile(&text, &table.field_names()).map_err(|e| GbError(format!("{}: {}", label, e.0)))?;
        let geographic = ctx.geographic(table.crs);
        let mut out = Vec::with_capacity(table.len());
        for (i, f) in table.features.iter().enumerate() {
            out.push(e.eval(&crate::expr::Row { feature: f, index: i, geographic })?.to_text());
        }
        Ok(Some(out))
    };
    let sun_camera = p.get("gltf_sun").unwrap_or_else(|| "none".into());
    let sun = if matches!(sun_camera.as_str(), "sun" | "both") {
        let when = p.get("gltf_when").filter(|w| !w.trim().is_empty());
        let (date, time) = match &when {
            Some(w) => {
                let mut parts = w.split_whitespace();
                (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("15:00").to_string())
            }
            None => (format!("{}-06-21", chrono::Local::now().format("%Y")), "15:00".to_string()),
        };
        let at = crate::sun::time_on(&date, &time, crate::sun::Clock::Swedish)
            .ok_or_else(|| GbError(format!("The time of the sun is written as 2026-06-21 15:00, not \"{}\".", when.unwrap_or_default())))?;
        // The place of the model in degrees, for where the sun stands.
        let middle = table.mesh.as_deref().and_then(|m| m.bounds()).map(|(lo, hi)| ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0)).or_else(|| {
            use geo::BoundingRect;
            let r = table.features.iter().filter_map(|f| f.geom.as_ref()?.bounding_rect()).reduce(|a, b| {
                geo::Rect::new(
                    geo::Coord { x: a.min().x.min(b.min().x), y: a.min().y.min(b.min().y) },
                    geo::Coord { x: a.max().x.max(b.max().x), y: a.max().y.max(b.max().y) },
                )
            })?;
            Some(((r.min().x + r.max().x) / 2.0, (r.min().y + r.max().y) / 2.0))
        });
        let lonlat = match (middle, table.crs, ctx.backend) {
            (Some((x, y)), Some(4326), _) => Some((x, y)),
            (Some((x, y)), Some(c), Some(b)) => {
                let (mut xs, mut ys) = (vec![x], vec![y]);
                b.transform(c, 4326, &mut xs, &mut ys).ok().map(|_| (xs[0], ys[0]))
            }
            _ => None,
        };
        // Without a place, Göteborg.
        let (lon, lat) = lonlat.unwrap_or((11.97, 57.71));
        let (az, alt) = crate::sun::position(at, lat, lon);
        Some((az, alt))
    } else {
        None
    };
    Ok(crate::mesh_write::GltfOpts {
        georef: p.get("gltf_georef").as_deref() != Some("false"),
        names: values("gltf_name", "Object names")?,
        groups: values("gltf_group", "Group by")?,
        kinds: values("gltf_material", "Materials by")?,
        sun,
        camera: matches!(sun_camera.as_str(), "camera" | "both"),
        origin: model_origin(p)?,
    })
}

/// The origin a 3D model is written around, as Origin gives it: None near its middle, and
/// [0, 0, 0] for none, that is the coordinates as they are.
fn model_origin(p: &P) -> Result<Option<[f64; 3]>> {
    Ok(match p.get("gltf_origin").filter(|o| !o.trim().is_empty()) {
        None => None,
        Some(text) if matches!(text.trim().to_lowercase().as_str(), "none" | "no" | "off") => Some([0.0; 3]),
        Some(text) => {
            let v: Vec<f64> = text.split([',', ';', ' ']).filter(|t| !t.trim().is_empty()).filter_map(|t| crate::model::parse_number(t.trim())).collect();
            match v.as_slice() {
                [x, y] => Some([*x, *y, 0.0]),
                [x, y, z] => Some([*x, *y, *z]),
                _ => return err(format!("The origin is written as x, y, z, such as 418300, 6472550, 0, or none, not \"{}\".", text)),
            }
        }
    })
}

pub(super) fn run_writer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    // A point cloud is written as it is to LAS, LAZ, PLY and SPZ; to other formats as features.
    if let Some(t) = inp.get("Input").filter(|t| t.points.is_some()) {
        let stored = p.get("path").unwrap_or_default();
        let ext = ext_of(Path::new(&writer_file(&stored, p.get("format").as_deref())));
        if !matches!(ext.as_str(), "las" | "laz" | "ply" | "spz") || stored.contains('{') {
            let mut inp = inp.clone();
            inp.insert("Input", crate::cloud::as_features(t, &mut ctx.notes));
            return run_writer_files(p, &inp, ctx);
        }
    }
    run_writer_files(p, inp, ctx)
}

fn run_writer_files(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    if let Some(out) = split_by_name(p, inp, ctx)? {
        return Ok(out);
    }
    match p.get("format").as_deref() {
        Some("postgis") => return err("PostGIS is written by the type PostgisWriter. Choose PostGIS table as the format in the program, and it switches."),
        Some("geopdf") => return err("A GeoSpatial PDF is written by the type LayeredPdfWriter. Choose the format in the program, and it switches."),
        Some("qgis") => return err("A QGIS project is written by the type QgisProjectWriter. Choose the format in the program, and it switches."),
        _ => {}
    }
    let stored = p.need("path", "The output has no file. Say where the result should be written.")?;
    if p.get("format").as_deref() == Some("gsheet") || crate::gsheets::is_sheet(&stored) {
        return write_sheet(p, input(inp, "Input"), &stored, ctx);
    }
    let path = ctx.resolve(&writer_file(&stored, p.get("format").as_deref()));
    let ext = ext_of(&path);
    if p.get("format").as_deref() == Some("json") {
        return write_json(p, input(inp, "Input"), &path, ctx);
    }
    if p.get("format").as_deref() == Some("landxml") {
        let table = input(inp, "Input");
        let text = crate::landxml::write(table, table.crs)?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        std::fs::write(&path, text).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        let tris = table.mesh.as_deref().map_or(0, |m| m.tris.len());
        ctx.notes.info(format!("written as LandXML: {} triangles", fmt_count(tris as u64)));
        ctx.files.push(path.clone());
        return Ok(Vec::new());
    }
    if ext == "xml" && input(inp, "Input").mesh.is_none() {
        let table = input(inp, "Input");
        let root = p.get("xml_root").filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "features".into());
        let row = p.get("xml_row").filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "feature".into());
        let text = crate::xml::write(table, &root, &row, p.get("xml_geometry").as_deref() == Some("wkt"));
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        std::fs::write(&path, text).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        if table.is_empty() {
            ctx.notes.warn("No features came in, so the XML has only its outer element.");
        }
        ctx.notes.info(format!("{} features written as XML", fmt_count(table.len() as u64)));
        ctx.files.push(path.clone());
        return Ok(Vec::new());
    }
    if p.get("format").as_deref() == Some("3dtiles") {
        // 3D Tiles: a folder with tileset.json and the tiles, placed on the globe.
        let table = input(inp, "Input");
        let backend = ctx.backend.ok_or_else(|| GbError("3D Tiles needs the GDAL support to place the model on the globe.".into()))?;
        let o = crate::tiles3d_write::TilesOpts {
            most: number_param(p, "t3_most", "Triangles in a tile")?.unwrap_or(100_000.0).max(1000.0) as usize,
            dz: number_param(p, "t3_dz", "Height added")?.unwrap_or(0.0),
        };
        let (files, tiles) = crate::tiles3d_write::write_tileset(table, &path, &o, backend)?;
        let tris = table.mesh.as_deref().map_or(0, |m| m.tris.len());
        ctx.notes.info(format!(
            "{} triangles written as 3D Tiles in {} tiles, in the folder {}",
            fmt_count(tris as u64),
            fmt_count(tiles as u64),
            path.display()
        ));
        if o.dz == 0.0 {
            ctx.notes.info("The heights are taken as heights above the ellipsoid. Heights above the sea, such as RH 2000, lie about 20 to 40 metres too low on the globe of Cesium: add that in Height added.");
        }
        ctx.files.extend(files);
        return Ok(Vec::new());
    }
    if ext == "gml" && (p.get("format").as_deref() == Some("citygml") || input(inp, "Input").mesh.is_some()) {
        let table = input(inp, "Input");
        let lod = number_param(p, "citygml_lod", "Level of detail")?.unwrap_or(2.0) as u8;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        let n = crate::citygml_write::write_citygml(table, &path, lod)?;
        if table.crs.is_none() {
            ctx.notes.warn("The buildings have no coordinate system, so the file says none.");
        }
        ctx.notes.info(format!("{} buildings written as CityGML 2.0 in LoD{}", fmt_count(n as u64), lod.clamp(1, 2)));
        ctx.files.push(path);
        return Ok(Vec::new());
    }
    if matches!(ext.as_str(), "pptx" | "odp") {
        return err("Slides are written by the output for slides: choose PowerPoint or Impress as the format, and connect each picture to it.");
    }
    if ext.is_empty() {
        return err(format!(
            "The file name {} has no extension. Choose a format, or use .gpkg, .shp, .geojson, .xlsx, .ods, .csv, .txt, .png, .jpg, .tif, .pdf, .obj, .glb or .laz.",
            path.display()
        ));
    }
    if ext == "parquet" {
        // GeoParquet, written by Ographis itself.
        let table = input(inp, "Input");
        let projjson = table.crs.filter(|c| *c != 4326).and_then(|c| ctx.backend.and_then(|b| b.crs_projjson(c)));
        let n = crate::parquet_io::write_parquet(table, &path, projjson.as_deref())?;
        ctx.notes.info(format!("{} rows written as GeoParquet", fmt_count(n)));
        ctx.files.push(path);
        return Ok(Vec::new());
    }
    if ext == "pmtiles" {
        // Vector tiles, cut by Ographis itself, in WGS 84.
        let table = input(inp, "Input");
        let table = if table.crs.is_some_and(|c| c != 4326) { reproject(table, 4326, ctx.backend)? } else { table.clone() };
        if table.crs.is_none() && table.has_geometry() {
            ctx.notes.warn("The features have no coordinate system, and were taken to be in WGS 84.");
        }
        let zoom = |key: &str, label: &str, d: f64| -> Result<u8> { Ok(number_param(p, key, label)?.unwrap_or(d).clamp(0.0, 16.0) as u8) };
        let (min_zoom, max_zoom) = (zoom("pm_min", "From zoom level", 0.0)?, zoom("pm_max", "To zoom level", 14.0)?);
        let layer = p
            .get("layer")
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "layer".into()));
        let gate = ctx.gate;
        let progress = ctx.progress;
        let o = crate::pmtiles_write::PmOpts { min_zoom, max_zoom, layer: layer.clone() };
        let s = crate::pmtiles_write::write_pmtiles(&table, &path, &o, &|i, n| progress("zoom levels", i, Some(n)), &|| gate.going())?;
        ctx.notes.info(format!(
            "{} tiles of zoom {} to {} in the layer {}, {} of them different; the largest {} kB",
            fmt_count(s.tiles),
            o.min_zoom.min(o.max_zoom),
            o.max_zoom.max(o.min_zoom),
            layer,
            fmt_count(s.contents),
            crate::model::fmt_real((s.largest as f64 / 102.4).round() / 10.0)
        ));
        if s.largest > 500 * 1024 {
            ctx.notes
                .warn("Some tiles are larger than 500 kB, which maps draw slowly. Start from a higher zoom level, or simplify the features with Generalize.");
        }
        ctx.files.push(path);
        return Ok(Vec::new());
    }
    if matches!(ext.as_str(), "las" | "laz") {
        // A point cloud: points with heights, with the attributes of LAS that they have.
        let table = input(inp, "Input");
        let wkt = table.crs.and_then(|c| ctx.backend.and_then(|b| b.crs_wkt(c)));
        if table.crs.is_some() && wkt.is_none() {
            ctx.notes.warn("The coordinate system could not be written into the point cloud without the GDAL support.");
        }
        let (n, flat) = crate::pointcloud::write_las(table, &path, wkt.as_deref())?;
        ctx.notes.info(format!("{} points written", fmt_count(n)));
        if flat > 0 {
            ctx.notes.warn(format!("{} points had no height and were written at 0.", fmt_count(flat)));
        }
        ctx.files.push(path);
        return Ok(Vec::new());
    }
    if matches!(ext.as_str(), "ply" | "spz") {
        // Gaussian splats, or points: around an origin, in the axes viewers expect.
        let table = input(inp, "Input");
        let (cloud, _) = crate::cloud::cloud_of(table);
        if cloud.is_empty() {
            return err("Nothing came in to write as splats or points.");
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        let axes = crate::splats::Axes::of(p.get("splat_up").as_deref().filter(|u| *u != "auto"), &ext);
        let origin = match model_origin(p)? {
            Some(o) => o,
            None => {
                let n = cloud.len() as f64;
                let (x, y) = (cloud.x.iter().sum::<f64>() / n, cloud.y.iter().sum::<f64>() / n);
                let low = cloud.z.iter().copied().filter(|z| z.is_finite()).fold(f64::INFINITY, f64::min);
                [x.round(), y.round(), if low.is_finite() { low.floor() } else { 0.0 }]
            }
        };
        let dropped = if ext == "spz" {
            crate::splats::write_spz(&cloud, &path, axes, origin)?
        } else {
            crate::splats::write_ply(&cloud, &path, axes, origin, table.crs)?
        };
        let what = if cloud.splat.is_some() { "Gaussian splats" } else { "points" };
        ctx.notes.info(format!("{} {} written around the origin {}, {}, {}", fmt_count(cloud.len() as u64), what, origin[0], origin[1], origin[2]));
        if dropped {
            ctx.notes.warn("The splats were turned, so their colours by the side they are seen from were left out: the colour of each is kept.");
        }
        ctx.files.push(path);
        return Ok(Vec::new());
    }
    if matches!(ext.as_str(), "usda" | "usdz") {
        // OpenUSD: the 3D model, or the lines and points, with layers and variants.
        let table = input(inp, "Input");
        let field = |key: &str| -> Result<Option<usize>> { p.get(key).filter(|v| !v.trim().is_empty()).map(|v| table.need_field(&v)).transpose() };
        let opts = crate::usd_write::UsdOpts { layer_by: field("usd_layers")?, variant_by: field("usd_variants")?, origin: model_origin(p)? };
        if table.mesh.is_none() && table.has_geometry() && ctx.geographic(table.crs) == Some(true) {
            ctx.notes.warn("The lines and points are in degrees: reproject them to metres first, or they come out tiny and flat in a 3D program.");
        }
        let files = crate::usd_write::write_usd(table, &path, &opts)?;
        ctx.notes.info(format!(
            "{} objects written as OpenUSD{}",
            fmt_count(table.mesh.as_ref().map_or(table.len(), |m| m.objects.len()) as u64),
            if files.len() > 1 { format!(", in {} files", files.len()) } else { String::new() }
        ));
        ctx.files.extend(files);
        return Ok(Vec::new());
    }
    if matches!(ext.as_str(), "obj" | "glb") {
        // A 3D model: the one the stream carries, from a 3D file, Extrude or Generate TIN.
        let table = input(inp, "Input");
        if ext == "glb" {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
            }
            let opts = gltf_opts(p, table, ctx)?;
            if table.mesh.is_none() && table.has_geometry() && ctx.geographic(table.crs) == Some(true) {
                ctx.notes.warn("The lines and points are in degrees: reproject them to metres first, or they come out tiny and flat in a 3D program.");
            }
            let files = crate::mesh_write::write_glb_with(table, &path, &opts)?;
            match table.mesh.as_deref() {
                Some(m) => {
                    let o = opts.origin.unwrap_or_else(|| crate::mesh_write::origin_of(m));
                    // glTF keeps coordinates as 32-bit numbers, of about seven digits.
                    let far = m.positions.iter().map(|q| (q[0] - o[0]).abs().max((q[1] - o[1]).abs()).max((q[2] - o[2]).abs())).fold(0.0, f64::max);
                    if far > 100_000.0 {
                        ctx.notes.warn(format!(
                            "The model reaches {} m from its origin. glTF keeps coordinates with about seven digits, so they are only good to {} there: give an origin near the model, or leave Origin empty.",
                            fmt_count(far as u64),
                            if far > 1_000_000.0 { "decimetres" } else { "centimetres" }
                        ));
                    }
                    ctx.notes.info(format!(
                        "{} triangles written around the origin {}, {}, {}, which is kept in the file",
                        fmt_count(m.tris.len() as u64),
                        o[0],
                        o[1],
                        o[2]
                    ));
                }
                None => ctx.notes.info(format!("{} lines and points written as a 3D model", fmt_count(table.len() as u64))),
            }
            ctx.files.extend(files);
            return Ok(Vec::new());
        }
        if table.mesh.is_none() {
            return err(format!("The stream has no 3D model to write as .{}. Read an OBJ, glTF or IFC file, or make one with Extrude or Generate TIN.", ext));
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        let origin = model_origin(p)?;
        let files = if ext == "obj" { crate::mesh_write::write_obj_around(table, &path, origin)? } else { crate::mesh_write::write_glb(table, &path)? };
        let o = origin.unwrap_or_else(|| crate::mesh_write::origin_of(table.mesh.as_deref().expect("a model")));
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
        // A map from Draw map as shapes: written here, without GDAL.
        let shapes = table.image.as_ref().and_then(|i| i.vector.clone());
        if ext == "svg" || (ext == "pdf" && shapes.is_some() && p.get("pdf_as").as_deref() != Some("picture")) {
            let Some(pic) = shapes else {
                return err(
                    "Only a map from Draw map or a chart can be written as .svg, since they are kept as lines and areas. Other pictures are written as .png.",
                );
            };
            let bytes = if ext == "svg" {
                crate::vector::svg(&pic).into_bytes()
            } else {
                let page = pdf_page(p)?;
                let l = page.layout(pic.width as usize, pic.height as usize);
                let heading = page.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| (t, l.title_at.0, l.page_h - l.title_at.1));
                let geo = vector_geo(ctx, table, &pic);
                if geo.is_none() && table.crs.is_some() {
                    ctx.notes
                        .warn("The corners of the map could not be put in degrees, so the PDF has no coordinates. Choose PDF as picture for one that has.");
                }
                crate::vector::pdf(&pic, l.page_w, l.page_h, l.left, l.top, l.scale, heading, geo.as_ref())
            };
            std::fs::write(&path, bytes).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
            let n = pic.ops.len();
            let placed = ext == "pdf" && table.crs.is_some() && table.image.as_ref().is_some_and(|i| i.geotransform[1] != 0.0);
            ctx.notes.info(format!(
                "drawn as {} shapes{}",
                fmt_count(n as u64),
                if ext == "pdf" {
                    if placed {
                        ", sharp at any size, with coordinates"
                    } else {
                        ", sharp at any size"
                    }
                } else {
                    ""
                }
            ));
            ctx.files.push(path.clone());
            return Ok(Vec::new());
        }
        if ext == "pdf" && table.image.is_some() && p.get("pdf_as").as_deref() != Some("picture") {
            ctx.notes.info("the picture is not a map from Draw map, so the PDF holds it as pixels");
        }
        let b = ctx.backend(&format!("The format .{}", ext))?;
        let pdf = pdf_page(p)?;
        let cog = p.get("format").as_deref() == Some("cog") && matches!(ext.as_str(), "tif" | "tiff");
        // Pictures over time: a file for each, named by its time.
        if let (Some(series), true) = (&table.series, values) {
            if series.len() > 1 {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                for (k, g) in series.iter().enumerate() {
                    let when = g.meta.time.as_deref().map(|t| t.get(..10).unwrap_or(t).replace(':', "-")).unwrap_or_else(|| (k + 1).to_string());
                    let one = path.with_file_name(format!("{}_{}.{}", stem, when, ext));
                    let one_table = g.clone().into_table();
                    let files = b.write(&one_table, &one, &WriteOpts { cog, image: image_opts(p, &one_table)?, ..Default::default() }, &mut ctx.notes)?;
                    ctx.files.extend(files);
                }
                ctx.notes.info(format!("{} pictures, a file for each time", series.len()));
                return Ok(Vec::new());
            }
        }
        let files = b.write(table, &path, &WriteOpts { pdf, cog, image: image_opts(p, table)?, ..Default::default() }, &mut ctx.notes)?;
        if cog {
            ctx.notes.info("written as a Cloud Optimized GeoTIFF, with overviews");
        }
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
        // GeoJSON is in WGS 84 (RFC 7946), unless the coordinate system is to be kept.
        let wgs84;
        let table = if matches!(ext.as_str(), "geojson" | "json") && table.crs.is_some_and(|c| c != 4326) {
            if p.get("geojson_crs").as_deref() == Some("keep") {
                ctx.notes.info(format!("the coordinates are in {}, not in WGS 84 as GeoJSON is to be", crs_label(table.crs)));
                table
            } else {
                ctx.notes.info(format!("reprojected from {} to WGS 84, as GeoJSON is to be", crs_label(table.crs)));
                wgs84 = reproject(table, 4326, ctx.backend)?;
                &wgs84
            }
        } else {
            table
        };
        let b = ctx.backend(&format!("The format .{}", ext))?;
        let key = if ext == "gpkg" { primary_key(p, "fid")? } else { None };
        let gpkg_version = p.get("gpkg_version").filter(|v| matches!(v.as_str(), "1.2" | "1.3" | "1.4"));
        let opts = WriteOpts { layer: p.get("layer"), encoding: p.get("encoding"), sizes, append, key, gpkg_version, ..Default::default() };
        let files = b.write(table, &path, &opts, &mut ctx.notes)?;
        ctx.files.extend(files);
        if p.flag("schema_files") && matches!(ext.as_str(), "geojson" | "json") {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
            let (schema, ts) = crate::typedefs::for_geojson(table, &stem);
            write_beside(&path, &stem, &schema, &ts, ctx)?;
        }
    }
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
    NodeType {
        name: "ReportWriter",
        title: "Report",
        hidden: false,
        group: Group::Writer,
        category: "",
        desc: "Writes a report of what the model made, as a web page with the pictures in it or as a PDF of A4 pages: a title, a text first and last of your own, and a section for each stream connected to Sections, in the order they were connected, under the name of the tool it comes from. Maps, charts and 3D scenes are shown as pictures; other streams as tables of their attributes.",
        inputs: &["Sections"],
        outputs: &[],
        ports: None,
        params: REPORT_PARAMS,
        run: run_report,
        summary: |p| file_name(p),
    },
    NodeType {
        name: "QgisProjectWriter",
        title: "QGIS project output",
        hidden: true,
        group: Group::Writer,
        category: "",
        desc: "Writes the streams connected to Layers as a QGIS project (.qgz), with the features in a GeoPackage beside it and pictures as GeoTIFF: each a layer with the look it has in the form, as in Draw map, with its classes of colour, sizes after a number and labels, and a background map if asked. To open in QGIS and go on from there.",
        inputs: &["Layers"],
        outputs: &[],
        ports: None,
        params: QGIS_PARAMS,
        run: run_qgis,
        summary: |p| file_name(p),
    },
    NodeType {
        name: "WebMapWriter",
        title: "Web map output",
        hidden: true,
        group: Group::Writer,
        category: "",
        desc: "Writes the streams connected to Layers as a web map: a folder with index.html, to open in a browser or put on a web server. Leaflet or MapLibre GL draws it, with each layer as it looks in the form, as in Draw map, with its classes of colour, sizes after a number and labels; a background map; a legend that turns the layers on and off; and the attributes of a feature when it is clicked. The features go in WGS 84 beside the page, as GeoJSON, which also opens from the disk, or for large layers with MapLibre as PMTiles, which need a web server.",
        inputs: &["Layers"],
        outputs: &[],
        ports: None,
        params: WEBMAP_PARAMS,
        run: run_webmap,
        summary: |p| file_name(p),
    },
];

/// Where a map drawn as shapes lies, for a PDF with coordinates: its corners from the
/// placement of the picture, in degrees. None when the map has no coordinate system, or no
/// backend can put the corners in degrees.
fn vector_geo(ctx: &mut NodeCtx, table: &Table, pic: &crate::vector::Picture) -> Option<crate::vector::Geo> {
    let epsg = table.crs?;
    let gt = table.image.as_ref()?.geotransform;
    if gt[1] == 0.0 || gt[5] == 0.0 {
        return None;
    }
    let (w, h) = (pic.width as f64, pic.height as f64);
    let at = |col: f64, row: f64| (gt[0] + col * gt[1] + row * gt[2], gt[3] + col * gt[4] + row * gt[5]);
    // Lower left, upper left, upper right, lower right.
    let corners = [at(0.0, h), at(0.0, 0.0), at(w, 0.0), at(w, h)];
    let mut xs: Vec<f64> = corners.iter().map(|c| c.0).collect();
    let mut ys: Vec<f64> = corners.iter().map(|c| c.1).collect();
    let b = ctx.backend("A PDF with coordinates").ok()?;
    if epsg != 4326 {
        b.transform(epsg, 4326, &mut xs, &mut ys).ok()?;
    }
    let wkt = b.crs_wkt(epsg);
    // Degrees as x is longitude and y latitude; the PDF wants latitude first.
    let corners = [(ys[0], xs[0]), (ys[1], xs[1]), (ys[2], xs[2]), (ys[3], xs[3])];
    corners.iter().all(|(la, lo)| la.is_finite() && lo.is_finite()).then_some(crate::vector::Geo { epsg, wkt, corners })
}

/// The stream as plain JSON: a list of objects, JSON Lines or one object, with the
/// attributes, names with a dot as objects, and the geometry if asked.
fn write_json(p: &P, table: &Table, path: &Path, ctx: &mut NodeCtx) -> Result<Outputs> {
    let idx: Vec<usize> = (0..table.fields.len()).collect();
    let geometry = p.get("json_geometry").unwrap_or_else(|| "none".into());
    let shape = p.get("json_shape").unwrap_or_else(|| "list".into());
    let objects: Vec<serde_json::Value> = table.features.iter().map(|f| crate::nodes::json::feature_object(table, f, &idx, true, true, &geometry)).collect();
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let n = objects.len();
    let text = match shape.as_str() {
        "lines" => {
            let mut t = String::new();
            for o in &objects {
                t.push_str(&serde_json::to_string(o).map_err(|e| GbError(e.to_string()))?);
                t.push('\n');
            }
            t
        }
        "first" => {
            if n > 1 {
                ctx.notes.warn(format!("{} features came in; only the first is written, as one object.", fmt_count(n as u64)));
            }
            serde_json::to_string_pretty(objects.first().unwrap_or(&serde_json::Value::Object(Default::default()))).map_err(|e| GbError(e.to_string()))?
        }
        _ => {
            let v = crate::nodes::json::list_under(objects, &p.get("json_list_name").unwrap_or_default());
            serde_json::to_string_pretty(&v).map_err(|e| GbError(e.to_string()))?
        }
    };
    let append = p.get("mode").as_deref() == Some("append") && shape == "lines";
    if append {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        f.write_all(text.as_bytes()).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    } else {
        if p.get("mode").as_deref() == Some("append") {
            ctx.notes.warn("A JSON list cannot be added to, so it was written over. JSON Lines can be.".to_string());
        }
        std::fs::write(path, text).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    if n == 0 {
        ctx.notes.warn("No features came in, so the JSON is empty.");
    }
    if p.flag("schema_files") {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
        let (schema, ts) = crate::typedefs::for_json(table, &stem, &shape, &p.get("json_list_name").unwrap_or_default(), &geometry);
        write_beside(path, &stem, &schema, &ts, ctx)?;
    }
    ctx.notes.info(format!("{} objects written as JSON{}", fmt_count(n as u64), if append { ", added at the end" } else { "" }));
    ctx.files.push(path.to_path_buf());
    Ok(Vec::new())
}

/// The stream to a Google sheet, through the API with a service account: over what is
/// there, or after it.
fn write_sheet(p: &P, table: &Table, url: &str, ctx: &mut NodeCtx) -> Result<Outputs> {
    if !crate::gsheets::is_sheet(url) {
        return err("Write the address of the Google sheet as the file, such as https://docs.google.com/spreadsheets/d/…/edit, as the browser shows it.");
    }
    let name = p.get("connection").filter(|c| !c.trim().is_empty()).ok_or_else(|| {
        GbError("Writing to Google Sheets needs an API connection with the key of a Google service account that the sheet is shared with. Make one under Edit → Settings → API connections.".into())
    })?;
    let conn = crate::web::connection(&name)?;
    if conn.auth != "google" {
        return err(format!("The API connection {} is not a Google service account. Choose one that holds the key of a service account.", name));
    }
    let b = ctx.backend("Writing to Google Sheets")?;
    let gate = ctx.gate;
    let stopped = move || !gate.going();
    let append = p.get("mode").as_deref() == Some("append");
    let sheet = p.get("layer");
    let (n, title) = crate::gsheets::write(table, url, conn, sheet.as_deref(), append, b, &stopped, &mut ctx.notes)?;
    if n == 0 {
        ctx.notes.warn("No features came in, so the sheet has only the names of the attributes.");
    }
    ctx.notes.info(format!("{} rows {} the sheet {} in Google Sheets", fmt_count(n as u64), if append { "added to" } else { "written to" }, title));
    Ok(Vec::new())
}

/// The JSON Schema and the TypeScript types of a file, written beside it.
fn write_beside(path: &Path, stem: &str, schema: &str, ts: &str, ctx: &mut NodeCtx) -> Result<()> {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let (s, t) = (dir.join(format!("{}.schema.json", stem)), dir.join(format!("{}.d.ts", stem)));
    std::fs::write(&s, schema).map_err(|e| GbError(format!("Cannot write {}: {}", s.display(), e)))?;
    std::fs::write(&t, ts).map_err(|e| GbError(format!("Cannot write {}: {}", t.display(), e)))?;
    ctx.notes.info(format!("the schema and the types beside it: {}.schema.json and {}.d.ts", stem, stem));
    ctx.files.push(s);
    ctx.files.push(t);
    Ok(())
}
