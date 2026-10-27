//! Renders 3D models, such as buildings from OBJ files, into pictures by following rays of
//! light: from the eye through each pixel to the nearest surface, and from there towards
//! the sun, to see whether it is in shadow, and towards the sky, which lights what the sun
//! does not and darkens corners. Glass lets light through and mirrors the sky. Several
//! rays go through each pixel, so that edges are smooth and soft shadows are smooth.
//!
//! The triangles are sorted into a bounding volume hierarchy, a tree of boxes, so that a
//! ray only meets the few triangles near its way.

use crate::mesh::{Material, Mesh, NONE};
use crate::model::{Grid, RasterImage};
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 {
        mul(a, 1.0 / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}
fn hadamard(a: V3, b: V3) -> V3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

/// From sRGB, as colours are given, to light that adds up.
fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// A small, quick random number generator, one for each row of the picture, so that a
/// picture comes out the same every time.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
}

/// Where a triangle comes from: a model, or the terrain.
#[derive(Clone, Copy)]
struct Prim {
    /// Index into the triangles of the mesh, or of the terrain when `terrain`.
    index: u32,
    terrain: bool,
}

#[derive(Clone, Copy)]
struct Node {
    lo: V3,
    hi: V3,
    /// For a leaf: the first primitive and how many; for a branch: the second child, the
    /// first being the next node, and count 0.
    start: u32,
    count: u32,
}

struct Bvh {
    nodes: Vec<Node>,
    prims: Vec<Prim>,
    /// The corners of each primitive, in the same order, as v0, e1, e2.
    tris: Vec<[V3; 3]>,
}

fn build(prims: Vec<Prim>, corners: Vec<[V3; 3]>) -> Bvh {
    let n = prims.len();
    let centre: Vec<V3> =
        corners.iter().map(|c| [(c[0][0] + c[1][0] + c[2][0]) / 3.0, (c[0][1] + c[1][1] + c[2][1]) / 3.0, (c[0][2] + c[1][2] + c[2][2]) / 3.0]).collect();
    let mut order: Vec<u32> = (0..n as u32).collect();
    let mut nodes: Vec<Node> = Vec::with_capacity(2 * n / 3 + 1);
    let bounds = |ids: &[u32]| -> (V3, V3) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for &i in ids {
            for p in &corners[i as usize] {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        (lo, hi)
    };
    // Built with an explicit stack: (start, end, index of the node to fill).
    nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], start: 0, count: 0 });
    let mut stack = vec![(0usize, n, 0usize)];
    while let Some((s, e, at)) = stack.pop() {
        let (lo, hi) = bounds(&order[s..e]);
        let count = e - s;
        if count <= 4 {
            nodes[at] = Node { lo, hi, start: s as u32, count: count as u32 };
            continue;
        }
        // Split by the surface area heuristic over 12 bins along the longest side of the
        // centres.
        let mut clo = [f32::INFINITY; 3];
        let mut chi = [f32::NEG_INFINITY; 3];
        for &i in &order[s..e] {
            for k in 0..3 {
                clo[k] = clo[k].min(centre[i as usize][k]);
                chi[k] = chi[k].max(centre[i as usize][k]);
            }
        }
        let axis = (0..3).max_by(|a, b| (chi[*a] - clo[*a]).total_cmp(&(chi[*b] - clo[*b]))).unwrap_or(0);
        let span = chi[axis] - clo[axis];
        let mid = if span <= 0.0 {
            s + count / 2
        } else {
            const B: usize = 12;
            let bin = |i: u32| (((centre[i as usize][axis] - clo[axis]) / span * B as f32) as usize).min(B - 1);
            let mut bcount = [0usize; B];
            let mut blo = [[f32::INFINITY; 3]; B];
            let mut bhi = [[f32::NEG_INFINITY; 3]; B];
            for &i in &order[s..e] {
                let b = bin(i);
                bcount[b] += 1;
                for p in &corners[i as usize] {
                    for k in 0..3 {
                        blo[b][k] = blo[b][k].min(p[k]);
                        bhi[b][k] = bhi[b][k].max(p[k]);
                    }
                }
            }
            let area = |lo: V3, hi: V3| {
                let d = sub(hi, lo);
                if d[0] < 0.0 {
                    0.0
                } else {
                    d[0] * d[1] + d[1] * d[2] + d[2] * d[0]
                }
            };
            let mut best = (f32::INFINITY, B / 2);
            for split in 1..B {
                let (mut l, mut r) = (([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]), ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]));
                let (mut nl, mut nr) = (0, 0);
                for b in 0..B {
                    let side = if b < split { (&mut l, &mut nl) } else { (&mut r, &mut nr) };
                    *side.1 += bcount[b];
                    for k in 0..3 {
                        side.0 .0[k] = side.0 .0[k].min(blo[b][k]);
                        side.0 .1[k] = side.0 .1[k].max(bhi[b][k]);
                    }
                }
                if nl == 0 || nr == 0 {
                    continue;
                }
                let cost = area(l.0, l.1) * nl as f32 + area(r.0, r.1) * nr as f32;
                if cost < best.0 {
                    best = (cost, split);
                }
            }
            let slice = &mut order[s..e];
            let mut i = 0;
            for j in 0..slice.len() {
                if bin(slice[j]) < best.1 {
                    slice.swap(i, j);
                    i += 1;
                }
            }
            if i == 0 || i == count {
                s + count / 2
            } else {
                s + i
            }
        };
        // Children: the first right after this node, the second after the first's subtree.
        // Built depth first, so the second child's place is set when it is made.
        let left = nodes.len();
        nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], start: 0, count: 0 });
        let right = nodes.len();
        nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], start: 0, count: 0 });
        nodes[at] = Node { lo, hi, start: left as u32, count: 0 };
        // A branch keeps both children's places: start is the left, and right follows it.
        let _ = right;
        stack.push((mid, e, right));
        stack.push((s, mid, left));
    }
    let prims_sorted: Vec<Prim> = order.iter().map(|i| prims[*i as usize]).collect();
    let tris: Vec<[V3; 3]> = order
        .iter()
        .map(|i| {
            let c = corners[*i as usize];
            [c[0], sub(c[1], c[0]), sub(c[2], c[0])]
        })
        .collect();
    Bvh { nodes, prims: prims_sorted, tris }
}

/// Where a ray met a triangle: how far, which, and where on it.
#[derive(Clone, Copy)]
struct Hit {
    t: f32,
    k: u32,
    u: f32,
    v: f32,
}

impl Bvh {
    fn slab(n: &Node, o: V3, inv: V3, tmax: f32) -> Option<f32> {
        let mut t0 = 0.0f32;
        let mut t1 = tmax;
        for k in 0..3 {
            let a = (n.lo[k] - o[k]) * inv[k];
            let b = (n.hi[k] - o[k]) * inv[k];
            let (a, b) = if a < b { (a, b) } else { (b, a) };
            t0 = t0.max(a);
            t1 = t1.min(b);
            if t0 > t1 {
                return None;
            }
        }
        Some(t0)
    }

