//! Tools that make new geometry from what comes in: Aggregate, Deaggregate, Remove geometry,
//! Create points from x and y, bounding boxes, polygons and lines from each other,
//! Thiessen polygons, Create grid, Geometry from text, Points to lines, Split lines into
//! segments, and Convex and Concave hull.

use super::*;
use crate::geomtext::{self, Kind};
use crate::model::{crs_label, err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use geo::{BoundingRect, Coord, Geometry, Intersects, LineString, Polygon};
use rstar::AABB;
use std::collections::HashMap;
use std::sync::Arc;

// ---------- Remove geometry (GeometryRemover) ----------

pub(super) fn run_geometry_remover(_p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let out = Table {
        mesh: None,
        fields: table.fields.clone(),
        crs: None,
        features: table.features.iter().map(|f| Feature { z: None, attrs: f.attrs.clone(), geom: None }).collect(),
        raster: false,
        image: None,
        grid: None,
    };
    Ok(vec![("Output".into(), out)])
}

// ---------- Create features (FeatureCreator) ----------

pub(super) const CREATE_KINDS: &[(&str, &str)] =
    &[("point", "Points"), ("line", "Lines"), ("polygon", "Areas"), ("text", "As WKT or GeoJSON, one on each line")];

pub(super) const CREATE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "kind", label: "Make", kind: ParamKind::Choice(CREATE_KINDS), default: "point", hint: "" },
    ParamSpec {
        key: "coordinates",
        label: "Coordinates",
        kind: ParamKind::Code(""),
        default: "",
        hint: "A feature on each line: x and y after each other, such as 319500 6399700 for a point, or 319500 6399700, 320100 6400200 for a line. An area is closed by itself. Or WKT, such as POINT(319500 6399700)",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "3006", hint: "" },
    ParamSpec { key: "name", label: "Names", kind: ParamKind::Text, default: "", hint: "Put in the attribute name, separated by commas, one for each feature in order" },
];

/// The numbers of a line of coordinates, with points or commas between them.
fn numbers_of(line: &str) -> std::result::Result<Vec<f64>, String> {
    line.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '(' || c == ')')
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("\"{}\" is not a number", t)))
        .collect()
}

pub(super) fn run_create(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let kind = p.get("kind").unwrap_or_else(|| "point".into());
    let text = p.exact("coordinates");
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    if lines.is_empty() {
        return err("Write the coordinates of the features, a feature on each line, such as 319500 6399700.");
    }
    let crs = p.crs("crs")?;
    let names: Vec<String> = p.get("name").map(|n| n.split(',').map(|v| v.trim().to_string()).collect()).unwrap_or_default();
    let mut out = Table { fields: vec![Field::new("id", FieldType::Int), Field::new("name", FieldType::Text)], crs, ..Default::default() };
    let mut text_crs = None;
    for (i, line) in lines.iter().enumerate() {
        let at = |e: String| GbError(format!("Line {}: {}", i + 1, e));
        let wkt = line.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '{');
        let (geom, z) = if kind == "text" || wkt {
            let (g, z, c) = geomtext::parse(line, None).map_err(|e| at(e.0))?;
            text_crs = text_crs.or(c);
            (g, z)
        } else {
            let v = numbers_of(line).map_err(at)?;
            if v.len() % 2 != 0 {
                return Err(at(format!("{} numbers do not make pairs of x and y.", v.len())));
            }
            let pts: Vec<Coord<f64>> = v.chunks(2).map(|c| Coord { x: c[0], y: c[1] }).collect();
            let g = match kind.as_str() {
                "line" => {
                    if pts.len() < 2 {
                        return Err(at("A line needs at least two points.".into()));
                    }
                    Geometry::LineString(LineString::new(pts))
                }
                "polygon" => {
                    let mut ring = pts;
                    if ring.len() >= 2 && ring.first() == ring.last() {
                        ring.pop();
                    }
                    if ring.len() < 3 {
                        return Err(at("An area needs at least three points.".into()));
                    }
                    ring.push(ring[0]);
                    Geometry::Polygon(Polygon::new(LineString::new(ring), vec![]))
                }
                _ => match pts.len() {
                    1 => Geometry::Point(pts[0].into()),
                    _ => Geometry::MultiPoint(geo::MultiPoint::new(pts.into_iter().map(Into::into).collect())),
                },
            };
            (g, None)
        };
        let name = names.get(i).filter(|n| !n.is_empty()).map_or(Value::Null, |n| Value::Text(n.clone()));
        out.features.push(Feature { attrs: vec![Value::Int(i as i64 + 1), name], geom: Some(geom), z: z.map(Arc::from) });
    }
    if out.crs.is_none() {
        out.crs = text_crs;
    }
    ctx.notes.info(format!("{} features in {}", fmt_count(out.features.len() as u64), crs_label(out.crs)));
    Ok(vec![("Output".into(), out)])
}

// ---------- Aggregate (Aggregator) ----------

pub(super) const AGGREGATOR_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "group_by",
        label: "Group by",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "Features with the same values become one. Empty puts all in one",
    },
    ParamSpec {
        key: "count",
        label: "Count attribute",
        kind: ParamKind::Text,
        default: "",
        hint: "An attribute with the number of features each one was made of, for example count. Empty adds none",
    },
];

/// Many geometries of one family as one multi-geometry.
pub(super) fn multi_of(parts: Vec<&geo::Geometry<f64>>) -> Option<geo::Geometry<f64>> {
    use geo::Geometry as G;
    let family = geom::family(parts.first()?);
    Some(match family {
        Family::Point => G::MultiPoint(geo::MultiPoint::new(
            parts
                .into_iter()
                .flat_map(|g| match g {
                    G::Point(p) => vec![*p],
                    G::MultiPoint(m) => m.0.clone(),
                    _ => vec![],
                })
                .collect(),
        )),
        Family::Line => G::MultiLineString(geo::MultiLineString::new(
            parts
                .into_iter()
                .flat_map(|g| match g {
                    G::LineString(l) => vec![l.clone()],
                    G::MultiLineString(m) => m.0.clone(),
                    G::Line(l) => vec![geo::LineString::from(*l)],
                    _ => vec![],
                })
                .collect(),
        )),
        Family::Area => G::MultiPolygon(geo::MultiPolygon::new(parts.into_iter().flat_map(geom::polygons_of).collect())),
        Family::Other => G::GeometryCollection(geo::GeometryCollection::from(parts.into_iter().cloned().collect::<Vec<_>>())),
    })
}

pub(super) fn run_aggregator(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    if table.fields.is_empty() && table.is_empty() {
        return Ok(vec![("Aggregated".into(), table.like())]);
    }
    let by = p.list("group_by");
    let idx: Vec<usize> = by.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;
    let count_name = p.get("count");
    if let Some(c) = &count_name {
        if by.contains(c) {
            return err(format!("The count attribute \"{}\" has the same name as a group-by attribute. Rename one of them.", c));
        }
    }
    let mut out = Table {
        mesh: None,
        fields: idx.iter().map(|i| table.fields[*i].clone()).collect(),
        crs: table.crs,
        features: Vec::new(),
        raster: false,
        image: None,
        grid: None,
    };
    if let Some(c) = &count_name {
        out.fields.push(Field::new(c.clone(), FieldType::Int));
    }
    let mut split = 0usize;
    for members in groups(table, &idx, |_| true) {
        // One multi-geometry per kind of geometry in the group. Points, lines and areas do
        // not go into one geometry.
        let mut families: Vec<(Option<Family>, Vec<usize>)> = Vec::new();
        for i in &members {
            let fam = table.features[*i].geom.as_ref().map(geom::family);
            match families.iter_mut().find(|x| x.0 == fam) {
                Some(x) => x.1.push(*i),
                None => families.push((fam, vec![*i])),
            }
        }
        if families.iter().filter(|f| f.0.is_some()).count() > 1 {
            split += 1;
        }
        for (_, part) in families {
            let geoms: Vec<&geo::Geometry<f64>> = part.iter().filter_map(|i| table.features[*i].geom.as_ref()).collect();
            let mut attrs: Vec<Value> = idx.iter().map(|i| table.features[part[0]].attrs[*i].clone()).collect();
            if count_name.is_some() {
                attrs.push(Value::Int(part.len() as i64));
            }
            out.features.push(Feature { z: None, attrs, geom: multi_of(geoms) });
        }
    }
    if split > 0 {
        ctx.notes.warn(format!("{} groups held more than one kind of geometry, and became one feature for each kind.", fmt_count(split as u64)));
    }
    Ok(vec![("Aggregated".into(), out)])
}

// ---------- Deaggregate (Deaggregator) ----------

pub(super) const DEAGGREGATOR_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "part",
    label: "Part number attribute",
    kind: ParamKind::Text,
    default: "",
    hint: "The number of the part within the feature it came from, 1, 2 and so on. Empty adds none",
}];

