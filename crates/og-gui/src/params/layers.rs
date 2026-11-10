//! The rows of the layers of Draw map, the GeoSpatial PDF and the QGIS project: colours, rasters,
//! values, labels and icons.

use super::*;

/// A colour as egui has it.
/// A colour as a code, #rrggbb, or #rrggbbaa when it is not opaque: as QGIS writes them.
pub(crate) fn colour_code(c: [u8; 4]) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

/// A colour from a code: #rrggbb, #rrggbbaa or #rgb, with or without #.
pub(crate) fn parse_colour_code(text: &str) -> Option<[u8; 4]> {
    let t = text.trim().trim_start_matches('#');
    let hex = |s: &str| u8::from_str_radix(s, 16).ok();
    match t.len() {
        3 => {
            let v: Vec<u8> = t.chars().map(|ch| hex(&format!("{}{}", ch, ch))).collect::<Option<_>>()?;
            Some([v[0], v[1], v[2], 255])
        }
        6 | 8 => {
            let v: Vec<u8> = (0..t.len()).step_by(2).map(|k| t.get(k..k + 2).and_then(hex)).collect::<Option<_>>()?;
            Some([v[0], v[1], v[2], v.get(3).copied().unwrap_or(255)])
        }
        _ => None,
    }
}

/// The colour last copied with the secondary button on a colour, to paste on another.
pub(super) const COPIED_COLOUR: &str = "ographis-copied-colour";

/// A button of a colour. A click opens the colour picker with the code of the colour, which
/// can be typed, such as #80d3ff; the secondary button copies the colour or pastes the one
/// copied. Gives whether the colour changed.
pub(crate) fn colour_button(ui: &mut Ui, c: &mut [u8; 4]) -> bool {
    let before = *c;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(34.0, 18.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let stroke = if r.hovered() { ui.visuals().widgets.hovered.fg_stroke } else { ui.visuals().widgets.inactive.bg_stroke };
        ui.painter().rect(rect, 3.0, to_color(*c), stroke, egui::StrokeKind::Inside);
    }
    let r = r.on_hover_text(format!("{}. Click to choose, or type its code. The secondary button copies it or pastes a copied one", colour_code(*c)));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, colour_code(before)));
    let code_id = r.id.with("code");
    egui::Popup::from_toggle_button_response(&r).id(r.id.with("picker")).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let mut col = to_color(*c);
        if egui::color_picker::color_picker_color32(ui, &mut col, egui::color_picker::Alpha::OnlyBlend) {
            let [r, g, b, a] = col.to_srgba_unmultiplied();
            *c = [r, g, b, a];
            ui.data_mut(|d| d.remove::<String>(code_id));
        }
        ui.horizontal(|ui| {
            ui.label("Code");
            let mut text: String = ui.data(|d| d.get_temp(code_id)).unwrap_or_else(|| colour_code(*c));
            let edit = ui
                .add(TextEdit::singleline(&mut text).desired_width(90.0).hint_text("#80d3ff"))
                .on_hover_text("As in QGIS: #rrggbb, or #rrggbbaa with how opaque it is");
            if edit.changed() {
                if let Some(v) = parse_colour_code(&text) {
                    *c = v;
                }
                ui.data_mut(|d| d.insert_temp(code_id, text.clone()));
            }
            if edit.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(code_id));
            }
            if parse_colour_code(&text).is_none() {
                ui.label(RichText::new("not a colour").small().color(ui.visuals().warn_fg_color));
            }
        });
    });
    let copied_id = egui::Id::new(COPIED_COLOUR);
    r.context_menu(|ui| {
        let copied: Option<[u8; 4]> = ui.ctx().data(|d| d.get_temp(copied_id));
        if ui.button("Copy colour").clicked() {
            let v = *c;
            ui.ctx().data_mut(|d| d.insert_temp(copied_id, v));
            ui.ctx().copy_text(colour_code(v));
            ui.close();
        }
        let paste = copied.map_or("Paste colour".to_string(), |v| format!("Paste colour {}", colour_code(v)));
        if ui.add_enabled(copied.is_some(), egui::Button::new(paste)).clicked() {
            if let Some(v) = copied {
                *c = v;
            }
            ui.close();
        }
    });
    *c != before
}

