//! Maps that show values: the size of points after a number, the colour of points, lines
//! and areas after a number in classes or after a text, the order they are drawn in, and
//! points that lie close together drawn as one with their number.

use crate::mapdraw::LayerStyle;
use crate::model::{GbError, Result, Table, Value};

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
        }
    }
}

impl ThemeSpec {
    pub fn is_off(&self) -> bool {
        self.size_by.is_empty() && self.color_by.is_empty() && self.order.is_empty() && self.cluster <= 0.0
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

/// A layer made ready to draw its values.
#[derive(Debug, Clone, Default)]
pub struct Theme {
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
        for (l, v) in looks.iter_mut().zip(&vals) {
            l.size = v.map_or(smin, size_of);
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
        let colors: Vec<Option<[u8; 3]>> = if spec.ramp == "as_written" {
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
            }
            cs
        } else if numeric {
            let vals: Vec<Option<f64>> = table.features.iter().map(|f| number(&f.attrs[i])).collect();
            let mut sorted: Vec<f64> = vals.iter().flatten().copied().collect();
            sorted.sort_by(f64::total_cmp);
            let classes = spec.classes.clamp(1, 12);
            let breaks: Vec<f64> = if sorted.is_empty() {
                Vec::new()
            } else {
                match spec.method.as_str() {
                    "natural" => natural_breaks(&sorted, classes),
                    "quantile" => {
                        let mut b: Vec<f64> = (0..=classes).map(|k| sorted[((sorted.len() - 1) as f64 * k as f64 / classes as f64).round() as usize]).collect();
                        b.dedup_by(|a, b| a == b);
                        b
                    }
                    _ => {
                        let (lo, hi) = (sorted[0], sorted[sorted.len() - 1]);
                        (0..=classes).map(|k| lo + (hi - lo) * k as f64 / classes as f64).collect()
                    }
                }
            };
            let count = breaks.len().saturating_sub(1).max(1);
            let ramp = if RAMPS.iter().any(|r| r.0 == spec.ramp) { spec.ramp.as_str() } else { "blues" };
            let color_of_class = |c: usize| ramp_at(ramp, if count == 1 { 0.7 } else { 0.08 + 0.92 * c as f64 / (count - 1) as f64 });
            let class_of = |v: f64| breaks.windows(2).position(|w| v <= w[1]).unwrap_or(count - 1).min(count - 1);
            for c in 0..count {
                if breaks.len() >= 2 {
                    let text = if breaks[c] == breaks[c + 1] { short(breaks[c]) } else { format!("{} – {}", short(breaks[c]), short(breaks[c + 1])) };
                    keys.push((color_of_class(c), text));
                }
            }
            if breaks.len() == 1 {
                keys.push((color_of_class(0), short(breaks[0])));
            }
            vals.iter().map(|v| v.map(|v| color_of_class(if breaks.len() >= 2 { class_of(v) } else { 0 }))).collect()
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
            named.sort_by_key(|n| crate::nodes::alphabetical(n));
            let color_of = |t: &str| chosen.iter().position(|c| c == t).map(|k| CATEGORIES[k]);
            for n in &named {
                keys.push((color_of(n).unwrap_or(OTHER), n.clone()));
            }
            if counts.len() > chosen.len() {
                keys.push((OTHER, "Other".to_string()));
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
    Ok(theme)
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
    fn natural_breaks_find_the_gaps_and_close_points_cluster() {
        let v = [1.0, 2.0, 3.0, 20.0, 21.0, 22.0, 50.0, 51.0];
        let b = natural_breaks(&v, 3);
        assert_eq!(b, [1.0, 3.0, 22.0, 51.0]);
        let pts = [(0.0, 0.0), (3.0, 0.0), (100.0, 100.0), (2.0, 2.0)];
        let groups = clusters(&pts, &[0, 1, 2, 3], 5.0);
        assert_eq!(groups, [vec![0, 1, 3], vec![2]]);
    }
}
