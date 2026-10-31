//! Tools for networks added in 0.91: Build network, which makes lines into a network that
//! routes can be found along; Service areas, what can be reached within some minutes or
//! metres; and Allocate to nearest, which part of the network and which customers each
//! facility is nearest.

use super::geom_transform::number_param;
use super::network::{
    lines_of, network_crs, network_from, point_of, P_AGAINST, P_ALONG, P_COST, P_DEFAULT_SPEED, P_SNAP, P_SPEED, P_SPEED_AGAINST, P_TOLERANCE,
};
use super::*;
use crate::model::{err, fmt_count, fmt_real};
use crate::network::{cut, Network, Place, Search};
use geo::{Coord, Geometry, LineString};

// ---------- Build network (NetworkBuilder) ----------

const BUILD_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "tolerance",
        label: "Join ends within",
        kind: ParamKind::Text,
        default: "0.5",
        hint: "In metres: ends this close become one junction, and an end this close to another line is joined to it there",
    },
    ParamSpec {
        key: "split",
        label: "Lines that cross",
        kind: ParamKind::Choice(&[("yes", "Are split and joined where they cross"), ("no", "Are joined only at their ends")]),
        default: "yes",
        hint: "OpenStreetMap and Overture need splitting; NVDB is joined at its ends already",
    },
    ParamSpec {
        key: "level",
        label: "Level",
        kind: ParamKind::Expression("Lines"),
        default: "",
        hint: "Lines of different levels cross without being joined, as a bridge over a road: such as layer in OpenStreetMap. Empty: all on one level",
    },
    ParamSpec {
        key: "near",
        label: "Report loose ends within",
        kind: ParamKind::Text,
        default: "2",
        hint: "In metres: an end that is this close to another line, but not joined to it, is reported on Problems",
    },
];

/// A line of the network being built: its feature, its coordinates and its level.
struct Piece {
    feature: usize,
    coords: Vec<Coord<f64>>,
    level: String,
}

