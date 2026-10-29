//! The data model: values, fields, features and tables. All geometry is 2D (`geo`).

use geo::Geometry;
use std::fmt;

/// An error with a message that can be shown to the user as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct GbError(pub String);

impl fmt::Display for GbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for GbError {}

pub type Result<T> = std::result::Result<T, GbError>;

/// Short for `Err(GbError(format!(...)))`.
#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => { return Err($crate::model::GbError(format!($($arg)*))) };
}
pub fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(GbError(msg.into()))
}

/// An attribute value. `Bool` only exists in expressions and is stored as the integer 1/0.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    /// The number, if the value is a number or a text that reads as one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Text(s) => parse_number(s),
            Value::Null => None,
        }
    }
    /// Text for display and output. Null becomes an empty string.
    pub fn to_text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
            Value::Int(i) => i.to_string(),
            Value::Real(r) => fmt_real(*r),
            Value::Text(s) => s.clone(),
        }
    }
    /// Key for joins: 5, 5.0 and "5" are the same key, but "0114" is not 114.
    pub fn key(&self) -> Option<String> {
        match self {
            Value::Null => None,
            Value::Text(s) if s.is_empty() => None,
            other => Some(other.to_text()),
        }
    }
    /// Turns `Bool` into an integer before it is stored in a table.
    pub fn stored(self) -> Value {
        match self {
            Value::Bool(b) => Value::Int(b as i64),
            Value::Real(r) if !r.is_finite() => Value::Null,
            v => v,
        }
    }
}

/// Parses a number with a point or a comma as the decimal separator.
pub fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok().or_else(|| t.replace(',', ".").parse::<f64>().ok()).filter(|v| v.is_finite())
}

/// The error a step gives when the run was stopped while it was at work.
pub const STOPPED: &str = "Stopped.";

/// A time in milliseconds as people read it: 0.04 s, 2.3 s, 45 s, 4 min 12 s, 1 h 3 min.
pub fn fmt_duration(ms: f64) -> String {
    let s = ms / 1000.0;
    if s < 0.1 {
        format!("{:.2} s", s)
    } else if s < 10.0 {
        format!("{:.1} s", s)
    } else if s < 59.5 {
        format!("{:.0} s", s)
    } else if s < 3600.0 {
        let whole = s.round() as u64;
        format!("{} min {} s", whole / 60, whole % 60)
    } else {
        let whole = (s / 60.0).round() as u64;
        format!("{} h {} min", whole / 60, whole % 60)
    }
}

/// A count with a space between the thousands, so that 4303726 reads as 4 303 726.
pub fn fmt_count(n: impl Into<u128>) -> String {
    let digits = n.into().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Formats a float without needless decimals: 5.0 becomes "5", 0.1+0.2 becomes "0.3".
pub fn fmt_real(r: f64) -> String {
    if !r.is_finite() {
        return String::new();
    }
    if r == r.trunc() && r.abs() < 1e15 {
        return format!("{}", r as i64);
    }
    // At most 15 significant digits, so that float noise such as 6400326.0360000003 is not printed.
    let int_digits = (r.abs().log10().floor() as i32 + 1).max(1);
    let decimals = (15 - int_digits).clamp(0, 10) as usize;
    let s = format!("{:.*}", decimals, r);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s.as_str() };
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Int,
    Real,
    Text,
}

