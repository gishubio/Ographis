//! The outputs that put several streams together into one document: slides, a report, a
//! GeoSpatial PDF with layers and a QGIS project.

use super::*;
use std::path::Path;

// ---------- Slides (SlideWriter) ----------

pub(in crate::nodes) const SLIDE_SIZES: &[(&str, &str)] = &[("wide", "Wide, 16:9"), ("standard", "Standard, 4:3")];

pub(in crate::nodes) const SLIDE_PARAMS: &[ParamSpec] = &[
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
        hint: "The name of the tool each picture comes from, or its comment when it has no name of its own",
    },
    ParamSpec { key: "size", label: "Slides", kind: ParamKind::Choice(SLIDE_SIZES), default: "wide", hint: "" },
    ParamSpec { key: "background", label: "Background colour", kind: ParamKind::Text, default: "", hint: "As #rrggbb. Empty: white" },
];

pub(in crate::nodes) fn run_slides(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let parts: Vec<super::Part> = ctx.parts.iter().filter(|x| x.port == "Pictures").cloned().collect();
    if parts.is_empty() {
        return err("Connect pictures to Pictures, such as maps from Draw map, charts and 3D scenes: each connection is a slide.");
    }
    let stored = p.need("path", "Say where the slides should be written.")?;
    let format = p.get("format").filter(|f| f == "pptx" || f == "odp");
    let path = ctx.resolve(&writer_file(&stored, format.as_deref().or(Some("pptx"))));
    let ext = ext_of(&path);
    let slides: Vec<crate::slides::Slide> =
        parts.iter().filter_map(|x| x.table.image.as_deref().map(|img| crate::slides::Slide { title: x.heading.clone(), picture: img })).collect();
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

// ---------- Report ----------

pub(in crate::nodes) const REPORT_FORMATS: &[(&str, &str)] = &[("html", "A web page (.html)"), ("pdf", "PDF of A4 pages (.pdf)")];

pub(in crate::nodes) const REPORT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "File",
        kind: ParamKind::FileSave,
        default: "",
        hint: "A web page (.html) with the pictures in it, or a PDF. {now('%Y-%m-%d')} puts the date in the name",
    },
    ParamSpec { key: "format", label: "Format", kind: ParamKind::Choice(REPORT_FORMATS), default: "html", hint: "" },
    ParamSpec { key: "title", label: "Title", kind: ParamKind::Text, default: "", hint: "Empty: the name of the file" },
    ParamSpec {
        key: "subtitle",
        label: "Under the title",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as who made it. Empty: the date and time of the run",
    },
    ParamSpec {
        key: "intro",
        label: "Text first",
        kind: ParamKind::Code(""),
        default: "",
        hint: "A blank line between paragraphs, # at the start of a line for a heading and - for a point in a list",
    },
    ParamSpec { key: "closing", label: "Text last", kind: ParamKind::Code(""), default: "", hint: "As the text first" },
    ParamSpec {
        key: "rows",
        label: "Rows of a table",
        kind: ParamKind::Text,
        default: "100",
        hint: "A stream without a picture is a table of its attributes, with at most this many rows",
    },
    ParamSpec {
        key: "headings",
        label: "Headings",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "Each section under the name of the tool it comes from, or its comment when the tool has no name of its own",
    },
    ParamSpec {
        key: "colours",
        label: "Colours of the tables",
        kind: ParamKind::Choice(crate::report::THEMES),
        default: "plain",
        hint: "The heading row and every other row, in the web page and the PDF",
    },
    ParamSpec {
        key: "sortable",
        label: "Sort by a click",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "Web page: a click on the heading of a column sorts the rows by it, a second click the other way. Only the rows in the table are sorted",
    },
];

