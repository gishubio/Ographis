//! Geometry written as text, as web services give it: WKT (and EWKT with SRID=…;), GeoJSON
//! and Esri JSON. Heights (Z) are kept, in the order of geom::coords, as features keep them.

use crate::model::{err, GbError, Result};
use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use serde_json::Value as Json;

/// A corner: x, y and the height, NaN when there is none.
type C = [f64; 3];

#[derive(Debug, Clone)]
enum Shape {
    Point(C),
    MultiPoint(Vec<C>),
    Line(Vec<C>),
    MultiLine(Vec<Vec<C>>),
    Polygon(Vec<Vec<C>>),
    MultiPolygon(Vec<Vec<Vec<C>>>),
    Collection(Vec<Shape>),
}

/// Geometry read from text: the shape, the heights if any, and the coordinate system the
/// text names, if it does.
pub type Parsed = (Geometry<f64>, Option<Vec<f64>>, Option<u32>);

/// What text it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wkt,
    GeoJson,
    EsriJson,
    /// Google's encoded polyline, as OpenTripPlanner, OSRM and Valhalla give routes, with
    /// 5 decimals or, as polyline6, with 6.
    Polyline(u32),
    /// HERE's flexible polyline, with its precision and a third dimension in its header: a
    /// line, or an area when `true`, as the outline of an isoline.
    FlexPolyline(bool),
    /// A JSON list of points, as objects with latitude and longitude (TomTom) or as
    /// [longitude, latitude]: a line, or an area when `true`.
    PointList(bool),
}

/// An encoded polyline as longitude and latitude.
pub fn decode_polyline(text: &str, decimals: u32) -> Result<Vec<Coord<f64>>> {
    let bytes = text.as_bytes();
    let factor = 10f64.powi(decimals as i32);
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    let mut out = Vec::new();
    let next = |i: &mut usize| -> Result<i64> {
        let (mut shift, mut result) = (0u32, 0i64);
        loop {
            let Some(&b) = bytes.get(*i) else { return err("The encoded polyline ends in the middle of a number.") };
            if !(63..=126).contains(&b) {
                return err(format!("\"{}\" does not belong in an encoded polyline.", b as char));
            }
            *i += 1;
            let v = (b - 63) as i64;
            result |= (v & 0x1f) << shift;
            shift += 5;
            if v < 0x20 {
                break;
            }
            if shift > 60 {
                return err("The encoded polyline has a number that is too long.");
            }
        }
        Ok(if result & 1 == 1 { !(result >> 1) } else { result >> 1 })
    };
    while i < bytes.len() {
        lat += next(&mut i)?;
        lon += next(&mut i)?;
        out.push(Coord { x: lon as f64 / factor, y: lat as f64 / factor });
    }
    Ok(out)
}

