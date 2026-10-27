//! Terrain drawn from a grid of heights: shaded relief seen from above, and a 3D view from
//! the side. Everything is computed on the processor, spread over its cores.
//!
//! The light is that of a sun: each cell is lit by how much it faces the sun, is in shadow
//! when the terrain between it and the sun is higher, and gets light from the sky by how
//! much of the sky it sees. Colours follow the height, or come from a map laid over it.

use crate::model::{Grid, RasterImage};
use rayon::prelude::*;

/// What the shadows are like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shadows {
    None,
    Sharp,
    Soft,
}

/// The colours of the ground before it is lit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Colours {
    /// Green low, through yellow, to light brown at the top.
    Land,
    /// Green low, through yellow and brown, to snow at the top.
    Mountains,
    /// Grey low to white at the top.
    Greys,
    /// The same light colour all over, so that only the light shows.
    White,
}

/// How terrain is lit and coloured. Angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Where the sun is, clockwise from north.
    pub sun_azimuth: f64,
    /// How high the sun is above the horizon.
    pub sun_altitude: f64,
    /// Heights are multiplied by this, to make the terrain stand out.
    pub exaggeration: f64,
    pub shadows: Shadows,
    /// How dark the shadows are, from 0 to 1.
    pub shadow_darkness: f64,
    /// Valleys and hollows get less light from the sky.
    pub sky_light: bool,
    pub colours: Colours,
    /// A haze that lies in the low parts, from 0 to 1.
    pub haze: f64,
    /// How much a map laid over the terrain covers it, from 0 to 1.
    pub map_opacity: f64,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            sun_azimuth: 315.0,
            sun_altitude: 35.0,
            exaggeration: 1.5,
            shadows: Shadows::Soft,
            shadow_darkness: 0.6,
            sky_light: true,
            colours: Colours::Land,
            haze: 0.0,
            map_opacity: 1.0,
        }
    }
}

/// The colour of the haze and of the fog in a 3D view.
const HAZE: [f64; 3] = [226.0, 233.0, 240.0];

/// A grid of heights at another size, by interpolation between the cells. The new grid
/// covers the same area.
pub fn resample(g: &Grid, width: usize, height: usize) -> Grid {
    if width == g.width && height == g.height {
        return g.clone();
    }
    let gt = &g.geotransform;
    let (sx, sy) = (gt[1] * g.width as f64 / width as f64, gt[5] * g.height as f64 / height as f64);
    let fx = g.width as f64 / width as f64;
    let fy = g.height as f64 / height as f64;
    let values: Vec<f32> = (0..height)
        .into_par_iter()
        .flat_map_iter(|row| {
            let cy = (row as f64 + 0.5) * fy - 0.5;
            (0..width).map(move |col| {
                let cx = (col as f64 + 0.5) * fx - 0.5;
                if fx > 1.0 || fy > 1.0 {
                    // Coarser: the mean of the cells that fall in the new one.
                    let (c0, c1) = (((col as f64) * fx) as usize, (((col + 1) as f64) * fx).ceil() as usize);
                    let (r0, r1) = (((row as f64) * fy) as usize, (((row + 1) as f64) * fy).ceil() as usize);
                    let (mut sum, mut n) = (0.0f64, 0usize);
                    for r in r0..r1.min(g.height) {
                        for c in c0..c1.min(g.width) {
                            if let Some(v) = g.get(c, r) {
                                sum += v as f64;
                                n += 1;
                            }
                        }
                    }
                    if n == 0 {
                        f32::NAN
                    } else {
                        (sum / n as f64) as f32
                    }
                } else {
                    bilinear(g, cx, cy).map_or(f32::NAN, |v| v as f32)
                }
            })
        })
        .collect();
    Grid { bands: Vec::new(), width, height, values, geotransform: [gt[0], sx, 0.0, gt[3], 0.0, sy], crs: g.crs }
}

/// The height at a place in the coordinates of the grid, between the middles of the cells.
/// None outside the grid or where it has no value.
pub fn height_at(g: &Grid, x: f64, y: f64) -> Option<f64> {
    let gt = g.geotransform;
    if gt[1] == 0.0 || gt[5] == 0.0 {
        return None;
    }
    let (cx, cy) = ((x - gt[0]) / gt[1] - 0.5, (y - gt[3]) / gt[5] - 0.5);
    if cx < -0.5 || cy < -0.5 || cx > g.width as f64 - 0.5 || cy > g.height as f64 - 0.5 {
        return None;
    }
    bilinear(g, cx, cy)
}

/// The height at a place in cell coordinates, where the middle of the first cell is 0, 0.
fn bilinear(g: &Grid, x: f64, y: f64) -> Option<f64> {
    let x = x.clamp(0.0, (g.width - 1) as f64);
    let y = y.clamp(0.0, (g.height - 1) as f64);
    let (c, r) = (x.floor() as usize, y.floor() as usize);
    let (c1, r1) = ((c + 1).min(g.width - 1), (r + 1).min(g.height - 1));
    let (tx, ty) = (x - c as f64, y - r as f64);
    let v00 = g.get(c, r)? as f64;
    let v10 = g.get(c1, r).map_or(v00, |v| v as f64);
    let v01 = g.get(c, r1).map_or(v00, |v| v as f64);
    let v11 = g.get(c1, r1).map_or(v00, |v| v as f64);
    Some(v00 * (1.0 - tx) * (1.0 - ty) + v10 * tx * (1.0 - ty) + v01 * (1.0 - tx) * ty + v11 * tx * ty)
}

