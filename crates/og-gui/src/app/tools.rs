//! Tools made of other tools: nodes made into a tool, the model inside a tool opened on the
//! canvas, and a tool unpacked into its nodes again.

use super::*;
use og_engine::toolmaking;

/// A model that is left while the model inside one of its tools is shown on the canvas.
pub struct Outer {
    /// The model as it was when the tool was opened.
    pub ws: Workspace,
    /// The id of the tool in it.
    pub node: String,
    /// The name of the tool, for the path above the canvas.
    pub label: String,
    undo: Vec<Workspace>,
    redo: Vec<Workspace>,
    transform: egui::emath::TSTransform,
}

impl App {
    /// The model that is on the canvas: the model, or the model inside a tool that is open.
    pub fn canvas_workspace(&self) -> Workspace {
        let mut ws = graph::workspace_from(&self.snarl, &self.name, &self.groups, &self.navigator);
        ws.variables = self.variables.clone();
        ws.cache = self.use_cache;
        ws.description = self.description.clone();
        ws
    }

    /// The names on the path above the canvas: the model, and each tool opened in it.
    pub fn tool_path(&self) -> Vec<String> {
        let mut out = vec![self.name.clone()];
        out.extend(self.inside.iter().map(|o| o.label.clone()));
        out
    }

    /// Whether the model inside a tool is on the canvas.
    pub fn in_tool(&self) -> bool {
        !self.inside.is_empty()
    }

    /// Shows the model inside a tool on the canvas, to change it.
    pub fn open_tool(&mut self, id: &str) {
        let Some(n) = graph::find(&self.snarl, id).and_then(|n| self.snarl.get_node(n)) else { return };
        if n.kind != "CustomTool" {
            return;
        }
        let (label, params) = (n.name.clone(), n.params.clone());
        self.take_in();
        let mut inner =
            og_engine::nodes::inner_model(&params).unwrap_or_else(|| Workspace::from_json(r#"{"format":"ographis/1","nodes":[],"edges":[]}"#).expect("empty"));
        let outer = self.canvas_workspace();
        inner.name = self.name.clone();
        inner.variables = outer.variables.clone();
        inner.cache = outer.cache;
        inner.description = outer.description.clone();
        self.inside.push(Outer {
            ws: outer,
            node: id.to_string(),
            label,
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            transform: self.transform,
        });
        self.show_level(&inner);
        self.fit_requested = Some(READABLE_SCALE);
        self.fit_frames = 4;
    }

    /// Puts the model on the canvas into its tool and shows the model around it again.
    /// Changes made inside are one step to undo there.
    pub fn close_tool(&mut self) {
        self.take_in();
        let Some(outer) = self.inside.pop() else { return };
        let mut inner = self.canvas_workspace();
        inner.name = outer.label.clone();
        let mut ws = outer.ws.clone();
        // What belongs to the model as a whole may have changed while inside.
        ws.variables = self.variables.clone();
        ws.cache = self.use_cache;
        ws.description = self.description.clone();
        toolmaking::put_inside(&mut ws, &outer.node, &inner);
        let changed = ws.to_json() != outer.ws.to_json();
        self.show_level(&ws);
        self.undo = outer.undo;
        self.redo = outer.redo;
        if changed {
            self.undo.push(outer.ws);
            self.redo.clear();
            self.stale = true;
        }
        self.transform = outer.transform;
        self.focus_node(&outer.node);
    }

    /// Goes out of every tool that is open, to the model itself.
    pub fn close_all_tools(&mut self) {
        while self.in_tool() {
            self.close_tool();
        }
    }

    /// Goes out to a level of the path above the canvas: 0 is the model.
    pub fn close_tools_to(&mut self, level: usize) {
        while self.inside.len() > level {
            self.close_tool();
        }
    }

    /// Shows a model on the canvas without counting it as a change.
    fn show_level(&mut self, ws: &Workspace) {
        let (dirty, stale, name) = (self.dirty, self.stale, self.name.clone());
        self.restore(ws);
        self.set_selection(Vec::new());
        (self.dirty, self.stale, self.name) = (dirty, stale, name);
        self.undo_base = self.canvas_workspace();
        self.taken_in = self.revision;
    }

    /// Makes the selected nodes into a tool.
    pub fn make_tool_of(&mut self, nodes: &[NodeId]) {
        let ids: Vec<String> = nodes.iter().filter_map(|n| self.snarl.get_node(*n)).filter(|n| !n.is_note()).map(|n| n.id.clone()).collect();
        if ids.is_empty() {
            self.say("Select the nodes to make into a tool.");
            return;
        }
        self.take_in();
        match toolmaking::make_tool(&self.canvas_workspace(), &ids, "New tool") {
            Ok((ws, id)) => {
                self.restore(&ws);
                self.changed(true);
                self.focus_node(&id);
                self.say(format!("{} nodes made into a tool. Double-click it to open it.", ids.len()));
            }
            Err(e) => self.say(e.0),
        }
    }

    /// Puts the nodes of a tool back in the model, in a box with its name.
    pub fn unpack_tool(&mut self, id: &str) {
        self.take_in();
        match toolmaking::unpack_tool(&self.canvas_workspace(), id) {
            Ok(ws) => {
                self.restore(&ws);
                self.set_selection(Vec::new());
                self.changed(true);
            }
            Err(e) => self.say(e.0),
        }
    }

    /// The path above the canvas while a tool is open: the model and the tools, each to
    /// click to go back to.
    pub(super) fn tool_path_bar(&mut self, ui: &mut Ui, pal: &Palette) {
        if !self.in_tool() {
            return;
        }
        let path = self.tool_path();
        let mut go = None;
        egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Back").on_hover_text("Puts what was changed into the tool and goes back out (Esc)").clicked() {
                    go = Some(path.len() - 2);
                }
                for (k, name) in path.iter().enumerate() {
                    if k > 0 {
                        ui.label(RichText::new("›").color(pal.muted));
                    }
                    if k + 1 == path.len() {
                        ui.label(RichText::new(name).strong());
                    } else if ui.link(name).clicked() {
                        go = Some(k);
                    }
                }
                ui.label(RichText::new("Inside a tool: Tool input and Tool output nodes are its ports").small().color(pal.muted));
            });
        });
        if let Some(level) = go {
            self.close_tools_to(level);
        }
    }
}
