//! 3D models made of triangles with materials, such as buildings read from OBJ files, for
//! the 3D view and for rendering. A stream carries a model beside its features: the
//! features are its objects, for GIS, and the model keeps what a picture needs, such as
//! normals, texture coordinates and the pictures of the textures.

use std::sync::Arc;

/// A picture laid on a surface, in RGBA, row by row from the top, as sRGB.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Texture {
    /// The colour at texture coordinates u and v, from 0 to 1 and repeated beyond, with v
    /// upwards as in OBJ, between the four nearest pixels. As sRGB from 0 to 1, and opacity.
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.width as f32, self.height as f32);
        let x = (u - u.floor()) * w - 0.5;
        let y = (1.0 - (v - v.floor())) * h - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |xi: f32, yi: f32| -> [f32; 4] {
            let xi = (xi as i64).rem_euclid(self.width as i64) as usize;
            let yi = (yi as i64).rem_euclid(self.height as i64) as usize;
            let p = &self.rgba[(yi * self.width + xi) * 4..][..4];
            [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]
        };
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        [0, 1, 2, 3].map(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
    }

    /// The average colour, as sRGB from 0 to 1, for drawing where a texture is too small to
    /// see.
    pub fn average(&self) -> [f32; 3] {
        let n = (self.width * self.height).max(1);
        let step = (n / 4096).max(1);
        let (mut s, mut k) = ([0f64; 3], 0usize);
        for i in (0..n).step_by(step) {
            for (c, sum) in s.iter_mut().enumerate() {
                *sum += self.rgba[i * 4 + c] as f64;
            }
            k += 1;
        }
        s.map(|v| (v / k.max(1) as f64 / 255.0) as f32)
    }
}

/// How a surface looks.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// The colour, as sRGB from 0 to 1.
    pub diffuse: [f32; 3],
    /// The colour of highlights, and how sharp they are.
    pub specular: [f32; 3],
    pub shininess: f32,
    /// 1 is solid, 0 is clear, as glass.
    pub opacity: f32,
    pub texture: Option<Arc<Texture>>,
    /// The light the surface gives off itself, as sRGB from 0 to 1 and above for brighter,
    /// such as a lamp or a sign. Black for most.
    pub emission: [f32; 3],
}

impl Default for Material {
    fn default() -> Self {
        Material { name: String::new(), diffuse: [0.8, 0.8, 0.8], specular: [0.0; 3], shininess: 10.0, opacity: 1.0, texture: None, emission: [0.0; 3] }
    }
}

impl Material {
    /// The colour the material shows from afar: its texture's average, or its own.
    pub fn colour(&self) -> [f32; 3] {
        match &self.texture {
            Some(t) => {
                let a = t.average();
                [a[0] * self.diffuse[0], a[1] * self.diffuse[1], a[2] * self.diffuse[2]]
            }
            None => self.diffuse,
        }
    }
}

/// A triangle: its corners, their normals and texture coordinates (u32::MAX for none), its
/// material and the object it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tri {
    pub v: [u32; 3],
    pub n: [u32; 3],
    pub t: [u32; 3],
    pub material: u32,
    pub object: u32,
}

pub const NONE: u32 = u32::MAX;

/// A model of triangles in the coordinates of the stream: x east, y north, z up, in metres
/// when the stream has a coordinate system in metres.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tris: Vec<Tri>,
    pub materials: Vec<Material>,
    /// The names of the objects, in the order the features of the stream have them.
    pub objects: Vec<String>,
}

impl Mesh {
    /// The objects of these rows, in this order: the model of a stream that keeps some of the
    /// features of another, as a filter does. The corners that are left are kept.
    pub fn pick(&self, rows: &[usize]) -> Mesh {
        let mut object = vec![NONE; self.objects.len()];
        for (k, r) in rows.iter().enumerate() {
            if let Some(o) = object.get_mut(*r) {
                *o = k as u32;
            }
        }
        let (mut pos, mut nor, mut uv) = (vec![NONE; self.positions.len()], vec![NONE; self.normals.len()], vec![NONE; self.uvs.len()]);
        let mut out = Mesh {
            materials: self.materials.clone(),
            objects: rows.iter().map(|r| self.objects.get(*r).cloned().unwrap_or_default()).collect(),
            ..Default::default()
        };
        for t in &self.tris {
            let Some(&o) = object.get(t.object as usize) else { continue };
            if o == NONE {
                continue;
            }
            let take = |i: u32, map: &mut Vec<u32>, from: &[[f64; 3]], to: &mut Vec<[f64; 3]>| -> u32 {
                if map[i as usize] == NONE {
                    map[i as usize] = to.len() as u32;
                    to.push(from[i as usize]);
                }
                map[i as usize]
            };
            let v = t.v.map(|i| take(i, &mut pos, &self.positions, &mut out.positions));
            let n = t.n.map(|i| {
                if i == NONE || i as usize >= nor.len() {
                    return NONE;
                }
                if nor[i as usize] == NONE {
                    nor[i as usize] = out.normals.len() as u32;
                    out.normals.push(self.normals[i as usize]);
                }
                nor[i as usize]
            });
            let tx = t.t.map(|i| {
                if i == NONE || i as usize >= uv.len() {
                    return NONE;
                }
                if uv[i as usize] == NONE {
                    uv[i as usize] = out.uvs.len() as u32;
                    out.uvs.push(self.uvs[i as usize]);
                }
                uv[i as usize]
            });
            out.tris.push(Tri { v, n, t: tx, material: t.material, object: o });
        }
        out
    }

