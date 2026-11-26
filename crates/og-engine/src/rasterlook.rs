//! How a raster is drawn on a map: a picture as it is, and a grid of values by a colour
//! ramp, a colour for each value (classes), or as grey shaded relief, with how see-through.

use crate::model::{Grid, RasterImage, Result, Table};
use crate::thematic::{Key, Look};

/// How a raster layer is drawn, as the user sets it. Empty mode chooses by the values.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterSpec {
    /// "ramp", "classes", "shade", or empty for a choice by the values.
    pub mode: String,
    /// The ramp of a ramp, from thematic::RAMPS. Empty gives one by the values.
    pub ramp: String,
    /// The values at the ends of the ramp. None takes the smallest and largest.
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Colours of values for classes: the value as text, #rrggbb or "none" for not drawn,
    /// and what it stands for in the legend, empty for the value itself.
    pub colors: Vec<(String, String, String)>,
    /// 0 is see-through, 1 covers what is under it.
    pub opacity: f32,
}

impl Default for RasterSpec {
    fn default() -> Self {
        RasterSpec { mode: String::new(), ramp: String::new(), min: None, max: None, colors: Vec::new(), opacity: 1.0 }
    }
}

/// What the grids of a stream hold, to choose and set how it is drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RasterInfo {
    /// A picture of colours, not values.
    pub image: bool,
    /// The smallest and largest value.
    pub range: Option<(f64, f64)>,
    /// The values, when there are at most CLASSES_AT_MOST and all are whole numbers.
    pub classes: Option<Vec<f64>>,
}

/// A grid with at most this many different whole values is drawn as classes.
pub const CLASSES_AT_MOST: usize = 16;

/// What a stream that carries a raster holds; None for a stream of features only.
pub fn info(t: &Table) -> Option<RasterInfo> {
    match (&t.grid, &t.image) {
        (Some(g), _) => Some(grid_info(g)),
        (None, Some(_)) => Some(RasterInfo { image: true, ..Default::default() }),
        _ => None,
    }
}

pub fn grid_info(g: &Grid) -> RasterInfo {
    let range = g.range().map(|(a, b)| (a as f64, b as f64));
    let mut classes: Vec<f64> = Vec::new();
    let mut whole = true;
    // A sample is enough to tell; then all are counted when it looks like classes.
    let step = (g.values.len() / 200_000).max(1);
    for v in g.values.iter().step_by(step).filter(|v| !v.is_nan()) {
        if v.fract() != 0.0 {
            whole = false;
            break;
        }
        let v = *v as f64;
        if !classes.contains(&v) {
            classes.push(v);
            if classes.len() > CLASSES_AT_MOST {
                break;
            }
        }
    }
    if whole && classes.len() <= CLASSES_AT_MOST && step > 1 {
        for v in g.values.iter().filter(|v| !v.is_nan()) {
            let v = *v as f64;
            if v.fract() != 0.0 {
                whole = false;
                break;
            }
            if !classes.contains(&v) {
                classes.push(v);
                if classes.len() > CLASSES_AT_MOST {
                    break;
                }
            }
        }
    }
    classes.sort_by(f64::total_cmp);
    let classes = (whole && !classes.is_empty() && classes.len() <= CLASSES_AT_MOST).then_some(classes);
    RasterInfo { image: false, range, classes }
}

/// The text of a value of a class, as it is kept in the colours of a spec.
pub fn value_text(v: f64) -> String {
    crate::model::fmt_real(v).replace('\u{a0}', "")
}

/// The colour a class gets until the user chooses one.
pub fn class_colour(k: usize) -> [u8; 4] {
    let c = crate::thematic::CATEGORIES[k % crate::thematic::CATEGORIES.len()];
    [c[0], c[1], c[2], 255]
}

/// How the cells of a grid are coloured, made ready.
#[derive(Debug, Clone)]
pub enum Paint {
    /// A picture drawn as it is.
    Image,
    Ramp {
        ramp: String,
        lo: f64,
        hi: f64,
    },
    /// A colour for each value; None for a value that is not drawn.
    Classes(Vec<(f64, Option<[u8; 4]>)>),
    /// Grey shaded relief, as heights.
    Shade,
    /// Grey relief lit from several sides at once, softer and clearer, as on Swiss maps.
    Soft,
    /// Heights in colours, from green lowland to brown and white heights, lit from several
    /// sides: hypsometric tints with relief.
    Relief {
        lo: f64,
        hi: f64,
    },
    /// Swiss relief: soft light from several sides, with air between the eye and the
    /// lowland, so that it is pale and hazy and the heights stand out clear, with blue-grey
    /// shadows and warm light, as Eduard Imhof drew it.
    Swiss {
        lo: f64,
        hi: f64,
    },
    /// Hachures: short strokes down the slopes, thicker and closer where it is steeper, as
    /// relief was drawn before contours.
    Hachures,
    /// Texture shading (Leland Brown): each cell lighter the higher it is than the land
    /// around it, at several scales at once, so that ridges and valleys stand out as a web.
    Texture,
    /// Land and sea: heights in colours with relief above 0, and below it the depths in
    /// blues, darker the deeper, down to `deep`.
    Depths {
        deep: f64,
        hi: f64,
    },
    /// The floor of the sea: the depths in blues from pale turquoise at the shore to deep
    /// navy, lit so that its ridges and canyons show, and the land as paper, a single colour
    /// with soft relief.
    Seafloor {
        deep: f64,
        /// The land in this colour; None leaves it see-through, for what is under it.
        land: Option<[u8; 3]>,
        /// The sea in one colour, a little darker the deeper, rather than in blues.
        sea: Option<[u8; 3]>,
    },
    /// Steps: each cell the colour of the step it is on, a step being from its value up to
    /// the next, as depths on an atlas map. With `shadow`, each step casts a soft shadow on
    /// those under it, as layers of paper.
    Bands {
        steps: Vec<(f64, Option<[u8; 4]>)>,
        shadow: bool,
    },
}

/// The steps of depth of an atlas map: the land, then from the shore down to 200 m, to
/// 1000 m and on by 1000 m, from pale lilac through blues to black, and their names.
pub const DEPTH_STEPS: &[(f64, [u8; 3], &str)] = &[
    (0.0, [0xf6, 0xf5, 0xf8], "Land"),
    (-200.0, [0xec, 0xeb, 0xf3], "< 200 m"),
    (-1000.0, [0xd5, 0xd4, 0xe8], "200 m"),
    (-2000.0, [0xa9, 0xbd, 0xdc], "1 000 m"),
    (-3000.0, [0x6e, 0xa6, 0xcf], "2 000 m"),
    (-4000.0, [0x2e, 0x8e, 0xc2], "3 000 m"),
    (-5000.0, [0x0e, 0x5e, 0x93], "4 000 m"),
    (f64::NEG_INFINITY, [0x10, 0x12, 0x18], "5 000 m"),
];

