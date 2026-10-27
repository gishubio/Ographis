//! Reads Wavefront OBJ files with their MTL materials and texture pictures: a 3D model of
//! triangles, and a feature for each object with its faces as a MultiPolygon with heights.
//! OBJ says nothing of where it stands or in which units, so that is set when it is read.

use crate::io::Notes;
use crate::mesh::{triangulate, Material, Mesh, Texture, Tri, NONE};
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where a model stands and how it is turned.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjOpts {
    /// Metres per unit of the file. None guesses from its size.
    pub unit: Option<f64>,
    /// Whether the file has y upwards, as many 3D programs do, instead of z.
    pub y_up: bool,
    /// Where the origin of the file lies: east, north and height.
    pub origin: [f64; 3],
    /// Turned clockwise, seen from above, in degrees.
    pub rotation: f64,
    pub crs: Option<u32>,
}

impl Default for ObjOpts {
    fn default() -> Self {
        ObjOpts { unit: None, y_up: false, origin: [0.0; 3], rotation: 0.0, crs: None }
    }
}

/// The largest side of a texture. Larger pictures are made smaller when read.
const MOST_TEXTURE: u32 = 4096;

fn read_texture(path: &Path, notes: &mut Notes) -> Option<Arc<Texture>> {
    let img = match image::open(path) {
        Ok(i) => i,
        Err(e) => {
            notes.warn(format!("The texture {} cannot be read: {}", path.display(), e));
            return None;
        }
    };
    let img = if img.width().max(img.height()) > MOST_TEXTURE { img.thumbnail(MOST_TEXTURE, MOST_TEXTURE) } else { img };
    let rgba = img.to_rgba8();
    Some(Arc::new(Texture { width: rgba.width() as usize, height: rgba.height() as usize, rgba: rgba.into_raw() }))
}