    fn tri(&self, k: usize, o: V3, d: V3, tmax: f32) -> Option<(f32, f32, f32)> {
        let [v0, e1, e2] = self.tris[k];
        let p = cross(d, e2);
        let det = dot(e1, p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = sub(o, v0);
        let u = dot(s, p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = cross(s, e1);
        let v = dot(d, q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = dot(e2, q) * inv;
        (t > 1e-4 && t < tmax).then_some((t, u, v))
    }

    /// The nearest triangle along a ray, if any nearer than `tmax`.
    fn closest(&self, o: V3, d: V3, tmax: f32) -> Option<Hit> {
        if self.nodes.is_empty() || self.tris.is_empty() {
            return None;
        }
        let inv = d.map(|x| if x.abs() > 1e-12 { 1.0 / x } else { 1e12f32.copysign(x) });
        let mut best: Option<Hit> = None;
        let mut limit = tmax;
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let ni = stack[sp] as usize;
            let n = &self.nodes[ni];
            if Self::slab(n, o, inv, limit).is_none() {
                continue;
            }
            if n.count > 0 {
                for k in n.start as usize..(n.start + n.count) as usize {
                    if let Some((t, u, v)) = self.tri(k, o, d, limit) {
                        limit = t;
                        best = Some(Hit { t, k: k as u32, u, v });
                    }
                }
            } else if sp + 2 <= stack.len() {
                // The nearer child first.
                let (a, b) = (n.start as usize, n.start as usize + 1);
                let ta = Self::slab(&self.nodes[a], o, inv, limit);
                let tb = Self::slab(&self.nodes[b], o, inv, limit);
                match (ta, tb) {
                    (Some(x), Some(y)) if x <= y => {
                        stack[sp] = b as u32;
                        stack[sp + 1] = a as u32;
                        sp += 2;
                    }
                    (Some(_), Some(_)) => {
                        stack[sp] = a as u32;
                        stack[sp + 1] = b as u32;
                        sp += 2;
                    }
                    (Some(_), None) => {
                        stack[sp] = a as u32;
                        sp += 1;
                    }
                    (None, Some(_)) => {
                        stack[sp] = b as u32;
                        sp += 1;
                    }
                    (None, None) => {}
                }
            }
        }
        best
    }

    /// How much light gets through along a ray: 1 when nothing is in the way, 0 behind
    /// something solid, and between behind glass.
    fn through(&self, o: V3, d: V3, tmax: f32, opacity: &dyn Fn(Prim) -> f32) -> f32 {
        if self.nodes.is_empty() || self.tris.is_empty() {
            return 1.0;
        }
        let inv = d.map(|x| if x.abs() > 1e-12 { 1.0 / x } else { 1e12f32.copysign(x) });
        let mut light = 1.0f32;
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if Self::slab(n, o, inv, tmax).is_none() {
                continue;
            }
            if n.count > 0 {
                for k in n.start as usize..(n.start + n.count) as usize {
                    if self.tri(k, o, d, tmax).is_some() {
                        let a = opacity(self.prims[k]);
                        if a >= 0.98 {
                            return 0.0;
                        }
                        light *= 1.0 - a;
                        if light < 0.02 {
                            return 0.0;
                        }
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = n.start;
                stack[sp + 1] = n.start + 1;
                sp += 2;
            }
        }
        light
    }
}

/// The background behind and around the models.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Background {
    /// A light, even room, as in a photo studio, with a floor that takes the shadows.
    Studio,
    /// The sky as it is with the sun where it stands: blue at noon, warm at sunset, with
    /// clouds when there are any.
    Sky,
    /// See-through, to put the picture on something else.
    Clear,
}

/// What the models are made of in the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Look {
    /// Their own materials and textures.
    Materials,
    /// All white, as an architect's model, but glass still glass.
    White,
    /// All light wood, as a model of a city in a planning office.
    Wood,
}

/// Water: a surface at a height, everywhere or within some areas.
#[derive(Debug, Clone)]
pub struct Water {
    /// The height of the surface, in the coordinates of the models.
    pub level: f64,
    /// The areas of water, in the coordinates of the models. Empty: everywhere, as a sea.
    pub areas: Vec<geo::Polygon<f64>>,
    /// How rough the surface is: 0 still, 1 a fresh breeze.
    pub waves: f32,
}

/// How a scene is seen and lit.
#[derive(Debug, Clone)]
pub struct Settings {
    pub width: usize,
    pub height: usize,
    /// Where the camera looks from, clockwise from north, and how high above the
    /// horizon, in degrees.
    pub azimuth: f64,
    pub elevation: f64,
    /// The angle the picture spans from top to bottom, in degrees.
    pub fov: f64,
    /// More than 1 comes closer.
    pub zoom: f64,
    /// How far the camera stands from what it looks at, in the units of the scene, such as
    /// metres. None stands it where everything is seen, by `zoom`.
    pub distance: Option<f64>,
    /// The colours of a terrain with no map on it: by the height, or None for grass.
    pub terrain_colours: Option<crate::terrain::Colours>,
    /// Where the sun is, in degrees, and how wide it is seen: 0.5 is the real sun, more
    /// gives softer shadows.
    pub sun_azimuth: f64,
    pub sun_altitude: f64,
    pub sun_size: f64,
    /// How strong the sun and the sky are, 1 as on a clear day.
    pub sun_strength: f32,
    pub sky_strength: f32,
    /// Rays per pixel.
    pub samples: usize,
    pub background: Background,
    /// A floor under the models, at their lowest point, when there is no terrain.
    pub ground: bool,
    /// How much of the sky is cloud, 0 to 1. Clouds dim the sun and soften the shadows,
    /// and are seen in a sky.
    pub clouds: f32,
    /// How hazy the air is, 0 to 1: far things fade into the colour of the horizon.
    pub haze: f32,
    /// How many times light bounces between surfaces before it reaches the eye: 1 is quick,
    /// 3 lights courtyards and arcades as they are.
    pub bounces: u32,
    /// Whether smooth and metallic surfaces mirror what is around them.
    pub reflections: bool,
    pub look: Look,
    /// Materials whose names hold any of these words, in small letters, are made glass.
    pub glass_names: Vec<String>,
    pub water: Option<Water>,
    /// Materials whose names hold any of these words, in small letters, give off light in
    /// their own colour, as lamps and signs.
    pub glow_names: Vec<String>,
    /// The share of the windows that are lit from inside, 0 to 1: panes of glass, and the
    /// windows drawn on plain walls.
    pub lit_windows: f32,
    /// Rows of windows drawn on plain walls without a texture, a window every 2.6 m and a
    /// floor every 3 m, as on blocks from Extrude.
    pub windows: bool,
    /// Lights at points, such as street lamps.
    pub lights: Vec<PointLight>,
    /// How much is blurred in front of and behind the focus, 0 to 1: 0 all sharp, 0.3 as a
    /// model photographed close up.
    pub blur: f32,
    /// How far from the camera things are sharp. None: what it looks at.
    pub focus: Option<f64>,
    /// Parallel rays, as in an isometric drawing or a model seen from far off with a long
    /// lens: what is far is as large as what is near. The lens angle is then not used.
    pub parallel: bool,
    /// The colour of the sunlight and of the light from the sky, each times its own: warm
    /// sun and cool shade make a livelier picture than white light. 1, 1, 1 changes nothing.
    pub sun_tint: [f32; 3],
    pub sky_tint: [f32; 3],
    /// How much stronger the colours are made, 0 for as they are: 0.15 is a little livelier.
    pub vivid: f32,
}

/// A light at a point, as a street lamp, in the coordinates of the models.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    pub at: [f64; 3],
    /// Its colour as sRGB from 0 to 1.
    pub colour: [f32; 3],
    /// 1 is a street lamp.
    pub strength: f32,
}

/// The colour of light bulbs and lit rooms: warm white, about 2700 K.
pub const WARM_LIGHT: [f32; 3] = [1.0, 0.78, 0.52];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            width: 1600,
            height: 1000,
            azimuth: 215.0,
            elevation: 25.0,
            fov: 35.0,
            zoom: 1.0,
            distance: None,
            terrain_colours: None,
            sun_azimuth: 160.0,
            sun_altitude: 45.0,
            sun_size: 3.0,
            sun_strength: 1.0,
            sky_strength: 1.0,
            samples: 16,
            background: Background::Studio,
            ground: true,
            clouds: 0.0,
            haze: 0.0,
            bounces: 1,
            reflections: false,
            look: Look::Materials,
            glass_names: Vec::new(),
            water: None,
            parallel: false,
            sun_tint: [1.0; 3],
            sky_tint: [1.0; 3],
            vivid: 0.0,
            glow_names: Vec::new(),
            lit_windows: 0.0,
            windows: false,
            lights: Vec::new(),
            blur: 0.0,
            focus: None,
        }
    }
}

// ---------- The sky ----------

/// The sky of Preetham, Shirley and Smits (1999): its light in every direction from the
/// height of the sun and how clear the air is, as luminance and colour (Y, x, y).
struct Preetham {
    sun: V3,
    coef: [[f32; 5]; 3],
    zenith: [f32; 3],
    denom: [f32; 3],
    /// Darker as the sun goes below the horizon.
    dusk: f32,
    /// How low the sun is, 0 above 12° to 1 at the horizon, for the colours of evening.
    low: f32,
}

fn perez(c: &[f32; 5], cos_t: f32, gamma: f32, cos_g: f32) -> f32 {
    (1.0 + c[0] * (c[1] / cos_t.max(0.02)).exp()) * (1.0 + c[2] * (c[3] * gamma).exp() + c[4] * cos_g * cos_g)
}

impl Preetham {
    fn new(sun: V3, turbidity: f32) -> Preetham {
        let t = turbidity.clamp(1.8, 10.0);
        // Below the horizon the sky of a sun just above it, darkened: dusk.
        let alt = sun[2].clamp(-1.0, 1.0).asin();
        let dusk = if alt < 0.02 { (((alt - 0.02) * 180.0 / std::f32::consts::PI) / 3.0).exp() } else { 1.0 };
        let alt = alt.max(0.02);
        let theta_s = std::f32::consts::FRAC_PI_2 - alt;
        let sun = norm([sun[0], sun[1], alt.sin() * (sun[0] * sun[0] + sun[1] * sun[1]).sqrt().max(1e-3) / alt.cos()]);
        let coef = [
            [0.1787 * t - 1.4630, -0.3554 * t + 0.4275, -0.0227 * t + 5.3251, 0.1206 * t - 2.5771, -0.0670 * t + 0.3703],
            [-0.0193 * t - 0.2592, -0.0665 * t + 0.0008, -0.0004 * t + 0.2125, -0.0641 * t - 0.8989, -0.0033 * t + 0.0452],
            [-0.0167 * t - 0.2608, -0.0950 * t + 0.0092, -0.0079 * t + 0.2102, -0.0441 * t - 1.6537, -0.0109 * t + 0.0529],
        ];
        let chi = (4.0 / 9.0 - t / 120.0) * (std::f32::consts::PI - 2.0 * theta_s);
        let yz = ((4.0453 * t - 4.9710) * chi.tan() - 0.2155 * t + 2.4192).max(0.05);
        let (t3, t2, t1) = (theta_s.powi(3), theta_s.powi(2), theta_s);
        let xz = t * t * (0.00166 * t3 - 0.00375 * t2 + 0.00209 * t1)
            + t * (-0.02903 * t3 + 0.06377 * t2 - 0.03202 * t1 + 0.00394)
            + (0.11693 * t3 - 0.21196 * t2 + 0.06052 * t1 + 0.25886);
        let yzc = t * t * (0.00275 * t3 - 0.00610 * t2 + 0.00317 * t1)
            + t * (-0.04214 * t3 + 0.08970 * t2 - 0.04153 * t1 + 0.00516)
            + (0.15346 * t3 - 0.26756 * t2 + 0.06670 * t1 + 0.26688);
        let denom = [0, 1, 2].map(|k| perez(&coef[k], 1.0, theta_s, theta_s.cos()));
        let low = ((12.0 - alt.to_degrees()) / 12.0).clamp(0.0, 1.0).powf(1.5);
        Preetham { sun, coef, zenith: [yz, xz, yzc], denom, dusk, low }
    }

    /// The light of the sky in a direction, in linear RGB, about 0.5 at the zenith at noon.
    fn light(&self, d: V3) -> V3 {
        let d = if d[2] < 0.01 { norm([d[0], d[1], 0.01]) } else { d };
        let cos_g = dot(d, self.sun).clamp(-1.0, 1.0);
        let gamma = cos_g.acos();
        let v = [0, 1, 2].map(|k| self.zenith[k] * perez(&self.coef[k], d[2], gamma, cos_g) / self.denom[k]);
        let (y, x, yc) = (v[0], v[1], v[2].max(1e-3));
        let big_x = x / yc * y;
        let big_z = (1.0 - x - yc) / yc * y;
        let rgb = [3.2406 * big_x - 1.5372 * y - 0.4986 * big_z, -0.9689 * big_x + 1.8758 * y + 0.0415 * big_z, 0.0557 * big_x - 0.2040 * y + 1.0570 * big_z];
        // From kilocandela to the scale of this renderer, with dusk taking the light away
        // and leaving the blue.
        let k = 0.055 * self.dusk;
        let blue = 1.0 - self.dusk;
        // The model leans to magenta near the horizon; a little more green sets it right.
        let mut c = [rgb[0].max(0.0) * k * (1.0 - 0.6 * blue) * 0.97, rgb[1].max(0.0) * k * (1.0 - 0.3 * blue) * 1.04, rgb[2].max(0.0) * k];
        // With the sun low the model is pale all over. As at sunset, it is made orange
        // towards the sun and blue away from it and overhead.
        if self.low > 0.0 {
            let toward = cos_g.max(0.0).powi(3) * (1.0 - d[2]).powi(2);
            let away = (1.0 - toward) * self.low;
            let toward = toward * self.low;
            let tint = [1.0 + 0.3 * toward - 0.6 * away, 1.0 - 0.3 * toward - 0.3 * away, 1.0 - 0.8 * toward + 0.6 * away];
            // The bright glow round the sun is held back, so that its colour shows.
            let glow = 1.0 / (1.0 + 0.6 * toward);
            let tint = tint.map(|t| t * glow);
            c = [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2]];
        }
        c
    }
}

