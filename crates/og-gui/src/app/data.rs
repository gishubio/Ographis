//! Under the canvas: the log, and the table, map and statistics of the data in a port.

use super::*;

impl App {
    pub(super) fn inspector(&mut self, ui: &mut Ui, pal: &Palette) {
        let (errors, warnings) = self.result.as_ref().map_or((0, 0), |r| (r.errors, r.warnings));
        egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Data, RichText::new("Data").font(theme::strong(13.5)));
                let log = if errors > 0 {
                    format!("Log · {} {}", errors, if errors == 1 { "error" } else { "errors" })
                } else if warnings > 0 {
                    format!("Log · {}", warnings)
                } else {
                    "Log".to_string()
                };
                let color = if errors > 0 {
                    pal.err
                } else if warnings > 0 {
                    pal.warn
                } else {
                    pal.ink
                };
                ui.selectable_value(&mut self.tab, Tab::Log, RichText::new(log).font(theme::strong(13.5)).color(color));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.tab == Tab::Log {
                        if let Some(r) = &self.result {
                            if ui.button("Copy log").clicked() {
                                let text: Vec<String> = r.log.iter().map(|l| format!("{} {}", level_tag(l.level), l.msg)).collect();
                                ui.ctx().copy_text(text.join("\n"));
                            }
                        }
                    }
                });
            });
        });
        match self.tab {
            Tab::Log => self.log_tab(ui, pal),
            Tab::Data => self.data_tab(ui, pal),
        }
    }

    pub(super) fn log_tab(&mut self, ui: &mut Ui, pal: &Palette) {
        let mut go = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                let Some(r) = &self.result else {
                    ui.label(RichText::new("No run yet. Press Run or F5.").color(pal.muted));
                    return;
                };
                if self.live {
                    ui.label(RichText::new("Running. The log is filled in when the run has finished.").color(pal.muted));
                }
                for l in &r.log {
                    let color = match l.level {
                        Level::Info => pal.muted,
                        Level::Warn => pal.warn,
                        Level::Error => pal.err,
                    };
                    ui.horizontal_top(|ui| {
                        ui.add(egui::Label::new(RichText::new(level_tag(l.level)).font(theme::mono(12.0)).color(color)).selectable(true));
                        let text = RichText::new(&l.msg).font(theme::mono(12.0)).color(if l.level == Level::Info { pal.ink } else { color });
                        // The text can be marked with the mouse and copied with Ctrl+C, across
                        // lines too; a plain click on a line about a node still goes to it.
                        let label = egui::Label::new(text).wrap().selectable(true);
                        let resp = ui.add(if l.node.is_some() { label.sense(Sense::click()) } else { label });
                        if resp.clicked() {
                            go = l.node.clone();
                        }
                    });
                }
            });
        });
        if let Some(id) = go {
            self.focus_node(&id);
        }
    }

    /// The order of the rows when the table is sorted by a column. Numbers sort as
    /// numbers, text without regard to case, and empty values come last either way.
    pub(super) fn row_order(&mut self, t: &Arc<Table>) -> Option<Arc<Vec<usize>>> {
        let by = self.sort.filter(|s| s.0 < t.fields.len())?;
        let table = Arc::as_ptr(t) as usize;
        if let Some(s) = self.sorted.as_ref().filter(|s| s.table == table && s.by == by) {
            return Some(s.order.clone());
        }
        let (column, descending) = by;
        let mut order: Vec<usize> = (0..t.len()).collect();
        order.sort_by(|a, b| {
            let (x, y) = (&t.features[*a].attrs[column], &t.features[*b].attrs[column]);
            match (x.is_null(), y.is_null()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) if descending => og_engine::nodes::sort_cmp(y, x),
                (false, false) => og_engine::nodes::sort_cmp(x, y),
            }
        });
        let order = Arc::new(order);
        self.sorted = Some(Sorted { table, by, order: order.clone() });
        Some(order)
    }

    pub(super) fn data_tab(&mut self, ui: &mut Ui, pal: &Palette) {
        let viewed = self.viewed();
        // Another node or port is another table, which starts in the order of its stream.
        let shown = (self.view.node.clone(), viewed.as_ref().map(|(ports, chosen)| ports[*chosen].0.clone()));
        if shown != self.sort_of {
            self.sort_of = shown;
            self.sort = None;
        }
        let node_name = self.view.node.as_deref().and_then(|id| self.snarl.nodes().find(|n| n.id == id)).map(|n| n.name.clone());
        let dynamic = self.view.node.as_deref().and_then(|id| self.snarl.nodes().find(|n| n.id == id)).and_then(|n| n.ty()).is_some_and(|t| t.ports.is_some());
        let status = self.view.node.as_deref().and_then(|id| self.result.as_ref().and_then(|r| r.nodes.get(id))).map(|r| (r.status, r.error.clone()));
        let table = viewed.as_ref().map(|(ports, chosen)| ports[*chosen].1.clone());
        // A View shows all its streams together in the map and in 3D; the table is the chosen one.
        let is_view = self.view.node.as_deref().and_then(|id| self.snarl.nodes().find(|n| n.id == id)).is_some_and(|n| n.kind == "Viewer");
        // The streams of a View that are hidden from the map, as the View keeps them.
        let hidden: Vec<String> = match self.view.node.as_deref().and_then(|id| self.snarl.nodes().find(|n| n.id == id)) {
            Some(n) if is_view => og_gui_list(n.params.get("hidden")),
            _ => Vec::new(),
        };
        let mut hide_toggled: Option<String> = None;
        let together = match &viewed {
            Some((ports, chosen)) if is_view && ports.len() > 1 => {
                let shown: Vec<(String, Arc<Table>)> = ports.iter().filter(|p| !hidden.contains(&p.0)).cloned().collect();
                let chosen_shown = shown.iter().position(|p| p.0 == ports[*chosen].0);
                let keys: Vec<usize> = std::iter::once(chosen_shown.map_or(usize::MAX, |c| c))
                    .chain(shown.iter().map(|p| Arc::as_ptr(&p.1) as usize))
                    .chain(std::iter::once(shown.len()))
                    .collect();
                if shown.is_empty() {
                    None
                } else {
                    if self.together.as_ref().is_none_or(|t| t.keys != keys) {
                        let mut t = together(&shown, chosen_shown.unwrap_or(0), keys, Some(&*self.backend as &dyn Backend));
                        // A hidden chosen stream has nothing on the map to pick.
                        if chosen_shown.is_none() {
                            t.len = 0;
                        }
                        self.together = Some(t);
                    }
                    self.together.clone()
                }
            }
            _ => None,
        };
        let all_hidden = is_view && viewed.as_ref().is_some_and(|(p, _)| p.len() > 1 && p.iter().all(|x| hidden.contains(&x.0)));

        egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                match (&node_name, &viewed) {
                    (Some(name), Some((ports, chosen))) => {
                        ui.label(RichText::new(name).font(theme::strong(14.5)));
                        for (i, (port, t)) in ports.iter().enumerate() {
                            // In a View with several streams, each can be shown on the map or not.
                            if is_view && ports.len() > 1 {
                                let mut on = !hidden.contains(port);
                                if ui.checkbox(&mut on, "").on_hover_text("Show on the map and in 3D").changed() {
                                    hide_toggled = Some(port.clone());
                                }
                            }
                            // Ports named after values are quoted, so that "1" 6 is not read as 16.
                            let label = if dynamic && i + 1 < ports.len() {
                                format!("\"{}\" {}", port, fmt_count(t.len() as u64))
                            } else {
                                format!("{} {}", port, fmt_count(t.len() as u64))
                            };
                            let text = RichText::new(label).font(theme::mono(11.5));
                            if ui.selectable_label(i == *chosen, text).clicked() {
                                self.view.port = Some(port.clone());
                                self.view.feature = None;
                                self.view.more.clear();
                            }
                        }
                        let t = &ports[*chosen].1;
                        let kinds = geometry_kinds(t);
                        let system = if t.crs.is_none() && t.image.is_some() {
                            "no coordinates".to_string()
                        } else if t.crs.is_none() && t.features.iter().any(|f| f.geom.is_some()) {
                            "local coordinates, no coordinate system".to_string()
                        } else {
                            match t.crs.filter(|c| !og_engine::model::KNOWN_CRS.iter().any(|k| k.0 == *c)).and_then(|c| self.crs_list.iter().find(|e| e.code == c)) {
                                Some(e) => format!("{} (EPSG:{})", e.name, e.code),
                                None => crs_label(t.crs),
                            }
                        };
                        let meta = if kinds.is_empty() { system } else { format!("{} · {}", system, kinds) };
                        ui.label(RichText::new(meta).font(theme::mono(12.0)).color(pal.muted));
                        ui.add_space(6.0);
                        if ui
                            .selectable_label(self.show_stats, "Statistics")
                            .on_hover_text("For each attribute: how many have a value, how many different values, the smallest and the largest, and for numbers the mean and the sum")
                            .clicked()
                        {
                            self.show_stats = !self.show_stats;
                        }
                    }
                    (Some(name), None) => {
                        ui.label(RichText::new(name).font(theme::strong(14.5)));
                        let msg = match &status {
                            Some((Status::Error, Some(e))) | Some((Status::Skipped, Some(e))) | Some((Status::Disabled, Some(e))) => e.clone(),
                            _ if self.result.is_none() => "Run the model to see data.".to_string(),
                            _ => "This node has not run yet.".to_string(),
                        };
                        let color = if matches!(status, Some((Status::Error, _))) { pal.err } else { pal.muted };
                        ui.label(RichText::new(msg).color(color));
                    }
                    _ => {
                        let msg = if self.result.is_some() {
                            "Click a node or an output port to see its data."
                        } else {
                            "Run the model, then click a node to see its data."
                        };
                        ui.label(RichText::new(msg).color(pal.muted));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.map.controls(ui));
            });
        });
        ui.separator();

        let backend = self.backend.clone();
        let mut selected = self.view.feature;
        let mut more = std::mem::take(&mut self.view.more);
        let before_map = selected;
        // The tick boxes of a View: the hidden streams are kept in the View, with the model.
        if let Some(port) = hide_toggled {
            if let Some(id) = self.view.node.clone() {
                if let Some(n) = self.snarl.nodes_mut().find(|n| n.id == id) {
                    let mut list = og_gui_list(n.params.get("hidden"));
                    match list.iter().position(|p| *p == port) {
                        Some(i) => {
                            list.remove(i);
                        }
                        None => list.push(port),
                    }
                    n.params.insert("hidden".into(), serde_json::json!(list));
                    self.dirty = true;
                }
            }
        }
        let map_table = if all_hidden { None } else { together.as_ref().map(|t| t.table.clone()).or_else(|| table.clone()) };
        let skip = together.as_ref().map(|t| t.models);
        egui::Panel::right("map").resizable(true).default_size(460.0).min_size(200.0).frame(egui::Frame::NONE).show(ui, |ui| {
            let backend: Arc<dyn Backend + Send + Sync> = backend.clone();
            // A picture, such as a rendered one, is shown as it is, also with 3D ticked.
            let picture = map_table.as_ref().is_some_and(|t| t.image.is_some() && t.grid.is_none() && t.mesh.is_none());
            // In a View's streams together, the chosen one's features are a run among the others.
            let (offset, len) = together.as_ref().map_or((0, usize::MAX), |t| (t.offset, t.len));
            let mut picked = selected.filter(|i| *i < len).map(|i| i + offset);
            self.map.also = more.iter().filter(|i| **i < len).map(|i| i + offset).collect();
            if self.map.three_d && !picture {
                // A click on a feature in 3D picks it, as on the map.
                if let Some(p) = self.map.view3d.show_skipping(ui, map_table.as_ref(), picked, skip) {
                    selected = p.and_then(|i| i.checked_sub(offset)).filter(|i| *i < len);
                }
            } else {
                let was = picked;
                self.map.shapes_over_picture = together.is_some();
                self.map.show(ui, map_table.as_ref(), &mut picked, &backend);
                if let Some((hit, _)) = self.map.multi_click.take() {
                    // Ctrl or Shift and a click on a feature adds it to those picked, or
                    // takes it away.
                    if let Some(i) = hit.and_then(|i| i.checked_sub(offset)).filter(|i| *i < len) {
                        pick_row(&mut selected, &mut more, i, egui::Modifiers::COMMAND, None);
                    }
                } else if picked != was {
                    selected = picked.and_then(|i| i.checked_sub(offset)).filter(|i| *i < len);
                    more.clear();
                }
            }
        });
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 0, top: 0, bottom: 0 })).show(
            ui,
            |ui| match &table {
                Some(t) if self.show_stats && (!t.is_empty() || !t.fields.is_empty()) => {
                    let key = Arc::as_ptr(t) as usize;
                    if self.stats_cache.as_ref().is_none_or(|c| c.0 != key) {
                        self.stats_cache = Some((key, crate::stats::column_stats(t)));
                    }
                    if let Some((_, stats)) = &self.stats_cache {
                        stats_ui(ui, stats, pal);
                    }
                }
                Some(t) if !t.is_empty() || !t.fields.is_empty() => {
                    // A feature that was just picked on the map is scrolled into view in the table.
                    let mut scroll_to = if selected != before_map { selected } else { None };
                    let order = self.row_order(t);
                    // Page Up and Page Down move a page of rows at a time, when the pointer is over
                    // the table or a row is selected, and no text is being written.
                    let over = ui.rect_contains_pointer(ui.max_rect());
                    if (over || selected.is_some()) && !ui.ctx().egui_wants_keyboard_input() && !t.is_empty() {
                        let (down, up) = ui.input_mut(|i| {
                            (i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown), i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp))
                        });
                        if down || up {
                            let page = ((ui.available_height() - 30.0) / 22.0).floor().max(1.0) as usize;
                            let place = |i: usize| order.as_ref().map_or(Some(i), |o| o.iter().position(|x| *x == i));
                            let row_at = |p: usize| order.as_ref().map_or(p, |o| o[p]);
                            let last = t.len() - 1;
                            let next = match selected.and_then(place) {
                                Some(p) if down => (p + page).min(last),
                                Some(p) => p.saturating_sub(page),
                                None if down => page.min(last),
                                None => 0,
                            };
                            selected = Some(row_at(next));
                            more.clear();
                            scroll_to = selected;
                        }
                    }
                    if let Some(column) = table_ui(ui, t, &mut selected, &mut more, scroll_to, self.sort, order.as_deref().map(|o| o.as_slice()), pal) {
                        // A click on a column sorts by it: first upwards, then downwards,
                        // and a third click gives the order of the stream back.
                        self.sort = match self.sort {
                            Some((c, false)) if c == column => Some((c, true)),
                            Some((c, true)) if c == column => None,
                            _ => Some((column, false)),
                        };
                    }
                }
                Some(_) => {
                    ui.add_space(10.0);
                    ui.label(RichText::new("No features came out here.").color(pal.muted));
                }
                None => {}
            },
        );
        if selected.is_none() {
            more.clear();
        }
        self.view.feature = selected;
        self.view.more = more;
    }
}

