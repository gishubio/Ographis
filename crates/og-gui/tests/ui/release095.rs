//! What came in 0.95.

use crate::common::*;

/// A line drawn across a connection with Ctrl and the right button cuts it.
#[test]
fn a_line_drawn_with_ctrl_and_the_right_button_cuts_connections() {
    let (mut h, _dir) = two_nodes();
    assert_eq!(wires(&h), ["in.Output > out.Input"]);
    let mid = between(&h, ("in", "Output"), ("out", "Input"));
    let (a, b) = (mid + egui::vec2(-10.0, -60.0), mid + egui::vec2(10.0, 60.0));
    let ctrl = egui::Modifiers::COMMAND;
    h.event(egui::Event::ModifiersChanged(ctrl));
    h.event(egui::Event::PointerMoved(a));
    h.event(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Secondary, pressed: true, modifiers: ctrl });
    h.run_steps(2);
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.event(egui::Event::PointerMoved(a + (b - a) * t));
        h.run_steps(2);
    }
    shot(&mut h, "95-cut-line");
    h.event(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Secondary, pressed: false, modifiers: ctrl });
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::default()));
    h.run_steps(4);
    assert!(wires(&h).is_empty(), "{:?}", wires(&h));
    // As one step that can be undone.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(4);
    assert_eq!(wires(&h), ["in.Output > out.Input"]);
}

/// F joins the selected nodes from left to right; a node dragged with Alt is lifted out of
/// its connections, and what came in goes on to where it went.
#[test]
fn f_connects_the_selected_nodes_and_alt_lifts_a_node_out() {
    let dir = example_copy();
    std::fs::write(
        dir.join("three.json"),
        r#"{
      "format": "ographis/1", "name": "Three",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "sort", "type": "Sorter", "name": "Sort", "x": 350, "y": 0},
        {"id": "out", "type": "Writer", "name": "Result", "x": 700, "y": 0, "params": {"path": "out/three.csv"}}
      ],
      "edges": []
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("three.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    let all: Vec<egui_snarl::NodeId> = ["out", "in", "sort"].iter().map(|id| graph_id(&h, id)).collect();
    h.state_mut().set_selection(all);
    h.run_steps(2);
    h.key_press(egui::Key::F);
    h.run_steps(4);
    assert_eq!(wires(&h), ["in.Output > sort.Input", "sort.Sorted > out.Input"]);
    // Alt and a drag of the middle node.
    let sort = graph_id(&h, "sort");
    h.state_mut().set_selection(vec![sort]);
    h.run_steps(2);
    let from = place_of(&h, "Sort", 0, true);
    let alt = egui::Modifiers::ALT;
    h.event(egui::Event::ModifiersChanged(alt));
    h.event(egui::Event::PointerMoved(from));
    h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: alt });
    h.run_steps(2);
    for k in 1..=4 {
        h.event(egui::Event::PointerMoved(from + egui::vec2(0.0, 40.0 * k as f32)));
        h.run_steps(2);
    }
    h.event(egui::Event::PointerButton { pos: from + egui::vec2(0.0, 160.0), button: egui::PointerButton::Primary, pressed: false, modifiers: alt });
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::default()));
    h.run_steps(4);
    assert_eq!(wires(&h), ["in.Output > out.Input"]);
}

/// A folded node stays folded in the file.
#[test]
fn a_folded_node_is_kept_folded() {
    let (mut h, _dir) = two_nodes();
    let id = graph_id(&h, "in");
    h.state_mut().snarl.open_node(id, false);
    h.run_steps(4);
    shot(&mut h, "95-folded-node");
    let ws = h.state().workspace();
    let n = ws.nodes.iter().find(|n| n.id == "in").unwrap();
    assert!(n.collapsed);
    assert!(!ws.nodes.iter().find(|n| n.id == "out").unwrap().collapsed);
    assert!(ws.to_json().contains("\"collapsed\": true"));
}

