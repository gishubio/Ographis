//! Running models: the counts filling in, the log, errors, stopping and pausing, running part
//! of a model, reusing what has not changed, and variables.

use crate::common::*;

#[test]
fn load_run_and_inspect() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    assert_eq!(h.state().snarl.nodes().count(), 18);
    assert!(!h.state().dirty, "a workspace that was only opened has no unsaved changes");
    shot(&mut h, "01-loaded");

    h.state_mut().run_blocking();
    h.state_mut().focus_node("clip_towns");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0);
    assert_eq!(r.output("clip_towns", "Inside").unwrap().len(), 12);
    assert!(!h.state().map.shows_tiles(), "without a tile server the map should show a grid");
    shot(&mut h, "02-run");

    // A click on a row in the table selects the feature, and it shows on the map.
    click_text(&mut h, "Lidköping", 0, false);
    assert_eq!(h.state().view.feature, Some(1));
    shot(&mut h, "03-selected-row");

    // A plain click on a node on the canvas selects it and shows its data.
    click_text(&mut h, "Municipalities", 0, true);
    let app = h.state();
    assert_eq!(app.focus.and_then(|n| app.snarl.get_node(n)).map(|n| n.id.as_str()), Some("municipalities"));
    assert_eq!(app.view.node.as_deref(), Some("municipalities"));
    assert_eq!(selected(&h), ["municipalities"]);
    // Neither opening, running nor clicking counts as a change.
    assert!(!h.state().dirty);
    h.state_mut().focus_node("clip_towns");
    h.run_steps(3);

    // The other port, and then the log.
    h.get_by_label("Outside 27").click();
    h.run_steps(3);
    assert_eq!(h.state().view.port.as_deref(), Some("Outside"));
    h.get_by_label("Log").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Run finished: 18 nodes").is_some());
    shot(&mut h, "04-log");
}

#[test]
fn add_node_from_palette_and_rerun() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.state_mut().run_blocking();
    h.run_steps(3);
    // "Calculate area" is both a node in the workspace and an entry in the palette.
    assert!(h.state().snarl.nodes().any(|n| n.id == "area" && n.name == "Calculate area"));
    add_from_palette(&mut h, "Geometry", "Calculate area");
    let app = h.state();
    assert_eq!(app.snarl.nodes().count(), 19);
    assert!(app.dirty && app.stale);
    assert!(app.snarl.nodes().any(|n| n.id == "areacalculator_1" && n.kind == "AreaCalculator" && n.name == "Calculate area 2"));
    assert_eq!(selected(&h), ["areacalculator_1"]);
    shot(&mut h, "20-new-node");

    // The saved workspace can be read back and gives the same result in the engine.
    let ws = h.state().workspace();
    let again = og_engine::Workspace::from_json(&ws.to_json()).unwrap();
    assert_eq!(again.nodes.len(), 19);
    assert_eq!(again.edges.len(), 17);
    h.state_mut().run_blocking();
    h.run_steps(3);
    assert!(!h.state().stale);
    assert_eq!(h.state().result.as_ref().unwrap().errors, 0);
}

