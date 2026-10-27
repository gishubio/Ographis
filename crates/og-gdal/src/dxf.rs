//! DXF from CAD: the entities through GDAL, with what CAD knows of them made into plain
//! attributes (layer, kind of entity, colour, line type, text and its height, block and the
//! values of its attributes), and what the drawing says of itself read from its header.

use crate::{gerr, open, read_layer};
use gdal::vector::LayerAccess;
use og_engine::io::{Notes, ReadOpts};
use og_engine::model::{Field, FieldType, Result, Table, Value};
use std::path::Path;

/// The kind of entity, as CAD names it, from the subclasses GDAL gives.
fn entity_of(subclasses: &str) -> &'static str {
    let last = subclasses.rsplit(':').next().unwrap_or("");
    match last {
        "AcDbLine" => "LINE",
        "AcDbPolyline" => "LWPOLYLINE",
        "AcDb2dPolyline" | "AcDb3dPolyline" | "AcDbPolyFaceMesh" | "AcDbPolygonMesh" => "POLYLINE",
        "AcDbCircle" => "CIRCLE",
        "AcDbArc" => "ARC",
        "AcDbEllipse" => "ELLIPSE",
        "AcDbSpline" => "SPLINE",
        "AcDbText" => "TEXT",
        "AcDbMText" => "MTEXT",
        "AcDbAttribute" => "ATTRIB",
        "AcDbAttributeDefinition" => "ATTDEF",
        "AcDbBlockReference" => "INSERT",
        "AcDbHatch" => "HATCH",
        "AcDbPoint" => "POINT",
        "AcDbFace" => "3DFACE",
        "AcDbTrace" => "TRACE",
        "AcDbSolid" | "AcDb3dSolid" => "SOLID",
        "AcDbLeader" => "LEADER",
        _ if subclasses.contains("AcDbDimension") => "DIMENSION",
        _ if subclasses.contains("AcDbArc") => "ARC",
        _ if subclasses.contains("AcDbCircle") => "CIRCLE",
        _ => "",
    }
}

/// A value from an OGR style string, such as c from PEN(c:#ff0000,w:0.5g).
fn style_value<'a>(style: &'a str, tool: &str, key: &str) -> Option<&'a str> {
    let start = style.find(&format!("{}(", tool))? + tool.len() + 1;
    let body = &style[start..];
    let end = body.find(')').unwrap_or(body.len());
    let body = &body[..end];
    let mut depth_quote = false;
    let mut from = 0;
    let bytes = body.as_bytes();
    for i in 0..=bytes.len() {
        if i < bytes.len() && bytes[i] == b'"' {
            depth_quote = !depth_quote;
        }
        if i == bytes.len() || (bytes[i] == b',' && !depth_quote) {
            let part = &body[from..i];
            if let Some(v) = part.strip_prefix(&format!("{}:", key)) {
                return Some(v.trim_matches('"'));
            }
            from = i + 1;
        }
    }
    None
}

/// The number at the start of a value of a style, such as 2.5 in 2.5g.
fn style_number(v: &str) -> Option<f64> {
    let t: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    t.parse().ok()
}

/// What the header of a DXF says: the version of AutoCAD, the units, the extent and more,
/// as (name, value) for the log. Binary DXF and files without a header give nothing.
pub(crate) fn header(path: &Path) -> Vec<(String, String)> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(2_000_000)]);
    let mut lines = text.lines().map(str::trim);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_header = false;
    let mut var: Option<String> = None;
    let mut point: Vec<String> = Vec::new();
    while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
        match (code, value) {
            ("2", "HEADER") => in_header = true,
            ("0", "ENDSEC") if in_header => {
                if let Some(v) = var.take() {
                    if !point.is_empty() {
                        out.push((v, point.join(", ")));
                    }
                }
                break;
            }
            ("9", name) if in_header => {
                if let Some(v) = var.take() {
                    if !point.is_empty() {
                        out.push((v, point.join(", ")));
                    }
                }
                point.clear();
                var = Some(name.to_string());
            }
            (c, v) if in_header && var.is_some() => {
                let c: i32 = c.parse().unwrap_or(-1);
                if (10..=39).contains(&c) {
                    point.push(v.to_string());
                } else {
                    out.push((var.take().unwrap_or_default(), v.to_string()));
                    point.clear();
                }
            }
            _ => {}
        }
    }
    out
}

