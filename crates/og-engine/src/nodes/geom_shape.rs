//! Tools that change the shape of geometry: Buffer, Center point, Generalize, Extend lines
//! and Snap.

use super::*;
use crate::model::{err, fmt_count, Result};
use geo::{Coord, Geometry};
use rstar::primitives::{GeomWithData, Line as RLine};
use rstar::RTree;

// ---------- Buffer (Bufferer) ----------

const BUFFER_SIDES: &[(&str, &str)] = &[("both", "Both"), ("left", "Left of the line"), ("right", "Right of the line")];
const BUFFER_ENDS: &[(&str, &str)] = &[("round", "Round"), ("flat", "Flat")];

pub(super) const BUFFERER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "distance",
        label: "Distance",
        kind: ParamKind::Expression("Input"),
        default: "100",
        hint: "In metres. A number, or an expression such as width / 2. Negative shrinks areas",
    },
    ParamSpec {
        key: "side",
        label: "Side",
        kind: ParamKind::Choice(BUFFER_SIDES),
        default: "both",
        hint: "Of lines: one side only, left or right as the line is drawn, such as the land along one bank of a river. Areas and points get buffers all round",
    },
    ParamSpec { key: "ends", label: "Ends of lines", kind: ParamKind::Choice(BUFFER_ENDS), default: "round", hint: "Flat ends stop where the line stops" },
];

// ---------- Remove holes (HoleRemover) ----------

pub(super) const HOLE_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "smallest",
    label: "Holes smaller than",
    kind: ParamKind::Text,
    default: "",
    hint: "In square metres: holes smaller than this are filled, larger ones kept. Empty fills every hole",
}];

pub(super) fn run_holes(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let smallest = number_param(p, "smallest", "Holes smaller than")?;
    let geographic = ctx.geographic(table.crs).unwrap_or(false);
    let mut out = table.clone();
    let mut removed = 0usize;
    for f in out.features.iter_mut() {
        if let Some(g) = &f.geom {
            let (g, n) = geom::remove_holes(g, smallest, geographic);
            if n > 0 {
                removed += n;
                f.geom = Some(g);
                f.z = None;
            }
        }
    }
    ctx.notes.info(format!("{} holes filled", fmt_count(removed as u64)));
    Ok(vec![("Output".into(), out)])
}

/// Fails with a clear message if the stream is in degrees, where a distance in metres means nothing.
pub(super) fn need_metres(ctx: &NodeCtx, table: &Table, what: &str) -> Result<()> {
    match ctx.geographic(table.crs) {
        Some(true) => err(format!("{} works in metres, but the stream is in {}, which is in degrees. Put Reproject before it.", what, crs_label(table.crs))),
        _ => Ok(()),
    }
}

// ---------- Round shapes (Rounder) ----------

pub(super) const ROUNDER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "radius",
        label: "Radius in metres",
        kind: ParamKind::Text,
        default: "50",
        hint: "Corners become curves of this radius, and inlets, gaps and spits narrower than twice it go",
    },
    ParamSpec { key: "smallest", label: "Smallest part in m²", kind: ParamKind::Text, default: "", hint: "Parts and holes smaller go. Empty: keep all" },
];

pub(super) fn run_rounder(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    if table.has_geometry() {
        need_metres(ctx, table, "Round shapes")?;
    }
    let radius = number_param(p, "radius", "The radius")?.unwrap_or(50.0);
    if radius < 0.0 {
        return err("The radius must be 0 or more.");
    }
    let smallest = number_param(p, "smallest", "The smallest part")?.unwrap_or(0.0);
    let mut out = table.like();
    let (mut vanished, mut not_areas) = (0usize, 0usize);
    for f in &table.features {
        let Some(g) = &f.geom else {
            out.features.push(f.clone());
            continue;
        };
        if geom::family(g) != Family::Area {
            not_areas += 1;
            out.features.push(f.clone());
            continue;
        }
        match geom::soften(g, radius, smallest) {
            Some(r) => out.features.push(Feature { z: None, attrs: f.attrs.clone(), geom: Some(r) }),
            None => vanished += 1,
        }
    }
    if not_areas > 0 {
        ctx.notes.info(format!("{} features are not areas and were passed on as they are", fmt_count(not_areas as u64)));
    }
    if vanished > 0 {
        ctx.notes
            .warn(format!("{} areas were narrower than twice the radius, or smaller than the smallest part, and disappeared.", fmt_count(vanished as u64)));
    }
    Ok(vec![("Rounded".into(), out)])
}

