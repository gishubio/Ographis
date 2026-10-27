//! Geometry in pure Rust: area, length, clipping, overlay, buffering and help for
//! reprojecting coordinates.

use crate::model::Result;
use geo::algorithm::indexed::IntervalTreeMultiPolygon;
use geo::line_intersection::{line_intersection, LineIntersection};
use geo::orient::{Direction, Orient};
use geo::{
    Area, BooleanOps, BoundingRect, Buffer, Centroid, Contains, Coord, CoordsIter, Euclidean, Geodesic, GeodesicArea, Geometry, InteriorPoint, Intersects,
    Length, Line, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect, Simplify,
};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{RTree, AABB};

/// A boundary segment of the clip area, with its bounding rectangle for searching.
type Seg = GeomWithData<Rectangle<[f64; 2]>, Line<f64>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Point,
    Line,
    Area,
    Other,
}

pub fn family(g: &Geometry<f64>) -> Family {
    match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => Family::Point,
        Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_) => Family::Line,
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => Family::Area,
        Geometry::GeometryCollection(_) => Family::Other,
    }
}

/// The coordinates of a geometry, in the order heights are kept in: a point; the points of a
/// line; the outer ring of an area and then its holes, each with the closing point; the
/// parts of a collection one after the other.
pub fn coords(g: &Geometry<f64>) -> Vec<geo::Coord<f64>> {
    let mut out = Vec::new();
    walk(g, &mut |c| out.push(c));
    out
}

/// How many coordinates `coords` gives, without making the list.
pub fn coord_count(g: &Geometry<f64>) -> usize {
    let mut n = 0;
    walk(g, &mut |_| n += 1);
    n
}

fn walk(g: &Geometry<f64>, f: &mut dyn FnMut(geo::Coord<f64>)) {
    let poly = |p: &geo::Polygon<f64>, f: &mut dyn FnMut(geo::Coord<f64>)| {
        p.exterior().0.iter().for_each(|c| f(*c));
        p.interiors().iter().for_each(|r| r.0.iter().for_each(|c| f(*c)));
    };
    match g {
        Geometry::Point(p) => f(p.0),
        Geometry::MultiPoint(m) => m.0.iter().for_each(|p| f(p.0)),
        Geometry::Line(l) => {
            f(l.start);
            f(l.end);
        }
        Geometry::LineString(l) => l.0.iter().for_each(|c| f(*c)),
        Geometry::MultiLineString(m) => m.0.iter().for_each(|l| l.0.iter().for_each(|c| f(*c))),
        Geometry::Polygon(p) => poly(p, f),
        Geometry::MultiPolygon(m) => m.0.iter().for_each(|p| poly(p, f)),
        Geometry::Rect(r) => poly(&r.to_polygon(), f),
        Geometry::Triangle(t) => poly(&t.to_polygon(), f),
        Geometry::GeometryCollection(c) => c.0.iter().for_each(|g| walk(g, f)),
    }
}

/// A geometry as WKT with a height on each coordinate, such as POINT Z (1 2 3), for writing.
pub fn wkt_z(g: &Geometry<f64>, z: &[f64]) -> String {
    let mut k = 0usize;
    let mut next = |c: &geo::Coord<f64>| {
        let h = z.get(k).copied().unwrap_or(0.0);
        k += 1;
        format!("{} {} {}", c.x, c.y, h)
    };
    fn ring(l: &geo::LineString<f64>, next: &mut dyn FnMut(&geo::Coord<f64>) -> String) -> String {
        format!("({})", l.0.iter().map(next).collect::<Vec<_>>().join(", "))
    }
    fn poly(p: &geo::Polygon<f64>, next: &mut dyn FnMut(&geo::Coord<f64>) -> String) -> String {
        let mut rings = vec![ring(p.exterior(), next)];
        rings.extend(p.interiors().iter().map(|r| ring(r, next)));
        format!("({})", rings.join(", "))
    }
    fn one(g: &Geometry<f64>, next: &mut dyn FnMut(&geo::Coord<f64>) -> String) -> String {
        match g {
            Geometry::Point(p) => format!("POINT Z ({})", next(&p.0)),
            Geometry::MultiPoint(m) => format!("MULTIPOINT Z ({})", m.0.iter().map(|p| format!("({})", next(&p.0))).collect::<Vec<_>>().join(", ")),
            Geometry::Line(l) => format!("LINESTRING Z ({}, {})", next(&l.start), next(&l.end)),
            Geometry::LineString(l) => format!("LINESTRING Z {}", ring(l, next)),
            Geometry::MultiLineString(m) => format!("MULTILINESTRING Z ({})", m.0.iter().map(|l| ring(l, next)).collect::<Vec<_>>().join(", ")),
            Geometry::Polygon(p) => format!("POLYGON Z {}", poly(p, next)),
            Geometry::MultiPolygon(m) => format!("MULTIPOLYGON Z ({})", m.0.iter().map(|p| poly(p, next)).collect::<Vec<_>>().join(", ")),
            Geometry::Rect(r) => format!("POLYGON Z {}", poly(&r.to_polygon(), next)),
            Geometry::Triangle(t) => format!("POLYGON Z {}", poly(&t.to_polygon(), next)),
            Geometry::GeometryCollection(c) => format!("GEOMETRYCOLLECTION Z ({})", c.0.iter().map(|g| one(g, next)).collect::<Vec<_>>().join(", ")),
        }
    }
    one(g, &mut next)
}

pub fn type_name(g: &Geometry<f64>) -> &'static str {
    match g {
        Geometry::Point(_) => "Point",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::Line(_) | Geometry::LineString(_) => "LineString",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::Polygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => "Polygon",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
    }
}

pub fn is_multi(g: &Geometry<f64>) -> bool {
    matches!(g, Geometry::MultiPoint(_) | Geometry::MultiLineString(_) | Geometry::MultiPolygon(_))
}

