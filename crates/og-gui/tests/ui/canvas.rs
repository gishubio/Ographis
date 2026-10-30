//! The canvas: nodes, connections and their bend points, ports, group boxes and notes,
//! selecting, moving, copying and undoing.

use crate::common::*;

#[test]
fn box_selection_moves_duplicates_and_deletes_together() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let bottom_row = ["clip_routes", "length", "out_routes", "routes"];

    // Dragging on empty canvas draws a box. The bottom row of the workspace is inside it.
    let (from, to) = (on_canvas(&h, 2.0, 468.0), on_canvas(&h, 1330.0, 650.0));
    let transform_before = on_canvas(&h, 0.0, 0.0);
    pointer(&mut h, from, None);
    pointer(&mut h, from, Some((egui::PointerButton::Primary, true)));
    pointer(&mut h, from + (to - from) * 0.5, None);
    pointer(&mut h, to, None);
    shot(&mut h, "80-selection-box");
    pointer(&mut h, to, Some((egui::PointerButton::Primary, false)));
    h.run_steps(2);
    assert_eq!(selected(&h), bottom_row);
    assert_eq!(on_canvas(&h, 0.0, 0.0), transform_before, "dragging on empty canvas must not move the canvas");
    assert!(h.state().focus.is_none(), "with several nodes selected, no single node has its parameters shown");
    assert!(!h.state().dirty, "selecting is not a change");
    assert!(h.query_by_label("4 nodes selected").is_some());
    shot(&mut h, "81-four-selected");

    // Dragging one of the selected nodes moves all of them by the same amount.
    let place = |h: &Harness<'_, App>, id: &str| h.state().node_rect(id).unwrap().min;
    let start: Vec<egui::Pos2> = bottom_row.iter().map(|id| place(&h, id)).collect();
    let other = place(&h, "towns");
    let grab = place_of(&h, "Routes", 0, true);
    drag(&mut h, grab, grab + egui::vec2(60.0, -30.0), egui::PointerButton::Primary);
    h.run_steps(2);
    let scale = h.state().canvas_scale();
    for (id, was) in bottom_row.iter().zip(&start) {
        let moved = place(&h, id) - *was;
        assert!((moved - egui::vec2(60.0, -30.0) / scale).length() < 1.5, "{} moved by {:?}", id, moved);
    }
    assert_eq!(place(&h, "towns"), other, "a node outside the selection stays where it was");
    assert!(h.state().dirty);
    assert_eq!(selected(&h), bottom_row, "the selection survives the move");

    // Ctrl+D duplicates the four nodes with the three connections between them, and
    // selects the copies.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    h.run_steps(3);
    let ws = h.state().workspace();
    assert_eq!((ws.nodes.len(), ws.edges.len()), (22, 20));
    assert_eq!(selected(&h), ["clipper_1", "lengthcalculator_1", "reader_1", "writer_1"]);
    shot(&mut h, "82-duplicated");

    // Delete removes the selected copies and nothing else.
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    let ws = h.state().workspace();
    assert_eq!((ws.nodes.len(), ws.edges.len()), (18, 17));
    assert!(selected(&h).is_empty());

    // Ctrl+A selects everything, Escape nothing.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(3);
    assert_eq!(selected(&h).len(), 18);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(selected(&h).is_empty());

    // Shift adds to the selection, and a click on empty canvas clears it.
    click_text(&mut h, "Towns", 0, true);
    assert_eq!(selected(&h), ["towns"]);
    let second = place_of(&h, "Subregions", 0, true);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    h.event(egui::Event::PointerMoved(second));
    h.event(egui::Event::PointerButton { pos: second, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::SHIFT });
    h.run_steps(2);
    h.event(egui::Event::PointerButton { pos: second, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::SHIFT });
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::default()));
    h.run_steps(3);
    assert_eq!(selected(&h), ["subregions", "towns"]);
    let empty = on_canvas(&h, 2.0, 468.0);
    click_at(&mut h, empty);
    h.run_steps(2);
    assert!(selected(&h).is_empty());
}

#[test]
fn panning_zooming_and_zoom_to_fit() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let home = (on_canvas(&h, 0.0, 0.0), h.state().canvas_scale());
    let empty = on_canvas(&h, 2.0, 468.0);

    // Space + drag moves the canvas, and so does the middle mouse button.
    h.key_down(egui::Key::Space);
    h.run_steps(1);
    drag(&mut h, empty, empty + egui::vec2(120.0, 40.0), egui::PointerButton::Primary);
    h.key_up(egui::Key::Space);
    h.run_steps(2);
    let after_space = on_canvas(&h, 0.0, 0.0);
    assert!((after_space - home.0 - egui::vec2(120.0, 40.0)).length() < 1.0, "moved by {:?}", after_space - home.0);
    assert!(selected(&h).is_empty(), "panning selects nothing");
    let grip = on_canvas(&h, 2.0, 468.0);
    drag(&mut h, grip, grip + egui::vec2(-300.0, -200.0), egui::PointerButton::Middle);
    h.run_steps(2);
    let after_middle = on_canvas(&h, 0.0, 0.0);
    assert!((after_middle - after_space - egui::vec2(-300.0, -200.0)).length() < 1.0, "moved by {:?}", after_middle - after_space);

    // The wheel zooms around the pointer, without Ctrl.
    let at = h.state().canvas_rect().center();
    let under = h.state().graph_to_screen(egui::Pos2::ZERO);
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(2);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 80.0),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    });
    h.run_steps(8);
    let zoomed = h.state().canvas_scale();
    assert!(zoomed > home.1 * 1.2, "scale went from {} to {}", home.1, zoomed);
    // The point under the pointer stays put, so the origin moves away from it.
    let origin = h.state().graph_to_screen(egui::Pos2::ZERO);
    assert!(((origin - at).length() / (under - at).length() - zoomed / home.1).abs() < 0.02);
    assert!(!h.state().dirty, "moving the view is not a change to the workspace");
    shot(&mut h, "85-zoomed-in");

    // The button is clicked with a real mouse click, since it once sat under the canvas
    // where no click could reach it.
    click_text(&mut h, "Zoom to fit", 0, false);
    h.run_steps(4);
    let app = h.state();
    let canvas = app.canvas_rect();
    for n in app.snarl.nodes() {
        let r = app.node_rect(&n.id).unwrap();
        let on_screen = egui::Rect::from_two_pos(app.graph_to_screen(r.min), app.graph_to_screen(r.max));
        assert!(canvas.expand(1.0).contains_rect(on_screen), "{} is outside the canvas after Zoom to fit: {:?}", n.id, on_screen);
    }
    assert!(app.canvas_scale() <= 1.0);
    shot(&mut h, "86-zoomed-to-fit");
}

#[test]
fn group_boxes_hold_nodes_together() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let bottom_row = ["clip_routes", "length", "out_routes", "routes"];
    let place = |h: &Harness<'_, App>, id: &str| h.state().node_rect(id).unwrap().min;
    let box_of = |h: &Harness<'_, App>, i: usize| og_gui::graph::group_rect(&h.state().groups[i]);

    // Select the bottom row with a box, and put a group around it with Ctrl+G.
    let (from, to) = (on_canvas(&h, 2.0, 468.0), on_canvas(&h, 1330.0, 650.0));
    drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(selected(&h), bottom_row);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::G);
    h.run_steps(3);
    assert_eq!(h.state().groups.len(), 1);
    assert_eq!(h.state().selected_group, Some(0));
    assert!(selected(&h).is_empty(), "the group is what is selected now");
    assert!(h.state().dirty && h.state().groups[0].locked);
    for id in bottom_row {
        assert!(box_of(&h, 0).contains_rect(h.state().node_rect(id).unwrap()), "{} should be inside the box", id);
    }
    assert_eq!(h.state().nodes_in_group(0).len(), 4);
    assert!(h.query_by_label("4 nodes in the box").is_some());

    // Give it a name and a colour, here straight in the state.
    h.state_mut().groups[0].name = "Routes".into();
    h.state_mut().groups[0].color = "green".into();
    h.run_steps(2);
    shot(&mut h, "105-group");
    // More colours to choose from, and one of its own.
    h.get_by_value("Green").click();
    h.run_steps(3);
    for c in ["Orange", "Red", "Purple", "Teal", "Brown"] {
        assert!(h.query_by_label(c).is_some(), "{} should be offered", c);
    }
    shot(&mut h, "106-group-colours");
    h.get_by_label("Teal").click();
    h.run_steps(3);
    assert_eq!(h.state().groups[0].color, "teal");
    h.state_mut().groups[0].color = "#8a2be2".into();
    h.run_steps(3);
    assert!(h.query_by_value("A colour of its own").is_some());
    shot(&mut h, "106-group-own-colour");
    h.state_mut().groups[0].color = "green".into();
    h.run_steps(2);

    // Dragging the box by its name moves the nodes in it, and nothing else.
    let scale = h.state().canvas_scale();
    let (before, nodes_before) = (box_of(&h, 0), bottom_row.map(|id| place(&h, id)));
    let other = place(&h, "towns");
    let grab = h.state().graph_to_screen(before.min + egui::vec2(before.width() / 2.0, 13.0));
    drag(&mut h, grab, grab + egui::vec2(60.0, -24.0), egui::PointerButton::Primary);
    h.run_steps(2);
    let by = egui::vec2(60.0, -24.0) / scale;
    assert!((box_of(&h, 0).min - before.min - by).length() < 1.5, "the box moved by {:?}", box_of(&h, 0).min - before.min);
    for (id, was) in bottom_row.iter().zip(&nodes_before) {
        assert!((place(&h, id) - *was - by).length() < 1.5, "{} moved by {:?}", id, place(&h, id) - *was);
    }
    assert_eq!(place(&h, "towns"), other);

    // Unlocked, the box moves alone.
    h.get_by_label("Lock the contents").click();
    h.run_steps(3);
    assert!(!h.state().groups[0].locked);
    let (before, nodes_before) = (box_of(&h, 0), bottom_row.map(|id| place(&h, id)));
    // By the title bar, to the left, where no node lies over the box after the move above.
    let grab = h.state().graph_to_screen(before.min + egui::vec2(120.0, 13.0));
    drag(&mut h, grab, grab + egui::vec2(-30.0, 12.0), egui::PointerButton::Primary);
    h.run_steps(2);
    assert!((box_of(&h, 0).min - before.min - egui::vec2(-30.0, 12.0) / scale).length() < 1.5);
    assert_eq!(bottom_row.map(|id| place(&h, id)), nodes_before);

    // The lower right corner resizes.
    let before = box_of(&h, 0);
    let corner = h.state().graph_to_screen(before.max - egui::vec2(6.0, 6.0));
    drag(&mut h, corner, corner + egui::vec2(40.0, 30.0), egui::PointerButton::Primary);
    h.run_steps(2);
    let grown = box_of(&h, 0).size() - before.size();
    assert!((grown - egui::vec2(40.0, 30.0) / scale).length() < 1.5, "the box grew by {:?}", grown);
    assert_eq!(box_of(&h, 0).min, before.min);

    // Dragging inside a box still draws a selection box, since only the name is a handle.
    let inside = h.state().graph_to_screen(box_of(&h, 0).max - egui::vec2(30.0, 30.0));
    let to = h.state().graph_to_screen(place(&h, "out_routes") + egui::vec2(20.0, 20.0));
    drag(&mut h, inside, to, egui::PointerButton::Primary);
    h.run_steps(2);
    assert_eq!(selected(&h), ["out_routes"]);
    assert_eq!(h.state().selected_group, None);

    // The box is saved with the workspace.
    let file = scratch("groups").join("grouped.json");
    h.state_mut().save_to(&file);
    let saved = og_engine::Workspace::load(&file).unwrap();
    assert_eq!((saved.groups.len(), saved.groups[0].name.as_str(), saved.groups[0].color.as_str(), saved.groups[0].locked), (1, "Routes", "green", false));
    let mut again = App::new();
    again.load(&file).unwrap();
    assert_eq!(again.groups, h.state().workspace().groups);
    assert_eq!(again.groups[0].x.fract(), 0.0, "the box is stored in whole units");

    // A click on the name selects the box, and Delete removes it. The nodes stay.
    let name = h.state().graph_to_screen(box_of(&h, 0).min + egui::vec2(40.0, 13.0));
    click_at(&mut h, name);
    assert_eq!(h.state().selected_group, Some(0));
    assert!(selected(&h).is_empty());
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    assert!(h.state().groups.is_empty());
    assert_eq!(h.state().snarl.nodes().count(), 18);
}

