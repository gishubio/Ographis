//! The window as a whole: the panels, the order they are drawn in, and the keyboard shortcuts.

use super::*;

/// Tells that a long run has finished: the button in the taskbar flashes, and Windows shows a
/// notice. Elsewhere only the flash.
fn tell_finished(ctx: &egui::Context, text: &str) {
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
    #[cfg(not(windows))]
    let _ = text;
    #[cfg(windows)]
    {
        // A toast through PowerShell, which every Windows 10 and 11 has: no installer needed.
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\'', "&apos;").replace('"', "&quot;");
        let xml = format!("<toast><visual><binding template='ToastGeneric'><text>Ographis</text><text>{}</text></binding></visual></toast>", esc(text));
        let script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > $null; \
             $x = New-Object Windows.Data.Xml.Dom.XmlDocument; $x.LoadXml('{}'); \
             $t = [Windows.UI.Notifications.ToastNotification]::new($x); \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe').Show($t)",
            xml
        );
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).creation_flags(0x0800_0000).spawn();
    }
}

impl App {
    /// The size of everything: as the settings say, and kept in them when Ctrl and + or −
    /// change it.
    fn keep_zoom(&mut self, ctx: &egui::Context) {
        let want = crate::settings::ui_scale(&self.settings);
        let now = ctx.zoom_factor();
        if self.applied_zoom > 0.0 && (now - self.applied_zoom).abs() > 1e-3 {
            // Changed by the keys.
            self.applied_zoom = now;
            self.settings.other.insert("ui_scale".into(), serde_json::json!((now as f64 * 100.0).round() / 100.0));
            let _ = self.settings.save();
            self.say(format!("Everything is shown at {} %. Ctrl+0 for 100 %.", (now * 100.0).round()));
        } else if (want - now).abs() > 1e-3 {
            ctx.set_zoom_factor(want);
            self.applied_zoom = want;
        } else {
            self.applied_zoom = now;
        }
    }

    // ---------- The user interface ----------

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !theme::ensure(&ctx) {
            ctx.request_repaint();
            return;
        }
        let pal = Palette::from_ui(ui);
        self.keep_zoom(&ctx);
        let calm = crate::settings::reduce_motion(&self.settings);
        if calm != (ctx.global_style().animation_time == 0.0) {
            ctx.all_styles_mut(|s| s.animation_time = if calm { 0.0 } else { 1.0 / 12.0 });
        }
        if let Some(text) = self.notice.take() {
            tell_finished(&ctx, &text);
        }
        // How long each part of the frame took, told in the log when a frame is slow, so
        // that a window that stops answering says why.
        let mut clock = Phases::start();
        if let Some(rx) = self.running.take() {
            // Everything the run has reported since the last frame is taken in at once.
            let mut going = true;
            while going {
                match rx.try_recv() {
                    Ok(msg) => {
                        going = !matches!(msg, RunMsg::Done(_));
                        self.on_run_msg(msg);
                        if !going {
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        going = false;
                        self.live = false;
                        self.running_node = None;
                        self.say("The run stopped unexpectedly.");
                    }
                }
            }
            if going {
                self.running = Some(rx);
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
            }
        }
        clock.lap("messages of the run");
        self.give_up_stopping();
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            let at = drop_position(&ctx);
            self.drop_files_at(&dropped, at);
        }
        self.shortcuts(&ctx);
        self.close_guard(&ctx);
        clock.lap("dropped files and keys");

