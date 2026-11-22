//! The command palette, Ctrl+K: one search box that reaches everything: the commands of the
//! menus, the nodes of the model, the tools to add, the latest models and the examples.

use super::*;

/// The palette while it is open: what is typed and which line is chosen.
#[derive(Debug, Clone, Default)]
pub struct Commands {
    pub text: String,
    pub chosen: usize,
}

/// What a line of the palette does.
#[derive(Debug, Clone, PartialEq)]
enum Act {
    Do(Cmd),
    GoTo(NodeId),
    Add(&'static str),
    Open(PathBuf),
}

/// The commands of the menus.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    SavePicture,
    CopyPicture,
    Document,
    Undo,
    Redo,
    Group,
    Note,
    SelectAll,
    Settings,
    Fit,
    Find,
    Minimap,
    ShowAttributes,
    HideAttributes,
    Run,
    RunAll,
    Trial,
    Stop,
    Pause,
    NextError,
    PreviousError,
    Description,
    Variables,
    History,
    Schedule,
    CommandLine,
    Help,
    Tips,
    StartScreen,
    Updates,
    Report,
    About,
}

/// The commands: what they do, their name, where they are in the menus and their keys.
const COMMANDS: &[(Cmd, &str, &str, &str)] = &[
    (Cmd::Run, "Run", "Model", "F5"),
    (Cmd::RunAll, "Run everything again", "Model", "Ctrl+F5"),
    (Cmd::Trial, "Trial run", "Model", "Ctrl+Shift+F5"),
    (Cmd::Stop, "Stop the run", "Model", "Shift+F5"),
    (Cmd::Pause, "Pause or resume the run", "Model", "F6"),
    (Cmd::NextError, "Go to the next error", "Model", "F8"),
    (Cmd::PreviousError, "Go to the error before", "Model", "Shift+F8"),
    (Cmd::Save, "Save", "File", "Ctrl+S"),
    (Cmd::SaveAs, "Save as…", "File", ""),
    (Cmd::New, "New model", "File", "Ctrl+N"),
    (Cmd::Open, "Open a model…", "File", "Ctrl+O"),
    (Cmd::Document, "Document the model as a web page…", "File", ""),
    (Cmd::SavePicture, "Save the model as a picture…", "File", ""),
    (Cmd::CopyPicture, "Copy the model as a picture", "File", "Ctrl+Shift+C"),
    (Cmd::Undo, "Undo", "Edit", "Ctrl+Z"),
    (Cmd::Redo, "Redo", "Edit", "Ctrl+Y"),
    (Cmd::Group, "Group the selected nodes", "Edit", "Ctrl+G"),
    (Cmd::Note, "Add a note", "Edit", ""),
    (Cmd::SelectAll, "Select all", "Edit", "Ctrl+A"),
    (Cmd::Settings, "Settings, connections and maps…", "Edit", "Ctrl+,"),
    (Cmd::Fit, "Zoom to fit", "View", "Home"),
    (Cmd::Find, "Find a node…", "View", "Ctrl+F"),
    (Cmd::Minimap, "Show or hide the map of the model", "View", ""),
    (Cmd::ShowAttributes, "Show the attributes on all nodes", "View", ""),
    (Cmd::HideAttributes, "Hide the attributes on all nodes", "View", ""),
    (Cmd::Description, "Description of the model…", "Model", ""),
    (Cmd::Variables, "Variables…", "Model", ""),
    (Cmd::History, "Run history…", "Model", ""),
    (Cmd::Schedule, "Schedule runs…", "Model", ""),
    (Cmd::CommandLine, "Copy command line", "Model", ""),
    (Cmd::Help, "Ographis help", "Help", "F1"),
    (Cmd::Tips, "Tips and shortcuts", "Help", ""),
    (Cmd::StartScreen, "Start screen", "Help", ""),
    (Cmd::Updates, "Check for updates", "Help", ""),
    (Cmd::Report, "Report a problem…", "Help", ""),
    (Cmd::About, "About Ographis", "Help", ""),
];

/// A line of the palette: what it says, what kind it is, its keys, and what it does.
struct Line {
    label: String,
    kind: &'static str,
    keys: &'static str,
    act: Act,
}

