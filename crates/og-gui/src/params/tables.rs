//! The rows of the forms that are tables: the fields of an output, sorting, ranges, checks,
//! renames and the list of attributes with their actions.

use super::*;

/// The type and size of single attributes, as a row for each attribute that comes in.
/// Only the rows that say something are stored, as a JSON object by attribute name.
pub(super) fn schema(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[(String, &'static str)], pal: &Palette) -> bool {
    const TYPES: [(&str, &str); 4] = [("", "As it is"), ("text", "Text"), ("integer", "Integer"), ("decimal", "Decimal")];
    let stored: serde_json::Map<String, Json> = match node.params.get(spec.key) {
        Some(Json::Object(o)) => o.clone(),
        _ => Default::default(),
    };
    let text = |name: &str, key: &str| match stored.get(name).and_then(|r| r.get(key)) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    // The attributes of the stream, and after them any that have settings but are gone.
    let mut names: Vec<(String, &str)> = fields.to_vec();
    for name in stored.keys() {
        if !names.iter().any(|f| &f.0 == name) {
            names.push((name.clone(), "not in the stream"));
        }
    }
    if names.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes are listed here.").small().color(pal.muted));
        return false;
    }
    let mut after = serde_json::Map::new();
    egui::Grid::new(("schema", spec.key)).num_columns(3).spacing(egui::vec2(6.0, 3.0)).show(ui, |ui| {
        for heading in ["Attribute", "Write as", "Length / decimals"] {
            ui.label(RichText::new(heading).small().color(pal.muted));
        }
        ui.end_row();
        for (name, has) in &names {
            let (mut ty, mut length, mut decimals) = (text(name, "type"), text(name, "length"), text(name, "decimals"));
            ui.add(egui::Label::new(RichText::new(name).font(theme::mono(11.5))).truncate()).on_hover_text(format!("{} ({})", name, has));
            let shown = match TYPES.iter().find(|t| t.0 == ty) {
                Some(("", _)) | None => format!("As it is ({})", has),
                Some((_, label)) => label.to_string(),
            };
            egui::ComboBox::from_id_salt(("schema-type", spec.key, name)).selected_text(shown).width(106.0).show_ui(ui, |ui| {
                for (value, label) in TYPES {
                    ui.selectable_value(&mut ty, value.to_string(), label);
                }
            });
            // What the attribute will be written as decides which size makes sense.
            let becomes = if ty.is_empty() { *has } else { ty.as_str() };
            match becomes {
                "text" => {
                    ui.add(TextEdit::singleline(&mut length).desired_width(44.0).hint_text("any"))
                        .on_hover_text("The largest number of characters. Longer text is cut");
                    decimals.clear();
                }
                "decimal" => {
                    ui.add(TextEdit::singleline(&mut decimals).desired_width(44.0).hint_text("all"))
                        .on_hover_text("The number of decimals. The values are rounded");
                    length.clear();
                }
                _ => {
                    ui.label("");
                    length.clear();
                    decimals.clear();
                }
            }
            ui.end_row();
            let mut rule = serde_json::Map::new();
            if !ty.is_empty() {
                rule.insert("type".to_string(), Json::String(ty));
            }
            for (key, value) in [("length", &length), ("decimals", &decimals)] {
                if let Ok(n) = value.trim().parse::<u32>() {
                    rule.insert(key.to_string(), Json::Number(n.into()));
                }
            }
            if !rule.is_empty() {
                after.insert(name.clone(), Json::Object(rule));
            }
        }
    });
    if after == stored {
        return false;
    }
    if after.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Object(after));
    }
    true
}

/// An attribute name that can be written, or picked from those that come in.
pub(super) fn attr_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, name: &mut String, fields: &[String], width: f32) {
    ui.spacing_mut().item_spacing.x = 2.0;
    let w = if fields.is_empty() { width } else { (width - 24.0).max(40.0) };
    ui.add(TextEdit::singleline(name).id_salt(("attr-name", &id)).desired_width(w).hint_text("attribute"));
    if !fields.is_empty() {
        egui::ComboBox::from_id_salt(("attr-pick", &id)).selected_text("").width(14.0).show_ui(ui, |ui| {
            for f in fields {
                ui.selectable_value(name, f.clone(), f);
            }
        });
    }
}

