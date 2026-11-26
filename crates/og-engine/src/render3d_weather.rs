//! The weather and the night in a rendered scene: falling snow and rain, bolts of lightning,
//! the night sky with the moon, stars and northern lights, lines along edges as in a drawing,
//! and names floating over places. What is in the air is laid on the picture after the rays,
//! hidden where something nearer stands in front of it.

use super::*;

/// The camera, to find where a point in the scene is seen in the picture.
pub(super) struct Cam {
    pub eye: V3,
    pub fwd: V3,
    pub right: V3,
    pub up: V3,
    pub half_w: f32,
    pub half_h: f32,
    pub parallel: bool,
    pub width: usize,
    pub height: usize,
    /// How far the camera is from what it looks at.
    pub reach: f32,
}

impl Cam {
    /// Where a point is seen: x and y in pixels, and how far it is in front of the eye.
    pub fn project(&self, p: V3) -> Option<(f32, f32, f32)> {
        let v = sub(p, self.eye);
        let z = dot(v, self.fwd);
        if z <= 1e-3 {
            return None;
        }
        let (sx, sy) = if self.parallel { (dot(v, self.right), dot(v, self.up)) } else { (dot(v, self.right) / z, dot(v, self.up) / z) };
        Some(((sx / self.half_w + 1.0) / 2.0 * self.width as f32, (1.0 - sy / self.half_h) / 2.0 * self.height as f32, z))
    }

    /// The point at x and y in the picture (from -1 to 1) that is `z` in front of the eye.
    pub(super) fn at(&self, x: f32, y: f32, z: f32) -> V3 {
        let (a, b) = if self.parallel { (x * self.half_w, y * self.half_h) } else { (x * self.half_w * z, y * self.half_h * z) };
        add(add(self.eye, mul(self.fwd, z)), add(mul(self.right, a), mul(self.up, b)))
    }

    /// Pixels on the picture for a length at a distance.
    pub(super) fn pixels(&self, length: f32, z: f32) -> f32 {
        let per = self.height as f32 / 2.0 / self.half_h;
        if self.parallel {
            length * per
        } else {
            length / z * per
        }
    }
}

/// Snow falling or rain: flakes or streaks spread evenly through the air in front of the
/// eye, as far as a little beyond what it looks at, lit by the sky.
pub(super) fn particles(colour: &mut [V3], depth: &[f32], cam: &Cam, light: V3, amount: f32, rain: bool, wind: V3) {
    let (w, h) = (cam.width, cam.height);
    let count = ((amount.clamp(0.0, 1.0) * (w * h) as f32 / if rain { 70.0 } else { 80.0 }) as usize).min(300_000);
    let mut rng = Rng(if rain { 0x005E_ED0F_4A1D } else { 0x005E_ED0F_5110 });
    let (near, far) = (cam.reach * 0.12, cam.reach * 1.5);
    // The size of a flake, and the length of a streak, as seen in the picture: larger than
    // they are, so that they show in a picture of a whole town.
    let flake = cam.reach * 0.0016;
    let streak = cam.reach * 0.022;
    // The wind slants the rain, a little in still air and much in a gale, and blows the
    // snow along in streaks.
    let slant = norm([0.12 + wind[0] * 1.6, 0.05 + wind[1] * 1.6, -1.0]);
    let blown = (wind[0] * wind[0] + wind[1] * wind[1]).sqrt();
    for _ in 0..count {
        let (x, y) = (rng.next() * 2.2 - 1.1, rng.next() * 2.2 - 1.1);
        // As many in each part of the air: more far off, where the view is wider.
        let z = if cam.parallel { near + (far - near) * rng.next() } else { (near.powi(3) + rng.next() * (far.powi(3) - near.powi(3))).cbrt() };
        let p = cam.at(x, y, z);
        let Some((px, py, pz)) = cam.project(p) else { continue };
        let size = rng.next() * 0.8 + 0.6;
        if rain {
            let Some((qx, qy, qz)) = cam.project(add(p, mul(slant, streak * size))) else { continue };
            let width = cam.pixels(flake * 0.6, pz).clamp(0.5, 3.0);
            let alpha = (0.2 / width.max(1.0)).min(0.2);
            line(colour, depth, w, h, (px, py, pz), (qx, qy, qz), width, mul(light, 1.25), alpha);
        } else if blown > 0.3 {
            let gust = mul([wind[0], wind[1], -0.4], flake * 9.0 * size);
            let Some((qx, qy, qz)) = cam.project(add(p, gust)) else { continue };
            let r = cam.pixels(flake * size, pz).clamp(0.45, 4.0);
            line(colour, depth, w, h, (px, py, pz), (qx, qy, qz), r, light, 0.7);
        } else {
            let r = cam.pixels(flake * size, pz).clamp(0.45, 6.0);
            // Near flakes are out of focus: larger and fainter.
            let alpha = if r <= 2.0 { 0.92 } else { 0.92 * (2.0 / r).powf(0.8) };
            dot_at(colour, depth, w, h, px, py, pz, r, light, alpha);
        }
    }
}

