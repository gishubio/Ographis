//! Writes the 3D model of a stream to OBJ, with its .mtl, or to binary glTF (.glb).
//!
//! The coordinates of a map are too large for the floats of a 3D file, so the model is
//! written around an origin near its middle: whole metres east and north, and the bottom of
//! the model. The origin and the coordinate system are kept in the file (a comment in OBJ,
//! the extras of the asset in glTF), and Ographis puts the model back in its place when it
//! reads the file again.

use crate::mesh::{Mesh, NONE};
use crate::model::{err, GbError, Result, Table, Value};
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The origin the model is written around.
pub fn origin_of(mesh: &Mesh) -> [f64; 3] {
    match mesh.bounds() {
        Some((lo, hi)) => [((lo[0] + hi[0]) / 2.0).round(), ((lo[1] + hi[1]) / 2.0).round(), lo[2].floor()],
        None => [0.0; 3],
    }
}

/// The line of an OBJ file that gives where its origin lies.
pub const OBJ_ORIGIN: &str = "# Ographis origin";

/// The normal of a triangle, from its corners.
fn face_normal(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f32; 3] {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-30);
    [(n[0] / l) as f32, (n[1] / l) as f32, (n[2] / l) as f32]
}

fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
    if t.is_empty() {
        "_".into()
    } else {
        t
    }
}

fn write_png(t: &crate::mesh::Texture) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let img = image::RgbaImage::from_raw(t.width as u32, t.height as u32, t.rgba.clone()).ok_or_else(|| GbError("A texture has the wrong size.".into()))?;
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| GbError(format!("A texture cannot be written: {}", e)))?;
    Ok(out)
}

/// Writes the model to an OBJ file with z up, its materials to a .mtl file beside it and its
/// textures as PNG files. Gives the files written.
pub fn write_obj(table: &Table, path: &Path) -> Result<Vec<PathBuf>> {
    let Some(mesh) = table.mesh.as_deref() else { return err("The stream has no 3D model to write.") };
    let o = origin_of(mesh);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "model".into());
    let dir = path.parent().unwrap_or(Path::new("."));
    let mtl_path = dir.join(format!("{}.mtl", stem));
    let mut files = vec![path.to_path_buf(), mtl_path.clone()];
    let mut obj = String::new();
    obj.push_str("# Written by Ographis. x east, y north, z up, in metres from the origin below.\n");
    obj.push_str(&format!("{} {} {} {}{}\n", OBJ_ORIGIN, o[0], o[1], o[2], table.crs.map(|c| format!(" EPSG:{}", c)).unwrap_or_default()));
    obj.push_str(&format!("mtllib {}.mtl\n", stem));
    for p in &mesh.positions {
        obj.push_str(&format!("v {:.4} {:.4} {:.4}\n", p[0] - o[0], p[1] - o[1], p[2] - o[2]));
    }
    for n in &mesh.normals {
        obj.push_str(&format!("vn {:.5} {:.5} {:.5}\n", n[0], n[1], n[2]));
    }
    for t in &mesh.uvs {
        obj.push_str(&format!("vt {:.6} {:.6}\n", t[0], t[1]));
    }
    // The triangles by object, and within it by material.
    let mut order: Vec<usize> = (0..mesh.tris.len()).collect();
    order.sort_by_key(|i| (mesh.tris[*i].object, mesh.tris[*i].material));
    let (mut object, mut material) = (u32::MAX, u32::MAX);
    for i in order {
        let t = &mesh.tris[i];
        if t.object != object {
            object = t.object;
            material = u32::MAX;
            obj.push_str(&format!("o {}\n", safe_name(mesh.objects.get(object as usize).map_or("", |s| s.as_str()))));
        }
        if t.material != material {
            material = t.material;
            obj.push_str(&format!("usemtl {}\n", safe_name(&mtl_name(mesh, material))));
        }
        let has_n = t.n.iter().all(|k| *k != NONE);
        let has_t = t.t.iter().all(|k| *k != NONE);
        obj.push('f');
        for k in 0..3 {
            let v = t.v[k] + 1;
            match (has_t, has_n) {
                (true, true) => obj.push_str(&format!(" {}/{}/{}", v, t.t[k] + 1, t.n[k] + 1)),
                (true, false) => obj.push_str(&format!(" {}/{}", v, t.t[k] + 1)),
                (false, true) => obj.push_str(&format!(" {}//{}", v, t.n[k] + 1)),
                (false, false) => obj.push_str(&format!(" {}", v)),
            }
        }
        obj.push('\n');
    }
    let mut mtl = String::from("# Written by Ographis\n");
    for (k, m) in mesh.materials.iter().enumerate() {
        let name = safe_name(&mtl_name(mesh, k as u32));
        mtl.push_str(&format!("newmtl {}\n", name));
        mtl.push_str(&format!("Kd {:.4} {:.4} {:.4}\n", m.diffuse[0], m.diffuse[1], m.diffuse[2]));
        mtl.push_str(&format!("Ks {:.4} {:.4} {:.4}\nNs {:.1}\n", m.specular[0], m.specular[1], m.specular[2], m.shininess));
        if m.opacity < 1.0 {
            mtl.push_str(&format!("d {:.3}\n", m.opacity));
        }
        if m.emission != [0.0; 3] {
            mtl.push_str(&format!("Ke {:.4} {:.4} {:.4}\n", m.emission[0], m.emission[1], m.emission[2]));
        }
        if let Some(t) = &m.texture {
            let file = format!("{}_{}.png", stem, name);
            std::fs::write(dir.join(&file), write_png(t)?).map_err(|e| GbError(format!("Cannot write {}: {}", file, e)))?;
            files.push(dir.join(&file));
            mtl.push_str(&format!("map_Kd {}\n", file));
        }
        mtl.push('\n');
    }
    std::fs::write(path, obj).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    std::fs::write(&mtl_path, mtl).map_err(|e| GbError(format!("Cannot write {}: {}", mtl_path.display(), e)))?;
    Ok(files)
}

