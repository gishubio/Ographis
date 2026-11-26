//! Maps that show values: the size of points after a number, the colour of points, lines
//! and areas after a number in classes or after a text, the order they are drawn in, and
//! points that lie close together drawn as one with their number.

use crate::mapdraw::LayerStyle;
use crate::model::{GbError, Result, Table, Value};
use geo::Geometry;

/// How a layer shows the values of its features, as the user sets it. Empty names mean off.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeSpec {
    /// Points get a size after this number: an area that grows with the value.
    pub size_by: String,
    pub size_min: f32,
    pub size_max: f32,
    /// Points, lines or areas get a colour after this attribute.
    pub color_by: String,
    /// The colours: a ramp for numbers, such as "reds", or "categories" for text.
    pub ramp: String,
    pub classes: usize,
    /// "equal" intervals, the same number in each ("quantile"), or "natural" breaks.
    pub method: String,
    /// "large_below" draws the largest values first, so that small ones lie on top;
    /// "small_below" the other way. Empty keeps the order of the stream.
    pub order: String,
    /// The attribute the order goes by. Empty uses the one sizes go by.
    pub order_by: String,
    /// Points closer than this many pixels are drawn as one, with their number. 0 is off.
    pub cluster: f32,
    /// Points are drawn as arrows pointing the way of this attribute, in degrees from north
    /// and clockwise, such as the direction of wind or a current. Their length is the size.
    pub arrow_by: String,
    /// The direction says where it comes from, as for wind, so the arrow points the other way.
    pub arrow_from: bool,
    /// Points are drawn as a heatmap: how many lie near each place, within this many pixels.
    /// 0 is off.
    pub heat: f32,
    /// What each point counts as in the heatmap. Empty: each counts as one.
    pub heat_by: String,
    /// The ramp the other way: dark for the smallest values.
    pub reverse: bool,
    /// The limits of the classes rounded to even numbers, as 1 000 – 2 000.
    pub round_breaks: bool,
    /// A second attribute, coloured with the first in a square of three by three colours.
    pub color_by2: String,
    /// A dot for every `dots_per` of this attribute, strewn over each area.
    pub dots_by: String,
    pub dots_per: f64,
    /// The sizes in the legend as circles within each other.
    pub nested: bool,
    /// Bars under the classes in the legend, as high as how many features each has.
    pub histogram: bool,
}

impl Default for ThemeSpec {
    fn default() -> ThemeSpec {
        ThemeSpec {
            size_by: String::new(),
            size_min: 4.0,
            size_max: 24.0,
            color_by: String::new(),
            ramp: String::new(),
            classes: 5,
            method: String::new(),
            order: String::new(),
            order_by: String::new(),
            cluster: 0.0,
            arrow_by: String::new(),
            arrow_from: false,
            heat: 0.0,
            heat_by: String::new(),
            reverse: false,
            round_breaks: false,
            color_by2: String::new(),
            dots_by: String::new(),
            dots_per: 0.0,
            nested: false,
            histogram: false,
        }
    }
}

impl ThemeSpec {
    pub fn is_off(&self) -> bool {
        self.size_by.is_empty()
            && self.color_by.is_empty()
            && self.order.is_empty()
            && self.cluster <= 0.0
            && self.arrow_by.is_empty()
            && self.heat <= 0.0
            && self.dots_by.is_empty()
    }
}

/// How one feature is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub fill: Option<[u8; 4]>,
    pub stroke: Option<[u8; 4]>,
    pub size: f32,
}

/// A line of the legend: the colour or size, and what it stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub look: Look,
    pub text: String,
}

/// What a class of colours holds, for other programs to draw it by, such as QGIS.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassKey {
    /// Numbers from and to, the first class from its lowest.
    Range(f64, f64),
    /// One text.
    Value(String),
    /// The texts that did not get colours of their own.
    Other,
    /// Features without a value.
    NoValue,
    /// A colour as written in the attribute.
    Written,
}

/// A layer made ready to draw its values.
#[derive(Debug, Clone, Default)]
pub struct Theme {
    /// The classes of the colours, with the look of each, in the order of the keys.
    pub classes: Vec<(ClassKey, Look)>,
    /// A look per feature, by its place in the stream. None draws all in the look of the layer.
    pub looks: Option<Vec<Look>>,
    /// The order to draw in. None keeps the order of the stream.
    pub order: Option<Vec<usize>>,
    pub cluster: f32,
    /// What the colours and sizes stand for, for the legend.
    pub keys: Vec<Key>,
    /// The heading of the keys: the attribute.
    pub heading: String,
    /// What the sizes stand for, when the colours stand for something else.
    pub size_keys: Vec<Key>,
    pub size_heading: String,
    /// For arrows: the angle on the map of each point, in radians clockwise from up. None
    /// where it has no direction.
    pub angles: Option<Vec<Option<f32>>>,
    /// For a heatmap: its reach in pixels, the weight of each point, and the colour ramp.
    pub heat: f32,
    pub heat_weights: Option<Vec<f32>>,
    pub heat_ramp: String,
    /// Lines are sized by a value, and drawn as wide as their size, as in a flow map.
    pub sized_lines: bool,
    /// How many features each key of the colours has, for bars in the legend. Empty: none.
    pub counts: Vec<usize>,
    /// Two attributes in one colour: the nine colours and the two attributes.
    pub bivariate: Option<Bivariate>,
    /// The number of dots in each feature, their colour and size in pixels, and what a dot
    /// stands for.
    pub dots: Option<Vec<usize>>,
    pub dot_colour: [u8; 4],
    pub dot_size: f32,
    pub dot_text: String,
    /// The sizes in the legend as circles within each other.
    pub nested: bool,
}