/// A soft round dot, hidden where something nearer stands.
#[allow(clippy::too_many_arguments)]
fn dot_at(colour: &mut [V3], depth: &[f32], w: usize, h: usize, x: f32, y: f32, z: f32, r: f32, c: V3, alpha: f32) {
    let reach = (r * 2.0).ceil() as i64;
    for yy in (y as i64 - reach).max(0)..(y as i64 + reach + 1).min(h as i64) {
        for xx in (x as i64 - reach).max(0)..(x as i64 + reach + 1).min(w as i64) {
            let i = yy as usize * w + xx as usize;
            if depth[i] < z {
                continue;
            }
            let (dx, dy) = (xx as f32 + 0.5 - x, yy as f32 + 0.5 - y);
            let a = alpha * (-(dx * dx + dy * dy) / (r * r)).exp();
            // A flake smaller than a pixel covers only its share of it.
            let a = a * (r * r * 1.3).min(1.0);
            colour[i] = add(mul(colour[i], 1.0 - a), mul(c, a));
        }
    }
}

/// A soft line from one point to another, each with its distance, hidden where something
/// nearer stands.
#[allow(clippy::too_many_arguments)]
fn line(colour: &mut [V3], depth: &[f32], w: usize, h: usize, a: (f32, f32, f32), b: (f32, f32, f32), width: f32, c: V3, alpha: f32) {
    let (x0, x1) = (a.0.min(b.0) - width * 2.0, a.0.max(b.0) + width * 2.0);
    let (y0, y1) = (a.1.min(b.1) - width * 2.0, a.1.max(b.1) + width * 2.0);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    for yy in (y0 as i64).max(0)..(y1 as i64 + 1).min(h as i64) {
        for xx in (x0 as i64).max(0)..(x1 as i64 + 1).min(w as i64) {
            let (px, py) = (xx as f32 + 0.5, yy as f32 + 0.5);
            let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
            let (ex, ey) = (a.0 + dx * t - px, a.1 + dy * t - py);
            let d = (ex * ex + ey * ey).sqrt();
            if d > width {
                continue;
            }
            let i = yy as usize * w + xx as usize;
            if depth[i] < a.2 + (b.2 - a.2) * t {
                continue;
            }
            let k = alpha * (1.0 - d / width);
            colour[i] = add(mul(colour[i], 1.0 - k), mul(c, k));
        }
    }
}

/// Where the bolts of lightning strike, and how high they come from: on the ground beyond what
/// the camera looks at, from as high as the top of the picture.
pub(super) fn bolt_places(cam: &Cam, target: V3, ground: f32, count: u32) -> Vec<(V3, V3)> {
    let flat = norm([cam.fwd[0], cam.fwd[1], 0.0]);
    let flat = if dot(flat, flat) < 0.5 { [0.0, 1.0, 0.0] } else { flat };
    let side = norm(cross(flat, [0.0, 0.0, 1.0]));
    let mut rng = Rng(0xB017_0F11);
    (0..count)
        .map(|k| {
            let ahead = cam.reach * (0.7 + 0.5 * rng.next() + 0.35 * k as f32);
            let across = cam.reach * (rng.next() - 0.5) * if k == 0 { 0.5 } else { 1.4 };
            let g = add(add([target[0], target[1], ground], mul(flat, ahead)), mul(side, across));
            // The height at which the line straight up from where it strikes leaves the picture.
            let top = if cam.parallel {
                cam.reach * 1.2
            } else {
                let v = sub(g, cam.eye);
                let (a, b, c, e) = (dot(v, cam.up), cam.up[2], dot(v, cam.fwd), cam.fwd[2]);
                let y = cam.half_h * 1.02;
                let z = (y * c - a) / (b - y * e);
                if z.is_finite() && z > 0.0 {
                    z.min(cam.reach * 3.0)
                } else {
                    cam.reach * 1.2
                }
            };
            (g, add(g, [0.0, 0.0, top.max(cam.reach * 0.2)]))
        })
        .collect()
}