/// Sort keys: rows of an attribute and an order, with arrows to change which comes first.
/// Stored as a JSON list of [attribute, order]. sort_by and order from before 0.61 become rows.
pub(super) fn sort_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    let mut rows: Vec<(String, bool)> = og_engine::nodes::sort_keys(&node.params);
    let before = rows.clone();
    let mut remove = None;
    let mut swap = None;
    let n = rows.len();
    for (i, (name, down)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 150.0).max(60.0);
            attr_picker(ui, ("sort-key", i), name, fields, w);
            egui::ComboBox::from_id_salt(("sort-dir", i)).selected_text(if *down { "Descending" } else { "Ascending" }).width(86.0).show_ui(ui, |ui| {
                ui.selectable_value(down, false, "Ascending");
                ui.selectable_value(down, true, "Descending");
            });
            if symbol(ui, "▲", i > 0, "Sort by this before the one above") {
                swap = Some(i - 1);
            }
            if symbol(ui, "▼", i + 1 < n, "Sort by this after the one below") {
                swap = Some(i);
            }
            if symbol(ui, "×", true, "Remove") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = swap {
        rows.swap(i, i + 1);
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    let free = fields.iter().find(|f| !rows.iter().any(|r| &r.0 == *f));
    if ui.add_enabled(free.is_some() || fields.is_empty(), egui::Button::new("Add attribute").small()).clicked() {
        rows.push((free.cloned().unwrap_or_default(), false));
    }
    if fields.is_empty() && rows.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes can be picked here.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    // Rows from sort_by and order, as models before 0.61 have them, are now stored as rows.
    node.params.remove("sort_by");
    node.params.remove("order");
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        let list = rows.into_iter().map(|(a, d)| Json::Array(vec![Json::String(a), Json::String(if d { "descending" } else { "ascending" }.into())])).collect();
        node.params.insert(spec.key.to_string(), Json::Array(list));
    }
    true
}

/// Classify by range: rows of from, to and the value. Stored as a JSON list of [from, to, value].
pub(super) fn range_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, pal: &Palette) -> bool {
    let text = |v: &Json| match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let mut rows: Vec<[String; 3]> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a
            .iter()
            .filter_map(|r| {
                r.as_array().map(|r| [r.first().map(text).unwrap_or_default(), r.get(1).map(text).unwrap_or_default(), r.get(2).map(text).unwrap_or_default()])
            })
            .collect(),
        _ => Vec::new(),
    };
    let before = rows.clone();
    let mut remove = None;
    if !rows.is_empty() {
        ui.horizontal(|ui| {
            for (head, w) in [("from", 64.0), ("to", 64.0), ("gives", 120.0)] {
                ui.add_sized([w, 14.0], egui::Label::new(RichText::new(head).small().color(pal.muted)));
            }
        });
    }
    for (i, row) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut row[0]).id_salt(("range-from", i)).desired_width(60.0).hint_text("…"));
            ui.add(TextEdit::singleline(&mut row[1]).id_salt(("range-to", i)).desired_width(60.0).hint_text("…"));
            ui.add(TextEdit::singleline(&mut row[2]).id_salt(("range-value", i)).desired_width((ui.available_width() - 30.0).max(60.0)).hint_text("value"));
            if symbol(ui, "×", true, "Remove this range") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.add(egui::Button::new("Add range").small()).clicked() {
        // The next range starts after the last one ends, when that is a whole number.
        let from = rows.last().and_then(|r| r[1].trim().parse::<i64>().ok()).map(|t| (t + 1).to_string()).unwrap_or_default();
        rows.push([from, String::new(), String::new()]);
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(rows.into_iter().map(|r| Json::Array(r.into_iter().map(Json::String).collect())).collect()));
    }
    true
}

