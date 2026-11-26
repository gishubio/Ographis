//! The cold, the light and the lens of a rendered scene: frost and rime, frosted panes,
//! frozen lakes with cracks, icicles under the eaves, smoke from chimneys, advent candles and
//! stars in windows, rays of sun through cloud and fog, the shadows of clouds, rainbows,
//! cones of light under lamps, wind, the trails of car lights at night, fireworks and the
//! tilt-shift of a model village. Most of it is laid on the picture after the rays, hidden
//! where something nearer stands in front of it.

use super::weather::Cam;
use super::*;

// ---------- Frost ----------

/// The colour of rime and frost: white, a little blue.
const RIME: V3 = [0.80, 0.86, 0.93];

/// How much rime grows at a place: on all that is out in the cold air, more on what faces up
/// and on edges, in feathery patches, from a little at 0.2 to white all over at 1.
pub(super) fn rime_on(amount: f32, n: V3, p: V3) -> f32 {
    if amount <= 0.0 {
        return 0.0;
    }
    // Hoar frost grows towards the air, so the undersides get least.
    let facing = 0.35 + 0.65 * smooth(-0.6, 0.6, n[2]);
    let patches = fbm(p[0] * 0.9 + p[2] * 0.4 + 3.1, p[1] * 0.9 - p[2] * 0.4 - 7.7, 4);
    let feathers = fbm(p[0] * 7.0 + 1.3, p[1] * 7.0 + p[2] * 3.0, 3);
    let k = smooth(1.0 - amount - 0.15, 1.0 - amount + 0.25, patches * 0.75 + feathers * 0.35);
    (k * facing * (0.55 + 0.45 * amount)).clamp(0.0, 1.0)
}

/// Rime on a surface: its colour towards white, and a sparkle of ice crystals.
pub(super) fn rime(surf: &mut Surface, k: f32, p: V3, rays: f32) {
    if k <= 0.0 {
        return;
    }
    surf.albedo = add(mul(surf.albedo, 1.0 - k), mul(RIME, k));
    surf.mirror *= 1.0 - k;
    surf.gloss *= 1.0 - k;
    // Here and there a crystal turned just so catches the light.
    let cell = (p[0] * 9.0).floor() as i32 as u32
        ^ ((p[1] * 9.0).floor() as i32 as u32).wrapping_mul(7919)
        ^ ((p[2] * 9.0).floor() as i32 as u32).wrapping_mul(104_729);
    // Fewer and fainter with few rays per pixel, where they would only be noise.
    let glint = if hash01(cell, 17, 3) < 0.03 * k * rays { rays } else { 0.0 };
    surf.specular = surf.specular * (1.0 - k) + (0.08 + 0.7 * glint) * k;
    surf.shininess = surf.shininess * (1.0 - k) + (40.0 + 300.0 * glint) * k;
}

/// Frost on a pane of glass: ferns of ice that grow in from the edges and corners, so that
/// the middle is clearest. 0 is clear glass, 1 frosted over.
pub(super) fn frosted_pane(amount: f32, p: V3, edge: f32) -> f32 {
    if amount <= 0.0 {
        return 0.0;
    }
    let (u, v) = (p[0] + p[1] * 0.7, p[2] + p[1] * 0.3);
    // Ferns: thin ridges of noise, the branching lines of frost on a window.
    let ridge = |x: f32, y: f32| 1.0 - (fbm(x, y, 4) - 0.5).abs() * 2.0;
    let ferns = ridge(u * 3.2 + 7.0, v * 3.2 - 1.0).powf(3.0) * 0.7 + ridge(u * 9.0 - 2.0, v * 9.0 + 5.0).powf(4.0) * 0.5;
    let from_edge = smooth(amount * 0.75, amount * 0.75 - 0.35, edge);
    (ferns * from_edge + 0.25 * amount).clamp(0.0, 0.95)
}

// ---------- Ice on the water ----------