/// The colours of two attributes, low to high on each: `colours[b * 3 + a]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bivariate {
    pub colours: [[u8; 4]; 9],
    pub a: String,
    pub b: String,
}

/// The nine colours of two attributes: the first from grey to teal, the second from grey to
/// pink, and both high a deep blue.
pub const BIVARIATE: [[u8; 3]; 9] = [
    [0xe8, 0xe8, 0xe8],
    [0xac, 0xe4, 0xe4],
    [0x5a, 0xc8, 0xc8],
    [0xdf, 0xb0, 0xd6],
    [0xa5, 0xad, 0xd3],
    [0x56, 0x98, 0xb9],
    [0xbe, 0x64, 0xac],
    [0x8c, 0x62, 0xaa],
    [0x3b, 0x49, 0x94],
];

/// Limits of classes moved to even numbers: as coarse as they can be while each stays near
/// where it was, the lowest at or under the smallest value and the highest at or over the largest.
pub fn round_breaks(b: &[f64]) -> Vec<f64> {
    let n = b.len();
    if n < 2 || b[n - 1].partial_cmp(&b[0]) != Some(std::cmp::Ordering::Greater) {
        return b.to_vec();
    }
    let span = (b[n - 1] - b[0]) / (n - 1) as f64;
    let mag = 10f64.powf(span.log10().floor());
    for unit in [10.0, 5.0, 2.0, 1.0, 0.5, 0.2, 0.1, 0.05, 0.02, 0.01].map(|u| u * mag) {
        let mut r: Vec<f64> = b.iter().map(|v| (v / unit).round() * unit).collect();
        r[0] = (b[0] / unit).floor() * unit;
        r[n - 1] = (b[n - 1] / unit).ceil() * unit;
        // Away from the noise of floating point, as 0.30000000000000004.
        let r: Vec<f64> = r.iter().map(|v| (v / unit).round() * unit).map(|v| (v * 1e9).round() / 1e9).collect();
        // No limit moves far into a class beside it: the inner ones less than a third of the
        // narrower class, the outer ones less than half of theirs.
        let near = (0..n).all(|i| {
            let left = if i > 0 { b[i] - b[i - 1] } else { f64::INFINITY };
            let right = if i + 1 < n { b[i + 1] - b[i] } else { f64::INFINITY };
            let share = if i == 0 || i + 1 == n { 0.5 } else { 0.3 };
            (r[i] - b[i]).abs() <= share * left.min(right) + 1e-12
        });
        if near && r.windows(2).all(|w| w[1] > w[0]) {
            return r;
        }
    }
    b.to_vec()
}

/// The colour ramps for numbers, each from light to dark: (value, label, stops).
pub const RAMPS: &[(&str, &str, &[[u8; 3]])] = &[
    ("blues", "Blues", &[[0xe4, 0xef, 0xfa], [0x8f, 0xbc, 0xeb], [0x2a, 0x78, 0xd6], [0x12, 0x3f, 0x80]]),
    ("reds", "Reds", &[[0xfb, 0xe5, 0xe1], [0xf2, 0xa1, 0x95], [0xe3, 0x49, 0x48], [0x8c, 0x1b, 0x1d]]),
    ("greens", "Greens", &[[0xe3, 0xf3, 0xdf], [0x9c, 0xd3, 0x8f], [0x2e, 0x9a, 0x3e], [0x0d, 0x55, 0x22]]),
    ("oranges", "Oranges", &[[0xfd, 0xec, 0xdc], [0xf8, 0xb2, 0x7a], [0xeb, 0x68, 0x34], [0x8f, 0x33, 0x0e]]),
    ("purples", "Purples", &[[0xec, 0xe8, 0xf7], [0xb3, 0xa7, 0xe6], [0x62, 0x50, 0xd6], [0x33, 0x23, 0x80]]),
    ("greys", "Greys", &[[0xf1, 0xf0, 0xec], [0xbd, 0xbb, 0xb2], [0x89, 0x87, 0x81], [0x3a, 0x39, 0x36]]),
    ("viridis", "Yellow to blue", &[[0xfd, 0xe7, 0x25], [0x5e, 0xc9, 0x62], [0x21, 0x91, 0x8c], [0x3b, 0x52, 0x8b], [0x44, 0x01, 0x54]]),
    ("heat", "Yellow to red", &[[0xff, 0xf3, 0xb0], [0xfd, 0xb8, 0x4b], [0xf0, 0x6b, 0x2c], [0xc0, 0x1d, 0x1d], [0x6b, 0x0a, 0x12]]),
    ("diverging", "Blue to red", &[[0x21, 0x66, 0xac], [0x92, 0xc5, 0xde], [0xf0, 0xef, 0xea], [0xf4, 0xa5, 0x82], [0xb2, 0x18, 0x2b]]),
    (
        "rainbow",
        "Depths: violet, blue, green, yellow to red",
        &[
            [0x5c, 0x28, 0x96],
            [0x34, 0x46, 0xd2],
            [0x24, 0x96, 0xe2],
            [0x30, 0xca, 0xba],
            [0x78, 0xd6, 0x54],
            [0xec, 0xe0, 0x3e],
            [0xf6, 0x96, 0x2c],
            [0xde, 0x2e, 0x28],
        ],
    ),
    (
        "sea",
        "Sea: navy, turquoise, green to pale yellow",
        &[[0x0e, 0x1e, 0x62], [0x16, 0x4a, 0xb0], [0x1e, 0x8e, 0xd6], [0x2e, 0xcc, 0xd6], [0x48, 0xdc, 0x8a], [0xb6, 0xee, 0x50], [0xf6, 0xfc, 0xba]],
    ),
    ("terrain", "Heights: green to brown", &[[0xec, 0xf2, 0xd0], [0xdc, 0xdc, 0xa2], [0xcc, 0xb2, 0x7c], [0xa8, 0x84, 0x5e], [0x76, 0x58, 0x44]]),
];