/// Noise that is smooth from place to place, from 0 to 1, for the clouds and the waves.
fn value_noise(x: f32, y: f32) -> f32 {
    let hash = |i: i32, j: i32| -> f32 {
        let mut h = (i as u32).wrapping_mul(374_761_393) ^ (j as u32).wrapping_mul(668_265_263);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
    };
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (i, j) = (xi as i32, yi as i32);
    let a = hash(i, j) + (hash(i + 1, j) - hash(i, j)) * sx;
    let b = hash(i, j + 1) + (hash(i + 1, j + 1) - hash(i, j + 1)) * sx;
    a + (b - a) * sy
}

fn fbm(x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut total) = (0.0, 0.5, 1.0, 0.0);
    for _ in 0..octaves {
        sum += amp * value_noise(x * freq, y * freq);
        total += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / total
}

/// The sky that lights the scene and is seen behind it.
struct Sky {
    background: Background,
    model: Option<Preetham>,
    clouds: f32,
    /// The colour of a cloud: lit by the sun from above, grey beneath.
    cloud_lit: V3,
    cloud_grey: V3,
    /// The colour the light from the sky is given.
    tint: V3,
}

impl Sky {
    /// The light from a direction, with the clouds spread out evenly: what lights surfaces.
    fn light(&self, d: V3) -> V3 {
        hadamard(self.untinted(d), self.tint)
    }

    fn untinted(&self, d: V3) -> V3 {
        let clear = match (&self.model, self.background) {
            (Some(m), Background::Sky) => m.light(d),
            _ => {
                // An even, white room, a little lighter above.
                let t = d[2].clamp(-1.0, 1.0) * 0.5 + 0.5;
                mul(add(mul([0.80, 0.81, 0.84], 1.0 - t), mul([1.0, 1.0, 1.0], t)), 0.62)
            }
        };
        if self.clouds <= 0.0 {
            return clear;
        }
        // An overcast sky is grey, lightest overhead, as the CIE describes it.
        let overcast = mul(self.cloud_grey, (1.0 + 2.0 * d[2].max(0.0)) / 3.0 * 1.6);
        let c = self.clouds.powf(0.7);
        add(mul(clear, 1.0 - c), mul(overcast, c))
    }

    /// The sky as the eye sees it: with the clouds where they are.
    fn seen(&self, d: V3) -> V3 {
        let base = match (&self.model, self.background) {
            (Some(m), Background::Sky) => m.light(d),
            _ => return self.light(d),
        };
        if self.clouds <= 0.0 || d[2] <= 0.0 {
            return if self.clouds > 0.0 { self.light(d) } else { base };
        }
        // Clouds on a layer high up: where a ray reaches it, how much cloud there is.
        let k = 1.0 / (d[2] + 0.06);
        let (x, y) = (d[0] * k * 1.6 + 7.3, d[1] * k * 1.6 - 3.1);
        let n = fbm(x, y, 5);
        let edge = 1.0 - self.clouds;
        let dense = ((n - edge + 0.12) / 0.3).clamp(0.0, 1.0);
        let dense = dense * dense * (3.0 - 2.0 * dense) * ((d[2] - 0.015) / 0.12).clamp(0.0, 1.0);
        // Thicker clouds are greyer underneath.
        let thick = ((n - edge) / 0.4).clamp(0.0, 1.0);
        let cloud = add(mul(self.cloud_lit, 1.0 - 0.55 * thick), mul(self.cloud_grey, 0.55 * thick));
        let behind = mul(add(mul(base, 1.0 - self.clouds), mul(self.light(d), self.clouds)), 1.0);
        add(mul(behind, 1.0 - dense), mul(cloud, dense))
    }
}

/// From light to the colours of the picture: a filmic curve, so that bright parts are not
/// cut off, and then sRGB.
fn tone(c: V3) -> [u8; 3] {
    let aces = |x: f32| {
        let x = x * 0.52;
        ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
    };
    c.map(|v| (srgb(aces(v)) * 255.0 + 0.5) as u8)
}

/// The sun seen through the air: weaker and redder the lower it stands, as more air is in
/// the way (Rayleigh and aerosol), and the same at noon as before.
fn sun_colour(altitude: f32, haze: f32) -> V3 {
    let z = (90.0 - altitude.to_degrees()).clamp(0.0, 90.0);
    // The air mass of Kasten and Young.
    let mass = |z: f32| 1.0 / (z.to_radians().cos() + 0.50572 * (96.07995 - z).powf(-1.6364));
    let tau = |beta: f32| -> V3 {
        let ray = [0.0401, 0.0936, 0.2286];
        let aer = [0.0662, 0.0870, 0.1159].map(|v| v * beta / 0.04);
        [ray[0] + aer[0], ray[1] + aer[1], ray[2] + aer[2]]
    };
    let t = tau(0.04 + 0.2 * haze);
    let (m, m0) = (mass(z), mass(0.0));
    [0, 1, 2].map(|k| (-(m - m0) * t[k]).exp())
}

/// A scene: the models and the terrain as triangles around a middle, and what they are made of.
struct World<'a> {
    bvh: Bvh,
    mesh: Option<&'a Mesh>,
    /// The terrain as triangles: corners in the scene, and the colour of each corner.
    terrain_colour: Vec<[V3; 3]>,
    floor: Option<f32>,
    /// How far the light that darkens corners looks for what is near: further in a large
    /// scene, such as a diorama of a whole island, so that its corners darken too.
    near: f32,
    /// How far out from the middle the studio floor fades into the room behind.
    fade: f32,
    /// Each material: its colour, how opaque, how much it mirrors at most, and how smooth.
    materials: Vec<Mat>,
    sky: Sky,
    water: Option<WaterIn>,
    /// The distance over which haze takes half the colour, or none.
    haze: Option<f32>,
    /// The lights at points, and a search tree of them by where they are.
    lights: Vec<LightIn>,
    light_tree: rstar::RTree<rstar::primitives::GeomWithData<[f32; 3], usize>>,
    /// How far the strongest light reaches.
    light_reach: f32,
    lit_windows: f32,
    windows: bool,
    /// The lowest point of each object, where its first floor begins.
    object_base: Vec<f32>,
}

/// A light at a point, in the coordinates of the scene: its light at 1 m, and how far it
/// reaches before it is too weak to see.
#[derive(Clone, Copy)]
struct LightIn {
    at: V3,
    power: V3,
    reach: f32,
}

