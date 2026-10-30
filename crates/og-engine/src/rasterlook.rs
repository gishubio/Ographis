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
        Paint::Shade | Paint::Image => Some([128, 128, 128, 255]),
    }
}

/// A grid as a picture with the same cells, coloured as asked. Shade gives grey relief.
pub fn picture(g: &Grid, paint: &Paint) -> RasterImage {
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
    RasterImage { width: g.width, height: g.height, rgba, geotransform: g.geotransform }
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
        small = Grid { width: w, height: h, values, geotransform: [t[0], t[1] * k, t[2] * k, t[3], t[4] * k, t[5] * k], crs: g.crs, bands: Vec::new() };
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
    fn classes_ramps_and_own_colours() {
        let g = Grid { width: 3, height: 1, values: vec![0.0, 1.0, 2.0], geotransform: [0.0, 1.0, 0.0, 1.0, 0.0, -1.0], crs: None, bands: Vec::new() };
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
        let g = Grid { width: 10, height: 10, values, geotransform: [0.0, 10.0, 0.0, 100.0, 0.0, -10.0], crs: Some(3006), bands: Vec::new() };
        let t = g.into_table();
        let spec = RasterSpec { colors: vec![("0".into(), "none".into(), String::new()), ("1".into(), "#22aa44".into(), String::new())], ..Default::default() };
        let r = prepare(&t, &spec).unwrap();
        let req = crate::mapdraw::Request {
            layers: vec![(&t, crate::mapdraw::LayerStyle::default_for(0))],
            themes: Vec::new(),
            labels: Vec::new(),
            icons: Vec::new(),
            rasters: vec![r],
            legend: None,
            width: 100,
            margin: 0.0,
            background: Some([255, 255, 255, 255]),
        };
        let img = crate::mapdraw::draw(&req, &|| true).unwrap();
        let at = |x: usize, y: usize| img.rgba[(y * img.width + x) * 4..][..3].to_vec();
        assert_eq!(at(20, 50), [0x22, 0xaa, 0x44]);
        assert_eq!(at(80, 50), [255, 255, 255], "not seen is not drawn, and no outline either");
    }
}
