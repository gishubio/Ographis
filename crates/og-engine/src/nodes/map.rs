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

pub(super) const LEGEND_PLACES: &[(&str, &str)] = &[
    ("none", "None"),
    ("bottom-left", "Lower left"),
    ("bottom-center", "Lower middle"),
    ("bottom-right", "Lower right"),
    ("top-left", "Upper left"),
    ("top-center", "Upper middle"),
    ("top-right", "Upper right"),
];

const CORNERS: &[(&str, &str)] = &[
    ("bottom-left", "Lower left"),
    ("bottom-center", "Lower middle"),
    ("bottom-right", "Lower right"),
    ("top-left", "Upper left"),
    ("top-center", "Upper middle"),
    ("top-right", "Upper right"),
];

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
        kind: ParamKind::Choice(&[
            ("pixels", "In pixels, for the screen"),
            ("print", "For print: width in mm and pixels to the inch"),
            ("scale", "At a scale on a paper, such as 1:50 000 on A3"),
        ]),
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
        key: "map_scale",
        label: "Scale 1:",
        kind: ParamKind::Text,
        default: "50000",
        hint: "Such as 50000 for 1:50 000. The map is as large as the paper less 10 mm on every side, centred on the features, in a coordinate system in metres",
    },
    ParamSpec {
        key: "paper",
        label: "Paper",
        kind: ParamKind::Choice(&[
            ("a4l", "A4 landscape"),
            ("a4p", "A4 portrait"),
            ("a3l", "A3 landscape"),
            ("a3p", "A3 portrait"),
            ("a2l", "A2 landscape"),
            ("a2p", "A2 portrait"),
            ("a1l", "A1 landscape"),
            ("a1p", "A1 portrait"),
        ]),
        default: "a3l",
        hint: "",
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
        hint: "PNG, JPG or WebP. With a world file beside it (.pgw, .jgw, .wpw) it lies in its place; otherwise it covers the whole map",
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
    ParamSpec { key: "legend_columns", label: "Legend columns", kind: ParamKind::Text, default: "1", hint: "Its rows in this many columns, for a long legend" },
    ParamSpec { key: "legend_fill", label: "Legend box", kind: ParamKind::Text, default: "", hint: "The colour of its box, such as #ffffffe6, or none. Empty: white, a little see-through" },
    ParamSpec { key: "legend_line", label: "Legend edge", kind: ParamKind::Text, default: "", hint: "The colour of the edge of its box, or none. Empty: light grey" },
    ParamSpec {
        key: "scale_bar",
        label: "Scale bar",
        kind: ParamKind::Choice(LEGEND_PLACES),
        default: "none",
        hint: "A round length of about a fifth of the map, in metres or km",
    },
    ParamSpec { key: "north_arrow", label: "North arrow", kind: ParamKind::Choice(LEGEND_PLACES), default: "none", hint: "" },
    ParamSpec {
        key: "scale_style",
        label: "Scale bar as",
        kind: ParamKind::Choice(&[("bar", "Fields dark and light"), ("line", "A line with ticks")]),
        default: "bar",
        hint: "",
    },
    ParamSpec { key: "scale_parts", label: "Fields of the scale bar", kind: ParamKind::Text, default: "4", hint: "From 1 to 10" },
    ParamSpec { key: "scale_unit", label: "Scale bar in", kind: ParamKind::Choice(&[("auto", "As fits"), ("km", "Kilometres"), ("m", "Metres")]), default: "auto", hint: "" },
    ParamSpec {
        key: "scale_number",
        label: "Scale in figures",
        kind: ParamKind::Choice(&[("bar", "The bar alone"), ("both", "Under the bar, as 1:50 000"), ("number", "In place of the bar")]),
        default: "bar",
        hint: "",
    },
    ParamSpec {
        key: "north_style",
        label: "North arrow as",
        kind: ParamKind::Choice(&[("arrow", "An arrow, half dark"), ("simple", "A thin arrow"), ("star", "A compass star")]),
        default: "arrow",
        hint: "",
    },
    ParamSpec {
        key: "grid",
        label: "Grid",
        kind: ParamKind::Choice(&[
            ("none", "None"),
            ("lines", "Lines across the map"),
            ("ticks", "Marks in the edge"),
            ("degrees", "Latitude and longitude, in degrees"),
            ("utm", "UTM, with the 100 km squares of MGRS"),
            ("sheets", "Map sheets of 10 km in SWEREF 99 TM, with their names"),
        ]),
        default: "none",
        hint: "With the coordinates in the edge: those of the map, such as SWEREF 99 TM, or degrees of WGS 84, whose lines curve on a map in metres",
    },
    ParamSpec {
        key: "grid_every",
        label: "Grid every",
        kind: ParamKind::Text,
        default: "",
        hint: "In the units of the map, such as 1000 for every km in SWEREF 99 TM, or in degrees for a grid of degrees. Empty gives a round number by the size",
    },
    ParamSpec {
        key: "index_grid",
        label: "Index squares across",
        kind: ParamKind::Text,
        default: "",
        hint: "Squares named A, B, C across and 1, 2, 3 down, as on a town map, this many across; Register then gives the square of each name. Empty: none",
    },
    ParamSpec {
        key: "vision",
        label: "Colours as seen by",
        kind: ParamKind::Choice(&[
            ("", "Everyone: as they are"),
            ("deuteranopia", "Deuteranopia: green-blind, about 1 man in 20"),
            ("protanopia", "Protanopia: red-blind, about 1 man in 50"),
            ("tritanopia", "Tritanopia: blue-blind, rare"),
            ("grey", "Grey, as printed in black and white"),
        ]),
        default: "",
        hint: "To try a map: the picture in the colours that someone with colour blindness sees, or in grey. A PDF of it is a picture",
    },
    ParamSpec {
        key: "check_contrast",
        label: "Look at the labels",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "Warns of labels without a halo that stand out too little from what is under them, less than 3 to 1 as WCAG counts contrast",
    },
    ParamSpec {
        key: "map_frame",
        label: "Frame",
        kind: ParamKind::Choice(&[("none", "None"), ("frame", "A frame, with the coordinates outside it")]),
        default: "none",
        hint: "A line around the map with a white margin, where the coordinates of the grid are written, as on a printed map",
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
    ParamSpec { key: "overview_fill", label: "Overview areas", kind: ParamKind::Text, default: "", hint: "A colour for the areas of the overview map, such as #e4e2dc. Empty: as the layers have it" },
    ParamSpec { key: "overview_line", label: "Overview lines", kind: ParamKind::Text, default: "", hint: "A colour for its lines and outlines, such as #777777. Empty: as the layers have it" },
    ParamSpec { key: "overview_frame", label: "Overview frame", kind: ParamKind::Text, default: "#444444", hint: "The colour of the frame around it" },
    ParamSpec { key: "overview_mark", label: "Where the map is", kind: ParamKind::Text, default: "#d73027", hint: "The colour of the box in the overview map that shows where the map is" },
    ParamSpec { key: "title", label: "Title", kind: ParamKind::Text, default: "", hint: "Written large over the map, where Title in the says" },
    ParamSpec { key: "subtitle", label: "Subtitle", kind: ParamKind::Text, default: "", hint: "Under the title: what is shown, where and when" },
    ParamSpec { key: "logo", label: "Logo", kind: ParamKind::FileOpen, default: "", hint: "A picture, such as a PNG with a see-through background" },
    ParamSpec { key: "title_place", label: "Title in the", kind: ParamKind::Choice(CORNERS), default: "top-left", hint: "Where the title and the subtitle go" },
    ParamSpec {
        key: "title_fill",
        label: "Box behind the title",
        kind: ParamKind::Text,
        default: "",
        hint: "A colour for a box behind the title and the subtitle, such as #ffffff, or #ffffffcc to let a little show through. Empty: no box",
    },
    ParamSpec { key: "title_line", label: "Edge of the box", kind: ParamKind::Text, default: "", hint: "A colour for the edge of the box, such as #888888. Empty: no edge" },
    ParamSpec { key: "title_font", label: "Font", kind: ParamKind::FileOpen, default: "", hint: "A font file, such as a TTF or an OTF from C:\\Windows\\Fonts. Empty: the built-in one" },
    ParamSpec { key: "title_size", label: "Title size", kind: ParamKind::Text, default: "26", hint: "In pixels on a map 800 pixels high: larger maps have it larger" },
    ParamSpec { key: "title_colour", label: "Title colour", kind: ParamKind::Text, default: "#111111", hint: "" },
    ParamSpec {
        key: "title_bold",
        label: "Title in bold",
        kind: ParamKind::Choice(&[("yes", "Yes"), ("no", "No")]),
        default: "yes",
        hint: "With a font file of one's own, the letters are as the file has them",
    },
    ParamSpec { key: "subtitle_size", label: "Subtitle size", kind: ParamKind::Text, default: "15", hint: "" },
    ParamSpec { key: "subtitle_colour", label: "Subtitle colour", kind: ParamKind::Text, default: "#444444", hint: "" },
    ParamSpec { key: "logo_corner", label: "Logo in the corner", kind: ParamKind::Choice(CORNERS), default: "bottom-right", hint: "" },
    ParamSpec { key: "logo_size", label: "Logo width in %", kind: ParamKind::Text, default: "12", hint: "Of the width of the map" },
    ParamSpec {
        key: "inset",
        label: "Inset of",
        kind: ParamKind::Text,
        default: "",
        hint: "An enlarged map in a corner of the features this expression picks out, such as kommunnamn = 'Göteborg', with a frame on the map where it is. Empty: none",
    },
    ParamSpec { key: "inset_corner", label: "Inset in the corner", kind: ParamKind::Choice(CORNERS), default: "top-right", hint: "" },
    ParamSpec {
        key: "grid_frame",
        label: "Frame along the degrees",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "With a grid of degrees: the map cut along its outermost lines of latitude and longitude, so that its edge curves with them, as on a map of a large area",
    },
    ParamSpec {
        key: "globe",
        label: "Globe in the corner",
        kind: ParamKind::Choice(&[
            ("none", "None"),
            ("bottom-left", "Lower left"),
            ("bottom-right", "Lower right"),
            ("top-left", "Upper left"),
            ("top-right", "Upper right"),
        ]),
        default: "none",
        hint: "A small globe turned to the map, with its place marked in red",
    },
    ParamSpec {
        key: "profile_from",
        label: "Height profile of",
        kind: ParamKind::Text,
        default: "",
        hint: "The name of a layer of lines, such as a trail: the heights along its first line, from the first grid of heights in the map, drawn as a profile in a corner. Empty: none",
    },
    ParamSpec {
        key: "profile_corner",
        label: "Profile in the corner",
        kind: ParamKind::Choice(&[("bottom-left", "Lower left"), ("bottom-right", "Lower right"), ("top-left", "Upper left"), ("top-right", "Upper right")]),
        default: "bottom-left",
        hint: "",
    },
    ParamSpec {
        key: "profile_title",
        label: "Title of the profile",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as Depth along the dashed line, A to B. Empty: the name of the layer",
    },
    ParamSpec {
        key: "profile_places",
        label: "Places along the profile",
        kind: ParamKind::Text,
        default: "",
        hint: "The name of a layer of points, such as basins and straits: those near the line are written over the profile where they are along it, by their attribute name or the first with text",
    },
    ParamSpec { key: "profile_size", label: "Profile width in %", kind: ParamKind::Text, default: "34", hint: "Of the width of the map" },
    ParamSpec { key: "inset_size", label: "Inset width in %", kind: ParamKind::Text, default: "30", hint: "Of the width of the map" },
    ParamSpec {
        key: "knockout",
        label: "Lines break under labels",
        kind: ParamKind::Choice(&[("no", "No"), ("yes", "Yes")]),
        default: "no",
        hint: "Roads, borders and outlines stop where a label goes, so that the names are free",
    },
    ParamSpec {
        key: "template",
        label: "Template",
        kind: ParamKind::FileOpen,
        default: "",
        hint: "A map template, .ogmap, saved from another map: what this map does not set itself is taken from it, so that maps look alike",
    },
    ParamSpec {
        key: "notes",
        label: "Notes on the map",
        kind: ParamKind::Text,
        default: "",
        hint: "One to a line: east, north in the coordinates of the map, and the text, as 323000 6402000 New bridge 2027. Each points to its place with a line",
    },
    ParamSpec {
        key: "map_font",
        label: "Font of the map",
        kind: ParamKind::FileOpen,
        default: "",
        hint: "A TTF or an OTF for all the text of the map: labels, legend, scale bar and the rest. Empty: the built-in one",
    },
    ParamSpec {
        key: "multiples_by",
        label: "A map for each value of",
        kind: ParamKind::Text,
        default: "",
        hint: "An attribute, such as year: the map is drawn once for each of its values, side by side at one scale, with one legend. Empty: one map",
    },
    ParamSpec { key: "multiples_columns", label: "Maps across", kind: ParamKind::Text, default: "", hint: "Empty: as many as make a square" },
    ParamSpec {
        key: "atlas_by",
        label: "A page for each feature, named by",
        kind: ParamKind::Text,
        default: "",
        hint: "An attribute of a layer, such as kommunnamn: a map of each feature of it, centred on it and named by it, as pages of one PDF or a file each. Empty: one map",
    },
    ParamSpec {
        key: "focus",
        label: "The map is about",
        kind: ParamKind::Text,
        default: "",
        hint: "An expression that picks out the areas the map is about, such as lan = 'Västra Götaland': the rest is paler, and the areas can have a shadow or lines along their coast. Empty: all of it",
    },
    ParamSpec {
        key: "focus_rest",
        label: "The rest",
        kind: ParamKind::Choice(&[("fade", "Paler"), ("hide", "Left out")]),
        default: "fade",
        hint: "Paler, or left out so that only the areas are drawn, in their own shape",
    },
    ParamSpec { key: "focus_fade", label: "The rest paler by %", kind: ParamKind::Text, default: "60", hint: "0 keeps it as it is" },
    ParamSpec {
        key: "focus_effect",
        label: "Around the areas",
        kind: ParamKind::Choice(&[("none", "Nothing"), ("shadow", "A soft shadow"), ("waterlines", "Lines along the coast")]),
        default: "none",
        hint: "Under the layers, around the areas the map is about",
    },
    ParamSpec {
        key: "simplify",
        label: "Simplify to the scale",
        kind: ParamKind::Choice(&[("no", "No"), ("yes", "Yes")]),
        default: "no",
        hint: "Points of lines and outlines closer than a pixel are left out: calmer lines and smaller files, with no change that can be seen",
    },
    ParamSpec {
        key: "source",
        label: "Source",
        kind: ParamKind::Text,
        default: "",
        hint: "Written small in the lower right corner, such as © Lantmäteriet, after the credit of the background map",
    },
    ParamSpec {
        key: "footer",
        label: "Band along the bottom",
        kind: ParamKind::Text,
        default: "",
        hint: "Written on the left of a band across the bottom of the map, such as Data: EMODnet Bathymetry · NOAA ETOPO, with the source on the right of it. Empty: no band",
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
    /// Small charts on its features.
    pub chart: crate::mapdraw::ChartSpec,
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
            if let Some(a) = r.get("arrows").and_then(|v| v.as_str()) {
                style.arrows = a.to_string();
            }
            if let Some(w) = r.get("arrow_size").and_then(|v| v.as_f64()) {
                style.arrow_size = w.clamp(1.0, 10.0) as f32;
            }
            if let Some(w) = r.get("gap").and_then(|v| v.as_f64()) {
                style.gap = w.clamp(0.0, 100.0) as f32;
            }
            if let Some(w) = r.get("scale_min").and_then(|v| v.as_f64()) {
                style.scale_min = w.max(0.0);
            }
            if let Some(w) = r.get("scale_max").and_then(|v| v.as_f64()) {
                style.scale_max = w.max(0.0);
            }
            if let Some(b) = r.get("blend").and_then(|v| v.as_str()) {
                style.blend = b.to_string();
            }
            style.smooth = r.get("smooth").and_then(|v| v.as_bool()).unwrap_or(false);
            style.no_legend = r.get("legend").and_then(|v| v.as_bool()) == Some(false);
            if let Some(g) = r.get("glow").and_then(|v| v.as_str()) {
                style.glow = g.to_string();
            }
            if let Some(c) = r.get("glow_colour").and_then(|v| v.as_str()) {
                style.glow_colour = crate::mapdraw::parse_color(c).ok().flatten();
            }
            if let Some(a) = r.get("index_by").and_then(|v| v.as_str()) {
                style.index_by = a.to_string();
            }
            style.index_every = r.get("index_every").and_then(|v| v.as_f64()).unwrap_or(0.0).max(0.0);
            style.band = r.get("band").and_then(|v| v.as_f64()).unwrap_or(0.0).clamp(0.0, 60.0) as f32;
            style.waterlines = r.get("waterlines").and_then(|v| v.as_bool()).unwrap_or(false);
            style.tanaka = r.get("tanaka").and_then(|v| v.as_bool()).unwrap_or(false);
            style.taper = r.get("taper").and_then(|v| v.as_bool()).unwrap_or(false);
            let word = |k: &str| r.get(k).and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
            style.width_by = word("width_by");
            style.alpha_by = word("alpha_by");
            style.bridge_where = word("bridge_where");
            style.tunnel_where = word("tunnel_where");
            style.oneway_where = word("oneway_where");
            style.shield = word("shield");
            if let Some(w) = r.get("shield_every").and_then(|v| v.as_f64()) {
                style.shield_every = w.clamp(60.0, 5000.0) as f32;
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
            let flag_of = |k: &str| r.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
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
                reverse: r.get("reverse").and_then(|v| v.as_bool()).unwrap_or(false),
                round_breaks: flag_of("round_breaks"),
                color_by2: text("color_by2"),
                dots_by: text("dots_by"),
                dots_per: num("dots_per", 0.0).max(0.0),
                nested: flag_of("nested"),
                histogram: flag_of("histogram"),
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
                upper_where: text("label_upper_where"),
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
                small_caps: flag("label_small_caps"),
                uphill: flag("label_uphill"),
                coast: flag("label_coast"),
                detail: flag("label_detail"),
                tracking: num("label_tracking", 0.0).clamp(0.0, 2.0) as f32,
                italic: flag("label_italic"),
                bold_where: text("label_bold_where"),
                min_px: num("label_min_px", 0.0).clamp(0.0, 2000.0) as f32,
                repeat: num("label_repeat", 0.0).clamp(0.0, 20000.0) as f32,
                leaders: flag("label_leaders"),
                curved: flag("label_curved"),
                rank: num("label_rank", 0.0).clamp(-100.0, 100.0) as i32,
                prefer: text("label_prefer"),
            };
            let icon = crate::mapdraw::IconSpec { name: text("icon"), by: text("icon_by"), style: text("icon_style") };
            let chart = crate::mapdraw::ChartSpec {
                kind: text("chart"),
                of: match r.get("chart_of") {
                    Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
                    Some(serde_json::Value::String(t)) => t.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                    _ => Vec::new(),
                },
                size: num("chart_size", 40.0).clamp(4.0, 500.0) as f32,
                size_by: text("chart_size_by"),
            };
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
            Some(MapLayer { from, name, show, style, theme, labels, icon, chart, raster })
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
                if !r.style.arrows.is_empty() {
                    o["arrows"] = serde_json::json!(r.style.arrows);
                    o["arrow_size"] = serde_json::json!(r.style.arrow_size);
                }
                if r.style.gap > 0.0 {
                    o["gap"] = serde_json::json!(r.style.gap);
                }
                if r.style.scale_min > 0.0 {
                    o["scale_min"] = serde_json::json!(r.style.scale_min);
                }
                if r.style.scale_max > 0.0 {
                    o["scale_max"] = serde_json::json!(r.style.scale_max);
                }
                if !r.style.blend.is_empty() {
                    o["blend"] = serde_json::json!(r.style.blend);
                }
                if r.style.no_legend {
                    o["legend"] = serde_json::Value::Bool(false);
                }
                if r.style.smooth {
                    o["smooth"] = serde_json::json!(true);
                }
                if !r.style.index_by.is_empty() {
                    o["index_by"] = serde_json::json!(r.style.index_by);
                    o["index_every"] = serde_json::json!(r.style.index_every);
                }
                if r.style.band > 0.0 {
                    o["band"] = serde_json::json!(r.style.band);
                }
                if r.style.waterlines {
                    o["waterlines"] = serde_json::json!(true);
                }
                if r.style.tanaka {
                    o["tanaka"] = serde_json::json!(true);
                }
                if r.style.taper {
                    o["taper"] = serde_json::json!(true);
                }
                for (k, v) in [
                    ("width_by", &r.style.width_by),
                    ("alpha_by", &r.style.alpha_by),
                    ("bridge_where", &r.style.bridge_where),
                    ("tunnel_where", &r.style.tunnel_where),
                    ("oneway_where", &r.style.oneway_where),
                    ("shield", &r.style.shield),
                ] {
                    if !v.trim().is_empty() {
                        o[k] = serde_json::json!(v);
                    }
                }
                if !r.style.shield.trim().is_empty() && r.style.shield_every != 260.0 {
                    o["shield_every"] = serde_json::json!(r.style.shield_every);
                }
                if !r.style.glow.is_empty() {
                    o["glow"] = serde_json::json!(r.style.glow);
                    if r.style.glow_colour.is_some() {
                        o["glow_colour"] = serde_json::json!(crate::mapdraw::color_text(r.style.glow_colour));
                    }
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
                    ("color_by2", &t.color_by2),
                    ("dots_by", &t.dots_by),
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
                if t.reverse {
                    put("reverse", serde_json::json!(true));
                }
                for (k, on) in [("round_breaks", t.round_breaks), ("nested", t.nested), ("histogram", t.histogram)] {
                    if on {
                        put(k, serde_json::json!(true));
                    }
                }
                if !t.dots_by.is_empty() && t.dots_per > 0.0 {
                    put("dots_per", serde_json::json!(t.dots_per));
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
                        ("label_bold_where", &l.bold_where, &d.bold_where),
                        ("label_upper_where", &l.upper_where, &d.upper_where),
                    ] {
                        if v != dv {
                            put(k, serde_json::json!(v));
                        }
                    }
                    for (k, v, dv) in [
                        ("label_all", l.all, d.all),
                        ("label_bold", l.bold, d.bold),
                        ("label_upper", l.upper, d.upper),
                        ("label_spaced", l.spaced, d.spaced),
                        ("label_italic", l.italic, d.italic),
                        ("label_leaders", l.leaders, d.leaders),
                        ("label_curved", l.curved, d.curved),
                        ("label_small_caps", l.small_caps, d.small_caps),
                        ("label_uphill", l.uphill, d.uphill),
                        ("label_coast", l.coast, d.coast),
                        ("label_detail", l.detail, d.detail),
                    ] {
                        if v != dv {
                            put(k, serde_json::json!(v));
                        }
                    }
                    for (k, v, dv) in [
                        ("label_size", l.size, d.size),
                        ("label_size_max", l.size_max, d.size_max),
                        ("label_halo_width", l.halo_width, d.halo_width),
                        ("label_min_px", l.min_px, d.min_px),
                        ("label_repeat", l.repeat, d.repeat),
                        ("label_tracking", l.tracking, d.tracking),
                    ] {
                        if v != dv {
                            put(k, serde_json::json!(v));
                        }
                    }
                    if l.rank != 0 {
                        put("label_rank", serde_json::json!(l.rank));
                    }
                    if !l.prefer.is_empty() {
                        put("label_prefer", serde_json::json!(l.prefer));
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
                if !r.chart.kind.is_empty() {
                    o["chart"] = serde_json::json!(r.chart.kind);
                    o["chart_of"] = serde_json::json!(r.chart.of);
                    o["chart_size"] = serde_json::json!(r.chart.size);
                    if !r.chart.size_by.is_empty() {
                        o["chart_size_by"] = serde_json::json!(r.chart.size_by);
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
            chart: Default::default(),
            raster: Default::default(),
            show: true,
            style: crate::mapdraw::LayerStyle::default_for(stored.len() + k),
        });
    }
    rows.extend(stored.iter().filter(|s| connected.contains(&s.from)).cloned());
    rows
}

pub(super) fn run_map(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    // A template gives what the map does not set itself.
    if let Some(path) = p.get("template").filter(|t| !t.trim().is_empty()) {
        let file = ctx.resolve(&path);
        let text = std::fs::read_to_string(&file).map_err(|e| GbError(format!("The template {} cannot be read: {}", file.display(), e)))?;
        let Ok(serde_json::Value::Object(template)) = serde_json::from_str::<serde_json::Value>(&text) else {
            return err(format!("{} is not a template of a map.", file.display()));
        };
        let mut merged = p.values.clone();
        for (k, v) in template {
            let unset = merged.get(&k).is_none_or(|own| own.as_str().is_some_and(|s| s.trim().is_empty()));
            if unset && k != "template" && k != "layers" {
                merged.insert(k, v);
            }
        }
        let q = P::new(&merged, MAP_PARAMS);
        return draw_map_node(&q, inp, ctx);
    }
    draw_map_node(p, inp, ctx)
}

/// The settings of a map that make its look, to keep as a template for other maps: all but
/// its layers, what it shows and where, and what it is about.
pub fn map_template(params: &Params) -> Params {
    const OWN: &[&str] = &["layers", "template", "title", "subtitle", "notes", "focus", "inset", "multiples_by", "atlas_by", "map_scale", "crs", "source"];
    params.iter().filter(|(k, _)| !OWN.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn draw_map_node(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
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
    let at_scale = p.get("size_as").as_deref() == Some("scale");
    // At a scale on a paper: the paper less 10 mm on every side, in mm across and down.
    let paper_mm = if at_scale {
        let (w, h) = match p.get("paper").as_deref().unwrap_or("a3l") {
            "a4p" => (210.0, 297.0),
            "a4l" => (297.0, 210.0),
            "a3p" => (297.0, 420.0),
            "a2l" => (594.0, 420.0),
            "a2p" => (420.0, 594.0),
            "a1l" => (841.0, 594.0),
            "a1p" => (594.0, 841.0),
            _ => (420.0, 297.0),
        };
        Some((w - 20.0, h - 20.0))
    } else {
        None
    };
    let (width, dpi) = if print || at_scale {
        let mm = match paper_mm {
            Some((w, _)) => w,
            None => number_param(p, "print_mm", "Width in mm")?.unwrap_or(180.0),
        };
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
    if at_scale && ctx.geographic(Some(crs)).unwrap_or(false) {
        return err("A map at a scale needs a coordinate system in metres, such as SWEREF 99 TM: choose one under Coordinate system.");
    }
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
    // Drawn by hand: long straight lines get points along them, so that they waver too.
    if p.get("map_style").as_deref() == Some("hand") {
        use geo::{BoundingRect, Densify, Euclidean};
        for (t, _) in tables.iter_mut() {
            if t.grid.is_some() || t.image.is_some() {
                continue;
            }
            let Some(b) = t
                .features
                .iter()
                .filter_map(|f| f.geom.as_ref()?.bounding_rect())
                .reduce(|a, b| geo::Rect::new((a.min().x.min(b.min().x), a.min().y.min(b.min().y)), (a.max().x.max(b.max().x), a.max().y.max(b.max().y))))
            else {
                continue;
            };
            let step = b.width().max(b.height()) / 400.0;
            if step <= 0.0 {
                continue;
            }
            let mut owned = t.as_ref().clone();
            for f in owned.features.iter_mut() {
                f.geom = f.geom.take().map(|g| match g {
                    geo::Geometry::LineString(l) => geo::Geometry::LineString(Euclidean.densify(&l, step)),
                    geo::Geometry::MultiLineString(l) => geo::Geometry::MultiLineString(Euclidean.densify(&l, step)),
                    geo::Geometry::Polygon(p) => geo::Geometry::Polygon(Euclidean.densify(&p, step)),
                    geo::Geometry::MultiPolygon(p) => geo::Geometry::MultiPolygon(Euclidean.densify(&p, step)),
                    g => g,
                });
            }
            *t = std::borrow::Cow::Owned(owned);
        }
    }
    // Smooth layers have their corners rounded off, twice.
    for (t, style) in tables.iter_mut() {
        if style.smooth {
            let mut owned = t.as_ref().clone();
            for f in owned.features.iter_mut() {
                if let Some(g) = f.geom.take() {
                    f.geom = Some(smoothed(g));
                }
            }
            *t = std::borrow::Cow::Owned(owned);
        }
    }
    if !map_style.is_empty() {
        for (_, style) in tables.iter_mut() {
            *style = style.in_style(&map_style);
        }
    }
    // At a scale, the area drawn: the paper at that scale, centred on the features.
    let fixed = match paper_mm {
        Some((w_mm, h_mm)) => {
            let n = number_param(p, "map_scale", "Scale")?.unwrap_or(50_000.0);
            if !(1.0..=100_000_000.0).contains(&n) {
                return err("The scale is a number such as 50000 for 1:50 000.");
            }
            let layers: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
            let Some(e) = crate::mapdraw::extent_of(&layers) else { return err("There is nothing to draw: the layers have no geometry.") };
            let (cx, cy) = ((e[0] + e[2]) / 2.0, (e[1] + e[3]) / 2.0);
            let (hw, hh) = (w_mm / 1000.0 * n / 2.0, h_mm / 1000.0 * n / 2.0);
            if e[2] - e[0] > 2.0 * hw || e[3] - e[1] > 2.0 * hh {
                ctx.notes.warn(format!(
                    "At 1:{} the paper shows {} × {} km, and the features reach further: the map shows the middle of them.",
                    crate::chart::number(n, 0, true).replace('\u{a0}', " "),
                    crate::model::fmt_real((hw * 2.0 / 100.0).round() / 10.0),
                    crate::model::fmt_real((hh * 2.0 / 100.0).round() / 10.0)
                ));
            }
            ctx.notes.info(format!("at 1:{} on {} × {} mm", crate::chart::number(n, 0, true).replace('\u{a0}', " "), w_mm, h_mm));
            Some([cx - hw, cy - hh, cx + hw, cy + hh])
        }
        None => None,
    };
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
    // Colours that one man in twelve cannot tell apart, in the legend of a layer.
    for (th, (part, row)) in themes.iter().zip(&rows_drawn) {
        let keyed: Vec<(&str, [u8; 3])> = th.keys.iter().filter_map(|k| k.look.fill.or(k.look.stroke).map(|c| (k.text.as_str(), [c[0], c[1], c[2]]))).collect();
        let colours: Vec<[u8; 3]> = keyed.iter().map(|k| k.1).collect();
        if let Some((i, j, kind)) = crate::colourvision::confused(&colours) {
            let name = if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() };
            ctx.notes.warn(format!(
                "{}: {} and {} have colours that are hard to tell apart for those who are {}, about one man in twelve. A ramp from light to dark, such as Blues or Yellow to blue, can be read by all.",
                name, keyed[i].0, keyed[j].0, kind
            ));
        }
    }
    // Bridges, tunnels and one-way streets, found by their expressions.
    for ((t, style), (part, _)) in tables.iter_mut().zip(&rows_drawn) {
        let wheres = [
            (1u8, style.bridge_where.clone(), "Bridges where"),
            (2, style.tunnel_where.clone(), "Tunnels where"),
            (4, style.oneway_where.clone(), "One way where"),
        ];
        if wheres.iter().all(|w| w.1.trim().is_empty()) {
            continue;
        }
        let names: Vec<String> = t.fields.iter().map(|f| f.name.clone()).collect();
        let geographic = ctx.geographic(t.crs);
        let mut marks = vec![0u8; t.features.len()];
        for (bit, w, what) in wheres {
            if w.trim().is_empty() {
                continue;
            }
            let e = crate::expr::compile(w.trim(), &names).map_err(|e| GbError(format!("{}: {}: {}", part.label, what, e.0)))?;
            for (i, f) in t.features.iter().enumerate() {
                if e.test(&crate::expr::Row { feature: f, index: i, geographic }).unwrap_or(false) {
                    marks[i] |= bit;
                }
            }
        }
        style.marks = std::sync::Arc::new(marks);
    }
    // How the layers that carry a raster draw it.
    let mut rasters = Vec::new();
    for ((t, _), (part, row)) in tables.iter().zip(&rows_drawn) {
        rasters.push(crate::rasterlook::prepare(t, &row.raster).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?);
    }
    // The labels and the icons of each layer.
    let mut labels = Vec::new();
    let mut icons = Vec::new();
    for ((t, style), (part, row)) in tables.iter().zip(&rows_drawn) {
        let mut l = prepare_labels(t, &row.labels, ctx).map_err(|e| GbError(format!("{}: labels: {}", part.label, e.0)))?;
        // Contours: only the index contours are named.
        if let (Some(l), Some(k)) = (l.as_mut(), t.field_index(&style.index_by)) {
            if style.index_every > 0.0 {
                l.items.retain(|it| crate::mapdraw::is_index(&t.features[it.feature].attrs[k], style.index_every));
            }
        }
        labels.push(l);
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
    let mut charts = Vec::new();
    for ((t, _), (part, row)) in tables.iter().zip(&rows_drawn) {
        charts.push(prepare_charts(t, &row.chart).map_err(|e| GbError(format!("{}: charts: {}", part.label, e.0)))?);
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
            fill: box_colour(p, "legend_fill", "Legend box")?,
            line: box_colour(p, "legend_line", "Legend edge")?,
            columns: number_param(p, "legend_columns", "Legend columns")?.map_or(1, |c| c.clamp(1.0, 6.0) as usize),
            items: tables
                .iter()
                .zip(&rows_drawn)
                .zip(&themes)
                .zip(&rasters)
                .rev()
                .filter(|((((_, style), _), _), _)| !style.no_legend)
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
                    } else if th.sized_lines {
                        "flow"
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
                    chart_keys: if row.chart.kind.is_empty() { Vec::new() } else { row.chart.of.iter().cloned().zip(chart_colours()).collect() },
                    heat_ramp: (th.heat > 0.0).then(|| th.heat_ramp.clone()),
                    counts: th.counts.clone(),
                    bivariate: th.bivariate.clone(),
                    nested: th.nested,
                    dots: th
                        .dots
                        .as_ref()
                        .map(|_| (crate::thematic::Look { fill: Some(th.dot_colour), stroke: None, size: th.dot_size.max(3.0) }, th.dot_text.clone())),
                    ramp: ra.as_ref().and_then(|r| crate::rasterlook::ramp_legend(&r.paint)),
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
            style.gap *= scale;
        }
        for th in themes.iter_mut() {
            th.classes.iter_mut().for_each(|(_, l)| look(l));
            th.looks.iter_mut().flatten().for_each(look);
            th.keys.iter_mut().for_each(|k| look(&mut k.look));
            th.size_keys.iter_mut().for_each(|k| look(&mut k.look));
            th.cluster *= scale;
            th.heat *= scale;
            th.dot_size *= scale;
        }
        for c in charts.iter_mut().flatten() {
            c.sizes.iter_mut().for_each(|s| *s *= scale);
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
            let frame = crate::mapdraw::frame_in(&layers, width, margin, fixed)?;
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
    // For print: what will not look well on paper.
    if dpi.is_some() {
        let layers: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
        let frame = crate::mapdraw::frame_in(&layers, width, margin, fixed)?;
        for ((t, style), (part, _)) in tables.iter().zip(&rows_drawn) {
            let cell = t.image.as_ref().map(|i| i.geotransform[1]).or_else(|| t.grid.as_ref().map(|g| g.geotransform[1])).unwrap_or(0.0).abs();
            if cell > 0.0 && cell / frame.px > 2.5 {
                ctx.notes.warn(format!(
                    "{}: each cell becomes {} pixels on paper, so it looks blocky in print. A finer raster, a smaller map or a lower resolution shows it as it is.",
                    part.label,
                    (cell / frame.px).round()
                ));
            }
            // Lines thinner than a tenth of a millimetre are lost in print. The widths are
            // in pixels at the resolution of the paper by now.
            let mm = style.width as f64 / dpi.unwrap_or(96.0) * 25.4;
            if style.stroke.is_some() && style.width > 0.0 && mm < 0.1 {
                ctx.notes.warn(format!(
                    "{}: lines {} mm wide are hard to see on paper. Make them at least 0.1 mm, 0.4 pixels on the screen.",
                    part.label,
                    crate::model::fmt_real((mm * 100.0).round() / 100.0)
                ));
            }
        }
    }
    let mut furniture = crate::mapdraw::Furniture {
        footer: p.get("footer").unwrap_or_default(),
        scale_bar: p.get("scale_bar").unwrap_or_default(),
        north: p.get("north_arrow").unwrap_or_default(),
        grid: p.get("grid").unwrap_or_default(),
        grid_every: number_param(p, "grid_every", "Grid every")?,
        geographic: ctx.geographic(Some(crs)).unwrap_or(false),
        graticule: if p.get("grid").as_deref() == Some("degrees") {
            let layers: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
            let frame = crate::mapdraw::frame_in(&layers, width, margin, fixed)?;
            graticule(&frame, crs, number_param(p, "grid_every", "Grid every")?, ctx)?
        } else {
            Vec::new()
        },
        knockout: p.get("knockout").as_deref() == Some("yes"),
        extent: fixed,
        title: p.exact("title").trim().to_string(),
        subtitle: p.exact("subtitle").trim().to_string(),
        title_place: p.get("title_place").unwrap_or_else(|| "top-left".into()),
        title_fill: colour_of(p, "title_fill", "Box behind the title")?,
        title_line: colour_of(p, "title_line", "Edge of the box")?,
        title_font: p.get("title_font").filter(|f| !f.trim().is_empty()).map(|f| ctx.resolve(&f).to_string_lossy().into_owned()).unwrap_or_default(),
        title_size: number_param(p, "title_size", "Title size")?.unwrap_or(26.0).clamp(4.0, 200.0) as f32,
        subtitle_size: number_param(p, "subtitle_size", "Subtitle size")?.unwrap_or(15.0).clamp(4.0, 200.0) as f32,
        title_colour: colour_of(p, "title_colour", "Title colour")?,
        subtitle_colour: colour_of(p, "subtitle_colour", "Subtitle colour")?,
        title_regular: p.get("title_bold").as_deref() == Some("no"),
        logo: match p.get("logo").filter(|l| !l.trim().is_empty()) {
            Some(path) => {
                let size = number_param(p, "logo_size", "Logo width")?.unwrap_or(12.0);
                Some(logo(&ctx.resolve(&path), width, size, dpi.is_some(), ctx)?)
            }
            None => None,
        },
        logo_corner: p.get("logo_corner").unwrap_or_else(|| "bottom-right".into()),
        inset: match p.get("inset").filter(|e| !e.trim().is_empty()) {
            Some(e) => Some(crate::mapdraw::Inset {
                area: inset_area(&tables, &e, ctx)?,
                corner: p.get("inset_corner").unwrap_or_else(|| "top-right".into()),
                share: (number_param(p, "inset_size", "Inset width")?.unwrap_or(30.0) / 100.0) as f32,
            }),
            None => None,
        },
        simplify: p.get("simplify").as_deref() == Some("yes"),
        dpi: dpi.unwrap_or(0.0) as f32,
        focus: match p.get("focus").filter(|e| !e.trim().is_empty()) {
            Some(e) => {
                let found = matching(&tables, &e, ctx)?;
                if found.is_empty() {
                    ctx.notes.warn(format!("No area matches {}, so the whole map is as it is.", e.trim()));
                }
                found
            }
            None => Vec::new(),
        },
        focus_fade: (number_param(p, "focus_fade", "The rest paler by %")?.unwrap_or(60.0).clamp(0.0, 100.0) / 100.0) as f32,
        focus_effect: p.get("focus_effect").filter(|e| e != "none").unwrap_or_default(),
        focus_only: p.get("focus_rest").as_deref() == Some("hide"),
        frame: p.get("map_frame").as_deref() == Some("frame"),
        map_font: p.get("map_font").filter(|f| !f.trim().is_empty()).map(|f| ctx.resolve(&f).to_string_lossy().into_owned()).unwrap_or_default(),
        notes: notes_of(&p.exact("notes"))?,
        scale_style: p.get("scale_style").unwrap_or_default(),
        scale_parts: number_param(p, "scale_parts", "Fields of the scale bar")?.map_or(4, |n| n.clamp(1.0, 10.0) as usize),
        scale_unit: p.get("scale_unit").unwrap_or_default(),
        north_style: p.get("north_style").unwrap_or_default(),
        scale_number: p.get("scale_number").unwrap_or_default(),
        index_grid: number_param(p, "index_grid", "Index squares across")?.map_or(0, |n| n.clamp(0.0, 52.0) as usize),
        vision: p.get("vision").unwrap_or_default(),
        profile: None,
        globe: None,
        cell_names: Vec::new(),
        check_contrast: p.flag("check_contrast"),
        faint: Default::default(),
        coords_outside: false,
    };
    let names: Vec<String> =
        rows_drawn.iter().map(|(part, row)| if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() }).collect();
    {
        // The frame of the map as it will be drawn, for the grids, the globe and the curved edge.
        let layers: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
        let frame = crate::mapdraw::frame_in(&layers, width, margin, fixed)?;
        match p.get("grid").as_deref() {
            Some("utm") => furniture.graticule = utm_grid(&frame, crs, number_param(p, "grid_every", "Grid every")?, ctx)?,
            Some("sheets") => {
                let (lines, names) = sheet_grid(&frame, crs, ctx)?;
                furniture.graticule = lines;
                furniture.cell_names = names;
            }
            Some("degrees") if p.flag("grid_frame") && furniture.focus.is_empty() => {
                // Within the features themselves, so that the curved edge cuts through them.
                let inner = if fixed.is_some() { frame } else { crate::mapdraw::frame_in(&layers, width, 0.0, None)? };
                if let Some(edge) = degree_frame(&inner, crs, ctx)? {
                    // The lines of the grid end at the edge, where they are named.
                    if let geo::Geometry::Polygon(poly) = &edge {
                        use geo::BooleanOps;
                        for g in furniture.graticule.iter_mut() {
                            let line = geo::MultiLineString::new(vec![geo::LineString::from(g.points.clone())]);
                            let inside = poly.clip(&line, false);
                            g.points = inside.0.into_iter().max_by_key(|l| l.0.len()).map(|l| l.0.iter().map(|c| (c.x, c.y)).collect()).unwrap_or_default();
                        }
                        furniture.graticule.retain(|g| g.points.len() > 1);
                    }
                    furniture.focus = vec![edge];
                    furniture.focus_only = true;
                }
            }
            _ => {}
        }
        if let Some(corner) = p.get("globe").filter(|c| c != "none" && !c.is_empty()) {
            furniture.globe = Some(globe_of(&frame, crs, &corner, ctx)?);
        }
        if let Some(from) = p.get("profile_from").filter(|f| !f.trim().is_empty()) {
            let from = from.trim();
            let line = tables.iter().zip(&names).find(|(_, n)| n.eq_ignore_ascii_case(from)).and_then(|((t, _), _)| {
                t.features.iter().find_map(|f| match &f.geom {
                    Some(geo::Geometry::LineString(l)) => Some(l.clone()),
                    Some(geo::Geometry::MultiLineString(m)) => m.0.first().cloned(),
                    _ => None,
                })
            });
            let at = tables.iter().position(|(t, _)| t.grid.is_some());
            let grid = at.and_then(|i| tables[i].0.grid.clone());
            match (line, grid) {
                (Some(l), Some(g)) => {
                    let points = profile_of(&l, &g, ctx.geographic(Some(crs)).unwrap_or(false));
                    let total = points.last().map_or(0.0, |p| p.0);
                    // The places near the line, each where it is along it.
                    let mut places = Vec::new();
                    if let Some(of) = p.get("profile_places").filter(|v| !v.trim().is_empty()) {
                        match tables.iter().zip(&names).find(|(_, n)| n.eq_ignore_ascii_case(of.trim())) {
                            None => ctx.notes.warn(format!("No layer named {} for the places along the profile.", of.trim())),
                            Some(((t, _), _)) => places = places_along(t, &l, total),
                        }
                    }
                    furniture.profile = Some(crate::mapdraw::Profile {
                        corner: p.get("profile_corner").unwrap_or_else(|| "bottom-left".into()),
                        points,
                        title: p.get("profile_title").map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| from.to_string()),
                        ends: [l.0.first(), l.0.last()].map(|c| c.map_or((0.0, 0.0), |c| (c.x, c.y))),
                        places,
                        colours: at.and_then(|i| rasters.get(i).cloned().flatten()).map(|r| r.paint),
                        share: (number_param(p, "profile_size", "Profile width in %")?.unwrap_or(34.0) / 100.0) as f32,
                    })
                }
                (None, _) => ctx.notes.warn(format!("No layer named {} with a line, so no height profile is drawn.", from)),
                (_, None) => ctx.notes.warn("A height profile needs a grid of heights among the layers of the map (read as a grid), so none is drawn."),
            }
        }
    }
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
            let colour = |key: &str, label: &str| -> Result<Option<[u8; 4]>> {
                match p.get(key).filter(|c| !c.trim().is_empty()) {
                    Some(c) => crate::mapdraw::parse_color(&c).map_err(|e| GbError(format!("{}: {}", label, e))),
                    None => Ok(None),
                }
            };
            let mut ov = crate::mapdraw::Overview {
                corner: corner.to_string(),
                share,
                zoom,
                layers,
                backdrop: None,
                fill: colour("overview_fill", "Overview areas")?,
                line: colour("overview_line", "Overview lines")?,
                frame: colour("overview_frame", "Overview frame")?,
                mark: colour("overview_mark", "Where the map is")?,
            };
            // The background map of the map, fetched for the overview too.
            if chosen == "map" && ov.layers.is_empty() {
                let drawn: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
                let f = crate::mapdraw::frame_in(&drawn, width, margin, fixed)?;
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
        charts,
        rasters,
        legend,
    };
    let gate = ctx.gate;
    let multiples = p.get("multiples_by").map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    let atlas = p.get("atlas_by").map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    if multiples.is_some() && atlas.is_some() {
        return err("A map for each value and a page for each feature cannot both be drawn: empty one of them.");
    }
    // The top layer that has the attribute.
    let layer_with = |attr: &str| -> Result<(usize, usize)> {
        req.layers
            .iter()
            .enumerate()
            .rev()
            .find_map(|(li, (t, _))| t.field_index(attr).map(|k| (li, k)))
            .ok_or_else(|| GbError(format!("None of the layers has an attribute {}. Choose one that the layers have.", attr)))
    };
    if (multiples.is_some() || atlas.is_some()) && matches!(chosen.as_str(), "map" | "picture") {
        ctx.notes.warn("The background map is drawn on a single map only, so these maps are drawn without it.");
    }
    let mut georeferenced = true;
    let image = if let Some(attr) = &multiples {
        let (li, k) = layer_with(attr)?;
        let t = req.layers[li].0;
        let mut values: Vec<String> = t.features.iter().map(|f| f.attrs[k].to_text()).filter(|v| !v.trim().is_empty()).collect();
        values.sort_by(|a, b| {
            let (x, y) = (parse_number(a), parse_number(b));
            match (x, y) {
                (Some(x), Some(y)) => x.total_cmp(&y),
                _ => crate::nodes::alphabetical(a).cmp(&crate::nodes::alphabetical(b)),
            }
        });
        values.dedup();
        if values.is_empty() {
            return err(format!("{} has no values to draw a map for.", attr));
        }
        if values.len() > 36 {
            ctx.notes.warn(format!("{} has {} values: the first 36 are drawn.", attr, values.len()));
            values.truncate(36);
        }
        let values: Vec<(String, Vec<bool>)> = values
            .into_iter()
            .map(|v| {
                let keep = t.features.iter().map(|f| f.attrs[k].to_text() == v).collect();
                (v, keep)
            })
            .collect();
        let columns = number_param(p, "multiples_columns", "Maps across")?.map_or(0, |c| c.clamp(1.0, 12.0) as usize);
        ctx.notes.info(format!("{} maps, one for each {}", values.len(), attr));
        georeferenced = false;
        crate::mapdraw::draw_multiples(&req, &crate::mapdraw::Multiples { layer: li, values, columns }, &|| gate.going())?
    } else if let Some(attr) = &atlas {
        let (li, k) = layer_with(attr)?;
        let t = req.layers[li].0;
        let mut pages: Vec<(String, usize)> = t.features.iter().enumerate().filter(|(_, f)| f.geom.is_some()).map(|(i, f)| (f.attrs[k].to_text(), i)).collect();
        pages.sort_by_key(|a| crate::nodes::alphabetical(&a.0));
        if pages.is_empty() {
            return err(format!("The layer with {} has no features to make pages of.", attr));
        }
        if pages.len() > 500 {
            ctx.notes.warn(format!("{} features: the first 500 get a page.", pages.len()));
            pages.truncate(500);
        }
        let mut drawn = Vec::with_capacity(pages.len());
        for (name, i) in &pages {
            if !gate.going() {
                return Err(GbError(crate::model::STOPPED.into()));
            }
            let Some(g) = &t.features[*i].geom else { continue };
            use geo::BoundingRect;
            let Some(bb) = g.bounding_rect() else { continue };
            let (cx, cy) = ((bb.min().x + bb.max().x) / 2.0, (bb.min().y + bb.max().y) / 2.0);
            // At a scale, the paper around the middle of the feature; else the feature with
            // room around it.
            let ext = match fixed {
                Some(f) => {
                    let (hw, hh) = ((f[2] - f[0]) / 2.0, (f[3] - f[1]) / 2.0);
                    [cx - hw, cy - hh, cx + hw, cy + hh]
                }
                None => {
                    let side = bb.width().max(bb.height()).max(1e-9);
                    let pad = side * margin.max(0.02) + if bb.width() == 0.0 && bb.height() == 0.0 { 500.0 } else { 0.0 };
                    [bb.min().x - pad, bb.min().y - pad, bb.max().x + pad, bb.max().y + pad]
                }
            };
            let mut furniture = req.furniture.clone();
            furniture.extent = Some(ext);
            furniture.inset = None;
            furniture.graticule = Vec::new();
            if furniture.title.is_empty() {
                furniture.title = name.clone();
            } else {
                furniture.subtitle = name.clone();
            }
            // The feature of the page stands out, unless the map says what it is about.
            if furniture.focus.is_empty() && matches!(g, geo::Geometry::Polygon(_) | geo::Geometry::MultiPolygon(_)) {
                furniture.focus = vec![g.clone()];
            }
            let page = crate::mapdraw::Request { furniture, backdrop: None, ..req.clone() };
            drawn.push((name.clone(), crate::mapdraw::draw(&page, &|| gate.going())?));
        }
        ctx.notes.info(format!("{} pages, one for each {}", drawn.len(), attr));
        let mut rest = drawn.into_iter();
        let (_, mut first) = rest.next().ok_or_else(|| GbError("No feature could be drawn as a page.".into()))?;
        let more: Vec<(String, std::sync::Arc<crate::model::RasterImage>)> = rest.map(|(n, img)| (n, std::sync::Arc::new(img))).collect();
        if let Some(v) = first.vector.take() {
            let mut pic = (*v).clone();
            pic.pages = more;
            first.vector = Some(std::sync::Arc::new(pic));
        }
        first
    } else {
        crate::mapdraw::draw(&req, &|| gate.going())?
    };
    let faint = req.furniture.faint.lock().map(|f| f.clone()).unwrap_or_default();
    if !faint.is_empty() {
        let mut names: Vec<String> = faint.iter().take(6).map(|n| format!("“{}”", n)).collect();
        if faint.len() > 6 {
            names.push(format!("and {} more", faint.len() - 6));
        }
        ctx.notes.warn(format!(
            "{} {} stand out too little from what is under them: {}. A halo, or a darker or lighter colour, makes them readable.",
            fmt_count(faint.len() as u64),
            if faint.len() == 1 { "label" } else { "labels" },
            names.join(", ")
        ));
    }
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
    let mut out = image.into_table(if georeferenced { Some(crs) } else { None });
    // The resolution goes with the picture, for Output to write into the file.
    if let Some(d) = dpi {
        out.fields.push(crate::model::Field::new("dpi", crate::model::FieldType::Real));
        out.features.iter_mut().for_each(|f| f.attrs.push(Value::Real(d)));
        ctx.notes.info(format!("{} mm wide at {} pixels to the inch", crate::model::fmt_real(width as f64 / d * 25.4), crate::model::fmt_real(d)));
    }
    // The index of names: the square each label is in.
    let mut out_all = vec![("Map".into(), out)];
    if req.furniture.index_grid > 0 {
        out_all.push(("Register".into(), register(&req)?));
    }
    Ok(out_all)
}

/// The labels of a layer: the text, size and priority of each feature that has a text.
pub(in crate::nodes) fn prepare_labels(t: &Table, spec: &crate::mapdraw::LabelSpec, ctx: &NodeCtx) -> Result<Option<crate::mapdraw::Labels>> {
    let src = spec.text.trim();
    if src.is_empty() {
        return Ok(None);
    }
    let names = t.field_names();
    // Each line of the text a line of the label: an attribute by its name, also one with
    // spaces in it, or else an expression.
    let parts: Vec<(Option<usize>, Option<crate::expr::Compiled>)> = src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| match t.field_index(l) {
            Some(k) => Ok((Some(k), None)),
            None => Ok((None, Some(crate::expr::compile(l, &names)?))),
        })
        .collect::<Result<_>>()?;
    let attr = |name: &str| -> Result<Option<usize>> {
        if name.trim().is_empty() {
            return Ok(None);
        }
        t.field_index(name.trim()).map(Some).ok_or_else(|| GbError(format!("there is no attribute {}", name.trim())))
    };
    // "@area": by the size of each area, as the names of lakes and seas are written larger the
    // larger the water.
    const BY_AREA: usize = usize::MAX;
    let size_at = if spec.size_by.trim() == "@area" { Some(BY_AREA) } else { attr(&spec.size_by)? };
    let prio_at = attr(&spec.priority_by)?;
    let xy = if spec.place == "xy" {
        match (attr(&spec.x_attr)?, attr(&spec.y_attr)?) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => return err("choose the attributes with x and y of the labels"),
        }
    } else {
        None
    };
    let number = |i: usize, k: usize| {
        if k == BY_AREA {
            use geo::Area;
            return t.features[i].geom.as_ref().map(|g| g.unsigned_area().sqrt()).filter(|a| *a > 0.0);
        }
        t.features[i].attrs[k].as_f64()
    };
    // The range of the values that set the size.
    let (lo, hi) = match size_at {
        Some(k) => (0..t.len()).filter_map(|i| number(i, k)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v))),
        None => (0.0, 0.0),
    };
    let geographic = ctx.geographic(t.crs);
    let bold_where = match spec.bold_where.trim() {
        "" => None,
        w => Some(crate::expr::compile(w, &names).map_err(|e| GbError(format!("Bold where: {}", e.0)))?),
    };
    let upper_where = match spec.upper_where.trim() {
        "" => None,
        w => Some(crate::expr::compile(w, &names).map_err(|e| GbError(format!("Capitals where: {}", e.0)))?),
    };
    let mut items = Vec::new();
    for (i, f) in t.features.iter().enumerate() {
        let mut lines = Vec::with_capacity(parts.len());
        for (field, compiled) in &parts {
            let v = match (field, compiled) {
                (Some(k), _) => f.attrs[*k].clone(),
                (None, Some(e)) => e.eval(&crate::expr::Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?,
                _ => continue,
            };
            let line = v.to_text();
            let line = line.trim();
            if !line.is_empty() {
                lines.push(line.to_string());
            }
        }
        let mut text = lines.join("\n");
        if text.is_empty() {
            continue;
        }
        let upper = match &upper_where {
            Some(e) => e.test(&crate::expr::Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?,
            None => spec.upper,
        };
        // Small capitals keep their small letters, which are set as smaller capitals.
        if upper {
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
        let bold = match &bold_where {
            Some(e) => e.test(&crate::expr::Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?,
            None => spec.bold,
        };
        items.push(crate::mapdraw::LabelItem { feature: i, text, size, priority, at, bold });
    }
    Ok(Some(crate::mapdraw::Labels { spec: spec.clone(), items }))
}

/// The lines of latitude and longitude over a map, in its coordinates: every `every` degrees,
/// or a round number of them by the size of the map.
fn graticule(frame: &crate::mapdraw::Frame, crs: u32, every: Option<f64>, ctx: &mut NodeCtx) -> Result<Vec<crate::mapdraw::GratLine>> {
    let b = frame.b;
    let (w, h) = (frame.width as f64 * frame.px, frame.height as f64 * frame.px);
    let b = [b[0], b[1], b[0] + w, b[1] + h];
    // The edges of the map in degrees, from points along them.
    let n = 24;
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for i in 0..=n {
        let f = i as f64 / n as f64;
        for (x, y) in [(b[0] + f * w, b[1]), (b[0] + f * w, b[3]), (b[0], b[1] + f * h), (b[2], b[1] + f * h)] {
            xs.push(x);
            ys.push(y);
        }
    }
    let backend = ctx.backend("A grid of degrees")?;
    if crs != 4326 {
        backend.transform(crs, 4326, &mut xs, &mut ys)?;
    }
    let fold = |v: &[f64]| v.iter().filter(|x| x.is_finite()).fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
    let (lon, lat) = (fold(&xs), fold(&ys));
    if lon.0 > lon.1 || lat.0 > lat.1 {
        return Ok(Vec::new());
    }
    let span = (lon.1 - lon.0).max(lat.1 - lat.0);
    const STEPS: &[f64] = &[0.01, 0.02, 0.05, 0.1, 0.2, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0];
    let step = every.filter(|e| *e > 0.0).unwrap_or_else(|| STEPS.iter().copied().find(|s| span / s <= 6.0).unwrap_or(30.0));
    if span / step > 200.0 {
        return err("The grid of degrees would have too many lines: set Grid every to more degrees.");
    }
    let text = |v: f64, pos: char, neg: char| {
        let decimals = if step >= 1.0 { 0 } else { (-step.log10()).ceil() as usize };
        let s = format!("{:.*}°{}", decimals, v.abs(), if v < 0.0 { neg } else { pos });
        s.replace('.', ",")
    };
    let mut lines = Vec::new();
    let pad = step;
    let samples = 60;
    let mut add = |pts: Vec<(f64, f64)>, label: String, meridian: bool| -> Result<()> {
        let (mut x, mut y): (Vec<f64>, Vec<f64>) = pts.into_iter().unzip();
        if crs != 4326 {
            backend.transform(4326, crs, &mut x, &mut y)?;
        }
        let points: Vec<(f64, f64)> = x.into_iter().zip(y).filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
        if points.len() > 1 {
            lines.push(crate::mapdraw::GratLine { points, label, meridian });
        }
        Ok(())
    };
    let (lat0, lat1) = ((lat.0 - pad).max(-89.9), (lat.1 + pad).min(89.9));
    let mut v = (lon.0 / step).ceil() * step;
    while v <= lon.1 {
        let pts = (0..=samples).map(|i| (v, lat0 + (lat1 - lat0) * i as f64 / samples as f64)).collect();
        add(pts, text(v, 'E', 'W'), true)?;
        v += step;
    }
    let (lon0, lon1) = (lon.0 - pad, lon.1 + pad);
    let mut v = (lat.0 / step).ceil() * step;
    while v <= lat.1 {
        let pts = (0..=samples).map(|i| (lon0 + (lon1 - lon0) * i as f64 / samples as f64, v)).collect();
        add(pts, text(v, 'N', 'S'), false)?;
        v += step;
    }
    Ok(lines)
}

/// The heights along a line, every so often: the distance from its start in metres, and the
/// height of the grid there.
/// The points of a layer near a line, each named and at its distance along the line, in
/// the order they come: near is within a twentieth of the length of the line.
fn places_along(t: &Table, l: &geo::LineString<f64>, total: f64) -> Vec<(f64, String)> {
    use geo::{Euclidean, Length, LineLocatePoint};
    let name_at = t.field_index("name").or_else(|| t.field_index("namn")).or_else(|| t.fields.iter().position(|f| f.ty == crate::model::FieldType::Text));
    let planar = Euclidean.length(l).max(1e-9);
    let mut out: Vec<(f64, String)> = t
        .features
        .iter()
        .filter_map(|f| {
            let p = match &f.geom {
                Some(geo::Geometry::Point(p)) => *p,
                _ => return None,
            };
            let name = name_at.map(|k| f.attrs[k].to_text()).filter(|n| !n.trim().is_empty())?;
            let frac = l.line_locate_point(&p)?;
            // How far the point is from the line, against its length.
            let near = {
                use geo::{Closest, ClosestPoint, Distance};
                match l.closest_point(&p) {
                    Closest::Intersection(c) | Closest::SinglePoint(c) => Euclidean.distance(c, p),
                    Closest::Indeterminate => f64::INFINITY,
                }
            };
            (near <= planar / 20.0).then(|| (frac * total, name.trim().to_string()))
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn profile_of(l: &geo::LineString<f64>, g: &crate::model::Grid, geographic: bool) -> Vec<(f64, f64)> {
    let dist = |a: geo::Coord<f64>, b: geo::Coord<f64>| {
        if geographic {
            use geo::Distance;
            geo::Haversine.distance(geo::Point::from(a), geo::Point::from(b))
        } else {
            (b.x - a.x).hypot(b.y - a.y)
        }
    };
    let total: f64 = l.0.windows(2).map(|w| dist(w[0], w[1])).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let n = 300;
    let mut out = Vec::with_capacity(n + 1);
    let (mut seg, mut run) = (0usize, 0.0f64);
    for i in 0..=n {
        let want = total * i as f64 / n as f64;
        while seg + 1 < l.0.len() - 1 && run + dist(l.0[seg], l.0[seg + 1]) < want {
            run += dist(l.0[seg], l.0[seg + 1]);
            seg += 1;
        }
        let (a, b) = (l.0[seg], l.0[(seg + 1).min(l.0.len() - 1)]);
        let len = dist(a, b).max(1e-9);
        let f = ((want - run) / len).clamp(0.0, 1.0);
        let (x, y) = (a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
        if let Some(z) = crate::terrain::height_at(g, x, y) {
            out.push((want, z));
        }
    }
    out
}

/// The edges of the picture of a map in another coordinate system, from points along them.
fn edges_in(frame: &crate::mapdraw::Frame, crs: u32, to: u32, ctx: &mut NodeCtx) -> Result<(Vec<f64>, Vec<f64>)> {
    let b = frame.b;
    let (w, h) = (frame.width as f64 * frame.px, frame.height as f64 * frame.px);
    let n = 24;
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for i in 0..=n {
        let f = i as f64 / n as f64;
        for (x, y) in [(b[0] + f * w, b[1]), (b[0] + f * w, b[1] + h), (b[0], b[1] + f * h), (b[0] + w, b[1] + f * h)] {
            xs.push(x);
            ys.push(y);
        }
    }
    if crs != to {
        ctx.backend("A grid in another coordinate system")?.transform(crs, to, &mut xs, &mut ys)?;
    }
    Ok((xs, ys))
}

/// Lines of a grid in another coordinate system, `every` apart, drawn in the coordinates of
/// the map, with a label for each from `label`.
fn grid_lines(
    frame: &crate::mapdraw::Frame,
    crs: u32,
    other: u32,
    every: f64,
    label: &dyn Fn(f64, bool) -> String,
    ctx: &mut NodeCtx,
) -> Result<Vec<crate::mapdraw::GratLine>> {
    let (xs, ys) = edges_in(frame, crs, other, ctx)?;
    let fold = |v: &[f64]| v.iter().filter(|x| x.is_finite()).fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
    let ((x0, x1), (y0, y1)) = (fold(&xs), fold(&ys));
    if x0 > x1 || (x1 - x0) / every > 200.0 || (y1 - y0) / every > 200.0 {
        return Ok(Vec::new());
    }
    let backend = ctx.backend("A grid in another coordinate system")?;
    let mut lines = Vec::new();
    let samples = 40;
    let mut add = |pts: Vec<(f64, f64)>, text: String, meridian: bool| -> Result<()> {
        let (mut x, mut y): (Vec<f64>, Vec<f64>) = pts.into_iter().unzip();
        if crs != other {
            backend.transform(other, crs, &mut x, &mut y)?;
        }
        let points: Vec<(f64, f64)> = x.into_iter().zip(y).filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
        if points.len() > 1 {
            lines.push(crate::mapdraw::GratLine { points, label: text, meridian });
        }
        Ok(())
    };
    let (ya, yb) = (y0 - every, y1 + every);
    let mut v = (x0 / every).ceil() * every;
    while v <= x1 {
        add((0..=samples).map(|i| (v, ya + (yb - ya) * i as f64 / samples as f64)).collect(), label(v, true), true)?;
        v += every;
    }
    let (xa, xb) = (x0 - every, x1 + every);
    let mut v = (y0 / every).ceil() * every;
    while v <= y1 {
        add((0..=samples).map(|i| (xa + (xb - xa) * i as f64 / samples as f64, v)).collect(), label(v, false), false)?;
        v += every;
    }
    Ok(lines)
}

/// The middle of the map in longitude and latitude.
fn centre_degrees(frame: &crate::mapdraw::Frame, crs: u32, ctx: &mut NodeCtx) -> Result<(f64, f64)> {
    let b = frame.b;
    let (mut x, mut y) = (vec![b[0] + frame.width as f64 * frame.px / 2.0], vec![b[1] + frame.height as f64 * frame.px / 2.0]);
    if crs != 4326 {
        ctx.backend("A grid")?.transform(crs, 4326, &mut x, &mut y)?;
    }
    Ok((x[0], y[0]))
}

/// The letters of the 100 km square of MGRS at an easting and a northing in a UTM zone.
pub fn mgrs_square(zone: u32, easting: f64, northing: f64) -> String {
    const COLS: [&str; 3] = ["ABCDEFGH", "JKLMNPQR", "STUVWXYZ"];
    const ROWS: &str = "ABCDEFGHJKLMNPQRSTUV";
    let set = ((zone + 2) % 3) as usize;
    let col = ((easting / 100_000.0).floor() as i64 - 1).clamp(0, 7) as usize;
    let offset = if zone.is_multiple_of(2) { 5 } else { 0 };
    let row = ((northing / 100_000.0).floor() as i64 + offset).rem_euclid(20) as usize;
    format!("{}{}", &COLS[set][col..col + 1], &ROWS[row..row + 1])
}

/// The letter of the band of latitude of UTM and MGRS.
fn utm_band(lat: f64) -> char {
    const BANDS: &[u8] = b"CDEFGHJKLMNPQRSTUVWX";
    BANDS[(((lat + 80.0) / 8.0).floor() as i64).clamp(0, 19) as usize] as char
}

/// A UTM grid in the zone of the middle of the map, `every` metres apart (1 km by the size
/// when not given), each line named by its kilometres within its 100 km square, as on
/// military maps, and the first ones with the zone, the band and the letters of the square.
fn utm_grid(frame: &crate::mapdraw::Frame, crs: u32, every: Option<f64>, ctx: &mut NodeCtx) -> Result<Vec<crate::mapdraw::GratLine>> {
    let (lon, lat) = centre_degrees(frame, crs, ctx)?;
    let zone = (((lon + 180.0) / 6.0).floor() as u32 + 1).clamp(1, 60);
    let utm = if lat >= 0.0 { 32600 + zone } else { 32700 + zone };
    let (xs, ys) = edges_in(frame, crs, utm, ctx)?;
    let span = xs.iter().chain(&ys).filter(|v| v.is_finite()).fold((f64::MAX, f64::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
    let wide = xs.iter().filter(|v| v.is_finite()).fold((f64::MAX, f64::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
    let _ = span;
    let every = every.filter(|e| *e > 0.0).unwrap_or_else(|| crate::chart::nice_step((wide.1 - wide.0).max(1.0), 6.0).max(1000.0));
    let mid_n = {
        let (mut x, mut y) = (vec![lon], vec![lat]);
        ctx.backend("A UTM grid")?.transform(4326, utm, &mut x, &mut y)?;
        y[0]
    };
    let first = std::cell::Cell::new(true);
    let label = |v: f64, east: bool| {
        let km = ((v / 1000.0).round() as i64).rem_euclid(100);
        let digits = format!("{:02}", km);
        if east && first.replace(false) {
            format!("{}{} {} {}", zone, utm_band(lat), mgrs_square(zone, v + 1.0, mid_n), digits)
        } else {
            digits
        }
    };
    grid_lines(frame, crs, utm, every, &label, ctx)
}

/// Map sheets of 10 km in SWEREF 99 TM: the lines between them, and the name of each in its
/// middle, the kilometres of its south-west corner north and east, as "6470 410".
/// The lines of a grid, and the names of its cells, each at a place.
type LinesAndNames = (Vec<crate::mapdraw::GratLine>, Vec<((f64, f64), String)>);

fn sheet_grid(frame: &crate::mapdraw::Frame, crs: u32, ctx: &mut NodeCtx) -> Result<LinesAndNames> {
    let every = 10_000.0;
    let lines = grid_lines(frame, crs, 3006, every, &|v: f64, _| format!("{}", (v / 1000.0).round() as i64), ctx)?;
    let (xs, ys) = edges_in(frame, crs, 3006, ctx)?;
    let fold = |v: &[f64]| v.iter().filter(|x| x.is_finite()).fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
    let ((x0, x1), (y0, y1)) = (fold(&xs), fold(&ys));
    let mut names = Vec::new();
    if (x1 - x0) / every <= 60.0 && (y1 - y0) / every <= 60.0 {
        let (mut cx, mut cy, mut text) = (Vec::new(), Vec::new(), Vec::new());
        let mut e = (x0 / every).floor() * every;
        while e <= x1 {
            let mut n = (y0 / every).floor() * every;
            while n <= y1 {
                // In the middle of the part of the sheet that is on the map, if enough of it is.
                let (a0, a1, b0, b1) = (e.max(x0), (e + every).min(x1), n.max(y0), (n + every).min(y1));
                if (a1 - a0) * (b1 - b0) < every * every * 0.12 {
                    n += every;
                    continue;
                }
                cx.push((a0 + a1) / 2.0);
                cy.push((b0 + b1) / 2.0);
                text.push(format!("{} {}", (n / 1000.0).round() as i64, (e / 1000.0).round() as i64));
                n += every;
            }
            e += every;
        }
        if crs != 3006 {
            ctx.backend("Map sheets")?.transform(3006, crs, &mut cx, &mut cy)?;
        }
        names = cx.into_iter().zip(cy).zip(text).filter(|((x, y), _)| x.is_finite() && y.is_finite()).collect();
    }
    Ok((lines, names))
}

/// The map cut along its outermost lines of latitude and longitude: the largest box of
/// degrees within the picture, its edges curved as they are on the map.
fn degree_frame(frame: &crate::mapdraw::Frame, crs: u32, ctx: &mut NodeCtx) -> Result<Option<geo::Geometry<f64>>> {
    if crs == 4326 {
        return Ok(None);
    }
    let (xs, ys) = edges_in(frame, crs, 4326, ctx)?;
    // The points come four at a time: south, north, west and east edges.
    let (mut west, mut east, mut south, mut north) = (f64::MIN, f64::MAX, f64::MIN, f64::MAX);
    for k in 0..xs.len() / 4 {
        let (sx, sy, nx, ny, wx, _wy, ex, _ey) =
            (xs[4 * k], ys[4 * k], xs[4 * k + 1], ys[4 * k + 1], xs[4 * k + 2], ys[4 * k + 2], xs[4 * k + 3], ys[4 * k + 3]);
        let _ = (sx, nx);
        south = south.max(sy);
        north = north.min(ny);
        west = west.max(wx);
        east = east.min(ex);
    }
    if !(west < east && south < north) {
        return Ok(None);
    }
    let n = 40;
    let mut ring: Vec<(f64, f64)> = Vec::new();
    for i in 0..n {
        ring.push((west + (east - west) * i as f64 / n as f64, south));
    }
    for i in 0..n {
        ring.push((east, south + (north - south) * i as f64 / n as f64));
    }
    for i in 0..n {
        ring.push((east - (east - west) * i as f64 / n as f64, north));
    }
    for i in 0..n {
        ring.push((west, north - (north - south) * i as f64 / n as f64));
    }
    let (mut x, mut y): (Vec<f64>, Vec<f64>) = ring.into_iter().unzip();
    ctx.backend("A frame along the degrees")?.transform(4326, crs, &mut x, &mut y)?;
    let mut pts: Vec<(f64, f64)> = x.into_iter().zip(y).collect();
    pts.push(pts[0]);
    Ok(Some(geo::Geometry::Polygon(geo::Polygon::new(pts.into(), Vec::new()))))
}

/// The globe for a corner: the middle of the map and its outline, in degrees.
fn globe_of(frame: &crate::mapdraw::Frame, crs: u32, corner: &str, ctx: &mut NodeCtx) -> Result<crate::mapdraw::Globe> {
    let centre = centre_degrees(frame, crs, ctx)?;
    let b = frame.b;
    let (w, h) = (frame.width as f64 * frame.px, frame.height as f64 * frame.px);
    let n = 10;
    let mut ring: Vec<(f64, f64)> = Vec::new();
    for i in 0..n {
        ring.push((b[0] + w * i as f64 / n as f64, b[1]));
    }
    for i in 0..n {
        ring.push((b[0] + w, b[1] + h * i as f64 / n as f64));
    }
    for i in 0..n {
        ring.push((b[0] + w - w * i as f64 / n as f64, b[1] + h));
    }
    for i in 0..n {
        ring.push((b[0], b[1] + h - h * i as f64 / n as f64));
    }
    let (mut x, mut y): (Vec<f64>, Vec<f64>) = ring.into_iter().unzip();
    if crs != 4326 {
        ctx.backend("A globe")?.transform(crs, 4326, &mut x, &mut y)?;
    }
    let area = x.into_iter().zip(y).filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    Ok(crate::mapdraw::Globe { corner: corner.to_string(), centre, area })
}

/// A logo read from its file and made as wide as a share of the map.
fn logo(path: &std::path::Path, map_width: u32, percent: f64, print: bool, ctx: &mut NodeCtx) -> Result<crate::model::RasterImage> {
    let img = image::open(path).map_err(|e| GbError(format!("Cannot read the logo {}: {}", path.display(), e)))?.to_rgba8();
    let w = ((map_width as f64 * percent.clamp(1.0, 60.0) / 100.0).round() as u32).max(4);
    let h = ((img.height() as f64 * w as f64 / img.width().max(1) as f64).round() as u32).max(1);
    // Made larger than it is, it is blurred on paper.
    if print && w as f64 > img.width() as f64 * 1.3 {
        ctx.notes.warn(format!(
            "The logo is {} pixels wide and is drawn {} wide, so it is blurred in print. A logo at least {} pixels wide, or as an SVG made into a large PNG, is sharp.",
            img.width(),
            w,
            w
        ));
    }
    let small = image::imageops::resize(&img, w, h, image::imageops::FilterType::Lanczos3);
    Ok(crate::model::RasterImage { vector: None, width: w as usize, height: h as usize, rgba: small.into_raw(), geotransform: [0.0, 1.0, 0.0, 0.0, 0.0, -1.0] })
}

/// The areas of the layers that an expression picks out, for the area a map is about.
fn matching(tables: &[(std::borrow::Cow<Table>, crate::mapdraw::LayerStyle)], expr: &str, ctx: &NodeCtx) -> Result<Vec<geo::Geometry<f64>>> {
    let mut out = Vec::new();
    for (t, _) in tables {
        let Ok(e) = crate::expr::compile(expr, &t.field_names()) else { continue };
        let geographic = ctx.geographic(t.crs);
        for (i, f) in t.features.iter().enumerate() {
            let Some(g) = f.geom.as_ref().filter(|g| matches!(g, geo::Geometry::Polygon(_) | geo::Geometry::MultiPolygon(_))) else { continue };
            if e.test(&crate::expr::Row { feature: f, index: i, geographic }).unwrap_or(false) {
                out.push(g.clone());
            }
        }
    }
    Ok(out)
}

/// Notes as they are written, one to a line: east, north and the text, as
/// "323000 6402000 New bridge 2027".
fn notes_of(text: &str) -> Result<Vec<(f64, f64, String)>> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut parts = line.splitn(3, char::is_whitespace);
        let x = parts.next().and_then(parse_number);
        let y = parts.next().and_then(parse_number);
        match (x, y, parts.next().map(str::trim).filter(|t| !t.is_empty())) {
            (Some(x), Some(y), Some(t)) => out.push((x, y, t.to_string())),
            _ => return err(format!("A note is east, north and its text, as \"323000 6402000 New bridge 2027\", not \"{}\".", line)),
        }
    }
    Ok(out)
}

/// The index of names of a map with index squares: each name that is labelled, its layer and
/// its square, in the order of the alphabet.
fn register(req: &crate::mapdraw::Request) -> Result<Table> {
    use geo::InteriorPoint;
    let b = crate::mapdraw::map_extent(req)?;
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for (li, (t, _)) in req.layers.iter().enumerate() {
        let Some(l) = req.labels.get(li).and_then(|l| l.as_ref()) else { continue };
        let layer = req.names.get(li).cloned().unwrap_or_default();
        for it in &l.items {
            let at = it.at.or_else(|| t.features[it.feature].geom.as_ref().and_then(|g| g.interior_point()).map(|p| (p.x(), p.y())));
            let Some((x, y)) = at else { continue };
            if let Some(sq) = crate::mapdraw::index_square(b, req.furniture.index_grid, x, y) {
                rows.push((it.text.replace('\n', " "), sq, layer.clone()));
            }
        }
    }
    rows.sort_by(|a, b| crate::nodes::alphabetical(&a.0).cmp(&crate::nodes::alphabetical(&b.0)).then(a.1.cmp(&b.1)));
    rows.dedup();
    let mut t = Table {
        fields: vec![Field::new("name", FieldType::Text), Field::new("square", FieldType::Text), Field::new("layer", FieldType::Text)],
        ..Default::default()
    };
    for (n, s, l) in rows {
        t.features.push(Feature { geom: None, z: None, attrs: vec![Value::Text(n), Value::Text(s), Value::Text(l)] });
    }
    Ok(t)
}

/// A line or an area with its corners rounded off; anything else as it is.
fn smoothed(g: geo::Geometry<f64>) -> geo::Geometry<f64> {
    use geo::ChaikinSmoothing;
    match g {
        geo::Geometry::LineString(l) => l.chaikin_smoothing(2).into(),
        geo::Geometry::MultiLineString(l) => l.chaikin_smoothing(2).into(),
        geo::Geometry::Polygon(p) => p.chaikin_smoothing(2).into(),
        geo::Geometry::MultiPolygon(p) => p.chaikin_smoothing(2).into(),
        other => other,
    }
}

/// The colour of a box: None when empty, for its own colour; Some(None) for "none".
fn box_colour(p: &super::P, key: &str, label: &str) -> Result<Option<Option<[u8; 4]>>> {
    match p.get(key).map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) {
        None => Ok(None),
        Some(c) if c.eq_ignore_ascii_case("none") => Ok(Some(None)),
        Some(c) => crate::mapdraw::parse_color(&c).map(Some).map_err(|e| GbError(format!("{}: {}", label, e))),
    }
}

/// A colour setting, or None when it is empty.
fn colour_of(p: &super::P, key: &str, label: &str) -> Result<Option<[u8; 4]>> {
    match p.get(key).filter(|c| !c.trim().is_empty()) {
        Some(c) => crate::mapdraw::parse_color(&c).map_err(|e| GbError(format!("{}: {}", label, e))),
        None => Ok(None),
    }
}

/// The area of an inset: around the features of every layer that the expression picks out,
/// with a little room.
fn inset_area(tables: &[(std::borrow::Cow<Table>, crate::mapdraw::LayerStyle)], expr: &str, ctx: &NodeCtx) -> Result<[f64; 4]> {
    use geo::BoundingRect;
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut found = 0usize;
    for (t, _) in tables {
        // A layer without the attributes the expression names is passed by.
        let Ok(e) = crate::expr::compile(expr, &t.field_names()) else { continue };
        let geographic = ctx.geographic(t.crs);
        for (i, f) in t.features.iter().enumerate() {
            if !e.test(&crate::expr::Row { feature: f, index: i, geographic }).unwrap_or(false) {
                continue;
            }
            if let Some(r) = f.geom.as_ref().and_then(|g| g.bounding_rect()) {
                b = [b[0].min(r.min().x), b[1].min(r.min().y), b[2].max(r.max().x), b[3].max(r.max().y)];
                found += 1;
            }
        }
    }
    if found == 0 {
        return err(format!("Inset of: no feature of any layer is picked out by {}.", expr));
    }
    // At least a tenth of the whole map across, so that a town picked out shows its
    // surroundings, and a little room around.
    let layers: Vec<(&Table, crate::mapdraw::LayerStyle)> = tables.iter().map(|(t, s)| (t.as_ref(), s.clone())).collect();
    let whole = crate::mapdraw::extent_of(&layers).map_or(0.0, |e| (e[2] - e[0]).max(e[3] - e[1]));
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let half = ((b[2] - b[0]).max(b[3] - b[1]) * 0.55).max(whole * 0.05).max(50.0);
    Ok([cx - half, cy - half, cx + half, cy + half])
}

/// The colours of the slices and bars of charts on a map, in turn.
fn chart_colours() -> impl Iterator<Item = [u8; 4]> {
    crate::chart::COLOURS.iter().filter(|c| c.0 != "grey").map(|c| [c.2[0], c.2[1], c.2[2], 255]).cycle()
}

/// The charts on the features of a layer: the values of each, and how large each is drawn.
fn prepare_charts(t: &Table, spec: &crate::mapdraw::ChartSpec) -> Result<Option<crate::mapdraw::MapCharts>> {
    if spec.kind.is_empty() || spec.kind == "none" {
        return Ok(None);
    }
    if spec.of.is_empty() {
        return err("Choose the attributes the charts show.");
    }
    let idx: Vec<usize> = spec.of.iter().map(|n| t.need_field(n)).collect::<Result<_>>()?;
    let values: Vec<Vec<f64>> = t
        .features
        .iter()
        .map(|f| {
            idx.iter()
                .map(|i| f.attrs[*i].as_f64().or_else(|| crate::model::parse_number(&f.attrs[*i].to_text())).filter(|v| v.is_finite()).unwrap_or(0.0))
                .collect()
        })
        .collect();
    // How large each is: a pie by its area, so the side grows as the square root.
    let by: Vec<f64> = match spec.size_by.as_str() {
        "" => vec![1.0; t.len()],
        "total" => values.iter().map(|v| v.iter().map(|x| x.max(0.0)).sum()).collect(),
        a => {
            let i = t.need_field(a)?;
            t.features.iter().map(|f| f.attrs[i].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0)).collect()
        }
    };
    let most = by.iter().copied().fold(0.0, f64::max);
    let sizes = by.iter().map(|b| if most > 0.0 { spec.size * (b / most).sqrt() as f32 } else { spec.size }).collect();
    let top = values.iter().flatten().copied().fold(0.0, f64::max);
    Ok(Some(crate::mapdraw::MapCharts { kind: spec.kind.clone(), values, sizes, colours: chart_colours().take(spec.of.len().max(1)).collect(), top }))
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

/// The outputs of Draw map: the map, and the register of the names when it has an index of
/// squares.
fn map_ports(p: &P) -> Vec<String> {
    let indexed = p.get("index_grid").and_then(|v| v.trim().parse::<usize>().ok()).is_some_and(|n| n > 0);
    if indexed {
        vec!["Map".into(), "Register".into()]
    } else {
        vec!["Map".into()]
    }
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
        outputs: &["Map", "Register"],
        ports: Some(map_ports),
        params: MAP_PARAMS,
        run: run_map,
        summary: |p| {
            if p.get("size_as").as_deref() == Some("print") {
                format!("{} mm at {} dpi", p.get("print_mm").unwrap_or_else(|| "180".into()), p.get("print_dpi").unwrap_or_else(|| "300".into()))
            } else if p.get("size_as").as_deref() == Some("scale") {
                let paper = p.get("paper").unwrap_or_else(|| "a3l".into());
                format!(
                    "1:{} on {} {}",
                    p.get("map_scale").unwrap_or_else(|| "50000".into()),
                    paper[..2].to_uppercase(),
                    if paper.ends_with('p') { "portrait" } else { "landscape" }
                )
            } else {
                format!("{} px wide", p.get("width").unwrap_or_else(|| "1600".into()))
            }
        },
    },
];

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn notes_are_read_and_a_template_keeps_the_look_only() {
        let n = notes_of("323000 6402000 New bridge 2027\n\n 1,5 2 A").unwrap();
        assert_eq!(n, vec![(323000.0, 6402000.0, "New bridge 2027".to_string()), (1.5, 2.0, "A".to_string())]);
        assert!(notes_of("323000 New bridge").is_err());
        let mut p = Params::new();
        for (k, v) in [("layers", "[]"), ("title", "Towns"), ("legend", "top-right"), ("legend_fill", "#ffffff"), ("notes", "1 2 A")] {
            p.insert(k.into(), serde_json::Value::String(v.into()));
        }
        let t = map_template(&p);
        assert_eq!(t.keys().cloned().collect::<Vec<_>>(), vec!["legend".to_string(), "legend_fill".to_string()]);
    }

    #[test]
    fn places_near_a_line_in_the_order_they_come_along_it() {
        let mut t = Table { fields: vec![crate::model::Field::new("name", crate::model::FieldType::Text)], ..Default::default() };
        for (x, y, n) in [(80.0, 1.0, "Far"), (20.0, -2.0, "Near"), (50.0, 40.0, "Away")] {
            t.features.push(crate::model::Feature::new(vec![Value::Text(n.into())], Some(geo::Geometry::Point(geo::Point::new(x, y)))));
        }
        let line: geo::LineString<f64> = vec![(0.0, 0.0), (100.0, 0.0)].into();
        let places = places_along(&t, &line, 1000.0);
        assert_eq!(places.iter().map(|p| p.1.as_str()).collect::<Vec<_>>(), vec!["Near", "Far"]);
        assert!((places[0].0 - 200.0).abs() < 1e-6);
    }

    #[test]
    fn squares_of_mgrs_and_bands_of_utm() {
        // The Washington Monument is in 18S UJ, and the made-up village in 33V VE.
        assert_eq!(mgrs_square(18, 323_487.0, 4_306_483.0), "UJ");
        assert_eq!(mgrs_square(33, 418_200.0, 6_474_300.0), "VE");
        assert_eq!((utm_band(38.9), utm_band(58.4), utm_band(-33.9)), ('S', 'V', 'H'));
    }
}
