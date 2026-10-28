//! Suggestions while an expression is written: the attributes of the stream and the
//! functions that begin with the word at the cursor. Arrow keys choose, Tab or Enter takes
//! the suggestion, Escape closes the list.

use crate::theme::{self, Palette};
use eframe::egui::{self, text::CCursor, text::CCursorRange, Key, RichText, TextEdit, Ui};

/// A suggestion: what is put in, what is shown, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub insert: String,
    pub shown: String,
    pub kind: &'static str,
}

/// The word that ends at the cursor, as the start of it in characters and the word.
pub fn word_at(text: &str, cursor: usize) -> (usize, String) {
    let chars: Vec<char> = text.chars().collect();
    let end = cursor.min(chars.len());
    let mut start = end;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    (start, chars[start..end].iter().collect())
}

/// What could finish a word: attributes first, then functions, those that begin with it
/// before those that only have it.
pub fn candidates(word: &str, fields: &[String], quote: &dyn Fn(&str) -> String) -> Vec<Suggestion> {
    let w = word.to_lowercase();
    if w.is_empty() || w.chars().all(|c| c.is_ascii_digit()) {
        return Vec::new();
    }
    let mut out: Vec<(u8, Suggestion)> = Vec::new();
    for f in fields {
        let l = f.to_lowercase();
        if l == w {
            continue;
        }
        let rank = if l.starts_with(&w) {
            0
        } else if l.contains(&w) && w.len() >= 2 {
            2
        } else {
            continue;
        };
        out.push((rank, Suggestion { insert: quote(f), shown: f.clone(), kind: "attribute" }));
    }
    for (name, lo, _, _) in og_engine::expr::functions() {
        if name.starts_with(&w) && name != w {
            let insert = if lo == 0 { format!("{}()", name) } else { format!("{}(", name) };
            out.push((1, Suggestion { insert, shown: format!("{}(…)", name), kind: "function" }));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.shown.to_lowercase().cmp(&b.1.shown.to_lowercase())));
    out.into_iter().map(|o| o.1).take(8).collect()
}

/// The text with the word at `start..cursor` replaced by `insert`, and where the cursor goes.
pub fn accept(text: &str, start: usize, cursor: usize, insert: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars[..start.min(chars.len())].iter().collect();
    let after: String = chars[cursor.min(chars.len())..].iter().collect();
    (format!("{}{}{}", before, insert, after), start + insert.chars().count())
}

/// The field of an expression, with the list of suggestions under it while it is written.
pub fn expression_field(
    ui: &mut Ui,
    pal: &Palette,
    value: &mut String,
    fields: &[String],
    on: bool,
    id: egui::Id,
    quote: &dyn Fn(&str) -> String,
) -> egui::Response {
    let list_id = id.with("suggestions");
    // The list from the frame before: its keys are taken before the field sees them.
    let (open, mut chosen): (bool, usize) = ui.data_mut(|d| d.get_temp(list_id).unwrap_or((false, 0)));
    let mut take = false;
    let mut close = false;
    if on && open {
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                chosen += 1;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                chosen = chosen.saturating_sub(1);
            }
            take = i.consume_key(egui::Modifiers::NONE, Key::Tab) || i.consume_key(egui::Modifiers::NONE, Key::Enter);
            close = i.consume_key(egui::Modifiers::NONE, Key::Escape);
        });
    }
    // While the list is open, Tab stays in the field, to take the suggestion.
    let output = TextEdit::multiline(value).id(id).desired_width(f32::INFINITY).desired_rows(2).font(theme::mono(12.5)).lock_focus(on && open).show(ui);
    let response = output.response.response.clone();
    if !on || !response.has_focus() {
        ui.data_mut(|d| d.insert_temp(list_id, (false, 0usize)));
        return response;
    }
    let cursor = output.cursor_range.map(|r| r.primary.index.0).unwrap_or_else(|| value.chars().count());
    let (start, word) = word_at(value, cursor);
    let found = if close { Vec::new() } else { candidates(&word, fields, quote) };
    if found.is_empty() {
        ui.data_mut(|d| d.insert_temp(list_id, (false, 0usize)));
        return response;
    }
    chosen = chosen.min(found.len() - 1);
    let mut picked = take.then(|| found[chosen].clone());
    // The list, under the field.
    let at = response.rect.left_bottom() + egui::vec2(0.0, 2.0);
    egui::Area::new(list_id).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(220.0);
            for (k, s) in found.iter().enumerate() {
                let b = egui::Button::selectable(k == chosen, RichText::new(&s.shown).font(theme::mono(12.0)))
                    .right_text(RichText::new(s.kind).small().color(pal.muted));
                if ui.add(b).clicked() {
                    picked = Some(s.clone());
                }
            }
            ui.label(RichText::new("Tab or Enter takes it · Escape closes").small().color(pal.muted));
        });
    });
    match picked {
        Some(s) => {
            let (text, at) = accept(value, start, cursor, &s.insert);
            *value = text;
            if let Some(mut state) = TextEdit::load_state(ui.ctx(), id) {
                state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
                state.store(ui.ctx(), id);
            }
            response.request_focus();
            ui.data_mut(|d| d.insert_temp(list_id, (false, 0usize)));
        }
        None => {
            ui.data_mut(|d| d.insert_temp(list_id, (true, chosen)));
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_at_the_cursor_is_finished() {
        let fields = vec!["population".to_string(), "pop_density".to_string(), "name".to_string(), "Tätort namn".to_string()];
        let quote = |f: &str| if f.contains(' ') { format!("\"{}\"", f) } else { f.to_string() };
        assert_eq!(word_at("round(pop", 9), (6, "pop".to_string()));
        let c = candidates("pop", &fields, &quote);
        assert_eq!(c[0].shown, "pop_density");
        assert_eq!(c[1].shown, "population");
        let c = candidates("rou", &fields, &quote);
        assert_eq!(c[0], Suggestion { insert: "round(".into(), shown: "round(…)".into(), kind: "function" });
        assert_eq!(candidates("namn", &fields, &quote)[0].insert, "\"Tätort namn\"");
        assert!(candidates("12", &fields, &quote).is_empty());
        assert_eq!(accept("round(pop / 2", 6, 9, "population"), ("round(population / 2".to_string(), 16));
    }
}
