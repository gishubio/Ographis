//! The canvas by the keyboard: Tab and Shift+Tab go from node to node in reading order, the
//! arrows move the selected nodes, Enter goes to the form of the node, Ctrl+F finds a node
//! by its name or tool, and Ctrl+L connects the selected node to another. And a small map of
//! the whole model in a corner of the canvas.

use super::*;

/// A list to choose from by typing: nodes to find, or ports to connect to.
#[derive(Debug, Clone, Default)]
pub struct Picker {
    /// The node to connect from, when the list is of ports to connect to.
    pub from: Option<NodeId>,
    pub text: String,
    pub chosen: usize,
}

/// One entry of a picker: what is shown, and the node and the input port it stands for.
struct Entry {
    label: String,
    node: NodeId,
    input: Option<usize>,
}

impl App {
    /// Starts choosing the rectangle of a download on the map under the canvas.
    pub fn start_area_pick(&mut self, id: &str) {
        self.area_for = Some(id.to_string());
        self.map.osm = true;
        self.map.three_d = false;
        self.map.measuring = Some(crate::maptools::Measuring { what: crate::maptools::Measure::Rectangle, points: Vec::new() });
        self.say("Click two opposite corners on the map, and press Enter. Escape stops.".to_string());
    }

    /// While a rectangle is chosen, and there is nothing else to show: a map to choose it on.
    /// The rectangle the download has is drawn as its outline; without one, the map shows
    /// Sweden, framed by two points at its corners out at sea rather than an area over it.
    pub fn area_table(&self) -> Option<Arc<og_engine::Table>> {
        // While a rectangle is chosen; or, once chosen, while its download is the one
        // selected and has nothing else to show, so that the map stays.
        let choosing = self.area_for.is_some();
        let node = match &self.area_for {
            Some(id) => self.snarl.nodes().find(|n| &n.id == id)?,
            None => match self.selected.as_slice() {
                [one] => self.snarl.get_node(*one).filter(|n| matches!(n.kind.as_str(), "OsmDownloader" | "OvertureDownloader" | "PostgisReader"))?,
                _ => return None,
            },
        };
        let given: Vec<f64> =
            node.params.get("bbox").and_then(|v| v.as_str()).unwrap_or("").split([',', ';', ' ']).filter_map(|t| t.trim().parse().ok()).collect();
        let set = given.len() == 4 && given[0] < given[2] && given[1] < given[3];
        if !choosing && !set {
            return None;
        }
        let geom = if set {
            let ring = geo::Rect::new(geo::coord! { x: given[0], y: given[1] }, geo::coord! { x: given[2], y: given[3] }).to_polygon().exterior().clone();
            geo::Geometry::LineString(ring)
        } else {
            geo::Geometry::MultiPoint(geo::MultiPoint::new(vec![geo::point! { x: 10.5, y: 55.2 }, geo::point! { x: 24.2, y: 69.1 }]))
        };
        Some(Arc::new(og_engine::Table { crs: Some(4326), features: vec![og_engine::Feature::new(vec![], Some(geom))], ..Default::default() }))
    }

    /// Takes a rectangle chosen on the map into the download that asked for it.
    pub(super) fn take_picked_area(&mut self) {
        let Some(r) = self.map.picked_area.take() else {
            if self.area_for.is_some() && self.map.measuring.as_ref().is_none_or(|m| m.what != crate::maptools::Measure::Rectangle) {
                self.area_for = None;
            }
            return;
        };
        let Some(id) = self.area_for.take() else { return };
        let Some(nid) = self.snarl.nodes_ids_data().find(|(_, n)| n.value.id == id).map(|(k, _)| k) else { return };
        let text = format!("{:.5}, {:.5}, {:.5}, {:.5}", r[0], r[1], r[2], r[3]);
        self.snarl[nid].params.insert("bbox".into(), serde_json::Value::String(text.clone()));
        self.changed(true);
        self.say(format!("The rectangle {} is set.", text));
    }

    /// The nodes in reading order: by rows from the top, then from the left.
    fn reading_order(&self) -> Vec<NodeId> {
        let mut v: Vec<(NodeId, egui::Pos2)> = self.snarl.nodes_pos_ids().map(|(id, p, _)| (id, p)).collect();
        v.sort_by(|a, b| ((a.1.y / 60.0).round(), a.1.x).partial_cmp(&((b.1.y / 60.0).round(), b.1.x)).unwrap_or(std::cmp::Ordering::Equal));
        v.into_iter().map(|x| x.0).collect()
    }

    fn go_to(&mut self, node: NodeId) {
        self.set_selection(vec![node]);
        if let Some(id) = self.snarl.get_node(node).map(|n| n.id.clone()) {
            self.focus_node(&id);
            self.reveal(&id);
        }
    }