/// HERE's flexible polyline as longitude, latitude and the third dimension (NaN without):
/// <https://github.com/heremaps/flexible-polyline>.
pub fn decode_flexpolyline(text: &str) -> Result<Vec<C>> {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = text.trim().as_bytes();
    let mut i = 0usize;
    let mut next = || -> Result<Option<u64>> {
        if i >= bytes.len() {
            return Ok(None);
        }
        let (mut shift, mut result) = (0u32, 0u64);
        loop {
            let Some(&b) = bytes.get(i) else { return err("The flexible polyline ends in the middle of a number.") };
            let v = TABLE.iter().position(|c| *c == b).ok_or_else(|| GbError(format!("\"{}\" does not belong in a flexible polyline.", b as char)))? as u64;
            i += 1;
            result |= (v & 0x1f) << shift;
            if v & 0x20 == 0 {
                return Ok(Some(result));
            }
            shift += 5;
            if shift > 60 {
                return err("The flexible polyline has a number that is too long.");
            }
        }
    };
    let version = next()?.ok_or_else(|| GbError("The flexible polyline is empty.".into()))?;
    if version != 1 {
        return err(format!("The flexible polyline is of version {}, and only version 1 is known.", version));
    }
    let header = next()?.ok_or_else(|| GbError("The flexible polyline has no header.".into()))?;
    let precision = (header & 15) as i32;
    let third = (header >> 4) & 7;
    let third_precision = ((header >> 7) & 15) as i32;
    let signed = |v: u64| -> i64 {
        if v & 1 == 1 {
            !((v >> 1) as i64)
        } else {
            (v >> 1) as i64
        }
    };
    let (f, fz) = (10f64.powi(precision), 10f64.powi(third_precision));
    let (mut lat, mut lon, mut z) = (0i64, 0i64, 0i64);
    let mut out = Vec::new();
    while let Some(a) = next()? {
        lat += signed(a);
        lon += signed(next()?.ok_or_else(|| GbError("The flexible polyline ends after a latitude.".into()))?);
        let h = if third != 0 {
            z += signed(next()?.ok_or_else(|| GbError("The flexible polyline ends before a third value.".into()))?);
            z as f64 / fz
        } else {
            f64::NAN
        };
        out.push([lon as f64 / f, lat as f64 / f, h]);
    }
    Ok(out)
}

/// A JSON list of points: objects with latitude and longitude, lat and lon or lng, or arrays
/// of longitude and latitude, with a height as the third number or height, if any.
pub fn point_list(text: &str) -> Result<Vec<C>> {
    let v: Json = serde_json::from_str(text.trim()).map_err(|e| GbError(format!("This is not JSON: {}", e)))?;
    let items = v.as_array().ok_or_else(|| GbError("This is not a list of points.".into()))?;
    let num = |o: &serde_json::Map<String, Json>, keys: &[&str]| {
        keys.iter().find_map(|k| o.get(*k).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))))
    };
    items
        .iter()
        .map(|it| match it {
            Json::Object(o) => match (num(o, &["longitude", "lon", "lng", "x"]), num(o, &["latitude", "lat", "y"])) {
                (Some(x), Some(y)) => Ok([x, y, num(o, &["height", "elevation", "z"]).unwrap_or(f64::NAN)]),
                _ => err("A point of the list has no latitude and longitude."),
            },
            Json::Array(a) if a.len() >= 2 => match (a[0].as_f64(), a[1].as_f64()) {
                (Some(x), Some(y)) => Ok([x, y, a.get(2).and_then(|h| h.as_f64()).unwrap_or(f64::NAN)]),
                _ => err("A point of the list is not two numbers."),
            },
            _ => err("A point of the list is neither an object nor a pair of numbers."),
        })
        .collect()
}