/// The single parts of a geometry. A single geometry is its own part.
pub(super) fn parts_of(g: &geo::Geometry<f64>) -> Vec<geo::Geometry<f64>> {
    use geo::Geometry as G;
    match g {
        G::MultiPoint(m) => m.0.iter().map(|p| G::Point(*p)).collect(),
        G::MultiLineString(m) => m.0.iter().map(|l| G::LineString(l.clone())).collect(),
        G::MultiPolygon(m) => m.0.iter().map(|p| G::Polygon(p.clone())).collect(),
        G::GeometryCollection(c) => c.0.iter().flat_map(parts_of).collect(),
        other => vec![other.clone()],
    }
}

pub(super) fn run_deaggregator(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let part_name = p.get("part");
    let mut out = table.like();
    let mut numbers = Vec::new();
    for f in &table.features {
        match &f.geom {
            Some(g) => {
                for (n, part) in parts_of(g).into_iter().enumerate() {
                    out.features.push(Feature { z: None, attrs: f.attrs.clone(), geom: Some(part) });
                    numbers.push(Value::Int(n as i64 + 1));
                }
            }
            None => {
                out.features.push(f.clone());
                numbers.push(Value::Null);
            }
        }
    }
    if out.len() != table.len() {
        ctx.notes.info(format!("{} features became {}", fmt_count(table.len() as u64), fmt_count(out.len() as u64)));
    }
    if let Some(name) = part_name {
        out = with_column(&out, &name, numbers);
    }
    Ok(vec![("Parts".into(), out)])
}

// ---------- Create points from x and y (PointCreator) ----------

pub(super) const POINT_CREATOR_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "x", label: "x attribute", kind: ParamKind::Attribute("Input"), default: "", hint: "East, or longitude" },
    ParamSpec { key: "y", label: "y attribute", kind: ParamKind::Attribute("Input"), default: "", hint: "North, or latitude" },
    ParamSpec {
        key: "crs",
        label: "Coordinate system",
        kind: ParamKind::Crs,
        default: "",
        hint: "Of the x and y values, for example EPSG:3006 for SWEREF 99 TM or EPSG:4326 for degrees. Empty keeps the one the stream has",
    },
];

/// A value as a coordinate: a number, or a text that reads as one, with a decimal comma too.
pub(super) fn coordinate(v: &Value) -> Option<f64> {
    let x = match v {
        Value::Int(i) => *i as f64,
        Value::Real(r) => *r,
        Value::Text(t) => parse_number(t.trim())?,
        _ => return None,
    };
    x.is_finite().then_some(x)
}

pub(super) fn run_point_creator(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let xa = p.need("x", "Choose the attribute with the x values.")?;
    let ya = p.need("y", "Choose the attribute with the y values.")?;
    let (xi, yi) = (table.need_field(&xa)?, table.need_field(&ya)?);
    let crs = match p.crs("crs")? {
        Some(c) => Some(c),
        None if table.crs.is_some() => table.crs,
        None => return err("Choose the coordinate system of the x and y values, for example EPSG:3006."),
    };
    if let (Some(given), Some(had)) = (p.crs("crs")?, table.crs) {
        if given != had && table.features.iter().any(|f| f.geom.is_some()) {
            ctx.notes.info(format!("the geometry in {} was replaced by points in {}", crs_label(Some(had)), crs_label(Some(given))));
        }
    }
    let mut points = Table { mesh: None, fields: table.fields.clone(), crs, features: Vec::new(), raster: false, image: None, grid: None };
    let mut failed = table.like();
    for f in &table.features {
        match (coordinate(&f.attrs[xi]), coordinate(&f.attrs[yi])) {
            (Some(x), Some(y)) => points.features.push(Feature { z: None, attrs: f.attrs.clone(), geom: Some(geo::Geometry::Point(geo::Point::new(x, y))) }),
            _ => failed.features.push(f.clone()),
        }
    }
    if !failed.is_empty() {
        ctx.notes.warn(format!("{} features have no x or y that is a number, and went to Failed.", fmt_count(failed.len() as u64)));
    }
    Ok(vec![("Points".into(), points), ("Failed".into(), failed)])
}

// ---------- Bounding boxes ----------

/// The box around a geometry as a geometry: a polygon, or a line or a point when the box
/// has no width or no height.
pub(super) fn box_geometry(r: geo::Rect<f64>) -> Geometry<f64> {
    let (a, b) = (r.min(), r.max());
    if a.x == b.x && a.y == b.y {
        Geometry::Point(geo::Point::from(a))
    } else if a.x == b.x || a.y == b.y {
        Geometry::LineString(geo::LineString::from(vec![a, b]))
    } else {
        Geometry::Polygon(r.to_polygon())
    }
}

pub(super) fn grow(b: Option<geo::Rect<f64>>, r: geo::Rect<f64>) -> geo::Rect<f64> {
    match b {
        None => r,
        Some(b) => geo::Rect::new(
            geo::coord! { x: b.min().x.min(r.min().x), y: b.min().y.min(r.min().y) },
            geo::coord! { x: b.max().x.max(r.max().x), y: b.max().y.max(r.max().y) },
        ),
    }
}

pub(super) const BOX_CORNERS: &[(&str, &str)] = &[("false", "No"), ("true", "Yes: xmin, ymin, xmax and ymax")];

pub(super) const BOX_REPLACER_PARAMS: &[ParamSpec] =
    &[ParamSpec { key: "corners", label: "Add the corners as attributes", kind: ParamKind::Choice(BOX_CORNERS), default: "false", hint: "" }];

/// Adds xmin, ymin, xmax and ymax with the corners of each box.
pub(super) fn add_corners(mut out: Table, boxes: &[Option<geo::Rect<f64>>]) -> Table {
    for (k, name) in ["xmin", "ymin", "xmax", "ymax"].into_iter().enumerate() {
        let values: Vec<Value> = boxes
            .iter()
            .map(|b| match b {
                Some(r) => Value::Real([r.min().x, r.min().y, r.max().x, r.max().y][k]),
                None => Value::Null,
            })
            .collect();
        out = with_column(&out, name, values);
        if let Some(i) = out.field_index(name) {
            out.fields[i].ty = FieldType::Real;
        }
    }
    out
}

pub(super) fn run_box_replacer(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::BoundingRect;
    let table = input(inp, "Input");
    let boxes: Vec<Option<geo::Rect<f64>>> = table.features.iter().map(|f| f.geom.as_ref().and_then(|g| g.bounding_rect())).collect();
    let mut out = table.clone();
    out.raster = false;
    out.image = None;
    for (f, b) in out.features.iter_mut().zip(&boxes) {
        f.geom = b.map(box_geometry);
    }
    if p.flag("corners") {
        out = add_corners(out, &boxes);
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) const BOX_ACCUMULATOR_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "group_by",
        label: "Group by",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "One box for each group of features with the same values. Empty gives one box around all",
    },
    ParamSpec {
        key: "count",
        label: "Count attribute",
        kind: ParamKind::Text,
        default: "count",
        hint: "How many features are in each box. Empty leaves it out",
    },
    ParamSpec { key: "corners", label: "Add the corners as attributes", kind: ParamKind::Choice(BOX_CORNERS), default: "false", hint: "" },
];

pub(super) fn run_box_accumulator(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::BoundingRect;
    let table = input(inp, "Input");
    let by = p.list("group_by");
    let idx: Vec<usize> = by.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;
    let count_name = p.get("count").filter(|c| !c.trim().is_empty());
    let mut out = Table {
        mesh: None,
        fields: idx.iter().map(|i| table.fields[*i].clone()).collect(),
        crs: table.crs,
        features: Vec::new(),
        raster: false,
        image: None,
        grid: None,
    };
    if let Some(c) = &count_name {
        if by.contains(c) {
            return err(format!("The count attribute {} has the same name as a group-by attribute. Rename one of them.", c));
        }
        out.fields.push(Field::new(c.clone(), FieldType::Int));
    }
    let mut boxes = Vec::new();
    for members in groups(table, &idx, |f| f.geom.is_some()) {
        let mut b: Option<geo::Rect<f64>> = None;
        for i in &members {
            if let Some(r) = table.features[*i].geom.as_ref().and_then(|g| g.bounding_rect()) {
                b = Some(grow(b, r));
            }
        }
        let mut attrs: Vec<Value> = idx.iter().map(|i| table.features[members[0]].attrs[*i].clone()).collect();
        if count_name.is_some() {
            attrs.push(Value::Int(members.len() as i64));
        }
        out.features.push(Feature { z: None, attrs, geom: b.map(box_geometry) });
        boxes.push(b);
    }
    if p.flag("corners") {
        out = add_corners(out, &boxes);
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Polygons to lines, lines to polygons ----------

pub(super) fn run_polygon_to_line(_p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::Geometry as G;
    let table = input(inp, "Input");
    let (mut lines, mut other) = (table.like(), table.like());
    lines.raster = false;
    let rings = |poly: &geo::Polygon<f64>| -> Vec<geo::LineString<f64>> {
        let mut v = vec![poly.exterior().clone()];
        v.extend(poly.interiors().iter().cloned());
        v
    };
    for f in &table.features {
        let parts: Option<Vec<geo::LineString<f64>>> = match &f.geom {
            Some(G::Polygon(poly)) => Some(rings(poly)),
            Some(G::MultiPolygon(m)) => Some(m.0.iter().flat_map(rings).collect()),
            Some(G::Rect(r)) => Some(rings(&r.to_polygon())),
            Some(G::Triangle(t)) => Some(rings(&t.to_polygon())),
            _ => None,
        };
        match parts {
            Some(mut parts) => {
                let geom = if parts.len() == 1 { G::LineString(parts.remove(0)) } else { G::MultiLineString(geo::MultiLineString::new(parts)) };
                lines.features.push(Feature { z: None, attrs: f.attrs.clone(), geom: Some(geom) });
            }
            None => other.features.push(f.clone()),
        }
    }
    if !other.is_empty() {
        ctx.notes.info(format!("{} without areas went to Other", fmt_count(other.len() as u64)));
    }
    Ok(vec![("Lines".into(), lines), ("Other".into(), other)])
}

pub(super) const LINE_TO_POLYGON_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "tolerance",
    label: "Ends that meet within",
    kind: ParamKind::Text,
    default: "0",
    hint: "In the units of the coordinate system. Ends this close count as one point, so that a line that almost closes becomes an area",
}];

