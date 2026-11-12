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

/// Closes a model where it was cut: for each object, the open edges that lie on the upright
/// "curtain" over the boundary of the clip areas are chained into rings, and each ring is
/// filled with a wall of its own, in the material of the object there. A solid cut in two,
/// such as a building, is then closed again on each side. Gives the number of walls.
///
/// The curtain bends where the boundary turns, so each ring is laid out flat along the
/// boundary (the length along it, and the height) before it is filled. A ring within another
/// of the same object, such as round a courtyard, is filled too.
pub fn close_cut(mesh: &mut Mesh, clipper: &Clipper) -> usize {
    use std::collections::HashSet;
    let key = |p: [f64; 3]| [(p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64, (p[2] * 1e5).round() as i64];
    // The rings of the clip boundary, each segment with how far along its ring it begins.
    let mut segs: Vec<(usize, Coord<f64>, Coord<f64>, f64)> = Vec::new();
    let mut ring_len: Vec<f64> = Vec::new();
    for poly in &clipper.area().0 {
        for ring in std::iter::once(poly.exterior()).chain(poly.interiors()) {
            let r = ring_len.len();
            let mut along = 0.0;
            for l in ring.lines() {
                segs.push((r, l.start, l.end, along));
                along += (l.end.x - l.start.x).hypot(l.end.y - l.start.y);
            }
            ring_len.push(along);
        }
    }
    // Where a point lies on the boundary: its ring and how far along it, if it is on it.
    let scale = {
        let b = clipper.bbox();
        b.width().max(b.height()).max(1.0)
    };
    let on_boundary = |p: [f64; 3]| -> Option<(usize, f64)> {
        let mut best: Option<(f64, usize, f64)> = None;
        for (r, a, b, start) in &segs {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            if len2 == 0.0 {
                continue;
            }
            let t = (((p[0] - a.x) * dx + (p[1] - a.y) * dy) / len2).clamp(0.0, 1.0);
            let (qx, qy) = (a.x + t * dx, a.y + t * dy);
            let d = (p[0] - qx).hypot(p[1] - qy);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, *r, start + t * len2.sqrt()));
            }
        }
        best.filter(|b| b.0 <= scale * 1e-7 + 1e-6).map(|b| (b.1, b.2))
    };
    // The edges of each object, by where their ends are, with how often each is used and
    // which way it runs in the triangle that uses it once.
    let mut count: HashMap<EdgeKey, EdgeUse> = HashMap::new();
    for t in &mesh.tris {
        for k in 0..3 {
            let (a, b) = (mesh.positions[t.v[k] as usize], mesh.positions[t.v[(k + 1) % 3] as usize]);
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            let e = if ka < kb { (t.object, ka, kb) } else { (t.object, kb, ka) };
            let slot = count.entry(e).or_insert((0, a, b, t.material));
            slot.0 += 1;
        }
    }
    // The open edges on the curtain, turned round: the wall runs the other way.
    let mut next: HashMap<(u32, [i64; 3]), Vec<EdgeNext>> = HashMap::new();
    for ((object, _, _), (n, a, b, material)) in &count {
        if *n != 1 {
            continue;
        }
        let (Some(_), Some(_)) = (on_boundary(*a), on_boundary(*b)) else { continue };
        // The wall goes from b to a.
        next.entry((*object, key(*b))).or_default().push((key(*a), *b, *a, *material));
    }
    let mut used: HashSet<(u32, [i64; 3], [i64; 3])> = HashSet::new();
    let mut walls = 0usize;
    let starts: Vec<(u32, [i64; 3])> = next.keys().copied().collect();
    for start in starts {
        let Some(list) = next.get(&start).cloned() else { continue };
        for first in list {
            if used.contains(&(start.0, start.1, first.0)) {
                continue;
            }
            // Follow the open edges round to where they began.
            let object = start.0;
            let material = first.3;
            let mut ring: Vec<[f64; 3]> = vec![first.1];
            used.insert((object, start.1, first.0));
            let mut at = first.0;
            let mut last = first.2;
            let mut closed = false;
            for _ in 0..100_000 {
                if at == start.1 {
                    closed = true;
                    break;
                }
                ring.push(last);
                let Some(cands) = next.get(&(object, at)) else { break };
                let Some(step) = cands.iter().find(|c| !used.contains(&(object, at, c.0))) else { break };
                used.insert((object, at, step.0));
                at = step.0;
                last = step.2;
            }
            if !closed || ring.len() < 3 {
                continue;
            }
            // Laid out flat along the boundary: how far along, and the height.
            let placed: Vec<Option<(usize, f64)>> = ring.iter().map(|p| on_boundary(*p)).collect();
            let Some(r) = placed.first().copied().flatten().map(|p| p.0) else { continue };
            let total = ring_len[r];
            let base = placed[0].map_or(0.0, |p| p.1);
            let flat: Vec<[f64; 3]> = ring
                .iter()
                .zip(&placed)
                .map(|(p, at)| {
                    let mut s = at.map_or(base, |a| a.1) - base;
                    // Round the ring of the boundary the short way.
                    if s > total / 2.0 {
                        s -= total;
                    } else if s < -total / 2.0 {
                        s += total;
                    }
                    [s, p[2], 0.0]
                })
                .collect();
            let tris = crate::mesh::triangulate(&flat);
            if tris.is_empty() {
                continue;
            }
            let first_index = mesh.positions.len() as u32;
            mesh.positions.extend(ring.iter().copied());
            for t in tris {
                mesh.tris.push(Tri { v: t.map(|i| first_index + i as u32), n: [NONE; 3], t: [NONE; 3], material, object });
            }
            walls += 1;
        }
    }
    walls
}

