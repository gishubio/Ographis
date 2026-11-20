//! GeoArrow: Arrow IPC files (.arrow, .feather, and streams as .arrows), as data science
//! tools and databases pass tables between them. Read with the geometry as WKB or in the
//! native GeoArrow layouts of points, lines and polygons, also compressed; written with the
//! geometry as WKB and the coordinate system in its metadata. Added in 0.96.5.

use crate::io::Notes;
use crate::model::{Feature, Field, FieldType, Table, Value};
use crate::model::{GbError, Result};
use crate::parquet_io::{epsg_of, from_wkb, to_wkb};
use arrow_array::cast::AsArray;
use arrow_array::types::{Float32Type, Float64Type, Int16Type, Int32Type, Int64Type, Int8Type, UInt16Type, UInt32Type, UInt64Type, UInt8Type};
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field as AField, Schema};
use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const EXTENSION: &str = "ARROW:extension:name";
const EXTENSION_META: &str = "ARROW:extension:metadata";

/// How the geometry of a column is kept.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Encoding {
    Wkb,
    Point,
    LineString,
    Polygon,
    MultiPoint,
    MultiLineString,
    MultiPolygon,
}

/// The geometry column of a schema: its place, how it is kept, and its coordinate system.
fn geometry_column(schema: &Schema) -> Option<(usize, Encoding, Option<u32>)> {
    let crs_of = |f: &AField| -> Option<u32> {
        let meta: serde_json::Value = serde_json::from_str(f.metadata().get(EXTENSION_META)?).ok()?;
        epsg_of(meta.get("crs")?)
    };
    for (i, f) in schema.fields().iter().enumerate() {
        let enc = match f.metadata().get(EXTENSION).map(|s| s.as_str()) {
            Some("geoarrow.wkb" | "ogc.wkb") => Encoding::Wkb,
            Some("geoarrow.point") => Encoding::Point,
            Some("geoarrow.linestring") => Encoding::LineString,
            Some("geoarrow.polygon") => Encoding::Polygon,
            Some("geoarrow.multipoint") => Encoding::MultiPoint,
            Some("geoarrow.multilinestring") => Encoding::MultiLineString,
            Some("geoarrow.multipolygon") => Encoding::MultiPolygon,
            _ => continue,
        };
        return Some((i, enc, crs_of(f)));
    }
    // Without the metadata, a column of bytes by a name that geometry has.
    schema
        .fields()
        .iter()
        .position(|f| {
            matches!(f.data_type(), DataType::Binary | DataType::LargeBinary | DataType::BinaryView)
                && matches!(f.name().to_lowercase().as_str(), "geometry" | "geom" | "wkb" | "wkb_geometry" | "geom_wkb" | "the_geom")
        })
        .map(|i| (i, Encoding::Wkb, None))
}

/// One coordinate of an array of coordinates, kept apart (a struct of x, y and z) or
/// together (a fixed list of x, y and z), with its height when it has one.
fn coord(arr: &dyn Array, i: usize) -> Option<(Coord<f64>, Option<f64>)> {
    let num = |a: &dyn Array, k: usize| -> Option<f64> {
        match a.data_type() {
            DataType::Float64 => Some(a.as_primitive::<Float64Type>().value(k)),
            DataType::Float32 => Some(a.as_primitive::<Float32Type>().value(k) as f64),
            _ => None,
        }
    };
    match arr.data_type() {
        DataType::Struct(fields) => {
            let s = arr.as_struct();
            let at = |name: &str| fields.iter().position(|f| f.name().eq_ignore_ascii_case(name));
            let x = num(s.column(at("x")?).as_ref(), i)?;
            let y = num(s.column(at("y")?).as_ref(), i)?;
            let z = at("z").and_then(|k| num(s.column(k).as_ref(), i));
            Some((Coord { x, y }, z))
        }
        DataType::FixedSizeList(_, n) => {
            let l = arr.as_fixed_size_list();
            let values = l.values();
            let n = *n as usize;
            let start = i * n;
            let x = num(values.as_ref(), start)?;
            let y = num(values.as_ref(), start + 1)?;
            // xyz or xyzm; xym is not told apart here and is rare.
            let z = (n >= 3).then(|| num(values.as_ref(), start + 2)).flatten();
            Some((Coord { x, y }, z))
        }
        _ => None,
    }
}