#[test]
fn comments_on_nodes_and_free_standing_notes() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);

    // A comment is written in the form of the node and shown under the node.
    h.state_mut().focus_node("towns");
    h.run_steps(3);
    let field = h.get_by_label("Comment").rect().center() + egui::vec2(40.0, 26.0);
    type_at(&mut h, field, "From the county, updated yearly");
    let comment = |h: &Harness<'_, App>| h.state().snarl.nodes().find(|n| n.id == "towns").unwrap().comment.clone();
    assert_eq!(comment(&h), "From the county, updated yearly");
    assert!(h.query_by_label("From the county, updated yearly").is_some(), "the comment is shown on the canvas");
    assert!(h.state().dirty && !h.state().stale, "a comment is saved but does not change the result");

    // A note is added from the Edit menu and written in the panel.
    h.get_by_label("Edit").click();
    h.run_steps(3);
    h.get_by_label("Add a note").click();
    h.run_steps(4);
    assert_eq!(h.state().snarl.nodes().count(), 19);
    let note = h.state().snarl.nodes().find(|n| n.is_note()).unwrap().id.clone();
    assert_eq!(selected(&h), std::slice::from_ref(&note));
    assert!(h.query_by_label("An empty note. Write the text in the panel on the right.").is_some());
    let field = h.get_by_label("Text").rect().center() + egui::vec2(40.0, 30.0);
    type_at(&mut h, field, "Ask Lena about the codes");
    assert!(h.query_by_label("Ask Lena about the codes").is_some(), "the note is shown on the canvas");
    assert!(!h.state().stale);
    shot(&mut h, "106-comment-and-note");

    // The run does not see the note.
    h.state_mut().run_blocking();
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.nodes.len(), r.errors), (18, 0));

    // Both are kept in the workspace file, and the note is not a node there.
    let ws = h.state().workspace();
    assert_eq!((ws.nodes.len(), ws.notes.len()), (18, 1));
    assert_eq!(ws.notes[0].text, "Ask Lena about the codes");
    assert_eq!(ws.nodes.iter().find(|n| n.id == "towns").unwrap().comment, "From the county, updated yearly");
    let file = scratch("notes").join("noted.json");
    h.state_mut().save_to(&file);
    let mut again = App::new();
    again.load(&file).unwrap();
    assert_eq!(again.snarl.nodes().filter(|n| n.is_note()).map(|n| n.comment.clone()).collect::<Vec<_>>(), ["Ask Lena about the codes"]);
    assert_eq!(again.snarl.nodes().find(|n| n.id == "towns").unwrap().comment, "From the county, updated yearly");

    // Delete removes the note like any node. The text field has to let go of the keyboard first.
    let empty = on_canvas(&h, 600.0, -40.0);
    click_at(&mut h, empty);
    h.state_mut().focus_node(&note);
    h.run_steps(3);
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    assert_eq!(h.state().snarl.nodes().count(), 18);
    assert!(!h.state().stale, "removing a note does not change the result");
}

#[test]
fn attributes_fold_out_on_the_nodes() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    assert!(h.query_by_label("▸ 5 attributes").is_none(), "before a run nothing is known about the attributes");
    h.state_mut().run_blocking();
    settle(&mut h);

    // Every node has a small handle with the number of attributes after it.
    assert!(h.state().expanded.is_empty());
    assert!(h.query_all_by_label("▸ 5 attributes").count() >= 2);
    assert!(h.query_by_label("pop_thousands decimal").is_none());
    // The join adds an attribute, and the calculation after it one more.
    click_text(&mut h, "▸ 7 attributes", 0, true);
    h.run_steps(3);
    assert_eq!(h.state().expanded.len(), 1);
    assert!(h.query_by_label("pop_thousands decimal").is_some(), "the list is folded out on the node, with the type of each attribute");
    assert!(!h.state().dirty, "folding out is not a change to the workspace");
    shot(&mut h, "107-attributes");
    click_text(&mut h, "▾ 7 attributes", 0, true);
    h.run_steps(3);
    assert!(h.state().expanded.is_empty() && h.query_by_label("pop_thousands decimal").is_none());

    // All at once from the View menu.
    h.get_by_label("View").click();
    h.run_steps(3);
    h.get_by_label("Show the attributes on all nodes").click();
    h.run_steps(6);
    assert_eq!(h.state().expanded.len(), 18);
    h.state_mut().fit_to_view();
    h.run_steps(4);
    shot(&mut h, "108-all-attributes");
    h.get_by_label("View").click();
    h.run_steps(3);
    h.get_by_label("Hide the attributes on all nodes").click();
    h.run_steps(3);
    assert!(h.state().expanded.is_empty());

    // The panel of a node lists them too. A node with ports that differ lists each port.
    h.state_mut().focus_node("join");
    h.run_steps(3);
    h.get_by_label("Attributes after this step").click();
    h.run_steps(3);
    for port in ["Merged", "UnmergedRequestor", "UnusedSupplier"] {
        assert!(h.query_all_by_label(port).count() >= 2, "{} should be a heading in the list as well as a port", port);
    }
    assert!(h.query_all_by_label("muni_code").count() >= 1);
}

#[test]
fn changes_are_undone_and_redone() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let place = |h: &Harness<'_, App>, id: &str| h.state().snarl.nodes_pos_ids().find(|(_, _, n)| n.id == id).map(|(_, pos, _)| pos).unwrap();
    assert!(!h.state().can_undo() && !h.state().can_redo(), "a workspace that was just opened has nothing to undo");

    // A node is moved, and moved back with Ctrl+Z.
    let start = place(&h, "towns");
    let grab = place_of(&h, "Towns", 0, true);
    drag(&mut h, grab, grab + egui::vec2(80.0, 50.0), egui::PointerButton::Primary);
    h.run_steps(3);
    let moved = place(&h, "towns");
    assert!((moved - start).length() > 50.0);
    assert!(h.state().can_undo());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(place(&h, "towns"), start, "the whole drag is one step");
    assert!(!h.state().can_undo() && h.state().can_redo());
    assert_eq!(selected(&h), ["towns"], "what was selected stays selected");
    // Ctrl+Y does it again.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
    h.run_steps(3);
    assert!((place(&h, "towns") - moved).length() < 1.0);
    assert!(!h.state().can_redo());

    // Deleting a node takes its connections with it, and undo brings both back.
    h.state_mut().run_blocking();
    h.state_mut().focus_node("join");
    h.run_steps(3);
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    assert_eq!((h.state().snarl.nodes().count(), wires(&h).len()), (17, 14));
    h.get_by_label("Undo").click();
    h.run_steps(3);
    assert_eq!((h.state().snarl.nodes().count(), wires(&h).len()), (18, 17));
    assert_eq!(param(&h, "join", "requestor_key"), "municipality");
    assert!(h.state().result.is_some() && h.state().stale, "the result of the latest run is kept, marked as out of date");

    // What is typed in a field is one step, taken in when the field is left.
    h.state_mut().focus_node("thousands");
    h.run_steps(3);
    let field = h.get_all_by_value("pop_thousands").next().unwrap().rect().center();
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("thousands".into()));
    h.run_steps(3);
    assert_eq!(param(&h, "thousands", "name"), "thousands");
    let empty = on_canvas(&h, 600.0, -40.0);
    click_at(&mut h, empty);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(param(&h, "thousands", "name"), "pop_thousands");
    // Redo by the button, and Ctrl+Shift+Z works like Ctrl+Y.
    h.get_by_label("Redo").click();
    h.run_steps(3);
    assert_eq!(param(&h, "thousands", "name"), "thousands");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(param(&h, "thousands", "name"), "thousands");

    // A new change after an undo ends what could be redone.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert!(h.state().can_redo());
    h.state_mut().add_node("Sorter");
    h.run_steps(3);
    assert!(!h.state().can_redo());
    // Several steps back, one at a time, all the way to how the file was opened.
    for _ in 0..10 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run_steps(2);
    }
    assert!(!h.state().can_undo());
    assert_eq!(h.state().snarl.nodes().count(), 18);
    assert_eq!(place(&h, "towns"), start);
    assert_eq!(param(&h, "thousands", "name"), "pop_thousands");
}

