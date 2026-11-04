//! What came in 0.94.

use crate::common::*;

/// The settings window keeps its size and place whichever tab is chosen.
#[test]
fn the_settings_window_keeps_its_size_between_tabs() {
    let mut h = empty_app();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    let rect = |h: &Harness<'_, App>| h.ctx.memory(|m| m.area_rect(egui::Id::new("gb-settings-window"))).expect("the settings window is open");
    // With something in every tab, as on a computer in use: the forms are the widest part.
    for (tab, button) in [("Database connections", "New connection"), ("API connections", "New API connection"), ("Background maps", "New background map")] {
        h.get_by_label(tab).click();
        h.run_steps(3);
        h.get_by_label(button).click();
        h.run_steps(3);
    }
    h.get_by_label("General").click();
    h.run_steps(4);
    let first = rect(&h);
    for tab in ["Database connections", "API connections", "Background maps", "General"] {
        h.get_by_label(tab).click();
        h.run_steps(4);
        let r = rect(&h);
        println!("{}: {:?}", tab, r);
        assert!((r.width() - first.width()).abs() < 0.5 && (r.height() - first.height()).abs() < 0.5, "{} gave {:?}, General {:?}", tab, r, first);
        assert!((r.min - first.min).length() < 0.5, "{} moved the window: {:?} from {:?}", tab, r.min, first.min);
        if tab == "Database connections" {
            shot(&mut h, "94-settings-connection");
        }
    }
}

/// The wheel zooms around the place under the pointer, not the middle of the map.
#[test]
fn the_wheel_zooms_where_the_pointer_is() {
    let mut h = harness(egui::Theme::Light, &tile_server());
    h.run_steps(4);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("areas_sweref");
    settle(&mut h);
    // The map is prepared beside the window; wait until it is drawn on the tiles.
    for _ in 0..400 {
        if h.state().map.drawn().is_some() && h.state().map.shows_tiles() {
            break;
        }
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    settle(&mut h);
    let rect = h.state().map.fitted_rect();
    assert!(rect.is_positive());
    // A point up and to the left of the middle.
    let at = rect.center() + egui::vec2(-rect.width() / 4.0, -rect.height() / 4.0);
    h.event(egui::Event::PointerMoved(at));
    settle(&mut h);
    let before = h.state().map.world_at(at);
    println!("middle before {:?} tiles {} osm {} rect {:?}", h.state().map.world_at(rect.center()), h.state().map.shows_tiles(), h.state().map.osm, rect);
    for _ in 0..3 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 2.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(2);
    }
    settle(&mut h);
    let after = h.state().map.world_at(at);
    println!("after: tiles {} osm {} rect {:?}", h.state().map.shows_tiles(), h.state().map.osm, h.state().map.fitted_rect());
    let middle = h.state().map.world_at(rect.center());
    let moved = ((after[0] - before[0]).powi(2) + (after[1] - before[1]).powi(2)).sqrt();
    let span = (h.state().map.world_at(rect.right_top())[0] - h.state().map.world_at(rect.left_top())[0]).abs();
    println!("before {:?} after {:?} middle {:?} span {}", before, after, middle, span);
    assert!(span > 0.0);
    assert!(moved < span * 0.01, "the place under the pointer moved {} of a view {} wide", moved, span);
}

/// A node that is removed takes its data with it, so that a new node given the same id, such
/// as another Input, does not show what the old one read.
#[test]
fn a_removed_node_takes_its_data_with_it() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("towns");
    settle(&mut h);
    assert!(h.state().result.as_ref().unwrap().nodes.contains_key("towns"));
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    assert!(h.state().snarl.nodes().all(|n| n.id != "towns"), "the node is removed");
    assert!(!h.state().result.as_ref().unwrap().nodes.contains_key("towns"), "and what it gave in the run");
    assert_eq!(h.state().view.node, None);
}

/// In Manage attributes, several attributes are ticked and removed, or given a type, at once.
#[test]
fn several_attributes_are_managed_at_once() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("outside");
    settle(&mut h);
    assert!(h.query_by_label_contains("Tick attributes to keep, remove").is_some());
    // Two ticked, as by clicking their boxes.
    let id = egui::Id::new(("attributes chosen", "outside", "attributes"));
    h.ctx.data_mut(|d| d.insert_temp(id, vec!["municipality".to_string(), "population".to_string()]));
    h.run_steps(3);
    assert!(h.query_by_label("2 ticked:").is_some());
    shot(&mut h, "94-manage-attributes-ticked");
    h.get_all_by_label("Remove").next().unwrap().click();
    h.run_steps(3);
    let rules = h.state().snarl.nodes().find(|n| n.id == "outside").unwrap().params["attributes"].clone();
    let action = |name: &str| rules.as_array().unwrap().iter().find(|r| r["name"] == name).map(|r| r["action"].clone());
    assert_eq!(action("municipality"), Some(serde_json::json!("remove")));
    assert_eq!(action("population"), Some(serde_json::json!("remove")));
    assert_ne!(action("name"), Some(serde_json::json!("remove")), "what is not ticked stays");
    // And kept again, made text.
    h.get_by_label("Keep").click();
    h.run_steps(3);
    h.get_by_value("Type").click();
    h.run_steps(3);
    h.get_by_label("text").click();
    h.run_steps(3);
    let rules = h.state().snarl.nodes().find(|n| n.id == "outside").unwrap().params["attributes"].clone();
    let row = |name: &str| rules.as_array().unwrap().iter().find(|r| r["name"] == name).cloned().unwrap();
    assert_ne!(row("population")["action"], "remove", "kept again");
    assert_eq!(row("population")["type"], "text");
    assert_eq!(row("municipality")["type"], serde_json::Value::Null, "already text, so as it comes");
}