/// Turns the unusual variants (Line, Rect, Triangle) into LineString and Polygon, so that
/// the rest of the engine only has to deal with the six common types.
pub fn normalize(g: Geometry<f64>) -> Geometry<f64> {
    match g {
        Geometry::Line(l) => Geometry::LineString(LineString::from(vec![l.start, l.end])),
        Geometry::Rect(r) => Geometry::Polygon(r.to_polygon()),
        Geometry::Triangle(t) => Geometry::Polygon(t.to_polygon()),
        Geometry::GeometryCollection(c) => Geometry::GeometryCollection(geo::GeometryCollection::from(c.0.into_iter().map(normalize).collect::<Vec<_>>())),
        other => other,
    }
}

/// Area in square metres. Planar in metre-based systems, geodesic on the WGS 84 ellipsoid in degrees.
pub fn area(g: &Geometry<f64>, geographic: bool) -> f64 {
    if family(g) != Family::Area {
        return 0.0;
    }
    if !geographic {
        return g.unsigned_area();
    }
    // The geodesic calculation gives the rest of the globe if the rings run the wrong way,
    // so the area is oriented first: outer ring counter-clockwise, holes clockwise.
    MultiPolygon::new(polygons_of(g)).orient(Direction::Default).geodesic_area_unsigned()
}

/// Length in metres. For areas the perimeter is given, holes included.
pub fn length(g: &Geometry<f64>, geographic: bool) -> f64 {
    let line = |ls: &LineString<f64>| if geographic { Geodesic.length(ls) } else { Euclidean.length(ls) };
    let poly = |p: &Polygon<f64>| line(p.exterior()) + p.interiors().iter().map(line).sum::<f64>();
    match g {
        Geometry::LineString(ls) => line(ls),
        Geometry::MultiLineString(m) => m.0.iter().map(line).sum(),
        Geometry::Polygon(p) => poly(p),
        Geometry::MultiPolygon(m) => m.0.iter().map(poly).sum(),
        Geometry::Line(l) => line(&LineString::from(vec![l.start, l.end])),
        Geometry::Rect(r) => poly(&r.to_polygon()),
        Geometry::Triangle(t) => poly(&t.to_polygon()),
        _ => 0.0,
    }
}

pub fn polygons_of(g: &Geometry<f64>) -> Vec<Polygon<f64>> {
    match g {
        Geometry::Polygon(p) => vec![p.clone()],
        Geometry::MultiPolygon(m) => m.0.clone(),
        Geometry::Rect(r) => vec![r.to_polygon()],
        Geometry::Triangle(t) => vec![t.to_polygon()],
        _ => Vec::new(),
    }
}

fn simplify_lines(parts: Vec<LineString<f64>>) -> Option<Geometry<f64>> {
    let mut parts: Vec<LineString<f64>> = parts.into_iter().filter(|l| l.0.len() >= 2).collect();
    match parts.len() {
        0 => None,
        1 => Some(Geometry::LineString(parts.remove(0))),
        _ => Some(Geometry::MultiLineString(MultiLineString::new(parts))),
    }
}

fn simplify_polygons(m: MultiPolygon<f64>) -> Option<Geometry<f64>> {
    let mut parts: Vec<Polygon<f64>> = m.0.into_iter().filter(|p| p.unsigned_area() > 0.0).collect();
    match parts.len() {
        0 => None,
        1 => Some(Geometry::Polygon(parts.remove(0))),
        _ => Some(Geometry::MultiPolygon(MultiPolygon::new(parts))),
    }
}

/// The result of clipping a geometry: the part inside and the part outside.
pub struct Clipped {
    pub inside: Option<Geometry<f64>>,
    pub outside: Option<Geometry<f64>>,
    /// False if the geometry type cannot be clipped. Everything is then in `outside`.
    pub supported: bool,
}

/// Clip areas, merged into one area and prepared for many queries.
pub struct Clipper {
    area: MultiPolygon<f64>,
    tree: IntervalTreeMultiPolygon<f64>,
    /// All boundary segments, outer rings and holes, in an R-tree.
    boundary: RTree<Seg>,
    bbox: Rect<f64>,
}