/// The file a map_Kd line names: the rest of the line, which can have options before the
/// name and spaces in it.
fn texture_file(dir: &Path, rest: &str) -> Option<PathBuf> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    for start in 0..tokens.len() {
        let name = tokens[start..].join(" ");
        let p = dir.join(&name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn read_mtl(path: &Path, materials: &mut Vec<Material>, notes: &mut Notes) -> Result<()> {
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read the materials {}: {}", path.display(), e)))?;
    let text = String::from_utf8_lossy(&bytes);
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut textures: HashMap<PathBuf, Option<Arc<Texture>>> = HashMap::new();
    let nums = |rest: &str| -> Vec<f32> { rest.split_whitespace().filter_map(|t| t.parse::<f32>().ok()).collect() };
    for line in text.lines() {
        let line = line.trim();
        let (key, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(k, r)| (k, r.trim()));
        if key == "newmtl" {
            materials.push(Material { name: rest.to_string(), ..Material::default() });
            continue;
        }
        let Some(m) = materials.last_mut() else { continue };
        match key {
            "Kd" => {
                if let [r, g, b, ..] = nums(rest)[..] {
                    m.diffuse = [r, g, b];
                }
            }
            "Ks" => {
                if let [r, g, b, ..] = nums(rest)[..] {
                    m.specular = [r, g, b];
                }
            }
            "Ke" => {
                if let [r, g, b, ..] = nums(rest)[..] {
                    m.emission = [r, g, b];
                }
            }
            "Ns" => {
                if let Some(v) = nums(rest).first() {
                    m.shininess = *v;
                }
            }
            "d" => {
                if let Some(v) = nums(rest).first() {
                    m.opacity = v.clamp(0.0, 1.0);
                }
            }
            "Tr" => {
                if let Some(v) = nums(rest).first() {
                    m.opacity = (1.0 - v).clamp(0.0, 1.0);
                }
            }
            "map_Kd" if !rest.is_empty() => match texture_file(dir, rest) {
                Some(p) => {
                    let t = textures.entry(p.clone()).or_insert_with(|| read_texture(&p, notes)).clone();
                    m.texture = t;
                }
                None => notes.warn(format!("The texture {} of {} is not there.", rest, m.name)),
            },
            _ => {}
        }
    }
    Ok(())
}

/// One index of a face, as the file gives it: from 1, or from the end when negative.
fn index(token: &str, count: usize) -> Option<u32> {
    if token.is_empty() {
        return None;
    }
    let i: i64 = token.parse().ok()?;
    let k = if i < 0 { count as i64 + i } else { i - 1 };
    (k >= 0 && (k as usize) < count).then_some(k as u32)
}

/// A face as it is read: its corners (position, texture, normal), material and object.
type Face = (Vec<(u32, u32, u32)>, u32, u32);

/// Reads a model and its materials, and places it.
pub fn read_obj(path: &Path, opts: &ObjOpts, notes: &mut Notes, going: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let text = String::from_utf8_lossy(&bytes);
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    // A model Ographis wrote goes back where it was, in metres, unless the input places it.
    let mut own = opts.clone();
    if let Some(rest) = text.lines().take(5).find_map(|l| l.strip_prefix(crate::mesh_write::OBJ_ORIGIN)) {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let v: Vec<f64> = words.iter().take(3).filter_map(|w| w.parse().ok()).collect();
        if v.len() == 3 && own.origin == [0.0; 3] && own.rotation == 0.0 {
            own.origin = [v[0], v[1], v[2]];
            own.unit = own.unit.or(Some(1.0));
            own.crs = own.crs.or_else(|| words.get(3).and_then(|w| w.strip_prefix("EPSG:")).and_then(|c| c.parse().ok()));
            notes.info(format!("placed at {}, {}, {}, as Ographis wrote it", v[0], v[1], v[2]));
        }
    }
    let opts = &own;
    let mut raw_pos: Vec<[f64; 3]> = Vec::new();
    let mut raw_nrm: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut materials: Vec<Material> = Vec::new();
    // The faces as they are read: corners (position, uv, normal), material and object.
    let mut faces: Vec<Face> = Vec::new();
    let mut objects: Vec<String> = Vec::new();
    let mut object: Option<u32> = None;
    let mut material: Option<u32> = None;
    let mut by_name: HashMap<String, u32> = HashMap::new();
    let mut lines_read = 0u64;
    let total = bytes.len() as u64;
    let mut done_bytes = 0u64;
    for line in text.lines() {
        lines_read += 1;
        done_bytes += line.len() as u64 + 1;
        if lines_read.is_multiple_of(100_000) && !going(done_bytes, Some(total)) {
            return err(crate::model::STOPPED);
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(k, r)| (k, r.trim()));
        let nums = || rest.split_whitespace().filter_map(|t| t.parse::<f64>().ok()).collect::<Vec<f64>>();
        match key {
            "v" => {
                if let [x, y, z, ..] = nums()[..] {
                    raw_pos.push([x, y, z]);
                }
            }
            "vn" => {
                if let [x, y, z, ..] = nums()[..] {
                    raw_nrm.push([x as f32, y as f32, z as f32]);
                }
            }
            "vt" => {
                let n = nums();
                if let [u, v, ..] = n[..] {
                    uvs.push([u as f32, v as f32]);
                } else if let [u] = n[..] {
                    uvs.push([u as f32, 0.0]);
                }
            }
            "g" | "o" => {
                let name = if rest.is_empty() { format!("object {}", objects.len() + 1) } else { rest.to_string() };
                let id = *by_name.entry(name.clone()).or_insert_with(|| {
                    objects.push(name);
                    (objects.len() - 1) as u32
                });
                object = Some(id);
            }
            "usemtl" => {
                material = match materials.iter().position(|m| m.name == rest) {
                    Some(i) => Some(i as u32),
                    None => {
                        materials.push(Material { name: rest.to_string(), ..Material::default() });
                        Some((materials.len() - 1) as u32)
                    }
                };
            }
            "mtllib" => {
                // The name can have spaces; the whole rest is tried first.
                let file = dir.join(rest);
                let found = if file.is_file() { Some(file) } else { rest.split_whitespace().map(|n| dir.join(n)).find(|p| p.is_file()) };
                match found {
                    Some(f) => {
                        let mut more = Vec::new();
                        read_mtl(&f, &mut more, notes)?;
                        // Materials named before the file was read get what it says of them.
                        for m in more {
                            match materials.iter_mut().find(|x| x.name == m.name) {
                                Some(x) => *x = m,
                                None => materials.push(m),
                            }
                        }
                    }
                    None => notes.warn(format!("The materials {} are not there, so the model is grey.", rest)),
                }
            }
            "f" => {
                let mut corners = Vec::new();
                for t in rest.split_whitespace() {
                    let mut parts = t.split('/');
                    let v = parts.next().and_then(|s| index(s, raw_pos.len()));
                    let vt = parts.next().and_then(|s| index(s, uvs.len())).unwrap_or(NONE);
                    let vn = parts.next().and_then(|s| index(s, raw_nrm.len())).unwrap_or(NONE);
                    if let Some(v) = v {
                        corners.push((v, vt, vn));
                    }
                }
                if corners.len() >= 3 {
                    let o = *object.get_or_insert_with(|| {
                        objects.push("object".to_string());
                        (objects.len() - 1) as u32
                    });
                    let m = *material.get_or_insert_with(|| {
                        materials.push(Material { name: "default".into(), ..Material::default() });
                        (materials.len() - 1) as u32
                    });
                    faces.push((corners, m, o));
                }
            }
            _ => {}
        }
    }
    if faces.is_empty() {
        return err(format!("{} has no faces.", path.display()));
    }
    // The units, from the size of the model when they are not given.
    let (lo, hi) = raw_pos.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(mut lo, mut hi), p| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
        (lo, hi)
    });
    let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
    let unit = match opts.unit {
        Some(u) => u,
        None => {
            let (u, name) = if size > 2000.0 {
                (0.001, "millimetres")
            } else if size > 200.0 {
                (0.01, "centimetres")
            } else {
                (1.0, "metres")
            };
            notes.info(format!(
                "the model is {} units across, so they were taken as {}; set the unit if that is wrong",
                crate::model::fmt_real((size * 10.0).round() / 10.0),
                name
            ));
            u
        }
    };
    let at = placement(opts, unit);
    let positions: Vec<[f64; 3]> = raw_pos.iter().map(|p| at.place(*p)).collect();
    let normals: Vec<[f32; 3]> = raw_nrm.iter().map(|n| at.turn(*n)).collect();
    // The triangles, and the faces of each object for its feature.
    let mut tris = Vec::with_capacity(faces.len() * 2);
    let mut parts: Vec<Vec<(Vec<[f64; 3]>, u32)>> = vec![Vec::new(); objects.len()];
    for (corners, m, o) in &faces {
        let pts: Vec<[f64; 3]> = corners.iter().map(|c| positions[c.0 as usize]).collect();
        for [a, b, cc] in triangulate(&pts) {
            let (ca, cb, ccc) = (corners[a], corners[b], corners[cc]);
            tris.push(Tri { v: [ca.0, cb.0, ccc.0], t: [ca.1, cb.1, ccc.1], n: [ca.2, cb.2, ccc.2], material: *m, object: *o });
        }
        parts[*o as usize].push((pts, *m));
    }
    let mesh = Mesh { positions, normals, uvs, tris, materials, objects };
    Ok(model_table(mesh, parts, "faces", opts.crs, notes))
}

