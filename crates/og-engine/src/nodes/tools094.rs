//! Tools added in 0.94: coordinates read from text as people write them, the profile of a
//! surface along lines, and the volume between two surfaces.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::raster089::{geoms_in, grid_on};
use super::vector093::{lines_of, point_at};
use super::*;
use crate::model::{err, fmt_count, fmt_real, Grid};
use geo::{BoundingRect, Contains, Coord, Euclidean, Geometry, Length, LineString, Point};

// ---------- Parse coordinates (CoordinateParser) ----------

const ORDERS: &[(&str, &str)] = &[("auto", "Auto: by the letters, or as written below"), ("yx", "Latitude or north first"), ("xy", "Longitude or east first")];

const PARSE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attribute",
        label: "Coordinates in",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "Text such as 57°42'31\"N 11°58'25\"E, N 57° 42.520' E 11° 58.418', 57.7087, 11.9737 or 6400123 319456",
    },
    ParamSpec {
        key: "order",
        label: "Order",
        kind: ParamKind::Choice(ORDERS),
        default: "auto",
        hint: "Without letters such as N and E: in degrees, latitude first unless a value is over 90; in metres, the larger is north",
    },
    ParamSpec {
        key: "crs",
        label: "Coordinate system",
        kind: ParamKind::Crs,
        default: "EPSG:4326",
        hint: "Of the coordinates: EPSG:4326 for degrees, EPSG:3006 for SWEREF 99 TM",
    },
];

/// Which way a coordinate goes, when a letter says so.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Axis {
    North,
    East,
}

/// One coordinate as written: its value and, when a letter gave it, which way it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Part {
    value: f64,
    axis: Option<Axis>,
}

/// The numbers of a text with the mark after each: ° ' " or none. A decimal comma is read
/// as one when a digit follows it directly.
fn numbers_with_marks(s: &str, comma_decimal: bool) -> Vec<(f64, Option<char>, bool)> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let negative = i > 0 && chars[i - 1] == '-';
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < chars.len() && (chars[i] == '.' || (comma_decimal && chars[i] == ',')) && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().map(|c| if *c == ',' { '.' } else { *c }).collect();
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let mark = chars.get(j).copied().filter(|m| matches!(m, '°' | '\'' | '"'));
            if mark.is_some() {
                i = j + 1;
            }
            if let Ok(v) = text.parse::<f64>() {
                out.push((v, mark, negative));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Degrees, minutes and seconds put together into coordinates: a new one starts with a
/// number marked ° when the one before has its degrees, or with an unmarked number when it
/// has all three.
fn degrees_of(tokens: &[(f64, Option<char>, bool)]) -> std::result::Result<Vec<f64>, String> {
    let mut out: Vec<[Option<f64>; 3]> = Vec::new();
    let mut signs: Vec<bool> = Vec::new();
    for &(v, mark, negative) in tokens {
        let slot = match mark {
            Some('°') => 0,
            Some('\'') => 1,
            Some('"') => 2,
            _ => match out.last() {
                None => 0,
                Some(c) if c[0].is_some() && c[1].is_none() => 1,
                Some(c) if c[1].is_some() && c[2].is_none() => 2,
                _ => 0,
            },
        };
        let start_new = match out.last() {
            None => true,
            Some(c) => c[slot].is_some() || (slot == 0 && c[0].is_some()),
        };
        if start_new {
            out.push([None, None, None]);
            signs.push(negative);
        }
        out.last_mut().expect("pushed above")[slot] = Some(v);
    }
    out.iter()
        .zip(signs)
        .map(|(c, negative)| {
            let (d, m, s) = (c[0].unwrap_or(0.0), c[1].unwrap_or(0.0), c[2].unwrap_or(0.0));
            if m >= 60.0 || s >= 60.0 {
                return Err(format!("{} minutes or {} seconds is 60 or more", fmt_real(m), fmt_real(s)));
            }
            let v = d + m / 60.0 + s / 3600.0;
            Ok(if negative { -v } else { v })
        })
        .collect()
}

/// The coordinates written in a text, in the order written, each with the way a letter
/// says it goes. Two, or three where the last is a height.
fn parse_coordinates(text: &str) -> std::result::Result<Vec<Part>, String> {
    // The marks as typed by people and by programs.
    let mut s: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '′' | '’' | '‘' | '´' | '`' => '\'',
            '″' | '“' | '”' => '"',
            'º' | '˚' => '°',
            c => c,
        })
        .collect();
    s = s.replace("''", "\"");
    let upper = s.to_uppercase();
    if upper.is_empty() {
        return Err("empty".into());
    }
    // A letter for the way, before or after the numbers: N and S, E and W, and Ö and V as in
    // Swedish. Only a letter that stands alone, so that words are not read.
    let letters: Vec<(usize, char)> = upper
        .char_indices()
        .filter(|(i, c)| {
            matches!(c, 'N' | 'S' | 'E' | 'W' | 'Ö' | 'V') && {
                let before = upper[..*i].chars().next_back();
                let after = upper[i + c.len_utf8()..].chars().next();
                !before.is_some_and(|b| b.is_alphabetic()) && !after.is_some_and(|a| a.is_alphabetic())
            }
        })
        .collect();
    let axis_of = |c: char| if matches!(c, 'N' | 'S') { Axis::North } else { Axis::East };
    let negative_of = |c: char| matches!(c, 'S' | 'W' | 'V');
    if !letters.is_empty() {
        // Letters first, as N 57° 42.5', or last, as 57°42'31"N: the text is cut at them.
        let first_is_letter = letters[0].0 == 0;
        let mut chunks: Vec<(String, char)> = Vec::new();
        if first_is_letter {
            for (k, (at, c)) in letters.iter().enumerate() {
                let end = letters.get(k + 1).map_or(upper.len(), |l| l.0);
                chunks.push((upper[at + c.len_utf8()..end].to_string(), *c));
            }
        } else {
            let mut from = 0;
            for (at, c) in &letters {
                chunks.push((upper[from..*at].to_string(), *c));
                from = at + c.len_utf8();
            }
        }
        let mut parts = Vec::new();
        for (chunk, letter) in chunks {
            let tokens = numbers_with_marks(&chunk, true);
            let values = degrees_of(&tokens)?;
            let [v] = values[..] else {
                return Err(format!("\"{}\" is not one coordinate", chunk.trim()));
            };
            let v = if negative_of(letter) { -v.abs() } else { v };
            parts.push(Part { value: v, axis: Some(axis_of(letter)) });
        }
        return Ok(parts);
    }
    // Without letters: with marks, degrees and minutes; else numbers apart by a comma, a
    // semicolon or a space, with a decimal point, or a decimal comma when they are apart
    // otherwise.
    let marked = s.contains(['°', '\'', '"']);
    let comma_decimal = !s.contains('.') && (s.contains(';') || s.contains(' ') || s.matches(',').count() >= 2 || marked);
    let tokens = numbers_with_marks(&s, comma_decimal);
    let values = if marked { degrees_of(&tokens)? } else { tokens.iter().map(|t| if t.2 { -t.0 } else { t.0 }).collect() };
    Ok(values.into_iter().map(|value| Part { value, axis: None }).collect())
}