impl Clipper {
    /// Builds the clipper from all areas among the geometries. `None` if there is no area.
    pub fn new<'a>(geoms: impl Iterator<Item = &'a Geometry<f64>>) -> Option<Clipper> {
        // The union and the point test rely on the winding of the rings, and files do not
        // always follow the same rule. All areas are therefore oriented first: outer ring
        // counter-clockwise, holes clockwise.
        let polys: Vec<Polygon<f64>> = geoms.flat_map(polygons_of).map(|p| p.orient(Direction::Default)).collect();
        if polys.is_empty() {
            return None;
        }
        let area = geo::unary_union(polys.iter()).orient(Direction::Default);
        let bbox = area.bounding_rect()?;
        let segments: Vec<Seg> = area
            .0
            .iter()
            .flat_map(|p| p.exterior().lines().chain(p.interiors().iter().flat_map(|r| r.lines())))
            .map(|l| GeomWithData::new(Rectangle::from_corners([l.start.x, l.start.y], [l.end.x, l.end.y]), l))
            .collect();
        let boundary = RTree::bulk_load(segments);
        let tree = IntervalTreeMultiPolygon::new(&area);
        Some(Clipper { area, tree, boundary, bbox })
    }

    /// The number of parts after the union.
    pub fn part_count(&self) -> usize {
        self.area.0.len()
    }

    /// Is the point in the clip area? Points exactly on the boundary count as inside.
    pub fn has_point(&self, c: Coord<f64>) -> bool {
        if !in_rect(&self.bbox, c) {
            return false;
        }
        self.tree.contains(&c) || self.near([c.x, c.y], [c.x, c.y]).any(|l| l.intersects(&c))
    }

    /// Boundary segments whose bounding rectangle reaches into the given rectangle.
    fn near(&self, a: [f64; 2], b: [f64; 2]) -> impl Iterator<Item = &Line<f64>> {
        self.boundary.locate_in_envelope_intersecting(&AABB::from_corners(a, b)).map(|s| &s.data)
    }

    /// Splits a line where it crosses the boundary of the clip area and sorts the pieces
    /// as inside or outside. Only boundary segments near the line are touched, so many
    /// short lines against a large area are fast.
    fn clip_linestring(&self, ls: &LineString<f64>, inside: &mut Vec<LineString<f64>>, outside: &mut Vec<LineString<f64>>) {
        let mut run: Vec<Coord<f64>> = Vec::new();
        let mut run_in = false;
        for seg in ls.lines() {
            let (a, b) = (seg.start, seg.end);
            if a == b {
                continue;
            }
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            let param = |p: Coord<f64>| ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
            let mut cuts: Vec<(f64, Coord<f64>)> = Vec::new();
            for other in self.near([a.x.min(b.x), a.y.min(b.y)], [a.x.max(b.x), a.y.max(b.y)]) {
                match line_intersection(seg, *other) {
                    Some(LineIntersection::SinglePoint { intersection, .. }) => cuts.push((param(intersection), intersection)),
                    Some(LineIntersection::Collinear { intersection }) => {
                        cuts.push((param(intersection.start), intersection.start));
                        cuts.push((param(intersection.end), intersection.end));
                    }
                    None => {}
                }
            }
            cuts.retain(|(t, p)| *t > 0.0 && *t < 1.0 && *p != a && *p != b);
            cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
            cuts.dedup_by(|x, y| x.1 == y.1);
            // Pieces of the segment with their class, merged where two neighbours have the same class.
            let mut pieces: Vec<(Coord<f64>, bool)> = Vec::new();
            let mut prev = a;
            for end in cuts.iter().map(|c| c.1).chain([b]) {
                let mid = Coord { x: (prev.x + end.x) / 2.0, y: (prev.y + end.y) / 2.0 };
                let is_in = self.has_point(mid);
                match pieces.last_mut() {
                    Some(last) if last.1 == is_in => last.0 = end,
                    _ => pieces.push((end, is_in)),
                }
                prev = end;
            }
            let mut start = a;
            for (end, is_in) in pieces {
                if run.is_empty() {
                    run.push(start);
                    run_in = is_in;
                } else if is_in != run_in {
                    let done = std::mem::replace(&mut run, vec![start]);
                    (if run_in { &mut *inside } else { &mut *outside }).push(LineString::new(done));
                    run_in = is_in;
                }
                run.push(end);
                start = end;
            }
        }
        if run.len() >= 2 {
            (if run_in { inside } else { outside }).push(LineString::new(run));
        }
    }

    pub fn clip(&self, g: &Geometry<f64>) -> Clipped {
        let done = |inside, outside| Clipped { inside, outside, supported: true };
        match g {
            Geometry::Point(p) => {
                if self.has_point(p.0) {
                    done(Some(g.clone()), None)
                } else {
                    done(None, Some(g.clone()))
                }
            }
            Geometry::MultiPoint(mp) => {
                let (i, o): (Vec<Point<f64>>, Vec<Point<f64>>) = mp.0.iter().partition(|p| self.has_point(p.0));
                let wrap = |v: Vec<Point<f64>>| if v.is_empty() { None } else { Some(Geometry::MultiPoint(MultiPoint::new(v))) };
                done(wrap(i), wrap(o))
            }
            Geometry::LineString(_) | Geometry::MultiLineString(_) | Geometry::Line(_) => {
                if !self.touches_bbox(g) {
                    return done(None, Some(g.clone()));
                }
                let parts: Vec<LineString<f64>> = match g {
                    Geometry::LineString(ls) => vec![ls.clone()],
                    Geometry::MultiLineString(m) => m.0.clone(),
                    Geometry::Line(l) => vec![LineString::from(vec![l.start, l.end])],
                    _ => unreachable!(),
                };
                let (mut inside, mut outside) = (Vec::new(), Vec::new());
                for ls in &parts {
                    self.clip_linestring(ls, &mut inside, &mut outside);
                }
                // A line that was not split is left untouched, so that it keeps its type.
                match (inside.len(), outside.len()) {
                    (_, 0) if inside.len() == parts.len() => done(Some(g.clone()), None),
                    (0, _) if outside.len() == parts.len() => done(None, Some(g.clone())),
                    _ => done(simplify_lines(inside), simplify_lines(outside)),
                }
            }
            Geometry::Polygon(_) | Geometry::MultiPolygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => {
                let r = match g.bounding_rect() {
                    Some(r) if r.intersects(&self.bbox) => r,
                    _ => return done(None, Some(g.clone())),
                };
                // If no part of the clip boundary reaches into the rectangle of the area, the
                // area lies entirely inside or entirely outside, and testing one point is enough.
                if self.near([r.min().x, r.min().y], [r.max().x, r.max().y]).next().is_none() {
                    let first = polygons_of(g).first().and_then(|p| p.exterior().0.first().copied());
                    return match first {
                        Some(c) if self.has_point(c) => done(Some(g.clone()), None),
                        _ => done(None, Some(g.clone())),
                    };
                }
                let m = MultiPolygon::new(polygons_of(g));
                done(simplify_polygons(self.area.intersection(&m)), simplify_polygons(m.difference(&self.area)))
            }
            Geometry::GeometryCollection(_) => Clipped { inside: None, outside: Some(g.clone()), supported: false },
        }
    }

    fn touches_bbox(&self, g: &Geometry<f64>) -> bool {
        g.bounding_rect().is_some_and(|r| r.intersects(&self.bbox))
    }
}

fn in_rect(r: &Rect<f64>, c: Coord<f64>) -> bool {
    c.x >= r.min().x && c.x <= r.max().x && c.y >= r.min().y && c.y <= r.max().y
}

