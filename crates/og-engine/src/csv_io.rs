//! CSV and txt, read and written without GDAL. Handles what Swedish and other European
//! files tend to contain: semicolons, decimal commas, Windows-1252 next to UTF-8, and codes
//! with a leading zero, which are kept as text.

use crate::geom;
use crate::io::Notes;
use crate::model::{err, fmt_real, infer_type, parse_number, Feature, Field, FieldType, GbError, Result, Table, Value};
use geo::{Geometry, Point};
use std::path::Path;
use wkt::{ToWkt, TryFromWkt};

#[derive(Debug, Default, Clone)]
pub struct CsvReadOpts {
    pub delimiter: Option<char>,
    pub encoding: Option<String>,
    pub x: Option<String>,
    pub y: Option<String>,
    pub wkt: Option<String>,
    pub crs: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeomMode {
    Auto,
    Xy,
    Wkt,
    None,
}

#[derive(Debug, Clone)]
pub struct CsvWriteOpts {
    pub delimiter: char,
    pub decimal_comma: bool,
    /// "utf-8", "utf-8-bom" or "windows-1252".
    pub encoding: String,
    pub geometry: GeomMode,
    /// Decides the number of decimals in coordinates: 7 for degrees, 3 for metres.
    pub geographic: Option<bool>,
}

impl Default for CsvWriteOpts {
    fn default() -> Self {
        CsvWriteOpts { delimiter: ';', decimal_comma: false, encoding: "utf-8".into(), geometry: GeomMode::Auto, geographic: None }
    }
}

// Column names that are taken as coordinates. The Swedish ones are there on purpose.
const X_NAMES: &[&str] = &["x", "lon", "long", "lng", "longitude", "e", "east", "easting", "ost", "öst", "xkoord", "x_koord"];
const Y_NAMES: &[&str] = &["y", "lat", "latitude", "n", "north", "northing", "nord", "ykoord", "y_koord"];
const WKT_NAMES: &[&str] = &["wkt", "geom", "geometry", "geometri", "the_geom"];

/// Turns bytes into text. Without a given encoding UTF-8 is tried first, then Windows-1252.
pub fn decode(bytes: &[u8], encoding: Option<&str>, notes: &mut Notes) -> Result<String> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Some(label) = encoding.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let enc = encoding_rs::Encoding::for_label(label.as_bytes())
            .ok_or_else(|| GbError(format!("Unknown character encoding: {}. Try utf-8 or windows-1252.", label)))?;
        let (text, _, bad) = enc.decode(bytes);
        if bad {
            notes.warn(format!("The file contains characters that are not valid {}.", enc.name()));
        }
        return Ok(text.into_owned());
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            notes.info("The file is not UTF-8 and was read as Windows-1252.");
            Ok(encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned())
        }
    }
}

