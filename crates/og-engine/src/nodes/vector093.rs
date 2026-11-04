//! Tools for vector data added in 0.93: overlaps and gaps between areas, slivers merged into
//! their neighbours, lines merged into one, points along lines, lines moved to the side,
//! linear referencing, and tables turned from long to wide and back.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, infer_type};
use geo::{Area, BooleanOps, BoundingRect, Coord, Euclidean, Geometry, Length, LineString, MultiLineString, MultiPolygon, Polygon};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{RTree, RTreeObject};
use std::collections::HashMap;

/// The lines of a geometry, each part of a multi line on its own.
pub(super) fn lines_of(g: &Geometry<f64>) -> Vec<LineString<f64>> {
    match g {
        Geometry::LineString(l) => vec![l.clone()],
        Geometry::MultiLineString(m) => m.0.clone(),
        Geometry::Line(l) => vec![LineString::from(vec![l.start, l.end])],
        Geometry::GeometryCollection(c) => c.0.iter().flat_map(lines_of).collect(),
        _ => Vec::new(),
    }
}

fn multi_area(g: &Geometry<f64>) -> Option<MultiPolygon<f64>> {
    let p = geom::polygons_of(g);
    (!p.is_empty()).then(|| MultiPolygon::new(p))
}

fn perimeter(m: &MultiPolygon<f64>) -> f64 {
    m.0.iter().map(|p| Euclidean.length(p.exterior()) + p.interiors().iter().map(|r| Euclidean.length(r)).sum::<f64>()).sum()
}

fn rect_of(m: &MultiPolygon<f64>, grow: f64) -> Option<Rectangle<[f64; 2]>> {
    let r = m.bounding_rect()?;
    Some(Rectangle::from_corners([r.min().x - grow, r.min().y - grow], [r.max().x + grow, r.max().y + grow]))
}

/// What names a feature in a report: the value of the attribute chosen, or its number.
fn name_of(t: &Table, at: Option<usize>, i: usize) -> Value {
    match at {
        Some(a) => t.features[i].attrs[a].clone(),
        None => Value::Int(i as i64 + 1),
    }
}

// ---------- Check topology (TopologyChecker) ----------

const TOPO_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "id",
        label: "Name the areas by",
        kind: ParamKind::Attribute("Areas"),
        default: "",
        hint: "An attribute such as a property id. Empty: their numbers from 1",
    },
    ParamSpec {
        key: "smallest",
        label: "Leave out smaller than",
        kind: ParamKind::Text,
        default: "0",
        hint: "Overlaps and gaps of less area than this are not reported: the noise of coordinates",
    },
    ParamSpec {
        key: "largest_gap",
        label: "Gaps up to",
        kind: ParamKind::Text,
        default: "",
        hint: "Holes larger than this are taken to be meant, such as a lake. Empty: every hole is a gap",
    },
];

fn run_topology(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Areas");
    let id_at = match p.get("id").filter(|s| !s.trim().is_empty()) {
        Some(n) => Some(t.need_field(&n)?),
        None => None,
    };
    let smallest = number_param(p, "smallest", "Leave out smaller than")?.unwrap_or(0.0).max(0.0);
    let largest = number_param(p, "largest_gap", "Gaps up to")?;
    let areas: Vec<Option<MultiPolygon<f64>>> = t.features.iter().map(|f| f.geom.as_ref().and_then(multi_area)).collect();
    let tree = RTree::bulk_load(areas.iter().enumerate().filter_map(|(i, a)| Some(GeomWithData::new(rect_of(a.as_ref()?, 0.0)?, i))).collect());
    let id_ty = id_at.map_or(FieldType::Int, |a| t.fields[a].ty);
    let mut overlaps = Table {
        fields: vec![Field::new("area_a", id_ty), Field::new("area_b", id_ty), Field::new("overlap", FieldType::Real)],
        crs: t.crs,
        ..Default::default()
    };
    let mut count = vec![0i64; t.len()];
    let gate = ctx.gate;
    for (i, a) in areas.iter().enumerate() {
        let Some(a) = a else { continue };
        if i % 500 == 0 && !gate.going() {
            return err(crate::model::STOPPED);
        }
        let r = rect_of(a, 0.0).expect("an area");
        let env = r.envelope();
        for c in tree.locate_in_envelope_intersecting(&env) {
            let j = c.data;
            if j <= i {
                continue;
            }
            let b = areas[j].as_ref().expect("an area");
            let both = a.intersection(b);
            let size = both.unsigned_area();
            if size > smallest && size > 0.0 {
                count[i] += 1;
                count[j] += 1;
                overlaps.features.push(Feature::new(
                    vec![name_of(t, id_at, i), name_of(t, id_at, j), Value::Real((size * 100.0).round() / 100.0)],
                    Some(Geometry::MultiPolygon(both)),
                ));
            }
        }
    }
    // Gaps: the holes in all the areas put together, or with a boundary, all of it that no
    // area covers, also gaps that open to the outside.
    let all: Vec<Polygon<f64>> = areas.iter().flatten().flat_map(|m| m.0.iter().cloned()).collect();
    let union = geo::unary_union(all.iter());
    let mut gaps = Table { fields: vec![Field::new("gap", FieldType::Real)], crs: t.crs, ..Default::default() };
    let boundary_in = input(inp, "Boundary");
    let boundary: Option<MultiPolygon<f64>> = if boundary_in.features.iter().any(|f| f.geom.is_some()) {
        let b = match (boundary_in.crs, t.crs) {
            (Some(a), Some(c)) if a != c => reproject(boundary_in, c, ctx.backend)?,
            _ => boundary_in.clone(),
        };
        let parts: Vec<Polygon<f64>> = b.features.iter().filter_map(|f| f.geom.as_ref().and_then(multi_area)).flat_map(|m| m.0).collect();
        (!parts.is_empty()).then(|| geo::unary_union(parts.iter()))
    } else {
        None
    };
    let mut push_gap = |g: Polygon<f64>| {
        let size = g.unsigned_area();
        if size > smallest && largest.is_none_or(|l| size <= l) {
            gaps.features.push(Feature::new(vec![Value::Real((size * 100.0).round() / 100.0)], Some(Geometry::Polygon(g))));
        }
    };
    match &boundary {
        Some(b) => {
            for g in b.difference(&union).0 {
                push_gap(g);
            }
        }
        None => {
            for p in &union.0 {
                for hole in p.interiors() {
                    push_gap(Polygon::new(hole.clone(), vec![]));
                }
            }
        }
    }
    // Areas that reach out beyond the boundary.
    let mut beyond = Table { fields: vec![Field::new("area", id_ty), Field::new("beyond", FieldType::Real)], crs: t.crs, ..Default::default() };
    if let Some(b) = &boundary {
        for (i, a) in areas.iter().enumerate() {
            let Some(a) = a else { continue };
            let out = a.difference(b);
            let size = out.unsigned_area();
            if size > smallest && size > 0.0 {
                beyond.features.push(Feature::new(vec![name_of(t, id_at, i), Value::Real((size * 100.0).round() / 100.0)], Some(Geometry::MultiPolygon(out))));
            }
        }
    }
    let mut out = t.clone();
    let slot = column(&mut out, "overlaps", FieldType::Int);
    for (f, n) in out.features.iter_mut().zip(&count) {
        f.attrs[slot] = Value::Int(*n);
    }
    let not_areas = areas.iter().filter(|a| a.is_none()).count();
    if not_areas > 0 {
        ctx.notes.warn(format!("{} features are not areas and were not checked.", fmt_count(not_areas as u64)));
    }
    if boundary.is_some() {
        ctx.notes.info(format!(
            "{} overlaps, {} gaps within the boundary and {} areas reaching beyond it",
            fmt_count(overlaps.len() as u64),
            fmt_count(gaps.len() as u64),
            fmt_count(beyond.len() as u64)
        ));
    } else {
        ctx.notes.info(format!("{} overlaps and {} gaps", fmt_count(overlaps.len() as u64), fmt_count(gaps.len() as u64)));
    }
    Ok(vec![("Areas".into(), out), ("Overlaps".into(), overlaps), ("Gaps".into(), gaps), ("Beyond".into(), beyond)])
}

