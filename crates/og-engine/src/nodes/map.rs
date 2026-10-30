//! Draw map: the streams as layers of a picture, with a legend and values shown by colour and size.

use super::*;

// ---------- Draw map (MapRenderer) ----------

pub(super) const MAP_BACKGROUNDS: &[(&str, &str)] = &[("#ffffff", "White"), ("none", "See-through"), ("#f4f1ea", "Paper"), ("#1e2326", "Dark")];

pub(super) const LEGEND_PLACES: &[(&str, &str)] =
    &[("none", "None"), ("bottom-left", "Lower left"), ("bottom-right", "Lower right"), ("top-left", "Upper left"), ("top-right", "Upper right")];

pub(super) const MAP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "layers",
        label: "Layers",
        kind: ParamKind::MapLayers("Layers"),
        default: "",
        hint: "Each stream connected to Layers is a layer. The top row is drawn on top",
    },
    ParamSpec { key: "width", label: "Width in pixels", kind: ParamKind::Text, default: "1600", hint: "The height follows from the area that is drawn" },
    ParamSpec { key: "margin", label: "Margin in %", kind: ParamKind::Text, default: "5", hint: "Room around the features, as a share of the larger side" },
    ParamSpec { key: "background", label: "Background", kind: ParamKind::Choice(MAP_BACKGROUNDS), default: "#ffffff", hint: "" },
    ParamSpec {
        key: "legend",
        label: "Legend",
        kind: ParamKind::Choice(LEGEND_PLACES),
        default: "none",
        hint: "A box with the layers and what their colours and sizes stand for",
    },
    ParamSpec { key: "legend_title", label: "Title of the legend", kind: ParamKind::Text, default: "", hint: "" },
    ParamSpec {
        key: "crs",
        label: "Coordinate system",
        kind: ParamKind::Crs,
        default: "",
        hint: "Of the map. Empty uses that of the bottom layer. Other layers are reprojected to it",
    },
];

/// One row of the layer list as it is stored: which stream, and how it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct MapLayer {
    pub from: String,
    /// The name of the layer, where it has one, such as in a GeoSpatial PDF. Empty gives
    /// the name of the stream.
    pub name: String,
    pub show: bool,
    pub style: crate::mapdraw::LayerStyle,
    /// How it shows the values of its features: sizes, colours, order and clusters.
    pub theme: crate::thematic::ThemeSpec,
    /// How its features are labelled.
    pub labels: crate::mapdraw::LabelSpec,
    /// Icons for its points.
    pub icon: crate::mapdraw::IconSpec,
    /// How a raster in it is drawn.
    pub raster: crate::rasterlook::RasterSpec,
}