/// A colour ramp for a legend: its colours from one end to the other, the values written
/// under it at their places from 0 to 1, and the unit after the last.
#[derive(Debug, Clone, Default)]
pub struct RampLegend {
    pub colours: Vec<[u8; 4]>,
    pub ticks: Vec<(f32, String)>,
}

/// The legend of a raster drawn with a ramp of colours, as a band with values under it:
/// depths from 0 down for the sea, heights for land, values for a ramp.
pub fn ramp_legend(paint: &Paint) -> Option<RampLegend> {
    type Colour = Box<dyn Fn(f64) -> [u8; 4]>;
    let (lo, hi, unit, colour): (f64, f64, &str, Colour) = match paint {
        Paint::Depths { deep, .. } if *deep < 0.0 => (0.0, -deep, " m", Box::new(move |t| bathymetric(t as f32))),
        Paint::Seafloor { deep, sea: None, .. } if *deep < 0.0 => (0.0, -deep, " m", Box::new(move |t| abyss(t as f32))),
        Paint::Seafloor { deep, sea: Some(c), .. } if *deep < 0.0 => {
            let c = *c;
            (0.0, -deep, " m", Box::new(move |t| one_sea(c, t)))
        }
        Paint::Relief { lo, hi } | Paint::Swiss { lo, hi } if hi > lo => (*lo, *hi, " m", Box::new(hypsometric)),
        Paint::Ramp { ramp, lo, hi } if hi > lo => {
            let ramp = ramp.clone();
            (
                *lo,
                *hi,
                "",
                Box::new(move |t| {
                    let c = crate::thematic::ramp_at(&ramp, t);
                    [c[0], c[1], c[2], 255]
                }),
            )
        }
        _ => return None,
    };
    let colours = (0..64).map(|k| colour(k as f64 / 63.0)).collect();
    let step = crate::chart::nice_step(hi - lo, 4.0);
    let mut ticks = Vec::new();
    let mut v = (lo / step).ceil() * step;
    while v <= hi + step * 1e-6 {
        let text = if v.fract().abs() < 1e-9 && v.abs() < 1e15 {
            let n = crate::model::fmt_count(v.abs().round() as u64);
            if v < 0.0 {
                format!("−{}", n)
            } else {
                n
            }
        } else {
            crate::model::fmt_real((v * 100.0).round() / 100.0)
        };
        ticks.push((((v - lo) / (hi - lo)) as f32, text));
        v += step;
    }
    if let Some(last) = ticks.last_mut() {
        last.1.push_str(unit);
    }
    Some(RampLegend { colours, ticks })
}

/// The sea in one colour at a depth, from 0 at the shore to 1 at the deepest: a little
/// darker the deeper.
pub fn one_sea(c: [u8; 3], t: f64) -> [u8; 4] {
    let k = 1.0 - 0.32 * t.clamp(0.0, 1.0).powf(0.6);
    [(c[0] as f64 * k) as u8, (c[1] as f64 * k) as u8, (c[2] as f64 * k) as u8, 255]
}

/// The colour of the sea floor at a depth, from 0 at the shore to 1 at the deepest: pale
/// turquoise, teal and blue to a deep navy.
pub fn abyss(t: f32) -> [u8; 4] {
    let stops = [[0xa9, 0xde, 0xdc], [0x6c, 0xbf, 0xca], [0x3a, 0x8f, 0xb0], [0x23, 0x60, 0x8f], [0x18, 0x3c, 0x66], [0x10, 0x25, 0x45]];
    let x = t.clamp(0.0, 1.0).powf(0.7) * (stops.len() - 1) as f32;
    let (i, f) = (x.floor() as usize, x.fract());
    let n = stops.len() - 1;
    let (a, b) = (stops[i.min(n)], stops[(i + 1).min(n)]);
    [0, 1, 2, 3].map(|k| if k == 3 { 255 } else { (a[k] as f32 + (b[k] as f32 - a[k] as f32) * f).round() as u8 })
}

/// A raster layer made ready to draw.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub paint: Paint,
    pub opacity: f32,
    /// What the colours stand for, for the legend.
    pub keys: Vec<Key>,
}

