//! The rows of the forms that list values: Filter by value, the paths of JSON, and Map values.

use super::*;

/// The values of Filter by value, one per row, and a button that takes them from the data.
pub(super) fn value_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, attr: &str, found: &[String], pal: &Palette) -> bool {
    let mut values: Vec<String> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect(),
        Some(Json::String(s)) => s.lines().map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let before = values.clone();
    let mut remove = None;
    // The rows ticked, to remove or keep several at once: kept by their place in the list
    // while the form is open.
    let ticked_id = egui::Id::new(("values-ticked", node.id.clone(), spec.key));
    let mut ticked: Vec<bool> = ui.data_mut(|d| d.get_temp::<Vec<bool>>(ticked_id)).unwrap_or_default();
    ticked.resize(values.len(), false);
    for (i, v) in values.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut ticked[i], "").on_hover_text("Tick several to remove them, or to keep only them, at once");
            ui.add(TextEdit::singleline(v).desired_width((ui.available_width() - 26.0).max(60.0)).hint_text("value"));
            if symbol(ui, "×", true, "Remove this value and its output") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        values.remove(i);
        ticked.remove(i);
    }
    let n_ticked = ticked.iter().filter(|t| **t).count();
    if values.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            if ui.add(egui::Button::new("Tick all").small()).clicked() {
                ticked.iter_mut().for_each(|t| *t = true);
            }
            if ui.add_enabled(n_ticked > 0, egui::Button::new("None").small()).clicked() {
                ticked.iter_mut().for_each(|t| *t = false);
            }
            if ui.add_enabled(n_ticked > 0, egui::Button::new(format!("Remove the {} ticked", n_ticked)).small()).clicked() {
                let mut k = 0;
                values.retain(|_| {
                    k += 1;
                    !ticked[k - 1]
                });
                ticked = vec![false; values.len()];
            }
            if ui.add_enabled(n_ticked > 0 && n_ticked < values.len(), egui::Button::new("Keep only the ticked").small()).clicked() {
                let mut k = 0;
                values.retain(|_| {
                    k += 1;
                    ticked[k - 1]
                });
                ticked = vec![false; values.len()];
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Add value").small()).clicked() {
            values.push(String::new());
        }
        let button = ui.add_enabled(!found.is_empty(), egui::Button::new("Use the values in the data").small());
        let why_not = if attr.is_empty() { "Choose the attribute first" } else { "Run the model, and the values that come in can be taken from there" };
        if button.on_hover_text(format!("Lists the {} values of {} in the latest run", found.len(), attr)).on_disabled_hover_text(why_not).clicked() {
            values = found.to_vec();
            ticked = vec![false; values.len()];
        }
    });
    ticked.resize(values.len(), false);
    ui.data_mut(|d| d.insert_temp(ticked_id, ticked));
    if found.len() >= 100 {
        ui.label(RichText::new("The data has more values than that. The first hundred are taken.").small().color(pal.muted));
    }
    if values == before {
        return false;
    }
    if values.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(values.into_iter().map(Json::String).collect()));
    }
    true
}

