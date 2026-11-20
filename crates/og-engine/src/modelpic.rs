//! A model as a picture: its nodes with their names, how they are set up and their ports,
//! the connections between them, group boxes, comments and notes, laid out and coloured as
//! on the canvas. Written as PNG, JPEG or WebP, or as a PDF page, for documentation and to
//! show others how a flow works.

use crate::chart::{Face, Text};
use crate::model::{GbError, RasterImage, Result};
use crate::nodes::{node_type, Group, P};
use crate::run::{Node, Workspace};
use std::collections::HashMap;
use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};

// The measures of the canvas, in its units.
/// The header of a node: its name between margins of 5 above and below.
const HEAD: f32 = 27.0;
/// A port, one to a row.
const ROW: f32 = 19.0;
/// The settings of a node under its inputs: wrapped at this width.
const BODY_W: f32 = 168.0;
const BODY_LINE: f32 = 14.0;
/// The size of the letters: the name, the ports and the settings.
const NAME: f32 = 14.0;
const PORT: f32 = 11.5;
const BODY: f32 = 11.0;
const COMMENT: f32 = 12.0;
const COMMENT_LINE: f32 = 15.0;
/// The strip at the top of a group box that holds its name.
const GROUP_TITLE: f32 = 26.0;
const NOTE_W: f32 = 210.0;

// The colours of the light theme of the canvas.
const BG: [u8; 3] = [0xe7, 0xec, 0xea];
const GRID: [u8; 3] = [0xd3, 0xdc, 0xd8];
const PANEL: [u8; 3] = [0xf4, 0xf7, 0xf6];
const SURFACE: [u8; 3] = [0xff, 0xff, 0xff];
const INK: [u8; 3] = [0x14, 0x20, 0x1d];
const MUTED: [u8; 3] = [0x55, 0x64, 0x5f];
const LINE: [u8; 3] = [0xc6, 0xd1, 0xcd];
const WIRE: [u8; 3] = [0x3a, 0x45, 0x42];
const NOTE: [u8; 3] = [0xff, 0xf7, 0xcf];
const NOTE_LINE: [u8; 3] = [0xd6, 0xbf, 0x63];

/// A node laid out: where it is, its size, its ports and where each lies down the node, and
/// the lines of its settings.
struct Laid {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    ins: Vec<(String, f32)>,
    outs: Vec<(String, f32)>,
    body: Vec<String>,
    body_y: f32,
}

/// The colour of a kind of node, and its light tint, as on the canvas.
fn tint(group: Group) -> ([u8; 3], [u8; 3]) {
    match group {
        Group::Reader => ([0x7d, 0x56, 0x00], [0xf8, 0xeb, 0xc6]),
        Group::Writer => ([0x1f, 0x6a, 0x3d], [0xd9, 0xef, 0xe1]),
        Group::Transformer => ([0x1c, 0x55, 0x96], [0xdb, 0xe7, 0xf6]),
    }
}

/// The edge and the fill of a group box by its colour, as on the canvas: the colours of the
/// nodes, others mixed into the canvas, a colour of its own as #rrggbb, or grey.
fn group_colour(name: &str) -> ([u8; 3], [u8; 3]) {
    let mixed = |e: [u8; 3]| (e, [0, 1, 2].map(|i| ((e[i] as u16 * 30 + BG[i] as u16 * 70) / 100) as u8));
    match name {
        "yellow" => tint(Group::Reader),
        "blue" => tint(Group::Transformer),
        "green" => tint(Group::Writer),
        "pink" | "magenta" => ([0xa3, 0x17, 0x5c], [0xf7, 0xdc, 0xea]),
        "orange" => mixed([0xd9, 0x7a, 0x1e]),
        "red" => mixed([0xc8, 0x3a, 0x32]),
        "purple" => mixed([0x7d, 0x4f, 0xb5]),
        "teal" => mixed([0x1f, 0x93, 0x8c]),
        "brown" => mixed([0x8a, 0x62, 0x3c]),
        other => match other.strip_prefix('#').filter(|h| h.len() == 6).and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(v) => mixed([(v >> 16) as u8, (v >> 8) as u8, v as u8]),
            None => (MUTED, LINE),
        },
    }
}

