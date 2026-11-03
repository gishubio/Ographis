//! DWG drawings, the native format of AutoCAD, read with acadrust and written as DXF, so
//! that they are read as DXF drawings are, with the same attributes.

use crate::model::{GbError, Result};
use std::path::{Path, PathBuf};

/// The DWG as a DXF file in the folder for temporary files, which the caller removes, and the
/// version of AutoCAD the drawing was saved in.
pub fn to_dxf(path: &Path) -> Result<(PathBuf, String, Vec<Body>, BodyReport)> {
    let named = |e: acadrust::DxfError| GbError(format!("{} cannot be read as a DWG drawing: {}", path.display(), e));
    let mut reader = acadrust::DwgReader::from_file_with_options(path, acadrust::DwgReadOptions::failsafe()).map_err(named)?;
    let mut doc = reader.read().map_err(named)?;
    let version = format!("{:?}", doc.version);
    let (bodies, report) = bodies_of(&doc);
    mirror_clockwise_arcs(&mut doc);
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "drawing".into());
    let out = std::env::temp_dir().join(format!("ographis-dwg-{}-{}-{}.dxf", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst), stem));
    acadrust::DxfWriter::new(&doc).write_to_file(&out).map_err(|e| GbError(format!("{} could not be made ready to read: {}", path.display(), e)))?;
    Ok((out, version, bodies, report))
}

/// The clockwise arcs of the edges of hatches as the DXF writer of acadrust expects them.
/// Read from DWG, their angles come as AutoCAD keeps them in DXF too, and the writer turns
/// them once more, so that every clockwise arc came out as its mirror image: the rest of the
/// circle, and with a large radius a circle across the whole drawing.
fn mirror_clockwise_arcs(doc: &mut acadrust::CadDocument) {
    use acadrust::entities::hatch::BoundaryEdge;
    use acadrust::entities::EntityType;
    let tau = std::f64::consts::TAU;
    for e in doc.entities_mut() {
        if let EntityType::Hatch(h) = e {
            for path in h.paths.iter_mut() {
                for edge in path.edges.iter_mut() {
                    if let BoundaryEdge::CircularArc(a) = edge {
                        if !a.counter_clockwise {
                            a.start_angle = (tau - a.start_angle).rem_euclid(tau);
                            a.end_angle = (tau - a.end_angle).rem_euclid(tau);
                        }
                    }
                }
            }
        }
    }
}

/// A 3D body of a drawing: a 3D face, a polyface or polygon mesh, a mesh or a 3D solid, as
/// triangles in the coordinates of the drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// 3DFACE, POLYFACE, POLYGONMESH, MESH or 3DSOLID.
    pub kind: &'static str,
    pub layer: String,
    pub handle: String,
    /// Its colour, or that of its layer.
    pub colour: Option<[u8; 3]>,
    pub tris: Vec<[[f64; 3]; 3]>,
}

/// What could not be made into triangles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyReport {
    /// Faces of 3D solids that are curved, such as cylinders and spheres, left out.
    pub curved_faces: usize,
    /// 3D solids that could not be read.
    pub unread_solids: usize,
}

/// The 3D bodies of a DWG or DXF drawing as triangles.
pub fn bodies(path: &Path) -> Result<(Vec<Body>, BodyReport)> {
    let is_dwg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
    let doc = if is_dwg {
        let named = |e: acadrust::DxfError| GbError(format!("{} cannot be read as a DWG drawing: {}", path.display(), e));
        acadrust::DwgReader::from_file_with_options(path, acadrust::DwgReadOptions::failsafe()).map_err(named)?.read().map_err(named)?
    } else {
        let named = |e: acadrust::DxfError| GbError(format!("{} cannot be read as a DXF drawing: {}", path.display(), e));
        acadrust::DxfReader::from_file(path).map_err(named)?.read().map_err(named)?
    };
    Ok(bodies_of(&doc))
}