/// x, y and maybe a height from the parts as written.
fn place_of(parts: &[Part], order: &str, degrees: bool) -> std::result::Result<(f64, f64, Option<f64>), String> {
    let (two, z) = match parts {
        [a, b] => ([*a, *b], None),
        [a, b, c] => ([*a, *b], Some(c.value)),
        [_] => return Err("only one coordinate".into()),
        [] => return Err("no numbers".into()),
        _ => return Err(format!("{} numbers, not two", parts.len())),
    };
    let [a, b] = two;
    let (x, y) = match (a.axis, b.axis) {
        (Some(Axis::North), Some(Axis::East)) => (b.value, a.value),
        (Some(Axis::East), Some(Axis::North)) => (a.value, b.value),
        (Some(_), Some(_)) => return Err("both letters say the same way".into()),
        _ => match order {
            "xy" => (a.value, b.value),
            "yx" => (b.value, a.value),
            _ if degrees => {
                if a.value.abs() > 90.0 && b.value.abs() <= 90.0 {
                    (a.value, b.value)
                } else {
                    (b.value, a.value)
                }
            }
            _ => {
                if a.value >= b.value {
                    (b.value, a.value)
                } else {
                    (a.value, b.value)
                }
            }
        },
    };
    if degrees && (y.abs() > 90.0 || x.abs() > 180.0) {
        return Err(format!("{} and {} are not degrees of latitude and longitude", fmt_real(y), fmt_real(x)));
    }
    Ok((x, y, z))
}

