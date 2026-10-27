//! Bar charts drawn into pictures: bars across for many or long names, columns for a few.
//! The look is quiet: thin bars with a rounded end, hairline grid, text in grey and black,
//! and colour only on the data. The text is set in Inter, which is built into the program.

use crate::model::{GbError, RasterImage, Result};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

static REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.otf");
static SEMIBOLD: &[u8] = include_bytes!("../fonts/Inter-SemiBold.otf");

/// The colours a bar can have, in the order they are offered, with the light and dark
/// steps of the reference palette.
pub const COLOURS: &[(&str, &str, [u8; 3])] = &[
    ("blue", "Blue", [0x2a, 0x78, 0xd6]),
    ("orange", "Orange", [0xeb, 0x68, 0x34]),
    ("aqua", "Aqua", [0x1b, 0xaf, 0x7a]),
    ("yellow", "Yellow", [0xed, 0xa1, 0x00]),
    ("magenta", "Magenta", [0xe8, 0x7b, 0xa4]),
    ("green", "Green", [0x00, 0x83, 0x00]),
    ("violet", "Violet", [0x62, 0x50, 0xd6]),
    ("red", "Red", [0xe3, 0x49, 0x48]),
    ("grey", "Grey", [0x89, 0x87, 0x81]),
];

pub fn colour(name: &str) -> [u8; 3] {
    COLOURS.iter().find(|c| c.0 == name).map_or(COLOURS[0].2, |c| c.2)
}

const INK: [u8; 3] = [0x0b, 0x0b, 0x0b];
const SECONDARY: [u8; 3] = [0x52, 0x51, 0x4e];
const MUTED: [u8; 3] = [0x89, 0x87, 0x81];
const GRID: [u8; 3] = [0xe1, 0xe0, 0xd9];
const BASELINE: [u8; 3] = [0xc3, 0xc2, 0xb7];
const SURFACE: [u8; 3] = [0xfc, 0xfc, 0xfb];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Orientation {
    /// Bars across, the names to the left. For many bars or long names.
    Bars,
    /// Columns standing up, the names below.
    Columns,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Labels {
    None,
    All,
    /// The largest and the smallest only.
    Ends,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    /// The bars, in the order they are drawn: a name and a value.
    pub bars: Vec<(String, f64)>,
    /// Bars whose names are here get the accent colour.
    pub highlight: Vec<String>,
    pub orientation: Orientation,
    pub title: String,
    pub subtitle: String,
    /// A line at the foot, such as where the data comes from.
    pub note: String,
    pub unit: String,
    pub decimals: usize,
    /// 1 234,5 as in Swedish, or 1,234.5.
    pub swedish: bool,
    pub labels: Labels,
    pub mean_line: bool,
    pub colour: [u8; 3],
    pub accent: [u8; 3],
    pub width: u32,
    /// None makes it fit the bars.
    pub height: Option<u32>,
    /// None leaves it see-through.
    pub background: Option<[u8; 3]>,
    /// How far the names under columns are turned, in degrees: 0, 45 or 90. Turned names
    /// get the room they need under the columns.
    pub label_angle: f32,
}

/// A number with a separator for the thousands and the decimals asked for.
pub fn number(x: f64, decimals: usize, swedish: bool) -> String {
    let s = format!("{:.*}", decimals, x.abs());
    let (int, frac) = s.split_once('.').map_or((s.as_str(), ""), |(a, b)| (a, b));
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(if swedish { '\u{a0}' } else { ',' });
        }
        grouped.push(ch);
    }
    let sign = if x < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') { "−" } else { "" };
    if frac.is_empty() {
        format!("{}{}", sign, grouped)
    } else {
        format!("{}{}{}{}", sign, grouped, if swedish { ',' } else { '.' }, frac)
    }
}

/// A step between grid lines that gives round numbers: 1, 2 or 5 times a power of ten.
fn nice_step(span: f64, lines: f64) -> f64 {
    let raw = (span / lines).max(f64::MIN_POSITIVE);
    let p = 10f64.powf(raw.log10().floor());
    let f = raw / p;
    (if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    }) * p
}

/// Writes text into a picture.
pub(crate) struct Text {
    regular: FontRef<'static>,
    bold: FontRef<'static>,
}

