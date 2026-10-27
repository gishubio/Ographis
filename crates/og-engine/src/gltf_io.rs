//! Reads glTF 2.0 models, as .gltf with its buffers and pictures beside it or inside it,
//! or as one binary .glb: the meshes of every node of the scene, placed by the nodes'
//! transforms, with their materials and textures. glTF has y up and metres, so that is
//! how it is read unless the input says otherwise.

use crate::io::Notes;
use crate::mesh::{Material, Mesh, Texture, Tri, NONE};
use crate::model::{err, GbError, Result, Table};
use crate::obj_io::{model_table, placement, ObjOpts};
use serde_json::Value as Json;
use std::path::Path;
use std::sync::Arc;

type M4 = [f64; 16];

const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// a × b, both column-major as glTF writes them.
fn mul(a: &M4, b: &M4) -> M4 {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    out
}

fn apply(m: &M4, p: [f64; 3]) -> [f64; 3] {
    [m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12], m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13], m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14]]
}

/// A normal through a transform: by the inverse of the transpose of its 3 × 3 part.
fn apply_normal(m: &M4, n: [f32; 3]) -> [f32; 3] {
    let a = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if det.abs() < 1e-18 {
        return n;
    }
    // The cofactors are the inverse transposed, times the determinant.
    let co = [
        [a[1][1] * a[2][2] - a[1][2] * a[2][1], a[1][2] * a[2][0] - a[1][0] * a[2][2], a[1][0] * a[2][1] - a[1][1] * a[2][0]],
        [a[0][2] * a[2][1] - a[0][1] * a[2][2], a[0][0] * a[2][2] - a[0][2] * a[2][0], a[0][1] * a[2][0] - a[0][0] * a[2][1]],
        [a[0][1] * a[1][2] - a[0][2] * a[1][1], a[0][2] * a[1][0] - a[0][0] * a[1][2], a[0][0] * a[1][1] - a[0][1] * a[1][0]],
    ];
    let v = [n[0] as f64, n[1] as f64, n[2] as f64];
    let r = [0, 1, 2].map(|i| (co[i][0] * v[0] + co[i][1] * v[1] + co[i][2] * v[2]) * det.signum());
    let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(1e-12);
    r.map(|x| (x / l) as f32)
}

/// The transform of a node: its matrix, or its translation, rotation and scale.
fn local_matrix(node: &Json) -> M4 {
    if let Some(m) = node.get("matrix").and_then(|m| m.as_array()).filter(|m| m.len() == 16) {
        let mut out = [0.0; 16];
        for (i, v) in m.iter().enumerate() {
            out[i] = v.as_f64().unwrap_or(0.0);
        }
        return out;
    }
    let get = |k: &str, d: &[f64]| -> Vec<f64> {
        node.get(k).and_then(|v| v.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_else(|| d.to_vec())
    };
    let t = get("translation", &[0.0, 0.0, 0.0]);
    let q = get("rotation", &[0.0, 0.0, 0.0, 1.0]);
    let s = get("scale", &[1.0, 1.0, 1.0]);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let r = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    [r[0] * s[0], r[1] * s[0], r[2] * s[0], 0.0, r[3] * s[1], r[4] * s[1], r[5] * s[1], 0.0, r[6] * s[2], r[7] * s[2], r[8] * s[2], 0.0, t[0], t[1], t[2], 1.0]
}

/// Bytes from base64, as in data: addresses.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// What a data: address holds.
fn data_uri(uri: &str) -> Result<Vec<u8>> {
    let rest = uri.strip_prefix("data:").unwrap_or(uri);
    let (_, data) = rest.split_once(";base64,").ok_or_else(|| GbError("A data address in the glTF file is not base64.".into()))?;
    base64(data).ok_or_else(|| GbError("A data address in the glTF file cannot be read.".into()))
}

/// What an address in a glTF file holds: data: inside it, or a file beside it.
fn fetch(uri: &str, dir: &Path) -> Result<Vec<u8>> {
    if uri.starts_with("data:") {
        return data_uri(uri);
    }
    // Addresses are URIs: spaces and such are written with %.
    let mut name = String::new();
    let bytes = uri.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&uri[i + 1..i + 3], 16) {
                name.push(v as char);
                i += 3;
                continue;
            }
        }
        name.push(bytes[i] as char);
        i += 1;
    }
    let path = dir.join(&name);
    std::fs::read(&path).map_err(|e| GbError(format!("Cannot read {}, which the glTF file uses: {}", path.display(), e)))
}

struct Gltf {
    json: Json,
    buffers: Vec<Vec<u8>>,
    /// Buffer views compressed with EXT_meshopt_compression, unpacked, with their stride.
    unpacked: std::collections::HashMap<usize, (Vec<u8>, usize)>,
}