fn detect_delimiter(line: &str) -> char {
    let mut best = (',', 0usize);
    for d in [';', ',', '\t', '|'] {
        let n = split_line_count(line, d);
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

fn split_line_count(line: &str, d: char) -> usize {
    let mut q = false;
    line.chars()
        .filter(|&c| {
            if c == '"' {
                q = !q;
            }
            c == d && !q
        })
        .count()
}

fn parse_rows(text: &str, d: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let (mut row, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            quoted = true;
        } else if c == d {
            row.push(std::mem::take(&mut cur));
        } else if c == '\n' {
            row.push(std::mem::take(&mut cur));
            rows.push(std::mem::take(&mut row));
        } else if c != '\r' {
            cur.push(c);
        }
    }
    if !cur.is_empty() || !row.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    rows.retain(|r| r.len() > 1 || r.first().is_some_and(|s| !s.trim().is_empty()));
    rows
}

/// A column of texts as values: whole numbers or decimal numbers when every text in it
/// looks like one, the way a CSV column is read, and text otherwise. Empty texts are empty.
pub fn typed_column(texts: &[Option<String>]) -> Vec<Value> {
    let shapes: Vec<Option<bool>> = texts.iter().flatten().filter(|t| !t.is_empty()).map(|t| number_shape(t, false)).collect();
    let ty = if shapes.is_empty() || shapes.iter().any(|s| s.is_none()) {
        FieldType::Text
    } else if shapes.iter().all(|s| *s == Some(true)) {
        FieldType::Int
    } else {
        FieldType::Real
    };
    texts
        .iter()
        .map(|t| match t.as_deref() {
            None | Some("") => Value::Null,
            Some(v) => match ty {
                FieldType::Int => v.parse::<i64>().map(Value::Int).unwrap_or_else(|_| Value::Text(v.to_string())),
                FieldType::Real => parse_number(v).map(Value::Real).unwrap_or(Value::Null),
                FieldType::Text => Value::Text(v.to_string()),
            },
        })
        .collect()
}

/// Does the text look like a number? A leading zero (0114) makes it text, except in 0 and 0.5.
fn number_shape(s: &str, decimal_comma: bool) -> Option<bool> {
    let t = s.strip_prefix('-').unwrap_or(s);
    let (int_part, frac) = match t.find(|c| c == '.' || (decimal_comma && c == ',')) {
        Some(i) => (&t[..i], Some(&t[i + 1..])),
        None => (t, None),
    };
    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if int_part.len() > 1 && int_part.starts_with('0') {
        return None;
    }
    match frac {
        None => Some(true),
        Some(f) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => Some(false),
        _ => None,
    }
}

pub fn read_csv(path: &Path, opts: &CsvReadOpts, notes: &mut Notes) -> Result<Table> {
    read_csv_with(path, opts, notes, &|_, _| true)
}

/// Reads as `read_csv` does, and tells `progress` how many rows have become features.
pub fn read_csv_with(path: &Path, opts: &CsvReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let bytes = std::fs::read(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let text = decode(&bytes, opts.encoding.as_deref(), notes)?;
    parse_csv_with(&text, opts, notes, progress)
}

pub fn parse_csv(text: &str, opts: &CsvReadOpts, notes: &mut Notes) -> Result<Table> {
    parse_csv_with(text, opts, notes, &|_, _| true)
}

fn parse_csv_with(text: &str, opts: &CsvReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let first = text.lines().find(|l| !l.trim().is_empty()).ok_or_else(|| GbError("The file is empty.".into()))?;
    let d = opts.delimiter.unwrap_or_else(|| detect_delimiter(first));
    let mut rows = parse_rows(text, d);
    if rows.is_empty() {
        return err("The file is empty.");
    }
    let mut names: Vec<String> = Vec::new();
    for (i, h) in rows.remove(0).into_iter().enumerate() {
        let base = if h.trim().is_empty() { format!("column_{}", i + 1) } else { h.trim().to_string() };
        let mut name = base.clone();
        let mut k = 2;
        while names.contains(&name) {
            name = format!("{}_{}", base, k);
            k += 1;
        }
        names.push(name);
    }
    let ncol = names.len();
    let decimal_comma = d != ',';

    // Type per column: integer, decimal or text.
    let mut types = Vec::with_capacity(ncol);
    for c in 0..ncol {
        let (mut any, mut all_int, mut numeric) = (false, true, true);
        for r in &rows {
            let v = r.get(c).map(|s| s.trim()).unwrap_or("");
            if v.is_empty() {
                continue;
            }
            any = true;
            match number_shape(v, decimal_comma) {
                Some(is_int) => all_int &= is_int && v.parse::<i64>().is_ok(),
                None => {
                    numeric = false;
                    break;
                }
            }
        }
        types.push(if !any || !numeric {
            FieldType::Text
        } else if all_int {
            FieldType::Int
        } else {
            FieldType::Real
        });
    }

    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let find = |wanted: &Option<String>, candidates: &[&str]| -> Result<Option<usize>> {
        if let Some(w) = wanted.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return match names.iter().position(|n| n == w).or_else(|| lower.iter().position(|n| *n == w.to_lowercase())) {
                Some(i) => Ok(Some(i)),
                None => err(format!("There is no column \"{}\". Columns: {}.", w, names.join(", "))),
            };
        }
        Ok(candidates.iter().find_map(|c| lower.iter().position(|n| n == c)))
    };
    let wkt_col = find(&opts.wkt, WKT_NAMES)?.filter(|_| opts.x.is_none() && opts.y.is_none() || opts.wkt.is_some());
    let (mut xi, mut yi) = if wkt_col.is_some() { (None, None) } else { (find(&opts.x, X_NAMES)?, find(&opts.y, Y_NAMES)?) };
    if xi.is_none() != yi.is_none() {
        notes.warn("Only one of the coordinate columns was found, so the file is read without geometry. Set x and y in the input.");
        xi = None;
        yi = None;
    }

    let mut features = Vec::with_capacity(rows.len());
    let (mut bad_geom, mut short_rows) = (0usize, 0usize);
    let total = Some(rows.len() as u64);
    for (k, r) in rows.iter().enumerate() {
        if k % 1_000 == 0 && !progress(k as u64, total) {
            return Err(GbError(crate::model::STOPPED.into()));
        }
        if r.len() != ncol {
            short_rows += 1;
        }
        let attrs: Vec<Value> = (0..ncol)
            .map(|c| {
                let v = r.get(c).map(|s| s.trim()).unwrap_or("");
                if v.is_empty() {
                    return Value::Null;
                }
                match types[c] {
                    FieldType::Int => v.parse::<i64>().map(Value::Int).unwrap_or(Value::Null),
                    FieldType::Real => parse_number(v).map(Value::Real).unwrap_or(Value::Null),
                    FieldType::Text => Value::Text(v.to_string()),
                }
            })
            .collect();
        let mut geom = None;
        if let Some(w) = wkt_col {
            let s = r.get(w).map(|s| s.trim()).unwrap_or("");
            if !s.is_empty() {
                match Geometry::<f64>::try_from_wkt_str(s) {
                    Ok(g) => geom = Some(geom::normalize(g)),
                    Err(_) => bad_geom += 1,
                }
            }
        } else if let (Some(x), Some(y)) = (xi, yi) {
            let get = |i: usize| r.get(i).and_then(|s| parse_number(s));
            match (get(x), get(y)) {
                (Some(px), Some(py)) => geom = Some(Geometry::Point(Point::new(px, py))),
                _ => bad_geom += 1,
            }
        }
        // The WKT column becomes the geometry and is not kept as an attribute. It would
        // otherwise stay behind with stale content when the geometry is clipped or reprojected.
        let mut attrs = attrs;
        if let Some(w) = wkt_col {
            attrs.remove(w);
        }
        features.push(Feature { z: None, attrs, geom });
    }
    progress(rows.len() as u64, total);
    if short_rows > 0 {
        notes.warn(format!("{} rows have a different number of columns than the header row.", short_rows));
    }
    if bad_geom > 0 {
        notes.warn(format!("{} rows have no valid coordinates and got no geometry.", bad_geom));
    }

    let has_geom_cols = wkt_col.is_some() || xi.is_some();
    let mut crs = if has_geom_cols { opts.crs } else { None };
    if has_geom_cols && crs.is_none() {
        crs = Some(guess_crs(&mut features, xi.map(|i| lower[i].as_str()), notes)?);
    }
    notes.info(match (wkt_col, xi, yi) {
        (Some(w), _, _) => format!("geometry from {}", names[w]),
        (_, Some(x), Some(y)) => format!("points from {}/{}", names[x], names[y]),
        _ => "no geometry".to_string(),
    });
    let fields = names.into_iter().zip(types).enumerate().filter(|(i, _)| Some(*i) != wkt_col).map(|(_, (n, t))| Field::new(n, t)).collect();
    Ok(Table { mesh: None, fields, crs, features, raster: false, image: None, grid: None })
}

/// Guesses the coordinate system from the values when the reader was given none. The guess is always logged.
fn guess_crs(features: &mut [Feature], x_name: Option<&str>, notes: &mut Notes) -> Result<u32> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for f in features.iter() {
        if let Some(g) = &f.geom {
            use geo::BoundingRect;
            if let Some(r) = g.bounding_rect() {
                b = [b[0].min(r.min().x), b[1].min(r.min().y), b[2].max(r.max().x), b[3].max(r.max().y)];
            }
        }
    }
    if !b[0].is_finite() {
        return Ok(4326);
    }
    let northing = |lo: f64, hi: f64| lo >= 6_100_000.0 && hi <= 7_700_000.0;
    let easting = |lo: f64, hi: f64| lo >= 200_000.0 && hi <= 1_000_000.0;
    if b[0] >= -180.0 && b[2] <= 180.0 && b[1] >= -90.0 && b[3] <= 90.0 {
        notes.info("no coordinate system was given, the values are degrees and are read as WGS 84");
        return Ok(4326);
    }
    if easting(b[0], b[2]) && northing(b[1], b[3]) {
        notes.warn("No coordinate system was given. The values look like SWEREF 99 TM and are read as such. Set crs in the input if that is wrong.");
        return Ok(3006);
    }
    if northing(b[0], b[2]) && easting(b[1], b[3]) {
        for f in features.iter_mut() {
            if let Some(Geometry::Point(p)) = &mut f.geom {
                *p = Point::new(p.y(), p.x());
            }
        }
        notes.warn(format!(
            "The column {} holds northings, so x and y were swapped. The values are read as SWEREF 99 TM. Set crs in the input if that is wrong.",
            x_name.unwrap_or("x")
        ));
        return Ok(3006);
    }
    err("The coordinate system cannot be guessed from the values. Set crs in the input, for example EPSG:3006.")
}

