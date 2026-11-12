//! LandXML, as civil engineering exchanges terrain and design: surfaces as triangles (a TIN),
//! points (CgPoints) and alignments as lines; and a TIN and points written as LandXML.

/// Whether a file is LandXML: its root element is LandXML.
pub fn is_landxml(file: &std::path::Path) -> bool {
    use std::io::Read;
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(file).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    String::from_utf8_lossy(&head[..n]).contains("<LandXML")
}

use crate::mesh::{Material, Mesh, Tri, NONE};
use crate::model::{Feature, Field, FieldType, GbError, Result, Table, Value};
use quick_xml::events::Event;
use std::sync::Arc;

/// What a LandXML file holds, each as a layer of its own.
pub const LAYERS: &[&str] = &["surfaces", "points", "alignments"];

/// A surface of triangles as it is read: its name and its points and faces.
struct Surface {
    name: String,
    points: Vec<[f64; 3]>,
    ids: std::collections::HashMap<String, u32>,
    faces: Vec<[String; 3]>,
}

/// Three numbers of LandXML: northing, easting and elevation, in that order, given as x, y, z.
fn nez(text: &str) -> Option<[f64; 3]> {
    let v: Vec<f64> = text.split_whitespace().filter_map(|s| s.parse().ok()).collect();
    match v.as_slice() {
        [n, e, z, ..] => Some([*e, *n, *z]),
        [n, e] => Some([*e, *n, 0.0]),
        _ => None,
    }
}

/// What was read: the layers that have something, and the EPSG code the file names.
pub struct Read {
    pub surfaces: Table,
    pub points: Table,
    pub alignments: Table,
    pub epsg: Option<u32>,
}

