//! Small toy objects for a diorama, made of triangles: trees, houses, sheep, boats and
//! fluffy clouds. Each is built around a point, in the units of the scene, and goes into a
//! model as one object.

use crate::mesh::{Mesh, Tri, NONE};

type P3 = [f64; 3];

/// A small random number maker, the same from the same seed.
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x2545_F491_4F6C_DD1D | 1)
    }
    /// From 0 to 1.
    pub(super) fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    pub(super) fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
    pub(super) fn pick<'a, T>(&mut self, list: &'a [T]) -> &'a T {
        &list[((self.next() * list.len() as f64) as usize).min(list.len() - 1)]
    }
}

fn norm3(v: [f64; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    [(v[0] / l) as f32, (v[1] / l) as f32, (v[2] / l) as f32]
}

/// Puts triangles into a model, for one object.
pub(super) struct Shapes<'a> {
    pub mesh: &'a mut Mesh,
    pub object: u32,
}

impl Shapes<'_> {
    fn tri(&mut self, p: [P3; 3], n: [[f32; 3]; 3], material: u32) {
        let v = self.mesh.positions.len() as u32;
        let k = self.mesh.normals.len() as u32;
        self.mesh.positions.extend_from_slice(&p);
        self.mesh.normals.extend_from_slice(&n);
        self.mesh.tris.push(Tri { v: [v, v + 1, v + 2], n: [k, k + 1, k + 2], t: [NONE; 3], material, object: self.object });
    }

    /// A flat triangle, facing the way its corners go round counter-clockwise.
    fn flat(&mut self, a: P3, b: P3, c: P3, material: u32) {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = norm3([u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]);
        self.tri([a, b, c], [n; 3], material);
    }

    fn quad(&mut self, a: P3, b: P3, c: P3, d: P3, material: u32) {
        self.flat(a, b, c, material);
        self.flat(a, c, d, material);
    }

    /// A ball, or an egg with radii of its own along x, y and z, smooth.
    pub(super) fn ball(&mut self, c: P3, r: [f64; 3], material: u32, around: usize, up: usize) {
        let at = |i: usize, j: usize| -> (P3, [f32; 3]) {
            let th = 2.0 * std::f64::consts::PI * i as f64 / around as f64;
            let ph = std::f64::consts::PI * j as f64 / up as f64;
            let (x, y, z) = (ph.sin() * th.cos(), ph.sin() * th.sin(), ph.cos());
            ([c[0] + r[0] * x, c[1] + r[1] * y, c[2] + r[2] * z], norm3([x / r[0], y / r[1], z / r[2]]))
        };
        for j in 0..up {
            for i in 0..around {
                let (a, na) = at(i, j);
                let (b, nb) = at(i, j + 1);
                let (cc, nc) = at(i + 1, j + 1);
                let (d, nd) = at(i + 1, j);
                if j + 1 < up {
                    self.tri([a, b, cc], [na, nb, nc], material);
                }
                if j > 0 {
                    self.tri([a, cc, d], [na, nc, nd], material);
                }
            }
        }
    }

    /// A cone standing on a disc, smooth on its side.
    pub(super) fn cone(&mut self, c: P3, r: f64, h: f64, material: u32, around: usize) {
        let top = [c[0], c[1], c[2] + h];
        let rim = |i: usize| {
            let th = 2.0 * std::f64::consts::PI * i as f64 / around as f64;
            ([c[0] + r * th.cos(), c[1] + r * th.sin(), c[2]], norm3([h * th.cos(), h * th.sin(), r]))
        };
        for i in 0..around {
            let (a, na) = rim(i);
            let (b, nb) = rim(i + 1);
            let mid = 2.0 * std::f64::consts::PI * (i as f64 + 0.5) / around as f64;
            let nt = norm3([h * mid.cos(), h * mid.sin(), r]);
            self.tri([a, b, top], [na, nb, nt], material);
            self.tri([c, b, a], [[0.0, 0.0, -1.0]; 3], material);
        }
    }

    /// A post, as a trunk or a mast.
    pub(super) fn post(&mut self, c: P3, r: f64, h: f64, material: u32, around: usize) {
        let at = |i: usize, z: f64| {
            let th = 2.0 * std::f64::consts::PI * i as f64 / around as f64;
            ([c[0] + r * th.cos(), c[1] + r * th.sin(), c[2] + z], norm3([th.cos(), th.sin(), 0.0]))
        };
        for i in 0..around {
            let (a, na) = at(i, 0.0);
            let (b, nb) = at(i + 1, 0.0);
            let (cc, nc) = at(i + 1, h);
            let (d, nd) = at(i, h);
            self.tri([a, b, cc], [na, nb, nc], material);
            self.tri([a, cc, d], [na, nc, nd], material);
        }
    }

    /// A box standing at c, w along the direction `angle` (radians from east), d across it.
    pub(super) fn block(&mut self, c: P3, w: f64, d: f64, h: f64, angle: f64, material: u32) {
        let (ca, sa) = (angle.cos(), angle.sin());
        let at = |x: f64, y: f64, z: f64| [c[0] + x * ca - y * sa, c[1] + x * sa + y * ca, c[2] + z];
        let (x, y) = (w / 2.0, d / 2.0);
        let b = [at(-x, -y, 0.0), at(x, -y, 0.0), at(x, y, 0.0), at(-x, y, 0.0)];
        let t = [at(-x, -y, h), at(x, -y, h), at(x, y, h), at(-x, y, h)];
        for i in 0..4 {
            let j = (i + 1) % 4;
            self.quad(b[i], b[j], t[j], t[i], material);
        }
        self.quad(t[0], t[1], t[2], t[3], material);
        self.quad(b[3], b[2], b[1], b[0], material);
    }

    /// A gable roof on a box: the ridge along its length w, h high, reaching a little out.
    pub(super) fn roof(&mut self, c: P3, w: f64, d: f64, h: f64, angle: f64, material: u32) {
        let (ca, sa) = (angle.cos(), angle.sin());
        let at = |x: f64, y: f64, z: f64| [c[0] + x * ca - y * sa, c[1] + x * sa + y * ca, c[2] + z];
        let (x, y) = (w / 2.0 * 1.08, d / 2.0 * 1.15);
        let (a, b, cc, dd) = (at(-x, -y, 0.0), at(x, -y, 0.0), at(x, y, 0.0), at(-x, y, 0.0));
        let (r0, r1) = (at(-x, 0.0, h), at(x, 0.0, h));
        self.quad(a, b, r1, r0, material);
        self.quad(cc, dd, r0, r1, material);
        self.flat(b, cc, r1, material);
        self.flat(dd, a, r0, material);
        self.quad(dd, cc, b, a, material);
    }

    /// A sail or a flag: a triangle seen from both sides.
    pub(super) fn sheet(&mut self, a: P3, b: P3, c: P3, material: u32) {
        self.flat(a, b, c, material);
        self.flat(a, c, b, material);
    }
}