// ---------- Eliminate slivers (SliverEliminator) ----------

const INTO: &[(&str, &str)] = &[("border", "The neighbour it shares the longest border with"), ("largest", "The largest neighbour")];

const SLIVER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "area",
        label: "Smaller than",
        kind: ParamKind::Text,
        default: "",
        hint: "Areas of less than this are slivers, in square units of the coordinate system",
    },
    ParamSpec {
        key: "thin",
        label: "Or thinner than",
        kind: ParamKind::Text,
        default: "",
        hint: "From 0 to 1: how round an area is, 1 for a circle. A long thin strip is below 0.1",
    },
    ParamSpec { key: "into", label: "Merged into", kind: ParamKind::Choice(INTO), default: "border", hint: "It keeps the attributes of that one" },
];

fn run_slivers(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Areas");
    let small = number_param(p, "area", "Smaller than")?;
    let thin = number_param(p, "thin", "Or thinner than")?;
    if small.is_none() && thin.is_none() {
        return err("Say what a sliver is: smaller than an area, thinner than a roundness, or both.");
    }
    let by_border = p.get("into").as_deref() != Some("largest");
    let mut areas: Vec<Option<MultiPolygon<f64>>> = t.features.iter().map(|f| f.geom.as_ref().and_then(multi_area)).collect();
    let is_sliver = |m: &MultiPolygon<f64>| {
        let a = m.unsigned_area();
        let round = 4.0 * std::f64::consts::PI * a / perimeter(m).powi(2).max(1e-12);
        small.is_some_and(|s| a < s) || thin.is_some_and(|x| round < x)
    };
    let sliver: Vec<bool> = areas.iter().map(|a| a.as_ref().is_some_and(is_sliver)).collect();
    let grow = 1e-6 * t.features.iter().filter_map(|f| f.geom.as_ref()?.bounding_rect()).map(|r| r.width().max(r.height())).fold(0.0, f64::max).max(1.0);
    let tree = RTree::bulk_load(areas.iter().enumerate().filter_map(|(i, a)| Some(GeomWithData::new(rect_of(a.as_ref()?, grow)?, i))).collect());
    // Where each area has gone: itself, or the area it was merged into.
    let mut owner: Vec<usize> = (0..areas.len()).collect();
    fn find(owner: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while owner[r] != r {
            r = owner[r];
        }
        owner[i] = r;
        r
    }
    let mut order: Vec<usize> = (0..areas.len()).filter(|i| sliver[*i]).collect();
    order.sort_by(|a, b| areas[*a].as_ref().map(|m| m.unsigned_area()).unwrap_or(0.0).total_cmp(&areas[*b].as_ref().map(|m| m.unsigned_area()).unwrap_or(0.0)));
    let (mut merged, mut alone) = (0usize, Vec::new());
    for i in order {
        let Some(s) = areas[i].clone() else { continue };
        let env = rect_of(&s, grow).expect("an area").envelope();
        let mut seen: Vec<usize> = Vec::new();
        let mut best: Option<(bool, f64, usize, MultiPolygon<f64>)> = None;
        for c in tree.locate_in_envelope_intersecting(&env) {
            let j = find(&mut owner, c.data);
            if j == i || seen.contains(&j) {
                continue;
            }
            seen.push(j);
            let Some(n) = &areas[j] else { continue };
            let u = s.union(n);
            let shared = (perimeter(&s) + perimeter(n) - perimeter(&u)) / 2.0;
            if shared <= grow {
                continue;
            }
            // Into an area that is not a sliver itself, when there is one.
            let score = if by_border { shared } else { n.unsigned_area() };
            let rank = (!sliver[j], score);
            if best.as_ref().is_none_or(|b| (rank.0, rank.1) > (b.0, b.1)) {
                best = Some((rank.0, rank.1, j, u));
            }
        }
        match best {
            Some((_, _, j, u)) => {
                areas[j] = Some(u);
                areas[i] = None;
                owner[i] = j;
                merged += 1;
            }
            None => alone.push(i),
        }
    }
    let mut out = t.like();
    let mut left = t.like();
    for (i, f) in t.features.iter().enumerate() {
        if alone.contains(&i) {
            left.features.push(f.clone());
        } else if owner[i] == i {
            let mut f = f.clone();
            if let Some(a) = &areas[i] {
                f.geom = Some(if a.0.len() == 1 { Geometry::Polygon(a.0[0].clone()) } else { Geometry::MultiPolygon(a.clone()) });
                f.z = None;
            }
            out.features.push(f);
        }
    }
    ctx.notes.info(format!("{} slivers merged into their neighbours", fmt_count(merged as u64)));
    if !alone.is_empty() {
        ctx.notes.warn(format!("{} slivers have no neighbour to merge into, and went to Left.", fmt_count(alone.len() as u64)));
    }
    Ok(vec![("Areas".into(), out), ("Left".into(), left)])
}

// ---------- Merge lines (LineMerger) ----------

const MERGE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "group_by",
        label: "Group by",
        kind: ParamKind::Attributes("Lines"),
        default: "",
        hint: "Only lines with the same values are merged, such as the name of a street. Empty merges all",
    },
    ParamSpec {
        key: "tolerance",
        label: "Ends within",
        kind: ParamKind::Text,
        default: "0",
        hint: "Ends this close count as the same point. 0: only ends that are equal",
    },
];