/// The water frozen: grey-blue ice, white where snow lies on it, crossed by cracks that are
/// darker and sharp, as on a lake in January. Gives the colour of the ice under the light,
/// and how much it mirrors.
pub(super) fn ice(p: V3, amount: f32, snow: f32) -> (V3, f32) {
    let (x, y) = (p[0], p[1]);
    // Cracks: the edges of cells of a Voronoi pattern at two sizes.
    let cracks = |scale: f32, seed: u32| -> f32 {
        let (gx, gy) = (x / scale, y / scale);
        let (cx, cy) = (gx.floor(), gy.floor());
        let (mut d1, mut d2) = (f32::MAX, f32::MAX);
        for j in -1..=1 {
            for i in -1..=1 {
                let (ci, cj) = ((cx as i32 + i) as u32, (cy as i32 + j) as u32);
                let (ox, oy) = (hash01(ci, cj, seed), hash01(ci, cj, seed + 1));
                let (dx, dy) = (cx + i as f32 + ox - gx, cy + j as f32 + oy - gy);
                let d = (dx * dx + dy * dy).sqrt();
                if d < d1 {
                    d2 = d1;
                    d1 = d;
                } else if d < d2 {
                    d2 = d;
                }
            }
        }
        // Wavering a little, as cracks run.
        let wiggle = (fbm(x * 0.6 + seed as f32, y * 0.6, 2) - 0.5) * 0.06;
        1.0 - smooth(0.0, 0.035, d2 - d1 + wiggle)
    };
    let crack = cracks(9.0, 31).max(cracks(3.0, 57) * 0.6);
    let clear = [0.10, 0.17, 0.22];
    let milky = [0.42, 0.52, 0.60];
    let cloudy = fbm(x * 0.08 + 2.0, y * 0.08 - 3.0, 4);
    let mut c = add(mul(clear, 1.0 - cloudy), mul(milky, cloudy));
    // Snow blown into patches on the ice, more with more snow on the ground.
    let drift = smooth(0.62 - snow * 0.4, 0.75 - snow * 0.4, fbm(x * 0.05 - 9.0, y * 0.05 + 4.0, 5));
    c = add(mul(c, 1.0 - drift), mul(SNOW, drift));
    c = mul(c, 1.0 - 0.55 * crack * (1.0 - drift));
    let c = add(mul(c, amount), mul(WATER_COLOUR, 1.0 - amount));
    (c, (0.08 * (1.0 - drift)) * amount)
}

// ---------- Icicles ----------

