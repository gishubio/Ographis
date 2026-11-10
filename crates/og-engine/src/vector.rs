//! A surface that draws into a picture and, when asked, keeps what was drawn as shapes: the
//! paths, colours and pictures of a map, so that it can also be written as a PDF or an SVG of
//! lines and areas rather than of pixels. Added in 0.96. Text is kept as the outlines of its
//! letters; rasters, background maps, icons and heatmaps as pictures.

use tiny_skia::{FillRule, Paint, Path, PathSegment, Pixmap, PixmapPaint, PixmapRef, Rect, Stroke, Transform};

/// A piece of a path, in pixels of the picture, y downwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// One thing drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Fill {
        path: Vec<Seg>,
        rgba: [u8; 4],
        even_odd: bool,
    },
    Stroke {
        path: Vec<Seg>,
        rgba: [u8; 4],
        width: f32,
        dash: Vec<f32>,
        round_cap: bool,
        round_join: bool,
    },
    /// A picture, not premultiplied, laid at x, y in pixels.
    Image {
        x: f32,
        y: f32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        opacity: f32,
    },
}

/// What was drawn, from the bottom up, on a picture of this size in pixels.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Picture {
    pub width: f32,
    pub height: f32,
    pub ops: Vec<Op>,
    /// The names of the layers it is drawn in, such as those of a map.
    pub layers: Vec<String>,
    /// Where each layer begins: from this op on, what is drawn is in that layer, until
    /// the next. Empty: it is all one drawing.
    pub starts: Vec<(usize, usize)>,
}

/// A picture to draw on, which keeps what is drawn when it is to be a vector picture too.
pub struct Canvas {
    pm: Pixmap,
    kept: Option<Vec<Op>>,
    layers: Vec<String>,
    starts: Vec<(usize, usize)>,
    /// How much has been kept, in pieces of paths and pixels of pictures.
    size: usize,
}

/// Beyond this, a map is too large to keep as shapes too, and is kept as a picture only.
const MOST: usize = 20_000_000;

fn segments(path: &Path, ts: Transform) -> Vec<Seg> {
    let at = |p: tiny_skia::Point| {
        let mut q = [p];
        ts.map_points(&mut q);
        (q[0].x, q[0].y)
    };
    path.segments()
        .map(|s| match s {
            PathSegment::MoveTo(p) => {
                let (x, y) = at(p);
                Seg::Move(x, y)
            }
            PathSegment::LineTo(p) => {
                let (x, y) = at(p);
                Seg::Line(x, y)
            }
            PathSegment::QuadTo(a, b) => {
                let ((ax, ay), (bx, by)) = (at(a), at(b));
                Seg::Quad(ax, ay, bx, by)
            }
            PathSegment::CubicTo(a, b, c) => {
                let ((ax, ay), (bx, by), (cx, cy)) = (at(a), at(b), at(c));
                Seg::Cubic(ax, ay, bx, by, cx, cy)
            }
            PathSegment::Close => Seg::Close,
        })
        .collect()
}

fn colour_of(paint: &Paint) -> Option<[u8; 4]> {
    match &paint.shader {
        tiny_skia::Shader::SolidColor(c) => {
            let c = c.to_color_u8();
            Some([c.red(), c.green(), c.blue(), c.alpha()])
        }
        _ => None,
    }
}

impl Canvas {
    /// A see-through picture; with `keep`, what is drawn is kept as shapes too.
    pub fn new(width: u32, height: u32, keep: bool) -> Option<Canvas> {
        Some(Canvas { pm: Pixmap::new(width, height)?, kept: keep.then(Vec::new), layers: Vec::new(), starts: Vec::new(), size: 0 })
    }

    pub fn width(&self) -> u32 {
        self.pm.width()
    }

    pub fn height(&self) -> u32 {
        self.pm.height()
    }

    /// The pixels, premultiplied, as tiny-skia keeps them.
    pub fn pixels(&self) -> &[tiny_skia::PremultipliedColorU8] {
        self.pm.pixels()
    }

    /// Keeps a shape, while there is room for it.
    fn keep(&mut self, op: Op, size: usize) {
        self.size += size;
        if self.size > MOST {
            self.kept = None;
        }
        if let Some(k) = &mut self.kept {
            k.push(op);
        }
    }