/// The rows of the layer list, top first, as they are stored. A row that cannot be read is
/// left out.
pub fn map_layers(params: &Params) -> Vec<MapLayer> {
    let Some(serde_json::Value::Array(rows)) = params.get("layers") else { return Vec::new() };
    rows.iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let from = r.get("from")?.as_str()?.to_string();
            let mut style = crate::mapdraw::LayerStyle::default_for(i);
            let color = |key: &str| r.get(key).and_then(|v| v.as_str()).map(crate::mapdraw::parse_color);
            if let Some(Ok(c)) = color("fill") {
                style.fill = c;
            }
            if let Some(Ok(c)) = color("stroke") {
                style.stroke = c;
            }
            if let Some(w) = r.get("width").and_then(|v| v.as_f64()) {
                style.width = w.clamp(0.0, 100.0) as f32;
            }
            if let Some(w) = r.get("size").and_then(|v| v.as_f64()) {
                style.size = w.clamp(0.0, 200.0) as f32;
            }
            let show = r.get("show").and_then(|v| v.as_bool()).unwrap_or(true);
            let name = r.get("name").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
            let text = |k: &str| r.get(k).and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
            let num = |k: &str, d: f64| r.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(crate::model::parse_number))).unwrap_or(d);
            let theme = crate::thematic::ThemeSpec {
                size_by: text("size_by"),
                size_min: num("size_min", 4.0) as f32,
                size_max: num("size_max", 24.0) as f32,
                color_by: text("color_by"),
                ramp: text("ramp"),
                classes: num("classes", 5.0).clamp(1.0, 12.0) as usize,
                method: text("method"),
                order: text("order"),
                order_by: text("order_by"),
                cluster: num("cluster", 0.0).max(0.0) as f32,
            };
            let d = crate::mapdraw::LabelSpec::default();
            let flag = |k: &str| r.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
            let labels = crate::mapdraw::LabelSpec {
                text: text("label"),
                place: Some(text("label_place")).filter(|v| !v.is_empty()).unwrap_or(d.place),
                x_attr: text("label_x"),
                y_attr: text("label_y"),
                all: flag("label_all"),
                size: num("label_size", d.size as f64).clamp(4.0, 200.0) as f32,
                size_by: text("label_size_by"),
                size_max: num("label_size_max", d.size_max as f64).clamp(4.0, 300.0) as f32,
                priority_by: text("label_priority"),
                bold: flag("label_bold"),
                upper: flag("label_upper"),
                colour: match color("label_colour") {
                    Some(Ok(Some(c))) => c,
                    _ => d.colour,
                },
                halo: match color("label_halo") {
                    Some(Ok(c)) => c,
                    _ => d.halo,
                },
                halo_width: num("label_halo_width", d.halo_width as f64).clamp(0.0, 20.0) as f32,
            };
            let icon = crate::mapdraw::IconSpec { name: text("icon"), by: text("icon_by"), style: text("icon_style") };
            let given = |k: &str| r.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(crate::model::parse_number)));
            let raster = crate::rasterlook::RasterSpec {
                mode: text("raster"),
                ramp: text("raster_ramp"),
                min: given("raster_min"),
                max: given("raster_max"),
                colors: r
                    .get("raster_colors")
                    .and_then(|v| v.as_array())
                    .map(|rows| {
                        rows.iter()
                            .filter_map(|row| {
                                let a = row.as_array()?;
                                let s = |v: &serde_json::Value| v.as_str().map(str::to_string).or_else(|| v.as_f64().map(crate::rasterlook::value_text));
                                Some((s(a.first()?)?, s(a.get(1)?)?, a.get(2).and_then(s).unwrap_or_default()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                opacity: (num("opacity", 100.0) / 100.0).clamp(0.0, 1.0) as f32,
            };
            Some(MapLayer { from, name, show, style, theme, labels, icon, raster })
        })
        .collect()
}

/// The layer list as it is stored.
pub fn map_layers_json(rows: &[MapLayer]) -> serde_json::Value {
    serde_json::Value::Array(
        rows.iter()
            .map(|r| {
                let mut o = serde_json::json!({
                    "from": r.from,
                    "fill": crate::mapdraw::color_text(r.style.fill),
                    "stroke": crate::mapdraw::color_text(r.style.stroke),
                    "width": r.style.width,
                    "size": r.style.size,
                });
                if !r.show {
                    o["show"] = serde_json::Value::Bool(false);
                }
                let t = &r.theme;
                let mut put = |k: &str, v: serde_json::Value| {
                    o[k] = v;
                };
                for (k, v) in [
                    ("size_by", &t.size_by),
                    ("color_by", &t.color_by),
                    ("ramp", &t.ramp),
                    ("method", &t.method),
                    ("order", &t.order),
                    ("order_by", &t.order_by),
                ] {
                    if !v.is_empty() {
                        put(k, serde_json::Value::String(v.clone()));
                    }
                }
                if !t.size_by.is_empty() {
                    put("size_min", serde_json::json!(t.size_min));
                    put("size_max", serde_json::json!(t.size_max));
                }
                if !t.color_by.is_empty() {
                    put("classes", serde_json::json!(t.classes));
                }
                if t.cluster > 0.0 {
                    put("cluster", serde_json::json!(t.cluster));
                }
                if !r.name.trim().is_empty() {
                    // As typed: a space at the end may be followed by another word.
                    o["name"] = serde_json::Value::String(r.name.clone());
                }
                // Labels and icons only when they are used, and only what differs from the
                // defaults, so that the model files stay short.
                let l = &r.labels;
                let d = crate::mapdraw::LabelSpec::default();
                if !l.text.is_empty() || *l != d {
                    let mut put = |k: &str, v: serde_json::Value| {
                        o[k] = v;
                    };
                    put("label", serde_json::json!(l.text));
                    for (k, v, dv) in [
                        ("label_place", &l.place, &d.place),
                        ("label_x", &l.x_attr, &d.x_attr),
                        ("label_y", &l.y_attr, &d.y_attr),
                        ("label_size_by", &l.size_by, &d.size_by),
                        ("label_priority", &l.priority_by, &d.priority_by),
                    ] {
                        if v != dv {
                            put(k, serde_json::json!(v));
                        }
                    }
                    for (k, v, dv) in [("label_all", l.all, d.all), ("label_bold", l.bold, d.bold), ("label_upper", l.upper, d.upper)] {
                        if v != dv {
                            put(k, serde_json::json!(v));
                        }
                    }
                    for (k, v, dv) in
                        [("label_size", l.size, d.size), ("label_size_max", l.size_max, d.size_max), ("label_halo_width", l.halo_width, d.halo_width)]
                    {
                        if v != dv {
                            put(k, serde_json::json!(v));
                        }
                    }
                    if l.colour != d.colour {
                        put("label_colour", serde_json::json!(crate::mapdraw::color_text(Some(l.colour))));
                    }
                    if l.halo != d.halo {
                        put("label_halo", serde_json::json!(crate::mapdraw::color_text(l.halo)));
                    }
                }
                for (k, v) in [("icon", &r.icon.name), ("icon_by", &r.icon.by), ("icon_style", &r.icon.style)] {
                    if !v.is_empty() {
                        o[k] = serde_json::json!(v);
                    }
                }
                // A raster only when it is set otherwise than chosen by its values.
                let ra = &r.raster;
                for (k, v) in [("raster", &ra.mode), ("raster_ramp", &ra.ramp)] {
                    if !v.is_empty() {
                        o[k] = serde_json::json!(v);
                    }
                }
                for (k, v) in [("raster_min", ra.min), ("raster_max", ra.max)] {
                    if let Some(v) = v {
                        o[k] = serde_json::json!(v);
                    }
                }
                if !ra.colors.is_empty() {
                    o["raster_colors"] =
                        serde_json::json!(ra.colors.iter().map(|(v, c, l)| if l.is_empty() { vec![v, c] } else { vec![v, c, l] }).collect::<Vec<_>>());
                }
                if ra.opacity < 1.0 {
                    o["opacity"] = serde_json::json!((ra.opacity * 100.0).round());
                }
                o
            })
            .collect(),
    )
}

/// The layers in the order they are listed, top first: those in the stored list, and on top
/// of them the streams that are connected but not listed yet, in the default look.
pub fn arrange_layers(stored: &[MapLayer], connected: &[String]) -> Vec<MapLayer> {
    let mut rows: Vec<MapLayer> = Vec::new();
    for (k, from) in connected.iter().filter(|c| !stored.iter().any(|s| &s.from == *c)).enumerate() {
        rows.push(MapLayer {
            from: from.clone(),
            name: String::new(),
            theme: Default::default(),
            labels: Default::default(),
            icon: Default::default(),
            raster: Default::default(),
            show: true,
            style: crate::mapdraw::LayerStyle::default_for(stored.len() + k),
        });
    }
    rows.extend(stored.iter().filter(|s| connected.contains(&s.from)).cloned());
    rows
}

pub(super) fn run_map(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let parts: Vec<Part> = ctx.parts.iter().filter(|x| x.port == "Layers").cloned().collect();
    if parts.is_empty() {
        return err("Connect the streams to draw to Layers. Each connection is a layer.");
    }
    let connected: Vec<String> = parts.iter().map(|x| x.from.clone()).collect();
    let rows = arrange_layers(&map_layers(p.values), &connected);
    // From the bottom up: the last row is drawn first.
    let mut drawn: Vec<(&Part, crate::mapdraw::LayerStyle, &MapLayer)> = Vec::new();
    for row in rows.iter().rev().filter(|r| r.show) {
        if let Some(part) = parts.iter().find(|x| x.from == row.from) {
            drawn.push((part, row.style.clone(), row));
        }
    }
    if drawn.is_empty() {
        return err("Every layer is hidden. Tick at least one to draw.");
    }
    let width = match p.get("width") {
        None => 1600,
        Some(t) => match t.trim().parse::<u32>() {
            Ok(w) if (1..=crate::mapdraw::MAX_SIDE).contains(&w) => w,
            _ => return err(format!("The width must be a whole number of pixels from 1 to {}, not \"{}\".", crate::mapdraw::MAX_SIDE, t)),
        },
    };
    let margin = match p.get("margin") {
        None => 0.05,
        Some(t) => match parse_number(&t) {
            Some(m) if (0.0..=100.0).contains(&m) => m / 100.0,
            _ => return err(format!("The margin must be a share in percent from 0 to 100, not \"{}\".", t)),
        },
    };
    let background = crate::mapdraw::parse_color(&p.get("background").unwrap_or_else(|| "#ffffff".into())).map_err(GbError)?;
    // The coordinate system of the map: given, or that of the bottom layer that has one.
    let crs = match p.crs("crs")? {
        Some(c) => c,
        None => match drawn.iter().find_map(|(part, _, _)| part.table.crs) {
            Some(c) => c,
            None => return err("None of the layers has a coordinate system, so there is nothing to place on a map."),
        },
    };
    let mut tables: Vec<(std::borrow::Cow<Table>, crate::mapdraw::LayerStyle)> = Vec::new();
    let mut rows_drawn: Vec<(&Part, &MapLayer)> = Vec::new();
    for (part, style, row) in drawn {
        let t = part.table.as_ref();
        if !t.has_geometry() {
            ctx.notes.warn(format!("{} has no geometry, so it is not drawn.", part.label));
            continue;
        }
        if t.crs.is_some_and(|c| c != crs) {
            ctx.notes.info(format!("{} was reprojected to {}", part.label, crs_label(Some(crs))));
            tables.push((std::borrow::Cow::Owned(reproject(t, crs, ctx.backend)?), style));
        } else {
            tables.push((std::borrow::Cow::Borrowed(t), style));
        }
        rows_drawn.push((part, row));
    }
    // What each layer shows of its values, and the legend from the top layer down.
    let mut themes = Vec::new();
    for ((t, style), (part, row)) in tables.iter().zip(&rows_drawn) {
        let th = crate::thematic::prepare(t, style, &row.theme).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?;
        themes.push(th);
    }
    // How the layers that carry a raster draw it.
    let mut rasters = Vec::new();
    for ((t, _), (part, row)) in tables.iter().zip(&rows_drawn) {
        rasters.push(crate::rasterlook::prepare(t, &row.raster).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?);
    }
    // The labels and the icons of each layer.
    let mut labels = Vec::new();
    let mut icons = Vec::new();
    for ((t, _), (part, row)) in tables.iter().zip(&rows_drawn) {
        labels.push(prepare_labels(t, &row.labels, ctx).map_err(|e| GbError(format!("{}: labels: {}", part.label, e.0)))?);
        let ic = prepare_icons(t, &row.icon);
        if let Some(ic) = &ic {
            let missing = t
                .features
                .iter()
                .zip(&ic.names)
                .filter(|(f, n)| n.is_none() && f.geom.as_ref().is_some_and(|g| matches!(g, geo::Geometry::Point(_) | geo::Geometry::MultiPoint(_))))
                .count();
            if missing > 0 {
                let what = if row.icon.by.is_empty() {
                    format!("\"{}\" is not an icon", row.icon.name)
                } else {
                    format!("{} has values that are not icons", row.icon.by)
                };
                ctx.notes.warn(format!("{}: {}, so {} points are drawn as circles.", part.label, what, fmt_count(missing as u64)));
            }
        }
        icons.push(ic);
    }
    let legend = match p.get("legend").as_deref() {
        None | Some("none") => None,
        Some(corner) => Some(crate::mapdraw::Legend {
            corner: corner.to_string(),
            title: p.get("legend_title").unwrap_or_default(),
            items: tables
                .iter()
                .zip(&rows_drawn)
                .zip(&themes)
                .zip(&rasters)
                .rev()
                .map(|((((t, style), (part, row)), th), ra)| crate::mapdraw::LegendItem {
                    name: if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() },
                    // A raster without keys, such as relief or a picture, gets a grey square.
                    style: match ra {
                        Some(r) if r.keys.is_empty() => crate::mapdraw::LayerStyle { fill: Some([0xb4, 0xb2, 0xac, 255]), stroke: None, ..style.clone() },
                        _ => style.clone(),
                    },
                    shape: if ra.is_some() { "area" } else { crate::mapdraw::shape_of(t) },
                    heading: th.heading.clone(),
                    keys: match ra {
                        Some(r) => r.keys.clone(),
                        None => th.keys.clone(),
                    },
                    size_heading: th.size_heading.clone(),
                    size_keys: th.size_keys.clone(),
                    icon: (!row.icon.name.trim().is_empty() && row.icon.by.trim().is_empty()).then(|| crate::icons::icon_for(&row.icon.name)).flatten(),
                    icon_keys: icon_keys(t, &row.icon),
                })
                .collect(),
            icon_styles: rows_drawn
                .iter()
                .map(|(part, row)| {
                    (
                        if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() },
                        if row.icon.style.is_empty() { "plain".into() } else { row.icon.style.clone() },
                    )
                })
                .collect(),
        }),
    };
    let count: usize = tables.iter().map(|(t, _)| t.len()).sum();
    let placed = labels.iter().flatten().map(|l| l.items.len()).sum::<usize>();
    let req = crate::mapdraw::Request {
        layers: tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect(),
        width,
        margin,
        background,
        themes,
        labels,
        icons,
        rasters,
        legend,
    };
    let gate = ctx.gate;
    let image = crate::mapdraw::draw(&req, &|| gate.going())?;
    let layers = req.layers.len();
    if placed > 0 {
        ctx.notes.info(format!("{} labels to place", fmt_count(placed as u64)));
    }
    ctx.notes.info(format!(
        "drew {} {} in {} {}, {} × {} pixels",
        fmt_count(count as u64),
        if count == 1 { "feature" } else { "features" },
        layers,
        if layers == 1 { "layer" } else { "layers" },
        fmt_count(image.width as u64),
        fmt_count(image.height as u64)
    ));
    Ok(vec![("Map".into(), image.into_table(Some(crs)))])
}

/// The labels of a layer: the text, size and priority of each feature that has a text.
fn prepare_labels(t: &Table, spec: &crate::mapdraw::LabelSpec, ctx: &NodeCtx) -> Result<Option<crate::mapdraw::Labels>> {
    let src = spec.text.trim();
    if src.is_empty() {
        return Ok(None);
    }
    let names = t.field_names();
    // An attribute by its name, also one with spaces in it; else an expression.
    let field = t.field_index(src);
    let compiled = match field {
        Some(_) => None,
        None => Some(crate::expr::compile(src, &names)?),
    };
    let attr = |name: &str| -> Result<Option<usize>> {
        if name.trim().is_empty() {
            return Ok(None);
        }
        t.field_index(name.trim()).map(Some).ok_or_else(|| GbError(format!("there is no attribute {}", name.trim())))
    };
    let (size_at, prio_at) = (attr(&spec.size_by)?, attr(&spec.priority_by)?);
    let xy = if spec.place == "xy" {
        match (attr(&spec.x_attr)?, attr(&spec.y_attr)?) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => return err("choose the attributes with x and y of the labels"),
        }
    } else {
        None
    };
    let number = |i: usize, k: usize| t.features[i].attrs[k].as_f64();
    // The range of the values that set the size.
    let (lo, hi) = match size_at {
        Some(k) => (0..t.len()).filter_map(|i| number(i, k)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v))),
        None => (0.0, 0.0),
    };
    let geographic = ctx.geographic(t.crs);
    let mut items = Vec::new();
    for (i, f) in t.features.iter().enumerate() {
        let v = match (field, &compiled) {
            (Some(k), _) => f.attrs[k].clone(),
            (None, Some(e)) => e.eval(&crate::expr::Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?,
            _ => continue,
        };
        let mut text = v.to_text().trim().to_string();
        if text.is_empty() {
            continue;
        }
        if spec.upper {
            text = text.to_uppercase();
        }
        let size = match (size_at.and_then(|k| number(i, k)), hi > lo) {
            (Some(v), true) => spec.size + (spec.size_max - spec.size) * ((v - lo) / (hi - lo)) as f32,
            _ => spec.size,
        };
        let at = match xy {
            Some((x, y)) => match (number(i, x), number(i, y)) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => continue,
            },
            None => None,
        };
        let priority = prio_at.and_then(|k| number(i, k)).unwrap_or(f64::NEG_INFINITY).max(-1e300) + if size_at.is_some() { size as f64 * 1e-9 } else { 0.0 };
        items.push(crate::mapdraw::LabelItem { feature: i, text, size, priority, at });
    }
    Ok(Some(crate::mapdraw::Labels { spec: spec.clone(), items }))
}

