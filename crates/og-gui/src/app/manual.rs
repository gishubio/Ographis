//! The help in the program: a few pages on getting started, and a page for every tool, made
//! from what the tool says about itself, so that it always tells what the tool does in this
//! version. Help → Ographis help, or F1, which opens the page of the selected node.

use super::*;
use og_engine::nodes::ParamKind;
use std::sync::OnceLock;

/// The page that is shown.
#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    /// A chapter of getting started, by its place.
    Guide(usize),
    /// The page of a tool, by its name in files.
    Tool(&'static str),
}

/// The help window: the page shown, and what is searched for among the tools.
#[derive(Debug, Clone)]
pub struct Help {
    pub page: Page,
    pub search: String,
    /// The page is to be scrolled to the top, as when another page is chosen.
    to_top: bool,
}

/// The chapters of getting started: a title and its paragraphs. A paragraph that starts with
/// "• " is a point in a list.
pub const GUIDE: &[(&str, &[&str])] = &[
    (
        "What Ographis does",
        &[
            "Ographis moves and changes geodata in steps you can see. A model is a drawing of nodes: inputs that read data, tools that change it and outputs that write it. The data flows along the connections, from left to right.",
            "Each step can be looked at by itself: click a port after a run, and its data is shown in the table and on the map under the canvas. A model is saved as a small text file, .ographis, that can be run again, by you or on a schedule, and shared with others.",
            "Nothing is sent anywhere unless a node in the model asks for it, such as an input that reads from the web or an HTTP request.",
        ],
    ),
    (
        "Your first model",
        &[
            "• Drop a data file, such as a GeoPackage, a shapefile or a CSV with coordinates, on the window. An input for it appears on the canvas.",
            "• Add a tool: double-click empty canvas, write a few letters, such as buff for Buffer, and press Enter. Or fold out a list on the left and click a tool.",
            "• Connect: drag from the output port of the input, the ring on its right, to the input port of the tool.",
            "• Click the tool and set it up in the form on the right, such as the distance of the buffer.",
            "• Add an output, from Input and output on the left, connect it and choose a file to write.",
            "• Press F5 to run. Each node shows how many features came out and how long it took.",
            "The examples show whole models to start from: Help → Start screen, or Ctrl+K and the name of an example.",
        ],
    ),
    (
        "The window",
        &[
            "• To the left: the inputs and outputs of the model, and every tool, in lists by what they work on. The search box finds a tool by its name or by what it does.",
            "• In the middle: the canvas with the model. Scroll to zoom, hold Space and drag to move it, Home to see all of it.",
            "• To the right: the form of the selected node, with its settings. Rest the pointer on a setting for what it does. Settings seldom changed are under More.",
            "• Below: the data of the selected port as a table and on a map, and the log of the latest run.",
            "• At the top: the menus, the buttons that are used most, and how the latest run went.",
            "Ctrl+K opens one search box for everything: the commands, the nodes of the model, the tools and the examples.",
        ],
    ),
    (
        "Running and looking at data",
        &[
            "F5 runs the model. Only what has changed since the latest run is run again; Ctrl+F5 runs everything again. F6 pauses and Shift+F5 stops.",
            "A trial run, Ctrl+Shift+F5, reads only the first thousand features of every input, to try a model on large data quickly.",
            "After a run, click an output port of a node to see what came out: the table lists the features, and the map shows them. A click on a row shows the feature on the map, and a click on the map its row.",
            "The log tab below lists what each node did, and its warnings. Model → Run history keeps every run, with its log.",
        ],
    ),
    (
        "When something goes wrong",
        &[
            "A node that fails is drawn red, and the error is shown in its form and in the log. The run goes on with what does not depend on it.",
            "• F8 goes to the next node with an error, Shift+F8 to the one before. A click on the errors at the top does the same.",
            "• Under the error in the form there is often a button to what is wrong: the setting it is about, such as a path to a file that is not there, or the connection that could not be reached.",
            "• Copy under the error copies it with the name of the node, to send to someone.",
            "• Help → Report a problem writes a report with the version and the latest errors, to send as an issue on GitHub.",
        ],
    ),
    (
        "Connections and passwords",
        &[
            "Databases and web APIs that need a password or a key are set up once, in Edit → Settings (Ctrl+,), under Database connections and API connections, each with a name of your own.",
            "A node chooses a connection by its name, and the model keeps only that name. So a model can be shared, or put in Git, without the password, and each one who runs it uses a connection of the same name of their own.",
            "Variables, under Model → Variables, are values the model uses in several places, such as a year or a folder. They are written $(name) in a setting and can be given when the model is run from the command line.",
        ],
    ),
    (
        "Running without the window",
        &[
            "Every model can be run from the command line with og run model.ographis, with the same result as in the window. Model → Copy command line gives the command.",
            "Model → Schedule runs a model by itself at set times, through the Task Scheduler of Windows. The runs are kept in the run history beside the model.",
            "File → Document the model as a web page writes a page with a picture of the model and each step, to keep beside it or send to others.",
        ],
    ),
];

