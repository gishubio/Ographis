//! The state of the program and the main window: menu, palette, canvas, parameters, inspector.

use crate::graph::{self, group_rect, CanvasEvents, GNode, GroupEvent, Viewer, GROUP_TITLE, NOTE};
use crate::map::MapView;
use crate::params::{self, ParamCtx};
use crate::sources::{self, Lookup};
use crate::theme::{self, Palette, GROUP_COLORS};
use eframe::egui::{self, emath::TSTransform, Align, Key, Layout, Rect, RichText, ScrollArea, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_snarl::ui::SnarlWidget;
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl};
use og_engine::io::{Backend, LayerInfo};
use og_engine::model::{crs_label, fmt_count, Table, Value};
use og_engine::nodes::{Group, NodeType, DEFAULT_PATTERN, READ_FORMATS, WRITER_FORMATS};
use og_engine::{GroupBox, Level, NodeResult, Progress, RunResult, Status, Workspace};
use og_gdal::GdalBackend;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

mod canvas;
mod data;
mod dialogs;
mod edit;
mod files;
mod form;
mod frame;
mod groups;
mod navigator;
mod palette;
mod run;
mod top_bar;
mod wires;

// Each module adds methods to App, in an impl block of its own. These two also have
// helpers that others use.
use data::*;
use files::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Data,
    Log,
}

/// What the inspector shows: a node, one of its ports and possibly a selected feature.
#[derive(Default, Clone)]
pub struct View {
    pub node: Option<String>,
    pub port: Option<String>,
    pub feature: Option<usize>,
    /// The other rows picked with Ctrl or Shift, besides `feature`.
    pub more: Vec<usize>,
}

/// The ports that can be shown for a node, with their tables, and which one is chosen.
type Viewed = (Vec<(String, Arc<Table>)>, usize);

enum Pending {
    New,
    Open,
    /// Open this file, for example one that was dropped on the window.
    OpenPath(PathBuf),
    Close,
}

/// What a run reports while it goes on.
pub enum RunMsg {
    /// The node with this id has started.
    Started(String),
    /// The node with this id is done, with this result.
    Finished(String, Box<NodeResult>),
    /// A line was written in the log.
    Log(og_engine::run::LogEntry),
    /// The whole run is done.
    Done(Box<RunResult>),
}

/// A file with several layers that waits for the user to choose which ones to read.
pub struct LayerChoice {
    pub file: PathBuf,
    pub layers: Vec<LayerInfo>,
    pub chosen: Vec<bool>,
    /// Read the chosen layers with one reader, as one stream, instead of one reader each.
    pub one_stream: bool,
    /// Where the file was dropped on the canvas, in graph coordinates.
    pub at: Option<egui::Pos2>,
}

/// What to show in the file manager for a path: the file itself, to be marked in its
/// folder, or a folder to open. A file that is not there yet gives the nearest folder
/// above it that is. `None` if nothing on the way exists.
pub fn folder_to_show(path: &Path) -> Option<(PathBuf, bool)> {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if path.is_file() {
        return Some((path, true));
    }
    path.ancestors().find(|a| a.is_dir()).map(|a| (a.to_path_buf(), false))
}

/// A group box that is being dragged, and what follows along. Decided when the drag
/// starts, so that the box does not sweep up the nodes it passes over.
struct GroupDrag {
    index: usize,
    nodes: Vec<NodeId>,
    groups: Vec<usize>,
    /// The box was taken hold of with a selection it belongs to, such as everything: all
    /// that is selected goes along, locked or not.
    with_selection: bool,
}

/// Files that belong to a Shapefile or a layer and are never data sources of their own.
const SIDECARS: &[&str] = &["shx", "prj", "cpg", "qpj", "qmd", "qix", "fix", "sbn", "sbx"];

