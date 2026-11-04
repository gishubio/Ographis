//! The curved faces of 3D solids in DWG: cylinders, cones, spheres and tori, made into
//! triangles. Each face is laid out flat on the parameters of its surface, as a map is of
//! the earth, its edges drawn there, filled with triangles between them and points in a
//! grid inside, and the triangles taken back onto the surface.

use acadrust::entities::acis::{SatConeSurface, SatDocument, SatFace, SatRecord, SatSphereSurface, SatTorusSurface, Sense};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

type P3 = [f64; 3];

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: P3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: P3) -> P3 {
    mul(a, 1.0 / len(a).max(1e-300))
}
fn t3(v: (f64, f64, f64)) -> P3 {
    [v.0, v.1, v.2]
}

/// A surface that goes round: a point of it from two parameters, and back. `u` goes round
/// the axis; `v` along it for a cone, from pole to pole for a sphere, and round the tube for
/// a torus.
#[derive(Clone, Copy, Debug)]
enum Surface {
    /// Centre, axis, major axis, its length, the minor axis as long as the major times the
    /// ratio, and how much the radius grows for each unit along the axis.
    Cone {
        c: P3,
        a: P3,
        m: P3,
        n: P3,
        r0: f64,
        ratio: f64,
        grow: f64,
    },
    Sphere {
        c: P3,
        r: f64,
        x: P3,
        y: P3,
        pole: P3,
    },
    Torus {
        c: P3,
        n: P3,
        x: P3,
        y: P3,
        big: f64,
        small: f64,
    },
}

impl Surface {
    fn of(record: &SatRecord) -> Option<(Surface, bool)> {
        if let Some(s) = SatConeSurface::from_record(record) {
            let a = unit(t3(s.axis()));
            let major = t3(s.major_axis());
            let r0 = len(major);
            if r0 <= 0.0 || s.cos_half_angle().abs() < 1e-12 {
                return None;
            }
            let m = unit(major);
            let n = cross(a, m);
            let grow = s.sin_half_angle() / s.cos_half_angle();
            return Some((Surface::Cone { c: t3(s.center()), a, m, n, r0, ratio: s.ratio(), grow }, s.sense() == Sense::Reversed));
        }
        if let Some(s) = SatSphereSurface::from_record(record) {
            let pole = unit(t3(s.pole()));
            let x = unit(sub(t3(s.u_direction()), mul(pole, dot(t3(s.u_direction()), pole))));
            let y = cross(pole, x);
            return Some((Surface::Sphere { c: t3(s.center()), r: s.radius().abs(), x, y, pole }, s.sense() == Sense::Reversed));
        }
        if let Some(s) = SatTorusSurface::from_record(record) {
            let n = unit(t3(s.normal()));
            let x = unit(sub(t3(s.u_direction()), mul(n, dot(t3(s.u_direction()), n))));
            let y = cross(n, x);
            return Some((Surface::Torus { c: t3(s.center()), n, x, y, big: s.major_radius(), small: s.minor_radius().abs() }, s.sense() == Sense::Reversed));
        }
        None
    }

    fn at(&self, u: f64, v: f64) -> P3 {
        match *self {
            Surface::Cone { c, a, m, n, r0, ratio, grow } => {
                let s = 1.0 + v * grow / r0;
                add(add(c, mul(a, v)), add(mul(m, r0 * s * u.cos()), mul(n, r0 * ratio * s * u.sin())))
            }
            Surface::Sphere { c, r, x, y, pole } => add(c, add(mul(pole, r * v.sin()), add(mul(x, r * v.cos() * u.cos()), mul(y, r * v.cos() * u.sin())))),
            Surface::Torus { c, n, x, y, big, small } => {
                let ring = big + small * v.cos();
                add(c, add(mul(n, small * v.sin()), add(mul(x, ring * u.cos()), mul(y, ring * u.sin()))))
            }
        }
    }