#[test]
fn the_overlay_example_runs_with_the_newer_tools() {
    let mut h = harness_with("overlay.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    assert_eq!(h.state().snarl.nodes().count(), 17);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("dissolve");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.errors, r.warnings), (0, 0));
    // Four roads and one railway, buffered and merged into one corridor each.
    assert_eq!(r.output("dissolve", "Dissolved").unwrap().len(), 2);
    // Every town falls in exactly one subregion, so the counts add up to all towns.
    let overlaps: i64 = {
        let t = r.output("count", "Polygons").unwrap();
        let i = t.field_index("towns").unwrap();
        t.features.iter().filter_map(|f| f.attrs[i].as_f64()).sum::<f64>() as i64
    };
    assert_eq!(overlaps, 39);
    shot(&mut h, "45-overlay-corridors");
    h.state_mut().focus_node("sort");
    settle(&mut h);
    shot(&mut h, "46-overlay-statistics");
}

#[test]
fn errors_show_in_log_and_on_nodes() {
    let mut app = App::new();
    let dir = std::env::temp_dir().join(format!("og-gui-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("errors.json");
    std::fs::write(
        &path,
        r#"{"name": "Workspace with an error", "nodes": [
          {"id": "a", "type": "Reader", "name": "Missing file", "x": 20, "y": 20, "params": {"path": "not_there.gpkg"}},
          {"id": "b", "type": "Tester", "name": "Tester", "x": 240, "y": 20, "params": {"test": "x > 1"}}
        ], "edges": [{"from": ["a", "Output"], "to": ["b", "Input"]}]}"#,
    )
    .unwrap();
    app.load(&path).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("a");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 1);
    assert!(r.nodes["a"].error.as_ref().unwrap().contains("The file does not exist"));
    assert!(h.query_by_label("Log · 1 error").is_some());
    assert!(h.query_by_label("1 error in the run").is_some());
    shot(&mut h, "50-error");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wire_by_dragging_delete_by_key_and_threaded_run() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(5);
    let before = h.state().workspace().edges.len();

    // Drag from the output port Failed on the tester to the input port Supplier on the
    // join. A port sits on the edge of the node, level with its label.
    let app = h.state();
    let from_y = h.get_by_label("Failed").rect().center().y;
    let to_y = h.get_by_label("Supplier").rect().center().y;
    let from = app.graph_to_screen(egui::pos2(app.node_rect("skaraborg").unwrap().right(), from_y));
    let to = app.graph_to_screen(egui::pos2(app.node_rect("join").unwrap().left(), to_y));
    drag(&mut h, from, to, egui::PointerButton::Primary);
    let ws = h.state().workspace();
    assert_eq!(ws.edges.len(), before + 1, "the connection should have been made");
    assert!(ws.edges.iter().any(|e| e.from == ("skaraborg".to_string(), "Failed".to_string()) && e.to == ("join".to_string(), "Supplier".to_string())));
    assert!(h.state().dirty && h.state().stale);
    shot(&mut h, "60-dragged-connection");

    // Delete removes the selected node and its connections.
    h.state_mut().focus_node("outside");
    h.run_steps(2);
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    let ws = h.state().workspace();
    assert_eq!(ws.nodes.len(), 17);
    assert!(ws.edges.iter().all(|e| e.from.0 != "outside" && e.to.0 != "outside"));

    // F5 runs in a background thread. The result fills in node by node, so wait until
    // the run says it is over.
    h.key_press(egui::Key::F5);
    h.run_steps(2);
    assert!(h.state().is_running() || h.state().result.as_ref().is_some_and(|r| !r.log.is_empty()), "F5 should start a run");
    for _ in 0..200 {
        h.run_steps(2);
        if !h.state().is_running() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!h.state().is_running(), "the run should finish");
    let r = h.state().result.clone().expect("a finished run has a result");
    assert_eq!(r.nodes.len(), 17, "every node has been run");
    assert_eq!(r.errors, 0);
    // The join got three more subregions as Supplier, without the attribute municipality.
    assert_eq!(r.nodes["join"].inputs["Supplier"].len(), 49 + 3);
}

#[test]
fn counts_fill_in_node_by_node_while_the_workspace_runs() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let full = og_engine::run(&h.state().workspace(), Some(&GdalBackend::new()), &h.state().base_dir());
    assert_eq!((full.errors, full.order.len()), (0, 18));
    let name_of = |h: &Harness<'_, App>, id: &str| h.state().snarl.nodes().find(|n| n.id == id).unwrap().name.clone();

    // A run that has just started has no counts to show.
    h.state_mut().begin_run();
    h.run_steps(3);
    assert!(h.state().is_running());
    assert!(egui_kittest::kittest::NodeT::accesskit_node(&h.get_by_label("Run")).is_disabled(), "the Run button cannot be pressed while a run is going on");
    assert!(h.query_by_label("39").is_none());

    // The reader of the towns comes first in this workspace. While it works it is marked.
    assert_eq!(full.order[0], "towns");
    h.state_mut().on_run_msg(RunMsg::Started("towns".into()));
    h.run_steps(3);
    assert!(h.query_by_label("running").is_some());
    assert!(h.query_by_label("Running Towns · 1 of 18").is_some());
    // When it is done its count is there, and nothing further down has one.
    h.state_mut().on_run_msg(RunMsg::Finished("towns".into(), Box::new(full.nodes["towns"].clone())));
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("39").count(), 1);
    assert!(h.query_by_label("running").is_none());

    // Half way through: the nodes that are done have counts, the one at work is marked.
    for id in &full.order[1..9] {
        h.state_mut().on_run_msg(RunMsg::Started(id.clone()));
        h.state_mut().on_run_msg(RunMsg::Finished(id.clone(), Box::new(full.nodes[id].clone())));
    }
    let at_work = full.order[9].clone();
    h.state_mut().on_run_msg(RunMsg::Started(at_work.clone()));
    h.run_steps(3);
    assert_eq!(h.state().result.as_ref().unwrap().nodes.len(), 9);
    assert_eq!(h.state().running_node.as_deref(), Some(at_work.as_str()));
    assert!(h.query_by_label(&format!("Running {} · 10 of 18", name_of(&h, &at_work))).is_some());
    shot(&mut h, "104-running");

    // The data of a node that is done can be looked at before the run is over.
    h.state_mut().focus_node("towns");
    h.run_steps(3);
    assert!(h.query_by_label("Output 39").is_some());

    for id in &full.order[9..] {
        h.state_mut().on_run_msg(RunMsg::Finished(id.clone(), Box::new(full.nodes[id].clone())));
    }
    h.state_mut().on_run_msg(RunMsg::Done(Box::new(full.clone())));
    h.run_steps(3);
    assert!(!h.state().is_running() && h.state().running_node.is_none());
    assert_eq!(h.state().result.as_ref().unwrap().log.len(), full.log.len());
    assert!(h.query_by_label("Run").is_some() && h.query_by_label("running").is_none());
}

