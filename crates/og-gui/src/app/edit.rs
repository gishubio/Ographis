//! Selecting, adding, removing and duplicating nodes, undo and redo, and copy, cut and paste,
//! also between windows.

use super::*;

impl App {
    /// Selects one node, shows its parameters, and shows its data in the inspector.
    pub fn focus_node(&mut self, id: &str) {
        if let Some(n) = graph::find(&self.snarl, id) {
            self.set_selection(vec![n]);
            // A note has no data to show, so the inspector keeps what it had.
            if self.view.node.as_deref() != Some(id) && !self.snarl[n].is_note() {
                self.view = View { node: Some(id.to_string()), port: None, feature: None, more: Vec::new() };
            }
        }
    }

    /// Replaces the selection, here and on the canvas.
    pub fn set_selection(&mut self, nodes: Vec<NodeId>) {
        if !nodes.is_empty() {
            self.selected_group = None;
            self.selected_wire = None;
        }
        self.focus = if nodes.len() == 1 { Some(nodes[0]) } else { None };
        self.selected = nodes.clone();
        self.select = Some(nodes);
    }

    /// Follows what the user selected on the canvas.
    pub(super) fn selection_changed(&mut self, nodes: Vec<NodeId>) {
        self.selected = nodes;
        if !self.selected.is_empty() {
            self.selected_group = None;
            self.selected_wire = None;
        }
        self.focus = if self.selected.len() == 1 { Some(self.selected[0]) } else { None };
        // One selected node is also what the inspector shows. With none or several
        // selected, the inspector keeps showing what it had. A note has no data to show.
        if let Some(id) = self.focus.and_then(|n| self.snarl.get_node(n)).filter(|n| !n.is_note()).map(|n| n.id.clone()) {
            if self.view.node.as_deref() != Some(id.as_str()) {
                self.view = View { node: Some(id), port: None, feature: None, more: Vec::new() };
            }
        }
    }

    pub(super) fn remove_nodes(&mut self, nodes: &[NodeId]) {
        let (mut any, mut part_of_run) = (false, false);
        for &node in nodes {
            if self.snarl.get_node(node).is_none() {
                continue;
            }
            let gone = self.snarl.remove_node(node);
            if self.view.node.as_deref() == Some(gone.id.as_str()) {
                self.view = View::default();
            }
            self.expanded.remove(&gone.id);
            any = true;
            part_of_run |= !gone.is_note();
        }
        if any {
            let left: Vec<NodeId> = self.selected.iter().copied().filter(|n| self.snarl.get_node(*n).is_some()).collect();
            self.set_selection(left);
            self.changed(part_of_run);
        }
    }

    pub(super) fn duplicate_nodes(&mut self, nodes: &[NodeId]) {
        let copies = graph::duplicate(&mut self.snarl, nodes);
        if copies.is_empty() {
            return;
        }
        let part_of_run = copies.iter().any(|c| !self.snarl[*c].is_note());
        if let [one] = copies[..] {
            let id = self.snarl[one].id.clone();
            self.focus_node(&id);
        } else {
            self.set_selection(copies);
        }
        self.changed(part_of_run);
    }

    /// The nodes an action on `node` applies to: the whole selection if the node is part
    /// of it, otherwise just the node.
    pub(super) fn targets(&self, node: NodeId) -> Vec<NodeId> {
        if self.selected.contains(&node) {
            self.selected.clone()
        } else {
            vec![node]
        }
    }

    /// Lines up the selected nodes, as one step that can be undone. The boxes around them stay.
    pub fn align_selected(&mut self, how: graph::Align) {
        let nodes: Vec<(NodeId, egui::Rect)> = self.selected.iter().filter_map(|n| self.node_rects.iter().find(|r| r.0 == *n).copied()).collect();
        if nodes.len() < 2 {
            return;
        }
        let rects: Vec<egui::Rect> = nodes.iter().map(|n| n.1).collect();
        let moves = graph::alignment(&rects, how);
        let mut any = false;
        for ((n, _), by) in nodes.iter().zip(moves) {
            if by != egui::Vec2::ZERO {
                if let Some(info) = self.snarl.get_node_info_mut(*n) {
                    info.pos += by;
                    any = true;
                }
            }
        }
        if any {
            self.touch();
        }
    }

    pub fn add_node(&mut self, kind: &str) {
        let pos = self.free_spot();
        let node = graph::new_node(&self.snarl, kind);
        let id = node.id.clone();
        self.snarl.insert_node(pos, node);
        self.focus_node(&id);
        self.changed(kind != NOTE);
    }

    // ---------- Undo and redo ----------