    /// The parameters of a point on the surface; `u` is None at a pole or the apex, where
    /// every `u` is the same point.
    fn params(&self, p: P3) -> (Option<f64>, f64) {
        match *self {
            Surface::Cone { c, a, m, n, ratio, .. } => {
                let q = sub(p, c);
                let v = dot(q, a);
                let radial = sub(q, mul(a, v));
                let u = (len(radial) > 1e-9).then(|| (dot(radial, n) / ratio.max(1e-12)).atan2(dot(radial, m)));
                (u, v)
            }
            Surface::Sphere { c, r, x, y, pole } => {
                let q = sub(p, c);
                let v = (dot(q, pole) / r.max(1e-300)).clamp(-1.0, 1.0).asin();
                let u = (v.cos() > 1e-9).then(|| dot(q, y).atan2(dot(q, x)));
                (u, v)
            }
            Surface::Torus { c, n, x, y, big, .. } => {
                let q = sub(p, c);
                let h = dot(q, n);
                let radial = sub(q, mul(n, h));
                let u = dot(radial, y).atan2(dot(radial, x));
                let v = h.atan2(len(radial) - big);
                (Some(u), v)
            }
        }
    }

    /// Metres for a step of each parameter, roughly, so that the triangles come out even.
    fn scale(&self) -> (f64, f64) {
        match *self {
            Surface::Cone { r0, .. } => (r0.max(1e-9), 1.0),
            Surface::Sphere { r, .. } => (r.max(1e-9), r.max(1e-9)),
            Surface::Torus { big, small, .. } => ((big + small).max(1e-9), small.max(1e-9)),
        }
    }

    fn v_goes_round(&self) -> bool {
        matches!(self, Surface::Torus { .. })
    }
}

/// A loop laid out on the parameters: the points with `u` made continuous, and how far it
/// went round in `u` and in `v`.
struct Laid {
    pts: Vec<(f64, f64)>,
    round_u: f64,
    round_v: f64,
}

fn lay(surface: &Surface, loop3: &[P3]) -> Option<Laid> {
    let raw: Vec<(Option<f64>, f64)> = loop3.iter().map(|p| surface.params(*p)).collect();
    // A pole has every u: it takes the u of the point before it, or after.
    let first_u = raw.iter().find_map(|r| r.0)?;
    let (mut pts, mut last_u, mut last_v) = (Vec::with_capacity(raw.len()), first_u, raw[0].1);
    for (u, v) in &raw {
        let u = match u {
            Some(u) => last_u + (u - last_u + PI).rem_euclid(TAU) - PI,
            None => last_u,
        };
        let v = if surface.v_goes_round() { last_v + (v - last_v + PI).rem_euclid(TAU) - PI } else { *v };
        pts.push((u, v));
        last_u = u;
        last_v = v;
    }
    // The step back to the first point closes the loop; how far it all went round.
    let back = |last: f64, first: f64| last + (first - last + PI).rem_euclid(TAU) - PI - first;
    let round_u = back(last_u, pts[0].0);
    let round_v = if surface.v_goes_round() { back(last_v, pts[0].1) } else { 0.0 };
    Some(Laid { pts, round_u, round_v })
}