fn bodies_of(doc: &acadrust::CadDocument) -> (Vec<Body>, BodyReport) {
    use acadrust::entities::EntityType;
    let mut out = Vec::new();
    let mut report = BodyReport::default();
    let colour_of = |c: &acadrust::entities::EntityCommon| -> Option<[u8; 3]> {
        let own = c.color.rgb();
        let by_layer = || doc.layers.get(&c.layer).and_then(|l| l.color.rgb());
        own.or_else(by_layer).map(|(r, g, b)| [r, g, b])
    };
    let face = |f: &acadrust::entities::Face3D| -> Vec<[[f64; 3]; 3]> {
        let p = |v: &acadrust::types::Vector3| [v.x, v.y, v.z];
        let (a, b, c, d) = (p(&f.first_corner), p(&f.second_corner), p(&f.third_corner), p(&f.fourth_corner));
        let mut t = vec![[a, b, c]];
        if d != c {
            t.push([a, c, d]);
        }
        t.into_iter().filter(|t| area3(t) > 1e-12).collect()
    };
    for e in doc.entities() {
        let (kind, common) = match e {
            EntityType::Face3D(f) => ("3DFACE", &f.common),
            EntityType::PolyfaceMesh(m) => ("POLYFACE", &m.common),
            EntityType::PolygonMesh(m) => ("POLYGONMESH", &m.common),
            EntityType::Mesh(m) => ("MESH", &m.common),
            EntityType::Solid3D(m) => ("3DSOLID", &m.common),
            _ => continue,
        };
        let tris: Vec<[[f64; 3]; 3]> = match e {
            EntityType::Face3D(f) => face(f),
            EntityType::Solid3D(s) => match s.acis_data.parse() {
                Some(sat) => solid_triangles(&sat, &mut report),
                None => {
                    report.unread_solids += 1;
                    Vec::new()
                }
            },
            // Meshes as acadrust breaks them into 3D faces.
            _ => e
                .explode()
                .iter()
                .flat_map(|x| match x {
                    EntityType::Face3D(f) => face(f),
                    _ => Vec::new(),
                })
                .collect(),
        };
        if tris.is_empty() {
            continue;
        }
        out.push(Body { kind, layer: common.layer.clone(), handle: format!("{:X}", common.handle.value()), colour: colour_of(common), tris });
    }
    (out, report)
}

