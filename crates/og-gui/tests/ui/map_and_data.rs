//! The data under the canvas: the table, the statistics, the map and the 3D view, and the
//! example models that draw maps.

use crate::common::*;

#[test]
fn dark_theme_with_polygons() {
    let mut h = harness(egui::Theme::Dark, NO_TILES);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("areas_sweref");
    settle(&mut h);
    shot(&mut h, "30-areas-dark");
    h.state_mut().focus_node("clip_routes");
    settle(&mut h);
    shot(&mut h, "31-lines-dark");
}

#[test]
fn background_tiles_from_a_local_server() {
    let mut h = harness(egui::Theme::Light, &tile_server());
    h.state_mut().run_blocking();
    h.state_mut().focus_node("areas_sweref");
    settle(&mut h);
    settle(&mut h);
    assert!(h.state().map.shows_tiles(), "with a tile server that answers the background should be shown");
    shot(&mut h, "40-background-tiles");
}

#[test]
fn the_table_is_sorted_by_a_click_on_a_column() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("towns");
    settle(&mut h);
    // In the order of the file, the largest town comes first and the smallest are out of sight.
    assert!(h.state().sort.is_none());
    // The town and its municipality have the same name, so the name is there twice.
    let shown = |h: &Harness<'_, App>, name: &str| h.query_all_by_label(name).count() > 0;
    assert!(shown(&h, "Göteborg") && !shown(&h, "Gullspång"));

    // One click sorts upwards: the smallest first.
    h.get_by_label("population").click();
    h.run_steps(3);
    assert_eq!(h.state().sort, Some((2, false)));
    assert!(h.query_by_label("▲ population").is_some());
    assert!(!shown(&h, "Göteborg"));
    let table = h.state().result.as_ref().unwrap().output("towns", "Output").unwrap().clone();
    let smallest = table.features.iter().min_by_key(|f| f.attrs[2].as_f64().unwrap() as i64).unwrap().attrs[0].to_text();
    assert!(shown(&h, &smallest), "{} should be at the top", smallest);
    shot(&mut h, "137-sorted");
    // The number of a row is still its place in the stream, and picking a row picks that feature.
    h.get_all_by_label(smallest.as_str()).next().unwrap().click();
    h.run_steps(3);
    let picked = h.state().view.feature.unwrap();
    assert_eq!(table.features[picked].attrs[0].to_text(), smallest);

    // A second click sorts downwards, a third gives the order of the stream back.
    h.get_by_label("▲ population").click();
    h.run_steps(3);
    assert_eq!(h.state().sort, Some((2, true)));
    assert!(h.query_by_label("▼ population").is_some() && shown(&h, "Göteborg"));
    h.get_by_label("▼ population").click();
    h.run_steps(3);
    assert!(h.state().sort.is_none() && h.query_by_label("population").is_some());
    // Text sorts as text. Another node starts unsorted.
    h.get_by_label("name").click();
    h.run_steps(3);
    assert!(shown(&h, "Ale") || shown(&h, "Alingsås"));
    h.state_mut().focus_node("routes");
    h.run_steps(3);
    assert!(h.state().sort.is_none());
}

#[test]
fn area_and_length_are_rounded_to_the_decimals_that_are_asked_for() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("area");
    settle(&mut h);
    let values = |h: &Harness<'_, App>, node: &str, name: &str| -> Vec<f64> {
        let r = h.state().result.clone().unwrap();
        let t = r.output(node, "Output").unwrap();
        let i = t.field_index(name).unwrap();
        t.features.iter().filter_map(|f| f.attrs[i].as_f64()).collect()
    };
    let whole = |v: f64| (v - v.round()).abs() < 1e-9;
    let before = values(&h, "area", "area_km2");
    assert!(!before.is_empty() && before.iter().any(|v| !whole(v * 10.0)), "without a setting the decimals are kept: {:?}", before);

    // The form of both tools asks for the number of decimals.
    assert!(h.query_all_by_label("Calculate area").count() >= 2, "the node and its form are headed by what the tool does");
    assert!(h.query_by_label("Decimals").is_some() && h.query_by_label("Rounds to this many decimals. Empty keeps them all").is_some());
    let field = h.get_by_label("Decimals").rect().center() + egui::vec2(40.0, 24.0);
    type_at(&mut h, field, "1");
    assert_eq!(param(&h, "area", "decimals"), "1");
    shot(&mut h, "145-decimals");
    h.state_mut().focus_node("length");
    h.run_steps(3);
    assert!(h.query_all_by_label("Calculate length").count() >= 2);
    let field = h.get_by_label("Decimals").rect().center() + egui::vec2(40.0, 24.0);
    type_at(&mut h, field, "0");
    assert_eq!(param(&h, "length", "decimals"), "0");

    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(h.state().result.as_ref().unwrap().errors, 0);
    let after = values(&h, "area", "area_km2");
    assert_eq!(after.len(), before.len());
    for (a, b) in after.iter().zip(&before) {
        assert!(whole(a * 10.0) && (a - b).abs() <= 0.05 + 1e-9, "{} should be {} with one decimal", a, b);
    }
    assert!(values(&h, "length", "length_km").iter().all(|v| whole(*v)));

    // Something that is not a number of decimals is said on the node, not guessed at.
    h.state_mut().snarl.nodes_mut().find(|n| n.id == "length").unwrap().params.insert("decimals".into(), serde_json::json!("many"));
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert!(r.nodes["length"].error.as_ref().unwrap().starts_with("The number of decimals must be a whole number from 0 to 15"));
}

