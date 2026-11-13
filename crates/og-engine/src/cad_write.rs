//! Drawings written as DXF and DWG, through acadrust: each feature an entity on a layer by an
//! attribute, in a colour and line type by others, points as blocks with their attributes
//! filled in or as points, texts beside them, and 3D models as 3D faces. A template drawing
//! gives the layers, line types, text styles and blocks, and its own drawing, such as a title
//! block, can be kept. DWG is written as AutoCAD 2010, 2013 or 2018.

use crate::model::{err, GbError, Result, Table, Value};
use acadrust::entities::{AttributeEntity, EntityType, Face3D, Insert, LwPolyline, LwVertex, Point, Polyline3D, Text};
use acadrust::types::{Color, DxfVersion, Vector2, Vector3};
use acadrust::CadDocument;
use geo::Geometry;
use std::collections::BTreeMap;
use std::path::Path;

/// The versions of AutoCAD a drawing can be written as.
pub const VERSIONS: &[(&str, &str)] = &[("2018", "AutoCAD 2018"), ("2013", "AutoCAD 2013"), ("2010", "AutoCAD 2010")];

/// How a stream is written as a drawing.
#[derive(Debug, Clone, Default)]
pub struct CadOpts<'a> {
    pub template: Option<&'a Path>,
    /// Keep what the template has drawn in model space, such as a frame or a title block.
    pub keep_template: bool,
    pub layer_by: Option<usize>,
    pub colour_by: Option<usize>,
    pub linetype_by: Option<usize>,
    pub block_by: Option<usize>,
    pub rotation_by: Option<usize>,
    pub text_by: Option<usize>,
    pub text_height: f64,
    pub version: Option<String>,
}

/// What was written, for the log.
#[derive(Debug, Default)]
pub struct CadReport {
    pub entities: usize,
    pub blocks: usize,
    pub layers_made: Vec<String>,
    /// Names asked for that the template does not have: line types and blocks.
    pub no_linetype: BTreeMap<String, usize>,
    pub no_block: BTreeMap<String, usize>,
    pub left_out: usize,
}

fn version_of(v: Option<&str>) -> DxfVersion {
    match v {
        Some("2010") => DxfVersion::AC1024,
        Some("2013") => DxfVersion::AC1027,
        _ => DxfVersion::AC1032,
    }
}

/// Reads a DXF or DWG drawing.
pub fn read_doc(path: &Path) -> Result<CadDocument> {
    let is_dwg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
    if is_dwg {
        let named = |e: acadrust::DxfError| GbError(format!("{} cannot be read as a DWG drawing: {}", path.display(), e));
        acadrust::DwgReader::from_file_with_options(path, acadrust::DwgReadOptions::failsafe()).map_err(named)?.read().map_err(named)
    } else {
        let named = |e: acadrust::DxfError| GbError(format!("{} cannot be read as a DXF drawing: {}", path.display(), e));
        acadrust::DxfReader::from_file(path).map_err(named)?.read().map_err(named)
    }
}

/// A colour from an attribute: #rrggbb, a number of the AutoCAD colours 1 to 255, or
/// ByLayer when empty.
pub fn colour_of(v: &Value) -> Color {
    let t = v.to_text();
    let t = t.trim();
    if let Some(hex) = t.strip_prefix('#').filter(|h| h.len() == 6) {
        if let Ok(n) = u32::from_str_radix(hex, 16) {
            return Color::Rgb { r: (n >> 16) as u8, g: (n >> 8) as u8, b: n as u8 };
        }
    }
    match t.parse::<i64>() {
        Ok(n @ 1..=255) => Color::Index(n as u8),
        _ => Color::ByLayer,
    }
}