/// The colours for texts, in the order they are given out. The ninth and later values are
/// "Other", in grey.
pub const CATEGORIES: &[[u8; 3]] = &[
    [0x2a, 0x78, 0xd6],
    [0xeb, 0x68, 0x34],
    [0x1b, 0xaf, 0x7a],
    [0xed, 0xa1, 0x00],
    [0xe8, 0x7b, 0xa4],
    [0x00, 0x83, 0x00],
    [0x62, 0x50, 0xd6],
    [0xe3, 0x49, 0x48],
];
const OTHER: [u8; 3] = [0xa8, 0xa6, 0x9f];
const NO_VALUE: [u8; 3] = [0xd9, 0xd7, 0xcf];

/// The colour of a theme at a place from 0 to 1: from its ramp, or between two colours of its
/// own written as "#e4effa,#123f80", the other way when asked.
fn theme_colour(spec: &ThemeSpec, t: f64) -> [u8; 3] {
    let t = if spec.reverse { 1.0 - t } else { t };
    if let Some((a, b)) = own_ramp(&spec.ramp) {
        let t = t.clamp(0.0, 1.0);
        return [0, 1, 2].map(|k| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * t).round() as u8);
    }
    let ramp = if RAMPS.iter().any(|r| r.0 == spec.ramp) { spec.ramp.as_str() } else { "blues" };
    ramp_at(ramp, t)
}

/// A ramp of one's own: two colours, from the smallest values to the largest.
pub fn own_ramp(ramp: &str) -> Option<([u8; 3], [u8; 3])> {
    let (a, b) = ramp.split_once(',')?;
    let c = |t: &str| crate::mapdraw::parse_color(t).ok().flatten().map(|c| [c[0], c[1], c[2]]);
    Some((c(a)?, c(b)?))
}

/// A colour of a ramp at a place from 0 to 1.
pub fn ramp_at(name: &str, t: f64) -> [u8; 3] {
    let stops = RAMPS.iter().find(|r| r.0 == name).map_or(RAMPS[0].2, |r| r.2);
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let (i, f) = (t.floor() as usize, t.fract());
    let (a, b) = (stops[i.min(stops.len() - 1)], stops[(i + 1).min(stops.len() - 1)]);
    [0, 1, 2].map(|k| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * f).round() as u8)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Text(t) => crate::model::parse_number(t),
        Value::Bool(b) => Some(*b as i64 as f64),
        _ => None,
    }
}

/// A number for a legend: whole when it is, else with few decimals, and spaces between thousands.
pub fn short(x: f64) -> String {
    let d = if x.abs() >= 100.0 || x.fract() == 0.0 {
        0
    } else if x.abs() >= 1.0 {
        1
    } else {
        2
    };
    crate::chart::number(x, d, true)
}