fn paint(c: [u8; 3], a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], a));
    p.anti_alias = true;
    p
}

/// A rectangle with its corners rounded: each by its own radius, from the upper left on.
fn rounded4(x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) -> Option<tiny_skia::Path> {
    let m = (w / 2.0).min(h / 2.0);
    let [a, b, c, d] = r.map(|v| v.min(m));
    let mut pb = PathBuilder::new();
    pb.move_to(x + a, y);
    pb.line_to(x + w - b, y);
    pb.quad_to(x + w, y, x + w, y + b);
    pb.line_to(x + w, y + h - c);
    pb.quad_to(x + w, y + h, x + w - c, y + h);
    pb.line_to(x + d, y + h);
    pb.quad_to(x, y + h, x, y + h - d);
    pb.line_to(x, y + a);
    pb.quad_to(x, y, x + a, y);
    pb.close();
    pb.finish()
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    rounded4(x, y, w, h, [r; 4])
}

/// Text cut into lines that fit a width.
fn wrap(text: &Text, s: &str, size: f32, face: Face, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.lines() {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let try_line = if line.is_empty() { word.to_string() } else { format!("{} {}", line, word) };
            if text.width_in(&try_line, size, face) > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = try_line;
            }
        }
        lines.push(line);
    }
    lines
}

/// The ports of a node: those of its tool, and any connection names it has besides.
fn ports(ws: &Workspace, id: &str, kind: &str, params: &crate::nodes::Params) -> (Vec<String>, Vec<String>) {
    let (mut ins, mut outs): (Vec<String>, Vec<String>) = match node_type(kind) {
        Some(t) => {
            let p = P::new(params, t.params);
            let outs = match t.ports {
                Some(f) => f(&p),
                None => t.outputs.iter().map(|s| s.to_string()).collect(),
            };
            (t.inputs_for(params).iter().map(|s| s.to_string()).collect(), outs)
        }
        None => (Vec::new(), Vec::new()),
    };
    for e in &ws.edges {
        if e.to.0 == id && !ins.contains(&e.to.1) {
            ins.push(e.to.1.clone());
        }
        if e.from.0 == id && !outs.contains(&e.from.1) {
            outs.push(e.from.1.clone());
        }
    }
    (ins, outs)
}

/// What the node says under its inputs: how it is set up, and the tool when the node has a
/// name of its own.
fn body_text(n: &Node) -> String {
    let Some(t) = node_type(&n.kind) else { return String::new() };
    let summary = (t.summary)(&P::new(&n.params, t.params));
    let renamed = !n.name.is_empty() && crate::run::default_name_today(&n.kind, &n.name).is_none();
    match (renamed, summary.trim().is_empty()) {
        (true, false) => format!("{} · {}", t.title, summary),
        (true, true) => t.title.to_string(),
        (false, _) => summary,
    }
}

/// A node laid out as on the canvas: the header with its name, the inputs, its settings and
/// the outputs, one under the other, as wide as what it holds.
fn lay_out(text: &Text, ws: &Workspace, n: &Node) -> Laid {
    let (ins, outs) = ports(ws, &n.id, &n.kind, &n.params);
    let body_full = body_text(n);
    let body = if body_full.is_empty() { Vec::new() } else { wrap(text, &body_full, BODY, Face::Mono, BODY_W) };
    let port_w = |s: &String| text.width_in(s, PORT, Face::Mono);
    // Room is kept beside each output for the number of features of a run.
    let count = text.width_in("99 999", PORT, Face::Mono) + 8.0;
    let w = [
        8.0 + 16.0 + text.width(n.label(), NAME, true).ceil() + 2.0 + if n.disabled { 6.0 + text.width_in("disabled", 10.5, Face::Mono) } else { 0.0 } + 8.0,
        ins.iter().map(|p| 12.0 + port_w(p) + 12.0).fold(0.0, f32::max),
        body.iter().map(|l| text.width_in(l, BODY, Face::Mono)).fold(0.0, f32::max) + 16.0,
        outs.iter().map(|p| 12.0 + port_w(p) + 5.0 + count + 12.0).fold(0.0, f32::max),
        80.0,
    ]
    .into_iter()
    .fold(0.0, f32::max)
    .ceil();
    let mut y = HEAD + 4.0;
    let ins: Vec<(String, f32)> = ins
        .into_iter()
        .map(|p| {
            let at = y + ROW / 2.0;
            y += ROW;
            (p, at)
        })
        .collect();
    let body_y = y + 2.0;
    if !body.is_empty() {
        y += body.len() as f32 * BODY_LINE + 6.0;
    }
    let outs: Vec<(String, f32)> = outs
        .into_iter()
        .map(|p| {
            let at = y + ROW / 2.0;
            y += ROW;
            (p, at)
        })
        .collect();
    Laid { x: n.x as f32, y: n.y as f32, w, h: y + 6.0, ins, outs, body, body_y }
}