pub(super) fn to_color(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// The layers of Draw map: one for each stream connected to its port, top first, each with
/// whether it is drawn, its colours and sizes, and arrows for the order. Once something is
/// changed the whole list is stored.
pub(super) fn map_layer_rows(
    ui: &mut Ui,
    node: &mut GNode,
    spec: &ParamSpec,
    connected: &[(String, String)],
    pal: &Palette,
    stream_fields: &dyn Fn(&str) -> Vec<(String, bool)>,
    stream_raster: &dyn Fn(&str) -> Option<og_engine::rasterlook::RasterInfo>,
) -> bool {
    use og_engine::nodes::{arrange_layers, map_layers, map_layers_json};
    if connected.is_empty() {
        ui.label(RichText::new("Connect the streams to draw to Layers on the node. Each connection is a layer.").small().color(pal.muted));
        return false;
    }
    let keys: Vec<String> = connected.iter().map(|c| c.0.clone()).collect();
    let named = matches!(node.kind.as_str(), "LayeredPdfWriter" | "QgisProjectWriter" | "WebMapWriter");
    // QGIS and the web map draw by values and labels, as Draw map does; the PDF does not.
    let qgis = matches!(node.kind.as_str(), "QgisProjectWriter" | "WebMapWriter");
    let place = match node.kind.as_str() {
        "QgisProjectWriter" => "QGIS",
        "WebMapWriter" => "the web map",
        _ => "the PDF",
    };
    let stored = map_layers(&node.params);
    let mut rows = arrange_layers(&stored, &keys);
    let before = rows.clone();
    let count = rows.len();
    let mut moved: Option<(usize, usize)> = None;
    // A layer dragged by its handle and let go over another: (from, to).
    let mut dropped: Option<(usize, usize)> = None;
    let copied_id = egui::Id::new(COPIED_COLOUR);
    for (i, row) in rows.iter_mut().enumerate() {
        let label = connected.iter().find(|c| c.0 == row.from).map_or(row.from.as_str(), |c| c.1.as_str());
        let frame = egui::Frame::new().fill(pal.bg).corner_radius(4).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // The handle to drag the layer by, to another place in the list.
                ui.dnd_drag_source(egui::Id::new(("layer-drag", spec.key, i)), i, |ui| {
                    // Six dots, drawn, so that they look the same whatever the font holds.
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                    for k in 0..6 {
                        let (cx, cy) = (r.left() + 3.0 + (k % 2) as f32 * 4.0, r.center().y - 4.0 + (k / 2) as f32 * 4.0);
                        ui.painter().circle_filled(egui::pos2(cx, cy), 1.1, pal.muted);
                    }
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Drag handle"));
                    resp.on_hover_cursor(egui::CursorIcon::Grab);
                })
                .response
                .on_hover_text("Drag to move the layer up or down in the list");
                ui.add_space(4.0);
                if symbol(ui, "▲", i > 0, "Move up: drawn above the layer over it") {
                    moved = Some((i, i - 1));
                }
                if symbol(ui, "▼", i + 1 < count, "Move down: drawn under the layer below it") {
                    moved = Some((i, i + 1));
                }
                ui.add_space(4.0);
                ui.checkbox(&mut row.show, "").on_hover_text(if named { format!("On or off when {} opens it", place) } else { "Drawn or not".to_string() });
                // The name of the layer: in the PDF, or in the legend of the map. The name of
                // the stream until another is given.
                let what = if named { format!("The name of the layer in {}", place) } else { "The name of the layer in the legend".to_string() };
                let colour = if row.show { pal.ink } else { pal.muted };
                ui.add(TextEdit::singleline(&mut row.name).id_salt(("layer-name", i)).hint_text(label).text_color(colour).desired_width(ui.available_width()))
                    .on_hover_text(format!("{}. Empty gives {}", what, label))
                    // Screen readers, and the tests, know the field by the layer it names.
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
            });
            // A raster is drawn by its cells, with settings of its own.
            let raster = if named { None } else { stream_raster(&row.from) };
            if let Some(info) = &raster {
                raster_rows(ui, &mut row.raster, info, &row.from, pal);
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
                // The secondary button on a colour copies it, and pastes a copied one on another.
                let menu = |r: egui::Response, value: &mut Option<[u8; 4]>| {
                    r.context_menu(|ui| {
                        let copied: Option<[u8; 4]> = ui.ctx().data(|d| d.get_temp(copied_id));
                        if ui.add_enabled(value.is_some(), egui::Button::new("Copy colour")).clicked() {
                            if let Some(c) = *value {
                                ui.ctx().data_mut(|d| d.insert_temp(copied_id, c));
                                ui.ctx().copy_text(format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]));
                            }
                            ui.close();
                        }
                        let paste = match copied {
                            Some(c) => format!("Paste colour #{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
                            None => "Paste colour".to_string(),
                        };
                        if ui.add_enabled(copied.is_some(), egui::Button::new(paste)).clicked() {
                            *value = copied;
                            ui.close();
                        }
                    });
                };
                let colour = |ui: &mut Ui, value: &mut Option<[u8; 4]>, name: &str, hint: &str, fallback: [u8; 4]| {
                    let mut on = value.is_some();
                    let r = ui.checkbox(&mut on, name).on_hover_text(hint);
                    if r.changed() {
                        *value = if on { Some(fallback) } else { None };
                    }
                    menu(r, value);
                    if let Some(c) = value {
                        colour_button(ui, c);
                    }
                    ui.add_space(6.0);
                };
                let defaults = og_engine::mapdraw::LayerStyle::default_for(i);
                colour(ui, &mut row.style.fill, "Fill", "The colour of areas and points", defaults.fill.unwrap_or([0, 0, 0, 255]));
                colour(
                    ui,
                    &mut row.style.stroke,
                    "Line",
                    "The colour of lines, and of the outlines of areas and points",
                    defaults.stroke.unwrap_or([0, 0, 0, 255]),
                );
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("width").small().color(pal.muted));
                ui.add(egui::DragValue::new(&mut row.style.width).range(0.0..=50.0).speed(0.1).max_decimals(1))
                    .on_hover_text("Of lines and outlines, in pixels");
                ui.add_space(6.0);
                ui.label(RichText::new("point size").small().color(pal.muted));
                ui.add(egui::DragValue::new(&mut row.style.size).range(0.0..=100.0).speed(0.2).max_decimals(1))
                    .on_hover_text("The diameter of a point, in pixels");
            });
            // How the lines are drawn: dashes, and an edge under them as on a road map.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("line").small().color(pal.muted));
                let shown = og_engine::mapdraw::DASHES.iter().find(|d| d.0 == row.style.dash).map_or("Solid", |d| d.1);
                egui::ComboBox::from_id_salt(("dash", i)).selected_text(shown).width(110.0).show_ui(ui, |ui| {
                    for (key, name) in og_engine::mapdraw::DASHES {
                        ui.selectable_value(&mut row.style.dash, key.to_string(), *name);
                    }
                });
                ui.add_space(6.0);
                let mut edge = row.style.casing.is_some();
                if ui.checkbox(&mut edge, "Edge").on_hover_text("A wider line under each line, as the edge of a road").changed() {
                    row.style.casing = if edge { Some([0x55, 0x55, 0x55, 255]) } else { None };
                }
                if let Some(c) = &mut row.style.casing {
                    colour_button(ui, c);
                    ui.add(egui::DragValue::new(&mut row.style.casing_width).range(0.0..=20.0).speed(0.1).max_decimals(1))
                        .on_hover_text("How much wider than the line, in pixels on each side");
                }
            });
            // Drawing by the values of an attribute: Draw map and QGIS, not the PDF. Icons
            // only in Draw map.
            if !named || qgis {
                let fields = stream_fields(&row.from);
                theme_rows(ui, &mut row.theme, &row.from, &fields, pal);
                label_rows(ui, &mut row.labels, &row.from, &fields, pal);
                if !named {
                    icon_rows(ui, &mut row.icon, &row.from, &fields, pal);
                }
            }
        });
        // A layer dragged over this one: a line shows where it goes, above or below.
        let r = &frame.response;
        if let (Some(from), Some(p)) = (r.dnd_hover_payload::<usize>(), ui.ctx().pointer_hover_pos()) {
            if *from != i {
                let below = p.y > r.rect.center().y;
                let y = if below { r.rect.bottom() + 1.0 } else { r.rect.top() - 1.0 };
                ui.painter().hline(r.rect.x_range(), y, egui::Stroke::new(2.0, pal.accent));
            }
        }
        if let (Some(from), Some(p)) = (r.dnd_release_payload::<usize>(), ui.ctx().pointer_interact_pos()) {
            if *from != i {
                let below = p.y > r.rect.center().y;
                // The place in the list without the dragged layer.
                let mut to = if below { i + 1 } else { i };
                if *from < to {
                    to -= 1;
                }
                dropped = Some((*from, to));
            }
        }
        ui.add_space(2.0);
    }
    if let Some((a, b)) = moved {
        rows.swap(a, b);
    }
    if let Some((from, to)) = dropped {
        let layer = rows.remove(from);
        rows.insert(to.min(rows.len()), layer);
    }
    // Nothing changed: the list stays as it is stored, or not stored at all.
    if rows == before {
        return false;
    }
    node.params.insert(spec.key.to_string(), map_layers_json(&rows));
    true
}