impl FieldType {
    pub fn label(&self) -> &'static str {
        match self {
            FieldType::Int => "integer",
            FieldType::Real => "decimal",
            FieldType::Text => "text",
        }
    }
    /// The type that holds both.
    pub fn widen(self, other: FieldType) -> FieldType {
        use FieldType::*;
        match (self, other) {
            (a, b) if a == b => a,
            (Text, _) | (_, Text) => Text,
            _ => Real,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: FieldType,
}

impl Field {
    pub fn new(name: impl Into<String>, ty: FieldType) -> Field {
        Field { name: name.into(), ty }
    }
}

#[derive(Debug, Clone)]
pub struct Feature {
    /// One value per field, in field order.
    pub attrs: Vec<Value>,
    pub geom: Option<Geometry<f64>>,
    /// The height of each coordinate of the geometry, in the order `crate::geom::coords`
    /// walks them. None for a flat geometry. Heights that no longer match the number of
    /// coordinates, after a tool changed the geometry, are not used.
    pub z: Option<std::sync::Arc<[f64]>>,
}

impl Feature {
    pub fn new(attrs: Vec<Value>, geom: Option<Geometry<f64>>) -> Feature {
        Feature { attrs, geom, z: None }
    }

    /// The heights of the coordinates, when there are as many as the geometry has.
    pub fn heights(&self) -> Option<&[f64]> {
        let z = self.z.as_deref()?;
        let g = self.geom.as_ref()?;
        (crate::geom::coord_count(g) == z.len()).then_some(z)
    }
}

/// A stream of features with a shared schema and coordinate system.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub fields: Vec<Field>,
    /// EPSG code. `None` means the stream has no geometry.
    pub crs: Option<u32>,
    pub features: Vec<Feature>,
    /// The features are the cells of a raster, such as a GeoTIFF, as points or squares.
    pub raster: bool,
    /// A picture with coordinates that goes with the stream: a map that has been drawn,
    /// or a raster read as an image. The stream then holds one feature, its outline.
    pub image: Option<std::sync::Arc<RasterImage>>,
    /// Values on a grid that goes with the stream, such as heights read from a raster for
    /// rendering terrain. The stream then holds one feature, its outline.
    pub grid: Option<std::sync::Arc<Grid>>,
    /// A 3D model of triangles that goes with the stream, such as a building read from an
    /// OBJ file. The features are then its objects.
    pub mesh: Option<std::sync::Arc<crate::mesh::Mesh>>,
}

/// Values on a regular grid, such as the heights of a terrain model.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    /// One value per cell, row by row from the top. NaN where there is no value.
    pub values: Vec<f32>,
    /// Where it lies, as in GDAL: x of the upper left corner, cell width, 0, y of the upper
    /// left corner, 0, cell height (negative, since rows go south).
    pub geotransform: [f64; 6],
    /// The coordinate system of the grid, which is that of the stream when it was read.
    pub crs: Option<u32>,
    /// The values of the further bands of a raster with several, band 2 first, each as
    /// `values`. Empty for one band.
    pub bands: Vec<Vec<f32>>,
}