/// Breaks that make classes of the sorted values: Jenks' natural breaks, which put values
/// that lie close together in the same class. On many values, a sample is used.
pub fn natural_breaks(sorted: &[f64], classes: usize) -> Vec<f64> {
    let data: Vec<f64> = if sorted.len() > 2000 {
        let step = sorted.len() as f64 / 2000.0;
        (0..2000).map(|i| sorted[(i as f64 * step) as usize]).collect()
    } else {
        sorted.to_vec()
    };
    let n = data.len();
    let k = classes.min(n).max(1);
    // mat1 holds the lower limits, mat2 the variances, as in Jenks' algorithm.
    let mut lower = vec![vec![0usize; k + 1]; n + 1];
    let mut var = vec![vec![f64::INFINITY; k + 1]; n + 1];
    for j in 1..=k {
        lower[1][j] = 1;
        var[1][j] = 0.0;
    }
    for l in 2..=n {
        let (mut s1, mut s2, mut w) = (0.0, 0.0, 0.0);
        for m in 1..=l {
            let i3 = l - m + 1;
            let val = data[i3 - 1];
            s2 += val * val;
            s1 += val;
            w += 1.0;
            let v = s2 - s1 * s1 / w;
            let i4 = i3 - 1;
            if i4 != 0 {
                for j in 2..=k {
                    if var[l][j] >= v + var[i4][j - 1] {
                        lower[l][j] = i3;
                        var[l][j] = v + var[i4][j - 1];
                    }
                }
            }
        }
        lower[l][1] = 1;
        var[l][1] = s2 - s1 * s1 / w;
    }
    let mut breaks = vec![0.0; k + 1];
    breaks[k] = data[n - 1];
    breaks[0] = data[0];
    let mut at = n;
    for j in (2..=k).rev() {
        let id = lower[at][j].saturating_sub(2);
        breaks[j - 1] = data[id];
        at = lower[at][j].saturating_sub(1).max(1);
    }
    breaks.dedup_by(|a, b| a == b);
    breaks
}