/// Lines merged where exactly two of them meet. Each comes back as one line, with how many
/// were merged into it.
pub(super) fn merge_lines(lines: Vec<LineString<f64>>, tolerance: f64) -> Vec<(LineString<f64>, usize)> {
    let key = |c: Coord<f64>| -> (i64, i64) {
        let s = if tolerance > 0.0 { tolerance } else { 1e-9 };
        ((c.x / s).round() as i64, (c.y / s).round() as i64)
    };
    let lines: Vec<LineString<f64>> = lines.into_iter().filter(|l| l.0.len() >= 2).collect();
    let mut at: HashMap<(i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (i, l) in lines.iter().enumerate() {
        at.entry(key(l.0[0])).or_default().push((i, true));
        at.entry(key(*l.0.last().expect("points"))).or_default().push((i, false));
    }
    let mut used = vec![false; lines.len()];
    let mut out = Vec::new();
    // Chains start at ends where a line does not go on, then what is left are rings.
    let starts: Vec<usize> = (0..lines.len())
        .flat_map(|i| [(i, key(lines[i].0[0])), (i, key(*lines[i].0.last().expect("points")))])
        .filter(|(_, k)| at[k].len() != 2)
        .map(|(i, _)| i)
        .chain(0..lines.len())
        .collect();
    for s in starts {
        if used[s] {
            continue;
        }
        used[s] = true;
        // Walk from the end of the first line that does not go on, so that it is followed
        // forwards.
        let mut coords = lines[s].0.clone();
        if at[&key(coords[coords.len() - 1])].len() != 2 && at[&key(coords[0])].len() == 2 {
            coords.reverse();
        }
        let mut n = 1;
        for _ in 0..2 {
            loop {
                let end = key(*coords.last().expect("points"));
                let here = &at[&end];
                if here.len() != 2 {
                    break;
                }
                let Some(&(j, from_start)) = here.iter().find(|(j, _)| !used[*j]) else { break };
                used[j] = true;
                n += 1;
                let mut next = lines[j].0.clone();
                if !from_start {
                    next.reverse();
                }
                coords.extend(next.into_iter().skip(1));
            }
            // Then the other way, from the start.
            coords.reverse();
        }
        out.push((LineString::new(coords), n));
    }
    out
}

fn run_merge_lines(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Lines");
    let by = p.list("group_by");
    let idx: Vec<usize> = by.iter().map(|n| t.need_field(n)).collect::<Result<_>>()?;
    let tolerance = number_param(p, "tolerance", "Ends within")?.unwrap_or(0.0).max(0.0);
    let mut groups: Vec<(Vec<Value>, Vec<LineString<f64>>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut other = t.like();
    for f in &t.features {
        let parts = f.geom.as_ref().map(lines_of).unwrap_or_default();
        if parts.is_empty() {
            other.features.push(f.clone());
            continue;
        }
        let vals: Vec<Value> = idx.iter().map(|i| f.attrs[*i].clone()).collect();
        let k = format!("{:?}", vals);
        let g = *index.entry(k).or_insert_with(|| {
            groups.push((vals, Vec::new()));
            groups.len() - 1
        });
        groups[g].1.extend(parts);
    }
    let mut fields: Vec<Field> = idx.iter().map(|i| t.fields[*i].clone()).collect();
    fields.push(Field::new("merged", FieldType::Int));
    let mut out = Table { fields, crs: t.crs, ..Default::default() };
    let before: usize = groups.iter().map(|g| g.1.len()).sum();
    for (vals, lines) in groups {
        for (l, n) in merge_lines(lines, tolerance) {
            let mut attrs = vals.clone();
            attrs.push(Value::Int(n as i64));
            out.features.push(Feature::new(attrs, Some(Geometry::LineString(l))));
        }
    }
    ctx.notes.info(format!("{} lines merged into {}", fmt_count(before as u64), fmt_count(out.len() as u64)));
    Ok(vec![("Merged".into(), out), ("Other".into(), other)])
}

// ---------- Points along lines (PointsAlongLines) ----------

const ALONG_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "distance", label: "Every", kind: ParamKind::Text, default: "100", hint: "A point at this distance along each line, from its start" },
    ParamSpec { key: "start", label: "First at", kind: ParamKind::Text, default: "0", hint: "The distance from the start of the first point" },
    ParamSpec { key: "end", label: "A point at the end", kind: ParamKind::Choice(YES_NO), default: "true", hint: "Also where the line ends" },
];

/// The point at a distance along a line, and the direction there in degrees clockwise from
/// north.
pub(super) fn point_at(l: &LineString<f64>, d: f64) -> Option<(Coord<f64>, f64)> {
    let mut walked = 0.0;
    for w in l.0.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        if len > 0.0 && walked + len >= d - 1e-9 {
            let k = ((d - walked) / len).clamp(0.0, 1.0);
            let dir = (b.x - a.x).atan2(b.y - a.y).to_degrees().rem_euclid(360.0);
            return Some((Coord { x: a.x + k * (b.x - a.x), y: a.y + k * (b.y - a.y) }, dir));
        }
        walked += len;
    }
    None
}

/// The part of a line from one distance along it to another.
pub(super) fn part_of(l: &LineString<f64>, from: f64, to: f64) -> Option<LineString<f64>> {
    let (from, to) = (from.min(to), from.max(to));
    let mut out: Vec<Coord<f64>> = Vec::new();
    let mut walked = 0.0;
    for w in l.0.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        let at = |d: f64| {
            let k = if len > 0.0 { ((d - walked) / len).clamp(0.0, 1.0) } else { 0.0 };
            Coord { x: a.x + k * (b.x - a.x), y: a.y + k * (b.y - a.y) }
        };
        if walked + len >= from && walked <= to {
            if out.is_empty() {
                out.push(at(from.max(walked)));
            }
            out.push(at(to.min(walked + len)));
        }
        walked += len;
    }
    out.dedup();
    (out.len() >= 2).then(|| LineString::new(out))
}

