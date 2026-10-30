//! The canvas with the nodes, the connections and the group boxes.

use super::*;

impl App {
    pub(super) fn canvas(&mut self, ui: &mut Ui, pal: &Palette) {
        self.canvas_rect = ui.max_rect();
        self.pointer_on_canvas = ui.ctx().pointer_hover_pos().filter(|p| self.canvas_rect.contains(*p));
        if let Some(min_scale) = self.fit_requested.take() {
            if let Some(b) = self.bounds() {
                self.fit = Some(graph::fit_transform(b.expand(24.0), self.canvas_rect, min_scale, 1.0));
            }
        }
        if let Some(nodes) = self.select.take() {
            egui_snarl::ui::set_selected_nodes(graph::canvas_id(), ui.ctx(), &nodes);
        }
        let mut events = CanvasEvents::default();
        let result = self.result.clone();
        let files = self.folder_counts();
        let running = self.running_node.clone();
        // A moved port is marked for two seconds.
        let now = ui.input(|i| i.time);
        let moved = self.moved_port.as_ref().filter(|m| now - m.3 < 2.0).map(|m| (m.0, m.1, m.2.clone()));
        // The dots along the connections into a node at work keep moving.
        if self.running_node.is_some() && !self.paused {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        if moved.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        let before: Vec<(NodeId, egui::Pos2)> = self.snarl.nodes_pos_ids().map(|(id, p, _)| (id, p)).collect();
        let followers = self.boxes_with_selection();
        let cache_state = self.cache_state(now);
        let background_clicked;
        {
            let view = match (&self.view.node, self.viewed()) {
                (Some(n), Some((ports, chosen))) => Some((n.clone(), ports[chosen].0.clone())),
                _ => None,
            };
            let mut viewer = Viewer {
                pal: *pal,
                result: result.as_deref(),
                stale: self.stale,
                view: view.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
                selected: &self.selected,
                events: &mut events,
                transform: &mut self.transform,
                fit: &mut self.fit,
                running: running.as_deref(),
                flow_phase: (now * 1.4).fract() as f32,
                files: &files,
                expanded: &self.expanded,
                groups: &self.groups,
                selected_group: self.selected_group,
                sizes: &self.node_rects,
                selected_wire: self.selected_wire,
                moved_port: moved.as_ref().map(|(n, o, name)| (*n, *o, name.as_str())),
                cache: self.use_cache,
                busy: self.live,
                paused: self.paused,
                cache_state: &cache_state,
            };
            let canvas = SnarlWidget::new().id(graph::canvas_id()).style(graph::snarl_style(*pal)).show(&mut self.snarl, &mut viewer, ui);
            background_clicked = canvas.clicked();
            // A double click on empty canvas opens the quick-add box there. Counted here, with
            // the half second Windows gives a double click.
            if canvas.clicked() {
                // In the coordinates of the screen: those of the canvas response are its own.
                let (now, p) = (ui.input(|i| i.time), ui.ctx().input(|i| i.pointer.interact_pos()));
                if let Some(p) = p {
                    let double = canvas.double_clicked() || self.canvas_click.is_some_and(|(t, q)| now - t <= 0.55 && q.distance(p) < 6.0);
                    if double {
                        self.quick_add = Some((p, self.transform.inverse() * p, None));
                        self.canvas_click = None;
                    } else {
                        self.canvas_click = Some((now, p));
                    }
                }
            }
        }
        // So does / with the pointer over the canvas; with one node selected, the new tool
        // comes after it, connected.
        if self.quick_add.is_none()
            && !ui.ctx().egui_wants_keyboard_input()
            && self.pointer_on_canvas.is_some()
            && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "/")))
        {
            let p = self.pointer_on_canvas.unwrap_or(self.canvas_rect.center());
            let after = (self.selected.len() == 1)
                .then(|| self.selected[0])
                .filter(|n| self.snarl.get_node(*n).is_some_and(|g| g.ty().is_some_and(|t| !t.outputs.is_empty())));
            self.quick_add = Some((p, self.transform.inverse() * p, after));
        }
        self.quick_add_box(ui.ctx(), pal);
        self.node_rects = std::mem::take(&mut events.rects);
        if background_clicked {
            self.selected_group = None;
            self.selected_wire = None;
        }
        // A connection that has been removed some other way is no longer selected.
        if self.selected_wire.is_some_and(|w| !self.snarl.wires().any(|x| x == w)) {
            self.selected_wire = None;
        }
        if let Some(wire) = events.wire_clicked {
            self.set_selection(Vec::new());
            self.selected_group = None;
            self.selected_wire = Some(wire);
        }
        if let Some((wire, at)) = events.wire_menu {
            self.set_selection(Vec::new());
            self.selected_group = None;
            self.selected_wire = Some(wire);
            self.wire_menu = Some((wire, at));
        }
        self.wire_menu_ui(ui.ctx());
        if let Some(b) = events.bend_menu {
            self.wire_menu = None;
            self.bend_menu = Some(b);
        }
        self.bend_menu_ui(ui.ctx());
        if events.moved_bend {
            self.touch();
        }
        if let Some(n) = events.toggle_disabled {
            self.toggle_disabled(&self.targets(n));
        }
        if let Some((n, output, index, up)) = events.move_port {
            self.move_port(n, output, index, up, ui.input(|i| i.time));
        }
        if self.fit_frames > 0 {
            self.fit_frames -= 1;
            self.fit_requested = Some(READABLE_SCALE);
            ui.ctx().request_repaint();
        }
        // A node that was dragged makes the workspace unsaved. Positions are compared with
        // some slack, so that rounding alone never does.
        let after: Vec<(NodeId, egui::Pos2)> = self.snarl.nodes_pos_ids().map(|(id, p, _)| (id, p)).collect();
        if before.len() == after.len() {
            let moved: Vec<(NodeId, egui::Vec2)> = before.iter().zip(&after).filter(|(a, b)| a.0 == b.0 && a.1 != b.1).map(|(a, b)| (a.0, b.1 - a.1)).collect();
            // When the selected nodes are dragged, the boxes around them go the same way.
            // The steps of the nodes can differ by a fraction of a pixel when the screen is
            // scaled, such as 125 % or 150 % in Windows, so they are compared with slack and
            // the boxes follow the average.
            if !moved.is_empty() {
                let by = moved.iter().fold(egui::Vec2::ZERO, |a, (_, d)| a + *d) / moved.len() as f32;
                let together = moved.len() == self.selected.len() && moved.iter().all(|(n, d)| self.selected.contains(n) && (*d - by).length() < 0.75);
                if together && self.group_drag.is_none() {
                    for k in &followers {
                        if let Some(g) = self.groups.get_mut(*k) {
                            g.x += by.x as f64;
                            g.y += by.y as f64;
                        }
                    }
                    let nodes: Vec<NodeId> = moved.iter().map(|m| m.0).collect();
                    graph::shift_bends(&mut self.snarl, &nodes, by);
                }
            }
            if moved.iter().any(|(_, d)| d.length() > 0.5) {
                self.touch();
            }
        }
        let now = egui_snarl::ui::get_selected_nodes(graph::canvas_id(), ui.ctx());
        let same = now.len() == self.selected.len() && now.iter().all(|n| self.selected.contains(n));
        if !same && self.select.is_none() {
            self.selection_changed(now);
        }
        if let Some(n) = events.added {
            if let Some(id) = self.snarl.get_node(n).map(|x| x.id.clone()) {
                self.focus_node(&id);
            }
        }
        for (index, event) in std::mem::take(&mut events.groups) {
            match event {
                GroupEvent::Clicked => self.select_group(index),
                GroupEvent::DragStarted => match self.selection_drag(index) {
                    // A box that belongs to the selection takes all of it along.
                    Some(d) => self.group_drag = Some(d),
                    None => {
                        self.select_group(index);
                        self.group_drag = Some(self.group_members(index));
                    }
                },
                GroupEvent::Moved(by) => self.move_group(index, by),
                GroupEvent::Resized(by, sides) => self.resize_group(index, by, sides),
                GroupEvent::ToggleLock => {
                    if let Some(g) = self.groups.get_mut(index) {
                        g.locked = !g.locked;
                        self.touch();
                    }
                }
                GroupEvent::Delete => self.remove_group(index),
            }
        }
        for (node, event) in std::mem::take(&mut events.comments) {
            self.comment_event(ui.ctx(), node, event);
        }
        // What follows a box is decided anew for the next drag.
        if ui.ctx().input(|i| !i.pointer.any_down()) {
            self.group_drag = None;
        }
        if let Some(pos) = events.add_group {
            self.add_group(&[], Some(pos));
        }
        if let Some(n) = events.group_nodes {
            self.add_group(&self.targets(n), None);
        }
        if let Some(id) = events.toggle_attributes.take() {
            if !self.expanded.remove(&id) {
                self.expanded.insert(id);
            }
        }
        if let Some((n, port)) = events.inspect {
            if let Some(id) = self.snarl.get_node(n).map(|x| x.id.clone()) {
                self.set_selection(vec![n]);
                self.view = View { node: Some(id), port: Some(port), feature: None, more: Vec::new() };
                self.tab = Tab::Data;
            }
        }
        if let Some(how) = events.align {
            self.align_selected(how);
        }
        if let Some(n) = events.duplicate {
            self.duplicate_nodes(&self.targets(n));
        }
        if let Some(id) = events.run_to.and_then(|n| self.snarl.get_node(n)).map(|n| n.id.clone()) {
            self.run_to(ui.ctx(), &id);
        }
        if let Some(id) = events.run_from.and_then(|n| self.snarl.get_node(n)).map(|n| n.id.clone()) {
            self.run_from(ui.ctx(), &id);
        }
        if let Some(n) = events.remove {
            self.remove_nodes(&self.targets(n));
        }
        if let Some(n) = events.copy {
            let targets = self.targets(n);
            self.set_selection(targets);
            self.copy_selected(ui.ctx(), false);
        }
        if let Some(at) = events.paste {
            self.paste_from_menu(Some(at));
        }
        if let Some(n) = events.show_folder {
            self.show_folder_of(n);
        }
        if events.changed {
            self.changed(events.rewired);
        }
        if let Some(m) = events.message {
            self.say(m);
        }

        // The button floats above the canvas. It has to be in a layer of its own: the
        // canvas is drawn in a layer above this panel, and would otherwise take the click.
        let corner = self.canvas_rect.right_top() + egui::vec2(-104.0, 10.0);
        egui::Area::new(egui::Id::new("canvas_tools")).order(egui::Order::Middle).fixed_pos(corner).constrain(false).show(ui.ctx(), |ui| {
            let hint = "Show all nodes (Home). Scroll to zoom. Hold Space and drag, or use the middle mouse button, to move the canvas.";
            if ui.add(egui::Button::new("Zoom to fit").min_size(egui::vec2(92.0, 26.0))).on_hover_text(hint).clicked() {
                self.fit_requested = Some(SMALLEST_SCALE);
            }
        });

        self.scrollbars(ui.ctx(), pal);
        self.description_plate(ui.ctx(), pal);

        if self.snarl.nodes().next().is_none() && self.start_screen {
            self.start_screen_ui(ui.ctx(), pal);
        } else if self.snarl.nodes().next().is_none() {
            // On a plate of its own above the canvas, so that the grid does not run through it.
            let at = self.canvas_rect.center();
            egui::Area::new(egui::Id::new("canvas_empty"))
                .order(egui::Order::Middle)
                .pivot(egui::Align2::CENTER_CENTER)
                .fixed_pos(at)
                .interactable(false)
                .constrain(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(pal.panel)
                        .stroke(egui::Stroke::new(1.0, pal.line))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(18, 12))
                        .show(ui, |ui| {
                            ui.label(RichText::new("The canvas is empty").font(theme::strong(15.0)).color(pal.ink));
                            ui.label(
                                RichText::new("Add an input from the list on the left, or drop a data file or a folder here.").size(14.0).color(pal.muted),
                            );
                        });
                });
        }
        // While files are held over the window, say what dropping them does.
        if ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop_hint")));
            let area = self.canvas_rect.shrink(12.0);
            painter.rect(area, 8.0, pal.accent_soft, egui::Stroke::new(2.0, pal.accent), egui::StrokeKind::Inside);
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to add an input for each file or folder, or to open a model",
                theme::strong(16.0),
                pal.ink,
            );
        }
    }

    /// Scroll bars along the bottom and the right of the canvas when the model reaches
    /// beyond what is seen: how much of it is in view, and where. Dragging one moves the
    /// canvas, and a click beside the bar jumps a view's width that way.
    fn scrollbars(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(content) = self.bounds() else { return };
        let t = self.transform;
        if t.scaling <= 0.0 {
            return;
        }
        let canvas = self.canvas_rect;
        let seen = egui::Rect::from_min_max(t.inverse() * canvas.min, t.inverse() * canvas.max);
        let content = content.expand(40.0);
        if seen.contains_rect(content) {
            return;
        }
        let total = seen.union(content);
        const THICK: f32 = 9.0;
        for horizontal in [true, false] {
            let (lo, hi, a, b) =
                if horizontal { (total.min.x, total.max.x, seen.min.x, seen.max.x) } else { (total.min.y, total.max.y, seen.min.y, seen.max.y) };
            // Only a direction in which there is more to see gets a bar.
            let more =
                if horizontal { content.min.x < seen.min.x || content.max.x > seen.max.x } else { content.min.y < seen.min.y || content.max.y > seen.max.y };
            if !more {
                continue;
            }
            let track = if horizontal {
                egui::Rect::from_min_max(
                    egui::pos2(canvas.left() + 6.0, canvas.bottom() - THICK - 4.0),
                    egui::pos2(canvas.right() - THICK - 12.0, canvas.bottom() - 4.0),
                )
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(canvas.right() - THICK - 4.0, canvas.top() + 44.0),
                    egui::pos2(canvas.right() - 4.0, canvas.bottom() - THICK - 12.0),
                )
            };
            let span = (hi - lo).max(1.0);
            let length = if horizontal { track.width() } else { track.height() };
            let start = (a - lo) / span * length;
            let size = ((b - a) / span * length).max(24.0).min(length);
            let thumb = if horizontal {
                egui::Rect::from_min_size(egui::pos2(track.left() + start.min(length - size), track.top()), egui::vec2(size, THICK))
            } else {
                egui::Rect::from_min_size(egui::pos2(track.left(), track.top() + start.min(length - size)), egui::vec2(THICK, size))
            };
            let id = egui::Id::new(("canvas_scrollbar", horizontal));
            let mut shift = 0.0f32;
            egui::Area::new(id).order(egui::Order::Middle).fixed_pos(track.min).constrain(false).show(ctx, |ui| {
                let (_, track_resp) = ui.allocate_exact_size(track.size(), egui::Sense::click());
                let thumb_resp = ui.interact(thumb, id.with("thumb"), egui::Sense::drag());
                let painter = ui.painter();
                painter.rect_filled(track, THICK / 2.0, pal.line.gamma_multiply(0.25));
                let active = thumb_resp.hovered() || thumb_resp.dragged();
                painter.rect_filled(thumb, THICK / 2.0, if active { pal.muted } else { pal.line });
                if thumb_resp.dragged() {
                    let d = thumb_resp.drag_delta();
                    shift = (if horizontal { d.x } else { d.y }) / length * span;
                } else if track_resp.clicked() {
                    if let Some(p) = track_resp.interact_pointer_pos() {
                        let at = if horizontal { p.x - thumb.center().x } else { p.y - thumb.center().y };
                        shift = at.signum() * (b - a) * 0.9;
                    }
                }
                thumb_resp.on_hover_text("Drag to move the canvas");
            });
            if shift != 0.0 {
                let mut moved = t;
                if horizontal {
                    moved.translation.x -= shift * t.scaling;
                } else {
                    moved.translation.y -= shift * t.scaling;
                }
                self.fit = Some(moved);
                ctx.request_repaint();
            }
        }
    }

    /// The quick-add box, while it is open: the tool chosen is put where the box was opened,
    /// or after the selected node and connected to it.
    fn quick_add_box(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some((screen, at, after)) = self.quick_add else { return };
        let want = if after.is_some() { crate::quickadd::Want::WithInput } else { crate::quickadd::Want::Any };
        let mut outcome = crate::quickadd::Outcome::Open;
        let area = egui::Area::new(egui::Id::new("quick_add")).order(egui::Order::Foreground).fixed_pos(screen).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                outcome = crate::quickadd::show(ui, pal, egui::Id::new("quick_add_box"), want);
            });
        });
        // A click beside the box closes it.
        if outcome == crate::quickadd::Outcome::Open
            && ctx.input(|i| i.pointer.any_pressed())
            && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p))
        {
            outcome = crate::quickadd::Outcome::Cancelled;
            ctx.data_mut(|d| d.remove::<(String, usize)>(egui::Id::new("quick_add_box")));
        }
        match outcome {
            crate::quickadd::Outcome::Open => {}
            crate::quickadd::Outcome::Cancelled => self.quick_add = None,
            crate::quickadd::Outcome::Picked(kind) => {
                self.quick_add = None;
                let pos = match after.and_then(|n| self.node_rects.iter().find(|r| r.0 == n).map(|r| r.1)) {
                    Some(r) => egui::pos2(r.right() + 60.0, r.top()),
                    None => at,
                };
                let node = graph::new_node(&self.snarl, kind);
                let id = node.id.clone();
                let new = self.snarl.insert_node(pos, node);
                if let Some(from) = after {
                    if self.snarl.get_node(new).is_some_and(|g| g.ty().is_some_and(|t| !t.inputs.is_empty())) {
                        self.snarl.connect(egui_snarl::OutPinId { node: from, output: 0 }, egui_snarl::InPinId { node: new, input: 0 });
                    }
                }
                self.set_selection(vec![new]);
                self.focus_node(&id);
                self.changed(kind != NOTE);
            }
        }
    }

    /// The description of a model that has just been opened, on a plate at the top of the
    /// canvas, until it is closed.
    fn description_plate(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.show_description || self.description.trim().is_empty() {
            return;
        }
        let at = self.canvas_rect.left_top() + egui::vec2(14.0, 12.0);
        let mut close = false;
        egui::Area::new(egui::Id::new("model_description")).order(egui::Order::Middle).fixed_pos(at).constrain(false).show(ctx, |ui| {
            egui::Frame::new().fill(pal.panel).stroke(egui::Stroke::new(1.0, pal.line)).corner_radius(8).inner_margin(egui::Margin::symmetric(14, 10)).show(
                ui,
                |ui| {
                    ui.set_max_width((self.canvas_rect.width() - 160.0).clamp(240.0, 560.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&self.name).font(theme::strong(14.5)).color(pal.ink));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text("Close. Model → Description… shows it again").clicked() {
                                close = true;
                            }
                        });
                    });
                    ui.label(RichText::new(self.description.trim()).color(pal.ink));
                },
            );
        });
        if close {
            self.show_description = false;
        }
    }
}