pub(super) fn run_bufferer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let e = expr::compile(&p.need("distance", "Give the buffer distance.")?, &table.field_names())?;
    if table.has_geometry() {
        need_metres(ctx, table, "Buffer")?;
    }
    let geographic = ctx.geographic(table.crs);
    let side = match p.get("side").as_deref() {
        Some("left") => geom::Side::Left,
        Some("right") => geom::Side::Right,
        _ => geom::Side::Both,
    };
    let flat = p.get("ends").as_deref() == Some("flat");
    let mut out = table.like();
    let (mut no_distance, mut vanished) = (0usize, 0usize);
    for (i, f) in table.features.iter().enumerate() {
        let Some(g) = &f.geom else {
            out.features.push(f.clone());
            continue;
        };
        let d = match e.eval(&Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?.as_f64() {
            Some(d) => d,
            None => {
                no_distance += 1;
                out.features.push(f.clone());
                continue;
            }
        };
        match geom::buffer_with(g, d, flat, side) {
            Some(b) => out.features.push(Feature { z: None, attrs: f.attrs.clone(), geom: Some(b) }),
            None => vanished += 1,
        }
    }
    if no_distance > 0 {
        ctx.notes.warn(format!("{} features have no distance and were passed on unchanged.", fmt_count(no_distance as u64)));
    }
    if vanished > 0 {
        ctx.notes.warn(format!("{} features disappeared because the distance left nothing of them.", fmt_count(vanished as u64)));
    }
    Ok(vec![("Buffered".into(), out)])
}

// ---------- Replace with center point (CenterPointReplacer) ----------

pub(super) const CENTER_MODES: &[(&str, &str)] = &[("centroid", "Centre of mass"), ("inside", "A point inside"), ("bbox", "Centre of bounding box")];
pub(super) const CENTER_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "mode",
    label: "Point",
    kind: ParamKind::Choice(CENTER_MODES),
    default: "centroid",
    hint: "The centre of mass can fall outside a curved shape. A point inside never does",
}];

pub(super) fn run_center(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let mode = match p.get("mode").as_deref() {
        Some("inside") => Center::Inside,
        Some("bbox") => Center::BoundingBox,
        Some("centroid") | None => Center::Centroid,
        Some(other) => return err(format!("Unknown point type: {}.", other)),
    };
    let mut out = table.like();
    out.features =
        table.features.iter().map(|f| Feature { z: None, attrs: f.attrs.clone(), geom: f.geom.as_ref().and_then(|g| geom::center(g, mode)) }).collect();
    Ok(vec![("Point".into(), out)])
}

// ---------- Generalize (Generalizer) ----------

pub(super) const GENERALIZER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "tolerance",
        label: "Tolerance",
        kind: ParamKind::Text,
        default: "10",
        hint: "In the units of the coordinate system, normally metres. Vertices closer than this to the simplified line are removed",
    },
    ParamSpec {
        key: "topology",
        label: "Keep neighbours together",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "A boundary that two areas share is simplified once, so that no gaps or overlaps arise. The areas must share their vertices along it",
    },
];

pub(super) fn run_generalizer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let text = p.need("tolerance", "Give a tolerance.")?;
    let tolerance = parse_number(&text)
        .filter(|t| *t >= 0.0)
        .ok_or_else(|| GbError(format!("The tolerance must be a number that is zero or larger, not \"{}\".", text)))?;
    if ctx.geographic(table.crs) == Some(true) && table.has_geometry() {
        ctx.notes.warn(format!("The stream is in degrees, so the tolerance {} is taken as degrees. Reproject first to give it in metres.", text));
    }
    let (mut before, mut after) = (0usize, 0usize);
    let mut out = table.like();
    let geoms: Vec<Option<geo::Geometry<f64>>> = if p.flag("topology") {
        geom::generalize_shared(&table.features.iter().map(|f| f.geom.clone()).collect::<Vec<_>>(), tolerance)
    } else {
        table.features.iter().map(|f| f.geom.as_ref().map(|g| geom::generalize(g, tolerance))).collect()
    };
    for (f, g) in table.features.iter().zip(geoms) {
        before += f.geom.as_ref().map_or(0, geom::vertex_count);
        after += g.as_ref().map_or(0, geom::vertex_count);
        out.features.push(Feature { z: None, attrs: f.attrs.clone(), geom: g });
    }
    if before > 0 {
        ctx.notes.info(format!("{} vertices became {}", before, after));
    }
    Ok(vec![("Generalized".into(), out)])
}

