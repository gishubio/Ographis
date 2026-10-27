//! City models: CityJSON (also as CityJSONSeq) and CityGML 2.0 and 3.0. Each city object,
//! such as a building, a building part, a road or a tree, becomes a feature with its
//! attributes, and its surfaces a 3D model coloured by what they are (roof, wall, ground),
//! for processing and for pictures.

use crate::io::Notes;
use crate::mesh::{Material, Mesh, Tri, NONE};
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// A surface of a city object: its outer ring and holes, and what kind of surface it is.
#[derive(Debug, Clone, Default)]
pub(crate) struct Surface {
    pub rings: Vec<Vec<[f64; 3]>>,
    pub semantic: Option<String>,
}

/// A city object as read, before it becomes a feature.
#[derive(Debug, Clone, Default)]
pub(crate) struct CityObject {
    pub id: String,
    pub kind: String,
    pub parent: Option<String>,
    pub attrs: Vec<(String, Value)>,
    /// The surfaces of each level of detail, by its name, such as "2" or "2.2".
    pub lods: Vec<(String, Vec<Surface>)>,
}

/// Which level of detail is read: the finest each object has, or one asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Lod {
    Finest,
    Coarsest,
    /// A level such as "1" or "2.2": the first that starts with it.
    Level(String),
}

/// What a city object becomes, in the colours of a city model.
fn colour_of(kind: &str, semantic: Option<&str>) -> ([f32; 3], &'static str) {
    match semantic.unwrap_or("") {
        "RoofSurface" => return ([0.62, 0.30, 0.25], "roof"),
        "WallSurface" | "OuterWallSurface" => return ([0.90, 0.88, 0.84], "wall"),
        "GroundSurface" => return ([0.55, 0.55, 0.53], "ground"),
        "ClosureSurface" => return ([0.80, 0.80, 0.80], "closure"),
        "OuterCeilingSurface" | "OuterFloorSurface" => return ([0.80, 0.78, 0.74], "wall"),
        "Window" | "WindowSurface" => return ([0.55, 0.68, 0.76], "window glass"),
        "Door" | "DoorSurface" => return ([0.55, 0.42, 0.30], "door"),
        "InteriorWallSurface" | "CeilingSurface" | "FloorSurface" => return ([0.85, 0.83, 0.80], "interior"),
        "WaterGroundSurface" => return ([0.45, 0.40, 0.32], "ground"),
        "TrafficArea" | "AuxiliaryTrafficArea" => return ([0.45, 0.45, 0.47], "road"),
        "WaterSurface" => return ([0.25, 0.45, 0.60], "water"),
        _ => {}
    }
    let k = kind.to_lowercase();
    if k.contains("vegetation") || k.contains("plantcover") {
        ([0.30, 0.50, 0.22], "vegetation")
    } else if k.contains("water") {
        ([0.25, 0.45, 0.60], "water")
    } else if k.contains("road") || k.contains("railway") || k.contains("transport") || k.contains("track") || k.contains("square") {
        ([0.45, 0.45, 0.47], "road")
    } else if k.contains("relief") || k.contains("tin") {
        ([0.55, 0.60, 0.42], "terrain")
    } else if k.contains("landuse") {
        ([0.70, 0.75, 0.55], "land")
    } else if k.contains("bridge") || k.contains("tunnel") {
        ([0.62, 0.62, 0.62], "structure")
    } else {
        ([0.88, 0.86, 0.82], "building")
    }
}

/// A city model that does not say its coordinate system is read all the same, to be looked
/// at and written on, with a warning and a guess from its coordinates.
fn no_crs(objects: &[CityObject], notes: &mut Notes) {
    let mut e = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in objects.iter().flat_map(|o| &o.lods).flat_map(|(_, s)| s).flat_map(|s| &s.rings).flatten() {
        e = [e[0].min(p[0]), e[1].min(p[1]), e[2].max(p[0]), e[3].max(p[1])];
    }
    let guess = crate::io::guess_crs(e).map(|g| format!(" {}.", g)).unwrap_or_default();
    notes.warn(format!(
        "The city model does not say which coordinate system it is in. It is read without one, so it can be looked at in 3D and written, but not reprojected or put on a background map; set crs in the input when it is known.{}",
        guess
    ));
}

/// The surfaces of an object at the level asked for.
fn pick<'a>(lods: &'a [(String, Vec<Surface>)], lod: &Lod) -> Option<&'a (String, Vec<Surface>)> {
    let num = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    match lod {
        Lod::Finest => lods.iter().filter(|(_, s)| !s.is_empty()).max_by(|a, b| num(&a.0).total_cmp(&num(&b.0))),
        Lod::Coarsest => lods.iter().filter(|(_, s)| !s.is_empty()).min_by(|a, b| num(&a.0).total_cmp(&num(&b.0))),
        Lod::Level(l) => lods.iter().find(|(n, s)| n.starts_with(l.as_str()) && !s.is_empty()),
    }
}