#[test]
fn the_stops_example_shows_what_is_new() {
    let mut h = harness_with("stops.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let app = h.state();
    assert_eq!((app.snarl.nodes().count(), app.groups.len()), (11, 3), "ten nodes and a note, in three groups");
    assert!(!app.dirty, "a workspace with groups and notes that was only opened has no unsaved changes");
    assert!(h.query_by_label("3 files").is_some(), "the reader of the folder says how many files it stands for");
    assert!(h.query_by_label("One file per municipality").is_some(), "the comment on the reader is shown under it");
    assert_eq!(h.state().groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Input", "Manage attributes", "Output"]);

    h.state_mut().run_blocking();
    h.state_mut().focus_node("tidy");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.errors, r.warnings, r.nodes.len()), (0, 0, 10));
    let t = r.output("tidy", "Output").unwrap();
    assert_eq!(t.field_names(), ["stop", "lines", "lat", "lon", "muni_code", "municipality", "stop_id", "exported"]);
    let row = |i: usize| t.features[i].attrs.iter().map(|v| v.to_text()).collect::<Vec<_>>();
    // The first four characters of the file name, the name from the joined table, and the id without spaces.
    assert_eq!(row(1)[4..7], ["1494", "Lidköping", "1494-Nyastadenstorg"]);
    assert_eq!(row(6)[4..7], ["1496", "Skövde", "1496-Skaraborgssjukhus"]);
    assert_eq!(row(0)[7], chrono_today());
    // Both writers wrote: a sheet without geometry and a layer with points.
    let dir = h.state().base_dir();
    let b = GdalBackend::new();
    let sheets = b.layers(&dir.join("out/stops.xlsx")).unwrap();
    assert_eq!((sheets[0].name.as_str(), sheets[0].count, sheets[0].fields.len()), ("Stops", Some(11), 8));
    let layers = b.layers(&dir.join("out/stops.gpkg")).unwrap();
    assert_eq!((layers[0].name.as_str(), layers[0].count, layers[0].crs), ("stops", Some(11), Some(4326)));
    shot(&mut h, "112-stops-example");

    // Saved again, the groups and the note are still there.
    let ws = h.state().workspace();
    assert_eq!((ws.nodes.len(), ws.groups.len(), ws.notes.len()), (10, 3, 1));
    assert_eq!(ws.groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Input", "Manage attributes", "Output"]);
}

#[test]
fn the_log_tells_how_far_a_large_read_has_come() {
    let dir = scratch("progress");
    let mut text = String::from("id;x;y\n");
    for i in 0..120_000 {
        text.push_str(&format!("{};{};{}\n", i, 400_000 + i % 1000, 6_400_000 + i / 1000));
    }
    std::fs::write(dir.join("pts.csv"), text).unwrap();
    std::fs::write(
        dir.join("m.json"),
        r#"{"format": "ographis/1", "name": "Progress", "nodes": [{"id": "in", "type": "Reader", "name": "Points", "x": 0, "y": 0, "params": {"path": "pts.csv", "crs": "EPSG:3006"}}], "edges": []}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("m.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    // A run in its own thread, as from the Run button: the lines come while it goes on.
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx);
    let start = std::time::Instant::now();
    while h.state().is_running() && start.elapsed() < std::time::Duration::from_secs(120) {
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.run_steps(1);
    }
    h.run_steps(3);
    let log: Vec<String> = h.state().result.as_ref().unwrap().log.iter().map(|l| l.msg.clone()).collect();
    let steps: Vec<&String> = log.iter().filter(|l| l.starts_with("Points: read ")).collect();
    assert_eq!(steps.len(), 10, "{:?}", log);
    assert_eq!(steps[0], "Points: read 12 000 of 120 000 features (10 %)");
    assert_eq!(steps[9], "Points: read 120 000 of 120 000 features (100 %)");
    assert!(log.iter().any(|l| l.starts_with("Points: 120 000 out")), "counts have a space between the thousands: {:?}", log);
    // The count on the port and in the data tab too.
    h.state_mut().focus_node("in");
    h.run_steps(3);
    assert!(h.query_all_by_label("120 000").count() >= 1);
    assert!(h.query_by_label("Output 120 000").is_some());
    h.get_by_label("Log").click();
    h.run_steps(3);
    shot(&mut h, "172-log-progress");
}

#[test]
fn variables_are_set_in_the_model_and_used_in_settings() {
    let dir = example_copy();
    std::fs::write(
        dir.join("vars.json"),
        r#"{
      "format": "ographis/1", "name": "Variables",
      "nodes": [
        {"id": "towns", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "$(source).csv", "crs": "EPSG:4326"}},
        {"id": "big", "type": "Tester", "name": "Large towns", "x": 300, "y": 0, "params": {"test": "population >= $(least)"}},
        {"id": "out", "type": "Writer", "name": "Result", "x": 600, "y": 0, "params": {"path": "out/$(source)_large.csv"}}
      ],
      "edges": [{"from": ["towns", "Output"], "to": ["big", "Input"]}, {"from": ["big", "Passed"], "to": ["out", "Input"]}],
      "variables": [{"name": "source", "value": "towns"}, {"name": "least", "value": "50000"}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("vars.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    assert_eq!(h.state().variables.len(), 2);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.nodes["out"].files, [dir.join("out/towns_large.csv")]);
    let large = r.output("big", "Passed").unwrap().len();
    // The test shows no error, since the variable is put in before it is checked.
    h.state_mut().focus_node("big");
    h.run_steps(3);
    assert!(h.query_by_label_contains("Unknown").is_none() && h.query_by_label_contains("cannot").is_none());

    // The window in the Model menu changes the value, which is undone like other changes.
    h.get_by_label("Model").click();
    h.run_steps(3);
    h.get_by_label("Variables (2)…").click();
    h.run_steps(3);
    assert!(h.state().variables_open);
    shot(&mut h, "174-variables");
    let field = h.get_all_by_value("50000").last().unwrap().rect().center();
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("100000".into()));
    h.run_steps(3);
    assert_eq!(h.state().variables[1].value, "100000");
    assert!(h.state().dirty && h.state().stale);
    h.get_by_label("Add variable").click();
    h.run_steps(3);
    // The new row has two empty fields: the name to the left and the value to the right.
    // Those above the button: the filter of the table is empty too.
    let button = h.get_by_label("Add variable").rect().center();
    let empty: Vec<egui::Pos2> = h.get_all_by_value("").map(|n| n.rect().center()).filter(|p| p.y < button.y && (p.x - button.x).abs() < 500.0).collect();
    let lowest = empty.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let name = *empty.iter().filter(|p| (p.y - lowest).abs() < 2.0).min_by(|a, b| a.x.total_cmp(&b.x)).expect("an empty name field");
    click_at(&mut h, name);
    h.event(egui::Event::Text("2bad".into()));
    h.run_steps(3);
    assert!(h.query_by_label_contains("cannot be a name").is_some());
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(!h.state().variables_open);
    h.state_mut().run_blocking();
    settle(&mut h);
    let fewer = h.state().result.as_ref().unwrap().output("big", "Passed").unwrap().len();
    assert!(fewer < large, "{} towns of 100 000 or more, {} of 50 000", fewer, large);
    // Saved with the model.
    let saved = h.state().workspace();
    assert_eq!(saved.variables[1].value, "100000");
    // A variable that is not defined is an error that says so.
    h.state_mut().variables.remove(0);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert!(r.nodes["towns"].error.as_ref().unwrap().starts_with("The variable source is not defined."), "{:?}", r.nodes["towns"].error);
}

// ---------- Version 0.56 ----------

#[test]
fn a_run_is_stopped_with_the_stop_button() {
    let dir = scratch("stop");
    let mut text = String::from("id;v1;v2\n");
    for i in 0..400_000 {
        text.push_str(&format!("{};{};{}\n", i, i % 1000, i / 1000));
    }
    std::fs::write(dir.join("big.csv"), text).unwrap();
    std::fs::write(
        dir.join("m.json"),
        r#"{"format": "ographis/1", "name": "Stop", "nodes": [
            {"id": "in", "type": "Reader", "name": "Rows", "x": 0, "y": 0, "params": {"path": "big.csv"}},
            {"id": "sort", "type": "Sorter", "name": "Sorted", "x": 300, "y": 0, "params": {"sort_by": "v1"}}],
          "edges": [{"from": ["in", "Output"], "to": ["sort", "Input"]}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("m.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    assert!(!h.get_by_label("Stop").rect().is_negative());
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx);
    h.run_steps(1);
    assert!(h.state().is_running());
    // No screenshot here: it waits its turn with the graphics, and the run could end meanwhile.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F5);
    h.run_steps(1);
    assert!(h.state().stopping && h.state().is_running());
    shot(&mut h, "180-stopping");
    let start = std::time::Instant::now();
    while h.state().is_running() && start.elapsed() < std::time::Duration::from_secs(60) {
        std::thread::sleep(std::time::Duration::from_millis(30));
        h.run_steps(1);
    }
    h.run_steps(3);
    assert!(!h.state().is_running() && !h.state().stopping);
    let r = h.state().result.clone().unwrap();
    let log: Vec<String> = r.log.iter().map(|l| l.msg.clone()).collect();
    assert!(log.last().unwrap().starts_with("Run stopped"), "{:?}", log);
    assert_eq!(r.nodes["sort"].status, og_engine::Status::Skipped, "{:?}", r.nodes["sort"]);
    assert!(
        r.nodes["sort"].warnings.iter().chain(r.nodes["sort"].error.iter()).any(|w| w.contains("Stopped")) || log.iter().any(|l| l.contains("Stopped")),
        "{:?}",
        r.nodes["sort"]
    );
    // A new run after a stop runs to the end.
    h.state_mut().run_blocking();
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.output("sort", "Sorted").map(|t| t.len()), Some(400_000));
    assert!(r.log.last().unwrap().msg.contains(" s"), "{:?}", r.log.last());
    assert!(!r.log.iter().any(|l| l.msg.contains(" ms")), "no milliseconds in the log");
}

#[test]
fn a_run_is_paused_and_resumed() {
    let dir = scratch("pause");
    let mut text = String::from("id;v1;v2\n");
    for i in 0..400_000 {
        text.push_str(&format!("{};{};{}\n", i, i % 1000, i / 1000));
    }
    std::fs::write(dir.join("big.csv"), text).unwrap();
    std::fs::write(
        dir.join("m.json"),
        r#"{"format": "ographis/1", "name": "Pause", "nodes": [
            {"id": "in", "type": "Reader", "name": "Rows", "x": 0, "y": 0, "params": {"path": "big.csv"}},
            {"id": "sort", "type": "Sorter", "name": "Sorted", "x": 300, "y": 0, "params": {"sort_by": "v1"}}],
          "edges": [{"from": ["in", "Output"], "to": ["sort", "Input"]}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("m.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    assert!(h.query_by_label("Pause").is_some());
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx);
    h.run_steps(1);
    h.key_press(egui::Key::F6);
    h.run_steps(2);
    assert!(h.state().paused && h.state().is_running());
    assert!(h.query_by_label("Resume").is_some(), "the button says Resume while paused");
    // Nothing more happens while it is paused.
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.run_steps(2);
    let lines = h.state().result.as_ref().unwrap().log.len();
    let done = h.state().result.as_ref().unwrap().nodes.len();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    h.run_steps(2);
    assert_eq!(h.state().result.as_ref().unwrap().log.len(), lines, "the log stands still while paused");
    assert_eq!(h.state().result.as_ref().unwrap().nodes.len(), done);
    assert!(h.state().is_running());
    shot(&mut h, "190-paused");
    // The button resumes.
    h.get_by_label("Resume").click();
    h.run_steps(2);
    assert!(!h.state().paused);
    wait_for_run(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.output("sort", "Sorted").map(|t| t.len()), Some(400_000));
    assert!(r.log.last().unwrap().msg.starts_with("Run finished"));
}

#[test]
fn parts_of_the_model_are_run_from_the_menu_of_a_node() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    // Without the cache, Run from here cannot be chosen.
    let at = place_of(&h, "Calculate length", 0, true);
    right_click_at(&mut h, at);
    h.run_steps(3);
    assert!(h.query_by_label("Run to here").is_some());
    assert!(egui_kittest::kittest::NodeT::accesskit_node(&h.get_by_label("Run from here")).is_disabled(), "Run from here needs the cache");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    // Model → Reuse what has not changed, which is saved with the model.
    h.get_by_label("Model").click();
    h.run_steps(3);
    h.get_by_label("Reuse what has not changed").click();
    h.run_steps(3);
    assert!(h.state().use_cache && h.state().dirty);
    assert!(h.state().workspace().cache);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    h.state_mut().run_blocking();
    settle(&mut h);
    let first = h.state().result.clone().unwrap();
    assert_eq!(first.errors, 0);

    // Run from here: the node and what follows it run, what comes before is cached.
    let at = place_of(&h, "Calculate length", 0, true);
    right_click_at(&mut h, at);
    h.run_steps(3);
    shot(&mut h, "191-node-menu-run");
    h.get_by_label("Run from here").click();
    h.run_steps(2);
    wait_for_run(&mut h);
    let r = h.state().result.clone().unwrap();
    let node = |name: &str| h.state().snarl.nodes().find(|n| n.name == name).unwrap().id.clone();
    let (length, routes, clip) = (node("Calculate length"), node("Routes"), node("Clip routes"));
    assert!(r.nodes[&routes].cached && r.nodes[&clip].cached, "what comes before is taken from the cache");
    assert!(!r.nodes[&length].cached);
    assert!(r.nodes.len() >= 17, "the nodes beside it keep their results: {}", r.nodes.len());
    assert!(h.query_all_by_label("cached").count() >= 2, "cached nodes are marked");
    assert!(r.log.iter().any(|l| l.msg.ends_with("from the cache")));
    shot(&mut h, "192-run-from-here");

    // A changed setting before the node is run again.
    let id = graph_id(&h, &clip);
    h.state_mut().snarl[id].params.insert("dummy".into(), serde_json::json!("1"));
    let ctx = h.ctx.clone();
    h.state_mut().run_from(&ctx, &length);
    wait_for_run(&mut h);
    let r = h.state().result.clone().unwrap();
    assert!(r.nodes[&routes].cached && !r.nodes[&clip].cached);

    // Run to here runs the node and what it needs, and keeps what is still up to date.
    let at = place_of(&h, "Clip routes", 0, true);
    right_click_at(&mut h, at);
    h.run_steps(3);
    h.get_by_label("Run to here").click();
    h.run_steps(2);
    wait_for_run(&mut h);
    let r = h.state().result.clone().unwrap();
    assert!(!r.nodes[&clip].cached);
    assert!(r.log.last().unwrap().msg.starts_with("Run finished: 5 nodes"), "{}", r.log.last().unwrap().msg);
}

#[test]
fn closing_lets_go_of_the_data_at_once() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    assert!(h.state().result.is_some());
    let start = std::time::Instant::now();
    h.state_mut().let_go_of_data();
    assert!(start.elapsed() < std::time::Duration::from_millis(50));
    assert!(h.state().result.is_none());
}

#[test]
fn cached_nodes_are_marked_and_the_command_line_is_copied() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    // Without a run, or without the cache, there are no marks.
    h.state_mut().use_cache = true;
    h.run_steps(3);
    assert!(h.query_by_label("cached").is_none());
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(h.query_all_by_label("cached").count(), 18, "every node that ran has its result in the cache");
    shot(&mut h, "197-cache-marks");
    // A changed setting makes the mark of that node, and those after it, hollow.
    let clip = h.state().snarl.nodes().find(|n| n.name == "Clip routes").unwrap().id.clone();
    let id = graph_id(&h, &clip);
    h.state_mut().snarl[id].params.insert("dummy".into(), serde_json::json!("2"));
    std::thread::sleep(std::time::Duration::from_millis(600));
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("cache out of date").count(), 3, "Clip routes, Calculate length and Routes in Skaraborg");
    assert_eq!(h.query_all_by_label("cached").count(), 15);
    shot(&mut h, "198-cache-out-of-date");
    // Without the cache there are no marks.
    h.state_mut().use_cache = false;
    h.run_steps(3);
    assert!(h.query_by_label("cached").is_none() && h.query_by_label("cache out of date").is_none());

    // The command line runs the saved file, with its variables.
    let mut empty = App::new();
    assert!(empty.command_line().is_none(), "an unsaved model has no command line");
    let file = h.state().path.clone().unwrap();
    h.state_mut().variables =
        vec![og_engine::Variable { name: "year".into(), value: "2026".into() }, og_engine::Variable { name: "place".into(), value: "Västra Götaland".into() }];
    let cmd = h.state().command_line().unwrap();
    assert!(cmd.contains(" run ") && cmd.contains(&*file.file_name().unwrap().to_string_lossy()), "{}", cmd);
    assert!(cmd.ends_with(" --var year=2026 --var \"place=Västra Götaland\""), "{}", cmd);
    h.get_by_label("Model").click();
    h.run_steps(3);
    h.get_by_label("Copy command line").click();
    h.run_steps(3);
    assert_eq!(h.state().last_command.as_deref(), Some(cmd.as_str()));
    empty.copy_command_line(&h.ctx.clone());
    assert!(empty.last_command.is_none());
}