/// The parts of item i of a list: the range of the items of the next level.
fn range(arr: &dyn Array, i: usize) -> Option<(std::ops::Range<usize>, ArrayRef)> {
    match arr.data_type() {
        DataType::List(_) => {
            let l = arr.as_list::<i32>();
            let o = l.value_offsets();
            Some((o[i] as usize..o[i + 1] as usize, l.values().clone()))
        }
        DataType::LargeList(_) => {
            let l = arr.as_list::<i64>();
            let o = l.value_offsets();
            Some((o[i] as usize..o[i + 1] as usize, l.values().clone()))
        }
        _ => None,
    }
}

fn line(arr: &dyn Array, i: usize, z: &mut Vec<f64>) -> Option<LineString<f64>> {
    let (r, coords) = range(arr, i)?;
    let mut pts = Vec::with_capacity(r.len());
    for k in r {
        let (c, h) = coord(coords.as_ref(), k)?;
        z.push(h.unwrap_or(f64::NAN));
        pts.push(c);
    }
    Some(LineString::new(pts))
}

fn polygon(arr: &dyn Array, i: usize, z: &mut Vec<f64>) -> Option<Polygon<f64>> {
    let (r, rings) = range(arr, i)?;
    let mut all: Vec<LineString<f64>> = r.map(|k| line(rings.as_ref(), k, z)).collect::<Option<_>>()?;
    if all.is_empty() {
        return Some(Polygon::new(LineString::new(vec![]), vec![]));
    }
    let outer = all.remove(0);
    Some(Polygon::new(outer, all))
}

/// The geometry of row i in a native GeoArrow layout, with its heights when it has them.
fn native(arr: &dyn Array, enc: Encoding, i: usize) -> Option<(Geometry<f64>, Option<Vec<f64>>)> {
    let mut z = Vec::new();
    let g = match enc {
        Encoding::Point => {
            let (c, h) = coord(arr, i)?;
            if c.x.is_nan() && c.y.is_nan() {
                return None;
            }
            z.push(h.unwrap_or(f64::NAN));
            Geometry::Point(Point(c))
        }
        Encoding::LineString => Geometry::LineString(line(arr, i, &mut z)?),
        Encoding::Polygon => Geometry::Polygon(polygon(arr, i, &mut z)?),
        Encoding::MultiPoint => Geometry::MultiPoint(MultiPoint::new(line(arr, i, &mut z)?.0.into_iter().map(Point).collect())),
        Encoding::MultiLineString => {
            let (r, lines) = range(arr, i)?;
            Geometry::MultiLineString(MultiLineString::new(r.map(|k| line(lines.as_ref(), k, &mut z)).collect::<Option<_>>()?))
        }
        Encoding::MultiPolygon => {
            let (r, polys) = range(arr, i)?;
            Geometry::MultiPolygon(MultiPolygon::new(r.map(|k| polygon(polys.as_ref(), k, &mut z)).collect::<Option<_>>()?))
        }
        Encoding::Wkb => return None,
    };
    let has_z = z.iter().any(|v| !v.is_nan());
    Some((g, has_z.then_some(z)))
}