#[test]
fn points_are_counted_and_joined_by_location_on_the_canvas() {
    let dir = example_copy();
    std::fs::write(
        dir.join("location.json"),
        r#"{
      "format": "ographis/1", "name": "Location",
      "nodes": [
        {"id": "towns", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv", "crs": "EPSG:4326"}},
        {"id": "areas", "type": "Reader", "name": "Subregions", "x": 0, "y": 220, "params": {"path": "subregions.gpkg"}},
        {"id": "count", "type": "PointCounter", "x": 300, "y": 220, "params": {}},
        {"id": "join", "type": "SpatialJoiner", "x": 300, "y": 0, "params": {}}
      ],
      "edges": [
        {"from": ["areas", "Output"], "to": ["count", "Polygons"]},
        {"from": ["towns", "Output"], "to": ["count", "Points"]},
        {"from": ["towns", "Output"], "to": ["join", "Features"]},
        {"from": ["areas", "Output"], "to": ["join", "Polygons"]}
      ]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("location.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    // The nodes are named by what the tools do, and say how they are set up.
    let names: Vec<String> = h.state().snarl.nodes().map(|n| n.name.clone()).collect();
    assert!(names.contains(&"Count points in polygons".to_string()) && names.contains(&"Join by location".to_string()), "{:?}", names);
    assert!(h.query_by_label("point_count").is_some() && h.query_by_label("all attributes, by a point").is_some());
    assert!(!h.state().dirty);

    h.state_mut().run_blocking();
    h.state_mut().focus_node("count");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let counted = r.output("count", "Polygons").unwrap();
    let i = counted.field_index("point_count").unwrap();
    assert_eq!(counted.features.iter().filter_map(|f| f.attrs[i].as_f64()).sum::<f64>(), 39.0, "every town is in one subregion");
    assert_eq!(counted.len(), 4);
    for label in ["Count points in polygons", "Count attribute", "Polygons 4", "Points"] {
        assert!(h.query_all_by_label(label).count() >= 1, "the window should say: {}", label);
    }
    shot(&mut h, "146-count-points");

    h.state_mut().focus_node("join");
    settle(&mut h);
    let joined = r.output("join", "Joined").unwrap();
    assert_eq!((joined.len(), r.output("join", "Unjoined").unwrap().len()), (39, 0));
    assert!(joined.field_index("subregion").is_some() && joined.field_index("population").is_some());
    for label in ["Join by location", "Attributes to fetch", "Prefix", "Joined 39", "Unjoined 0"] {
        assert!(h.query_all_by_label(label).count() >= 1, "the window should say: {}", label);
    }
    shot(&mut h, "147-join-by-location");
}

#[test]
fn attributes_are_managed_in_a_table() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("outside");
    settle(&mut h);
    // Every attribute that comes in has a row: the listed ones first, then what has not
    // been listed, which this node removes.
    for label in ["name", "municipality", "population", "new", "Attributes that are not listed", "New attributes"] {
        assert!(h.query_all_by_label(label).count() >= 1, "the form should say: {}", label);
    }
    assert!(h.query_all_by_value("town").count() >= 1, "the new name is in its field");
    assert!(h.query_all_by_value("Rename").count() == 1 && h.query_all_by_value("Keep").count() == 2);
    shot(&mut h, "153-manage-attributes");
    let before = h.state().result.as_ref().unwrap().output("outside", "Output").unwrap().field_names();
    assert_eq!(before, ["town", "municipality", "population"]);

    // Population is removed, municipality goes first, and a new attribute is made.
    h.get_all_by_value("Keep").nth(1).unwrap().click();
    h.run_steps(3);
    h.get_all_by_label("Remove").last().unwrap().click();
    h.run_steps(3);
    let down = h.get_all_by_label("▼").next().unwrap().rect().center();
    click_at(&mut h, down);
    h.get_by_label("Add attribute").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Add attribute").click();
    h.run_steps(3);
    let rules = h.state().snarl.nodes().find(|n| n.id == "outside").unwrap().params["attributes"].clone();
    assert_eq!(rules[0]["name"], "municipality");
    assert_eq!(rules[1]["name"], "name");
    assert_eq!(rules.as_array().unwrap().iter().find(|r| r["name"] == "population").unwrap()["action"], "remove");
    h.state_mut()
        .snarl
        .nodes_mut()
        .find(|n| n.id == "outside")
        .unwrap()
        .params
        .insert("create".into(), serde_json::json!([{"name": "county", "value": "'Västra Götaland'"}]));
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.output("outside", "Output").unwrap().field_names(), ["municipality", "town", "county"]);
    assert!(h.query_all_by_value("county").count() >= 1 && h.query_all_by_value("'Västra Götaland'").count() >= 1);
    shot(&mut h, "154-manage-attributes-changed");
}

#[test]
fn manage_attributes_from_an_earlier_version_becomes_a_table() {
    let dir = example_copy();
    std::fs::write(
        dir.join("old_manager.json"),
        r#"{
      "format": "ographis/1", "name": "From 0.52",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv", "crs": "EPSG:4326"}},
        {"id": "m", "type": "AttributeManager", "x": 300, "y": 0, "params": {"keep": "population, name", "rename": {"name": "town"}}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["m", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("old_manager.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    // Before a run the form says that it waits for the attributes, and changes nothing.
    h.state_mut().focus_node("m");
    h.run_steps(3);
    assert!(h.query_by_label_contains("The settings this node had in an earlier version are kept until then").is_some());
    assert!(!h.state().dirty);
    h.state_mut().run_blocking();
    settle(&mut h);
    let fields = |h: &Harness<'_, App>| h.state().result.as_ref().unwrap().output("m", "Output").unwrap().field_names();
    assert_eq!(fields(&h), ["population", "town"]);
    // After the run the settings are rows of the table, and the result is the same.
    let params = h.state().snarl.nodes().find(|n| n.id == "m").unwrap().params.clone();
    assert!(!params.contains_key("keep") && !params.contains_key("rename"), "{:?}", params);
    assert_eq!(params["others"], "remove");
    assert_eq!(params["attributes"][0], serde_json::json!({"name": "population"}));
    assert_eq!(params["attributes"][1], serde_json::json!({"name": "name", "action": "rename", "to": "town"}));
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(fields(&h), ["population", "town"]);
    shot(&mut h, "155-manage-attributes-from-before");
}

#[test]
fn the_grid_example_counts_towns_in_squares() {
    let mut h = harness_with("grid.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("count");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.errors, r.warnings), (0, 0), "{:?}", r.log);
    let c = r.output("count", "Polygons").unwrap();
    let i = c.field_index("towns").unwrap();
    assert_eq!(c.features.iter().filter_map(|f| f.attrs[i].as_f64()).sum::<f64>(), 39.0, "every town is on land in the raster");
    let with_towns = c.features.iter().filter(|f| f.attrs[i].as_f64().is_some_and(|n| n > 0.0)).count();
    assert_eq!(r.output("some", "Passed").unwrap().len(), with_towns);
    assert!(r.log.iter().any(|l| l.msg.contains("19 cells without a value were left out")), "the sea has no heights");
    shot(&mut h, "159-grid-example");
}

#[test]
fn the_map_draws_every_feature_also_hundreds_of_thousands() {
    // 300 000 short roads in a grid over a part of Västergötland, in SWEREF 99 TM.
    let n = 300_000usize;
    let side = 600usize;
    let features: Vec<Feature> = (0..n)
        .map(|i| {
            let (x, y) = (360_000.0 + (i % side) as f64 * 200.0, 6_400_000.0 + (i / side) as f64 * 200.0);
            let g = geo::Geometry::LineString(geo::LineString::from(vec![(x, y), (x + 120.0, y + 60.0), (x + 150.0, y + 150.0)]));
            Feature { z: None, attrs: vec![Value::Int(i as i64)], geom: Some(g) }
        })
        .collect();
    let table = Table { mesh: None, fields: vec![Field::new("id", FieldType::Int)], crs: Some(3006), features, raster: false, image: None, grid: None };
    let mut h = app_with_result(table);
    wait_for_map(&mut h);
    // Zoomed out, every road is drawn as one image, and the map says so.
    // What is in view is counted when the picture is shown, which can take a few frames.
    for _ in 0..30 {
        if h.state().map.in_view.is_some_and(|v| v.1) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        h.run_steps(2);
    }
    let (count, picture) = h.state().map.in_view.expect("the map knows what is in view");
    assert!(picture && count >= n, "{} in view, as a picture: {}", count, picture);
    assert!(h.state().map.shows_picture());
    assert!(h.query_by_label_contains("drawn as an image").is_none(), "the note is painted on the map, and is seen in the picture");
    shot(&mut h, "170-map-many");

    // Zoomed in, the roads in view are few enough to be drawn as shapes, which can be clicked.
    // In the middle of the map, where the data lies after the fit.
    let map_at = h.state().map.fitted_rect().center();
    h.event(egui::Event::PointerMoved(map_at));
    h.run_steps(2);
    for _ in 0..4 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 200.0),
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        });
        h.run_steps(4);
    }
    wait_for_map(&mut h);
    let (count, picture) = h.state().map.in_view.unwrap();
    assert!(!picture && count > 0 && count < 40_000, "{} in view, as a picture: {}", count, picture);
    shot(&mut h, "171-map-zoomed-in");
}

#[test]
fn page_up_and_page_down_move_through_the_rows() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("towns");
    settle(&mut h);
    assert_eq!(h.state().view.feature, None);
    // The pointer over the table.
    h.event(egui::Event::PointerMoved(egui::pos2(500.0, 850.0)));
    h.run_steps(2);
    h.key_press(egui::Key::PageDown);
    h.run_steps(3);
    let first = h.state().view.feature.expect("a row is selected a page down");
    assert!(first >= 3, "{}", first);
    h.key_press(egui::Key::PageDown);
    h.run_steps(3);
    let second = h.state().view.feature.unwrap();
    assert_eq!(second, (2 * first).min(38), "a page further");
    h.key_press(egui::Key::PageUp);
    h.run_steps(3);
    assert_eq!(h.state().view.feature, Some(first));
    for _ in 0..20 {
        h.key_press(egui::Key::PageDown);
        h.run_steps(2);
    }
    assert_eq!(h.state().view.feature, Some(38), "the last row, and no further");
    shot(&mut h, "173-page-down");
    for _ in 0..20 {
        h.key_press(egui::Key::PageUp);
        h.run_steps(2);
    }
    assert_eq!(h.state().view.feature, Some(0));
}

#[test]
fn a_map_is_drawn_shown_and_written_as_pictures() {
    let mut h = harness_with("map.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().focus_node("map");
    h.run_steps(3);
    // The form lists the connected streams as layers, top first.
    for label in ["Towns", "Routes", "Subregions", "Fill", "Line"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{} should be in the form", label);
    }
    let y = |h: &Harness<'_, App>, label: &str| {
        h.get_all_by_label(label).map(|n| n.rect().center()).filter(|c| c.x > 1100.0).map(|c| c.y).next().unwrap_or(f32::NAN)
    };
    assert!(y(&h, "Towns") < y(&h, "Routes") && y(&h, "Routes") < y(&h, "Subregions"), "{} {} {}", y(&h, "Towns"), y(&h, "Routes"), y(&h, "Subregions"));
    shot(&mut h, "185-draw-map-form");
    // The arrow moves Routes up, over Towns.
    let arrows: Vec<egui::Pos2> = h.get_all_by_label("▲").map(|n| n.rect().center()).filter(|c| c.x > 1100.0).collect();
    assert_eq!(arrows.len(), 3);
    click_at(&mut h, arrows[1]);
    h.run_steps(3);
    let froms: Vec<String> =
        og_engine::nodes::map_layers(&h.state().snarl.nodes().find(|n| n.id == "map").unwrap().params).into_iter().map(|l| l.from).collect();
    assert_eq!(froms, ["routes:Output", "towns:Output", "areas:Output"]);

    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let out = h.state().base_dir().join("out");
    for f in ["map.png", "map.pgw", "map.tif", "map.pdf"] {
        assert!(out.join(f).exists(), "{} was not written", f);
    }
    let map = r.output("map", "Map").unwrap();
    assert!(map.image.is_some() && map.len() == 1);
    h.state_mut().focus_node("map");
    wait_for_map(&mut h);
    assert!(h.state().map.shows_image(), "the drawn map is shown in the map of the data");
    shot(&mut h, "186-drawn-map-in-data");
}

#[test]
fn the_data_tab_has_statistics() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    h.state_mut().focus_node("towns");
    h.state_mut().view = og_gui::app::View { node: Some("towns".into()), port: None, feature: None, ..Default::default() };
    h.run_steps(4);
    h.get_by_label("Statistics").click();
    h.run_steps(4);
    assert!(h.state().show_stats);
    for head in ["attribute", "values", "empty", "unique", "min", "max", "mean", "sum"] {
        assert!(h.query_by_label(head).is_some(), "{}", head);
    }
    assert!(h.query_by_label("geometry").is_some() && h.query_by_label("population").is_some());
    shot(&mut h, "204-statistics");
    h.get_by_label("Statistics").click();
    h.run_steps(3);
    assert!(!h.state().show_stats && h.query_by_label("mean").is_none());
}