/// Gives the material of a name and a colour, made the first time it is asked for.
pub(super) type Paint<'a> = dyn FnMut(&mut Mesh, &str, [f32; 3]) -> u32 + 'a;

fn hex(c: &str) -> [f32; 3] {
    let v = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0x808080);
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}

const BROADLEAF: &[&str] = &["#5cb85c", "#7ac74f", "#4caf50", "#9ccc65", "#66bb6a"];
const PINE: &[&str] = &["#2e7d4f", "#3a8a55", "#256b43", "#33804c"];
const WALLS: &[&str] = &["#fff8ec", "#fbe9d0", "#f6f1e7", "#ffe9e3"];
const ROOFS: &[&str] = &["#e05a47", "#d9534f", "#ef8a45", "#5b7fd6", "#c0504d", "#7e6bc4"];
const TRUNK: &str = "#8a5a3b";

/// What a tree is like.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tree {
    Broadleaf,
    Pine,
}

/// A tree s high, of round crown or of cones. Its main colour.
pub(super) fn tree(sh: &mut Shapes, paint: &mut Paint, at: P3, s: f64, kind: Tree, rng: &mut Rng) -> [f32; 3] {
    let trunk = paint(sh.mesh, "trunk", hex(TRUNK));
    match kind {
        Tree::Broadleaf => {
            let c = hex(rng.pick(BROADLEAF));
            let crown = paint(sh.mesh, &format!("leaves {}", hex_name(c)), c);
            sh.post(at, s * 0.07, s * 0.45, trunk, 6);
            let r = s * rng.range(0.3, 0.36);
            sh.ball([at[0], at[1], at[2] + s * 0.45 + r * 0.8], [r, r, r * 1.05], crown, 12, 8);
            c
        }
        Tree::Pine => {
            let c = hex(rng.pick(PINE));
            let needles = paint(sh.mesh, &format!("needles {}", hex_name(c)), c);
            sh.post(at, s * 0.06, s * 0.25, trunk, 6);
            sh.cone([at[0], at[1], at[2] + s * 0.18], s * 0.32, s * 0.55, needles, 10);
            sh.cone([at[0], at[1], at[2] + s * 0.5], s * 0.24, s * 0.5, needles, 10);
            c
        }
    }
}

