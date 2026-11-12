//! Gaussian splats: a scene as many small, soft, coloured ellipsoids, each with its middle,
//! size, turn, opacity and colour, as made from photos by 3D Gaussian Splatting (B. Kerbl,
//! G. Kopanas, T. Leimkühler and G. Drettakis, 2023). Read and written as PLY, as the
//! training writes them, and as SPZ, the compressed format of Niantic. In Ographis a scene is
//! a point cloud whose points are the middles of the splats, in the coordinate system of the
//! map, x east, y north and z up, so that it can be placed in SWEREF 99 and shown and
//! clipped with map data. A point cloud from a scanner or a drone can be made splats.

use crate::cloud::Cloud;
use crate::model::{err, Field, FieldType, GbError, Result, Value};
use std::io::{BufRead, Read, Write};
use std::path::Path;

/// The first coefficient of the spherical harmonics, which turns their base into a colour.
const C0: f32 = 0.282_094_8;

/// The splats of a cloud, a value of each for each point.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Splats {
    /// The size along the splat's own three axes, one standard deviation, in metres.
    pub scale: Vec<[f32; 3]>,
    /// How it is turned, as a quaternion w, x, y, z, to the axes of the map.
    pub rot: Vec<[f32; 4]>,
    /// From 0, clear, to 1, solid.
    pub opacity: Vec<f32>,
    /// The colour seen from any side, red, green and blue from 0 to 1 (above 1 for bright).
    pub colour: Vec<[f32; 3]>,
    /// How many bands of spherical harmonics change the colour by the side it is seen from:
    /// 0 to 3.
    pub sh_degree: u8,
    /// Their coefficients after the first: for each splat, for each coefficient, red, green
    /// and blue.
    pub sh: Vec<f32>,
    /// The turn from the axes the coefficients were made in to the axes of the map, as a
    /// quaternion. They are only right in a file with those axes.
    pub sh_frame: [f32; 4],
}

/// The coefficients after the first for a degree.
pub fn sh_count(degree: u8) -> usize {
    match degree {
        0 => 0,
        1 => 3,
        2 => 8,
        _ => 15,
    }
}

impl Splats {
    pub fn len(&self) -> usize {
        self.scale.len()
    }
    pub fn is_empty(&self) -> bool {
        self.scale.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.len() * (12 + 16 + 4 + 12) + self.sh.len() * 4
    }
    pub fn like(&self) -> Splats {
        Splats { sh_degree: self.sh_degree, sh_frame: self.sh_frame, ..Default::default() }
    }
    pub fn push_from(&mut self, other: &Splats, i: usize) {
        self.scale.push(other.scale[i]);
        self.rot.push(other.rot[i]);
        self.opacity.push(other.opacity[i]);
        self.colour.push(other.colour[i]);
        let k = sh_count(other.sh_degree) * 3;
        if self.sh_degree == other.sh_degree && k > 0 {
            self.sh.extend_from_slice(&other.sh[i * k..(i + 1) * k]);
        }
    }
    pub fn append(&mut self, other: &Splats) {
        // The view-dependent colours are kept only when both have them alike.
        if self.sh_degree != other.sh_degree || !same_turn(self.sh_frame, other.sh_frame) {
            self.sh_degree = 0;
            self.sh.clear();
        } else {
            self.sh.extend_from_slice(&other.sh);
        }
        self.scale.extend_from_slice(&other.scale);
        self.rot.extend_from_slice(&other.rot);
        self.opacity.extend_from_slice(&other.opacity);
        self.colour.extend_from_slice(&other.colour);
    }

    /// The covariance of splat `i`, in the axes of the map: xx, xy, xz, yy, yz, zz.
    pub fn covariance(&self, i: usize) -> [f32; 6] {
        let r = matrix_of(self.rot[i]);
        let s = self.scale[i].map(|v| v * v);
        let m = |a: usize, b: usize| (0..3).map(|k| r[a][k] * s[k] * r[b][k]).sum::<f32>();
        [m(0, 0), m(0, 1), m(0, 2), m(1, 1), m(1, 2), m(2, 2)]
    }
}

// ---- Quaternions and turns ----

type Quat = [f32; 4];
type Mat = [[f32; 3]; 3];

fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

fn qinv(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

fn qnorm(q: Quat) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 0.0 && n.is_finite() {
        q.map(|v| v / n)
    } else {
        [1.0, 0.0, 0.0, 0.0]
    }
}

fn same_turn(a: Quat, b: Quat) -> bool {
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
    d > 0.9999
}

/// The matrix of a turn; its columns are where the axes go.
fn matrix_of(q: Quat) -> Mat {
    let [w, x, y, z] = qnorm(q);
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// The turn of a matrix of a turn.
fn quat_of(m: Mat) -> Quat {
    let t = m[0][0] + m[1][1] + m[2][2];
    let q = if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        [0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s]
    };
    qnorm(q)
}

/// The axes of a file of splats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axes {
    /// Right, down, forward, as the training of 3D Gaussian Splatting and COLMAP: most PLY.
    YDown,
    /// Right, up, back, as OpenGL and three.js: SPZ.
    YUp,
    /// Right, forward, up, as maps and most GIS and CAD.
    ZUp,
}

