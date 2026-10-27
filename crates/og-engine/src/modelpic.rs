//! A model as a picture: its nodes with their names, how they are set up and their ports,
//! the connections between them, group boxes, comments and notes, as on the canvas. Written
//! as PNG or JPEG, or as a PDF page, for documentation and to show others how a flow works.

use crate::chart::Text;
use crate::model::{GbError, RasterImage, Result};
use crate::nodes::{node_type, Group, P};
use crate::run::Workspace;
use std::collections::HashMap;
use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Rect, Stroke, StrokeDash, Transform};

const NODE_W: f32 = 196.0;
const HEAD: f32 = 30.0;
const ROW: f32 = 20.0;

const BG: [u8; 3] = [0xf4, 0xf7, 0xf6];
const SURFACE: [u8; 3] = [0xff, 0xff, 0xff];
const INK: [u8; 3] = [0x14, 0x20, 0x1d];
const MUTED: [u8; 3] = [0x55, 0x64, 0x5f];
const LINE: [u8; 3] = [0xc6, 0xd1, 0xcd];
const WIRE: [u8; 3] = [0x3a, 0x45, 0x42];
const NOTE: [u8; 3] = [0xff, 0xf7, 0xcf];
const NOTE_LINE: [u8; 3] = [0xd6, 0xbf, 0x63];

/// Where a node is: x, y, height, and its input and output ports.
type Placed = (f32, f32, f32, Vec<String>, Vec<String>);

/// The colour of a kind of node, and its light tint, as on the canvas.
fn tint(group: Group) -> ([u8; 3], [u8; 3]) {
    match group {
        Group::Reader => ([0x7d, 0x56, 0x00], [0xf8, 0xeb, 0xc6]),
        Group::Writer => ([0x1f, 0x6a, 0x3d], [0xd9, 0xef, 0xe1]),
        Group::Transformer => ([0x1c, 0x55, 0x96], [0xdb, 0xe7, 0xf6]),
    }
}

fn group_colour(name: &str) -> ([u8; 3], [u8; 3]) {
    match name {
        "yellow" => tint(Group::Reader),
        "green" => tint(Group::Writer),
        "pink" | "magenta" => ([0xa3, 0x17, 0x5c], [0xf7, 0xdc, 0xea]),
        _ => tint(Group::Transformer),
    }
}

