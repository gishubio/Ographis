//! Reads 3D Tiles, the OGC standard for streaming 3D content such as city models, from a
//! tileset.json on this computer or on the web.
//!
//! A tileset is a tree of tiles. Each tile covers a volume and can have content: a 3D model
//! as glTF (3D Tiles 1.1), as Batched 3D Model (.b3dm, glTF with a table of its features),
//! or a composite of those (.cmpt). A tile is refined by its children, which either replace
//! it or add to it. Which tiles are read is chosen by their geometric error: the finest
//! level, the coarsest, or down to an error in metres. A tile's content can also be
//! another tileset, which is read as a branch of the tree.
//!
//! The models are placed by the transforms of the tiles, most often in earth-centred
//! coordinates (ECEF, EPSG:4978), and from there into the coordinate system of the map:
//! through latitude, longitude and the height above the ellipsoid. Normals are turned to
//! east, north and up where each tile is.
//!
//! Not read yet: instanced models (.i3dm), point clouds (.pnts) and implicit tiling.

use crate::gltf_io;
use crate::io::{Backend, Notes};
use crate::mesh::{Mesh, Tri, NONE};
use crate::model::{err, fmt_count, Field, FieldType, GbError, Result, Table, Value};
use crate::obj_io::model_table;
use serde_json::Value as Json;
use std::collections::HashMap;

type M4 = [f64; 16];
const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Which tiles of the tree are read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Detail {
    /// The tiles at the bottom of the tree, and those whose children add to them.
    Finest,
    /// The first tiles from the top that have content.
    Coarsest,
    /// Tiles down to a geometric error in metres: where a tile's error is at most this, its
    /// children are not needed.
    Error(f64),
}

pub struct TilesOpts<'a> {
    pub detail: Detail,
    /// The largest number of tiles to read. More is an error that says how to read fewer.
    pub most: usize,
    /// The coordinate system to place the models in. None: the UTM zone of the tileset.
    pub crs: Option<u32>,
    /// Only tiles that reach into this rectangle, as west, south, east and north in degrees.
    pub within: Option<[f64; 4]>,
    /// Added to every height, such as from the ellipsoid to a height system of the country.
    pub dz: f64,
    /// Headers sent with every request, such as a key.
    pub headers: &'a [(String, String)],
}

// ---------- Matrices and the earth ----------

/// a × b, both column-major as 3D Tiles and glTF write them.
fn mul(a: &M4, b: &M4) -> M4 {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    out
}

fn apply(m: &M4, p: [f64; 3]) -> [f64; 3] {
    [m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12], m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13], m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14]]
}

fn turn(m: &M4, v: [f64; 3]) -> [f64; 3] {
    [m[0] * v[0] + m[4] * v[1] + m[8] * v[2], m[1] * v[0] + m[5] * v[1] + m[9] * v[2], m[2] * v[0] + m[6] * v[1] + m[10] * v[2]]
}

fn matrix(v: Option<&Json>) -> Option<M4> {
    let a = v?.as_array()?;
    if a.len() != 16 {
        return None;
    }
    let mut m = [0.0; 16];
    for (i, x) in a.iter().enumerate() {
        m[i] = x.as_f64()?;
    }
    Some(m)
}

const WGS84_A: f64 = 6_378_137.0;
const WGS84_F: f64 = 1.0 / 298.257_223_563;

/// Earth-centred coordinates from longitude and latitude in degrees and the height above
/// the ellipsoid.
pub fn to_ecef(lon: f64, lat: f64, h: f64) -> [f64; 3] {
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let (sl, cl) = lon.to_radians().sin_cos();
    let (sp, cp) = lat.to_radians().sin_cos();
    let n = WGS84_A / (1.0 - e2 * sp * sp).sqrt();
    [(n + h) * cp * cl, (n + h) * cp * sl, (n * (1.0 - e2) + h) * sp]
}

/// Longitude and latitude in degrees and the height above the ellipsoid, from earth-centred
/// coordinates. Bowring's method, iterated: well below a millimetre on and above the earth.
pub fn from_ecef(p: [f64; 3]) -> (f64, f64, f64) {
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let b = WGS84_A * (1.0 - WGS84_F);
    let ep2 = (WGS84_A * WGS84_A - b * b) / (b * b);
    let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
    let lon = p[1].atan2(p[0]);
    let mut theta = (p[2] * WGS84_A).atan2(r * b);
    let mut lat = 0.0;
    for _ in 0..3 {
        let (st, ct) = theta.sin_cos();
        lat = (p[2] + ep2 * b * st * st * st).atan2(r - e2 * WGS84_A * ct * ct * ct);
        theta = ((1.0 - WGS84_F) * lat.tan()).atan();
    }
    let (sp, cp) = lat.sin_cos();
    let n = WGS84_A / (1.0 - e2 * sp * sp).sqrt();
    let h = if cp.abs() > 1e-10 { r / cp - n } else { p[2].abs() / sp.abs() - n * (1.0 - e2) };
    (lon.to_degrees(), lat.to_degrees(), h)
}

/// Whether a point is on or near the surface of the earth, as tiles that are placed on it
/// are; local coordinates of a model are not.
fn on_earth(p: [f64; 3]) -> bool {
    let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    (6_300_000.0..6_450_000.0).contains(&r)
}

/// East, north and up at a place, as unit vectors in earth-centred coordinates.
fn enu(lon: f64, lat: f64) -> [[f64; 3]; 3] {
    let (sl, cl) = lon.to_radians().sin_cos();
    let (sp, cp) = lat.to_radians().sin_cos();
    [[-sl, cl, 0.0], [-sp * cl, -sp * sl, cp], [cp * cl, cp * sl, sp]]
}

/// The UTM zone of a place, as an EPSG code of WGS 84.
pub fn utm_zone(lon: f64, lat: f64) -> u32 {
    let zone = (((lon + 180.0) / 6.0).floor() as i64).clamp(0, 59) as u32 + 1;
    if lat >= 0.0 {
        32600 + zone
    } else {
        32700 + zone
    }
}

