//! Files: dropped, read, written, saved and restored, layers and sheets, the navigator, and
//! databases.

use crate::common::*;

#[test]
fn dropped_files_become_readers_or_open_a_workspace() {
    let dir = example_copy();
    let mut app = App::new();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);

    // While files are held over the window it says what will happen.
    h.input_mut().hovered_files.push(egui::HoveredFile { path: Some(dir.join("towns.csv")), mime: String::new() });
    h.run_steps(2);
    shot(&mut h, "90-drop-hint");
    h.input_mut().hovered_files.clear();

    // A CSV file and a Shapefile with its companions: two readers, not five.
    drop_files(&mut h, &[dir.join("towns.csv"), dir.join("routes.shp"), dir.join("routes.dbf"), dir.join("routes.prj"), dir.join("routes.cpg")]);
    let app = h.state();
    let readers: Vec<(String, String, String)> =
        app.snarl.nodes().map(|n| (n.kind.clone(), n.name.clone(), n.params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string())).collect();
    assert_eq!(readers.len(), 2, "{:?}", readers);
    assert_eq!((readers[0].0.as_str(), readers[0].1.as_str()), ("Reader", "towns"));
    assert_eq!((readers[1].0.as_str(), readers[1].1.as_str()), ("Reader", "routes"));
    // The workspace has not been saved, so there is no folder to be relative to.
    assert_eq!(Path::new(&readers[0].2), dir.join("towns.csv"));
    assert_eq!(app.message(), Some("Added 2 inputs"));
    assert_eq!(selected(&h).len(), 2);
    assert!(app.dirty);
    // The two nodes do not lie on top of each other.
    let (a, b) = (app.node_rect("reader_1"), app.node_rect("reader_2"));
    assert!(a.is_some() && b.is_some() && !a.unwrap().intersects(b.unwrap()), "{:?} {:?}", a, b);

    // They read their files.
    h.state_mut().run_blocking();
    h.state_mut().focus_node("reader_2");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0);
    assert_eq!((r.output("reader_1", "Output").unwrap().len(), r.output("reader_2", "Output").unwrap().len()), (39, 5));
    shot(&mut h, "91-dropped-files");

    // Only a companion file: nothing is added, and the message says why.
    drop_files(&mut h, &[dir.join("routes.prj")]);
    assert_eq!(h.state().snarl.nodes().count(), 2);
    assert!(h.state().message().unwrap().contains("Drop the .shp file"));

    // Once the workspace is saved next to the data, a dropped file is stored by its
    // relative path, so that the folder can be moved.
    h.state_mut().save_to(&dir.join("mine.json"));
    drop_files(&mut h, &[dir.join("subregions.gpkg")]);
    let app = h.state();
    let node = app.snarl.nodes().find(|n| n.name == "subregions").expect("a reader named after the file");
    assert_eq!(node.params.get("path").and_then(|v| v.as_str()), Some("subregions.gpkg"));
    assert_eq!(selected(&h), std::slice::from_ref(&node.id));

    // A dropped workspace is opened, after the question about unsaved changes.
    drop_files(&mut h, &[dir.join("skaraborg.ographis")]);
    assert_eq!(h.state().snarl.nodes().count(), 3, "nothing is discarded before the user has answered");
    assert!(h.query_by_label("Save the changes?").is_some());
    h.get_by_label("Don't save").click();
    h.run_steps(6);
    assert_eq!(h.state().snarl.nodes().count(), 18);
    assert_eq!(h.state().name, "Towns and routes in Skaraborg");
    assert!(!h.state().dirty);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_and_unsaved_changes_dialog() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    let dir = std::env::temp_dir().join(format!("og-gui-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("saved.json");

    add_from_palette(&mut h, "Attributes", "Test");
    assert!(h.state().dirty);
    h.state_mut().save_to(&file);
    assert!(!h.state().dirty);
    let saved = og_engine::Workspace::load(&file).unwrap();
    assert_eq!(saved.nodes.len(), 19);
    assert_eq!(saved.name, "Towns and routes in Skaraborg");
    assert_eq!(saved.format, "ographis/1");

    // Ctrl+N with unsaved changes asks first.
    add_from_palette(&mut h, "Input and output", "Output");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 20, "nothing is discarded before the user has answered");
    shot(&mut h, "70-unsaved");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 20);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    h.run_steps(3);
    h.get_by_label("Don't save").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 0);
    assert!(!h.state().dirty);
    assert_eq!(h.state().name, "Untitled model");
    shot(&mut h, "71-empty");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_with_several_layers_asks_which_to_read() {
    let dir = scratch("layers");
    let b = GdalBackend::new();
    let file = dir.join("many.gpkg");
    for (k, name) in ["north", "south", "east"].iter().enumerate() {
        b.write(&points(k as i64 * 10, 2), &file, &WriteOpts { layer: Some(name.to_string()), ..Default::default() }, &mut Notes::default()).unwrap();
    }
    let mut h = empty_app();

    // Dropping the file adds nothing yet. It asks first.
    drop_files(&mut h, std::slice::from_ref(&file));
    assert_eq!(h.state().snarl.nodes().count(), 0);
    assert!(h.query_by_label("many.gpkg holds 3 layers").is_some());
    h.get_by_label("north").click();
    h.run_steps(2);
    h.get_by_label("east").click();
    h.run_steps(3);
    shot(&mut h, "101-layer-choice");
    h.get_by_label("Add 2 inputs").click();
    h.run_steps(4);
    assert_eq!(param_of(&h, "layer"), [("east".to_string(), r#"["east"]"#.to_string()), ("north".to_string(), r#"["north"]"#.to_string())]);
    assert_eq!(selected(&h).len(), 2, "the new readers are selected");
    assert!(h.state().dirty);

    // Cancel adds nothing.
    drop_files(&mut h, std::slice::from_ref(&file));
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 2);
    assert!(h.state().layer_choices.is_empty());

    // All layers with one reader: one stream.
    drop_files(&mut h, std::slice::from_ref(&file));
    h.get_by_label("Tick all").click();
    h.run_steps(3);
    h.get_by_label("One input for all of them, as one stream").click();
    h.run_steps(3);
    h.get_by_label("Add 1 input for 3 layers").click();
    h.run_steps(4);
    assert_eq!(param_of(&h, "layer")[1], ("many".to_string(), r#"["north","south","east"]"#.to_string()));
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let id = h.state().snarl.nodes().find(|n| n.name == "many").unwrap().id.clone();
    assert_eq!(r.output(&id, "Output").unwrap().len(), 6);
    assert_eq!(selected(&h), std::slice::from_ref(&id));

    // The form of the reader has the same layers as ticks. Unticking one changes what is read.
    assert!(h.query_by_label("All 3 layers").is_some());
    shot(&mut h, "102-layer-ticks");
    h.get_by_label("south").click();
    h.run_steps(3);
    assert_eq!(param_of(&h, "layer")[1].1, r#"["north","east"]"#);
    assert!(h.state().stale, "the choice of layers changes the result");
    h.state_mut().run_blocking();
    assert_eq!(h.state().result.as_ref().unwrap().output(&id, "Output").unwrap().len(), 4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_is_one_reader_for_many_files() {
    let dir = scratch("folder");
    let b = GdalBackend::new();
    std::fs::create_dir_all(dir.join("areas")).unwrap();
    for k in 0..30 {
        b.write(&points(k * 3, 3), &dir.join(format!("areas/area_{:02}.shp", k + 1)), &WriteOpts::default(), &mut Notes::default()).unwrap();
    }
    let mut h = empty_app();
    drop_files(&mut h, &[dir.join("areas")]);
    settle(&mut h);

    // One node for the thirty files, and it says so.
    let nodes: Vec<(String, String)> = h.state().snarl.nodes().map(|n| (n.kind.clone(), n.name.clone())).collect();
    assert_eq!(nodes, [("Reader".to_string(), "areas".to_string())]);
    assert_eq!(param_of(&h, "pattern")[0].1, "", "Shapefiles are the default, so no filter is stored");
    assert!(h.query_by_label("30 files").is_some(), "the node shows how many files it stands for");
    assert!(h.query_by_label("30 files are read as one stream").is_some(), "and so does its form");
    assert!(h.query_by_label("Files in the folder").is_some() && h.query_by_label("Include subfolders").is_some());

    // The file name as an attribute: a tick fills in a name, which can be changed.
    assert!(h.query_by_label("File name as").is_none());
    h.get_by_label("Add the file name as an attribute").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Add the file name as an attribute").click();
    h.run_steps(3);
    assert_eq!(param_of(&h, "filename_attr")[0].1, "\"file_name\"");
    let field = h.get_all_by_value("file_name").last().unwrap().rect().center();
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("source".into()));
    h.run_steps(3);
    assert_eq!(param_of(&h, "filename_attr")[0].1, "\"source\"");
    assert!(h.query_by_label("File name as").is_some(), "how the name is written is asked for once there is an attribute for it");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let id = h.state().snarl.nodes().next().unwrap().id.clone();
    let t = r.output(&id, "Output").unwrap();
    assert_eq!((t.len(), t.field_names()), (90, vec!["id".to_string(), "code".to_string(), "source".to_string()]));
    assert_eq!(t.features[89].attrs[2], Value::Text("area_30".into()));
    assert_eq!(r.nodes[&id].sources.len(), 30);
    shot(&mut h, "103-folder-reader");

    // A folder of CSV files gets a filter for them.
    std::fs::create_dir_all(dir.join("tables")).unwrap();
    for name in ["a.csv", "b.csv", "c.csv"] {
        std::fs::write(dir.join("tables").join(name), "id;name\n1;x\n").unwrap();
    }
    std::fs::write(dir.join("tables/readme.txt"), "about the tables\n").unwrap();
    drop_files(&mut h, &[dir.join("tables")]);
    settle(&mut h);
    assert_eq!(param_of(&h, "pattern"), [("areas".to_string(), String::new()), ("tables".to_string(), "\"*.csv\"".to_string())]);
    assert!(h.query_by_label("3 files").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_navigator_lists_what_is_read_and_written() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.state_mut().quiet = true;
    h.run_steps(6);
    assert!(h.query_by_label("Inputs (4)").is_some() && h.query_by_label("Outputs (4)").is_some());

    // A click on a reader in the tree selects it on the canvas and folds out what it reads.
    assert!(h.query_by_value("routes.shp").is_none());
    h.get_all_by_label("Routes").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(selected(&h), ["routes"]);
    assert!(h.query_all_by_value("routes.shp").count() >= 1, "the path is there to be read and changed");
    assert!(h.query_by_label("5 features · SWEREF 99 TM (EPSG:3006)").is_none());

    h.state_mut().run_blocking();
    h.run_steps(3);
    assert!(h.query_by_label("5 features · SWEREF 99 TM (EPSG:3006)").is_some(), "after a run the tree says what came out");
    // The tree is longer than its part of the panel, so the writer is scrolled to first.
    h.get_all_by_label("Towns in Skaraborg").next().unwrap().scroll_to_me();
    h.run_steps(3);
    h.get_all_by_label("Towns in Skaraborg").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(selected(&h), ["out_towns"]);
    for line in ["layer towns", "12 features written"] {
        assert!(h.query_by_label(line).is_some(), "the tree should say: {}", line);
    }
    assert!(h.query_all_by_value("out/skaraborg.gpkg").count() >= 1);
    shot(&mut h, "109-navigator");

    // A reader without a file says so.
    h.state_mut().add_node("Reader");
    h.run_steps(3);
    assert!(h.query_by_label("Inputs (5)").is_some());
    h.get_all_by_label("Input").next().unwrap().scroll_to_me();
    h.run_steps(3);
    h.get_all_by_label("Input").next().unwrap().click();
    h.run_steps(3);
    assert!(h.query_by_label("no file chosen").is_some());
}

#[test]
fn paths_are_changed_and_shown_from_the_navigator() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.state_mut().quiet = true;
    h.run_steps(6);
    let dir = h.state().base_dir();

    // The path of a reader is changed in the tree, without going to the node.
    h.get_all_by_label("Routes").next().unwrap().click();
    h.run_steps(3);
    let field = h.get_all_by_value("routes.shp").next().unwrap().rect().center();
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("towns.csv".into()));
    h.run_steps(3);
    assert_eq!(param(&h, "routes", "path"), "towns.csv");
    assert!(h.state().dirty);
    shot(&mut h, "120-navigator-path");
    let empty = on_canvas(&h, 600.0, -40.0);
    click_at(&mut h, empty);
    h.state_mut().run_blocking();
    h.run_steps(3);
    assert_eq!(h.state().result.as_ref().unwrap().output("routes", "Output").unwrap().len(), 39, "the reader now reads the other file");

    // The menu of a row shows the file in the file manager. The test only notes what would be shown.
    let row = h.get_all_by_label("Towns").next().unwrap().rect().center();
    right_click_at(&mut h, row);
    h.get_by_label("Show in folder").click();
    h.run_steps(3);
    assert_eq!(h.state().shown, [dir.join("towns.csv")]);
    // A writer whose file is not written yet shows the nearest folder that is there.
    assert_eq!(og_gui::app::folder_to_show(&dir.join("not/yet/there.gpkg")), Some((dir.clone(), false)));
    assert_eq!(og_gui::app::folder_to_show(&dir.join("towns.csv")), Some((dir.join("towns.csv"), true)));
    assert_eq!(og_gui::app::folder_to_show(&dir), Some((dir.clone(), false)));
}

#[test]
fn the_order_in_the_navigator_is_changed_by_dragging() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    // The rows of the tree from the top, by where they are drawn.
    let rows = |h: &Harness<'_, App>, names: &[&str]| -> Vec<String> {
        let mut at: Vec<(f32, String)> = names.iter().map(|n| (h.get_all_by_label(n).next().unwrap().rect().center().y, n.to_string())).collect();
        at.sort_by(|a, b| a.0.total_cmp(&b.0));
        at.into_iter().map(|a| a.1).collect()
    };
    let inputs = ["Municipalities", "Routes", "Subregions", "Towns"];
    assert_eq!(rows(&h, &inputs), inputs, "by name to begin with");
    assert!(h.state().workspace().navigator.is_empty() && !h.state().dirty);

    // Towns is dragged to the top.
    let from = h.get_all_by_label("Towns").next().unwrap().rect().center();
    let to = h.get_all_by_label("Municipalities").next().unwrap().rect().center() - egui::vec2(0.0, 8.0);
    drag(&mut h, from, to, egui::PointerButton::Primary);
    h.run_steps(3);
    assert_eq!(rows(&h, &inputs), ["Towns", "Municipalities", "Routes", "Subregions"]);
    assert!(h.state().dirty, "the order is part of the workspace");
    // Municipalities is dragged to the bottom.
    let from = h.get_all_by_label("Municipalities").next().unwrap().rect().center();
    let to = h.get_all_by_label("Subregions").next().unwrap().rect().center() + egui::vec2(0.0, 9.0);
    drag(&mut h, from, to, egui::PointerButton::Primary);
    h.run_steps(3);
    assert_eq!(rows(&h, &inputs), ["Towns", "Routes", "Subregions", "Municipalities"]);
    shot(&mut h, "121-navigator-order");
    // A row that is let go of where it was stays.
    let from = h.get_all_by_label("Routes").next().unwrap().rect().center();
    drag(&mut h, from, from + egui::vec2(3.0, 5.0), egui::PointerButton::Primary);
    h.run_steps(3);
    assert_eq!(rows(&h, &inputs), ["Towns", "Routes", "Subregions", "Municipalities"]);

    // The order is saved with the workspace, the writers after the readers.
    let ws = h.state().workspace();
    assert_eq!(ws.navigator[..4], ["towns", "routes", "subregions", "municipalities"]);
    assert_eq!(ws.navigator.len(), 8);
    let file = scratch("order").join("ordered.json");
    h.state_mut().save_to(&file);
    let mut again = App::new();
    again.load(&file).unwrap();
    assert_eq!(again.navigator, ws.navigator);
    // And it is undone like any other change.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(rows(&h, &inputs), ["Towns", "Municipalities", "Routes", "Subregions"]);
}

#[test]
fn excel_sheets_are_chosen_like_layers() {
    let dir = example_copy();
    // A workbook with two sheets, written by the program itself.
    let ws = og_engine::Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "towns", "type": "Reader", "params": {"path": "towns.csv"}},
        {"id": "municipalities", "type": "Reader", "params": {"path": "municipalities.csv"}},
        {"id": "w1", "type": "Writer", "params": {"path": "book.xlsx", "layer": "Towns"}},
        {"id": "w2", "type": "Writer", "params": {"path": "book.xlsx", "layer": "Municipalities"}}
      ],
      "edges": [
        {"from": ["towns", "Output"], "to": ["w1", "Input"]},
        {"from": ["municipalities", "Output"], "to": ["w2", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = og_engine::run(&ws, Some(&GdalBackend::new()), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);

    let mut h = empty_app();
    drop_files(&mut h, &[dir.join("book.xlsx")]);
    assert!(h.query_by_label("book.xlsx holds 2 sheets").is_some());
    h.get_by_label("Municipalities").click();
    h.run_steps(3);
    h.get_by_label("Add 1 input").click();
    h.run_steps(4);
    assert_eq!(param_of(&h, "layer"), [("Municipalities".to_string(), r#"["Municipalities"]"#.to_string())]);

    // The form of a reader of a workbook: sheets as ticks, and whether the first row has the names.
    for label in ["Sheets", "All 2 sheets", "Towns", "First row has names", "Add the sheet name as an attribute"] {
        assert!(h.query_by_label(label).is_some(), "the form should have: {}", label);
    }
    assert!(h.query_by_label("Coordinate system").is_none() && h.query_by_label("X column").is_none());
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let id = h.state().snarl.nodes().next().unwrap().id.clone();
    let t = r.output(&id, "Output").unwrap();
    assert_eq!((t.len(), t.field_names()), (49, vec!["muni_code".to_string(), "municipality".to_string()]));
    shot(&mut h, "110-excel-reader");

    // A writer to a workbook asks for the sheet and for how the geometry is written.
    h.state_mut().add_node("Writer");
    h.run_steps(3);
    // "File" is also a menu. The label in the form comes last.
    let field = h.get_all_by_label("File").last().unwrap().rect().center() + egui::vec2(40.0, 24.0);
    type_at(&mut h, field, "result.xlsx");
    // Geometry is also a list of tools on the left.
    for label in ["Sheet", "Geometry"] {
        assert!(h.query_all_by_label(label).count() >= 1, "the form should have: {}", label);
    }
    assert!(h.query_by_label("Encoding").is_none() && h.query_by_label("Delimiter").is_none());
    shot(&mut h, "111-excel-writer");
}

#[test]
fn a_reader_is_limited_to_a_number_of_features() {
    let (mut h, _dir) = two_nodes();
    h.state_mut().focus_node("in");
    h.run_steps(3);
    // The field is empty to begin with, and then everything is read.
    h.state_mut().run_blocking();
    h.run_steps(3);
    assert_eq!(h.state().result.as_ref().unwrap().output("in", "Output").unwrap().len(), 39);
    assert_eq!(param(&h, "in", "limit"), "");

    let field = h.get_by_label("Largest number of features").rect().center() + egui::vec2(40.0, 24.0);
    type_at(&mut h, field, "5");
    assert_eq!(param(&h, "in", "limit"), "5");
    assert!(h.state().stale);
    assert!(h.query_by_label("Input · towns.csv · first 5").is_some(), "the node says that it is limited");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.errors, r.output("in", "Output").unwrap().len()), (0, 5));
    assert_eq!(std::fs::read_to_string(h.state().base_dir().join("out/pair.csv")).unwrap().lines().count(), 6);
    shot(&mut h, "135-limit");
}

#[test]
fn a_reader_fetches_a_file_from_the_web() {
    let dir = scratch("web");
    let b = GdalBackend::new();
    for (k, name) in ["north", "south"].iter().enumerate() {
        let opts = WriteOpts { layer: Some(name.to_string()), ..Default::default() };
        b.write(&points(k as i64 * 10, 3), &dir.join("served.gpkg"), &opts, &mut Notes::default()).unwrap();
    }
    let server = file_server(vec![("/open/areas.gpkg", "application/geopackage+sqlite3", std::fs::read(dir.join("served.gpkg")).unwrap())]);
    let url = format!("{}/open/areas.gpkg", server);

    let mut h = empty_app();
    h.state_mut().add_node("Reader");
    h.run_steps(3);
    let field = h.get_by_label("File or folder").rect().center() + egui::vec2(40.0, 24.0);
    type_at(&mut h, field, &url);
    assert_eq!(param(&h, "reader_1", "path"), url);
    // The form is that of a file on the web: whether to keep it, and no filter for a folder.
    assert!(h.query_by_label("Keep the fetched file").is_some());
    assert!(h.query_by_label("Files in the folder").is_none() && h.query_by_label("All 2 layers").is_none());
    assert!(h.query_by_label("Input · 127.0.0.1 · areas.gpkg").is_none(), "the node has taken the name of the file");
    assert_eq!(h.state().snarl.nodes().next().unwrap().name, "areas");

    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.output("reader_1", "Output").unwrap().len(), 3);
    assert!(r.log[0].msg.contains("fetched areas.gpkg ("), "{}", r.log[0].msg);
    // Once the file has been fetched its layers can be ticked.
    assert!(h.query_by_label("All 2 layers").is_some());
    h.get_by_label("All 2 layers").click();
    h.run_steps(3);
    assert_eq!(param(&h, "reader_1", "layer"), r#"["*"]"#);
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(h.state().result.as_ref().unwrap().output("reader_1", "Output").unwrap().len(), 6);
    shot(&mut h, "136-from-the-web");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_writer_is_told_the_format_and_the_fields() {
    let (mut h, dir) = two_nodes();
    h.state_mut().run_blocking();
    h.state_mut().focus_node("out");
    settle(&mut h);
    // The format follows the file name until one is chosen from the list.
    assert_eq!((param(&h, "out", "path"), param(&h, "out", "format")), ("out/pair.csv".to_string(), String::new()));
    assert!(h.query_by_label("Delimiter").is_some(), "a CSV file has a delimiter");
    // A list shows what is chosen as its value.
    h.get_by_value("From the file name").click();
    h.run_steps(3);
    shot(&mut h, "138-writer-formats");
    // In alphabetical order Shapefile is far down the list: the list is scrolled to it.
    let over = h.get_by_label("GeoPackage (.gpkg)").rect().center();
    h.event(egui::Event::PointerMoved(over));
    h.run_steps(2);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -300.0),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    });
    h.run_steps(6);
    h.get_by_label("Shapefile (.shp)").click();
    h.run_steps(3);
    assert_eq!((param(&h, "out", "path"), param(&h, "out", "format")), ("out/pair.shp".to_string(), "shp".to_string()));
    assert!(h.query_by_label("Delimiter").is_none() && h.query_by_label("Encoding").is_some(), "the form is now that of a Shapefile");
    // A file name with the extension of another format takes the list along.
    let field = h.get_all_by_value("out/pair.shp").last().unwrap().rect().center();
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("out/pair.gpkg".into()));
    h.run_steps(3);
    assert_eq!((param(&h, "out", "path"), param(&h, "out", "format")), ("out/pair.gpkg".to_string(), "gpkg".to_string()));
    assert!(h.query_by_value("GeoPackage (.gpkg)").is_some());

    // The attributes that come in are listed, each with what it is written as.
    for label in ["Data types and sizes", "Attribute", "Write as"] {
        assert!(h.query_all_by_label(label).count() >= 1, "the form should have: {}", label);
    }
    for value in ["As it is (text)", "As it is (integer)", "As it is (decimal)"] {
        assert!(h.query_all_by_value(value).count() >= 1, "the form should have a list that says: {}", value);
    }
    assert_eq!(param(&h, "out", "fields"), "");
    // The first one is the name of the town. It is written as text of at most six characters.
    // The form of a GeoPackage is long: its list of attributes is scrolled to first.
    h.get_all_by_value("As it is (decimal)").next().unwrap().scroll_to_me();
    h.run_steps(3);
    h.get_all_by_value("As it is (text)").next().unwrap().click();
    h.run_steps(3);
    h.get_all_by_label("Text").last().unwrap().click();
    h.run_steps(3);
    assert_eq!(param(&h, "out", "fields"), r#"{"name":{"type":"text"}}"#);
    h.state_mut()
        .snarl
        .nodes_mut()
        .find(|n| n.id == "out")
        .unwrap()
        .params
        .insert("fields".into(), serde_json::json!({"name": {"length": 6}, "population": {"type": "text"}}));
    h.run_steps(3);
    assert!(h.query_all_by_value("6").count() >= 1, "the length that is stored is shown");
    shot(&mut h, "139-writer-fields");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert!(r.nodes["out"].warnings[0].contains("values in name are longer than 6 characters and were cut."), "{:?}", r.nodes["out"].warnings);
    let back = GdalBackend::new().read(&dir.join("out/pair.gpkg"), &Default::default(), &mut Notes::default()).unwrap();
    assert_eq!(back.features[0].attrs[0], Value::Text("Götebo".into()));
    assert_eq!(back.fields[2].ty, FieldType::Text);
    assert_eq!(back.features[0].attrs[2], Value::Text("600000".into()));
}

#[test]
fn geojson_is_read_and_written_from_the_window() {
    let dir = scratch("geojson");
    std::fs::write(
        dir.join("places.geojson"),
        r#"{"type": "FeatureCollection", "features": [
  {"type": "Feature", "properties": {"id": 1, "name": "Skövde"}, "geometry": {"type": "Point", "coordinates": [13.8455, 58.3912]}},
  {"type": "Feature", "properties": {"id": 2, "name": "Lidköping"}, "geometry": {"type": "Point", "coordinates": [13.1577, 58.5052]}},
  {"type": "Feature", "properties": {"id": 3, "name": "Göta kanal"}, "geometry": {"type": "LineString", "coordinates": [[14.0, 58.5], [14.2, 58.6]]}}
]}"#,
    )
    .unwrap();

    // Dropped on the window, the file becomes a reader with the name of the file.
    let mut h = empty_app();
    drop_files(&mut h, &[dir.join("places.geojson")]);
    settle(&mut h);
    let nodes: Vec<(String, String)> = h.state().snarl.nodes().map(|n| (n.kind.clone(), n.name.clone())).collect();
    assert_eq!(nodes, [("Reader".to_string(), "places".to_string())]);
    assert!(h.query_by_label("Layers").is_none(), "a GeoJSON file has one layer, so there is nothing to choose");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let t = r.output("reader_1", "Output").unwrap();
    assert_eq!((t.len(), t.crs), (3, Some(4326)));
    assert_eq!(t.field_names(), ["id", "name"]);
    shot(&mut h, "143-geojson-read");

    // A writer gets GeoJSON from the list of formats, and the file name follows.
    std::fs::write(
        dir.join("flow.json"),
        r#"{
      "format": "ographis/1", "name": "GeoJSON",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Places", "x": 0, "y": 0, "params": {"path": "places.geojson"}},
        {"id": "out", "type": "Writer", "name": "Result", "x": 500, "y": 0, "params": {"path": "out/result.csv"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["out", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("flow.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    h.state_mut().focus_node("out");
    h.run_steps(3);
    h.get_by_value("From the file name").click();
    h.run_steps(3);
    h.get_by_label("GeoJSON (.geojson)").click();
    h.run_steps(3);
    assert_eq!((param(&h, "out", "path"), param(&h, "out", "format")), ("out/result.geojson".to_string(), "geojson".to_string()));
    assert!(h.query_by_label("Delimiter").is_none() && h.query_by_label("Encoding").is_none(), "GeoJSON has neither a delimiter nor an encoding to choose");
    shot(&mut h, "144-geojson-writer");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.errors, r.warnings), (0, 0), "{:?}", r.log);
    assert_eq!(r.nodes["out"].files, [dir.join("out/result.geojson")]);
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("out/result.geojson")).unwrap()).unwrap();
    assert_eq!(json["features"].as_array().unwrap().len(), 3);
    assert_eq!(json["features"][1]["properties"]["name"], "Lidköping");
    assert_eq!(json["features"][2]["geometry"]["type"], "LineString");
    // Typing a name that ends in .geojson chooses the format too.
    let field = h.get_all_by_value("out/result.geojson").last().unwrap().rect().center();
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("out/other.gpkg".into()));
    h.run_steps(3);
    assert_eq!(param(&h, "out", "format"), "gpkg");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("out/other.geojson".into()));
    h.run_steps(3);
    assert_eq!(param(&h, "out", "format"), "geojson");
    assert!(h.query_by_value("GeoJSON (.geojson)").is_some());
}

