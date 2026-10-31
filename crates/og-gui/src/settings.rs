//! The settings window: tabs on the left, and what each tab sets up on the right: the
//! connections to databases and to web APIs that models use by name, background maps of
//! one's own, and how the program behaves.

use crate::sources::{self, Layers};
use crate::theme::{self, Palette};
use eframe::egui::{self, Align, Layout, RichText, TextEdit};
use og_engine::io::Backend;
use og_engine::settings::{Connection, Settings};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Connections,
    Apis,
    Maps,
    General,
}

const TABS: &[(Tab, &str)] =
    &[(Tab::Connections, "Database connections"), (Tab::Apis, "API connections"), (Tab::Maps, "Background maps"), (Tab::General, "General")];

/// How many features each input reads in a trial run, from the settings.
pub fn trial_features(s: &Settings) -> usize {
    s.other.get("trial_features").and_then(|v| v.as_u64()).filter(|n| *n > 0).map_or(1000, |n| n as usize)
}

/// Whether expressions get suggestions while they are written.
pub fn suggestions(s: &Settings) -> bool {
    s.other.get("suggestions").and_then(|v| v.as_bool()).unwrap_or(true)
}

/// Whether a notice is shown when a long run finishes, and after how many seconds a run counts
/// as long.
pub fn notify(s: &Settings) -> Option<f64> {
    let on = s.other.get("notify").and_then(|v| v.as_bool()).unwrap_or(false);
    on.then(|| s.other.get("notify_after").and_then(|v| v.as_f64()).unwrap_or(30.0).max(0.0))
}