/// Where the comment of a node is: where it was put, or above the node, as wide as the node
/// and high enough for its text, up to six lines.
fn comment_box(text: &Text, n: &Node, b: &Laid) -> [f32; 4] {
    match n.comment_box {
        Some(c) => [b.x + c[0] as f32, b.y + c[1] as f32, c[2] as f32, c[3] as f32],
        None => {
            let w = b.w.max(170.0);
            let lines = wrap(text, n.comment.trim(), COMMENT, Face::Regular, w - 16.0).len().min(6);
            let h = (lines as f32 * COMMENT_LINE + 10.0).ceil();
            [b.x, b.y - h - 18.0, w, h]
        }
    }
}

/// The model drawn, `scale` pixels to a unit of the canvas.
pub fn draw(ws: &Workspace, scale: f32) -> Result<RasterImage> {
    let text = Text::new()?;
    let laid: HashMap<&str, Laid> = ws.nodes.iter().map(|n| (n.id.as_str(), lay_out(&text, ws, n))).collect();
    let notes: Vec<(f32, f32, f32, f32, Vec<String>)> = ws
        .notes
        .iter()
        .map(|note| {
            let w = note.w.map_or(NOTE_W, |w| w as f32).max(90.0);
            let lines = wrap(&text, note.text.trim_end(), 13.0, Face::Regular, w);
            let h = (lines.len() as f32 * 17.0).max(note.h.unwrap_or(0.0) as f32);
            // A strip to take hold of it by, the text, and margins.
            (note.x as f32, note.y as f32, w + 16.0, 20.0 + h + 6.0, lines)
        })
        .collect();
    // The extent of it all.
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut grow = |x: f32, y: f32, w: f32, h: f32| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + w);
        y1 = y1.max(y + h);
    };
    for n in &ws.nodes {
        let b = &laid[n.id.as_str()];
        grow(b.x, b.y, b.w, b.h);
        if !n.comment.trim().is_empty() {
            let c = comment_box(&text, n, b);
            grow(c[0], c[1], c[2], c[3]);
        }
    }
    for g in &ws.groups {
        grow(g.x as f32, g.y as f32, g.w as f32, g.h as f32);
    }
    for n in &notes {
        grow(n.0, n.1, n.2, n.3);
    }
    if !x0.is_finite() {
        return Err(GbError("The model has nothing to draw.".into()));
    }
    let title = if ws.name.trim().is_empty() { None } else { Some(ws.name.trim().to_string()) };
    let margin = 40.0;
    // The description of the model under the title, as wide as the model, or at least 600.
    let described: Vec<String> = if ws.description.trim().is_empty() {
        Vec::new()
    } else {
        ws.description
            .trim()
            .lines()
            .flat_map(|l| if l.trim().is_empty() { vec![String::new()] } else { wrap(&text, l, 14.0, Face::Regular, (x1 - x0).max(600.0)) })
            .collect()
    };
    let headed = if title.is_some() { 60.0 } else { 0.0 };
    let top = headed + if described.is_empty() { 0.0 } else { described.len() as f32 * 20.0 + 16.0 };
    let (w, h) = (x1 - x0 + 2.0 * margin, y1 - y0 + 2.0 * margin + top);
    let (pw, ph) = ((w * scale).ceil() as u32, (h * scale).ceil() as u32);
    if (pw as u64) * (ph as u64) > 120_000_000 {
        return Err(GbError("The model is too large to draw at this size. Draw it smaller.".into()));
    }
    let mut pm = Pixmap::new(pw.max(1), ph.max(1)).ok_or_else(|| GbError("The picture of the model is too large.".into()))?;
    pm.fill(Color::from_rgba8(BG[0], BG[1], BG[2], 255));
    let (ox, oy) = (margin - x0, margin - y0 + top);
    let t = Transform::from_scale(scale, scale).pre_translate(ox, oy);
    // Where a point of the canvas is in the picture, for text, which is drawn in pixels.
    let at = |x: f32, y: f32| ((x + ox) * scale, (y + oy) * scale);
    let fill = |pm: &mut Pixmap, path: &Option<tiny_skia::Path>, c: [u8; 3], a: u8| {
        if let Some(p) = path {
            pm.fill_path(p, &paint(c, a), FillRule::Winding, t, None);
        }
    };
    let line = |pm: &mut Pixmap, path: &Option<tiny_skia::Path>, c: [u8; 3], width: f32, dash: bool| {
        if let Some(p) = path {
            let mut s = Stroke { width, line_cap: LineCap::Round, ..Default::default() };
            if dash {
                s.dash = StrokeDash::new(vec![6.0, 5.0], 0.0);
            }
            pm.stroke_path(p, &paint(c, 255), &s, t, None);
        }
    };
    let say = |pm: &mut Pixmap, s: &str, x: f32, y: f32, size: f32, face: Face, c: [u8; 3], align: f32| {
        let (px, py) = at(x, y);
        text.draw_in(pm, s, px, py, size * scale, face, c, align);
    };
    // The grid of the canvas, every 24 units, as it lies on the canvas.
    {
        let mut pb = PathBuilder::new();
        let (gx0, gy0) = (((-ox) / 24.0).floor() * 24.0, ((-oy) / 24.0).floor() * 24.0);
        let (gx1, gy1) = (w - ox, h - oy);
        let mut x = gx0;
        while x <= gx1 {
            pb.move_to(x, gy0);
            pb.line_to(x, gy1);
            x += 24.0;
        }
        let mut y = gy0;
        while y <= gy1 {
            pb.move_to(gx0, y);
            pb.line_to(gx1, y);
            y += 24.0;
        }
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &paint(GRID, 255), &Stroke { width: 1.0 / scale.max(0.5), ..Default::default() }, t, None);
        }
    }
    // A triangle that points down, as the arrow that folds a node or a note.
    let fold = |pm: &mut Pixmap, x: f32, y: f32, c: [u8; 3]| {
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + 9.0, y);
        pb.line_to(x + 4.5, y + 8.0);
        pb.close();
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &paint(c, 255), FillRule::Winding, t, None);
        }
    };
    if let Some(tt) = &title {
        say(&mut pm, tt, x0, y0 - top + 36.0, 24.0, Face::Bold, INK, 0.0);
    }
    for (i, l) in described.iter().enumerate() {
        say(&mut pm, l, x0, y0 - top + headed + 14.0 + i as f32 * 20.0, 14.0, Face::Regular, MUTED, 0.0);
    }
    // Group boxes, beneath everything, with their names in a strip at the top.
    for g in &ws.groups {
        let (edge, soft) = group_colour(&g.color);
        let (gx, gy, gw, gh) = (g.x as f32, g.y as f32, g.w as f32, g.h as f32);
        let r = rounded(gx, gy, gw, gh, 6.0);
        fill(&mut pm, &r, soft, 150);
        fill(&mut pm, &rounded4(gx + 1.0, gy + 1.0, gw - 2.0, GROUP_TITLE.min(gh) - 1.0, [5.0, 5.0, 0.0, 0.0]), soft, 150);
        line(&mut pm, &r, edge, 1.0, false);
        let name = if g.name.trim().is_empty() { "Group" } else { g.name.as_str() };
        let name = text.fit(name, 13.5, true, gw - 20.0);
        say(&mut pm, &name, gx + 10.0, gy + GROUP_TITLE / 2.0 + 5.0, 13.5, Face::Bold, INK, 0.0);
    }
    // The comments, each in a box with a thin line to its node.
    for n in ws.nodes.iter().filter(|n| !n.comment.trim().is_empty()) {
        let b = &laid[n.id.as_str()];
        let [cx, cy, cw, ch] = comment_box(&text, n, b);
        let (mx, my) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
        let from = (mx.clamp(cx, cx + cw), my.clamp(cy, cy + ch));
        let to = (from.0.clamp(b.x, b.x + b.w), from.1.clamp(b.y, b.y + b.h));
        let apart = cx > b.x + b.w || cx + cw < b.x || cy > b.y + b.h || cy + ch < b.y;
        if apart {
            let mut pb = PathBuilder::new();
            pb.move_to(from.0, from.1);
            pb.line_to(to.0, to.1);
            line(&mut pm, &pb.finish(), NOTE_LINE, 1.0, false);
            fill(&mut pm, &PathBuilder::from_circle(to.0, to.1, 2.5), NOTE_LINE, 255);
        }
        let r = rounded(cx, cy, cw, ch, 5.0);
        fill(&mut pm, &r, NOTE, 255);
        line(&mut pm, &r, NOTE_LINE, 1.0, false);
        let fits = (((ch - 10.0) / COMMENT_LINE).floor() as usize).max(1);
        for (k, l) in wrap(&text, n.comment.trim(), COMMENT, Face::Regular, cw - 16.0).iter().take(fits).enumerate() {
            say(&mut pm, l, cx + 8.0, cy + 5.0 + 12.0 + k as f32 * COMMENT_LINE, COMMENT, Face::Regular, INK, 0.0);
        }
    }
    // The connections, from the output port on the right edge to the input port on the left.
    for e in &ws.edges {
        let (Some(a), Some(b)) = (laid.get(e.from.0.as_str()), laid.get(e.to.0.as_str())) else { continue };
        let ya = a.outs.iter().find(|p| p.0 == e.from.1).or(a.outs.first()).map_or(a.h / 2.0, |p| p.1);
        let yb = b.ins.iter().find(|p| p.0 == e.to.1).or(b.ins.first()).map_or(b.h / 2.0, |p| p.1);
        let (sx, sy) = (a.x + a.w, a.y + ya);
        let (ex, ey) = (b.x, b.y + yb);
        let mut pb = PathBuilder::new();
        pb.move_to(sx, sy);
        let mut prev = (sx, sy);
        for q in &e.points {
            let (qx, qy) = (q[0] as f32, q[1] as f32);
            let d = ((qx - prev.0).abs() / 2.0).max(40.0);
            pb.cubic_to(prev.0 + d, prev.1, qx - d, qy, qx, qy);
            prev = (qx, qy);
        }
        let d = ((ex - prev.0).abs() / 2.0).max(60.0);
        pb.cubic_to(prev.0 + d, prev.1, ex - d, ey, ex, ey);
        line(&mut pm, &pb.finish(), WIRE, 2.25, e.disabled);
    }
    // The nodes: the header in the colour of their kind, then the inputs, the settings and
    // the outputs, with the ports on the edges.
    for n in &ws.nodes {
        let b = &laid[n.id.as_str()];
        let soft = if n.disabled { LINE } else { node_type(&n.kind).map_or([0xfb, 0xe3, 0xe1], |t| tint(t.group).1) };
        let body = rounded(b.x, b.y, b.w, b.h, 5.0);
        fill(&mut pm, &body, if n.disabled { PANEL } else { SURFACE }, 255);
        fill(&mut pm, &rounded4(b.x, b.y, b.w, HEAD, [4.0, 4.0, 0.0, 0.0]), soft, 255);
        line(&mut pm, &body, LINE, 1.0, n.disabled);
        let ink = if n.disabled { MUTED } else { INK };
        fold(&mut pm, b.x + 8.0, b.y + 10.0, ink);
        let room = if n.disabled { text.width_in("disabled", 10.5, Face::Mono) + 6.0 } else { 0.0 };
        let name = text.fit(n.label(), NAME, true, b.w - 32.0 - room);
        say(&mut pm, &name, b.x + 24.0, b.y + 19.0, NAME, Face::Bold, ink, 0.0);
        if n.disabled {
            say(&mut pm, "disabled", b.x + b.w - 8.0, b.y + 18.0, 10.5, Face::Mono, MUTED, 1.0);
        }
        for (k, l) in b.body.iter().enumerate() {
            say(&mut pm, l, b.x + 8.0, b.y + b.body_y + 10.0 + k as f32 * BODY_LINE, BODY, Face::Mono, MUTED, 0.0);
        }
        let pin = |pm: &mut Pixmap, x: f32, y: f32| {
            fill(pm, &PathBuilder::from_circle(x, y, 4.5), SURFACE, 255);
            line(pm, &PathBuilder::from_circle(x, y, 4.5), WIRE, 1.5, false);
        };
        for (port, py) in &b.ins {
            pin(&mut pm, b.x, b.y + py);
            say(&mut pm, port, b.x + 12.0, b.y + py + 4.0, PORT, Face::Mono, MUTED, 0.0);
        }
        for (port, py) in &b.outs {
            pin(&mut pm, b.x + b.w, b.y + py);
            say(&mut pm, port, b.x + b.w - 12.0, b.y + py + 4.0, PORT, Face::Mono, MUTED, 1.0);
        }
    }
    // The notes: a sheet of paper with a strip to take hold of it by.
    for (nx, ny, nw, nh, lines) in &notes {
        let r = rounded(*nx, *ny, *nw, *nh, 5.0);
        fill(&mut pm, &r, NOTE, 255);
        line(&mut pm, &r, NOTE_LINE, 1.0, false);
        fold(&mut pm, nx + 8.0, ny + 6.0, INK);
        for (k, l) in lines.iter().enumerate() {
            say(&mut pm, l, nx + 8.0, ny + 20.0 + 13.0 + k as f32 * 17.0, 13.0, Face::Regular, INK, 0.0);
        }
    }
    // Opaque all through: the pixmap holds colours times opacity, and here all is opaque.
    Ok(RasterImage { vector: None, width: pw as usize, height: ph as usize, rgba: pm.data().to_vec(), geotransform: [0.0, 1.0, 0.0, ph as f64, 0.0, -1.0] })
}