fn run_parse(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let name = p.get("attribute").filter(|a| !a.trim().is_empty()).ok_or_else(|| GbError("Choose the attribute with the coordinates.".into()))?;
    let at = t.need_field(&name)?;
    let crs = p.crs("crs")?.or(Some(4326));
    let degrees = ctx.geographic(crs).unwrap_or(crs == Some(4326));
    let order = p.get("order").unwrap_or_else(|| "auto".into());
    let mut points = Table { fields: t.fields.clone(), crs, ..Default::default() };
    let (xi, yi) = (column(&mut points, "_x", FieldType::Real), column(&mut points, "_y", FieldType::Real));
    let mut failed = t.like();
    let error_at = column(&mut failed, "_error", FieldType::Text);
    let mut with_height = 0usize;
    for f in &t.features {
        let text = f.attrs[at].to_text();
        match parse_coordinates(&text).and_then(|parts| place_of(&parts, &order, degrees)) {
            Ok((x, y, z)) => {
                let mut attrs = f.attrs.clone();
                attrs.resize(points.fields.len(), Value::Null);
                attrs[xi] = Value::Real(x);
                attrs[yi] = Value::Real(y);
                let mut feature = Feature::new(attrs, Some(Geometry::Point(Point::new(x, y))));
                if let Some(z) = z {
                    feature.z = Some(std::sync::Arc::from(vec![z]));
                    with_height += 1;
                }
                points.features.push(feature);
            }
            Err(why) => {
                let mut attrs = f.attrs.clone();
                attrs.resize(failed.fields.len(), Value::Null);
                attrs[error_at] = Value::Text(format!("\"{}\": {}", text, why));
                failed.features.push(Feature::new(attrs, f.geom.clone()));
            }
        }
    }
    if !failed.is_empty() {
        ctx.notes.warn(format!("{} values could not be read as coordinates and went to Failed, with the reason in _error.", fmt_count(failed.len() as u64)));
    }
    if with_height > 0 {
        ctx.notes.info(format!("{} with a height as the third number", fmt_count(with_height as u64)));
    }
    ctx.notes.info(format!("{} points", fmt_count(points.len() as u64)));
    Ok(vec![("Points".into(), points), ("Failed".into(), failed)])
}

// ---------- Profile along line (ProfileMaker) ----------

const PROFILE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "step",
        label: "Every",
        kind: ParamKind::Text,
        default: "",
        hint: "Metres between the points of the profile. Empty: the size of a cell of the surface",
    },
    ParamSpec {
        key: "method",
        label: "Value",
        kind: ParamKind::Choice(&[("bilinear", "Between the four nearest cells"), ("cell", "Of the cell")]),
        default: "bilinear",
        hint: "",
    },
];

fn run_profile(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Lines");
    let g = grid_on(input(inp, "Surface"), "Surface")?;
    let step = number_param(p, "step", "Every")?.unwrap_or_else(|| g.geotransform[1].abs().max(g.geotransform[5].abs()));
    if step <= 0.0 {
        return err("The distance between the points of the profile must be more than 0.");
    }
    if ctx.geographic(t.crs) == Some(true) {
        return err("The lines are in degrees. Reproject them to a system in metres, such as SWEREF 99 TM, first.");
    }
    let smooth = p.get("method").as_deref() != Some("cell");
    let look = |x: f64, y: f64| if smooth { g.bilinear(x, y) } else { g.at(x, y) };
    // The places along each line, in the system of the lines, and the same places in that
    // of the surface, to look the heights up.
    struct Sample {
        row: usize,
        part: usize,
        d: f64,
        at: Coord<f64>,
    }
    let mut samples: Vec<Sample> = Vec::new();
    for (row, f) in t.features.iter().enumerate() {
        for (part, l) in f.geom.as_ref().map(lines_of).unwrap_or_default().iter().enumerate() {
            let len = Euclidean.length(l);
            let n = (len / step).floor() as usize;
            if n > 2_000_000 || samples.len() > 20_000_000 {
                return err(format!("A profile every {} m would have too many points. Choose a longer distance.", fmt_real(step)));
            }
            let mut ds: Vec<f64> = (0..=n).map(|k| k as f64 * step).collect();
            if ds.last().is_none_or(|d| (len - d).abs() > 1e-6) {
                ds.push(len);
            }
            for d in ds {
                if let Some((c, _)) = point_at(l, d) {
                    samples.push(Sample { row, part, d, at: c });
                }
            }
        }
    }
    let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = samples.iter().map(|s| (s.at.x, s.at.y)).unzip();
    if let (Some(from), Some(to)) = (t.crs, g.crs) {
        if from != to {
            ctx.backend("Reprojecting to the surface")?.transform(from, to, &mut xs, &mut ys)?;
        }
    }
    let heights: Vec<Option<f64>> = xs.iter().zip(&ys).map(|(x, y)| look(*x, *y).map(|v| v as f64)).collect();

    let mut points = t.like();
    let (line_at, dist_at, height_at) =
        (column(&mut points, "line", FieldType::Int), column(&mut points, "distance", FieldType::Real), column(&mut points, "height", FieldType::Real));
    let mut profiles = t.like();
    let stats: Vec<usize> = ["length", "length_3d", "lowest", "highest", "climb", "descent", "empty_points"]
        .iter()
        .map(|n| column(&mut profiles, n, if *n == "empty_points" { FieldType::Int } else { FieldType::Real }))
        .collect();
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let mut k = 0;
    let mut empty_total = 0usize;
    while k < samples.len() {
        let (row, part) = (samples[k].row, samples[k].part);
        let mut end = k;
        while end < samples.len() && samples[end].row == row && samples[end].part == part {
            end += 1;
        }
        let f = &t.features[row];
        let (mut coords, mut zs) = (Vec::new(), Vec::new());
        let (mut lo, mut hi, mut climb, mut descent, mut len3d, mut empty) = (f64::MAX, f64::MIN, 0.0, 0.0, 0.0, 0usize);
        let mut last: Option<(Coord<f64>, f64)> = None;
        for (s, h) in samples[k..end].iter().zip(&heights[k..end]) {
            let mut attrs = f.attrs.clone();
            attrs.resize(points.fields.len(), Value::Null);
            attrs[line_at] = Value::Int(row as i64 + 1);
            attrs[dist_at] = Value::Real(round(s.d));
            attrs[height_at] = h.map_or(Value::Null, |h| Value::Real(round(h)));
            let mut pt = Feature::new(attrs, Some(Geometry::Point(s.at.into())));
            match h {
                Some(h) => {
                    pt.z = Some(std::sync::Arc::from(vec![*h]));
                    lo = lo.min(*h);
                    hi = hi.max(*h);
                    if let Some((c, z)) = last {
                        let dz = h - z;
                        if dz > 0.0 {
                            climb += dz;
                        } else {
                            descent -= dz;
                        }
                        len3d += ((s.at.x - c.x).powi(2) + (s.at.y - c.y).powi(2) + dz * dz).sqrt();
                    }
                    last = Some((s.at, *h));
                    coords.push(s.at);
                    zs.push(*h);
                }
                None => empty += 1,
            }
            points.features.push(pt);
        }
        empty_total += empty;
        if coords.len() >= 2 {
            let mut attrs = f.attrs.clone();
            attrs.resize(profiles.fields.len(), Value::Null);
            let length = samples[end - 1].d;
            for (slot, v) in stats.iter().zip([length, len3d, lo, hi, climb, descent]) {
                attrs[*slot] = Value::Real(round(v));
            }
            attrs[stats[6]] = Value::Int(empty as i64);
            let mut line = Feature::new(attrs, Some(Geometry::LineString(LineString::from(coords))));
            line.z = Some(std::sync::Arc::from(zs));
            profiles.features.push(line);
        }
        k = end;
    }
    if empty_total > 0 {
        ctx.notes.warn(format!("{} points of the profiles lie outside the surface or on empty cells, and have no height.", fmt_count(empty_total as u64)));
    }
    ctx.notes.info(format!("{} profiles, {} points every {} m", fmt_count(profiles.len() as u64), fmt_count(points.len() as u64), fmt_real(step)));
    Ok(vec![("Profiles".into(), profiles), ("Points".into(), points)])
}

