//! Clipping 3D models of triangles by areas seen from above, as Clip does for TINs,
//! surfaces and buildings. Each triangle that the boundary crosses is cut where the boundary
//! runs over it, and the pieces keep the plane of the triangle: their heights, normals and
//! texture coordinates are taken from its corners. Upright triangles, such as walls, are cut
//! by upright planes along the boundary. Triangles far from the boundary are not cut, and the
//! boundary is looked up in an R-tree and in a grid of tiles, so that large models against
//! long boundaries stay fast.

use crate::geom::Clipper;
use crate::mesh::{Mesh, Tri, NONE};
use geo::{BooleanOps, Coord, Intersects, Line, LineString, MultiPolygon, Polygon, TriangulateEarcut};
use rayon::prelude::*;
use std::collections::HashMap;

/// A corner of a piece: where it is, and its weights over the corners of the triangle it was
/// cut from.
#[derive(Clone, Copy)]
struct Corner {
    pos: [f64; 3],
    w: [f64; 3],
}

/// A triangle after cutting: the triangle it came from, which side it is on, and its corners.
struct Piece {
    from: usize,
    inside: bool,
    corners: [Corner; 3],
}

/// The weights of a point over the corners of a triangle, in the plane seen along the axis
/// the triangle faces most.
fn weights(t: &[[f64; 3]; 3], p: [f64; 3]) -> [f64; 3] {
    let n = normal(t);
    let axis = if n[2].abs() >= n[0].abs() && n[2].abs() >= n[1].abs() {
        2
    } else if n[0].abs() >= n[1].abs() {
        0
    } else {
        1
    };
    let (u, v) = match axis {
        2 => (0, 1),
        0 => (1, 2),
        _ => (2, 0),
    };
    let (a, b, c) = (t[0], t[1], t[2]);
    let d = (b[u] - a[u]) * (c[v] - a[v]) - (c[u] - a[u]) * (b[v] - a[v]);
    if d.abs() < 1e-30 {
        return [1.0, 0.0, 0.0];
    }
    let w1 = ((p[u] - a[u]) * (c[v] - a[v]) - (c[u] - a[u]) * (p[v] - a[v])) / d;
    let w2 = ((b[u] - a[u]) * (p[v] - a[v]) - (p[u] - a[u]) * (b[v] - a[v])) / d;
    [1.0 - w1 - w2, w1, w2]
}

fn normal(t: &[[f64; 3]; 3]) -> [f64; 3] {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

fn whole(from: usize, inside: bool, t: &[[f64; 3]; 3]) -> Piece {
    let c = |k: usize| {
        let mut w = [0.0; 3];
        w[k] = 1.0;
        Corner { pos: t[k], w }
    };
    Piece { from, inside, corners: [c(0), c(1), c(2)] }
}

/// The corner at a point of the triangle seen from above, with its height from the plane.
/// A point at a corner of the triangle is that corner.
fn corner_at(t: &[[f64; 3]; 3], x: f64, y: f64) -> Corner {
    for (k, q) in t.iter().enumerate() {
        if (q[0] - x).abs() <= 1e-9 * (1.0 + q[0].abs()) && (q[1] - y).abs() <= 1e-9 * (1.0 + q[1].abs()) {
            let mut w = [0.0; 3];
            w[k] = 1.0;
            return Corner { pos: *q, w };
        }
    }
    let (a, b, c) = (t[0], t[1], t[2]);
    let d = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    let w1 = ((x - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (y - a[1])) / d;
    let w2 = ((b[0] - a[0]) * (y - a[1]) - (x - a[0]) * (b[1] - a[1])) / d;
    let w = [1.0 - w1 - w2, w1, w2];
    Corner { pos: [x, y, w[0] * a[2] + w[1] * b[2] + w[2] * c[2]], w }
}

/// Cuts one triangle that faces up or down. The boundary is known to run near it.
fn cut_flat(from: usize, t: &[[f64; 3]; 3], area: &MultiPolygon<f64>, out: &mut Vec<Piece>) {
    let ring = LineString::new(vec![
        Coord { x: t[0][0], y: t[0][1] },
        Coord { x: t[1][0], y: t[1][1] },
        Coord { x: t[2][0], y: t[2][1] },
        Coord { x: t[0][0], y: t[0][1] },
    ]);
    let tri = MultiPolygon::new(vec![Polygon::new(ring, Vec::new())]);
    // Counter-clockwise from above or not, which the pieces keep.
    let up = normal(t)[2] > 0.0;
    let size = ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1])).abs() / 2.0;
    for (inside, parts) in [(true, area.intersection(&tri)), (false, tri.difference(area))] {
        for poly in parts.0 {
            let raw = poly.earcut_triangles_raw();
            for k in raw.triangle_indices.chunks_exact(3) {
                let p = [k[0], k[1], k[2]].map(|i| raw.vertices[i]);
                let cross = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
                // Slivers that are left from cutting along an edge are dropped.
                if cross.abs() / 2.0 <= size * 1e-12 {
                    continue;
                }
                let mut c = p.map(|q| corner_at(t, q[0], q[1]));
                if (cross > 0.0) != up {
                    c.swap(1, 2);
                }
                out.push(Piece { from, inside, corners: c });
            }
        }
    }
}

