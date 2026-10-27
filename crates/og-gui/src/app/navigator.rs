//! The panel on the left: Data sets, which lists what is read and written, and Show in folder.

use super::*;

impl App {
    /// The panel on the left: the navigator with what is read and written at the top, and
    /// the list of what can be added below it.
    pub(super) fn left_panel(&mut self, ui: &mut Ui, pal: &Palette) {
        egui::Panel::top("navigator")
            .resizable(true)
            .default_size(236.0)
            .min_size(70.0)
            .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(8, 8)))
            .show(ui, |ui| self.navigator(ui, pal));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(8, 8)))
            .show(ui, |ui| self.palette(ui, pal));
    }

    /// Shows a file or a folder in the file manager of the system.
    pub fn show_in_folder(&mut self, path: &Path) {
        let Some((target, select)) = folder_to_show(path) else {
            self.say(format!("There is no such folder yet: {}", path.display()));
            return;
        };
        self.shown.push(target.clone());
        if self.quiet {
            return;
        }
        if let Err(e) = open_file_manager(&target, select) {
            self.say(format!("Could not open the file manager: {}", e));
        }
    }

    /// Shows the folder of the file an input or an output points to.
    pub fn show_folder_of(&mut self, node: NodeId) {
        let Some(group) = self.snarl.get_node(node).and_then(|n| n.ty()).map(|t| t.group) else { return };
        let target = self.navigator_entries(group).into_iter().find(|e| e.node == node).map(|e| e.target);
        match target {
            Some(Some(path)) => self.show_in_folder(&path),
            _ => self.say("Choose a file first. There is no folder to show yet.".to_string()),
        }
    }

    /// Changes the file or folder a reader or a writer points to.
    pub fn set_path(&mut self, node: NodeId, value: &str) {
        if let Some(n) = self.snarl.get_node_mut(node) {
            if params::set_path(n, value) {
                self.changed(true);
            }
        }
    }

    /// The readers or the writers in the order the navigator lists them: first those the
    /// user has put in an order, then the rest by name.
    pub(super) fn navigator_order(&self, group: Group) -> Vec<NodeId> {
        let mut nodes: Vec<(NodeId, &GNode)> =
            self.snarl.nodes_ids_data().map(|(id, n)| (id, &n.value)).filter(|(_, n)| n.ty().is_some_and(|t| t.group == group)).collect();
        nodes.sort_by_key(|(_, n)| (self.navigator.iter().position(|id| id == &n.id).unwrap_or(usize::MAX), n.name.to_lowercase()));
        nodes.into_iter().map(|(id, _)| id).collect()
    }

    /// Puts the readers or the writers in a new order in the navigator.
    pub fn reorder_navigator(&mut self, group: Group, ids: Vec<String>) {
        let other = if group == Group::Reader { Group::Writer } else { Group::Reader };
        let rest: Vec<String> = self.navigator_order(other).iter().map(|n| self.snarl[*n].id.clone()).collect();
        let (readers, writers) = if group == Group::Reader { (ids, rest) } else { (rest, ids) };
        let order: Vec<String> = readers.into_iter().chain(writers).collect();
        if order != self.navigator {
            self.navigator = order;
            self.touch();
        }
    }

    /// What the navigator says about the readers or the writers: for each, its name, the
    /// path it points to, and some lines about what is there and what the latest run gave.
    pub(super) fn navigator_entries(&mut self, group: Group) -> Vec<NavEntry> {
        let base = self.base_dir();
        let mut out = Vec::new();
        for node in self.navigator_order(group) {
            let n = &self.snarl[node];
            let text = |key: &str| n.params.get(key).and_then(|v| v.as_str()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            let result = self.result.as_ref().and_then(|r| r.nodes.get(&n.id));
            let mut lines: Vec<String> = Vec::new();
            let mut files: Vec<String> = Vec::new();
            let path = text("path");
            let mut target = None;
            if n.kind == "PostgisReader" {
                lines.push(match (text("connection"), text("database")) {
                    (Some(c), _) => format!("PostGIS · {}", c),
                    (None, Some(db)) => format!("PostGIS · {} on {}", db, text("host").unwrap_or_else(|| "localhost".into())),
                    (None, None) => "no connection chosen".into(),
                });
                if let Some(t) = result.and_then(|r| r.outputs.get("Output")) {
                    lines.push(format!("{} features · {}", fmt_count(t.len() as u64), crs_label(t.crs)));
                }
            } else if n.kind == "PostgisWriter" {
                lines.push(match (text("connection"), text("database")) {
                    (Some(c), _) => format!("PostGIS · {}", c),
                    (None, Some(db)) => format!("PostGIS · {} on {}", db, text("host").unwrap_or_else(|| "localhost".into())),
                    (None, None) => "no connection chosen".into(),
                });
                if let Some(t) = text("table") {
                    lines.push(format!("table {}.{}", text("schema").unwrap_or_else(|| "public".into()), t));
                }
                if let Some(t) = result.and_then(|r| r.inputs.get("Input")) {
                    lines.push(format!("{} features written", fmt_count(t.len() as u64)));
                }
            } else if group == Group::Reader {
                if path.is_some() {
                    self.lookup.find_sources(n, &base);
                }
                match (&path, self.lookup.sources(&n.id)) {
                    (None, _) => lines.push("no file chosen".into()),
                    // A file from the web is on this computer once a run has fetched it.
                    (Some(_), Some(Ok(s))) if s.url.is_some() => target = result.and_then(|r| r.sources.first()).cloned(),
                    (Some(_), Some(Ok(s))) if s.is_many() => {
                        let (dir, filter) = s.folder.clone().unwrap_or_default();
                        lines.push(format!("files that match {}", filter.join("; ")));
                        files = s.files.iter().map(|f| f.strip_prefix(&dir).unwrap_or(f).to_string_lossy().replace('\\', "/")).collect();
                        target = Some(dir);
                    }
                    (Some(_), Some(Ok(s))) => target = s.files.first().cloned(),
                    (Some(p), Some(Err(e))) => {
                        lines.push(e.clone());
                        target = Some(base.join(p));
                    }
                    (Some(p), None) => target = Some(base.join(p)),
                }
                let layers = params::list_of(n, "layer");
                match layers.as_slice() {
                    [] => {}
                    [all] if all == "*" => lines.push("all layers".into()),
                    [one] => lines.push(format!("layer {}", one)),
                    many => lines.push(format!("layers {}", many.join(", "))),
                }
                if let Some(limit) = text("limit") {
                    lines.push(format!("at most {} features", limit.parse::<u64>().map(fmt_count).unwrap_or(limit)));
                }
                if let Some(t) = result.and_then(|r| r.outputs.get("Output")) {
                    let crs = if t.crs.is_some() { format!(" · {}", crs_label(t.crs)) } else { String::new() };
                    lines.push(format!("{} features{}", fmt_count(t.len() as u64), crs));
                }
            } else {
                match &path {
                    None => lines.push("no file chosen".into()),
                    Some(p) => target = Some(base.join(og_engine::nodes::writer_file(p, text("format").as_deref()))),
                }
                if let Some(layer) = text("layer") {
                    lines.push(format!("layer {}", layer));
                }
                if let Some(r) = result {
                    if let Some(t) = r.inputs.get("Input") {
                        lines.push(format!("{} features written", fmt_count(t.len() as u64)));
                    }
                    if r.files.len() > 1 {
                        files = r.files.iter().map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
                    }
                }
            }
            if n.disabled {
                lines.push("disabled".into());
            }
            if let Some(e) = result.filter(|r| r.status == Status::Error).and_then(|r| r.error.as_ref()) {
                lines.push(format!("error: {}", e));
            }
            out.push(NavEntry {
                node,
                id: n.id.clone(),
                name: n.name.clone(),
                path: n.params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                target,
                db: matches!(n.kind.as_str(), "PostgisReader" | "PostgisWriter"),
                lines,
                files,
                failed: result.is_some_and(|r| r.status == Status::Error),
                disabled: n.disabled,
            });
        }
        out
    }

    pub(super) fn navigator(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.label(RichText::new("DATA SETS").font(theme::strong(11.5)).color(pal.muted));
        let sections = [(Group::Reader, "Inputs", self.navigator_entries(Group::Reader)), (Group::Writer, "Outputs", self.navigator_entries(Group::Writer))];
        let mut act = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let line = |ui: &mut Ui, text: &str, color: egui::Color32| {
                ui.add(egui::Label::new(RichText::new(text).font(theme::mono(11.0)).color(color)).truncate()).on_hover_text(text);
            };
            for (group, heading, entries) in &sections {
                let title = RichText::new(format!("{} ({})", heading, entries.len())).font(theme::strong(13.5)).color(pal.ink);
                egui::CollapsingHeader::new(title).id_salt(("nav", *heading)).default_open(true).show(ui, |ui| {
                    if entries.is_empty() {
                        let none = if *group == Group::Reader { "No inputs yet. Drop a file on the window." } else { "No outputs yet." };
                        ui.label(RichText::new(none).small().color(pal.muted));
                    }
                    // The rows as they are drawn, and the one that is being dragged or was let go of.
                    let mut rows: Vec<Rect> = Vec::new();
                    let (mut dragged, mut dropped) = (None, None);
                    for (k, e) in entries.iter().enumerate() {
                        let open = self.nav_open.contains(&e.id);
                        let color = if e.failed {
                            pal.err
                        } else if self.selected.contains(&e.node) {
                            pal.accent
                        } else if e.disabled {
                            pal.muted
                        } else {
                            pal.ink
                        };
                        let row = ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            // The monospace font has the triangles. The other one may not.
                            let arrow =
                                egui::Button::new(RichText::new(if open { "▾" } else { "▸" }).font(theme::mono(12.0)).color(pal.muted)).frame(false).small();
                            if ui.add(arrow).on_hover_text(if open { "Fold in" } else { "Show the path and more" }).clicked() {
                                act = Some(NavAct::Toggle(e.id.clone()));
                            }
                            ui.add(egui::Label::new(RichText::new(&e.name).color(color)).truncate().sense(Sense::click_and_drag()))
                        });
                        rows.push(row.response.rect);
                        let label = row.inner;
                        if label.clicked() {
                            act = Some(NavAct::Select(e.id.clone()));
                        }
                        if entries.len() > 1 {
                            if label.dragged() {
                                dragged = Some(k);
                            }
                            if label.drag_stopped() {
                                dropped = Some(k);
                            }
                        }
                        label.context_menu(|ui| {
                            if e.db {
                                ui.label(RichText::new("A database, not a file").color(pal.muted));
                                return;
                            }
                            let show = ui.add_enabled(e.target.is_some(), egui::Button::new("Show in folder"));
                            if show.on_hover_text("Opens the folder of the file in the file manager").clicked() {
                                act = e.target.clone().map(NavAct::Show);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Choose a file…").clicked() {
                                act = Some(NavAct::Browse(e.node, false));
                                ui.close();
                            }
                            if *group == Group::Reader && ui.button("Choose a folder…").clicked() {
                                act = Some(NavAct::Browse(e.node, true));
                                ui.close();
                            }
                        });
                        if !open {
                            continue;
                        }
                        ui.indent(("nav-more", &e.id), |ui| {
                            // The path can be changed here, without going to the node.
                            if !e.db {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    let browse = ui.small_button("…").on_hover_text("Choose a file");
                                    let mut path = e.path.clone();
                                    let field = egui::TextEdit::singleline(&mut path)
                                        .id_salt(("nav-path", &e.id))
                                        .font(theme::mono(11.5))
                                        .hint_text("path to the file")
                                        .desired_width(ui.available_width() - 8.0);
                                    if ui.add(field).on_hover_text(&e.path).changed() {
                                        act = Some(NavAct::Path(e.node, path));
                                    }
                                    if browse.clicked() {
                                        act = Some(NavAct::Browse(e.node, false));
                                    }
                                });
                            }
                            for l in &e.lines {
                                line(ui, l, if l.starts_with("error: ") { pal.err } else { pal.muted });
                            }
                            if !e.files.is_empty() {
                                let files = RichText::new(format!("{} files", e.files.len())).font(theme::mono(11.0)).color(pal.muted);
                                egui::CollapsingHeader::new(files).id_salt(("nav-files", &e.id)).show(ui, |ui| {
                                    for f in e.files.iter().take(300) {
                                        line(ui, f, pal.muted);
                                    }
                                    if e.files.len() > 300 {
                                        line(ui, &format!("… and {} more", e.files.len() - 300), pal.muted);
                                    }
                                });
                            }
                        });
                    }
                    // A row that is dragged goes in before the first row whose middle is
                    // below the pointer. A line shows where.
                    let Some(k) = dragged.or(dropped) else { return };
                    let Some(pointer) = ui.ctx().input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos())) else { return };
                    let to = rows.iter().filter(|r| r.center().y < pointer.y).count();
                    if dragged.is_some() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        let y = if to < rows.len() { rows[to].top() - 1.0 } else { rows[rows.len() - 1].bottom() + 1.0 };
                        ui.painter().hline(rows[0].x_range(), y, egui::Stroke::new(2.0, pal.accent));
                    }
                    if dropped.is_some() && to != k && to != k + 1 {
                        let mut ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
                        let moved = ids.remove(k);
                        ids.insert(if to > k { to - 1 } else { to }, moved);
                        act = Some(NavAct::Reorder(*group, ids));
                    }
                });
            }
        });
        match act {
            Some(NavAct::Select(id)) => {
                self.focus_node(&id);
                self.reveal(&id);
                self.nav_open.insert(id);
            }
            Some(NavAct::Toggle(id)) => {
                if !self.nav_open.remove(&id) {
                    self.nav_open.insert(id);
                }
            }
            Some(NavAct::Path(node, path)) => self.set_path(node, &path),
            Some(NavAct::Browse(node, folder)) => self.browse_for(node, folder),
            Some(NavAct::Show(path)) => self.show_in_folder(&path),
            Some(NavAct::Reorder(group, ids)) => self.reorder_navigator(group, ids),
            None => {}
        }
    }

    /// Asks for a file, or a folder, for a reader or a writer with the dialog of the system.
    pub(super) fn browse_for(&mut self, node: NodeId, folder: bool) {
        let Some(writer) = self.snarl.get_node(node).map(|n| n.kind == "Writer") else { return };
        let dialog = rfd::FileDialog::new().set_directory(self.base_dir());
        let picked = if folder {
            dialog.pick_folder()
        } else {
            let dialog = dialog
                .add_filter(
                    "Spatial data and tables",
                    &[
                        "gpkg", "shp", "geojson", "json", "fgb", "parquet", "sqlite", "db", "dxf", "gml", "xlsx", "ods", "tif", "tiff", "csv", "txt", "zip",
                        "obj", "gltf", "glb", "ifc", "b3dm", "las", "laz",
                    ],
                )
                .add_filter("All files", &["*"]);
            if writer {
                dialog.save_file()
            } else {
                dialog.pick_file()
            }
        };
        if let Some(p) = picked {
            let stored = self.stored_path(&p);
            self.set_path(node, &stored);
            if folder {
                // The filter starts as the kind of file there is most of in the folder.
                let pattern = sources::guess_pattern(&p).filter(|g| g != DEFAULT_PATTERN);
                if let Some(n) = self.snarl.get_node_mut(node) {
                    match pattern {
                        Some(g) => n.params.insert("pattern".into(), serde_json::Value::String(g)),
                        None => n.params.remove("pattern"),
                    };
                }
            }
        }
    }
}