    /// The keys of the canvas, when no text field and no other widget has the keyboard.
    pub(super) fn canvas_keys(&mut self, ctx: &egui::Context) {
        // egui hands the focus on with Tab as well, to a field in a panel: after a Tab that
        // went to the nodes, that focus is let go of again.
        if self.tab_surrender > 0 {
            self.tab_surrender -= 1;
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            ctx.request_repaint();
        }
        if self.picker.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::F)) {
            self.picker = Some(Picker::default());
            return;
        }
        // F joins the selected nodes from left to right: the first output of each into the
        // first free input of the next. With fewer than two selected, F is a letter typed on
        // the canvas, which opens the quick-add box (see canvas).
        if self.selected.len() >= 2 && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F)) {
            self.connect_selected();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::L)) {
            match self.selected.as_slice() {
                [one] => self.picker = Some(Picker { from: Some(*one), ..Default::default() }),
                _ => self.say("Select one node to connect, then Ctrl+L."),
            }
            return;
        }
        // Tab is for the nodes when no widget has the keyboard, or when the canvas is in use:
        // a node is selected or the pointer is over it. Then a button that egui gave the
        // focus to on Tab lets go of it.
        let focused = ctx.memory(|m| m.focused());
        if focused.is_some() && self.selected.is_empty() && self.pointer_on_canvas.is_none() {
            return;
        }
        let surrender = |ctx: &egui::Context| {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        };
        let back = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::Tab));
        let next = !back && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));
        if next || back {
            let order = self.reading_order();
            if order.is_empty() {
                return;
            }
            let at = self.selected.first().and_then(|s| order.iter().position(|n| n == s));
            let k = match (at, next) {
                (None, true) => 0,
                (None, false) => order.len() - 1,
                (Some(i), true) => (i + 1) % order.len(),
                (Some(i), false) => (i + order.len() - 1) % order.len(),
            };
            self.go_to(order[k]);
            surrender(ctx);
            self.tab_surrender = 2;
            return;
        }
        if self.selected.is_empty() {
            return;
        }
        let (dx, dy, step) = ctx.input(|i| {
            let step = if i.modifiers.shift { 60.0 } else { 12.0 };
            let x = (i.key_pressed(Key::ArrowRight) as i32 - i.key_pressed(Key::ArrowLeft) as i32) as f32;
            let y = (i.key_pressed(Key::ArrowDown) as i32 - i.key_pressed(Key::ArrowUp) as i32) as f32;
            (x, y, step)
        });
        if (dx != 0.0 || dy != 0.0) && !ctx.input(|i| i.modifiers.command) {
            for n in self.selected.clone() {
                if let Some(info) = self.snarl.get_node_info_mut(n) {
                    info.pos += egui::vec2(dx * step, dy * step);
                }
            }
            self.touch();
            if let [one] = self.selected.as_slice() {
                if let Some(id) = self.snarl.get_node(*one).map(|n| n.id.clone()) {
                    self.reveal(&id);
                }
            }
            return;
        }
        // Enter takes a rectangle drawn on the map; the map sees it after the canvas.
        let rectangle_drawn = self.map.measuring.as_ref().is_some_and(|m| m.what == crate::maptools::Measure::Rectangle && m.points.len() == 2);
        if !rectangle_drawn && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            // To the form: the first field of the selected node gets the keyboard.
            self.focus_form = 1;
        }
    }

    fn picker_entries(&self, p: &Picker) -> Vec<Entry> {
        let words: Vec<String> = p.text.to_lowercase().split_whitespace().map(str::to_string).collect();
        let fits = |label: &str| {
            let l = label.to_lowercase();
            words.iter().all(|w| l.contains(w.as_str()))
        };
        let mut out = Vec::new();
        for id in self.reading_order() {
            let Some(n) = self.snarl.get_node(id) else { continue };
            if n.is_note() {
                continue;
            }
            let tool = n.ty().map_or(n.kind.as_str(), |t| t.title);
            match p.from {
                None => {
                    let label = if n.name == tool { n.name.clone() } else { format!("{} · {}", n.name, tool) };
                    if fits(&label) {
                        out.push(Entry { label, node: id, input: None });
                    }
                }
                Some(from) if from != id => {
                    for (k, port) in n.inputs().iter().enumerate() {
                        let label = format!("{} · {}", n.name, port);
                        if fits(&label) {
                            out.push(Entry { label, node: id, input: Some(k) });
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The picker of Ctrl+F and Ctrl+L, while it is open.
    pub(super) fn picker_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut p) = self.picker.take() else { return };
        let entries = self.picker_entries(&p);
        p.chosen = p.chosen.min(entries.len().saturating_sub(1));
        let mut done: Option<Option<usize>> = None;
        let from_name = p.from.and_then(|f| self.snarl.get_node(f)).map(|n| n.name.clone());
        let modal = egui::Modal::new(egui::Id::new("picker")).show(ctx, |ui| {
            ui.set_width(460.0);
            let title = match &from_name {
                Some(n) => format!("Connect {} to…", n),
                None => "Find a node".to_string(),
            };
            ui.label(RichText::new(title).font(theme::strong(16.0)).color(pal.ink));
            let field = ui.add(egui::TextEdit::singleline(&mut p.text).hint_text("Type a part of the name or the tool").desired_width(f32::INFINITY));
            field.request_focus();
            if field.changed() {
                p.chosen = 0;
            }
            let (down, up, enter, esc) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, Key::Escape),
                )
            });
            if down && p.chosen + 1 < entries.len() {
                p.chosen += 1;
            }
            if up && p.chosen > 0 {
                p.chosen -= 1;
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(RichText::new("Nothing matches.").color(pal.muted));
                }
                for (k, e) in entries.iter().enumerate() {
                    let r = ui.selectable_label(k == p.chosen, &e.label);
                    if k == p.chosen && (down || up) {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        done = Some(Some(k));
                    }
                }
            });
            ui.label(RichText::new("Up and Down to choose, Enter to take it, Escape to close").small().color(pal.muted));
            if enter && !entries.is_empty() {
                done = Some(Some(p.chosen));
            }
            if esc {
                done = Some(None);
            }
        });
        if modal.should_close() && done.is_none() {
            done = Some(None);
        }
        match done {
            None => self.picker = Some(p),
            Some(None) => {}
            Some(Some(k)) => {
                let e = &entries[k];
                match (p.from, e.input) {
                    (Some(from), Some(input)) => {
                        let to = egui_snarl::InPinId { node: e.node, input };
                        self.snarl.connect(egui_snarl::OutPinId { node: from, output: 0 }, to);
                        self.changed(true);
                        self.say(format!("Connected to {}.", e.label));
                        self.go_to(from);
                    }
                    _ => self.go_to(e.node),
                }
            }
        }
    }

    /// A small map of the whole model in the lower right corner of the canvas, with the part
    /// that is seen marked: a click or a drag on it moves the view there.
    pub(super) fn minimap(&mut self, ui: &mut Ui, pal: &Palette) {
        if !self.show_minimap || self.snarl.nodes_pos_ids().count() < 2 {
            return;
        }
        let Some(world) = self.bounds() else { return };
        let seen = self.transform.inverse() * self.canvas_rect;
        let all = world.union(seen).expand(40.0);
        let size = egui::vec2(180.0, 120.0);
        let scale = (size.x / all.width()).min(size.y / all.height());
        let box_size = all.size() * scale;
        let at = Rect::from_min_size(self.canvas_rect.max - box_size - egui::vec2(18.0, 18.0), box_size);
        let to_map = |p: egui::Pos2| at.min + (p - all.min) * scale;
        let painter = ui.painter_at(at.expand(2.0));
        painter.rect_filled(at, 4.0, pal.surface.gamma_multiply(0.92));
        painter.rect_stroke(at, 4.0, egui::Stroke::new(1.0, pal.line), egui::StrokeKind::Inside);
        for (id, pos, n) in self.snarl.nodes_pos_ids() {
            let r = self.node_rects.iter().find(|x| x.0 == id).map_or(Rect::from_min_size(pos, egui::vec2(190.0, 110.0)), |x| x.1);
            let colour = if self.selected.contains(&id) { pal.accent } else { n.ty().map_or(pal.muted, |t| pal.group(t.group).0) };
            painter.rect_filled(Rect::from_min_max(to_map(r.min), to_map(r.max)), 1.5, colour);
        }
        painter.rect_stroke(Rect::from_min_max(to_map(seen.min), to_map(seen.max)), 2.0, egui::Stroke::new(1.5, pal.ink), egui::StrokeKind::Inside);
        let r = ui.interact(at, egui::Id::new("minimap"), egui::Sense::click_and_drag());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "A map of the whole model: click to go there"));
        if r.clicked() || r.dragged() {
            if let Some(p) = r.interact_pointer_pos() {
                let target = all.min + (p - at.min) / scale;
                let s = self.transform.scaling;
                self.fit = Some(TSTransform::new(self.canvas_rect.center().to_vec2() - target.to_vec2() * s, s));
            }
        }
    }
}