/// Joins the parts of a line end to end into closed rings. None when a part cannot be
/// made part of a ring.
pub(super) fn rings_of(parts: Vec<Vec<geo::Coord<f64>>>, tol: f64) -> Option<Vec<geo::LineString<f64>>> {
    let same = |a: geo::Coord<f64>, b: geo::Coord<f64>| (a.x - b.x).hypot(a.y - b.y) <= tol;
    let mut left: Vec<Vec<geo::Coord<f64>>> = parts.into_iter().filter(|p| p.len() >= 2).collect();
    let mut rings = Vec::new();
    while let Some(mut ring) = left.pop() {
        loop {
            let (first, last) = (ring[0], *ring.last().expect("two points at least"));
            if ring.len() >= 3 && same(first, last) {
                *ring.last_mut().expect("there") = first;
                break;
            }
            // Another part that starts or ends where this one ends.
            let found = left.iter().position(|p| same(p[0], last) || same(*p.last().expect("there"), last))?;
            let mut next = left.swap_remove(found);
            if !same(next[0], last) {
                next.reverse();
            }
            // The meeting point is taken from the part that comes next.
            ring.pop();
            ring.extend(next);
        }
        if ring.len() < 4 {
            return None;
        }
        rings.push(geo::LineString::new(ring));
    }
    if rings.is_empty() {
        None
    } else {
        Some(rings)
    }
}

pub(super) fn run_line_to_polygon(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::Geometry as G;
    let table = input(inp, "Input");
    let tol = number_param(p, "tolerance", "The distance")?.unwrap_or(0.0).max(0.0);
    let (mut areas, mut failed) = (table.like(), table.like());
    for f in &table.features {
        let parts: Vec<Vec<geo::Coord<f64>>> = match &f.geom {
            Some(G::LineString(l)) => vec![l.0.clone()],
            Some(G::MultiLineString(m)) => m.0.iter().map(|l| l.0.clone()).collect(),
            _ => Vec::new(),
        };
        match rings_of(parts, tol) {
            Some(rings) => {
                let mut polys: Vec<geo::Polygon<f64>> = rings.into_iter().map(|r| geo::Polygon::new(r, vec![])).collect();
                let geom = if polys.len() == 1 { G::Polygon(polys.remove(0)) } else { G::MultiPolygon(geo::MultiPolygon::new(polys)) };
                areas.features.push(Feature { z: None, attrs: f.attrs.clone(), geom: Some(geom) });
            }
            None => failed.features.push(f.clone()),
        }
    }
    if !failed.is_empty() {
        ctx.notes.warn(format!(
            "{} could not be made into areas: they are not lines, or their ends do not meet. They went to Failed",
            fmt_count(failed.len() as u64)
        ));
    }
    Ok(vec![("Polygons".into(), areas), ("Failed".into(), failed)])
}

// ---------- Thiessen polygons ----------

pub(super) const THIESSEN_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "margin",
    label: "Margin in %",
    kind: ParamKind::Text,
    default: "10",
    hint: "How far the outer areas reach beyond the points, as a share of the larger side of the area the points cover. Not used when areas are connected to Boundary",
}];

pub(super) fn run_thiessen(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::{BooleanOps, BoundingRect};
    use spade::{DelaunayTriangulation, Point2, Triangulation};
    let table = input(inp, "Points");
    let boundary = input(inp, "Boundary");
    let margin = number_param(p, "margin", "The margin")?.unwrap_or(10.0).max(0.0);
    let mut out = table.like();
    out.raster = false;
    // The points, by the feature they come from.
    let mut points: Vec<(usize, f64, f64)> = Vec::new();
    let mut not_points = 0usize;
    for (i, f) in table.features.iter().enumerate() {
        match &f.geom {
            Some(Geometry::Point(pt)) => points.push((i, pt.x(), pt.y())),
            Some(Geometry::MultiPoint(m)) if m.0.len() == 1 => points.push((i, m.0[0].x(), m.0[0].y())),
            Some(_) => not_points += 1,
            None => {}
        }
    }
    if not_points > 0 {
        ctx.notes.warn(format!("{} are not single points and were left out", fmt_count(not_points as u64)));
    }
    if points.is_empty() {
        return Ok(vec![("Polygons".into(), out)]);
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (_, x, y) in &points {
        (x0, y0, x1, y1) = (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y));
    }
    let side = (x1 - x0).max(y1 - y0).max(1.0);
    // The area the cells are cut to: the boundary when there is one, otherwise a box around
    // the points with the margin.
    let areas: Vec<geo::Polygon<f64>> = boundary
        .features
        .iter()
        .flat_map(|f| match &f.geom {
            Some(Geometry::Polygon(p)) => vec![p.clone()],
            Some(Geometry::MultiPolygon(m)) => m.0.clone(),
            _ => vec![],
        })
        .collect();
    if !areas.is_empty() && boundary.crs != table.crs {
        ctx.notes.warn(format!("The boundary is in {} and the points in {}. Reproject one of them first.", crs_label(boundary.crs), crs_label(table.crs)));
    }
    let clip: geo::MultiPolygon<f64> = if areas.is_empty() {
        let m = side * margin / 100.0;
        geo::MultiPolygon::new(vec![geo::Rect::new(geo::coord! { x: x0 - m, y: y0 - m }, geo::coord! { x: x1 + m, y: y1 + m }).to_polygon()])
    } else {
        geo::unary_union(areas.iter())
    };
    let reach = clip.bounding_rect().map_or(side, |r| r.width().max(r.height())).max(side);
    // Four points far away, so that every cell of a real point is closed.
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let far = reach * 100.0;
    let mut tri: DelaunayTriangulation<Point2<f64>> = DelaunayTriangulation::new();
    for (dx, dy) in [(-far, -far), (far, -far), (far, far), (-far, far)] {
        tri.insert(Point2::new(cx + dx, cy + dy)).map_err(|e| GbError(format!("Cannot make the areas: {:?}", e)))?;
    }
    let mut owners: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, x, y) in &points {
        let h = tri.insert(Point2::new(*x, *y)).map_err(|e| GbError(format!("A point cannot be used: {:?}", e)))?;
        owners.entry(h.index()).or_default().push(*i);
    }
    let mut cells: Vec<(usize, Geometry<f64>)> = Vec::new();
    let mut shared = 0usize;
    for face in tri.voronoi_faces() {
        let v = face.as_delaunay_vertex();
        let Some(feats) = owners.get(&v.fix().index()) else { continue };
        let ring: Option<Vec<geo::Coord<f64>>> = face.adjacent_edges().map(|e| e.from().position().map(|p| geo::coord! { x: p.x, y: p.y })).collect();
        let Some(mut ring) = ring else { continue };
        if ring.len() < 3 {
            continue;
        }
        ring.push(ring[0]);
        let cell = geo::Polygon::new(geo::LineString::new(ring), vec![]);
        let cut = geo::MultiPolygon::new(vec![cell]).intersection(&clip);
        if cut.0.is_empty() {
            continue;
        }
        let geom = if cut.0.len() == 1 { Geometry::Polygon(cut.0[0].clone()) } else { Geometry::MultiPolygon(cut) };
        shared += feats.len() - 1;
        for i in feats {
            cells.push((*i, geom.clone()));
        }
    }
    cells.sort_by_key(|c| c.0);
    for (i, g) in cells {
        out.features.push(Feature { z: None, attrs: table.features[i].attrs.clone(), geom: Some(g) });
    }
    if shared > 0 {
        ctx.notes.warn(format!("{} points lie where another point is, and share its area", fmt_count(shared as u64)));
    }
    Ok(vec![("Polygons".into(), out)])
}

