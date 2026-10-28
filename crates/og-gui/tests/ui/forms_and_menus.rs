//! The palette of tools, the forms of their settings, the menus, the toolbar and the windows
//! that open over the program.

use crate::common::*;

#[test]
fn parameter_forms() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.state_mut().run_blocking();
    for (i, id) in ["skaraborg", "outside", "towns", "join", "out_outside"].iter().enumerate() {
        h.state_mut().focus_node(id);
        settle(&mut h);
        shot(&mut h, &format!("1{}-parameters-{}", i, id));
    }
    // The form must not have changed anything just by being shown.
    assert!(!h.state().dirty && !h.state().stale);
}

#[test]
fn palette_search_and_all_node_types() {
    let mut h = harness_for(App::new(), egui::Theme::Light);
    h.run_steps(4);
    let headings = [
        "Input and output",
        "3D models and views",
        "Attributes",
        "Charts",
        "Database",
        "Geometry",
        "JSON",
        "Maps",
        "Network",
        "Raster",
        "Terrain",
        "Web",
        "Workflow",
        "Annotations",
    ];
    let offered: Vec<&og_engine::NodeType> = og_engine::NODE_TYPES.iter().filter(|t| !t.hidden).collect();
    // From the start every list is folded in: the headings are there, the tools are not.
    for heading in headings {
        assert!(h.query_by_label(heading).is_some(), "{} is missing from the palette", heading);
    }
    // View is also the name of a menu, which is always there.
    for t in offered.iter().filter(|t| t.title != "View") {
        assert!(h.query_by_label(t.title).is_none(), "{} is shown although its list is folded in", t.title);
    }
    assert!(h.query_by_label("Note").is_none());
    shot(&mut h, "21-palette-folded-in");

    // A click on a heading folds its list out, and only that one.
    unfold(&mut h, "Geometry");
    assert!(h.query_by_label("Buffer").is_some() && h.query_by_label("Sort").is_none() && h.query_by_label("Input").is_none());
    for heading in headings {
        unfold(&mut h, heading);
    }
    shot(&mut h, "21-palette");
    // Every tool is in the list by the name that says what it does, and the names it has in
    // files are not shown.
    for t in &offered {
        assert!(h.query_all_by_label(t.title).count() >= 1, "{} is missing from the palette", t.title);
        assert!(t.title == t.name || h.query_by_label(t.name).is_none(), "{} is shown by its old name", t.name);
    }
    for (old, new) in [("AttributeManager", "Manage attributes"), ("Sorter", "Sort"), ("Bufferer", "Buffer"), ("Tester", "Test")] {
        assert_eq!(og_engine::node_type(old).unwrap().title, new);
    }
    // The tool that was replaced is not offered.
    assert!(h.query_by_label("Overlay points on areas").is_none() && h.query_by_label("PointOnAreaOverlayer").is_none());
    assert!(h.query_by_label("Count points in polygons").is_some() && h.query_by_label("Join by location").is_some());

    // Under each heading the names are in alphabetical order.
    let top = |h: &Harness<'_, App>, label: &str| h.get_by_label(label).rect().center().y;
    for (heading, category) in [("Attributes", "Attributes"), ("Geometry", "Geometry")] {
        let mut names: Vec<&str> = offered.iter().filter(|t| t.category == category).map(|t| t.title).collect();
        names.sort_by_key(|n| n.to_lowercase());
        let places: Vec<f32> = names.iter().map(|n| top(&h, n)).collect();
        assert!(places.windows(2).all(|w| w[0] < w[1]), "{:?} are not in alphabetical order: {:?}", names, places);
        assert!(top(&h, heading) < places[0]);
    }
    assert!(top(&h, "Input") < top(&h, "Output"));
    assert!(top(&h, "Input and output") < top(&h, "Attributes") && top(&h, "Geometry") < top(&h, "Web"));
    assert!(top(&h, "HTTP request") < top(&h, "Annotations"), "HTTP request is under Web");
    h.get_by_label("Annotations").scroll_to_me();
    h.run_steps(3);
    assert!(top(&h, "Group box") < top(&h, "Note"));

    // Another click folds the list in again.
    h.get_by_label("Attributes").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Attributes").click();
    h.run_steps(3);
    assert!(h.query_by_label("Sort").is_none() && h.query_by_label("Buffer").is_some());
    assert!(!h.state().open_sections.contains("Attributes") && h.state().open_sections.len() == 13);
    shot(&mut h, "21-palette-one-folded-in");

    // A search narrows the list, whatever is folded in. A word from a description finds
    // the tool too, and so does the name it had before.
    h.state_mut().filter = "buff".into();
    h.run_steps(3);
    assert!(h.query_by_label("Buffer").is_some() && h.query_by_label("Test").is_none() && h.query_by_label("Geometry").is_none());
    h.state_mut().filter = "sort".into();
    h.run_steps(3);
    assert!(h.query_by_label("Sort").is_some());
    h.state_mut().filter = "AttributeMan".into();
    h.run_steps(3);
    assert!(h.query_by_label("Manage attributes").is_some() && h.query_by_label("Sort").is_none());
    h.state_mut().filter = "spatial join".into();
    h.run_steps(3);
    assert!(h.query_by_label("Join by location").is_some() && h.query_by_label("Buffer").is_none());
    h.state_mut().filter = "PointOnArea".into();
    h.run_steps(3);
    assert!(h.query_by_label("Nothing matches.").is_some(), "the replaced tool is not found by a search either");
    h.state_mut().filter = "xyzzy".into();
    h.run_steps(3);
    assert!(h.query_by_label("Nothing matches.").is_some());
}