impl Grid {
    /// The cell at a world coordinate, as (column, row, value), or None outside the grid.
    /// The value is None where the cell has none.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize, Option<f32>)> {
        let g = &self.geotransform;
        if g[1] == 0.0 || g[5] == 0.0 {
            return None;
        }
        let (col, row) = ((x - g[0]) / g[1], (y - g[3]) / g[5]);
        if col < 0.0 || row < 0.0 || col as usize >= self.width || row as usize >= self.height {
            return None;
        }
        let (c, r) = (col as usize, row as usize);
        Some((c, r, self.get(c, r)))
    }

    /// The value at a cell, or None outside the grid or where there is no value.
    pub fn get(&self, col: usize, row: usize) -> Option<f32> {
        if col >= self.width || row >= self.height {
            return None;
        }
        let v = self.values[row * self.width + col];
        (!v.is_nan()).then_some(v)
    }
    /// The smallest and largest value.
    pub fn range(&self) -> Option<(f32, f32)> {
        let (lo, hi) = self.values.iter().filter(|v| !v.is_nan()).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
        (lo <= hi).then_some((lo, hi))
    }
    /// The size of a cell across and down, in the units of the coordinate system.
    pub fn cell_size(&self) -> (f64, f64) {
        let g = &self.geotransform;
        ((g[1] * g[1] + g[4] * g[4]).sqrt(), (g[2] * g[2] + g[5] * g[5]).sqrt())
    }
    /// The outline of the grid.
    pub fn outline(&self) -> geo::Polygon<f64> {
        let g = &self.geotransform;
        let at = |c: f64, r: f64| geo::Coord { x: g[0] + c * g[1] + r * g[2], y: g[3] + c * g[4] + r * g[5] };
        let (w, h) = (self.width as f64, self.height as f64);
        geo::Polygon::new(geo::LineString::new(vec![at(0.0, 0.0), at(w, 0.0), at(w, h), at(0.0, h), at(0.0, 0.0)]), vec![])
    }
    /// The value at a place in the coordinates of the grid, from the cell it lies in.
    pub fn at(&self, x: f64, y: f64) -> Option<f32> {
        let g = &self.geotransform;
        let col = (x - g[0]) / g[1];
        let row = (y - g[3]) / g[5];
        if col < 0.0 || row < 0.0 {
            return None;
        }
        self.get(col as usize, row as usize)
    }

    /// A grid with no values yet, every cell empty.
    pub fn blank(width: usize, height: usize, geotransform: [f64; 6], crs: Option<u32>) -> Grid {
        Grid { width, height, values: vec![f32::NAN; width * height], geotransform, crs, bands: Vec::new() }
    }

    /// Whether the grid is turned, so that its rows do not run east and west.
    pub fn is_turned(&self) -> bool {
        self.geotransform[2] != 0.0 || self.geotransform[4] != 0.0
    }

    /// The middle of a cell, in the coordinates of the grid.
    pub fn center(&self, col: usize, row: usize) -> (f64, f64) {
        let g = &self.geotransform;
        let (c, r) = (col as f64 + 0.5, row as f64 + 0.5);
        (g[0] + c * g[1] + r * g[2], g[3] + c * g[4] + r * g[5])
    }

    /// The value at a place, between the middles of the four nearest cells. Where one of them
    /// is empty, or at the edge, the value of the cell the place lies in.
    pub fn bilinear(&self, x: f64, y: f64) -> Option<f32> {
        let g = &self.geotransform;
        if g[1] == 0.0 || g[5] == 0.0 {
            return None;
        }
        let (fc, fr) = ((x - g[0]) / g[1] - 0.5, (y - g[3]) / g[5] - 0.5);
        let own = self.at(x, y)?;
        let (c0, r0) = (fc.floor(), fr.floor());
        if c0 < 0.0 || r0 < 0.0 || c0 as usize + 1 >= self.width || r0 as usize + 1 >= self.height {
            return Some(own);
        }
        let (c, r) = (c0 as usize, r0 as usize);
        let (Some(a), Some(b), Some(cc), Some(d)) = (self.get(c, r), self.get(c + 1, r), self.get(c, r + 1), self.get(c + 1, r + 1)) else {
            return Some(own);
        };
        let (tx, ty) = ((fc - c0) as f32, (fr - r0) as f32);
        Some((a * (1.0 - tx) + b * tx) * (1.0 - ty) + (cc * (1.0 - tx) + d * tx) * ty)
    }

    /// Several grids put together into one, such as the tiles of a terrain model. The cells
    /// are the size of the coarsest grid, lined up with the first. Where grids overlap, the
    /// first with a value wins. At most `max_cells` cells: a larger result gets coarser cells.
    pub fn mosaic(grids: &[&Grid], max_cells: usize) -> Option<Grid> {
        let first = grids.first()?;
        let mut size = grids.iter().map(|g| g.cell_size().0.max(g.cell_size().1)).fold(0.0, f64::max);
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for g in grids {
            let t = &g.geotransform;
            let (ax, bx) = (t[0], t[0] + t[1] * g.width as f64);
            let (ay, by) = (t[3], t[3] + t[5] * g.height as f64);
            x0 = x0.min(ax.min(bx));
            x1 = x1.max(ax.max(bx));
            y0 = y0.min(ay.min(by));
            y1 = y1.max(ay.max(by));
        }
        let cells = |s: f64| ((x1 - x0) / s).ceil() * ((y1 - y0) / s).ceil();
        while cells(size) > max_cells as f64 {
            size *= 1.25;
        }
        // Lined up with the corner of the first grid.
        let fx = first.geotransform[0];
        let fy = first.geotransform[3];
        let left = fx - ((fx - x0) / size).ceil() * size;
        let top = fy + ((y1 - fy) / size).ceil() * size;
        let width = ((x1 - left) / size).ceil().max(1.0) as usize;
        let height = ((top - y0) / size).ceil().max(1.0) as usize;
        let mut values = vec![f32::NAN; width * height];
        for row in 0..height {
            let y = top - (row as f64 + 0.5) * size;
            for col in 0..width {
                let x = left + (col as f64 + 0.5) * size;
                values[row * width + col] = grids.iter().find_map(|g| g.at(x, y)).unwrap_or(f32::NAN);
            }
        }
        Some(Grid { bands: Vec::new(), width, height, values, geotransform: [left, size, 0.0, top, 0.0, -size], crs: first.crs })
    }

    /// The grid as a stream: one feature, its outline, with its size and the range of values.
    pub fn into_table(self) -> Table {
        let fields = vec![
            Field::new("columns", FieldType::Int),
            Field::new("rows", FieldType::Int),
            Field::new("cell_size", FieldType::Real),
            Field::new("min", FieldType::Real),
            Field::new("max", FieldType::Real),
        ];
        let range = self.range();
        let attrs = vec![
            Value::Int(self.width as i64),
            Value::Int(self.height as i64),
            Value::Real(self.cell_size().0),
            // Heights are stored with the precision of f32. Two decimals say what they hold.
            range.map_or(Value::Null, |r| Value::Real((r.0 as f64 * 100.0).round() / 100.0)),
            range.map_or(Value::Null, |r| Value::Real((r.1 as f64 * 100.0).round() / 100.0)),
        ];
        let feature = Feature { z: None, attrs, geom: Some(geo::Geometry::Polygon(self.outline())) };
        Table { mesh: None, fields, crs: self.crs, features: vec![feature], raster: false, image: None, grid: Some(std::sync::Arc::new(self)) }
    }
}

