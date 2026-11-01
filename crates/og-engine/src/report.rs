//! A report of what a model made: a title, text of one's own, and a section for each stream
//! connected: its picture, such as a map, a chart or a 3D scene, or its rows as a table.
//! Written as one web page with the pictures in it, or as a PDF of A4 pages.

use crate::model::{GbError, RasterImage, Result};
use std::fmt::Write as _;
use std::path::Path;

/// A piece of the report, in the order it is read.
pub enum Block<'a> {
    Heading(String),
    /// Text as written: a blank line between paragraphs, "# " for a heading of its own and
    /// "- " for a point in a list.
    Text(String),
    Picture(&'a RasterImage),
    Table(TableBlock),
}

pub struct TableBlock {
    pub fields: Vec<String>,
    /// Whether a column holds numbers, set to the right.
    pub numeric: Vec<bool>,
    pub rows: Vec<Vec<String>>,
    /// All the rows of the stream, of which `rows` are the first.
    pub total: usize,
}

pub struct Report<'a> {
    pub title: String,
    /// Under the title, such as a date.
    pub subtitle: String,
    pub blocks: Vec<Block<'a>>,
}

/// The paragraphs of a text: (kind, text), where kind is 'h' for a heading, 'p' for a
/// paragraph and 'l' for a point in a list.
fn paragraphs(text: &str) -> Vec<(char, String)> {
    let mut out: Vec<(char, String)> = Vec::new();
    let mut open = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            open = false;
        } else if let Some(h) = t.strip_prefix("# ") {
            out.push(('h', h.trim().to_string()));
            open = false;
        } else if let Some(l) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            out.push(('l', l.trim().to_string()));
            open = false;
        } else if open {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(t);
            }
        } else {
            out.push(('p', t.to_string()));
            open = true;
        }
    }
    out
}