// ---------- Create grid ----------

pub(super) const SHAPES: &[(&str, &str)] = &[("square", "Squares"), ("hexagon", "Hexagons")];
pub(super) const GRID_KEEP: &[(&str, &str)] = &[("touching", "Cells that touch the features"), ("all", "All cells in the extent")];

pub(super) const GRID_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "shape", label: "Shape", kind: ParamKind::Choice(SHAPES), default: "square", hint: "" },
    ParamSpec {
        key: "size",
        label: "Size",
        kind: ParamKind::Text,
        default: "1000",
        hint: "The side of a square, or the width of a hexagon from side to side, in the units of the coordinate system",
    },
    ParamSpec { key: "keep", label: "Keep", kind: ParamKind::Choice(GRID_KEEP), default: "touching", hint: "" },
    ParamSpec {
        key: "snap",
        label: "Start at whole sizes",
        kind: ParamKind::Choice(&[("true", "Yes"), ("false", "No")]),
        default: "true",
        hint: "Lines up the grid with multiples of the size, so that grids made from different data fit together",
    },
];

pub(super) fn run_grid(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Extent");
    let size = number_param(p, "size", "The size")?.unwrap_or(1000.0);
    if size <= 0.0 {
        return err("The size must be larger than 0.");
    }
    let hex = p.get("shape").as_deref() == Some("hexagon");
    let all = p.get("keep").as_deref() == Some("all");
    let snap = p.get("snap").as_deref() != Some("false");
    if ctx.geographic(table.crs) == Some(true) {
        ctx.notes.warn(format!("The size is in degrees, since the stream is in {}. Reproject first to get a grid in metres.", crs_label(table.crs)));
    }
    let mut out = Table {
        fields: vec![Field::new("cell_id", FieldType::Int), Field::new("row", FieldType::Int), Field::new("col", FieldType::Int)],
        crs: table.crs,
        ..Default::default()
    };
    let ext = table.features.iter().filter_map(|f| f.geom.as_ref()?.bounding_rect()).reduce(|a, b| {
        geo::Rect::new(Coord { x: a.min().x.min(b.min().x), y: a.min().y.min(b.min().y) }, Coord { x: a.max().x.max(b.max().x), y: a.max().y.max(b.max().y) })
    });
    let Some(ext) = ext else {
        ctx.notes.warn("Nothing came in on Extent, so there is no grid.".to_string());
        return Ok(vec![("Grid".into(), out)]);
    };
    let (mut x0, mut y0, x1, y1) = (ext.min().x, ext.min().y, ext.max().x, ext.max().y);
    // Hexagons with flat tops: the corners are `r` from the middle, the columns 1.5 r apart,
    // and the rows `size` apart, every second column half a row up.
    let r = size / 3f64.sqrt();
    let (dx, dy) = if hex { (1.5 * r, size) } else { (size, size) };
    if snap {
        x0 = (x0 / dx).floor() * dx;
        y0 = (y0 / dy).floor() * dy;
    }
    // Hexagons get a column and a row more, since their edges zigzag.
    let cols = (((x1 - x0) / dx).floor() as i64 + 1).max(1) + i64::from(hex);
    let rows = (((y1 - y0) / dy).floor() as i64 + 1).max(1) + i64::from(hex);
    if cols.saturating_mul(rows) > 5_000_000 {
        return err(format!("That would be {} cells. Choose a larger size: at most 5 million cells are made.", fmt_count((cols.saturating_mul(rows)) as u64)));
    }
    let index = box_index(table.features.iter().map(|f| f.geom.as_ref()));
    let mut id = 0i64;
    for row in 0..rows {
        for col in 0..cols {
            let poly = if hex {
                let cx = x0 + col as f64 * dx;
                let cy = y0 + row as f64 * dy + if col % 2 == 1 { dy / 2.0 } else { 0.0 };
                let ring: Vec<Coord<f64>> = (0..=6)
                    .map(|k| {
                        let a = std::f64::consts::PI / 3.0 * (k % 6) as f64;
                        Coord { x: cx + r * a.cos(), y: cy + r * a.sin() }
                    })
                    .collect();
                Polygon::new(LineString::new(ring), vec![])
            } else {
                let (ax, ay) = (x0 + col as f64 * dx, y0 + row as f64 * dy);
                geo::Rect::new(Coord { x: ax, y: ay }, Coord { x: ax + dx, y: ay + dy }).to_polygon()
            };
            let Some(b) = poly.bounding_rect() else { continue };
            if b.min().x > x1 || b.min().y > y1 || b.max().x < ext.min().x || b.max().y < ext.min().y {
                continue;
            }
            if !all {
                let env = AABB::from_corners([b.min().x, b.min().y], [b.max().x, b.max().y]);
                let touches = index.locate_in_envelope_intersecting(&env).any(|h| table.features[h.data].geom.as_ref().is_some_and(|g| poly.intersects(g)));
                if !touches {
                    continue;
                }
            }
            id += 1;
            out.features.push(Feature { z: None, attrs: vec![Value::Int(id), Value::Int(row + 1), Value::Int(col + 1)], geom: Some(Geometry::Polygon(poly)) });
        }
        if ctx.stopped() {
            return err("Stopped.");
        }
    }
    Ok(vec![("Grid".into(), out)])
}

pub(super) const TEXT_KINDS: &[(&str, &str)] = &[
    ("auto", "As the text is: WKT, GeoJSON or Esri JSON"),
    ("wkt", "WKT, such as POINT (12.5 57.7)"),
    ("geojson", "GeoJSON"),
    ("esri", "Esri JSON, as from ArcGIS"),
    ("polyline", "Encoded polyline, as routes from OpenTripPlanner and OSRM"),
    ("polyline6", "Encoded polyline with 6 decimals, as from Valhalla"),
    ("flexpolyline", "Flexible polyline, as routes from HERE"),
    ("flexpolyline_area", "Flexible polyline as an area, as isolines from HERE"),
    ("points", "A JSON list of points, as a route from TomTom: a line"),
    ("points_area", "A JSON list of points as an area, as a reachable range from TomTom"),
];

pub(super) const FROM_TEXT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attribute",
        label: "Geometry in",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The attribute with the geometry as text, such as Geometry.SWEREF99TM from Trafikverket or geometry from an API",
    },
    ParamSpec { key: "kind", label: "Written as", kind: ParamKind::Choice(TEXT_KINDS), default: "auto", hint: "" },
    ParamSpec {
        key: "crs",
        label: "Coordinate system",
        kind: ParamKind::Crs,
        default: "",
        hint: "Of the coordinates in the text. Empty: as the text says (SRID=, spatialReference), WGS 84 for GeoJSON, or else none",
    },
    ParamSpec { key: "keep", label: "Keep the text attribute", kind: ParamKind::Choice(YES_NO), default: "false", hint: "" },
];

// ---------- Geometry to text (GeometryToText) ----------

pub(super) const TEXT_FORMATS: &[(&str, &str)] = &[("wkt", "WKT, as POINT (319500 6399700)"), ("ewkt", "EWKT, with SRID= in front"), ("geojson", "GeoJSON")];

pub(super) const TO_TEXT_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "format", label: "As", kind: ParamKind::Choice(TEXT_FORMATS), default: "wkt", hint: "" },
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "wkt", hint: "The attribute that gets the text" },
    ParamSpec {
        key: "decimals",
        label: "Decimals",
        kind: ParamKind::Text,
        default: "",
        hint: "Of the coordinates, such as 2 for centimetres in metres. Empty: as they are",
    },
    ParamSpec {
        key: "heights",
        label: "Heights",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "With Z where the geometry has heights, as POINT Z (1 2 3)",
    },
    ParamSpec { key: "keep_geometry", label: "Keep the geometry", kind: ParamKind::Choice(YES_NO), default: "true", hint: "" },
];