pub struct App {
    pub snarl: Snarl<GNode>,
    pub name: String,
    /// What the model does, kept in the model, shown when it is opened.
    pub description: String,
    /// The description is shown on the canvas, until it is closed.
    pub show_description: bool,
    /// The window where the description is written is open.
    pub description_open: bool,
    pub path: Option<PathBuf>,
    /// Changes that are not saved.
    pub dirty: bool,
    /// Changes since the latest run.
    pub stale: bool,
    pub result: Option<Arc<RunResult>>,
    running: Option<Receiver<RunMsg>>,
    /// Set to stop the run that is going on.
    stop: Arc<std::sync::atomic::AtomicBool>,
    /// Whether Stop has been pressed for the run that is going on.
    pub stopping: bool,
    /// When Stop was pressed: a tool that does not stop in a few seconds is left to finish
    /// on its own, and the run ends without it.
    stop_asked: Option<std::time::Instant>,
    /// Set to pause the run that is going on, and cleared to go on.
    pause: Arc<std::sync::atomic::AtomicBool>,
    /// The run that is going on is paused.
    pub paused: bool,
    /// Keep the results of the latest run, so that a part of the model can be run with
    /// what comes before it taken from them. Saved with the model.
    pub use_cache: bool,
    /// With the cache on: the nodes that have a result in it, and whether it is up to date.
    /// Worked out again now and then, since a file that is read can change.
    cache_state: (f64, HashMap<String, bool>),
    /// The key of each node as the model is now, worked out at most twice a second: a result
    /// with another key is from before a change.
    keys_now: (f64, HashMap<String, u64>),
    /// The latest command line put on the clipboard.
    pub last_command: Option<String>,
    /// The latest nodes put on the clipboard, as JSON.
    pub clipboard: Option<String>,
    /// Where the pointer is on the canvas, on the screen, while it is there.
    pub pointer_on_canvas: Option<egui::Pos2>,
    /// Where the next paste goes on the canvas. Set by Paste in the menu of the canvas.
    pub paste_at: Option<egui::Pos2>,
    /// The result from before a run of a part of the model. What the run does not touch,
    /// and is still up to date, is kept from it.
    previous: Option<Arc<RunResult>>,
    /// A run is going on. The result is then filled in node by node.
    live: bool,
    /// The id of the node that is running right now.
    pub running_node: Option<String>,
    /// How far the node that runs has come, such as "read 430 373 of 4 303 726 features (10 %)".
    pub running_step: Option<String>,
    /// How many nodes of the run are done, and how many there are.
    progress: (usize, usize),
    /// The named boxes that hold nodes together on the canvas.
    pub groups: Vec<GroupBox>,
    /// The group box that is selected. Its settings are shown in the panel on the right.
    pub selected_group: Option<usize>,
    group_drag: Option<GroupDrag>,
    /// The ids of the nodes whose attribute list is folded out on the canvas.
    pub expanded: HashSet<String>,
    lookup: Lookup,
    /// The connection that is selected, to be removed with Delete.
    pub selected_wire: Option<(OutPinId, InPinId)>,
    /// The menu of a connection, open at a place on the screen.
    pub wire_menu: Option<((OutPinId, InPinId), egui::Pos2)>,
    /// The menu of a point of a connection: the connection, which point, and where.
    pub bend_menu: Option<((OutPinId, InPinId), usize, egui::Pos2)>,
    /// The ids of readers and writers in the order the user has put them in the navigator.
    pub navigator: Vec<String>,
    /// The variables of the model, used in its settings as $(name).
    pub variables: Vec<og_engine::Variable>,
    /// Whether the window with the variables is open.
    pub variables_open: bool,
    /// The window that connects an input or an output to PostGIS, when it is open.
    pub db_dialog: Option<crate::dbdialog::DbDialog>,
    /// The settings of the user, for all models: the database connections.
    pub settings: og_engine::settings::Settings,
    /// The settings window, when it is open.
    pub settings_window: Option<crate::settings::SettingsWindow>,
    /// The port that was moved last, and when: (node, output side, name, time).
    pub moved_port: Option<(NodeId, bool, String, f64)>,
    /// The ids of the readers and writers that are folded out in the navigator.
    nav_open: HashSet<String>,
    /// The earlier states of the workspace, to go back to with undo, and those undone.
    undo: Vec<Workspace>,
    redo: Vec<Workspace>,
    /// The workspace as it was when the latest change was taken in.
    undo_base: Workspace,
    /// Counts changes. When it differs from `taken_in`, something has changed since.
    revision: u64,
    taken_in: u64,
    /// How the table in the inspector is sorted: column and whether it is descending.
    pub sort: Option<(usize, bool)>,
    sorted: Option<Sorted>,
    /// The node and port the sorting belongs to. Another table starts unsorted.
    sort_of: (Option<String>, Option<String>),
    /// The Data tab shows statistics of the attributes instead of the features.
    pub show_stats: bool,
    /// The statistics of the table last shown, by the address of the table.
    stats_cache: Option<(usize, Vec<crate::stats::ColumnStats>)>,
    /// What a View shows in the map and in 3D: all its streams together, made once for the
    /// streams it has.
    together: Option<Together>,
    /// Every coordinate system of PROJ, for the forms.
    crs_list: Arc<Vec<og_engine::io::CrsEntry>>,
    /// Keeps changes that are not saved in a file now and then.
    keeper: crate::recovery::Keeper,
    /// Changes left behind by a program that stopped, to offer.
    pub recoverable: Vec<crate::recovery::Found>,
    /// Set by tests, so that "Show in folder" does not start a file manager. What would
    /// have been shown is kept in `shown`.
    pub quiet: bool,
    pub shown: Vec<PathBuf>,
    /// Files with several layers that wait for the user to choose among them.
    pub layer_choices: VecDeque<LayerChoice>,
    /// Whether the About window is open.
    pub about: bool,
    /// Whether the window with the history of the runs is open.
    pub history_open: bool,
    /// The lock of the model while it runs, and when the run started.
    run_lock: Option<og_engine::history::Lock>,
    /// The number of features of a trial run that is about to start.
    pub trial: Option<usize>,
    /// The filter of the data view for each node and port, while the program runs; the rows
    /// it let through the last time, and what was wrong with it.
    pub row_filter: HashMap<(String, String), String>,
    filtered: Option<data::Filtered>,
    filter_error: Option<String>,
    /// The quick-add box that is open: where on the screen, where on the canvas, and the
    /// node to connect the new one after, when it was opened from the keyboard.
    pub quick_add: Option<(egui::Pos2, egui::Pos2, Option<NodeId>)>,
    /// When and where the canvas was last clicked, to tell a double click.
    canvas_click: Option<(f64, egui::Pos2)>,
    /// The log of the run going on, written line by line beside the model, so that it is
    /// there to read also if the program stops before the run ends; with its file name.
    run_log: Option<(std::fs::File, String)>,
    run_started: Option<chrono::NaiveDateTime>,
    /// Whether the window with tips and shortcuts is open.
    pub tips: bool,
    /// The models that were opened or saved most recently.
    pub recent: crate::recent::Recent,
    /// The headings in the list of things to add that are folded out. All are folded in
    /// from the start.
    pub open_sections: HashSet<String>,
    /// The node whose parameters are shown: the selected node when exactly one is selected.
    pub focus: Option<NodeId>,
    /// The nodes that are selected on the canvas, as of the latest frame.
    pub selected: Vec<NodeId>,
    /// A selection to apply to the canvas in the next frame.
    select: Option<Vec<NodeId>>,
    pub view: View,
    pub tab: Tab,
    pub map: MapView,
    backend: Arc<GdalBackend>,
    pub transform: TSTransform,
    fit: Option<TSTransform>,
    /// Fit the nodes into the canvas in the next frame, at no less than this scale.
    fit_requested: Option<f32>,
    /// Frames left to fit in after loading. The nodes only get their true size after a
    /// couple of frames.
    fit_frames: u8,
    canvas_rect: Rect,
    /// The rectangles of the nodes in graph coordinates, from the latest frame.
    node_rects: Vec<(NodeId, Rect)>,
    /// What is typed in the search box above the list of node types.
    pub filter: String,
    flash: Option<(String, Instant)>,
    /// An action that waits for an answer about unsaved changes.
    pending: Option<Pending>,
    pub allow_close: bool,
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

/// The smallest scale used when a workspace is fitted on its own accord. Below this the
/// text in the nodes gets hard to read.
const READABLE_SCALE: f32 = 0.65;
/// The smallest scale the canvas allows. "Zoom to fit" may go this far to show everything.
const SMALLEST_SCALE: f32 = 0.3;
/// The settings of the user, which runs started from the window use as they are edited.
fn load_settings() -> og_engine::settings::Settings {
    let s = og_engine::settings::Settings::load().unwrap_or_default();
    og_engine::settings::use_settings(Some(s.clone()));
    s
}

/// Frees something in a thread of its own. The results of a large run take a long time to
/// free, and the window would freeze meanwhile.
pub fn drop_later<T: Send + 'static>(thing: Option<T>) {
    if let Some(t) = thing {
        std::thread::spawn(move || drop(t));
    }
}