    /// The smallest and largest x, y and z.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut it = self.positions.iter();
        let first = *it.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in it {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        Some((lo, hi))
    }

    /// The models of several streams as one.
    pub fn merge(parts: &[&Mesh]) -> Mesh {
        let mut out = Mesh::default();
        for m in parts {
            let (vp, vn, vt, mm, oo) =
                (out.positions.len() as u32, out.normals.len() as u32, out.uvs.len() as u32, out.materials.len() as u32, out.objects.len() as u32);
            out.positions.extend_from_slice(&m.positions);
            out.normals.extend_from_slice(&m.normals);
            out.uvs.extend_from_slice(&m.uvs);
            out.materials.extend(m.materials.iter().cloned());
            out.objects.extend(m.objects.iter().cloned());
            let shift = |a: [u32; 3], by: u32| a.map(|i| if i == NONE { NONE } else { i + by });
            out.tris.extend(m.tris.iter().map(|t| Tri {
                v: shift(t.v, vp),
                n: shift(t.n, vn),
                t: shift(t.t, vt),
                material: t.material + mm,
                object: t.object + oo,
            }));
        }
        out
    }
}

/// The triangles of a polygon in 3D, by ear clipping in the plane it lies in, seen along
/// the axis it faces most. Indices into `pts`. A polygon that cannot be cut, such as one
/// that crosses itself, is cut as a fan.
pub fn triangulate(pts: &[[f64; 3]]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }
    // The normal by Newell's method, and the plane to look at it in.
    let mut nrm = [0.0f64; 3];
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        nrm[0] += (a[1] - b[1]) * (a[2] + b[2]);
        nrm[1] += (a[2] - b[2]) * (a[0] + b[0]);
        nrm[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let axis = (0..3).max_by(|a, b| nrm[*a].abs().total_cmp(&nrm[*b].abs())).unwrap_or(2);
    let (i0, i1) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let flip = nrm[axis] < 0.0;
    let p2: Vec<[f64; 2]> = pts.iter().map(|p| if flip { [p[i1], p[i0]] } else { [p[i0], p[i1]] }).collect();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    // A convex polygon, as most faces of buildings are, is a fan.
    let convex = (0..n).all(|i| cross(p2[i], p2[(i + 1) % n], p2[(i + 2) % n]) >= -1e-12);
    if convex {
        return (1..n - 1).map(|i| [0, i, i + 1]).collect();
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let same = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12;
    // The size of the polygon, for what counts as on a line.
    let span = p2.iter().fold(0.0f64, |m, p| m.max(p[0].abs()).max(p[1].abs())).max(1e-9);
    let eps = 1e-12 * span * span;
    let mut guard = 0;
    while idx.len() > 3 && guard < 4 * n * n {
        guard += 1;
        let m = idx.len();
        let mut cut = false;
        for k in 0..m {
            let (a, b, c) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            if cross(p2[a], p2[b], p2[c]) <= eps {
                continue;
            }
            // No other corner may lie inside the ear, or on its edges, where the outline
            // could cross it. Corners at the same place as one of its own, as where a hole
            // is joined to the outside by a bridge that goes there and back, do not count.
            let inside = idx.iter().any(|&q| {
                let p = p2[q];
                q != a && q != b && q != c && !same(p, p2[a]) && !same(p, p2[b]) && !same(p, p2[c]) && {
                    let (d1, d2, d3) = (cross(p2[a], p2[b], p), cross(p2[b], p2[c], p), cross(p2[c], p2[a], p));
                    d1 >= -eps && d2 >= -eps && d3 >= -eps
                }
            });
            if inside {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(k);
            cut = true;
            break;
        }
        if !cut {
            // No ear: a corner on a straight line, or doubled, is taken away without a
            // triangle, and the cutting goes on.
            let m = idx.len();
            let flat = (0..m).find(|&k| {
                let (a, b, c) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
                cross(p2[a], p2[b], p2[c]).abs() <= eps
            });
            match flat {
                Some(k) => {
                    idx.remove(k);
                }
                None => break,
            }
        }
    }
    if idx.len() == 3 {
        if cross(p2[idx[0]], p2[idx[1]], p2[idx[2]]).abs() > eps {
            out.push([idx[0], idx[1], idx[2]]);
        }
    } else if idx.len() > 3 {
        // What is left could not be cut: a fan of it.
        out.extend((1..idx.len() - 1).map(|i| [idx[0], idx[i], idx[i + 1]]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygons_are_cut_into_triangles_in_their_own_plane() {
        // A wall, standing up: seen from above it has no area.
        let wall = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 0.0, 3.0], [0.0, 0.0, 3.0]];
        assert_eq!(triangulate(&wall).len(), 2);
        // An L, which is not convex: four triangles, none outside it.
        let l = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0]];
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        let area: f64 = t
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (l[*a], l[*b], l[*c]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-9, "{}", area);
        // A wall of 10 × 4 with a window of 2 × 2, the hole joined to the outside by a bridge
        // that goes there and back: 36 square metres, none over the window.
        let w = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 0.0, 4.0],
            [0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0],
            [4.0, 0.0, 1.0],
            [4.0, 0.0, 3.0],
            [6.0, 0.0, 3.0],
            [6.0, 0.0, 1.0],
            [4.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ];
        let t = triangulate(&w);
        let area: f64 = t
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (w[*a], w[*b], w[*c]);
                ((b[0] - a[0]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum();
        assert!((area - 36.0).abs() < 1e-6, "{} from {:?}", area, t);
    }
}
