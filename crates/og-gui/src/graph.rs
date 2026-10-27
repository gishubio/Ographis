//! The node graph on the canvas: conversion to and from the workspace format, and how
//! nodes, ports and menus are drawn.

use crate::theme::{self, Palette};
use eframe::egui::{self, emath::TSTransform, Color32, Pos2, Rect, RichText, Stroke, Ui};
use egui_snarl::ui::{AnyPins, BackgroundPattern, NodeLayout, PinInfo, PinPlacement, SelectionStyle, SnarlStyle, SnarlViewer, WireLayer, WireStyle};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use og_engine::model::fmt_count;
use og_engine::nodes::{node_type, Group, NodeType, Params, NODE_TYPES, P};
use og_engine::{Edge, GroupBox, Node, Note, RunResult, Status, Table, Workspace};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The kind of a free-standing note. It is a node on the canvas, so that it can be moved,
/// selected and deleted like the others, but it has no ports and is not part of the run.
pub const NOTE: &str = "Note";

/// The id of the canvas widget. The selection is stored under it.
pub fn canvas_id() -> egui::Id {
    egui::Id::new("canvas")
}

/// A node on the canvas. Its position lives in the graph, the rest here.
#[derive(Clone, Debug)]
pub struct GNode {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub params: Params,
    /// A comment about the node, shown in a box tied to it. For a note, the text of the note.
    pub comment: String,
    /// Where the box with the comment is, from the upper left corner of the node: x, y,
    /// width and height. None until it has been moved or resized.
    pub comment_box: Option<[f32; 4]>,
    /// A disabled node does not run and lets nothing through.
    pub disabled: bool,
    /// The connections into this node that are disabled: the node and port they come from,
    /// and the port they go to here.
    pub off_wires: Vec<(String, String, String)>,
    /// The points that connections into this node are drawn through, by the same key.
    pub bends: Vec<(WireKey, Vec<[f32; 2]>)>,
}

impl GNode {
    pub fn is_note(&self) -> bool {
        self.kind == NOTE
    }
    pub fn ty(&self) -> Option<&'static NodeType> {
        node_type(&self.kind)
    }
    /// The output ports of the node, in the order they are shown. For most tools they are
    /// fixed; for some they follow the settings, such as one per value in Filter by value.
    pub fn outputs(&self) -> Vec<String> {
        ordered(self.ty().map(|t| t.outputs_for(&self.params)).unwrap_or_default(), self.port_order("out"))
    }
    /// The input ports of the node, in the order they are shown.
    pub fn inputs(&self) -> Vec<String> {
        ordered(self.ty().map(|t| t.inputs.iter().map(|p| p.to_string()).collect()).unwrap_or_default(), self.port_order("in"))
    }
    /// The order the user has given the ports on one side, if any. It is kept with the
    /// parameters, under a key the engine does not read.
    fn port_order(&self, side: &str) -> Vec<String> {
        self.params
            .get(PORT_ORDER)
            .and_then(|o| o.get(side))
            .and_then(|l| l.as_array())
            .map(|l| l.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }
    /// Moves a port one step up or down on its side. Returns whether it moved.
    pub fn move_port(&mut self, output: bool, index: usize, up: bool) -> bool {
        let mut ports = if output { self.outputs() } else { self.inputs() };
        let other = if up { index.checked_sub(1) } else { Some(index + 1).filter(|i| *i < ports.len()) };
        let Some(other) = other.filter(|_| index < ports.len()) else { return false };
        ports.swap(index, other);
        let side = if output { "out" } else { "in" };
        let mut order = match self.params.remove(PORT_ORDER) {
            Some(serde_json::Value::Object(o)) => o,
            _ => serde_json::Map::new(),
        };
        order.insert(side.to_string(), serde_json::Value::Array(ports.into_iter().map(serde_json::Value::String).collect()));
        self.params.insert(PORT_ORDER.to_string(), serde_json::Value::Object(order));
        true
    }
    pub fn summary(&self) -> String {
        match self.ty() {
            Some(t) => (t.summary)(&P::new(&self.params, t.params)),
            None => format!("unknown node type: {}", self.kind),
        }
    }
}

pub fn snarl_from(ws: &Workspace) -> Snarl<GNode> {
    let mut snarl = Snarl::new();
    let mut ids = std::collections::HashMap::new();
    for n in &ws.nodes {
        let node = GNode {
            id: n.id.clone(),
            kind: n.kind.clone(),
            name: n.label().to_string(),
            params: n.params.clone(),
            comment: n.comment.clone(),
            comment_box: n.comment_box.map(|b| b.map(|v| v as f32)),
            disabled: n.disabled,
            off_wires: ws.edges.iter().filter(|e| e.disabled && e.to.0 == n.id).map(|e| (e.from.0.clone(), e.from.1.clone(), e.to.1.clone())).collect(),
            bends: ws
                .edges
                .iter()
                .filter(|e| !e.points.is_empty() && e.to.0 == n.id)
                .map(|e| ((e.from.0.clone(), e.from.1.clone(), e.to.1.clone()), e.points.iter().map(|p| [p[0] as f32, p[1] as f32]).collect()))
                .collect(),
        };
        let ports = (node.outputs(), node.inputs());
        ids.insert(n.id.clone(), (snarl.insert_node(Pos2::new(n.x as f32, n.y as f32), node), ports));
    }
    for note in &ws.notes {
        let mut node = new_node(&snarl, NOTE);
        node.comment = note.text.clone();
        if let (Some(w), Some(h)) = (note.w, note.h) {
            node.comment_box = Some([0.0, 0.0, w as f32, h as f32]);
        }
        snarl.insert_node(Pos2::new(note.x as f32, note.y as f32), node);
    }
    for e in &ws.edges {
        let (Some((from, (outputs, _))), Some((to, (_, inputs)))) = (ids.get(&e.from.0), ids.get(&e.to.0)) else { continue };
        let out = outputs.iter().position(|p| *p == e.from.1);
        let inp = inputs.iter().position(|p| *p == e.to.1);
        if let (Some(output), Some(input)) = (out, inp) {
            snarl.connect(OutPinId { node: *from, output }, InPinId { node: *to, input });
        }
    }
    snarl
}

/// Whether a node is disabled, so that nothing goes in or out of it.
fn node_off(snarl: &Snarl<GNode>, node: NodeId) -> bool {
    snarl.get_node(node).is_some_and(|n| n.disabled)
}

/// A connection as the node it goes to knows it: the node and port it comes from, and the
/// port it goes to.
pub type WireKey = (String, String, String);

/// The key a connection has in `off_wires` of the node it goes to.
fn wire_key(snarl: &Snarl<GNode>, from: OutPinId, to: InPinId) -> Option<(String, String, String)> {
    let (a, b) = (snarl.get_node(from.node)?, snarl.get_node(to.node)?);
    Some((a.id.clone(), a.outputs().get(from.output)?.clone(), b.inputs().get(to.input)?.clone()))
}

/// The points a connection is drawn through.
pub fn wire_bends(snarl: &Snarl<GNode>, from: OutPinId, to: InPinId) -> Vec<Pos2> {
    let Some(k) = wire_key(snarl, from, to) else { return Vec::new() };
    snarl[to.node].bends.iter().find(|b| b.0 == k).map(|b| b.1.iter().map(|p| Pos2::new(p[0], p[1])).collect()).unwrap_or_default()
}

/// Moves the bend points of the connections between these nodes, as the nodes are moved.
/// A connection with only one end among them keeps its points.
pub fn shift_bends(snarl: &mut Snarl<GNode>, nodes: &[NodeId], by: egui::Vec2) {
    let ids: std::collections::HashSet<String> = nodes.iter().filter_map(|n| snarl.get_node(*n)).map(|n| n.id.clone()).collect();
    for n in nodes {
        if let Some(node) = snarl.get_node_mut(*n) {
            for (key, pts) in node.bends.iter_mut() {
                if ids.contains(&key.0) {
                    for p in pts.iter_mut() {
                        p[0] += by.x;
                        p[1] += by.y;
                    }
                }
            }
        }
    }
}

/// Gives a connection the points it is drawn through. None takes them all away.
pub fn set_wire_bends(snarl: &mut Snarl<GNode>, from: OutPinId, to: InPinId, points: Vec<Pos2>) {
    let Some(k) = wire_key(snarl, from, to) else { return };
    let list = &mut snarl[to.node].bends;
    list.retain(|b| b.0 != k);
    if !points.is_empty() {
        list.push((k, points.iter().map(|p| [p.x, p.y]).collect()));
    }
}

/// Where a new point goes among the points of a connection: between the two it lies
/// nearest the line between, so that the curve goes through it with the least detour.
pub fn insert_bend(snarl: &Snarl<GNode>, from: OutPinId, to: InPinId, start: Pos2, end: Pos2, at: Pos2) -> Vec<Pos2> {
    let mut pts = wire_bends(snarl, from, to);
    let mut all = vec![start];
    all.extend(pts.iter().copied());
    all.push(end);
    let detour = |i: usize| (all[i] - at).length() + (at - all[i + 1]).length() - (all[i + 1] - all[i]).length();
    let best = (0..all.len() - 1).min_by(|a, b| detour(*a).total_cmp(&detour(*b))).unwrap_or(0);
    pts.insert(best, at);
    pts
}

/// Whether a connection is turned off.
pub fn wire_disabled(snarl: &Snarl<GNode>, from: OutPinId, to: InPinId) -> bool {
    wire_key(snarl, from, to).is_some_and(|k| snarl[to.node].off_wires.contains(&k))
}

/// Turns a connection off or on.
pub fn set_wire_disabled(snarl: &mut Snarl<GNode>, from: OutPinId, to: InPinId, off: bool) {
    let Some(k) = wire_key(snarl, from, to) else { return };
    let list = &mut snarl[to.node].off_wires;
    list.retain(|w| *w != k);
    if off {
        list.push(k);
    }
}

