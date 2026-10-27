//! Tanaka contours, or illuminated contours: the contour lines of a terrain drawn light
//! where the slope faces the sun and dark where it faces away, thicker where they face it
//! squarely, over bands of colour between the heights. The terrain stands out as in relief.
//! After Kitiro Tanaka's method of 1950.

use crate::model::{Grid, RasterImage};
use tiny_skia::{Color, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Debug, Clone, PartialEq)]
pub struct Tanaka {
    /// The height between contour lines, and a height that one of them goes through.
    pub interval: f64,
    pub base: f64,
    /// Where the light comes from, in degrees clockwise from north.
    pub sun: f64,
    /// The widest line, in pixels, where a slope faces the sun or turns from it squarely.
    pub width: f32,
    /// The colours of the bands: "grey", "terrain", "blue" or "none".
    pub tint: String,
    /// The width of the picture in pixels.
    pub pixels: usize,
}

/// A piece of a contour line, in the cells of the grid: from, to, its height, and how
/// it faces the light, from -1 (away) to 1 (towards).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub a: (f64, f64),
    pub b: (f64, f64),
    pub height: f64,
    pub light: f64,
}

fn value(g: &Grid, c: usize, r: usize) -> f64 {
    g.values[r * g.width + c] as f64
}

/// The grid smoothed by a mean over each cell and its eight neighbours, `times` times, so
/// that the lines are calm over flat land. Cells without a value stay without one.
pub fn smooth(g: &Grid, times: usize) -> Grid {
    let mut out = g.clone();
    for _ in 0..times {
        let src = out.values.clone();
        for r in 0..g.height {
            for c in 0..g.width {
                let i = r * g.width + c;
                if src[i].is_nan() {
                    continue;
                }
                let (mut sum, mut n) = (0.0f64, 0.0f64);
                for dr in -1i64..=1 {
                    for dc in -1i64..=1 {
                        let (rr, cc) = (r as i64 + dr, c as i64 + dc);
                        if rr < 0 || cc < 0 || rr >= g.height as i64 || cc >= g.width as i64 {
                            continue;
                        }
                        let v = src[rr as usize * g.width + cc as usize];
                        if !v.is_nan() {
                            sum += v as f64;
                            n += 1.0;
                        }
                    }
                }
                out.values[i] = (sum / n) as f32;
            }
        }
    }
    out
}

/// The colour of a band, counted from the lowest, out of `bands`.
pub fn band_colour(tint: &str, band: usize, bands: usize) -> Option<[u8; 3]> {
    let t = if bands <= 1 { 0.5 } else { band as f64 / (bands - 1) as f64 };
    let mix = |stops: &[[u8; 3]]| {
        let x = t * (stops.len() - 1) as f64;
        let (i, f) = (x.floor() as usize, x.fract());
        let (a, b) = (stops[i.min(stops.len() - 1)], stops[(i + 1).min(stops.len() - 1)]);
        [0, 1, 2].map(|k| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * f).round() as u8)
    };
    match tint {
        "none" => None,
        "terrain" => Some(mix(&[[0x6f, 0x8f, 0x63], [0xa9, 0xb5, 0x7c], [0xc9, 0xb9, 0x87], [0xb0, 0x93, 0x73], [0xd8, 0xd2, 0xc8]])),
        "blue" => Some(mix(&[[0x3b, 0x5f, 0x87], [0x6f, 0x92, 0xb5], [0xa7, 0xc2, 0xd8]])),
        // A grey in the middle, from a little darker below to a little lighter above, so that
        // both the light and the dark lines show.
        _ => Some(mix(&[[0x7c, 0x7c, 0x7a], [0x9c, 0x9b, 0x97]])),
    }
}

