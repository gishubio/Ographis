//! Reads and writes real files through GDAL. Needs GDAL and PROJ to be installed.

use gdal::vector::{LayerAccess, LayerOptions, OGRwkbGeometryType};
use geo::{wkt, Geometry};
use og_engine::io::{Backend, Notes, ReadOpts, WriteOpts};
use og_engine::{run, Feature, Field, FieldType, Status, Table, Value, Workspace};
use og_gdal::GdalBackend;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("og-gdal-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
/// The folder of the whole project, where the examples are.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn text(s: &str) -> Value {
    Value::Text(s.to_string())
}
fn col(t: &Table, name: &str) -> Vec<Value> {
    let i = t.field_index(name).unwrap_or_else(|| panic!("no {} among {:?}", name, t.field_names()));
    t.features.iter().map(|f| f.attrs[i].clone()).collect()
}

fn sample() -> Table {
    Table {
        mesh: None,
        fields: vec![
            Field::new("muni_code", FieldType::Text),
            Field::new("name", FieldType::Text),
            // Swedish for "population_total". The å is here to test field names that are not ASCII.
            Field::new("invånare_totalt", FieldType::Int),
            Field::new("share", FieldType::Real),
            Field::new("big", FieldType::Int),
        ],
        crs: Some(3006),
        features: vec![
            Feature {
                z: None,
                attrs: vec![text("0114"), text("Upplands Väsby"), Value::Int(48000), Value::Real(12.5), Value::Int(5_000_000_000)],
                geom: Some(Geometry::Polygon(wkt!(POLYGON((660000. 6590000., 670000. 6590000., 670000. 6600000., 660000. 6600000., 660000. 6590000.))))),
            },
            Feature {
                z: None,
                attrs: vec![text("1480"), text("Göteborg"), Value::Null, Value::Null, Value::Int(1)],
                geom: Some(Geometry::MultiPolygon(
                    wkt!(MULTIPOLYGON(((310000. 6390000., 320000. 6390000., 320000. 6400000., 310000. 6390000.)), ((330000. 6390000., 340000. 6390000., 340000. 6400000., 330000. 6390000.)))),
                )),
            },
        ],
        raster: false,
        image: None,
        grid: None,
    }
}

