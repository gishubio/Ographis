//! Writes vector tiles into one PMTiles file, as MapLibre, Leaflet and OpenLayers draw maps
//! from it straight off a web server, without a tile server: the features are cut into tiles
//! of Mapbox Vector Tiles for each zoom level, and the tiles put in the file in the order of
//! a Hilbert curve, with a directory of where each is.
//!
//! The format: <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>, and the
//! tiles: <https://github.com/mapbox/vector-tile-spec/tree/master/2.1>.

use crate::model::{GbError, Result, Table, Value};
use geo::{Coord, Geometry};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// The size of a tile in its own coordinates, and the margin around it that features reach
/// into, so that lines and the edges of areas do not show where the tiles meet.
const EXTENT: f64 = 4096.0;
const MARGIN: f64 = 64.0;

/// What to write: the zoom levels and the name of the layer.
pub struct PmOpts {
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub layer: String,
}

/// What was written: tiles, tiles with the same content written once, and the largest tile.
#[derive(Debug, Default)]
pub struct PmStats {
    pub tiles: u64,
    pub contents: u64,
    pub largest: usize,
    pub features: u64,
}

// ---------- Protocol buffers, as much as tiles need ----------

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn key(out: &mut Vec<u8>, field: u32, wire: u32) {
    varint(out, ((field << 3) | wire) as u64);
}

fn bytes_field(out: &mut Vec<u8>, field: u32, b: &[u8]) {
    key(out, field, 2);
    varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

// ---------- Web Mercator and the tiles ----------

/// A place in degrees as a fraction of the world, 0..1 from west and from north.
fn world(c: Coord<f64>) -> Coord<f64> {
    let lat = c.y.clamp(-85.051_128_78, 85.051_128_78).to_radians();
    Coord { x: (c.x + 180.0) / 360.0, y: (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 }
}

/// The number of a tile along the Hilbert curve of PMTiles, after all the tiles of the zoom
/// levels before it.
pub fn tile_id(z: u8, x: u64, y: u64) -> u64 {
    let acc: u64 = (0..z).map(|i| 1u64 << (2 * i as u64)).sum();
    let n = 1u64 << z;
    let (mut tx, mut ty) = (x, y);
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(tx & s > 0);
        let ry = u64::from(ty & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                tx = n - 1 - tx;
                ty = n - 1 - ty;
            }
            std::mem::swap(&mut tx, &mut ty);
        }
        s /= 2;
    }
    acc + d
}

/// The geometry of a feature as parts in world fractions: points, lines or rings.
enum Shape {
    Points(Vec<Coord<f64>>),
    Lines(Vec<Vec<Coord<f64>>>),
    /// Polygons, each its rings, the outer first.
    Areas(Vec<Vec<Vec<Coord<f64>>>>),
}

fn shape_of(g: &Geometry<f64>) -> Option<Shape> {
    let ring = |l: &geo::LineString<f64>| l.0.iter().map(|c| world(*c)).collect::<Vec<_>>();
    let poly = |p: &geo::Polygon<f64>| std::iter::once(ring(p.exterior())).chain(p.interiors().iter().map(ring)).collect::<Vec<_>>();
    Some(match g {
        Geometry::Point(p) => Shape::Points(vec![world(p.0)]),
        Geometry::MultiPoint(m) => Shape::Points(m.0.iter().map(|p| world(p.0)).collect()),
        Geometry::LineString(l) => Shape::Lines(vec![ring(l)]),
        Geometry::MultiLineString(m) => Shape::Lines(m.0.iter().map(ring).collect()),
        Geometry::Line(l) => Shape::Lines(vec![vec![world(l.start), world(l.end)]]),
        Geometry::Polygon(p) => Shape::Areas(vec![poly(p)]),
        Geometry::MultiPolygon(m) => Shape::Areas(m.0.iter().map(poly).collect()),
        Geometry::Rect(r) => Shape::Areas(vec![poly(&r.to_polygon())]),
        Geometry::Triangle(t) => Shape::Areas(vec![poly(&t.to_polygon())]),
        Geometry::GeometryCollection(_) => return None,
    })
}

