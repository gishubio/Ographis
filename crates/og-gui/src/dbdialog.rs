//! The window that connects an input or an output to PostGIS: a connection from the
//! settings, and then the tables to read, or the schema and table to write to. The tables are
//! listed in a thread of their own, since a server can take a while to answer.

use crate::sources::{self, Layers};
use crate::theme::{self, Palette};
use eframe::egui::{self, Align, Layout, RichText, TextEdit};
use og_engine::io::{Backend, LayerInfo};
use og_engine::nodes::Params;
use og_engine::settings::Settings;
use serde_json::Value as Json;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// The settings of a connection as models before 0.63 stored them in the node.
pub const CONNECTION_KEYS: &[&str] = &["host", "port", "database", "user", "password"];

pub struct DbDialog {
    /// The node it is for, by its id.
    pub node: String,
    /// Writing to a table, not reading.
    pub output: bool,
    /// The name of the connection in the settings.
    pub connection: String,
    /// The tables ticked to read.
    pub ticked: Vec<String>,
    pub search: String,
    /// Where to write: the schema and the table.
    pub schema: String,
    pub table: String,
    /// The tables of the database, once listed.
    pub tables: Option<Layers>,
    /// The connection the list belongs to, to keep it for the form.
    pub listed_for: Option<String>,
    /// The name of the connection the list is of.
    listed_name: String,
    listing: Option<Receiver<(String, Layers)>>,
    /// The format the output had before PostGIS was chosen, to go back to on Cancel.
    pub revert_format: Option<String>,
}

pub enum DbAction {
    Cancel,
    Use,
    /// Open the settings, to add or change a connection.
    Settings,
}