/// A picture with coordinates, in the coordinate system of the table it goes with.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterImage {
    pub width: usize,
    pub height: usize,
    /// Red, green, blue and alpha for each pixel, row by row from the top, not premultiplied.
    pub rgba: Vec<u8>,
    /// Where it lies, as in GDAL: x of the upper left corner, pixel width, 0, y of the upper
    /// left corner, 0, pixel height (negative, since rows go south).
    pub geotransform: [f64; 6],
}

impl RasterImage {
    /// The pixel at a world coordinate, as (column, row), or None outside the picture.
    pub fn pixel_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let g = &self.geotransform;
        let det = g[1] * g[5] - g[2] * g[4];
        if det == 0.0 {
            return None;
        }
        let (dx, dy) = (x - g[0], y - g[3]);
        let col = (g[5] * dx - g[2] * dy) / det;
        let row = (g[1] * dy - g[4] * dx) / det;
        (col >= 0.0 && row >= 0.0 && (col as usize) < self.width && (row as usize) < self.height).then_some((col as usize, row as usize))
    }
    /// The colour of the pixel at a world coordinate, red, green, blue and opacity.
    pub fn colour_at(&self, x: f64, y: f64) -> Option<(usize, usize, [u8; 4])> {
        let (c, r) = self.pixel_at(x, y)?;
        let i = (r * self.width + c) * 4;
        Some((c, r, [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]))
    }
    /// The world coordinate of a point given in pixels from the upper left corner.
    pub fn at(&self, col: f64, row: f64) -> (f64, f64) {
        let g = &self.geotransform;
        (g[0] + col * g[1] + row * g[2], g[3] + col * g[4] + row * g[5])
    }
    /// The outline as an area, for the feature that stands for the image.
    pub fn outline(&self) -> geo::Polygon<f64> {
        let (w, h) = (self.width as f64, self.height as f64);
        let ring: Vec<(f64, f64)> = vec![self.at(0.0, 0.0), self.at(w, 0.0), self.at(w, h), self.at(0.0, h), self.at(0.0, 0.0)];
        geo::Polygon::new(geo::LineString::from(ring), vec![])
    }
    /// The image as the stream that carries it: one feature, its outline, with its size.
    pub fn into_table(self, crs: Option<u32>) -> Table {
        let fields = vec![Field::new("width", FieldType::Int), Field::new("height", FieldType::Int), Field::new("pixel_size", FieldType::Real)];
        let g = &self.geotransform;
        let attrs = vec![Value::Int(self.width as i64), Value::Int(self.height as i64), Value::Real((g[1] * g[1] + g[4] * g[4]).sqrt())];
        let feature = Feature { z: None, attrs, geom: Some(geo::Geometry::Polygon(self.outline())) };
        Table { mesh: None, fields, crs, features: vec![feature], raster: false, image: Some(std::sync::Arc::new(self)), grid: None }
    }
    /// The image with the values of its cells: a picture that is shown, and the values
    /// that a click on the map tells, and that Shade terrain and the like can use.
    pub fn into_table_with(self, crs: Option<u32>, grid: Option<Grid>) -> Table {
        let mut t = self.into_table(crs);
        t.grid = grid.map(std::sync::Arc::new);
        t
    }
}