// ---------- Where files are ----------

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Undoes the %-escapes of an address, for a file on this computer.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The address or path of `rel`, as it is written in the tileset at `base`. On the web, the
/// query of the tileset (such as a key) goes along to what it names, unless that has its own.
pub fn join(base: &str, rel: &str) -> String {
    if is_url(rel) {
        return rel.to_string();
    }
    if is_url(base) {
        let (path, query) = match base.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (base, None),
        };
        let joined = if let Some(abs) = rel.strip_prefix('/') {
            let host_end = path.find("://").map(|k| k + 3).and_then(|k| path[k..].find('/').map(|e| k + e)).unwrap_or(path.len());
            format!("{}/{}", &path[..host_end], abs)
        } else {
            let dir = &path[..path.rfind('/').map_or(path.len(), |k| k + 1)];
            format!("{}{}", dir, rel)
        };
        // ./ and ../ are worked out, after the host.
        let start = joined.find("://").map(|k| k + 3).and_then(|k| joined[k..].find('/').map(|e| k + e)).unwrap_or(joined.len());
        let (host, rest) = joined.split_at(start);
        let (rest, own_query) = match rest.split_once('?') {
            Some((r, q)) => (r.to_string(), Some(q.to_string())),
            None => (rest.to_string(), None),
        };
        let mut parts: Vec<&str> = Vec::new();
        for seg in rest.split('/') {
            match seg {
                "." => {}
                ".." => {
                    if parts.len() > 1 {
                        parts.pop();
                    }
                }
                s => parts.push(s),
            }
        }
        let mut out = format!("{}{}", host, parts.join("/"));
        match (own_query, query) {
            (Some(q), _) => out = format!("{}?{}", out, q),
            (None, Some(q)) if !q.is_empty() => out = format!("{}?{}", out, q),
            _ => {}
        }
        return out;
    }
    let rel = unescape(rel.split(['?', '#']).next().unwrap_or(rel));
    let dir = std::path::Path::new(base).parent().unwrap_or(std::path::Path::new("."));
    dir.join(rel).to_string_lossy().into_owned()
}

/// The bytes at an address or path, unpacked when a server keeps them gzipped.
fn load(loc: &str, opts: &TilesOpts, backend: Option<&dyn Backend>) -> Result<Vec<u8>> {
    let bytes = if is_url(loc) {
        let b = backend.ok_or_else(|| GbError("Reading 3D Tiles from the web needs GDAL in this build.".into()))?;
        b.fetch_bytes(loc, opts.headers)?
    } else {
        std::fs::read(loc).map_err(|e| GbError(format!("Cannot read {}: {}", loc, e)))?
    };
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out).map_err(|e| GbError(format!("{} is gzipped and cannot be unpacked: {}", loc, e)))?;
        return Ok(out);
    }
    Ok(bytes)
}

fn short(loc: &str) -> String {
    let path = loc.split(['?', '#']).next().unwrap_or(loc);
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

// ---------- The tree ----------

/// A content chosen to be read: where it is, the transform it is placed by, how deep in the
/// tree and its geometric error.
struct Pick {
    loc: String,
    transform: M4,
    level: usize,
    error: f64,
}

struct Walk<'a> {
    opts: &'a TilesOpts<'a>,
    backend: Option<&'a dyn Backend>,
    going: &'a dyn Fn(u64, Option<u64>) -> bool,
    picks: Vec<Pick>,
    tilesets: usize,
    /// glTF with z up instead of y, as some early tilesets say in asset.gltfUpAxis.
    z_up: bool,
}

/// The rectangle a bounding volume covers, in degrees, or None when it cannot be told, as
/// for a tileset that is not placed on the earth.
fn bounds(bv: &Json, t: &M4) -> Option<[f64; 4]> {
    let nums = |k: &str| -> Option<Vec<f64>> { bv.get(k)?.as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect()) };
    if let Some(r) = nums("region").filter(|r| r.len() >= 4) {
        return Some([r[0].to_degrees(), r[1].to_degrees(), r[2].to_degrees(), r[3].to_degrees()]);
    }
    let points: Vec<[f64; 3]> = if let Some(b) = nums("box").filter(|b| b.len() == 12) {
        let mut pts = Vec::with_capacity(8);
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    let p = [0, 1, 2].map(|k| b[k] + sx * b[3 + k] + sy * b[6 + k] + sz * b[9 + k]);
                    pts.push(apply(t, p));
                }
            }
        }
        pts
    } else {
        let s = nums("sphere").filter(|s| s.len() == 4)?;
        let c = apply(t, [s[0], s[1], s[2]]);
        let scale = [0, 1, 2].map(|k| (t[k * 4].powi(2) + t[k * 4 + 1].powi(2) + t[k * 4 + 2].powi(2)).sqrt()).into_iter().fold(0.0, f64::max);
        let r = s[3] * scale;
        let mut pts = vec![c];
        for k in 0..3 {
            let mut a = c;
            a[k] += r;
            let mut b = c;
            b[k] -= r;
            pts.push(a);
            pts.push(b);
        }
        pts
    };
    // A volume in coordinates of its own says nothing about where on the earth it is.
    if points.iter().any(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() < 6_000_000.0) {
        return None;
    }
    let mut out = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in points {
        let (lon, lat, _) = from_ecef(p);
        out = [out[0].min(lon), out[1].min(lat), out[2].max(lon), out[3].max(lat)];
    }
    Some(out)
}

fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

/// The addresses of a tile's content: content.uri, or content.url as in early tilesets, and
/// every one of contents in 3D Tiles 1.1.
fn contents(tile: &Json) -> Vec<String> {
    let one = |c: &Json| c.get("uri").or_else(|| c.get("url")).and_then(|u| u.as_str()).map(str::to_string);
    let mut out: Vec<String> = tile.get("content").and_then(one).into_iter().collect();
    out.extend(tile.get("contents").and_then(|c| c.as_array()).into_iter().flatten().filter_map(one));
    out
}

fn is_tileset_address(loc: &str) -> bool {
    loc.split(['?', '#']).next().unwrap_or(loc).to_lowercase().ends_with(".json")
}