// ---------- Volume between surfaces (VolumeCalculator) ----------

const VOLUME_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "level",
    label: "Level",
    kind: ParamKind::Text,
    default: "0",
    hint: "The height to measure from when nothing is connected to Reference, such as the bottom of a pit or the level of a lake",
}];

/// The volumes of one area: above and below the reference, and how much ground was counted.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct Volumes {
    above: f64,
    below: f64,
    cells: usize,
    empty: usize,
}

fn run_volume(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let g = grid_on(input(inp, "Surface"), "Surface")?;
    if ctx.geographic(g.crs) == Some(true) {
        return err("The surface is in degrees, so its cells have no size in square metres. Reproject it to a system in metres first.");
    }
    let reference_in = input(inp, "Reference");
    let reference = if reference_in.grid.is_some() { Some(grid_on(reference_in, "Reference")?) } else { None };
    let level = number_param(p, "level", "Level")?.unwrap_or(0.0);
    if let (Some(r), Some(a), Some(b)) = (reference, reference.and_then(|r| r.crs), g.crs) {
        if a != b {
            return err(format!(
                "The reference is in {} and the surface in {}. Reproject one of them so that both are in the same system.",
                crate::model::crs_label(Some(a)),
                crate::model::crs_label(Some(b))
            ));
        }
        let _ = r;
    }
    let cell_area = (g.geotransform[1] * g.geotransform[5]).abs();
    // The difference, cell by cell: the surface less the reference at the middle of each
    // cell, so that a reference of other cells is followed between its own.
    let mut diff = vec![f32::NAN; g.width * g.height];
    for r in 0..g.height {
        for c in 0..g.width {
            let Some(top) = g.get(c, r) else { continue };
            let (x, y) = (g.geotransform[0] + (c as f64 + 0.5) * g.geotransform[1], g.geotransform[3] + (r as f64 + 0.5) * g.geotransform[5]);
            let base = match reference {
                Some(rg) => match rg.bilinear(x, y) {
                    Some(v) => v as f64,
                    None => continue,
                },
                None => level,
            };
            diff[r * g.width + c] = (top as f64 - base) as f32;
        }
    }
    let count = |cells: &mut dyn Iterator<Item = usize>| {
        let mut v = Volumes::default();
        for i in cells {
            let d = diff[i];
            if d.is_nan() {
                v.empty += 1;
                continue;
            }
            v.cells += 1;
            if d > 0.0 {
                v.above += d as f64 * cell_area;
            } else {
                v.below -= d as f64 * cell_area;
            }
        }
        v
    };
    let areas = input(inp, "Areas");
    let mut out = Table { crs: g.crs, ..Default::default() };
    let rows: Vec<(Vec<Value>, Option<Geometry<f64>>, Volumes)> = if areas.features.iter().any(|f| f.geom.is_some()) {
        out.fields = areas.fields.clone();
        let geoms = geoms_in(areas, g.crs, ctx)?;
        let mut rows = Vec::new();
        for (f, geom) in areas.features.iter().zip(geoms) {
            let Some(geom) = geom else { continue };
            let Some(b) = geom.bounding_rect() else { continue };
            // The cells whose middle lies in the area.
            let gt = &g.geotransform;
            let (c0, c1) = (((b.min().x - gt[0]) / gt[1]).floor().max(0.0) as usize, (((b.max().x - gt[0]) / gt[1]).ceil().max(0.0) as usize).min(g.width));
            let (r0, r1) = (((b.max().y - gt[3]) / gt[5]).floor().max(0.0) as usize, (((b.min().y - gt[3]) / gt[5]).ceil().max(0.0) as usize).min(g.height));
            let inside: Vec<usize> = (r0..r1)
                .flat_map(|r| (c0..c1).map(move |c| (r, c)))
                .filter(|(r, c)| geom.contains(&Point::new(gt[0] + (*c as f64 + 0.5) * gt[1], gt[3] + (*r as f64 + 0.5) * gt[5])))
                .map(|(r, c)| r * g.width + c)
                .collect();
            let v = count(&mut inside.into_iter());
            rows.push((f.attrs.clone(), Some(geom), v));
        }
        rows
    } else {
        let v = count(&mut (0..diff.len()));
        vec![(Vec::new(), Some(Geometry::Polygon(g.outline())), v)]
    };
    let slots: Vec<usize> = ["above_m3", "below_m3", "net_m3", "area_m2", "empty_m2"].iter().map(|n| column(&mut out, n, FieldType::Real)).collect();
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let (mut above, mut below) = (0.0, 0.0);
    for (mut attrs, geom, v) in rows {
        attrs.resize(out.fields.len(), Value::Null);
        for (slot, x) in slots.iter().zip([v.above, v.below, v.above - v.below, v.cells as f64 * cell_area, v.empty as f64 * cell_area]) {
            attrs[*slot] = Value::Real(round(x));
        }
        above += v.above;
        below += v.below;
        out.features.push(Feature::new(attrs, geom));
    }
    let empty: usize = out.features.iter().filter(|f| f.attrs[slots[4]].as_f64().is_some_and(|e| e > 0.0)).count();
    if empty > 0 {
        ctx.notes
            .warn(format!("{} areas reach over empty cells or beyond the surfaces; those cells are not counted, as empty_m2 tells.", fmt_count(empty as u64)));
    }
    ctx.notes.info(format!(
        "{} m³ above and {} m³ below {}",
        fmt_real(round(above)),
        fmt_real(round(below)),
        if reference.is_some() { "the reference".to_string() } else { format!("the level {}", fmt_real(level)) }
    ));
    let difference = Grid { values: diff, bands: Vec::new(), meta: Default::default(), ..g.clone() };
    Ok(vec![("Volumes".into(), out), ("Difference".into(), difference.into_table())])
}