/// The examples in the folder of examples and the folder of web services in it, as (name,
/// path), named by their file names.
fn examples() -> Vec<(String, PathBuf)> {
    let Some(dir) = help::examples_dir() else { return Vec::new() };
    let mut out = Vec::new();
    for d in [dir.clone(), dir.join("apis")] {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "ographis")).collect();
        files.sort();
        for p in files {
            let name = p.file_stem().map(|s| s.to_string_lossy().replace('_', " ")).unwrap_or_default();
            out.push((name, p));
        }
    }
    out
}

/// Whether every word typed is in the text, without regard to case.
fn fits(words: &[String], text: &str) -> bool {
    let t = text.to_lowercase();
    words.iter().all(|w| t.contains(w.as_str()))
}

impl App {
    fn command_lines(&self, text: &str) -> Vec<Line> {
        let words: Vec<String> = text.to_lowercase().split_whitespace().map(str::to_string).collect();
        let mut out = Vec::new();
        for (cmd, label, menu, keys) in COMMANDS {
            if fits(&words, &format!("{} {}", label, menu)) {
                out.push(Line { label: label.to_string(), kind: menu, keys, act: Act::Do(*cmd) });
            }
        }
        // The rest only once something is typed, so that the list starts with the commands.
        if words.is_empty() {
            for p in self.recent.list.iter().filter(|p| p.exists()).take(5) {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(Line { label: name, kind: "Recent", keys: "", act: Act::Open(p.clone()) });
            }
            return out;
        }
        for (id, _, n) in self.snarl.nodes_pos_ids() {
            if n.is_note() {
                continue;
            }
            let tool = n.ty().map_or(n.kind.as_str(), |t| t.title);
            let label = if n.name == tool { n.name.clone() } else { format!("{} · {}", n.name, tool) };
            if fits(&words, &format!("{} {} {}", label, n.comment, n.id)) {
                out.push(Line { label, kind: "Node", keys: "", act: Act::GoTo(id) });
            }
        }
        for t in og_engine::nodes::NODE_TYPES.iter().filter(|t| !t.hidden) {
            if fits(&words, &format!("{} {} {}", t.title, t.name, t.category)) {
                out.push(Line { label: format!("Add {}", t.title), kind: "Tool", keys: "", act: Act::Add(t.name) });
            }
        }
        for p in self.recent.list.iter().filter(|p| p.exists()) {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if fits(&words, &name) {
                out.push(Line { label: name, kind: "Recent", keys: "", act: Act::Open(p.clone()) });
            }
        }
        for (name, p) in examples() {
            if fits(&words, &format!("{} example", name)) {
                out.push(Line { label: name, kind: "Example", keys: "", act: Act::Open(p) });
            }
        }
        out
    }