/// How a layer of Draw map draws a raster: a picture as it is; a grid by a colour ramp, a
/// colour for each value, or as relief; and how see-through.
pub(super) fn raster_rows(ui: &mut Ui, r: &mut og_engine::rasterlook::RasterSpec, info: &og_engine::rasterlook::RasterInfo, from: &str, pal: &Palette) {
    use og_engine::rasterlook::{class_colour, value_text};
    if !info.image {
        const MODES: &[(&str, &str)] =
            &[("", "By its values"), ("ramp", "Colour ramp"), ("classes", "A colour for each value"), ("shade", "Grey relief, as heights")];
        let auto = if info.classes.is_some() { "classes" } else { "ramp" };
        let shown = MODES.iter().find(|m| m.0 == r.mode).map_or("By its values", |m| m.1);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Draw as").small().color(pal.muted));
            egui::ComboBox::from_id_salt(("raster-mode", from)).selected_text(shown).width(190.0).show_ui(ui, |ui| {
                for (v, l) in MODES {
                    ui.selectable_value(&mut r.mode, v.to_string(), *l);
                }
            });
        });
        let mode = if r.mode.is_empty() { auto } else { r.mode.as_str() };
        match mode {
            "classes" => {
                // Each value, with its colour, whether it is drawn, and its name in the legend.
                let mut values: Vec<f64> = info.classes.clone().unwrap_or_default();
                for (v, _, _) in &r.colors {
                    if let Some(v) = og_engine::model::parse_number(v) {
                        if !values.contains(&v) {
                            values.push(v);
                        }
                    }
                }
                values.sort_by(f64::total_cmp);
                if values.is_empty() {
                    ui.label(RichText::new("Run the model, and the values of the raster are listed here.").small().color(pal.muted));
                }
                for (k, v) in values.iter().enumerate() {
                    let text = value_text(*v);
                    let at = r.colors.iter().position(|c| og_engine::model::parse_number(&c.0) == Some(*v));
                    let (colour, label) = match at {
                        Some(i) => (og_engine::mapdraw::parse_color(&r.colors[i].1).ok().unwrap_or(Some(class_colour(k))), r.colors[i].2.clone()),
                        None => (Some(class_colour(k)), String::new()),
                    };
                    let (mut on, mut c, mut name) = (colour.is_some(), colour.unwrap_or(class_colour(k)), label);
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        changed |= ui
                            .checkbox(&mut on, RichText::new(&text).monospace())
                            .on_hover_text("Drawn or not: a value that is not drawn lets the map under it show")
                            .changed();
                        if on {
                            changed |= colour_button(ui, &mut c);
                        }
                        changed |= ui
                            .add(TextEdit::singleline(&mut name).id_salt(("raster-class", from, k)).hint_text("name in the legend").desired_width(120.0))
                            .on_hover_text(format!("What {} stands for, such as Seen. Empty shows the value", text))
                            .changed();
                    });
                    if changed {
                        let colour = if on { og_engine::mapdraw::color_text(Some(c)) } else { "none".to_string() };
                        match at {
                            Some(i) => r.colors[i] = (text.clone(), colour, name),
                            None => r.colors.push((text.clone(), colour, name)),
                        }
                    }
                }
            }
            "ramp" => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Colours").small().color(pal.muted));
                    let shown = og_engine::thematic::RAMPS.iter().find(|x| x.0 == r.ramp).map_or("Yellow to blue", |x| x.1);
                    egui::ComboBox::from_id_salt(("raster-ramp", from)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                        for (v, l, _) in og_engine::thematic::RAMPS {
                            ui.selectable_value(&mut r.ramp, v.to_string(), *l);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (lo, hi) = info.range.unwrap_or((0.0, 1.0));
                    let edit = |ui: &mut Ui, v: &mut Option<f64>, hint: f64, id: &str| {
                        let mut t = v.map(value_text).unwrap_or_default();
                        let resp =
                            ui.add(TextEdit::singleline(&mut t).id_salt((id, from)).hint_text(value_text((hint * 100.0).round() / 100.0)).desired_width(70.0));
                        if resp.changed() {
                            *v = og_engine::model::parse_number(&t);
                        }
                    };
                    ui.label(RichText::new("from").small().color(pal.muted));
                    edit(ui, &mut r.min, lo, "raster-min");
                    ui.label(RichText::new("to").small().color(pal.muted));
                    edit(ui, &mut r.max, hi, "raster-max");
                })
                .response
                .on_hover_text("The values at the ends of the colours. Empty takes the smallest and the largest");
            }
            _ => {}
        }
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Opacity").small().color(pal.muted));
        let mut pct = (r.opacity * 100.0).round();
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix(" %").step_by(5.0)).on_hover_text("How much it covers what is under it").changed() {
            r.opacity = pct / 100.0;
        }
    });
}