/// Visits every coordinate of a geometry, always in the same order.
fn each_coord(g: &mut Geometry<f64>, f: &mut dyn FnMut(&mut Coord<f64>)) {
    match g {
        Geometry::Point(p) => f(&mut p.0),
        Geometry::Line(l) => {
            f(&mut l.start);
            f(&mut l.end);
        }
        Geometry::LineString(ls) => ls.0.iter_mut().for_each(f),
        Geometry::MultiPoint(m) => m.0.iter_mut().for_each(|p| f(&mut p.0)),
        Geometry::MultiLineString(m) => m.0.iter_mut().for_each(|ls| ls.0.iter_mut().for_each(&mut *f)),
        Geometry::Polygon(p) => each_poly_coord(p, f),
        Geometry::MultiPolygon(m) => m.0.iter_mut().for_each(|p| each_poly_coord(p, f)),
        Geometry::GeometryCollection(c) => c.0.iter_mut().for_each(|x| each_coord(x, f)),
        Geometry::Rect(_) | Geometry::Triangle(_) => {
            let mut n = normalize(g.clone());
            each_coord(&mut n, f);
            *g = n;
        }
    }
}

fn each_poly_coord(p: &mut Polygon<f64>, f: &mut dyn FnMut(&mut Coord<f64>)) {
    p.exterior_mut(|ls| ls.0.iter_mut().for_each(&mut *f));
    p.interiors_mut(|rings| rings.iter_mut().for_each(|ls| ls.0.iter_mut().for_each(&mut *f)));
}

/// Reprojects all coordinates in one go. `transform` gets two lists of equal length,
/// x and y, and writes the new values in the same places.
pub fn transform_all<'a>(geoms: impl Iterator<Item = &'a mut Geometry<f64>>, transform: impl FnOnce(&mut [f64], &mut [f64]) -> Result<()>) -> Result<()> {
    let mut geoms: Vec<&mut Geometry<f64>> = geoms.collect();
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for g in geoms.iter_mut() {
        each_coord(g, &mut |c| {
            xs.push(c.x);
            ys.push(c.y);
        });
    }
    transform(&mut xs, &mut ys)?;
    let mut i = 0;
    for g in geoms.iter_mut() {
        each_coord(g, &mut |c| {
            c.x = xs[i];
            c.y = ys[i];
            i += 1;
        });
    }
    Ok(())
}

/// Buffers a geometry with round joins and caps. A negative distance shrinks areas.
/// `None` if nothing is left, as when an area is shrunk away or a point gets no width.
pub fn buffer(g: &Geometry<f64>, distance: f64) -> Option<Geometry<f64>> {
    if distance == 0.0 {
        return if family(g) == Family::Area { Some(g.clone()) } else { None };
    }
    simplify_polygons(g.buffer(distance))
}

/// Rounds areas as in a drawing for children: narrow gaps and inlets narrower than twice
/// the radius are filled, points and spits narrower than that are cut away, and every
/// corner becomes a curve of the radius. Parts and holes smaller than `smallest` square
/// metres go. Lines and points are returned as they are; `None` if nothing is left.
pub fn soften(g: &Geometry<f64>, radius: f64, smallest: f64) -> Option<Geometry<f64>> {
    if family(g) != Family::Area {
        return Some(g.clone());
    }
    let mut m = MultiPolygon::new(polygons_of(g));
    if radius > 0.0 {
        // Opening, then closing: in, out twice as far, and in again. Spits go first, so
        // that no bump is left where they began.
        m = m.buffer(-radius);
        m = m.buffer(2.0 * radius);
        m = m.buffer(-radius);
        // The arcs of the buffers have more points than a curve of this size needs.
        m = m.simplify(radius * 0.01);
    }
    let ring_area = |r: &LineString<f64>| Polygon::new(r.clone(), vec![]).unsigned_area();
    let parts: Vec<Polygon<f64>> =
        m.0.into_iter()
            .filter(|p| p.unsigned_area() >= smallest.max(0.0) && p.exterior().0.len() >= 4)
            .map(|p| {
                let (outer, holes) = p.into_inner();
                let holes = holes.into_iter().filter(|h| h.0.len() >= 4 && ring_area(h) >= smallest.max(0.0)).collect();
                Polygon::new(outer, holes)
            })
            .collect();
    simplify_polygons(MultiPolygon::new(parts))
}

/// Merges all areas among the geometries into one. Shared boundaries disappear and
/// overlaps are counted once. `None` if there is no area.
pub fn dissolve<'a>(geoms: impl Iterator<Item = &'a Geometry<f64>>) -> Option<Geometry<f64>> {
    let polys: Vec<Polygon<f64>> = geoms.flat_map(polygons_of).map(|p| p.orient(Direction::Default)).collect();
    if polys.is_empty() {
        return None;
    }
    simplify_polygons(geo::unary_union(polys.iter()))
}

/// Which point stands in for a geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Center {
    /// The centre of mass. It can fall outside a curved line or a C-shaped area.
    Centroid,
    /// A point that is guaranteed to lie on the geometry.
    Inside,
    /// The centre of the bounding box.
    BoundingBox,
}

pub fn center(g: &Geometry<f64>, mode: Center) -> Option<Geometry<f64>> {
    let p = match mode {
        Center::Centroid => g.centroid(),
        Center::Inside => g.interior_point(),
        Center::BoundingBox => g.bounding_rect().map(|r| Point::from(r.center())),
    };
    p.filter(|p| p.x().is_finite() && p.y().is_finite()).map(Geometry::Point)
}

/// Removes vertices that lie closer than `tolerance` to the simplified line
/// (Douglas–Peucker). Points are returned unchanged. An area that would collapse keeps
/// its original shape.
pub fn generalize(g: &Geometry<f64>, tolerance: f64) -> Geometry<f64> {
    let poly = |p: &Polygon<f64>| {
        let s = p.simplify(tolerance);
        if s.exterior().0.len() >= 4 && s.unsigned_area() > 0.0 {
            s
        } else {
            p.clone()
        }
    };
    match g {
        Geometry::LineString(ls) => Geometry::LineString(ls.simplify(tolerance)),
        Geometry::MultiLineString(m) => Geometry::MultiLineString(m.simplify(tolerance)),
        Geometry::Polygon(p) => Geometry::Polygon(poly(p)),
        Geometry::MultiPolygon(m) => Geometry::MultiPolygon(MultiPolygon::new(m.0.iter().map(poly).collect())),
        Geometry::GeometryCollection(c) => {
            Geometry::GeometryCollection(geo::GeometryCollection::from(c.0.iter().map(|x| generalize(x, tolerance)).collect::<Vec<_>>()))
        }
        other => other.clone(),
    }
}