/// Geometry from text, with the heights if it has any, and the coordinate system if the text
/// says. `kind` None looks at the text to know.
pub fn parse(text: &str, kind: Option<Kind>) -> Result<Parsed> {
    let t = text.trim().trim_start_matches('\u{feff}');
    if t.is_empty() {
        return err("The text is empty.");
    }
    // Looked at only when no kind is given: an encoded polyline may well begin with {.
    let kind = match kind {
        Some(k) => k,
        None if t.starts_with('{') => {
            let v: Json = serde_json::from_str(t).map_err(|e| GbError(format!("This is not JSON: {}", e)))?;
            if v.get("type").is_some() {
                Kind::GeoJson
            } else {
                Kind::EsriJson
            }
        }
        None => Kind::Wkt,
    };
    let (shape, crs) = match kind {
        Kind::Polyline(d) => {
            let cs = decode_polyline(t, d)?;
            let shape = match cs.len() {
                0 => return err("The encoded polyline is empty."),
                1 => Shape::Point([cs[0].x, cs[0].y, f64::NAN]),
                _ => Shape::Line(cs.iter().map(|c| [c.x, c.y, f64::NAN]).collect()),
            };
            (shape, Some(4326))
        }
        Kind::FlexPolyline(area) => {
            let cs = decode_flexpolyline(t)?;
            let shape = match cs.len() {
                0 => return err("The flexible polyline is empty."),
                1 => Shape::Point(cs[0]),
                n if area && n >= 3 => Shape::Polygon(vec![cs]),
                _ => Shape::Line(cs),
            };
            (shape, Some(4326))
        }
        Kind::PointList(area) => {
            let cs = point_list(t)?;
            let shape = match cs.len() {
                0 => return err("The list of points is empty."),
                1 => Shape::Point(cs[0]),
                n if area && n >= 3 => Shape::Polygon(vec![cs]),
                _ => Shape::Line(cs),
            };
            (shape, Some(4326))
        }
        Kind::Wkt => wkt(t)?,
        Kind::GeoJson => {
            let v: Json = serde_json::from_str(t).map_err(|e| GbError(format!("This is not JSON: {}", e)))?;
            (geojson(&v)?, None)
        }
        Kind::EsriJson => {
            let v: Json = serde_json::from_str(t).map_err(|e| GbError(format!("This is not JSON: {}", e)))?;
            esri(&v)?
        }
    };
    let mut z = Vec::new();
    let g = to_geo(shape, &mut z);
    let z = z.iter().any(|h| !h.is_nan()).then(|| z.iter().map(|h| if h.is_nan() { 0.0 } else { *h }).collect());
    Ok((g, z, crs))
}

/// Closes a ring, as geo would, so that the heights stay as many as the corners.
fn closed(mut r: Vec<C>) -> Vec<C> {
    if r.len() > 1 && (r[0][0] != r[r.len() - 1][0] || r[0][1] != r[r.len() - 1][1]) {
        r.push(r[0]);
    }
    r
}

fn line(cs: Vec<C>, z: &mut Vec<f64>) -> LineString<f64> {
    z.extend(cs.iter().map(|c| c[2]));
    LineString::new(cs.iter().map(|c| Coord { x: c[0], y: c[1] }).collect())
}

fn poly(rings: Vec<Vec<C>>, z: &mut Vec<f64>) -> Polygon<f64> {
    let mut it = rings.into_iter().map(closed);
    let outer = it.next().map(|r| line(r, z)).unwrap_or_else(|| LineString::new(vec![]));
    let inner: Vec<LineString<f64>> = it.map(|r| line(r, z)).collect();
    Polygon::new(outer, inner)
}

fn to_geo(s: Shape, z: &mut Vec<f64>) -> Geometry<f64> {
    match s {
        Shape::Point(c) => {
            z.push(c[2]);
            Geometry::Point(Point::new(c[0], c[1]))
        }
        Shape::MultiPoint(cs) => {
            z.extend(cs.iter().map(|c| c[2]));
            Geometry::MultiPoint(MultiPoint::new(cs.iter().map(|c| Point::new(c[0], c[1])).collect()))
        }
        Shape::Line(cs) => Geometry::LineString(line(cs, z)),
        Shape::MultiLine(ls) => Geometry::MultiLineString(MultiLineString::new(ls.into_iter().map(|l| line(l, z)).collect())),
        Shape::Polygon(rs) => Geometry::Polygon(poly(rs, z)),
        Shape::MultiPolygon(ps) => Geometry::MultiPolygon(MultiPolygon::new(ps.into_iter().map(|p| poly(p, z)).collect())),
        Shape::Collection(all) => Geometry::GeometryCollection(geo::GeometryCollection::new_from(all.into_iter().map(|s| to_geo(s, z)).collect())),
    }
}

// ---------- WKT ----------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Num(f64),
    Open,
    Close,
    Comma,
}