impl Walk<'_> {
    fn visit(&mut self, tile: &Json, base: &str, parent: &M4, parent_refine: &str, level: usize) -> Result<()> {
        if level > 64 {
            return Ok(());
        }
        if !(self.going)(self.picks.len() as u64, None) {
            return err(crate::model::STOPPED);
        }
        let t = match matrix(tile.get("transform")) {
            Some(m) => mul(parent, &m),
            None => *parent,
        };
        if let Some(imp) = tile.get("implicitTiling") {
            let tree = Implicit::new(tile, imp, base, t, parent_refine)?;
            let mut subtrees = HashMap::new();
            return self.visit_implicit(&tree, &mut subtrees, (0, 0, 0, 0), level);
        }
        if let (Some(w), Some(bv)) = (self.opts.within, tile.get("boundingVolume")) {
            if let Some(b) = bounds(bv, &t) {
                if !overlaps(&w, &b) {
                    return Ok(());
                }
            }
        }
        let refine = tile.get("refine").and_then(|r| r.as_str()).map(|r| r.to_uppercase()).unwrap_or_else(|| parent_refine.to_string());
        let error = tile.get("geometricError").and_then(|e| e.as_f64()).unwrap_or(0.0);
        let found = contents(tile);
        let children: Vec<&Json> = tile.get("children").and_then(|c| c.as_array()).map(|c| c.iter().collect()).unwrap_or_default();
        let leaf = children.is_empty();
        let add = refine == "ADD";
        let (here, down) = match self.opts.detail {
            Detail::Finest => (leaf || add, !leaf),
            Detail::Coarsest => (!found.is_empty(), found.is_empty()),
            Detail::Error(e) => {
                if error <= e || leaf {
                    (true, false)
                } else {
                    (add, true)
                }
            }
        };
        for c in &found {
            let loc = join(base, c);
            if is_tileset_address(&loc) {
                // Another tileset: a branch of this tree, read when this tile or what is
                // under it is wanted.
                if here || down {
                    self.tilesets += 1;
                    if self.tilesets > 10_000 {
                        return err("The tileset leads to more than 10 000 other tilesets. Choose a coarser level of detail or a smaller rectangle.");
                    }
                    let set = read_json(&load(&loc, self.opts, self.backend)?, &loc)?;
                    let root = set.get("root").ok_or_else(|| GbError(format!("{} has no root tile.", short(&loc))))?;
                    self.visit(root, &loc, &t, &refine, level)?;
                }
            } else if here {
                self.pick(Pick { loc, transform: t, level, error })?;
            }
        }
        if down {
            for child in children {
                self.visit(child, base, &t, &refine, level + 1)?;
            }
        }
        Ok(())
    }
}

impl Walk<'_> {
    /// A content to read, or an error when there are more than may be read.
    fn pick(&mut self, pick: Pick) -> Result<()> {
        self.picks.push(pick);
        if self.picks.len() > self.opts.most {
            // Some tilesets, such as 3DBAG, have content only in the smallest tiles. A
            // coarser level of detail then reads just as many.
            let only_finest = self.picks.iter().all(|p| p.error == 0.0);
            let rectangle = if self.opts.within.is_some() { "a smaller rectangle" } else { "read within a rectangle" };
            return err(if only_finest && self.opts.detail != Detail::Finest {
                format!(
                    "More than {} tiles would be read. The tileset has content only in its smallest tiles, so a coarser level of detail does not help: {}, or allow more tiles.",
                    fmt_count(self.opts.most as u64),
                    rectangle
                )
            } else {
                format!(
                    "More than {} tiles would be read. Choose a coarser level of detail, a larger error, {}, or allow more tiles.",
                    fmt_count(self.opts.most as u64),
                    rectangle
                )
            });
        }
        Ok(())
    }

    /// A tile of an implicit tree, at (level, x, y, z), and what is under it.
    fn visit_implicit(
        &mut self,
        tree: &Implicit,
        subtrees: &mut HashMap<(u32, u64, u64, u64), Option<Subtree>>,
        at: (u32, u64, u64, u64),
        depth: usize,
    ) -> Result<()> {
        if !(self.going)(self.picks.len() as u64, None) {
            return err(crate::model::STOPPED);
        }
        let Some((tile_here, content_here)) = tree.available(at, subtrees, self)? else { return Ok(()) };
        if !tile_here {
            return Ok(());
        }
        let bv = tree.volume(at);
        if let Some(w) = self.opts.within {
            if let Some(b) = bounds(&bv, &tree.transform) {
                if !overlaps(&w, &b) {
                    return Ok(());
                }
            }
        }
        let error = tree.error / 2f64.powi(at.0 as i32);
        let children: Vec<(u32, u64, u64, u64)> = if at.0 + 1 >= tree.levels {
            Vec::new()
        } else {
            let mut kids = Vec::new();
            for dz in 0..if tree.octree { 2 } else { 1 } {
                for dy in 0..2 {
                    for dx in 0..2 {
                        let kid = (at.0 + 1, at.1 * 2 + dx, at.2 * 2 + dy, at.3 * 2 + dz);
                        if tree.available(kid, subtrees, self)?.is_some_and(|a| a.0) {
                            kids.push(kid);
                        }
                    }
                }
            }
            kids
        };
        let leaf = children.is_empty();
        let add = tree.refine == "ADD";
        let (here, down) = match self.opts.detail {
            Detail::Finest => (leaf || add, !leaf),
            Detail::Coarsest => (content_here, !content_here),
            Detail::Error(e) => {
                if error <= e || leaf {
                    (true, false)
                } else {
                    (add, true)
                }
            }
        };
        if here && content_here {
            let loc = join(&tree.base, &tree.address(&tree.content, at));
            self.pick(Pick { loc, transform: tree.transform, level: depth + at.0 as usize, error: if leaf { 0.0 } else { error } })?;
        }
        if down {
            for kid in children {
                self.visit_implicit(tree, subtrees, kid, depth)?;
            }
        }
        Ok(())
    }
}

// ---------- Implicit tiling ----------

/// A tree of tiles that tileset.json does not list, only describes: each level halves the
/// one above, and subtree files say which tiles there are.
struct Implicit {
    octree: bool,
    /// Levels in a subtree file, and levels in all.
    subtree_levels: u32,
    levels: u32,
    content: String,
    subtrees: String,
    /// The address the templates are relative to.
    base: String,
    root_volume: Json,
    error: f64,
    refine: String,
    transform: M4,
}

/// What a subtree file says: which tiles there are, which have content, and which subtrees
/// go on below it, each a list of bits or the same for all.
struct Subtree {
    tiles: Bits,
    content: Bits,
    children: Bits,
}

enum Bits {
    All(bool),
    List(Vec<u8>),
}

impl Bits {
    fn get(&self, i: usize) -> bool {
        match self {
            Bits::All(b) => *b,
            Bits::List(v) => v.get(i / 8).is_some_and(|b| b >> (i % 8) & 1 == 1),
        }
    }
}

/// x, y and z interleaved, as the bits of a subtree are ordered.
fn morton(x: u64, y: u64, z: u64, octree: bool) -> u64 {
    let mut out = 0u64;
    for b in 0..21 {
        if octree {
            out |= ((x >> b) & 1) << (3 * b) | ((y >> b) & 1) << (3 * b + 1) | ((z >> b) & 1) << (3 * b + 2);
        } else {
            out |= ((x >> b) & 1) << (2 * b) | ((y >> b) & 1) << (2 * b + 1);
        }
    }
    out
}

