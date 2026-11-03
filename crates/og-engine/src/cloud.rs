//! A point cloud kept as columns: the coordinates, the class, the intensity and the returns
//! of each point in arrays of their own, about 30 bytes a point, rather than a feature for
//! each, which takes some 1 000. Tens of millions of points then fit in memory.
//!
//! A stream that carries a cloud has no features; its fields name the columns. The tools for
//! point clouds work on the columns, and give a cloud back when they got one and features
//! when they got features. Other tools get the points as features: the run makes them so.

use crate::model::{Feature, Field, FieldType, Table, Value};
use std::borrow::Cow;
use std::sync::Arc;

/// The points of a point cloud, as columns of the same length.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cloud {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// The class of ASPRS, such as 2 for ground and 6 for buildings.
    pub class: Vec<u8>,
    pub intensity: Vec<u16>,
    /// Which return of its pulse the point is, from 1, and of how many.
    pub ret: Vec<u8>,
    pub rets: Vec<u8>,
    /// Red, green and blue of 8 bits, when the cloud has colours.
    pub rgb: Option<Vec<[u8; 3]>>,
    /// Further attributes, such as the height above ground, a value for each point.
    pub extra: Vec<(Field, Vec<Value>)>,
}

/// The names of the columns every cloud has, in order.
const BASE: [(&str, FieldType); 5] = [
    ("classification", FieldType::Int),
    ("class", FieldType::Text),
    ("intensity", FieldType::Int),
    ("return_number", FieldType::Int),
    ("returns", FieldType::Int),
];

impl Cloud {
    pub fn len(&self) -> usize {
        self.x.len()
    }
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// A cloud with room for `n` points.
    pub fn with_capacity(n: usize, colour: bool) -> Cloud {
        Cloud {
            x: Vec::with_capacity(n),
            y: Vec::with_capacity(n),
            z: Vec::with_capacity(n),
            class: Vec::with_capacity(n),
            intensity: Vec::with_capacity(n),
            ret: Vec::with_capacity(n),
            rets: Vec::with_capacity(n),
            rgb: colour.then(|| Vec::with_capacity(n)),
            extra: Vec::new(),
        }
    }

    /// The bytes the cloud takes in memory, roughly.
    pub fn bytes(&self) -> usize {
        self.len() * (3 * 8 + 1 + 2 + 2 + self.rgb.as_ref().map_or(0, |_| 3) + self.extra.len() * std::mem::size_of::<Value>())
    }

    /// The fields that name the columns, as a stream of features would have them.
    pub fn fields(&self) -> Vec<Field> {
        let mut f: Vec<Field> = BASE.iter().map(|(n, t)| Field::new(*n, *t)).collect();
        if self.rgb.is_some() {
            for c in ["red", "green", "blue"] {
                f.push(Field::new(c, FieldType::Int));
            }
        }
        f.extend(self.extra.iter().map(|(f, _)| f.clone()));
        f
    }

    /// The values of the attributes of point `i`, in the order of `fields`.
    pub fn attrs(&self, i: usize) -> Vec<Value> {
        let mut a = vec![
            Value::Int(self.class[i] as i64),
            Value::Text(crate::pointcloud::class_name(self.class[i]).to_string()),
            Value::Int(self.intensity[i] as i64),
            Value::Int(self.ret[i] as i64),
            Value::Int(self.rets[i] as i64),
        ];
        if let Some(c) = &self.rgb {
            a.extend(c[i].map(|v| Value::Int(v as i64)));
        }
        a.extend(self.extra.iter().map(|(_, v)| v[i].clone()));
        a
    }

    /// The value of column `k` of `fields` for point `i`, as a number, without making the
    /// others.
    pub fn number(&self, i: usize, k: usize) -> Option<f64> {
        let colours = if self.rgb.is_some() { 3 } else { 0 };
        match k {
            0 => Some(self.class[i] as f64),
            1 => None,
            2 => Some(self.intensity[i] as f64),
            3 => Some(self.ret[i] as f64),
            4 => Some(self.rets[i] as f64),
            k if k < 5 + colours => self.rgb.as_ref().map(|c| c[i][k - 5] as f64),
            k => self.extra.get(k - 5 - colours).and_then(|(_, v)| v[i].as_f64()),
        }
    }

    /// Point `i` as a feature.
    pub fn feature(&self, i: usize) -> Feature {
        let z = self.z[i];
        Feature { z: (!z.is_nan()).then(|| Arc::from(vec![z])), attrs: self.attrs(i), geom: Some(geo::Geometry::Point(geo::Point::new(self.x[i], self.y[i]))) }
    }