/// Unpacks the buffer views that are compressed with EXT_meshopt_compression (or its
/// Khronos twin), as tilers such as tyler and gltfpack write them.
fn unpack_meshopt(json: &Json, buffers: &[Vec<u8>]) -> Result<std::collections::HashMap<usize, (Vec<u8>, usize)>> {
    let mut out = std::collections::HashMap::new();
    let views = json.get("bufferViews").and_then(|v| v.as_array()).map(|v| v.as_slice()).unwrap_or(&[]);
    for (i, v) in views.iter().enumerate() {
        let Some(ext) = v.get("extensions").and_then(|e| e.get("EXT_meshopt_compression").or_else(|| e.get("KHR_meshopt_compression"))) else { continue };
        let num = |k: &str| ext.get(k).and_then(|x| x.as_u64()).map(|x| x as usize);
        let (Some(buffer), Some(len), Some(stride), Some(count)) = (num("buffer"), num("byteLength"), num("byteStride"), num("count")) else {
            return err("A compressed part of the glTF file says too little about itself to be unpacked.");
        };
        let off = num("byteOffset").unwrap_or(0);
        let src = buffers.get(buffer).and_then(|b| b.get(off..off + len)).ok_or_else(|| GbError("A compressed part of the glTF file is cut off.".into()))?;
        let mode = ext.get("mode").and_then(|m| m.as_str()).unwrap_or("ATTRIBUTES");
        let mut dst = vec![0u8; count * stride];
        // SAFETY: dst holds count × stride bytes, as the functions write, and src is read
        // within its length.
        let status = unsafe {
            let d = dst.as_mut_ptr() as *mut std::os::raw::c_void;
            match mode {
                "TRIANGLES" => meshopt::ffi::meshopt_decodeIndexBuffer(d, count, stride, src.as_ptr(), src.len()),
                "INDICES" => meshopt::ffi::meshopt_decodeIndexSequence(d, count, stride, src.as_ptr(), src.len()),
                _ => meshopt::ffi::meshopt_decodeVertexBuffer(d, count, stride, src.as_ptr(), src.len()),
            }
        };
        if status != 0 {
            return err(format!("A part of the glTF file compressed with meshopt cannot be unpacked (error {}).", status));
        }
        let filter = ext.get("filter").and_then(|f| f.as_str()).unwrap_or("NONE");
        // SAFETY: the filters work on count elements of stride bytes, which dst holds.
        unsafe {
            let d = dst.as_mut_ptr() as *mut std::os::raw::c_void;
            match filter {
                "OCTAHEDRAL" if stride == 4 || stride == 8 => meshopt::ffi::meshopt_decodeFilterOct(d, count, stride),
                "QUATERNION" if stride == 8 => meshopt::ffi::meshopt_decodeFilterQuat(d, count, stride),
                "EXPONENTIAL" if stride % 4 == 0 => meshopt::ffi::meshopt_decodeFilterExp(d, count, stride),
                "COLOR" if stride == 4 || stride == 8 => meshopt::ffi::meshopt_decodeFilterColor(d, count, stride),
                "NONE" => {}
                other => return err(format!("The meshopt filter {} with {} bytes to an element is not known.", other, stride)),
            }
        }
        // Indices are read tightly packed; attributes with the stride they were packed with.
        out.insert(i, (dst, if mode == "ATTRIBUTES" { stride } else { 0 }));
    }
    Ok(out)
}

impl Gltf {
    fn arr(&self, key: &str) -> &[Json] {
        self.json.get(key).and_then(|v| v.as_array()).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The bytes of a buffer view.
    fn view(&self, i: usize) -> Option<(&[u8], usize)> {
        if let Some((bytes, stride)) = self.unpacked.get(&i) {
            return Some((bytes.as_slice(), *stride));
        }
        let v = self.arr("bufferViews").get(i)?;
        let buf = self.buffers.get(v.get("buffer")?.as_u64()? as usize)?;
        let off = v.get("byteOffset").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let len = v.get("byteLength")?.as_u64()? as usize;
        let stride = v.get("byteStride").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        Some((buf.get(off..off + len)?, stride))
    }

    /// The values of an accessor, as numbers, `n` to an element: positions are 3, texture
    /// coordinates 2. Normalized integers are made 0 to 1.
    fn floats(&self, i: usize) -> Option<(Vec<f32>, usize)> {
        let a = self.arr("accessors").get(i)?;
        let count = a.get("count")?.as_u64()? as usize;
        let n = match a.get("type")?.as_str()? {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            _ => return None,
        };
        let ct = a.get("componentType")?.as_u64()?;
        let normalized = a.get("normalized").and_then(|x| x.as_bool()).unwrap_or(false);
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return None,
        };
        let Some(view) = a.get("bufferView").and_then(|v| v.as_u64()) else {
            // An accessor without a view is all zeros.
            return Some((vec![0.0; count * n], n));
        };
        let (bytes, stride) = self.view(view as usize)?;
        let start = a.get("byteOffset").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let stride = if stride > 0 { stride } else { n * size };
        let mut out = Vec::with_capacity(count * n);
        for e in 0..count {
            for k in 0..n {
                let at = start + e * stride + k * size;
                let b = bytes.get(at..at + size)?;
                let v = match ct {
                    5126 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                    5123 => {
                        let v = u16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized {
                            v / 65535.0
                        } else {
                            v
                        }
                    }
                    5122 => {
                        let v = i16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized {
                            (v / 32767.0).max(-1.0)
                        } else {
                            v
                        }
                    }
                    5121 => {
                        let v = b[0] as f32;
                        if normalized {
                            v / 255.0
                        } else {
                            v
                        }
                    }
                    _ => {
                        let v = b[0] as i8 as f32;
                        if normalized {
                            (v / 127.0).max(-1.0)
                        } else {
                            v
                        }
                    }
                };
                out.push(v);
            }
        }
        Some((out, n))
    }