#[test]
fn a_second_run_reuses_what_has_not_changed() {
    let dir = example_copy();
    std::fs::write(
        dir.join("cached.json"),
        r#"{"format": "ographis/1", "name": "Cached", "cache": true,
            "nodes": [{"id": "in", "type": "Reader", "name": "Towns", "x": 40, "y": 40, "params": {"path": "towns.csv"}},
                      {"id": "count", "type": "Counter", "name": "Count", "x": 300, "y": 40, "params": {}},
                      {"id": "out", "type": "Writer", "name": "Out", "x": 560, "y": 40, "params": {"path": "out/counted.csv"}}],
            "edges": [{"from": ["in", "Output"], "to": ["count", "Input"]}, {"from": ["count", "Output"], "to": ["out", "Input"]}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("cached.json")).unwrap();
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(3);
    let run = |h: &mut Harness<'_, App>, again: bool| {
        if again {
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F5);
        } else {
            h.key_press(egui::Key::F5);
        }
        for _ in 0..100 {
            h.run_steps(2);
            if !h.state().is_running() && h.state().result.as_ref().is_some_and(|r| !r.order.is_empty()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let r = h.state().result.clone().unwrap();
        (r.nodes["in"].cached, r.nodes["count"].cached, r.nodes["out"].cached)
    };
    assert_eq!(run(&mut h, false), (false, false, false), "the first run reads all");
    assert_eq!(run(&mut h, false), (true, true, false), "the second takes them from the cache, and the output writes");
    // A changed setting runs that node and what comes after it again.
    h.state_mut().snarl.nodes_mut().find(|n| n.id == "count").unwrap().params.insert("name".into(), serde_json::json!("n"));
    h.run_steps(2);
    assert_eq!(run(&mut h, false), (true, false, false));
    assert_eq!(run(&mut h, true), (false, false, false), "Ctrl+F5 reads everything again");
}

#[test]
fn a_new_tool_knows_the_attributes_without_a_new_run() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.state_mut().run_blocking();
    h.run_steps(3);
    h.state_mut().add_node("AttributeKeeper");
    settle(&mut h);
    assert!(h.query_by_label("population").is_none(), "nothing is connected yet");
    let app = h.state_mut();
    let src = app.snarl.node_ids().find(|(_, n)| n.id == "clip_towns").map(|(id, _)| id).unwrap();
    let out = app.snarl[src].outputs().iter().position(|p| p == "Inside").unwrap();
    let keep = app.snarl.node_ids().find(|(_, n)| n.kind == "AttributeKeeper").map(|(id, _)| id).unwrap();
    app.snarl.connect(egui_snarl::OutPinId { node: src, output: out }, egui_snarl::InPinId { node: keep, input: 0 });
    settle(&mut h);
    assert!(h.query_by_label("population").is_some(), "the attributes of the tool before are offered at once");
    shot(&mut h, "attributes-before-a-run");
}

