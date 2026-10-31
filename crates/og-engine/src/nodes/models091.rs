//! 3D models added in 0.91: LoD2 buildings, with the shapes of their roofs fitted to the
//! points of a point cloud, and Terrain to 3D model, a surface of the heights of a raster
//! with a picture such as an orthophoto draped over it, for Blender and other 3D programs.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::pointcloud090::{points_on_buildings, OnBuildings};
use super::*;
use crate::mesh::{Material, Mesh, Texture, Tri, NONE};
use crate::model::{err, fmt_count, fmt_real, Feature};
use geo::{BooleanOps, Coord, Geometry, MultiPolygon, Polygon, TriangulateEarcut};
use std::sync::Arc;

// ---------- LoD2 buildings from points (Lod2Builder) ----------

const LOD2_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "classes",
        label: "Roof points",
        kind: ParamKind::Text,
        default: "",
        hint: "The classes of the points on the roofs, such as 6 for buildings. Empty takes every point on the footprint that is not ground",
    },
    ParamSpec {
        key: "ground_classes",
        label: "Ground points",
        kind: ParamKind::Text,
        default: "2",
        hint: "Without a terrain model on Ground, the ground is taken from the points of these classes around the building",
    },
    ParamSpec {
        key: "inset",
        label: "Leave out the edge",
        kind: ParamKind::Text,
        default: "0.5",
        hint: "Metres inside the footprint, so that walls, eaves and trees at the edge do not count",
    },
    ParamSpec {
        key: "shapes",
        label: "Roofs",
        kind: ParamKind::Choice(&[
            ("fitted", "The shape that fits the points best: flat, shed, gable or hipped"),
            ("flat", "All flat, at the height of the roof"),
        ]),
        default: "fitted",
        hint: "",
    },
    ParamSpec { key: "wall_colour", label: "Walls", kind: ParamKind::Text, default: "#e8e0d0", hint: "A colour, such as #e8e0d0" },
    ParamSpec {
        key: "roof_colour",
        label: "Roofs coloured",
        kind: ParamKind::Text,
        default: "#9a4b3c",
        hint: "A colour, such as #9a4b3c for tiles or #555555 for felt",
    },
];

/// A roof: z as a function of where on it, in the coordinates of the smallest rotated
/// rectangle around the building: u along its long side and v across, from its middle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Roof {
    Flat(f64),
    /// z = a + b·u + c·v.
    Shed(f64, f64, f64),
    /// z = a + b·(w − |v|), a ridge along the long side; or across it, with u and l.
    Gable {
        a: f64,
        b: f64,
        along: bool,
    },
    /// z = a + b·min(w − |v|, l − |u|): sloping up from every side.
    Hip(f64, f64),
}

impl Roof {
    fn name(&self) -> &'static str {
        match self {
            Roof::Flat(_) => "flat",
            Roof::Shed(..) => "shed",
            Roof::Gable { .. } => "gable",
            Roof::Hip(..) => "hipped",
        }
    }
    fn z(&self, u: f64, v: f64, l: f64, w: f64) -> f64 {
        match *self {
            Roof::Flat(a) => a,
            Roof::Shed(a, b, c) => a + b * u + c * v,
            Roof::Gable { a, b, along: true } => a + b * (w - v.abs()),
            Roof::Gable { a, b, along: false } => a + b * (l - u.abs()),
            Roof::Hip(a, b) => a + b * (w - v.abs()).min(l - u.abs()),
        }
    }
}

/// The frame of a building: its middle, the direction of its long side, and half its length
/// and width.
#[derive(Clone, Copy)]
struct Frame {
    c: Coord<f64>,
    e: (f64, f64),
    l: f64,
    w: f64,
}

