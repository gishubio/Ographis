//! Tests of the tools added in 0.86: joins, geometry as text, duplicates by geometry,
//! raised points, the terrain of a diorama, charts as web pages and slides.

use super::*;
use crate::model::{Feature, Field, FieldType, Table, Value};

fn table(fields: &[&str], rows: Vec<Vec<Value>>) -> Table {
    Table {
        fields: fields.iter().map(|f| Field::new(*f, FieldType::Text)).collect(),
        crs: None,
        features: rows.into_iter().map(|attrs| Feature { attrs, geom: None, z: None }).collect(),
        ..Default::default()
    }
}

fn t(s: &str) -> Value {
    Value::Text(s.into())
}

fn port<'a>(out: &'a Outputs, name: &str) -> &'a Table {
    &out.iter().find(|o| o.0 == name).unwrap().1
}

#[test]
fn joins_are_inner_left_or_full_with_the_first_or_every_match() {
    let req = table(&["code", "name"], vec![vec![t("1"), t("Ale")], vec![t("2"), t("Borås")], vec![t("3"), t("Dals-Ed")]]);
    let sup = table(&["kod", "year", "name"], vec![vec![t("1"), t("2020"), t("x")], vec![t("1"), t("2021"), t("y")], vec![t("9"), t("2020"), t("z")]]);
    let run = |extra: &[(&str, &str)]| {
        let mut params = vec![("requestor_key", "code"), ("supplier_key", "kod")];
        params.extend_from_slice(extra);
        run_for_test("FeatureMerger", &params, vec![("Requestor", req.clone()), ("Supplier", sup.clone())]).unwrap().0
    };
    let inner = run(&[]);
    assert_eq!(port(&inner, "Merged").features.len(), 1);
    assert_eq!(port(&inner, "UnmergedRequestor").features.len(), 2);
    let every = run(&[("matches", "all")]);
    let m = port(&every, "Merged");
    assert_eq!(m.features.len(), 2);
    assert_eq!(m.features[1].attrs[m.field_index("year").unwrap()], t("2021"));
    // Requestor's name wins, unless Supplier's attributes get a prefix.
    assert_eq!(m.field_index("name"), Some(1));
    assert!(m.field_index("kod").is_some());
    let prefixed = run(&[("prefix", "s_")]);
    let m = port(&prefixed, "Merged");
    assert_eq!(m.features[0].attrs[m.field_index("s_name").unwrap()], t("x"));
    let left = run(&[("join", "left"), ("matches", "all")]);
    let m = port(&left, "Merged");
    assert_eq!(m.features.len(), 4);
    assert_eq!(m.features[2].attrs[1], t("Borås"));
    assert_eq!(m.features[2].attrs[m.field_index("year").unwrap()], Value::Null);
    let full = run(&[("join", "full")]);
    let m = port(&full, "Merged");
    assert_eq!(m.features.len(), 4);
    let last = &m.features[3];
    assert_eq!((last.attrs[0].clone(), last.attrs[m.field_index("year").unwrap()].clone()), (t("9"), t("2020")));
    // The second with key 1 was not taken, but its key matched: it is not a row of its own.
    assert_eq!(port(&full, "UnusedSupplier").features.len(), 2);
}

