//! Quick add: a search box on the canvas where a tool is put by typing a few letters of its
//! name, opened by a double click on the canvas, by / , or by a connection let go on empty
//! canvas.

use crate::theme::Palette;
use eframe::egui::{self, Key, RichText, Ui};
use og_engine::nodes::{Group, NodeType, NODE_TYPES};
use std::sync::Mutex;

/// Which tools fit where the box was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Any,
    /// To be connected after an output: tools and outputs with an input.
    WithInput,
    /// To be connected before an input: inputs and tools with an output.
    WithOutput,
}

/// What came of the box in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Open,
    Picked(&'static str),
    Cancelled,
}

/// The tools used last, the latest first, so that they come first in an empty box.
static RECENT: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

pub fn used(kind: &'static str) {
    if let Ok(mut r) = RECENT.lock() {
        r.retain(|k| *k != kind);
        r.insert(0, kind);
        r.truncate(8);
    }
}

fn fits(t: &NodeType, want: Want) -> bool {
    !t.hidden
        && match want {
            Want::Any => true,
            Want::WithInput => !t.inputs.is_empty(),
            Want::WithOutput => !t.outputs.is_empty() && t.group != Group::Writer,
        }
}

/// The tools that match what is written, the best first: those whose name begins with it,
/// then those with a word that begins with it, then those that have it anywhere in the
/// name, the name in files or the category. With nothing written, the tools used last.
pub fn matches(query: &str, want: Want) -> Vec<&'static NodeType> {
    let q = query.trim().to_lowercase();
    let all = NODE_TYPES.iter().filter(|t| fits(t, want));
    if q.is_empty() {
        let recent: Vec<&'static str> = RECENT.lock().map(|r| r.clone()).unwrap_or_default();
        let mut out: Vec<&'static NodeType> = recent.iter().filter_map(|k| NODE_TYPES.iter().find(|t| t.name == *k && fits(t, want))).collect();
        let mut rest: Vec<&'static NodeType> = all.filter(|t| !recent.contains(&t.name)).collect();
        rest.sort_by_key(|t| t.title.to_lowercase());
        out.extend(rest);
        return out;
    }
    let mut scored: Vec<(u8, String, &'static NodeType)> = all
        .filter_map(|t| {
            let title = t.title.to_lowercase();
            let score = if title.starts_with(&q) {
                0
            } else if title.split([' ', '-', '(']).any(|w| w.starts_with(&q)) {
                1
            } else if title.contains(&q) {
                2
            } else if t.name.to_lowercase().contains(&q) {
                3
            } else if t.category.to_lowercase().contains(&q) {
                4
            } else {
                return None;
            };
            Some((score, title, t))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|s| s.2).collect()
}

/// The box: a field to write in, and the tools that match under it. Arrow keys move in the
/// list, Enter or a click puts the tool, Escape closes the box.
pub fn show(ui: &mut Ui, pal: &Palette, id: egui::Id, want: Want) -> Outcome {
    let (mut query, mut at): (String, usize) = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    let hint = match want {
        Want::Any => "Type to add a tool, such as buff",
        Want::WithInput => "Type the tool to connect, such as sort",
        Want::WithOutput => "Type the input or tool to connect",
    };
    let field = ui.add(egui::TextEdit::singleline(&mut query).id(id.with("field")).hint_text(hint).desired_width(260.0));
    field.request_focus();
    if field.changed() {
        at = 0;
    }
    let found = matches(&query, want);
    let (down, up, enter, escape) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    let shown = found.len().min(12);
    if down && shown > 0 {
        at = (at + 1).min(shown - 1);
    }
    if up {
        at = at.saturating_sub(1);
    }
    let mut outcome = Outcome::Open;
    if escape {
        outcome = Outcome::Cancelled;
    }
    if enter {
        outcome = match found.get(at) {
            Some(t) => Outcome::Picked(t.name),
            None => Outcome::Cancelled,
        };
    }
    for (i, t) in found.iter().take(12).enumerate() {
        let what = if t.category.is_empty() {
            match t.group {
                Group::Reader => "input",
                Group::Writer => "output",
                Group::Transformer => "tool",
            }
        } else {
            t.category
        };
        let label = egui::Button::selectable(i == at, RichText::new(t.title)).right_text(RichText::new(what).small().color(pal.muted));
        if ui.add_sized([280.0, 20.0], label).on_hover_text(t.desc).clicked() {
            outcome = Outcome::Picked(t.name);
        }
    }
    if found.is_empty() {
        ui.label(RichText::new("No tool by that name").color(pal.muted));
    } else if found.len() > 12 {
        ui.label(RichText::new(format!("and {} more — write more of the name", found.len() - 12)).small().color(pal.muted));
    }
    match outcome {
        Outcome::Open => {
            ui.data_mut(|d| d.insert_temp(id, (query, at)));
        }
        Outcome::Picked(kind) => {
            used(kind);
            ui.data_mut(|d| d.remove::<(String, usize)>(id));
        }
        Outcome::Cancelled => {
            ui.data_mut(|d| d.remove::<(String, usize)>(id));
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_few_letters_find_the_tool() {
        assert_eq!(matches("buff", Want::Any)[0].name, "Bufferer");
        assert_eq!(matches("sort", Want::Any)[0].title, "Sort");
        assert_eq!(matches("Reader", Want::Any)[0].name, "Reader", "by the name in files too");
        // After an output only what has an input; before an input no outputs.
        assert!(matches("", Want::WithInput).iter().all(|t| !t.inputs.is_empty()));
        assert!(matches("", Want::WithOutput).iter().all(|t| t.group != Group::Writer));
        used("Sorter");
        assert_eq!(matches("", Want::Any)[0].name, "Sorter", "the latest used first");
    }
}