impl Frame {
    fn of(g: &Geometry<f64>) -> Option<Frame> {
        use geo::MinimumRotatedRect;
        let r = g.minimum_rotated_rect()?;
        let p = &r.exterior().0;
        if p.len() < 4 {
            return None;
        }
        let (a, b) = ((p[1].x - p[0].x, p[1].y - p[0].y), (p[2].x - p[1].x, p[2].y - p[1].y));
        let (la, lb) = (a.0.hypot(a.1), b.0.hypot(b.1));
        let (long, ll, ls) = if la >= lb { (a, la, lb) } else { (b, lb, la) };
        if ll < 1e-6 {
            return None;
        }
        let c = Coord { x: (p[0].x + p[2].x) / 2.0, y: (p[0].y + p[2].y) / 2.0 };
        Some(Frame { c, e: (long.0 / ll, long.1 / ll), l: ll / 2.0, w: ls / 2.0 })
    }
    fn uv(&self, x: f64, y: f64) -> (f64, f64) {
        let (dx, dy) = (x - self.c.x, y - self.c.y);
        (dx * self.e.0 + dy * self.e.1, -dx * self.e.1 + dy * self.e.0)
    }
    fn xy(&self, u: f64, v: f64) -> Coord<f64> {
        Coord { x: self.c.x + u * self.e.0 - v * self.e.1, y: self.c.y + u * self.e.1 + v * self.e.0 }
    }
}

/// The least squares fit of z = a + b·f, and how far the points lie from it.
fn fit1(pts: &[(f64, f64, f64)], f: &dyn Fn(f64, f64) -> f64) -> Option<(f64, f64, f64)> {
    let n = pts.len() as f64;
    let (mut sf, mut sz, mut sff, mut sfz) = (0.0, 0.0, 0.0, 0.0);
    for (u, v, z) in pts {
        let x = f(*u, *v);
        sf += x;
        sz += z;
        sff += x * x;
        sfz += x * z;
    }
    let det = n * sff - sf * sf;
    if det.abs() < 1e-9 {
        return None;
    }
    let b = (n * sfz - sf * sz) / det;
    let a = (sz - b * sf) / n;
    let rms = (pts.iter().map(|(u, v, z)| (z - a - b * f(*u, *v)).powi(2)).sum::<f64>() / n).sqrt();
    Some((a, b, rms))
}

/// The roof that fits the points (u, v, z) best, with how far they lie from it. A shape with
/// more planes must fit clearly better to be chosen.
fn fit_roof(pts: &[(f64, f64, f64)], l: f64, w: f64) -> (Roof, f64) {
    let n = pts.len() as f64;
    let mut zs: Vec<f64> = pts.iter().map(|p| p.2).collect();
    zs.sort_by(f64::total_cmp);
    let median = zs[zs.len() / 2];
    let flat_rms = (pts.iter().map(|p| (p.2 - median).powi(2)).sum::<f64>() / n).sqrt();
    let mut best = (Roof::Flat(median), flat_rms, flat_rms);
    if flat_rms < 0.15 {
        return (best.0, best.1);
    }
    let mut consider = |r: Roof, rms: f64, penalty: f64| {
        if rms * penalty < best.2 {
            best = (r, rms, rms * penalty);
        }
    };
    let as_xy: Vec<(f64, f64, f64)> = pts.to_vec();
    if let Some(([a, b, c], rms)) = plane_uv(&as_xy) {
        consider(Roof::Shed(a, b, c), rms, 1.1);
    }
    for along in [true, false] {
        let f = move |u: f64, v: f64| if along { w - v.abs() } else { l - u.abs() };
        if let Some((a, b, rms)) = fit1(pts, &f) {
            if b > 0.05 {
                consider(Roof::Gable { a, b, along }, rms, 1.05);
            }
        }
    }
    let f = move |u: f64, v: f64| (w - v.abs()).min(l - u.abs());
    if let Some((a, b, rms)) = fit1(pts, &f) {
        if b > 0.05 {
            consider(Roof::Hip(a, b), rms, 1.12);
        }
    }
    (best.0, best.1)
}