    /// What is drawn from now on is in the layer of this name, for a PDF or an SVG with
    /// layers. A name used before goes on in that layer.
    pub fn layer(&mut self, name: Option<&str>) {
        let Some(ops) = &self.kept else { return };
        let Some(name) = name else { return };
        let k = match self.layers.iter().position(|l| l == name) {
            Some(k) => k,
            None => {
                self.layers.push(name.to_string());
                self.layers.len() - 1
            }
        };
        let at = ops.len();
        match self.starts.last_mut() {
            // Nothing was drawn in the layer before: it begins here instead.
            Some(last) if last.0 == at => last.1 = k,
            Some(last) if last.1 == k => {}
            _ => self.starts.push((at, k)),
        }
    }

    /// The shapes that were kept, if they were, with only the layers something was drawn in.
    pub fn picture(&self) -> Option<Picture> {
        let ops = self.kept.as_ref()?;
        let mut starts: Vec<(usize, usize)> = self.starts.iter().copied().filter(|s| s.0 < ops.len()).collect();
        starts.dedup_by(|b, a| a.1 == b.1);
        let mut used: Vec<usize> = starts.iter().map(|s| s.1).collect();
        used.sort_unstable();
        used.dedup();
        let layers: Vec<String> = used.iter().map(|k| self.layers[*k].clone()).collect();
        let starts = starts.into_iter().map(|(at, k)| (at, used.iter().position(|u| *u == k).unwrap_or(0))).collect();
        Some(Picture { width: self.pm.width() as f32, height: self.pm.height() as f32, ops: ops.clone(), layers, starts })
    }

    pub fn fill(&mut self, c: tiny_skia::Color) {
        self.pm.fill(c);
        if self.kept.is_some() {
            let c = c.to_color_u8();
            let (w, h) = (self.pm.width() as f32, self.pm.height() as f32);
            let path = vec![Seg::Move(0.0, 0.0), Seg::Line(w, 0.0), Seg::Line(w, h), Seg::Line(0.0, h), Seg::Close];
            self.keep(Op::Fill { path, rgba: [c.red(), c.green(), c.blue(), c.alpha()], even_odd: false }, 5);
        }
    }

    pub fn fill_path(&mut self, path: &Path, paint: &Paint, rule: FillRule, ts: Transform, mask: Option<&tiny_skia::Mask>) {
        self.pm.fill_path(path, paint, rule, ts, mask);
        if let (true, Some(rgba)) = (self.kept.is_some(), colour_of(paint)) {
            let segs = segments(path, ts);
            let n = segs.len();
            self.keep(Op::Fill { path: segs, rgba, even_odd: rule == FillRule::EvenOdd }, n);
        }
    }

    pub fn stroke_path(&mut self, path: &Path, paint: &Paint, stroke: &Stroke, ts: Transform, mask: Option<&tiny_skia::Mask>) {
        self.pm.stroke_path(path, paint, stroke, ts, mask);
        if let (true, Some(rgba)) = (self.kept.is_some(), colour_of(paint)) {
            let segs = segments(path, ts);
            let n = segs.len();
            let op = Op::Stroke {
                path: segs,
                rgba,
                width: stroke.width,
                dash: stroke.dash.as_ref().map(dash_of).unwrap_or_default(),
                round_cap: stroke.line_cap == tiny_skia::LineCap::Round,
                round_join: stroke.line_join == tiny_skia::LineJoin::Round,
            };
            self.keep(op, n);
        }
    }

    pub fn fill_rect(&mut self, r: Rect, paint: &Paint, ts: Transform, mask: Option<&tiny_skia::Mask>) {
        let path = tiny_skia::PathBuilder::from_rect(r);
        self.fill_path(&path, paint, FillRule::Winding, ts, mask);
    }

    /// Lays a picture at x, y. Only moving it is kept, not turning or scaling it.
    pub fn draw_pixmap(&mut self, x: i32, y: i32, pixmap: PixmapRef, paint: &PixmapPaint, ts: Transform, mask: Option<&tiny_skia::Mask>) {
        self.pm.draw_pixmap(x, y, pixmap, paint, ts, mask);
        if self.kept.is_some() {
            let mut rgba = Vec::with_capacity(pixmap.data().len());
            let mut any = false;
            for p in pixmap.pixels() {
                let c = p.demultiply();
                any |= c.alpha() > 0;
                rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
            }
            if any {
                let n = (pixmap.width() * pixmap.height()) as usize / 4;
                self.keep(
                    Op::Image { x: x as f32 + ts.tx, y: y as f32 + ts.ty, width: pixmap.width(), height: pixmap.height(), rgba, opacity: paint.opacity },
                    n,
                );
            }
        }
    }