// ---------- Version 0.60 ----------

#[test]
fn terrain_is_shaded_and_seen_in_3d() {
    let mut h = harness_with("terrain.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log.iter().map(|l| l.msg.clone()).collect::<Vec<_>>());
    shot(&mut h, "206-terrain-model");
    // The grid is shown on the map as grey relief.
    h.state_mut().focus_node("dem");
    h.state_mut().view = og_gui::app::View { node: Some("dem".into()), port: None, feature: None, ..Default::default() };
    settle(&mut h);
    shot(&mut h, "207-grid-on-the-map");
    // The shaded relief, and the 3D view.
    h.state_mut().focus_node("shade");
    h.state_mut().view = og_gui::app::View { node: Some("shade".into()), port: None, feature: None, ..Default::default() };
    settle(&mut h);
    for label in ["Sun from", "Sun height", "Height exaggeration", "Shadows", "Darker valleys", "Haze in the valleys in %", "Map opacity in %"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of Shade terrain", label);
    }
    shot(&mut h, "208-shaded-relief");
    h.state_mut().focus_node("view");
    h.state_mut().view = og_gui::app::View { node: Some("view".into()), port: None, feature: None, ..Default::default() };
    settle(&mut h);
    for label in ["Looking from", "Camera height in degrees", "Lens angle", "Zoom in %", "Fog in %", "As a block"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of Quick terrain view", label);
    }
    shot(&mut h, "209-3d-view");
    // The tools are listed under Terrain, and the cell size of a grid in the input.
    let mut e = empty_app();
    unfold(&mut e, "Terrain");
    assert!(e.query_by_label("Shade terrain").is_some(), "shaded relief is under Terrain since 0.79");
    unfold(&mut e, "3D models and views");
    for t in ["Quick terrain view", "Render 3D scene", "Extrude", "Generate TIN"] {
        assert!(e.query_by_label(t).is_some(), "{} should be under 3D models and views", t);
    }
    let mut h2 = harness_with("terrain.ographis", egui::Theme::Light, NO_TILES);
    h2.state_mut().focus_node("dem");
    h2.run_steps(4);
    assert!(h2.query_by_label("Cell size").is_some());
    h2.state_mut().focus_node("water");
    h2.run_steps(4);
    assert!(h2.query_by_label("Cell size").is_none(), "only for a raster read as a grid");
}

#[test]
fn pictures_and_grids_are_named_in_the_table() {
    let mut h = harness_with("terrain.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    h.state_mut().focus_node("shade");
    h.state_mut().view = og_gui::app::View { node: Some("shade".into()), port: None, feature: None, ..Default::default() };
    settle(&mut h);
    assert!(h.query_by_label("picture 640 × 480").is_some());
    assert!(h.query_by_label_contains("Picture").is_some());
    h.state_mut().view = og_gui::app::View { node: Some("dem".into()), port: None, feature: None, ..Default::default() };
    settle(&mut h);
    assert!(h.query_by_label("grid 640 × 480").is_some());
    h.state_mut().view = og_gui::app::View { node: Some("view".into()), port: None, feature: None, ..Default::default() };
    settle(&mut h);
    assert!(h.query_by_label_contains("no coordinates").is_some());
    shot(&mut h, "212-picture-in-the-table");
}

// ---------- Version 0.64 ----------

#[test]
fn draw_map_shows_values_by_attribute() {
    let mut h = harness_with("map.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    h.state_mut().focus_node("map");
    settle(&mut h);
    assert!(h.query_by_label("Legend").is_some());
    // Each layer has its values folded in; one is opened.
    let values = h.get_all_by_label("Values").next().unwrap().rect().center();
    click_at(&mut h, values);
    h.run_steps(3);
    for label in ["Size by", "Colour by", "Order", "Gather points within"] {
        assert!(h.query_by_label(label).is_some(), "{} should be under Values", label);
    }
    // Choosing an attribute stores it with the layer, and the map draws by it.
    let top = h.get_all_by_value("None").next().unwrap().rect().center();
    click_at(&mut h, top);
    h.run_steps(3);
    h.get_by_label("population").click();
    h.run_steps(3);
    let layers = h.state().snarl.nodes().find(|n| n.id == "map").unwrap().params.get("layers").cloned().unwrap();
    assert!(layers.as_array().unwrap().iter().any(|l| l["size_by"] == "population"), "{}", layers);
    assert!(h.query_by_label("Values · in use").is_some());
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(h.state().result.as_ref().unwrap().errors, 0);
    shot(&mut h, "240-map-values");
}

// ---------- Version 0.65 ----------

#[test]
fn a_raster_read_as_an_image_tells_the_value_of_a_clicked_cell() {
    let dir = example_copy();
    std::fs::write(
        dir.join("picture.json"),
        r#"{"nodes": [{"id": "dem", "type": "Reader", "name": "Terrain as a picture", "x": 40, "y": 40, "params": {"path": "terrain.tif", "raster_as": "image"}}], "edges": []}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("picture.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let out = h.state().result.as_ref().unwrap().output("dem", "Output").unwrap();
    assert!(out.image.is_some() && out.grid.is_some(), "the picture carries the values of its cells");
    h.state_mut().focus_node("dem");
    settle(&mut h);
    wait_for_map(&mut h);
    let at = h.state().map.fitted_rect().center();
    click_at(&mut h, at);
    h.run_steps(4);
    let text = h.state().map.probe_text().map(str::to_string);
    let text = text.expect("a click on the picture tells the value");
    assert!(text.starts_with("column ") && text.contains(", row ") && !text.contains("no value"), "{}", text);
    shot(&mut h, "250-raster-cell-value");
    // Pointing is enough: the value under the pointer is written at the top of the map.
    h.input_mut().events.push(egui::Event::PointerMoved(at + egui::vec2(40.0, 20.0)));
    h.run_steps(4);
    shot(&mut h, "251-raster-value-under-pointer");
}

// ---------- Version 0.66 ----------

#[test]
fn the_map_turns_in_3d_with_the_heights() {
    let dir = example_copy();
    std::fs::write(
        dir.join("three.json"),
        r#"{"nodes": [
          {"id": "dem", "type": "Reader", "name": "Terrain model", "x": 40, "y": 40, "params": {"path": "terrain.tif", "raster_as": "grid"}},
          {"id": "lines", "type": "ContourGenerator", "name": "Contour lines", "x": 260, "y": 40, "params": {"interval": "20"}},
          {"id": "up", "type": "HeightSetter", "name": "At their height", "x": 480, "y": 40, "params": {"height": "height"}}],
         "edges": [{"from": ["dem", "Output"], "to": ["lines", "Heights"]}, {"from": ["lines", "Contours"], "to": ["up", "Input"]}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("three.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(h.state().result.as_ref().unwrap().errors, 0);
    h.state_mut().focus_node("up");
    settle(&mut h);
    h.get_by_label("3D").click();
    h.run_steps(4);
    assert!(h.state().map.three_d);
    assert!(h.query_by_value("heights × 1").is_some() || h.query_all_by_role(egui::accesskit::Role::SpinButton).count() > 0);
    shot(&mut h, "260-contours-in-3d");
    // A drag turns the view.
    let before = (h.state().map.view3d.azimuth, h.state().map.view3d.elevation);
    let map = h.get_by_label("3D").rect().center() + egui::vec2(-150.0, 200.0);
    drag(&mut h, map, map + egui::vec2(80.0, 40.0), egui::PointerButton::Primary);
    h.run_steps(3);
    let after = (h.state().map.view3d.azimuth, h.state().map.view3d.elevation);
    assert!(after.0 != before.0 && after.1 != before.1, "{:?} {:?}", before, after);
    // The terrain model itself is a surface.
    h.state_mut().map.view3d.exaggeration = 3.0;
    h.state_mut().focus_node("dem");
    settle(&mut h);
    shot(&mut h, "261-terrain-in-3d");
}

// ---------- Version 0.67 ----------

#[test]
fn a_house_from_obj_is_seen_in_3d_and_rendered() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("house.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    // Small pictures, so that the test is quick.
    for n in app.snarl.nodes_mut() {
        if n.kind == "SceneRenderer" {
            n.params.insert("width".into(), serde_json::json!("160"));
            n.params.insert("height".into(), serde_json::json!("100"));
            n.params.insert("quality".into(), serde_json::json!("4"));
        }
    }
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let house = r.output("house", "Output").unwrap();
    assert!(house.mesh.is_some() && house.len() == 7);
    assert!(r.output("spring", "Image").unwrap().image.is_some());
    // The house turns in 3D under the canvas, in the colours of its materials.
    h.state_mut().focus_node("house");
    settle(&mut h);
    h.get_by_label("3D").click();
    h.run_steps(4);
    shot(&mut h, "270-house-in-3d");
    // The form asks for a date and a time only when the sun is to stand as it does then.
    h.state_mut().focus_node("studio");
    settle(&mut h);
    assert!(h.query_by_label("Sun from").is_some() && h.query_by_label("Date").is_none());
    h.state_mut().focus_node("spring");
    settle(&mut h);
    assert!(h.query_by_label("Date").is_some() && h.query_by_label("Time zone").is_some() && h.query_by_label("Sun from").is_none());
    shot(&mut h, "271-render-3d-scene");
}

#[test]
fn a_model_without_a_coordinate_system_is_drawn_in_the_map() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("house.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    // A model by itself, as a chair from a design program: no coordinate system, no place.
    for n in app.snarl.nodes_mut() {
        if n.kind == "Reader" && n.params.get("path").and_then(|v| v.as_str()) == Some("house/house.glb") {
            for k in ["crs", "obj_east", "obj_north", "obj_height", "obj_rotation"] {
                n.params.remove(k);
            }
        }
        if n.kind == "SceneRenderer" {
            n.params.insert("width".into(), serde_json::json!("80"));
            n.params.insert("height".into(), serde_json::json!("50"));
            n.params.insert("quality".into(), serde_json::json!("4"));
        }
    }
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    let t = r.output("neighbour", "Output").unwrap();
    assert!(t.crs.is_none() && t.features.iter().any(|f| f.geom.is_some()));
    h.state_mut().focus_node("neighbour");
    settle(&mut h);
    shot(&mut h, "272-model-without-crs-in-map");
}

#[test]
fn a_view_shows_a_model_and_the_terrain_together() {
    let dir = example_copy();
    std::fs::write(
        dir.join("together.json"),
        r#"{"format": "ographis/1", "name": "Together", "nodes": [
            {"id": "house", "type": "Reader", "name": "House", "x": 20, "y": 20, "params": {"path": "house/house.glb", "crs": "EPSG:3006", "obj_east": "418216", "obj_north": "6472480", "obj_height": "86.6"}},
            {"id": "big", "type": "Scaler", "name": "Scale", "x": 250, "y": 20, "params": {"factor": "3", "around": "all"}},
            {"id": "dem", "type": "Reader", "name": "Terrain", "x": 20, "y": 200, "params": {"path": "terrain.tif", "raster_as": "grid"}},
            {"id": "view", "type": "Viewer", "name": "View", "x": 500, "y": 100, "params": {}}
        ], "edges": [
            {"from": ["house", "Output"], "to": ["big", "Input"]},
            {"from": ["big", "Output"], "to": ["view", "Input"]},
            {"from": ["dem", "Output"], "to": ["view", "Input"]}
        ]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("together.json")).unwrap();
    assert!(og_gui::app::window_title(&app).contains("together.json"), "{}", og_gui::app::window_title(&app));
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    // The model is three times as large, its heights too, standing where it stood.
    let before = r.output("house", "Output").unwrap().mesh.clone().unwrap().bounds().unwrap();
    let after = r.output("big", "Output").unwrap().mesh.clone().unwrap().bounds().unwrap();
    for k in 0..3 {
        assert!(((after.1[k] - after.0[k]) - 3.0 * (before.1[k] - before.0[k])).abs() < 1e-6, "{k}");
    }
    assert!((after.0[2] - before.0[2]).abs() < 1e-6);
    h.state_mut().focus_node("view");
    settle(&mut h);
    shot(&mut h, "273-view-model-and-terrain-in-map");
    h.get_by_label("3D").click();
    h.run_steps(4);
    shot(&mut h, "274-view-model-and-terrain-in-3d");
    // Each stream of the View can be hidden from the map and 3D, and the View keeps it.
    h.get_by_label("3D").click();
    h.run_steps(3);
    let boxes: Vec<egui::Rect> =
        h.query_all_by_role(egui::accesskit::Role::CheckBox).filter(|b| b.rect().top() > 680.0 && b.rect().left() < 800.0).map(|b| b.rect()).collect();
    assert_eq!(boxes.len(), 2, "a tick box for each stream");
    click_at(&mut h, boxes[1].center());
    h.run_steps(4);
    let hidden = h.state().snarl.nodes().find(|n| n.id == "view").unwrap().params.get("hidden").cloned();
    assert_eq!(hidden, Some(serde_json::json!(["2 · Terrain"])), "{:?}", hidden);
    shot(&mut h, "278-view-with-a-layer-hidden");
    // About tells what Ographis is built with.
    h.state_mut().about = true;
    h.run_steps(3);
    h.get_by_label("Built with").click();
    h.run_steps(3);
    for part in ["GDAL", "PROJ", "egui", "geo", "tiny-skia"] {
        assert!(h.query_by_label(part).is_some(), "{} in About", part);
    }
    shot(&mut h, "279-about-built-with");
}

#[test]
fn a_layer_colour_is_chosen_in_draw_map() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("map.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().focus_node("map");
    settle(&mut h);
    // The colour button of Fill of the first layer opens the colour picker.
    let fill = h.get_all_by_label("Fill").next().unwrap().rect();
    click_at(&mut h, egui::pos2(fill.right() + 22.0, fill.center().y));
    h.run_steps(4);
    shot(&mut h, "280-layer-colour-picker");
    assert!(
        h.query_all_by_role(egui::accesskit::Role::Slider).count() + h.query_all_by_role(egui::accesskit::Role::SpinButton).count() > 0,
        "the colour picker is open"
    );
}

#[test]
fn a_3d_tiles_tileset_has_its_own_settings_and_is_seen_in_3d() {
    let dir = example_copy();
    std::fs::write(
        dir.join("tiles_view.ographis"),
        r#"{"format": "ographis/1", "name": "Tiles", "nodes": [
            {"id": "tiles", "type": "Reader", "name": "Houses", "x": 20, "y": 20, "params": {"path": "tiles/tileset.json", "tiles_crs": "EPSG:3006", "tiles_dz": "-32"}},
            {"id": "dem", "type": "Reader", "name": "Terrain", "x": 20, "y": 200, "params": {"path": "terrain.tif", "raster_as": "grid"}},
            {"id": "view", "type": "Viewer", "name": "View", "x": 300, "y": 100, "params": {}}
        ], "edges": [
            {"from": ["tiles", "Output"], "to": ["view", "Input"]},
            {"from": ["dem", "Output"], "to": ["view", "Input"]}
        ]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("tiles_view.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().focus_node("tiles");
    settle(&mut h);
    // The settings of a tileset, and not those of other files.
    for label in ["Level of detail", "At most this many tiles", "Coordinate system to place them in", "Move the heights by"] {
        assert!(h.query_by_label(label).is_some(), "{label} should be in the form");
    }
    for label in ["Units of the model", "Tags as attributes", "Largest error, in metres"] {
        assert!(h.query_by_label(label).is_none(), "{label} should not be in the form");
    }
    shot(&mut h, "290-3d-tiles-form");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.output("tiles", "Output").unwrap().len(), 3);
    h.state_mut().focus_node("view");
    settle(&mut h);
    h.get_by_label("3D").click();
    h.run_steps(4);
    shot(&mut h, "291-3d-tiles-in-3d");
}

#[test]
fn the_3d_scene_has_light_water_and_a_sun_study() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("house.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().focus_node("evening");
    settle(&mut h);
    for label in ["Cloud cover in %", "Haze in %", "The models as", "Reflections", "Light bounces", "Water", "Water height", "Waves"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} should be in the form");
    }
    // The settings of a sun study only for a sun study.
    assert!(h.query_by_label("Dates").is_none());
    shot(&mut h, "310-scene-evening-form");
    h.state_mut().focus_node("study");
    settle(&mut h);
    for label in ["Dates", "From", "To", "Every so many hours", "Each picture also in"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} should be in the form");
    }
    assert!(h.query_by_label("Water height").is_none(), "no water, so no height");
    shot(&mut h, "311-scene-study-form");
}

#[test]
fn blocks_and_a_tin_at_dusk_and_keys_and_print_settings() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("extrude_tin.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    // Small and quick pictures: the renderer is slow in a debug build.
    for n in app.snarl.nodes_mut() {
        if n.kind == "SceneRenderer" {
            n.params.insert("width".into(), serde_json::json!("120"));
            n.params.insert("height".into(), serde_json::json!("80"));
            n.params.insert("quality".into(), serde_json::json!("4"));
        }
    }
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().focus_node("extrude");
    settle(&mut h);
    for label in ["Height", "The height is", "Bottom", "Bottom value", "Colour of the walls", "Colour of the top", "Names from"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} should be in the form of Extrude");
    }
    shot(&mut h, "320-extrude-form");
    h.state_mut().focus_node("tin");
    settle(&mut h);
    for label in ["Make", "Tolerance", "At most corners", "Colour"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} should be in the form of Generate TIN");
    }
    shot(&mut h, "321-tin-form");
    h.state_mut().focus_node("dusk");
    settle(&mut h);
    for label in [
        "Lit windows in %",
        "Windows on plain walls",
        "Light in materials named",
        "Lamps, strength in %",
        "Lamps, colour",
        "Depth of field, blur in %",
        "Focus distance",
    ] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} should be in the form of the dusk scene");
    }
    shot(&mut h, "322-scene-dusk-form");
    // The whole model runs, and the 3D model is written as glTF.
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert!(dir.join("out/blocks.glb").exists() && dir.join("out/blocks_at_dusk.png").exists());
    // The blocks in the 3D view.
    h.state_mut().focus_node("extrude");
    h.state_mut().view = og_gui::app::View { node: Some("extrude".into()), port: Some("Output".into()), feature: None, ..Default::default() };
    settle(&mut h);
    if let Some(b) = h.query_by_label("3D") {
        b.click();
        h.run_steps(6);
    }
    shot(&mut h, "323-blocks-in-3d");
    // A GeoPackage gets a primary key; a PDF its print settings.
    h.state_mut().focus_node("gpkg");
    settle(&mut h);
    assert!(h.query_by_label("Primary key").is_some() && h.query_by_label("Key column").is_some());
    assert!(h.query_by_label("Key attribute").is_none(), "only when an attribute is the key");
    let set = |h: &mut Harness<'_, App>, id: &str, key: &str, value: &str| {
        h.state_mut().snarl.nodes_mut().find(|n| n.id == id).unwrap().params.insert(key.into(), serde_json::json!(value));
    };
    set(&mut h, "gpkg", "key", "attribute");
    settle(&mut h);
    assert!(h.query_by_label("Key attribute").is_some() && h.query_by_label("Key column").is_none());
    shot(&mut h, "324-writer-primary-key");
    set(&mut h, "png", "path", "out/blocks_on_tin.pdf");
    set(&mut h, "png", "pdf_size", "custom");
    set(&mut h, "png", "pdf_compress", "jpeg");
    h.state_mut().focus_node("png");
    settle(&mut h);
    for label in ["Page size", "Page width in mm", "Page height in mm", "Picture packed as", "JPEG quality"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} should be in the form of a PDF output");
    }
    assert!(h.query_by_label("Pixels to the inch").is_none(), "only for a page as large as the picture");
    assert!(h.query_by_label("Primary key").is_none());
    shot(&mut h, "325-pdf-print-settings");
}