fn bounds(s: &Shape) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut add = |c: &Coord<f64>| {
        b = [b[0].min(c.x), b[1].min(c.y), b[2].max(c.x), b[3].max(c.y)];
    };
    match s {
        Shape::Points(p) => p.iter().for_each(&mut add),
        Shape::Lines(l) => l.iter().flatten().for_each(&mut add),
        Shape::Areas(a) => a.iter().flat_map(|p| p.first()).flatten().for_each(&mut add),
    }
    b[0].is_finite().then_some(b)
}

/// A ring cut to a rectangle, one side at a time (Sutherland–Hodgman). What lies along the
/// edge of the rectangle is drawn outside the tile, in the margin, and does not show.
fn clip_ring(ring: &[Coord<f64>], lo: f64, hi: f64) -> Vec<Coord<f64>> {
    let mut pts: Vec<Coord<f64>> = ring.to_vec();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    for side in 0..4 {
        if pts.is_empty() {
            break;
        }
        let inside = |c: &Coord<f64>| match side {
            0 => c.x >= lo,
            1 => c.x <= hi,
            2 => c.y >= lo,
            _ => c.y <= hi,
        };
        let cross = |a: &Coord<f64>, b: &Coord<f64>| {
            let (edge, va, vb) = match side {
                0 => (lo, a.x, b.x),
                1 => (hi, a.x, b.x),
                2 => (lo, a.y, b.y),
                _ => (hi, a.y, b.y),
            };
            let t = (edge - va) / (vb - va);
            Coord { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }
        };
        let mut out = Vec::with_capacity(pts.len() + 4);
        for i in 0..pts.len() {
            let (a, b) = (&pts[(i + pts.len() - 1) % pts.len()], &pts[i]);
            match (inside(a), inside(b)) {
                (true, true) => out.push(*b),
                (true, false) => out.push(cross(a, b)),
                (false, true) => {
                    out.push(cross(a, b));
                    out.push(*b);
                }
                (false, false) => {}
            }
        }
        pts = out;
    }
    pts
}