fn version_name(acadver: &str) -> String {
    let name = match acadver {
        "AC1009" => "AutoCAD R12",
        "AC1012" => "AutoCAD R13",
        "AC1014" => "AutoCAD R14",
        "AC1015" => "AutoCAD 2000",
        "AC1018" => "AutoCAD 2004",
        "AC1021" => "AutoCAD 2007",
        "AC1024" => "AutoCAD 2010",
        "AC1027" => "AutoCAD 2013",
        "AC1032" => "AutoCAD 2018",
        _ => return acadver.to_string(),
    };
    format!("{} ({})", name, acadver)
}

/// The units of $INSUNITS.
pub(crate) fn units_name(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        "1" => "inches",
        "2" => "feet",
        "3" => "miles",
        "4" => "millimetres",
        "5" => "centimetres",
        "6" => "metres",
        "7" => "kilometres",
        "14" => "decimetres",
        _ => return None,
    })
}

/// Reads a DXF file: the entities, with blocks as points that carry their attributes, or
/// drawn out in full.
pub(crate) fn read(path: &Path, opts: &ReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let set = |k: &str, v: &str| gdal::config::set_thread_local_config_option(k, v).map_err(|e| gerr("Cannot set up the reading of DXF", e));
    set("DXF_INLINE_BLOCKS", if opts.dxf_blocks_as_points { "FALSE" } else { "TRUE" })?;
    set("DXF_CLOSED_LINE_AS_POLYGON", if opts.dxf_areas { "TRUE" } else { "FALSE" })?;
    set("DXF_MERGE_BLOCK_GEOMETRIES", "FALSE")?;
    let result = (|| -> Result<Table> {
        let ds = open(path, false, &[])?;
        let mut layer = ds.layer_by_name("entities").or_else(|_| ds.layer(0)).map_err(|e| gerr("The DXF has no entities", e))?;
        if opts.crs.is_none() {
            // DXF has no coordinate system: it is given in the input. Without it the drawing
            // is read all the same, to be looked at; the size of the coordinates may tell
            // which it is.
            let hint = layer
                .get_extent()
                .ok()
                .and_then(|e| og_engine::io::guess_crs([e.MinX, e.MinY, e.MaxX, e.MaxY]))
                .map(|g| format!(" {}.", g))
                .unwrap_or_default();
            notes.warn(format!(
                "DXF does not say which coordinate system it is in. It is read without one, so it can be looked at and written, but not reprojected or put on a background map; choose it under Coordinate system in the input.{}",
                hint
            ));
        }
        let opts = ReadOpts { allow_no_crs: true, ..opts.clone() };
        let raw = read_layer(&mut layer, &opts, notes, progress)?;
        // The styles, in the same order: colour, line weight, the height and turn of text.
        layer.reset_feature_reading();
        let styles: Vec<String> = layer
            .features()
            .map(|f| {
                // SAFETY: the feature is valid while `f` lives, and GDAL owns the text.
                unsafe {
                    let p = gdal_sys::OGR_F_GetStyleString(f.c_feature());
                    if p.is_null() {
                        String::new()
                    } else {
                        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
                    }
                }
            })
            .take(raw.len())
            .collect();
        Ok(tidy(raw, &styles, opts.dxf_areas))
    })();
    let _ = gdal::config::clear_thread_local_config_option("DXF_INLINE_BLOCKS");
    let _ = gdal::config::clear_thread_local_config_option("DXF_CLOSED_LINE_AS_POLYGON");
    let _ = gdal::config::clear_thread_local_config_option("DXF_MERGE_BLOCK_GEOMETRIES");
    let mut t = result?;
    // What the drawing says of itself.
    let head = header(path);
    let get = |k: &str| head.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let mut said = Vec::new();
    if let Some(v) = get("$ACADVER") {
        said.push(version_name(&v));
    }
    if let Some(u) = get("$INSUNITS") {
        match units_name(&u) {
            Some(name) => said.push(format!("drawn in {}", name)),
            None if u.trim() == "0" => said.push("drawn without units".to_string()),
            None => {}
        }
    }
    if let Some(by) = get("$LASTSAVEDBY").filter(|v| !v.is_empty()) {
        said.push(format!("last saved by {}", by));
    }
    if let Some(cp) = get("$DWGCODEPAGE") {
        said.push(format!("code page {}", cp));
    }
    if !said.is_empty() {
        notes.info(said.join(", "));
    }
    // The coordinates: DXF has no coordinate system, so it is given in the input.
    let mut ext = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for f in &t.features {
        if let Some(r) = f.geom.as_ref().and_then(geo::BoundingRect::bounding_rect) {
            ext = [ext[0].min(r.min().x), ext[1].min(r.min().y), ext[2].max(r.max().x), ext[3].max(r.max().y)];
        }
    }
    if ext[0].is_finite() {
        notes.info(format!("coordinates from {:.1}, {:.1} to {:.1}, {:.1}", ext[0], ext[1], ext[2], ext[3]));
    }
    t.crs = opts.crs;
    Ok(t)
}