fn points(rows: Vec<(&str, f64, f64, Option<f64>)>) -> Table {
    Table {
        fields: vec![Field::new("name", FieldType::Text)],
        crs: Some(3006),
        features: rows
            .into_iter()
            .map(|(n, x, y, z)| Feature {
                attrs: vec![t(n)],
                geom: Some(geo::Geometry::Point(geo::Point::new(x, y))),
                z: z.map(|z| std::sync::Arc::from(vec![z])),
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn geometry_is_written_as_wkt_ewkt_or_geojson() {
    let pts = points(vec![("a", 1.23456, 2.5, None), ("b", 3.0, 4.0, Some(7.25))]);
    let (out, _) = run_for_test("GeometryToText", &[("decimals", "2")], vec![("Input", pts.clone())]).unwrap();
    let o = &out[0].1;
    let w = o.field_index("wkt").unwrap();
    assert_eq!(o.features[0].attrs[w], t("POINT(1.23 2.5)"));
    assert_eq!(o.features[1].attrs[w], t("POINT Z (3 4 7.25)"));
    assert!(o.features[0].geom.is_some());
    let (out, _) =
        run_for_test("GeometryToText", &[("format", "ewkt"), ("heights", "false"), ("keep_geometry", "false")], vec![("Input", pts.clone())]).unwrap();
    let o = &out[0].1;
    assert_eq!(o.features[1].attrs[1], t("SRID=3006;POINT(3 4)"));
    assert!(o.features[1].geom.is_none() && o.crs.is_none());
    let (out, _) = run_for_test("GeometryToText", &[("format", "geojson"), ("name", "geo")], vec![("Input", pts.clone())]).unwrap();
    let j: serde_json::Value = serde_json::from_str(&out[0].1.features[1].attrs[1].to_text()).unwrap();
    assert_eq!(j["coordinates"], serde_json::json!([3.0, 4.0, 7.25]));
    // And back again, as it was.
    let (back, _) = run_for_test("GeometryFromText", &[("attribute", "geo"), ("crs", "3006")], vec![("Input", out[0].1.clone())]).unwrap();
    assert_eq!(back[0].1.features[1].heights(), Some(&[7.25][..]));
}

#[test]
fn duplicates_can_be_found_by_their_geometry() {
    let pts = points(vec![("a", 1.0, 2.0, None), ("b", 1.0004, 2.0, None), ("a", 5.0, 5.0, None), ("c", 1.0, 2.0, None)]);
    let run = |extra: &[(&str, &str)]| run_for_test("DuplicateFilter", extra, vec![("Input", pts.clone())]).unwrap().0;
    assert_eq!(port(&run(&[]), "Duplicate").features.len(), 1);
    assert_eq!(port(&run(&[("geometry", "only")]), "Duplicate").features.len(), 2);
    assert_eq!(port(&run(&[("geometry", "only"), ("decimals", "4")]), "Duplicate").features.len(), 1);
    assert_eq!(port(&run(&[("geometry", "also")]), "Duplicate").features.len(), 0);
}

#[test]
fn points_are_raised_on_sticks_from_the_ground_or_the_terrain() {
    let mut pts = points(vec![("a", 10.0, 10.0, None), ("b", 30.0, 10.0, Some(5.0))]);
    pts.fields.push(Field::new("pop", FieldType::Int));
    pts.features[0].attrs.push(Value::Int(200));
    pts.features[1].attrs.push(Value::Int(500));
    let (out, _) = run_for_test("PointRaiser", &[("height", "pop / 10")], vec![("Points", pts.clone())]).unwrap();
    let (tops, sticks) = (port(&out, "Tops"), port(&out, "Sticks"));
    assert_eq!(tops.features[1].heights(), Some(&[50.0][..]));
    assert_eq!(sticks.features[0].heights(), Some(&[0.0, 20.0][..]));
    let m = sticks.mesh.as_ref().unwrap();
    assert_eq!(m.objects.len(), 2);
    let (lo, hi) = m.bounds().unwrap();
    assert!(lo[2] < 0.1 && hi[2] > 50.0 && hi[2] < 50.0 + 2.6, "{:?} {:?}", lo, hi);
    // From their own heights.
    let (out, _) = run_for_test("PointRaiser", &[("height", "pop / 10"), ("base", "own")], vec![("Points", pts.clone())]).unwrap();
    assert_eq!(port(&out, "Tops").features[1].heights(), Some(&[55.0][..]));
    // On a terrain at 100 m.
    let g = crate::model::Grid {
        width: 10,
        height: 10,
        values: vec![100.0; 100],
        geotransform: [0.0, 10.0, 0.0, 100.0, 0.0, -10.0],
        crs: Some(3006),
        bands: vec![],
    };
    let terr = Table { grid: Some(std::sync::Arc::new(g)), crs: Some(3006), ..Default::default() };
    let (out, _) = run_for_test("PointRaiser", &[("height", "pop / 10"), ("base", "terrain")], vec![("Points", pts), ("Terrain", terr)]).unwrap();
    assert_eq!(port(&out, "Sticks").features[0].heights(), Some(&[100.0, 120.0][..]));
}

#[test]
fn a_diorama_rises_in_terraces_with_its_trees_on_them() {
    // An island of 2 km with a hill 100 m high in the middle, and forest on it all.
    let ring = vec![(0.0, 0.0), (2000.0, 0.0), (2000.0, 2000.0), (0.0, 2000.0), (0.0, 0.0)];
    let island = Table {
        fields: vec![Field::new("kind", FieldType::Text)],
        crs: Some(3006),
        features: vec![Feature { attrs: vec![t("Skog")], geom: Some(geo::Geometry::Polygon(geo::Polygon::new(ring.into(), vec![]))), z: None }],
        ..Default::default()
    };
    let (w, cell) = (100usize, 20.0);
    let values: Vec<f32> = (0..w * w)
        .map(|i| {
            let (x, y) = ((i % w) as f64 * cell + 10.0 - 1000.0, (i / w) as f64 * cell + 10.0 - 1000.0);
            (100.0 * (-(x * x + y * y) / (2.0 * 400.0 * 400.0)).exp()) as f32
        })
        .collect();
    let g = crate::model::Grid { width: w, height: w, values, geotransform: [0.0, cell, 0.0, 2000.0, 0.0, -cell], crs: Some(3006), bands: vec![] };
    let heights = Table { grid: Some(std::sync::Arc::new(g)), crs: Some(3006), ..Default::default() };
    let (out, notes) =
        run_for_test("DioramaMaker", &[("class", "kind"), ("steps", "3"), ("relief", "300")], vec![("Areas", island.clone()), ("Heights", heights)]).unwrap();
    let d = &out[0].1;
    let terraces: Vec<&Feature> = d.features.iter().filter(|f| f.attrs[0].to_text() == "terrace").collect();
    assert_eq!(terraces.len(), 3, "{:?}", notes);
    // Each smaller than the one below, 100 m higher.
    let area = |f: &Feature| crate::geom::area(f.geom.as_ref().unwrap(), false);
    assert!(area(terraces[0]) > area(terraces[1]) && area(terraces[1]) > area(terraces[2]));
    assert!((terraces[1].attrs[4].as_f64().unwrap() - terraces[0].attrs[4].as_f64().unwrap() - 100.0).abs() < 1e-6);
    // A tree in the middle stands on the top terrace, one at the shore on the ground.
    let trees: Vec<(f64, f64, f64)> = d
        .features
        .iter()
        .filter(|f| f.attrs[0].to_text() == "tree")
        .map(|f| match f.geom {
            Some(geo::Geometry::Point(p)) => (p.x(), p.y(), f.attrs[3].as_f64().unwrap()),
            _ => unreachable!(),
        })
        .collect();
    let near = |cx: f64, cy: f64| trees.iter().min_by(|a, b| (a.0 - cx).hypot(a.1 - cy).total_cmp(&(b.0 - cx).hypot(b.1 - cy))).unwrap().2;
    assert!(near(1000.0, 1000.0) >= terraces[2].attrs[4].as_f64().unwrap() - 1e-6);
    assert!(near(150.0, 150.0) < terraces[0].attrs[3].as_f64().unwrap() + 1e-6);
    // Without heights it stays flat.
    let (flat, _) = run_for_test("DioramaMaker", &[("class", "kind")], vec![("Areas", island)]).unwrap();
    assert!(flat[0].1.features.iter().all(|f| f.attrs[0].to_text() != "terrace"));
}

#[test]
fn a_chart_is_also_written_as_a_web_page() {
    let dir = std::env::temp_dir().join(format!("og-chart-html-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let page = dir.join("towns.html");
    let rows = vec![vec![t("Borås"), t("113000")], vec![t("Skövde"), t("57000")], vec![t("Ale </script>"), t("32000")]];
    let towns = table(&["name", "pop"], rows);
    let path = page.to_string_lossy().to_string();
    let (out, _) =
        run_for_test("ChartDrawer", &[("category", "name"), ("value", "pop"), ("title", "Towns & people"), ("html", &path)], vec![("Input", towns)]).unwrap();
    assert!(out[0].1.image.is_some());
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.starts_with("<!doctype html>") && html.contains("<title>Towns &amp; people</title>"));
    assert!(html.contains("\"Borås\",113000") && html.contains("prefers-color-scheme: dark"));
    // A name cannot end the script early.
    assert!(!html.contains("Ale </script>") && html.contains("Ale <\\/script>"));
    assert_eq!(html.matches("</script>").count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

fn picture(w: usize, h: usize, rgb: [u8; 3]) -> Table {
    let rgba = (0..w * h).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect();
    crate::model::RasterImage { width: w, height: h, rgba, geotransform: [0.0, 1.0, 0.0, h as f64, 0.0, -1.0] }.into_table(None)
}

#[test]
fn pictures_are_written_as_slides_for_powerpoint_and_impress() {
    let dir = std::env::temp_dir().join(format!("og-slides-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for ext in ["pptx", "odp"] {
        let path = dir.join(format!("deck.{}", ext)).to_string_lossy().to_string();
        let params = [("path", path.as_str()), ("format", ext), ("cover", "Skärgården & öarna"), ("cover_note", "2026")];
        // Each connection is a slide, titled by the tool it comes from.
        let given = vec![("Pictures", picture(400, 300, [200, 30, 30])), ("Pictures", picture(300, 600, [30, 30, 200])), ("Pictures", Table::default())];
        let (_, notes) = run_for_test("SlideWriter", &params, given).unwrap();
        assert!(notes.warnings.iter().any(|w| w.contains("1 of the streams")), "{:?}", notes.warnings);
        let f = std::fs::File::open(dir.join(format!("deck.{}", ext))).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        if ext == "pptx" {
            assert!(names.contains(&"ppt/slides/slide3.xml".to_string()) && !names.contains(&"ppt/slides/slide4.xml".to_string()), "{:?}", names);
            assert!(names.contains(&"ppt/media/image2.png".to_string()));
            let mut s = String::new();
            std::io::Read::read_to_string(&mut z.by_name("ppt/slides/slide1.xml").unwrap(), &mut s).unwrap();
            assert!(s.contains("Skärgården &amp; öarna"));
        } else {
            assert_eq!(names[0], "mimetype");
            let mut s = String::new();
            std::io::Read::read_to_string(&mut z.by_name("content.xml").unwrap(), &mut s).unwrap();
            assert_eq!(s.matches("<draw:page ").count(), 3);
            assert!(s.contains("Pictures/image2.png"));
        }
    }
    // A writer asked for slides says where they are written.
    let e = run_for_test("Writer", &[("path", "x.pptx")], vec![("Input", picture(10, 10, [0, 0, 0]))]).unwrap_err();
    assert!(e.0.contains("output for slides"), "{}", e.0);
    assert_eq!(output_kind("odp"), "SlideWriter");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_model_is_drawn_as_a_picture_and_a_pdf() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let ws = crate::run::Workspace::load(&root.join("stops.ographis")).unwrap();
    let img = crate::modelpic::draw(&ws, 1.0).unwrap();
    assert!(img.width > 900 && img.height > 500, "{} × {}", img.width, img.height);
    // Ink where the title is, the canvas colour in an empty corner.
    let dark = img.rgba.as_chunks::<4>().0.iter().take(img.width * 80).filter(|p| p[0] < 100).count();
    assert!(dark > 50, "{}", dark);
    let dir = std::env::temp_dir().join(format!("og-modelpic-{}", std::process::id()));
    for ext in ["png", "pdf", "jpg"] {
        let path = dir.join(format!("stops.{}", ext));
        crate::modelpic::export(&ws, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        match ext {
            "pdf" => assert!(bytes.starts_with(b"%PDF-1.4") && bytes.ends_with(b"%%EOF\n")),
            "png" => assert!(bytes.starts_with(b"\x89PNG")),
            _ => assert!(bytes.starts_with(&[0xff, 0xd8])),
        }
    }
    assert!(crate::modelpic::export(&ws, &dir.join("x.gif")).is_err());
    // A description goes under the title, and the picture grows by its lines.
    let mut described = ws.clone();
    described.description = "Reads the stops of a folder of files.\nWrites them as one GeoPackage.".into();
    let taller = crate::modelpic::draw(&described, 1.0).unwrap();
    assert_eq!(taller.height, img.height + 56, "{} → {}", img.height, taller.height);
    let _ = std::fs::remove_dir_all(&dir);
}