// ---------- Change height system (HeightTransformer) ----------

/// The height systems to choose among: heights above the ellipsoid, or the EPSG code of a
/// vertical system.
const HEIGHT_SYSTEMS: &[(&str, &str)] = &[
    ("ellipsoid", "Above the ellipsoid, as from GNSS"),
    ("5613", "RH 2000, Sweden"),
    ("5718", "RH 70, Sweden"),
    ("5941", "NN2000, Norway"),
    ("3900", "N2000, Finland"),
    ("5799", "DVR90, Denmark"),
    ("7837", "DHHN2016, Germany"),
    ("5773", "EGM96, the world"),
    ("3855", "EGM2008, the world"),
    ("5703", "NAVD88, North America"),
    ("code", "Another, by its EPSG code"),
];

const HEIGHT_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "from", label: "From", kind: ParamKind::Choice(HEIGHT_SYSTEMS), default: "ellipsoid", hint: "The height system the heights are in now" },
    ParamSpec {
        key: "from_code",
        label: "From, EPSG code",
        kind: ParamKind::Text,
        default: "",
        hint: "With Another: the code of a vertical system, such as 5613",
    },
    ParamSpec { key: "to", label: "To", kind: ParamKind::Choice(HEIGHT_SYSTEMS), default: "5613", hint: "" },
    ParamSpec { key: "to_code", label: "To, EPSG code", kind: ParamKind::Text, default: "", hint: "With Another: the code of a vertical system" },
    ParamSpec {
        key: "fetch",
        label: "Fetch a missing grid",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "From cdn.proj.org, when the grid of the geoid that is needed is not among those that come with Ographis: Sweden, Norway and EGM96",
    },
];