/// A search field with a × at its end that empties it, shown when there is something in it.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> egui::Response {
    let output = egui::TextEdit::singleline(text).hint_text(hint).desired_width(width).margin(egui::Margin { left: 4, right: 22, top: 2, bottom: 2 }).show(ui);
    let mut response = output.response.response;
    let rect = response.rect;
    if !text.is_empty() {
        let spot = Rect::from_center_size(egui::pos2(rect.right() - 11.0, rect.center().y), egui::vec2(18.0, 18.0));
        let clear = ui.interact(spot, response.id.with("clear"), Sense::click());
        clear.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Clear the search"));
        let clear = clear.on_hover_text("Clear the search").on_hover_cursor(egui::CursorIcon::PointingHand);
        let color = if clear.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
        let c = spot.center();
        let s = 4.0;
        let stroke = egui::Stroke::new(1.5, color);
        ui.painter().line_segment([c - egui::vec2(s, s), c + egui::vec2(s, s)], stroke);
        ui.painter().line_segment([c + egui::vec2(-s, s), c + egui::vec2(s, -s)], stroke);
        if clear.clicked() {
            text.clear();
            response.request_focus();
            response.mark_changed();
        }
    }
    response
}

/// What the program is for, at the top of the panel on the right when nothing is selected.
const INTRO: &str = "Ographis is a tool for working with geodata: ETL, that is moving data between formats and coordinate systems and reshaping it on the way, automated map making, statistics, graphs and reports. A model is built from inputs, tools and outputs that you connect on the canvas, and does the same work every time it runs.";
const START: &str = "To start, fold out Input and output on the left and add an input, or drop a file or a folder on the window. Tips and shortcuts in the Help menu tell more.";