/// A plane z = a + b·u + c·v by least squares.
fn plane_uv(pts: &[(f64, f64, f64)]) -> Option<([f64; 3], f64)> {
    let n = pts.len() as f64;
    if pts.len() < 3 {
        return None;
    }
    let (mu, mv, mz) = pts.iter().fold((0.0, 0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n, a.2 + p.2 / n));
    let (mut suu, mut suv, mut svv, mut suz, mut svz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (u, v, z) in pts {
        let (du, dv, dz) = (u - mu, v - mv, z - mz);
        suu += du * du;
        suv += du * dv;
        svv += dv * dv;
        suz += du * dz;
        svz += dv * dz;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-9 {
        return None;
    }
    let b = (suz * svv - svz * suv) / det;
    let c = (svz * suu - suz * suv) / det;
    let rms = (pts.iter().map(|(u, v, z)| (z - (mz + b * (u - mu) + c * (v - mv))).powi(2)).sum::<f64>() / n).sqrt();
    Some(([mz - b * mu - c * mv, b, c], rms))
}

/// The parts of a roof that are each one plane, as polygons in (u, v): where the formula of
/// the roof bends, at a ridge or a hip, the footprint is cut.
fn planes_of(roof: &Roof, l: f64, w: f64) -> Vec<Vec<(f64, f64)>> {
    let e = 4.0 * (l + w) + 10.0;
    match roof {
        Roof::Flat(_) | Roof::Shed(..) => vec![vec![(-e, -e), (e, -e), (e, e), (-e, e)]],
        Roof::Gable { along: true, .. } => vec![vec![(-e, 0.0), (e, 0.0), (e, e), (-e, e)], vec![(-e, -e), (e, -e), (e, 0.0), (-e, 0.0)]],
        Roof::Gable { along: false, .. } => vec![vec![(0.0, -e), (e, -e), (e, e), (0.0, e)], vec![(-e, -e), (0.0, -e), (0.0, e), (-e, e)]],
        Roof::Hip(..) => {
            // The ridge from −r to r along u, and the hips from its ends at 45°.
            let r = l - w;
            vec![
                vec![(-r, 0.0), (r, 0.0), (r + e, e), (-r - e, e)],
                vec![(r, 0.0), (-r, 0.0), (-r - e, -e), (r + e, -e)],
                vec![(r, 0.0), (r + e, -e), (r + e, e)],
                vec![(-r, 0.0), (-r - e, e), (-r - e, -e)],
            ]
        }
    }
}

/// Adds a building to a model: its roof in planes, its walls from the ground up to the edge
/// of the roof, and a floor.
fn add_building(mesh: &mut Mesh, poly: &Polygon<f64>, frame: &Frame, roof: &Roof, ground: f64, object: u32) {
    use geo::Winding;
    let z_at = |c: Coord<f64>| {
        let (u, v) = frame.uv(c.x, c.y);
        roof.z(u, v, frame.l, frame.w).max(ground + 0.5)
    };
    // The roof, a plane at a time, so that its ridges are edges of the triangles.
    let fp = MultiPolygon::new(vec![poly.clone()]);
    let mut on_outline: Vec<Coord<f64>> = Vec::new();
    for part in planes_of(roof, frame.l, frame.w) {
        let mut ring: Vec<Coord<f64>> = part.iter().map(|(u, v)| frame.xy(*u, *v)).collect();
        ring.push(ring[0]);
        let cut = fp.intersection(&MultiPolygon::new(vec![Polygon::new(geo::LineString(ring), vec![])]));
        for piece in &cut.0 {
            on_outline.extend(piece.exterior().0.iter().copied());
            let raw = piece.earcut_triangles_raw();
            let base = mesh.positions.len() as u32;
            mesh.positions.extend(raw.vertices.iter().map(|c| {
                let at = Coord { x: c[0], y: c[1] };
                [c[0], c[1], z_at(at)]
            }));
            for t in raw.triangle_indices.as_chunks::<3>().0.iter() {
                let (pa, pb, pc) = (raw.vertices[t[0]], raw.vertices[t[1]], raw.vertices[t[2]]);
                let ccw = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]) > 0.0;
                let (b, c) = if ccw { (t[1], t[2]) } else { (t[2], t[1]) };
                mesh.tris.push(Tri { v: [base + t[0] as u32, base + b as u32, base + c as u32], n: [NONE; 3], t: [NONE; 3], material: 1, object });
            }
        }
    }
    // The walls, with a corner wherever the roof bends above the outline.
    let mut rings: Vec<geo::LineString<f64>> = Vec::new();
    let mut outer = poly.exterior().clone();
    outer.make_ccw_winding();
    rings.push(outer);
    for r in poly.interiors() {
        let mut r = r.clone();
        r.make_cw_winding();
        rings.push(r);
    }
    for ring in &rings {
        for wpair in ring.0.windows(2) {
            let (a, b) = (wpair[0], wpair[1]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            if len2 < 1e-12 {
                continue;
            }
            let mut ts: Vec<f64> = vec![0.0, 1.0];
            for c in &on_outline {
                let t = ((c.x - a.x) * dx + (c.y - a.y) * dy) / len2;
                if t > 1e-6 && t < 1.0 - 1e-6 {
                    let (px, py) = (a.x + dx * t, a.y + dy * t);
                    if (px - c.x).hypot(py - c.y) < 1e-6 {
                        ts.push(t);
                    }
                }
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            for k in ts.windows(2) {
                let p = Coord { x: a.x + dx * k[0], y: a.y + dy * k[0] };
                let q = Coord { x: a.x + dx * k[1], y: a.y + dy * k[1] };
                let v = mesh.positions.len() as u32;
                mesh.positions.extend_from_slice(&[[p.x, p.y, ground], [q.x, q.y, ground], [q.x, q.y, z_at(q)], [p.x, p.y, z_at(p)]]);
                for t in [[v, v + 1, v + 2], [v, v + 2, v + 3]] {
                    mesh.tris.push(Tri { v: t, n: [NONE; 3], t: [NONE; 3], material: 0, object });
                }
            }
        }
    }
    // The floor.
    let raw = poly.earcut_triangles_raw();
    let base = mesh.positions.len() as u32;
    mesh.positions.extend(raw.vertices.iter().map(|c| [c[0], c[1], ground]));
    for t in raw.triangle_indices.as_chunks::<3>().0.iter() {
        let (pa, pb, pc) = (raw.vertices[t[0]], raw.vertices[t[1]], raw.vertices[t[2]]);
        let ccw = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]) > 0.0;
        let (b, c) = if ccw { (t[2], t[1]) } else { (t[1], t[2]) };
        mesh.tris.push(Tri { v: [base + t[0] as u32, base + b as u32, base + c as u32], n: [NONE; 3], t: [NONE; 3], material: 0, object });
    }
}

