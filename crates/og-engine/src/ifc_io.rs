//! Reads IFC files (IFC2x3, IFC4 and IFC4.3, as STEP text): the building elements as 3D
//! models with their attributes, property sets, quantities, storey, type and material, for
//! processing and for pictures of buildings.
//!
//! The geometry that most files use is read: extruded profiles (rectangles, circles, I, L, U
//! and T sections, polylines and arcs, with holes), faceted boundary representations, face
//! sets and surface models, triangulated and polygonal face sets of IFC4, mapped items
//! (types placed many times), boolean results by their first part, cut by a plane where
//! that is the second. Openings are cut out of walls and slabs where they are boxes or
//! other convex shapes. Curved surfaces (NURBS, swept disks) and general booleans are not.
//!
//! Where the model lies is read from IfcMapConversion (IFC4) or the property set
//! ePSet_MapConversion (IFC2x3), in the coordinate system of IfcProjectedCRS.

use crate::io::Notes;
use crate::mesh::{Material, Mesh, Tri, NONE};
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// A colour as sRGB from 0 to 1, and how opaque it is.
type Colour = ([f32; 3], f32);

/// The triangles of a product, in sets that each have their colour.
type Parts = Vec<(Option<Colour>, Part)>;

// ---------- STEP ----------

/// A value in a STEP record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum V {
    Null,
    Derived,
    Ref(u64),
    Int(i64),
    Real(f64),
    Str(String),
    Enum(String),
    List(Vec<V>),
    /// A value with its type, such as IFCLABEL('Wall').
    Typed(String, Box<V>),
}

impl V {
    fn num(&self) -> Option<f64> {
        match self {
            V::Int(i) => Some(*i as f64),
            V::Real(r) => Some(*r),
            V::Typed(_, v) => v.num(),
            _ => None,
        }
    }
    fn r(&self) -> Option<u64> {
        match self {
            V::Ref(r) => Some(*r),
            _ => None,
        }
    }
    fn list(&self) -> &[V] {
        match self {
            V::List(l) => l,
            _ => &[],
        }
    }
    fn text(&self) -> Option<String> {
        match self {
            V::Str(s) => Some(s.clone()),
            V::Typed(_, v) => v.text(),
            V::Enum(e) => Some(e.clone()),
            _ => None,
        }
    }
    fn refs(&self) -> Vec<u64> {
        self.list().iter().filter_map(|v| v.r()).collect()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Entity {
    pub ty: String,
    pub args: Vec<V>,
}

/// The records of the DATA section, by their number.
pub(crate) struct Step {
    pub ents: HashMap<u64, Entity>,
    pub schema: String,
}

impl Step {
    fn get(&self, id: u64) -> Option<&Entity> {
        self.ents.get(&id)
    }
    fn arg(&self, id: u64, k: usize) -> Option<&V> {
        self.ents.get(&id).and_then(|e| e.args.get(k))
    }
    fn ty(&self, id: u64) -> &str {
        self.ents.get(&id).map_or("", |e| e.ty.as_str())
    }
}

/// Text in a STEP file: '' is one ', and \X2\…\X0\, \X\hh and \S\c are letters beyond ASCII.
fn decode(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    if !s.contains('\\') {
        return s.into_owned();
    }
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\' && i + 3 < b.len() && b[i + 1] == 'X' && b[i + 2] == '2' && b[i + 3] == '\\' {
            i += 4;
            let mut hex = String::new();
            while i < b.len() && b[i] != '\\' {
                hex.push(b[i]);
                i += 1;
            }
            let units: Vec<u16> = hex.as_bytes().chunks(4).filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect();
            out.push_str(&String::from_utf16_lossy(&units));
            // \X0\
            i += 4;
        } else if b[i] == '\\' && i + 4 < b.len() && b[i + 1] == 'X' && b[i + 2] == '\\' {
            let hex: String = b[i + 3..i + 5].iter().collect();
            if let Ok(v) = u8::from_str_radix(&hex, 16) {
                out.push(v as char);
            }
            i += 5;
        } else if b[i] == '\\' && i + 3 < b.len() && b[i + 1] == 'S' && b[i + 2] == '\\' {
            out.push(char::from_u32(b[i + 3] as u32 + 128).unwrap_or('?'));
            i += 4;
        } else if b[i] == '\\' && i + 3 < b.len() && b[i + 1] == 'P' && b[i + 3] == '\\' {
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn skip(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"/*") {
                match self.b[self.i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(p) => self.i += p + 4,
                    None => self.i = self.b.len(),
                }
            } else {
                return;
            }
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.b.get(self.i).copied()
    }
    fn word(&mut self) -> String {
        let start = self.i;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.b[start..self.i]).to_ascii_uppercase()
    }
    fn value(&mut self) -> Option<V> {
        match self.peek()? {
            b'$' => {
                self.i += 1;
                Some(V::Null)
            }
            b'*' => {
                self.i += 1;
                Some(V::Derived)
            }
            b'#' => {
                self.i += 1;
                let start = self.i;
                while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[start..self.i]).ok()?.parse().ok().map(V::Ref)
            }
            b'\'' => {
                self.i += 1;
                let mut raw = Vec::new();
                while self.i < self.b.len() {
                    let c = self.b[self.i];
                    if c == b'\'' {
                        if self.b.get(self.i + 1) == Some(&b'\'') {
                            raw.push(b'\'');
                            self.i += 2;
                            continue;
                        }
                        self.i += 1;
                        break;
                    }
                    raw.push(c);
                    self.i += 1;
                }
                Some(V::Str(decode(&raw)))
            }
            b'"' => {
                // Binary: kept as text.
                self.i += 1;
                let start = self.i;
                while self.i < self.b.len() && self.b[self.i] != b'"' {
                    self.i += 1;
                }
                let s = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
                self.i += 1;
                Some(V::Str(s))
            }
            b'.' => {
                self.i += 1;
                let w = self.word();
                if self.b.get(self.i) == Some(&b'.') {
                    self.i += 1;
                }
                Some(V::Enum(w))
            }
            b'(' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    match self.peek()? {
                        b')' => {
                            self.i += 1;
                            break;
                        }
                        b',' => self.i += 1,
                        _ => items.push(self.value()?),
                    }
                }
                Some(V::List(items))
            }
            c if c == b'-' || c == b'+' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                let mut real = false;
                while self.i < self.b.len() {
                    let c = self.b[self.i];
                    if c.is_ascii_digit() {
                        self.i += 1;
                    } else if c == b'.' || c == b'E' || c == b'e' || ((c == b'-' || c == b'+') && matches!(self.b[self.i - 1], b'E' | b'e')) {
                        real = true;
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                let t = std::str::from_utf8(&self.b[start..self.i]).ok()?;
                if real {
                    t.parse::<f64>().ok().map(V::Real)
                } else {
                    t.parse::<i64>().ok().map(V::Int).or_else(|| t.parse::<f64>().ok().map(V::Real))
                }
            }
            c if c.is_ascii_alphabetic() => {
                let w = self.word();
                if self.peek() == Some(b'(') {
                    self.i += 1;
                    let inner = if self.peek() == Some(b')') { V::Null } else { self.value()? };
                    // A typed value has one value; skip to its end.
                    while self.peek().is_some_and(|c| c != b')') {
                        self.i += 1;
                    }
                    self.i += 1;
                    Some(V::Typed(w, Box::new(inner)))
                } else {
                    Some(V::Enum(w))
                }
            }
            _ => None,
        }
    }
}

/// Reads the records of a STEP file.
pub(crate) fn parse(bytes: &[u8]) -> Result<Step> {
    let head = &bytes[..bytes.len().min(4000)];
    if !String::from_utf8_lossy(head).contains("ISO-10303-21") {
        return err("This is not an IFC file in STEP form (ISO-10303-21). IFC as XML or zip is not read: save it as .ifc.");
    }
    let schema = {
        let h = String::from_utf8_lossy(head).to_uppercase();
        h.find("FILE_SCHEMA").and_then(|i| h[i..].split('\'').nth(1).map(str::to_string)).unwrap_or_default()
    };
    let data = match bytes.windows(5).position(|w| w == b"DATA;") {
        Some(p) => p + 5,
        None => return err("The IFC file has no DATA section."),
    };
    let mut p = Parser { b: bytes, i: data };
    let mut ents = HashMap::new();
    // Records until ENDSEC; and what comes after.
    while p.peek() == Some(b'#') {
        p.i += 1;
        let start = p.i;
        while p.i < bytes.len() && bytes[p.i].is_ascii_digit() {
            p.i += 1;
        }
        let id: u64 = std::str::from_utf8(&bytes[start..p.i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0);
        if p.peek() != Some(b'=') {
            return err(format!("The IFC file cannot be read at record #{}.", id));
        }
        p.i += 1;
        p.skip();
        let ty = p.word();
        let args = match p.value() {
            Some(V::List(a)) => a,
            _ => return err(format!("The IFC file cannot be read at record #{} ({}).", id, ty)),
        };
        while p.i < bytes.len() && bytes[p.i] != b';' {
            p.i += 1;
        }
        p.i += 1;
        ents.insert(id, Entity { ty, args });
    }
    if ents.is_empty() {
        return err("The IFC file has no records.");
    }
    Ok(Step { ents, schema })
}

// ---------- Placements ----------

/// An affine transform: x, y and z axes and the origin, each a column.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tf {
    m: [[f64; 4]; 3],
}

impl Tf {
    const ID: Tf = Tf { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]] };
    fn from_axes(x: [f64; 3], y: [f64; 3], z: [f64; 3], o: [f64; 3]) -> Tf {
        Tf { m: [[x[0], y[0], z[0], o[0]], [x[1], y[1], z[1], o[1]], [x[2], y[2], z[2], o[2]]] }
    }
    fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
    }
    fn dir(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2])
    }
    /// The determinant of the axes: below zero for a mirror.
    fn det(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
    /// This after `other`: other first, then this.
    fn then(&self, outer: &Tf) -> Tf {
        let a = &outer.m;
        let b = &self.m;
        let mut m = [[0.0; 4]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c] + if c == 3 { a[r][3] } else { 0.0 };
            }
        }
        Tf { m }
    }
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-12 {
        v
    } else {
        v.map(|c| c / l)
    }
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

struct Ifc<'a> {
    s: &'a Step,
    placements: std::cell::RefCell<HashMap<u64, Tf>>,
    /// Radians in a degree of the file: 1 when angles are in radians.
    angle: f64,
    /// Colours of representation items, from IfcStyledItem: (colour as sRGB, opacity).
    styles: HashMap<u64, Colour>,
    warned: std::cell::RefCell<HashSet<String>>,
    /// How far curves and curved surfaces may lie from their straight pieces, in the units
    /// of the file: 2 mm.
    tolerance: f64,
    /// How much the item being read is scaled, as a mapped item may be: the tolerance is
    /// smaller by as much.
    scale_now: std::cell::Cell<f64>,
}

impl Ifc<'_> {
    fn point(&self, id: u64) -> [f64; 3] {
        let c = self.s.arg(id, 0).map(|v| v.list()).unwrap_or(&[]);
        let n = |k: usize| c.get(k).and_then(|v| v.num()).unwrap_or(0.0);
        [n(0), n(1), n(2)]
    }
    fn direction(&self, id: Option<u64>, default: [f64; 3]) -> [f64; 3] {
        match id {
            Some(d) => {
                let c = self.s.arg(d, 0).map(|v| v.list()).unwrap_or(&[]);
                let n = |k: usize| c.get(k).and_then(|v| v.num()).unwrap_or(0.0);
                norm([n(0), n(1), n(2)])
            }
            None => default,
        }
    }
    fn axis3(&self, id: u64) -> Tf {
        let Some(e) = self.s.get(id) else { return Tf::ID };
        if e.ty == "IFCAXIS2PLACEMENT2D" {
            return self.axis2(id);
        }
        let o = e.args.first().and_then(|v| v.r()).map_or([0.0; 3], |p| self.point(p));
        let z = self.direction(e.args.get(1).and_then(|v| v.r()), [0.0, 0.0, 1.0]);
        let mut x = self.direction(e.args.get(2).and_then(|v| v.r()), if z[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] });
        x = norm(sub(x, z.map(|c| c * dot(x, z))));
        if dot(x, x) < 0.5 {
            x = norm(cross([0.0, 1.0, 0.0], z));
        }
        let y = cross(z, x);
        Tf::from_axes(x, y, z, o)
    }
    fn axis2(&self, id: u64) -> Tf {
        let Some(e) = self.s.get(id) else { return Tf::ID };
        let o = e.args.first().and_then(|v| v.r()).map_or([0.0; 3], |p| self.point(p));
        let x = self.direction(e.args.get(1).and_then(|v| v.r()), [1.0, 0.0, 0.0]);
        let x = norm([x[0], x[1], 0.0]);
        Tf::from_axes(x, [-x[1], x[0], 0.0], [0.0, 0.0, 1.0], [o[0], o[1], 0.0])
    }
    /// Where an object placement puts things, through the placements it is relative to.
    fn placement(&self, id: u64) -> Tf {
        if let Some(t) = self.placements.borrow().get(&id) {
            return *t;
        }
        let tf = match self.s.get(id) {
            Some(e) if e.ty == "IFCLOCALPLACEMENT" => {
                let own = e.args.get(1).and_then(|v| v.r()).map_or(Tf::ID, |a| self.axis3(a));
                match e.args.first().and_then(|v| v.r()) {
                    Some(parent) if parent != id => own.then(&self.placement(parent)),
                    _ => own,
                }
            }
            _ => Tf::ID,
        };
        self.placements.borrow_mut().insert(id, tf);
        tf
    }
    fn transform_operator(&self, id: u64) -> Tf {
        let Some(e) = self.s.get(id) else { return Tf::ID };
        let a = |k: usize| e.args.get(k).and_then(|v| v.r());
        let x1 = self.direction(a(0), [1.0, 0.0, 0.0]);
        let o = a(2).map_or([0.0; 3], |p| self.point(p));
        let scale = e.args.get(3).and_then(|v| v.num()).unwrap_or(1.0);
        let z = if e.ty.contains("3D") { self.direction(a(4), [0.0, 0.0, 1.0]) } else { [0.0, 0.0, 1.0] };
        let (s2, s3) = if e.ty.contains("NONUNIFORM") {
            (e.args.get(5).and_then(|v| v.num()).unwrap_or(scale), e.args.get(6).and_then(|v| v.num()).unwrap_or(scale))
        } else {
            (scale, scale)
        };
        let x = norm(sub(x1, z.map(|c| c * dot(x1, z))));
        let y = cross(z, x);
        Tf::from_axes(x.map(|c| c * scale), y.map(|c| c * s2), z.map(|c| c * s3), o)
    }
    fn warn_once(&self, notes: &mut Notes, what: &str) {
        if self.warned.borrow_mut().insert(what.to_string()) {
            notes.warn(format!("{} is not read, so some shapes are missing.", what));
        }
    }
}

// ---------- Profiles and curves ----------

type Ring = Vec<[f64; 2]>;

