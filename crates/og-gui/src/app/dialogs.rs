//! The windows that open over the program: restoring changes, unsaved changes, layers to
//! read, About, PostGIS, settings, variables and tips.

use super::*;

impl App {
    /// Offers changes left behind by a program that stopped, one at a time.
    pub(super) fn recovery_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if self.pending.is_some() {
            return;
        }
        let Some(found) = self.recoverable.first().cloned() else { return };
        let mut choice = None;
        egui::Modal::new(egui::Id::new("recovery")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Restore changes that were not saved?").font(theme::strong(16.0)));
            ui.label(RichText::new("Ographis stopped without being closed, and kept these changes:").color(pal.muted));
            ui.add_space(6.0);
            ui.label(RichText::new(&found.name).strong());
            let place = found.path.as_ref().map_or("A model that was never saved".to_string(), |p| p.display().to_string());
            ui.label(RichText::new(place).font(theme::mono(11.5)).color(pal.muted));
            ui.label(RichText::new(format!("Kept {}", found.saved)).color(pal.muted));
            if self.dirty {
                ui.add_space(4.0);
                ui.label(RichText::new("The model that is open now has changes too: save it first if you want to keep them.").color(pal.warn));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Restore").clicked() {
                    choice = Some(true);
                }
                if ui.button("Throw away").clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                self.recoverable.remove(0);
                match self.restore_kept(&found) {
                    Ok(()) => self.say(format!("Restored {}. Save it to keep it.", found.name)),
                    Err(e) => self.say(format!("Could not restore {}: {}", found.name, e)),
                }
            }
            Some(false) => {
                self.recoverable.remove(0);
                crate::recovery::forget(&found);
            }
            None => {}
        }
    }

    pub(super) fn unsaved_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if self.pending.is_none() {
            return;
        }
        let mut choice = None;
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Save the changes?").font(theme::strong(16.0)));
            ui.label(RichText::new(format!("The model {} has changes that are not saved.", self.name)).color(pal.muted));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                if self.save(false) {
                    if let Some(a) = self.pending.take() {
                        self.perform(a);
                    }
                }
            }
            Some(1) => {
                self.dirty = false;
                if let Some(a) = self.pending.take() {
                    self.perform(a);
                }
            }
            Some(2) => self.pending = None,
            _ => {}
        }
    }

    /// Asks which layers to read from a file that has several: the tables of a GeoPackage
    /// or the sheets of a workbook.
    pub(super) fn layer_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if self.pending.is_some() {
            return;
        }
        let Some(choice) = self.layer_choices.front_mut() else { return };
        let mut answer = None;
        egui::Modal::new(egui::Id::new("layers")).show(ctx, |ui| {
            ui.set_width(440.0);
            let name = choice.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let what = if params::is_spreadsheet(&sources::ext_of(&choice.file)) { "sheets" } else { "layers" };
            ui.label(RichText::new(format!("{} holds {} {}", name, choice.layers.len(), what)).font(theme::strong(16.0)));
            ui.label(RichText::new("Tick the ones to read.").color(pal.muted));
            ui.add_space(4.0);
            ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                for (layer, on) in choice.layers.iter().zip(choice.chosen.iter_mut()) {
                    ui.horizontal(|ui| {
                        ui.checkbox(on, &layer.name);
                        let count = layer.count.map_or(String::new(), |c| format!("{} · ", c));
                        ui.label(RichText::new(format!("{}{}", count, layer.geometry.to_lowercase())).font(theme::mono(11.0)).color(pal.muted));
                    });
                }
            });
            ui.horizontal(|ui| {
                if ui.small_button("Tick all").clicked() {
                    choice.chosen.iter_mut().for_each(|c| *c = true);
                }
                if ui.small_button("Tick none").clicked() {
                    choice.chosen.iter_mut().for_each(|c| *c = false);
                }
            });
            let n = choice.chosen.iter().filter(|c| **c).count();
            ui.add_space(6.0);
            ui.add_enabled_ui(n > 1, |ui| {
                ui.radio_value(&mut choice.one_stream, false, if what == "sheets" { "An input for each sheet" } else { "An input for each layer" });
                ui.radio_value(&mut choice.one_stream, true, "One input for all of them, as one stream");
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = match n {
                    0 => "Add".to_string(),
                    1 => "Add 1 input".to_string(),
                    _ if choice.one_stream => format!("Add 1 input for {} {}", n, what),
                    _ => format!("Add {} inputs", n),
                };
                if ui.add_enabled(n > 0, egui::Button::new(label)).clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(add) = answer {
            if let Some(choice) = self.layer_choices.pop_front() {
                if add {
                    self.add_layer_readers(&choice);
                }
            }
        }
    }

    /// The runs of the saved model: from the program and from the command line, the latest
    /// first, with the lock if it is running.
    pub(super) fn history_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.history_open {
            return;
        }
        let Some(path) = self.path.clone() else {
            self.history_open = false;
            return;
        };
        let runs = og_engine::history::history(&path);
        let holder = og_engine::history::holder(&path, 24.0);
        let mut close = false;
        let mut show: Option<std::path::PathBuf> = None;
        let mut unlock = false;
        let modal = egui::Modal::new(egui::Id::new("run-history")).show(ctx, |ui| {
            ui.set_width(900.0);
            ui.label(RichText::new(format!("Run history · {}", self.name)).font(theme::strong(18.0)).color(pal.ink));
            ui.label(RichText::new(format!("Kept in {}", og_engine::history::runs_dir(&path).display())).small().color(pal.muted));
            ui.add_space(6.0);
            if let Some(h) = &holder {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Running now, {}.", h.describe())).color(pal.accent));
                    if ui.button("Remove lock").on_hover_text("Only when that run has crashed and left its lock behind").clicked() {
                        unlock = true;
                    }
                });
                ui.add_space(4.0);
            }
            if runs.is_empty() {
                ui.label(RichText::new("No runs yet. Each run from here and from og run is kept.").color(pal.muted));
            } else {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("runs").num_columns(8).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                        for h in ["Started", "Seconds", "Outcome", "Written", "Files", "By", "Where", "Log and message"] {
                            ui.label(RichText::new(h).font(theme::strong(12.5)));
                        }
                        ui.end_row();
                        for r in runs.iter().take(500) {
                            ui.label(&r.started);
                            ui.label(format!("{}", r.seconds));
                            let colour = match r.outcome.as_str() {
                                "errors" => pal.err,
                                "warnings" | "stopped" => pal.warn,
                                _ => pal.ok,
                            };
                            ui.label(RichText::new(&r.outcome).color(colour));
                            ui.label(r.written.to_string());
                            ui.label(r.files.to_string());
                            ui.label(&r.by);
                            ui.label(format!("{}{}{}", r.user, if r.user.is_empty() { "" } else { " on " }, r.computer));
                            ui.horizontal(|ui| {
                                if !r.log.is_empty() && ui.small_button("Log").on_hover_text(&r.log).clicked() {
                                    show = Some(og_engine::history::runs_dir(&path).join("logs").join(&r.log));
                                }
                                let lineage = og_engine::history::runs_dir(&path).join("lineage").join(format!("{}.json", r.started.replace([':', ' '], "-")));
                                if lineage.is_file()
                                    && ui
                                        .small_button("Lineage")
                                        .on_hover_text("Where the data came from and went: the files, addresses and tables read and written, with their sizes and times, and the model as it was")
                                        .clicked()
                                {
                                    show = Some(lineage);
                                }
                                if !r.message.is_empty() {
                                    let short: String = r.message.chars().take(70).collect();
                                    ui.label(RichText::new(short).small().color(pal.muted)).on_hover_text(&r.message);
                                }
                            });
                            ui.end_row();
                        }
                    });
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Open the folder").clicked() {
                    show = Some(og_engine::history::runs_dir(&path).join("history.csv"));
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
        if unlock {
            og_engine::history::unlock(&path);
        }
        if let Some(p) = show {
            self.show_in_folder(&p);
        }
        if close || modal.should_close() {
            self.history_open = false;
        }
    }

    pub fn open_schedule(&mut self) {
        self.schedule = Some(ScheduleForm {
            name: self.name.clone(),
            every: og_engine::schedule::Every::Weekday,
            time: "06:00".into(),
            day: "MON".into(),
            hours: 1,
            date: 1,
            answer: None,
        });
    }

    /// Runs schtasks with these arguments, and gives whether it went well and what it said.
    fn schtasks(args: &[String]) -> (bool, String) {
        let mut cmd = std::process::Command::new("schtasks");
        cmd.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.output() {
            Ok(o) => {
                let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string();
                (o.status.success(), text)
            }
            Err(e) => (false, format!("schtasks could not be started: {}", e)),
        }
    }

    /// Model → Schedule: when the saved model runs by itself, through the Task Scheduler.
    pub(super) fn schedule_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        use og_engine::schedule::{self, Every};
        let Some(mut form) = self.schedule.take() else { return };
        let Some(path) = self.path.clone() else { return };
        let command = self.command_line().unwrap_or_default();
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("schedule")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new(format!("Schedule · {}", self.name)).font(theme::strong(18.0)).color(pal.ink));
            ui.label(
                RichText::new("The saved model runs by itself, through the Task Scheduler of Windows, while you are logged in. Each run is kept in Model → Run history, with its log and lineage.")
                    .color(pal.muted),
            );
            ui.add_space(8.0);
            egui::Grid::new("schedule-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Name of the task");
                ui.text_edit_singleline(&mut form.name);
                ui.end_row();
                ui.label("Runs");
                ui.horizontal_wrapped(|ui| {
                    let kinds = [("day", "Every day"), ("weekday", "Every weekday"), ("week", "Every week"), ("month", "Every month"), ("hours", "Every few hours")];
                    let now = match form.every {
                        Every::Day => "day",
                        Every::Weekday => "weekday",
                        Every::Week(_) => "week",
                        Every::Month(_) => "month",
                        Every::Hours(_) => "hours",
                    };
                    for (k, label) in kinds {
                        if ui.selectable_label(now == k, label).clicked() {
                            form.every = match k {
                                "day" => Every::Day,
                                "weekday" => Every::Weekday,
                                "week" => Every::Week(form.day.clone()),
                                "month" => Every::Month(form.date),
                                _ => Every::Hours(form.hours),
                            };
                        }
                    }
                });
                ui.end_row();
                match &mut form.every {
                    Every::Week(d) => {
                        ui.label("On");
                        egui::ComboBox::from_id_salt("schedule-day").selected_text(schedule::DAYS.iter().find(|x| x.0 == d).map_or("", |x| x.1)).show_ui(ui, |ui| {
                            for (k, label) in schedule::DAYS {
                                ui.selectable_value(d, k.to_string(), *label);
                            }
                        });
                        form.day = d.clone();
                        ui.end_row();
                    }
                    Every::Month(n) => {
                        ui.label("On day");
                        ui.add(egui::DragValue::new(n).range(1..=28));
                        form.date = *n;
                        ui.end_row();
                    }
                    Every::Hours(n) => {
                        ui.label("Every");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(n).range(1..=23));
                            ui.label("hours");
                        });
                        form.hours = *n;
                        ui.end_row();
                    }
                    _ => {}
                }
                ui.label(if matches!(form.every, Every::Hours(_)) { "From" } else { "At" });
                ui.add(egui::TextEdit::singleline(&mut form.time).desired_width(70.0).hint_text("06:00"));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new(format!("It runs: {}", command)).small().color(pal.muted));
            if self.dirty {
                ui.label(RichText::new("The model has changes that are not saved: the scheduled run runs what is saved.").color(pal.warn));
            }
            if let Some((ok, text)) = &form.answer {
                ui.add_space(4.0);
                ui.label(RichText::new(text).color(if *ok { pal.ok } else { pal.err }));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if cfg!(windows) {
                    if ui.button("Create the scheduled run").clicked() {
                        let script = schedule::script_path(&path);
                        let made = script
                            .parent()
                            .map(std::fs::create_dir_all)
                            .unwrap_or(Ok(()))
                            .and_then(|_| std::fs::write(&script, schedule::script(&path, &command)));
                        form.answer = Some(match made {
                            Err(e) => (false, format!("Cannot write {}: {}", script.display(), e)),
                            Ok(()) => match schedule::create_args(&form.name, &form.every, &form.time, &script) {
                                Err(e) => (false, e),
                                Ok(args) => {
                                    let (ok, text) = Self::schtasks(&args);
                                    if ok {
                                        (true, format!("Made: {} in the Task Scheduler, under Ographis. It starts {}.", schedule::task_name(&form.name), script.display()))
                                    } else {
                                        (false, text)
                                    }
                                }
                            },
                        });
                    }
                    if ui.button("Remove it").on_hover_text("Removes the task of this name from the Task Scheduler").clicked() {
                        let (ok, text) = Self::schtasks(&schedule::delete_args(&form.name));
                        form.answer = Some((ok, if ok { format!("{} is removed.", schedule::task_name(&form.name)) } else { text }));
                    }
                    if ui.button("Is it there?").clicked() {
                        form.answer = Some(Self::schtasks(&schedule::query_args(&form.name)));
                    }
                } else if let Some(line) = schedule::cron_line(&form.every, &form.time, &command) {
                    if ui.button("Copy a line for crontab").on_hover_text(&line).clicked() {
                        ctx.copy_text(line);
                        form.answer = Some((true, "Copied. Add it with crontab -e.".into()));
                    }
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
        if !(close || modal.should_close()) {
            self.schedule = Some(form);
        }
    }

    pub(super) fn about_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.about {
            return;
        }
        let tab_id = egui::Id::new("about-tab");
        let mut parts = ctx.data(|d| d.get_temp::<bool>(tab_id)).unwrap_or(false);
        let modal = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
            ui.set_width(if parts { 620.0 } else { 380.0 });
            ui.horizontal(|ui| {
                if ui.selectable_label(!parts, "About").clicked() {
                    parts = false;
                }
                if ui.selectable_label(parts, "Built with").clicked() {
                    parts = true;
                }
            });
            ui.add_space(6.0);
            if !parts {
                ui.label(RichText::new("Ographis").font(theme::strong(24.0)).color(pal.ink));
                ui.label(RichText::new(format!("Version {}", App::version())).font(theme::strong(15.0)));
                ui.label(RichText::new(format!("Built {}", env!("OGRAPHIS_BUILT"))).color(pal.muted));
                ui.add_space(4.0);
                ui.label(RichText::new("Spatial ETL for the desktop: inputs, tools and outputs that you connect on a canvas.").color(pal.muted));
                ui.add_space(4.0);
                ui.label(RichText::new(format!("File formats and coordinate systems through {}.", self.backend.describe())).color(pal.muted));
                ui.label(RichText::new("Licensed under MIT or Apache 2.0, at your option.").color(pal.muted));
                ui.hyperlink_to("github.com/gishubio/Ographis", env!("CARGO_PKG_REPOSITORY"));
            } else {
                ui.label(
                    RichText::new("Ographis is built on these, with thanks. All libraries and their licences are in THIRD-PARTY-LICENSES.md.").color(pal.muted),
                );
                ui.add_space(6.0);
                let versions = self.backend.versions();
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    egui::Grid::new("components").num_columns(4).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                        for h in ["Part", "Version", "What for", "Licence"] {
                            ui.label(RichText::new(h).font(theme::strong(12.5)));
                        }
                        ui.end_row();
                        for (name, what, licence) in COMPONENTS {
                            let version =
                                versions.iter().find(|v| v.0 == *name).map(|v| v.1.clone()).or_else(|| crate_version(name)).unwrap_or_else(|| "–".into());
                            ui.label(RichText::new(*name).font(theme::mono(12.0)));
                            ui.label(RichText::new(version).font(theme::mono(12.0)).color(pal.muted));
                            ui.label(RichText::new(*what).color(pal.muted));
                            ui.label(RichText::new(*licence).color(pal.muted));
                            ui.end_row();
                        }
                        ui.label(RichText::new("Rust").font(theme::mono(12.0)));
                        ui.label(RichText::new(env!("OGRAPHIS_RUSTC")).font(theme::mono(12.0)).color(pal.muted));
                        ui.label(RichText::new("The language and its compiler").color(pal.muted));
                        ui.label(RichText::new("MIT OR Apache-2.0").color(pal.muted));
                        ui.end_row();
                    });
                });
            }
            ui.add_space(10.0);
            if ui.button("Close").clicked() {
                self.about = false;
            }
        });
        ctx.data_mut(|d| d.insert_temp(tab_id, parts));
        if modal.should_close() {
            self.about = false;
        }
    }

    /// What the panel on the right says when nothing is selected.
    pub(super) fn intro(&self, ui: &mut Ui, pal: &Palette) {
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width() - 10.0);
            ui.label(RichText::new("Work with geodata").font(theme::strong(17.0)));
            ui.label(RichText::new(INTRO).color(pal.muted));
            ui.add_space(6.0);
            let list = |ui: &mut Ui, heading: &str, items: &mut dyn Iterator<Item = &str>| {
                ui.label(RichText::new(heading).font(theme::strong(13.5)));
                // One label per line, which wraps in a narrow panel: labels side by side would
                // keep their width and widen the panel.
                for item in items {
                    ui.add(egui::Label::new(RichText::new(format!("·  {}", item)).color(pal.muted)).wrap());
                }
                ui.add_space(6.0);
            };
            list(ui, "Reads", &mut READ_FORMATS.iter().copied());
            list(ui, "Writes", &mut WRITER_FORMATS.iter().filter(|f| !f.0.is_empty()).map(|f| f.1));
            ui.label(RichText::new(START).color(pal.muted));
        });
    }

    /// Opens the window that connects a node to PostGIS: an input to read tables, or an
    /// output to write one. It is filled in from the node, or from another node that uses
    /// PostGIS.
    pub fn open_postgis(&mut self, node_id: NodeId, output: bool, revert_format: Option<String>) {
        let n = &self.snarl[node_id];
        let other = self
            .snarl
            .nodes()
            .find(|o| matches!(o.kind.as_str(), "PostgisReader" | "PostgisWriter") && o.params.contains_key("connection"))
            .map(|o| o.params.clone());
        let mut d = crate::dbdialog::DbDialog::new(n.id.clone(), output, &n.params, other.as_ref(), &self.settings);
        d.revert_format = revert_format;
        self.db_dialog = Some(d);
        // Without connections there is nothing to choose: the settings open over the window.
        if self.settings.connections.is_empty() {
            self.open_settings();
        }
    }

    pub(super) fn postgis_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut d) = self.db_dialog.take() else { return };
        if let Some((conn, found)) = d.poll() {
            self.lookup.put_db_layers(&conn, found);
        }
        // While the settings are open over it, the window waits.
        if self.settings_window.is_some() {
            self.db_dialog = Some(d);
            return;
        }
        let action = d.show(ctx, pal, &self.backend, &self.settings);
        let Some(node_id) = graph::find(&self.snarl, &d.node) else { return };
        match action {
            None => self.db_dialog = Some(d),
            Some(crate::dbdialog::DbAction::Settings) => {
                self.db_dialog = Some(d);
                self.open_settings();
            }
            Some(crate::dbdialog::DbAction::Cancel) => {
                // An output that was switched to PostGIS in its format goes back to its format.
                if let Some(f) = d.revert_format {
                    self.snarl[node_id].params.insert("format".into(), serde_json::Value::String(f));
                }
            }
            Some(crate::dbdialog::DbAction::Use) => {
                let kind = if d.output { "PostgisWriter" } else { "PostgisReader" };
                self.switch_kind(node_id, kind);
                let n = &mut self.snarl[node_id];
                for k in crate::dbdialog::CONNECTION_KEYS {
                    n.params.remove(*k);
                }
                n.params.insert("connection".into(), serde_json::Value::String(d.connection.clone()));
                if d.output {
                    n.params.insert("schema".into(), serde_json::Value::String(crate::dbdialog::launder(&d.schema)));
                    n.params.insert("table".into(), serde_json::Value::String(crate::dbdialog::launder(&d.table)));
                    n.params.insert("format".into(), serde_json::Value::String("postgis".into()));
                } else {
                    n.params.insert("tables".into(), serde_json::Value::Array(d.ticked.iter().cloned().map(serde_json::Value::String).collect()));
                    // A query would be read instead of the tables.
                    n.params.remove("sql");
                }
                self.changed(true);
            }
        }
    }

    pub fn open_settings(&mut self) {
        self.settings_window = Some(crate::settings::SettingsWindow::new(&self.settings));
    }

    pub(super) fn settings_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut w) = self.settings_window.take() else { return };
        let (close, changed) = w.show(ctx, pal, &mut self.settings, &self.backend);
        if changed {
            // Saved at once, and used by runs at once.
            w.save_error = self.settings.save().err().map(|e| e.0);
            self.stale = true;
        }
        if !close {
            self.settings_window = Some(w);
        }
    }

    /// Makes a node another tool, keeping its connections, its name if the user gave it one,
    /// and the settings the new tool has too. Inputs and outputs switch between a file, a
    /// GeoSpatial PDF and PostGIS this way.
    pub fn switch_kind(&mut self, node_id: NodeId, kind: &str) {
        let Some(ty) = og_engine::node_type(kind) else { return };
        let n = &mut self.snarl[node_id];
        if n.kind == kind {
            return;
        }
        if graph::has_default_name(n) {
            n.name = ty.title.to_string();
        }
        // Only what the new tool has, and for a list only a value that is in its list.
        n.params.retain(|k, v| {
            ty.params.iter().any(|p| {
                p.key == k
                    && match p.kind {
                        og_engine::nodes::ParamKind::Choice(list) => v.as_str().is_some_and(|v| list.iter().any(|c| c.0 == v)),
                        _ => true,
                    }
            })
        });
        n.kind = kind.to_string();
        self.touch();
        self.stale = true;
    }

    /// The description of the model: what it does, in a few lines. Changes are made at once
    /// and can be undone.
    pub(super) fn description_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.description_open {
            return;
        }
        let mut text = self.description.clone();
        let modal = egui::Modal::new(egui::Id::new("description")).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.label(RichText::new("Description").font(theme::strong(17.0)).color(pal.ink));
            ui.label(
                RichText::new(
                    "What the model does, where its data comes from and what it writes. Shown when the model is opened, and in the picture of the model.",
                )
                .color(pal.muted),
            );
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .id_salt("description-text")
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .hint_text("Such as: Reads the stops of Västtrafik every night and writes those in Göteborg to PostGIS."),
            );
            ui.add_space(6.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.description_open = false;
                }
            });
        });
        if modal.should_close() {
            self.description_open = false;
        }
        if text != self.description {
            self.description = text;
            self.touch();
        }
    }

    /// The variables of the model: a name and a value on each row. Changes are made at once,
    /// and can be undone like other changes.
    pub(super) fn variables_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.variables_open {
            return;
        }
        let mut vars = self.variables.clone();
        let modal = egui::Modal::new(egui::Id::new("variables")).show(ctx, |ui| {
            ui.set_width(720.0);
            ui.label(RichText::new("Variables").font(theme::strong(17.0)).color(pal.ink));
            ui.label(
                RichText::new("Named values for the settings of the model. Write $(name) in a path, an expression or any other setting, and the value is put in its place when the model runs. From the command line: og run model.json --var name=value")
                    .color(pal.muted),
            );
            ui.add_space(6.0);
            let mut remove = None;
            // A row for each variable: the name, wide enough to read, and the value in what is left.
            if !vars.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_sized([240.0, 16.0], egui::Label::new(RichText::new("Name").small().color(pal.muted)));
                    ui.label(RichText::new("Value").small().color(pal.muted));
                });
            }
            for (i, v) in vars.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut v.name).id_salt(("var-name", i)).desired_width(236.0).font(theme::mono(12.5)).hint_text("name"));
                    let secret = og_engine::run::is_secret(&v.name);
                    let w = (ui.available_width() - 30.0).max(120.0);
                    ui.add(egui::TextEdit::singleline(&mut v.value).id_salt(("var-value", i)).password(secret).desired_width(w).hint_text("value"));
                    if ui.small_button("×").on_hover_text("Remove this variable").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                vars.remove(i);
            }
            // What is wrong with the names, said under the list.
            let mut problems = Vec::new();
            for (i, v) in vars.iter().enumerate() {
                let name = v.name.trim();
                if name.is_empty() {
                    continue;
                }
                if !og_engine::run::is_variable_name(name) {
                    problems.push(format!("\"{}\" cannot be a name. Use letters, digits and _, and start with a letter.", name));
                } else if vars[..i].iter().any(|w| w.name.trim() == name) {
                    problems.push(format!("There are two variables called {}. The first one is used.", name));
                }
            }
            for p in problems {
                ui.colored_label(pal.err, RichText::new(p).small());
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Add variable").clicked() {
                    vars.push(og_engine::Variable::default());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.variables_open = false;
                    }
                });
            });
        });
        if modal.should_close() {
            self.variables_open = false;
        }
        if vars != self.variables {
            self.variables = vars;
            self.changed(true);
        }
    }

    /// How the program is worked, for the one who is new to it. Reached from the Help menu.
    pub(super) fn tips_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.tips {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("tips")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("Tips and shortcuts").font(theme::strong(17.0)).color(pal.ink));
            ui.add_space(4.0);
            // The list scrolls when the window is low, so that Close is always there.
            // A fixed height, so that the window does not change size while it opens.
            let room = (ctx.content_rect().height() - 160.0).clamp(200.0, 440.0);
            ScrollArea::vertical().max_height(room).min_scrolled_height(room).auto_shrink([false, false]).show(ui, |ui| {
                for line in TIPS {
                    ui.label(RichText::new(*line).color(pal.muted));
                    ui.add_space(2.0);
                }
            });
            ui.add_space(10.0);
            if ui.button("Close").clicked() {
                self.tips = false;
            }
        });
        if modal.should_close() {
            self.tips = false;
        }
    }
}