/// How a layer of Draw map shows the values of its features: size, colour, order, arrows,
/// heatmap and clusters, folded in under "Values" until it is used.
pub(super) fn theme_rows(ui: &mut Ui, t: &mut og_engine::thematic::ThemeSpec, from: &str, fields: &[(String, bool)], pal: &Palette) {
    let on = !t.is_off();
    let title = if on { "Values · in use" } else { "Values" };
    egui::CollapsingHeader::new(RichText::new(title).small().color(if on { pal.accent } else { pal.muted })).id_salt(("theme", from)).default_open(on).show(
        ui,
        |ui| {
            if fields.is_empty() {
                ui.label(RichText::new("Run the model, and the attributes of this layer can be chosen.").small().color(pal.muted));
            }
            let pick = |ui: &mut Ui, id: &str, value: &mut String, numbers_only: bool| {
                let shown = if value.is_empty() { "None".to_string() } else { value.clone() };
                egui::ComboBox::from_id_salt((id, from)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                    ui.selectable_value(value, String::new(), "None");
                    for (f, num) in fields {
                        if *num || !numbers_only {
                            ui.selectable_value(value, f.clone(), f);
                        }
                    }
                });
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("Size by").small().color(pal.muted)).on_hover_text("Points get larger with the value: the area of the point follows it");
                pick(ui, "size-by", &mut t.size_by, true);
            });
            if !t.size_by.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("from").small().color(pal.muted));
                    ui.add(egui::DragValue::new(&mut t.size_min).range(1.0..=200.0).speed(0.2).max_decimals(1));
                    ui.label(RichText::new("to").small().color(pal.muted));
                    ui.add(egui::DragValue::new(&mut t.size_max).range(1.0..=300.0).speed(0.2).max_decimals(1));
                    ui.label(RichText::new("pixels").small().color(pal.muted));
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Colour by").small().color(pal.muted))
                    .on_hover_text("A colour after the value: classes of a number, or a colour per text. For areas, a choropleth map");
                pick(ui, "color-by", &mut t.color_by, false);
            });
            if !t.color_by.is_empty() {
                let numeric = fields.iter().find(|f| f.0 == t.color_by).is_none_or(|f| f.1) && t.ramp != "categories" && t.ramp != "as_written";
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let shown = if t.ramp == "categories" {
                        "A colour per value"
                    } else if t.ramp == "as_written" {
                        "The colours written in it"
                    } else {
                        og_engine::thematic::RAMPS.iter().find(|r| r.0 == t.ramp).map_or("Blues", |r| r.1)
                    };
                    egui::ComboBox::from_id_salt(("ramp", from)).selected_text(shown).width(130.0).show_ui(ui, |ui| {
                        for (v, l, _) in og_engine::thematic::RAMPS {
                            ui.selectable_value(&mut t.ramp, v.to_string(), *l);
                        }
                        ui.selectable_value(&mut t.ramp, "categories".to_string(), "A colour per value");
                        ui.selectable_value(&mut t.ramp, "as_written".to_string(), "The colours written in it")
                            .on_hover_text("For an attribute that holds colours as #rrggbb, such as colour in a CAD drawing");
                    });
                    if numeric {
                        ui.add(egui::DragValue::new(&mut t.classes).range(1..=12).speed(0.1)).on_hover_text("The number of classes");
                        ui.label(RichText::new("classes").small().color(pal.muted));
                        let methods = [("equal", "Equal intervals"), ("quantile", "As many in each"), ("natural", "Natural breaks")];
                        let shown = methods.iter().find(|m| m.0 == t.method).map_or("Equal intervals", |m| m.1);
                        egui::ComboBox::from_id_salt(("method", from)).selected_text(shown).width(130.0).show_ui(ui, |ui| {
                            for (v, l) in methods {
                                ui.selectable_value(&mut t.method, v.to_string(), l);
                            }
                        });
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Order").small().color(pal.muted))
                    .on_hover_text("Which are drawn first, and so lie underneath. By the attribute of the size, or else of the colour");
                let orders = [("", "As they come"), ("large_below", "Largest underneath"), ("small_below", "Smallest underneath")];
                let shown = orders.iter().find(|o| o.0 == t.order).map_or("As they come", |o| o.1);
                egui::ComboBox::from_id_salt(("order", from)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                    for (v, l) in orders {
                        ui.selectable_value(&mut t.order, v.to_string(), l);
                    }
                });
            });
            if !t.order.is_empty() && t.size_by.is_empty() && t.color_by.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("by").small().color(pal.muted));
                    pick(ui, "order-by", &mut t.order_by, true);
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Arrows by").small().color(pal.muted))
                    .on_hover_text("Points become arrows the way of this attribute: degrees from north, clockwise, such as the direction of wind or a current. Their length is the size, which Size by can set by the speed");
                pick(ui, "arrow-by", &mut t.arrow_by, true);
            });
            if !t.arrow_by.is_empty() {
                ui.checkbox(&mut t.arrow_from, RichText::new("It says where it comes from, as for wind").small())
                    .on_hover_text("Wind from 270° comes from the west, so the arrow points east");
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Heatmap within").small().color(pal.muted))
                    .on_hover_text("Points become a heatmap: how many lie near each place, coloured by the ramp of Colour by, or yellow to red. 0 draws the points");
                ui.add(egui::DragValue::new(&mut t.heat).range(0.0..=300.0).speed(0.5).max_decimals(0));
                ui.label(RichText::new("pixels").small().color(pal.muted));
            });
            if t.heat > 0.0 {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("each counting as").small().color(pal.muted)).on_hover_text("Such as the number of people at an address. None: each point counts as one");
                    pick(ui, "heat-by", &mut t.heat_by, true);
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Gather points within").small().color(pal.muted))
                    .on_hover_text("Points closer than this on the map are drawn as one, with their number in it. 0 draws every point");
                ui.add(egui::DragValue::new(&mut t.cluster).range(0.0..=200.0).speed(0.5).max_decimals(0));
                ui.label(RichText::new("pixels").small().color(pal.muted));
            });
        },
    );
}