pub(super) fn run_to_text(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    use wkt::ToWkt;
    let table = input(inp, "Input");
    let name = p.get("name").unwrap_or_else(|| "wkt".into());
    let format = p.get("format").unwrap_or_else(|| "wkt".into());
    let decimals = number_param(p, "decimals", "The decimals")?.map(|d| d.clamp(0.0, 15.0) as i32);
    let heights = p.get("heights").as_deref() != Some("false");
    let keep = p.get("keep_geometry").as_deref() != Some("false");
    let mut out = table.like();
    let at = super::geom_inspect::column(&mut out, &name, FieldType::Text);
    let round = |v: f64| decimals.map_or(v, |d| (v * 10f64.powi(d)).round() / 10f64.powi(d));
    for f in &table.features {
        let mut g = f.clone();
        g.attrs.resize(out.fields.len(), Value::Null);
        if let Some(geom) = &f.geom {
            let geom = decimals.map_or_else(|| geom.clone(), |d| crate::geom::rounded(geom, d));
            let z: Option<Vec<f64>> = f.heights().filter(|_| heights).map(|z| z.iter().map(|v| round(*v)).collect());
            let text = match format.as_str() {
                "geojson" => {
                    let mut j = crate::nodes::geojson(&geom);
                    // GeoJSON takes a height as the third number of each position.
                    if let Some(z) = &z {
                        let mut k = 0usize;
                        fn put(v: &mut serde_json::Value, z: &[f64], k: &mut usize) {
                            match v {
                                serde_json::Value::Array(a) if a.first().is_some_and(|x| x.is_number()) => {
                                    a.push(serde_json::json!(z.get(*k).copied().unwrap_or(0.0)));
                                    *k += 1;
                                }
                                serde_json::Value::Array(a) => a.iter_mut().for_each(|x| put(x, z, k)),
                                serde_json::Value::Object(o) => {
                                    if let Some(c) = o.get_mut("coordinates") {
                                        put(c, z, k);
                                    }
                                    if let Some(serde_json::Value::Array(gs)) = o.get_mut("geometries") {
                                        gs.iter_mut().for_each(|x| put(x, z, k));
                                    }
                                }
                                _ => {}
                            }
                        }
                        put(&mut j, z, &mut k);
                    }
                    j.to_string()
                }
                _ => {
                    let w = match &z {
                        Some(z) => crate::geom::wkt_z(&geom, z),
                        None => geom.wkt_string(),
                    };
                    match (format.as_str(), table.crs) {
                        ("ewkt", Some(c)) => format!("SRID={};{}", c, w),
                        _ => w,
                    }
                }
            };
            g.attrs[at] = Value::Text(text);
        }
        if !keep {
            g.geom = None;
            g.z = None;
        }
        out.features.push(g);
    }
    if !keep {
        out.crs = None;
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) fn run_from_text(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute with the geometry as text.")?;
    let at = table.need_field(&attr)?;
    let kind = match p.get("kind").as_deref() {
        Some("wkt") => Some(Kind::Wkt),
        Some("geojson") => Some(Kind::GeoJson),
        Some("esri") => Some(Kind::EsriJson),
        Some("polyline") => Some(Kind::Polyline(5)),
        Some("polyline6") => Some(Kind::Polyline(6)),
        Some("flexpolyline") => Some(Kind::FlexPolyline(false)),
        Some("flexpolyline_area") => Some(Kind::FlexPolyline(true)),
        Some("points") => Some(Kind::PointList(false)),
        Some("points_area") => Some(Kind::PointList(true)),
        _ => None,
    };
    let chosen = p.crs("crs")?;
    let keep = p.flag("keep");
    let fields: Vec<Field> = table.fields.iter().enumerate().filter(|(i, _)| keep || *i != at).map(|(_, f)| f.clone()).collect();
    let mut out = Table { fields, crs: None, ..Table::default() };
    let mut failed = table.like();
    failed.fields.push(Field::new("_error", FieldType::Text));
    let mut found: HashMap<u32, usize> = HashMap::new();
    let mut geojson_seen = false;
    for f in &table.features {
        let text = match &f.attrs[at] {
            Value::Null => String::new(),
            v => v.to_text(),
        };
        let parsed = if text.trim().is_empty() { Err(GbError("The attribute is empty.".into())) } else { geomtext::parse(&text, kind) };
        match parsed {
            Ok((g, z, crs)) => {
                if let Some(c) = crs {
                    *found.entry(c).or_default() += 1;
                }
                geojson_seen |= kind == Some(Kind::GeoJson) || (kind.is_none() && text.trim_start().starts_with('{') && text.contains("\"type\""));
                let attrs = f.attrs.iter().enumerate().filter(|(i, _)| keep || *i != at).map(|(_, v)| v.clone()).collect();
                out.features.push(Feature { attrs, geom: Some(g), z: z.map(Arc::from) });
            }
            Err(e) => {
                let mut g = f.clone();
                g.attrs.push(Value::Text(e.0));
                failed.features.push(g);
            }
        }
    }
    // The coordinate system: as chosen, or as most of the texts say, or WGS 84 for GeoJSON.
    out.crs = chosen.or_else(|| found.iter().max_by_key(|(_, n)| **n).map(|(c, _)| *c)).or(if geojson_seen { Some(4326) } else { None });
    if chosen.is_none() && found.len() > 1 {
        ctx.notes.warn(format!(
            "The texts name {} coordinate systems; all are taken to be in EPSG:{}. Split the stream first, or choose the system.",
            found.len(),
            out.crs.unwrap_or(0)
        ));
    }
    if out.crs.is_none() {
        ctx.notes.warn("The text does not say which coordinate system the geometry is in. Choose it, so that it lands in the right place.");
    }
    ctx.notes.info(format!("{} geometries from {}", fmt_count(out.len() as u64), attr));
    if !failed.is_empty() {
        ctx.notes.warn(format!("{} could not be read and went to Failed, with the reason in _error.", fmt_count(failed.len() as u64)));
    }
    Ok(vec![("Output".into(), out), ("Failed".into(), failed)])
}

// ---------- Points to lines ----------

pub(super) const POINTS_TO_LINES_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "group",
        label: "One line for each",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The attribute that says which line a point belongs to, such as shape_id in GTFS or a track id. Empty makes one line of all points",
    },
    ParamSpec {
        key: "order",
        label: "In the order of",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "Such as shape_pt_sequence or a time. Empty keeps the order they come in",
    },
    ParamSpec { key: "close", label: "Close each line into an area", kind: ParamKind::Choice(YES_NO), default: "false", hint: "" },
];

/// A corner of a line: the feature it came from, where it is, and its height.
pub(super) type Corner = (usize, Coord<f64>, Option<f64>);

pub(super) fn run_points_to_lines(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let group = p.get("group").filter(|g| !g.trim().is_empty()).map(|g| table.need_field(&g)).transpose()?;
    let order = p.get("order").filter(|g| !g.trim().is_empty()).map(|g| table.need_field(&g)).transpose()?;
    let close = p.flag("close");
    // The points of each line, by its first appearance.
    let mut lines: Vec<(Value, Vec<Corner>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut skipped = 0usize;
    for (i, f) in table.features.iter().enumerate() {
        let corners: Vec<Coord<f64>> = match &f.geom {
            Some(Geometry::Point(pt)) => vec![pt.0],
            Some(Geometry::MultiPoint(m)) => m.0.iter().map(|p| p.0).collect(),
            _ => {
                skipped += 1;
                continue;
            }
        };
        let key = group.map_or(Value::Null, |g| f.attrs[g].clone());
        let k = *index.entry(key.to_text()).or_insert_with(|| {
            lines.push((key.clone(), Vec::new()));
            lines.len() - 1
        });
        let heights = f.heights();
        for (j, c) in corners.into_iter().enumerate() {
            lines[k].1.push((i, c, heights.and_then(|h| h.get(j).copied())));
        }
    }
    let sort_key = |i: usize| -> (f64, String) {
        match order {
            Some(o) => match &table.features[i].attrs[o] {
                Value::Int(n) => (*n as f64, String::new()),
                Value::Real(x) => (*x, String::new()),
                v => {
                    let t = v.to_text();
                    match t.trim().replace(',', ".").parse::<f64>() {
                        Ok(x) => (x, String::new()),
                        Err(_) => (0.0, t),
                    }
                }
            },
            None => (i as f64, String::new()),
        }
    };
    let mut fields = Vec::new();
    if let Some(g) = group {
        fields.push(table.fields[g].clone());
    }
    fields.push(Field::new("points", FieldType::Int));
    let mut out = Table { fields, crs: table.crs, ..Table::default() };
    let mut short = 0usize;
    for (key, mut pts) in lines {
        pts.sort_by(|a, b| {
            let (ka, kb) = (sort_key(a.0), sort_key(b.0));
            ka.0.partial_cmp(&kb.0).unwrap_or(std::cmp::Ordering::Equal).then_with(|| ka.1.cmp(&kb.1))
        });
        if pts.len() < 2 || (close && pts.len() < 3) {
            short += 1;
            continue;
        }
        let mut coords: Vec<Coord<f64>> = pts.iter().map(|p| p.1).collect();
        let mut z: Vec<f64> = pts.iter().map(|p| p.2.unwrap_or(0.0)).collect();
        let with_z = pts.iter().any(|p| p.2.is_some());
        let geom = if close {
            if coords.first() != coords.last() {
                coords.push(coords[0]);
                z.push(z[0]);
            }
            Geometry::Polygon(geo::Polygon::new(LineString::new(coords), vec![]))
        } else {
            Geometry::LineString(LineString::new(coords))
        };
        let mut attrs = Vec::new();
        if group.is_some() {
            attrs.push(key);
        }
        attrs.push(Value::Int(pts.len() as i64));
        out.features.push(Feature { attrs, geom: Some(geom), z: with_z.then(|| Arc::from(z)) });
    }
    ctx.notes.info(format!("{} {}", fmt_count(out.len() as u64), if close { "areas" } else { "lines" }));
    if skipped > 0 {
        ctx.notes.warn(format!("{} features that are not points were left out.", fmt_count(skipped as u64)));
    }
    if short > 0 {
        ctx.notes.warn(format!("{} groups with too few points for a line were left out.", fmt_count(short as u64)));
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Split lines into segments (LineExploder) ----------

pub(super) const EXPLODER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "areas",
        label: "Areas",
        kind: ParamKind::Choice(&[("keep", "Passed on as they are"), ("outline", "Their outlines split too")]),
        default: "keep",
        hint: "",
    },
    ParamSpec {
        key: "number",
        label: "Number attribute",
        kind: ParamKind::Text,
        default: "segment",
        hint: "Which segment of its line each is, from 1. Empty leaves it out",
    },
];

/// The lines of a geometry, with whether there were any: for areas, their outlines when asked.
fn line_parts(g: &Geometry<f64>, outlines: bool) -> Option<Vec<LineString<f64>>> {
    Some(match g {
        Geometry::LineString(l) => vec![l.clone()],
        Geometry::MultiLineString(m) => m.0.clone(),
        Geometry::Line(l) => vec![LineString::from(vec![l.start, l.end])],
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) if outlines => {
            crate::geom::polygons_of(g).iter().flat_map(|p| std::iter::once(p.exterior().clone()).chain(p.interiors().iter().cloned())).collect()
        }
        _ => return None,
    })
}