impl Implicit {
    fn new(tile: &Json, imp: &Json, base: &str, transform: M4, parent_refine: &str) -> Result<Implicit> {
        let scheme = imp.get("subdivisionScheme").and_then(|v| v.as_str()).unwrap_or("QUADTREE");
        let num = |k: &str| imp.get(k).and_then(|v| v.as_u64()).map(|v| v as u32);
        let (Some(subtree_levels), Some(levels)) = (num("subtreeLevels"), num("availableLevels").or_else(|| num("maximumLevel").map(|m| m + 1))) else {
            return err("The implicit tiling of the tileset gives no subtreeLevels or availableLevels.");
        };
        let content = contents(tile).into_iter().next().unwrap_or_default();
        let subtrees = imp.get("subtrees").and_then(|s| s.get("uri").or_else(|| s.get("url"))).and_then(|u| u.as_str()).unwrap_or_default().to_string();
        if subtrees.is_empty() {
            return err("The implicit tiling of the tileset says nothing about where its subtrees are.");
        }
        Ok(Implicit {
            octree: scheme.eq_ignore_ascii_case("OCTREE"),
            subtree_levels: subtree_levels.max(1),
            levels: levels.min(40),
            content,
            subtrees,
            base: base.to_string(),
            root_volume: tile.get("boundingVolume").cloned().unwrap_or(Json::Null),
            error: tile.get("geometricError").and_then(|e| e.as_f64()).unwrap_or(0.0),
            refine: tile.get("refine").and_then(|r| r.as_str()).map(|r| r.to_uppercase()).unwrap_or_else(|| parent_refine.to_string()),
            transform,
        })
    }

    /// A template with {level}, {x}, {y} and {z} put in.
    fn address(&self, template: &str, at: (u32, u64, u64, u64)) -> String {
        template.replace("{level}", &at.0.to_string()).replace("{x}", &at.1.to_string()).replace("{y}", &at.2.to_string()).replace("{z}", &at.3.to_string())
    }

    /// The bounding volume of a tile: the root's, divided.
    fn volume(&self, at: (u32, u64, u64, u64)) -> Json {
        let (level, x, y, z) = at;
        let n = 2f64.powi(level as i32);
        let nums = |k: &str| -> Option<Vec<f64>> { self.root_volume.get(k)?.as_array().map(|a| a.iter().filter_map(|v| v.as_f64()).collect()) };
        if let Some(r) = nums("region").filter(|r| r.len() == 6) {
            let (w, e) = (r[0] + (r[2] - r[0]) * x as f64 / n, r[0] + (r[2] - r[0]) * (x + 1) as f64 / n);
            let (s, nn) = (r[1] + (r[3] - r[1]) * y as f64 / n, r[1] + (r[3] - r[1]) * (y + 1) as f64 / n);
            let (lo, hi) = if self.octree { (r[4] + (r[5] - r[4]) * z as f64 / n, r[4] + (r[5] - r[4]) * (z + 1) as f64 / n) } else { (r[4], r[5]) };
            return serde_json::json!({ "region": [w, s, e, nn, lo, hi] });
        }
        if let Some(b) = nums("box").filter(|b| b.len() == 12) {
            let f = |i: u64| (2.0 * i as f64 + 1.0) / n - 1.0;
            let (fx, fy, fz) = (f(x), f(y), if self.octree { f(z) } else { 0.0 });
            let mut out = vec![0.0; 12];
            for k in 0..3 {
                out[k] = b[k] + fx * b[3 + k] + fy * b[6 + k] + fz * b[9 + k];
                out[3 + k] = b[3 + k] / n;
                out[6 + k] = b[6 + k] / n;
                out[9 + k] = if self.octree { b[9 + k] / n } else { b[9 + k] };
            }
            return serde_json::json!({ "box": out });
        }
        Json::Null
    }

    /// Whether the tile at `at` is there and has content, from the subtree it is in, read
    /// when first needed. None when its subtree is not there.
    fn available(&self, at: (u32, u64, u64, u64), subtrees: &mut HashMap<(u32, u64, u64, u64), Option<Subtree>>, walk: &Walk) -> Result<Option<(bool, bool)>> {
        let (level, x, y, z) = at;
        let rel = level % self.subtree_levels;
        let root = (level - rel, x >> rel, y >> rel, z >> rel);
        if !subtrees.contains_key(&root) {
            // A subtree below the first is there only if the one above says so.
            let there = if root.0 == 0 {
                true
            } else {
                let parent = self.available_subtree(root, subtrees, walk)?;
                parent.unwrap_or(false)
            };
            let tree = if there { Some(self.read_subtree(root, walk)?) } else { None };
            subtrees.insert(root, tree);
        }
        let Some(Some(tree)) = subtrees.get(&root) else { return Ok(None) };
        let per = if self.octree { 8u64 } else { 4 };
        let offset = (per.pow(rel) - 1) / (per - 1);
        let mask = (1u64 << rel) - 1;
        let i = (offset + morton(x & mask, y & mask, z & mask, self.octree)) as usize;
        Ok(Some((tree.tiles.get(i), tree.content.get(i))))
    }

    /// Whether the subtree whose root is `root` is there, from the subtree above it.
    fn available_subtree(
        &self,
        root: (u32, u64, u64, u64),
        subtrees: &mut HashMap<(u32, u64, u64, u64), Option<Subtree>>,
        walk: &Walk,
    ) -> Result<Option<bool>> {
        let up = self.subtree_levels;
        let parent = (root.0 - up, root.1 >> up, root.2 >> up, root.3 >> up);
        // The parent's own availability makes sure it is read.
        let above = (parent.0, parent.1, parent.2, parent.3);
        if self.available(above, subtrees, walk)?.is_none() {
            return Ok(Some(false));
        }
        let Some(Some(tree)) = subtrees.get(&parent) else { return Ok(Some(false)) };
        let mask = (1u64 << up) - 1;
        Ok(Some(tree.children.get(morton(root.1 & mask, root.2 & mask, root.3 & mask, self.octree) as usize)))
    }