/// The examples that use each tool: (name, path) by the name of the tool. Read once.
fn examples_by_tool() -> &'static HashMap<String, Vec<(String, PathBuf)>> {
    static FOUND: OnceLock<HashMap<String, Vec<(String, PathBuf)>>> = OnceLock::new();
    FOUND.get_or_init(|| {
        let mut out: HashMap<String, Vec<(String, PathBuf)>> = HashMap::new();
        let Some(dir) = super::help::examples_dir() else { return out };
        let mut files: Vec<PathBuf> = [dir.clone(), dir.join("apis")]
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flat_map(|r| r.flatten().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "ographis"))
            .collect();
        files.sort();
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mut kinds: Vec<&str> = json.get("nodes").and_then(|n| n.as_array()).into_iter().flatten().filter_map(|n| n.get("type")?.as_str()).collect();
            kinds.sort();
            kinds.dedup();
            for k in kinds {
                out.entry(k.to_string()).or_default().push((name.clone(), path.clone()));
            }
        }
        out
    })
}

/// What a setting holds, in a few words.
fn kind_text(kind: &ParamKind) -> String {
    match kind {
        ParamKind::Choice(c) => format!("One of: {}", c.iter().map(|x| x.1).collect::<Vec<_>>().join(" · ")),
        ParamKind::Picks(c) => format!("Any of: {}", c.iter().map(|x| x.1).collect::<Vec<_>>().join(" · ")),
        ParamKind::Attribute(p) => format!("An attribute of {}", p),
        ParamKind::Attributes(p) => format!("Attributes of {}", p),
        ParamKind::Expression(p) => format!("An expression on {}", p),
        ParamKind::FileOpen | ParamKind::Source => "A file".to_string(),
        ParamKind::FileSave => "A file to write".to_string(),
        ParamKind::Connection => "A database connection from the settings".to_string(),
        ParamKind::ApiConnection => "An API connection from the settings".to_string(),
        ParamKind::Crs => "A coordinate system".to_string(),
        ParamKind::Secret => "A password or key: better kept in a connection".to_string(),
        _ => String::new(),
    }
}

impl App {
    /// Opens the help: at the page of a tool, or at getting started.
    pub fn open_help(&mut self, tool: Option<&str>) {
        let page = tool.and_then(og_engine::node_type).map_or(Page::Guide(0), |t| Page::Tool(t.name));
        self.help = Some(Help { page, search: String::new(), to_top: true });
    }

    /// F1: the page of the selected node, or getting started.
    pub(super) fn help_for_selection(&mut self) {
        let kind = match self.selected.as_slice() {
            [one] => self.snarl.get_node(*one).map(|n| n.kind.clone()),
            _ => None,
        };
        self.open_help(kind.as_deref());
    }