/// The city objects as a table with a 3D model: a feature for each object that has
/// surfaces at the level asked for, with its surfaces as areas with heights.
pub(crate) fn to_table(objects: Vec<CityObject>, lod: &Lod, crs: Option<u32>, notes: &mut Notes) -> Result<Table> {
    let mut mesh = Mesh::default();
    let mut material_index: HashMap<String, u32> = HashMap::new();
    let mut attr_names: Vec<String> = Vec::new();
    for o in &objects {
        for (k, _) in &o.attrs {
            if !attr_names.contains(k) {
                attr_names.push(k.clone());
            }
        }
    }
    let mut features = Vec::new();
    let mut values: Vec<HashMap<String, Value>> = Vec::new();
    let mut lods_used: HashMap<String, usize> = HashMap::new();
    let (mut without, mut flat) = (0usize, 0usize);
    for o in objects {
        let Some((level, surfaces)) = pick(&o.lods, lod) else {
            without += 1;
            continue;
        };
        *lods_used.entry(level.clone()).or_default() += 1;
        let object = mesh.objects.len() as u32;
        mesh.objects.push(o.id.clone());
        let mut polys = Vec::new();
        let mut zs = Vec::new();
        let (mut zlo, mut zhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for s in surfaces {
            let Some(outer) = s.rings.first().filter(|r| r.len() >= 3) else { continue };
            let (rgb, name) = colour_of(&o.kind, s.semantic.as_deref());
            let mi = *material_index.entry(name.to_string()).or_insert_with(|| {
                let glass = name.contains("glass");
                mesh.materials.push(Material { name: name.to_string(), diffuse: rgb, opacity: if glass { 0.35 } else { 1.0 }, ..Default::default() });
                (mesh.materials.len() - 1) as u32
            });
            let strip = |r: &Vec<[f64; 3]>| -> Vec<[f64; 3]> {
                let mut r = r.clone();
                if r.len() > 1 && r.first() == r.last() {
                    r.pop();
                }
                r
            };
            let holes: Vec<Vec<[f64; 3]>> = s.rings[1..].iter().map(strip).collect();
            for t in crate::ifc_io::triangulate_face(&strip(outer), &holes) {
                let v = mesh.positions.len() as u32;
                mesh.positions.extend_from_slice(&t);
                mesh.tris.push(Tri { v: [v, v + 1, v + 2], n: [NONE; 3], t: [NONE; 3], material: mi, object });
            }
            let ring = |r: &Vec<[f64; 3]>| {
                let mut c: Vec<geo::Coord<f64>> = r.iter().map(|p| geo::coord! { x: p[0], y: p[1] }).collect();
                if c.first() != c.last() {
                    c.push(c[0]);
                }
                geo::LineString::new(c)
            };
            let closed = |r: &Vec<[f64; 3]>| {
                let mut r = r.clone();
                if r.first() != r.last() {
                    r.push(r[0]);
                }
                r
            };
            polys.push(geo::Polygon::new(ring(outer), s.rings[1..].iter().map(ring).collect()));
            for r in &s.rings {
                for p in closed(r) {
                    zs.push(p[2]);
                    zlo = zlo.min(p[2]);
                    zhi = zhi.max(p[2]);
                }
            }
        }
        if polys.is_empty() {
            flat += 1;
            mesh.objects.pop();
            continue;
        }
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        let attrs = vec![
            Value::Text(o.id),
            Value::Text(o.kind),
            o.parent.map_or(Value::Null, Value::Text),
            Value::Text(level.clone()),
            if zlo.is_finite() { Value::Real(round(zlo)) } else { Value::Null },
            if zhi.is_finite() { Value::Real(round(zhi)) } else { Value::Null },
            if zlo.is_finite() { Value::Real(round(zhi - zlo)) } else { Value::Null },
        ];
        values.push(o.attrs.into_iter().collect());
        features.push(Feature { attrs, geom: Some(geo::Geometry::MultiPolygon(geo::MultiPolygon::new(polys))), z: Some(Arc::from(zs)) });
    }
    if features.is_empty() {
        return err(match lod {
            Lod::Level(l) => format!("No city object has surfaces at level of detail {}. Choose another level, or the finest.", l),
            _ => "The city model has no objects with surfaces that can be read.".to_string(),
        });
    }
    let mut fields = vec![
        Field::new("id", FieldType::Text),
        Field::new("type", FieldType::Text),
        Field::new("parent", FieldType::Text),
        Field::new("lod", FieldType::Text),
        Field::new("z_min", FieldType::Real),
        Field::new("z_max", FieldType::Real),
        Field::new("height", FieldType::Real),
    ];
    let own: Vec<String> = attr_names.into_iter().filter(|n| !fields.iter().any(|f| f.name == *n)).collect();
    for n in &own {
        let ty = crate::model::infer_type(values.iter().filter_map(|m| m.get(n)));
        fields.push(Field::new(n.clone(), ty));
    }
    for (f, v) in features.iter_mut().zip(&values) {
        for n in &own {
            f.attrs.push(crate::model::coerce(v.get(n).cloned().unwrap_or(Value::Null), fields[f.attrs.len()].ty));
        }
    }
    let mut lods: Vec<(String, usize)> = lods_used.into_iter().collect();
    lods.sort();
    notes.info(format!(
        "{} city objects, {} triangles, at level of detail {}{}",
        fmt_count(features.len() as u64),
        fmt_count(mesh.tris.len() as u64),
        lods.iter().map(|(l, n)| format!("{} ({})", l, fmt_count(*n as u64))).collect::<Vec<_>>().join(", "),
        if without + flat > 0 { format!("; {} without surfaces there were left out", fmt_count((without + flat) as u64)) } else { String::new() }
    ));
    Ok(Table { fields, crs, features, raster: false, image: None, grid: None, mesh: Some(Arc::new(mesh)) })
}

/// The EPSG code in a name of a coordinate system: EPSG:3006, urn:ogc:def:crs:EPSG::3006,
/// https://www.opengis.net/def/crs/EPSG/0/3006, or the first of a compound one.
pub(crate) fn epsg_in(name: &str) -> Option<u32> {
    let u = name.to_uppercase();
    // The names of the German surveying authorities (AdV): ETRS89 with UTM 32 or 33.
    if let Some(i) = u.find("ETRS89_UTM") {
        let zone: String = u[i + 10..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(z @ 28..=38) = zone.parse::<u32>() {
            return Some(25800 + z);
        }
    }
    let at = u.find("EPSG")?;
    // The part of this system, up to the next of a compound one; its last whole number, after
    // any version such as 6.12 in EPSG:6.12:25833.
    let part = u[at + 4..].split(',').next().unwrap_or("");
    part.split([':', '/']).map(str::trim).rfind(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).and_then(|s| s.parse().ok())
}

// ---------- CityJSON ----------

fn json_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n.as_i64().map_or_else(|| n.as_f64().map_or(Value::Null, Value::Real), Value::Int),
        serde_json::Value::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}

/// A geometry of CityJSON as surfaces: its boundaries with their semantic values.
fn cj_surfaces(g: &serde_json::Value, vertex: &dyn Fn(usize) -> Option<[f64; 3]>) -> Vec<Surface> {
    let kind = g.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let boundaries = g.get("boundaries").cloned().unwrap_or(serde_json::Value::Null);
    let sem = g.get("semantics");
    let names: Vec<String> = sem
        .and_then(|s| s.get("surfaces"))
        .and_then(|s| s.as_array())
        .map(|a| a.iter().map(|x| x.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string()).collect())
        .unwrap_or_default();
    let values = sem.and_then(|s| s.get("values")).cloned().unwrap_or(serde_json::Value::Null);
    let ring = |r: &serde_json::Value| -> Vec<[f64; 3]> {
        r.as_array().map(|a| a.iter().filter_map(|i| i.as_u64().and_then(|i| vertex(i as usize))).collect()).unwrap_or_default()
    };
    let surface = |s: &serde_json::Value, v: &serde_json::Value| -> Surface {
        Surface {
            rings: s.as_array().map(|a| a.iter().map(ring).collect()).unwrap_or_default(),
            semantic: v.as_u64().and_then(|i| names.get(i as usize)).cloned(),
        }
    };
    let arr = |v: &serde_json::Value| v.as_array().cloned().unwrap_or_default();
    let at = |v: &serde_json::Value, i: usize| v.as_array().and_then(|a| a.get(i)).cloned().unwrap_or(serde_json::Value::Null);
    let mut out = Vec::new();
    match kind {
        "MultiSurface" | "CompositeSurface" => {
            for (i, s) in arr(&boundaries).iter().enumerate() {
                out.push(surface(s, &at(&values, i)));
            }
        }
        "Solid" => {
            for (i, shell) in arr(&boundaries).iter().enumerate() {
                let vs = at(&values, i);
                for (j, s) in arr(shell).iter().enumerate() {
                    out.push(surface(s, &at(&vs, j)));
                }
            }
        }
        "MultiSolid" | "CompositeSolid" => {
            for (k, solid) in arr(&boundaries).iter().enumerate() {
                let vk = at(&values, k);
                for (i, shell) in arr(solid).iter().enumerate() {
                    let vs = at(&vk, i);
                    for (j, s) in arr(shell).iter().enumerate() {
                        out.push(surface(s, &at(&vs, j)));
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// The objects of a CityJSON document or a CityJSONFeature, with their vertices transformed.
fn cj_objects(doc: &serde_json::Value, transform: Option<(&[f64], &[f64])>, templates: Option<&serde_json::Value>, out: &mut Vec<CityObject>) {
    let verts: Vec<[f64; 3]> = doc
        .get("vertices")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|p| {
                    let n = |k: usize| p.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                    match transform {
                        Some((s, t)) => [n(0) * s[0] + t[0], n(1) * s[1] + t[1], n(2) * s[2] + t[2]],
                        None => [n(0), n(1), n(2)],
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let tpl_verts: Vec<[f64; 3]> = templates
        .and_then(|t| t.get("vertices-templates"))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|p| [0, 1, 2].map(|k| p.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0))).collect())
        .unwrap_or_default();
    let Some(objs) = doc.get("CityObjects").and_then(|c| c.as_object()) else { return };
    for (id, o) in objs {
        let kind = o.get("type").and_then(|t| t.as_str()).unwrap_or("CityObject").to_string();
        let mut attrs = Vec::new();
        if let Some(a) = o.get("attributes").and_then(|a| a.as_object()) {
            for (k, v) in a {
                attrs.push((k.clone(), json_value(v)));
            }
        }
        let mut lods: Vec<(String, Vec<Surface>)> = Vec::new();
        for g in o.get("geometry").and_then(|g| g.as_array()).into_iter().flatten() {
            let (lod, surfaces) = if g.get("type").and_then(|t| t.as_str()) == Some("GeometryInstance") {
                // A template, such as a tree, placed at a point by a matrix.
                let Some(tpl) = g.get("template").and_then(|t| t.as_u64()).and_then(|i| templates?.get("templates")?.get(i as usize)) else { continue };
                let anchor =
                    g.get("boundaries").and_then(|b| b.get(0)).and_then(|i| i.as_u64()).and_then(|i| verts.get(i as usize)).copied().unwrap_or([0.0; 3]);
                let m: Vec<f64> =
                    g.get("transformationMatrix").and_then(|m| m.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
                let m = if m.len() == 16 { m } else { vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] };
                let place = |i: usize| -> Option<[f64; 3]> {
                    let p = tpl_verts.get(i)?;
                    Some([0, 1, 2].map(|r| m[r * 4] * p[0] + m[r * 4 + 1] * p[1] + m[r * 4 + 2] * p[2] + m[r * 4 + 3] + anchor[r]))
                };
                (tpl.get("lod").map(|l| l.to_string().trim_matches('"').to_string()).unwrap_or_default(), cj_surfaces(tpl, &place))
            } else {
                let lod = g.get("lod").map(|l| l.to_string().trim_matches('"').to_string()).unwrap_or_default();
                (lod, cj_surfaces(g, &|i| verts.get(i).copied()))
            };
            match lods.iter_mut().find(|(l, _)| *l == lod) {
                Some((_, s)) => s.extend(surfaces),
                None => lods.push((lod, surfaces)),
            }
        }
        let parent = o.get("parents").and_then(|p| p.get(0)).and_then(|p| p.as_str()).map(str::to_string);
        out.push(CityObject { id: id.clone(), kind, parent, attrs, lods });
    }
}

/// Reads CityJSON, or CityJSONSeq with a line for each feature.
pub fn read_cityjson(path: &Path, lod: &Lod, crs_given: Option<u32>, notes: &mut Notes) -> Result<Table> {
    let text = std::fs::read_to_string(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let mut objects = Vec::new();
    let (mut crs, mut version) = (None, String::new());
    let first_line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let seq = serde_json::from_str::<serde_json::Value>(first_line).is_ok_and(|v| v.get("type").and_then(|t| t.as_str()) == Some("CityJSON"))
        && text.lines().filter(|l| !l.trim().is_empty()).count() > 1;
    let header: serde_json::Value = if seq {
        serde_json::from_str(first_line).map_err(|e| GbError(format!("{}: {}", path.display(), e)))?
    } else {
        serde_json::from_str(&text).map_err(|e| GbError(format!("{} cannot be read as CityJSON: {}", path.display(), e)))?
    };
    if header.get("type").and_then(|t| t.as_str()) != Some("CityJSON") {
        return err(format!("{} is JSON, but not CityJSON.", path.display()));
    }
    version = header.get("version").and_then(|v| v.as_str()).unwrap_or(&version).to_string();
    let scale: Vec<f64> = header
        .get("transform")
        .and_then(|t| t.get("scale"))
        .and_then(|s| s.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default();
    let translate: Vec<f64> = header
        .get("transform")
        .and_then(|t| t.get("translate"))
        .and_then(|s| s.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default();
    let transform = (scale.len() == 3 && translate.len() == 3).then_some((scale.as_slice(), translate.as_slice()));
    if let Some(rs) = header.get("metadata").and_then(|m| m.get("referenceSystem")).and_then(|r| r.as_str()) {
        crs = epsg_in(rs);
    }
    let templates = header.get("geometry-templates").cloned();
    if seq {
        for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
            let f: serde_json::Value = serde_json::from_str(line).map_err(|e| GbError(format!("{}: a line cannot be read: {}", path.display(), e)))?;
            cj_objects(&f, transform, templates.as_ref(), &mut objects);
        }
    } else {
        cj_objects(&header, transform, templates.as_ref(), &mut objects);
    }
    notes.info(format!("CityJSON {}{}", version, if seq { ", a feature per line" } else { "" }));
    let crs = crs.or(crs_given);
    if crs.is_none() {
        no_crs(&objects, notes);
    }
    to_table(objects, lod, crs, notes)
}

// ---------- CityGML ----------

/// The kinds of city objects in CityGML 2.0 and 3.0, by their local names.
const CITY_OBJECTS: &[&str] = &[
    "Building",
    "BuildingPart",
    "BuildingInstallation",
    "Road",
    "Railway",
    "Track",
    "Square",
    "TransportationComplex",
    "Bridge",
    "BridgePart",
    "Tunnel",
    "TunnelPart",
    "WaterBody",
    "LandUse",
    "PlantCover",
    "SolitaryVegetationObject",
    "CityFurniture",
    "GenericCityObject",
    "ReliefFeature",
    "TINRelief",
    "CityObjectGroup",
    "BuildingConstructiveElement",
    "Waterway",
    "HollowSpace",
    "OtherConstruction",
];

/// The kinds of surfaces, by the local names of their elements.
const SEMANTICS: &[&str] = &[
    "RoofSurface",
    "WallSurface",
    "GroundSurface",
    "ClosureSurface",
    "OuterCeilingSurface",
    "OuterFloorSurface",
    "Window",
    "Door",
    "WindowSurface",
    "DoorSurface",
    "InteriorWallSurface",
    "CeilingSurface",
    "FloorSurface",
    "WaterSurface",
    "WaterGroundSurface",
    "TrafficArea",
    "AuxiliaryTrafficArea",
];

/// Parts of a model that are not attributes: geometry and links, by their local names.
fn is_geometry_holder(name: &str) -> bool {
    name.starts_with("lod") || name == "boundedBy" || name.ends_with("Geometry") || name == "consistsOfBuildingPart"
}

/// Reads CityGML 2.0 or 3.0: the city objects, their simple attributes and generic ones,
/// and their polygons by level of detail and kind of surface.
pub fn read_citygml(path: &Path, lod: &Lod, crs_given: Option<u32>, notes: &mut Notes, going: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    use quick_xml::events::Event;
    let file = std::fs::File::open(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let size = file.metadata().map(|m| m.len()).ok();
    let mut reader = quick_xml::Reader::from_reader(std::io::BufReader::new(file));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    // The objects being read, the innermost last.
    let mut stack: Vec<CityObject> = Vec::new();
    let mut done: Vec<CityObject> = Vec::new();
    // The names of the open elements, and the depth at which each open object began.
    let mut path_names: Vec<String> = Vec::new();
    let mut object_depth: Vec<usize> = Vec::new();
    let mut crs: Option<u32> = None;
    let mut dim = 3usize;
    let mut text = String::new();
    // Inside a polygon: its rings, the ring being read.
    let mut rings: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut in_polygon = 0usize;
    // A generic attribute: its name, waiting for its value.
    let mut generic: Option<String> = None;
    let mut seen_gml = false;
    // An implicit geometry, such as a tree placed many times: its polygons in their own
    // coordinates, its matrix and the point it is placed at.
    let mut implicit: Option<(Vec<Surface>, Vec<f64>, [f64; 3])> = None;
    let mut in_reference = false;
    let mut count = 0u64;
    let parse_coords = |t: &str, dim: usize| -> Vec<[f64; 3]> {
        let nums: Vec<f64> = t.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        let d = dim.clamp(2, 3);
        nums.chunks_exact(d).map(|c| [c[0], c[1], if d == 3 { c[2] } else { 0.0 }]).collect()
    };
    let decoder = reader.decoder();
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| GbError(format!("{} cannot be read as XML at byte {}: {}", path.display(), reader.buffer_position(), e)))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let attr = |key: &str| -> Option<String> {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == key.as_bytes())
                        .and_then(|a| a.decode_and_unescape_value(decoder).ok().map(|v| v.into_owned()))
                };
                if name == "CityModel" {
                    seen_gml = true;
                }
                if let Some(s) = attr("srsName") {
                    if crs.is_none() {
                        crs = epsg_in(&s);
                    }
                }
                if let Some(d) = attr("srsDimension").and_then(|d| d.parse().ok()) {
                    dim = d;
                }
                if CITY_OBJECTS.contains(&name.as_str()) && !empty {
                    count += 1;
                    if count.is_multiple_of(500) && !going(reader.buffer_position(), size) {
                        return err(crate::model::STOPPED);
                    }
                    let id = attr("id").unwrap_or_else(|| format!("{}_{}", name, count));
                    let parent = stack.last().map(|p| p.id.clone());
                    stack.push(CityObject { id, kind: name.clone(), parent, ..Default::default() });
                    object_depth.push(path_names.len());
                }
                if name == "ImplicitGeometry" {
                    implicit = Some((Vec::new(), Vec::new(), [0.0; 3]));
                }
                if name == "referencePoint" {
                    in_reference = true;
                }
                if name == "Polygon" || name == "Triangle" {
                    in_polygon += 1;
                    rings.clear();
                }
                if name == "LinearRing" && in_polygon > 0 {
                    rings.push(Vec::new());
                }
                if matches!(name.as_str(), "stringAttribute" | "intAttribute" | "doubleAttribute" | "dateAttribute" | "uriAttribute" | "measureAttribute") {
                    generic = attr("name");
                }
                if !empty {
                    path_names.push(name);
                }
                text.clear();
            }
            Event::Text(t) => {
                let s = t.decode().map(|c| c.into_owned()).unwrap_or_default();
                text.push_str(&quick_xml::escape::unescape(&s).map(|c| c.into_owned()).unwrap_or(s));
            }
            Event::GeneralRef(r) => {
                let name = String::from_utf8_lossy(r.as_ref()).into_owned();
                text.push_str(match name.as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                path_names.pop();
                let depth = path_names.len();
                match name.as_str() {
                    "posList" | "coordinates" if in_polygon > 0 => {
                        if let Some(r) = rings.last_mut() {
                            r.extend(parse_coords(&text, dim));
                        }
                    }
                    "transformationMatrix" => {
                        if let Some(im) = implicit.as_mut() {
                            im.1 = text.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                        }
                    }
                    "pos" if in_reference && in_polygon == 0 => {
                        if let (Some(im), Some(p)) = (implicit.as_mut(), parse_coords(&text, 3).first()) {
                            im.2 = *p;
                        }
                    }
                    "referencePoint" => in_reference = false,
                    "ImplicitGeometry" => {
                        if let (Some((surfaces, m, at)), Some(obj)) = (implicit.take(), stack.last_mut()) {
                            let m = if m.len() == 16 { m } else { vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] };
                            let lod = path_names
                                .iter()
                                .rev()
                                .find_map(|n| n.strip_prefix("lod").and_then(|r| r.chars().next().filter(|c| c.is_ascii_digit()).map(|c| c.to_string())))
                                .unwrap_or_default();
                            let placed: Vec<Surface> = surfaces
                                .into_iter()
                                .map(|s| Surface {
                                    rings: s
                                        .rings
                                        .into_iter()
                                        .map(|r| {
                                            r.into_iter()
                                                .map(|p| [0, 1, 2].map(|k| m[k * 4] * p[0] + m[k * 4 + 1] * p[1] + m[k * 4 + 2] * p[2] + m[k * 4 + 3] + at[k]))
                                                .collect()
                                        })
                                        .collect(),
                                    semantic: s.semantic,
                                })
                                .collect();
                            match obj.lods.iter_mut().find(|(l, _)| *l == lod) {
                                Some((_, v)) => v.extend(placed),
                                None => obj.lods.push((lod, placed)),
                            }
                        }
                    }
                    "pos" if in_polygon > 0 => {
                        if let Some(r) = rings.last_mut() {
                            r.extend(parse_coords(&text, dim));
                        }
                    }
                    "Polygon" | "Triangle" => {
                        in_polygon = in_polygon.saturating_sub(1);
                        if let Some(obj) = stack.last_mut() {
                            // The level of detail and the kind of surface, from the elements around.
                            let lod = path_names
                                .iter()
                                .rev()
                                .find_map(|n| n.strip_prefix("lod").and_then(|r| r.chars().next().filter(|c| c.is_ascii_digit()).map(|c| c.to_string())))
                                .unwrap_or_default();
                            let semantic = path_names.iter().rev().find(|n| SEMANTICS.contains(&n.as_str())).cloned();
                            let rings = std::mem::take(&mut rings).into_iter().filter(|r| r.len() >= 3).collect::<Vec<_>>();
                            if !rings.is_empty() {
                                let s = Surface { rings, semantic };
                                if let Some(im) = implicit.as_mut() {
                                    // In the coordinates of the template: placed when it ends.
                                    im.0.push(s);
                                    text.clear();
                                    buf.clear();
                                    continue;
                                }
                                match obj.lods.iter_mut().find(|(l, _)| *l == lod) {
                                    Some((_, v)) => v.push(s),
                                    None => obj.lods.push((lod, vec![s])),
                                }
                            }
                        }
                    }
                    "value" if generic.is_some() => {
                        if let (Some(obj), Some(g)) = (stack.last_mut(), generic.take()) {
                            let v = crate::model::parse_number(&text).filter(|_| !text.trim().is_empty()).map_or_else(
                                || Value::Text(text.trim().to_string()),
                                |n| if n.fract() == 0.0 && n.abs() < 9e15 && !text.contains('.') { Value::Int(n as i64) } else { Value::Real(n) },
                            );
                            obj.attrs.push((g, v));
                        }
                    }
                    _ => {}
                }
                // A simple element right under an object is an attribute of it.
                if let (Some(obj), Some(start)) = (stack.last_mut(), object_depth.last()) {
                    if depth == start + 1 && !text.trim().is_empty() && !is_geometry_holder(&name) && in_polygon == 0 {
                        let v = text.trim().to_string();
                        let key = if name == "name" { "name".to_string() } else { name.clone() };
                        if !obj.attrs.iter().any(|(k, _)| *k == key) {
                            obj.attrs.push((
                                key,
                                crate::model::parse_number(&v)
                                    .filter(|_| !v.starts_with('0') || v.contains('.') || v == "0")
                                    .map_or(Value::Text(v.clone()), Value::Real),
                            ));
                        }
                    }
                }
                if object_depth.last() == Some(&depth) && CITY_OBJECTS.contains(&name.as_str()) {
                    object_depth.pop();
                    if let Some(o) = stack.pop() {
                        done.push(o);
                    }
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !seen_gml && done.is_empty() {
        return err(format!("{} is XML, but not CityGML.", path.display()));
    }
    // GML in an EPSG system may give northing before easting, as the system defines its axes.
    // SWEREF 99 TM and the like have northings of millions and eastings below one: swapped
    // back when every coordinate looks so.
    let mut n = 0usize;
    let mut north_first = true;
    'check: for o in &done {
        for (_, ss) in &o.lods {
            for s in ss {
                for p in s.rings.iter().flatten().take(4) {
                    n += 1;
                    if !(p[0] > 3_000_000.0 && p[1].abs() < 2_000_000.0) {
                        north_first = false;
                        break 'check;
                    }
                }
            }
        }
    }
    if n > 0 && north_first {
        for o in &mut done {
            for (_, ss) in &mut o.lods {
                for s in ss {
                    for p in s.rings.iter_mut().flatten() {
                        p.swap(0, 1);
                    }
                }
            }
        }
        notes.info("the coordinates are northing before easting, as the coordinate system defines them, and were swapped");
    }
    let crs = crs.or(crs_given);
    if crs.is_none() {
        no_crs(&done, notes);
    }
    to_table(done, lod, crs, notes)
}

/// Whether a file named in an input is a city model, for the form: by its contents when it
/// is on this computer.
pub fn is_city_path(path: &str) -> bool {
    let p = Path::new(path);
    let lower = path.to_lowercase();
    lower.contains(".city.json") || lower.contains(".city.jsonl") || (p.is_file() && (is_cityjson(p) || is_citygml(p)))
}

/// Whether a JSON file is CityJSON, by its first bytes.
pub fn is_cityjson(path: &Path) -> bool {
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    let s = String::from_utf8_lossy(&head[..n]);
    s.contains("\"CityJSON\"") || s.contains("\"CityJSONFeature\"") || s.contains("\"CityObjects\"")
}

/// Whether an XML or GML file is CityGML, by its first bytes.
pub fn is_citygml(path: &Path) -> bool {
    let mut head = vec![0u8; 8192];
    let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    let s = String::from_utf8_lossy(&head[..n]);
    s.contains("CityModel") && (s.contains("citygml") || s.contains("CityGML"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("og-city-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    /// A box of 10 × 10 × 6 m with its roof, walls and floor, as surfaces.
    const CITYJSON: &str = r#"{
      "type": "CityJSON", "version": "2.0",
      "transform": {"scale": [0.001, 0.001, 0.001], "translate": [418000, 6472000, 80]},
      "metadata": {"referenceSystem": "https://www.opengis.net/def/crs/EPSG/0/3006"},
      "CityObjects": {
        "b1": {"type": "Building", "attributes": {"measuredHeight": 6.0, "function": "bostad", "yearOfConstruction": 1974},
               "children": ["b1-p"],
               "geometry": [
                 {"type": "MultiSurface", "lod": "0", "boundaries": [[[0, 1, 2, 3]]]},
                 {"type": "Solid", "lod": "2.2",
                  "boundaries": [[[[0, 3, 2, 1]], [[4, 5, 6, 7]], [[0, 1, 5, 4]], [[1, 2, 6, 5]], [[2, 3, 7, 6]], [[3, 0, 4, 7]]]],
                  "semantics": {"surfaces": [{"type": "GroundSurface"}, {"type": "RoofSurface"}, {"type": "WallSurface"}], "values": [[0, 1, 2, 2, 2, 2]]}}
               ]},
        "b1-p": {"type": "BuildingPart", "parents": ["b1"], "geometry": []}
      },
      "vertices": [[0,0,0],[10000,0,0],[10000,10000,0],[0,10000,0],[0,0,6000],[10000,0,6000],[10000,10000,6000],[0,10000,6000]]
    }"#;

    #[test]
    fn coordinate_systems_by_their_names() {
        assert_eq!(epsg_in("EPSG:3006"), Some(3006));
        assert_eq!(epsg_in("urn:ogc:def:crs:EPSG::3006"), Some(3006));
        assert_eq!(epsg_in("urn:ogc:def:crs,crs:EPSG:6.12:25833,crs:EPSG:6.12:5783"), Some(25833));
        assert_eq!(epsg_in("https://www.opengis.net/def/crs/EPSG/0/7415"), Some(7415));
        assert_eq!(epsg_in("urn:adv:crs:ETRS89_UTM32*DE_DHHN92_NH"), Some(25832));
        assert_eq!(epsg_in("urn:adv:crs:DE_DHDN"), None);
    }

    #[test]
    fn cityjson_with_semantics_and_levels() {
        let p = tmp("box.city.json", CITYJSON);
        assert!(is_cityjson(&p));
        let mut notes = Notes::default();
        let t = read_cityjson(&p, &Lod::Finest, None, &mut notes).unwrap();
        assert_eq!(t.crs, Some(3006));
        assert_eq!(t.len(), 1, "the part without geometry is left out: {:?}", notes.info);
        let col = |n: &str| t.features[0].attrs[t.field_index(n).unwrap()].clone();
        assert_eq!(col("lod"), Value::Text("2.2".into()));
        assert_eq!(col("function"), Value::Text("bostad".into()));
        assert_eq!(col("yearOfConstruction"), Value::Int(1974));
        assert_eq!(col("height"), Value::Real(6.0));
        let m = t.mesh.as_ref().unwrap();
        assert_eq!(m.tris.len(), 12);
        let (lo, hi) = m.bounds().unwrap();
        assert_eq!((lo, hi), ([418000.0, 6472000.0, 80.0], [418010.0, 6472010.0, 86.0]));
        let roof = m.materials.iter().position(|x| x.name == "roof").unwrap() as u32;
        assert_eq!(m.tris.iter().filter(|t| t.material == roof).count(), 2);
        // The coarsest is the footprint.
        let t0 = read_cityjson(&p, &Lod::Coarsest, None, &mut notes).unwrap();
        assert_eq!(t0.features[0].attrs[3], Value::Text("0".into()));
        assert!(read_cityjson(&p, &Lod::Level("3".into()), None, &mut notes).is_err());
    }

    const CITYGML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
  xmlns:gen="http://www.opengis.net/citygml/generics/2.0" xmlns:gml="http://www.opengis.net/gml">
  <gml:boundedBy><gml:Envelope srsName="urn:ogc:def:crs,crs:EPSG::3006,crs:EPSG::5613" srsDimension="3"/></gml:boundedBy>
  <core:cityObjectMember>
    <bldg:Building gml:id="BLD_1">
      <gml:name>Kv. Eken 3</gml:name>
      <gen:stringAttribute name="objektidentitet"><gen:value>a1b2 &amp; c3</gen:value></gen:stringAttribute>
      <gen:intAttribute name="våningar"><gen:value>2</gen:value></gen:intAttribute>
      <bldg:function>bostad</bldg:function>
      <bldg:measuredHeight uom="m">7.5</bldg:measuredHeight>
      <bldg:boundedBy><bldg:RoofSurface><bldg:lod2MultiSurface><gml:MultiSurface><gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing>
        <gml:posList>6472000 418000 87.5 6472000 418010 87.5 6472010 418010 87.5 6472010 418000 87.5 6472000 418000 87.5</gml:posList>
      </gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:RoofSurface></bldg:boundedBy>
      <bldg:boundedBy><bldg:WallSurface><bldg:lod2MultiSurface><gml:MultiSurface><gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing>
        <gml:pos>6472000 418000 80</gml:pos><gml:pos>6472000 418010 80</gml:pos><gml:pos>6472000 418010 87.5</gml:pos><gml:pos>6472000 418000 87.5</gml:pos><gml:pos>6472000 418000 80</gml:pos>
      </gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
      <bldg:lod1Solid><gml:Solid><gml:exterior><gml:CompositeSurface><gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing>
        <gml:posList>6472000 418000 87.5 6472000 418010 87.5 6472010 418010 87.5 6472000 418000 87.5</gml:posList>
      </gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:CompositeSurface></gml:exterior></gml:Solid></bldg:lod1Solid>
    </bldg:Building>
  </core:cityObjectMember>
</core:CityModel>"#;

    #[test]
    fn citygml_with_generic_attributes_surfaces_and_axes() {
        let p = tmp("house.gml", CITYGML);
        assert!(is_citygml(&p));
        let mut notes = Notes::default();
        let t = read_citygml(&p, &Lod::Finest, None, &mut notes, &|_, _| true).unwrap();
        assert_eq!(t.crs, Some(3006));
        assert_eq!(t.len(), 1);
        let col = |n: &str| t.features[0].attrs[t.field_index(n).unwrap_or_else(|| panic!("no {} in {:?}", n, t.field_names()))].clone();
        assert_eq!(col("id"), Value::Text("BLD_1".into()));
        assert_eq!(col("type"), Value::Text("Building".into()));
        assert_eq!(col("name"), Value::Text("Kv. Eken 3".into()));
        assert_eq!(col("objektidentitet"), Value::Text("a1b2 & c3".into()));
        assert_eq!(col("våningar"), Value::Int(2));
        assert_eq!(col("function"), Value::Text("bostad".into()));
        assert_eq!(col("measuredHeight"), Value::Real(7.5));
        assert_eq!(col("lod"), Value::Text("2".into()));
        // Northing first in the file: swapped to easting first.
        let (lo, hi) = t.mesh.as_ref().unwrap().bounds().unwrap();
        assert_eq!((lo[0], lo[1], hi[2]), (418000.0, 6472000.0, 87.5));
        assert!(notes.info.iter().any(|i| i.contains("swapped")));
        let m = t.mesh.as_ref().unwrap();
        assert!(m.materials.iter().any(|x| x.name == "roof") && m.materials.iter().any(|x| x.name == "wall"));
        let t1 = read_citygml(&p, &Lod::Level("1".into()), None, &mut notes, &|_, _| true).unwrap();
        assert_eq!(t1.mesh.as_ref().unwrap().tris.len(), 1);
    }
}
