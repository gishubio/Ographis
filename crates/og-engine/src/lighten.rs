//! Lighter 3D models for realtime: fewer triangles where they show little, by meshoptimizer,
//! textures no larger than they need to be, and the materials put together into one texture,
//! an atlas, so that a model is drawn in few calls.

use crate::mesh::{Material, Mesh, Texture, Tri, NONE};
use crate::model::{GbError, Result};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// How a model is made lighter.
#[derive(Debug, Clone, Copy)]
pub struct Simplify {
    /// At most this many triangles in all. None: as few as `error` allows.
    pub most: Option<usize>,
    /// How far, in metres, the surface may move. None: as far as it must to reach `most`.
    pub error: Option<f64>,
    /// Keep the edges of each object and material where they are, so that pieces still meet.
    pub keep_edges: bool,
}

/// What simplification did: triangles before and after, and the largest change in metres.
#[derive(Debug, Clone, Copy, Default)]
pub struct Simplified {
    pub before: usize,
    pub after: usize,
    pub error: f64,
}

/// A corner as it is joined: where it lies, to a millimetre, its texture coordinates and its
/// normal, as bits.
type Corner = ([u64; 3], [u32; 2], [u32; 3]);

/// Fewer triangles: each object and material by itself, its corners joined where they lie
/// together and have the same texture coordinates and normal, then simplified by
/// meshoptimizer to its share of the triangles. Small pieces, of 12 triangles or fewer such as
/// boxes, are kept as they are.
pub fn simplify(mesh: &Mesh, s: &Simplify) -> (Mesh, Simplified) {
    let before = mesh.tris.len();
    let mut groups: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (i, t) in mesh.tris.iter().enumerate() {
        groups.entry((t.object, t.material)).or_default().push(i);
    }
    let share = s.most.map(|m| m as f64 / before.max(1) as f64).unwrap_or(0.0).min(1.0);
    let mut out = Mesh { materials: mesh.materials.clone(), objects: mesh.objects.clone(), ..Default::default() };
    let mut largest = 0f64;
    for ((object, material), tris) in groups {
        if tris.len() <= 12 || (s.most.is_some() && share >= 1.0) {
            append(&mut out, mesh, &tris);
            continue;
        }
        // The corners of the piece, joined where they are the same.
        let mut key_of: HashMap<Corner, u32> = HashMap::new();
        let mut corners: Vec<(u32, u32, u32)> = Vec::new();
        let mut indices: Vec<u32> = Vec::with_capacity(tris.len() * 3);
        for &i in &tris {
            let t = &mesh.tris[i];
            for k in 0..3 {
                let p = mesh.positions[t.v[k] as usize];
                let uv = if t.t[k] != NONE { mesh.uvs.get(t.t[k] as usize).copied().unwrap_or([0.0; 2]) } else { [f32::NAN; 2] };
                let n = if t.n[k] != NONE { mesh.normals.get(t.n[k] as usize).copied().unwrap_or([0.0; 3]) } else { [f32::NAN; 3] };
                // Within a millimetre is the same place.
                let q = p.map(|x| (x * 1000.0).round() as i64 as u64);
                let key = (q, uv.map(f32::to_bits), n.map(f32::to_bits));
                let id = *key_of.entry(key).or_insert_with(|| {
                    corners.push((t.v[k], t.t[k], t.n[k]));
                    (corners.len() - 1) as u32
                });
                indices.push(id);
            }
        }
        // Positions near the middle, as meshoptimizer takes them in 32 bits.
        let first = mesh.positions[corners[0].0 as usize];
        let local: Vec<[f32; 3]> = corners
            .iter()
            .map(|c| {
                let p = mesh.positions[c.0 as usize];
                [(p[0] - first[0]) as f32, (p[1] - first[1]) as f32, (p[2] - first[2]) as f32]
            })
            .collect();
        let bytes = meshopt::typed_to_bytes(&local);
        let Ok(adapter) = meshopt::VertexDataAdapter::new(bytes, 12, 0) else {
            append(&mut out, mesh, &tris);
            continue;
        };
        let target = match s.most {
            Some(_) => ((tris.len() as f64 * share).round() as usize).max(4) * 3,
            None => 0,
        };
        let limit = s.error.map_or(f32::MAX, |e| e as f32);
        let mut options = meshopt::SimplifyOptions::ErrorAbsolute;
        if s.keep_edges {
            options |= meshopt::SimplifyOptions::LockBorder;
        }
        let mut err = 0f32;
        let kept = meshopt::simplify(&indices, &adapter, target, limit, options, Some(&mut err));
        largest = largest.max(err as f64);
        // The triangles that are left, with their own corners.
        let base_v = out.positions.len() as u32;
        let base_t = out.uvs.len() as u32;
        let base_n = out.normals.len() as u32;
        let mut used: HashMap<u32, u32> = HashMap::new();
        let mut order: Vec<u32> = Vec::new();
        for id in &kept {
            used.entry(*id).or_insert_with(|| {
                order.push(*id);
                (order.len() - 1) as u32
            });
        }
        for id in &order {
            let (v, t, n) = corners[*id as usize];
            out.positions.push(mesh.positions[v as usize]);
            out.uvs.push(if t != NONE { mesh.uvs.get(t as usize).copied().unwrap_or([0.0; 2]) } else { [0.0; 2] });
            out.normals.push(if n != NONE { mesh.normals.get(n as usize).copied().unwrap_or([0.0, 0.0, 1.0]) } else { [0.0, 0.0, 1.0] });
        }
        let textured = corners.first().is_some_and(|c| c.1 != NONE);
        let smooth = corners.first().is_some_and(|c| c.2 != NONE);
        for tri in kept.chunks_exact(3) {
            let local = [used[&tri[0]], used[&tri[1]], used[&tri[2]]];
            out.tris.push(Tri {
                v: local.map(|i| base_v + i),
                t: if textured { local.map(|i| base_t + i) } else { [NONE; 3] },
                n: if smooth { local.map(|i| base_n + i) } else { [NONE; 3] },
                material,
                object,
            });
        }
    }
    let after = out.tris.len();
    (compact(&out), Simplified { before, after, error: largest })
}