/// The colour of a height, `t` from 0 at the lowest to 1 at the highest.
pub(crate) fn tint(colours: Colours, t: f64) -> [f64; 3] {
    const LAND: [(f64, [f64; 3]); 5] =
        [(0.0, [96.0, 140.0, 92.0]), (0.3, [146.0, 176.0, 106.0]), (0.6, [204.0, 198.0, 146.0]), (0.82, [190.0, 166.0, 128.0]), (1.0, [214.0, 200.0, 180.0])];
    const MOUNTAINS: [(f64, [f64; 3]); 6] = [
        (0.0, [104.0, 146.0, 98.0]),
        (0.22, [150.0, 178.0, 112.0]),
        (0.45, [214.0, 205.0, 150.0]),
        (0.68, [188.0, 158.0, 120.0]),
        (0.86, [160.0, 142.0, 132.0]),
        (1.0, [248.0, 248.0, 248.0]),
    ];
    const GREYS: [(f64, [f64; 3]); 2] = [(0.0, [168.0, 172.0, 178.0]), (1.0, [250.0, 250.0, 250.0])];
    let ramp: &[(f64, [f64; 3])] = match colours {
        Colours::Land => &LAND,
        Colours::Mountains => &MOUNTAINS,
        Colours::Greys => &GREYS,
        Colours::White => return [242.0, 241.0, 236.0],
    };
    let t = t.clamp(0.0, 1.0);
    for w in ramp.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let k = if b > a { (t - a) / (b - a) } else { 0.0 };
            return [ca[0] + (cb[0] - ca[0]) * k, ca[1] + (cb[1] - ca[1]) * k, ca[2] + (cb[2] - ca[2]) * k];
        }
    }
    ramp[ramp.len() - 1].1
}

/// The way along the ground toward a direction, as steps in cells: (columns, rows) per
/// unit of distance in the coordinate system. North is up, so rows go the other way.
fn step_toward(azimuth: f64, sx: f64, sy: f64) -> (f64, f64) {
    let a = azimuth.to_radians();
    (a.sin() / sx, -a.cos() / sy)
}

/// The steepest angle up to the terrain seen from a cell toward a direction, as its
/// tangent, looking no farther than `reach` and stopping early once nothing farther can
/// be steeper than `enough`. Angles below `floor` do not matter, so the search stops when
/// nothing farther can be steeper than that. `dir` is in cells per unit
/// of distance, and `cell`, the first step, and `reach` are distances.
#[allow(clippy::too_many_arguments)]
fn horizon(
    g: &Grid,
    col: usize,
    row: usize,
    h0: f64,
    dir: (f64, f64),
    cell: f64,
    reach: f64,
    top: f64,
    (floor, enough): (f64, f64),
    e: f64,
    growth: f64,
) -> f64 {
    let mut best = f64::NEG_INFINITY;
    let mut d = cell;
    let (cx, cy) = (col as f64, row as f64);
    while d <= reach {
        let (x, y) = (cx + dir.0 * d, cy + dir.1 * d);
        if x < 0.0 || y < 0.0 || x > (g.width - 1) as f64 || y > (g.height - 1) as f64 {
            break;
        }
        if let Some(h) = bilinear(g, x, y) {
            let tan = (h - h0) * e / d;
            if tan > best {
                best = tan;
                if best >= enough {
                    break;
                }
            }
        }
        // Nothing farther can rise above the highest point of the grid, so once that is
        // no steeper than what has been found, or than what matters, the answer is known.
        if (top - h0) * e / d <= best.max(floor) {
            break;
        }
        // The steps grow with the distance: near detail matters more than far.
        d += cell.max(d * growth);
    }
    best
}

/// How much of the sky each cell sees, from 0 to 1, measured as how far the ground around
/// it rises above it on average, at distances from 2 to 64 cells. Means over squares are
/// taken from a table of sums, so the time does not grow with the distance.
fn sky_view(g: &Grid, e: f64) -> Vec<f32> {
    let (w, h) = (g.width, g.height);
    let (sx, sy) = g.cell_size();
    let cell = (sx + sy) / 2.0;
    // Sums of heights and of cells with a height, from the corner to each cell.
    let mut sum = vec![0.0f64; (w + 1) * (h + 1)];
    let mut cnt = vec![0u32; (w + 1) * (h + 1)];
    for r in 0..h {
        let (mut row_sum, mut row_cnt) = (0.0, 0u32);
        for c in 0..w {
            if let Some(v) = g.get(c, r) {
                row_sum += v as f64;
                row_cnt += 1;
            }
            sum[(r + 1) * (w + 1) + c + 1] = sum[r * (w + 1) + c + 1] + row_sum;
            cnt[(r + 1) * (w + 1) + c + 1] = cnt[r * (w + 1) + c + 1] + row_cnt;
        }
    }
    let mean = |c0: usize, r0: usize, c1: usize, r1: usize| -> Option<f64> {
        let k = |c: usize, r: usize| r * (w + 1) + c;
        let n = cnt[k(c1, r1)] + cnt[k(c0, r0)] - cnt[k(c1, r0)] - cnt[k(c0, r1)];
        (n > 0).then(|| (sum[k(c1, r1)] + sum[k(c0, r0)] - sum[k(c1, r0)] - sum[k(c0, r1)]) / n as f64)
    };
    let radii = [2usize, 4, 8, 16, 32, 64];
    (0..h)
        .into_par_iter()
        .flat_map_iter(|r| {
            let mean = &mean;
            (0..w).map(move |c| {
                let Some(h0) = g.get(c, r).map(|v| v as f64) else { return f32::NAN };
                let mut covered = 0.0;
                for k in radii {
                    let (c0, r0) = (c.saturating_sub(k), r.saturating_sub(k));
                    let (c1, r1) = ((c + k + 1).min(w), (r + k + 1).min(h));
                    if let Some(m) = mean(c0, r0, c1, r1) {
                        // The mean of a square of half side k lies at about half that distance.
                        let rise = (m - h0) * e / (k as f64 * cell * 0.5);
                        covered += rise.max(0.0).atan().sin();
                    }
                }
                (1.0 - covered / radii.len() as f64 * 1.6).clamp(0.0, 1.0).powf(1.5) as f32
            })
        })
        .collect()
}