/// Whether motion is kept to a minimum: no running dots on the wires, no glide when the view
/// moves.
pub fn reduce_motion(s: &Settings) -> bool {
    s.other.get("reduce_motion").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// How large the whole program is drawn, 1 as it is made.
pub fn ui_scale(s: &Settings) -> f32 {
    s.other.get("ui_scale").and_then(|v| v.as_f64()).map_or(1.0, |v| v as f32).clamp(0.5, 3.0)
}

const MAP_KINDS: &[(&str, &str)] = &[("xyz", "Tiles: an address with {z}/{x}/{y}"), ("wms", "WMS"), ("wmts", "WMTS")];
const MAP_LOGINS: &[(&str, &str)] =
    &[("", "None"), ("basic", "A user and a password"), ("key_query", "A key in the address"), ("key_header", "A key in a header")];

pub struct SettingsWindow {
    pub tab: Tab,
    /// The connection that is shown, by its place in the list.
    pub selected: Option<usize>,
    /// The answer to Test, and the connection it was for.
    pub tested: Option<(usize, Result<String, String>)>,
    testing: Option<(usize, Receiver<Layers>)>,
    /// What went wrong when the settings were written, if anything.
    pub save_error: Option<String>,
    /// The API connection that is shown.
    pub api: Option<usize>,
    /// The background map that is shown.
    pub map: Option<usize>,
}

impl SettingsWindow {
    pub fn new(settings: &Settings) -> SettingsWindow {
        SettingsWindow {
            tab: Tab::Connections,
            selected: if settings.connections.is_empty() { None } else { Some(0) },
            tested: None,
            testing: None,
            save_error: None,
            api: if settings.apis.is_empty() { None } else { Some(0) },
            map: if settings.maps.is_empty() { None } else { Some(0) },
        }
    }

    pub fn busy(&self) -> bool {
        self.testing.is_some()
    }

    /// Lists the tables of a connection in a thread, to see that it works.
    fn test(&mut self, i: usize, c: &Connection, backend: Arc<og_gdal::GdalBackend>) {
        let conn = match og_engine::nodes::named_connection(c) {
            Ok(c) => c,
            Err(e) => {
                self.tested = Some((i, Err(e.0)));
                return;
            }
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(sources::db_layers(&*backend as &dyn Backend, &conn));
        });
        self.testing = Some((i, rx));
        self.tested = None;
    }

    /// Draws the window. Returns whether it is to close; changes are made in `settings`, and
    /// `changed` says whether there are any.
    pub fn show(&mut self, ctx: &egui::Context, pal: &Palette, settings: &mut Settings, backend: &Arc<og_gdal::GdalBackend>) -> (bool, bool) {
        let mut close = false;
        let mut changed = false;
        if let Some((i, rx)) = &self.testing {
            if let Ok(found) = rx.try_recv() {
                let answer = found.map(|l| format!("Connected. The database has {} {} with geometry.", l.len(), if l.len() == 1 { "table" } else { "tables" }));
                self.tested = Some((*i, answer));
                self.testing = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        let modal = egui::Modal::new(egui::Id::new("gb-settings-window")).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.label(RichText::new("Settings").font(theme::strong(17.0)).color(pal.ink));
            ui.label(
                RichText::new(
                    "Kept for you on this computer, for all models, in settings.json in your folder for settings, with the passwords and keys in the Credential Manager of Windows. The command line og reads them too.",
                )
                .color(pal.muted),
            );
            ui.add_space(8.0);
            // A fixed height: the lines between the parts reach as far down as there is room,
            // which would otherwise make the window grow with every frame.
            let size = egui::vec2(ui.available_width(), 380.0);
            ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Min), |ui| {
                ui.set_min_size(size);
                // The tabs, down the left side.
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    for (tab, label) in TABS {
                        if ui.selectable_label(self.tab == *tab, *label).clicked() {
                            self.tab = *tab;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| match self.tab {
                    Tab::Connections => changed |= self.connections(ui, pal, settings, backend),
                    Tab::Apis => changed |= self.apis(ui, pal, settings),
                    Tab::Maps => changed |= self.maps(ui, pal, settings),
                    Tab::General => changed |= self.general(ui, pal, settings),
                });
            });
            ui.add_space(8.0);
            ui.separator();
            if let Some(e) = &self.save_error {
                ui.label(RichText::new(e).color(pal.err));
            }
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        });
        if modal.should_close() {
            close = true;
        }
        (close, changed)
    }

    fn general(&mut self, ui: &mut egui::Ui, pal: &Palette, settings: &mut Settings) -> bool {
        let mut changed = false;
        ui.label(RichText::new("General").font(theme::strong(14.0)).color(pal.ink));
        ui.add_space(6.0);
        let mut on = suggestions(settings);
        if ui
            .checkbox(&mut on, "Suggest attributes and functions while an expression is written")
            .on_hover_text("Tab or Enter takes the suggestion, Escape closes the list")
            .changed()
        {
            settings.other.insert("suggestions".into(), serde_json::json!(on));
            changed = true;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("A trial run reads at most").color(pal.muted));
            let mut n = trial_features(settings);
            if ui.add(egui::DragValue::new(&mut n).range(1..=10_000_000).speed(10.0)).changed() {
                settings.other.insert("trial_features".into(), serde_json::json!(n));
                changed = true;
            }
            ui.label(RichText::new("features of every input").color(pal.muted));
        });
        ui.label(RichText::new("Model → Trial run, or Ctrl+Shift+F5, to try a model out quickly on large data.").small().color(pal.muted));
        ui.add_space(10.0);
        let mut on = notify(settings).is_some();
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut on, "Tell me when a run that took longer than")
                .on_hover_text("A notice from Windows, and the button of the program in the taskbar flashes, so that you can do something else meanwhile")
                .changed()
            {
                settings.other.insert("notify".into(), serde_json::json!(on));
                changed = true;
            }
            let mut secs = settings.other.get("notify_after").and_then(|v| v.as_f64()).unwrap_or(30.0);
            if ui.add_enabled(on, egui::DragValue::new(&mut secs).range(0.0..=86_400.0).speed(1.0)).changed() {
                settings.other.insert("notify_after".into(), serde_json::json!(secs));
                changed = true;
            }
            ui.label(RichText::new("seconds has finished").color(pal.muted));
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Accessibility").font(theme::strong(13.0)).color(pal.ink));
        let mut calm = reduce_motion(settings);
        if ui
            .checkbox(&mut calm, "Reduce motion")
            .on_hover_text("No moving dots on the wires while a model runs, and the view jumps instead of gliding")
            .changed()
        {
            settings.other.insert("reduce_motion".into(), serde_json::json!(calm));
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Size of everything").color(pal.muted));
            let mut pct = (ui_scale(settings) * 100.0).round() as i32;
            if ui.add(egui::DragValue::new(&mut pct).range(50..=300).speed(1.0).suffix(" %")).changed() {
                settings.other.insert("ui_scale".into(), serde_json::json!(pct as f64 / 100.0));
                changed = true;
            }
            ui.label(RichText::new("Ctrl and + or −, Ctrl+0 back to 100 %").small().color(pal.muted));
        });
        changed
    }

    fn maps(&mut self, ui: &mut egui::Ui, pal: &Palette, settings: &mut Settings) -> bool {
        use og_engine::settings::BackgroundMap;
        let mut changed = false;
        ui.label(RichText::new("Background maps").font(theme::strong(14.0)).color(pal.ink));
        ui.label(
            RichText::new("Maps of your own to choose under the map, besides OpenStreetMap: tiles, WMS or WMTS in Web Mercator (EPSG:3857). A password or a key is kept in the Credential Manager.")
                .color(pal.muted),
        );
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                egui::ScrollArea::vertical().id_salt("maps").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, m) in settings.maps.iter().enumerate() {
                        let name = if m.name.trim().is_empty() { "(no name)".to_string() } else { m.name.clone() };
                        if ui.selectable_label(self.map == Some(i), name).clicked() {
                            self.map = Some(i);
                        }
                    }
                    if settings.maps.is_empty() {
                        ui.label(RichText::new("No background maps yet.").color(pal.muted));
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("New background map").clicked() {
                        let mut n = settings.maps.len() + 1;
                        while settings.maps.iter().any(|m| m.name == format!("Map {}", n)) {
                            n += 1;
                        }
                        settings.maps.push(BackgroundMap { name: format!("Map {}", n), kind: "wms".into(), format: "image/png".into(), ..Default::default() });
                        self.map = Some(settings.maps.len() - 1);
                        changed = true;
                    }
                    let sel = self.map.filter(|i| *i < settings.maps.len());
                    if ui.add_enabled(sel.is_some(), egui::Button::new("Remove")).clicked() {
                        if let Some(i) = sel {
                            settings.maps.remove(i);
                            self.map = if settings.maps.is_empty() { None } else { Some(i.min(settings.maps.len() - 1)) };
                            changed = true;
                        }
                    }
                });
            });
            ui.separator();
            let Some(i) = self.map.filter(|i| *i < settings.maps.len()) else {
                ui.label(RichText::new("Click New background map to add one.").color(pal.muted));
                return;
            };
            ui.vertical(|ui| {
                let m = &mut settings.maps[i];
                let mut picked = false;
                egui::Grid::new("map-form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    let mut row = |ui: &mut egui::Ui, label: &str, v: &mut String, secret: bool, hint: &str, id: &str| {
                        ui.label(RichText::new(label).color(pal.muted));
                        changed |= ui.add(TextEdit::singleline(v).id_salt((id, i)).password(secret).desired_width(300.0).hint_text(hint)).changed();
                        ui.end_row();
                    };
                    row(ui, "Name", &mut m.name, false, "such as Ortofoto", "map-name");
                    ui.label(RichText::new("Kind").color(pal.muted));
                    let shown = MAP_KINDS.iter().find(|k| k.0 == m.kind).map_or("Choose", |k| k.1);
                    let before = m.kind.clone();
                    egui::ComboBox::from_id_salt(("map-kind", i)).selected_text(shown).width(300.0).show_ui(ui, |ui| {
                        for (k, label) in MAP_KINDS {
                            ui.selectable_value(&mut m.kind, k.to_string(), *label);
                        }
                    });
                    picked |= before != m.kind;
                    ui.end_row();
                    match m.kind.as_str() {
                        "wms" => {
                            row(ui, "Address", &mut m.url, false, "https://…/wms", "map-url");
                            row(ui, "Layers", &mut m.layers, false, "separated by commas", "map-layers");
                            row(ui, "Style", &mut m.style, false, "empty: the default", "map-style");
                            row(ui, "Format", &mut m.format, false, "image/png", "map-format");
                        }
                        "wmts" => {
                            row(ui, "Address", &mut m.url, false, "https://…/wmts, or a REST address with {TileMatrix}/{TileRow}/{TileCol}", "map-url");
                            row(ui, "Layer", &mut m.layers, false, "", "map-layers");
                            row(ui, "Style", &mut m.style, false, "default", "map-style");
                            row(ui, "Tile matrix set", &mut m.matrix_set, false, "in Web Mercator, such as GoogleMapsCompatible or 3857", "map-set");
                            row(ui, "Tile matrix", &mut m.matrix, false, "{z}, or such as EPSG:3857:{z}", "map-matrix");
                            row(ui, "Format", &mut m.format, false, "image/png", "map-format");
                        }
                        _ => row(ui, "Address", &mut m.url, false, "https://…/{z}/{x}/{y}.png", "map-url"),
                    }
                    ui.label(RichText::new("Login").color(pal.muted));
                    let shown = MAP_LOGINS.iter().find(|k| k.0 == m.auth).map_or("None", |k| k.1);
                    let before = m.auth.clone();
                    egui::ComboBox::from_id_salt(("map-auth", i)).selected_text(shown).width(300.0).show_ui(ui, |ui| {
                        for (k, label) in MAP_LOGINS {
                            ui.selectable_value(&mut m.auth, k.to_string(), *label);
                        }
                    });
                    picked |= before != m.auth;
                    ui.end_row();
                    match m.auth.as_str() {
                        "basic" => {
                            row(ui, "User", &mut m.user, false, "", "map-user");
                            row(ui, "Password", &mut m.secret, true, "", "map-secret");
                        }
                        "key_query" => {
                            row(ui, "Parameter", &mut m.param, false, "key", "map-param");
                            row(ui, "Key", &mut m.secret, true, "", "map-secret");
                        }
                        "key_header" => {
                            row(ui, "Header", &mut m.param, false, "X-Api-Key", "map-param");
                            row(ui, "Key", &mut m.secret, true, "", "map-secret");
                        }
                        _ => {}
                    }
                    row(ui, "Attribution", &mut m.attribution, false, "such as © Lantmäteriet", "map-attribution");
                });
                changed |= picked;
                ui.label(RichText::new("Choose it under the map, where OpenStreetMap is chosen.").small().color(pal.muted));
                if m.name.trim().is_empty() || m.url.trim().is_empty() {
                    ui.label(RichText::new("Give the map a name and an address.").small().color(pal.warn));
                }
            });
        });
        changed
    }

    fn apis(&mut self, ui: &mut egui::Ui, pal: &Palette, settings: &mut Settings) -> bool {
        use og_engine::web::{ApiConnection, AUTH_KINDS};
        let mut changed = false;
        ui.label(RichText::new("API connections").font(theme::strong(14.0)).color(pal.ink));
        ui.label(
            RichText::new(
                "Keys, users and OAuth2 clients for web services. HTTP request and Input name one of these, so the key stays here and out of the model. Where the key goes in an address or a body, write {{key}}.",
            )
            .color(pal.muted),
        );
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                egui::ScrollArea::vertical().id_salt("apis").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, c) in settings.apis.iter().enumerate() {
                        let name = if c.name.trim().is_empty() { "(no name)".to_string() } else { c.name.clone() };
                        if ui.selectable_label(self.api == Some(i), name).clicked() {
                            self.api = Some(i);
                        }
                    }
                    if settings.apis.is_empty() {
                        ui.label(RichText::new("No API connections yet.").color(pal.muted));
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("New API connection").clicked() {
                        let mut n = settings.apis.len() + 1;
                        while settings.apis.iter().any(|c| c.name == format!("API {}", n)) {
                            n += 1;
                        }
                        settings.apis.push(ApiConnection { name: format!("API {}", n), auth: "key_header".into(), ..Default::default() });
                        self.api = Some(settings.apis.len() - 1);
                        changed = true;
                    }
                    let sel = self.api.filter(|i| *i < settings.apis.len());
                    if ui.add_enabled(sel.is_some(), egui::Button::new("Remove")).clicked() {
                        if let Some(i) = sel {
                            settings.apis.remove(i);
                            self.api = if settings.apis.is_empty() { None } else { Some(i.min(settings.apis.len() - 1)) };
                            changed = true;
                        }
                    }
                });
            });
            ui.separator();
            let Some(i) = self.api.filter(|i| *i < settings.apis.len()) else {
                ui.label(RichText::new("Click New API connection to add one.").color(pal.muted));
                return;
            };
            ui.vertical(|ui| {
                let c = &mut settings.apis[i];
                let kind_changed = egui::Grid::new("api-form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    let mut row = |ui: &mut egui::Ui, label: &str, v: &mut String, secret: bool, hint: &str, id: &str| {
                        ui.label(RichText::new(label).color(pal.muted));
                        changed |= ui.add(TextEdit::singleline(v).id_salt((id, i)).password(secret).desired_width(260.0).hint_text(hint)).changed();
                        ui.end_row();
                    };
                    row(ui, "Name", &mut c.name, false, "such as Trafikverket", "api-name");
                    ui.label(RichText::new("Kind").color(pal.muted));
                    let shown = AUTH_KINDS.iter().find(|k| k.0 == c.auth).map_or("Choose", |k| k.1);
                    let before = c.auth.clone();
                    egui::ComboBox::from_id_salt(("api-kind", i)).selected_text(shown).width(300.0).show_ui(ui, |ui| {
                        for (k, label) in AUTH_KINDS {
                            ui.selectable_value(&mut c.auth, k.to_string(), *label);
                        }
                    });
                    let kind_changed = before != c.auth;
                    ui.end_row();
                    match c.auth.as_str() {
                        "key_header" => {
                            row(ui, "Header", &mut c.param, false, "X-Api-Key", "api-param");
                            row(ui, "Key", &mut c.secret, true, "", "api-secret");
                        }
                        "key_query" => {
                            row(ui, "Parameter", &mut c.param, false, "key", "api-param");
                            row(ui, "Key", &mut c.secret, true, "", "api-secret");
                        }
                        "basic" => {
                            row(ui, "User", &mut c.user, false, "", "api-user");
                            row(ui, "Password", &mut c.secret, true, "", "api-secret");
                        }
                        "oauth2" => {
                            row(ui, "Token address", &mut c.token_url, false, "https://…/token", "api-token");
                            row(ui, "Client id", &mut c.user, false, "", "api-user");
                            row(ui, "Client secret", &mut c.secret, true, "", "api-secret");
                            row(ui, "Scope", &mut c.scope, false, "if the service asks for one", "api-scope");
                        }
                        _ => row(ui, "Key", &mut c.secret, true, "", "api-secret"),
                    }
                    row(ui, "Only for addresses", &mut c.base_url, false, "such as https://api.trafikinfo.trafikverket.se/", "api-base");
                    kind_changed
                });
                changed |= kind_changed.inner;
                ui.label(RichText::new("With an address, the key is only ever sent to addresses that begin with it.").small().color(pal.muted));
                if c.name.trim().is_empty() {
                    ui.label(RichText::new("Give the connection a name, which models use.").small().color(pal.warn));
                }
            });
        });
        changed
    }

    fn connections(&mut self, ui: &mut egui::Ui, pal: &Palette, settings: &mut Settings, backend: &Arc<og_gdal::GdalBackend>) -> bool {
        let mut changed = false;
        ui.label(RichText::new("Database connections").font(theme::strong(14.0)).color(pal.ink));
        ui.label(RichText::new("Inputs and outputs that read or write PostGIS name one of these. The model keeps the name only, so the server, the user and the password stay here.").color(pal.muted));
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            // The list of connections, with buttons under it.
            ui.vertical(|ui| {
                ui.set_width(200.0);
                egui::ScrollArea::vertical().id_salt("connections").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, c) in settings.connections.iter().enumerate() {
                        let name = if c.name.trim().is_empty() { "(no name)".to_string() } else { c.name.clone() };
                        if ui.selectable_label(self.selected == Some(i), name).clicked() {
                            self.selected = Some(i);
                        }
                    }
                    if settings.connections.is_empty() {
                        ui.label(RichText::new("No connections yet.").color(pal.muted));
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("New connection").clicked() {
                        let mut n = settings.connections.len() + 1;
                        while settings.connection(&format!("Connection {}", n)).is_some() {
                            n += 1;
                        }
                        settings.connections.push(Connection {
                            name: format!("Connection {}", n),
                            kind: "postgis".into(),
                            host: "localhost".into(),
                            port: "5432".into(),
                            ..Default::default()
                        });
                        self.selected = Some(settings.connections.len() - 1);
                        changed = true;
                    }
                    let sel = self.selected.filter(|i| *i < settings.connections.len());
                    if ui.add_enabled(sel.is_some(), egui::Button::new("Copy")).clicked() {
                        if let Some(i) = sel {
                            let mut c = settings.connections[i].clone();
                            c.name = format!("{} copy", c.name);
                            settings.connections.insert(i + 1, c);
                            self.selected = Some(i + 1);
                            changed = true;
                        }
                    }
                    if ui.add_enabled(sel.is_some(), egui::Button::new("Remove")).clicked() {
                        if let Some(i) = sel {
                            settings.connections.remove(i);
                            self.selected = if settings.connections.is_empty() { None } else { Some(i.min(settings.connections.len() - 1)) };
                            self.tested = None;
                            changed = true;
                        }
                    }
                });
            });
            ui.separator();
            // The chosen connection.
            let Some(i) = self.selected.filter(|i| *i < settings.connections.len()) else {
                ui.label(RichText::new("Click New connection to add one.").color(pal.muted));
                return;
            };
            ui.vertical(|ui| {
                let names: Vec<String> = settings.connections.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, c)| c.name.trim().to_lowercase()).collect();
                let c = &mut settings.connections[i];
                let mut switched = false;
                egui::Grid::new("connection-form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    let mut row = |ui: &mut egui::Ui, label: &str, v: &mut String, secret: bool, hint: &str, id: &str| {
                        ui.label(RichText::new(label).color(pal.muted));
                        changed |= ui.add(TextEdit::singleline(v).id_salt((id, i)).password(secret).desired_width(280.0).hint_text(hint)).changed();
                        ui.end_row();
                    };
                    row(ui, "Name", &mut c.name, false, "such as VGR prod", "name");
                    ui.label(RichText::new("Kind").color(pal.muted));
                    let shown = if c.kind == "mssql" { "SQL Server" } else { "PostGIS (PostgreSQL)" };
                    egui::ComboBox::from_id_salt(("db-kind", i)).selected_text(shown).width(280.0).show_ui(ui, |ui| {
                        for (k, label) in [("postgis", "PostGIS (PostgreSQL)"), ("mssql", "SQL Server")] {
                            if ui.selectable_label(c.kind == k, label).clicked() && c.kind != k {
                                // The usual port of the one goes with it.
                                if c.port.trim().is_empty() || c.port.trim() == "5432" || c.port.trim() == "1433" {
                                    c.port = if k == "mssql" { "1433".into() } else { "5432".into() };
                                }
                                c.kind = k.to_string();
                                switched = true;
                            }
                        }
                    });
                    ui.end_row();
                    let mssql = c.kind == "mssql";
                    row(ui, "Server", &mut c.host, false, if mssql { "such as sql01 or sql01\\GIS" } else { "localhost" }, "host");
                    row(ui, "Port", &mut c.port, false, if mssql { "1433" } else { "5432" }, "port");
                    row(ui, "Database", &mut c.database, false, "", "database");
                    row(ui, "User", &mut c.user, false, if mssql { "empty: your Windows login" } else { "" }, "user");
                    row(ui, "Password", &mut c.password, true, "", "password");
                });
                changed |= switched;
                if c.name.trim().is_empty() {
                    ui.label(RichText::new("Give the connection a name, which models use.").small().color(pal.warn));
                } else if names.contains(&c.name.trim().to_lowercase()) {
                    ui.label(RichText::new("Another connection has this name. Models will get the first one.").small().color(pal.warn));
                }
                ui.label(
                    RichText::new("Renaming a connection does not change the models that use it; they name the connection by its old name.")
                        .small()
                        .color(pal.muted),
                );
                ui.add_space(6.0);
                let c = c.clone();
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.busy() && !c.database.trim().is_empty(), egui::Button::new("Test connection")).clicked() {
                        self.test(i, &c, backend.clone());
                    }
                    if self.testing.as_ref().is_some_and(|t| t.0 == i) {
                        ui.spinner();
                    }
                });
                if let Some((k, answer)) = &self.tested {
                    if *k == i {
                        match answer {
                            Ok(t) => ui.label(RichText::new(t).color(pal.ok)),
                            Err(e) => ui.label(RichText::new(e).color(pal.err)),
                        };
                    }
                }
            });
        });
        if changed {
            self.tested = None;
        }
        changed
    }
}
