//! What came to Render 3D scene in 0.96.1: roofs coloured by their kind, trees on points,
//! and ready places for the camera.

use super::*;
use crate::mesh::{Material, Mesh, Tri, NONE};
use crate::model::Table;

/// Ready places for the camera, as they are stored and named.
pub(super) const CAMERAS: &[(&str, &str)] = &[
    ("", "As set below"),
    ("birds_eye", "Bird's eye: from high up, in perspective"),
    ("axonometric", "Axonometric: parallel, from 35° up"),
    ("top", "Straight down, as a plan"),
];

/// The camera height, projection and lens angle of a ready place, or None to keep those set.
pub(super) fn camera_of(c: &str) -> Option<(f64, bool, f64)> {
    match c {
        "birds_eye" => Some((50.0, false, 30.0)),
        "axonometric" => Some((35.264, true, 35.0)),
        "top" => Some((89.9, true, 35.0)),
        _ => None,
    }
}

/// The colour of a roof of a kind, as sRGB from 0 to 1: by words in English or Swedish, a
/// colour written as #rrggbb, or else one of a few roof colours, the same for the same kind.
pub(super) fn roof_colour(kind: &str) -> [f32; 3] {
    let k = kind.trim().to_lowercase();
    if let Ok(Some(c)) = crate::mapdraw::parse_color(&k).map(|c| c.filter(|_| k.starts_with('#'))) {
        return [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
    }
    let has = |words: &[&str]| words.iter().any(|w| k.contains(w));
    if has(&["flat", "platt"]) {
        [0.40, 0.40, 0.41]
    } else if has(&["hip", "valm"]) {
        [0.45, 0.21, 0.16]
    } else if has(&["gable", "sadel"]) {
        [0.63, 0.25, 0.18]
    } else if has(&["shed", "mono", "pulpet"]) {
        [0.33, 0.37, 0.43]
    } else if has(&["mansard", "gambrel", "brut"]) {
        [0.30, 0.27, 0.26]
    } else if has(&["dome", "kupol", "copper", "koppar"]) {
        [0.42, 0.58, 0.50]
    } else if has(&["green", "grön", "sedum"]) {
        [0.36, 0.48, 0.25]
    } else {
        const OTHERS: &[[f32; 3]] = &[[0.55, 0.30, 0.22], [0.28, 0.29, 0.31], [0.50, 0.44, 0.38], [0.24, 0.30, 0.38]];
        let h = k.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
        OTHERS[h as usize % OTHERS.len()]
    }
}

/// The model of a stream with the surfaces that face up, its roofs, coloured by the kind of
/// roof each feature has in an attribute. Also the number of kinds.
pub(super) fn colour_roofs(mesh: &Mesh, table: &Table, attr: &str) -> Option<(Mesh, usize)> {
    let col = table.fields.iter().position(|f| f.name == attr)?;
    let mut out = mesh.clone();
    let mut kinds: Vec<String> = Vec::new();
    for tri in out.tris.iter_mut() {
        let Some(f) = table.features.get(tri.object as usize) else { continue };
        let kind = f.attrs.get(col).map(|v| v.to_text()).unwrap_or_default();
        if kind.trim().is_empty() {
            continue;
        }
        let [a, b, c] = tri.v.map(|i| mesh.positions[i as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        // A roof faces up, also when it is steep; a wall stands up.
        if len == 0.0 || n[2] / len < 0.3 {
            continue;
        }
        let k = match kinds.iter().position(|x| *x == kind) {
            Some(k) => k,
            None => {
                kinds.push(kind.clone());
                out.materials.push(Material {
                    name: format!("roof {}", kind),
                    diffuse: roof_colour(&kind),
                    specular: [0.05; 3],
                    shininess: 20.0,
                    ..Material::default()
                });
                kinds.len() - 1
            }
        };
        tri.material = (mesh.materials.len() + k) as u32;
        tri.t = [NONE; 3];
    }
    Some((out, kinds.len()))
}

/// Simple trees on points, as one model: a trunk and a round crown, or a cone for a
/// conifer, standing on `ground`.
pub(super) fn trees(points: &[([f64; 2], f64, bool)], ground: &dyn Fn(f64, f64) -> f64) -> Mesh {
    let mut m = Mesh {
        materials: vec![
            Material { name: "trunk".into(), diffuse: [0.36, 0.26, 0.18], ..Material::default() },
            Material { name: "leaves".into(), diffuse: [0.30, 0.46, 0.21], ..Material::default() },
            Material { name: "needles".into(), diffuse: [0.16, 0.32, 0.19], ..Material::default() },
        ],
        ..Mesh::default()
    };
    let sides = 8;
    for (k, (at, height, conifer)) in points.iter().enumerate() {
        let (x, y) = (at[0], at[1]);
        let z0 = ground(x, y);
        let h = height.max(1.0);
        m.objects.push(format!("tree {}", k + 1));
        let object = k as u32;
        // The trunk, a prism of eight sides.
        let r = (h * 0.035).max(0.08);
        let top = z0 + h * if *conifer { 0.25 } else { 0.45 };
        let ring = |z: f64, r: f64| -> Vec<[f64; 3]> {
            (0..sides)
                .map(|s| {
                    let a = s as f64 / sides as f64 * std::f64::consts::TAU;
                    [x + r * a.cos(), y + r * a.sin(), z]
                })
                .collect()
        };
        let base = m.positions.len() as u32;
        m.positions.extend(ring(z0, r));
        m.positions.extend(ring(top, r));
        for s in 0..sides as u32 {
            let n = (s + 1) % sides as u32;
            m.tris.push(Tri { v: [base + s, base + n, base + sides as u32 + n], n: [NONE; 3], t: [NONE; 3], material: 0, object });
            m.tris.push(Tri { v: [base + s, base + sides as u32 + n, base + sides as u32 + s], n: [NONE; 3], t: [NONE; 3], material: 0, object });
        }
        if *conifer {
            // A cone from a quarter of the height to the top.
            let cr = h * 0.22;
            let b2 = m.positions.len() as u32;
            m.positions.extend(ring(z0 + h * 0.2, cr));
            m.positions.push([x, y, z0 + h]);
            m.positions.push([x, y, z0 + h * 0.2]);
            let (tip, mid) = (b2 + sides as u32, b2 + sides as u32 + 1);
            for s in 0..sides as u32 {
                let n = (s + 1) % sides as u32;
                m.tris.push(Tri { v: [b2 + s, b2 + n, tip], n: [NONE; 3], t: [NONE; 3], material: 2, object });
                m.tris.push(Tri { v: [b2 + n, b2 + s, mid], n: [NONE; 3], t: [NONE; 3], material: 2, object });
            }
        } else {
            // A round crown: a sphere of rings, a little flattened.
            let cr = h * 0.3;
            let cz = z0 + h - cr * 0.95;
            let (rings, b3) = (6usize, m.positions.len() as u32);
            m.positions.push([x, y, cz + cr * 0.95]);
            for i in 1..rings {
                let phi = i as f64 / rings as f64 * std::f64::consts::PI;
                for s in 0..sides {
                    let a = s as f64 / sides as f64 * std::f64::consts::TAU;
                    m.positions.push([x + cr * phi.sin() * a.cos(), y + cr * phi.sin() * a.sin(), cz + cr * 0.95 * phi.cos()]);
                }
            }
            m.positions.push([x, y, cz - cr * 0.95]);
            let at = |i: usize, s: usize| b3 + 1 + ((i - 1) * sides + s % sides) as u32;
            let bottom = b3 + 1 + ((rings - 1) * sides) as u32;
            for s in 0..sides {
                m.tris.push(Tri { v: [b3, at(1, s), at(1, s + 1)], n: [NONE; 3], t: [NONE; 3], material: 1, object });
                m.tris.push(Tri { v: [bottom, at(rings - 1, s + 1), at(rings - 1, s)], n: [NONE; 3], t: [NONE; 3], material: 1, object });
                for i in 1..rings - 1 {
                    m.tris.push(Tri { v: [at(i, s), at(i + 1, s), at(i + 1, s + 1)], n: [NONE; 3], t: [NONE; 3], material: 1, object });
                    m.tris.push(Tri { v: [at(i, s), at(i + 1, s + 1), at(i, s + 1)], n: [NONE; 3], t: [NONE; 3], material: 1, object });
                }
            }
        }
    }
    m
}

/// The trees of the points on Trees: where, how high and whether a conifer.
pub(super) fn tree_points(p: &P, t: &Table) -> Result<Vec<([f64; 2], f64, bool)>> {
    let height = super::geom_transform::number_param(p, "tree_height", "The height of the trees")?.unwrap_or(10.0).max(1.0);
    let by = p.get("tree_height_by").filter(|s| !s.trim().is_empty());
    let hcol = match &by {
        Some(a) => Some(t.fields.iter().position(|f| &f.name == a).ok_or_else(|| GbError(format!("Trees has no attribute {} for the height.", a)))?),
        None => None,
    };
    let kind = p.get("tree_kind").unwrap_or_else(|| "leafy".into());
    let kcol = p.get("tree_kind_by").filter(|s| !s.trim().is_empty()).and_then(|a| t.fields.iter().position(|f| f.name == a));
    let conifer_word = |s: &str| {
        let s = s.to_lowercase();
        ["conifer", "pine", "spruce", "fir", "barr", "gran", "tall", "fur"].iter().any(|w| s.contains(w))
    };
    let mut out = Vec::new();
    for f in &t.features {
        let Some(g) = &f.geom else { continue };
        let pts: Vec<geo::Point<f64>> = match g {
            geo::Geometry::Point(q) => vec![*q],
            geo::Geometry::MultiPoint(m) => m.0.clone(),
            _ => continue,
        };
        let h = hcol.and_then(|c| f.attrs.get(c)).and_then(|v| v.as_f64()).filter(|v| *v > 0.0).unwrap_or(height);
        let conifer = match kcol {
            Some(c) => f.attrs.get(c).is_some_and(|v| conifer_word(&v.to_text())),
            None => kind == "conifer",
        };
        for q in pts {
            out.push(([q.x(), q.y()], h, conifer));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roofs_get_the_colour_of_their_kind_and_trees_stand_on_the_ground() {
        assert_eq!(roof_colour("Sadeltak"), roof_colour("gable"));
        assert_ne!(roof_colour("flat"), roof_colour("hipped"));
        assert_eq!(roof_colour("#ff0000"), [1.0, 0.0, 0.0]);
        let m = trees(&[([0.0, 0.0], 10.0, false), ([20.0, 0.0], 12.0, true)], &|_, _| 5.0);
        assert_eq!(m.objects.len(), 2);
        let (lo, hi) = m.bounds().unwrap();
        assert!((lo[2] - 5.0).abs() < 1e-9 && (hi[2] - 17.0).abs() < 1e-9, "{:?} {:?}", lo, hi);
        assert!(m.tris.iter().all(|t| t.v.iter().all(|v| (*v as usize) < m.positions.len())));
    }
}