fn run_along(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Lines");
    let every = number_param(p, "distance", "Every")?.unwrap_or(100.0);
    if every <= 0.0 {
        return err("The distance between the points must be more than 0.");
    }
    let start = number_param(p, "start", "First at")?.unwrap_or(0.0).max(0.0);
    let end = p.get("end").as_deref() != Some("false");
    if ctx.geographic(t.crs) == Some(true) {
        ctx.notes.warn("The lines are in degrees, so the distance is in degrees too. Reproject them first to have it in metres.");
    }
    let mut out = t.like();
    out.fields.push(Field::new("line", FieldType::Int));
    out.fields.push(Field::new("distance", FieldType::Real));
    out.fields.push(Field::new("direction", FieldType::Real));
    for (row, f) in t.features.iter().enumerate() {
        for l in f.geom.as_ref().map(lines_of).unwrap_or_default() {
            let len = Euclidean.length(&l);
            let mut ds: Vec<f64> = Vec::new();
            let mut d = start;
            while d <= len + 1e-9 && ds.len() < 1_000_000 {
                ds.push(d);
                d += every;
            }
            if end && ds.last().is_none_or(|x| (len - x).abs() > 1e-6) {
                ds.push(len);
            }
            for d in ds {
                let Some((c, dir)) = point_at(&l, d) else { continue };
                let mut attrs = f.attrs.clone();
                attrs.extend([Value::Int(row as i64 + 1), Value::Real((d * 1000.0).round() / 1000.0), Value::Real((dir * 10.0).round() / 10.0)]);
                out.features.push(Feature::new(attrs, Some(Geometry::Point(c.into()))));
            }
        }
    }
    ctx.notes.info(format!("{} points", fmt_count(out.len() as u64)));
    Ok(vec![("Points".into(), out)])
}

// ---------- Offset lines (LineOffsetter) ----------

const OFFSET_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "distance",
    label: "Distance",
    kind: ParamKind::Expression("Lines"),
    default: "5",
    hint: "To the left of the direction of the line; a negative number to the right. Or an attribute, such as half the width of a road",
}];

/// A line moved to the side by `d`, left of its direction: each segment moved, and the
/// corners where they meet, cut off where they would reach far out.
pub(super) fn offset_line(l: &LineString<f64>, d: f64) -> Option<LineString<f64>> {
    let mut pts: Vec<Coord<f64>> = l.0.clone();
    pts.dedup();
    if pts.len() < 2 {
        return None;
    }
    let closed = pts.len() > 3 && pts[0] == pts[pts.len() - 1];
    let normal = |a: Coord<f64>, b: Coord<f64>| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        Coord { x: -dy / len * d, y: dx / len * d }
    };
    let segs: Vec<(Coord<f64>, Coord<f64>)> = pts
        .windows(2)
        .map(|w| {
            let n = normal(w[0], w[1]);
            (w[0] + n, w[1] + n)
        })
        .collect();
    let meet = |s: (Coord<f64>, Coord<f64>), t: (Coord<f64>, Coord<f64>), corner: Coord<f64>| -> Vec<Coord<f64>> {
        let (r, q) = (s.1 - s.0, t.1 - t.0);
        let cross = r.x * q.y - r.y * q.x;
        if cross.abs() < 1e-12 {
            return vec![s.1];
        }
        let k = ((t.0.x - s.0.x) * q.y - (t.0.y - s.0.y) * q.x) / cross;
        let p = s.0 + r * k;
        // A sharp corner is cut off, rather than reaching far out.
        if ((p.x - corner.x).powi(2) + (p.y - corner.y).powi(2)).sqrt() > 4.0 * d.abs() {
            vec![s.1, t.0]
        } else {
            vec![p]
        }
    };
    let mut out = Vec::new();
    if closed {
        out.extend(meet(segs[segs.len() - 1], segs[0], pts[0]));
    } else {
        out.push(segs[0].0);
    }
    for i in 0..segs.len() - 1 {
        out.extend(meet(segs[i], segs[i + 1], pts[i + 1]));
    }
    if closed {
        let first = out[0];
        out.push(first);
    } else {
        out.push(segs[segs.len() - 1].1);
    }
    Some(LineString::new(out))
}

fn run_offset(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Lines");
    let text = p.get("distance").unwrap_or_else(|| "5".into());
    let e = crate::expr::compile(&text, &t.field_names()).map_err(|e| GbError(format!("Distance: {}", e.0)))?;
    let geographic = ctx.geographic(t.crs);
    let mut out = t.like();
    let mut other = t.like();
    for (i, f) in t.features.iter().enumerate() {
        let d = e.eval(&crate::expr::Row { feature: f, index: i, geographic })?.as_f64();
        let lines = f.geom.as_ref().map(lines_of).unwrap_or_default();
        let (Some(d), false) = (d, lines.is_empty()) else {
            other.features.push(f.clone());
            continue;
        };
        let moved: Vec<LineString<f64>> = lines.iter().filter_map(|l| offset_line(l, d)).collect();
        let mut g = f.clone();
        g.z = None;
        g.geom = Some(if moved.len() == 1 { Geometry::LineString(moved[0].clone()) } else { Geometry::MultiLineString(MultiLineString::new(moved)) });
        out.features.push(g);
    }
    if !other.is_empty() {
        ctx.notes.warn(format!("{} features are not lines or have no distance, and went to Other.", fmt_count(other.len() as u64)));
    }
    Ok(vec![("Lines".into(), out), ("Other".into(), other)])
}

// ---------- Locate along lines (LineLocator) ----------

const MEASURES: &[(&str, &str)] = &[("metres", "Metres from the start"), ("share", "A share from 0 at the start to 1 at the end")];

const LOCATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "key",
        label: "Line key",
        kind: ParamKind::Attribute("Lines"),
        default: "",
        hint: "Such as the id of a road, which each point gets. Empty: the number of the line",
    },
    ParamSpec { key: "within", label: "Within", kind: ParamKind::Text, default: "50", hint: "Points further from every line are left out" },
    ParamSpec { key: "measure", label: "Measure as", kind: ParamKind::Choice(MEASURES), default: "metres", hint: "" },
];