/// Icicles under the eaves: thin cones that hang from the low edges of roofs, more with more
/// cold. In the coordinates of the scene, each with its colour.
pub(super) fn icicles(m: &Mesh, local: &dyn Fn([f64; 3]) -> V3, amount: f32) -> Vec<[V3; 3]> {
    if amount <= 0.0 {
        return Vec::new();
    }
    // The edges of triangles that face up as a roof does: an edge with one such triangle is
    // where the roof ends. The lowest edge of each such triangle, if it is level, is an eave.
    let mut count: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    let mut roofs = Vec::new();
    for t in &m.tris {
        let p = t.v.map(|v| local(m.positions[v as usize]));
        let n = norm(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        if n[2].abs() < 0.25 || n[2].abs() > 0.995 {
            continue;
        }
        let name = m.materials.get(t.material as usize).map(|x| x.name.to_lowercase()).unwrap_or_default();
        if ["leaves", "needles", "trunk", "blossom", "branches"].iter().any(|w| name.contains(w)) {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (t.v[k], t.v[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
        roofs.push((t.v, p));
    }
    let mut out = Vec::new();
    let mut rng = Rng(0x01C1_C1E5);
    for (v, p) in roofs {
        for k in 0..3 {
            let (ia, ib) = (v[k], v[(k + 1) % 3]);
            if count.get(&(ia.min(ib), ia.max(ib))).copied().unwrap_or(0) != 1 {
                continue;
            }
            let (a, b, c) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
            // A level edge, below the third corner: the drip edge.
            if (a[2] - b[2]).abs() > 0.15 || c[2] <= a[2].max(b[2]) + 0.05 {
                continue;
            }
            let along = sub(b, a);
            let len = dot(along, along).sqrt();
            if len < 0.5 {
                continue;
            }
            let dir = mul(along, 1.0 / len);
            let side = norm([-dir[1], dir[0], 0.0]);
            let mut s = rng.next() * 0.3;
            while s < len {
                let foot = add(a, mul(dir, s));
                let long = (0.15 + 0.9 * rng.next().powi(2)) * (0.5 + amount);
                let r = 0.025 + 0.03 * rng.next();
                let tip = add(foot, [0.0, 0.0, -long]);
                // A cone of five sides.
                let ring: Vec<V3> = (0..5)
                    .map(|j| {
                        let ang = j as f32 / 5.0 * std::f32::consts::TAU;
                        add(foot, add(mul(dir, r * ang.cos()), mul(side, r * ang.sin())))
                    })
                    .collect();
                for j in 0..5 {
                    out.push([ring[j], ring[(j + 1) % 5], tip]);
                }
                s += (0.08 + 0.35 * rng.next()) / amount.max(0.15);
            }
        }
        if out.len() > 400_000 {
            break;
        }
    }
    out
}

/// The colour of icicles: clear ice, a little blue, lighter than water.
pub(super) const ICE: V3 = [0.62, 0.74, 0.84];

// ---------- Smoke ----------

/// Where smoke rises: the highest point of some of the buildings, as many as `amount` says,
/// the same ones every time.
pub(super) fn chimneys(m: &Mesh, local: &dyn Fn([f64; 3]) -> V3, amount: f32) -> Vec<V3> {
    if amount <= 0.0 {
        return Vec::new();
    }
    let n = m.objects.len().max(1);
    let mut top: Vec<Option<V3>> = vec![None; n];
    let mut low = vec![f32::INFINITY; n];
    for t in &m.tris {
        let name = m.materials.get(t.material as usize).map(|x| x.name.to_lowercase()).unwrap_or_default();
        if ["leaves", "needles", "trunk", "blossom", "branches"].iter().any(|w| name.contains(w)) {
            continue;
        }
        let k = (t.object as usize).min(n - 1);
        for v in t.v {
            let p = local(m.positions[v as usize]);
            low[k] = low[k].min(p[2]);
            if top[k].is_none_or(|q| p[2] > q[2]) {
                top[k] = Some(p);
            }
        }
    }
    top.iter().enumerate().filter_map(|(k, t)| t.filter(|p| p[2] - low[k] > 2.5 && hash01(k as u32, 41, 5) < amount)).take(400).collect()
}

/// Smoke from chimneys, or steam in the cold: puffs that rise, widen and thin, bent by the
/// wind, lit by the sky and the sun, hidden where something nearer stands.
#[allow(clippy::too_many_arguments)]
pub(super) fn smoke(colour: &mut [V3], depth: &[f32], cam: &Cam, sources: &[V3], down_wind: V3, wind: f32, lit: V3, shade: V3) {
    let (w, h) = (cam.width, cam.height);
    for (k, s) in sources.iter().enumerate() {
        let mut rng = Rng(0x5307_E000 ^ (k as u64 + 1).wrapping_mul(0x9E37_79B9));
        // Rises straight in still air, and is laid down by a wind.
        let rise = 1.0 - 0.75 * wind;
        let puffs = 46;
        for j in 0..puffs {
            let t = j as f32 / puffs as f32;
            let up = 26.0 * t.powf(0.8) * rise + 2.0;
            let drift = 34.0 * t * wind * (0.6 + 0.4 * t);
            let wobble = [(rng.next() - 0.5) * 3.0 * t, (rng.next() - 0.5) * 3.0 * t, (rng.next() - 0.5) * 2.0 * t];
            let p = add(add(add(*s, [0.0, 0.0, up]), mul(down_wind, drift)), wobble);
            let Some((x, y, z)) = cam.project(p) else { continue };
            let r = cam.pixels(0.7 + 5.5 * t, z).clamp(0.8, w as f32 * 0.2);
            let alpha = 0.22 * (1.0 - t).powf(1.3) * (0.7 + 0.6 * rng.next());
            // The top of a puff is lit, its foot in its own shade.
            let c = add(mul(lit, 0.6 + 0.4 * t), mul(shade, 0.4 - 0.4 * t));
            puff(colour, depth, w, h, x, y, z, r, c, alpha);
        }
    }
}

/// A soft round puff, hidden where something nearer stands.
#[allow(clippy::too_many_arguments)]
fn puff(colour: &mut [V3], depth: &[f32], w: usize, h: usize, x: f32, y: f32, z: f32, r: f32, c: V3, alpha: f32) {
    let reach = (r * 1.6).ceil() as i64;
    for yy in (y as i64 - reach).max(0)..(y as i64 + reach + 1).min(h as i64) {
        for xx in (x as i64 - reach).max(0)..(x as i64 + reach + 1).min(w as i64) {
            let i = yy as usize * w + xx as usize;
            if depth[i] < z {
                continue;
            }
            let (dx, dy) = (xx as f32 + 0.5 - x, yy as f32 + 0.5 - y);
            let a = alpha * (-(dx * dx + dy * dy) / (r * r) * 1.6).exp();
            colour[i] = add(mul(colour[i], 1.0 - a), mul(c, a));
        }
    }
}

// ---------- Advent ----------

/// What stands in a lit window in December: an advent candlestick of seven candles on the
/// sill, or a star hanging high, by how far across (`fu`) and up (`fv`) the window a place
/// is, from 0 to 1. Gives the light it adds, or none.
pub(super) fn advent(fu: f32, fv: f32, which: f32) -> Option<V3> {
    let warm = [3.2, 1.9, 0.75];
    if which < 0.55 {
        // Seven candles on a bow: the middle one highest.
        let base = 0.08;
        for i in 0..7 {
            let x = 0.2 + 0.6 * i as f32 / 6.0;
            let top = base + 0.2 - (x - 0.5).abs() * 0.3;
            let (dx, dy) = ((fu - x) / 0.034, (fv - (top + 0.05)) / 0.055);
            if dx * dx + dy * dy < 1.0 {
                return Some(mul(warm, 1.6));
            }
            if (fu - x).abs() < 0.02 && fv > base && fv < top {
                return Some([0.9, 0.85, 0.75]);
            }
        }
        if fv > base - 0.03 && fv < base + 0.01 && (0.18..0.82).contains(&fu) {
            return Some([0.15, 0.1, 0.06]);
        }
        None
    } else {
        // A star of paper with a lamp inside: five points, glowing.
        let (cx, cy) = (0.5, 0.72);
        let (dx, dy) = (fu - cx, (fv - cy) * 1.3);
        let r = (dx * dx + dy * dy).sqrt();
        let a = dy.atan2(dx) + std::f32::consts::FRAC_PI_2;
        let points = 0.5 + 0.5 * (a * 5.0).cos();
        let edge = 0.06 + 0.1 * points.powf(2.5);
        if r < edge {
            return Some(mul(warm, 1.2 + 0.8 * (1.0 - r / edge)));
        }
        if r < edge + 0.05 {
            return Some(mul(warm, 0.35 * (1.0 - (r - edge) / 0.05)));
        }
        None
    }
}

// ---------- Light in the air ----------

/// Rays of sun through gaps in clouds, fog or haze: the light of the bright sky near the
/// sun, smeared out from where the sun stands, as rays that fan out over the picture.
pub(super) fn sun_rays(colour: &mut [V3], depth: &[f32], cam: &Cam, sun: V3, sun_col: V3, amount: f32) {
    let (w, h) = (cam.width, cam.height);
    let far = add(cam.eye, mul(sun, cam.reach * 1000.0));
    let Some((sx, sy, _)) = cam.project(far) else { return };
    // Beyond the edges, but not far: rays from a sun out of the picture still fan in.
    if sx < -(w as f32) || sx > 2.0 * w as f32 || sy < -(h as f32) || sy > 2.0 * h as f32 {
        return;
    }
    // What gives rays: the brightest of the sky, nearest the sun. What stands in front of
    // it, a roof or a tree, casts the dark between the rays.
    let lum = |c: V3| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let mut sky: Vec<f32> = (0..w * h).filter(|i| depth[*i] == f32::INFINITY).map(|i| lum(colour[i])).collect();
    if sky.len() < 16 {
        return;
    }
    sky.sort_by(|a, b| a.total_cmp(b));
    let (mid, top) = (sky[sky.len() * 3 / 4], sky[sky.len() * 995 / 1000]);
    let tint = {
        let peak = sun_col.iter().fold(0.0f32, |a, b| a.max(*b)).max(1e-6);
        mul(sun_col, 1.0 / peak)
    };
    let mut bright = vec![[0.0f32; 3]; w * h];
    for i in 0..w * h {
        if depth[i] < f32::INFINITY {
            continue;
        }
        let (x, y) = ((i % w) as f32 - sx, (i / w) as f32 - sy);
        let near = (-(x * x + y * y).sqrt() / (w.max(h) as f32 * 0.3)).exp();
        let over = smooth(mid, top.max(mid + 1e-3), lum(colour[i]));
        // The glare of the sun itself, where the sky is seen: what stands in front of it,
        // a roof, a tower or a tree, casts its ray of shade.
        let glare = (-(x * x + y * y) / (w.max(h) as f32 * 0.05).powi(2)).exp() * 0.8;
        bright[i] = mul(tint, (over * near + glare) * top.min(4.0));
    }
    // A radial blur towards the sun, at a quarter of the size, for speed.
    let (qw, qh) = (w.div_ceil(4), h.div_ceil(4));
    let mut small = vec![[0.0f32; 3]; qw * qh];
    for (i, b) in bright.iter().enumerate() {
        let (x, y) = ((i % w) / 4, (i / w) / 4);
        let k = y * qw + x;
        small[k] = add(small[k], mul(*b, 1.0 / 16.0));
    }
    let samples = 64;
    let (qx, qy) = (sx / 4.0, sy / 4.0);
    let rays: Vec<V3> = (0..qw * qh)
        .into_par_iter()
        .map(|k| {
            let (x, y) = ((k % qw) as f32, (k / qw) as f32);
            let mut acc = [0.0f32; 3];
            let mut weight = 1.0f32;
            for s in 0..samples {
                let t = s as f32 / samples as f32 * 0.97;
                let (px, py) = (x + (qx - x) * t, y + (qy - y) * t);
                if px < 0.0 || py < 0.0 || px >= qw as f32 || py >= qh as f32 {
                    continue;
                }
                acc = add(acc, mul(small[py as usize * qw + px as usize], weight));
                weight *= 0.985;
            }
            mul(acc, 1.0 / samples as f32)
        })
        .collect();
    for (i, c) in colour.iter_mut().enumerate() {
        let (x, y) = ((i % w) / 4, (i / w) / 4);
        *c = add(*c, mul(rays[y * qw + x], 1.8 * amount));
    }
}

/// How much of the sun reaches a place under broken cloud: the shadows of clouds drift over
/// the land as large soft patches. 1 is all of it.
pub(super) fn cloud_shadow(p: V3, sun: V3, amount: f32, offset: [f32; 3]) -> f32 {
    if amount <= 0.0 || sun[2] <= 0.02 {
        return 1.0;
    }
    // Where the ray to the sun meets the clouds, high up; the clouds as large as the scene
    // makes them seen, and a real cloud at most.
    let scale = offset[2];
    let up = (scale * 1.5 - p[2]).max(0.0) / sun[2];
    let (x, y) = (p[0] + sun[0] * up + offset[0], p[1] + sun[1] * up + offset[1]);
    let c = fbm(x / scale, y / scale, 5);
    let shade = smooth(0.42, 0.58, c);
    1.0 - amount * 0.85 * shade
}

/// A rainbow: a bow 42° around the point opposite the sun, red outside and violet inside, a
/// fainter second bow at 51° with its colours the other way, and the sky lighter inside the
/// bow, where rain falls in sunlight.
pub(super) fn rainbow(colour: &mut [V3], depth: &[f32], cam: &Cam, sun: V3, amount: f32, backdrop: bool) {
    if sun[2] <= 0.0 || sun[2] > 42f32.to_radians().sin() {
        return;
    }
    let anti = mul(sun, -1.0);
    let (w, h) = (cam.width, cam.height);
    // The colours of the bow from its outer edge in, by angle.
    let bands: [(f32, V3); 6] = [
        (42.3, [0.9, 0.08, 0.05]),
        (41.9, [0.95, 0.45, 0.04]),
        (41.5, [0.9, 0.85, 0.1]),
        (41.0, [0.15, 0.8, 0.2]),
        (40.6, [0.1, 0.35, 0.95]),
        (40.2, [0.45, 0.1, 0.8]),
    ];
    let spectral = |a: f32, first: bool| -> V3 {
        let mut c = [0.0f32; 3];
        for (angle, col) in bands {
            let at = if first { angle } else { 51.0 + (42.3 - angle) * 1.8 };
            let width = if first { 0.45 } else { 0.8 };
            let f = (-((a - at) / width).powi(2)).exp();
            c = add(c, mul(col, f));
        }
        c
    };
    colour.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, c) in row.iter_mut().enumerate() {
            let i = y * w + x;
            // Over the sky, and faintly in front of what is far. In a studio, over the room
            // behind the models, its floor too, as on a painted backdrop.
            let far = if depth[i] == f32::INFINITY {
                1.0
            } else if backdrop {
                smooth(cam.reach * 1.1, cam.reach * 1.5, depth[i])
            } else {
                smooth(cam.reach * 0.8, cam.reach * 3.0, depth[i]) * 0.6
            };
            if far <= 0.0 {
                continue;
            }
            let px = ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0) * cam.half_w;
            let py = (1.0 - (y as f32 + 0.5) / h as f32 * 2.0) * cam.half_h;
            let d = norm(add(cam.fwd, add(mul(cam.right, px), mul(cam.up, py))));
            if d[2] < -0.02 && depth[i] == f32::INFINITY && !backdrop {
                continue;
            }
            let a = dot(d, anti).clamp(-1.0, 1.0).acos().to_degrees();
            if a > 56.0 {
                continue;
            }
            let lum = (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).max(0.02);
            let k = amount * far * lum.sqrt() * 0.55;
            let mut add_c = add(spectral(a, true), mul(spectral(a, false), 0.35));
            // Lighter inside the first bow, darker between the two (Alexander's band).
            if a < 40.0 {
                add_c = add(add_c, mul([0.25, 0.25, 0.27], smooth(40.0, 30.0, a) * 0.5));
            }
            // On a light sky or wall the colours are added to, the bow would be washed out:
            // there it also takes a little of the light where its colours are, as a filter.
            let band = spectral(a, true).iter().fold(0.0f32, |m, v| m.max(*v)).min(1.0);
            let light = smooth(0.4, 0.9, lum);
            *c = add(mul(*c, 1.0 - 0.35 * amount * far * band * light), mul(add_c, k));
            if (43.0..50.0).contains(&a) {
                *c = mul(*c, 1.0 - 0.12 * amount * far);
            }
        }
    });
}