    /// Whole numbers of an accessor, as indices are.
    fn indices(&self, i: usize) -> Option<Vec<u32>> {
        let a = self.arr("accessors").get(i)?;
        let count = a.get("count")?.as_u64()? as usize;
        let ct = a.get("componentType")?.as_u64()?;
        let (bytes, stride) = self.view(a.get("bufferView")?.as_u64()? as usize)?;
        let start = a.get("byteOffset").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let size = match ct {
            5121 => 1,
            5123 => 2,
            5125 => 4,
            _ => return None,
        };
        let stride = if stride > 0 { stride } else { size };
        (0..count)
            .map(|e| {
                let at = start + e * stride;
                let b = bytes.get(at..at + size)?;
                Some(match size {
                    1 => b[0] as u32,
                    2 => u16::from_le_bytes([b[0], b[1]]) as u32,
                    _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                })
            })
            .collect()
    }

    /// A picture of the file, from a buffer view or an address.
    fn image(&self, i: usize, fetch: &dyn Fn(&str) -> Result<Vec<u8>>, notes: &mut Notes) -> Option<Arc<Texture>> {
        let img = self.arr("images").get(i)?;
        let bytes = match (img.get("bufferView").and_then(|v| v.as_u64()), img.get("uri").and_then(|u| u.as_str())) {
            (Some(v), _) => self.view(v as usize)?.0.to_vec(),
            (None, Some(uri)) => match if uri.starts_with("data:") { data_uri(uri) } else { fetch(uri) } {
                Ok(b) => b,
                Err(e) => {
                    notes.warn(e.0);
                    return None;
                }
            },
            _ => return None,
        };
        match image::load_from_memory(&bytes) {
            Ok(p) => {
                let p = if p.width().max(p.height()) > 4096 { p.thumbnail(4096, 4096) } else { p };
                let rgba = p.to_rgba8();
                Some(Arc::new(Texture { width: rgba.width() as usize, height: rgba.height() as usize, rgba: rgba.into_raw() }))
            }
            Err(e) => {
                notes.warn(format!("A texture of the glTF file cannot be read: {}", e));
                None
            }
        }
    }
}

/// Which texture coordinates a texture uses, and how they are moved, turned and scaled,
/// after KHR_texture_transform: offset + rotation × scale × uv.
#[derive(Clone, Copy)]
struct UvWay {
    set: u64,
    offset: [f32; 2],
    rotation: f32,
    scale: [f32; 2],
}

impl Default for UvWay {
    fn default() -> Self {
        UvWay { set: 0, offset: [0.0; 2], rotation: 0.0, scale: [1.0; 2] }
    }
}

impl UvWay {
    fn apply(&self, u: f32, v: f32) -> (f32, f32) {
        let (su, sv) = (u * self.scale[0], v * self.scale[1]);
        let (s, c) = self.rotation.sin_cos();
        (c * su + s * sv + self.offset[0], -s * su + c * sv + self.offset[1])
    }
}

fn uv_way(info: Option<&Json>) -> UvWay {
    let Some(info) = info else { return UvWay::default() };
    let pair = |v: Option<&Json>, d: [f32; 2]| -> [f32; 2] {
        v.and_then(|a| a.as_array())
            .filter(|a| a.len() == 2)
            .map_or(d, |a| [a[0].as_f64().unwrap_or(d[0] as f64) as f32, a[1].as_f64().unwrap_or(d[1] as f64) as f32])
    };
    let mut way = UvWay { set: info.get("texCoord").and_then(|v| v.as_u64()).unwrap_or(0), ..UvWay::default() };
    if let Some(t) = info.get("extensions").and_then(|e| e.get("KHR_texture_transform")) {
        way.offset = pair(t.get("offset"), [0.0, 0.0]);
        way.scale = pair(t.get("scale"), [1.0, 1.0]);
        way.rotation = t.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        if let Some(set) = t.get("texCoord").and_then(|v| v.as_u64()) {
            way.set = set;
        }
    }
    way
}

/// The parts of a .glb: its JSON and the binary buffer that follows.
fn split_glb(bytes: &[u8]) -> Result<(Json, Option<Vec<u8>>)> {
    let word = |at: usize| -> Option<u32> { bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) };
    if word(0) != Some(0x4654_6C67) {
        return err("This is not a binary glTF file (.glb).");
    }
    let mut at = 12;
    let (mut json, mut bin) = (None, None);
    while let (Some(len), Some(kind)) = (word(at), word(at + 4)) {
        let body = bytes.get(at + 8..at + 8 + len as usize).ok_or_else(|| GbError("The .glb file is cut off.".into()))?;
        match kind {
            0x4E4F_534A => json = Some(serde_json::from_slice(body).map_err(|e| GbError(format!("The JSON of the .glb file cannot be read: {}", e)))?),
            0x004E_4942 => bin = Some(body.to_vec()),
            _ => {}
        }
        at += 8 + len as usize;
    }
    Ok((json.ok_or_else(|| GbError("The .glb file has no JSON.".into()))?, bin))
}

fn linear_to_srgb(c: f64) -> f32 {
    let c = c.clamp(0.0, 1.0);
    (if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }) as f32
}