#[test]
fn about_shows_the_name_and_the_version() {
    let mut h = empty_app();
    assert!(h.query_by_label("Ographis").is_none(), "the name of the program is not written in the window itself");
    h.get_by_label("Help").click();
    h.run_steps(3);
    h.get_by_label("About Ographis").click();
    h.run_steps(3);
    assert!(h.state().about);
    // The version is shown the short way: 0.52 and not 0.52.0.
    let full = env!("CARGO_PKG_VERSION");
    let short = full.strip_suffix(".0").unwrap_or(full);
    assert!(h.query_by_label(&format!("Version {}", short)).is_some());
    assert_eq!(App::version(), short);
    assert!(short.split('.').count() == 2 || !full.ends_with(".0"), "{}", short);
    assert!(h.query_by_label("Ographis").is_some());
    shot(&mut h, "100-about");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(!h.state().about);
    assert!(h.query_by_label(&format!("Version {}", env!("CARGO_PKG_VERSION"))).is_none());
}

#[test]
fn the_toolbar_has_the_buttons_for_what_is_done_most() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    assert!(h.query_by_label("Ographis").is_none(), "the name of the program is not in the window");
    for button in ["New model", "Save", "Run", "Undo", "Redo"] {
        assert!(h.query_by_label(button).is_some(), "the toolbar should have {}", button);
    }
    // The buttons are on a row of their own, under the menus.
    let (menu, button) = (h.get_by_label("File").rect(), h.get_by_label("New model").rect());
    assert!(button.top() >= menu.bottom(), "{:?} should be under {:?}", button, menu);
    shot(&mut h, "130-toolbar");

    // Run runs.
    h.get_by_label("Run").click();
    for _ in 0..200 {
        h.run_steps(2);
        if h.state().result.is_some() && !h.state().is_running() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(h.state().result.as_ref().unwrap().nodes.len(), 18);

    // Save saves to the file the workspace was opened from.
    let file = h.state().path.clone().unwrap();
    h.state_mut().add_node("Sorter");
    h.run_steps(3);
    assert!(h.state().dirty);
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert!(!h.state().dirty);
    assert_eq!(og_engine::Workspace::load(&file).unwrap().nodes.len(), 19);

    // New model gives an empty workspace, and asks first if there is something to lose.
    h.get_by_label("New model").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 0);
    h.state_mut().add_node("Reader");
    h.run_steps(3);
    h.get_by_label("New model").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 1, "nothing is discarded before the user has answered");
    h.get_by_label("Don't save").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 0);
}

// ---------- Version 0.54 ----------