/// Cones of light under street lamps, seen in fog, haze, snow or rain: the air lit from the
/// lamp down to the ground, brightest near the lamp.
pub(super) fn lamp_cones(colour: &mut [V3], depth: &[f32], cam: &Cam, lamps: &[(V3, V3)], height: f32, amount: f32) {
    let (w, h) = (cam.width, cam.height);
    let spread = 38f32.to_radians().tan();
    for (at, power) in lamps {
        let peak = power.iter().fold(0.0f32, |a, b| a.max(*b)).max(1e-6);
        let col = mul(*power, 1.0 / peak);
        // The cone: from the lamp down to the ground under it.
        let height = height.max(1.0);
        let foot = add(*at, [0.0, 0.0, -height]);
        let corners: Vec<(f32, f32)> = [
            add(foot, [-height * spread, -height * spread, 0.0]),
            add(foot, [height * spread, -height * spread, 0.0]),
            add(foot, [-height * spread, height * spread, 0.0]),
            add(foot, [height * spread, height * spread, 0.0]),
            *at,
        ]
        .iter()
        .filter_map(|p| cam.project(*p).map(|(x, y, _)| (x, y)))
        .collect();
        if corners.len() < 5 {
            continue;
        }
        let (x0, x1) = corners.iter().fold((f32::MAX, f32::MIN), |a, c| (a.0.min(c.0), a.1.max(c.0)));
        let (y0, y1) = corners.iter().fold((f32::MAX, f32::MIN), |a, c| (a.0.min(c.1), a.1.max(c.1)));
        if x1 < 0.0 || y1 < 0.0 || x0 > w as f32 || y0 > h as f32 || x1 - x0 > w as f32 * 2.0 {
            continue;
        }
        let (r0, r1) = ((y0 as i64).max(0) as usize, (y1 as i64 + 1).clamp(0, h as i64) as usize);
        let (c0, c1) = ((x0 as i64).max(0), (x1 as i64 + 1).min(w as i64));
        // Row by row on all the cores.
        colour[r0 * w..r1 * w].par_chunks_mut(w).enumerate().for_each(|(k, row)| {
            let yy = (r0 + k) as i64;
            for xx in c0..c1 {
                let i = yy as usize * w + xx as usize;
                let px = ((xx as f32 + 0.5) / w as f32 * 2.0 - 1.0) * cam.half_w;
                let py = (1.0 - (yy as f32 + 0.5) / h as f32 * 2.0) * cam.half_h;
                let (o, d) = if cam.parallel {
                    (add(cam.eye, add(mul(cam.right, px), mul(cam.up, py))), cam.fwd)
                } else {
                    (cam.eye, norm(add(cam.fwd, add(mul(cam.right, px), mul(cam.up, py)))))
                };
                // Along the ray, through the part of it that is near the lamp.
                let to = sub(*at, o);
                let mid = dot(to, d);
                let span = height * 1.3;
                let (t0, t1) = ((mid - span).max(0.0), (mid + span).min(depth[i] / dot(d, cam.fwd).max(1e-3)));
                if t1 <= t0 {
                    continue;
                }
                let steps = 20;
                let mut acc = 0.0f32;
                for s in 0..steps {
                    let t = t0 + (t1 - t0) * (s as f32 + 0.5) / steps as f32;
                    let q = add(o, mul(d, t));
                    let below = at[2] - q[2];
                    if !(0.0..height).contains(&below) {
                        continue;
                    }
                    let r = ((q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2)).sqrt();
                    let rim = below * spread + 0.15;
                    if r > rim {
                        continue;
                    }
                    // A clear edge, as the light of a shaded lamp has, a little brighter there.
                    let edge = smooth(rim, rim * 0.9, r) * (0.7 + 0.5 * smooth(rim * 0.6, rim * 0.95, r));
                    let dist2 = below * below + r * r + 0.3;
                    acc += edge / dist2;
                }
                let f = acc * (t1 - t0) / steps as f32 * 0.9 * amount;
                if f > 0.0 {
                    let c = &mut row[xx as usize];
                    *c = add(*c, mul(col, f));
                }
            }
        });
    }
}