fn height_system(p: &P, key: &str) -> Result<String> {
    let v = p.get(key).unwrap_or_else(|| "ellipsoid".into());
    if v != "code" {
        return Ok(v);
    }
    let code = p.get(&format!("{}_code", key)).unwrap_or_default();
    let code = code.trim().trim_start_matches("EPSG:").trim_start_matches("epsg:");
    match code.parse::<u32>() {
        Ok(c) => Ok(c.to_string()),
        Err(_) => {
            err(format!("Write the EPSG code of the height system under {}, EPSG code, such as 5613 for RH 2000.", if key == "from" { "From" } else { "To" }))
        }
    }
}

fn run_heights(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let (from, to) = (height_system(p, "from")?, height_system(p, "to")?);
    let fetch = p.flag("fetch");
    let mut out = t.clone();
    if from == to {
        ctx.notes.info("the same height system: nothing changed");
        return Ok(vec![("Output".into(), out)]);
    }
    let crs = t.crs.ok_or_else(|| GbError("The stream has no coordinate system, so the places of the heights are not known. Set it first.".into()))?;
    let b = ctx.backend("Changing height systems")?;
    let (mut changed, mut lost) = (0usize, 0usize);
    let mut count = |before: &[f64], after: &[f64]| {
        for (a, z) in before.iter().zip(after) {
            if a.is_finite() {
                if z.is_finite() {
                    changed += 1;
                } else {
                    lost += 1;
                }
            }
        }
    };
    // Features with heights: every coordinate.
    let (mut xs, mut ys, mut zs, mut owners) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, f) in t.features.iter().enumerate() {
        let (Some(g), Some(z)) = (f.geom.as_ref(), f.heights()) else { continue };
        for (c, h) in crate::geom::coords(g).into_iter().zip(z) {
            xs.push(c.x);
            ys.push(c.y);
            zs.push(*h);
        }
        owners.push((i, z.len()));
    }
    if !zs.is_empty() {
        let before = zs.clone();
        b.transform_heights(crs, &from, &to, &xs, &ys, &mut zs, fetch)?;
        count(&before, &zs);
        let mut at = 0;
        for (i, n) in owners {
            out.features[i].z = Some(std::sync::Arc::from(&zs[at..at + n]));
            at += n;
        }
    }
    // A point cloud.
    if let Some(c) = t.points.as_deref() {
        let mut c = c.clone();
        let before = c.z.clone();
        b.transform_heights(crs, &from, &to, &c.x, &c.y, &mut c.z, fetch)?;
        count(&before, &c.z);
        out.points = Some(std::sync::Arc::new(c));
    }
    // A grid of heights: at the middle of each cell.
    if let Some(g) = t.grid.as_deref() {
        let gt = g.geotransform;
        let mut cx = Vec::with_capacity(g.values.len());
        let mut cy = Vec::with_capacity(g.values.len());
        for r in 0..g.height {
            for c in 0..g.width {
                cx.push(gt[0] + (c as f64 + 0.5) * gt[1] + (r as f64 + 0.5) * gt[2]);
                cy.push(gt[3] + (c as f64 + 0.5) * gt[4] + (r as f64 + 0.5) * gt[5]);
            }
        }
        let mut z: Vec<f64> = g.values.iter().map(|v| *v as f64).collect();
        let before = z.clone();
        b.transform_heights(crs, &from, &to, &cx, &cy, &mut z, fetch)?;
        count(&before, &z);
        let values = z.iter().map(|v| *v as f32).collect();
        out.grid = Some(std::sync::Arc::new(Grid { values, ..g.clone() }));
    }
    // A 3D model with coordinates.
    if let Some(m) = t.mesh.as_deref() {
        let mut m = m.clone();
        let (mx, my): (Vec<f64>, Vec<f64>) = m.positions.iter().map(|p| (p[0], p[1])).unzip();
        let mut mz: Vec<f64> = m.positions.iter().map(|p| p[2]).collect();
        let before = mz.clone();
        b.transform_heights(crs, &from, &to, &mx, &my, &mut mz, fetch)?;
        count(&before, &mz);
        for (p, z) in m.positions.iter_mut().zip(mz) {
            p[2] = z;
        }
        out.mesh = Some(std::sync::Arc::new(m));
    }
    if changed + lost == 0 {
        ctx.notes.warn("The stream has no heights to change: no heights on its coordinates, no point cloud, grid of heights or 3D model.");
    }
    if lost > 0 {
        ctx.notes.warn(format!("{} heights lie outside the grid of the geoid and became empty.", fmt_count(lost as u64)));
    }
    let name = |v: &str| HEIGHT_SYSTEMS.iter().find(|h| h.0 == v).map_or(format!("EPSG:{}", v), |h| h.1.split(',').next().unwrap_or(h.1).to_string());
    ctx.notes.info(format!("{} heights from {} to {}", fmt_count(changed as u64), name(&from), name(&to)));
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "CoordinateParser",
        title: "Parse coordinates",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes points of coordinates written as text, as people and programs write them: degrees, minutes and seconds (57°42'31\"N 11°58'25\"E), degrees and minutes (N 57° 42.520' E 11° 58.418'), decimal degrees with a point or a comma, or metres such as SWEREF 99 TM. The letters N, S, E and W, and Ö and V, say which is which; a third number is a height. What cannot be read goes to Failed with the reason.",
        inputs: &["Input"],
        outputs: &["Points", "Failed"],
        ports: None,
        params: PARSE_PARAMS,
        run: run_parse,
        summary: |p| p.get("attribute").unwrap_or_else(|| "nothing chosen".into()),
    },
    NodeType {
        name: "ProfileMaker",
        title: "Profile along line",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "The height of a surface, such as a terrain model, along each line: a point every few metres with its distance along the line and its height, for a chart of the profile, and the line itself in 3D with its length over the ground, lowest and highest point, and how much it climbs and descends. A path, a road or a planned pipe.",
        inputs: &["Lines", "Surface"],
        outputs: &["Profiles", "Points"],
        ports: None,
        params: PROFILE_PARAMS,
        run: run_profile,
        summary: |p| match p.get("step").filter(|s| !s.trim().is_empty()) {
            Some(s) => format!("every {} m", s),
            None => "every cell".into(),
        },
    },
    NodeType {
        name: "VolumeCalculator",
        title: "Volume between surfaces",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "How much lies above and below: the volume between a surface and a reference surface, such as the ground before and after, or a level, such as the bottom of a pit. Cut and fill in cubic metres, for each area connected to Areas or for the whole surface, and the difference cell by cell as a raster.",
        inputs: &["Surface", "Reference", "Areas"],
        outputs: &["Volumes", "Difference"],
        ports: None,
        params: VOLUME_PARAMS,
        run: run_volume,
        summary: |p| format!("from {}", p.get("level").unwrap_or_else(|| "0".into())),
    },
    NodeType {
        name: "HeightTransformer",
        title: "Change height system",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Changes heights from one height system to another with the grid of the geoid: from heights above the ellipsoid, as GNSS and many point clouds give them, to heights above sea level in RH 2000, or back, or between others such as NN2000 and EGM2008. The heights of the coordinates, of a point cloud, of a grid of heights and of a 3D model. The places stay as they are; Reproject moves them.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: HEIGHT_PARAMS,
        run: run_heights,
        summary: |p| {
            let name = |k: &str| {
                let v = p.get(k).unwrap_or_default();
                if v == "code" {
                    format!("EPSG:{}", p.get(&format!("{}_code", k)).unwrap_or_default())
                } else {
                    HEIGHT_SYSTEMS.iter().find(|h| h.0 == v).map_or(v.clone(), |h| h.1.split(',').next().unwrap_or(h.1).to_string())
                }
            };
            format!("{} → {}", name("from"), name("to"))
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn xy(text: &str, order: &str, degrees: bool) -> (f64, f64) {
        let (x, y, _) = parse_coordinates(text).and_then(|p| place_of(&p, order, degrees)).unwrap_or_else(|e| panic!("{}: {}", text, e));
        ((x * 1e5).round() / 1e5, (y * 1e5).round() / 1e5)
    }

    #[test]
    fn coordinates_are_read_as_people_write_them() {
        let gbg = (11.97361, 57.70867);
        assert_eq!(xy("57°42'31.2\"N 11°58'25\"E", "auto", true), gbg);
        assert_eq!(xy("57° 42′ 31.2″ N, 11° 58′ 25″ E", "auto", true), gbg);
        assert_eq!(xy("N 57° 42.520' E 11° 58.41667'", "auto", true), gbg);
        assert_eq!(xy("E 11° 58.41667' N 57° 42.520'", "auto", true), gbg);
        assert_eq!(xy("57.70867, 11.97361", "auto", true), gbg);
        assert_eq!(xy("57,70867 11,97361", "auto", true), gbg);
        assert_eq!(xy("57,70867;11,97361", "auto", true), gbg);
        assert_eq!(xy("11.97361 57.70867", "xy", true), gbg);
        assert_eq!(xy("57°42'31.2\"N 11°58'25\"Ö", "auto", true), gbg);
        // West and south are negative; a longitude over 90 is known as one.
        assert_eq!(xy("33°52'S 151°12'E", "auto", true), (151.2, -33.86667));
        assert_eq!(xy("40.7128, -74.0060", "auto", true), (-74.006, 40.7128));
        // Both under 90 without letters: latitude first, unless the order says otherwise.
        assert_eq!(xy("-74.0060 40.7128", "xy", true), (-74.006, 40.7128));
        // SWEREF 99 TM: the larger is north, whichever comes first.
        assert_eq!(xy("6400123 319456", "auto", false), (319456.0, 6400123.0));
        assert_eq!(xy("319456, 6400123", "auto", false), (319456.0, 6400123.0));
        // A height as the third number.
        assert_eq!(parse_coordinates("57.7 11.9 42.5").and_then(|p| place_of(&p, "auto", true)).unwrap().2, Some(42.5));
    }

    #[test]
    fn what_is_not_coordinates_is_told() {
        let bad = |text: &str| parse_coordinates(text).and_then(|p| place_of(&p, "auto", true)).unwrap_err();
        assert!(bad("").contains("empty"));
        assert!(bad("57.7").contains("one coordinate"));
        assert!(bad("57°75'N 11°E").contains("60 or more"));
        assert!(bad("57°N 11°N").contains("same way"));
        assert!(bad("Göteborg").contains("no numbers"));
        assert!(bad("100 200").contains("not degrees"));
    }

    fn slope() -> Grid {
        // 10 × 10 cells of 10 m, rising 1 m a cell to the east, from 100.
        let mut g = Grid::blank(10, 10, [0.0, 10.0, 0.0, 100.0, 0.0, -10.0], Some(3006));
        for r in 0..10 {
            for c in 0..10 {
                g.values[r * 10 + c] = 100.0 + c as f32;
            }
        }
        g
    }

    #[test]
    fn a_profile_follows_the_surface_along_the_line() {
        let line = Table {
            fields: vec![Field::new("name", FieldType::Text)],
            crs: Some(3006),
            features: vec![Feature::new(vec![Value::Text("path".into())], Some(Geometry::LineString(LineString::from(vec![(5.0, 50.0), (95.0, 50.0)]))))],
            ..Default::default()
        };
        let (out, _) = run_for_test("ProfileMaker", &[("step", "10"), ("method", "cell")], vec![("Lines", line), ("Surface", slope().into_table())]).unwrap();
        let profiles = &out[0].1;
        let points = &out[1].1;
        assert_eq!(points.len(), 10, "every 10 m over 90 m, and the end");
        let h: Vec<f64> = points.features.iter().map(|f| f.attrs[points.field_index("height").unwrap()].as_f64().unwrap()).collect();
        assert_eq!(h, [100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0]);
        let get = |n: &str| profiles.features[0].attrs[profiles.field_index(n).unwrap()].as_f64().unwrap();
        assert_eq!((get("length"), get("lowest"), get("highest"), get("climb"), get("descent")), (90.0, 100.0, 109.0, 9.0, 0.0));
        assert!(get("length_3d") > 90.0 && get("length_3d") < 90.5);
        assert_eq!(profiles.features[0].heights().unwrap().len(), 10);
        assert_eq!(profiles.features[0].attrs[0], Value::Text("path".into()), "the attributes of the line are kept");
    }

    #[test]
    fn volumes_above_and_below_a_level_and_a_reference() {
        // From the level 104.5: cells of 105 to 109 above, 100 to 104 below, each 100 m².
        let (out, _) = run_for_test("VolumeCalculator", &[("level", "104.5")], vec![("Surface", slope().into_table())]).unwrap();
        let v = &out[0].1;
        let get = |t: &Table, row: usize, n: &str| t.features[row].attrs[t.field_index(n).unwrap()].as_f64().unwrap();
        // Each row: 0.5 + 1.5 + 2.5 + 3.5 + 4.5 = 12.5 m of height on each side, over 10 rows.
        assert_eq!((get(v, 0, "above_m3"), get(v, 0, "below_m3"), get(v, 0, "net_m3"), get(v, 0, "area_m2")), (12500.0, 12500.0, 0.0, 10000.0));
        // Against a flat reference at 100, within an area over the western half.
        let mut flat = slope();
        flat.values.iter_mut().for_each(|v| *v = 100.0);
        let area = Table {
            fields: vec![Field::new("name", FieldType::Text)],
            crs: Some(3006),
            features: vec![Feature::new(vec![Value::Text("west".into())], Some(Geometry::Polygon(geo::Rect::new((0.0, 0.0), (50.0, 100.0)).to_polygon())))],
            ..Default::default()
        };
        let (out, _) =
            run_for_test("VolumeCalculator", &[], vec![("Surface", slope().into_table()), ("Reference", flat.into_table()), ("Areas", area)]).unwrap();
        let v = &out[0].1;
        // Columns 0 to 4: 0 + 1 + 2 + 3 + 4 = 10 m a row, 10 rows, 100 m² each.
        assert_eq!((get(v, 0, "above_m3"), get(v, 0, "below_m3"), get(v, 0, "area_m2")), (10000.0, 0.0, 5000.0));
        assert_eq!(v.features[0].attrs[0], Value::Text("west".into()));
        let diff = out[1].1.grid.as_ref().unwrap();
        assert_eq!(diff.values[9], 9.0);
    }
}