fn run_lod2(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let buildings = input(inp, "Buildings");
    let cloud = input(inp, "Points");
    if ctx.geographic(buildings.crs) == Some(true) {
        return err("The buildings are in degrees. Reproject them to the system of the point cloud, in metres.");
    }
    let ground_grid = input(inp, "Ground").grid.clone();
    let OnBuildings { roofs, ground } = points_on_buildings(p, buildings, cloud, ground_grid.as_deref())?;
    let only_flat = p.get("shapes").as_deref() == Some("flat");
    let mut mesh = Mesh {
        materials: vec![
            Material { name: "wall".into(), diffuse: super::solid::colour_param(p, "wall_colour", [0.91, 0.88, 0.82])?, ..Default::default() },
            Material { name: "roof".into(), diffuse: super::solid::colour_param(p, "roof_colour", [0.6, 0.29, 0.24])?, ..Default::default() },
        ],
        ..Default::default()
    };
    let mut out = buildings.clone();
    let slots: Vec<usize> =
        [("ground", FieldType::Real), ("height", FieldType::Real), ("roof_shape", FieldType::Text), ("roof_fit", FieldType::Real), ("points", FieldType::Int)]
            .iter()
            .map(|(n, t)| column(&mut out, n, *t))
            .collect();
    let round = |v: f64, d: i32| (v * 10f64.powi(d)).round() / 10f64.powi(d);
    let (mut made, mut without) = (0usize, 0usize);
    let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for (k, f) in out.features.iter_mut().enumerate() {
        mesh.objects.push(format!("building {}", k + 1));
        let Some(g) = &buildings.features[k].geom else { continue };
        let pts = &roofs[k];
        let frame = Frame::of(g);
        let mut zs: Vec<f64> = pts.iter().map(|p| p.2).collect();
        zs.sort_by(f64::total_cmp);
        let (Some(frame), false) = (frame, zs.len() < 3) else {
            without += 1;
            f.attrs[slots[4]] = Value::Int(pts.len() as i64);
            continue;
        };
        let base = ground[k].unwrap_or(zs[0]);
        let roof_pts: Vec<(f64, f64, f64)> = pts
            .iter()
            .filter(|p| p.2 > base + 1.5)
            .map(|p| {
                let (u, v) = frame.uv(p.0, p.1);
                (u, v, p.2)
            })
            .collect();
        let (roof, fit) = if roof_pts.len() >= 6 && !only_flat {
            fit_roof(&roof_pts, frame.l, frame.w)
        } else {
            let top = zs[((zs.len() - 1) as f64 * 0.9).round() as usize];
            (Roof::Flat(top), 0.0)
        };
        for poly in geom::polygons_of(g) {
            add_building(&mut mesh, &poly, &frame, &roof, base, k as u32);
        }
        let top = match roof {
            Roof::Flat(a) => a,
            _ => roof_pts.iter().map(|(u, v, _)| roof.z(*u, *v, frame.l, frame.w)).fold(f64::NEG_INFINITY, f64::max),
        };
        *counts.entry(roof.name()).or_default() += 1;
        f.attrs[slots[0]] = Value::Real(round(base, 2));
        f.attrs[slots[1]] = Value::Real(round(top - base, 2));
        f.attrs[slots[2]] = Value::Text(roof.name().into());
        f.attrs[slots[3]] = Value::Real(round(fit, 2));
        f.attrs[slots[4]] = Value::Int(pts.len() as i64);
        made += 1;
    }
    ctx.notes.info(format!(
        "{} buildings with roofs: {}",
        fmt_count(made as u64),
        counts.iter().map(|(n, c)| format!("{} {}", fmt_count(*c as u64), n)).collect::<Vec<_>>().join(", ")
    ));
    if without > 0 {
        ctx.notes.warn(format!("{} buildings have fewer than three points on them, and have no model.", fmt_count(without as u64)));
    }
    out.mesh = Some(Arc::new(mesh));
    Ok(vec![("Buildings".into(), out)])
}