// ---------- Night ----------

/// The trails of car lights along roads in a long exposure at night: white headlights on one
/// side and red tail lights on the other, as streaks of light that follow the roads.
pub(super) fn trails(colour: &mut [V3], depth: &[f32], cam: &Cam, roads: &[Vec<V3>], amount: f32) {
    let k = (cam.width.max(cam.height) as f32 / 1600.0).max(0.6);
    let white = [2.6, 2.4, 2.0];
    let red = [2.8, 0.25, 0.12];
    for (n, road) in roads.iter().enumerate() {
        let mut rng = Rng(0x7A11 ^ (n as u64 + 1).wrapping_mul(0x9E37_79B9));
        // A lane each way, a little apart, each with a few streaks side by side.
        for (lane, col) in [(1.0f32, white), (-1.0, red)] {
            for streak in 0..3 {
                let off = lane * (1.6 + 0.5 * streak as f32);
                let strength = amount * (0.5 + 0.5 * rng.next());
                let lift = 0.65 + 0.1 * streak as f32;
                for pair in road.windows(2) {
                    let along = sub(pair[1], pair[0]);
                    let len = dot(along, along).sqrt();
                    if len < 0.01 {
                        continue;
                    }
                    let side = norm([-along[1], along[0], 0.0]);
                    let a = add(add(pair[0], mul(side, off)), [0.0, 0.0, lift]);
                    let b = add(add(pair[1], mul(side, off)), [0.0, 0.0, lift]);
                    let (Some(pa), Some(pb)) = (cam.project(a), cam.project(b)) else { continue };
                    super::weather::glow_line(colour, depth, cam.width, cam.height, pa, pb, 0.9 * k, 7.0 * k, mul(col, strength * 0.7));
                }
            }
        }
    }
}