/// A bolt of lightning: a crooked path from the cloud to the ground with a few branches, a
/// white-hot core with a glow around it, and the cloud lit above it.
pub(super) fn bolt(colour: &mut [V3], depth: &[f32], cam: &Cam, ground: V3, top: V3, seed: u64) {
    let mut rng = Rng(0xB017 ^ seed.wrapping_mul(0x9E37_79B9));
    let height = top[2] - ground[2];
    let mut paths: Vec<Vec<V3>> = vec![crooked(top, ground, height * 0.16, 7, &mut rng)];
    for _ in 0..4 {
        let main = &paths[0];
        let from = main[(main.len() as f32 * (0.15 + 0.5 * rng.next())) as usize];
        let len = height * (0.15 + 0.25 * rng.next());
        let a = rng.next() * std::f32::consts::TAU;
        let to = add(from, [a.cos() * len * 0.6, a.sin() * len * 0.6, -len]);
        paths.push(crooked(from, to, len * 0.2, 5, &mut rng));
    }
    let k = (cam.width.max(cam.height) as f32 / 1600.0).max(0.6);
    let hot = [3.5, 3.8, 4.6];
    let (w, h) = (cam.width, cam.height);
    // The white-hot core, drawn sharp; and its light, gathered apart and spread out softly
    // after, as the air around a bolt glows, so that the glow has no edge anywhere.
    let mut light = vec![[0.0f32; 3]; w * h];
    let mut nearest = f32::INFINITY;
    for (n, path) in paths.iter().enumerate() {
        let (core, strength) = if n == 0 { (1.1 * k, 1.0) } else { (0.7 * k, 0.5) };
        for pair in path.windows(2) {
            let (Some(a), Some(b)) = (cam.project(pair[0]), cam.project(pair[1])) else { continue };
            nearest = nearest.min(a.2).min(b.2);
            glow_line(colour, depth, w, h, a, b, core, 0.0, mul(hot, strength));
            glow_line(&mut light, depth, w, h, a, b, core * 1.6, 0.0, mul(hot, strength));
        }
    }
    if nearest.is_finite() {
        // Three widths of glow: close and bright, wider, and the faint light in the air far
        // around. Each a blur of near Gaussian shape, so it fades smoothly to nothing.
        let r = (6.0 * k).round().max(1.0) as usize;
        let near = super::more::box_blur(&light, w, h, r);
        let wide = super::more::box_blur(&near, w, h, r * 4);
        let far = super::more::box_blur(&wide, w, h, r * 12);
        for (i, c) in colour.iter_mut().enumerate() {
            // Mostly hidden behind what stands well in front of the bolt, though a little of
            // its light spills over; gradually, so that the ground shows no line where it
            // passes the bolt.
            let seen = 0.3 + 0.7 * smooth(nearest * 0.5, nearest, depth[i]);
            let g = add(add(mul(near[i], 0.9), mul(wide[i], 1.6)), mul(far[i], 2.2));
            *c = add(*c, mul(g, seen));
        }
    }
    // The cloud the bolt comes from, lit from inside: brightest in the sky, and fainter over
    // the land, so that the horizon does not cut it off.
    if let Some((cx, cy, _)) = cam.project(top) {
        let r = cam.width as f32 * 0.25;
        for (i, c) in colour.iter_mut().enumerate() {
            // Fainter over what is near, and as strong over the far land as over the sky, so
            // that the horizon shows no edge.
            let over = 0.25 + 0.75 * smooth(cam.reach * 0.5, cam.reach * 3.0, depth[i]);
            let (x, y) = ((i % cam.width) as f32 - cx, (i / cam.width) as f32 - cy.max(0.0));
            let f = (-(x * x + y * y) / (r * r)).exp();
            *c = add(*c, mul([0.55, 0.58, 0.75], 0.35 * f * over));
        }
    }
}