#[test]
fn labels_icons_and_a_house_from_ifc() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("labels.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    h.state_mut().focus_node("pois");
    settle(&mut h);
    // The layer of places: its labels and icons are in use, and so folded out.
    assert!(h.query_by_label("Labels · in use").is_some() && h.query_by_label("Icons · in use").is_some());
    for label in ["Label text", "All, also where they overlap", "Bold", "CAPITALS", "Halo"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} should be among the labels of the layer");
    }
    assert!(h.query_by_value("amenity").is_some(), "the icons come from amenity");
    shot(&mut h, "330-labels-and-icons-form");
    h.state_mut().view = og_gui::app::View { node: Some("pois".into()), port: Some("Map".into()), feature: None, ..Default::default() };
    settle(&mut h);
    shot(&mut h, "331-places-with-icons");
    h.state_mut().focus_node("map");
    h.state_mut().view = og_gui::app::View { node: Some("map".into()), port: Some("Map".into()), feature: None, ..Default::default() };
    settle(&mut h);
    shot(&mut h, "332-labelled-map");

    // A house from IFC: the settings of IFC, and its elements with their property sets.
    let mut app = App::new();
    app.load(&dir.join("ifc.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    for n in app.snarl.nodes_mut() {
        if n.kind == "SceneRenderer" {
            n.params.insert("width".into(), serde_json::json!("160"));
            n.params.insert("height".into(), serde_json::json!("100"));
            n.params.insert("quality".into(), serde_json::json!("4"));
        }
    }
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().focus_node("villa");
    settle(&mut h);
    for label in ["Openings", "Spaces", "Property sets", "Origin of the model, east"] {
        assert!(h.query_by_label(label).is_some(), "{label} should be in the form of an IFC input");
    }
    assert!(h.query_by_label("Units of the model").is_none(), "IFC says its units itself");
    shot(&mut h, "333-ifc-form");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.output("windows", "Passed").unwrap().len(), 7);
    h.state_mut().view = og_gui::app::View { node: Some("villa".into()), port: Some("Output".into()), feature: None, ..Default::default() };
    settle(&mut h);
    if let Some(b) = h.query_by_label("3D") {
        b.click();
        h.run_steps(6);
    }
    shot(&mut h, "334-ifc-in-3d");
}