impl Text {
    pub(crate) fn new() -> Result<Text> {
        let f = |b: &'static [u8]| FontRef::try_from_slice(b).map_err(|_| GbError("The built-in font cannot be read.".into()));
        Ok(Text { regular: f(REGULAR)?, bold: f(SEMIBOLD)? })
    }

    fn font(&self, bold: bool) -> &FontRef<'static> {
        if bold {
            &self.bold
        } else {
            &self.regular
        }
    }

    pub(crate) fn width(&self, s: &str, size: f32, bold: bool) -> f32 {
        let font = self.font(bold).as_scaled(PxScale::from(size));
        let mut w = 0.0;
        let mut last = None;
        for ch in s.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = last {
                w += font.kern(prev, id);
            }
            w += font.h_advance(id);
            last = Some(id);
        }
        w
    }

    /// The text cut to fit a width, with … at the end when it is cut.
    pub(crate) fn fit(&self, s: &str, size: f32, bold: bool, max: f32) -> String {
        if self.width(s, size, bold) <= max {
            return s.to_string();
        }
        let mut t: String = s.to_string();
        while !t.is_empty() && self.width(&format!("{}…", t), size, bold) > max {
            t.pop();
        }
        format!("{}…", t.trim_end())
    }

    /// Draws a line of text with its baseline at y, starting at x, or ending there when
    /// `right` is set, or with its middle there when `centre` is set.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(&self, pm: &mut Pixmap, s: &str, x: f32, y: f32, size: f32, bold: bool, rgb: [u8; 3], align: f32) {
        let font = self.font(bold).as_scaled(PxScale::from(size));
        let mut pen = x - self.width(s, size, bold) * align;
        let mut last = None;
        let (w, h) = (pm.width() as i32, pm.height() as i32);
        let data = pm.data_mut();
        for ch in s.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = last {
                pen += font.kern(prev, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(pen, y));
            pen += font.h_advance(id);
            last = Some(id);
            let Some(outline) = self.font(bold).outline_glyph(glyph) else { continue };
            let b = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px < 0 || py < 0 || px >= w || py >= h || cov <= 0.0 {
                    return;
                }
                let k = ((py * w + px) * 4) as usize;
                // The picture is premultiplied: the text is laid over what is there.
                let a = cov.min(1.0);
                for (i, c) in rgb.iter().enumerate() {
                    data[k + i] = (*c as f32 * a + data[k + i] as f32 * (1.0 - a)).round() as u8;
                }
                data[k + 3] = (255.0 * a + data[k + 3] as f32 * (1.0 - a)).round() as u8;
            });
        }
    }
}

impl Text {
    /// A line of text as the outlines of its letters, with the start of its baseline at 0, 0
    /// and y downwards, as a picture has it: to be filled, given an edge, turned and moved.
    pub(crate) fn path(&self, s: &str, size: f32, bold: bool) -> Option<tiny_skia::Path> {
        use ab_glyph::OutlineCurve;
        let font = self.font(bold);
        let scaled = font.as_scaled(PxScale::from(size));
        let (kx, ky) = (scaled.h_scale_factor(), scaled.v_scale_factor());
        let mut pb = PathBuilder::new();
        let mut pen = 0.0f32;
        let mut last = None;
        for ch in s.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(prev) = last {
                pen += scaled.kern(prev, id);
            }
            if let Some(outline) = font.outline(id) {
                let at = |p: ab_glyph::Point| (pen + p.x * kx, -p.y * ky);
                let mut current: Option<(f32, f32)> = None;
                for c in &outline.curves {
                    let (start, end) = match c {
                        OutlineCurve::Line(a, b) => (*a, *b),
                        OutlineCurve::Quad(a, _, b) => (*a, *b),
                        OutlineCurve::Cubic(a, _, _, b) => (*a, *b),
                    };
                    let s0 = at(start);
                    if current.is_none_or(|c| (c.0 - s0.0).abs() > 1e-3 || (c.1 - s0.1).abs() > 1e-3) {
                        if current.is_some() {
                            pb.close();
                        }
                        pb.move_to(s0.0, s0.1);
                    }
                    match c {
                        OutlineCurve::Line(_, b) => {
                            let q = at(*b);
                            pb.line_to(q.0, q.1);
                        }
                        OutlineCurve::Quad(_, c1, b) => {
                            let (q1, q) = (at(*c1), at(*b));
                            pb.quad_to(q1.0, q1.1, q.0, q.1);
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let (q1, q2, q) = (at(*c1), at(*c2), at(*b));
                            pb.cubic_to(q1.0, q1.1, q2.0, q2.1, q.0, q.1);
                        }
                    }
                    current = Some(at(end));
                }
                if current.is_some() {
                    pb.close();
                }
            }
            pen += scaled.h_advance(id);
            last = Some(id);
        }
        pb.finish()
    }

    /// Draws a line of text turned by `degrees` counterclockwise, with the end of its baseline
    /// at (x, y): for names under columns that slant up to their column.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_turned(&self, pm: &mut Pixmap, s: &str, x: f32, y: f32, size: f32, bold: bool, rgb: [u8; 3], degrees: f32) {
        if degrees.abs() < 0.01 {
            self.draw(pm, s, x, y, size, bold, rgb, 1.0);
            return;
        }
        let w = self.width(s, size, bold).ceil() + 4.0;
        let h = (size * 1.4).ceil();
        let Some(mut small) = Pixmap::new(w as u32, h as u32) else { return };
        let base = size * 1.05;
        self.draw(&mut small, s, 2.0, base, size, bold, rgb, 0.0);
        let t = Transform::from_translate(x, y).pre_rotate(-degrees).pre_translate(-(w - 2.0), -base);
        pm.draw_pixmap(0, 0, small.as_ref(), &tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() }, t, None);
    }
}