    fn read_subtree(&self, root: (u32, u64, u64, u64), walk: &Walk) -> Result<Subtree> {
        let loc = join(&self.base, &self.address(&self.subtrees, root));
        let bytes = load(&loc, walk.opts, walk.backend)?;
        let (json, internal): (Json, Vec<u8>) = if bytes.starts_with(b"subt") {
            let word = |at: usize| bytes.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")) as usize);
            let (Some(jl), Some(bl)) = (word(8), word(16)) else { return err(format!("{} is cut off.", short(&loc))) };
            let j = bytes.get(24..24 + jl).ok_or_else(|| GbError(format!("{} is cut off.", short(&loc))))?;
            let b = bytes.get(24 + jl..24 + jl + bl).unwrap_or(&[]).to_vec();
            (serde_json::from_slice(j).map_err(|e| GbError(format!("{} cannot be read: {}", short(&loc), e)))?, b)
        } else {
            (serde_json::from_slice(&bytes).map_err(|e| GbError(format!("{} cannot be read: {}", short(&loc), e)))?, Vec::new())
        };
        let mut buffers: Vec<Vec<u8>> = Vec::new();
        for b in json.get("buffers").and_then(|b| b.as_array()).into_iter().flatten() {
            buffers.push(match b.get("uri").and_then(|u| u.as_str()) {
                Some(uri) => load(&join(&loc, uri), walk.opts, walk.backend)?,
                None => internal.clone(),
            });
        }
        let bits = |a: Option<&Json>| -> Bits {
            let Some(a) = a else { return Bits::All(false) };
            if let Some(c) = a.get("constant").and_then(|c| c.as_u64()) {
                return Bits::All(c == 1);
            }
            let view =
                a.get("bitstream").or_else(|| a.get("bufferView")).and_then(|v| v.as_u64()).and_then(|v| json.get("bufferViews")?.as_array()?.get(v as usize));
            let Some(view) = view else { return Bits::All(false) };
            let buf = view.get("buffer").and_then(|b| b.as_u64()).unwrap_or(0) as usize;
            let off = view.get("byteOffset").and_then(|b| b.as_u64()).unwrap_or(0) as usize;
            let len = view.get("byteLength").and_then(|b| b.as_u64()).unwrap_or(0) as usize;
            Bits::List(buffers.get(buf).and_then(|b| b.get(off..off + len)).map(|b| b.to_vec()).unwrap_or_default())
        };
        let content = json.get("contentAvailability").map(|c| if c.is_array() { c.get(0) } else { Some(c) }).unwrap_or(None);
        Ok(Subtree { tiles: bits(json.get("tileAvailability")), content: bits(content), children: bits(json.get("childSubtreeAvailability")) })
    }
}

fn read_json(bytes: &[u8], loc: &str) -> Result<Json> {
    serde_json::from_slice(bytes).map_err(|e| GbError(format!("{} cannot be read as a tileset: {}", short(loc), e)))
}

// ---------- The content of a tile ----------

/// A model from a tile, with the centre its positions are relative to and the table of its
/// features (the batch table of .b3dm).
struct Piece {
    model: gltf_io::Model,
    /// The number of features the tile says it has.
    features: usize,
    rtc: [f64; 3],
    batch_table: Option<Json>,
}