impl Table {
    pub fn len(&self) -> usize {
        self.features.len()
    }
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
    pub fn field_names(&self) -> Vec<String> {
        self.fields.iter().map(|f| f.name.clone()).collect()
    }
    /// The index of a field, or an error that lists the ones that exist.
    pub fn need_field(&self, name: &str) -> Result<usize> {
        // An empty stream often lacks the attributes that its features would have brought.
        if self.features.is_empty() && self.field_index(name).is_none() {
            return Err(GbError(format!(
                "There is no attribute \"{}\": nothing came in, so the stream has none of the attributes it would have. Look at what comes before, such as requests that went to Failed.",
                name
            )));
        }
        self.field_index(name).ok_or_else(|| {
            GbError(format!(
                "There is no attribute \"{}\". Available: {}.",
                name,
                if self.fields.is_empty() { "no attributes".to_string() } else { self.field_names().join(", ") }
            ))
        })
    }
    /// The same schema and coordinate system, but no features.
    pub fn like(&self) -> Table {
        Table { mesh: None, fields: self.fields.clone(), crs: self.crs, features: Vec::new(), raster: self.raster, image: None, grid: None }
    }
    pub fn has_geometry(&self) -> bool {
        self.features.iter().any(|f| f.geom.is_some())
    }
}

/// The narrowest field type that holds all the values. Only nulls gives text.
pub fn infer_type<'a>(values: impl Iterator<Item = &'a Value>) -> FieldType {
    let mut ty: Option<FieldType> = None;
    for v in values {
        let t = match v {
            Value::Null => continue,
            Value::Int(_) | Value::Bool(_) => FieldType::Int,
            Value::Real(_) => FieldType::Real,
            Value::Text(_) => FieldType::Text,
        };
        ty = Some(ty.map_or(t, |old| old.widen(t)));
    }
    ty.unwrap_or(FieldType::Text)
}