fn quote(s: &str, d: char) -> String {
    if s.contains(d) || s.contains('"') || s.contains('\n') || s.contains('\r') || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn free_name(table: &Table, wanted: &str, fallback: &str) -> String {
    if table.field_index(wanted).is_none() {
        wanted.to_string()
    } else {
        fallback.to_string()
    }
}

/// The table without geometry, with the geometry as ordinary columns instead: x and y
/// for points, WKT for anything else. For formats that have no geometry of their own.
pub fn geometry_as_columns(table: &Table, mode: GeomMode, geographic: Option<bool>) -> Table {
    let mode = match mode {
        GeomMode::Auto if !table.has_geometry() => GeomMode::None,
        GeomMode::Auto => {
            if table.features.iter().all(|f| matches!(f.geom, None | Some(Geometry::Point(_)))) {
                GeomMode::Xy
            } else {
                GeomMode::Wkt
            }
        }
        m => m,
    };
    let decimals = match geographic {
        Some(true) => 7,
        Some(false) => 3,
        None => 6,
    };
    let round = |v: f64| {
        let k = 10f64.powi(decimals);
        (v * k).round() / k
    };
    let mut fields = table.fields.clone();
    match mode {
        GeomMode::Xy => {
            fields.push(Field::new(free_name(table, "x", "geom_x"), FieldType::Real));
            fields.push(Field::new(free_name(table, "y", "geom_y"), FieldType::Real));
        }
        GeomMode::Wkt => fields.push(Field::new(free_name(table, "wkt", "geom_wkt"), FieldType::Text)),
        _ => {}
    }
    let features = table
        .features
        .iter()
        .map(|f| {
            let mut attrs = f.attrs.clone();
            match mode {
                GeomMode::Xy => match &f.geom {
                    Some(Geometry::Point(p)) => attrs.extend([Value::Real(round(p.x())), Value::Real(round(p.y()))]),
                    _ => attrs.extend([Value::Null, Value::Null]),
                },
                GeomMode::Wkt => attrs.push(f.geom.as_ref().map_or(Value::Null, |g| Value::Text(geom::rounded(g, decimals).wkt_string()))),
                _ => {}
            }
            Feature { z: None, attrs, geom: None }
        })
        .collect();
    Table { mesh: None, fields, crs: None, features, raster: false, image: None, grid: None }
}

pub fn csv_text(table: &Table, opts: &CsvWriteOpts) -> Result<String> {
    let d = opts.delimiter;
    if opts.decimal_comma && d == ',' {
        return err("A decimal comma does not work with a comma as the delimiter. Choose semicolon.");
    }
    let flat = geometry_as_columns(table, opts.geometry, opts.geographic);
    let real = |r: f64| if opts.decimal_comma { fmt_real(r).replace('.', ",") } else { fmt_real(r) };
    let sep = d.to_string();
    let mut out = flat.fields.iter().map(|f| quote(&f.name, d)).collect::<Vec<_>>().join(&sep);
    out.push('\n');
    for f in &flat.features {
        let row: Vec<String> = f
            .attrs
            .iter()
            .map(|v| match v {
                Value::Real(r) => real(*r),
                other => quote(&other.to_text(), d),
            })
            .collect();
        out.push_str(&row.join(&sep));
        out.push('\n');
    }
    Ok(out)
}

pub fn write_csv(table: &Table, path: &Path, opts: &CsvWriteOpts, notes: &mut Notes) -> Result<()> {
    let text = csv_text(table, opts)?;
    let bytes = encode(text, &opts.encoding, true, notes)?;
    std::fs::write(path, bytes).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))
}