/// An edge of an object by the object and its two ends, rounded.
type EdgeKey = (u32, [i64; 3], [i64; 3]);
/// How often an edge is used, its ends as they run in the triangle, and the triangle's object.
type EdgeUse = (u32, [f64; 3], [f64; 3], u32);
/// The far end of an open edge, rounded, its two ends and the object.
type EdgeNext = ([i64; 3], [f64; 3], [f64; 3], u32);

#[cfg(test)]
mod close_tests {
    use super::*;

    /// A box cut in two by an area is closed again on each side, so that every edge is used
    /// twice.
    #[test]
    fn a_box_cut_in_two_is_closed_on_both_sides() {
        // A box of 10 × 10 × 5, as eight corners and twelve triangles facing out.
        let p = |x: f64, y: f64, z: f64| [x, y, z];
        let positions = vec![p(0., 0., 0.), p(10., 0., 0.), p(10., 10., 0.), p(0., 10., 0.), p(0., 0., 5.), p(10., 0., 5.), p(10., 10., 5.), p(0., 10., 5.)];
        let faces: [[u32; 3]; 12] =
            [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        let mesh = Mesh {
            positions,
            tris: faces.iter().map(|v| Tri { v: *v, n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 }).collect(),
            materials: vec![Default::default()],
            objects: vec!["box".into()],
            ..Default::default()
        };
        let area = geo::Geometry::Polygon(Polygon::new(LineString::from(vec![(-5.0, -5.0), (4.0, -5.0), (4.0, 15.0), (-5.0, 15.0), (-5.0, -5.0)]), vec![]));
        let clipper = Clipper::new(std::iter::once(&area)).unwrap();
        let (mut inside, mut outside) = clip_mesh(&mesh, &clipper);
        let open = |m: &Mesh| -> usize {
            let key = |p: [f64; 3]| [(p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64, (p[2] * 1e5).round() as i64];
            let mut n: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
            for t in &m.tris {
                for k in 0..3 {
                    let (a, b) = (key(m.positions[t.v[k] as usize]), key(m.positions[t.v[(k + 1) % 3] as usize]));
                    *n.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1;
                }
            }
            n.values().filter(|c| **c == 1).count()
        };
        assert!(open(&inside) > 0, "the cut leaves the box open");
        assert_eq!(close_cut(&mut inside, &clipper), 1);
        assert_eq!(close_cut(&mut outside, &clipper), 1);
        assert_eq!(open(&inside), 0, "closed inside");
        assert_eq!(open(&outside), 0, "closed outside");
    }
}