/// Makes a raster layer ready to draw; None for a stream without a raster.
pub fn prepare(t: &Table, spec: &RasterSpec) -> Result<Option<Prepared>> {
    let opacity = spec.opacity.clamp(0.0, 1.0);
    let Some(i) = info(t) else { return Ok(None) };
    if i.image && t.grid.is_none() {
        return Ok(Some(Prepared { paint: Paint::Image, opacity, keys: Vec::new() }));
    }
    let mode = match spec.mode.as_str() {
        "" if i.classes.is_some() => "classes",
        "" => "ramp",
        "classes" if i.classes.is_none() && spec.colors.is_empty() => {
            return Err(crate::model::GbError(format!(
                "The raster has more than {} different values, or values with decimals, so it cannot be drawn as classes. Draw it with a colour ramp, or make classes of it first with Reclassify raster.",
                CLASSES_AT_MOST
            )))
        }
        m => m,
    };
    let swatch = |c: [u8; 4]| Look { fill: Some(c), stroke: None, size: 10.0 };
    Ok(Some(match mode {
        "shade" => Prepared { paint: Paint::Shade, opacity, keys: Vec::new() },
        "soft" => Prepared { paint: Paint::Soft, opacity, keys: Vec::new() },
        "hachures" => Prepared { paint: Paint::Hachures, opacity, keys: Vec::new() },
        "texture" => Prepared { paint: Paint::Texture, opacity, keys: Vec::new() },
        "depths" => {
            let (lo0, hi0) = i.range.unwrap_or((-50.0, 100.0));
            Prepared { paint: Paint::Depths { deep: spec.min.unwrap_or(lo0.min(-1.0)), hi: spec.max.unwrap_or(hi0.max(1.0)) }, opacity, keys: Vec::new() }
        }
        "seafloor" | "sea" | "landsea" => {
            let (lo0, _) = i.range.unwrap_or((-50.0, 100.0));
            // Colours of one's own as #rrggbb in the ramp: the land, and for land and sea
            // the sea after it.
            let own: Vec<[u8; 3]> =
                spec.ramp.split([' ', ',', ';']).filter_map(|t| crate::mapdraw::parse_color(t.trim()).ok().flatten()).map(|c| [c[0], c[1], c[2]]).collect();
            let (land, sea) = match mode {
                "sea" => (None, None),
                "landsea" => (Some(own.first().copied().unwrap_or([0xec, 0xdc, 0xc6])), Some(own.get(1).copied().unwrap_or([0x8e, 0xd2, 0xd2]))),
                _ => (Some(own.first().copied().unwrap_or([0xee, 0xe7, 0xd7])), None),
            };
            Prepared { paint: Paint::Seafloor { deep: spec.min.unwrap_or(lo0.min(-1.0)), land, sea }, opacity, keys: Vec::new() }
        }
        "bands" | "paper" => {
            // Steps of one's own: from a value up to the next, with a colour and a name. Else
            // the steps of depth of an atlas for a grid that goes below 0, or six even steps.
            let mut own: Vec<(f64, Option<[u8; 4]>, String)> = Vec::new();
            for (v, c, name) in &spec.colors {
                if let Some(v) = crate::model::parse_number(v) {
                    let c = if c.trim().is_empty() { None } else { crate::mapdraw::parse_color(c).map_err(crate::model::GbError)? };
                    own.push((v, c, name.trim().to_string()));
                }
            }
            let (lo0, hi0) = i.range.unwrap_or((0.0, 1.0));
            if own.is_empty() {
                if lo0 < 0.0 {
                    own = DEPTH_STEPS.iter().map(|(v, c, n)| (*v, Some([c[0], c[1], c[2], 255]), n.to_string())).collect();
                } else {
                    let step = crate::chart::nice_step(hi0 - lo0, 6.0);
                    let mut v = (lo0 / step).floor() * step;
                    let ramp = if crate::thematic::RAMPS.iter().any(|r| r.0 == spec.ramp) { spec.ramp.as_str() } else { "terrain" };
                    let n = ((hi0 - v) / step).ceil().max(1.0);
                    let mut k = 0.0;
                    while v < hi0 {
                        let c = crate::thematic::ramp_at(ramp, k / (n - 1.0).max(1.0));
                        own.push((v, Some([c[0], c[1], c[2], 255]), value_text(v)));
                        v += step;
                        k += 1.0;
                    }
                }
            }
            own.sort_by(|a, b| b.0.total_cmp(&a.0));
            // The legend from the deepest, as atlases list the depths.
            let keys = own
                .iter()
                .rev()
                .filter(|(v, c, _)| c.is_some() && !(*v == 0.0 && lo0 < 0.0 && spec.colors.is_empty()))
                .map(|(v, c, n)| Key { look: swatch(c.unwrap()), text: if n.is_empty() { value_text(*v) } else { n.clone() } })
                .collect();
            Prepared { paint: Paint::Bands { steps: own.into_iter().map(|(v, c, _)| (v, c)).collect(), shadow: mode == "paper" }, opacity, keys }
        }
        "swiss" => {
            let (lo0, hi0) = i.range.unwrap_or((0.0, 1.0));
            Prepared { paint: Paint::Swiss { lo: spec.min.unwrap_or(lo0), hi: spec.max.unwrap_or(hi0) }, opacity, keys: Vec::new() }
        }
        "relief" => {
            let (lo0, hi0) = i.range.unwrap_or((0.0, 1.0));
            let (lo, hi) = (spec.min.unwrap_or(lo0), spec.max.unwrap_or(hi0));
            let keys = (0..5)
                .map(|k| {
                    let f = k as f64 / 4.0;
                    Key { look: swatch(hypsometric(f)), text: short(lo + (hi - lo) * f) }
                })
                .rev()
                .collect();
            Prepared { paint: Paint::Relief { lo, hi }, opacity, keys }
        }
        "classes" => {
            let mut values: Vec<f64> = i.classes.clone().unwrap_or_default();
            for (v, _, _) in &spec.colors {
                if let Some(v) = crate::model::parse_number(v) {
                    if !values.contains(&v) {
                        values.push(v);
                    }
                }
            }
            values.sort_by(f64::total_cmp);
            let mut classes = Vec::new();
            let mut keys = Vec::new();
            for (k, v) in values.iter().enumerate() {
                let own = spec.colors.iter().find(|(t, _, _)| crate::model::parse_number(t) == Some(*v));
                let colour = match own {
                    Some((_, c, _)) if !c.trim().is_empty() => crate::mapdraw::parse_color(c).map_err(crate::model::GbError)?,
                    _ => Some(class_colour(k)),
                };
                let text = own.map(|o| o.2.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| value_text(*v));
                if let Some(c) = colour {
                    keys.push(Key { look: swatch(c), text });
                }
                classes.push((*v, colour));
            }
            Prepared { paint: Paint::Classes(classes), opacity, keys }
        }
        _ => {
            let (lo0, hi0) = i.range.unwrap_or((0.0, 1.0));
            let (lo, hi) = (spec.min.unwrap_or(lo0), spec.max.unwrap_or(hi0));
            let ramp = if crate::thematic::RAMPS.iter().any(|r| r.0 == spec.ramp) { spec.ramp.clone() } else { "viridis".to_string() };
            let keys = (0..5)
                .map(|k| {
                    let f = k as f64 / 4.0;
                    let c = crate::thematic::ramp_at(&ramp, f);
                    Key { look: swatch([c[0], c[1], c[2], 255]), text: short(lo + (hi - lo) * f) }
                })
                .rev()
                .collect();
            Prepared { paint: Paint::Ramp { ramp, lo, hi }, opacity, keys }
        }
    }))
}

fn short(v: f64) -> String {
    let d = if v.abs() >= 100.0 {
        0
    } else if v.abs() >= 1.0 {
        1
    } else {
        3
    };
    let k = 10f64.powi(d);
    value_text((v * k).round() / k)
}