const UNTITLED: &str = "Untitled model";

impl App {
    pub fn new() -> App {
        App {
            snarl: Snarl::new(),
            name: UNTITLED.into(),
            description: String::new(),
            show_description: false,
            description_open: false,
            path: None,
            dirty: false,
            stale: false,
            result: None,
            running: None,
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stopping: false,
            stop_asked: None,
            pause: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            paused: false,
            // A new model reuses what has not changed, so that large files are read once.
            use_cache: true,
            cache_state: (f64::NEG_INFINITY, HashMap::new()),
            keys_now: (f64::NEG_INFINITY, HashMap::new()),
            last_command: None,
            clipboard: None,
            pointer_on_canvas: None,
            paste_at: None,
            previous: None,
            live: false,
            running_node: None,
            running_step: None,
            progress: (0, 0),
            groups: Vec::new(),
            selected_group: None,
            group_drag: None,
            expanded: HashSet::new(),
            lookup: Lookup::default(),
            selected_wire: None,
            wire_menu: None,
            bend_menu: None,
            navigator: Vec::new(),
            variables: Vec::new(),
            variables_open: false,
            db_dialog: None,
            settings: load_settings(),
            settings_window: None,
            moved_port: None,
            nav_open: HashSet::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            undo_base: graph::workspace_from(&Snarl::new(), UNTITLED, &[], &[]),
            revision: 0,
            taken_in: 0,
            sort: None,
            sorted: None,
            sort_of: (None, None),
            show_stats: false,
            stats_cache: None,
            together: None,
            crs_list: Arc::new(Vec::new()),
            keeper: crate::recovery::Keeper::default(),
            recoverable: Vec::new(),
            quiet: false,
            shown: Vec::new(),
            layer_choices: VecDeque::new(),
            about: false,
            history_open: false,
            run_lock: None,
            trial: None,
            quick_add: None,
            row_filter: HashMap::new(),
            filtered: None,
            filter_error: None,
            canvas_click: None,
            run_log: None,
            run_started: None,
            tips: false,
            recent: crate::recent::Recent::load(),
            open_sections: HashSet::new(),
            focus: None,
            selected: Vec::new(),
            select: None,
            view: View::default(),
            tab: Tab::Data,
            map: MapView::default(),
            backend: Arc::new(GdalBackend::new()),
            transform: TSTransform::IDENTITY,
            fit: None,
            fit_requested: Some(READABLE_SCALE),
            fit_frames: 0,
            canvas_rect: Rect::NOTHING,
            node_rects: Vec::new(),
            filter: String::new(),
            flash: None,
            pending: None,
            allow_close: false,
        }
    }