// ---------- Terrain to 3D model (TerrainModel) ----------

const TERRAIN_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "most",
        label: "Points along a side",
        kind: ParamKind::Text,
        default: "500",
        hint: "At most this many points of height along the longer side: more is finer and larger. Cells are skipped to keep to it",
    },
    ParamSpec { key: "exaggerate", label: "Heights times", kind: ParamKind::Text, default: "1", hint: "Such as 2 to make hills twice as high" },
    ParamSpec {
        key: "base",
        label: "Base",
        kind: ParamKind::Choice(&[("none", "None: only the surface"), ("block", "A block under it, with sides and a bottom, to print or set on a table")]),
        default: "none",
        hint: "",
    },
    ParamSpec {
        key: "depth",
        label: "Block below the lowest",
        kind: ParamKind::Text,
        default: "",
        hint: "In metres. Empty: a tenth of the height of the terrain, at least 1",
    },
    ParamSpec {
        key: "texture_most",
        label: "Largest picture",
        kind: ParamKind::Text,
        default: "4096",
        hint: "Pixels along a side of the picture draped over the terrain; larger is made smaller",
    },
];

/// The part of a picture over a rectangle, at most `most` pixels on a side, as a texture.
fn texture_of(img: &crate::model::RasterImage, rect: [f64; 4], most: usize) -> Option<Texture> {
    let g = &img.geotransform;
    if g[2] != 0.0 || g[4] != 0.0 || g[1] == 0.0 || g[5] == 0.0 {
        return None;
    }
    let col = |x: f64| (x - g[0]) / g[1];
    let row = |y: f64| (y - g[3]) / g[5];
    let (c0, c1) = (col(rect[0]).min(col(rect[2])), col(rect[0]).max(col(rect[2])));
    let (r0, r1) = (row(rect[3]).min(row(rect[1])), row(rect[3]).max(row(rect[1])));
    if c1 <= 0.0 || r1 <= 0.0 || c0 >= img.width as f64 || r0 >= img.height as f64 {
        return None;
    }
    let (wpx, hpx) = ((c1 - c0).max(1.0), (r1 - r0).max(1.0));
    let scale = (most as f64 / wpx.max(hpx)).min(1.0);
    let (tw, th) = (((wpx * scale).round() as usize).max(1), ((hpx * scale).round() as usize).max(1));
    let mut rgba = vec![0u8; tw * th * 4];
    for ty in 0..th {
        for tx in 0..tw {
            let sx = (c0 + (tx as f64 + 0.5) / tw as f64 * wpx).floor();
            let sy = (r0 + (ty as f64 + 0.5) / th as f64 * hpx).floor();
            if sx < 0.0 || sy < 0.0 || sx as usize >= img.width || sy as usize >= img.height {
                continue;
            }
            let at = (sy as usize * img.width + sx as usize) * 4;
            rgba[(ty * tw + tx) * 4..(ty * tw + tx) * 4 + 4].copy_from_slice(&img.rgba[at..at + 4]);
        }
    }
    Some(Texture { width: tw, height: th, rgba })
}