#[test]
fn a_tool_dropped_on_a_connection_goes_in_between() {
    let (mut h, _dir) = two_nodes();
    assert_eq!(wires(&h), ["in.Output > out.Input"]);

    // A new tool lands somewhere free. It is dragged onto the connection.
    h.state_mut().add_node("Sorter");
    h.run_steps(4);
    let middle = between(&h, ("in", "Output"), ("out", "Input"));
    let rect = h.state().node_rect("sorter_1").unwrap();
    let centre = h.state().graph_to_screen(rect.center());
    let grab = place_of(&h, "Sort", 0, true);
    // Half way there the connection is drawn heavier, to show what will happen.
    pointer(&mut h, grab, None);
    pointer(&mut h, grab, Some((egui::PointerButton::Primary, true)));
    pointer(&mut h, grab + (middle - centre) * 0.5, None);
    pointer(&mut h, grab + (middle - centre), None);
    shot(&mut h, "131-over-a-connection");
    pointer(&mut h, grab + (middle - centre), Some((egui::PointerButton::Primary, false)));
    h.run_steps(3);
    assert_eq!(wires(&h), ["in.Output > sorter_1.Input", "sorter_1.Sorted > out.Input"]);
    assert_eq!(h.state().message(), Some("Put Sort between Towns and Result"));
    assert!(h.state().dirty);
    shot(&mut h, "132-put-in-between");

    // Undo takes the tool out again and puts it back where it was.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(wires(&h), ["in.Output > out.Input"]);
    assert!((h.state().node_rect("sorter_1").unwrap().min - rect.min).length() < 1.0);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
    h.run_steps(3);
    assert_eq!(wires(&h).len(), 2);

    // A tool that is already connected is only moved, and so is one that is let go of elsewhere.
    h.state_mut().add_node("Tester");
    h.run_steps(4);
    let middle = between(&h, ("sorter_1", "Sorted"), ("out", "Input"));
    let centre = h.state().graph_to_screen(h.state().node_rect("tester_1").unwrap().center());
    let grab = place_of(&h, "Test", 0, true);
    drag(&mut h, grab, grab + egui::vec2(0.0, 260.0), egui::PointerButton::Primary);
    h.run_steps(3);
    assert_eq!(wires(&h).len(), 2);
    // A reader has no input, so it cannot go into a connection.
    h.state_mut().add_node("Reader");
    h.run_steps(4);
    let reader = h.state().graph_to_screen(h.state().node_rect("reader_1").unwrap().center());
    // The name of the new input is also the name of ports, so it is grabbed by its title bar.
    let grab = h.state().graph_to_screen(h.state().node_rect("reader_1").unwrap().center_top() + egui::vec2(0.0, 8.0));
    drag(&mut h, grab, grab + (middle - reader), egui::PointerButton::Primary);
    h.run_steps(3);
    assert_eq!(wires(&h).len(), 2);
    let _ = centre;
}

#[test]
fn a_connection_is_clicked_and_deleted() {
    let (mut h, _dir) = two_nodes();
    let middle = between(&h, ("in", "Output"), ("out", "Input"));

    // A click on the connection selects it. The panel says what it connects.
    click_at(&mut h, middle);
    h.run_steps(3);
    assert!(h.state().selected_wire.is_some(), "the connection should be selected");
    assert!(selected(&h).is_empty());
    for label in ["Connection", "Towns · Output", "Result · Input", "Delete connection"] {
        assert!(h.query_by_label(label).is_some(), "the panel should say: {}", label);
    }
    shot(&mut h, "133-connection-selected");
    // A click on empty canvas lets go of it, and so does Escape.
    let empty = on_canvas(&h, 350.0, 300.0);
    click_at(&mut h, empty);
    assert!(h.state().selected_wire.is_none());
    click_at(&mut h, middle);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().selected_wire.is_none());
    assert_eq!(wires(&h).len(), 1);

    // Delete removes the selected connection, and nothing else.
    click_at(&mut h, middle);
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    assert!(wires(&h).is_empty());
    assert_eq!(h.state().snarl.nodes().count(), 2);
    assert!(h.state().dirty && h.state().selected_wire.is_none());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(wires(&h), ["in.Output > out.Input"]);
    // The button in the panel does the same.
    click_at(&mut h, middle);
    h.run_steps(2);
    h.get_by_label("Delete connection").click();
    h.run_steps(3);
    assert!(wires(&h).is_empty());
    // Selecting a node lets go of a connection.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    click_at(&mut h, middle);
    click_text(&mut h, "Towns", 0, true);
    assert!(h.state().selected_wire.is_none());
    assert_eq!(selected(&h), ["in"]);
}

#[test]
fn a_node_is_disabled_from_its_menu() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let disabled = |h: &Harness<'_, App>| -> Vec<String> {
        let mut ids: Vec<String> = h.state().snarl.nodes().filter(|n| n.disabled).map(|n| n.id.clone()).collect();
        ids.sort();
        ids
    };

    // Right-click the node and choose Disable.
    let node = place_of(&h, "Clip routes", 0, true);
    right_click_at(&mut h, node);
    h.get_by_label("Disable").click();
    h.run_steps(3);
    assert_eq!(disabled(&h), ["clip_routes"]);
    assert!(h.state().dirty);
    assert!(h.query_by_label("disabled").is_some(), "the node says that it is disabled");

    // It does not run, and what it alone feeds rests. The rest of the workspace runs.
    h.state_mut().run_blocking();
    h.state_mut().focus_node("clip_routes");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.errors, r.warnings), (0, 0), "{:?}", r.log);
    assert_eq!(r.nodes["clip_routes"].status, og_engine::Status::Disabled);
    assert_eq!(r.nodes["length"].status, og_engine::Status::Skipped);
    assert_eq!(r.nodes["out_routes"].status, og_engine::Status::Skipped);
    assert_eq!(r.nodes["out_towns"].status, og_engine::Status::Ok);
    assert!(h.query_by_label("This node is disabled. It does not run and lets nothing through.").is_some());
    shot(&mut h, "134-disabled");

    // It is saved with the workspace.
    assert!(h.state().workspace().nodes.iter().find(|n| n.id == "clip_routes").unwrap().disabled);
    // The same menu enables it again, and so does the button in its form.
    let node = place_of(&h, "Clip routes", 0, true);
    right_click_at(&mut h, node);
    // The form has a button with the same name. The menu is drawn after it.
    h.get_all_by_label("Enable").last().unwrap().click();
    h.run_steps(3);
    assert!(disabled(&h).is_empty());
    h.state_mut().toggle_disabled(&[]);
    assert!(disabled(&h).is_empty());
    let node = h.state().focus.unwrap();
    h.state_mut().toggle_disabled(&[node]);
    h.run_steps(3);
    h.get_by_label("Enable").click();
    h.run_steps(3);
    assert!(disabled(&h).is_empty(), "the button in the form enables the node");

    // With several nodes selected the menu acts on all of them.
    let (from, to) = (on_canvas(&h, 2.0, 468.0), on_canvas(&h, 1330.0, 650.0));
    drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(selected(&h).len(), 4);
    let node = place_of(&h, "Routes", 0, true);
    right_click_at(&mut h, node);
    h.get_by_label("Disable selected").click();
    h.run_steps(3);
    assert_eq!(disabled(&h), ["clip_routes", "length", "out_routes", "routes"]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert!(disabled(&h).is_empty(), "disabling is undone like any other change");
}

// ---------- Version 0.52 ----------

#[test]
fn group_boxes_follow_when_what_is_in_them_is_dragged() {
    let mut h = harness_with("stops.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let nodes = ["stops", "municipalities", "code", "join", "id", "nospace", "stamp", "tidy", "out_xlsx", "out_gpkg"];
    let places = |h: &Harness<'_, App>| nodes.map(|id| h.state().node_rect(id).unwrap().min);
    let boxes = |h: &Harness<'_, App>| h.state().groups.iter().map(og_gui::graph::group_rect).collect::<Vec<_>>();
    let inside = |h: &Harness<'_, App>| (0..3).map(|g| h.state().nodes_in_group(g).len()).collect::<Vec<_>>();
    assert_eq!(inside(&h), [2, 6, 2]);
    let scale = h.state().canvas_scale();

    // Ctrl+A and a drag moves precisely everything: the nodes, the note and the boxes.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(3);
    assert_eq!(h.state().selected.len(), 11, "ten nodes and a note");
    assert_eq!(h.state().boxes_with_selection(), [0, 1, 2]);
    let (nodes_before, boxes_before) = (places(&h), boxes(&h));
    let grab = place_of(&h, "Municipality name", 0, true);
    drag(&mut h, grab, grab + egui::vec2(90.0, 40.0), egui::PointerButton::Primary);
    h.run_steps(3);
    let by = egui::vec2(90.0, 40.0) / scale;
    let moved = places(&h)[0] - nodes_before[0];
    assert!((moved - by).length() < 1.5, "the nodes moved by {:?}", moved);
    for (id, (now, was)) in nodes.iter().zip(places(&h).iter().zip(&nodes_before)) {
        assert!((*now - *was - moved).length() < 0.05, "{} moved by {:?}", id, *now - *was);
    }
    for (i, (now, was)) in boxes(&h).iter().zip(&boxes_before).enumerate() {
        assert!((now.min - was.min - moved).length() < 0.05, "box {} moved by {:?} and the nodes by {:?}", i, now.min - was.min, moved);
        assert!((now.size() - was.size()).length() < 0.01, "a box keeps its size: {:?} {:?}", now.size(), was.size());
    }
    assert_eq!(inside(&h), [2, 6, 2], "every node is still in its box");
    assert!(h.state().dirty);
    shot(&mut h, "140-everything-moved");
    // The whole move is one step to undo, boxes and all.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(places(&h), nodes_before);
    assert_eq!(boxes(&h), boxes_before);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(selected(&h).is_empty());

    // With all that is in one box selected, that box follows and the others stay.
    let (from, to) = (on_canvas(&h, 6.0, 56.0), on_canvas(&h, 244.0, 444.0));
    drag(&mut h, from, to, egui::PointerButton::Primary);
    h.run_steps(2);
    assert_eq!(selected(&h), ["municipalities", "stops"]);
    assert_eq!(h.state().boxes_with_selection(), [0]);
    let (nodes_before, boxes_before) = (places(&h), boxes(&h));
    let grab = place_of(&h, "Stops", 0, true);
    drag(&mut h, grab, grab + egui::vec2(-20.0, 30.0), egui::PointerButton::Primary);
    h.run_steps(3);
    let moved = places(&h)[0] - nodes_before[0];
    assert!((moved - egui::vec2(-20.0, 30.0) / scale).length() < 1.5, "the nodes moved by {:?}", moved);
    assert!((boxes(&h)[0].min - boxes_before[0].min - moved).length() < 0.05);
    assert_eq!(boxes(&h)[1..], boxes_before[1..], "the other boxes stay where they were");
    assert_eq!(places(&h)[2..], nodes_before[2..], "and so do the nodes in them");

    // One node alone leaves its box behind, so that it can be taken out of it. So do some
    // of the nodes in a box.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    let boxes_before = boxes(&h);
    click_text(&mut h, "Stop id", 0, true);
    assert_eq!(selected(&h), ["id"]);
    assert!(h.state().boxes_with_selection().is_empty());
    let grab = place_of(&h, "Stop id", 0, true);
    let was = places(&h)[4];
    drag(&mut h, grab, grab + egui::vec2(24.0, 12.0), egui::PointerButton::Primary);
    h.run_steps(3);
    assert!((places(&h)[4] - was).length() > 10.0);
    assert_eq!(boxes(&h), boxes_before);
    let second = place_of(&h, "Export date", 0, true);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    h.event(egui::Event::PointerMoved(second));
    h.event(egui::Event::PointerButton { pos: second, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::SHIFT });
    h.run_steps(2);
    h.event(egui::Event::PointerButton { pos: second, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::SHIFT });
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::default()));
    h.run_steps(3);
    assert_eq!(selected(&h), ["id", "stamp"]);
    assert!(h.state().boxes_with_selection().is_empty(), "two of the six nodes in the box are not the box");
    let (grab, was) = (place_of(&h, "Export date", 0, true), places(&h));
    drag(&mut h, grab, grab + egui::vec2(-16.0, 10.0), egui::PointerButton::Primary);
    h.run_steps(3);
    assert!((places(&h)[6] - was[6]).length() > 8.0 && (places(&h)[4] - was[4] - (places(&h)[6] - was[6])).length() < 0.05);
    assert_eq!(boxes(&h), boxes_before);
}