/// A picture on white, without its see-through parts.
fn on_white(img: &RasterImage) -> image::RgbImage {
    let mut out = image::RgbImage::new(img.width as u32, img.height as u32);
    for (k, px) in out.pixels_mut().enumerate() {
        let p = &img.rgba[k * 4..k * 4 + 4];
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        *px = image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])]);
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(A[(n >> 18) as usize & 63] as char);
        s.push(A[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    s
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The report as one web page, with its pictures in it.
pub fn write_html(r: &Report, path: &Path) -> Result<()> {
    let mut body = String::new();
    let _ = write!(body, "<header><h1>{}</h1>", esc(&r.title));
    if !r.subtitle.is_empty() {
        let _ = write!(body, "<p class=\"sub\">{}</p>", esc(&r.subtitle));
    }
    body.push_str("</header>\n");
    for b in &r.blocks {
        match b {
            Block::Heading(h) => {
                let _ = writeln!(body, "<h2>{}</h2>", esc(h));
            }
            Block::Text(t) => {
                let mut list = false;
                for (kind, text) in paragraphs(t) {
                    if kind != 'l' && list {
                        body.push_str("</ul>\n");
                        list = false;
                    }
                    match kind {
                        'h' => {
                            let _ = writeln!(body, "<h3>{}</h3>", esc(&text));
                        }
                        'l' => {
                            if !list {
                                body.push_str("<ul>");
                                list = true;
                            }
                            let _ = write!(body, "<li>{}</li>", esc(&text));
                        }
                        _ => {
                            let _ = writeln!(body, "<p>{}</p>", esc(&text));
                        }
                    }
                }
                if list {
                    body.push_str("</ul>\n");
                }
            }
            Block::Picture(img) => {
                let mut png = Vec::new();
                image::DynamicImage::ImageRgba8(
                    image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.rgba.clone()).ok_or_else(|| GbError("A picture is broken.".into()))?,
                )
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|e| GbError(format!("Cannot make a picture of the report: {}", e)))?;
                let _ = writeln!(body, "<figure><img alt=\"\" width=\"{}\" src=\"data:image/png;base64,{}\"></figure>", img.width, base64(&png));
            }
            Block::Table(t) => {
                body.push_str("<div class=\"table\"><table><thead><tr>");
                for (f, n) in t.fields.iter().zip(&t.numeric) {
                    let _ = write!(body, "<th{}>{}</th>", if *n { " class=\"n\"" } else { "" }, esc(f));
                }
                body.push_str("</tr></thead><tbody>\n");
                for row in &t.rows {
                    body.push_str("<tr>");
                    for (v, n) in row.iter().zip(&t.numeric) {
                        let _ = write!(body, "<td{}>{}</td>", if *n { " class=\"n\"" } else { "" }, esc(v));
                    }
                    body.push_str("</tr>\n");
                }
                body.push_str("</tbody></table></div>\n");
                let _ = writeln!(body, "<p class=\"count\">{}</p>", count_text(t));
            }
        }
    }
    let html = format!(
        r#"<!doctype html>
<html lang="sv"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="generator" content="Ographis {version}">
<style>
:root {{ --ink: #1d2327; --muted: #5f6b73; --line: #dde3e7; --band: #f4f6f8; --bg: #ffffff; }}
@media (prefers-color-scheme: dark) {{ :root {{ --ink: #e6eaed; --muted: #9aa6ad; --line: #36404a; --band: #20272d; --bg: #161b1f; }} }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 16px/1.55 Inter, "Segoe UI", system-ui, sans-serif; }}
main {{ max-width: 900px; margin: 0 auto; padding: 40px 20px 60px; }}
h1 {{ font-size: 2rem; line-height: 1.2; margin: 0 0 4px; }}
.sub {{ color: var(--muted); margin: 0 0 32px; }}
h2 {{ font-size: 1.35rem; margin: 40px 0 12px; padding-top: 16px; border-top: 1px solid var(--line); }}
h3 {{ font-size: 1.1rem; margin: 24px 0 8px; }}
figure {{ margin: 16px 0; }}
img {{ max-width: 100%; height: auto; display: block; border-radius: 4px; }}
.table {{ overflow-x: auto; border: 1px solid var(--line); border-radius: 4px; }}
table {{ border-collapse: collapse; width: 100%; font-size: 0.85rem; font-variant-numeric: tabular-nums; }}
th, td {{ padding: 5px 10px; text-align: left; white-space: nowrap; border-bottom: 1px solid var(--line); }}
th {{ background: var(--band); position: sticky; top: 0; }}
tbody tr:nth-child(even) {{ background: var(--band); }}
.n {{ text-align: right; }}
.count {{ color: var(--muted); font-size: 0.85rem; margin-top: 6px; }}
@media print {{ body {{ background: #fff; color: #000; }} h2 {{ break-before: auto; }} figure, tr {{ break-inside: avoid; }} th {{ position: static; }} }}
</style></head>
<body><main>
{body}</main></body></html>
"#,
        title = esc(&r.title),
        version = env!("CARGO_PKG_VERSION"),
        body = body
    );
    std::fs::write(path, html).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))
}

fn count_text(t: &TableBlock) -> String {
    let all = crate::model::fmt_count(t.total as u64);
    if t.total > t.rows.len() {
        format!("The first {} of {} rows", crate::model::fmt_count(t.rows.len() as u64), all)
    } else if t.total == 1 {
        "1 row".to_string()
    } else {
        format!("{} rows", all)
    }
}

// ---------- PDF ----------

/// The widths of the letters of Helvetica, in thousandths of the size, for ASCII from the
/// space on.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584,
    556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278,
    469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260,
    334, 584,
];
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584,
    611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333,
    584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280,
    389, 584,
];

/// A letter as Windows-1252, which the fonts of PDF are set in here; one that it lacks is a
/// question mark.
fn cp1252(c: char) -> u8 {
    match c as u32 {
        0x20..=0x7e | 0xa0..=0xff => c as u32 as u8,
        _ => match c {
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '‰' => 0x89,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            '\t' => b' ',
            _ => b'?',
        },
    }
}

/// The width of a letter in thousandths of the size: of its letter without accents for the
/// letters of Latin-1.
fn width_of(c: char, bold: bool) -> u16 {
    let table = if bold { &HELVETICA_BOLD } else { &HELVETICA };
    let base = match c {
        'À'..='Å' => 'A',
        'Æ' => return 1000,
        'Ç' => 'C',
        'È'..='Ë' => 'E',
        'Ì'..='Ï' => 'I',
        'Ñ' => 'N',
        'Ò'..='Ö' | 'Ø' => 'O',
        'Ù'..='Ü' => 'U',
        'Ý' => 'Y',
        'ß' => return 611,
        'à'..='å' => 'a',
        'æ' => return 889,
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '–' => return 556,
        '—' | '…' | '‰' => return 1000,
        '€' => return 556,
        _ => c,
    };
    let k = base as u32;
    if (0x20..0x7f).contains(&k) {
        table[(k - 0x20) as usize]
    } else {
        556
    }
}

fn text_width(s: &str, size: f64, bold: bool) -> f64 {
    s.chars().map(|c| width_of(c, bold) as f64).sum::<f64>() * size / 1000.0
}

/// A text as a string of PDF.
fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        let b = cp1252(c);
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{:03o}", b);
            }
        }
    }
    out.push(')');
    out
}

/// The text cut to a width, with … at the end when it was cut.
fn fit(s: &str, width: f64, size: f64, bold: bool) -> String {
    if text_width(s, size, bold) <= width + 0.01 {
        return s.to_string();
    }
    let room = width - text_width("…", size, bold);
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        let cw = width_of(c, bold) as f64 * size / 1000.0;
        if w + cw > room {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.push('…');
    out
}

/// The lines of a paragraph broken to a width.
fn wrap(s: &str, width: f64, size: f64, bold: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{} {}", line, word) };
        if text_width(&candidate, size, bold) <= width || line.is_empty() {
            line = candidate;
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

const PAGE_W: f64 = 595.28;
const PAGE_H: f64 = 841.89;
const MARGIN: f64 = 50.0;
const INK: &str = "0.114 0.137 0.153";
const MUTED: &str = "0.37 0.42 0.45";

/// Pages as they are laid out: the content of each, and the pictures it shows.
struct Layout {
    pages: Vec<(String, Vec<usize>)>,
    y: f64,
}

impl Layout {
    fn new() -> Layout {
        Layout { pages: vec![(String::new(), Vec::new())], y: PAGE_H - MARGIN }
    }
    fn page(&mut self) -> &mut String {
        &mut self.pages.last_mut().expect("a page").0
    }
    /// Room for this much more on the page, or a new page.
    fn room(&mut self, height: f64) {
        if self.y - height < MARGIN + 20.0 && self.y < PAGE_H - MARGIN - 1.0 {
            self.pages.push((String::new(), Vec::new()));
            self.y = PAGE_H - MARGIN;
        }
    }
    fn text(&mut self, x: f64, s: &str, size: f64, bold: bool, colour: &str) {
        let y = self.y;
        let _ = writeln!(self.page(), "BT {} rg /{} {} Tf {:.2} {:.2} Td {} Tj ET", colour, if bold { "F2" } else { "F1" }, size, x, y, pdf_string(s));
    }
    fn lines(&mut self, s: &str, size: f64, bold: bool, indent: f64, colour: &str) {
        let width = PAGE_W - 2.0 * MARGIN - indent;
        for line in wrap(s, width, size, bold) {
            self.room(size * 1.45);
            self.y -= size * 1.2;
            self.text(MARGIN + indent, &line, size, bold, colour);
            self.y -= size * 0.25;
        }
    }
}

/// The report as a PDF of A4 pages, with page numbers.
pub fn write_pdf(r: &Report, path: &Path) -> Result<()> {
    let mut l = Layout::new();
    let mut pictures: Vec<(Vec<u8>, u32, u32)> = Vec::new();
    let width = PAGE_W - 2.0 * MARGIN;
    l.lines(&r.title, 22.0, true, 0.0, INK);
    if !r.subtitle.is_empty() {
        l.y -= 2.0;
        l.lines(&r.subtitle, 11.0, false, 0.0, MUTED);
    }
    l.y -= 14.0;
    for b in &r.blocks {
        match b {
            Block::Heading(h) => {
                l.room(16.0 * 1.5 + 60.0);
                l.y -= 14.0;
                l.lines(h, 15.0, true, 0.0, INK);
                l.y -= 4.0;
            }
            Block::Text(t) => {
                for (kind, text) in paragraphs(t) {
                    match kind {
                        'h' => {
                            l.room(40.0);
                            l.y -= 6.0;
                            l.lines(&text, 12.5, true, 0.0, INK);
                        }
                        'l' => {
                            l.room(14.0);
                            let top = l.y;
                            l.lines(&text, 10.5, false, 14.0, INK);
                            let keep = l.y;
                            l.y = top - 10.5 * 1.2;
                            l.text(MARGIN + 3.0, "•", 10.5, false, INK);
                            l.y = keep;
                        }
                        _ => {
                            l.lines(&text, 10.5, false, 0.0, INK);
                            l.y -= 6.0;
                        }
                    }
                }
            }
            Block::Picture(img) => {
                let (w, h) = (img.width as f64, img.height as f64);
                let most_h = PAGE_H - 2.0 * MARGIN - 40.0;
                let scale = (width / w).min(most_h / h);
                let (dw, dh) = (w * scale, h * scale);
                l.room(dh + 10.0);
                l.y -= dh;
                let mut jpg = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
                    .encode_image(&on_white(img))
                    .map_err(|e| GbError(format!("Cannot make a picture of the report: {}", e)))?;
                pictures.push((jpg, img.width as u32, img.height as u32));
                let k = pictures.len() - 1;
                let y = l.y;
                let _ = writeln!(l.page(), "q {:.2} 0 0 {:.2} {:.2} {:.2} cm /Im{} Do Q", dw, dh, MARGIN + (width - dw) / 2.0, y, k);
                l.pages.last_mut().expect("a page").1.push(k);
                l.y -= 10.0;
            }
            Block::Table(t) => {
                let size = 8.0;
                let pad = 4.0;
                // Each column as wide as it needs, up to a limit, and narrower to fit.
                let mut widths: Vec<f64> = t
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let cells = t.rows.iter().map(|r| text_width(&r[i], size, false)).fold(0.0, f64::max);
                        cells.max(text_width(f, size, true)).min(150.0) + 2.0 * pad
                    })
                    .collect();
                let mut shown = widths.len();
                while shown > 1 && widths[..shown].iter().map(|w| w.min(45.0)).sum::<f64>() > width {
                    shown -= 1;
                }
                widths.truncate(shown);
                let total: f64 = widths.iter().sum();
                if total > width {
                    // The wide ones give up room first.
                    let mut over = total - width;
                    while over > 0.5 {
                        let widest = widths.iter().cloned().fold(0.0, f64::max);
                        let next = widths.iter().cloned().filter(|w| *w < widest - 0.5).fold(45.0, f64::max);
                        let n = widths.iter().filter(|w| **w >= widest - 0.5).count() as f64;
                        let cut = ((widest - next) * n).min(over);
                        for w in widths.iter_mut().filter(|w| **w >= widest - 0.5) {
                            *w -= cut / n;
                        }
                        over -= cut;
                    }
                }
                let row_h = size * 1.9;
                let header = |l: &mut Layout| {
                    let y = l.y - row_h;
                    let w: f64 = widths.iter().sum();
                    let _ = writeln!(l.page(), "0.93 0.945 0.955 rg {:.2} {:.2} {:.2} {:.2} re f", MARGIN, y, w, row_h);
                    let mut x = MARGIN;
                    l.y = y + size * 0.6;
                    for (i, cw) in widths.iter().enumerate() {
                        let s = fit(&t.fields[i], cw - 2.0 * pad, size, true);
                        let dx = if t.numeric[i] { cw - pad - text_width(&s, size, true) } else { pad };
                        l.text(x + dx, &s, size, true, INK);
                        x += cw;
                    }
                    l.y = y;
                };
                l.room(row_h * 3.0);
                header(&mut l);
                for (k, row) in t.rows.iter().enumerate() {
                    if l.y - row_h < MARGIN + 20.0 {
                        l.room(PAGE_H);
                        header(&mut l);
                    }
                    let y = l.y - row_h;
                    if k % 2 == 1 {
                        let w: f64 = widths.iter().sum();
                        let _ = writeln!(l.page(), "0.97 0.975 0.98 rg {:.2} {:.2} {:.2} {:.2} re f", MARGIN, y, w, row_h);
                    }
                    let mut x = MARGIN;
                    l.y = y + size * 0.6;
                    for (i, cw) in widths.iter().enumerate() {
                        let s = fit(&row[i], cw - 2.0 * pad, size, false);
                        let dx = if t.numeric[i] { cw - pad - text_width(&s, size, false) } else { pad };
                        l.text(x + dx, &s, size, false, INK);
                        x += cw;
                    }
                    l.y = y;
                }
                l.y -= 4.0;
                let mut note = count_text(t);
                if shown < t.fields.len() {
                    let _ = write!(note, "; {} more columns are in the web page or the data", t.fields.len() - shown);
                }
                l.lines(&note, 8.5, false, 0.0, MUTED);
                l.y -= 8.0;
            }
        }
    }
    // The objects: catalogue, pages, fonts, pictures, then each page and its content.
    let n_pages = l.pages.len();
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1 catalogue, 2 pages, 3 and 4 fonts, then the pictures, then a page and its content each.
    let first_picture = 5;
    let first_page = first_picture + pictures.len();
    let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", first_page + 2 * i)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), n_pages).into_bytes());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec());
    for (jpg, w, h) in &pictures {
        let mut o = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            w,
            h,
            jpg.len()
        )
        .into_bytes();
        o.extend_from_slice(jpg);
        o.extend_from_slice(b"\nendstream");
        objects.push(o);
    }
    for (i, (content, pics)) in l.pages.iter().enumerate() {
        let mut content = content.clone();
        // The page number at the foot of the page.
        let foot = format!("{} / {}", i + 1, n_pages);
        let _ = writeln!(
            content,
            "BT {} rg /F1 8 Tf {:.2} {:.2} Td {} Tj ET",
            MUTED,
            PAGE_W - MARGIN - text_width(&foot, 8.0, false),
            MARGIN - 20.0,
            pdf_string(&foot)
        );
        let xobjects: Vec<String> = pics.iter().map(|k| format!("/Im{} {} 0 R", k, first_picture + k)).collect();
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /XObject << {} >> >> /Contents {} 0 R >>",
                PAGE_W,
                PAGE_H,
                xobjects.join(" "),
                first_page + 2 * i + 1
            )
            .into_bytes(),
        );
        let mut o = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        o.extend_from_slice(content.as_bytes());
        o.extend_from_slice(b"\nendstream");
        objects.push(o);
    }
    let mut pdf: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(o);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for o in offsets {
        let _ = writeln!(tail, "{:010} 00000 n ", o);
    }
    let _ = write!(
        tail,
        "trailer\n<< /Size {} /Root 1 0 R /Info << /Title {} /Producer (Ographis {}) >> >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        pdf_string(&r.title),
        env!("CARGO_PKG_VERSION"),
        xref
    );
    pdf.extend_from_slice(tail.as_bytes());
    std::fs::write(path, pdf).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_becomes_headings_paragraphs_and_points() {
        let p = paragraphs("# Om\nEn rad\nsom fortsätter.\n\n- ett\n- två\nSlut");
        assert_eq!(p, [('h', "Om".into()), ('p', "En rad som fortsätter.".into()), ('l', "ett".into()), ('l', "två".into()), ('p', "Slut".into())]);
    }

    #[test]
    fn letters_are_set_in_windows_1252() {
        assert_eq!(pdf_string("Åäö (1)"), "(\\305\\344\\366 \\(1\\))");
        assert_eq!(base64(b"Ographis"), "T2dyYXBoaXM=");
        assert_eq!(fit("Göteborgs stad", 30.0, 8.0, false).chars().last(), Some('…'));
        assert_eq!(wrap("a b c", 15.0, 10.0, false).len(), 2);
    }
}