fn word(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

/// The models in the bytes of a tile. What is not read is counted in `skipped`, by kind.
fn pieces(
    bytes: &[u8],
    loc: &str,
    fetch: &dyn Fn(&str) -> Result<Vec<u8>>,
    skipped: &mut HashMap<&'static str, usize>,
    notes: &mut Notes,
) -> Result<Vec<Piece>> {
    let magic = bytes.get(0..4).unwrap_or(&[]);
    match magic {
        b"b3dm" => {
            let (Some(ft_json), Some(ft_bin), Some(bt_json), Some(bt_bin)) = (word(bytes, 12), word(bytes, 16), word(bytes, 20), word(bytes, 24)) else {
                return err(format!("{} is cut off.", short(loc)));
            };
            if bt_json >= 570_425_344 || bt_bin >= 570_425_344 {
                return err(format!("{} is a Batched 3D Model with an old header from before 3D Tiles 1.0, which is not read.", short(loc)));
            }
            let mut at = 28usize;
            let take = |at: &mut usize, n: u32| -> Result<&[u8]> {
                let s = bytes.get(*at..*at + n as usize).ok_or_else(|| GbError(format!("{} is cut off.", short(loc))))?;
                *at += n as usize;
                Ok(s)
            };
            let ft: Json = serde_json::from_slice(take(&mut at, ft_json)?).unwrap_or(Json::Null);
            take(&mut at, ft_bin)?;
            let bt_text = take(&mut at, bt_json)?;
            let bt: Option<Json> = (!bt_text.is_empty()).then(|| serde_json::from_slice(bt_text).ok()).flatten();
            take(&mut at, bt_bin)?;
            let glb = &bytes[at..];
            let model = gltf_io::parse(glb, fetch, notes).map_err(|e| GbError(format!("{}: {}", short(loc), e.0)))?;
            let rtc = ft
                .get("RTC_CENTER")
                .and_then(|c| c.as_array())
                .and_then(|c| {
                    let v: Vec<f64> = c.iter().filter_map(|x| x.as_f64()).collect();
                    (v.len() == 3).then(|| [v[0], v[1], v[2]])
                })
                .or(model.rtc)
                .unwrap_or([0.0; 3]);
            let features = ft.get("BATCH_LENGTH").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let (features, bt) = match bt {
                Some(bt) => (features, Some(bt)),
                None => (features.max(model.feature_count), model.properties.clone()),
            };
            Ok(vec![Piece { model, features, rtc, batch_table: bt }])
        }
        b"cmpt" => {
            let count = word(bytes, 12).unwrap_or(0) as usize;
            let mut at = 16usize;
            let mut out = Vec::new();
            for _ in 0..count {
                let Some(len) = word(bytes, at + 8) else { break };
                let inner = bytes.get(at..at + len as usize).ok_or_else(|| GbError(format!("{} is cut off.", short(loc))))?;
                out.extend(pieces(inner, loc, fetch, skipped, notes)?);
                at += len as usize;
            }
            Ok(out)
        }
        b"i3dm" => {
            *skipped.entry("instanced models (.i3dm)").or_default() += 1;
            Ok(Vec::new())
        }
        b"pnts" => {
            // Read on their own, by points().
            Ok(Vec::new())
        }
        _ if magic == b"glTF" || bytes.first() == Some(&b'{') => {
            let model = gltf_io::parse(bytes, fetch, notes).map_err(|e| GbError(format!("{}: {}", short(loc), e.0)))?;
            let rtc = model.rtc.unwrap_or([0.0; 3]);
            let (features, batch_table) = (model.feature_count, model.properties.clone());
            Ok(vec![Piece { model, features, rtc, batch_table }])
        }
        _ => {
            *skipped.entry("tiles of a kind that is not known").or_default() += 1;
            Ok(Vec::new())
        }
    }
}

/// The points of a point cloud tile (.pnts): where each is, in the coordinates of the tile,
/// and its colour if it has one.
/// A point of a point cloud tile: where it is, and its colour if it has one.
type CloudPoint = ([f64; 3], Option<[u8; 3]>);

fn points(bytes: &[u8], loc: &str) -> Result<Vec<CloudPoint>> {
    let (Some(ft_json), Some(ft_bin)) = (word(bytes, 12), word(bytes, 16)) else {
        return err(format!("{} is cut off.", short(loc)));
    };
    let ft: Json = bytes.get(28..28 + ft_json as usize).and_then(|b| serde_json::from_slice(b).ok()).unwrap_or(Json::Null);
    let bin = bytes.get(28 + ft_json as usize..28 + ft_json as usize + ft_bin as usize).ok_or_else(|| GbError(format!("{} is cut off.", short(loc))))?;
    let n = ft.get("POINTS_LENGTH").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let offset_of = |k: &str| ft.get(k).and_then(|v| v.get("byteOffset")).and_then(|v| v.as_u64()).map(|v| v as usize);
    let triple = |k: &str| -> Option<[f64; 3]> {
        let v: Vec<f64> = ft.get(k)?.as_array()?.iter().filter_map(|x| x.as_f64()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    };
    let rtc = triple("RTC_CENTER").unwrap_or([0.0; 3]);
    let f32_at = |at: usize| bin.get(at..at + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64);
    let u16_at = |at: usize| bin.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as f64);
    let mut out = Vec::with_capacity(n);
    let rgb = offset_of("RGB").map(|o| (o, 3)).or_else(|| offset_of("RGBA").map(|o| (o, 4)));
    let rgb565 = offset_of("RGB565");
    for i in 0..n {
        let p = if let Some(o) = offset_of("POSITION") {
            [f32_at(o + i * 12), f32_at(o + i * 12 + 4), f32_at(o + i * 12 + 8)]
        } else if let Some(o) = offset_of("POSITION_QUANTIZED") {
            let (Some(off), Some(scale)) = (triple("QUANTIZED_VOLUME_OFFSET"), triple("QUANTIZED_VOLUME_SCALE")) else {
                return err(format!("{} has quantized positions without their volume.", short(loc)));
            };
            let q = [u16_at(o + i * 6), u16_at(o + i * 6 + 2), u16_at(o + i * 6 + 4)];
            [0, 1, 2].map(|k| q[k].map(|v| v / 65535.0 * scale[k] + off[k]))
        } else {
            return err(format!("{} has no positions.", short(loc)));
        };
        let [Some(x), Some(y), Some(z)] = p else { return err(format!("{} is cut off.", short(loc))) };
        let colour = match (rgb, rgb565) {
            (Some((o, k)), _) => bin.get(o + i * k..o + i * k + 3).map(|c| [c[0], c[1], c[2]]),
            (None, Some(o)) => u16_at(o + i * 2).map(|v| {
                let v = v as u16;
                [((v >> 11) & 31) as u8 * 255 / 31, ((v >> 5) & 63) as u8 * 255 / 63, (v & 31) as u8 * 255 / 31]
            }),
            _ => None,
        };
        out.push(([x + rtc[0], y + rtc[1], z + rtc[2]], colour));
    }
    Ok(out)
}

/// A value of the batch table for a feature, from its JSON.
fn json_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n.as_i64().map(Value::Int).unwrap_or_else(|| Value::Real(n.as_f64().unwrap_or(f64::NAN))),
        Json::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}

// ---------- Reading ----------