/// The value of row i of a column, as Ographis keeps values.
fn value(arr: &dyn Array, i: usize, fmt: Option<&arrow_cast::display::ArrayFormatter>) -> Value {
    if arr.is_null(i) {
        return Value::Null;
    }
    match arr.data_type() {
        DataType::Int8 => Value::Int(arr.as_primitive::<Int8Type>().value(i) as i64),
        DataType::Int16 => Value::Int(arr.as_primitive::<Int16Type>().value(i) as i64),
        DataType::Int32 => Value::Int(arr.as_primitive::<Int32Type>().value(i) as i64),
        DataType::Int64 => Value::Int(arr.as_primitive::<Int64Type>().value(i)),
        DataType::UInt8 => Value::Int(arr.as_primitive::<UInt8Type>().value(i) as i64),
        DataType::UInt16 => Value::Int(arr.as_primitive::<UInt16Type>().value(i) as i64),
        DataType::UInt32 => Value::Int(arr.as_primitive::<UInt32Type>().value(i) as i64),
        DataType::UInt64 => Value::Int(arr.as_primitive::<UInt64Type>().value(i) as i64),
        DataType::Float32 => Value::Real(arr.as_primitive::<Float32Type>().value(i) as f64),
        DataType::Float64 => Value::Real(arr.as_primitive::<Float64Type>().value(i)),
        DataType::Boolean => Value::Int(arr.as_boolean().value(i) as i64),
        DataType::Utf8 => Value::Text(arr.as_string::<i32>().value(i).to_string()),
        DataType::LargeUtf8 => Value::Text(arr.as_string::<i64>().value(i).to_string()),
        DataType::Utf8View => Value::Text(arr.as_string_view().value(i).to_string()),
        _ => fmt.map(|f| Value::Text(f.value(i).to_string())).unwrap_or(Value::Null),
    }
}

fn field_type(t: &DataType) -> FieldType {
    match t {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Boolean => FieldType::Int,
        DataType::Float16 | DataType::Float32 | DataType::Float64 | DataType::Decimal128(..) | DataType::Decimal256(..) => FieldType::Real,
        _ => FieldType::Text,
    }
}

/// Whether the bytes begin an Arrow IPC file, rather than a stream.
fn is_file(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 6];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == b"ARROW1"
}

/// Reads an Arrow IPC file or stream as a table, at most `limit` rows.
pub fn read_arrow(path: &Path, crs_given: Option<u32>, limit: Option<usize>, notes: &mut Notes, going: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let named = |e: arrow_schema::ArrowError| GbError(format!("Cannot read {}: {}", path.display(), e));
    let open = || std::fs::File::open(path).map_err(|e| GbError(format!("Cannot open {}: {}", path.display(), e)));
    let (schema, batches): (Arc<Schema>, Box<dyn Iterator<Item = std::result::Result<RecordBatch, arrow_schema::ArrowError>>>) = if is_file(path) {
        let r = arrow_ipc::reader::FileReader::try_new_buffered(open()?, None).map_err(named)?;
        (r.schema(), Box::new(r))
    } else {
        let r = arrow_ipc::reader::StreamReader::try_new_buffered(open()?, None).map_err(named)?;
        (r.schema(), Box::new(r))
    };
    let geo = geometry_column(&schema);
    let mut t = Table { crs: crs_given.or(geo.and_then(|g| g.2)), ..Default::default() };
    let attr_cols: Vec<usize> = (0..schema.fields().len()).filter(|i| geo.is_none_or(|g| g.0 != *i)).collect();
    t.fields = attr_cols.iter().map(|i| Field::new(schema.field(*i).name().clone(), field_type(schema.field(*i).data_type()))).collect();
    let mut unread = 0usize;
    'batches: for b in batches {
        let b = b.map_err(named)?;
        let formats: Vec<Option<arrow_cast::display::ArrayFormatter>> = attr_cols
            .iter()
            .map(|i| match field_type(b.column(*i).data_type()) {
                FieldType::Text => arrow_cast::display::ArrayFormatter::try_new(b.column(*i).as_ref(), &Default::default()).ok(),
                _ => None,
            })
            .collect();
        for row in 0..b.num_rows() {
            if limit.is_some_and(|l| t.features.len() >= l) {
                break 'batches;
            }
            let attrs = attr_cols.iter().zip(&formats).map(|(i, f)| value(b.column(*i).as_ref(), row, f.as_ref())).collect();
            let mut feature = Feature::new(attrs, None);
            if let Some((gi, enc, _)) = geo {
                let col = b.column(gi);
                if !col.is_null(row) {
                    let got = if enc == Encoding::Wkb {
                        let bytes: Option<&[u8]> = match col.data_type() {
                            DataType::Binary => Some(col.as_binary::<i32>().value(row)),
                            DataType::LargeBinary => Some(col.as_binary::<i64>().value(row)),
                            DataType::BinaryView => Some(col.as_binary_view().value(row)),
                            _ => None,
                        };
                        bytes.and_then(|b| from_wkb(b).ok())
                    } else {
                        native(col.as_ref(), enc, row)
                    };
                    match got {
                        Some((g, z)) => {
                            feature.geom = Some(g);
                            feature.z = z.map(Into::into);
                        }
                        None => unread += 1,
                    }
                }
            }
            t.features.push(feature);
        }
        if !going(t.features.len() as u64, None) {
            return Err(GbError("Stopped.".into()));
        }
    }
    if unread > 0 {
        notes.warn(format!("{} geometries could not be read, and the features have none.", unread));
    }
    if geo.is_none() {
        notes.info("the file has no geometry column, and is read as a table");
    }
    Ok(t)
}

