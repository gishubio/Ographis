//! What came in 0.96.

use crate::common::*;
use og_gui::App;

/// An output that has written its file can show it in its folder and open it.
#[test]
fn an_output_opens_what_it_wrote() {
    let dir = example_copy();
    std::fs::write(
        dir.join("open.json"),
        r#"{
      "format": "ographis/1", "name": "Open",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "out", "type": "Writer", "name": "Towns out", "x": 350, "y": 0, "params": {"path": "out/towns_open.csv"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["out", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("open.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    let out = graph_id(&h, "out");
    h.state_mut().set_selection(vec![out]);
    settle(&mut h);
    // Nothing is written before the run.
    assert!(h.query_by_label("Open folder").is_none());
    let opens = h.query_all_by_label("Open").count();
    h.state_mut().run_blocking();
    settle(&mut h);
    assert!(h.query_by_label("Open folder").is_some());
    assert_eq!(h.query_all_by_label("Open").count(), opens + 1, "one file, so it can be opened");
    h.get_by_label("Open folder").scroll_to_me();
    settle(&mut h);
    shot(&mut h, "96-open-what-was-written");
    h.get_by_label("Open folder").click();
    settle(&mut h);
    assert!(h.state().shown.last().is_some_and(|p| p.ends_with("towns_open.csv") || p.ends_with("out")), "{:?}", h.state().shown);

    // Help → Report a problem saves a zip with the model, without its secrets, to attach.
    h.state_mut().open_report();
    settle(&mut h);
    h.get_by_label("Save a zip to attach").click();
    settle(&mut h);
    let zip = h.state().shown.last().cloned().unwrap();
    assert!(zip.extension().is_some_and(|e| e == "zip") && zip.exists(), "{}", zip.display());
}

/// Every character the interface writes has a glyph in the fonts, also where the fonts of
/// Windows are not there, as on Mac and Linux: no boxes for → or ⚠.
#[test]
fn every_character_the_interface_writes_has_a_glyph() {
    // The characters outside ASCII in the texts of the program: what is between quotes on the
    // lines that are not comments.
    let mut chars = std::collections::BTreeSet::new();
    let mut files = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(path) = files.pop() {
        if path.is_dir() {
            files.extend(std::fs::read_dir(&path).unwrap().flatten().map(|e| e.path()));
        } else if path.extension().is_some_and(|e| e == "rs") {
            for line in std::fs::read_to_string(&path).unwrap().lines() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                let mut quoted = false;
                for c in code.chars() {
                    if c == '"' {
                        quoted = !quoted;
                    } else if quoted && !c.is_ascii() {
                        chars.insert(c);
                    }
                }
            }
        }
    }
    assert!(chars.contains(&'→') && chars.contains(&'å'), "{:?}", chars);
    // Each kind of text looks in its fonts in turn; one of them must have the character.
    let defs = og_gui::theme::fonts();
    for (family, list) in &defs.families {
        let faces: Vec<ttf_parser::Face> =
            list.iter().filter_map(|name| defs.font_data.get(name)).filter_map(|d| ttf_parser::Face::parse(&d.font, d.index).ok()).collect();
        let missing: String = chars.iter().filter(|c| !faces.iter().any(|f| f.glyph_index(**c).is_some())).collect();
        assert!(missing.is_empty(), "{:?} has no glyph for {:?}", family, missing);
    }
}

/// Nodes are made into a tool, which is opened with a double click, changed inside, closed
/// with Escape, run and unpacked again into a box of nodes.
#[test]
fn nodes_become_a_tool_that_opens_and_unpacks() {
    let dir = example_copy();
    std::fs::write(
        dir.join("tool.json"),
        r#"{
      "format": "ographis/1", "name": "Tools",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "size", "type": "AttributeCreator", "name": "Size", "x": 300, "y": 0, "params": {"name": "big", "expression": "population > 50000"}},
        {"id": "keep", "type": "Tester", "name": "Big ones", "x": 600, "y": 0, "params": {"test": "big = 1"}},
        {"id": "out", "type": "Writer", "name": "Big towns", "x": 900, "y": 0, "params": {"path": "out/big_towns.csv"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["size", "Input"]}, {"from": ["size", "Output"], "to": ["keep", "Input"]},
                {"from": ["keep", "Passed"], "to": ["out", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("tool.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    let (size, keep) = (graph_id(&h, "size"), graph_id(&h, "keep"));
    h.state_mut().make_tool_of(&[size, keep]);
    settle(&mut h);
    let ws = h.state().workspace();
    assert_eq!(ws.nodes.len(), 3);
    assert!(ws.edges.iter().any(|e| e.from.0 == "in" && e.to == ("new_tool".to_string(), "Towns".to_string())), "{:?}", ws.edges);
    assert!(h.query_by_label("Unpack").is_some(), "the form of the tool");
    shot(&mut h, "96-a-tool-made-of-nodes");

    // A double click opens it: the nodes inside and a path back above the canvas.
    let p = place_of(&h, "New tool", 0, true);
    double_click(&mut h, p);
    settle(&mut h);
    assert!(h.state().in_tool(), "opened by a double click");
    assert_eq!(h.state().snarl.nodes().count(), 4, "two nodes and a port each way");
    assert!(h.query_by_label("Back").is_some());
    shot(&mut h, "96-inside-a-tool");
    // A change inside, and the whole model is what is saved and run.
    let keep = graph_id(&h, "keep");
    h.state_mut().snarl.get_node_mut(keep).unwrap().params.insert("test".into(), serde_json::json!("big = 1 and population > 100000"));
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(!h.state().in_tool(), "Escape goes back out");
    let ws = h.state().workspace();
    let inner = og_engine::nodes::inner_model(&ws.nodes.iter().find(|n| n.id == "new_tool").unwrap().params).unwrap();
    assert!(inner.nodes.iter().any(|n| n.params.get("test").and_then(|v| v.as_str()) == Some("big = 1 and population > 100000")));
    h.state_mut().run_blocking();
    settle(&mut h);
    let written = std::fs::read_to_string(dir.join("out/big_towns.csv")).unwrap();
    assert!(written.contains("Göteborg") && !written.contains("Borås"), "{}", written);
    // Undo takes back what was changed inside as one step.
    h.state_mut().undo();
    let ws = h.state().workspace();
    let inner = og_engine::nodes::inner_model(&ws.nodes.iter().find(|n| n.id == "new_tool").unwrap().params).unwrap();
    assert!(inner.nodes.iter().any(|n| n.params.get("test").and_then(|v| v.as_str()) == Some("big = 1")));

    // Unpacked, the nodes are back in a box with the name of the tool.
    h.state_mut().unpack_tool("new_tool");
    settle(&mut h);
    let ws = h.state().workspace();
    assert_eq!(ws.nodes.len(), 4);
    assert_eq!(ws.groups.last().unwrap().name, "New tool");
    let pairs: Vec<(String, String)> = ws.edges.iter().map(|e| (e.from.0.clone(), e.to.0.clone())).collect();
    for (a, b) in [("in", "size"), ("size", "keep"), ("keep", "out")] {
        assert!(pairs.contains(&(a.into(), b.into())), "{:?}", pairs);
    }
}

/// With the pointer on a connection, what went through it in the latest run is shown.
#[test]
fn a_connection_tells_what_goes_through_it() {
    let mut h = harness_with("buildtest.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    // The connection out of Reproject: looked for to the right of the name of its port.
    let out = place_of(&h, "Reprojected", 0, true);
    let mut found = false;
    'look: for dx in (20..=90).step_by(10) {
        for dy in (-20..=30).step_by(4) {
            pointer(&mut h, out + egui::vec2(dx as f32, dy as f32), None);
            if h.query_all_by_label_contains("features").next().is_some() {
                found = true;
                break 'look;
            }
        }
    }
    shot(&mut h, "96-what-goes-through-a-connection");
    assert!(found, "a count on the connection");
}

/// A variable marked Ask is asked for before a run, and the value given is the one used.
#[test]
fn a_variable_is_asked_for_before_the_run() {
    let dir = example_copy();
    std::fs::write(
        dir.join("ask.json"),
        r#"{
      "format": "ographis/1", "name": "Ask",
      "variables": [{"name": "least", "value": "50000", "ask": true}],
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "big", "type": "Tester", "name": "Big", "x": 300, "y": 0, "params": {"test": "population > $(least)"}},
        {"id": "out", "type": "Writer", "name": "Big towns", "x": 600, "y": 0, "params": {"path": "out/asked.csv"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["big", "Input"]}, {"from": ["big", "Passed"], "to": ["out", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("ask.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.key_press(egui::Key::F5);
    settle(&mut h);
    assert!(h.state().asking.is_some(), "asked before the run");
    assert!(!h.state().is_running());
    shot(&mut h, "96-asked-before-the-run");
    h.state_mut().asking.as_mut().unwrap()[0].value = "300000".into();
    h.query_all_by_label("Run").last().unwrap().click();
    for _ in 0..40 {
        settle(&mut h);
        if !h.state().is_running() && h.state().result.is_some() {
            break;
        }
    }
    assert!(h.state().asking.is_none());
    assert_eq!(h.state().variables[0].value, "300000", "kept for the next time");
    let written = std::fs::read_to_string(dir.join("out/asked.csv")).unwrap();
    assert!(written.contains("Göteborg") && !written.contains("Borås"), "{}", written);
}

/// The other files of a zip file are written one on each row, with a new empty row below the
/// last one filled in.
#[test]
fn the_files_of_a_zip_file_get_a_row_each() {
    let mut h = harness_for(App::new(), egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().add_node("ZipWriter");
    settle(&mut h);
    let zip = h.state().selected[0];
    let rows = |h: &Harness<'_, App>| h.query_all_by_label("File…").count();
    assert_eq!(rows(&h), 1, "an empty row to start with");
    // The text field on the row of the button.
    let button = h.get_by_label("File…").rect();
    let field =
        h.query_all_by_role(egui::accesskit::Role::TextInput).find(|f| (f.rect().center().y - button.center().y).abs() < 6.0).expect("a field on the row");
    field.click();
    h.run_steps(2);
    h.query_all_by_role(egui::accesskit::Role::TextInput).find(|f| (f.rect().center().y - button.center().y).abs() < 6.0).unwrap().type_text("README.txt");
    settle(&mut h);
    assert_eq!(rows(&h), 2, "a new row below the one filled in");
    shot(&mut h, "96-files-of-a-zip-file");
    let params = &h.state().snarl.get_node(zip).unwrap().params;
    assert_eq!(params.get("extra"), Some(&serde_json::json!(["README.txt"])));
}

/// Many attributes removed at once in Manage attributes leave the form as wide as it was: the
/// lists in each row keep to the edge, and the panel does not grow over the model.
#[test]
fn many_attributes_removed_at_once_keep_the_form_its_width() {
    let dir = example_copy();
    let names: Vec<String> = (0..40).map(|i| format!("BaseQuantities.GrossSideArea_{i}")).collect();
    let mut csv = format!("name;{};wkt\n", names.join(";"));
    for row in 0..3 {
        let values: Vec<String> = (0..40).map(|i| format!("{}.5", row * 100 + i)).collect();
        csv.push_str(&format!("Door {row};{};POINT ({} 6400000)\n", values.join(";"), 400000 + row * 10));
    }
    std::fs::write(dir.join("doors.csv"), csv).unwrap();
    std::fs::write(
        dir.join("doors.json"),
        r#"{
      "format": "ographis/1", "name": "Doors",
      "nodes": [
        {"id": "in", "type": "Reader", "x": 0, "y": 0, "params": {"path": "doors.csv", "crs": "EPSG:3006"}},
        {"id": "manage", "type": "AttributeManager", "x": 400, "y": 0, "params": {}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["manage", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("doors.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("manage");
    settle(&mut h);
    let edge = |h: &Harness<'_, App>| h.get_all_by_label("Attributes").map(|n| n.rect().left()).fold(f32::MIN, f32::max);
    let untouched = edge(&h);
    let id = egui::Id::new(("attributes chosen", "manage", "attributes"));
    let mut all = vec!["name".to_string()];
    all.extend(names.iter().cloned());
    h.ctx.data_mut(|d| d.insert_temp(id, all));
    h.run_steps(3);
    let left = |h: &Harness<'_, App>| h.get_by_label("41 ticked:").rect().left();
    let before = left(&h);
    h.get_all_by_label("Remove").next().unwrap().click();
    h.run_steps(3);
    let rules = h.state().snarl.nodes().find(|n| n.id == "manage").unwrap().params["attributes"].clone();
    assert_eq!(rules.as_array().unwrap().iter().filter(|r| r["action"] == "remove").count(), 41);
    // Frame after frame the form stays where it was.
    for _ in 0..40 {
        h.run_steps(1);
        assert!((edge(&h) - untouched).abs() < 1.0, "the panel grew from {} to {}", untouched, edge(&h));
    }
    shot(&mut h, "96-many-attributes-removed");
    let after = left(&h);
    assert!((after - before).abs() < 1.0, "the form moved from {before} to {after}");
    // Each row ends at the same edge, inside the window.
    let rights: Vec<f32> = h.get_all_by_value("decimal").map(|n| n.rect().right()).collect();
    assert!(rights.len() >= 10, "{} type lists", rights.len());
    let (lo, hi) = rights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
    assert!(hi - lo < 1.0 && hi <= h.ctx.content_rect().right(), "the rows end between {lo} and {hi}");
    // A wider panel has the lists on the line of the name.
    let panel = egui::Id::new("params");
    let mut state = egui::containers::panel::PanelState::load(&h.ctx, panel).expect("the panel has a size");
    state.outer_rect.min.x -= 260.0;
    h.ctx.data_mut(|d| d.insert_persisted(panel, state));
    h.run_steps(4);
    shot(&mut h, "96-many-attributes-wide");
    let name_y = h.get_all_by_label("name").max_by(|a, b| a.rect().left().total_cmp(&b.rect().left())).unwrap().rect().center().y;
    let list_y = h.get_all_by_value("text").next().unwrap().rect().center().y;
    assert!((name_y - list_y).abs() < 3.0, "the lists are beside the name: {name_y} and {list_y}");
}

/// Outline from above in 3D shows the model that comes in, with the height of the cut as a
/// see-through plane.
#[test]
fn the_cut_of_outline_from_above_is_a_plane_in_3d() {
    let dir = example_copy();
    let data = root().join("crates/og-engine/tests/data");
    for f in ["house.gltf", "house.bin"] {
        std::fs::copy(data.join(f), dir.join(f)).unwrap();
    }
    std::fs::write(
        dir.join("cut.json"),
        r#"{
      "format": "ographis/1", "name": "Cut",
      "nodes": [
        {"id": "house", "type": "Reader", "x": 0, "y": 0, "params": {"path": "house.gltf"}},
        {"id": "plan", "type": "OutlineFromAbove", "name": "Plan", "x": 300, "y": 0, "params": {"cut": "1.5", "height_from": "absolute"}}
      ],
      "edges": [{"from": ["house", "Output"], "to": ["plan", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("cut.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("plan");
    h.state_mut().map.three_d = true;
    settle(&mut h);
    h.run_steps(4);
    shot(&mut h, "96-outline-cut-plane");
    let planes = h.state().map.view3d.planes.clone();
    assert!(!planes.is_empty(), "a plane for the cut");
    // In the heights of the model: one plane, at 1.5 m.
    assert_eq!(planes.len(), 1, "{:?}", planes);
    assert!((planes[0].0 - 1.5).abs() < 1e-6, "{:?}", planes);
    // Turned off in the tool: the model alone.
    h.state_mut().snarl.nodes_mut().find(|n| n.id == "plan").unwrap().params.insert("plane_3d".into(), serde_json::json!("false"));
    h.run_steps(4);
    assert!(h.state().map.view3d.planes.is_empty(), "no plane when it is turned off");
}

/// A start folder in the settings is where a model that is not saved yet reads and writes,
/// and the settings show it.
#[test]
fn a_start_folder_is_where_an_unsaved_model_stands() {
    let dir = scratch("start-folder");
    let mut h = empty_app();
    h.state_mut().settings.other.insert("start_folder".into(), serde_json::json!(dir.to_string_lossy()));
    h.run_steps(2);
    assert_eq!(h.state().base_dir(), dir);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    assert!(h.query_by_label("Start folder").is_some());
    assert!(h.query_by_label(&dir.to_string_lossy()).is_some(), "the folder is shown");
    shot(&mut h, "96-start-folder");
    // A folder that is not there counts as none.
    h.state_mut().settings.other.insert("start_folder".into(), serde_json::json!(dir.join("gone").to_string_lossy()));
    assert_ne!(h.state().base_dir(), dir.join("gone"));
}

/// Gaussian splats made from a coloured point cloud are drawn soft in 3D, together with the
/// buildings of a map in a View.
#[test]
fn gaussian_splats_are_seen_in_3d_with_map_data() {
    let dir = example_copy();
    // A green hill of points, 40 × 40 m, with a red top, as a drone might give, in its own
    // axes with z up, placed by the input in SWEREF 99 TM.
    let mut ply = String::from(
        "ply\nformat ascii 1.0\nelement vertex 3600\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n",
    );
    for i in 0..60 {
        for j in 0..60 {
            let (x, y) = (i as f64 * 0.66 - 20.0, j as f64 * 0.66 - 20.0);
            let z = 8.0 * (-(x * x + y * y) / 150.0).exp();
            let red = if z > 6.0 { 220 } else { 60 };
            ply.push_str(&format!("{:.2} {:.2} {:.2} {} {} 60\n", x, y, z, red, 170 - red / 3));
        }
    }
    std::fs::write(dir.join("hill.ply"), ply).unwrap();
    std::fs::write(dir.join("blocks.csv"), "name;wkt\nHouse;POLYGON ((418290 6472540, 418300 6472540, 418300 6472548, 418290 6472548, 418290 6472540))\n")
        .unwrap();
    std::fs::write(
        dir.join("splats.json"),
        r#"{
      "format": "ographis/1", "name": "Splats",
      "nodes": [
        {"id": "hill", "type": "Reader", "name": "Hill", "x": 0, "y": 0, "params": {"path": "hill.ply", "splat_up": "z", "obj_east": "418280", "obj_north": "6472530", "obj_height": "80", "crs": "EPSG:3006"}},
        {"id": "make", "type": "SplatMaker", "name": "Splats", "x": 260, "y": 0, "params": {"size": "1.4"}},
        {"id": "blocks", "type": "Reader", "name": "Blocks", "x": 0, "y": 160, "params": {"path": "blocks.csv", "crs": "EPSG:3006"}},
        {"id": "high", "type": "HeightSetter", "name": "Up", "x": 260, "y": 160, "params": {"height": "86"}},
        {"id": "view", "type": "Viewer", "name": "Together", "x": 520, "y": 60, "params": {}}
      ],
      "edges": [{"from": ["hill", "Output"], "to": ["make", "Points"]}, {"from": ["blocks", "Output"], "to": ["high", "Input"]},
        {"from": ["make", "Splats"], "to": ["view", "Input"]}, {"from": ["high", "Output"], "to": ["view", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("splats.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    // The splats alone.
    h.state_mut().focus_node("make");
    settle(&mut h);
    h.get_by_label("3D").click();
    h.run_steps(4);
    let (n, pixels) = h.state().map.view3d.splats_shown();
    assert_eq!(n, 3600);
    assert!(pixels > 2000, "the splats cover the hill: {} pixels", pixels);
    shot(&mut h, "96-splats-in-3d");
    // With the map data in the View: the splats and the house together.
    h.state_mut().focus_node("view");
    settle(&mut h);
    h.run_steps(4);
    let (n, pixels) = h.state().map.view3d.splats_shown();
    assert_eq!(n, 3600, "the splats are drawn among the streams of the View");
    assert!(pixels > 2000);
    shot(&mut h, "96-splats-with-map-data");
}

/// A picture draped over a terrain model shows in the 3D view in its own colours, where each
/// triangle lies on it, and not as one colour for the whole model.
#[test]
fn a_picture_draped_over_a_terrain_shows_in_3d() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("blender.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("model");
    h.state_mut().map.three_d = true;
    settle(&mut h);
    h.run_steps(4);
    shot(&mut h, "96-terrain-with-picture-3d");
    let colours = h.state().map.view3d.model_colours();
    assert!(!colours.is_empty(), "the model is drawn");
    // The shaded relief in its many colours, lighter and darker, and not one for all.
    let mut seen: Vec<[u32; 3]> = colours.iter().map(|c| c.map(|v| v.round() as u32)).collect();
    seen.sort_unstable();
    seen.dedup();
    assert!(seen.len() > 100, "{} colours", seen.len());
}

/// A model that an assistant changes on disk is read again in the window when it has no
/// changes there; the window tells what it has open, and opens a model it is asked to.
#[test]
fn a_model_changed_by_an_assistant_is_shown_and_the_open_one_is_told() {
    let dir = example_copy();
    let model = |n: usize| {
        let nodes: Vec<String> =
            (0..n).map(|k| format!(r#"{{"id": "n{}", "type": "Reader", "x": {}, "y": 0, "params": {{"path": "towns.csv"}}}}"#, k, k * 260)).collect();
        format!(r#"{{"format": "ographis/1", "name": "Built", "nodes": [{}]}}"#, nodes.join(","))
    };
    let file = dir.join("built.ographis");
    std::fs::write(&file, model(1)).unwrap();
    let mut app = App::new();
    app.load(&file).unwrap();
    app.map.set_tile_source(NO_TILES);
    app.quiet = true;
    app.shared.when_quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    assert_eq!(h.state().snarl.nodes().count(), 1);
    // The assistant adds a node; a second later the window has it.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&file, model(2)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    h.run_steps(4);
    assert_eq!(h.state().snarl.nodes().count(), 2, "read again");
    // What is open is told, and a model asked for is opened.
    let open = og_engine::session::open_models();
    assert!(open.iter().any(|m| m.path.as_deref() == Some(file.as_path()) && m.model.contains("n1")), "{:?}", open.iter().map(|m| &m.path).collect::<Vec<_>>());
    let other = dir.join("other.ographis");
    std::fs::write(&other, model(3)).unwrap();
    assert!(og_engine::session::ask_to_open(&other));
    // The window looks for requests every second; under a full run of the tests it may take
    // longer, and a window of another test in this process may take the request first, so it
    // is asked again now and then.
    for k in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        h.run_steps(2);
        if k % 10 == 9 && h.state().path.as_deref() != Some(other.as_path()) {
            og_engine::session::ask_to_open(&other);
        }
        if h.state().path.as_deref() == Some(other.as_path()) {
            break;
        }
    }
    assert_eq!(h.state().path.as_deref(), Some(other.as_path()), "opened when asked");
    assert_eq!(h.state().snarl.nodes().count(), 3);
}

#[test]
fn a_run_does_not_change_the_size_of_the_nodes() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("lines.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    let ids: Vec<String> = h.state().workspace().nodes.iter().map(|n| n.id.clone()).collect();
    let before: Vec<(String, egui::Rect)> = ids.iter().filter_map(|id| h.state().node_rect(id).map(|r| (id.clone(), r))).collect();
    assert!(!before.is_empty());
    shot(&mut h, "sizes_before_run");
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx);
    wait_for_run(&mut h);
    h.run_steps(4);
    shot(&mut h, "sizes_after_run");
    for (id, r) in &before {
        let now = h.state().node_rect(id).unwrap();
        assert!((now.width() - r.width()).abs() < 0.5 && (now.height() - r.height()).abs() < 0.5, "{}: {:?} became {:?}", id, r.size(), now.size());
    }
    // A table with room for all its attributes has no buttons to scroll it.
    h.state_mut().focus_node(&ids[0]);
    h.run_steps(4);
    assert!(h.query_by_label("◀").is_none() && h.query_by_label("▶").is_none());
}

#[test]
fn the_arrow_keys_scroll_the_table_once_it_is_clicked_and_buttons_do_too() {
    use og_engine::{Feature, Field, FieldType, Table, Value};
    // Many attributes, more than the table shows at once.
    let fields: Vec<Field> = (0..40).map(|k| Field::new(format!("attribute_number_{:02}", k), FieldType::Text)).collect();
    let features = (0..5)
        .map(|i| Feature {
            z: None,
            attrs: (0..40).map(|k| Value::Text(format!("value {} {}", i, k))).collect(),
            geom: Some(geo::Geometry::Point(geo::Point::new(300000. + i as f64, 6400000.))),
        })
        .collect();
    let table = Table { mesh: None, points: None, series: None, fields, crs: Some(3006), features, raster: false, image: None, grid: None };
    let mut h = app_with_result(table);
    h.run_steps(4);
    assert!(h.state().table_scroll.most() > 100.0, "{:?}", h.state().table_scroll);
    let node = h.state().node_rect("in").unwrap();
    // A click on a row gives the table the keys.
    h.get_by_label("value 0 0").click();
    h.run_steps(2);
    assert_eq!(h.state().view.feature, Some(0));
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    let moved = h.state().table_scroll.offset;
    assert!(moved > 50.0, "{:?}", h.state().table_scroll);
    assert_eq!(h.state().node_rect("in").unwrap(), node, "the node stays");
    shot(&mut h, "table_scrolled");
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(3);
    assert_eq!(h.state().view.feature, Some(1), "the next row");
    // The buttons scroll too.
    h.get_by_label("◀").click();
    h.run_steps(3);
    assert!(h.state().table_scroll.offset < moved, "{:?}", h.state().table_scroll);
    // A click on the canvas gives the keys back to the nodes.
    let empty = on_canvas(&h, 600.0, 500.0);
    click_at(&mut h, empty);
    h.run_steps(2);
    h.state_mut().focus_node("in");
    h.run_steps(2);
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    assert!(h.state().node_rect("in").unwrap().left() > node.left(), "the node moves again");
}

/// A long error is cut beside the buttons of the map, and does not push the panel under the
/// canvas over the settings of the node.
#[test]
fn a_long_error_does_not_widen_the_panels() {
    let dir = example_copy();
    let long = format!("{}/missing.gpkg", "a_folder_with_a_very_long_name_that_goes_on_and_on".repeat(8));
    let model = format!(r#"{{"format": "ographis/1", "nodes": [{{"id": "in", "type": "Reader", "x": 0, "y": 0, "params": {{"path": "{}"}}}}]}}"#, long);
    std::fs::write(dir.join("long_error.ographis"), model).unwrap();
    let mut app = App::new();
    app.load(&dir.join("long_error.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(3);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx);
    wait_for_run(&mut h);
    h.state_mut().focus_node("in");
    h.run_steps(2);
    h.get_by_label("Data").click();
    h.run_steps(4);
    shot(&mut h, "96-long-error");
    let button = h.get_all_by_label("Zoom to data").next().expect("the buttons of the map").rect();
    let settings = h.get_by_label("Comment").rect();
    assert!(button.right() <= settings.left(), "{:?} reaches into {:?}", button, settings);
}

/// A few features with millions of corners, such as the borders of a whole country, are
/// drawn as an image and do not overfill the graphics card.
#[test]
fn features_with_very_many_corners_are_drawn_as_an_image() {
    use og_engine::{Feature, Field, FieldType, Table, Value};
    let ring = |cx: f64, n: usize| {
        let mut pts: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                let r = 1000.0 + 30.0 * (a * 400.0).sin();
                (cx + r * a.cos(), 6_400_000.0 + r * a.sin())
            })
            .collect();
        pts.push(pts[0]);
        geo::Geometry::Polygon(geo::Polygon::new(geo::LineString::from(pts), vec![]))
    };
    let features = (0..3).map(|k| Feature { z: None, attrs: vec![Value::Int(k)], geom: Some(ring(300_000.0 + k as f64 * 2500.0, 600_000)) }).collect();
    let table = Table { fields: vec![Field::new("kommun", FieldType::Int)], crs: Some(3006), features, ..Default::default() };
    let mut h = app_with_result(table);
    h.state_mut().focus_node("in");
    for _ in 0..20 {
        h.run_steps(2);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    shot(&mut h, "96-many-corners");
    assert_eq!(h.state().map.in_view, Some((3, true)), "drawn as an image");
}

/// The help of a setting shows once when the pointer rests on its name, not twice.
#[test]
fn the_help_of_a_setting_shows_once() {
    let dir = example_copy();
    std::fs::write(
        dir.join("help.ographis"),
        r#"{"format": "ographis/1", "nodes": [{"id": "g", "type": "GeodataFetcher", "x": 0, "y": 0, "params": {"bbox": "1, 2, 3, 4"}}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("help.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.state_mut().focus_node("g");
    h.run_steps(3);
    let name = h.get_by_label("Largest area, km²").rect();
    let at = name.left_center() + egui::vec2(4.0, 0.0);
    h.event(egui::Event::PointerMoved(at));
    for _ in 0..12 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    let shown = h.query_all_by_label_contains("A larger area is not fetched").count();
    assert_eq!(shown, 1, "the help shows once");
}

/// No value (NULL) is an empty cell, and an empty text is '' so that they are told apart.
#[test]
fn no_value_is_an_empty_cell_and_an_empty_text_is_quoted() {
    use og_engine::{Feature, Field, FieldType, Table, Value};
    let features = vec![
        Feature { z: None, attrs: vec![Value::Text("first".into()), Value::Null], geom: None },
        Feature { z: None, attrs: vec![Value::Text(String::new()), Value::Text("-".into())], geom: None },
    ];
    let table = Table { fields: vec![Field::new("name", FieldType::Text), Field::new("note", FieldType::Text)], features, ..Default::default() };
    let mut h = app_with_result(table);
    h.state_mut().focus_node("in");
    h.run_steps(4);
    assert!(h.query_by_label("first").is_some());
    assert!(h.query_by_label("''").is_some(), "an empty text");
    assert!(h.query_by_label("-").is_some(), "a dash is a dash");
    assert!(h.query_by_label("–").is_none(), "no value is not drawn as a dash");
    // How many rows are picked.
    assert!(h.query_by_label_contains("row selected").is_none() && h.query_by_label_contains("rows selected").is_none());
    h.get_by_label("first").click();
    h.run_steps(3);
    h.get_by_label("1 row selected");
    h.state_mut().view.more = vec![1];
    h.run_steps(3);
    h.get_by_label("2 rows selected");
}

/// Try one request sends only the first request of HTTP request and shows the answer as a
/// tree, whose values give their paths to the node.
#[test]
fn one_request_is_tried_and_its_paths_go_into_the_node() {
    let body = br#"{"data": [{"id": 7, "name": "Alpha", "kind": "town"}], "meta": {"nextCursor": "c-2"}, "links": [{"rel": "self", "href": "a"}, {"rel": "next", "href": "b"}]}"#;
    let base = file_server(vec![("/api", "application/json", body.to_vec())]);
    let dir = example_copy();
    std::fs::write(
        dir.join("try.ographis"),
        format!(
            r#"{{"format": "ographis/1", "nodes": [{{"id": "web", "type": "HttpCaller", "x": 0, "y": 0, "params": {{"url": "'{}/api?q=' || 'x'"}}}}]}}"#,
            base
        ),
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("try.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.state_mut().focus_node("web");
    h.run_steps(3);
    h.get_by_label("Try one request").click();
    for _ in 0..50 {
        h.run_steps(2);
        if h.state().try_answer.as_ref().is_some_and(|t| t.got.is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    h.run_steps(3);
    assert!(h.state().try_answer.is_some(), "{:?}", h.state().message());
    let got = h.state().try_answer.as_ref().unwrap().got.clone().unwrap().unwrap();
    // A value is scrolled to before it is clicked, as the tree may be taller than the window.
    let pick = |h: &mut Harness<'_, App>, label: &str| {
        h.get_by_label_contains(label).scroll_to_me();
        h.run_steps(3);
        h.get_by_label_contains(label).click();
        h.run_steps(3);
    };
    assert_eq!(got.status, 200);
    assert!(got.url.ends_with("/api?q=x"), "the address is worked out: {}", got.url);
    assert!(h.query_by_label("200").is_some());
    // A value deep in the answer: the cursor.
    pick(&mut h, "nextCursor");
    h.get_by_label("Cursor at").click();
    h.run_steps(3);
    let param =
        |h: &Harness<'_, App>, id: &str, key: &str| h.state().snarl.nodes().find(|n| n.id == id).and_then(|n| n.params.get(key).cloned()).unwrap_or_default();
    assert_eq!(param(&h, "web", "cursor_path"), "meta.nextCursor");
    assert_eq!(param(&h, "web", "paging"), "cursor");
    // A link among links is told by its rel.
    pick(&mut h, "[1]");
    pick(&mut h, "href  \"b\"");
    h.get_by_label("Next page at").click();
    h.run_steps(3);
    assert_eq!(param(&h, "web", "next_path"), "links[rel=next].href");
    // A list can be where the items are; a value can be taken out after the node.
    pick(&mut h, "data  […]");
    h.get_by_label("Items at").click();
    h.run_steps(3);
    assert_eq!(param(&h, "web", "items_path"), "data");
    pick(&mut h, "[0]  {…} 3 keys");
    pick(&mut h, "name  \"Alpha\"");
    shot(&mut h, "096-try-one-request");
    h.get_by_label("Extract from JSON").click();
    h.run_steps(3);
    pick(&mut h, "id  7");
    h.get_by_label("Extract from JSON").click();
    h.run_steps(3);
    let extract: Vec<_> = h.state().snarl.nodes().filter(|n| n.kind == "JsonExtractor").collect();
    assert_eq!(extract.len(), 1, "the second value goes to the same Extract JSON values");
    assert_eq!(extract[0].params.get("attribute").cloned().unwrap_or_default(), "_response");
    assert_eq!(extract[0].params.get("paths").cloned().unwrap_or_default(), serde_json::json!([["data[0].name", ""], ["data[0].id", ""]]));
    assert_eq!(h.state().snarl.wires().count(), 1, "connected after HTTP request");
}

/// Test the connection, among the API connections in the settings, asks the test address.
#[test]
fn an_api_connection_is_tested() {
    let base = file_server(vec![("/ok", "application/json", b"{}".to_vec())]);
    let mut h = empty_app();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    h.get_by_label("API connections").click();
    h.run_steps(3);
    h.get_by_label("New API connection").click();
    h.run_steps(3);
    let n = h.state().settings.apis.len() - 1;
    let wait = |h: &mut Harness<'_, App>| {
        for _ in 0..50 {
            h.run_steps(2);
            if !h.state().settings_window.as_ref().unwrap().busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        h.run_steps(2);
        h.state().settings_window.as_ref().unwrap().api_tested.clone().unwrap().1
    };
    h.get_by_label("Test the connection").click();
    assert!(wait(&mut h).unwrap_err().contains("Write a test address"));
    {
        let c = &mut h.state_mut().settings.apis[n];
        c.secret = "abcdef".into();
        c.test_url = format!("{}/ok", base);
    }
    h.run_steps(2);
    h.get_by_label("Test the connection").click();
    let said = wait(&mut h).unwrap();
    assert!(said.ends_with("/ok answered 200."), "{}", said);
    assert!(h.query_by_label_contains("answered 200").is_some());
    shot(&mut h, "096-test-api-connection");
    h.state_mut().settings.apis[n].test_url = format!("{}/gone", base);
    h.run_steps(2);
    h.get_by_label("Test the connection").click();
    assert!(wait(&mut h).unwrap_err().contains("answered 404"));
    h.get_by_label("Remove").click();
    h.run_steps(2);
}

/// The error of a node can be copied, with the name of the node, to paste in a mail.
#[test]
fn an_error_is_copied_from_the_form() {
    let dir = example_copy();
    std::fs::write(
        dir.join("broken.ographis"),
        r#"{"format": "ographis/1", "nodes": [{"id": "in", "type": "Reader", "x": 0, "y": 0, "params": {"path": "not-there.csv"}}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("broken.ographis")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.key_press(egui::Key::F5);
    wait_for_run(&mut h);
    h.state_mut().focus_node("in");
    h.run_steps(4);
    // The form is taller than the window: the button is scrolled to.
    h.get_by_label("Copy").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Copy").click();
    h.run_steps(2);
    shot(&mut h, "096-copy-error");
    assert_eq!(h.state().message(), Some("The error is copied."));
}

/// Settings exported for a colleague hold no secrets, and are taken in by name.
#[test]
fn settings_go_to_another_computer_without_secrets() {
    use og_engine::settings::{Connection, Settings};
    let file = scratch("settings-export").join("ographis-settings.json");
    let mine = Settings {
        connections: vec![Connection { name: "GIS".into(), host: "db".into(), user: "magnus".into(), password: "pw-123".into(), ..Default::default() }],
        ..Default::default()
    };
    let said = og_gui::settings::export_settings(&mine, &file).unwrap();
    assert!(said.starts_with("1 connection or map written"), "{}", said);
    assert!(!std::fs::read_to_string(&file).unwrap().contains("pw-123"));
    let mut theirs = Settings::default();
    let said = og_gui::settings::import_settings(&mut theirs, &file).unwrap();
    assert_eq!(said, "1 added, 0 put in place of those with the same name. Write the password or key of 1 under their tabs.");
    assert_eq!(theirs.connections[0].host, "db");
    std::fs::write(&file, "{not json").unwrap();
    assert!(og_gui::settings::import_settings(&mut theirs, &file).unwrap_err().contains("is not a file of settings"));
    // The section is in the General tab.
    let mut h = empty_app();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    assert!(h.query_by_label("Export settings…").is_some() && h.query_by_label("Import settings…").is_some());
}