pub(in crate::nodes) fn run_report(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let parts: Vec<super::Part> = ctx.parts.iter().filter(|x| x.port == "Sections").cloned().collect();
    let stored = p.need("path", "Say where the report should be written.")?;
    // The format chosen, or that of the name.
    let chosen = p.values.get("format").and_then(|v| v.as_str()).filter(|f| !f.is_empty()).map(str::to_string);
    let format = match chosen {
        Some(f) => if f == "pdf" { "pdf" } else { "html" }.to_string(),
        None => if ext_of(Path::new(&stored)) == "pdf" { "pdf" } else { "html" }.to_string(),
    };
    let named = match NameTemplate::parse(&stored, &[])? {
        Some(t) => t.name_for(&crate::model::Feature { z: None, attrs: Vec::new(), geom: None }, 0, None, true)?,
        None => stored,
    };
    let path = ctx.resolve(&writer_file(&named, Some(&format)));
    let most = number_param(p, "rows", "Rows of a table")?.unwrap_or(100.0).max(0.0) as usize;
    let headings = p.get("headings").as_deref() != Some("false");
    let mut blocks = Vec::new();
    let intro = p.exact("intro");
    if !intro.trim().is_empty() {
        blocks.push(crate::report::Block::Text(intro));
    }
    let (mut pictures, mut tables) = (0, 0);
    for part in &parts {
        if headings {
            blocks.push(crate::report::Block::Heading(part.heading.clone()));
        }
        let t = part.table.as_ref();
        if let Some(img) = t.image.as_deref() {
            blocks.push(crate::report::Block::Picture(img));
            pictures += 1;
            continue;
        }
        let numeric: Vec<bool> = t.fields.iter().map(|f| matches!(f.ty, crate::model::FieldType::Int | crate::model::FieldType::Real)).collect();
        let rows = t
            .features
            .iter()
            .take(most)
            .map(|f| {
                f.attrs
                    .iter()
                    .map(|v| match v {
                        crate::model::Value::Real(r) => crate::model::fmt_real(*r),
                        v => v.to_text(),
                    })
                    .collect()
            })
            .collect();
        blocks.push(crate::report::Block::Table(crate::report::TableBlock { fields: t.field_names(), numeric, rows, total: t.len() }));
        tables += 1;
    }
    let closing = p.exact("closing");
    if !closing.trim().is_empty() {
        blocks.push(crate::report::Block::Text(closing));
    }
    if blocks.is_empty() {
        return err("The report has nothing in it. Connect pictures or tables to Sections, or write a text.");
    }
    let title =
        p.get("title").filter(|t| !t.trim().is_empty()).unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let subtitle = p.get("subtitle").filter(|t| !t.trim().is_empty()).unwrap_or_else(|| ctx.now.format("%Y-%m-%d %H:%M").to_string());
    let theme = crate::report::theme(p.get("colours").as_deref().unwrap_or("plain"));
    let sortable = p.get("sortable").as_deref() != Some("false");
    let report = crate::report::Report { title, subtitle, blocks, theme, sortable };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    if format == "pdf" {
        crate::report::write_pdf(&report, &path)?;
    } else {
        crate::report::write_html(&report, &path)?;
    }
    let many = |n: usize, one: &str, more: &str| format!("{} {}", n, if n == 1 { one } else { more });
    ctx.notes.info(format!("{} and {} in the report", many(pictures, "picture", "pictures"), many(tables, "table", "tables")));
    ctx.files.push(path);
    Ok(Vec::new())
}

// ---------- PDF with layers ----------