    pub(super) fn help_window(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut help) = self.help.take() else { return };
        let mut open = true;
        let mut add: Option<&'static str> = None;
        let mut example: Option<PathBuf> = None;
        egui::Window::new("Ographis help")
            .id(egui::Id::new("help-window"))
            .open(&mut open)
            .default_size(egui::vec2(820.0, 600.0))
            .max_width(1000.0)
            .min_size(egui::vec2(520.0, 320.0))
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // The contents to the left: getting started, then the tools by their lists.
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        ui.add(egui::TextEdit::singleline(&mut help.search).hint_text("Search the tools").desired_width(222.0));
                        ScrollArea::vertical().id_salt("help-contents").auto_shrink([false, false]).show(ui, |ui| {
                            let needle = help.search.trim().to_lowercase();
                            if needle.is_empty() {
                                ui.label(RichText::new("GETTING STARTED").font(theme::mono(11.0)).color(pal.muted));
                                for (i, (title, _)) in GUIDE.iter().enumerate() {
                                    let on = help.page == Page::Guide(i);
                                    if ui.selectable_label(on, *title).clicked() {
                                        help.page = Page::Guide(i);
                                        help.to_top = true;
                                    }
                                }
                                ui.add_space(8.0);
                                ui.label(RichText::new("TOOLS").font(theme::mono(11.0)).color(pal.muted));
                            }
                            let all = graph::offered(&[Group::Reader, Group::Transformer, Group::Writer]);
                            let mut lists: Vec<(&str, Vec<&NodeType>)> = Vec::new();
                            for t in all.iter().copied() {
                                let found = needle.is_empty()
                                    || t.title.to_lowercase().contains(&needle)
                                    || t.name.to_lowercase().contains(&needle)
                                    || t.desc.to_lowercase().contains(&needle);
                                if !found {
                                    continue;
                                }
                                let list = if t.group != Group::Transformer { "Input and output" } else { t.category };
                                match lists.iter_mut().find(|l| l.0 == list) {
                                    Some(l) => l.1.push(t),
                                    None => lists.push((list, vec![t])),
                                }
                            }
                            lists.sort_by_key(|l| (l.0 != "Input and output", l.0));
                            if lists.is_empty() {
                                ui.label(RichText::new("No tool matches.").color(pal.muted));
                            }
                            for (list, mut tools) in lists {
                                tools.sort_by_key(|t| t.title.to_lowercase());
                                let shown = egui::CollapsingHeader::new(RichText::new(list).small())
                                    .id_salt(("help-list", list))
                                    .default_open(!needle.is_empty() || tools.iter().any(|t| help.page == Page::Tool(t.name)))
                                    .open((!needle.is_empty()).then_some(true));
                                shown.show(ui, |ui| {
                                    for t in tools {
                                        if ui.selectable_label(help.page == Page::Tool(t.name), t.title).clicked() {
                                            help.page = Page::Tool(t.name);
                                            help.to_top = true;
                                        }
                                    }
                                });
                            }
                        });
                    });
                    ui.separator();
                    // The page.
                    let mut scroll = ScrollArea::vertical().id_salt("help-page").auto_shrink([false, false]);
                    if help.to_top {
                        scroll = scroll.vertical_scroll_offset(0.0);
                        help.to_top = false;
                    }
                    scroll.show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width(560.0);
                            match help.page.clone() {
                                Page::Guide(i) => guide_page(ui, pal, i, &mut help),
                                Page::Tool(name) => {
                                    if let Some(t) = og_engine::node_type(name) {
                                        tool_page(ui, pal, t, &mut add, &mut example);
                                    }
                                }
                            }
                        });
                    });
                });
            });
        if let Some(kind) = add {
            self.add_node(kind);
        }
        if let Some(path) = example {
            self.request(Pending::OpenPath(path));
        }
        if open {
            self.help = Some(help);
        }
    }
}

fn paragraphs(ui: &mut Ui, pal: &Palette, text: &[&str]) {
    for p in text {
        match p.strip_prefix("• ") {
            Some(point) => {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").color(pal.muted));
                    ui.add(egui::Label::new(RichText::new(point).color(pal.ink)).wrap());
                });
            }
            None => {
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(*p).color(pal.ink)).wrap());
            }
        }
        ui.add_space(3.0);
    }
}