/// From the file to the map: units, which way is up, turning, and the origin.
pub(crate) struct Placement {
    unit: f64,
    y_up: bool,
    sin: f64,
    cos: f64,
    origin: [f64; 3],
}

impl Placement {
    fn orient(&self, p: [f64; 3]) -> [f64; 3] {
        if self.y_up {
            [p[0], -p[2], p[1]]
        } else {
            p
        }
    }

    /// Where a point of the file goes on the map.
    pub(crate) fn place(&self, p: [f64; 3]) -> [f64; 3] {
        let q = self.orient(p);
        let (x, y, z) = (q[0] * self.unit, q[1] * self.unit, q[2] * self.unit);
        [x * self.cos - y * self.sin + self.origin[0], x * self.sin + y * self.cos + self.origin[1], z + self.origin[2]]
    }

    /// Which way a normal of the file points on the map.
    pub(crate) fn turn(&self, n: [f32; 3]) -> [f32; 3] {
        let q = self.orient([n[0] as f64, n[1] as f64, n[2] as f64]);
        [(q[0] * self.cos - q[1] * self.sin) as f32, (q[0] * self.sin + q[1] * self.cos) as f32, q[2] as f32]
    }
}

pub(crate) fn placement(opts: &ObjOpts, unit: f64) -> Placement {
    let (sin, cos) = (-opts.rotation.to_radians()).sin_cos();
    Placement { unit, y_up: opts.y_up, sin, cos, origin: opts.origin }
}