fn paint(rgb: [u8; 3]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255));
    p.anti_alias = true;
    p
}

fn rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3]) {
    // As a path: tiny-skia's own way with thin smooth rectangles trips on some sizes.
    if let Some(r) = Rect::from_xywh(x, y, w.max(1.0), h.max(1.0)) {
        let path = PathBuilder::from_rect(r);
        pm.fill_path(&path, &paint(rgb), FillRule::Winding, Transform::identity(), None);
    }
}

/// A bar with its far end rounded and its foot square, from (x, y) by (w, h). `across`
/// says the bar grows to the right; otherwise it grows upwards from its foot at y + h.
fn bar(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, across: bool, rgb: [u8; 3]) {
    let r = 4.0f32.min(if across { h / 2.0 } else { w / 2.0 }).min(if across { w } else { h }).max(0.0);
    let mut pb = PathBuilder::new();
    if across {
        pb.move_to(x, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x, y + h);
    } else {
        pb.move_to(x, y + h);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h);
    }
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &paint(rgb), FillRule::Winding, Transform::identity(), None);
    }
}

/// The room under the columns that turned names need: the longest name, at most a quarter
/// of the width, at its angle.
fn turned_room(text: &Text, c: &Chart, size: f32, width: f32) -> f32 {
    if c.orientation != Orientation::Columns || c.label_angle.abs() < 0.5 {
        return size * 2.4;
    }
    let longest = c.bars.iter().map(|b| text.width(&b.0, size, false)).fold(0.0f32, f32::max).min(width * 0.25);
    longest * c.label_angle.to_radians().sin() + size * 1.6
}