/// Writes the stream as a drawing at `path`, DXF or DWG by its extension.
pub fn write_cad(table: &Table, path: &Path, o: &CadOpts) -> Result<CadReport> {
    let mut doc = match o.template {
        Some(t) => read_doc(t)?,
        None => CadDocument::with_version(version_of(o.version.as_deref())),
    };
    doc.version = version_of(o.version.as_deref());
    let mut report = CadReport::default();
    if o.template.is_some() && !o.keep_template {
        // Only the template's tables and blocks: what it drew in model space goes.
        let model = doc.block_records.get("*Model_Space").map(|b| b.handle);
        let drawn: Vec<acadrust::Handle> = doc
            .entities()
            .filter(|e| model.is_some_and(|m| e.common().owner_handle == m || e.common().owner_handle == acadrust::Handle::NULL))
            .map(|e| e.common().handle)
            .collect();
        for h in drawn {
            doc.remove_entity(h);
        }
    }
    // The attribute definitions of each block, by its name, with its base point.
    let mut definitions: BTreeMap<String, (Vector3, Vec<acadrust::entities::AttributeDefinition>)> = BTreeMap::new();
    for br in doc.block_records.iter() {
        if br.name.starts_with('*') {
            continue;
        }
        let defs = br
            .entity_handles
            .iter()
            .filter_map(|h| match doc.get_entity(*h) {
                Some(EntityType::AttributeDefinition(d)) => Some(d.clone()),
                _ => None,
            })
            .collect();
        definitions.insert(br.name.to_uppercase(), (br.base_point, defs));
    }
    let block_name = |wanted: &str| -> Option<String> {
        doc.block_records.iter().find(|b| b.name.eq_ignore_ascii_case(wanted) && !b.name.starts_with('*')).map(|b| b.name.clone())
    };
    let names = table.field_names();
    let text_of = |at: Option<usize>, f: &crate::model::Feature| at.map(|i| f.attrs[i].to_text()).filter(|t| !t.trim().is_empty());
    // The entities, made first and added after, so that the tables they need exist.
    let mut made: Vec<EntityType> = Vec::new();
    let mut layers: Vec<String> = Vec::new();
    for f in &table.features {
        let layer = text_of(o.layer_by, f).map(|l| clean_name(&l)).unwrap_or_else(|| "0".into());
        if !layers.contains(&layer) {
            layers.push(layer.clone());
        }
        let colour = o.colour_by.map_or(Color::ByLayer, |i| colour_of(&f.attrs[i]));
        let linetype = match text_of(o.linetype_by, f) {
            Some(lt) if doc.line_types.iter().any(|t| t.name.eq_ignore_ascii_case(&lt)) => {
                doc.line_types.iter().find(|t| t.name.eq_ignore_ascii_case(&lt)).map(|t| t.name.clone()).unwrap_or_default()
            }
            Some(lt) => {
                *report.no_linetype.entry(lt).or_insert(0) += 1;
                String::new()
            }
            None => String::new(),
        };
        let style = |e: &mut EntityType| {
            let c = e.common_mut();
            c.layer = layer.clone();
            c.color = colour;
            c.linetype = linetype.clone();
        };
        let z_at = |k: usize| f.z.as_ref().and_then(|z| z.get(k).copied()).filter(|v| v.is_finite());
        let before = made.len();
        if let Some(g) = &f.geom {
            let mut k = 0usize;
            let line = |l: &geo::LineString<f64>, closed: bool, k: &mut usize, made: &mut Vec<EntityType>| {
                let zs: Vec<Option<f64>> = (0..l.0.len()).map(|i| z_at(*k + i)).collect();
                *k += l.0.len();
                let mut pts: Vec<geo::Coord<f64>> = l.0.clone();
                if closed && pts.len() > 1 && pts.first() == pts.last() {
                    pts.pop();
                }
                if pts.len() < 2 {
                    return;
                }
                let flat = zs.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| (a.min(*z), b.max(*z)));
                if flat.0.is_finite() && flat.1 - flat.0 > 1e-9 {
                    let mut pl = Polyline3D::from_points(pts.iter().zip(&zs).map(|(c, z)| Vector3::new(c.x, c.y, z.unwrap_or(0.0))).collect());
                    if closed {
                        pl.close();
                    }
                    made.push(EntityType::Polyline3D(pl));
                } else {
                    let mut pl = LwPolyline::new();
                    for c in &pts {
                        pl.add_vertex(LwVertex::new(Vector2::new(c.x, c.y)));
                    }
                    pl.is_closed = closed;
                    pl.elevation = if flat.0.is_finite() { flat.0 } else { 0.0 };
                    made.push(EntityType::LwPolyline(pl));
                }
            };
            let point = |x: f64, y: f64, z: f64, made: &mut Vec<EntityType>, report: &mut CadReport| {
                let wanted = text_of(o.block_by, f);
                match wanted.as_deref().and_then(block_name) {
                    Some(name) => {
                        let rotation = o.rotation_by.and_then(|i| f.attrs[i].as_f64()).unwrap_or(0.0).to_radians();
                        let mut ins = Insert::new(name.clone(), Vector3::new(x, y, z)).with_rotation(rotation);
                        if let Some((base, defs)) = definitions.get(&name.to_uppercase()) {
                            let (s, c) = rotation.sin_cos();
                            for d in defs {
                                // The value of the attribute of the same name, without case.
                                let value = names.iter().position(|n| n.eq_ignore_ascii_case(&d.tag)).map(|i| f.attrs[i].to_text());
                                let mut a = AttributeEntity::from_definition(d, value);
                                let (dx, dy) = (d.insertion_point.x - base.x, d.insertion_point.y - base.y);
                                a.insertion_point = Vector3::new(x + dx * c - dy * s, y + dx * s + dy * c, z + d.insertion_point.z - base.z);
                                let (ax, ay) = (d.alignment_point.x - base.x, d.alignment_point.y - base.y);
                                a.alignment_point = Vector3::new(x + ax * c - ay * s, y + ax * s + ay * c, z + d.alignment_point.z - base.z);
                                a.rotation = d.rotation + rotation;
                                a.common.layer = layer.clone();
                                ins.attributes.push(a);
                            }
                        }
                        report.blocks += 1;
                        made.push(EntityType::Insert(ins));
                    }
                    None => {
                        if let Some(w) = wanted {
                            *report.no_block.entry(w).or_insert(0) += 1;
                        }
                        let mut p = Point::new();
                        p.location = Vector3::new(x, y, z);
                        made.push(EntityType::Point(p));
                    }
                }
            };
            match g {
                Geometry::Point(p) => point(p.x(), p.y(), z_at(0).unwrap_or(0.0), &mut made, &mut report),
                Geometry::MultiPoint(m) => {
                    for (i, p) in m.0.iter().enumerate() {
                        point(p.x(), p.y(), z_at(i).unwrap_or(0.0), &mut made, &mut report);
                    }
                }
                Geometry::LineString(l) => line(l, false, &mut k, &mut made),
                Geometry::MultiLineString(m) => m.0.iter().for_each(|l| line(l, false, &mut k, &mut made)),
                Geometry::Polygon(p) => {
                    line(p.exterior(), true, &mut k, &mut made);
                    p.interiors().iter().for_each(|r| line(r, true, &mut k, &mut made));
                }
                Geometry::MultiPolygon(m) => m.0.iter().for_each(|p| {
                    line(p.exterior(), true, &mut k, &mut made);
                    p.interiors().iter().for_each(|r| line(r, true, &mut k, &mut made));
                }),
                Geometry::Line(l) => line(&geo::LineString::from(vec![l.start, l.end]), false, &mut k, &mut made),
                Geometry::Rect(r) => line(&r.to_polygon().exterior().clone(), true, &mut k, &mut made),
                Geometry::Triangle(t) => line(&t.to_polygon().exterior().clone(), true, &mut k, &mut made),
                Geometry::GeometryCollection(_) => report.left_out += 1,
            }
            // A text beside it: at the point, inside the area, or halfway along the line.
            if let Some(t) = text_of(o.text_by, f) {
                use geo::{Euclidean, InteriorPoint, InterpolatableLine};
                let at = match g {
                    Geometry::LineString(l) => l.point_at_ratio_from_start(&Euclidean, 0.5),
                    _ => g.interior_point(),
                };
                if let Some(at) = at {
                    let txt = Text::with_value(t, Vector3::new(at.x(), at.y(), z_at(0).unwrap_or(0.0))).with_height(if o.text_height > 0.0 {
                        o.text_height
                    } else {
                        2.5
                    });
                    made.push(EntityType::Text(txt));
                }
            }
        }
        for e in made.iter_mut().skip(before) {
            style(e);
        }
    }
    // A 3D model as 3D faces, each object on its layer.
    if let Some(mesh) = table.mesh.as_deref() {
        for t in &mesh.tris {
            let p = |i: u32| {
                let v = mesh.positions[i as usize];
                Vector3::new(v[0], v[1], v[2])
            };
            let (a, b, c) = (p(t.v[0]), p(t.v[1]), p(t.v[2]));
            let mut face = EntityType::Face3D(Face3D::new(a, b, c, c));
            let f = table.features.get(t.object as usize);
            let layer = f.and_then(|f| text_of(o.layer_by, f)).map(|l| clean_name(&l)).unwrap_or_else(|| "0".into());
            if !layers.contains(&layer) {
                layers.push(layer.clone());
            }
            let common = face.common_mut();
            common.layer = layer;
            common.color = match (f, o.colour_by) {
                (Some(f), Some(i)) => colour_of(&f.attrs[i]),
                _ => mesh
                    .materials
                    .get(t.material as usize)
                    .map(|m| {
                        let c = m.diffuse.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                        Color::Rgb { r: c[0], g: c[1], b: c[2] }
                    })
                    .unwrap_or(Color::ByLayer),
            };
            made.push(face);
        }
    }
    for l in &layers {
        if !doc.layers.iter().any(|x| x.name.eq_ignore_ascii_case(l)) {
            doc.layers.add(acadrust::tables::Layer::new(l.clone())).map_err(|e| GbError(format!("The layer {} could not be made: {}", l, e)))?;
            report.layers_made.push(l.clone());
        } else if let Some(existing) = doc.layers.iter().find(|x| x.name.eq_ignore_ascii_case(l)).map(|x| x.name.clone()) {
            // The template's spelling of the name.
            for e in made.iter_mut() {
                if e.common().layer.eq_ignore_ascii_case(&existing) {
                    e.common_mut().layer = existing.clone();
                }
            }
        }
    }
    report.entities = made.len();
    for e in made {
        doc.add_entity(e).map_err(|e| GbError(format!("An entity could not be added to the drawing: {}", e)))?;
    }
    let is_dwg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
    if is_dwg {
        acadrust::DwgWriter::write_to_file(path, &doc).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    } else {
        acadrust::DxfWriter::new(&doc).write_to_file(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    if report.entities == 0 && table.has_geometry() {
        return err("Nothing could be written to the drawing.");
    }
    Ok(report)
}

/// A name AutoCAD takes for a layer: without < > / \ " : ; ? * | = `.
fn clean_name(s: &str) -> String {
    let t: String = s.trim().chars().map(|c| if "<>/\\\":;?*|=`".contains(c) { '_' } else { c }).collect();
    if t.is_empty() {
        "0".into()
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, FieldType};
    use acadrust::TableEntry;

    #[test]
    fn features_become_entities_on_their_layers_with_blocks_from_a_template() {
        let dir = std::env::temp_dir().join(format!("og-cad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A template with a block BRUNN that has an attribute DJUP, and a line type DASHED.
        let mut t = CadDocument::with_version(DxfVersion::AC1032);
        let mut br = acadrust::BlockRecord::new("BRUNN");
        let h = t.allocate_handle();
        br.set_handle(h);
        br.block_entity_handle = t.allocate_handle();
        br.block_end_handle = t.allocate_handle();
        t.block_records.add(br).unwrap();
        let mut circle = EntityType::Circle(acadrust::entities::Circle::from_coords(0.0, 0.0, 0.0, 1.0));
        circle.common_mut().owner_handle = h;
        t.add_entity(circle).unwrap();
        let mut def = acadrust::entities::AttributeDefinition::new("DJUP".into(), "Depth".into(), "0".into());
        def.insertion_point = Vector3::new(1.5, 0.0, 0.0);
        def.height = 1.0;
        let mut def = EntityType::AttributeDefinition(def);
        def.common_mut().owner_handle = h;
        t.add_entity(def).unwrap();
        let mut dashed = acadrust::tables::LineType::new("DASHED");
        dashed.description = "Dashed __ __ __".into();
        let _ = t.line_types.add(dashed);
        let template = dir.join("template.dxf");
        acadrust::DxfWriter::new(&t).write_to_file(&template).unwrap();

        let table = Table {
            fields: vec![
                Field::new("kind", FieldType::Text),
                Field::new("block", FieldType::Text),
                Field::new("djup", FieldType::Real),
                Field::new("colour", FieldType::Text),
                Field::new("lt", FieldType::Text),
            ],
            features: vec![
                Feature {
                    z: None,
                    attrs: vec![Value::Text("VA_brunn".into()), Value::Text("brunn".into()), Value::Real(2.4), Value::Text("#ff0000".into()), Value::Null],
                    geom: Some(Geometry::Point(geo::Point::new(100.0, 200.0))),
                },
                Feature {
                    z: None,
                    attrs: vec![Value::Text("VA_ledning".into()), Value::Null, Value::Null, Value::Text("5".into()), Value::Text("dashed".into())],
                    geom: Some(Geometry::LineString(geo::LineString::from(vec![(100.0, 200.0), (150.0, 210.0)]))),
                },
                Feature {
                    z: None,
                    attrs: vec![Value::Text("Tomt".into()), Value::Text("saknas".into()), Value::Null, Value::Null, Value::Text("ZIGZAG".into())],
                    geom: Some(Geometry::Point(geo::Point::new(0.0, 0.0))),
                },
            ],
            crs: Some(3006),
            ..Table::default()
        };
        for (file, version) in [("out.dwg", "2010"), ("out.dxf", "2018")] {
            let out = dir.join(file);
            let r = write_cad(
                &table,
                &out,
                &CadOpts {
                    template: Some(&template),
                    keep_template: true,
                    layer_by: Some(0),
                    colour_by: Some(3),
                    linetype_by: Some(4),
                    block_by: Some(1),
                    version: Some(version.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!((r.entities, r.blocks), (3, 1));
            assert_eq!(r.no_linetype.get("ZIGZAG"), Some(&1));
            assert_eq!(r.no_block.get("saknas"), Some(&1));
            assert!(r.layers_made.contains(&"VA_brunn".to_string()));
            // Read back: the block with its attribute filled in, the line dashed and blue.
            let doc = read_doc(&out).unwrap();
            assert_eq!(doc.version, version_of(Some(version)));
            let insert = doc.entities().find_map(|e| if let EntityType::Insert(i) = e { Some(i.clone()) } else { None }).unwrap();
            assert_eq!(insert.block_name.to_uppercase(), "BRUNN");
            assert_eq!(insert.common.layer, "VA_brunn");
            assert_eq!(insert.attributes.first().map(|a| (a.tag.as_str(), a.value.as_str(), a.insertion_point.x)), Some(("DJUP", "2.4", 101.5)));
            let line = doc.entities().find_map(|e| if let EntityType::LwPolyline(l) = e { Some(l.clone()) } else { None }).unwrap();
            assert_eq!((line.common.layer.as_str(), line.common.linetype.as_str(), line.common.color), ("VA_ledning", "DASHED", Color::Index(5)));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