#[test]
fn runs_are_kept_in_the_history_and_a_running_model_is_not_run_again() {
    let dir = example_copy();
    let model = dir.join("skaraborg.ographis");
    let mut app = App::new();
    app.load(&model).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let runs = og_engine::history::history(&model);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].outcome, "ok");
    assert_eq!(runs[0].by, "program");
    assert!(og_engine::history::holder(&model, 24.0).is_none(), "the lock is let go after the run");

    h.state_mut().history_open = true;
    h.run_steps(4);
    for label in ["Started", "Outcome", "Log and message", "program", "Open the folder"] {
        assert!(h.query_by_label_contains(label).is_some(), "no {} in the run history", label);
    }
    shot(&mut h, "history-01-window");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(!h.state().history_open);

    // Another run holds the lock: the program says so and does not run.
    let (user, computer) = og_engine::history::who();
    std::fs::write(og_engine::history::runs_dir(&model).join("lock"), format!("{};{};{};2026-10-22 08:00:00;og", std::process::id(), computer, user)).unwrap();
    h.state_mut().run_blocking();
    h.run_steps(3);
    assert_eq!(og_engine::history::history(&model).len(), 1, "a held lock stops the run");
    assert!(h.state().message().is_some_and(|m| m.contains("running already")), "{:?}", h.state().message());
    h.state_mut().history_open = true;
    h.run_steps(4);
    assert!(h.query_by_label_contains("Running now").is_some());
    shot(&mut h, "history-02-locked");
    h.get_by_label("Remove lock").click();
    h.run_steps(3);
    assert!(og_engine::history::holder(&model, 24.0).is_none());
    h.get_by_label("Close").click();
    h.run_steps(3);
    h.state_mut().run_blocking();
    settle(&mut h);
    assert_eq!(og_engine::history::history(&model).len(), 2);
}