/// Reads the tiles of a tileset, at a path or an address, as one model with a feature for
/// each feature of each tile (or for each tile, when its features are not told apart).
pub fn read_tiles(root: &str, opts: &TilesOpts, backend: Option<&dyn Backend>, notes: &mut Notes, going: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let first = load(root, opts, backend)?;
    // A single tile can be read by itself, as if it were the whole tileset.
    let (set, single) = if first.starts_with(b"b3dm") || first.starts_with(b"cmpt") || first.starts_with(b"glTF") || first.starts_with(b"pnts") {
        (Json::Null, true)
    } else {
        (read_json(&first, root)?, false)
    };
    let mut walk = Walk { opts, backend, going, picks: Vec::new(), tilesets: 0, z_up: false };
    if single {
        walk.picks.push(Pick { loc: root.to_string(), transform: IDENTITY, level: 0, error: 0.0 });
    } else {
        let version = set.get("asset").and_then(|a| a.get("version")).and_then(|v| v.as_str()).unwrap_or("");
        if !version.starts_with("1.") && !version.starts_with("0.") {
            return err(format!("{} is not a 3D Tiles tileset: it has no asset.version 1.0 or 1.1.", short(root)));
        }
        walk.z_up = set.get("asset").and_then(|a| a.get("gltfUpAxis")).and_then(|v| v.as_str()).is_some_and(|v| v.eq_ignore_ascii_case("z"));
        let tree = set.get("root").ok_or_else(|| GbError(format!("{} has no root tile.", short(root))))?;
        walk.visit(tree, root, &IDENTITY, "REPLACE", 0)?;
    }
    let z_up = walk.z_up;
    let picks = std::mem::take(&mut walk.picks);
    if picks.is_empty() {
        return err(if opts.within.is_some() { "No tile of the tileset reaches into the rectangle." } else { "The tileset has no tiles with content." });
    }

    // The tiles, read and placed in earth-centred coordinates, each with its features.
    struct Read {
        positions: Vec<[f64; 3]>,
        normals: Vec<[f32; 3]>,
        piece: Piece,
        pick: usize,
    }
    let mut read: Vec<Read> = Vec::new();
    // The points of point cloud tiles, earth-centred, with their colour and tile.
    let mut cloud: Vec<([f64; 3], Option<[u8; 3]>, usize)> = Vec::new();
    let mut skipped: HashMap<&'static str, usize> = HashMap::new();
    let total = picks.len() as u64;
    for (k, pick) in picks.iter().enumerate() {
        if !going(k as u64, Some(total)) {
            return err(crate::model::STOPPED);
        }
        let bytes = load(&pick.loc, opts, backend)?;
        if bytes.starts_with(b"pnts") {
            for (p, c) in points(&bytes, &pick.loc)? {
                cloud.push((apply(&pick.transform, p), c, k));
            }
            continue;
        }
        let loc = pick.loc.clone();
        let fetch = move |uri: &str| -> Result<Vec<u8>> { load(&join(&loc, uri), opts, backend) };
        for piece in pieces(&bytes, &pick.loc, &fetch, &mut skipped, notes)? {
            // glTF has y up; tiles have z up.
            let up = |p: [f64; 3]| if z_up { p } else { [p[0], -p[2], p[1]] };
            let positions: Vec<[f64; 3]> = piece
                .model
                .mesh
                .positions
                .iter()
                .map(|p| {
                    let q = up(*p);
                    apply(&pick.transform, [q[0] + piece.rtc[0], q[1] + piece.rtc[1], q[2] + piece.rtc[2]])
                })
                .collect();
            let normals: Vec<[f32; 3]> = piece
                .model
                .mesh
                .normals
                .iter()
                .map(|n| {
                    let v = turn(&pick.transform, up([n[0] as f64, n[1] as f64, n[2] as f64]));
                    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
                    [(v[0] / l) as f32, (v[1] / l) as f32, (v[2] / l) as f32]
                })
                .collect();
            read.push(Read { positions, normals, piece, pick: k });
        }
    }
    for (what, n) in &skipped {
        notes.warn(format!("{} tiles are {}, which are not read yet.", fmt_count(*n as u64), what));
    }
    if read.is_empty() && !cloud.is_empty() {
        return point_table(cloud, &picks, opts, backend, notes);
    }
    if !cloud.is_empty() {
        notes.warn(format!("The tileset has models and point clouds. The models are read; the {} points are left out.", fmt_count(cloud.len() as u64)));
    }
    if read.is_empty() {
        return err("None of the tiles holds a model that can be read.");
    }

    // Placed on the earth, or in coordinates of their own?
    let probe = read.iter().flat_map(|r| r.positions.first()).copied().next().unwrap_or([0.0; 3]);
    let placed = on_earth(probe);
    let crs = if placed {
        let (lon, lat, _) = from_ecef(probe);
        Some(opts.crs.unwrap_or_else(|| {
            let zone = utm_zone(lon, lat);
            notes.info(format!("placed in WGS 84 / UTM zone {} (EPSG:{}); choose another coordinate system in the input", zone % 100, zone));
            zone
        }))
    } else {
        notes.warn("The tiles are not placed on the earth: their own coordinates are kept, with z up.".to_string());
        opts.crs
    };

    // Everything as one model, with an object for each feature of each tile.
    let mut mesh = Mesh::default();
    let mut faces: Vec<Vec<([u32; 3], u32)>> = Vec::new();
    // For each object: the tile it is in, and its feature in the tile.
    let mut whose: Vec<(usize, Option<u32>)> = Vec::new();
    for (ri, r) in read.iter_mut().enumerate() {
        let m = &r.piece.model.mesh;
        if placed && !r.positions.is_empty() {
            // Earth-centred to longitude, latitude and height, and on to the map.
            let mut lons = Vec::with_capacity(r.positions.len());
            let mut lats = Vec::with_capacity(r.positions.len());
            let mut hs = Vec::with_capacity(r.positions.len());
            for p in &r.positions {
                let (lon, lat, h) = from_ecef(*p);
                lons.push(lon);
                lats.push(lat);
                hs.push(h + opts.dz);
            }
            let centre = from_ecef(r.positions[r.positions.len() / 2]);
            if let Some(to) = crs.filter(|c| *c != 4326) {
                let b = backend.ok_or_else(|| GbError("Placing 3D Tiles on a map needs GDAL in this build.".into()))?;
                b.transform(4326, to, &mut lons, &mut lats)?;
            }
            for (k, p) in r.positions.iter_mut().enumerate() {
                *p = [lons[k], lats[k], hs[k]];
            }
            // Normals to east, north and up at the middle of the tile.
            let e = enu(centre.0, centre.1);
            for n in r.normals.iter_mut() {
                let v = [n[0] as f64, n[1] as f64, n[2] as f64];
                let d = |a: [f64; 3]| (a[0] * v[0] + a[1] * v[1] + a[2] * v[2]) as f32;
                *n = [d(e[0]), d(e[1]), d(e[2])];
            }
        }
        let (vp, vn, vt, mm) = (mesh.positions.len() as u32, mesh.normals.len() as u32, mesh.uvs.len() as u32, mesh.materials.len() as u32);
        mesh.positions.extend_from_slice(&r.positions);
        mesh.normals.extend_from_slice(&r.normals);
        mesh.uvs.extend_from_slice(&m.uvs);
        mesh.materials.extend(m.materials.iter().cloned());
        // An object for each feature of the tile, or one for the tile.
        let mut objects: HashMap<u32, u32> = HashMap::new();
        // A tile of one feature need not say which triangles are its.
        let all_one = r.piece.features == 1 && r.piece.model.batch.iter().all(|b| *b == NONE);
        let shift = |a: [u32; 3], by: u32| a.map(|i| if i == NONE { NONE } else { i + by });
        for (ti, t) in m.tris.iter().enumerate() {
            let feature = if all_one { 0 } else { r.piece.model.batch.get(ti).copied().unwrap_or(NONE) };
            let object = *objects.entry(feature).or_insert_with(|| {
                whose.push((ri, (feature != NONE).then_some(feature)));
                faces.push(Vec::new());
                (whose.len() - 1) as u32
            });
            let v = shift(t.v, vp);
            mesh.tris.push(Tri { v, n: shift(t.n, vn), t: shift(t.t, vt), material: t.material + mm, object });
            faces[object as usize].push((v, t.material + mm));
        }
    }

    // The names of the objects, and the attributes of the batch tables.
    let mut extra: Vec<String> = Vec::new();
    for r in &read {
        if let Some(Json::Object(bt)) = &r.piece.batch_table {
            for (k, v) in bt {
                if v.is_array() && !extra.contains(k) && !matches!(k.as_str(), "extensions" | "extras") {
                    extra.push(k.clone());
                }
            }
        }
    }
    let name_key = ["name", "Name", "NAME", "id", "ID"].into_iter().find(|k| extra.iter().any(|e| e == k));
    for (ri, feature) in &whose {
        let r = &read[*ri];
        let file = short(&picks[r.pick].loc);
        let named = feature.and_then(|f| {
            let bt = r.piece.batch_table.as_ref()?;
            let v = bt.get(name_key?)?.as_array()?.get(f as usize)?;
            Some(json_value(v).to_text())
        });
        mesh.objects.push(match (named, feature) {
            (Some(n), _) if !n.is_empty() => n,
            (_, Some(f)) => format!("{} {}", file, f),
            _ => file,
        });
    }
    let parts = gltf_io::corners(&mesh, &faces);
    let mut table = model_table(mesh, parts, "triangles", crs, notes);
    // Where each object comes from, and what the batch table says about it.
    table.fields.push(Field::new("tile", FieldType::Text));
    table.fields.push(Field::new("level", FieldType::Int));
    table.fields.push(Field::new("geometric_error", FieldType::Real));
    table.fields.push(Field::new("feature", FieldType::Int));
    let mut columns: Vec<Vec<Value>> = vec![Vec::with_capacity(whose.len()); extra.len()];
    for (ri, feature) in &whose {
        let r = &read[*ri];
        for (c, key) in extra.iter().enumerate() {
            let v = feature.and_then(|f| r.piece.batch_table.as_ref()?.get(key)?.as_array()?.get(f as usize).map(json_value)).unwrap_or(Value::Null);
            columns[c].push(v);
        }
    }
    for (o, (ri, feature)) in whose.iter().enumerate() {
        let pick = &picks[read[*ri].pick];
        let f = &mut table.features[o];
        f.attrs.push(Value::Text(short(&pick.loc)));
        f.attrs.push(Value::Int(pick.level as i64));
        f.attrs.push(Value::Real(pick.error));
        f.attrs.push(feature.map_or(Value::Null, |v| Value::Int(v as i64)));
    }
    for (c, key) in extra.iter().enumerate() {
        // The name is the name of the object already.
        if Some(key.as_str()) == name_key && key == "name" {
            continue;
        }
        // A name that is there already, also with other capitals, which databases do not
        // tell apart, gets _2.
        let taken = table.fields.iter().any(|f| f.name.eq_ignore_ascii_case(key));
        let name = if taken { format!("{}_2", key) } else { key.clone() };
        let ty = crate::model::infer_type(columns[c].iter());
        table.fields.push(Field::new(name, ty));
        for (o, v) in columns[c].drain(..).enumerate() {
            table.features[o].attrs.push(crate::model::coerce(v, ty));
        }
    }
    notes.info(format!("{} tiles", fmt_count(picks.len() as u64)));
    Ok(table)
}