/// Converts a value to fit the field type. Used when schemas are merged.
pub fn coerce(v: Value, ty: FieldType) -> Value {
    match (ty, v) {
        (_, Value::Null) => Value::Null,
        (FieldType::Text, Value::Text(s)) => Value::Text(s),
        (FieldType::Text, other) => Value::Text(other.to_text()),
        (FieldType::Real, Value::Int(i)) => Value::Real(i as f64),
        (FieldType::Real, Value::Bool(b)) => Value::Real(b as i64 as f64),
        (FieldType::Int, Value::Bool(b)) => Value::Int(b as i64),
        (_, v) => v,
    }
}

// ---------- Coordinate systems ----------

/// The systems the tool knows without help from GDAL: (EPSG, name, degrees?).
pub const KNOWN_CRS: &[(u32, &str, bool)] = &[
    (3006, "SWEREF 99 TM", false),
    (4326, "WGS 84", true),
    (3007, "SWEREF 99 12 00", false),
    (3008, "SWEREF 99 13 30", false),
    (3012, "SWEREF 99 14 15", false),
    (3009, "SWEREF 99 15 00", false),
    (3013, "SWEREF 99 15 45", false),
    (3010, "SWEREF 99 16 30", false),
    (3014, "SWEREF 99 17 15", false),
    (3011, "SWEREF 99 18 00", false),
    (3015, "SWEREF 99 18 45", false),
    (3016, "SWEREF 99 20 15", false),
    (3017, "SWEREF 99 21 45", false),
    (3018, "SWEREF 99 23 15", false),
];

pub fn crs_label(crs: Option<u32>) -> String {
    match crs {
        None => "no geometry".to_string(),
        Some(code) => match KNOWN_CRS.iter().find(|c| c.0 == code) {
            Some(c) => format!("{} (EPSG:{})", c.1, code),
            None => format!("EPSG:{}", code),
        },
    }
}

/// Parses 3006, "3006" and "EPSG:3006".
pub fn parse_crs(s: &str) -> Result<u32> {
    let t = s.trim();
    let digits = t.strip_prefix("EPSG:").or_else(|| t.strip_prefix("epsg:")).unwrap_or(t);
    digits.parse::<u32>().ok().filter(|c| *c > 0).ok_or_else(|| GbError(format!("\"{}\" is not an EPSG code. Write EPSG:3006, for example.", s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_formatting() {
        assert_eq!(fmt_real(5.0), "5");
        assert_eq!(fmt_real(0.1 + 0.2), "0.3");
        assert_eq!(fmt_real(-16.392000000000053), "-16.392");
        assert_eq!(fmt_real(1234.5), "1234.5");
        assert_eq!(fmt_real((6400326.036_f64 * 1000.0).round() / 1000.0), "6400326.036");
        assert_eq!(fmt_real(10666.109875184911), "10666.1098751849");
        assert_eq!(fmt_real(0.000123), "0.000123");
        assert_eq!(fmt_real(1e20), "100000000000000000000");
        assert_eq!(fmt_real(f64::NAN), "");
    }

    #[test]
    fn join_keys() {
        assert_eq!(Value::Int(1480).key(), Value::Text("1480".into()).key());
        assert_eq!(Value::Real(5.0).key(), Value::Int(5).key());
        assert_ne!(Value::Int(114).key(), Value::Text("0114".into()).key());
        assert_eq!(Value::Null.key(), None);
        assert_eq!(Value::Text(String::new()).key(), None);
    }

    #[test]
    fn types() {
        assert_eq!(infer_type([Value::Int(1), Value::Null, Value::Real(2.5)].iter()), FieldType::Real);
        assert_eq!(infer_type([Value::Int(1), Value::Text("a".into())].iter()), FieldType::Text);
        assert_eq!(infer_type([Value::Null].iter()), FieldType::Text);
        assert_eq!(coerce(Value::Int(3), FieldType::Text), Value::Text("3".into()));
        assert_eq!(parse_crs("EPSG:3006").unwrap(), 3006);
        assert!(parse_crs("sweref").is_err());
        assert_eq!(parse_number("57,7089"), Some(57.7089));
    }
}