#[test]
fn drawings_and_city_models_have_settings_of_their_own() {
    let dir = example_copy();
    let model = dir.join("readers.ographis");
    std::fs::write(
        &model,
        r#"{"format": "ographis/1", "name": "Readers", "nodes": [
        {"id": "plan", "type": "Reader", "name": "Plan", "x": 0, "y": 0, "params": {"path": "formats/plan.dxf", "crs": "EPSG:3006"}},
        {"id": "city", "type": "Reader", "name": "City", "x": 0, "y": 200, "params": {"path": "formats/blocks.gml"}}], "edges": []}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&model).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().focus_node("plan");
    h.run_steps(4);
    assert!(h.query_by_label_contains("Blocks").is_some());
    assert!(h.query_by_label_contains("Closed polylines").is_some());
    assert!(h.query_by_label_contains("Level of detail").is_none());
    shot(&mut h, "formats-01-dxf");
    h.state_mut().focus_node("city");
    h.run_steps(4);
    assert!(h.query_by_label_contains("Level of detail").is_some());
    assert!(h.query_by_label_contains("Closed polylines").is_none());
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert!(r.output("plan", "Output").map(|t| t.features.len()).unwrap_or(0) > 20);
    assert_eq!(r.output("city", "Output").unwrap().features.len(), 12);
    shot(&mut h, "formats-02-city");
}