/// The name of a material, made unique by its number when another has the same.
fn mtl_name(mesh: &Mesh, k: u32) -> String {
    let m = &mesh.materials[k as usize];
    let base = if m.name.is_empty() { "material".to_string() } else { m.name.clone() };
    if mesh.materials.iter().enumerate().any(|(j, n)| j as u32 != k && n.name == m.name) {
        format!("{}_{}", base, k)
    } else {
        base
    }
}

fn srgb_to_linear(c: f32) -> f64 {
    let c = c.clamp(0.0, 1.0) as f64;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn attrs_json(table: &Table, feature: usize) -> Option<Json> {
    let f = table.features.get(feature)?;
    let mut map = serde_json::Map::new();
    for (field, v) in table.fields.iter().zip(&f.attrs) {
        let j = match v {
            Value::Null => continue,
            Value::Bool(b) => json!(b),
            Value::Int(i) => json!(i),
            Value::Real(r) => json!(r),
            Value::Text(s) => json!(s),
        };
        map.insert(field.name.clone(), j);
    }
    Some(Json::Object(map))
}

/// The most objects that get a node of their own in glTF. A model with more, such as a TIN
/// with a triangle for each, is one node.
const MOST_NODES: usize = 20_000;

/// How a glTF file is made for Blender and other 3D programs.
#[derive(Debug, Clone, Default)]
pub struct GltfOpts {
    /// Where the model lies, as the scene properties BlenderGIS reads: crs, crs x and crs y.
    pub georef: bool,
    /// The name of the node of each feature, in place of the name of its object.
    pub names: Option<Vec<String>>,
    /// The group of each feature: the nodes of a group are children of a node named after
    /// it, which Blender shows as an empty with the objects under it.
    pub groups: Option<Vec<String>>,
    /// The kind of each feature, for the names of the materials: a material for each kind,
    /// such as roof tile and roof sheet metal, to change in Blender all at once.
    pub kinds: Option<Vec<String>>,
    /// The sun, as its azimuth clockwise from north and its altitude, in degrees: a light.
    pub sun: Option<(f64, f64)>,
    /// A camera looking at the model from the south-west, from above.
    pub camera: bool,
    /// The origin to write around, the same for files that are to fit together in a 3D
    /// program. None: near the middle of the model.
    pub origin: Option<[f64; 3]>,
}

/// A 4 × 4 matrix of glTF, by columns, for a node at `at` whose −Z looks along `dir` with
/// its Y as near up as can be.
fn look_matrix(at: [f64; 3], dir: [f64; 3]) -> Vec<f64> {
    let norm = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let f = norm(dir);
    let up = if f[1].abs() > 0.999 { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
    let r = norm(cross(f, up));
    let u = cross(r, f);
    vec![r[0], r[1], r[2], 0.0, u[0], u[1], u[2], 0.0, -f[0], -f[1], -f[2], 0.0, at[0], at[1], at[2], 1.0]
}

/// Writes the model to a binary glTF file, with y up as glTF has it. Each object is a node
/// with its name and the attributes of its feature as extras.
pub fn write_glb(table: &Table, path: &Path) -> Result<Vec<PathBuf>> {
    write_glb_with(table, path, &GltfOpts { georef: true, ..Default::default() })
}

/// The lines and points of a stream without a 3D model, as a model of lines and points:
/// each feature an object, at its heights or at 0.
/// A model of lines and points: (object, corner, corner) of each piece of line and
/// (object, corner) of each point.
type Loose = (Mesh, Vec<(u32, u32, u32)>, Vec<(u32, u32)>);

fn lines_and_points(table: &Table) -> Option<Loose> {
    let mut mesh =
        Mesh { materials: vec![crate::mesh::Material { name: "lines".into(), diffuse: [0.15, 0.15, 0.15], ..Default::default() }], ..Default::default() };
    // (object, first corner, second corner) of each piece of line, and (object, corner) of points.
    let (mut segs, mut pts) = (Vec::new(), Vec::new());
    for (k, f) in table.features.iter().enumerate() {
        mesh.objects.push(format!("feature {}", k + 1));
        let Some(g) = &f.geom else { continue };
        let z = |i: usize| f.z.as_ref().and_then(|z| z.get(i).copied()).filter(|v| v.is_finite()).unwrap_or(0.0);
        let mut i = 0usize;
        let line = |l: &geo::LineString<f64>, i: &mut usize, mesh: &mut Mesh, segs: &mut Vec<(u32, u32, u32)>| {
            let first = mesh.positions.len() as u32;
            for c in &l.0 {
                mesh.positions.push([c.x, c.y, z(*i)]);
                *i += 1;
            }
            for k2 in 1..l.0.len() as u32 {
                segs.push((k as u32, first + k2 - 1, first + k2));
            }
        };
        match g {
            geo::Geometry::Point(p) => {
                pts.push((k as u32, mesh.positions.len() as u32));
                mesh.positions.push([p.x(), p.y(), z(0)]);
            }
            geo::Geometry::MultiPoint(m) => {
                for (j, p) in m.0.iter().enumerate() {
                    pts.push((k as u32, mesh.positions.len() as u32));
                    mesh.positions.push([p.x(), p.y(), z(j)]);
                }
            }
            geo::Geometry::LineString(l) => line(l, &mut i, &mut mesh, &mut segs),
            geo::Geometry::MultiLineString(m) => m.0.iter().for_each(|l| line(l, &mut i, &mut mesh, &mut segs)),
            geo::Geometry::Polygon(p) => std::iter::once(p.exterior()).chain(p.interiors()).for_each(|l| line(l, &mut i, &mut mesh, &mut segs)),
            geo::Geometry::MultiPolygon(m) => {
                m.0.iter().flat_map(|p| std::iter::once(p.exterior()).chain(p.interiors())).for_each(|l| line(l, &mut i, &mut mesh, &mut segs))
            }
            _ => {}
        }
    }
    (!segs.is_empty() || !pts.is_empty()).then_some((mesh, segs, pts))
}

/// Writes a binary glTF file as `o` says: the 3D model of the stream, or its lines and
/// points when it has none.
pub fn write_glb_with(table: &Table, path: &Path, opts: &GltfOpts) -> Result<Vec<PathBuf>> {
    let drawn;
    let (mesh, segs, points) = match table.mesh.as_deref() {
        Some(m) => (m, Vec::new(), Vec::new()),
        None => match lines_and_points(table) {
            Some((m, s, p)) => {
                drawn = m;
                (&drawn, s, p)
            }
            None => return err("The stream has no 3D model, lines or points to write."),
        },
    };
    // The materials, one for each kind and material when the features have kinds.
    let mut mats: Vec<crate::mesh::Material> = Vec::new();
    let mut mat_of: std::collections::HashMap<(String, u32), u32> = std::collections::HashMap::new();
    let mut material_for = |object: u32, material: u32| -> u32 {
        let kind = opts.kinds.as_ref().and_then(|k| k.get(object as usize)).cloned().unwrap_or_default();
        *mat_of.entry((kind.clone(), material)).or_insert_with(|| {
            let mut m = mesh.materials.get(material as usize).cloned().unwrap_or_default();
            if !kind.is_empty() {
                m.name = if mesh.materials.len() > 1 && !m.name.is_empty() { format!("{} {}", kind, m.name) } else { kind.clone() };
            }
            mats.push(m);
            (mats.len() - 1) as u32
        })
    };
    let tris: Vec<crate::mesh::Tri> = mesh.tris.iter().map(|t| crate::mesh::Tri { material: material_for(t.object, t.material), ..*t }).collect();
    let line_mats: Vec<u32> = segs.iter().map(|s| material_for(s.0, 0)).collect();
    let point_mats: Vec<u32> = points.iter().map(|p| material_for(p.0, 0)).collect();
    let mesh = &Mesh { tris, materials: mats, ..mesh.clone() };
    let o = opts.origin.unwrap_or_else(|| origin_of(mesh));
    let apart = mesh.objects.len() <= MOST_NODES && !mesh.objects.is_empty();
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut meshes, mut nodes) = (Vec::<Json>::new(), Vec::<Json>::new(), Vec::<Json>::new(), Vec::<Json>::new());
    let align = |bin: &mut Vec<u8>| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
    };
    // Materials, and the textures as PNG pictures in the buffer.
    let (mut images, mut textures, mut materials) = (Vec::<Json>::new(), Vec::<Json>::new(), Vec::<Json>::new());
    for m in &mesh.materials {
        let mut pbr = json!({
            "baseColorFactor": [srgb_to_linear(m.diffuse[0]), srgb_to_linear(m.diffuse[1]), srgb_to_linear(m.diffuse[2]), m.opacity.clamp(0.0, 1.0) as f64],
            "metallicFactor": 0.0,
            "roughnessFactor": (1.0 - (m.specular.iter().sum::<f32>() / 3.0) as f64 * 0.8).clamp(0.05, 1.0),
        });
        if let Some(t) = &m.texture {
            let png = write_png(t)?;
            align(&mut bin);
            views.push(json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": png.len() }));
            bin.extend_from_slice(&png);
            images.push(json!({ "bufferView": views.len() - 1, "mimeType": "image/png" }));
            textures.push(json!({ "source": images.len() - 1 }));
            pbr["baseColorTexture"] = json!({ "index": textures.len() - 1 });
            pbr["baseColorFactor"] = json!([1.0, 1.0, 1.0, m.opacity.clamp(0.0, 1.0) as f64]);
        }
        let mut mat = json!({ "name": m.name, "pbrMetallicRoughness": pbr, "doubleSided": true });
        if m.opacity < 1.0 {
            mat["alphaMode"] = json!("BLEND");
        }
        if m.emission != [0.0; 3] {
            // Brighter than 1 needs the extension for the strength.
            let k = m.emission.iter().fold(1.0f32, |a, b| a.max(*b));
            mat["emissiveFactor"] = json!(m.emission.map(|c| srgb_to_linear(c / k)));
            if k > 1.0 {
                mat["extensions"] = json!({ "KHR_materials_emissive_strength": { "emissiveStrength": k } });
            }
        }
        materials.push(mat);
    }
    // The triangles, by object when each is a node, and by material.
    let mut groups: std::collections::BTreeMap<(u32, u32), Vec<usize>> = std::collections::BTreeMap::new();
    for (i, t) in mesh.tris.iter().enumerate() {
        groups.entry((if apart { t.object } else { 0 }, t.material)).or_default().push(i);
    }
    let mut by_object: std::collections::BTreeMap<u32, Vec<Json>> = std::collections::BTreeMap::new();
    let local = |p: [f64; 3]| [(p[0] - o[0]) as f32, (p[2] - o[2]) as f32, -(p[1] - o[1]) as f32];
    // Lines and points: corners in pairs (mode 1, LINES) and one by one (mode 0, POINTS).
    let mut loose: std::collections::BTreeMap<(u32, u32, u32), Vec<[f32; 3]>> = std::collections::BTreeMap::new();
    for (s, m) in segs.iter().zip(&line_mats) {
        let e = loose.entry((if apart { s.0 } else { 0 }, *m, 1)).or_default();
        e.push(local(mesh.positions[s.1 as usize]));
        e.push(local(mesh.positions[s.2 as usize]));
    }
    for (p, m) in points.iter().zip(&point_mats) {
        loose.entry((if apart { p.0 } else { 0 }, *m, 0)).or_default().push(local(mesh.positions[p.1 as usize]));
    }
    for ((object, material, mode), pos) in loose {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        align(&mut bin);
        let data: Vec<u8> = pos.iter().flat_map(|p| p.iter().flat_map(|x| x.to_le_bytes())).collect();
        views.push(json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": data.len(), "target": 34962 }));
        bin.extend_from_slice(&data);
        accessors.push(json!({ "bufferView": views.len() - 1, "componentType": 5126, "count": pos.len(), "type": "VEC3", "min": lo, "max": hi }));
        by_object.entry(object).or_default().push(json!({ "attributes": { "POSITION": accessors.len() - 1 }, "material": material, "mode": mode }));
    }
    for ((object, material), tris) in groups {
        let textured = mesh.materials.get(material as usize).is_some_and(|m| m.texture.is_some());
        let (mut pos, mut nrm, mut uv) = (Vec::<[f32; 3]>::new(), Vec::<[f32; 3]>::new(), Vec::<[f32; 2]>::new());
        for i in tris {
            let t = &mesh.tris[i];
            let p = t.v.map(|k| mesh.positions[k as usize]);
            let flat = face_normal(p[0], p[1], p[2]);
            for (k, corner) in p.iter().enumerate() {
                pos.push(local(*corner));
                let n = if t.n[k] != NONE { mesh.normals.get(t.n[k] as usize).copied().unwrap_or(flat) } else { flat };
                nrm.push([n[0], n[2], -n[1]]);
                if textured {
                    let q = if t.t[k] != NONE { mesh.uvs.get(t.t[k] as usize).copied().unwrap_or([0.0; 2]) } else { [0.0; 2] };
                    uv.push([q[0], 1.0 - q[1]]);
                }
            }
        }
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let mut add = |data: &[u8], count: usize, ty: &str, extra: Option<(Json, Json)>| -> usize {
            align(&mut bin);
            views.push(json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": data.len(), "target": 34962 }));
            bin.extend_from_slice(data);
            let mut a = json!({ "bufferView": views.len() - 1, "componentType": 5126, "count": count, "type": ty });
            if let Some((mn, mx)) = extra {
                a["min"] = mn;
                a["max"] = mx;
            }
            accessors.push(a);
            accessors.len() - 1
        };
        let bytes3 = |v: &[[f32; 3]]| v.iter().flat_map(|p| p.iter().flat_map(|x| x.to_le_bytes())).collect::<Vec<u8>>();
        let ap = add(&bytes3(&pos), pos.len(), "VEC3", Some((json!(lo), json!(hi))));
        let an = add(&bytes3(&nrm), nrm.len(), "VEC3", None);
        let mut attributes = json!({ "POSITION": ap, "NORMAL": an });
        if textured {
            let b: Vec<u8> = uv.iter().flat_map(|p| p.iter().flat_map(|x| x.to_le_bytes())).collect();
            attributes["TEXCOORD_0"] = json!(add(&b, uv.len(), "VEC2", None));
        }
        by_object.entry(object).or_default().push(json!({ "attributes": attributes, "material": material, "mode": 4 }));
    }
    // The nodes, under a node for each group when the features have groups.
    let mut top: Vec<usize> = Vec::new();
    let mut group_node: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut children: Vec<(usize, String)> = Vec::new();
    for (object, primitives) in by_object {
        meshes.push(json!({ "primitives": primitives }));
        let mut node = json!({ "mesh": meshes.len() - 1 });
        if apart {
            let own = opts.names.as_ref().and_then(|n| n.get(object as usize)).filter(|n| !n.is_empty());
            if let Some(name) = own.or(mesh.objects.get(object as usize)) {
                node["name"] = json!(name);
            }
            if let Some(a) = attrs_json(table, object as usize) {
                node["extras"] = a;
            }
        }
        nodes.push(node);
        let me = nodes.len() - 1;
        match opts.groups.as_ref().filter(|_| apart).and_then(|g| g.get(object as usize)) {
            Some(g) => children.push((me, if g.is_empty() { "(none)".to_string() } else { g.clone() })),
            None => top.push(me),
        }
    }
    for (_, g) in &children {
        if !group_node.contains_key(g) {
            nodes.push(json!({ "name": g, "children": [] }));
            group_node.insert(g.clone(), nodes.len() - 1);
            top.push(nodes.len() - 1);
        }
    }
    for (child, g) in &children {
        let parent = group_node[g];
        if let Some(list) = nodes[parent]["children"].as_array_mut() {
            list.push(json!(child));
        }
    }
    // The light of the sun and a camera.
    let (lo, hi) = mesh.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
    let middle = [((lo[0] + hi[0]) / 2.0 - o[0]), ((lo[2] + hi[2]) / 2.0 - o[2]), -((lo[1] + hi[1]) / 2.0 - o[1])];
    let radius = (((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() / 2.0).max(1.0);
    let mut lights = false;
    if let Some((az, alt)) = opts.sun {
        let (a, h) = (az.to_radians(), alt.max(1.0).to_radians());
        // Towards the sun: east, up, south as glTF has its axes. The light shines the other way.
        let to_sun = [a.sin() * h.cos(), h.sin(), -a.cos() * h.cos()];
        let at = [middle[0] + to_sun[0] * radius * 2.0, middle[1] + to_sun[1] * radius * 2.0, middle[2] + to_sun[2] * radius * 2.0];
        nodes.push(
            json!({ "name": "Sun", "matrix": look_matrix(at, [-to_sun[0], -to_sun[1], -to_sun[2]]), "extensions": { "KHR_lights_punctual": { "light": 0 } } }),
        );
        top.push(nodes.len() - 1);
        lights = true;
    }
    let mut cameras = Vec::new();
    if opts.camera {
        // From the south-west, 35° above the horizon.
        let (a, h) = (225f64.to_radians(), 35f64.to_radians());
        let d = radius * 2.4;
        let from = [a.sin() * h.cos(), h.sin(), -a.cos() * h.cos()];
        let at = [middle[0] + from[0] * d, middle[1] + from[1] * d, middle[2] + from[2] * d];
        cameras.push(json!({ "type": "perspective", "perspective": { "yfov": 0.7, "znear": (d / 1000.0).max(0.1), "zfar": d * 20.0 } }));
        nodes.push(json!({ "name": "Camera", "camera": 0, "matrix": look_matrix(at, [-from[0], -from[1], -from[2]]) }));
        top.push(nodes.len() - 1);
    }
    align(&mut bin);
    let mut scene = json!({ "nodes": top });
    if opts.georef {
        if let Some(c) = table.crs {
            // As BlenderGIS keeps where a scene lies, so that it can add maps and terrain in place.
            scene["extras"] = json!({ "crs": format!("EPSG:{}", c), "crs x": o[0], "crs y": o[1], "ographis_z": o[2] });
        }
    }
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "Ographis", "extras": { "ographis_origin": o, "crs": table.crs } },
        "scene": 0,
        "scenes": [scene],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    let mut used: Vec<&str> = Vec::new();
    if mesh.materials.iter().any(|m| m.emission.iter().any(|c| *c > 1.0)) {
        used.push("KHR_materials_emissive_strength");
    }
    if lights {
        used.push("KHR_lights_punctual");
        doc["extensions"] =
            json!({ "KHR_lights_punctual": { "lights": [{ "type": "directional", "name": "Sun", "color": [1.0, 0.96, 0.9], "intensity": 4.0 }] } });
    }
    if !used.is_empty() {
        doc["extensionsUsed"] = json!(used);
    }
    if !cameras.is_empty() {
        doc["cameras"] = json!(cameras);
    }
    if !images.is_empty() {
        doc["images"] = json!(images);
        doc["textures"] = json!(textures);
        doc["samplers"] = json!([{}]);
        for t in doc["textures"].as_array_mut().into_iter().flatten() {
            t["sampler"] = json!(0);
        }
    }
    let mut text = serde_json::to_vec(&doc).map_err(|e| GbError(e.to_string()))?;
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?);
    let parts: [&[u8]; 9] = [
        b"glTF",
        &2u32.to_le_bytes(),
        &(total as u32).to_le_bytes(),
        &(text.len() as u32).to_le_bytes(),
        &0x4E4F_534Au32.to_le_bytes(),
        &text,
        &(bin.len() as u32).to_le_bytes(),
        &0x004E_4942u32.to_le_bytes(),
        &bin,
    ];
    for b in parts {
        f.write_all(b).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    f.flush().map_err(|e| GbError(e.to_string()))?;
    Ok(vec![path.to_path_buf()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, FieldType};

    fn block() -> Table {
        let mut mesh = Mesh {
            materials: vec![
                crate::mesh::Material { name: "walls".into(), diffuse: [0.8, 0.7, 0.6], ..Default::default() },
                crate::mesh::Material { name: "top".into(), opacity: 0.5, ..Default::default() },
            ],
            ..Default::default()
        };
        let sq = geo::Polygon::new(
            vec![(418000.0, 6472000.0), (418010.0, 6472000.0), (418010.0, 6472008.0), (418000.0, 6472008.0), (418000.0, 6472000.0)].into(),
            vec![],
        );
        crate::nodes::add_block(&mut mesh, &[&sq], 85.0, 97.5, 0);
        mesh.objects.push("house 1".into());
        Table {
            fields: vec![Field::new("floors", FieldType::Int)],
            crs: Some(3006),
            features: vec![Feature::new(vec![Value::Int(4)], Some(geo::Geometry::Polygon(sq)))],
            mesh: Some(std::sync::Arc::new(mesh)),
            ..Default::default()
        }
    }

    #[test]
    fn a_model_written_is_read_back_in_its_place() {
        let dir = std::env::temp_dir().join("og_mesh_write");
        std::fs::create_dir_all(&dir).unwrap();
        let t = block();
        let want = t.mesh.as_ref().unwrap().bounds().unwrap();
        for name in ["block.obj", "block.glb"] {
            let path = dir.join(name);
            let files = if name.ends_with("obj") { write_obj(&t, &path).unwrap() } else { write_glb(&t, &path).unwrap() };
            assert!(files.iter().all(|f| f.exists()));
            let mut notes = crate::io::Notes::default();
            let opts = crate::obj_io::ObjOpts { y_up: name.ends_with("glb"), ..Default::default() };
            let back = if name.ends_with("obj") {
                crate::obj_io::read_obj(&path, &opts, &mut notes, &|_, _| true).unwrap()
            } else {
                crate::gltf_io::read_gltf(&path, &opts, &mut notes).unwrap()
            };
            let m = back.mesh.as_ref().unwrap();
            let got = m.bounds().unwrap();
            for k in 0..3 {
                assert!((got.0[k] - want.0[k]).abs() < 1e-3 && (got.1[k] - want.1[k]).abs() < 1e-3, "{}: {:?} {:?}", name, got, want);
            }
            assert_eq!(back.crs, Some(3006), "{}", name);
            assert_eq!(m.tris.len(), t.mesh.as_ref().unwrap().tris.len());
            let own = if name.ends_with("obj") { "house_1" } else { "house 1" };
            assert_eq!(back.features[0].attrs[0], Value::Text(own.into()));
            assert!(m.materials.iter().any(|x| x.name == "top" && (x.opacity - 0.5).abs() < 1e-3), "{}", name);
        }
    }

    fn json_of(path: &Path) -> Json {
        let b = std::fs::read(path).unwrap();
        let n = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as usize;
        serde_json::from_slice(&b[20..20 + n]).unwrap()
    }

    #[test]
    fn a_model_for_blender_with_groups_materials_sun_and_camera() {
        let dir = std::env::temp_dir().join("og_mesh_write_blender");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("block.glb");
        let t = block();
        let opts = GltfOpts {
            georef: true,
            names: Some(vec!["Storgatan 1".into()]),
            groups: Some(vec!["houses".into()]),
            kinds: Some(vec!["brick".into()]),
            sun: Some((225.0, 40.0)),
            camera: true,
            origin: None,
        };
        write_glb_with(&t, &path, &opts).unwrap();
        let j = json_of(&path);
        assert_eq!(j["scenes"][0]["extras"]["crs"], json!("EPSG:3006"));
        assert_eq!(j["scenes"][0]["extras"]["crs x"], json!(418005.0));
        let names: Vec<&str> = j["nodes"].as_array().unwrap().iter().filter_map(|n| n["name"].as_str()).collect();
        assert!(names.contains(&"Storgatan 1") && names.contains(&"houses") && names.contains(&"Sun") && names.contains(&"Camera"), "{:?}", names);
        let group = j["nodes"].as_array().unwrap().iter().find(|n| n["name"] == "houses").unwrap();
        assert_eq!(group["children"].as_array().unwrap().len(), 1);
        let mats: Vec<&str> = j["materials"].as_array().unwrap().iter().filter_map(|m| m["name"].as_str()).collect();
        assert_eq!(mats, vec!["brick walls", "brick top"]);
        assert!(j["extensionsUsed"].as_array().unwrap().contains(&json!("KHR_lights_punctual")));
        // The sun shines down: −Z of its node points below the horizon.
        let sun = j["nodes"].as_array().unwrap().iter().find(|n| n["name"] == "Sun").unwrap();
        assert!(sun["matrix"][9].as_f64().unwrap() > 0.0, "the Z column points up, so −Z points down");
        // Read back in its place, through the group.
        let back = crate::gltf_io::read_gltf(&path, &crate::obj_io::ObjOpts { y_up: true, ..Default::default() }, &mut crate::io::Notes::default()).unwrap();
        assert_eq!(back.mesh.as_ref().unwrap().tris.len(), t.mesh.as_ref().unwrap().tris.len());
    }

    #[test]
    fn lines_and_points_are_written_without_a_model() {
        let dir = std::env::temp_dir().join("og_mesh_write_lines");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lines.glb");
        let mut road = Feature::new(vec![], Some(geo::Geometry::LineString(vec![(1000.0, 2000.0), (1100.0, 2000.0), (1100.0, 2100.0)].into())));
        road.z = Some(std::sync::Arc::from(vec![10.0, 12.0, 15.0]));
        let t = Table {
            fields: vec![],
            crs: Some(3006),
            features: vec![road, Feature::new(vec![], Some(geo::Geometry::Point(geo::Point::new(1050.0, 2050.0))))],
            ..Default::default()
        };
        write_glb_with(&t, &path, &GltfOpts::default()).unwrap();
        let j = json_of(&path);
        let modes: Vec<u64> =
            j["meshes"].as_array().unwrap().iter().flat_map(|m| m["primitives"].as_array().unwrap().iter().map(|p| p["mode"].as_u64().unwrap())).collect();
        assert_eq!(modes, vec![1, 0]);
        assert!(j["scenes"][0].get("extras").is_none());
    }
}