pub fn workspace_from(snarl: &Snarl<GNode>, name: &str, groups: &[GroupBox], navigator: &[String]) -> Workspace {
    let mut nodes: Vec<Node> = snarl
        .nodes_pos_ids()
        .filter(|(_, _, n)| !n.is_note())
        .map(|(_, pos, n)| Node {
            id: n.id.clone(),
            kind: n.kind.clone(),
            name: n.name.clone(),
            x: pos.x.round() as f64,
            y: pos.y.round() as f64,
            params: n.params.clone(),
            comment: n.comment.trim().to_string(),
            // The place of the box is kept only while there is a comment to put in it.
            comment_box: n.comment_box.filter(|_| !n.comment.trim().is_empty()).map(|b| b.map(|v| v.round() as f64)),
            disabled: n.disabled,
        })
        .collect();
    nodes.sort_by(|a, b| (a.x, a.y).partial_cmp(&(b.x, b.y)).unwrap_or(std::cmp::Ordering::Equal));
    let mut notes: Vec<Note> = snarl
        .nodes_pos_ids()
        .filter(|(_, _, n)| n.is_note())
        .map(|(_, pos, n)| Note {
            text: n.comment.clone(),
            x: pos.x.round() as f64,
            y: pos.y.round() as f64,
            w: n.comment_box.map(|b| b[2].round() as f64),
            h: n.comment_box.map(|b| b[3].round() as f64),
        })
        .collect();
    notes.sort_by(|a, b| (a.x, a.y).partial_cmp(&(b.x, b.y)).unwrap_or(std::cmp::Ordering::Equal));
    let mut edges = Vec::new();
    for (out, inp) in snarl.wires() {
        let (Some(a), Some(b)) = (snarl.get_node(out.node), snarl.get_node(inp.node)) else { continue };
        let (Some(op), Some(ip)) = (a.outputs().get(out.output).cloned(), b.inputs().get(inp.input).cloned()) else { continue };
        let disabled = b.off_wires.iter().any(|w| w.0 == a.id && w.1 == op && w.2 == ip);
        let points = b
            .bends
            .iter()
            .find(|w| w.0 .0 == a.id && w.0 .1 == op && w.0 .2 == ip)
            .map(|w| w.1.iter().map(|p| [p[0].round() as f64, p[1].round() as f64]).collect())
            .unwrap_or_default();
        edges.push(Edge { from: (a.id.clone(), op), to: (b.id.clone(), ip), disabled, points });
    }
    edges.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    // The order in the navigator only lists nodes that are still there.
    let navigator = navigator.iter().filter(|id| nodes.iter().any(|n| &n.id == *id)).cloned().collect();
    // Like the nodes, the boxes are stored in whole units. Fractions of a pixel say nothing
    // and do not survive being written and read.
    let groups = groups.iter().map(|g| GroupBox { x: g.x.round(), y: g.y.round(), w: g.w.round(), h: g.h.round(), ..g.clone() }).collect();
    Workspace {
        format: og_engine::run::FORMAT.to_string(),
        name: name.to_string(),
        nodes,
        edges,
        groups,
        notes,
        navigator,
        variables: Vec::new(),
        cache: false,
    }
}

/// The line under the ports of a node: the node type if the node has a name of its own,
/// and how it is set up.
fn body_text(n: &GNode) -> String {
    let summary = n.summary();
    let renamed = n.ty().is_some() && !has_default_name(n);
    match (renamed, summary.is_empty()) {
        (true, false) => format!("{} · {}", title_of(&n.kind), summary),
        (true, true) => title_of(&n.kind).to_string(),
        (false, _) => summary,
    }
}

/// The name a tool is shown by: what it does, such as "Sort". A type that this version
/// does not know is shown by the name in the file.
pub fn title_of(kind: &str) -> &str {
    node_type(kind).map_or(kind, |t| t.title)
}

/// Whether the node has the name it was given when it was added, the name of its tool
/// alone or with a number, and not one the user chose.
pub fn has_default_name(n: &GNode) -> bool {
    og_engine::run::default_name_today(&n.kind, &n.name).is_some()
}

/// The tools of the given groups that are offered, by name in alphabetical order.
pub fn offered(groups: &[Group]) -> Vec<&'static NodeType> {
    let mut list: Vec<&NodeType> = NODE_TYPES.iter().filter(|t| !t.hidden && groups.contains(&t.group)).collect();
    list.sort_by_key(|t| t.title.to_lowercase());
    list
}

/// Where a node keeps the order of its ports, among its parameters.
pub const PORT_ORDER: &str = "_port_order";

/// The ports in the order the user gave them. Ports that are not in that order, such as a
/// new value in Filter by value, come after, in their own order.
fn ordered(ports: Vec<String>, order: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = order.into_iter().filter(|o| ports.contains(o)).collect();
    out.dedup();
    for p in ports {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

fn free_id(snarl: &Snarl<GNode>, kind: &str) -> String {
    let base: String = kind.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let mut i = 1;
    while snarl.nodes().any(|n| n.id == format!("{}_{}", base, i)) {
        i += 1;
    }
    format!("{}_{}", base, i)
}

/// A free id and name for a new node of a given type.
pub fn new_node(snarl: &Snarl<GNode>, kind: &str) -> GNode {
    let title = title_of(kind);
    let mut name = title.to_string();
    let mut k = 2;
    while snarl.nodes().any(|n| n.name == name) {
        name = format!("{} {}", title, k);
        k += 1;
    }
    GNode {
        id: free_id(snarl, kind),
        kind: kind.to_string(),
        name,
        params: Params::new(),
        comment: String::new(),
        comment_box: None,
        disabled: false,
        off_wires: Vec::new(),
        bends: Vec::new(),
    }
}

/// Copies the nodes, a little down and to the right of the originals, and returns the
/// copies. Connections between two copied nodes are copied too. Connections to nodes
/// outside the set are not, so that the copies do not feed the same ports twice.
pub fn duplicate(snarl: &mut Snarl<GNode>, nodes: &[NodeId]) -> Vec<NodeId> {
    let mut pairs: Vec<(NodeId, NodeId)> = Vec::new();
    for &n in nodes {
        let Some((pos, src)) = snarl.get_node_info(n).map(|i| (i.pos, i.value.clone())) else { continue };
        let mut copy = new_node(snarl, &src.kind);
        // A node the user has named keeps its name, marked as a copy.
        if !src.is_note() && !has_default_name(&src) {
            copy.name = format!("{} copy", src.name);
        }
        copy.params = src.params;
        copy.comment = src.comment;
        copy.comment_box = src.comment_box;
        copy.disabled = src.disabled;
        pairs.push((n, snarl.insert_node(pos + egui::vec2(30.0, 30.0), copy)));
    }
    let wires: Vec<(OutPinId, InPinId)> = snarl.wires().collect();
    let twin = |n: NodeId| pairs.iter().find(|p| p.0 == n).map(|p| p.1);
    for (out, inp) in wires {
        if let (Some(a), Some(b)) = (twin(out.node), twin(inp.node)) {
            snarl.connect(OutPinId { node: a, output: out.output }, InPinId { node: b, input: inp.input });
        }
    }
    pairs.into_iter().map(|p| p.1).collect()
}

pub fn find(snarl: &Snarl<GNode>, id: &str) -> Option<NodeId> {
    snarl.nodes_ids_data().find(|(_, n)| n.value.id == id).map(|(nid, _)| nid)
}

/// Is there a path along the connections from `from` to `to`?
fn reaches(snarl: &Snarl<GNode>, from: NodeId, to: NodeId) -> bool {
    let wires: Vec<(NodeId, NodeId)> = snarl.wires().map(|(o, i)| (o.node, i.node)).collect();
    let (mut stack, mut seen) = (vec![from], Vec::new());
    while let Some(n) = stack.pop() {
        if n == to {
            return true;
        }
        if seen.contains(&n) {
            continue;
        }
        seen.push(n);
        stack.extend(wires.iter().filter(|w| w.0 == n).map(|w| w.1));
    }
    false
}

pub fn snarl_style(p: Palette) -> SnarlStyle {
    let mut s = SnarlStyle::new();
    s.node_layout = Some(NodeLayout::sandwich());
    s.pin_placement = Some(PinPlacement::Edge);
    s.pin_size = Some(9.0);
    s.pin_fill = Some(p.surface);
    s.pin_stroke = Some(Stroke::new(1.5, p.wire));
    s.wire_width = Some(2.25);
    s.wire_style = Some(WireStyle::Bezier5);
    s.wire_layer = Some(WireLayer::BehindNodes);
    s.collapsible = Some(false);
    s.header_drag_space = Some(egui::vec2(0.0, 0.0));
    s.node_frame = Some(egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.line)).corner_radius(5).inner_margin(egui::Margin::same(0)));
    s.header_frame = Some(egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 5)).corner_radius(egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }));
    s.bg_frame = Some(egui::Frame::new().fill(p.bg));
    s.bg_pattern = Some(BackgroundPattern::grid(egui::vec2(24.0, 24.0), 0.0));
    s.bg_pattern_stroke = Some(Stroke::new(1.0, p.grid));
    s.min_scale = Some(0.3);
    s.max_scale = Some(1.5);
    s.centering = Some(false);
    // The selection box that is dragged out on the canvas.
    s.select_stoke = Some(Stroke::new(1.0, p.accent));
    s.select_fill = Some(Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 24));
    // A node is selected when it touches the box, as in most drawing programs.
    s.select_rect_contained = Some(false);
    // Selected nodes are marked by their own frame instead, see `Viewer::node_frame`. The
    // outline the canvas would draw ends up half hidden behind the node.
    s.select_style = Some(SelectionStyle { margin: egui::Margin::ZERO, rounding: egui::CornerRadius::ZERO, fill: Color32::TRANSPARENT, stroke: Stroke::NONE });
    s
}

/// What the user did to a group box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupEvent {
    Clicked,
    DragStarted,
    Moved(egui::Vec2),
    Resized(egui::Vec2, Sides),
    ToggleLock,
    Delete,
}