/// Adds the rows of the table to the end of a file that is there, under the names in its
/// first row. Attributes the file has no column for are left out, with a warning, and
/// columns the stream lacks are left empty. A file that is not there is written whole.
pub fn append_csv(table: &Table, path: &Path, opts: &CsvWriteOpts, notes: &mut Notes) -> Result<()> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return write_csv(table, path, opts, notes),
    };
    let label = opts.encoding.trim().to_lowercase().replace("-bom", "");
    let text = decode(&bytes, Some(&label).filter(|l| !l.is_empty()).map(|l| l.as_str()), &mut Notes::default())?;
    let Some(first) = text.lines().find(|l| !l.trim().is_empty()) else { return write_csv(table, path, opts, notes) };
    let header: Vec<String> = parse_rows(first, opts.delimiter).into_iter().next().unwrap_or_default().into_iter().map(|h| h.trim().to_string()).collect();
    let flat = geometry_as_columns(table, opts.geometry, opts.geographic);
    let left_out: Vec<&str> = flat.fields.iter().map(|f| f.name.as_str()).filter(|n| !header.iter().any(|h| h == n)).collect();
    if !left_out.is_empty() {
        notes.warn(format!("The file has no column for {}, so they were left out. Write to a new file to get them.", left_out.join(", ")));
    }
    let idx: Vec<Option<usize>> = header.iter().map(|h| flat.field_index(h)).collect();
    if idx.iter().all(|i| i.is_none()) && !flat.fields.is_empty() {
        return err(format!(
            "None of the columns in {} has the name of an attribute in the stream. Is the delimiter right? The first row is: {}",
            path.display(),
            first
        ));
    }
    let lined = Table {
        mesh: None,
        fields: header.iter().zip(&idx).map(|(h, i)| Field::new(h.clone(), i.map_or(FieldType::Text, |i| flat.fields[i].ty))).collect(),
        crs: None,
        features: flat
            .features
            .iter()
            .map(|f| Feature { z: None, attrs: idx.iter().map(|i| i.map_or(Value::Null, |i| f.attrs[i].clone())).collect(), geom: None })
            .collect(),
        raster: false,
        image: None,
        grid: None,
    };
    let plain = CsvWriteOpts { geometry: GeomMode::None, ..opts.clone() };
    let rows = csv_text(&lined, &plain)?;
    let body = rows.split_once('\n').map_or("", |x| x.1).to_string();
    let mut out = encode(body, &opts.encoding, false, notes)?;
    if !bytes.ends_with(b"\n") && !bytes.is_empty() {
        out.insert(0, b'\n');
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).map_err(|e| GbError(format!("Cannot add to {}: {}", path.display(), e)))?;
    file.write_all(&out).map_err(|e| GbError(format!("Cannot add to {}: {}", path.display(), e)))
}