/// The icon of each feature of a layer, from the name given or from an attribute.
fn prepare_icons(t: &Table, spec: &crate::mapdraw::IconSpec) -> Option<crate::mapdraw::Icons> {
    if spec.is_off() {
        return None;
    }
    let style = if spec.style.is_empty() { "plain".to_string() } else { spec.style.clone() };
    let names = match t.field_index(spec.by.trim()).filter(|_| !spec.by.trim().is_empty()) {
        Some(k) => t.features.iter().map(|f| crate::icons::icon_for(&f.attrs[k].to_text())).collect(),
        None => {
            let one = crate::icons::icon_for(&spec.name);
            vec![one; t.len()]
        }
    };
    Some(crate::mapdraw::Icons { style, names })
}

/// The values of the attribute that names the icons, each with its icon, for the legend: at
/// most twenty, in the order they come.
fn icon_keys(t: &Table, spec: &crate::mapdraw::IconSpec) -> Vec<(String, &'static str)> {
    let Some(k) = t.field_index(spec.by.trim()).filter(|_| !spec.by.trim().is_empty()) else { return Vec::new() };
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for f in &t.features {
        let v = f.attrs[k].to_text();
        if let Some(icon) = crate::icons::icon_for(&v) {
            if !out.iter().any(|(w, _)| *w == v) && out.len() < 20 {
                out.push((v, icon));
            }
        }
    }
    out
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "MapRenderer",
        title: "Draw map",
        hidden: false,
        group: Group::Transformer,
        category: "Maps",
        desc: "Draws the streams connected to Layers into a picture with coordinates: each stream a layer with its own colours, in the order you set, with values shown by colour and size, labels, icons for points, and a legend. Write the picture to PNG, JPG, GeoTIFF or PDF with an output.",
        inputs: &["Layers"],
        outputs: &["Map"],
        ports: None,
        params: MAP_PARAMS,
        run: run_map,
        summary: |p| format!("{} px wide", p.get("width").unwrap_or_else(|| "1600".into())),
    },
];