pub(super) fn run_line_exploder(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let outlines = p.get("areas").as_deref() == Some("outline");
    let number = p.get("number").filter(|n| !n.trim().is_empty());
    let mut out = table.like();
    if let Some(n) = &number {
        if table.field_index(n).is_some() {
            return err(format!("The stream already has an attribute {}. Choose another name for the number.", n));
        }
        out.fields.push(Field::new(n.clone(), FieldType::Int));
    }
    let mut passed = 0usize;
    for f in &table.features {
        let Some(parts) = f.geom.as_ref().and_then(|g| line_parts(g, outlines)) else {
            passed += usize::from(f.geom.is_some());
            let mut attrs = f.attrs.clone();
            if number.is_some() {
                attrs.push(Value::Null);
            }
            out.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
            continue;
        };
        // Heights follow the corners, when there is one for each.
        let fits = f.z.as_ref().is_some_and(|z| z.len() == parts.iter().map(|l| l.0.len()).sum::<usize>());
        let mut at = 0usize;
        let mut k = 0i64;
        for line in &parts {
            for w in 0..line.0.len().saturating_sub(1) {
                let (a, b) = (line.0[w], line.0[w + 1]);
                if a == b {
                    continue;
                }
                k += 1;
                let mut attrs = f.attrs.clone();
                if number.is_some() {
                    attrs.push(Value::Int(k));
                }
                let z: Option<Arc<[f64]>> = if fits { f.z.as_ref().map(|z| Arc::from(vec![z[at + w], z[at + w + 1]])) } else { None };
                out.features.push(Feature { z, attrs, geom: Some(Geometry::LineString(LineString::from(vec![a, b]))) });
            }
            at += line.0.len();
        }
    }
    if passed > 0 {
        ctx.notes.info(format!("{} features that are not lines were passed on as they are", fmt_count(passed as u64)));
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Convex hull (ConvexHuller) and Concave hull (ConcaveHuller) ----------

pub(super) const HULL_FOR: &[(&str, &str)] = &[("each", "Each feature"), ("together", "All together, or each group")];

pub(super) const CONVEX_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "for", label: "A hull around", kind: ParamKind::Choice(HULL_FOR), default: "together", hint: "" },
    ParamSpec {
        key: "group_by",
        label: "Group by",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "All together: one hull for each group of features with the same values. Empty gives one around all",
    },
    ParamSpec {
        key: "count",
        label: "Count attribute",
        kind: ParamKind::Text,
        default: "count",
        hint: "All together: how many features each hull holds. Empty leaves it out",
    },
];

pub(super) const CONCAVE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "for", label: "A hull around", kind: ParamKind::Choice(HULL_FOR), default: "together", hint: "" },
    ParamSpec {
        key: "group_by",
        label: "Group by",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "All together: one hull for each group of features with the same values. Empty gives one around all",
    },
    ParamSpec {
        key: "count",
        label: "Count attribute",
        kind: ParamKind::Text,
        default: "count",
        hint: "All together: how many features each hull holds. Empty leaves it out",
    },
    ParamSpec {
        key: "concavity",
        label: "Concavity",
        kind: ParamKind::Text,
        default: "1",
        hint: "Lower follows the points more tightly: 0.5 is close, 2 rough, and 10 or more nearly the convex hull",
    },
    ParamSpec {
        key: "shortest",
        label: "Shortest edge",
        kind: ParamKind::Text,
        default: "0",
        hint: "Edges shorter than this are not cut into further, in the units of the coordinate system. 0 cuts all",
    },
];

/// The hull around a set of corners: an area, or a line or a point when they do not span one.
fn hull_of(coords: Vec<Coord<f64>>, concave: Option<(f64, f64)>) -> Option<Geometry<f64>> {
    use geo::{Area, ConcaveHull, ConvexHull};
    let mut unique = coords.clone();
    unique.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    unique.dedup();
    match unique.len() {
        0 => return None,
        1 => return Some(Geometry::Point(unique[0].into())),
        _ => {}
    }
    let points = geo::MultiPoint::from(unique.iter().map(|c| geo::Point::from(*c)).collect::<Vec<_>>());
    let convex = points.convex_hull();
    if convex.unsigned_area() <= 0.0 {
        // In a row: the line from one end to the other.
        let (a, b) = (unique[0], unique[unique.len() - 1]);
        return Some(Geometry::LineString(LineString::from(vec![a, b])));
    }
    Some(Geometry::Polygon(match concave {
        None => convex,
        Some((concavity, shortest)) => {
            let hull = points.concave_hull_with_options(geo::algorithm::concave_hull::ConcaveHullOptions { concavity, length_threshold: shortest });
            if hull.unsigned_area() > 0.0 {
                hull
            } else {
                convex
            }
        }
    }))
}

fn run_hull(p: &P, inp: &Inputs, concave: Option<(f64, f64)>) -> Result<Outputs> {
    use geo::CoordsIter;
    let table = input(inp, "Input");
    let corners = |f: &Feature| f.geom.as_ref().map(|g| g.coords_iter().collect::<Vec<_>>()).unwrap_or_default();
    if p.get("for").as_deref() == Some("each") {
        let mut out = table.like();
        out.features = table.features.iter().map(|f| Feature { z: None, attrs: f.attrs.clone(), geom: hull_of(corners(f), concave) }).collect();
        return Ok(vec![("Output".into(), out)]);
    }
    let by = p.list("group_by");
    let idx: Vec<usize> = by.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;
    let count_name = p.get("count").filter(|c| !c.trim().is_empty());
    let mut out = Table {
        mesh: None,
        fields: idx.iter().map(|i| table.fields[*i].clone()).collect(),
        crs: table.crs,
        features: Vec::new(),
        raster: false,
        image: None,
        grid: None,
    };
    if let Some(c) = &count_name {
        if by.contains(c) {
            return err(format!("The count attribute {} has the same name as a group-by attribute. Rename one of them.", c));
        }
        out.fields.push(Field::new(c.clone(), FieldType::Int));
    }
    for members in groups(table, &idx, |f| f.geom.is_some()) {
        let all: Vec<Coord<f64>> = members.iter().flat_map(|i| corners(&table.features[*i])).collect();
        let mut attrs: Vec<Value> = idx.iter().map(|i| table.features[members[0]].attrs[*i].clone()).collect();
        if count_name.is_some() {
            attrs.push(Value::Int(members.len() as i64));
        }
        out.features.push(Feature { z: None, attrs, geom: hull_of(all, concave) });
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) fn run_convex_huller(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    run_hull(p, inp, None)
}

pub(super) fn run_concave_huller(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let concavity = match number_param(p, "concavity", "The concavity")? {
        None => 1.0,
        Some(c) if c > 0.0 => c,
        Some(_) => return err("The concavity must be more than 0."),
    };
    let shortest = number_param(p, "shortest", "The shortest edge")?.unwrap_or(0.0).max(0.0);
    run_hull(p, inp, Some((concavity, shortest)))
}

// ---------- Every pair (PairMaker) ----------

pub(super) const PAIR_GEOMETRY: &[(&str, &str)] = &[("line", "A line from one to the other"), ("from", "That of From"), ("none", "None")];

pub(super) const PAIR_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "max_distance",
        label: "Largest distance",
        kind: ParamKind::Text,
        default: "",
        hint: "Only pairs this near each other, in the units of the coordinate system. Empty pairs every one with every one",
    },
    ParamSpec { key: "geometry", label: "Geometry of a pair", kind: ParamKind::Choice(PAIR_GEOMETRY), default: "line", hint: "" },
    ParamSpec {
        key: "prefix",
        label: "Prefix of To",
        kind: ParamKind::Text,
        default: "to_",
        hint: "Put in front of the names of the attributes of To, so that they are not mixed up with those of From",
    },
    ParamSpec {
        key: "coords",
        label: "Coordinates as attributes in",
        kind: ParamKind::Crs,
        default: "EPSG:4326",
        hint: "from_lat, from_lon, to_lat and to_lon in WGS 84, as OpenTripPlanner and other route planners want them. Empty leaves them out",
    },
    ParamSpec {
        key: "skip_same",
        label: "Leave out a feature paired with itself",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "When the same stream comes to both, as for travel times between all stops",
    },
];