/// A path from one point to another, made crooked by moving the middle of each part aside,
/// less at each step.
fn crooked(a: V3, b: V3, spread: f32, levels: u32, rng: &mut Rng) -> Vec<V3> {
    let mut pts = vec![a, b];
    let mut s = spread;
    for _ in 0..levels {
        let mut next = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            next.push(w[0]);
            let mid = mul(add(w[0], w[1]), 0.5);
            next.push(add(mid, [(rng.next() - 0.5) * 2.0 * s, (rng.next() - 0.5) * 2.0 * s, (rng.next() - 0.5) * s * 0.5]));
        }
        next.push(*pts.last().expect("a point"));
        pts = next;
        s *= 0.55;
    }
    pts
}

/// A glowing line: a bright core and a soft glow, hidden where something nearer stands.
#[allow(clippy::too_many_arguments)]
pub(super) fn glow_line(colour: &mut [V3], depth: &[f32], w: usize, h: usize, a: (f32, f32, f32), b: (f32, f32, f32), core: f32, glow: f32, c: V3) {
    let pad = (glow * 3.0).max(core * 3.0);
    let (x0, x1) = (a.0.min(b.0) - pad, a.0.max(b.0) + pad);
    let (y0, y1) = (a.1.min(b.1) - pad, a.1.max(b.1) + pad);
    if x1 < 0.0 || y1 < 0.0 || x0 > w as f32 || y0 > h as f32 {
        return;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    // Each part adds its share of the glow, so the glow is as strong along a crooked line
    // as along a straight one.
    let share = (len2.sqrt() / glow.max(1e-3)).min(1.0);
    for yy in (y0 as i64).max(0)..(y1 as i64 + 1).min(h as i64) {
        for xx in (x0 as i64).max(0)..(x1 as i64 + 1).min(w as i64) {
            let (px, py) = (xx as f32 + 0.5, yy as f32 + 0.5);
            let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
            let (ex, ey) = (a.0 + dx * t - px, a.1 + dy * t - py);
            let d2 = ex * ex + ey * ey;
            let i = yy as usize * w + xx as usize;
            if depth[i] < a.2 + (b.2 - a.2) * t {
                continue;
            }
            let f = (-d2 / (core * core)).exp() + if glow > 0.0 { 0.045 * share * (-d2.sqrt() / glow).exp() } else { 0.0 };
            colour[i] = add(colour[i], mul(c, f));
        }
    }
}

/// The sky at night: dark blue, with stars, the moon where the sun would stand, and northern
/// lights in the north.
pub(super) struct NightSky {
    pub moon: V3,
    pub aurora: f32,
}

impl NightSky {
    /// The light from a direction, as it lights what is below: the dark sky, a little lighter
    /// at the horizon and under the moon, and green under northern lights.
    pub fn light(&self, d: V3) -> V3 {
        let up = d[2].clamp(-1.0, 1.0);
        let t = up.max(0.0).sqrt();
        let base = add(mul([0.010, 0.014, 0.032], 1.0 - t), mul([0.003, 0.0055, 0.016], t));
        let base = if up < 0.0 { mul([0.010, 0.014, 0.032], 1.0 + up * 0.6) } else { base };
        let moon_up = self.moon[2].max(0.0);
        let near_moon = dot(d, self.moon).max(0.0).powi(4);
        let lit = mul([0.012, 0.016, 0.03], moon_up * (0.4 + 0.6 * near_moon));
        let green = mul([0.002, 0.012, 0.006], self.aurora * d[1].max(0.0) * smooth(-0.1, 0.4, up));
        add(add(base, lit), green)
    }

    /// The sky as the eye sees it.
    pub fn seen(&self, d: V3, clouds: f32) -> V3 {
        let mut c = self.light(d);
        if d[2] <= 0.0 {
            return c;
        }
        let clear = (1.0 - clouds * 1.15).clamp(0.0, 1.0);
        // Stars: some cells of a fine grid over the sky hold one, of a brightness of its own.
        let (az, el) = (d[0].atan2(d[1]), d[2].asin());
        let n = 1400.0;
        let (u, v) = (az * n / std::f32::consts::TAU * el.cos().max(0.05), el * n / std::f32::consts::TAU);
        let (cu, cv) = (u.floor(), v.floor());
        let id = (cu as i32 as u32, cv as i32 as u32);
        if hash01(id.0, id.1, 3) < 0.012 {
            let (ju, jv) = (hash01(id.0, id.1, 5), hash01(id.0, id.1, 9));
            let (du, dv) = (u - cu - ju, v - cv - jv);
            let f = (-(du * du + dv * dv) * 30.0).exp();
            let b = 0.6 + 6.0 * hash01(id.0, id.1, 11).powi(6);
            let tint = if hash01(id.0, id.1, 13) < 0.3 { [1.0, 0.85, 0.7] } else { [0.85, 0.9, 1.0] };
            // Fainter near the horizon, through more air.
            c = add(c, mul(tint, b * f * clear * (d[2] * 4.0).min(1.0)));
        }
        // Northern lights: curtains over the north, green below and violet at the top, with
        // rays that run up them.
        if self.aurora > 0.0 && d[1] > -0.2 {
            let a = d[0].atan2(d[1]);
            let el = el.to_degrees();
            let centre = 18.0 + 14.0 * fbm(a * 1.3 + 2.0, 0.7, 3);
            let width = 9.0 + 6.0 * fbm(a * 2.1, 3.3, 2);
            let below = ((el - (centre - width)) / 3.0).clamp(0.0, 1.0);
            let fade = (-((el - centre) / width).powi(2) * 1.5).exp() * below + (-((el - centre - width) / (width * 1.6)).powi(2)).exp() * 0.35;
            let rays = 0.35 + 0.65 * fbm(a * 38.0, el * 0.03, 2).powf(1.6);
            let north = ((d[1] + 0.2) / 0.6).clamp(0.0, 1.0);
            let low = smooth(0.0, 8.0, el);
            let strength = self.aurora * fade * rays * north * clear * low * 0.55;
            let high = ((el - centre) / width).clamp(0.0, 1.0);
            let tint = add(mul([0.08, 1.0, 0.45], 1.0 - high), mul([0.55, 0.2, 0.85], high));
            c = add(c, mul(tint, strength));
        }
        // The moon, with darker seas on it, and a halo around it.
        let cos_m = dot(d, self.moon);
        let radius = 0.9f32.to_radians();
        if cos_m > radius.cos() {
            let a = [d[0] - self.moon[0], d[1] - self.moon[1], d[2] - self.moon[2]];
            let seas = 0.75 + 0.25 * fbm(a[0] * 160.0 + 3.0, (a[1] + a[2]) * 160.0, 3);
            let disc = mul([1.0, 0.97, 0.9], 5.0 * seas * (1.0 - clouds * 0.8));
            c = add(mul(c, 0.1), disc);
        } else if cos_m > 0.0 {
            let angle = cos_m.min(1.0).acos();
            c = add(c, mul([0.55, 0.6, 0.75], 0.08 * (-angle / 0.06).exp() * (1.0 - clouds * 0.6)));
        }
        c
    }
}

/// The places in the picture where one surface ends in front of another, or where a surface
/// turns sharply: drawn as lines in ink, as in a drawing.
pub(super) fn outlines(colour: &mut [V3], depth: &[f32], ids: &[u32], normals: &[V3], w: usize, h: usize) {
    let thick = ((w.max(h) as f32 / 1600.0).round() as i64).max(1);
    let mut edge = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut e = 0.0f32;
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                if nx >= w || ny >= h {
                    continue;
                }
                let j = ny * w + nx;
                let (a, b) = (depth[i], depth[j]);
                if a.is_infinite() != b.is_infinite() {
                    e = 1.0;
                    continue;
                }
                if a.is_infinite() {
                    continue;
                }
                let rel = (a - b).abs() / a.min(b).max(1e-3);
                if ids[i] != ids[j] && rel > 0.004 {
                    e = 1.0;
                } else if rel > 0.03 {
                    e = e.max(1.0);
                } else if dot(normals[i], normals[j]) < 0.8 {
                    e = e.max(0.7);
                }
            }
            edge[i] = e;
        }
    }
    let ink = [0.035, 0.035, 0.04];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let mut e = 0.0f32;
            for dy in 0..thick {
                for dx in 0..thick {
                    let (xx, yy) = (x - dx, y - dy);
                    if xx >= 0 && yy >= 0 {
                        e = e.max(edge[yy as usize * w + xx as usize]);
                    }
                }
            }
            if e > 0.0 {
                let i = y as usize * w + x as usize;
                let k = 0.85 * e;
                colour[i] = add(mul(colour[i], 1.0 - k), mul(ink, k));
            }
        }
    }
}

