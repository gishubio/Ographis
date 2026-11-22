//! What came in 0.96.6: going from error to error, and to what is wrong.

use crate::common::*;
use og_gui::App;

/// F8 goes from one failed node to the next and Shift+F8 back, a click on the errors in the
/// top bar goes to the first, and the form of a failed node has a button to the setting.
#[test]
fn errors_are_gone_through_and_lead_to_the_setting() {
    let dir = example_copy();
    std::fs::write(
        dir.join("errors.json"),
        r#"{
      "format": "ographis/1", "name": "Errors",
      "nodes": [
        {"id": "good", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "a", "type": "Reader", "name": "Lost A", "x": 0, "y": 200, "params": {"path": "nowhere_a.csv"}},
        {"id": "b", "type": "Reader", "name": "Lost B", "x": 0, "y": 400, "params": {"path": "nowhere_b.csv"}}
      ],
      "edges": []
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("errors.json")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    let failed = h.state().error_nodes();
    assert_eq!(failed.len(), 2, "{:?}", failed);
    let (a, b) = (graph_id(&h, &failed[0]), graph_id(&h, &failed[1]));
    // The errors in the top bar go to the first.
    h.get_by_label("2 errors in the run").click();
    settle(&mut h);
    assert_eq!(h.state().selected, vec![a]);
    h.key_press(egui::Key::F8);
    settle(&mut h);
    assert_eq!(h.state().selected, vec![b]);
    assert!(h.state().message().is_some_and(|m| m.starts_with("Error 2 of 2")), "{:?}", h.state().message());
    // Round to the first again, and back.
    h.key_press(egui::Key::F8);
    settle(&mut h);
    assert_eq!(h.state().selected, vec![a]);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F8);
    settle(&mut h);
    assert_eq!(h.state().selected, vec![b]);
    // The file cannot be found, so the button leads to the path.
    h.get_by_label("Go to File or folder").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Go to File or folder").click();
    h.run_steps(2);
    let marked: Option<(String, String, f64, bool)> = h.ctx.data(|d| d.get_temp(egui::Id::new("focus-setting")));
    assert_eq!(marked.map(|m| m.1), Some("path".to_string()));
    shot(&mut h, "966-error-leads-to-the-setting");
}

/// F1 opens the page of the selected tool, made from what the tool says of itself; Help in the
/// menu opens getting started, and the tools can be searched.
#[test]
fn the_help_has_a_page_for_each_tool() {
    let dir = example_copy();
    let mut app = App::new();
    app.load(&dir.join("statistics.ographis")).unwrap();
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    let classes = graph_id(&h, "classes");
    h.state_mut().set_selection(vec![classes]);
    settle(&mut h);
    h.key_press(egui::Key::F1);
    settle(&mut h);
    assert!(h.state().help.is_some());
    assert!(h.query_all_by_label("Classify values").count() >= 2, "the form and the page");
    assert!(h.query_all_by_label_contains("Natural breaks (Jenks)").next().is_some(), "the choices of a setting are listed");
    assert!(h.query_by_label("statistics").is_some(), "the examples that use it");
    shot(&mut h, "966-help-for-a-tool");
    // Getting started, from the menu, and onwards.
    h.state_mut().open_help(None);
    settle(&mut h);
    assert_eq!(h.query_all_by_label("What Ographis does").count(), 2, "in the contents and as the heading");
    h.get_by_label("Your first model →").click();
    settle(&mut h);
    assert!(h.query_all_by_label_contains("Press F5 to run").next().is_some());
    shot(&mut h, "966-help-getting-started");
}