/// The triangles of a curved face, or None when its surface is not one of those made here
/// or its edges cannot be laid out.
pub(crate) fn face_triangles(sat: &SatDocument, face: &SatFace, loops: &[Vec<P3>], place: &dyn Fn(P3) -> P3) -> Option<Vec<[P3; 3]>> {
    let (surface, surface_reversed) = Surface::of(sat.resolve(face.surface())?)?;
    let reversed = surface_reversed ^ (face.sense() == Sense::Reversed);
    let laid: Vec<Laid> = loops.iter().filter(|l| l.len() >= 2).map(|l| lay(&surface, l)).collect::<Option<_>>()?;
    let (su, sv) = surface.scale();
    // A torus whose loops go round its tube, as at the ends of a bend in a pipe: v is the
    // direction that goes round, and the parameters change places.
    let swap = surface.v_goes_round() && laid.iter().any(|l| l.round_v.abs() > PI) && !laid.iter().any(|l| l.round_u.abs() > PI);
    let (su, sv) = if swap { (sv, su) } else { (su, sv) };
    let laid: Vec<Laid> = if swap {
        laid.into_iter().map(|l| Laid { pts: l.pts.iter().map(|p| (p.1, p.0)).collect(), round_u: l.round_v, round_v: l.round_u }).collect()
    } else {
        laid
    };
    let eval = |u: f64, v: f64| if swap { surface.at(v, u) } else { surface.at(u, v) };
    let flipped = reversed ^ swap;
    let round: Vec<&Laid> = laid.iter().filter(|l| l.round_u.abs() > PI).collect();
    let flat: Vec<&Laid> = laid.iter().filter(|l| l.round_u.abs() <= PI).collect();
    // The region on the parameters: an outer ring and holes, in metres.
    let ring = |pts: Vec<(f64, f64)>| geo::LineString::from(pts.into_iter().map(|(u, v)| (u * su, v * sv)).collect::<Vec<_>>());
    // Where a loop going round in u lies in v, from u = 0 to u = 2π.
    let band = |l: &Laid| -> Vec<(f64, f64)> {
        let mut pts: Vec<(f64, f64)> = l.pts.iter().map(|(u, v)| (u.rem_euclid(TAU), *v)).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-12);
        let (first, last) = (pts[0], pts[pts.len() - 1]);
        // Where it crosses u = 0, between the last and the first point.
        let gap = first.0 + TAU - last.0;
        let k = if gap > 1e-12 { (TAU - last.0) / gap } else { 0.0 };
        let v0 = last.1 + (first.1 - last.1) * k;
        let mut out = vec![(0.0, v0)];
        out.extend(pts.into_iter().filter(|p| p.0 > 1e-12 && p.0 < TAU - 1e-12));
        out.push((TAU, v0));
        out
    };
    let (outer, holes): (geo::LineString<f64>, Vec<&Laid>) = match (round.len(), flat.is_empty()) {
        // The whole of a sphere or a torus, with no edges.
        (0, true) if loops.is_empty() => match surface {
            Surface::Sphere { .. } => (ring(vec![(0.0, -FRAC_PI_2), (TAU, -FRAC_PI_2), (TAU, FRAC_PI_2), (0.0, FRAC_PI_2)]), Vec::new()),
            Surface::Torus { .. } => (ring(vec![(0.0, -PI), (TAU, -PI), (TAU, PI), (0.0, PI)]), Vec::new()),
            Surface::Cone { .. } => return None,
        },
        // A patch: its largest loop the outer ring, the others holes in it.
        (0, false) => {
            let area = |l: &Laid| geo::Area::unsigned_area(&geo::Polygon::new(ring(l.pts.clone()), vec![]));
            let outer = flat.iter().copied().max_by(|a, b| area(a).total_cmp(&area(b)))?;
            let holes = flat.iter().copied().filter(|l| !std::ptr::eq(*l, outer)).collect();
            (ring(outer.pts.clone()), holes)
        }
        // Between two loops that go round, as the side of a cylinder.
        (2, _) => {
            let (a, b) = (band(round[0]), band(round[1]));
            let mean = |x: &[(f64, f64)]| x.iter().map(|p| p.1).sum::<f64>() / x.len() as f64;
            let (low, high) = if mean(&a) <= mean(&b) { (a, b) } else { (b, a) };
            let mut pts = low;
            pts.extend(high.into_iter().rev());
            (ring(pts), flat)
        }
        // From one loop that goes round to the pole or the apex on the side the face lies.
        (1, _) => {
            let l = round[0];
            let above = (l.round_u > 0.0) ^ flipped;
            let end = match surface {
                Surface::Sphere { .. } if !swap => {
                    if above {
                        FRAC_PI_2
                    } else {
                        -FRAC_PI_2
                    }
                }
                // The other way the cone has no end, so the face goes to the apex.
                Surface::Cone { r0, grow, .. } if grow.abs() > 1e-12 && !swap => -r0 / grow,
                _ => return None,
            };
            let mut pts = band(l);
            pts.push((TAU, end));
            pts.push((0.0, end));
            (ring(pts), flat)
        }
        _ => return None,
    };
    // Holes laid out a turn away from the outer ring are moved into it.
    let (u_lo, u_hi) = outer.0.iter().fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c.x), b.max(c.x)));
    let hole_rings: Vec<geo::LineString<f64>> = holes
        .iter()
        .map(|h| {
            let mid = h.pts.iter().map(|p| p.0).sum::<f64>() / h.pts.len() as f64 * su;
            let shift = ((u_lo + u_hi) / 2.0 - mid) / (TAU * su);
            let k = shift.round();
            ring(h.pts.iter().map(|(u, v)| (u + k * TAU, *v)).collect())
        })
        .collect();
    let region = geo::Polygon::new(outer, hole_rings);
    // A 32nd of a turn, round the tighter way of a sphere or a torus; along a cone the
    // surface is straight, and its turn alone sets the step.
    let step = match surface {
        Surface::Cone { .. } => su * TAU / 32.0,
        _ => su.min(sv) * TAU / 32.0,
    };
    Some(fill(&region, step, &|x, y| place(eval(x / su, y / sv))))
}