/// Finds the root of a set, halving the path on the way.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Makes lines into a network: ends within `tol` made one, ends that nearly touch a line
/// joined to it, lines split where they cross on the same level, and pieces that lie twice
/// taken away. Gives the pieces with their features, and how many ends were moved, how many
/// splits and how many duplicates.
fn build(mut pieces: Vec<Piece>, tol: f64, split: bool) -> (Vec<(usize, LineString<f64>)>, usize, usize, usize) {
    use rstar::{primitives::GeomWithData, primitives::Line as RLine, PointDistance, RTree, RTreeObject, AABB};
    // 1. Ends within the tolerance become their mean.
    let mut ends: Vec<(usize, bool)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        if p.coords.len() >= 2 {
            ends.push((i, false));
            ends.push((i, true));
        }
    }
    let end_at = |pieces: &Vec<Piece>, e: (usize, bool)| if e.1 { *pieces[e.0].coords.last().expect("two") } else { pieces[e.0].coords[0] };
    let mut parent: Vec<usize> = (0..ends.len()).collect();
    let tree: RTree<GeomWithData<[f64; 2], usize>> = RTree::bulk_load(
        ends.iter()
            .enumerate()
            .map(|(k, e)| {
                let c = end_at(&pieces, *e);
                GeomWithData::new([c.x, c.y], k)
            })
            .collect(),
    );
    for (k, e) in ends.iter().enumerate() {
        let c = end_at(&pieces, *e);
        for n in tree.locate_within_distance([c.x, c.y], tol * tol) {
            let (a, b) = (root(&mut parent, k), root(&mut parent, n.data));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut sums: HashMap<usize, (f64, f64, usize)> = HashMap::new();
    for (k, e) in ends.iter().enumerate() {
        let c = end_at(&pieces, *e);
        let r = root(&mut parent, k);
        let s = sums.entry(r).or_insert((0.0, 0.0, 0));
        *s = (s.0 + c.x, s.1 + c.y, s.2 + 1);
    }
    let mut moved = 0usize;
    for (k, e) in ends.iter().enumerate() {
        let r = root(&mut parent, k);
        let s = sums[&r];
        let mean = Coord { x: s.0 / s.2 as f64, y: s.1 / s.2 as f64 };
        let p = &mut pieces[e.0];
        let slot = if e.1 { p.coords.len() - 1 } else { 0 };
        if p.coords[slot] != mean {
            if (p.coords[slot].x - mean.x).hypot(p.coords[slot].y - mean.y) > 1e-9 {
                moved += 1;
            }
            p.coords[slot] = mean;
        }
    }
    // 2. Where each line is to be split: by distance along it, at a point.
    type Seg = GeomWithData<RLine<[f64; 2]>, (usize, usize)>;
    let segs: Vec<Seg> = pieces
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p.coords.windows(2).enumerate().map(move |(k, w)| GeomWithData::new(RLine::new([w[0].x, w[0].y], [w[1].x, w[1].y]), (i, k))))
        .collect();
    let index: RTree<Seg> = RTree::bulk_load(segs.clone());
    let mut cuts: Vec<Vec<(usize, f64, Coord<f64>)>> = vec![Vec::new(); pieces.len()];
    let on_seg = |a: Coord<f64>, b: Coord<f64>, p: Coord<f64>| -> f64 {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l2 = dx * dx + dy * dy;
        if l2 == 0.0 {
            0.0
        } else {
            (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0)
        }
    };
    // Ends that nearly touch another line are joined to it.
    let mut end_moves: Vec<(usize, usize, Coord<f64>)> = Vec::new();
    for e in &ends {
        let c = end_at(&pieces, *e);
        let level = &pieces[e.0].level;
        let near = index
            .locate_in_envelope_intersecting(&AABB::from_corners([c.x - tol, c.y - tol], [c.x + tol, c.y + tol]))
            .filter(|s| s.data.0 != e.0 && pieces[s.data.0].level == *level)
            .min_by(|a, b| a.distance_2(&[c.x, c.y]).total_cmp(&b.distance_2(&[c.x, c.y])));
        if let Some(s) = near {
            if s.distance_2(&[c.x, c.y]) > tol * tol {
                continue;
            }
            let (j, k) = s.data;
            let (a, b) = (pieces[j].coords[k], pieces[j].coords[k + 1]);
            let t = on_seg(a, b, c);
            let at = Coord { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
            // At an end of the other line it was joined above already.
            let first = k == 0 && t < 1e-9;
            let last = k + 2 == pieces[j].coords.len() && t > 1.0 - 1e-9;
            if !first && !last {
                cuts[j].push((k, t, at));
                end_moves.push((e.0, if e.1 { pieces[e.0].coords.len() - 1 } else { 0 }, at));
            }
        }
    }
    for (i, slot, at) in end_moves {
        if pieces[i].coords[slot] != at {
            moved += 1;
            pieces[i].coords[slot] = at;
        }
    }
    // Crossings on the same level.
    if split {
        let index: RTree<Seg> = RTree::bulk_load(
            pieces
                .iter()
                .enumerate()
                .flat_map(|(i, p)| p.coords.windows(2).enumerate().map(move |(k, w)| GeomWithData::new(RLine::new([w[0].x, w[0].y], [w[1].x, w[1].y]), (i, k))))
                .collect(),
        );
        for s in index.iter() {
            let (i, k) = s.data;
            let env = s.envelope();
            for o in index.locate_in_envelope_intersecting(&env) {
                let (j, m) = o.data;
                if (j, m) <= (i, k) || pieces[i].level != pieces[j].level {
                    continue;
                }
                // Neighbouring pieces of the same line meet at their shared point anyway.
                if i == j && (m == k + 1 || k == m + 1) {
                    continue;
                }
                let l1 = geo::Line::new(pieces[i].coords[k], pieces[i].coords[k + 1]);
                let l2 = geo::Line::new(pieces[j].coords[m], pieces[j].coords[m + 1]);
                let mut points = Vec::new();
                match geo::algorithm::line_intersection::line_intersection(l1, l2) {
                    Some(geo::LineIntersection::SinglePoint { intersection, .. }) => points.push(intersection),
                    Some(geo::LineIntersection::Collinear { intersection }) => {
                        points.push(intersection.start);
                        points.push(intersection.end);
                    }
                    None => {}
                }
                for p in points {
                    cuts[i].push((k, on_seg(l1.start, l1.end, p), p));
                    cuts[j].push((m, on_seg(l2.start, l2.end, p), p));
                }
            }
        }
    }
    // 3. The lines cut at their points.
    let mut out: Vec<(usize, LineString<f64>)> = Vec::new();
    let mut splits = 0usize;
    for (i, p) in pieces.iter().enumerate() {
        if p.coords.len() < 2 {
            continue;
        }
        let mut c = std::mem::take(&mut cuts[i]);
        c.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap_or(std::cmp::Ordering::Equal));
        let mut part: Vec<Coord<f64>> = vec![p.coords[0]];
        let mut ci = 0;
        for k in 0..p.coords.len() - 1 {
            while ci < c.len() && c[ci].0 == k {
                let at = c[ci].2;
                ci += 1;
                let last = *part.last().expect("a point");
                if (at.x - last.x).hypot(at.y - last.y) < 1e-9 {
                    continue;
                }
                let end = p.coords[p.coords.len() - 1];
                if (at.x - end.x).hypot(at.y - end.y) < 1e-9 {
                    continue;
                }
                part.push(at);
                out.push((p.feature, LineString::new(std::mem::replace(&mut part, vec![at]))));
                splits += 1;
            }
            let next = p.coords[k + 1];
            if part.last() != Some(&next) {
                part.push(next);
            }
        }
        if part.len() >= 2 {
            out.push((p.feature, LineString::new(part)));
        }
    }
    // 4. Pieces that lie twice, either way round, once.
    let key = |l: &LineString<f64>| -> String {
        let fwd: Vec<(i64, i64)> = l.0.iter().map(|c| ((c.x * 1000.0).round() as i64, (c.y * 1000.0).round() as i64)).collect();
        let mut back = fwd.clone();
        back.reverse();
        format!("{:?}", if fwd <= back { fwd } else { back })
    };
    let mut seen = std::collections::HashSet::new();
    let before = out.len();
    out.retain(|(_, l)| seen.insert(key(l)));
    let duplicates = before - out.len();
    (out, moved, splits, duplicates)
}