impl Ifc<'_> {
    /// The points of a curve in its plane.
    fn curve(&self, id: u64, notes: &mut Notes) -> Ring {
        let Some(e) = self.s.get(id) else { return Vec::new() };
        match e.ty.as_str() {
            "IFCPOLYLINE" => e.args.first().map(|v| v.refs()).unwrap_or_default().iter().map(|p| self.point(*p)).map(|p| [p[0], p[1]]).collect(),
            "IFCINDEXEDPOLYCURVE" => {
                let pts: Vec<[f64; 2]> = e
                    .args
                    .first()
                    .and_then(|v| v.r())
                    .and_then(|l| self.s.arg(l, 0))
                    .map(|v| {
                        v.list()
                            .iter()
                            .map(|c| {
                                let c = c.list();
                                [c.first().and_then(|x| x.num()).unwrap_or(0.0), c.get(1).and_then(|x| x.num()).unwrap_or(0.0)]
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let segs = e.args.get(1).map(|v| v.list()).unwrap_or(&[]);
                if segs.is_empty() {
                    return pts;
                }
                let at = |v: &V| v.num().map(|i| i as usize).filter(|i| *i >= 1 && *i <= pts.len()).map(|i| pts[i - 1]);
                let mut out: Ring = Vec::new();
                for seg in segs {
                    let V::Typed(kind, inner) = seg else { continue };
                    let idx: Vec<[f64; 2]> = inner.list().iter().filter_map(at).collect();
                    if kind.contains("ARC") && idx.len() == 3 {
                        out.extend(arc3(idx[0], idx[1], idx[2]));
                    } else {
                        out.extend(idx);
                    }
                }
                out.dedup();
                out
            }
            "IFCCOMPOSITECURVE" => {
                let mut out: Ring = Vec::new();
                for seg in e.args.first().map(|v| v.refs()).unwrap_or_default() {
                    let Some(se) = self.s.get(seg) else { continue };
                    let same = !matches!(se.args.get(1), Some(V::Enum(f)) if f == "F");
                    let Some(parent) = se.args.get(2).and_then(|v| v.r()) else { continue };
                    let mut pts = self.curve(parent, notes);
                    if !same {
                        pts.reverse();
                    }
                    if let (Some(last), Some(first)) = (out.last(), pts.first()) {
                        if (last[0] - first[0]).abs() < 1e-9 && (last[1] - first[1]).abs() < 1e-9 {
                            pts.remove(0);
                        }
                    }
                    out.extend(pts);
                }
                out
            }
            "IFCTRIMMEDCURVE" => self.trimmed(e, notes),
            "IFCCIRCLE" => {
                let tf = e.args.first().and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
                let r = e.args.get(1).and_then(|v| v.num()).unwrap_or(0.0);
                (0..32)
                    .map(|k| {
                        let a = k as f64 / 32.0 * std::f64::consts::TAU;
                        let p = tf.apply([r * a.cos(), r * a.sin(), 0.0]);
                        [p[0], p[1]]
                    })
                    .collect()
            }
            other => {
                self.warn_once(notes, &format!("A curve of the type {}", other));
                Vec::new()
            }
        }
    }

    fn trimmed(&self, e: &Entity, notes: &mut Notes) -> Ring {
        let Some(basis) = e.args.first().and_then(|v| v.r()) else { return Vec::new() };
        let sense = !matches!(e.args.get(3), Some(V::Enum(f)) if f == "F");
        let cartesian = |v: Option<&V>| v.map(|l| l.list()).unwrap_or(&[]).iter().find_map(|x| x.r()).map(|p| self.point(p));
        let param = |v: Option<&V>| v.map(|l| l.list()).unwrap_or(&[]).iter().find_map(|x| if let V::Typed(_, n) = x { n.num() } else { None });
        let Some(be) = self.s.get(basis) else { return Vec::new() };
        match be.ty.as_str() {
            "IFCCIRCLE" | "IFCELLIPSE" => {
                let tf = be.args.first().and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
                let (rx, ry) = (
                    be.args.get(1).and_then(|v| v.num()).unwrap_or(0.0),
                    be.args.get(if be.ty == "IFCELLIPSE" { 2 } else { 1 }).and_then(|v| v.num()).unwrap_or(0.0),
                );
                // The angles of the ends: from points, or from parameters in the unit of angles.
                let inv = |p: [f64; 3]| {
                    let o = tf.apply([0.0; 3]);
                    let (x, y) = (tf.dir([1.0, 0.0, 0.0]), tf.dir([0.0, 1.0, 0.0]));
                    let d = sub(p, o);
                    (dot(d, y) / ry.max(1e-12)).atan2(dot(d, x) / rx.max(1e-12))
                };
                let a1 = cartesian(e.args.get(1)).map(inv).or_else(|| param(e.args.get(1)).map(|t| t * self.angle));
                let a2 = cartesian(e.args.get(2)).map(inv).or_else(|| param(e.args.get(2)).map(|t| t * self.angle));
                let (Some(mut a1), Some(mut a2)) = (a1, a2) else { return Vec::new() };
                if sense {
                    while a2 <= a1 {
                        a2 += std::f64::consts::TAU;
                    }
                } else {
                    while a1 <= a2 {
                        a1 += std::f64::consts::TAU;
                    }
                }
                let n = (((a2 - a1).abs() / std::f64::consts::TAU * 32.0).ceil() as usize).max(2);
                (0..=n)
                    .map(|k| {
                        let a = a1 + (a2 - a1) * k as f64 / n as f64;
                        let p = tf.apply([rx * a.cos(), ry * a.sin(), 0.0]);
                        [p[0], p[1]]
                    })
                    .collect()
            }
            "IFCLINE" | "IFCPOLYLINE" => {
                let (Some(p1), Some(p2)) = (cartesian(e.args.get(1)), cartesian(e.args.get(2))) else { return self.curve(basis, notes) };
                let mut v = vec![[p1[0], p1[1]], [p2[0], p2[1]]];
                if !sense {
                    v.reverse();
                }
                v
            }
            other => {
                self.warn_once(notes, &format!("A trimmed curve on a {}", other));
                Vec::new()
            }
        }
    }

    /// The outer ring and the holes of a profile, in the plane of the extrusion.
    fn profile(&self, id: u64, notes: &mut Notes) -> Vec<Ring> {
        let Some(e) = self.s.get(id) else { return Vec::new() };
        let a = |k: usize| e.args.get(k).and_then(|v| v.num()).unwrap_or(0.0);
        let place = |rings: Vec<Ring>, k: usize| -> Vec<Ring> {
            let tf = e.args.get(k).and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis2(p));
            rings
                .into_iter()
                .map(|r| {
                    r.into_iter()
                        .map(|p| {
                            let q = tf.apply([p[0], p[1], 0.0]);
                            [q[0], q[1]]
                        })
                        .collect()
                })
                .collect()
        };
        let rect = |w: f64, h: f64| vec![[-w / 2.0, -h / 2.0], [w / 2.0, -h / 2.0], [w / 2.0, h / 2.0], [-w / 2.0, h / 2.0]];
        let circle = |r: f64| (0..32).map(|k| (k as f64 / 32.0 * std::f64::consts::TAU).sin_cos()).map(|(s, c)| [r * c, r * s]).collect::<Ring>();
        match e.ty.as_str() {
            "IFCRECTANGLEPROFILEDEF" | "IFCROUNDEDRECTANGLEPROFILEDEF" => place(vec![rect(a(3), a(4))], 2),
            "IFCRECTANGLEHOLLOWPROFILEDEF" => {
                let t = a(5);
                let mut inner = rect(a(3) - 2.0 * t, a(4) - 2.0 * t);
                inner.reverse();
                place(vec![rect(a(3), a(4)), inner], 2)
            }
            "IFCCIRCLEPROFILEDEF" => place(vec![circle(a(3))], 2),
            "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let mut inner = circle(a(3) - a(4));
                inner.reverse();
                place(vec![circle(a(3)), inner], 2)
            }
            "IFCELLIPSEPROFILEDEF" => {
                let (rx, ry) = (a(3), a(4));
                place(vec![(0..32).map(|k| (k as f64 / 32.0 * std::f64::consts::TAU).sin_cos()).map(|(s, c)| [rx * c, ry * s]).collect()], 2)
            }
            "IFCISHAPEPROFILEDEF" | "IFCASYMMETRICISHAPEPROFILEDEF" => {
                let (b, h, tw, tf) = (a(3), a(4), a(5), a(6));
                let (x, y, w) = (b / 2.0, h / 2.0, tw / 2.0);
                place(
                    vec![vec![
                        [-x, -y],
                        [x, -y],
                        [x, -y + tf],
                        [w, -y + tf],
                        [w, y - tf],
                        [x, y - tf],
                        [x, y],
                        [-x, y],
                        [-x, y - tf],
                        [-w, y - tf],
                        [-w, -y + tf],
                        [-x, -y + tf],
                    ]],
                    2,
                )
            }
            "IFCLSHAPEPROFILEDEF" => {
                let (d, w, t) = (a(3), if a(4) > 0.0 { a(4) } else { a(3) }, a(5));
                let (x, y) = (w / 2.0, d / 2.0);
                place(vec![vec![[-x, -y], [x, -y], [x, -y + t], [-x + t, -y + t], [-x + t, y], [-x, y]]], 2)
            }
            "IFCUSHAPEPROFILEDEF" => {
                let (d, w, tw, tf) = (a(3), a(4), a(5), a(6));
                let (x, y) = (w / 2.0, d / 2.0);
                place(vec![vec![[-x, -y], [x, -y], [x, -y + tf], [-x + tw, -y + tf], [-x + tw, y - tf], [x, y - tf], [x, y], [-x, y]]], 2)
            }
            "IFCTSHAPEPROFILEDEF" => {
                let (d, w, tw, tf) = (a(3), a(4), a(5), a(6));
                let (x, y, h) = (w / 2.0, d / 2.0, tw / 2.0);
                place(vec![vec![[-h, -y], [h, -y], [h, y - tf], [x, y - tf], [x, y], [-x, y], [-x, y - tf], [-h, y - tf]]], 2)
            }
            "IFCCSHAPEPROFILEDEF" | "IFCZSHAPEPROFILEDEF" => {
                // As a U: near enough at a distance.
                let (d, w, t) = (a(3), a(4), a(5));
                let (x, y) = (w / 2.0, d / 2.0);
                place(vec![vec![[-x, -y], [x, -y], [x, -y + t], [-x + t, -y + t], [-x + t, y - t], [x, y - t], [x, y], [-x, y]]], 2)
            }
            "IFCARBITRARYCLOSEDPROFILEDEF" => vec![self.curve(e.args.get(2).and_then(|v| v.r()).unwrap_or(0), notes)],
            "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                let mut out = vec![self.curve(e.args.get(2).and_then(|v| v.r()).unwrap_or(0), notes)];
                for h in e.args.get(3).map(|v| v.refs()).unwrap_or_default() {
                    out.push(self.curve(h, notes));
                }
                out
            }
            "IFCDERIVEDPROFILEDEF" => {
                let parent = self.profile(e.args.get(2).and_then(|v| v.r()).unwrap_or(0), notes);
                let tf = e.args.get(3).and_then(|v| v.r()).map_or(Tf::ID, |o| self.transform_operator(o));
                parent
                    .into_iter()
                    .map(|r| {
                        r.into_iter()
                            .map(|p| {
                                let q = tf.apply([p[0], p[1], 0.0]);
                                [q[0], q[1]]
                            })
                            .collect()
                    })
                    .collect()
            }
            "IFCCOMPOSITEPROFILEDEF" => e.args.get(2).map(|v| v.refs()).unwrap_or_default().into_iter().flat_map(|p| self.profile(p, notes)).collect(),
            other => {
                self.warn_once(notes, &format!("A profile of the type {}", other));
                Vec::new()
            }
        }
    }
}

/// An arc through three points, as points.
fn arc3(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Ring {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return vec![a, b, c];
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let ux = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let uy = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    let r = ((a[0] - ux).powi(2) + (a[1] - uy).powi(2)).sqrt();
    let ang = |p: [f64; 2]| (p[1] - uy).atan2(p[0] - ux);
    let (a0, a1, a2) = (ang(a), ang(b), ang(c));
    // Which way round: the way that passes the middle point.
    let tau = std::f64::consts::TAU;
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(tau);
    let (start, sweep) = if ccw(a0, a1) < ccw(a0, a2) { (a0, ccw(a0, a2)) } else { (a0, -ccw(a2, a0)) };
    let n = ((sweep.abs() / tau * 32.0).ceil() as usize).max(2);
    (0..=n)
        .map(|k| {
            let t = start + sweep * k as f64 / n as f64;
            [ux + r * t.cos(), uy + r * t.sin()]
        })
        .collect()
}

// ---------- Shapes ----------

/// Triangles of a part of a product, in the coordinates of the item, with their colour.
#[derive(Default)]
struct Part {
    tris: Vec<[[f64; 3]; 3]>,
}

/// The triangles of a polygon in 3D with holes: in the plane it lies in, cut by ear clipping.
pub(crate) fn triangulate_face(outer: &[[f64; 3]], holes: &[Vec<[f64; 3]>]) -> Vec<[[f64; 3]; 3]> {
    if outer.len() < 3 {
        return Vec::new();
    }
    if holes.is_empty() {
        return crate::mesh::triangulate(outer).into_iter().map(|t| t.map(|i| outer[i])).collect();
    }
    let mut n = [0.0f64; 3];
    for i in 0..outer.len() {
        let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let axis = (0..3).max_by(|a, b| n[*a].abs().total_cmp(&n[*b].abs())).unwrap_or(2);
    let (i0, i1) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let ring = |r: &[[f64; 3]]| geo::LineString::new(r.iter().map(|p| geo::coord! { x: p[i0], y: p[i1] }).collect());
    let poly = geo::Polygon::new(ring(outer), holes.iter().map(|h| ring(h)).collect());
    let all: Vec<[f64; 3]> = outer.iter().chain(holes.iter().flatten()).copied().collect();
    let raw = geo::TriangulateEarcut::earcut_triangles_raw(&poly);
    // Back to 3D by the corners: earcut keeps the corners, in their order, with the rings closed.
    let find = |c: [f64; 2]| all.iter().copied().find(|p| (p[i0] - c[0]).abs() < 1e-9 && (p[i1] - c[1]).abs() < 1e-9).unwrap_or([c[0], c[1], 0.0]);
    // Earcut does not keep the turn of the face: each triangle is turned as the outer ring.
    raw.triangle_indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let tri = [find(raw.vertices[t[0]]), find(raw.vertices[t[1]]), find(raw.vertices[t[2]])];
            if dot(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0])), n) < 0.0 {
                [tri[0], tri[2], tri[1]]
            } else {
                tri
            }
        })
        .collect()
}

/// A closed solid turned to face outwards: its volume, counted by its faces, is then above
/// zero. A profile drawn clockwise, or a mirror, gives one that faces in.
fn outward(mut tris: Vec<[[f64; 3]; 3]>) -> Vec<[[f64; 3]; 3]> {
    let volume: f64 = tris.iter().map(|t| dot(t[0], cross(t[1], t[2]))).sum();
    if volume < 0.0 {
        for t in tris.iter_mut() {
            t.swap(1, 2);
        }
    }
    tris
}

/// A solid extruded from a profile: the sides and both ends.
fn extrude(rings: &[Ring], dir: [f64; 3], depth: f64) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    let rings: Vec<Ring> = rings
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if r.len() > 1 && r.first() == r.last() {
                r.pop();
            }
            r
        })
        .filter(|r| r.len() >= 3)
        .collect();
    if rings.is_empty() {
        return out;
    }
    let v = dir.map(|c| c * depth);
    for r in &rings {
        for k in 0..r.len() {
            let (a, b) = (r[k], r[(k + 1) % r.len()]);
            let (a0, b0) = ([a[0], a[1], 0.0], [b[0], b[1], 0.0]);
            let (a1, b1) = ([a[0] + v[0], a[1] + v[1], v[2]], [b[0] + v[0], b[1] + v[1], v[2]]);
            out.push([a0, b0, b1]);
            out.push([a0, b1, a1]);
        }
    }
    let outer: Vec<[f64; 3]> = rings[0].iter().map(|p| [p[0], p[1], 0.0]).collect();
    let holes: Vec<Vec<[f64; 3]>> = rings[1..].iter().map(|r| r.iter().map(|p| [p[0], p[1], 0.0]).collect()).collect();
    for t in triangulate_face(&outer, &holes) {
        out.push([t[0], t[2], t[1]]);
        out.push(t.map(|p| [p[0] + v[0], p[1] + v[1], p[2] + v[2]]));
    }
    outward(out)
}

/// Keeps the parts of triangles on one side of a plane: where dot(n, p) <= d.
fn clip_by_plane(tris: Vec<[[f64; 3]; 3]>, n: [f64; 3], d: f64) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    for t in tris {
        let poly = clip_polygon(&t, n, d);
        for k in 1..poly.len().saturating_sub(1) {
            out.push([poly[0], poly[k], poly[k + 1]]);
        }
    }
    out
}

/// Sutherland–Hodgman: the part of a convex polygon where dot(n, p) <= d.
fn clip_polygon(poly: &[[f64; 3]], n: [f64; 3], d: f64) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let (da, db) = (dot(n, a) - d, dot(n, b) - d);
        if da <= 0.0 {
            out.push(a);
        }
        if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
            let t = da / (da - db);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
        }
    }
    out
}

/// Takes a convex solid out of triangles: what lies inside every one of its planes is
/// removed, and the rest is kept, cut along the planes.
fn subtract_convex(tris: Vec<[[f64; 3]; 3]>, planes: &[([f64; 3], f64)]) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    for t in tris {
        // A face that lies on a plane of the solid counts as inside it, so that an opening as
        // deep as its wall goes through. The tolerance is that of the file's units.
        let eps = 1e-6;
        // Quick: a triangle wholly outside one plane stays as it is.
        if planes.iter().any(|(n, d)| t.iter().all(|p| dot(*n, *p) > *d + eps)) {
            out.push(t);
            continue;
        }
        // Each piece outside plane i and inside the planes before it is kept.
        let mut rest: Vec<[f64; 3]> = t.to_vec();
        for (n, d) in planes {
            if rest.len() < 3 {
                break;
            }
            if rest.iter().all(|p| (dot(*n, *p) - d).abs() <= eps) {
                // On the plane: inside when it faces out as the plane does, as the face of
                // a wall where an opening as deep as the wall starts; outside when it faces
                // into the solid, as the reveal of a hole already cut there.
                let facing = cross(sub(rest[1], rest[0]), sub(rest[2], rest[0]));
                if dot(facing, *n) >= 0.0 {
                    continue;
                }
                for k in 1..rest.len().saturating_sub(1) {
                    out.push([rest[0], rest[k], rest[k + 1]]);
                }
                rest.clear();
                break;
            }
            let outside = clip_polygon(&rest, n.map(|c| -c), -d);
            for k in 1..outside.len().saturating_sub(1) {
                out.push([outside[0], outside[k], outside[k + 1]]);
            }
            rest = clip_polygon(&rest, *n, *d);
        }
    }
    out
}

/// Whether a polygon turns the same way at every corner.
fn is_convex(r: &[[f64; 2]]) -> bool {
    let n = r.len();
    let mut sign = 0.0;
    for k in 0..n {
        let (a, b, c) = (r[k], r[(k + 1) % n], r[(k + 2) % n]);
        let z = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if z.abs() < 1e-12 {
            continue;
        }
        if sign == 0.0 {
            sign = z.signum();
        } else if z.signum() != sign {
            return false;
        }
    }
    true
}

/// A convex solid made of its planes: each face is the part of its plane within the others.
fn solid_of_planes(planes: &[([f64; 3], f64)], around: &[[[f64; 3]; 3]]) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    for (i, (n, d)) in planes.iter().enumerate() {
        let mut face = plane_square(*n, *d, around);
        for (j, (m, e)) in planes.iter().enumerate() {
            if i != j {
                face = clip_by_plane(face, *m, *e);
            }
        }
        out.extend(face);
    }
    out
}

