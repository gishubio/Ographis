//! Group boxes and the notes on the canvas: moved, resized, with their nodes or alone.

use super::*;

impl App {
    // ---------- Group boxes ----------

    /// The nodes that lie in a group box: those whose middle is inside it.
    pub fn nodes_in_group(&self, index: usize) -> Vec<NodeId> {
        let Some(area) = self.groups.get(index).map(group_rect) else { return Vec::new() };
        self.snarl
            .nodes_pos_ids()
            .filter(|(id, pos, _)| {
                let rect = self.node_rects.iter().find(|r| r.0 == *id).map_or(Rect::from_min_size(*pos, egui::vec2(190.0, 110.0)), |r| r.1);
                area.contains(rect.center())
            })
            .map(|(id, _, _)| id)
            .collect()
    }

    /// The boxes that go along when the selected nodes are dragged: every box if everything
    /// is selected, and otherwise the boxes where all that lies inside is selected. A single
    /// node moves alone, so that it can be taken out of its box.
    pub fn boxes_with_selection(&self) -> Vec<usize> {
        if self.selected.len() < 2 {
            return Vec::new();
        }
        let everything = self.snarl.nodes().count() == self.selected.len();
        (0..self.groups.len())
            .filter(|g| {
                let inside = self.nodes_in_group(*g);
                everything || (!inside.is_empty() && inside.iter().all(|n| self.selected.contains(n)))
            })
            .collect()
    }

    pub(super) fn group_members(&self, index: usize) -> GroupDrag {
        let area = group_rect(&self.groups[index]);
        // A smaller box that lies wholly inside follows as well.
        let groups = (0..self.groups.len()).filter(|k| *k != index && area.contains_rect(group_rect(&self.groups[*k]))).collect();
        GroupDrag { index, nodes: self.nodes_in_group(index), groups, with_selection: false }
    }

    /// What goes along when a box that belongs to the selection is dragged: the selected
    /// nodes and every box that goes with them.
    pub(super) fn selection_drag(&self, index: usize) -> Option<GroupDrag> {
        let boxes = self.boxes_with_selection();
        if !boxes.contains(&index) {
            return None;
        }
        Some(GroupDrag { index, nodes: self.selected.clone(), groups: boxes.into_iter().filter(|k| *k != index).collect(), with_selection: true })
    }

    pub(super) fn select_group(&mut self, index: usize) {
        if index < self.groups.len() {
            self.set_selection(Vec::new());
            self.selected_wire = None;
            self.selected_group = Some(index);
        }
    }

    /// Moves a group box, and with it what lies inside, if the box is locked.
    pub fn move_group(&mut self, index: usize, by: egui::Vec2) {
        if index >= self.groups.len() || by == egui::Vec2::ZERO {
            return;
        }
        let members = match self.group_drag.take() {
            Some(d) if d.index == index => d,
            _ => self.group_members(index),
        };
        let shift = |g: &mut GroupBox| {
            g.x += by.x as f64;
            g.y += by.y as f64;
        };
        shift(&mut self.groups[index]);
        if self.groups[index].locked || members.with_selection {
            for node in &members.nodes {
                if let Some(info) = self.snarl.get_node_info_mut(*node) {
                    info.pos += by;
                }
            }
            for k in &members.groups {
                shift(&mut self.groups[*k]);
            }
            // The bend points of the connections between them go along too.
            graph::shift_bends(&mut self.snarl, &members.nodes, by);
        }
        self.group_drag = Some(members);
        self.touch();
    }

    /// Resizes a group box from the edges or the corner that is dragged. The nodes stay.
    pub(super) fn resize_group(&mut self, index: usize, by: egui::Vec2, sides: graph::Sides) {
        if let Some(g) = self.groups.get_mut(index) {
            let b = sides.apply([g.x as f32, g.y as f32, g.w as f32, g.h as f32], by, egui::vec2(120.0, GROUP_TITLE + 34.0));
            (g.x, g.y, g.w, g.h) = (b[0] as f64, b[1] as f64, b[2] as f64, b[3] as f64);
            self.touch();
        }
    }