    /// Draws with something that works on the pixels themselves, such as an icon, in a
    /// picture of its own that is then laid at x, y, so that it is kept as a picture.
    pub fn patch(&mut self, x: i32, y: i32, width: u32, height: u32, draw: impl FnOnce(&mut Pixmap)) {
        let Some(mut p) = Pixmap::new(width.max(1), height.max(1)) else { return };
        draw(&mut p);
        self.draw_pixmap(x, y, p.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
}

fn dash_of(d: &tiny_skia::StrokeDash) -> Vec<f32> {
    // tiny-skia keeps the intervals to itself; they are read back from its debug form.
    let text = format!("{:?}", d);
    let Some(start) = text.find('[') else { return Vec::new() };
    let Some(end) = text[start..].find(']') else { return Vec::new() };
    text[start + 1..start + end].split(',').filter_map(|v| v.trim().parse::<f32>().ok()).collect()
}

/// A number as PDF and SVG want it: short, with a point.
fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{}", r)
    }
}

/// Text for a PDF string in parentheses: backslashes and parentheses escaped.
fn pdf_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)").replace(['\n', '\r'], " ")
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// Where a picture lies on the ground, for a PDF with coordinates: the coordinate system,
/// and the corners of the picture in degrees.
pub struct Geo {
    pub epsg: u32,
    /// The coordinate system as WKT, when it is known.
    pub wkt: Option<String>,
    /// Latitude and longitude of the corners: lower left, upper left, upper right, lower
    /// right.
    pub corners: [(f64, f64); 4],
}