/// Cuts an upright triangle, such as a wall, by upright planes where its line seen from
/// above crosses the boundary.
fn cut_upright(from: usize, t: &[[f64; 3]; 3], clipper: &Clipper, out: &mut Vec<Piece>) {
    // The line the triangle stands on, from above: from the two corners farthest apart.
    let mut ends = (0, 1);
    let mut far = -1.0;
    for (a, b) in [(0, 1), (1, 2), (0, 2)] {
        let d = (t[a][0] - t[b][0]).powi(2) + (t[a][1] - t[b][1]).powi(2);
        if d > far {
            far = d;
            ends = (a, b);
        }
    }
    let (o, e) = (t[ends.0], t[ends.1]);
    if far <= 0.0 {
        // A triangle that stands on a single point from above.
        out.push(whole(from, clipper.has_point(Coord { x: o[0], y: o[1] }), t));
        return;
    }
    let len = far.sqrt();
    let dir = [(e[0] - o[0]) / len, (e[1] - o[1]) / len];
    let along = |p: [f64; 3]| (p[0] - o[0]) * dir[0] + (p[1] - o[1]) * dir[1];
    let seg = Line::new(Coord { x: o[0], y: o[1] }, Coord { x: e[0], y: e[1] });
    let mut cuts: Vec<f64> = Vec::new();
    for b in clipper.boundary_near([o[0].min(e[0]), o[1].min(e[1])], [o[0].max(e[0]), o[1].max(e[1])]) {
        use geo::line_intersection::{line_intersection, LineIntersection};
        match line_intersection(seg, *b) {
            Some(LineIntersection::SinglePoint { intersection: p, .. }) => cuts.push(along([p.x, p.y, 0.0])),
            Some(LineIntersection::Collinear { intersection: l }) => {
                cuts.push(along([l.start.x, l.start.y, 0.0]));
                cuts.push(along([l.end.x, l.end.y, 0.0]));
            }
            None => {}
        }
    }
    cuts.retain(|s| *s > len * 1e-9 && *s < len * (1.0 - 1e-9));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() <= len * 1e-9);
    let side = |lo: f64, hi: f64| {
        let m = (lo + hi) / 2.0;
        clipper.has_point(Coord { x: o[0] + dir[0] * m, y: o[1] + dir[1] * m })
    };
    if cuts.is_empty() {
        out.push(whole(from, side(0.0, len), t));
        return;
    }
    // The slices between the cuts, each the triangle clipped by two upright planes: a convex
    // polygon, laid out in triangles as a fan.
    let mut bounds = vec![f64::NEG_INFINITY];
    bounds.extend(cuts.iter().copied());
    bounds.push(f64::INFINITY);
    for w in bounds.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let mut poly: Vec<[f64; 3]> = t.to_vec();
        for (limit, keep_below) in [(lo, false), (hi, true)] {
            if !limit.is_finite() {
                continue;
            }
            let inside = |p: &[f64; 3]| if keep_below { along(*p) <= limit } else { along(*p) >= limit };
            let mut next = Vec::new();
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                let (ia, ib) = (inside(&a), inside(&b));
                if ia {
                    next.push(a);
                }
                if ia != ib {
                    let (sa, sb) = (along(a), along(b));
                    let f = (limit - sa) / (sb - sa);
                    next.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]);
                }
            }
            poly = next;
        }
        if poly.len() < 3 {
            continue;
        }
        let is_in = side(lo.max(0.0), hi.min(len));
        let corner = |p: [f64; 3]| Corner { pos: p, w: weights(t, p) };
        for i in 1..poly.len() - 1 {
            let tri = [poly[0], poly[i], poly[i + 1]];
            let n = normal(&tri);
            if n.iter().map(|v| v * v).sum::<f64>() <= 1e-24 * far * far {
                continue;
            }
            out.push(Piece { from, inside: is_in, corners: tri.map(corner) });
        }
    }
}