/// The lines and rings of a geometry, each passed through `f` in the same order every time,
/// and the geometry built again from what `f` gives. `f` is told whether the line is a ring.
fn map_chains(g: &Geometry<f64>, f: &mut dyn FnMut(&LineString<f64>, bool) -> LineString<f64>) -> Geometry<f64> {
    let poly = |p: &Polygon<f64>, f: &mut dyn FnMut(&LineString<f64>, bool) -> LineString<f64>| {
        let ext = f(p.exterior(), true);
        let holes = p.interiors().iter().map(|h| f(h, true)).collect();
        Polygon::new(ext, holes)
    };
    match g {
        Geometry::LineString(ls) => Geometry::LineString(f(ls, false)),
        Geometry::MultiLineString(m) => Geometry::MultiLineString(MultiLineString::new(m.0.iter().map(|l| f(l, false)).collect())),
        Geometry::Polygon(p) => Geometry::Polygon(poly(p, f)),
        Geometry::MultiPolygon(m) => Geometry::MultiPolygon(MultiPolygon::new(m.0.iter().map(|p| poly(p, f)).collect())),
        Geometry::GeometryCollection(c) => {
            Geometry::GeometryCollection(geo::GeometryCollection::from(c.0.iter().map(|x| map_chains(x, f)).collect::<Vec<_>>()))
        }
        other => other.clone(),
    }
}

/// A coordinate as a key that is the same for the same coordinate, exactly.
fn coord_key(c: &Coord<f64>) -> (u64, u64) {
    // 0.0 and -0.0 are the same place.
    ((c.x + 0.0).to_bits(), (c.y + 0.0).to_bits())
}

/// Generalizes many geometries together, so that what they share stays shared: a boundary
/// between two areas, or a line that runs along one, is simplified once and both get the
/// same result. No gaps or overlaps arise between neighbours. Neighbours must share their
/// vertices exactly along what they share; where they do not, each is simplified on its own.
pub fn generalize_shared(geoms: &[Option<Geometry<f64>>], tolerance: f64) -> Vec<Option<Geometry<f64>>> {
    use std::collections::{HashMap, HashSet};
    // All lines and rings, in the order map_chains visits them.
    let mut chains: Vec<(Vec<Coord<f64>>, bool)> = Vec::new();
    for g in geoms.iter().flatten() {
        map_chains(g, &mut |ls, ring| {
            let mut c: Vec<Coord<f64>> = Vec::with_capacity(ls.0.len());
            for p in &ls.0 {
                if c.last() != Some(p) {
                    c.push(*p);
                }
            }
            chains.push((c, ring));
            ls.clone()
        });
    }
    // Which other vertices each vertex is joined to, over all chains.
    let mut neighbours: HashMap<(u64, u64), HashSet<(u64, u64)>> = HashMap::new();
    for (c, _) in &chains {
        for w in c.windows(2) {
            let (a, b) = (coord_key(&w[0]), coord_key(&w[1]));
            neighbours.entry(a).or_default().insert(b);
            neighbours.entry(b).or_default().insert(a);
        }
    }
    // A node is where a chain meets more than one other way: three or more neighbours, or
    // the end of a line, also where that end lies on another chain. Between nodes run arcs,
    // which are simplified once each.
    let ends: HashSet<(u64, u64)> =
        chains.iter().filter(|(c, ring)| !ring && !c.is_empty()).flat_map(|(c, _)| [coord_key(&c[0]), coord_key(&c[c.len() - 1])]).collect();
    let is_node = |k: &(u64, u64)| ends.contains(k) || neighbours.get(k).is_none_or(|n| n.len() != 2);
    let mut done: HashMap<Vec<(u64, u64)>, Vec<Coord<f64>>> = HashMap::new();
    let mut simplify_arc = |arc: &[Coord<f64>]| -> Vec<Coord<f64>> {
        let keys: Vec<(u64, u64)> = arc.iter().map(coord_key).collect();
        let mut rev = keys.clone();
        rev.reverse();
        // The same arc seen from the other side is stored once, in one direction.
        let forward = keys <= rev;
        let canonical = if forward { keys } else { rev };
        let simplified = done.entry(canonical).or_insert_with(|| {
            let mut pts = arc.to_vec();
            if !forward {
                pts.reverse();
            }
            LineString::new(pts).simplify(tolerance).0
        });
        let mut out = simplified.clone();
        if !forward {
            out.reverse();
        }
        out
    };
    let mut results: Vec<LineString<f64>> = Vec::with_capacity(chains.len());
    for (c, ring) in &chains {
        if c.len() < 2 {
            results.push(LineString::new(c.clone()));
            continue;
        }
        let mut pts = c.clone();
        let mut breaks: Vec<usize>;
        if *ring {
            if pts.first() == pts.last() {
                pts.pop();
            }
            let n = pts.len();
            if n < 3 {
                results.push(LineString::new(c.clone()));
                continue;
            }
            // A ring starts at a node. One with none starts at its lowest vertex, so that a
            // ring that is the same as another, such as an island and its hole, starts at the
            // same place.
            let start = (0..n).find(|i| is_node(&coord_key(&pts[*i]))).unwrap_or_else(|| (0..n).min_by_key(|i| coord_key(&pts[*i])).unwrap_or(0));
            pts.rotate_left(start);
            pts.push(pts[0]);
            breaks = (0..pts.len()).filter(|i| *i == 0 || *i == pts.len() - 1 || is_node(&coord_key(&pts[*i]))).collect();
        } else {
            breaks = (0..pts.len()).filter(|i| *i == 0 || *i == pts.len() - 1 || is_node(&coord_key(&pts[*i]))).collect();
        }
        breaks.dedup();
        let mut out: Vec<Coord<f64>> = Vec::new();
        for w in breaks.windows(2) {
            let arc = simplify_arc(&pts[w[0]..=w[1]]);
            for p in arc {
                if out.last() != Some(&p) {
                    out.push(p);
                }
            }
        }
        // A ring that collapsed keeps its original shape.
        if *ring && (out.len() < 4 || Polygon::new(LineString::new(out.clone()), vec![]).unsigned_area() == 0.0) {
            out = c.clone();
        }
        results.push(LineString::new(out));
    }
    let mut next = results.into_iter();
    geoms.iter().map(|g| g.as_ref().map(|g| map_chains(g, &mut |ls, _| next.next().unwrap_or_else(|| ls.clone())))).collect()
}