/// Where a point lies along a line: the distance from its start, the distance to it, and
/// on which side, + left of its direction.
fn locate(l: &LineString<f64>, c: Coord<f64>) -> (f64, f64) {
    let (mut best, mut at, mut walked) = (f64::INFINITY, 0.0, 0.0);
    let mut side = 1.0;
    for w in l.0.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let k = if len2 > 0.0 { (((c.x - a.x) * dx + (c.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let (px, py) = (a.x + k * dx, a.y + k * dy);
        let d = ((c.x - px).powi(2) + (c.y - py).powi(2)).sqrt();
        if d < best {
            best = d;
            at = walked + k * len2.sqrt();
            side = if dx * (c.y - a.y) - dy * (c.x - a.x) >= 0.0 { 1.0 } else { -1.0 };
        }
        walked += len2.sqrt();
    }
    (at, best * side)
}

fn run_locate(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let pts = input(inp, "Points");
    let lines_t = input(inp, "Lines");
    if pts.crs.is_some() && lines_t.crs.is_some() && pts.crs != lines_t.crs {
        return err("The points and the lines are in different coordinate systems. Reproject one of them first.");
    }
    let key_at = match p.get("key").filter(|k| !k.trim().is_empty()) {
        Some(k) => Some(lines_t.need_field(&k)?),
        None => None,
    };
    let within = number_param(p, "within", "Within")?.unwrap_or(50.0).max(0.0);
    let share = p.get("measure").as_deref() == Some("share");
    let mut lines: Vec<(usize, LineString<f64>)> = Vec::new();
    for (i, f) in lines_t.features.iter().enumerate() {
        for l in f.geom.as_ref().map(lines_of).unwrap_or_default() {
            lines.push((i, l));
        }
    }
    let tree = RTree::bulk_load(
        lines
            .iter()
            .enumerate()
            .filter_map(|(k, (_, l))| {
                let r = l.bounding_rect()?;
                Some(GeomWithData::new(Rectangle::from_corners([r.min().x - within, r.min().y - within], [r.max().x + within, r.max().y + within]), k))
            })
            .collect(),
    );
    let mut out = pts.like();
    let key_name = key_at.map_or("line".to_string(), |k| lines_t.fields[k].name.clone());
    let key_ty = key_at.map_or(FieldType::Int, |k| lines_t.fields[k].ty);
    let slots = [column(&mut out, &key_name, key_ty), column(&mut out, "measure", FieldType::Real), column(&mut out, "offset", FieldType::Real)];
    let mut unmatched = pts.like();
    for f in &pts.features {
        let Some(Geometry::Point(pt)) = &f.geom else {
            unmatched.features.push(f.clone());
            continue;
        };
        let c = pt.0;
        let mut best: Option<(f64, f64, usize)> = None;
        for cand in tree.locate_all_at_point(&[c.x, c.y]) {
            let (m, o) = locate(&lines[cand.data].1, c);
            if o.abs() <= within && best.is_none_or(|b| o.abs() < b.1.abs()) {
                best = Some((m, o, cand.data));
            }
        }
        match best {
            Some((m, o, k)) => {
                let (row, l) = &lines[k];
                let mut g = f.clone();
                g.attrs.resize(out.fields.len(), Value::Null);
                g.attrs[slots[0]] = name_of(lines_t, key_at, *row);
                let m = if share { m / Euclidean.length(l).max(1e-12) } else { m };
                g.attrs[slots[1]] = Value::Real((m * 1e4).round() / 1e4);
                g.attrs[slots[2]] = Value::Real((o * 1000.0).round() / 1000.0);
                out.features.push(g);
            }
            None => unmatched.features.push(f.clone()),
        }
    }
    ctx.notes.info(format!("{} points located, {} not within {}", fmt_count(out.len() as u64), fmt_count(unmatched.len() as u64), fmt_real(within)));
    Ok(vec![("Located".into(), out), ("Unmatched".into(), unmatched)])
}

// ---------- Place along lines (EventPlacer) ----------

const PLACE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "event_key",
        label: "Line key of the events",
        kind: ParamKind::Attribute("Events"),
        default: "",
        hint: "Which line each event is on, such as a road id",
    },
    ParamSpec {
        key: "line_key",
        label: "Line key of the lines",
        kind: ParamKind::Attribute("Lines"),
        default: "",
        hint: "The same, among the attributes of the lines",
    },
    ParamSpec {
        key: "from",
        label: "From",
        kind: ParamKind::Attribute("Events"),
        default: "",
        hint: "Where the event is along the line, or where a stretch begins",
    },
    ParamSpec { key: "to", label: "To", kind: ParamKind::Attribute("Events"), default: "", hint: "Where a stretch ends. Empty: the events are points" },
    ParamSpec { key: "measure", label: "Measures are", kind: ParamKind::Choice(MEASURES), default: "metres", hint: "" },
    ParamSpec {
        key: "offset",
        label: "To the side",
        kind: ParamKind::Text,
        default: "0",
        hint: "Moves points and stretches to the left of the line, or the right with a negative number",
    },
];

fn run_place(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let ev = input(inp, "Events");
    let lines_t = input(inp, "Lines");
    let ek = ev.need_field(&p.need("event_key", "Choose the attribute of the events that says which line they are on.")?)?;
    let lk = lines_t.need_field(&p.get("line_key").filter(|k| !k.trim().is_empty()).unwrap_or_else(|| ev.fields[ek].name.clone()))?;
    let from_at = ev.need_field(&p.need("from", "Choose the attribute with where along the line each event is.")?)?;
    let to_at = match p.get("to").filter(|k| !k.trim().is_empty()) {
        Some(k) => Some(ev.need_field(&k)?),
        None => None,
    };
    let share = p.get("measure").as_deref() == Some("share");
    let side = number_param(p, "offset", "To the side")?.unwrap_or(0.0);
    let mut by_key: HashMap<String, LineString<f64>> = HashMap::new();
    for f in &lines_t.features {
        let parts = f.geom.as_ref().map(lines_of).unwrap_or_default();
        // A line of several parts is followed as one, part after part.
        let merged = merge_lines(parts, 1e-6);
        if let Some((l, _)) = merged.into_iter().max_by(|a, b| Euclidean.length(&a.0).total_cmp(&Euclidean.length(&b.0))) {
            by_key.insert(f.attrs[lk].to_text(), l);
        }
    }
    let mut out = Table { crs: lines_t.crs, ..ev.like() };
    let mut unplaced = ev.like();
    let mut beyond = 0usize;
    for f in &ev.features {
        let (Some(l), Some(a)) = (by_key.get(&f.attrs[ek].to_text()), f.attrs[from_at].as_f64()) else {
            unplaced.features.push(f.clone());
            continue;
        };
        let len = Euclidean.length(l);
        let scale = if share { len } else { 1.0 };
        let a = a * scale;
        let l2 = if side != 0.0 { offset_line(l, side).unwrap_or_else(|| l.clone()) } else { l.clone() };
        let g = match to_at.and_then(|t| f.attrs[t].as_f64()) {
            Some(b) => {
                let b = b * scale;
                if a.min(b) > len + 1e-6 {
                    beyond += 1;
                    None
                } else {
                    // Along the moved line by the share of the length, so that the ends match.
                    let k = Euclidean.length(&l2) / len.max(1e-12);
                    part_of(&l2, a * k, b.min(len) * k).map(Geometry::LineString)
                }
            }
            None if to_at.is_some() => None,
            None => {
                if a > len + 1e-6 {
                    beyond += 1;
                    None
                } else {
                    point_at(l, a).map(|(c, dir)| {
                        let r = dir.to_radians();
                        Geometry::Point(geo::Point::new(c.x - side * r.cos(), c.y + side * r.sin()))
                    })
                }
            }
        };
        match g {
            Some(g) => {
                let mut f = f.clone();
                f.geom = Some(g);
                f.z = None;
                out.features.push(f);
            }
            None => unplaced.features.push(f.clone()),
        }
    }
    if beyond > 0 {
        ctx.notes.warn(format!("{} events lie beyond the end of their line, and went to Unplaced.", fmt_count(beyond as u64)));
    }
    ctx.notes.info(format!("{} events placed, {} not", fmt_count(out.len() as u64), fmt_count(unplaced.len() as u64)));
    Ok(vec![("Placed".into(), out), ("Unplaced".into(), unplaced)])
}

// ---------- Pivot (Pivoter) and Unpivot (Unpivoter) ----------

const PIVOT_HOW: &[(&str, &str)] =
    &[("first", "The first value"), ("sum", "The sum"), ("mean", "The mean"), ("min", "The lowest"), ("max", "The highest"), ("count", "How many")];

const PIVOT_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "group_by", label: "A row for each", kind: ParamKind::Attributes("Input"), default: "", hint: "Such as the municipality code" },
    ParamSpec { key: "names", label: "Columns from", kind: ParamKind::Attribute("Input"), default: "", hint: "Each value becomes a column, such as each year" },
    ParamSpec {
        key: "values",
        label: "Values from",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "What goes in the columns, such as the population",
    },
    ParamSpec { key: "how", label: "When there are several", kind: ParamKind::Choice(PIVOT_HOW), default: "first", hint: "" },
    ParamSpec { key: "prefix", label: "Before the names", kind: ParamKind::Text, default: "", hint: "Such as y_, so that a year becomes y_2026" },
];