fn paint(c: [u8; 3], a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], a));
    p.anti_alias = true;
    p
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let r = r.min(w / 2.0).min(h / 2.0);
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Text cut into lines that fit a width.
fn wrap(text: &Text, s: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.lines() {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let try_line = if line.is_empty() { word.to_string() } else { format!("{} {}", line, word) };
            if text.width(&try_line, size, false) > width && !line.is_empty() {
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
            (t.inputs.iter().map(|s| s.to_string()).collect(), outs)
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

/// The model drawn, `scale` pixels to a unit of the canvas.
pub fn draw(ws: &Workspace, scale: f32) -> Result<RasterImage> {
    let text = Text::new()?;
    let notes_w = 240.0f32;
    // Where each node is, and how tall.
    let mut boxes: HashMap<&str, Placed> = HashMap::new();
    for n in &ws.nodes {
        let (ins, outs) = ports(ws, &n.id, &n.kind, &n.params);
        let rows = ins.len().max(outs.len()).max(1) as f32;
        let h = HEAD + 22.0 + rows * ROW + 8.0;
        boxes.insert(n.id.as_str(), (n.x as f32, n.y as f32, h, ins, outs));
    }
    // The extent of it all.
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut grow = |x: f32, y: f32, w: f32, h: f32| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + w);
        y1 = y1.max(y + h);
    };
    for n in &ws.nodes {
        let b = &boxes[n.id.as_str()];
        // Room for a comment above.
        let above = if n.comment.trim().is_empty() { 0.0 } else { 60.0 };
        grow(b.0, b.1 - above, NODE_W, b.2 + above);
    }
    for g in &ws.groups {
        grow(g.x as f32, g.y as f32 - 26.0, g.w as f32, g.h as f32 + 26.0);
    }
    for note in &ws.notes {
        let w = note.w.map_or(notes_w, |w| w as f32);
        let lines = wrap(&text, &note.text, 13.0, w - 20.0).len() as f32;
        grow(note.x as f32, note.y as f32, w, lines * 18.0 + 20.0);
    }
    if !x0.is_finite() {
        return Err(GbError("The model has nothing to draw.".into()));
    }
    let title = if ws.name.trim().is_empty() { None } else { Some(ws.name.trim().to_string()) };
    let margin = 40.0;
    let top = if title.is_some() { 60.0 } else { 0.0 };
    let (w, h) = (x1 - x0 + 2.0 * margin, y1 - y0 + 2.0 * margin + top);
    let (pw, ph) = ((w * scale).ceil() as u32, (h * scale).ceil() as u32);
    if (pw as u64) * (ph as u64) > 120_000_000 {
        return Err(GbError("The model is too large to draw at this size. Draw it smaller.".into()));
    }
    let mut pm = Pixmap::new(pw.max(1), ph.max(1)).ok_or_else(|| GbError("The picture of the model is too large.".into()))?;
    pm.fill(Color::from_rgba8(BG[0], BG[1], BG[2], 255));
    let t = Transform::from_scale(scale, scale).pre_translate(margin - x0, margin - y0 + top);
    // Where a point of the canvas is in the picture, for text, which is drawn in pixels.
    let at = |x: f32, y: f32| ((x + margin - x0) * scale, (y + margin - y0 + top) * scale);
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
    let say = |pm: &mut Pixmap, s: &str, x: f32, y: f32, size: f32, bold: bool, c: [u8; 3], align: f32| {
        let (px, py) = at(x, y);
        text.draw(pm, s, px, py, size * scale, bold, c, align);
    };
    if let Some(tt) = &title {
        say(&mut pm, tt, x0, y0 - 24.0, 24.0, true, INK, 0.0);
    }
    // Group boxes, beneath everything.
    for g in &ws.groups {
        let (c, soft) = group_colour(&g.color);
        let r = rounded(g.x as f32, g.y as f32, g.w as f32, g.h as f32, 10.0);
        fill(&mut pm, &r, soft, 150);
        line(&mut pm, &r, c, 1.2, false);
        say(&mut pm, &g.name, g.x as f32 + 8.0, g.y as f32 - 8.0, 15.0, true, c, 0.0);
    }
    // The connections, from the right of the output port to the left of the input port.
    let port_y = |b: &Placed, k: usize| b.1 + HEAD + 22.0 + (k as f32 + 0.5) * ROW;
    for e in &ws.edges {
        let (Some(a), Some(b)) = (boxes.get(e.from.0.as_str()), boxes.get(e.to.0.as_str())) else { continue };
        let ka = a.4.iter().position(|p| *p == e.from.1).unwrap_or(0);
        let kb = b.3.iter().position(|p| *p == e.to.1).unwrap_or(0);
        let (sx, sy) = (a.0 + NODE_W, port_y(a, ka));
        let (ex, ey) = (b.0, port_y(b, kb));
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
        line(&mut pm, &pb.finish(), WIRE, 1.8, e.disabled);
    }
    // The nodes.
    for n in &ws.nodes {
        let b = &boxes[n.id.as_str()];
        let ty = node_type(&n.kind);
        let (c, soft) = tint(ty.map_or(Group::Transformer, |t| t.group));
        let alpha = if n.disabled { 110 } else { 255 };
        let body = rounded(b.0, b.1, NODE_W, b.2, 8.0);
        fill(&mut pm, &body, SURFACE, alpha);
        let head = rounded(b.0, b.1, NODE_W, HEAD, 8.0);
        fill(&mut pm, &head, soft, alpha);
        fill(&mut pm, &Rect::from_xywh(b.0, b.1 + HEAD - 8.0, NODE_W, 8.0).map(PathBuilder::from_rect), soft, alpha);
        line(&mut pm, &body, c, 1.4, n.disabled);
        let name = text.fit(n.label(), 14.0, true, NODE_W - 20.0);
        say(&mut pm, &name, b.0 + 10.0, b.1 + 20.0, 14.0, true, INK, 0.0);
        if let Some(ty) = ty {
            let p = P::new(&n.params, ty.params);
            let line_text = (ty.summary)(&p);
            let line_text = if line_text.trim().is_empty() { ty.title.to_string() } else { line_text };
            let s = text.fit(&line_text, 11.0, false, NODE_W - 20.0);
            say(&mut pm, &s, b.0 + 10.0, b.1 + HEAD + 15.0, 11.0, false, MUTED, 0.0);
        }
        for (k, port) in b.3.iter().enumerate() {
            let y = port_y(b, k);
            fill(&mut pm, &PathBuilder::from_circle(b.0, y, 4.5), SURFACE, 255);
            line(&mut pm, &PathBuilder::from_circle(b.0, y, 4.5), WIRE, 1.3, false);
            say(&mut pm, port, b.0 + 10.0, y + 4.5, 12.0, false, INK, 0.0);
        }
        for (k, port) in b.4.iter().enumerate() {
            let y = port_y(b, k);
            fill(&mut pm, &PathBuilder::from_circle(b.0 + NODE_W, y, 4.5), SURFACE, 255);
            line(&mut pm, &PathBuilder::from_circle(b.0 + NODE_W, y, 4.5), WIRE, 1.3, false);
            say(&mut pm, port, b.0 + NODE_W - 10.0, y + 4.5, 12.0, false, INK, 1.0);
        }
        // The comment, in a box above the node or where it was put.
        if !n.comment.trim().is_empty() {
            let (cx, cy, cw) = match n.comment_box {
                Some(cb) => (b.0 + cb[0] as f32, b.1 + cb[1] as f32, cb[2] as f32),
                None => (b.0, b.1 - 54.0, NODE_W),
            };
            let lines = wrap(&text, &n.comment, 12.0, cw - 16.0);
            let lines: Vec<String> = lines.into_iter().take(3).collect();
            let ch = lines.len() as f32 * 16.0 + 10.0;
            let r = rounded(cx, cy, cw, ch, 6.0);
            fill(&mut pm, &r, SURFACE, 230);
            line(&mut pm, &r, LINE, 1.0, false);
            for (k, l) in lines.iter().enumerate() {
                say(&mut pm, l, cx + 8.0, cy + 18.0 + k as f32 * 16.0, 12.0, false, MUTED, 0.0);
            }
        }
    }
    for note in &ws.notes {
        let w = note.w.map_or(notes_w, |w| w as f32);
        let lines = wrap(&text, &note.text, 13.0, w - 20.0);
        let r = rounded(note.x as f32, note.y as f32, w, lines.len() as f32 * 18.0 + 20.0, 6.0);
        fill(&mut pm, &r, NOTE, 255);
        line(&mut pm, &r, NOTE_LINE, 1.2, false);
        for (k, l) in lines.iter().enumerate() {
            say(&mut pm, l, note.x as f32 + 10.0, note.y as f32 + 24.0 + k as f32 * 18.0, 13.0, false, INK, 0.0);
        }
    }
    // Opaque all through: the pixmap holds colours times opacity, and here all is opaque.
    Ok(RasterImage { width: pw as usize, height: ph as usize, rgba: pm.data().to_vec(), geotransform: [0.0, 1.0, 0.0, ph as f64, 0.0, -1.0] })
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

/// Draws a model to a file: .png, .jpg or .pdf, by its extension.
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
        "png" => image::save_buffer(path, &img.rgba, img.width as u32, img.height as u32, image::ColorType::Rgba8)
            .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e))),
        _ => Err(GbError(format!("A picture of the model is written as .png, .jpg or .pdf, not .{}.", ext))),
    }
}