        egui::Panel::top("top").frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(12, 6))).show(ui, |ui| self.top_bar(ui, &pal));
        egui::Panel::left("left")
            .resizable(true)
            .default_size(226.0)
            .min_size(180.0)
            .max_size(420.0)
            .frame(egui::Frame::new().fill(pal.panel))
            .show(ui, |ui| self.left_panel(ui, &pal));
        clock.lap("tools and files");
        egui::Panel::right("params")
            .resizable(true)
            .default_size(310.0)
            .min_size(240.0)
            .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(12, 10)))
            .show(ui, |ui| self.params_panel(ui, &pal));
        clock.lap("the form");
        egui::Panel::bottom("inspector")
            .resizable(true)
            .default_size(300.0)
            .min_size(120.0)
            .frame(egui::Frame::new().fill(pal.surface))
            .show(ui, |ui| self.inspector(ui, &pal));
        clock.lap("data, map and log");
        egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.bg)).show(ui, |ui| self.canvas(ui, &pal));
        clock.lap("the canvas");
        self.unsaved_dialog(&ctx, &pal);
        self.recovery_dialog(&ctx, &pal);
        self.layer_dialog(&ctx, &pal);
        self.about_dialog(&ctx, &pal);
        self.history_dialog(&ctx, &pal);
        self.schedule_dialog(&ctx, &pal);
        self.picker_dialog(&ctx, &pal);
        self.tips_dialog(&ctx, &pal);
        self.update_dialog(&ctx, &pal);
        self.report_dialog(&ctx, &pal);
        self.variables_dialog(&ctx, &pal);
        self.description_dialog(&ctx, &pal);
        self.postgis_dialog(&ctx, &pal);
        self.settings_dialog(&ctx, &pal);
        // Last in the frame, so that everything a mouse button did when it was let go of
        // ends up in one step: a tool dragged onto a connection is moved and put in at once.
        self.take_in_when_idle(&ctx);
        clock.lap("dialogs");
        // The first frames load fonts and settings, and may well be slow.
        if let Some(line) = clock.slow(SLOW_FRAME).filter(|_| ctx.cumulative_frame_nr() > 5) {
            self.say(&line);
        }
    }

    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        if !typing && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::C)) {
            self.copy_picture(ctx);
            // The system may also have sent it as Copy, which is not for the nodes.
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Copy)));
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::F5)) {
            if !self.is_running() {
                self.trial_run(ctx);
            }
            return;
        }
        let stop = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::F5));
        if stop {
            self.stop_run();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::F5)) {
            if !self.is_running() {
                self.run_all_again(ctx);
            }
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F6)) {
            self.toggle_pause();
            return;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Comma)) && self.settings_window.is_none() {
            self.open_settings();
        }
        let (run, save, open, new) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                i.key_pressed(Key::F5) || (cmd && i.key_pressed(Key::Enter)),
                cmd && i.key_pressed(Key::S),
                cmd && i.key_pressed(Key::O),
                cmd && i.key_pressed(Key::N),
            )
        });
        // These would get in the way of editing text, so they only apply outside text fields.
        // Escape closes an open popup, such as the colour picker, before it lets go of the
        // selection.
        let popup = egui::Popup::is_any_open(ctx);
        let (delete, duplicate, all, none, fit, group) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                !typing && (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)),
                !typing && cmd && i.key_pressed(Key::D),
                !typing && cmd && i.key_pressed(Key::A),
                !typing && !popup && i.key_pressed(Key::Escape),
                !typing && i.key_pressed(Key::Home),
                !typing && cmd && i.key_pressed(Key::G),
            )
        });
        // Copy, cut and paste of nodes, also between two windows of Ographis. The system
        // turns Ctrl+C, Ctrl+X and Ctrl+V into events of their own.
        if !typing {
            let (copy, cut, paste) = ctx.input(|i| {
                let mut out = (false, false, None);
                for e in &i.events {
                    match e {
                        egui::Event::Copy => out.0 = true,
                        egui::Event::Cut => out.1 = true,
                        egui::Event::Paste(t) => out.2 = Some(t.clone()),
                        _ => {}
                    }
                }
                out
            });
            if copy || cut {
                self.copy_selected(ctx, cut);
            }
            if let Some(text) = paste {
                self.paste(&text);
            }
        }
        // A text field has an undo of its own for what is typed in it.
        let (undo, redo) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                !typing && cmd && !i.modifiers.shift && i.key_pressed(Key::Z),
                !typing && cmd && (i.key_pressed(Key::Y) || (i.modifiers.shift && i.key_pressed(Key::Z))),
            )
        });
        // While a window asks something, the keys belong to it.
        let asking = self.pending.is_some()
            || self.about
            || self.history_open
            || self.schedule.is_some()
            || self.picker.is_some()
            || self.tips
            || self.variables_open
            || self.db_dialog.is_some()
            || self.settings_window.is_some()
            || !self.layer_choices.is_empty();
        if asking {
            if none && self.pending.is_none() {
                self.about = false;
                self.history_open = false;
                self.schedule = None;
                self.tips = false;
                self.variables_open = false;
                self.layer_choices.pop_front();
            }
            return;
        }
        self.canvas_keys(ctx);
        if run {
            self.run(ctx);
        }
        if save {
            self.save(false);
        }
        if open {
            self.request(Pending::Open);
        }
        if new {
            self.request(Pending::New);
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if delete {
            self.delete_selected();
        }
        if group {
            self.add_group(&self.selected.clone(), None);
        }
        if duplicate {
            self.duplicate_nodes(&self.selected.clone());
        }
        if all {
            let everything: Vec<NodeId> = self.snarl.nodes_ids_data().map(|(id, _)| id).collect();
            self.set_selection(everything);
        }
        if none {
            self.set_selection(Vec::new());
            self.selected_group = None;
            self.selected_wire = None;
        }
        if fit {
            self.fit_requested = Some(SMALLEST_SCALE);
        }
    }
}

/// A frame slower than this is told in the log, with what took the time.
const SLOW_FRAME: std::time::Duration = std::time::Duration::from_millis(700);

/// The time of each part of a frame.
pub(crate) struct Phases {
    began: std::time::Instant,
    last: std::time::Instant,
    laps: Vec<(&'static str, std::time::Duration)>,
}

impl Phases {
    pub(crate) fn start() -> Self {
        let now = std::time::Instant::now();
        Phases { began: now, last: now, laps: Vec::new() }
    }
    pub(crate) fn lap(&mut self, what: &'static str) {
        let now = std::time::Instant::now();
        self.laps.push((what, now - self.last));
        self.last = now;
    }
    /// A line for the log when the frame took longer than `limit`: the whole time and the
    /// parts that took a tenth of it or more, the longest first.
    pub(crate) fn slow(&self, limit: std::time::Duration) -> Option<String> {
        let all = self.last - self.began;
        if all < limit {
            return None;
        }
        let mut parts: Vec<_> = self.laps.iter().filter(|(_, t)| *t * 10 >= all).collect();
        parts.sort_by_key(|p| std::cmp::Reverse(p.1));
        let parts: Vec<String> = parts.iter().map(|(w, t)| format!("{} {:.1} s", w, t.as_secs_f64())).collect();
        Some(format!(
            "The window did not answer for {:.1} s: {}. Tell this line along with what you did, should it happen often.",
            all.as_secs_f64(),
            parts.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_frame_says_what_took_the_time() {
        let mut p = Phases::start();
        p.laps = vec![
            ("the form", std::time::Duration::from_millis(50)),
            ("data, map and log", std::time::Duration::from_millis(1900)),
            ("the canvas", std::time::Duration::from_millis(5)),
        ];
        p.last = p.began + std::time::Duration::from_millis(1955);
        let line = p.slow(SLOW_FRAME).unwrap();
        assert!(line.starts_with("The window did not answer for 2.0 s: data, map and log 1.9 s."), "{}", line);
        p.last = p.began + std::time::Duration::from_millis(100);
        assert!(p.slow(SLOW_FRAME).is_none());
    }
}