fn run_pivot(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let by: Vec<usize> = p.list("group_by").iter().map(|n| t.need_field(n)).collect::<Result<_>>()?;
    let names_at = t.need_field(&p.need("names", "Choose the attribute whose values become columns.")?)?;
    let values_at = t.need_field(&p.need("values", "Choose the attribute whose values go in the columns.")?)?;
    let how = p.get("how").unwrap_or_else(|| "first".into());
    let prefix = p.get("prefix").unwrap_or_default();
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<(usize, HashMap<String, Vec<Value>>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, f) in t.features.iter().enumerate() {
        let name = f.attrs[names_at].to_text();
        if !columns.contains(&name) {
            columns.push(name.clone());
        }
        let k = format!("{:?}", by.iter().map(|b| &f.attrs[*b]).collect::<Vec<_>>());
        let r = *index.entry(k).or_insert_with(|| {
            rows.push((i, HashMap::new()));
            rows.len() - 1
        });
        rows[r].1.entry(name).or_default().push(f.attrs[values_at].clone());
    }
    if columns.len() > 2000 {
        return err(format!("That would be {} columns. Choose an attribute with fewer values to make columns of.", columns.len()));
    }
    let numeric = matches!(how.as_str(), "sum" | "mean" | "min" | "max");
    let ty = match how.as_str() {
        "count" => FieldType::Int,
        _ if numeric => FieldType::Real,
        _ => t.fields[values_at].ty,
    };
    let mut fields: Vec<Field> = by.iter().map(|b| t.fields[*b].clone()).collect();
    for c in &columns {
        let name = format!("{}{}", prefix, if c.is_empty() { "empty" } else { c });
        if fields.iter().any(|f| f.name == name) {
            return err(format!("The column {} would have the name of a group-by attribute. Give the names a prefix.", name));
        }
        fields.push(Field::new(name, ty));
    }
    let mut out = Table { fields, crs: t.crs, ..Default::default() };
    for (first, cells) in rows {
        let f = &t.features[first];
        let mut attrs: Vec<Value> = by.iter().map(|b| f.attrs[*b].clone()).collect();
        for c in &columns {
            let vals = cells.get(c).map(|v| v.as_slice()).unwrap_or(&[]);
            let nums: Vec<f64> = vals.iter().filter_map(|v| v.as_f64()).collect();
            attrs.push(match how.as_str() {
                "count" => Value::Int(vals.iter().filter(|v| !v.is_null()).count() as i64),
                "sum" if !nums.is_empty() => Value::Real(nums.iter().sum()),
                "mean" if !nums.is_empty() => Value::Real(nums.iter().sum::<f64>() / nums.len() as f64),
                "min" if !nums.is_empty() => Value::Real(nums.iter().copied().fold(f64::INFINITY, f64::min)),
                "max" if !nums.is_empty() => Value::Real(nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                "first" => vals.first().cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            });
        }
        out.features.push(Feature { attrs, geom: f.geom.clone(), z: f.z.clone() });
    }
    ctx.notes.info(format!("{} rows with {} columns of values", fmt_count(out.len() as u64), columns.len()));
    Ok(vec![("Output".into(), out)])
}

const UNPIVOT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attributes",
        label: "Attributes to rows",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "Each becomes a row of its own, such as one for each year",
    },
    ParamSpec { key: "name", label: "Name of the name column", kind: ParamKind::Text, default: "name", hint: "Gets the name of the attribute" },
    ParamSpec { key: "value", label: "Name of the value column", kind: ParamKind::Text, default: "value", hint: "Gets its value" },
    ParamSpec { key: "strip", label: "Take away from the names", kind: ParamKind::Text, default: "", hint: "Such as y_, so that y_2026 becomes 2026" },
    ParamSpec { key: "empty", label: "Empty values", kind: ParamKind::Choice(&[("keep", "Kept as rows"), ("drop", "Left out")]), default: "keep", hint: "" },
];

fn run_unpivot(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let cols: Vec<usize> = p.list("attributes").iter().map(|n| t.need_field(n)).collect::<Result<_>>()?;
    if cols.is_empty() {
        return err("Choose the attributes that are to become rows.");
    }
    let name = p.get("name").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "name".into());
    let value = p.get("value").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "value".into());
    let strip = p.get("strip").unwrap_or_default();
    let drop_empty = p.get("empty").as_deref() == Some("drop");
    let keep: Vec<usize> = (0..t.fields.len()).filter(|i| !cols.contains(i)).collect();
    if keep.iter().any(|i| t.fields[*i].name == name || t.fields[*i].name == value) {
        return err(format!("An attribute is already named {} or {}. Choose other names for the new columns.", name, value));
    }
    let ty = infer_type(t.features.iter().flat_map(|f| cols.iter().map(move |c| &f.attrs[*c])));
    let mut fields: Vec<Field> = keep.iter().map(|i| t.fields[*i].clone()).collect();
    fields.push(Field::new(name, FieldType::Text));
    fields.push(Field::new(value, ty));
    let mut out = Table { fields, crs: t.crs, ..Default::default() };
    for f in &t.features {
        for c in &cols {
            let v = &f.attrs[*c];
            if drop_empty && v.is_null() {
                continue;
            }
            let mut attrs: Vec<Value> = keep.iter().map(|i| f.attrs[*i].clone()).collect();
            let n = &t.fields[*c].name;
            attrs.push(Value::Text(n.strip_prefix(strip.as_str()).filter(|_| !strip.is_empty()).unwrap_or(n).to_string()));
            attrs.push(crate::model::coerce(v.clone(), ty));
            out.features.push(Feature { attrs, geom: f.geom.clone(), z: f.z.clone() });
        }
    }
    ctx.notes.info(format!("{} rows from {} × {}", fmt_count(out.len() as u64), fmt_count(t.len() as u64), cols.len()));
    Ok(vec![("Output".into(), out)])
}

pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "TopologyChecker",
        title: "Check topology",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Finds where areas that should lie edge to edge, such as properties or zones, overlap one another, and the gaps between them. Each overlap comes out with the two areas it lies in, and each gap as an area; the areas get how many others they overlap. With a boundary on Boundary, such as the municipality the areas should fill, gaps that open to the outside are found too, and what reaches beyond it comes out on Beyond. Overlaps and gaps smaller than a size are left out as noise, and holes larger than a size are taken to be meant.",
        inputs: &["Areas", "Boundary"],
        outputs: &["Areas", "Overlaps", "Gaps", "Beyond"],
        ports: None,
        params: TOPO_PARAMS,
        run: run_topology,
        summary: |_| String::new(),
    },
    NodeType {
        name: "SliverEliminator",
        title: "Eliminate slivers",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Merges small or thin areas, as overlays leave behind, into a neighbour: the one they share the longest border with, or the largest. The neighbour keeps its attributes. A sliver is smaller than an area, thinner than a roundness, or either. Slivers without a neighbour go to Left.",
        inputs: &["Areas"],
        outputs: &["Areas", "Left"],
        ports: None,
        params: SLIVER_PARAMS,
        run: run_slivers,
        summary: |p| p.get("area").map(|a| format!("< {}", a)).unwrap_or_default(),
    },
    NodeType {
        name: "LineMerger",
        title: "Merge lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Merges lines that meet end to end into one line, where exactly two of them meet: the pieces of a street between crossings, or a river cut into many. Only lines with the same values of the attributes chosen are merged, and only those attributes are kept, with how many lines each is made of.",
        inputs: &["Lines"],
        outputs: &["Merged", "Other"],
        ports: None,
        params: MERGE_PARAMS,
        run: run_merge_lines,
        summary: |p| p.list("group_by").join(", "),
    },
    NodeType {
        name: "PointsAlongLines",
        title: "Points along lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Puts points along each line at a distance from one another, from its start, and at its end if asked: stations along a road, or where to take a profile. Each point has the attributes of its line, the distance along it and the direction of the line there, clockwise from north.",
        inputs: &["Lines"],
        outputs: &["Points"],
        ports: None,
        params: ALONG_PARAMS,
        run: run_along,
        summary: |p| format!("every {}", p.get("distance").unwrap_or_else(|| "100".into())),
    },
    NodeType {
        name: "LineOffsetter",
        title: "Offset lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Moves lines to the side by a distance, left of their direction or right with a negative number, keeping their shape: the edges of a road from its centre line, or two directions of traffic side by side. The distance can be an attribute or an expression.",
        inputs: &["Lines"],
        outputs: &["Lines", "Other"],
        ports: None,
        params: OFFSET_PARAMS,
        run: run_offset,
        summary: |p| p.get("distance").unwrap_or_else(|| "5".into()),
    },
    NodeType {
        name: "LineLocator",
        title: "Locate along lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Linear referencing: finds for each point the nearest line within a distance, and gives it the key of that line, how far along it the point is from its start (in metres, or as a share from 0 to 1) and how far to the side, + to the left. Such as where accidents lie along roads.",
        inputs: &["Points", "Lines"],
        outputs: &["Located", "Unmatched"],
        ports: None,
        params: LOCATE_PARAMS,
        run: run_locate,
        summary: |p| p.get("key").unwrap_or_default(),
    },
    NodeType {
        name: "EventPlacer",
        title: "Place along lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Linear referencing: places events that say where they are along a line, such as a speed limit from metre 120 to 860 of a road, as points or as the stretches of the line between from and to. The lines are found by a key, and the measures are metres or shares from 0 to 1, as in NVDB. They can be moved to the side.",
        inputs: &["Events", "Lines"],
        outputs: &["Placed", "Unplaced"],
        ports: None,
        params: PLACE_PARAMS,
        run: run_place,
        summary: |p| p.get("from").unwrap_or_default(),
    },
    NodeType {
        name: "Pivoter",
        title: "Pivot",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Turns a long table wide: a row for each group, and a column for each value of an attribute, such as a column for each year with the population in it. When a group has several values for a column, the first, the sum, the mean, the lowest, the highest or how many.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: PIVOT_PARAMS,
        run: run_pivot,
        summary: |p| p.get("names").unwrap_or_default(),
    },
    NodeType {
        name: "Unpivoter",
        title: "Unpivot",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Turns a wide table long: the attributes chosen become rows, each with the name of the attribute and its value, and the other attributes on every row. Such as a column for each year made into a row for each year, for a chart or a database.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: UNPIVOT_PARAMS,
        run: run_unpivot,
        summary: |p| p.list("attributes").join(", "),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{line_string, polygon};

    fn areas(polys: Vec<Polygon<f64>>) -> Table {
        Table {
            fields: vec![Field::new("id", FieldType::Text)],
            crs: Some(3006),
            features: polys.into_iter().enumerate().map(|(i, p)| Feature::new(vec![Value::Text(format!("p{}", i + 1))], Some(Geometry::Polygon(p)))).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn overlaps_gaps_and_slivers() {
        // Three squares in a row: the second overlaps the first by 1, and leaves a gap of 1 to
        // the third above a sliver. Without a boundary, a gap open to the outside is not
        // found: only holes.
        let t = areas(vec![
            polygon![(x: 0.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 10.0), (x: 0.0, y: 10.0)],
            polygon![(x: 9.0, y: 0.0), (x: 20.0, y: 0.0), (x: 20.0, y: 4.0), (x: 21.0, y: 4.0), (x: 21.0, y: 6.0), (x: 20.0, y: 6.0), (x: 20.0, y: 10.0), (x: 9.0, y: 10.0)],
            polygon![(x: 21.0, y: 0.0), (x: 30.0, y: 0.0), (x: 30.0, y: 10.0), (x: 21.0, y: 10.0), (x: 21.0, y: 6.0), (x: 21.0, y: 4.0)],
            polygon![(x: 20.0, y: 0.0), (x: 21.0, y: 0.0), (x: 21.0, y: 4.0), (x: 20.0, y: 4.0)],
            // A strip over them all, so that the gap below it is closed in.
            polygon![(x: 0.0, y: 10.0), (x: 30.0, y: 10.0), (x: 30.0, y: 12.0), (x: 0.0, y: 12.0)],
        ]);
        let (out, _) = run_for_test("TopologyChecker", &[("id", "id")], vec![("Areas", t.clone())]).unwrap();
        let overlaps = &out[1].1;
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps.features[0].attrs[..2], [Value::Text("p1".into()), Value::Text("p2".into())]);
        assert_eq!(overlaps.features[0].attrs[2], Value::Real(10.0));
        let gaps = &out[2].1;
        assert_eq!(gaps.len(), 1, "the strip from 6 to 10 between the second and third");
        assert_eq!(gaps.features[0].attrs[0], Value::Real(4.0));
        // The strip p4 of 1 × 4 is merged into the neighbour it shares the most border with.
        let (out, _) = run_for_test("SliverEliminator", &[("area", "5")], vec![("Areas", t)]).unwrap();
        assert_eq!(out[0].1.len(), 4);
        // It shares 5 m of border with p2 and 4 with p3.
        let merged = out[0].1.features.iter().find(|f| f.attrs[0] == Value::Text("p2".into())).unwrap();
        assert!((merged.geom.as_ref().unwrap().unsigned_area() - 116.0).abs() < 1e-6);
    }

    #[test]
    fn gaps_open_to_the_outside_are_found_within_a_boundary() {
        // Two squares side by side with a notch between them open at the top, in a boundary
        // that the second does not reach the end of, and that the first reaches beyond.
        let t = areas(vec![
            polygon![(x: -1.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 10.0), (x: -1.0, y: 10.0)],
            polygon![(x: 10.0, y: 0.0), (x: 20.0, y: 0.0), (x: 20.0, y: 8.0), (x: 10.0, y: 8.0)],
        ]);
        let boundary = areas(vec![polygon![(x: 0.0, y: 0.0), (x: 20.0, y: 0.0), (x: 20.0, y: 10.0), (x: 0.0, y: 10.0)]]);
        let (out, _) = run_for_test("TopologyChecker", &[("id", "id")], vec![("Areas", t.clone())]).unwrap();
        assert_eq!(out[2].1.len(), 0, "without a boundary, no hole and no gap");
        let (out, notes) = run_for_test("TopologyChecker", &[("id", "id")], vec![("Areas", t), ("Boundary", boundary)]).unwrap();
        let gaps = &out[2].1;
        assert_eq!(gaps.len(), 1, "{:?}", notes.info);
        assert_eq!(gaps.features[0].attrs[0], Value::Real(20.0), "10 × 2 above the second square");
        let beyond = &out[3].1;
        assert_eq!(beyond.len(), 1);
        assert_eq!(beyond.features[0].attrs, [Value::Text("p1".into()), Value::Real(10.0)]);
    }

    #[test]
    fn lines_merged_points_along_offsets_and_places() {
        let lines = Table {
            fields: vec![Field::new("road", FieldType::Text)],
            crs: Some(3006),
            features: vec![
                Feature::new(vec![Value::Text("E20".into())], Some(Geometry::LineString(line_string![(x: 10.0, y: 0.0), (x: 0.0, y: 0.0)]))),
                Feature::new(
                    vec![Value::Text("E20".into())],
                    Some(Geometry::LineString(line_string![(x: 10.0, y: 0.0), (x: 20.0, y: 0.0), (x: 20.0, y: 10.0)])),
                ),
                Feature::new(vec![Value::Text("26".into())], Some(Geometry::LineString(line_string![(x: 20.0, y: 10.0), (x: 30.0, y: 10.0)]))),
            ],
            ..Default::default()
        };
        let (out, _) = run_for_test("LineMerger", &[("group_by", "road")], vec![("Lines", lines.clone())]).unwrap();
        assert_eq!(out[0].1.len(), 2);
        let e20 = &out[0].1.features[0];
        assert_eq!(e20.attrs, vec![Value::Text("E20".into()), Value::Int(2)]);
        let e20_line = lines_of(e20.geom.as_ref().unwrap()).remove(0);
        assert!((Euclidean.length(&e20_line) - 30.0).abs() < 1e-9);
        let (out, _) = run_for_test("PointsAlongLines", &[("distance", "4")], vec![("Lines", out[0].1.clone())]).unwrap();
        assert_eq!(out[0].1.len(), 8 + 1 + 3 + 1, "every 4 m and the ends");
        let off = offset_line(&line_string![(x: 0.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 10.0)], 1.0).unwrap();
        assert_eq!(off.0, vec![Coord { x: 0.0, y: 1.0 }, Coord { x: 9.0, y: 1.0 }, Coord { x: 9.0, y: 10.0 }]);
        // An accident 5 m along road 26, 2 m to its left, and a stretch on E20.
        let (at, side) = locate(&line_string![(x: 20.0, y: 10.0), (x: 30.0, y: 10.0)], Coord { x: 25.0, y: 12.0 });
        assert_eq!((at, side), (5.0, 2.0));
        let events = Table {
            fields: vec![Field::new("road", FieldType::Text), Field::new("from", FieldType::Real), Field::new("to", FieldType::Real)],
            features: vec![Feature::new(vec![Value::Text("E20".into()), Value::Real(5.0), Value::Real(15.0)], None)],
            ..Default::default()
        };
        let (out, _) =
            run_for_test("EventPlacer", &[("event_key", "road"), ("from", "from"), ("to", "to")], vec![("Events", events), ("Lines", lines)]).unwrap();
        let stretch = lines_of(out[0].1.features[0].geom.as_ref().unwrap()).remove(0);
        assert!((Euclidean.length(&stretch) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn tables_are_turned_wide_and_long() {
        let t = Table {
            fields: vec![Field::new("kommun", FieldType::Text), Field::new("year", FieldType::Int), Field::new("pop", FieldType::Int)],
            features: [("Skara", 2025, 19000), ("Skara", 2026, 19100), ("Mark", 2026, 35000)]
                .iter()
                .map(|(k, y, p)| Feature::new(vec![Value::Text(k.to_string()), Value::Int(*y), Value::Int(*p)], None))
                .collect(),
            ..Default::default()
        };
        let (out, _) = run_for_test("Pivoter", &[("group_by", "kommun"), ("names", "year"), ("values", "pop"), ("prefix", "y_")], vec![("Input", t)]).unwrap();
        let wide = &out[0].1;
        assert_eq!(wide.field_names(), ["kommun", "y_2025", "y_2026"]);
        assert_eq!(wide.features[1].attrs, vec![Value::Text("Mark".into()), Value::Null, Value::Int(35000)]);
        let (out, _) = run_for_test(
            "Unpivoter",
            &[("attributes", "y_2025, y_2026"), ("name", "year"), ("value", "pop"), ("strip", "y_"), ("empty", "drop")],
            vec![("Input", wide.clone())],
        )
        .unwrap();
        let long = &out[0].1;
        assert_eq!(long.len(), 3);
        assert_eq!(long.features[2].attrs, vec![Value::Text("Mark".into()), Value::Text("2026".into()), Value::Int(35000)]);
    }
}