/// Writes a table as an Arrow IPC file, compressed with Zstandard, the geometry as WKB in
/// a column named geometry with the GeoArrow metadata. Returns the number of rows.
pub fn write_arrow(t: &Table, path: &Path) -> Result<u64> {
    use arrow_array::builder::{BinaryBuilder, Float64Builder, Int64Builder, StringBuilder};
    let named = |e: arrow_schema::ArrowError| GbError(format!("Cannot write {}: {}", path.display(), e));
    let has_geom = t.has_geometry();
    let mut fields: Vec<AField> = t
        .fields
        .iter()
        .map(|f| {
            let dt = match f.ty {
                FieldType::Int => DataType::Int64,
                FieldType::Real => DataType::Float64,
                FieldType::Text => DataType::Utf8,
            };
            AField::new(f.name.clone(), dt, true)
        })
        .collect();
    let geom_name = if t.field_index("geometry").is_some() { "geometry_wkb" } else { "geometry" };
    if has_geom {
        let mut meta = HashMap::new();
        meta.insert(EXTENSION.to_string(), "geoarrow.wkb".to_string());
        let crs = match t.crs {
            Some(c) => serde_json::json!({"crs": format!("EPSG:{}", c), "crs_type": "authority_code"}),
            None => serde_json::json!({}),
        };
        meta.insert(EXTENSION_META.to_string(), crs.to_string());
        fields.push(AField::new(geom_name, DataType::Binary, true).with_metadata(meta));
    }
    let schema = Arc::new(Schema::new(fields));
    let file = std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    let options = arrow_ipc::writer::IpcWriteOptions::default().try_with_compression(Some(arrow_ipc::CompressionType::ZSTD)).map_err(named)?;
    let mut w = arrow_ipc::writer::FileWriter::try_new_with_options(std::io::BufWriter::new(file), &schema, options).map_err(named)?;
    for chunk in t.features.chunks(65_536) {
        let mut cols: Vec<ArrayRef> = Vec::new();
        for (k, f) in t.fields.iter().enumerate() {
            let col: ArrayRef = match f.ty {
                FieldType::Int => {
                    let mut b = Int64Builder::with_capacity(chunk.len());
                    for feat in chunk {
                        match &feat.attrs[k] {
                            Value::Null => b.append_null(),
                            v => b.append_option(v.as_f64().map(|x| x as i64)),
                        }
                    }
                    Arc::new(b.finish())
                }
                FieldType::Real => {
                    let mut b = Float64Builder::with_capacity(chunk.len());
                    for feat in chunk {
                        b.append_option(feat.attrs[k].as_f64());
                    }
                    Arc::new(b.finish())
                }
                FieldType::Text => {
                    let mut b = StringBuilder::with_capacity(chunk.len(), chunk.len() * 8);
                    for feat in chunk {
                        match &feat.attrs[k] {
                            Value::Null => b.append_null(),
                            v => b.append_value(v.to_text()),
                        }
                    }
                    Arc::new(b.finish())
                }
            };
            cols.push(col);
        }
        if has_geom {
            let mut b = BinaryBuilder::with_capacity(chunk.len(), chunk.len() * 64);
            for feat in chunk {
                match &feat.geom {
                    Some(g) => b.append_value(to_wkb(g, feat.z.as_deref())),
                    None => b.append_null(),
                }
            }
            cols.push(Arc::new(b.finish()));
        }
        let batch = RecordBatch::try_new(schema.clone(), cols).map_err(named)?;
        w.write(&batch).map_err(named)?;
    }
    w.finish().map_err(named)?;
    Ok(t.features.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{point, polygon};

    #[test]
    fn a_table_goes_through_geoarrow_and_back() {
        let dir = std::env::temp_dir().join(format!("og-arrow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = Table {
            fields: vec![Field::new("id", FieldType::Int), Field::new("name", FieldType::Text), Field::new("beds", FieldType::Real)],
            crs: Some(3006),
            ..Default::default()
        };
        t.features.push(Feature::new(
            vec![Value::Int(1), Value::Text("Sahlgrenska".into()), Value::Real(1700.5)],
            Some(Geometry::Point(point!(x: 319500.0, y: 6399600.0))),
        ));
        t.features.push(Feature::new(
            vec![Value::Int(2), Value::Null, Value::Real(2.0)],
            Some(Geometry::Polygon(polygon![(x: 0.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 10.0), (x: 0.0, y: 0.0)])),
        ));
        t.features.push(Feature::new(vec![Value::Int(3), Value::Text("no place".into()), Value::Null], None));
        let path = dir.join("hospitals.arrow");
        assert_eq!(write_arrow(&t, &path).unwrap(), 3);
        let mut notes = Notes::default();
        let back = read_arrow(&path, None, None, &mut notes, &|_, _| true).unwrap();
        assert_eq!(back.crs, Some(3006));
        assert_eq!(back.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["id", "name", "beds"]);
        assert_eq!(back.features.len(), 3);
        for (a, b) in t.features.iter().zip(&back.features) {
            assert_eq!(a.attrs, b.attrs);
            assert_eq!(a.geom, b.geom);
        }
        let first = read_arrow(&path, None, Some(1), &mut notes, &|_, _| true).unwrap();
        assert_eq!(first.features.len(), 1, "a limit stops the reading");
    }

    #[test]
    fn native_geoarrow_points_and_polygons_are_read() {
        use arrow_array::builder::{Float64Builder, ListBuilder, StructBuilder};
        let dir = std::env::temp_dir().join(format!("og-arrow-native-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Polygons as list<list<struct<x, y>>>, as GDAL and geoarrow-python write them.
        let xy = || vec![AField::new("x", DataType::Float64, false), AField::new("y", DataType::Float64, false)];
        let coords = StructBuilder::new(xy(), vec![Box::new(Float64Builder::new()), Box::new(Float64Builder::new())]);
        let mut polys = ListBuilder::new(ListBuilder::new(coords));
        for (x, y) in [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 0.0)] {
            let s = polys.values().values();
            s.field_builder::<Float64Builder>(0).unwrap().append_value(x);
            s.field_builder::<Float64Builder>(1).unwrap().append_value(y);
            s.append(true);
        }
        polys.values().append(true);
        polys.append(true);
        let arr = polys.finish();
        let mut meta = HashMap::new();
        meta.insert(EXTENSION.to_string(), "geoarrow.polygon".to_string());
        meta.insert(EXTENSION_META.to_string(), r#"{"crs": {"id": {"authority": "EPSG", "code": 3006}}}"#.to_string());
        let schema = Arc::new(Schema::new(vec![AField::new("geometry", arr.data_type().clone(), true).with_metadata(meta)]));
        let path = dir.join("areas.arrows");
        let mut w = arrow_ipc::writer::StreamWriter::try_new(std::fs::File::create(&path).unwrap(), &schema).unwrap();
        w.write(&RecordBatch::try_new(schema.clone(), vec![Arc::new(arr)]).unwrap()).unwrap();
        w.finish().unwrap();
        let back = read_arrow(&path, None, None, &mut Notes::default(), &|_, _| true).unwrap();
        assert_eq!(back.crs, Some(3006));
        let Some(Geometry::Polygon(p)) = &back.features[0].geom else { panic!("{:?}", back.features[0].geom) };
        assert_eq!(p.exterior().0.len(), 4);
        assert_eq!(geo::Area::unsigned_area(p), 8.0);
    }
}