/// One reader or writer as the navigator shows it.
pub(super) struct NavEntry {
    node: NodeId,
    id: String,
    name: String,
    /// The path as it is stored in the node, to be changed in place.
    path: String,
    /// The file or folder on this computer, for showing it in the file manager.
    target: Option<PathBuf>,
    /// A table in a database rather than a file.
    db: bool,
    /// More about what it points to, and what the latest run gave.
    lines: Vec<String>,
    /// The files of a folder, or the files a writer wrote when there are several.
    files: Vec<String>,
    failed: bool,
    disabled: bool,
}
/// What the user did in the navigator.
pub(super) enum NavAct {
    Select(String),
    Toggle(String),
    Path(NodeId, String),
    /// Choose a file with the file dialog, or a folder if the flag is set.
    Browse(NodeId, bool),
    Show(PathBuf),
    /// The new order of the ids in one of the two lists.
    Reorder(Group, Vec<String>),
}
/// Opens the file manager of the system at a folder, or with a file marked in its folder.
pub(super) fn open_file_manager(target: &Path, select: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Explorer wants backslashes, and the path in quotes right after the comma.
        let path = target.to_string_lossy().replace('/', "\\");
        let mut explorer = std::process::Command::new("explorer.exe");
        if select {
            explorer.raw_arg(format!("/select,\"{}\"", path));
        } else {
            explorer.raw_arg(format!("\"{}\"", path));
        }
        explorer.spawn().map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        let mut open = std::process::Command::new("open");
        if select {
            open.arg("-R");
        }
        open.arg(target).spawn().map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let folder = if select { target.parent().unwrap_or(target) } else { target };
        std::process::Command::new("xdg-open").arg(folder).spawn().map(|_| ())
    }
}