/// Takes a solid out of the parts, and closes the hole with the faces of the solid that lie
/// within them: the reveals of an opening. A side of a convex solid that stops short of a
/// face of the parts by up to `tol`, as an opening drawn as thick as its wall but a few
/// millimetres off, is taken through it.
fn cut_out(parts: &mut Parts, solid: &[[[f64; 3]; 3]], tol: f64) -> bool {
    let mut planes = planes_of(solid);
    if planes.len() < 4 {
        return false;
    }
    let before: Vec<[[f64; 3]; 3]> = parts.iter().flat_map(|(_, p)| p.tris.iter().copied()).collect();
    let convex = solid.iter().flatten().all(|p| planes.iter().all(|(n, d)| dot(*n, *p) <= d + 1e-7));
    let mut widened = false;
    if convex && tol > 0.0 {
        // Measured at the middle of the solid, along its sides, against the faces of the parts
        // near it: a wall may be turned a hair against its openings.
        let count = (solid.len() * 3) as f64;
        let mid = solid.iter().flatten().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]).map(|v| v / count);
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in solid.iter().flatten() {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] - tol);
                hi[k] = hi[k].max(p[k] + tol);
            }
        }
        let close: Vec<&[[f64; 3]; 3]> = before.iter().filter(|u| (0..3).all(|k| u.iter().any(|p| p[k] >= lo[k]) && u.iter().any(|p| p[k] <= hi[k]))).collect();
        for (n, d) in planes.iter_mut() {
            let at = dot(*n, mid);
            let reach = close
                .iter()
                .filter_map(|t| {
                    let m = cross(sub(t[1], t[0]), sub(t[2], t[0]));
                    if dot(m, m) < 1e-24 {
                        return None;
                    }
                    let m = norm(m);
                    // A face of the parts on the same side, facing the same way.
                    let c = dot(m, *n);
                    if c < 0.9999 {
                        return None;
                    }
                    // How far along n from the middle the face of the parts is from this side.
                    let gap = (dot(m, t[0]) - dot(m, mid)) / c - (*d - at);
                    // Within the tolerance on either side: the side may be turned a hair against
                    // the face, so that it is in front of it at one end and behind at the other.
                    (gap.abs() <= tol).then_some(gap)
                })
                .fold(f64::NEG_INFINITY, f64::max);
            if reach.is_finite() {
                *d += reach.max(0.0) + tol;
                widened = true;
            }
        }
    }
    let wide;
    let solid = if widened {
        wide = solid_of_planes(&planes, solid);
        &wide[..]
    } else {
        solid
    };
    for (_, p) in parts.iter_mut() {
        p.tris = if convex { subtract_convex(std::mem::take(&mut p.tris), &planes) } else { subtract_mesh(std::mem::take(&mut p.tris), solid) };
    }
    // The faces of the solid, turned to face into the hole. A closed solid with its faces
    // outwards has a volume above zero.
    let volume: f64 = solid.iter().map(|t| dot(t[0], cross(t[1], t[2]))).sum();
    let inward: Vec<[[f64; 3]; 3]> = if volume > 0.0 { solid.iter().map(|t| [t[0], t[2], t[1]]).collect() } else { solid.to_vec() };
    let reveals = faces_inside(inward, &before);
    if let Some((_, p)) = parts.first_mut() {
        p.tris.extend(reveals);
    }
    true
}

/// A triangle cut along where other triangles cross it, or lie on it: a triangulation of it
/// that has those lines as edges. The pieces face as the triangle does.
fn split_triangle(t: [[f64; 3]; 3], others: &[&[[f64; 3]; 3]]) -> Vec<[[f64; 3]; 3]> {
    use spade::Triangulation;
    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    let len = dot(n, n).sqrt();
    if len < 1e-14 {
        return vec![t];
    }
    let nn = n.map(|c| c / len);
    let d = dot(nn, t[0]);
    let ex = norm(sub(t[1], t[0]));
    let ey = cross(nn, ex);
    let to2 = |p: [f64; 3]| {
        let q = sub(p, t[0]);
        [dot(q, ex), dot(q, ey)]
    };
    let size = [dot(sub(t[1], t[0]), sub(t[1], t[0])), dot(sub(t[2], t[0]), sub(t[2], t[0]))].iter().fold(0.0f64, |a, b| a.max(*b)).sqrt();
    let eps = size * 1e-9 + 1e-12;
    let corners = t.map(to2);
    // A segment in the plane, cut to the triangle: by its three sides, inside on the left.
    let clip = |mut a: [f64; 2], mut b: [f64; 2]| -> Option<([f64; 2], [f64; 2])> {
        for k in 0..3 {
            let (p, q) = (corners[k], corners[(k + 1) % 3]);
            let side = |x: [f64; 2]| (q[0] - p[0]) * (x[1] - p[1]) - (q[1] - p[1]) * (x[0] - p[0]);
            // On the side counts as inside.
            let tiny = eps * size;
            let (sa, sb) = (side(a), side(b));
            let (oa, ob) = (sa < -tiny, sb < -tiny);
            if oa && ob {
                return None;
            }
            if oa {
                let r = sa / (sa - sb);
                a = [a[0] + (b[0] - a[0]) * r, a[1] + (b[1] - a[1]) * r];
            } else if ob {
                let r = sb / (sb - sa);
                b = [b[0] + (a[0] - b[0]) * r, b[1] + (a[1] - b[1]) * r];
            }
        }
        let dx = [b[0] - a[0], b[1] - a[1]];
        (dx[0].hypot(dx[1]) > eps).then_some((a, b))
    };
    let mut segments: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for u in others {
        let su = u.map(|p| dot(nn, p) - d);
        let tol = eps.max(1e-9);
        if su.iter().all(|v| *v > tol) || su.iter().all(|v| *v < -tol) {
            continue;
        }
        if su.iter().all(|v| v.abs() <= tol) {
            // On the plane: its sides, where they are within the triangle.
            for k in 0..3 {
                if let Some(sg) = clip(to2(u[k]), to2(u[(k + 1) % 3])) {
                    segments.push(sg);
                }
            }
            continue;
        }
        // Where it crosses the plane: a segment.
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for k in 0..3 {
            let (a, b) = (u[k], u[(k + 1) % 3]);
            let (da, db) = (su[k], su[(k + 1) % 3]);
            if da.abs() <= tol {
                pts.push(a);
            }
            if (da > tol && db < -tol) || (da < -tol && db > tol) {
                let r = da / (da - db);
                pts.push([a[0] + (b[0] - a[0]) * r, a[1] + (b[1] - a[1]) * r, a[2] + (b[2] - a[2]) * r]);
            }
        }
        if pts.len() >= 2 {
            if let Some(sg) = clip(to2(pts[0]), to2(pts[pts.len() - 1])) {
                segments.push(sg);
            }
        }
    }
    if segments.is_empty() {
        return vec![t];
    }
    let from2 = |p: spade::Point2<f64>| [0, 1, 2].map(|k| t[0][k] + ex[k] * p.x + ey[k] * p.y);
    // A triangulation can fail on points all but on top of each other: the triangle is then cut
    // by the planes of the others, as before.
    let made = crate::crash::quietly(|| -> Option<Vec<[[f64; 3]; 3]>> {
        let mut cdt = spade::ConstrainedDelaunayTriangulation::<spade::Point2<f64>>::new();
        let mut handles = Vec::new();
        for c in corners.iter().copied().chain(segments.iter().flat_map(|(a, b)| [*a, *b])) {
            handles.push(cdt.insert(spade::Point2::new(c[0], c[1])).ok()?);
        }
        for k in 0..3 {
            let (a, b) = (handles[k], handles[(k + 1) % 3]);
            if a != b {
                cdt.add_constraint_and_split(a, b, |v| v);
            }
        }
        for k in 0..segments.len() {
            let (a, b) = (handles[3 + 2 * k], handles[4 + 2 * k]);
            if a != b {
                cdt.add_constraint_and_split(a, b, |v| v);
            }
        }
        Some(
            cdt.inner_faces()
                .map(|f| {
                    let v = f.vertices().map(|v| from2(v.position()));
                    if dot(cross(sub(v[1], v[0]), sub(v[2], v[0])), nn) < 0.0 {
                        [v[0], v[2], v[1]]
                    } else {
                        v
                    }
                })
                .collect(),
        )
    })
    .flatten();
    made.unwrap_or_else(|| {
        let mut pieces = vec![t];
        for u in others {
            let m = cross(sub(u[1], u[0]), sub(u[2], u[0]));
            if dot(m, m) < 1e-24 {
                continue;
            }
            let m = norm(m);
            let e = dot(m, u[0]);
            pieces = pieces
                .into_iter()
                .flat_map(|p| {
                    let sd = p.map(|q| dot(m, q) - e);
                    if sd.iter().all(|v| *v <= 1e-9) || sd.iter().all(|v| *v >= -1e-9) {
                        return vec![p];
                    }
                    let mut out = Vec::new();
                    for poly in [clip_polygon(&p, m, e), clip_polygon(&p, m.map(|c| -c), -e)] {
                        for k in 1..poly.len().saturating_sub(1) {
                            out.push([poly[0], poly[k], poly[k + 1]]);
                        }
                    }
                    out
                })
                .collect();
        }
        pieces
    })
}

/// The triangles of `solid` whose box meets that of `t`.
fn near<'a>(t: &[[f64; 3]; 3], solid: &'a [[[f64; 3]; 3]]) -> Vec<&'a [[f64; 3]; 3]> {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in t {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let e = 1e-7;
    solid.iter().filter(|u| (0..3).all(|k| u.iter().any(|p| p[k] >= lo[k] - e) && u.iter().any(|p| p[k] <= hi[k] + e))).collect()
}

/// Takes a closed solid of any shape out of triangles: they are cut where its faces cross
/// them, and the pieces with the solid behind them go.
fn subtract_mesh(tris: Vec<[[f64; 3]; 3]>, solid: &[[[f64; 3]; 3]]) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    for t in tris {
        let close = near(&t, solid);
        if close.is_empty() {
            out.push(t);
            continue;
        }
        for p in split_triangle(t, &close) {
            let a = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            if dot(a, a) < 1e-24 {
                continue;
            }
            let n = norm(a);
            let c = [0, 1, 2].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0);
            // Behind a face is the material it bounds.
            if !inside_mesh(sub(c, n.map(|v| v * 1e-5)), solid) {
                out.push(p);
            }
        }
    }
    out
}