/// The colour of a value; None leaves the cell empty.
pub fn colour_of(paint: &Paint, v: f32) -> Option<[u8; 4]> {
    if v.is_nan() {
        return None;
    }
    match paint {
        Paint::Ramp { ramp, lo, hi } => {
            let t = if hi > lo { (v as f64 - lo) / (hi - lo) } else { 0.5 };
            let c = crate::thematic::ramp_at(ramp, t);
            Some([c[0], c[1], c[2], 255])
        }
        Paint::Classes(classes) => classes.iter().find(|(c, _)| *c == v as f64).and_then(|(_, c)| *c),
        Paint::Shade | Paint::Image | Paint::Soft | Paint::Swiss { .. } | Paint::Hachures | Paint::Texture => Some([128, 128, 128, 255]),
        Paint::Depths { deep, hi } => {
            Some(if (v as f64) < 0.0 { bathymetric((v as f64 / deep).clamp(0.0, 1.0) as f32) } else { hypsometric((v as f64 / hi).clamp(0.0, 1.0)) })
        }
        Paint::Relief { lo, hi } => Some(hypsometric(if hi > lo { (v as f64 - lo) / (hi - lo) } else { 0.5 })),
        Paint::Seafloor { deep, land, sea } => {
            let t = (v as f64 / deep).clamp(0.0, 1.0);
            match ((v as f64) < 0.0, sea) {
                (true, Some(c)) => Some(one_sea(*c, t)),
                (true, None) => Some(abyss(t as f32)),
                (false, _) => land.map(|l| [l[0], l[1], l[2], 255]),
            }
        }
        Paint::Bands { steps, .. } => step_of(steps, v as f64).and_then(|k| steps[k].1),
    }
}

/// The step a value is on: the one with the highest value at or under it, or the lowest
/// step for a value under them all. The steps are from the highest down.
fn step_of(steps: &[(f64, Option<[u8; 4]>)], v: f64) -> Option<usize> {
    if steps.is_empty() || v.is_nan() {
        return None;
    }
    Some(steps.iter().position(|s| v >= s.0).unwrap_or(steps.len() - 1))
}

/// Steps as a picture, each step casting a soft shadow to the south-east on the steps under
/// it when `shadow`, as layers of paper lit from the north-west. The shadow is `reach`
/// cells long.
fn bands_picture(g: &Grid, steps: &[(f64, Option<[u8; 4]>)], shadow: bool, reach: f64) -> RasterImage {
    let (w, h) = (g.width, g.height);
    let level: Vec<f32> = g.values.iter().map(|v| step_of(steps, *v as f64).map_or(f32::NAN, |k| (steps.len() - k) as f32)).collect();
    let mut dark = vec![0f32; w * h];
    if shadow && w > 2 && h > 2 {
        let d = reach.max(1.0);
        let off = (d * 0.7).round() as isize;
        let base: Vec<f32> = level.iter().map(|v| if v.is_nan() { 0.0 } else { *v }).collect();
        // The steps moved away from the light, then softened: where a higher step lies to
        // the north-west, the moved surface is above the cell, which is then in its shadow.
        let mut moved = vec![0f32; w * h];
        for y in 0..h as isize {
            for x in 0..w as isize {
                let (sx, sy) = ((x - off).clamp(0, w as isize - 1), (y - off).clamp(0, h as isize - 1));
                moved[(y as usize) * w + x as usize] = base[sy as usize * w + sx as usize];
            }
        }
        let r = (d * 0.8).round().max(1.0) as usize;
        let soft = box_blur3(&moved, w, h, r);
        for i in 0..w * h {
            // A little is let go, so that the sums of the blur do not leave faint marks.
            dark[i] = (soft[i] - base[i] - 0.004).clamp(0.0, 1.0);
        }
    }
    let mut rgba = vec![0u8; w * h * 4];
    for i in 0..w * h {
        let Some(k) = step_of(steps, g.values[i] as f64) else { continue };
        let Some(c) = steps[k].1 else { continue };
        let f = 1.0 - 0.38 * dark[i];
        rgba[i * 4..i * 4 + 4].copy_from_slice(&[(c[0] as f32 * f) as u8, (c[1] as f32 * f) as u8, (c[2] as f32 * f) as u8, c[3]]);
    }
    RasterImage { vector: None, width: w, height: h, rgba, geotransform: g.geotransform }
}

/// A blur near a Gaussian: a running mean `r` cells to each side, across and down, three
/// times.
fn box_blur3(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut a = src.to_vec();
    for _ in 0..3 {
        let mut b = vec![0f32; w * h];
        for y in 0..h {
            let row = &a[y * w..(y + 1) * w];
            let mut sum: f32 = row[..r.min(w)].iter().sum::<f32>() + row[0] * (r + 1) as f32;
            for x in 0..w {
                sum += row[(x + r).min(w - 1)] - row[x.saturating_sub(r + 1).min(w - 1)];
                b[y * w + x] = sum / (2 * r + 1) as f32;
            }
        }
        let mut c = vec![0f32; w * h];
        for x in 0..w {
            let mut sum: f32 = (0..r.min(h)).map(|y| b[y * w + x]).sum::<f32>() + b[x] * (r + 1) as f32;
            for y in 0..h {
                sum += b[(y + r).min(h - 1) * w + x] - b[y.saturating_sub(r + 1).min(h - 1) * w + x];
                c[y * w + x] = sum / (2 * r + 1) as f32;
            }
        }
        a = c;
    }
    a
}