/// A line cut to a rectangle: the parts of it inside.
fn clip_line(line: &[Coord<f64>], lo: f64, hi: f64) -> Vec<Vec<Coord<f64>>> {
    let mut parts = Vec::new();
    let mut cur: Vec<Coord<f64>> = Vec::new();
    let inside = |c: &Coord<f64>| c.x >= lo && c.x <= hi && c.y >= lo && c.y <= hi;
    for w in line.windows(2) {
        // Liang–Barsky: the part of the segment inside the rectangle.
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut keep = true;
        for (p, q) in [(-dx, a.x - lo), (dx, hi - a.x), (-dy, a.y - lo), (dy, hi - a.y)] {
            if p == 0.0 {
                if q < 0.0 {
                    keep = false;
                    break;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
        if !keep || t0 > t1 {
            if cur.len() >= 2 {
                parts.push(std::mem::take(&mut cur));
            }
            cur.clear();
            continue;
        }
        let s = Coord { x: a.x + dx * t0, y: a.y + dy * t0 };
        let e = Coord { x: a.x + dx * t1, y: a.y + dy * t1 };
        if cur.last() != Some(&s) {
            if cur.len() >= 2 {
                parts.push(std::mem::take(&mut cur));
            }
            cur = vec![s];
        }
        cur.push(e);
        if !inside(&b) {
            if cur.len() >= 2 {
                parts.push(std::mem::take(&mut cur));
            }
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        parts.push(cur);
    }
    parts
}

/// Points in tile coordinates rounded, with points that fall on the one before left out.
fn snap(pts: &[Coord<f64>]) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(pts.len());
    for c in pts {
        let p = (c.x.round() as i64, c.y.round() as i64);
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

/// Twice the area of a ring in tile coordinates, positive when it turns clockwise on the
/// screen, as the outer ring of an area in a vector tile is to.
fn area2(r: &[(i64, i64)]) -> i64 {
    let mut s = 0i64;
    for i in 0..r.len() {
        let (a, b) = (r[i], r[(i + 1) % r.len()]);
        s += a.0 * b.1 - b.0 * a.1;
    }
    s
}

/// The commands of a geometry: MoveTo, LineTo and ClosePath with their offsets.
struct Pen {
    cmds: Vec<u32>,
    at: (i64, i64),
}

impl Pen {
    fn cmd(&mut self, id: u32, count: u32) {
        self.cmds.push((id & 0x7) | (count << 3));
    }
    fn to(&mut self, p: (i64, i64)) {
        self.cmds.push(zigzag(p.0 - self.at.0) as u32);
        self.cmds.push(zigzag(p.1 - self.at.1) as u32);
        self.at = p;
    }
    fn path(&mut self, pts: &[(i64, i64)], close: bool) {
        self.cmd(1, 1);
        self.to(pts[0]);
        self.cmd(2, (pts.len() - 1) as u32);
        for p in &pts[1..] {
            self.to(*p);
        }
        if close {
            self.cmd(7, 1);
        }
    }
}

/// A feature in a tile: its type and its commands, or None when nothing of it is in the tile.
fn encode(shape: &Shape, z: u8, x: u64, y: u64) -> Option<(u32, Vec<u32>)> {
    let n = (1u64 << z) as f64;
    let local = |c: &Coord<f64>| Coord { x: (c.x * n - x as f64) * EXTENT, y: (c.y * n - y as f64) * EXTENT };
    let (lo, hi) = (-MARGIN, EXTENT + MARGIN);
    let mut pen = Pen { cmds: Vec::new(), at: (0, 0) };
    match shape {
        Shape::Points(ps) => {
            let pts: Vec<(i64, i64)> = ps
                .iter()
                .map(local)
                .filter(|c| c.x >= 0.0 && c.x < EXTENT && c.y >= 0.0 && c.y < EXTENT)
                .map(|c| (c.x.round() as i64, c.y.round() as i64))
                .collect();
            if pts.is_empty() {
                return None;
            }
            pen.cmd(1, pts.len() as u32);
            for p in pts {
                pen.to(p);
            }
            Some((1, pen.cmds))
        }
        Shape::Lines(ls) => {
            for l in ls {
                let loc: Vec<Coord<f64>> = l.iter().map(local).collect();
                for part in clip_line(&loc, lo, hi) {
                    let s = snap(&part);
                    if s.len() >= 2 {
                        pen.path(&s, false);
                    }
                }
            }
            (!pen.cmds.is_empty()).then_some((2, pen.cmds))
        }
        Shape::Areas(polys) => {
            for poly in polys {
                for (k, ring) in poly.iter().enumerate() {
                    let loc: Vec<Coord<f64>> = ring.iter().map(local).collect();
                    let mut s = snap(&clip_ring(&loc, lo, hi));
                    if s.len() > 1 && s.first() == s.last() {
                        s.pop();
                    }
                    if s.len() < 3 {
                        if k == 0 {
                            break;
                        }
                        continue;
                    }
                    let a = area2(&s);
                    if a == 0 {
                        if k == 0 {
                            break;
                        }
                        continue;
                    }
                    // Outer rings clockwise on the screen, holes the other way.
                    if (k == 0) != (a > 0) {
                        s.reverse();
                    }
                    pen.path(&s, true);
                }
            }
            (!pen.cmds.is_empty()).then_some((3, pen.cmds))
        }
    }
}

/// A value of an attribute as a Value message of a tile, and its type in the metadata.
fn value_bytes(v: &Value) -> Option<(Vec<u8>, &'static str)> {
    let mut out = Vec::new();
    let kind = match v {
        Value::Null => return None,
        Value::Bool(b) => {
            key(&mut out, 7, 0);
            varint(&mut out, u64::from(*b));
            "Boolean"
        }
        Value::Int(i) => {
            key(&mut out, 6, 0);
            varint(&mut out, zigzag(*i));
            "Number"
        }
        Value::Real(r) => {
            if !r.is_finite() {
                return None;
            }
            key(&mut out, 3, 1);
            out.extend_from_slice(&r.to_le_bytes());
            "Number"
        }
        other => {
            bytes_field(&mut out, 1, other.to_text().as_bytes());
            "String"
        }
    };
    Some((out, kind))
}

/// One tile being made: its features, and the keys and values they use.
#[derive(Default)]
struct Tile {
    features: Vec<u8>,
    count: usize,
    keys: Vec<String>,
    key_at: HashMap<String, u32>,
    values: Vec<Vec<u8>>,
    value_at: HashMap<Vec<u8>, u32>,
}

impl Tile {
    fn add(&mut self, id: u64, ty: u32, geom: &[u32], attrs: &[(&str, &[u8])]) {
        let mut tags: Vec<u32> = Vec::with_capacity(attrs.len() * 2);
        for (k, v) in attrs {
            let ki = match self.key_at.get(*k) {
                Some(i) => *i,
                None => {
                    let i = self.keys.len() as u32;
                    self.keys.push(k.to_string());
                    self.key_at.insert(k.to_string(), i);
                    i
                }
            };
            let vi = match self.value_at.get(*v) {
                Some(i) => *i,
                None => {
                    let i = self.values.len() as u32;
                    self.values.push(v.to_vec());
                    self.value_at.insert(v.to_vec(), i);
                    i
                }
            };
            tags.push(ki);
            tags.push(vi);
        }
        let mut f = Vec::new();
        key(&mut f, 1, 0);
        varint(&mut f, id);
        if !tags.is_empty() {
            let mut packed = Vec::new();
            for t in tags {
                varint(&mut packed, t as u64);
            }
            bytes_field(&mut f, 2, &packed);
        }
        key(&mut f, 3, 0);
        varint(&mut f, ty as u64);
        let mut packed = Vec::new();
        for c in geom {
            varint(&mut packed, *c as u64);
        }
        bytes_field(&mut f, 4, &packed);
        bytes_field(&mut self.features, 2, &f);
        self.count += 1;
    }

    fn finish(self, layer: &str) -> Vec<u8> {
        let mut l = Vec::new();
        key(&mut l, 15, 0);
        varint(&mut l, 2);
        bytes_field(&mut l, 1, layer.as_bytes());
        l.extend_from_slice(&self.features);
        for k in &self.keys {
            bytes_field(&mut l, 3, k.as_bytes());
        }
        for v in &self.values {
            bytes_field(&mut l, 4, v);
        }
        key(&mut l, 5, 0);
        varint(&mut l, EXTENT as u64);
        let mut t = Vec::new();
        bytes_field(&mut t, 3, &l);
        t
    }
}

fn gzip(b: &[u8]) -> Result<Vec<u8>> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).and_then(|_| e.finish()).map_err(|e| GbError(format!("Cannot compress a tile: {}", e)))
}

/// An entry of a directory: the first tile, where its content is, and how many tiles in a
/// row have it. A run of 0 points to a directory of leaves.
#[derive(Clone, Copy)]
struct Entry {
    id: u64,
    offset: u64,
    length: u64,
    run: u64,
}

fn directory(entries: &[Entry]) -> Result<Vec<u8>> {
    let mut b = Vec::new();
    varint(&mut b, entries.len() as u64);
    let mut last = 0;
    for e in entries {
        varint(&mut b, e.id - last);
        last = e.id;
    }
    for e in entries {
        varint(&mut b, e.run);
    }
    for e in entries {
        varint(&mut b, e.length);
    }
    for (i, e) in entries.iter().enumerate() {
        if i > 0 && e.offset == entries[i - 1].offset + entries[i - 1].length {
            varint(&mut b, 0);
        } else {
            varint(&mut b, e.offset + 1);
        }
    }
    gzip(&b)
}

/// The root directory, and the directories of leaves when the root alone would be too large
/// for the first 16 KiB of the file that a map fetches first.
fn directories(entries: &[Entry]) -> Result<(Vec<u8>, Vec<u8>)> {
    let root = directory(entries)?;
    if root.len() <= 16384 - 127 {
        return Ok((root, Vec::new()));
    }
    let mut size = 4096usize;
    loop {
        let mut leaves = Vec::new();
        let mut root_entries = Vec::new();
        for chunk in entries.chunks(size) {
            let leaf = directory(chunk)?;
            root_entries.push(Entry { id: chunk[0].id, offset: leaves.len() as u64, length: leaf.len() as u64, run: 0 });
            leaves.extend_from_slice(&leaf);
        }
        let root = directory(&root_entries)?;
        if root.len() <= 16384 - 127 {
            return Ok((root, leaves));
        }
        size *= 2;
    }
}

/// Writes the features of a table, in WGS 84, as vector tiles into a PMTiles file.
pub fn write_pmtiles(t: &Table, path: &Path, o: &PmOpts, progress: &dyn Fn(u64, u64), going: &dyn Fn() -> bool) -> Result<PmStats> {
    if t.crs.is_some_and(|c| c != 4326) {
        return Err(GbError("The features must be in WGS 84 to be cut into tiles.".into()));
    }
    let (minz, maxz) = (o.min_zoom.min(o.max_zoom), o.max_zoom.max(o.min_zoom).min(16));
    // The attributes of each feature as values of tiles, once.
    let mut field_types: Vec<(String, &'static str)> = Vec::new();
    let names: Vec<&str> = t.fields.iter().map(|f| f.name.as_str()).collect();
    let mut kinds: Vec<Option<&'static str>> = vec![None; names.len()];
    let shapes: Vec<Option<(Shape, [f64; 4])>> =
        t.features.iter().map(|f| f.geom.as_ref().and_then(shape_of).and_then(|s| bounds(&s).map(|b| (s, b)))).collect();
    let values: Vec<Vec<Option<Vec<u8>>>> = t
        .features
        .iter()
        .map(|f| {
            f.attrs
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    value_bytes(v).map(|(b, k)| {
                        if i < kinds.len() {
                            kinds[i] = Some(match kinds[i] {
                                Some(old) if old != k => "String",
                                _ => k,
                            });
                        }
                        b
                    })
                })
                .collect()
        })
        .collect();
    for (i, n) in names.iter().enumerate() {
        if let Some(k) = kinds[i] {
            field_types.push((n.to_string(), k));
        }
    }
    // The tiles of each zoom level, each with its features.
    let mut stats = PmStats::default();
    let mut entries: Vec<Entry> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let mut seen: HashMap<Vec<u8>, (u64, u64)> = HashMap::new();
    let mut world_bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (_, b) in shapes.iter().flatten() {
        world_bounds = [world_bounds[0].min(b[0]), world_bounds[1].min(b[1]), world_bounds[2].max(b[2]), world_bounds[3].max(b[3])];
    }
    if !world_bounds[0].is_finite() {
        return Err(GbError("There is nothing with a geometry to cut into tiles.".into()));
    }
    let levels = (maxz - minz + 1) as u64;
    for (step, z) in (minz..=maxz).enumerate() {
        progress(step as u64, levels);
        if !going() {
            return Err(GbError(crate::model::STOPPED.into()));
        }
        let n = (1u64 << z) as f64;
        let pad = MARGIN / EXTENT;
        let last = (1u64 << z) - 1;
        let cell = |v: f64| ((v * n).floor().max(0.0) as u64).min(last);
        // Which features each tile has.
        let mut in_tile: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
        for (i, s) in shapes.iter().enumerate() {
            let Some((_, b)) = s else { continue };
            for x in cell(b[0] - pad / n)..=cell(b[2] + pad / n) {
                for y in cell(b[1] - pad / n)..=cell(b[3] + pad / n) {
                    in_tile.entry((x, y)).or_default().push(i);
                }
            }
        }
        let mut tiles: Vec<(u64, Vec<u8>)> = Vec::with_capacity(in_tile.len());
        for ((x, y), feats) in in_tile {
            let mut tile = Tile::default();
            for i in feats {
                let (shape, _) = shapes[i].as_ref().expect("a shape");
                if let Some((ty, geom)) = encode(shape, z, x, y) {
                    let attrs: Vec<(&str, &[u8])> = values[i].iter().enumerate().filter_map(|(k, v)| v.as_ref().map(|b| (names[k], b.as_slice()))).collect();
                    tile.add(i as u64 + 1, ty, &geom, &attrs);
                }
            }
            if tile.count == 0 {
                continue;
            }
            stats.features += tile.count as u64;
            tiles.push((tile_id(z, x, y), gzip(&tile.finish(&o.layer))?));
        }
        tiles.sort_by_key(|t| t.0);
        for (id, bytes) in tiles {
            stats.tiles += 1;
            stats.largest = stats.largest.max(bytes.len());
            let (offset, length) = match seen.get(&bytes) {
                Some(at) => *at,
                None => {
                    let at = (data.len() as u64, bytes.len() as u64);
                    data.extend_from_slice(&bytes);
                    stats.contents += 1;
                    seen.insert(bytes, at);
                    at
                }
            };
            match entries.last_mut() {
                Some(e) if e.offset == offset && e.length == length && e.id + e.run == id => e.run += 1,
                _ => entries.push(Entry { id, offset, length, run: 1 }),
            }
        }
    }
    let (root, leaves) = directories(&entries)?;
    // The rectangle in degrees, back from the world fractions.
    let lon = |v: f64| v * 360.0 - 180.0;
    let lat = |v: f64| (std::f64::consts::PI * (1.0 - 2.0 * v)).sinh().atan().to_degrees();
    let rect = [lon(world_bounds[0]), lat(world_bounds[3]), lon(world_bounds[2]), lat(world_bounds[1])];
    let fields: serde_json::Map<String, serde_json::Value> = field_types.iter().map(|(n, k)| (n.clone(), serde_json::Value::String(k.to_string()))).collect();
    let meta = serde_json::json!({
        "name": o.layer,
        "format": "pbf",
        "type": "overlay",
        "generator": format!("Ographis {}", env!("CARGO_PKG_VERSION")),
        "vector_layers": [{ "id": o.layer, "fields": fields, "minzoom": minz, "maxzoom": maxz }],
    });
    let meta = gzip(meta.to_string().as_bytes())?;
    // The header, then the root, the metadata, the leaves and the tiles.
    let root_at = 127u64;
    let meta_at = root_at + root.len() as u64;
    let leaves_at = meta_at + meta.len() as u64;
    let data_at = leaves_at + leaves.len() as u64;
    let mut h: Vec<u8> = Vec::with_capacity(127);
    h.extend_from_slice(b"PMTiles");
    h.push(3);
    for v in [
        root_at,
        root.len() as u64,
        meta_at,
        meta.len() as u64,
        leaves_at,
        leaves.len() as u64,
        data_at,
        data.len() as u64,
        stats.tiles,
        entries.len() as u64,
        stats.contents,
    ] {
        h.extend_from_slice(&v.to_le_bytes());
    }
    h.extend_from_slice(&[1, 2, 2, 1, minz, maxz]);
    let e7 = |v: f64| ((v * 1e7).round() as i32).to_le_bytes();
    for v in rect {
        h.extend_from_slice(&e7(v));
    }
    h.push(minz.max(((minz as u16 + maxz as u16) / 2) as u8));
    h.extend_from_slice(&e7((rect[0] + rect[2]) / 2.0));
    h.extend_from_slice(&e7((rect[1] + rect[3]) / 2.0));
    debug_assert_eq!(h.len(), 127);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?);
    for part in [&h, &root, &meta, &leaves, &data] {
        f.write_all(part).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    f.flush().map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_are_numbered_along_the_hilbert_curve() {
        // The numbers in the specification of PMTiles.
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(2, 0, 0), 5);
        assert_eq!(tile_id(12, 3423, 1763), 19078479);
    }

    #[test]
    fn rings_and_lines_are_cut_to_the_tile() {
        let sq = vec![Coord { x: -10.0, y: -10.0 }, Coord { x: 10.0, y: -10.0 }, Coord { x: 10.0, y: 10.0 }, Coord { x: -10.0, y: 10.0 }];
        let c = clip_ring(&sq, 0.0, 100.0);
        assert_eq!(c.len(), 4);
        assert!(c.iter().all(|p| p.x >= 0.0 && p.y >= 0.0));
        let l = clip_line(&[Coord { x: -50.0, y: 50.0 }, Coord { x: 150.0, y: 50.0 }], 0.0, 100.0);
        assert_eq!(l, vec![vec![Coord { x: 0.0, y: 50.0 }, Coord { x: 100.0, y: 50.0 }]]);
    }
}