impl App {
    /// Connects the selected nodes in a row, as they stand from left to right.
    pub(super) fn connect_selected(&mut self) {
        let mut nodes: Vec<(egui_snarl::NodeId, f32)> = self.selected.iter().filter_map(|id| self.snarl.get_node_info(*id).map(|n| (*id, n.pos.x))).collect();
        if nodes.len() < 2 {
            self.say("Select two or more nodes to connect them from left to right, then F.");
            return;
        }
        nodes.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut made = 0;
        for w in nodes.windows(2) {
            let (from, to) = (w[0].0, w[1].0);
            if self.snarl[from].outputs().is_empty() {
                continue;
            }
            let inputs = self.snarl[to].inputs().len();
            let taken = |k: usize| self.snarl.wires().any(|(_, i)| i.node == to && i.input == k);
            // An input that is free, or the first, which takes several connections.
            let Some(k) = (0..inputs).find(|k| !taken(*k)).or((inputs > 0).then_some(0)) else { continue };
            let (o, i) = (egui_snarl::OutPinId { node: from, output: 0 }, egui_snarl::InPinId { node: to, input: k });
            if self.snarl.wires().any(|(a, b)| a == o && b == i) {
                continue;
            }
            self.snarl.connect(o, i);
            made += 1;
        }
        if made > 0 {
            self.changed(true);
            self.say(format!("{} {} made.", made, if made == 1 { "connection" } else { "connections" }));
        }
    }
}