/// Copies triangles as they are.
fn append(out: &mut Mesh, mesh: &Mesh, tris: &[usize]) {
    for &i in tris {
        let t = mesh.tris[i];
        let v = t.v.map(|k| {
            out.positions.push(mesh.positions[k as usize]);
            (out.positions.len() - 1) as u32
        });
        let tx = t.t.map(|k| {
            if k == NONE || k as usize >= mesh.uvs.len() {
                return NONE;
            }
            out.uvs.push(mesh.uvs[k as usize]);
            (out.uvs.len() - 1) as u32
        });
        let n = t.n.map(|k| {
            if k == NONE || k as usize >= mesh.normals.len() {
                return NONE;
            }
            out.normals.push(mesh.normals[k as usize]);
            (out.normals.len() - 1) as u32
        });
        out.tris.push(Tri { v, t: tx, n, ..t });
    }
}

/// The model with only the corners its triangles use.
fn compact(m: &Mesh) -> Mesh {
    let rows: Vec<usize> = (0..m.objects.len()).collect();
    if m.objects.is_empty() || m.tris.iter().any(|t| t.object as usize >= m.objects.len()) {
        return m.clone();
    }
    m.pick(&rows)
}

/// A texture made smaller, by the average of the pixels it covers, so that its larger side is
/// at most `most` pixels.
pub fn shrink(t: &Texture, most: usize) -> Option<Texture> {
    let side = t.width.max(t.height);
    if side <= most || most == 0 {
        return None;
    }
    let f = most as f64 / side as f64;
    let (w, h) = (((t.width as f64 * f).round() as usize).max(1), ((t.height as f64 * f).round() as usize).max(1));
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let (y0, y1) = (y * t.height / h, ((y + 1) * t.height / h).max(y * t.height / h + 1));
        for x in 0..w {
            let (x0, x1) = (x * t.width / w, ((x + 1) * t.width / w).max(x * t.width / w + 1));
            let mut sum = [0u32; 4];
            for yy in y0..y1.min(t.height) {
                for xx in x0..x1.min(t.width) {
                    let i = (yy * t.width + xx) * 4;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += t.rgba[i + c] as u32;
                    }
                }
            }
            let n = ((y1.min(t.height) - y0) * (x1.min(t.width) - x0)).max(1) as u32;
            let o = (y * w + x) * 4;
            for c in 0..4 {
                rgba[o + c] = (sum[c] / n) as u8;
            }
        }
    }
    Some(Texture { width: w, height: h, rgba })
}