    /// Puts point `i` of `other`, which has the same columns, at the end.
    pub fn push_from(&mut self, other: &Cloud, i: usize) {
        self.x.push(other.x[i]);
        self.y.push(other.y[i]);
        self.z.push(other.z[i]);
        self.class.push(other.class[i]);
        self.intensity.push(other.intensity[i]);
        self.ret.push(other.ret[i]);
        self.rets.push(other.rets[i]);
        if let (Some(a), Some(b)) = (&mut self.rgb, &other.rgb) {
            a.push(b[i]);
        }
        for ((_, a), (_, b)) in self.extra.iter_mut().zip(&other.extra) {
            a.push(b[i].clone());
        }
    }

    /// An empty cloud with the same columns.
    pub fn like(&self) -> Cloud {
        Cloud { rgb: self.rgb.as_ref().map(|_| Vec::new()), extra: self.extra.iter().map(|(f, _)| (f.clone(), Vec::new())).collect(), ..Default::default() }
    }

    /// The points for which `keep` says true, and those for which it says false.
    pub fn split(&self, keep: impl Fn(usize) -> bool) -> (Cloud, Cloud) {
        let (mut a, mut b) = (self.like(), self.like());
        for i in 0..self.len() {
            if keep(i) {
                a.push_from(self, i);
            } else {
                b.push_from(self, i);
            }
        }
        (a, b)
    }

    /// The column of an extra attribute, made when it is not there; its values are null.
    pub fn column(&mut self, name: &str, ty: FieldType) -> usize {
        if let Some(i) = self.extra.iter().position(|(f, _)| f.name == name) {
            return i;
        }
        self.extra.push((Field::new(name, ty), vec![Value::Null; self.len()]));
        self.extra.len() - 1
    }

    /// Puts the points of another cloud at the end. The extra columns are those of both.
    pub fn append(&mut self, other: &Cloud) {
        let n = self.len();
        for (f, _) in &other.extra {
            if !self.extra.iter().any(|(g, _)| g.name == f.name) {
                self.extra.push((f.clone(), vec![Value::Null; n]));
            }
        }
        if self.rgb.is_some() && other.rgb.is_none() {
            self.rgb = None;
        }
        self.x.extend_from_slice(&other.x);
        self.y.extend_from_slice(&other.y);
        self.z.extend_from_slice(&other.z);
        self.class.extend_from_slice(&other.class);
        self.intensity.extend_from_slice(&other.intensity);
        self.ret.extend_from_slice(&other.ret);
        self.rets.extend_from_slice(&other.rets);
        if let (Some(a), Some(b)) = (&mut self.rgb, &other.rgb) {
            a.extend_from_slice(b);
        }
        for (f, col) in self.extra.iter_mut() {
            match other.extra.iter().find(|(g, _)| g.name == f.name) {
                Some((_, v)) => col.extend(v.iter().cloned()),
                None => col.extend(std::iter::repeat_n(Value::Null, other.len())),
            }
        }
    }

    /// The cloud as a stream that carries it.
    pub fn into_table(self, crs: Option<u32>) -> Table {
        Table { fields: self.fields(), crs, points: Some(Arc::new(self)), ..Default::default() }
    }

    /// Every `every`-th point as a feature: the stream other tools work on.
    pub fn to_features(&self, crs: Option<u32>, every: usize) -> Table {
        let every = every.max(1);
        Table { fields: self.fields(), crs, features: (0..self.len()).step_by(every).map(|i| self.feature(i)).collect(), ..Default::default() }
    }

    /// Points as a cloud, those without a height with NaN for it: classification, intensity, return_number and returns
    /// (or number_of_returns), and red, green and blue, are taken from the attributes of the
    /// same names, and the other attributes go along as extra columns. Features that are not
    /// points are left out, and counted.
    pub fn from_features(t: &Table) -> (Cloud, usize) {
        let (c, rows) = Cloud::from_features_rows(t);
        let left = t.len() - rows.len();
        (c, left)
    }