/// A point that stands for a geometry: the point itself, or one inside it.
fn standing_point(g: &Geometry<f64>) -> Option<Coord<f64>> {
    match g {
        Geometry::Point(p) => Some(p.0),
        Geometry::MultiPoint(m) if m.0.len() == 1 => Some(m.0[0].0),
        other => match crate::geom::center(other, crate::geom::Center::Inside)? {
            Geometry::Point(p) => Some(p.0),
            _ => None,
        },
    }
}

pub(super) fn run_pair_maker(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::{Distance, Euclidean};
    let (from, to_in) = (input(inp, "From"), input(inp, "To"));
    let moved = same_crs(to_in, "To", from.crs, ctx)?;
    let to = moved.as_ref().unwrap_or(to_in);
    let limit = number_param(p, "max_distance", "The largest distance")?.filter(|d| *d >= 0.0);
    let geometry = p.get("geometry").unwrap_or_else(|| "line".into());
    let prefix = p.get("prefix").unwrap_or_else(|| "to_".into());
    let skip_same = p.get("skip_same").as_deref() != Some("false");
    if limit.is_none() && from.len().saturating_mul(to.len()) > 10_000_000 {
        return err(format!("That would be {} pairs. Give a largest distance, or make fewer go in.", fmt_count((from.len() as u128) * (to.len() as u128))));
    }
    if limit.is_some() && ctx.geographic(from.crs) == Some(true) {
        ctx.notes.warn(format!("The distance is in degrees, since From is in {}. Reproject first to count in metres.", crs_label(from.crs)));
    }
    // The coordinates to give each pair, in the coordinate system asked for.
    let coords_crs = p.crs("coords")?;
    let points_in = |t: &Table, ctx: &mut NodeCtx| -> Result<Vec<Option<Coord<f64>>>> {
        let pts: Vec<Option<Coord<f64>>> = t.features.iter().map(|f| f.geom.as_ref().and_then(standing_point)).collect();
        let (Some(want), Some(have)) = (coords_crs, t.crs) else { return Ok(pts) };
        if want == have {
            return Ok(pts);
        }
        let mut xs: Vec<f64> = pts.iter().map(|c| c.map_or(0.0, |c| c.x)).collect();
        let mut ys: Vec<f64> = pts.iter().map(|c| c.map_or(0.0, |c| c.y)).collect();
        ctx.backend("Coordinates in another system")?.transform(have, want, &mut xs, &mut ys)?;
        Ok(pts.iter().enumerate().map(|(i, c)| c.map(|_| Coord { x: xs[i], y: ys[i] })).collect())
    };
    let (from_pts, to_pts) = if coords_crs.is_some() { (points_in(from, ctx)?, points_in(to, ctx)?) } else { (Vec::new(), Vec::new()) };
    let degrees = coords_crs.is_some_and(|c| ctx.geographic(Some(c)) == Some(true));
    let names: [&str; 4] = if degrees { ["from_lat", "from_lon", "to_lat", "to_lon"] } else { ["from_x", "from_y", "to_x", "to_y"] };
    let mut out = Table {
        mesh: None,
        fields: from.fields.clone(),
        crs: if geometry == "none" { None } else { from.crs },
        features: Vec::new(),
        raster: false,
        image: None,
        grid: None,
    };
    for f in &to.fields {
        let name = format!("{}{}", prefix, f.name);
        if out.field_index(&name).is_some() {
            return err(format!("From already has an attribute {}. Choose another prefix for To.", name));
        }
        out.fields.push(Field::new(name, f.ty));
    }
    if coords_crs.is_some() {
        for n in names {
            out.fields.push(Field::new(n, FieldType::Real));
        }
    }
    let index = super::geom_overlay::box_index(to.features.iter().map(|f| f.geom.as_ref()));
    let mut skipped = 0usize;
    for (i, a) in from.features.iter().enumerate() {
        if i % 256 == 0 && ctx.stopped() {
            return err(crate::model::STOPPED);
        }
        let candidates: Vec<usize> = match (limit, &a.geom) {
            (Some(d), Some(g)) => {
                use geo::BoundingRect;
                let Some(r) = g.bounding_rect() else { continue };
                let env = rstar::AABB::from_corners([r.min().x - d, r.min().y - d], [r.max().x + d, r.max().y + d]);
                let mut c: Vec<usize> = index
                    .locate_in_envelope_intersecting(&env)
                    .map(|b| b.data)
                    .filter(|j| to.features[*j].geom.as_ref().is_some_and(|h| Euclidean.distance(g, h) <= d))
                    .collect();
                c.sort_unstable();
                c
            }
            (Some(_), None) => continue,
            (None, _) => (0..to.len()).collect(),
        };
        for j in candidates {
            let b = &to.features[j];
            if skip_same && a.attrs == b.attrs && a.geom == b.geom {
                skipped += 1;
                continue;
            }
            let mut attrs = a.attrs.clone();
            attrs.extend(b.attrs.iter().cloned());
            if coords_crs.is_some() {
                let (pa, pb) = (from_pts[i], to_pts[j]);
                let pair = |c: Option<Coord<f64>>| -> [Value; 2] {
                    match c {
                        Some(c) if degrees => [Value::Real(c.y), Value::Real(c.x)],
                        Some(c) => [Value::Real(c.x), Value::Real(c.y)],
                        None => [Value::Null, Value::Null],
                    }
                };
                attrs.extend(pair(pa));
                attrs.extend(pair(pb));
            }
            let geom = match geometry.as_str() {
                "none" => None,
                "from" => a.geom.clone(),
                _ => match (a.geom.as_ref().and_then(standing_point), b.geom.as_ref().and_then(standing_point)) {
                    (Some(x), Some(y)) => Some(Geometry::LineString(LineString::new(vec![x, y]))),
                    _ => None,
                },
            };
            out.features.push(Feature { z: None, attrs, geom });
        }
    }
    if skipped > 0 {
        ctx.notes.info(format!("{} features were not paired with themselves", fmt_count(skipped as u64)));
    }
    ctx.notes.info(format!("{} pairs", fmt_count(out.len() as u64)));
    Ok(vec![("Pairs".into(), out)])
}