#[test]
fn a_click_on_a_building_in_3d_picks_its_row() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("ifc.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("villa");
    settle(&mut h);
    h.get_by_label("3D").click();
    h.run_steps(4);
    assert!(h.state().map.three_d);
    assert_eq!(h.state().view.feature, None);
    // The middle of the 3D view, below and left of the tick box, is on the house.
    let tick = h.get_by_label("3D").rect();
    let middle = egui::pos2(tick.left() - 180.0, tick.bottom() + 125.0);
    click_at(&mut h, middle);
    h.run_steps(4);
    let picked = h.state().view.feature;
    shot(&mut h, "280-picked-in-3d");
    assert!(picked.is_some(), "a click on the house picks an element");
    shot(&mut h, "280-picked-in-3d");
    // The row is chosen in the table too: it is the one drawn as chosen.
    let r = h.state().result.clone().unwrap();
    let t = r.output("villa", "Output").unwrap();
    let name = t.features[picked.unwrap()].attrs[t.field_index("Name").unwrap()].to_text();
    assert!(!name.is_empty());
    // A click beside the house, on nothing, lets go.
    click_at(&mut h, egui::pos2(tick.right() - 10.0, tick.bottom() + 60.0));
    h.run_steps(4);
    assert_eq!(h.state().view.feature, None);
    // Turned and zoomed away, Zoom to data shows all of it again.
    drag(&mut h, middle, middle + egui::vec2(120.0, 40.0), egui::PointerButton::Primary);
    h.event(egui::Event::PointerMoved(middle));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -400.0),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    });
    h.run_steps(6);
    assert!(!h.state().map.view3d.at_home());
    h.get_by_label("Zoom to data").click();
    h.run_steps(4);
    assert!(h.state().map.view3d.at_home());
}