    /// Takes in what has changed since the last time as one step that can be undone.
    pub(super) fn take_in(&mut self) {
        if self.revision == self.taken_in {
            return;
        }
        self.taken_in = self.revision;
        let now = self.workspace();
        if now.to_json() == self.undo_base.to_json() {
            return;
        }
        self.undo.push(std::mem::replace(&mut self.undo_base, now));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Changes are taken in when the hands are off: a drag is one step, and so is what is
    /// typed in a field before leaving it.
    pub(super) fn take_in_when_idle(&mut self, ctx: &egui::Context) {
        if self.revision != self.taken_in && !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
            self.take_in();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.revision != self.taken_in
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty() && self.revision == self.taken_in
    }

    pub fn undo(&mut self) {
        self.take_in();
        if let Some(earlier) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.undo_base, earlier.clone()));
            self.restore(&earlier);
        }
    }

    pub fn redo(&mut self) {
        self.take_in();
        if let Some(later) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.undo_base, later.clone()));
            self.restore(&later);
        }
    }

    /// Puts the workspace back to a state it has been in. What is selected stays
    /// selected if it is still there, and the result of the latest run is kept, marked
    /// as out of date.
    pub(super) fn restore(&mut self, ws: &Workspace) {
        let selected: Vec<String> = self.selected.iter().filter_map(|n| self.snarl.get_node(*n)).map(|n| n.id.clone()).collect();
        self.snarl = graph::snarl_from(ws);
        self.groups = ws.groups.clone();
        self.navigator = ws.navigator.clone();
        self.variables = ws.variables.clone();
        self.use_cache = ws.cache;
        self.name = ws.name.clone();
        self.description = ws.description.clone();
        self.node_rects.clear();
        self.selected_group = None;
        self.selected_wire = None;
        self.group_drag = None;
        let still: Vec<NodeId> = selected.iter().filter_map(|id| graph::find(&self.snarl, id)).collect();
        self.set_selection(still);
        self.dirty = true;
        self.stale = self.result.is_some();
        self.revision += 1;
        self.taken_in = self.revision;
    }

    /// Puts the selected nodes on the clipboard, as a small model. Cut removes them too.
    pub fn copy_selected(&mut self, ctx: &egui::Context, cut: bool) {
        if self.selected.is_empty() {
            return;
        }
        // The group boxes go along whose contents are all copied.
        let boxes = self.boxes_with_selection();
        let groups: Vec<GroupBox> = boxes.iter().filter_map(|g| self.groups.get(*g).cloned()).collect();
        let Some(json) = graph::copy_json(&self.snarl, &self.selected, &groups, &self.base_dir()) else { return };
        ctx.copy_text(json.clone());
        self.clipboard = Some(json);
        let n = self.selected.len();
        let what = if n == 1 { "1 node".to_string() } else { format!("{} nodes", n) };
        if cut {
            // The boxes go with what they hold.
            for g in boxes.iter().rev() {
                if *g < self.groups.len() {
                    self.groups.remove(*g);
                }
            }
            self.selected_group = None;
            self.remove_nodes(&self.selected.clone());
            self.say(format!("Cut {}. Ctrl+V puts it back, here or in another window.", what));
        } else {
            self.say(format!("Copied {}. Ctrl+V pastes, here or in another window of Ographis.", what));
        }
    }

    /// Pastes nodes copied in this or another window of Ographis, or a whole model file
    /// copied as text. They go where the pointer is on the canvas, or else in a free spot.
    pub fn paste(&mut self, text: &str) {
        let at = self.paste_at.take().or_else(|| self.pointer_on_canvas.map(|p| self.transform.inverse() * p)).unwrap_or_else(|| self.free_spot());
        let base = self.base_dir();
        match graph::paste_json(&mut self.snarl, text, at, &base) {
            Some((added, groups)) if !added.is_empty() => {
                let part_of_run = added.iter().any(|n| !self.snarl[*n].is_note());
                let n = added.len();
                self.groups.extend(groups);
                self.set_selection(added);
                self.changed(part_of_run);
                self.say(if n == 1 { "Pasted 1 node.".to_string() } else { format!("Pasted {} nodes.", n) });
            }
            _ => self.say("The clipboard holds no nodes. Copy nodes in Ographis with Ctrl+C first.".to_string()),
        }
    }

    /// What is on the clipboard of the system, for Paste in the menus. While testing, and
    /// when the system clipboard cannot be read, what this window copied last.
    pub fn clipboard_text(&self) -> Option<String> {
        if !self.quiet {
            if let Some(t) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()) {
                return Some(t);
            }
        }
        self.clipboard.clone()
    }

    /// Paste in a menu.
    pub fn paste_from_menu(&mut self, at: Option<egui::Pos2>) {
        match self.clipboard_text() {
            Some(t) => {
                self.paste_at = at;
                self.paste(&t);
            }
            None => self.say("The clipboard is empty. Copy nodes in Ographis with Ctrl+C first.".to_string()),
        }
    }

    /// Removes what is selected: the nodes, or else the connection, or else the group box.
    pub(super) fn delete_selected(&mut self) {
        if !self.selected.is_empty() {
            self.remove_nodes(&self.selected.clone());
        } else if self.selected_wire.is_some() {
            self.remove_selected_wire();
        } else if let Some(g) = self.selected_group {
            self.remove_group(g);
        }
    }

    /// Carries out an action that discards the workspace, or asks first if there are
    /// unsaved changes.
    pub(super) fn request(&mut self, action: Pending) {
        if self.dirty {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    pub(super) fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.new_workspace(),
            Pending::Open => self.open_dialog(),
            Pending::OpenPath(p) => {
                if let Err(e) = self.load(&p) {
                    self.say(e);
                }
            }
            Pending::Close => self.allow_close = true,
        }
    }

    pub(super) fn close_guard(&mut self, ctx: &egui::Context) {
        let asked = ctx.input(|i| i.viewport().close_requested());
        if asked && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Close);
            return;
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if asked || self.allow_close {
            // Closed as it should be: the kept changes were saved or let go of.
            self.keeper.clear();
            self.let_go_of_data();
            return;
        }
        let mut keeper = std::mem::take(&mut self.keeper);
        keeper.tick(self.dirty, &self.name, self.path.as_deref(), || self.workspace().to_json());
        self.keeper = keeper;
    }

    /// The program is closing. Freeing the data of a large run one piece at a time can take
    /// minutes, and the system frees it all at once when the program ends. So it is let
    /// go of without being freed.
    pub fn let_go_of_data(&mut self) {
        std::mem::forget(self.result.take());
        std::mem::forget(self.previous.take());
        self.map.let_go_of_data();
    }
}