    fn do_command(&mut self, ctx: &egui::Context, cmd: Cmd) {
        match cmd {
            Cmd::New => {
                self.request(Pending::New);
            }
            Cmd::Open => {
                self.request(Pending::Open);
            }
            Cmd::Save => {
                self.save(false);
            }
            Cmd::SaveAs => {
                self.save(true);
            }
            Cmd::SavePicture => {
                self.save_picture();
            }
            Cmd::CopyPicture => {
                self.copy_picture(ctx);
            }
            Cmd::Document => {
                self.save_documentation(ctx);
            }
            Cmd::Undo => {
                self.undo();
            }
            Cmd::Redo => {
                self.redo();
            }
            Cmd::Group => {
                self.add_group(&self.selected.clone(), None);
            }
            Cmd::Note => {
                self.add_node(NOTE);
            }
            Cmd::SelectAll => {
                let everything: Vec<NodeId> = self.snarl.nodes_ids_data().map(|(id, _)| id).collect();
                self.set_selection(everything);
            }
            Cmd::Settings => {
                self.open_settings();
            }
            Cmd::Fit => {
                self.fit_requested = Some(SMALLEST_SCALE);
            }
            Cmd::Find => {
                self.picker = Some(Default::default());
            }
            Cmd::Minimap => self.show_minimap = !self.show_minimap,
            Cmd::ShowAttributes => {
                self.expanded = self.snarl.nodes().filter(|n| !n.is_note()).map(|n| n.id.clone()).collect();
            }
            Cmd::HideAttributes => {
                self.expanded.clear();
            }
            Cmd::Run if !self.is_running() => {
                self.run(ctx);
            }
            Cmd::RunAll if !self.is_running() => {
                self.run_all_again(ctx);
            }
            Cmd::Trial if !self.is_running() => {
                self.trial_run(ctx);
            }
            Cmd::Run | Cmd::RunAll | Cmd::Trial => self.say("A run is going on."),
            Cmd::Stop => {
                self.stop_run();
            }
            Cmd::Pause => {
                self.toggle_pause();
            }
            Cmd::NextError => self.go_to_error(true),
            Cmd::PreviousError => self.go_to_error(false),
            Cmd::Description => self.description_open = true,
            Cmd::Variables => self.variables_open = true,
            Cmd::History if self.path.is_some() => self.history_open = true,
            Cmd::Schedule if self.path.is_some() => {
                self.open_schedule();
            }
            Cmd::History | Cmd::Schedule => self.say("Save the model first."),
            Cmd::CommandLine => {
                self.copy_command_line(ctx);
            }
            Cmd::Tips => self.tips = true,
            Cmd::StartScreen => {
                self.start_screen = true;
                self.request(Pending::New);
            }
            Cmd::Updates => {
                self.check_for_updates();
            }
            Cmd::Report => {
                self.open_report();
            }
            Cmd::About => self.about = true,
            Cmd::Help => self.help_for_selection(),
        }
    }

    /// The palette of Ctrl+K, while it is open.
    pub(super) fn commands_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut c) = self.commands.take() else { return };
        let lines = self.command_lines(&c.text);
        c.chosen = c.chosen.min(lines.len().saturating_sub(1));
        let mut done: Option<Option<usize>> = None;
        let modal = egui::Modal::new(egui::Id::new("commands")).show(ctx, |ui| {
            ui.set_width(560.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut c.text)
                    .hint_text("Type a command, a node, a tool or an example")
                    .font(theme::strong(15.0))
                    .desired_width(f32::INFINITY),
            );
            field.request_focus();
            if field.changed() {
                c.chosen = 0;
            }
            let (down, up, enter, esc) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, Key::Escape),
                )
            });
            if down && c.chosen + 1 < lines.len() {
                c.chosen += 1;
            }
            if up && c.chosen > 0 {
                c.chosen -= 1;
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                if lines.is_empty() {
                    ui.label(RichText::new("Nothing matches.").color(pal.muted));
                }
                for (k, l) in lines.iter().enumerate() {
                    let side = if l.keys.is_empty() { l.kind.to_string() } else { format!("{}   {}", l.kind, l.keys) };
                    let r = ui.add(
                        egui::Button::selectable(k == c.chosen, &l.label)
                            .right_text(RichText::new(side).font(theme::mono(11.0)).color(pal.muted))
                            .min_size(egui::vec2(ui.available_width(), 24.0)),
                    );
                    if k == c.chosen && (down || up) {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        done = Some(Some(k));
                    }
                }
            });
            ui.label(RichText::new("Up and Down to choose, Enter to do it, Escape to close").small().color(pal.muted));
            if enter && !lines.is_empty() {
                done = Some(Some(c.chosen));
            }
            if esc {
                done = Some(None);
            }
        });
        if modal.should_close() && done.is_none() {
            done = Some(None);
        }
        match done {
            None => self.commands = Some(c),
            Some(None) => {}
            Some(Some(k)) => match lines[k].act.clone() {
                Act::Do(cmd) => self.do_command(ctx, cmd),
                Act::GoTo(node) => {
                    self.set_selection(vec![node]);
                    if let Some(id) = self.snarl.get_node(node).map(|n| n.id.clone()) {
                        self.focus_node(&id);
                        self.reveal(&id);
                    }
                }
                Act::Add(kind) => self.add_node(kind),
                Act::Open(path) => self.request(Pending::OpenPath(path)),
            },
        }
    }
}
