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

    /// What goes through a connection, shown while the pointer is on it: how many features,
    /// of what kind and with which attributes, from the latest run.
    pub(super) fn wire_tooltip(&self, ui: &Ui, from: OutPinId, to: InPinId, pal: &Palette) {
        let mismatch = graph::wire_mismatch(&self.snarl, from, to);
        let Some(n) = self.snarl.get_node(from.node) else { return };
        let Some(port) = n.outputs().get(from.output).cloned() else { return };
        let table = self.result.as_ref().and_then(|r| r.nodes.get(&n.id)).and_then(|r| r.outputs.get(&port)).cloned();
        let stale = self.stale;
        let id = egui::Id::new(("wire-tip", from));
        egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), id, egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(RichText::new(format!("Out of {} · {}", n.name, port)).strong());
            if let Some(said) = &mismatch {
                ui.label(RichText::new(format!("⚠ {}", said)).color(pal.warn));
            }
            let Some(t) = table else {
                ui.label(RichText::new("Run the model to see what goes through").color(pal.muted));
                return;
            };
            let kinds = geometry_kinds(&t);
            let what = carried(&t).unwrap_or_else(|| format!("{} features", fmt_count(t.features.len() as u64)));
            ui.label(if kinds.is_empty() || t.image.is_some() || t.grid.is_some() { what } else { format!("{}, {}", what, kinds) });
            if let Some(crs) = t.crs {
                ui.label(RichText::new(crs_label(Some(crs))).small().color(pal.muted));
            }
            if !t.fields.is_empty() {
                let names: Vec<&str> = t.fields.iter().take(12).map(|f| f.name.as_str()).collect();
                let more = if t.fields.len() > 12 { format!(" and {} more", t.fields.len() - 12) } else { String::new() };
                ui.label(RichText::new(format!("{}{}", names.join(", "), more)).small());
            }
            if stale {
                ui.label(RichText::new("From the latest run; the model has changed since").small().color(pal.warn));
            }
        });
    }

    /// What the connection goes between, in words.
    pub(super) fn wire_text(&self, wire: (OutPinId, InPinId)) -> Option<(String, String)> {
        let (a, b) = (self.snarl.get_node(wire.0.node)?, self.snarl.get_node(wire.1.node)?);
        let (out, inp) = (a.outputs().get(wire.0.output)?.clone(), b.inputs().get(wire.1.input)?.clone());
        Some((format!("{} · {}", a.name, out), format!("{} · {}", b.name, inp)))
    }

    /// Lifts nodes out of their connections, and joins what came in to where it went, so
    /// that the flow goes on without them.
    pub(super) fn detach(&mut self, nodes: &[NodeId]) {
        let mut changed = false;
        for &node in nodes {
            let wires: Vec<(OutPinId, InPinId)> = self.snarl.wires().filter(|(o, i)| o.node == node || i.node == node).collect();
            if wires.is_empty() {
                continue;
            }
            let into: Vec<OutPinId> = wires.iter().filter(|(o, i)| i.node == node && i.input == 0 && !nodes.contains(&o.node)).map(|w| w.0).collect();
            let out_of: Vec<InPinId> = wires.iter().filter(|(o, i)| o.node == node && o.output == 0 && !nodes.contains(&i.node)).map(|w| w.1).collect();
            for (o, i) in &wires {
                self.snarl.disconnect(*o, *i);
            }
            for o in &into {
                for i in &out_of {
                    self.snarl.connect(*o, *i);
                }
            }
            changed = true;
        }
        if changed {
            self.changed(true);
        }
    }

    /// Follows a drag of nodes with Alt held: the first move lifts them out of their
    /// connections.
    pub(super) fn alt_drag_detach(&mut self, ctx: &egui::Context) {
        let (alt, down) = ctx.input(|i| (i.modifiers.alt, i.pointer.primary_down()));
        if !down || self.selected.is_empty() {
            self.alt_drag = (Vec::new(), false);
            return;
        }
        let now: Vec<(NodeId, egui::Pos2)> = self.selected.iter().filter_map(|id| self.snarl.get_node_info(*id).map(|n| (*id, n.pos))).collect();
        let moved = !self.alt_drag.0.is_empty() && now.iter().any(|(id, p)| self.alt_drag.0.iter().any(|(j, q)| j == id && q != p));
        if alt && moved && !self.alt_drag.1 {
            let nodes: Vec<NodeId> = now.iter().map(|n| n.0).collect();
            self.detach(&nodes);
            self.alt_drag.1 = true;
        }
        self.alt_drag.0 = now;
    }
}