fn hull_summary(p: &P) -> String {
    if p.get("for").as_deref() == Some("each") {
        return "each feature".into();
    }
    let by = p.list("group_by");
    if by.is_empty() {
        "all together".into()
    } else {
        format!("per {}", by.join(", "))
    }
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "PairMaker",
        title: "Every pair",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Pairs every feature of From with every one of To, or with those within a distance: the origins and destinations of a matrix of travel times. Each pair has the attributes of both, the coordinates of both for a route planner such as OpenTripPlanner, and a line between them.",
        inputs: &["From", "To"],
        outputs: &["Pairs"],
        ports: None,
        params: PAIR_PARAMS,
        run: run_pair_maker,
        summary: |p| match p.get("max_distance").filter(|d| !d.trim().is_empty()) {
            Some(d) => format!("within {}", d),
            None => "all with all".into(),
        },
    },
    NodeType {
        name: "LineExploder",
        title: "Split lines into segments",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Explodes each line into its segments: a line from each corner to the next, with the attributes of the line and which segment it is. The outlines of areas can be split too; points pass as they are.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: EXPLODER_PARAMS,
        run: run_line_exploder,
        summary: |p| if p.get("areas").as_deref() == Some("outline") { "lines and outlines".into() } else { String::new() },
    },
    NodeType {
        name: "ConvexHuller",
        title: "Convex hull",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "The smallest convex area around each feature, or around all of them or each group together, as a rubber band stretched around them. Points in a row give a line.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: CONVEX_PARAMS,
        run: run_convex_huller,
        summary: |p| hull_summary(p),
    },
    NodeType {
        name: "ConcaveHuller",
        title: "Concave hull",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "An area around points, or the corners of lines and areas, that follows them inwards where they leave room: the shape of a spread of points rather than the rubber band around them. Concavity says how tightly.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: CONCAVE_PARAMS,
        run: run_concave_huller,
        summary: |p| {
            let base = hull_summary(p);
            let c = format!("concavity {}", p.get("concavity").unwrap_or_else(|| "1".into()));
            if base.is_empty() {
                c
            } else {
                format!("{}, {}", base, c)
            }
        },
    },    NodeType {
        name: "Aggregator",
        title: "Aggregate",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Joins features into one multi-geometry, all of them or one per group. The geometries are kept as they are; to merge areas into one, use Dissolve.",
        inputs: &["Input"],
        outputs: &["Aggregated"],
        ports: None,
        params: AGGREGATOR_PARAMS,
        run: run_aggregator,
        summary: |p| {
            let by = p.list("group_by");
            if by.is_empty() {
                "all into one".to_string()
            } else {
                format!("by {}", by.join(", "))
            }
        },
    },
    NodeType {
        name: "Deaggregator",
        title: "Deaggregate",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Splits multi-geometries into their parts, each a feature of its own with the attributes of the whole.",
        inputs: &["Input"],
        outputs: &["Parts"],
        ports: None,
        params: DEAGGREGATOR_PARAMS,
        run: run_deaggregator,
        summary: |_| String::new(),
    },
    NodeType {
        name: "PointCreator",
        title: "Create points from x and y",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes a point of each feature from two attributes with its x and y. Features where either is missing or not a number go to Failed.",
        inputs: &["Input"],
        outputs: &["Points", "Failed"],
        ports: None,
        params: POINT_CREATOR_PARAMS,
        run: run_point_creator,
        summary: |p| match (p.get("x"), p.get("y")) {
            (Some(x), Some(y)) => format!("{}, {}{}", x, y, p.get("crs").map(|c| format!(" · {}", c)).unwrap_or_default()),
            _ => String::new(),
        },
    },
    NodeType {
        name: "GeometryToText",
        title: "Geometry to text",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Writes the geometry of each feature as text in an attribute: WKT, EWKT with the coordinate system in front, or GeoJSON, with heights where it has them and as many decimals as you want. For a web service, a database or a sheet. The geometry can be kept or dropped.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: TO_TEXT_PARAMS,
        run: run_to_text,
        summary: |p| format!("{} in {}", choice_label(TEXT_FORMATS, &p.get("format").unwrap_or_else(|| "wkt".into())).split(',').next().unwrap_or(""), p.get("name").unwrap_or_else(|| "wkt".into())),
    },
    NodeType {
        name: "GeometryFromText",
        title: "Geometry from text",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes geometry of text in an attribute, as web services give it: WKT (also with SRID= in front and with Z), GeoJSON and Esri JSON from ArcGIS. The coordinate system is the one you choose, or the one the text names. Text that cannot be read goes to Failed.",
        inputs: &["Input"],
        outputs: &["Output", "Failed"],
        ports: None,
        params: FROM_TEXT_PARAMS,
        run: run_from_text,
        summary: |p| match (p.get("attribute"), p.get("crs").filter(|c| !c.is_empty())) {
            (Some(a), Some(c)) => format!("{} · {}", a, c),
            (Some(a), None) => a,
            _ => String::new(),
        },
    },
    NodeType {
        name: "PointsToLines",
        title: "Points to lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Joins points into lines, one for each value of an attribute, in the order of another: the shapes of GTFS, a GPS track, a way from Overpass. Can close each line into an area. Heights go along.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: POINTS_TO_LINES_PARAMS,
        run: run_points_to_lines,
        summary: |p| match (p.get("group").filter(|g| !g.is_empty()), p.get("order").filter(|o| !o.is_empty())) {
            (Some(g), Some(o)) => format!("by {} in order of {}", g, o),
            (Some(g), None) => format!("by {}", g),
            (None, Some(o)) => format!("one line, in order of {}", o),
            _ => "one line".into(),
        },
    },
    NodeType {
        name: "BoundingBoxReplacer",
        title: "Replace with bounding box",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Replaces each geometry with the smallest rectangle around it, along the axes. A box without width or height becomes a line or a point.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: BOX_REPLACER_PARAMS,
        run: run_box_replacer,
        summary: |_| String::new(),
    },
    NodeType {
        name: "BoundingBoxAccumulator",
        title: "Bounding box of all",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "One rectangle around all the features of the stream, or one for each group, with a count of the features in it.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: BOX_ACCUMULATOR_PARAMS,
        run: run_box_accumulator,
        summary: |p| {
            let by = p.list("group_by");
            if by.is_empty() { "around all".into() } else { format!("per {}", by.join(", ")) }
        },
    },
    NodeType {
        name: "PolygonToLine",
        title: "Polygons to lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Turns each area into its boundary as lines: the outer ring and the rings of the holes. What is not an area goes to Other.",
        inputs: &["Input"],
        outputs: &["Lines", "Other"],
        ports: None,
        params: &[],
        run: run_polygon_to_line,
        summary: |_| String::new(),
    },
    NodeType {
        name: "LineToPolygon",
        title: "Lines to polygons",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Turns closed lines into areas. The parts of a line that meet end to end are joined into a ring first. Lines whose ends do not meet go to Failed.",
        inputs: &["Input"],
        outputs: &["Polygons", "Failed"],
        ports: None,
        params: LINE_TO_POLYGON_PARAMS,
        run: run_line_to_polygon,
        summary: |p| match p.get("tolerance").filter(|t| t.trim() != "0" && !t.trim().is_empty()) {
            Some(t) => format!("ends within {}", t),
            None => String::new(),
        },
    },
    NodeType {
        name: "VoronoiDiagrammer",
        title: "Thiessen polygons",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Gives each point the area that lies nearer to it than to any other point (Thiessen or Voronoi polygons), with the attributes of the point. Areas connected to Boundary cut them, otherwise a box around the points does.",
        inputs: &["Points", "Boundary"],
        outputs: &["Polygons"],
        ports: None,
        params: THIESSEN_PARAMS,
        run: run_thiessen,
        summary: |_| String::new(),
    },
    NodeType {
        name: "FeatureCreator",
        title: "Create features",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes points, lines or areas from coordinates you write, a feature on each line, or from WKT and GeoJSON: a place to start from, a study area to clip by, or a few points to test a model with. Each gets a number and, if you give them, a name.",
        inputs: &[],
        outputs: &["Output"],
        ports: None,
        params: CREATE_PARAMS,
        run: run_create,
        summary: |p| {
            let n = p.exact("coordinates").lines().filter(|l| !l.trim().is_empty()).count();
            format!("{} × {}", n, choice_label(CREATE_KINDS, &p.get("kind").unwrap_or_else(|| "point".into())).split(',').next().unwrap_or("").to_lowercase())
        },
    },
    NodeType {
        name: "GridCreator",
        title: "Create grid",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes a grid of squares or hexagons over the features that come in on Extent: all cells in their extent, or the ones that touch them. Each cell is numbered, with its row and column.",
        inputs: &["Extent"],
        outputs: &["Grid"],
        ports: None,
        params: GRID_PARAMS,
        run: run_grid,
        summary: |p| {
            let shape = if p.get("shape").as_deref() == Some("hexagon") { "hexagons" } else { "squares" };
            format!("{} of {}", shape, p.get("size").unwrap_or_else(|| "1000".into()))
        },
    },
    NodeType {
        name: "GeometryRemover",
        title: "Remove geometry",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Takes away the geometry and keeps the attributes, so that the stream is a plain table.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: &[],
        run: run_geometry_remover,
        summary: |_| String::new(),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn ls(c: &[(f64, f64)]) -> LineString<f64> {
        LineString::new(c.iter().map(|(x, y)| Coord { x: *x, y: *y }).collect())
    }

    #[test]
    fn a_line_is_cut_at_the_places() {
        let l = ls(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        let parts = cut_line(&l, vec![(1, 0.5), (0, 0.3), (0, 1.0), (0, 0.0)]);
        let ends: Vec<(Coord<f64>, Coord<f64>)> = parts.iter().map(|p| (p.0[0], *p.0.last().unwrap())).collect();
        // At 3 along the first segment, at the corner, and half way up the second.
        assert_eq!(parts.len(), 4, "{:?}", ends);
        assert_eq!(ends[0], (Coord { x: 0.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }));
        assert_eq!(ends[1], (Coord { x: 3.0, y: 0.0 }, Coord { x: 10.0, y: 0.0 }));
        assert_eq!(ends[2], (Coord { x: 10.0, y: 0.0 }, Coord { x: 10.0, y: 5.0 }));
        assert_eq!(ends[3], (Coord { x: 10.0, y: 5.0 }, Coord { x: 10.0, y: 10.0 }));
        assert!(parts.iter().all(|p| p.0.len() == 2));
        assert_eq!(cut_line(&l, vec![]).len(), 1);
    }
}