/// Which sides of a box follow when it is resized: one edge, or two at a corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sides {
    pub left: bool,
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
}

impl Sides {
    /// A box resized by `by` from these sides, not smaller than `min`: x, y, width, height.
    pub fn apply(self, b: [f32; 4], by: egui::Vec2, min: egui::Vec2) -> [f32; 4] {
        let [mut x, mut y, mut w, mut h] = b;
        if self.left {
            let dx = by.x.min(w - min.x);
            x += dx;
            w -= dx;
        }
        if self.right {
            w = (w + by.x).max(min.x);
        }
        if self.top {
            let dy = by.y.min(h - min.y);
            y += dy;
            h -= dy;
        }
        if self.bottom {
            h = (h + by.y).max(min.y);
        }
        [x, y, w, h]
    }
}

/// What the user did with the box of a comment.
#[derive(Clone, Copy, Debug)]
pub enum CommentEvent {
    Clicked,
    Moved(egui::Vec2),
    Resized(egui::Vec2, Sides),
    /// A note resized from its edges: how far, which sides, and the size its text had.
    NoteResized(egui::Vec2, Sides, egui::Vec2),
}

/// Grips on the four edges and four corners of a box. Returns the sides being dragged and
/// how far, and whether a drag began.
fn resize_grips(ui: &mut Ui, rect: Rect, id: egui::Id) -> Option<(egui::Vec2, Sides, bool)> {
    let (t, c) = (5.0, 12.0);
    let side = |left, top, right, bottom| Sides { left, top, right, bottom };
    let grips = [
        // The edges first, so that the corners, made after them, are on top.
        (
            Rect::from_min_max(egui::pos2(rect.left(), rect.top() + c), egui::pos2(rect.left() + t, rect.bottom() - c)),
            side(true, false, false, false),
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            Rect::from_min_max(egui::pos2(rect.right() - t, rect.top() + c), egui::pos2(rect.right(), rect.bottom() - c)),
            side(false, false, true, false),
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            Rect::from_min_max(egui::pos2(rect.left() + c, rect.top()), egui::pos2(rect.right() - c, rect.top() + t)),
            side(false, true, false, false),
            egui::CursorIcon::ResizeVertical,
        ),
        (
            Rect::from_min_max(egui::pos2(rect.left() + c, rect.bottom() - t), egui::pos2(rect.right() - c, rect.bottom())),
            side(false, false, false, true),
            egui::CursorIcon::ResizeVertical,
        ),
        (Rect::from_min_size(rect.left_top(), egui::vec2(c, c)), side(true, true, false, false), egui::CursorIcon::ResizeNwSe),
        (Rect::from_min_size(rect.right_bottom() - egui::vec2(c, c), egui::vec2(c, c)), side(false, false, true, true), egui::CursorIcon::ResizeNwSe),
        (Rect::from_min_size(egui::pos2(rect.right() - c, rect.top()), egui::vec2(c, c)), side(false, true, true, false), egui::CursorIcon::ResizeNeSw),
        (Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - c), egui::vec2(c, c)), side(true, false, false, true), egui::CursorIcon::ResizeNeSw),
    ];
    let mut out = None;
    for (k, (r, sides, cursor)) in grips.into_iter().enumerate() {
        let resp = ui.interact(r, id.with(("grip", k)), egui::Sense::drag()).on_hover_cursor(cursor);
        if resp.dragged_by(egui::PointerButton::Primary) || resp.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().set_cursor_icon(cursor);
            out = Some((resp.drag_delta(), sides, resp.drag_started_by(egui::PointerButton::Primary)));
        }
    }
    out
}

/// Where the box of a comment is, in graph coordinates, for a node at `pos` of `size`.
pub fn comment_rect(ctx: &egui::Context, n: &GNode, pos: Pos2, size: egui::Vec2) -> Rect {
    let [x, y, w, h] = n.comment_box.unwrap_or_else(|| default_comment_box(ctx, &n.comment, size));
    Rect::from_min_size(pos + egui::vec2(x, y), egui::vec2(w, h))
}

/// The space between the edge of a comment and its text, across and down.
pub const COMMENT_PAD: egui::Vec2 = egui::vec2(8.0, 5.0);

/// The smallest a comment can be made: as wide as its longest word and as high as a line.
pub fn comment_min_size(ctx: &egui::Context, text: &str) -> egui::Vec2 {
    let font = egui::FontId::proportional(12.0);
    let widest =
        text.split_whitespace().map(|w| ctx.fonts_mut(|f| f.layout_no_wrap(w.to_string(), font.clone(), Color32::BLACK).size().x)).fold(0.0f32, f32::max);
    let line = ctx.fonts_mut(|f| f.row_height(&font));
    egui::vec2((widest + 2.0 * COMMENT_PAD.x).ceil().max(20.0), (line + 2.0 * COMMENT_PAD.y).ceil())
}

/// The box a comment gets until it is moved: above the node, as wide as the node and just
/// high enough for the text, up to six lines.
pub fn default_comment_box(ctx: &egui::Context, text: &str, size: egui::Vec2) -> [f32; 4] {
    let w = size.x.max(170.0);
    let font = egui::FontId::proportional(12.0);
    let galley = ctx.fonts_mut(|f| f.layout(text.trim().to_string(), font.clone(), Color32::BLACK, w - 2.0 * COMMENT_PAD.x));
    let line = ctx.fonts_mut(|f| f.row_height(&font));
    let h = (galley.size().y.min(6.0 * line) + 2.0 * COMMENT_PAD.y).ceil();
    [0.0, -h - 18.0, w, h]
}

/// What the user did on the canvas during this frame.
#[derive(Default)]
pub struct CanvasEvents {
    /// Nodes, connections or positions changed.
    pub changed: bool,
    /// Connections changed, so the latest run no longer matches.
    pub rewired: bool,
    /// A node was added from a menu on the canvas, and should become the selection.
    pub added: Option<NodeId>,
    /// The user clicked an output port to see its data.
    pub inspect: Option<(NodeId, String)>,
    /// "Delete" in the menu of this node.
    pub remove: Option<NodeId>,
    /// "Duplicate" in the menu of this node.
    pub duplicate: Option<NodeId>,
    /// "Run to here" in the menu of this node.
    pub run_to: Option<NodeId>,
    /// "Run from here" in the menu of this node.
    pub run_from: Option<NodeId>,
    /// "Copy" in the menu of this node.
    pub copy: Option<NodeId>,
    /// "Paste" in the menu of the canvas, at this place.
    pub paste: Option<Pos2>,
    /// "Show in folder" in the menu of an input or an output.
    pub show_folder: Option<NodeId>,
    /// "Group" in the menu of this node.
    pub group_nodes: Option<NodeId>,
    /// The attribute list of this node was folded in or out.
    pub toggle_attributes: Option<String>,
    /// "Disable" or "Enable" in the menu of this node.
    pub toggle_disabled: Option<NodeId>,
    /// A connection was clicked.
    pub wire_clicked: Option<(OutPinId, InPinId)>,
    /// A connection was clicked with the secondary button, at this place on the screen.
    pub wire_menu: Option<((OutPinId, InPinId), Pos2)>,
    /// A point of a connection was clicked with the secondary button: which, and where.
    pub bend_menu: Option<((OutPinId, InPinId), usize, Pos2)>,
    /// A point of a connection was dragged.
    pub moved_bend: bool,
    /// What happened to the group boxes, by their index.
    pub groups: Vec<(usize, GroupEvent)>,
    /// Things done with the boxes of comments, by node.
    pub comments: Vec<(NodeId, CommentEvent)>,
    /// "Group box" in the menu of the canvas, at this place.
    pub add_group: Option<Pos2>,
    /// "Move up" or "Move down" in the menu of a port: (node, output side, index, up).
    pub move_port: Option<(NodeId, bool, usize, bool)>,
    pub message: Option<String>,
    /// The rectangle of every node in graph coordinates, as it was drawn.
    pub rects: Vec<(NodeId, Rect)>,
}