fn run_build(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Lines");
    if ctx.geographic(t.crs) == Some(true) {
        return err(format!("The lines are in {}, in degrees. Reproject them to metres first, such as to EPSG:3006.", crs_label(t.crs)));
    }
    let tol = number_param(p, "tolerance", "Join ends within")?.unwrap_or(0.5).max(1e-6);
    let near = number_param(p, "near", "Report loose ends within")?.unwrap_or(2.0).max(0.0);
    let split = p.get("split").as_deref() != Some("no");
    let level = match p.get("level").filter(|v| !v.trim().is_empty()) {
        Some(e) => Some(crate::expr::compile(&e, &t.field_names()).map_err(|e| GbError(format!("Level: {}", e.0)))?),
        None => None,
    };
    let mut pieces = Vec::new();
    let mut not_lines = 0usize;
    for (i, f) in t.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        let ls = lines_of(g);
        if ls.is_empty() {
            not_lines += 1;
            continue;
        }
        let lv = match &level {
            Some(e) => e.eval(&crate::expr::Row { feature: f, index: i, geographic: Some(false) })?.to_text(),
            None => String::new(),
        };
        for l in ls {
            pieces.push(Piece { feature: i, coords: l.0, level: lv.clone() });
        }
    }
    if not_lines > 0 {
        ctx.notes.warn(format!("{} features are not lines and were left out.", fmt_count(not_lines as u64)));
    }
    let (built, moved, splits, duplicates) = build(pieces, tol, split);
    // The parts of the network that hang together.
    let snap = |c: &Coord<f64>| ((c.x / tol).round() as i64, (c.y / tol).round() as i64);
    let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    let mut node = |c: &Coord<f64>, parent: &mut Vec<usize>| {
        let n = ids.len();
        let id = *ids.entry(snap(c)).or_insert(n);
        if id == parent.len() {
            parent.push(id);
        }
        id
    };
    let mut ends: Vec<(usize, usize)> = Vec::new();
    for (_, l) in &built {
        let a = node(&l.0[0], &mut parent);
        let b = node(l.0.last().expect("two"), &mut parent);
        ends.push((a, b));
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    }
    let mut degree = vec![0usize; parent.len()];
    for (a, b) in &ends {
        degree[*a] += 1;
        degree[*b] += 1;
    }
    let comp: Vec<usize> = ends.iter().map(|(a, _)| root(&mut parent, *a)).collect();
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for c in &comp {
        *sizes.entry(*c).or_default() += 1;
    }
    let mut ranked: Vec<(usize, usize)> = sizes.iter().map(|(c, n)| (*c, *n)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let number: HashMap<usize, usize> = ranked.iter().enumerate().map(|(k, (c, _))| (*c, k + 1)).collect();
    let mut out = t.like();
    out.fields = t.fields.clone();
    let part_at = super::geom_inspect::column(&mut out, "network_part", FieldType::Int);
    for ((f, l), c) in built.iter().zip(&comp) {
        let mut attrs = t.features[*f].attrs.clone();
        attrs.resize(out.fields.len(), Value::Null);
        attrs[part_at] = Value::Int(number[c] as i64);
        out.features.push(Feature::new(attrs, Some(Geometry::LineString(l.clone()))));
    }
    // Problems: loose ends close to a line they are not joined to, and parts on their own.
    let mut problems = Table {
        fields: vec![Field::new("problem", FieldType::Text), Field::new("network_part", FieldType::Int), Field::new("lines", FieldType::Int)],
        crs: t.crs,
        ..Default::default()
    };
    use rstar::{primitives::GeomWithData, primitives::Line as RLine, PointDistance, RTree};
    let index: RTree<GeomWithData<RLine<[f64; 2]>, usize>> = RTree::bulk_load(
        built
            .iter()
            .enumerate()
            .flat_map(|(i, (_, l))| l.0.windows(2).map(move |w| GeomWithData::new(RLine::new([w[0].x, w[0].y], [w[1].x, w[1].y]), i)))
            .collect(),
    );
    let mut loose = 0usize;
    for (i, ((_, l), (a, b))) in built.iter().zip(&ends).enumerate() {
        for (c, n) in [(l.0[0], *a), (*l.0.last().expect("two"), *b)] {
            if degree[n] != 1 || near <= 0.0 {
                continue;
            }
            let close = index.locate_within_distance([c.x, c.y], near * near).any(|s| s.data != i && s.distance_2(&[c.x, c.y]) > 1e-12);
            if close {
                loose += 1;
                problems.features.push(Feature::new(
                    vec![Value::Text("A loose end close to another line".into()), Value::Int(number[&comp[i]] as i64), Value::Null],
                    Some(Geometry::Point(geo::Point(c))),
                ));
            }
        }
    }
    for (c, n) in ranked.iter().skip(1) {
        let first = comp.iter().position(|x| x == c).expect("a line");
        let at = built[first].1 .0[built[first].1 .0.len() / 2];
        problems.features.push(Feature::new(
            vec![Value::Text("A part not joined to the largest".into()), Value::Int(number[c] as i64), Value::Int(*n as i64)],
            Some(Geometry::Point(geo::Point(at))),
        ));
    }
    ctx.notes.info(format!(
        "{} pieces: {} ends joined, {} splits where lines cross or touch, {} pieces that lay twice taken away",
        fmt_count(out.len() as u64),
        fmt_count(moved as u64),
        fmt_count(splits as u64),
        fmt_count(duplicates as u64)
    ));
    if ranked.len() > 1 {
        ctx.notes.warn(format!(
            "The network is in {} parts that are not joined: the largest has {} of the {} pieces. The others are on Problems; routes cannot go between them.",
            ranked.len(),
            fmt_count(ranked[0].1 as u64),
            fmt_count(out.len() as u64)
        ));
    }
    if loose > 0 {
        ctx.notes.warn(format!("{} loose ends are within {} m of a line they are not joined to: see Problems.", fmt_count(loose as u64), fmt_real(near)));
    }
    Ok(vec![("Network".into(), out), ("Problems".into(), problems)])
}