    /// As `from_features`, with the row of the stream each point came from.
    pub fn from_features_rows(t: &Table) -> (Cloud, Vec<usize>) {
        let at = |n: &str| t.field_index(n);
        let (class_at, int_at, ret_at, rets_at) =
            (at("classification"), at("intensity"), at("return_number"), at("returns").or_else(|| at("number_of_returns")));
        let rgb_at = match (at("red"), at("green"), at("blue")) {
            (Some(r), Some(g), Some(b)) => Some([r, g, b]),
            _ => None,
        };
        let used: Vec<usize> = [class_at, int_at, ret_at, rets_at, at("class")].into_iter().flatten().chain(rgb_at.into_iter().flatten()).collect();
        let others: Vec<usize> = (0..t.fields.len()).filter(|i| !used.contains(i)).collect();
        let mut c = Cloud::with_capacity(t.len(), rgb_at.is_some());
        c.extra = others.iter().map(|i| (t.fields[*i].clone(), Vec::with_capacity(t.len()))).collect();
        let num = |f: &Feature, i: Option<usize>, d: f64| i.and_then(|i| f.attrs[i].as_f64()).unwrap_or(d);
        let mut rows = Vec::with_capacity(t.len());
        for (row, f) in t.features.iter().enumerate() {
            let Some(geo::Geometry::Point(p)) = &f.geom else {
                continue;
            };
            rows.push(row);
            let z = f.heights().and_then(|z| z.first().copied()).unwrap_or(f64::NAN);
            c.x.push(p.x());
            c.y.push(p.y());
            c.z.push(z);
            c.class.push(num(f, class_at, 1.0).clamp(0.0, 255.0) as u8);
            c.intensity.push(num(f, int_at, 0.0).clamp(0.0, 65535.0) as u16);
            let rets = num(f, rets_at, 1.0).clamp(1.0, 15.0) as u8;
            c.rets.push(rets);
            c.ret.push((num(f, ret_at, 1.0) as u8).clamp(1, rets));
            if let (Some(v), Some(ix)) = (&mut c.rgb, rgb_at) {
                v.push(ix.map(|i| num(f, Some(i), 0.0).clamp(0.0, 255.0) as u8));
            }
            for (k, i) in others.iter().enumerate() {
                c.extra[k].1.push(f.attrs[*i].clone());
            }
        }
        (c, rows)
    }
}

/// The points of a stream a tool for point clouds works on: the cloud it carries, or its
/// points made into one, with the row each came from, so that a stream of features comes
/// back as features with all its attributes as they were.
pub struct PointsIn<'a> {
    pub table: &'a Table,
    pub cloud: Cow<'a, Cloud>,
    /// For a stream of features: the row of each point. Empty for a cloud.
    pub rows: Vec<usize>,
    /// Features that are not points, which were left out.
    pub left: usize,
}

pub fn points_in(t: &Table) -> PointsIn<'_> {
    match &t.points {
        Some(c) => PointsIn { table: t, cloud: Cow::Borrowed(c.as_ref()), rows: Vec::new(), left: 0 },
        None => {
            let (c, rows) = Cloud::from_features_rows(t);
            let left = t.len() - rows.len();
            PointsIn { table: t, cloud: Cow::Owned(c), rows, left }
        }
    }
}

impl PointsIn<'_> {
    /// The points for which `keep` says true, and the rest, as streams of the kind that came
    /// in. Features that are not points go with the rest.
    pub fn split(&self, keep: impl Fn(usize) -> bool) -> (Table, Table) {
        if self.table.points.is_some() {
            let (a, b) = self.cloud.split(keep);
            return (a.into_table(self.table.crs), b.into_table(self.table.crs));
        }
        let mut kept = vec![false; self.table.len()];
        for (i, r) in self.rows.iter().enumerate() {
            kept[*r] = keep(i);
        }
        let (mut a, mut b) = (self.table.like(), self.table.like());
        for (f, k) in self.table.features.iter().zip(kept) {
            if k {
                a.features.push(f.clone());
            } else {
                b.features.push(f.clone());
            }
        }
        (a, b)
    }

    /// The stream with a new value of an attribute for each point, and new heights if
    /// given: in the cloud, or in the features the points came from.
    pub fn with_values(&self, name: &str, ty: FieldType, values: &[Value], heights: Option<&[f64]>) -> Table {
        if self.table.points.is_some() {
            let mut c = self.cloud.clone().into_owned();
            if matches!(name, "classification") {
                for (k, v) in values.iter().enumerate() {
                    if let Some(x) = v.as_f64() {
                        c.class[k] = x.clamp(0.0, 255.0) as u8;
                    }
                }
            } else {
                let at = c.column(name, ty);
                c.extra[at].1 = values.to_vec();
            }
            if let Some(h) = heights {
                c.z = h.to_vec();
            }
            return c.into_table(self.table.crs);
        }
        let mut out = self.table.clone();
        let slot = match out.field_index(name) {
            Some(i) => i,
            None => {
                out.fields.push(Field::new(name, ty));
                for f in out.features.iter_mut() {
                    f.attrs.push(Value::Null);
                }
                out.fields.len() - 1
            }
        };
        let class_name = (name == "classification").then(|| out.field_index("class")).flatten();
        for (k, r) in self.rows.iter().enumerate() {
            let f = &mut out.features[*r];
            f.attrs[slot] = values[k].clone();
            if let (Some(n), Some(c)) = (class_name, values[k].as_f64()) {
                f.attrs[n] = Value::Text(crate::pointcloud::class_name(c as u8).to_string());
            }
            if let Some(h) = heights {
                if !h[k].is_nan() {
                    f.z = Some(Arc::from(vec![h[k]]));
                }
            }
        }
        out
    }
}