/// What the window with tips says. One line for each thing that is not found by looking.
pub(super) const TIPS: &[&str] = &[
    "Fold out a list on the left and click an input, tool or output to add it to the canvas. You can also drop a data file or a folder on the window to get an input for it.",
    "Drag from an output port to an input port to connect. Drop the connection on empty canvas to choose a new node.",
    "Drag a tool onto a connection to put it in between. Click a connection and press Delete to remove it.",
    "Click a node to set it up on the right. Click an output port to see its data in the table and on the map.",
    "Drag on empty canvas to select several nodes. Shift adds to the selection. Selected nodes move together, and so do the group boxes around them. Ctrl+A selects everything.",
    "Ctrl+G puts a group box around the selected nodes. Move the box by its name and the nodes follow.",
    "Scroll to zoom. Hold Space and drag, or drag with the middle mouse button, to move the canvas.",
    "Right-click a node to disable, duplicate or delete it, to run the model up to it, or from it with Model → Use cache. Right-click a port to remove its connections.",
    "F5 runs the model, F6 pauses and resumes it, and Shift+F5 stops it. Ctrl+Z takes back the latest change. Delete removes what is selected.",
    "Alt and the first letter of a menu, or F10, opens the menus from the keyboard. The arrows move in them, Enter chooses and Esc closes.",
    "Model → Copy command line gives the command that runs the model without the window, for a script or a scheduled task.",
    "Model → Run history shows each run of the model, from here and from og run, with its log. A model that is running cannot be started a second time.",
    "Ctrl+C and Ctrl+V copy nodes, also to another window of Ographis. They land where the pointer is.",
    "A terrain model read with Cells as set to a grid can be shaded from above with Shade terrain, or seen from the side with Quick terrain view.",
    "Edit → Settings (Ctrl+,) holds the database connections. PostGIS… on an Input reads tables from one, and the format PostGIS table of an Output writes to one. The model keeps only the name of the connection.",
    "Drag from a connected input to move its connection to another input. Right-click a connection to disable it, so that nothing goes through, or to delete it.",
    "Web addresses in comments and notes can be clicked. A note is resized from any edge or corner, as a comment.",
    "An input reads JSON from a file or a web address. Flatten JSON, Explode JSON list and Extract JSON values make it a plain table.",
    "The format GeoSpatial PDF with layers of Output writes a PDF where each connected stream is a layer that can be turned on and off.",
    "Draw chart makes a bar chart of an attribute per name, such as the area of each municipality, to write as PNG or PDF.",
    "Under Values in a layer of Draw map, points get a size by a number, and points, lines and areas a colour by a value. Areas coloured by a number make a choropleth map. Legend puts a key to it in a corner.",
    "Read a building as an OBJ or glTF file and connect it to Render 3D scene for a picture with sun, shadows and materials. Set the sun from a date and a time for shadow studies.",
    "Tick 3D above the map to see heights from an angle, and drag to turn it. Set height gives features their heights.",
    "The secondary button on a connection adds a bend point, to drag where the connection should go.",
    "Read a GeoTIFF with Cells as an image: point at it on the map for the value of a cell, in every band.",
    "Make zip file packs what the outputs wrote, with other files you list, after everything else has run.",
    "An attribute in braces in the file name of an output, such as out/{municipality}.gpkg, writes a file for each value. In the layer, a table or a sheet for each.",
    "Tanaka contours draws lit contour lines from a terrain model: light on the slopes towards the light, dark on those away from it.",
    "Statistics above the table in the Data tab sums up each attribute: values, empty, unique, smallest, largest, mean and sum.",
];
/// The parts Ographis is built on, for the About window: (name, what for, licence). The
/// versions of the crates come from Cargo.lock when the program is built, and those of GDAL,
/// PROJ and GEOS from the libraries themselves.
pub(super) const COMPONENTS: &[(&str, &str, &str)] = &[
    ("GDAL", "Reads and writes the file formats and web services", "MIT"),
    ("PROJ", "Coordinate systems and reprojection", "MIT"),
    ("GEOS", "Geometry operations inside GDAL", "LGPL-2.1"),
    ("gdal", "GDAL from Rust (GeoRust)", "MIT"),
    ("egui", "The user interface, drawn on the graphics card", "MIT OR Apache-2.0"),
    ("eframe", "The window around egui", "MIT OR Apache-2.0"),
    ("egui-snarl", "The canvas with nodes and connections, with a small change", "MIT OR Apache-2.0"),
    ("geo", "Geometry: areas, lengths, buffers, overlays (GeoRust)", "MIT OR Apache-2.0"),
    ("rstar", "The spatial index (GeoRust)", "MIT OR Apache-2.0"),
    ("spade", "Delaunay triangles and Thiessen polygons", "MIT OR Apache-2.0"),
    ("wkt", "Geometry as text (GeoRust)", "MIT OR Apache-2.0"),
    ("tiny-skia", "Draws maps and charts", "BSD-3-Clause"),
    ("ab_glyph", "Text in maps and charts, in the Inter font (OFL-1.1)", "Apache-2.0"),
    ("rayon", "Work spread over all cores", "MIT OR Apache-2.0"),
    ("ureq", "Web requests", "MIT OR Apache-2.0"),
    ("umya-spreadsheet", "Excel workbooks", "MIT"),
    ("spreadsheet-ods", "LibreOffice Calc workbooks", "MIT/Apache-2.0"),
    ("zip", "Zip files", "MIT"),
    ("acadrust", "DWG drawings", "MPL-2.0"),
    ("parquet", "GeoParquet (Apache Arrow)", "Apache-2.0"),
    ("las", "Point clouds, LAS and LAZ", "MIT"),
    ("serde_json", "JSON and the model files", "MIT OR Apache-2.0"),
    ("chrono", "Dates and times", "MIT OR Apache-2.0"),
    ("encoding_rs", "Text in older encodings, such as Windows-1252", "(Apache-2.0 OR MIT) AND BSD-3-Clause"),
    ("image", "PNG and JPEG pictures", "MIT OR Apache-2.0"),
    ("rfd", "The windows for opening and saving files", "MIT"),
    ("arboard", "The clipboard", "MIT OR Apache-2.0"),
];
/// The version of a crate as Cargo.lock had it when the program was built.
pub(super) fn crate_version(name: &str) -> Option<String> {
    env!("OGRAPHIS_CRATES").split(';').find_map(|nv| nv.split_once('=').filter(|(n, _)| *n == name).map(|(_, v)| v.to_string()))
}