/// Text as bytes in a character encoding. A byte order mark is only written at the start
/// of a file.
fn encode(text: String, encoding: &str, start: bool, notes: &mut Notes) -> Result<Vec<u8>> {
    Ok(match encoding.trim().to_lowercase().as_str() {
        "" | "utf-8" | "utf8" => text.into_bytes(),
        "utf-8-bom" | "utf8-bom" if start => [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
        "utf-8-bom" | "utf8-bom" => text.into_bytes(),
        label => {
            let enc = encoding_rs::Encoding::for_label(label.as_bytes())
                .ok_or_else(|| GbError(format!("Unknown character encoding: {}. Try utf-8, utf-8-bom or windows-1252.", label)))?;
            let (b, _, lossy) = enc.encode(&text);
            if lossy {
                notes.warn(format!("Some characters do not exist in {} and were written as character codes.", enc.name()));
            }
            b.into_owned()
        }
    })
}

/// The field type of a new attribute, worked out from the values.
pub fn type_of_column(table: &Table, idx: usize) -> FieldType {
    infer_type(table.features.iter().map(|f| &f.attrs[idx]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, opts: CsvReadOpts) -> (Table, Notes) {
        let mut n = Notes::default();
        let t = parse_csv(text, &opts, &mut n).unwrap();
        (t, n)
    }

    #[test]
    fn semicolons_and_decimal_commas() {
        let (t, _) = read(
            "code;name;population;share;lat;lon\n0114;\"Upplands Väsby; north\";48000;12,5;59,5184;17,9113\n1480;Göteborg;600000;;57,7089;11,9746\n",
            CsvReadOpts::default(),
        );
        let types: Vec<FieldType> = t.fields.iter().map(|f| f.ty).collect();
        assert_eq!(types, [FieldType::Text, FieldType::Text, FieldType::Int, FieldType::Real, FieldType::Real, FieldType::Real]);
        assert_eq!(t.features[0].attrs[0], Value::Text("0114".into()));
        assert_eq!(t.features[0].attrs[1], Value::Text("Upplands Väsby; north".into()));
        assert_eq!(t.features[0].attrs[3], Value::Real(12.5));
        assert_eq!(t.features[1].attrs[3], Value::Null);
        assert_eq!(t.crs, Some(4326));
        assert_eq!(t.features[1].geom, Some(Geometry::Point(Point::new(11.9746, 57.7089))));
    }

    #[test]
    fn comma_csv_keeps_dot_decimals() {
        let (t, _) = read("a,b,c\n1,2.5,x\n-3,0.25,\"y,z\"\n", CsvReadOpts::default());
        assert_eq!(t.fields[0].ty, FieldType::Int);
        assert_eq!(t.fields[1].ty, FieldType::Real);
        assert_eq!(t.features[1].attrs[2], Value::Text("y,z".into()));
        assert_eq!(t.crs, None);
    }

    #[test]
    fn sweref_guess_and_swapped_axes() {
        let (t, n) = read("name;e;n\nGöteborg;319758;6400326\n", CsvReadOpts::default());
        assert_eq!(t.crs, Some(3006));
        assert_eq!(n.warnings.len(), 1);
        // The old Swedish habit: x = north.
        let (t, n) = read("name;x;y\nGöteborg;6400326;319758\n", CsvReadOpts::default());
        assert_eq!(t.crs, Some(3006));
        assert_eq!(t.features[0].geom, Some(Geometry::Point(Point::new(319758., 6400326.))));
        assert!(n.warnings[0].contains("swapped"));
        // A given coordinate system is never second-guessed.
        let (t, n) = read("x;y\n150000;6400000\n", CsvReadOpts { crs: Some(3007), ..Default::default() });
        assert_eq!(t.crs, Some(3007));
        assert!(n.warnings.is_empty());
        // Values that look like nothing known give an error instead of a guess.
        let mut notes = Notes::default();
        assert!(parse_csv("x;y\n1500000;6400000\n", &CsvReadOpts::default(), &mut notes).is_err());
    }

    #[test]
    fn wkt_column_and_encoding() {
        let (t, _) = read("id;wkt\n1;\"LINESTRING (0 0, 10 0)\"\n2;POINT (5 5)\n", CsvReadOpts { crs: Some(3006), ..Default::default() });
        assert_eq!(t.field_names(), ["id"]);
        assert!(matches!(t.features[0].geom, Some(Geometry::LineString(_))));
        assert!(matches!(t.features[1].geom, Some(Geometry::Point(_))));
        let mut n = Notes::default();
        // "Åmål" in Windows-1252 is not valid UTF-8.
        assert_eq!(decode(&[0xC5, b'm', 0xE5, b'l'], None, &mut n).unwrap(), "Åmål");
        assert_eq!(n.info.len(), 1);
        assert_eq!(decode("Åmål".as_bytes(), None, &mut n).unwrap(), "Åmål");
        assert_eq!(decode(&[0xEF, 0xBB, 0xBF, b'a'], None, &mut n).unwrap(), "a");
    }

    #[test]
    fn round_trip() {
        let (t, _) = read("code;name;v;lat;lon\n0114;\"a;b\";1,5;59,5;17,9\n", CsvReadOpts::default());
        let text = csv_text(&t, &CsvWriteOpts { geographic: Some(true), ..Default::default() }).unwrap();
        assert_eq!(text, "code;name;v;lat;lon;x;y\n0114;\"a;b\";1.5;59.5;17.9;17.9;59.5\n");
        let text = csv_text(&t, &CsvWriteOpts { decimal_comma: true, geometry: GeomMode::Wkt, ..Default::default() }).unwrap();
        assert_eq!(text, "code;name;v;lat;lon;wkt\n0114;\"a;b\";1,5;59,5;17,9;POINT(17.9 59.5)\n");
        assert!(csv_text(&t, &CsvWriteOpts { delimiter: ',', decimal_comma: true, ..Default::default() }).is_err());
    }
}