fn hex_name(c: [f32; 3]) -> String {
    let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

/// A house of s across, with a gable roof in a bright colour. Its roof colour.
pub(super) fn house(sh: &mut Shapes, paint: &mut Paint, at: P3, s: f64, rng: &mut Rng) -> [f32; 3] {
    let wc = hex(rng.pick(WALLS));
    let rc = hex(rng.pick(ROOFS));
    let walls = paint(sh.mesh, &format!("walls {}", hex_name(wc)), wc);
    let roof = paint(sh.mesh, &format!("roof {}", hex_name(rc)), rc);
    let angle = rng.range(0.0, std::f64::consts::PI);
    let (w, d) = (s * rng.range(0.9, 1.3), s * rng.range(0.6, 0.8));
    let h = s * rng.range(0.5, 0.7);
    sh.block(at, w, d, h, angle, walls);
    sh.roof([at[0], at[1], at[2] + h], w, d, s * 0.45, angle, roof);
    rc
}

/// A woolly sheep s long, looking any way.
pub(super) fn sheep(sh: &mut Shapes, paint: &mut Paint, at: P3, s: f64, rng: &mut Rng) {
    let wool = paint(sh.mesh, "wool", [0.97, 0.96, 0.93]);
    let dark = paint(sh.mesh, "sheep face", [0.18, 0.16, 0.16]);
    let a = rng.range(0.0, 2.0 * std::f64::consts::PI);
    let (ca, sa) = (a.cos(), a.sin());
    let p = |x: f64, y: f64, z: f64| [at[0] + x * ca - y * sa, at[1] + x * sa + y * ca, at[2] + z];
    for (x, y) in [(-0.25, -0.15), (0.25, -0.15), (-0.25, 0.15), (0.25, 0.15)] {
        sh.post(p(x * s, y * s, 0.0), s * 0.05, s * 0.3, dark, 5);
    }
    // The body of a few balls of wool, and the head in front.
    for (x, y, z, r) in [(0.0, 0.0, 0.48, 0.3), (-0.22, 0.0, 0.5, 0.24), (0.2, 0.05, 0.52, 0.22), (0.05, -0.12, 0.55, 0.2)] {
        sh.ball(p(x * s, y * s, z * s), [r * s, r * s, r * s * 0.9], wool, 10, 6);
    }
    sh.ball(p(0.48 * s, 0.0, 0.62 * s), [0.14 * s, 0.11 * s, 0.13 * s], dark, 8, 6);
}

/// A sailing boat s long on the water at z, heading any way.
pub(super) fn boat(sh: &mut Shapes, paint: &mut Paint, at: P3, s: f64, rng: &mut Rng) {
    let hull_c = hex(rng.pick(&["#ffffff", "#e05a47", "#5b7fd6", "#f2c14e"]));
    let hull = paint(sh.mesh, &format!("hull {}", hex_name(hull_c)), hull_c);
    let sail = paint(sh.mesh, "sail", [0.98, 0.97, 0.94]);
    let mast = paint(sh.mesh, "trunk", hex(TRUNK));
    let a = rng.range(0.0, 2.0 * std::f64::consts::PI);
    let (ca, sa) = (a.cos(), a.sin());
    let p = |x: f64, y: f64, z: f64| [at[0] + x * ca - y * sa, at[1] + x * sa + y * ca, at[2] + z];
    let (l, w, h) = (s * 0.5, s * 0.18, s * 0.14);
    // The hull: wider on deck than at the keel, pointed at the bow.
    let deck = [p(-l, -w, h), p(l * 0.6, -w, h), p(l, 0.0, h), p(l * 0.6, w, h), p(-l, w, h)];
    let keel = [p(-l * 0.85, -w * 0.6, 0.0), p(l * 0.5, -w * 0.6, 0.0), p(l * 0.85, 0.0, 0.0), p(l * 0.5, w * 0.6, 0.0), p(-l * 0.85, w * 0.6, 0.0)];
    for i in 0..5 {
        let j = (i + 1) % 5;
        sh.quad(keel[i], keel[j], deck[j], deck[i], hull);
    }
    sh.flat(deck[0], deck[1], deck[2], hull);
    sh.flat(deck[0], deck[2], deck[3], hull);
    sh.flat(deck[0], deck[3], deck[4], hull);
    sh.post(p(0.0, 0.0, h), s * 0.02, s * 0.9, mast, 5);
    sh.sheet(p(0.02 * s, 0.0, h + s * 0.12), p(0.02 * s, 0.0, h + s * 0.88), p(0.42 * s, 0.0, h + s * 0.12), sail);
    sh.sheet(p(-0.02 * s, 0.0, h + s * 0.2), p(-0.02 * s, 0.0, h + s * 0.75), p(-0.35 * s, 0.0, h + s * 0.2), sail);
}

/// A fluffy cloud about s long: balls of cloud along a line, the largest in the middle,
/// with a flatter bottom.
pub(super) fn cloud(sh: &mut Shapes, paint: &mut Paint, at: P3, s: f64, rng: &mut Rng) {
    let white = paint(sh.mesh, "cloud", [0.99, 0.99, 1.0]);
    let a = rng.range(-0.4, 0.4);
    let (ca, sa) = (a.cos(), a.sin());
    let n = 4 + (rng.next() * 3.0) as usize;
    for k in 0..n {
        let t = (k as f64 + 0.5) / n as f64 - 0.5;
        let r = s * (0.3 - t.abs() * 0.28) * rng.range(0.85, 1.15);
        let (x, y) = (t * s * 0.85, rng.range(-0.08, 0.08) * s);
        let z = r * 0.55 + rng.range(0.0, 0.05) * s;
        sh.ball([at[0] + x * ca - y * sa, at[1] + x * sa + y * ca, at[2] + z], [r, r, r * 0.8], white, 14, 9);
    }
    // A second row behind, smaller, so it is round seen from any side.
    for k in 0..n.saturating_sub(2) {
        let t = (k as f64 + 0.5) / (n - 2) as f64 - 0.5;
        let r = s * (0.2 - t.abs() * 0.15);
        let (x, y) = (t * s * 0.6, s * 0.14);
        sh.ball([at[0] + x * ca - y * sa, at[1] + x * sa + y * ca, at[2] + r * 0.6], [r, r, r * 0.8], white, 12, 8);
    }
}

/// The animals of a diorama, by name: what they are called, in English and Swedish, where
/// they like to be, and how large they are against a tree.
pub(super) struct Animal {
    pub name: &'static str,
    pub words: &'static [&'static str],
    pub home: Home,
    pub size: f64,
}