// ---------- Extend lines ----------

pub(super) const EXTENDER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "distance",
        label: "Distance",
        kind: ParamKind::Text,
        default: "1",
        hint: "In the units of the coordinate system, such as metres in SWEREF 99 TM",
    },
    ParamSpec {
        key: "ends",
        label: "At",
        kind: ParamKind::Choice(&[("both", "Both ends"), ("start", "The start"), ("end", "The end")]),
        default: "both",
        hint: "Each end goes on in the direction of its last piece",
    },
];

/// Lengthens a line at its start, its end or both, straight on from the piece nearest the end.
pub(super) fn extend_line(line: &mut geo::LineString<f64>, d: f64, start: bool, end: bool) {
    let pts = &mut line.0;
    if pts.len() < 2 {
        return;
    }
    let out_from = |a: geo::Coord<f64>, b: geo::Coord<f64>| -> Option<geo::Coord<f64>> {
        let (dx, dy) = (a.x - b.x, a.y - b.y);
        let len = dx.hypot(dy);
        (len > 0.0).then(|| geo::coord! { x: a.x + dx / len * d, y: a.y + dy / len * d })
    };
    if start {
        // The first point that is not where the start is gives the direction.
        if let Some(b) = pts.iter().skip(1).find(|c| **c != pts[0]).copied() {
            if let Some(c) = out_from(pts[0], b) {
                pts[0] = c;
            }
        }
    }
    if end {
        let last = pts.len() - 1;
        if let Some(b) = pts.iter().rev().skip(1).find(|c| **c != pts[last]).copied() {
            if let Some(c) = out_from(pts[last], b) {
                pts[last] = c;
            }
        }
    }
}

pub(super) fn run_extender(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::Geometry as G;
    let table = input(inp, "Input");
    let Some(d) = number_param(p, "distance", "The distance")? else {
        return err("Write how far to extend the lines, for example 1.");
    };
    let ends = p.get("ends").unwrap_or_else(|| "both".into());
    let (start, end) = (ends != "end", ends != "start");
    let mut out = table.clone();
    let mut others = 0usize;
    for f in &mut out.features {
        match f.geom.as_mut() {
            Some(G::LineString(l)) => extend_line(l, d, start, end),
            Some(G::MultiLineString(m)) => m.0.iter_mut().for_each(|l| extend_line(l, d, start, end)),
            Some(G::Line(l)) => {
                let mut ls = geo::LineString::from(vec![l.start, l.end]);
                extend_line(&mut ls, d, start, end);
                f.geom = Some(G::LineString(ls));
            }
            Some(_) => others += 1,
            None => {}
        }
    }
    if others > 0 {
        ctx.notes.warn(format!("{} are not lines and were let through as they were", fmt_count(others as u64)));
    }
    if table.crs.is_some_and(|c| c == 4326 || c == 4258) {
        ctx.notes.warn("The stream is in degrees, so the distance is in degrees too. Reproject first to extend by metres.".to_string());
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Snap ----------

pub(super) const SNAP_TO: &[(&str, &str)] = &[("vertices", "The nearest point of a line or area"), ("edges", "The nearest place on a line or edge")];

pub(super) const SNAP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "tolerance",
        label: "Largest distance",
        kind: ParamKind::Text,
        default: "1",
        hint: "Points closer than this are moved. In the units of the coordinate system, such as metres",
    },
    ParamSpec {
        key: "to",
        label: "Snap to",
        kind: ParamKind::Choice(SNAP_TO),
        default: "vertices",
        hint: "With Fixed connected. Without it, the features snap to each other's points, and the first ones stay where they are",
    },
];