// ---------- Service areas and Allocate to nearest ----------

const DIRECTIONS: &[(&str, &str)] =
    &[("to", "Travelling to the facilities, as customers do"), ("from", "Travelling from the facilities, as an ambulance does")];

const P_BREAKS: ParamSpec = ParamSpec {
    key: "breaks",
    label: "Within",
    kind: ParamKind::Text,
    default: "5, 10, 15",
    hint: "Minutes for the quickest, metres for the shortest, separated by commas: an area for each",
};
const P_DIRECTION: ParamSpec =
    ParamSpec { key: "direction", label: "Direction", kind: ParamKind::Choice(DIRECTIONS), default: "to", hint: "Matters where there are one-way roads" };
const P_EACH: ParamSpec = ParamSpec {
    key: "each",
    label: "Areas",
    kind: ParamKind::Choice(&[("together", "Of all the facilities together"), ("each", "Of each facility on its own")]),
    default: "together",
    hint: "",
};
const P_WIDTH: ParamSpec = ParamSpec {
    key: "width",
    label: "Area around the roads",
    kind: ParamKind::Text,
    default: "100",
    hint: "In metres: how far from the roads reached the area reaches, for the houses and land along them",
};
const P_REACH: ParamSpec = ParamSpec {
    key: "reach",
    label: "At most",
    kind: ParamKind::Text,
    default: "",
    hint: "Minutes or metres: farther than this belongs to no facility. Empty: no limit",
};

const SERVICE_PARAMS: &[ParamSpec] =
    &[P_BREAKS, P_DIRECTION, P_EACH, P_WIDTH, P_COST, P_SPEED, P_SPEED_AGAINST, P_DEFAULT_SPEED, P_ALONG, P_AGAINST, P_SNAP, P_TOLERANCE];
const ALLOCATE_PARAMS: &[ParamSpec] =
    &[P_DIRECTION, P_REACH, P_WIDTH, P_COST, P_SPEED, P_SPEED_AGAINST, P_DEFAULT_SPEED, P_ALONG, P_AGAINST, P_SNAP, P_TOLERANCE];

/// A table in the coordinate system of the network.
fn in_crs(t: &Table, crs: u32, what: &str, ctx: &NodeCtx) -> Result<Table> {
    match t.crs {
        Some(c) if c != crs => reproject(t, crs, ctx.backend),
        None if !t.is_empty() => err(format!("{} has no coordinate system. Set it on the input.", what)),
        _ => Ok(t.clone()),
    }
}