/// Makes a layer ready to show its values, from what the user has set.
pub fn prepare(table: &Table, style: &LayerStyle, spec: &ThemeSpec) -> Result<Theme> {
    let mut theme = Theme { cluster: spec.cluster.max(0.0), ..Default::default() };
    if spec.is_off() {
        return Ok(theme);
    }
    let field = |name: &str| -> Result<Option<usize>> {
        if name.trim().is_empty() {
            return Ok(None);
        }
        table
            .field_index(name.trim())
            .map(Some)
            .ok_or_else(|| GbError(format!("The layer has no attribute {} to draw by. It has: {}.", name.trim(), table.field_names().join(", "))))
    };
    // Arrows: the angle of each point; from where the wind comes, the arrow points away.
    if let Some(i) = field(&spec.arrow_by)? {
        let turn = if spec.arrow_from { std::f32::consts::PI } else { 0.0 };
        theme.angles = Some(table.features.iter().map(|f| f.attrs[i].as_f64().map(|d| (d as f32).to_radians() + turn)).collect());
    }
    // A heatmap: the weight of each point, and the ramp from the colour setting.
    if spec.heat > 0.0 {
        theme.heat = spec.heat;
        theme.heat_weights = field(&spec.heat_by)?.map(|i| table.features.iter().map(|f| f.attrs[i].as_f64().unwrap_or(0.0).max(0.0) as f32).collect());
        theme.heat_ramp = if RAMPS.iter().any(|r| r.0 == spec.ramp) { spec.ramp.clone() } else { "heat".into() };
        return Ok(theme);
    }
    let size_at = field(&spec.size_by)?;
    let color_at = field(&spec.color_by)?;
    let order_at = match field(&spec.order_by)? {
        Some(i) => Some(i),
        None => size_at.or(color_at),
    };
    let n = table.len();
    let mut looks: Vec<Look> = vec![Look { fill: style.fill, stroke: style.stroke, size: style.size }; n];

    // Size: the area of the point grows with the value, from the smallest to the largest.
    if let Some(i) = size_at {
        let vals: Vec<Option<f64>> = table.features.iter().map(|f| number(&f.attrs[i])).collect();
        let (lo, hi) = vals.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
        let (smin, smax) = (spec.size_min.max(1.0), spec.size_max.max(spec.size_min.max(1.0)));
        let size_of = |v: f64| {
            if hi <= lo {
                return (smin + smax) / 2.0;
            }
            let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
            // By area: the diameter goes with the square root.
            (smin.powi(2) + (smax.powi(2) - smin.powi(2)) * t as f32).sqrt()
        };
        // Lines get a width that grows with the value itself; points an area that does.
        let lines = table.features.iter().filter_map(|f| f.geom.as_ref()).all(|g| matches!(g, Geometry::LineString(_) | Geometry::MultiLineString(_)));
        theme.sized_lines = lines && table.has_geometry();
        for (l, v) in looks.iter_mut().zip(&vals) {
            l.size = match (v, lines) {
                (Some(v), true) if hi > lo => smin + (smax - smin) * ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32,
                (Some(v), _) => size_of(*v),
                (None, _) => smin,
            };
        }
        if lo.is_finite() {
            theme.heading = spec.size_by.trim().to_string();
            let mid = (lo + hi) / 2.0;
            for v in if hi > lo { vec![hi, mid, lo] } else { vec![lo] } {
                theme.keys.push(Key { look: Look { fill: style.fill, stroke: style.stroke, size: size_of(v) }, text: short(v) });
            }
        }
    }

    // Colour: classes of a number on a ramp, or a colour per text.
    if let Some(i) = color_at {
        let numeric = table.fields[i].ty != crate::model::FieldType::Text && spec.ramp != "categories";
        let mut keys = Vec::new();
        let mut class_keys: Vec<ClassKey> = Vec::new();
        let second = field(&spec.color_by2)?;
        let colors: Vec<Option<[u8; 3]>> = if let Some(j) = second {
            // Two attributes: each in thirds, by how many features have lower values.
            let thirds = |k: usize| -> Vec<Option<usize>> {
                let vals: Vec<Option<f64>> = table.features.iter().map(|f| number(&f.attrs[k])).collect();
                let mut sorted: Vec<f64> = vals.iter().flatten().copied().collect();
                sorted.sort_by(f64::total_cmp);
                if sorted.is_empty() {
                    return vec![None; vals.len()];
                }
                let (t1, t2) = (sorted[(sorted.len() - 1) / 3], sorted[(sorted.len() - 1) * 2 / 3]);
                vals.iter()
                    .map(|v| {
                        v.map(|v| {
                            if v <= t1 {
                                0
                            } else if v <= t2 {
                                1
                            } else {
                                2
                            }
                        })
                    })
                    .collect()
            };
            let (a, b) = (thirds(i), thirds(j));
            let alpha = style.fill.map_or(255, |c| c[3]).max(1);
            theme.bivariate = Some(Bivariate {
                colours: BIVARIATE.map(|c| [c[0], c[1], c[2], alpha]),
                a: spec.color_by.trim().to_string(),
                b: spec.color_by2.trim().to_string(),
            });
            a.iter().zip(&b).map(|(a, b)| a.zip(*b).map(|(a, b)| BIVARIATE[b * 3 + a])).collect()
        } else if spec.ramp == "as_written" {
            // The colours the attribute holds, such as the colours of a CAD drawing.
            let cs: Vec<Option<[u8; 3]>> =
                table.features.iter().map(|f| crate::mapdraw::parse_color(&f.attrs[i].to_text()).ok().flatten().map(|c| [c[0], c[1], c[2]])).collect();
            let mut seen: Vec<[u8; 3]> = Vec::new();
            for c in cs.iter().flatten() {
                if !seen.contains(c) && seen.len() < 12 {
                    seen.push(*c);
                }
            }
            for c in seen {
                keys.push((c, format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])));
                class_keys.push(ClassKey::Written);
            }
            cs
        } else if numeric {
            let vals: Vec<Option<f64>> = table.features.iter().map(|f| number(&f.attrs[i])).collect();
            let mut sorted: Vec<f64> = vals.iter().flatten().copied().collect();
            sorted.sort_by(f64::total_cmp);
            let classes = spec.classes.clamp(1, 12);
            let mut distinct = sorted.clone();
            distinct.dedup();
            // The names of the values from an attribute beside it, such as class_label beside
            // the class of Classify values: used in the legend when each value has one name.
            let names: Option<Vec<String>> = table.field_index(&format!("{}_label", spec.color_by.trim())).and_then(|li| {
                let mut named: Vec<Option<String>> = vec![None; distinct.len()];
                for (f, v) in table.features.iter().zip(&vals) {
                    let (Some(v), text) = (v, f.attrs[li].to_text()) else { continue };
                    let at = distinct.iter().position(|d| d == v)?;
                    match &named[at] {
                        Some(t) if *t != text => return None,
                        _ => named[at] = Some(text),
                    }
                }
                named.into_iter().collect()
            });
            if distinct.len() >= 2 && (distinct.len() <= classes || (names.is_some() && distinct.len() <= 12)) {
                // As few values as classes, such as 5, 10 and 15 minutes: a class for each value,
                // named by it, not ranges between them.
                let n = distinct.len();
                let color_of = |c: usize| theme_colour(spec, 0.08 + 0.92 * c as f64 / (n - 1) as f64);
                for (c, v) in distinct.iter().enumerate() {
                    keys.push((color_of(c), names.as_ref().map_or_else(|| short(*v), |n| n[c].clone())));
                    class_keys.push(ClassKey::Range(*v, *v));
                }
                vals.iter().map(|v| v.and_then(|v| distinct.iter().position(|d| *d == v)).map(color_of)).collect()
            } else {
                let breaks: Vec<f64> = if sorted.is_empty() {
                    Vec::new()
                } else {
                    match spec.method.as_str() {
                        "natural" => natural_breaks(&sorted, classes),
                        "quantile" => {
                            let mut b: Vec<f64> =
                                (0..=classes).map(|k| sorted[((sorted.len() - 1) as f64 * k as f64 / classes as f64).round() as usize]).collect();
                            b.dedup_by(|a, b| a == b);
                            b
                        }
                        _ => {
                            let (lo, hi) = (sorted[0], sorted[sorted.len() - 1]);
                            (0..=classes).map(|k| lo + (hi - lo) * k as f64 / classes as f64).collect()
                        }
                    }
                };
                let breaks = if spec.round_breaks { round_breaks(&breaks) } else { breaks };
                let count = breaks.len().saturating_sub(1).max(1);
                let color_of_class = |c: usize| theme_colour(spec, if count == 1 { 0.7 } else { 0.08 + 0.92 * c as f64 / (count - 1) as f64 });
                let class_of = |v: f64| breaks.windows(2).position(|w| v <= w[1]).unwrap_or(count - 1).min(count - 1);
                for c in 0..count {
                    if breaks.len() >= 2 {
                        let text = if breaks[c] == breaks[c + 1] { short(breaks[c]) } else { format!("{} – {}", short(breaks[c]), short(breaks[c + 1])) };
                        keys.push((color_of_class(c), text));
                        class_keys.push(ClassKey::Range(breaks[c], breaks[c + 1]));
                    }
                }
                if breaks.len() == 1 {
                    keys.push((color_of_class(0), short(breaks[0])));
                    class_keys.push(ClassKey::Range(breaks[0], breaks[0]));
                }
                vals.iter().map(|v| v.map(|v| color_of_class(if breaks.len() >= 2 { class_of(v) } else { 0 }))).collect()
            }
        } else {
            // The most common texts get the colours, the rest are Other.
            let mut counts: Vec<(String, usize)> = Vec::new();
            for f in &table.features {
                let t = f.attrs[i].to_text();
                if t.is_empty() {
                    continue;
                }
                match counts.iter_mut().find(|c| c.0 == t) {
                    Some(c) => c.1 += 1,
                    None => counts.push((t, 1)),
                }
            }
            let mut ranked = counts.clone();
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let chosen: Vec<String> = ranked.iter().take(CATEGORIES.len()).map(|c| c.0.clone()).collect();
            // In the legend in alphabetical order, Other last.
            let mut named = chosen.clone();
            // Names that begin with a number, such as 10 – 50, in the order of the numbers.
            let lead = |t: &str| {
                let first: String = t.split(['–', '—']).next().unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect();
                crate::model::parse_number(&first).unwrap_or(f64::INFINITY)
            };
            named.sort_by(|a, b| lead(a).total_cmp(&lead(b)).then_with(|| crate::nodes::alphabetical(a).cmp(&crate::nodes::alphabetical(b))));
            let color_of = |t: &str| chosen.iter().position(|c| c == t).map(|k| CATEGORIES[k]);
            for n in &named {
                keys.push((color_of(n).unwrap_or(OTHER), n.clone()));
                class_keys.push(ClassKey::Value(n.clone()));
            }
            if counts.len() > chosen.len() {
                keys.push((OTHER, "Other".to_string()));
                class_keys.push(ClassKey::Other);
            }
            table
                .features
                .iter()
                .map(|f| {
                    let t = f.attrs[i].to_text();
                    if t.is_empty() {
                        None
                    } else {
                        Some(color_of(&t).unwrap_or(OTHER))
                    }
                })
                .collect()
        };
        let lines = table.features.iter().all(|f| matches!(f.geom, Some(geo::Geometry::LineString(_) | geo::Geometry::MultiLineString(_)) | None));
        let alpha = style.fill.map_or(255, |c| c[3]).max(if lines { 255 } else { 0 });
        let written = spec.ramp == "as_written";
        let apply = |look: &mut Look, c: Option<[u8; 3]>| {
            let c = c.unwrap_or(NO_VALUE);
            if written {
                // Lines, points and the edges of areas all in their own colour, as drawn in CAD.
                look.stroke = Some([c[0], c[1], c[2], style.stroke.map_or(255, |s| s[3])]);
                if look.fill.is_some() {
                    look.fill = Some([c[0], c[1], c[2], alpha.max(1)]);
                }
            } else if lines {
                look.stroke = Some([c[0], c[1], c[2], style.stroke.map_or(255, |s| s[3])]);
            } else {
                look.fill = Some([c[0], c[1], c[2], alpha.max(1)]);
            }
        };
        for (l, c) in looks.iter_mut().zip(&colors) {
            apply(l, *c);
        }
        if colors.iter().any(|c| c.is_none()) {
            keys.push((NO_VALUE, "No value".to_string()));
            class_keys.push(ClassKey::NoValue);
        }
        if spec.histogram && second.is_none() {
            theme.counts = keys.iter().map(|(c, _)| colors.iter().filter(|x| x.unwrap_or(NO_VALUE) == *c).count()).collect();
        }
        let base = Look { fill: style.fill, stroke: style.stroke, size: if size_at.is_some() { 12.0 } else { style.size.max(10.0) } };
        // With both, the sizes get keys of their own.
        if size_at.is_some() {
            theme.size_heading = spec.size_by.trim().to_string();
            theme.size_keys = std::mem::take(&mut theme.keys);
            for k in theme.size_keys.iter_mut() {
                k.look.fill = Some([0xd9, 0xd7, 0xcf, 255]);
                k.look.stroke = Some([0x89, 0x87, 0x81, 255]);
            }
        }
        theme.heading = spec.color_by.trim().to_string();
        theme.keys = keys
            .into_iter()
            .map(|(c, text)| {
                let mut look = base;
                apply(&mut look, Some(c));
                Key { look, text }
            })
            .collect();
        theme.classes = class_keys.into_iter().zip(theme.keys.iter().map(|k| k.look)).collect();
    }

    // Order: the largest first, so that they lie under, or the smallest first.
    if let (Some(i), false) = (order_at, spec.order.is_empty()) {
        let mut idx: Vec<usize> = (0..n).collect();
        let key = |k: usize| number(&table.features[k].attrs[i]).unwrap_or(f64::NEG_INFINITY);
        idx.sort_by(|a, b| key(*a).total_cmp(&key(*b)));
        if spec.order == "large_below" {
            idx.reverse();
        }
        theme.order = Some(idx);
    }
    if size_at.is_some() || color_at.is_some() {
        theme.looks = Some(looks);
    }
    theme.nested = spec.nested && size_at.is_some() && !theme.sized_lines;
    // Dots: as many in each feature as its value holds of what a dot stands for.
    if let Some(i) = field(&spec.dots_by)? {
        let vals: Vec<f64> = table.features.iter().map(|f| number(&f.attrs[i]).unwrap_or(0.0).max(0.0)).collect();
        let total: f64 = vals.iter().sum();
        // A dot for what the user says, or for so much that there are about 2 000 dots.
        let per = if spec.dots_per > 0.0 { spec.dots_per } else { nice_number((total / 2_000.0).max(1.0)) };
        let per = per.max(total / 200_000.0);
        theme.dots = Some(vals.iter().map(|v| (v / per).round() as usize).collect());
        let c = style.stroke.unwrap_or([0x22, 0x22, 0x22, 255]);
        theme.dot_colour = [c[0], c[1], c[2], 255];
        theme.dot_size = 2.2;
        theme.dot_text = format!("1 dot = {} {}", crate::chart::number(per, if per < 10.0 { 1 } else { 0 }, true).replace('\u{a0}', " "), spec.dots_by.trim());
    }
    Ok(theme)
}