/// The light on each cell, from 0 up: the sun as it falls on the cell and the light of
/// the sky, with shadows. 1 is flat open ground in sunlight.
pub fn light(g: &Grid, look: &Look) -> Vec<f32> {
    let (sx, sy) = g.cell_size();
    let e = look.exaggeration.max(0.01);
    let (az, alt) = (look.sun_azimuth.to_radians(), look.sun_altitude.clamp(1.0, 90.0).to_radians());
    let sun = [az.sin() * alt.cos(), az.cos() * alt.cos(), alt.sin()];
    let top = g.range().map_or(0.0, |r| r.1 as f64);
    let dir_sun = step_toward(look.sun_azimuth, sx, sy);
    let cell = sx.min(sy);
    let diag = ((g.width as f64 * sx).powi(2) + (g.height as f64 * sy).powi(2)).sqrt();
    // Soft shadows have an edge as wide as the sun is seen from the earth, made wider.
    let soft = 3f64.to_radians();
    let sky_view = if look.sky_light { sky_view(g, e) } else { Vec::new() };
    (0..g.height)
        .into_par_iter()
        .flat_map_iter(|row| {
            let sky_view = &sky_view;
            (0..g.width).map(move |col| {
                let Some(h0) = g.get(col, row).map(|v| v as f64) else { return f32::NAN };
                let at = |c: isize, r: isize| -> f64 {
                    let (c, r) = (c.clamp(0, g.width as isize - 1) as usize, r.clamp(0, g.height as isize - 1) as usize);
                    g.get(c, r).map_or(h0, |v| v as f64)
                };
                let (c, r) = (col as isize, row as isize);
                // Horn's slopes, east and north, with the heights made higher.
                let dzdx =
                    ((at(c + 1, r - 1) + 2.0 * at(c + 1, r) + at(c + 1, r + 1)) - (at(c - 1, r - 1) + 2.0 * at(c - 1, r) + at(c - 1, r + 1))) / (8.0 * sx) * e;
                let dzdy =
                    ((at(c - 1, r - 1) + 2.0 * at(c, r - 1) + at(c + 1, r - 1)) - (at(c - 1, r + 1) + 2.0 * at(c, r + 1) + at(c + 1, r + 1))) / (8.0 * sy) * e;
                let n = [-dzdx, -dzdy, 1.0];
                let len = (n[0] * n[0] + n[1] * n[1] + 1.0).sqrt();
                let facing = ((n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]) / len).max(0.0);
                let lit = match look.shadows {
                    Shadows::None => 1.0,
                    Shadows::Sharp => {
                        let hz = horizon(g, col, row, h0, dir_sun, cell, diag, top, (alt.tan() * 0.999, alt.tan()), e, 0.04);
                        if hz >= alt.tan() {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    Shadows::Soft => {
                        let hz = horizon(g, col, row, h0, dir_sun, cell, diag, top, ((alt - soft).max(0.0).tan(), (alt + soft).tan()), e, 0.04);
                        let angle = hz.atan();
                        ((alt + soft - angle) / (2.0 * soft)).clamp(0.0, 1.0)
                    }
                };
                let lit = 1.0 - look.shadow_darkness.clamp(0.0, 1.0) * (1.0 - lit);
                // How much of the sky the cell sees: 1 on open ground, less in a valley.
                let sky = sky_view.get(row * g.width + col).map_or(1.0, |v| *v as f64);
                // The sun makes flat ground 1. Slopes toward it are brighter, away darker.
                let direct = facing / sun[2] * lit;
                (0.3 * sky + 0.7 * direct * (0.55 + 0.45 * sky)) as f32
            })
        })
        .collect()
}

/// A picture with coordinates sampled at a place, or None outside it.
fn sample_image(img: &RasterImage, x: f64, y: f64) -> Option<[f64; 4]> {
    let g = &img.geotransform;
    let col = (x - g[0]) / g[1];
    let row = (y - g[3]) / g[5];
    if col < 0.0 || row < 0.0 || col >= img.width as f64 || row >= img.height as f64 {
        return None;
    }
    let k = (row as usize * img.width + col as usize) * 4;
    let p = &img.rgba[k..k + 4];
    Some([p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64 / 255.0])
}

/// The colour of a cell before it is lit, and its height from 0 at the lowest to 1 at the
/// highest: the colour of the height, with the map laid over it.
fn cell_colour(g: &Grid, look: &Look, map: Option<&RasterImage>, (lo, span): (f64, f64), col: usize, row: usize) -> [f64; 4] {
    let Some(h) = g.get(col, row) else { return [0.0, 0.0, 0.0, 0.0] };
    let t = (h as f64 - lo) / span;
    let mut c = tint(look.colours, t);
    if let Some(m) = map {
        let gt = &g.geotransform;
        let (x, y) = (gt[0] + (col as f64 + 0.5) * gt[1], gt[3] + (row as f64 + 0.5) * gt[5]);
        if let Some(p) = sample_image(m, x, y) {
            let a = p[3] * look.map_opacity.clamp(0.0, 1.0);
            c = [c[0] * (1.0 - a) + p[0] * a, c[1] * (1.0 - a) + p[1] * a, c[2] * (1.0 - a) + p[2] * a];
        }
    }
    [c[0], c[1], c[2], t]
}

/// The colour of a cell once it is lit, with the haze of the low parts.
fn lit_colour(c: [f64; 4], light: f32, look: &Look) -> [u8; 4] {
    let l = light as f64;
    // Above full light the colour goes toward white, slowly, rather than being cut off.
    let mut rgb = [0.0; 3];
    for i in 0..3 {
        let v = c[i] * l.min(1.0) + (255.0 - c[i]) * ((l - 1.0).max(0.0) * 0.4).min(0.25);
        let haze = look.haze.clamp(0.0, 1.0) * (1.0 - c[3]).powi(3);
        rgb[i] = v * (1.0 - haze) + HAZE[i] * haze;
    }
    [rgb[0].round().clamp(0.0, 255.0) as u8, rgb[1].round().clamp(0.0, 255.0) as u8, rgb[2].round().clamp(0.0, 255.0) as u8, 255]
}

/// Shaded relief seen from above, as a picture with the coordinates of the grid. Cells
/// without a height are see-through.
pub fn shade(g: &Grid, look: &Look, map: Option<&RasterImage>) -> RasterImage {
    let light = light(g, look);
    paint(g, &light, look, map)
}

/// Shaded relief of a given width, keeping the shape of the grid. A picture wider than the
/// grid gets its light from the grid itself, spread out over the pixels, since the cells hold
/// no more detail than that and the shadows take a long time to find.
pub fn shade_width(g: &Grid, look: &Look, map: Option<&RasterImage>, width: usize) -> RasterImage {
    let out = of_width(g, width);
    if out.width <= g.width {
        return shade(&out, look, map);
    }
    let light = light(g, look);
    let (fx, fy) = (g.width as f64 / out.width as f64, g.height as f64 / out.height as f64);
    let spread: Vec<f32> = (0..out.height)
        .into_par_iter()
        .flat_map_iter(|row| {
            let (light, out) = (&light, &out);
            (0..out.width).map(move |col| {
                let (x, y) = ((col as f64 + 0.5) * fx - 0.5, (row as f64 + 0.5) * fy - 0.5);
                let (x, y) = (x.clamp(0.0, (g.width - 1) as f64), y.clamp(0.0, (g.height - 1) as f64));
                let (c, r) = (x.floor() as usize, y.floor() as usize);
                let (c1, r1) = ((c + 1).min(g.width - 1), (r + 1).min(g.height - 1));
                let (tx, ty) = (x - c as f64, y - r as f64);
                let at = |c: usize, r: usize| light[r * g.width + c] as f64;
                let v00 = at(c, r);
                if v00.is_nan() || out.get(col, row).is_none() {
                    return f32::NAN;
                }
                let or = |v: f64| if v.is_nan() { v00 } else { v };
                (v00 * (1.0 - tx) * (1.0 - ty) + or(at(c1, r)) * tx * (1.0 - ty) + or(at(c, r1)) * (1.0 - tx) * ty + or(at(c1, r1)) * tx * ty) as f32
            })
        })
        .collect();
    paint(&out, &spread, look, map)
}

/// The picture: the colour of each cell, lit.
fn paint(g: &Grid, light: &[f32], look: &Look, map: Option<&RasterImage>) -> RasterImage {
    let (lo, hi) = g.range().map_or((0.0, 1.0), |r| (r.0 as f64, r.1 as f64));
    let range = (lo, if hi > lo { hi - lo } else { 1.0 });
    let w = g.width;
    let rgba: Vec<u8> = light
        .par_iter()
        .enumerate()
        .flat_map_iter(|(k, l)| if l.is_nan() { [0, 0, 0, 0] } else { lit_colour(cell_colour(g, look, map, range, k % w, k / w), *l, look) })
        .collect();
    RasterImage { width: g.width, height: g.height, rgba, geotransform: g.geotransform }
}

/// Where the camera of a 3D view is, and what it sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The direction the camera looks from, clockwise from north: 180 looks north from the south.
    pub azimuth: f64,
    /// How far above the horizon the camera looks down, in degrees.
    pub tilt: f64,
    /// The angle the picture spans from top to bottom, in degrees.
    pub fov: f64,
    /// More than 1 comes closer, less goes farther away.
    pub zoom: f64,
    pub width: usize,
    pub height: usize,
    /// Fog that grows with the distance, from 0 to 1.
    pub fog: f64,
    /// The background: a sky, or see-through.
    pub sky: bool,
    /// The terrain stands on a block with sides, as in a block diagram.
    pub block: bool,
    /// Each pixel is computed as this many by this many, for smooth edges.
    pub quality: usize,
}