#[test]
fn values_taken_from_the_data_split_the_stream_at_the_next_run() {
    let dir = example_copy();
    let model = dir.join("split.ographis");
    std::fs::write(
        &model,
        r#"{"format": "ographis/1", "name": "Split", "cache": true, "nodes": [
        {"id": "ifc", "type": "Reader", "name": "Villa", "x": 0, "y": 0, "params": {"path": "house/villa.ifc"}},
        {"id": "split", "type": "ValueFilter", "name": "Filter by value", "x": 300, "y": 0, "params": {"attribute": "IfcClass"}}],
        "edges": [{"from": ["ifc", "Output"], "to": ["split", "Input"]}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&model).unwrap();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.key_press(egui::Key::F5);
    wait_for_run(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.output("split", "Other").unwrap().len(), 17);
    h.state_mut().focus_node("split");
    h.run_steps(4);
    assert!(h.query_by_label_contains("before the settings were changed").is_none());
    h.get_by_label("Use the values in the data").click();
    h.run_steps(4);
    assert!(param(&h, "split", "values").contains("IfcWindow"), "{}", param(&h, "split", "values"));
    // The warning of the run before is still there, said to be from before the change.
    std::thread::sleep(std::time::Duration::from_millis(600));
    h.run_steps(4);
    assert!(h.query_by_label_contains("before the settings were changed").is_some());
    shot(&mut h, "split-warning-from-before");
    h.key_press(egui::Key::F5);
    wait_for_run(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.nodes["split"].warnings, Vec::<String>::new());
    assert_eq!(r.output("split", "Other").unwrap().len(), 0);
    assert_eq!(r.output("split", "IfcWindow").unwrap().len(), 7);
    h.run_steps(4);
    assert!(h.query_by_label_contains("No values are given").is_none());
    shot(&mut h, "split-by-ifc-class");
}