/// A list of the attributes of a layer to choose one from, or none.
pub(super) fn attribute_pick(ui: &mut Ui, id: (&str, &str), value: &mut String, fields: &[(String, bool)], numbers_only: bool, width: f32) {
    let shown = if value.is_empty() { "None".to_string() } else { value.clone() };
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(width).show_ui(ui, |ui| {
        ui.selectable_value(value, String::new(), "None");
        for (f, num) in fields {
            if *num || !numbers_only {
                ui.selectable_value(value, f.clone(), f);
            }
        }
    });
}

/// A colour that can be switched off, with a button to choose it.
pub(super) fn optional_colour(ui: &mut Ui, value: &mut Option<[u8; 4]>, name: &str, hint: &str, fallback: [u8; 4]) {
    let mut on = value.is_some();
    if ui.checkbox(&mut on, name).on_hover_text(hint).changed() {
        *value = if on { Some(fallback) } else { None };
    }
    if let Some(c) = value {
        colour_button(ui, c);
    }
}

/// How a layer of Draw map labels its features, folded in under "Labels" until it is used.
pub(super) fn label_rows(ui: &mut Ui, l: &mut og_engine::mapdraw::LabelSpec, from: &str, fields: &[(String, bool)], pal: &Palette) {
    let on = !l.text.trim().is_empty();
    let title = if on { "Labels · in use" } else { "Labels" };
    egui::CollapsingHeader::new(RichText::new(title).small().color(if on { pal.accent } else { pal.muted })).id_salt(("labels", from)).default_open(on).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Text").small().color(pal.muted)).on_hover_text("An attribute, or an expression such as name || ' ' || year");
                let mut chosen = if fields.iter().any(|f| f.0 == l.text) { l.text.clone() } else { String::new() };
                let before = chosen.clone();
                attribute_pick(ui, ("label-attr", from), &mut chosen, fields, false, 120.0);
                if chosen != before {
                    l.text = chosen;
                }
            });
            ui.add(
                TextEdit::singleline(&mut l.text).id_salt(("label-text", from)).hint_text("an attribute or an expression").desired_width(ui.available_width()),
            )
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Label text"));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Placed").small().color(pal.muted));
                let places = [
                    ("auto", "Beside points, bending along lines"),
                    ("straight", "Beside points, straight along lines"),
                    ("center", "On the point, in the middle"),
                    ("xy", "At x and y from attributes"),
                ];
                let shown = places.iter().find(|p| p.0 == l.place).map_or(places[0].1, |p| p.1);
                egui::ComboBox::from_id_salt(("label-place", from)).selected_text(shown).width(170.0).show_ui(ui, |ui| {
                    for (v, t) in places {
                        ui.selectable_value(&mut l.place, v.to_string(), t);
                    }
                });
            });
            if l.place == "xy" {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("x").small().color(pal.muted));
                    attribute_pick(ui, ("label-x", from), &mut l.x_attr, fields, true, 90.0);
                    ui.label(RichText::new("y").small().color(pal.muted));
                    attribute_pick(ui, ("label-y", from), &mut l.y_attr, fields, true, 90.0);
                });
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut l.all, "All, also where they overlap")
                    .on_hover_text("Otherwise a label that would cover another, or reach outside the map, is left out");
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("First").small().color(pal.muted))
                    .on_hover_text("When there is not room for all: the largest values of this attribute get it first");
                attribute_pick(ui, ("label-priority", from), &mut l.priority_by, fields, true, 120.0);
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("size").small().color(pal.muted));
                ui.add(egui::DragValue::new(&mut l.size).range(4.0..=200.0).speed(0.2).max_decimals(1)).on_hover_text("The height of the letters, in pixels");
                ui.label(RichText::new("by").small().color(pal.muted));
                attribute_pick(ui, ("label-size-by", from), &mut l.size_by, fields, true, 100.0);
                if !l.size_by.is_empty() {
                    ui.label(RichText::new("to").small().color(pal.muted));
                    ui.add(egui::DragValue::new(&mut l.size_max).range(4.0..=300.0).speed(0.2).max_decimals(1)).on_hover_text("The size of the largest value");
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.checkbox(&mut l.bold, "Bold");
                ui.checkbox(&mut l.upper, "CAPITALS");
                ui.checkbox(&mut l.spaced, "Spread out").on_hover_text("The letters spread out over an area, as the name of a lake or a district");
                let mut c = Some(l.colour);
                optional_colour(ui, &mut c, "Colour", "The colour of the letters", [0x22, 0x22, 0x22, 255]);
                if let Some(c) = c {
                    l.colour = c;
                }
                optional_colour(ui, &mut l.halo, "Halo", "An edge around the letters, so that they can be read on anything", [255, 255, 255, 255]);
                if l.halo.is_some() {
                    ui.add(egui::DragValue::new(&mut l.halo_width).range(0.0..=20.0).speed(0.1).max_decimals(1))
                        .on_hover_text("How wide the halo is, in pixels");
                }
            });
        },
    );
}