fn area3(t: &[[f64; 3]; 3]) -> f64 {
    let (u, v) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let n = cross(u, v);
    (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() / 2.0
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt().max(1e-300);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// The points along one loop of a face of a solid: the corners, and along arcs and circles
/// points between them.
fn loop_points(sat: &acadrust::entities::acis::SatDocument, first: acadrust::entities::acis::SatPointer) -> Vec<[f64; 3]> {
    use acadrust::entities::acis::{SatCoedge, SatEdge, SatEllipseCurve, SatPoint, SatVertex, Sense};
    let point_of = |v: acadrust::entities::acis::SatPointer| -> Option<[f64; 3]> {
        let vx = sat.resolve(v).and_then(SatVertex::from_record)?;
        let (x, y, z) = sat.resolve(vx.point()).and_then(SatPoint::from_record)?.position();
        Some([x, y, z])
    };
    let mut pts = Vec::new();
    let mut ptr = first;
    let mut seen = std::collections::HashSet::new();
    while !ptr.is_null() && seen.insert(ptr) {
        let Some(co) = sat.resolve(ptr).and_then(SatCoedge::from_record) else { break };
        let Some(edge) = sat.resolve(co.edge()).and_then(SatEdge::from_record) else { break };
        let reversed = co.sense() == Sense::Reversed;
        let (a, b) = (point_of(edge.start_vertex()), point_of(edge.end_vertex()));
        let start = if reversed { b } else { a };
        // An arc or a circle: points along it, from where the coedge starts.
        let arc = sat.resolve(edge.curve()).and_then(SatEllipseCurve::from_record);
        match (arc, start) {
            (Some(c), _) => {
                let (cx, cy, cz) = c.center();
                let (nx, ny, nz) = c.normal();
                let (mx, my, mz) = c.major_axis();
                let major = [mx, my, mz];
                let minor = {
                    let m = cross(unit([nx, ny, nz]), major);
                    let r = c.ratio();
                    [m[0] * r, m[1] * r, m[2] * r]
                };
                let flip = if edge.sense() == Sense::Reversed { -1.0 } else { 1.0 };
                let (t0, t1) = (edge.start_param() * flip, edge.end_param() * flip);
                let span = t1 - t0;
                let n = ((span.abs() / std::f64::consts::TAU) * 32.0).ceil().max(2.0) as usize;
                let at = |t: f64| {
                    [cx + major[0] * t.cos() + minor[0] * t.sin(), cy + major[1] * t.cos() + minor[1] * t.sin(), cz + major[2] * t.cos() + minor[2] * t.sin()]
                };
                let steps: Vec<f64> = (0..n).map(|k| t0 + span * k as f64 / n as f64).collect();
                if reversed {
                    pts.push(at(t1));
                    pts.extend(steps.iter().skip(1).rev().map(|t| at(*t)));
                } else {
                    pts.extend(steps.iter().map(|t| at(*t)));
                }
            }
            (None, Some(p)) => pts.push(p),
            (None, None) => {}
        }
        ptr = co.next();
    }
    pts.dedup_by(|a, b| sub(*a, *b).iter().all(|d| d.abs() < 1e-9));
    pts
}

/// The plane faces of a 3D solid as triangles, placed as the body is. Curved faces are
/// counted and left out.
fn solid_triangles(sat: &acadrust::entities::acis::SatDocument, report: &mut BodyReport) -> Vec<[[f64; 3]; 3]> {
    use acadrust::entities::acis::{SatLoop, SatPlaneSurface};
    use geo::TriangulateEarcut;
    let (m, t, scale) = sat.placement();
    let place = |p: [f64; 3]| -> [f64; 3] {
        let q = [
            p[0] * m[0][0] + p[1] * m[1][0] + p[2] * m[2][0],
            p[0] * m[0][1] + p[1] * m[1][1] + p[2] * m[2][1],
            p[0] * m[0][2] + p[1] * m[1][2] + p[2] * m[2][2],
        ];
        [scale * q[0] + t[0], scale * q[1] + t[1], scale * q[2] + t[2]]
    };
    let mut tris = Vec::new();
    for f in sat.faces() {
        let Some(plane) = sat.resolve(f.surface()).and_then(SatPlaneSurface::from_record) else {
            report.curved_faces += 1;
            continue;
        };
        let (nx, ny, nz) = plane.normal();
        let n = unit([nx, ny, nz]);
        let (ox, oy, oz) = plane.root_point();
        let o = [ox, oy, oz];
        let u = unit(if n[2].abs() < 0.9 { cross([0.0, 0.0, 1.0], n) } else { cross([1.0, 0.0, 0.0], n) });
        let v = cross(n, u);
        let to2 = |p: [f64; 3]| geo::Coord { x: dot(sub(p, o), u), y: dot(sub(p, o), v) };
        let mut rings: Vec<geo::LineString<f64>> = Vec::new();
        let mut lp = f.first_loop();
        let mut seen = std::collections::HashSet::new();
        while !lp.is_null() && seen.insert(lp) {
            let Some(l) = sat.resolve(lp).and_then(SatLoop::from_record) else { break };
            let pts = loop_points(sat, l.first_coedge());
            if pts.len() >= 3 {
                let mut c: Vec<geo::Coord<f64>> = pts.iter().map(|p| to2(*p)).collect();
                c.push(c[0]);
                rings.push(geo::LineString::new(c));
            }
            lp = l.next_loop();
        }
        if rings.is_empty() {
            continue;
        }
        // The outer ring is the largest.
        use geo::Area;
        let outer = (0..rings.len())
            .max_by(|a, b| {
                geo::Polygon::new(rings[*a].clone(), vec![]).unsigned_area().total_cmp(&geo::Polygon::new(rings[*b].clone(), vec![]).unsigned_area())
            })
            .expect("a ring");
        let ext = rings.remove(outer);
        for tri in geo::Polygon::new(ext, rings).earcut_triangles() {
            let back = |c: geo::Coord<f64>| place([o[0] + u[0] * c.x + v[0] * c.y, o[1] + u[1] * c.x + v[1] * c.y, o[2] + u[2] * c.x + v[2] * c.y]);
            let t3 = [back(tri.v1()), back(tri.v2()), back(tri.v3())];
            if area3(&t3) > 1e-12 {
                tris.push(t3);
            }
        }
    }
    tris
}

/// Whether a DXF file may hold 3D bodies, by a quick look for their names, so that a
/// drawing of lines is not read twice.
pub fn may_have_bodies(dxf: &Path) -> bool {
    let Ok(bytes) = std::fs::read(dxf) else { return false };
    let has = |w: &[u8]| bytes.windows(w.len()).any(|x| x == w);
    has(b"3DFACE") || has(b"3DSOLID") || has(b"AcDbPolyFaceMesh") || has(b"AcDbPolygonMesh") || has(b"AcDbSubDMesh")
}

/// The stream of a drawing with its 3D bodies as a 3D model: each body a feature of its own
/// with its layer, kind, colour and handle, and its outline seen from above, in place of what
/// the reading of the DXF made of it: 3D faces as areas, and meshes without geometry. The
/// other features are kept, without triangles.
pub fn add_bodies(mut t: crate::model::Table, bodies: Vec<Body>) -> crate::model::Table {
    use crate::mesh::{Material, Mesh, Tri, NONE};
    use crate::model::{Feature, Value};
    if bodies.is_empty() {
        return t;
    }
    let entity_at = t.field_index("entity");
    t.features.retain(|f| {
        let e = entity_at.map(|i| f.attrs[i].to_text()).unwrap_or_default();
        !(e == "3DFACE" || (f.geom.is_none() && matches!(e.as_str(), "POLYLINE" | "MESH" | "3DSOLID")))
    });
    let at = |n: &str| t.field_index(n);
    let (layer_at, colour_at, handle_at) = (at("layer"), at("colour"), at("handle"));
    let mut mesh = Mesh { objects: (0..t.features.len()).map(|k| format!("feature {}", k + 1)).collect(), ..Default::default() };
    let mut colours: Vec<Option<[u8; 3]>> = Vec::new();
    for b in bodies {
        let mut attrs = vec![Value::Null; t.fields.len()];
        let text = |s: &str| Value::Text(s.to_string());
        if let Some(i) = layer_at {
            attrs[i] = text(&b.layer);
        }
        if let Some(i) = entity_at {
            attrs[i] = text(b.kind);
        }
        if let (Some(i), Some(c)) = (colour_at, b.colour) {
            attrs[i] = Value::Text(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
        }
        if let Some(i) = handle_at {
            attrs[i] = text(&b.handle);
        }
        let points: Vec<geo::Point<f64>> = b.tris.iter().flat_map(|t| t.iter().map(|p| geo::Point::new(p[0], p[1]))).collect();
        let hull = geo::ConvexHull::convex_hull(&geo::MultiPoint::new(points));
        let object = mesh.objects.len() as u32;
        mesh.objects.push(format!("{} {}", b.kind, b.handle));
        let material = match colours.iter().position(|c| *c == b.colour) {
            Some(m) => m as u32,
            None => {
                colours.push(b.colour);
                let c = b.colour.unwrap_or([200, 200, 200]);
                mesh.materials.push(Material {
                    name: format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
                    diffuse: [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0],
                    opacity: 1.0,
                    ..Default::default()
                });
                (colours.len() - 1) as u32
            }
        };
        for tri in &b.tris {
            let v = mesh.positions.len() as u32;
            mesh.positions.extend_from_slice(tri);
            mesh.tris.push(Tri { v: [v, v + 1, v + 2], n: [NONE; 3], t: [NONE; 3], material, object });
        }
        t.features.push(Feature { z: None, attrs, geom: Some(geo::Geometry::Polygon(hull)) });
    }
    t.mesh = Some(std::sync::Arc::new(mesh));
    t
}

/// Writes a DXF drawing as DWG, for the tests and for those who need to.
pub fn dxf_to_dwg(dxf: &Path, dwg: &Path) -> Result<()> {
    let named = |e: acadrust::DxfError| GbError(format!("{}: {}", dxf.display(), e));
    let doc = acadrust::DxfReader::from_file(dxf).map_err(named)?.read().map_err(named)?;
    acadrust::DwgWriter::write_to_file(dwg, &doc).map_err(named)
}

#[cfg(test)]
mod tests {
    /// Writes the DXF of the drawing in OG_DWG beside it, to look at: cargo test -- --ignored.
    #[test]
    #[ignore]
    fn a_drawing_given_is_written_as_dxf() {
        let Ok(p) = std::env::var("OG_DWG") else { return };
        let (dxf, v, _, _) = super::to_dxf(std::path::Path::new(&p)).unwrap();
        std::fs::copy(&dxf, format!("{}.dxf", p)).unwrap();
        println!("{} {}", v, dxf.display());
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;

    #[test]
    fn a_box_solid_is_made_into_triangles() {
        let sat = acadrust::entities::acis::primitives::build_box([10.0, 20.0, 1.5], 2.0, 4.0, 3.0);
        let mut report = BodyReport::default();
        let tris = solid_triangles(&sat, &mut report);
        assert_eq!((tris.len(), report.curved_faces), (12, 0));
        let area: f64 = tris.iter().map(area3).sum();
        assert!((area - 2.0 * (8.0 + 6.0 + 12.0)).abs() < 1e-9, "{}", area);
        let zs: Vec<f64> = tris.iter().flat_map(|t| t.iter().map(|p| p[2])).collect();
        assert!(zs.iter().all(|z| (*z - 0.0).abs() < 1e-9 || (*z - 3.0).abs() < 1e-9), "{:?}", zs);
        let cyl = acadrust::entities::acis::primitives::build_cylinder([0.0, 0.0, 0.0], 1.0, 2.0);
        let tris = solid_triangles(&cyl, &mut report);
        assert!(report.curved_faces >= 1, "the side of a cylinder is left out");
        let area: f64 = tris.iter().map(area3).sum();
        assert!((area - 2.0 * std::f64::consts::PI).abs() < 0.1, "the two discs, {}", area);
    }

    /// The bodies of the drawing in OG_DWG: cargo test -- --ignored.
    #[test]
    #[ignore]
    fn the_bodies_of_a_drawing_given() {
        let Ok(p) = std::env::var("OG_DWG") else { return };
        let (b, r) = bodies(Path::new(&p)).unwrap();
        let mut kinds: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
        for x in &b {
            let e = kinds.entry(x.kind).or_default();
            e.0 += 1;
            e.1 += x.tris.len();
        }
        println!("{:?} {:?}", kinds, r);
    }
}