/// Where an animal is put.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Home {
    /// Fields, meadows and open land.
    Grazing,
    /// Forest.
    Forest,
    /// Towns.
    Town,
}

pub(super) const ANIMALS: &[Animal] = &[
    Animal { name: "sheep", words: &["sheep", "får", "lamb"], home: Home::Grazing, size: 0.5 },
    Animal { name: "pig", words: &["pig", "pigs", "gris", "grisar", "svin"], home: Home::Grazing, size: 0.5 },
    Animal { name: "cow", words: &["cow", "cows", "ko", "kor"], home: Home::Grazing, size: 0.75 },
    Animal { name: "horse", words: &["horse", "horses", "häst", "hästar"], home: Home::Grazing, size: 0.8 },
    Animal { name: "rabbit", words: &["rabbit", "rabbits", "kanin", "kaniner", "hare"], home: Home::Grazing, size: 0.28 },
    Animal { name: "dog", words: &["dog", "dogs", "hund", "hundar"], home: Home::Town, size: 0.4 },
    Animal { name: "cat", words: &["cat", "cats", "katt", "katter"], home: Home::Town, size: 0.32 },
    Animal { name: "bear", words: &["bear", "bears", "björn", "björnar"], home: Home::Forest, size: 0.75 },
    Animal { name: "elephant", words: &["elephant", "elephants", "elefant", "elefanter"], home: Home::Grazing, size: 1.3 },
    Animal { name: "giraffe", words: &["giraffe", "giraffes", "giraff", "giraffer"], home: Home::Grazing, size: 1.4 },
    Animal { name: "dinosaur", words: &["dinosaur", "dinosaurs", "dinosaurie", "dinosaurier", "dino"], home: Home::Grazing, size: 2.0 },
];