fn guide_page(ui: &mut Ui, pal: &Palette, i: usize, help: &mut Help) {
    let Some((title, text)) = GUIDE.get(i) else { return };
    ui.label(RichText::new(*title).font(theme::strong(19.0)).color(pal.ink));
    ui.add_space(6.0);
    paragraphs(ui, pal, text);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if i > 0 && ui.button(format!("← {}", GUIDE[i - 1].0)).clicked() {
            help.page = Page::Guide(i - 1);
            help.to_top = true;
        }
        if i + 1 < GUIDE.len() && ui.button(format!("{} →", GUIDE[i + 1].0)).clicked() {
            help.page = Page::Guide(i + 1);
            help.to_top = true;
        }
    });
}

fn tool_page(ui: &mut Ui, pal: &Palette, t: &'static NodeType, add: &mut Option<&'static str>, example: &mut Option<PathBuf>) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 22.0), Sense::hover());
        ui.painter().rect_filled(Rect::from_center_size(dot.center(), egui::vec2(10.0, 10.0)), 2.0, pal.group(t.group).0);
        ui.label(RichText::new(t.title).font(theme::strong(19.0)).color(pal.ink));
    });
    let list = if t.group != Group::Transformer { "Input and output" } else { t.category };
    ui.label(RichText::new(format!("{} · {} in model files", list, t.name)).font(theme::mono(11.5)).color(pal.muted));
    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(t.desc).color(pal.ink)).wrap());
    ui.add_space(6.0);
    if ui.button("Add to the model").on_hover_text("Puts the tool on the canvas").clicked() {
        *add = Some(t.name);
    }
    let ports = |ui: &mut Ui, heading: &str, names: &[&str]| {
        if names.is_empty() {
            return;
        }
        ui.add_space(8.0);
        ui.label(RichText::new(heading).font(theme::strong(14.0)).color(pal.ink));
        ui.label(RichText::new(names.join(" · ")).font(theme::mono(12.0)).color(pal.ink));
    };
    ports(ui, "Comes in on", t.inputs);
    ports(ui, "Goes out on", t.outputs);
    if t.ports.is_some() {
        ui.label(RichText::new("The ports follow the settings.").small().color(pal.muted));
    }
    if !t.params.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("Settings").font(theme::strong(14.0)).color(pal.ink));
        for p in t.params {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(p.label).strong().color(pal.ink));
                if !p.default.is_empty() {
                    let shown = match p.kind {
                        ParamKind::Choice(c) | ParamKind::Picks(c) => c.iter().find(|x| x.0 == p.default).map_or(p.default, |x| x.1),
                        _ => p.default,
                    };
                    ui.label(RichText::new(format!("{} if not set", shown)).small().color(pal.muted));
                }
            });
            if !p.hint.is_empty() {
                ui.add(egui::Label::new(RichText::new(p.hint).color(pal.muted)).wrap());
            }
            let kind = kind_text(&p.kind);
            if !kind.is_empty() {
                ui.add(egui::Label::new(RichText::new(kind).small().color(pal.muted)).wrap());
            }
        }
    }
    if let Some(found) = examples_by_tool().get(t.name) {
        ui.add_space(10.0);
        ui.label(RichText::new("In the examples").font(theme::strong(14.0)).color(pal.ink));
        ui.horizontal_wrapped(|ui| {
            for (name, path) in found.iter().take(12) {
                if ui.link(name).on_hover_text("Opens the example. A model with unsaved changes asks first").clicked() {
                    *example = Some(path.clone());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_tool_has_a_page_with_what_it_does() {
        let all = graph::offered(&[Group::Reader, Group::Transformer, Group::Writer]);
        assert!(all.len() > 100);
        for t in all {
            assert!(t.desc.len() > 20, "{} says too little about itself", t.name);
            for p in t.params {
                assert!(!p.label.trim().is_empty(), "{}: a setting without a label: {}", t.name, p.key);
            }
        }
        assert!(GUIDE.iter().all(|(title, text)| !title.is_empty() && !text.is_empty()));
    }
}