#[test]
fn nothing_selected_leaves_the_panel_empty() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    assert!(selected(&h).is_empty() && h.state().focus.is_none());
    assert!(h.query_by_label("Getting started").is_none());
    assert!(h.query_by_label_contains("Drag from an output port").is_none(), "nothing is written in the panel when nothing is selected");
    assert!(h.query_by_label("Name").is_none() && h.query_by_label("Comment").is_none());
    shot(&mut h, "141-nothing-selected");

    // What the text said is found in the Help menu.
    h.get_by_label("Help").click();
    h.run_steps(3);
    h.get_by_label("Tips and shortcuts").click();
    h.run_steps(3);
    assert!(h.state().tips);
    for part in ["Drag from an output port to an input port", "Ctrl+A selects everything", "F5 runs the model"] {
        assert!(h.query_by_label_contains(part).is_some(), "the tips should say: {}", part);
    }
    shot(&mut h, "142-tips");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(!h.state().tips && h.query_by_label_contains("Ctrl+A selects everything").is_none());
    assert!(!h.state().dirty);
}

// ---------- Version 0.53 ----------

#[test]
fn the_panel_says_what_the_program_is_for_when_nothing_is_selected() {
    let mut h = empty_app();
    for text in ["Work with geodata", "Reads", "Writes"] {
        assert!(h.query_all_by_label(text).count() >= 1, "the panel should say: {}", text);
    }
    for text in [
        "LibreOffice Calc (.ods)",
        "Excel (.xlsx)",
        "GeoTIFF, also Cloud Optimized (.tif)",
        "Point clouds (.las, .laz, .copc.laz)",
        "CAD drawings (.dxf, .dwg)",
    ] {
        assert!(h.query_all_by_label_contains(text).count() >= 1, "the panel should say: {}", text);
    }
    // The panel keeps its width: long lines wrap.
    let heading = h.get_by_label("Work with geodata").rect();
    assert!(heading.left() > 1600.0 - 380.0, "the panel is {} wide", 1600.0 - heading.left());
    assert!(h.query_by_label_contains("automated map making, statistics, graphs and reports").is_some());
    assert!(h.query_by_label_contains("To start, fold out Input and output").is_some());
    shot(&mut h, "150-intro");
    // With a node selected the form takes its place.
    h.state_mut().add_node("Reader");
    h.run_steps(3);
    assert!(h.query_by_label("Work with geodata").is_none() && h.query_all_by_label("Input").count() >= 1);
    // Once the model has something in it, the text is not shown again with nothing selected.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().selected.is_empty());
    assert!(h.query_by_label("Work with geodata").is_none());
}

#[test]
fn values_get_an_output_each_and_connections_follow_them() {
    let mut h = harness_with("lines.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    h.state_mut().run_blocking();
    h.state_mut().focus_node("filter");
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!((r.errors, r.warnings), (0, 0), "{:?}", r.log);
    assert_eq!(r.output("filter", "1").unwrap().len(), 6);
    // The ports on the canvas are the values and Other, with their counts.
    for port in ["1", "2", "3", "Other"] {
        assert!(h.query_all_by_label(port).count() >= 1, "the port {} should be on the canvas", port);
    }
    assert!(h.query_all_by_label("Filter by value").count() >= 1);
    assert_eq!(param(&h, "filter", "attribute"), "line");
    shot(&mut h, "151-filter-by-value");

    // The values are taken from the data: 5 is added, and the connection from Other follows
    // its port to the end.
    h.get_by_label("Use the values in the data").click();
    h.run_steps(3);
    let node = h.state().snarl.nodes().find(|n| n.id == "filter").unwrap().clone();
    assert_eq!(node.outputs(), ["1", "2", "3", "5", "Other"]);
    let w = wires(&h);
    for wire in ["filter.1 > line1.Input", "filter.2 > line2.Input", "filter.3 > line3.Input", "filter.Other > other.Input"] {
        assert!(w.contains(&wire.to_string()), "{} is missing from {:?}", wire, w);
    }
    assert!(h.state().dirty && h.state().stale);

    // A value that is taken away takes its connection with it. The others keep theirs.
    let cross = h.get_all_by_label("×").nth(1).expect("each value has a button to remove it").rect().center();
    click_at(&mut h, cross);
    h.run_steps(3);
    let node = h.state().snarl.nodes().find(|n| n.id == "filter").unwrap().clone();
    assert_eq!(node.outputs(), ["1", "3", "5", "Other"]);
    let w = wires(&h);
    assert!(!w.iter().any(|x| x.starts_with("filter.2 ")), "{:?}", w);
    assert!(w.contains(&"filter.3 > line3.Input".to_string()) && w.contains(&"filter.Other > other.Input".to_string()), "{:?}", w);
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.output("filter", "5").unwrap().len(), 2);
    assert_eq!(r.output("filter", "Other").unwrap().len(), 5, "line 2 now goes to Other");
    assert_eq!(r.nodes["line2"].status, og_engine::Status::Skipped, "an output with nothing connected does not write an empty sheet");
    shot(&mut h, "152-values-from-the-data");
    // Undo brings the value and its connection back.
    let empty = on_canvas(&h, 600.0, -60.0);
    click_at(&mut h, empty);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert!(wires(&h).contains(&"filter.2 > line2.Input".to_string()));
}

#[test]
fn ports_are_moved_up_and_down_and_keep_their_connections() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let node = |h: &Harness<'_, App>| h.state().snarl.nodes().find(|n| n.id == "clip_towns").unwrap().clone();
    assert_eq!(node(&h).inputs(), ["Clipper", "Clippee"]);
    let edges_before = wires(&h);
    // Right-click the name of the second input port on that node.
    let rect = h.state().node_rect("clip_towns").unwrap();
    let label = h.get_all_by_label("Clippee").map(|n| n.rect().center()).find(|c| rect.contains(*c)).unwrap();
    let at = h.state().graph_to_screen(label);
    right_click_at(&mut h, at);
    h.run_steps(3);
    assert!(h.query_by_label("Move up").is_some() && h.query_by_label("Move down").is_some());
    shot(&mut h, "163-port-menu");
    h.get_by_label("Move up").click();
    h.run_steps(3);
    shot(&mut h, "164-port-moved");
    assert_eq!(node(&h).inputs(), ["Clippee", "Clipper"]);
    assert_eq!(wires(&h), edges_before, "the connections go to the same ports, by name");
    assert!(h.state().dirty);
    // The order is saved with the model and read back.
    let file = h.state().base_dir().join("ports.json");
    h.state_mut().save_to(&file);
    let mut again = App::new();
    again.load(&file).unwrap();
    assert_eq!(again.snarl.nodes().find(|n| n.id == "clip_towns").unwrap().inputs(), ["Clippee", "Clipper"]);
    // The run does not care about the order.
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.output("clip_towns", "Inside").map(|t| t.len()), Some(12), "{:?}", r.nodes["clip_towns"]);
    // Undo puts the port back.
    let empty = on_canvas(&h, 600.0, -60.0);
    click_at(&mut h, empty);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(node(&h).inputs(), ["Clipper", "Clippee"]);
    assert_eq!(wires(&h), edges_before);
    // Outputs move the same way.
    let mut n = node(&h);
    assert!(n.move_port(true, 0, false));
    assert_eq!(n.outputs(), ["Outside", "Inside"]);
    assert!(!n.move_port(true, 1, false), "the last port cannot go further down");
}

#[test]
fn a_moved_port_is_marked_and_keeps_its_connection() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let node = h.state().snarl.nodes_ids_data().find(|(_, n)| n.value.id == "clip_towns").map(|(id, _)| id).unwrap();
    let before = wires(&h);
    let t = h.ctx.input(|i| i.time);
    h.state_mut().move_port(node, false, 1, true, t);
    h.run_steps(3);
    let n = h.state().snarl[node].clone();
    assert_eq!(n.inputs(), ["Clippee", "Clipper"]);
    assert_eq!(wires(&h), before, "each connection is still on the port it was on");
    assert_eq!(h.state().moved_port.as_ref().map(|m| (m.1, m.2.as_str())), Some((false, "Clippee")));
    shot(&mut h, "175-port-marked");
}