pub(super) fn animal_by_word(word: &str) -> Option<&'static Animal> {
    let w = word.trim().to_lowercase();
    ANIMALS.iter().find(|a| a.words.contains(&w.as_str()) || a.name == w)
}

/// A four-legged animal, the body along x from the back to the head, looking any way: the
/// parts in its own coordinates, s across, put in place.
struct Body<'s, 'm> {
    sh: &'s mut Shapes<'m>,
    at: P3,
    ca: f64,
    sa: f64,
    s: f64,
}

impl Body<'_, '_> {
    fn p(&self, x: f64, y: f64, z: f64) -> P3 {
        let (x, y, z) = (x * self.s, y * self.s, z * self.s);
        [self.at[0] + x * self.ca - y * self.sa, self.at[1] + x * self.sa + y * self.ca, self.at[2] + z]
    }
    fn ball(&mut self, x: f64, y: f64, z: f64, r: [f64; 3], m: u32) {
        let c = self.p(x, y, z);
        let s = self.s;
        // Radii along the body and across it are turned with it only roughly: the animals
        // are seen from afar.
        let (rx, ry) = (r[0] * s, r[1] * s);
        let (ax, ay) = ((rx * self.ca).hypot(ry * self.sa), (rx * self.sa).hypot(ry * self.ca));
        self.sh.ball(c, [ax, ay, r[2] * s], m, 10, 7);
    }
    fn legs(&mut self, front: f64, back: f64, wide: f64, high: f64, thick: f64, m: u32) {
        for (x, y) in [(front, -wide), (front, wide), (back, -wide), (back, wide)] {
            let c = self.p(x, y, 0.0);
            self.sh.post(c, thick * self.s, high * self.s, m, 6);
        }
    }
    /// Balls in a row from one point to another, growing from r0 to r1: a neck, a tail, a trunk.
    fn chain(&mut self, from: P3, to: P3, r0: f64, r1: f64, n: usize, m: u32) {
        for k in 0..=n {
            let t = k as f64 / n.max(1) as f64;
            let q = [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, from[2] + (to[2] - from[2]) * t];
            let r = r0 + (r1 - r0) * t;
            self.ball(q[0], q[1], q[2], [r, r, r], m);
        }
    }
}

