//! Photos with their places: JPEG pictures from a camera or a phone that keep where they were
//! taken in their Exif data, as points with the time, the direction and the camera.

use crate::model::{Feature, Field, FieldType, GbError, Result, Table, Value};
use std::path::Path;

/// Whether a JPEG is a photo, rather than a picture placed on a map by a world file.
pub fn is_photo(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "jpg" | "jpeg") {
        return false;
    }
    let placed = ["jgw", "jpgw", "jpegw", "wld"].iter().any(|w| path.with_extension(w).exists()) || path.with_extension(format!("{}.aux.xml", ext)).exists();
    !placed
}

/// The fields of a stream of photos.
pub fn fields() -> Vec<Field> {
    vec![
        Field::new("file", FieldType::Text),
        Field::new("path", FieldType::Text),
        Field::new("taken", FieldType::Text),
        Field::new("latitude", FieldType::Real),
        Field::new("longitude", FieldType::Real),
        Field::new("altitude", FieldType::Real),
        Field::new("direction", FieldType::Real),
        Field::new("make", FieldType::Text),
        Field::new("model", FieldType::Text),
        Field::new("width", FieldType::Int),
        Field::new("height", FieldType::Int),
    ]
}

/// Degrees, minutes and seconds as degrees.
fn degrees(v: &exif::Value) -> Option<f64> {
    match v {
        exif::Value::Rational(r) if !r.is_empty() => {
            let part = |k: usize| r.get(k).map_or(0.0, |x| x.to_f64());
            Some(part(0) + part(1) / 60.0 + part(2) / 3600.0)
        }
        _ => None,
    }
}

fn number(f: Option<&exif::Field>) -> Option<f64> {
    match &f?.value {
        exif::Value::Rational(r) => r.first().map(|x| x.to_f64()),
        exif::Value::SRational(r) => r.first().map(|x| x.to_f64()),
        v => v.get_uint(0).map(|u| u as f64),
    }
}

fn text(f: Option<&exif::Field>) -> Option<String> {
    match &f?.value {
        exif::Value::Ascii(parts) => parts.first().map(|b| String::from_utf8_lossy(b).trim().trim_end_matches('\0').to_string()).filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// One photo as a feature: a point in degrees where it was taken, or no geometry when the
/// photo does not say.
pub fn read_photo(path: &Path) -> Result<Feature> {
    use exif::{In, Tag};
    let file = std::fs::File::open(path).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut attrs = vec![Value::Null; fields().len()];
    attrs[0] = Value::Text(name);
    attrs[1] = Value::Text(path.display().to_string());
    let data = match exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file)) {
        Ok(d) => d,
        // A picture without Exif is a photo without a place.
        Err(_) => return Ok(Feature::new(attrs, None)),
    };
    let get = |t: Tag| data.get_field(t, In::PRIMARY);
    // Taken: 2026:10:30 12:41:07 becomes 2026-10-30 12:41:07.
    if let Some(t) = text(get(Tag::DateTimeOriginal)).or_else(|| text(get(Tag::DateTime))) {
        let t = match t.split_once(' ') {
            Some((d, h)) => format!("{} {}", d.replace(':', "-"), h),
            None => t,
        };
        attrs[2] = Value::Text(t);
    }
    let lat = get(Tag::GPSLatitude).and_then(|f| degrees(&f.value));
    let lon = get(Tag::GPSLongitude).and_then(|f| degrees(&f.value));
    let south = text(get(Tag::GPSLatitudeRef)).is_some_and(|r| r.starts_with('S'));
    let west = text(get(Tag::GPSLongitudeRef)).is_some_and(|r| r.starts_with('W'));
    let geom = match (lat, lon) {
        (Some(la), Some(lo)) if la != 0.0 || lo != 0.0 => {
            let (la, lo) = (if south { -la } else { la }, if west { -lo } else { lo });
            attrs[3] = Value::Real(la);
            attrs[4] = Value::Real(lo);
            Some(geo::Geometry::Point(geo::Point::new(lo, la)))
        }
        _ => None,
    };
    if let Some(a) = number(get(Tag::GPSAltitude)) {
        let below = get(Tag::GPSAltitudeRef).and_then(|f| f.value.get_uint(0)) == Some(1);
        attrs[5] = Value::Real(if below { -a } else { a });
    }
    if let Some(d) = number(get(Tag::GPSImgDirection)) {
        attrs[6] = Value::Real(d);
    }
    if let Some(m) = text(get(Tag::Make)) {
        attrs[7] = Value::Text(m);
    }
    if let Some(m) = text(get(Tag::Model)) {
        attrs[8] = Value::Text(m);
    }
    let size = |t: Tag, alt: Tag| get(t).or_else(|| get(alt)).and_then(|f| f.value.get_uint(0));
    if let Some(w) = size(Tag::PixelXDimension, Tag::ImageWidth) {
        attrs[9] = Value::Int(w as i64);
    }
    if let Some(h) = size(Tag::PixelYDimension, Tag::ImageLength) {
        attrs[10] = Value::Int(h as i64);
    }
    Ok(Feature::new(attrs, geom))
}