#[test]
fn a_large_point_cloud_is_seen_in_3d() {
    // As a tile of laser points: two million points with heights, more than a graphics card
    // takes as shapes of their own.
    let n = 2_000_000usize;
    let table = Table {
        mesh: None,
        fields: vec![Field::new("class", FieldType::Int)],
        crs: Some(2961),
        features: (0..n)
            .map(|i| {
                let (x, y) = ((i % 1414) as f64 * 0.7, (i / 1414) as f64 * 0.7);
                Feature {
                    attrs: vec![Value::Int(2)],
                    geom: Some(geo::Geometry::Point(geo::Point::new(347000.0 + x, 5424000.0 + y))),
                    z: Some(std::sync::Arc::from(vec![10.0 + 20.0 * (x / 80.0).sin()])),
                }
            })
            .collect(),
        raster: false,
        image: None,
        grid: None,
    };
    let mut h = app_with_result(table);
    h.state_mut().focus_node("in");
    settle(&mut h);
    h.get_by_label("3D").click();
    h.run_steps(6);
    assert!(h.state().map.three_d);
    shot(&mut h, "283-point-cloud-in-3d");
}

// ---------- Version 0.87 ----------

/// The answer of an API, such as every stop of an OpenTripPlanner, is megabytes of text in
/// one value. A click on its row once took the program down; the cell shows the beginning.
#[test]
fn a_row_with_megabytes_of_text_is_picked_and_drawn() {
    let stops: Vec<String> = (0..40_000).map(|i| format!(r#"{{"id":"1:{i}","name":"Stop {i}","lat":57.7,"lon":11.9}}"#)).collect();
    let one_line = format!("[{}]", stops.join(","));
    let lines = stops.join(",\n");
    assert!(one_line.len() > 2_000_000);
    let table = Table {
        mesh: None,
        fields: vec![Field::new("name", FieldType::Text), Field::new("_response", FieldType::Text), Field::new("_status", FieldType::Int)],
        crs: None,
        features: vec![
            Feature { z: None, attrs: vec![Value::Text("first".into()), Value::Text(one_line), Value::Int(200)], geom: None },
            Feature { z: None, attrs: vec![Value::Text("second".into()), Value::Text(lines), Value::Int(200)], geom: None },
        ],
        raster: false,
        image: None,
        grid: None,
    };
    let mut h = app_with_result(table);
    settle(&mut h);
    assert!(h.query_all_by_label_contains("characters)").next().is_some(), "the cell says how long the text is");
    click_text(&mut h, "second", 0, false);
    h.run_steps(3);
    assert_eq!(h.state().view.feature, Some(1));
    click_text(&mut h, "first", 0, false);
    h.run_steps(3);
    assert_eq!(h.state().view.feature, Some(0));
    shot(&mut h, "087-long-text");
}

/// A click with keys held down, as on a row.
fn click_with(h: &mut Harness<'_, App>, label: &str, keys: egui::Modifiers) {
    let pos = place_of(h, label, 0, false);
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    h.event(egui::Event::ModifiersChanged(keys));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: keys });
        h.run_steps(2);
    }
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    // The table scrolls to the row picked, smoothly.
    settle(h);
}