/// Whether a ray from `o` along `dir` meets a triangle in front of `o`.
fn ray_hits(o: [f64; 3], dir: [f64; 3], t: &[[f64; 3]; 3]) -> bool {
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let p = cross(dir, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-14 {
        return false;
    }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = dot(dir, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    dot(e2, q) * inv > 1e-9
}

/// Whether a point lies inside a closed mesh: three rays in odd directions, by majority.
fn inside_mesh(p: [f64; 3], tris: &[[[f64; 3]; 3]]) -> bool {
    let dirs = [[0.5773, 0.5774, 0.5776], [-0.6123, 0.3531, 0.7072], [0.2673, -0.8018, -0.5345]];
    dirs.iter().filter(|d| tris.iter().filter(|t| ray_hits(p, **d, t)).count() % 2 == 1).count() >= 2
}

/// The parts of `faces` that lie inside the closed solid `solid`, cut along its planes: the
/// faces a cut opens, such as the reveals of an opening or the face where a plane cuts.
/// Parts on its surface, or in a hole already cut, are left out.
fn faces_inside(faces: Vec<[[f64; 3]; 3]>, solid: &[[[f64; 3]; 3]]) -> Vec<[[f64; 3]; 3]> {
    if faces.is_empty() || solid.is_empty() {
        return Vec::new();
    }
    // Each face cut by the planes of the faces of the solid near it, so that every piece is
    // wholly inside or wholly outside.
    let mut pieces = Vec::new();
    for t in faces {
        let mut planes: Vec<([f64; 3], f64)> = Vec::new();
        for u in near(&t, solid) {
            let n = cross(sub(u[1], u[0]), sub(u[2], u[0]));
            if dot(n, n) < 1e-24 {
                continue;
            }
            let n = norm(n);
            let d = dot(n, u[0]);
            if !planes.iter().any(|(m, e)| dot(*m, n).abs() > 0.999999 && (e - d * dot(*m, n).signum()).abs() < 1e-7) {
                planes.push((n, d));
            }
        }
        let mut here = vec![t];
        for (n, d) in planes {
            here = here
                .into_iter()
                .flat_map(|p| {
                    let sd = p.map(|q| dot(n, q) - d);
                    if sd.iter().all(|v| *v <= 1e-9) || sd.iter().all(|v| *v >= -1e-9) {
                        return vec![p];
                    }
                    let mut out = Vec::new();
                    for poly in [clip_polygon(&p, n, d), clip_polygon(&p, n.map(|c| -c), -d)] {
                        for k in 1..poly.len().saturating_sub(1) {
                            out.push([poly[0], poly[k], poly[k + 1]]);
                        }
                    }
                    out
                })
                .collect();
            if here.len() > 50_000 {
                break;
            }
        }
        pieces.extend(here);
    }
    pieces
        .into_iter()
        .filter(|t| {
            let area = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            if dot(area, area) < 1e-20 {
                return false;
            }
            let c = [0, 1, 2].map(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0);
            // A new face has the solid on both sides of it, before the cut: one on a face
            // of the solid, or in a hole already cut, has not.
            let n = norm(area);
            let e = 1e-5;
            inside_mesh(sub(c, n.map(|v| v * e)), solid) && inside_mesh([c[0] + n[0] * e, c[1] + n[1] * e, c[2] + n[2] * e], solid)
        })
        .collect()
}

/// A square on a plane, large enough to cover the box of the points, facing along `n`.
fn plane_square(n: [f64; 3], d: f64, around: &[[[f64; 3]; 3]]) -> Vec<[[f64; 3]; 3]> {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in around.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if !lo[0].is_finite() {
        return Vec::new();
    }
    let mid = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let size = ((0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>()).sqrt() * 2.0 + 1.0;
    let c = sub(mid, n.map(|v| v * (dot(n, mid) - d)));
    let a = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = norm(cross(n, a));
    let v = cross(n, u);
    let at = |x: f64, y: f64| [0, 1, 2].map(|k| c[k] + u[k] * x * size + v[k] * y * size);
    let q = [at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)];
    // u × v = n: counterclockwise seen from where n points.
    vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
}

/// The planes of a convex solid given as triangles, facing out: (normal, distance).
fn planes_of(tris: &[[[f64; 3]; 3]]) -> Vec<([f64; 3], f64)> {
    if tris.is_empty() {
        return Vec::new();
    }
    let mut c = [0.0; 3];
    let mut n_pts = 0.0;
    for t in tris {
        for p in t {
            c = [c[0] + p[0], c[1] + p[1], c[2] + p[2]];
            n_pts += 1.0;
        }
    }
    let c = c.map(|v| v / n_pts);
    let mut out: Vec<([f64; 3], f64)> = Vec::new();
    for t in tris {
        let n = norm(cross(sub(t[1], t[0]), sub(t[2], t[0])));
        if dot(n, n) < 0.5 {
            continue;
        }
        let mut n = n;
        let mut d = dot(n, t[0]);
        // Facing away from the middle.
        if dot(n, c) > d {
            n = n.map(|v| -v);
            d = -d;
        }
        if !out.iter().any(|(m, e)| dot(*m, n) > 0.9999 && (e - d).abs() < 1e-6) {
            out.push((n, d));
        }
    }
    out
}

impl Ifc<'_> {
    fn colour_of(&self, item: u64, inherited: Option<Colour>) -> Option<Colour> {
        self.styles.get(&item).copied().or(inherited)
    }

    /// The triangles of a representation item, placed by `tf`, each set with its colour.
    fn item(&self, id: u64, tf: &Tf, colour: Option<Colour>, out: &mut Parts, notes: &mut Notes, depth: usize) {
        if depth > 16 {
            return;
        }
        let Some(e) = self.s.get(id) else { return };
        let colour = self.colour_of(id, colour);
        // A mirror turns faces inside out: they are turned back.
        let mirror = tf.det() < 0.0;
        let place = move |t: [[f64; 3]; 3]| {
            let q = t.map(|p| tf.apply(p));
            if mirror {
                [q[0], q[2], q[1]]
            } else {
                q
            }
        };
        let put = |tris: Vec<[[f64; 3]; 3]>, out: &mut Parts| {
            if !tris.is_empty() {
                out.push((colour, Part { tris: tris.into_iter().map(place).collect() }));
            }
        };
        match e.ty.as_str() {
            "IFCEXTRUDEDAREASOLID" | "IFCEXTRUDEDAREASOLIDTAPERED" => {
                let rings = self.profile(e.args.first().and_then(|v| v.r()).unwrap_or(0), notes);
                let place = e.args.get(1).and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
                let dir = self.direction(e.args.get(2).and_then(|v| v.r()), [0.0, 0.0, 1.0]);
                let depth = e.args.get(3).and_then(|v| v.num()).unwrap_or(0.0);
                let tris = extrude(&rings, dir, depth);
                put(tris.into_iter().map(|t| t.map(|p| place.apply(p))).collect(), out);
            }
            "IFCFACETEDBREP" | "IFCFACETEDBREPWITHVOIDS" | "IFCADVANCEDBREP" | "IFCADVANCEDBREPWITHVOIDS" => {
                let shell = e.args.first().and_then(|v| v.r()).unwrap_or(0);
                let before = self.scale_now.replace(tf.det().abs().cbrt().max(1e-12));
                put(outward(self.shell(shell, notes)), out);
                self.scale_now.set(before);
            }
            "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                let mut tris = Vec::new();
                for s in e.args.first().map(|v| v.refs()).unwrap_or_default() {
                    tris.extend(self.shell(s, notes));
                }
                put(tris, out);
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" => put(self.shell(id, notes), out),
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" => {
                let pts = self.point_list(e.args.first().and_then(|v| v.r()).unwrap_or(0));
                let idx = e.args.get(3).map(|v| v.list()).unwrap_or(&[]);
                let pn: Vec<usize> = e.args.get(4).map(|v| v.list().iter().filter_map(|x| x.num().map(|n| n as usize)).collect()).unwrap_or_default();
                let at = |i: usize| -> Option<[f64; 3]> {
                    let i = if pn.is_empty() { i } else { *pn.get(i.checked_sub(1)?)? };
                    pts.get(i.checked_sub(1)?).copied()
                };
                let tris = idx
                    .iter()
                    .filter_map(|t| {
                        let t: Vec<usize> = t.list().iter().filter_map(|x| x.num().map(|n| n as usize)).collect();
                        Some([at(*t.first()?)?, at(*t.get(1)?)?, at(*t.get(2)?)?])
                    })
                    .collect();
                put(tris, out);
            }
            "IFCPOLYGONALFACESET" => {
                let pts = self.point_list(e.args.first().and_then(|v| v.r()).unwrap_or(0));
                let pn: Vec<usize> = e.args.get(3).map(|v| v.list().iter().filter_map(|x| x.num().map(|n| n as usize)).collect()).unwrap_or_default();
                let at = |i: usize| -> Option<[f64; 3]> {
                    let i = if pn.is_empty() { i } else { *pn.get(i.checked_sub(1)?)? };
                    pts.get(i.checked_sub(1)?).copied()
                };
                let mut tris = Vec::new();
                for f in e.args.get(2).map(|v| v.refs()).unwrap_or_default() {
                    let Some(fe) = self.s.get(f) else { continue };
                    let ring = |v: &V| -> Vec<[f64; 3]> { v.list().iter().filter_map(|x| x.num().and_then(|n| at(n as usize))).collect() };
                    let outer = fe.args.first().map(ring).unwrap_or_default();
                    let holes: Vec<Vec<[f64; 3]>> = fe.args.get(1).map(|v| v.list().iter().map(ring).collect()).unwrap_or_default();
                    tris.extend(triangulate_face(&outer, &holes));
                }
                put(tris, out);
            }
            "IFCMAPPEDITEM" => {
                let source = e.args.first().and_then(|v| v.r()).unwrap_or(0);
                let target = e.args.get(1).and_then(|v| v.r()).map_or(Tf::ID, |t| self.transform_operator(t));
                let origin = self.s.arg(source, 0).and_then(|v| v.r()).map_or(Tf::ID, |o| self.axis3(o));
                let rep = self.s.arg(source, 1).and_then(|v| v.r()).unwrap_or(0);
                let inner = origin.then(&target).then(tf);
                let mut mapped = Vec::new();
                for it in self.s.arg(rep, 3).map(|v| v.refs()).unwrap_or_default() {
                    self.item(it, &inner, colour, &mut mapped, notes, depth + 1);
                }
                out.extend(mapped);
            }
            "IFCBOOLEANCLIPPINGRESULT" | "IFCBOOLEANRESULT" => {
                let op = e.args.first().and_then(|v| v.text()).unwrap_or_default();
                let first = e.args.get(1).and_then(|v| v.r()).unwrap_or(0);
                let second = e.args.get(2).and_then(|v| v.r()).unwrap_or(0);
                let mut parts = Vec::new();
                self.item(first, &Tf::ID, colour, &mut parts, notes, depth + 1);
                if op == "UNION" {
                    self.item(second, &Tf::ID, colour, &mut parts, notes, depth + 1);
                } else if op == "DIFFERENCE" && matches!(self.s.ty(second), "IFCHALFSPACESOLID" | "IFCBOXEDHALFSPACE" | "IFCPOLYGONALBOUNDEDHALFSPACE") {
                    self.cut_by_half_space(second, &mut parts, notes);
                } else if op == "DIFFERENCE" {
                    // Another solid taken away: where it is convex, as an opening is.
                    let mut cutter = Vec::new();
                    self.item(second, &Tf::ID, colour, &mut cutter, notes, depth + 1);
                    let tris: Vec<[[f64; 3]; 3]> = cutter.into_iter().flat_map(|(_, p)| p.tris).collect();
                    cut_out(&mut parts, &tris, self.tol());
                }
                for (c, p) in parts {
                    out.push((c, Part { tris: p.tris.into_iter().map(place).collect() }));
                }
            }
            "IFCHALFSPACESOLID" | "IFCPOLYGONALBOUNDEDHALFSPACE" | "IFCBOXEDHALFSPACE" | "IFCBOUNDINGBOX" | "IFCGEOMETRICCURVESET" | "IFCGEOMETRICSET" => {}
            "IFCPOLYLINE"
            | "IFCINDEXEDPOLYCURVE"
            | "IFCCOMPOSITECURVE"
            | "IFCTRIMMEDCURVE"
            | "IFCCARTESIANPOINT"
            | "IFCTEXTLITERAL"
            | "IFCANNOTATIONFILLAREA" => {}
            "IFCBLOCK" => {
                let place = e.args.first().and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
                let (x, y, z) = (
                    e.args.get(1).and_then(|v| v.num()).unwrap_or(0.0),
                    e.args.get(2).and_then(|v| v.num()).unwrap_or(0.0),
                    e.args.get(3).and_then(|v| v.num()).unwrap_or(0.0),
                );
                let tris = extrude(&[vec![[0.0, 0.0], [x, 0.0], [x, y], [0.0, y]]], [0.0, 0.0, 1.0], z);
                put(tris.into_iter().map(|t| t.map(|p| place.apply(p))).collect(), out);
            }
            other => self.warn_once(notes, &format!("A shape of the type {}", other)),
        }
    }

    /// Takes a half space away from the parts, closed where it cuts: a plane, a plane
    /// within a box, or a plane within a prism over a polygon, as roofs cut gable walls.
    fn cut_by_half_space(&self, id: u64, parts: &mut Parts, notes: &mut Notes) {
        // n points into the solid that is taken away.
        let (n, d) = self.half_space(id);
        let all: Vec<[[f64; 3]; 3]> = parts.iter().flat_map(|(_, p)| p.tris.iter().copied()).collect();
        if all.is_empty() {
            return;
        }
        let inward = (n.map(|c| -c), -d);
        let mut caps = Vec::new();
        if self.s.ty(id) == "IFCPOLYGONALBOUNDEDHALFSPACE" {
            let pos = self.s.arg(id, 2).and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
            let mut ring = self.curve(self.s.arg(id, 3).and_then(|v| v.r()).unwrap_or(0), notes);
            if ring.len() > 1 && ring.first() == ring.last() {
                ring.pop();
            }
            if ring.len() < 3 {
                return;
            }
            let ccw = |r: &[[f64; 2]]| (0..r.len()).map(|k| r[k][0] * r[(k + 1) % r.len()][1] - r[(k + 1) % r.len()][0] * r[k][1]).sum::<f64>() > 0.0;
            // The plane of an edge of the prism, facing out of it.
            let edge_plane = |a: [f64; 2], b: [f64; 2], ccw: bool| -> ([f64; 3], f64) {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let o = if ccw { [dy, -dx, 0.0] } else { [-dy, dx, 0.0] };
                let m = norm(pos.dir(o));
                (m, dot(m, pos.apply([a[0], a[1], 0.0])))
            };
            let pieces: Vec<Vec<[f64; 2]>> = if is_convex(&ring) {
                vec![ring.clone()]
            } else {
                let flat: Vec<[f64; 3]> = ring.iter().map(|p| [p[0], p[1], 0.0]).collect();
                crate::mesh::triangulate(&flat).into_iter().map(|t| t.iter().map(|i| ring[*i]).collect()).collect()
            };
            for piece in &pieces {
                let turn = ccw(piece);
                let mut planes: Vec<([f64; 3], f64)> = (0..piece.len()).map(|k| edge_plane(piece[k], piece[(k + 1) % piece.len()], turn)).collect();
                // Where the plane cuts, within the prism.
                let mut cap = plane_square(n, d, &all);
                for (m, e) in &planes {
                    cap = clip_by_plane(cap, *m, *e);
                }
                caps.extend(faces_inside(cap, &all));
                planes.push(inward);
                for (_, p) in parts.iter_mut() {
                    p.tris = subtract_convex(std::mem::take(&mut p.tris), &planes);
                }
            }
            // The sides of the prism, within the half space, where it steps.
            let turn = ccw(&ring);
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for c in all.iter().flatten().flatten() {
                lo = lo.min(*c);
                hi = hi.max(*c);
            }
            let big = (hi - lo) * 4.0 + 1.0;
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                let q = [[a[0], a[1], -big], [b[0], b[1], -big], [b[0], b[1], big], [a[0], a[1], big]].map(|p| pos.apply(p));
                // Facing into the prism, away from what is kept beside it.
                let side = if turn { vec![[q[0], q[2], q[1]], [q[0], q[3], q[2]]] } else { vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]] };
                let _ = edge_plane;
                caps.extend(faces_inside(clip_by_plane(side, inward.0, inward.1), &all));
            }
        } else {
            for (_, p) in parts.iter_mut() {
                p.tris = clip_by_plane(std::mem::take(&mut p.tris), n, d);
            }
            caps.extend(faces_inside(plane_square(n, d, &all), &all));
        }
        if let Some((_, p)) = parts.first_mut() {
            p.tris.extend(caps);
        }
    }

    fn half_space(&self, id: u64) -> ([f64; 3], f64) {
        let surface = self.s.arg(id, 0).and_then(|v| v.r()).unwrap_or(0);
        let agree = !matches!(self.s.arg(id, 1), Some(V::Enum(f)) if f == "F");
        let plane = self.s.arg(surface, 0).and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
        let n = norm(plane.dir([0.0, 0.0, 1.0]));
        let o = plane.apply([0.0; 3]);
        // The solid lies on the side the normal points away from when AgreementFlag is true.
        let n = if agree { n.map(|c| -c) } else { n };
        (n, dot(n, o))
    }

    fn point_list(&self, id: u64) -> Vec<[f64; 3]> {
        self.s
            .arg(id, 0)
            .map(|v| {
                v.list()
                    .iter()
                    .map(|c| {
                        let c = c.list();
                        let n = |k: usize| c.get(k).and_then(|x| x.num()).unwrap_or(0.0);
                        [n(0), n(1), n(2)]
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn shell(&self, id: u64, notes: &mut Notes) -> Vec<[[f64; 3]; 3]> {
        let mut out = Vec::new();
        for f in self.s.arg(id, 0).map(|v| v.refs()).unwrap_or_default() {
            let Some(fe) = self.s.get(f) else { continue };
            if matches!(fe.ty.as_str(), "IFCADVANCEDFACE" | "IFCFACESURFACE") {
                out.extend(self.advanced_face(f, notes));
                continue;
            }
            if !fe.ty.starts_with("IFCFACE") {
                continue;
            }
            let (mut outer, mut holes) = (Vec::new(), Vec::new());
            for b in fe.args.first().map(|v| v.refs()).unwrap_or_default() {
                let Some(be) = self.s.get(b) else { continue };
                let lp = be.args.first().and_then(|v| v.r()).unwrap_or(0);
                let mut pts: Vec<[f64; 3]> = self.loop_points(lp, notes);
                if pts.len() < 3 {
                    continue;
                }
                if matches!(be.args.get(1), Some(V::Enum(f)) if f == "F") {
                    pts.reverse();
                }
                if be.ty == "IFCFACEOUTERBOUND" || (outer.is_empty() && holes.is_empty()) {
                    if !outer.is_empty() {
                        holes.push(std::mem::replace(&mut outer, pts));
                    } else {
                        outer = pts;
                    }
                } else {
                    holes.push(pts);
                }
            }
            out.extend(triangulate_face(&outer, &holes));
        }
        out
    }
}

// ---------- Advanced B-reps: faces on curved surfaces, bounded by curves ----------

/// The full knot vector, from the knots and how many times each is repeated.
fn expand_knots(mult: &[f64], knots: &[f64]) -> Vec<f64> {
    knots.iter().zip(mult).flat_map(|(k, m)| std::iter::repeat_n(*k, m.max(0.0) as usize)).collect()
}

/// The span of the knots that holds `t`, for a B-spline of degree `p` with `n` + 1 control
/// points (The NURBS Book, A2.1).
fn find_span(n: usize, p: usize, t: f64, u: &[f64]) -> usize {
    if t >= u[n + 1] {
        return n;
    }
    if t <= u[p] {
        return p;
    }
    let (mut lo, mut hi) = (p, n + 1);
    let mut mid = (lo + hi) / 2;
    while t < u[mid] || t >= u[mid + 1] {
        if t < u[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
        mid = (lo + hi) / 2;
    }
    mid
}

/// The basis functions that are not zero at `t` (A2.2).
fn basis_funs(span: usize, t: f64, p: usize, u: &[f64]) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    n[0] = 1.0;
    for j in 1..=p {
        left[j] = t - u[span + 1 - j];
        right[j] = u[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let d = right[r + 1] + left[j - r];
            let temp = if d.abs() < 1e-300 { 0.0 } else { n[r] / d };
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

/// A B-spline curve, rational when the weights differ.
#[derive(Debug, Clone)]
struct Spline1 {
    deg: usize,
    knots: Vec<f64>,
    ctrl: Vec<[f64; 3]>,
    w: Vec<f64>,
}

impl Spline1 {
    fn domain(&self) -> (f64, f64) {
        (self.knots[self.deg], self.knots[self.ctrl.len()])
    }
    fn eval(&self, t: f64) -> [f64; 3] {
        let n = self.ctrl.len() - 1;
        let span = find_span(n, self.deg, t, &self.knots);
        let b = basis_funs(span, t, self.deg, &self.knots);
        let (mut p, mut wsum) = ([0.0; 3], 0.0);
        for (j, bj) in b.iter().enumerate() {
            let i = span - self.deg + j;
            let w = self.w[i] * bj;
            for (pk, ck) in p.iter_mut().zip(self.ctrl[i]) {
                *pk += ck * w;
            }
            wsum += w;
        }
        p.map(|c| c / wsum.max(1e-300))
    }
    /// The parameter of the point of the curve nearest `p`.
    fn inverse(&self, p: [f64; 3]) -> f64 {
        let (a, b) = self.domain();
        let steps = 200;
        let at = |k: usize| a + (b - a) * k as f64 / steps as f64;
        let d2 = |t: f64| {
            let q = sub(self.eval(t), p);
            dot(q, q)
        };
        let k = (0..=steps).min_by(|x, y| d2(at(*x)).total_cmp(&d2(at(*y)))).unwrap_or(0);
        // Narrowed down between the neighbours.
        let (mut lo, mut hi) = (at(k.saturating_sub(1)), at((k + 1).min(steps)));
        for _ in 0..60 {
            let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if d2(m1) < d2(m2) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        (lo + hi) / 2.0
    }
}

/// A B-spline surface, rational when the weights differ: control points by u, then v.
#[derive(Debug, Clone)]
struct Spline2 {
    du: usize,
    dv: usize,
    ku: Vec<f64>,
    kv: Vec<f64>,
    ctrl: Vec<Vec<[f64; 3]>>,
    w: Vec<Vec<f64>>,
}

impl Spline2 {
    fn domain(&self) -> ([f64; 2], [f64; 2]) {
        ([self.ku[self.du], self.ku[self.ctrl.len()]], [self.kv[self.dv], self.kv[self.ctrl[0].len()]])
    }
    fn eval(&self, u: f64, v: f64) -> [f64; 3] {
        let (nu, nv) = (self.ctrl.len() - 1, self.ctrl[0].len() - 1);
        let (su, sv) = (find_span(nu, self.du, u, &self.ku), find_span(nv, self.dv, v, &self.kv));
        let (bu, bv) = (basis_funs(su, u, self.du, &self.ku), basis_funs(sv, v, self.dv, &self.kv));
        let (mut p, mut wsum) = ([0.0; 3], 0.0);
        for (a, ba) in bu.iter().enumerate() {
            let i = su - self.du + a;
            for (b, bb) in bv.iter().enumerate() {
                let j = sv - self.dv + b;
                let w = self.w[i][j] * ba * bb;
                for (pk, ck) in p.iter_mut().zip(self.ctrl[i][j]) {
                    *pk += ck * w;
                }
                wsum += w;
            }
        }
        p.map(|c| c / wsum.max(1e-300))
    }
}

/// The surface a face lies on, with its parameters (u, v).
#[derive(Debug, Clone)]
enum Surf {
    Plane(Tf),
    Cylinder(Tf, f64),
    Cone(Tf, f64, f64),
    Sphere(Tf, f64),
    Torus(Tf, f64, f64),
    Spline(Spline2),
}

impl Surf {
    fn local(tf: &Tf, p: [f64; 3]) -> [f64; 3] {
        let d = sub(p, tf.apply([0.0; 3]));
        [dot(d, tf.dir([1.0, 0.0, 0.0])), dot(d, tf.dir([0.0, 1.0, 0.0])), dot(d, tf.dir([0.0, 0.0, 1.0]))]
    }
    fn eval(&self, u: f64, v: f64) -> [f64; 3] {
        match self {
            Surf::Plane(tf) => tf.apply([u, v, 0.0]),
            Surf::Cylinder(tf, r) => tf.apply([r * u.cos(), r * u.sin(), v]),
            Surf::Cone(tf, r, semi) => {
                let rr = r + v * semi.tan();
                tf.apply([rr * u.cos(), rr * u.sin(), v])
            }
            Surf::Sphere(tf, r) => tf.apply([r * v.cos() * u.cos(), r * v.cos() * u.sin(), r * v.sin()]),
            Surf::Torus(tf, big, r) => tf.apply([(big + r * v.cos()) * u.cos(), (big + r * v.cos()) * u.sin(), r * v.sin()]),
            Surf::Spline(s) => s.eval(u, v),
        }
    }
    /// Whether u and v go round, as an angle.
    fn periodic(&self) -> [bool; 2] {
        match self {
            Surf::Cylinder(..) | Surf::Cone(..) | Surf::Sphere(..) => [true, false],
            Surf::Torus(..) => [true, true],
            _ => [false, false],
        }
    }
    /// The parameters of the point of the surface nearest `p`, starting from `near` for a
    /// spline.
    fn inverse(&self, p: [f64; 3], near: Option<[f64; 2]>) -> [f64; 2] {
        match self {
            Surf::Plane(tf) => {
                let l = Surf::local(tf, p);
                [l[0], l[1]]
            }
            Surf::Cylinder(tf, _) | Surf::Cone(tf, _, _) => {
                let l = Surf::local(tf, p);
                [l[1].atan2(l[0]), l[2]]
            }
            Surf::Sphere(tf, _) => {
                let l = Surf::local(tf, p);
                [l[1].atan2(l[0]), l[2].atan2(l[0].hypot(l[1]))]
            }
            Surf::Torus(tf, big, _) => {
                let l = Surf::local(tf, p);
                [l[1].atan2(l[0]), l[2].atan2(l[0].hypot(l[1]) - big)]
            }
            Surf::Spline(s) => {
                let (du, dv) = s.domain();
                let d2 = |uv: [f64; 2]| {
                    let q = sub(s.eval(uv[0], uv[1]), p);
                    dot(q, q)
                };
                let grid = || {
                    let n = 16;
                    let mut best = ([du[0], dv[0]], f64::INFINITY);
                    for i in 0..=n {
                        for j in 0..=n {
                            let uv = [du[0] + (du[1] - du[0]) * i as f64 / n as f64, dv[0] + (dv[1] - dv[0]) * j as f64 / n as f64];
                            let d = d2(uv);
                            if d < best.1 {
                                best = (uv, d);
                            }
                        }
                    }
                    best.0
                };
                let newton = |mut uv: [f64; 2]| {
                    let (hu, hv) = ((du[1] - du[0]).abs().max(1e-12) * 1e-6, (dv[1] - dv[0]).abs().max(1e-12) * 1e-6);
                    for _ in 0..20 {
                        let s0 = s.eval(uv[0], uv[1]);
                        let f = sub(s0, p);
                        // Forwards, or backwards at the far edge of the domain.
                        let (eu, ev) = (if uv[0] + hu > du[1] { -hu } else { hu }, if uv[1] + hv > dv[1] { -hv } else { hv });
                        let su = sub(s.eval(uv[0] + eu, uv[1]), s0).map(|c| c / eu);
                        let sv = sub(s.eval(uv[0], uv[1] + ev), s0).map(|c| c / ev);
                        let (a, b, c) = (dot(su, su), dot(su, sv), dot(sv, sv));
                        let det = a * c - b * b;
                        if det.abs() < 1e-300 {
                            break;
                        }
                        let (gu, gv) = (dot(su, f), dot(sv, f));
                        let step = [-(c * gu - b * gv) / det, -(a * gv - b * gu) / det];
                        uv = [(uv[0] + step[0]).clamp(du[0], du[1]), (uv[1] + step[1]).clamp(dv[0], dv[1])];
                        if step[0].abs() < hu * 1e-3 && step[1].abs() < hv * 1e-3 {
                            break;
                        }
                    }
                    uv
                };
                let first = newton(near.unwrap_or_else(grid));
                // A start too far away may end on another part of the surface.
                let size = sub(s.eval(du[1], dv[1]), s.eval(du[0], dv[0]));
                if near.is_some() && d2(first) > dot(size, size) * 1e-8 {
                    let again = newton(grid());
                    if d2(again) < d2(first) {
                        return again;
                    }
                }
                first
            }
        }
    }
    /// The normal at (u, v), from the way the parameters run.
    fn normal(&self, u: f64, v: f64) -> [f64; 3] {
        let h = 1e-6;
        let p = self.eval(u, v);
        let su = sub(self.eval(u + h, v), p);
        let sv = sub(self.eval(u, v + h), p);
        cross(su, sv)
    }
}

impl Ifc<'_> {
    fn surface(&self, id: u64) -> Option<Surf> {
        let e = self.s.get(id)?;
        let pos = || e.args.first().and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
        let num = |k: usize| e.args.get(k).and_then(|v| v.num()).unwrap_or(0.0);
        Some(match e.ty.as_str() {
            "IFCPLANE" => Surf::Plane(pos()),
            "IFCCYLINDRICALSURFACE" => Surf::Cylinder(pos(), num(1)),
            "IFCCONICALSURFACE" => Surf::Cone(pos(), num(1), num(2) * self.angle),
            "IFCSPHERICALSURFACE" => Surf::Sphere(pos(), num(1)),
            "IFCTOROIDALSURFACE" => Surf::Torus(pos(), num(1), num(2)),
            "IFCBSPLINESURFACEWITHKNOTS" | "IFCRATIONALBSPLINESURFACEWITHKNOTS" => {
                let nums = |k: usize| -> Vec<f64> { e.args.get(k).map(|v| v.list().iter().filter_map(|x| x.num()).collect()).unwrap_or_default() };
                let ctrl: Vec<Vec<[f64; 3]>> =
                    e.args.get(2).map(|v| v.list().iter().map(|row| row.refs().iter().map(|p| self.point(*p)).collect()).collect()).unwrap_or_default();
                if ctrl.is_empty() || ctrl[0].is_empty() {
                    return None;
                }
                let w: Vec<Vec<f64>> = if e.ty.starts_with("IFCRATIONAL") {
                    e.args.get(12).map(|v| v.list().iter().map(|row| row.list().iter().filter_map(|x| x.num()).collect()).collect()).unwrap_or_default()
                } else {
                    ctrl.iter().map(|r| vec![1.0; r.len()]).collect()
                };
                let (du, dv) = (num(0) as usize, num(1) as usize);
                let (ku, kv) = (expand_knots(&nums(7), &nums(9)), expand_knots(&nums(8), &nums(10)));
                if ku.len() != ctrl.len() + du + 1
                    || kv.len() != ctrl[0].len() + dv + 1
                    || w.len() != ctrl.len()
                    || w.iter().zip(&ctrl).any(|(a, b)| a.len() != b.len())
                {
                    return None;
                }
                Surf::Spline(Spline2 { du, dv, ku, kv, ctrl, w })
            }
            _ => return None,
        })
    }

    /// The tolerance for the item being read, in its own units.
    fn tol(&self) -> f64 {
        self.tolerance / self.scale_now.get()
    }

    fn vertex(&self, id: u64) -> [f64; 3] {
        self.s.arg(id, 0).and_then(|v| v.r()).map_or([0.0; 3], |p| self.point(p))
    }

    /// The points along an edge, from its start to its end.
    fn edge_points(&self, edge: u64, notes: &mut Notes) -> Vec<[f64; 3]> {
        let Some(e) = self.s.get(edge) else { return Vec::new() };
        if e.ty == "IFCORIENTEDEDGE" {
            let mut pts = self.edge_points(e.args.get(2).and_then(|v| v.r()).unwrap_or(0), notes);
            if matches!(e.args.get(3), Some(V::Enum(f)) if f == "F") {
                pts.reverse();
            }
            return pts;
        }
        let a = self.vertex(e.args.first().and_then(|v| v.r()).unwrap_or(0));
        let b = self.vertex(e.args.get(1).and_then(|v| v.r()).unwrap_or(0));
        let same = !matches!(e.args.get(3), Some(V::Enum(f)) if f == "F");
        let curve = e.args.get(2).and_then(|v| v.r()).unwrap_or(0);
        let closed = {
            let d = sub(a, b);
            dot(d, d) < 1e-18
        };
        let Some(ce) = self.s.get(curve) else { return vec![a, b] };
        let mut pts = match ce.ty.as_str() {
            "IFCLINE" => vec![a, b],
            "IFCPOLYLINE" => {
                let mut p: Vec<[f64; 3]> = ce.args.first().map(|v| v.refs()).unwrap_or_default().iter().map(|q| self.point(*q)).collect();
                if !same {
                    p.reverse();
                }
                p
            }
            "IFCCIRCLE" | "IFCELLIPSE" => {
                let tf = ce.args.first().and_then(|v| v.r()).map_or(Tf::ID, |p| self.axis3(p));
                let rx = ce.args.get(1).and_then(|v| v.num()).unwrap_or(0.0);
                let ry = if ce.ty == "IFCELLIPSE" { ce.args.get(2).and_then(|v| v.num()).unwrap_or(rx) } else { rx };
                let angle = |p: [f64; 3]| {
                    let l = Surf::local(&tf, p);
                    (l[1] / ry.max(1e-300)).atan2(l[0] / rx.max(1e-300))
                };
                let a0 = angle(a);
                let mut a1 = angle(b);
                let tau = std::f64::consts::TAU;
                if closed {
                    a1 = if same { a0 + tau } else { a0 - tau };
                } else if same {
                    while a1 <= a0 {
                        a1 += tau;
                    }
                } else {
                    while a1 >= a0 {
                        a1 -= tau;
                    }
                }
                // As many pieces as keep the chord within the tolerance.
                let r = rx.max(ry).max(1e-300);
                let per_turn = (std::f64::consts::PI / (1.0 - (self.tol() / r).min(1.0)).acos().max(1e-6)).clamp(8.0, 128.0);
                let n = (((a1 - a0).abs() / tau * per_turn).ceil() as usize).max(2);
                (0..=n).map(|k| a0 + (a1 - a0) * k as f64 / n as f64).map(|t| tf.apply([rx * t.cos(), ry * t.sin(), 0.0])).collect()
            }
            "IFCBSPLINECURVEWITHKNOTS" | "IFCRATIONALBSPLINECURVEWITHKNOTS" => {
                let nums = |k: usize| -> Vec<f64> { ce.args.get(k).map(|v| v.list().iter().filter_map(|x| x.num()).collect()).unwrap_or_default() };
                let ctrl: Vec<[f64; 3]> = ce.args.get(1).map(|v| v.refs()).unwrap_or_default().iter().map(|p| self.point(*p)).collect();
                let deg = ce.args.first().and_then(|v| v.num()).unwrap_or(1.0) as usize;
                let knots = expand_knots(&nums(5), &nums(6));
                let w = if ce.ty.starts_with("IFCRATIONAL") { nums(8) } else { vec![1.0; ctrl.len()] };
                if ctrl.len() < 2 || knots.len() != ctrl.len() + deg + 1 || w.len() != ctrl.len() {
                    vec![a, b]
                } else {
                    let sp = Spline1 { deg, knots, ctrl, w };
                    let (lo, hi) = sp.domain();
                    let (t0, t1) = if closed {
                        if same {
                            (lo, hi)
                        } else {
                            (hi, lo)
                        }
                    } else {
                        (sp.inverse(a), sp.inverse(b))
                    };
                    let n = ((sp.ctrl.len() * 8) as f64 * ((t1 - t0).abs() / (hi - lo).abs().max(1e-300))).ceil().max(2.0) as usize;
                    // Not thinned: a face on a curved surface needs its edges in small steps.
                    let n = n.min(256);
                    (0..=n).map(|k| sp.eval(t0 + (t1 - t0) * k as f64 / n as f64)).collect()
                }
            }
            other => {
                self.warn_once(notes, &format!("An edge along a {}", other));
                vec![a, b]
            }
        };
        // The ends exactly at the vertices.
        if let Some(f) = pts.first_mut() {
            *f = a;
        }
        if let Some(l) = pts.last_mut() {
            *l = b;
        }
        pts
    }

    /// The points around a loop of a face, without the first again at the end.
    fn loop_points(&self, id: u64, notes: &mut Notes) -> Vec<[f64; 3]> {
        match self.s.ty(id) {
            "IFCPOLYLOOP" => self.s.arg(id, 0).map(|v| v.refs()).unwrap_or_default().iter().map(|p| self.point(*p)).collect(),
            "IFCEDGELOOP" => {
                let mut out: Vec<[f64; 3]> = Vec::new();
                for oe in self.s.arg(id, 0).map(|v| v.refs()).unwrap_or_default() {
                    let pts = self.edge_points(oe, notes);
                    for p in pts {
                        if out.last().is_none_or(|q| {
                            let d = sub(*q, p);
                            dot(d, d) > 1e-18
                        }) {
                            out.push(p);
                        }
                    }
                }
                if out.len() > 1 {
                    let d = sub(out[0], *out.last().unwrap());
                    if dot(d, d) < 1e-18 {
                        out.pop();
                    }
                }
                out
            }
            other => {
                self.warn_once(notes, &format!("A face bounded by {}", other));
                Vec::new()
            }
        }
    }

    /// The triangles of a face on a surface: an IfcAdvancedFace or IfcFaceSurface.
    fn advanced_face(&self, id: u64, notes: &mut Notes) -> Vec<[[f64; 3]; 3]> {
        let Some(fe) = self.s.get(id) else { return Vec::new() };
        let Some(surf) = self.surface(fe.args.get(1).and_then(|v| v.r()).unwrap_or(0)) else {
            let ty = self.s.ty(fe.args.get(1).and_then(|v| v.r()).unwrap_or(0)).to_string();
            self.warn_once(notes, &format!("A face on a {}", ty));
            return Vec::new();
        };
        let same = !matches!(fe.args.get(2), Some(V::Enum(f)) if f == "F");
        let mut loops: Vec<(Vec<[f64; 3]>, bool)> = Vec::new();
        for b in fe.args.first().map(|v| v.refs()).unwrap_or_default() {
            let Some(be) = self.s.get(b) else { continue };
            let mut pts = self.loop_points(be.args.first().and_then(|v| v.r()).unwrap_or(0), notes);
            if matches!(be.args.get(1), Some(V::Enum(f)) if f == "F") {
                pts.reverse();
            }
            if pts.len() >= 2 {
                loops.push((pts, be.ty == "IFCFACEOUTERBOUND"));
            }
        }
        if loops.is_empty() {
            return Vec::new();
        }
        if !loops.iter().any(|l| l.1) {
            loops[0].1 = true;
        }
        let outer_at = loops.iter().position(|l| l.1).unwrap_or(0);
        if let Surf::Plane(_) = surf {
            let outer = loops[outer_at].0.clone();
            let holes: Vec<Vec<[f64; 3]>> = loops.iter().enumerate().filter(|(k, _)| *k != outer_at).map(|(_, l)| l.0.clone()).collect();
            // Facing as the plane does, or against it.
            let n = surf.normal(0.0, 0.0).map(|c| if same { c } else { -c });
            return triangulate_face(&outer, &holes)
                .into_iter()
                .map(|t| if dot(cross(sub(t[1], t[0]), sub(t[2], t[0])), n) < 0.0 { [t[0], t[2], t[1]] } else { t })
                .collect();
        }
        // Into the parameters of the surface, with angles kept from jumping a turn.
        let per = surf.periodic();
        let tau = std::f64::consts::TAU;
        let mut wraps = false;
        let mut uv_loops: Vec<Vec<[f64; 3]>> = Vec::new();
        for (pts, _) in &loops {
            let mut uv: Vec<[f64; 2]> = Vec::new();
            for p in pts {
                let mut q = surf.inverse(*p, uv.last().copied());
                if let Some(prev) = uv.last() {
                    for k in 0..2 {
                        if per[k] {
                            while q[k] - prev[k] > std::f64::consts::PI {
                                q[k] -= tau;
                            }
                            while q[k] - prev[k] < -std::f64::consts::PI {
                                q[k] += tau;
                            }
                        }
                    }
                }
                uv.push(q);
            }
            // Round once: the loop goes about the surface, as a circle round a cylinder.
            if let (Some(f), Some(l)) = (uv.first(), uv.last()) {
                for k in 0..2 {
                    if per[k] && (l[k] - f[k]).abs() > std::f64::consts::PI {
                        wraps = true;
                    }
                }
            }
            uv_loops.push(uv.into_iter().map(|q| [q[0], q[1], 0.0]).collect());
        }
        // The parameters the face covers: all round for a band about the surface.
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in uv_loops.iter().flatten() {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if !(lo[0].is_finite() && lo[1].is_finite()) || hi[0] - lo[0] <= 0.0 || hi[1] - lo[1] <= 0.0 {
            return Vec::new();
        }
        for k in 0..2 {
            if wraps && per[k] && hi[k] - lo[k] > std::f64::consts::PI {
                hi[k] = lo[k] + tau;
            }
        }
        // As many rows and columns as keep the surface within the tolerance of flat cells.
        let (mut blo, mut bhi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in loops.iter().flat_map(|l| l.0.iter()) {
            for k in 0..3 {
                blo[k] = blo[k].min(p[k]);
                bhi[k] = bhi[k].max(p[k]);
            }
        }
        let size = dot(sub(bhi, blo), sub(bhi, blo)).sqrt();
        let tol = (size * 0.005).max(self.tol());
        let steps = |along: usize| -> usize {
            let mut n = 1;
            while n < 64 {
                let mut worst: f64 = 0.0;
                for row in 0..=4 {
                    for c in 0..n {
                        let (a, b, m) = if along == 0 {
                            let v = lo[1] + (hi[1] - lo[1]) * row as f64 / 4.0;
                            let u = |t: f64| lo[0] + (hi[0] - lo[0]) * t;
                            ([u(c as f64 / n as f64), v], [u((c + 1) as f64 / n as f64), v], [u((c as f64 + 0.5) / n as f64), v])
                        } else {
                            let u = lo[0] + (hi[0] - lo[0]) * row as f64 / 4.0;
                            let v = |t: f64| lo[1] + (hi[1] - lo[1]) * t;
                            ([u, v(c as f64 / n as f64)], [u, v((c + 1) as f64 / n as f64)], [u, v((c as f64 + 0.5) / n as f64)])
                        };
                        let (pa, pb, pm) = (surf.eval(a[0], a[1]), surf.eval(b[0], b[1]), surf.eval(m[0], m[1]));
                        let d = sub(pm, [0, 1, 2].map(|k| (pa[k] + pb[k]) / 2.0));
                        worst = worst.max(dot(d, d).sqrt());
                    }
                }
                if worst <= tol {
                    break;
                }
                n *= 2;
            }
            n
        };
        let (nu, nv) = (steps(0), steps(1));
        // The face in the parameters, cut by the grid: whole cells inside it are two
        // triangles, cells at its edge what of them is inside.
        let ring = |l: &Vec<[f64; 3]>| geo::LineString::new(l.iter().map(|p| geo::coord! { x: p[0], y: p[1] }).collect());
        let shape: geo::Polygon<f64> = if wraps {
            geo::Rect::new(geo::coord! { x: lo[0], y: lo[1] }, geo::coord! { x: hi[0], y: hi[1] }).to_polygon()
        } else {
            geo::Polygon::new(ring(&uv_loops[outer_at]), uv_loops.iter().enumerate().filter(|(k, _)| *k != outer_at).map(|(_, l)| ring(l)).collect())
        };
        let shape = geo::MultiPolygon::new(vec![shape]);
        let mut tris: Vec<[[f64; 3]; 3]> = Vec::new();
        for i in 0..nu {
            for j in 0..nv {
                let (u0, u1) = (lo[0] + (hi[0] - lo[0]) * i as f64 / nu as f64, lo[0] + (hi[0] - lo[0]) * (i + 1) as f64 / nu as f64);
                let (v0, v1) = (lo[1] + (hi[1] - lo[1]) * j as f64 / nv as f64, lo[1] + (hi[1] - lo[1]) * (j + 1) as f64 / nv as f64);
                let cell = geo::Rect::new(geo::coord! { x: u0, y: v0 }, geo::coord! { x: u1, y: v1 }).to_polygon();
                if wraps {
                    tris.push([[u0, v0, 0.0], [u1, v0, 0.0], [u1, v1, 0.0]]);
                    tris.push([[u0, v0, 0.0], [u1, v1, 0.0], [u0, v1, 0.0]]);
                    continue;
                }
                use geo::BooleanOps;
                let part = shape.intersection(&geo::MultiPolygon::new(vec![cell]));
                for piece in part.0 {
                    let to3 = |l: &geo::LineString<f64>| -> Vec<[f64; 3]> {
                        let mut v: Vec<[f64; 3]> = l.0.iter().map(|c| [c.x, c.y, 0.0]).collect();
                        if v.len() > 1 && v.first() == v.last() {
                            v.pop();
                        }
                        v
                    };
                    let outer = to3(piece.exterior());
                    let holes: Vec<Vec<[f64; 3]>> = piece.interiors().iter().map(to3).collect();
                    tris.extend(triangulate_face(&outer, &holes));
                }
            }
        }
        let at = |q: [f64; 3]| surf.eval(q[0], q[1]);
        // Onto the surface, facing as the face does.
        tris.into_iter()
            .map(|t| {
                let p = t.map(at);
                let c = [(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0];
                let mut n = surf.normal(c[0], c[1]);
                if !same {
                    n = n.map(|x| -x);
                }
                if dot(cross(sub(p[1], p[0]), sub(p[2], p[0])), n) < 0.0 {
                    [p[0], p[2], p[1]]
                } else {
                    p
                }
            })
            .collect()
    }
}

// ---------- Products ----------

/// The names of the classes as IFC writes them, for the ones written in capitals in files.
const CLASSES: &[&str] = &[
    "IfcWall",
    "IfcWallStandardCase",
    "IfcWallElementedCase",
    "IfcSlab",
    "IfcSlabStandardCase",
    "IfcSlabElementedCase",
    "IfcRoof",
    "IfcWindow",
    "IfcWindowStandardCase",
    "IfcDoor",
    "IfcDoorStandardCase",
    "IfcColumn",
    "IfcColumnStandardCase",
    "IfcBeam",
    "IfcBeamStandardCase",
    "IfcStair",
    "IfcStairFlight",
    "IfcRamp",
    "IfcRampFlight",
    "IfcRailing",
    "IfcCovering",
    "IfcCurtainWall",
    "IfcPlate",
    "IfcPlateStandardCase",
    "IfcMember",
    "IfcMemberStandardCase",
    "IfcFooting",
    "IfcPile",
    "IfcChimney",
    "IfcShadingDevice",
    "IfcBuildingElementProxy",
    "IfcBuildingElementPart",
    "IfcFurnishingElement",
    "IfcFurniture",
    "IfcSystemFurnitureElement",
    "IfcFlowTerminal",
    "IfcFlowSegment",
    "IfcFlowFitting",
    "IfcFlowController",
    "IfcDistributionElement",
    "IfcSanitaryTerminal",
    "IfcLightFixture",
    "IfcLamp",
    "IfcSpace",
    "IfcSite",
    "IfcBuilding",
    "IfcBuildingStorey",
    "IfcOpeningElement",
    "IfcOpeningStandardCase",
    "IfcGeographicElement",
    "IfcCivilElement",
    "IfcTransportElement",
    "IfcPipeSegment",
    "IfcPipeFitting",
    "IfcDuctSegment",
    "IfcDuctFitting",
    "IfcAirTerminal",
    "IfcCableSegment",
    "IfcCableCarrierSegment",
    "IfcElectricAppliance",
    "IfcBridge",
    "IfcBridgePart",
    "IfcRoad",
    "IfcRoadPart",
    "IfcRailway",
    "IfcCourse",
    "IfcPavement",
    "IfcKerb",
    "IfcEarthworksFill",
    "IfcEarthworksCut",
    "IfcSign",
    "IfcSignal",
    "IfcTank",
    "IfcPump",
    "IfcValve",
    "IfcBearing",
    "IfcTendon",
    "IfcReinforcingBar",
    "IfcElementAssembly",
    "IfcDiscreteAccessory",
    "IfcMechanicalFastener",
    "IfcFastener",
    "IfcVirtualElement",
    "IfcAnnotation",
    "IfcGrid",
    "IfcAlignment",
    "IfcTrackElement",
    "IfcRail",
];

fn class_name(upper: &str) -> String {
    CLASSES.iter().find(|c| c.eq_ignore_ascii_case(upper)).map_or_else(
        || {
            let rest = upper.strip_prefix("IFC").unwrap_or(upper).to_lowercase();
            let mut c = rest.chars();
            format!("Ifc{}{}", c.next().map(|x| x.to_ascii_uppercase()).unwrap_or_default(), c.as_str())
        },
        |c| c.to_string(),
    )
}

/// The colour of an element without a style of its own: by its class.
fn class_colour(class: &str) -> ([f32; 3], f32) {
    let c = class.to_uppercase();
    if c.contains("WINDOW") || c.contains("CURTAINWALL") {
        ([0.55, 0.68, 0.76], 0.35)
    } else if c.contains("DOOR") {
        ([0.55, 0.42, 0.30], 1.0)
    } else if c.contains("ROOF") {
        ([0.55, 0.30, 0.24], 1.0)
    } else if c.contains("SLAB") || c.contains("FOOTING") || c.contains("PILE") {
        ([0.68, 0.68, 0.66], 1.0)
    } else if c.contains("BEAM") || c.contains("COLUMN") || c.contains("MEMBER") {
        ([0.62, 0.62, 0.64], 1.0)
    } else if c.contains("STAIR") || c.contains("RAMP") || c.contains("RAILING") {
        ([0.60, 0.58, 0.55], 1.0)
    } else if c.contains("SPACE") {
        ([0.70, 0.80, 0.90], 0.3)
    } else if c.contains("SITE") || c.contains("GEOGRAPHIC") {
        ([0.45, 0.55, 0.36], 1.0)
    } else if c.contains("FURNI") {
        ([0.70, 0.60, 0.45], 1.0)
    } else {
        ([0.86, 0.85, 0.82], 1.0)
    }
}

/// How an IFC file is read.
#[derive(Debug, Clone)]
pub struct IfcOpts {
    pub spaces: bool,
    pub cut_openings: bool,
    pub properties: bool,
}

impl Default for IfcOpts {
    fn default() -> Self {
        IfcOpts { spaces: false, cut_openings: true, properties: true }
    }
}

/// Where an IFC file says it lies.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Georef {
    /// From IfcMapConversion: east, north, height, the turn of the x axis in radians
    /// counterclockwise from east, and the scale.
    pub map: Option<([f64; 3], f64, f64)>,
    pub crs: Option<u32>,
    /// From IfcSite: latitude and longitude, when there is no map conversion.
    pub lat_lon: Option<(f64, f64)>,
    /// The turn of true north from the y axis of the model, in radians counterclockwise.
    pub true_north: f64,
}

/// An IFC file read: the model in metres in its own coordinates, its products as features,
/// and where it says it lies.
pub struct IfcModel {
    pub table: Table,
    pub georef: Georef,
}

fn epsg_in(text: &str) -> Option<u32> {
    let t = text.to_uppercase();
    let digits: String = t.trim_start_matches(|c: char| !c.is_ascii_digit()).chars().take_while(|c| c.is_ascii_digit()).collect();
    if t.contains("EPSG") || t.chars().all(|c| c.is_ascii_digit()) {
        digits.parse().ok()
    } else {
        None
    }
}

fn compound_angle(v: Option<&V>) -> Option<f64> {
    let l = v?.list();
    if l.is_empty() {
        return None;
    }
    let n = |k: usize| l.get(k).and_then(|x| x.num()).unwrap_or(0.0);
    let deg = n(0);
    let sign = if deg < 0.0 || n(1) < 0.0 { -1.0 } else { 1.0 };
    Some(sign * (deg.abs() + n(1).abs() / 60.0 + n(2).abs() / 3600.0 + n(3).abs() / 3.6e9))
}

/// Reads an IFC file.
/// Reads an IFC file. `tell` says how it goes as it goes: how many entities there are, and
/// each element that takes long, so that the log shows what a slow file is busy with.
pub fn read_ifc(path: &Path, opts: &IfcOpts, notes: &mut Notes, going: &dyn Fn(u64, Option<u64>) -> bool, tell: &dyn Fn(&str)) -> Result<IfcModel> {
    let started = std::time::Instant::now();
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let step = parse(&bytes)?;
    drop(bytes);
    tell(&format!("{} entities read in {:.1} s", crate::model::fmt_count(step.ents.len() as u64), started.elapsed().as_secs_f64()));
    from_step(&step, opts, notes, going, tell)
}

pub(crate) fn from_step(step: &Step, opts: &IfcOpts, notes: &mut Notes, going: &dyn Fn(u64, Option<u64>) -> bool, tell: &dyn Fn(&str)) -> Result<IfcModel> {
    let s = step;
    // The units of lengths and angles.
    let mut length = 1.0;
    let mut angle = 1.0;
    // The units of the project: those its unit assignment names, not those of a coordinate
    // system or a quantity elsewhere.
    let assigned: Vec<u64> =
        s.ents.values().filter(|e| e.ty == "IFCUNITASSIGNMENT").flat_map(|e| e.args.first().map(|v| v.refs()).unwrap_or_default()).collect();
    // A file without one: whatever units it has.
    let assigned: Vec<u64> = if assigned.is_empty() {
        s.ents.iter().filter(|(_, e)| e.ty == "IFCSIUNIT" || e.ty == "IFCCONVERSIONBASEDUNIT").map(|(id, _)| *id).collect()
    } else {
        assigned
    };
    for e in assigned.iter().filter_map(|id| s.get(*id)) {
        match e.ty.as_str() {
            "IFCSIUNIT" => {
                let kind = e.args.get(1).and_then(|v| v.text()).unwrap_or_default();
                let prefix = e.args.get(2).and_then(|v| v.text()).unwrap_or_default();
                let k = match prefix.as_str() {
                    "MILLI" => 1e-3,
                    "CENTI" => 1e-2,
                    "DECI" => 1e-1,
                    "KILO" => 1e3,
                    _ => 1.0,
                };
                if kind == "LENGTHUNIT" {
                    length = k;
                }
            }
            "IFCCONVERSIONBASEDUNIT" => {
                let kind = e.args.get(1).and_then(|v| v.text()).unwrap_or_default();
                let factor = e.args.get(3).and_then(|v| v.r()).and_then(|m| s.arg(m, 0)).and_then(|v| v.num());
                match (kind.as_str(), factor) {
                    ("LENGTHUNIT", Some(f)) => length = f,
                    ("PLANEANGLEUNIT", Some(f)) => angle = f,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // Colours: styled items, through presentation style assignments and surface styles.
    let colour_of_style = |style: u64| -> Option<Colour> {
        let mut todo = vec![style];
        let mut seen = 0;
        while let Some(id) = todo.pop() {
            seen += 1;
            if seen > 50 {
                break;
            }
            let e = s.get(id)?;
            match e.ty.as_str() {
                "IFCPRESENTATIONSTYLEASSIGNMENT" => todo.extend(e.args.first().map(|v| v.refs()).unwrap_or_default()),
                "IFCSURFACESTYLE" => todo.extend(e.args.get(2).map(|v| v.refs()).unwrap_or_default()),
                "IFCSURFACESTYLERENDERING" | "IFCSURFACESTYLESHADING" => {
                    let c = e.args.first().and_then(|v| v.r())?;
                    let rgb = s.get(c)?;
                    let n = |k: usize| rgb.args.get(k).and_then(|v| v.num()).unwrap_or(0.8) as f32;
                    let transparency = e.args.get(1).and_then(|v| v.num()).unwrap_or(0.0) as f32;
                    return Some(([n(1), n(2), n(3)], (1.0 - transparency).clamp(0.05, 1.0)));
                }
                _ => {}
            }
        }
        None
    };
    let mut styles = HashMap::new();
    let mut material_colours: HashMap<u64, Colour> = HashMap::new();
    for e in s.ents.values() {
        if e.ty == "IFCSTYLEDITEM" {
            let found = e.args.get(1).map(|v| v.refs()).unwrap_or_default().into_iter().find_map(colour_of_style);
            if let (Some(item), Some(c)) = (e.args.first().and_then(|v| v.r()), found) {
                styles.insert(item, c);
            }
        }
    }
    // The colours of materials: IfcMaterialDefinitionRepresentation → styled items.
    for e in s.ents.values() {
        if e.ty == "IFCMATERIALDEFINITIONREPRESENTATION" {
            let Some(mat) = e.args.get(3).and_then(|v| v.r()) else { continue };
            'reps: for rep in e.args.get(2).map(|v| v.refs()).unwrap_or_default() {
                for it in s.arg(rep, 3).map(|v| v.refs()).unwrap_or_default() {
                    if let Some(c) = s
                        .get(it)
                        .filter(|x| x.ty == "IFCSTYLEDITEM")
                        .and_then(|x| x.args.get(1))
                        .map(|v| v.refs())
                        .unwrap_or_default()
                        .into_iter()
                        .find_map(colour_of_style)
                    {
                        material_colours.insert(mat, c);
                        break 'reps;
                    }
                }
            }
        }
    }
    let ifc =
        Ifc { s, placements: Default::default(), angle, styles, warned: Default::default(), tolerance: 0.002 / length, scale_now: std::cell::Cell::new(1.0) };

    // Relations: storey, type, material, properties, openings and parts.
    let mut storey_of: HashMap<u64, String> = HashMap::new();
    let mut parent_of: HashMap<u64, u64> = HashMap::new();
    let mut type_of: HashMap<u64, u64> = HashMap::new();
    let mut material_of: HashMap<u64, u64> = HashMap::new();
    let mut psets_of: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut openings_of: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut filled: HashSet<u64> = HashSet::new();
    for e in s.ents.values() {
        let refs = |k: usize| e.args.get(k).map(|v| v.refs()).unwrap_or_default();
        let one = |k: usize| e.args.get(k).and_then(|v| v.r());
        match e.ty.as_str() {
            "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                if let Some(st) = one(5) {
                    let name = s.arg(st, 2).and_then(|v| v.text()).unwrap_or_default();
                    for x in refs(4) {
                        storey_of.insert(x, name.clone());
                    }
                }
            }
            "IFCRELAGGREGATES" | "IFCRELNESTS" => {
                if let Some(p) = one(4) {
                    for x in refs(5) {
                        parent_of.insert(x, p);
                    }
                }
            }
            "IFCRELDEFINESBYTYPE" => {
                if let Some(t) = one(5) {
                    for x in refs(4) {
                        type_of.insert(x, t);
                    }
                }
            }
            "IFCRELASSOCIATESMATERIAL" => {
                if let Some(m) = one(5) {
                    for x in refs(4) {
                        material_of.insert(x, m);
                    }
                }
            }
            "IFCRELDEFINESBYPROPERTIES" => {
                if let Some(p) = one(5) {
                    for x in refs(4) {
                        psets_of.entry(x).or_default().push(p);
                    }
                }
            }
            "IFCRELVOIDSELEMENT" => {
                if let (Some(host), Some(op)) = (one(4), one(5)) {
                    openings_of.entry(host).or_default().push(op);
                }
            }
            "IFCRELFILLSELEMENT" => {
                if let Some(op) = one(4) {
                    filled.insert(op);
                }
            }
            _ => {}
        }
    }
    // The first material of a material set, layer set or usage, by its name and colour.
    let material = |m: u64| -> Option<(String, Option<Colour>)> {
        let mut id = m;
        for _ in 0..6 {
            let e = s.get(id)?;
            match e.ty.as_str() {
                "IFCMATERIAL" => return Some((e.args.first().and_then(|v| v.text()).unwrap_or_default(), material_colours.get(&id).copied())),
                "IFCMATERIALLAYERSETUSAGE" | "IFCMATERIALPROFILESETUSAGE" => id = e.args.first().and_then(|v| v.r())?,
                "IFCMATERIALLAYERSET" | "IFCMATERIALPROFILESET" | "IFCMATERIALCONSTITUENTSET" => id = *e.args.first()?.refs().first()?,
                "IFCMATERIALLIST" => id = *e.args.first()?.refs().first()?,
                "IFCMATERIALLAYER" | "IFCMATERIALPROFILE" | "IFCMATERIALCONSTITUENT" => {
                    id = e.args.iter().find_map(|v| v.r().filter(|r| s.ty(*r) == "IFCMATERIAL"))?;
                }
                _ => return None,
            }
        }
        None
    };
    // The properties of a property set or quantities, as (name, value).
    let props = |p: u64| -> Vec<(String, Value)> {
        let Some(e) = s.get(p) else { return Vec::new() };
        let set = e.args.get(2).and_then(|v| v.text()).unwrap_or_default();
        let value = |v: &V| -> Value {
            match v {
                V::Typed(_, inner) => match inner.as_ref() {
                    V::Int(i) => Value::Int(*i),
                    V::Real(r) => Value::Real(*r),
                    V::Str(t) => Value::Text(t.clone()),
                    V::Enum(t) if t == "T" => Value::Bool(true),
                    V::Enum(t) if t == "F" => Value::Bool(false),
                    V::Enum(t) => Value::Text(t.clone()),
                    _ => Value::Null,
                },
                V::Str(t) => Value::Text(t.clone()),
                V::Int(i) => Value::Int(*i),
                V::Real(r) => Value::Real(*r),
                _ => Value::Null,
            }
        };
        let mut out = Vec::new();
        let list = match e.ty.as_str() {
            "IFCPROPERTYSET" => e.args.get(4),
            "IFCELEMENTQUANTITY" => e.args.get(5),
            _ => None,
        };
        for q in list.map(|v| v.refs()).unwrap_or_default() {
            let Some(qe) = s.get(q) else { continue };
            let name = qe.args.first().and_then(|v| v.text()).unwrap_or_default();
            let v = match qe.ty.as_str() {
                "IFCPROPERTYSINGLEVALUE" => qe.args.get(2).map(value).unwrap_or(Value::Null),
                "IFCPROPERTYENUMERATEDVALUE" | "IFCPROPERTYLISTVALUE" => {
                    let texts: Vec<String> = qe.args.get(2).map(|v| v.list().iter().map(|x| value(x).to_text()).collect()).unwrap_or_default();
                    if texts.is_empty() {
                        Value::Null
                    } else {
                        Value::Text(texts.join(", "))
                    }
                }
                t if t.starts_with("IFCQUANTITY") => qe.args.get(3).and_then(|v| v.num()).map_or(Value::Null, Value::Real),
                _ => continue,
            };
            if !v.is_null() {
                out.push((format!("{}.{}", set, name), v));
            }
        }
        out
    };

    // The products with a shape.
    let mut ids: Vec<u64> = s
        .ents
        .iter()
        .filter(|(_, e)| e.args.len() > 6 && e.args.get(6).and_then(|v| v.r()).is_some_and(|r| s.ty(r) == "IFCPRODUCTDEFINITIONSHAPE"))
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    let openings: HashSet<u64> = openings_of.values().flatten().copied().collect();
    let total = ids.len() as u64;
    let mut mesh = Mesh::default();
    let mut material_index: HashMap<String, u32> = HashMap::new();
    let mut features = Vec::new();
    let mut pset_fields: Vec<String> = Vec::new();
    let mut pset_values: Vec<HashMap<String, Value>> = Vec::new();
    let mut skipped_spaces = 0usize;
    let mut cut = 0usize;
    tell(&format!("{} elements with a shape", crate::model::fmt_count(total)));
    // The element before, and when it began: one that takes long is told of, and the
    // slowest are named at the end.
    let mut before: Option<(u64, std::time::Instant)> = None;
    let mut slow: Vec<(f64, u64)> = Vec::new();
    let name_of = |id: u64| -> String {
        let e = &s.ents[&id];
        let name = e.args.get(2).and_then(|v| v.text()).filter(|t| !t.trim().is_empty());
        match name {
            Some(n) => format!("{} #{} ({})", class_name(&e.ty), id, n),
            None => format!("{} #{}", class_name(&e.ty), id),
        }
    };
    let took = |before: Option<(u64, std::time::Instant)>, slow: &mut Vec<(f64, u64)>| {
        if let Some((prev, t)) = before {
            let secs = t.elapsed().as_secs_f64();
            if secs >= 2.0 {
                tell(&format!("{} took {:.0} s", name_of(prev), secs));
                slow.push((secs, prev));
            }
        }
    };
    for (n, id) in ids.iter().enumerate() {
        took(before.take(), &mut slow);
        before = Some((*id, std::time::Instant::now()));
        if !going(n as u64, Some(total)) {
            return err(crate::model::STOPPED);
        }
        let e = &s.ents[id];
        if e.ty.starts_with("IFCOPENING") || openings.contains(id) || e.ty == "IFCANNOTATION" || e.ty == "IFCGRID" || e.ty == "IFCVIRTUALELEMENT" {
            continue;
        }
        if matches!(e.ty.as_str(), "IFCSPACE" | "IFCSPATIALZONE" | "IFCZONE" | "IFCEXTERNALSPATIALELEMENT") && !opts.spaces {
            skipped_spaces += 1;
            continue;
        }
        let class = class_name(&e.ty);
        let tf = e.args.get(5).and_then(|v| v.r()).map_or(Tf::ID, |p| ifc.placement(p));
        // The body, or else whatever shape it has.
        let reps = s.arg(e.args[6].r().unwrap_or(0), 2).map(|v| v.refs()).unwrap_or_default();
        let ident = |r: &u64| s.arg(*r, 1).and_then(|v| v.text()).unwrap_or_default();
        let body: Vec<u64> = {
            let b: Vec<u64> = reps.iter().copied().filter(|r| matches!(ident(r).as_str(), "Body" | "Facetation" | "Mesh")).collect();
            if b.is_empty() {
                reps.iter()
                    .copied()
                    .filter(|r| !matches!(ident(r).as_str(), "Axis" | "FootPrint" | "Box" | "Annotation" | "Profile" | "Clearance" | "Reference"))
                    .collect()
            } else {
                b
            }
        };
        let mat = material_of.get(id).or_else(|| type_of.get(id).and_then(|t| material_of.get(t))).and_then(|m| material(*m));
        let fallback = mat.as_ref().and_then(|m| m.1).unwrap_or_else(|| class_colour(&class));
        let mut parts = Vec::new();
        for r in &body {
            for it in s.arg(*r, 3).map(|v| v.refs()).unwrap_or_default() {
                ifc.item(it, &tf, None, &mut parts, notes, 0);
            }
        }
        if parts.iter().all(|(_, p)| p.tris.is_empty()) {
            continue;
        }
        // Openings, cut out of the walls and slabs they lie in.
        if opts.cut_openings {
            for op in openings_of.get(id).into_iter().flatten() {
                let Some(oe) = s.get(*op) else { continue };
                let otf = oe.args.get(5).and_then(|v| v.r()).map_or(Tf::ID, |p| ifc.placement(p));
                let mut solids = Vec::new();
                for r in s.arg(oe.args.get(6).and_then(|v| v.r()).unwrap_or(0), 2).map(|v| v.refs()).unwrap_or_default() {
                    for it in s.arg(r, 3).map(|v| v.refs()).unwrap_or_default() {
                        ifc.item(it, &otf, None, &mut solids, notes, 0);
                    }
                }
                for (_, solid) in solids {
                    // An opening is taken through its wall when it stops short of a face of it by
                    // up to 10 mm, as openings exported from Revit often do.
                    if cut_out(&mut parts, &solid.tris, ifc.tolerance * 5.0) {
                        cut += 1;
                    }
                }
            }
        }
        let object = mesh.objects.len() as u32;
        let guid = e.args.first().and_then(|v| v.text()).unwrap_or_default();
        let name = e.args.get(2).and_then(|v| v.text()).unwrap_or_default();
        mesh.objects.push(if name.is_empty() { guid.clone() } else { name.clone() });
        let mut polys = Vec::new();
        let mut zs = Vec::new();
        let (mut zlo, mut zhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (colour, part) in parts {
            let (rgb, opacity) = colour.unwrap_or(fallback);
            let key = format!("{}|{:.3},{:.3},{:.3}|{:.2}", class, rgb[0], rgb[1], rgb[2], opacity);
            let mi = *material_index.entry(key).or_insert_with(|| {
                let glass = opacity < 0.98;
                let material_name = format!("{}{}", class, if glass { " glass" } else { "" });
                mesh.materials.push(Material {
                    name: material_name,
                    diffuse: rgb,
                    opacity,
                    specular: if glass { [0.6; 3] } else { [0.04; 3] },
                    shininess: if glass { 200.0 } else { 20.0 },
                    ..Default::default()
                });
                (mesh.materials.len() - 1) as u32
            });
            for t in part.tris {
                let t = t.map(|p| p.map(|c| c * length));
                let area = cross(sub(t[1], t[0]), sub(t[2], t[0]));
                if dot(area, area) < 1e-14 {
                    continue;
                }
                let v = mesh.positions.len() as u32;
                mesh.positions.extend_from_slice(&t);
                mesh.tris.push(Tri { v: [v, v + 1, v + 2], n: [NONE; 3], t: [NONE; 3], material: mi, object });
                let ring: Vec<geo::Coord<f64>> = [t[0], t[1], t[2], t[0]].iter().map(|p| geo::coord! { x: p[0], y: p[1] }).collect();
                polys.push(geo::Polygon::new(geo::LineString::new(ring), Vec::new()));
                for p in [t[0], t[1], t[2], t[0]] {
                    zs.push(p[2]);
                    zlo = zlo.min(p[2]);
                    zhi = zhi.max(p[2]);
                }
            }
        }
        // The storey: its own, or that of what it is a part of.
        let mut storey = storey_of.get(id).cloned();
        let mut up = *id;
        for _ in 0..8 {
            if storey.is_some() {
                break;
            }
            match parent_of.get(&up) {
                Some(p) => {
                    storey = storey_of
                        .get(p)
                        .cloned()
                        .or_else(|| (s.ty(*p) == "IFCBUILDINGSTOREY").then(|| s.arg(*p, 2).and_then(|v| v.text()).unwrap_or_default()));
                    up = *p;
                }
                None => break,
            }
        }
        let type_name = type_of.get(id).and_then(|t| s.arg(*t, 2)).and_then(|v| v.text());
        let text = |k: usize| e.args.get(k).and_then(|v| v.text()).map_or(Value::Null, Value::Text);
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        let attrs = vec![
            Value::Text(guid),
            Value::Text(class.clone()),
            if name.is_empty() { Value::Null } else { Value::Text(name) },
            text(3),
            text(4),
            if e.args.len() > 7 && class != "IfcSpace" { text(7) } else { Value::Null },
            storey.map_or(Value::Null, Value::Text),
            type_name.map_or(Value::Null, Value::Text),
            mat.map_or(Value::Null, |m| Value::Text(m.0)),
            if zlo.is_finite() { Value::Real(round(zlo)) } else { Value::Null },
            if zhi.is_finite() { Value::Real(round(zhi)) } else { Value::Null },
            if zlo.is_finite() { Value::Real(round(zhi - zlo)) } else { Value::Null },
        ];
        // Property sets of the element, and of its type.
        let mut own: HashMap<String, Value> = HashMap::new();
        if opts.properties {
            let from_type: Vec<u64> = type_of.get(id).and_then(|t| s.arg(*t, 5)).map(|v| v.refs()).unwrap_or_default();
            for p in from_type.iter().chain(psets_of.get(id).into_iter().flatten()) {
                for (k, v) in props(*p) {
                    if !own.contains_key(&k) && !pset_fields.contains(&k) {
                        pset_fields.push(k.clone());
                    }
                    own.insert(k, v);
                }
            }
        }
        pset_values.push(own);
        let geom = (!polys.is_empty()).then(|| geo::Geometry::MultiPolygon(geo::MultiPolygon::new(polys)));
        features.push(Feature { attrs, geom, z: (!zs.is_empty()).then(|| Arc::from(zs)) });
    }
    took(before.take(), &mut slow);
    if !slow.is_empty() {
        slow.sort_by(|a, b| b.0.total_cmp(&a.0));
        let named: Vec<String> = slow.iter().take(3).map(|(t, id)| format!("{} {:.0} s", name_of(*id), t)).collect();
        notes.info(format!("the slowest elements: {}", named.join(", ")));
    }
    if features.is_empty() {
        return err("The IFC file has no elements with a shape that can be read.");
    }
    // The fields, with the properties after the attributes every element has.
    let mut fields = vec![
        Field::new("GlobalId", FieldType::Text),
        Field::new("IfcClass", FieldType::Text),
        Field::new("Name", FieldType::Text),
        Field::new("Description", FieldType::Text),
        Field::new("ObjectType", FieldType::Text),
        Field::new("Tag", FieldType::Text),
        Field::new("storey", FieldType::Text),
        Field::new("type", FieldType::Text),
        Field::new("material", FieldType::Text),
        Field::new("z_min", FieldType::Real),
        Field::new("z_max", FieldType::Real),
        Field::new("height", FieldType::Real),
    ];
    pset_fields.sort();
    for k in &pset_fields {
        let ty = crate::model::infer_type(pset_values.iter().filter_map(|m| m.get(k)));
        fields.push(Field::new(k.clone(), ty));
    }
    for (f, own) in features.iter_mut().zip(pset_values) {
        for k in &pset_fields {
            f.attrs.push(own.get(k).cloned().unwrap_or(Value::Null));
        }
    }
    // Where the model lies.
    let mut georef = Georef::default();
    for e in s.ents.values() {
        if e.ty == "IFCMAPCONVERSION" || e.ty == "IFCMAPCONVERSIONSCALED" {
            let n = |k: usize| e.args.get(k).and_then(|v| v.num());
            let (a, b) = (n(5).unwrap_or(1.0), n(6).unwrap_or(0.0));
            georef.map = Some(([n(2).unwrap_or(0.0), n(3).unwrap_or(0.0), n(4).unwrap_or(0.0)], b.atan2(a), n(7).unwrap_or(1.0)));
            if let Some(crs) = e.args.get(1).and_then(|v| v.r()) {
                georef.crs = s.arg(crs, 0).and_then(|v| v.text()).and_then(|t| epsg_in(&t));
            }
        }
    }
    if georef.map.is_none() {
        // IFC2x3: the same in property sets of the project.
        let mut values: HashMap<String, f64> = HashMap::new();
        let mut crs_name = None;
        for e in s.ents.values().filter(|e| e.ty == "IFCPROPERTYSET") {
            let set = e.args.get(2).and_then(|v| v.text()).unwrap_or_default();
            if set == "ePSet_MapConversion" || set == "ePSet_ProjectedCRS" {
                for q in e.args.get(4).map(|v| v.refs()).unwrap_or_default() {
                    let name = s.arg(q, 0).and_then(|v| v.text()).unwrap_or_default();
                    match s.arg(q, 2) {
                        Some(v) if v.num().is_some() => {
                            values.insert(name, v.num().unwrap_or(0.0));
                        }
                        Some(v) if name == "Name" => crs_name = v.text(),
                        _ => {}
                    }
                }
            }
        }
        if values.contains_key("Eastings") {
            let g = |k: &str| values.get(k).copied();
            georef.map = Some((
                [g("Eastings").unwrap_or(0.0), g("Northings").unwrap_or(0.0), g("OrthogonalHeight").unwrap_or(0.0)],
                g("XAxisOrdinate").unwrap_or(0.0).atan2(g("XAxisAbscissa").unwrap_or(1.0)),
                g("Scale").unwrap_or(1.0),
            ));
            georef.crs = crs_name.and_then(|t| epsg_in(&t));
        }
    }
    if let Some(site) = s.ents.values().find(|e| e.ty == "IFCSITE") {
        if let (Some(lat), Some(lon)) = (compound_angle(site.args.get(9)), compound_angle(site.args.get(10))) {
            if lat != 0.0 || lon != 0.0 {
                georef.lat_lon = Some((lat, lon));
            }
        }
    }
    for e in s.ents.values().filter(|e| e.ty == "IFCGEOMETRICREPRESENTATIONCONTEXT") {
        if let Some(d) = e.args.get(5).and_then(|v| v.r()) {
            let v = ifc.direction(Some(d), [0.0, 1.0, 0.0]);
            // The angle of north from the y axis, counterclockwise.
            georef.true_north = (-v[0]).atan2(v[1]);
        }
    }
    let classes: HashSet<&str> = features.iter().filter_map(|f| if let Value::Text(t) = &f.attrs[1] { Some(t.as_str()) } else { None }).collect();
    notes.info(format!(
        "{} elements of {} kinds, {} triangles{}{}, {}",
        fmt_count(features.len() as u64),
        classes.len(),
        fmt_count(mesh.tris.len() as u64),
        if cut > 0 { format!(", {} openings cut", fmt_count(cut as u64)) } else { String::new() },
        if skipped_spaces > 0 { format!(", {} spaces left out", fmt_count(skipped_spaces as u64)) } else { String::new() },
        if s.schema.is_empty() { "IFC".to_string() } else { s.schema.clone() }
    ));
    let table = Table { fields, crs: None, features, raster: false, image: None, grid: None, mesh: Some(Arc::new(mesh)) };
    Ok(IfcModel { table, georef })
}

/// Moves a model read from IFC to where it lies: by its map conversion, or by the origin,
/// turn and coordinate system given, as for OBJ.
pub fn place(model: &mut IfcModel, origin: Option<([f64; 3], f64)>) {
    let (o, turn, scale) = match (origin, &model.georef.map) {
        (Some((o, clockwise_degrees)), _) => (o, -clockwise_degrees.to_radians(), 1.0),
        (None, Some((o, theta, scale))) => (*o, *theta, if (*scale - 1.0).abs() > 1e-9 && *scale > 0.01 && *scale < 100.0 { *scale } else { 1.0 }),
        (None, None) => return,
    };
    let (sn, cs) = turn.sin_cos();
    let at = |p: [f64; 3]| [o[0] + scale * (cs * p[0] - sn * p[1]), o[1] + scale * (sn * p[0] + cs * p[1]), o[2] + p[2]];
    let t = &mut model.table;
    if let Some(m) = t.mesh.as_mut() {
        let m = Arc::make_mut(m);
        for p in &mut m.positions {
            *p = at(*p);
        }
    }
    for f in &mut t.features {
        if let Some(g) = f.geom.as_mut() {
            crate::geom::transform_all(std::iter::once(g), |xs, ys| {
                for (x, y) in xs.iter_mut().zip(ys.iter_mut()) {
                    let q = at([*x, *y, 0.0]);
                    *x = q[0];
                    *y = q[1];
                }
                Ok(())
            })
            .ok();
        }
        if let Some(z) = f.z.as_mut() {
            *z = z.iter().map(|v| v + o[2]).collect::<Vec<_>>().into();
        }
    }
    let i = t.field_index("z_min").unwrap_or(0);
    for f in &mut t.features {
        for k in [i, i + 1] {
            if let Value::Real(v) = f.attrs[k] {
                f.attrs[k] = Value::Real(((v + o[2]) * 1000.0).round() / 1000.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = r"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView]'),'2;1');
FILE_NAME('wall.ifc','2026-10-22',(''),(''),'','','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCCARTESIANPOINT((0.,0.,0.));
#3=IFCAXIS2PLACEMENT3D(#2,$,$);
#4=IFCLOCALPLACEMENT($,#3);
#5=IFCCARTESIANPOINT((1000.,2000.,0.));
#6=IFCAXIS2PLACEMENT3D(#5,$,$);
#7=IFCLOCALPLACEMENT(#4,#6);
/* a wall 5 m long, 200 mm thick and 3 m high, along x */
#10=IFCRECTANGLEPROFILEDEF(.AREA.,'w',#11,5000.,200.);
#11=IFCAXIS2PLACEMENT2D(#12,$);
#12=IFCCARTESIANPOINT((2500.,100.));
#13=IFCDIRECTION((0.,0.,1.));
#14=IFCEXTRUDEDAREASOLID(#10,#3,#13,3000.);
#15=IFCSHAPEREPRESENTATION(#99,'Body','SweptSolid',(#14));
#16=IFCPRODUCTDEFINITIONSHAPE($,$,(#15));
#20=IFCWALL('2O2Fr$t4X7Zf8NOew3FLOH',$,'V\X2\00E4\X0\gg A','A wall',$,#7,#16,'W-1',$);
/* a window opening 1 m wide and 1.2 m high, through the wall */
#30=IFCRECTANGLEPROFILEDEF(.AREA.,'o',#31,1000.,400.);
#31=IFCAXIS2PLACEMENT2D(#32,$);
#32=IFCCARTESIANPOINT((2500.,100.));
#33=IFCCARTESIANPOINT((0.,0.,900.));
#34=IFCAXIS2PLACEMENT3D(#33,$,$);
#35=IFCEXTRUDEDAREASOLID(#30,#34,#13,1200.);
#36=IFCSHAPEREPRESENTATION(#99,'Body','SweptSolid',(#35));
#37=IFCPRODUCTDEFINITIONSHAPE($,$,(#36));
#38=IFCOPENINGELEMENT('1O2Fr$t4X7Zf8NOew3FLOH',$,'Opening',$,$,#7,#37,$,.OPENING.);
#39=IFCRELVOIDSELEMENT('3O2Fr$t4X7Zf8NOew3FLOH',$,$,$,#20,#38);
#40=IFCBUILDINGSTOREY('4O2Fr$t4X7Zf8NOew3FLOH',$,'Plan 1',$,$,#4,$,$,.ELEMENT.,0.);
#41=IFCRELCONTAINEDINSPATIALSTRUCTURE('5O2Fr$t4X7Zf8NOew3FLOH',$,$,$,(#20),#40);
#50=IFCPROPERTYSINGLEVALUE('IsExternal',$,IFCBOOLEAN(.T.),$);
#51=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('EI 60'),$);
#52=IFCPROPERTYSET('6O2Fr$t4X7Zf8NOew3FLOH',$,'Pset_WallCommon',$,(#50,#51));
#53=IFCRELDEFINESBYPROPERTIES('7O2Fr$t4X7Zf8NOew3FLOH',$,$,$,(#20),#52);
#60=IFCCOLOURRGB($,0.8,0.2,0.1);
#61=IFCSURFACESTYLERENDERING(#60,0.,$,$,$,$,$,$,.NOTDEFINED.);
#62=IFCSURFACESTYLE('red',.BOTH.,(#61));
#63=IFCSTYLEDITEM(#14,(#62),$);
#70=IFCPROJECTEDCRS('EPSG:3006',$,$,$,$,$,$);
#71=IFCMAPCONVERSION(#99,#70,319500.,6399800.,52.5,0.,1.,$);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn a_wall_with_an_opening_its_properties_and_its_place() {
        let step = parse(SMALL.as_bytes()).unwrap();
        assert_eq!(step.schema, "IFC4");
        assert_eq!(step.get(20).unwrap().args[2], V::Str("Vägg A".into()));
        let mut notes = Notes::default();
        let mut m = from_step(&step, &IfcOpts::default(), &mut notes, &|_, _| true, &|_| {}).unwrap();
        let t = &m.table;
        assert_eq!(t.len(), 1, "the opening is not an element of its own");
        let col = |name: &str| t.features[0].attrs[t.field_index(name).unwrap_or_else(|| panic!("no {}", name))].clone();
        assert_eq!(col("IfcClass"), Value::Text("IfcWall".into()));
        assert_eq!(col("Name"), Value::Text("Vägg A".into()));
        assert_eq!(col("Tag"), Value::Text("W-1".into()));
        assert_eq!(col("storey"), Value::Text("Plan 1".into()));
        assert_eq!(col("Pset_WallCommon.IsExternal"), Value::Bool(true));
        assert_eq!(col("Pset_WallCommon.FireRating"), Value::Text("EI 60".into()));
        assert_eq!(col("height"), Value::Real(3.0));
        let mesh = t.mesh.as_ref().unwrap();
        let (lo, hi) = mesh.bounds().unwrap();
        // In metres, placed at 1, 2 by its placement.
        assert!((lo[0] - 1.0).abs() < 1e-9 && (hi[0] - 6.0).abs() < 1e-9 && (lo[1] - 2.0).abs() < 1e-9 && (hi[1] - 2.2).abs() < 1e-9, "{:?} {:?}", lo, hi);
        // The opening is cut: the area of the faces facing south is the wall less the hole.
        let south: f64 = mesh
            .tris
            .iter()
            .map(|tr| tr.v.map(|i| mesh.positions[i as usize]))
            .filter(|p| p.iter().all(|q| (q[1] - 2.0).abs() < 1e-9))
            .map(|p| {
                let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                dot(c, c).sqrt() / 2.0
            })
            .sum();
        assert!((south - (5.0 * 3.0 - 1.0 * 1.2)).abs() < 1e-6, "{}", south);
        assert!(notes.info.iter().any(|i| i.contains("1 openings cut")), "{:?}", notes.info);
        // Its colour from the style.
        assert!(mesh.materials.iter().any(|x| (x.diffuse[0] - 0.8).abs() < 1e-6 && x.name == "IfcWall"));
        // Where it lies: the map conversion, in SWEREF 99 TM, turned a quarter.
        assert_eq!(m.georef.crs, Some(3006));
        place(&mut m, None);
        let (lo, hi) = m.table.mesh.as_ref().unwrap().bounds().unwrap();
        // x of the model points north: x 1..6 → north 6399801..6399806; y 2..2.2 → west.
        assert!((lo[1] - 6_399_801.0).abs() < 1e-6 && (hi[1] - 6_399_806.0).abs() < 1e-6, "{:?} {:?}", lo, hi);
        assert!((hi[0] - (319_500.0 - 2.0)).abs() < 1e-6 && (lo[2] - 52.5).abs() < 1e-9, "{:?} {:?}", lo, hi);
        assert_eq!(m.table.features[0].attrs[m.table.field_index("z_min").unwrap()], Value::Real(52.5));
    }

    #[test]
    fn an_advanced_brep_of_a_cylinder_is_closed_and_faces_out() {
        // A cylinder of radius 1 and height 2, as CAD writes it: a side on a cylindrical
        // surface with a seam, and two ends on planes, bounded by circles.
        let text = "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCCARTESIANPOINT((0.,0.,0.));
#2=IFCCARTESIANPOINT((0.,0.,2.));
#3=IFCCARTESIANPOINT((1.,0.,0.));
#4=IFCCARTESIANPOINT((1.,0.,2.));
#5=IFCDIRECTION((0.,0.,1.));
#6=IFCDIRECTION((1.,0.,0.));
#7=IFCAXIS2PLACEMENT3D(#1,#5,#6);
#8=IFCAXIS2PLACEMENT3D(#2,#5,#6);
#10=IFCVERTEXPOINT(#3);
#11=IFCVERTEXPOINT(#4);
#12=IFCCIRCLE(#7,1.);
#13=IFCCIRCLE(#8,1.);
#14=IFCVECTOR(#5,1.);
#15=IFCLINE(#3,#14);
#20=IFCEDGECURVE(#10,#10,#12,.T.);
#21=IFCEDGECURVE(#11,#11,#13,.T.);
#22=IFCEDGECURVE(#10,#11,#15,.T.);
#30=IFCEDGELOOP((#31,#32,#33,#34));
#31=IFCORIENTEDEDGE(*,*,#20,.T.);
#32=IFCORIENTEDEDGE(*,*,#22,.T.);
#33=IFCORIENTEDEDGE(*,*,#21,.F.);
#34=IFCORIENTEDEDGE(*,*,#22,.F.);
#35=IFCFACEOUTERBOUND(#30,.T.);
#36=IFCCYLINDRICALSURFACE(#7,1.);
#37=IFCADVANCEDFACE((#35),#36,.T.);
#40=IFCEDGELOOP((#41));
#41=IFCORIENTEDEDGE(*,*,#20,.F.);
#42=IFCFACEOUTERBOUND(#40,.T.);
#43=IFCPLANE(#7);
#44=IFCADVANCEDFACE((#42),#43,.F.);
#50=IFCEDGELOOP((#51));
#51=IFCORIENTEDEDGE(*,*,#21,.T.);
#52=IFCFACEOUTERBOUND(#50,.T.);
#53=IFCPLANE(#8);
#54=IFCADVANCEDFACE((#52),#53,.T.);
#60=IFCCLOSEDSHELL((#37,#44,#54));
#61=IFCADVANCEDBREP(#60);
ENDSEC;END-ISO-10303-21;";
        let step = parse(text.as_bytes()).unwrap();
        let ifc = Ifc {
            s: &step,
            placements: Default::default(),
            angle: 1.0,
            styles: HashMap::new(),
            warned: Default::default(),
            tolerance: 0.002,
            scale_now: std::cell::Cell::new(1.0),
        };
        let mut notes = Notes::default();
        let mut parts = Vec::new();
        ifc.item(61, &Tf::ID, None, &mut parts, &mut notes, 0);
        let tris: Vec<[[f64; 3]; 3]> = parts.into_iter().flat_map(|(_, p)| p.tris).collect();
        assert!(notes.warnings.is_empty(), "{:?}", notes.warnings);
        let area: f64 = tris
            .iter()
            .map(|p| {
                let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                dot(c, c).sqrt() / 2.0
            })
            .sum();
        let volume: f64 = tris.iter().map(|p| dot(p[0], cross(p[1], p[2]))).sum::<f64>() / 6.0;
        let pi = std::f64::consts::PI;
        assert!((area - 6.0 * pi).abs() < 6.0 * pi * 0.01, "{}", area);
        assert!((volume - 2.0 * pi).abs() < 2.0 * pi * 0.01, "{}", volume);
        // Every point on the surface of the cylinder.
        for p in tris.iter().flatten() {
            let r = p[0].hypot(p[1]);
            assert!((p[2].abs() < 1e-9 || (p[2] - 2.0).abs() < 1e-9) && r <= 1.0 + 1e-9 || (r - 1.0).abs() < 1e-9, "{:?}", p);
        }
    }

    #[test]
    fn an_opening_is_closed_with_its_reveals() {
        let area = |t: &[[[f64; 3]; 3]]| {
            t.iter()
                .map(|p| {
                    let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                    dot(c, c).sqrt() / 2.0
                })
                .sum::<f64>()
        };
        let volume = |t: &[[[f64; 3]; 3]]| t.iter().map(|p| dot(p[0], cross(p[1], p[2]))).sum::<f64>() / 6.0;
        // A wall 4 m long, 0.3 m thick and 3 m high, with a window 1 by 1 m through it, from an
        // opening deeper than the wall.
        let wall = extrude(&[vec![[0.0, 0.0], [4.0, 0.0], [4.0, 0.3], [0.0, 0.3]]], [0.0, 0.0, 1.0], 3.0);
        assert!((volume(&wall) - 3.6).abs() < 1e-9, "{}", volume(&wall));
        let hole: Vec<[[f64; 3]; 3]> = extrude(&[vec![[1.0, -0.1], [2.0, -0.1], [2.0, 0.4], [1.0, 0.4]]], [0.0, 0.0, 1.0], 1.0)
            .into_iter()
            .map(|t| t.map(|p| [p[0], p[1], p[2] + 1.0]))
            .collect();
        let mut parts: Parts = vec![(None, Part { tris: wall })];
        assert!(cut_out(&mut parts, &hole, 0.002));
        let t = parts[0].1.tris.clone();
        let t = &t;
        // The faces less two square metres, and four reveals of 0.3 m².
        assert!((area(t) - (2.0 * 12.0 + 2.0 * 0.9 + 2.0 * 1.2 - 2.0 + 4.0 * 0.3)).abs() < 1e-9, "{}", area(t));
        assert!((volume(t) - (3.6 - 0.3)).abs() < 1e-9, "{}", volume(t));
        // A second opening in the same place adds nothing.
        assert!(cut_out(&mut parts, &hole, 0.002));
        assert!((area(&parts[0].1.tris) - area(t)).abs() < 1e-9);
    }

    #[test]
    fn an_opening_of_any_shape_is_cut() {
        let area = |t: &[[[f64; 3]; 3]]| {
            t.iter()
                .map(|p| {
                    let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                    dot(c, c).sqrt() / 2.0
                })
                .sum::<f64>()
        };
        let volume = |t: &[[[f64; 3]; 3]]| t.iter().map(|p| dot(p[0], cross(p[1], p[2]))).sum::<f64>() / 6.0;
        // A slab 10 by 10 m and 0.2 m thick, with a shaft shaped as an L through it.
        let slab = extrude(&[vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]], [0.0, 0.0, 1.0], 0.2);
        let l: Vec<[[f64; 3]; 3]> = extrude(&[vec![[2.0, 2.0], [6.0, 2.0], [6.0, 4.0], [4.0, 4.0], [4.0, 6.0], [2.0, 6.0]]], [0.0, 0.0, 1.0], 1.0)
            .into_iter()
            .map(|t| t.map(|p| [p[0], p[1], p[2] - 0.4]))
            .collect();
        let mut parts: Parts = vec![(None, Part { tris: slab })];
        assert!(cut_out(&mut parts, &l, 0.002));
        let t = &parts[0].1.tris;
        // 12 m² of shaft, with a rim of 16 m by 0.2 m.
        assert!((volume(t) - (20.0 - 12.0 * 0.2)).abs() < 1e-9, "{}", volume(t));
        assert!((area(t) - (2.0 * (100.0 - 12.0) + 40.0 * 0.2 + 16.0 * 0.2)).abs() < 1e-9, "{}", area(t));
    }

    #[test]
    fn a_niche_is_cut_and_closed_within_the_wall() {
        let area = |t: &[[[f64; 3]; 3]]| {
            t.iter()
                .map(|p| {
                    let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                    dot(c, c).sqrt() / 2.0
                })
                .sum::<f64>()
        };
        let volume = |t: &[[[f64; 3]; 3]]| t.iter().map(|p| dot(p[0], cross(p[1], p[2]))).sum::<f64>() / 6.0;
        // An opening that reaches only part of the way into the wall, from outside it.
        let wall = extrude(&[vec![[0.0, 0.0], [4.0, 0.0], [4.0, 0.3], [0.0, 0.3]]], [0.0, 0.0, 1.0], 3.0);
        let niche: Vec<[[f64; 3]; 3]> = extrude(&[vec![[1.0, -0.115], [2.0, -0.115], [2.0, 0.185], [1.0, 0.185]]], [0.0, 0.0, 1.0], 1.0)
            .into_iter()
            .map(|t| t.map(|p| [p[0], p[1], p[2] + 1.0]))
            .collect();
        let mut parts: Parts = vec![(None, Part { tris: wall })];
        assert!(cut_out(&mut parts, &niche, 0.002));
        let t = &parts[0].1.tris;
        for p in t.iter().flatten() {
            assert!(p[1] >= -1e-9 && p[1] <= 0.3 + 1e-9, "{:?}", p);
        }
        // The faces less the mouth of 1 m², four sides 0.185 deep, and the back of 1 m².
        assert!((area(t) - (2.0 * 12.0 + 2.0 * 0.9 + 2.0 * 1.2 - 1.0 + 4.0 * 0.185 + 1.0)).abs() < 1e-9, "{}", area(t));
        assert!((volume(t) - (3.6 - 0.185)).abs() < 1e-9, "{}", volume(t));
    }

    #[test]
    fn an_opening_a_fraction_of_a_millimetre_off_goes_through() {
        let area = |t: &[[[f64; 3]; 3]]| {
            t.iter()
                .map(|p| {
                    let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                    dot(c, c).sqrt() / 2.0
                })
                .sum::<f64>()
        };
        // A door drawn as thick as its wall, 0.4 mm off it.
        let wall = extrude(&[vec![[0.0, 0.0], [4.0, 0.0], [4.0, 0.1], [0.0, 0.1]]], [0.0, 0.0, 1.0], 3.0);
        let door = extrude(&[vec![[1.0, 0.0004], [2.0, 0.0004], [2.0, 0.1004], [1.0, 0.1004]]], [0.0, 0.0, 1.0], 2.0);
        let mut parts: Parts = vec![(None, Part { tris: wall })];
        assert!(cut_out(&mut parts, &door, 0.002));
        let t = &parts[0].1.tris;
        // Both faces have the hole of 2 m²: no skin is left on the side it stopped short of.
        let facing = |y: f64| {
            t.iter()
                .filter(|p| p.iter().all(|q| (q[1] - y).abs() < 1e-9))
                .map(|p| {
                    let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                    dot(c, c).sqrt() / 2.0
                })
                .sum::<f64>()
        };
        assert!((facing(0.0) - 10.0).abs() < 1e-6, "{}", facing(0.0));
        assert!((facing(0.1) - 10.0).abs() < 1e-6, "{}", facing(0.1));
        assert!(area(t) > 0.0);
    }

    #[test]
    fn arcs_and_profiles() {
        let a = arc3([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]);
        assert!(a.iter().all(|p| ((p[0] * p[0] + p[1] * p[1]).sqrt() - 1.0).abs() < 1e-9));
        assert!(a.iter().all(|p| p[1] >= -1e-9), "through the top");
        let tris = extrude(&[vec![[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]], [0.0, 0.0, 1.0], 3.0);
        assert_eq!(tris.len(), 8 + 4);
        // A box cut out of a square face leaves a frame.
        let face = vec![[[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 4.0, 0.0]], [[0.0, 0.0, 0.0], [4.0, 4.0, 0.0], [0.0, 4.0, 0.0]]];
        let boxed = extrude(&[vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]]], [0.0, 0.0, 1.0], 2.0)
            .into_iter()
            .map(|t| t.map(|p| [p[0], p[1], p[2] - 1.0]))
            .collect::<Vec<_>>();
        let planes = planes_of(&boxed);
        assert_eq!(planes.len(), 6);
        let left = subtract_convex(face, &planes);
        let area: f64 = left.iter().map(|p| cross(sub(p[1], p[0]), sub(p[2], p[0]))[2].abs() / 2.0).sum();
        assert!((area - 12.0).abs() < 1e-9, "{}", area);
    }
}