/// Check attributes: a row for each attribute, with what it must be. Stored as a JSON list
/// of objects.
pub(super) fn check_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    const KINDS: [(&str, &str); 5] = [("", "Any type"), ("integer", "Whole number"), ("decimal", "Number"), ("text", "Text"), ("date", "Date")];
    let mut rows: Vec<serde_json::Map<String, Json>> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a.iter().filter_map(|r| r.as_object().cloned()).collect(),
        _ => Vec::new(),
    };
    let before = rows.clone();
    let mut remove = None;
    let text = |o: &serde_json::Map<String, Json>, k: &str| match o.get(k) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    for (i, row) in rows.iter_mut().enumerate() {
        egui::Frame::new().stroke(egui::Stroke::new(1.0, pal.line)).corner_radius(4).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut name = text(row, "attribute");
                attr_picker(ui, ("check-attr", i), &mut name, fields, (ui.available_width() - 40.0).max(80.0));
                row.insert("attribute".into(), Json::String(name));
                if symbol(ui, "×", true, "Remove this check") {
                    remove = Some(i);
                }
            });
            ui.horizontal(|ui| {
                let mut required = row.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
                if ui.checkbox(&mut required, "Must have a value").changed() {
                    row.insert("required".into(), Json::Bool(required));
                }
                let mut kind = text(row, "type");
                let shown = KINDS.iter().find(|k| k.0 == kind).map_or("Any type", |k| k.1);
                egui::ComboBox::from_id_salt(("check-type", i)).selected_text(shown).width(110.0).show_ui(ui, |ui| {
                    for (v, l) in KINDS {
                        ui.selectable_value(&mut kind, v.to_string(), l);
                    }
                });
                row.insert("type".into(), Json::String(kind));
            });
            let mut field = |ui: &mut Ui, key: &str, label: &str, hint: &str, width: f32| {
                ui.label(RichText::new(label).small().color(pal.muted));
                let mut v = text(row, key);
                if ui.add(TextEdit::singleline(&mut v).id_salt(("check", key, i)).desired_width(width).hint_text(hint)).changed() {
                    row.insert(key.into(), Json::String(v));
                }
            };
            ui.horizontal(|ui| {
                field(ui, "max_length", "Longest", "chars", 46.0);
                field(ui, "min", "Smallest", "", 56.0);
                field(ui, "max", "Largest", "", 56.0);
            });
            ui.horizontal(|ui| {
                field(ui, "values", "One of", "A, B, C", (ui.available_width() - 50.0).max(80.0));
            });
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    let free = fields.iter().find(|f| !rows.iter().any(|r| text(r, "attribute") == **f));
    if ui.add_enabled(free.is_some() || fields.is_empty(), egui::Button::new("Add a check").small()).clicked() {
        let mut row = serde_json::Map::new();
        row.insert("attribute".into(), Json::String(free.cloned().unwrap_or_default()));
        row.insert("required".into(), Json::Bool(true));
        rows.push(row);
    }
    if fields.is_empty() && rows.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes can be picked here.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    // Empty settings are left out, so that the file only says what is checked.
    let clean: Vec<Json> = rows
        .into_iter()
        .map(|mut r| {
            r.retain(|_, v| !matches!(v, Json::String(s) if s.trim().is_empty()) && *v != Json::Bool(false));
            Json::Object(r)
        })
        .collect();
    if clean.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(clean));
    }
    true
}

/// Renames as rows of old and new name. Stored as a JSON object.
pub(super) fn renames(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    let mut pairs: Vec<(String, String)> = match node.params.get(spec.key) {
        Some(Json::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect(),
        Some(Json::String(s)) => s.lines().filter_map(|l| l.split_once('=')).map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).collect(),
        _ => Vec::new(),
    };
    let before = pairs.clone();
    let mut remove = None;
    for (i, (old, new)) in pairs.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ((ui.available_width() - 110.0) / 2.0).max(50.0);
            egui::ComboBox::from_id_salt(("rename", spec.key, i)).selected_text(old.as_str()).width(w).show_ui(ui, |ui| {
                for f in fields {
                    ui.selectable_value(old, f.clone(), f);
                }
            });
            ui.label(RichText::new("to").small().color(pal.muted));
            ui.add(TextEdit::singleline(new).desired_width(w).hint_text("new name"));
            if ui.small_button("×").on_hover_text("Remove this rename").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        pairs.remove(i);
    }
    let free = fields.iter().find(|f| !pairs.iter().any(|p| &p.0 == *f));
    if ui.add_enabled(free.is_some(), egui::Button::new("Add rename").small()).clicked() {
        if let Some(f) = free {
            pairs.push((f.clone(), String::new()));
        }
    }
    if fields.is_empty() && pairs.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes can be picked here.").small().color(pal.muted));
    }
    if pairs == before {
        return false;
    }
    if pairs.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Object(pairs.into_iter().map(|(k, v)| (k, Json::String(v))).collect()));
    }
    true
}

/// The arrow buttons and other small symbols are drawn in the monospace font, which has them.
pub(super) fn symbol(ui: &mut Ui, text: &str, enabled: bool, hint: &str) -> bool {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(text).font(theme::mono(11.0))).small().frame(false)).on_hover_text(hint).clicked()
}