/// The attributes at a node after the latest run: one list when every port has the same
/// attributes, otherwise one per port. Each attribute comes with the name of its type.
pub type AttributeLists = Vec<(Option<String>, Vec<(String, &'static str)>)>;

pub fn attributes(n: &GNode, result: Option<&RunResult>) -> Option<AttributeLists> {
    let r = result?.nodes.get(&n.id).filter(|r| r.status == Status::Ok || r.status == Status::Warn)?;
    n.ty()?;
    // A writer has no output ports. What came in is shown there.
    let outputs = n.outputs();
    let tables: Vec<(&str, &Table)> = if outputs.is_empty() {
        n.inputs().iter().filter_map(|p| r.inputs.get_key_value(p.as_str()).map(|(k, t)| (k.as_str(), t.as_ref()))).collect()
    } else {
        outputs.iter().filter_map(|p| r.outputs.get(p).map(|t| (p.as_str(), t.as_ref()))).collect()
    };
    let mut lists: AttributeLists = tables
        .iter()
        // A port where nothing at all came out says nothing about the attributes.
        .filter(|(_, t)| !t.fields.is_empty() || !t.is_empty())
        .map(|(p, t)| (Some(p.to_string()), t.fields.iter().map(|f| (f.name.clone(), f.ty.label())).collect()))
        .collect();
    if lists.is_empty() {
        return if tables.is_empty() { None } else { Some(vec![(None, Vec::new())]) };
    }
    if lists.iter().all(|l| l.1 == lists[0].1) {
        lists.truncate(1);
        lists[0].0 = None;
    }
    Some(lists)
}

pub struct Viewer<'a> {
    pub pal: Palette,
    pub result: Option<&'a RunResult>,
    pub stale: bool,
    /// The node and port that the inspector shows.
    pub view: Option<(&'a str, &'a str)>,
    /// The selected nodes. They get a frame in the accent colour.
    pub selected: &'a [NodeId],
    pub events: &'a mut CanvasEvents,
    /// The position of the graph on screen. Read off every frame.
    pub transform: &'a mut TSTransform,
    /// A new transform to switch to, for example to fit all nodes.
    pub fit: &'a mut Option<TSTransform>,
    /// The id of the node that is running right now.
    pub running: Option<&'a str>,
    /// How many files each reader of a folder stands for, by node id.
    pub files: &'a HashMap<String, usize>,
    /// The ids of the nodes whose attribute list is folded out.
    pub expanded: &'a HashSet<String>,
    pub groups: &'a [GroupBox],
    pub selected_group: Option<usize>,
    /// The rectangle of every node as it was drawn in the latest frame.
    pub sizes: &'a [(NodeId, Rect)],
    /// The connection that is selected. It is drawn in the accent colour.
    pub selected_wire: Option<(OutPinId, InPinId)>,
    /// A port that was just moved: (node, output side, name). It is marked for a moment, so
    /// that it is seen where it went.
    pub moved_port: Option<(NodeId, bool, &'a str)>,
    /// Whether the results of the latest run are kept, so that a part can be run.
    pub cache: bool,
    /// A run is going on.
    pub busy: bool,
    /// The run is paused.
    pub paused: bool,
    /// With the cache on: the nodes that have a result in it, and whether it is still up
    /// to date, that is, made with the settings and data the node has now.
    pub cache_state: &'a HashMap<String, bool>,
}

/// A small cylinder, as a sign of a store of data: the result of the node is in the cache.
/// Filled when it is up to date, an outline when the node has changed since.
fn cache_mark(ui: &mut Ui, pal: &Palette, fresh: bool, taken: bool) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(11.0, 13.0), egui::Sense::hover());
    let color = if fresh { pal.accent } else { pal.muted };
    let p = ui.painter();
    let body = egui::Rect::from_min_max(rect.min + egui::vec2(1.0, 2.5), rect.max - egui::vec2(1.0, 1.0));
    if fresh {
        p.rect_filled(body, 3.0, color);
        p.line_segment([egui::pos2(body.left() + 1.0, body.top() + 3.0), egui::pos2(body.right() - 1.0, body.top() + 3.0)], Stroke::new(1.0, pal.surface));
    } else {
        p.rect_stroke(body, 3.0, Stroke::new(1.2, color), egui::StrokeKind::Inside);
        p.line_segment([egui::pos2(body.left() + 1.0, body.top() + 3.0), egui::pos2(body.right() - 1.0, body.top() + 3.0)], Stroke::new(1.0, color));
    }
    let text = match (fresh, taken) {
        (true, true) => "Cached: the result was taken from the cache in the latest run, since nothing it depends on had changed",
        (true, false) => "Cached: the result is kept, and Run from here can start after this node",
        (false, _) => "The cached result is out of date: the node, or something before it, has changed since. It runs again",
    };
    let name = if fresh { "cached" } else { "cache out of date" };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name));
    resp.on_hover_text(text);
}

/// "Add tool" in a menu. A click in its search box, or on the cross that clears it, does
/// not close the menu; picking a tool does.
fn tool_submenu(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    let config = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::SubMenuButton::new("Add tool").config(config).ui(ui, add_contents);
}

/// The tools in a menu: a search box, and under it the tools that match, or all of them
/// under their headings. Returns the type that was picked.
pub fn tool_menu(ui: &mut Ui, pal: &Palette) -> Option<&'static str> {
    let id = ui.make_persistent_id("tool_search");
    let mut needle: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    let field = crate::app::search_field(ui, &mut needle, "Search tools", 220.0);
    if !field.has_focus() && needle.is_empty() {
        field.request_focus();
    }
    ui.data_mut(|d| d.insert_temp(id, needle.clone()));
    let wanted = needle.trim().to_lowercase();
    let tools = offered(&[Group::Transformer]);
    let mut picked = None;
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        if wanted.is_empty() {
            let mut categories: Vec<&str> = tools.iter().map(|t| t.category).collect();
            categories.sort();
            categories.dedup();
            for category in categories {
                ui.label(RichText::new(category).small().color(pal.muted));
                for t in tools.iter().filter(|t| t.category == category) {
                    if ui.button(t.title).on_hover_text(t.desc).clicked() {
                        picked = Some(t.name);
                    }
                }
            }
        } else {
            // Names first, then descriptions, and the names the tools had before.
            let named = |t: &&NodeType| t.title.to_lowercase().contains(&wanted) || t.name.to_lowercase().contains(&wanted);
            let mut found: Vec<&NodeType> = tools.iter().copied().filter(|t| named(t)).collect();
            found.extend(tools.iter().copied().filter(|t| !named(t) && t.desc.to_lowercase().contains(&wanted)));
            if found.is_empty() {
                ui.label(RichText::new("Nothing matches.").color(pal.muted));
            }
            for t in found {
                if ui.button(t.title).on_hover_text(t.desc).clicked() {
                    picked = Some(t.name);
                }
            }
        }
    });
    if picked.is_some() {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    picked
}

/// The height of the strip at the top of a group box that holds its name.
pub const GROUP_TITLE: f32 = 26.0;
/// The widest a comment or a list of attributes gets under a node.
const FOOTER_WIDTH: f32 = 168.0;
const NOTE_WIDTH: f32 = 210.0;
/// The smallest a note can be made: the width, and the height of its text area.
pub const NOTE_MIN: (f32, f32) = (90.0, 16.0);

pub fn group_rect(g: &GroupBox) -> Rect {
    Rect::from_min_size(Pos2::new(g.x as f32, g.y as f32), egui::vec2(g.w as f32, g.h as f32))
}

impl Viewer<'_> {
    /// The menu of a port name: move it up or down among the ports on its side, so that
    /// connections need not cross.
    fn port_menu(&mut self, label: egui::Response, node: NodeId, output: bool, index: usize, count: usize) {
        label.context_menu(|ui| {
            ui.label(RichText::new("The port and its connections").small().color(self.pal.muted));
            if ui.add_enabled(index > 0, egui::Button::new("Move up")).clicked() {
                self.events.move_port = Some((node, output, index, true));
                ui.close();
            }
            if ui.add_enabled(index + 1 < count, egui::Button::new("Move down")).clicked() {
                self.events.move_port = Some((node, output, index, false));
                ui.close();
            }
        });
    }

    fn status(&self, n: &GNode) -> Option<Status> {
        self.result.and_then(|r| r.nodes.get(&n.id)).map(|r| r.status)
    }
    fn count(&self, n: &GNode, port: &str) -> Option<usize> {
        self.result
            .and_then(|r| r.nodes.get(&n.id))
            .filter(|r| r.status == Status::Ok || r.status == Status::Warn)
            .and_then(|r| r.outputs.get(port))
            .map(|t| t.len())
    }

    /// The group boxes: drawn under the nodes, moved by the strip with the name and
    /// resized by the lower right corner. The inside is left to the canvas, so that a
    /// selection box can be dragged there.
    fn show_groups(&mut self, ui: &mut Ui) {
        let groups = self.groups;
        for (i, g) in groups.iter().enumerate() {
            let rect = group_rect(g);
            let (edge, fill) = self.pal.group_box(&g.color);
            let selected = self.selected_group == Some(i);
            let stroke = if selected { Stroke::new(2.0, self.pal.accent) } else { Stroke::new(1.0, edge) };
            let painter = ui.painter();
            painter.rect(rect, 6.0, fill, stroke, egui::StrokeKind::Inside);
            let title = Rect::from_min_size(rect.min, egui::vec2(rect.width(), GROUP_TITLE.min(rect.height())));
            painter.rect_filled(title.shrink(stroke.width), egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, fill);
            let name = if g.name.trim().is_empty() { "Group" } else { g.name.as_str() };
            let clip = painter.with_clip_rect(title.intersect(ui.clip_rect()));
            clip.text(title.left_center() + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, name, theme::strong(13.5), self.pal.ink);
            if !g.locked {
                clip.text(title.right_center() - egui::vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, "unlocked", theme::mono(10.5), self.pal.muted);
            }

            let id = ui.id().with(("group", i));
            let hint = if g.locked { "Drag to move the group and the nodes in it" } else { "Drag to move the group. The nodes stay where they are" };
            let r = ui.interact(title, id, egui::Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(hint);
            if r.clicked() {
                self.events.groups.push((i, GroupEvent::Clicked));
            }
            if r.drag_started_by(egui::PointerButton::Primary) {
                self.events.groups.push((i, GroupEvent::DragStarted));
            }
            if r.dragged_by(egui::PointerButton::Primary) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                self.events.groups.push((i, GroupEvent::Moved(r.drag_delta())));
            }
            r.context_menu(|ui| {
                if ui.button(if g.locked { "Unlock the contents" } else { "Lock the contents" }).clicked() {
                    self.events.groups.push((i, GroupEvent::ToggleLock));
                    ui.close();
                }
                if ui.button("Delete the group").on_hover_text("The nodes in it stay").clicked() {
                    self.events.groups.push((i, GroupEvent::Delete));
                    ui.close();
                }
            });

            // The box is resized from any edge or corner. The marks at the lower right corner
            // show that it can be.
            if let Some((by, sides, started)) = resize_grips(ui, rect, id.with("resize")) {
                if started {
                    self.events.groups.push((i, GroupEvent::Clicked));
                }
                self.events.groups.push((i, GroupEvent::Resized(by, sides)));
            }
            let grip = Stroke::new(1.0, edge);
            for k in [4.0, 8.0, 12.0] {
                ui.painter().line_segment([rect.max - egui::vec2(k, 3.0), rect.max - egui::vec2(3.0, k)], grip);
            }
        }
    }

    /// The comments of the nodes, each in a box with a thin line to its node. The box can be
    /// moved and resized, and follows the node.
    fn show_comments(&mut self, ui: &mut Ui, snarl: &Snarl<GNode>) {
        for (id, pos, n) in snarl.nodes_pos_ids() {
            if n.is_note() {
                // A note is resized from any edge or corner, as a comment is. The grips lie
                // just outside it, where the note itself does not take the pointer.
                if let Some(node) = self.sizes.iter().find(|s| s.0 == id).map(|s| s.1) {
                    let text = ui.ctx().data(|d| d.get_temp::<egui::Vec2>(egui::Id::new(("note-text-size", id)))).unwrap_or(egui::vec2(NOTE_WIDTH, 16.0));
                    if let Some((by, sides, _)) = resize_grips(ui, node.expand(5.0), ui.id().with(("note-resize", id))) {
                        self.events.comments.push((id, CommentEvent::NoteResized(by, sides, text)));
                    }
                }
                continue;
            }
            if n.comment.trim().is_empty() {
                continue;
            }
            let node = self.sizes.iter().find(|s| s.0 == id).map_or(Rect::from_min_size(pos, egui::vec2(170.0, 80.0)), |s| s.1);
            let rect = comment_rect(ui.ctx(), n, node.min, node.size());
            let selected = self.selected.contains(&id);
            let line = if selected { self.pal.accent } else { self.pal.note_line };
            // The line goes from the nearest point of the box to the nearest point of the node.
            let to = node.center();
            let from = egui::pos2(to.x.clamp(rect.left(), rect.right()), to.y.clamp(rect.top(), rect.bottom()));
            let at = egui::pos2(from.x.clamp(node.left(), node.right()), from.y.clamp(node.top(), node.bottom()));
            if !rect.intersects(node) {
                ui.painter().line_segment([from, at], Stroke::new(1.0, line));
                ui.painter().circle_filled(at, 2.5, line);
            }
            ui.painter().rect(rect, 5.0, self.pal.note, Stroke::new(if selected { 1.5 } else { 1.0 }, line), egui::StrokeKind::Inside);
            let inner = rect.shrink2(COMMENT_PAD);
            let key = ui.id().with(("comment", &n.id));
            let r = ui.interact(rect, key, egui::Sense::click_and_drag());
            // The text is painted, so it is given to screen readers this way.
            let said = n.comment.trim().to_string();
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, said.clone()));
            let r = r.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to move the comment. Drag an edge to resize it");
            if r.clicked() || r.drag_started_by(egui::PointerButton::Primary) {
                self.events.comments.push((id, CommentEvent::Clicked));
            }
            if r.dragged_by(egui::PointerButton::Primary) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                self.events.comments.push((id, CommentEvent::Moved(r.drag_delta())));
            }
            if let Some((by, sides, _)) = resize_grips(ui, rect, key.with("resize")) {
                self.events.comments.push((id, CommentEvent::Resized(by, sides)));
            }
            // Painted after the box takes the pointer, so that a link shows the hand.
            let text = n.comment.trim().to_string();
            crate::links::paint(
                ui,
                &text,
                inner.min,
                inner.width().max(10.0),
                egui::FontId::proportional(12.0),
                self.pal.ink,
                self.pal.accent,
                inner.intersect(ui.clip_rect()),
            );
        }
    }

    /// A reader that stands for many files is drawn as a stack: two more sheets stick out
    /// behind it.
    fn show_stacks(&self, ui: &mut Ui, snarl: &Snarl<GNode>) {
        for (id, pos, n) in snarl.nodes_pos_ids() {
            if !self.files.contains_key(&n.id) {
                continue;
            }
            let Some(size) = self.sizes.iter().find(|s| s.0 == id).map(|s| s.1.size()) else { continue };
            let rect = Rect::from_min_size(pos.round(), size);
            for k in [12.0, 6.0] {
                ui.painter().rect(rect.translate(egui::vec2(k, k)), 5.0, self.pal.surface, Stroke::new(1.0, self.pal.wire), egui::StrokeKind::Inside);
            }
        }
    }
}