/// Where fireworks burst, high over what the camera looks at, and their colours.
pub(super) fn bursts(cam: &Cam, target: V3, count: u32) -> Vec<(V3, V3)> {
    let colours: [V3; 6] = [[1.0, 0.25, 0.2], [0.3, 0.6, 1.0], [1.0, 0.85, 0.3], [0.4, 1.0, 0.45], [0.95, 0.4, 1.0], [1.0, 1.0, 0.95]];
    let mut rng = Rng(0xF1AE_0047);
    let mut out = Vec::new();
    let mut tries = 0;
    while out.len() < count as usize && tries < count * 20 {
        tries += 1;
        // In the upper part of the picture, a little beyond what the camera looks at, and
        // well above the ground.
        let (x, y) = (rng.next() * 1.5 - 0.75, 0.25 + rng.next() * 0.6);
        let at = cam.at(x, y, cam.reach * (1.0 + 0.4 * rng.next()));
        if at[2] < target[2] + cam.reach * 0.08 {
            continue;
        }
        out.push((at, colours[(out.len() + (rng.next() * 6.0) as usize) % 6]));
    }
    out
}

/// Fireworks: each burst a sphere of sparks flying out and falling, as streaks of light in a
/// photograph, with a soft glow in the air around it.
pub(super) fn fireworks(colour: &mut [V3], depth: &[f32], cam: &Cam, bursts: &[(V3, V3)]) {
    let k = (cam.width.max(cam.height) as f32 / 1600.0).max(0.6);
    for (n, (at, col)) in bursts.iter().enumerate() {
        let mut rng = Rng(0xB0A5 ^ (n as u64 + 7).wrapping_mul(0x2545_F491));
        let radius = cam.reach * (0.07 + 0.04 * rng.next());
        let sparks = 70;
        for _ in 0..sparks {
            // Evenly over a sphere.
            let z = rng.next() * 2.0 - 1.0;
            let a = rng.next() * std::f32::consts::TAU;
            let r = (1.0 - z * z).sqrt();
            let dir = [r * a.cos(), r * a.sin(), z];
            let mut pts = Vec::with_capacity(8);
            for s in 0..8 {
                let t = s as f32 / 7.0;
                // Out fast, then slower, and falling.
                let out = radius * (1.0 - (1.0 - t).powi(2));
                pts.push(add(add(*at, mul(dir, out)), [0.0, 0.0, -radius * 0.35 * t * t]));
            }
            for (s, pair) in pts.windows(2).enumerate() {
                let (Some(pa), Some(pb)) = (cam.project(pair[0]), cam.project(pair[1])) else { continue };
                let fade = 1.0 - s as f32 / 7.0 * 0.6;
                super::weather::glow_line(colour, depth, cam.width, cam.height, pa, pb, 0.7 * k, 4.0 * k, mul(*col, 1.6 * fade));
            }
        }
        // The air around the burst, lit.
        if let Some((cx, cy, cz)) = cam.project(*at) {
            let r = cam.pixels(radius * 1.6, cz).max(4.0);
            let reach = (r * 2.0) as i64;
            for yy in (cy as i64 - reach).max(0)..(cy as i64 + reach).min(cam.height as i64) {
                for xx in (cx as i64 - reach).max(0)..(cx as i64 + reach).min(cam.width as i64) {
                    let i = yy as usize * cam.width + xx as usize;
                    if depth[i] < cz {
                        continue;
                    }
                    let (dx, dy) = (xx as f32 - cx, yy as f32 - cy);
                    let f = (-(dx * dx + dy * dy) / (r * r)).exp() * 0.08;
                    colour[i] = add(colour[i], mul(*col, f));
                }
            }
        }
    }
}

