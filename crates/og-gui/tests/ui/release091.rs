//! What came in 0.91: the canvas by the keyboard, finding and connecting nodes by name, the
//! map of the model, values suggested in expressions and the scheduled runs.

use crate::common::*;
use eframe::egui;

#[test]
fn the_canvas_is_worked_by_the_keyboard() {
    let (mut h, _dir) = two_nodes();
    // Remove the connection, to make it again by the keyboard.
    {
        let s = h.state_mut();
        let w: Vec<_> = s.snarl.wires().collect();
        for (o, i) in w {
            s.snarl.disconnect(o, i);
        }
    }
    h.run_steps(3);
    assert!(wires(&h).is_empty());
    // Tab goes from node to node, in reading order, and round again.
    h.key_press(egui::Key::Tab);
    h.run_steps(3);
    assert_eq!(selected(&h), ["in"]);
    h.key_press(egui::Key::Tab);
    h.run_steps(3);
    assert_eq!(selected(&h), ["out"]);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    h.run_steps(3);
    assert_eq!(selected(&h), ["in"]);
    // The arrows move it, further with Shift.
    let at = |h: &Harness<'_, og_gui::App>| h.state().snarl.nodes_pos_ids().find(|(_, _, n)| n.id == "in").unwrap().1;
    let before = at(&h);
    h.key_press(egui::Key::ArrowRight);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowDown);
    h.run_steps(3);
    assert_eq!(at(&h) - before, egui::vec2(12.0, 60.0));
    // Ctrl+L connects it to a port chosen by typing.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::L);
    h.run_steps(3);
    assert!(h.query_by_label("Connect Towns to…").is_some());
    h.get_all_by_role(egui::accesskit::Role::TextInput).last().unwrap().type_text("result");
    h.run_steps(2);
    shot(&mut h, "091-connect-to");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(wires(&h), ["in.Output > out.Input"]);
    // Ctrl+F finds a node by its name.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run_steps(3);
    h.get_all_by_role(egui::accesskit::Role::TextInput).last().unwrap().type_text("resu");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(selected(&h), ["out"]);
    assert!(h.state().picker.is_none());
    // A screen reader is told what a node is connected to.
    assert!(h.query_all_by_label_contains("From Towns Output into Input").count() > 0);
    // The map of the model is in the corner.
    assert!(h.query_by_label("A map of the whole model: click to go there").is_some());
}