fn run_terrain_model(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let heights = input(inp, "Heights");
    let Some(g) = heights.grid.as_deref() else {
        return err("The stream on Heights has no grid of heights. Read a terrain model (GeoTIFF) with Cells as set to Heights.");
    };
    if g.is_turned() {
        return err("The terrain model is turned, which this tool cannot work with yet.");
    }
    let most = number_param(p, "most", "Points along a side")?.unwrap_or(500.0).clamp(2.0, 5000.0) as usize;
    let k = number_param(p, "exaggerate", "Heights times")?.unwrap_or(1.0);
    let step = g.width.max(g.height).div_ceil(most).max(1);
    let (nx, ny) = ((g.width - 1) / step + 1, (g.height - 1) / step + 1);
    if nx < 2 || ny < 2 {
        return err("The terrain model is too small: it needs at least two cells each way.");
    }
    let gt = g.geotransform;
    let at = |c: usize, r: usize| -> [f64; 3] {
        let (cc, rr) = ((c * step).min(g.width - 1), (r * step).min(g.height - 1));
        let v = g.values[rr * g.width + cc];
        [gt[0] + (cc as f64 + 0.5) * gt[1], gt[3] + (rr as f64 + 0.5) * gt[5], v as f64 * k]
    };
    let pts: Vec<[f64; 3]> = (0..ny).flat_map(|r| (0..nx).map(move |c| (c, r))).map(|(c, r)| at(c, r)).collect();
    let rect = {
        let (a, b) = (pts[0], pts[pts.len() - 1]);
        [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
    };
    // The picture draped over it, when one comes on Picture.
    let picture = input(inp, "Picture");
    if picture.image.is_some() && picture.crs.is_some() && heights.crs.is_some() && picture.crs != heights.crs {
        return err(format!(
            "The picture is in {} and the terrain in {}. Read the picture in the system of the terrain, or reproject it first.",
            crs_label(picture.crs),
            crs_label(heights.crs)
        ));
    }
    let tex_most = number_param(p, "texture_most", "Largest picture")?.unwrap_or(4096.0).clamp(16.0, 16384.0) as usize;
    let texture = picture.image.as_deref().and_then(|img| texture_of(img, rect, tex_most));
    if picture.image.is_some() && texture.is_none() {
        ctx.notes.warn("The picture does not cover the terrain, and was left out.");
    }
    let mut mesh = Mesh {
        materials: vec![
            Material { name: "terrain".into(), diffuse: [0.55, 0.62, 0.45], texture: texture.map(Arc::new), ..Default::default() },
            Material { name: "base".into(), diffuse: [0.42, 0.36, 0.3], ..Default::default() },
        ],
        objects: vec!["terrain".into()],
        ..Default::default()
    };
    let textured = mesh.materials[0].texture.is_some();
    mesh.positions = pts.clone();
    if textured {
        mesh.uvs = pts.iter().map(|q| [((q[0] - rect[0]) / (rect[2] - rect[0])) as f32, ((q[1] - rect[1]) / (rect[3] - rect[1])) as f32]).collect();
    }
    let idx = |c: usize, r: usize| (r * nx + c) as u32;
    let mut holes = 0usize;
    for r in 0..ny - 1 {
        for c in 0..nx - 1 {
            let q = [idx(c, r), idx(c + 1, r), idx(c + 1, r + 1), idx(c, r + 1)];
            if q.iter().any(|i| !pts[*i as usize][2].is_finite()) {
                holes += 1;
                continue;
            }
            // Rows go south: the corners in this order turn clockwise seen from above.
            for t in [[q[0], q[2], q[1]], [q[0], q[3], q[2]]] {
                mesh.tris.push(Tri { v: t, n: [NONE; 3], t: if textured { t } else { [NONE; 3] }, material: 0, object: 0 });
            }
        }
    }
    if holes > 0 {
        ctx.notes.warn(format!("{} squares have cells without heights, and are holes in the surface.", fmt_count(holes as u64)));
    }
    let zs: Vec<f64> = pts.iter().map(|q| q[2]).filter(|z| z.is_finite()).collect();
    let (lo, hi) = zs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, z| (a.0.min(*z), a.1.max(*z)));
    if !lo.is_finite() {
        return err("The terrain model has no heights.");
    }
    if p.get("base").as_deref() == Some("block") {
        let depth = number_param(p, "depth", "Block below the lowest")?.unwrap_or(((hi - lo) / 10.0).max(1.0));
        let floor = lo - depth.abs();
        // The sides: down from the edge of the surface, all the way round.
        let mut edge: Vec<(usize, usize)> = (0..nx).map(|c| (c, 0)).collect();
        edge.extend((1..ny).map(|r| (nx - 1, r)));
        edge.extend((0..nx - 1).rev().map(|c| (c, ny - 1)));
        edge.extend((1..ny - 1).rev().map(|r| (0, r)));
        edge.push((0, 0));
        for w in edge.windows(2) {
            let (a, b) = (pts[idx(w[0].0, w[0].1) as usize], pts[idx(w[1].0, w[1].1) as usize]);
            if !a[2].is_finite() || !b[2].is_finite() {
                continue;
            }
            let v = mesh.positions.len() as u32;
            mesh.positions.extend_from_slice(&[[a[0], a[1], floor], [b[0], b[1], floor], b, a]);
            for t in [[v, v + 2, v + 1], [v, v + 3, v + 2]] {
                mesh.tris.push(Tri { v: t, n: [NONE; 3], t: [NONE; 3], material: 1, object: 0 });
            }
        }
        let v = mesh.positions.len() as u32;
        mesh.positions.extend_from_slice(&[[rect[0], rect[1], floor], [rect[2], rect[1], floor], [rect[2], rect[3], floor], [rect[0], rect[3], floor]]);
        for t in [[v, v + 2, v + 1], [v, v + 3, v + 2]] {
            mesh.tris.push(Tri { v: t, n: [NONE; 3], t: [NONE; 3], material: 1, object: 0 });
        }
    }
    let foot = geo::Rect::new(Coord { x: rect[0], y: rect[1] }, Coord { x: rect[2], y: rect[3] }).to_polygon();
    let out = Table {
        fields: vec![Field::new("name", FieldType::Text), Field::new("lowest", FieldType::Real), Field::new("highest", FieldType::Real)],
        crs: heights.crs,
        features: vec![Feature::new(
            vec![Value::Text("terrain".into()), Value::Real((lo / k * 100.0).round() / 100.0), Value::Real((hi / k * 100.0).round() / 100.0)],
            Some(Geometry::Polygon(foot)),
        )],
        mesh: Some(Arc::new(mesh)),
        ..Default::default()
    };
    ctx.notes.info(format!(
        "{} × {} points of height, {} triangles{}; heights {} to {} m",
        nx,
        ny,
        fmt_count(out.mesh.as_ref().map_or(0, |m| m.tris.len()) as u64),
        if textured { ", with the picture draped over" } else { "" },
        fmt_real((lo / k).round()),
        fmt_real((hi / k).round())
    ));
    Ok(vec![("Model".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "Lod2Builder",
        title: "LoD2 buildings from points",
        hidden: false,
        group: Group::Transformer,
        category: "Point clouds",
        desc: "3D models of the buildings on Buildings with the shapes of their roofs, fitted to the points of the cloud on Points that lie on them: flat, shed, gable or hipped, whichever fits best, with walls from the ground up to the edge of the roof. As the buildings of LoD2 in CityGML, for the 3D view, Render 3D scene and glTF for Blender. Each gets its ground, height, the shape of its roof and how far the points lie from it (roof_fit, in metres: large for roofs of other shapes).",
        inputs: &["Buildings", "Points", "Ground"],
        outputs: &["Buildings"],
        ports: None,
        params: LOD2_PARAMS,
        run: run_lod2,
        summary: |p| if p.get("shapes").as_deref() == Some("flat") { "flat roofs".into() } else { "fitted roofs".into() },
    },
    NodeType {
        name: "TerrainModel",
        title: "Terrain to 3D model",
        hidden: false,
        group: Group::Transformer,
        category: "3D models and views",
        desc: "Makes the heights of a raster on Heights into a 3D model of the terrain: a surface of triangles, with a picture on Picture, such as an orthophoto or a map from Draw map, draped over it. With a block under it if you like, to print or to set on a table. For glTF and Blender, the 3D view and Render 3D scene.",
        inputs: &["Heights", "Picture"],
        outputs: &["Model"],
        ports: None,
        params: TERRAIN_PARAMS,
        run: run_terrain_model,
        summary: |p| format!("at most {} points a side", p.get("most").unwrap_or_else(|| "500".into())),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roofs_of_each_shape_are_told_apart() {
        let (l, w) = (6.0, 4.0);
        let mut grid = Vec::new();
        for i in -5..=5 {
            for j in -3..=3 {
                grid.push((i as f64, j as f64 * 1.2));
            }
        }
        for want in [Roof::Flat(10.0), Roof::Gable { a: 8.0, b: 0.8, along: true }, Roof::Hip(8.0, 0.7), Roof::Shed(9.0, 0.0, 0.5)] {
            let pts: Vec<(f64, f64, f64)> = grid.iter().map(|(u, v)| (*u, *v, want.z(*u, *v, l, w))).collect();
            let (got, rms) = fit_roof(&pts, l, w);
            assert_eq!(got.name(), want.name(), "{:?} {}", got, rms);
            assert!(rms < 0.01);
        }
    }

    #[test]
    fn a_house_with_a_gable_roof_from_its_points() {
        // A house 12 × 8 m, ground at 50, eaves at 56 and the ridge at 59 along the long side.
        let sq = geo::Polygon::new(vec![(0.0, 0.0), (12.0, 0.0), (12.0, 8.0), (0.0, 8.0), (0.0, 0.0)].into(), vec![]);
        let buildings = Table { fields: vec![], crs: Some(3006), features: vec![Feature::new(vec![], Some(Geometry::Polygon(sq)))], ..Default::default() };
        let mut cloud = Table { fields: vec![Field::new("classification", FieldType::Int)], crs: Some(3006), ..Default::default() };
        for i in -4..=16 {
            for j in -4..=12 {
                let (x, y) = (i as f64 + 0.25, j as f64 + 0.25);
                let inside = (0.0..12.0).contains(&x) && (0.0..8.0).contains(&y);
                let z = if inside { 56.0 + 0.75 * (4.0 - (y - 4.0).abs()) } else { 50.0 };
                let mut f = Feature::new(vec![Value::Int(if inside { 6 } else { 2 })], Some(Geometry::Point(geo::Point::new(x, y))));
                f.z = Some(Arc::from(vec![z]));
                cloud.features.push(f);
            }
        }
        let (out, notes) = run_for_test("Lod2Builder", &[], vec![("Buildings", buildings), ("Points", cloud)]).unwrap();
        let b = &out[0].1;
        let at = |n: &str| b.features[0].attrs[b.field_index(n).unwrap()].clone();
        assert_eq!(at("roof_shape"), Value::Text("gable".into()), "{:?}", notes);
        assert_eq!(at("ground"), Value::Real(50.0));
        let m = b.mesh.as_ref().unwrap();
        let (lo, hi) = m.bounds().unwrap();
        assert!((lo[2] - 50.0).abs() < 1e-6 && (hi[2] - 59.0).abs() < 0.1, "{:?} {:?}", lo, hi);
        // The ridge is an edge: a corner of the roof lies on it.
        assert!(m.positions.iter().any(|q| (q[1] - 4.0).abs() < 1e-6 && (q[2] - 59.0).abs() < 0.1));
    }

    #[test]
    fn terrain_with_a_picture_draped_over_it() {
        let g = crate::model::Grid {
            width: 20,
            height: 10,
            values: (0..200).map(|i| (i % 20) as f32).collect(),
            geotransform: [1000.0, 10.0, 0.0, 2000.0, 0.0, -10.0],
            crs: Some(3006),
            bands: vec![],
        };
        let heights = Table { crs: Some(3006), grid: Some(Arc::new(g)), raster: true, ..Default::default() };
        let img = crate::model::RasterImage { width: 40, height: 20, rgba: vec![200; 40 * 20 * 4], geotransform: [1000.0, 5.0, 0.0, 2000.0, 0.0, -5.0] };
        let picture = Table { crs: Some(3006), image: Some(Arc::new(img)), raster: true, ..Default::default() };
        let (out, _) = run_for_test_with(
            "TerrainModel",
            serde_json::json!({"base": "block"}).as_object().unwrap().clone(),
            vec![("Heights", heights), ("Picture", picture)],
        )
        .unwrap();
        let m = out[0].1.mesh.as_ref().unwrap();
        assert!(m.materials[0].texture.is_some());
        assert_eq!(m.uvs.len(), 200);
        assert_eq!(m.tris.iter().filter(|t| t.material == 0).count(), 19 * 9 * 2);
        assert!(m.tris.iter().any(|t| t.material == 1), "the block");
    }
}