fn tokens(t: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let b: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' || c == '[' {
            out.push(Tok::Open);
            i += 1;
        } else if c == ')' || c == ']' {
            out.push(Tok::Close);
            i += 1;
        } else if c == ',' {
            out.push(Tok::Comma);
            i += 1;
        } else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], '-' | '+' | '.' | 'e' | 'E')) {
                i += 1;
            }
            let s: String = b[start..i].iter().collect();
            out.push(Tok::Num(s.parse().map_err(|_| GbError(format!("\"{}\" is not a number.", s)))?));
        } else if c.is_alphabetic() {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push(Tok::Word(b[start..i].iter().collect::<String>().to_uppercase()));
        } else {
            return err(format!("The character {} cannot be read in WKT.", c));
        }
    }
    Ok(out)
}

struct W {
    t: Vec<Tok>,
    i: usize,
    /// How many numbers a corner has: 2, 3 for Z or M, 4 for ZM; and whether the third is M.
    dims: usize,
    m: bool,
}

impl W {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn expect(&mut self, want: Tok) -> Result<()> {
        if self.peek() == Some(&want) {
            self.i += 1;
            Ok(())
        } else {
            err(format!("The WKT is not written as expected near token {}.", self.i + 1))
        }
    }
    fn corner(&mut self) -> Result<C> {
        let mut n = Vec::new();
        while let Some(Tok::Num(v)) = self.peek() {
            n.push(*v);
            self.i += 1;
        }
        if n.len() < 2 {
            return err("A corner in the WKT has fewer than two numbers.");
        }
        let z = match (n.len(), self.m) {
            (3, false) | (4, _) => n[2],
            _ => f64::NAN,
        };
        Ok([n[0], n[1], z])
    }
    fn corners(&mut self) -> Result<Vec<C>> {
        self.expect(Tok::Open)?;
        let mut out = Vec::new();
        loop {
            // MULTIPOINT ((1 2), (3 4)) as well as MULTIPOINT (1 2, 3 4).
            if self.peek() == Some(&Tok::Open) {
                self.i += 1;
                out.push(self.corner()?);
                self.expect(Tok::Close)?;
            } else {
                out.push(self.corner()?);
            }
            match self.peek() {
                Some(Tok::Comma) => self.i += 1,
                _ => break,
            }
        }
        self.expect(Tok::Close)?;
        Ok(out)
    }
    fn list<T>(&mut self, mut each: impl FnMut(&mut W) -> Result<T>) -> Result<Vec<T>> {
        self.expect(Tok::Open)?;
        let mut out = vec![each(self)?];
        while self.peek() == Some(&Tok::Comma) {
            self.i += 1;
            out.push(each(self)?);
        }
        self.expect(Tok::Close)?;
        Ok(out)
    }
    fn shape(&mut self) -> Result<Shape> {
        let Some(Tok::Word(kind)) = self.peek().cloned() else { return err("The WKT does not begin with the kind of geometry, such as POINT.") };
        self.i += 1;
        self.dims = 2;
        self.m = false;
        match self.peek() {
            Some(Tok::Word(w)) if w == "Z" => {
                self.i += 1;
            }
            Some(Tok::Word(w)) if w == "M" => {
                self.m = true;
                self.i += 1;
            }
            Some(Tok::Word(w)) if w == "ZM" => {
                self.i += 1;
            }
            _ => {}
        }
        if matches!(self.peek(), Some(Tok::Word(w)) if w == "EMPTY") {
            return err("The geometry is EMPTY.");
        }
        Ok(match kind.as_str() {
            "POINT" => {
                let c = self.corners()?;
                Shape::Point(c[0])
            }
            "MULTIPOINT" => Shape::MultiPoint(self.corners()?),
            "LINESTRING" => Shape::Line(self.corners()?),
            "MULTILINESTRING" => Shape::MultiLine(self.list(|w| w.corners())?),
            "POLYGON" => Shape::Polygon(self.list(|w| w.corners())?),
            "MULTIPOLYGON" => Shape::MultiPolygon(self.list(|w| w.list(|w| w.corners()))?),
            "GEOMETRYCOLLECTION" => Shape::Collection(self.list(|w| w.shape())?),
            other => return err(format!("{} cannot be read; POINT, LINESTRING, POLYGON, their MULTI kinds and GEOMETRYCOLLECTION can.", other)),
        })
    }
}