impl Axes {
    /// The axes by a choice, or by the format: SPZ y up, PLY y down.
    pub fn of(choice: Option<&str>, ext: &str) -> Axes {
        match choice {
            Some("y") => Axes::YUp,
            Some("-y") => Axes::YDown,
            Some("z") => Axes::ZUp,
            _ if ext == "spz" => Axes::YUp,
            _ => Axes::YDown,
        }
    }

    /// From the axes of the file to x east, y north, z up, the forward direction north.
    fn matrix(self) -> Mat {
        match self {
            // x = X, y = Z, z = -Y
            Axes::YDown => [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]],
            // x = X, y = -Z, z = Y
            Axes::YUp => [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            Axes::ZUp => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }
}

/// Where a scene goes on the map: its axes, how many metres a unit of it is, where its 0, 0, 0
/// goes, and how much it is turned clockwise seen from above.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub axes: Axes,
    pub scale: f64,
    pub origin: [f64; 3],
    pub turn: f64,
}

impl Placement {
    /// The turn from the file to the map, without the scale.
    fn quat(&self) -> Quat {
        let a = quat_of(self.axes.matrix());
        let half = (-self.turn.to_radians() / 2.0) as f32;
        qmul([half.cos(), 0.0, 0.0, half.sin()], a)
    }
}

/// Puts a scene read in the axes of its file on the map.
pub fn place(c: &mut Cloud, p: &Placement) {
    let q = p.quat();
    let m = matrix_of(q);
    let s = p.scale;
    for i in 0..c.len() {
        let v = [c.x[i], c.y[i], c.z[i]];
        let at = |r: usize| (m[r][0] as f64 * v[0] + m[r][1] as f64 * v[1] + m[r][2] as f64 * v[2]) * s;
        let (x, y, z) = (at(0), at(1), at(2));
        c.x[i] = p.origin[0] + x;
        c.y[i] = p.origin[1] + y;
        c.z[i] = p.origin[2] + z;
    }
    if let Some(sp) = &mut c.splat {
        for r in sp.rot.iter_mut() {
            *r = qnorm(qmul(q, *r));
        }
        for v in sp.scale.iter_mut() {
            *v = v.map(|a| a * s as f32);
        }
        sp.sh_frame = qnorm(qmul(q, sp.sh_frame));
    }
}