/// Extract JSON values: rows of a path and the attribute it goes to, and the paths that the
/// JSON of the latest run has, to click.
pub(super) fn json_path_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, found: &[String], pal: &Palette) -> bool {
    let mut rows = og_engine::nodes::json_paths_of(&node.params, spec.key);
    // Rows being typed may have an empty path; those are kept while the form is open.
    if let Some(Json::Array(a)) = node.params.get(spec.key) {
        rows = a
            .iter()
            .filter_map(|r| {
                r.as_array()
                    .map(|r| (r.first().and_then(|v| v.as_str()).unwrap_or("").to_string(), r.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string()))
            })
            .collect();
    }
    let before = rows.clone();
    let mut remove = None;
    for (i, (path, name)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ((ui.available_width() - 34.0) / 2.0).max(60.0);
            ui.add(TextEdit::singleline(path).id_salt(("json-path", i)).desired_width(w).font(theme::mono(12.0)).hint_text("path"));
            let hint = if path.trim().is_empty() { "name".to_string() } else { og_engine::nodes::name_of_path(path) };
            ui.add(TextEdit::singleline(name).id_salt(("json-name", i)).desired_width(w).hint_text(hint));
            if symbol(ui, "×", true, "Remove this value") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.add(egui::Button::new("Add value").small()).clicked() {
        rows.push((String::new(), String::new()));
    }
    // The paths in the JSON that came in, from the first features.
    let mut paths: Vec<String> = Vec::new();
    for text in found.iter().take(20) {
        if let Ok(v) = og_engine::json::parse(text) {
            og_engine::json::paths(&v, "", &mut paths);
        }
        if paths.len() > 60 {
            break;
        }
    }
    if paths.is_empty() {
        ui.label(RichText::new("Run the model, and the paths in the JSON can be clicked here.").small().color(pal.muted));
    } else {
        ui.label(RichText::new("Paths in the JSON, to click:").small().color(pal.muted));
        if let Some(p) = chips(ui, pal, &paths, |p| rows.iter().any(|r| r.0 == p)) {
            match rows.iter().position(|r| r.0 == p) {
                Some(i) => {
                    rows.remove(i);
                }
                None => rows.push((p, String::new())),
            }
        }
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(rows.into_iter().map(|(p, n)| Json::Array(vec![Json::String(p), Json::String(n)])).collect()));
    }
    true
}

/// Map values: rows of a value and what it becomes. Stored as a JSON list of [from, to].
pub(super) fn mapping_rows(ui: &mut Ui, node: &mut GNode, spec: &ParamSpec, attr: &str, found: &[String], pal: &Palette) -> bool {
    let text = |v: &Json| match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let mut rows: Vec<(String, String)> = match node.params.get(spec.key) {
        Some(Json::Array(a)) => a.iter().filter_map(|r| r.as_array().filter(|r| r.len() == 2).map(|r| (text(&r[0]), text(&r[1])))).collect(),
        Some(Json::Object(o)) => o.iter().map(|(k, v)| (k.clone(), text(v))).collect(),
        _ => Vec::new(),
    };
    let before = rows.clone();
    let mut remove = None;
    for (i, (from, to)) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ((ui.available_width() - 60.0) / 2.0).max(50.0);
            ui.add(TextEdit::singleline(from).id_salt(("map-from", i)).desired_width(w).hint_text("value"));
            ui.label(RichText::new("→").font(theme::mono(11.0)).color(pal.muted));
            ui.add(TextEdit::singleline(to).id_salt(("map-to", i)).desired_width(w).hint_text("becomes"));
            if symbol(ui, "×", true, "Remove this row") {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        rows.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Add value").small()).clicked() {
            rows.push((String::new(), String::new()));
        }
        let button = ui.add_enabled(!found.is_empty(), egui::Button::new("Use the values in the data").small());
        let why_not = if attr.is_empty() { "Choose the attribute first" } else { "Run the model, and the values that come in can be taken from there" };
        if button
            .on_hover_text(format!("Adds a row for each value of {} in the latest run that is not in the list yet", attr))
            .on_disabled_hover_text(why_not)
            .clicked()
        {
            for v in found {
                if !rows.iter().any(|r| r.0.trim() == v.trim()) {
                    rows.push((v.clone(), String::new()));
                }
            }
        }
    });
    if found.len() >= 100 {
        ui.label(RichText::new("The data has more values than that. The first hundred are taken.").small().color(pal.muted));
    }
    if rows == before {
        return false;
    }
    if rows.is_empty() {
        node.params.remove(spec.key);
    } else {
        node.params.insert(spec.key.to_string(), Json::Array(rows.into_iter().map(|(a, b)| Json::Array(vec![Json::String(a), Json::String(b)])).collect()));
    }
    true
}