#[test]
fn comments_are_boxes_that_are_moved_and_resized() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    let id = h.state().snarl.nodes_ids_data().find(|(_, n)| n.value.id == "towns").map(|(id, _)| id).unwrap();
    h.state_mut().snarl[id].comment = "From the county, updated yearly".into();
    h.run_steps(4);
    let node = h.state().node_rect("towns").unwrap();
    let boxed = |h: &Harness<'_, App>| h.get_by_label("From the county, updated yearly").rect();
    // A comment without a place of its own is above its node.
    assert!(boxed(&h).bottom() < node.top() && boxed(&h).intersects(node.expand2(egui::vec2(0.0, 40.0))), "{:?} {:?}", boxed(&h), node);
    assert!(h.state().snarl[id].comment_box.is_none());
    shot(&mut h, "181-comment-box");

    // Dragged, it moves.
    let scale = h.state().canvas_scale();
    let before = boxed(&h);
    let grab = h.state().graph_to_screen(before.center());
    drag(&mut h, grab, grab + egui::vec2(160.0, -15.0), egui::PointerButton::Primary);
    h.run_steps(2);
    let moved = boxed(&h).min - before.min;
    assert!((moved - egui::vec2(160.0, -15.0) / scale).length() < 1.5, "moved by {:?}", moved);
    assert!(h.state().dirty && !h.state().stale);

    // Its right edge and lower left corner resize it.
    let before = boxed(&h);
    let edge = h.state().graph_to_screen(egui::pos2(before.right() - 2.0, before.center().y));
    drag(&mut h, edge, edge + egui::vec2(60.0, 0.0), egui::PointerButton::Primary);
    h.run_steps(2);
    assert!((boxed(&h).width() - before.width() - 60.0 / scale).abs() < 1.5, "{:?} → {:?}", before, boxed(&h));
    assert_eq!(boxed(&h).min, before.min);
    let before = boxed(&h);
    let corner = h.state().graph_to_screen(before.left_bottom() + egui::vec2(3.0, -3.0));
    drag(&mut h, corner, corner + egui::vec2(-20.0, 24.0), egui::PointerButton::Primary);
    h.run_steps(2);
    let after = boxed(&h);
    assert!(
        (after.left() - before.left() + 20.0 / scale).abs() < 1.5 && (after.height() - before.height() - 24.0 / scale).abs() < 1.5,
        "{:?} → {:?}",
        before,
        after
    );
    assert!((after.right() - before.right()).abs() < 0.5);

    // It follows its node.
    let node_before = h.state().node_rect("towns").unwrap();
    let title = h.state().graph_to_screen(node_before.center_top() + egui::vec2(0.0, 12.0));
    let comment_before = boxed(&h);
    drag(&mut h, title, title + egui::vec2(40.0, 40.0), egui::PointerButton::Primary);
    h.run_steps(3);
    let node_moved = h.state().node_rect("towns").unwrap().min - node_before.min;
    assert!(node_moved.length() > 10.0, "the node did not move");
    assert!((boxed(&h).min - comment_before.min - node_moved).length() < 1.5);
    shot(&mut h, "182-comment-moved-and-resized");

    // The place and size are saved with the model.
    let file = scratch("comments").join("c.json");
    h.state_mut().save_to(&file);
    let mut again = App::new();
    again.load(&file).unwrap();
    let n = again.snarl.nodes().find(|n| n.id == "towns").unwrap();
    let (saved, shown) = (n.comment_box.unwrap(), h.state().snarl[id].comment_box.unwrap());
    assert!(saved.iter().zip(shown).all(|(a, b)| (a - b).abs() <= 0.5), "{:?} {:?}", saved, shown);
}

#[test]
fn group_boxes_are_resized_from_every_edge_and_corner() {
    let mut h = empty_app();
    let scale = h.state().canvas_scale();
    // A box in the middle of the empty canvas.
    let origin = h.state().graph_to_screen(egui::Pos2::ZERO);
    let mid = (h.state().canvas_rect().center() - origin) / scale;
    h.state_mut().groups.push(og_engine::GroupBox {
        name: "Box".into(),
        x: (mid.x - 150.0) as f64,
        y: (mid.y - 100.0) as f64,
        w: 300.0,
        h: 200.0,
        color: String::new(),
        locked: true,
    });
    h.run_steps(3);
    let rect = |h: &Harness<'_, App>| og_gui::graph::group_rect(&h.state().groups[0]);
    type Case = (fn(egui::Rect) -> egui::Pos2, egui::Vec2, [f32; 4]);
    let cases: [Case; 8] = [
        (|r| egui::pos2(r.left() + 2.0, r.center().y), egui::vec2(-30.0, 0.0), [-1.0, 0.0, 1.0, 0.0]),
        (|r| egui::pos2(r.right() - 2.0, r.center().y), egui::vec2(30.0, 0.0), [0.0, 0.0, 1.0, 0.0]),
        (|r| egui::pos2(r.center().x + 40.0, r.top() + 2.0), egui::vec2(0.0, -30.0), [0.0, -1.0, 0.0, 1.0]),
        (|r| egui::pos2(r.center().x, r.bottom() - 2.0), egui::vec2(0.0, 30.0), [0.0, 0.0, 0.0, 1.0]),
        (|r| r.left_top() + egui::vec2(3.0, 3.0), egui::vec2(-20.0, -20.0), [-1.0, -1.0, 1.0, 1.0]),
        (|r| r.right_top() + egui::vec2(-3.0, 3.0), egui::vec2(20.0, -20.0), [0.0, -1.0, 1.0, 1.0]),
        (|r| r.left_bottom() + egui::vec2(3.0, -3.0), egui::vec2(-20.0, 20.0), [-1.0, 0.0, 1.0, 1.0]),
        (|r| r.right_bottom() - egui::vec2(3.0, 3.0), egui::vec2(20.0, 20.0), [0.0, 0.0, 1.0, 1.0]),
    ];
    for (k, (spot, by, expect)) in cases.into_iter().enumerate() {
        let before = rect(&h);
        let at = h.state().graph_to_screen(spot(before));
        drag(&mut h, at, at + by, egui::PointerButton::Primary);
        h.run_steps(2);
        let after = rect(&h);
        let d = by / scale;
        let got = [after.left() - before.left(), after.top() - before.top(), after.width() - before.width(), after.height() - before.height()];
        // Moving the left or top edge moves that edge, the others stay.
        let want = [
            if expect[0] != 0.0 { d.x } else { 0.0 },
            if expect[1] != 0.0 { d.y } else { 0.0 },
            if expect[2] != 0.0 {
                if expect[0] != 0.0 {
                    -d.x
                } else {
                    d.x
                }
            } else {
                0.0
            },
            if expect[3] != 0.0 {
                if expect[1] != 0.0 {
                    -d.y
                } else {
                    d.y
                }
            } else {
                0.0
            },
        ];
        for i in 0..4 {
            assert!((got[i] - want[i]).abs() < 1.5, "case {}: {:?} should be {:?}", k, got, want);
        }
        assert_eq!(h.state().groups.len(), 1);
    }
    shot(&mut h, "183-group-resized");
}

#[test]
fn the_search_boxes_are_cleared_with_the_cross() {
    let mut h = empty_app();
    assert!(h.query_by_label("Clear the search").is_none(), "no cross while the box is empty");
    h.state_mut().filter = "buff".into();
    h.run_steps(3);
    assert!(h.query_by_label("Sort").is_none());
    shot(&mut h, "184-search-with-cross");
    h.get_by_label("Clear the search").click();
    h.run_steps(3);
    assert!(h.state().filter.is_empty());
    assert!(h.query_by_label("Attributes").is_some());
    // The search in the menu of the canvas has one too.
    let empty = h.state().canvas_rect().center();
    right_click_at(&mut h, empty);
    h.run_steps(3);
    h.get_by_label_contains("Add tool").hover();
    h.run_steps(4);
    h.event(egui::Event::Text("buff".into()));
    h.run_steps(3);
    assert!(h.query_by_label("Sort").is_none());
    shot(&mut h, "184-tool-search-with-cross");
    h.get_by_label("Clear the search").click();
    h.run_steps(3);
    shot(&mut h, "184-tool-search-cleared");
    assert!(h.query_by_label("Sort").is_some() && h.query_by_label("Buffer").is_some(), "the whole list is back");
}

#[test]
fn comments_are_as_high_as_their_text_and_can_be_narrow() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    let id = graph_id(&h, "towns");
    h.state_mut().snarl[id].comment = "PDF".into();
    h.run_steps(4);
    let r = h.get_by_label("PDF").rect();
    // One line of text and the space above and below it, and no more.
    let [_, _, _, height] = og_gui::graph::default_comment_box(&h.ctx.clone(), "PDF", egui::vec2(170.0, 80.0));
    assert!(height <= 26.0, "{}", height);
    assert!(r.height() <= height + 0.5, "{:?}", r);
    // The box can be made as narrow as the word.
    let box_at = h.state().graph_to_screen(r.right_center()) - egui::vec2(1.0, 0.0);
    drag(&mut h, box_at, box_at - egui::vec2(400.0, 0.0), egui::PointerButton::Primary);
    let b = h.state().snarl[id].comment_box.unwrap();
    let least = og_gui::graph::comment_min_size(&h.ctx.clone(), "PDF");
    assert!(b[2] <= 40.0 && (b[2] - least.x).abs() < 0.5, "as narrow as the word, and no narrower: {:?} {:?}", b, least);
    shot(&mut h, "201-narrow-comment");
}

#[test]
fn nodes_are_copied_and_pasted_between_windows() {
    let mut a = harness(egui::Theme::Light, NO_TILES);
    a.run_steps(4);
    let picked = vec![graph_id(&a, "towns"), graph_id(&a, "towns_sweref"), graph_id(&a, "skaraborg")];
    a.state_mut().set_selection(picked);
    a.event(egui::Event::Copy);
    a.run_steps(3);
    let json = a.state().clipboard.clone().expect("the nodes are on the clipboard");
    let dir = a.state().base_dir();
    assert!(json.contains(&*dir.join("towns.csv").to_string_lossy().replace('\\', "\\\\")), "paths are made whole: {}", json);
    assert_eq!(a.state().snarl.nodes().count(), 18, "copy leaves the model as it was");

    // Another window, which has never seen the model.
    let mut b = empty_app();
    b.run_steps(3);
    let at = b.state().canvas_rect().center();
    b.event(egui::Event::PointerMoved(at));
    b.run_steps(2);
    b.event(egui::Event::Paste(json.clone()));
    b.run_steps(4);
    let st = b.state();
    assert_eq!(st.snarl.nodes().count(), 3);
    assert_eq!(st.snarl.wires().count(), 1, "the connection between Towns and Towns to SWEREF comes along, not the ones to nodes left behind");
    let mut names: Vec<String> = st.snarl.nodes().map(|n| n.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["Only Skaraborg", "Towns", "Towns to SWEREF"]);
    assert_eq!(st.selected.len(), 3, "what was pasted is selected");
    // The upper left node lands where the pointer is.
    let first = st.snarl.nodes_pos_ids().map(|(_, p, _)| p).fold(egui::pos2(f32::MAX, f32::MAX), |a, p| a.min(p));
    let want = st.screen_to_graph(at);
    assert!((first - want).length() < 1.0, "{:?} {:?}", first, want);
    assert!(st.stale && st.dirty);
    shot(&mut b, "202-pasted");

    // Pasting again in the same window gives new ids and marks the names as copies.
    a.event(egui::Event::Paste(json));
    a.run_steps(4);
    assert_eq!(a.state().snarl.nodes().count(), 21);
    assert!(a.state().snarl.nodes().any(|n| n.name == "Towns copy"));
    let ids: std::collections::HashSet<String> = a.state().snarl.nodes().map(|n| n.id.clone()).collect();
    assert_eq!(ids.len(), 21);
    // Cut takes them away, and text that is no model is not pasted.
    let n = a.state().selected.len();
    a.event(egui::Event::Cut);
    a.run_steps(3);
    assert_eq!((n, a.state().snarl.nodes().count()), (3, 18));
    a.event(egui::Event::Paste("just some text".into()));
    a.run_steps(3);
    assert_eq!(a.state().snarl.nodes().count(), 18);
    // Paste in the Edit menu.
    a.get_by_label("Edit").click();
    a.run_steps(3);
    a.get_by_label_contains("Paste").click();
    a.run_steps(3);
    assert_eq!(a.state().snarl.nodes().count(), 21);
}

