//! The panel on the right: the settings of the selected node, group box or note.

use super::*;

impl App {
    pub(super) fn params_panel(&mut self, ui: &mut Ui, pal: &Palette) {
        if self.selected.len() > 1 {
            self.selection_panel(ui, pal);
            return;
        }
        let Some(node_id) = self.focus.filter(|n| self.snarl.get_node(*n).is_some()) else {
            if let Some(g) = self.selected_group.filter(|g| *g < self.groups.len()) {
                self.group_panel(ui, pal, g);
                return;
            }
            if let Some((from, to)) = self.selected_wire.and_then(|w| self.wire_text(w)) {
                ui.label(RichText::new("Connection").font(theme::strong(17.0)));
                ui.label(RichText::new("From").small().color(pal.muted));
                ui.label(from);
                ui.label(RichText::new("To").small().color(pal.muted));
                ui.label(to);
                let wire = self.selected_wire.unwrap();
                let off = graph::wire_disabled(&self.snarl, wire.0, wire.1);
                if off {
                    ui.add_space(4.0);
                    ui.label(RichText::new("This connection is disabled. It lets nothing through.").color(pal.muted));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(if off { "Enable connection" } else { "Disable connection" }).clicked() {
                        self.toggle_wire(wire);
                    }
                    if ui.button(RichText::new("Delete connection").color(pal.err)).on_hover_text("Delete").clicked() {
                        self.remove_selected_wire();
                    }
                });
                return;
            }
            // With nothing selected there is nothing to set up. In a new, empty model the
            // panel says what the program is for and what it reads and writes; once work has
            // begun it stays empty.
            if self.snarl.nodes().next().is_none() {
                self.intro(ui, pal);
            }
            return;
        };
        if self.snarl[node_id].is_note() {
            self.note_panel(ui, pal, node_id);
            return;
        }
        let base = self.base_dir();
        let result = self.result.clone();
        let (id, kind, group, desc) = {
            let n = &self.snarl[node_id];
            (n.id.clone(), n.kind.clone(), n.ty().map(|t| t.group), n.ty().map_or("", |t| t.desc))
        };
        // What a reader points to is looked up before the form is drawn: the files of a
        // folder, or the layers of a single file.
        let mut layer_file = None;
        if kind == "Reader" {
            let node = &self.snarl[node_id];
            if node.params.contains_key("path") {
                self.lookup.find_sources(node, &base);
            }
            if let Some(Ok(s)) = self.lookup.sources(&id) {
                layer_file = s.files.first().filter(|_| !s.is_many()).cloned();
                // A file on the web is on this computer once it has been fetched by a run.
                if let Some(url) = &s.url {
                    let web_as = node.params.get("web_as").and_then(|v| v.as_str()).map(str::to_string);
                    match og_engine::nodes::service_name(url, web_as.as_deref()).filter(|_| !url.contains("{{key}}")) {
                        // A service lists its layers itself, in a thread.
                        Some(name) => layer_file = Some(PathBuf::from(name)),
                        None => {
                            layer_file = result.as_ref().and_then(|r| r.nodes.get(&id)).and_then(|r| r.sources.first()).filter(|f| f.exists()).cloned();
                        }
                    }
                }
            }
            if let Some(file) = &layer_file {
                if og_engine::nodes::is_service(file) {
                    self.lookup.find_service_layers(self.backend.clone(), file);
                    if self.lookup.poll() {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    }
                } else {
                    self.lookup.find_layers(&*self.backend, file);
                }
            }
        }
        // The tables of a database are listed when the button above the form asks for them,
        // since a server can take a while to answer.
        let mut db_key: Option<String> = None;
        if kind == "PostgisReader" {
            let n = &self.snarl[node_id];
            if let Some(ty) = n.ty() {
                let params = og_engine::run::expand_params(&n.params, &self.variables).unwrap_or_else(|_| n.params.clone());
                if let Ok((conn, _)) = og_engine::nodes::postgis_connection(&og_engine::nodes::P::new(&params, ty.params)) {
                    db_key = Some(conn);
                }
            }
        }
        ui.horizontal(|ui| {
            if let Some(g) = group {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), Sense::hover());
                ui.painter().rect_filled(Rect::from_center_size(dot.center(), egui::vec2(9.0, 9.0)), 2.0, pal.group(g).0);
            }
            ui.label(RichText::new(graph::title_of(&kind)).font(theme::strong(17.0)));
        });
        let mut remove = false;
        let mut toggle = false;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // The form must never be wider than the panel, or the panel grows. Room is left
            // for the scroll bar, which otherwise lies on top of the fields.
            ui.set_max_width(ui.available_width() - 10.0);
            ui.label(RichText::new(desc).color(pal.muted));
            ui.add_space(4.0);
            // What is connected to each input port: where from, and the name to show.
            let mut connected: HashMap<String, Vec<(String, String)>> = HashMap::new();
            let ins = self.snarl[node_id].inputs();
            for (out, inp) in self.snarl.wires() {
                let (Some(port), true) = (ins.get(inp.input), inp.node == node_id) else { continue };
                let Some(from) = self.snarl.get_node(out.node) else { continue };
                let ports = from.outputs();
                let Some(name) = ports.get(out.output) else { continue };
                let label = if ports.len() > 1 { format!("{} · {}", from.name, name) } else { from.name.clone() };
                connected.entry(port.clone()).or_default().push((format!("{}:{}", from.id, name), label));
            }
            for list in connected.values_mut() {
                list.sort();
            }
            let connections = |port: &str| connected.get(port).cloned().unwrap_or_default();
            // The tables that came in on a port in the latest run. A tool added after that run
            // has none of its own yet, so it is shown what the tools before it gave out.
            let incoming = |port: &str| -> Vec<&og_engine::model::Table> {
                let Some(r) = result.as_ref() else { return Vec::new() };
                if let Some(t) = r.nodes.get(&id).and_then(|n| n.inputs.get(port)) {
                    return vec![t.as_ref()];
                }
                connected
                    .get(port)
                    .map(|list| {
                        list.iter()
                            .filter_map(|(from, _)| from.rsplit_once(':'))
                            .filter_map(|(node, out)| r.nodes.get(node).and_then(|n| n.outputs.get(out)).map(|t| t.as_ref()))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let fields = |port: &str| {
                let mut names: Vec<String> = Vec::new();
                for t in incoming(port) {
                    for f in t.field_names() {
                        if !names.contains(&f) {
                            names.push(f);
                        }
                    }
                }
                names
            };
            let types = |port: &str| {
                let mut out: Vec<(String, &'static str)> = Vec::new();
                for t in incoming(port) {
                    for f in &t.fields {
                        if !out.iter().any(|(n, _)| n == &f.name) {
                            out.push((f.name.clone(), f.ty.label()));
                        }
                    }
                }
                out
            };
            let values = |port: &str, attr: &str| {
                let mut out: Vec<String> = Vec::new();
                for table in incoming(port) {
                    let Some(i) = table.field_index(attr) else { continue };
                    for f in &table.features {
                        let v = f.attrs[i].to_text();
                        if !v.is_empty() && !out.contains(&v) {
                            out.push(v);
                            if out.len() >= 100 {
                                return out;
                            }
                        }
                    }
                }
                out
            };
            let has_path = self.snarl[node_id].params.contains_key("path");
            if self.snarl[node_id].disabled {
                let mut enable = false;
                egui::Frame::new().fill(pal.bg).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                    ui.label(RichText::new("This node is disabled. It does not run and lets nothing through.").color(pal.muted));
                    enable = ui.button("Enable").clicked();
                });
                if enable {
                    self.toggle_disabled(&[node_id]);
                }
                ui.add_space(4.0);
            }
            let mut open_db = false;
            let mut to_file = false;
            if kind == "PostgisReader" || kind == "PostgisWriter" {
                ui.horizontal(|ui| {
                    let what = if kind == "PostgisReader" { "Choose tables…" } else { "Choose table…" };
                    open_db = ui.button(what).on_hover_text("The connection, and the tables of the database to choose from").clicked();
                    if kind == "PostgisReader" {
                        to_file = ui.button("Read a file instead").clicked();
                    }
                });
                ui.add_space(4.0);
            }
            if open_db {
                self.open_postgis(node_id, kind == "PostgisWriter", None);
            }
            if to_file {
                self.switch_kind(node_id, "Reader");
                return;
            }
            if let Some(key) = &db_key {
                let listed = self.lookup.layers(std::path::Path::new(key)).is_some();
                let label = if listed { "List the tables again" } else { "Connect and list the tables" };
                if ui.button(label).on_hover_text("Connects to the database with the settings below and lists the tables that have geometry").clicked() {
                    self.lookup.find_db_layers(&*self.backend, key);
                }
                ui.add_space(4.0);
            }
            let db_connections: Vec<String> = self.settings.connections.iter().map(|c| c.name.clone()).collect();
            let api_connections: Vec<String> = self.settings.apis.iter().map(|c| c.name.clone()).collect();
            if self.crs_list.is_empty() {
                self.crs_list = Arc::new(self.backend.crs_list());
            }
            let crs_list = self.crs_list.clone();
            let stream_fields = |from: &str| -> Vec<(String, bool)> {
                let Some((node, port)) = from.rsplit_once(':') else { return Vec::new() };
                let table = result.as_ref().and_then(|r| r.nodes.get(node)).and_then(|r| r.outputs.get(port));
                table.map(|t| t.fields.iter().map(|f| (f.name.clone(), f.ty != og_engine::model::FieldType::Text)).collect()).unwrap_or_default()
            };
            let backend_ref = &*self.backend;
            let variables_ref = &self.variables;
            let preview = |port: &str, src: &str| -> Option<String> {
                let tables = incoming(port);
                let table = tables.iter().find(|t| !t.features.is_empty())?;
                let names = table.field_names();
                let text = og_engine::run::expand_variables(src, variables_ref).ok()?;
                let e = og_engine::expr::compile(&text, &names).ok()?;
                let geographic = og_engine::nodes::is_geographic(Some(backend_ref), table.crs);
                let row = og_engine::expr::Row { feature: &table.features[0], index: 0, geographic };
                Some(match e.eval(&row) {
                    Ok(v) if v.is_null() => "First feature: null".to_string(),
                    Ok(v) => {
                        let t = v.to_text();
                        let t = if t.chars().count() > 120 { format!("{}…", t.chars().take(120).collect::<String>()) } else { t };
                        format!("First feature: {}", t)
                    }
                    Err(e) => format!("First feature: {}", e.0),
                })
            };
            let cx = ParamCtx {
                preview: &preview,
                stream_fields: &stream_fields,
                suggest: crate::settings::suggestions(&self.settings),
                db_connections: &db_connections,
                api_connections: &api_connections,
                crs_list: &crs_list,
                pal: *pal,
                fields: &fields,
                field_types: &types,
                base_dir: &base,
                relative: self.path.is_some(),
                sources: if kind == "Reader" && has_path { self.lookup.sources(&id) } else { None },
                layers: layer_file
                    .as_ref()
                    .and_then(|f| self.lookup.layers(f))
                    .or_else(|| db_key.as_ref().and_then(|k| self.lookup.layers(std::path::Path::new(k)))),
                values: &values,
                variables: &self.variables,
                connections: &connections,
            };
            let node = &mut self.snarl[node_id];
            let ports_before = node.outputs();
            let format_before = node.params.get("format").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let edited = params::node_form(ui, node, &cx);
            // An input asked to read from PostGIS, or an output given a format that another
            // tool writes: a GeoSpatial PDF or a table in PostGIS.
            if edited.postgis {
                self.open_postgis(node_id, false, None);
            }
            if edited.open_settings {
                self.open_settings();
            }
            let format_now = self.snarl[node_id].params.get("format").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if group == Some(Group::Writer) && format_now != format_before {
                let want = og_engine::nodes::output_kind(&format_now);
                if want == "PostgisWriter" {
                    self.open_postgis(node_id, true, Some(format_before.clone()));
                } else if want != kind {
                    self.switch_kind(node_id, want);
                    // A file format chosen for the PDF or PostGIS output is the format of the file.
                    let n = &mut self.snarl[node_id];
                    if want == "Writer" {
                        n.params.insert("format".into(), serde_json::Value::String(format_now.clone()));
                    }
                    let path = n.params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if !path.is_empty() && !format_now.is_empty() {
                        n.params.insert("path".into(), serde_json::Value::String(og_engine::nodes::writer_file(&path, Some(&format_now))));
                    }
                }
            }
            // A tool whose outputs follow its settings keeps its connections by the names
            // of the ports.
            if self.snarl[node_id].outputs() != ports_before {
                self.follow_ports(node_id, &ports_before);
            }
            let now = ui.input(|i| i.time);
            let old = self.result_is_old(&id, now);
            if old && result.as_ref().and_then(|r| r.nodes.get(&id)).is_some_and(|r| r.error.is_some() || !r.warnings.is_empty()) {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("From the latest run, before the settings were changed. Run again (F5) to see what they give.").small().color(pal.muted),
                );
            }
            if let Some(r) = result.as_ref().and_then(|r| r.nodes.get(&id)) {
                match (&r.error, r.status) {
                    // That a node is switched off is said above, and is no error.
                    (_, Status::Disabled) | (None, _) => {}
                    (Some(why), Status::Skipped) => {
                        ui.add_space(6.0);
                        ui.label(RichText::new(why).color(pal.warn));
                    }
                    (Some(e), _) => {
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(pal.err_soft)
                            .corner_radius(4)
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| ui.label(RichText::new(e).color(pal.err)));
                    }
                }
                for w in &r.warnings {
                    ui.label(RichText::new(w).color(if old { pal.muted } else { pal.warn }));
                }
            }
            // The attributes as they are after this step, the same list that can be folded
            // out on the node.
            ui.add_space(6.0);
            let lists = graph::attributes(&self.snarl[node_id], result.as_deref());
            let total: usize = lists.as_ref().map_or(0, |l| l.iter().map(|x| x.1.len()).sum());
            let heading = match &lists {
                Some(l) if l.len() == 1 => format!("Attributes after this step ({})", total),
                _ => "Attributes after this step".to_string(),
            };
            egui::CollapsingHeader::new(RichText::new(heading).small().color(pal.ink)).id_salt("attributes_after").show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let Some(lists) = &lists else {
                    ui.label(RichText::new("Run the model, and the attributes are listed here.").small().color(pal.muted));
                    return;
                };
                for (port, attrs) in lists {
                    if let Some(port) = port {
                        ui.label(RichText::new(port).font(theme::mono(11.0)).color(pal.muted));
                    }
                    if attrs.is_empty() {
                        ui.label(RichText::new("none").font(theme::mono(11.5)).color(pal.muted));
                    }
                    for (name, ty) in attrs {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).font(theme::mono(11.5)).color(pal.ink));
                            ui.label(RichText::new(*ty).font(theme::mono(10.5)).color(pal.muted));
                        });
                    }
                }
                let on = self.expanded.contains(&id);
                if ui.small_button(if on { "Hide on the node" } else { "Show on the node" }).clicked() {
                    toggle = true;
                }
            });
            ui.add_space(8.0);
            if ui.button(RichText::new("Delete node").color(pal.err)).clicked() {
                remove = true;
            }
            if edited.any {
                self.touch();
            }
            if edited.run {
                self.stale = true;
            }
        });
        if toggle && !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        if remove {
            self.remove_nodes(&[node_id]);
        }
    }

    /// What the panel shows for a free-standing note.
    pub(super) fn note_panel(&mut self, ui: &mut Ui, pal: &Palette, node: NodeId) {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), Sense::hover());
            ui.painter().rect_filled(Rect::from_center_size(dot.center(), egui::vec2(9.0, 9.0)), 2.0, pal.note_line);
            ui.label(RichText::new("Note").font(theme::strong(17.0)));
        });
        ui.label(RichText::new("A free-standing note on the canvas. It is saved with the model and is not part of the run.").color(pal.muted));
        ui.add_space(4.0);
        ui.label(RichText::new("Text").small().color(pal.muted));
        let edit = egui::TextEdit::multiline(&mut self.snarl[node].comment).desired_width(f32::INFINITY).desired_rows(6).hint_text("Write the note here");
        if ui.add(edit).changed() {
            self.touch();
        }
        ui.add_space(8.0);
        if ui.button(RichText::new("Delete note").color(pal.err)).clicked() {
            self.remove_nodes(&[node]);
        }
    }

    /// What the panel shows for a group box.
    pub(super) fn group_panel(&mut self, ui: &mut Ui, pal: &Palette, index: usize) {
        let inside = self.nodes_in_group(index);
        ui.label(RichText::new("Group").font(theme::strong(17.0)));
        ui.label(RichText::new("A named box that holds nodes together. Drag it by its name, and resize it by its lower right corner.").color(pal.muted));
        ui.add_space(4.0);
        let before = self.groups[index].clone();
        let g = &mut self.groups[index];
        ui.label(RichText::new("Name").small().color(pal.muted));
        ui.add(egui::TextEdit::singleline(&mut g.name).desired_width(f32::INFINITY));
        ui.add_space(2.0);
        ui.label(RichText::new("Colour").small().color(pal.muted));
        let own = theme::parse_hex(&g.color);
        let shown = match own {
            Some(_) => "A colour of its own",
            None => GROUP_COLORS.iter().find(|c| c.0 == g.color).map_or("Grey", |c| c.1),
        };
        ui.horizontal(|ui| {
            // The colour of its own, picked in a window of its own.
            // It starts from the colour the box has.
            let now = own.unwrap_or_else(|| pal.group_box(&g.color).0);
            let mut rgb = [now.r(), now.g(), now.b()];
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).on_hover_text("Choose any colour").changed() {
                g.color = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
            }
            egui::ComboBox::from_id_salt("group_color").selected_text(shown).width(ui.available_width()).height(400.0).show_ui(ui, |ui| {
                for (value, label) in GROUP_COLORS {
                    let (edge, fill) = pal.group_box(value);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        ui.painter().rect(r, 2.0, fill, egui::Stroke::new(1.5, edge), egui::StrokeKind::Inside);
                        ui.selectable_value(&mut g.color, value.to_string(), *label);
                    });
                }
            });
        });
        ui.add_space(4.0);
        ui.checkbox(&mut g.locked, "Lock the contents");
        let explained = if g.locked { "The nodes in the box move with it." } else { "The box moves alone. The nodes stay where they are." };
        ui.label(RichText::new(explained).small().color(pal.muted));
        if *g != before {
            self.touch();
        }
        ui.add_space(6.0);
        let n = inside.len();
        ui.label(RichText::new(format!("{} {} in the box", n, if n == 1 { "node" } else { "nodes" })).color(pal.muted));
        let (mut select, mut fit, mut remove) = (false, false, false);
        ui.horizontal_wrapped(|ui| {
            select = ui.add_enabled(n > 0, egui::Button::new("Select them")).clicked();
            fit = ui.add_enabled(n > 0, egui::Button::new("Fit the box to them")).clicked();
        });
        ui.add_space(8.0);
        remove = ui.button(RichText::new("Delete group").color(pal.err)).on_hover_text("The nodes in it stay").clicked() || remove;
        if fit {
            self.fit_group(index);
        }
        if select {
            self.set_selection(inside);
        }
        if remove {
            self.remove_group(index);
        }
    }

    /// What the parameter panel shows when several nodes are selected.
    pub(super) fn selection_panel(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.label(RichText::new(format!("{} nodes selected", self.selected.len())).font(theme::strong(16.0)));
        ui.label(RichText::new("Drag one of them to move them all. Click a single node to set it up.").color(pal.muted));
        ui.add_space(6.0);
        let (mut duplicate, mut remove, mut group) = (false, false, false);
        ui.horizontal(|ui| {
            group = ui.button("Group").on_hover_text("Put a group box around them (Ctrl+G)").clicked();
            duplicate = ui.button("Duplicate").on_hover_text("Ctrl+D").clicked();
            remove = ui.button(RichText::new("Delete").color(pal.err)).on_hover_text("Delete").clicked();
        });
        ui.add_space(6.0);
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for n in &self.selected {
                if let Some(node) = self.snarl.get_node(*n) {
                    ui.horizontal(|ui| {
                        let color = if node.is_note() { Some(pal.note_line) } else { node.ty().map(|t| pal.group(t.group).0) };
                        if let Some(color) = color {
                            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), Sense::hover());
                            ui.painter().rect_filled(Rect::from_center_size(dot.center(), egui::vec2(9.0, 9.0)), 2.0, color);
                        }
                        ui.label(&node.name);
                    });
                }
            }
        });
        if group {
            self.add_group(&self.selected.clone(), None);
        }
        if duplicate {
            self.duplicate_nodes(&self.selected.clone());
        }
        if remove {
            self.remove_nodes(&self.selected.clone());
        }
    }
}