pub(super) type Seg = GeomWithData<RLine<[f64; 2]>, ()>;

/// Moves every coordinate of a geometry with a function, and takes away a point that lands
/// on the one before it.
pub(super) fn map_coords(g: &mut Geometry<f64>, f: &mut dyn FnMut(Coord<f64>) -> Coord<f64>) {
    fn ring(l: &mut geo::LineString<f64>, f: &mut dyn FnMut(Coord<f64>) -> Coord<f64>) {
        for c in l.0.iter_mut() {
            *c = f(*c);
        }
        l.0.dedup();
    }
    fn poly(p: &mut geo::Polygon<f64>, f: &mut dyn FnMut(Coord<f64>) -> Coord<f64>) {
        p.exterior_mut(|e| ring(e, f));
        p.interiors_mut(|is| {
            for i in is.iter_mut() {
                ring(i, f);
            }
        });
    }
    match g {
        Geometry::Point(p) => p.0 = f(p.0),
        Geometry::MultiPoint(m) => m.0.iter_mut().for_each(|p| p.0 = f(p.0)),
        Geometry::LineString(l) => ring(l, f),
        Geometry::MultiLineString(m) => m.0.iter_mut().for_each(|l| ring(l, f)),
        Geometry::Polygon(p) => poly(p, f),
        Geometry::MultiPolygon(m) => m.0.iter_mut().for_each(|p| poly(p, f)),
        Geometry::GeometryCollection(c) => c.0.iter_mut().for_each(|g| map_coords(g, f)),
        Geometry::Line(l) => {
            l.start = f(l.start);
            l.end = f(l.end);
        }
        Geometry::Rect(_) | Geometry::Triangle(_) => {}
    }
}

/// The straight pieces of the lines and edges of a geometry.
pub(super) fn segments(g: &Geometry<f64>, out: &mut Vec<Seg>) {
    let mut add = |l: &geo::LineString<f64>| {
        for w in l.0.windows(2) {
            if w[0] != w[1] {
                out.push(GeomWithData::new(RLine::new([w[0].x, w[0].y], [w[1].x, w[1].y]), ()));
            }
        }
    };
    match g {
        Geometry::LineString(l) => add(l),
        Geometry::MultiLineString(m) => m.0.iter().for_each(&mut add),
        Geometry::Polygon(p) => {
            add(p.exterior());
            p.interiors().iter().for_each(&mut add);
        }
        Geometry::MultiPolygon(m) => {
            for p in &m.0 {
                add(p.exterior());
                p.interiors().iter().for_each(&mut add);
            }
        }
        Geometry::GeometryCollection(c) => {
            for g in &c.0 {
                segments(g, out);
            }
        }
        _ => {}
    }
}