/// A glTF model as it is in the file: positions and normals through the transforms of the
/// nodes, but not yet placed on the map. The faces of each object are kept as corners
/// (indices into the positions) and a material, for the features of the stream.
pub(crate) struct Model {
    pub mesh: Mesh,
    pub faces: Vec<Vec<([u32; 3], u32)>>,
    /// For each triangle, the feature it belongs to (_BATCHID in 3D Tiles 1.0,
    /// _FEATURE_ID_0 in 1.1), or NONE.
    pub batch: Vec<u32>,
    /// The centre the positions are relative to (the extension CESIUM_RTC), if any.
    pub rtc: Option<[f64; 3]>,
    /// The property table of the features (EXT_structural_metadata), as a batch table of
    /// 3D Tiles 1.0 has it: each property a list with a value for each feature.
    pub properties: Option<Json>,
    /// How many features the property table holds.
    pub feature_count: usize,
    /// Where the model lies and in what coordinate system, when Ographis wrote it.
    pub origin: Option<([f64; 3], Option<u32>)>,
}

/// Reads a property table of EXT_structural_metadata into a JSON object of lists, a value
/// for each feature, as the batch table of 3D Tiles 1.0. Vectors, matrices and arrays become
/// text with the values separated by commas.
fn property_table(g: &Gltf, index: usize) -> Option<(Json, usize)> {
    let meta = g.json.get("extensions")?.get("EXT_structural_metadata")?;
    let table = meta.get("propertyTables")?.as_array()?.get(index)?;
    let count = table.get("count")?.as_u64()? as usize;
    let class = meta.get("schema")?.get("classes")?.get(table.get("class")?.as_str()?)?;
    let enums = meta.get("schema").and_then(|s| s.get("enums"));
    let mut out = serde_json::Map::new();
    for (name, prop) in table.get("properties")?.as_object()? {
        let Some(def) = class.get("properties").and_then(|p| p.get(name)) else { continue };
        let view = |k: &str| prop.get(k).and_then(|v| v.as_u64()).and_then(|v| g.view(v as usize)).map(|v| v.0);
        let Some(values) = view("values") else { continue };
        let ty = def.get("type").and_then(|t| t.as_str()).unwrap_or("SCALAR");
        let enum_def = def.get("enumType").and_then(|e| e.as_str()).and_then(|e| enums?.get(e));
        let component = match ty {
            "ENUM" => enum_def.and_then(|e| e.get("valueType")).and_then(|v| v.as_str()).unwrap_or("UINT16"),
            _ => def.get("componentType").and_then(|c| c.as_str()).unwrap_or("FLOAT32"),
        };
        let width = match ty {
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT2" => 4,
            "MAT3" => 9,
            "MAT4" => 16,
            _ => 1,
        };
        let normalized = def.get("normalized").and_then(|v| v.as_bool()).unwrap_or(false);
        let num_of = |k: &str| prop.get(k).or_else(|| def.get(k)).and_then(|v| v.as_f64());
        let (offset, scale) = (num_of("offset"), num_of("scale"));
        let size = |c: &str| match c {
            "INT8" | "UINT8" => 1,
            "INT16" | "UINT16" => 2,
            "INT32" | "UINT32" | "FLOAT32" => 4,
            _ => 8,
        };
        // The k-th number of the kind `c` in `bytes`.
        let number = |bytes: &[u8], c: &str, k: usize| -> Option<f64> {
            let n = size(c);
            let b = bytes.get(k * n..k * n + n)?;
            let raw = match c {
                "INT8" => b[0] as i8 as f64,
                "UINT8" => b[0] as f64,
                "INT16" => i16::from_le_bytes([b[0], b[1]]) as f64,
                "UINT16" => u16::from_le_bytes([b[0], b[1]]) as f64,
                "INT32" => i32::from_le_bytes(b.try_into().ok()?) as f64,
                "UINT32" => u32::from_le_bytes(b.try_into().ok()?) as f64,
                "INT64" => i64::from_le_bytes(b.try_into().ok()?) as f64,
                "UINT64" => u64::from_le_bytes(b.try_into().ok()?) as f64,
                "FLOAT32" => f32::from_le_bytes(b.try_into().ok()?) as f64,
                _ => f64::from_le_bytes(b.try_into().ok()?),
            };
            Some(raw)
        };
        let offsets = |k: &str, kind: &str| -> Option<Vec<usize>> {
            let bytes = view(k)?;
            let c = prop.get(kind).and_then(|v| v.as_str()).unwrap_or("UINT32");
            (0..bytes.len() / size(c)).map(|i| number(bytes, c, i).map(|v| v as usize)).collect()
        };
        let array = def.get("array").and_then(|v| v.as_bool()).unwrap_or(false);
        let fixed = def.get("count").and_then(|v| v.as_u64()).map(|v| v as usize);
        let array_offsets = if array && fixed.is_none() { offsets("arrayOffsets", "arrayOffsetType") } else { None };
        let string_offsets = if ty == "STRING" { offsets("stringOffsets", "stringOffsetType") } else { None };
        // The elements of feature f: (first, how many), counting elements of the type.
        let span = |f: usize| -> Option<(usize, usize)> {
            match (&array_offsets, array, fixed) {
                (Some(o), _, _) => {
                    let (a, b) = (*o.get(f)?, *o.get(f + 1)?);
                    // Offsets count bytes, except for strings and booleans, where they count
                    // the strings and the bits.
                    let unit = match ty {
                        "STRING" | "BOOLEAN" => 1,
                        "ENUM" => size(component),
                        _ => size(component) * width,
                    };
                    Some((a / unit, (b - a) / unit))
                }
                (None, true, Some(n)) => Some((f * n, n)),
                _ => Some((f, 1)),
            }
        };
        let element = |e: usize| -> Option<Json> {
            Some(match ty {
                "STRING" => {
                    let o = string_offsets.as_ref()?;
                    Json::String(String::from_utf8_lossy(values.get(*o.get(e)?..*o.get(e + 1)?)?).into_owned())
                }
                "BOOLEAN" => Json::Bool(values.get(e / 8)? >> (e % 8) & 1 == 1),
                "ENUM" => {
                    let v = number(values, component, e)? as i64;
                    let name = enum_def?
                        .get("values")?
                        .as_array()?
                        .iter()
                        .find(|x| x.get("value").and_then(|n| n.as_i64()) == Some(v))
                        .and_then(|x| x.get("name"))
                        .and_then(|n| n.as_str())
                        .map(str::to_string);
                    name.map_or(Json::from(v), Json::String)
                }
                _ => {
                    let mut parts = Vec::with_capacity(width);
                    for k in 0..width {
                        let mut v = number(values, component, e * width + k)?;
                        if normalized {
                            v = match component {
                                "UINT8" => v / 255.0,
                                "INT8" => (v / 127.0).max(-1.0),
                                "UINT16" => v / 65535.0,
                                "INT16" => (v / 32767.0).max(-1.0),
                                "UINT32" => v / 4_294_967_295.0,
                                "INT32" => (v / 2_147_483_647.0).max(-1.0),
                                _ => v,
                            };
                        }
                        v = v * scale.unwrap_or(1.0) + offset.unwrap_or(0.0);
                        // A 32-bit decimal holds about seven digits: 0.1, not 0.10000000149.
                        if component == "FLOAT32" || normalized {
                            v = (v as f32).to_string().parse().unwrap_or(v);
                        }
                        parts.push(v);
                    }
                    let one = |v: f64| {
                        if v.fract() == 0.0 && v.abs() < 9e15 && !component.starts_with("FLOAT") && !normalized && scale.is_none() {
                            Json::from(v as i64)
                        } else {
                            Json::from(v)
                        }
                    };
                    if width == 1 {
                        one(parts[0])
                    } else {
                        Json::String(parts.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "))
                    }
                }
            })
        };
        let no_data = def.get("noData").cloned();
        let column: Vec<Json> = (0..count)
            .map(|f| {
                let Some((first, n)) = span(f) else { return Json::Null };
                let v = if array {
                    let items: Vec<String> = (first..first + n)
                        .filter_map(element)
                        .map(|j| match j {
                            Json::String(s) => s,
                            Json::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |v| v.to_string()),
                            other => other.to_string(),
                        })
                        .collect();
                    Json::String(items.join(", "))
                } else {
                    element(first).unwrap_or(Json::Null)
                };
                if no_data.as_ref() == Some(&v) {
                    Json::Null
                } else {
                    v
                }
            })
            .collect();
        out.insert(name.clone(), Json::Array(column));
    }
    Some((Json::Object(out), count))
}