#[test]
fn database_connections_are_kept_in_the_settings() {
    let mut h = empty_app();
    // Ctrl+, opens the settings, as in many programs. The Edit menu has them too.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    assert!(h.state().settings_window.is_some());
    for label in ["Settings", "Database connections", "New connection", "Close"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{} should be in the settings", label);
    }
    let before = h.state().settings.connections.len();
    h.get_by_label("New connection").click();
    h.run_steps(3);
    let n = h.state().settings.connections.len();
    assert_eq!(n, before + 1);
    let name = h.state().settings.connections[n - 1].name.clone();
    // The name is written over, and the connection saved at once.
    // The field, which is wider than the name in the list.
    let field = h.get_all_by_value(&name).map(|n| n.rect()).max_by(|a, b| a.width().total_cmp(&b.width())).unwrap().center();
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("VGR test".into()));
    h.run_steps(3);
    assert_eq!(h.state().settings.connections[n - 1].name, "VGR test");
    // Database is also a list of tools on the left.
    for label in ["Server", "Port", "Database", "User", "Password", "Test connection"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{} should be in the form of a connection", label);
    }
    let stored = std::fs::read_to_string(og_engine::settings::settings_file().unwrap()).unwrap();
    assert!(stored.contains("\"VGR test\""), "{}", stored);
    // With the test database there, Test connection says how many tables it has.
    {
        let s = &mut h.state_mut().settings.connections[n - 1];
        s.database = "gistest".into();
        s.user = "gis".into();
        s.password = "gis-test".into();
    }
    h.run_steps(2);
    h.get_by_label("Test connection").click();
    for _ in 0..50 {
        h.run_steps(2);
        if !h.state().settings_window.as_ref().unwrap().busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    h.run_steps(2);
    let tested = h.state().settings_window.as_ref().unwrap().tested.clone();
    assert!(tested.is_some_and(|t| t.1.is_ok() || t.1.unwrap_err().contains("connect")), "the test gives an answer");
    shot(&mut h, "230-settings-connections");
    // The connection is offered in the form of PostGIS input, by its name.
    h.get_by_label("Remove").click();
    h.run_steps(2);
    assert_eq!(h.state().settings.connections.len(), before);
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.state().settings_window.is_none());
}

#[test]
fn connections_are_disabled_moved_and_hit_easily() {
    let (mut h, _dir) = two_nodes();
    let middle = between(&h, ("in", "Output"), ("out", "Input"));
    // The secondary button on a connection opens its menu; it is not removed.
    right_click_at(&mut h, middle);
    assert!(h.state().wire_menu.is_some());
    assert_eq!(wires(&h).len(), 1);
    for label in ["Disable connection", "Delete connection"] {
        assert_eq!(h.query_all_by_label(label).count(), 2, "{} should be in the menu and in the panel", label);
    }
    shot(&mut h, "231-connection-menu");
    // The menu, at the pointer, and the panel of the selected connection both have it.
    let item = h.get_all_by_label("Disable connection").map(|n| n.rect().center()).min_by(|a, b| a.distance(middle).total_cmp(&b.distance(middle))).unwrap();
    click_at(&mut h, item);
    h.run_steps(3);
    assert!(h.state().wire_menu.is_none());
    let ws = h.state().workspace();
    assert!(ws.edges[0].disabled && h.state().stale);
    // A disabled connection lets nothing through: the output gets nothing to write.
    h.state_mut().run_blocking();
    let r = h.state().result.clone().unwrap();
    assert!(r.nodes["out"].inputs.get("Input").is_none_or(|t| t.is_empty()), "nothing comes through");
    // It is turned on again from the panel of the selected connection, and undo knows of it.
    assert!(h.query_by_label("This connection is disabled. It lets nothing through.").is_some());
    shot(&mut h, "232-connection-disabled");
    h.get_by_label("Enable connection").click();
    h.run_steps(3);
    assert!(!h.state().workspace().edges[0].disabled);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert!(h.state().workspace().edges[0].disabled, "undo turns it off again");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
    h.run_steps(3);
    assert!(!h.state().workspace().edges[0].disabled);

    // A connection is picked up at its input and dropped on another input, and a port is hit
    // a few points away from its circle.
    h.state_mut().add_node("Writer");
    h.run_steps(4);
    let other = h.state().snarl.nodes().find(|n| n.kind == "Writer" && n.id != "out").unwrap().id.clone();
    let from = pin_at(&h, "out", "Input", true) + egui::vec2(-5.0, 3.0);
    let to = pin_at(&h, &other, "Input", true) + egui::vec2(4.0, -4.0);
    drag(&mut h, from, to, egui::PointerButton::Primary);
    h.run_steps(3);
    assert_eq!(wires(&h), [format!("in.Output > {}.Input", other)], "the connection moved");
}

#[test]
fn notes_are_resized_and_links_open() {
    let mut h = empty_app();
    h.get_by_label("Edit").click();
    h.run_steps(3);
    h.get_by_label("Add a note").click();
    h.run_steps(4);
    let note = h.state().selected[0];
    h.state_mut().snarl[note].comment = "Data from https://vgregion.entryscape.net/rowstore, updated daily.".into();
    h.run_steps(4);
    let id = h.state().snarl[note].id.clone();
    let before = h.state().node_rect(&id).unwrap();
    // The lower right corner makes the note wider and taller, as a comment's does.
    let grip = h.state().graph_to_screen(before.max + egui::vec2(3.0, 3.0));
    drag(&mut h, grip, grip + egui::vec2(120.0, 60.0), egui::PointerButton::Primary);
    h.run_steps(4);
    let after = h.state().node_rect(&id).unwrap();
    assert!(after.width() > before.width() + 60.0 && after.height() > before.height() + 30.0, "{:?} → {:?}", before, after);
    // The left edge makes it wider to the left: the right edge stays.
    let edge = h.state().graph_to_screen(egui::pos2(after.left() - 3.0, after.center().y));
    drag(&mut h, edge, edge - egui::vec2(60.0, 0.0), egui::PointerButton::Primary);
    h.run_steps(4);
    let wider = h.state().node_rect(&id).unwrap();
    assert!(wider.left() < after.left() - 30.0 && (wider.right() - after.right()).abs() < 4.0, "{:?} → {:?}", after, wider);
    assert!(h.query_by_label("NOTE").is_none(), "a note says what it is by its look");
    let ws = h.state().workspace();
    assert!(ws.notes[0].w.unwrap() > 250.0 && ws.notes[0].h.is_some(), "the size is kept in the model");
    shot(&mut h, "233-note-resized-with-link");
    // A click on the link opens it in the browser.
    let text = h.get_by_label("Data from https://vgregion.entryscape.net/rowstore, updated daily.").rect();
    let link = h.state().graph_to_screen(text.left_top()) + egui::vec2(110.0, 7.0);
    click_at(&mut h, link);
    let opened: Option<String> = h.ctx.data(|d| d.get_temp(egui::Id::new(og_gui::links::LAST_OPENED)));
    assert_eq!(opened.as_deref(), Some("https://vgregion.entryscape.net/rowstore"));
}

#[test]
fn group_boxes_are_copied_with_their_nodes() {
    let mut h = harness_with("stops.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(6);
    assert_eq!(h.state().groups.len(), 3);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(3);
    h.state_mut().quiet = true;
    let ctx = h.ctx.clone();
    h.state_mut().copy_selected(&ctx, false);
    let text = h.state().clipboard_text().unwrap();
    let ws = og_engine::Workspace::from_json(&text).unwrap();
    assert_eq!(ws.groups.len(), 3, "the boxes go along when all they hold is copied");
    // Pasted into another window, the boxes come with the nodes, around them as before.
    let mut e = empty_app();
    e.state_mut().paste(&text);
    e.run_steps(4);
    assert_eq!(e.state().groups.len(), 3);
    assert_eq!((0..3).map(|g| e.state().nodes_in_group(g).len()).collect::<Vec<_>>(), [2, 6, 2]);
}

#[test]
fn connections_of_a_disabled_node_are_dashed_and_a_zip_file_is_added() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    // Disabling a node shows on its connections too: they are dashed.
    let id = h.state().snarl.node_ids().find(|(_, n)| n.id == "outside").map(|(i, _)| i);
    h.state_mut().snarl[id.expect("the node is there")].disabled = true;
    h.run_steps(4);
    shot(&mut h, "252-disabled-node-wires");
    // Make zip file is found among the inputs and outputs, and asks only what it needs.
    h.state_mut().filter = "zip".into();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Make zip file").is_some());
    h.state_mut().add_node("ZipWriter");
    h.run_steps(4);
    assert!(h.query_by_label("Zip file").is_some() && h.query_by_label("Other files and folders").is_some());
    assert!(h.query_by_label("Outputs by name").is_none(), "only when outputs are chosen by name");
    shot(&mut h, "253-zip-file");
}