/// A round number near `x`: 1, 2 or 5 times a power of ten.
fn nice_number(x: f64) -> f64 {
    let mag = 10f64.powf(x.log10().floor());
    let f = x / mag;
    mag * if f < 1.5 {
        1.0
    } else if f < 3.5 {
        2.0
    } else if f < 7.5 {
        5.0
    } else {
        10.0
    }
}

/// Points that lie closer than `distance` pixels: groups of indexes into `points`, in the
/// order given, each led by the first of its members in that order.
pub fn clusters(points: &[(f32, f32)], order: &[usize], distance: f32) -> Vec<Vec<usize>> {
    let d = distance.max(1.0);
    let cell = |p: (f32, f32)| ((p.0 / d).floor() as i64, (p.1 / d).floor() as i64);
    let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for &i in order {
        grid.entry(cell(points[i])).or_default().push(i);
    }
    let mut taken = vec![false; points.len()];
    let mut out = Vec::new();
    for &i in order {
        if taken[i] {
            continue;
        }
        taken[i] = true;
        let mut group = vec![i];
        let (cx, cy) = cell(points[i]);
        for gx in cx - 1..=cx + 1 {
            for gy in cy - 1..=cy + 1 {
                for &j in grid.get(&(gx, gy)).map_or(&[][..], |v| v.as_slice()) {
                    if !taken[j] {
                        let (dx, dy) = (points[j].0 - points[i].0, points[j].1 - points[i].1);
                        if dx * dx + dy * dy <= d * d {
                            taken[j] = true;
                            group.push(j);
                        }
                    }
                }
            }
        }
        out.push(group);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, FieldType};

    fn table(values: &[f64]) -> Table {
        Table {
            fields: vec![Field::new("mag", FieldType::Real), Field::new("kind", FieldType::Text)],
            crs: Some(4326),
            features: values
                .iter()
                .enumerate()
                .map(|(i, v)| Feature {
                    z: None,
                    attrs: vec![Value::Real(*v), Value::Text(if i % 2 == 0 { "A".into() } else { "B".into() })],
                    geom: Some(geo::Geometry::Point(geo::Point::new(i as f64, 0.0))),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn sizes_colours_and_order_follow_the_values() {
        let t = table(&[4.5, 6.0, 5.0, 4.6]);
        let style = LayerStyle::default_for(0);
        let spec = ThemeSpec { size_by: "mag".into(), size_min: 4.0, size_max: 20.0, order: "large_below".into(), ..Default::default() };
        let th = prepare(&t, &style, &spec).unwrap();
        let looks = th.looks.unwrap();
        assert_eq!((looks[0].size, looks[1].size), (4.0, 20.0));
        assert!(looks[2].size > looks[3].size);
        assert_eq!(th.order.unwrap(), [1, 2, 3, 0], "the largest first, so it lies under");
        assert_eq!(th.keys.len(), 3);
        let spec = ThemeSpec { color_by: "mag".into(), ramp: "reds".into(), classes: 2, method: "equal".into(), ..Default::default() };
        let th = prepare(&t, &style, &spec).unwrap();
        let looks = th.looks.unwrap();
        assert_eq!(looks[0].fill, looks[3].fill);
        assert_ne!(looks[0].fill, looks[1].fill);
        assert_eq!(th.keys.iter().map(|k| k.text.as_str()).collect::<Vec<_>>(), ["4,5 – 5,2", "5,2 – 6"]);
        let spec = ThemeSpec { color_by: "kind".into(), ..Default::default() };
        let th = prepare(&t, &style, &spec).unwrap();
        assert_eq!(th.keys.iter().map(|k| k.text.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert!(prepare(&t, &style, &ThemeSpec { color_by: "nothing".into(), ..Default::default() }).is_err());
    }

    #[test]
    fn classes_are_named_by_their_label_and_ordered_by_number() {
        // As Classify values gives them: seven classes, more than the five of the theme.
        let mut t = Table { fields: vec![Field::new("class", FieldType::Int), Field::new("class_label", FieldType::Text)], ..Default::default() };
        let labels = ["0 – 2", "2 – 10", "10 – 50", "50 – 100", "100 – 500", "500 – 2 000", "2 000 – 9 000"];
        for (i, l) in labels.iter().enumerate().rev() {
            t.features.push(Feature {
                z: None,
                attrs: vec![Value::Int(i as i64 + 1), Value::Text(l.to_string())],
                geom: Some(geo::Geometry::Point(geo::Point::new(i as f64, 0.0))),
            });
        }
        let style = LayerStyle::default_for(0);
        let th = prepare(&t, &style, &ThemeSpec { color_by: "class".into(), ..Default::default() }).unwrap();
        assert_eq!(th.keys.iter().map(|k| k.text.as_str()).collect::<Vec<_>>(), labels);
        // Coloured by the names themselves, they come in the order of their numbers too.
        let th = prepare(&t, &style, &ThemeSpec { color_by: "class_label".into(), ..Default::default() }).unwrap();
        assert_eq!(th.keys.iter().map(|k| k.text.as_str()).collect::<Vec<_>>(), labels);
    }

    #[test]
    fn class_limits_can_be_even_numbers() {
        assert_eq!(round_breaks(&[1237.0, 2157.0, 3078.0, 3998.0, 4918.0]), vec![1000.0, 2000.0, 3000.0, 4000.0, 5000.0]);
        // Close limits keep apart, and in their place, by a finer step.
        assert_eq!(round_breaks(&[1237.0, 1300.0, 4000.0, 4918.0]), vec![1220.0, 1300.0, 4000.0, 4920.0]);
        assert_eq!(round_breaks(&[0.12, 0.31, 0.58]), vec![0.1, 0.3, 0.6]);
        let spec = ThemeSpec { color_by: "mag".into(), classes: 4, method: "equal".into(), round_breaks: true, histogram: true, ..Default::default() };
        let th = prepare(&table(&[1237.0, 1500.0, 2000.0, 2600.0, 3500.0, 4918.0]), &LayerStyle::default(), &spec).unwrap();
        assert_eq!(th.keys[0].text.replace('\u{a0}', " "), "1 000 – 2 000");
        assert_eq!(th.counts.iter().sum::<usize>(), 6);
    }

    #[test]
    fn two_attributes_make_nine_colours_and_dots_follow_a_value() {
        let mut t = table(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        t.fields.push(Field::new("other", FieldType::Real));
        for (i, f) in t.features.iter_mut().enumerate() {
            f.attrs.push(Value::Real(6.0 - i as f64));
        }
        let spec = ThemeSpec { color_by: "mag".into(), color_by2: "other".into(), ..Default::default() };
        let th = prepare(&t, &LayerStyle::default(), &spec).unwrap();
        let fills: Vec<[u8; 4]> = th.looks.unwrap().iter().map(|l| l.fill.unwrap()).collect();
        let b = th.bivariate.unwrap();
        // Low first and high second, then the other way round.
        assert_eq!(fills[0], b.colours[2 * 3]);
        assert_eq!(fills[5], b.colours[2]);
        let spec = ThemeSpec { dots_by: "mag".into(), dots_per: 2.0, ..Default::default() };
        let th = prepare(&t, &LayerStyle::default(), &spec).unwrap();
        assert_eq!(th.dots.unwrap(), vec![1, 1, 2, 2, 3, 3]);
        assert!(th.dot_text.starts_with("1 dot = 2"), "{}", th.dot_text);
    }

    #[test]
    fn natural_breaks_find_the_gaps_and_close_points_cluster() {
        let v = [1.0, 2.0, 3.0, 20.0, 21.0, 22.0, 50.0, 51.0];
        let b = natural_breaks(&v, 3);
        assert_eq!(b, [1.0, 3.0, 22.0, 51.0]);
        let pts = [(0.0, 0.0), (3.0, 0.0), (100.0, 100.0), (2.0, 2.0)];
        let groups = clusters(&pts, &[0, 1, 2, 3], 5.0);
        assert_eq!(groups, [vec![0, 1, 3], vec![2]]);
    }
}
