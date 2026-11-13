//! Formats for 3D printing and CAD: STL and 3MF, read and written, and STEP, written as
//! faceted solids and read where its faces are triangles, polygons or flat faces with
//! straight edges. A model is written in millimetres, at a scale such as 1:1000, with its
//! lowest corner at 0, so that it stands on the plate of the printer.

use crate::mesh::{Material, Mesh, Tri, NONE};
use crate::model::{err, fmt_count, GbError, Result, Table};
use crate::obj_io::{model_table, placement, ObjOpts};
use crate::Notes;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

// ---------- Writing ----------

/// How a model is written for printing.
#[derive(Debug, Clone, Copy)]
pub struct PrintOpts {
    /// The scale as its denominator, 1000 for 1:1000; None fits the model in `fit_mm`.
    pub scale: Option<f64>,
    pub fit_mm: f64,
}

/// Where a model goes in a file for printing: millimetres from its lowest corner.
pub struct Frame {
    pub low: [f64; 3],
    /// Millimetres of the file for a metre of the map.
    pub mm_per_m: f64,
    pub scale: f64,
}

pub fn frame_of(mesh: &Mesh, o: &PrintOpts) -> Result<Frame> {
    let Some((lo, hi)) = mesh.bounds() else { return err("The model has no triangles to write.") };
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]).max(1e-9);
    let scale = match o.scale {
        Some(s) if s > 0.0 => s,
        _ => (size * 1000.0 / o.fit_mm).max(1e-9),
    };
    Ok(Frame { low: lo, mm_per_m: 1000.0 / scale, scale })
}

impl Frame {
    fn at(&self, p: [f64; 3]) -> [f64; 3] {
        [(p[0] - self.low[0]) * self.mm_per_m, (p[1] - self.low[1]) * self.mm_per_m, (p[2] - self.low[2]) * self.mm_per_m]
    }
}