impl SnarlViewer<GNode> for Viewer<'_> {
    fn title(&mut self, node: &GNode) -> String {
        node.name.clone()
    }

    fn show_background(&mut self, _viewport: &Rect, ui: &mut Ui, snarl: &mut Snarl<GNode>) {
        self.show_groups(ui);
        self.show_stacks(ui, snarl);
        self.show_comments(ui, snarl);
    }

    fn node_frame(&mut self, default: egui::Frame, node: NodeId, _i: &[InPin], _o: &[OutPin], snarl: &Snarl<GNode>) -> egui::Frame {
        let n = &snarl[node];
        let default = if n.is_note() {
            default.fill(self.pal.note).stroke(Stroke::new(1.0, self.pal.note_line))
        } else if n.disabled {
            // A node that is switched off fades into the canvas.
            default.fill(self.pal.panel)
        } else {
            default
        };
        if self.selected.contains(&node) {
            default.stroke(Stroke::new(2.0, self.pal.accent))
        } else if self.running == Some(n.id.as_str()) {
            default.stroke(Stroke::new(2.0, self.pal.xform))
        } else if self.status(n) == Some(Status::Error) {
            default.stroke(Stroke::new(1.5, self.pal.err))
        } else {
            default
        }
    }

    fn header_frame(&mut self, default: egui::Frame, node: NodeId, _i: &[InPin], _o: &[OutPin], snarl: &Snarl<GNode>) -> egui::Frame {
        let n = &snarl[node];
        if n.is_note() {
            // A note is one sheet of paper. Its own frame carries the colour and the outline.
            return default.inner_margin(egui::Margin { left: 8, right: 8, top: 4, bottom: 0 });
        }
        let fill = if n.disabled { self.pal.line } else { n.ty().map(|t| self.pal.group(t.group).1).unwrap_or(self.pal.err_soft) };
        // The header is drawn over the top edge of the node frame, so it carries the
        // selection outline there.
        if self.selected.contains(&node) {
            default.fill(fill).stroke(Stroke::new(2.0, self.pal.accent))
        } else if self.running == Some(n.id.as_str()) {
            default.fill(fill).stroke(Stroke::new(2.0, self.pal.xform))
        } else {
            default.fill(fill)
        }
    }

    fn show_header(&mut self, node: NodeId, _i: &[InPin], _o: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<GNode>) {
        let n = &snarl[node];
        ui.spacing_mut().item_spacing.x = 6.0;
        if n.is_note() {
            // A thin strip to take hold of the note by.
            ui.allocate_exact_size(egui::vec2(10.0, 4.0), egui::Sense::hover());
            return;
        }
        ui.label(RichText::new(&n.name).font(theme::strong(14.0)).color(if n.disabled { self.pal.muted } else { self.pal.ink }));
        if n.disabled {
            ui.label(RichText::new("disabled").font(theme::mono(10.5)).color(self.pal.muted)).on_hover_text("Right-click the node to enable it");
            return;
        }
        if let Some(count) = self.files.get(&n.id) {
            let text = format!("{} {}", count, if *count == 1 { "file" } else { "files" });
            ui.label(RichText::new(text).font(theme::mono(10.5)).color(self.pal.reader)).on_hover_text("This input reads all the files that match in a folder");
        }
        if self.running == Some(n.id.as_str()) && self.paused {
            ui.label(RichText::new("paused").font(theme::mono(10.5)).color(self.pal.warn));
            return;
        }
        if self.running == Some(n.id.as_str()) {
            ui.add(egui::Spinner::new().size(11.0).color(self.pal.xform));
            ui.label(RichText::new("running").font(theme::mono(10.5)).color(self.pal.xform));
            return;
        }
        let badge = match self.status(n) {
            Some(Status::Error) => Some(("error", self.pal.err)),
            Some(Status::Warn) => Some(("warning", self.pal.warn)),
            Some(Status::Skipped) => Some(("skipped", self.pal.warn)),
            _ => None,
        };
        if let Some((text, color)) = badge {
            ui.label(RichText::new(text).font(theme::mono(10.5)).color(color));
        }
        if let Some(fresh) = self.cache_state.get(&n.id) {
            let taken = self.result.and_then(|r| r.nodes.get(&n.id)).is_some_and(|r| r.cached);
            cache_mark(ui, &self.pal, *fresh, taken);
        }
    }

    fn inputs(&mut self, node: &GNode) -> usize {
        node.inputs().len()
    }

    fn outputs(&mut self, node: &GNode) -> usize {
        node.outputs().len()
    }

    #[allow(refining_impl_trait)]
    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<GNode>) -> PinInfo {
        let n = &snarl[pin.id.node];
        let ports = n.inputs();
        let name = ports.get(pin.id.input).map_or("?", |p| p.as_str());
        let moved = self.moved_port == Some((pin.id.node, false, name));
        let text = RichText::new(name).font(theme::mono(11.5)).color(if moved { self.pal.accent } else { self.pal.muted });
        let label = ui.add(egui::Label::new(if moved { text.strong() } else { text }).sense(egui::Sense::click()));
        if ports.len() > 1 {
            self.port_menu(label, pin.id.node, false, pin.id.input, ports.len());
        }
        PinInfo::circle().with_fill(self.pal.surface).with_stroke(Stroke::new(1.5, self.pal.wire))
    }

    #[allow(refining_impl_trait)]
    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<GNode>) -> PinInfo {
        let n = &snarl[pin.id.node];
        let name = n.outputs().get(pin.id.output).cloned().unwrap_or_else(|| "?".into());
        let name = name.as_str();
        let count = self.count(n, name);
        let viewed = self.view == Some((n.id.as_str(), name));
        ui.spacing_mut().item_spacing.x = 5.0;
        let moved = self.moved_port == Some((pin.id.node, true, name));
        let color = if moved {
            self.pal.accent
        } else if viewed {
            self.pal.ink
        } else {
            self.pal.muted
        };
        let text = RichText::new(name).font(theme::mono(11.5)).color(color);
        let label =
            ui.add(egui::Label::new(if moved { text.strong() } else { text }).sense(egui::Sense::click())).on_hover_text("Show the data from this port");
        let mut clicked = label.clicked();
        let ports = n.outputs().len();
        if ports > 1 {
            self.port_menu(label, pin.id.node, true, pin.id.output, ports);
        }
        if let Some(c) = count {
            let color = if self.stale { self.pal.muted } else { self.pal.ink };
            let badge = egui::Frame::new().fill(self.pal.bg).corner_radius(3).inner_margin(egui::Margin::symmetric(4, 1));
            let r = badge
                .show(ui, |ui| ui.add(egui::Label::new(RichText::new(fmt_count(c as u64)).font(theme::mono(11.5)).color(color)).sense(egui::Sense::click())));
            clicked |= r.inner.clicked();
        }
        if clicked {
            self.events.inspect = Some((pin.id.node, name.to_string()));
        }
        let flows = count.is_some_and(|c| c > 0) && !self.stale;
        let wire = if flows { self.pal.xform } else { self.pal.wire };
        let fill = if viewed { self.pal.accent } else { self.pal.surface };
        let stroke = if viewed { self.pal.accent } else { self.pal.wire };
        PinInfo::circle().with_fill(fill).with_stroke(Stroke::new(1.5, stroke)).with_wire_color(wire)
    }

    fn has_body(&mut self, node: &GNode) -> bool {
        node.is_note() || !body_text(node).is_empty()
    }

    fn show_body(&mut self, node: NodeId, _i: &[InPin], _o: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<GNode>) {
        let n = &snarl[node];
        if n.is_note() {
            let empty = n.comment.trim().is_empty();
            let text = if empty { "An empty note. Write the text in the panel on the right." } else { n.comment.trim_end() }.to_string();
            // The size of a note that has been resized is kept in its comment box: width and
            // the height of the text area.
            let [_, _, w, h] = n.comment_box.unwrap_or([0.0, 0.0, NOTE_WIDTH, 0.0]);
            let (w, h) = (w.max(NOTE_MIN.0), h.max(0.0));
            let pal = self.pal;
            egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 6 }).show(ui, |ui| {
                let font = egui::TextStyle::Body.resolve(ui.style());
                let (job, _) = crate::links::layout(&text, font.clone(), pal.ink, pal.accent, w);
                let text_h = ui.painter().layout_job(job).rect.height();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, text_h.max(h)), egui::Sense::hover());
                // The text is painted, so it is given to screen readers this way.
                let said = text.clone();
                let r = ui.interact(rect, ui.id().with(("note-text", node)), egui::Sense::hover());
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, said.clone()));
                crate::links::paint(
                    ui,
                    &text,
                    rect.min,
                    w,
                    font,
                    if empty { pal.muted } else { pal.ink },
                    pal.accent,
                    rect.expand(2.0).intersect(ui.clip_rect()),
                );
                // The size of the text, for resizing from the edges, which is done outside.
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("note-text-size", node)), rect.size()));
                // Marks at the lower right corner say that the note can be resized.
                let corner = rect.max + egui::vec2(8.0, 6.0);
                for k in [4.0, 8.0] {
                    ui.painter().line_segment([corner - egui::vec2(k + 4.0, 5.0), corner - egui::vec2(5.0, k + 4.0)], Stroke::new(1.0, pal.note_line));
                }
            });
            return;
        }
        let text = body_text(n);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
            ui.set_max_width(FOOTER_WIDTH);
            ui.add(egui::Label::new(RichText::new(text).font(theme::mono(11.0)).color(self.pal.muted)).wrap());
        });
    }

    fn has_footer(&mut self, node: &GNode) -> bool {
        !node.is_note() && attributes(node, self.result).is_some()
    }

    /// Under the ports: the comment about the node, and the attributes as they are after
    /// this step, folded in until they are asked for.
    fn show_footer(&mut self, node: NodeId, _i: &[InPin], _o: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<GNode>) {
        let n = &snarl[node];
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 4 }).show(ui, |ui| {
            ui.set_max_width(FOOTER_WIDTH);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                let Some(lists) = attributes(n, self.result) else { return };
                let open = self.expanded.contains(&n.id);
                let total: usize = lists.iter().map(|l| l.1.len()).sum();
                let label = match (lists.len(), total) {
                    (1, 0) => "no attributes".to_string(),
                    (1, 1) => "1 attribute".to_string(),
                    (1, k) => format!("{} attributes", k),
                    _ => "attributes".to_string(),
                };
                let color = if self.stale { self.pal.muted } else { self.pal.xform };
                let text = RichText::new(format!("{} {}", if open { "▾" } else { "▸" }, label)).font(theme::mono(11.0)).color(color);
                let hint = if open { "Hide the attributes" } else { "Show the attributes that are available after this step" };
                if ui.add(egui::Label::new(text).sense(egui::Sense::click())).on_hover_text(hint).clicked() {
                    self.events.toggle_attributes = Some(n.id.clone());
                }
                if !open {
                    return;
                }
                for (port, fields) in &lists {
                    if let Some(port) = port {
                        ui.label(RichText::new(port).font(theme::mono(10.5)).color(self.pal.muted));
                    }
                    for (name, ty) in fields {
                        // One label for the name and its type, so that the rows stay low.
                        let mut row = egui::text::LayoutJob::default();
                        row.append(name, 0.0, egui::TextFormat::simple(theme::mono(11.0), self.pal.ink));
                        row.append(&format!(" {}", ty), 0.0, egui::TextFormat::simple(theme::mono(10.0), self.pal.muted));
                        ui.add(egui::Label::new(row).truncate());
                    }
                }
            });
        });
    }

    fn final_node_rect(&mut self, node: NodeId, rect: Rect, _ui: &mut Ui, _snarl: &mut Snarl<GNode>) {
        self.events.rects.push((node, rect));
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<GNode>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: Pos2, ui: &mut Ui, snarl: &mut Snarl<GNode>) {
        let mut add = None;
        if ui.button("Add input").clicked() {
            add = Some("Reader");
        }
        if ui.button("Add output").clicked() {
            add = Some("Writer");
        }
        tool_submenu(ui, |ui| {
            if let Some(kind) = tool_menu(ui, &self.pal) {
                add = Some(kind);
            }
        });
        ui.separator();
        if ui.button("Add note").clicked() {
            add = Some(NOTE);
        }
        if ui.button("Add group box").clicked() {
            self.events.add_group = Some(pos);
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::new("Paste").shortcut_text("Ctrl+V")).on_hover_text("Pastes nodes copied here or in another window of Ographis").clicked() {
            self.events.paste = Some(pos);
            ui.close();
        }
        if let Some(kind) = add {
            let node = new_node(snarl, kind);
            self.events.added = Some(snarl.insert_node(pos, node));
            self.events.changed = true;
            self.events.rewired = kind != NOTE;
            ui.close();
        }
    }

    fn has_dropped_wire_menu(&mut self, _src: AnyPins, _snarl: &mut Snarl<GNode>) -> bool {
        true
    }

    fn show_dropped_wire_menu(&mut self, pos: Pos2, ui: &mut Ui, src: AnyPins, snarl: &mut Snarl<GNode>) {
        // A connection dropped on empty canvas gives a new node, already connected.
        ui.label(RichText::new("Connect to a new node").small().color(self.pal.muted));
        let mut add = None;
        let (end, label) = match src {
            AnyPins::Out(_) => ("Writer", "Add output"),
            AnyPins::In(_) => ("Reader", "Add input"),
        };
        if ui.button(label).clicked() {
            add = Some(end);
        }
        tool_submenu(ui, |ui| {
            if let Some(kind) = tool_menu(ui, &self.pal) {
                add = Some(kind);
            }
        });
        if let Some(kind) = add {
            let node = new_node(snarl, kind);
            let id = snarl.insert_node(pos, node);
            match src {
                AnyPins::Out(pins) => {
                    for p in pins {
                        snarl.connect(*p, InPinId { node: id, input: 0 });
                    }
                }
                AnyPins::In(pins) => {
                    for p in pins {
                        snarl.connect(OutPinId { node: id, output: 0 }, *p);
                    }
                }
            }
            self.events.added = Some(id);
            self.events.changed = true;
            self.events.rewired = true;
            ui.close();
        }
    }

    fn has_node_menu(&mut self, _node: &GNode) -> bool {
        true
    }

    fn show_node_menu(&mut self, node: NodeId, _i: &[InPin], _o: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<GNode>) {
        // With several nodes selected the menu acts on all of them. The application
        // checks whether this node is one of them.
        let many = self.selected.len() > 1 && self.selected.contains(&node);
        let n = &snarl[node];
        if !n.is_note() {
            let label = match (n.disabled, many) {
                (true, false) => "Enable",
                (true, true) => "Enable selected",
                (false, false) => "Disable",
                (false, true) => "Disable selected",
            };
            if ui.button(label).on_hover_text("A disabled node does not run and lets nothing through").clicked() {
                self.events.toggle_disabled = Some(node);
                ui.close();
            }
            ui.separator();
            let to_hint = if self.cache {
                "Runs this node and what it needs, and nothing after it. What has not changed since the latest run is taken from the cache"
            } else {
                "Runs this node and what it needs, and nothing after it"
            };
            if ui.add_enabled(!self.busy, egui::Button::new("Run to here")).on_hover_text(to_hint).clicked() {
                self.events.run_to = Some(node);
                ui.close();
            }
            let from_hint = if self.cache {
                "Runs this node and everything after it. What comes before it is taken from the cache where it has not changed"
            } else {
                "Needs the cache: turn on Model → Use cache, and run the model once"
            };
            if ui.add_enabled(!self.busy && self.cache, egui::Button::new("Run from here")).on_hover_text(from_hint).on_disabled_hover_text(from_hint).clicked()
            {
                self.events.run_from = Some(node);
                ui.close();
            }
            ui.separator();
            let file_node = !matches!(n.kind.as_str(), "PostgisReader" | "PostgisWriter")
                && n.ty().is_some_and(|t| matches!(t.group, og_engine::nodes::Group::Reader | og_engine::nodes::Group::Writer));
            if file_node {
                if ui.button("Show in folder").on_hover_text("Opens the folder of the file in the file manager").clicked() {
                    self.events.show_folder = Some(node);
                    ui.close();
                }
                ui.separator();
            }
        }
        if ui.add(egui::Button::new(if many { "Group selected" } else { "Put in a group" }).shortcut_text("Ctrl+G")).clicked() {
            self.events.group_nodes = Some(node);
            ui.close();
        }
        if ui.add(egui::Button::new(if many { "Copy selected" } else { "Copy" }).shortcut_text("Ctrl+C")).clicked() {
            self.events.copy = Some(node);
            ui.close();
        }
        if ui.add(egui::Button::new(if many { "Duplicate selected" } else { "Duplicate" }).shortcut_text("Ctrl+D")).clicked() {
            self.events.duplicate = Some(node);
            ui.close();
        }
        if ui.add(egui::Button::new(if many { "Delete selected" } else { "Delete" }).shortcut_text("Del")).clicked() {
            self.events.remove = Some(node);
            ui.close();
        }
    }

    /// A node can be put into a connection when it has a first input and a first output
    /// that are both free, and when that makes no loop.
    fn can_drop_node_on_wire(&mut self, node: NodeId, from: OutPinId, to: InPinId, snarl: &Snarl<GNode>) -> bool {
        let Some(ty) = snarl.get_node(node).and_then(|n| n.ty()) else { return false };
        if ty.inputs.is_empty() || ty.outputs.is_empty() || from.node == node || to.node == node {
            return false;
        }
        let taken = snarl.wires().any(|(o, i)| (i.node == node && i.input == 0) || (o.node == node && o.output == 0));
        !taken && !reaches(snarl, node, from.node) && !reaches(snarl, to.node, node)
    }

    fn drop_node_on_wire(&mut self, node: NodeId, from: &OutPin, to: &InPin, snarl: &mut Snarl<GNode>) {
        snarl.disconnect(from.id, to.id);
        snarl.connect(from.id, InPinId { node, input: 0 });
        snarl.connect(OutPinId { node, output: 0 }, to.id);
        let name = |n: NodeId| snarl.get_node(n).map(|x| x.name.clone()).unwrap_or_default();
        self.events.message = Some(format!("Put {} between {} and {}", name(node), name(from.id.node), name(to.id.node)));
        self.events.changed = true;
        self.events.rewired = true;
    }

    fn wire_clicked(&mut self, from: &OutPin, to: &InPin, _snarl: &mut Snarl<GNode>) {
        self.events.wire_clicked = Some((from.id, to.id));
    }

    fn wire_secondary_clicked(&mut self, from: &OutPin, to: &InPin, pos: Pos2, _snarl: &mut Snarl<GNode>) {
        self.events.wire_menu = Some(((from.id, to.id), pos));
    }

    fn wire_stroke(&mut self, from: OutPinId, to: InPinId, default: Stroke, snarl: &Snarl<GNode>) -> Stroke {
        if self.selected_wire == Some((from, to)) {
            Stroke::new(default.width.max(2.0) * 1.6, self.pal.accent)
        } else if wire_disabled(snarl, from, to) {
            // A disabled connection is drawn in dashes, in the colour of an error.
            Stroke::new(default.width.max(2.0), self.pal.err)
        } else if node_off(snarl, from.node) || node_off(snarl, to.node) {
            // To or from a disabled node, nothing flows: dashes, in a light grey.
            Stroke::new(default.width.max(2.0), self.pal.muted.gamma_multiply(0.55))
        } else {
            default
        }
    }

    fn wire_points(&mut self, from: OutPinId, to: InPinId, snarl: &Snarl<GNode>) -> Vec<Pos2> {
        wire_bends(snarl, from, to)
    }

    fn wire_point_moved(&mut self, from: OutPinId, to: InPinId, index: usize, pos: Pos2, snarl: &mut Snarl<GNode>) {
        let mut pts = wire_bends(snarl, from, to);
        if let Some(p) = pts.get_mut(index) {
            *p = pos;
            set_wire_bends(snarl, from, to, pts);
            self.events.moved_bend = true;
        }
    }

    fn wire_point_secondary_clicked(&mut self, from: OutPinId, to: InPinId, index: usize, pos: Pos2, _snarl: &mut Snarl<GNode>) {
        self.events.bend_menu = Some(((from, to), index, pos));
    }

    fn wire_dashed(&mut self, from: OutPinId, to: InPinId, snarl: &Snarl<GNode>) -> bool {
        wire_disabled(snarl, from, to) || node_off(snarl, from.node) || node_off(snarl, to.node)
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GNode>) {
        if from.id.node == to.id.node {
            self.events.message = Some("A node cannot be connected to itself.".into());
            return;
        }
        if reaches(snarl, to.id.node, from.id.node) {
            self.events.message = Some("That connection would create a loop.".into());
            return;
        }
        if snarl.connect(from.id, to.id) {
            // A connection made anew is on, even where one like it was turned off before.
            let key = (snarl[from.id.node].id.clone(), snarl[from.id.node].outputs().get(from.id.output).cloned().unwrap_or_default());
            let port = snarl[to.id.node].inputs().get(to.id.input).cloned().unwrap_or_default();
            snarl[to.id.node].off_wires.retain(|w| !(w.0 == key.0 && w.1 == key.1 && w.2 == port));
            self.events.changed = true;
            self.events.rewired = true;
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GNode>) {
        if snarl.disconnect(from.id, to.id) {
            self.events.changed = true;
            self.events.rewired = true;
        }
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<GNode>) {
        if snarl.drop_outputs(pin.id) > 0 {
            self.events.changed = true;
            self.events.rewired = true;
        }
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<GNode>) {
        if snarl.drop_inputs(pin.id) > 0 {
            self.events.changed = true;
            self.events.rewired = true;
        }
    }

    fn current_transform(&mut self, to_global: &mut TSTransform, _snarl: &mut Snarl<GNode>) {
        if let Some(t) = self.fit.take() {
            *to_global = t;
        }
        *self.transform = *to_global;
    }
}