/// The pieces of every contour line, by marching squares over the cells.
pub fn pieces(g: &Grid, t: &Tanaka) -> Vec<Piece> {
    let mut out = Vec::new();
    if g.width < 2 || g.height < 2 || t.interval <= 0.0 {
        return out;
    }
    let sun = t.sun.to_radians();
    // The grid has rows going south; x east, y south in cells.
    let (sx, sy) = (sun.sin(), -sun.cos());
    for r in 0..g.height - 1 {
        for c in 0..g.width - 1 {
            let z = [value(g, c, r), value(g, c + 1, r), value(g, c + 1, r + 1), value(g, c, r + 1)];
            if z.iter().any(|v| v.is_nan()) {
                continue;
            }
            let (lo, hi) = (z.iter().cloned().fold(f64::INFINITY, f64::min), z.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
            let first = ((lo - t.base) / t.interval).ceil() as i64;
            let last = ((hi - t.base) / t.interval).floor() as i64;
            if first > last {
                continue;
            }
            // The way down the slope in this cell, as a vector in cells.
            let gx = ((z[1] + z[2]) - (z[0] + z[3])) / 2.0;
            let gy = ((z[3] + z[2]) - (z[0] + z[1])) / 2.0;
            let len = (gx * gx + gy * gy).sqrt();
            let light = if len > 0.0 { (-gx * sx - gy * sy) / len } else { 0.0 };
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            for k in first..=last {
                let level = t.base + k as f64 * t.interval;
                // Where the level crosses the four edges.
                let mut hits: Vec<(f64, f64)> = Vec::with_capacity(4);
                for e in 0..4 {
                    let (za, zb) = (z[e], z[(e + 1) % 4]);
                    let (pa, pb) = (corners[e], corners[(e + 1) % 4]);
                    if (za < level) != (zb < level) {
                        let f = (level - za) / (zb - za);
                        hits.push((c as f64 + pa.0 + (pb.0 - pa.0) * f, r as f64 + pa.1 + (pb.1 - pa.1) * f));
                    }
                }
                let pairs: &[(usize, usize)] = match hits.len() {
                    2 => &[(0, 1)],
                    // A saddle: the middle decides which edges go together.
                    4 => {
                        let mid = z.iter().sum::<f64>() / 4.0;
                        if (mid < level) == (z[0] < level) {
                            &[(0, 3), (1, 2)]
                        } else {
                            &[(0, 1), (2, 3)]
                        }
                    }
                    _ => &[],
                };
                for (i, j) in pairs {
                    out.push(Piece { a: hits[*i], b: hits[*j], height: level, light });
                }
            }
        }
    }
    out
}

/// Draws the bands and the lit contour lines, as large as asked, with the coordinates of
/// the grid.
pub fn draw(g: &Grid, t: &Tanaka, pieces: &[Piece]) -> RasterImage {
    let w = t.pixels.clamp(16, 12000);
    let k = w as f64 / g.width as f64;
    let h = ((g.height as f64 * k).round() as usize).max(1);
    let mut pm = Pixmap::new(w as u32, h as u32).expect("a picture of this size");
    // The bands: the height under each pixel, between the cell centres.
    let (lo, hi) = g.values.iter().filter(|v| !v.is_nan()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v as f64), b.max(*v as f64)));
    if lo.is_finite() && t.tint != "none" {
        let first = ((lo - t.base) / t.interval).floor() as i64;
        let last = ((hi - t.base) / t.interval).floor() as i64;
        let bands = (last - first + 1).max(1) as usize;
        let data = pm.data_mut();
        for py in 0..h {
            let gy = ((py as f64 + 0.5) / k - 0.5).clamp(0.0, (g.height - 1) as f64);
            for px in 0..w {
                let gx = ((px as f64 + 0.5) / k - 0.5).clamp(0.0, (g.width - 1) as f64);
                let (c, r) = (gx.floor() as usize, gy.floor() as usize);
                let (c1, r1) = ((c + 1).min(g.width - 1), (r + 1).min(g.height - 1));
                let (fx, fy) = (gx - c as f64, gy - r as f64);
                let v = [value(g, c, r), value(g, c1, r), value(g, c, r1), value(g, c1, r1)];
                if v.iter().any(|x| x.is_nan()) {
                    continue;
                }
                let z = v[0] * (1.0 - fx) * (1.0 - fy) + v[1] * fx * (1.0 - fy) + v[2] * (1.0 - fx) * fy + v[3] * fx * fy;
                let band = (((z - t.base) / t.interval).floor() as i64 - first).clamp(0, bands as i64 - 1) as usize;
                if let Some(col) = band_colour(&t.tint, band, bands) {
                    let i = (py * w + px) * 4;
                    data[i..i + 4].copy_from_slice(&[col[0], col[1], col[2], 255]);
                }
            }
        }
    }
    // The lines, the darker ones first, so that light lines lie on top where they meet.
    let mut order: Vec<&Piece> = pieces.iter().collect();
    order.sort_by(|a, b| a.light.total_cmp(&b.light));
    let base_width = t.width.max(0.2) * (k as f32).clamp(1.0, 2.0);
    for p in order {
        let strength = p.light.abs() as f32;
        let mut paint = Paint { anti_alias: true, ..Default::default() };
        // Light lines white, shadowed lines nearly black, both fading where the slope runs
        // across the light.
        let (rgb, alpha) = if p.light >= 0.0 { ([255u8, 255, 255], 0.35 + 0.65 * strength) } else { ([0x14u8, 0x16, 0x1a], 0.35 + 0.65 * strength) };
        paint.set_color(Color::from_rgba8(rgb[0], rgb[1], rgb[2], (alpha * 255.0) as u8));
        let mut pb = PathBuilder::new();
        let at = |q: (f64, f64)| (((q.0 + 0.5) * k) as f32, ((q.1 + 0.5) * k) as f32);
        let (a, b) = (at(p.a), at(p.b));
        pb.move_to(a.0, a.1);
        pb.line_to(b.0, b.1);
        if let Some(path) = pb.finish() {
            let stroke = Stroke { width: base_width * (0.25 + 0.75 * strength), line_cap: LineCap::Round, ..Stroke::default() };
            pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in pm.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let gt = g.geotransform;
    let (sx, sy) = (gt[1] * g.width as f64 / w as f64, gt[5] * g.height as f64 / h as f64);
    RasterImage { width: w, height: h, rgba, geotransform: [gt[0], sx, 0.0, gt[3], 0.0, sy] }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A round hill in the middle of a square grid.
    fn hill(n: usize) -> Grid {
        let mut values = Vec::with_capacity(n * n);
        let c = (n - 1) as f64 / 2.0;
        for r in 0..n {
            for col in 0..n {
                let d = ((r as f64 - c).powi(2) + (col as f64 - c).powi(2)).sqrt();
                values.push((100.0 - d * 4.0).max(0.0) as f32);
            }
        }
        Grid { bands: Vec::new(), width: n, height: n, values, geotransform: [0.0, 10.0, 0.0, 1000.0, 0.0, -10.0], crs: Some(3006) }
    }

    #[test]
    fn the_side_towards_the_sun_is_light_and_the_other_dark() {
        let g = hill(41);
        let t = Tanaka { interval: 20.0, base: 0.0, sun: 315.0, width: 2.0, tint: "grey".into(), pixels: 200 };
        let p = pieces(&g, &t);
        assert!(!p.is_empty());
        // North-west of the top the slope faces the sun; south-east it faces away.
        let nw = p.iter().filter(|x| x.a.0 < 14.0 && x.a.1 < 14.0).map(|x| x.light).sum::<f64>();
        let se = p.iter().filter(|x| x.a.0 > 26.0 && x.a.1 > 26.0).map(|x| x.light).sum::<f64>();
        assert!(nw > 0.0 && se < 0.0, "{} {}", nw, se);
        assert!(p.iter().all(|x| x.height % 20.0 == 0.0));
        let img = draw(&g, &t, &p);
        assert_eq!((img.width, img.height), (200, 200));
        assert_eq!(img.geotransform, [0.0, 2.05, 0.0, 1000.0, 0.0, -2.05]);
        // Light pixels to the north-west of the top, dark ones to the south-east.
        let px = |x: usize, y: usize| &img.rgba[(y * 200 + x) * 4..][..3];
        let bright = (40..80).flat_map(|y| (40..80).map(move |x| (x, y))).filter(|(x, y)| px(*x, *y)[0] > 230).count();
        let dark = (120..160).flat_map(|y| (120..160).map(move |x| (x, y))).filter(|(x, y)| px(*x, *y)[0] < 50).count();
        assert!(bright > 20 && dark > 20, "{} {}", bright, dark);
    }
}