    /// A point on the canvas converted to window coordinates.
    /// Moves and zooms the canvas, as from the graph to the screen.
    pub fn set_canvas_transform(&mut self, t: egui::emath::TSTransform) {
        self.fit = Some(t);
    }

    pub fn graph_to_screen(&self, pos: egui::Pos2) -> egui::Pos2 {
        self.transform * pos
    }

    pub fn screen_to_graph(&self, pos: egui::Pos2) -> egui::Pos2 {
        self.transform.inverse() * pos
    }

    /// The part of the window that the canvas fills.
    pub fn canvas_rect(&self) -> Rect {
        self.canvas_rect
    }

    /// Shows all nodes, at the next frame. The same as the Zoom to fit button.
    pub fn fit_to_view(&mut self) {
        self.fit_requested = Some(SMALLEST_SCALE);
    }

    /// The current zoom of the canvas. 1 is full size.
    pub fn canvas_scale(&self) -> f32 {
        self.transform.scaling
    }

    pub fn workspace(&self) -> Workspace {
        let mut ws = graph::workspace_from(&self.snarl, &self.name, &self.groups, &self.navigator);
        ws.variables = self.variables.clone();
        ws.cache = self.use_cache;
        ws.description = self.description.clone();
        ws
    }

    /// The version of the program, as it is shown in the About window.
    /// The last part is left out when it is zero: 0.52.0 is shown as 0.52.
    pub fn version() -> &'static str {
        let v = env!("CARGO_PKG_VERSION");
        v.strip_suffix(".0").unwrap_or(v)
    }

    /// The folder that relative paths in the workspace start from.
    pub fn base_dir(&self) -> PathBuf {
        self.path
            .as_ref()
            .and_then(|p| p.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        let ws = Workspace::load(path).map_err(|e| e.0)?;
        self.take_in_model(ws, path);
        self.recent.add(path);
        Ok(())
    }

    /// Looks for changes left behind by a program that stopped, to offer them.
    pub fn look_for_recovery(&mut self) {
        self.recoverable = crate::recovery::left_behind();
    }

    /// Takes back changes that were left behind, as a model with changes not saved, in the
    /// file it came from, if any.
    pub fn restore_kept(&mut self, found: &crate::recovery::Found) -> Result<(), String> {
        let ws = Workspace::from_json(&found.model).map_err(|e| e.0)?;
        let path = found.path.clone().unwrap_or_else(|| PathBuf::from(""));
        self.take_in_model(ws, &path);
        if found.path.is_none() {
            self.path = None;
            if self.name.is_empty() {
                self.name = UNTITLED.into();
            }
        }
        self.dirty = true;
        crate::recovery::forget(found);
        Ok(())
    }

    fn take_in_model(&mut self, ws: Workspace, path: &Path) {
        self.snarl = graph::snarl_from(&ws);
        self.groups = ws.groups.clone();
        self.navigator = ws.navigator.clone();
        self.variables = ws.variables.clone();
        self.use_cache = ws.cache;
        self.description = ws.description.clone();
        self.show_description = !ws.description.trim().is_empty();
        self.name = if ws.name.is_empty() { path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { ws.name };
        self.path = Some(path.to_path_buf());
        self.reset_session();
    }

    fn reset_session(&mut self) {
        self.node_rects.clear();
        self.dirty = false;
        self.stale = false;
        drop_later(self.result.take());
        drop_later(self.previous.take());
        self.running = None;
        self.live = false;
        self.running_node = None;
        self.selected_group = None;
        self.selected_wire = None;
        self.group_drag = None;
        self.expanded.clear();
        self.nav_open.clear();
        self.layer_choices.clear();
        self.sort = None;
        // A workspace that was just opened has nothing to undo.
        self.undo.clear();
        self.redo.clear();
        self.undo_base = self.workspace();
        self.taken_in = self.revision;
        self.focus = None;
        self.selected.clear();
        self.select = Some(Vec::new());
        self.view = View::default();
        self.fit_requested = Some(READABLE_SCALE);
        self.fit_frames = 4;
    }

    fn new_workspace(&mut self) {
        self.snarl = Snarl::new();
        self.groups.clear();
        self.navigator.clear();
        self.name = UNTITLED.into();
        self.description.clear();
        self.show_description = false;
        self.path = None;
        self.use_cache = true;
        self.reset_session();
    }

    pub fn save_to(&mut self, path: &Path) {
        match std::fs::write(path, self.workspace().to_json()) {
            Ok(()) => {
                self.path = Some(path.to_path_buf());
                self.recent.add(path);
                self.dirty = false;
                self.say(format!("Saved as {}", path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
            }
            Err(e) => self.say(format!("Could not save: {}", e)),
        }
    }

    /// Saves. Asks for a file name if the workspace has none. True if it was saved.
    fn save(&mut self, ask: bool) -> bool {
        let path = match (&self.path, ask) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new().set_directory(self.base_dir()).set_file_name("model.ographis").add_filter("Ographis model", &["ographis"]).save_file(),
        };
        match path {
            Some(p) => {
                self.save_to(&p);
                !self.dirty
            }
            None => false,
        }
    }

    /// Draws the model as a picture to the clipboard, to paste in a mail, a chat or a
    /// presentation. Gives the size of the picture.
    pub fn copy_picture(&mut self, ctx: &egui::Context) -> Option<[usize; 2]> {
        match og_engine::modelpic::draw(&self.workspace(), 2.0) {
            Ok(img) => {
                let size = [img.width, img.height];
                ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied(size, &img.rgba));
                self.say(format!("The model was copied as a picture, {} × {} pixels. Paste it where you want it.", img.width, img.height));
                Some(size)
            }
            Err(e) => {
                self.say(e.0);
                None
            }
        }
    }

    /// Draws the model to a picture or a PDF that the user chooses.
    fn save_picture(&mut self) {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
        let Some(p) = rfd::FileDialog::new()
            .set_directory(self.base_dir())
            .set_file_name(format!("{}.png", name))
            .add_filter("PNG picture", &["png"])
            .add_filter("PDF", &["pdf"])
            .add_filter("JPEG picture", &["jpg"])
            .save_file()
        else {
            return;
        };
        let p = if p.extension().is_none() { p.with_extension("png") } else { p };
        match og_engine::modelpic::export(&self.workspace(), &p) {
            Ok(()) => self.say(format!("The model was drawn to {}", p.display())),
            Err(e) => self.say(e.0),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().set_directory(self.base_dir()).add_filter("Ographis model", &["ographis", "json"]).pick_file() {
            if let Err(e) = self.load(&p) {
                self.say(e);
            }
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), Instant::now()));
    }

    /// The message that is shown in the top bar right now, if any.
    pub fn message(&self) -> Option<&str> {
        self.flash.as_ref().filter(|f| f.1.elapsed().as_secs_f32() < 5.0).map(|f| f.0.as_str())
    }

    /// Notes that the workspace was changed. Every change goes through here, so that it
    /// can be undone.
    fn touch(&mut self) {
        self.dirty = true;
        self.revision += 1;
    }

    fn changed(&mut self, affects_result: bool) {
        self.touch();
        if affects_result {
            self.stale = true;
        }
    }
}

/// The title of the window: the name of the model, the file it is kept in with its folder,
/// and a dot when there are changes that are not saved.
pub fn window_title(app: &App) -> String {
    let dot = if app.dirty { " ●" } else { "" };
    match &app.path {
        Some(p) => format!("{}{} – {} – Ographis", app.name, dot, p.display()),
        None => format!("{}{} – not saved – Ographis", app.name, dot),
    }
}
