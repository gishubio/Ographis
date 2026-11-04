//! KML and KMZ for Google Earth, GML for exchange, and GPX for GPS units: written with the
//! drivers of GDAL, in degrees where the format wants them.

use super::*;

/// The stream in WGS 84 degrees, as KML and GPX are always written.
fn in_degrees(table: &Table) -> Result<Table> {
    let mut t = table.clone();
    match table.crs {
        Some(4326) | None => {}
        Some(from) => {
            let b = GdalBackend::new();
            geom::transform_all(t.features.iter_mut().filter_map(|f| f.geom.as_mut()), |xs, ys| b.transform(from, 4326, xs, ys))?;
            t.crs = Some(4326);
        }
    }
    Ok(t)
}

/// The attribute that names each feature: one called name, or else the first text.
fn name_field(table: &Table) -> Option<String> {
    let names = ["name", "namn", "title", "titel", "label"];
    table
        .fields
        .iter()
        .find(|f| names.contains(&f.name.to_lowercase().as_str()))
        .or_else(|| table.fields.iter().find(|f| f.ty == FieldType::Text))
        .map(|f| f.name.clone())
}

/// A file of vector data made with options of the driver, such as FORMAT=GML3.2.
fn create(driver: &gdal::Driver, path: impl AsRef<Path>, options: &[&str]) -> Result<Dataset> {
    let mut list = gdal::cpl::CslStringList::new();
    for o in options {
        list.add_string(o).map_err(|e| gerr("Cannot set up the file", e))?;
    }
    driver.create_with_band_type_with_options::<u8, _>(path.as_ref(), 0, 0, 0, &list).map_err(|e| gerr(format!("Cannot create {}", path.as_ref().display()), e))
}

fn remove_old(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| GbError(format!("Cannot overwrite {}: {}", path.display(), e)))?;
    }
    Ok(())
}

/// The type of a field as KML's schema names it.
fn kml_type(ty: FieldType) -> &'static str {
    match ty {
        FieldType::Int => "int",
        FieldType::Real => "double",
        FieldType::Text => "string",
    }
}

/// A geometry as KML, with heights when there are as many as coordinates.
fn kml_geometry(g: &geo::Geometry<f64>, zs: Option<&[f64]>, at: &mut usize, out: &mut String) {
    use std::fmt::Write as _;
    let coords = |cs: &[geo::Coord<f64>], at: &mut usize, out: &mut String| {
        out.push_str("<coordinates>");
        for (k, c) in cs.iter().enumerate() {
            if k > 0 {
                out.push(' ');
            }
            match zs.and_then(|z| z.get(*at)).filter(|z| z.is_finite()) {
                Some(z) => {
                    let _ = write!(out, "{:.8},{:.8},{}", c.x, c.y, (z * 1000.0).round() / 1000.0);
                }
                None => {
                    let _ = write!(out, "{:.8},{:.8}", c.x, c.y);
                }
            }
            *at += 1;
        }
        out.push_str("</coordinates>");
    };
    let altitude = if zs.is_some() { "<altitudeMode>absolute</altitudeMode>" } else { "" };
    match g {
        geo::Geometry::Point(p) => {
            let _ = write!(out, "<Point>{}", altitude);
            coords(&[p.0], at, out);
            out.push_str("</Point>");
        }
        geo::Geometry::LineString(l) => {
            let _ = write!(out, "<LineString>{}", altitude);
            coords(&l.0, at, out);
            out.push_str("</LineString>");
        }
        geo::Geometry::Polygon(p) => {
            let _ = write!(out, "<Polygon>{}<outerBoundaryIs><LinearRing>", altitude);
            coords(&p.exterior().0, at, out);
            out.push_str("</LinearRing></outerBoundaryIs>");
            for r in p.interiors() {
                out.push_str("<innerBoundaryIs><LinearRing>");
                coords(&r.0, at, out);
                out.push_str("</LinearRing></innerBoundaryIs>");
            }
            out.push_str("</Polygon>");
        }
        geo::Geometry::MultiPoint(m) => {
            out.push_str("<MultiGeometry>");
            for p in &m.0 {
                kml_geometry(&geo::Geometry::Point(*p), zs, at, out);
            }
            out.push_str("</MultiGeometry>");
        }
        geo::Geometry::MultiLineString(m) => {
            out.push_str("<MultiGeometry>");
            for l in &m.0 {
                kml_geometry(&geo::Geometry::LineString(l.clone()), zs, at, out);
            }
            out.push_str("</MultiGeometry>");
        }
        geo::Geometry::MultiPolygon(m) => {
            out.push_str("<MultiGeometry>");
            for p in &m.0 {
                kml_geometry(&geo::Geometry::Polygon(p.clone()), zs, at, out);
            }
            out.push_str("</MultiGeometry>");
        }
        geo::Geometry::GeometryCollection(c) => {
            out.push_str("<MultiGeometry>");
            for g in &c.0 {
                kml_geometry(g, zs, at, out);
            }
            out.push_str("</MultiGeometry>");
        }
        // Rectangles, triangles and lines of two points, as the areas and lines they are.
        other => {
            let as_poly: Option<geo::Geometry<f64>> = match other {
                geo::Geometry::Rect(r) => Some(geo::Geometry::Polygon(r.to_polygon())),
                geo::Geometry::Triangle(t) => Some(geo::Geometry::Polygon(t.to_polygon())),
                geo::Geometry::Line(l) => Some(geo::Geometry::LineString(geo::LineString::from(vec![l.start, l.end]))),
                _ => None,
            };
            if let Some(g) = as_poly {
                kml_geometry(&g, zs, at, out);
            }
        }
    }
}