/// Manage attributes: every attribute that comes in, with Keep, Rename or Remove, and arrows
/// to change the order. What is stored is the whole list as it is shown, once something in
/// it has been changed.
pub(super) fn attribute_table(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[(String, &'static str)], pal: &Palette) -> bool {
    use og_engine::nodes::{attribute_rules, manager_from_old, manager_is_old, rules_json, AttrAction};
    let names: Vec<String> = fields.iter().map(|f| f.0.clone()).collect();
    // A node from before 0.53 has lists to keep and remove. They become rows once the
    // attributes that come in are known.
    if manager_is_old(&node.params) {
        if names.is_empty() {
            ui.label(
                RichText::new("Run the model, and the attributes are listed here. The settings this node had in an earlier version are kept until then.")
                    .small()
                    .color(pal.muted),
            );
            return false;
        }
        let (rows, only_listed) = manager_from_old(&node.params, &names);
        for k in ["keep", "remove", "rename"] {
            node.params.remove(k);
        }
        node.params.insert(spec.key.to_string(), rules_json(&rows));
        if only_listed {
            node.params.insert("others".to_string(), Json::String("remove".into()));
        }
        return true;
    }
    let others_go = node.params.get("others").and_then(|v| v.as_str()) == Some("remove");
    let unlisted = if others_go { AttrAction::Remove } else { AttrAction::Keep };
    let stored = attribute_rules(&node.params);
    // The types chosen, by name: "integer", "decimal" or "text". Empty keeps the type.
    let stored_types: Vec<(String, String)> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => {
            a.iter().filter_map(|r| Some((r.get("name")?.as_str()?.to_string(), r.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string()))).collect()
        }
        _ => Vec::new(),
    };
    let mut types: Vec<String> = Vec::new();
    // The rows: what is listed, in its order, and then what has come in since.
    let mut rows: Vec<(String, AttrAction, bool)> = stored.iter().map(|(n, a)| (n.clone(), a.clone(), true)).collect();
    for n in &names {
        if !rows.iter().any(|r| &r.0 == n) {
            rows.push((n.clone(), unlisted.clone(), false));
        }
    }
    if rows.is_empty() {
        ui.label(RichText::new("Connect a stream and run, and the attributes are listed here.").small().color(pal.muted));
        return false;
    }
    for r in &rows {
        types.push(stored_types.iter().find(|t| t.0 == r.0).map(|t| t.1.clone()).unwrap_or_default());
    }
    let types_before = types.clone();
    let before: Vec<(String, AttrAction)> = rows.iter().map(|r| (r.0.clone(), r.1.clone())).collect();
    let mut moved: Option<(usize, usize)> = None;
    let mut dropped = None;
    let count = rows.len();
    ui.spacing_mut().item_spacing.y = 2.0;
    for (i, (name, action, listed)) in rows.iter_mut().enumerate() {
        let here = names.contains(name);
        let ty = fields.iter().find(|f| &f.0 == name).map_or("", |f| f.1);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if symbol(ui, "▲", i > 0, "Move up") {
                moved = Some((i, i - 1));
            }
            if symbol(ui, "▼", i + 1 < count, "Move down") {
                moved = Some((i, i + 1));
            }
            ui.add_space(4.0);
            // The name takes what the lists of actions and types leave.
            let width = (ui.available_width() - 176.0).max(40.0);
            ui.allocate_ui_with_layout(egui::vec2(width, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.x = 4.0;
                let text = RichText::new(name.as_str()).color(if here { pal.ink } else { pal.muted });
                let text = if *action == AttrAction::Remove { text.strikethrough() } else { text };
                let hover = match (here, *listed) {
                    (false, _) => "Not in the stream that comes in".to_string(),
                    (true, false) => format!("{}. Not in the list yet, so it is treated as the attributes that are not listed", ty),
                    (true, true) => ty.to_string(),
                };
                ui.add(egui::Label::new(text).truncate()).on_hover_text(hover);
                let note = match (here, *listed) {
                    (false, _) => "gone",
                    (true, false) => "new",
                    (true, true) => "",
                };
                if !note.is_empty() {
                    ui.label(RichText::new(note).font(theme::mono(10.5)).color(pal.muted));
                }
            });
            let label = match action {
                AttrAction::Keep => "Keep",
                AttrAction::Rename(_) => "Rename",
                AttrAction::Remove => "Remove",
            };
            egui::ComboBox::from_id_salt(("attr_action", spec.key, i)).selected_text(label).width(78.0).show_ui(ui, |ui| {
                let renamed = match action {
                    AttrAction::Rename(to) => to.clone(),
                    _ => String::new(),
                };
                ui.selectable_value(action, AttrAction::Keep, "Keep");
                ui.selectable_value(action, AttrAction::Rename(renamed), "Rename");
                ui.selectable_value(action, AttrAction::Remove, "Remove");
            });
            // The type: as it comes in, or another that the values are made into.
            let names_of = [("integer", "integer"), ("decimal", "decimal"), ("text", "text")];
            let current = types[i].clone();
            let shown = if current.is_empty() { ty.to_string() } else { format!("→ {}", current) };
            ui.add_space(4.0);
            egui::ComboBox::from_id_salt(("attr_type", spec.key, i))
                .selected_text(shown)
                .width(76.0)
                .show_ui(ui, |ui| {
                    for (v, l) in names_of {
                        let label = if l == ty { format!("{} (as it is)", l) } else { l.to_string() };
                        if ui.selectable_label(current == v || (current.is_empty() && l == ty), label).clicked() {
                            types[i] = if l == ty { String::new() } else { v.to_string() };
                        }
                    }
                })
                .response
                .on_hover_text(
                    "The type of the attribute. Another type makes the values into it; those that cannot be, such as text that is not a number, are left empty",
                );
            if !here && symbol(ui, "×", true, "Take it out of the list") {
                dropped = Some(i);
            }
        });
        if let AttrAction::Rename(to) = action {
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(RichText::new("to").small().color(pal.muted));
                ui.add(TextEdit::singleline(to).desired_width((ui.available_width() - 4.0).max(60.0)).hint_text("new name"));
            });
        }
    }
    let mut after: Vec<(String, AttrAction)> = rows.iter().map(|r| (r.0.clone(), r.1.clone())).collect();
    if let Some((a, b)) = moved {
        after.swap(a, b);
        types.swap(a, b);
    }
    if let Some(i) = dropped {
        after.remove(i);
        types.remove(i);
    }
    if after == before && types == types_before {
        return false;
    }
    let mut json = rules_json(&after);
    if let Json::Array(list) = &mut json {
        for (row, t) in list.iter_mut().zip(&types) {
            if !t.is_empty() {
                row["type"] = Json::String(t.clone());
            }
        }
    }
    node.params.insert(spec.key.to_string(), json);
    true
}

