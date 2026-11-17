//! Try one request: HTTP request sends only its first request, and the answer is shown in a
//! window, with the status, the headers and the JSON as a tree. A value clicked in the tree
//! gives its path, which can go straight into the settings of the node: where the cursor or
//! the next page is, where the items are, or a new Extract JSON values after the node.

use super::*;
use og_engine::nodes::Tried;
use serde_json::Value as Json;
use std::sync::mpsc::{channel, Receiver};

/// An answer to look at, or one on its way.
pub struct Trial {
    /// The node it was sent for, by its id.
    pub node: String,
    /// The node's name, for the title of the window.
    title: String,
    /// What is sent, as a line: the method and the address.
    sending: String,
    waiting: Option<Receiver<Result<Tried, String>>>,
    pub got: Option<Result<Tried, String>>,
    /// The answer read as JSON, when it is.
    json: Option<Json>,
    /// The path that was clicked in the tree, and whether it points at a list.
    pub picked: Option<(String, bool)>,
}

impl Trial {
    pub fn waiting(&self) -> bool {
        self.waiting.is_some()
    }
}

/// The step from a value to one in it: .name, or [n] in a list. An item of a list of links is
/// told by its rel, as links[rel=next], since its place may change from page to page.
fn step_of(parent: &Json, key: &str) -> String {
    match parent {
        Json::Array(items) => {
            let i: usize = key.parse().unwrap_or(0);
            match items.get(i).and_then(|v| v.get("rel")).and_then(|r| r.as_str()) {
                Some(rel) if !rel.contains([']', '=']) => format!("[rel={}]", rel),
                _ => format!("[{}]", i),
            }
        }
        _ => format!(".{}", key),
    }
}

/// A short look at a value, for the line of the tree it is on.
fn glance(v: &Json) -> String {
    match v {
        Json::Object(o) => format!("{{…}} {} {}", o.len(), if o.len() == 1 { "key" } else { "keys" }),
        Json::Array(a) => format!("[…] {} {}", a.len(), if a.len() == 1 { "item" } else { "items" }),
        Json::String(s) => {
            let short: String = s.chars().take(80).collect();
            if short.len() < s.len() {
                format!("\"{}…\"", short)
            } else {
                format!("\"{}\"", s)
            }
        }
        other => other.to_string(),
    }
}

/// Draws a value and what is in it. Returns the path of a value that was clicked.
fn tree(ui: &mut Ui, pal: &Palette, path: &str, name: &str, v: &Json, picked: Option<&str>, depth: usize) -> Option<(String, bool)> {
    let mut clicked = None;
    let is_on = picked == Some(path);
    let label = |ui: &mut Ui| -> bool {
        let text = RichText::new(format!("{}  {}", name, glance(v))).font(theme::mono(12.0));
        let text = if is_on { text.color(pal.accent) } else { text.color(pal.ink) };
        ui.add(egui::Label::new(text).sense(Sense::click()).truncate()).on_hover_text(format!("Path: {}", path.trim_start_matches('.'))).clicked()
    };
    match v {
        Json::Object(_) | Json::Array(_) => {
            let id = ui.make_persistent_id(("trial-tree", path));
            let open = depth < 2;
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open);
            // A click on what is closed also opens it, in the next frame.
            if ui.ctx().data_mut(|d| d.remove_temp::<bool>(id.with("open"))).unwrap_or(false) {
                state.set_open(true);
            }
            state
                .show_header(ui, |ui| {
                    if label(ui) {
                        clicked = Some((path.to_string(), v.is_array()));
                        ui.ctx().data_mut(|d| d.insert_temp(id.with("open"), true));
                    }
                })
                .body(|ui| {
                    let shown = 200;
                    let children: Vec<(String, &Json)> = match v {
                        Json::Object(o) => o.iter().map(|(k, c)| (k.clone(), c)).collect(),
                        Json::Array(a) => a.iter().enumerate().map(|(i, c)| (i.to_string(), c)).collect(),
                        _ => Vec::new(),
                    };
                    for (key, child) in children.iter().take(shown) {
                        let p = format!("{}{}", path, step_of(v, key));
                        let name = if v.is_array() { format!("[{}]", key) } else { key.clone() };
                        if let Some(c) = tree(ui, pal, &p, &name, child, picked, depth + 1) {
                            clicked = Some(c);
                        }
                    }
                    if children.len() > shown {
                        ui.label(RichText::new(format!("and {} more", children.len() - shown)).small().color(pal.muted));
                    }
                });
        }
        _ => {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                if label(ui) {
                    clicked = Some((path.to_string(), false));
                }
            });
        }
    }
    clicked
}