#[test]
fn the_toolbar_has_icons_and_the_menus_list_the_latest_models() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    // Icons, each with its name for the pointer and for screen readers, and no text.
    for name in ["New model", "Open", "Save", "Run", "Undo", "Redo"] {
        let b = h.get_by_label(name);
        assert!(b.rect().width() <= 34.0, "{} is an icon, not a button with text", name);
    }
    assert!(h.query_by_label("DATA SETS").is_some() && h.query_by_label("NAVIGATOR").is_none());

    // The model that was opened is first among the latest, and another one that is saved
    // comes before it.
    let first = h.state().path.clone().unwrap();
    assert_eq!(h.state().recent.list.first(), Some(&first));
    let other = first.with_file_name("other.json");
    h.state_mut().save_to(&other);
    assert_eq!(h.state().recent.list[..2], [other.clone(), first.clone()]);
    let stored = std::fs::read_to_string(og_gui::recent::settings_dir().unwrap().join("recent.json")).unwrap();
    assert!(stored.contains("other.json"), "the list is kept between sessions");
    assert_eq!(og_gui::recent::Recent::load().list[0], other);

    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label_contains("Open recent").hover();
    h.run_steps(4);
    assert!(h.query_by_label("1  other.json").is_some() && h.query_by_label("2  skaraborg.ographis").is_some());
    shot(&mut h, "160-open-recent");
    h.get_by_label("2  skaraborg.ographis").click();
    h.run_steps(4);
    assert_eq!(h.state().path.as_deref(), Some(first.as_path()));
    assert_eq!(h.state().recent.list[0], first, "the model that was opened goes first");
    // There are never more than five.
    for k in 0..6 {
        h.state_mut().save_to(&first.with_file_name(format!("copy_{}.json", k)));
    }
    assert_eq!(h.state().recent.list.len(), 5);
}

#[test]
fn the_canvas_menu_adds_an_input_an_output_or_a_tool_found_by_search() {
    let mut h = empty_app();
    let empty = h.state().canvas_rect().center() + egui::vec2(-200.0, -100.0);
    right_click_at(&mut h, empty);
    h.run_steps(3);
    for item in ["Add input", "Add output", "Add tool", "Add note", "Add group box"] {
        assert!(h.query_by_label_contains(item).is_some(), "the menu should have {}", item);
    }
    assert!(h.query_by_label("Buffer").is_none(), "the tools are not listed until Add tool is opened");
    shot(&mut h, "161-canvas-menu");
    h.get_by_label("Add input").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().map(|n| n.kind.clone()).collect::<Vec<_>>(), ["Reader"]);

    // Add tool opens a list with a search box.
    let empty = h.state().canvas_rect().center() + egui::vec2(150.0, 60.0);
    right_click_at(&mut h, empty);
    h.run_steps(3);
    h.get_by_label_contains("Add tool").hover();
    h.run_steps(4);
    assert!(h.query_by_label("Buffer").is_some() && h.query_by_label("Sort").is_some());
    h.event(egui::Event::Text("buff".into()));
    h.run_steps(3);
    assert!(h.query_by_label("Buffer").is_some() && h.query_by_label("Sort").is_none(), "the search narrows the list");
    shot(&mut h, "162-add-tool-search");
    h.get_by_label("Buffer").click();
    h.run_steps(3);
    let kinds: Vec<String> = h.state().snarl.nodes().map(|n| n.kind.clone()).collect();
    assert_eq!(kinds, ["Reader", "Bufferer"]);
    assert!(h.query_by_label_contains("Add tool").is_none(), "the menu closes");
}