/// Photos as a stream of points in WGS 84.
pub fn read_photos(paths: &[&Path]) -> Result<Table> {
    let mut t = Table { fields: fields(), crs: Some(4326), ..Default::default() };
    for p in paths {
        t.features.push(read_photo(p)?);
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small JPEG with the place, the time and the direction in its Exif data.
    pub fn photo(path: &Path, lat: f64, lon: f64) {
        use exif::experimental::Writer;
        use exif::{Field, In, Rational, Tag, Value};
        let dms = |v: f64| {
            let d = v.abs().floor();
            let m = ((v.abs() - d) * 60.0).floor();
            let s = ((v.abs() - d) * 60.0 - m) * 60.0;
            Value::Rational(vec![Rational::from((d as u32, 1)), Rational::from((m as u32, 1)), Rational::from(((s * 10000.0).round() as u32, 10000))])
        };
        let fields = [
            Field { tag: Tag::GPSLatitude, ifd_num: In::PRIMARY, value: dms(lat) },
            Field { tag: Tag::GPSLatitudeRef, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"N".to_vec()]) },
            Field { tag: Tag::GPSLongitude, ifd_num: In::PRIMARY, value: dms(lon) },
            Field { tag: Tag::GPSLongitudeRef, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"E".to_vec()]) },
            Field { tag: Tag::GPSImgDirection, ifd_num: In::PRIMARY, value: Value::Rational(vec![Rational::from((1355, 10))]) },
            Field { tag: Tag::DateTimeOriginal, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"2026:10:30 12:41:07".to_vec()]) },
            Field { tag: Tag::Model, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"Test camera".to_vec()]) },
        ];
        let mut w = Writer::new();
        for f in &fields {
            w.push_field(f);
        }
        let mut tiff = std::io::Cursor::new(Vec::new());
        w.write(&mut tiff, false).unwrap();
        let tiff = tiff.into_inner();
        // A JPEG of one grey pixel, with the Exif data as an APP1 segment after its start.
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode(&[128, 128, 128], 1, 1, image::ExtendedColorType::Rgb8).unwrap();
        let mut out = jpeg[..2].to_vec();
        let len = (tiff.len() + 8) as u16;
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&jpeg[2..]);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn a_photo_tells_where_it_was_taken() {
        let dir = std::env::temp_dir().join(format!("og-photos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("IMG_0001.jpg");
        photo(&p, 57.70883, 11.97461);
        assert!(is_photo(&p));
        let f = read_photo(&p).unwrap();
        assert_eq!(f.attrs[2], Value::Text("2026-10-30 12:41:07".into()));
        assert_eq!(f.attrs[6], Value::Real(135.5));
        assert_eq!(f.attrs[8], Value::Text("Test camera".into()));
        match f.geom {
            Some(geo::Geometry::Point(pt)) => assert!((pt.y() - 57.70883).abs() < 1e-5 && (pt.x() - 11.97461).abs() < 1e-5, "{:?}", pt),
            g => panic!("{:?}", g),
        }
        // With a world file it is a picture on a map, not a photo.
        std::fs::write(dir.join("IMG_0001.jgw"), "1\n0\n0\n-1\n0\n0\n").unwrap();
        assert!(!is_photo(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