/// A click on row `i`. Alone it picks the row, or lets go of it when it was the one picked;
/// with Ctrl (Cmd on a Mac) the row is added to those picked, or taken away; with Shift every
/// row from the one picked first to it, in the order the table shows them, is picked.
pub(crate) fn pick_row(selected: &mut Option<usize>, more: &mut Vec<usize>, i: usize, keys: egui::Modifiers, order: Option<&[usize]>) {
    match *selected {
        Some(first) if keys.shift => {
            let place = |r: usize| order.map_or(Some(r), |o| o.iter().position(|x| *x == r));
            if let (Some(a), Some(b)) = (place(first), place(i)) {
                let (lo, hi) = (a.min(b), a.max(b));
                *more = (lo..=hi).map(|p| order.map_or(p, |o| o[p])).filter(|r| *r != first).collect();
            }
        }
        Some(first) if keys.command => {
            if first == i {
                *selected = more.pop();
            } else if let Some(at) = more.iter().position(|r| *r == i) {
                more.remove(at);
            } else {
                more.push(first);
                *selected = Some(i);
            }
        }
        Some(first) if first == i && more.is_empty() => *selected = None,
        _ => {
            more.clear();
            *selected = Some(i);
        }
    }
}

/// The order of the rows of a table when it is sorted by a column. Kept until the table
/// or the column changes, since sorting a large table in every frame would be slow.
pub(super) struct Sorted {
    table: usize,
    by: (usize, bool),
    order: Arc<Vec<usize>>,
}
pub(super) fn level_tag(level: Level) -> &'static str {
    match level {
        Level::Info => "INFO ",
        Level::Warn => "WARN ",
        Level::Error => "ERROR",
    }
}
/// What the one feature of a stream with a picture or a grid stands for, as it is shown in
/// the table, or None for an ordinary stream.
pub(super) fn carried(t: &Table) -> Option<String> {
    if let Some(img) = &t.image {
        return Some(format!("picture {} × {}", img.width, img.height));
    }
    t.grid.as_ref().map(|g| format!("grid {} × {}", g.width, g.height))
}
pub(super) fn geometry_kinds(t: &Table) -> String {
    if t.image.is_some() {
        return "Picture".into();
    }
    if t.grid.is_some() {
        return "Grid of values".into();
    }
    let mut kinds: Vec<&str> = Vec::new();
    for f in t.features.iter().take(2000) {
        if let Some(g) = &f.geom {
            let k = og_engine::geom::type_name(g);
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
    }
    kinds.join(", ")
}
/// The text of a value in a cell of the table: on one line, and only the beginning of a long
/// one, such as the answer of a web service, which can be megabytes. Laid out whole, such a
/// text would be more than the graphics card takes. The whole is copied from the menu.
pub(super) fn cell_text(v: &Value) -> String {
    const MOST: usize = 300;
    match v {
        Value::Null => "–".to_string(),
        Value::Text(t) => {
            let mut s: String = t.chars().take(MOST).map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
            if t.chars().nth(MOST).is_some() {
                s.push_str(&format!(" … ({} characters)", og_engine::model::fmt_count(t.chars().count() as u64)));
            }
            s
        }
        other => other.to_text(),
    }
}
/// The statistics of a stream, one row per attribute.
pub(super) fn stats_ui(ui: &mut Ui, stats: &[crate::stats::ColumnStats], pal: &Palette) {
    let font = theme::mono(12.0);
    let heads = ["attribute", "type", "values", "empty", "unique", "min", "max", "mean", "sum"];
    ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .min_scrolled_height(0.0)
            .column(Column::auto().at_least(90.0).clip(true))
            .column(Column::auto().at_least(50.0))
            .columns(Column::auto().at_least(56.0), 3)
            .columns(Column::auto().at_least(70.0).clip(true), 2)
            .columns(Column::auto().at_least(70.0), 2)
            .header(24.0, |mut h| {
                for head in heads {
                    h.col(|ui| {
                        ui.label(RichText::new(head).font(font.clone()).color(pal.muted));
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, stats.len(), |mut row| {
                    let s = &stats[row.index()];
                    let cells = [
                        s.name.clone(),
                        s.kind.clone(),
                        fmt_count(s.values as u64),
                        fmt_count(s.empty as u64),
                        fmt_count(s.unique as u64),
                        s.min.clone().unwrap_or_else(|| "–".into()),
                        s.max.clone().unwrap_or_else(|| "–".into()),
                        s.mean.map(crate::stats::fmt_number).unwrap_or_else(|| "–".into()),
                        s.sum.map(crate::stats::fmt_number).unwrap_or_else(|| "–".into()),
                    ];
                    for (k, text) in cells.into_iter().enumerate() {
                        row.col(|ui| {
                            let color = if text == "–" || k == 1 { pal.muted } else { pal.ink };
                            ui.label(RichText::new(text).font(font.clone()).color(color));
                        });
                    }
                });
            });
    });
}
/// The table only draws the rows that are visible, so it copes with large streams.
/// `order` is the order to show the rows in when the table is sorted. The number of a row
/// is always its place in the stream. Returns the column whose heading was clicked.
#[allow(clippy::too_many_arguments)]
pub(super) fn table_ui(
    ui: &mut Ui,
    t: &Table,
    selected: &mut Option<usize>,
    more: &mut Vec<usize>,
    scroll_to: Option<usize>,
    sort: Option<(usize, bool)>,
    order: Option<&[usize]>,
    pal: &Palette,
) -> Option<usize> {
    let has_geom = t.crs.is_some() || carried(t).is_some();
    let what = carried(t);
    let font = theme::mono(12.0);
    let mut heading = None;
    ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .min_scrolled_height(0.0)
            .column(Column::auto().at_least(44.0));
        if has_geom {
            builder = builder.column(Column::auto().at_least(86.0));
        }
        builder = builder.columns(Column::auto().at_least(70.0).clip(true), t.fields.len());
        // Where the feature is in the table, which is not its number when the table is sorted.
        let place = |i: usize| order.map_or(Some(i), |o| o.iter().position(|x| *x == i));
        if let Some(row) = scroll_to.filter(|i| *i < t.len()).and_then(place) {
            builder = builder.scroll_to_row(row, Some(Align::Center));
        }
        let mut clicked = None;
        let marked: std::collections::HashSet<usize> = more.iter().copied().collect();
        builder
            .header(24.0, |mut h| {
                h.col(|ui| {
                    ui.label(RichText::new("#").font(font.clone()).color(pal.muted));
                });
                if has_geom {
                    h.col(|ui| {
                        ui.label(RichText::new("geometry").font(font.clone()).color(pal.muted));
                    });
                }
                for (c, f) in t.fields.iter().enumerate() {
                    h.col(|ui| {
                        let (text, color) = match sort {
                            // The arrow comes first, so that it shows when the column is narrow.
                            Some((by, descending)) if by == c => (format!("{} {}", if descending { "▼" } else { "▲" }, f.name), pal.ink),
                            _ => (f.name.clone(), pal.muted),
                        };
                        let label = egui::Label::new(RichText::new(text).font(font.clone()).color(color)).sense(Sense::click());
                        if ui.add(label).on_hover_text(format!("{} · click to sort", f.ty.label())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            heading = Some(c);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, t.len(), |mut row| {
                    let i = order.map_or(row.index(), |o| o[row.index()]);
                    let f = &t.features[i];
                    row.set_selected(*selected == Some(i) || marked.contains(&i));
                    row.col(|ui| {
                        ui.label(RichText::new((i + 1).to_string()).font(font.clone()).color(pal.muted));
                    });
                    if has_geom {
                        row.col(|ui| {
                            let kind = what.as_deref().unwrap_or_else(|| f.geom.as_ref().map_or("–", og_engine::geom::type_name));
                            // A geometry with heights says so, as GIS programs do.
                            let kind = if f.heights().is_some() { format!("{} Z", kind) } else { kind.to_string() };
                            ui.label(RichText::new(kind).font(font.clone()).color(pal.muted));
                        });
                    }
                    for (c, v) in f.attrs.iter().enumerate() {
                        row.col(|ui| {
                            let color = if v.is_null() { pal.muted } else { pal.ink };
                            let r = ui.add(egui::Label::new(RichText::new(cell_text(v)).font(font.clone()).color(color)).sense(Sense::click()));
                            // A click on the value picks the row, as a click beside it does.
                            if r.clicked() {
                                clicked = Some(i);
                            }
                            // The secondary button copies the value, the row or the column.
                            r.context_menu(|ui| {
                                let plain = |v: &og_engine::Value| if v.is_null() { String::new() } else { v.to_text() };
                                if ui.button("Copy value").clicked() {
                                    ui.ctx().copy_text(plain(v));
                                    ui.close();
                                }
                                if ui.button("Copy row").on_hover_text("The names and the values, separated by tabs, to paste in a spreadsheet").clicked() {
                                    let names: Vec<&str> = t.fields.iter().map(|f| f.name.as_str()).collect();
                                    let values: Vec<String> = f.attrs.iter().map(plain).collect();
                                    ui.ctx().copy_text(format!("{}\n{}", names.join("\t"), values.join("\t")));
                                    ui.close();
                                }
                                if !marked.is_empty()
                                    && ui
                                        .button(format!("Copy the {} picked rows", marked.len() + 1))
                                        .on_hover_text("Picked with Ctrl or Shift, in the order shown")
                                        .clicked()
                                {
                                    let names: Vec<&str> = t.fields.iter().map(|f| f.name.as_str()).collect();
                                    let first = *selected;
                                    let mut lines = vec![names.join("\t")];
                                    for k in (0..t.len()).map(|k| order.map_or(k, |o| o[k])).filter(|k| marked.contains(k) || first == Some(*k)) {
                                        lines.push(t.features[k].attrs.iter().map(plain).collect::<Vec<_>>().join("\t"));
                                    }
                                    ui.ctx().copy_text(lines.join("\n"));
                                    ui.close();
                                }
                                if ui
                                    .button(format!("Copy column {}", t.fields[c].name))
                                    .on_hover_text("Every value of the attribute, one per line, in the order shown")
                                    .clicked()
                                {
                                    let rows: Vec<String> = (0..t.len()).map(|k| order.map_or(k, |o| o[k])).map(|k| plain(&t.features[k].attrs[c])).collect();
                                    ui.ctx().copy_text(format!("{}\n{}", t.fields[c].name, rows.join("\n")));
                                    ui.close();
                                }
                            });
                        });
                    }
                    if row.response().clicked() {
                        clicked = Some(i);
                    }
                });
            });
        if let Some(i) = clicked {
            let keys = ui.input(|k| k.modifiers);
            pick_row(selected, more, i, keys, order);
        }
    });
    heading
}
/// The streams of a View put together for the map and the 3D view.
#[derive(Clone)]
pub(super) struct Together {
    /// The chosen stream, then each stream, by where it is kept.
    keys: Vec<usize>,
    table: Arc<Table>,
    /// Where the chosen stream's features begin among all, and how many they are.
    offset: usize,
    len: usize,
    /// The features first that the 3D models show already.
    models: usize,
}
/// Puts the streams of a View together in the coordinate system of the chosen one, or of the
/// first that has one: the features of all, the models of all, and a grid or a picture. The
/// features have the attributes of the chosen stream, empty for the others. Streams that
/// cannot be put in that system are left out.
pub(super) fn together(ports: &[(String, Arc<Table>)], chosen: usize, keys: Vec<usize>, backend: Option<&dyn Backend>) -> Together {
    let crs = ports[chosen].1.crs.or_else(|| ports.iter().find_map(|p| p.1.crs));
    let base = &ports[chosen].1;
    let mut tables: Vec<(usize, Arc<Table>)> = Vec::new();
    for (i, (_, t)) in ports.iter().enumerate() {
        let t = match (t.crs, crs) {
            (Some(a), Some(b)) if a != b => match og_engine::nodes::reproject(t, b, backend) {
                Ok(r) => Arc::new(r),
                Err(_) => continue,
            },
            (None, Some(_)) if t.has_geometry() || t.mesh.is_some() => continue,
            _ => t.clone(),
        };
        tables.push((i, t));
    }
    // Models first, in the order they are merged, so that their faces are not drawn twice.
    tables.sort_by_key(|(i, t)| (t.mesh.is_none(), *i != chosen, *i));
    let mut out = Table { fields: base.fields.clone(), crs, features: Vec::new(), raster: false, image: None, grid: None, mesh: None };
    let (mut offset, mut len, mut models) = (0, 0, 0);
    let mut meshes = Vec::new();
    for (i, t) in &tables {
        if out.grid.is_none() && out.image.is_none() {
            out.grid = t.grid.clone();
            if t.crs.is_some() {
                out.image = t.image.clone();
            }
        }
        // The outline of a grid or a picture is not a feature to draw.
        let own = if t.grid.is_some() || t.image.is_some() { &[][..] } else { &t.features[..] };
        if *i == chosen {
            offset = out.features.len();
            len = own.len();
        }
        if let Some(m) = &t.mesh {
            meshes.push(m.clone());
            models += own.len();
        }
        let n = out.fields.len();
        for f in own {
            let mut f = f.clone();
            if *i != chosen {
                f.attrs = vec![og_engine::model::Value::Null; n];
            }
            out.features.push(f);
        }
    }
    if !meshes.is_empty() {
        let refs: Vec<&og_engine::mesh::Mesh> = meshes.iter().map(|m| m.as_ref()).collect();
        out.mesh = Some(if refs.len() == 1 { meshes[0].clone() } else { Arc::new(og_engine::mesh::Mesh::merge(&refs)) });
    }
    Together { keys, table: Arc::new(out), offset, len, models }
}
/// A list of names kept in a parameter as a JSON list.
pub(super) fn og_gui_list(v: Option<&serde_json::Value>) -> Vec<String> {
    match v {
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}