// ---- PLY ----

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlyType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl PlyType {
    fn of(s: &str) -> Option<PlyType> {
        Some(match s {
            "char" | "int8" => PlyType::I8,
            "uchar" | "uint8" => PlyType::U8,
            "short" | "int16" => PlyType::I16,
            "ushort" | "uint16" => PlyType::U16,
            "int" | "int32" => PlyType::I32,
            "uint" | "uint32" => PlyType::U32,
            "float" | "float32" => PlyType::F32,
            "double" | "float64" => PlyType::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            PlyType::I8 | PlyType::U8 => 1,
            PlyType::I16 | PlyType::U16 => 2,
            PlyType::I32 | PlyType::U32 | PlyType::F32 => 4,
            PlyType::F64 => 8,
        }
    }
    fn read(self, b: &[u8], big: bool) -> f64 {
        macro_rules! num {
            ($t:ty, $n:expr) => {{
                let mut a = [0u8; $n];
                a.copy_from_slice(&b[..$n]);
                (if big { <$t>::from_be_bytes(a) } else { <$t>::from_le_bytes(a) }) as f64
            }};
        }
        match self {
            PlyType::I8 => b[0] as i8 as f64,
            PlyType::U8 => b[0] as f64,
            PlyType::I16 => num!(i16, 2),
            PlyType::U16 => num!(u16, 2),
            PlyType::I32 => num!(i32, 4),
            PlyType::U32 => num!(u32, 4),
            PlyType::F32 => num!(f32, 4),
            PlyType::F64 => num!(f64, 8),
        }
    }
}

/// What a PLY file was: a scene of splats or plain points, and what Ographis wrote in its
/// comments about where it lies.
pub struct PlyRead {
    pub cloud: Cloud,
    pub splats: bool,
    pub origin: Option<[f64; 3]>,
    pub crs: Option<u32>,
    pub axes: Option<Axes>,
}

/// Whether a PLY file holds points: its first element is vertex, without faces.
pub fn is_point_ply(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    let mut r = std::io::BufReader::new(f);
    let mut faces = false;
    let mut vertex = false;
    let mut line = String::new();
    for _ in 0..400 {
        line.clear();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let t = line.trim();
        if t.starts_with("element vertex") {
            vertex = true;
        }
        if t.starts_with("element face") {
            faces = true;
        }
        if t == "end_header" {
            break;
        }
    }
    vertex && !faces
}

/// Reads a PLY of points or splats, in the axes of the file.
pub fn read_ply(path: &Path) -> Result<PlyRead> {
    let named = |e: std::io::Error| GbError(format!("Cannot read {}: {}", path.display(), e));
    let f = std::fs::File::open(path).map_err(named)?;
    let mut r = std::io::BufReader::new(f);
    let mut line = String::new();
    r.read_line(&mut line).map_err(named)?;
    if line.trim() != "ply" {
        return err(format!("{} is not a PLY file.", path.display()));
    }
    let (mut format, mut count, mut props, mut in_vertex, mut first_element) = (String::new(), 0usize, Vec::<(String, PlyType)>::new(), false, true);
    let (mut origin, mut crs, mut axes) = (None, None, None);
    loop {
        line.clear();
        if r.read_line(&mut line).map_err(named)? == 0 {
            return err(format!("{} ends before its header does.", path.display()));
        }
        let t = line.trim();
        let words: Vec<&str> = t.split_whitespace().collect();
        match words.as_slice() {
            ["format", f, ..] => format = f.to_string(),
            ["comment", "ographis", "origin", x, y, z] => origin = Some([x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0), z.parse().unwrap_or(0.0)]),
            ["comment", "ographis", "crs", c] => crs = c.trim_start_matches("EPSG:").parse().ok(),
            ["comment", "ographis", "axes", a] => axes = Some(Axes::of(Some(a), "ply")),
            ["element", name, n] => {
                if *name == "vertex" {
                    if !first_element {
                        return err(format!("{}: the points must come first in the file.", path.display()));
                    }
                    count = n.parse().map_err(|_| GbError(format!("{}: the number of points is not a number.", path.display())))?;
                    in_vertex = true;
                } else {
                    in_vertex = false;
                }
                first_element = false;
            }
            ["property", "list", ..] if in_vertex => return err(format!("{}: lists in the points are not read.", path.display())),
            ["property", ty, name] if in_vertex => {
                let ty = PlyType::of(ty).ok_or_else(|| GbError(format!("{}: the type {} is not known.", path.display(), ty)))?;
                props.push((name.to_string(), ty));
            }
            ["end_header"] => break,
            _ => {}
        }
    }
    let at = |n: &str| props.iter().position(|(p, _)| p == n);
    let need = |n: &str| at(n).ok_or_else(|| GbError(format!("{} has no {} for its points.", path.display(), n)));
    let (ix, iy, iz) = (need("x")?, need("y")?, need("z")?);
    let dc = [at("f_dc_0"), at("f_dc_1"), at("f_dc_2")];
    let splats = dc.iter().all(|d| d.is_some()) && at("opacity").is_some() && at("scale_0").is_some() && at("rot_0").is_some();
    let rest: Vec<usize> = (0..45).map_while(|k| at(&format!("f_rest_{}", k))).collect();
    let degree: u8 = match rest.len() {
        n if n >= 45 => 3,
        n if n >= 24 => 2,
        n if n >= 9 => 1,
        _ => 0,
    };
    let colours = [at("red").or(at("r")), at("green").or(at("g")), at("blue").or(at("b"))];
    let has_rgb = splats || colours.iter().all(|c| c.is_some());
    let intensity = at("intensity").or(at("scalar_intensity")).or(at("scalar_Intensity"));
    let class = at("classification").or(at("scalar_classification")).or(at("scalar_Classification"));
    let mut cloud = Cloud::with_capacity(count, has_rgb);
    let mut sp = splats.then(|| Splats { sh_degree: degree, sh_frame: [1.0, 0.0, 0.0, 0.0], ..Default::default() });
    let mut handle = |v: &[f64]| {
        cloud.x.push(v[ix]);
        cloud.y.push(v[iy]);
        cloud.z.push(v[iz]);
        cloud.class.push(class.map_or(1, |c| v[c].clamp(0.0, 255.0) as u8));
        cloud.intensity.push(intensity.map_or(0, |c| v[c].clamp(0.0, 65535.0) as u16));
        cloud.ret.push(1);
        cloud.rets.push(1);
        if let Some(s) = &mut sp {
            let c = [0, 1, 2].map(|k| 0.5 + C0 * v[dc[k].expect("checked")] as f32);
            s.colour.push(c);
            s.opacity.push(1.0 / (1.0 + (-(v[at("opacity").expect("checked")] as f32)).exp()));
            s.scale.push([0, 1, 2].map(|k| (v[at(&format!("scale_{}", k)).expect("checked")] as f32).exp()));
            s.rot.push(qnorm([0, 1, 2, 3].map(|k| at(&format!("rot_{}", k)).map_or(if k == 0 { 1.0 } else { 0.0 }, |i| v[i] as f32))));
            // PLY keeps them by colour: all of red, then green, then blue.
            let n = sh_count(degree);
            let per = rest.len() / 3;
            for j in 0..n {
                for ch in 0..3 {
                    s.sh.push(v[rest[ch * per + j]] as f32);
                }
            }
            if let Some(rgb) = &mut cloud.rgb {
                rgb.push(c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        } else if let Some(rgb) = &mut cloud.rgb {
            let k = |i: Option<usize>| i.map_or(0.0, |i| v[i]);
            let (r, g, b) = (k(colours[0]), k(colours[1]), k(colours[2]));
            // Colours of 16 bits, or as numbers from 0 to 1.
            let scale = match props[colours[0].expect("checked")].1 {
                PlyType::U16 => 1.0 / 257.0,
                PlyType::F32 | PlyType::F64 if r.max(g).max(b) <= 1.0 => 255.0,
                _ => 1.0,
            };
            rgb.push([r, g, b].map(|c| (c * scale).round().clamp(0.0, 255.0) as u8));
        }
    };
    let mut v = vec![0.0f64; props.len()];
    match format.as_str() {
        "ascii" => {
            for _ in 0..count {
                line.clear();
                if r.read_line(&mut line).map_err(named)? == 0 {
                    return err(format!("{} ends before its points do.", path.display()));
                }
                for (k, w) in line.split_whitespace().take(props.len()).enumerate() {
                    v[k] = w.parse().unwrap_or(0.0);
                }
                handle(&v);
            }
        }
        "binary_little_endian" | "binary_big_endian" => {
            let big = format == "binary_big_endian";
            let stride: usize = props.iter().map(|(_, t)| t.size()).sum();
            let offsets: Vec<usize> = props
                .iter()
                .scan(0, |o, (_, t)| {
                    let at = *o;
                    *o += t.size();
                    Some(at)
                })
                .collect();
            let mut buf = vec![0u8; stride * 65536];
            let mut left = count;
            while left > 0 {
                let n = left.min(65536);
                r.read_exact(&mut buf[..n * stride]).map_err(|_| GbError(format!("{} ends before its points do.", path.display())))?;
                for row in buf[..n * stride].chunks(stride) {
                    for (k, (_, t)) in props.iter().enumerate() {
                        v[k] = t.read(&row[offsets[k]..], big);
                    }
                    handle(&v);
                }
                left -= n;
            }
        }
        other => return err(format!("{}: the format {} is not read.", path.display(), other)),
    }
    cloud.splat = sp;
    Ok(PlyRead { cloud, splats, origin, crs, axes })
}

/// Writes a cloud as PLY: splats as 3D Gaussian Splatting writes them, plain points with
/// their colours. Around `origin`, in `axes`, with where it lies in the comments. Gives whether
/// the view-dependent colours were left out, since the splats were turned.
pub fn write_ply(c: &Cloud, path: &Path, axes: Axes, origin: [f64; 3], crs: Option<u32>) -> Result<bool> {
    let named = |e: std::io::Error| GbError(format!("Cannot write {}: {}", path.display(), e));
    let f = std::fs::File::create(path).map_err(named)?;
    let mut w = std::io::BufWriter::new(f);
    let back = matrix_of(qinv(quat_of(axes.matrix())));
    let qback = qinv(quat_of(axes.matrix()));
    let mut h = String::from("ply\nformat binary_little_endian 1.0\ncomment Written by Ographis\n");
    h.push_str(&format!("comment ographis origin {} {} {}\n", origin[0], origin[1], origin[2]));
    if let Some(e) = crs {
        h.push_str(&format!("comment ographis crs EPSG:{}\n", e));
    }
    h.push_str(&format!(
        "comment ographis axes {}\n",
        match axes {
            Axes::YDown => "-y",
            Axes::YUp => "y",
            Axes::ZUp => "z",
        }
    ));
    h.push_str(&format!("element vertex {}\n", c.len()));
    let mut dropped = false;
    let keep_sh = match &c.splat {
        Some(s) if s.sh_degree > 0 => {
            let keep = same_turn(qmul(qback, s.sh_frame), [1.0, 0.0, 0.0, 0.0]);
            dropped = !keep;
            keep
        }
        _ => false,
    };
    match &c.splat {
        Some(s) => {
            h.push_str("property float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\n");
            h.push_str("property float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\n");
            if keep_sh {
                for k in 0..sh_count(s.sh_degree) * 3 {
                    h.push_str(&format!("property float f_rest_{}\n", k));
                }
            }
            h.push_str("property float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\n");
            h.push_str("property float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\n");
        }
        None => {
            h.push_str("property double x\nproperty double y\nproperty double z\n");
            if c.rgb.is_some() {
                h.push_str("property uchar red\nproperty uchar green\nproperty uchar blue\n");
            }
            h.push_str("property ushort intensity\nproperty uchar classification\n");
        }
    }
    h.push_str("end_header\n");
    w.write_all(h.as_bytes()).map_err(named)?;
    let mut row: Vec<u8> = Vec::with_capacity(512);
    for i in 0..c.len() {
        row.clear();
        let d = [c.x[i] - origin[0], c.y[i] - origin[1], c.z[i] - origin[2]];
        let p = [0, 1, 2].map(|r| back[r][0] as f64 * d[0] + back[r][1] as f64 * d[1] + back[r][2] as f64 * d[2]);
        match &c.splat {
            Some(s) => {
                let f = |row: &mut Vec<u8>, v: f32| row.extend_from_slice(&v.to_le_bytes());
                for v in p {
                    f(&mut row, v as f32);
                }
                for _ in 0..3 {
                    f(&mut row, 0.0);
                }
                for k in 0..3 {
                    f(&mut row, (s.colour[i][k] - 0.5) / C0);
                }
                if keep_sh {
                    let n = sh_count(s.sh_degree);
                    for ch in 0..3 {
                        for j in 0..n {
                            f(&mut row, s.sh[(i * n + j) * 3 + ch]);
                        }
                    }
                }
                let o = s.opacity[i].clamp(1e-6, 1.0 - 1e-6);
                f(&mut row, (o / (1.0 - o)).ln());
                for k in 0..3 {
                    f(&mut row, s.scale[i][k].max(1e-9).ln());
                }
                for v in qnorm(qmul(qback, s.rot[i])) {
                    f(&mut row, v);
                }
            }
            None => {
                for v in p {
                    row.extend_from_slice(&v.to_le_bytes());
                }
                if let Some(rgb) = &c.rgb {
                    row.extend_from_slice(&rgb[i]);
                }
                row.extend_from_slice(&c.intensity[i].to_le_bytes());
                row.push(c.class[i]);
            }
        }
        w.write_all(&row).map_err(named)?;
    }
    w.flush().map_err(named)?;
    Ok(dropped)
}

// ---- SPZ ----

const SPZ_MAGIC: u32 = 0x5053_474e;

/// Reads an SPZ file, in the axes of the file.
pub fn read_spz(path: &Path) -> Result<Cloud> {
    let named = |e: std::io::Error| GbError(format!("Cannot read {}: {}", path.display(), e));
    let packed = std::fs::read(path).map_err(named)?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&packed[..]).read_to_end(&mut bytes).map_err(|e| GbError(format!("{} is not an SPZ file: {}", path.display(), e)))?;
    if bytes.len() < 16 || u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != SPZ_MAGIC {
        return err(format!("{} is not an SPZ file.", path.display()));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let n = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let degree = bytes[12].min(3);
    let frac = bytes[13] as i32;
    if !(1..=3).contains(&version) {
        return err(format!("{}: SPZ version {} is not read.", path.display(), version));
    }
    let shn = sh_count(degree);
    let pos_size = if version == 1 { 6 } else { 9 };
    let rot_size = if version >= 3 { 4 } else { 3 };
    let want = 16 + n * (pos_size + 1 + 3 + 3 + rot_size + shn * 3);
    if bytes.len() < want {
        return err(format!("{} ends before its splats do.", path.display()));
    }
    let mut at = 16usize;
    let mut take = |len: usize| {
        let s = at;
        at += len;
        s
    };
    let (pos, alpha, col, scl, rot, shs) = (take(n * pos_size), take(n), take(n * 3), take(n * 3), take(n * rot_size), take(n * shn * 3));
    let mut c = Cloud::with_capacity(n, true);
    let mut s = Splats { sh_degree: degree, sh_frame: [1.0, 0.0, 0.0, 0.0], ..Default::default() };
    let unit = 1.0 / (1i64 << frac) as f64;
    for i in 0..n {
        let p = [0, 1, 2].map(|k| {
            if version == 1 {
                // Half floats.
                let o = pos + (i * 3 + k) * 2;
                half(u16::from_le_bytes([bytes[o], bytes[o + 1]])) as f64
            } else {
                let o = pos + (i * 3 + k) * 3;
                let v = bytes[o] as i32 | (bytes[o + 1] as i32) << 8 | (bytes[o + 2] as i32) << 16;
                let v = if v & 0x80_0000 != 0 { v | !0xff_ffff } else { v };
                v as f64 * unit
            }
        });
        c.x.push(p[0]);
        c.y.push(p[1]);
        c.z.push(p[2]);
        c.class.push(1);
        c.intensity.push(0);
        c.ret.push(1);
        c.rets.push(1);
        s.opacity.push(bytes[alpha + i] as f32 / 255.0);
        let colour = [0, 1, 2].map(|k| ((bytes[col + i * 3 + k] as f32 / 255.0) - 0.5) / 0.15 * C0 + 0.5);
        s.colour.push(colour);
        if let Some(rgb) = &mut c.rgb {
            rgb.push(colour.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        s.scale.push([0, 1, 2].map(|k| (bytes[scl + i * 3 + k] as f32 / 16.0 - 10.0).exp()));
        let q = if version >= 3 {
            let o = rot + i * 4;
            smallest_three(u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]))
        } else {
            let o = rot + i * 3;
            let xyz = [0, 1, 2].map(|k| bytes[o + k] as f32 / 127.5 - 1.0);
            let w = (1.0 - xyz.iter().map(|v| v * v).sum::<f32>()).max(0.0).sqrt();
            [w, xyz[0], xyz[1], xyz[2]]
        };
        s.rot.push(qnorm(q));
        for j in 0..shn * 3 {
            s.sh.push((bytes[shs + i * shn * 3 + j] as f32 - 128.0) / 128.0);
        }
    }
    c.splat = Some(s);
    Ok(c)
}

/// A quaternion of SPZ 3: the largest part left out, the other three of 10 bits each.
fn smallest_three(mut v: u32) -> Quat {
    let largest = (v >> 30) as usize;
    let mut q = [0.0f32; 4];
    let mut sum = 0.0;
    for i in (0..4).rev() {
        if i == largest {
            continue;
        }
        let mag = (v & 0x1ff) as f32 / 511.0 * std::f32::consts::FRAC_1_SQRT_2;
        let neg = (v >> 9) & 1 == 1;
        v >>= 10;
        q[i] = if neg { -mag } else { mag };
        sum += q[i] * q[i];
    }
    q[largest] = (1.0 - sum).max(0.0).sqrt();
    // SPZ keeps x, y, z, w.
    [q[3], q[0], q[1], q[2]]
}

fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    match e {
        0 => s * m * 2f32.powi(-24),
        31 => s * f32::INFINITY,
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Writes splats as SPZ, version 2, around `origin`, in `axes`. Gives whether the
/// view-dependent colours were left out.
pub fn write_spz(c: &Cloud, path: &Path, axes: Axes, origin: [f64; 3]) -> Result<bool> {
    let Some(s) = &c.splat else { return err("The points are not splats. Make them splats first, with Make Gaussian splats, or write them as PLY.") };
    let back = matrix_of(qinv(quat_of(axes.matrix())));
    let qback = qinv(quat_of(axes.matrix()));
    let n = c.len();
    let keep_sh = s.sh_degree > 0 && same_turn(qmul(qback, s.sh_frame), [1.0, 0.0, 0.0, 0.0]);
    let degree = if keep_sh { s.sh_degree } else { 0 };
    let shn = sh_count(degree);
    // Fixed point fine enough for the extent: 24 bits for the largest coordinate.
    let local: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let d = [c.x[i] - origin[0], c.y[i] - origin[1], c.z[i] - origin[2]];
            [0, 1, 2].map(|r| back[r][0] as f64 * d[0] + back[r][1] as f64 * d[1] + back[r][2] as f64 * d[2])
        })
        .collect();
    let far = local.iter().flat_map(|p| p.iter()).fold(0.0f64, |a, v| a.max(v.abs())).max(1e-6);
    let frac = ((8_388_607.0 / far).log2().floor() as i32).clamp(0, 24);
    let mut out: Vec<u8> = Vec::with_capacity(16 + n * (9 + 1 + 3 + 3 + 3 + shn * 3));
    out.extend_from_slice(&SPZ_MAGIC.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(&[degree, frac as u8, 0, 0]);
    let unit = (1i64 << frac) as f64;
    for p in &local {
        for v in p {
            let q = (v * unit).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
            out.extend_from_slice(&q.to_le_bytes()[..3]);
        }
    }
    for i in 0..n {
        out.push((s.opacity[i].clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    for i in 0..n {
        for k in 0..3 {
            let v = ((s.colour[i][k] - 0.5) / C0) * 0.15 + 0.5;
            out.push((v * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    for i in 0..n {
        for k in 0..3 {
            out.push(((s.scale[i][k].max(1e-9).ln() + 10.0) * 16.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    for i in 0..n {
        let mut q = qnorm(qmul(qback, s.rot[i]));
        if q[0] < 0.0 {
            q = q.map(|v| -v);
        }
        for v in &q[1..4] {
            out.push((v * 127.5 + 127.5).round().clamp(0.0, 255.0) as u8);
        }
    }
    if keep_sh {
        for i in 0..n {
            for j in 0..shn * 3 {
                out.push((s.sh[i * shn * 3 + j] * 128.0 + 128.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    let file = std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    gz.write_all(&out).and_then(|_| gz.finish().map(|_| ())).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    Ok(s.sh_degree > 0 && !keep_sh)
}

// ---- Splats from points ----

/// How points are made splats.
#[derive(Debug, Clone, Copy)]
pub struct Making {
    /// How many neighbours a point's size and shape are taken from.
    pub neighbours: usize,
    /// How much larger than the spacing of the points, so that they meet.
    pub size: f32,
    pub opacity: f32,
    /// Flat as a coin along the surface the neighbours lie on, or round.
    pub flat: bool,
}

/// The eigenvalues and eigenvectors (as columns) of a symmetric 3 × 3 matrix, by Jacobi
/// rotations.
fn eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..24 {
        let (mut p, mut q, mut big) = (0, 1, a[0][1].abs());
        for (i, j) in [(0, 2), (1, 2)] {
            if a[i][j].abs() > big {
                (p, q, big) = (i, j, a[i][j].abs());
            }
        }
        if big < 1e-15 {
            break;
        }
        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
        let t = if theta == 0.0 { 1.0 } else { t };
        let (c, s) = (1.0 / (t * t + 1.0).sqrt(), t / (t * t + 1.0).sqrt());
        let mut b = a;
        for k in 0..3 {
            b[k][p] = c * a[k][p] - s * a[k][q];
            b[k][q] = s * a[k][p] + c * a[k][q];
        }
        a = b;
        for k in 0..3 {
            b[p][k] = c * a[p][k] - s * a[q][k];
            b[q][k] = s * a[p][k] + c * a[q][k];
        }
        a = b;
        for row in v.iter_mut() {
            let (x, y) = (row[p], row[q]);
            row[p] = c * x - s * y;
            row[q] = s * x + c * y;
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

/// The cloud made splats: each point the middle of one, as large as the spacing to its
/// neighbours and, when flat, lying along the surface they make, in the colour of the
/// point, or by its height when the points have none.
pub fn make_splats(c: &Cloud, m: &Making, going: &(dyn Fn() -> bool + Sync)) -> Result<Cloud> {
    use rayon::prelude::*;
    use rstar::{primitives::GeomWithData, RTree};
    let n = c.len();
    if n < 2 {
        return err("A cloud of fewer than two points cannot be made splats.");
    }
    let mid = [c.x.iter().sum::<f64>() / n as f64, c.y.iter().sum::<f64>() / n as f64, c.z.iter().sum::<f64>() / n as f64];
    let pts: Vec<[f64; 3]> = (0..n).map(|i| [c.x[i] - mid[0], c.y[i] - mid[1], if c.z[i].is_finite() { c.z[i] - mid[2] } else { 0.0 }]).collect();
    let tree = RTree::bulk_load(pts.iter().enumerate().map(|(i, p)| GeomWithData::new(*p, i)).collect::<Vec<_>>());
    let k = m.neighbours.clamp(3, 64);
    let (zlo, zhi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[2]), b.max(p[2])));
    let stop = std::sync::atomic::AtomicBool::new(false);
    let made: Vec<([f32; 3], [f32; 4])> = (0..n)
        .into_par_iter()
        .map(|i| {
            if i % 4096 == 0 && !going() {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let p = pts[i];
            let near: Vec<[f64; 3]> = tree.nearest_neighbor_iter(&p).skip(1).take(k).map(|g| *g.geom()).collect();
            if near.is_empty() {
                return ([0.05; 3], [1.0, 0.0, 0.0, 0.0]);
            }
            let spacing = near.iter().map(|q| ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt()).sum::<f64>() / near.len() as f64;
            let size = (spacing * 0.5 * m.size as f64).max(1e-4);
            if !m.flat || near.len() < 3 {
                return ([size as f32; 3], [1.0, 0.0, 0.0, 0.0]);
            }
            // The shape of the neighbours: the axis they spread least along is the normal.
            let all: Vec<[f64; 3]> = near.iter().copied().chain(std::iter::once(p)).collect();
            let c = [0, 1, 2].map(|a| all.iter().map(|q| q[a]).sum::<f64>() / all.len() as f64);
            let mut cov = [[0.0; 3]; 3];
            for q in &all {
                let d = [q[0] - c[0], q[1] - c[1], q[2] - c[2]];
                for a in 0..3 {
                    for b in 0..3 {
                        cov[a][b] += d[a] * d[b] / all.len() as f64;
                    }
                }
            }
            let (vals, vecs) = eigen(cov);
            let mut order = [0usize, 1, 2];
            order.sort_by(|a, b| vals[*b].total_cmp(&vals[*a]));
            // Columns: the widest axis, the next, and the normal, as a right-handed turn.
            let col = |j: usize| [vecs[0][order[j]], vecs[1][order[j]], vecs[2][order[j]]];
            let (a, b) = (col(0), col(1));
            let nrm = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let mat: Mat = [0, 1, 2].map(|r| [a[r] as f32, b[r] as f32, nrm[r] as f32]);
            let along = |v: f64| (v.max(0.0).sqrt() * 1.5).clamp(size * 0.5, size * 2.0) as f32;
            let s = [along(vals[order[0]]), along(vals[order[1]]), (size * 0.15) as f32];
            (s, quat_of(mat))
        })
        .collect();
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return err(crate::model::STOPPED);
    }
    let mut out = c.clone();
    let colour: Vec<[f32; 3]> = (0..n)
        .map(|i| match &c.rgb {
            Some(rgb) => rgb[i].map(|v| v as f32 / 255.0),
            None => {
                let t = if zhi > zlo { ((pts[i][2] - zlo) / (zhi - zlo)) as f32 } else { 0.5 };
                [0.25 + 0.6 * t, 0.45 + 0.35 * t, 0.35 + 0.2 * (1.0 - t)]
            }
        })
        .collect();
    if out.rgb.is_none() {
        out.rgb = Some(colour.iter().map(|c| c.map(|v| (v * 255.0) as u8)).collect());
    }
    out.splat = Some(Splats {
        scale: made.iter().map(|m| m.0).collect(),
        rot: made.iter().map(|m| m.1).collect(),
        opacity: vec![m.opacity.clamp(0.01, 1.0); n],
        colour,
        sh_degree: 0,
        sh: Vec::new(),
        sh_frame: [1.0, 0.0, 0.0, 0.0],
    });
    Ok(out)
}

/// The attributes of splats as columns of features, for the table and the map: size and
/// opacity.
pub fn describe(c: &mut Cloud) {
    let Some(s) = c.splat.clone() else { return };
    let size: Vec<Value> = s.scale.iter().map(|v| Value::Real(((v[0].max(v[1]).max(v[2]) * 3.0) as f64 * 1000.0).round() / 1000.0)).collect();
    let opacity: Vec<Value> = s.opacity.iter().map(|o| Value::Real(((*o as f64) * 1000.0).round() / 1000.0)).collect();
    for (name, values) in [("splat_size", size), ("splat_opacity", opacity)] {
        match c.extra.iter_mut().find(|(f, _)| f.name == name) {
            Some((_, v)) => *v = values,
            None => c.extra.push((Field::new(name, FieldType::Real), values)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Cloud {
        let mut c = Cloud::with_capacity(3, true);
        let mut s = Splats { sh_degree: 1, sh_frame: [1.0, 0.0, 0.0, 0.0], ..Default::default() };
        for i in 0..3 {
            c.x.push(i as f64);
            c.y.push(-(i as f64) * 0.5);
            c.z.push(2.0);
            c.class.push(1);
            c.intensity.push(0);
            c.ret.push(1);
            c.rets.push(1);
            c.rgb.as_mut().unwrap().push([200, 100, 50]);
            s.scale.push([0.1, 0.2, 0.05]);
            s.rot.push(qnorm([0.9, 0.1 * i as f32, 0.2, 0.0]));
            s.opacity.push(0.8);
            s.colour.push([0.8, 0.4, 0.2]);
            s.sh.extend_from_slice(&[0.1, -0.1, 0.05, 0.0, 0.2, -0.2, 0.3, 0.0, 0.1]);
        }
        c.splat = Some(s);
        c
    }

    #[test]
    fn splats_go_through_ply_and_spz_and_are_placed_on_the_map() {
        let dir = std::env::temp_dir().join(format!("og-splats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = scene();
        // PLY: as it was, in the same axes.
        let ply = dir.join("a.ply");
        assert!(!write_ply(&c, &ply, Axes::ZUp, [0.0; 3], Some(3006)).unwrap());
        assert!(is_point_ply(&ply));
        let back = read_ply(&ply).unwrap();
        assert!(back.splats);
        assert_eq!((back.crs, back.origin, back.axes), (Some(3006), Some([0.0; 3]), Some(Axes::ZUp)));
        let (a, b) = (c.splat.as_ref().unwrap(), back.cloud.splat.as_ref().unwrap());
        assert_eq!(b.sh_degree, 1);
        for i in 0..3 {
            assert!((back.cloud.x[i] - c.x[i]).abs() < 1e-5);
            assert!((a.opacity[i] - b.opacity[i]).abs() < 1e-4 && (a.scale[i][1] - b.scale[i][1]).abs() < 1e-5);
            assert!(same_turn(a.rot[i], b.rot[i]));
            assert!((a.sh[i * 9 + 4] - b.sh[i * 9 + 4]).abs() < 1e-6, "the coefficients keep their order");
        }
        // SPZ: within its 8 bits.
        let spz = dir.join("a.spz");
        write_spz(&c, &spz, Axes::ZUp, [0.0; 3]).unwrap();
        let s = read_spz(&spz).unwrap();
        let t = s.splat.as_ref().unwrap();
        for i in 0..3 {
            assert!((s.y[i] - c.y[i]).abs() < 1e-3);
            assert!((t.opacity[i] - 0.8).abs() < 0.01 && (t.scale[i][0] - 0.1).abs() < 0.01);
            assert!((t.colour[i][0] - 0.8).abs() < 0.05);
            let d: f32 = (0..4).map(|k| t.rot[i][k] * a.rot[i][k]).sum::<f32>().abs();
            assert!(d > 0.99, "{:?} {:?}", t.rot[i], a.rot[i]);
        }
        // Placed: y down in the file is z up on the map, 2 m a unit, at a place in SWEREF 99.
        let mut p = scene();
        for z in p.z.iter_mut() {
            *z = 0.0;
        }
        p.y[1] = -3.0;
        place(&mut p, &Placement { axes: Axes::YDown, scale: 2.0, origin: [319_500.0, 6_399_800.0, 50.0], turn: 0.0 });
        assert!((p.x[1] - 319_502.0).abs() < 1e-6 && (p.z[1] - 56.0).abs() < 1e-6 && (p.y[1] - 6_399_800.0).abs() < 1e-6);
        assert!((p.splat.as_ref().unwrap().scale[0][0] - 0.2).abs() < 1e-6);
        // Written in other axes than its coefficients were made in, they are left out.
        assert!(write_ply(&p, &ply, Axes::ZUp, [319_500.0, 6_399_800.0, 0.0], None).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn points_of_a_surface_become_flat_splats_along_it() {
        // A flat grid of points on a plane tilted along x.
        let mut c = Cloud::with_capacity(400, false);
        for i in 0..20 {
            for j in 0..20 {
                c.x.push(i as f64 * 0.1);
                c.y.push(j as f64 * 0.1);
                c.z.push(i as f64 * 0.1);
                c.class.push(1);
                c.intensity.push(0);
                c.ret.push(1);
                c.rets.push(1);
            }
        }
        let made = make_splats(&c, &Making { neighbours: 8, size: 1.0, opacity: 0.9, flat: true }, &|| true).unwrap();
        let s = made.splat.as_ref().unwrap();
        let k = 10 * 20 + 10;
        let r = matrix_of(s.rot[k]);
        // The third axis, the thin one, is the normal of the plane: (-1, 0, 1) / √2.
        let n = [r[0][2], r[1][2], r[2][2]];
        assert!((n[0] + n[2]).abs() < 0.05 && n[1].abs() < 0.05, "{:?}", n);
        assert!(s.scale[k][2] < s.scale[k][0] * 0.5);
        assert!(made.rgb.is_some());
    }
}
