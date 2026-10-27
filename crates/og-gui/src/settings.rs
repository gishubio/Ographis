//! The settings window: tabs on the left, and what each tab sets up on the right: the
//! connections to databases and to web APIs that models use by name.

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
}

const TABS: &[(Tab, &str)] = &[(Tab::Connections, "Database connections"), (Tab::Apis, "API connections")];

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