impl Default for View {
    fn default() -> Self {
        View { azimuth: 200.0, tilt: 30.0, fov: 30.0, zoom: 1.0, width: 1600, height: 1000, fog: 0.3, sky: true, block: true, quality: 2 }
    }
}

type V3 = [f64; 3];
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A point of a triangle on the screen: x and y in pixels, the depth, and what is laid
/// on it: the place in the texture, or a colour for the sides of the block.
#[derive(Clone, Copy)]
struct Vert {
    x: f64,
    y: f64,
    z: f64,
    u: f64,
    v: f64,
}

/// A 3D view of the terrain, as a picture without coordinates. The terrain is shaded from
/// above as by `shade`, and that picture is laid over it, so the light and the shadows are
/// the same in both.
pub fn render(g: &Grid, look: &Look, map: Option<&RasterImage>, view: &View) -> RasterImage {
    // The texture is the shaded relief at up to 3000 cells across; the surface that is drawn
    // has up to a million cells.
    // A block has straight sides: a grid with a ragged edge, such as one that has been
    // reprojected, is cut to the rectangle inside it where every edge cell has a height.
    let trimmed;
    let g = if view.block {
        trimmed = trim_to_rectangle(&fit(g, 3000));
        &trimmed
    } else {
        g
    };
    let tex_grid = fit(g, 3000);
    let texture = shade(&tex_grid, look, map);
    let mesh = fit(g, 1000);
    let e = look.exaggeration.max(0.01);
    let (lo, hi) = mesh.range().map_or((0.0, 1.0), |r| (r.0 as f64, r.1 as f64));
    let gt = mesh.geotransform;
    let (w_world, h_world) = (mesh.width as f64 * gt[1], mesh.height as f64 * -gt[5]);
    let (cx, cy) = (gt[0] + w_world / 2.0, gt[3] - h_world / 2.0);
    let relief = (hi - lo) * e;
    // The block goes down below the lowest point by a part of its size.
    let base = -(relief * 0.25 + w_world.max(h_world) * 0.02);
    // Points of the surface around the middle: east, north and up, with the lowest point at 0.
    let world = |col: f64, row: f64, h: f64| -> V3 { [gt[0] + col * gt[1] - cx, gt[3] + row * gt[5] - cy, (h - lo) * e] };

    let ss = view.quality.clamp(1, 4);
    let (w, h) = (view.width.max(16) * ss, view.height.max(16) * ss);
    let fov = view.fov.clamp(5.0, 120.0).to_radians();
    let radius = ((w_world / 2.0).powi(2) + (h_world / 2.0).powi(2) + (relief / 2.0).powi(2)).sqrt();
    let aspect = w as f64 / h as f64;
    let (az, tilt) = (view.azimuth.to_radians(), view.tilt.clamp(1.0, 89.0).to_radians());
    let tan_y = (fov / 2.0).tan();
    let tan_x = tan_y * aspect;
    let camera = |target: V3, dist: f64| -> (V3, V3, V3, V3) {
        let eye: V3 = [target[0] + az.sin() * tilt.cos() * dist, target[1] + az.cos() * tilt.cos() * dist, target[2] + tilt.sin() * dist];
        let fwd = norm(sub(target, eye));
        let right = norm(cross(fwd, [0.0, 0.0, 1.0]));
        (eye, fwd, right, cross(right, fwd))
    };
    // What has to be in the picture: points of the surface, and the foot of the block.
    let floor = if view.block { base } else { 0.0 };
    let mut outline: Vec<V3> = Vec::new();
    let step = (mesh.width.max(mesh.height) / 40).max(1);
    for r in (0..mesh.height).step_by(step) {
        for c in (0..mesh.width).step_by(step) {
            if let Some(v) = mesh.get(c, r) {
                outline.push(world(c as f64 + 0.5, r as f64 + 0.5, v as f64));
            }
        }
    }
    for (c, r) in [(0.5, 0.5), (mesh.width as f64 - 0.5, 0.5), (0.5, mesh.height as f64 - 0.5), (mesh.width as f64 - 0.5, mesh.height as f64 - 0.5)] {
        let p = world(c, r, lo);
        outline.push([p[0], p[1], floor]);
    }
    // The camera is moved until all of it fills the picture, with a little room around.
    let mut target: V3 = [0.0, 0.0, relief * 0.35];
    let mut dist = radius / (fov / 2.0).sin();
    for _ in 0..6 {
        let (eye, fwd, right, up) = camera(target, dist);
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in &outline {
            let d = sub(*p, eye);
            let z = dot(d, fwd).max(dist * 1e-3);
            let (x, y) = (dot(d, right) / z, dot(d, up) / z);
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        for i in 0..3 {
            target[i] += right[i] * mx * dist + up[i] * my * dist;
        }
        let fill = ((x1 - x0) / (2.0 * tan_x * 0.9)).max((y1 - y0) / (2.0 * tan_y * 0.9));
        dist *= fill.clamp(0.3, 3.0);
    }
    let dist = dist / view.zoom.max(0.05);
    let (eye, fwd, right, up) = camera(target, dist);
    let f_px = (h as f64 / 2.0) / tan_y;
    let project = |p: V3| -> Option<(f64, f64, f64)> {
        let d = sub(p, eye);
        let z = dot(d, fwd);
        if z <= dist * 1e-3 {
            return None;
        }
        Some((w as f64 / 2.0 + dot(d, right) / z * f_px, h as f64 / 2.0 - dot(d, up) / z * f_px, z))
    };

    let mut depth = vec![f32::INFINITY; w * h];
    // What each pixel shows: the place in the texture, or a side of the block (u < 0, with
    // its light in v).
    let mut what = vec![(f32::NAN, f32::NAN); w * h];
    let mut draw = |a: Vert, b: Vert, c: Vert| {
        let (x0, x1) = (a.x.min(b.x).min(c.x).floor().max(0.0), a.x.max(b.x).max(c.x).ceil().min(w as f64 - 1.0));
        let (y0, y1) = (a.y.min(b.y).min(c.y).floor().max(0.0), a.y.max(b.y).max(c.y).ceil().min(h as f64 - 1.0));
        if x0 > x1 || y0 > y1 {
            return;
        }
        let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
        if area.abs() < 1e-12 {
            return;
        }
        for py in y0 as usize..=y1 as usize {
            let yc = py as f64 + 0.5;
            for px in x0 as usize..=x1 as usize {
                let xc = px as f64 + 0.5;
                let w0 = ((b.x - xc) * (c.y - yc) - (c.x - xc) * (b.y - yc)) / area;
                let w1 = ((c.x - xc) * (a.y - yc) - (a.x - xc) * (c.y - yc)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                    continue;
                }
                // Interpolated so that the texture follows the perspective.
                let iz = w0 / a.z + w1 / b.z + w2 / c.z;
                let z = 1.0 / iz;
                let k = py * w + px;
                if (z as f32) < depth[k] {
                    depth[k] = z as f32;
                    let u = (w0 * a.u / a.z + w1 * b.u / b.z + w2 * c.u / c.z) * z;
                    let v = (w0 * a.v / a.z + w1 * b.v / b.z + w2 * c.v / c.z) * z;
                    what[k] = (u as f32, v as f32);
                }
            }
        }
    };
    let tex_u = |col: f64| col / mesh.width as f64 * texture.width as f64;
    let tex_v = |row: f64| row / mesh.height as f64 * texture.height as f64;
    // The surface: two triangles for each square between the middles of four cells.
    let verts: Vec<Option<Vert>> = (0..mesh.height)
        .flat_map(|r| (0..mesh.width).map(move |c| (c, r)))
        .map(|(c, r)| {
            let hv = mesh.get(c, r)? as f64;
            let (fc, fr) = (c as f64 + 0.5, r as f64 + 0.5);
            let (x, y, z) = project(world(fc, fr, hv))?;
            Some(Vert { x, y, z, u: tex_u(fc), v: tex_v(fr) })
        })
        .collect();
    for r in 0..mesh.height.saturating_sub(1) {
        for c in 0..mesh.width.saturating_sub(1) {
            let at = |c: usize, r: usize| verts[r * mesh.width + c];
            if let (Some(a), Some(b), Some(cc), Some(d)) = (at(c, r), at(c + 1, r), at(c, r + 1), at(c + 1, r + 1)) {
                draw(a, b, d);
                draw(a, d, cc);
            }
        }
    }
    // The sides of the block, lit by how they face the sun. They follow the edge of the
    // surface wherever it goes: a grid that has been reprojected is turned a little, and
    // its edge then runs across the cells, with no heights outside it.
    if view.block {
        let sun_az = look.sun_azimuth.to_radians();
        let (mw, mh) = (mesh.width as isize, mesh.height as isize);
        // A square between the middles of four cells is drawn when all four have a height.
        let quad = |c: isize, r: isize| {
            c >= 0
                && r >= 0
                && c + 1 < mw
                && r + 1 < mh
                && [(0, 0), (1, 0), (0, 1), (1, 1)].iter().all(|(dc, dr)| mesh.get((c + dc) as usize, (r + dr) as usize).is_some())
        };
        let mut wall = |a: (isize, isize), b: (isize, isize), facing: f64| {
            let light = 0.55 + 0.35 * (facing - sun_az).cos().max(0.0);
            let (Some(h0), Some(h1)) = (mesh.get(a.0 as usize, a.1 as usize), mesh.get(b.0 as usize, b.1 as usize)) else { return };
            let top0 = world(a.0 as f64 + 0.5, a.1 as f64 + 0.5, h0 as f64);
            let top1 = world(b.0 as f64 + 0.5, b.1 as f64 + 0.5, h1 as f64);
            let p = [top0, top1, [top1[0], top1[1], base], [top0[0], top0[1], base]];
            let q: Option<Vec<Vert>> =
                p.iter().enumerate().map(|(i, v)| project(*v).map(|(x, y, z)| Vert { x, y, z, u: -1.0, v: light * if i >= 2 { 0.8 } else { 1.0 } })).collect();
            if let Some(q) = q {
                draw(q[0], q[1], q[2]);
                draw(q[0], q[2], q[3]);
            }
        };
        use std::f64::consts::{FRAC_PI_2, PI};
        for r in 0..mh - 1 {
            for c in 0..mw - 1 {
                if !quad(c, r) {
                    continue;
                }
                if !quad(c, r - 1) {
                    wall((c, r), (c + 1, r), 0.0);
                }
                if !quad(c, r + 1) {
                    wall((c, r + 1), (c + 1, r + 1), PI);
                }
                if !quad(c - 1, r) {
                    wall((c, r), (c, r + 1), -FRAC_PI_2);
                }
                if !quad(c + 1, r) {
                    wall((c + 1, r), (c + 1, r + 1), FRAC_PI_2);
                }
            }
        }
    }

    // The colours: the texture, the sides, fog by the distance, and the sky behind.
    let near = (dot(sub(target, eye), fwd) - radius).max(0.0);
    let fog_density = view.fog.clamp(0.0, 1.0) * 2.5;
    // The colour of one sample, worked out when the picture needs it rather than kept.
    let pixel = |k: usize| -> [f64; 4] {
        let (u, v) = what[k];
        let z = depth[k] as f64;
        if !z.is_finite() {
            if !view.sky {
                return [0.0, 0.0, 0.0, 0.0];
            }
            let t = (k / w) as f64 / h as f64;
            let top = [168.0, 196.0, 228.0];
            return [top[0] + (HAZE[0] - top[0]) * t, top[1] + (HAZE[1] - top[1]) * t, top[2] + (HAZE[2] - top[2]) * t, 255.0];
        }
        let mut c = if u < 0.0 {
            let l = v as f64;
            [150.0 * l, 132.0 * l, 112.0 * l]
        } else {
            let (tx, ty) = ((u as usize).min(texture.width - 1), (v as usize).min(texture.height - 1));
            let p = &texture.rgba[(ty * texture.width + tx) * 4..][..4];
            [p[0] as f64, p[1] as f64, p[2] as f64]
        };
        let fog = 1.0 - (-fog_density * ((z - near).max(0.0) / (2.0 * radius))).exp();
        for i in 0..3 {
            c[i] = c[i] * (1.0 - fog) + HAZE[i] * fog;
        }
        [c[0], c[1], c[2], 255.0]
    };
    // Each pixel of the picture is the mean of its samples.
    let (ow, oh) = (w / ss, h / ss);
    let rgba: Vec<u8> = (0..oh)
        .into_par_iter()
        .flat_map_iter(|row| {
            let pixel = &pixel;
            (0..ow).flat_map(move |col| {
                let mut sum = [0.0; 4];
                for dy in 0..ss {
                    for dx in 0..ss {
                        let p = pixel((row * ss + dy) * w + col * ss + dx);
                        // Colours are weighed by how much they cover, so edges do not darken.
                        for i in 0..3 {
                            sum[i] += p[i] * p[3];
                        }
                        sum[3] += p[3];
                    }
                }
                let n = (ss * ss) as f64;
                let a = sum[3] / n;
                let c = |i: usize| if sum[3] > 0.0 { (sum[i] / sum[3]).round().clamp(0.0, 255.0) as u8 } else { 0 };
                [c(0), c(1), c(2), a.round().clamp(0.0, 255.0) as u8]
            })
        })
        .collect();
    RasterImage { width: ow, height: oh, rgba, geotransform: [0.0, 1.0, 0.0, oh as f64, 0.0, -1.0] }
}

/// The largest part of the grid, cut from its sides, whose edge cells all have a height.
/// The side with the most cells without a height is cut first, one row or column at a time.
pub fn trim_to_rectangle(g: &Grid) -> Grid {
    let (mut c0, mut r0, mut c1, mut r1) = (0usize, 0usize, g.width, g.height);
    let empty_row = |r: usize, c0: usize, c1: usize| (c0..c1).filter(|c| g.get(*c, r).is_none()).count();
    let empty_col = |c: usize, r0: usize, r1: usize| (r0..r1).filter(|r| g.get(c, *r).is_none()).count();
    while c1 - c0 > 2 && r1 - r0 > 2 {
        let sides = [empty_row(r0, c0, c1), empty_row(r1 - 1, c0, c1), empty_col(c0, r0, r1), empty_col(c1 - 1, r0, r1)];
        let worst = (0..4).max_by_key(|i| sides[*i]).unwrap_or(0);
        if sides[worst] == 0 {
            break;
        }
        match worst {
            0 => r0 += 1,
            1 => r1 -= 1,
            2 => c0 += 1,
            _ => c1 -= 1,
        }
    }
    if (c0, r0, c1, r1) == (0, 0, g.width, g.height) {
        return g.clone();
    }
    let (w, h) = (c1 - c0, r1 - r0);
    let mut values = Vec::with_capacity(w * h);
    for r in r0..r1 {
        values.extend_from_slice(&g.values[r * g.width + c0..r * g.width + c1]);
    }
    let t = &g.geotransform;
    let gt = [t[0] + c0 as f64 * t[1] + r0 as f64 * t[2], t[1], t[2], t[3] + c0 as f64 * t[4] + r0 as f64 * t[5], t[4], t[5]];
    Grid { bands: Vec::new(), width: w, height: h, values, geotransform: gt, crs: g.crs }
}

/// The grid with at most `most` cells across or down.
fn fit(g: &Grid, most: usize) -> Grid {
    let k = (g.width.max(g.height) as f64 / most as f64).max(1.0);
    if k <= 1.0 {
        return g.clone();
    }
    resample(g, ((g.width as f64 / k).round() as usize).max(2), ((g.height as f64 / k).round() as usize).max(2))
}

/// The grid with at most `most` cells across, or as it is.
pub fn at_most(g: &Grid, most: usize) -> Grid {
    if g.width <= most {
        return g.clone();
    }
    let k = g.width as f64 / most as f64;
    resample(g, most, ((g.height as f64 / k).round() as usize).max(1))
}

/// A grid of a given width, keeping its shape.
pub fn of_width(g: &Grid, width: usize) -> Grid {
    let k = g.width as f64 / width as f64;
    resample(g, width.max(2), ((g.height as f64 / k).round() as usize).max(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cone 100 high in the middle of a plain, in cells of 10.
    fn cone(n: usize) -> Grid {
        let mut values = Vec::new();
        for r in 0..n {
            for c in 0..n {
                let d = (((c as f64 - n as f64 / 2.0).powi(2) + (r as f64 - n as f64 / 2.0).powi(2)).sqrt()) * 10.0;
                values.push((100.0 - d * 0.5).max(0.0) as f32);
            }
        }
        Grid { bands: Vec::new(), width: n, height: n, values, geotransform: [0.0, 10.0, 0.0, n as f64 * 10.0, 0.0, -10.0], crs: Some(3006) }
    }

    #[test]
    fn the_sun_lights_the_side_toward_it_and_the_other_side_is_in_shadow() {
        let g = cone(101);
        let look = Look { sun_azimuth: 270.0, sun_altitude: 20.0, exaggeration: 1.0, sky_light: false, ..Default::default() };
        let l = light(&g, &look);
        let at = |c: usize, r: usize| l[r * g.width + c];
        // West of the top faces the sun, east of it faces away, far out it is flat.
        assert!(at(35, 50) > 1.1, "{}", at(35, 50));
        assert!(at(65, 50) < 0.4, "{}", at(65, 50));
        assert!((at(5, 50) - 1.0).abs() < 0.05, "{}", at(5, 50));
        // The plain just east of the cone lies in its shadow, which is 27 cells long from
        // the top, and the plain farther east does not.
        assert!(at(74, 50) < 0.7, "{}", at(74, 50));
        assert!(at(95, 50) > 0.95, "{}", at(95, 50));
        let none = light(&g, &Look { shadows: Shadows::None, ..look });
        assert!((none[50 * g.width + 74] - 1.0).abs() < 0.05);
        // The light of the sky is less at the foot of the cone than out on the plain.
        let sky = light(&g, &Look { sky_light: true, shadows: Shadows::None, ..look });
        assert!(sky[50 * g.width + 70] < sky[50 * g.width + 98]);
    }

    #[test]
    fn grids_are_made_smaller_and_larger() {
        let g = cone(101);
        let small = resample(&g, 50, 50);
        assert_eq!((small.width, small.height), (50, 50));
        assert!((small.geotransform[1] - 20.2).abs() < 1e-9);
        let big = of_width(&g, 202);
        assert_eq!(big.width, 202);
        assert!((big.range().unwrap().1 - g.range().unwrap().1).abs() < 1.0);
    }

    #[test]
    fn a_ragged_grid_is_cut_to_a_rectangle() {
        let mut g = cone(20);
        // The corners have no heights, as after a small turn.
        for (c, r) in [(0, 0), (1, 0), (0, 1), (19, 19), (18, 19), (19, 18), (19, 0), (0, 19)] {
            g.values[r * 20 + c] = f32::NAN;
        }
        let t = trim_to_rectangle(&g);
        assert!(t.width * t.height >= 300 && t.width * t.height < 400, "{} × {}", t.width, t.height);
        for c in 0..t.width {
            assert!(t.get(c, 0).is_some() && t.get(c, t.height - 1).is_some());
        }
        for r in 0..t.height {
            assert!(t.get(0, r).is_some() && t.get(t.width - 1, r).is_some());
        }
        assert_eq!(trim_to_rectangle(&cone(10)).width, 10);
    }

    #[test]
    fn a_wider_picture_gets_the_light_of_the_grid() {
        let g = cone(61);
        let look = Look::default();
        let wide = shade_width(&g, &look, None, 244);
        assert_eq!((wide.width, wide.height), (244, 244));
        assert!((wide.geotransform[1] - 2.5).abs() < 1e-9);
        let small = shade_width(&g, &look, None, 30);
        assert_eq!(small.width, 30);
        // The same place is about as light in both.
        let px = |img: &RasterImage, x: usize, y: usize| img.rgba[(y * img.width + x) * 4] as i32;
        assert!((px(&wide, 40, 122) - px(&shade(&g, &look, None), 10, 30)).abs() < 20);
    }

    #[test]
    fn a_view_shows_terrain_against_the_sky() {
        let g = cone(61);
        let view = View { width: 200, height: 120, quality: 1, ..Default::default() };
        let img = render(&g, &Look::default(), None, &view);
        assert_eq!((img.width, img.height), (200, 120));
        let px = |x: usize, y: usize| &img.rgba[(y * img.width + x) * 4..][..4];
        // The top row is sky, the middle is ground.
        assert_eq!(px(5, 0)[3], 255);
        assert!(px(5, 0)[2] > px(5, 0)[0], "the sky is blue");
        assert_ne!(px(100, 70), px(5, 0));
        let clear = render(&g, &Look::default(), None, &View { sky: false, ..view });
        assert_eq!(clear.rgba[3], 0, "without sky the corner is see-through");
    }
}