/// The picture as a PDF page of `page_w` by `page_h` points, laid with its upper left
/// corner at `left`, `top` (from the top of the page) and `scale` points to a pixel, with an
/// optional heading of outlined letters. With `geo`, the page says where the picture lies
/// (as in ISO 32000-2), so that GIS programs place it on the map.
#[allow(clippy::too_many_arguments)]
pub fn pdf(pic: &Picture, page_w: f64, page_h: f64, left: f64, top: f64, scale: f64, heading: Option<(&str, f64, f64)>, geo: Option<&Geo>) -> Vec<u8> {
    let mut content = String::new();
    let mut images: Vec<(u32, u32, Vec<u8>, Vec<u8>)> = Vec::new();
    let mut alphas: Vec<u8> = Vec::new();
    // From the pixels of the picture, y down, to points of the page, y up.
    content.push_str(&format!("q {} 0 0 {} {} {} cm\n", num(scale as f32), num(-scale as f32), num(left as f32), num((page_h - top) as f32)));
    // The picture is clipped to its own rectangle.
    content.push_str(&format!("0 0 {} {} re W n\n", num(pic.width), num(pic.height)));
    let gs_of = |a: u8, alphas: &mut Vec<u8>| -> usize {
        match alphas.iter().position(|x| *x == a) {
            Some(i) => i,
            None => {
                alphas.push(a);
                alphas.len() - 1
            }
        }
    };
    let path_text = |path: &[Seg]| {
        let mut s = String::new();
        for seg in path {
            match *seg {
                Seg::Move(x, y) => s.push_str(&format!("{} {} m\n", num(x), num(y))),
                Seg::Line(x, y) => s.push_str(&format!("{} {} l\n", num(x), num(y))),
                Seg::Quad(..) => {}
                Seg::Cubic(a, b, c, d, e, f) => s.push_str(&format!("{} {} {} {} {} {} c\n", num(a), num(b), num(c), num(d), num(e), num(f))),
                Seg::Close => s.push_str("h\n"),
            }
        }
        s
    };
    // A quadratic piece becomes a cubic one, which is what PDF has.
    let cubic = |path: &[Seg]| -> Vec<Seg> {
        let mut out = Vec::with_capacity(path.len());
        let mut at = (0.0f32, 0.0f32);
        let mut start = at;
        for seg in path {
            match *seg {
                Seg::Move(x, y) => {
                    at = (x, y);
                    start = at;
                    out.push(*seg);
                }
                Seg::Line(x, y) => {
                    at = (x, y);
                    out.push(*seg);
                }
                Seg::Quad(cx, cy, x, y) => {
                    let c1 = (at.0 + 2.0 / 3.0 * (cx - at.0), at.1 + 2.0 / 3.0 * (cy - at.1));
                    let c2 = (x + 2.0 / 3.0 * (cx - x), y + 2.0 / 3.0 * (cy - y));
                    out.push(Seg::Cubic(c1.0, c1.1, c2.0, c2.1, x, y));
                    at = (x, y);
                }
                Seg::Cubic(_, _, _, _, x, y) => {
                    at = (x, y);
                    out.push(*seg);
                }
                Seg::Close => {
                    at = start;
                    out.push(*seg);
                }
            }
        }
        out
    };
    // The layers, as optional content that a reader can turn off and on.
    let mut open = false;
    for (i, op) in pic.ops.iter().enumerate() {
        if let Some(&(_, k)) = pic.starts.iter().find(|s| s.0 == i) {
            if open {
                content.push_str("EMC\n");
            }
            content.push_str(&format!("/OC /L{} BDC\n", k));
            open = true;
        }
        match op {
            Op::Fill { path, rgba, even_odd } => {
                if rgba[3] == 0 {
                    continue;
                }
                let gs = gs_of(rgba[3], &mut alphas);
                content.push_str(&format!(
                    "/G{} gs {} {} {} rg\n{}{}\n",
                    gs,
                    num(rgba[0] as f32 / 255.0),
                    num(rgba[1] as f32 / 255.0),
                    num(rgba[2] as f32 / 255.0),
                    path_text(&cubic(path)),
                    if *even_odd { "f*" } else { "f" }
                ));
            }
            Op::Stroke { path, rgba, width, dash, round_cap, round_join } => {
                if rgba[3] == 0 || *width <= 0.0 {
                    continue;
                }
                let gs = gs_of(rgba[3], &mut alphas);
                let dashes =
                    if dash.is_empty() { "[] 0 d".to_string() } else { format!("[{}] 0 d", dash.iter().map(|d| num(*d)).collect::<Vec<_>>().join(" ")) };
                content.push_str(&format!(
                    "/G{} gs {} {} {} RG {} w {} J {} j {}\n{}S\n",
                    gs,
                    num(rgba[0] as f32 / 255.0),
                    num(rgba[1] as f32 / 255.0),
                    num(rgba[2] as f32 / 255.0),
                    num(*width),
                    if *round_cap { 1 } else { 0 },
                    if *round_join { 1 } else { 0 },
                    dashes,
                    path_text(&cubic(path))
                ));
            }
            Op::Image { x, y, width, height, rgba, opacity } => {
                let n = images.len();
                let mut rgb = Vec::with_capacity((*width * *height * 3) as usize);
                let mut a = Vec::with_capacity((*width * *height) as usize);
                for p in rgba.chunks_exact(4) {
                    rgb.extend_from_slice(&p[..3]);
                    a.push((p[3] as f32 * opacity.clamp(0.0, 1.0)).round() as u8);
                }
                images.push((*width, *height, rgb, a));
                // The picture is drawn upright: its rows go down as the pixels do.
                content.push_str(&format!("q {} 0 0 {} {} {} cm /I{} Do Q\n", width, -(*height as f32), num(*x), num(*y + *height as f32), n));
            }
        }
    }
    if open {
        content.push_str("EMC\n");
    }
    content.push_str("Q\n");
    if let Some((text, x, y)) = heading {
        if let Ok(t) = crate::chart::Text::new() {
            if let Some(path) = t.path(text, 16.0, true) {
                let segs = segments(&path, Transform::from_row(1.0, 0.0, 0.0, -1.0, x as f32, (page_h - y) as f32));
                content.push_str(&format!("0.1 0.1 0.1 rg\n{}f\n", path_text(&cubic(&segs))));
            }
        }
    }

    // The objects: catalogue, pages, page, contents, the states of opacity, the pictures,
    // the layers.
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let first_image = 5;
    let first_layer = first_image + images.len() * 2;
    let refs: Vec<String> = (0..pic.layers.len()).map(|k| format!("{} 0 R", first_layer + k)).collect();
    let catalog = if refs.is_empty() {
        "<< /Type /Catalog /Pages 2 0 R >>".to_string()
    } else {
        format!(
            "<< /Type /Catalog /Pages 2 0 R /PageMode /UseOC /OCProperties << /OCGs [{0}] /D << /Name (Layers) /Order [{0}] /ON [{0}] >> >> >>",
            refs.join(" ")
        )
    };
    objects.push(catalog.into_bytes());
    objects.push(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    let properties = if refs.is_empty() {
        String::new()
    } else {
        format!(" /Properties << {} >>", refs.iter().enumerate().map(|(k, r)| format!("/L{} {}", k, r)).collect::<Vec<_>>().join(" "))
    };
    let mut xobjects = String::new();
    for i in 0..images.len() {
        xobjects.push_str(&format!("/I{} {} 0 R ", i, first_image + i * 2));
    }
    let mut states = String::new();
    for (i, a) in alphas.iter().enumerate() {
        let v = num(*a as f32 / 255.0);
        states.push_str(&format!("/G{} << /Type /ExtGState /ca {} /CA {} >> ", i, v, v));
    }
    // The viewport of the picture on the page, with its corners on the ground.
    let viewport = geo.map(|g| {
        let (x1, y1) = (left, page_h - top - pic.height as f64 * scale);
        let (x2, y2) = (left + pic.width as f64 * scale, page_h - top);
        let gpts: Vec<String> = g.corners.iter().map(|(lat, lon)| format!("{:.9} {:.9}", lat, lon)).collect();
        let wkt = g.wkt.as_deref().map(|w| format!(" /WKT ({})", pdf_string(w))).unwrap_or_default();
        let kind = if g.wkt.as_deref().is_some_and(|w| w.trim_start().starts_with("GEOGCS") || w.trim_start().starts_with("GEOGCRS")) { "GEOGCS" } else { "PROJCS" };
        format!(
            " /VP [<< /Type /Viewport /Name (Map) /BBox [{:.3} {:.3} {:.3} {:.3}] /Measure << /Type /Measure /Subtype /GEO /Bounds [0 0 0 1 1 1 1 0] /GPTS [{}] /LPTS [0 0 0 1 1 1 1 0] /PDU [/M /SQM /DEG] /GCS << /Type /{} /EPSG {}{} >> >> >>]",
            x1,
            y1,
            x2,
            y2,
            gpts.join(" "),
            kind,
            g.epsg,
            wkt
        )
    });
    objects.push(
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}]{} /Contents 4 0 R /Resources << /ExtGState << {} >> /XObject << {} >>{} >> >>",
            num(page_w as f32),
            num(page_h as f32),
            viewport.unwrap_or_default(),
            states,
            xobjects,
            properties
        )
        .into_bytes(),
    );
    let packed = deflate(content.as_bytes());
    let mut c = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", packed.len()).into_bytes();
    c.extend_from_slice(&packed);
    c.extend_from_slice(b"\nendstream");
    objects.push(c);
    for (i, (w, h, rgb, a)) in images.iter().enumerate() {
        let mask_id = first_image + i * 2 + 1;
        let data = deflate(rgb);
        let mut o = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask {} 0 R /Length {} >>\nstream\n",
            w,
            h,
            mask_id,
            data.len()
        )
        .into_bytes();
        o.extend_from_slice(&data);
        o.extend_from_slice(b"\nendstream");
        objects.push(o);
        let data = deflate(a);
        let mut m = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
            w,
            h,
            data.len()
        )
        .into_bytes();
        m.extend_from_slice(&data);
        m.extend_from_slice(b"\nendstream");
        objects.push(m);
    }
    for name in &pic.layers {
        objects.push(format!("<< /Type /OCG /Name ({}) >>", pdf_string(name)).into_bytes());
    }
    let mut out = b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R /Info << /Producer (Ographis) >> >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref).as_bytes(),
    );
    out
}