// ---------- Version 0.87.1 ----------

/// A run in the program keeps its log beside the model as it goes, line by line, so that
/// what happened is there to read also when the program stops in the middle of a run.
#[test]
fn a_run_in_the_program_writes_its_log_as_it_goes() {
    let (mut h, dir) = two_nodes();
    h.key_press(egui::Key::F5);
    wait_for_run(&mut h);
    let runs = og_engine::history::history(&dir.join("pair.json"));
    let last = runs.first().expect("the run is in the history");
    assert_eq!(last.by, "program");
    assert!(last.log.ends_with(".log"), "{:?}", last.log);
    let text = std::fs::read_to_string(dir.join("pair.runs").join("logs").join(&last.log)).unwrap();
    assert!(text.lines().next().unwrap().contains("INFO  Running pair.json in Ographis"), "{}", text);
    assert!(text.contains("Towns:") && text.contains("Result"), "{}", text);
}

// ---------- Version 0.88 ----------

/// A trial run, Ctrl+Shift+F5, reads at most the number of features of the settings from
/// every input, and says so in the log.
#[test]
fn a_trial_run_reads_the_first_features() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().settings.other.insert("trial_features".into(), serde_json::json!(2));
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::F5);
    wait_for_run(&mut h);
    let result = h.state().result.clone().expect("a result");
    let towns = result.output("towns", "Output").expect("towns");
    assert_eq!(towns.len(), 2);
    assert!(result.log.iter().any(|l| l.msg.contains("trial run") && l.msg.contains("first 2 features")));
    // A normal run afterwards reads them all.
    h.key_press(egui::Key::F5);
    wait_for_run(&mut h);
    assert!(h.state().result.clone().unwrap().output("towns", "Output").unwrap().len() > 2);
}