/// New attributes, each with a name and an expression for the value.
pub(super) fn new_attribute_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, fields: &[String], pal: &Palette) -> bool {
    let types = og_engine::nodes::attribute_types(&node.params, spec.key);
    let type_text = |t: og_engine::model::FieldType| match t {
        og_engine::model::FieldType::Int => "integer",
        og_engine::model::FieldType::Real => "decimal",
        og_engine::model::FieldType::Text => "text",
    };
    let mut rows: Vec<(String, String, String)> = og_engine::nodes::new_attributes(&node.params)
        .into_iter()
        .map(|(n, v)| {
            let t = types.iter().find(|t| t.0 == n).map_or("", |t| type_text(t.1)).to_string();
            (n, v, t)
        })
        .collect();
    let before = rows.clone();
    let mut remove = None;
    for (i, (name, value, ty)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.add(TextEdit::singleline(name).desired_width((w * 0.3).max(60.0)).hint_text("name"));
            ui.label(RichText::new("=").color(pal.muted));
            ui.add(TextEdit::singleline(value).desired_width((ui.available_width() - 132.0).max(60.0)).font(theme::mono(12.5)).hint_text("empty"));
            let shown = if ty.is_empty() { "as it comes" } else { ty.as_str() }.to_string();
            egui::ComboBox::from_id_salt(("new_type", spec.key, i)).selected_text(shown).width(76.0).show_ui(ui, |ui| {
                for (v, l) in [("", "as it comes"), ("integer", "integer"), ("decimal", "decimal"), ("text", "text")] {
                    ui.selectable_value(ty, v.to_string(), l);
                }
            });
            if symbol(ui, "×", true, "Remove this attribute") {
                remove = Some(i);
            }
        });
        if !fields.is_empty() && !value.trim().is_empty() {
            if let Err(e) = expr::compile(value, fields) {
                ui.colored_label(pal.err, RichText::new(e.0).small());
            }
        }
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.add(egui::Button::new("Add attribute").small()).clicked() {
        rows.push((String::new(), String::new(), String::new()));
    }
    if rows.iter().any(|r| r.1.trim().is_empty() && !r.0.trim().is_empty()) {
        ui.label(RichText::new("An attribute without a value is empty, of the type chosen, or text.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        let list = rows
            .into_iter()
            .map(|(n, v, t)| if t.is_empty() { serde_json::json!({ "name": n, "value": v }) } else { serde_json::json!({ "name": n, "value": v, "type": t }) })
            .collect();
        node.params.insert(spec.key.to_string(), Json::Array(list));
    }
    true
}
