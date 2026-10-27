//! The menus, the buttons under them and the status line.

use super::*;

impl App {
    /// The top of the window: the menus with the name of the workspace on one row, and
    /// under it the buttons for what is done most often.
    pub(super) fn top_bar(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.horizontal(|ui| {
            // The names of the menus are plain text that lights up under the pointer, as in
            // the programs of Windows.
            flat_buttons(ui, pal);
            ui.spacing_mut().button_padding = egui::vec2(9.0, 4.0);
            menu_bar_keys(ui);
            let mut tops: Vec<egui::Response> = Vec::new();
            tops.push(ui.menu_button("File", |ui| {
                menu_style(ui, pal);
                if ui.add(egui::Button::new("New").shortcut_text("Ctrl+N")).clicked() {
                    self.request(Pending::New);
                    ui.close();
                }
                if ui.add(egui::Button::new("Open…").shortcut_text("Ctrl+O")).clicked() {
                    self.request(Pending::Open);
                    ui.close();
                }
                ui.menu_button("Open recent", |ui| {
                    menu_style(ui, pal);
                    if self.recent.list.is_empty() {
                        ui.add_enabled(false, egui::Button::new("No models yet"));
                    }
                    let mut open = None;
                    for (i, path) in self.recent.list.iter().enumerate() {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let there = path.exists();
                        let text = if there { format!("{}  {}", i + 1, name) } else { format!("{}  {} (missing)", i + 1, name) };
                        if ui.add_enabled(there, egui::Button::new(text)).on_hover_text(path.display().to_string()).clicked() {
                            open = Some(path.clone());
                        }
                    }
                    if !self.recent.list.is_empty() {
                        ui.separator();
                        if ui.button("Clear the list").clicked() {
                            self.recent.clear();
                            ui.close();
                        }
                    }
                    if let Some(p) = open {
                        self.request(Pending::OpenPath(p));
                        ui.close();
                    }
                });
                ui.separator();
                if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    self.save(false);
                    ui.close();
                }
                if ui.button("Save as…").clicked() {
                    self.save(true);
                    ui.close();
                }
                if ui.button("Save the model as a picture…").on_hover_text("PNG, JPEG or PDF: the canvas with all its nodes, connections, groups and notes").clicked() {
                    self.save_picture();
                    ui.close();
                }
                ui.separator();
                // Closing asks to save first, if something has changed.
                if ui.add(egui::Button::new("Exit").shortcut_text("Alt+F4")).clicked() {
                    self.request(Pending::Close);
                    ui.close();
                }
            }).response);
            tops.push(ui.menu_button("Edit", |ui| {
                menu_style(ui, pal);
                if ui.add_enabled(self.can_undo(), egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                    self.undo();
                    ui.close();
                }
                if ui.add_enabled(self.can_redo(), egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                let n = self.selected.len();
                let group = if n > 0 { "Group the selected nodes" } else { "Add a group box" };
                if ui.add(egui::Button::new(group).shortcut_text("Ctrl+G")).clicked() {
                    self.add_group(&self.selected.clone(), None);
                    ui.close();
                }
                if ui.button("Add a note").clicked() {
                    self.add_node(NOTE);
                    ui.close();
                }
                ui.separator();
                if ui.add_enabled(n > 0, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
                    self.copy_selected(ui.ctx(), true);
                    ui.close();
                }
                if ui
                    .add_enabled(n > 0, egui::Button::new("Copy").shortcut_text("Ctrl+C"))
                    .on_hover_text("The nodes can be pasted in this model or in another window of Ographis")
                    .clicked()
                {
                    self.copy_selected(ui.ctx(), false);
                    ui.close();
                }
                if ui.add(egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
                    self.paste_from_menu(None);
                    ui.close();
                }
                if ui.add_enabled(n > 0, egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).clicked() {
                    self.duplicate_nodes(&self.selected.clone());
                    ui.close();
                }
                let something = n > 0 || self.selected_group.is_some() || self.selected_wire.is_some();
                if ui.add_enabled(something, egui::Button::new("Delete").shortcut_text("Del")).clicked() {
                    self.delete_selected();
                    ui.close();
                }
                if ui.add(egui::Button::new("Select all").shortcut_text("Ctrl+A")).clicked() {
                    let everything: Vec<NodeId> = self.snarl.nodes_ids_data().map(|(id, _)| id).collect();
                    self.set_selection(everything);
                    ui.close();
                }
                ui.separator();
                if ui.add(egui::Button::new("Settings…").shortcut_text("Ctrl+,")).on_hover_text("Database connections, for all models").clicked() {
                    self.open_settings();
                    ui.close();
                }
            }).response);
            tops.push(ui.menu_button("View", |ui| {
                menu_style(ui, pal);
                if ui.add(egui::Button::new("Zoom to fit").shortcut_text("Home")).clicked() {
                    self.fit_requested = Some(SMALLEST_SCALE);
                    ui.close();
                }
                ui.separator();
                if ui.button("Show the attributes on all nodes").on_hover_text("The attributes are known after a run").clicked() {
                    self.expanded = self.snarl.nodes().filter(|n| !n.is_note()).map(|n| n.id.clone()).collect();
                    ui.close();
                }
                if ui.button("Hide the attributes on all nodes").clicked() {
                    self.expanded.clear();
                    ui.close();
                }
            }).response);
            tops.push(ui.menu_button("Model", |ui| {
                menu_style(ui, pal);
                let running = self.is_running();
                if ui.add_enabled(!running, egui::Button::new("Run").shortcut_text("F5")).clicked() {
                    self.run(ui.ctx());
                    ui.close();
                }
                if ui
                    .add_enabled(!running && self.use_cache, egui::Button::new("Run everything again").shortcut_text("Ctrl+F5"))
                    .on_hover_text("Reads and runs every node again, also those that have not changed")
                    .clicked()
                {
                    self.run_all_again(ui.ctx());
                    ui.close();
                }
                if ui.add_enabled(running && !self.stopping, egui::Button::new("Stop").shortcut_text("Shift+F5")).clicked() {
                    self.stop_run();
                    ui.close();
                }
                let pause = if self.paused { "Resume" } else { "Pause" };
                if ui.add_enabled(running && !self.stopping, egui::Button::new(pause).shortcut_text("F6")).clicked() {
                    self.toggle_pause();
                    ui.close();
                }
                ui.separator();
                let mut cache = self.use_cache;
                if ui
                    .checkbox(&mut cache, "Reuse what has not changed")
                    .on_hover_text("A run takes the results of the latest run for the nodes whose settings, data files and what comes before them have not changed, so that large files are not read again. Outputs always write. Run everything again reads all. Also lets a node run from where it is: right-click it and choose Run from here")
                    .clicked()
                {
                    self.use_cache = cache;
                    self.touch();
                }
                ui.separator();
                let label = if self.variables.is_empty() { "Variables…".to_string() } else { format!("Variables ({})…", self.variables.len()) };
                if ui.button(label).clicked() {
                    self.variables_open = true;
                    ui.close();
                }
                if ui
                    .add_enabled(self.path.is_some(), egui::Button::new("Run history…"))
                    .on_hover_text("When the model has been run, from here and from the command line, how long it took, how it went and what was written")
                    .on_disabled_hover_text("Save the model first: the history is kept beside it")
                    .clicked()
                {
                    self.history_open = true;
                    ui.close();
                }
                if ui
                    .button("Copy command line")
                    .on_hover_text("Copies the command that runs this model without the window, for a script or the Task Scheduler of Windows")
                    .clicked()
                {
                    self.copy_command_line(ui.ctx());
                    ui.close();
                }
            }).response);
            tops.push(ui.menu_button("Help", |ui| {
                menu_style(ui, pal);
                if ui.button("Tips and shortcuts").clicked() {
                    self.tips = true;
                    ui.close();
                }
                if ui.button("About Ographis").clicked() {
                    self.about = true;
                    ui.close();
                }
            }).response);
            let drawn: Vec<(egui::Id, Rect)> = tops.iter().map(|r| (r.id, r.rect)).collect();
            ui.ctx().data_mut(|d| d.insert_temp(menu_bar_id(), drawn));
            ui.add_space(6.0);
            // The field is as wide as the name, so that the mark for unsaved changes ends up next to it.
            let font = egui::FontId::proportional(15.0);
            let width = ui.painter().layout_no_wrap(self.name.clone(), font.clone(), pal.ink).size().x.clamp(120.0, 520.0) + 12.0;
            let name = ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(width).frame(egui::Frame::NONE).font(font));
            if name.changed() {
                self.touch();
            }
            if self.dirty {
                let (dot, resp) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, pal.muted);
                resp.on_hover_text("Changes that are not saved");
            }
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            use crate::icons::{button, Icon};
            ui.spacing_mut().item_spacing.x = 2.0;
            if button(ui, pal, Icon::New, "New model", "New model (Ctrl+N)", true).clicked() {
                self.request(Pending::New);
            }
            if button(ui, pal, Icon::Open, "Open", "Open a model (Ctrl+O)", true).clicked() {
                self.request(Pending::Open);
            }
            if button(ui, pal, Icon::Save, "Save", "Save the model (Ctrl+S)", true).clicked() {
                self.save(false);
            }
            ui.add_space(4.0);
            ui.separator();
            let running = self.is_running();
            let hint = if running { "Running…" } else { "Run the model (F5)" };
            if button(ui, pal, Icon::Run, "Run", hint, !running).clicked() {
                self.run(ui.ctx());
            }
            let can_stop = running && !self.stopping;
            if button(ui, pal, Icon::Stop, "Stop", "Stop the run (Shift+F5)", can_stop).clicked() {
                self.stop_run();
            }
            let (icon, name, hint) = if self.paused {
                (Icon::Run, "Resume", "Go on with the run (F6)")
            } else {
                (Icon::Pause, "Pause", "Pause the run (F6). It waits until you resume or stop it")
            };
            if button(ui, pal, icon, name, hint, can_stop).clicked() {
                self.toggle_pause();
            }
            if running && !self.paused {
                ui.spinner();
            }
            ui.separator();
            if button(ui, pal, Icon::Undo, "Undo", "Undo: take back the latest change (Ctrl+Z)", self.can_undo()).clicked() {
                self.undo();
            }
            if button(ui, pal, Icon::Redo, "Redo", "Redo: do again what was undone (Ctrl+Y)", self.can_redo()).clicked() {
                self.redo();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (text, color) = self.status(pal);
                ui.label(RichText::new(text).font(theme::mono(12.0)).color(color));
            });
        });
    }

    pub(super) fn status(&self, pal: &Palette) -> (String, egui::Color32) {
        if let Some(msg) = self.message() {
            return (msg.to_string(), pal.accent);
        }
        if self.is_running() && self.stopping {
            return ("Stopping…".to_string(), pal.warn);
        }
        if self.is_running() && self.paused {
            let name = self.running_node.as_deref().and_then(|id| self.snarl.nodes().find(|n| n.id == id)).map(|n| n.name.clone());
            return (name.map_or("Paused".to_string(), |n| format!("Paused · {}", n)), pal.warn);
        }
        if self.is_running() {
            let name = self.running_node.as_deref().and_then(|id| self.snarl.nodes().find(|n| n.id == id)).map(|n| n.name.as_str());
            let (done, total) = self.progress;
            let text = match name {
                Some(name) if self.running_step.is_some() => format!("Running {} · {}", name, self.running_step.as_deref().unwrap_or_default()),
                Some(name) => format!("Running {} · {} of {}", name, (done + 1).min(total), total),
                None => format!("Running · {} of {} done", done, total),
            };
            return (text, pal.xform);
        }
        match &self.result {
            None => ("Not run".into(), pal.muted),
            Some(_) if self.stale => ("Changed since the run".into(), pal.warn),
            Some(r) if r.errors > 0 => (format!("{} {} in the run", r.errors, if r.errors == 1 { "error" } else { "errors" }), pal.err),
            Some(r) => {
                let n = r.nodes.len();
                let warn = if r.warnings > 0 { format!(" · {} {}", r.warnings, if r.warnings == 1 { "warning" } else { "warnings" }) } else { String::new() };
                (
                    format!("Ran · {} {} · {}{}", n, if n == 1 { "node" } else { "nodes" }, og_engine::model::fmt_duration(r.ms), warn),
                    if r.warnings > 0 { pal.warn } else { pal.ok },
                )
            }
        }
    }
}