/// Draws the chart.
pub fn draw(c: &Chart) -> Result<RasterImage> {
    if c.bars.is_empty() {
        return Err(GbError("There are no bars to draw.".into()));
    }
    let text = Text::new()?;
    // Sizes are for a chart 1200 pixels wide, and follow the width.
    let k = (c.width as f32 / 1200.0).clamp(0.4, 6.0);
    let (title_size, sub_size, tick_size, label_size, note_size) = (24.0 * k, 15.0 * k, 13.0 * k, 13.5 * k, 12.0 * k);
    let pad = 28.0 * k;
    let n = c.bars.len();
    let across = c.orientation == Orientation::Bars;
    // Room at the top for the title and the explanation, and at the foot for the note.
    let mut top = pad;
    if !c.title.is_empty() {
        top += title_size * 1.3;
    }
    if !c.subtitle.is_empty() {
        top += sub_size * 1.5;
    }
    top += if c.title.is_empty() && c.subtitle.is_empty() { 0.0 } else { 14.0 * k };
    let foot = pad + if c.note.is_empty() { 0.0 } else { note_size * 2.0 };
    // A band per bar: the bar takes at most 24 pixels of it and the rest is air.
    let band = if across { (28.0 * k).max(14.0) } else { 0.0 };
    let tick_room = tick_size * 2.2;
    let height = match c.height {
        Some(h) => h as f32,
        None if across => top + tick_room + band * n as f32 + foot,
        None => (c.width as f32 * 0.6).max(320.0 * k) + turned_room(&text, c, label_size, c.width as f32) - label_size * 2.4,
    };
    let (w, h) = (c.width as f32, height);
    let mut pm = Pixmap::new(c.width, height.round() as u32).ok_or_else(|| GbError("There is not memory enough for a chart that large.".into()))?;
    if let Some(bg) = c.background {
        pm.fill(Color::from_rgba8(bg[0], bg[1], bg[2], 255));
    }

    // The scale: from 0, or from below 0 when there are negative values, to a round number.
    let lo = c.bars.iter().map(|b| b.1).fold(0.0f64, f64::min);
    let hi = c.bars.iter().map(|b| b.1).fold(0.0f64, f64::max);
    let step = nice_step((hi - lo).max(f64::MIN_POSITIVE), 5.0);
    let (s0, s1) = ((lo / step).floor() * step, ((hi / step).ceil() * step).max(step));
    let tick_text = |v: f64| {
        let d = if step < 1.0 { (-step.log10()).ceil() as usize } else { 0 };
        format!("{}{}", number(v, d, c.swedish), if c.unit.is_empty() { String::new() } else { format!(" {}", c.unit) })
    };
    let value_text = |v: f64| format!("{}{}", number(v, c.decimals, c.swedish), if c.unit.is_empty() { String::new() } else { format!(" {}", c.unit) });
    let max_i = c.bars.iter().enumerate().max_by(|a, b| a.1 .1.total_cmp(&b.1 .1)).map(|x| x.0);
    let min_i = c.bars.iter().enumerate().min_by(|a, b| a.1 .1.total_cmp(&b.1 .1)).map(|x| x.0);
    let show_ends = |i: usize| Some(i) == max_i || Some(i) == min_i;
    let show_label = |i: usize| match c.labels {
        Labels::None => false,
        Labels::All => true,
        Labels::Ends => show_ends(i),
    };

    if across {
        // Names to the left, as wide as the longest needs, but at most a third of the width.
        let name_w = c.bars.iter().map(|b| text.width(&b.0, label_size, false)).fold(0.0f32, f32::max).min(w * 0.33) + 12.0 * k;
        let value_room = if c.labels == Labels::None {
            8.0 * k
        } else {
            c.bars.iter().map(|b| text.width(&value_text(b.1), label_size, false)).fold(0.0f32, f32::max) + 10.0 * k
        };
        let (x0, x1) = (pad + name_w, w - pad - value_room);
        let plot_top = top + tick_room;
        let plot_bottom = plot_top + band * n as f32;
        let at = |v: f64| x0 + ((v - s0) / (s1 - s0)) as f32 * (x1 - x0);
        // Grid lines and their numbers above the plot.
        let mut v = s0;
        while v <= s1 + step * 1e-9 {
            let x = at(v).round() + 0.5;
            rect(&mut pm, x - 0.5, plot_top, 1.0 * k.max(1.0), plot_bottom - plot_top, if v == 0.0 { BASELINE } else { GRID });
            text.draw(&mut pm, &tick_text(v), x, plot_top - tick_size * 0.7, tick_size, false, MUTED, 0.5);
            v += step;
        }
        let thick = (band * 0.62).min(24.0 * k).max(4.0);
        for (i, (name, value)) in c.bars.iter().enumerate() {
            let cy = plot_top + band * (i as f32 + 0.5);
            let rgb = if c.highlight.iter().any(|x| x == name) { c.accent } else { c.colour };
            let (a, b) = (at(0.0f64.max(s0)), at(*value));
            if *value >= 0.0 {
                bar(&mut pm, a, cy - thick / 2.0, b - a, thick, true, rgb);
            } else {
                // A negative bar grows to the left: drawn mirrored.
                rect(&mut pm, b, cy - thick / 2.0, a - b, thick, rgb);
            }
            let shown = text.fit(name, label_size, false, name_w - 12.0 * k);
            text.draw(&mut pm, &shown, x0 - 10.0 * k, cy + label_size * 0.36, label_size, false, SECONDARY, 1.0);
            if show_label(i) {
                text.draw(&mut pm, &value_text(*value), b.max(a) + 6.0 * k, cy + label_size * 0.36, label_size, false, INK, 0.0);
            }
        }
        if c.mean_line {
            let mean = c.bars.iter().map(|b| b.1).sum::<f64>() / n as f64;
            let x = at(mean);
            rect(&mut pm, x - 0.75 * k, plot_top - 4.0 * k, 1.5 * k, plot_bottom - plot_top + 8.0 * k, INK);
            let label = format!("mean {}", value_text(mean));
            text.draw(&mut pm, &label, x, plot_bottom + label_size * 1.4, label_size, true, INK, 0.5);
        }
    } else {
        let tick_w = [s0, s1].iter().map(|v| text.width(&tick_text(*v), tick_size, false)).fold(0.0f32, f32::max) + 12.0 * k;
        let (x0, x1) = (pad + tick_w, w - pad);
        let turned = c.label_angle.abs() > 0.5;
        let name_room = if turned { turned_room(&text, c, label_size, w).min(h * 0.45) } else { label_size * 2.4 };
        let (plot_top, plot_bottom) = (top + label_size * 1.6, h - foot - name_room);
        let at = |v: f64| plot_bottom - ((v - s0) / (s1 - s0)) as f32 * (plot_bottom - plot_top);
        let mut v = s0;
        while v <= s1 + step * 1e-9 {
            let y = at(v).round() + 0.5;
            rect(&mut pm, x0, y - 0.5, x1 - x0, 1.0 * k.max(1.0), if v == 0.0 { BASELINE } else { GRID });
            text.draw(&mut pm, &tick_text(v), x0 - 8.0 * k, y + tick_size * 0.35, tick_size, false, MUTED, 1.0);
            v += step;
        }
        let slot = (x1 - x0) / n as f32;
        let thick = (slot * 0.62).min(24.0 * k * 2.0).max(2.0);
        // Numbers over every column only when they fit side by side; else the ends only.
        let crowded = c.bars.iter().any(|b| text.width(&value_text(b.1), label_size, false) > slot * 0.95);
        let show_label = |i: usize| if crowded && c.labels == Labels::All { show_ends(i) } else { show_label(i) };
        for (i, (name, value)) in c.bars.iter().enumerate() {
            let cx = x0 + slot * (i as f32 + 0.5);
            let rgb = if c.highlight.iter().any(|x| x == name) { c.accent } else { c.colour };
            let (a, b) = (at(0.0f64.max(s0)), at(*value));
            if *value >= 0.0 {
                bar(&mut pm, cx - thick / 2.0, b, thick, a - b, false, rgb);
            } else {
                rect(&mut pm, cx - thick / 2.0, a, thick, b - a, rgb);
            }
            if turned {
                // The end of the name at the column, the rest slanting down to the left.
                let a = c.label_angle.to_radians();
                let longest = (name_room - label_size * 1.2) / a.sin().max(0.2);
                let shown = text.fit(name, label_size, false, longest.max(20.0));
                let (dx, dy) = (label_size * 0.3 * a.sin(), label_size * 0.9);
                text.draw_turned(&mut pm, &shown, cx + dx, plot_bottom + dy, label_size, false, SECONDARY, c.label_angle);
            } else {
                let shown = text.fit(name, label_size, false, slot * 0.96);
                text.draw(&mut pm, &shown, cx, plot_bottom + label_size * 1.5, label_size, false, SECONDARY, 0.5);
            }
            if show_label(i) {
                text.draw(&mut pm, &value_text(*value), cx, b.min(a) - 6.0 * k, label_size, false, INK, 0.5);
            }
        }
        if c.mean_line {
            let mean = c.bars.iter().map(|b| b.1).sum::<f64>() / n as f64;
            let y = at(mean);
            rect(&mut pm, x0 - 4.0 * k, y - 0.75 * k, x1 - x0 + 8.0 * k, 1.5 * k, INK);
            let label = format!("mean {}", value_text(mean));
            text.draw(&mut pm, &label, x1, y - 6.0 * k, label_size, true, INK, 1.0);
        }
    }

    // The title and the explanation at the top, the note at the foot.
    let mut y = pad + title_size * 0.95;
    if !c.title.is_empty() {
        let t = text.fit(&c.title, title_size, true, w - 2.0 * pad);
        text.draw(&mut pm, &t, pad, y, title_size, true, INK, 0.0);
        y += sub_size * 1.6;
    } else {
        y = pad + sub_size;
    }
    if !c.subtitle.is_empty() {
        let t = text.fit(&c.subtitle, sub_size, false, w - 2.0 * pad);
        text.draw(&mut pm, &t, pad, y, sub_size, false, SECONDARY, 0.0);
    }
    if !c.note.is_empty() {
        let t = text.fit(&c.note, note_size, false, w - 2.0 * pad);
        text.draw(&mut pm, &t, pad, h - pad * 0.7, note_size, false, MUTED, 0.0);
    }

    let mut rgba = Vec::with_capacity((pm.width() * pm.height() * 4) as usize);
    for p in pm.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(RasterImage { width: pm.width() as usize, height: pm.height() as usize, rgba, geotransform: [0.0, 1.0, 0.0, pm.height() as f64, 0.0, -1.0] })
}