/// A number from 0 to 1 that is always the same for the same numbers.
fn hash01(a: u32, b: u32, c: u32) -> f32 {
    let mut h =
        (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (c as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

#[derive(Clone, Copy)]
struct Mat {
    albedo: V3,
    opacity: f32,
    /// The share mirrored when looked at straight on, and how sharp the mirror image is.
    mirror: f32,
    gloss: f32,
    textured: bool,
    /// The light it gives off itself, in linear colour.
    glow: V3,
}

/// Water in the coordinates of the scene.
struct WaterIn {
    z: f32,
    /// The areas, each with its bounding box. Empty: everywhere.
    areas: Vec<([f32; 4], geo::Polygon<f32>)>,
    waves: f32,
}

impl WaterIn {
    fn covers(&self, x: f32, y: f32) -> bool {
        use geo::Contains;
        self.areas.is_empty() || self.areas.iter().any(|(b, p)| x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3] && p.contains(&geo::Coord { x, y }))
    }
    /// The normal of the surface at a place, tilted by small waves.
    fn normal(&self, x: f32, y: f32) -> V3 {
        if self.waves <= 0.0 {
            return [0.0, 0.0, 1.0];
        }
        let h = |x: f32, y: f32| fbm(x * 0.35, y * 0.35, 4) + 0.5 * fbm(x * 1.7 + 11.0, y * 1.7 - 5.0, 3);
        let e = 0.05;
        let (dx, dy) = ((h(x + e, y) - h(x - e, y)) / (2.0 * e), (h(x, y + e) - h(x, y - e)) / (2.0 * e));
        let a = 0.6 * self.waves;
        norm([-dx * a, -dy * a, 1.0])
    }
}

/// Renders models and terrain. `rows` counts the rows that are done, and `stop` ends the
/// rendering when it is set; then nothing is given.
pub fn render(meshes: &[&Mesh], terrain: Option<&Grid>, drape: Option<&RasterImage>, s: &Settings, rows: &AtomicU64, stop: &AtomicBool) -> Option<RasterImage> {
    let mesh_all = if meshes.len() == 1 { None } else { Some(Mesh::merge(meshes)) };
    let mesh: Option<&Mesh> = match (&mesh_all, meshes.first()) {
        (Some(m), _) => Some(m),
        (None, Some(m)) => Some(*m),
        (None, None) => None,
    };
    let mesh = mesh.filter(|m| !m.tris.is_empty());
    // The middle: of the models, or of the terrain without them.
    let bounds = mesh.and_then(|m| m.bounds());
    let (lo, hi) = match (bounds, terrain) {
        (Some(b), _) => b,
        (None, Some(g)) => {
            let gt = g.geotransform;
            let (x1, y1) = (gt[0] + gt[1] * g.width as f64, gt[3] + gt[5] * g.height as f64);
            let (zl, zh) = g.values.iter().filter(|v| !v.is_nan()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v as f64), b.max(*v as f64)));
            ([gt[0].min(x1), gt[3].min(y1), zl], [gt[0].max(x1), gt[3].max(y1), zh])
        }
        (None, None) => return None,
    };
    let origin = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]];
    let local = |p: [f64; 3]| -> V3 { [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32] };
    let mut prims = Vec::new();
    let mut corners = Vec::new();
    if let Some(m) = mesh {
        for (i, t) in m.tris.iter().enumerate() {
            prims.push(Prim { index: i as u32, terrain: false });
            corners.push(t.v.map(|v| local(m.positions[v as usize])));
        }
    }
    // The terrain, around the models when there are models, at most 600 cells on a side.
    let mut terrain_colour = Vec::new();
    if let Some(g) = terrain {
        let gt = g.geotransform;
        let (mut c0, mut c1, mut r0, mut r1) = (0usize, g.width, 0usize, g.height);
        if bounds.is_some() {
            // Far enough that its edge is beyond what is seen, unless the camera looks down.
            let reach = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * 25.0).max(1500.0).max(s.distance.unwrap_or(0.0) * 4.0);
            let col = |x: f64| ((x - gt[0]) / gt[1]).floor();
            let row = |y: f64| ((y - gt[3]) / gt[5]).floor();
            let (ca, cb) = (col(lo[0] - reach), col(hi[0] + reach));
            let (ra, rb) = (row(hi[1] + reach), row(lo[1] - reach));
            c0 = ca.min(cb).max(0.0) as usize;
            c1 = (ca.max(cb) as usize + 1).min(g.width);
            r0 = ra.min(rb).max(0.0) as usize;
            r1 = (ra.max(rb) as usize + 1).min(g.height);
        }
        if c1 > c0 + 1 && r1 > r0 + 1 {
            let step = ((c1 - c0).max(r1 - r0) as f64 / 600.0).ceil().max(1.0) as usize;
            let at = |c: usize, r: usize| -> Option<[f64; 3]> {
                let z = g.get(c, r)? as f64;
                Some([gt[0] + gt[1] * (c as f64 + 0.5), gt[3] + gt[5] * (r as f64 + 0.5), z])
            };
            // The heights of the part that is used, for colours by the height.
            let (mut zl, mut zh) = (f64::INFINITY, f64::NEG_INFINITY);
            if s.terrain_colours.is_some() {
                for r in (r0..r1).step_by(step) {
                    for c in (c0..c1).step_by(step) {
                        if let Some(z) = g.get(c, r).filter(|z| !z.is_nan()) {
                            zl = zl.min(z as f64);
                            zh = zh.max(z as f64);
                        }
                    }
                }
            }
            let by_height = |z: f64| -> V3 {
                match s.terrain_colours {
                    Some(c) if zh > zl => {
                        let t = crate::terrain::tint(c, (z - zl) / (zh - zl));
                        [linear(t[0] as f32 / 255.0), linear(t[1] as f32 / 255.0), linear(t[2] as f32 / 255.0)]
                    }
                    Some(c) => {
                        let t = crate::terrain::tint(c, 0.5);
                        [linear(t[0] as f32 / 255.0), linear(t[1] as f32 / 255.0), linear(t[2] as f32 / 255.0)]
                    }
                    None => [0.16, 0.26, 0.11],
                }
            };
            let colour_at = |p: [f64; 3]| -> V3 {
                match drape {
                    Some(img) => {
                        let ig = img.geotransform;
                        let (x, y) = ((p[0] - ig[0]) / ig[1], (p[1] - ig[3]) / ig[5]);
                        if x >= 0.0 && y >= 0.0 && (x as usize) < img.width && (y as usize) < img.height {
                            let px = &img.rgba[((y as usize) * img.width + x as usize) * 4..][..4];
                            if px[3] > 0 {
                                return [linear(px[0] as f32 / 255.0), linear(px[1] as f32 / 255.0), linear(px[2] as f32 / 255.0)];
                            }
                        }
                        by_height(p[2])
                    }
                    None => by_height(p[2]),
                }
            };
            let mut r = r0;
            while r + step < r1 {
                let mut c = c0;
                while c + step < c1 {
                    if let (Some(a), Some(b), Some(cc), Some(d)) = (at(c, r), at(c + step, r), at(c, r + step), at(c + step, r + step)) {
                        for tri in [[a, cc, b], [b, cc, d]] {
                            prims.push(Prim { index: terrain_colour.len() as u32, terrain: true });
                            corners.push(tri.map(local));
                            terrain_colour.push(tri.map(colour_at));
                        }
                    }
                    c += step;
                }
                r += step;
            }
        }
    }
    // Water that covers everything stands in for the floor.
    let sea = s.water.as_ref().is_some_and(|w| w.areas.is_empty());
    let floor = (terrain.is_none() && s.ground && mesh.is_some() && !sea).then_some(-0.002f32);
    let bvh = build(prims, corners);
    let wood = [linear(0.80), linear(0.66), linear(0.47)];
    let materials: Vec<Mat> = mesh.map_or(Vec::new(), |m| {
        m.materials
            .iter()
            .map(|x| {
                let name = x.name.to_lowercase();
                let named_glass = s.glass_names.iter().any(|g| !g.is_empty() && name.contains(g.as_str()));
                let (albedo, opacity) = if named_glass { ([0.42, 0.50, 0.55], 0.28f32) } else { (x.diffuse.map(linear), x.opacity) };
                let glass = opacity < 0.98;
                let albedo = match s.look {
                    _ if glass => albedo,
                    Look::Materials => albedo,
                    Look::White => [0.70, 0.70, 0.68],
                    Look::Wood => wood,
                };
                // How much a surface mirrors, from its shine: metal and polished stone much,
                // painted walls hardly at all.
                let spec = (x.specular[0] + x.specular[1] + x.specular[2]) / 3.0;
                let gloss = ((x.shininess - 40.0) / 300.0).clamp(0.0, 1.0);
                let mirror = if s.look == Look::Materials && !glass { spec.clamp(0.02, 0.9) } else { 0.04 };
                let glows = s.glow_names.iter().any(|g| !g.is_empty() && name.contains(g.as_str()));
                let glow = if glows {
                    mul(x.diffuse.map(linear), 4.0)
                } else if s.look == Look::Materials {
                    mul(x.emission.map(|c| linear(c.min(1.0)) * c.max(1.0)), 2.0)
                } else {
                    [0.0; 3]
                };
                Mat { albedo, opacity, mirror, gloss, textured: s.look == Look::Materials && !named_glass, glow }
            })
            .collect()
    });
    let reach = ((hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]) as f32).max(1e-3);

    // The sun, behind the clouds there are.
    let (sa, salt) = ((s.sun_azimuth as f32).to_radians(), (s.sun_altitude.clamp(-20.0, 90.0) as f32).to_radians());
    let sun_dir = norm([sa.sin() * salt.cos(), sa.cos() * salt.cos(), salt.sin()]);
    let clouds = s.clouds.clamp(0.0, 1.0);
    let haze = s.haze.clamp(0.0, 1.0);
    let through_air = sun_colour(salt.max(0.0), haze);
    let above = ((salt.to_degrees() + 1.0) / 3.0).clamp(0.0, 1.0);
    let sun_col = mul(hadamard(hadamard([1.0, 0.95, 0.88], through_air), s.sun_tint), 2.6 * s.sun_strength * above * (1.0 - 0.92 * clouds.powf(1.3)));
    let sun_size = s.sun_size as f32 + 30.0 * clouds * clouds;
    let sun_cos = (sun_size.clamp(0.1, 60.0) / 2.0).to_radians().cos();
    // Cloud or not, a sky in more cloud lights a little more evenly, and the light through
    // haze is brighter in the shade.
    let sky_k = s.sky_strength * (1.0 + 0.4 * clouds + 0.2 * haze);
    let model = (s.background == Background::Sky).then(|| Preetham::new(sun_dir, 2.4 + 6.0 * haze));
    let grey_level = model.as_ref().map_or(0.45, |m| (m.light([0.0, 0.0, 1.0]).iter().sum::<f32>() / 3.0 * 1.4).max(0.03));
    let cloud_grey = mul([0.92, 0.93, 0.96], grey_level);
    let cloud_lit = add(mul(cloud_grey, 1.25), mul(sun_col, 0.12 * (1.0 - clouds)));
    let sky = Sky { background: s.background, model, clouds, cloud_lit, cloud_grey, tint: s.sky_tint };
    let water = s.water.as_ref().map(|w| {
        let z = (w.level - origin[2]) as f32;
        let areas = w
            .areas
            .iter()
            .filter_map(|p| {
                use geo::{BoundingRect, MapCoords};
                let q: geo::Polygon<f32> = p.map_coords(|c| geo::Coord { x: (c.x - origin[0]) as f32, y: (c.y - origin[1]) as f32 });
                let r = q.bounding_rect()?;
                Some(([r.min().x, r.min().y, r.max().x, r.max().y], q))
            })
            .collect();
        WaterIn { z, areas, waves: w.waves.clamp(0.0, 1.0) }
    });
    let haze_len = (haze > 0.0).then(|| 40_000.0 * (1.0 - haze).powi(2) + 300.0);
    // The lights: a street lamp lights the ground a few metres around it, and is seen as far
    // as it is weaker than a hundredth of what it gives under it.
    let lights: Vec<LightIn> = s
        .lights
        .iter()
        .filter(|l| l.strength > 0.0)
        .map(|l| {
            let power = mul(l.colour.map(linear), LAMP * l.strength);
            let peak = power.iter().fold(0.0f32, |a, b| a.max(*b));
            LightIn { at: local(l.at), power, reach: (peak / 0.004).sqrt() }
        })
        .collect();
    let light_reach = lights.iter().fold(0.0f32, |a, l| a.max(l.reach));
    let light_tree = rstar::RTree::bulk_load(lights.iter().enumerate().map(|(i, l)| rstar::primitives::GeomWithData::new(l.at, i)).collect());
    let object_base = mesh.map_or(Vec::new(), |m| {
        let mut base = vec![f32::INFINITY; m.objects.len().max(1)];
        for t in &m.tris {
            let k = (t.object as usize).min(base.len() - 1);
            for v in t.v {
                base[k] = base[k].min((m.positions[v as usize][2] - origin[2]) as f32);
            }
        }
        base
    });
    let world = World {
        bvh,
        mesh,
        terrain_colour,
        floor,
        near: (reach * 0.02).max(60.0),
        fade: reach * 2.5,
        materials,
        sky,
        water,
        haze: haze_len,
        lights,
        light_tree,
        light_reach,
        lit_windows: s.lit_windows.clamp(0.0, 1.0),
        windows: s.windows,
        object_base,
    };

    // The camera, far enough away to see everything at the angle it has.
    let size = [(hi[0] - lo[0]) as f32, (hi[1] - lo[1]) as f32, (hi[2] - lo[2]) as f32];
    let target: V3 = [0.0, 0.0, size[2] * if mesh.is_some() { 0.45 } else { 0.3 }];
    let radius = 0.5 * dot(size, size).sqrt().max(1.0);
    let fov = (s.fov.clamp(5.0, 120.0) as f32).to_radians();
    let aspect = s.width as f32 / s.height.max(1) as f32;
    let fit = (fov / 2.0).tan().min((fov / 2.0).tan() * aspect);
    // With parallel rays the camera stands well outside the scene, and the picture spans
    // the scene by the zoom alone.
    let dist = match s.distance {
        _ if s.parallel => radius * 4.0 + 10.0,
        Some(d) if d > 0.0 => d as f32,
        _ => radius / fit.max(0.05) / s.zoom.max(0.05) as f32 * 1.05,
    };
    let span = radius * 1.05 / s.zoom.max(0.05) as f32 / aspect.min(1.0);
    let (az, el) = ((s.azimuth as f32).to_radians(), (s.elevation.clamp(-10.0, 90.0) as f32).to_radians());
    let eye = add(target, mul([az.sin() * el.cos(), az.cos() * el.cos(), el.sin()], dist));
    let fwd = norm(sub(target, eye));
    let right = norm(cross(fwd, [0.0, 0.0, 1.0]));
    let right = if dot(right, right) < 0.5 { [1.0, 0.0, 0.0] } else { right };
    let up = cross(right, fwd);
    let half_h = if s.parallel { span } else { (fov / 2.0).tan() };
    let half_w = half_h * aspect;
    let light = Light { sun: sun_dir, sun_col, sun_cos, sky_k };
    // As a camera does: in the dimmer light of dusk the picture is exposed longer, so that
    // the warm light shows rather than darkness. The noon sky is the measure.
    let exposure = match &world.sky.model {
        Some(m) => {
            let z = m.light([0.0, 0.0, 1.0]);
            let lum = 0.2126 * z[0] + 0.7152 * z[1] + 0.0722 * z[2];
            (0.36 / lum.max(1e-3)).clamp(1.0, 2.2)
        }
        None => 1.0,
    };

    // Depth of field: rays start from a disc around the eye and meet at the distance in
    // focus, so that only there things are sharp.
    let to_target = dot(sub(target, eye), fwd);
    let focus = match s.focus {
        Some(f) if f > 0.0 => f as f32,
        _ => to_target,
    };
    // The size of the lens goes with how far away the scene is, so that the same blur looks
    // alike for a house and for a town.
    let lens = if s.parallel { 0.0 } else { s.blur.clamp(0.0, 1.0) * 0.06 * to_target };
    let n = s.samples.clamp(1, 1024);
    let lines: Vec<(Vec<V3>, Vec<f32>)> = (0..s.height)
        .into_par_iter()
        .map(|y| {
            let mut row = vec![[0.0f32; 3]; s.width];
            let mut alphas = vec![0.0f32; s.width];
            if stop.load(Ordering::Relaxed) {
                return (row, alphas);
            }
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (y as u64 + 1).wrapping_mul(0x2545_F491_4F6C_DD1D));
            for x in 0..s.width {
                let mut acc = [0.0f32; 3];
                let mut alpha = 0.0f32;
                for k in 0..n {
                    // Stratified over the pixel.
                    let side = (n as f32).sqrt().ceil() as usize;
                    let (sx, sy) = ((k % side) as f32, (k / side) as f32);
                    let jx = (sx + rng.next()) / side as f32;
                    let jy = (sy + rng.next()) / side as f32;
                    let px = ((x as f32 + jx) / s.width as f32 * 2.0 - 1.0) * half_w;
                    let py = (1.0 - (y as f32 + jy) / s.height as f32 * 2.0) * half_h;
                    let d = norm(add(fwd, add(mul(right, px), mul(up, py))));
                    let (from, d) = if s.parallel {
                        (add(eye, add(mul(right, px), mul(up, py))), fwd)
                    } else if lens > 0.0 {
                        let sharp = add(eye, mul(d, focus / dot(d, fwd)));
                        let (r, a) = (lens * rng.next().sqrt(), 2.0 * std::f32::consts::PI * rng.next());
                        let from = add(eye, add(mul(right, r * a.cos()), mul(up, r * a.sin())));
                        (from, norm(sub(sharp, from)))
                    } else {
                        (eye, d)
                    };
                    // The sun and the sky are sampled evenly over the rays of the pixel, each
                    // in its own order, so that soft shadows are smooth with few rays.
                    let cell = |i: usize, rng: &mut Rng| -> [f32; 2] {
                        let i = i % (side * side);
                        [((i % side) as f32 + rng.next()) / side as f32, ((i / side) as f32 + rng.next()) / side as f32]
                    };
                    let spread = Spread {
                        sun: cell(k.wrapping_mul(7).wrapping_add(x * 13 + y * 5), &mut rng),
                        sky: cell(k.wrapping_mul(11).wrapping_add(x * 3 + y * 17), &mut rng),
                    };
                    let (c, a) = trace(&world, from, d, s, &light, &mut rng, 0, spread);
                    acc = add(acc, c);
                    alpha += a;
                }
                row[x] = mul(acc, exposure / n as f32);
                alphas[x] = alpha / n as f32;
            }
            rows.fetch_add(1, Ordering::Relaxed);
            (row, alphas)
        })
        .collect();
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let (mut colour, alpha): (Vec<V3>, Vec<f32>) = {
        let (c, a): (Vec<Vec<V3>>, Vec<Vec<f32>>) = lines.into_iter().unzip();
        (c.concat(), a.concat())
    };
    // The lamps themselves, as small bright points with a glow around them, where the eye
    // sees them.
    let px_per_rad = s.height as f32 / 2.0 / half_h;
    for l in &world.lights {
        let v = sub(l.at, eye);
        let z = dot(v, fwd);
        if z <= 0.1 {
            continue;
        }
        let dist = dot(v, v).sqrt();
        let dir = mul(v, 1.0 / dist);
        if world.bvh.through(eye, dir, dist - 0.3, &|_| 1.0) <= 0.0 {
            continue;
        }
        let (sx, sy) = if s.parallel { (dot(v, right), dot(v, up)) } else { (dot(v, right) / z, dot(v, up) / z) };
        let (cx, cy) = ((sx / half_w + 1.0) / 2.0 * s.width as f32, (1.0 - sy / half_h) / 2.0 * s.height as f32);
        // The bulb is about 0.4 m across; the glow a little wider, and never less than a few
        // pixels, so that far lamps still twinkle.
        let core = (if s.parallel { 0.2 * px_per_rad } else { 0.2 / dist * px_per_rad }).max(1.2);
        let glow = (core * 4.0).max(5.0);
        let peak = l.power.iter().fold(0.0f32, |a, b| a.max(*b)).max(1e-6);
        let col = mul(l.power, 1.0 / peak);
        let k = (exposure * 3.0).min(6.0);
        let reach = (glow * 6.0).ceil() as i64;
        for yy in (cy as i64 - reach).max(0)..(cy as i64 + reach + 1).min(s.height as i64) {
            for xx in (cx as i64 - reach).max(0)..(cx as i64 + reach + 1).min(s.width as i64) {
                let (dx, dy) = (xx as f32 + 0.5 - cx, yy as f32 + 0.5 - cy);
                let r2 = dx * dx + dy * dy;
                let edge = (1.0 - r2.sqrt() / (glow * 6.0)).max(0.0);
                let f = 2.5 * (-r2 / (core * core)).exp() + 0.25 * (-r2.sqrt() / glow).exp() * edge * edge;
                let i = yy as usize * s.width + xx as usize;
                colour[i] = add(colour[i], mul(col, f * k));
            }
        }
    }
    let mut rgba = Vec::with_capacity(s.width * s.height * 4);
    for (c, a) in colour.iter().zip(&alpha) {
        let c = if s.vivid != 0.0 {
            // Further from grey by the same share, so that light and dark stay as they are.
            let lum = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            c.map(|v| (lum + (v - lum) * (1.0 + s.vivid)).max(0.0))
        } else {
            *c
        };
        let c = tone(c);
        rgba.extend_from_slice(&[c[0], c[1], c[2], if s.background == Background::Clear { (a * 255.0).round() as u8 } else { 255 }]);
    }
    Some(RasterImage { width: s.width, height: s.height, rgba, geotransform: [0.0, 1.0, 0.0, s.height as f64, 0.0, -1.0] })
}