/// The transform that shows `content` (graph coordinates) in `screen` (screen coordinates).
/// The scale never goes below `min_scale`, so that the text stays readable. If not
/// everything fits, the top left corner of the content goes in the top left corner of the area.
pub fn fit_transform(content: Rect, screen: Rect, min_scale: f32, max_scale: f32) -> TSTransform {
    let scale = (screen.width() / content.width().max(1.0)).min(screen.height() / content.height().max(1.0)).clamp(min_scale, max_scale);
    let size = content.size() * scale;
    let x = if size.x <= screen.width() { screen.center().x - content.center().x * scale } else { screen.left() - content.left() * scale };
    let y = if size.y <= screen.height() { screen.center().y - content.center().y * scale } else { screen.top() - content.top() * scale };
    TSTransform::new(egui::vec2(x, y), scale)
}

// ---------- Copy and paste ----------

/// The chosen nodes as a small model in JSON, for the clipboard: the nodes and notes, and
/// the connections between them. Paths are made whole from `base`, so that they still point
/// to the same files when pasted into a model in another folder.
pub fn copy_json(snarl: &Snarl<GNode>, nodes: &[NodeId], groups: &[GroupBox], base: &Path) -> Option<String> {
    let mut part: Snarl<GNode> = Snarl::new();
    let mut pairs: Vec<(NodeId, NodeId)> = Vec::new();
    for &n in nodes {
        let Some(info) = snarl.get_node_info(n) else { continue };
        let mut node = info.value.clone();
        if let Some(p) = node.params.get("path").and_then(|v| v.as_str()).map(str::to_string) {
            let p = p.trim();
            if !p.is_empty() && !og_engine::nodes::is_url(p) && Path::new(p).is_relative() {
                node.params.insert("path".into(), serde_json::Value::String(base.join(p).to_string_lossy().into_owned()));
            }
        }
        pairs.push((n, part.insert_node(info.pos, node)));
    }
    if pairs.is_empty() {
        return None;
    }
    let twin = |n: NodeId| pairs.iter().find(|p| p.0 == n).map(|p| p.1);
    for (out, inp) in snarl.wires() {
        if let (Some(a), Some(b)) = (twin(out.node), twin(inp.node)) {
            part.connect(OutPinId { node: a, output: out.output }, InPinId { node: b, input: inp.input });
        }
    }
    Some(workspace_from(&part, CLIPBOARD_NAME, groups, &[]).to_json())
}