/// A stream of the objects of a model: one feature for each, with its faces as a
/// MultiPolygon with heights, its main material and its height, and the model beside.
/// `parts` holds the faces of each object, as corners and material.
pub(crate) fn model_table(mesh: Mesh, parts: Vec<Vec<(Vec<[f64; 3]>, u32)>>, faces_name: &str, crs: Option<u32>, notes: &mut Notes) -> Table {
    let fields = vec![
        Field::new("name", FieldType::Text),
        Field::new("material", FieldType::Text),
        Field::new(faces_name, FieldType::Int),
        Field::new("z_min", FieldType::Real),
        Field::new("z_max", FieldType::Real),
        Field::new("height", FieldType::Real),
    ];
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let mut features = Vec::with_capacity(mesh.objects.len());
    for (k, faces) in parts.iter().enumerate() {
        let mut polys = Vec::with_capacity(faces.len());
        let mut z = Vec::new();
        let mut count: HashMap<u32, usize> = HashMap::new();
        let (mut zlo, mut zhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (pts, m) in faces {
            *count.entry(*m).or_default() += 1;
            let mut ring: Vec<geo::Coord<f64>> = pts.iter().map(|p| geo::coord! { x: p[0], y: p[1] }).collect();
            ring.push(ring[0]);
            for p in pts.iter().chain(std::iter::once(&pts[0])) {
                z.push(p[2]);
                zlo = zlo.min(p[2]);
                zhi = zhi.max(p[2]);
            }
            polys.push(geo::Polygon::new(geo::LineString::new(ring), Vec::new()));
        }
        let main = count.iter().max_by_key(|(m, n)| (**n, std::cmp::Reverse(**m))).map(|(m, _)| mesh.materials[*m as usize].name.clone());
        let geom = (!polys.is_empty()).then(|| geo::Geometry::MultiPolygon(geo::MultiPolygon::new(polys)));
        let attrs = vec![
            Value::Text(mesh.objects.get(k).cloned().unwrap_or_default()),
            main.map_or(Value::Null, Value::Text),
            Value::Int(faces.len() as i64),
            if zlo.is_finite() { Value::Real(round(zlo)) } else { Value::Null },
            if zhi.is_finite() { Value::Real(round(zhi)) } else { Value::Null },
            if zlo.is_finite() { Value::Real(round(zhi - zlo)) } else { Value::Null },
        ];
        features.push(Feature { attrs, geom, z: (!z.is_empty()).then(|| Arc::from(z)) });
    }
    let textured = mesh.materials.iter().filter(|m| m.texture.is_some()).count();
    notes.info(format!(
        "{} objects, {} triangles, {} materials{}",
        fmt_count(mesh.objects.len() as u64),
        fmt_count(mesh.tris.len() as u64),
        mesh.materials.len(),
        if textured > 0 { format!(", {} with textures", textured) } else { String::new() }
    ));
    Table { fields, crs, features, raster: false, image: None, grid: None, mesh: Some(Arc::new(mesh)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_model_is_read_placed_and_turned() {
        let dir = std::env::temp_dir().join(format!("gb-obj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("box.mtl"), "newmtl Red\nKd 0.8 0.1 0.1\nmap_Kd \nnewmtl Glass\nKd 0.5 0.6 0.7\nd 0.1\n").unwrap();
        std::fs::write(
            dir.join("box.obj"),
            "mtllib box.mtl\ng Wall\nusemtl Red\nv 0 0 0\nv 400 0 0\nv 400 0 300\nv 0 0 300\nf 1 2 3 4\ng Window\nusemtl Glass\nv 100 -1 100\nv 200 -1 100\nv 200 -1 200\nf -3 -2 -1\n",
        )
        .unwrap();
        let mut notes = Notes::default();
        let opts = ObjOpts { origin: [1000.0, 2000.0, 50.0], rotation: 90.0, crs: Some(3006), ..Default::default() };
        let t = read_obj(&dir.join("box.obj"), &opts, &mut notes, &|_, _| true).unwrap();
        assert!(notes.info.iter().any(|i| i.contains("centimetres")), "{:?}", notes);
        let m = t.mesh.as_ref().unwrap();
        assert_eq!((m.tris.len(), m.materials.len(), t.len()), (3, 2, 2));
        assert_eq!(m.materials[1].opacity, 0.1);
        // 4 m along x, turned a quarter clockwise, goes south: y gets smaller.
        let p = m.positions[1];
        assert!((p[0] - 1000.0).abs() < 1e-9 && (p[1] - 1996.0).abs() < 1e-9 && (p[2] - 50.0).abs() < 1e-9, "{:?}", p);
        assert_eq!(t.features[0].attrs[0], Value::Text("Wall".into()));
        assert_eq!(t.features[0].attrs[5], Value::Real(3.0));
        assert_eq!(t.features[0].heights().unwrap().len(), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