/// KML, or KMZ, which is the same KML zipped as doc.kml: written by Ographis itself, so that
/// every attribute lands by its name. Each feature a placemark, named by its name attribute,
/// with all its attributes in its extended data after a schema of their types; with heights,
/// at those heights above sea level.
pub(crate) fn write_kml(table: &Table, path: &Path, layer: &str, _sizes: &HashMap<String, FieldSize>, kmz: bool, notes: &mut Notes) -> Result<Vec<PathBuf>> {
    use std::fmt::Write as _;
    let t = in_degrees(table)?;
    let name_at = name_field(&t).and_then(|n| t.field_index(&n));
    let schema = {
        let mut s: String = layer.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
            s.insert(0, '_');
        }
        s
    };
    let mut doc = String::new();
    let _ = writeln!(doc, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document id=\"{}\">", xml(&schema));
    let _ = writeln!(doc, "<name>{}</name>", xml(layer));
    let _ = writeln!(doc, "<Schema name=\"{}\" id=\"{}\">", xml(&schema), xml(&schema));
    for f in &t.fields {
        let _ = writeln!(doc, "  <SimpleField name=\"{}\" type=\"{}\"/>", xml(&f.name), kml_type(f.ty));
    }
    doc.push_str("</Schema>\n");
    let mut without = 0usize;
    for f in &t.features {
        doc.push_str("<Placemark>");
        if let Some(n) = name_at.map(|i| f.attrs[i].to_text()).filter(|n| !n.is_empty()) {
            let _ = write!(doc, "<name>{}</name>", xml(&n));
        }
        let _ = write!(doc, "<ExtendedData><SchemaData schemaUrl=\"#{}\">", xml(&schema));
        for (field, v) in t.fields.iter().zip(&f.attrs) {
            let v = v.to_text();
            if !v.is_empty() {
                let _ = write!(doc, "<SimpleData name=\"{}\">{}</SimpleData>", xml(&field.name), xml(&v));
            }
        }
        doc.push_str("</SchemaData></ExtendedData>");
        match f.geom.as_ref() {
            Some(g) => kml_geometry(g, f.heights(), &mut 0, &mut doc),
            None => without += 1,
        }
        doc.push_str("</Placemark>\n");
    }
    doc.push_str("</Document>\n</kml>\n");
    remove_old(path)?;
    if kmz {
        // A zip as GDAL's own KMZ is: doc.kml links to the layer in layers/, which QGIS and
        // GDAL need to find its attributes; Google Earth follows the link.
        let link = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document id=\"root_doc\">\n<NetworkLink><Link><href>layers/{}.kml</href></Link></NetworkLink>\n</Document>\n</kml>\n",
            schema
        );
        let put = |inside: &str, text: &str| -> Result<()> {
            let name = CString::new(format!("/vsizip/{}/{}", path.display(), inside)).map_err(|_| GbError("The path has a NUL in it.".into()))?;
            let mode = CString::new("wb").expect("no nul");
            unsafe {
                let f = gdal_sys::VSIFOpenL(name.as_ptr(), mode.as_ptr());
                if f.is_null() {
                    return Err(GbError(format!("Cannot create {}", path.display())));
                }
                let wrote = gdal_sys::VSIFWriteL(text.as_ptr() as *const std::ffi::c_void, 1, text.len(), f);
                let closed = gdal_sys::VSIFCloseL(f);
                if wrote != text.len() || closed != 0 {
                    return Err(GbError(format!("Cannot write {}", path.display())));
                }
            }
            Ok(())
        };
        put("doc.kml", &link)?;
        put(&format!("layers/{}.kml", schema), &doc)?;
    } else {
        std::fs::write(path, &doc).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    if without > 0 {
        notes.warn(format!("{} features without geometry are placemarks without a place.", without));
    }
    if table.crs.is_some_and(|c| c != 4326) {
        notes.info("written in WGS 84 degrees, as KML always is");
    }
    Ok(vec![path.to_path_buf()])
}