pub fn vertex_count(g: &Geometry<f64>) -> usize {
    g.coords_count()
}

/// Areas prepared for the question "which areas contain this point?".
pub struct AreaIndex {
    tree: RTree<GeomWithData<Rectangle<[f64; 2]>, usize>>,
    areas: Vec<Option<MultiPolygon<f64>>>,
}

impl AreaIndex {
    /// One entry per geometry, in order. Entries that are not areas never match.
    pub fn new<'a>(geoms: impl Iterator<Item = Option<&'a Geometry<f64>>>) -> AreaIndex {
        let areas: Vec<Option<MultiPolygon<f64>>> =
            geoms.map(|g| g.map(polygons_of).filter(|p| !p.is_empty()).map(|p| MultiPolygon::new(p).orient(Direction::Default))).collect();
        let boxes = areas
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                a.as_ref()
                    .and_then(|a| a.bounding_rect())
                    .map(|r| GeomWithData::new(Rectangle::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]), i))
            })
            .collect();
        AreaIndex { tree: RTree::bulk_load(boxes), areas }
    }

    /// The areas that contain the point, lowest index first. A point exactly on a boundary
    /// counts as inside, so it can match two neighbouring areas.
    pub fn containing(&self, c: Coord<f64>) -> Vec<usize> {
        let mut hits: Vec<usize> = self
            .tree
            .locate_in_envelope_intersecting(&AABB::from_point([c.x, c.y]))
            .map(|b| b.data)
            .filter(|i| self.areas[*i].as_ref().is_some_and(|a| a.intersects(&c)))
            .collect();
        hits.sort_unstable();
        hits
    }
}