    /// Moves or resizes the box of a comment. The box keeps its place next to its node.
    pub(super) fn comment_event(&mut self, ctx: &egui::Context, node: NodeId, event: graph::CommentEvent) {
        // A note: its text area grows or shrinks, and a left or top edge moves the note too.
        if let graph::CommentEvent::NoteResized(by, sides, text) = event {
            // The size kept in the note is the truth; the size the text was drawn at lags a
            // frame behind and is only the start, before the note has been resized.
            let kept = self.snarl.get_node(node).and_then(|n| n.comment_box);
            let (w, h) = match kept {
                Some([_, _, w, h]) if w > 0.0 => (w, if h > 0.0 { h } else { text.y }),
                _ => (text.x, text.y),
            };
            let b = sides.apply([0.0, 0.0, w, h], by, egui::vec2(graph::NOTE_MIN.0, graph::NOTE_MIN.1));
            if let Some(info) = self.snarl.get_node_info_mut(node) {
                info.pos += egui::vec2(b[0], b[1]);
                info.value.comment_box = Some([0.0, 0.0, b[2], b[3]]);
            }
            self.touch();
            return;
        }
        let size = self.node_rects.iter().find(|r| r.0 == node).map_or(egui::vec2(170.0, 80.0), |r| r.1.size());
        let Some(n) = self.snarl.get_node_mut(node) else { return };
        let b = n.comment_box.unwrap_or_else(|| graph::default_comment_box(ctx, &n.comment, size));
        let b = match event {
            graph::CommentEvent::Clicked => {
                self.set_selection(vec![node]);
                return;
            }
            graph::CommentEvent::Moved(by) => [b[0] + by.x, b[1] + by.y, b[2], b[3]],
            graph::CommentEvent::Resized(by, sides) => sides.apply(b, by, graph::comment_min_size(ctx, &n.comment)),
            graph::CommentEvent::NoteResized(..) => return,
        };
        n.comment_box = Some(b);
        self.touch();
    }

    pub fn remove_group(&mut self, index: usize) {
        if index < self.groups.len() {
            self.groups.remove(index);
            self.selected_group = None;
            self.group_drag = None;
            self.touch();
        }
    }

    /// The rectangle that holds these nodes with room for the name of the group above them.
    pub(super) fn frame_around(&self, nodes: &[NodeId]) -> Option<Rect> {
        let mut bounds: Option<Rect> = None;
        for (id, pos, _) in self.snarl.nodes_pos_ids().filter(|(id, _, _)| nodes.contains(id)) {
            let rect = self.node_rects.iter().find(|r| r.0 == id).map_or(Rect::from_min_size(pos, egui::vec2(190.0, 110.0)), |r| r.1);
            bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
        }
        bounds.map(|b| Rect::from_min_max(b.min - egui::vec2(18.0, GROUP_TITLE + 16.0), b.max + egui::vec2(18.0, 18.0)))
    }

    /// Puts a group box around these nodes. With no nodes, an empty box in the middle of
    /// what is shown. Returns the index of the new box.
    pub fn add_group(&mut self, nodes: &[NodeId], at: Option<egui::Pos2>) -> usize {
        let rect = self.frame_around(nodes).unwrap_or_else(|| {
            let size = egui::vec2(320.0, 220.0);
            let corner = at.unwrap_or_else(|| {
                if self.canvas_rect.is_finite() {
                    self.transform.inverse() * self.canvas_rect.center() - size / 2.0
                } else {
                    egui::pos2(40.0, 40.0)
                }
            });
            Rect::from_min_size(corner, size)
        });
        let mut name = "Group".to_string();
        let mut k = 2;
        while self.groups.iter().any(|g| g.name == name) {
            name = format!("Group {}", k);
            k += 1;
        }
        let round = |v: f32| v.round() as f64;
        self.groups.push(GroupBox {
            name,
            x: round(rect.min.x),
            y: round(rect.min.y),
            w: round(rect.width()),
            h: round(rect.height()),
            color: String::new(),
            locked: true,
        });
        let index = self.groups.len() - 1;
        self.select_group(index);
        self.touch();
        index
    }

    /// Makes a group box just large enough for what lies in it.
    pub(super) fn fit_group(&mut self, index: usize) {
        if let Some(rect) = self.frame_around(&self.nodes_in_group(index)) {
            let g = &mut self.groups[index];
            (g.x, g.y, g.w, g.h) = (rect.min.x.round() as f64, rect.min.y.round() as f64, rect.width().round() as f64, rect.height().round() as f64);
            self.touch();
        }
    }
}