/// The colour of a height from 0, the lowest, to 1, the highest: green lowland, yellow and
/// brown hills, grey rock and white tops.
pub fn hypsometric(t: f64) -> [u8; 4] {
    const STOPS: [(f64, [f64; 3]); 6] = [
        (0.0, [112.0, 160.0, 102.0]),
        (0.25, [176.0, 196.0, 128.0]),
        (0.5, [226.0, 212.0, 158.0]),
        (0.7, [196.0, 160.0, 118.0]),
        (0.88, [178.0, 170.0, 164.0]),
        (1.0, [250.0, 250.0, 250.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    let k = STOPS.iter().position(|s| s.0 >= t).unwrap_or(STOPS.len() - 1).max(1);
    let (a, b) = (STOPS[k - 1], STOPS[k]);
    let f = ((t - a.0) / (b.0 - a.0).max(1e-9)).clamp(0.0, 1.0);
    let c = |i: usize| (a.1[i] + (b.1[i] - a.1[i]) * f).round() as u8;
    [c(0), c(1), c(2), 255]
}

/// How light each cell is when lit from several sides at once, from the northwest most:
/// 0 dark to 1 light. The light of a slope towards it, the steepness from the heights.
pub fn soft_light(g: &Grid) -> Vec<f32> {
    soft_light_lifted(g, 1.6)
}

/// As soft_light, with the heights lifted `z` times: more for a large area, where the
/// slopes over cells of kilometres are too gentle to see.
pub fn soft_light_lifted(g: &Grid, z: f64) -> Vec<f32> {
    let (w, h) = (g.width, g.height);
    let (sx, sy) = (g.geotransform[1].abs().max(1e-9), g.geotransform[5].abs().max(1e-9));
    let at = |x: usize, y: usize| -> f64 {
        let v = g.values[y.min(h - 1) * w + x.min(w - 1)];
        if v.is_nan() {
            0.0
        } else {
            v as f64
        }
    };
    // The light from these sides, as in the multidirectional shading of Mark (1992).
    let suns: [(f64, f64); 4] = [(225.0, 0.25), (270.0, 0.5), (315.0, 1.0), (360.0, 0.5)];
    let alt = 45f64.to_radians();
    let total: f64 = suns.iter().map(|s| s.1).sum();
    let mut out = vec![f32::NAN; w * h];
    for y in 0..h {
        for x in 0..w {
            if g.values[y * w + x].is_nan() {
                continue;
            }
            let (xl, xr) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let (yu, yd) = (y.saturating_sub(1), (y + 1).min(h - 1));
            let dzdx = (at(xr, y) - at(xl, y)) / (sx * (xr - xl).max(1) as f64) * z;
            let dzdy = (at(x, yd) - at(x, yu)) / (sy * (yd - yu).max(1) as f64) * z;
            let slope = dzdx.hypot(dzdy).atan();
            let aspect = dzdy.atan2(-dzdx);
            let mut light = 0.0;
            for (az, weight) in suns {
                let a = (360.0 - az + 90.0).to_radians();
                let l = alt.sin() * slope.cos() + alt.cos() * slope.sin() * (a - aspect).cos();
                light += l.max(0.0) * weight;
            }
            out[y * w + x] = (light / total) as f32;
        }
    }
    out
}

/// A grid as a picture with the same cells, coloured as asked. Shade gives grey relief.
pub fn picture(g: &Grid, paint: &Paint) -> RasterImage {
    if let Paint::Swiss { lo, hi } = paint {
        return swiss(g, *lo, *hi);
    }
    if let Paint::Texture = paint {
        return texture(g);
    }
    if let Paint::Bands { steps, shadow } = paint {
        return bands_picture(g, steps, *shadow, (g.width.max(g.height) as f64 / 220.0).max(1.5));
    }
    if let Paint::Seafloor { sea, .. } = paint {
        // The floor lit more strongly than the land, so that its ridges, canyons and
        // seamounts stand out; the land quiet, as paper with a soft relief. Over cells of
        // kilometres the heights are lifted more, or the slopes would not show.
        let cell = g.geotransform[1].abs() * if g.geotransform[1].abs() < 0.5 { 111_000.0 } else { 1.0 };
        let light = soft_light_lifted(g, (cell / 60.0).clamp(1.6, 40.0));
        let mut rgba = vec![0u8; g.width * g.height * 4];
        for (i, (v, l)) in g.values.iter().zip(&light).enumerate() {
            if v.is_nan() || l.is_nan() {
                continue;
            }
            let Some(c) = colour_of(paint, *v) else { continue };
            // Land and sea in a colour each have the relief strong on both, as a model.
            let k = match (*v < 0.0, sea.is_some()) {
                (true, false) => (0.62 + 0.55 * l).min(1.15),
                (true, true) => (0.66 + 0.48 * l).min(1.12),
                (false, true) => (0.5 + 0.62 * l).min(1.1),
                (false, false) => (0.84 + 0.24 * l).min(1.05),
            };
            let px = |c: u8| ((c as f32) * k).clamp(0.0, 255.0) as u8;
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[px(c[0]), px(c[1]), px(c[2]), 255]);
        }
        return RasterImage { vector: None, width: g.width, height: g.height, rgba, geotransform: g.geotransform };
    }
    if let Paint::Soft | Paint::Relief { .. } | Paint::Depths { .. } = paint {
        // Over cells of kilometres the heights are lifted, as for the sea floor, or the
        // slopes would not show.
        let cell = g.geotransform[1].abs() * if g.geotransform[1].abs() < 0.5 { 111_000.0 } else { 1.0 };
        let light = soft_light_lifted(g, (cell / 100.0).clamp(1.6, 25.0));
        let mut rgba = vec![0u8; g.width * g.height * 4];
        for (i, (v, l)) in g.values.iter().zip(&light).enumerate() {
            if v.is_nan() || l.is_nan() {
                continue;
            }
            let c = match paint {
                Paint::Relief { .. } | Paint::Depths { .. } => colour_of(paint, *v).unwrap_or([128, 128, 128, 255]),
                _ => [236, 234, 230, 255],
            };
            // The sea is not lit as land is, only a little by the shape of its floor.
            if matches!(paint, Paint::Depths { .. }) && *v < 0.0 {
                let k = (0.74 + 0.36 * l).min(1.08);
                let px = |c: u8| ((c as f32) * k).clamp(0.0, 255.0) as u8;
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[px(c[0]), px(c[1]), px(c[2]), 255]);
                continue;
            }
            // Shade darkens, and the lit side is a little lighter than the colour.
            let k = (0.38 + 0.78 * l).min(1.12);
            let px = |c: u8| ((c as f32) * k).clamp(0.0, 255.0) as u8;
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[px(c[0]), px(c[1]), px(c[2]), 255]);
        }
        return RasterImage { vector: None, width: g.width, height: g.height, rgba, geotransform: g.geotransform };
    }
    if matches!(paint, Paint::Shade | Paint::Image) {
        use crate::terrain::{Colours, Look, Shadows};
        let look = Look { colours: Colours::White, shadows: Shadows::None, sky_light: false, exaggeration: 2.0, ..Default::default() };
        return crate::terrain::shade(g, &look, None);
    }
    let mut rgba = vec![0u8; g.width * g.height * 4];
    for (i, v) in g.values.iter().enumerate() {
        if let Some(c) = colour_of(paint, *v) {
            rgba[i * 4..i * 4 + 4].copy_from_slice(&c);
        }
    }
    RasterImage { vector: None, width: g.width, height: g.height, rgba, geotransform: g.geotransform }
}

/// The colour of the sea at a depth, from 0 at the shore to 1 at the deepest: pale blue to
/// deep blue.
pub fn bathymetric(t: f32) -> [u8; 4] {
    let stops = [[0xd8, 0xec, 0xf2], [0xa9, 0xd2, 0xe6], [0x6f, 0xa8, 0xd2], [0x3a, 0x74, 0xb0], [0x1d, 0x45, 0x7c]];
    let x = t.clamp(0.0, 1.0).sqrt() * (stops.len() - 1) as f32;
    let (i, f) = (x.floor() as usize, x.fract());
    let (a, b) = (stops[i.min(4)], stops[(i + 1).min(4)]);
    [0, 1, 2, 3].map(|k| if k == 3 { 255 } else { (a[k] as f32 + (b[k] as f32 - a[k] as f32) * f).round() as u8 })
}