/// The points of point cloud tiles as a stream of points, placed on the map.
fn point_table(
    cloud: Vec<([f64; 3], Option<[u8; 3]>, usize)>,
    picks: &[Pick],
    opts: &TilesOpts,
    backend: Option<&dyn Backend>,
    notes: &mut Notes,
) -> Result<Table> {
    let placed = cloud.first().is_some_and(|c| on_earth(c.0));
    let mut xs = Vec::with_capacity(cloud.len());
    let mut ys = Vec::with_capacity(cloud.len());
    let mut zs = Vec::with_capacity(cloud.len());
    for (p, _, _) in &cloud {
        if placed {
            let (lon, lat, h) = from_ecef(*p);
            xs.push(lon);
            ys.push(lat);
            zs.push(h + opts.dz);
        } else {
            xs.push(p[0]);
            ys.push(p[1]);
            zs.push(p[2] + opts.dz);
        }
    }
    let crs = if placed {
        let to = opts.crs.unwrap_or_else(|| {
            let zone = utm_zone(xs[0], ys[0]);
            notes.info(format!("placed in WGS 84 / UTM zone {} (EPSG:{}); choose another coordinate system in the input", zone % 100, zone));
            zone
        });
        if to != 4326 {
            let b = backend.ok_or_else(|| GbError("Placing 3D Tiles on a map needs GDAL in this build.".into()))?;
            b.transform(4326, to, &mut xs, &mut ys)?;
        }
        Some(to)
    } else {
        notes.warn("The tiles are not placed on the earth: their own coordinates are kept, with z up.".to_string());
        opts.crs
    };
    let colour = cloud.iter().any(|c| c.1.is_some());
    let mut fields = vec![Field::new("tile", FieldType::Text), Field::new("level", FieldType::Int)];
    if colour {
        for c in ["red", "green", "blue"] {
            fields.push(Field::new(c, FieldType::Int));
        }
    }
    let features = cloud
        .iter()
        .enumerate()
        .map(|(i, (_, c, k))| {
            let mut attrs = vec![Value::Text(short(&picks[*k].loc)), Value::Int(picks[*k].level as i64)];
            if colour {
                attrs.extend(c.unwrap_or([0; 3]).map(|v| Value::Int(v as i64)));
            }
            crate::model::Feature { z: Some(std::sync::Arc::from(vec![zs[i]])), attrs, geom: Some(geo::Geometry::Point(geo::Point::new(xs[i], ys[i]))) }
        })
        .collect();
    notes.info(format!("{} points of {} tiles", fmt_count(cloud.len() as u64), fmt_count(picks.len() as u64)));
    Ok(Table { mesh: None, fields, crs, features, raster: false, image: None, grid: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earth_centred_coordinates_go_there_and_back() {
        for (lon, lat, h) in [(11.97, 57.71, 45.0), (-70.0, -33.4, 2500.0), (0.0, 89.9, 0.0), (179.0, 0.0, -30.0)] {
            let (a, b, c) = from_ecef(to_ecef(lon, lat, h));
            assert!((a - lon).abs() < 1e-9 && (b - lat).abs() < 1e-9 && (c - h).abs() < 1e-4, "{a} {b} {c}");
        }
        assert_eq!(utm_zone(11.97, 57.71), 32632);
        assert_eq!(utm_zone(18.07, 59.33), 32634);
        assert_eq!(utm_zone(-70.0, -33.4), 32719);
    }

    #[test]
    fn addresses_are_joined_as_tilesets_write_them() {
        assert_eq!(join("https://a.org/x/tileset.json?key=1", "t/0.b3dm"), "https://a.org/x/t/0.b3dm?key=1");
        assert_eq!(join("https://a.org/x/y/tileset.json", "../z/1.glb?s=2"), "https://a.org/x/z/1.glb?s=2");
        assert_eq!(join("https://a.org/x/tileset.json", "/root/2.glb"), "https://a.org/root/2.glb");
        assert_eq!(join("https://a.org/x/tileset.json", "https://b.org/3.glb"), "https://b.org/3.glb");
        let local = join("/data/city/tileset.json", "tiles/a%20b.b3dm");
        assert_eq!(std::path::Path::new(&local), std::path::Path::new("/data/city/tiles/a b.b3dm"));
    }
}