/// The places of the features on the network, by their index, and how many are too far.
fn places_of(net: &Network, t: &Table, snap: f64) -> (Vec<(Place, usize)>, usize) {
    let mut out = Vec::new();
    let mut far = 0;
    for (k, f) in t.features.iter().enumerate() {
        match f.geom.as_ref().and_then(point_of).and_then(|c| net.place(c, snap)) {
            Some(pl) => out.push((pl, k)),
            None => far += 1,
        }
    }
    (out, far)
}

/// The parts of each edge within `limit` of the starts of a search: as distances along the
/// edge, from and to.
fn reached(net: &Network, s: &Search, starts: &[(Place, usize)], backwards: bool, limit: f64) -> Vec<(usize, f64, f64)> {
    let mut out = Vec::new();
    for (i, e) in net.edges.iter().enumerate() {
        let mut parts: Vec<(f64, f64)> = Vec::new();
        let (ra, rg) = (net.rate(i, true), net.rate(i, false));
        // Forwards: on from the start of the edge along it, and from its end against it.
        // Backwards: from the end along it to the junction, and from the start against it.
        let (cf, ct) = (s.cost[e.from], s.cost[e.to]);
        if !backwards {
            if let Some(r) = ra.filter(|_| cf < limit) {
                parts.push((0.0, if r > 0.0 { ((limit - cf) / r).min(e.length) } else { e.length }));
            }
            if let Some(r) = rg.filter(|_| ct < limit) {
                parts.push((if r > 0.0 { (e.length - (limit - ct) / r).max(0.0) } else { 0.0 }, e.length));
            }
        } else {
            if let Some(r) = ra.filter(|_| ct < limit) {
                parts.push((if r > 0.0 { (e.length - (limit - ct) / r).max(0.0) } else { 0.0 }, e.length));
            }
            if let Some(r) = rg.filter(|_| cf < limit) {
                parts.push((0.0, if r > 0.0 { ((limit - cf) / r).min(e.length) } else { e.length }));
            }
        }
        // The edges the facilities stand on, from them.
        for (pl, _) in starts.iter().filter(|q| q.0.edge == i) {
            let fwd = if backwards { rg } else { ra };
            let bwd = if backwards { ra } else { rg };
            if let Some(r) = fwd {
                parts.push((pl.at, if r > 0.0 { (pl.at + limit / r).min(e.length) } else { e.length }));
            }
            if let Some(r) = bwd {
                parts.push((if r > 0.0 { (pl.at - limit / r).max(0.0) } else { 0.0 }, pl.at));
            }
        }
        parts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in parts {
            match merged.last_mut() {
                Some(m) if a <= m.1 + 1e-9 => m.1 = m.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        for (a, b) in merged {
            if b - a > 1e-6 {
                out.push((i, a, b));
            }
        }
    }
    out
}

/// The area around lines: each buffered, and all put together.
fn area_around(lines: &[LineString<f64>], width: f64) -> Option<Geometry<f64>> {
    use geo::Buffer;
    if lines.is_empty() {
        return None;
    }
    let u = geo::MultiLineString::new(lines.to_vec()).buffer(width);
    match u.0.len() {
        0 => None,
        1 => Some(Geometry::Polygon(u.0.into_iter().next().expect("one"))),
        _ => Some(Geometry::MultiPolygon(u)),
    }
}

fn run_service(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let net_t = input(inp, "Network");
    if net_t.is_empty() {
        return err("Nothing came in on Network. Connect the roads, built with Build network if they are from OpenStreetMap or Overture.");
    }
    let crs = network_crs(net_t, ctx)?;
    let fac = in_crs(input(inp, "Facilities"), crs, "Facilities", ctx)?;
    if fac.is_empty() {
        return err("Nothing came in on Facilities. Connect the shops, schools or stations to reach.");
    }
    let (net, time) = network_from(p, net_t, ctx)?;
    let snap = number_param(p, "snap", "The farthest from the network")?.unwrap_or(500.0).max(0.0);
    let mut breaks: Vec<f64> = p.list("breaks").iter().filter_map(|b| crate::model::parse_number(b)).filter(|b| *b > 0.0).collect();
    if breaks.is_empty() {
        return err("Give the limits, such as 5, 10, 15 minutes.");
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let width = number_param(p, "width", "Area around the roads")?.unwrap_or(100.0).max(1.0);
    let backwards = p.get("direction").as_deref() != Some("from");
    let each = p.get("each").as_deref() == Some("each");
    let (starts, far) = places_of(&net, &fac, snap);
    if far > 0 {
        ctx.notes.warn(format!("{} facilities are more than {} m from the network and were left out.", fmt_count(far as u64), fmt_real(snap)));
    }
    if starts.is_empty() {
        return err("No facility is near the network. Are they in the same area and coordinate system?");
    }
    let unit = if time { "minutes" } else { "metres" };
    let mut areas = Table { fields: vec![Field::new(unit, FieldType::Real)], crs: Some(crs), ..Default::default() };
    let mut lines = Table { fields: vec![Field::new(unit, FieldType::Real)], crs: Some(crs), ..Default::default() };
    if each {
        for f in &fac.fields {
            areas.fields.push(f.clone());
            lines.fields.push(f.clone());
        }
    }
    let gate = ctx.gate;
    let groups: Vec<Vec<(Place, usize)>> = if each { starts.iter().map(|s| vec![*s]).collect() } else { vec![starts.clone()] };
    for (g, group) in groups.iter().enumerate() {
        (ctx.progress)("facilities", g as u64, Some(groups.len() as u64));
        let s = net.search(group, backwards, &|| gate.going());
        if !gate.going() {
            return err(crate::model::STOPPED);
        }
        // The largest first, so that the smaller lie on top when drawn.
        for b in breaks.iter().rev() {
            let parts = reached(&net, &s, group, backwards, *b);
            let ls: Vec<LineString<f64>> = parts.iter().map(|(e, a, c)| LineString::new(cut(&net.edges[*e].line, *a, *c))).filter(|l| l.0.len() >= 2).collect();
            let mut attrs = vec![Value::Real(*b)];
            if each {
                attrs.extend(fac.features[group[0].1].attrs.iter().cloned());
            }
            for l in &ls {
                lines.features.push(Feature::new(attrs.clone(), Some(Geometry::LineString(l.clone()))));
            }
            if let Some(a) = area_around(&ls, width) {
                areas.features.push(Feature::new(attrs, Some(a)));
            }
        }
    }
    ctx.notes.info(format!("{} areas within {} {}", areas.len(), breaks.iter().map(|b| fmt_real(*b)).collect::<Vec<_>>().join(", "), unit));
    Ok(vec![("Areas".into(), areas), ("Roads".into(), lines)])
}

fn run_allocate(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let net_t = input(inp, "Network");
    if net_t.is_empty() {
        return err("Nothing came in on Network. Connect the roads, built with Build network if they are from OpenStreetMap or Overture.");
    }
    let crs = network_crs(net_t, ctx)?;
    let fac = in_crs(input(inp, "Facilities"), crs, "Facilities", ctx)?;
    if fac.is_empty() {
        return err("Nothing came in on Facilities. Connect the shops, schools or stations.");
    }
    let demand = in_crs(input(inp, "Demand"), crs, "Demand", ctx)?;
    let (net, time) = network_from(p, net_t, ctx)?;
    let snap = number_param(p, "snap", "The farthest from the network")?.unwrap_or(500.0).max(0.0);
    let reach = number_param(p, "reach", "At most")?.filter(|r| *r > 0.0).unwrap_or(f64::INFINITY);
    let width = number_param(p, "width", "Area around the roads")?.unwrap_or(100.0).max(1.0);
    let backwards = p.get("direction").as_deref() != Some("from");
    let (starts, far) = places_of(&net, &fac, snap);
    if far > 0 {
        ctx.notes.warn(format!("{} facilities are more than {} m from the network and were left out.", fmt_count(far as u64), fmt_real(snap)));
    }
    if starts.is_empty() {
        return err("No facility is near the network.");
    }
    let gate = ctx.gate;
    let s = net.search(&starts, backwards, &|| gate.going());
    let unit = if time { "minutes" } else { "metres" };
    let fac_fields: Vec<Field> = fac.fields.iter().map(|f| Field::new(format!("facility_{}", f.name), f.ty)).collect();
    // Each piece of the network to the facility nearest its middle.
    let mut roads = Table { fields: vec![Field::new(unit, FieldType::Real)], crs: Some(crs), ..Default::default() };
    roads.fields.extend(fac_fields.iter().cloned());
    let mut by_fac: HashMap<usize, Vec<LineString<f64>>> = HashMap::new();
    for (i, e) in net.edges.iter().enumerate() {
        let mid = e.length / 2.0;
        let mut best: Option<(f64, usize)> = None;
        let mut consider = |c: f64, src: usize| {
            if c.is_finite() && best.is_none_or(|b| c < b.0) {
                best = Some((c, src));
            }
        };
        let (ra, rg) = (net.rate(i, true), net.rate(i, false));
        // The middle reached from either end, as the search and the direction allow.
        let (via_from, via_to) = if backwards { (rg, ra) } else { (ra, rg) };
        if let Some(r) = via_from {
            consider(s.cost[e.from] + mid * r, s.source[e.from]);
        }
        if let Some(r) = via_to {
            consider(s.cost[e.to] + mid * r, s.source[e.to]);
        }
        for (pl, k) in starts.iter().filter(|q| q.0.edge == i) {
            let r = ra.or(rg).unwrap_or(1.0);
            consider((pl.at - mid).abs() * r, *k);
        }
        let Some((c, k)) = best.filter(|b| b.0 <= reach) else { continue };
        let mut attrs = vec![Value::Real((c * 100.0).round() / 100.0)];
        attrs.extend(fac.features[k].attrs.iter().cloned());
        roads.features.push(Feature::new(attrs, Some(Geometry::LineString(e.line.clone()))));
        by_fac.entry(k).or_default().push(e.line.clone());
    }
    // The areas of each facility, around its roads.
    let mut areas = Table { fields: fac_fields.clone(), crs: Some(crs), ..Default::default() };
    let mut keys: Vec<usize> = by_fac.keys().copied().collect();
    keys.sort();
    for k in keys {
        if let Some(a) = area_around(&by_fac[&k], width) {
            areas.features.push(Feature::new(fac.features[k].attrs.clone(), Some(a)));
        }
    }
    // Each customer to the facility nearest along the network.
    let mut out = demand.like();
    out.fields = demand.fields.clone();
    out.fields.push(Field::new(unit, FieldType::Real));
    out.fields.extend(fac_fields.iter().cloned());
    let mut missed = 0usize;
    for f in &demand.features {
        let mut attrs = f.attrs.clone();
        let found = f.geom.as_ref().and_then(point_of).and_then(|c| net.place(c, snap)).and_then(|pl| net.route(&s, &pl, &starts)).filter(|r| r.0 <= reach);
        match found {
            Some((c, k, _)) => {
                attrs.push(Value::Real((c * 100.0).round() / 100.0));
                attrs.extend(fac.features[k].attrs.iter().cloned());
            }
            None => {
                missed += 1;
                attrs.resize(out.fields.len(), Value::Null);
            }
        }
        attrs.resize(out.fields.len(), Value::Null);
        out.features.push(Feature { attrs, geom: f.geom.clone(), z: f.z.clone() });
    }
    if missed > 0 {
        ctx.notes.warn(format!("{} features on Demand reach no facility, within the limit, and have no facility.", fmt_count(missed as u64)));
    }
    ctx.notes.info(format!(
        "{} pieces of road and {} features shared among {} facilities",
        fmt_count(roads.len() as u64),
        fmt_count(out.len() as u64),
        starts.len()
    ));
    Ok(vec![("Demand".into(), out), ("Roads".into(), roads), ("Areas".into(), areas)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "NetworkBuilder",
        title: "Build network",
        hidden: false,
        group: Group::Transformer,
        category: "Network",
        desc: "Makes lines into a network that routes can follow: ends close together become one junction, an end that stops just short of a line is joined to it, lines are split where they cross or meet on the same level (a bridge passes over a road on another level), and lines that lie twice are taken away. Each piece gets the part of the network it belongs to; loose ends close to a line and parts not joined to the rest come out on Problems. For OpenStreetMap and Overture roads before Shortest route, Service areas and Allocate to nearest.",
        inputs: &["Lines"],
        outputs: &["Network", "Problems"],
        ports: None,
        params: BUILD_PARAMS,
        run: run_build,
        summary: |p| format!("ends within {} m", p.get("tolerance").unwrap_or_else(|| "0.5".into())),
    },
    NodeType {
        name: "ServiceAreas",
        title: "Service areas",
        hidden: false,
        group: Group::Transformer,
        category: "Network",
        desc: "What can be reached along the network from the facilities, or to them, within some minutes or metres: an area for each limit, around the roads reached, and the roads themselves. For all the facilities together, or each on its own: how many live within ten minutes of a store, where a new one would reach the most.",
        inputs: &["Network", "Facilities"],
        outputs: &["Areas", "Roads"],
        ports: None,
        params: SERVICE_PARAMS,
        run: run_service,
        summary: |p| format!("within {}", p.get("breaks").unwrap_or_else(|| "5, 10, 15".into())),
    },
    NodeType {
        name: "NetworkAllocation",
        title: "Allocate to nearest",
        hidden: false,
        group: Group::Transformer,
        category: "Network",
        desc: "Gives each piece of the network, and each feature on Demand such as the addresses or the people of a population grid, the facility nearest along the network, with the time or distance to it. With the areas of each facility around its roads: the catchment of each store, school or health centre, to count the customers of each with Statistics.",
        inputs: &["Network", "Facilities", "Demand"],
        outputs: &["Demand", "Roads", "Areas"],
        ports: None,
        params: ALLOCATE_PARAMS,
        run: run_allocate,
        summary: |p| if p.get("direction").as_deref() == Some("from") { "from the facilities".into() } else { "to the facilities".into() },
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use geo::line_string;

    fn lines(ls: Vec<LineString<f64>>) -> Table {
        Table {
            fields: vec![Field::new("id", FieldType::Int)],
            crs: Some(3006),
            features: ls.into_iter().enumerate().map(|(i, l)| Feature::new(vec![Value::Int(i as i64)], Some(Geometry::LineString(l)))).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn lines_that_cross_and_nearly_meet_become_a_network() {
        // A cross of two roads, one that stops 0.3 m short of the first, a bridge over the
        // second on another level, a road drawn twice, and one far away.
        let mut t = lines(vec![
            line_string![(x: 0.0, y: 50.0), (x: 100.0, y: 50.0)],
            line_string![(x: 50.0, y: 0.0), (x: 50.0, y: 100.0)],
            line_string![(x: 80.0, y: 50.3), (x: 80.0, y: 90.0)],
            line_string![(x: 30.0, y: 20.0), (x: 70.0, y: 20.0)],
            line_string![(x: 100.0, y: 50.0), (x: 0.0, y: 50.0)],
            line_string![(x: 500.0, y: 500.0), (x: 600.0, y: 500.0)],
        ]);
        t.fields.push(Field::new("layer", FieldType::Int));
        for (i, f) in t.features.iter_mut().enumerate() {
            f.attrs.push(Value::Int(if i == 3 { 1 } else { 0 }));
        }
        let (out, notes) = run_for_test("NetworkBuilder", &[("tolerance", "0.5"), ("level", "layer")], vec![("Lines", t)]).unwrap();
        let net = &out[0].1;
        // The first road in three (at 50 and 80), the second in two, the short one, the
        // bridge, and the far one; the twice drawn road is gone.
        assert_eq!(net.len(), 3 + 2 + 1 + 1 + 1, "{:?}", notes);
        let parts: std::collections::HashSet<i64> = net.features.iter().map(|f| f.attrs[2].as_f64().unwrap() as i64).collect();
        assert_eq!(parts.len(), 3, "the cross, the bridge, the far one");
        let problems = &out[1].1;
        assert_eq!(problems.features.iter().filter(|f| f.attrs[0].to_text().starts_with("A part")).count(), 2);
    }

    #[test]
    fn service_areas_and_allocation_along_a_grid_of_roads() {
        // A grid of roads 100 m apart, 10 × 10, and two shops at opposite corners.
        let mut ls = Vec::new();
        for k in 0..=10 {
            let v = k as f64 * 100.0;
            ls.push(line_string![(x: 0.0, y: v), (x: 1000.0, y: v)]);
            ls.push(line_string![(x: v, y: 0.0), (x: v, y: 1000.0)]);
        }
        let (built, _) = run_for_test("NetworkBuilder", &[], vec![("Lines", lines(ls))]).unwrap();
        let net = built[0].1.clone();
        let shops = Table {
            fields: vec![Field::new("shop", FieldType::Text)],
            crs: Some(3006),
            features: vec![
                Feature::new(vec![Value::Text("West".into())], Some(Geometry::Point(geo::Point::new(0.0, 0.0)))),
                Feature::new(vec![Value::Text("East".into())], Some(Geometry::Point(geo::Point::new(1000.0, 1000.0)))),
            ],
            ..Default::default()
        };
        let (out, _) =
            run_for_test("ServiceAreas", &[("breaks", "300, 600"), ("width", "20")], vec![("Network", net.clone()), ("Facilities", shops.clone())]).unwrap();
        let roads = &out[1].1;
        let within = |limit: f64| {
            roads
                .features
                .iter()
                .filter(|f| f.attrs[0] == Value::Real(limit))
                .map(|f| {
                    geo::Euclidean.length(match f.geom.as_ref().unwrap() {
                        Geometry::LineString(l) => l,
                        _ => unreachable!(),
                    })
                })
                .sum::<f64>()
        };
        use geo::Length;
        // Within 300 m of a corner along a grid: the roads with x + y ≤ 300, from each shop.
        let near = within(300.0);
        assert!((near - 2.0 * 1200.0).abs() < 1.0, "{}", near);
        assert_eq!(out[0].1.len(), 2);
        // Allocation: the houses nearer each shop.
        let houses = Table {
            fields: vec![Field::new("house", FieldType::Int)],
            crs: Some(3006),
            features: vec![
                Feature::new(vec![Value::Int(1)], Some(Geometry::Point(geo::Point::new(150.0, 100.0)))),
                Feature::new(vec![Value::Int(2)], Some(Geometry::Point(geo::Point::new(900.0, 850.0)))),
            ],
            ..Default::default()
        };
        let (out, _) = run_for_test("NetworkAllocation", &[], vec![("Network", net), ("Facilities", shops), ("Demand", houses)]).unwrap();
        let d = &out[0].1;
        let shop_at = d.field_index("facility_shop").unwrap();
        assert_eq!(d.features[0].attrs[shop_at], Value::Text("West".into()));
        assert_eq!(d.features[1].attrs[shop_at], Value::Text("East".into()));
        assert_eq!(d.features[0].attrs[1], Value::Real(250.0));
        assert_eq!(out[2].1.len(), 2, "an area for each shop");
    }
}
