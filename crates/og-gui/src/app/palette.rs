//! The list of tools to add, with its search.

use super::*;

impl App {
    pub(super) fn palette(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.label(RichText::new("TOOLS").font(theme::strong(11.5)).color(pal.muted));
        search_field(ui, &mut self.filter, "Search", f32::INFINITY);
        let needle = self.filter.trim().to_lowercase();
        let mut add = None;
        let mut annotate = None;
        let mut open = std::mem::take(&mut self.open_sections);
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // The list is long, so the rows are kept low.
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.spacing_mut().interact_size.y = 22.0;
            ui.spacing_mut().button_padding.y = 2.0;
            let row = |ui: &mut Ui, name: &str, desc: &str, color: egui::Color32| {
                let r = ui
                    .horizontal(|ui| {
                        let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), Sense::hover());
                        ui.painter().rect_filled(Rect::from_center_size(dot.center(), egui::vec2(9.0, 9.0)), 2.0, color);
                        ui.add(egui::Button::new(RichText::new(name).font(theme::strong(14.0))).frame(false))
                    })
                    .inner;
                r.on_hover_text(desc).clicked()
            };
            let mut item = |ui: &mut Ui, t: &'static NodeType| {
                if row(ui, t.title, t.desc, pal.group(t.group).0) {
                    add = Some(t.name);
                }
            };
            // The two things that can be added and are not part of the run.
            let annotations = [
                ("Group box", "A named box that holds nodes together. With nodes selected it is put around them. Move the box and the nodes follow."),
                ("Note", "A free-standing note on the canvas, for what the next person needs to know."),
            ];
            let all = graph::offered(&[Group::Reader, Group::Transformer, Group::Writer]);
            if !needle.is_empty() {
                // A search shows the matches as one list, names first and then descriptions.
                // The names the tools had before are found as well.
                let named = |t: &NodeType| t.title.to_lowercase().contains(&needle) || t.name.to_lowercase().contains(&needle);
                let by_name: Vec<&NodeType> = all.iter().copied().filter(|t| named(t)).collect();
                let by_desc: Vec<&NodeType> = all.iter().copied().filter(|t| !named(t) && t.desc.to_lowercase().contains(&needle)).collect();
                let notes: Vec<&(&str, &str)> = annotations.iter().filter(|a| a.0.to_lowercase().contains(&needle)).collect();
                ui.add_space(8.0);
                if by_name.is_empty() && by_desc.is_empty() && notes.is_empty() {
                    ui.label(RichText::new("Nothing matches.").color(pal.muted));
                }
                for t in by_name.into_iter().chain(by_desc) {
                    item(ui, t);
                }
                for (name, desc) in notes {
                    if row(ui, name, desc, pal.note_line) {
                        annotate = Some(*name);
                    }
                }
                return;
            }
            // A heading with an arrow. A click folds the list under it out or in. Returns
            // whether the list is to be shown.
            let mut heading = |ui: &mut Ui, text: &str| -> bool {
                ui.add_space(6.0);
                let is_open = open.contains(text);
                let clicked = ui
                    .horizontal(|ui| {
                        let (arrow, on_arrow) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), Sense::click());
                        let c = arrow.center();
                        // The arrow is drawn, so that it looks the same whatever the font holds.
                        let points = if is_open {
                            vec![c + egui::vec2(-4.0, -2.5), c + egui::vec2(4.0, -2.5), c + egui::vec2(0.0, 3.0)]
                        } else {
                            vec![c + egui::vec2(-2.5, -4.0), c + egui::vec2(3.0, 0.0), c + egui::vec2(-2.5, 4.0)]
                        };
                        ui.painter().add(egui::Shape::convex_polygon(points, pal.muted, egui::Stroke::NONE));
                        let name = ui.add(egui::Button::new(RichText::new(text).font(theme::mono(11.0)).color(pal.muted)).frame(false));
                        let hint = if is_open { "Hide the list" } else { "Show the list" };
                        on_arrow.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                            || name.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                    })
                    .inner;
                if clicked && !open.remove(text) {
                    open.insert(text.to_string());
                }
                is_open != clicked
            };
            // Readers and writers come first, since every workspace needs them. Within a
            // list the names are in alphabetical order.
            // Tools that read or write in the middle of a model, such as Read files named in
            // an attribute, are there too.
            if heading(ui, "Input and output") {
                for t in all.iter().filter(|t| t.group != Group::Transformer || t.category == "Input and output") {
                    item(ui, t);
                }
            }
            let mut categories: Vec<&str> =
                all.iter().filter(|t| t.group == Group::Transformer && t.category != "Input and output").map(|t| t.category).collect();
            categories.sort();
            categories.dedup();
            for category in categories {
                if heading(ui, category) {
                    for t in all.iter().filter(|t| t.group == Group::Transformer && t.category == category) {
                        item(ui, t);
                    }
                }
            }
            if heading(ui, "Annotations") {
                for (name, desc) in annotations {
                    if row(ui, name, desc, pal.note_line) {
                        annotate = Some(name);
                    }
                }
            }
        });
        self.open_sections = open;
        if let Some(kind) = add {
            self.add_node(kind);
        }
        match annotate {
            Some("Note") => self.add_node(NOTE),
            Some(_) => {
                self.add_group(&self.selected.clone(), None);
            }
            None => {}
        }
    }
}