/// Cuts a model by the clip areas: the part inside and the part outside, each with all the
/// objects of the model, some of which may have no triangles left.
pub fn clip_mesh(mesh: &Mesh, clipper: &Clipper) -> (Mesh, Mesh) {
    let bbox = clipper.bbox();
    let pieces: Vec<Vec<Piece>> = mesh
        .tris
        .par_chunks(4096)
        .enumerate()
        .map(|(chunk, tris)| {
            let mut out = Vec::new();
            for (k, tri) in tris.iter().enumerate() {
                let from = chunk * 4096 + k;
                let t = tri.v.map(|i| mesh.positions[i as usize]);
                let lo = [t[0][0].min(t[1][0]).min(t[2][0]), t[0][1].min(t[1][1]).min(t[2][1])];
                let hi = [t[0][0].max(t[1][0]).max(t[2][0]), t[0][1].max(t[1][1]).max(t[2][1])];
                if hi[0] < bbox.min().x || hi[1] < bbox.min().y || lo[0] > bbox.max().x || lo[1] > bbox.max().y {
                    out.push(whole(from, false, &t));
                    continue;
                }
                let n = normal(&t);
                let len2 = n.iter().map(|v| v * v).sum::<f64>();
                let upright = n[2] * n[2] <= len2 * 1e-12;
                if upright {
                    cut_upright(from, &t, clipper, &mut out);
                    continue;
                }
                // Does the boundary cross the triangle seen from above? If not, one point tells
                // the side of all of it.
                let flat = geo::Triangle::new(Coord { x: t[0][0], y: t[0][1] }, Coord { x: t[1][0], y: t[1][1] }, Coord { x: t[2][0], y: t[2][1] });
                let crossed = clipper.boundary_near(lo, hi).any(|l| l.intersects(&flat));
                if !crossed {
                    let c = Coord { x: (t[0][0] + t[1][0] + t[2][0]) / 3.0, y: (t[0][1] + t[1][1] + t[2][1]) / 3.0 };
                    out.push(whole(from, clipper.has_point(c), &t));
                    continue;
                }
                cut_flat(from, &t, &clipper.local_area(lo, hi), &mut out);
            }
            out
        })
        .collect();
    // The model the two parts share: the corners, normals and texture coordinates there were,
    // and those of the cuts. Each part then keeps what it uses.
    let mut shared = Mesh { positions: mesh.positions.clone(), normals: mesh.normals.clone(), uvs: mesh.uvs.clone(), ..Default::default() };
    let mut new_corner: HashMap<[u64; 3], u32> = HashMap::new();
    let (mut tris_in, mut tris_out) = (Vec::new(), Vec::new());
    for p in pieces.iter().flatten() {
        let src = &mesh.tris[p.from];
        let mut tri = Tri { v: [0; 3], n: [NONE; 3], t: [NONE; 3], material: src.material, object: src.object };
        for (k, c) in p.corners.iter().enumerate() {
            if let Some(j) = (0..3).find(|j| c.w[*j] == 1.0 && c.pos == mesh.positions[src.v[*j] as usize]) {
                tri.v[k] = src.v[j];
                tri.n[k] = src.n[j];
                tri.t[k] = src.t[j];
                continue;
            }
            let key = c.pos.map(f64::to_bits);
            tri.v[k] = *new_corner.entry(key).or_insert_with(|| {
                shared.positions.push(c.pos);
                (shared.positions.len() - 1) as u32
            });
            let ok = |a: [u32; 3], len: usize| a.iter().all(|i| *i != NONE && (*i as usize) < len);
            if ok(src.n, mesh.normals.len()) {
                let mut n = [0f32; 3];
                for j in 0..3 {
                    let s = mesh.normals[src.n[j] as usize];
                    for (a, v) in n.iter_mut().enumerate() {
                        *v += c.w[j] as f32 * s[a];
                    }
                }
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
                shared.normals.push(n.map(|v| v / l));
                tri.n[k] = (shared.normals.len() - 1) as u32;
            }
            if ok(src.t, mesh.uvs.len()) {
                let mut uv = [0f32; 2];
                for j in 0..3 {
                    let s = mesh.uvs[src.t[j] as usize];
                    uv[0] += c.w[j] as f32 * s[0];
                    uv[1] += c.w[j] as f32 * s[1];
                }
                shared.uvs.push(uv);
                tri.t[k] = (shared.uvs.len() - 1) as u32;
            }
        }
        if p.inside {
            tris_in.push(tri);
        } else {
            tris_out.push(tri);
        }
    }
    let part = |tris: Vec<Tri>| Mesh {
        positions: shared.positions.clone(),
        normals: shared.normals.clone(),
        uvs: shared.uvs.clone(),
        tris,
        materials: mesh.materials.clone(),
        objects: mesh.objects.clone(),
    };
    let all: Vec<usize> = (0..mesh.objects.len()).collect();
    (part(tris_in).pick(&all), part(tris_out).pick(&all))
}