pub(in crate::nodes) const LAYERED_PDF_PARAMS: &[ParamSpec] = &[
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

pub(in crate::nodes) fn run_layered_pdf(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
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

pub(in crate::nodes) const QGIS_BASEMAPS: &[(&str, &str)] = &[
    ("none", "None"),
    ("osm", "OpenStreetMap"),
    ("carto_light", "Light grey (CARTO Positron)"),
    ("carto_voyager", "Soft colours (CARTO Voyager)"),
    ("carto_dark", "Dark (CARTO Dark Matter)"),
    ("own", "One of my own, or an address"),
];

pub(in crate::nodes) const QGIS_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "File",
        kind: ParamKind::FileSave,
        default: "",
        hint: "A .qgz project. The features go in a GeoPackage of the same name beside it, and pictures in GeoTIFFs",
    },
    ParamSpec { key: "format", label: "Format", kind: ParamKind::Choice(super::OUTPUT_FORMATS), default: "qgis", hint: "" },
    ParamSpec {
        key: "layers",
        label: "Layers",
        kind: ParamKind::MapLayers("Layers"),
        default: "",
        hint: "The top row is on top in QGIS. A layer that is not ticked is in the project, but turned off",
    },
    ParamSpec {
        key: "crs",
        label: "Coordinate system",
        kind: ParamKind::Crs,
        default: "",
        hint: "Of the project. Empty uses that of the first layer. The others are reprojected to it",
    },
    ParamSpec { key: "basemap", label: "Background map", kind: ParamKind::Choice(QGIS_BASEMAPS), default: "none", hint: "Tiles from the web under the layers" },
    ParamSpec {
        key: "basemap_own",
        label: "Name or address",
        kind: ParamKind::Text,
        default: "",
        hint: "The name of one of your background maps of tiles under Settings, or an address with {z}, {x} and {y}",
    },
];

/// A name that GeoPackage and file names take: letters, digits and _.
fn plain_name(s: &str) -> String {
    let t: String = s.trim().chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    let t = t.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_");
    if t.is_empty() {
        "layer".into()
    } else {
        t
    }
}