#[test]
fn a_workspace_from_before_the_tools_were_renamed_opens_as_it_was() {
    let dir = example_copy();
    // As version 0.5 saved it: the nodes that were not named by the user have the old names
    // of their tools, and the overlay is done by the tool that has since been replaced.
    std::fs::write(
        dir.join("old.json"),
        r#"{
      "format": "ographis/1", "name": "From 0.5",
      "nodes": [
        {"id": "towns", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv", "crs": "EPSG:4326"}},
        {"id": "areas", "type": "Reader", "name": "Reader", "x": 0, "y": 220, "params": {"path": "subregions.gpkg"}},
        {"id": "overlay", "type": "PointOnAreaOverlayer", "name": "PointOnAreaOverlayer", "x": 300, "y": 100, "params": {}},
        {"id": "large", "type": "Tester", "name": "Large towns", "x": 560, "y": 0, "params": {"test": "population > 20000"}},
        {"id": "sort", "type": "Sorter", "name": "Sorter_2", "x": 820, "y": 0, "params": {"sort_by": "population", "order": "descending"}},
        {"id": "manage", "type": "AttributeManager", "name": "AttributeManager", "x": 1080, "y": 0, "params": {"keep": "name, population, subregion"}}
      ],
      "edges": [
        {"from": ["towns", "Output"], "to": ["overlay", "Point"]},
        {"from": ["areas", "Output"], "to": ["overlay", "Area"]},
        {"from": ["overlay", "Point"], "to": ["large", "Input"]},
        {"from": ["large", "Passed"], "to": ["sort", "Input"]},
        {"from": ["sort", "Sorted"], "to": ["manage", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("old.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    // The names the program gave are now the new ones, and the one the user chose is kept.
    let mut names: Vec<(String, String)> = h.state().snarl.nodes().map(|n| (n.id.clone(), n.name.clone())).collect();
    names.sort();
    let expected = [
        ("areas", "Input"),
        ("large", "Large towns"),
        ("manage", "Manage attributes"),
        ("overlay", "Overlay points on areas"),
        ("sort", "Sort 2"),
        ("towns", "Towns"),
    ];
    assert_eq!(names, expected.map(|(a, b)| (a.to_string(), b.to_string())));
    assert!(!h.state().dirty, "opening an older workspace is not a change");
    // A node with a name of its own says under it which tool it is.
    assert!(h.query_by_label("Test · population > 20000").is_some());
    assert!(h.query_by_label("AttributeManager").is_none() && h.query_by_label("Sorter_2").is_none());

    // It runs as before, also the tool that is no longer in the list.
    h.state_mut().run_blocking();
    h.state_mut().focus_node("manage");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let t = r.output("manage", "Output").unwrap();
    assert_eq!(t.field_names(), ["name", "population", "subregion"]);
    assert!(t.len() > 3 && t.features[0].attrs[0] == Value::Text("Göteborg".into()));
    assert!(r.log.iter().any(|l| l.msg.starts_with("Sort 2: ")), "the log uses the names that are shown");
    assert!(h.query_all_by_label("Manage attributes").count() >= 2, "on the node and above its form");
    shot(&mut h, "148-older-workspace");

    // A new node of the same tool gets the next free name, and what is saved has the new
    // names with the same types as before.
    h.state_mut().add_node("Sorter");
    h.state_mut().add_node("AttributeManager");
    h.run_steps(3);
    let names: Vec<String> = h.state().snarl.nodes().map(|n| n.name.clone()).collect();
    assert!(names.contains(&"Sort".to_string()) && names.contains(&"Manage attributes 2".to_string()), "{:?}", names);
    let saved = h.state().workspace();
    let kinds: Vec<(&str, &str)> = saved.nodes.iter().map(|n| (n.kind.as_str(), n.name.as_str())).collect();
    assert!(kinds.contains(&("Sorter", "Sort 2")) && kinds.contains(&("PointOnAreaOverlayer", "Overlay points on areas")), "{:?}", kinds);
}

#[test]
fn a_spreadsheet_is_given_a_look() {
    let (mut h, dir) = two_nodes();
    h.state_mut().focus_node("out");
    h.run_steps(3);
    let look = ["Font", "Bold top row", "Lock the top row", "Colours"];
    assert!(look.iter().all(|l| h.query_by_label(l).is_none()), "a CSV file has no look to set");
    h.get_by_value("From the file name").click();
    h.run_steps(3);
    h.get_by_label("Excel (.xlsx)").click();
    h.run_steps(3);
    assert_eq!(param(&h, "out", "path"), "out/pair.xlsx");
    for l in look {
        assert!(h.query_by_label(l).is_some(), "the form should have: {}", l);
    }
    h.get_by_label("Colours").scroll_to_me();
    h.run_steps(3);
    // The geometry is not written by default, so there are two lists that say None.
    assert_eq!(h.get_all_by_value("None").count(), 2);
    h.get_all_by_value("None").last().unwrap().click();
    h.run_steps(3);
    h.get_by_label("Blue").click();
    h.run_steps(3);
    assert_eq!(param(&h, "out", "colors"), "blue");
    assert_eq!(param(&h, "out", "bold_header"), "", "a bold top row is the default, and is not stored");
    shot(&mut h, "156-spreadsheet-look");
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(h.state().result.as_ref().unwrap().errors, 0);
    let back = GdalBackend::new().read(&dir.join("out/pair.xlsx"), &Default::default(), &mut Notes::default()).unwrap();
    assert_eq!(back.len(), 39);
    // LibreOffice too, and a GeoPackage has no such settings.
    h.get_by_value("Excel (.xlsx)").scroll_to_me();
    h.run_steps(3);
    h.get_by_value("Excel (.xlsx)").click();
    h.run_steps(3);
    h.get_by_label("LibreOffice Calc (.ods)").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("LibreOffice Calc (.ods)").click();
    h.run_steps(3);
    assert_eq!(param(&h, "out", "path"), "out/pair.ods");
    assert!(h.query_by_label("Colours").is_some());
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(h.state().result.as_ref().unwrap().errors, 0);
    assert_eq!(GdalBackend::new().read(&dir.join("out/pair.ods"), &Default::default(), &mut Notes::default()).unwrap().len(), 39);
    h.get_by_value("LibreOffice Calc (.ods)").click();
    h.run_steps(3);
    h.get_by_label("GeoPackage (.gpkg)").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("GeoPackage (.gpkg)").click();
    h.run_steps(3);
    assert!(look.iter().all(|l| h.query_by_label(l).is_none()));
}

#[test]
fn a_geotiff_is_dropped_and_read_as_points_or_squares() {
    let dir = example_copy();
    let mut h = empty_app();
    drop_files(&mut h, &[dir.join("heights.tif")]);
    settle(&mut h);
    let nodes: Vec<(String, String)> = h.state().snarl.nodes().map(|n| (n.kind.clone(), n.name.clone())).collect();
    assert_eq!(nodes, [("Reader".to_string(), "heights".to_string())]);
    assert!(h.query_by_label("Cells as").is_some() && h.query_by_value("Points, one per cell").is_some());
    assert!(h.query_by_label("Layers").is_none() && h.query_by_label("Encoding").is_none());
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let t = r.output("reader_1", "Output").unwrap();
    assert_eq!((t.len(), t.crs), (579, Some(3006)));
    assert!(matches!(t.features[0].geom, Some(geo::Geometry::Point(_))));
    shot(&mut h, "157-geotiff-points");
    h.get_by_value("Points, one per cell").click();
    h.run_steps(3);
    h.get_by_label("Squares, one per cell").click();
    h.run_steps(3);
    h.state_mut().run_blocking();
    settle(&mut h);
    let t = h.state().result.clone().unwrap();
    let t = t.output("reader_1", "Output").unwrap();
    assert!(matches!(t.features[0].geom, Some(geo::Geometry::Polygon(_))));
    shot(&mut h, "158-geotiff-squares");
}

// ---------- Version 0.59 ----------

#[test]
fn the_menu_of_a_node_shows_its_folder() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.state_mut().quiet = true;
    h.run_steps(6);
    let dir = h.state().base_dir();
    let towns = place_of(&h, "Towns", 0, true);
    right_click_at(&mut h, towns);
    shot(&mut h, "200-node-menu-folder");
    h.get_by_label("Show in folder").click();
    h.run_steps(3);
    assert_eq!(h.state().shown, [dir.join("towns.csv")]);
    // An output whose file is not written yet shows the folder it goes to, or the nearest that is there.
    let out = graph_id(&h, "out_towns");
    h.state_mut().show_folder_of(out);
    assert_eq!(h.state().shown.len(), 2);
    assert!(dir.join("out").starts_with(&h.state().shown[1]) || h.state().shown[1].starts_with(&dir), "{:?}", h.state().shown);
    // A tool has no file, and its menu has no such item.
    let tool = place_of(&h, "Only Skaraborg", 0, true);
    right_click_at(&mut h, tool);
    assert!(h.query_by_label("Show in folder").is_none());
}

#[test]
fn dropped_files_land_where_they_are_dropped() {
    let mut h = empty_app();
    h.run_steps(3);
    let dir = example_copy();
    let at = h.state().canvas_rect().left_top() + egui::vec2(300.0, 200.0);
    h.state_mut().drop_files_at(&[dir.join("towns.csv"), dir.join("routes.shp")], Some(at));
    h.run_steps(4);
    let st = h.state();
    let mut pos: Vec<(String, egui::Pos2)> = st.snarl.nodes_pos_ids().map(|(_, p, n)| (n.name.clone(), p)).collect();
    pos.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    let want = st.screen_to_graph(at);
    assert_eq!(pos[0].0, "towns");
    assert!((pos[0].1 - want).length() < 1.0, "{:?} {:?}", pos[0].1, want);
    assert!((pos[1].1.x - want.x).abs() < 1.0 && pos[1].1.y > want.y + 100.0, "the next one goes below: {:?}", pos);
    // Outside the canvas, the files go in a free spot as before.
    h.state_mut().drop_files_at(&[dir.join("towns.csv")], Some(egui::pos2(5.0, 5.0)));
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 3);
    shot(&mut h, "203-dropped-where-dropped");
}

// ---------- Version 0.62 ----------

#[test]
fn postgis_is_reached_from_input_and_output() {
    let mut h = empty_app();
    // A connection in the settings, as Edit → Settings makes it.
    h.state_mut().settings.connections = vec![og_engine::settings::Connection {
        name: "Local test".into(),
        kind: "postgis".into(),
        host: "localhost".into(),
        port: "5432".into(),
        database: "gistest".into(),
        user: "gis".into(),
        password: "gis-test".into(),
    }];
    h.state_mut().add_node("Reader");
    h.run_steps(3);
    let reader = h.state().selected[0];
    h.get_by_label("PostGIS…").click();
    h.run_steps(3);
    assert!(h.state().db_dialog.is_some() && h.state().settings_window.is_none());
    for label in ["Connections…", "Cancel"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the PostGIS window", label);
    }
    assert!(h.query_by_label("Password").is_none(), "the connection is chosen by its name");
    // The only connection is chosen, and its tables are listed at once.
    assert_eq!(h.state().db_dialog.as_ref().unwrap().connection, "Local test");
    for _ in 0..50 {
        h.run_steps(2);
        if !h.state().db_dialog.as_ref().unwrap().busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    h.run_steps(3);
    let listed = matches!(h.state().db_dialog.as_ref().unwrap().tables, Some(Ok(_)));
    if listed {
        // The tables can be searched and ticked.
        let d = h.state_mut().db_dialog.as_mut().unwrap();
        d.search = "subreg".into();
        h.run_steps(3);
        assert!(h.query_by_label("public.towns").is_none(), "the search leaves out what does not match");
        h.get_by_label("vgr.subregions").click();
        h.run_steps(3);
        shot(&mut h, "220-postgis-tables");
    } else {
        println!("No test database: {:?}", h.state().db_dialog.as_ref().unwrap().tables);
        h.state_mut().db_dialog.as_mut().unwrap().ticked = vec!["vgr.subregions".into()];
        h.run_steps(3);
    }
    h.get_by_label("Read 1 table").click();
    h.run_steps(4);
    assert!(h.state().db_dialog.is_none());
    let n = &h.state().snarl[reader];
    assert_eq!(n.kind, "PostgisReader");
    assert_eq!(n.name, "PostGIS input");
    assert_eq!(n.params.get("tables"), Some(&serde_json::json!(["vgr.subregions"])));
    assert_eq!(n.params.get("connection"), Some(&serde_json::json!("Local test")));
    assert!(n.params.get("password").is_none() && n.params.get("host").is_none(), "the model keeps the name only");
    // And back to a file.
    assert!(h.query_by_label("Choose tables…").is_some());
    h.get_by_label("Read a file instead").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl[reader].kind, "Reader");

    // An output: the format list has the GeoSpatial PDF and PostGIS.
    h.state_mut().add_node("Writer");
    h.run_steps(3);
    let writer = h.state().selected[0];
    h.get_by_value("From the file name").click();
    h.run_steps(3);
    h.get_by_label("GeoSpatial PDF with layers (.pdf)").click();
    h.run_steps(4);
    assert_eq!(h.state().snarl[writer].kind, "LayeredPdfWriter");
    assert!(h.query_by_label("Page size").is_some() && h.query_by_label("Margin in %").is_some());
    // PostGIS: the connection is taken from the other node, and Cancel goes back to the PDF.
    h.get_by_value("GeoSpatial PDF with layers (.pdf)").click();
    h.run_steps(3);
    // Far down the list: it is scrolled to.
    let over = h.get_by_label("GeoPackage (.gpkg)").rect().center();
    h.event(egui::Event::PointerMoved(over));
    h.run_steps(2);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -300.0),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    });
    h.run_steps(6);
    h.get_by_label("PostGIS table").click();
    h.run_steps(3);
    assert!(h.state().db_dialog.as_ref().is_some_and(|d| d.output));
    // The tables are listed before Cancel, as a user would see them.
    for _ in 0..50 {
        h.run_steps(2);
        if !h.state().db_dialog.as_ref().unwrap().busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    h.run_steps(2);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl[writer].kind, "LayeredPdfWriter");
    assert_eq!(h.state().snarl[writer].params.get("format"), Some(&serde_json::json!("geopdf")));
    h.get_by_value("GeoSpatial PDF with layers (.pdf)").scroll_to_me();
    h.run_steps(3);
    h.get_by_value("GeoSpatial PDF with layers (.pdf)").click();
    h.run_steps(3);
    // Far down the list: it is scrolled to.
    let over = h.get_by_label("GeoPackage (.gpkg)").rect().center();
    h.event(egui::Event::PointerMoved(over));
    h.run_steps(2);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -300.0),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    });
    h.run_steps(6);
    h.get_by_label("PostGIS table").click();
    h.run_steps(3);
    // The tables are listed first, which can take a moment when much else runs.
    for _ in 0..100 {
        h.run_steps(2);
        if h.state().db_dialog.as_ref().is_some_and(|d| !d.busy()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    {
        let d = h.state_mut().db_dialog.as_mut().unwrap();
        d.schema = "vgr".into();
        d.table = "Sites 2026".into();
    }
    h.run_steps(3);
    assert!(h.query_by_label("Written as sites_2026, as PostgreSQL prefers its names.").is_some());
    shot(&mut h, "221-postgis-output");
    h.get_by_label("Write to this table").click();
    h.run_steps(4);
    let n = &h.state().snarl[writer];
    assert_eq!(n.kind, "PostgisWriter");
    assert_eq!((n.params.get("schema"), n.params.get("table")), (Some(&serde_json::json!("vgr")), Some(&serde_json::json!("sites_2026"))));
    for label in ["Schema", "Table", "If the table is there", "Geometry type", "Coordinate system", "Geometry column"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form of PostGIS output", label);
    }
    assert!(h.query_by_value("As the stream is").is_some());
    shot(&mut h, "222-postgis-output-form");
    // A file format makes it an output to a file again.
    h.get_by_value("PostGIS table").click();
    h.run_steps(3);
    // The list opens where the chosen format is, with the slides and Shapefile below it.
    shot(&mut h, "223-formats-list");
    h.get_by_label("Shapefile (.shp)").click();
    h.run_steps(3);
    assert_eq!(h.state().snarl[writer].kind, "Writer");
}

#[test]
fn the_file_menu_closes_the_program() {
    let mut h = empty_app();
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label_contains("Exit").click();
    h.run_steps(3);
    assert!(h.state().allow_close, "nothing to save, so it closes");
}

#[test]
fn changes_left_behind_are_offered_and_restored() {
    let dir = example_copy();
    let folder = og_gui::recovery::folder().unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    let model = std::fs::read_to_string(dir.join("house.ographis")).unwrap();
    let kept = folder.join("1-1.json");
    let doc = serde_json::json!({
        "name": "Two houses, changed",
        "path": dir.join("house.ographis").display().to_string(),
        "saved": "2026-10-16 21:40",
        "model": serde_json::from_str::<serde_json::Value>(&model).unwrap(),
    });
    std::fs::write(&kept, doc.to_string()).unwrap();
    // Left alone long enough to belong to a program that stopped.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    std::fs::File::options().write(true).open(&kept).unwrap().set_modified(old).unwrap();
    let mut app = App::new();
    app.look_for_recovery();
    assert_eq!(app.recoverable.len(), 1);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(3);
    shot(&mut h, "275-restore-changes");
    h.get_by_label("Restore").click();
    h.run_steps(3);
    assert!(h.state().dirty && h.state().recoverable.is_empty());
    assert_eq!(h.state().path.as_deref(), Some(dir.join("house.ographis").as_path()));
    assert!(h.state().snarl.nodes().any(|n| n.id == "neighbour"));
    assert!(!kept.exists());
}

// ---------- Version 0.88 ----------

/// A model says what it does: the description is written under Model → Description…,
/// saved with the model, and shown on the canvas when the model is opened. The model is
/// copied as a picture with Ctrl+Shift+C.
#[test]
fn a_model_has_a_description_and_is_copied_as_a_picture() {
    let (mut h, dir) = two_nodes();
    h.state_mut().description_open = true;
    h.run_steps(3);
    let field = h.get_by_role(egui::accesskit::Role::MultilineTextInput).rect().center();
    click_at(&mut h, field);
    h.event(egui::Event::Text("Writes the towns to a CSV file.".into()));
    h.run_steps(3);
    assert_eq!(h.state().description, "Writes the towns to a CSV file.");
    shot(&mut h, "088-description-window");
    h.state_mut().description_open = false;
    h.run_steps(2);
    let file = dir.join("described.ographis");
    h.state_mut().save_to(&file);
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"description\": \"Writes the towns to a CSV file.\""));
    // Opened again: the description is on the canvas.
    let mut app = App::new();
    app.load(&file).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    assert!(h.query_by_label("Writes the towns to a CSV file.").is_some());
    shot(&mut h, "088-description-on-canvas");
    // Copied as a picture.
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::C);
    let copied = h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyImage(_)));
    h.run_steps(2);
    assert!(copied || h.state().message().is_some_and(|m| m.contains("copied as a picture")), "{:?}", h.state().message());
    assert!(h.state().message().is_some_and(|m| m.contains("copied as a picture")), "{:?}", h.state().message());
}