/// Buttons without a frame, that light up under the pointer: the names of the menus and the
/// items in them.
pub(super) fn flat_buttons(ui: &mut Ui, pal: &Palette) {
    let w = &mut ui.visuals_mut().widgets;
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.bg_stroke = egui::Stroke::NONE;
        state.corner_radius = egui::CornerRadius::same(4);
    }
    w.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    w.inactive.bg_fill = egui::Color32::TRANSPARENT;
    w.hovered.weak_bg_fill = pal.bg;
    w.active.weak_bg_fill = pal.line;
    w.open.weak_bg_fill = pal.bg;
}
/// Makes the menus work as in Windows: while one is open, pointing at another opens that
/// one instead. Alt and the first letter, or F10, open a menu from the keyboard, the arrows
/// up and down move through it, left and right go to the menu beside it, Enter chooses and
/// Esc closes.
pub(super) fn menu_bar_keys(ui: &Ui) {
    use egui::{Key, Modifiers, Popup};
    let ctx = ui.ctx();
    // The menus as they were drawn in the frame before: the id of each button and where it is.
    // A menu is opened before the buttons are drawn, so that it shows in this frame.
    let tops: Vec<(egui::Id, Rect)> = ctx.data(|d| d.get_temp(menu_bar_id())).unwrap_or_default();
    if tops.is_empty() {
        return;
    }
    let popup = |i: usize| tops[i].0.with("popup");
    let open = (0..tops.len()).find(|i| Popup::is_id_open(ctx, popup(*i)));
    let show = |i: usize| {
        Popup::open_id(ctx, popup(i));
        // The name of the menu gets the focus, and the arrow that brought it there does not
        // move the focus on from it.
        ctx.memory_mut(|m| {
            m.request_focus(tops[i].0);
            m.move_focus(egui::FocusDirection::None);
        });
    };
    // Alt and the first letter of a menu.
    let letters = [Key::F, Key::E, Key::V, Key::M, Key::H];
    for (i, k) in letters.iter().enumerate().take(tops.len()) {
        if ctx.input_mut(|inp| inp.consume_key(Modifiers::ALT, *k)) {
            show(i);
            return;
        }
    }
    if ctx.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::F10)) {
        match open {
            Some(_) => Popup::close_all(ctx),
            None => show(0),
        }
        return;
    }
    let Some(o) = open else { return };
    // The menu itself can take the focus on its way to the first item. It is passed over.
    let step: Option<bool> = ctx.data(|d| d.get_temp(menu_bar_id().with("step")));
    if let Some(down) = step {
        ctx.data_mut(|d| d.remove::<bool>(menu_bar_id().with("step")));
        if ctx.memory(|m| m.focused()) == Some(popup(o).with("move")) {
            ctx.memory_mut(|m| m.move_focus(if down { egui::FocusDirection::Next } else { egui::FocusDirection::Previous }));
        }
    }
    // Pointing at another menu while one is open. Only when the pointer moves, so that a
    // pointer that rests on a menu does not take over from the keyboard.
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    if let Some(pos) = ctx.pointer_hover_pos().filter(|_| moved) {
        if let Some(h) = tops.iter().position(|t| t.1.contains(pos)) {
            if h != o {
                Popup::open_id(ctx, popup(h));
                return;
            }
        }
    }
    // Left and right go to the menu beside.
    let (left, right, up, down) =
        ctx.input(|inp| (inp.key_pressed(Key::ArrowLeft), inp.key_pressed(Key::ArrowRight), inp.key_pressed(Key::ArrowUp), inp.key_pressed(Key::ArrowDown)));
    if left || right {
        let n = tops.len();
        show(if right { (o + 1) % n } else { (o + n - 1) % n });
        return;
    }
    // Up and down go through the items of the open menu. They come right after the name of
    // the menu in the order of the window, so the next and the one before are taken, rather
    // than what lies below on the screen.
    if down || up {
        if ctx.memory(|m| m.focused()).is_none() {
            ctx.memory_mut(|m| m.request_focus(tops[o].0));
        }
        ctx.memory_mut(|m| m.move_focus(if down { egui::FocusDirection::Next } else { egui::FocusDirection::Previous }));
        ctx.data_mut(|d| d.insert_temp(menu_bar_id().with("step"), down));
    }
}
pub(super) fn menu_bar_id() -> egui::Id {
    egui::Id::new("ographis-menu-bar")
}
/// A menu the way they look in Windows: a list of plain items as wide as the menu, with the
/// shortcuts to the right in grey and a thin line between groups.
pub(super) fn menu_style(ui: &mut Ui, pal: &Palette) {
    flat_buttons(ui, pal);
    ui.set_min_width(230.0);
    let sp = ui.spacing_mut();
    sp.button_padding = egui::vec2(10.0, 5.0);
    sp.item_spacing.y = 1.0;
    ui.visuals_mut().widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, pal.line);
}