/// The point cloud of a stream: the one it carries, or its points with heights made into one.
/// The number is how many features were not points and were left out.
pub fn cloud_of(t: &Table) -> (Cow<'_, Cloud>, usize) {
    match &t.points {
        Some(c) => (Cow::Borrowed(c.as_ref()), 0),
        None => {
            let (c, left) = Cloud::from_features(t);
            (Cow::Owned(c), left)
        }
    }
}

/// What a tool for point clouds gives back: a cloud when it got one, features when it got
/// features, so that small streams of points stay as they were.
pub fn give_back(input: &Table, c: Cloud) -> Table {
    if input.points.is_some() {
        c.into_table(input.crs)
    } else {
        c.to_features(input.crs, 1)
    }
}

/// The tools that work on point clouds as columns. Others get the points as features.
pub fn takes_clouds(kind: &str) -> bool {
    matches!(
        kind,
        "Writer"
            | "Viewer"
            | "PointCloudRasterizer"
            | "HeightAboveGround"
            | "PointCloudFilter"
            | "PointThinner"
            | "GroundClassifier"
            | "BuildingHeights"
            | "Lod2Builder"
            | "Reprojector"
    )
}

/// A stream with its point cloud made into features, for a tool that works on features. The
/// note says what that takes when the cloud is large.
pub fn as_features(t: &Arc<Table>, notes: &mut crate::io::Notes) -> Arc<Table> {
    let Some(c) = &t.points else { return t.clone() };
    if c.len() > 2_000_000 {
        notes.warn(format!(
            "{} points were made into features for this tool, which takes about {} GB of memory. The tools for point clouds work on them without that: filter, thin or make a raster first.",
            crate::model::fmt_count(c.len() as u64),
            crate::model::fmt_real((c.len() as f64 * 1.1e-6).round().max(1.0))
        ));
    }
    Arc::new(c.to_features(t.crs, 1))
}

/// The cloud in another coordinate system. The heights stay as they are.
pub fn reproject(c: &Cloud, from: u32, to: u32, backend: Option<&dyn crate::io::Backend>) -> crate::model::Result<Cloud> {
    let b = backend.ok_or_else(|| crate::model::GbError("Reprojecting a point cloud needs the GDAL support.".into()))?;
    let mut out = c.clone();
    // A million at a time, so that the coordinates are not copied all at once.
    for (xs, ys) in out.x.chunks_mut(1_000_000).zip(out.y.chunks_mut(1_000_000)) {
        b.transform(from, to, xs, ys)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> Cloud {
        let mut c = Cloud::with_capacity(3, false);
        for i in 0..3 {
            c.x.push(i as f64);
            c.y.push(0.0);
            c.z.push(10.0 + i as f64);
            c.class.push(if i == 1 { 6 } else { 2 });
            c.intensity.push(100);
            c.ret.push(1);
            c.rets.push(1);
        }
        c
    }

    #[test]
    fn a_cloud_goes_to_features_and_back() {
        let mut c = cloud();
        let k = c.column("above_ground", FieldType::Real);
        c.extra[k].1[1] = Value::Real(4.5);
        let t = c.to_features(Some(3006), 1);
        assert_eq!(t.len(), 3);
        assert_eq!(t.field_names(), ["classification", "class", "intensity", "return_number", "returns", "above_ground"]);
        assert_eq!(t.features[1].attrs[1], Value::Text("building".into()));
        let (back, left) = Cloud::from_features(&t);
        assert_eq!((left, back), (0, c.clone()));
        let (ground, other) = c.split(|i| c.class[i] == 2);
        assert_eq!((ground.len(), other.len()), (2, 1));
        assert!(c.bytes() < 3 * 60);
        let mut both = ground.clone();
        both.append(&other);
        assert_eq!(both.len(), 3);
        assert_eq!(both.extra[0].1[2], Value::Real(4.5));
    }
}