/// The attributes as CAD people know them, from what GDAL gives.
fn tidy(raw: Table, styles: &[String], areas: bool) -> Table {
    let at = |n: &str| raw.field_index(n);
    let (i_layer, i_sub, i_type, i_handle, i_text) = (at("Layer"), at("SubClasses"), at("Linetype"), at("EntityHandle"), at("Text"));
    let (i_block, i_angle, i_scale, i_attrs, i_paper) = (at("BlockName"), at("BlockAngle"), at("BlockScale"), at("BlockAttributes"), at("PaperSpace"));
    let (i_def_block, i_tag) = (at("Block"), at("AttributeTag"));
    let text = |f: &og_engine::model::Feature, i: Option<usize>| i.map(|k| f.attrs[k].clone()).unwrap_or(Value::Null);
    // The tags of the attributes of blocks, in the order they are first met.
    let mut tags: Vec<String> = Vec::new();
    let parse_attrs = |v: &Value| -> Vec<(String, String)> {
        // A list as GDAL writes it: (2:ID B-12,DJUP 2.4), or the items one per line.
        let s = v.to_text();
        let body = s.trim().trim_start_matches('(').trim_end_matches(')');
        let body = body.split_once(':').map_or(body, |(n, rest)| if n.chars().all(|c| c.is_ascii_digit()) { rest } else { body });
        body.split(',').filter_map(|item| item.trim().split_once(' ').map(|(k, v)| (k.to_string(), v.to_string()))).collect()
    };
    if let Some(k) = i_attrs {
        for f in &raw.features {
            for (tag, _) in parse_attrs(&f.attrs[k]) {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
    }
    let mut fields = vec![
        Field::new("layer", FieldType::Text),
        Field::new("entity", FieldType::Text),
        Field::new("colour", FieldType::Text),
        Field::new("linetype", FieldType::Text),
        Field::new("line_weight", FieldType::Real),
        Field::new("handle", FieldType::Text),
        Field::new("text", FieldType::Text),
        Field::new("text_height", FieldType::Real),
        Field::new("text_angle", FieldType::Real),
        Field::new("block", FieldType::Text),
        Field::new("block_angle", FieldType::Real),
        Field::new("block_scale", FieldType::Real),
        Field::new("attribute_tag", FieldType::Text),
        Field::new("paper_space", FieldType::Int),
    ];
    let taken: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
    let tag_names: Vec<String> = tags.iter().map(|t| if taken.iter().any(|n| n.eq_ignore_ascii_case(t)) { format!("attr_{}", t) } else { t.clone() }).collect();
    for n in &tag_names {
        fields.push(Field::new(n.clone(), FieldType::Text));
    }
    let mut features = Vec::with_capacity(raw.features.len());
    for (k, f) in raw.features.iter().enumerate() {
        let style = styles.get(k).map(|s| s.as_str()).unwrap_or("");
        let num = |v: Option<&str>| v.and_then(style_number).map_or(Value::Null, Value::Real);
        let tool = if style.starts_with("LABEL") {
            "LABEL"
        } else if style.contains("BRUSH(") && !style.contains("PEN(") {
            "BRUSH"
        } else {
            "PEN"
        };
        let colour = style_value(style, tool, if tool == "BRUSH" { "fc" } else { "c" }).map_or(Value::Null, |c| Value::Text(c.to_string()));
        let sub = text(f, i_sub).to_text();
        let block = text(f, i_block).to_text();
        let mut attrs = vec![
            text(f, i_layer),
            if sub.is_empty() { Value::Null } else { Value::Text(entity_of(&sub).to_string()).filter_empty() },
            colour,
            text(f, i_type),
            if tool == "PEN" { num(style_value(style, "PEN", "w")) } else { Value::Null },
            text(f, i_handle),
            text(f, i_text),
            if tool == "LABEL" { num(style_value(style, "LABEL", "s")) } else { Value::Null },
            if tool == "LABEL" { num(style_value(style, "LABEL", "a")) } else { Value::Null },
            if block.is_empty() { text(f, i_def_block) } else { Value::Text(block) },
            text(f, i_angle),
            i_scale.map_or(Value::Null, |k| {
                let s = f.attrs[k].to_text();
                let body = s.trim().trim_start_matches('(').trim_end_matches(')');
                let body = body.split_once(':').map_or(body, |(_, r)| r);
                body.split(',').next().and_then(|x| x.trim().parse::<f64>().ok()).map_or(Value::Null, Value::Real)
            }),
            text(f, i_tag),
            Value::Int(i_paper.is_some_and(|k| f.attrs[k].as_f64().is_some_and(|v| v != 0.0)) as i64),
        ];
        let own = i_attrs.map(|k| parse_attrs(&f.attrs[k])).unwrap_or_default();
        for t in &tags {
            attrs.push(own.iter().find(|(k, _)| k == t).map_or(Value::Null, |(_, v)| Value::Text(v.clone())));
        }
        // A closed polyline, circle or ellipse is an area, when areas are asked for. GDAL
        // before 3.9 gives them as lines.
        let closed_kind = matches!(entity_of(&sub), "LWPOLYLINE" | "POLYLINE" | "CIRCLE" | "ELLIPSE" | "SPLINE");
        let geom = match &f.geom {
            Some(geo::Geometry::LineString(l)) if areas && closed_kind && l.0.len() >= 4 && l.0.first() == l.0.last() => {
                Some(geo::Geometry::Polygon(geo::Polygon::new(l.clone(), Vec::new())))
            }
            g => g.clone(),
        };
        features.push(og_engine::model::Feature { attrs, geom, z: f.z.clone() });
    }
    // Columns no feature has a value in are left out, so that a drawing without blocks or
    // text has no columns for them.
    let used: Vec<bool> = (0..fields.len()).map(|i| i < 2 || features.iter().any(|f| !f.attrs[i].is_null() && f.attrs[i] != Value::Int(0))).collect();
    let fields: Vec<Field> = fields.into_iter().zip(&used).filter(|(_, u)| **u).map(|(f, _)| f).collect();
    for f in &mut features {
        let mut k = 0;
        f.attrs.retain(|_| {
            let keep = used[k];
            k += 1;
            keep
        });
    }
    Table { fields, crs: raw.crs, features, raster: false, image: None, grid: None, mesh: None }
}

trait FilterEmpty {
    fn filter_empty(self) -> Value;
}

impl FilterEmpty for Value {
    fn filter_empty(self) -> Value {
        match &self {
            Value::Text(t) if t.is_empty() => Value::Null,
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_and_entities_are_read() {
        let s = "PEN(c:#00ff00,p:\"1.27g 0.254g\",w:0.35g)";
        assert_eq!(style_value(s, "PEN", "c"), Some("#00ff00"));
        assert_eq!(style_value(s, "PEN", "w").and_then(style_number), Some(0.35));
        assert_eq!(style_value("LABEL(f:\"Arial\",t:\"A, B\",s:2.5g,c:#000000)", "LABEL", "s"), Some("2.5g"));
        assert_eq!(entity_of("AcDbEntity:AcDbPolyline"), "LWPOLYLINE");
        assert_eq!(entity_of("AcDbEntity:AcDbBlockReference"), "INSERT");
        assert_eq!(units_name("6"), Some("metres"));
        assert!(og_engine::io::guess_crs([418000.0, 6472000.0, 418200.0, 6472400.0]).unwrap().contains("3006"));
    }
}