impl App {
    /// Sends the first request of an HTTP request in a thread, and opens the window for the answer.
    pub(super) fn start_trial(&mut self, node_id: NodeId, url: String, body: Option<String>) {
        let n = &self.snarl[node_id];
        let params = og_engine::run::expand_params(&n.params, &self.variables).unwrap_or_else(|_| n.params.clone());
        let method = params.get("method").and_then(|v| v.as_str()).filter(|m| !m.trim().is_empty()).unwrap_or("GET").to_uppercase();
        let (tx, rx) = channel();
        let backend = self.backend.clone();
        let (u, b) = (url.clone(), body.clone());
        std::thread::spawn(move || {
            let _ = tx.send(og_engine::nodes::try_request(&*backend, &params, &u, b.as_deref()).map_err(|e| e.0));
        });
        self.try_answer = Some(Trial {
            node: n.id.clone(),
            title: n.name.clone(),
            sending: format!("{} {}", method, url),
            waiting: Some(rx),
            got: None,
            json: None,
            picked: None,
        });
    }

    /// The window with the answer of Try one request.
    pub(super) fn trial_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(t) = self.try_answer.as_mut() else { return };
        if let Some(rx) = &t.waiting {
            match rx.try_recv() {
                Ok(got) => {
                    t.json = got.as_ref().ok().and_then(|a| serde_json::from_str::<Json>(a.body.trim_start_matches('\u{feff}')).ok());
                    t.got = Some(got);
                    t.waiting = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(_) => {
                    t.got = Some(Err("The request stopped without an answer.".into()));
                    t.waiting = None;
                }
            }
        }
        let mut open = true;
        let mut action: Option<(&'static str, String)> = None;
        let mut again = false;
        egui::Window::new(format!("Try one request · {}", t.title))
            .id(egui::Id::new("gb-trial-window"))
            .open(&mut open)
            .default_size([560.0, 520.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| match &t.got {
                None => {
                    ui.label(RichText::new(&t.sending).font(theme::mono(12.0)).color(pal.muted));
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Waiting for the answer…");
                    });
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(&t.sending).font(theme::mono(12.0)).color(pal.muted));
                    ui.label(RichText::new(e).color(pal.err));
                    again = ui.button("Try again").clicked();
                }
                Some(Ok(a)) => {
                    ui.label(RichText::new(format!("{} {}", a.method, a.url)).font(theme::mono(12.0)).color(pal.muted));
                    ui.horizontal(|ui| {
                        let colour = if a.status < 300 {
                            pal.ok
                        } else if a.status < 400 {
                            pal.warn
                        } else {
                            pal.err
                        };
                        ui.label(RichText::new(format!("{}", a.status)).font(theme::strong(15.0)).color(colour));
                        let size = if a.body.len() >= 1024 { format!("{:.1} kB", a.body.len() as f64 / 1024.0) } else { format!("{} bytes", a.body.len()) };
                        ui.label(RichText::new(format!("{} ms · {}", a.took.as_millis(), size)).color(pal.muted));
                        if ui.small_button("Again").on_hover_text("Sends the same request once more").clicked() {
                            again = true;
                        }
                    });
                    egui::CollapsingHeader::new(format!("Headers ({})", a.headers.len())).id_salt("trial-headers").show(ui, |ui| {
                        for (k, v) in &a.headers {
                            ui.label(RichText::new(format!("{}: {}", k, v)).font(theme::mono(11.5)));
                        }
                    });
                    ui.separator();
                    match &t.json {
                        Some(j) => {
                            ui.label(RichText::new("Click a value to use its path.").small().color(pal.muted));
                            let picked = t.picked.as_ref().map(|p| p.0.clone());
                            egui::ScrollArea::both().id_salt("trial-tree").auto_shrink([false, false]).max_height(ui.available_height() - 70.0).show(
                                ui,
                                |ui| {
                                    if let Some(c) = tree(ui, pal, "", "answer", j, picked.as_deref(), 0) {
                                        t.picked = Some(c);
                                    }
                                },
                            );
                        }
                        None => {
                            ui.label(RichText::new("The answer is not JSON.").small().color(pal.muted));
                            let mut shown: String = a.body.chars().take(20_000).collect();
                            egui::ScrollArea::both().id_salt("trial-text").auto_shrink([false, false]).max_height(ui.available_height() - 20.0).show(
                                ui,
                                |ui| {
                                    ui.add(egui::TextEdit::multiline(&mut shown).font(theme::mono(11.5)).desired_width(f32::INFINITY).interactive(true));
                                },
                            );
                        }
                    }
                    if let Some((path, list)) = t.picked.clone() {
                        let path = path.trim_start_matches('.').to_string();
                        ui.separator();
                        if path.is_empty() {
                            ui.label(RichText::new("That is the whole answer. Click a value in it.").small().color(pal.muted));
                        } else {
                            ui.label(RichText::new(&path).font(theme::mono(12.0)));
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Cursor at").on_hover_text("Pages follow this value, sent with the next request").clicked() {
                                    action = Some(("cursor_path", path.clone()));
                                }
                                if ui.button("Next page at").on_hover_text("Pages follow the address at this path").clicked() {
                                    action = Some(("next_path", path.clone()));
                                }
                                if list && ui.button("Items at").on_hover_text("The list of items in each page, which tells when the pages end").clicked() {
                                    action = Some(("items_path", path.clone()));
                                }
                                if ui
                                    .button("Extract from JSON")
                                    .on_hover_text("Adds Extract JSON values after this node, taking this value into an attribute")
                                    .clicked()
                                {
                                    action = Some(("extract", path.clone()));
                                }
                            });
                        }
                    }
                }
            });
        if !open {
            self.try_answer = None;
            return;
        }
        if again {
            if let Some(n) = self.try_answer.as_ref().and_then(|t| graph::find(&self.snarl, &t.node)) {
                self.try_request_of(n);
            }
            return;
        }
        if let Some((key, path)) = action {
            self.use_trial_path(key, &path);
        }
    }

    /// Sends the request of an HTTP request once more, with its address and body as they are now.
    pub(super) fn try_request_of(&mut self, node_id: NodeId) {
        let (url, body) = self.trial_request(node_id);
        match url {
            Ok(u) => self.start_trial(node_id, u, body),
            Err(e) => self.say(e),
        }
    }

    /// The address and the body of the first request of a node: worked out from the first
    /// feature that came in on Input in the latest run, or as they are written.
    pub(super) fn trial_request(&self, node_id: NodeId) -> (Result<String, String>, Option<String>) {
        let n = &self.snarl[node_id];
        let id = n.id.clone();
        let text = |key: &str| n.params.get(key).and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        // The first table with features on Input, from the latest run.
        let table = self.result.as_ref().and_then(|r| {
            let own = r.nodes.get(&id).and_then(|nr| nr.inputs.get("Input")).map(|t| t.as_ref());
            own.or_else(|| {
                self.snarl.wires().find(|(_, i)| i.node == node_id && i.input == 0).and_then(|(o, _)| {
                    let from = self.snarl.get_node(o.node)?;
                    let port = from.outputs().get(o.output).cloned()?;
                    r.nodes.get(&from.id).and_then(|nr| nr.outputs.get(&port)).map(|t| t.as_ref())
                })
            })
        });
        let table = table.filter(|t| !t.features.is_empty());
        let value = |src: &str| -> Result<String, String> {
            let src = og_engine::run::expand_variables(src, &self.variables).map_err(|e| e.0)?;
            if let Some(t) = table {
                let names = t.field_names();
                if let Ok(e) = og_engine::expr::compile(&src, &names) {
                    let geographic = og_engine::nodes::is_geographic(Some(&*self.backend), t.crs);
                    let row = og_engine::expr::Row { feature: &t.features[0], index: 0, geographic };
                    return e.eval(&row).map(|v| v.to_text()).map_err(|e| e.0);
                }
            }
            if let Some(v) = og_engine::nodes::constant_text(&src) {
                return Ok(v);
            }
            if src.starts_with("http://") || src.starts_with("https://") {
                return Ok(src);
            }
            Err("The address is built from attributes. Run the model once, so that it can be built from the first feature that comes in.".into())
        };
        let url = match text("url") {
            Some(u) => value(&u),
            None => Err("Write the address to send the request to.".into()),
        };
        let body = text("body").and_then(|b| value(&b).ok());
        (url, body)
    }

    /// Puts a path from the answer into the node it came from.
    fn use_trial_path(&mut self, key: &str, path: &str) {
        let Some(t) = self.try_answer.as_ref() else { return };
        let Some(node_id) = graph::find(&self.snarl, &t.node) else {
            self.say("The node that sent the request is gone.");
            return;
        };
        if key == "extract" {
            let response =
                self.snarl[node_id].params.get("response").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).unwrap_or("_response").to_string();
            // Next to an Extract JSON values already after the node, the path is added to it.
            let after: Option<NodeId> = self
                .snarl
                .wires()
                .filter(|(o, _)| o.node == node_id && o.output == 0)
                .map(|(_, i)| i.node)
                .find(|n| self.snarl.get_node(*n).is_some_and(|g| g.kind == "JsonExtractor"));
            let target = match after {
                Some(n) => n,
                None => {
                    let pos = self.node_rects.iter().find(|r| r.0 == node_id).map_or_else(|| self.free_spot(), |r| egui::pos2(r.1.right() + 60.0, r.1.top()));
                    let mut node = graph::new_node(&self.snarl, "JsonExtractor");
                    node.params.insert("attribute".into(), Json::String(response));
                    let new = self.snarl.insert_node(pos, node);
                    self.snarl.connect(egui_snarl::OutPinId { node: node_id, output: 0 }, egui_snarl::InPinId { node: new, input: 0 });
                    new
                }
            };
            let n = &mut self.snarl[target];
            let mut rows = match n.params.get("paths") {
                Some(Json::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            if !rows.iter().any(|r| r.get(0).and_then(|p| p.as_str()) == Some(path)) {
                rows.push(Json::Array(vec![Json::String(path.to_string()), Json::String(String::new())]));
            }
            n.params.insert("paths".into(), Json::Array(rows));
            let name = n.name.clone();
            self.changed(true);
            self.say(format!("{} takes {}.", name, path));
            return;
        }
        let n = &mut self.snarl[node_id];
        n.params.insert(key.into(), Json::String(path.to_string()));
        // A cursor or a link to the next page tells how the pages go.
        let paging = match key {
            "cursor_path" => Some("cursor"),
            "next_path" => Some("next"),
            _ => None,
        };
        if let Some(p) = paging {
            n.params.insert("paging".into(), Json::String(p.into()));
        }
        let label = match key {
            "cursor_path" => "Cursor at",
            "next_path" => "Next page at",
            _ => "Items at",
        };
        let name = n.name.clone();
        self.changed(true);
        self.say(format!("{}: {} is now {}.", name, label, path));
    }
}