/// The JSON and the buffers of a glTF file, .gltf or .glb, from its bytes. `fetch` gives
/// what an address in the file holds: a buffer or a picture beside it.
pub(crate) fn parse(bytes: &[u8], fetch: &dyn Fn(&str) -> Result<Vec<u8>>, notes: &mut Notes) -> Result<Model> {
    let (json, bin) = if bytes.starts_with(b"glTF") {
        split_glb(bytes)?
    } else {
        (serde_json::from_slice(bytes).map_err(|e| GbError(format!("This cannot be read as glTF: {}", e)))?, None)
    };
    if json.get("asset").and_then(|a| a.get("version")).and_then(|v| v.as_str()).is_some_and(|v| !v.starts_with('2')) {
        return err("Only glTF 2.0 is read.");
    }
    for ext in json.get("extensionsRequired").and_then(|e| e.as_array()).into_iter().flatten().filter_map(|e| e.as_str()) {
        if matches!(ext, "KHR_draco_mesh_compression") {
            return err(format!("The model is compressed with {}, which Ographis does not read. Save it from the 3D program without compression.", ext));
        }
    }
    let rtc = json.get("extensions").and_then(|e| e.get("CESIUM_RTC")).and_then(|r| r.get("center")).and_then(|c| c.as_array()).and_then(|c| {
        let v: Vec<f64> = c.iter().filter_map(|x| x.as_f64()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    });
    let origin = json.get("asset").and_then(|a| a.get("extras")).and_then(|e| {
        let v: Vec<f64> = e.get("ographis_origin")?.as_array()?.iter().filter_map(|x| x.as_f64()).collect();
        (v.len() == 3).then(|| ([v[0], v[1], v[2]], e.get("crs").and_then(|c| c.as_u64()).map(|c| c as u32)))
    });
    let mut buffers = Vec::new();
    for (i, b) in json.get("buffers").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
        let data = match b.get("uri").and_then(|u| u.as_str()) {
            Some(uri) => match uri.strip_prefix("data:") {
                Some(_) => data_uri(uri)?,
                None => fetch(uri)?,
            },
            None if i == 0 => bin.clone().ok_or_else(|| GbError("The glTF file names a buffer that is not there.".into()))?,
            None => Vec::new(),
        };
        buffers.push(data);
    }
    let unpacked = unpack_meshopt(&json, &buffers)?;
    let g = Gltf { json, buffers, unpacked };

    // Materials, with the textures they use read once.
    let mut images: Vec<Option<Option<Arc<Texture>>>> = vec![None; g.arr("images").len()];
    let mut materials: Vec<Material> = Vec::new();
    // For each material, the texture coordinates its colour texture uses, and how they are
    // moved, turned and scaled first (KHR_texture_transform).
    let mut uv_ways: Vec<UvWay> = Vec::new();
    for (i, m) in g.arr("materials").iter().enumerate() {
        let pbr = m.get("pbrMetallicRoughness");
        let factor: Vec<f64> = pbr
            .and_then(|p| p.get("baseColorFactor"))
            .and_then(|f| f.as_array())
            .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(1.0)).collect())
            .unwrap_or_else(|| vec![1.0; 4]);
        let metallic = pbr.and_then(|p| p.get("metallicFactor")).and_then(|v| v.as_f64()).unwrap_or(1.0);
        let rough = pbr.and_then(|p| p.get("roughnessFactor")).and_then(|v| v.as_f64()).unwrap_or(1.0);
        let blend = m.get("alphaMode").and_then(|a| a.as_str()) == Some("BLEND");
        let transmission = m
            .get("extensions")
            .and_then(|e| e.get("KHR_materials_transmission"))
            .and_then(|t| t.get("transmissionFactor"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let mut opacity = if blend { factor.get(3).copied().unwrap_or(1.0) } else { 1.0 };
        opacity = opacity.min(1.0 - transmission * 0.9);
        uv_ways.push(uv_way(pbr.and_then(|p| p.get("baseColorTexture"))));
        let texture = pbr
            .and_then(|p| p.get("baseColorTexture"))
            .and_then(|t| t.get("index"))
            .and_then(|t| t.as_u64())
            .and_then(|t| g.arr("textures").get(t as usize))
            .and_then(|t| t.get("source"))
            .and_then(|s| s.as_u64())
            .and_then(|s| {
                let s = s as usize;
                if s >= images.len() {
                    return None;
                }
                if images[s].is_none() {
                    images[s] = Some(g.image(s, fetch, notes));
                }
                images[s].clone().flatten()
            });
        // Shine from how metallic and how smooth it is.
        let spec = (0.04 + 0.6 * metallic * (1.0 - rough)) as f32;
        let name = m.get("name").and_then(|n| n.as_str()).map_or_else(|| format!("material {}", i + 1), str::to_string);
        materials.push(Material {
            name,
            diffuse: [linear_to_srgb(factor[0]), linear_to_srgb(factor[1]), linear_to_srgb(factor[2])],
            specular: [spec; 3],
            shininess: (2.0 / (rough.powi(4) + 0.002) - 2.0).clamp(2.0, 600.0) as f32,
            opacity: opacity.clamp(0.0, 1.0) as f32,
            texture,
            emission: {
                let e: Vec<f64> =
                    m.get("emissiveFactor").and_then(|f| f.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default();
                let k = m
                    .get("extensions")
                    .and_then(|x| x.get("KHR_materials_emissive_strength"))
                    .and_then(|x| x.get("emissiveStrength"))
                    .and_then(|x| x.as_f64())
                    .unwrap_or(1.0) as f32;
                if e.len() >= 3 {
                    [linear_to_srgb(e[0]) * k, linear_to_srgb(e[1]) * k, linear_to_srgb(e[2]) * k]
                } else {
                    [0.0; 3]
                }
            },
        });
    }
    let default_material = materials.len() as u32;
    materials.push(Material { name: "default".into(), ..Material::default() });
    uv_ways.push(UvWay::default());

    // The nodes of the scene, each with the transform from it to the scene.
    let roots: Vec<usize> = match g.json.get("scene").and_then(|s| s.as_u64()).or(Some(0)).and_then(|s| g.arr("scenes").get(s as usize)) {
        Some(sc) => sc.get("nodes").and_then(|n| n.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|v| v as usize)).collect()).unwrap_or_default(),
        None => {
            // Without scenes, the nodes no other node holds.
            let held: std::collections::HashSet<usize> = g
                .arr("nodes")
                .iter()
                .flat_map(|n| n.get("children").and_then(|c| c.as_array()).into_iter().flatten().filter_map(|c| c.as_u64().map(|v| v as usize)))
                .collect();
            (0..g.arr("nodes").len()).filter(|i| !held.contains(i)).collect()
        }
    };
    let mut placed: Vec<(usize, M4)> = Vec::new();
    let mut stack: Vec<(usize, M4, usize)> = roots.iter().rev().map(|r| (*r, IDENTITY, 0)).collect();
    while let Some((n, parent, depth)) = stack.pop() {
        let Some(node) = g.arr("nodes").get(n) else { continue };
        if depth > 64 {
            continue;
        }
        let m = mul(&parent, &local_matrix(node));
        if node.get("mesh").is_some() {
            placed.push((n, m));
        }
        for c in node.get("children").and_then(|c| c.as_array()).into_iter().flatten().rev().filter_map(|c| c.as_u64()) {
            stack.push((c as usize, m, depth + 1));
        }
    }
    if placed.is_empty() {
        return err("There are no meshes in the scene of the glTF model.");
    }
    let (mut positions, mut normals, mut uvs, mut tris) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut objects = Vec::new();
    let mut faces_of: Vec<Vec<([u32; 3], u32)>> = Vec::new();
    let mut batch = Vec::new();
    let mut skipped = 0usize;
    let mut table_used: Option<usize> = None;
    let mut textured_ids = false;
    for (n, m) in placed {
        let node = &g.arr("nodes")[n];
        let mi = node.get("mesh").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let Some(mesh) = g.arr("meshes").get(mi) else { continue };
        let name = node
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| mesh.get("name").and_then(|v| v.as_str()))
            .map_or_else(|| format!("mesh {}", objects.len() + 1), str::to_string);
        let object = objects.len() as u32;
        objects.push(name);
        let mut faces = Vec::new();
        for prim in mesh.get("primitives").and_then(|p| p.as_array()).into_iter().flatten() {
            let mode = prim.get("mode").and_then(|v| v.as_u64()).unwrap_or(4);
            if !matches!(mode, 4..=6) {
                skipped += 1;
                continue;
            }
            let attrs = prim.get("attributes");
            let acc = |k: &str| attrs.and_then(|a| a.get(k)).and_then(|v| v.as_u64()).map(|v| v as usize);
            let Some((pos, 3)) = acc("POSITION").and_then(|a| g.floats(a)) else {
                skipped += 1;
                continue;
            };
            let count = pos.len() / 3;
            let base = positions.len() as u32;
            for k in 0..count {
                positions.push(apply(&m, [pos[k * 3] as f64, pos[k * 3 + 1] as f64, pos[k * 3 + 2] as f64]));
            }
            let nbase = normals.len() as u32;
            let has_n = match acc("NORMAL").and_then(|a| g.floats(a)) {
                Some((nr, 3)) if nr.len() == count * 3 => {
                    for k in 0..count {
                        normals.push(apply_normal(&m, [nr[k * 3], nr[k * 3 + 1], nr[k * 3 + 2]]));
                    }
                    true
                }
                _ => false,
            };
            // The features: _BATCHID in 3D Tiles 1.0; in 1.1 the first set of feature ids of
            // EXT_mesh_features, from an attribute or, without one, the vertex itself.
            let set = prim
                .get("extensions")
                .and_then(|e| e.get("EXT_mesh_features"))
                .and_then(|e| e.get("featureIds"))
                .and_then(|f| f.as_array())
                .and_then(|f| f.first());
            if let Some(t) = set.and_then(|s| s.get("propertyTable")).and_then(|t| t.as_u64()) {
                table_used.get_or_insert(t as usize);
            }
            let ids = match set {
                Some(s) if s.get("attribute").is_some() => s.get("attribute").and_then(|a| a.as_u64()).and_then(|a| acc(&format!("_FEATURE_ID_{}", a))),
                Some(s) if s.get("texture").is_some() => {
                    textured_ids = true;
                    None
                }
                Some(_) => None,
                None => acc("_BATCHID").or_else(|| acc("_FEATURE_ID_0")),
            }
            .and_then(|a| g.floats(a))
            .filter(|(v, n)| *n == 1 && v.len() == count)
            .map(|(v, _)| v)
            .or_else(|| set.filter(|s| s.get("attribute").is_none() && s.get("texture").is_none()).map(|_| (0..count).map(|i| i as f32).collect()));
            let material = prim.get("material").and_then(|v| v.as_u64()).map_or(default_material, |v| (v as u32).min(default_material));
            let way = &uv_ways[material as usize];
            let tbase = uvs.len() as u32;
            let has_t = match acc(&format!("TEXCOORD_{}", way.set)).and_then(|a| g.floats(a)) {
                Some((t, 2)) if t.len() == count * 2 => {
                    // glTF counts v down from the top of the picture; OBJ, as kept here, up.
                    for k in 0..count {
                        let (u, v) = way.apply(t[k * 2], t[k * 2 + 1]);
                        uvs.push([u, 1.0 - v]);
                    }
                    true
                }
                _ => false,
            };
            let idx: Vec<u32> = match prim.get("indices").and_then(|v| v.as_u64()) {
                Some(a) => g.indices(a as usize).ok_or_else(|| GbError("The indices of a mesh in the glTF file cannot be read.".into()))?,
                None => (0..count as u32).collect(),
            };
            let triangles: Vec<[u32; 3]> = match mode {
                5 => (2..idx.len()).map(|i| if i % 2 == 0 { [idx[i - 2], idx[i - 1], idx[i]] } else { [idx[i - 1], idx[i - 2], idx[i]] }).collect(),
                6 => (2..idx.len()).map(|i| [idx[0], idx[i - 1], idx[i]]).collect(),
                _ => idx.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            };
            for t in triangles {
                if t.iter().any(|i| *i as usize >= count) {
                    continue;
                }
                tris.push(Tri {
                    v: t.map(|i| base + i),
                    n: if has_n { t.map(|i| nbase + i) } else { [NONE; 3] },
                    t: if has_t { t.map(|i| tbase + i) } else { [NONE; 3] },
                    material,
                    object,
                });
                batch.push(ids.as_ref().map_or(NONE, |v| v[t[0] as usize].max(0.0) as u32));
                faces.push((t.map(|i| base + i), material));
            }
        }
        faces_of.push(faces);
    }
    if skipped > 0 {
        notes.warn(format!("{} parts of meshes are points or lines, or have no positions, and were left out.", skipped));
    }
    if tris.is_empty() {
        return err("The glTF model has no triangles.");
    }
    if textured_ids {
        notes.warn("Some features are told apart by a texture (EXT_mesh_features), which is not read: they are one object each mesh.");
    }
    // A table of one feature that no feature ids point to, as a tile of one building has
    // after 3d-tiles-tools upgrade: the whole model is that feature.
    let lone = || {
        let tables = g.json.get("extensions")?.get("EXT_structural_metadata")?.get("propertyTables")?.as_array()?;
        (tables.len() == 1 && tables[0].get("count").and_then(|c| c.as_u64()) == Some(1)).then_some(0)
    };
    let (properties, feature_count) = match table_used.or_else(lone).and_then(|t| property_table(&g, t)) {
        Some((p, n)) => (Some(p), n),
        None => (None, 0),
    };
    Ok(Model { mesh: Mesh { positions, normals, uvs, tris, materials, objects }, faces: faces_of, batch, rtc, properties, feature_count, origin })
}