#[test]
fn a_view_shows_every_stream_connected_to_it_and_data_is_copied() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    // Two streams into one View.
    h.state_mut().add_node("Viewer");
    h.run_steps(3);
    let view = h.state().snarl.node_ids().find(|(_, n)| n.kind == "Viewer").map(|(i, _)| i).unwrap();
    for from in ["towns", "routes"] {
        let src = h.state().snarl.node_ids().find(|(_, n)| n.id == from).map(|(i, _)| i).unwrap();
        h.state_mut().snarl.connect(egui_snarl::OutPinId { node: src, output: 0 }, egui_snarl::InPinId { node: view, input: 0 });
    }
    h.state_mut().run_blocking();
    settle(&mut h);
    let id = h.state().snarl[view].id.clone();
    h.state_mut().focus_node(&id);
    settle(&mut h);
    let names: Vec<String> = h.state().result.as_ref().unwrap().nodes[&id].outputs.keys().cloned().collect();
    assert_eq!(names.len(), 2, "{:?}", names);
    assert!(h.query_by_label_contains("1 · ").is_some() && h.query_by_label_contains("2 · ").is_some(), "both are offered under the canvas");
    shot(&mut h, "262-view");
    // A value in the table is copied with the secondary button.
    let cell = h.query_all_by_label("Göteborg–Mariestad").map(|n| n.rect().center()).next_back();
    assert!(cell.is_some(), "the routes are in the table");
    if let Some(at) = cell {
        right_click_at(&mut h, at);
        h.run_steps(3);
        assert!(h.query_by_label("Copy value").is_some() && h.query_by_label("Copy row").is_some());
        shot(&mut h, "263-copy-value");
        h.get_by_label("Copy value").click();
        h.run_steps(3);
        let copied = h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "Göteborg–Mariestad"));
        assert!(copied || h.query_by_label("Copy value").is_none());
    }
}

#[test]
fn colours_are_copied_and_layers_dragged_in_draw_map() {
    let mut h = harness_with("map.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().focus_node("map");
    settle(&mut h);
    let layers = |h: &Harness<'_, App>| {
        let p = h.state().snarl.nodes().find(|n| n.id == "map").unwrap().params.get("layers").cloned().unwrap();
        p.as_array().unwrap().iter().map(|l| (l["from"].as_str().unwrap().to_string(), l["fill"].clone())).collect::<Vec<_>>()
    };
    let before = layers(&h);
    // The secondary button on the first Fill copies it; on the third, pastes it.
    let fills: Vec<egui::Pos2> = h.get_all_by_label("Fill").map(|n| n.rect().center()).collect();
    right_click_at(&mut h, fills[0]);
    h.run_steps(3);
    h.get_by_label("Copy colour").click();
    h.run_steps(3);
    right_click_at(&mut h, fills[2]);
    h.run_steps(3);
    shot(&mut h, "264-paste-colour");
    h.get_by_label_contains("Paste colour").click();
    h.run_steps(3);
    let after = layers(&h);
    assert_eq!(after[2].1, before[0].1, "the third layer got the fill of the first");
    // A layer is dragged by its handle to the top.
    let handles: Vec<egui::Pos2> = h.get_all_by_label("Drag handle").map(|n| n.rect().center()).collect();
    assert_eq!(handles.len(), 3);
    drag(&mut h, handles[2], handles[0] - egui::vec2(0.0, 12.0), egui::PointerButton::Primary);
    h.run_steps(4);
    let moved = layers(&h);
    assert_eq!(moved[0].0, after[2].0, "the bottom layer is now on top: {:?}", moved);
    shot(&mut h, "265-layer-dragged");
}

#[test]
fn connections_get_bend_points_that_are_dragged_and_removed() {
    let (mut h, _dir) = two_nodes();
    let middle = between(&h, ("in", "Output"), ("out", "Input"));
    right_click_at(&mut h, middle);
    h.get_by_label("Add bend point").click();
    h.run_steps(3);
    let points = |h: &Harness<'_, App>| h.state().workspace().edges[0].points.clone();
    assert_eq!(points(&h).len(), 1);
    assert!(h.state().dirty);
    // The point is dragged, and the connection goes through it.
    let at = h.state().transform * egui::pos2(points(&h)[0][0] as f32, points(&h)[0][1] as f32);
    drag(&mut h, at, at + egui::vec2(0.0, 90.0), egui::PointerButton::Primary);
    h.run_steps(3);
    let moved = points(&h)[0];
    let was = h.state().transform.inverse() * at;
    assert!(moved[1] > was.y as f64 + 40.0, "{:?} from {:?}", moved, was);
    // A second point, then one taken away by its own menu.
    let wire = h.state().snarl.wires().next().unwrap();
    let place = h.state().transform.inverse() * (middle + egui::vec2(60.0, -50.0));
    h.state_mut().add_bend(wire, place);
    h.run_steps(3);
    assert_eq!(points(&h).len(), 2);
    shot(&mut h, "266-bend-points");
    let p = points(&h)[0];
    let at = h.state().transform * egui::pos2(p[0] as f32, p[1] as f32);
    right_click_at(&mut h, at);
    assert!(h.state().bend_menu.is_some());
    h.get_by_label("Remove bend point").click();
    h.run_steps(3);
    assert_eq!(points(&h).len(), 1);
    // Saved with the model and read back.
    let ws = h.state().workspace();
    let back = og_engine::Workspace::from_json(&ws.to_json()).unwrap();
    assert_eq!(back.edges[0].points.len(), 1);
}

// ---------- Version 0.73 ----------

#[test]
fn api_connections_and_every_coordinate_system() {
    let mut h = empty_app();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    h.get_by_label("API connections").click();
    h.run_steps(2);
    let before = h.state().settings.apis.len();
    h.get_by_label("New API connection").click();
    h.run_steps(3);
    assert_eq!(h.state().settings.apis.len(), before + 1);
    for label in ["Header", "Key", "Only for addresses"] {
        assert!(h.query_by_label(label).is_some(), "{} should be in the form", label);
    }
    shot(&mut h, "276-api-connections");
    h.get_by_label("Close").click();
    h.run_steps(3);

    // A tool with a coordinate system: every one of PROJ can be searched for.
    let dir = example_copy();
    std::fs::write(
        dir.join("crs.json"),
        r#"{"format": "ographis/1", "name": "CRS", "nodes": [{"id": "rp", "type": "Reprojector", "name": "Reproject", "x": 40, "y": 40, "params": {"crs": "EPSG:3006"}}], "edges": []}"#,
    )
    .unwrap();
    h.state_mut().load(&dir.join("crs.json")).unwrap();
    h.run_steps(3);
    h.state_mut().focus_node("rp");
    settle(&mut h);
    h.get_by_value("SWEREF 99 TM · EPSG:3006").click();
    h.run_steps(2);
    let n = og_gdal::crs_list().len();
    h.get_by_label(&format!("Search all {}…", n)).click();
    h.run_steps(3);
    h.event(egui::Event::Text("RT90 2.5 gon V".into()));
    h.run_steps(3);
    shot(&mut h, "277-search-every-crs");
    h.get_by_label("EPSG:3021 · RT90 2.5 gon V").click();
    h.run_steps(3);
    let node = h.state().snarl.nodes().find(|n| n.id == "rp").unwrap().params.get("crs").cloned();
    assert_eq!(node, Some(serde_json::json!("EPSG:3021")));
}

#[test]
fn a_note_edge_follows_the_pointer_smoothly() {
    let mut h = empty_app();
    h.get_by_label("Edit").click();
    h.run_steps(3);
    h.get_by_label("Add a note").click();
    h.run_steps(4);
    let note = h.state().selected[0];
    h.state_mut().snarl[note].comment = "A note about the data, which is long enough to wrap over a few lines when it is narrow.".into();
    h.run_steps(4);
    let id = h.state().snarl[note].id.clone();
    // Zoomed in, so that screen and canvas units differ.
    let r = h.state().node_rect(&id).unwrap();
    let over = h.state().graph_to_screen(r.center()) + egui::vec2(0.0, 160.0);
    pointer(&mut h, over, None);
    h.event(egui::Event::Zoom(1.5));
    h.run_steps(3);
    let scale = h.state().transform.scaling;
    assert!((scale - 1.0).abs() > 0.2, "zoomed: {}", scale);
    let start = h.state().node_rect(&id).unwrap();
    // The right edge, dragged slowly to the right in small steps, then back: the edge stays
    // under the pointer all the way.
    let from = h.state().graph_to_screen(egui::pos2(start.right() + 3.0, start.center().y));
    pointer(&mut h, from, None);
    pointer(&mut h, from, Some((egui::PointerButton::Primary, true)));
    let mut worst: f32 = 0.0;
    let mut steps: Vec<f32> = (1..=40).map(|k| k as f32 * 4.0).collect();
    steps.extend((1..=20).rev().map(|k| k as f32 * 4.0));
    for dx in steps {
        let at = from + egui::vec2(dx, 0.0);
        pointer(&mut h, at, None);
        h.run_steps(1);
        let r = h.state().node_rect(&id).unwrap();
        let edge = h.state().graph_to_screen(egui::pos2(r.right() + 3.0, r.center().y)).x;
        worst = worst.max((edge - at.x).abs());
    }
    pointer(&mut h, from + egui::vec2(80.0, 0.0), Some((egui::PointerButton::Primary, false)));
    assert!(worst <= 6.0 * scale.max(1.0), "the edge strays {} px from the pointer", worst);
}

