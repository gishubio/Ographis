//! Web addresses in comments and notes: drawn as links, and opened in the browser when
//! clicked.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Galley, Pos2, TextFormat};
use std::ops::Range;
use std::sync::Arc;

/// The web addresses in a text, as byte ranges: http://…, https://… and www.…, up to the
/// next space, without punctuation that ends a sentence.
pub fn find_urls(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let starts = rest.starts_with("https://") || rest.starts_with("http://") || rest.starts_with("www.");
        // Only at the start of a word, so that a word that merely contains "www." is left alone.
        let at_word = i == 0 || text[..i].chars().next_back().is_some_and(|c| c.is_whitespace() || "([<\"'".contains(c));
        if starts && at_word {
            let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let mut end = i + len;
            while end > i && text[..end].ends_with(['.', ',', ';', ':', ')', ']', '>', '"', '\'', '!', '?']) {
                end -= 1;
            }
            if end > i + 4 {
                out.push(i..end);
            }
            i += len.max(1);
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// Where the address of the link opened last is kept, in the memory of egui.
pub const LAST_OPENED: &str = "gb-last-opened-link";

/// The address to open for a link: www.… gets https:// in front.
pub fn url_of(text: &str) -> String {
    if text.starts_with("www.") {
        format!("https://{}", text)
    } else {
        text.to_string()
    }
}

/// The text laid out with its links underlined in the colour of links.
pub fn layout(text: &str, font: FontId, color: Color32, link: Color32, width: f32) -> (LayoutJob, Vec<Range<usize>>) {
    let urls = find_urls(text);
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let plain = TextFormat { font_id: font.clone(), color, ..Default::default() };
    let linked = TextFormat { font_id: font, color: link, underline: egui::Stroke::new(1.0, link), ..Default::default() };
    let mut at = 0;
    for r in &urls {
        if r.start > at {
            job.append(&text[at..r.start], 0.0, plain.clone());
        }
        job.append(&text[r.clone()], 0.0, linked.clone());
        at = r.end;
    }
    if at < text.len() {
        job.append(&text[at..], 0.0, plain);
    }
    (job, urls)
}

/// The link under a point of a laid out text, given where the text starts.
pub fn link_at(galley: &Arc<Galley>, origin: Pos2, pos: Pos2, text: &str, urls: &[Range<usize>]) -> Option<String> {
    if urls.is_empty() || !galley.rect.translate(origin.to_vec2()).contains(pos) {
        return None;
    }
    let cursor = galley.cursor_from_pos(pos - origin);
    // The cursor counts characters; the ranges are in bytes.
    let byte = text.char_indices().nth(cursor.index.into()).map_or(text.len(), |(b, _)| b);
    urls.iter().find(|r| r.start <= byte && byte < r.end).map(|r| url_of(&text[r.clone()]))
}

/// Draws a text with links at a place, and opens a link that is clicked. Returns the height.
#[allow(clippy::too_many_arguments)]
pub fn paint(ui: &egui::Ui, text: &str, origin: Pos2, width: f32, font: FontId, color: Color32, link: Color32, clip: egui::Rect) -> f32 {
    let (job, urls) = layout(text, font, color, link, width);
    let galley = ui.painter().layout_job(job);
    let height = galley.rect.height();
    ui.painter().with_clip_rect(clip).galley(origin, galley.clone(), color);
    if let Some(pos) = ui.ctx().pointer_hover_pos().filter(|p| clip.contains(*p)) {
        if let Some(url) = link_at(&galley, origin, pos, text, &urls) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if ui.ctx().input(|i| i.pointer.primary_clicked()) {
                // Kept too, so that the tests can see what was opened.
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(LAST_OPENED), url.clone()));
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
    }
    height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_found_without_the_full_stop_after_them() {
        let t = "See https://vgregion.se/karta. And (www.gishub.io), not wwwx or awww.b.";
        let found: Vec<&str> = find_urls(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(found, ["https://vgregion.se/karta", "www.gishub.io"]);
        assert_eq!(url_of("www.gishub.io"), "https://www.gishub.io");
        assert!(find_urls("Åmål http://a.se/ö?x=1").len() == 1);
    }
}