/// Textures larger than `most` pixels made smaller. Gives how many.
pub fn shrink_textures(m: &mut Mesh, most: usize) -> usize {
    let mut n = 0;
    for mat in &mut m.materials {
        if let Some(t) = mat.texture.as_ref().and_then(|t| shrink(t, most)) {
            mat.texture = Some(Arc::new(t));
            n += 1;
        }
    }
    n
}

/// What putting the materials together did.
#[derive(Debug, Clone, Copy, Default)]
pub struct Atlased {
    /// Materials before, and after.
    pub before: usize,
    pub after: usize,
    /// Of them, textures that repeat and so are kept apart.
    pub repeating: usize,
    pub width: usize,
    pub height: usize,
}

/// Puts the materials of a model into one texture, an atlas, as far as they go: plain colours
/// as small squares of their colour, and textures whose coordinates stay within them. Textures
/// that repeat, such as facades, and clear or glowing materials keep their own. The atlas is
/// at most `most` pixels on a side, the textures made smaller to fit when they must.
pub fn atlas(m: &Mesh, most: usize) -> Result<(Mesh, Atlased)> {
    let most = most.clamp(64, 8192);
    let n = m.materials.len();
    // Which materials go in: used, opaque, not glowing, and with coordinates within 0 and 1.
    let mut used = vec![false; n];
    let mut inside = vec![true; n];
    for t in &m.tris {
        let k = t.material as usize;
        if k >= n {
            continue;
        }
        used[k] = true;
        if m.materials[k].texture.is_some() {
            for c in t.t {
                let uv = if c == NONE { None } else { m.uvs.get(c as usize) };
                if !uv.is_some_and(|uv| uv.iter().all(|x| (-0.001..=1.001).contains(x))) {
                    inside[k] = false;
                }
            }
        }
    }
    let goes: Vec<bool> = (0..n)
        .map(|k| used[k] && inside[k] && m.materials[k].opacity >= 0.999 && m.materials[k].emission == [0.0; 3] && m.materials[k].specular == [0.0; 3])
        .collect();
    let repeating = (0..n).filter(|k| used[*k] && m.materials[*k].texture.is_some() && !inside[*k]).count();
    let in_atlas = goes.iter().filter(|g| **g).count();
    if in_atlas < 2 {
        return Ok((m.clone(), Atlased { before: n, after: n, repeating, width: 0, height: 0 }));
    }
    // The pieces: a texture with a border of 2 pixels, or a square of 4 pixels of a colour.
    const PAD: usize = 2;
    let pieces = |scale: f64| -> Vec<(usize, usize, usize)> {
        (0..n)
            .filter(|k| goes[*k])
            .map(|k| match &m.materials[k].texture {
                Some(t) => (k, ((t.width as f64 * scale).ceil() as usize).max(1) + 2 * PAD, ((t.height as f64 * scale).ceil() as usize).max(1) + 2 * PAD),
                None => (k, 4, 4),
            })
            .collect()
    };
    // Shelves, the tallest first, the textures made smaller until they fit.
    let mut scale = 1.0;
    let (places, width, height) = loop {
        let mut ps = pieces(scale);
        ps.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));
        let area: usize = ps.iter().map(|p| p.1 * p.2).sum();
        let width = ((area as f64).sqrt().ceil() as usize).next_power_of_two().clamp(16, most).max(ps.iter().map(|p| p.1).max().unwrap_or(4).min(most));
        let (mut x, mut y, mut shelf) = (0usize, 0usize, 0usize);
        let mut places: HashMap<usize, (usize, usize, usize, usize)> = HashMap::new();
        let mut fits = true;
        for (k, w, h) in &ps {
            if *w > width {
                fits = false;
                break;
            }
            if x + w > width {
                x = 0;
                y += shelf;
                shelf = 0;
            }
            places.insert(*k, (x, y, *w, *h));
            x += w;
            shelf = shelf.max(*h);
        }
        let height = (y + shelf).next_power_of_two();
        if fits && height <= most {
            break (places, width, height);
        }
        scale *= 0.8;
        if scale < 0.01 {
            return Err(GbError("The materials do not fit into one texture.".into()));
        }
    };
    let mut rgba = vec![255u8; width * height * 4];
    for (k, (x0, y0, w, h)) in &places {
        let mat = &m.materials[*k];
        match &mat.texture {
            Some(t) => {
                let (iw, ih) = (w - 2 * PAD, h - 2 * PAD);
                for y in 0..*h {
                    for x in 0..*w {
                        // The border repeats the edge of the texture.
                        let sx = ((x as isize - PAD as isize).clamp(0, iw as isize - 1) as usize * t.width / iw).min(t.width - 1);
                        let sy = ((y as isize - PAD as isize).clamp(0, ih as isize - 1) as usize * t.height / ih).min(t.height - 1);
                        let s = (sy * t.width + sx) * 4;
                        let d = ((y0 + y) * width + x0 + x) * 4;
                        rgba[d..d + 4].copy_from_slice(&t.rgba[s..s + 4]);
                    }
                }
            }
            None => {
                let c = mat.diffuse.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                for y in 0..*h {
                    for x in 0..*w {
                        let d = ((y0 + y) * width + x0 + x) * 4;
                        rgba[d..d + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                    }
                }
            }
        }
    }
    // The materials that are left, and the atlas after them.
    let mut new_index = vec![0u32; n];
    let mut materials: Vec<Material> = Vec::new();
    for k in 0..n {
        if !goes[k] {
            new_index[k] = materials.len() as u32;
            materials.push(m.materials[k].clone());
        }
    }
    let atlas_k = materials.len() as u32;
    materials.push(Material { name: "atlas".into(), diffuse: [1.0; 3], texture: Some(Arc::new(Texture { width, height, rgba })), ..Default::default() });
    let mut out =
        Mesh { positions: m.positions.clone(), normals: m.normals.clone(), uvs: m.uvs.clone(), materials, objects: m.objects.clone(), tris: Vec::new() };
    out.uvs.clear();
    let (fw, fh) = (width as f32, height as f32);
    for t in &m.tris {
        let k = t.material as usize;
        if k < n && goes[k] {
            let (x0, y0, w, h) = places[&k];
            let tx = match &m.materials[k].texture {
                Some(_) => t.t.map(|c| {
                    let uv = m.uvs[c as usize];
                    let (iw, ih) = ((w - 2 * PAD) as f32, (h - 2 * PAD) as f32);
                    let u = (x0 + PAD) as f32 + uv[0].clamp(0.0, 1.0) * iw;
                    let row = (y0 + PAD) as f32 + (1.0 - uv[1].clamp(0.0, 1.0)) * ih;
                    out.uvs.push([u / fw, 1.0 - row / fh]);
                    (out.uvs.len() - 1) as u32
                }),
                None => {
                    // The middle of its square.
                    out.uvs.push([(x0 as f32 + 2.0) / fw, 1.0 - (y0 as f32 + 2.0) / fh]);
                    [(out.uvs.len() - 1) as u32; 3]
                }
            };
            out.tris.push(Tri { t: tx, material: atlas_k, ..*t });
        } else {
            let tx = t.t.map(|c| {
                if c == NONE || c as usize >= m.uvs.len() {
                    return NONE;
                }
                out.uvs.push(m.uvs[c as usize]);
                (out.uvs.len() - 1) as u32
            });
            out.tris.push(Tri { t: tx, material: new_index.get(k).copied().unwrap_or(0), ..*t });
        }
    }
    let after = out.materials.len();
    Ok((out, Atlased { before: n, after, repeating, width, height }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wavy surface of 100 × 100 squares, of 1 m, almost flat.
    fn surface(n: usize) -> Mesh {
        let mut m = Mesh { materials: vec![Material::default()], objects: vec!["ground".into()], ..Default::default() };
        for y in 0..=n {
            for x in 0..=n {
                m.positions.push([x as f64, y as f64, ((x as f64) * 0.05).sin() * 0.02]);
            }
        }
        let at = |x: usize, y: usize| (y * (n + 1) + x) as u32;
        for y in 0..n {
            for x in 0..n {
                for v in [[at(x, y), at(x + 1, y), at(x + 1, y + 1)], [at(x, y), at(x + 1, y + 1), at(x, y + 1)]] {
                    m.tris.push(Tri { v, n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 });
                }
            }
        }
        m
    }

    #[test]
    fn a_surface_gets_fewer_triangles_and_keeps_its_edges() {
        let m = surface(100);
        let (light, s) = simplify(&m, &Simplify { most: Some(2000), error: None, keep_edges: true });
        assert_eq!(s.before, 20_000);
        assert!(s.after <= 2000 && s.after > 200, "{:?}", s);
        // The edges stay: the corners are still there, and the outline is as long.
        let (lo, hi) = light.bounds().unwrap();
        assert_eq!((lo[0], lo[1], hi[0], hi[1]), (0.0, 0.0, 100.0, 100.0));
        // By the error alone: an almost flat surface becomes very few triangles.
        let (_, s2) = simplify(&m, &Simplify { most: None, error: Some(0.05), keep_edges: false });
        assert!(s2.after < 100 && s2.error <= 0.05, "{:?}", s2);
        // Small pieces stay as they are.
        let mut tiny = surface(2);
        tiny.tris.truncate(6);
        let (same, s3) = simplify(&tiny, &Simplify { most: Some(1), error: None, keep_edges: true });
        assert_eq!((s3.after, same.tris.len()), (6, 6));
    }

    #[test]
    fn materials_go_into_one_texture() {
        let mut m = surface(2);
        m.materials = vec![
            Material { name: "red".into(), diffuse: [1.0, 0.0, 0.0], ..Default::default() },
            Material { name: "blue".into(), diffuse: [0.0, 0.0, 1.0], ..Default::default() },
            Material {
                name: "picture".into(),
                texture: Some(Arc::new(Texture { width: 8, height: 8, rgba: [0, 255, 0, 255].repeat(64) })),
                ..Default::default()
            },
            Material { name: "glass".into(), opacity: 0.4, ..Default::default() },
        ];
        m.uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        for (i, t) in m.tris.iter_mut().enumerate() {
            t.material = (i % 4) as u32;
            if t.material == 2 {
                t.t = [0, 1, 2];
            }
        }
        let (out, a) = atlas(&m, 1024).unwrap();
        assert_eq!((a.before, a.after, a.repeating), (4, 2, 0));
        let tex = out.materials[1].texture.as_ref().unwrap();
        // Each triangle shows the colour it had.
        for (i, t) in out.tris.iter().enumerate() {
            let want = [[255, 0, 0], [0, 0, 255], [0, 255, 0]];
            if i % 4 == 3 {
                assert_eq!(t.material, 0);
                continue;
            }
            for c in t.t {
                let uv = out.uvs[c as usize];
                let (u, v) = (uv[0], uv[1]);
                let x = ((u * tex.width as f32) as usize).min(tex.width - 1);
                let y = (((1.0 - v) * tex.height as f32) as usize).min(tex.height - 1);
                let p = &tex.rgba[(y * tex.width + x) * 4..][..3];
                assert_eq!(p, want[i % 4], "triangle {}", i);
            }
        }
        // A texture larger than allowed is made smaller.
        let big = Texture { width: 4096, height: 2048, rgba: vec![100; 4096 * 2048 * 4] };
        let small = shrink(&big, 1024).unwrap();
        assert_eq!((small.width, small.height, small.rgba[0]), (1024, 512, 100));
    }
}