#[test]
fn everything_selected_moves_together_also_by_a_box() {
    let dir = example_copy();
    std::fs::write(
        dir.join("boxes.json"),
        r#"{"format": "ographis/1", "name": "Boxes",
            "nodes": [{"id": "a", "type": "Reader", "name": "A", "x": 60, "y": 80, "params": {"path": "towns.csv"}},
                      {"id": "b", "type": "Counter", "name": "B", "x": 420, "y": 300, "params": {}},
                      {"id": "c", "type": "Counter", "name": "C", "x": 700, "y": 80, "params": {}}],
            "edges": [{"from": ["a", "Output"], "to": ["b", "Input"], "points": [[300.0, 120.0]]},
                      {"from": ["b", "Output"], "to": ["c", "Input"]}],
            "groups": [{"name": "First", "x": 30, "y": 30, "w": 300, "h": 200, "locked": false},
                       {"name": "Last", "x": 670, "y": 30, "w": 300, "h": 200}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("boxes.json")).unwrap();
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    let ws0 = h.state().workspace();
    let pos = |ws: &og_engine::Workspace, id: &str| ws.nodes.iter().find(|n| n.id == id).map(|n| (n.x, n.y)).unwrap();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(3);
    // Taken hold of by the title of a box that is not locked, everything goes along.
    let title = on_canvas(&h, 120.0, 40.0);
    drag(&mut h, title, title + egui::vec2(100.0, 60.0), egui::PointerButton::Primary);
    h.run_steps(4);
    let ws = h.state().workspace();
    let scale = h.state().transform.scaling as f64;
    let moved = |id: &str| {
        let (a, b) = (pos(&ws0, id), pos(&ws, id));
        ((b.0 - a.0) * scale, (b.1 - a.1) * scale)
    };
    for id in ["a", "b", "c"] {
        let (dx, dy) = moved(id);
        assert!((dx - 100.0).abs() < 3.0 && (dy - 60.0).abs() < 3.0, "{} moved {:?}", id, (dx, dy));
    }
    for k in 0..2 {
        let (dx, dy) = ((ws.groups[k].x - ws0.groups[k].x) * scale, (ws.groups[k].y - ws0.groups[k].y) * scale);
        assert!((dx - 100.0).abs() < 3.0 && (dy - 60.0).abs() < 3.0, "box {} moved {:?}", k, (dx, dy));
    }
    let bend = ws.edges.iter().find(|e| e.from.0 == "a").unwrap().points[0];
    assert!(((bend[0] - 300.0) * scale - 100.0).abs() < 3.0 && ((bend[1] - 120.0) * scale - 60.0).abs() < 3.0, "the bend point went along: {:?}", bend);
    assert_eq!(h.state().selected.len(), 3, "the selection stays");
}

#[test]
fn colours_are_typed_as_codes_and_copied_on_the_colour_itself() {
    let mut h = harness_with("map.ographis", egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().focus_node("map");
    settle(&mut h);
    let fill = |h: &Harness<'_, App>, k: usize| {
        let p = h.state().snarl.nodes().find(|n| n.id == "map").unwrap().params.get("layers").cloned().unwrap();
        p.as_array().unwrap()[k]["fill"].clone()
    };
    // The colours are known by their codes.
    let swatches = |h: &Harness<'_, App>| {
        h.query_all_by_label_contains("#").filter(|n| format!("{:?}", n).contains("ColorWell")).map(|n| n.rect().center()).collect::<Vec<_>>()
    };
    // A click on the colour opens the picker, with the code to type in.
    let first = swatches(&h)[0];
    click_at(&mut h, first);
    h.run_steps(3);
    let code = h.get_by_label("Code").rect();
    let field = egui::pos2(code.right() + 40.0, code.center().y);
    click_at(&mut h, field);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.event(egui::Event::Text("#80d3ff".into()));
    h.run_steps(3);
    shot(&mut h, "281-colour-code");
    let typed = fill(&h, 0).to_string().to_lowercase();
    assert!(typed.contains("80d3ff") || typed.contains("128") && typed.contains("211"), "{}", typed);
    // Closed, and then the secondary button on the colour itself copies it, and on another
    // pastes it.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(!swatches(&h).is_empty(), "Escape closes the picker, and the Draw map stays chosen");
    let s = swatches(&h);
    right_click_at(&mut h, s[0]);
    h.run_steps(3);
    h.get_by_label("Copy colour").click();
    h.run_steps(3);
    let s = swatches(&h);
    right_click_at(&mut h, *s.last().unwrap());
    h.run_steps(3);
    assert!(h.query_by_label_contains("Paste colour #80d3ff").is_some());
    shot(&mut h, "282-paste-on-colour");
    h.get_by_label_contains("Paste colour").click();
    h.run_steps(3);
    let p = h.state().snarl.nodes().find(|n| n.id == "map").unwrap().params.get("layers").cloned().unwrap();
    let text = p.to_string().to_lowercase();
    assert!(text.matches("80d3ff").count() + text.matches("[128,211,255").count() >= 2, "{}", text);
}

/// A node dragged to the edge of the canvas pans it, and goes along under the pointer, so
/// that it can be put down beyond what was seen. Away from the edge nothing pans.
#[test]
fn a_node_dragged_to_the_edge_pans_the_canvas() {
    let (mut h, _dir) = two_nodes();
    let canvas = h.state().canvas_rect();
    let fixed = egui::pos2(0.0, 0.0);
    let grab = place_of(&h, "Towns", 0, true);
    pointer(&mut h, grab, None);
    pointer(&mut h, grab, Some((egui::PointerButton::Primary, true)));
    // In the middle: the canvas stays where it was.
    let before = h.state().graph_to_screen(fixed);
    pointer(&mut h, canvas.center(), None);
    pointer(&mut h, canvas.center() + egui::vec2(3.0, 0.0), None);
    assert_eq!(h.state().graph_to_screen(fixed), before);
    // At the right edge it pans to the left, a little more each frame the pointer moves.
    let edge = egui::pos2(canvas.right() - 6.0, canvas.center().y);
    for k in 0..8 {
        pointer(&mut h, edge + egui::vec2(0.0, (k % 2) as f32), None);
    }
    let after = h.state().graph_to_screen(fixed);
    assert!(after.x < before.x - 40.0, "{:?} {:?}", before, after);
    assert!((after.y - before.y).abs() < 1.0);
    pointer(&mut h, edge, Some((egui::PointerButton::Primary, false)));
    h.run_steps(3);
    // The node is under the pointer still, beyond where the canvas began.
    let rect = h.state().node_rect("in").unwrap();
    let on_screen = h.state().graph_to_screen(rect.center());
    assert!((on_screen.x - edge.x).abs() < rect.width() * h.state().transform.scaling + 10.0, "{:?} {:?}", on_screen, edge);
}

/// When the model reaches beyond the canvas, scroll bars show how much of it is in view;
/// dragging one moves the canvas.
#[test]
fn scroll_bars_show_and_move_a_large_model() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    // Zoomed in on the left part of the model.
    let t = egui::emath::TSTransform { scaling: 1.5, translation: egui::vec2(300.0, 200.0) };
    h.state_mut().set_canvas_transform(t);
    h.run_steps(4);
    let canvas = h.state().canvas_rect();
    let before = h.state().transform;
    // The bar along the bottom, dragged to the right.
    let from = egui::pos2(canvas.left() + 30.0, canvas.bottom() - 8.5);
    drag(&mut h, from, from + egui::vec2(120.0, 0.0), egui::PointerButton::Primary);
    h.run_steps(3);
    let after = h.state().transform;
    assert!(after.translation.x < before.translation.x - 100.0, "{:?} → {:?}", before, after);
    assert_eq!(after.translation.y, before.translation.y);
    shot(&mut h, "088-scroll-bars");
}

/// Two clicks in the same moment, as a double click.
fn double_click(h: &mut Harness<'_, App>, pos: egui::Pos2) {
    pointer(h, pos, None);
    // A frame each, a quarter of a second apart in the harness.
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
    }
    h.run_steps(2);
}

/// Quick add: a double click on empty canvas opens a box where a few letters find the tool,
/// / puts the tool after the selected node, connected, and a connection let go on empty
/// canvas asks for the tool to connect.
#[test]
fn tools_are_added_by_typing() {
    let (mut h, _dir) = two_nodes();
    let empty = on_canvas(&h, 350.0, 220.0);
    assert!(h.state().canvas_rect().contains(empty), "{:?} {:?}", empty, h.state().canvas_rect());
    double_click(&mut h, empty);
    h.run_steps(3);
    assert!(h.state().quick_add.is_some(), "a double click opens the box");
    h.event(egui::Event::Text("buff".into()));
    h.run_steps(2);
    shot(&mut h, "088-quick-add");
    assert!(h.query_all_by_label_contains("Buffer").next().is_some());
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().quick_add.is_none());
    let buffer = h.state().snarl.nodes().find(|n| n.kind == "Bufferer").expect("a buffer was added").id.clone();
    let rect = h.state().node_rect(&buffer).unwrap();
    assert!((rect.left() - h.state().screen_to_graph(empty).x).abs() < 30.0, "where the box was opened");
    // With the buffer selected, / adds the next tool after it, connected.
    pointer(&mut h, empty + egui::vec2(220.0, 0.0), None);
    h.event(egui::Event::Text("/".into()));
    h.run_steps(2);
    h.event(egui::Event::Text("sort".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let sorter = h.state().snarl.nodes().find(|n| n.kind == "Sorter").expect("a sorter was added").id.clone();
    assert!(
        wires(&h).contains(&format!("{}.Buffered > {}.Input", buffer, sorter))
            || wires(&h).iter().any(|w| w.starts_with(&format!("{}.", buffer)) && w.ends_with(&format!("{}.Input", sorter))),
        "{:?}",
        wires(&h)
    );
    // Escape closes the box with nothing added.
    double_click(&mut h, empty);
    h.run_steps(2);
    let count = h.state().snarl.nodes().count();
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().quick_add.is_none());
    assert_eq!(h.state().snarl.nodes().count(), count);
    // A connection from the output of Towns let go on empty canvas.
    let from = pin_at(&h, "in", "Output", false);
    let to = on_canvas(&h, 300.0, -200.0);
    drag(&mut h, from, to, egui::PointerButton::Primary);
    h.run_steps(3);
    h.event(egui::Event::Text("count".into()));
    h.run_steps(2);
    shot(&mut h, "088-dropped-connection");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let added: Vec<String> = h
        .state()
        .snarl
        .nodes()
        .filter(|n| !["in", "out"].contains(&n.id.as_str()) && n.kind != "Bufferer" && n.kind != "Sorter")
        .map(|n| n.kind.clone())
        .collect();
    assert_eq!(added.len(), 1, "{:?}", added);
    assert!(wires(&h).iter().any(|w| w.starts_with("in.Output > ") && !w.ends_with("out.Input")), "{:?}", wires(&h));
}

/// Selected nodes are lined up from the Edit menu, and Ctrl+Z puts them back.
#[test]
fn selected_nodes_are_lined_up() {
    let (mut h, _dir) = two_nodes();
    // The output a bit lower than the input.
    let grab = place_of(&h, "Result", 0, true);
    drag(&mut h, grab, grab + egui::vec2(0.0, 70.0), egui::PointerButton::Primary);
    h.run_steps(4);
    let top = |h: &Harness<'_, App>, id: &str| h.state().node_rect(id).unwrap().top();
    assert!((top(&h, "out") - top(&h, "in")).abs() > 30.0);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(3);
    click_text(&mut h, "Edit", 0, false);
    h.run_steps(2);
    let align = h.query_all_by_label_contains("Align").next().expect("Align in the Edit menu").rect().center();
    click_at(&mut h, align);
    h.run_steps(2);
    click_text(&mut h, "Top edges", 0, false);
    h.run_steps(4);
    assert!((top(&h, "out") - top(&h, "in")).abs() < 0.5, "{} {}", top(&h, "out"), top(&h, "in"));
    // One step back.
    for _ in 0..5 {
        h.run_steps(2);
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(4);
    assert!((top(&h, "out") - top(&h, "in")).abs() > 30.0, "undone");
}