/// The faces of each object with their corners, as `model_table` wants them.
pub(crate) fn corners(mesh: &Mesh, faces: &[Vec<([u32; 3], u32)>]) -> Vec<Vec<(Vec<[f64; 3]>, u32)>> {
    faces.iter().map(|f| f.iter().map(|(t, m)| (t.iter().map(|i| mesh.positions[*i as usize]).collect(), *m)).collect()).collect()
}

/// Reads a glTF file, .gltf or .glb, and places it.
pub fn read_gltf(path: &Path, opts: &ObjOpts, notes: &mut Notes) -> Result<Table> {
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut model = parse(&bytes, &|uri| fetch(uri, dir), notes).map_err(|e| GbError(format!("{}: {}", path.display(), e.0)))?;
    // A model Ographis wrote goes back where it was, unless the input places it.
    let mut opts = opts.clone();
    if let Some((o, crs)) = model.origin.filter(|_| opts.origin == [0.0; 3] && opts.rotation == 0.0) {
        opts.origin = o;
        opts.crs = opts.crs.or(crs);
        notes.info(format!("placed at {}, {}, {}, as Ographis wrote it", o[0], o[1], o[2]));
    }
    let opts = &opts;
    // The unit: metres, as glTF has them, unless the input says otherwise.
    let at = placement(opts, opts.unit.unwrap_or(1.0));
    for p in &mut model.mesh.positions {
        *p = at.place(*p);
    }
    for n in &mut model.mesh.normals {
        *n = at.turn(*n);
    }
    let parts = corners(&model.mesh, &model.faces);
    Ok(model_table(model.mesh, parts, "triangles", opts.crs, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_coordinates_are_scaled_turned_and_moved() {
        let info: Json = serde_json::from_str(
            r#"{"index":0,"texCoord":0,"extensions":{"KHR_texture_transform":{"offset":[0.5,-3],"rotation":1.5707964,"scale":[7,2],"texCoord":1}}}"#,
        )
        .unwrap();
        let way = uv_way(Some(&info));
        assert_eq!(way.set, 1);
        let (u, v) = way.apply(1.0, 0.0);
        assert!((u - 0.5).abs() < 1e-5 && (v - (-7.0 - 3.0)).abs() < 1e-5, "{u} {v}");
        let (u, v) = way.apply(0.0, 1.0);
        assert!((u - 2.5).abs() < 1e-5 && (v + 3.0).abs() < 1e-5, "{u} {v}");
        let plain = uv_way(None);
        assert_eq!((plain.set, plain.apply(0.25, 0.75)), (0, (0.25, 0.75)));
    }

    /// A glTF with one triangle, in a node turned a quarter around y and moved, with its
    /// buffer as a data address.
    #[test]
    fn a_small_gltf_is_read_through_its_nodes() {
        let mut bin = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in bin.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                for k in 0..4 {
                    if k <= c.len() {
                        s.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
                    } else {
                        s.push('=');
                    }
                }
            }
            s
        };
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"name": "Parent", "translation": [10.0, 0.0, 0.0], "children": [1]}, {"name": "Wall", "mesh": 0, "rotation": [0.0, half, 0.0, half]}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}],
            "materials": [{"name": "Red", "pbrMetallicRoughness": {"baseColorFactor": [1.0, 0.0, 0.0, 1.0]}}],
            "buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{}", b64)}],
            "bufferViews": [{"buffer": 0, "byteLength": 36}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}]
        });
        let dir = std::env::temp_dir().join(format!("gb-gltf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t.gltf"), json.to_string()).unwrap();
        let mut notes = Notes::default();
        let t = read_gltf(&dir.join("t.gltf"), &ObjOpts { y_up: true, ..Default::default() }, &mut notes).unwrap();
        let m = t.mesh.as_ref().unwrap();
        assert_eq!((m.tris.len(), t.len(), m.objects[0].as_str()), (1, 1, "Wall"));
        // The point (2, 0, 0) of the wall, turned a quarter around y, is (0, 0, -2), moved
        // to (10, 0, -2); on the map, with z of glTF towards the south, 2 m north.
        let p = m.positions[1];
        assert!((p[0] - 10.0).abs() < 1e-9 && (p[1] - 2.0).abs() < 1e-9 && p[2].abs() < 1e-9, "{:?}", p);
        // (0, 3, 0) is 3 m up.
        assert!((m.positions[2][2] - 3.0).abs() < 1e-9);
        assert_eq!(m.materials[0].diffuse, [1.0, 0.0, 0.0]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