// ---------- The lens ----------

/// Tilt-shift: sharp in a band across the picture and more and more blurred above and below
/// it, with livelier colours, so that a town looks like a model of itself.
pub(super) fn tilt_shift(colour: &mut [V3], w: usize, h: usize, amount: f32, band: f32) {
    let most = (h as f32 * 0.012 * amount).max(1.0);
    // Blurred copies at a few sizes, chosen between row by row.
    let levels = [most * 0.25, most * 0.5, most];
    let copies: Vec<Vec<V3>> = levels.iter().map(|r| box_blur(colour, w, h, r.round().max(1.0) as usize)).collect();
    let half = h as f32 * 0.09;
    for y in 0..h {
        let d = ((y as f32 - band * h as f32).abs() - half).max(0.0) / (h as f32 * 0.35);
        let t = (d * 3.0).min(3.0);
        for x in 0..w {
            let i = y * w + x;
            let c = if t <= 0.0 {
                colour[i]
            } else {
                let k = t.floor() as usize;
                let f = t - k as f32;
                let a = if k == 0 { colour[i] } else { copies[k - 1][i] };
                let b = copies[k.min(2)][i];
                add(mul(a, 1.0 - f), mul(b, f))
            };
            // Livelier colours, as the photographs that do this have.
            let lum = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            colour[i] = c.map(|v| (lum + (v - lum) * (1.0 + 0.15 * amount)).max(0.0));
        }
    }
}