/// Rounds the coordinates to a number of decimals. Used when writing as text.
pub fn rounded(g: &Geometry<f64>, decimals: i32) -> Geometry<f64> {
    let k = 10f64.powi(decimals);
    let mut out = g.clone();
    each_coord(&mut out, &mut |c| {
        c.x = (c.x * k).round() / k;
        c.y = (c.y * k).round() / k;
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::wkt;

    fn square() -> Geometry<f64> {
        Geometry::Polygon(wkt!(POLYGON((0. 0., 10. 0., 10. 10., 0. 10., 0. 0.))))
    }

    #[test]
    fn planar_area_and_length() {
        assert_eq!(area(&square(), false), 100.0);
        assert_eq!(length(&square(), false), 40.0);
        let holed = Geometry::Polygon(wkt!(POLYGON((0. 0., 10. 0., 10. 10., 0. 10., 0. 0.), (2. 2., 4. 2., 4. 4., 2. 4., 2. 2.))));
        assert_eq!(area(&holed, false), 96.0);
        assert_eq!(length(&holed, false), 48.0);
        let line = Geometry::LineString(wkt!(LINESTRING(0. 0., 3. 4., 3. 10.)));
        assert_eq!(length(&line, false), 11.0);
        assert_eq!(area(&line, false), 0.0);
    }

    #[test]
    fn geodesic_area_and_length() {
        // One degree of longitude at the equator is about 111.32 km.
        let line = Geometry::LineString(wkt!(LINESTRING(0. 0., 1. 0.)));
        assert!((length(&line, true) - 111_319.49).abs() < 1.0, "{}", length(&line, true));
        // A cell of 1 × 1 degree at the equator is about 12,309 km².
        let cell = Geometry::Polygon(wkt!(POLYGON((0. 0., 1. 0., 1. 1., 0. 1., 0. 0.))));
        let km2 = area(&cell, true) / 1e6;
        assert!((km2 - 12_309.0).abs() < 2.0, "{}", km2);
        // The same cell clockwise, and with a hole in either direction, must give the same answer.
        let cw = Geometry::Polygon(wkt!(POLYGON((0. 0., 0. 1., 1. 1., 1. 0., 0. 0.))));
        assert!((area(&cw, true) / 1e6 - km2).abs() < 1e-6, "{}", area(&cw, true) / 1e6);
        let hole_a = Geometry::Polygon(wkt!(POLYGON((0. 0., 1. 0., 1. 1., 0. 1., 0. 0.), (0.25 0.25, 0.75 0.25, 0.75 0.75, 0.25 0.75, 0.25 0.25))));
        let hole_b = Geometry::Polygon(wkt!(POLYGON((0. 0., 0. 1., 1. 1., 1. 0., 0. 0.), (0.25 0.25, 0.25 0.75, 0.75 0.75, 0.75 0.25, 0.25 0.25))));
        let (a, b) = (area(&hole_a, true) / 1e6, area(&hole_b, true) / 1e6);
        assert!((a - b).abs() < 1e-6 && (a - km2 * 0.75).abs() < 5.0, "{} {} {}", a, b, km2);
    }

    #[test]
    fn clip_points_lines_polygons() {
        let c = Clipper::new([square()].iter()).unwrap();
        // Points: inside, outside and exactly on the boundary (counts as inside).
        assert!(c.has_point(Coord { x: 5., y: 5. }));
        assert!(!c.has_point(Coord { x: 15., y: 5. }));
        assert!(c.has_point(Coord { x: 10., y: 5. }));
        assert!(c.has_point(Coord { x: 0., y: 0. }));

        // A line straight through gives one part inside and two outside.
        let line = Geometry::LineString(wkt!(LINESTRING(-5. 5., 15. 5.)));
        let r = c.clip(&line);
        assert_eq!(length(r.inside.as_ref().unwrap(), false), 10.0);
        assert_eq!(length(r.outside.as_ref().unwrap(), false), 10.0);
        assert_eq!(type_name(r.inside.as_ref().unwrap()), "LineString");
        assert_eq!(type_name(r.outside.as_ref().unwrap()), "MultiLineString");

        // An area that sticks out is split in two.
        let poly = Geometry::Polygon(wkt!(POLYGON((5. 5., 15. 5., 15. 8., 5. 8., 5. 5.))));
        let r = c.clip(&poly);
        assert_eq!(area(r.inside.as_ref().unwrap(), false), 15.0);
        assert_eq!(area(r.outside.as_ref().unwrap(), false), 15.0);

        // A line that only touches a corner, one that follows the boundary and one with several parts.
        let touch = Geometry::LineString(wkt!(LINESTRING(-5. 5., 0. 0., -5. -5.)));
        let r = c.clip(&touch);
        assert!(r.inside.is_none());
        assert_eq!(r.outside, Some(touch));
        let along = Geometry::LineString(wkt!(LINESTRING(-5. 0., 5. 0., 5. 5., 15. 5.)));
        let r = c.clip(&along);
        // The piece along the boundary counts as inside, like points on the boundary.
        assert_eq!(r.inside, Some(Geometry::LineString(wkt!(LINESTRING(0. 0., 5. 0., 5. 5., 10. 5.)))));
        assert_eq!(length(r.outside.as_ref().unwrap(), false), 10.0);
        let zigzag = Geometry::LineString(wkt!(LINESTRING(-1. 1., 1. 1., 1. 12., 3. 12., 3. 1., 12. 1.)));
        let r = c.clip(&zigzag);
        assert!(matches!(r.inside, Some(Geometry::MultiLineString(ref m)) if m.0.len() == 2));
        assert!(matches!(r.outside, Some(Geometry::MultiLineString(ref m)) if m.0.len() == 3));
        assert_eq!(length(r.inside.as_ref().unwrap(), false) + length(r.outside.as_ref().unwrap(), false), length(&zigzag, false));
        assert_eq!(length(r.inside.as_ref().unwrap(), false), 1.0 + 9.0 + 9.0 + 7.0);

        // Entirely outside and entirely inside.
        let far = Geometry::LineString(wkt!(LINESTRING(20. 20., 30. 30.)));
        let r = c.clip(&far);
        assert!(r.inside.is_none() && r.outside.is_some());
        let within = Geometry::Polygon(wkt!(POLYGON((1. 1., 2. 1., 2. 2., 1. 2., 1. 1.))));
        let r = c.clip(&within);
        assert!(r.outside.is_none());
        assert_eq!(area(r.inside.as_ref().unwrap(), false), 1.0);

        // A multipoint is split up.
        let mp = Geometry::MultiPoint(wkt!(MULTIPOINT(1. 1., 20. 20., 3. 3.)));
        let r = c.clip(&mp);
        assert!(matches!(r.inside, Some(Geometry::MultiPoint(ref m)) if m.0.len() == 2));
        assert!(matches!(r.outside, Some(Geometry::MultiPoint(ref m)) if m.0.len() == 1));
    }

    #[test]
    fn clipper_merges_overlapping_areas_and_respects_holes() {
        let a = square();
        let b = Geometry::Polygon(wkt!(POLYGON((5. 0., 20. 0., 20. 10., 5. 10., 5. 0.))));
        let c = Clipper::new([a, b].iter()).unwrap();
        assert_eq!(c.part_count(), 1);
        assert!(c.has_point(Coord { x: 18., y: 5. }));

        let holed = Geometry::Polygon(wkt!(POLYGON((0. 0., 10. 0., 10. 10., 0. 10., 0. 0.), (4. 4., 6. 4., 6. 6., 4. 6., 4. 4.))));
        let c = Clipper::new([holed].iter()).unwrap();
        assert!(!c.has_point(Coord { x: 5., y: 5. }));
        assert!(c.has_point(Coord { x: 4., y: 5. }));
        let line = Geometry::LineString(wkt!(LINESTRING(0. 5., 10. 5.)));
        let r = c.clip(&line);
        assert_eq!(length(r.inside.as_ref().unwrap(), false), 8.0);
        assert_eq!(length(r.outside.as_ref().unwrap(), false), 2.0);
        assert!(Clipper::new([Geometry::Point(Point::new(1., 1.))].iter()).is_none());

        // The same area with the hole wound the other way, and with everything clockwise as in Shapefiles.
        for g in [
            wkt!(POLYGON((0. 0., 10. 0., 10. 10., 0. 10., 0. 0.), (4. 4., 4. 6., 6. 6., 6. 4., 4. 4.))),
            wkt!(POLYGON((0. 0., 0. 10., 10. 10., 10. 0., 0. 0.), (4. 4., 6. 4., 6. 6., 4. 6., 4. 4.))),
        ] {
            let c = Clipper::new([Geometry::Polygon(g)].iter()).unwrap();
            assert!(!c.has_point(Coord { x: 5., y: 5. }));
            assert!(c.has_point(Coord { x: 2., y: 2. }));
            assert!(c.has_point(Coord { x: 6., y: 5. }));
            let r = c.clip(&Geometry::Polygon(wkt!(POLYGON((3. 3., 7. 3., 7. 7., 3. 7., 3. 3.)))));
            assert_eq!(area(r.inside.as_ref().unwrap(), false), 12.0);
            assert_eq!(area(r.outside.as_ref().unwrap(), false), 4.0);
        }
    }

    #[test]
    fn buffer_dissolve_center_generalize() {
        // A point buffered by 10 is close to a circle with radius 10.
        let disc = buffer(&Geometry::Point(Point::new(0., 0.)), 10.0).unwrap();
        let a = area(&disc, false);
        assert!(a > 310.0 && a < std::f64::consts::PI * 100.0 + 0.01, "{}", a);
        // A square grows by its perimeter times the distance, plus a quarter circle per corner.
        let grown = area(&buffer(&square(), 2.0).unwrap(), false);
        assert!((grown - (100.0 + 40.0 * 2.0 + std::f64::consts::PI * 4.0)).abs() < 0.1, "{}", grown);
        // It shrinks without rounding, and disappears when shrunk by more than half its width.
        assert_eq!(area(&buffer(&square(), -1.0).unwrap(), false), 64.0);
        assert!(buffer(&square(), -6.0).is_none());
        assert!(buffer(&Geometry::Point(Point::new(0., 0.)), -1.0).is_none());
        let line = Geometry::LineString(wkt!(LINESTRING(0. 0., 100. 0.)));
        let corridor = area(&buffer(&line, 5.0).unwrap(), false);
        // The round ends are polygons inside the true half circles, so the area is slightly less.
        assert!((corridor - (1000.0 + std::f64::consts::PI * 25.0)).abs() < 1.0, "{}", corridor);

        // Two overlapping squares and one apart: the overlap counts once, the parts stay separate.
        let b = Geometry::Polygon(wkt!(POLYGON((5. 0., 15. 0., 15. 10., 5. 10., 5. 0.))));
        let far = Geometry::Polygon(wkt!(POLYGON((100. 0., 101. 0., 101. 1., 100. 1., 100. 0.))));
        let d = dissolve([square(), b.clone(), far].iter()).unwrap();
        assert_eq!(area(&d, false), 151.0);
        assert!(matches!(d, Geometry::MultiPolygon(ref m) if m.0.len() == 2));
        assert!(matches!(dissolve([square(), b].iter()), Some(Geometry::Polygon(_))));
        assert!(dissolve([line.clone()].iter()).is_none());

        // A C-shaped area: the centre of mass falls in the opening, the inside point does not.
        let c_shape = Geometry::Polygon(wkt!(POLYGON((0. 0., 10. 0., 10. 2., 2. 2., 2. 8., 10. 8., 10. 10., 0. 10., 0. 0.))));
        let index = AreaIndex::new([Some(&c_shape)].into_iter());
        let pt = |g: Option<Geometry<f64>>| match g {
            Some(Geometry::Point(p)) => p.0,
            other => panic!("not a point: {:?}", other),
        };
        assert!(index.containing(pt(center(&c_shape, Center::Centroid))).is_empty());
        assert_eq!(index.containing(pt(center(&c_shape, Center::Inside))), [0]);
        assert_eq!(pt(center(&c_shape, Center::BoundingBox)), Coord { x: 5., y: 5. });
        assert_eq!(pt(center(&line, Center::Centroid)), Coord { x: 50., y: 0. });

        // Generalizing drops the vertex that is 0.1 off the straight line, keeps the one 5 off.
        let wobbly = Geometry::LineString(wkt!(LINESTRING(0. 0., 5. 0.1, 10. 0., 15. 5., 20. 0.)));
        assert_eq!(generalize(&wobbly, 1.0), Geometry::LineString(wkt!(LINESTRING(0. 0., 10. 0., 15. 5., 20. 0.))));
        assert_eq!(vertex_count(&generalize(&wobbly, 10.0)), 2);
        // An area never collapses, however large the tolerance.
        assert_eq!(area(&generalize(&square(), 1000.0), false), 100.0);
    }

    #[test]
    fn area_index_finds_all_containing_areas() {
        let a = square();
        let b = Geometry::Polygon(wkt!(POLYGON((10. 0., 20. 0., 20. 10., 10. 10., 10. 0.))));
        let holed = Geometry::Polygon(wkt!(POLYGON((0. 0., 0. 30., 30. 30., 30. 0., 0. 0.), (12. 12., 18. 12., 18. 18., 12. 18., 12. 12.))));
        let line = Geometry::LineString(wkt!(LINESTRING(0. 0., 30. 30.)));
        let index = AreaIndex::new([Some(&a), None, Some(&b), Some(&line), Some(&holed)].into_iter());
        assert_eq!(index.containing(Coord { x: 5., y: 5. }), [0, 4]);
        // On the shared edge of the two squares: both, and the large one.
        assert_eq!(index.containing(Coord { x: 10., y: 5. }), [0, 2, 4]);
        // In the hole of the large one.
        assert!(index.containing(Coord { x: 15., y: 15. }).is_empty());
        assert_eq!(index.containing(Coord { x: 25., y: 25. }), [4]);
        assert!(index.containing(Coord { x: 99., y: 99. }).is_empty());
    }

    #[test]
    fn transform_keeps_order() {
        let mut gs = [
            Geometry::Point(Point::new(1., 2.)),
            Geometry::Polygon(wkt!(POLYGON((0. 0., 10. 0., 10. 10., 0. 0.), (1. 1., 2. 1., 2. 2., 1. 1.)))),
            Geometry::MultiLineString(wkt!(MULTILINESTRING((0. 0., 1. 1.), (5. 5., 6. 6.)))),
        ];
        transform_all(gs.iter_mut(), |xs, ys| {
            for i in 0..xs.len() {
                xs[i] += 100.0;
                ys[i] *= 2.0;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(gs[0], Geometry::Point(Point::new(101., 4.)));
        assert_eq!(gs[1], Geometry::Polygon(wkt!(POLYGON((100. 0., 110. 0., 110. 20., 100. 0.), (101. 2., 102. 2., 102. 4., 101. 2.)))));
        assert_eq!(gs[2], Geometry::MultiLineString(wkt!(MULTILINESTRING((100. 0., 101. 2.), (105. 10., 106. 12.)))));
    }
}
