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

/// Writes the model to a binary glTF file, with y up as glTF has it. Each object is a node
/// with its name and the attributes of its feature as extras.
pub fn write_glb(table: &Table, path: &Path) -> Result<Vec<PathBuf>> {
    let Some(mesh) = table.mesh.as_deref() else { return err("The stream has no 3D model to write.") };
    let o = origin_of(mesh);
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
    for (object, primitives) in by_object {
        meshes.push(json!({ "primitives": primitives }));
        let mut node = json!({ "mesh": meshes.len() - 1 });
        if apart {
            if let Some(name) = mesh.objects.get(object as usize) {
                node["name"] = json!(name);
            }
            if let Some(a) = attrs_json(table, object as usize) {
                node["extras"] = a;
            }
        }
        nodes.push(node);
    }
    align(&mut bin);
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "Ographis", "extras": { "ographis_origin": o, "crs": table.crs } },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    if mesh.materials.iter().any(|m| m.emission.iter().any(|c| *c > 1.0)) {
        doc["extensionsUsed"] = json!(["KHR_materials_emissive_strength"]);
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
}