/// Texture shading, after Leland Brown: how much higher each cell is than the land around
/// it, at three scales together, so that the ridges are light and the valleys dark at every
/// size. The values are stretched so that the middle 96 in a hundred span grey.
fn texture(g: &Grid) -> RasterImage {
    let (w, h) = (g.width, g.height);
    let base: Vec<f32> = g.values.iter().map(|v| if v.is_nan() { 0.0 } else { *v }).collect();
    let blur = |src: &[f32], r: usize| -> Vec<f32> {
        // A box blur across, then down, three times: near a Gaussian.
        let mut a = src.to_vec();
        for _ in 0..3 {
            let mut b = vec![0f32; w * h];
            for y in 0..h {
                let row = &a[y * w..(y + 1) * w];
                let mut sum: f32 = row[..r.min(w)].iter().sum::<f32>() + row[0] * (r + 1) as f32;
                for x in 0..w {
                    sum += row[(x + r).min(w - 1)] - row[x.saturating_sub(r + 1).min(w - 1)];
                    b[y * w + x] = sum / (2 * r + 1) as f32;
                }
            }
            let mut c = vec![0f32; w * h];
            for x in 0..w {
                let mut sum: f32 = (0..r.min(h)).map(|y| b[y * w + x]).sum::<f32>() + b[x] * (r + 1) as f32;
                for y in 0..h {
                    sum += b[(y + r).min(h - 1) * w + x] - b[y.saturating_sub(r + 1).min(h - 1) * w + x];
                    c[y * w + x] = sum / (2 * r + 1) as f32;
                }
            }
            a = c;
        }
        a
    };
    // Bands of detail, each the difference of two blurs, each brought to the same strength,
    // so that the large ridges and valleys are seen as well as the small, and the finest,
    // often only noise, counts the least.
    let cap = (w.max(h) / 4).max(1);
    let radii: Vec<usize> = [1usize, 3, 9, 27].iter().map(|r| (*r).min(cap)).collect();
    let blurs: Vec<Vec<f32>> = radii.iter().map(|r| blur(&base, *r)).collect();
    let weights = [0.2f32, 0.35, 0.45];
    let inside: Vec<usize> = (0..w * h).filter(|i| !g.values[*i].is_nan()).collect();
    let mut t = vec![0f32; w * h];
    for (k, weight) in weights.iter().enumerate() {
        let band: Vec<f32> = (0..w * h).map(|i| blurs[k][i] - blurs[k + 1][i]).collect();
        let rms = (inside.iter().map(|i| (band[*i] as f64).powi(2)).sum::<f64>() / inside.len().max(1) as f64).sqrt().max(1e-9) as f32;
        for i in 0..w * h {
            // Softly limited, so that a cliff does not outshine the rest.
            t[i] += weight * (band[i] / rms / 1.5).tanh();
        }
    }
    let mut sorted: Vec<f32> = inside.iter().map(|i| t[*i]).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let (lo, hi) = if sorted.is_empty() { (-1.0, 1.0) } else { (sorted[sorted.len() / 100], sorted[(sorted.len() * 99 / 100).min(sorted.len() - 1)]) };
    let span = (hi - lo).max(1e-6);
    let mut rgba = vec![0u8; w * h * 4];
    for i in 0..w * h {
        if g.values[i].is_nan() {
            continue;
        }
        let k = ((t[i] - lo) / span).clamp(0.0, 1.0);
        let v = (40.0 + 205.0 * k) as u8;
        rgba[i * 4..i * 4 + 4].copy_from_slice(&[v, v, (v as f32 * 0.98 + 4.0) as u8, 255]);
    }
    RasterImage { vector: None, width: w, height: h, rgba, geotransform: g.geotransform }
}

/// Swiss relief: the shading softer in the lowland and stronger towards the tops, the
/// lowland pale and a little blue as seen through air, the light warm and the shadows
/// blue-grey.
fn swiss(g: &Grid, lo: f64, hi: f64) -> RasterImage {
    let light = soft_light(g);
    let mut rgba = vec![0u8; g.width * g.height * 4];
    let span = if hi > lo { hi - lo } else { 1.0 };
    for (i, (v, l)) in g.values.iter().zip(&light).enumerate() {
        if v.is_nan() || l.is_nan() {
            continue;
        }
        let t = ((*v as f64 - lo) / span).clamp(0.0, 1.0) as f32;
        // Flat land is lit about 0.7: the shade is how far from that, more so higher up.
        let contrast = 0.45 + 0.85 * t;
        let k = (0.8 + (l - 0.7) * 1.8 * contrast).clamp(0.0, 1.0);
        let shadow = [96.0, 112.0, 132.0];
        let lit = [250.0, 244.0, 226.0];
        // The ground: grey-green low, warmer and lighter high.
        let ground = [226.0 + 26.0 * t, 232.0 + 16.0 * t, 216.0 + 22.0 * t];
        // Air over the lowland: pale blue-grey.
        let haze = 0.35 * (1.0 - t).powf(1.5);
        let mut c = [0f32; 3];
        for j in 0..3 {
            let shade = shadow[j] + (lit[j] - shadow[j]) * k;
            let base = ground[j] * shade / 255.0;
            c[j] = base * (1.0 - haze) + [226.0, 232.0, 238.0][j] * haze;
        }
        rgba[i * 4..i * 4 + 4].copy_from_slice(&[c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8, 255]);
    }
    RasterImage { vector: None, width: g.width, height: g.height, rgba, geotransform: g.geotransform }
}