fn wkt(t: &str) -> Result<(Shape, Option<u32>)> {
    // EWKT, as PostGIS writes: SRID=3006;POINT(…)
    let (srid, body) = match t.split_once(';') {
        Some((head, rest)) if head.trim().to_uppercase().starts_with("SRID=") => (head.trim()[5..].trim().parse::<u32>().ok(), rest),
        _ => (None, t),
    };
    let mut w = W { t: tokens(body)?, i: 0, dims: 2, m: false };
    let s = w.shape()?;
    if w.i < w.t.len() {
        return err("There is more after the geometry in the WKT.");
    }
    Ok((s, srid))
}

// ---------- GeoJSON ----------

fn pos(v: &Json) -> Result<C> {
    let a = v.as_array().ok_or_else(|| GbError("A position in GeoJSON is a list of numbers.".into()))?;
    let n = |i: usize| a.get(i).and_then(|x| x.as_f64());
    match (n(0), n(1)) {
        (Some(x), Some(y)) => Ok([x, y, n(2).unwrap_or(f64::NAN)]),
        _ => err("A position in GeoJSON has fewer than two numbers."),
    }
}
fn positions(v: &Json) -> Result<Vec<C>> {
    v.as_array().ok_or_else(|| GbError("The coordinates are not a list.".into()))?.iter().map(pos).collect()
}
fn rings(v: &Json) -> Result<Vec<Vec<C>>> {
    v.as_array().ok_or_else(|| GbError("The coordinates are not a list.".into()))?.iter().map(positions).collect()
}

fn geojson(v: &Json) -> Result<Shape> {
    let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let c = || v.get("coordinates").ok_or_else(|| GbError(format!("The {} has no coordinates.", kind)));
    Ok(match kind {
        "Feature" => return geojson(v.get("geometry").filter(|g| !g.is_null()).ok_or_else(|| GbError("The feature has no geometry.".into()))?),
        "Point" => Shape::Point(pos(c()?)?),
        "MultiPoint" => Shape::MultiPoint(positions(c()?)?),
        "LineString" => Shape::Line(positions(c()?)?),
        "MultiLineString" => Shape::MultiLine(rings(c()?)?),
        "Polygon" => Shape::Polygon(rings(c()?)?),
        "MultiPolygon" => {
            Shape::MultiPolygon(c()?.as_array().ok_or_else(|| GbError("The coordinates are not a list.".into()))?.iter().map(rings).collect::<Result<_>>()?)
        }
        "GeometryCollection" => Shape::Collection(
            v.get("geometries")
                .and_then(|g| g.as_array())
                .ok_or_else(|| GbError("The collection has no geometries.".into()))?
                .iter()
                .map(geojson)
                .collect::<Result<_>>()?,
        ),
        "FeatureCollection" => return err("This is a whole FeatureCollection. Read it with Input, or take one feature out of it first."),
        other => return err(format!("\"{}\" is not a kind of GeoJSON geometry.", other)),
    })
}

// ---------- Esri JSON ----------

/// Twice the area with a sign: positive when the ring goes against the clock.
fn signed(r: &[C]) -> f64 {
    let mut a = 0.0;
    for w in r.windows(2) {
        a += w[0][0] * w[1][1] - w[1][0] * w[0][1];
    }
    if let (Some(f), Some(l)) = (r.first(), r.last()) {
        a += l[0] * f[1] - f[0] * l[1];
    }
    a
}