/// A picture as a PDF page of its own size, at a resolution: the pixels laid on white and
/// packed with zlib.
pub fn write_pdf(img: &RasterImage, dpi: f64, path: &std::path::Path) -> Result<()> {
    use std::io::Write;
    let mut rgb = Vec::with_capacity(img.width * img.height * 3);
    for p in img.rgba.as_chunks::<4>().0.iter() {
        let a = p[3] as u32;
        for c in &p[..3] {
            rgb.push(((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8);
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&rgb).map_err(|e| GbError(format!("Cannot pack the picture: {}", e)))?;
    let packed = z.finish().map_err(|e| GbError(format!("Cannot pack the picture: {}", e)))?;
    let (wpt, hpt) = (img.width as f64 * 72.0 / dpi, img.height as f64 * 72.0 / dpi);
    let content = format!("q {:.2} 0 0 {:.2} 0 0 cm /Im0 Do Q", wpt, hpt);
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut out,
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>", wpt, hpt).as_bytes(),
    );
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        img.width,
        img.height,
        packed.len()
    )
    .into_bytes();
    image.extend_from_slice(&packed);
    image.extend_from_slice(b"\nendstream");
    obj(&mut out, &image);
    let mut c = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    c.extend_from_slice(content.as_bytes());
    c.extend_from_slice(b"\nendstream");
    obj(&mut out, &c);
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", offsets.len() + 1, xref).as_bytes());
    std::fs::write(path, out).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))
}

/// Draws a model to a file: .png, .jpg, .webp or .pdf, by its extension.
pub fn export(ws: &Workspace, path: &std::path::Path) -> Result<()> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let img = draw(ws, 2.0)?;
    match ext.as_str() {
        "pdf" => write_pdf(&img, 144.0, path),
        "jpg" | "jpeg" => {
            let rgb: Vec<u8> = img.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
            image::save_buffer(path, &rgb, img.width as u32, img.height as u32, image::ColorType::Rgb8)
                .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))
        }
        // WebP keeps every pixel, and the see-through parts, in a smaller file than PNG.
        "webp" => image::save_buffer(path, &img.rgba, img.width as u32, img.height as u32, image::ColorType::Rgba8)
            .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e))),
        "png" => image::save_buffer(path, &img.rgba, img.width as u32, img.height as u32, image::ColorType::Rgba8)
            .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e))),
        _ => Err(GbError(format!("A picture of the model is written as .png, .jpg, .webp or .pdf, not .{}.", ext))),
    }
}