pub(super) fn run_snap(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use rstar::PointDistance;
    let table = input(inp, "Input");
    let fixed = input(inp, "Fixed");
    let tol = number_param(p, "tolerance", "The largest distance")?.unwrap_or(1.0);
    if tol <= 0.0 {
        return err("The largest distance must be more than 0.");
    }
    let moved_fixed = super::geom_overlay::same_crs(fixed, "Fixed", table.crs, ctx)?;
    let fixed = moved_fixed.as_ref().unwrap_or(fixed);
    let edges = p.get("to").as_deref() == Some("edges");
    let tol2 = tol * tol;
    let mut out = table.clone();
    let mut moved = 0usize;
    if fixed.has_geometry() {
        let points: Vec<[f64; 2]> = fixed
            .features
            .iter()
            .filter_map(|f| f.geom.as_ref())
            .flat_map(|g| geo::CoordsIter::coords_iter(g).map(|c| [c.x, c.y]).collect::<Vec<_>>())
            .collect();
        let tree = RTree::bulk_load(points);
        let mut segs: Vec<Seg> = Vec::new();
        if edges {
            for g in fixed.features.iter().filter_map(|f| f.geom.as_ref()) {
                segments(g, &mut segs);
            }
        }
        let seg_tree = RTree::bulk_load(segs);
        for f in out.features.iter_mut() {
            let Some(g) = f.geom.as_mut() else { continue };
            map_coords(g, &mut |c| {
                let q = [c.x, c.y];
                if let Some(t) = tree.nearest_neighbor(&q).filter(|t| t.distance_2(&q) <= tol2) {
                    if t[0] != c.x || t[1] != c.y {
                        moved += 1;
                    }
                    return Coord { x: t[0], y: t[1] };
                }
                if let Some(s) = seg_tree.nearest_neighbor(&q).filter(|s| s.distance_2(&q) <= tol2) {
                    let at = s.geom().nearest_point(&q);
                    moved += 1;
                    return Coord { x: at[0], y: at[1] };
                }
                c
            });
        }
    } else {
        // To each other: a point moves to one that came before it, if one is near enough.
        let mut tree: RTree<[f64; 2]> = RTree::new();
        for f in out.features.iter_mut() {
            let Some(g) = f.geom.as_mut() else { continue };
            let mut added: Vec<[f64; 2]> = Vec::new();
            map_coords(g, &mut |c| {
                let q = [c.x, c.y];
                if let Some(t) = tree.nearest_neighbor(&q).filter(|t| t.distance_2(&q) <= tol2) {
                    if t[0] != c.x || t[1] != c.y {
                        moved += 1;
                    }
                    return Coord { x: t[0], y: t[1] };
                }
                added.push(q);
                c
            });
            for a in added {
                tree.insert(a);
            }
        }
    }
    ctx.notes.info(format!("{} points moved", fmt_count(moved as u64)));
    Ok(vec![("Snapped".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "LineExtender",
        title: "Extend lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes lines longer at one or both ends, straight on in the direction of the end, by a distance in the units of the coordinate system.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: EXTENDER_PARAMS,
        run: run_extender,
        summary: |p| {
            let at = match p.get("ends").as_deref() {
                Some("start") => " at the start",
                Some("end") => " at the end",
                _ => "",
            };
            format!("+ {}{}", p.get("distance").unwrap_or_else(|| "1".into()), at)
        },
    },
    NodeType {
        name: "Snapper",
        title: "Snap",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Moves the points of the features that lie within a distance of the features on Fixed, which stay as they are: to their nearest point, or to the nearest place on their lines and edges. Without Fixed, the features snap to each other.",
        inputs: &["Input", "Fixed"],
        outputs: &["Snapped"],
        ports: None,
        params: SNAP_PARAMS,
        run: run_snap,
        summary: |p| format!("within {}", p.get("tolerance").unwrap_or_else(|| "1".into())),
    },
    NodeType {
        name: "Bufferer",
        title: "Buffer",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Replaces each geometry with the area within a distance of it. Points become discs, lines corridors, and areas grow, or shrink with a negative distance. Lines can be buffered on one side only, left or right as they are drawn, and with flat ends. Works in metres.",
        inputs: &["Input"],
        outputs: &["Buffered"],
        ports: None,
        params: BUFFERER_PARAMS,
        run: run_bufferer,
        summary: |p| {
            let side = match p.get("side").as_deref() {
                Some("left") => ", left",
                Some("right") => ", right",
                _ => "",
            };
            format!("{} m{}", p.get("distance").unwrap_or_default(), side)
        },
    },
    NodeType {
        name: "HoleRemover",
        title: "Remove holes",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Fills the holes of areas: all of them, or those smaller than a size, such as the courtyards of blocks or small clearings in a forest.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: HOLE_PARAMS,
        run: run_holes,
        summary: |p| p.get("smallest").filter(|s| !s.is_empty()).map(|s| format!("smaller than {} m²", s)).unwrap_or_else(|| "all".into()),
    },
    NodeType {
        name: "ShapeRounder",
        title: "Round shapes",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Rounds areas as in a drawing for children or a game map: every corner becomes a curve of the radius, narrow gaps and inlets are filled and narrow spits and points cut away. Parts and holes smaller than a size you give go. Works in metres; lines and points are passed on as they are.",
        inputs: &["Input"],
        outputs: &["Rounded"],
        ports: None,
        params: ROUNDER_PARAMS,
        run: run_rounder,
        summary: |p| format!("radius {} m", p.get("radius").unwrap_or_default()),
    },
    NodeType {
        name: "CenterPointReplacer",
        title: "Replace with center point",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Replaces each geometry with a single point: its centre of mass, a point guaranteed to lie inside it, or the centre of its bounding box.",
        inputs: &["Input"],
        outputs: &["Point"],
        ports: None,
        params: CENTER_PARAMS,
        run: run_center,
        summary: |p| choice_label(CENTER_MODES, &p.get("mode").unwrap_or_default()).to_lowercase(),
    },
    NodeType {
        name: "Generalizer",
        title: "Generalize",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Simplifies lines and area boundaries by removing vertices that change the shape by less than the tolerance (Douglas–Peucker). Points are left as they are.",
        inputs: &["Input"],
        outputs: &["Generalized"],
        ports: None,
        params: GENERALIZER_PARAMS,
        run: run_generalizer,
        summary: |p| format!("tolerance {}{}", p.get("tolerance").unwrap_or_default(), if p.flag("topology") { " · neighbours kept together" } else { "" }),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{line_string, Area, BoundingRect};

    fn one(g: Geometry<f64>) -> Table {
        Table { fields: vec![], crs: Some(3006), features: vec![Feature::new(vec![], Some(g))], ..Default::default() }
    }

    #[test]
    fn a_line_is_buffered_on_one_side_and_holes_are_filled() {
        // A line drawn east: left is north.
        let line = Geometry::LineString(line_string![(x: 0.0, y: 0.0), (x: 100.0, y: 0.0)]);
        let (out, _) = run_for_test("Bufferer", &[("distance", "10"), ("side", "left")], vec![("Input", one(line.clone()))]).unwrap();
        let g = out[0].1.features[0].geom.clone().unwrap();
        let r = g.bounding_rect().unwrap();
        assert!(r.min().y > -0.01 && (r.max().y - 10.0).abs() < 0.01, "{:?}", r);
        assert!((g.unsigned_area() - 1000.0).abs() < 1.0, "{}", g.unsigned_area());
        let (out, _) = run_for_test("Bufferer", &[("distance", "10"), ("side", "right")], vec![("Input", one(line.clone()))]).unwrap();
        let r = out[0].1.features[0].geom.clone().unwrap().bounding_rect().unwrap();
        assert!(r.max().y < 0.01 && (r.min().y + 10.0).abs() < 0.01, "{:?}", r);
        // Flat ends, both sides: a rectangle.
        let (out, _) = run_for_test("Bufferer", &[("distance", "10"), ("ends", "flat")], vec![("Input", one(line))]).unwrap();
        let g = out[0].1.features[0].geom.clone().unwrap();
        assert!((g.unsigned_area() - 2000.0).abs() < 1.0, "{}", g.unsigned_area());
        // A block with a courtyard of 100 m² and one of 2500 m².
        let block = Geometry::Polygon(geo::Polygon::new(
            line_string![(x: 0.0, y: 0.0), (x: 200.0, y: 0.0), (x: 200.0, y: 100.0), (x: 0.0, y: 100.0), (x: 0.0, y: 0.0)],
            vec![
                line_string![(x: 10.0, y: 10.0), (x: 20.0, y: 10.0), (x: 20.0, y: 20.0), (x: 10.0, y: 20.0), (x: 10.0, y: 10.0)],
                line_string![(x: 100.0, y: 10.0), (x: 150.0, y: 10.0), (x: 150.0, y: 60.0), (x: 100.0, y: 60.0), (x: 100.0, y: 10.0)],
            ],
        ));
        let (out, _) = run_for_test("HoleRemover", &[("smallest", "1000")], vec![("Input", one(block.clone()))]).unwrap();
        match &out[0].1.features[0].geom {
            Some(Geometry::Polygon(p)) => assert_eq!(p.interiors().len(), 1),
            g => panic!("{:?}", g),
        }
        let (out, _) = run_for_test("HoleRemover", &[], vec![("Input", one(block))]).unwrap();
        assert!(matches!(&out[0].1.features[0].geom, Some(Geometry::Polygon(p)) if p.interiors().is_empty()));
    }
}