/// Hachures over a grid of heights: for points on a staggered lattice `every` pixels apart
/// on the map, a stroke down the slope, as long as the lattice is wide, and as thick as the
/// slope is steep. Each stroke as two ends in the coordinates of the grid, and its width
/// from 0 to 1. `to_map` turns a place on the map in pixels into the coordinates of the grid.
pub fn hachures(g: &Grid, origin: [f64; 2], px: f64, size: (usize, usize), every: f32) -> Vec<([f32; 2], [f32; 2], f32)> {
    let (w, h) = (g.width, g.height);
    let gt = g.geotransform;
    if gt[1] == 0.0 || gt[5] == 0.0 || w < 3 || h < 3 {
        return Vec::new();
    }
    let at = |c: f64, r: f64| -> Option<f64> {
        let (x, y) = (c.floor() as i64, r.floor() as i64);
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            return None;
        }
        let v = g.values[y as usize * w + x as usize];
        (!v.is_nan()).then_some(v as f64)
    };
    let mut out = Vec::new();
    let step = every.max(3.0) as f64;
    let mut row = 0;
    let mut py = step / 2.0;
    while py < size.1 as f64 {
        let shift = if row % 2 == 1 { step / 2.0 } else { 0.0 };
        let mut px_ = step / 2.0 + shift;
        while px_ < size.0 as f64 {
            let (x, y) = (origin[0] + px_ * px, origin[1] - py * px);
            let (c, r) = ((x - gt[0]) / gt[1], (y - gt[3]) / gt[5]);
            if let (Some(l), Some(rr), Some(u), Some(d)) = (at(c - 1.0, r), at(c + 1.0, r), at(c, r - 1.0), at(c, r + 1.0)) {
                // The slope in metres per metre, east and north.
                let gx = (rr - l) / (2.0 * gt[1]);
                let gy = (u - d) / (2.0 * gt[5].abs());
                let gy = if gt[5] < 0.0 { gy } else { -gy };
                let slope = gx.hypot(gy);
                if slope > 0.14 {
                    // Down the slope, on the map: x east, y south (pixels).
                    let (dx, dy) = (-gx / slope, gy / slope);
                    let half = step * 0.48;
                    out.push((
                        [(px_ - dx * half) as f32, (py - dy * half) as f32],
                        [(px_ + dx * half) as f32, (py + dy * half) as f32],
                        ((slope.atan() / 0.9).min(1.0)) as f32,
                    ));
                }
            }
            px_ += step;
        }
        py += step * 0.62;
        row += 1;
    }
    out
}