/// Choosing the rectangle of a download shows a map of Sweden without an area over it, and
/// a rectangle that is set as its outline.
#[test]
fn the_map_to_choose_a_download_on_has_no_area_over_it_and_stays() {
    let dir = example_copy();
    std::fs::write(
        dir.join("osm.json"),
        r#"{"format": "ographis/1", "name": "OSM", "nodes": [{"id": "osm", "type": "OsmDownloader", "name": "Roads", "x": 0, "y": 0}], "edges": []}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("osm.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().start_area_pick("osm");
    h.run_steps(6);
    shot(&mut h, "95-choose-area");
    let t = h.state().area_table().unwrap();
    assert!(matches!(t.features[0].geom, Some(geo::Geometry::MultiPoint(_))));
    // Two corners, and one Enter takes the rectangle.
    let corner = place_of(&h, "3D", 0, false);
    let mid = egui::pos2(corner.x - 220.0, corner.y + 120.0);
    for d in [(-40.0, -30.0), (40.0, 30.0)] {
        click_at(&mut h, mid + egui::vec2(d.0, d.1));
        h.run_steps(1);
    }
    assert_eq!(h.state().map.measuring.as_ref().map(|m| m.points.len()), Some(2));
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let id = graph_id(&h, "osm");
    let bbox = h.state().snarl[id].params.get("bbox").and_then(|v| v.as_str()).unwrap_or("").to_string();
    assert_eq!(bbox.split(',').count(), 4, "{:?}", bbox);
    // The map stays, with the rectangle chosen, while the download is selected.
    h.state_mut().set_selection(vec![id]);
    h.run_steps(4);
    assert!(h.state().area_for.is_none());
    assert!(matches!(h.state().area_table().map(|t| t.features[0].geom.clone()), Some(Some(geo::Geometry::LineString(_)))));
    shot(&mut h, "95-area-chosen");
    h.state_mut().start_area_pick("osm");
    h.state_mut().snarl[id].params.insert("bbox".into(), serde_json::json!("11.9, 57.6, 12.1, 57.8"));
    h.run_steps(4);
    let t = h.state().area_table().unwrap();
    assert!(matches!(t.features[0].geom, Some(geo::Geometry::LineString(_))));
}

/// Points of a cloud are drawn in 3D in their own colours, and by class, height or
/// intensity when asked.
#[test]
fn points_are_drawn_in_their_own_colours_in_3d() {
    use og_engine::{Feature, Field, FieldType, Table, Value};
    let n = 40_000usize;
    let fields = ["classification", "intensity", "red", "green", "blue"].into_iter().map(|f| Field::new(f, FieldType::Int)).collect();
    let table = Table {
        fields,
        crs: Some(3006),
        features: (0..n)
            .map(|i| {
                let (x, y) = ((i % 200) as f64 * 0.5, (i / 200) as f64 * 0.5);
                // A red half and a blue half, ground and buildings, rising to the east.
                let east = x > 50.0;
                Feature {
                    attrs: vec![
                        Value::Int(if east { 6 } else { 2 }),
                        Value::Int((y * 300.0) as i64),
                        Value::Int(if east { 220 } else { 30 }),
                        Value::Int(40),
                        Value::Int(if east { 30 } else { 220 }),
                    ],
                    geom: Some(geo::Geometry::Point(geo::Point::new(400000.0 + x, 6400000.0 + y))),
                    z: Some(std::sync::Arc::from(vec![x / 10.0])),
                }
            })
            .collect(),
        ..Default::default()
    };
    let mut h = app_with_result(table);
    h.state_mut().focus_node("in");
    settle(&mut h);
    h.get_by_label("3D").click();
    h.run_steps(6);
    assert!(h.state().map.view3d.has_points());
    shot(&mut h, "95-points-own-colours");
    assert!(h.query_all_by_value("Points: their own colours").next().is_some() || h.query_by_label_contains("Points: their own colours").is_some());
    h.state_mut().map.view3d.points_by = "class".into();
    h.run_steps(4);
    shot(&mut h, "95-points-by-class");
}

/// Filter point cloud lists the classes of the points that came in, to tick, and gives each
/// class ticked a port of its own when asked.
#[test]
fn filter_point_cloud_ticks_the_classes_in_the_data() {
    let dir = example_copy();
    std::fs::write(
        dir.join("classes.json"),
        r#"{
      "format": "ographis/1", "name": "Classes",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Drone", "x": 0, "y": 0, "params": {"path": "pointcloud/drone_points.laz"}},
        {"id": "f", "type": "PointCloudFilter", "name": "Filter", "x": 350, "y": 0}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["f", "Points"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("classes.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    let f = graph_id(&h, "f");
    h.state_mut().set_selection(vec![f]);
    settle(&mut h);
    shot(&mut h, "95-classes-to-tick");
    // The labels of the ticks: class · name · count.
    let ticks: Vec<String> = h
        .query_all_by_role(egui::accesskit::Role::CheckBox)
        .filter_map(|n| {
            let d = format!("{:?}", n);
            let at = d.find(" · ")?;
            let start = d[..at].rfind('"')? + 1;
            let end = start + d[start..].find('"')?;
            Some(d[start..end].to_string())
        })
        .collect();
    assert!(!ticks.is_empty(), "no classes to tick");
    let first = ticks[0].clone();
    let class: i64 = first.split([' ', '·']).next().unwrap().parse().unwrap();
    h.get_by_label(&first).click();
    h.run_steps(3);
    let params = &h.state().snarl[f].params;
    assert_eq!(params.get("classes").and_then(|v| v.as_str()), Some(class.to_string().as_str()));
    // A port for the class, named by its number and name, without asking.
    assert_eq!(h.state().snarl[f].outputs(), vec![og_engine::nodes::class_port(class), "Other".to_string()]);
}

/// PostGIS output shows the names the table gets in the database, and warns of a password
/// written in the model.
#[test]
fn postgis_output_shows_the_names_in_the_database_and_warns_of_a_password() {
    let dir = example_copy();
    std::fs::write(
        dir.join("pg.json"),
        r#"{
      "format": "ographis/1", "name": "PG",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "pg", "type": "PostgisWriter", "name": "To the database", "x": 350, "y": 0,
         "params": {"format": "postgis", "host": "nowhere.invalid", "database": "og", "user": "me", "password": "secret", "table": "Towns 2026"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["pg", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("pg.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    let pg = graph_id(&h, "pg");
    h.state_mut().set_selection(vec![pg]);
    settle(&mut h);
    shot(&mut h, "95-postgis-names");
    assert!(h.query_by_label("Towns 2026 → towns_2026").is_some());
    assert!(h.query_by_label_contains("Written in the model").is_some());
    // og check says it too.
    let text = std::fs::read_to_string(dir.join("pg.json")).unwrap();
    assert!(og_engine::reference::check(&text).iter().any(|f| !f.error && f.message.contains("password written in the model")));
}

/// A PostGIS node with the server written in it saves it as a connection, and the seldom
/// changed settings wait under More, which is open when one of them is set.
#[test]
fn a_postgis_node_saves_its_server_as_a_connection_and_keeps_the_rest_under_more() {
    let dir = example_copy();
    std::fs::write(
        dir.join("pg.json"),
        r#"{
      "format": "ographis/1", "name": "PG",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "pg", "type": "PostgisWriter", "name": "To the database", "x": 350, "y": 0,
         "params": {"format": "postgis", "host": "db.invalid", "database": "og953", "user": "me", "password": "secret", "table": "towns"}},
        {"id": "pg2", "type": "PostgisWriter", "name": "With SQL after", "x": 350, "y": 200,
         "params": {"format": "postgis", "connection": "Somewhere", "table": "towns", "sql_after": "GRANT SELECT ON {table} TO readers"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["pg", "Input"]}, {"from": ["in", "Output"], "to": ["pg2", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("pg.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    let pg = graph_id(&h, "pg");
    h.state_mut().set_selection(vec![pg]);
    settle(&mut h);
    // Nothing under More is set, so it is closed.
    assert!(h.query_by_label("More").is_some());
    assert!(h.query_by_label("SQL before").is_none());
    h.get_by_label("More").scroll_to_me();
    settle(&mut h);
    h.get_by_label("More").click();
    settle(&mut h);
    shot(&mut h, "95-more-open");
    assert!(h.query_by_label("SQL before").is_some());
    shot(&mut h, "95-more-open");
    h.get_by_label("Save as a connection").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Save as a connection").click();
    settle(&mut h);
    assert_eq!(param(&h, "pg", "connection"), "og953 on db.invalid");
    assert_eq!(param(&h, "pg", "password"), "");
    assert_eq!(param(&h, "pg", "host"), "");
    let c = h.state().settings.connection("og953 on db.invalid").cloned().expect("the connection is in the settings");
    assert_eq!((c.host.as_str(), c.port.as_str(), c.user.as_str(), c.password.as_str()), ("db.invalid", "5432", "me", "secret"));
    assert!(h.query_by_label_contains("Written in the model").is_none());
    // The other has SQL after, so More is open and says so.
    let pg2 = graph_id(&h, "pg2");
    h.state_mut().set_selection(vec![pg2]);
    settle(&mut h);
    assert!(h.query_by_label("More · 1 set").is_some());
    assert!(h.query_by_label("SQL after").is_some());
}

/// Draw map for print says how many pixels wide the map becomes, before it is run.
#[test]
fn draw_map_for_print_says_how_wide_the_map_becomes() {
    let dir = example_copy();
    std::fs::write(
        dir.join("print.json"),
        r#"{
      "format": "ographis/1", "name": "Print",
      "nodes": [
        {"id": "map", "type": "MapRenderer", "name": "Map", "x": 0, "y": 0,
         "params": {"size_as": "print", "print_mm": "180", "print_dpi": "300"}}
      ],
      "edges": []
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("print.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    let map = graph_id(&h, "map");
    h.state_mut().set_selection(vec![map]);
    settle(&mut h);
    shot(&mut h, "95-print-pixels");
    let line = h.query_by_label_contains("pixels in width").expect("the width is shown");
    let text = format!("{:?}", line);
    assert!(text.contains("2") && text.contains("126"), "{}", text);
}