/// The name a copied part of a model has, so that it is known again when pasted.
pub const CLIPBOARD_NAME: &str = "Ographis clipboard";

/// Puts the nodes of a copied part of a model, or of a whole model file, on the canvas
/// with their upper left corner at `at`. Each node gets an id of its own. A path under
/// `base` is made relative to it again. Gives the new nodes, or None when the text is not
/// a model.
pub fn paste_json(snarl: &mut Snarl<GNode>, text: &str, at: Pos2, base: &Path) -> Option<(Vec<NodeId>, Vec<GroupBox>)> {
    let text = text.trim();
    if !text.starts_with('{') {
        return None;
    }
    let ws = Workspace::from_json(text).ok()?;
    if ws.nodes.is_empty() && ws.notes.is_empty() {
        return None;
    }
    // The upper left corner of what was copied, group boxes too, goes where it is pasted.
    let min_x = ws.nodes.iter().map(|n| n.x).chain(ws.notes.iter().map(|n| n.x)).chain(ws.groups.iter().map(|g| g.x)).fold(f64::INFINITY, f64::min);
    let min_y = ws.nodes.iter().map(|n| n.y).chain(ws.notes.iter().map(|n| n.y)).chain(ws.groups.iter().map(|g| g.y)).fold(f64::INFINITY, f64::min);
    let place = |x: f64, y: f64| at + egui::vec2((x - min_x) as f32, (y - min_y) as f32);
    let taken: Vec<String> = snarl.nodes().map(|n| n.name.clone()).collect();
    let mut ids: HashMap<String, (NodeId, Vec<String>, Vec<String>)> = HashMap::new();
    let mut added = Vec::new();
    for n in &ws.nodes {
        if og_engine::nodes::node_type(&n.kind).is_none() {
            continue;
        }
        let mut node = new_node(snarl, &n.kind);
        let name = n.label().to_string();
        if !name.is_empty() && name != node.name {
            node.name = if taken.contains(&name) { format!("{} copy", name) } else { name };
        }
        node.params = n.params.clone();
        if let Some(p) = node.params.get("path").and_then(|v| v.as_str()).map(str::to_string) {
            if let Ok(rest) = Path::new(p.trim()).strip_prefix(base) {
                node.params.insert("path".into(), serde_json::Value::String(rest.to_string_lossy().replace('\\', "/")));
            }
        }
        node.comment = n.comment.clone();
        node.comment_box = n.comment_box.map(|b| b.map(|v| v as f32));
        node.disabled = n.disabled;
        let ports = (node.outputs(), node.inputs());
        let id = snarl.insert_node(place(n.x, n.y), node);
        ids.insert(n.id.clone(), (id, ports.0, ports.1));
        added.push(id);
    }
    for note in &ws.notes {
        let mut node = new_node(snarl, NOTE);
        node.comment = note.text.clone();
        if let (Some(w), Some(h)) = (note.w, note.h) {
            node.comment_box = Some([0.0, 0.0, w as f32, h as f32]);
        }
        added.push(snarl.insert_node(place(note.x, note.y), node));
    }
    for e in &ws.edges {
        let (Some((from, outputs, _)), Some((to, _, inputs))) = (ids.get(&e.from.0), ids.get(&e.to.0)) else { continue };
        if let (Some(output), Some(input)) = (outputs.iter().position(|p| *p == e.from.1), inputs.iter().position(|p| *p == e.to.1)) {
            snarl.connect(OutPinId { node: *from, output }, InPinId { node: *to, input });
            if !e.points.is_empty() {
                let pts = e.points.iter().map(|p| place(p[0], p[1])).collect();
                set_wire_bends(snarl, OutPinId { node: *from, output }, InPinId { node: *to, input }, pts);
            }
            if e.disabled {
                let from_id = snarl[*from].id.clone();
                snarl[*to].off_wires.push((from_id, e.from.1.clone(), e.to.1.clone()));
            }
        }
    }
    let groups = ws
        .groups
        .iter()
        .map(|g| {
            let p = place(g.x, g.y);
            GroupBox { x: p.x as f64, y: p.y as f64, ..g.clone() }
        })
        .collect();
    Some((added, groups))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> (Snarl<GNode>, Vec<NodeId>) {
        let mut snarl = Snarl::new();
        let ids = ["Reader", "Tester", "Tester", "Writer"].iter().map(|k| {
            let n = new_node(&snarl, k);
            snarl.insert_node(Pos2::ZERO, n)
        });
        let ids: Vec<NodeId> = ids.collect();
        (snarl, ids)
    }

    fn try_connect(snarl: &mut Snarl<GNode>, from: (NodeId, usize), to: (NodeId, usize)) -> CanvasEvents {
        let mut events = CanvasEvents::default();
        let (mut transform, mut fit) = (TSTransform::IDENTITY, None);
        let mut v = Viewer {
            pal: Palette::LIGHT,
            result: None,
            stale: false,
            view: None,
            selected: &[],
            events: &mut events,
            transform: &mut transform,
            fit: &mut fit,
            running: None,
            files: &HashMap::new(),
            expanded: &HashSet::new(),
            groups: &[],
            selected_group: None,
            sizes: &[],
            selected_wire: None,
            moved_port: None,
            cache: false,
            busy: false,
            paused: false,
            cache_state: &HashMap::new(),
        };
        let out = OutPin { id: OutPinId { node: from.0, output: from.1 }, remotes: vec![] };
        let inp = InPin { id: InPinId { node: to.0, input: to.1 }, remotes: vec![] };
        v.connect(&out, &inp, snarl);
        events
    }

    #[test]
    fn new_nodes_get_free_ids_and_names() {
        let (snarl, _) = graph();
        let names: Vec<(String, String)> = snarl.nodes().map(|n| (n.id.clone(), n.name.clone())).collect();
        assert_eq!(
            names,
            [
                ("reader_1".into(), "Input".into()),
                ("tester_1".into(), "Test".into()),
                ("tester_2".into(), "Test 2".into()),
                ("writer_1".into(), "Output".into())
            ]
        );
    }

    #[test]
    fn connections_cannot_form_loops() {
        let (mut snarl, n) = graph();
        assert!(try_connect(&mut snarl, (n[0], 0), (n[1], 0)).rewired);
        assert!(try_connect(&mut snarl, (n[1], 0), (n[2], 0)).rewired);
        // Back from the second tester to the first would be a loop.
        let e = try_connect(&mut snarl, (n[2], 0), (n[1], 0));
        assert!(!e.changed && e.message.unwrap().contains("loop"));
        let e = try_connect(&mut snarl, (n[1], 1), (n[1], 0));
        assert!(!e.changed && e.message.unwrap().contains("to itself"));
        // Two connections into the same port are allowed. The engine merges the streams.
        assert!(try_connect(&mut snarl, (n[0], 0), (n[2], 0)).rewired);
        assert_eq!(snarl.wires().count(), 3);
    }

    #[test]
    fn only_a_free_tool_goes_into_a_connection() {
        let (mut snarl, n) = graph();
        // Reader > Tester 2 > Writer, and the first tester stands free.
        try_connect(&mut snarl, (n[0], 0), (n[2], 0));
        try_connect(&mut snarl, (n[2], 0), (n[3], 0));
        let mut events = CanvasEvents::default();
        let (mut transform, mut fit) = (TSTransform::IDENTITY, None);
        let (files, expanded) = (HashMap::new(), HashSet::new());
        let mut v = Viewer {
            pal: Palette::LIGHT,
            result: None,
            stale: false,
            view: None,
            selected: &[],
            events: &mut events,
            transform: &mut transform,
            fit: &mut fit,
            running: None,
            files: &files,
            expanded: &expanded,
            groups: &[],
            selected_group: None,
            sizes: &[],
            selected_wire: None,
            moved_port: None,
            cache: false,
            busy: false,
            paused: false,
            cache_state: &HashMap::new(),
        };
        let first = (OutPinId { node: n[0], output: 0 }, InPinId { node: n[2], input: 0 });
        let second = (OutPinId { node: n[2], output: 0 }, InPinId { node: n[3], input: 0 });
        assert!(v.can_drop_node_on_wire(n[1], first.0, first.1, &snarl));
        // A reader has no input and a writer no output. A node cannot go into its own connection.
        assert!(!v.can_drop_node_on_wire(n[0], second.0, second.1, &snarl));
        assert!(!v.can_drop_node_on_wire(n[3], first.0, first.1, &snarl));
        assert!(!v.can_drop_node_on_wire(n[2], first.0, first.1, &snarl));

        let (out, inp) = (OutPin { id: first.0, remotes: vec![] }, InPin { id: first.1, remotes: vec![] });
        v.drop_node_on_wire(n[1], &out, &inp, &mut snarl);
        let wires: Vec<(NodeId, NodeId)> = snarl.wires().map(|(o, i)| (o.node, i.node)).collect();
        assert_eq!(wires.len(), 3);
        for pair in [(n[0], n[1]), (n[1], n[2]), (n[2], n[3])] {
            assert!(wires.contains(&pair), "{:?} should be connected", pair);
        }
        // Now that it is connected it does not go into another connection.
        assert!(!v.can_drop_node_on_wire(n[1], second.0, second.1, &snarl));
        assert!(events.rewired && events.message.unwrap().starts_with("Put Test between Input and Test 2"));
    }

    #[test]
    fn workspace_round_trip() {
        let (mut snarl, n) = graph();
        try_connect(&mut snarl, (n[0], 0), (n[1], 0));
        try_connect(&mut snarl, (n[1], 1), (n[3], 0));
        let ws = workspace_from(&snarl, "test", &[], &[]);
        assert_eq!(
            ws.edges,
            [
                Edge::new(("reader_1".into(), "Output".into()), ("tester_1".into(), "Input".into())),
                Edge::new(("tester_1".into(), "Failed".into()), ("writer_1".into(), "Input".into())),
            ]
        );
        // A disabled connection is kept as such, both ways.
        snarl[n[3]].off_wires.push(("tester_1".into(), "Failed".into(), "Input".into()));
        let ws = workspace_from(&snarl, "test", &[], &[]);
        assert!(ws.edges[1].disabled && !ws.edges[0].disabled);
        let again = workspace_from(&snarl_from(&ws), "test", &[], &[]);
        assert_eq!(again.edges, ws.edges);
        assert_eq!(again.nodes.len(), 4);
    }

    #[test]
    fn duplicating_copies_inner_connections_only() {
        let (mut snarl, n) = graph();
        try_connect(&mut snarl, (n[0], 0), (n[1], 0));
        try_connect(&mut snarl, (n[1], 0), (n[2], 0));
        try_connect(&mut snarl, (n[2], 0), (n[3], 0));
        snarl[n[1]].name = "Large towns".into();
        snarl[n[1]].params.insert("test".into(), serde_json::Value::String("population > 10000".into()));
        // The two testers are copied. The connection between them follows, the ones to
        // the reader and the writer do not.
        let copies = duplicate(&mut snarl, &[n[1], n[2]]);
        assert_eq!(copies.len(), 2);
        assert_eq!(snarl.nodes().count(), 6);
        assert_eq!(snarl.wires().count(), 4);
        assert!(snarl.wires().any(|(o, i)| o.node == copies[0] && i.node == copies[1]));
        let (a, b) = (&snarl[copies[0]], &snarl[copies[1]]);
        assert_eq!((a.id.as_str(), a.name.as_str()), ("tester_3", "Large towns copy"));
        assert_eq!(a.params.get("test").and_then(|v| v.as_str()), Some("population > 10000"));
        // The second copy takes the first free default name, which is "Tester" now that the
        // first tester has a name of its own.
        assert_eq!((b.id.as_str(), b.name.as_str()), ("tester_4", "Test"));
    }
}