/// GML 3.2, with its schema in an .xsd beside it, in the coordinate system of the stream.
pub(crate) fn write_gml(table: &Table, path: &Path, layer: &str, sizes: &HashMap<String, FieldSize>, notes: &mut Notes) -> Result<Vec<PathBuf>> {
    let driver = DriverManager::get_driver_by_name("GML").map_err(|e| gerr("GDAL has no GML support", e))?;
    let features: Vec<&Feature> = table.features.iter().collect();
    let fields = plan_fields(table, &features, table.field_names(), None, sizes, notes);
    let xsd = path.with_extension("xsd");
    remove_old(path)?;
    remove_old(&xsd)?;
    let options = ["FORMAT=GML3.2", "SRSNAME_FORMAT=OGC_URL", "WRITE_FEATURE_BOUNDED_BY=NO"];
    let mut ds = create(&driver, path, &options[..])?;
    create_layer_and_write(&mut ds, table, &features, layer, &fields, Shapes::Any, &[], None)?;
    ds.flush_cache().map_err(|e| gerr("Cannot save the file", e))?;
    ds.close().map_err(|e| gerr("Cannot close the file", e))?;
    Ok([path.to_path_buf(), xsd].into_iter().filter(|p| p.exists()).collect())
}

/// The lines of a geometry, each part of a multi line on its own.
fn lines(g: &geo::Geometry<f64>) -> Vec<geo::LineString<f64>> {
    match g {
        geo::Geometry::LineString(l) => vec![l.clone()],
        geo::Geometry::MultiLineString(m) => m.0.clone(),
        geo::Geometry::GeometryCollection(c) => c.0.iter().flat_map(lines).collect(),
        _ => Vec::new(),
    }
}