fn normal(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f32; 3] {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 {
        [(n[0] / l) as f32, (n[1] / l) as f32, (n[2] / l) as f32]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Writes a binary STL, in millimetres.
pub fn write_stl(mesh: &Mesh, path: &Path, f: &Frame) -> Result<()> {
    let named = |e: std::io::Error| GbError(format!("Cannot write {}: {}", path.display(), e));
    let mut out: Vec<u8> = Vec::with_capacity(84 + mesh.tris.len() * 50);
    let mut head = format!("Ographis, mm, 1:{}", crate::model::fmt_real(f.scale.round())).into_bytes();
    head.resize(80, b' ');
    out.extend_from_slice(&head);
    out.extend_from_slice(&(mesh.tris.len() as u32).to_le_bytes());
    for t in &mesh.tris {
        let [a, b, c] = t.v.map(|i| f.at(mesh.positions[i as usize]));
        for v in normal(a, b, c) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for p in [a, b, c] {
            for v in p {
                out.extend_from_slice(&(v as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    std::fs::write(path, out).map_err(named)
}

fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The corners of an object by their number in the model, the corners in its order, and its
/// triangles.
type ObjectParts<'a> = (HashMap<u32, u32>, Vec<u32>, Vec<&'a Tri>);
/// A triangle of 3MF: its corners and its colour, as a group and an index in it.
type Tri3mf = ([usize; 3], Option<(String, usize)>);
/// The colour groups of 3MF: each colour and the name of its material.
type Groups3mf = HashMap<String, Vec<([f32; 3], Option<String>)>>;

/// The corners and triangles of each object, numbered within it.
fn by_object(mesh: &Mesh) -> Vec<(Vec<u32>, Vec<&Tri>)> {
    let mut out: Vec<ObjectParts> = vec![(HashMap::new(), Vec::new(), Vec::new()); mesh.objects.len().max(1)];
    for t in &mesh.tris {
        let o = (t.object as usize).min(out.len() - 1);
        let (map, corners, tris) = &mut out[o];
        for v in t.v {
            map.entry(v).or_insert_with(|| {
                corners.push(v);
                corners.len() as u32 - 1
            });
        }
        tris.push(t);
    }
    out.into_iter().map(|(_, c, t)| (c, t)).collect()
}

/// Writes a 3MF package: an object for each, coloured by its materials, in millimetres.
pub fn write_3mf(mesh: &Mesh, path: &Path, f: &Frame) -> Result<()> {
    let mut model = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n");
    model.push_str(&format!("  <metadata name=\"Application\">Ographis {}</metadata>\n", env!("CARGO_PKG_VERSION")));
    model.push_str(&format!("  <metadata name=\"Description\">1:{}</metadata>\n  <resources>\n", crate::model::fmt_real(f.scale.round())));
    let colours = !mesh.materials.is_empty();
    if colours {
        model.push_str("    <basematerials id=\"1\">\n");
        for m in &mesh.materials {
            let c = m.colour().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            model.push_str(&format!("      <base name=\"{}\" displaycolor=\"#{:02X}{:02X}{:02X}FF\" />\n", xml_text(&m.name), c[0], c[1], c[2]));
        }
        model.push_str("    </basematerials>\n");
    }
    let objects = by_object(mesh);
    let mut ids = Vec::new();
    for (k, (corners, tris)) in objects.iter().enumerate() {
        if tris.is_empty() {
            continue;
        }
        let id = k + 2;
        ids.push(id);
        let name = mesh.objects.get(k).cloned().unwrap_or_default();
        let first = tris[0].material as usize;
        let pid = if colours { format!(" pid=\"1\" pindex=\"{}\"", first.min(mesh.materials.len() - 1)) } else { String::new() };
        model.push_str(&format!("    <object id=\"{}\" type=\"model\" name=\"{}\"{}>\n      <mesh>\n        <vertices>\n", id, xml_text(&name), pid));
        let mut local: HashMap<u32, usize> = HashMap::new();
        for (i, v) in corners.iter().enumerate() {
            local.insert(*v, i);
            let p = f.at(mesh.positions[*v as usize]);
            model.push_str(&format!("          <vertex x=\"{:.4}\" y=\"{:.4}\" z=\"{:.4}\" />\n", p[0], p[1], p[2]));
        }
        model.push_str("        </vertices>\n        <triangles>\n");
        for t in tris {
            let [a, b, c] = t.v.map(|v| local[&v]);
            let p = if colours && (t.material as usize) < mesh.materials.len() && t.material as usize != first {
                format!(" pid=\"1\" p1=\"{}\"", t.material)
            } else {
                String::new()
            };
            model.push_str(&format!("          <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"{} />\n", a, b, c, p));
        }
        model.push_str("        </triangles>\n      </mesh>\n    </object>\n");
    }
    model.push_str("  </resources>\n  <build>\n");
    for id in ids {
        model.push_str(&format!("    <item objectid=\"{}\" />\n", id));
    }
    model.push_str("  </build>\n</model>\n");
    let named = |e: zip::result::ZipError| GbError(format!("Cannot write {}: {}", path.display(), e));
    let file = std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    let mut z = zip::ZipWriter::new(file);
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let parts = [
        (
            "[Content_Types].xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\" /><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\" /></Types>\n".to_string(),
        ),
        (
            "_rels/.rels",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\" /></Relationships>\n".to_string(),
        ),
        ("3D/3dmodel.model", model),
    ];
    for (name, text) in parts {
        z.start_file(name, o).map_err(named)?;
        z.write_all(text.as_bytes()).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    z.finish().map_err(named)?;
    Ok(())
}

/// Writes STEP (AP214): each object a solid of flat faces with straight edges when it is
/// closed, otherwise a surface model, coloured, in millimetres.
pub fn write_step(mesh: &Mesh, path: &Path, f: &Frame) -> Result<()> {
    let mut n = 0u64;
    let mut lines: Vec<String> = Vec::new();
    let mut add = |lines: &mut Vec<String>, s: String| -> u64 {
        n += 1;
        lines.push(format!("#{}={};", n, s));
        n
    };
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").replace('\'', "");
    // The context: millimetres, and a product the shapes belong to.
    let app = add(&mut lines, "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into());
    add(&mut lines, format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{})", app));
    let pc = add(&mut lines, format!("PRODUCT_CONTEXT('',#{},'mechanical')", app));
    let product = add(&mut lines, format!("PRODUCT('{0}','{0}','',(#{1}))", name, pc));
    let pdfw = add(&mut lines, format!("PRODUCT_DEFINITION_FORMATION('','',#{})", product));
    let pdc = add(&mut lines, format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{},'design')", app));
    let pd = add(&mut lines, format!("PRODUCT_DEFINITION('design','',#{},#{})", pdfw, pdc));
    let pds = add(&mut lines, format!("PRODUCT_DEFINITION_SHAPE('','',#{})", pd));
    let mm = add(&mut lines, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let rad = add(&mut lines, "(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sr = add(&mut lines, "(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let unc = add(&mut lines, format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-04),#{},'distance_accuracy_value','')", mm));
    let ctx = add(
        &mut lines,
        format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{},#{}))REPRESENTATION_CONTEXT('',''))", unc, mm, rad, sr),
    );
    let origin = add(&mut lines, "CARTESIAN_POINT('',(0.,0.,0.))".into());
    let zdir = add(&mut lines, "DIRECTION('',(0.,0.,1.))".into());
    let xdir = add(&mut lines, "DIRECTION('',(1.,0.,0.))".into());
    let axis = add(&mut lines, format!("AXIS2_PLACEMENT_3D('',#{},#{},#{})", origin, zdir, xdir));
    let mut solids: Vec<u64> = Vec::new();
    let mut styled: Vec<u64> = Vec::new();
    let num = |v: f64| {
        let s = format!("{:.4}", v);
        let s = s.trim_end_matches('0');
        if s.contains('.') {
            s.to_string()
        } else {
            format!("{}.", s)
        }
    };
    for (k, (corners, tris)) in by_object(mesh).iter().enumerate() {
        if tris.is_empty() {
            continue;
        }
        // Points and vertices, each corner once.
        let mut vertex: HashMap<u32, (u64, [f64; 3])> = HashMap::new();
        for v in corners {
            let p = f.at(mesh.positions[*v as usize]);
            let pt = add(&mut lines, format!("CARTESIAN_POINT('',({},{},{}))", num(p[0]), num(p[1]), num(p[2])));
            let vp = add(&mut lines, format!("VERTEX_POINT('',#{})", pt));
            vertex.insert(*v, (vp, p));
        }
        // Each edge once, shared by the faces on either side: from the lower corner.
        let mut uses: HashMap<(u32, u32), usize> = HashMap::new();
        for t in tris {
            for e in 0..3 {
                let (a, b) = (t.v[e], t.v[(e + 1) % 3]);
                *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let closed = uses.values().all(|n| *n == 2);
        let mut edge: HashMap<(u32, u32), u64> = HashMap::new();
        let mut faces = Vec::with_capacity(tris.len());
        for t in tris {
            let p = t.v.map(|v| vertex[&v].1);
            let n = normal(p[0], p[1], p[2]);
            let (ux, uy, uz) = (p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]);
            let ul = (ux * ux + uy * uy + uz * uz).sqrt();
            // A triangle without area has no plane, and is left out.
            let (vx, vy, vz) = (p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]);
            let cross = ((uy * vz - uz * vy).powi(2) + (uz * vx - ux * vz).powi(2) + (ux * vy - uy * vx).powi(2)).sqrt();
            if ul <= 0.0 || cross <= 1e-12 {
                continue;
            }
            let mut oriented = Vec::with_capacity(3);
            for e in 0..3 {
                let (a, b) = (t.v[e], t.v[(e + 1) % 3]);
                let key = (a.min(b), a.max(b));
                let id = match edge.get(&key) {
                    Some(id) => *id,
                    None => {
                        let (pa, pb) = (vertex[&key.0], vertex[&key.1]);
                        let d = [pb.1[0] - pa.1[0], pb.1[1] - pa.1[1], pb.1[2] - pa.1[2]];
                        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
                        let dir = add(&mut lines, format!("DIRECTION('',({},{},{}))", num(d[0] / len), num(d[1] / len), num(d[2] / len)));
                        let vec = add(&mut lines, format!("VECTOR('',#{},{})", dir, num(len)));
                        let start = add(&mut lines, format!("CARTESIAN_POINT('',({},{},{}))", num(pa.1[0]), num(pa.1[1]), num(pa.1[2])));
                        let line = add(&mut lines, format!("LINE('',#{},#{})", start, vec));
                        let id = add(&mut lines, format!("EDGE_CURVE('',#{},#{},#{},.T.)", pa.0, pb.0, line));
                        edge.insert(key, id);
                        id
                    }
                };
                oriented.push(add(&mut lines, format!("ORIENTED_EDGE('',*,*,#{},{})", id, if a < b { ".T." } else { ".F." })));
            }
            let lp = add(&mut lines, format!("EDGE_LOOP('',(#{},#{},#{}))", oriented[0], oriented[1], oriented[2]));
            let bound = add(&mut lines, format!("FACE_OUTER_BOUND('',#{},.T.)", lp));
            let o = add(&mut lines, format!("CARTESIAN_POINT('',({},{},{}))", num(p[0][0]), num(p[0][1]), num(p[0][2])));
            let axis = add(&mut lines, format!("DIRECTION('',({},{},{}))", num(n[0] as f64), num(n[1] as f64), num(n[2] as f64)));
            let refd = add(&mut lines, format!("DIRECTION('',({},{},{}))", num(ux / ul), num(uy / ul), num(uz / ul)));
            let place = add(&mut lines, format!("AXIS2_PLACEMENT_3D('',#{},#{},#{})", o, axis, refd));
            let plane = add(&mut lines, format!("PLANE('',#{})", place));
            faces.push(add(&mut lines, format!("ADVANCED_FACE('',(#{}),#{},.T.)", bound, plane)));
        }
        if faces.is_empty() {
            continue;
        }
        let list = faces.iter().map(|f| format!("#{}", f)).collect::<Vec<_>>().join(",");
        let label = mesh.objects.get(k).cloned().unwrap_or_default().replace('\'', "");
        // A closed model is a solid; an open one a surface model.
        let solid = if closed {
            let shell = add(&mut lines, format!("CLOSED_SHELL('',({}))", list));
            add(&mut lines, format!("MANIFOLD_SOLID_BREP('{}',#{})", label, shell))
        } else {
            let shell = add(&mut lines, format!("OPEN_SHELL('',({}))", list));
            add(&mut lines, format!("SHELL_BASED_SURFACE_MODEL('{}',(#{}))", label, shell))
        };
        solids.push(solid);
        // The colour of its first material.
        if let Some(m) = mesh.materials.get(tris[0].material as usize) {
            let c = m.colour();
            let rgb = add(&mut lines, format!("COLOUR_RGB('',{},{},{})", num(c[0] as f64), num(c[1] as f64), num(c[2] as f64)));
            let fill = add(&mut lines, format!("FILL_AREA_STYLE_COLOUR('',#{})", rgb));
            let area = add(&mut lines, format!("FILL_AREA_STYLE('',(#{}))", fill));
            let ssfa = add(&mut lines, format!("SURFACE_STYLE_FILL_AREA(#{})", area));
            let side = add(&mut lines, format!("SURFACE_SIDE_STYLE('',(#{}))", ssfa));
            let usage = add(&mut lines, format!("SURFACE_STYLE_USAGE(.BOTH.,#{})", side));
            let psa = add(&mut lines, format!("PRESENTATION_STYLE_ASSIGNMENT((#{}))", usage));
            styled.push(add(&mut lines, format!("STYLED_ITEM('color',(#{}),#{})", psa, solid)));
        }
    }
    if solids.is_empty() {
        return err("The model has no triangles to write.");
    }
    let items = std::iter::once(axis).chain(solids.iter().copied()).map(|s| format!("#{}", s)).collect::<Vec<_>>().join(",");
    let rep = add(&mut lines, format!("SHAPE_REPRESENTATION('{}',({}),#{})", name, items, ctx));
    add(&mut lines, format!("SHAPE_DEFINITION_REPRESENTATION(#{},#{})", pds, rep));
    if !styled.is_empty() {
        let list = styled.iter().map(|s| format!("#{}", s)).collect::<Vec<_>>().join(",");
        add(&mut lines, format!("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({}),#{})", list, ctx));
    }
    let header = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Ographis, mm, 1:{}'),'2;1');\nFILE_NAME('{}.step','{}',(''),(''),'Ographis {}','Ographis','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n",
        crate::model::fmt_real(f.scale.round()),
        name,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"),
        env!("CARGO_PKG_VERSION")
    );
    let mut text = header;
    for l in lines {
        text.push_str(&l);
        text.push('\n');
    }
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    std::fs::write(path, text).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))
}

// ---------- Reading ----------

/// Builds a model from triangles by object: corners merged where they are the same point.
struct Builder {
    positions: Vec<[f64; 3]>,
    at: HashMap<[u64; 3], u32>,
    tris: Vec<Tri>,
    objects: Vec<String>,
    materials: Vec<Material>,
    colours: HashMap<[u8; 3], u32>,
    parts: Vec<Vec<(Vec<[f64; 3]>, u32)>>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            positions: Vec::new(),
            at: HashMap::new(),
            tris: Vec::new(),
            objects: Vec::new(),
            materials: Vec::new(),
            colours: HashMap::new(),
            parts: Vec::new(),
        }
    }
    fn object(&mut self, name: String) -> u32 {
        self.objects.push(name);
        self.parts.push(Vec::new());
        self.objects.len() as u32 - 1
    }
    fn material(&mut self, rgb: [f32; 3], name: Option<String>) -> u32 {
        let key = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        if let Some(m) = self.colours.get(&key) {
            return *m;
        }
        let m = self.materials.len() as u32;
        self.materials.push(Material {
            name: name.unwrap_or_else(|| format!("#{:02x}{:02x}{:02x}", key[0], key[1], key[2])),
            diffuse: rgb,
            ..Default::default()
        });
        self.colours.insert(key, m);
        m
    }
    fn corner(&mut self, p: [f64; 3]) -> u32 {
        let key = p.map(|v| v.to_bits());
        if let Some(i) = self.at.get(&key) {
            return *i;
        }
        let i = self.positions.len() as u32;
        self.positions.push(p);
        self.at.insert(key, i);
        i
    }
    fn tri(&mut self, p: [[f64; 3]; 3], material: u32, object: u32) {
        let v = p.map(|q| self.corner(q));
        if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
            return;
        }
        self.tris.push(Tri { v, n: [NONE; 3], t: [NONE; 3], material, object });
        self.parts[object as usize].push((p.to_vec(), material));
    }
    /// The model on the map: placed by the options, in metres from the unit given.
    fn finish(self, opts: &ObjOpts, unit: f64, notes: &mut Notes) -> Result<Table> {
        if self.tris.is_empty() {
            return err("The file has no triangles that could be read.");
        }
        let mut b = self;
        if b.materials.is_empty() {
            b.materials.push(Material { name: "model".into(), diffuse: [0.75, 0.75, 0.75], ..Default::default() });
        }
        let at = placement(opts, unit);
        let positions: Vec<[f64; 3]> = b.positions.iter().map(|p| at.place(*p)).collect();
        let parts: Vec<Vec<(Vec<[f64; 3]>, u32)>> =
            b.parts.into_iter().map(|faces| faces.into_iter().map(|(pts, m)| (pts.into_iter().map(|p| at.place(p)).collect(), m)).collect()).collect();
        let mesh = Mesh { positions, normals: Vec::new(), uvs: Vec::new(), tris: b.tris, materials: b.materials, objects: b.objects };
        Ok(model_table(mesh, parts, "triangles", opts.crs, notes))
    }
}

/// The metres of a unit: given, or the millimetres of 3D printing.
fn unit_or_mm(opts: &ObjOpts, file_unit: Option<f64>, notes: &mut Notes) -> f64 {
    match (opts.unit, file_unit) {
        (Some(u), _) => u,
        (None, Some(u)) => u,
        (None, None) => {
            notes.info("the file does not say its unit, so it was taken as millimetres; set the unit if that is wrong");
            0.001
        }
    }
}

/// Reads an STL file, binary or text: an object for each solid.
pub fn read_stl(path: &Path, opts: &ObjOpts, notes: &mut Notes) -> Result<Table> {
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let mut b = Builder::new();
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_string();
    let m = b.material([0.75, 0.75, 0.75], Some("model".into()));
    let binary_size = (bytes.len() >= 84).then(|| 84 + u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize * 50);
    let text = bytes.starts_with(b"solid") && binary_size != Some(bytes.len()) && String::from_utf8_lossy(&bytes[..bytes.len().min(1000)]).contains("facet");
    if text {
        let s = String::from_utf8_lossy(&bytes);
        let mut o = None;
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for line in s.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.as_slice() {
                ["solid", rest @ ..] => o = Some(b.object(if rest.is_empty() { stem.clone() } else { rest.join(" ") })),
                ["vertex", x, y, z] => pts.push([x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0), z.parse().unwrap_or(0.0)]),
                ["endloop"] => {
                    let obj = match o {
                        Some(o) => o,
                        None => {
                            let n = b.object(stem.clone());
                            o = Some(n);
                            n
                        }
                    };
                    for k in 1..pts.len().saturating_sub(1) {
                        b.tri([pts[0], pts[k], pts[k + 1]], m, obj);
                    }
                    pts.clear();
                }
                _ => {}
            }
        }
    } else {
        if binary_size != Some(bytes.len()) && binary_size.is_none_or(|s| s > bytes.len()) {
            return err(format!("{} is not an STL file: it is shorter than it says.", path.display()));
        }
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        let o = b.object(stem);
        let f = |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as f64;
        // Colours in the two bytes after each triangle, as some programs write them (bit 15
        // set, five bits each of blue, green and red).
        for k in 0..n {
            let at = 84 + k * 50;
            let p = [0, 1, 2].map(|c| [f(at + 12 + c * 12), f(at + 16 + c * 12), f(at + 20 + c * 12)]);
            let attr = u16::from_le_bytes([bytes[at + 48], bytes[at + 49]]);
            let mat = if attr & 0x8000 != 0 {
                let c = |s: u16| ((attr >> s) & 31) as f32 / 31.0;
                b.material([c(10), c(5), c(0)], None)
            } else {
                m
            };
            b.tri(p, mat, o);
        }
    }
    let unit = unit_or_mm(opts, None, notes);
    b.finish(opts, unit, notes)
}

fn attr_of(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.local_name().as_ref() == name).and_then(|a| String::from_utf8(a.value.to_vec()).ok())
}

/// A colour #RRGGBB or #RRGGBBAA.
fn hex_colour(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() < 6 {
        return None;
    }
    let n = u32::from_str_radix(&h[..6], 16).ok()?;
    Some([((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0])
}

type Matrix = [f64; 12];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

fn matrix(s: Option<String>) -> Matrix {
    let v: Vec<f64> = s.unwrap_or_default().split_whitespace().filter_map(|w| w.parse().ok()).collect();
    if v.len() == 12 {
        let mut m = [0.0; 12];
        m.copy_from_slice(&v);
        m
    } else {
        IDENTITY
    }
}

/// A point turned by a matrix of 3MF: rows m00 m01 m02 / m10 … / translation.
fn apply(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    [p[0] * m[0] + p[1] * m[3] + p[2] * m[6] + m[9], p[0] * m[1] + p[1] * m[4] + p[2] * m[7] + m[10], p[0] * m[2] + p[1] * m[5] + p[2] * m[8] + m[11]]
}

fn then(a: &Matrix, b: &Matrix) -> Matrix {
    // First a, then b.
    let p = |x: [f64; 3]| apply(b, apply(a, x));
    let o = p([0.0; 3]);
    let ex = p([1.0, 0.0, 0.0]);
    let ey = p([0.0, 1.0, 0.0]);
    let ez = p([0.0, 0.0, 1.0]);
    [ex[0] - o[0], ex[1] - o[1], ex[2] - o[2], ey[0] - o[0], ey[1] - o[1], ey[2] - o[2], ez[0] - o[0], ez[1] - o[1], ez[2] - o[2], o[0], o[1], o[2]]
}

/// Reads a 3MF package: each item of its build an object, placed by its transforms, in its
/// unit, coloured by its base materials and colour groups.
pub fn read_3mf(path: &Path, opts: &ObjOpts, notes: &mut Notes) -> Result<Table> {
    let named = |e: String| GbError(format!("{} cannot be read as 3MF: {}", path.display(), e));
    let file = std::fs::File::open(path).map_err(|e| named(e.to_string()))?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| named(e.to_string()))?;
    let name = (0..z.len())
        .filter_map(|i| z.by_index(i).ok().map(|f| f.name().to_string()))
        .find(|n| n.to_lowercase().ends_with(".model") && n.to_lowercase().starts_with("3d/"))
        .ok_or_else(|| named("it has no 3D model".into()))?;
    let mut text = String::new();
    z.by_name(&name).map_err(|e| named(e.to_string()))?.read_to_string(&mut text).map_err(|e| named(e.to_string()))?;
    struct Obj {
        name: String,
        verts: Vec<[f64; 3]>,
        tris: Vec<Tri3mf>,
        prop: Option<(String, usize)>,
        components: Vec<(String, Matrix)>,
    }
    let mut objects: HashMap<String, Obj> = HashMap::new();
    let mut groups: Groups3mf = HashMap::new();
    let mut build: Vec<(String, Matrix)> = Vec::new();
    let mut unit = None;
    let mut current: Option<String> = None;
    let mut group: Option<String> = None;
    let mut reader = quick_xml::Reader::from_str(&text);
    loop {
        use quick_xml::events::Event;
        match reader.read_event().map_err(|e| named(e.to_string()))? {
            Event::Start(e) | Event::Empty(e) => {
                let a = |k: &[u8]| attr_of(&e, k);
                match e.local_name().as_ref() {
                    b"model" => {
                        unit = Some(match a(b"unit").as_deref() {
                            Some("micron") => 1e-6,
                            Some("centimeter") => 0.01,
                            Some("inch") => 0.0254,
                            Some("foot") => 0.3048,
                            Some("meter") => 1.0,
                            _ => 0.001,
                        })
                    }
                    b"basematerials" | b"colorgroup" => {
                        let id = a(b"id").unwrap_or_default();
                        groups.insert(id.clone(), Vec::new());
                        group = Some(id);
                    }
                    b"base" => {
                        if let Some(g) = group.as_ref().and_then(|g| groups.get_mut(g)) {
                            g.push((a(b"displaycolor").and_then(|c| hex_colour(&c)).unwrap_or([0.75; 3]), a(b"name")));
                        }
                    }
                    b"color" => {
                        if let Some(g) = group.as_ref().and_then(|g| groups.get_mut(g)) {
                            g.push((a(b"color").and_then(|c| hex_colour(&c)).unwrap_or([0.75; 3]), None));
                        }
                    }
                    b"object" => {
                        let id = a(b"id").unwrap_or_default();
                        let prop = a(b"pid").map(|p| (p, a(b"pindex").and_then(|i| i.parse().ok()).unwrap_or(0)));
                        objects.insert(
                            id.clone(),
                            Obj {
                                name: a(b"name").unwrap_or_else(|| format!("object {}", id)),
                                verts: Vec::new(),
                                tris: Vec::new(),
                                prop,
                                components: Vec::new(),
                            },
                        );
                        current = Some(id);
                    }
                    b"vertex" => {
                        if let Some(o) = current.as_ref().and_then(|c| objects.get_mut(c)) {
                            let f = |k: &[u8]| a(k).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            o.verts.push([f(b"x"), f(b"y"), f(b"z")]);
                        }
                    }
                    b"triangle" => {
                        if let Some(o) = current.as_ref().and_then(|c| objects.get_mut(c)) {
                            let i = |k: &[u8]| a(k).and_then(|v| v.parse().ok()).unwrap_or(0usize);
                            let prop = a(b"pid")
                                .or_else(|| o.prop.as_ref().map(|p| p.0.clone()))
                                .map(|p| (p, a(b"p1").and_then(|v| v.parse().ok()).or(o.prop.as_ref().map(|p| p.1)).unwrap_or(0)));
                            o.tris.push(([i(b"v1"), i(b"v2"), i(b"v3")], prop));
                        }
                    }
                    b"component" => {
                        if let Some(o) = current.as_ref().and_then(|c| objects.get_mut(c)) {
                            o.components.push((a(b"objectid").unwrap_or_default(), matrix(a(b"transform"))));
                        }
                    }
                    b"item" => build.push((a(b"objectid").unwrap_or_default(), matrix(a(b"transform")))),
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"object" => current = None,
                b"basematerials" | b"colorgroup" => group = None,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    let mut b = Builder::new();
    fn add(b: &mut Builder, objects: &HashMap<String, Obj>, groups: &Groups3mf, id: &str, m: &Matrix, object: u32, depth: usize) {
        let Some(o) = objects.get(id) else { return };
        if depth > 16 {
            return;
        }
        for (v, prop) in &o.tris {
            if v.iter().any(|i| *i >= o.verts.len()) {
                continue;
            }
            let colour = prop.as_ref().and_then(|(g, i)| groups.get(g).and_then(|g| g.get(*i))).cloned();
            let mat = match colour {
                Some((c, name)) => b.material(c, name),
                None => b.material([0.75; 3], Some("model".into())),
            };
            b.tri(v.map(|i| apply(m, o.verts[i])), mat, object);
        }
        for (child, cm) in &o.components {
            add(b, objects, groups, child, &then(cm, m), object, depth + 1);
        }
    }
    for (id, m) in &build {
        let name = objects.get(id).map(|o| o.name.clone()).unwrap_or_default();
        let object = b.object(name);
        add(&mut b, &objects, &groups, id, m, object, 0);
    }
    let unit = unit_or_mm(opts, unit, notes);
    b.finish(opts, unit, notes)
}

// ---------- STEP ----------

/// A value of a STEP file.
#[derive(Debug, Clone)]
enum P {
    Ref(u64),
    Num(f64),
    Str(String),
    Enum(String),
    List(Vec<P>),
    /// A typed value, such as LENGTH_MEASURE(1.E-04), which is not needed here.
    Typed,
    Null,
}

/// The entities of a STEP file by number: the parts of each, name and values (several
/// for a complex entity).
fn parse_step(text: &str) -> HashMap<u64, Vec<(String, Vec<P>)>> {
    let data = match text.find("DATA;") {
        Some(at) => &text[at + 5..],
        None => text,
    };
    let b = data.as_bytes();
    let mut out = HashMap::new();
    let mut i = 0usize;
    fn skip(b: &[u8], i: &mut usize) {
        loop {
            while *i < b.len() && (b[*i] as char).is_whitespace() {
                *i += 1;
            }
            if *i + 1 < b.len() && b[*i] == b'/' && b[*i + 1] == b'*' {
                *i += 2;
                while *i + 1 < b.len() && !(b[*i] == b'*' && b[*i + 1] == b'/') {
                    *i += 1;
                }
                *i += 2;
            } else {
                break;
            }
        }
    }
    fn word(b: &[u8], i: &mut usize) -> String {
        let s = *i;
        while *i < b.len() && (b[*i].is_ascii_alphanumeric() || b[*i] == b'_' || b[*i] == b'-') {
            *i += 1;
        }
        String::from_utf8_lossy(&b[s..*i]).to_uppercase()
    }
    fn value(b: &[u8], i: &mut usize) -> P {
        skip(b, i);
        if *i >= b.len() {
            return P::Null;
        }
        match b[*i] {
            b'#' => {
                *i += 1;
                let s = *i;
                while *i < b.len() && b[*i].is_ascii_digit() {
                    *i += 1;
                }
                P::Ref(std::str::from_utf8(&b[s..*i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0))
            }
            b'\'' => {
                *i += 1;
                let mut s = Vec::new();
                while *i < b.len() {
                    if b[*i] == b'\'' {
                        if *i + 1 < b.len() && b[*i + 1] == b'\'' {
                            s.push(b'\'');
                            *i += 2;
                            continue;
                        }
                        *i += 1;
                        break;
                    }
                    s.push(b[*i]);
                    *i += 1;
                }
                P::Str(String::from_utf8_lossy(&s).into_owned())
            }
            b'.' => {
                *i += 1;
                let w = word(b, i);
                if *i < b.len() && b[*i] == b'.' {
                    *i += 1;
                }
                P::Enum(w)
            }
            b'(' => P::List(list(b, i)),
            b'$' | b'*' => {
                *i += 1;
                P::Null
            }
            c if c == b'-' || c == b'+' || c.is_ascii_digit() => {
                let s = *i;
                *i += 1;
                while *i < b.len() && (b[*i].is_ascii_digit() || b"+-.eE".contains(&b[*i])) {
                    *i += 1;
                }
                P::Num(std::str::from_utf8(&b[s..*i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0))
            }
            c if c.is_ascii_alphabetic() => {
                let w = word(b, i);
                skip(b, i);
                if *i < b.len() && b[*i] == b'(' {
                    list(b, i);
                    P::Typed
                } else {
                    P::Enum(w)
                }
            }
            _ => {
                *i += 1;
                P::Null
            }
        }
    }
    fn list(b: &[u8], i: &mut usize) -> Vec<P> {
        // At '('.
        *i += 1;
        let mut out = Vec::new();
        loop {
            skip(b, i);
            if *i >= b.len() {
                break;
            }
            if b[*i] == b')' {
                *i += 1;
                break;
            }
            if b[*i] == b',' {
                *i += 1;
                continue;
            }
            let before = *i;
            out.push(value(b, i));
            if *i == before {
                *i += 1;
            }
        }
        out
    }
    loop {
        skip(b, &mut i);
        if i >= b.len() || b[i..].starts_with(b"ENDSEC") {
            break;
        }
        if b[i] != b'#' {
            // Up to the end of what cannot be read.
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
            i += 1;
            continue;
        }
        i += 1;
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let id: u64 = std::str::from_utf8(&b[s..i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0);
        skip(b, &mut i);
        if i < b.len() && b[i] == b'=' {
            i += 1;
        }
        skip(b, &mut i);
        let mut parts = Vec::new();
        if i < b.len() && b[i] == b'(' {
            // A complex entity: several names, each with its values.
            i += 1;
            loop {
                skip(b, &mut i);
                if i >= b.len() || b[i] == b')' {
                    i += 1;
                    break;
                }
                let w = word(b, &mut i);
                skip(b, &mut i);
                let vals = if i < b.len() && b[i] == b'(' { list(b, &mut i) } else { Vec::new() };
                if w.is_empty() {
                    i += 1;
                    continue;
                }
                parts.push((w, vals));
            }
        } else {
            let w = word(b, &mut i);
            skip(b, &mut i);
            let vals = if i < b.len() && b[i] == b'(' { list(b, &mut i) } else { Vec::new() };
            parts.push((w, vals));
        }
        while i < b.len() && b[i] != b';' {
            i += 1;
        }
        i += 1;
        out.insert(id, parts);
    }
    out
}

/// Reads a STEP file: solids and surface models whose faces are triangles, polygons or flat
/// faces with straight edges, and tessellated shapes of AP242, each an object, coloured as
/// the file styles them. Curved faces are not read, and the log says how many.
pub fn read_step(path: &Path, opts: &ObjOpts, notes: &mut Notes) -> Result<Table> {
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let text = String::from_utf8_lossy(&bytes);
    if !text.trim_start().starts_with("ISO-10303-21") {
        return err(format!("{} is not a STEP file.", path.display()));
    }
    let ents = parse_step(&text);
    let first = |id: u64| ents.get(&id).and_then(|p| p.first());
    let point = |id: u64| -> Option<[f64; 3]> {
        let (n, v) = first(id)?;
        if n != "CARTESIAN_POINT" {
            return None;
        }
        match v.get(1)? {
            P::List(c) => {
                let num = |k: usize| match c.get(k) {
                    Some(P::Num(x)) => *x,
                    _ => 0.0,
                };
                Some([num(0), num(1), num(2)])
            }
            _ => None,
        }
    };
    let refs = |v: Option<&P>| -> Vec<u64> {
        match v {
            Some(P::List(l)) => l.iter().filter_map(|p| if let P::Ref(r) = p { Some(*r) } else { None }).collect(),
            _ => Vec::new(),
        }
    };
    let rf = |v: Option<&P>| -> Option<u64> {
        match v {
            Some(P::Ref(r)) => Some(*r),
            _ => None,
        }
    };
    // The unit of length: millimetres unless a prefix says otherwise.
    let mut unit = None;
    for parts in ents.values() {
        if parts.iter().any(|(n, _)| n == "LENGTH_UNIT") {
            if let Some((_, v)) = parts.iter().find(|(n, _)| n == "SI_UNIT") {
                unit = Some(match v.first() {
                    Some(P::Enum(p)) if p == "MILLI" => 0.001,
                    Some(P::Enum(p)) if p == "CENTI" => 0.01,
                    Some(P::Enum(p)) if p == "KILO" => 1000.0,
                    Some(P::Enum(p)) if p == "MICRO" => 1e-6,
                    _ => 1.0,
                });
            } else if parts.iter().any(|(n, _)| n == "CONVERSION_BASED_UNIT") {
                unit = Some(0.0254);
            }
        }
    }
    // The colours given to items by their styles: the first colour found under each.
    let mut colour_of: HashMap<u64, [f32; 3]> = HashMap::new();
    for parts in ents.values() {
        let Some((n, v)) = parts.first() else { continue };
        if n != "STYLED_ITEM" && n != "OVER_RIDING_STYLED_ITEM" {
            continue;
        }
        let Some(item) = rf(v.get(2)) else { continue };
        let mut stack = refs(v.get(1));
        let mut seen = 0;
        while let Some(id) = stack.pop() {
            seen += 1;
            if seen > 64 {
                break;
            }
            let Some((n, v)) = first(id) else { continue };
            if n == "COLOUR_RGB" {
                let c = |k: usize| match v.get(k) {
                    Some(P::Num(x)) => *x as f32,
                    _ => 0.75,
                };
                colour_of.insert(item, [c(1), c(2), c(3)]);
                break;
            }
            for p in v {
                match p {
                    P::Ref(r) => stack.push(*r),
                    P::List(l) => stack.extend(l.iter().filter_map(|x| if let P::Ref(r) = x { Some(*r) } else { None })),
                    _ => {}
                }
            }
        }
    }
    let mut b = Builder::new();
    let grey = b.material([0.75; 3], Some("model".into()));
    let (mut curved, mut faces_read) = (0usize, 0usize);
    // The corners of a loop: a polygon, or edges of straight lines between points.
    let loop_points = |id: u64| -> Option<Vec<[f64; 3]>> {
        let (n, v) = first(id)?;
        match n.as_str() {
            "POLY_LOOP" => Some(refs(v.get(1)).into_iter().filter_map(point).collect()),
            "EDGE_LOOP" => {
                let mut pts = Vec::new();
                for oe in refs(v.get(1)) {
                    let (n, v) = first(oe)?;
                    if n != "ORIENTED_EDGE" {
                        return None;
                    }
                    let forward = !matches!(v.get(4), Some(P::Enum(e)) if e == "F");
                    let (en, ev) = first(rf(v.get(3))?)?;
                    if en != "EDGE_CURVE" {
                        return None;
                    }
                    // Straight edges only.
                    let (cn, _) = first(rf(ev.get(3))?)?;
                    if cn != "LINE" && cn != "POLYLINE" {
                        return None;
                    }
                    let vertex = |r: Option<u64>| -> Option<[f64; 3]> {
                        let (vn, vv) = first(r?)?;
                        if vn != "VERTEX_POINT" {
                            return None;
                        }
                        point(rf(vv.get(1))?)
                    };
                    let (a, z) = (vertex(rf(ev.get(1)))?, vertex(rf(ev.get(2)))?);
                    pts.push(if forward { a } else { z });
                }
                Some(pts)
            }
            _ => None,
        }
    };
    let face_tris = |id: u64, curved: &mut usize| -> Vec<[[f64; 3]; 3]> {
        let Some((n, v)) = first(id) else { return Vec::new() };
        if !matches!(n.as_str(), "FACE" | "ADVANCED_FACE" | "FACE_SURFACE") {
            return Vec::new();
        }
        if n != "FACE" {
            if let Some((sn, _)) = rf(v.get(2)).and_then(first) {
                if sn != "PLANE" {
                    *curved += 1;
                    return Vec::new();
                }
            }
        }
        let (mut outer, mut holes) = (None, Vec::new());
        for bound in refs(v.get(1)) {
            let Some((bn, bv)) = first(bound) else { continue };
            let Some(pts) = rf(bv.get(1)).and_then(loop_points) else {
                *curved += 1;
                return Vec::new();
            };
            if bn == "FACE_OUTER_BOUND" && outer.is_none() {
                outer = Some(pts);
            } else {
                holes.push(pts);
            }
        }
        let outer = match outer {
            Some(o) => o,
            None if !holes.is_empty() => holes.remove(0),
            None => return Vec::new(),
        };
        if outer.len() == 3 && holes.is_empty() {
            return vec![[outer[0], outer[1], outer[2]]];
        }
        crate::ifc_io::triangulate_face(&outer, &holes)
    };
    // The shapes: each solid, shell model or tessellated set an object.
    let mut ids: Vec<u64> = ents.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let Some((n, v)) = first(id) else { continue };
        let name = match v.first() {
            Some(P::Str(s)) if !s.is_empty() => s.clone(),
            _ => format!("{} {}", n.to_lowercase().replace('_', " "), id),
        };
        let shells: Vec<u64> = match n.as_str() {
            "MANIFOLD_SOLID_BREP" | "FACETED_BREP" | "BREP_WITH_VOIDS" => rf(v.get(1)).into_iter().collect(),
            "SHELL_BASED_SURFACE_MODEL" => refs(v.get(1)),
            "TRIANGULATED_FACE_SET" | "COMPLEX_TRIANGULATED_FACE_SET" => {
                // AP242: points in a coordinates list, triangles as numbers from 1.
                let object = b.object(name);
                let mat = colour_of.get(&id).map(|c| b.material(*c, None)).unwrap_or(grey);
                let coords: Vec<[f64; 3]> = match rf(v.get(1)).and_then(first) {
                    Some((_, cv)) => match cv.get(2) {
                        Some(P::List(l)) => l
                            .iter()
                            .map(|p| match p {
                                P::List(c) => [0, 1, 2].map(|k| if let Some(P::Num(x)) = c.get(k) { *x } else { 0.0 }),
                                _ => [0.0; 3],
                            })
                            .collect(),
                        _ => Vec::new(),
                    },
                    None => Vec::new(),
                };
                let index: Vec<usize> = match v.get(5) {
                    Some(P::List(l)) if !l.is_empty() => l.iter().map(|p| if let P::Num(x) = p { *x as usize } else { 0 }).collect(),
                    _ => (1..=coords.len()).collect(),
                };
                let at = |k: usize| index.get(k.wrapping_sub(1)).and_then(|i| coords.get(i.wrapping_sub(1))).copied();
                if let Some(P::List(tl)) = v.get(6) {
                    for t in tl {
                        if let P::List(c) = t {
                            let k: Vec<usize> = c.iter().map(|p| if let P::Num(x) = p { *x as usize } else { 0 }).collect();
                            if let (Some(a), Some(bb), Some(cc)) =
                                (k.first().and_then(|k| at(*k)), k.get(1).and_then(|k| at(*k)), k.get(2).and_then(|k| at(*k)))
                            {
                                b.tri([a, bb, cc], mat, object);
                                faces_read += 1;
                            }
                        }
                    }
                }
                continue;
            }
            _ => continue,
        };
        let object = b.object(name);
        let mat = colour_of.get(&id).map(|c| b.material(*c, None)).unwrap_or(grey);
        for shell in shells {
            let Some((_, sv)) = first(shell) else { continue };
            for face in refs(sv.get(1)) {
                let fmat = colour_of.get(&face).map(|c| b.material(*c, None)).unwrap_or(mat);
                let tris = face_tris(face, &mut curved);
                if !tris.is_empty() {
                    faces_read += 1;
                }
                for t in tris {
                    b.tri(t, fmat, object);
                }
            }
        }
    }
    if curved > 0 {
        notes.warn(format!(
            "{} curved faces were left out: STEP is read where its faces are flat with straight edges, or triangles. Save the model with its faces as triangles (tessellated) to read it all.",
            fmt_count(curved as u64)
        ));
    }
    if b.tris.is_empty() {
        return err(format!("{} has no faces that could be read: STEP is read where its faces are flat with straight edges, or triangles.", path.display()));
    }
    notes.info(format!("{} faces read", fmt_count(faces_read as u64)));
    let unit = unit_or_mm(opts, unit, notes);
    // Objects without triangles are kept out.
    b.finish(opts, unit, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Mesh {
        // A cube of 10 m from 100, 200, 50, its sides two triangles each, red.
        let mut positions = Vec::new();
        for z in [0.0, 10.0] {
            for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
                positions.push([100.0 + x, 200.0 + y, 50.0 + z]);
            }
        }
        let quads = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
        let mut tris = Vec::new();
        for q in quads {
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                tris.push(Tri { v: t, n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 });
            }
        }
        Mesh {
            positions,
            normals: Vec::new(),
            uvs: Vec::new(),
            tris,
            materials: vec![Material { name: "brick".into(), diffuse: [0.8, 0.2, 0.1], ..Default::default() }],
            objects: vec!["House".into()],
        }
    }

    #[test]
    fn a_model_goes_through_stl_3mf_and_step_at_its_scale() {
        let dir = std::env::temp_dir().join(format!("og-print-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = cube();
        // At 1:100, 10 m is 100 mm.
        let f = frame_of(&m, &PrintOpts { scale: Some(100.0), fit_mm: 180.0 }).unwrap();
        let back = |file: &str, read: &dyn Fn(&Path, &ObjOpts, &mut Notes) -> Result<Table>| {
            let mut notes = Notes::default();
            let t = read(&dir.join(file), &ObjOpts::default(), &mut notes).unwrap();
            let mesh = t.mesh.clone().unwrap();
            let (lo, hi) = mesh.bounds().unwrap();
            (t, mesh.tris.len(), lo, hi, notes)
        };
        write_stl(&m, &dir.join("a.stl"), &f).unwrap();
        let (_, n, lo, hi, notes) = back("a.stl", &read_stl);
        // Read in millimetres, taken as such: 100 mm is 0.1 m.
        assert_eq!(n, 12);
        assert!(lo.iter().all(|v| v.abs() < 1e-6) && (hi[0] - 0.1).abs() < 1e-6, "{:?} {:?}", lo, hi);
        assert!(notes.info.iter().any(|i| i.contains("millimetres")));
        write_3mf(&m, &dir.join("a.3mf"), &f).unwrap();
        let (t, n, _, hi, _) = back("a.3mf", &read_3mf);
        assert_eq!((n, t.features[0].attrs[0].to_text()), (12, "House".to_string()));
        assert!((hi[2] - 0.1).abs() < 1e-6);
        assert_eq!(t.mesh.as_ref().unwrap().materials[0].name, "brick");
        write_step(&m, &dir.join("a.step"), &f).unwrap();
        let (t, n, _, hi, _) = back("a.step", &read_step);
        assert_eq!((n, t.features[0].attrs[0].to_text()), (12, "House".to_string()));
        assert!((hi[1] - 0.1).abs() < 1e-6, "{:?}", hi);
        let mm = t.mesh.as_ref().unwrap();
        let c = mm.materials[mm.tris[0].material as usize].diffuse;
        assert!((c[0] - 0.8).abs() < 0.01 && (c[2] - 0.1).abs() < 0.01, "{:?}", c);
        // Fitted: the largest side 180 mm.
        let fit = frame_of(&m, &PrintOpts { scale: None, fit_mm: 180.0 }).unwrap();
        assert!((fit.mm_per_m * 10.0 - 180.0).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_faces_of_flat_polygons_and_straight_edges_are_read() {
        // A square face by edges and a triangle by a polygon loop, in a shell, in metres.
        let text = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n\
            #1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(1.,0.,0.));#3=CARTESIAN_POINT('',(1.,1.,0.));#4=CARTESIAN_POINT('',(0.,1.,0.));\n\
            #5=VERTEX_POINT('',#1);#6=VERTEX_POINT('',#2);#7=VERTEX_POINT('',#3);#8=VERTEX_POINT('',#4);\n\
            #9=LINE('',#1,#99);#10=EDGE_CURVE('',#5,#6,#9,.T.);#11=EDGE_CURVE('',#6,#7,#9,.T.);#12=EDGE_CURVE('',#7,#8,#9,.T.);#13=EDGE_CURVE('',#8,#5,#9,.T.);\n\
            #14=ORIENTED_EDGE('',*,*,#10,.T.);#15=ORIENTED_EDGE('',*,*,#11,.T.);#16=ORIENTED_EDGE('',*,*,#12,.T.);#17=ORIENTED_EDGE('',*,*,#13,.T.);\n\
            #18=EDGE_LOOP('',(#14,#15,#16,#17));#19=FACE_OUTER_BOUND('',#18,.T.);#20=PLANE('',#98);#21=ADVANCED_FACE('',(#19),#20,.T.);\n\
            #22=POLY_LOOP('',(#1,#2,#3));#23=FACE_OUTER_BOUND('',#22,.T.);#24=FACE('',(#23));\n\
            #25=CYLINDRICAL_SURFACE('',#98,1.);#26=ADVANCED_FACE('',(#19),#25,.T.);\n\
            #27=OPEN_SHELL('',(#21,#24,#26));#28=SHELL_BASED_SURFACE_MODEL('Roof',(#27));\n\
            #30=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\nENDSEC;\nEND-ISO-10303-21;\n";
        let dir = std::env::temp_dir().join(format!("og-step-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("roof.stp"), text).unwrap();
        let mut notes = Notes::default();
        let t = read_step(&dir.join("roof.stp"), &ObjOpts::default(), &mut notes).unwrap();
        assert_eq!(t.mesh.as_ref().unwrap().tris.len(), 3);
        assert_eq!(t.features[0].attrs[0].to_text(), "Roof");
        assert!(notes.warnings.iter().any(|w| w.contains("1 curved faces")), "{:?}", notes);
        let (_, hi) = t.mesh.as_ref().unwrap().bounds().unwrap();
        assert!((hi[0] - 1.0).abs() < 1e-9, "metres as the file says");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