/// A name over a place: the point on the ground, the point the name stands at, and the name.
pub(super) type Name = (V3, V3, String);

/// Names floating over places, each on a thin line down to its place, where the eye sees the
/// name: the nearest first, and those that would cover a nearer name left out.
pub(super) fn names(rgba: &mut [u8], cam: &Cam, list: &[Name], seen: &dyn Fn(V3) -> bool) -> crate::model::Result<usize> {
    let (w, h) = (cam.width, cam.height);
    let text = crate::chart::Text::new()?;
    let base = (h as f32 / 42.0).clamp(11.0, 48.0);
    // The box of each name, where it stands, where its place is, and the name.
    type Placed = ([f32; 4], (f32, f32), (f32, f32), String);
    let mut placed: Vec<Placed> = Vec::new();
    let mut order: Vec<(f32, usize)> = list.iter().enumerate().filter_map(|(i, n)| cam.project(n.1).map(|p| (p.2, i))).collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, i) in order {
        let (foot, at, name) = &list[i];
        let (Some(top), Some(bottom)) = (cam.project(*at), cam.project(*foot)) else { continue };
        if !seen(*at) {
            continue;
        }
        let size = base * (cam.reach / top.2).sqrt().clamp(0.7, 1.45);
        let wide = text.width(name, size, true);
        let (x0, y0) = (top.0 - wide / 2.0 - size * 0.3, top.1 - size * 1.1);
        let (x1, y1) = (top.0 + wide / 2.0 + size * 0.3, top.1 + size * 0.25);
        if x1 < 0.0 || x0 > w as f32 || y1 < 0.0 || y0 > h as f32 {
            continue;
        }
        if placed.iter().any(|p| x0 < p.0[2] && x1 > p.0[0] && y0 < p.0[3] && y1 > p.0[1]) {
            continue;
        }
        placed.push(([x0, y0, x1, y1], (top.0, top.1), (bottom.0, bottom.1), name.clone()));
    }
    if placed.is_empty() {
        return Ok(0);
    }
    let mut pm = tiny_skia::Pixmap::new(w as u32, h as u32).ok_or_else(|| crate::model::GbError("The picture is too large for names.".into()))?;
    for (d, q) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(rgba.as_chunks::<4>().0.iter()) {
        let a = q[3] as u32;
        for k in 0..3 {
            d[k] = ((q[k] as u32 * a + 127) / 255) as u8;
        }
        d[3] = q[3];
    }
    let k = (h as f32 / 900.0).max(0.7);
    // The farthest first, so that nearer names are on top.
    for p in placed.iter().rev() {
        let ([_, y0, _, y1], (tx, ty), (fx, fy), name) = p;
        let (tx, ty, fx, fy) = (*tx, *ty, *fx, *fy);
        let size = (y1 - y0) / 1.35;
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(tx, ty + size * 0.35);
        pb.line_to(fx, fy);
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
            paint.set_color_rgba8(20, 22, 30, 170);
            pm.stroke_path(&path, &paint, &tiny_skia::Stroke { width: 2.6 * k, ..Default::default() }, tiny_skia::Transform::identity(), None);
            paint.set_color_rgba8(255, 255, 255, 235);
            pm.stroke_path(&path, &paint, &tiny_skia::Stroke { width: 1.1 * k, ..Default::default() }, tiny_skia::Transform::identity(), None);
            if let Some(c) = tiny_skia::PathBuilder::from_circle(fx, fy, 2.4 * k) {
                pm.fill_path(&c, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
            }
        }
        // A dark halo, and the name in white over it.
        let halo = (size / 9.0).max(1.0);
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)] {
            text.draw(&mut pm, name, tx + dx * halo, ty + dy * halo, size, true, [24, 26, 36], 0.5);
        }
        text.draw(&mut pm, name, tx, ty, size, true, [255, 255, 255], 0.5);
    }
    for (q, d) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(pm.data().as_chunks::<4>().0.iter()) {
        let a = d[3] as u32;
        for k in 0..3 {
            q[k] = (d[k] as u32 * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
        }
        q[3] = d[3];
    }
    Ok(placed.len())
}