/// Icons for the points of a layer of Draw map, from Maki, folded in under "Icons".
pub(super) fn icon_rows(ui: &mut Ui, ic: &mut og_engine::mapdraw::IconSpec, from: &str, fields: &[(String, bool)], pal: &Palette) {
    let on = !ic.is_off();
    let title = if on { "Icons · in use" } else { "Icons" };
    egui::CollapsingHeader::new(RichText::new(title).small().color(if on { pal.accent } else { pal.muted })).id_salt(("icons", from)).default_open(on).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Icon").small().color(pal.muted)).on_hover_text("The same icon for every point. Icons from Maki, by Mapbox (CC0)");
            let shown = if ic.name.is_empty() { "None".to_string() } else { ic.name.clone() };
            egui::ComboBox::from_id_salt(("icon-name", from)).selected_text(shown).width(170.0).height(320.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut ic.name, String::new(), "None");
                for (n, _) in og_engine::icons::MAKI {
                    ui.selectable_value(&mut ic.name, n.to_string(), *n);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("or by").small().color(pal.muted))
                .on_hover_text("An attribute whose values name the icons, such as amenity from OpenStreetMap: hospital, doctors, police, charging_station, hotel. Swedish words such as vårdcentral and apotek work too");
            attribute_pick(ui, ("icon-by", from), &mut ic.by, fields, false, 140.0);
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Drawn").small().color(pal.muted));
            let styles = [("plain", "In the fill colour, edged in the line colour"), ("circle", "White on a circle of the fill colour")];
            let shown = styles.iter().find(|s| s.0 == ic.style).map_or(styles[0].1, |s| s.1);
            egui::ComboBox::from_id_salt(("icon-style", from)).selected_text(shown).width(230.0).show_ui(ui, |ui| {
                for (v, t) in styles {
                    ui.selectable_value(&mut ic.style, v.to_string(), t);
                }
            });
        });
        ui.label(RichText::new("The point size is the size of the icon; below 10 it is 18.").small().color(pal.muted));
    });
}