fn inside(p: C, r: &[C]) -> bool {
    let mut odd = false;
    let n = r.len();
    for i in 0..n {
        let (a, b) = (r[i], r[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            odd = !odd;
        }
    }
    odd
}

fn esri(v: &Json) -> Result<(Shape, Option<u32>)> {
    let sr = v.get("spatialReference").and_then(|s| s.get("latestWkid").or_else(|| s.get("wkid"))).and_then(|w| w.as_u64()).map(|w| match w {
        102100 | 102113 => 3857,
        w => w as u32,
    });
    let corner = |p: &Json| -> Result<C> { pos(p) };
    let shape = if let (Some(x), Some(y)) = (v.get("x").and_then(|x| x.as_f64()), v.get("y").and_then(|y| y.as_f64())) {
        Shape::Point([x, y, v.get("z").and_then(|z| z.as_f64()).unwrap_or(f64::NAN)])
    } else if let Some(pts) = v.get("points").and_then(|p| p.as_array()) {
        Shape::MultiPoint(pts.iter().map(corner).collect::<Result<_>>()?)
    } else if let Some(paths) = v.get("paths") {
        let mut ls = rings(paths)?;
        if ls.len() == 1 {
            Shape::Line(ls.remove(0))
        } else {
            Shape::MultiLine(ls)
        }
    } else if let Some(rs) = v.get("rings") {
        // Outer rings go with the clock, holes against it; each hole goes in the outer ring
        // it lies in.
        let all = rings(rs)?;
        let (mut outer, mut holes): (Vec<Vec<Vec<C>>>, Vec<Vec<C>>) = (Vec::new(), Vec::new());
        for r in all {
            if signed(&r) <= 0.0 {
                outer.push(vec![r]);
            } else {
                holes.push(r);
            }
        }
        if outer.is_empty() {
            // Rings that all go one way: taken as outer rings.
            outer = holes.drain(..).map(|r| vec![r]).collect();
        }
        for h in holes {
            let at = outer.iter().position(|o| h.first().is_some_and(|p| inside(*p, &o[0]))).unwrap_or(0);
            outer[at].push(h);
        }
        if outer.len() == 1 {
            Shape::Polygon(outer.remove(0))
        } else {
            Shape::MultiPolygon(outer)
        }
    } else {
        return err("This Esri JSON has no x and y, points, paths or rings.");
    };
    Ok((shape, sr))
}

#[cfg(test)]
mod polyline_tests {
    #[test]
    fn the_example_of_google_is_decoded() {
        // From the description of the format by Google.
        let cs = super::decode_polyline("_p~iF~ps|U_ulLnnqC_mqNvxq`@", 5).unwrap();
        let want = [(-120.2, 38.5), (-120.95, 40.7), (-126.453, 43.252)];
        assert_eq!(cs.len(), 3);
        for (c, w) in cs.iter().zip(want) {
            assert!((c.x - w.0).abs() < 1e-9 && (c.y - w.1).abs() < 1e-9, "{:?}", c);
        }
        assert!(super::decode_polyline("_p~iF~ps|", 5).is_err());
        let (g, _, crs) = super::parse("_p~iF~ps|U_ulLnnqC_mqNvxq`@", Some(super::Kind::Polyline(5))).unwrap();
        assert!(matches!(g, geo::Geometry::LineString(_)));
        assert_eq!(crs, Some(4326));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_polyline_that_begins_with_a_brace_is_not_taken_for_json() {
        let (g, _, _) = parse("{hj_JkgpgAoKgE", Some(Kind::Polyline(5))).unwrap();
        assert!(matches!(g, Geometry::LineString(_)));
    }

    #[test]
    fn heres_flexible_polyline_and_tomtoms_list_of_points() {
        // The example of the specification.
        let cs = decode_flexpolyline("BFoz5xJ67i1B1B7PzIhaxL7Y").unwrap();
        let want = [(50.10228, 8.69821), (50.10201, 8.69567), (50.10063, 8.69150), (50.09878, 8.68752)];
        assert_eq!(cs.len(), 4);
        for (c, w) in cs.iter().zip(want) {
            assert!((c[1] - w.0).abs() < 1e-9 && (c[0] - w.1).abs() < 1e-9, "{:?}", c);
        }
        let (g, _, crs) = parse(r#"[{"latitude": 57.7, "longitude": 11.9}, {"latitude": 57.71, "longitude": 11.95}]"#, Some(Kind::PointList(false))).unwrap();
        assert!(matches!(g, Geometry::LineString(ref l) if l.0[1].x == 11.95) && crs == Some(4326));
        let (g, _, _) = parse("[[11.9, 57.7], [11.95, 57.7], [11.95, 57.75]]", Some(Kind::PointList(true))).unwrap();
        assert!(matches!(g, Geometry::Polygon(ref p) if p.exterior().0.len() == 4));
    }
    use geo::Area;

    #[test]
    fn wkt_ewkt_with_heights() {
        let (g, z, crs) = parse("POINT (12.5 57.7)", None).unwrap();
        assert_eq!(g, Geometry::Point(Point::new(12.5, 57.7)));
        assert!(z.is_none() && crs.is_none());
        let (g, z, crs) = parse("SRID=3006;LINESTRING Z (0 0 10, 10 0 12.5)", None).unwrap();
        assert!(matches!(g, Geometry::LineString(_)));
        assert_eq!(z.unwrap(), vec![10.0, 12.5]);
        assert_eq!(crs, Some(3006));
        let (g, _, _) = parse("MULTIPOINT ((1 2), (3 4))", None).unwrap();
        assert!(matches!(g, Geometry::MultiPoint(m) if m.0.len() == 2));
        let (g, _, _) = parse("multipolygon(((0 0, 4 0, 4 4, 0 4, 0 0), (1 1, 2 1, 2 2, 1 1)))", None).unwrap();
        assert!((g.unsigned_area() - 15.5).abs() < 1e-9);
        // M is not a height.
        let (_, z, _) = parse("POINT M (1 2 99)", None).unwrap();
        assert!(z.is_none());
        assert!(parse("POINT EMPTY", None).is_err());
        assert!(parse("CIRCLE (1 2)", None).unwrap_err().0.contains("cannot be read"));
    }

    #[test]
    fn geojson_and_esri_json() {
        let (g, z, _) = parse(r#"{"type": "Polygon", "coordinates": [[[0,0,1],[2,0,1],[2,2,1],[0,2,1]]]}"#, None).unwrap();
        assert!((g.unsigned_area() - 4.0).abs() < 1e-9);
        assert_eq!(z.unwrap().len(), 5, "the ring is closed, and the heights with it");
        let (g, _, _) = parse(r#"{"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": [1, 2]}}"#, None).unwrap();
        assert_eq!(g, Geometry::Point(Point::new(1.0, 2.0)));
        let (g, _, crs) = parse(r#"{"x": 319500, "y": 6399800, "spatialReference": {"wkid": 3006}}"#, None).unwrap();
        assert_eq!(g, Geometry::Point(Point::new(319500.0, 6399800.0)));
        assert_eq!(crs, Some(3006));
        // An outer ring with the clock and a hole against it; and one more outer ring.
        let rings = r#"{"rings": [[[0,0],[0,10],[10,10],[10,0],[0,0]], [[2,2],[4,2],[4,4],[2,4],[2,2]], [[20,0],[20,5],[25,5],[25,0],[20,0]]],
            "spatialReference": {"wkid": 102100, "latestWkid": 3857}}"#;
        let (g, _, crs) = parse(rings, None).unwrap();
        assert_eq!(crs, Some(3857));
        let Geometry::MultiPolygon(m) = &g else { panic!("{:?}", g) };
        assert_eq!(m.0.len(), 2);
        assert_eq!(m.0[0].interiors().len(), 1);
        assert!((g.unsigned_area() - 96.0 - 25.0).abs() < 1e-9);
        let (g, _, _) = parse(r#"{"paths": [[[0,0],[1,1]]]}"#, None).unwrap();
        assert!(matches!(g, Geometry::LineString(_)));
    }
}