/// The picture as an SVG of the same size, with its pictures as PNG inside it.
pub fn svg(pic: &Picture) -> String {
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        num(pic.width),
        num(pic.height),
        num(pic.width),
        num(pic.height)
    );
    let d = |path: &[Seg]| {
        let mut t = String::new();
        for seg in path {
            match *seg {
                Seg::Move(x, y) => t.push_str(&format!("M{} {}", num(x), num(y))),
                Seg::Line(x, y) => t.push_str(&format!("L{} {}", num(x), num(y))),
                Seg::Quad(a, b, x, y) => t.push_str(&format!("Q{} {} {} {}", num(a), num(b), num(x), num(y))),
                Seg::Cubic(a, b, c, e, x, y) => t.push_str(&format!("C{} {} {} {} {} {}", num(a), num(b), num(c), num(e), num(x), num(y))),
                Seg::Close => t.push('Z'),
            }
        }
        t
    };
    let colour = |c: &[u8; 4]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    // The layers, as groups that Inkscape shows as its layers.
    let mut open = false;
    for (i, op) in pic.ops.iter().enumerate() {
        if let Some(&(_, k)) = pic.starts.iter().find(|s| s.0 == i) {
            if open {
                s.push_str("</g>\n");
            }
            let name = pic.layers[k].replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
            s.push_str(&format!("<g inkscape:groupmode=\"layer\" inkscape:label=\"{}\">\n", name));
            open = true;
        }
        match op {
            Op::Fill { path, rgba, even_odd } => {
                s.push_str(&format!(
                    "<path d=\"{}\" fill=\"{}\"{}{}/>\n",
                    d(path),
                    colour(rgba),
                    if rgba[3] < 255 { format!(" fill-opacity=\"{}\"", num(rgba[3] as f32 / 255.0)) } else { String::new() },
                    if *even_odd { " fill-rule=\"evenodd\"" } else { "" }
                ));
            }
            Op::Stroke { path, rgba, width, dash, round_cap, round_join } => {
                s.push_str(&format!(
                    "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}{}{}{}/>\n",
                    d(path),
                    colour(rgba),
                    num(*width),
                    if rgba[3] < 255 { format!(" stroke-opacity=\"{}\"", num(rgba[3] as f32 / 255.0)) } else { String::new() },
                    if *round_cap { " stroke-linecap=\"round\"" } else { "" },
                    if *round_join { " stroke-linejoin=\"round\"" } else { "" },
                    if dash.is_empty() {
                        String::new()
                    } else {
                        format!(" stroke-dasharray=\"{}\"", dash.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" "))
                    }
                ));
            }
            Op::Image { x, y, width, height, rgba, opacity } => {
                let mut png = Vec::new();
                if let Some(img) = image::RgbaImage::from_raw(*width, *height, rgba.clone()) {
                    let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
                }
                s.push_str(&format!(
                    "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{} href=\"data:image/png;base64,{}\"/>\n",
                    num(*x),
                    num(*y),
                    width,
                    height,
                    if *opacity < 1.0 { format!(" opacity=\"{}\"", num(*opacity)) } else { String::new() },
                    base64(&png)
                ));
            }
        }
    }
    if open {
        s.push_str("</g>\n");
    }
    s.push_str("</svg>\n");
    s
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_drawn_is_kept_and_written_as_shapes() {
        let mut c = Canvas::new(100, 50, true).unwrap();
        c.fill(tiny_skia::Color::WHITE);
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(10.0, 10.0);
        pb.line_to(90.0, 40.0);
        let path = pb.finish().unwrap();
        let mut paint = Paint::default();
        paint.set_color_rgba8(200, 0, 0, 255);
        let st = Stroke { width: 3.0, dash: tiny_skia::StrokeDash::new(vec![6.0, 3.0], 0.0), ..Stroke::default() };
        c.stroke_path(&path, &paint, &st, Transform::identity(), None);
        let pic = c.picture().unwrap();
        assert_eq!(pic.ops.len(), 2);
        assert!(matches!(&pic.ops[1], Op::Stroke { dash, width, .. } if dash == &[6.0, 3.0] && *width == 3.0));
        let doc = pdf(&pic, 300.0, 200.0, 50.0, 50.0, 2.0, Some(("A map", 50.0, 30.0)), None);
        let text = String::from_utf8_lossy(&doc);
        assert!(text.starts_with("%PDF-1.5") && text.trim_end().ends_with("%%EOF"));
        assert!(text.contains("/MediaBox [0 0 300 200]"));
        let svg = svg(&pic);
        assert!(svg.contains("stroke-dasharray=\"6 3\"") && svg.contains("stroke=\"#c80000\""));
        // A picture that keeps nothing keeps nothing.
        assert!(Canvas::new(10, 10, false).unwrap().picture().is_none());
    }
}