/// The colour of the surface under a chart that has one.
pub const PAPER: [u8; 3] = SURFACE;

/// The chart as a web page of its own, with nothing to fetch: the bars drawn in SVG by a
/// little script, the value of a bar shown when the pointer is on it, and buttons to sort
/// by size or by name. Light or dark as the reader's computer is set.
pub fn html(c: &Chart) -> String {
    let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let data = serde_json::json!({
        "bars": c.bars.iter().map(|(n, v)| serde_json::json!([n, v])).collect::<Vec<_>>(),
        "highlight": c.highlight,
        "columns": c.orientation == Orientation::Columns,
        "unit": c.unit,
        "decimals": c.decimals,
        "swedish": c.swedish,
        "labels": match c.labels { Labels::None => "none", Labels::All => "all", Labels::Ends => "ends" },
        "mean": c.mean_line,
        "colour": hex(c.colour),
        "accent": hex(c.accent),
        "angle": c.label_angle,
    });
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let title = if c.title.is_empty() { "Chart".to_string() } else { esc(&c.title) };
    let bg = c.background.map_or("transparent".to_string(), hex);
    format!(
        r##"<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ --bg: {bg}; --ink: #0b0b0b; --muted: #52514e; --grid: #e1e0d9; --base: #c3c2b7; --tip: #ffffff; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #1b1b1a; --ink: #f2f1ec; --muted: #b0aea6; --grid: #3a3936; --base: #5a5954; --tip: #2a2a28; }} }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 15px/1.4 Inter, "Segoe UI", system-ui, sans-serif; }}
main {{ max-width: 1100px; margin: 0 auto; padding: 24px 16px; }}
h1 {{ font-size: 1.5em; margin: 0 0 4px; font-weight: 600; }}
p.sub {{ margin: 0 0 12px; color: var(--muted); }}
p.note {{ margin: 12px 0 0; color: var(--muted); font-size: 0.85em; }}
.tools {{ margin: 8px 0 12px; display: flex; gap: 6px; flex-wrap: wrap; }}
.tools button {{ font: inherit; font-size: 0.85em; padding: 4px 10px; border-radius: 6px; border: 1px solid var(--base); background: transparent; color: var(--ink); cursor: pointer; }}
.tools button.on {{ background: var(--ink); color: var(--bg); }}
svg {{ width: 100%; height: auto; display: block; overflow: visible; }}
svg text {{ fill: var(--muted); font-size: 12px; }}
svg .val {{ fill: var(--ink); font-weight: 600; }}
svg rect.bar {{ transition: opacity .15s; }}
svg rect.bar:hover {{ opacity: .75; }}
#tip {{ position: fixed; pointer-events: none; background: var(--tip); color: var(--ink); border: 1px solid var(--base); border-radius: 6px; padding: 6px 9px; font-size: 13px; box-shadow: 0 2px 8px rgba(0,0,0,.15); display: none; }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
{sub}
<div class="tools" id="tools"></div>
<div id="chart"></div>
{note}
</main>
<div id="tip"></div>
<script>
const D = {data};
const sw = D.swedish;
const T = {{ size: "Largest first", small: "Smallest first", name: "Alphabetical", given: "As drawn", mean: "mean" }};
function fmt(x) {{
  const s = Math.abs(x).toFixed(D.decimals);
  let [i, f] = s.split(".");
  i = i.replace(/\B(?=(\d{{3}})+(?!\d))/g, sw ? " " : ",");
  return (x < 0 ? "−" : "") + i + (f ? (sw ? "," : ".") + f : "") + (D.unit ? " " + D.unit : "");
}}
const given = D.bars.map((b, k) => [b[0], b[1], k]);
const orders = {{
  given: () => given.slice(),
  size: () => given.slice().sort((a, b) => b[1] - a[1]),
  small: () => given.slice().sort((a, b) => a[1] - b[1]),
  name: () => given.slice().sort((a, b) => a[0].localeCompare(b[0], sw ? "sv" : "en")),
}};
let order = "given";
const NS = "http://www.w3.org/2000/svg";
function el(name, attrs, parent, text) {{
  const e = document.createElementNS(NS, name);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  if (text !== undefined) e.textContent = text;
  if (parent) parent.appendChild(e);
  return e;
}}
const tip = document.getElementById("tip");
function hover(e, b) {{
  tip.style.display = "block";
  tip.innerHTML = "<b></b><br>";
  tip.firstChild.textContent = b[0];
  tip.appendChild(document.createTextNode(fmt(b[1])));
  tip.style.left = (e.clientX + 14) + "px";
  tip.style.top = (e.clientY + 14) + "px";
}}
function draw() {{
  const bars = orders[order]();
  const host = document.getElementById("chart");
  host.innerHTML = "";
  const max = Math.max(0, ...bars.map(b => b[1])), min = Math.min(0, ...bars.map(b => b[1]));
  const span = (max - min) || 1;
  const mean = bars.reduce((s, b) => s + b[1], 0) / Math.max(1, bars.length);
  const hi = Math.max(...bars.map(b => b[1])), lo = Math.min(...bars.map(b => b[1]));
  const show = b => D.labels === "all" || (D.labels === "ends" && (b[1] === hi || b[1] === lo));
  const colour = b => D.highlight.includes(b[0]) ? D.accent : D.colour;
  // Round steps for the grid.
  const raw = span / 5, p = Math.pow(10, Math.floor(Math.log10(raw))), f = raw / p;
  const step = (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * p;
  const ticks = [];
  for (let v = Math.ceil(min / step) * step; v <= max + 1e-9; v += step) ticks.push(v);
  if (!D.columns) {{
    const nameW = Math.min(320, 12 + 7.2 * Math.max(...bars.map(b => b[0].length)));
    const W = 900, row = 26, top = 24, H = top + bars.length * row + 8;
    const svg = el("svg", {{ viewBox: `0 0 ${{W}} ${{H}}`, role: "img" }}, host);
    const x = v => nameW + (v - min) / span * (W - nameW - 90);
    for (const v of ticks) {{
      el("line", {{ x1: x(v), x2: x(v), y1: top - 6, y2: H - 6, stroke: "var(--grid)" }}, svg);
      el("text", {{ x: x(v), y: 12, "text-anchor": "middle" }}, svg, fmt(v));
    }}
    bars.forEach((b, i) => {{
      const y = top + i * row;
      el("text", {{ x: nameW - 8, y: y + 15, "text-anchor": "end" }}, svg, b[0]);
      const r = el("rect", {{ class: "bar", x: Math.min(x(0), x(b[1])), y: y + 4, width: Math.max(1, Math.abs(x(b[1]) - x(0))), height: row - 10, rx: 3, fill: colour(b) }}, svg);
      r.addEventListener("mousemove", e => hover(e, b));
      r.addEventListener("mouseleave", () => tip.style.display = "none");
      if (show(b)) el("text", {{ class: "val", x: x(Math.max(0, b[1])) + 6, y: y + 15 }}, svg, fmt(b[1]));
    }});
    el("line", {{ x1: x(0), x2: x(0), y1: top - 6, y2: H - 6, stroke: "var(--base)" }}, svg);
    if (D.mean) {{
      el("line", {{ x1: x(mean), x2: x(mean), y1: top - 6, y2: H - 6, stroke: "var(--ink)", "stroke-dasharray": "4 3" }}, svg);
      el("text", {{ x: x(mean) + 4, y: H + 8, class: "val" }}, svg, T.mean + " " + fmt(mean));
    }}
  }} else {{
    const W = 900, H = 420, left = 70, bottom = D.angle > 0 ? 110 : 40, top = 24;
    const svg = el("svg", {{ viewBox: `0 0 ${{W}} ${{H + 20}}`, role: "img" }}, host);
    const y = v => top + (max - v) / span * (H - top - bottom);
    const colW = (W - left - 10) / bars.length;
    for (const v of ticks) {{
      el("line", {{ x1: left, x2: W - 10, y1: y(v), y2: y(v), stroke: "var(--grid)" }}, svg);
      el("text", {{ x: left - 8, y: y(v) + 4, "text-anchor": "end" }}, svg, fmt(v));
    }}
    bars.forEach((b, i) => {{
      const x0 = left + i * colW, w = Math.max(2, colW * 0.62);
      const r = el("rect", {{ class: "bar", x: x0 + (colW - w) / 2, y: Math.min(y(0), y(b[1])), width: w, height: Math.max(1, Math.abs(y(b[1]) - y(0))), rx: 3, fill: colour(b) }}, svg);
      r.addEventListener("mousemove", e => hover(e, b));
      r.addEventListener("mouseleave", () => tip.style.display = "none");
      const cx = x0 + colW / 2, ly = H - bottom + 18;
      if (D.angle > 0) el("text", {{ x: cx, y: ly, "text-anchor": "end", transform: `rotate(-${{D.angle}} ${{cx}} ${{ly}})` }}, svg, b[0]);
      else el("text", {{ x: cx, y: ly, "text-anchor": "middle" }}, svg, b[0]);
      if (show(b)) el("text", {{ class: "val", x: cx, y: y(Math.max(0, b[1])) - 6, "text-anchor": "middle" }}, svg, fmt(b[1]));
    }});
    el("line", {{ x1: left, x2: W - 10, y1: y(0), y2: y(0), stroke: "var(--base)" }}, svg);
    if (D.mean) {{
      el("line", {{ x1: left, x2: W - 10, y1: y(mean), y2: y(mean), stroke: "var(--ink)", "stroke-dasharray": "4 3" }}, svg);
      el("text", {{ x: W - 10, y: y(mean) - 6, "text-anchor": "end", class: "val" }}, svg, T.mean + " " + fmt(mean));
    }}
  }}
}}
const tools = document.getElementById("tools");
for (const k of ["given", "size", "small", "name"]) {{
  const b = document.createElement("button");
  b.textContent = T[k];
  b.onclick = () => {{ order = k; for (const o of tools.children) o.classList.toggle("on", o === b); draw(); }};
  if (k === order) b.classList.add("on");
  tools.appendChild(b);
}}
draw();
</script>
</body>
</html>
"##,
        lang = "en",
        title = title,
        bg = bg,
        sub = if c.subtitle.is_empty() { String::new() } else { format!("<p class=\"sub\">{}</p>", esc(&c.subtitle)) },
        note = if c.note.is_empty() { String::new() } else { format!("<p class=\"note\">{}</p>", esc(&c.note)) },
        data = data.to_string().replace("</", "<\\/"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_the_swedish_way_or_the_english() {
        assert_eq!(number(1234567.891, 1, true), "1\u{a0}234\u{a0}567,9");
        assert_eq!(number(1234567.891, 2, false), "1,234,567.89");
        assert_eq!(number(-42.0, 0, true), "−42");
        assert_eq!(number(999.0, 0, true), "999");
        assert_eq!(nice_step(3400.0, 5.0), 1000.0);
        assert_eq!(nice_step(9.0, 5.0), 2.0);
    }

    #[test]
    fn a_chart_is_drawn_with_text() {
        let c = Chart {
            bars: vec![("Göteborg".into(), 447.8), ("Borås".into(), 915.2), ("Mark".into(), 935.7)],
            highlight: vec!["Borås".into()],
            orientation: Orientation::Bars,
            title: "Area per kommun".into(),
            subtitle: "Km² land".into(),
            note: "Källa: SCB".into(),
            unit: "km²".into(),
            decimals: 0,
            swedish: true,
            labels: Labels::All,
            mean_line: true,
            colour: colour("blue"),
            accent: colour("orange"),
            width: 800,
            height: None,
            background: Some(PAPER),
            label_angle: 0.0,
        };
        let img = draw(&c).unwrap();
        assert_eq!(img.width, 800);
        assert!(img.height > 150 && img.height < 400, "{}", img.height);
        // Blue bars, an orange one, and dark text somewhere.
        let px: Vec<[u8; 4]> = img.rgba.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        assert!(px.iter().any(|p| p[..3] == [0x2a, 0x78, 0xd6]));
        assert!(px.iter().any(|p| p[..3] == [0xeb, 0x68, 0x34]));
        assert!(px.iter().filter(|p| p[0] < 40 && p[1] < 40 && p[2] < 40).count() > 200);
        let cols = draw(&Chart { orientation: Orientation::Columns, ..c }).unwrap();
        assert!(cols.height > 300);
    }
}