/// The triangles of a region of the parameters, in metres: its edges kept, with points in a
/// grid inside, each triangle taken onto the surface by `onto`.
fn fill(region: &geo::Polygon<f64>, step: f64, onto: &dyn Fn(f64, f64) -> P3) -> Vec<[P3; 3]> {
    use geo::{BoundingRect, Contains};
    use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = ConstrainedDelaunayTriangulation::new();
    // The edges with points no further apart than the grid inside, so that no triangle
    // reaches far across the surface along an edge.
    let add_ring = |r: &geo::LineString<f64>, cdt: &mut ConstrainedDelaunayTriangulation<Point2<f64>>| {
        let mut dense: Vec<geo::Coord<f64>> = Vec::new();
        for w in r.0.windows(2) {
            let (a, b) = (w[0], w[1]);
            let n = (((b.x - a.x).hypot(b.y - a.y)) / step).ceil().max(1.0) as usize;
            for k in 0..n {
                let t = k as f64 / n as f64;
                dense.push(geo::Coord { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });
            }
        }
        if let Some(last) = r.0.last() {
            dense.push(*last);
        }
        let mut prev = None;
        for c in dense.iter() {
            let Ok(h) = cdt.insert(Point2::new(c.x, c.y)) else { continue };
            if let Some(p) = prev {
                if p != h && cdt.can_add_constraint(p, h) {
                    cdt.add_constraint(p, h);
                }
            }
            prev = Some(h);
        }
    };
    add_ring(region.exterior(), &mut cdt);
    for h in region.interiors() {
        add_ring(h, &mut cdt);
    }
    // Points inside, a 32nd of a turn apart, so that the triangles follow the curve.
    if let Some(b) = region.bounding_rect() {
        let (nx, ny) = (((b.width() / step).ceil() as usize).min(400), ((b.height() / step).ceil() as usize).min(400));
        for i in 1..nx {
            for j in 1..ny {
                let p = geo::Point::new(b.min().x + b.width() * i as f64 / nx as f64, b.min().y + b.height() * j as f64 / ny as f64);
                if region.contains(&p) {
                    let _ = cdt.insert(Point2::new(p.x(), p.y()));
                }
            }
        }
    }
    let mut out = Vec::new();
    for f in cdt.inner_faces() {
        let v = f.vertices().map(|v| v.position());
        let centre = geo::Point::new((v[0].x + v[1].x + v[2].x) / 3.0, (v[0].y + v[1].y + v[2].y) / 3.0);
        if !region.contains(&centre) {
            continue;
        }
        let t = [onto(v[0].x, v[0].y), onto(v[1].x, v[1].y), onto(v[2].x, v[2].y)];
        if len(cross(sub(t[1], t[0]), sub(t[2], t[0]))) > 1e-12 {
            out.push(t);
        }
    }
    out
}