#[test]
fn the_new_tools_are_in_the_list_with_their_settings() {
    let mut h = empty_app();
    unfold(&mut h, "Geometry");
    for tool in ["Aggregate", "Deaggregate", "Set coordinate system", "Filter by geometry type"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    assert!(h.query_by_label("Split by geometry type").is_none());
    h.state_mut().add_node("Generalizer");
    h.run_steps(3);
    assert!(h.query_by_label("Keep neighbours together").is_some());
    h.state_mut().add_node("CrsSetter");
    h.run_steps(3);
    assert!(h.query_by_label("Coordinate system").is_some());
    h.state_mut().add_node("GeometryFilter");
    h.run_steps(3);
    assert!(h.query_all_by_label("Raster").count() >= 1, "Filter by geometry type has an output for rasters");
}

#[test]
fn the_new_tools_and_settings_are_in_the_window() {
    let mut h = empty_app();
    unfold(&mut h, "Attributes");
    unfold(&mut h, "Geometry");
    unfold(&mut h, "Web");
    for tool in ["Keep attributes", "HTTP request", "Remove geometry"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    h.state_mut().add_node("Writer");
    h.run_steps(3);
    assert!(h.query_by_label("If it is already there").is_some());
    assert!(h.query_by_value("Overwrite it").is_some());
    h.state_mut().add_node("HttpCaller");
    h.run_steps(3);
    for label in ["Address", "Headers", "Response attribute", "Status attribute", "Timeout in seconds"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form", label);
    }
    // The Model menu has Run and Variables.
    h.get_by_label("Model").click();
    h.run_steps(3);
    assert!(h.query_by_label("Variables…").is_some() && h.query_all_by_label("Run").count() >= 1);
}

#[test]
fn the_tools_of_this_version_are_in_the_list() {
    let mut h = empty_app();
    unfold(&mut h, "Geometry");
    unfold(&mut h, "Maps");
    unfold(&mut h, "Terrain");
    for tool in ["Create points from x and y", "Draw map", "Tanaka contours"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    h.state_mut().add_node("PointCreator");
    h.run_steps(3);
    shot(&mut h, "187-create-points-form");
    let params = &h.state().snarl.nodes().last().unwrap().params;
    assert!(params.is_empty() || params.get("crs").is_some() || params.get("x").is_some());
    // A spreadsheet is written without a locked or bold top row, unless that is asked for.
    let defaults = og_engine::node_type("Writer").unwrap().params;
    for key in ["bold_header", "freeze"] {
        assert_eq!(defaults.iter().find(|p| p.key == key).map(|p| p.default), Some("false"), "{}", key);
    }
}

#[test]
fn the_tools_and_settings_of_this_version_are_in_the_window() {
    let mut h = empty_app();
    unfold(&mut h, "Geometry");
    unfold(&mut h, "Web");
    for tool in ["Geometry to 2D", "Scale", "Move by x, y and z", "HTTP request"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    h.state_mut().add_node("Scaler");
    h.run_steps(3);
    for label in ["Factor", "Factor in y", "Around"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of Scale", label);
    }
    h.state_mut().add_node("Offsetter");
    h.run_steps(3);
    assert!(h.query_by_label("Along x").is_some() && h.query_by_label("Along y").is_some());
    // The settings of a PDF are shown for a PDF only.
    h.state_mut().add_node("Writer");
    h.run_steps(3);
    assert!(h.query_by_label("Page size").is_none());
    let id = h.state().snarl.nodes().last().unwrap().id.clone();
    let n = graph_id(&h, &id);
    h.state_mut().snarl[n].params.insert("path".into(), serde_json::json!("out/map.pdf"));
    h.run_steps(3);
    for label in ["Page size", "Orientation", "Margin in mm", "Title"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of a PDF output", label);
    }
    assert!(h.query_by_label("If it is already there").is_none());
    shot(&mut h, "193-pdf-output");
    // The Model menu has Pause and Reuse what has not changed.
    h.get_by_label("Model").click();
    h.run_steps(3);
    assert!(h.query_by_label("Pause").is_some() && h.query_by_label("Reuse what has not changed").is_some());
}

// ---------- Version 0.58 ----------

#[test]
fn the_menus_follow_the_pointer_and_the_keyboard() {
    let mut h = empty_app();
    // Pointing at another menu while one is open opens that one instead.
    h.get_by_label("File").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Open recent").is_some());
    let edit = h.get_by_label("Edit").rect().center();
    h.event(egui::Event::PointerMoved(edit));
    h.run_steps(3);
    assert!(h.query_by_label_contains("Open recent").is_none(), "File closed");
    assert!(h.query_by_label("Add a note").is_some(), "Edit opened by pointing at it");
    shot(&mut h, "195-menu-by-pointing");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.query_by_label("Add a note").is_none(), "Esc closes the menu");
    // With nothing open, pointing opens nothing.
    let view = h.get_by_label("View").rect().center();
    h.event(egui::Event::PointerMoved(view));
    h.run_steps(3);
    assert!(h.query_by_label("Add a note").is_none() && h.query_by_label_contains("Open recent").is_none());

    // Alt and a letter opens a menu, and left and right go to the menus beside it.
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::M);
    h.run_steps(3);
    assert!(h.query_by_label("Reuse what has not changed").is_some(), "Alt+M opens Model");
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    assert!(h.query_by_label("About Ographis").is_some() && h.query_by_label("Reuse what has not changed").is_none(), "right goes to Help");
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Open recent").is_some(), "from the last menu, right goes round to File");
    h.key_press(egui::Key::ArrowLeft);
    h.run_steps(3);
    assert!(h.query_by_label("About Ographis").is_some(), "left goes back");
    // Down goes into the menu, and Enter chooses.
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(3);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(3);
    shot(&mut h, "196-menu-by-keys");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert!(h.state().about, "Down, Down and Enter in Help chooses About");
    h.get_by_label("Close").click();
    h.run_steps(3);
    // F10 opens the first menu, and closes it again.
    h.key_press(egui::Key::F10);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Open recent").is_some());
    h.key_press(egui::Key::F10);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Open recent").is_none());
}

#[test]
fn the_tools_of_0_58_are_in_the_list() {
    let mut h = empty_app();
    unfold(&mut h, "Attributes");
    unfold(&mut h, "Geometry");
    for tool in ["Count", "Replace with bounding box", "Bounding box of all", "Polygons to lines", "Lines to polygons", "Extend lines", "Thiessen polygons"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    h.state_mut().add_node("Counter");
    h.run_steps(3);
    for label in ["Attribute", "Start at", "Start again for each"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of Count", label);
    }
    h.state_mut().add_node("VoronoiDiagrammer");
    h.run_steps(3);
    assert!(h.query_all_by_label("Boundary").count() >= 1 && h.query_by_label("Margin in %").is_some());
    h.state_mut().add_node("LineExtender");
    h.run_steps(3);
    assert!(h.query_by_label("Distance").is_some() && h.query_by_value("Both ends").is_some());
    shot(&mut h, "199-new-tools");
}

#[test]
fn the_tools_of_0_59_are_in_the_list() {
    let mut h = empty_app();
    unfold(&mut h, "Attributes");
    unfold(&mut h, "Geometry");
    for tool in ["Map values", "Find nearest", "Split lines", "Overlay areas", "Create grid"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    h.state_mut().add_node("ValueMapper");
    h.run_steps(3);
    for label in ["Attribute", "Values", "Write to", "Values not in the list", "Add value", "Use the values in the data"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of Map values", label);
    }
    assert!(h.query_by_label("Text for those").is_none(), "only when the rest get a text");
    h.get_by_label("Add value").click();
    h.run_steps(3);
    let node = h.state().selected[0];
    assert_eq!(h.state().snarl[node].params.get("map"), Some(&serde_json::json!([["", ""]])));
    shot(&mut h, "205-map-values");
    h.state_mut().add_node("GridCreator");
    h.run_steps(3);
    assert!(h.query_by_label("Shape").is_some() && h.query_by_label("Start at whole sizes").is_some());
}

// ---------- Version 0.61 ----------

#[test]
fn the_forms_of_0_61() {
    let mut h = empty_app();
    // Since 0.62 PostGIS and the PDF with layers are reached from Input and Output.
    unfold(&mut h, "Input and output");
    for item in ["PostGIS input", "PDF with layers", "GeoSpatial PDF output", "PostGIS output"] {
        assert!(h.query_by_label(item).is_none(), "{} should not be in the list", item);
    }
    unfold(&mut h, "Attributes");
    unfold(&mut h, "Geometry");
    unfold(&mut h, "Raster");
    unfold(&mut h, "Terrain");
    for tool in ["Check attributes", "Take first", "Check geometry", "Extract coordinates from point", "Merge rasters", "Contour lines"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    // PostGIS input: the connection, a password that is not shown, and a button for the tables.
    h.state_mut().add_node("PostgisReader");
    h.run_steps(3);
    for label in ["Connection", "Tables", "Only rows where", "SQL query"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of PostGIS input", label);
    }
    // Since 0.63 the connection is chosen by name; the fields of a model from 0.61 are shown
    // only when it has them.
    assert!(h.query_by_label("Server").is_none());
    let node = h.state().selected[0];
    h.state_mut().snarl[node].params.insert("database".into(), serde_json::json!("gistest"));
    h.state_mut().snarl[node].params.insert("user".into(), serde_json::json!("gis"));
    h.state_mut().snarl[node].params.insert("password".into(), serde_json::json!("gis-test"));
    h.run_steps(3);
    assert!(h.query_by_label("Server").is_some() && h.query_by_label("Password").is_some());
    assert!(h.query_by_value("gis-test").is_none(), "the password is not shown");
    h.get_by_label("Connect and list the tables").click();
    h.run_steps(4);
    // With the test database there, its tables can be ticked. Without it, the form says why.
    if h.query_by_label("vgr.subregions").is_some() {
        h.get_by_label("vgr.subregions").click();
        h.run_steps(3);
        assert_eq!(h.state().snarl[node].params.get("tables"), Some(&serde_json::json!(["vgr.subregions"])));
    }
    shot(&mut h, "210-postgis-form");
    // Sort: rows of attributes, each with its order.
    h.state_mut().add_node("Sorter");
    h.run_steps(3);
    assert!(h.query_by_label("Add attribute").is_some());
    // Check attributes: rows with what each attribute must be.
    h.state_mut().add_node("AttributeValidator");
    h.run_steps(3);
    h.get_by_label("Add a check").click();
    h.run_steps(3);
    for label in ["Must have a value", "Longest", "Smallest", "Largest", "One of"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in a check", label);
    }
    shot(&mut h, "211-check-attributes");
    h.state_mut().add_node("LayeredPdfWriter");
    h.run_steps(3);
    assert!(h.query_all_by_label("Layers").count() >= 1 && h.query_by_label("Page size").is_some());
}

#[test]
fn the_forms_of_0_62() {
    let mut h = empty_app();
    unfold(&mut h, "Charts");
    unfold(&mut h, "Attributes");
    unfold(&mut h, "Geometry");
    unfold(&mut h, "Raster");
    for tool in ["Draw chart", "Classify by range", "Snap", "Merge rasters"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    h.state_mut().add_node("ChartDrawer");
    h.run_steps(3);
    for label in
        ["Names", "Values", "Order", "Title", "Explanation", "Note at the foot", "Unit", "Numbers on the bars", "Line at the mean", "Bars to point out"]
    {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of Draw chart", label);
    }
    assert!(h.query_by_label("Features with the same name").is_none(), "counting has nothing to put together");
    assert!(h.query_by_label("Second colour").is_none(), "no bars are pointed out");
    shot(&mut h, "223-chart-form");
    h.state_mut().add_node("RangeMapper");
    h.run_steps(3);
    assert!(h.query_by_label("From, to and value").is_some() && h.query_by_label("Number of classes").is_none());
    h.state_mut().add_node("Snapper");
    h.run_steps(3);
    assert!(h.query_by_label("Largest distance").is_some() && h.query_by_label("Snap to").is_some());
}

#[test]
fn the_tools_and_forms_of_0_63() {
    let mut h = empty_app();
    unfold(&mut h, "JSON");
    for tool in ["Flatten JSON", "Explode JSON list", "Extract JSON values", "Build JSON"] {
        assert!(h.query_by_label(tool).is_some(), "{} should be in the list", tool);
    }
    h.state_mut().add_node("JsonExtractor");
    h.run_steps(3);
    assert!(h.query_by_label("Values to take").is_some() && h.query_by_label("Add value").is_some());
    h.get_by_label("Add value").click();
    h.run_steps(3);
    shot(&mut h, "234-extract-json");
    // An input of a JSON file shows where the records are.
    h.state_mut().add_node("Reader");
    h.run_steps(3);
    let reader = h.state().selected[0];
    h.state_mut().snarl[reader].params.insert("path".into(), serde_json::json!("data.json"));
    h.run_steps(3);
    assert!(h.query_by_label("Records at (JSON)").is_some());
    // About says when the program was built.
    h.state_mut().about = true;
    h.run_steps(3);
    assert!(h.query_by_label_contains("Built 20").is_some());
}

#[test]
fn manage_attributes_sets_types_and_empty_attributes() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    h.state_mut().focus_node("outside");
    settle(&mut h);
    // Every attribute shows its type, in a list to change it.
    assert!(h.query_all_by_value("text").count() >= 1 || h.query_all_by_value("integer").count() >= 1);
    h.get_by_label("Add attribute").click();
    h.run_steps(3);
    assert!(h.query_by_value("as it comes").is_some());
    shot(&mut h, "241-manage-attribute-types");
}

#[test]
fn an_expression_shows_its_value_and_the_functions() {
    let dir = example_copy();
    std::fs::write(
        dir.join("expr.ographis"),
        r#"{"format": "ographis/1", "name": "Expressions", "nodes": [
            {"id": "towns", "type": "Reader", "name": "Towns", "x": 20, "y": 20, "params": {"path": "towns.csv", "crs": "EPSG:4326"}},
            {"id": "calc", "type": "AttributeCreator", "name": "Size", "x": 300, "y": 20, "params": {"name": "size", "expression": "CASE WHEN population > 20000 THEN 'large' ELSE 'small' END || ' ' || $id"}},
            {"id": "join", "type": "SpatialJoiner", "name": "Zones", "x": 300, "y": 200, "params": {"relation": "near", "distance": "0.1", "mode": "summary"}}
        ], "edges": [
            {"from": ["towns", "Output"], "to": ["calc", "Input"]},
            {"from": ["towns", "Output"], "to": ["join", "Features"]},
            {"from": ["towns", "Output"], "to": ["join", "Join"]}
        ]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("expr.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("calc");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    // The value for the first town, worked out as the expression is written.
    // Göteborg is first, with 600 000 inhabitants.
    assert!(h.query_by_label("First feature: large 1").is_some(), "the value of the first feature should be shown");
    h.get_by_label("Functions and variables").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("regexp_replace(text, pattern").is_some());
    assert!(h.query_by_label_contains("$perimeter is the perimeter").is_some());
    shot(&mut h, "300-expression-help");
    h.state_mut().focus_node("join");
    settle(&mut h);
    for label in ["Features that", "Distance", "When several are found", "Summarize numbers as"] {
        assert!(h.query_by_label(label).is_some(), "{label} should be in the form");
    }
    shot(&mut h, "301-join-by-location");
}

// ---------- Version 0.88 ----------

/// While an expression is written, the attributes and functions that begin with the word at
/// the cursor are suggested; Tab takes one. Settings → General turns it off.
#[test]
fn expressions_get_suggestions_while_they_are_written() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("skaraborg");
    h.run_steps(4);
    let field = h.get_all_by_value("subregion = 'Skaraborg'").last().unwrap().rect();
    click_at(&mut h, field.right_bottom() - egui::vec2(6.0, 6.0));
    h.key_press(egui::Key::End);
    h.run_steps(1);
    h.event(egui::Event::Text(" OR subr".into()));
    h.run_steps(3);

    assert!(h.query_all_by_label_contains("Tab or Enter takes it").next().is_some(), "the list is shown");
    shot(&mut h, "088-expression-suggestions");
    h.key_press(egui::Key::Tab);
    h.run_steps(3);
    assert_eq!(param(&h, "skaraborg", "test"), "subregion = 'Skaraborg' OR subregion");
    // A function, with its parenthesis.
    h.event(egui::Event::Text(" = low".into()));
    h.run_steps(3);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(param(&h, "skaraborg", "test"), "subregion = 'Skaraborg' OR subregion = lower(");
    // Turned off, nothing is suggested.
    h.state_mut().settings.other.insert("suggestions".into(), serde_json::json!(false));
    h.event(egui::Event::Text("'X') OR subr".into()));
    h.run_steps(3);
    assert!(h.query_all_by_label_contains("Tab or Enter takes it").next().is_none());
}