pub(in crate::nodes) fn run_qgis(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use super::{arrange_layers, map_layers, reproject, writer_file};
    use crate::qgis_project::{Basemap, Project, ProjectLayer, Source};
    let parts: Vec<super::Part> = ctx.parts.iter().filter(|x| x.port == "Layers").cloned().collect();
    if parts.is_empty() {
        return err("Connect the streams to Layers. Each connection is a layer of the project.");
    }
    let stored = p.need("path", "Say where the QGIS project should be written.")?;
    let mut path = ctx.resolve(&writer_file(&stored, Some("qgis")));
    if ext_of(&path) == "qgs" {
        // A .qgs is written as it is named.
    } else if ext_of(&path) != "qgz" {
        path = path.with_extension("qgz");
    }
    let connected: Vec<String> = parts.iter().map(|x| x.from.clone()).collect();
    let rows = arrange_layers(&map_layers(p.values), &connected);
    let crs = match p.get("crs") {
        Some(c) => super::parse_crs(&c)?,
        None => parts.iter().find_map(|x| x.table.crs).ok_or_else(|| GbError("No layer has a coordinate system. Set one for the project.".into()))?,
    };
    let b = ctx.backend("A QGIS project")?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let gpkg_name = format!("{}.gpkg", stem);
    let gpkg = dir.join(&gpkg_name);
    // The GeoPackage is the project's own, and is made anew.
    let _ = std::fs::remove_file(&gpkg);
    let mut layers = Vec::new();
    let mut files = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut extent = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let grow = |e: &mut [f64; 4], r: [f64; 4]| {
        *e = [e[0].min(r[0]), e[1].min(r[1]), e[2].max(r[2]), e[3].max(r[3])];
    };
    for row in &rows {
        let Some(part) = parts.iter().find(|x| x.from == row.from) else { continue };
        let name = if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() };
        let t = match part.table.crs {
            Some(c) if c != crs => reproject(&part.table, crs, Some(b))?,
            _ => (*part.table).clone(),
        };
        let mut unique = |base: String| {
            let mut n = base.clone();
            let mut k = 2;
            while used.contains(&n) {
                n = format!("{}_{}", base, k);
                k += 1;
            }
            used.push(n.clone());
            n
        };
        // A picture or a raster of values: a GeoTIFF beside the project.
        if t.image.is_some() || t.grid.is_some() {
            let grey = t.image.is_none();
            let file = format!("{}_{}.tif", stem, unique(plain_name(&name)));
            files.extend(b.write(&t, &dir.join(&file), &crate::io::WriteOpts::default(), &mut ctx.notes)?);
            if let Some(img) = t.image.as_deref() {
                let gt = img.geotransform;
                grow(&mut extent, [gt[0], gt[3] + gt[5] * img.height as f64, gt[0] + gt[1] * img.width as f64, gt[3]]);
            }
            layers.push(ProjectLayer {
                name,
                source: Source::Picture { file, grey },
                visible: row.show,
                style: row.style.clone(),
                theme: Default::default(),
                spec: row.theme.clone(),
                labels: row.labels.clone(),
            });
            continue;
        }
        if !t.features.iter().any(|f| f.geom.is_some()) {
            ctx.notes.info(format!("{} has no geometry and was left out", part.label));
            continue;
        }
        // Points, lines and areas each a layer of their own, as QGIS wants them.
        let families =
            [(crate::geom::Family::Area, "polygon", "areas"), (crate::geom::Family::Line, "line", "lines"), (crate::geom::Family::Point, "point", "points")];
        let present: Vec<_> =
            families.iter().filter(|(f, _, _)| t.features.iter().any(|x| x.geom.as_ref().is_some_and(|g| crate::geom::family(g) == *f))).collect();
        for (family, geometry, word) in &present {
            let mut sub = t.like();
            sub.fields = t.fields.clone();
            sub.features = t.features.iter().filter(|x| x.geom.as_ref().is_some_and(|g| crate::geom::family(g) == *family)).cloned().collect();
            for f in &sub.features {
                if let Some(r) = f.geom.as_ref().and_then(geo::BoundingRect::bounding_rect) {
                    grow(&mut extent, [r.min().x, r.min().y, r.max().x, r.max().y]);
                }
            }
            let shown = if present.len() > 1 { format!("{} ({})", name, word) } else { name.clone() };
            let gpkg_layer = unique(plain_name(&shown));
            let opts = crate::io::WriteOpts { layer: Some(gpkg_layer.clone()), ..Default::default() };
            files.extend(b.write(&sub, &gpkg, &opts, &mut ctx.notes)?);
            let theme = crate::thematic::prepare(&sub, &row.style, &row.theme).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?;
            let size_range = sub.field_index(row.theme.size_by.trim()).and_then(|i| {
                let v: Vec<f64> = sub.features.iter().filter_map(|f| f.attrs[i].as_f64()).collect();
                (!v.is_empty()).then(|| v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x))))
            });
            layers.push(ProjectLayer {
                name: shown,
                source: Source::Vector { gpkg_layer, geometry, size_range },
                visible: row.show,
                style: row.style.clone(),
                theme,
                spec: row.theme.clone(),
                labels: row.labels.clone(),
            });
        }
    }
    if layers.is_empty() {
        return err("No layer has anything to put in the project.");
    }
    if !extent[0].is_finite() {
        extent = [0.0, 0.0, 1.0, 1.0];
    }
    // Some room around the layers.
    let pad = ((extent[2] - extent[0]).max(extent[3] - extent[1]) * 0.05).max(1e-6);
    let extent = [extent[0] - pad, extent[1] - pad, extent[2] + pad, extent[3] + pad];
    let basemap = match p.get("basemap").as_deref() {
        None | Some("none") | Some("") => None,
        Some(which) => {
            let name = if which == "own" {
                p.need("basemap_own", "Write the name of one of your background maps, or an address with {z}, {x} and {y}.")?
            } else {
                which.to_string()
            };
            let m = crate::basemap::find(&name)?;
            if m.kind != "xyz" {
                ctx.notes.warn(format!("{} is a {} service, and only maps of tiles are put in the project. Add it in QGIS.", m.name, m.kind.to_uppercase()));
                None
            } else {
                Some(Basemap { name: m.name, url: m.url })
            }
        }
    };
    let project = Project { title: stem.clone(), crs, crs_wkt: b.crs_wkt(crs), extent, gpkg: &gpkg_name, layers, basemap };
    crate::qgis_project::write_project(&project, &path)?;
    let names: Vec<String> = project.layers.iter().map(|l| l.name.clone()).collect();
    ctx.notes.info(format!("{} {} in the project, from the top: {}", names.len(), if names.len() == 1 { "layer" } else { "layers" }, names.join(", ")));
    ctx.files.push(path);
    files.sort();
    files.dedup();
    ctx.files.extend(files);
    Ok(Vec::new())
}