/// Ctrl adds a row to those picked or takes it away, Shift picks all between, and the
/// map draws them all as picked.
#[test]
fn several_rows_are_picked_with_ctrl_and_shift() {
    let mut h = app_with_result(points(0, 12));
    settle(&mut h);
    click_text(&mut h, "0001", 0, false);
    h.run_steps(2);
    assert_eq!((h.state().view.feature, h.state().view.more.clone()), (Some(1), vec![]));
    click_with(&mut h, "0003", egui::Modifiers::COMMAND);
    click_with(&mut h, "0004", egui::Modifiers::COMMAND);
    let mut all: Vec<usize> = h.state().view.more.iter().copied().chain(h.state().view.feature).collect();
    all.sort();
    assert_eq!(all, [1, 3, 4]);
    h.run_steps(2);
    assert_eq!(h.state().map.also.len(), 2, "the map draws the others as picked");
    shot(&mut h, "087-several-rows");
    // Ctrl again on one takes it away.
    click_with(&mut h, "0003", egui::Modifiers::COMMAND);
    let mut all: Vec<usize> = h.state().view.more.iter().copied().chain(h.state().view.feature).collect();
    all.sort();
    assert_eq!(all, [1, 4]);
    // Shift picks from the one picked first to the one clicked.
    click_text(&mut h, "0002", 0, false);
    settle(&mut h);
    click_with(&mut h, "0005", egui::Modifiers::SHIFT);
    let mut all: Vec<usize> = h.state().view.more.iter().copied().chain(h.state().view.feature).collect();
    all.sort();
    assert_eq!(all, [2, 3, 4, 5]);
    // A plain click picks one again.
    click_text(&mut h, "0000", 0, false);
    h.run_steps(2);
    assert_eq!((h.state().view.feature, h.state().view.more.len()), (Some(0), 0));
}