#[test]
fn geopackage_round_trip_keeps_values_types_and_crs() {
    let dir = tmp("gpkg");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let path = dir.join("test.gpkg");
    let files = b.write(&sample(), &path, &WriteOpts { layer: Some("areas".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(files, std::slice::from_ref(&path));
    assert!(notes.warnings.is_empty(), "{:?}", notes.warnings);

    let back = b.read(&path, &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(back.crs, Some(3006));
    assert_eq!(back.fields, sample().fields);
    assert_eq!(col(&back, "muni_code"), [text("0114"), text("1480")]);
    assert_eq!(col(&back, "name"), [text("Upplands Väsby"), text("Göteborg")]);
    assert_eq!(col(&back, "invånare_totalt"), [Value::Int(48000), Value::Null]);
    assert_eq!(col(&back, "share"), [Value::Real(12.5), Value::Null]);
    assert_eq!(col(&back, "big"), [Value::Int(5_000_000_000), Value::Int(1)]);
    // Single and multi in the same layer: the layer becomes MultiPolygon and the single area is promoted.
    assert!(matches!(back.features[0].geom, Some(Geometry::MultiPolygon(ref m)) if m.0.len() == 1));
    assert!(matches!(back.features[1].geom, Some(Geometry::MultiPolygon(ref m)) if m.0.len() == 2));

    // A second layer in the same file, and then the first one is overwritten: the second remains.
    let mut points = sample();
    points.features.iter_mut().for_each(|f| f.geom = Some(Geometry::Point(geo::Point::new(319758., 6400326.))));
    b.write(&points, &path, &WriteOpts { layer: Some("points".into()), ..Default::default() }, &mut notes).unwrap();
    let mut one = sample();
    one.features.truncate(1);
    b.write(&one, &path, &WriteOpts { layer: Some("areas".into()), ..Default::default() }, &mut notes).unwrap();
    let layers = b.layers(&path).unwrap();
    let summary: Vec<(String, Option<u64>)> = layers.iter().map(|l| (l.name.clone(), l.count)).collect();
    assert_eq!(summary, [("points".to_string(), Some(2)), ("areas".to_string(), Some(1))]);
    assert!(layers.iter().all(|l| l.crs == Some(3006)));
    let areas = b.read(&path, &ReadOpts { layer: Some("areas".into()), ..Default::default() }, &mut notes).unwrap();
    assert!(matches!(areas.features[0].geom, Some(Geometry::Polygon(_))));

    let e = b.read(&path, &ReadOpts { layer: Some("not_there".into()), ..Default::default() }, &mut notes).unwrap_err();
    assert!(e.0.contains("Available: points, areas"), "{}", e.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shapefile_names_encoding_and_split() {
    let dir = tmp("shp");
    let b = GdalBackend::new();
    for encoding in ["utf-8", "windows-1252"] {
        let mut notes = Notes::default();
        let path = dir.join(format!("areas_{}.shp", encoding.replace('-', "")));
        b.write(&sample(), &path, &WriteOpts { encoding: Some(encoding.into()), ..Default::default() }, &mut notes).unwrap();
        assert!(notes.warnings.iter().any(|w| w.contains("invånare_totalt → invånare_")), "{:?}", notes.warnings);
        let cpg = std::fs::read_to_string(path.with_extension("cpg")).unwrap();
        assert_eq!(cpg.trim(), if encoding == "utf-8" { "UTF-8" } else { "CP1252" });

        let mut notes = Notes::default();
        let back = b.read(&path, &ReadOpts::default(), &mut notes).unwrap();
        assert_eq!(back.crs, Some(3006));
        // Ten bytes: in UTF-8 the å takes two bytes, in Windows-1252 one.
        let expected = if encoding == "utf-8" { "invånare_" } else { "invånare_t" };
        assert_eq!(back.field_names(), ["muni_code", "name", expected, "share", "big"]);
        assert_eq!(col(&back, "name"), [text("Upplands Väsby"), text("Göteborg")]);
        assert_eq!(col(&back, "muni_code"), [text("0114"), text("1480")]);
        assert_eq!(col(&back, "big"), [Value::Int(5_000_000_000), Value::Int(1)]);
        assert_eq!(col(&back, "share"), [Value::Real(12.5), Value::Null]);
    }

    // A file without .cpg written in Windows-1252 reads correctly when the reader is told the encoding.
    let path = dir.join("areas_windows1252.shp");
    std::fs::remove_file(path.with_extension("cpg")).unwrap();
    let mut notes = Notes::default();
    let back = b.read(&path, &ReadOpts { encoding: Some("windows-1252".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(col(&back, "name"), [text("Upplands Väsby"), text("Göteborg")]);

    // Mixed geometry types are split into one file per type. Features without geometry go with the first.
    let mut mixed = sample();
    mixed.features[1].geom = Some(Geometry::LineString(wkt!(LINESTRING(310000. 6390000., 320000. 6400000.))));
    mixed.features.push(Feature { z: None, attrs: vec![Value::Null; 5], geom: Some(Geometry::Point(geo::Point::new(1., 2.))) });
    mixed.features.push(Feature { z: None, attrs: vec![Value::Null; 5], geom: None });
    let mut notes = Notes::default();
    let files = b.write(&mixed, &dir.join("mixed.shp"), &WriteOpts::default(), &mut notes).unwrap();
    let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["mixed_point.shp", "mixed_line.shp", "mixed_area.shp"]);
    assert!(notes.warnings.iter().any(|w| w.contains("split into 3 files")));
    assert_eq!(b.read(&files[0], &ReadOpts::default(), &mut notes).unwrap().len(), 2);
    assert_eq!(b.read(&files[1], &ReadOpts::default(), &mut notes).unwrap().len(), 1);

    // Writing once more overwrites without an error.
    b.write(&mixed, &dir.join("mixed.shp"), &WriteOpts::default(), &mut notes).unwrap();
    assert!(b.write(&mixed, &dir.join("wrong.kml"), &WriteOpts::default(), &mut notes).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reprojection_matches_known_points() {
    let b = GdalBackend::new();
    // Gothenburg in WGS 84 → SWEREF 99 TM, SWEREF 99 12 00 and back.
    let (mut x, mut y) = (vec![11.9746], vec![57.7089]);
    b.transform(4326, 3006, &mut x, &mut y).unwrap();
    // Reference from proj4js: 319758.020 E, 6400326.036 N.
    assert!((x[0] - 319_758.020).abs() < 0.001 && (y[0] - 6_400_326.036).abs() < 0.001, "{} {}", x[0], y[0]);
    b.transform(3006, 3007, &mut x, &mut y).unwrap();
    // The central meridian 12°00' is at 150,000 m, and Gothenburg is just west of it.
    // Reference from proj4js, an independent implementation: 148485.856 E, 6398861.670 N.
    assert!((x[0] - 148_485.856).abs() < 0.001 && (y[0] - 6_398_861.670).abs() < 0.001, "{} {}", x[0], y[0]);
    b.transform(3007, 4326, &mut x, &mut y).unwrap();
    assert!((x[0] - 11.9746).abs() < 1e-8 && (y[0] - 57.7089).abs() < 1e-8, "{} {}", x[0], y[0]);
    assert!(b.transform(4326, 999_999, &mut x, &mut y).is_err());
    assert!(b.crs_info(4326).unwrap().geographic);
    assert!(!b.crs_info(3021).unwrap().geographic);
}

#[test]
fn odd_inputs_z_curves_and_missing_crs() {
    let dir = tmp("odd");
    let path = dir.join("odd.gpkg");
    {
        let driver = gdal::DriverManager::get_driver_by_name("GPKG").unwrap();
        let mut ds = driver.create_vector_only(&path).unwrap();
        let srs = gdal::spatial_ref::SpatialRef::from_epsg(3006).unwrap();
        let mut z = ds.create_layer(LayerOptions { name: "with_z", srs: Some(&srs), ty: OGRwkbGeometryType::wkbPoint25D, options: None }).unwrap();
        z.create_feature(gdal::vector::Geometry::from_wkt("POINT Z (319758 6400326 12.5)").unwrap()).unwrap();
        let mut c = ds.create_layer(LayerOptions { name: "curves", srs: Some(&srs), ty: OGRwkbGeometryType::wkbMultiSurface, options: None }).unwrap();
        c.create_feature(gdal::vector::Geometry::from_wkt("MULTISURFACE (CURVEPOLYGON (CIRCULARSTRING (0 0, 10 10, 20 0, 10 -10, 0 0)))").unwrap()).unwrap();
        c.create_feature(gdal::vector::Geometry::from_wkt("MULTISURFACE (((0 0, 10 0, 10 10, 0 10, 0 0)))").unwrap()).unwrap();
        let mut n = ds.create_layer(LayerOptions { name: "no_crs", srs: None, ty: OGRwkbGeometryType::wkbPoint, options: None }).unwrap();
        n.create_feature(gdal::vector::Geometry::from_wkt("POINT (1 2)").unwrap()).unwrap();
        ds.create_layer(LayerOptions { name: "table", srs: None, ty: OGRwkbGeometryType::wkbNone, options: None })
            .unwrap()
            .create_defn_fields(&[("a", gdal::vector::OGRFieldType::OFTString)])
            .unwrap();
    }
    let b = GdalBackend::new();
    let read = |layer: &str, crs: Option<u32>| {
        let mut notes = Notes::default();
        let t = b.read(&path, &ReadOpts { layer: Some(layer.into()), crs, ..Default::default() }, &mut notes);
        (t, notes)
    };
    let (t, notes) = read("with_z", None);
    let t = t.unwrap();
    assert_eq!(t.features[0].geom, Some(Geometry::Point(geo::Point::new(319758., 6400326.))));
    // Since 0.66 the heights are kept, and said so.
    assert!(notes.info.iter().any(|w| w.contains("with heights (Z)")), "{:?}", notes);
    assert!(t.features[0].heights().is_some());

    let (t, notes) = read("curves", None);
    let t = t.unwrap();
    assert!(notes.info.iter().any(|i| i.contains("curved features")), "{:?}", notes);
    let circle = og_engine::geom::area(t.features[0].geom.as_ref().unwrap(), false);
    // A circle with radius 10 has the area 314.16. Splitting it into segments gives slightly less.
    assert!((circle - 314.16).abs() < 1.0, "{}", circle);
    assert_eq!(og_engine::geom::area(t.features[1].geom.as_ref().unwrap(), false), 100.0);

    let (t, _) = read("no_crs", None);
    assert!(t.unwrap_err().0.contains("states no coordinate system"));
    let (t, _) = read("no_crs", Some(3006));
    assert_eq!(t.unwrap().crs, Some(3006));
    let (t, _) = read("table", None);
    let t = t.unwrap();
    assert_eq!((t.crs, t.len(), t.field_names()), (None, 0, vec!["a".to_string()]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn whole_flow_with_mixed_crs_and_formats() {
    let dir = tmp("flow");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    // A clip area in SWEREF 99 TM around Gothenburg, points in WGS 84 from CSV.
    let mut area = sample();
    area.features = vec![Feature {
        z: None,
        attrs: vec![text("1480"), text("Göteborg"), Value::Null, Value::Null, Value::Null],
        geom: Some(Geometry::Polygon(wkt!(POLYGON((300000. 6380000., 340000. 6380000., 340000. 6420000., 300000. 6420000., 300000. 6380000.))))),
    }];
    b.write(&area, &dir.join("area.gpkg"), &WriteOpts::default(), &mut notes).unwrap();
    std::fs::write(dir.join("towns.csv"), "name;lat;lon\nGöteborg;57,7089;11,9746\nBorås;57,7210;12,9401\n").unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "y", "type": "Reader", "params": {"path": "area.gpkg"}},
        {"id": "o", "type": "Reader", "params": {"path": "towns.csv"}},
        {"id": "c", "type": "Clipper"},
        {"id": "r", "type": "Reprojector", "params": {"crs": "EPSG:3007"}},
        {"id": "w", "type": "Writer", "params": {"path": "out/inside.shp"}},
        {"id": "a", "type": "AreaCalculator", "params": {"unit": "km2"}},
        {"id": "g", "type": "Reprojector", "params": {"crs": 4326}},
        {"id": "ag", "type": "AreaCalculator", "params": {"name": "geod", "unit": "km2"}}
      ], "edges": [
        {"from": ["y", "Output"], "to": ["c", "Clipper"]},
        {"from": ["o", "Output"], "to": ["c", "Clippee"]},
        {"from": ["c", "Inside"], "to": ["r", "Input"]},
        {"from": ["r", "Reprojected"], "to": ["w", "Input"]},
        {"from": ["y", "Output"], "to": ["a", "Input"]},
        {"from": ["a", "Output"], "to": ["g", "Input"]},
        {"from": ["g", "Reprojected"], "to": ["ag", "Input"]}
      ]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    // The clip area was reprojected to the system of the points, with a warning about it.
    assert_eq!(r.nodes["c"].status, Status::Warn);
    assert_eq!(col(r.output("c", "Inside").unwrap(), "name"), [text("Göteborg")]);
    assert_eq!(col(r.output("c", "Outside").unwrap(), "name"), [text("Borås")]);
    let back = b.read(&dir.join("out/inside.shp"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(back.crs, Some(3007));
    match &back.features[0].geom {
        Some(Geometry::Point(p)) => assert!((p.x() - 148_485.856).abs() < 0.001, "{:?}", p),
        other => panic!("{:?}", other),
    }
    // 40 × 40 km planar. At Gothenburg the scale factor of SWEREF 99 TM is close to 1, so
    // the geodesic area of the same four corners must be within a square kilometre of the planar one.
    let planar = col(r.output("ag", "Output").unwrap(), "_area")[0].as_f64().unwrap();
    let geod = col(r.output("ag", "Output").unwrap(), "geod")[0].as_f64().unwrap();
    assert_eq!(planar, 1600.0);
    assert!((planar - geod).abs() < 1.0 && planar != geod, "{} {}", planar, geod);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A small point table in SWEREF 99 TM, with ids starting at `first`.
fn points(first: i64, n: i64) -> Table {
    Table {
        mesh: None,
        fields: vec![Field::new("id", FieldType::Int), Field::new("code", FieldType::Text)],
        crs: Some(3006),
        features: (first..first + n)
            .map(|i| Feature {
                z: None,
                attrs: vec![Value::Int(i), text(&format!("{:04}", i))],
                geom: Some(Geometry::Point(geo::Point::new(300000. + i as f64 * 100., 6400000.))),
            })
            .collect(),
        raster: false,
        image: None,
        grid: None,
    }
}

#[test]
fn a_folder_of_shapefiles_is_one_stream() {
    let dir = tmp("folder");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    std::fs::create_dir_all(dir.join("in/old")).unwrap();
    // Thirty files with the same structure and three features each.
    for k in 0..30 {
        b.write(&points(k * 3, 3), &dir.join(format!("in/area_{:02}.shp", k + 1)), &WriteOpts::default(), &mut notes).unwrap();
    }
    b.write(&points(1000, 1), &dir.join("in/old/area_99.shp"), &WriteOpts::default(), &mut notes).unwrap();
    // One file in another coordinate system: it is reprojected to that of the others.
    let mut wgs = points(2000, 1);
    wgs.crs = Some(4326);
    wgs.features[0].geom = Some(Geometry::Point(geo::Point::new(11.9746, 57.7089)));
    b.write(&wgs, &dir.join("in/other_crs.shp"), &WriteOpts::default(), &mut notes).unwrap();

    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "all", "type": "Reader", "params": {"path": "in", "pattern": "area_*.shp", "filename_attr": "source"}},
        {"id": "deep", "type": "Reader", "params": {"path": "in", "pattern": "area_*.shp", "recursive": "true"}},
        {"id": "default", "type": "Reader", "params": {"path": "in"}},
        {"id": "out", "type": "Writer", "params": {"path": "out/all.gpkg"}}
      ],
      "edges": [{"from": ["all", "Output"], "to": ["out", "Input"]}]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    let all = r.output("all", "Output").unwrap();
    assert_eq!(all.len(), 90);
    assert_eq!(all.crs, Some(3006));
    assert_eq!(all.field_names(), ["id", "code", "source"]);
    assert_eq!(col(all, "id"), (0..90).map(Value::Int).collect::<Vec<_>>());
    let sources = col(all, "source");
    assert_eq!((sources[0].clone(), sources[3].clone(), sources[89].clone()), (text("area_01"), text("area_02"), text("area_30")));
    assert_eq!(r.nodes["all"].sources.len(), 30);
    assert_eq!(r.nodes["all"].status, Status::Ok);
    assert_eq!(r.output("deep", "Output").unwrap().len(), 91);

    // The default filter is *.shp. The file in WGS 84 is reprojected, with a warning that names it.
    let d = r.output("default", "Output").unwrap();
    assert_eq!((d.len(), d.crs), (91, Some(3006)));
    let Some(Geometry::Point(p)) = &d.features[90].geom else { panic!("the last feature should be a point") };
    assert!((p.x() - 319_758.3).abs() < 1.0 && (p.y() - 6_400_326.0).abs() < 1.0, "{:?}", p);
    assert_eq!(r.nodes["default"].status, Status::Warn);
    assert!(r.nodes["default"].warnings[0].contains("The files have different coordinate systems"), "{:?}", r.nodes["default"].warnings);

    let back = b.read(&dir.join("out/all.gpkg"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(back.len(), 90);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn several_layers_of_a_geopackage() {
    let dir = tmp("layers");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let path = dir.join("many.gpkg");
    for (k, name) in ["north", "south", "east"].iter().enumerate() {
        b.write(&points(k as i64 * 10, 2), &path, &WriteOpts { layer: Some(name.to_string()), ..Default::default() }, &mut notes).unwrap();
    }
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "first", "type": "Reader", "params": {"path": "many.gpkg"}},
        {"id": "two", "type": "Reader", "params": {"path": "many.gpkg", "layer": ["east", "north"], "layer_attr": "layer"}},
        {"id": "text", "type": "Reader", "params": {"path": "many.gpkg", "layer": "south, east"}},
        {"id": "all", "type": "Reader", "params": {"path": "many.gpkg", "layer": "*", "layer_attr": "layer", "filename_attr": "file", "filename_part": "name"}},
        {"id": "named", "type": "Reader", "params": {"path": "many.gpkg", "layer_attr": "layer"}},
        {"id": "missing", "type": "Reader", "params": {"path": "many.gpkg", "layer": ["north", "west"]}}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(col(r.output("first", "Output").unwrap(), "id"), [Value::Int(0), Value::Int(1)]);
    assert!(r.log.iter().any(|l| l.msg.contains("layer north, one of 3 in the file")));
    // The layers come in the order they were chosen.
    let two = r.output("two", "Output").unwrap();
    assert_eq!(col(two, "id"), [20, 21, 0, 1].map(Value::Int));
    assert_eq!(col(two, "layer"), ["east", "east", "north", "north"].map(text));
    assert!(r.log.iter().any(|l| l.msg.starts_with("Input: 4 out") && l.msg.contains("2 layers")));
    assert_eq!(col(r.output("text", "Output").unwrap(), "id"), [10, 11, 20, 21].map(Value::Int));
    let all = r.output("all", "Output").unwrap();
    assert_eq!(col(all, "layer"), ["north", "north", "south", "south", "east", "east"].map(text));
    assert!(col(all, "file").iter().all(|v| *v == text("many.gpkg")));
    assert_eq!(all.crs, Some(3006));
    // The name of the first layer is found when it is asked for.
    assert_eq!(col(r.output("named", "Output").unwrap(), "layer"), ["north", "north"].map(text));
    assert!(r.nodes["missing"].error.as_ref().unwrap().contains("There is no layer \"west\""));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn excel_sheets_are_read_and_written() {
    let dir = tmp("excel");
    let b = GdalBackend::new();
    std::fs::write(
        dir.join("towns.csv"),
        "code;name;population;share;e;n\n0114;Upplands Väsby;48000;12,5;666100;6600500\n1480;Göteborg;600000;;319758;6400326\n",
    )
    .unwrap();
    std::fs::write(dir.join("names.csv"), "code;name\n0114;Upplands Väsby\n1480;Göteborg\n").unwrap();
    std::fs::write(dir.join("lines.csv"), "id;wkt\n1;LINESTRING (0 0, 10 10)\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "towns", "type": "Reader", "params": {"path": "towns.csv", "crs": "EPSG:3006"}},
        {"id": "names", "type": "Reader", "params": {"path": "names.csv"}},
        {"id": "lines", "type": "Reader", "params": {"path": "lines.csv", "crs": "EPSG:3006"}},
        {"id": "w1", "type": "Writer", "params": {"path": "out/book.xlsx", "layer": "Towns", "geometry": "xy"}},
        {"id": "w2", "type": "Writer", "params": {"path": "out/book.xlsx", "layer": "Names"}},
        {"id": "w3", "type": "Writer", "params": {"path": "out/book.xlsx", "layer": "Lines: a/b?", "geometry": "wkt"}},
        {"id": "w4", "type": "Writer", "params": {"path": "out/plain.xlsx", "geometry": "none"}}
      ],
      "edges": [
        {"from": ["towns", "Output"], "to": ["w1", "Input"]},
        {"from": ["names", "Output"], "to": ["w2", "Input"]},
        {"from": ["lines", "Output"], "to": ["w3", "Input"]},
        {"from": ["towns", "Output"], "to": ["w4", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    assert!(r.nodes["w3"].warnings[0].contains("It was named \"Lines_ a_b_\""), "{:?}", r.nodes["w3"].warnings);
    let book = dir.join("out/book.xlsx");

    // Three writers, one workbook, three sheets.
    let sheets: Vec<(String, Option<u64>)> = b.layers(&book).unwrap().into_iter().map(|l| (l.name, l.count)).collect();
    assert_eq!(sheets.len(), 3, "{:?}", sheets);
    for (name, count) in [("Towns", 2), ("Names", 2), ("Lines_ a_b_", 1)] {
        assert!(sheets.contains(&(name.to_string(), Some(count))), "{} is not among {:?}", name, sheets);
    }

    let mut notes = Notes::default();
    let read = |layer: &str, header: Option<bool>, notes: &mut Notes| {
        b.read(&book, &ReadOpts { layer: Some(layer.into()), header, ..Default::default() }, notes).unwrap()
    };
    // Values and types survive. The code keeps its leading zero, and the points became x and y.
    let towns = read("Towns", Some(true), &mut notes);
    assert_eq!(towns.field_names(), ["code", "name", "population", "share", "e", "n", "x", "y"]);
    assert_eq!(col(&towns, "code"), [text("0114"), text("1480")]);
    assert_eq!(col(&towns, "name"), [text("Upplands Väsby"), text("Göteborg")]);
    assert_eq!(col(&towns, "population"), [Value::Int(48000), Value::Int(600000)]);
    assert_eq!(col(&towns, "share"), [Value::Real(12.5), Value::Null]);
    assert_eq!(col(&towns, "x"), [Value::Int(666100), Value::Int(319758)]);
    assert_eq!((towns.crs, towns.has_geometry()), (None, false));
    // Lines are written as WKT.
    assert_eq!(col(&read("Lines_ a_b_", Some(true), &mut notes), "wkt"), [text("LINESTRING(0 0,10 10)")]);

    // A sheet where every column is text: the first row is the names all the same.
    let names = read("Names", Some(true), &mut notes);
    assert_eq!((names.field_names(), names.len()), (vec!["code".to_string(), "name".to_string()], 2));
    // Told that there are no names, the reader takes the first row as data.
    let raw = read("Names", Some(false), &mut notes);
    assert_eq!((raw.field_names(), raw.len()), (vec!["Field1".to_string(), "Field2".to_string()], 3));
    assert_eq!(col(&raw, "Field1"), [text("code"), text("0114"), text("1480")]);

    // "none" leaves the geometry out.
    let plain = b.read(&dir.join("out/plain.xlsx"), &ReadOpts { header: Some(true), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(plain.field_names(), ["code", "name", "population", "share", "e", "n"]);

    // Read through a workspace: chosen sheets, all sheets, and the sheet name as an attribute.
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "first", "type": "Reader", "params": {"path": "out/book.xlsx"}},
        {"id": "two", "type": "Reader", "params": {"path": "out/book.xlsx", "layer": ["Names", "Towns"], "layer_attr": "sheet"}},
        {"id": "raw", "type": "Reader", "params": {"path": "out/book.xlsx", "layer": "Names", "header": "false"}},
        {"id": "again", "type": "Writer", "params": {"path": "out/book.xlsx", "layer": "names"}}
      ],
      "edges": [{"from": ["first", "Output"], "to": ["again", "Input"]}]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let two = r.output("two", "Output").unwrap();
    assert_eq!(col(two, "sheet"), ["Names", "Names", "Towns", "Towns"].map(text));
    assert_eq!(col(two, "code"), ["0114", "1480", "0114", "1480"].map(text));
    assert_eq!(col(two, "population"), [Value::Null, Value::Null, Value::Int(48000), Value::Int(600000)]);
    assert_eq!(r.output("raw", "Output").unwrap().len(), 3);

    // Writing a sheet that is already there replaces it, whatever the case, and keeps the others.
    let sheets: Vec<(String, Option<u64>)> = b.layers(&book).unwrap().into_iter().map(|l| (l.name, l.count)).collect();
    assert_eq!(sheets.len(), 3, "{:?}", sheets);
    assert!(sheets.contains(&("names".to_string(), Some(2))) && sheets.contains(&("Lines_ a_b_".to_string(), Some(1))), "{:?}", sheets);
    assert_eq!(col(&read("Towns", Some(true), &mut notes), "code"), [text("0114"), text("1480")]);
    assert_eq!(col(&read("Lines_ a_b_", Some(true), &mut notes), "wkt"), [text("LINESTRING(0 0,10 10)")]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_workbook_from_another_program() {
    // The file was written with openpyxl: three sheets, of which one is empty and one holds
    // only text, dates, and a code with a leading zero.
    let book = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/other_tool.xlsx");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    // An empty sheet is not a layer.
    let sheets: Vec<(String, Option<u64>, Vec<String>)> =
        b.layers(&book).unwrap().into_iter().map(|l| (l.name, l.count, l.fields.into_iter().map(|f| f.name).collect())).collect();
    assert_eq!(sheets.len(), 2, "{:?}", sheets);
    assert_eq!(sheets[0], ("Municipalities".to_string(), Some(2), ["Code", "Name", "Population", "Share", "Updated"].map(String::from).to_vec()));
    // The first row is the names in every sheet, not just the first one.
    assert_eq!(sheets[1], ("Only text".to_string(), Some(2), ["code", "note"].map(String::from).to_vec()));

    let m = b.read(&book, &ReadOpts { header: Some(true), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(col(&m, "Code"), [text("0114"), text("1480")]);
    assert_eq!(col(&m, "Population"), [Value::Int(48000), Value::Int(600000)]);
    // An empty cell is empty, not zero.
    assert_eq!(col(&m, "Share"), [Value::Real(12.5), Value::Null]);
    assert_eq!(col(&m, "Updated"), [text("2026-10-01"), text("2026-10-02")]);
    let t = b.read(&book, &ReadOpts { layer: Some("Only text".into()), header: Some(true), ..Default::default() }, &mut notes).unwrap();
    assert_eq!((t.field_names(), col(&t, "note")), (vec!["code".to_string(), "note".to_string()], vec![text("north"), text("west")]));
    let raw = b.read(&book, &ReadOpts { layer: Some("Only text".into()), header: Some(false), ..Default::default() }, &mut notes).unwrap();
    assert_eq!((raw.field_names(), raw.len()), (vec!["Field1".to_string(), "Field2".to_string()], 3));
    // The setting does not linger: the next read decides for itself.
    let auto = b.read(&book, &ReadOpts { layer: Some("Only text".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(auto.len(), 3);
}

#[test]
fn sheet_names_follow_the_rules_of_excel() {
    use og_gdal::sheet_name as s;
    assert_eq!(s("Towns"), "Towns");
    assert_eq!(s("  a/b\\c:d*e?f[g]  "), "a_b_c_d_e_f_g_");
    assert_eq!(s("A_name_that_is_far_too_long_to_be_a_sheet"), "A_name_that_is_far_too_long_to_");
    assert_eq!(s("A_name_that_is_far_too_long_to_be_a_sheet").chars().count(), 31);
    assert_eq!(s(""), "Sheet1");
    assert_eq!(s("'quoted'"), "quoted");
}

#[test]
fn the_limit_stops_the_reading_of_layers_and_files() {
    let dir = tmp("limit");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let path = dir.join("many.gpkg");
    for (k, name) in ["north", "south"].iter().enumerate() {
        b.write(&points(k as i64 * 100, 40), &path, &WriteOpts { layer: Some(name.to_string()), ..Default::default() }, &mut notes).unwrap();
    }
    b.write(&points(500, 40), &dir.join("points.shp"), &WriteOpts::default(), &mut notes).unwrap();
    // The backend itself stops after the number it is given.
    let few = b.read(&path, &ReadOpts { limit: Some(7), ..Default::default() }, &mut notes).unwrap();
    assert_eq!((few.len(), col(&few, "id")[6].clone()), (7, Value::Int(6)));

    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "all", "type": "Reader", "params": {"path": "many.gpkg", "layer": "*"}},
        {"id": "some", "type": "Reader", "params": {"path": "many.gpkg", "layer": "*", "limit": "50", "layer_attr": "layer"}},
        {"id": "shp", "type": "Reader", "params": {"path": "points.shp", "limit": "1000"}},
        {"id": "ten", "type": "Reader", "params": {"path": "points.shp", "limit": "10"}}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.output("all", "Output").unwrap().len(), 80);
    // Forty from the first layer and ten from the second.
    let some = r.output("some", "Output").unwrap();
    assert_eq!(some.len(), 50);
    assert_eq!(col(some, "layer")[39..41], [text("north"), text("south")]);
    assert_eq!(col(some, "id")[49], Value::Int(109));
    assert_eq!(r.output("shp", "Output").unwrap().len(), 40);
    assert_eq!(r.output("ten", "Output").unwrap().len(), 10);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_zip_file_is_read_without_unpacking() {
    // Two Shapefiles in a folder inside the zip file, made with ogr2ogr and zipped.
    let zip = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/zipped.zip");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let layers: Vec<(String, Option<u64>, Option<u32>)> = b.layers(&zip).unwrap().into_iter().map(|l| (l.name, l.count, l.crs)).collect();
    assert_eq!(layers, [("north".to_string(), Some(2), Some(3006)), ("south".to_string(), Some(1), Some(3006))]);
    let north = b.read(&zip, &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!((north.crs, col(&north, "name")), (Some(3006), vec![text("Skara"), text("Lidköping")]));
    let south = b.read(&zip, &ReadOpts { layer: Some("south".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(col(&south, "name"), [text("Kinna")]);
    let Some(Geometry::Point(p)) = &south.features[0].geom else { panic!("a point was expected") };
    assert_eq!((p.x(), p.y()), (362270.0, 6376250.0));
    // A zip file that is not there, or holds nothing that can be read, is an ordinary error.
    let e = b.read(&zip.with_file_name("missing.zip"), &ReadOpts::default(), &mut notes).unwrap_err();
    assert!(e.0.starts_with("Cannot open ") && e.0.contains("missing.zip"), "{}", e.0);
}

#[test]
fn a_writer_sets_the_sizes_of_fields() {
    let dir = tmp("sizes");
    let b = GdalBackend::new();
    std::fs::write(dir.join("t.csv"), "code;name;share;e;n\n0114;Upplands Väsby;12,456;666100;6600500\n1480;Göteborg;3,25678;319758;6400326\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv", "crs": "EPSG:3006"}},
        {"id": "shp", "type": "Writer", "params": {"path": "sized", "format": "shp", "fields": {
          "code": {"type": "integer", "length": 6},
          "name": {"length": 10},
          "share": {"decimals": 2}
        }}},
        {"id": "plain", "type": "Writer", "params": {"path": "plain.shp"}},
        {"id": "gpkg", "type": "Writer", "params": {"path": "sized.gpkg", "fields": {"name": {"length": 10}, "code": {"type": "integer"}}}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["shp", "Input"]},
        {"from": ["in", "Output"], "to": ["plain", "Input"]},
        {"from": ["in", "Output"], "to": ["gpkg", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    assert_eq!(r.nodes["shp"].files, [dir.join("sized.shp")]);
    assert_eq!(r.nodes["shp"].warnings, ["1 values in name are longer than 10 characters and were cut."]);

    // What the file itself says about its fields: (type, width, decimals).
    let described = |path: &std::path::Path| -> Vec<(String, u32, i32, i32)> {
        let ds = gdal::Dataset::open(path).unwrap();
        let layer = ds.layer(0).unwrap();
        layer.defn().fields().map(|f| (f.name(), f.field_type(), f.width(), f.precision())).collect()
    };
    use gdal::vector::OGRFieldType::{OFTInteger, OFTReal, OFTString};
    let sized = described(&dir.join("sized.shp"));
    assert_eq!(sized[0], ("code".to_string(), OFTInteger, 6, 0));
    assert_eq!(sized[1], ("name".to_string(), OFTString, 10, 0));
    assert_eq!(sized[2], ("share".to_string(), OFTReal, 24, 2));
    // Without settings a text field is as wide as its longest value, and the code is text.
    let plain = described(&dir.join("plain.shp"));
    assert_eq!((plain[0].1, plain[1].1, plain[1].2), (OFTString, OFTString, 15));

    let mut notes = Notes::default();
    let back = b.read(&dir.join("sized.shp"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(col(&back, "code"), [Value::Int(114), Value::Int(1480)]);
    assert_eq!(col(&back, "name"), [text("Upplands V"), text("Göteborg")]);
    assert_eq!(col(&back, "share"), [Value::Real(12.46), Value::Real(3.26)]);
    let g = described(&dir.join("sized.gpkg"));
    assert_eq!((g[0].1, g[1].1, g[1].2), (OFTInteger, OFTString, 10));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A web server for one test. It serves the files it is given by path, counts the
/// requests, and answers 404 for anything else.
fn web_server(files: Vec<(&'static str, &'static str, Vec<u8>)>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (mut buf, mut n) = ([0u8; 4096], 0);
            while n < buf.len() && !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf[n..]) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => n += k,
                }
            }
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let asked = request.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_string();
            let head = match files.iter().find(|f| f.0 == asked) {
                Some((_, kind, body)) => {
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", kind, body.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                    continue;
                }
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            };
            let _ = stream.write_all(head.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{}", port), hits)
}

#[test]
fn files_are_fetched_from_the_web() {
    let dir = tmp("web");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    b.write(&points(0, 5), &dir.join("served.gpkg"), &WriteOpts { layer: Some("points".into()), ..Default::default() }, &mut notes).unwrap();
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let geojson = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"Göteborg"},"geometry":{"type":"Point","coordinates":[11.9746,57.7089]}}]}"#;
    let (server, hits) = web_server(vec![
        ("/open/towns.csv", "text/csv; charset=utf-8", "code;name;lat;lon\n0114;Upplands Väsby;59,52;17,93\n1480;Göteborg;57,71;11,97\n".as_bytes().to_vec()),
        ("/open/points.gpkg", "application/geopackage+sqlite3", std::fs::read(dir.join("served.gpkg")).unwrap()),
        ("/open/zipped.zip", "application/zip", std::fs::read(data.join("zipped.zip")).unwrap()),
        ("/geoserver/wfs", "application/json", geojson.as_bytes().to_vec()),
        (
            "/rowstore/dataset/abc/json",
            "application/json",
            "\u{feff}{\"resultCount\":1,\"results\":[{\"\u{feff}id\":\"1\",\"namn\":\"Exempel\"}]}".as_bytes().to_vec(),
        ),
    ]);
    let ws = Workspace::from_json(
        &r#"{
      "nodes": [
        {"id": "csv", "type": "Reader", "params": {"path": "SERVER/open/towns.csv", "filename_attr": "file", "filename_part": "name"}},
        {"id": "gpkg", "type": "Reader", "params": {"path": "SERVER/open/points.gpkg?version=3", "limit": "3"}},
        {"id": "zip", "type": "Reader", "params": {"path": "SERVER/open/zipped.zip", "layer": "south"}},
        {"id": "wfs", "type": "Reader", "params": {"path": "SERVER/geoserver/wfs?service=WFS&request=GetFeature&typeName=towns&outputFormat=application/json"}},
        {"id": "rows", "type": "Reader", "params": {"path": "SERVER/rowstore/dataset/abc/json"}},
        {"id": "gone", "type": "Reader", "params": {"path": "SERVER/open/nothing.csv"}}
      ]
    }"#
        .replace("SERVER", &server),
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    // A CSV file is read by the program's own reader, with its codes kept as text.
    let csv = r.output("csv", "Output").unwrap();
    assert_eq!(col(csv, "code"), [text("0114"), text("1480")]);
    assert_eq!(col(csv, "file"), [text("towns.csv"), text("towns.csv")]);
    assert_eq!(csv.crs, Some(4326));
    assert!(r.log.iter().any(|l| l.msg.starts_with("Input: 2 out") && l.msg.contains("fetched towns.csv (")), "the log should say what was fetched");
    // What follows the ? is not part of the file name.
    let gpkg = r.output("gpkg", "Output").unwrap();
    assert_eq!((gpkg.len(), gpkg.crs), (3, Some(3006)));
    assert_eq!(r.nodes["gpkg"].sources[0].file_name().unwrap(), "points.gpkg");
    assert_eq!(col(r.output("zip", "Output").unwrap(), "name"), [text("Kinna")]);
    // An address without a file name gets one from what the server says it sends.
    let wfs = r.output("wfs", "Output").unwrap();
    assert_eq!((col(wfs, "name"), wfs.crs), (vec![text("Göteborg")], Some(4326)));
    assert_eq!(r.nodes["wfs"].sources[0].file_name().unwrap(), "wfs.json");
    // JSON that is not GeoJSON, as the row stores of open data give it, is read as a table.
    let rows = r.output("rows", "Output").unwrap();
    assert_eq!((rows.field_names(), col(rows, "namn")), (vec!["id".to_string(), "namn".to_string()], vec![text("Exempel")]));
    assert_eq!(r.nodes["gone"].error.as_deref(), Some(format!("The server answered 404 for {}/open/nothing.csv.", server).as_str()));
    assert_eq!((r.errors, hits.load(std::sync::atomic::Ordering::SeqCst)), (1, 6));

    // Every run fetches again, unless the reader is told to keep the file.
    let once = Workspace::from_json(
        &r#"{"nodes": [{"id": "csv", "type": "Reader", "params": {"path": "SERVER/open/towns.csv", "keep_download": "true"}}]}"#.replace("SERVER", &server),
    )
    .unwrap();
    let again =
        Workspace::from_json(&r#"{"nodes": [{"id": "csv", "type": "Reader", "params": {"path": "SERVER/open/towns.csv"}}]}"#.replace("SERVER", &server))
            .unwrap();
    let count = |ws: &Workspace| {
        let r = run(ws, Some(&b), &dir);
        assert_eq!((r.errors, r.output("csv", "Output").unwrap().len()), (0, 2));
        (hits.load(std::sync::atomic::Ordering::SeqCst), r.log[0].msg.clone())
    };
    let (kept, said) = count(&once);
    assert_eq!(kept, 6, "the copy from the run before is used");
    assert!(said.contains("read from the copy that was fetched earlier"), "{}", said);
    assert_eq!(count(&again).0, 7);
    assert_eq!(count(&again).0, 8);
    assert_eq!(count(&once).0, 8);

    // A server that is not there.
    let dead = Workspace::from_json(r#"{"nodes": [{"id": "csv", "type": "Reader", "params": {"path": "http://127.0.0.1:9/data.csv"}}]}"#).unwrap();
    let e = run(&dead, Some(&b), &dir).nodes["csv"].error.clone().unwrap();
    assert!(e.starts_with("Cannot fetch http://127.0.0.1:9/data.csv: "), "{}", e);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fetched_files_get_sensible_names() {
    use og_gdal::download_name as n;
    assert_eq!(n("https://example.org/open/roads.gpkg", None, None), "roads.gpkg");
    assert_eq!(n("https://example.org/open/roads.gpkg?v=2&x=a/b.csv#top", None, Some("application/octet-stream")), "roads.gpkg");
    // The server's own name for the file wins.
    assert_eq!(n("https://example.org/download?id=17", Some("attachment; filename=\"Vägar 2026.zip\""), Some("application/zip")), "Vägar_2026.zip");
    // No extension: the kind of content decides.
    assert_eq!(n("https://example.org/geoserver/wfs?request=GetFeature", None, Some("application/json;charset=UTF-8")), "wfs.json");
    assert_eq!(n("https://example.org/api/stops", None, Some("text/csv")), "stops.csv");
    assert_eq!(n("https://example.org/api/stops", None, None), "stops");
    assert_eq!(n("https://example.org/", None, Some("text/csv")), "download.csv");
    assert_eq!(n("https://example.org", None, None), "download");
    // Nothing in a name can lead out of the folder it is saved in.
    assert_eq!(n("https://example.org/a/..%2F..%2Fx.csv", Some("attachment; filename=\"../../evil.csv\""), None), "evil.csv");
    assert_ne!(og_gdal::download_dir("https://example.org/a.csv"), og_gdal::download_dir("https://example.org/b.csv"));
}

#[test]
fn geojson_is_read_and_written() {
    let dir = tmp("geojson");
    let b = GdalBackend::new();
    let mut notes = Notes::default();

    // A file as it comes from a web service: WGS 84, mixed geometries, a missing value.
    std::fs::write(
        dir.join("places.geojson"),
        r#"{"type": "FeatureCollection", "name": "places", "features": [
  {"type": "Feature", "properties": {"id": 1, "name": "Skövde", "share": 12.5}, "geometry": {"type": "Point", "coordinates": [13.8455, 58.3912]}},
  {"type": "Feature", "properties": {"id": 2, "name": "Göta kanal", "share": null}, "geometry": {"type": "LineString", "coordinates": [[14.0, 58.5], [14.2, 58.6]]}},
  {"type": "Feature", "properties": {"id": 3, "name": "Nowhere", "share": 1.0}, "geometry": null}
]}"#,
    )
    .unwrap();
    let t = b.read(&dir.join("places.geojson"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(t.crs, Some(4326));
    assert_eq!(t.field_names(), ["id", "name", "share"]);
    assert_eq!(col(&t, "id"), [Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert_eq!(col(&t, "name"), [text("Skövde"), text("Göta kanal"), text("Nowhere")]);
    assert_eq!(col(&t, "share"), [Value::Real(12.5), Value::Null, Value::Real(1.0)]);
    assert!(matches!(t.features[0].geom, Some(Geometry::Point(p)) if (p.x() - 13.8455).abs() < 1e-9 && (p.y() - 58.3912).abs() < 1e-9));
    assert!(matches!(t.features[1].geom, Some(Geometry::LineString(_))));
    assert!(t.features[2].geom.is_none());
    assert!(notes.warnings.is_empty(), "{:?}", notes.warnings);

    // Written and read again: the same values, single and multi polygons side by side, and
    // the coordinate system of the stream.
    let out = dir.join("out.geojson");
    let files = b.write(&sample(), &out, &WriteOpts::default(), &mut notes).unwrap();
    assert_eq!(files, std::slice::from_ref(&out));
    let back = b.read(&out, &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(back.crs, Some(3006));
    assert_eq!(back.field_names(), sample().field_names());
    assert_eq!(col(&back, "name"), [text("Upplands Väsby"), text("Göteborg")]);
    assert_eq!(col(&back, "invånare_totalt"), [Value::Int(48000), Value::Null]);
    assert_eq!(col(&back, "big"), [Value::Int(5_000_000_000), Value::Int(1)]);
    assert_eq!(col(&back, "muni_code"), [text("0114"), text("1480")], "a code with a leading zero stays text");
    assert!(matches!(back.features[0].geom, Some(Geometry::Polygon(_))));
    assert!(matches!(back.features[1].geom, Some(Geometry::MultiPolygon(ref m)) if m.0.len() == 2));
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(json["type"], "FeatureCollection");
    assert_eq!(json["features"].as_array().unwrap().len(), 2);
    assert_eq!(json["features"][0]["properties"]["name"], "Upplands Väsby");
    assert_eq!(json["features"][0]["geometry"]["coordinates"][0][0][0], 660000.0);

    // In a flow: chosen by format, written twice to the same file, and a note about the
    // coordinate system when it is not the one GeoJSON usually has.
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "places.geojson"}},
        {"id": "points", "type": "Tester", "params": {"test": "id < 3"}},
        {"id": "wgs", "type": "Writer", "params": {"path": "copy", "format": "geojson"}},
        {"id": "tm", "type": "Reprojector", "params": {"to": "EPSG:3006"}},
        {"id": "sweref", "type": "Writer", "params": {"path": "sweref.geojson"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["points", "Input"]},
        {"from": ["points", "Passed"], "to": ["wgs", "Input"]},
        {"from": ["points", "Passed"], "to": ["tm", "Input"]},
        {"from": ["tm", "Reprojected"], "to": ["sweref", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    for round in 0..2 {
        let r = run(&ws, Some(&b), &dir);
        for l in &r.log {
            println!("{:?} {}", l.level, l.msg);
        }
        assert_eq!((r.errors, r.nodes["wgs"].status, r.nodes["sweref"].status), (0, Status::Ok, Status::Ok), "round {}", round);
        assert_eq!(r.nodes["wgs"].files, [dir.join("copy.geojson")]);
        assert!(!r.log.iter().any(|l| l.msg.contains("not in WGS 84") && l.node.as_deref() == Some("wgs")));
        assert!(
            r.log.iter().any(|l| l.msg.contains("SWEREF 99 TM") && l.msg.contains("not in WGS 84 as GeoJSON usually is")),
            "the writer says which coordinate system the file has"
        );
    }
    let copy = b.read(&dir.join("copy.geojson"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!((copy.len(), copy.crs), (2, Some(4326)));
    let tm = b.read(&dir.join("sweref.geojson"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!((tm.len(), tm.crs), (2, Some(3006)));
    assert!(
        matches!(tm.features[0].geom, Some(Geometry::Point(p)) if (p.x() - 432_500.0).abs() < 1_000.0 && (p.y() - 6_472_900.0).abs() < 1_000.0),
        "{:?}",
        tm.features[0].geom
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a workbook says about one cell: (value, bold, fill colour, font name).
fn xlsx_cell(book: &umya_spreadsheet::Workbook, sheet: &str, at: &str) -> (String, bool, Option<String>, Option<String>) {
    let ws = book.sheet_by_name(sheet).unwrap();
    let cell = ws.cell(at);
    let value = cell.map(|c| c.value().to_string()).unwrap_or_default();
    let style = ws.style(at);
    let bold = style.font().is_some_and(|f| f.bold());
    let fill = style.background_color().map(|c| c.argb_str().to_string()).filter(|c| !c.is_empty());
    let font = style.font().map(|f| f.name().to_string());
    (value, bold, fill, font)
}

#[test]
fn workbooks_are_styled_and_keep_their_other_sheets() {
    let dir = tmp("styled");
    let b = GdalBackend::new();
    std::fs::write(dir.join("towns.csv"), "code;name;population;share\n0114;Upplands Väsby;48000;12,456\n1480;Göteborg;600000;\n1490;Borås;75000;3,5\n")
        .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "towns.csv"}},
        {"id": "styled", "type": "Writer", "params": {"path": "out/book.xlsx", "layer": "Towns", "font": "Arial", "colors": "blue", "bold_header": "true", "freeze": "true",
          "fields": {"share": {"decimals": 2}}}},
        {"id": "plain", "type": "Writer", "params": {"path": "out/book.xlsx", "layer": "Plain", "bold_header": "false", "freeze": "false"}},
        {"id": "ods", "type": "Writer", "params": {"path": "out/book", "format": "ods", "layer": "Towns", "colors": "green", "font": "Liberation Sans", "bold_header": "true", "freeze": "true"}},
        {"id": "ods2", "type": "Writer", "params": {"path": "out/book.ods", "layer": "Second", "bold_header": "false", "freeze": "false"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["styled", "Input"]},
        {"from": ["in", "Output"], "to": ["plain", "Input"]},
        {"from": ["in", "Output"], "to": ["ods", "Input"]},
        {"from": ["in", "Output"], "to": ["ods2", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!((r.errors, r.warnings), (0, 0));
    assert_eq!(r.nodes["ods"].files, [dir.join("out/book.ods")]);

    // Excel: the second writer added its sheet, and the first sheet kept its look.
    let path = dir.join("out/book.xlsx");
    let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
    let names: Vec<String> = book.sheet_collection().iter().map(|s| s.name().to_string()).collect();
    assert_eq!(names, ["Towns", "Plain"]);
    let (value, bold, fill, font) = xlsx_cell(&book, "Towns", "A1");
    assert_eq!((value.as_str(), bold, fill.as_deref(), font.as_deref()), ("code", true, Some("FF1F4E79"), Some("Arial")));
    assert_eq!(xlsx_cell(&book, "Towns", "A2").2, None, "the first row of data is not tinted");
    assert_eq!(xlsx_cell(&book, "Towns", "B3"), ("Göteborg".to_string(), false, Some("FFE9F0F7".to_string()), Some("Arial".to_string())));
    assert_eq!(xlsx_cell(&book, "Towns", "D3").2.as_deref(), Some("FFE9F0F7"), "an empty cell in a tinted row is tinted too");
    assert_eq!(xlsx_cell(&book, "Towns", "A2").0, "0114", "the code is text");
    let towns = book.sheet_by_name("Towns").unwrap();
    assert_eq!(towns.style("D2").number_format().map(|f| f.format_code().to_string()).as_deref(), Some("0.00"));
    let pane = towns.sheets_views().sheet_view_list()[0].pane().expect("the top row is locked");
    assert_eq!(pane.vertical_split(), 1.0);
    assert!(towns.column_dimension("B").unwrap().width() > towns.column_dimension("A").unwrap().width(), "columns are as wide as their contents");
    let plain = book.sheet_by_name("Plain").unwrap();
    assert!(!xlsx_cell(&book, "Plain", "A1").1 && plain.sheets_views().sheet_view_list().iter().all(|v| v.pane().is_none()));

    // Written again, the plain sheet is replaced in its place and the styled one is untouched.
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0);
    let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
    let names: Vec<String> = book.sheet_collection().iter().map(|s| s.name().to_string()).collect();
    assert_eq!(names, ["Towns", "Plain"]);
    assert!(xlsx_cell(&book, "Towns", "A1").1);

    // GDAL reads both, with the same values and types as before.
    let mut notes = Notes::default();
    let t = b.read(&path, &ReadOpts { layer: Some("Towns".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(t.field_names(), ["code", "name", "population", "share"]);
    assert_eq!(col(&t, "code"), ["0114", "1480", "1490"].map(text));
    assert_eq!(col(&t, "population"), [Value::Int(48000), Value::Int(600000), Value::Int(75000)]);
    assert_eq!(col(&t, "share"), [Value::Real(12.46), Value::Null, Value::Real(3.5)]);

    // LibreOffice: two sheets, read back by GDAL, and the styles are in the file.
    let ods = dir.join("out/book.ods");
    let sheets: Vec<(String, Option<u64>)> = b.layers(&ods).unwrap().into_iter().map(|l| (l.name, l.count)).collect();
    assert_eq!(sheets, [("Towns".to_string(), Some(3)), ("Second".to_string(), Some(3))]);
    let t = b.read(&ods, &ReadOpts { layer: Some("Towns".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(col(&t, "name"), ["Upplands Väsby", "Göteborg", "Borås"].map(text));
    assert_eq!(col(&t, "code"), ["0114", "1480", "1490"].map(text));
    assert_eq!(col(&t, "population"), [Value::Int(48000), Value::Int(600000), Value::Int(75000)]);
    let book = spreadsheet_ods::read_ods(&ods).unwrap();
    let towns = book.sheet(0);
    assert_eq!(towns.name(), "Towns");
    assert_eq!(towns.config().vert_split_pos, 1, "the top row is locked");
    let style_of = |row: u32, col: u32| towns.cellstyle(row, col).map(|s| s.as_str().to_string()).unwrap_or_default();
    assert!(
        style_of(0, 0).contains("head") && style_of(2, 1).contains("band") && style_of(1, 1).contains("row"),
        "{} {} {}",
        style_of(0, 0),
        style_of(2, 1),
        style_of(1, 1)
    );
    let look = |row: u32, col: u32, attr: &str, text: bool| {
        let s = book.cellstyle(style_of(row, col)).unwrap();
        (if text { s.textstyle() } else { s.cellstyle() }).attr(attr).map(str::to_string)
    };
    assert_eq!(look(0, 0, "fo:background-color", false).as_deref(), Some("#2e6b45"), "the top row has the colour of the scheme");
    assert_eq!(look(0, 0, "fo:font-weight", true).as_deref(), Some("bold"));
    assert_eq!(look(2, 1, "fo:background-color", false).as_deref(), Some("#e9f4ed"), "every second row is tinted");
    assert_eq!(look(1, 1, "fo:background-color", false), None);
    assert_eq!(look(1, 1, "style:font-name", true).as_deref(), Some("Liberation Sans"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A small GeoTIFF in SWEREF 99 TM: 4 columns and 3 rows of 100 m cells, with heights as
/// whole numbers in the first band and a share in the second. One cell has no value.
fn make_geotiff(path: &std::path::Path, crs: Option<u32>, size: (usize, usize)) {
    use gdal::raster::Buffer;
    let driver = gdal::DriverManager::get_driver_by_name("GTiff").unwrap();
    let (w, h) = size;
    let mut ds = driver.create_with_band_type::<i16, _>(path, w, h, 2).unwrap();
    ds.set_geo_transform(&[400_000.0, 100.0, 0.0, 6_500_300.0, 0.0, -100.0]).unwrap();
    if let Some(c) = crs {
        ds.set_spatial_ref(&gdal::spatial_ref::SpatialRef::from_epsg(c).unwrap()).unwrap();
    }
    if w * h <= 100 {
        let heights: Vec<i16> = (0..(w * h) as i16).map(|i| if i == 5 { -9999 } else { 100 + i }).collect();
        let shares: Vec<i16> = (0..(w * h) as i16).map(|i| if i == 5 { -9999 } else { i * 10 }).collect();
        for (b, data) in [(1, heights), (2, shares)] {
            let mut band = ds.rasterband(b).unwrap();
            band.set_no_data_value(Some(-9999.0)).unwrap();
            let mut buf = Buffer::new((w, h), data);
            band.write((0, 0), (w, h), &mut buf).unwrap();
        }
        gdal::Metadata::set_description(&mut ds.rasterband(1).unwrap(), "height").unwrap();
    }
}

#[test]
fn a_geotiff_is_read_as_points_cells_or_its_outline() {
    let dir = tmp("geotiff");
    let b = GdalBackend::new();
    make_geotiff(&dir.join("heights.tif"), Some(3006), (4, 3));
    make_geotiff(&dir.join("nocrs.tif"), None, (4, 3));
    make_geotiff(&dir.join("big.tif"), Some(3006), (3000, 2000));
    std::fs::write(dir.join("visits.csv"), "id;x;y\n1;400050;6500250\n2;400060;6500260\n3;400350;6500050\n4;390000;6500000\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "points", "type": "Reader", "params": {"path": "heights.tif"}},
        {"id": "cells", "type": "Reader", "params": {"path": "heights.tif", "raster_as": "cells"}},
        {"id": "outline", "type": "Reader", "params": {"path": "heights.tif", "raster_as": "extent"}},
        {"id": "few", "type": "Reader", "params": {"path": "heights.tif", "limit": "3"}},
        {"id": "nocrs", "type": "Reader", "params": {"path": "nocrs.tif"}},
        {"id": "nocrs_given", "type": "Reader", "params": {"path": "nocrs.tif", "crs": "EPSG:3006"}},
        {"id": "big", "type": "Reader", "params": {"path": "big.tif"}},
        {"id": "big_outline", "type": "Reader", "params": {"path": "big.tif", "raster_as": "extent"}},
        {"id": "big_some", "type": "Reader", "params": {"path": "big.tif", "limit": "10"}},
        {"id": "visits", "type": "Reader", "params": {"path": "visits.csv", "crs": "EPSG:3006"}},
        {"id": "count", "type": "PointCounter", "params": {"name": "visits"}},
        {"id": "out", "type": "Writer", "params": {"path": "out/cells.gpkg"}},
        {"id": "kinds", "type": "GeometryFilter"},
        {"id": "kinds2", "type": "GeometryFilter"}
      ],
      "edges": [
        {"from": ["cells", "Output"], "to": ["kinds", "Input"]},
        {"from": ["count", "Polygons"], "to": ["kinds2", "Input"]},
        {"from": ["visits", "Output"], "to": ["kinds2", "Input"]},
        {"from": ["cells", "Output"], "to": ["count", "Polygons"]},
        {"from": ["visits", "Output"], "to": ["count", "Points"]},
        {"from": ["count", "Polygons"], "to": ["out", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }

    // A point in the middle of each cell that has a value, with the bands as attributes.
    let p = r.output("points", "Output").unwrap();
    assert_eq!(p.crs, Some(3006));
    assert_eq!(p.field_names(), ["col", "row", "height", "band_2"]);
    assert_eq!(p.len(), 11, "the cell without a value is left out");
    assert!(r.log.iter().any(|l| l.msg.contains("1 cells without a value were left out")));
    assert_eq!(p.features[0].attrs, [Value::Int(0), Value::Int(0), Value::Int(100), Value::Int(0)]);
    assert!(matches!(p.features[0].geom, Some(Geometry::Point(pt)) if pt.x() == 400_050.0 && pt.y() == 6_500_250.0));
    // The last cell: column 3, row 2.
    assert_eq!(p.features[10].attrs, [Value::Int(3), Value::Int(2), Value::Int(111), Value::Int(110)]);
    assert!(matches!(p.features[10].geom, Some(Geometry::Point(pt)) if pt.x() == 400_350.0 && pt.y() == 6_500_050.0));

    // Squares, which can be used like any areas: the visits are counted in them.
    let c = r.output("cells", "Output").unwrap();
    assert_eq!(c.len(), 11);
    assert!(matches!(&c.features[0].geom, Some(Geometry::Polygon(sq)) if geo::Area::unsigned_area(sq) == 10_000.0));
    let counted = r.output("count", "Polygons").unwrap();
    let visits: Vec<Value> = col(counted, "visits");
    assert_eq!((visits[0].clone(), visits[10].clone()), (Value::Int(2), Value::Int(1)));
    assert_eq!(visits.iter().filter_map(|v| v.as_f64()).sum::<f64>(), 3.0, "the visit outside the raster is in no cell");
    assert_eq!(r.nodes["out"].status, Status::Ok);

    // The cells of a raster go to Raster in Filter by geometry type, also after a tool. Mixed
    // with other features they are areas like any other.
    assert_eq!(r.output("kinds", "Raster").unwrap().len(), 11);
    assert_eq!(r.output("kinds", "Area").unwrap().len(), 0);
    assert_eq!(r.output("kinds2", "Raster").unwrap().len(), 0);
    assert_eq!((r.output("kinds2", "Area").unwrap().len(), r.output("kinds2", "Point").unwrap().len()), (11, 4));

    // The outline: one area with the size of the raster.
    let o = r.output("outline", "Output").unwrap();
    assert_eq!(o.len(), 1);
    assert_eq!(col(o, "columns"), [Value::Int(4)]);
    assert_eq!(col(o, "rows"), [Value::Int(3)]);
    assert_eq!(col(o, "bands"), [Value::Int(2)]);
    assert_eq!(col(o, "cell_width"), [Value::Real(100.0)]);
    assert_eq!(col(o, "data_type"), [text("int16")]);
    assert_eq!(col(o, "nodata"), [Value::Real(-9999.0)]);
    assert!(matches!(&o.features[0].geom, Some(Geometry::Polygon(a)) if geo::Area::unsigned_area(a) == 120_000.0));

    // A limit, a raster that does not say its coordinate system, and one that is too large.
    assert_eq!(r.output("few", "Output").unwrap().len(), 3);
    assert!(r.nodes["nocrs"].error.as_ref().unwrap().contains("does not say which coordinate system"), "{:?}", r.nodes["nocrs"].error);
    assert_eq!(r.output("nocrs_given", "Output").unwrap().crs, Some(3006));
    let e = r.nodes["big"].error.as_ref().unwrap();
    assert!(e.contains("6000000 cells (3000 × 2000)") && e.contains("largest number of features"), "{}", e);
    assert_eq!(col(r.output("big_outline", "Output").unwrap(), "columns"), [Value::Int(3000)]);
    assert_eq!(r.output("big_some", "Output").unwrap().len(), 10);

    // As a layer, for the list of layers and for og info.
    let l = b.layers(&dir.join("heights.tif")).unwrap();
    assert_eq!((l.len(), l[0].name.as_str(), l[0].count, l[0].crs), (1, "heights", Some(12), Some(3006)));
    assert_eq!(l[0].geometry, "raster, 4 × 3 cells, 2 bands");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn features_are_added_to_layers_and_sheets_that_are_there() {
    let dir = tmp("append");
    let b = GdalBackend::new();
    std::fs::write(dir.join("a.csv"), "id;name;x;y\n1;A;400000;6400000\n2;B;410000;6410000\n").unwrap();
    std::fs::write(dir.join("b.csv"), "name;id;note;x;y\nC;3;new;420000;6420000\n").unwrap();
    std::fs::write(dir.join("w.csv"), "id;lon;lat\n4;12;58\n").unwrap();
    let model = |input: &str, crs: &str| {
        format!(
            r#"{{
      "nodes": [
        {{"id": "in", "type": "Reader", "params": {{"path": "{}", "crs": "{}"}}}},
        {{"id": "gpkg", "type": "Writer", "params": {{"path": "out/all.gpkg", "layer": "places", "mode": "append"}}}},
        {{"id": "shp", "type": "Writer", "params": {{"path": "out/all.shp", "mode": "append"}}}},
        {{"id": "json", "type": "Writer", "params": {{"path": "out/all.geojson", "mode": "append"}}}},
        {{"id": "xlsx", "type": "Writer", "params": {{"path": "out/all.xlsx", "layer": "Places", "mode": "append", "colors": "blue", "bold_header": "true"}}}},
        {{"id": "ods", "type": "Writer", "params": {{"path": "out/all.ods", "layer": "Places", "mode": "append", "colors": "blue", "freeze": "true"}}}},
        {{"id": "over", "type": "Writer", "params": {{"path": "out/over.gpkg", "layer": "places"}}}}
      ],
      "edges": [
        {{"from": ["in", "Output"], "to": ["gpkg", "Input"]}},
        {{"from": ["in", "Output"], "to": ["shp", "Input"]}},
        {{"from": ["in", "Output"], "to": ["json", "Input"]}},
        {{"from": ["in", "Output"], "to": ["xlsx", "Input"]}},
        {{"from": ["in", "Output"], "to": ["ods", "Input"]}},
        {{"from": ["in", "Output"], "to": ["over", "Input"]}}
      ]
    }}"#,
            input, crs
        )
    };
    let first = Workspace::from_json(&model("a.csv", "EPSG:3006")).unwrap();
    let second = Workspace::from_json(&model("b.csv", "EPSG:3006")).unwrap();
    for ws in [&first, &second] {
        let r = run(ws, Some(&b), &dir);
        for l in &r.log {
            println!("{:?} {}", l.level, l.msg);
        }
        assert_eq!(r.errors, 0);
    }
    let mut notes = Notes::default();
    let read = |file: &str, layer: Option<&str>, notes: &mut Notes| {
        b.read(&dir.join("out").join(file), &ReadOpts { layer: layer.map(String::from), ..Default::default() }, notes).unwrap()
    };
    // Every format has the three features, in the order they were written, and the new
    // attribute is added where the format allows it.
    for (file, layer) in [("all.gpkg", Some("places")), ("all.shp", None), ("all.geojson", None)] {
        let t = read(file, layer, &mut notes);
        assert_eq!(col(&t, "id"), [Value::Int(1), Value::Int(2), Value::Int(3)], "{}", file);
        assert_eq!(col(&t, "name"), [text("A"), text("B"), text("C")], "{}", file);
        assert_eq!(col(&t, "note"), [Value::Null, Value::Null, text("new")], "{}", file);
        assert_eq!(t.crs, Some(3006));
    }
    for file in ["all.xlsx", "all.ods"] {
        let t = read(file, Some("Places"), &mut notes);
        assert_eq!(t.field_names(), ["id", "name", "x", "y", "note"], "{}", file);
        assert_eq!(col(&t, "id"), [Value::Int(1), Value::Int(2), Value::Int(3)], "{}", file);
        assert_eq!(col(&t, "note"), [Value::Null, Value::Null, text("new")], "{}", file);
    }
    assert_eq!(read("over.gpkg", Some("places"), &mut notes).len(), 1, "without append the layer is replaced");
    // The bands of the colour scheme go on in the same rhythm.
    let book = umya_spreadsheet::reader::xlsx::read(dir.join("out/all.xlsx")).unwrap();
    assert_eq!(xlsx_cell(&book, "Places", "A2").2, None);
    assert_eq!(xlsx_cell(&book, "Places", "A3").2.as_deref(), Some("FFE9F0F7"));
    assert_eq!(xlsx_cell(&book, "Places", "A4").2, None);
    assert!(xlsx_cell(&book, "Places", "C1").1, "the new column has a bold name too");

    // A stream in another coordinate system is not added: its coordinates would be wrong.
    let wgs = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "w.csv", "crs": "EPSG:4326"}},
        {"id": "gpkg", "type": "Writer", "params": {"path": "out/all.gpkg", "layer": "places", "mode": "append"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["gpkg", "Input"]}]
    }"#,
    )
    .unwrap();
    let r = run(&wgs, Some(&b), &dir);
    let e = r.nodes["gpkg"].error.as_ref().unwrap();
    assert!(e.contains("is in EPSG:3006 and the stream in EPSG:4326") && e.contains("Reproject"), "{}", e);
    assert_eq!(read("all.gpkg", Some("places"), &mut notes).len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A web server on this computer that answers a few addresses, for as long as the test runs.
fn little_server() -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let (mut accept, mut kind, mut length) = (String::new(), String::new(), 0usize);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                let lower = line.to_lowercase();
                if lower.starts_with("accept:") {
                    accept = line[7..].trim().to_string();
                }
                if lower.starts_with("content-type:") {
                    kind = line[13..].trim().to_string();
                }
                if lower.starts_with("content-length:") {
                    length = line[15..].trim().parse().unwrap_or(0);
                }
            }
            let mut sent = vec![0u8; length];
            let _ = reader.read_exact(&mut sent);
            let method = first.split_whitespace().next().unwrap_or("").to_string();
            let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, body) = match path.as_str() {
                "/echo" => ("200 OK", format!("{} {} {}", method, kind, String::from_utf8_lossy(&sent))),
                "/town/1" => ("200 OK", format!("{{\"name\": \"Skövde\", \"accept\": \"{}\"}}", accept)),
                "/town/2" => ("200 OK", "{\"name\": \"Skara\"}".to_string()),
                "/slow" => {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    ("200 OK", "late".to_string())
                }
                _ => ("404 Not Found", "no such town".to_string()),
            };
            let mut s = stream;
            let _ = write!(
                s,
                "HTTP/1.1 {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                body.len(),
                body
            );
        }
    });
    format!("http://{}", addr)
}

#[test]
fn web_requests_put_the_answer_in_an_attribute() {
    let dir = tmp("http");
    let b = GdalBackend::new();
    let base = little_server();
    std::fs::write(dir.join("ids.csv"), "id;name\n1;a\n2;b\n3;c\n1;d\n").unwrap();
    let ws = Workspace::from_json(&format!(
        r#"{{
      "nodes": [
        {{"id": "in", "type": "Reader", "params": {{"path": "ids.csv"}}}},
        {{"id": "get", "type": "HttpCaller", "params": {{"url": "'$(base)/town/' || id", "headers": "Accept: application/json"}}}},
        {{"id": "fixed", "type": "HttpCaller", "params": {{"url": "$(base)/town/2", "response": "json", "status": "code"}}}},
        {{"id": "slow", "type": "HttpCaller", "params": {{"url": "'$(base)/slow'", "timeout": "1"}}}},
        {{"id": "nowhere", "type": "HttpCaller", "params": {{"url": "'http://127.0.0.1:1/'"}}}},
        {{"id": "bad", "type": "HttpCaller", "params": {{"url": "id +"}}}},
        {{"id": "json", "type": "JsonBuilder", "params": {{"attributes": ["id"]}}}},
        {{"id": "post", "type": "HttpCaller", "params": {{"url": "$(base)/echo", "method": "POST", "body": "json"}}}},
        {{"id": "alone", "type": "HttpCaller", "params": {{"url": "$(base)/town/2"}}}}
      ],
      "edges": [
        {{"from": ["in", "Output"], "to": ["get", "Input"]}},
        {{"from": ["in", "Output"], "to": ["fixed", "Input"]}},
        {{"from": ["in", "Output"], "to": ["slow", "Input"]}},
        {{"from": ["in", "Output"], "to": ["nowhere", "Input"]}},
        {{"from": ["in", "Output"], "to": ["bad", "Input"]}},
        {{"from": ["in", "Output"], "to": ["json", "Input"]}},
        {{"from": ["json", "Output"], "to": ["post", "Input"]}}
      ],
      "variables": [{{"name": "base", "value": "{}"}}]
    }}"#,
        base
    ))
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    let ok = r.output("get", "Output").unwrap();
    assert_eq!(ok.field_names(), ["id", "name", "_response", "_status"]);
    assert_eq!(col(ok, "id"), [Value::Int(1), Value::Int(2), Value::Int(1)]);
    assert_eq!(col(ok, "_response")[0], text("{\"name\": \"Skövde\", \"accept\": \"application/json\"}"), "the header was sent");
    assert_eq!(col(ok, "_status"), [Value::Int(200), Value::Int(200), Value::Int(200)]);
    let failed = r.output("get", "Failed").unwrap();
    assert_eq!(
        (col(failed, "id"), col(failed, "_status"), col(failed, "_error")),
        (vec![Value::Int(3)], vec![Value::Int(404)], vec![text("The server answered 404")])
    );
    assert_eq!(col(failed, "_response"), [text("no such town")]);
    assert!(r.log.iter().any(|l| l.msg.contains("4 requests to 3 addresses")));
    // An address without quotes is taken as it is, with names of its own for the answer.
    let f = r.output("fixed", "Output").unwrap();
    assert_eq!((f.len(), col(f, "code")[0].clone()), (4, Value::Int(200)));
    assert!(f.field_index("json").is_some());
    // No answer in time, and nobody there: they go to Failed with the reason.
    let slow = r.output("slow", "Failed").unwrap();
    assert_eq!(slow.len(), 4);
    assert!(col(slow, "_error")[0].to_text().contains("within 1 seconds"), "{:?}", col(slow, "_error")[0]);
    assert!(col(r.output("nowhere", "Failed").unwrap(), "_error")[0].to_text().starts_with("Cannot reach http://127.0.0.1:1/"));
    assert!(r.nodes["bad"].error.as_ref().unwrap().starts_with("The address:"));
    // POST sends the body with its content type, a request per feature.
    let posted = r.output("post", "Output").unwrap();
    assert_eq!(col(posted, "_response")[0], text("POST application/json {\"id\":1}"));
    assert_eq!(posted.len(), 4);
    // With nothing connected to Input, one request is sent.
    let alone = r.output("alone", "Output").unwrap();
    assert_eq!((alone.len(), col(alone, "_response")[0].clone()), (1, text("{\"name\": \"Skara\"}")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_is_written_as_png_jpg_and_geotiff_with_its_coordinates() {
    let dir = tmp("images");
    let b = GdalBackend::new();
    std::fs::write(dir.join("areas.csv"), "name;wkt\nA;POLYGON ((400000 6400000, 401000 6400000, 401000 6401000, 400000 6401000, 400000 6400000))\n").unwrap();
    std::fs::write(dir.join("dots.csv"), "id;lon;lat\n1;12;57.7\n").unwrap();
    let ws = Workspace::from_json(
        r##"{
      "nodes": [
        {"id": "areas", "type": "Reader", "params": {"path": "areas.csv", "crs": "EPSG:3006"}},
        {"id": "map", "type": "MapRenderer", "params": {"width": "300", "margin": "10", "background": "none",
          "layers": [{"from": "areas:Output", "fill": "#a6cee3", "stroke": "#1f78b4", "width": 2, "size": 6}]}},
        {"id": "png", "type": "Writer", "params": {"path": "out/map.png"}},
        {"id": "jpg", "type": "Writer", "params": {"path": "out/map", "format": "jpg"}},
        {"id": "tif", "type": "Writer", "params": {"path": "out/map.tif"}},
        {"id": "gpkg", "type": "Writer", "params": {"path": "out/outline.gpkg"}}
      ],
      "edges": [
        {"from": ["areas", "Output"], "to": ["map", "Layers"]},
        {"from": ["map", "Map"], "to": ["png", "Input"]},
        {"from": ["map", "Map"], "to": ["jpg", "Input"]},
        {"from": ["map", "Map"], "to": ["tif", "Input"]},
        {"from": ["map", "Map"], "to": ["gpkg", "Input"]}
      ]
    }"##,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    let out = dir.join("out");
    assert_eq!(r.nodes["png"].files, [out.join("map.png"), out.join("map.pgw"), out.join("map.png.aux.xml")]);
    assert_eq!(r.nodes["jpg"].files[1], out.join("map.jgw"));
    assert_eq!(r.nodes["jpg"].files[0], out.join("map.jpg"));
    assert_eq!(r.nodes["tif"].files, [out.join("map.tif")]);
    // Each picture is where the area is, in SWEREF 99 TM.
    for (file, bands) in [("map.png", 4), ("map.jpg", 3), ("map.tif", 4)] {
        let ds = gdal::Dataset::open(out.join(file)).unwrap();
        assert_eq!(ds.raster_size(), (300, 300), "{}", file);
        assert_eq!(ds.raster_count(), bands, "{}", file);
        let gt = ds.geo_transform().unwrap();
        assert!((gt[0] - 399_900.0).abs() < 1e-6 && (gt[3] - 6_401_100.0).abs() < 1e-6 && (gt[1] - 4.0).abs() < 1e-9, "{} {:?}", file, gt);
        let srs = ds.spatial_ref().unwrap();
        assert_eq!(srs.auth_code().ok(), Some(3006), "{}", file);
    }
    // The see-through background stays see-through in PNG and GeoTIFF, and is white in JPEG.
    let pixel = |file: &str, band: usize, x: isize, y: isize| -> u8 {
        let ds = gdal::Dataset::open(out.join(file)).unwrap();
        let buf = ds.rasterband(band).unwrap().read_as::<u8>((x, y), (1, 1), (1, 1), None).unwrap();
        buf.data()[0]
    };
    assert_eq!(pixel("map.png", 4, 2, 2), 0);
    assert_eq!(pixel("map.tif", 4, 2, 2), 0);
    assert_eq!((pixel("map.jpg", 1, 2, 2), pixel("map.jpg", 2, 2, 2)), (255, 255));
    assert_eq!((pixel("map.png", 1, 150, 150), pixel("map.png", 3, 150, 150), pixel("map.png", 4, 150, 150)), (0xa6, 0xe3, 255));
    // To a format for features the outline is written.
    let mut notes = Notes::default();
    let outline = b.read(&out.join("outline.gpkg"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!((outline.len(), outline.field_names()), (1, vec!["width".to_string(), "height".to_string(), "pixel_size".to_string()]));
    // A GeoTIFF read as an image comes back with its colours, place and size.
    let back = b.read(&out.join("map.tif"), &ReadOpts { raster: og_engine::io::RasterMode::Image, ..Default::default() }, &mut notes).unwrap();
    let img = back.image.as_ref().expect("an image");
    assert_eq!((img.width, img.height, back.crs), (300, 300, Some(3006)));
    assert_eq!(&img.rgba[(150 * 300 + 150) * 4..(150 * 300 + 150) * 4 + 4], &[0xa6, 0xce, 0xe3, 255]);
    // A raster of values is shown in grey from its lowest to its highest.
    make_geotiff(&dir.join("heights.tif"), Some(3006), (4, 3));
    let grey = b.read(&dir.join("heights.tif"), &ReadOpts { raster: og_engine::io::RasterMode::Image, ..Default::default() }, &mut notes).unwrap();
    let img = grey.image.as_ref().unwrap();
    assert_eq!((img.width, img.height), (4, 3));
    assert_eq!(&img.rgba[0..4], &[0, 0, 0, 255], "the lowest is black");
    assert_eq!(&img.rgba[11 * 4..12 * 4], &[255, 255, 255, 255], "the highest is white");
    assert_eq!(img.rgba[5 * 4 + 3], 0, "a cell without a value is see-through");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_is_written_as_a_pdf_page_with_a_title() {
    let dir = tmp("pdf");
    let b = GdalBackend::new();
    std::fs::write(dir.join("areas.csv"), "name;wkt\nA;POLYGON ((400000 6400000, 402000 6400000, 402000 6401000, 400000 6401000, 400000 6400000))\n").unwrap();
    let ws = Workspace::from_json(
        r##"{
      "nodes": [
        {"id": "areas", "type": "Reader", "params": {"path": "areas.csv", "crs": "EPSG:3006"}},
        {"id": "map", "type": "MapRenderer", "params": {"width": "800", "layers": [{"from": "areas:Output", "fill": "#a6cee3"}]}},
        {"id": "a4", "type": "Writer", "params": {"path": "out/a4.pdf", "pdf_title": "Vårdcentraler i Västra Götaland"}},
        {"id": "a3", "type": "Writer", "params": {"path": "out/a3", "format": "pdf", "pdf_size": "a3", "pdf_orientation": "portrait", "pdf_margin": "20"}},
        {"id": "fit", "type": "Writer", "params": {"path": "out/fit.pdf", "pdf_size": "fit", "pdf_margin": "0"}},
        {"id": "fit300", "type": "Writer", "params": {"path": "out/fit300.pdf", "pdf_size": "fit", "pdf_margin": "0", "pdf_dpi": "300"}},
        {"id": "own", "type": "Writer", "params": {"path": "out/own.pdf", "pdf_size": "custom", "pdf_width": "500", "pdf_height": "300", "pdf_compress": "jpeg", "pdf_quality": "60"}},
        {"id": "a0", "type": "Writer", "params": {"path": "out/a0.pdf", "pdf_size": "a0"}},
        {"id": "nothing", "type": "Writer", "params": {"path": "out/areas.pdf"}}
      ],
      "edges": [
        {"from": ["areas", "Output"], "to": ["map", "Layers"]},
        {"from": ["map", "Map"], "to": ["a4", "Input"]},
        {"from": ["map", "Map"], "to": ["a3", "Input"]},
        {"from": ["map", "Map"], "to": ["fit", "Input"]},
        {"from": ["map", "Map"], "to": ["fit300", "Input"]},
        {"from": ["map", "Map"], "to": ["own", "Input"]},
        {"from": ["map", "Map"], "to": ["a0", "Input"]},
        {"from": ["areas", "Output"], "to": ["nothing", "Input"]}
      ]
    }"##,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 1, "a stream without an image cannot be a PDF");
    assert!(r.nodes["nothing"].error.as_deref().unwrap().contains("no image to write as .pdf"));
    let out = dir.join("out");
    assert_eq!(r.nodes["a3"].files, [out.join("a3.pdf")]);
    let text = |f: &str| String::from_utf8_lossy(&std::fs::read(out.join(f)).unwrap()).into_owned();
    // The page size, in points, as the PDF states it.
    let media = |f: &str| -> (f64, f64) {
        let t = text(f);
        let at = t.find("/MediaBox").unwrap_or_else(|| panic!("no page size in {}", f));
        let nums: Vec<f64> = t[at..].split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|s| !s.is_empty()).take(4).map(|s| s.parse().unwrap()).collect();
        (nums[2], nums[3])
    };
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 4.0 && (a.1 - b.1).abs() < 4.0;
    // The map is wider than high, so A4 turns to landscape. A3 is kept upright as asked.
    assert!(near(media("a4.pdf"), (841.9, 595.3)), "{:?}", media("a4.pdf"));
    assert!(near(media("a3.pdf"), (841.9, 1190.6)), "{:?}", media("a3.pdf"));
    // Without margin, a page as large as the picture holds it at 150 pixels to the inch.
    let fit = media("fit.pdf");
    let img = r.output("map", "Map").unwrap().image.clone().unwrap();
    assert!(near(fit, (img.width as f64 * 72.0 / 150.0, img.height as f64 * 72.0 / 150.0)), "{:?}", fit);
    // At 300 pixels to the inch the page is half as large.
    assert!(near(media("fit300.pdf"), (fit.0 / 2.0, fit.1 / 2.0)), "{:?}", media("fit300.pdf"));
    // A size of one's own, as given, with the picture packed as JPEG.
    assert!(near(media("own.pdf"), (500.0 * 72.0 / 25.4, 300.0 * 72.0 / 25.4)), "{:?}", media("own.pdf"));
    assert!(text("own.pdf").contains("/DCTDecode") && !text("a4.pdf").contains("/DCTDecode"), "JPEG only where asked for");
    // A0 is large for a picture 800 pixels wide: the log says how coarse it prints.
    assert!(near(media("a0.pdf"), (3370.4, 2383.9)), "{:?}", media("a0.pdf"));
    let coarse = r.log.iter().any(|l| l.msg.contains("pixels to the inch, which looks coarse") && l.msg.contains("Make it"));
    assert!(coarse, "a warning about the resolution");
    // The title is written in the standard font, with å and ä in its encoding, and the page
    // has coordinates.
    let a4 = text("a4.pdf");
    // What is drawn on the page is packed. Each packed stream is unpacked to find the title.
    let raw = std::fs::read(out.join("a4.pdf")).unwrap();
    let mut drawn = String::new();
    let mut at = 0;
    while let Some(i) = raw[at..].windows(6).position(|w| w == b"stream") {
        let start = at + i + 6;
        let start = start + raw[start..].iter().take_while(|c| **c == b'\r' || **c == b'\n').count();
        let mut z = flate2::read::ZlibDecoder::new(&raw[start..]);
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut z, &mut buf);
        drawn.push_str(&String::from_utf8_lossy(&buf));
        at = start;
    }
    assert!(a4.contains("/FHelveticaBold") && drawn.contains("(V\\345rdcentraler i V\\344stra G\\366taland) Tj"), "the title is on the page");
    assert!(a4.contains("/Measure") && a4.contains("/GEO"), "the page has coordinates");
}

#[test]
fn terrain_is_read_as_a_grid_shaded_and_seen_in_3d() {
    let b = GdalBackend::new();
    let dir = tmp("terrain");
    // Two tiles of a terrain model side by side, as a folder of files: a slope that rises
    // to the east, 10 m cells, each tile 20 × 10 cells.
    std::fs::create_dir_all(dir.join("tiles")).unwrap();
    for (k, x0) in [(0, 400000.0), (1, 400200.0)] {
        let mut text = format!("ncols 20\nnrows 10\nxllcorner {}\nyllcorner 6500000\ncellsize 10\nNODATA_value -9999\n", x0);
        for r in 0..10 {
            let row: Vec<String> = (0..20)
                .map(|c| if k == 1 && r == 0 && c == 19 { "-9999".to_string() } else { format!("{}", (k * 20 + c) as f64 * 2.0 + r as f64 * 0.5) })
                .collect();
            text += &(row.join(" ") + "\n");
        }
        std::fs::write(dir.join("tiles").join(format!("tile_{}.asc", k)), text).unwrap();
    }
    std::fs::copy(root().join("examples/terrain.tif"), dir.join("terrain.tif")).unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "tiles", "type": "Reader", "params": {"path": "tiles", "pattern": "*.asc", "raster_as": "grid", "crs": "EPSG:3006"}},
        {"id": "dem", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "grid", "cell_size": "40"}},
        {"id": "points", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "extent"}},
        {"id": "shade", "type": "TerrainShader", "params": {"width": "200", "haze": "40"}},
        {"id": "view", "type": "TerrainRenderer", "params": {"width": "320", "height": "200", "quality": "1"}},
        {"id": "wrong", "type": "TerrainShader", "params": {}},
        {"id": "tile_shade", "type": "TerrainShader", "params": {}},
        {"id": "out_shade", "type": "Writer", "params": {"path": "out/relief.tif"}},
        {"id": "out_png", "type": "Writer", "params": {"path": "out/relief.png"}},
        {"id": "out_view", "type": "Writer", "params": {"path": "out/view.png"}}],
      "edges": [
        {"from": ["dem", "Output"], "to": ["shade", "Heights"]},
        {"from": ["dem", "Output"], "to": ["view", "Heights"]},
        {"from": ["points", "Output"], "to": ["wrong", "Heights"]},
        {"from": ["tiles", "Output"], "to": ["tile_shade", "Heights"]},
        {"from": ["shade", "Image"], "to": ["out_shade", "Input"]},
        {"from": ["shade", "Image"], "to": ["out_png", "Input"]},
        {"from": ["view", "Image"], "to": ["out_view", "Input"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    // The tiles are put together into one grid of 40 × 10 cells, with the cell without a value.
    let tiles = r.output("tiles", "Output").unwrap();
    let g = tiles.grid.as_ref().expect("the tiles are a grid");
    assert_eq!((g.width, g.height, g.crs), (40, 10, Some(3006)));
    assert_eq!(g.get(0, 9), Some(4.5));
    assert_eq!(g.get(39, 9), Some(78.0 + 4.5));
    assert_eq!(g.get(39, 0), None);
    assert_eq!(tiles.len(), 1, "the stream is the outline of the grid");
    // A cell size of 40 reads the 20 m model at half the cells each way.
    let dem = r.output("dem", "Output").unwrap().grid.clone().unwrap();
    assert_eq!((dem.width, dem.height), (320, 240));
    assert!((dem.cell_size().0 - 40.0).abs() < 1e-9);
    // The relief has the coordinates of the terrain, and the 3D view has none.
    assert_eq!(r.nodes["shade"].status, Status::Ok);
    let relief = gdal::Dataset::open(dir.join("out/relief.tif")).unwrap();
    assert_eq!(relief.raster_size(), (200, 150));
    let gt = relief.geo_transform().unwrap();
    assert!((gt[0] - 412000.0).abs() < 1e-6 && (gt[1] - 64.0).abs() < 1e-6, "{:?}", gt);
    assert!(dir.join("out/relief.pgw").exists(), "a relief as PNG has a world file");
    let view = gdal::Dataset::open(dir.join("out/view.png")).unwrap();
    assert_eq!(view.raster_size(), (320, 200));
    assert!(!dir.join("out/view.pgw").exists(), "a 3D view is not placed on a map");
    // Heights that are not a grid give a clear error.
    assert!(r.nodes["wrong"].error.as_deref().unwrap().contains("has no grid"), "{:?}", r.nodes["wrong"].error);
    assert_eq!(r.nodes["tile_shade"].status, Status::Ok);
}

#[test]
fn rasters_are_reprojected_contoured_and_geometries_repaired() {
    let b = GdalBackend::new();
    let dir = tmp("t61");
    std::fs::copy(root().join("examples/terrain.tif"), dir.join("terrain.tif")).unwrap();
    std::fs::write(
        dir.join("bad.csv"),
        "id;wkt\n1;POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))\n2;POLYGON ((20 0, 30 0, 30 10, 20 10, 20 0))\n3;LINESTRING (0 0, 0 0)\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "dem", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "grid"}},
        {"id": "img", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "image"}},
        {"id": "g3007", "type": "Reprojector", "params": {"crs": "EPSG:3007"}},
        {"id": "i4326", "type": "Reprojector", "params": {"crs": "EPSG:4326"}},
        {"id": "c", "type": "ContourGenerator", "params": {"interval": "50"}},
        {"id": "ca", "type": "ContourGenerator", "params": {"interval": "100", "kind": "areas", "name": "h"}},
        {"id": "bad", "type": "Reader", "params": {"path": "bad.csv", "crs": "EPSG:3006"}},
        {"id": "chk", "type": "GeometryValidator", "params": {}},
        {"id": "find", "type": "GeometryValidator", "params": {"repair": "false"}}],
      "edges": [
        {"from": ["dem", "Output"], "to": ["g3007", "Input"]},
        {"from": ["img", "Output"], "to": ["i4326", "Input"]},
        {"from": ["dem", "Output"], "to": ["c", "Heights"]},
        {"from": ["dem", "Output"], "to": ["ca", "Heights"]},
        {"from": ["bad", "Output"], "to": ["chk", "Input"]},
        {"from": ["bad", "Output"], "to": ["find", "Input"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    // The grid is warped into SWEREF 99 12 00, east as x, about as gdalwarp makes it.
    let g = r.output("g3007", "Reprojected").unwrap().grid.clone().unwrap();
    assert_eq!(g.crs, Some(3007));
    assert!((g.geotransform[0] - 237132.16).abs() < 1.0 && (g.geotransform[3] - 6482743.95).abs() < 1.0, "{:?}", g.geotransform);
    assert!((g.width as i64 - 661).abs() <= 1 && (g.height as i64 - 508).abs() <= 1, "{} × {}", g.width, g.height);
    let (lo, hi) = g.range().unwrap();
    assert!((lo - 36.1).abs() < 1.0 && (hi - 249.9).abs() < 2.0, "{} {}", lo, hi);
    // The picture lands in degrees, with see-through corners where it does not reach.
    let t = r.output("i4326", "Reprojected").unwrap();
    let img = t.image.clone().unwrap();
    assert_eq!(t.crs, Some(4326));
    assert!(img.geotransform[0] > 13.0 && img.geotransform[0] < 14.0 && img.geotransform[3] > 58.0 && img.geotransform[3] < 59.0, "{:?}", img.geotransform);
    assert!(img.rgba.chunks(4).any(|p| p[3] == 0) && img.rgba.chunks(4).any(|p| p[3] == 255));
    // Contour lines every 50 m: 100, 150 and 200 lie within 36 to 250.
    let c = r.output("c", "Contours").unwrap();
    let heights: std::collections::BTreeSet<i64> = col(c, "height").iter().map(|v| v.as_f64().unwrap() as i64).collect();
    assert_eq!(heights.into_iter().collect::<Vec<_>>(), [50, 100, 150, 200]);
    assert_eq!(c.crs, Some(3006));
    let ca = r.output("ca", "Contours").unwrap();
    assert!(ca.field_index("h_min").is_some() && ca.field_index("h_max").is_some());
    assert!(ca.features.iter().all(|f| matches!(f.geom, Some(Geometry::MultiPolygon(_)) | Some(Geometry::Polygon(_)))));
    // Repairs need GDAL built with GEOS, as OSGeo4W and Linux distributions have it.
    let geos = unsafe { gdal_sys::OGRGetGEOSVersion(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
    if !geos {
        println!("GDAL has no GEOS: repairs are not tested");
        return;
    }
    // The bow tie is repaired, the square is fine, and a line of one point cannot be repaired.
    assert_eq!(col(r.output("chk", "Valid").unwrap(), "id"), [Value::Int(2)]);
    let fixed = r.output("chk", "Repaired").unwrap();
    assert_eq!(col(fixed, "id"), [Value::Int(1)]);
    assert!(col(fixed, "problem")[0].to_text().contains("self-intersection"), "{:?}", col(fixed, "problem"));
    assert!(og_engine::nodes::geometry_problem(fixed.features[0].geom.as_ref().unwrap()).is_none());
    assert_eq!(col(r.output("chk", "Invalid").unwrap(), "id"), [Value::Int(3)]);
    assert_eq!(r.output("find", "Invalid").unwrap().len(), 2, "without repair both go to Invalid");
}

#[test]
fn openstreetmap_is_read_with_its_tags() {
    let b = GdalBackend::new();
    if gdal::DriverManager::get_driver_by_name("OSM").is_err() {
        println!("GDAL has no OpenStreetMap driver: not tested");
        return;
    }
    let dir = tmp("osm");
    std::fs::copy(root().join("examples/village.osm.pbf"), dir.join("village.osm.pbf")).unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "pts", "type": "Reader", "params": {"path": "village.osm.pbf", "layer": "points"}},
        {"id": "areas", "type": "Reader", "params": {"path": "village.osm.pbf", "layer": "multipolygons"}},
        {"id": "raw", "type": "Reader", "params": {"path": "village.osm.pbf", "layer": "points", "osm_tags": "false"}},
        {"id": "two", "type": "Reader", "params": {"path": "village.osm.pbf", "layer": "points", "limit": "2"}},
        {"id": "rels", "type": "Reader", "params": {"path": "village.osm.pbf", "layer": "multilinestrings"}},
        {"id": "all", "type": "Reader", "params": {"path": "village.osm.pbf", "layer": "*", "osm_tags": "all"}}],
      "edges": []}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log.iter().map(|l| l.msg.clone()).collect::<Vec<_>>());
    let pts = r.output("pts", "Output").unwrap();
    assert_eq!(col(pts, "name")[0], text("Kaffestugan"));
    assert_eq!(col(pts, "amenity")[0], text("cafe"));
    assert_eq!(col(pts, "religion")[3], text("christian"));
    assert!(pts.field_index("other_tags").is_none());
    let areas = r.output("areas", "Output").unwrap();
    assert_eq!(areas.len(), 7);
    assert!(col(areas, "landuse").contains(&text("forest")) && col(areas, "natural").contains(&text("water")));
    assert!(r.output("raw", "Output").unwrap().field_index("other_tags").is_some());
    // One layer at a time: the others are not kept in memory meanwhile.
    assert_eq!(r.output("two", "Output").unwrap().len(), 2);
    assert!(r.output("rels", "Output").is_some());
    assert!(r.output("all", "Output").unwrap().len() >= pts.len() + areas.len());
    assert_eq!(b.layers(&dir.join("village.osm.pbf")).unwrap().len(), 5);
}

#[test]
fn a_pdf_has_layers_that_can_be_turned_off() {
    let b = GdalBackend::new();
    let dir = tmp("lpdf");
    for f in ["subregions.gpkg", "towns.csv", "routes.shp", "routes.shx", "routes.dbf", "routes.prj", "routes.cpg"] {
        std::fs::copy(root().join("examples").join(f), dir.join(f)).unwrap();
    }
    let ws = Workspace::from_json(
        r##"{"nodes": [
        {"id": "areas", "type": "Reader", "name": "Subregions", "params": {"path": "subregions.gpkg"}},
        {"id": "routes", "type": "Reader", "name": "Routes", "params": {"path": "routes.shp"}},
        {"id": "towns", "type": "Reader", "name": "Towns", "params": {"path": "towns.csv", "crs": "EPSG:4326"}},
        {"id": "pdf", "type": "LayeredPdfWriter", "params": {"path": "out/layers", "crs": "EPSG:3006", "pdf_title": "Västra Götaland",
          "layers": [{"from": "towns:Output"}, {"from": "routes:Output", "stroke": "#b2182b"}, {"from": "areas:Output", "show": false}]}}],
      "edges": [{"from": ["areas", "Output"], "to": ["pdf", "Layers"]}, {"from": ["routes", "Output"], "to": ["pdf", "Layers"]}, {"from": ["towns", "Output"], "to": ["pdf", "Layers"]}]}"##,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    let file = dir.join("out/layers.pdf");
    assert_eq!(r.nodes["pdf"].files, std::slice::from_ref(&file));
    assert!(r.log.iter().any(|l| l.msg == "pdf wrote 3 layers to layers.pdf" || l.msg.ends_with("wrote 3 layers to layers.pdf")));
    // The layers by name, from the bottom up, the subregions off, and the page has coordinates.
    let ds = gdal::Dataset::open(&file).unwrap();
    let names: Vec<String> = gdal::Metadata::metadata_domain(&ds, "LAYERS").unwrap_or_default();
    assert!(names.iter().any(|n| n.ends_with("=Subregions")) && names.iter().any(|n| n.ends_with("=Towns")), "{:?}", names);
    assert_eq!(gdal::Metadata::metadata_item(&ds, "LAYER_00_NAME", "LAYERS").as_deref(), Some("Subregions"));
    assert!(ds.geo_transform().is_ok());
    let raw = std::fs::read(&file).unwrap();
    assert!(raw.windows(4).any(|w| w == b"/OFF"), "a layer is off when the PDF opens");
}

#[test]
fn postgis_tables_are_read_when_a_database_is_there() {
    // Runs against a local test database when there is one, as on the machine where
    // Ographis is built: gistest with the user gis. Otherwise it says so and passes.
    let b = GdalBackend::new();
    let dir = tmp("pg");
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "pg", "type": "PostgisReader", "params": {"database": "gistest", "user": "gis", "password": "$(password)", "tables": ["vgr.subregions", "public.towns"]}},
        {"id": "q", "type": "PostgisReader", "params": {"database": "gistest", "user": "gis", "password": "$(password)", "sql": "SELECT name, geom FROM towns WHERE name LIKE 'B%'"}},
        {"id": "w", "type": "PostgisReader", "params": {"database": "gistest", "user": "gis", "password": "$(password)", "tables": ["public.towns"], "where": "name = 'Borås'"}},
        {"id": "bad", "type": "PostgisReader", "params": {"database": "gistest", "user": "gis", "password": "not-the-one", "tables": ["public.towns"]}}],
      "edges": [], "variables": [{"name": "password", "value": "gis-test"}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    if r.nodes["pg"].status != Status::Ok {
        println!("No test database: {:?}", r.nodes["pg"].error);
        return;
    }
    let t = r.output("pg", "Output").unwrap();
    assert_eq!(t.len(), 4 + 39);
    assert_eq!(col(t, "table")[0], text("vgr.subregions"));
    assert_eq!(t.crs, Some(4326));
    assert!(!r.output("q", "Output").unwrap().is_empty());
    assert_eq!(r.output("w", "Output").unwrap().len(), 1);
    let e = r.nodes["bad"].error.clone().unwrap();
    assert!(e.contains("password authentication failed") && !e.contains("not-the-one"), "{}", e);
}

#[test]
fn postgis_tables_are_written_when_a_database_is_there() {
    // As above: against the local test database when there is one.
    let b = GdalBackend::new();
    let dir = tmp("pgw");
    std::fs::write(
        dir.join("areas.csv"),
        "Namn;Area km2;wkt\nA;1,5;POLYGON ((319000 6400000, 320000 6400000, 320000 6401000, 319000 6400000))\nB;2;MULTIPOLYGON (((330000 6400000, 331000 6400000, 331000 6401000, 330000 6400000)))\n",
    )
    .unwrap();
    // A connection from the settings, by its name, as models from 0.63 have it.
    og_engine::settings::use_settings(Some(og_engine::settings::Settings {
        connections: vec![og_engine::settings::Connection {
            name: "Local test".into(),
            kind: "postgis".into(),
            database: "gistest".into(),
            user: "gis".into(),
            password: "gis-test".into(),
            ..Default::default()
        }],
        ..Default::default()
    }));
    let conn = r#""connection": "Local test""#;
    let ws = Workspace::from_json(&format!(
        r#"{{"nodes": [
        {{"id": "in", "type": "Reader", "params": {{"path": "areas.csv", "crs": "EPSG:3006"}}}},
        {{"id": "out", "type": "PostgisWriter", "params": {{{conn}, "schema": "vgr", "table": "Test Areas", "geometry": "multipolygon", "crs": "EPSG:4326", "geom_column": "shape"}}}},
        {{"id": "single", "type": "PostgisWriter", "params": {{{conn}, "table": "test_single", "geometry": "polygon"}}}},
        {{"id": "points", "type": "PostgisWriter", "params": {{{conn}, "table": "test_wrong", "geometry": "point"}}}},
        {{"id": "noschema", "type": "PostgisWriter", "params": {{{conn}, "schema": "nowhere", "table": "x"}}}}],
      "edges": [
        {{"from": ["in", "Output"], "to": ["out", "Input"]}},
        {{"from": ["in", "Output"], "to": ["single", "Input"]}},
        {{"from": ["in", "Output"], "to": ["points", "Input"]}},
        {{"from": ["in", "Output"], "to": ["noschema", "Input"]}}],
      "variables": [{{"name": "password", "value": "gis-test"}}]}}"#
    ))
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    if r.nodes["out"].error.as_deref().is_some_and(|e| e.contains("PQconnectdb") || e.contains("Connection refused")) {
        println!("No test database: {:?}", r.nodes["out"].error);
        return;
    }
    assert_eq!(r.nodes["out"].error, None);
    assert_eq!(r.nodes["single"].error, None);
    assert!(r.nodes["points"].error.as_deref().unwrap().contains("not points"));
    assert!(r.nodes["noschema"].error.as_deref().unwrap().contains("no schema nowhere"));
    // A name that is not in the settings says where connections are made.
    let missing =
        Workspace::from_json(r#"{"nodes": [{"id": "x", "type": "PostgisReader", "params": {"connection": "Elsewhere", "tables": ["t"]}}], "edges": []}"#)
            .unwrap();
    let e = run(&missing, Some(&b), &dir).nodes["x"].error.clone().unwrap();
    assert!(e.contains("no database connection called Elsewhere") && e.contains("Local test") && e.contains("Settings"), "{}", e);
    // Read back: in the schema, lower case, multi, in WGS 84, with the column named shape.
    let read = Workspace::from_json(
        r#"{"nodes": [
        {"id": "pg", "type": "PostgisReader", "params": {"connection": "local TEST", "sql": "SELECT GeometryType(shape) AS kind, ST_SRID(shape) AS srid, area_km2 AS area FROM vgr.test_areas ORDER BY 1"}},
        {"id": "s", "type": "PostgisReader", "params": {"database": "gistest", "user": "gis", "password": "gis-test", "sql": "SELECT GeometryType(geom) AS kind FROM test_single"}}],
      "edges": []}"#,
    )
    .unwrap();
    let r = run(&read, Some(&b), &dir);
    let t = r.output("pg", "Output").unwrap();
    assert_eq!(col(t, "kind"), vec![text("MULTIPOLYGON"), text("MULTIPOLYGON")]);
    assert_eq!(col(t, "srid")[0], Value::Int(4326));
    assert_eq!(col(t, "area")[0], Value::Real(1.5));
    assert_eq!(col(r.output("s", "Output").unwrap(), "kind"), vec![text("POLYGON"), text("POLYGON")]);
    // Writing again replaces; adding doubles.
    let again = ws.to_json().replace(r#""geom_column": "shape""#, r#""geom_column": "shape", "mode": "append""#);
    let r2 = run(&Workspace::from_json(&again).unwrap(), Some(&b), &dir);
    assert_eq!(r2.nodes["out"].error, None);
    let r = run(&read, Some(&b), &dir);
    assert_eq!(r.output("pg", "Output").unwrap().len(), 4);
}

#[test]
fn outputs_split_by_names_from_expressions() {
    let dir = tmp("split64");
    let b = GdalBackend::new();
    std::fs::write(dir.join("towns.csv"), "name;kommun;wkt\nA;Borås;POINT (1 1)\nB;Mark;POINT (2 2)\nC;Borås;POINT (3 3)\nD;;POINT (4 4)\n").unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "towns.csv", "crs": "EPSG:3006"}},
        {"id": "files", "type": "Writer", "params": {"path": "out/towns_{lower(kommun)}.gpkg"}},
        {"id": "layers", "type": "Writer", "params": {"path": "out/all.gpkg", "layer": "kommun_{kommun}"}},
        {"id": "sheets", "type": "Writer", "params": {"path": "out/sheets.xlsx", "layer": "{kommun}"}},
        {"id": "dated", "type": "Writer", "params": {"path": "out/towns_{now('%Y')}.csv"}},
        {"id": "bad", "type": "Writer", "params": {"path": "out/{nothing}.csv"}}],
      "edges": [
        {"from": ["in", "Output"], "to": ["files", "Input"]},
        {"from": ["in", "Output"], "to": ["layers", "Input"]},
        {"from": ["in", "Output"], "to": ["sheets", "Input"]},
        {"from": ["in", "Output"], "to": ["dated", "Input"]},
        {"from": ["in", "Output"], "to": ["bad", "Input"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    let mut notes = Notes::default();
    let read = |f: &str, layer: Option<&str>, notes: &mut Notes| {
        b.read(&dir.join(f), &ReadOpts { layer: layer.map(str::to_string), ..Default::default() }, notes).unwrap()
    };
    assert_eq!(read("out/towns_borås.gpkg", None, &mut notes).len(), 2);
    assert_eq!(read("out/towns_mark.gpkg", None, &mut notes).len(), 1);
    assert_eq!(read("out/towns_empty.gpkg", None, &mut notes).len(), 1, "an empty value is called empty");
    let layers: Vec<String> = b.layers(&dir.join("out/all.gpkg")).unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(layers.len(), 3, "{:?}", layers);
    assert!(layers.contains(&"kommun_Borås".to_string()));
    let sheets: Vec<String> = b.layers(&dir.join("out/sheets.xlsx")).unwrap().into_iter().map(|l| l.name).collect();
    assert!(sheets.contains(&"Mark".to_string()) && sheets.contains(&"Borås".to_string()), "{:?}", sheets);
    let year = chrono_year();
    assert!(dir.join(format!("out/towns_{}.csv", year)).exists());
    assert!(r.nodes["bad"].error.as_deref().unwrap().contains("nothing"), "{:?}", r.nodes["bad"].error);
    assert!(r.log.iter().any(|l| l.msg.contains("3 files, one per name")));
}

fn chrono_year() -> String {
    // The year of the clock, as now('%Y') gives it, without a date library in the tests.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    (1970 + (secs as f64 / 31_556_952.0) as u64).to_string()
}

#[test]
fn sql_is_run_against_postgis_for_each_feature() {
    let b = GdalBackend::new();
    let dir = tmp("sql65");
    std::fs::write(dir.join("names.csv"), "who;wkt\nBorås;POINT (1 1)\nSkövde;POINT (2 2)\nO'Nowhere;POINT (3 3)\n").unwrap();
    let conn = r#""database": "gistest", "user": "gis", "password": "gis-test""#;
    let ws = Workspace::from_json(&format!(
        r#"{{"nodes": [
        {{"id": "in", "type": "Reader", "params": {{"path": "names.csv", "crs": "EPSG:3006"}}}},
        {{"id": "once", "type": "SqlExecutor", "params": {{{c}, "sql": "SELECT count(*) AS n FROM towns"}}}},
        {{"id": "each", "type": "SqlExecutor", "params": {{{c}, "sql": "SELECT name, population, geom FROM towns WHERE name = {{who}}"}}}},
        {{"id": "noattr", "type": "SqlExecutor", "params": {{{c}, "sql": "SELECT population FROM towns WHERE name = {{who}}", "keep": "false"}}}},
        {{"id": "make", "type": "SqlExecutor", "params": {{{c}, "sql": "DROP TABLE IF EXISTS public.gb_sql_test; CREATE TABLE public.gb_sql_test (n int)"}}}},
        {{"id": "bad", "type": "SqlExecutor", "params": {{{c}, "sql": "SELECT * FROM no_such_table WHERE x = {{who}}"}}}},
        {{"id": "typo", "type": "SqlExecutor", "params": {{{c}, "sql": "SELECT {{whom}}"}}}},
        {{"id": "json", "type": "SqlExecutor", "params": {{{c}, "sql": "SELECT '{{\"a\": 1}}'::jsonb ->> 'a' AS a"}}}}],
      "edges": [
        {{"from": ["in", "Output"], "to": ["each", "Input"]}},
        {{"from": ["in", "Output"], "to": ["noattr", "Input"]}},
        {{"from": ["in", "Output"], "to": ["bad", "Input"]}},
        {{"from": ["in", "Output"], "to": ["typo", "Input"]}}]}}"#,
        c = conn
    ))
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    if r.nodes["once"].status != Status::Ok {
        println!("No test database: {:?}", r.nodes["once"].error);
        return;
    }
    // Once, with nothing connected.
    assert_eq!(col(r.output("once", "Result").unwrap(), "n"), [Value::Int(39)]);
    // Once per feature, with the value quoted, and the attributes of the feature along.
    let each = r.output("each", "Result").unwrap();
    assert_eq!(each.len(), 2, "no town is called O'Nowhere, and the quote did no harm");
    assert_eq!(each.field_names()[..3], ["name", "population", "who"]);
    assert!(each.features[0].geom.is_some() && each.crs.is_some());
    assert_eq!(r.output("each", "Sent").unwrap().len(), 3);
    assert_eq!(r.output("noattr", "Result").unwrap().field_names(), ["population"]);
    // A statement that sends nothing back.
    assert_eq!(r.nodes["make"].status, Status::Ok, "{:?}", r.nodes["make"].error);
    // A failing statement sends the feature to Failed with the reason.
    let failed = r.output("bad", "Failed").unwrap();
    assert_eq!(failed.len(), 3);
    assert!(col(failed, "_error")[0].to_text().contains("no_such_table"), "{:?}", col(failed, "_error")[0]);
    assert!(r.nodes["typo"].error.as_deref().unwrap().contains("{whom}"), "{:?}", r.nodes["typo"].error);
    assert_eq!(col(r.output("json", "Result").unwrap(), "a"), [text("1")], "braces that are not an attribute are left as they are");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn images_keep_every_band_and_large_ones_are_read_smaller() {
    let dir = tmp("bands65");
    let b = GdalBackend::new();
    let drv = gdal::DriverManager::get_driver_by_name("GTiff").unwrap();
    // Three bands of decimal numbers, as from a satellite.
    {
        let mut ds = drv.create_with_band_type::<f32, _>(dir.join("three.tif"), 4, 2, 3).unwrap();
        ds.set_geo_transform(&[100.0, 10.0, 0.0, 500.0, 0.0, -10.0]).unwrap();
        for band in 1..=3 {
            let data: Vec<f32> = (0..8).map(|i| band as f32 * 100.0 + i as f32 + 0.5).collect();
            let mut rb = ds.rasterband(band).unwrap();
            rb.write((0, 0), (4, 2), &mut gdal::raster::Buffer::new((4, 2), data)).unwrap();
        }
    }
    let mut notes = Notes::default();
    let t = b.read(&dir.join("three.tif"), &ReadOpts { raster: og_engine::io::RasterMode::Image, crs: Some(3006), ..Default::default() }, &mut notes).unwrap();
    let g = t.grid.as_ref().expect("the values go along");
    assert_eq!(g.bands.len(), 2);
    let (c, r, v) = g.cell_at(125.0, 485.0).unwrap();
    assert_eq!((c, r, v), (2, 1, Some(106.5)));
    assert_eq!((g.bands[0][6], g.bands[1][6]), (206.5, 306.5));
    // A raster larger than can be shown is read smaller, over the same ground.
    {
        let opts = gdal::cpl::CslStringList::from_iter(["SPARSE_OK=TRUE", "TILED=YES"]);
        let mut ds = drv.create_with_band_type_with_options::<u8, _>(dir.join("big.tif"), 9000, 8000, 1, &opts).unwrap();
        ds.set_geo_transform(&[0.0, 1.0, 0.0, 8000.0, 0.0, -1.0]).unwrap();
        let mut rb = ds.rasterband(1).unwrap();
        rb.write((0, 0), (9000, 1), &mut gdal::raster::Buffer::new((9000, 1), vec![200u8; 9000])).unwrap();
    }
    let big = b.read(&dir.join("big.tif"), &ReadOpts { raster: og_engine::io::RasterMode::Image, crs: Some(3006), ..Default::default() }, &mut notes).unwrap();
    let img = big.image.as_ref().unwrap();
    assert_eq!((img.width, img.height), (4500, 4000));
    assert_eq!(img.geotransform, [0.0, 2.0, 0.0, 8000.0, 0.0, -2.0]);
    assert!(notes.info.iter().any(|n| n.contains("was read at 4500 × 4000")), "{:?}", notes.info);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_zip_file_packs_what_the_outputs_wrote() {
    let dir = tmp("zip65");
    let b = GdalBackend::new();
    std::fs::write(dir.join("towns.csv"), "name;wkt\nA;POINT (1 1)\nB;POINT (2 2)\n").unwrap();
    std::fs::write(dir.join("README.txt"), "About the data").unwrap();
    std::fs::create_dir_all(dir.join("styles/sub")).unwrap();
    std::fs::write(dir.join("styles/a.qml"), "<qgis/>").unwrap();
    std::fs::write(dir.join("styles/sub/b.sld"), "<sld/>").unwrap();
    // The zip comes first in the file, but runs last.
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "zip", "type": "ZipWriter", "params": {"path": "out/delivery.zip", "extra": "README.txt; styles"}},
        {"id": "some", "type": "ZipWriter", "name": "Only the shapefile", "params": {"path": "out/shp.zip", "outputs": "named", "names": "Shapefile", "places": "flat"}},
        {"id": "in", "type": "Reader", "params": {"path": "towns.csv", "crs": "EPSG:3006"}},
        {"id": "shp", "type": "Writer", "name": "Shapefile", "params": {"path": "out/towns.shp"}},
        {"id": "gpkg", "type": "Writer", "name": "GeoPackage", "params": {"path": "out/gpkg/towns.gpkg"}}],
      "edges": [{"from": ["in", "Output"], "to": ["shp", "Input"]}, {"from": ["in", "Output"], "to": ["gpkg", "Input"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    assert_eq!(r.order.last().map(String::as_str), Some("some"));
    let names = |f: &str| {
        let mut z = zip::ZipArchive::new(std::fs::File::open(dir.join(f)).unwrap()).unwrap();
        let mut n: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        n.sort();
        n
    };
    let all = names("out/delivery.zip");
    for want in ["README.txt", "out/gpkg/towns.gpkg", "out/towns.dbf", "out/towns.shp", "out/towns.shx", "styles/a.qml", "styles/sub/b.sld"] {
        assert!(all.contains(&want.to_string()), "{} should be in {:?}", want, all);
    }
    let shp = names("out/shp.zip");
    assert!(shp.contains(&"towns.shp".to_string()) && !shp.iter().any(|n| n.ends_with(".gpkg")), "{:?}", shp);
    // What was packed can be read again, through the zip.
    let mut notes = Notes::default();
    let back = b.read(&dir.join("out/shp.zip"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(back.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inputs_read_only_within_a_rectangle() {
    let dir = tmp("within66");
    let b = GdalBackend::new();
    std::fs::write(dir.join("towns.csv"), "name;wkt\nA;POINT (100 100)\nB;POINT (500 500)\nC;LINESTRING (0 900, 300 900)\n").unwrap();
    // A GeoPackage of the same, for the filter of GDAL.
    let csv = b.read(&dir.join("towns.csv"), &ReadOpts { crs: Some(3006), ..Default::default() }, &mut Notes::default()).unwrap();
    b.write(&csv, &dir.join("towns.gpkg"), &WriteOpts::default(), &mut Notes::default()).unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "csv", "type": "Reader", "params": {"path": "towns.csv", "crs": "EPSG:3006", "within": "rectangle", "min_x": "0", "min_y": "0", "max_x": "200", "max_y": "950"}},
        {"id": "gpkg", "type": "Reader", "params": {"path": "towns.gpkg", "within": "variables"}},
        {"id": "wgs", "type": "Reader", "params": {"path": "towns.gpkg", "within": "rectangle", "min_x": "11.0", "min_y": "0.0", "max_x": "12.0", "max_y": "0.1", "within_crs": "EPSG:4326"}},
        {"id": "bad", "type": "Reader", "params": {"path": "towns.gpkg", "within": "rectangle", "min_x": "5", "min_y": "0", "max_x": "1", "max_y": "1"}}],
      "edges": [],
      "variables": [{"name": "xmin", "value": "400"}, {"name": "ymin", "value": "400"}, {"name": "xmax", "value": "600"}, {"name": "ymax", "value": "600"}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(col(r.output("csv", "Output").unwrap(), "name"), [text("A"), text("C")], "a line that reaches in counts");
    assert_eq!(col(r.output("gpkg", "Output").unwrap(), "name"), [text("B")]);
    assert!(r.output("wgs", "Output").unwrap().is_empty(), "a rectangle in degrees far from the data finds nothing");
    assert!(r.nodes["bad"].error.as_deref().unwrap().contains("rectangle is empty"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn heights_are_read_kept_set_and_written() {
    let dir = tmp("z66");
    let b = GdalBackend::new();
    // A GeoPackage with heights, made by GDAL itself.
    {
        let drv = gdal::DriverManager::get_driver_by_name("GPKG").unwrap();
        let mut ds = drv.create_vector_only(dir.join("pipes.gpkg")).unwrap();
        let srs = gdal::spatial_ref::SpatialRef::from_epsg(3006).unwrap();
        let mut layer = ds.create_layer(LayerOptions { name: "pipes", srs: Some(&srs), ty: OGRwkbGeometryType::wkbLineString25D, options: None }).unwrap();
        layer.create_defn_fields(&[("name", gdal::vector::OGRFieldType::OFTString)]).unwrap();
        for (wkt, name) in [
            ("LINESTRING Z (400000 6500000 10, 400100 6500000 12.5, 400100 6500100 20)", "main"),
            ("POLYGON Z ((400000 6500000 1, 400010 6500000 2, 400010 6500010 3, 400000 6500000 1))", "area"),
        ] {
            layer.create_feature(gdal::vector::Geometry::from_wkt(wkt).unwrap()).unwrap();
            let _ = name;
        }
    }
    make_geotiff(&dir.join("dem.tif"), Some(3006), (10, 10));
    {
        // The names, set on the features that are there.
        let ds = gdal::Dataset::open_ex(
            dir.join("pipes.gpkg"),
            gdal::DatasetOptions { open_flags: gdal::GdalOpenFlags::GDAL_OF_UPDATE | gdal::GdalOpenFlags::GDAL_OF_VECTOR, ..Default::default() },
        )
        .unwrap();
        ds.execute_sql("UPDATE pipes SET name = CASE WHEN fid = 1 THEN 'main' ELSE 'area' END", None, gdal::vector::sql::Dialect::DEFAULT).unwrap();
    }
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "pipes.gpkg"}},
        {"id": "keep", "type": "AttributeManager", "params": {"attributes": [{"name": "name", "action": "rename", "to": "pipe"}]}},
        {"id": "wgs", "type": "Reprojector", "params": {"crs": "EPSG:4326"}},
        {"id": "out", "type": "Writer", "params": {"path": "out/pipes3d.gpkg"}},
        {"id": "flat", "type": "HeightSetter", "params": {"mode": "remove"}},
        {"id": "up", "type": "HeightSetter", "params": {"mode": "add", "height": "100"}},
        {"id": "dem", "type": "Reader", "params": {"path": "dem.tif", "raster_as": "grid"}},
        {"id": "lines", "type": "ContourGenerator", "params": {"interval": "20", "base": "0"}},
        {"id": "atlevel", "type": "HeightSetter", "params": {"height": "height"}},
        {"id": "drape", "type": "HeightSetter", "params": {"mode": "terrain", "height": "2"}}],
      "edges": [
        {"from": ["in", "Output"], "to": ["keep", "Input"]}, {"from": ["keep", "Output"], "to": ["wgs", "Input"]},
        {"from": ["keep", "Output"], "to": ["out", "Input"]}, {"from": ["in", "Output"], "to": ["flat", "Input"]},
        {"from": ["in", "Output"], "to": ["up", "Input"]},
        {"from": ["dem", "Output"], "to": ["lines", "Heights"]}, {"from": ["lines", "Contours"], "to": ["atlevel", "Input"]},
        {"from": ["in", "Output"], "to": ["drape", "Input"]}, {"from": ["dem", "Output"], "to": ["drape", "Terrain"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    let read = r.output("in", "Output").unwrap();
    assert_eq!(read.features[0].heights(), Some(&[10.0, 12.5, 20.0][..]));
    assert_eq!(read.features[1].heights(), Some(&[1.0, 2.0, 3.0, 1.0][..]));
    // Kept through tools that leave the geometry be, and through reprojection.
    assert_eq!(r.output("keep", "Output").unwrap().features[0].heights(), Some(&[10.0, 12.5, 20.0][..]));
    assert_eq!(r.output("wgs", "Reprojected").unwrap().features[0].heights(), Some(&[10.0, 12.5, 20.0][..]));
    assert!(r.output("flat", "Output").unwrap().features[0].heights().is_none());
    assert_eq!(r.output("up", "Output").unwrap().features[0].heights(), Some(&[110.0, 112.5, 120.0][..]));
    // Written with Z, and read back the same.
    let mut notes = Notes::default();
    let back = b.read(&dir.join("out/pipes3d.gpkg"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(back.features[0].heights(), Some(&[10.0, 12.5, 20.0][..]));
    assert_eq!(back.features[1].heights(), Some(&[1.0, 2.0, 3.0, 1.0][..]));
    // Contour lines at the height in their attribute.
    let c = r.output("atlevel", "Output").unwrap();
    assert!(!c.is_empty());
    let hi = c.field_index("height").expect("the height of the line");
    for f in &c.features {
        let h = f.attrs[hi].as_f64().unwrap();
        assert!(f.heights().unwrap().iter().all(|z| *z == h));
    }
    // Laid on the terrain, 2 m above it: the first point is in the cell with height 100 + its
    // place, between the middles of the cells.
    let d = r.output("drape", "Output").unwrap();
    let z = d.features[0].heights().unwrap();
    assert!(z.iter().all(|v| *v > 100.0 && *v < 200.0), "{:?}", z);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rasters_are_generalized_and_counted_with() {
    let b = GdalBackend::new();
    let dir = tmp("rastercalc");
    std::fs::copy(root().join("examples/terrain.tif"), dir.join("terrain.tif")).unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "dem", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "grid"}},
        {"id": "coarse", "type": "RasterGeneralizer", "params": {"factor": "4"}},
        {"id": "classes", "type": "RasterCalculator", "params": {"expression": "CASE WHEN A > 150 THEN 2 ELSE 1 END"}},
        {"id": "major", "type": "RasterGeneralizer", "params": {"factor": "8", "method": "majority"}},
        {"id": "diff", "type": "RasterCalculator", "params": {"expression": "A - B"}},
        {"id": "high", "type": "RasterCalculator", "params": {"expression": "if(a > 200, a, null)"}},
        {"id": "out", "type": "Writer", "params": {"path": "out/diff.tif"}}],
      "edges": [
        {"from": ["dem", "Output"], "to": ["coarse", "Raster"]},
        {"from": ["dem", "Output"], "to": ["classes", "Rasters"]},
        {"from": ["classes", "Raster"], "to": ["major", "Raster"]},
        {"from": ["dem", "Output"], "to": ["diff", "Rasters"]},
        {"from": ["coarse", "Raster"], "to": ["diff", "Rasters"]},
        {"from": ["dem", "Output"], "to": ["high", "Rasters"]},
        {"from": ["diff", "Raster"], "to": ["out", "Input"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    let dem = r.output("dem", "Output").unwrap().grid.clone().unwrap();
    assert_eq!((dem.width, dem.height), (640, 480));
    // Sixteen cells into one: 160 × 120 cells of 80 m, each the mean of its sixteen.
    let coarse = r.output("coarse", "Raster").unwrap().grid.clone().unwrap();
    assert_eq!((coarse.width, coarse.height), (160, 120));
    assert!((coarse.cell_size().0 - 80.0).abs() < 1e-9);
    let mean: f64 = (0..4).flat_map(|r| (0..4).map(move |c| (c, r))).map(|(c, r)| dem.get(c, r).unwrap() as f64).sum::<f64>() / 16.0;
    assert!((coarse.get(0, 0).unwrap() as f64 - mean).abs() < 1e-3);
    // Classes, and the most common class in each block of 8 × 8.
    let classes = r.output("classes", "Raster").unwrap().grid.clone().unwrap();
    assert!(classes.values.iter().all(|v| *v == 1.0 || *v == 2.0));
    let major = r.output("major", "Raster").unwrap().grid.clone().unwrap();
    assert_eq!((major.width, major.height), (80, 60));
    assert!(major.values.iter().all(|v| *v == 1.0 || *v == 2.0), "majority keeps the classes");
    assert!(major.values.contains(&2.0) && major.values.contains(&1.0));
    // A fine grid less a coarse one, on the cells of the fine one.
    let diff = r.output("diff", "Raster").unwrap().grid.clone().unwrap();
    assert_eq!((diff.width, diff.height), (640, 480));
    assert!((diff.get(0, 0).unwrap() as f64 - (dem.get(0, 0).unwrap() as f64 - mean)).abs() < 1e-3);
    // Only the high ground, empty elsewhere.
    let high = r.output("high", "Raster").unwrap().grid.clone().unwrap();
    assert!(high.values.iter().any(|v| v.is_nan()) && high.values.iter().filter(|v| !v.is_nan()).all(|v| *v > 200.0));
    let written = gdal::Dataset::open(dir.join("out/diff.tif")).unwrap();
    assert_eq!(written.raster_size(), (640, 480));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in for OpenTripPlanner 2: the GraphQL API answers each plan with a duration from
/// the distance between the places and a route as an encoded polyline, after a pause as a
/// real planner takes; the travel time API gives a GeoTIFF. It answers many at once.
fn otp_server(surface: Vec<u8>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = counter.clone();
            let surface = surface.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // The head, then as much body as it says.
                loop {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        while buf.len() < end + 4 + len {
                            let n = stream.read(&mut chunk).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                        }
                        break;
                    }
                }
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let text = String::from_utf8_lossy(&buf).to_string();
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (kind, body): (&str, Vec<u8>) = if path.starts_with("/otp/gtfs/v1") {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let json: serde_json::Value = text.split("\r\n\r\n").nth(1).and_then(|b| serde_json::from_str(b).ok()).unwrap_or_default();
                    let v = &json["variables"];
                    let (a, b) = (
                        (v["fromLat"].as_f64().unwrap_or(0.0), v["fromLon"].as_f64().unwrap_or(0.0)),
                        (v["toLat"].as_f64().unwrap_or(0.0), v["toLon"].as_f64().unwrap_or(0.0)),
                    );
                    let km = ((a.0 - b.0) * 111.0).hypot((a.1 - b.1) * 59.0);
                    let duration = (300.0 + km * 120.0).round() as i64;
                    let answer = serde_json::json!({"data": {"planConnection": {"edges": [{"node": {"duration": duration, "legs": [
                        {"mode": "BUS", "legGeometry": {"points": "_p~iF~ps|U_ulLnnqC_mqNvxq`@"}}]}}]}}});
                    ("application/json", answer.to_string().into_bytes())
                } else if path.starts_with("/otp/traveltime/surface") {
                    ("image/tiff", surface)
                } else {
                    ("text/plain", b"not found".to_vec())
                };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", kind, body.len());
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    (format!("http://127.0.0.1:{}", port), hits)
}

#[test]
fn travel_times_from_an_open_trip_planner() {
    let b = GdalBackend::new();
    let dir = tmp("otp");
    // A travel time surface of 3 × 3 cells of 100 m, in minutes.
    let grid = og_engine::model::Grid {
        width: 3,
        height: 3,
        values: vec![10.0, 12.0, 15.0, 8.0, 0.0, 9.0, 20.0, 14.0, f32::NAN],
        bands: Vec::new(),
        geotransform: [319000.0, 100.0, 0.0, 6400300.0, 0.0, -100.0],
        crs: Some(3006),
    };
    let mut notes = Notes::default();
    b.write(&grid.into_table(), &dir.join("surface.tif"), &WriteOpts::default(), &mut notes).unwrap();
    let surface = std::fs::read(dir.join("surface.tif")).unwrap();
    let (url, hits) = otp_server(surface);
    // Four stops in Borås.
    std::fs::write(dir.join("stops.csv"), "stop;x;y\nResecentrum;374300;6403500\nKnalleland;374900;6404200\nHulta;372800;6405400\nNorrby;373900;6404600\n")
        .unwrap();
    let query = "query($fromLat: CoordinateValue!, $fromLon: CoordinateValue!, $toLat: CoordinateValue!, $toLon: CoordinateValue!) { planConnection(origin: {location: {coordinate: {latitude: $fromLat, longitude: $fromLon}}}, destination: {location: {coordinate: {latitude: $toLat, longitude: $toLon}}}, first: 1) { edges { node { duration legs { mode legGeometry { points } } } } } }";
    let body = format!(
        "'{{\"query\": \"{}\", \"variables\": {{\"fromLat\": ' || from_lat || ', \"fromLon\": ' || from_lon || ', \"toLat\": ' || to_lat || ', \"toLon\": ' || to_lon || '}}}}'",
        query
    );
    let model = serde_json::json!({
        "format": "ographis/1",
        "nodes": [
            {"id": "stops", "type": "Reader", "params": {"path": "stops.csv", "x": "x", "y": "y", "crs": "EPSG:3006"}},
            {"id": "pairs", "type": "PairMaker", "params": {}},
            {"id": "ask", "type": "HttpCaller", "params": {"url": format!("'{}/otp/gtfs/v1'", url), "method": "POST", "body": body, "parallel": "6", "cache": "disk"}},
            {"id": "take", "type": "JsonExtractor", "params": {"attribute": "_response", "paths": [["data.planConnection.edges[0].node.duration", "seconds"], ["data.planConnection.edges[0].node.legs[0].legGeometry.points", "route"]]}},
            {"id": "minutes", "type": "AttributeCreator", "params": {"name": "minutes", "expression": "round(seconds / 60, 1)"}},
            {"id": "lines", "type": "GeometryFromText", "params": {"attribute": "route", "kind": "polyline"}},
            {"id": "stats", "type": "StatisticsCalculator", "params": {"attributes": "minutes", "group_by": "stop", "percentiles": "10, 90"}},
            {"id": "surface", "type": "HttpCaller", "params": {"url": format!("{}/otp/traveltime/surface?location=57.72,12.94&time=2026-10-21T08:00:00%2B02:00", url), "answer_as": "file", "folder": "answers"}},
            {"id": "read", "type": "AttributeFileReader", "params": {}}
        ],
        "edges": [
            {"from": ["stops", "Output"], "to": ["pairs", "From"]},
            {"from": ["stops", "Output"], "to": ["pairs", "To"]},
            {"from": ["pairs", "Pairs"], "to": ["ask", "Input"]},
            {"from": ["ask", "Output"], "to": ["take", "Input"]},
            {"from": ["take", "Output"], "to": ["minutes", "Input"]},
            {"from": ["minutes", "Output"], "to": ["lines", "Input"]},
            {"from": ["minutes", "Output"], "to": ["stats", "Input"]},
            {"from": ["surface", "Output"], "to": ["read", "Input"]}
        ]
    });
    let ws = Workspace::from_json(&model.to_string()).unwrap();
    let started = std::time::Instant::now();
    let r = run(&ws, Some(&b), &dir);
    let took = started.elapsed();
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    // Four stops make twelve pairs, each stop not with itself, with WGS 84 coordinates.
    let pairs = r.output("pairs", "Pairs").unwrap();
    assert_eq!(pairs.len(), 12);
    let lat = col(pairs, "from_lat")[0].as_f64().unwrap();
    assert!((57.7..57.8).contains(&lat), "{}", lat);
    // Twelve requests, six at a time: about two pauses, not twelve.
    assert!(took < std::time::Duration::from_millis(1500), "{:?}", took);
    let minutes = r.output("minutes", "Output").unwrap();
    assert_eq!(minutes.len(), 12);
    assert!(col(minutes, "minutes").iter().all(|m| m.as_f64().is_some_and(|m| m > 5.0 && m < 30.0)));
    // The route as a line in WGS 84.
    let lines = r.output("lines", "Output").unwrap();
    assert_eq!(lines.crs, Some(4326));
    assert!(matches!(lines.features[0].geom, Some(geo::Geometry::LineString(_))));
    let stats = r.output("stats", "Summary").unwrap();
    assert!(stats.field_index("minutes_p10").is_some() && stats.field_index("minutes_p90").is_some());
    // The surface as a file, read back as a grid.
    let grid = r.output("read", "Output").unwrap().grid.clone().expect("a grid");
    assert_eq!((grid.width, grid.height, grid.get(1, 1)), (3, 3, Some(0.0)));
    assert_eq!(grid.get(2, 2), None);
    // A second run takes the answers from the cache, and the file from before is used.
    let before = hits.load(std::sync::atomic::Ordering::SeqCst);
    let r2 = run(&ws, Some(&b), &dir);
    assert_eq!(r2.errors, 0);
    let asked = hits.load(std::sync::atomic::Ordering::SeqCst) - before;
    assert_eq!(asked, 1, "only the surface, which is not kept, is asked for again");
    assert!(r2.log.iter().any(|l| l.msg.contains("12 answers were taken from http-cache")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server that gives parts of files, as cloud storage does: HEAD with the size, and GET
/// with a Range of bytes. It counts the bytes it sends.
fn range_server(files: Vec<(&'static str, Vec<u8>)>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = sent.clone();
    let files = std::sync::Arc::new(files);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (counter, files) = (counter.clone(), files.clone());
            std::thread::spawn(move || loop {
                // Requests one after another on the same connection, as curl sends them.
                let mut buf = Vec::new();
                let mut byte = [0u8; 1];
                while !buf.ends_with(b"\r\n\r\n") {
                    match stream.read(&mut byte) {
                        Ok(1) => buf.push(byte[0]),
                        _ => return,
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let mut words = head.split_whitespace();
                let (method, path) = (words.next().unwrap_or("").to_string(), words.next().unwrap_or("/").split('?').next().unwrap_or("/").to_string());
                let Some((_, body)) = files.iter().find(|f| f.0 == path) else {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                let range = head.lines().find_map(|l| l.to_lowercase().strip_prefix("range: bytes=").map(|r| r.trim().to_string()));
                let (from, to, status) = match range {
                    Some(r) => {
                        let (a, b) = r.split_once('-').unwrap_or((&r, ""));
                        let a: usize = a.parse().unwrap_or(0);
                        let b: usize = b.parse().unwrap_or(body.len() - 1).min(body.len() - 1);
                        (a.min(body.len()), b + 1, "206 Partial Content")
                    }
                    None => (0, body.len(), "200 OK"),
                };
                let part = &body[from..to.max(from)];
                let mut answer =
                    format!("HTTP/1.1 {}\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n", status, part.len());
                if status.starts_with("206") {
                    answer += &format!("Content-Range: bytes {}-{}/{}\r\n", from, to.max(from + 1) - 1, body.len());
                }
                answer += "\r\n";
                let _ = stream.write_all(answer.as_bytes());
                if method != "HEAD" {
                    let _ = stream.write_all(part);
                    counter.fetch_add(part.len(), std::sync::atomic::Ordering::SeqCst);
                }
            });
        }
    });
    (format!("http://127.0.0.1:{}", port), sent)
}

#[test]
fn cloud_optimized_geotiff_and_copc_are_read_where_they_are() {
    let b = GdalBackend::new();
    let dir = tmp("cloud");
    // The made-up terrain as a Cloud Optimized GeoTIFF, with tiles of 256 and overviews.
    let src = gdal::Dataset::open(root().join("examples/terrain.tif")).unwrap();
    let cog = gdal::DriverManager::get_driver_by_name("COG").unwrap();
    let opts: gdal::raster::RasterCreationOptions = ["BLOCKSIZE=128", "OVERVIEWS=AUTO", "COMPRESS=DEFLATE"].iter().map(|s| s.to_string()).collect();
    src.create_copy(&cog, dir.join("terrain_cog.tif"), &opts).unwrap();
    let tif = std::fs::read(dir.join("terrain_cog.tif")).unwrap();
    let copc = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/autzen.copc.laz")).unwrap();
    std::fs::copy(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/autzen.copc.laz"), dir.join("autzen.copc.laz")).unwrap();
    let size = tif.len();
    let (url, sent) = range_server(vec![("/terrain_cog.tif", tif), ("/autzen.copc.laz", copc)]);
    let model = serde_json::json!({
        "nodes": [
            {"id": "local", "type": "Reader", "params": {"path": "terrain_cog.tif", "raster_as": "grid"}},
            {"id": "part", "type": "Reader", "params": {"path": format!("{}/terrain_cog.tif", url), "raster_as": "grid",
                "within": "rectangle", "min_x": "418000", "min_y": "6474000", "max_x": "422000", "max_y": "6477000", "cell_size": "80"}},
            {"id": "points", "type": "Reader", "params": {"path": "autzen.copc.laz", "within": "rectangle", "min_x": "636000", "min_y": "849000", "max_x": "638000", "max_y": "852000"}},
            {"id": "web_points", "type": "Reader", "params": {"path": format!("{}/autzen.copc.laz", url), "within": "rectangle", "min_x": "636000", "min_y": "849000", "max_x": "638000", "max_y": "852000"}}
        ],
        "edges": []
    });
    let ws = Workspace::from_json(&model.to_string()).unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    // Only the rectangle, 4 × 3 km, at 80 m: 50 × 37 or 38 cells, from the overviews.
    let part = r.output("part", "Output").unwrap().grid.clone().unwrap();
    assert_eq!(part.width, 50);
    assert!((37..=38).contains(&part.height), "{}", part.height);
    let local = r.output("local", "Output").unwrap().grid.clone().unwrap();
    let mean = |g: &og_engine::model::Grid| {
        g.values.iter().filter(|v| !v.is_nan()).map(|v| *v as f64).sum::<f64>() / g.values.iter().filter(|v| !v.is_nan()).count() as f64
    };
    let (x, y) = (420000.0, 6475500.0);
    let near = local.at(x, y).unwrap();
    assert!((part.at(x, y).unwrap() - near).abs() < 15.0, "{} {}", part.at(x, y).unwrap(), near);
    assert!(mean(&part).is_finite());
    let fetched = sent.load(std::sync::atomic::Ordering::SeqCst);
    assert!(fetched < size, "only part of the GeoTIFF was fetched: {} of {}", fetched, size);
    // The points in the rectangle, the same from the disk and from the web.
    let a = r.output("points", "Output").unwrap();
    let w = r.output("web_points", "Output").unwrap();
    assert!(!a.is_empty() && a.len() < 107, "{}", a.len());
    assert_eq!(a.len(), w.len());
    assert_eq!(a.crs, Some(2992));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_geopackage_gets_the_primary_key_asked_for() {
    use og_engine::io::PrimaryKey;
    let dir = tmp("gpkg-key");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let fid_of = |path: &std::path::Path, layer: &str| -> (String, Vec<u64>) {
        let ds = gdal::Dataset::open(path).unwrap();
        let mut l = ds.layer_by_name(layer).unwrap();
        // SAFETY: the layer is valid, and GDAL owns the name.
        let name = unsafe { std::ffi::CStr::from_ptr(gdal_sys::OGR_L_GetFIDColumn(l.c_layer())).to_string_lossy().into_owned() };
        (name, l.features().filter_map(|f| f.fid()).collect())
    };
    // A new column of numbers with a name of its own.
    let path = dir.join("keys.gpkg");
    let opts = |layer: &str, key: PrimaryKey| WriteOpts { layer: Some(layer.into()), key: Some(key), ..Default::default() };
    b.write(&sample(), &path, &opts("numbered", PrimaryKey::New("row_id".into())), &mut notes).unwrap();
    assert_eq!(fid_of(&path, "numbered"), ("row_id".to_string(), vec![1, 2]));
    // An attribute of whole numbers is the key itself, and is read back as an attribute.
    let mut t = sample();
    t.fields.push(Field::new("object_id", FieldType::Int));
    t.features[0].attrs.push(Value::Int(1001));
    t.features[1].attrs.push(Value::Int(2002));
    b.write(&t, &path, &opts("by_id", PrimaryKey::Attribute("object_id".into())), &mut notes).unwrap();
    assert_eq!(fid_of(&path, "by_id"), ("object_id".to_string(), vec![1001, 2002]));
    let back = b.read(&path, &ReadOpts { layer: Some("by_id".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(col(&back, "object_id"), [Value::Int(1001), Value::Int(2002)]);
    assert_eq!(col(&back, "name"), [text("Upplands Väsby"), text("Göteborg")]);
    // A text is kept apart by an index: a second row with the same code is refused.
    b.write(&sample(), &path, &opts("by_code", PrimaryKey::Attribute("muni_code".into())), &mut notes).unwrap();
    assert!(notes.info.iter().any(|i| i.contains("muni_code") && i.contains("index")), "{:?}", notes.info);
    {
        let ds = gdal::Dataset::open_ex(
            &path,
            gdal::DatasetOptions { open_flags: gdal::GdalOpenFlags::GDAL_OF_UPDATE | gdal::GdalOpenFlags::GDAL_OF_VECTOR, ..Default::default() },
        )
        .unwrap();
        let dup = ds.execute_sql("INSERT INTO by_code (muni_code) VALUES ('0114')", None, gdal::vector::sql::Dialect::DEFAULT).map(|_| ());
        let rows = {
            let mut rs = ds.execute_sql("SELECT COUNT(*) FROM by_code", None, gdal::vector::sql::Dialect::DEFAULT).unwrap().unwrap();
            let n = rs.features().next().unwrap().field_as_integer(0).unwrap();
            n
        };
        assert!(dup.is_err() || rows == Some(2), "the same code twice: {:?}", rows);
    }
    // Values that repeat, or are missing, cannot be a key.
    let mut twice = sample();
    twice.features[1].attrs[0] = text("0114");
    let e = b.write(&twice, &path, &opts("bad", PrimaryKey::Attribute("muni_code".into())), &mut notes).unwrap_err();
    assert!(e.0.contains("more than one row"), "{}", e.0);
    let e = b.write(&sample(), &path, &opts("bad", PrimaryKey::Attribute("invånare_totalt".into())), &mut notes).unwrap_err();
    assert!(e.0.contains("Row 2 has no"), "{}", e.0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Needs a PostgreSQL with PostGIS: set OG_TEST_PG to a connection, such as
/// "PG:host=/var/tmp/ogpg port=5433 dbname=og user=postgres". Skipped without it.
#[test]
fn a_postgis_table_gets_the_primary_key_asked_for() {
    use og_engine::io::{DbWrite, PrimaryKey};
    let Ok(conn) = std::env::var("OG_TEST_PG") else { return };
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let key_of = |table: &str| -> Vec<String> {
        let ds = gdal::Dataset::open_ex(&conn, gdal::DatasetOptions { open_flags: gdal::GdalOpenFlags::GDAL_OF_VECTOR, ..Default::default() }).unwrap();
        let sql = format!(
            "SELECT a.attname FROM pg_index i JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) WHERE i.indrelid = 'public.{}'::regclass AND i.indisprimary",
            table
        );
        let mut rs = ds.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).unwrap().unwrap();
        let out = rs.features().map(|f| f.field_as_string(0).unwrap().unwrap()).collect();
        out
    };
    let opts = |table: &str, key: Option<PrimaryKey>| DbWrite {
        schema: "public".into(),
        table: table.into(),
        geometry: "auto".into(),
        geom_column: "geom".into(),
        append: false,
        key,
        empty_first: false,
        commit_every: None,
    };
    b.write_postgis(&sample(), &conn, &opts("og_key_default", None), &mut notes).unwrap();
    assert_eq!(key_of("og_key_default"), ["ogc_fid"]);
    b.write_postgis(&sample(), &conn, &opts("og_key_new", Some(PrimaryKey::New("Row ID".into()))), &mut notes).unwrap();
    assert_eq!(key_of("og_key_new"), ["row_id"]);
    let mut t = sample();
    t.fields.push(Field::new("object_id", FieldType::Int));
    t.features[0].attrs.push(Value::Int(1001));
    t.features[1].attrs.push(Value::Int(2002));
    b.write_postgis(&t, &conn, &opts("og_key_int", Some(PrimaryKey::Attribute("object_id".into()))), &mut notes).unwrap();
    assert_eq!(key_of("og_key_int"), ["object_id"]);
    let back = b.read(std::path::Path::new(&conn), &ReadOpts { layer: Some("og_key_int".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(col(&back, "object_id"), [Value::Int(1001), Value::Int(2002)]);
    b.write_postgis(&sample(), &conn, &opts("og_key_text", Some(PrimaryKey::Attribute("muni_code".into()))), &mut notes).unwrap();
    assert_eq!(key_of("og_key_text"), ["muni_code"]);
    let back = b.read(std::path::Path::new(&conn), &ReadOpts { layer: Some("og_key_text".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(col(&back, "muni_code"), [text("0114"), text("1480")]);
    assert!(back.field_index("ogc_fid").is_none(), "{:?}", back.field_names());
}

/// A table emptied and filled again in one transaction keeps the view on it; and a large
/// write is committed in parts. Needs OG_TEST_PG, as above.
#[test]
fn a_postgis_table_is_emptied_and_filled_or_written_in_parts() {
    use og_engine::io::DbWrite;
    let Ok(conn) = std::env::var("OG_TEST_PG") else { return };
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let sql = |q: &str| -> Option<String> {
        let ds = gdal::Dataset::open_ex(
            &conn,
            gdal::DatasetOptions { open_flags: gdal::GdalOpenFlags::GDAL_OF_VECTOR | gdal::GdalOpenFlags::GDAL_OF_UPDATE, ..Default::default() },
        )
        .unwrap();
        let mut rs = ds.execute_sql(q, None, gdal::vector::sql::Dialect::DEFAULT).unwrap()?;
        let v = rs.features().next().and_then(|f| f.field_as_string(0).ok().flatten());
        v
    };
    let opts = |empty_first: bool, commit_every: Option<usize>| DbWrite {
        schema: "public".into(),
        table: "og_tx".into(),
        geometry: "auto".into(),
        geom_column: "geom".into(),
        append: false,
        key: None,
        empty_first,
        commit_every,
    };
    let _ = sql("DROP VIEW IF EXISTS og_tx_view");
    b.write_postgis(&sample(), &conn, &opts(false, None), &mut notes).unwrap();
    sql("CREATE VIEW og_tx_view AS SELECT * FROM og_tx");
    let mut more = sample();
    let first = more.features[0].clone();
    more.features = (0..25).map(|_| first.clone()).collect();
    b.write_postgis(&more, &conn, &opts(true, Some(10)), &mut notes).unwrap();
    assert_eq!(sql("SELECT count(*) FROM og_tx_view").as_deref(), Some("25"), "the view is there, on the new rows");
    assert!(notes.info.iter().any(|n| n.contains("committed in 3 parts")), "{:?}", notes.info);
    let _ = sql("DROP VIEW IF EXISTS og_tx_view");
}

#[test]
fn a_cad_drawing_is_read_with_layers_colours_text_and_blocks() {
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let path = root().join("examples/formats/plan.dxf");
    // Without a coordinate system it is read all the same, with a warning that says which it
    // looks like.
    let bare = b.read(&path, &ReadOpts { dxf_blocks_as_points: true, dxf_areas: true, ..Default::default() }, &mut notes).unwrap();
    assert_eq!((bare.crs, bare.len() > 20), (None, true));
    let warned = format!("{:?}", notes);
    assert!(warned.contains("DXF does not say") && warned.contains("EPSG:3006"), "{}", warned);
    let mut notes = Notes::default();
    let opts = ReadOpts { crs: Some(3006), dxf_blocks_as_points: true, dxf_areas: true, ..Default::default() };
    let t = b.read(&path, &opts, &mut notes).unwrap();
    assert_eq!(t.crs, Some(3006));
    for f in ["layer", "entity", "colour", "handle", "text", "text_height", "block", "ID", "DJUP"] {
        assert!(t.field_index(f).is_some(), "{} among {:?}", f, t.field_names());
    }
    let rows = |layer: &str| t.features.iter().filter(|f| f.attrs[0] == text(layer)).count();
    assert_eq!((rows("BYGGNAD"), rows("VA_BRUNN"), rows("TEXT"), rows("VAG")), (12, 6, 12, 2));
    // The outlines of the buildings are areas, in the colour of their layer.
    let i = |n: &str| t.field_index(n).unwrap();
    let building = t.features.iter().find(|f| f.attrs[0] == text("BYGGNAD")).unwrap();
    assert!(matches!(building.geom, Some(Geometry::Polygon(_))));
    assert_eq!((building.attrs[i("entity")].clone(), building.attrs[i("colour")].clone()), (text("LWPOLYLINE"), text("#ff0000")));
    // A well: a point with the values of its attributes.
    let well = t.features.iter().find(|f| f.attrs[0] == text("VA_BRUNN")).unwrap();
    assert!(matches!(well.geom, Some(Geometry::Point(_))));
    assert_eq!((well.attrs[i("block")].clone(), well.attrs[i("ID")].clone(), well.attrs[i("DJUP")].clone()), (text("BRUNN"), text("B-01"), text("1.8")));
    let label = t.features.iter().find(|f| f.attrs[0] == text("TEXT")).unwrap();
    assert_eq!(label.attrs[i("text_height")], Value::Real(2.0));
    // What the header says.
    assert!(notes.info.iter().any(|n| n.contains("AutoCAD 2018") && n.contains("drawn in metres")), "{:?}", notes.info);
    // Blocks drawn out, and closed polylines as lines.
    let drawn = b.read(&path, &ReadOpts { crs: Some(3006), ..Default::default() }, &mut notes).unwrap();
    assert!(drawn.field_index("ID").is_none() && drawn.len() > t.len(), "the wells are their circles and texts");
    assert!(drawn.features.iter().all(|f| !matches!(f.geom, Some(Geometry::Polygon(_)))));
}

#[test]
fn flatgeobuf_and_spatialite() {
    let dir = tmp("fgb");
    let b = GdalBackend::new();
    let mut notes = Notes::default();
    let path = dir.join("areas.fgb");
    b.write(&sample(), &path, &WriteOpts::default(), &mut notes).unwrap();
    let back = b.read(&path, &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!(back.crs, Some(3006));
    assert_eq!(col(&back, "name"), [text("Upplands Väsby"), text("Göteborg")]);
    assert!(back.features.iter().all(|f| matches!(f.geom, Some(Geometry::MultiPolygon(_)))), "one type: the single area as a multi");
    // Added to: read and written again with the new ones after.
    b.write(&sample(), &path, &WriteOpts { append: true, ..Default::default() }, &mut notes).unwrap();
    assert_eq!(b.read(&path, &ReadOpts::default(), &mut notes).unwrap().len(), 4);
    // SpatiaLite, made by GDAL, read with its layers.
    let lite = dir.join("data.sqlite");
    {
        let drv = gdal::DriverManager::get_driver_by_name("SQLite").unwrap();
        let mut opts = gdal::cpl::CslStringList::new();
        opts.set_name_value("SPATIALITE", "YES").unwrap();
        let mut ds = drv.create_with_band_type_with_options::<u8, _>(&lite, 0, 0, 0, &opts).unwrap();
        let srs = gdal::spatial_ref::SpatialRef::from_epsg(3006).unwrap();
        let l = ds.create_layer(LayerOptions { name: "stops", srs: Some(&srs), ty: OGRwkbGeometryType::wkbPoint, options: None }).unwrap();
        gdal::vector::FieldDefn::new("name", gdal::vector::OGRFieldType::OFTString).unwrap().add_to_layer(&l).unwrap();
        let mut f = gdal::vector::Feature::new(l.defn()).unwrap();
        f.set_field_string(0, "Brunnsparken").unwrap();
        f.set_geometry(gdal::vector::Geometry::from_wkt("POINT (319758 6400326)").unwrap()).unwrap();
        f.create(&l).unwrap();
    }
    let t = b.read(&lite, &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!((t.crs, t.len()), (Some(3006), 1));
    assert_eq!(col(&t, "name"), [text("Brunnsparken")]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The raster and point cloud tools of 0.89 on the example terrain model: slope, zonal
/// statistics, visibility, classes made into areas, and points laid on the terrain written as
/// a point cloud with its coordinate system and read again.
#[test]
fn rasters_are_measured_and_point_clouds_written() {
    let b = GdalBackend::new();
    let dir = tmp("raster089");
    std::fs::copy(root().join("examples/terrain.tif"), dir.join("terrain.tif")).unwrap();
    std::fs::write(dir.join("places.csv"), "name,x,y\ntower,418400,6474800\nhill,414000,6472000\nfar,1,1\n").unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "dem", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "grid"}},
        {"id": "outline", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "extent"}},
        {"id": "places", "type": "Reader", "params": {"path": "places.csv", "x": "x", "y": "y", "crs": "EPSG:3006"}},
        {"id": "slope", "type": "TerrainAnalyzer", "params": {"measure": "slope"}},
        {"id": "zones", "type": "ZonalStatistics", "params": {"stats": "count, mean, max"}},
        {"id": "sample", "type": "RasterSampler", "params": {"name": "height", "method": "bilinear"}},
        {"id": "view", "type": "Viewshed", "params": {"radius": "3000", "eye": "20"}},
        {"id": "bands", "type": "RasterReclassifier", "params": {"ranges": [["", "100", "1"], ["100", "", "2"]]}},
        {"id": "areas", "type": "RasterPolygonizer", "params": {"merge": "yes", "name": "band"}},
        {"id": "on_ground", "type": "HeightSetter", "params": {"mode": "terrain", "height": "0"}},
        {"id": "laz", "type": "Writer", "params": {"path": "out/places.laz"}},
        {"id": "distance", "type": "DistanceRaster", "params": {"cell": "1000", "reach": "500"}}],
      "edges": [
        {"from": ["dem", "Output"], "to": ["slope", "Heights"]},
        {"from": ["outline", "Output"], "to": ["zones", "Zones"]},
        {"from": ["dem", "Output"], "to": ["zones", "Raster"]},
        {"from": ["places", "Output"], "to": ["sample", "Features"]},
        {"from": ["dem", "Output"], "to": ["sample", "Raster"]},
        {"from": ["dem", "Output"], "to": ["view", "Heights"]},
        {"from": ["places", "Output"], "to": ["view", "Places"]},
        {"from": ["dem", "Output"], "to": ["bands", "Raster"]},
        {"from": ["bands", "Raster"], "to": ["areas", "Raster"]},
        {"from": ["places", "Output"], "to": ["on_ground", "Input"]},
        {"from": ["dem", "Output"], "to": ["on_ground", "Terrain"]},
        {"from": ["on_ground", "Output"], "to": ["laz", "Input"]},
        {"from": ["places", "Output"], "to": ["distance", "Features"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    let slope = r.output("slope", "Raster").unwrap().grid.clone().unwrap();
    let (lo, hi) = slope.range().unwrap();
    assert!(lo >= 0.0 && hi < 90.0 && hi > 1.0, "{} {}", lo, hi);
    let zones = r.output("zones", "Output").unwrap();
    assert_eq!(col(zones, "count"), [Value::Int(640 * 480)]);
    let dem = r.output("dem", "Output").unwrap().grid.clone().unwrap();
    assert_eq!(col(zones, "max")[0], Value::Real(dem.range().unwrap().1 as f64));
    let sample = r.output("sample", "Output").unwrap();
    let h = col(sample, "height");
    assert!(h[0].as_f64().is_some() && h[2] == Value::Null, "{:?}", h);
    let view = r.output("view", "Raster").unwrap().grid.clone().unwrap();
    assert!(view.values.contains(&2.0), "some cells are seen from both places");
    assert!(view.values.contains(&0.0), "and some from neither");
    let areas = r.output("areas", "Areas").unwrap();
    assert_eq!(areas.len(), 2, "one feature for each band");
    assert_eq!(areas.crs, Some(3006));
    // The points on the ground, as a point cloud, with their heights and system.
    let r2 = run(
        &Workspace::from_json(r#"{"nodes": [{"id": "back", "type": "Reader", "params": {"path": "out/places.laz"}}], "edges": []}"#).unwrap(),
        Some(&b),
        &dir,
    );
    let back = r2.output("back", "Output").unwrap();
    assert_eq!((back.len(), back.crs), (3, Some(3006)), "{:?}", r2.log);
    let z = back.features[0].heights().unwrap()[0];
    assert!((z - h[0].as_f64().unwrap()).abs() < 0.5, "{} {:?}", z, h[0]);
    let d = r.output("distance", "Raster").unwrap().grid.clone().unwrap();
    assert_eq!(d.cell_size().0, 1000.0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The raster tools of 0.90 on the example terrain: catchments and streams, a raster
/// reprojected to cells of a size, and heights interpolated from points laid on it.
#[test]
fn catchments_streams_and_interpolation() {
    let b = GdalBackend::new();
    let dir = tmp("raster090");
    std::fs::copy(root().join("examples/terrain.tif"), dir.join("terrain.tif")).unwrap();
    std::fs::write(dir.join("outlet.csv"), "name,x,y\nmouth,424790,6471000\n").unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "dem", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "grid"}},
        {"id": "outlet", "type": "Reader", "params": {"path": "outlet.csv", "x": "x", "y": "y", "crs": "EPSG:3006"}},
        {"id": "all", "type": "Watershed", "params": {"smallest": "2"}},
        {"id": "one", "type": "Watershed", "params": {"snap": "300"}},
        {"id": "wgs", "type": "Reprojector", "params": {"crs": "EPSG:3021", "cell": "100"}},
        {"id": "cells", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "points", "cell_size": "200"}},
        {"id": "on_ground", "type": "HeightSetter", "params": {"mode": "terrain", "height": "0"}},
        {"id": "tin", "type": "PointInterpolator", "params": {"cell": "100"}}],
      "edges": [
        {"from": ["dem", "Output"], "to": ["all", "Heights"]},
        {"from": ["dem", "Output"], "to": ["one", "Heights"]},
        {"from": ["outlet", "Output"], "to": ["one", "Outlets"]},
        {"from": ["dem", "Output"], "to": ["wgs", "Input"]},
        {"from": ["cells", "Output"], "to": ["on_ground", "Input"]},
        {"from": ["dem", "Output"], "to": ["on_ground", "Terrain"]},
        {"from": ["on_ground", "Output"], "to": ["tin", "Points"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    let all = r.output("all", "Catchments").unwrap();
    assert!(all.len() >= 2, "{}", all.len());
    let total: f64 = col(all, "area_km2").iter().filter_map(|v| v.as_f64()).sum();
    assert!(total > 50.0 && total <= 640.0 * 480.0 * 400.0 / 1e6 + 0.01, "{}", total);
    assert!(!r.output("all", "Streams").unwrap().is_empty());
    let one = r.output("one", "Catchments").unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(col(one, "name"), [text("mouth")]);
    let flow = r.output("all", "Flow").unwrap().grid.clone().unwrap();
    assert_eq!((flow.width, flow.height), (640, 480));
    let g = r.output("wgs", "Reprojected").unwrap().grid.clone().unwrap();
    assert_eq!((g.crs, g.cell_size().0), (Some(3021), 100.0));
    let tin = r.output("tin", "Raster").unwrap().grid.clone().unwrap();
    let dem = r.output("dem", "Output").unwrap().grid.clone().unwrap();
    let (x, y) = (418400.0, 6474800.0);
    assert!((tin.at(x, y).unwrap() - dem.at(x, y).unwrap()).abs() < 15.0, "{:?} {:?}", tin.at(x, y), dem.at(x, y));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A DWG drawing is read as the same drawing in DXF is: the example plan written as DWG by
/// acadrust, and read again through it.
#[test]
fn a_dwg_drawing_is_read_as_its_dxf() {
    let b = GdalBackend::new();
    let dir = tmp("dwg");
    let dxf = root().join("examples/formats/plan.dxf");
    let dwg = dir.join("plan.dwg");
    og_engine::dwg::dxf_to_dwg(&dxf, &dwg).unwrap();
    let opts = ReadOpts { crs: Some(3006), dxf_blocks_as_points: true, dxf_areas: true, ..Default::default() };
    let mut notes = Notes::default();
    let a = b.read(&dxf, &opts, &mut notes).unwrap();
    let mut notes = Notes::default();
    let d = b.read(&dwg, &opts, &mut notes).unwrap();
    println!("{:?}", notes.info);
    assert!(notes.info.iter().any(|n| n.contains("DWG drawing")), "{:?}", notes.info);
    assert_eq!(d.len(), a.len());
    assert_eq!(d.field_names(), a.field_names());
    // The same layers, entities, blocks and their attributes.
    for name in ["layer", "entity", "block", "ID", "DJUP", "text", "colour", "linetype"] {
        if a.field_index(name).is_some() {
            let mut x = col(&a, name);
            let mut y = col(&d, name);
            x.sort_by_key(|v| v.to_text());
            y.sort_by_key(|v| v.to_text());
            assert_eq!(x, y, "{}", name);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A GPX file gives its tracks, without the empty columns of GPX.
#[test]
fn a_gpx_file_gives_its_tracks() {
    let b = GdalBackend::new();
    let dir = tmp("gpx");
    std::fs::write(
        dir.join("walk.gpx"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test" xmlns="http://www.topografix.com/GPX/1/1">
  <wpt lat="57.7089" lon="11.9746"><name>Start</name></wpt>
  <trk><name>Walk</name><trkseg>
    <trkpt lat="57.7089" lon="11.9746"><ele>12</ele></trkpt>
    <trkpt lat="57.7095" lon="11.9760"><ele>14</ele></trkpt>
  </trkseg></trk>
</gpx>"#,
    )
    .unwrap();
    let mut notes = Notes::default();
    let t = b.read(&dir.join("walk.gpx"), &ReadOpts::default(), &mut notes).unwrap();
    assert_eq!((t.len(), t.crs), (1, Some(4326)));
    assert_eq!(t.field_names(), ["name"]);
    let pts = b.read(&dir.join("walk.gpx"), &ReadOpts { layer: Some("track_points".into()), ..Default::default() }, &mut notes).unwrap();
    assert_eq!(pts.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