/// Text for XML: the five characters that mean something there, escaped.
fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// GPX: points as waypoints and lines as tracks, in degrees, with the heights as ele. The
/// attributes GPX knows, name, cmt, desc, src, sym, type and time, go in their places, and
/// ele where a point has no height of its own; the others in extensions. Areas have no
/// place in GPX and are left out. Written by Ographis itself, so that every attribute
/// lands by its name.
pub(crate) fn write_gpx(table: &Table, path: &Path, notes: &mut Notes) -> Result<Vec<PathBuf>> {
    use std::fmt::Write as _;
    let t = in_degrees(table)?;
    const KNOWN: [&str; 7] = ["name", "cmt", "desc", "src", "sym", "type", "time"];
    let known: Vec<(usize, &str)> = KNOWN.iter().filter_map(|k| t.fields.iter().position(|f| f.name.eq_ignore_ascii_case(k)).map(|i| (i, *k))).collect();
    let ele_at = t.fields.iter().position(|f| f.name.eq_ignore_ascii_case("ele"));
    let others: Vec<usize> = (0..t.fields.len()).filter(|i| !known.iter().any(|k| k.0 == *i) && Some(*i) != ele_at).collect();
    // A name GPX allows for an element of the extensions: letters, digits, _ - and .
    let tag = |name: &str| {
        let mut s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' }).collect();
        if !s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
            s.insert(0, '_');
        }
        s
    };
    let fields_of = |f: &Feature, indent: &str, with_ele: bool, out: &mut String| {
        if with_ele {
            if let Some(e) = ele_at.and_then(|i| f.attrs[i].as_f64()) {
                let _ = writeln!(out, "{}<ele>{}</ele>", indent, e);
            }
        }
        for (i, k) in &known {
            let v = f.attrs[*i].to_text();
            if !v.is_empty() {
                let _ = writeln!(out, "{}<{}>{}</{}>", indent, k, xml(&v), k);
            }
        }
        let extra: Vec<(usize, String)> = others.iter().map(|i| (*i, f.attrs[*i].to_text())).filter(|(_, v)| !v.is_empty()).collect();
        if !extra.is_empty() {
            let _ = writeln!(out, "{}<extensions>", indent);
            for (i, v) in extra {
                let n = tag(&t.fields[i].name);
                let _ = writeln!(out, "{}  <og:{}>{}</og:{}>", indent, n, xml(&v), n);
            }
            let _ = writeln!(out, "{}</extensions>", indent);
        }
    };
    let (mut wpts, mut trks, mut left) = (String::new(), String::new(), 0usize);
    let (mut n_wpt, mut n_trk) = (0usize, 0usize);
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    let mut grow = |x: f64, y: f64| {
        lo = [lo[0].min(x), lo[1].min(y)];
        hi = [hi[0].max(x), hi[1].max(y)];
    };
    for f in &t.features {
        let Some(g) = f.geom.as_ref() else {
            left += 1;
            continue;
        };
        let zs = f.heights();
        match geom::family(g) {
            Family::Point => {
                let cs = geom::coords(g);
                for (k, c) in cs.iter().enumerate() {
                    grow(c.x, c.y);
                    let _ = writeln!(wpts, "<wpt lat=\"{:.8}\" lon=\"{:.8}\">", c.y, c.x);
                    let z = zs.map(|z| z[k]).filter(|z| z.is_finite());
                    if let Some(z) = z {
                        let _ = writeln!(wpts, "  <ele>{}</ele>", (z * 1000.0).round() / 1000.0);
                    }
                    fields_of(f, "  ", z.is_none(), &mut wpts);
                    wpts.push_str("</wpt>\n");
                    n_wpt += 1;
                }
            }
            Family::Line => {
                trks.push_str("<trk>\n");
                fields_of(f, "  ", false, &mut trks);
                let mut at = 0;
                for part in lines(g) {
                    trks.push_str("  <trkseg>\n");
                    for c in &part.0 {
                        grow(c.x, c.y);
                        let z = zs.map(|z| z[at]).filter(|z| z.is_finite());
                        match z {
                            Some(z) => {
                                let _ = writeln!(trks, "    <trkpt lat=\"{:.8}\" lon=\"{:.8}\"><ele>{}</ele></trkpt>", c.y, c.x, (z * 1000.0).round() / 1000.0);
                            }
                            None => {
                                let _ = writeln!(trks, "    <trkpt lat=\"{:.8}\" lon=\"{:.8}\"/>", c.y, c.x);
                            }
                        }
                        at += 1;
                    }
                    trks.push_str("  </trkseg>\n");
                }
                trks.push_str("</trk>\n");
                n_trk += 1;
            }
            _ => left += 1,
        }
    }
    if n_wpt + n_trk == 0 {
        return Err(GbError(
            "GPX holds points and lines, and the stream has neither. Areas can be written as their outlines with Polygons to lines first.".into(),
        ));
    }
    let mut doc = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    doc.push_str("<gpx version=\"1.1\" creator=\"Ographis\" xmlns=\"http://www.topografix.com/GPX/1/1\" xmlns:og=\"https://github.com/gishubio/Ographis/gpx\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.topografix.com/GPX/1/1 http://www.topografix.com/GPX/1/1/gpx.xsd\">\n");
    let _ = writeln!(doc, "<metadata><bounds minlat=\"{:.8}\" minlon=\"{:.8}\" maxlat=\"{:.8}\" maxlon=\"{:.8}\"/></metadata>", lo[1], lo[0], hi[1], hi[0]);
    doc.push_str(&wpts);
    doc.push_str(&trks);
    doc.push_str("</gpx>\n");
    std::fs::write(path, doc).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    if left > 0 {
        notes.warn(format!("{} areas or features without geometry were left out: GPX holds points and lines.", left));
    }
    notes.info(format!("{} waypoints and {} tracks", n_wpt, n_trk));
    Ok(vec![path.to_path_buf()])
}