/// How a grid is shown in the map under the canvas, before anyone has chosen: classes in a
/// colour each, other values by a ramp, over grey relief so that heights still show their
/// shape. At most `most` cells across: a larger grid is shown by every n-th cell, so that
/// classes keep their values.
pub fn preview(g: &Grid, most: usize) -> RasterImage {
    let i = grid_info(g);
    let small;
    let g = if g.width > most {
        let n = g.width.div_ceil(most);
        let (w, h) = (g.width.div_ceil(n), g.height.div_ceil(n));
        let mut values = Vec::with_capacity(w * h);
        for r in 0..h {
            for c in 0..w {
                values.push(g.values[(r * n) * g.width + c * n]);
            }
        }
        let t = g.geotransform;
        let k = n as f64;
        small = Grid {
            width: w,
            height: h,
            values,
            geotransform: [t[0], t[1] * k, t[2] * k, t[3], t[4] * k, t[5] * k],
            crs: g.crs,
            bands: Vec::new(),
            meta: Default::default(),
        };
        &small
    } else {
        g
    };
    if let Some(values) = &i.classes {
        let classes = values.iter().enumerate().map(|(k, v)| (*v, Some(class_colour(k)))).collect();
        return picture(g, &Paint::Classes(classes));
    }
    let (lo, hi) = i.range.unwrap_or((0.0, 1.0));
    let mut img = picture(g, &Paint::Ramp { ramp: "viridis".into(), lo, hi });
    let relief = picture(g, &Paint::Shade);
    // The colours lit by the relief: light where it is light, darker where it is shaded.
    for (c, r) in img.rgba.chunks_exact_mut(4).zip(relief.rgba.chunks_exact(4)) {
        if c[3] == 0 {
            continue;
        }
        let light = 0.45 + 0.55 * (r[0] as f32 / 255.0);
        for v in c.iter_mut().take(3) {
            *v = (*v as f32 * light).min(255.0) as u8;
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_of_depth_land_and_sea_in_a_colour_each_and_the_sea_alone() {
        // A floor that falls from land in the west to 6000 m deep in the east.
        let (w, h) = (60, 20);
        let values: Vec<f32> = (0..w * h).map(|i| 200.0 - (i % w) as f32 * 105.0).collect();
        let g = Grid {
            width: w,
            height: h,
            values,
            geotransform: [0.0, 1000.0, 0.0, 20000.0, 0.0, -1000.0],
            crs: None,
            bands: Vec::new(),
            meta: Default::default(),
        };
        let t = Table { grid: Some(std::sync::Arc::new(g.clone())), ..Table::default() };
        let spec = |mode: &str| RasterSpec { mode: mode.into(), opacity: 1.0, ..RasterSpec::default() };
        // The steps of an atlas: the land and seven steps of the sea, the deepest first in
        // the legend, the land not in it.
        let p = prepare(&t, &spec("paper")).unwrap().unwrap();
        assert_eq!(p.keys.first().map(|k| k.text.as_str()), Some("5 000 m"));
        assert_eq!(p.keys.last().map(|k| k.text.as_str()), Some("< 200 m"));
        assert!(!p.keys.iter().any(|k| k.text == "Land"));
        let Paint::Bands { steps, shadow } = &p.paint else { panic!("{:?}", p.paint) };
        assert!(*shadow);
        assert_eq!(colour_of(&p.paint, -100.0), steps[1].1);
        assert_eq!(colour_of(&p.paint, -2500.0), steps[4].1);
        assert_eq!(colour_of(&p.paint, -9000.0), steps[7].1);
        // Each step casts a shadow on the deeper step east of it, and not on itself further
        // from its edge.
        let paper = picture(&g, &p.paint);
        let flat = picture(&g, &prepare(&t, &spec("bands")).unwrap().unwrap().paint);
        let at = |img: &RasterImage, x: usize| img.rgba[(10 * w + x) * 4] as i32;
        let edge = (0..w).find(|x| (200.0 - *x as f32 * 105.0) < -1000.0).unwrap();
        assert!(at(&paper, edge) < at(&flat, edge) - 3, "{} {}", at(&paper, edge), at(&flat, edge));
        assert_eq!(at(&paper, edge + 6), at(&flat, edge + 6));
        // Steps of one's own, from a value up to the next.
        let own =
            RasterSpec { colors: vec![("0".into(), "#ffffff".into(), "Land".into()), ("-3000".into(), "#0000ff".into(), "Deep".into())], ..spec("bands") };
        let p = prepare(&t, &own).unwrap().unwrap();
        assert_eq!(colour_of(&p.paint, 100.0), Some([255, 255, 255, 255]));
        assert_eq!(colour_of(&p.paint, -2000.0), Some([0, 0, 255, 255]));
        // Under the lowest step is on the lowest step.
        assert_eq!(colour_of(&p.paint, -4000.0), Some([0, 0, 255, 255]));
        assert_eq!(p.keys.len(), 2);
        // Land and sea in a colour each: both with relief; the sea alone leaves the land out.
        let ls = RasterSpec { ramp: "#e0d0b0 #80c0c0".into(), ..spec("landsea") };
        let p = prepare(&t, &ls).unwrap().unwrap();
        assert_eq!(colour_of(&p.paint, 100.0), Some([0xe0, 0xd0, 0xb0, 255]));
        assert!(colour_of(&p.paint, -5000.0).unwrap()[2] < 0xc0);
        let sea = prepare(&t, &spec("sea")).unwrap().unwrap();
        assert_eq!(colour_of(&sea.paint, 100.0), None);
        assert!(picture(&g, &sea.paint).rgba[3] == 0);
    }

    #[test]
    fn swiss_relief_and_hachures() {
        // A cone: steep sides, a top in the middle.
        let (w, h) = (40, 40);
        let values: Vec<f32> = (0..w * h).map(|i| (300.0 - (((i % w) as f32 - 20.0).hypot((i / w) as f32 - 20.0)) * 12.0).max(0.0)).collect();
        let g = Grid { width: w, height: h, values, geotransform: [0.0, 10.0, 0.0, 400.0, 0.0, -10.0], crs: None, bands: Vec::new(), meta: Default::default() };
        let img = swiss(&g, 0.0, 300.0);
        assert_eq!(img.rgba.len(), w * h * 4);
        // The lowland is paler than the shaded side of the cone.
        let at = |x: usize, y: usize| img.rgba[(y * w + x) * 4] as i32;
        assert!(at(1, 1) > at(28, 28) - 5, "{} {}", at(1, 1), at(28, 28));
        let strokes = hachures(&g, [0.0, 400.0], 10.0, (40, 40), 6.0);
        assert!(!strokes.is_empty());
        // Each runs down the slope: its second end is further from the top than its first.
        for (a, b, _) in &strokes {
            let d = |p: [f32; 2]| (p[0] - 20.0).hypot(p[1] - 20.0);
            assert!(d(*b) > d(*a), "{:?} {:?}", a, b);
        }
    }

    #[test]
    fn relief_is_lit_and_coloured_by_height() {
        // A slope rising to the east is lit otherwise than flat land.
        let (w, h) = (8, 8);
        let values: Vec<f32> = (0..w * h).map(|i| (i % w) as f32 * 10.0).collect();
        let g = Grid { width: w, height: h, values, geotransform: [0.0, 10.0, 0.0, 80.0, 0.0, -10.0], crs: None, bands: Vec::new(), meta: Default::default() };
        let light = soft_light(&g);
        assert_eq!(light.len(), w * h);
        assert!(light.iter().all(|l| (0.0..=1.0).contains(l)));
        let flat = Grid { values: vec![5.0; w * h], ..g.clone() };
        assert!((soft_light(&flat)[27] - light[27]).abs() > 0.02);
        assert_ne!(hypsometric(0.0), hypsometric(1.0));
        for mode in ["soft", "relief"] {
            let spec = RasterSpec { mode: mode.into(), ..Default::default() };
            assert!(prepare(&g.clone().into_table(), &spec).unwrap().is_some(), "{mode}");
        }
    }

    #[test]
    fn classes_ramps_and_own_colours() {
        let g = Grid {
            width: 3,
            height: 1,
            values: vec![0.0, 1.0, 2.0],
            geotransform: [0.0, 1.0, 0.0, 1.0, 0.0, -1.0],
            crs: None,
            bands: Vec::new(),
            meta: Default::default(),
        };
        let i = grid_info(&g);
        assert_eq!(i.classes, Some(vec![0.0, 1.0, 2.0]));
        // Seen in green, not seen not drawn at all.
        let spec = RasterSpec { colors: vec![("0".into(), "none".into(), String::new()), ("1".into(), "#22aa44".into(), "Seen".into())], ..Default::default() };
        let p = prepare(&g.clone().into_table(), &spec).unwrap().unwrap();
        assert_eq!(colour_of(&p.paint, 0.0), None);
        assert_eq!(colour_of(&p.paint, 1.0), Some([0x22, 0xaa, 0x44, 255]));
        assert!(colour_of(&p.paint, 2.0).is_some());
        assert_eq!(p.keys.len(), 2, "the value not drawn is not in the legend");
        assert_eq!(p.keys[0].text, "Seen");
        // Decimals go by a ramp, from the smallest to the largest.
        let h = Grid { values: vec![0.5, 1.5, 2.5], ..g.clone() };
        let p = prepare(&h.into_table(), &RasterSpec { ramp: "greys".into(), ..Default::default() }).unwrap().unwrap();
        let (a, b) = (colour_of(&p.paint, 0.5).unwrap(), colour_of(&p.paint, 2.5).unwrap());
        assert!(a[0] > b[0], "light to dark");
        assert_eq!(p.keys.len(), 5);
    }

    #[test]
    fn a_grid_is_drawn_on_a_map_in_its_colours() {
        // Left half seen, right half not, 10 × 10 cells of 10 m.
        let values = (0..100).map(|i| if i % 10 < 5 { 1.0 } else { 0.0 }).collect();
        let g = Grid {
            width: 10,
            height: 10,
            values,
            geotransform: [0.0, 10.0, 0.0, 100.0, 0.0, -10.0],
            crs: Some(3006),
            bands: Vec::new(),
            meta: Default::default(),
        };
        let t = g.into_table();
        let spec = RasterSpec { colors: vec![("0".into(), "none".into(), String::new()), ("1".into(), "#22aa44".into(), String::new())], ..Default::default() };
        let r = prepare(&t, &spec).unwrap();
        let req = crate::mapdraw::Request {
            layers: vec![(&t, crate::mapdraw::LayerStyle::default_for(0))],
            themes: Vec::new(),
            labels: Vec::new(),
            icons: Vec::new(),
            charts: Vec::new(),
            rasters: vec![r],
            legend: None,
            width: 100,
            margin: 0.0,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
        };
        let img = crate::mapdraw::draw(&req, &|| true).unwrap();
        let at = |x: usize, y: usize| img.rgba[(y * img.width + x) * 4..][..3].to_vec();
        assert_eq!(at(20, 50), [0x22, 0xaa, 0x44]);
        assert_eq!(at(80, 50), [255, 255, 255], "not seen is not drawn, and no outline either");
    }
}