fn text(params: &Params, key: &str) -> String {
    params.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

impl DbDialog {
    /// A window for a node, with its connection, or else that of another node that reads or
    /// writes PostGIS, since a model mostly uses one database, or else the first one there is.
    pub fn new(node: String, output: bool, own: &Params, other: Option<&Params>, settings: &Settings) -> DbDialog {
        let or = |s: String, d: &str| if s.trim().is_empty() { d.to_string() } else { s };
        let ticked: Vec<String> = match own.get("tables") {
            Some(Json::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        let mut connection = text(own, "connection");
        if connection.is_empty() {
            connection = other.map(|o| text(o, "connection")).unwrap_or_default();
        }
        if connection.is_empty() {
            connection = settings.connections.first().map(|c| c.name.clone()).unwrap_or_default();
        }
        DbDialog {
            node,
            output,
            connection,
            ticked,
            search: String::new(),
            schema: or(text(own, "schema"), "public"),
            table: text(own, "table"),
            tables: None,
            listed_for: None,
            listed_name: String::new(),
            listing: None,
            revert_format: None,
        }
    }

    /// Starts listing the tables of the chosen connection.
    pub fn connect(&mut self, backend: Arc<og_gdal::GdalBackend>, settings: &Settings) {
        self.listed_name = self.connection.clone();
        let conn = match settings
            .connection(&self.connection)
            .ok_or_else(|| format!("There is no connection called {}.", self.connection))
            .and_then(|c| og_engine::nodes::named_connection(c).map_err(|e| e.0))
        {
            Ok(c) => c,
            Err(e) => {
                self.tables = Some(Err(e));
                return;
            }
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let found = sources::db_layers(&*backend as &dyn Backend, &conn);
            let _ = tx.send((conn, found));
        });
        self.listing = Some(rx);
        self.tables = None;
    }

    /// Takes in the list when it has come. Returns the connection and the list then.
    pub fn poll(&mut self) -> Option<(String, Layers)> {
        let got = self.listing.as_ref()?.try_recv().ok()?;
        self.listing = None;
        self.tables = Some(got.1.clone());
        self.listed_for = Some(got.0.clone());
        // Tables that are no longer there are not kept ticked.
        if let Ok(list) = &got.1 {
            self.ticked.retain(|t| list.iter().any(|l| &l.name == t));
        }
        Some(got)
    }

    pub fn busy(&self) -> bool {
        self.listing.is_some()
    }

    /// The schema and the name of a table as PostGIS lists it: public tables without a schema.
    fn split(name: &str) -> (String, String) {
        match name.split_once('.') {
            Some((s, t)) => (s.to_string(), t.to_string()),
            None => ("public".to_string(), name.to_string()),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, pal: &Palette, backend: &Arc<og_gdal::GdalBackend>, settings: &Settings) -> Option<DbAction> {
        let mut action = None;
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // A connection that is chosen, or changed, is listed at once.
        if !self.busy() && !self.connection.is_empty() && self.listed_name != self.connection && settings.connection(&self.connection).is_some() {
            self.connect(backend.clone(), settings);
        }
        let modal = egui::Modal::new(egui::Id::new("postgis")).show(ctx, |ui| {
            ui.set_width(560.0);
            let title = if self.output { "Write to PostGIS" } else { "Read from PostGIS" };
            ui.label(RichText::new(title).font(theme::strong(17.0)).color(pal.ink));
            ui.label(
                RichText::new("Choose a database connection. Connections are made under Edit → Settings, and the model keeps only the name.").color(pal.muted),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Connection").color(pal.muted));
                let shown = if self.connection.is_empty() { "Choose a connection".to_string() } else { self.connection.clone() };
                egui::ComboBox::from_id_salt("pg-connection").selected_text(shown).width(300.0).show_ui(ui, |ui| {
                    for c in &settings.connections {
                        let label = format!("{}  ·  {} on {}", c.name, c.database, if c.host.is_empty() { "localhost" } else { &c.host });
                        ui.selectable_value(&mut self.connection, c.name.clone(), label);
                    }
                });
                if ui.button("Connections…").on_hover_text("Add or change connections in the settings").clicked() {
                    action = Some(DbAction::Settings);
                }
                if !self.connection.is_empty() && ui.add_enabled(!self.busy(), egui::Button::new("List again")).clicked() {
                    self.connect(backend.clone(), settings);
                }
            });
            if settings.connections.is_empty() {
                ui.label(RichText::new("There are no connections yet. Click Connections… to add one.").color(pal.warn));
            } else if !self.connection.is_empty() && settings.connection(&self.connection).is_none() {
                ui.label(
                    RichText::new(format!("There is no connection called {} on this computer. Choose another, or add it under Connections….", self.connection))
                        .color(pal.warn),
                );
            }
            // One line that says how it goes, always there, so that nothing below it moves
            // when the answer comes.
            ui.horizontal(|ui| {
                ui.set_min_height(22.0);
                if self.busy() {
                    ui.spinner();
                    ui.label(RichText::new("Asking the database for its tables…").color(pal.muted));
                } else {
                    match &self.tables {
                        Some(Ok(l)) => ui.label(
                            RichText::new(format!("Connected. {} {} with geometry.", l.len(), if l.len() == 1 { "table" } else { "tables" })).color(pal.muted),
                        ),
                        Some(Err(_)) => ui.label(RichText::new("The database could not be reached:").color(pal.err)),
                        None => ui.label(""),
                    };
                }
            });
            if let Some(Err(e)) = &self.tables {
                ui.label(RichText::new(e).color(pal.err));
            }
            ui.add_space(6.0);
            let list: Vec<LayerInfo> = match &self.tables {
                Some(Ok(l)) => l.clone(),
                _ => Vec::new(),
            };
            if self.output {
                self.write_part(ui, pal, &list);
            } else {
                self.read_part(ui, pal, &list);
            }
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let ready =
                        settings.connection(&self.connection).is_some() && if self.output { !self.table.trim().is_empty() } else { !self.ticked.is_empty() };
                    let ok = if self.output {
                        "Write to this table".to_string()
                    } else {
                        match self.ticked.len() {
                            0 => "Read the tables".to_string(),
                            1 => "Read 1 table".to_string(),
                            n => format!("Read {} tables", n),
                        }
                    };
                    if ui.add_enabled(ready, egui::Button::new(ok)).clicked() {
                        action = Some(DbAction::Use);
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(DbAction::Cancel);
                    }
                });
            });
        });
        if modal.should_close() && action.is_none() {
            action = Some(DbAction::Cancel);
        }
        action
    }

    /// The tables to read: a search field and a tick for each.
    fn read_part(&mut self, ui: &mut egui::Ui, pal: &Palette, list: &[LayerInfo]) {
        if list.is_empty() {
            if self.tables.is_none() && !self.busy() {
                ui.label(RichText::new("Connect, and the tables with geometry are listed here to choose from.").color(pal.muted));
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Tables").color(pal.muted));
            ui.add(TextEdit::singleline(&mut self.search).desired_width(220.0).hint_text("Search"));
            let shown: Vec<String> = list.iter().filter(|l| self.matches(&l.name)).map(|l| l.name.clone()).collect();
            if ui.small_button("Tick those shown").clicked() {
                for n in shown {
                    if !self.ticked.contains(&n) {
                        self.ticked.push(n);
                    }
                }
            }
            if ui.small_button("None").clicked() {
                self.ticked.clear();
            }
        });
        egui::ScrollArea::vertical().id_salt("pg-tables").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
            let shown: Vec<&LayerInfo> = list.iter().filter(|l| self.matches(&l.name)).collect();
            for l in shown {
                let mut on = self.ticked.contains(&l.name);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, &l.name).changed() {
                        if on {
                            self.ticked.push(l.name.clone());
                        } else {
                            self.ticked.retain(|t| t != &l.name);
                        }
                    }
                    let count = l.count.map_or(String::new(), |c| format!("{} · ", og_engine::model::fmt_count(c)));
                    let crs = l.crs.map_or(String::new(), |c| format!(" · EPSG:{}", c));
                    ui.label(RichText::new(format!("{}{}{}", count, l.geometry.to_lowercase(), crs)).font(theme::mono(10.5)).color(pal.muted));
                });
            }
        });
        // In the order of the list, which is the order they are read in.
        let order: Vec<String> = list.iter().map(|l| l.name.clone()).collect();
        self.ticked.sort_by_key(|t| order.iter().position(|o| o == t).unwrap_or(usize::MAX));
        if self.ticked.len() > 1 {
            ui.label(RichText::new("Several tables become one stream, with the name of each table in the attribute table.").small().color(pal.muted));
        }
    }

    fn matches(&self, name: &str) -> bool {
        let s = self.search.trim().to_lowercase();
        s.is_empty() || name.to_lowercase().contains(&s)
    }

    /// Where to write: a schema and a table, new or one that is there.
    fn write_part(&mut self, ui: &mut egui::Ui, pal: &Palette, list: &[LayerInfo]) {
        let mut schemas: Vec<String> = list.iter().map(|l| Self::split(&l.name).0).collect();
        if !schemas.contains(&"public".to_string()) {
            schemas.push("public".to_string());
        }
        schemas.sort();
        schemas.dedup();
        egui::Grid::new("pg-target").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new("Schema").color(pal.muted));
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.schema).desired_width(200.0));
                if !list.is_empty() {
                    egui::ComboBox::from_id_salt("pg-schemas").selected_text("Choose").width(120.0).show_ui(ui, |ui| {
                        for s in &schemas {
                            if ui.selectable_label(&self.schema == s, s).clicked() {
                                self.schema = s.clone();
                            }
                        }
                    });
                }
            });
            ui.end_row();
            ui.label(RichText::new("Table").color(pal.muted));
            ui.add(TextEdit::singleline(&mut self.table).desired_width(400.0).hint_text("a new name, or one of those below"));
            ui.end_row();
        });
        let laundered = launder(&self.table);
        if !self.table.trim().is_empty() && laundered != self.table.trim() {
            ui.label(RichText::new(format!("Written as {}, as PostgreSQL prefers its names.", laundered)).small().color(pal.muted));
        }
        let there: Vec<&LayerInfo> = list.iter().filter(|l| Self::split(&l.name).0 == self.schema.trim()).collect();
        if there.iter().any(|l| Self::split(&l.name).1 == laundered) {
            ui.label(RichText::new("The table is there. It is replaced, or added to if the form says so.").small().color(pal.warn));
        }
        if !there.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Tables in {}", self.schema.trim())).small().color(pal.muted));
            egui::ScrollArea::vertical().id_salt("pg-there").max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                for l in there {
                    let name = Self::split(&l.name).1;
                    ui.horizontal(|ui| {
                        if ui.selectable_label(name == laundered, &name).clicked() {
                            self.table = name.clone();
                        }
                        ui.label(RichText::new(l.geometry.to_lowercase()).font(theme::mono(10.5)).color(pal.muted));
                    });
                }
            });
        }
    }
}

/// A name as it is written to the database: lower case, with spaces and dashes as _.
pub fn launder(s: &str) -> String {
    s.trim().to_lowercase().chars().map(|c| if matches!(c, ' ' | '-' | '#' | '.') { '_' } else { c }).collect()
}