/// The sun and the sky, as the rays see them.
struct Light {
    sun: V3,
    sun_col: V3,
    sun_cos: f32,
    sky_k: f32,
}

/// Two numbers from 0 to 1 for a sample: spread evenly over the samples of a pixel for the
/// first rays, and at random after.
#[derive(Clone, Copy)]
struct Spread {
    sun: [f32; 2],
    sky: [f32; 2],
}

impl Spread {
    fn random(rng: &mut Rng) -> Spread {
        Spread { sun: [rng.next(), rng.next()], sky: [rng.next(), rng.next()] }
    }
}

/// A direction around a normal, more of them near it, as light falls on a surface.
fn cosine_dir(n: V3, u: [f32; 2]) -> V3 {
    let (u1, u2) = (u[0], u[1]);
    let r = u1.sqrt();
    let phi = 2.0 * std::f32::consts::PI * u2;
    let (x, y, z) = (r * phi.cos(), r * phi.sin(), (1.0 - u1).max(0.0).sqrt());
    let a = if n[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let t = norm(cross(a, n));
    let b = cross(n, t);
    norm(add(add(mul(t, x), mul(b, y)), mul(n, z)))
}

/// A direction within the disc of the sun.
fn sun_sample(d: V3, cos_max: f32, u: [f32; 2]) -> V3 {
    let (u1, u2) = (u[0], u[1]);
    let cos_t = 1.0 - u1 * (1.0 - cos_max);
    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
    let phi = 2.0 * std::f32::consts::PI * u2;
    let a = if d[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let t = norm(cross(a, d));
    let b = cross(d, t);
    norm(add(add(mul(t, sin_t * phi.cos()), mul(b, sin_t * phi.sin())), mul(d, cos_t)))
}

/// A mirror direction, spread a little by how rough the surface is.
fn glossy(d: V3, n: V3, gloss: f32, rng: &mut Rng) -> V3 {
    let r = norm(sub(d, mul(n, 2.0 * dot(d, n))));
    if gloss >= 0.99 {
        return r;
    }
    let cos_max = 1.0 - (1.0 - gloss) * 0.08;
    let s = sun_sample(r, cos_max, [rng.next(), rng.next()]);
    if dot(s, n) > 0.0 {
        s
    } else {
        r
    }
}

/// What a ray meets: a triangle, the floor, the water, or nothing.
enum Met {
    Tri(Hit),
    Floor(f32),
    Water(f32),
}

fn meet(w: &World, o: V3, d: V3, tmax: f32) -> Option<Met> {
    let hit = w.bvh.closest(o, d, tmax);
    let nearest = hit.map_or(tmax, |h| h.t);
    let plane = |z: f32| -> Option<f32> {
        if d[2].abs() < 1e-6 {
            return None;
        }
        let t = (z - o[2]) / d[2];
        (t > 1e-4 && t < nearest).then_some(t)
    };
    let water = w.water.as_ref().and_then(|wa| {
        let t = plane(wa.z)?;
        let p = add(o, mul(d, t));
        wa.covers(p[0], p[1]).then_some(t)
    });
    let floor = w.floor.and_then(plane);
    match (water, floor, hit) {
        (Some(a), Some(b), _) => Some(if a <= b { Met::Water(a) } else { Met::Floor(b) }),
        (Some(a), None, _) => Some(Met::Water(a)),
        (None, Some(b), _) => Some(Met::Floor(b)),
        (None, None, Some(h)) => Some(Met::Tri(h)),
        (None, None, None) => None,
    }
}

/// What a surface is where a ray met it: its colour, normal, opacity and shine.
struct Surface {
    albedo: V3,
    normal: V3,
    opacity: f32,
    specular: f32,
    shininess: f32,
    mirror: f32,
    gloss: f32,
    /// The light it gives off itself.
    glow: V3,
    /// A lit window: its light is seen, not what is behind it.
    pane: bool,
}

/// The light at 1 m from a street lamp, before its colour.
const LAMP: f32 = 14.0;

/// A lit room seen through its window.
fn room_light() -> V3 {
    mul(WARM_LIGHT.map(linear), 1.6)
}

const WATER_COLOUR: V3 = [0.012, 0.035, 0.042];

fn surface(w: &World, h: &Hit, o: V3, d: V3) -> Surface {
    let prim = w.bvh.prims[h.k as usize];
    let [_, e1, e2] = w.bvh.tris[h.k as usize];
    let mut geo_n = norm(cross(e1, e2));
    if dot(geo_n, d) > 0.0 {
        geo_n = mul(geo_n, -1.0);
    }
    let (u, v) = (h.u, h.v);
    let wgt = [1.0 - u - v, u, v];
    if prim.terrain {
        let c = w.terrain_colour[prim.index as usize];
        let albedo = [0, 1, 2].map(|k| c[0][k] * wgt[0] + c[1][k] * wgt[1] + c[2][k] * wgt[2]);
        return Surface { albedo, normal: geo_n, opacity: 1.0, specular: 0.0, shininess: 1.0, mirror: 0.0, gloss: 0.0, glow: [0.0; 3], pane: false };
    }
    let m = w.mesh.expect("a mesh");
    let t = &m.tris[prim.index as usize];
    let mat: &Material = &m.materials[t.material as usize];
    // A smooth normal from the corners, when the model has them, facing the viewer.
    let mut n = geo_n;
    if t.n.iter().all(|i| *i != NONE && (*i as usize) < m.normals.len()) {
        let ns = t.n.map(|i| m.normals[i as usize]);
        let s = norm([0, 1, 2].map(|k| ns[0][k] * wgt[0] + ns[1][k] * wgt[1] + ns[2][k] * wgt[2]));
        n = if dot(s, geo_n) < 0.0 { mul(s, -1.0) } else { s };
        // Normals smoothed over a sharp corner, as some programs export them, shade a flat
        // wall in wedges: then the face's own normal is used.
        let flat = ns.iter().any(|v| {
            let v = norm(*v);
            dot(v, geo_n).abs() < 0.9
        });
        if flat || dot(n, geo_n) < 0.2 {
            n = geo_n;
        }
    }
    let own = w.materials[t.material as usize];
    let mut albedo = own.albedo;
    if let (Some(tex), true) = (&mat.texture, own.textured) {
        if t.t.iter().all(|i| *i != NONE && (*i as usize) < m.uvs.len()) {
            let uv = t.t.map(|i| m.uvs[i as usize]);
            let (tu, tv) = (uv[0][0] * wgt[0] + uv[1][0] * wgt[1] + uv[2][0] * wgt[2], uv[0][1] * wgt[0] + uv[1][1] * wgt[1] + uv[2][1] * wgt[2]);
            let c = tex.sample(tu, tv);
            albedo = hadamard([linear(c[0]), linear(c[1]), linear(c[2])], own.albedo);
        }
    }
    let spec = (mat.specular[0] + mat.specular[1] + mat.specular[2]) / 3.0;
    let mut out = Surface {
        albedo: albedo.map(|v| v.min(0.95)),
        normal: n,
        opacity: own.opacity,
        specular: if own.textured { spec * 0.12 } else { 0.02 },
        shininess: mat.shininess.clamp(2.0, 600.0),
        mirror: own.mirror,
        gloss: own.gloss,
        glow: own.glow,
        pane: false,
    };
    // Glass: a pane, two triangles as most models have them, is lit or not.
    if own.opacity < 0.98 && w.lit_windows > 0.0 && hash01(t.object, prim.index / 2, 7) < w.lit_windows {
        out.glow = room_light();
        out.pane = true;
        out.opacity = 1.0;
    }
    // Windows on plain walls: in rows, a floor every 3 m from the bottom of the building.
    if w.windows && own.opacity >= 0.98 && mat.texture.is_none() && geo_n[2].abs() < 0.15 {
        let p = add(o, mul(d, h.t));
        let along = [-geo_n[1], geo_n[0]];
        let horiz = (along[0] * along[0] + along[1] * along[1]).sqrt().max(1e-6);
        let u = (p[0] * along[0] + p[1] * along[1]) / horiz;
        let base = w.object_base.get(t.object as usize).copied().unwrap_or(0.0);
        let v = p[2] - base;
        let (cu, cv) = ((u / 2.6).floor(), (v / 3.0).floor());
        let (fu, fv) = (u / 2.6 - cu, v / 3.0 - cv);
        if cv >= 0.0 && (0.22..0.78).contains(&fu) && (0.3..0.82).contains(&fv) {
            let lit = w.lit_windows > 0.0 && hash01(t.object, cu as i32 as u32, cv as u32) < w.lit_windows;
            out.albedo = [0.04, 0.05, 0.06];
            out.mirror = 0.06;
            out.gloss = 0.95;
            out.specular = 0.4;
            out.shininess = 200.0;
            if lit {
                out.glow = room_light();
                out.pane = true;
            }
        }
    }
    out
}

/// Haze between the eye and what it sees: far things fade into the colour of the horizon.
fn hazed(w: &World, c: V3, d: V3, t: f32, l: &Light) -> V3 {
    match w.haze {
        Some(len) => {
            let k = (-t / len).exp();
            let horizon = mul(w.sky.light(norm([d[0], d[1], 0.03])), l.sky_k * 1.1);
            add(mul(c, k), mul(horizon, 1.0 - k))
        }
        None => c,
    }
}

#[allow(clippy::too_many_arguments)]
fn trace(w: &World, o: V3, d: V3, s: &Settings, l: &Light, rng: &mut Rng, depth: u32, spread: Spread) -> (V3, f32) {
    let opacity = |p: Prim| -> f32 {
        if p.terrain {
            1.0
        } else {
            let m = w.mesh.expect("a mesh");
            w.materials[m.tris[p.index as usize].material as usize].opacity
        }
    };
    let far = 1e7f32;
    let Some(met) = meet(w, o, d, far) else {
        // The room behind is seen lighter than it lights, as in a photo with a white wall.
        let lift = if s.background == Background::Sky { 1.0 } else { 1.45 };
        let bg = if depth == 0 { mul(w.sky.seen(d), lift) } else { mul(w.sky.seen(d), l.sky_k) };
        return (bg, if depth == 0 { 0.0 } else { 1.0 });
    };
    if let Met::Water(t) = met {
        let p = add(o, mul(d, t));
        let wa = w.water.as_ref().expect("water");
        let mut n = wa.normal(p[0], p[1]);
        if dot(n, d) > 0.0 {
            n = [0.0, 0.0, 1.0];
        }
        let lift = add(p, mul([0.0, 0.0, 1.0], 1e-3 + t * 2e-5));
        let cos_i = (-dot(d, n)).clamp(0.0, 1.0);
        let fresnel = 0.02 + 0.98 * (1.0 - cos_i).powi(5);
        let refl_dir = norm(sub(d, mul(n, 2.0 * dot(d, n))));
        let refl_dir = if refl_dir[2] < 0.01 { norm([refl_dir[0], refl_dir[1], 0.01]) } else { refl_dir };
        let again = Spread::random(rng);
        let mirrored = if depth < 3 { trace(w, lift, refl_dir, s, l, rng, depth + 1, again).0 } else { mul(w.sky.light(refl_dir), l.sky_k) };
        // The sun on the water, where it is not in shadow, and its glitter.
        let ls = sun_sample(l.sun, l.sun_cos, spread.sun);
        let lit = if l.sun[2] > 0.0 { w.bvh.through(lift, ls, 1e7, &opacity) } else { 0.0 };
        let h = norm(sub(ls, d));
        let glint = mul(
            l.sun_col,
            lit * dot(n, h).max(0.0).powf(900.0) * 60.0 * (1.0 - 0.8 * wa.waves.min(1.0)) + lit * dot(n, h).max(0.0).powf(120.0) * 1.5 * wa.waves,
        );
        let body = hadamard(WATER_COLOUR, add(mul(l.sun_col, ls[2].max(0.0) * lit), mul(w.sky.light([0.0, 0.0, 1.0]), l.sky_k)));
        let c = add(add(mul(mirrored, fresnel), mul(body, 1.0 - fresnel)), glint);
        let c = if depth == 0 { hazed(w, c, d, t, l) } else { c };
        return (c, 1.0);
    }
    let on_floor = matches!(met, Met::Floor(_));
    let (t, surf) = match met {
        Met::Tri(h) => (h.t, surface(w, &h, o, d)),
        Met::Floor(t) => {
            // Under a sky the floor is a lawn; in the studio, a light floor.
            let c = if s.background == Background::Sky { [0.16, 0.24, 0.09] } else { [0.78, 0.78, 0.78] };
            (
                t,
                Surface {
                    albedo: c,
                    normal: [0.0, 0.0, 1.0],
                    opacity: 1.0,
                    specular: 0.0,
                    shininess: 1.0,
                    mirror: 0.0,
                    gloss: 0.0,
                    glow: [0.0; 3],
                    pane: false,
                },
            )
        }
        Met::Water(_) => unreachable!(),
    };
    let p = add(o, mul(d, t));
    let n = surf.normal;
    let lift = add(p, mul(n, 1e-3 + t * 2e-5));
    // A lit window: the warm room behind, and what is around mirrored a little in the glass.
    if surf.pane {
        let cos_i = (-dot(d, n)).clamp(0.0, 1.0);
        let fresnel = 0.04 + 0.96 * (1.0 - cos_i).powi(5);
        let refl_dir = norm(sub(d, mul(n, 2.0 * dot(d, n))));
        let mirrored = mul(w.sky.light(refl_dir), l.sky_k);
        let c = add(mul(surf.glow, 1.0 - fresnel), mul(mirrored, fresnel));
        let c = if depth == 0 { hazed(w, c, d, t, l) } else { c };
        return (c, 1.0);
    }
    // Glass: some of what is behind shows through, and what is around mirrors in it.
    if surf.opacity < 0.98 && depth < 4 {
        let behind = trace(w, add(p, mul(d, 1e-3 + t * 2e-5)), d, s, l, rng, depth + 1, spread).0;
        let cos_i = (-dot(d, n)).clamp(0.0, 1.0);
        let fresnel = 0.04 + 0.96 * (1.0 - cos_i).powi(5);
        let refl_dir = norm(sub(d, mul(n, 2.0 * dot(d, n))));
        let again = Spread::random(rng);
        let mirrored = if depth < 2 { trace(w, lift, refl_dir, s, l, rng, depth + 1, again).0 } else { mul(w.sky.light(refl_dir), l.sky_k) };
        let own = shade(w, &surf, lift, d, s, l, spread, rng, &opacity);
        let a = surf.opacity;
        let c = add(add(mul(own, a), mul(behind, (1.0 - a) * (1.0 - fresnel))), mul(mirrored, fresnel * (1.0 - a * 0.5)));
        let c = if depth == 0 { hazed(w, c, d, t, l) } else { c };
        return (c, 1.0);
    }
    let mut c = add(shade(w, &surf, lift, d, s, l, spread, rng, &opacity), surf.glow);
    // Smooth and metallic surfaces mirror what is around them, more at a glancing angle.
    if s.reflections && depth < 2 && surf.gloss > 0.0 {
        let cos_i = (-dot(d, n)).clamp(0.0, 1.0);
        let f = surf.mirror + (1.0 - surf.mirror) * (1.0 - cos_i).powi(5);
        let k = (f * surf.gloss).clamp(0.0, 0.95);
        if k > 0.01 {
            let r = glossy(d, n, surf.gloss, rng);
            let again = Spread::random(rng);
            let seen = trace(w, lift, r, s, l, rng, depth + 1, again).0;
            c = add(mul(c, 1.0 - k), mul(seen, k));
        }
    }
    // In the studio the floor bends up into the wall behind, as on a photo backdrop, so the
    // eye sees no horizon: far out, the floor fades into the room.
    if depth == 0 && s.background != Background::Sky && on_floor {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt() / w.fade;
        let k = ((r - 1.0) / 2.0).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        let room = mul(w.sky.light([d[0], d[1], 0.0]), 1.45);
        return (add(mul(c, 1.0 - k), mul(room, k)), 1.0);
    }
    let c = if depth == 0 { hazed(w, c, d, t, l) } else { c };
    (c, 1.0)
}

/// The light that leaves a surface towards the eye: from the sun, if it reaches, and from
/// the sky and nearby surfaces.
#[allow(clippy::too_many_arguments)]
fn shade(w: &World, surf: &Surface, p: V3, d: V3, s: &Settings, l: &Light, spread: Spread, rng: &mut Rng, opacity: &dyn Fn(Prim) -> f32) -> V3 {
    let n = surf.normal;
    let mut c = [0.0f32; 3];
    // The sun, from a point on its disc, so that shadows are soft at their edges.
    let ls = sun_sample(l.sun, l.sun_cos, spread.sun);
    let ndl = dot(n, ls);
    if ndl > 0.0 && l.sun[2] > -0.05 {
        let light = w.bvh.through(p, ls, 1e7, opacity);
        let light = if light > 0.0 && w.floor.is_some_and(|z| p[2] < z && ls[2] < 0.0) { 0.0 } else { light };
        if light > 0.0 {
            let diffuse = mul(hadamard(surf.albedo, l.sun_col), ndl * light);
            let h = norm(sub(ls, d));
            let spec = surf.specular * dot(n, h).max(0.0).powf(surf.shininess) * (surf.shininess + 8.0) / 25.0;
            c = add(c, add(diffuse, mul(l.sun_col, spec * light)));
        }
    }
    // Lamps near enough to light it, where nothing is in the way.
    if !w.lights.is_empty() {
        let r = w.light_reach;
        let env = rstar::AABB::from_corners([p[0] - r, p[1] - r, p[2] - r], [p[0] + r, p[1] + r, p[2] + r]);
        for g in w.light_tree.locate_in_envelope(&env) {
            let lamp = w.lights[g.data];
            let v = sub(lamp.at, p);
            let d2 = dot(v, v).max(0.04);
            let dist = d2.sqrt();
            if dist > lamp.reach {
                continue;
            }
            let ldir = mul(v, 1.0 / dist);
            let ndl = dot(n, ldir);
            if ndl <= 0.0 {
                continue;
            }
            let seen = w.bvh.through(p, ldir, dist - 0.25, opacity);
            if seen > 0.0 {
                c = add(c, mul(hadamard(surf.albedo, lamp.power), ndl * seen / d2));
            }
        }
    }
    // The sky and what is around, over the half above the surface.
    let bounce = gather(w, p, n, s, l, spread.sky, rng, opacity, 0);
    add(c, hadamard(surf.albedo, bounce))
}

/// The light that comes to a place from the half above its surface, by one way out: the
/// sky, or a surface that is lit by the sun and, for more bounces, by what is around it.
#[allow(clippy::too_many_arguments)]
fn gather(w: &World, p: V3, n: V3, s: &Settings, l: &Light, u: [f32; 2], rng: &mut Rng, opacity: &dyn Fn(Prim) -> f32, level: u32) -> V3 {
    let dir = cosine_dir(n, u);
    // The first bounce looks near; further out the sky is taken.
    let reach = if level == 0 { w.near } else { w.near / 2.0 };
    match meet(w, p, dir, reach) {
        None => mul(w.sky.light(dir), l.sky_k),
        Some(m) => {
            let mut glow = [0.0f32; 3];
            let (t, albedo, n2) = match m {
                Met::Tri(h) => {
                    let s2 = surface(w, &h, p, dir);
                    // Lamps and signs light what is around them; lit windows are left out,
                    // as the light of a room hardly reaches the street and would only speckle.
                    if !s2.pane {
                        glow = s2.glow.map(|g| g.min(2.0));
                    }
                    (h.t, s2.albedo, s2.normal)
                }
                Met::Floor(t) => (t, if s.background == Background::Sky { [0.16, 0.24, 0.09] } else { [0.6, 0.6, 0.6] }, [0.0, 0.0, 1.0]),
                Met::Water(t) => (t, [0.05, 0.08, 0.09], [0.0, 0.0, 1.0]),
            };
            let q = add(add(p, mul(dir, t)), mul(n2, 1e-3));
            let ndl2 = dot(n2, l.sun).max(0.0);
            let lit = if ndl2 > 0.0 { w.bvh.through(q, l.sun, 1e7, opacity) } else { 0.0 };
            let direct = mul(hadamard(albedo, l.sun_col), ndl2 * lit);
            let further = if level + 1 < s.bounces.clamp(1, 4) {
                hadamard(albedo, gather(w, q, n2, s, l, [rng.next(), rng.next()], rng, opacity, level + 1))
            } else {
                mul(albedo, 0.25 * l.sky_k)
            };
            add(add(direct, further), glow)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Material, Tri};

    /// A box of 4 × 4 × 3 m on the ground, red.
    fn cube() -> Mesh {
        let (x0, x1, y0, y1, z0, z1) = (0.0, 4.0, 0.0, 4.0, 0.0, 3.0);
        let positions = vec![[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]];
        let quads = [[0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7], [4, 5, 6, 7]];
        let mut tris = Vec::new();
        for q in quads {
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                tris.push(Tri { v: t.map(|i| i as u32), n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 });
            }
        }
        let red = Material { name: "red".into(), diffuse: [0.8, 0.1, 0.1], ..Material::default() };
        Mesh { positions, normals: Vec::new(), uvs: Vec::new(), tris, materials: vec![red], objects: vec!["box".into()] }
    }

    #[test]
    fn a_box_casts_its_shadow_away_from_the_sun() {
        let m = cube();
        // From the south, the sun from the west: the shadow lies east of the box.
        let s = Settings {
            width: 160,
            height: 100,
            azimuth: 180.0,
            elevation: 60.0,
            sun_azimuth: 270.0,
            sun_altitude: 30.0,
            sun_size: 0.5,
            samples: 4,
            ..Settings::default()
        };
        let img = render(&[&m], None, None, &s, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
        assert_eq!((img.width, img.height), (160, 100));
        let px = |x: usize, y: usize| &img.rgba[(y * 160 + x) * 4..][..3];
        // The middle is the red box.
        let mid = px(80, 50);
        assert!(mid[0] > mid[1] + 40 && mid[0] > mid[2] + 40, "{:?}", mid);
        // The floor east of the box is darker than west of it.
        let bright = |x: usize| px(x, 62).iter().map(|v| *v as u32).sum::<u32>();
        let (west, east) = ((15..30).map(bright).sum::<u32>(), (130..145).map(bright).sum::<u32>());
        assert!(east < west, "the shadow lies east: west {} east {}", west, east);
    }

    /// A small picture of the cube from the south-east, with these settings.
    fn shot(m: &Mesh, more: impl Fn(&mut Settings)) -> RasterImage {
        let mut s = Settings {
            width: 120,
            height: 80,
            azimuth: 150.0,
            elevation: 30.0,
            sun_azimuth: 240.0,
            sun_altitude: 35.0,
            sun_size: 0.5,
            samples: 4,
            ..Settings::default()
        };
        more(&mut s);
        render(&[m], None, None, &s, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap()
    }
    fn px(img: &RasterImage, x: usize, y: usize) -> [i32; 3] {
        let k = (y * img.width + x) * 4;
        [img.rgba[k] as i32, img.rgba[k + 1] as i32, img.rgba[k + 2] as i32]
    }

    #[test]
    fn white_and_wood_models_and_glass_by_name() {
        let m = cube();
        let red = px(&shot(&m, |_| {}), 60, 40);
        assert!(red[0] > red[1] + 40, "{:?}", red);
        let white = px(&shot(&m, |s| s.look = Look::White), 60, 40);
        assert!((white[0] - white[1]).abs() < 12 && white[0] > 120, "{:?}", white);
        let wood = px(&shot(&m, |s| s.look = Look::Wood), 60, 40);
        assert!(wood[0] > wood[2] + 30 && wood[1] > wood[2], "{:?}", wood);
        // A material named as glass lets the floor behind show: no longer red.
        let mut named = cube();
        named.materials[0].name = "Window glass".into();
        let glass = px(&shot(&named, |s| s.glass_names = vec!["glass".into()]), 60, 40);
        assert!(glass[0] < glass[1] + 30, "{:?}", glass);
    }

    #[test]
    fn clouds_soften_the_shadows_and_water_mirrors() {
        let m = cube();
        // From the south, the sun from the west: the shadow lies east of the box, less dark
        // under clouds.
        let from_west = |s: &mut Settings| {
            s.width = 160;
            s.height = 100;
            s.azimuth = 180.0;
            s.elevation = 60.0;
            s.sun_azimuth = 270.0;
            s.sun_altitude = 30.0;
        };
        let shade = |img: &RasterImage| -> f32 {
            let b = |x: usize| px(img, x, 62).iter().sum::<i32>() as f32;
            let (west, east) = ((15..30).map(b).sum::<f32>(), (130..145).map(b).sum::<f32>());
            east / west
        };
        let clear = shot(&m, from_west);
        let overcast = shot(&m, |s| {
            from_west(s);
            s.clouds = 1.0;
        });
        assert!(shade(&clear) < 0.8 && shade(&overcast) > shade(&clear) + 0.1, "{} {}", shade(&clear), shade(&overcast));
        // A sea under the sky: the floor in front is water, dark and blue-green, not grass.
        let sea = shot(&m, |s| {
            s.background = Background::Sky;
            s.water = Some(Water { level: 0.0, areas: Vec::new(), waves: 0.0 });
        });
        let w = px(&sea, 10, 75);
        assert!(w[2] >= w[0] && w[1] >= w[0], "{:?}", w);
        let lawn = px(&shot(&m, |s| s.background = Background::Sky), 10, 75);
        assert!(lawn[1] > lawn[2] + 10, "{:?}", lawn);
    }

    #[test]
    fn a_shiny_surface_mirrors_what_is_beside_it() {
        // The red box, and a polished metal one beside it.
        let mut m = cube();
        let shift = m.positions.len() as u32;
        let more: Vec<[f64; 3]> = m.positions.iter().map(|p| [p[0] + 5.0, p[1], p[2]]).collect();
        m.positions.extend(more);
        let tris: Vec<Tri> = m.tris.iter().map(|t| Tri { v: t.v.map(|v| v + shift), material: 1, ..*t }).collect();
        m.tris.extend(tris);
        m.materials.push(Material { name: "steel".into(), diffuse: [0.3, 0.3, 0.3], specular: [0.9, 0.9, 0.9], shininess: 500.0, ..Material::default() });
        let plain = shot(&m, |s| s.azimuth = 120.0);
        let mirrored = shot(&m, |s| {
            s.azimuth = 120.0;
            s.reflections = true;
        });
        let differ = (0..plain.rgba.len()).step_by(4).filter(|k| (plain.rgba[*k] as i32 - mirrored.rgba[*k] as i32).abs() > 20).count();
        assert!(differ > 50, "the metal box mirrors: {} pixels differ", differ);
    }

    #[test]
    fn the_sky_is_blue_by_day_and_warm_at_sunset() {
        let day = Preetham::new(norm([0.0, 0.6, 0.8]), 2.4);
        let z = day.light([0.0, 0.0, 1.0]);
        assert!(z[2] > z[0] * 1.5, "{:?}", z);
        // Evening: towards the low sun warm, away from it blue.
        let eve = Preetham::new(norm([0.0, 1.0, 0.05]), 2.4);
        let toward = eve.light(norm([0.0, 1.0, 0.05]));
        let away = eve.light(norm([0.0, -1.0, 0.3]));
        assert!(toward[0] > toward[2] * 1.5, "{:?}", toward);
        assert!(away[2] > away[0], "{:?}", away);
        // The sun low is redder than at noon.
        let (noon, low) = (sun_colour(1.2, 0.0), sun_colour(0.06, 0.0));
        assert!(low[2] / low[0] < noon[2] / noon[0] * 0.5);
    }

    #[test]
    fn the_tree_finds_the_same_as_looking_at_every_triangle() {
        let m = cube();
        let mut prims = Vec::new();
        let mut corners = Vec::new();
        for (i, t) in m.tris.iter().enumerate() {
            prims.push(Prim { index: i as u32, terrain: false });
            corners.push(t.v.map(|v| m.positions[v as usize].map(|x| x as f32)));
        }
        let bvh = build(prims, corners);
        let mut rng = Rng(7);
        for _ in 0..500 {
            let o = [rng.next() * 20.0 - 8.0, rng.next() * 20.0 - 8.0, rng.next() * 10.0];
            let d = norm(sub([rng.next() * 4.0, rng.next() * 4.0, rng.next() * 3.0], o));
            let fast = bvh.closest(o, d, 1e9).map(|h| h.t);
            let slow = (0..bvh.tris.len()).filter_map(|k| bvh.tri(k, o, d, 1e9).map(|x| x.0)).fold(None, |a: Option<f32>, t| Some(a.map_or(t, |a| a.min(t))));
            assert_eq!(fast.map(|t| (t * 1000.0).round()), slow.map(|t| (t * 1000.0).round()));
        }
    }

    #[test]
    fn windows_lamps_and_glow_light_the_dusk() {
        let m = cube();
        let dusk = |s: &mut Settings| {
            s.sun_altitude = -3.0;
            s.look = Look::White;
            s.samples = 4;
        };
        let lum = |img: &RasterImage| img.rgba.chunks(4).map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64).sum::<u64>();
        let dark = shot(&m, dusk);
        // Windows drawn on the walls, all lit: warm light where the walls were grey.
        let lit = shot(&m, |s| {
            dusk(s);
            s.windows = true;
            s.lit_windows = 1.0;
        });
        assert!(lum(&lit) > lum(&dark), "{} {}", lum(&lit), lum(&dark));
        let warm = lit.rgba.chunks(4).filter(|p| p[0] as i32 > p[2] as i32 + 60).count();
        assert!(warm > 50, "{} warm pixels", warm);
        // No windows lit: the same as without windows, or darker.
        let none = shot(&m, |s| {
            dusk(s);
            s.windows = true;
        });
        assert!(lum(&none) <= lum(&dark) * 101 / 100);
        // A lamp beside the box lights the floor around it and is seen as a bright point.
        let lamp = shot(&m, |s| {
            dusk(s);
            s.lights = vec![PointLight { at: [-3.0, 2.0, 3.0], colour: WARM_LIGHT, strength: 1.0 }];
        });
        assert!(lum(&lamp) > lum(&dark) * 103 / 100, "{} {}", lum(&lamp), lum(&dark));
        assert!(lamp.rgba.chunks(4).any(|p| p[0] > 240 && p[1] > 200), "the lamp itself");
        // A material named as a lamp glows in its own colour.
        let mut sign = cube();
        sign.materials[0].name = "neon sign".into();
        let glow = px(
            &shot(&sign, |s| {
                dusk(s);
                s.look = Look::Materials;
                s.glow_names = vec!["neon".into()];
            }),
            60,
            40,
        );
        assert!(glow[0] > 200 && glow[0] > glow[1] + 60, "{:?}", glow);
    }

    #[test]
    fn depth_of_field_blurs_what_is_not_in_focus() {
        let m = cube();
        // How sharp: the steepest step in brightness across the middle rows, averaged over
        // them so that the noise of the rays evens out. A blurred edge rises over several
        // pixels.
        let edges = |img: &RasterImage| {
            let rows = 30..50;
            let n = rows.len() as f32;
            let profile: Vec<f32> = (0..img.width).map(|x| rows.clone().map(|y| px(img, x, y).iter().sum::<i32>() as f32).sum::<f32>() / n).collect();
            profile.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max) as i64
        };
        let sharp = shot(&m, |s| s.samples = 36);
        let near = shot(&m, |s| {
            s.samples = 36;
            s.blur = 0.5;
            s.focus = Some(2.0);
        });
        let on = shot(&m, |s| {
            s.samples = 36;
            s.blur = 0.5;
        });
        assert!(edges(&near) < edges(&sharp) * 8 / 10, "{} {}", edges(&near), edges(&sharp));
        // In focus on the box, it stays nearly as sharp.
        assert!(edges(&on) > edges(&near), "{} {}", edges(&on), edges(&near));
    }
}
