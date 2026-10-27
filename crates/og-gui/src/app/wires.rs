//! The connections: turned off and on, their bend points, and ports that move.

use super::*;

impl App {
    // ---------- Connections and switches ----------

    /// Removes the selected connection.
    /// Turns a connection off or on. A connection that is off lets nothing through.
    pub fn toggle_wire(&mut self, wire: (OutPinId, InPinId)) {
        let off = graph::wire_disabled(&self.snarl, wire.0, wire.1);
        graph::set_wire_disabled(&mut self.snarl, wire.0, wire.1, !off);
        self.changed(true);
    }

    /// The menu of a connection: turn it off or on, or delete it.
    pub(super) fn wire_menu_ui(&mut self, ctx: &egui::Context) {
        let Some((wire, at)) = self.wire_menu else { return };
        if !self.snarl.wires().any(|w| w == wire) {
            self.wire_menu = None;
            return;
        }
        let off = graph::wire_disabled(&self.snarl, wire.0, wire.1);
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("wire-menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                if ui.button(if off { "Enable connection" } else { "Disable connection" }).clicked() {
                    self.toggle_wire(wire);
                    close = true;
                }
                if ui.button("Add bend point").on_hover_text("A point the connection is drawn through, to drag where you like").clicked() {
                    self.add_bend(wire, self.transform.inverse() * at);
                    close = true;
                }
                if !graph::wire_bends(&self.snarl, wire.0, wire.1).is_empty() && ui.button("Remove all bend points").clicked() {
                    graph::set_wire_bends(&mut self.snarl, wire.0, wire.1, Vec::new());
                    self.changed(false);
                    close = true;
                }
                ui.separator();
                if ui.button("Delete connection").clicked() {
                    self.selected_wire = Some(wire);
                    self.remove_selected_wire();
                    close = true;
                }
            });
        });
        // A click elsewhere, or Esc, closes the menu.
        let elsewhere = ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !ctx.input(|i| i.pointer.secondary_pressed());
        if close || elsewhere || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.wire_menu = None;
        }
    }

    /// Puts a point into a connection, at a place on the canvas.
    pub fn add_bend(&mut self, wire: (OutPinId, InPinId), at: egui::Pos2) {
        let rect = |n: NodeId| self.node_rects.iter().find(|r| r.0 == n).map(|r| r.1);
        let start = rect(wire.0.node).map_or(at, |r| r.right_center());
        let end = rect(wire.1.node).map_or(at, |r| r.left_center());
        let pts = graph::insert_bend(&self.snarl, wire.0, wire.1, start, end, at);
        graph::set_wire_bends(&mut self.snarl, wire.0, wire.1, pts);
        self.changed(false);
    }

    /// The menu of a point of a connection: it is taken away from there.
    pub(super) fn bend_menu_ui(&mut self, ctx: &egui::Context) {
        let Some((wire, index, at)) = self.bend_menu else { return };
        if !self.snarl.wires().any(|w| w == wire) {
            self.bend_menu = None;
            return;
        }
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("bend-menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                if ui.button("Remove bend point").clicked() {
                    let mut pts = graph::wire_bends(&self.snarl, wire.0, wire.1);
                    if index < pts.len() {
                        pts.remove(index);
                    }
                    graph::set_wire_bends(&mut self.snarl, wire.0, wire.1, pts);
                    self.changed(false);
                    close = true;
                }
                if ui.button("Remove all bend points").clicked() {
                    graph::set_wire_bends(&mut self.snarl, wire.0, wire.1, Vec::new());
                    self.changed(false);
                    close = true;
                }
            });
        });
        let elsewhere = ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !ctx.input(|i| i.pointer.secondary_pressed());
        if close || elsewhere || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.bend_menu = None;
        }
    }

    pub(super) fn remove_selected_wire(&mut self) {
        if let Some((from, to)) = self.selected_wire.take() {
            if self.snarl.disconnect(from, to) {
                self.changed(true);
            }
        }
    }

    /// Switches nodes off, or on again: all of them the way the first one goes.
    pub fn toggle_disabled(&mut self, nodes: &[NodeId]) {
        let Some(off) = nodes.iter().filter_map(|n| self.snarl.get_node(*n)).find(|n| !n.is_note()).map(|n| !n.disabled) else { return };
        for n in nodes {
            if let Some(node) = self.snarl.get_node_mut(*n).filter(|x| !x.is_note()) {
                node.disabled = off;
            }
        }
        self.changed(true);
    }

    /// Moves a port of a node up or down, and its connections with it.
    pub fn move_port(&mut self, node: NodeId, output: bool, index: usize, up: bool, now: f64) {
        let Some(n) = self.snarl.get_node_mut(node) else { return };
        let (outs, ins) = (n.outputs(), n.inputs());
        let name = if output { outs.get(index) } else { ins.get(index) }.cloned().unwrap_or_default();
        if n.move_port(output, index, up) {
            self.moved_port = Some((node, output, name, now));
            self.follow_ports(node, &outs);
            self.follow_inputs(node, &ins);
            self.touch();
        }
    }

    /// Moves the connections into a node whose input ports have changed order to the ports
    /// with the same names.
    pub(super) fn follow_inputs(&mut self, node: NodeId, before: &[String]) {
        let after = self.snarl[node].inputs();
        let wires: Vec<(OutPinId, InPinId)> = self.snarl.wires().filter(|(_, i)| i.node == node).collect();
        for (o, i) in &wires {
            self.snarl.disconnect(*o, *i);
        }
        for (o, i) in wires {
            if let Some(k) = before.get(i.input).and_then(|n| after.iter().position(|a| a == n)) {
                self.snarl.connect(o, InPinId { node, input: k });
            }
        }
    }

    /// Moves the connections from a node whose output ports have changed to the ports with
    /// the same names. A connection from a port that is gone is removed.
    pub(super) fn follow_ports(&mut self, node: NodeId, before: &[String]) {
        let after = self.snarl[node].outputs();
        let wires: Vec<(OutPinId, InPinId)> = self.snarl.wires().filter(|(o, _)| o.node == node).collect();
        for (o, i) in &wires {
            self.snarl.disconnect(*o, *i);
        }
        for (o, i) in wires {
            let name = before.get(o.output);
            if let Some(k) = name.and_then(|n| after.iter().position(|a| a == n)) {
                self.snarl.connect(OutPinId { node, output: k }, i);
            }
        }
    }

    /// What the connection goes between, in words.
    pub(super) fn wire_text(&self, wire: (OutPinId, InPinId)) -> Option<(String, String)> {
        let (a, b) = (self.snarl.get_node(wire.0.node)?, self.snarl.get_node(wire.1.node)?);
        let (out, inp) = (a.outputs().get(wire.0.output)?.clone(), b.inputs().get(wire.1.input)?.clone());
        Some((format!("{} · {}", a.name, out), format!("{} · {}", b.name, inp)))
    }
}