/// A box blur across and down, three times: near a Gaussian.
pub(super) fn box_blur(src: &[V3], w: usize, h: usize, r: usize) -> Vec<V3> {
    let mut a = src.to_vec();
    for _ in 0..3 {
        let rows: Vec<Vec<V3>> = (0..h)
            .into_par_iter()
            .map(|y| {
                let row = &a[y * w..(y + 1) * w];
                let mut out = vec![[0.0f32; 3]; w];
                let mut sum = [0.0f32; 3];
                for v in &row[..=r.min(w - 1)] {
                    sum = add(sum, *v);
                }
                sum = add(sum, mul(row[0], r as f32));
                for (x, o) in out.iter_mut().enumerate() {
                    *o = mul(sum, 1.0 / (2 * r + 1) as f32);
                    sum = add(sum, sub(row[(x + r + 1).min(w - 1)], row[x.saturating_sub(r)]));
                }
                out
            })
            .collect();
        let b: Vec<V3> = rows.into_iter().flatten().collect();
        let cols: Vec<Vec<V3>> = (0..w)
            .into_par_iter()
            .map(|x| {
                let mut out = vec![[0.0f32; 3]; h];
                let mut sum = [0.0f32; 3];
                for y in 0..=r.min(h - 1) {
                    sum = add(sum, b[y * w + x]);
                }
                sum = add(sum, mul(b[x], r as f32));
                for (y, o) in out.iter_mut().enumerate() {
                    *o = mul(sum, 1.0 / (2 * r + 1) as f32);
                    sum = add(sum, sub(b[(y + r + 1).min(h - 1) * w + x], b[y.saturating_sub(r) * w + x]));
                }
                out
            })
            .collect();
        for (x, col) in cols.into_iter().enumerate() {
            for (y, v) in col.into_iter().enumerate() {
                a[y * w + x] = v;
            }
        }
    }
    a
}

// ---------- Wind ----------

/// The way the wind blows, level, from the direction it comes from in degrees clockwise
/// from north.
pub(super) fn down_wind(from: f64) -> V3 {
    let a = (from as f32).to_radians();
    [-a.sin(), -a.cos(), 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ice_has_cracks_and_a_rainbow_needs_a_low_sun() {
        // Somewhere on a large piece of ice there are cracks: darker places.
        let shades: Vec<f32> = (0..400).map(|i| ice([i as f32 * 0.05, 3.0, 0.0], 1.0, 0.0).0[1]).collect();
        let (lo, hi) = shades.iter().fold((f32::MAX, f32::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
        assert!(hi > lo * 1.5, "{lo} {hi}");
        // Rime grows more on what faces up.
        let up: f32 = (0..50).map(|i| rime_on(0.7, [0.0, 0.0, 1.0], [i as f32, 0.0, 0.0])).sum();
        let down: f32 = (0..50).map(|i| rime_on(0.7, [0.0, 0.0, -1.0], [i as f32, 0.0, 0.0])).sum();
        assert!(up > down);
        // Candles stand on the sill, and a star hangs high.
        assert!(advent(0.5, 0.33, 0.1).is_some() && advent(0.5, 0.9, 0.1).is_none());
        assert!(advent(0.5, 0.72, 0.9).is_some() && advent(0.1, 0.1, 0.9).is_none());
        // The shadows of clouds come and go, and there are none with the sun down.
        let lit: Vec<f32> = (0..60).map(|i| cloud_shadow([i as f32 * 300.0, 0.0, 0.0], norm([0.3, 0.3, 0.8]), 1.0, [0.0, 0.0, 900.0])).collect();
        assert!(lit.iter().any(|v| *v < 0.5) && lit.iter().any(|v| *v > 0.9));
        assert_eq!(cloud_shadow([0.0; 3], [0.0, 1.0, 0.0], 1.0, [0.0, 0.0, 900.0]), 1.0);
        let wind = down_wind(270.0);
        assert!(wind[0] > 0.99, "a west wind blows east");
    }
}