/// An animal `s` across, by its name, standing at `at`, facing any way. Its main colour.
pub(super) fn animal(sh: &mut Shapes, paint: &mut Paint, name: &str, at: P3, s: f64, rng: &mut Rng) -> [f32; 3] {
    if name == "sheep" {
        sheep(sh, paint, at, s, rng);
        return [0.97, 0.96, 0.93];
    }
    let a = rng.range(0.0, 2.0 * std::f64::consts::PI);
    let mut colour = |sh: &mut Shapes, n: &str, c: &str| paint(sh.mesh, n, hex(c));
    // The colours first, the shape after.
    let (main, dark, extra): (&str, &str, &str) = match name {
        "pig" => ("#f4a6b4", "#e07f92", "#f4a6b4"),
        "cow" => ("#f7f5f0", "#2b2b2b", "#f2b8b8"),
        "horse" => (*rng.pick(&["#8b5a2b", "#5a3a22", "#c8a27a", "#3a3a3a"]), "#2a1d14", "#2a1d14"),
        "rabbit" => (*rng.pick(&["#d9d4cc", "#b08d6e", "#f5f2ec"]), "#f1b9c1", "#ffffff"),
        "dog" => (*rng.pick(&["#b07a45", "#3d3028", "#e8d7b8", "#7a5230"]), "#2a211b", "#2a211b"),
        "cat" => (*rng.pick(&["#e89a4c", "#6d6d6d", "#2d2d2d", "#f2ede4"]), "#e9a0a8", "#2a2a2a"),
        "bear" => ("#6b4429", "#3e2617", "#a07a5a"),
        "elephant" => ("#9aa0a6", "#7b8086", "#f4efe6"),
        "giraffe" => ("#e9b949", "#8a5a2b", "#5a3a22"),
        _ => (*rng.pick(&["#6cc070", "#4fa3d9", "#e0884f", "#a77bd9"]), "#3a6e3c", "#f2e6a8"),
    };
    let (m, d, e) = (
        colour(sh, &format!("{} {}", name, main), main),
        colour(sh, &format!("{} dark {}", name, dark), dark),
        colour(sh, &format!("{} extra {}", name, extra), extra),
    );
    let mut b = Body { sh, at, ca: a.cos(), sa: a.sin(), s };
    match name {
        "pig" => {
            b.legs(0.22, -0.22, 0.13, 0.22, 0.06, d);
            b.ball(0.0, 0.0, 0.42, [0.45, 0.3, 0.27], m);
            b.ball(0.42, 0.0, 0.5, [0.2, 0.19, 0.19], m);
            b.ball(0.6, 0.0, 0.47, [0.07, 0.09, 0.08], d);
            b.ball(0.42, -0.12, 0.66, [0.06, 0.05, 0.07], d);
            b.ball(0.42, 0.12, 0.66, [0.06, 0.05, 0.07], d);
            b.ball(-0.45, 0.0, 0.5, [0.05, 0.05, 0.05], d);
        }
        "cow" => {
            b.legs(0.3, -0.3, 0.14, 0.4, 0.06, m);
            b.ball(0.0, 0.0, 0.62, [0.5, 0.25, 0.25], m);
            b.ball(0.1, 0.18, 0.66, [0.18, 0.1, 0.15], d);
            b.ball(-0.2, -0.17, 0.6, [0.15, 0.1, 0.14], d);
            b.ball(0.58, 0.0, 0.8, [0.17, 0.13, 0.15], m);
            b.ball(0.72, 0.0, 0.74, [0.09, 0.1, 0.08], e);
            b.ball(0.55, -0.13, 0.95, [0.04, 0.04, 0.06], d);
            b.ball(0.55, 0.13, 0.95, [0.04, 0.04, 0.06], d);
        }
        "horse" => {
            b.legs(0.3, -0.3, 0.12, 0.55, 0.05, m);
            b.ball(0.0, 0.0, 0.75, [0.48, 0.2, 0.22], m);
            b.chain([0.38, 0.0, 0.85], [0.55, 0.0, 1.15], 0.1, 0.09, 3, m);
            b.ball(0.68, 0.0, 1.17, [0.17, 0.08, 0.08], m);
            b.chain([0.42, 0.0, 1.0], [0.5, 0.0, 1.22], 0.05, 0.05, 3, d);
            b.chain([-0.48, 0.0, 0.78], [-0.6, 0.0, 0.45], 0.05, 0.04, 3, d);
        }
        "rabbit" => {
            b.ball(0.0, 0.0, 0.32, [0.33, 0.25, 0.3], m);
            b.ball(0.3, 0.0, 0.55, [0.17, 0.15, 0.16], m);
            b.ball(0.27, -0.06, 0.85, [0.04, 0.05, 0.2], m);
            b.ball(0.27, 0.06, 0.85, [0.04, 0.05, 0.2], m);
            b.ball(0.45, 0.0, 0.55, [0.03, 0.03, 0.03], d);
            b.ball(-0.33, 0.0, 0.36, [0.08, 0.08, 0.08], e);
        }
        "dog" => {
            b.legs(0.25, -0.25, 0.12, 0.35, 0.06, m);
            b.ball(0.0, 0.0, 0.5, [0.42, 0.2, 0.2], m);
            b.ball(0.42, 0.0, 0.72, [0.18, 0.16, 0.17], m);
            b.ball(0.6, 0.0, 0.67, [0.1, 0.08, 0.07], m);
            b.ball(0.68, 0.0, 0.69, [0.04, 0.04, 0.04], d);
            b.ball(0.4, -0.15, 0.7, [0.05, 0.04, 0.12], e);
            b.ball(0.4, 0.15, 0.7, [0.05, 0.04, 0.12], e);
            b.chain([-0.4, 0.0, 0.6], [-0.55, 0.0, 0.85], 0.05, 0.04, 3, m);
        }
        "cat" => {
            b.legs(0.2, -0.2, 0.1, 0.3, 0.05, m);
            b.ball(0.0, 0.0, 0.42, [0.36, 0.17, 0.17], m);
            b.ball(0.38, 0.0, 0.62, [0.17, 0.17, 0.16], m);
            let (l, r) = (b.p(0.38, -0.1, 0.72), b.p(0.38, 0.1, 0.72));
            b.sh.cone(l, 0.06 * s, 0.14 * s, m, 6);
            b.sh.cone(r, 0.06 * s, 0.14 * s, m, 6);
            b.ball(0.54, 0.0, 0.6, [0.03, 0.03, 0.03], d);
            b.chain([-0.34, 0.0, 0.5], [-0.42, 0.0, 0.95], 0.045, 0.04, 5, e);
        }
        "bear" => {
            b.legs(0.25, -0.25, 0.17, 0.3, 0.1, m);
            b.ball(0.0, 0.0, 0.55, [0.48, 0.32, 0.32], m);
            b.ball(0.45, 0.0, 0.75, [0.22, 0.21, 0.21], m);
            b.ball(0.64, 0.0, 0.7, [0.1, 0.09, 0.08], e);
            b.ball(0.7, 0.0, 0.72, [0.04, 0.04, 0.04], d);
            b.ball(0.4, -0.15, 0.94, [0.07, 0.04, 0.07], d);
            b.ball(0.4, 0.15, 0.94, [0.07, 0.04, 0.07], d);
        }
        "elephant" => {
            b.legs(0.25, -0.25, 0.17, 0.4, 0.1, m);
            b.ball(0.0, 0.0, 0.7, [0.5, 0.36, 0.36], m);
            b.ball(0.5, 0.0, 0.9, [0.25, 0.23, 0.25], m);
            b.ball(0.47, -0.28, 0.92, [0.05, 0.22, 0.24], d);
            b.ball(0.47, 0.28, 0.92, [0.05, 0.22, 0.24], d);
            b.chain([0.72, 0.0, 0.85], [0.82, 0.0, 0.35], 0.08, 0.05, 6, m);
            b.chain([0.68, -0.1, 0.75], [0.8, -0.13, 0.7], 0.03, 0.02, 2, e);
            b.chain([0.68, 0.1, 0.75], [0.8, 0.13, 0.7], 0.03, 0.02, 2, e);
        }
        "giraffe" => {
            b.legs(0.18, -0.18, 0.09, 0.75, 0.04, m);
            b.ball(0.0, 0.0, 0.88, [0.3, 0.15, 0.17], m);
            b.chain([0.2, 0.0, 0.95], [0.38, 0.0, 1.7], 0.07, 0.05, 8, m);
            b.ball(0.46, 0.0, 1.76, [0.12, 0.06, 0.07], m);
            b.ball(0.4, -0.04, 1.86, [0.02, 0.02, 0.05], e);
            b.ball(0.4, 0.04, 1.86, [0.02, 0.02, 0.05], e);
            // Spots.
            for (x, y, z) in [(0.1, -0.13, 0.95), (-0.12, 0.13, 0.9), (-0.05, -0.12, 0.8), (0.15, 0.12, 0.82), (0.0, 0.0, 1.04)] {
                b.ball(x, y, z, [0.07, 0.04, 0.06], d);
            }
        }
        // A dinosaur with a long neck, as a sauropod of a picture book.
        _ => {
            b.legs(0.25, -0.25, 0.15, 0.4, 0.09, m);
            b.ball(0.0, 0.0, 0.6, [0.48, 0.28, 0.3], m);
            b.chain([0.35, 0.0, 0.75], [0.7, 0.0, 1.5], 0.13, 0.07, 9, m);
            b.ball(0.82, 0.0, 1.55, [0.15, 0.09, 0.09], m);
            b.chain([-0.4, 0.0, 0.6], [-1.1, 0.0, 0.3], 0.12, 0.03, 9, m);
            for k in 0..5 {
                let x = -0.3 + k as f64 * 0.15;
                b.ball(x, 0.0, 0.88 - (x.abs() * 0.2), [0.05, 0.03, 0.07], e);
            }
            b.ball(0.88, -0.06, 1.6, [0.02, 0.02, 0.02], d);
            b.ball(0.88, 0.06, 1.6, [0.02, 0.02, 0.02], d);
        }
    }
    hex(main)
}