/// Reads a LandXML file: surfaces as a 3D model of triangles with a feature each, points
/// with their heights, and alignments as lines with their lines and arcs.
pub fn read(path: &std::path::Path) -> Result<Read> {
    let text = std::fs::read_to_string(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let mut r = quick_xml::Reader::from_str(&text);
    let mut stack: Vec<String> = Vec::new();
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut points: Vec<(String, String, [f64; 3])> = Vec::new();
    // Alignments: name, length, start station, and their geometry as lines.
    let mut alignments: Vec<(String, f64, f64, Vec<geo::Coord<f64>>)> = Vec::new();
    let mut epsg: Option<u32> = None;
    let mut text_buf = String::new();
    let mut attrs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    // An arc as it is read: start, centre, end, and whether it turns clockwise.
    let mut arc: ArcRead = (None, None, None, false);
    let mut line: (Option<[f64; 3]>, Option<[f64; 3]>) = (None, None);
    let decoder = r.decoder();
    let attrs_of = |e: &quick_xml::events::BytesStart| -> std::collections::HashMap<String, String> {
        e.attributes()
            .flatten()
            .map(|a| {
                (
                    String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned(),
                    a.decode_and_unescape_value(decoder).map(|v| v.into_owned()).unwrap_or_default(),
                )
            })
            .collect()
    };
    loop {
        let ev = r.read_event().map_err(|e| GbError(format!("The LandXML could not be read at byte {}: {}", r.buffer_position(), e)))?;
        let opened = match &ev {
            Event::Start(e) => Some((String::from_utf8_lossy(e.local_name().as_ref()).into_owned(), attrs_of(e), false)),
            Event::Empty(e) => Some((String::from_utf8_lossy(e.local_name().as_ref()).into_owned(), attrs_of(e), true)),
            _ => None,
        };
        match ev {
            Event::Start(_) | Event::Empty(_) => {
                let Some((name, a, empty)) = opened else { continue };
                match name.as_str() {
                    "CoordinateSystem" => {
                        epsg = a.get("epsgCode").and_then(|c| c.trim().parse().ok()).or(epsg);
                    }
                    "Surface" => surfaces.push(Surface {
                        name: a.get("name").cloned().unwrap_or_else(|| format!("Surface {}", surfaces.len() + 1)),
                        points: Vec::new(),
                        ids: Default::default(),
                        faces: Vec::new(),
                    }),
                    "Alignment" => alignments.push((
                        a.get("name").cloned().unwrap_or_else(|| format!("Alignment {}", alignments.len() + 1)),
                        a.get("length").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                        a.get("staStart").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                        Vec::new(),
                    )),
                    "Curve" if stack.iter().any(|s| s == "CoordGeom") => arc = (None, None, None, a.get("rot").is_some_and(|r| r == "cw")),
                    "Line" if stack.iter().any(|s| s == "CoordGeom") => line = (None, None),
                    _ => {}
                }
                attrs = a;
                text_buf.clear();
                // An empty element, as <X/>, has no end to wait for.
                if !empty {
                    stack.push(name);
                }
            }
            Event::Text(t) => {
                text_buf.push_str(&String::from_utf8_lossy(t.as_ref()));
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                stack.pop();
                let parent = stack.last().map(|s| s.as_str()).unwrap_or("");
                match (name.as_str(), parent) {
                    ("P", "Pnts") => {
                        if let (Some(s), Some(p)) = (surfaces.last_mut(), nez(&text_buf)) {
                            let id = attrs.get("id").cloned().unwrap_or_else(|| (s.points.len() + 1).to_string());
                            s.ids.insert(id, s.points.len() as u32);
                            s.points.push(p);
                        }
                    }
                    ("F", "Faces") => {
                        let v: Vec<String> = text_buf.split_whitespace().map(str::to_string).collect();
                        // A face marked i="1" is left out of the surface, as a hole.
                        if let (Some(s), [a, b, c, ..]) = (surfaces.last_mut(), v.as_slice()) {
                            if attrs.get("i").map(|i| i.as_str()) != Some("1") {
                                s.faces.push([a.clone(), b.clone(), c.clone()]);
                            }
                        }
                    }
                    ("CgPoint", _) => {
                        if let Some(p) = nez(&text_buf) {
                            let label = attrs.get("name").or(attrs.get("oID")).cloned().unwrap_or_default();
                            points.push((label, attrs.get("code").or(attrs.get("desc")).cloned().unwrap_or_default(), p));
                        }
                    }
                    ("Start", "Line") => line.0 = nez(&text_buf),
                    ("End", "Line") => line.1 = nez(&text_buf),
                    ("Start", "Curve") => arc.0 = nez(&text_buf),
                    ("Center", "Curve") => arc.1 = nez(&text_buf),
                    ("End", "Curve") => arc.2 = nez(&text_buf),
                    ("Start", "Spiral") | ("End", "Spiral") => {
                        // A spiral is taken as the straight line between its ends.
                        if let (Some(a), Some(p)) = (alignments.last_mut(), nez(&text_buf)) {
                            push(&mut a.3, geo::coord! { x: p[0], y: p[1] });
                        }
                    }
                    ("Line", _) if stack.iter().any(|s| s == "CoordGeom") => {
                        if let (Some(a), (Some(s), Some(e))) = (alignments.last_mut(), line) {
                            push(&mut a.3, geo::coord! { x: s[0], y: s[1] });
                            push(&mut a.3, geo::coord! { x: e[0], y: e[1] });
                        }
                    }
                    ("Curve", _) if stack.iter().any(|s| s == "CoordGeom") => {
                        if let (Some(a), (Some(s), Some(c), Some(e), cw)) = (alignments.last_mut(), arc) {
                            for p in arc_points(s, c, e, cw) {
                                push(&mut a.3, p);
                            }
                        }
                    }
                    _ => {}
                }
                text_buf.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    // Surfaces: a 3D model of triangles, with a feature of its outline box for each.
    let mut st = Table {
        fields: vec![Field::new("name", FieldType::Text), Field::new("points", FieldType::Int), Field::new("triangles", FieldType::Int)],
        ..Default::default()
    };
    let mut mesh = Mesh { materials: vec![Material { name: "surface".into(), diffuse: [0.72, 0.68, 0.58], ..Material::default() }], ..Mesh::default() };
    let mut missing = 0usize;
    for (k, s) in surfaces.iter().enumerate() {
        let base = mesh.positions.len() as u32;
        mesh.positions.extend(s.points.iter().copied());
        let mut n = 0i64;
        for f in &s.faces {
            let v: Option<Vec<u32>> = f.iter().map(|id| s.ids.get(id).map(|i| base + i)).collect();
            match v {
                Some(v) => {
                    mesh.tris.push(Tri { v: [v[0], v[1], v[2]], n: [NONE; 3], t: [NONE; 3], material: 0, object: k as u32 });
                    n += 1;
                }
                None => missing += 1,
            }
        }
        mesh.objects.push(s.name.clone());
        let (lo, hi) = s
            .points
            .iter()
            .fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
        let geom = (lo[0].is_finite())
            .then(|| geo::Geometry::Polygon(geo::Rect::new(geo::coord! { x: lo[0], y: lo[1] }, geo::coord! { x: hi[0], y: hi[1] }).to_polygon()));
        st.features.push(Feature { attrs: vec![Value::Text(s.name.clone()), Value::Int(s.points.len() as i64), Value::Int(n)], geom, z: None });
    }
    if missing > 0 {
        return Err(GbError(format!("{} triangles of the surfaces name points that are not there.", missing)));
    }
    if !surfaces.is_empty() {
        st.mesh = Some(Arc::new(mesh));
    }
    // Points, with their heights.
    let mut pt = Table {
        fields: vec![Field::new("name", FieldType::Text), Field::new("code", FieldType::Text), Field::new("z", FieldType::Real)],
        ..Default::default()
    };
    for (name, code, p) in points {
        pt.features.push(Feature {
            attrs: vec![Value::Text(name), if code.is_empty() { Value::Null } else { Value::Text(code) }, Value::Real(p[2])],
            geom: Some(geo::Geometry::Point(geo::Point::new(p[0], p[1]))),
            z: Some(Arc::from(vec![p[2]])),
        });
    }
    // Alignments, as lines.
    let mut al = Table {
        fields: vec![Field::new("name", FieldType::Text), Field::new("length", FieldType::Real), Field::new("station_start", FieldType::Real)],
        ..Default::default()
    };
    for (name, length, start, coords) in alignments {
        let geom = (coords.len() >= 2).then(|| geo::Geometry::LineString(geo::LineString::new(coords)));
        al.features.push(Feature { attrs: vec![Value::Text(name), Value::Real(length), Value::Real(start)], geom, z: None });
    }
    Ok(Read { surfaces: st, points: pt, alignments: al, epsg })
}

/// Adds a coordinate to a line, unless it is where the line already is.
fn push(line: &mut Vec<geo::Coord<f64>>, c: geo::Coord<f64>) {
    if line.last().is_none_or(|l| (l.x - c.x).abs() > 1e-9 || (l.y - c.y).abs() > 1e-9) {
        line.push(c);
    }
}

/// Points along an arc from start to end around its centre, about every 2 degrees.
fn arc_points(s: [f64; 3], c: [f64; 3], e: [f64; 3], cw: bool) -> Vec<geo::Coord<f64>> {
    let r = (s[0] - c[0]).hypot(s[1] - c[1]);
    let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
    let a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
    let mut sweep = a1 - a0;
    let tau = std::f64::consts::TAU;
    if cw {
        while sweep > 0.0 {
            sweep -= tau;
        }
    } else {
        while sweep < 0.0 {
            sweep += tau;
        }
    }
    let steps = ((sweep.abs() / 2f64.to_radians()).ceil() as usize).clamp(2, 720);
    (0..=steps)
        .map(|k| {
            let a = a0 + sweep * k as f64 / steps as f64;
            geo::coord! { x: c[0] + r * a.cos(), y: c[1] + r * a.sin() }
        })
        .collect()
}

/// A TIN and points written as LandXML: the triangles of each object of the 3D model as a
/// surface, and points with heights as CgPoints, with the coordinate system by its EPSG code.
pub fn write(table: &Table, epsg: Option<u32>) -> Result<String> {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<LandXML xmlns=\"http://www.landxml.org/schema/LandXML-1.2\" version=\"1.2\">\n");
    out.push_str("  <Units><Metric linearUnit=\"meter\" areaUnit=\"squareMeter\" volumeUnit=\"cubicMeter\" angularUnit=\"decimal degrees\" directionUnit=\"decimal degrees\"/></Units>\n");
    if let Some(c) = epsg {
        out.push_str(&format!("  <CoordinateSystem epsgCode=\"{}\"/>\n", c));
    }
    out.push_str("  <Application name=\"Ographis\"/>\n");
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
    let mut wrote = false;
    if let Some(m) = table.mesh.as_deref() {
        out.push_str("  <Surfaces>\n");
        let names: Vec<String> = if m.objects.is_empty() { vec!["Surface".into()] } else { m.objects.clone() };
        for (k, name) in names.iter().enumerate() {
            let tris: Vec<&Tri> = m.tris.iter().filter(|t| m.objects.is_empty() || t.object as usize == k).collect();
            if tris.is_empty() {
                continue;
            }
            // The points the triangles use, numbered from 1.
            let mut id: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
            let mut pnts = String::new();
            for t in &tris {
                for v in t.v {
                    if !id.contains_key(&v) {
                        let n = id.len() + 1;
                        id.insert(v, n);
                        let p = m.positions[v as usize];
                        pnts.push_str(&format!("          <P id=\"{}\">{:.4} {:.4} {:.4}</P>\n", n, p[1], p[0], p[2]));
                    }
                }
            }
            out.push_str(&format!(
                "    <Surface name=\"{}\">\n      <Definition surfType=\"TIN\">\n        <Pnts>\n{}        </Pnts>\n        <Faces>\n",
                esc(name),
                pnts
            ));
            for t in &tris {
                out.push_str(&format!("          <F>{} {} {}</F>\n", id[&t.v[0]], id[&t.v[1]], id[&t.v[2]]));
            }
            out.push_str("        </Faces>\n      </Definition>\n    </Surface>\n");
            wrote = true;
        }
        out.push_str("  </Surfaces>\n");
    }
    let name_at = table.field_index("name");
    let code_at = table.field_index("code");
    let pts: Vec<(usize, &Feature)> = table.features.iter().enumerate().filter(|(_, f)| matches!(f.geom, Some(geo::Geometry::Point(_)))).collect();
    if !pts.is_empty() {
        out.push_str("  <CgPoints>\n");
        for (k, f) in pts {
            let Some(geo::Geometry::Point(p)) = &f.geom else { continue };
            let z = f.heights().and_then(|z| z.first().copied()).or_else(|| table.field_index("z").and_then(|i| f.attrs[i].as_f64())).unwrap_or(0.0);
            let name = name_at.map(|i| f.attrs[i].to_text()).filter(|s| !s.is_empty()).unwrap_or_else(|| (k + 1).to_string());
            let code = code_at.map(|i| f.attrs[i].to_text()).filter(|s| !s.is_empty()).map(|c| format!(" code=\"{}\"", esc(&c))).unwrap_or_default();
            out.push_str(&format!("    <CgPoint name=\"{}\"{}>{:.4} {:.4} {:.4}</CgPoint>\n", esc(&name), code, p.y(), p.x(), z));
        }
        out.push_str("  </CgPoints>\n");
        wrote = true;
    }
    if !wrote {
        return Err(GbError(
            "LandXML is written from a 3D model of triangles, such as from Generate TIN, or from points with heights. This stream has neither.".into(),
        ));
    }
    out.push_str("</LandXML>\n");
    Ok(out)
}

/// An arc as it is read: start, centre, end, and whether it turns clockwise.
type ArcRead = (Option<[f64; 3]>, Option<[f64; 3]>, Option<[f64; 3]>, bool);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surface_points_and_an_alignment_are_read_and_a_tin_written() {
        let dir = std::env::temp_dir().join(format!("og-landxml-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("site.xml");
        std::fs::write(
            &file,
            r#"<?xml version="1.0"?>
<LandXML xmlns="http://www.landxml.org/schema/LandXML-1.2" version="1.2">
  <CoordinateSystem epsgCode="3006"/>
  <CgPoints><CgPoint name="P1" code="BRUNN">6400010 300010 101.5</CgPoint></CgPoints>
  <Surfaces><Surface name="Mark"><Definition surfType="TIN">
    <Pnts><P id="1">6400000 300000 100</P><P id="2">6400000 300020 101</P><P id="3">6400020 300000 102</P><P id="4">6400020 300020 103</P></Pnts>
    <Faces><F>1 2 3</F><F>2 4 3</F></Faces>
  </Definition></Surface></Surfaces>
  <Alignments><Alignment name="Väg 1" length="45.7" staStart="0">
    <CoordGeom>
      <Line><Start>6400000 300000</Start><End>6400000 300030</End></Line>
      <Curve rot="ccw"><Start>6400000 300030</Start><Center>6400010 300030</Center><End>6400020 300030</End></Curve>
    </CoordGeom>
  </Alignment></Alignments>
</LandXML>"#,
        )
        .unwrap();
        assert!(is_landxml(&file));
        let r = read(&file).unwrap();
        assert_eq!(r.epsg, Some(3006));
        let m = r.surfaces.mesh.as_deref().unwrap();
        assert_eq!((m.positions.len(), m.tris.len()), (4, 2));
        assert_eq!(m.positions[1], [300020.0, 6400000.0, 101.0], "northing, easting, elevation become x, y, z");
        assert_eq!(r.points.features[0].attrs[1], Value::Text("BRUNN".into()));
        let line = match &r.alignments.features[0].geom {
            Some(geo::Geometry::LineString(l)) => l.clone(),
            _ => panic!("an alignment is a line"),
        };
        assert!(line.0.len() > 10, "the arc is drawn with many points");
        let last = line.0.last().unwrap();
        assert!((last.x - 300030.0).abs() < 1e-6 && (last.y - 6400020.0).abs() < 1e-6);
        // Written and read again: the same surface.
        let text = write(&r.surfaces, Some(3006)).unwrap();
        std::fs::write(dir.join("again.xml"), text).unwrap();
        let again = read(&dir.join("again.xml")).unwrap();
        assert_eq!(again.surfaces.mesh.as_deref().unwrap().tris.len(), 2);
        assert_eq!(again.epsg, Some(3006));
        let _ = std::fs::remove_dir_all(dir);
    }
}
