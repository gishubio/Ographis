//! Draw map: the streams as layers of a picture, with a legend and values shown by colour and size.

use super::geom_transform::number_param;
use super::*;

// ---------- Draw map (MapRenderer) ----------

pub(super) const MAP_BACKGROUNDS: &[(&str, &str)] = &[
    ("#ffffff", "White"),
    ("none", "See-through"),
    ("#f4f1ea", "Paper"),
    ("#1e2326", "Dark"),
    ("colour", "A colour of my own"),
    ("map", "A background map, such as OpenStreetMap"),
    ("picture", "A picture"),
];

/// The background maps to choose from: those built in, and the user's own by their names
/// written in the field.
pub(super) const BASEMAPS: &[(&str, &str)] = &[
    ("osm", "OpenStreetMap"),
    ("carto_light", "Light grey (CARTO Positron)"),
    ("carto_voyager", "Soft colours (CARTO Voyager)"),
    ("carto_dark", "Dark (CARTO Dark Matter)"),
    ("own", "One of my own, or an address"),
];

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
    ParamSpec {
        key: "size_as",
        label: "Size",
        kind: ParamKind::Choice(&[("pixels", "In pixels, for the screen"), ("print", "For print: width in mm and pixels to the inch")]),
        default: "pixels",
        hint: "",
    },
    ParamSpec { key: "width", label: "Width in pixels", kind: ParamKind::Text, default: "1600", hint: "The height follows from the area that is drawn" },
    ParamSpec {
        key: "print_mm",
        label: "Width in mm",
        kind: ParamKind::Text,
        default: "180",
        hint: "On paper, such as 180 for an A4 page with margins. The height follows from the area that is drawn",
    },
    ParamSpec {
        key: "print_dpi",
        label: "Pixels to the inch",
        kind: ParamKind::Text,
        default: "300",
        hint: "300 for print, 150 for a draft. Lines, points and labels keep the size they have on the screen, measured on paper, and the picture tells Output its resolution",
    },
    ParamSpec {
        key: "map_style",
        label: "Style",
        kind: ParamKind::Choice(crate::mapdraw::MAP_STYLES),
        default: "",
        hint: "A ready look for the whole map: the colours of the layers are made to fit it, each layer keeping its own hue, and a background of a colour gets the paper of the style",
    },
    ParamSpec { key: "margin", label: "Margin in %", kind: ParamKind::Text, default: "5", hint: "Room around the features, as a share of the larger side" },
    ParamSpec { key: "background", label: "Background", kind: ParamKind::Choice(MAP_BACKGROUNDS), default: "#ffffff", hint: "" },
    ParamSpec { key: "background_colour", label: "Colour", kind: ParamKind::Text, default: "#e8eef2", hint: "Such as #e8eef2, or with opacity as #e8eef280" },
    ParamSpec {
        key: "basemap",
        label: "Background map",
        kind: ParamKind::Choice(BASEMAPS),
        default: "osm",
        hint: "Fetched from the web as tiles, and kept for a month",
    },
    ParamSpec {
        key: "basemap_own",
        label: "Name or address",
        kind: ParamKind::Text,
        default: "",
        hint: "The name of one of your background maps under Settings, or an address with {z}, {x} and {y}",
    },
    ParamSpec {
        key: "background_picture",
        label: "Picture",
        kind: ParamKind::FileOpen,
        default: "",
        hint: "PNG or JPG. With a world file beside it (.pgw, .jgw) it lies in its place; otherwise it covers the whole map",
    },
    ParamSpec {
        key: "background_crs",
        label: "Picture in",
        kind: ParamKind::Crs,
        default: "",
        hint: "The coordinate system of the world file. Empty: that of the map",
    },
    ParamSpec {
        key: "background_fade",
        label: "Paler by %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Makes the background map or picture paler, so that the layers stand out: 30 to 50 suits most maps",
    },
    ParamSpec {
        key: "legend",
        label: "Legend",
        kind: ParamKind::Choice(LEGEND_PLACES),
        default: "none",
        hint: "A box with the layers and what their colours and sizes stand for",
    },
    ParamSpec { key: "legend_title", label: "Title of the legend", kind: ParamKind::Text, default: "", hint: "" },
    ParamSpec {
        key: "scale_bar",
        label: "Scale bar",
        kind: ParamKind::Choice(LEGEND_PLACES),
        default: "none",
        hint: "A round length of about a fifth of the map, in metres or km",
    },
    ParamSpec { key: "north_arrow", label: "North arrow", kind: ParamKind::Choice(LEGEND_PLACES), default: "none", hint: "" },
    ParamSpec {
        key: "grid",
        label: "Grid",
        kind: ParamKind::Choice(&[("none", "None"), ("lines", "Lines across the map"), ("ticks", "Marks in the edge")]),
        default: "none",
        hint: "With the coordinates in the edge",
    },
    ParamSpec {
        key: "grid_every",
        label: "Grid every",
        kind: ParamKind::Text,
        default: "",
        hint: "In the units of the map, such as 1000 for every km in SWEREF 99 TM. Empty gives a round number by the size",
    },
    ParamSpec {
        key: "overview",
        label: "Overview map",
        kind: ParamKind::Choice(LEGEND_PLACES),
        default: "none",
        hint: "A small map in a corner that shows where the map lies, with the map as a red frame: what is connected to Overview, such as the outline of the county, or else the lines and areas of the map seen from further away",
    },
    ParamSpec {
        key: "overview_size",
        label: "Overview width in %",
        kind: ParamKind::Text,
        default: "25",
        hint: "Of the width of the map",
    },
    ParamSpec {
        key: "overview_zoom",
        label: "Overview reaches",
        kind: ParamKind::Text,
        default: "5",
        hint: "How many times wider than the map, when nothing is connected to Overview",
    },
    ParamSpec {
        key: "source",
        label: "Source",
        kind: ParamKind::Text,
        default: "",
        hint: "Written small in the lower right corner, such as © Lantmäteriet, after the credit of the background map",
    },
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
            if let Some(d) = r.get("dash").and_then(|v| v.as_str()) {
                style.dash = d.to_string();
            }
            if let Some(Ok(c)) = color("casing") {
                style.casing = c;
            }
            if let Some(w) = r.get("casing_width").and_then(|v| v.as_f64()) {
                style.casing_width = w.clamp(0.0, 50.0) as f32;
            }
            if let Some(p) = r.get("pattern").and_then(|v| v.as_str()) {
                style.pattern = p.to_string();
            }
            if let Some(Ok(c)) = color("pattern_colour") {
                style.pattern_colour = c;
            }
            if let Some(w) = r.get("pattern_every").and_then(|v| v.as_f64()) {
                style.pattern_every = w.clamp(2.0, 200.0) as f32;
            }
            // One opacity for the layer, whether it is drawn as features or as a raster.
            if let Some(o) = r.get("opacity").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(crate::model::parse_number))) {
                style.opacity = (o / 100.0).clamp(0.0, 1.0) as f32;
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
                arrow_by: text("arrow_by"),
                arrow_from: r.get("arrow_from").and_then(|v| v.as_bool()).unwrap_or(false),
                heat: num("heat", 0.0).max(0.0) as f32,
                heat_by: text("heat_by"),
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
                spaced: flag("label_spaced"),
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
                if !r.style.dash.is_empty() {
                    o["dash"] = serde_json::json!(r.style.dash);
                }
                if r.style.casing.is_some() {
                    o["casing"] = serde_json::json!(crate::mapdraw::color_text(r.style.casing));
                    o["casing_width"] = serde_json::json!(r.style.casing_width);
                }
                if !r.style.pattern.is_empty() {
                    o["pattern"] = serde_json::json!(r.style.pattern);
                    o["pattern_every"] = serde_json::json!(r.style.pattern_every);
                    if r.style.pattern_colour.is_some() {
                        o["pattern_colour"] = serde_json::json!(crate::mapdraw::color_text(r.style.pattern_colour));
                    }
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
                if !t.arrow_by.is_empty() {
                    put("arrow_by", serde_json::Value::String(t.arrow_by.clone()));
                    if t.arrow_from {
                        put("arrow_from", serde_json::json!(true));
                    }
                }
                if t.heat > 0.0 {
                    put("heat", serde_json::json!(t.heat));
                    if !t.heat_by.is_empty() {
                        put("heat_by", serde_json::Value::String(t.heat_by.clone()));
                    }
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
                    for (k, v, dv) in
                        [("label_all", l.all, d.all), ("label_bold", l.bold, d.bold), ("label_upper", l.upper, d.upper), ("label_spaced", l.spaced, d.spaced)]
                    {
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
                // The opacity of the layer, as the form has it for features or a raster.
                let opacity = ra.opacity.min(r.style.opacity);
                if opacity < 1.0 {
                    o["opacity"] = serde_json::json!((opacity * 100.0).round());
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

pub(super) fn run_map(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
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
    // For print, the width on paper and the resolution give the pixels, and what is drawn
    // in pixels on the screen, at 96 to the inch, is made as large on paper.
    let print = p.get("size_as").as_deref() == Some("print");
    let (width, dpi) = if print {
        let mm = number_param(p, "print_mm", "Width in mm")?.unwrap_or(180.0);
        let dpi = number_param(p, "print_dpi", "Pixels to the inch")?.unwrap_or(300.0);
        if !(1.0..=10_000.0).contains(&dpi) {
            return err("Pixels to the inch is a number from 1 to 10 000, such as 300.");
        }
        let w = (mm / 25.4 * dpi).round();
        if w < 1.0 || w > crate::mapdraw::MAX_SIDE as f64 {
            return err(format!(
                "{} mm at {} to the inch is {} pixels, and a map can be at most {} wide. Make it narrower or lower the resolution.",
                crate::model::fmt_real(mm),
                crate::model::fmt_real(dpi),
                fmt_count(w.max(0.0) as u64),
                fmt_count(crate::mapdraw::MAX_SIDE as u64)
            ));
        }
        (w as u32, Some(dpi))
    } else {
        let w = match p.get("width") {
            None => 1600,
            Some(t) => match t.trim().parse::<u32>() {
                Ok(w) if (1..=crate::mapdraw::MAX_SIDE).contains(&w) => w,
                _ => return err(format!("The width must be a whole number of pixels from 1 to {}, not \"{}\".", crate::mapdraw::MAX_SIDE, t)),
            },
        };
        (w, None)
    };
    let scale = dpi.map_or(1.0, |d| (d / 96.0) as f32);
    let margin = match p.get("margin") {
        None => 0.05,
        Some(t) => match parse_number(&t) {
            Some(m) if (0.0..=100.0).contains(&m) => m / 100.0,
            _ => return err(format!("The margin must be a share in percent from 0 to 100, not \"{}\".", t)),
        },
    };
    let chosen = p.get("background").unwrap_or_else(|| "#ffffff".into());
    let background = match chosen.as_str() {
        "colour" => crate::mapdraw::parse_color(&p.get("background_colour").unwrap_or_else(|| "#e8eef2".into()))
            .map_err(|e| GbError(format!("The colour of the background: {}", e)))?,
        // Under a map or a picture: white where they do not reach.
        "map" | "picture" => Some([255, 255, 255, 255]),
        c => crate::mapdraw::parse_color(c).map_err(GbError)?,
    };
    // A ready style gives a background of a colour its paper.
    let map_style = p.get("map_style").unwrap_or_default();
    let background = match (chosen.as_str(), crate::mapdraw::style_paper(&map_style)) {
        ("map" | "picture" | "none", _) | (_, None) => background,
        (_, Some(paper)) => Some(paper),
    };
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
    if !map_style.is_empty() {
        for (_, style) in tables.iter_mut() {
            *style = style.in_style(&map_style);
        }
    }
    // What each layer shows of its values, and the legend from the top layer down.
    let mut themes = Vec::new();
    for ((t, style), (part, row)) in tables.iter().zip(&rows_drawn) {
        let th = crate::thematic::prepare(t, style, &row.theme).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?;
        themes.push(th);
    }
    // The colours of values in the style too.
    if !map_style.is_empty() {
        let restyle = |l: &mut crate::thematic::Look| {
            l.fill = l.fill.map(|c| crate::mapdraw::style_colour(&map_style, c, crate::mapdraw::Role::Area));
            l.stroke = l.stroke.map(|c| crate::mapdraw::style_colour(&map_style, c, crate::mapdraw::Role::Line));
        };
        for th in themes.iter_mut() {
            th.classes.iter_mut().for_each(|(_, l)| restyle(l));
            if let Some(ls) = &mut th.looks {
                ls.iter_mut().for_each(restyle);
            }
            th.keys.iter_mut().for_each(|k| restyle(&mut k.look));
            th.size_keys.iter_mut().for_each(|k| restyle(&mut k.look));
        }
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
    if !map_style.is_empty() {
        for lb in labels.iter_mut().flatten() {
            lb.spec.colour = crate::mapdraw::style_colour(&map_style, lb.spec.colour, crate::mapdraw::Role::Text);
            lb.spec.halo = lb.spec.halo.map(|c| crate::mapdraw::style_colour(&map_style, c, crate::mapdraw::Role::Halo));
        }
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
                    shape: if ra.is_some() {
                        "area"
                    } else if th.heat > 0.0 {
                        "heat"
                    } else if th.angles.is_some() {
                        "arrow"
                    } else {
                        crate::mapdraw::shape_of(t)
                    },
                    heading: th.heading.clone(),
                    keys: match ra {
                        Some(r) => r.keys.clone(),
                        None => th.keys.clone(),
                    },
                    size_heading: th.size_heading.clone(),
                    size_keys: th.size_keys.clone(),
                    icon: (!row.icon.name.trim().is_empty() && row.icon.by.trim().is_empty()).then(|| crate::icons::icon_for(&row.icon.name)).flatten(),
                    icon_keys: icon_keys(t, &row.icon),
                    heat_ramp: (th.heat > 0.0).then(|| th.heat_ramp.clone()),
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
    // For print, every size in pixels is made larger by as much as the pixels are smaller.
    // The legend and the credit of the background map grow with the picture already.
    if scale != 1.0 {
        let look = |l: &mut crate::thematic::Look| l.size *= scale;
        for (_, style) in tables.iter_mut() {
            style.width *= scale;
            style.size *= scale;
        }
        for th in themes.iter_mut() {
            th.classes.iter_mut().for_each(|(_, l)| look(l));
            th.looks.iter_mut().flatten().for_each(look);
            th.keys.iter_mut().for_each(|k| look(&mut k.look));
            th.size_keys.iter_mut().for_each(|k| look(&mut k.look));
            th.cluster *= scale;
            th.heat *= scale;
        }
        for l in labels.iter_mut().flatten() {
            l.spec.size *= scale;
            l.spec.size_max *= scale;
            l.spec.halo_width *= scale;
            l.items.iter_mut().for_each(|i| i.size *= scale);
        }
    }
    let count: usize = tables.iter().map(|(t, _)| t.len()).sum();
    let placed = labels.iter().flatten().map(|l| l.items.len()).sum::<usize>();
    // Under the layers: a background map or a picture, made as large as the map.
    let (backdrop, attribution) = match chosen.as_str() {
        "map" | "picture" => {
            let layers: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
            let frame = crate::mapdraw::frame(&layers, width, margin)?;
            let fade = (number_param(p, "background_fade", "Paler by %")?.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0) as f32;
            if chosen == "map" {
                let which = p.get("basemap").unwrap_or_else(|| "osm".into());
                let name = if which == "own" {
                    p.need("basemap_own", "Write the name of one of your background maps, or an address with {z}, {x} and {y}.")?
                } else {
                    which
                };
                let map = crate::basemap::find(&name)?;
                let b = ctx.backend("A background map")?;
                let gate = ctx.gate;
                match crate::basemap::tiles(&map, &frame, crs, b, fade, &|| gate.going()) {
                    Ok((img, n, missing)) => {
                        ctx.notes.info(format!("{} tiles of {} under the layers", n, map.name));
                        if missing > 0 {
                            ctx.notes.warn(format!("{} tiles of {} could not be fetched, and are left empty.", missing, map.name));
                        }
                        let credit = if map.attribution.trim().is_empty() { None } else { Some(map.attribution.clone()) };
                        (Some(img), credit)
                    }
                    Err(e) if e.0 != crate::model::STOPPED => {
                        // Without the internet the map is drawn all the same, without its background.
                        ctx.notes.warn(format!("{}{} The map is drawn without it.", e.0, if e.0.ends_with('.') { "" } else { "." }));
                        (None, None)
                    }
                    Err(e) => return Err(e),
                }
            } else {
                let file = p.need("background_picture", "Choose the picture to have under the layers.")?;
                let path = ctx.resolve(&file);
                let (img, placed) = crate::basemap::picture(&path, &frame, crs, p.crs("background_crs")?, ctx.backend, fade)?;
                ctx.notes.info(if placed {
                    "the picture lies under the layers where its world file says"
                } else {
                    "the picture covers the map; with a world file it would lie in its place"
                });
                (Some(img), None)
            }
        }
        _ => (None, None),
    };
    // Who the data is from, after who the background map is from.
    let attribution = match (attribution, p.get("source").map(|s| s.trim().to_string()).filter(|s| !s.is_empty())) {
        (Some(a), Some(s)) => Some(format!("{} · {}", a, s)),
        (a, s) => a.or(s),
    };
    let furniture = crate::mapdraw::Furniture {
        scale_bar: p.get("scale_bar").unwrap_or_default(),
        north: p.get("north_arrow").unwrap_or_default(),
        grid: p.get("grid").unwrap_or_default(),
        grid_every: number_param(p, "grid_every", "Grid every")?,
        geographic: ctx.geographic(Some(crs)).unwrap_or(false),
    };
    let names = rows_drawn.iter().map(|(part, row)| if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() }).collect();
    // The overview map: its own layer when one is connected, else the map seen from further.
    let overview = match p.get("overview").as_deref() {
        None | Some("none") | Some("") => None,
        Some(corner) => {
            let share = (number_param(p, "overview_size", "Overview width in %")?.unwrap_or(25.0) / 100.0) as f32;
            let zoom = number_param(p, "overview_zoom", "Overview reaches")?.unwrap_or(5.0);
            let mut layers = Vec::new();
            if let Some(t) = inp.get("Overview").filter(|t| t.has_geometry()) {
                let t = match t.crs {
                    Some(c) if c != crs => reproject(t, crs, ctx.backend)?,
                    _ => (**t).clone(),
                };
                let style =
                    crate::mapdraw::LayerStyle { fill: Some([0xe4, 0xe2, 0xdc, 255]), stroke: Some([0x77, 0x77, 0x77, 255]), width: 0.8, ..Default::default() };
                layers.push((t, style.in_style(&map_style)));
            }
            ctx.notes.info(if layers.is_empty() {
                format!("an overview map in a corner, {} times wider than the map", crate::model::fmt_real(zoom))
            } else {
                "an overview map in a corner, of what is connected to Overview".to_string()
            });
            let mut ov = crate::mapdraw::Overview { corner: corner.to_string(), share, zoom, layers, backdrop: None };
            // The background map of the map, fetched for the overview too.
            if chosen == "map" && ov.layers.is_empty() {
                let drawn: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
                let f = crate::mapdraw::frame(&drawn, width, margin)?;
                let main = [f.b[0], f.b[1], f.b[0] + f.width as f64 * f.px, f.b[1] + f.height as f64 * f.px];
                let ext = ov.extent(main);
                let (bw, bh) = ov.size(f.width, f.height, ext);
                let px = (ext[2] - ext[0]) / bw as f64;
                let small = crate::mapdraw::Frame { b: [ext[0], ext[3] - bh as f64 * px, ext[2], ext[3]], px, width: bw, height: bh };
                let which = p.get("basemap").unwrap_or_else(|| "osm".into());
                let name = if which == "own" { p.get("basemap_own").unwrap_or_default() } else { which };
                let fade = (number_param(p, "background_fade", "Paler by %")?.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0) as f32;
                if let (Ok(map), Some(b)) = (crate::basemap::find(&name), ctx.backend) {
                    let gate = ctx.gate;
                    match crate::basemap::tiles(&map, &small, crs, b, fade, &|| gate.going()) {
                        Ok((img, _, _)) => ov.backdrop = Some(img),
                        Err(e) if e.0 == crate::model::STOPPED => return Err(e),
                        Err(e) => ctx.notes.warn(format!("The overview map is drawn without its background map: {}", e.0)),
                    }
                }
            }
            Some(ov)
        }
    };
    let req = crate::mapdraw::Request {
        overview,
        map_style: map_style.clone(),
        furniture,
        names,
        layers: tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect(),
        width,
        margin,
        background,
        backdrop,
        attribution,
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
    let mut out = image.into_table(Some(crs));
    // The resolution goes with the picture, for Output to write into the file.
    if let Some(d) = dpi {
        out.fields.push(crate::model::Field::new("dpi", crate::model::FieldType::Real));
        out.features.iter_mut().for_each(|f| f.attrs.push(Value::Real(d)));
        ctx.notes.info(format!("{} mm wide at {} pixels to the inch", crate::model::fmt_real(width as f64 / d * 25.4), crate::model::fmt_real(d)));
    }
    Ok(vec![("Map".into(), out)])
}

/// The labels of a layer: the text, size and priority of each feature that has a text.
pub(in crate::nodes) fn prepare_labels(t: &Table, spec: &crate::mapdraw::LabelSpec, ctx: &NodeCtx) -> Result<Option<crate::mapdraw::Labels>> {
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
        inputs: &["Layers", "Overview"],
        outputs: &["Map"],
        ports: None,
        params: MAP_PARAMS,
        run: run_map,
        summary: |p| {
            if p.get("size_as").as_deref() == Some("print") {
                format!("{} mm at {} dpi", p.get("print_mm").unwrap_or_else(|| "180".into()), p.get("print_dpi").unwrap_or_else(|| "300".into()))
            } else {
                format!("{} px wide", p.get("width").unwrap_or_else(|| "1600".into()))
            }
        },
    },
];
