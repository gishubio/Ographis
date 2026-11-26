//! Draws the streams of a model into a picture with coordinates: one layer per stream, each
//! with its colours, in the order the user has given. Anti-aliased, with tiny-skia.

use crate::model::{GbError, RasterImage, Result, Table};
use crate::vector::Canvas;
use geo::{BoundingRect, Coord, Geometry, LineString};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// The largest picture, in pixels on a side.
pub const MAX_SIDE: u32 = 20_000;

/// The look of one layer.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerStyle {
    /// Areas and points are filled with it. None leaves them unfilled.
    pub fill: Option<[u8; 4]>,
    /// Lines, and the outlines of areas and points. None draws no outline.
    pub stroke: Option<[u8; 4]>,
    /// Of lines and outlines, in pixels.
    pub width: f32,
    /// The diameter of a point, in pixels.
    pub size: f32,
    /// How lines and outlines are drawn: "" solid, "dash", "dot" or "dashdot".
    pub dash: String,
    /// A wider line under each line, in this colour, as the edge of a road. None: none.
    pub casing: Option<[u8; 4]>,
    /// How much wider the edge is than the line, in pixels on each side.
    pub casing_width: f32,
    /// A pattern over the fill of areas: "" none, or one of [`PATTERNS`].
    pub pattern: String,
    /// The colour of the pattern. None: that of the line.
    pub pattern_colour: Option<[u8; 4]>,
    /// The space between the lines or dots of the pattern, in pixels.
    pub pattern_every: f32,
    /// How much of the layer shows, from 0 (nothing) to 1 (all of it).
    pub opacity: f32,
    /// Arrow heads on lines: "" none, or one of [`ARROWS`].
    pub arrows: String,
    /// How long an arrow head is, in widths of its line.
    pub arrow_size: f32,
    /// How far lines stop short of their ends, in pixels, as flows short of their towns.
    pub gap: f32,
    /// The scales the layer is drawn at, as the number after 1:, from the closest to the
    /// farthest. 0: no limit that way.
    pub scale_min: f64,
    pub scale_max: f64,
    /// How the layer is laid on what is under it: "multiply" and the others of Op::Blend,
    /// or empty for as it is.
    pub blend: String,
    /// Corners rounded off, so that coarse lines and areas look smooth.
    pub smooth: bool,
    /// Left out of the legend, such as a line that a profile is drawn along.
    pub no_legend: bool,
    /// A soft "glow" around the features, or a "shadow" under them, in this colour. Empty:
    /// none.
    pub glow: String,
    pub glow_colour: Option<[u8; 4]>,
    /// Contours: every line whose value of this attribute is a whole number of `index_every`
    /// is drawn heavier, as an index contour, and only those are named.
    pub index_by: String,
    pub index_every: f64,
    /// A band of this many pixels inside the edge of each area, in the colour of its edge,
    /// as along a border on a topographic map. 0: none.
    pub band: f32,
    /// Thin lines along the shore of the areas, fading outwards, as water is drawn on old maps.
    pub waterlines: bool,
    /// Illuminated contours (Tanaka): the edges of areas between contours light where they
    /// face the light from the north-west and dark where they turn from it, thicker the more.
    pub tanaka: bool,
    /// Lines: an attribute that sets the width, from a third of `width` at its least to twice
    /// it at its most, as the order of a stream or the traffic on a road.
    pub width_by: String,
    /// Lines that widen from their start to their end, as rivers downstream.
    pub taper: bool,
    /// Areas: an attribute whose value sets how see-through each area is, from faint at the
    /// least to solid at the most, so that what concerns few is not as loud (value by alpha).
    pub alpha_by: String,
    /// For each feature, found from `bridge_where`, `tunnel_where` and `oneway_where`:
    /// 1 a bridge, 2 a tunnel, 4 one way. Empty: none.
    pub marks: std::sync::Arc<Vec<u8>>,
    pub bridge_where: String,
    pub tunnel_where: String,
    pub oneway_where: String,
    /// Lines: an attribute with the number of a road, written in a sign along it, such as
    /// E20 in green and 49 in blue.
    pub shield: String,
    /// Pixels between the signs along a line.
    pub shield_every: f32,
}

impl Default for LayerStyle {
    fn default() -> Self {
        LayerStyle {
            fill: None,
            stroke: Some([0, 0, 0, 255]),
            width: 1.0,
            size: 6.0,
            dash: String::new(),
            casing: None,
            casing_width: 1.0,
            pattern: String::new(),
            pattern_colour: None,
            pattern_every: 8.0,
            opacity: 1.0,
            arrows: String::new(),
            arrow_size: 3.0,
            gap: 0.0,
            scale_min: 0.0,
            scale_max: 0.0,
            blend: String::new(),
            smooth: false,
            no_legend: false,
            glow: String::new(),
            glow_colour: None,
            index_by: String::new(),
            index_every: 0.0,
            band: 0.0,
            waterlines: false,
            tanaka: false,
            width_by: String::new(),
            taper: false,
            alpha_by: String::new(),
            marks: Default::default(),
            bridge_where: String::new(),
            tunnel_where: String::new(),
            oneway_where: String::new(),
            shield: String::new(),
            shield_every: 260.0,
        }
    }
}

/// The patterns an area can have over its fill, as they are stored and named.
pub const PATTERNS: &[(&str, &str)] = &[
    ("", "No pattern"),
    ("hatch", "Hatched /"),
    ("back", "Hatched \\"),
    ("cross", "Cross-hatched"),
    ("horizontal", "Horizontal lines"),
    ("vertical", "Vertical lines"),
    ("grid", "Squares"),
    ("dots", "Dots"),
    ("marsh", "Marsh: tufts of grass"),
    ("conifers", "Conifers"),
    ("deciduous", "Deciduous trees"),
    ("orchard", "Orchard: trees in rows"),
    ("rock", "Rock and scree"),
    ("sand", "Sand: fine dots"),
];

/// The ways a line can be drawn, as they are stored and named.
/// The ways a layer can be laid on what is under it, as stored and named.
pub const BLENDS: &[(&str, &str)] =
    &[("", "Normal"), ("multiply", "Multiply"), ("screen", "Screen"), ("darken", "Darken"), ("lighten", "Lighten"), ("overlay", "Overlay")];

/// A soft edge around the features of a layer, as stored and named.
pub const GLOWS: &[(&str, &str)] = &[("", "Nothing"), ("glow", "A glow around"), ("shadow", "A shadow under")];

pub const DASHES: &[(&str, &str)] = &[
    ("", "Solid"),
    ("dash", "Dashed"),
    ("dot", "Dotted"),
    ("dashdot", "Dash and dot"),
    ("rail", "Railway"),
    ("tram", "Tram, with ticks"),
    ("border_country", "Border of a country: dash and dot"),
    ("border_region", "Border of a county: dash and two dots"),
    ("border_local", "Border of a municipality: short dashes"),
];

/// Where lines get an arrow head, as it is stored and named.
pub const ARROWS: &[(&str, &str)] = &[("", "No arrows"), ("end", "At the end"), ("start", "At the start"), ("both", "At both ends")];

/// An arrow head: where its tip is, the way it points (of length 1) and how long it is.
type Head = (f32, f32, f32, f32, f32);

/// The lines of a geometry in pixels, cut back where they get an arrow head so that the line
/// does not show beside its tip, with the heads that go on them. None for what is not a line.
fn arrowed_lines(
    g: &Geometry<f64>,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
    arrows: &str,
    len: f32,
    gap: f32,
    heads: &mut Vec<Head>,
) -> Option<Vec<Vec<(f32, f32)>>> {
    let parts: Vec<&LineString<f64>> = match g {
        Geometry::LineString(l) => vec![l],
        Geometry::MultiLineString(m) => m.0.iter().collect(),
        _ => return None,
    };
    let (at_end, at_start) = (matches!(arrows, "end" | "both"), matches!(arrows, "start" | "both"));
    let mut out = Vec::new();
    for l in parts {
        let mut pts: Vec<(f32, f32)> = Vec::with_capacity(l.0.len());
        for c in &l.0 {
            let q = to_px(c.x, c.y);
            if pts.last().is_none_or(|p: &(f32, f32)| (p.0 - q.0).hypot(p.1 - q.1) > 0.01) {
                pts.push(q);
            }
        }
        let mut total: f32 = pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
        // Short of its ends, as a flow short of the towns, when it is long enough for that.
        if gap > 0.0 && pts.len() >= 2 && total > gap * 3.0 {
            cut_back(&mut pts, gap, 0.0);
            pts.reverse();
            cut_back(&mut pts, gap, 0.0);
            pts.reverse();
            total -= gap * 2.0;
        }
        // Too short for its heads: drawn as it is.
        let ends = at_end as u8 as f32 + at_start as u8 as f32;
        if pts.len() < 2 || total < len * ends * 1.2 {
            out.push(pts);
            continue;
        }
        if at_end {
            if let Some(head) = cut_back(&mut pts, len, 0.3) {
                heads.push(head);
            }
        }
        if at_start {
            pts.reverse();
            if let Some(head) = cut_back(&mut pts, len, 0.3) {
                heads.push(head);
            }
            pts.reverse();
        }
        out.push(pts);
    }
    Some(out)
}

/// Cuts the end of a line back under a head as long as `len`, and gives that head: its tip at
/// the end, pointing the way the last `len` of the line goes.
fn cut_back(pts: &mut Vec<(f32, f32)>, len: f32, under: f32) -> Option<Head> {
    let tip = *pts.last()?;
    let mut left = len;
    let mut i = pts.len() - 1;
    let base = loop {
        if i == 0 {
            return None;
        }
        let (a, b) = (pts[i - 1], pts[i]);
        let d = (b.0 - a.0).hypot(b.1 - a.1);
        if d >= left {
            let t = left / d;
            break (b.0 - (b.0 - a.0) * t, b.1 - (b.1 - a.1) * t);
        }
        left -= d;
        i -= 1;
    };
    let (dx, dy) = (tip.0 - base.0, tip.1 - base.1);
    let n = dx.hypot(dy).max(f32::MIN_POSITIVE);
    // The line stops a little inside the head, so that no gap shows between them.
    let keep = (base.0 + dx * under, base.1 + dy * under);
    pts.truncate(i);
    pts.push(keep);
    Some((tip.0, tip.1, dx / n, dy / n, len))
}

/// The arrow heads of a line as wide as `width`.
fn draw_heads(pixmap: &mut Canvas, heads: &[Head], width: f32, colour: [u8; 4]) {
    for &(x, y, dx, dy, len) in heads {
        let half = (len * 0.42).max(width * 0.5 + 2.0);
        let (bx, by) = (x - dx * len, y - dy * len);
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(bx - dy * half, by + dx * half);
        pb.line_to(bx + dy * half, by - dx * half);
        pb.close();
        if let Some(p) = pb.finish() {
            pixmap.fill_path(&p, &paint(colour), FillRule::Winding, Transform::identity(), None);
        }
    }
}

/// How long the heads of a line as wide as `width` are.
fn head_length(style: &LayerStyle, width: f32) -> f32 {
    (width.max(0.5) * style.arrow_size.clamp(1.0, 10.0)).max(7.0)
}

fn polyline_path(lines: &[Vec<(f32, f32)>], pb: &mut PathBuilder) {
    for l in lines {
        for (k, &(x, y)) in l.iter().enumerate() {
            if k == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
    }
}

impl LayerStyle {
    /// The stroke of lines and outlines in this style, with its dashes.
    pub fn stroke(&self) -> Stroke {
        let w = self.width.max(0.0);
        let unit = w.max(1.0);
        let pattern: Option<Vec<f32>> = match self.dash.as_str() {
            "dash" => Some(vec![unit * 4.0, unit * 2.5]),
            "dot" => Some(vec![0.01, unit * 2.0]),
            "dashdot" => Some(vec![unit * 4.0, unit * 2.0, 0.01, unit * 2.0]),
            // The lines of borders, as the legend shows them; on the map they are fitted to
            // their corners.
            "border_country" | "border_region" | "border_local" => {
                let u = (w * 1.6).max(2.0);
                Some(match self.dash.as_str() {
                    "border_country" => vec![u * 7.0, u * 1.8, 0.01, u * 1.8],
                    "border_region" => vec![u * 5.0, u * 1.5, 0.01, u * 1.5, 0.01, u * 1.5],
                    _ => vec![u * 3.0, u * 1.6],
                })
            }
            _ => None,
        };
        let cap = if self.dash == "rail" {
            tiny_skia::LineCap::Butt
        } else if matches!(self.dash.as_str(), "dot" | "dashdot" | "border_country" | "border_region" | "border_local") {
            tiny_skia::LineCap::Round
        } else if pattern.is_some() {
            tiny_skia::LineCap::Butt
        } else {
            tiny_skia::LineCap::Round
        };
        Stroke {
            width: w,
            line_join: tiny_skia::LineJoin::Round,
            line_cap: cap,
            dash: pattern.and_then(|p| tiny_skia::StrokeDash::new(p, 0.0)),
            ..Stroke::default()
        }
    }

    /// What is drawn over the line for a railway or a tramway: white dashes in a railway,
    /// short ticks across a tramway. None for other lines.
    pub fn overlay(&self, colour: [u8; 4]) -> Option<(Stroke, [u8; 4])> {
        let w = self.width.max(0.5);
        match self.dash.as_str() {
            "rail" => Some((
                Stroke {
                    width: (w * 0.55).max(0.5),
                    line_join: tiny_skia::LineJoin::Round,
                    dash: tiny_skia::StrokeDash::new(vec![w.max(1.0) * 5.0, w.max(1.0) * 5.0], 0.0),
                    ..Stroke::default()
                },
                [255, 255, 255, colour[3]],
            )),
            "tram" => Some((
                Stroke {
                    width: (w * 4.0).max(5.0),
                    line_join: tiny_skia::LineJoin::Miter,
                    dash: tiny_skia::StrokeDash::new(vec![(w * 0.8).max(1.0), (w * 7.0).max(8.0)], 0.0),
                    ..Stroke::default()
                },
                colour,
            )),
            _ => None,
        }
    }

    /// Whether the layer is drawn at the scale 1:`scale`.
    pub fn shown_at(&self, scale: f64) -> bool {
        (self.scale_min <= 0.0 || scale >= self.scale_min * 0.999) && (self.scale_max <= 0.0 || scale <= self.scale_max * 1.001)
    }

    /// The stroke of the edge under a line, solid and wider.
    pub fn casing_stroke(&self) -> Stroke {
        Stroke {
            width: self.width.max(0.0) + 2.0 * self.casing_width.max(0.0),
            line_join: tiny_skia::LineJoin::Round,
            line_cap: tiny_skia::LineCap::Round,
            ..Stroke::default()
        }
    }
}

/// The colours a layer gets until the user chooses others: a light fill and a darker line
/// of the same hue, one pair after the other.
pub const DEFAULT_COLORS: &[([u8; 3], [u8; 3])] = &[
    ([0xa6, 0xce, 0xe3], [0x1f, 0x78, 0xb4]),
    ([0xb2, 0xdf, 0x8a], [0x33, 0xa0, 0x2c]),
    ([0xfd, 0xbf, 0x6f], [0xff, 0x7f, 0x00]),
    ([0xca, 0xb2, 0xd6], [0x6a, 0x3d, 0x9a]),
    ([0xfb, 0x9a, 0x99], [0xe3, 0x1a, 0x1c]),
    ([0xd9, 0xd9, 0xd9], [0x52, 0x52, 0x52]),
];

impl LayerStyle {
    /// The look a layer has before the user has chosen one: the colours by its place.
    pub fn default_for(index: usize) -> LayerStyle {
        let (fill, line) = DEFAULT_COLORS[index % DEFAULT_COLORS.len()];
        LayerStyle {
            fill: Some([fill[0], fill[1], fill[2], 255]),
            stroke: Some([line[0], line[1], line[2], 255]),
            width: 1.0,
            size: 6.0,
            ..LayerStyle::default()
        }
    }
}

/// A colour as text: #rrggbb or #rrggbbaa. Empty or "none" is no colour.
pub fn parse_color(text: &str) -> std::result::Result<Option<[u8; 4]>, String> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let hex = t.strip_prefix('#').unwrap_or(t);
    let byte =
        |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| format!("\"{}\" is not a colour. Write it as #rrggbb, for example #1f78b4.", text));
    match hex.len() {
        6 => Ok(Some([byte(0)?, byte(2)?, byte(4)?, 255])),
        8 => Ok(Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
        _ => Err(format!("\"{}\" is not a colour. Write it as #rrggbb, for example #1f78b4.", text)),
    }
}

/// A colour as text, the way it is stored: #rrggbb, with the opacity only when it is not full.
pub fn color_text(c: Option<[u8; 4]>) -> String {
    match c {
        None => "none".into(),
        Some([r, g, b, 255]) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        Some([r, g, b, a]) => format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a),
    }
}

/// How the features of a layer are labelled, as it is stored with the layer.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelSpec {
    /// An attribute, or an expression such as name || ' ' || year. Empty: no labels.
    pub text: String,
    /// "auto": beside points, along lines bending with them, and in the middle of areas;
    /// "straight": as auto, but straight along lines; "center": on the point or in the
    /// middle; "xy": at the coordinates in two attributes.
    pub place: String,
    pub x_attr: String,
    pub y_attr: String,
    /// Draw them all, also where they overlap. Otherwise a label that would cover one
    /// already placed is left out.
    pub all: bool,
    /// The height of the letters in pixels, or the smallest with `size_by`.
    pub size: f32,
    /// An attribute that makes the labels larger with its value, up to `size_max`.
    pub size_by: String,
    pub size_max: f32,
    /// An attribute whose largest values are placed first, when there is not room for all.
    pub priority_by: String,
    pub bold: bool,
    pub upper: bool,
    /// An expression: the labels of the features where it is true are in capitals, such as
    /// population > 100000 for the large towns. Empty: as `upper` says, for all.
    pub upper_where: String,
    pub colour: [u8; 4],
    /// An edge around the letters, so that they can be read on anything. None: none.
    pub halo: Option<[u8; 4]>,
    pub halo_width: f32,
    /// The letters spread out over an area, as the name of a lake or a district.
    pub spaced: bool,
    /// Small capitals: the label in capitals as high as small letters, such as for parishes.
    pub small_caps: bool,
    /// Room between the letters, as a share of the size: 0.2 is a fifth of it. 0: none.
    pub tracking: f32,
    /// Along lines, the top of the letters towards the higher ground of the first grid of
    /// heights in the map, as the numbers of contours are written, even upside down.
    pub uphill: bool,
    /// The names of areas of water written along their shore, a little out from it, as the
    /// names of bays and fjords are.
    pub coast: bool,
    /// The lines after the first smaller and not bold, as a description under a name:
    /// "summit 510 m below sea level" under "Marsili Seamount".
    pub detail: bool,
    /// Leaning letters, as the names of water are written.
    pub italic: bool,
    /// An expression: the labels of the features where it is true are bold, such as
    /// kind = 'county seat'. Empty: as `bold` says, for all.
    pub bold_where: String,
    /// Areas smaller than this many pixels across get no label, or one beside them with a
    /// line when `leaders` is on. 0: every area.
    pub min_px: f32,
    /// Along lines, a label every this many pixels, such as the number of a road. 0: once.
    pub repeat: f32,
    /// A label that does not fit in its area is put beside it, with a thin line in to it.
    pub leaders: bool,
    /// Names of areas along a gentle curve through the middle of the area, as the names of
    /// long lakes and regions are written.
    pub curved: bool,
    /// Labels of layers with a higher rank are placed first, whatever layer is on top.
    pub rank: i32,
    /// "classic": labels of points tried first above and to the right, then below to the
    /// right, above to the left, below to the left, then beside, above and below.
    pub prefer: String,
}

impl Default for LabelSpec {
    fn default() -> Self {
        LabelSpec {
            text: String::new(),
            place: "auto".into(),
            x_attr: String::new(),
            y_attr: String::new(),
            all: false,
            size: 12.0,
            size_by: String::new(),
            size_max: 24.0,
            priority_by: String::new(),
            bold: false,
            upper: false,
            upper_where: String::new(),
            colour: [0x22, 0x22, 0x22, 255],
            halo: Some([255, 255, 255, 255]),
            halo_width: 2.0,
            spaced: false,
            small_caps: false,
            tracking: 0.0,
            uphill: false,
            coast: false,
            detail: false,
            italic: false,
            bold_where: String::new(),
            min_px: 0.0,
            repeat: 0.0,
            leaders: false,
            curved: false,
            rank: 0,
            prefer: String::new(),
        }
    }
}

/// The labels of a layer, ready to place: the text of each feature, its size, and where
/// it comes in the queue for room.
#[derive(Debug, Clone)]
pub struct Labels {
    pub spec: LabelSpec,
    pub items: Vec<LabelItem>,
}

#[derive(Debug, Clone)]
pub struct LabelItem {
    pub feature: usize,
    pub text: String,
    pub size: f32,
    /// Larger is placed first.
    pub priority: f64,
    /// Bold, by the layer's `bold_where`.
    pub bold: bool,
    /// Where the label goes, in the coordinates of the map, when it is placed from attributes.
    pub at: Option<(f64, f64)>,
}

/// How the points of a layer are drawn as icons, as it is stored with the layer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IconSpec {
    /// The icon of every point: a name of Maki, or a word for one, such as hospital.
    pub name: String,
    /// An attribute whose values name the icon of each point, such as amenity.
    pub by: String,
    /// "plain": the icon in the fill colour with an edge in the line colour; "circle": the
    /// icon in white on a circle of the fill colour.
    pub style: String,
}

impl IconSpec {
    pub fn is_off(&self) -> bool {
        self.name.trim().is_empty() && self.by.trim().is_empty()
    }
}

/// Small charts on the features of a layer, as it is stored with the layer: a pie or a few
/// bars on each area or point, of the values of some attributes, such as the votes of each
/// party in each municipality.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChartSpec {
    /// "pie", "bars", or empty for none.
    pub kind: String,
    /// The attributes whose values make the slices or the bars, in that order.
    pub of: Vec<String>,
    /// The width of the largest chart, in pixels.
    pub size: f32,
    /// "total": pies as large as the sum of their values, by their area; an attribute: as
    /// large as its value; empty: all as large.
    pub size_by: String,
}

/// The charts of a layer, ready to draw: the values of each feature, how large its chart is,
/// and the colour of each attribute.
#[derive(Debug, Clone)]
pub struct MapCharts {
    pub kind: String,
    pub values: Vec<Vec<f64>>,
    pub sizes: Vec<f32>,
    pub colours: Vec<[u8; 4]>,
    /// For bars: the largest value of all, which a bar as high as the chart shows.
    pub top: f64,
}

/// Draws a chart at (x, y) on the map: a pie with its middle there, or bars with their foot.
fn draw_map_chart(pm: &mut Canvas, c: &MapCharts, values: &[f64], size: f32, x: f32, y: f32) {
    let edge = Stroke { width: (size / 40.0).clamp(0.6, 1.5), line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
    let white = paint([255, 255, 255, 255]);
    if c.kind == "bars" {
        let n = values.len().max(1) as f32;
        let bw = size / n;
        let h = size * 0.8;
        let x0 = x - size / 2.0;
        let foot = y + h / 2.0;
        // A line at the foot, so that a bar of nothing still shows where it would be.
        if let Some(r) = tiny_skia::Rect::from_xywh(x0 - 1.0, foot, size + 2.0, edge.width.max(1.0)) {
            pm.fill_rect(r, &paint([0x52, 0x51, 0x4e, 255]), Transform::identity(), None);
        }
        for (i, v) in values.iter().enumerate() {
            let bh = if c.top > 0.0 { (v.max(0.0) / c.top) as f32 * h } else { 0.0 };
            if bh <= 0.0 {
                continue;
            }
            if let Some(r) = tiny_skia::Rect::from_xywh(x0 + bw * i as f32, foot - bh, bw, bh) {
                let path = PathBuilder::from_rect(r);
                pm.fill_path(&path, &paint(c.colours[i % c.colours.len()]), FillRule::Winding, Transform::identity(), None);
                pm.stroke_path(&path, &white, &edge, Transform::identity(), None);
            }
        }
        return;
    }
    let total: f64 = values.iter().map(|v| v.max(0.0)).sum();
    if total <= 0.0 || size < 1.0 {
        return;
    }
    let r = size / 2.0;
    // From twelve o'clock, clockwise.
    let mut a0 = -std::f32::consts::FRAC_PI_2;
    for (i, v) in values.iter().enumerate() {
        let part = (v.max(0.0) / total) as f32;
        if part <= 0.0 {
            continue;
        }
        let a1 = a0 + part * std::f32::consts::TAU;
        let mut pb = PathBuilder::new();
        if part >= 0.9999 {
            pb.push_circle(x, y, r);
        } else {
            pb.move_to(x, y);
            let steps = ((part * 64.0).ceil() as usize).max(2);
            for s in 0..=steps {
                let a = a0 + (a1 - a0) * s as f32 / steps as f32;
                pb.line_to(x + r * a.cos(), y + r * a.sin());
            }
            pb.close();
        }
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint(c.colours[i % c.colours.len()]), FillRule::Winding, Transform::identity(), None);
            pm.stroke_path(&path, &white, &edge, Transform::identity(), None);
        }
        a0 = a1;
    }
}

/// Draws the charts of a layer, the largest first so that small ones are not hidden.
fn draw_map_charts(pm: &mut Canvas, table: &Table, charts: &MapCharts, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let mut order: Vec<usize> = (0..table.len()).collect();
    order.sort_by(|a, b| charts.sizes[*b].total_cmp(&charts.sizes[*a]));
    for i in order {
        let Some(g) = table.features[i].geom.as_ref() else { continue };
        let at = point_of(g).or_else(|| {
            use geo::InteriorPoint;
            g.interior_point()
        });
        let Some(p) = at else { continue };
        let (x, y) = to_px(p.x(), p.y());
        draw_map_chart(pm, charts, &charts.values[i], charts.sizes[i], x, y);
    }
}

/// The icons of the points of a layer: the icon of each feature, if it has one.
#[derive(Debug, Clone)]
pub struct Icons {
    pub style: String,
    pub names: Vec<Option<&'static str>>,
}

/// How large an icon is drawn: the point size, or 18 when that is too small for an icon.
pub fn icon_size(size: f32) -> f32 {
    if size < 10.0 {
        18.0
    } else {
        size
    }
}

/// Draws an icon in its style, with its middle at (x, y).
#[allow(clippy::too_many_arguments)]
pub fn draw_icon(pm: &mut Canvas, name: &str, x: f32, y: f32, size: f32, style: &str, fill: Option<[u8; 4]>, stroke: Option<[u8; 4]>) {
    if style == "circle" {
        let mut pb = PathBuilder::new();
        pb.push_circle(x, y, size / 2.0);
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint(fill.unwrap_or([0x33, 0x33, 0x33, 255])), FillRule::Winding, Transform::identity(), None);
            if let Some(c) = stroke {
                pm.stroke_path(&path, &paint(c), &Stroke { width: (size / 14.0).max(1.0), ..Stroke::default() }, Transform::identity(), None);
            }
        }
        icon_patch(pm, name, x, y, size * 0.62, [255, 255, 255, 255], None);
    } else {
        icon_patch(pm, name, x, y, size, fill.or(stroke).unwrap_or([0x33, 0x33, 0x33, 255]), stroke.filter(|_| fill.is_some()));
    }
}

/// An icon drawn in a little picture of its own, laid on the map, so that a PDF of the map
/// gets it too.
fn icon_patch(pm: &mut Canvas, name: &str, x: f32, y: f32, size: f32, colour: [u8; 4], halo: Option<[u8; 4]>) {
    let side = (size.ceil() as u32 + 6).max(8);
    let (x0, y0) = ((x - side as f32 / 2.0).floor() as i32, (y - side as f32 / 2.0).floor() as i32);
    pm.patch(x0, y0, side, side, |p| crate::icons::draw(p, name, x - x0 as f32, y - y0 as f32, size, colour, halo));
}

/// A line of text with its baseline at y, from x, or with `align` of it before x: 0.5 has
/// its middle there. As the outlines of its letters, so that a PDF gets them as shapes.
#[allow(clippy::too_many_arguments)]
fn write_text(pm: &mut Canvas, t: &crate::chart::Text, s: &str, x: f32, y: f32, size: f32, bold: bool, rgb: [u8; 3], align: f32) {
    if let Some(path) = t.path(s, size, bold) {
        let w = t.width(s, size, bold);
        pm.fill_path(&path, &paint([rgb[0], rgb[1], rgb[2], 255]), FillRule::Winding, Transform::from_translate(x - w * align, y), None);
    }
}

/// What to draw: the layers from the bottom up, and how large.
#[derive(Clone)]
pub struct Request<'a> {
    pub layers: Vec<(&'a Table, LayerStyle)>,
    /// How each layer shows its values, in the order of `layers`. Empty: none does.
    pub themes: Vec<crate::thematic::Theme>,
    /// The labels of each layer, in the order of `layers`. Empty: none has labels.
    pub labels: Vec<Option<Labels>>,
    /// The icons of the points of each layer, in the order of `layers`. Empty: none.
    pub icons: Vec<Option<Icons>>,
    /// The charts on the features of each layer, in the order of `layers`. Empty: none.
    pub charts: Vec<Option<MapCharts>>,
    /// How each layer that carries a raster draws it, in the order of `layers`. A layer
    /// without one draws its features. Empty: none does.
    pub rasters: Vec<Option<crate::rasterlook::Prepared>>,
    /// A legend in a corner of the map.
    pub legend: Option<Legend>,
    /// Width of the picture in pixels. The height follows from the extent.
    pub width: u32,
    /// Space around the features, as a part of the larger side of the extent.
    pub margin: f64,
    pub background: Option<[u8; 4]>,
    /// A picture under the layers, as large as the map: a background map or a picture.
    pub backdrop: Option<RasterImage>,
    /// Who the background map is from, written small in the lower right corner.
    pub attribution: Option<String>,
    /// The scale bar, north arrow and grid of the map.
    pub furniture: Furniture,
    /// The names of the layers, in the order of `layers`, for the layers of a PDF or an
    /// SVG of the map. Empty: they are named Layer 1, Layer 2 and so on.
    pub names: Vec<String>,
    /// A small map in a corner that shows where the map lies.
    pub overview: Option<Overview>,
    /// A ready style, which rasters are drawn in too. Empty: as they are.
    pub map_style: String,
}

/// What is drawn on a map besides its layers: a scale bar, a north arrow and a grid with
/// coordinates in the edge.
#[derive(Debug, Clone, Default)]
pub struct Furniture {
    /// Written on the left of a band along the bottom, such as the sources of the data,
    /// with the credit on the right of it. Empty: no band.
    pub footer: String,
    /// The corner of the scale bar, as the legend has it. Empty or "none": no scale bar.
    pub scale_bar: String,
    /// The corner of the north arrow. Empty or "none": none.
    pub north: String,
    /// "lines": grid lines across the map; "ticks": only marks in the edge; empty: none.
    pub grid: String,
    /// Between the grid lines, in the units of the map. None: a round number by the size.
    pub grid_every: Option<f64>,
    /// Whether the coordinates are degrees, for the lengths of the scale bar.
    pub geographic: bool,
    /// Lines of latitude and longitude, in the coordinates of the map, for a grid of degrees
    /// on a map in another coordinate system, where they curve.
    pub graticule: Vec<GratLine>,
    /// Lines and outlines break where the labels go, so that the names are free.
    pub knockout: bool,
    /// The area drawn, west, south, east and north, when it is set, as by a scale on a
    /// paper or for an inset. None: around the features.
    pub extent: Option<[f64; 4]>,
    /// Written large at the top left, and under it smaller.
    pub title: String,
    pub subtitle: String,
    /// A picture in a corner, such as a logo, as large as it is to be drawn.
    pub logo: Option<RasterImage>,
    pub logo_corner: String,
    /// An enlarged map of part of the map, in a corner.
    pub inset: Option<Inset>,
    /// Points of lines and outlines closer than a pixel to the one before are left out.
    pub simplify: bool,
    /// Where the title and its subtitle go, as the legend: "top-left" when empty.
    pub title_place: String,
    /// A box behind the title and the subtitle: its colour and the colour of its edge. None:
    /// no box, or no edge.
    pub title_fill: Option<[u8; 4]>,
    pub title_line: Option<[u8; 4]>,
    /// The look of the title: a font file of one's own (empty: the built-in one), the sizes of
    /// the title and the subtitle on a map 800 pixels high (0: 26 and 15), their colours, and
    /// whether the title is not bold.
    pub title_font: String,
    pub title_size: f32,
    pub subtitle_size: f32,
    pub title_colour: Option<[u8; 4]>,
    pub subtitle_colour: Option<[u8; 4]>,
    pub title_regular: bool,
    /// The resolution the map is drawn at, for its scale. 0: as a screen, 96 to the inch.
    pub dpi: f32,
    /// The area the map is about: what is outside it is paler, and it can have a shadow or
    /// lines along its coast.
    pub focus: Vec<Geometry<f64>>,
    /// How much paler the rest is, from 0 to 1.
    pub focus_fade: f32,
    /// "shadow", "waterlines" or empty.
    pub focus_effect: String,
    /// Only the area the map is about is drawn: the rest is left empty.
    pub focus_only: bool,
    /// A frame around the map with a white margin, where the coordinates of a grid are written.
    pub frame: bool,
    /// A font file for all the text of the map, such as a TTF or an OTF. Empty: the built-in.
    pub map_font: String,
    /// Notes written on the map, each by a point it points to, in the units of the map.
    pub notes: Vec<(f64, f64, String)>,
    /// The scale bar: "line" for a line with ticks, else a bar of fields dark and light; how
    /// many fields (0: four); and "km" or "m" for its unit, else as fits.
    pub scale_style: String,
    pub scale_parts: usize,
    pub scale_unit: String,
    /// The north arrow: "simple" for a line with a head, "star" for a compass star, else an
    /// arrow half dark and half light.
    pub north_style: String,
    /// The scale also in figures, as 1:50 000: "both" under the bar, "number" in its place,
    /// else the bar alone.
    pub scale_number: String,
    /// A grid of squares named by letters across and numbers down, as A1, for an index of
    /// names: how many squares across. 0: none.
    pub index_grid: usize,
    /// The coordinates are written outside, by the frame: the map itself has none.
    pub coords_outside: bool,
    /// The colours as someone sees them who is "deuteranopia", "protanopia" or "tritanopia",
    /// or "grey". Empty: as they are.
    pub vision: String,
    /// Look at how well each label without a halo stands out from what is under it.
    pub check_contrast: bool,
    /// Labels found to stand out too little from what is under them.
    pub faint: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// The heights along a line, drawn as a profile in a corner.
    pub profile: Option<Profile>,
    /// A small globe in a corner with the map's place on it.
    pub globe: Option<Globe>,
    /// Names written in the middle of the cells of a grid, such as the names of map sheets:
    /// where, in the coordinates of the map, and the name.
    pub cell_names: Vec<((f64, f64), String)>,
}

/// The heights along a line, for a profile in a corner.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub corner: String,
    /// The distance from the start, in metres, and the height there.
    pub points: Vec<(f64, f64)>,
    /// What the line is, written over the profile.
    pub title: String,
    /// Where the line begins and ends on the map, marked A and B there and on the profile.
    pub ends: [(f64, f64); 2],
    /// Places along the line, each at its distance from the start, written over it.
    pub places: Vec<(f64, String)>,
    /// The colours of the grid the heights are from, so that the profile is filled as the
    /// map is coloured: the water in the blues of its depth.
    pub colours: Option<crate::rasterlook::Paint>,
    /// Its width, as a share of the width of the map.
    pub share: f32,
}

/// A globe in a corner, turned so that the map is in its middle.
#[derive(Debug, Clone, Default)]
pub struct Globe {
    pub corner: String,
    /// The middle of the map, longitude and latitude.
    pub centre: (f64, f64),
    /// The outline of the map, in longitude and latitude.
    pub area: Vec<(f64, f64)>,
}

/// An enlarged map of an area of the map, in a corner, with a frame on the map where it is.
#[derive(Debug, Clone, Default)]
pub struct Inset {
    /// West, south, east and north, in the coordinates of the map.
    pub area: [f64; 4],
    pub corner: String,
    /// Its width, as a share of the width of the map.
    pub share: f32,
}

/// A line of the grid of degrees: its points in the coordinates of the map, its degree as text,
/// and whether it runs north and south (a meridian) or east and west.
#[derive(Debug, Clone, Default)]
pub struct GratLine {
    pub points: Vec<(f64, f64)>,
    pub label: String,
    pub meridian: bool,
}

/// Where a map lies and how large it is: west, south, east and north, the size of a pixel
/// and the picture in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub b: [f64; 4],
    pub px: f64,
    pub width: u32,
    pub height: u32,
}

impl Frame {
    /// The west and north edge, as the geotransform of a picture has them.
    pub fn geotransform(&self) -> [f64; 6] {
        [self.b[0], self.px, 0.0, self.b[1] + self.height as f64 * self.px, 0.0, -self.px]
    }
}

/// The frame of a map of these layers, as wide as asked, with the margin around them.
/// The frame of a map: around the features, or of an area set beforehand.
pub fn frame_in(layers: &[(&Table, LayerStyle)], width: u32, margin: f64, fixed: Option<[f64; 4]>) -> Result<Frame> {
    let Some(b) = fixed else { return frame(layers, width, margin) };
    let width = width.clamp(1, MAX_SIDE);
    let px = (b[2] - b[0]).max(f64::MIN_POSITIVE) / width as f64;
    let height = ((b[3] - b[1]) / px).round().max(1.0);
    if height > MAX_SIDE as f64 {
        return Err(GbError(format!("The picture would be {} pixels high, and at most {} are allowed.", height as u64, MAX_SIDE)));
    }
    Ok(Frame { b, px, width, height: height as u32 })
}

/// West, south, east and north of all the features of the layers, or None when none has a
/// geometry.
pub fn extent_of(layers: &[(&Table, LayerStyle)]) -> Option<[f64; 4]> {
    extent(layers)
}

pub fn frame(layers: &[(&Table, LayerStyle)], width: u32, margin: f64) -> Result<Frame> {
    let Some(mut b) = extent(layers) else {
        return Err(GbError("There is nothing to draw: the layers have no geometry.".into()));
    };
    // A single point, or features in a line, still get a picture with some room.
    let side = (b[2] - b[0]).max(b[3] - b[1]);
    let pad = if side > 0.0 { side * margin.max(0.0) } else { 100.0 };
    b = [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad];
    let width = width.clamp(1, MAX_SIDE);
    let px = (b[2] - b[0]) / width as f64;
    let height = ((b[3] - b[1]) / px).ceil().max(1.0);
    if height > MAX_SIDE as f64 {
        return Err(GbError(format!(
            "The picture would be {} pixels high, and at most {} are allowed. Make it narrower, or draw a smaller area.",
            height as u64, MAX_SIDE
        )));
    }
    Ok(Frame { b, px, width, height: height as u32 })
}

/// What a legend shows, and where.
#[derive(Debug, Clone, Default)]
pub struct Legend {
    /// "top-left", "top-right", "bottom-left" or "bottom-right".
    pub corner: String,
    pub title: String,
    /// From the top: a layer with its name, its look, and its keys if it shows values.
    pub items: Vec<LegendItem>,
    /// The style of the icons of each item, by its name: plain or circle.
    pub icon_styles: Vec<(String, String)>,
    /// The colour of its box and of the edge of the box: None for white and grey, Some(None)
    /// for none.
    pub fill: Option<Option<[u8; 4]>>,
    pub line: Option<Option<[u8; 4]>>,
    /// How many columns its rows are in. 0 or 1: one.
    pub columns: usize,
}

impl Legend {
    fn icon_style(&self, item: &LegendItem) -> String {
        self.icon_styles.iter().find(|(n, _)| *n == item.name).map_or_else(|| "plain".to_string(), |(_, s)| s.clone())
    }
}

#[derive(Debug, Clone, Default)]
pub struct LegendItem {
    pub name: String,
    pub style: LayerStyle,
    /// "point", "line" or "area": how its symbol is drawn.
    pub shape: &'static str,
    pub heading: String,
    pub keys: Vec<crate::thematic::Key>,
    pub size_heading: String,
    pub size_keys: Vec<crate::thematic::Key>,
    /// The icon of the points of the layer, and the icons of its values with what they
    /// stand for, when there are icons.
    pub icon: Option<&'static str>,
    pub icon_keys: Vec<(String, &'static str)>,
    /// The attributes of the charts on the features, each with its colour.
    pub chart_keys: Vec<(String, [u8; 4])>,
    /// The colour ramp of a heatmap, drawn as a band from little to most.
    pub heat_ramp: Option<String>,
    /// How many features each key has, drawn as bars under the keys. Empty: no bars.
    pub counts: Vec<usize>,
    /// Two attributes in a square of nine colours.
    pub bivariate: Option<crate::thematic::Bivariate>,
    /// The sizes as circles within each other, rather than a row each.
    pub nested: bool,
    /// A dot of a dot map and what it stands for.
    pub dots: Option<(crate::thematic::Look, String)>,
    /// A raster drawn with a ramp of colours, as a band with values under it.
    pub ramp: Option<crate::rasterlook::RampLegend>,
}

/// The kind of symbol a table is drawn with in a legend.
pub fn shape_of(t: &Table) -> &'static str {
    match t.features.iter().find_map(|f| f.geom.as_ref()) {
        Some(Geometry::Point(_) | Geometry::MultiPoint(_)) => "point",
        Some(Geometry::LineString(_) | Geometry::MultiLineString(_) | Geometry::Line(_)) => "line",
        _ => "area",
    }
}

fn paint(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], c[3]));
    p.anti_alias = true;
    p
}

/// The bounding box of the geometries of the layers, all in one coordinate system.
fn extent(layers: &[(&Table, LayerStyle)]) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (t, _) in layers {
        for f in &t.features {
            if let Some(r) = f.geom.as_ref().and_then(geo::BoundingRect::bounding_rect) {
                b = [b[0].min(r.min().x), b[1].min(r.min().y), b[2].max(r.max().x), b[3].max(r.max().y)];
            }
        }
    }
    b[0].is_finite().then_some(b)
}

/// Draws the raster of a layer: each pixel of the picture gets the cell under its middle.
/// `origin` is the west and north edge of the picture, and `px` the size of a pixel.
fn draw_raster(pixmap: &mut Canvas, table: &Table, r: &crate::rasterlook::Prepared, origin: [f64; 2], px: f64, style: &str) {
    use crate::rasterlook::Paint as RP;
    // The source of the colours: a picture, or the cells of a grid.
    let shaded;
    let mut relief = false;
    let (image, grid): (Option<&RasterImage>, Option<&crate::model::Grid>) = match (&r.paint, &table.grid, &table.image) {
        (RP::Image, _, Some(img)) => (Some(img.as_ref()), None),
        (RP::Hachures, Some(g), _) => {
            draw_hachures(pixmap, g, origin, px, r.opacity, style);
            return;
        }
        // Relief is lit by the cells around each one, so it is made as a picture first.
        // Steps are cut at the size of the pixels, so that their edges are smooth curves
        // rather than the squares of the cells.
        (RP::Bands { .. }, Some(g), _) => {
            let k = (g.geotransform[1].abs() / px).clamp(1.0, 6.0);
            let (nw, nh) = ((g.width as f64 * k).round() as usize, (g.height as f64 * k).round() as usize);
            shaded = if k > 1.05 && nw * nh <= 40_000_000 {
                crate::rasterlook::picture(&crate::terrain::resample(g, nw, nh), &r.paint)
            } else {
                crate::rasterlook::picture(g, &r.paint)
            };
            (Some(&shaded), None)
        }
        (RP::Shade | RP::Soft | RP::Relief { .. } | RP::Swiss { .. } | RP::Texture | RP::Depths { .. } | RP::Seafloor { .. }, Some(g), _) => {
            shaded = crate::rasterlook::picture(g, &r.paint);
            relief = true;
            (Some(&shaded), None)
        }
        (_, Some(g), _) => (None, Some(g.as_ref())),
        (_, None, Some(img)) => (Some(img.as_ref()), None),
        _ => return,
    };
    let gt = image.map_or_else(|| grid.map(|g| g.geotransform).unwrap_or_default(), |i| i.geotransform);
    let (w, h) = image.map_or_else(|| grid.map_or((0, 0), |g| (g.width, g.height)), |i| (i.width, i.height));
    if gt[1] == 0.0 || gt[5] == 0.0 || gt[2] != 0.0 || gt[4] != 0.0 || w == 0 || h == 0 {
        return;
    }
    let (pw, ph) = (pixmap.width() as usize, pixmap.height() as usize);
    // The pixels the raster covers.
    let xs = [gt[0], gt[0] + gt[1] * w as f64];
    let ys = [gt[3], gt[3] + gt[5] * h as f64];
    let c0 = (((xs[0].min(xs[1]) - origin[0]) / px).floor().max(0.0) as usize).min(pw);
    let c1 = (((xs[0].max(xs[1]) - origin[0]) / px).ceil().max(0.0) as usize).min(pw);
    let r0 = (((origin[1] - ys[0].max(ys[1])) / px).floor().max(0.0) as usize).min(ph);
    let r1 = (((origin[1] - ys[0].min(ys[1])) / px).ceil().max(0.0) as usize).min(ph);
    if c0 >= c1 || r0 >= r1 {
        return;
    }
    let Some(mut layer) = Pixmap::new(pw as u32, ph as u32) else { return };
    let data = layer.pixels_mut();
    // Relief made larger than its cells is blended between them, so that its slopes stay
    // smooth rather than stepped.
    let blend = relief && gt[1].abs() > px * 1.2;
    for row in r0..r1 {
        let y = origin[1] - (row as f64 + 0.5) * px;
        let sr = (y - gt[3]) / gt[5];
        if sr < 0.0 || sr >= h as f64 {
            continue;
        }
        for col in c0..c1 {
            let x = origin[0] + (col as f64 + 0.5) * px;
            let sc = (x - gt[0]) / gt[1];
            if sc < 0.0 || sc >= w as f64 {
                continue;
            }
            let k = sr as usize * w + sc as usize;
            let c = match (image, grid) {
                (Some(img), _) if blend => {
                    let (fx, fy) = ((sc - 0.5).max(0.0), (sr - 0.5).max(0.0));
                    let (x0, y0) = ((fx as usize).min(w - 1), (fy as usize).min(h - 1));
                    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                    let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
                    let at = |x: usize, y: usize| &img.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                    let corners = [at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1)];
                    if corners.iter().any(|p| p[3] == 0) {
                        let p = &img.rgba[k * 4..k * 4 + 4];
                        [p[0], p[1], p[2], p[3]]
                    } else {
                        let wts = [(1.0 - tx) * (1.0 - ty), tx * (1.0 - ty), (1.0 - tx) * ty, tx * ty];
                        [0, 1, 2, 3].map(|i| corners.iter().zip(wts).map(|(p, wt)| p[i] as f32 * wt).sum::<f32>().round() as u8)
                    }
                }
                (Some(img), _) => {
                    let p = &img.rgba[k * 4..k * 4 + 4];
                    [p[0], p[1], p[2], p[3]]
                }
                (None, Some(g)) => match crate::rasterlook::colour_of(&r.paint, g.values[k]) {
                    Some(c) => c,
                    None => continue,
                },
                _ => continue,
            };
            if c[3] == 0 {
                continue;
            }
            let c = if style.is_empty() { c } else { style_colour(style, c, Role::Picture) };
            data[row * pw + col] = tiny_skia::ColorU8::from_rgba(c[0], c[1], c[2], c[3]).premultiply();
        }
    }
    let paint = tiny_skia::PixmapPaint { opacity: r.opacity, quality: tiny_skia::FilterQuality::Nearest, ..Default::default() };
    pixmap.draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
}

/// Hachures over the map, in brown or as the style draws them: a stroke down each slope,
/// thicker where it is steeper.
fn draw_hachures(pixmap: &mut Canvas, g: &crate::model::Grid, origin: [f64; 2], px: f64, opacity: f32, style: &str) {
    let k = (pixmap.width().min(pixmap.height()) as f32 / 800.0).clamp(0.6, 4.0);
    let strokes = crate::rasterlook::hachures(g, origin, px, (pixmap.width() as usize, pixmap.height() as usize), 5.5 * k);
    let ink = [0x5a, 0x44, 0x30, (255.0 * opacity.clamp(0.0, 1.0)) as u8];
    let ink = if style.is_empty() { ink } else { style_colour(style, ink, Role::Line) };
    // In a few widths, each drawn at once.
    let mut by_width: Vec<PathBuilder> = (0..6).map(|_| PathBuilder::new()).collect();
    for (a, b, t) in strokes {
        let j = ((t * 5.0).round() as usize).min(5);
        by_width[j].move_to(a[0], a[1]);
        by_width[j].line_to(b[0], b[1]);
    }
    for (j, pb) in by_width.into_iter().enumerate() {
        let Some(path) = pb.finish() else { continue };
        let width = (0.25 + 0.42 * j as f32) * k;
        pixmap.stroke_path(&path, &paint(ink), &Stroke { width, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() }, Transform::identity(), None);
    }
}

#[path = "mapdraw_extra.rs"]
mod extra;

/// Draws the layers. `going` is asked now and then whether to go on, and the drawing ends
/// with an error when it answers false.
pub fn draw(req: &Request, going: &dyn Fn() -> bool) -> Result<RasterImage> {
    let mut img = draw_framed(req, going)?;
    if !req.furniture.vision.is_empty() {
        extra::as_seen_by(&mut img.rgba, &req.furniture.vision);
        // The shapes for a PDF keep their own colours; the picture is what is to be seen.
        img.vector = None;
    }
    Ok(img)
}

fn draw_framed(req: &Request, going: &dyn Fn() -> bool) -> Result<RasterImage> {
    // Every text of the map in its own font, when it has one.
    let _font = crate::chart::own_font(&req.furniture.map_font)?;
    if !req.furniture.frame {
        return draw_map(req, going);
    }
    // The map inside a frame, with a margin around it for the coordinates.
    let wide = req.width as f32;
    let k = (wide / 1100.0).clamp(0.7, 4.0);
    let m = (wide * 0.035).max(34.0 * k).round();
    // At a scale the map keeps its width, and the margin comes outside it.
    let (inner_w, wide) = if req.furniture.extent.is_some() { (req.width, wide + 2.0 * m) } else { ((wide - 2.0 * m).max(40.0) as u32, wide) };
    let gridded = (!req.furniture.grid.is_empty() && req.furniture.grid != "none") || !req.furniture.graticule.is_empty();
    let indexed = req.furniture.index_grid > 0;
    let sub = Request { width: inner_w, furniture: Furniture { frame: false, coords_outside: gridded || indexed, ..req.furniture.clone() }, ..req.clone() };
    let img = draw_map(&sub, going)?;
    let (iw, ih) = (img.width as f32, img.height as f32);
    let high = (ih + 2.0 * m).round() as u32;
    let mut pixmap = Canvas::new(wide as u32, high, true).ok_or_else(|| GbError("There is not memory enough for a picture that large.".into()))?;
    pixmap.layer(Some("Frame"));
    pixmap.fill(Color::from_rgba8(255, 255, 255, 255));
    pixmap.draw_picture(&img, m, m);
    pixmap.layer(Some("Frame"));
    let ink = [0x22, 0x22, 0x22, 255];
    if let Some(r) = tiny_skia::Rect::from_xywh(m, m, iw, ih) {
        pixmap.stroke_path(&PathBuilder::from_rect(r), &paint(ink), &Stroke { width: (1.2 * k).max(1.0), ..Stroke::default() }, Transform::identity(), None);
    }
    // The coordinates outside, with a short tick in to the frame: east above and below it,
    // north on either side, turned to run along it.
    let gt = img.geotransform;
    let px = gt[1];
    let b = [gt[0], gt[3] - ih as f64 * px, gt[0] + iw as f64 * px, gt[3]];
    let t = crate::chart::Text::new()?;
    let small = 10.5 * k;
    let tick = 5.0 * k;
    let mut marks: Vec<(f32, String, bool)> = Vec::new();
    let f = &req.furniture;
    if indexed {
        // The names of the squares of the index, in the middle of each, rather than
        // coordinates.
        let (cell, cols, rows) = index_cells(iw, ih, f.index_grid);
        for c in 0..cols {
            marks.push((((c as f32 + 0.5) * cell).min(iw), index_column(c), true));
        }
        for r in 0..rows {
            marks.push((((r as f32 + 0.5) * cell).min(ih), (r + 1).to_string(), false));
        }
    } else if !f.graticule.is_empty() {
        let to = |(x, y): (f64, f64)| (((x - b[0]) / px) as f32, ((b[3] - y) / px) as f32);
        for g in &f.graticule {
            for w in g.points.windows(2) {
                let ((x0, y0), (x1, y1)) = (to(w[0]), to(w[1]));
                if g.meridian && y0 * y1 <= 0.0 && y0 != y1 {
                    marks.push((x0 + (x1 - x0) * (0.0 - y0) / (y1 - y0), g.label.clone(), true));
                    break;
                }
                if !g.meridian && x0 * x1 <= 0.0 && x0 != x1 {
                    marks.push((y0 + (y1 - y0) * (0.0 - x0) / (x1 - x0), g.label.clone(), false));
                    break;
                }
            }
        }
    } else if gridded {
        let step = f.grid_every.filter(|s| *s > 0.0).unwrap_or_else(|| crate::chart::nice_step((b[2] - b[0]).max(b[3] - b[1]), 5.0));
        if (b[2] - b[0]) / step <= 200.0 && (b[3] - b[1]) / step <= 200.0 {
            let mut x = (b[0] / step).ceil() * step;
            while x <= b[2] {
                marks.push((((x - b[0]) / px) as f32, coordinate_text(x, step, f.geographic), true));
                x += step;
            }
            let mut y = (b[1] / step).ceil() * step;
            while y <= b[3] {
                marks.push((((b[3] - y) / px) as f32, coordinate_text(y, step, f.geographic), false));
                y += step;
            }
        }
    }
    let mut ticks = PathBuilder::new();
    for (at, s, east) in &marks {
        let w = t.width(s, small, false);
        if *east {
            let x = m + at;
            if x - w / 2.0 < 2.0 || x + w / 2.0 > wide - 2.0 {
                continue;
            }
            ticks.move_to(x, m - tick);
            ticks.line_to(x, m);
            ticks.move_to(x, m + ih);
            ticks.line_to(x, m + ih + tick);
            write_text(&mut pixmap, &t, s, x - w / 2.0, m - tick - small * 0.45, small, false, [0x22, 0x22, 0x22], 0.0);
            write_text(&mut pixmap, &t, s, x - w / 2.0, m + ih + tick + small * 1.05, small, false, [0x22, 0x22, 0x22], 0.0);
        } else {
            let y = m + at;
            if y - w / 2.0 < 2.0 || y + w / 2.0 > high as f32 - 2.0 {
                continue;
            }
            ticks.move_to(m - tick, y);
            ticks.line_to(m, y);
            ticks.move_to(m + iw, y);
            ticks.line_to(m + iw + tick, y);
            if let Some(path) = t.path(s, small, false) {
                let left = Transform::from_rotate(-90.0).post_translate(m - tick - small * 0.45, y + w / 2.0);
                pixmap.fill_path(&path, &paint(ink), FillRule::Winding, left, None);
                let right = Transform::from_rotate(90.0).post_translate(m + iw + tick + small * 0.45, y - w / 2.0);
                pixmap.fill_path(&path, &paint(ink), FillRule::Winding, right, None);
            }
        }
    }
    if let Some(path) = ticks.finish() {
        pixmap.stroke_path(&path, &paint(ink), &Stroke { width: (1.0 * k).max(0.8), ..Stroke::default() }, Transform::identity(), None);
    }
    Ok(finish(pixmap, [gt[0] - m as f64 * px, px, 0.0, gt[3] + m as f64 * px, 0.0, -px]))
}

/// The map itself, without a frame around it.
fn draw_map(req: &Request, going: &dyn Fn() -> bool) -> Result<RasterImage> {
    CHECK.with(|c| c.set(req.furniture.check_contrast));
    let uphill = req.labels.iter().flatten().any(|l| l.spec.uphill);
    let grid = if uphill { req.layers.iter().find_map(|(t, _)| t.grid.clone()) } else { None };
    if let Some(g) = grid {
        let Frame { b, px, height, .. } = frame_in(&req.layers, req.width, req.margin, req.furniture.extent)?;
        HEIGHTS.with(|h| *h.borrow_mut() = Some((g, b[0], b[1] + height as f64 * px, px)));
    }
    FAINT.with(|f| f.borrow_mut().clear());
    let out = draw_map_inner(req, going);
    let faint = FAINT.with(|f| std::mem::take(&mut *f.borrow_mut()));
    if !faint.is_empty() {
        if let Ok(mut list) = req.furniture.faint.lock() {
            list.extend(faint);
        }
    }
    CHECK.with(|c| c.set(false));
    HEIGHTS.with(|h| *h.borrow_mut() = None);
    out
}

fn draw_map_inner(req: &Request, going: &dyn Fn() -> bool) -> Result<RasterImage> {
    let Frame { b, px, width, height } = frame_in(&req.layers, req.width, req.margin, req.furniture.extent)?;
    // The extent is made as high as the whole number of rows.
    let top = b[1] + height as f64 * px;
    let mut pixmap = Canvas::new(width, height, true).ok_or_else(|| GbError("There is not memory enough for a picture that large.".into()))?;
    let name_of = |li: usize| req.names.get(li).filter(|n| !n.trim().is_empty()).cloned().unwrap_or_else(|| format!("Layer {}", li + 1));
    // Drawn by hand: each point a little off where it is, the same for the same point, and
    // smoothly so along a line, as a steady hand that is not a ruler.
    let hand = req.map_style == "hand";
    let wobble = (width.min(height) as f32 / 900.0).clamp(0.6, 3.0) * 1.4;
    let to_px = |x: f64, y: f64| {
        let (u, v) = (((x - b[0]) / px) as f32, ((top - y) / px) as f32);
        if hand {
            let (a, c) = (extra::wavy(u / 55.0, v / 55.0), extra::wavy(v / 55.0 + 17.3, u / 55.0 - 4.1));
            (u + a * wobble, v + c * wobble)
        } else {
            (u, v)
        }
    };
    // Only the area the map is about: the layers and the background map are clipped to it.
    let clipped = req.furniture.focus_only && !req.furniture.focus.is_empty();
    if clipped {
        if let Some(path) = focus_path(&req.furniture, &to_px) {
            pixmap.set_clip(Some(&path), true);
        }
    }
    pixmap.layer(Some("Background"));
    if let Some(c) = req.background {
        pixmap.fill(Color::from_rgba8(c[0], c[1], c[2], c[3]));
    }
    if let Some(bd) = req.backdrop.as_ref().filter(|bd| bd.width == width as usize && bd.height == height as usize) {
        // Premultiplied, as tiny-skia keeps its pixels.
        let mut data = Vec::with_capacity(bd.rgba.len());
        for p in bd.rgba.chunks_exact(4) {
            let a = p[3] as u16;
            data.extend_from_slice(&[(p[0] as u16 * a / 255) as u8, (p[1] as u16 * a / 255) as u8, (p[2] as u16 * a / 255) as u8, p[3]]);
        }
        if let Some(under) = tiny_skia::IntSize::from_wh(width, height).and_then(|size| Pixmap::from_vec(data, size)) {
            pixmap.draw_pixmap(0, 0, under.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
        }
    }
    // The scale, for the layers that are drawn only at some scales.
    let scale_now = scale_of(&req.furniture, px, (b[1] + top) / 2.0);
    let shown = |li: usize| req.layers[li].1.shown_at(scale_now);
    // The legend without the layers that are not drawn at this scale.
    let hidden: Vec<String> = (0..req.layers.len()).filter(|li| !shown(*li)).map(name_of).collect();
    let legend: Option<Legend> =
        req.legend.as_ref().map(|l| Legend { items: l.items.iter().filter(|i| !hidden.contains(&i.name)).cloned().collect(), ..l.clone() });
    let credit = req.attribution.as_ref().filter(|a| !a.trim().is_empty());
    let main = [b[0], b[1], b[0] + width as f64 * px, top];
    // Where everything around the map goes, before anything is drawn.
    let mut layout = plan(&mut pixmap, req, legend.as_ref(), main, px, credit.is_some())?;
    // The notes, where they keep clear of the rest; the labels keep clear of them.
    let notes = place_notes(&pixmap, &req.furniture, &to_px, &layout.boxes)?;
    layout.boxes.extend(notes.iter().map(|n| n.2));
    // A shadow or lines along the coast of the area the map is about, under the layers.
    if !req.furniture.focus.is_empty() && !req.furniture.focus_effect.is_empty() {
        pixmap.layer(Some("Focus"));
        draw_focus_under(&mut pixmap, &req.furniture, &to_px, req.background);
    }
    let thin = req.furniture.simplify;
    let ring = |pb: &mut PathBuilder, ls: &geo::LineString<f64>, close: bool| {
        let mut first = true;
        let mut last = (0.0f32, 0.0f32);
        let n = ls.0.len();
        for (i, c) in ls.0.iter().enumerate() {
            let (x, y) = to_px(c.x, c.y);
            if first {
                pb.move_to(x, y);
                first = false;
                last = (x, y);
            } else {
                // Simplified to the scale: a point closer than a pixel to the one before is
                // left out, but never the last.
                if thin && i + 1 < n && (x - last.0).abs() < 1.0 && (y - last.1).abs() < 1.0 {
                    continue;
                }
                pb.line_to(x, y);
                last = (x, y);
            }
        }
        if close && !first {
            pb.close();
        }
    };
    let polygon = |pb: &mut PathBuilder, p: &geo::Polygon<f64>| {
        ring(pb, p.exterior(), true);
        for h in p.interiors() {
            ring(pb, h, true);
        }
    };
    // Where the labels will go, found first when lines are to break under them.
    if req.furniture.knockout && req.labels.iter().any(|l| l.is_some()) {
        let text = crate::chart::Text::new()?;
        let mut taken: Vec<[f32; 4]> = layout.boxes.clone();
        for li in label_order(req).into_iter().filter(|li| shown(*li)) {
            let Some(lb) = req.labels.get(li).and_then(|l| l.as_ref()) else { continue };
            let (table, style) = &req.layers[li];
            let point_r = match req.icons.get(li).and_then(|i| i.as_ref()) {
                Some(_) => icon_size(style.size) / 2.0,
                None => style.size / 2.0,
            };
            place_labels(&mut pixmap, &text, table, lb, point_r, &to_px, &mut taken, true);
        }
        pixmap.set_holes(taken);
    }
    for (li, (table, style)) in req.layers.iter().enumerate() {
        if !going() {
            return Err(GbError(crate::model::STOPPED.into()));
        }
        if !shown(li) {
            continue;
        }
        pixmap.set_blend(&style.blend);
        if !style.glow.is_empty() {
            draw_glow(&mut pixmap, table, style, &to_px);
        }
        if style.waterlines {
            draw_waterlines(&mut pixmap, table, &to_px, req.background);
        }
        // Each layer is a layer of its own in a PDF or an SVG of the map, and as
        // see-through as its style says.
        pixmap.layer(Some(&name_of(li)));
        pixmap.set_opacity(style.opacity);
        // A raster is drawn cell by cell, rather than its outline, with its own opacity.
        if let Some(r) = req.rasters.get(li).and_then(|r| r.as_ref()) {
            pixmap.set_opacity(1.0);
            draw_raster(&mut pixmap, table, r, [b[0], top], px, &req.map_style);
            continue;
        }
        // Points with icons are drawn as icons, after the rest of the layer.
        let icons = req.icons.get(li).and_then(|i| i.as_ref());
        let without_points;
        let table: &Table = match icons {
            Some(ic) => {
                let mut t = (*table).clone();
                for (f, name) in t.features.iter_mut().zip(&ic.names) {
                    if name.is_some() && point_of(f.geom.as_ref().unwrap_or(&Geometry::GeometryCollection(Default::default()))).is_some() {
                        f.geom = None;
                    }
                }
                without_points = t;
                &without_points
            }
            None => table,
        };
        let theme = req.themes.get(li);
        // A heatmap of the points of the layer, in place of the points.
        if let Some(theme) = theme.filter(|t| t.heat > 0.0) {
            draw_heat(&mut pixmap, table, theme, &to_px);
            continue;
        }
        // A layer that shows its values is drawn a feature at a time, in its own order.
        if let Some(theme) = theme.filter(|t| t.looks.is_some() || t.order.is_some() || t.cluster > 0.0 || t.angles.is_some()) {
            draw_themed(&mut pixmap, table, style, theme, &to_px, going)?;
            draw_dots(&mut pixmap, table, theme, &to_px);
            draw_after_layer(&mut pixmap, table, style, &to_px);
            if let Some(ic) = icons {
                draw_icons(&mut pixmap, req.layers[li].0, style, Some(theme), ic, &to_px);
            }
            if let Some(ch) = req.charts.get(li).and_then(|c| c.as_ref()) {
                draw_map_charts(&mut pixmap, req.layers[li].0, ch, &to_px);
            }
            continue;
        }
        let stroke = style.stroke();
        // Lines of their own widths, tunnels, and areas each as see-through as its value
        // says, are drawn one by one.
        let widths = extra::widths(table, style);
        let alphas = extra::alphas(table, style);
        let own: std::cell::Cell<Option<usize>> = std::cell::Cell::new(None);
        draw_casing(&mut pixmap, table, style, &to_px);
        // Borders are drawn after, with their dashes fitted to the corners.
        let line_paint = style.stroke.filter(|_| style.width > 0.0 && !extra::is_border(style)).map(paint);
        let fill_paint = style.fill.map(paint);
        // Lines and points are gathered in batches and drawn a batch at a time, which is
        // much quicker than one at a time. Areas are drawn one by one, so that each is
        // filled on its own and areas that overlap do not cancel out.
        let mut lines = PathBuilder::new();
        let mut dots = PathBuilder::new();
        let mut in_batch = 0usize;
        let mut heads: Vec<Head> = Vec::new();
        let head_len = head_length(style, style.width);
        let radius = (style.size / 2.0).max(0.5);
        let flush = |pixmap: &mut Canvas, lines: &mut PathBuilder, dots: &mut PathBuilder| {
            let (l, d) = (std::mem::take(lines), std::mem::take(dots));
            if let (Some(path), Some(p)) = (l.finish(), &line_paint) {
                pixmap.stroke_path(&path, p, &stroke, Transform::identity(), None);
                // The white dashes of a railway, the ticks of a tramway.
                if let Some((st, c)) = style.stroke.and_then(|c| style.overlay(c)) {
                    pixmap.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
                }
            }
            if let Some(path) = d.finish() {
                if let Some(p) = &fill_paint {
                    pixmap.fill_path(&path, p, FillRule::Winding, Transform::identity(), None);
                }
                if let Some(p) = &line_paint {
                    pixmap.stroke_path(&path, p, &stroke, Transform::identity(), None);
                }
            }
        };
        let add = |pixmap: &mut Canvas, g: &Geometry<f64>, lines: &mut PathBuilder, dots: &mut PathBuilder, in_batch: &mut usize, heads: &mut Vec<Head>| {
            if !style.arrows.is_empty() || style.gap > 0.0 {
                if let Some(parts) = arrowed_lines(g, &to_px, &style.arrows, head_len, style.gap, heads) {
                    polyline_path(&parts, lines);
                    *in_batch += 1;
                    return;
                }
            }
            if let Some(k) = own.get() {
                if matches!(g, Geometry::LineString(_) | Geometry::MultiLineString(_) | Geometry::Line(_))
                    && (widths.is_some() || style.taper || extra::is_tunnel(style, k))
                {
                    extra::own_line(pixmap, g, style, widths.as_ref().map_or(1.0, |w| w[k]), extra::is_tunnel(style, k), &to_px);
                    return;
                }
            }
            match g {
                Geometry::Point(p) => {
                    let (x, y) = to_px(p.x(), p.y());
                    dots.push_circle(x, y, radius);
                }
                Geometry::MultiPoint(m) => {
                    for p in &m.0 {
                        let (x, y) = to_px(p.x(), p.y());
                        dots.push_circle(x, y, radius);
                    }
                }
                Geometry::LineString(l) => ring(lines, l, false),
                Geometry::Line(l) => ring(lines, &geo::LineString::from(*l), false),
                Geometry::MultiLineString(m) => m.0.iter().for_each(|l| ring(lines, l, false)),
                Geometry::Polygon(_) | Geometry::MultiPolygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => {
                    let mut pb = PathBuilder::new();
                    match g {
                        Geometry::Polygon(p) => polygon(&mut pb, p),
                        Geometry::MultiPolygon(m) => m.0.iter().for_each(|p| polygon(&mut pb, p)),
                        Geometry::Rect(r) => polygon(&mut pb, &r.to_polygon()),
                        Geometry::Triangle(t) => polygon(&mut pb, &t.to_polygon()),
                        _ => {}
                    }
                    if let Some(path) = pb.finish() {
                        let faint = own.get().and_then(|k| alphas.as_ref().map(|a| a[k]));
                        if let (Some(c), Some(a)) = (style.fill, faint) {
                            pixmap.fill_path(&path, &paint([c[0], c[1], c[2], (c[3] as f32 * a) as u8]), FillRule::EvenOdd, Transform::identity(), None);
                        } else if let Some(p) = &fill_paint {
                            pixmap.fill_path(&path, p, FillRule::EvenOdd, Transform::identity(), None);
                        }
                        draw_pattern(pixmap, g, style, &to_px);
                        if let Some(p) = &line_paint {
                            pixmap.stroke_path(&path, p, &stroke, Transform::identity(), None);
                        }
                    }
                }
                Geometry::GeometryCollection(_) => {}
            }
            *in_batch += 1;
        };
        for (k, f) in table.features.iter().enumerate() {
            if k % 10_000 == 0 && k > 0 && !going() {
                return Err(GbError(crate::model::STOPPED.into()));
            }
            let Some(g) = &f.geom else { continue };
            own.set(Some(k));
            match g {
                Geometry::GeometryCollection(c) => {
                    for part in &c.0 {
                        add(&mut pixmap, part, &mut lines, &mut dots, &mut in_batch, &mut heads);
                    }
                }
                _ => add(&mut pixmap, g, &mut lines, &mut dots, &mut in_batch, &mut heads),
            }
            if in_batch >= 2_000 {
                flush(&mut pixmap, &mut lines, &mut dots);
                in_batch = 0;
            }
        }
        flush(&mut pixmap, &mut lines, &mut dots);
        if let Some(c) = style.stroke.filter(|_| style.width > 0.0) {
            draw_heads(&mut pixmap, &heads, style.width, c);
        }
        if let Some(theme) = theme {
            draw_dots(&mut pixmap, table, theme, &to_px);
        }
        extra::bridges_and_arrows(&mut pixmap, table, style, &to_px);
        draw_after_layer(&mut pixmap, table, style, &to_px);
        if let Some(ic) = icons {
            draw_icons(&mut pixmap, req.layers[li].0, style, theme, ic, &to_px);
        }
        if let Some(ch) = req.charts.get(li).and_then(|c| c.as_ref()) {
            draw_map_charts(&mut pixmap, req.layers[li].0, ch, &to_px);
        }
    }
    pixmap.set_opacity(1.0);
    pixmap.set_blend("");
    pixmap.set_holes(Vec::new());
    // The rest paler than the area the map is about.
    if !req.furniture.focus.is_empty() && (req.furniture.focus_fade > 0.0 || req.furniture.focus_only) {
        pixmap.layer(Some("Focus"));
        draw_focus_over(&mut pixmap, &req.furniture, &to_px, req.background);
    }
    // The labels, on top of every layer: those of the top layer first, so that they get
    // the room when there is not enough for all.
    if req.labels.iter().any(|l| l.is_some()) {
        let text = crate::chart::Text::new()?;
        let mut taken: Vec<[f32; 4]> = layout.boxes.clone();
        for li in label_order(req).into_iter().filter(|li| shown(*li)) {
            let Some(lb) = req.labels.get(li).and_then(|l| l.as_ref()) else { continue };
            let (table, style) = &req.layers[li];
            let point_r = match req.icons.get(li).and_then(|i| i.as_ref()) {
                Some(_) => icon_size(style.size) / 2.0,
                None => style.size / 2.0,
            };
            pixmap.layer(Some(&format!("{} labels", name_of(li))));
            place_labels(&mut pixmap, &text, table, lb, point_r, &to_px, &mut taken, false);
        }
    }
    // The squares of an index, with their names inside the edges when no frame has them.
    if req.furniture.index_grid > 0 {
        pixmap.layer(Some("Index grid"));
        draw_index_grid(&mut pixmap, req.furniture.index_grid, !req.furniture.coords_outside)?;
    }
    // The labels too were within the area; what goes around the map is not.
    if clipped {
        pixmap.set_clip(None, false);
    }
    if !notes.is_empty() {
        pixmap.layer(Some("Notes"));
        draw_notes(&mut pixmap, &notes)?;
    }
    // An enlarged map of part of the map, in a corner, and a frame where it is.
    if let Some(inset) = &req.furniture.inset {
        pixmap.layer(Some("Inset"));
        draw_inset(&mut pixmap, req, inset, &to_px, going, layout.inset)?;
    }
    if let (Some(pr), Some(at)) = (&req.furniture.profile, layout.profile) {
        pixmap.layer(Some("Profile"));
        extra::draw_profile(&mut pixmap, pr, at)?;
        extra::profile_ends(&mut pixmap, pr, &to_px)?;
    }
    if let (Some(g), Some(at)) = (&req.furniture.globe, layout.globe) {
        pixmap.layer(Some("Globe"));
        extra::draw_globe(&mut pixmap, g, at);
    }
    if !req.furniture.cell_names.is_empty() {
        extra::draw_cell_names(&mut pixmap, &req.furniture.cell_names, &to_px)?;
    }
    pixmap.layer(Some("Frame and legend"));
    draw_title_and_logo(&mut pixmap, &req.furniture, layout.logo, layout.title)?;
    draw_furniture(&mut pixmap, &req.furniture, main, px, &layout)?;
    if let Some(ov) = &req.overview {
        draw_overview(&mut pixmap, ov, main, &req.layers, req.background, layout.overview);
    }
    if let Some(legend) = &legend {
        draw_legend_at(&mut pixmap, legend, layout.legend, false)?;
    }
    if !req.furniture.footer.trim().is_empty() {
        draw_footer(&mut pixmap, req.furniture.footer.trim(), credit.map(|s| s.as_str()))?;
    } else if let Some(a) = credit {
        draw_attribution(&mut pixmap, a)?;
    }
    // tiny-skia keeps the colours premultiplied by their opacity. Files want them plain.
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let vector = pixmap.picture().map(std::sync::Arc::new);
    Ok(RasterImage { vector, width: width as usize, height: height as usize, rgba, geotransform: [b[0], px, 0.0, top, 0.0, -px] })
}

/// The squares of an index on a map `w` by `h` pixels, `across` of them across: their side,
/// and how many across and down.
fn index_cells(w: f32, h: f32, across: usize) -> (f32, usize, usize) {
    let cell = w / across.max(1) as f32;
    (cell, across.max(1), ((h / cell).ceil() as usize).max(1))
}

/// The name of a column of an index: A to Z, then AA, AB.
pub fn index_column(c: usize) -> String {
    let letters = |n: usize| ((b'A' + (n % 26) as u8) as char).to_string();
    if c < 26 {
        letters(c)
    } else {
        format!("{}{}", letters(c / 26 - 1), letters(c))
    }
}

/// The name of the square of an index a point falls in, as C4, on a map of the extent `b`.
pub fn index_square(b: [f64; 4], across: usize, x: f64, y: f64) -> Option<String> {
    if x < b[0] || x > b[2] || y < b[1] || y > b[3] || across == 0 {
        return None;
    }
    let cell = (b[2] - b[0]) / across as f64;
    let c = (((x - b[0]) / cell).floor() as usize).min(across - 1);
    let rows = ((b[3] - b[1]) / cell).ceil().max(1.0) as usize;
    let r = (((b[3] - y) / cell).floor() as usize).min(rows - 1);
    Some(format!("{}{}", index_column(c), r + 1))
}

/// Thin lines between the squares of an index, and their names along the top and left edge
/// when `names`.
fn draw_index_grid(pixmap: &mut Canvas, across: usize, names: bool) -> Result<()> {
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let (cell, cols, rows) = index_cells(pw, ph, across);
    let k = (pw.min(ph) / 800.0).clamp(0.6, 4.0);
    let mut pb = PathBuilder::new();
    for c in 1..cols {
        pb.move_to(c as f32 * cell, 0.0);
        pb.line_to(c as f32 * cell, ph);
    }
    for r in 1..rows {
        pb.move_to(0.0, r as f32 * cell);
        pb.line_to(pw, r as f32 * cell);
    }
    if let Some(p) = pb.finish() {
        pixmap.stroke_path(&p, &paint([0x55, 0x55, 0x55, 150]), &Stroke { width: (0.8 * k).max(0.6), ..Stroke::default() }, Transform::identity(), None);
    }
    if names {
        let t = crate::chart::Text::new()?;
        let size = 13.0 * k;
        let ink = [0x22, 0x22, 0x22, 255];
        for c in 0..cols {
            let s = index_column(c);
            let w = t.width(&s, size, true);
            halo_text(pixmap, &t, &s, (c as f32 + 0.5) * cell - w / 2.0, size * 1.2, size, true, ink);
        }
        for r in 0..rows {
            let s = (r + 1).to_string();
            halo_text(pixmap, &t, &s, size * 0.4, ((r as f32 + 0.5) * cell + size * 0.35).min(ph - 4.0), size, true, ink);
        }
    }
    Ok(())
}

/// The extent on the ground of the map itself, as it will be drawn: inside its frame, when it
/// has one.
pub fn map_extent(req: &Request) -> Result<[f64; 4]> {
    let width = if req.furniture.frame && req.furniture.extent.is_none() {
        let wide = req.width as f32;
        let k = (wide / 1100.0).clamp(0.7, 4.0);
        let m = (wide * 0.035).max(34.0 * k).round();
        (wide - 2.0 * m).max(40.0) as u32
    } else {
        req.width
    };
    let f = frame_in(&req.layers, width, req.margin, req.furniture.extent)?;
    Ok([f.b[0], f.b[1], f.b[0] + f.width as f64 * f.px, f.b[1] + f.height as f64 * f.px])
}

/// A note placed on the map: the point it is about, the box it is written in, and its text.
type Note = (f32, f32, [f32; 4], String);

/// The size of the text of notes on a picture of this size.
fn note_size(pw: f32, ph: f32) -> (f32, f32) {
    let k = (pw.min(ph) / 800.0).clamp(0.6, 4.0);
    (13.0 * k, k)
}

/// Where each note goes: its box up and to the right of its point, or another way round when
/// that is outside the map or on something else.
fn place_notes(pixmap: &Canvas, f: &Furniture, to_px: &dyn Fn(f64, f64) -> (f32, f32), taken: &[[f32; 4]]) -> Result<Vec<Note>> {
    if f.notes.is_empty() {
        return Ok(Vec::new());
    }
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let t = crate::chart::Text::new()?;
    let (size, k) = note_size(pw, ph);
    let mut taken = taken.to_vec();
    let mut out = Vec::new();
    for (x, y, text) in &f.notes {
        let (px, py) = to_px(*x, *y);
        let (w, h) = (t.width(text, size, false) + 14.0 * k, size * 1.7);
        let (dx, dy) = (34.0 * k, 30.0 * k);
        let tries = [(px + dx, py - dy - h), (px - dx - w, py - dy - h), (px + dx, py + dy), (px - dx - w, py + dy)];
        let inside = |b: [f32; 4]| b[0] >= 2.0 && b[1] >= 2.0 && b[2] <= pw - 2.0 && b[3] <= ph - 2.0;
        let free = |b: [f32; 4]| !taken.iter().any(|c| b[0] < c[2] && b[2] > c[0] && b[1] < c[3] && b[3] > c[1]);
        let boxes: Vec<[f32; 4]> = tries.iter().map(|(x0, y0)| [*x0, *y0, x0 + w, y0 + h]).collect();
        let Some(b) = boxes.iter().find(|b| inside(**b) && free(**b)).or_else(|| boxes.iter().find(|b| inside(**b))).copied() else { continue };
        taken.push(b);
        out.push((px, py, b, text.clone()));
    }
    Ok(out)
}

/// The notes: a dot at the point, a line to the box, and the text in the box.
fn draw_notes(pixmap: &mut Canvas, notes: &[Note]) -> Result<()> {
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let t = crate::chart::Text::new()?;
    let (size, k) = note_size(pw, ph);
    let ink = [0x22, 0x22, 0x22, 255];
    for (px, py, b, text) in notes {
        // From the point to the nearest corner of the box.
        let cx = if (b[0] - px).abs() < (b[2] - px).abs() { b[0] } else { b[2] };
        let cy = if (b[1] - py).abs() < (b[3] - py).abs() { b[1] } else { b[3] };
        let mut pb = PathBuilder::new();
        pb.move_to(*px, *py);
        pb.line_to(cx, cy);
        if let Some(p) = pb.finish() {
            pixmap.stroke_path(
                &p,
                &paint([255, 255, 255, 220]),
                &Stroke { width: 3.2 * k, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() },
                Transform::identity(),
                None,
            );
            pixmap.stroke_path(&p, &paint(ink), &Stroke { width: 1.1 * k, ..Stroke::default() }, Transform::identity(), None);
        }
        let mut dot = PathBuilder::new();
        dot.push_circle(*px, *py, 3.2 * k);
        if let Some(p) = dot.finish() {
            pixmap.fill_path(&p, &paint(ink), FillRule::Winding, Transform::identity(), None);
            pixmap.stroke_path(&p, &paint([255, 255, 255, 255]), &Stroke { width: 1.2 * k, ..Stroke::default() }, Transform::identity(), None);
        }
        if let Some(r) = tiny_skia::Rect::from_ltrb(b[0], b[1], b[2], b[3]) {
            let path = PathBuilder::from_rect(r);
            pixmap.fill_path(&path, &paint([255, 255, 255, 235]), FillRule::Winding, Transform::identity(), None);
            pixmap.stroke_path(&path, &paint(ink), &Stroke { width: 1.0 * k, ..Stroke::default() }, Transform::identity(), None);
        }
        write_text(pixmap, &t, text, b[0] + 7.0 * k, b[1] + size * 1.2, size, false, [0x11, 0x11, 0x11], 0.0);
    }
    Ok(())
}

/// Whether a value is a whole number of `every`, as the height of an index contour.
pub fn is_index(v: &crate::model::Value, every: f64) -> bool {
    v.as_f64().is_some_and(|x| every > 0.0 && ((x / every) - (x / every).round()).abs() < 1e-6)
}

/// After a layer is drawn: its index contours heavier, and a band inside the edge of its
/// areas.
fn draw_after_layer(pixmap: &mut Canvas, table: &Table, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let colour = style.stroke.unwrap_or([0x55, 0x55, 0x55, 255]);
    if let (Some(k), true) = (table.field_index(&style.index_by), style.index_every > 0.0) {
        let mut pb = PathBuilder::new();
        for f in &table.features {
            if !is_index(&f.attrs[k], style.index_every) {
                continue;
            }
            if let Some((p, false)) = f.geom.as_ref().and_then(|g| path_of(g, to_px)) {
                pb.push_path(&p);
            }
        }
        if let Some(path) = pb.finish() {
            let st = Stroke { width: (style.width * 2.2).max(1.4), line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
            pixmap.stroke_path(&path, &paint(colour), &st, Transform::identity(), None);
        }
    }
    if style.tanaka {
        extra::tanaka(pixmap, table, style, to_px);
    }
    if extra::is_border(style) {
        extra::borders(pixmap, table, style, to_px);
    }
    if !style.shield.is_empty() {
        extra::shields(pixmap, table, style, to_px);
    }
    if style.band > 0.0 {
        // Each area clipped to itself, so that only the inner half of a wide edge shows.
        let before = pixmap.clip();
        let band = [colour[0], colour[1], colour[2], 110];
        for f in &table.features {
            let Some((path, true)) = f.geom.as_ref().and_then(|g| path_of(g, to_px)) else { continue };
            pixmap.set_clip(Some(&path), true);
            pixmap.stroke_path(
                &path,
                &paint(band),
                &Stroke { width: style.band * 2.0, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() },
                Transform::identity(),
                None,
            );
        }
        match before {
            Some((p, even_odd)) => pixmap.set_clip(Some(&p), even_odd),
            None => pixmap.set_clip(None, false),
        }
    }
}

/// Thin lines along the shores of the areas of a layer, fading outwards, under the areas.
fn draw_waterlines(pixmap: &mut Canvas, table: &Table, to_px: &dyn Fn(f64, f64) -> (f32, f32), background: Option<[u8; 4]>) {
    use geo::Buffer;
    let areas: Vec<geo::Polygon<f64>> = table
        .features
        .iter()
        .filter_map(|f| f.geom.as_ref())
        .flat_map(|g| match g {
            Geometry::Polygon(p) => vec![p.clone()],
            Geometry::MultiPolygon(m) => m.0.clone(),
            _ => Vec::new(),
        })
        .collect();
    let Some(first) = areas.first().and_then(|p| p.exterior().0.first().copied()) else { return };
    // How many pixels a unit of the map is, where the areas are.
    let (a, b) = (to_px(first.x, first.y), to_px(first.x + 1.0, first.y));
    let per = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() as f64;
    if per <= 0.0 || !per.is_finite() {
        return;
    }
    let shape = geo::MultiPolygon::new(areas);
    let k = (pixmap.width().min(pixmap.height()) as f32 / 800.0).clamp(0.6, 4.0);
    let dark = background.is_some_and(|c| (c[0] as u16 + c[1] as u16 + c[2] as u16) < 300);
    let colour = if dark { [0xd8, 0xe6, 0xf0] } else { [0x4f, 0x7a, 0x9c] };
    // Each line is the edge of the areas grown by a little more, so that it keeps clear of
    // the coast and of the other lines in the bays.
    for i in 1..=4 {
        let gap = 2.6 * k * i as f32;
        let grown = shape.buffer(gap as f64 / per);
        let mut pb = PathBuilder::new();
        for p in &grown.0 {
            for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
                for (j, c) in ring.0.iter().enumerate() {
                    let (x, y) = to_px(c.x, c.y);
                    if j == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                pb.close();
            }
        }
        let Some(path) = pb.finish() else { continue };
        let a = (170.0 * (1.0 - (i - 1) as f32 / 4.0)) as u8;
        pixmap.stroke_path(&path, &paint([colour[0], colour[1], colour[2], a]), &Stroke { width: 0.7 * k, ..Stroke::default() }, Transform::identity(), None);
    }
}

/// A soft glow around the features of a layer, or a shadow under them, before they are drawn.
fn draw_glow(pixmap: &mut Canvas, table: &Table, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let k = (pixmap.width().min(pixmap.height()) as f32 / 800.0).clamp(0.6, 4.0);
    let (mut areas, mut lines) = (PathBuilder::new(), PathBuilder::new());
    let mut points = PathBuilder::new();
    for f in &table.features {
        let Some(g) = &f.geom else { continue };
        match g {
            Geometry::Point(p) => {
                let (x, y) = to_px(p.x(), p.y());
                points.push_circle(x, y, (style.size / 2.0).max(1.0));
            }
            Geometry::MultiPoint(m) => {
                for p in &m.0 {
                    let (x, y) = to_px(p.x(), p.y());
                    points.push_circle(x, y, (style.size / 2.0).max(1.0));
                }
            }
            _ => {
                if let Some((p, area)) = path_of(g, to_px) {
                    if area {
                        areas.push_path(&p);
                    } else {
                        lines.push_path(&p);
                    }
                }
            }
        }
    }
    let shadow = style.glow == "shadow";
    let c = style.glow_colour.unwrap_or(if shadow { [0x1e, 0x24, 0x2c, 255] } else { [255, 255, 255, 255] });
    let round = |w: f32| Stroke { width: w, line_join: tiny_skia::LineJoin::Round, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
    let line_w = style.width.max(1.0);
    for path in [areas.finish(), points.finish()].into_iter().flatten() {
        if shadow {
            for i in (1..=5).rev() {
                let d = i as f32 * 1.2 * k;
                pixmap.fill_path(&path, &paint([c[0], c[1], c[2], 26]), FillRule::EvenOdd, Transform::from_translate(d, d), None);
            }
        } else {
            for i in (1..=5).rev() {
                pixmap.stroke_path(&path, &paint([c[0], c[1], c[2], 34]), &round(i as f32 * 2.4 * k), Transform::identity(), None);
            }
        }
    }
    if let Some(path) = lines.finish() {
        for i in (1..=5).rev() {
            let (st, ts) = if shadow {
                let d = i as f32 * 1.0 * k;
                (round(line_w), Transform::from_translate(d, d))
            } else {
                (round(line_w + i as f32 * 2.4 * k), Transform::identity())
            };
            pixmap.stroke_path(&path, &paint([c[0], c[1], c[2], if shadow { 30 } else { 34 }]), &st, ts, None);
        }
    }
}

/// The dots of a dot map: strewn at random within each area, but the same each time.
fn draw_dots(pixmap: &mut Canvas, table: &Table, theme: &crate::thematic::Theme, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    use geo::Contains;
    let Some(dots) = &theme.dots else { return };
    let mut pb = PathBuilder::new();
    let r = (theme.dot_size / 2.0).max(0.4);
    for (i, f) in table.features.iter().enumerate() {
        let n = dots.get(i).copied().unwrap_or(0);
        let Some(g) = f.geom.as_ref().filter(|g| matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_))) else { continue };
        let Some(bb) = g.bounding_rect() else { continue };
        // A small generator of its own, seeded by the feature, so that the map is the same
        // each time it is drawn.
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15 ^ (i as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let (mut placed, mut tries) = (0, 0);
        while placed < n && tries < n * 60 + 100 {
            tries += 1;
            let q = geo::Point::new(bb.min().x + next() * bb.width(), bb.min().y + next() * bb.height());
            if g.contains(&q) {
                let (x, y) = to_px(q.x(), q.y());
                pb.push_circle(x, y, r);
                placed += 1;
            }
        }
    }
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, &paint(theme.dot_colour), FillRule::Winding, Transform::identity(), None);
    }
}

/// The scale of a map as the number after 1:, from the size of its pixels in its units.
pub fn scale_of(f: &Furniture, px: f64, mid_lat: f64) -> f64 {
    let dpi = if f.dpi > 0.0 { f.dpi as f64 } else { 96.0 };
    let metres = if f.geographic { px * 111_320.0 * mid_lat.to_radians().cos().abs().max(0.01) } else { px };
    metres * dpi / 0.0254
}

/// The area the map is about, as one path.
fn focus_path(f: &Furniture, to_px: &dyn Fn(f64, f64) -> (f32, f32)) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for g in &f.focus {
        if let Some((p, true)) = path_of(g, to_px) {
            pb.push_path(&p);
        }
    }
    pb.finish()
}

/// Under the layers: a soft shadow of the area, or lines along its coast that fade outwards.
fn draw_focus_under(pixmap: &mut Canvas, f: &Furniture, to_px: &dyn Fn(f64, f64) -> (f32, f32), background: Option<[u8; 4]>) {
    let Some(path) = focus_path(f, to_px) else { return };
    let k = (pixmap.width().min(pixmap.height()) as f32 / 800.0).clamp(0.6, 4.0);
    match f.focus_effect.as_str() {
        "shadow" => {
            for i in (1..=6).rev() {
                let d = i as f32 * 1.4 * k;
                pixmap.fill_path(&path, &paint([0x1e, 0x24, 0x2c, 22]), FillRule::EvenOdd, Transform::from_translate(d, d), None);
            }
        }
        "waterlines" => {
            // On a light background blue lines, on a dark one light lines.
            let dark = background.is_some_and(|c| (c[0] as u16 + c[1] as u16 + c[2] as u16) < 300);
            let colour = if dark { [0xd8, 0xe6, 0xf0] } else { [0x4f, 0x7a, 0x9c] };
            for i in 1..=5 {
                let gap = 3.2 * k * i as f32;
                let wide = Stroke { width: gap * 2.0, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                if let Some(ring) = path.stroke(&wide, 1.0) {
                    let a = (190.0 * (1.0 - (i - 1) as f32 / 5.0)) as u8;
                    pixmap.stroke_path(
                        &ring,
                        &paint([colour[0], colour[1], colour[2], a]),
                        &Stroke { width: 0.8 * k, ..Stroke::default() },
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
        _ => {}
    }
}

/// Over the layers: what lies outside the area paler, and the area outlined.
fn draw_focus_over(pixmap: &mut Canvas, f: &Furniture, to_px: &dyn Fn(f64, f64) -> (f32, f32), background: Option<[u8; 4]>) {
    let Some(path) = focus_path(f, to_px) else { return };
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    let mut pb = PathBuilder::new();
    pb.push_rect(tiny_skia::Rect::from_xywh(-2.0, -2.0, w + 4.0, h + 4.0).unwrap_or(tiny_skia::Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()));
    pb.push_path(&path);
    let c = background.unwrap_or([255, 255, 255, 255]);
    if let Some(outside) = pb.finish().filter(|_| !f.focus_only) {
        pixmap.fill_path(&outside, &paint([c[0], c[1], c[2], (f.focus_fade.clamp(0.0, 1.0) * 255.0) as u8]), FillRule::EvenOdd, Transform::identity(), None);
    }
    let k = (w.min(h) / 800.0).clamp(0.6, 4.0);
    pixmap.stroke_path(
        &path,
        &paint([0x33, 0x33, 0x33, 220]),
        &Stroke { width: 1.3 * k, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() },
        Transform::identity(),
        None,
    );
}

fn path_of(g: &Geometry<f64>, to_px: &dyn Fn(f64, f64) -> (f32, f32)) -> Option<(tiny_skia::Path, bool)> {
    let mut pb = PathBuilder::new();
    let ring = |pb: &mut PathBuilder, ls: &geo::LineString<f64>, close: bool| {
        for (k, c) in ls.0.iter().enumerate() {
            let (x, y) = to_px(c.x, c.y);
            if k == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if close && !ls.0.is_empty() {
            pb.close();
        }
    };
    let poly = |pb: &mut PathBuilder, p: &geo::Polygon<f64>| {
        ring(pb, p.exterior(), true);
        p.interiors().iter().for_each(|h| ring(pb, h, true));
    };
    let area = match g {
        Geometry::LineString(l) => {
            ring(&mut pb, l, false);
            false
        }
        Geometry::Line(l) => {
            ring(&mut pb, &geo::LineString::from(*l), false);
            false
        }
        Geometry::MultiLineString(m) => {
            m.0.iter().for_each(|l| ring(&mut pb, l, false));
            false
        }
        Geometry::Polygon(p) => {
            poly(&mut pb, p);
            true
        }
        Geometry::MultiPolygon(m) => {
            m.0.iter().for_each(|p| poly(&mut pb, p));
            true
        }
        Geometry::Rect(r) => {
            poly(&mut pb, &r.to_polygon());
            true
        }
        Geometry::Triangle(t) => {
            poly(&mut pb, &t.to_polygon());
            true
        }
        _ => return None,
    };
    pb.finish().map(|p| (p, area))
}

/// The point a feature is drawn at as a symbol: the point itself, the first of several.
fn point_of(g: &Geometry<f64>) -> Option<geo::Point<f64>> {
    match g {
        Geometry::Point(p) => Some(*p),
        Geometry::MultiPoint(m) => m.0.first().copied(),
        _ => None,
    }
}

/// The edges of the lines of a layer, all of them under all the lines, so that roads that
/// meet run into each other without a seam.
/// The pattern of a style over the areas of a geometry: lines or dots, cut to the area, so
/// that they are lines in a PDF or SVG too.
fn draw_pattern(pixmap: &mut Canvas, g: &Geometry<f64>, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    use geo::{BooleanOps, BoundingRect, Contains, MapCoords};
    if style.pattern.is_empty() {
        return;
    }
    let Some(colour) = style.pattern_colour.or(style.stroke).or(style.fill.map(|c| [c[0] / 2, c[1] / 2, c[2] / 2, c[3]])) else { return };
    let areas: Vec<geo::Polygon<f64>> = match g {
        Geometry::Polygon(p) => vec![p.clone()],
        Geometry::MultiPolygon(m) => m.0.clone(),
        Geometry::Rect(r) => vec![r.to_polygon()],
        Geometry::Triangle(t) => vec![t.to_polygon()],
        Geometry::GeometryCollection(c) => c.0.iter().filter_map(|g| if let Geometry::Polygon(p) = g { Some(p.clone()) } else { None }).collect(),
        _ => return,
    };
    let every = style.pattern_every.max(2.0) as f64;
    let width = (style.width * 0.6).clamp(0.5, every as f32 / 3.0);
    for area in areas {
        // In pixels, where the spacing is.
        let px = area.map_coords(|c| {
            let (x, y) = to_px(c.x, c.y);
            geo::Coord { x: x as f64, y: y as f64 }
        });
        let Some(b) = px.bounding_rect() else { continue };
        if b.width() < 1.0 || b.height() < 1.0 {
            continue;
        }
        if matches!(style.pattern.as_str(), "marsh" | "conifers" | "deciduous" | "orchard" | "rock" | "sand") {
            extra::symbol_pattern(pixmap, &px, b, &style.pattern, every as f32, colour);
            continue;
        }
        if style.pattern == "dots" {
            let mut pb = PathBuilder::new();
            let r = (every as f32 / 6.0).max(0.6);
            let mut y = (b.min().y / every).floor() * every + every / 2.0;
            let mut row = 0;
            while y <= b.max().y {
                let shift = if row % 2 == 1 { every / 2.0 } else { 0.0 };
                let mut x = (b.min().x / every).floor() * every + shift;
                while x <= b.max().x {
                    if px.contains(&geo::Point::new(x, y)) {
                        pb.push_circle(x as f32, y as f32, r);
                    }
                    x += every;
                }
                y += every;
                row += 1;
            }
            if let Some(path) = pb.finish() {
                pixmap.fill_path(&path, &paint(colour), FillRule::Winding, Transform::identity(), None);
            }
            continue;
        }
        // The directions of the lines, as the normal (x, y) of each.
        let dirs: &[(f64, f64)] = match style.pattern.as_str() {
            "hatch" => &[(1.0, 1.0)],
            "back" => &[(1.0, -1.0)],
            "cross" => &[(1.0, 1.0), (1.0, -1.0)],
            "horizontal" => &[(0.0, 1.0)],
            "vertical" => &[(1.0, 0.0)],
            "grid" => &[(0.0, 1.0), (1.0, 0.0)],
            _ => return,
        };
        let mut lines: Vec<geo::LineString<f64>> = Vec::new();
        let (cx, cy) = (b.center().x, b.center().y);
        let reach = b.width().hypot(b.height()) / 2.0 + every;
        for (nx, ny) in dirs {
            let len = nx.hypot(*ny);
            let (nx, ny) = (nx / len, ny / len);
            let (tx, ty) = (-ny, nx);
            // Lines at whole steps from the corner of the picture, so that the lines of
            // areas side by side meet.
            let at = cx * nx + cy * ny;
            let first = ((at - reach) / every).floor() as i64;
            let last = ((at + reach) / every).ceil() as i64;
            for k in first..=last {
                let d = k as f64 * every - at;
                let (mx, my) = (cx + nx * d, cy + ny * d);
                lines.push(geo::LineString::from(vec![(mx - tx * reach, my - ty * reach), (mx + tx * reach, my + ty * reach)]));
            }
        }
        let cut = px.clip(&geo::MultiLineString::new(lines), false);
        let mut pb = PathBuilder::new();
        for l in &cut.0 {
            let mut first = true;
            for c in &l.0 {
                if first {
                    pb.move_to(c.x as f32, c.y as f32);
                    first = false;
                } else {
                    pb.line_to(c.x as f32, c.y as f32);
                }
            }
        }
        if let Some(path) = pb.finish() {
            let stroke = Stroke { width, line_cap: tiny_skia::LineCap::Butt, ..Stroke::default() };
            pixmap.stroke_path(&path, &paint(colour), &stroke, Transform::identity(), None);
        }
    }
}

fn draw_casing(pixmap: &mut Canvas, table: &Table, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let Some(c) = style.casing.filter(|_| style.casing_width > 0.0) else { return };
    let stroke = style.casing_stroke();
    let p = paint(c);
    let mut pb = PathBuilder::new();
    let mut n = 0usize;
    let add = |pb: &mut PathBuilder, l: &geo::LineString<f64>| {
        for (k, q) in l.0.iter().enumerate() {
            let (x, y) = to_px(q.x, q.y);
            if k == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
    };
    for f in &table.features {
        match &f.geom {
            Some(Geometry::LineString(l)) => add(&mut pb, l),
            Some(Geometry::MultiLineString(m)) => m.0.iter().for_each(|l| add(&mut pb, l)),
            Some(Geometry::Line(l)) => add(&mut pb, &geo::LineString::from(*l)),
            _ => continue,
        }
        n += 1;
        if n.is_multiple_of(2_000) {
            if let Some(path) = std::mem::take(&mut pb).finish() {
                pixmap.stroke_path(&path, &p, &stroke, Transform::identity(), None);
            }
        }
    }
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &p, &stroke, Transform::identity(), None);
    }
}

/// Draws a layer a feature at a time, each in its own look and in the order of the theme,
/// with points that lie close together drawn as one with their number.
fn draw_themed(
    pixmap: &mut Canvas,
    table: &Table,
    style: &LayerStyle,
    theme: &crate::thematic::Theme,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
    going: &dyn Fn() -> bool,
) -> Result<()> {
    let base = crate::thematic::Look { fill: style.fill, stroke: style.stroke, size: style.size };
    // Faint by a value too, when the layer says so: the colour by one value and the
    // faintness by another (value by alpha).
    let alphas = extra::alphas(table, style);
    let look = |i: usize| {
        let mut l = theme.looks.as_ref().map_or(base, |l| l[i]);
        if let (Some(a), Some(c)) = (alphas.as_ref().map(|a| a[i]), l.fill) {
            l.fill = Some([c[0], c[1], c[2], (c[3] as f32 * a) as u8]);
        }
        l
    };
    let order: Vec<usize> = theme.order.clone().unwrap_or_else(|| (0..table.len()).collect());
    let stroke = style.stroke();
    draw_casing(pixmap, table, style, to_px);
    let dot = |pixmap: &mut Canvas, x: f32, y: f32, r: f32, l: &crate::thematic::Look| {
        let mut pb = PathBuilder::new();
        pb.push_circle(x, y, r.max(0.5));
        if let Some(path) = pb.finish() {
            if let Some(c) = l.fill {
                pixmap.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
            }
            if let (Some(c), true) = (l.stroke, style.width > 0.0) {
                pixmap.stroke_path(&path, &paint(c), &stroke, Transform::identity(), None);
            }
        }
    };
    // Points that are to be gathered into clusters are put aside and drawn last.
    let mut points: Vec<(f32, f32)> = vec![(f32::NAN, f32::NAN); table.len()];
    let clustering = theme.cluster > 0.0;
    for (k, &i) in order.iter().enumerate() {
        if k % 5_000 == 0 && k > 0 && !going() {
            return Err(GbError(crate::model::STOPPED.into()));
        }
        let Some(g) = &table.features[i].geom else { continue };
        let l = look(i);
        if let Some(p) = point_of(g) {
            let (x, y) = to_px(p.x(), p.y());
            // An arrow the way of the point's direction, as long as its size.
            if let Some(angles) = &theme.angles {
                if let Some(a) = angles[i] {
                    arrow(pixmap, x, y, a, l.size, &l, style.width);
                }
                continue;
            }
            if clustering {
                points[i] = (x, y);
            } else {
                if let Geometry::MultiPoint(m) = g {
                    for q in &m.0 {
                        let (x, y) = to_px(q.x(), q.y());
                        dot(pixmap, x, y, l.size / 2.0, &l);
                    }
                } else {
                    dot(pixmap, x, y, l.size / 2.0, &l);
                }
            }
            continue;
        }
        // Lines with arrow heads, cut back under them.
        if !style.arrows.is_empty() || style.gap > 0.0 {
            let w = if theme.sized_lines { l.size.max(0.3) } else { style.width };
            let mut heads = Vec::new();
            if let Some(parts) = arrowed_lines(g, to_px, &style.arrows, head_length(style, w), style.gap, &mut heads) {
                let mut pb = PathBuilder::new();
                polyline_path(&parts, &mut pb);
                let colour = if theme.sized_lines { l.stroke.or(l.fill) } else { l.stroke.filter(|_| w > 0.0) };
                if let (Some(path), Some(c)) = (pb.finish(), colour) {
                    let st = if theme.sized_lines { Stroke { width: w, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() } } else { stroke.clone() };
                    pixmap.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
                    if let Some((o, oc)) = (!theme.sized_lines).then(|| style.overlay(c)).flatten() {
                        pixmap.stroke_path(&path, &paint(oc), &o, Transform::identity(), None);
                    }
                    draw_heads(pixmap, &heads, w, c);
                }
                continue;
            }
        }
        let Some((path, area)) = path_of(g, to_px) else { continue };
        if let (Some(c), true) = (l.fill, area) {
            pixmap.fill_path(&path, &paint(c), FillRule::EvenOdd, Transform::identity(), None);
        }
        if area {
            draw_pattern(pixmap, g, style, to_px);
        }
        // A line sized by a value, as in a flow map, is as wide as its size.
        if !area && theme.sized_lines {
            if let Some(c) = l.stroke.or(l.fill) {
                let st = Stroke { width: l.size.max(0.3), line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                pixmap.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
            }
            continue;
        }
        if let (Some(c), true) = (l.stroke, style.width > 0.0) {
            pixmap.stroke_path(&path, &paint(c), &stroke, Transform::identity(), None);
            if let Some((o, oc)) = (!area).then(|| style.overlay(c)).flatten() {
                pixmap.stroke_path(&path, &paint(oc), &o, Transform::identity(), None);
            }
        }
    }
    if clustering {
        let placed: Vec<usize> = order.iter().copied().filter(|i| !points[*i].0.is_nan()).collect();
        let text = crate::chart::Text::new()?;
        // The numbers are written when every circle is drawn, so that none is covered.
        let mut labels: Vec<(f32, f32, f32, usize, [u8; 3])> = Vec::new();
        for group in crate::thematic::clusters(&points, &placed, theme.cluster) {
            let lead = look(group[0]);
            let n = group.len();
            let (x, y) = if n == 1 {
                points[group[0]]
            } else {
                let (sx, sy) = group.iter().fold((0.0, 0.0), |(a, b), i| (a + points[*i].0, b + points[*i].1));
                (sx / n as f32, sy / n as f32)
            };
            // A cluster grows with the number in it, by area, but not without end.
            let r = lead.size / 2.0 * (1.0 + (n as f32).log2() * 0.45);
            dot(pixmap, x, y, r, &lead);
            if n > 1 {
                let fill = lead.fill.unwrap_or([255, 255, 255, 255]);
                // White on a dark or strong fill, else dark.
                let light = 0.299 * fill[0] as f32 + 0.587 * fill[1] as f32 + 0.114 * fill[2] as f32;
                let ink = if light < 150.0 && fill[3] > 120 { [255, 255, 255] } else { [0x0b, 0x0b, 0x0b] };
                labels.push((x, y, (r * 0.95).clamp(8.0, 40.0), n, ink));
            }
        }
        for (x, y, size, n, ink) in labels {
            write_text(pixmap, &text, &n.to_string(), x, y + size * 0.36, size, true, ink, 0.5);
        }
    }
    Ok(())
}

/// An arrow centred on a point, pointing `angle` radians clockwise from up, `length` pixels
/// long: a shaft and a head, in the colour of the line, or of the fill when it has none.
fn arrow(pixmap: &mut Canvas, x: f32, y: f32, angle: f32, length: f32, look: &crate::thematic::Look, width: f32) {
    let colour = look.stroke.or(look.fill).unwrap_or([0x22, 0x22, 0x22, 255]);
    let (dx, dy) = (angle.sin(), -angle.cos());
    let half = length.max(4.0) / 2.0;
    let (tx, ty) = (x + dx * half, y + dy * half);
    let (bx, by) = (x - dx * half, y - dy * half);
    let head = (length * 0.35).clamp(3.0, 18.0);
    let (nx, ny) = (-dy, dx);
    let mut shaft = PathBuilder::new();
    shaft.move_to(bx, by);
    shaft.line_to(tx - dx * head * 0.6, ty - dy * head * 0.6);
    if let Some(path) = shaft.finish() {
        let stroke = Stroke { width: width.max(1.0), line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
        pixmap.stroke_path(&path, &paint(colour), &stroke, Transform::identity(), None);
    }
    let mut tip = PathBuilder::new();
    tip.move_to(tx, ty);
    tip.line_to(tx - dx * head + nx * head * 0.45, ty - dy * head + ny * head * 0.45);
    tip.line_to(tx - dx * head - nx * head * 0.45, ty - dy * head - ny * head * 0.45);
    tip.close();
    if let Some(path) = tip.finish() {
        pixmap.fill_path(&path, &paint(colour), FillRule::Winding, Transform::identity(), None);
    }
}

/// A heatmap of the points of a layer: each spreads over the reach of the theme, falling
/// off smoothly (the quartic kernel), and the sums are coloured by the ramp, from clear where
/// there is little to the darkest where there is most.
fn draw_heat(pixmap: &mut Canvas, table: &Table, theme: &crate::thematic::Theme, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let reach = theme.heat.max(1.0);
    let r = reach.ceil() as isize;
    let mut sum = vec![0.0f32; w * h];
    for (i, f) in table.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        let weight = theme.heat_weights.as_ref().map_or(1.0, |ws| ws[i]);
        if weight <= 0.0 {
            continue;
        }
        let pts: Vec<geo::Point<f64>> = match g {
            Geometry::Point(p) => vec![*p],
            Geometry::MultiPoint(m) => m.0.clone(),
            _ => continue,
        };
        for p in pts {
            let (x, y) = to_px(p.x(), p.y());
            let (cx, cy) = (x.round() as isize, y.round() as isize);
            for yy in (cy - r).max(0)..(cy + r + 1).min(h as isize) {
                for xx in (cx - r).max(0)..(cx + r + 1).min(w as isize) {
                    let d2 = ((xx as f32 + 0.5 - x).powi(2) + (yy as f32 + 0.5 - y).powi(2)) / (reach * reach);
                    if d2 < 1.0 {
                        sum[yy as usize * w + xx as usize] += weight * (1.0 - d2) * (1.0 - d2);
                    }
                }
            }
        }
    }
    // The top is near the highest, so that one lone peak does not make the rest pale.
    let mut nonzero: Vec<f32> = sum.iter().copied().filter(|v| *v > 0.0).collect();
    if nonzero.is_empty() {
        return;
    }
    let k = (((nonzero.len() as f32) * 0.995) as usize).min(nonzero.len() - 1);
    let top = *nonzero.select_nth_unstable_by(k, f32::total_cmp).1;
    let top = top.max(1e-9);
    let Some(mut layer) = Pixmap::new(w as u32, h as u32) else { return };
    let px = layer.pixels_mut();
    for (i, v) in sum.iter().enumerate() {
        if *v <= 0.0 {
            continue;
        }
        let t = (*v / top).min(1.0);
        let c = crate::thematic::ramp_at(&theme.heat_ramp, t as f64);
        // Clear where there is little, nearly solid where there is most.
        let a = (t.sqrt() * 0.85 + 0.05).min(0.9);
        px[i] = tiny_skia::ColorU8::from_rgba(c[0], c[1], c[2], (a * 255.0).round() as u8).premultiply();
    }
    pixmap.draw_pixmap(0, 0, layer.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
}

/// Draws the points of a layer that have icons, in the order and the looks of the theme.
fn draw_icons(
    pm: &mut Canvas,
    table: &Table,
    style: &LayerStyle,
    theme: Option<&crate::thematic::Theme>,
    icons: &Icons,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
) {
    let order: Vec<usize> = theme.and_then(|t| t.order.clone()).unwrap_or_else(|| (0..table.len()).collect());
    for i in order {
        let (Some(name), Some(g)) = (icons.names.get(i).copied().flatten(), table.features[i].geom.as_ref()) else { continue };
        let look = theme.and_then(|t| t.looks.as_ref()).map(|l| l[i]);
        let (fill, stroke, size) = match look {
            Some(l) => (l.fill, l.stroke, l.size),
            None => (style.fill, style.stroke, style.size),
        };
        let pts: Vec<geo::Point<f64>> = match g {
            Geometry::Point(p) => vec![*p],
            Geometry::MultiPoint(m) => m.0.clone(),
            _ => continue,
        };
        for p in pts {
            let (x, y) = to_px(p.x(), p.y());
            draw_icon(pm, name, x, y, icon_size(size), &icons.style, fill, stroke);
        }
    }
}

/// A place for a label: where the start of its baseline goes, its angle, and which part of
/// it lies at the place (0 the start, 0.5 the middle, 1 the end).
struct Spot {
    x: f32,
    y: f32,
    angle: f32,
    align: f32,
    /// Letter by letter, for a label that bends with its line or is spread out: the middle
    /// of each letter on the line, and how it is turned. Empty: the label is one piece.
    glyphs: Vec<(f32, f32, f32)>,
}

/// The places a label may go, the best first.
#[allow(clippy::too_many_arguments)]
/// The layers in the order their labels are placed: by rank, highest first, and among the
/// same rank the top layer first.
fn label_order(req: &Request) -> Vec<usize> {
    let mut order: Vec<usize> = (0..req.layers.len()).rev().collect();
    let rank = |li: usize| req.labels.get(li).and_then(|l| l.as_ref()).map_or(0, |l| l.spec.rank);
    order.sort_by_key(|li| std::cmp::Reverse(rank(*li)));
    order
}

/// A line through the middle of an area, along its length: across the area at even steps
/// along its longest side, the middle of the widest stretch inside it, smoothed. None for an
/// area not twice as long as it is wide.
fn centre_line(g: &Geometry<f64>) -> Option<LineString<f64>> {
    use geo::{Area, ChaikinSmoothing, MinimumRotatedRect};
    let poly = match g {
        Geometry::Polygon(p) => p.clone(),
        Geometry::MultiPolygon(m) => m.0.iter().max_by(|a, b| a.unsigned_area().total_cmp(&b.unsigned_area()))?.clone(),
        _ => return None,
    };
    let rect = poly.minimum_rotated_rect()?;
    let c = &rect.exterior().0;
    if c.len() < 4 {
        return None;
    }
    let (e1, e2) = ((c[1].x - c[0].x, c[1].y - c[0].y), (c[2].x - c[1].x, c[2].y - c[1].y));
    let (l1, l2) = (e1.0.hypot(e1.1), e2.0.hypot(e2.1));
    let (along, across, long, short, start) = if l1 >= l2 { (e1, e2, l1, l2, c[0]) } else { (e2, e1, l2, l1, c[1]) };
    if long < short * 2.0 || long <= 0.0 {
        return None;
    }
    let (ux, uy) = (along.0 / long, along.1 / long);
    let (vx, vy) = (across.0 / short, across.1 / short);
    let rings: Vec<&LineString<f64>> = std::iter::once(poly.exterior()).chain(poly.interiors()).collect();
    let mut pts = Vec::new();
    for k in 1..12 {
        let t = long * k as f64 / 12.0;
        let (px, py) = (start.x + ux * t, start.y + uy * t);
        // Where the line across meets the edges, as distances along it.
        let mut hits: Vec<f64> = Vec::new();
        for ring in &rings {
            for w in ring.0.windows(2) {
                let (a, b) = (w[0], w[1]);
                // Solve p + s·v = a + r·(b - a).
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let den = vx * dy - vy * dx;
                if den.abs() < 1e-12 {
                    continue;
                }
                let (qx, qy) = (a.x - px, a.y - py);
                let s = (qx * dy - qy * dx) / den;
                let r = (qx * vy - qy * vx) / den;
                if (0.0..=1.0).contains(&r) {
                    hits.push(s);
                }
            }
        }
        hits.sort_by(f64::total_cmp);
        let best = hits.chunks_exact(2).max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])));
        if let Some(seg) = best {
            let s = (seg[0] + seg[1]) / 2.0;
            pts.push(geo::Coord { x: px + vx * s, y: py + vy * s });
        }
    }
    if pts.len() < 3 {
        return None;
    }
    // Read from left to right.
    if pts[0].x > pts[pts.len() - 1].x {
        pts.reverse();
    }
    Some(LineString::new(pts).chaikin_smoothing(2))
}

/// A line along the shore inside an area of water, `inset` in from it: the longest edge
/// of the area shrunk by that much. None when nothing is left.
fn coast_line(g: &Geometry<f64>, inset: f64) -> Option<LineString<f64>> {
    use geo::{Buffer, Length};
    let shrunk = match g {
        Geometry::Polygon(p) => p.buffer(-inset),
        Geometry::MultiPolygon(m) => m.buffer(-inset),
        _ => return None,
    };
    shrunk.0.into_iter().map(|p| p.exterior().clone()).max_by(|a, b| geo::Euclidean.length(a).total_cmp(&geo::Euclidean.length(b))).filter(|l| l.0.len() > 3)
}

#[allow(clippy::too_many_arguments)]
fn spots(
    g: &Geometry<f64>,
    spec: &LabelSpec,
    at: Option<(f64, f64)>,
    w: f32,
    adv: &[f32],
    size: f32,
    point_r: f32,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
) -> Vec<Spot> {
    use geo::{InteriorPoint, Length};
    let mid = size * 0.35;
    let centred = |x: f32, y: f32| Spot { x, y: y + mid, angle: 0.0, align: 0.5, glyphs: Vec::new() };
    if let Some((ax, ay)) = at {
        let (x, y) = to_px(ax, ay);
        return vec![centred(x, y)];
    }
    if let Some(p) = point_of(g) {
        let (x, y) = to_px(p.x(), p.y());
        if spec.place == "center" {
            return vec![centred(x, y)];
        }
        let r = point_r + 2.0;
        let d = r * 0.75;
        // Around the point, in the order mapmakers like: right, upper right, lower right,
        // left, above, below, upper left, lower left; or in the classic order, the corners
        // first.
        if spec.prefer == "classic" {
            return vec![
                Spot { x: x + d, y: y - d, angle: 0.0, align: 0.0, glyphs: Vec::new() },
                Spot { x: x + d, y: y + d + size * 0.75, angle: 0.0, align: 0.0, glyphs: Vec::new() },
                Spot { x: x - d, y: y - d, angle: 0.0, align: 1.0, glyphs: Vec::new() },
                Spot { x: x - d, y: y + d + size * 0.75, angle: 0.0, align: 1.0, glyphs: Vec::new() },
                Spot { x: x + r, y: y + mid, angle: 0.0, align: 0.0, glyphs: Vec::new() },
                Spot { x: x - r, y: y + mid, angle: 0.0, align: 1.0, glyphs: Vec::new() },
                Spot { x, y: y - r - size * 0.2, angle: 0.0, align: 0.5, glyphs: Vec::new() },
                Spot { x, y: y + r + size * 0.8, angle: 0.0, align: 0.5, glyphs: Vec::new() },
            ];
        }
        return vec![
            Spot { x: x + r, y: y + mid, angle: 0.0, align: 0.0, glyphs: Vec::new() },
            Spot { x: x + d, y: y - d, angle: 0.0, align: 0.0, glyphs: Vec::new() },
            Spot { x: x + d, y: y + d + size * 0.75, angle: 0.0, align: 0.0, glyphs: Vec::new() },
            Spot { x: x - r, y: y + mid, angle: 0.0, align: 1.0, glyphs: Vec::new() },
            Spot { x, y: y - r - size * 0.2, angle: 0.0, align: 0.5, glyphs: Vec::new() },
            Spot { x, y: y + r + size * 0.8, angle: 0.0, align: 0.5, glyphs: Vec::new() },
            Spot { x: x - d, y: y - d, angle: 0.0, align: 1.0, glyphs: Vec::new() },
            Spot { x: x - d, y: y + d + size * 0.75, angle: 0.0, align: 1.0, glyphs: Vec::new() },
        ];
    }
    let line = match g {
        // The name of an area along the middle of it, when it is long enough for that.
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) if spec.coast => {
            let (a, b) = (to_px(0.0, 0.0), to_px(1.0, 0.0));
            let per = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() as f64;
            coast_line(g, size as f64 * 1.4 / per.max(1e-12))
        }
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) if spec.curved => centre_line(g),
        Geometry::LineString(l) => Some(l.clone()),
        Geometry::Line(l) => Some(geo::LineString::from(*l)),
        Geometry::MultiLineString(m) => m.0.iter().max_by(|a, b| geo::Euclidean.length(*a).total_cmp(&geo::Euclidean.length(*b))).cloned(),
        _ => None,
    };
    if let Some(l) = line {
        let pts: Vec<(f32, f32)> = l.0.iter().map(|c| to_px(c.x, c.y)).collect();
        let mut acc = vec![0.0f32];
        for w2 in pts.windows(2) {
            let d = ((w2[1].0 - w2[0].0).powi(2) + (w2[1].1 - w2[0].1).powi(2)).sqrt();
            acc.push(acc.last().unwrap() + d);
        }
        let total = *acc.last().unwrap_or(&0.0);
        let along = |t: f32| -> (f32, f32) {
            let t = t.clamp(0.0, total);
            let k = acc.iter().position(|a| *a >= t).unwrap_or(acc.len() - 1).max(1);
            let span = (acc[k] - acc[k - 1]).max(1e-6);
            let f = (t - acc[k - 1]) / span;
            (pts[k - 1].0 + (pts[k].0 - pts[k - 1].0) * f, pts[k - 1].1 + (pts[k].1 - pts[k - 1].1) * f)
        };
        // A small closed ring, such as a contour around a knoll, and a contour that is
        // little longer than its number are left without: they would only crowd the map.
        let closed = pts.len() > 2 && pts.first() == pts.last() && !spec.coast;
        let need = if closed {
            4.0
        } else if spec.uphill {
            2.5
        } else {
            1.1
        };
        if total < w * need && !spec.all {
            return Vec::new();
        }
        let mut out = Vec::new();
        for frac in [0.5f32, 0.33, 0.67, 0.2, 0.8, 0.42, 0.58, 0.1, 0.9] {
            let c = total * frac;
            // Letter by letter along the line, so that the label bends with a road or a
            // river. It reads from left to right, and is not used where the line bends too
            // sharply for the letters to follow it.
            if spec.place != "straight" && adv.len() > 1 {
                let (a, b) = (along(c - w / 2.0), along(c + w / 2.0));
                let backwards = match spec.uphill.then(|| downhill(a, b, along(c), size)).flatten() {
                    Some(down) => down,
                    None => b.0 < a.0,
                };
                let at_t = |t: f32| if backwards { along(2.0 * c - t) } else { along(t) };
                let mut glyphs = Vec::with_capacity(adv.len());
                let mut pen = c - w / 2.0;
                let mut ok = true;
                let mut last: Option<f32> = None;
                for a in adv {
                    let m = pen + a / 2.0;
                    let (p0, p1) = (at_t(m - a.max(size * 0.3) / 2.0), at_t(m + a.max(size * 0.3) / 2.0));
                    let angle = (p1.1 - p0.1).atan2(p1.0 - p0.0).to_degrees();
                    if let Some(l) = last {
                        let mut d = (angle - l).abs();
                        if d > 180.0 {
                            d = 360.0 - d;
                        }
                        if d > 35.0 {
                            ok = false;
                            break;
                        }
                    }
                    last = Some(angle);
                    let (cx, cy) = at_t(m);
                    let t = angle.to_radians();
                    glyphs.push((cx - t.sin() * mid, cy + t.cos() * mid, angle));
                    pen += a;
                }
                let bent = {
                    let first = glyphs.first().map_or(0.0, |g| g.2);
                    glyphs.iter().any(|g| (g.2 - first).abs() > 4.0)
                };
                if ok && bent {
                    out.push(Spot { x: c, y: 0.0, angle: 0.0, align: 0.5, glyphs });
                    continue;
                }
            }
            let (a, b) = (along(c - w / 2.0), along(c + w / 2.0));
            let (cx, cy) = along(c);
            let mut angle = (b.1 - a.1).atan2(b.0 - a.0).to_degrees();
            if let Some(down) = spec.uphill.then(|| downhill(a, b, (cx, cy), size)).flatten() {
                // The top towards the higher ground, even when that is upside down.
                if down {
                    angle += 180.0;
                }
            } else if angle > 90.0 {
                // Never upside down.
                angle -= 180.0;
            } else if angle < -90.0 {
                angle += 180.0;
            }
            let t = angle.to_radians();
            // The middle of the letters on the line: the baseline a little below it.
            out.push(Spot { x: cx - t.sin() * mid, y: cy + t.cos() * mid, angle, align: 0.5, glyphs: Vec::new() });
        }
        return out;
    }
    match g.interior_point() {
        Some(p) => {
            let (x, y) = to_px(p.x(), p.y());
            // Spread out over the area, as the names of lakes and districts are written.
            if spec.spaced && adv.len() > 1 {
                use geo::BoundingRect;
                if let Some(r) = g.bounding_rect() {
                    let (x0, _) = to_px(r.min().x, r.min().y);
                    let (x1, _) = to_px(r.max().x, r.max().y);
                    let room = (x1 - x0).abs() * 0.6;
                    let gap = ((room - w) / (adv.len() - 1) as f32).clamp(0.0, size * 1.2);
                    if gap > size * 0.05 {
                        let total = w + gap * (adv.len() - 1) as f32;
                        let mut pen = x - total / 2.0;
                        let glyphs = adv
                            .iter()
                            .map(|a| {
                                let g = (pen + a / 2.0, y + mid, 0.0);
                                pen += a + gap;
                                g
                            })
                            .collect();
                        return vec![Spot { x, y: y + mid, angle: 0.0, align: 0.5, glyphs }, centred(x, y)];
                    }
                }
            }
            // Then a little above, below and to the sides, while that is still inside the area,
            // so that the name of an area finds room between the names of its towns.
            let mut out = vec![centred(x, y)];
            if spec.place != "center" && matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) {
                use geo::Contains;
                let (x1, _) = to_px(p.x() + 1.0, p.y());
                let per_px = 1.0 / ((x1 - x) as f64).abs().max(f64::MIN_POSITIVE);
                let lines = 1.0 + adv.is_empty() as u8 as f32;
                // Nearest first, on a grid of half label widths across and line heights down.
                let mut tries: Vec<(f32, f32)> = (-2..=2_i8)
                    .flat_map(|i| (-4..=4_i8).map(move |j| (f32::from(i) * 0.5, f32::from(j) * 1.6)))
                    .filter(|&(dx, dy)| dx != 0.0 || dy != 0.0)
                    .collect();
                tries.sort_by(|a, b| (a.0 * a.0 * 9.0 + a.1 * a.1).total_cmp(&(b.0 * b.0 * 9.0 + b.1 * b.1)));
                for (dx, dy) in tries {
                    let (ox, oy) = (dx * w, dy * size * lines);
                    let q = geo::Point::new(p.x() + ox as f64 * per_px, p.y() - oy as f64 * per_px);
                    if g.contains(&q) {
                        out.push(centred(x + ox, y + oy));
                    }
                }
            }
            out
        }
        None => Vec::new(),
    }
}

/// Places the labels of a layer where there is room, or all of them, and draws them.
#[allow(clippy::too_many_arguments)]
fn place_labels(
    pm: &mut Canvas,
    text: &crate::chart::Text,
    table: &Table,
    labels: &Labels,
    point_r: f32,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
    taken: &mut Vec<[f32; 4]>,
    dry: bool,
) {
    let spec = &labels.spec;
    let mut order: Vec<&LabelItem> = labels.items.iter().collect();
    order.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    let (pw, ph) = (pm.width() as f32, pm.height() as f32);
    // The labels placed so far, and where: a line whose own place for its label already
    // shows the same words, as where two lines named 0 cross, is not labelled somewhere
    // else along it to say them again.
    let mut said: Vec<(&str, f32, f32)> = Vec::new();
    for item in order {
        let Some(g) = table.features.get(item.feature).and_then(|f| f.geom.as_ref()) else {
            if item.at.is_none() {
                continue;
            }
            // Placed by attributes, a feature needs no geometry of its own.
            let spots = spots(&Geometry::GeometryCollection(Default::default()), spec, item.at, 0.0, &[], item.size, point_r, to_px);
            draw_one(pm, text, item, spec, &spots, taken, pw, ph, dry);
            continue;
        };
        let w = label_width(text, &item.text, item.size, item.bold);
        // A label of several lines stays straight and is not spread out.
        let adv: Vec<f32> = if item.text.contains('\n') {
            Vec::new()
        } else {
            item.text
                .chars()
                .map(|c| {
                    let (l, size) = small_cap(c, spec.small_caps, item.size);
                    text.width(&l, size, item.bold)
                })
                .collect()
        };
        // Spread out along a line, as the names of rivers: room between the letters.
        let along = matches!(g, Geometry::LineString(_) | Geometry::MultiLineString(_));
        let gap = item.size * (spec.tracking + if spec.small_caps { 0.06 } else { 0.0 } + if spec.spaced && along { 0.32 } else { 0.0 });
        let (adv, w) = if gap > 0.0 && !adv.is_empty() {
            let adv: Vec<f32> = adv.iter().map(|a| a + gap).collect();
            let w = adv.iter().sum::<f32>();
            (adv, w)
        } else {
            (adv, w)
        };
        // An area too small for its name: none, or one beside it with a line in to it.
        if item.at.is_none() && matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) && (spec.min_px > 0.0 || spec.leaders) {
            if let Some(r) = g.bounding_rect() {
                let (x0, y0) = to_px(r.min().x, r.max().y);
                let (x1, y1) = to_px(r.max().x, r.min().y);
                let (bw, bh) = ((x1 - x0).abs(), (y1 - y0).abs());
                let lines = item.text.split('\n').count() as f32;
                let small = spec.min_px > 0.0 && bw.min(bh) < spec.min_px;
                let fits = w <= bw * 0.9 && item.size * 1.2 * lines <= bh * 0.9;
                if small || (spec.leaders && !fits) {
                    if spec.leaders {
                        leader_label(pm, text, item, spec, g, [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)], w, to_px, taken, pw, ph, dry);
                    }
                    continue;
                }
            }
        }
        // Along a long line, a label every so many pixels.
        if spec.repeat > 0.0 && item.at.is_none() && matches!(g, Geometry::LineString(_) | Geometry::MultiLineString(_)) {
            for piece in pieces_along(g, spec.repeat, to_px) {
                let candidates = spots(&piece, spec, None, w, &adv, item.size, point_r, to_px);
                checked(pm, text, item, spec, &candidates, taken, pw, ph, dry);
            }
            continue;
        }
        let candidates = spots(g, spec, item.at, w, &adv, item.size, point_r, to_px);
        // Letters apart on a straight label: each placed on its own along the line of it.
        let candidates = if gap > 0.0 && !adv.is_empty() { candidates.into_iter().map(|c| tracked(c, &adv, w)).collect() } else { candidates };
        let near = item.size.max(w);
        if along && candidates.first().map(spot_centre).is_some_and(|(x, y)| said.iter().any(|(t, sx, sy)| *t == item.text && (sx - x).hypot(sy - y) < near)) {
            continue;
        }
        let placed = checked(pm, text, item, spec, &candidates, taken, pw, ph, dry);
        if let Some(b) = placed {
            said.push((item.text.as_str(), (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0));
        }
        // A point whose name has no room beside it: further out, with a line in to it.
        if placed.is_none() && spec.leaders && item.at.is_none() {
            if let Some(p) = point_of(g).filter(|_| matches!(g, Geometry::Point(_) | Geometry::MultiPoint(_))) {
                let (x, y) = to_px(p.x(), p.y());
                let r = point_r + 1.0;
                leader_label(pm, text, item, spec, g, [x - r, y - r, x + r, y + r], w, to_px, taken, pw, ph, dry);
            }
        }
    }
}

/// The middle of a place for a label on the picture.
fn spot_centre(s: &Spot) -> (f32, f32) {
    if s.glyphs.is_empty() {
        return (s.x, s.y);
    }
    let n = s.glyphs.len() as f32;
    (s.glyphs.iter().map(|g| g.0).sum::<f32>() / n, s.glyphs.iter().map(|g| g.1).sum::<f32>() / n)
}

/// A grid of heights, and where the map lies: its west, its north and the size of a pixel.
type HeightsOnMap = Option<(std::sync::Arc<crate::model::Grid>, f64, f64, f64)>;

thread_local! {
    /// The heights that labels along lines are turned up towards.
    static HEIGHTS: std::cell::RefCell<HeightsOnMap> = const { std::cell::RefCell::new(None) };
}

/// Whether a label read from `a` to `b`, with its middle at `c`, would have the top of its
/// letters towards lower ground. None: no heights there.
fn downhill(a: (f32, f32), b: (f32, f32), c: (f32, f32), size: f32) -> Option<bool> {
    HEIGHTS.with(|h| {
        let h = h.borrow();
        let (g, west, north, px) = h.as_ref()?;
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy).max(1e-6);
        // The top of the letters, on the picture: to the left of the way of reading.
        let (ux, uy) = (dy / len, -dx / len);
        let at = |x: f32, y: f32| {
            let (mx, my) = (west + x as f64 * px, north - y as f64 * px);
            crate::terrain::height_at(g, mx, my)
        };
        let d = size.max(4.0) * 1.5;
        let (up, down) = (at(c.0 + ux * d, c.1 + uy * d)?, at(c.0 - ux * d, c.1 - uy * d)?);
        (up != down).then_some(up < down)
    })
}

/// A straight label made letter by letter, each letter `adv` wide, so that the room between
/// them shows: the middle of each along the line of the label, as it is aligned.
/// A letter as small capitals set it: a small letter as a capital the height of the small
/// letters, the rest as it is.
fn small_cap(c: char, on: bool, size: f32) -> (String, f32) {
    if on && c.is_lowercase() {
        (c.to_uppercase().collect(), size * 0.78)
    } else {
        (c.to_string(), size)
    }
}

fn tracked(c: Spot, adv: &[f32], w: f32) -> Spot {
    if !c.glyphs.is_empty() {
        return c;
    }
    let (cos, sin) = (c.angle.to_radians().cos(), c.angle.to_radians().sin());
    let start = -w * c.align;
    let mut run = start;
    let glyphs = adv
        .iter()
        .map(|a| {
            let mid = run + a / 2.0;
            run += a;
            (c.x + cos * mid, c.y + sin * mid, c.angle)
        })
        .collect();
    Spot { glyphs, ..c }
}

thread_local! {
    /// Whether to look at how well each label stands out, and the labels found that do not.
    static CHECK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAINT: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// As `draw_one`, but when the labels are looked at and have no halo, the colour under the
/// label is measured first, and a label that hardly stands out from it is noted.
#[allow(clippy::too_many_arguments)]
fn checked(
    pm: &mut Canvas,
    text: &crate::chart::Text,
    item: &LabelItem,
    spec: &LabelSpec,
    candidates: &[Spot],
    taken: &mut Vec<[f32; 4]>,
    pw: f32,
    ph: f32,
    dry: bool,
) -> Option<[f32; 4]> {
    let halo = spec.halo.is_some() && spec.halo_width > 0.0;
    if dry || halo || !CHECK.with(|c| c.get()) {
        return draw_one(pm, text, item, spec, candidates, taken, pw, ph, dry);
    }
    let bx = draw_one(pm, text, item, spec, candidates, taken, pw, ph, true)?;
    taken.pop();
    let (w, h) = (pm.width() as i64, pm.height() as i64);
    let px = pm.pixels();
    let (mut sum, mut n) = ([0f32; 3], 0f32);
    let mut y = bx[1].max(0.0) as i64;
    while y < (bx[3] as i64).min(h) {
        let mut x = bx[0].max(0.0) as i64;
        while x < (bx[2] as i64).min(w) {
            let c = px[(y * w + x) as usize].demultiply();
            sum[0] += c.red() as f32;
            sum[1] += c.green() as f32;
            sum[2] += c.blue() as f32;
            n += 1.0;
            x += 3;
        }
        y += 3;
    }
    if n > 0.0 {
        let under = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8];
        if extra::contrast(under, [spec.colour[0], spec.colour[1], spec.colour[2]]) < 3.0 {
            FAINT.with(|f| f.borrow_mut().push(item.text.replace('\n', " ")));
        }
    }
    draw_one(pm, text, item, spec, candidates, taken, pw, ph, false)
}

/// A line cut in pieces about `every` pixels long on the map, for a label on each.
fn pieces_along(g: &Geometry<f64>, every: f32, to_px: &dyn Fn(f64, f64) -> (f32, f32)) -> Vec<Geometry<f64>> {
    let lines: Vec<&LineString<f64>> = match g {
        Geometry::LineString(l) => vec![l],
        Geometry::MultiLineString(m) => m.0.iter().collect(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for l in lines {
        let mut piece: Vec<Coord<f64>> = Vec::new();
        let mut run = 0.0f32;
        for w in l.0.windows(2) {
            if piece.is_empty() {
                piece.push(w[0]);
            }
            let (a, b) = (to_px(w[0].x, w[0].y), to_px(w[1].x, w[1].y));
            run += ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            piece.push(w[1]);
            if run >= every {
                out.push(Geometry::LineString(LineString::new(std::mem::take(&mut piece))));
                run = 0.0;
            }
        }
        // What is left at the end gets a label too when it is long enough for one.
        if piece.len() > 1 && run >= every * 0.5 {
            out.push(Geometry::LineString(LineString::new(piece)));
        }
    }
    out
}

/// A label beside an area too small for it, joined to the area by a thin line: tried to the
/// right, the left, above and below, then on the diagonals, further out each round.
#[allow(clippy::too_many_arguments)]
fn leader_label(
    pm: &mut Canvas,
    text: &crate::chart::Text,
    item: &LabelItem,
    spec: &LabelSpec,
    g: &Geometry<f64>,
    bbox: [f32; 4],
    w: f32,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
    taken: &mut Vec<[f32; 4]>,
    pw: f32,
    ph: f32,
    dry: bool,
) {
    use geo::InteriorPoint;
    let Some(inside) = g.interior_point() else { return };
    let (ax, ay) = to_px(inside.x(), inside.y());
    let h = item.size;
    let mid = h * 0.35;
    let mut candidates = Vec::new();
    for reach in [1.2f32, 2.2, 3.4] {
        let d = h * reach;
        candidates.extend([
            Spot { x: bbox[2] + d, y: ay + mid, angle: 0.0, align: 0.0, glyphs: Vec::new() },
            Spot { x: bbox[0] - d, y: ay + mid, angle: 0.0, align: 1.0, glyphs: Vec::new() },
            Spot { x: ax, y: bbox[1] - d, angle: 0.0, align: 0.5, glyphs: Vec::new() },
            Spot { x: ax, y: bbox[3] + d + h * 0.8, angle: 0.0, align: 0.5, glyphs: Vec::new() },
            Spot { x: bbox[2] + d * 0.8, y: bbox[1] - d * 0.6, angle: 0.0, align: 0.0, glyphs: Vec::new() },
            Spot { x: bbox[0] - d * 0.8, y: bbox[1] - d * 0.6, angle: 0.0, align: 1.0, glyphs: Vec::new() },
            Spot { x: bbox[2] + d * 0.8, y: bbox[3] + d * 0.6 + h * 0.8, angle: 0.0, align: 0.0, glyphs: Vec::new() },
            Spot { x: bbox[0] - d * 0.8, y: bbox[3] + d * 0.6 + h * 0.8, angle: 0.0, align: 1.0, glyphs: Vec::new() },
        ]);
    }
    let _ = w;
    let Some(bx) = draw_one(pm, text, item, spec, &candidates, taken, pw, ph, true) else { return };
    // The line from the area to the nearest point of the label's box, under the label.
    let tx = ax.clamp(bx[0], bx[2]);
    let ty = ay.clamp(bx[1], bx[3]);
    if !dry {
        let mut pb = PathBuilder::new();
        pb.move_to(ax, ay);
        pb.line_to(tx, ty);
        if let Some(path) = pb.finish() {
            let c = spec.colour;
            if let Some(hc) = spec.halo.filter(|_| spec.halo_width > 0.0) {
                pm.stroke_path(
                    &path,
                    &paint(hc),
                    &Stroke { width: 2.6, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() },
                    Transform::identity(),
                    None,
                );
            }
            pm.stroke_path(
                &path,
                &paint([c[0], c[1], c[2], 200]),
                &Stroke { width: 0.9, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() },
                Transform::identity(),
                None,
            );
        }
        let mut dot = PathBuilder::new();
        dot.push_circle(ax, ay, 1.8);
        if let Some(path) = dot.finish() {
            pm.fill_path(&path, &paint(spec.colour), FillRule::Winding, Transform::identity(), None);
        }
        // The label itself, at the spot just found, on top of its line.
        taken.pop();
        draw_one(pm, text, item, spec, &candidates, taken, pw, ph, false);
    }
}

/// Places a label at the first of the spots where it fits, and draws it unless `dry`: then
/// only the room it takes is kept, as when the labels are placed first so that lines can
/// break under them. Returns the box it took.
#[allow(clippy::too_many_arguments)]
fn draw_one(
    pm: &mut Canvas,
    text: &crate::chart::Text,
    item: &LabelItem,
    spec: &LabelSpec,
    candidates: &[Spot],
    taken: &mut Vec<[f32; 4]>,
    pw: f32,
    ph: f32,
    dry: bool,
) -> Option<[f32; 4]> {
    let bold = item.bold;
    let w = if spec.detail {
        item.text
            .split('\n')
            .enumerate()
            .map(|(i, l)| {
                let (size, b) = line_look(spec, item, i);
                text.width(l, size, b)
            })
            .fold(0.0, f32::max)
    } else {
        label_width(text, &item.text, item.size, bold)
    };
    let h = item.size;
    // Leaning letters, for the names of water.
    let lean = if spec.italic { Transform::from_skew(-0.21, 0.0) } else { Transform::identity() };
    // The lines of a label, each a line height below the one before, the block centred on
    // where one line would stand.
    let lines: Vec<&str> = item.text.split('\n').collect();
    let line_h = h * 1.2;
    let extra = (lines.len() - 1) as f32 * line_h / 2.0;
    let pad = spec.halo.map_or(0.0, |_| spec.halo_width) + 1.0;
    for spot in candidates {
        if !spot.glyphs.is_empty() {
            // A box for each letter, so that a bent label takes only the room it covers.
            let r = h * 0.62 + pad;
            let boxes: Vec<[f32; 4]> = spot.glyphs.iter().map(|(x, y, _)| [x - r, y - h * 0.35 - r, x + r, y - h * 0.35 + r]).collect();
            if !spec.all {
                let inside = boxes.iter().all(|b| b[0] >= 0.0 && b[1] >= 0.0 && b[2] <= pw && b[3] <= ph);
                let clear = boxes.iter().all(|b| taken.iter().all(|o| b[2] <= o[0] || b[0] >= o[2] || b[3] <= o[1] || b[1] >= o[3]));
                if !inside || !clear {
                    continue;
                }
            }
            if !dry {
                let letters: Vec<(String, f32)> = item.text.chars().map(|c| small_cap(c, spec.small_caps, item.size)).collect();
                let halo = spec.halo.filter(|_| spec.halo_width > 0.0);
                let st = Stroke { width: spec.halo_width * 2.0, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                let paths: Vec<Option<(tiny_skia::Path, Transform)>> = letters
                    .iter()
                    .zip(&spot.glyphs)
                    .map(|((l, size), (x, y, a))| {
                        let lw = text.width(l, *size, bold);
                        text.path(l, *size, bold).map(|p| (p, Transform::from_translate(*x, *y).pre_rotate(*a).pre_translate(-lw / 2.0, 0.0).pre_concat(lean)))
                    })
                    .collect();
                // All the halos first, so that one letter's does not cover the next.
                if let Some(hc) = halo {
                    for (p, tf) in paths.iter().flatten() {
                        pm.stroke_path(p, &paint(hc), &st, *tf, None);
                    }
                }
                for (p, tf) in paths.iter().flatten() {
                    pm.fill_path(p, &paint(spec.colour), FillRule::Winding, *tf, None);
                }
            }
            let all = boxes.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
            taken.extend(boxes);
            return Some(all);
        }
        // The box the label takes, turned, as the box around that.
        let t = spot.angle.to_radians();
        let (c, s) = (t.cos(), t.sin());
        let x0 = -w * spot.align - pad;
        let x1 = x0 + w + 2.0 * pad;
        let (y0, y1) = (-h * 0.8 - pad - extra, h * 0.25 + pad + extra);
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(u, v)| (spot.x + u * c - v * s, spot.y + u * s + v * c));
        let bx = [
            corners.iter().map(|p| p.0).fold(f32::INFINITY, f32::min),
            corners.iter().map(|p| p.1).fold(f32::INFINITY, f32::min),
            corners.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max),
            corners.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max),
        ];
        if !spec.all {
            let inside = bx[0] >= 0.0 && bx[1] >= 0.0 && bx[2] <= pw && bx[3] <= ph;
            let clear = taken.iter().all(|o| bx[2] <= o[0] || bx[0] >= o[2] || bx[3] <= o[1] || bx[1] >= o[3]);
            if !inside || !clear {
                continue;
            }
        }
        if !dry {
            // Each line aligned as the label is: from the left beside a point to the right,
            // from the right to the left, and centred in the middle.
            let paths: Vec<(tiny_skia::Path, Transform)> = lines
                .iter()
                .enumerate()
                .filter_map(|(i, line)| {
                    // Small capitals, where the letters are not set one by one: all in
                    // capitals a little smaller.
                    let (size, bold) = line_look(spec, item, i);
                    let (line, size) = if spec.small_caps { (line.to_uppercase(), size * 0.84) } else { (line.to_string(), size) };
                    let lw = text.width(&line, size, bold);
                    let dy = i as f32 * line_h - extra;
                    text.path(&line, size, bold)
                        .map(|p| (p, Transform::from_translate(spot.x, spot.y).pre_rotate(spot.angle).pre_translate(-lw * spot.align, dy).pre_concat(lean)))
                })
                .collect();
            if let Some(hc) = spec.halo.filter(|_| spec.halo_width > 0.0) {
                let st = Stroke { width: spec.halo_width * 2.0, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                for (path, tf) in &paths {
                    pm.stroke_path(path, &paint(hc), &st, *tf, None);
                }
            }
            for (path, tf) in &paths {
                pm.fill_path(path, &paint(spec.colour), FillRule::Winding, *tf, None);
            }
        }
        taken.push(bx);
        return Some(bx);
    }
    None
}

/// The inset: the same layers, labels and looks, drawn again for its area at the size of its
/// box, laid in its corner with a frame, and the same frame on the map around the area.
fn draw_inset(
    pixmap: &mut Canvas,
    req: &Request,
    inset: &Inset,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
    going: &dyn Fn() -> bool,
    at: Option<(f32, f32)>,
) -> Result<()> {
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let a = inset.area;
    let box_w = (pw * inset.share.clamp(0.05, 0.8)).round().max(20.0) as u32;
    let sub = Request {
        layers: req.layers.clone(),
        themes: req.themes.clone(),
        // The towns and roads keep their names, but the areas are named on the map itself.
        labels: req.labels.iter().zip(&req.layers).map(|(l, (t, _))| l.clone().filter(|_| shape_of(t) != "area")).collect(),
        icons: req.icons.clone(),
        charts: req.charts.clone(),
        rasters: req.rasters.clone(),
        legend: None,
        width: box_w,
        margin: 0.0,
        background: req.background.or(Some([255, 255, 255, 255])),
        backdrop: None,
        attribution: None,
        // Its own scale bar, when the map has one, as the inset is at another scale.
        furniture: Furniture {
            extent: Some(a),
            knockout: req.furniture.knockout,
            simplify: req.furniture.simplify,
            scale_bar: if req.furniture.scale_bar.is_empty() || req.furniture.scale_bar == "none" { String::new() } else { "bottom-left".into() },
            scale_style: req.furniture.scale_style.clone(),
            dpi: req.furniture.dpi,
            ..Furniture::default()
        },
        names: req.names.clone(),
        overview: None,
        map_style: req.map_style.clone(),
    };
    let img = draw(&sub, going)?;
    let (w, h) = (img.width as f32, img.height as f32);
    if h > ph * 0.9 {
        return Err(GbError("The inset would be higher than the map: make it narrower, or pick out a wider area.".into()));
    }
    let b = inset_box(pw, ph, inset);
    let (x0, y0) = at.unwrap_or((b[0], if inset.corner.starts_with("top") { b[1] } else { b[3] - h }));
    paste(pixmap, &img, x0.round() as i32, y0.round() as i32);
    let ink = [0x33, 0x33, 0x33, 255];
    let st = Stroke { width: 1.5, ..Stroke::default() };
    if let Some(r) = tiny_skia::Rect::from_xywh(x0.round(), y0.round(), w, h) {
        pixmap.stroke_path(&PathBuilder::from_rect(r), &paint(ink), &st, Transform::identity(), None);
    }
    let (ax, ay) = to_px(a[0], a[3]);
    let (bx, by) = to_px(a[2], a[1]);
    if let Some(r) = tiny_skia::Rect::from_ltrb(ax.min(bx), ay.min(by), ax.max(bx), ay.max(by)) {
        pixmap.stroke_path(&PathBuilder::from_rect(r), &paint(ink), &st, Transform::identity(), None);
    }
    Ok(())
}

/// Puts a picture on the canvas with its upper left corner at x, y.
fn paste(pixmap: &mut Canvas, img: &RasterImage, x: i32, y: i32) {
    if img.vector.is_some() {
        pixmap.draw_picture(img, x as f32, y as f32);
        return;
    }
    let mut data = Vec::with_capacity(img.rgba.len());
    for p in img.rgba.chunks_exact(4) {
        let al = p[3] as u16;
        data.extend_from_slice(&[(p[0] as u16 * al / 255) as u8, (p[1] as u16 * al / 255) as u8, (p[2] as u16 * al / 255) as u8, p[3]]);
    }
    if let Some(pic) = tiny_skia::IntSize::from_wh(img.width as u32, img.height as u32).and_then(|size| Pixmap::from_vec(data, size)) {
        pixmap.draw_pixmap(x, y, pic.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
    }
}

/// The canvas as a picture, its colours no longer premultiplied.
fn finish(pixmap: Canvas, geotransform: [f64; 6]) -> RasterImage {
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    let mut rgba = Vec::with_capacity(width * height * 4);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let vector = pixmap.picture().map(std::sync::Arc::new);
    RasterImage { vector, width, height, rgba, geotransform }
}

/// The same map once for each value of an attribute of one layer, side by side: small
/// multiples. `values` holds each value with which features of the layer it keeps.
pub struct Multiples {
    pub layer: usize,
    pub values: Vec<(String, Vec<bool>)>,
    /// Maps across. 0: as many as make a square.
    pub columns: usize,
}

/// Small multiples: the maps in a grid at one scale, each named by its value, with the title
/// over them and one legend in a cell of its own.
pub fn draw_multiples(req: &Request, m: &Multiples, going: &dyn Fn() -> bool) -> Result<RasterImage> {
    let ext = match req.furniture.extent {
        Some(e) => e,
        None => {
            let e = extent_of(&req.layers).ok_or_else(|| GbError("There is nothing to draw: the layers have no geometry.".into()))?;
            let pad = (e[2] - e[0]).max(e[3] - e[1]) * req.margin.max(0.0);
            [e[0] - pad, e[1] - pad, e[2] + pad, e[3] + pad]
        }
    };
    let n = m.values.len();
    let cells = n + req.legend.is_some() as usize;
    let cols = if m.columns > 0 { m.columns } else { (cells as f64).sqrt().ceil() as usize }.clamp(1, cells.max(1));
    let rows = cells.div_ceil(cols);
    let wide = req.width as f32;
    let k = (wide / 1100.0).clamp(0.6, 4.0);
    let gap = (14.0 * k).round();
    let panel_w = ((wide - gap * (cols as f32 + 1.0)) / cols as f32).floor().max(40.0) as u32;
    let mut panels = Vec::with_capacity(n);
    for (i, (_, keep)) in m.values.iter().enumerate() {
        if !going() {
            return Err(GbError(crate::model::STOPPED.into()));
        }
        let mut t = req.layers[m.layer].0.clone();
        for (f, k) in t.features.iter_mut().zip(keep) {
            if !k {
                f.geom = None;
            }
        }
        let mut layers = req.layers.clone();
        layers[m.layer].0 = &t;
        let sub = Request {
            layers,
            legend: None,
            width: panel_w,
            margin: 0.0,
            backdrop: None,
            attribution: None,
            overview: None,
            furniture: Furniture {
                extent: Some(ext),
                scale_bar: if i == 0 { req.furniture.scale_bar.clone() } else { String::new() },
                north: String::new(),
                title: String::new(),
                subtitle: String::new(),
                logo: None,
                inset: None,
                graticule: Vec::new(),
                grid: String::new(),
                ..req.furniture.clone()
            },
            ..req.clone()
        };
        panels.push(draw(&sub, going)?);
    }
    let panel_h = panels.first().map_or(panel_w as f32, |p| p.height as f32);
    let text = crate::chart::Text::new()?;
    let f = &req.furniture;
    let pad = (wide / 50.0).max(6.0).round();
    let mut head = pad;
    if !f.title.trim().is_empty() {
        head += 26.0 * k * 1.35;
    }
    if !f.subtitle.trim().is_empty() {
        head += 15.0 * k * 1.55;
    }
    let label_h = (22.0 * k).round();
    let credit = req.attribution.as_ref().filter(|a| !a.trim().is_empty());
    let high = head + rows as f32 * (label_h + panel_h + gap) + gap + if credit.is_some() { 20.0 * k } else { 0.0 };
    let mut pixmap = Canvas::new(req.width, high.ceil() as u32, true).ok_or_else(|| GbError("There is not memory enough for a picture that large.".into()))?;
    let bg = req.background.unwrap_or([255, 255, 255, 255]);
    pixmap.fill(Color::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
    draw_title_and_logo(
        &mut pixmap,
        &Furniture { title: f.title.clone(), subtitle: f.subtitle.clone(), logo: f.logo.clone(), logo_corner: f.logo_corner.clone(), ..Furniture::default() },
        None,
        None,
    )?;
    let at = |i: usize| {
        let (c, r) = (i % cols, i / cols);
        (gap + c as f32 * (panel_w as f32 + gap), head + r as f32 * (label_h + panel_h + gap))
    };
    let frame = [0xc3, 0xc2, 0xb7, 255];
    for (i, (img, (name, _))) in panels.iter().zip(&m.values).enumerate() {
        let (x, y) = at(i);
        pixmap.layer(Some(name));
        write_text(&mut pixmap, &text, name, x, y + label_h * 0.72, 15.0 * k, true, [0x0b, 0x0b, 0x0b], 0.0);
        paste(&mut pixmap, img, x.round() as i32, (y + label_h).round() as i32);
        if let Some(r) = tiny_skia::Rect::from_xywh(x.round(), (y + label_h).round(), img.width as f32, img.height as f32) {
            pixmap.stroke_path(&PathBuilder::from_rect(r), &paint(frame), &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
        }
    }
    // The legend in the cell after the maps, as high as there is room for.
    if let Some(legend) = &req.legend {
        let (x, y) = at(n);
        let room = (high - y - label_h).max(panel_h).round() as u32;
        if let Some(mut cell) = Canvas::new(panel_w, room, false) {
            draw_legend(&mut cell, &Legend { corner: "top-left".into(), ..legend.clone() })?;
            let img = finish(cell, [0.0, 1.0, 0.0, 0.0, 0.0, -1.0]);
            pixmap.layer(Some("Legend"));
            paste(&mut pixmap, &img, x.round() as i32, (y + label_h).round() as i32);
        }
    }
    if let Some(a) = credit {
        draw_attribution(&mut pixmap, a)?;
    }
    Ok(finish(pixmap, [0.0, 1.0, 0.0, 0.0, 0.0, -1.0]))
}

/// Where the inset goes, in pixels: as wide as its share, as high as its area is for that.
fn inset_box(pw: f32, ph: f32, inset: &Inset) -> [f32; 4] {
    let a = inset.area;
    let w = (pw * inset.share.clamp(0.05, 0.8)).round().max(20.0);
    let h = (w as f64 * (a[3] - a[1]) / (a[2] - a[0]).max(f64::MIN_POSITIVE)).round() as f32;
    let pad = (pw.min(ph) / 50.0).max(6.0).round();
    let x0 = if inset.corner.ends_with("left") { pad } else { pw - pad - w };
    let y0 = if inset.corner.starts_with("top") { pad } else { ph - pad - h };
    [x0, y0, x0 + w, y0 + h]
}

/// Places in the corners and in the middle of the top and bottom edges. Things that share
/// a place are stacked: down from the top, up from the bottom, and in a new column beside the
/// first when there is no more room up or down.
struct Slots {
    pw: f32,
    ph: f32,
    pad: f32,
    gap: f32,
    /// For each place: how far from its edge it is taken, how far in from its side the column
    /// is, and how wide the column is.
    used: Vec<(String, f32, f32, f32)>,
    /// How far in from the top and the left edge things begin: past the coordinates written
    /// there by a grid.
    top: f32,
    left: f32,
}

impl Slots {
    fn new(pw: f32, ph: f32) -> Slots {
        let pad = (pw.min(ph) / 50.0).max(6.0).round();
        Slots { pw, ph, pad, gap: (pad * 0.5).round(), used: Vec::new(), top: pad, left: pad }
    }

    /// The place as it is written, or the lower right corner.
    fn name(place: &str) -> &str {
        match place {
            "top-left" | "top-right" | "bottom-left" | "top-center" | "bottom-center" => place,
            _ => "bottom-right",
        }
    }

    fn slot(&mut self, place: &str) -> usize {
        let place = Slots::name(place);
        match self.used.iter().position(|u| u.0 == place) {
            Some(i) => i,
            None => {
                self.used.push((place.to_string(), 0.0, 0.0, 0.0));
                self.used.len() - 1
            }
        }
    }

    /// Keeps `h` pixels of a place, from its edge, for something drawn there of its own.
    fn keep(&mut self, place: &str, h: f32) {
        let i = self.slot(place);
        self.used[i].1 = self.used[i].1.max(h);
    }

    /// Where something `w` by `h` goes in a place: after what is there already.
    fn place(&mut self, place: &str, w: f32, h: f32) -> (f32, f32) {
        let (pw, ph, pad, gap) = (self.pw, self.ph, self.pad, self.gap);
        let i = self.slot(place);
        let name = self.used[i].0.clone();
        let (mut u, mut col, mut col_w) = (self.used[i].1, self.used[i].2, self.used[i].3);
        // No room left up or down, with what the other end of the side holds: a new column
        // beside the one before.
        let other = if name.starts_with("top") { name.replacen("top", "bottom", 1) } else { name.replacen("bottom", "top", 1) };
        let across = self.used.iter().find(|o| o.0 == other).map_or(0.0, |o| o.1);
        if u > 0.0 && u + h > ph - pad - self.top - across && !name.ends_with("center") {
            col += col_w + gap;
            u = 0.0;
            col_w = 0.0;
        }
        let x = if name.ends_with("left") {
            self.left + col
        } else if name.ends_with("center") {
            (pw - w) / 2.0
        } else {
            pw - pad - col - w
        };
        let y = if name.starts_with("top") { self.top + u } else { ph - pad - u - h };
        self.used[i] = (name, u + h + gap, col, col_w.max(w));
        (x.round(), y.round())
    }
}

/// Where the things around the map go, found before any of them is drawn, so that they keep
/// clear of each other and the labels keep clear of them.
#[derive(Debug, Clone, Default)]
struct Layout {
    /// The upper left corner of the title and its subtitle, and how wide they are.
    title: Option<(f32, f32, f32)>,
    logo: Option<(f32, f32)>,
    scale_bar: Option<(f32, f32)>,
    north: Option<(f32, f32)>,
    legend: Option<(f32, f32)>,
    overview: Option<(f32, f32)>,
    inset: Option<(f32, f32)>,
    profile: Option<(f32, f32)>,
    globe: Option<(f32, f32)>,
    /// The boxes of them all, and of the title.
    boxes: Vec<[f32; 4]>,
}

/// Finds where the title, the credit, the logo, the north arrow, the scale bar, the legend,
/// the overview map and the inset go, in that order, each after those before it in its place.
fn plan(pixmap: &mut Canvas, req: &Request, legend: Option<&Legend>, main: [f64; 4], px: f64, credit: bool) -> Result<Layout> {
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let f = &req.furniture;
    let mut slots = Slots::new(pw, ph);
    let mut out = Layout::default();
    let t = crate::chart::Text::new()?;
    let on = |c: &str| !c.is_empty() && c != "none";
    // A grid writes its coordinates along the top and the left edge: what goes there begins
    // past them, so that they can be read.
    if (on(&f.grid) || !f.graticule.is_empty()) && !f.coords_outside {
        let gk = (pw.min(ph) / 900.0).clamp(0.8, 3.0);
        let band = (8.0 * gk + 10.5 * gk * 1.6 + 6.0 * gk).round();
        slots.top = slots.top.max(band);
        slots.left = slots.left.max(band);
    }
    // The title first, at the edge of its place. A wide one keeps the places beside it free
    // too on that edge.
    let k = (pw.min(ph) / 800.0).clamp(0.6, 4.0);
    let (ts, ss) = title_sizes(f, k);
    let tt = title_text(f)?;
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    if !f.title.trim().is_empty() {
        w = w.max(tt.width(f.title.trim(), ts, !f.title_regular));
        h += ts * 1.35;
    }
    if !f.subtitle.trim().is_empty() {
        w = w.max(tt.width(f.subtitle.trim(), ss, false));
        h += ss * 1.55;
    }
    if w > 0.0 {
        // With room inside a box around it.
        let inner = title_room(f, k);
        let w = w.min(pw - 2.0 * slots.pad - 2.0 * inner) + 2.0 * inner;
        let h = h + 2.0 * inner;
        let place = if f.title_place.is_empty() { "top-left" } else { f.title_place.as_str() };
        let (x, y) = slots.place(place, w, h);
        out.title = Some((x, y, w));
        out.boxes.push([x - 2.0, y - 2.0, x + w + 4.0, y + h + 4.0]);
        let edge = if place.starts_with("top") { "top" } else { "bottom" };
        let taken = h + slots.gap + if edge == "top" { y - slots.top } else { 0.0 };
        for side in ["left", "center", "right"] {
            let other = format!("{}-{}", edge, side);
            if other == Slots::name(place) {
                continue;
            }
            // The part of the edge that the place beside it covers, as thirds of the width.
            let (a, b) = match side {
                "left" => (0.0, pw / 3.0),
                "center" => (pw / 3.0, pw * 2.0 / 3.0),
                _ => (pw * 2.0 / 3.0, pw),
            };
            if x < b && x + w > a {
                slots.keep(&other, taken);
            }
        }
    }
    // The credit is written along the bottom on the right; a band with the footer takes the
    // whole bottom.
    if !f.footer.trim().is_empty() {
        let band = footer_height(pw);
        slots.keep("bottom-right", (band - slots.pad * 0.5).max(0.0) + slots.gap);
        slots.keep("bottom-left", (band - slots.pad * 0.5).max(0.0) + slots.gap);
    } else if credit {
        let size = (pw / 140.0).clamp(10.0, 18.0);
        slots.keep("bottom-right", (size * 1.6 - slots.pad * 0.5).max(0.0) + slots.gap);
    }
    let put = |slots: &mut Slots, out: &mut Layout, place: &str, w: f32, h: f32| -> (f32, f32) {
        let (x, y) = slots.place(place, w, h);
        out.boxes.push([x - 2.0, y - 2.0, x + w + 2.0, y + h + 2.0]);
        (x, y)
    };
    if let Some(logo) = &f.logo {
        out.logo = Some(put(&mut slots, &mut out, &f.logo_corner, logo.width as f32, logo.height as f32));
    }
    let fk = (pw.min(ph) / 900.0).clamp(0.8, 3.0);
    if on(&f.north) {
        let (aw, ah, nh) = (16.0 * fk, 30.0 * fk, 12.0 * fk * 1.4);
        out.north = Some(put(&mut slots, &mut out, &f.north, aw, ah + nh));
    }
    if on(&f.scale_bar) {
        let (bw, bh) = scale_bar_box(f, main, px, pw, ph, &t);
        out.scale_bar = Some(put(&mut slots, &mut out, &f.scale_bar, bw, bh));
    }
    if let Some(l) = legend {
        let (w, h) = draw_legend_at(pixmap, l, None, true)?;
        if w > 0.0 {
            out.legend = Some(put(&mut slots, &mut out, &l.corner, w, h));
        }
    }
    if let Some(ov) = &req.overview {
        let (bw, bh) = ov.size(pixmap.width(), pixmap.height(), ov.extent(main));
        out.overview = Some(put(&mut slots, &mut out, &ov.corner, bw as f32, bh as f32));
    }
    if let Some(inset) = &f.inset {
        let b = inset_box(pw, ph, inset);
        out.inset = Some(put(&mut slots, &mut out, &inset.corner, b[2] - b[0], b[3] - b[1]));
    }
    if let Some(pr) = &f.profile {
        let (w, h) = extra::profile_size(pw, ph, f.profile.as_ref().map_or(0.34, |p| p.share));
        out.profile = Some(put(&mut slots, &mut out, &pr.corner, w, h));
    }
    if let Some(g) = &f.globe {
        let d = extra::globe_size(pw, ph);
        out.globe = Some(put(&mut slots, &mut out, &g.corner, d, d));
    }
    Ok(out)
}

/// The size of the scale bar with its numbers: as long as a round distance near a fifth of the
/// map.
fn scale_bar_box(f: &Furniture, b: [f64; 4], px: f64, pw: f32, ph: f32, t: &crate::chart::Text) -> (f32, f32) {
    let (bar_w, bar_h, last, size) = scale_bar_parts(f, b, px, pw, ph);
    let lw = t.width(&last, size, false);
    let figures = t.width(&scale_figures(f, b, px), size * 1.1, false);
    match f.scale_number.as_str() {
        "number" => (figures + 4.0, size * 1.6),
        "both" => ((bar_w + lw / 2.0 + 4.0).max(figures + 4.0), bar_h + size * 1.6 + size * 1.5),
        _ => (bar_w + lw / 2.0 + 4.0, bar_h + size * 1.6),
    }
}

/// The scale in figures, as 1:50 000, to three figures that count.
fn scale_figures(f: &Furniture, b: [f64; 4], px: f64) -> String {
    let s = scale_of(f, px, (b[1] + b[3]) / 2.0);
    let mag = 10f64.powf((s.log10().floor() - 2.0).max(0.0));
    let s = (s / mag).round() * mag;
    format!("1:{}", crate::chart::number(s, 0, true).replace('\u{a0}', " "))
}

/// The scale bar: its width and height in pixels, the text at its end and the size of its
/// text.
fn scale_bar_parts(f: &Furniture, b: [f64; 4], px: f64, pw: f32, ph: f32) -> (f32, f32, String, f32) {
    let k = (pw.min(ph) / 900.0).clamp(0.8, 3.0);
    let metres_per_px = if f.geographic { px * 111_320.0 * ((b[1] + b[3]) / 2.0).to_radians().cos().abs().max(0.01) } else { px };
    let want = metres_per_px * pw as f64 / 5.0;
    let p10 = 10f64.powf(want.log10().floor());
    let length = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * p10).rfind(|l| *l <= want).unwrap_or(p10);
    let bar_w = (length / metres_per_px) as f32;
    let km = match f.scale_unit.as_str() {
        "km" => true,
        "m" => false,
        _ => length >= 1000.0,
    };
    let last = if km { format!("{} km", crate::model::fmt_real(length / 1000.0)) } else { format!("{} m", crate::model::fmt_real(length)) };
    (bar_w, 6.0 * k, last, 12.0 * k)
}

/// The letters of the title: from its font file, or the built-in ones.
fn title_text(f: &Furniture) -> Result<crate::chart::Text> {
    if f.title_font.trim().is_empty() {
        return crate::chart::Text::new();
    }
    let bytes = std::fs::read(f.title_font.trim()).map_err(|e| GbError(format!("The font {} cannot be read: {}", f.title_font.trim(), e)))?;
    crate::chart::Text::with_font(bytes)
}

/// The sizes of the title and the subtitle in pixels, for a picture of this scale.
fn title_sizes(f: &Furniture, k: f32) -> (f32, f32) {
    let ts = if f.title_size > 0.0 { f.title_size } else { 26.0 };
    let ss = if f.subtitle_size > 0.0 { f.subtitle_size } else { 15.0 };
    (ts * k, ss * k)
}

/// The room between a box behind the title and what is written in it: none without a box.
fn title_room(f: &Furniture, k: f32) -> f32 {
    if f.title_fill.is_some() || f.title_line.is_some() {
        (10.0 * k).round()
    } else {
        0.0
    }
}

/// The title and its subtitle at the top left, and a logo in its corner.
fn draw_title_and_logo(pixmap: &mut Canvas, f: &Furniture, logo_at: Option<(f32, f32)>, title_at: Option<(f32, f32, f32)>) -> Result<()> {
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let k = (pw.min(ph) / 800.0).clamp(0.6, 4.0);
    let pad = (pw.min(ph) / 50.0).max(6.0).round();
    if !f.title.trim().is_empty() || !f.subtitle.trim().is_empty() {
        let t = title_text(f)?;
        let (ts, ss) = title_sizes(f, k);
        let bold = !f.title_regular;
        let inner = title_room(f, k);
        let (bx, by, bw) = title_at.unwrap_or((pad, pad, pw - 2.0 * pad));
        // The box behind it, as high as what is written in it.
        if title_at.is_some() && (f.title_fill.is_some() || f.title_line.is_some()) {
            let mut bh = 2.0 * inner;
            if !f.title.trim().is_empty() {
                bh += ts * 1.35;
            }
            if !f.subtitle.trim().is_empty() {
                bh += ss * 1.55;
            }
            if let Some(r) = tiny_skia::Rect::from_xywh(bx, by, bw, bh) {
                let path = PathBuilder::from_rect(r);
                if let Some(c) = f.title_fill {
                    pixmap.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
                }
                if let Some(c) = f.title_line {
                    pixmap.stroke_path(&path, &paint(c), &Stroke { width: (1.0 * k).max(1.0), ..Stroke::default() }, Transform::identity(), None);
                }
            }
        }
        let (x0, mut y, w) = (bx + inner, by + inner, bw - 2.0 * inner);
        // Lined up on the side of its place, or in the middle.
        let align = if f.title_place.ends_with("center") {
            0.5
        } else if f.title_place.ends_with("right") {
            1.0
        } else {
            0.0
        };
        if !f.title.trim().is_empty() {
            y += ts;
            let s = t.fit(f.title.trim(), ts, bold, pw - 2.0 * pad);
            let lw = t.width(&s, ts, bold);
            halo_text(pixmap, &t, &s, x0 + (w - lw).max(0.0) * align, y, ts, bold, f.title_colour.unwrap_or([0x11, 0x11, 0x11, 255]));
            y += ts * 0.35;
        }
        if !f.subtitle.trim().is_empty() {
            y += ss * 1.15;
            let s = t.fit(f.subtitle.trim(), ss, false, pw - 2.0 * pad);
            let lw = t.width(&s, ss, false);
            halo_text(pixmap, &t, &s, x0 + (w - lw).max(0.0) * align, y, ss, false, f.subtitle_colour.unwrap_or([0x44, 0x44, 0x44, 255]));
        }
    }
    // The logo comes as large as it is to be drawn.
    if let Some(logo) = &f.logo {
        let (w, h) = (logo.width as f32, logo.height as f32);
        let (x0, y0) = logo_at.unwrap_or_else(|| Slots::new(pw, ph).place(&f.logo_corner, w, h));
        let mut data = Vec::with_capacity(logo.rgba.len());
        for p in logo.rgba.chunks_exact(4) {
            let al = p[3] as u16;
            data.extend_from_slice(&[(p[0] as u16 * al / 255) as u8, (p[1] as u16 * al / 255) as u8, (p[2] as u16 * al / 255) as u8, p[3]]);
        }
        if let Some(pic) = tiny_skia::IntSize::from_wh(logo.width as u32, logo.height as u32).and_then(|size| Pixmap::from_vec(data, size)) {
            pixmap.draw_pixmap(x0.round() as i32, y0.round() as i32, pic.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
        }
    }
    Ok(())
}

/// The size and weight of a line of a label: after the first, a description smaller and
/// not bold, when the layer asks for it.
fn line_look(spec: &LabelSpec, item: &LabelItem, line: usize) -> (f32, bool) {
    if spec.detail && line > 0 {
        (item.size * 0.76, false)
    } else {
        (item.size, item.bold)
    }
}

/// The width of a label: that of its longest line.
fn label_width(text: &crate::chart::Text, s: &str, size: f32, bold: bool) -> f32 {
    s.split('\n').map(|l| text.width(l, size, bold)).fold(0.0, f32::max)
}

/// A coordinate as it is written in the edge of a map: metres with spaces between the
/// thousands, degrees with their sign.
fn coordinate_text(v: f64, step: f64, geographic: bool) -> String {
    if geographic {
        let decimals = if step >= 1.0 { 0 } else { (-step.log10()).ceil().max(0.0) as usize };
        format!("{:.*}°", decimals, v)
    } else {
        let n = v.round() as i64;
        let digits = n.unsigned_abs().to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(' ');
            }
            out.push(c);
        }
        if n < 0 {
            format!("-{}", out)
        } else {
            out
        }
    }
}

/// A text with a white edge, so that it can be read on anything.
#[allow(clippy::too_many_arguments)]
fn halo_text(pixmap: &mut Canvas, t: &crate::chart::Text, s: &str, x: f32, y: f32, size: f32, bold: bool, colour: [u8; 4]) {
    if let Some(path) = t.path(s, size, bold) {
        let at = Transform::from_translate(x, y);
        let edge = Stroke { width: size * 0.25, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
        pixmap.stroke_path(&path, &paint([255, 255, 255, 230]), &edge, at, None);
        pixmap.fill_path(&path, &paint(colour), FillRule::Winding, at, None);
    }
}

/// The scale bar, the north arrow and the grid. `b` is the west, south, east and north edge
/// of the picture and `px` the size of a pixel. With `credit`, the lower right corner keeps
/// room for the line that says who the background map is from.
fn draw_furniture(pixmap: &mut Canvas, f: &Furniture, b: [f64; 4], px: f64, layout: &Layout) -> Result<()> {
    let on = |c: &str| !c.is_empty() && c != "none";
    if !on(&f.scale_bar) && !on(&f.north) && !on(&f.grid) && f.graticule.is_empty() {
        return Ok(());
    }
    let t = crate::chart::Text::new()?;
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let k = (pw.min(ph) / 900.0).clamp(0.8, 3.0);
    let ink = [0x22, 0x22, 0x22, 255];
    let size = 12.0 * k;
    // Where the scale bar and the north arrow go, found by the plan, so that the coordinates
    // keep clear of them.
    let (bar_w, bar_h, last, _) = scale_bar_parts(f, b, px, pw, ph);
    let lw = t.width(&last, size, false);
    let bar_box = scale_bar_box(f, b, px, pw, ph, &t);
    let bar_at = layout.scale_bar.filter(|_| on(&f.scale_bar));
    let (aw, ah, nh) = (16.0 * k, 30.0 * k, size * 1.4);
    let north_at = layout.north.filter(|_| on(&f.north));
    let mut keep_clear: Vec<[f32; 4]> = Vec::new();
    if let Some((x, y)) = bar_at {
        keep_clear.push([x - 12.0 * k, y - 8.0 * k, x + bar_box.0 + 12.0 * k, y + bar_box.1 + 14.0 * k]);
    }
    if let Some((x, y)) = north_at {
        keep_clear.push([x - 8.0 * k, y - 8.0 * k, x + aw + 8.0 * k, y + ah + nh + 8.0 * k]);
    }
    // And of the title, the legend, the overview map and the rest around the map.
    keep_clear.extend(layout.boxes.iter().copied());
    let clear = |r: [f32; 4]| !keep_clear.iter().any(|c| r[0] < c[2] && r[2] > c[0] && r[1] < c[3] && r[3] > c[1]);
    // The lines of latitude and longitude, with their degrees where they leave the map at the
    // top and on the left.
    if !f.graticule.is_empty() {
        let to = |(x, y): (f64, f64)| (((x - b[0]) / px) as f32, ((b[3] - y) / px) as f32);
        let mut pb = PathBuilder::new();
        let small = 10.5 * k;
        let mut labels: Vec<(f32, f32, &str, bool)> = Vec::new();
        // Cut along the degrees: the lines end at the curved edge, and are named there, above
        // it and to the left of it.
        let curved = f.focus_only && !f.focus.is_empty() && f.grid == "degrees";
        let mut ends: Vec<(f32, f32, &str, bool, bool)> = Vec::new();
        for g in &f.graticule {
            for (i, p) in g.points.iter().enumerate() {
                let (x, y) = to(*p);
                if i == 0 {
                    pb.move_to(x, y);
                } else {
                    pb.line_to(x, y);
                }
            }
            if curved {
                // Both ends: above and below the edge, and left and right of it.
                let pts: Vec<(f32, f32)> = g.points.iter().map(|p| to(*p)).collect();
                let key = |p: &(f32, f32)| if g.meridian { p.1 } else { p.0 };
                let first = pts.iter().copied().min_by(|a, b| key(a).total_cmp(&key(b)));
                let last = pts.iter().copied().max_by(|a, b| key(a).total_cmp(&key(b)));
                for (end, before) in [(first, true), (last, false)] {
                    if let Some((x, y)) = end {
                        ends.push((x, y, &g.label, g.meridian, before));
                    }
                }
                continue;
            }
            // Where the line crosses the top edge (a meridian) or the left one.
            for w in g.points.windows(2) {
                let ((x0, y0), (x1, y1)) = (to(w[0]), to(w[1]));
                if g.meridian && (y0 - 0.0) * (y1 - 0.0) <= 0.0 && y0 != y1 {
                    let x = x0 + (x1 - x0) * (0.0 - y0) / (y1 - y0);
                    labels.push((x, 0.0, &g.label, true));
                    break;
                }
                if !g.meridian && x0 * x1 <= 0.0 && x0 != x1 {
                    let y = y0 + (y1 - y0) * (0.0 - x0) / (x1 - x0);
                    labels.push((0.0, y, &g.label, false));
                    break;
                }
            }
        }
        if let Some(path) = pb.finish() {
            let st = Stroke { width: (0.8 * k).max(0.6), ..Stroke::default() };
            pixmap.stroke_path(&path, &paint([0x22, 0x22, 0x22, 80]), &st, Transform::identity(), None);
        }
        for (x, y, s, meridian, before) in ends {
            let w = t.width(s, small, false);
            let (lx, ly) = match (meridian, before) {
                (true, true) => (x - w / 2.0, y - small * 0.6),
                (true, false) => (x - w / 2.0, y + small * 1.5),
                (false, true) => (x - w - small * 0.5, y + small * 0.35),
                (false, false) => (x + small * 0.5, y + small * 0.35),
            };
            if lx > 2.0 && ly - small > 2.0 && lx + w < pw - 2.0 && ly < ph - 2.0 && clear([lx, ly - small, lx + w, ly]) {
                halo_text(pixmap, &t, s, lx, ly, small, false, ink);
            }
        }
        for (x, y, s, top) in labels.into_iter().filter(|_| !f.coords_outside) {
            let w = t.width(s, small, false);
            if top {
                if x - w / 2.0 > 2.0 && x + w / 2.0 < pw - 2.0 && clear([x - w / 2.0, 0.0, x + w / 2.0, small * 1.6]) {
                    halo_text(pixmap, &t, s, x - w / 2.0, small * 1.3, small, false, ink);
                }
            } else if y - w / 2.0 > 2.0 && y + w / 2.0 < ph - 2.0 && clear([0.0, y - w / 2.0, small * 1.6, y + w / 2.0]) {
                if let Some(path) = t.path(s, small, false) {
                    let at = Transform::from_rotate(-90.0).post_translate(small * 1.3, y + w / 2.0);
                    let edge = Stroke { width: small * 0.25, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                    pixmap.stroke_path(&path, &paint([255, 255, 255, 230]), &edge, at, None);
                    pixmap.fill_path(&path, &paint(ink), FillRule::Winding, at, None);
                }
            }
        }
    }
    // The grid, under the rest: lines at round coordinates, and the coordinates in the edge.
    if on(&f.grid) && !matches!(f.grid.as_str(), "degrees" | "utm" | "sheets") {
        let step = f.grid_every.filter(|s| *s > 0.0).unwrap_or_else(|| crate::chart::nice_step((b[2] - b[0]).max(b[3] - b[1]), 5.0));
        if (b[2] - b[0]) / step <= 200.0 && (b[3] - b[1]) / step <= 200.0 {
            let to_x = |x: f64| ((x - b[0]) / px) as f32;
            let to_y = |y: f64| ((b[3] - y) / px) as f32;
            let lines = f.grid == "lines";
            let tick = 8.0 * k;
            let line_paint = paint([0x22, 0x22, 0x22, if lines { 70 } else { 200 }]);
            let st = Stroke { width: (0.8 * k).max(0.6), ..Stroke::default() };
            let mut pb = PathBuilder::new();
            let mut x = (b[0] / step).ceil() * step;
            let mut xs = Vec::new();
            while x <= b[2] {
                let sx = to_x(x);
                if lines {
                    pb.move_to(sx, 0.0);
                    pb.line_to(sx, ph);
                } else {
                    pb.move_to(sx, 0.0);
                    pb.line_to(sx, tick);
                    pb.move_to(sx, ph - tick);
                    pb.line_to(sx, ph);
                }
                xs.push((sx, x));
                x += step;
            }
            let mut y = (b[1] / step).ceil() * step;
            let mut ys = Vec::new();
            while y <= b[3] {
                let sy = to_y(y);
                if lines {
                    pb.move_to(0.0, sy);
                    pb.line_to(pw, sy);
                } else {
                    pb.move_to(0.0, sy);
                    pb.line_to(tick, sy);
                    pb.move_to(pw - tick, sy);
                    pb.line_to(pw, sy);
                }
                ys.push((sy, y));
                y += step;
            }
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &line_paint, &st, Transform::identity(), None);
            }
            let small = 10.5 * k;
            // East along the top edge, north along the left one, turned to run up it; or
            // outside, by the frame.
            let (xs, ys) = if f.coords_outside { (Vec::new(), Vec::new()) } else { (xs, ys) };
            for (sx, v) in xs {
                let s = coordinate_text(v, step, f.geographic);
                let w = t.width(&s, small, false);
                if sx - w / 2.0 > 2.0 && sx + w / 2.0 < pw - 2.0 && clear([sx - w / 2.0, 0.0, sx + w / 2.0, tick + small * 1.4]) {
                    halo_text(pixmap, &t, &s, sx - w / 2.0, tick + small * 1.1, small, false, ink);
                }
            }
            for (sy, v) in ys {
                let s = coordinate_text(v, step, f.geographic);
                let w = t.width(&s, small, false);
                if sy - w / 2.0 > 2.0 && sy + w / 2.0 < ph - 2.0 && clear([0.0, sy - w / 2.0, tick + small * 1.4, sy + w / 2.0]) {
                    if let Some(path) = t.path(&s, small, false) {
                        let at = Transform::from_rotate(-90.0).post_translate(tick + small * 1.1, sy + w / 2.0);
                        let edge = Stroke { width: small * 0.25, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                        pixmap.stroke_path(&path, &paint([255, 255, 255, 230]), &edge, at, None);
                        pixmap.fill_path(&path, &paint(ink), FillRule::Winding, at, None);
                    }
                }
            }
        }
    }
    if let (Some((x, y)), "number") = (bar_at, f.scale_number.as_str()) {
        // The figures alone, on a pale box.
        let (w, h) = bar_box;
        if let Some(r) = tiny_skia::Rect::from_xywh(x - 8.0 * k, y - 4.0 * k, w + 16.0 * k, h + 6.0 * k) {
            pixmap.fill_rect(r, &paint([255, 255, 255, 200]), Transform::identity(), None);
        }
        halo_text(pixmap, &t, &scale_figures(f, b, px), x, y + size * 1.2, size * 1.1, false, ink);
    } else if let Some((x, y)) = bar_at {
        let (w, h) = bar_box;
        let top = y + size * 1.3;
        // A box behind it, so that it can be read on a background map.
        if let Some(r) = tiny_skia::Rect::from_xywh(x - 8.0 * k, y - 4.0 * k, w + 16.0 * k, h + 10.0 * k) {
            pixmap.fill_rect(r, &paint([255, 255, 255, 200]), Transform::identity(), None);
        }
        let parts = if f.scale_parts > 0 { f.scale_parts.min(10) } else { 4 };
        let each = bar_w / parts as f32;
        let thin = Stroke { width: (k * 0.8).max(0.6), ..Stroke::default() };
        if f.scale_style == "line" {
            // A line along the bottom, with a tick up at every field.
            let mut pb = PathBuilder::new();
            pb.move_to(x, top + bar_h);
            pb.line_to(x + bar_w, top + bar_h);
            for part in 0..=parts {
                let tx = x + part as f32 * each;
                let up = if part == 0 || part == parts { bar_h * 1.3 } else { bar_h * 0.7 };
                pb.move_to(tx, top + bar_h);
                pb.line_to(tx, top + bar_h - up);
            }
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint(ink), &Stroke { width: (k * 1.2).max(0.8), ..Stroke::default() }, Transform::identity(), None);
            }
        } else {
            for part in 0..parts {
                if let Some(r) = tiny_skia::Rect::from_xywh(x + part as f32 * each, top, each, bar_h) {
                    let c = if part % 2 == 0 { ink } else { [255, 255, 255, 255] };
                    pixmap.fill_rect(r, &paint(c), Transform::identity(), None);
                }
            }
            if let Some(r) = tiny_skia::Rect::from_xywh(x, top, bar_w, bar_h) {
                let path = PathBuilder::from_rect(r);
                pixmap.stroke_path(&path, &paint(ink), &thin, Transform::identity(), None);
            }
        }
        halo_text(pixmap, &t, "0", x - t.width("0", size, false) / 2.0, top - size * 0.35, size, false, ink);
        halo_text(pixmap, &t, &last, x + bar_w - lw / 2.0, top - size * 0.35, size, false, ink);
        if f.scale_number == "both" {
            halo_text(pixmap, &t, &scale_figures(f, b, px), x, y + bar_box.1 - size * 0.15, size * 1.1, false, ink);
        }
    }
    if let Some((x, y)) = north_at {
        // An arrow, half dark and half light, with N above it. North is up on the map.
        let (cx, top, foot) = (x + aw / 2.0, y + nh, y + nh + ah);
        let notch = foot - ah * 0.28;
        let mut left = PathBuilder::new();
        left.move_to(cx, top);
        left.line_to(x, foot);
        left.line_to(cx, notch);
        left.close();
        let mut right = PathBuilder::new();
        right.move_to(cx, top);
        right.line_to(x + aw, foot);
        right.line_to(cx, notch);
        right.close();
        let edge = Stroke { width: (k * 1.0).max(0.8), line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
        if f.north_style == "simple" {
            // A thin line with a head at the top.
            let mut pb = PathBuilder::new();
            pb.move_to(cx, top + ah * 0.25);
            pb.line_to(cx, foot);
            if let Some(p) = pb.finish() {
                pixmap.stroke_path(&p, &paint(ink), &Stroke { width: (k * 1.4).max(1.0), ..Stroke::default() }, Transform::identity(), None);
            }
            draw_heads(pixmap, &[(cx, top, 0.0, -1.0, ah * 0.4)], 1.4 * k, ink);
            left = PathBuilder::new();
            right = PathBuilder::new();
        } else if f.north_style == "star" {
            // Four points, the one to the north dark on one side; the others light.
            let r = ah / 2.0;
            let (cy, inner) = (top + r, r * 0.22);
            for (dx, dy) in [(0.0f32, -1.0f32), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
                let tip = (cx + dx * r, cy + dy * r);
                let (sx, sy) = (-dy, dx);
                let mut a = PathBuilder::new();
                a.move_to(tip.0, tip.1);
                a.line_to(cx + sx * inner, cy + sy * inner);
                a.line_to(cx, cy);
                a.close();
                let mut b = PathBuilder::new();
                b.move_to(tip.0, tip.1);
                b.line_to(cx - sx * inner, cy - sy * inner);
                b.line_to(cx, cy);
                b.close();
                if let (Some(a), Some(b)) = (a.finish(), b.finish()) {
                    pixmap.fill_path(&a, &paint(ink), FillRule::Winding, Transform::identity(), None);
                    pixmap.fill_path(&b, &paint([255, 255, 255, 255]), FillRule::Winding, Transform::identity(), None);
                    pixmap.stroke_path(&a, &paint(ink), &edge, Transform::identity(), None);
                    pixmap.stroke_path(&b, &paint(ink), &edge, Transform::identity(), None);
                }
            }
            left = PathBuilder::new();
            right = PathBuilder::new();
        }
        if let (Some(l), Some(r)) = (left.finish(), right.finish()) {
            pixmap.fill_path(&l, &paint(ink), FillRule::Winding, Transform::identity(), None);
            pixmap.fill_path(&r, &paint([255, 255, 255, 255]), FillRule::Winding, Transform::identity(), None);
            pixmap.stroke_path(&l, &paint(ink), &edge, Transform::identity(), None);
            pixmap.stroke_path(&r, &paint(ink), &edge, Transform::identity(), None);
        }
        let nw = t.width("N", size * 1.1, true);
        halo_text(pixmap, &t, "N", cx - nw / 2.0, y + size * 1.05, size * 1.1, true, ink);
    }
    Ok(())
}

/// Writes who a background map is from in the lower right corner, small, on a pale band.
/// How high the band along the bottom is, on a picture `pw` pixels wide.
fn footer_height(pw: f32) -> f32 {
    (pw / 140.0).clamp(10.0, 18.0) * 2.3
}

/// A band along the bottom of the map: the footer on the left, such as the sources, and the
/// credit on the right, over a thin line.
fn draw_footer(pixmap: &mut Canvas, footer: &str, credit: Option<&str>) -> Result<()> {
    let t = crate::chart::Text::new()?;
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let size = (pw / 140.0).clamp(10.0, 18.0);
    let band = footer_height(pw);
    if let Some(r) = tiny_skia::Rect::from_xywh(0.0, ph - band, pw, band) {
        pixmap.fill_rect(r, &paint([0xf6, 0xf2, 0xe9, 245]), Transform::identity(), None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(0.0, ph - band);
    pb.line_to(pw, ph - band);
    if let Some(p) = pb.finish() {
        pixmap.stroke_path(&p, &paint([0x99, 0x92, 0x86, 255]), &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
    }
    let y = ph - band / 2.0 + size * 0.36;
    write_text(pixmap, &t, footer, size * 1.2, y, size, false, [0x44, 0x40, 0x3a], 0.0);
    if let Some(c) = credit {
        write_text(pixmap, &t, c, pw - size * 1.2, y, size, false, [0x44, 0x40, 0x3a], 1.0);
    }
    Ok(())
}

fn draw_attribution(pixmap: &mut Canvas, text: &str) -> Result<()> {
    let t = crate::chart::Text::new()?;
    let size = (pixmap.width() as f32 / 140.0).clamp(10.0, 18.0);
    let w = t.width(text, size, false);
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let (x, y) = (pw - w - size * 0.6, ph - size * 0.45);
    if let Some(r) = tiny_skia::Rect::from_xywh(x - size * 0.4, y - size * 1.05, w + size * 0.8, size * 1.45) {
        pixmap.fill_rect(r, &paint([255, 255, 255, 190]), Transform::identity(), None);
    }
    if let Some(path) = t.path(text, size, false) {
        pixmap.fill_path(&path, &paint([60, 60, 60, 255]), FillRule::Winding, Transform::from_translate(x, y), None);
    }
    Ok(())
}

/// Draws the legend in a box in its corner.
/// A part of the legend larger than a row: drawn from its top left corner.
enum Block<'a> {
    /// The square of nine colours of two attributes, with an arrow along each.
    Bivariate(&'a crate::thematic::Bivariate),
    /// Circles within each other, standing on one line, each with its value: (diameter, text).
    Nested(Vec<(f32, String)>, crate::thematic::Look),
    /// Bars as high as how many features each class has, in its colour.
    Histogram(Vec<([u8; 4], usize)>),
    /// A band of colours from one end of a ramp to the other, with values under it.
    Ramp(&'a crate::rasterlook::RampLegend),
}

impl Block<'_> {
    fn size(&self, k: f32, t: &crate::chart::Text) -> (f32, f32) {
        let small = 12.5 * k;
        match self {
            Block::Bivariate(b) => {
                let c = 14.0 * k;
                let tw = t.width(&b.a, small, false).max(t.width(&b.b, small, false));
                (8.0 * k + 3.0 * c + 8.0 * k + tw, 3.0 * c + 14.0 * k)
            }
            Block::Nested(circles, _) => {
                let d = circles.iter().map(|c| c.0).fold(0.0f32, f32::max);
                let tw = circles.iter().map(|c| t.width(&c.1, small, false)).fold(0.0f32, f32::max);
                // As high as the circles, or as the values one under the other when the circles
                // are small.
                (d + 14.0 * k + tw, d.max(circles.len() as f32 * small * 1.1) + small * 0.6 + 6.0 * k)
            }
            Block::Histogram(bars) => (bars.len() as f32 * 13.0 * k, 32.0 * k),
            Block::Ramp(r) => {
                let last = r.ticks.last().map_or(0.0, |t| t.1.as_str().len() as f32);
                ((200.0 * k).max(last * small * 0.6 * 2.0), 12.0 * k + 6.0 * k + small * 1.2)
            }
        }
    }

    fn draw(&self, pm: &mut Canvas, x: f32, y: f32, k: f32, t: &crate::chart::Text) {
        let small = 12.5 * k;
        let ink = [0x44, 0x44, 0x44, 255];
        match self {
            Block::Bivariate(b) => {
                let c = 14.0 * k;
                let gx = x + 8.0 * k;
                for (n, colour) in b.colours.iter().enumerate() {
                    let (a, bb) = (n % 3, n / 3);
                    if let Some(r) = tiny_skia::Rect::from_xywh(gx + a as f32 * c, y + (2 - bb) as f32 * c, c, c) {
                        pm.fill_rect(r, &paint(*colour), Transform::identity(), None);
                    }
                }
                // Up for the second attribute, beside the grid; right for the first, under it.
                let st = Stroke { width: 1.2 * k, ..Stroke::default() };
                let mut up = PathBuilder::new();
                up.move_to(x + 3.0 * k, y + 3.0 * c);
                up.line_to(x + 3.0 * k, y + 4.0 * k);
                let mut right = PathBuilder::new();
                right.move_to(gx, y + 3.0 * c + 5.0 * k);
                right.line_to(gx + 3.0 * c - 4.0 * k, y + 3.0 * c + 5.0 * k);
                for p in [up.finish(), right.finish()].into_iter().flatten() {
                    pm.stroke_path(&p, &paint(ink), &st, Transform::identity(), None);
                }
                draw_heads(pm, &[(x + 3.0 * k, y, 0.0, -1.0, 5.0 * k), (gx + 3.0 * c, y + 3.0 * c + 5.0 * k, 1.0, 0.0, 5.0 * k)], 1.2 * k, ink);
                let tx = gx + 3.0 * c + 8.0 * k;
                write_text(pm, t, &b.b, tx, y + small * 0.8, small, false, [0x0b, 0x0b, 0x0b], 0.0);
                write_text(pm, t, &b.a, tx, y + 3.0 * c + 5.0 * k + small * 0.36, small, false, [0x0b, 0x0b, 0x0b], 0.0);
            }
            Block::Nested(circles, look) => {
                let d = circles.iter().map(|c| c.0).fold(0.0f32, f32::max);
                let (cx, base) = (x + d / 2.0, y + small * 0.3 + d);
                let mut sorted: Vec<&(f32, String)> = circles.iter().collect();
                sorted.sort_by(|a, b| b.0.total_cmp(&a.0));
                let fill = look.fill.map(|c| [c[0], c[1], c[2], (c[3] as f32 * 0.35) as u8]);
                let edge = look.stroke.or(look.fill).unwrap_or(ink);
                let mut last_y = y - small * 0.2;
                for (dia, text) in sorted {
                    let r = (dia / 2.0).max(1.0);
                    let mut pb = PathBuilder::new();
                    pb.push_circle(cx, base - r, r);
                    if let Some(p) = pb.finish() {
                        if let Some(f) = fill {
                            pm.fill_path(&p, &paint(f), FillRule::Winding, Transform::identity(), None);
                        }
                        pm.stroke_path(&p, &paint(edge), &Stroke { width: 1.0 * k, ..Stroke::default() }, Transform::identity(), None);
                    }
                    // A thin line from the top of the circle to its value, which keeps clear of
                    // the one above.
                    let top = base - 2.0 * r;
                    let ty = top.max(last_y + small * 1.05);
                    last_y = ty;
                    let mut pb = PathBuilder::new();
                    pb.move_to(cx, top);
                    pb.line_to(x + d + 6.0 * k, ty);
                    if let Some(p) = pb.finish() {
                        pm.stroke_path(&p, &paint([0x66, 0x66, 0x66, 255]), &Stroke { width: 0.7 * k, ..Stroke::default() }, Transform::identity(), None);
                    }
                    write_text(pm, t, text, x + d + 8.0 * k, ty + small * 0.36, small, false, [0x0b, 0x0b, 0x0b], 0.0);
                }
            }
            Block::Ramp(r) => {
                let (bw, bh) = (self.size(k, t).0, 12.0 * k);
                let n = r.colours.len().max(1);
                for (i, c) in r.colours.iter().enumerate() {
                    let x0 = x + bw * i as f32 / n as f32;
                    if let Some(rect) = tiny_skia::Rect::from_xywh(x0, y, bw / n as f32 + 0.6, bh) {
                        pm.fill_rect(rect, &paint(*c), Transform::identity(), None);
                    }
                }
                let st = Stroke { width: 0.8 * k, ..Stroke::default() };
                for (i, (at, text)) in r.ticks.iter().enumerate() {
                    let tx = x + bw * at;
                    let mut pb = PathBuilder::new();
                    pb.move_to(tx, y + bh);
                    pb.line_to(tx, y + bh + 4.0 * k);
                    if let Some(p) = pb.finish() {
                        pm.stroke_path(&p, &paint([0x55, 0x55, 0x55, 255]), &st, Transform::identity(), None);
                    }
                    // The first from its left edge, the last to its right, the rest centred.
                    let align = if i == 0 && *at < 0.02 {
                        0.0
                    } else if i + 1 == r.ticks.len() && *at > 0.9 {
                        1.0
                    } else {
                        0.5
                    };
                    write_text(pm, t, text, tx, y + bh + 4.0 * k + small, small, false, [0x33, 0x33, 0x33], align);
                }
            }
            Block::Histogram(bars) => {
                let most = bars.iter().map(|b| b.1).max().unwrap_or(1).max(1) as f32;
                let (bw, hh) = (10.0 * k, 26.0 * k);
                let base = y + hh + 2.0 * k;
                for (n, (c, count)) in bars.iter().enumerate() {
                    let h = (*count as f32 / most * hh).max(if *count > 0 { 1.0 } else { 0.0 });
                    if let Some(r) = tiny_skia::Rect::from_xywh(x + n as f32 * 13.0 * k, base - h, bw, h) {
                        pm.fill_rect(r, &paint(*c), Transform::identity(), None);
                    }
                }
                let mut pb = PathBuilder::new();
                pb.move_to(x - 1.0, base);
                pb.line_to(x + bars.len() as f32 * 13.0 * k - 2.0 * k, base);
                if let Some(p) = pb.finish() {
                    pm.stroke_path(&p, &paint([0x88, 0x88, 0x88, 255]), &Stroke { width: 0.8 * k, ..Stroke::default() }, Transform::identity(), None);
                }
            }
        }
    }
}

fn draw_legend(pixmap: &mut Canvas, legend: &Legend) -> Result<()> {
    draw_legend_at(pixmap, legend, None, false).map(|_| ())
}

/// Draws the legend with its upper left corner at `at`, or in its place when that is None,
/// and gives its size. `dry` only measures it.
fn draw_legend_at(pixmap: &mut Canvas, legend: &Legend, at: Option<(f32, f32)>, dry: bool) -> Result<(f32, f32)> {
    let text = crate::chart::Text::new()?;
    let k = (pixmap.width().min(pixmap.height()) as f32 / 900.0).clamp(0.8, 3.0);
    let (size, small, pad, sym) = (14.0 * k, 12.5 * k, 12.0 * k, 16.0 * k);
    let line = size * 1.5;
    // The rows: (indent, text, bold, symbol)
    enum Sym<'a> {
        None,
        Look(crate::thematic::Look, &'static str, f32, Option<&'a LayerStyle>),
        Icon(&'static str, crate::thematic::Look, String),
        Block(Block<'a>),
    }
    let mut rows: Vec<(f32, String, bool, Sym, f32)> = Vec::new();
    // Circles as large as on the map, as the other symbols of points are.
    let k_scale = |_: &crate::thematic::Key| k.min(1.5);
    if !legend.title.is_empty() {
        rows.push((0.0, legend.title.clone(), true, Sym::None, size * 1.1));
    }
    for item in &legend.items {
        let look = crate::thematic::Look { fill: item.style.fill, stroke: item.style.stroke, size: item.style.size };
        let style = legend.icon_style(item);
        if !item.icon_keys.is_empty() {
            rows.push((0.0, item.name.clone(), true, Sym::None, size));
            for (what, icon) in &item.icon_keys {
                rows.push((0.0, what.clone(), false, Sym::Icon(icon, look, style.clone()), small));
            }
        } else if let (Some(icon), true) = (item.icon, item.keys.is_empty()) {
            rows.push((0.0, item.name.clone(), false, Sym::Icon(icon, look, style.clone()), size));
        } else if let Some(bv) = &item.bivariate {
            rows.push((0.0, item.name.clone(), true, Sym::None, size));
            rows.push((0.0, String::new(), false, Sym::Block(Block::Bivariate(bv)), small));
        } else if item.nested && item.size_keys.is_empty() && !item.keys.is_empty() {
            let head = if item.heading.is_empty() { item.name.clone() } else { format!("{} · {}", item.name, item.heading) };
            rows.push((0.0, head, true, Sym::None, size));
            let circles = item.keys.iter().map(|k| (k.look.size * k_scale(k), k.text.clone())).collect();
            rows.push((0.0, String::new(), false, Sym::Block(Block::Nested(circles, item.keys[0].look)), small));
        } else if let Some(r) = &item.ramp {
            rows.push((0.0, item.name.clone(), true, Sym::None, size));
            rows.push((0.0, String::new(), false, Sym::Block(Block::Ramp(r)), small));
        } else if item.keys.is_empty() {
            rows.push((0.0, item.name.clone(), false, Sym::Look(look, item.shape, item.style.width, Some(&item.style)), size));
        } else {
            let head = if item.heading.is_empty() { item.name.clone() } else { format!("{} · {}", item.name, item.heading) };
            rows.push((0.0, head, true, Sym::None, size));
            for key in &item.keys {
                rows.push((0.0, key.text.clone(), false, Sym::Look(key.look, item.shape, item.style.width, Some(&item.style)), small));
            }
            if item.counts.len() == item.keys.len() && item.counts.iter().any(|c| *c > 0) {
                let bars = item.keys.iter().zip(&item.counts).map(|(k, c)| (k.look.fill.or(k.look.stroke).unwrap_or([0x88, 0x88, 0x88, 255]), *c)).collect();
                rows.push((0.0, String::new(), false, Sym::Block(Block::Histogram(bars)), small));
            }
        }
        if let Some((l, text)) = &item.dots {
            rows.push((0.0, text.clone(), false, Sym::Look(*l, "point", 0.0, None), small));
        }
        if item.nested && !item.size_keys.is_empty() {
            rows.push((0.0, item.size_heading.clone(), true, Sym::None, small));
            let circles = item.size_keys.iter().map(|k| (k.look.size * k_scale(k), k.text.clone())).collect();
            rows.push((0.0, String::new(), false, Sym::Block(Block::Nested(circles, item.size_keys[0].look)), small));
        } else if !item.size_keys.is_empty() {
            rows.push((0.0, item.size_heading.clone(), true, Sym::None, small));
            for key in &item.size_keys {
                rows.push((0.0, key.text.clone(), false, Sym::Look(key.look, item.shape, item.style.width, Some(&item.style)), small));
            }
        }
        for (what, c) in &item.chart_keys {
            let look = crate::thematic::Look { fill: Some(*c), stroke: None, size: 0.0 };
            rows.push((0.0, what.clone(), false, Sym::Look(look, "area", 0.0, None), small));
        }
    }
    if rows.is_empty() {
        return Ok((0.0, 0.0));
    }
    let icon_px = (18.0 * k).round();
    let sym_w = rows
        .iter()
        .map(|r| match &r.3 {
            Sym::Look(l, "point", _, _) => (l.size * k.min(1.5)).max(sym),
            Sym::Look(l, "arrow", _, _) => (l.size * k.min(1.5)).clamp(sym, sym * 4.0),
            Sym::Look(_, "heat", _, _) => sym * 3.0,
            Sym::Icon(..) => icon_px,
            _ => sym,
        })
        .fold(sym, f32::max);
    let heights: Vec<f32> = rows
        .iter()
        .map(|r| match &r.3 {
            Sym::Look(l, "point", _, _) => line.max(l.size * k.min(1.5) + 4.0),
            Sym::Look(l, "flow", _, _) => line.max(l.size * k.min(1.5) + 6.0),
            Sym::Icon(..) => line.max(icon_px + 4.0),
            Sym::Block(b) => b.size(k, &text).1,
            _ => line,
        })
        .collect();
    let widths: Vec<f32> = rows
        .iter()
        .map(|r| match &r.3 {
            Sym::Block(b) => b.size(k, &text).0,
            Sym::None => text.width(&r.1, r.4, r.2) + r.0,
            _ => text.width(&r.1, r.4, r.2) + sym_w + 8.0 * k + r.0,
        })
        .collect();
    // In columns, each as high as the others as near as can be, and begun at a heading when
    // one is near.
    let cols = legend.columns.clamp(1, 6);
    let total: f32 = heights.iter().sum();
    let mut col_of = vec![0usize; rows.len()];
    let (mut c, mut acc) = (0usize, 0.0f32);
    for (i, (row, rh)) in rows.iter().zip(&heights).enumerate() {
        let target = total / cols as f32;
        if c + 1 < cols && acc > 0.0 && ((row.2 && acc >= target * 0.75) || acc + rh > target * 1.25) {
            c += 1;
            acc = 0.0;
        }
        col_of[i] = c;
        acc += rh;
    }
    let used = col_of.last().map_or(1, |c| c + 1);
    let col_w: Vec<f32> = (0..used).map(|c| widths.iter().zip(&col_of).filter(|(_, k)| **k == c).map(|(w, _)| *w).fold(0.0f32, f32::max)).collect();
    let col_h: Vec<f32> = (0..used).map(|c| heights.iter().zip(&col_of).filter(|(_, k)| **k == c).map(|(h, _)| *h).sum()).collect();
    let col_gap = 18.0 * k;
    let w = col_w.iter().sum::<f32>() + col_gap * (used - 1) as f32 + 2.0 * pad;
    let h = col_h.iter().fold(0.0f32, |a, b| a.max(*b)) + 2.0 * pad - (line - size) * 0.6;
    if dry {
        return Ok((w, h));
    }
    let (x0, y0) = at.unwrap_or_else(|| Slots::new(pixmap.width() as f32, pixmap.height() as f32).place(&legend.corner, w, h));
    if let Some(r) = tiny_skia::Rect::from_xywh(x0, y0, w, h) {
        let bg = PathBuilder::from_rect(r);
        if let Some(c) = legend.fill.unwrap_or(Some([255, 255, 255, 230])) {
            pixmap.fill_path(&bg, &paint(c), FillRule::Winding, Transform::identity(), None);
        }
        if let Some(c) = legend.line.unwrap_or(Some([0xc3, 0xc2, 0xb7, 255])) {
            pixmap.stroke_path(&bg, &paint(c), &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
        }
    }
    let mut y = y0 + pad;
    let mut col_x = x0 + pad;
    let mut col = 0usize;
    for ((row, rh), &c) in rows.iter().zip(&heights).zip(&col_of) {
        if c != col {
            col_x += col_w[col] + col_gap;
            col = c;
            y = y0 + pad;
        }
        let mid = y + rh / 2.0 - (line - size) * 0.3;
        let mut x = col_x + row.0;
        if let Sym::Block(bk) = &row.3 {
            bk.draw(pixmap, x, y, k, &text);
            y += rh;
            continue;
        }
        if let Sym::Icon(icon, l, style) = &row.3 {
            draw_icon(pixmap, icon, x + sym_w / 2.0, mid, icon_px, style, l.fill, l.stroke);
            x += sym_w + 8.0 * k;
        }
        if let Sym::Look(l, shape, width, style) = &row.3 {
            let cx = x + sym_w / 2.0;
            let st = Stroke { width: width.max(0.0).min(4.0 * k), line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
            match *shape {
                "point" => {
                    let mut pb = PathBuilder::new();
                    pb.push_circle(cx, mid, (l.size * k.min(1.5) / 2.0).max(1.5));
                    if let Some(p) = pb.finish() {
                        if let Some(c) = l.fill {
                            pixmap.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
                        }
                        if let (Some(c), true) = (l.stroke, *width > 0.0) {
                            pixmap.stroke_path(&p, &paint(c), &st, Transform::identity(), None);
                        }
                    }
                }
                "arrow" => {
                    // Pointing right, as long as on the map, but not longer than the column.
                    let len = (l.size * k.min(1.5)).min(sym_w);
                    arrow(pixmap, cx, mid, std::f32::consts::FRAC_PI_2, len, l, *width);
                }
                "heat" => {
                    let ramp = legend.items.iter().find_map(|i| i.heat_ramp.clone()).unwrap_or_else(|| "heat".into());
                    let steps = 24;
                    for s in 0..steps {
                        let t = s as f64 / (steps - 1) as f64;
                        let c = crate::thematic::ramp_at(&ramp, t);
                        let bw = sym_w / steps as f32;
                        if let Some(r) = tiny_skia::Rect::from_xywh(x + s as f32 * bw, mid - sym * 0.35, bw + 0.5, sym * 0.7) {
                            let a = ((t as f32).sqrt() * 0.85 + 0.05).min(0.9);
                            pixmap.fill_rect(r, &paint([c[0], c[1], c[2], (a * 255.0) as u8]), Transform::identity(), None);
                        }
                    }
                }
                "line" | "flow" => {
                    // With its edge and its dashes, as on the map; a flow as wide as its value.
                    let w = if *shape == "flow" { (l.size * k.min(1.5)).clamp(0.5, sym * 1.6) } else { width.max(1.5).min(4.0 * k) };
                    let mut heads = Vec::new();
                    let mut ends = vec![(x, mid), (x + sym_w, mid)];
                    if let Some(s) = style.filter(|s| !s.arrows.is_empty()) {
                        let len = head_length(s, w).min(sym_w * 0.45);
                        if matches!(s.arrows.as_str(), "end" | "both") {
                            heads.extend(cut_back(&mut ends, len, 0.3));
                        }
                        if matches!(s.arrows.as_str(), "start" | "both") {
                            ends.reverse();
                            heads.extend(cut_back(&mut ends, len, 0.3));
                        }
                    }
                    let mut pb = PathBuilder::new();
                    pb.move_to(ends[0].0, ends[0].1);
                    pb.line_to(ends[ends.len() - 1].0, ends[ends.len() - 1].1);
                    if let Some(c) = l.stroke.or(l.fill).filter(|_| !heads.is_empty()) {
                        draw_heads(pixmap, &heads, w, c);
                    }
                    if let Some(p) = pb.finish() {
                        if let Some((c, e)) = style.and_then(|s| s.casing.map(|c| (c, s.casing_width))) {
                            pixmap.stroke_path(&p, &paint(c), &Stroke { width: w + 2.0 * e, ..st.clone() }, Transform::identity(), None);
                        }
                        if let Some(c) = l.stroke {
                            let dashed = style.map(|s| LayerStyle { width: w, ..s.clone() }.stroke()).unwrap_or(Stroke { width: w, ..st.clone() });
                            pixmap.stroke_path(&p, &paint(c), &dashed, Transform::identity(), None);
                            if let Some((o, oc)) = style.and_then(|s| LayerStyle { width: w, ..s.clone() }.overlay(c)) {
                                pixmap.stroke_path(&p, &paint(oc), &o, Transform::identity(), None);
                            }
                        }
                    }
                }
                _ => {
                    if let Some(r) = tiny_skia::Rect::from_xywh(x, mid - sym * 0.4, sym_w, sym * 0.8) {
                        let p = PathBuilder::from_rect(r);
                        if let Some(c) = l.fill {
                            pixmap.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
                        }
                        // The pattern, at the spacing it has on the map.
                        if let Some(s) = style.filter(|s| !s.pattern.is_empty()) {
                            let swatch = Geometry::Rect(geo::Rect::new((r.left() as f64, r.top() as f64), (r.right() as f64, r.bottom() as f64)));
                            draw_pattern(pixmap, &swatch, s, &|x, y| (x as f32, y as f32));
                        }
                        match (l.stroke, *width > 0.0, style.filter(|s| extra::is_border(s))) {
                            // A border is shown as its line, through the middle of the swatch.
                            (Some(c), true, Some(s)) => {
                                let mut pb = PathBuilder::new();
                                pb.move_to(x - 2.0 * k, mid);
                                pb.line_to(x + sym_w + 2.0 * k, mid);
                                if let Some(line) = pb.finish() {
                                    let dashed = LayerStyle { width: width.min(2.5 * k), ..s.clone() }.stroke();
                                    pixmap.stroke_path(&line, &paint(c), &dashed, Transform::identity(), None);
                                }
                            }
                            (Some(c), true, None) => {
                                pixmap.stroke_path(&p, &paint(c), &Stroke { width: width.min(2.0), ..st }, Transform::identity(), None);
                            }
                            _ => {}
                        }
                    }
                }
            }
            x += sym_w + 8.0 * k;
        }
        write_text(pixmap, &text, &row.1, x, mid + row.4 * 0.36, row.4, row.2, [0x0b, 0x0b, 0x0b], 0.0);
        y += rh;
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, FieldType, Value};
    use geo::{line_string, point, polygon};

    fn table(geoms: Vec<Geometry<f64>>) -> Table {
        Table {
            mesh: None,
            points: None,
            series: None,
            fields: vec![Field::new("id", FieldType::Int)],
            crs: Some(3006),
            features: geoms.into_iter().enumerate().map(|(i, g)| Feature { z: None, attrs: vec![Value::Int(i as i64)], geom: Some(g) }).collect(),
            raster: false,
            image: None,
            grid: None,
        }
    }

    #[test]
    fn index_squares_are_named_by_letter_and_row() {
        let b = [0.0, 0.0, 600.0, 400.0];
        assert_eq!(index_square(b, 6, 50.0, 390.0).as_deref(), Some("A1"));
        assert_eq!(index_square(b, 6, 250.0, 150.0).as_deref(), Some("C3"));
        assert_eq!(index_square(b, 6, 600.0, 0.0).as_deref(), Some("F4"));
        assert_eq!(index_square(b, 6, 700.0, 10.0), None);
        assert_eq!(index_column(27), "AB");
    }

    #[test]
    fn index_contours_are_found_by_their_height() {
        assert!(is_index(&Value::Real(150.0), 50.0));
        assert!(is_index(&Value::Int(100), 50.0));
        assert!(!is_index(&Value::Real(160.0), 50.0));
        assert!(!is_index(&Value::Real(100.0), 0.0));
    }

    #[test]
    fn a_long_area_has_a_line_through_its_middle() {
        let long: Geometry<f64> = polygon![(x: 0.0, y: 0.0), (x: 1000.0, y: 0.0), (x: 1000.0, y: 100.0), (x: 0.0, y: 100.0), (x: 0.0, y: 0.0)].into();
        let line = centre_line(&long).expect("a long area has a middle line");
        assert!(line.0.iter().all(|c| c.y > 20.0 && c.y < 80.0));
        let ends = (line.0.first().unwrap().x, line.0.last().unwrap().x);
        assert!((ends.0 - ends.1).abs() > 500.0);
        let square: Geometry<f64> = polygon![(x: 0.0, y: 0.0), (x: 100.0, y: 0.0), (x: 100.0, y: 100.0), (x: 0.0, y: 100.0), (x: 0.0, y: 0.0)].into();
        assert!(centre_line(&square).is_none());
    }

    #[test]
    fn the_scale_is_given_in_round_figures() {
        let f = Furniture { dpi: 96.0, ..Default::default() };
        let text = scale_figures(&f, [0.0, 0.0, 1000.0, 1000.0], 10.0);
        assert!(text.starts_with("1:"), "{text}");
        let n: f64 = text[2..].replace(' ', "").parse().unwrap();
        assert!(n > 1000.0 && n % 100.0 == 0.0, "{text}");
    }

    #[test]
    fn colours_are_read_and_written_as_text() {
        assert_eq!(parse_color("#1f78b4"), Ok(Some([0x1f, 0x78, 0xb4, 255])));
        assert_eq!(parse_color("1F78B480"), Ok(Some([0x1f, 0x78, 0xb4, 0x80])));
        assert_eq!(parse_color(" none "), Ok(None));
        assert_eq!(parse_color(""), Ok(None));
        assert!(parse_color("#12").is_err() && parse_color("blue").is_err());
        assert_eq!(color_text(Some([0x1f, 0x78, 0xb4, 255])), "#1f78b4");
        assert_eq!(color_text(Some([0, 0, 0, 0x80])), "#00000080");
        assert_eq!(color_text(None), "none");
    }

    #[test]
    fn layers_are_drawn_in_order_with_their_colours() {
        // An area of 1 km × 1 km, a line across it and a point in its middle.
        let area = table(vec![polygon![(x: 0.0, y: 0.0), (x: 1000.0, y: 0.0), (x: 1000.0, y: 1000.0), (x: 0.0, y: 1000.0), (x: 0.0, y: 0.0)].into()]);
        let line = table(vec![line_string![(x: 0.0, y: 250.0), (x: 1000.0, y: 250.0)].into()]);
        let dot = table(vec![point!(x: 500.0, y: 500.0).into()]);
        let blue = LayerStyle { fill: Some([166, 206, 227, 255]), stroke: Some([31, 120, 180, 255]), width: 2.0, size: 6.0, ..LayerStyle::default() };
        let red = LayerStyle { fill: None, stroke: Some([255, 0, 0, 255]), width: 4.0, size: 1.0, ..LayerStyle::default() };
        let black = LayerStyle { fill: Some([0, 0, 0, 255]), stroke: None, width: 0.0, size: 10.0, ..LayerStyle::default() };
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&area, blue), (&line, red), (&dot, black)],
            width: 100,
            margin: 0.0,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![],
            icons: vec![],
            charts: vec![],
            legend: None,
        };
        let img = draw(&req, &|| true).unwrap();
        assert_eq!((img.width, img.height), (100, 100));
        assert_eq!(img.geotransform, [0.0, 10.0, 0.0, 1000.0, 0.0, -10.0]);
        let px = |x: usize, y: usize| -> [u8; 4] { img.rgba[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4].try_into().unwrap() };
        assert_eq!(px(25, 25), [166, 206, 227, 255], "the area is filled");
        assert_eq!(px(50, 50), [0, 0, 0, 255], "the point is on top");
        assert_eq!(px(25, 75), [255, 0, 0, 255], "the line is on the area");
        assert!(px(0, 50)[2] < 220, "the outline of the area is darker: {:?}", px(0, 50));
        // With a margin the picture holds more around the features, and the background shows.
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&area, LayerStyle::default_for(0))],
            width: 120,
            margin: 0.1,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![],
            icons: vec![],
            charts: vec![],
            legend: None,
        };
        let img = draw(&req, &|| true).unwrap();
        assert_eq!(img.height, 120);
        assert_eq!(&img.rgba[0..4], &[255, 255, 255, 255]);
        // Without a background the corners are see-through.
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&area, LayerStyle::default_for(0))],
            width: 120,
            margin: 0.1,
            background: None,
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![],
            icons: vec![],
            charts: vec![],
            legend: None,
        };
        assert_eq!(draw(&req, &|| true).unwrap().rgba[3], 0);
        // A stop ends the drawing.
        assert_eq!(draw(&req, &|| false).unwrap_err().0, crate::model::STOPPED);
        // Nothing to draw, and a picture far too high.
        let empty = Table::default();
        assert!(draw(
            &Request {
                rasters: Vec::new(),
                layers: vec![(&empty, LayerStyle::default_for(0))],
                width: 10,
                margin: 0.0,
                background: None,
                backdrop: None,
                attribution: None,
                furniture: Default::default(),
                names: Vec::new(),
                overview: None,
                map_style: String::new(),
                themes: vec![],
                labels: vec![],
                icons: vec![],
                charts: vec![],
                legend: None
            },
            &|| true
        )
        .is_err());
        let tall = table(vec![line_string![(x: 0.0, y: 0.0), (x: 1.0, y: 100_000.0)].into()]);
        let e = draw(
            &Request {
                rasters: Vec::new(),
                layers: vec![(&tall, LayerStyle::default_for(0))],
                width: 2000,
                margin: 0.0,
                background: None,
                backdrop: None,
                attribution: None,
                furniture: Default::default(),
                names: Vec::new(),
                overview: None,
                map_style: String::new(),
                themes: vec![],
                labels: vec![],
                icons: vec![],
                charts: vec![],
                legend: None,
            },
            &|| true,
        )
        .unwrap_err()
        .0;
        assert!(e.contains("pixels high"), "{}", e);
    }

    fn labelled(n: usize) -> Labels {
        Labels {
            spec: LabelSpec { size: 16.0, halo: None, ..LabelSpec::default() },
            items: (0..n).map(|i| LabelItem { feature: i, text: "Lidköping".into(), size: 16.0, priority: -(i as f64), at: None, bold: false }).collect(),
        }
    }

    #[test]
    fn a_band_along_the_bottom_a_described_label_and_a_ramp_in_the_legend() {
        let t = table(vec![Geometry::Point(geo::Point::new(0.0, 0.0)), Geometry::Point(geo::Point::new(100.0, 60.0))]);
        let style = LayerStyle { size: 6.0, ..LayerStyle::default() };
        // The band: across the bottom, in its own colour.
        let banded = drawn(&t, style.clone(), None, Furniture { footer: "Data: made up".into(), ..Furniture::default() });
        let bottom = (banded.height - 3) * banded.width + banded.width / 2;
        let px = &banded.rgba[bottom * 4..bottom * 4 + 3];
        assert!(px.iter().zip([0xf6u8, 0xf2, 0xe9]).all(|(a, b)| a.abs_diff(b) <= 2), "{:?}", px);
        // A second line smaller and not bold, when the layer asks for it.
        let spec = LabelSpec { detail: true, ..LabelSpec::default() };
        let item = LabelItem { feature: 0, text: "Name\nwhat it is".into(), size: 20.0, priority: 0.0, at: None, bold: true };
        assert_eq!(line_look(&spec, &item, 0), (20.0, true));
        let (size, bold) = line_look(&spec, &item, 1);
        assert!(size < 16.0 && !bold);
        // The depths of the sea as a band of colours from 0 down, in metres.
        let ramp = crate::rasterlook::ramp_legend(&crate::rasterlook::Paint::Seafloor { deep: -5170.0, land: Some([0xee, 0xe7, 0xd7]), sea: None }).unwrap();
        assert_eq!(ramp.ticks.first().map(|t| t.1.as_str()), Some("0"));
        assert!(ramp.ticks.last().unwrap().1.ends_with(" m"));
        assert!(ramp.colours.first().unwrap()[2] > ramp.colours.last().unwrap()[2], "pale at the shore, dark in the deep");
    }

    /// A request of one layer, its labels and its furniture, drawn 400 pixels wide.
    fn drawn(t: &Table, style: LayerStyle, labels: Option<Labels>, furniture: Furniture) -> RasterImage {
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(t, style)],
            width: 400,
            margin: 0.1,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture,
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![labels],
            icons: vec![],
            charts: vec![],
            legend: None,
        };
        let img = draw(&req, &|| true).unwrap();
        if let Ok(dir) = std::env::var("OG_SHOTS") {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = std::fs::write(format!("{}/map{}.png", dir, n), crate::session::png(&img).unwrap());
        }
        img
    }

    fn label(feature: usize, text: &str) -> LabelItem {
        LabelItem { feature, text: text.into(), size: 18.0, priority: 0.0, at: None, bold: false }
    }

    #[test]
    fn lines_break_under_labels_and_the_picture_knows_where() {
        // A thick black line across the middle, and a label on a point on it.
        let t = table(vec![line_string![(x: 0.0, y: 500.0), (x: 1000.0, y: 500.0)].into(), point!(x: 500.0, y: 500.0).into()]);
        let style = LayerStyle { fill: None, stroke: Some([0, 0, 0, 255]), width: 6.0, size: 0.0, ..LayerStyle::default() };
        let labels = || Labels {
            spec: LabelSpec { size: 18.0, halo: None, place: "center".into(), colour: [255, 0, 0, 255], ..LabelSpec::default() },
            items: vec![label(1, "MMMM")],
        };
        let black_near_middle = |img: &RasterImage| {
            let y = img.height / 2;
            (img.width / 2 - 20..img.width / 2 + 20).filter(|x| (y - 4..y + 4).any(|yy| img.rgba[(yy * img.width + x) * 4] < 40)).count()
        };
        let whole = drawn(&t, style.clone(), Some(labels()), Furniture::default());
        let broken = drawn(&t, style, Some(labels()), Furniture { knockout: true, ..Furniture::default() });
        assert!(black_near_middle(&whole) > 15, "{}", black_near_middle(&whole));
        assert!(black_near_middle(&broken) < 5, "{}", black_near_middle(&broken));
        let pic = broken.vector.as_ref().unwrap();
        assert!(pic.ops.iter().any(|o| matches!(o, crate::vector::Op::Stroke { holes, .. } if !holes.is_empty())));
        assert!(crate::vector::svg(pic).contains("clip-rule=\"evenodd\""));
    }

    #[test]
    fn a_line_can_end_in_an_arrow_head() {
        // A line from left to right: the head is at the right end, wider than the line.
        let t = table(vec![line_string![(x: 0.0, y: 500.0), (x: 1000.0, y: 500.0)].into()]);
        let style = LayerStyle { fill: None, stroke: Some([0, 0, 0, 255]), width: 4.0, size: 0.0, ..LayerStyle::default() };
        let tall = |img: &RasterImage, x: usize| (0..img.height).filter(|y| img.rgba[(y * img.width + x) * 4] < 100).count();
        let plain = drawn(&t, style.clone(), None, Furniture::default());
        let arrowed = drawn(&t, LayerStyle { arrows: "end".into(), ..style.clone() }, None, Furniture::default());
        let (left, right) = (plain.width / 8, plain.width * 7 / 8);
        let near_end = (right..plain.width).map(|x| tall(&arrowed, x)).max().unwrap_or(0);
        assert!(near_end > tall(&plain, right) + 4, "{} {}", near_end, tall(&plain, right));
        assert_eq!(tall(&arrowed, left), tall(&plain, left), "no head at the start");
        let both = drawn(&t, LayerStyle { arrows: "both".into(), ..style }, None, Furniture::default());
        assert!((0..left).map(|x| tall(&both, x)).max().unwrap_or(0) > tall(&plain, left) + 4);
    }

    #[test]
    fn rails_trams_scales_and_a_paler_rest() {
        let t = table(vec![line_string![(x: 0.0, y: 500.0), (x: 1000.0, y: 500.0)].into()]);
        let style = LayerStyle { fill: None, stroke: Some([0, 0, 0, 255]), width: 4.0, size: 0.0, ..LayerStyle::default() };
        let white_on_line = |img: &RasterImage| {
            let y = img.height / 2;
            (0..img.width).filter(|x| img.rgba[(y * img.width + x) * 4] > 200).count()
        };
        let plain = drawn(&t, style.clone(), None, Furniture::default());
        let rail = drawn(&t, LayerStyle { dash: "rail".into(), ..style.clone() }, None, Furniture::default());
        assert!(white_on_line(&rail) > white_on_line(&plain) + 100, "white dashes along the middle");
        let tall = |img: &RasterImage| (0..img.height).filter(|y| img.rgba[(y * img.width + img.width / 2) * 4] < 100).count();
        let tram = drawn(&t, LayerStyle { dash: "tram".into(), ..style.clone() }, None, Furniture::default());
        let ticks = (0..tram.width).map(|x| (0..tram.height).filter(|y| tram.rgba[(y * tram.width + x) * 4] < 100).count()).max().unwrap_or(0);
        assert!(ticks > tall(&plain) + 6, "ticks across the line");
        // Drawn only from 1:5 000 and closer: this map is far smaller.
        let hidden = drawn(&t, LayerStyle { scale_max: 5_000.0, ..style.clone() }, None, Furniture::default());
        assert_eq!(tall(&hidden), 0);
        assert!(LayerStyle { scale_min: 1000.0, scale_max: 50_000.0, ..LayerStyle::default() }.shown_at(20_000.0));
        assert!(!LayerStyle { scale_min: 1000.0, ..LayerStyle::default() }.shown_at(500.0));
        // Outside the area the map is about, paler.
        let area = polygon![(x: 0.0, y: 0.0), (x: 400.0, y: 0.0), (x: 400.0, y: 1000.0), (x: 0.0, y: 1000.0)];
        let faded = drawn(&t, style, None, Furniture { focus: vec![area.into()], focus_fade: 0.7, ..Furniture::default() });
        let y = faded.height / 2;
        assert!(faded.rgba[(y * faded.width + faded.width / 8) * 4] < 60, "dark inside");
        assert!(faded.rgba[(y * faded.width + faded.width * 7 / 8) * 4] > 120, "pale outside");
    }

    #[test]
    fn small_multiples_and_pages_of_a_pdf() {
        let t = Table {
            fields: vec![crate::model::Field::new("year", crate::model::FieldType::Text)],
            features: (0..4)
                .map(|i| crate::model::Feature {
                    z: None,
                    attrs: vec![Value::Text(if i < 2 { "2020".into() } else { "2024".into() })],
                    geom: Some(point!(x: i as f64 * 100.0, y: i as f64 * 50.0).into()),
                })
                .collect(),
            ..Default::default()
        };
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&t, LayerStyle { size: 10.0, ..LayerStyle::default() })],
            width: 600,
            margin: 0.1,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture: Furniture { title: "Over time".into(), ..Furniture::default() },
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![],
            icons: vec![],
            charts: vec![],
            legend: Some(Legend {
                corner: "top-right".into(),
                items: vec![LegendItem { name: "Places".into(), shape: "point", ..LegendItem::default() }],
                ..Legend::default()
            }),
        };
        let m =
            Multiples { layer: 0, values: vec![("2020".into(), vec![true, true, false, false]), ("2024".into(), vec![false, false, true, true])], columns: 0 };
        let img = draw_multiples(&req, &m, &|| true).unwrap();
        assert_eq!(img.width, 600);
        // Two maps and the legend: two across, two down.
        let pic = img.vector.unwrap();
        assert!(pic.layers.iter().any(|l| l == "2020") && pic.layers.iter().any(|l| l == "2024") && pic.layers.iter().any(|l| l == "Legend"));
        let one = crate::vector::PdfPage {
            pic: &pic,
            page_w: 300.0,
            page_h: 300.0,
            left: 0.0,
            top: 0.0,
            scale: 0.5,
            heading: None,
            geo: None,
            bleed: 8.5,
            marks: true,
            cmyk: true,
            overprint: false,
            icc: None,
            window: None,
            folds: (1, 1),
        };
        let two = crate::vector::PdfPage {
            pic: &pic,
            page_w: 300.0,
            page_h: 300.0,
            left: 0.0,
            top: 0.0,
            scale: 0.5,
            heading: None,
            geo: None,
            bleed: 8.5,
            marks: true,
            cmyk: true,
            overprint: true,
            icc: None,
            window: None,
            folds: (1, 1),
        };
        let book = String::from_utf8_lossy(&crate::vector::pdf_book(&[one, two])).into_owned();
        assert!(book.contains("/Count 2"));
        assert_eq!(book.matches("/Type /Page ").count(), 2);
    }

    #[test]
    fn the_title_can_have_a_box_a_colour_and_a_size() {
        let t = table(vec![line_string![(x: 0.0, y: 500.0), (x: 1000.0, y: 500.0)].into()]);
        let style = LayerStyle { fill: None, stroke: Some([0, 0, 0, 255]), width: 2.0, size: 0.0, ..LayerStyle::default() };
        let f = Furniture {
            title: "Title".into(),
            title_place: "top-center".into(),
            title_fill: Some([0, 0, 255, 255]),
            title_colour: Some([255, 0, 0, 255]),
            title_size: 40.0,
            ..Furniture::default()
        };
        let img = drawn(&t, style, None, f);
        let count = |c: [u8; 3]| img.rgba.chunks_exact(4).filter(|p| p[0] == c[0] && p[1] == c[1] && p[2] == c[2]).count();
        assert!(count([0, 0, 255]) > 800, "a blue box");
        assert!(count([255, 0, 0]) > 100, "red letters");
        // In the middle of the top.
        let first_blue = img.rgba.chunks_exact(4).position(|p| p[..3] == [0, 0, 255]).unwrap();
        let x = first_blue % img.width;
        assert!(x > img.width / 5 && x < img.width / 2, "{} of {}", x, img.width);
    }

    #[test]
    fn a_frame_a_clip_a_blend_and_notes() {
        let area = polygon![(x: 0.0, y: 0.0), (x: 1000.0, y: 0.0), (x: 1000.0, y: 1000.0), (x: 0.0, y: 1000.0)];
        let t = table(vec![area.into()]);
        let style = LayerStyle { fill: Some([200, 100, 0, 255]), stroke: None, blend: "multiply".into(), glow: "shadow".into(), ..LayerStyle::default() };
        let half = polygon![(x: 0.0, y: 0.0), (x: 500.0, y: 0.0), (x: 500.0, y: 1000.0), (x: 0.0, y: 1000.0)];
        let f = Furniture {
            frame: true,
            grid: "lines".into(),
            focus: vec![half.into()],
            focus_only: true,
            notes: vec![(250.0, 500.0, "Here".into())],
            ..Furniture::default()
        };
        let img = drawn(&t, style, None, f);
        // A white margin all round, the frame inside it.
        assert_eq!(&img.rgba[..4], &[255, 255, 255, 255]);
        let pic = img.vector.as_ref().unwrap();
        assert!(pic.ops.iter().any(|o| matches!(o, crate::vector::Op::Clip { path, .. } if !path.is_empty())));
        assert!(pic.ops.iter().any(|o| matches!(o, crate::vector::Op::Blend(b) if b == "multiply")));
        assert!(pic.layers.iter().any(|l| l == "Notes"));
        // Only the left half is drawn: the right half of the map is empty.
        let y = img.height / 2;
        let at = |x: usize| &img.rgba[(y * img.width + x) * 4..(y * img.width + x) * 4 + 3];
        assert!(at(img.width / 4)[2] < 100, "orange on the left");
        assert!(at(img.width * 3 / 4)[2] > 200, "nothing on the right");
        let pdf = String::from_utf8_lossy(&crate::vector::pdf(pic, 400.0, 400.0, 0.0, 0.0, 0.5, None, None)).into_owned();
        assert!(pdf.contains("/BMMultiply << /Type /ExtGState /BM /Multiply >>"), "the blend among the states of the PDF");
        let svg = crate::vector::svg(pic);
        assert!(svg.contains("mix-blend-mode:multiply") && svg.contains("<clipPath id=\"area"));
    }

    #[test]
    fn things_in_one_corner_stack_and_the_middle_is_a_place_too() {
        let mut s = Slots::new(1000.0, 800.0);
        let a = s.place("bottom-right", 100.0, 50.0);
        let b = s.place("bottom-right", 100.0, 50.0);
        assert!(b.1 + 50.0 <= a.1, "{:?} above {:?}", b, a);
        let c = s.place("top-center", 200.0, 40.0);
        assert_eq!(c, (400.0, 16.0));
        // Too tall to go above them: beside them.
        let d = s.place("bottom-right", 100.0, 700.0);
        assert!(d.0 + 100.0 <= a.0, "{:?} beside {:?}", d, a);
    }

    #[test]
    fn small_areas_get_their_label_beside_them_with_a_line() {
        let small = polygon![(x: 490.0, y: 490.0), (x: 510.0, y: 490.0), (x: 510.0, y: 510.0), (x: 490.0, y: 510.0)];
        let t = table(vec![small.into(), point!(x: 0.0, y: 0.0).into(), point!(x: 1000.0, y: 1000.0).into()]);
        let style = LayerStyle { fill: Some([200, 200, 200, 255]), stroke: None, width: 0.0, size: 1.0, ..LayerStyle::default() };
        let labels = |leaders: bool| Labels {
            spec: LabelSpec { size: 18.0, halo: None, min_px: 30.0, leaders, colour: [255, 0, 0, 255], ..LabelSpec::default() },
            items: vec![label(0, "Tjörn")],
        };
        let red = |img: &RasterImage| img.rgba.chunks(4).filter(|p| p[0] > 200 && p[1] < 60).count();
        assert_eq!(red(&drawn(&t, style.clone(), Some(labels(false)), Furniture::default())), 0, "too small, and no line");
        assert!(red(&drawn(&t, style, Some(labels(true)), Furniture::default())) > 40);
    }

    #[test]
    fn a_long_line_gets_its_label_again_and_again() {
        let t = table(vec![line_string![(x: 0.0, y: 500.0), (x: 250.0, y: 500.0), (x: 500.0, y: 500.0), (x: 750.0, y: 500.0), (x: 1000.0, y: 500.0)].into()]);
        let style = LayerStyle { fill: None, stroke: Some([200, 200, 200, 255]), width: 2.0, size: 0.0, ..LayerStyle::default() };
        let count = |repeat: f32| {
            let labels =
                Labels { spec: LabelSpec { size: 14.0, halo: None, repeat, colour: [255, 0, 0, 255], ..LabelSpec::default() }, items: vec![label(0, "E6")] };
            let img = drawn(&t, style.clone(), Some(labels), Furniture::default());
            // Runs of red columns along the line.
            let cols: Vec<bool> = (0..img.width)
                .map(|x| {
                    (0..img.height).any(|y| {
                        let p = &img.rgba[(y * img.width + x) * 4..];
                        p[0] > 200 && p[1] < 60
                    })
                })
                .collect();
            cols.windows(2).filter(|w| w[1] && !w[0]).count()
        };
        let once = count(0.0);
        let often = count(60.0);
        assert!(often >= once + 2, "{} against {}", often, once);
    }

    #[test]
    fn a_title_a_set_area_and_an_inset() {
        let t = table(vec![polygon![(x: 0.0, y: 0.0), (x: 1000.0, y: 0.0), (x: 1000.0, y: 1000.0), (x: 0.0, y: 1000.0)].into()]);
        let style = LayerStyle { fill: Some([0, 0, 255, 255]), stroke: None, width: 0.0, size: 1.0, ..LayerStyle::default() };
        // An area twice as wide as the features: half the picture is white.
        let wide = drawn(&t, style.clone(), None, Furniture { extent: Some([-1000.0, 0.0, 1000.0, 1000.0]), ..Furniture::default() });
        assert_eq!((wide.width, wide.height), (400, 200));
        assert_eq!(&wide.rgba[(100 * 400 + 50) * 4..][..3], &[255, 255, 255]);
        assert_eq!(&wide.rgba[(100 * 400 + 350) * 4..][..3], &[0, 0, 255]);
        let titled = drawn(&t, style.clone(), None, Furniture { title: "Västra Götaland".into(), ..Furniture::default() });
        let dark = titled.rgba.chunks(4).filter(|p| p[0] < 120 && p[2] < 120).count();
        assert!(dark > 30, "{}", dark);
        let inset = Inset { area: [400.0, 400.0, 600.0, 600.0], corner: "top-right".into(), share: 0.3 };
        let with = drawn(&t, style, None, Furniture { inset: Some(inset), ..Furniture::default() });
        // The frames of the inset and of its area on the map, dark grey.
        let frame = with.rgba.chunks(4).filter(|p| p[0] < 100 && p[1] < 100 && p[2] < 100).count();
        assert!(frame > 200, "{}", frame);
    }

    #[test]
    fn a_label_whose_place_already_says_the_same_is_not_moved_to_say_it_again() {
        // The equator and the prime meridian, both named 0, crossing in the middle.
        let lines = table(vec![
            Geometry::LineString(geo::LineString::from(vec![(-1000.0, 0.0), (1000.0, 0.0)])),
            Geometry::LineString(geo::LineString::from(vec![(0.0, -1000.0), (0.0, 1000.0)])),
        ]);
        let style = LayerStyle { fill: None, stroke: None, width: 0.0, size: 1.0, ..LayerStyle::default() };
        let item = |feature| LabelItem { feature, text: "0".into(), size: 16.0, priority: 0.0, at: None, bold: false };
        let labels = Labels { spec: LabelSpec { size: 16.0, halo: None, ..LabelSpec::default() }, items: vec![item(0), item(1)] };
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&lines, style)],
            width: 400,
            margin: 0.0,
            background: None,
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![Some(labels)],
            icons: vec![],
            charts: vec![],
            legend: None,
        };
        let img = draw(&req, &|| true).unwrap();
        let (cx, cy) = (img.width as f32 / 2.0, img.height as f32 / 2.0);
        let ink: Vec<(f32, f32)> = (0..img.height)
            .flat_map(|y| (0..img.width).map(move |x| (x, y)))
            .filter(|(x, y)| img.rgba[(y * img.width + x) * 4 + 3] > 0)
            .map(|(x, y)| (x as f32, y as f32))
            .collect();
        assert!(!ink.is_empty());
        // One 0 where they cross, and none further along either line.
        assert!(ink.iter().all(|(x, y)| (x - cx).hypot(y - cy) < 30.0), "{:?}", ink.iter().map(|(x, y)| (x - cx).hypot(y - cy) as i32).max());
    }

    #[test]
    fn a_label_can_have_two_lines_one_under_the_other() {
        let pts = table(vec![point!(x: 0.0, y: 0.0).into(), point!(x: 1000.0, y: 1000.0).into()]);
        let style = LayerStyle { fill: None, stroke: None, width: 0.0, size: 1.0, ..LayerStyle::default() };
        let rows_with_ink = |text: &str| {
            let labels = Labels {
                spec: LabelSpec { size: 20.0, halo: None, place: "center".into(), ..LabelSpec::default() },
                items: vec![LabelItem { feature: 0, text: text.into(), size: 20.0, priority: 0.0, at: None, bold: false }],
            };
            let req = Request {
                rasters: Vec::new(),
                layers: vec![(&pts, style.clone())],
                width: 400,
                margin: 0.3,
                background: None,
                backdrop: None,
                attribution: None,
                furniture: Default::default(),
                names: Vec::new(),
                overview: None,
                map_style: String::new(),
                themes: vec![],
                labels: vec![Some(labels)],
                icons: vec![],
                charts: vec![],
                legend: None,
            };
            let img = draw(&req, &|| true).unwrap();
            let rows: Vec<usize> = (0..img.height).filter(|y| (0..img.width).any(|x| img.rgba[(y * img.width + x) * 4 + 3] > 0)).collect();
            (rows.len(), rows.first().copied().unwrap_or(0), rows.last().copied().unwrap_or(0))
        };
        let (one, ..) = rows_with_ink("Skaraborg");
        let (two, top, bottom) = rows_with_ink("Skaraborg\n7 937 km²");
        assert!(two > one + 10, "two lines are higher than one: {} against {}", two, one);
        assert!(bottom - top > 32, "{}..{}", top, bottom);
    }

    #[test]
    fn labels_take_the_room_there_is_and_follow_lines() {
        // Three points close together: only one label fits beside them, unless all are asked for.
        let pts = table(vec![
            point!(x: 500.0, y: 500.0).into(),
            point!(x: 505.0, y: 500.0).into(),
            point!(x: 500.0, y: 505.0).into(),
            point!(x: 0.0, y: 0.0).into(),
            point!(x: 1000.0, y: 1000.0).into(),
        ]);
        let style = LayerStyle { fill: Some([0, 0, 0, 255]), stroke: None, width: 0.0, size: 6.0, ..LayerStyle::default() };
        let ink = |img: &RasterImage| img.rgba.chunks(4).filter(|p| p[3] > 0 && p[0] < 100).count();
        let run = |labels: Labels| {
            let req = Request {
                rasters: Vec::new(),
                layers: vec![(&pts, style.clone())],
                width: 400,
                margin: 0.1,
                background: None,
                backdrop: None,
                attribution: None,
                furniture: Default::default(),
                names: Vec::new(),
                overview: None,
                map_style: String::new(),
                themes: vec![],
                labels: vec![Some(labels)],
                icons: vec![],
                charts: vec![],
                legend: None,
            };
            draw(&req, &|| true).unwrap()
        };
        let none = ink(&run(Labels { spec: LabelSpec::default(), items: vec![] }));
        let mut single = labelled(5);
        single.items.truncate(1);
        let one = ink(&run(single)) - none;
        // Three labels of points close together: those that find a place of their own around
        // their point do not cover one another, so each adds the ink of a whole label.
        let mut three = labelled(5);
        three.items.truncate(3);
        let apart = ink(&run(three.clone())) - none;
        let k = apart as f64 / one as f64;
        assert!(k > 1.8 && (k - k.round()).abs() < 0.12, "{} {}", apart, one);
        // All of them where they would go first: on one another.
        three.spec.all = true;
        let piled = ink(&run(three)) - none;
        assert!(piled < apart * 9 / 10, "{} {}", piled, apart);
        // A label of a point in a corner goes where it stays on the map.
        let mut corner = labelled(5);
        corner.items = vec![LabelItem { feature: 4, text: "Hjo".into(), size: 16.0, priority: 0.0, at: None, bold: false }];
        assert!(ink(&run(corner)) > none + 20, "the label of the point in the upper right corner is drawn to its left");
        // Along a line, turned with it: a line that rises at 45°.
        let line = table(vec![line_string![(x: 0.0, y: 0.0), (x: 1000.0, y: 1000.0)].into()]);
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&line, LayerStyle { fill: None, stroke: Some([200, 200, 200, 255]), width: 1.0, size: 1.0, ..LayerStyle::default() })],
            width: 300,
            margin: 0.0,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![Some(Labels {
                spec: LabelSpec { size: 20.0, halo: None, ..LabelSpec::default() },
                items: vec![LabelItem { feature: 0, text: "MMMM".into(), size: 20.0, priority: 0.0, at: None, bold: false }],
            })],
            icons: vec![],
            charts: vec![],
            legend: None,
        };
        let img = draw(&req, &|| true).unwrap();
        let dark: Vec<(usize, usize)> = (0..300 * 300).filter(|k| img.rgba[k * 4] < 90).map(|k| (k % 300, k / 300)).collect();
        assert!(!dark.is_empty());
        // The letters lie along the diagonal from lower left to upper right: x + y is near 300.
        assert!(dark.iter().all(|(x, y)| (x + y).abs_diff(300) < 40), "{:?}", dark.iter().take(5).collect::<Vec<_>>());
        let spread_x = dark.iter().map(|p| p.0).max().unwrap() - dark.iter().map(|p| p.0).min().unwrap();
        assert!(spread_x > 40, "turned, the word spreads across: {}", spread_x);
    }

    #[test]
    fn points_are_drawn_as_icons_and_shown_in_the_legend() {
        let pts = table(vec![point!(x: 0.0, y: 0.0).into(), point!(x: 1000.0, y: 1000.0).into()]);
        let style = LayerStyle { fill: Some([200, 0, 0, 255]), stroke: Some([255, 255, 255, 255]), width: 1.0, size: 40.0, ..LayerStyle::default() };
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&pts, style.clone())],
            width: 200,
            margin: 0.2,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![],
            charts: vec![],
            icons: vec![Some(Icons { style: "circle".into(), names: vec![Some("hospital"), None] })],
            legend: None,
        };
        let img = draw(&req, &|| true).unwrap();
        let px = |x: usize, y: usize| &img.rgba[(y * img.width + x) * 4..][..3];
        // The first point: a red circle with the white cross of the hospital in its middle.
        let (cx, cy) = ((0.0 + 0.2 * 1000.0) / (1400.0 / 200.0), (1000.0 + 0.2 * 1000.0) / (1400.0 / 200.0));
        let (cx, cy) = (cx as usize, cy as usize);
        assert!(px(cx, cy).iter().all(|c| *c > 230), "white in the middle: {:?}", px(cx, cy));
        assert!(px(cx + 17, cy)[0] > 150 && px(cx + 17, cy)[1] < 60, "red around: {:?}", px(cx + 17, cy));
    }

    #[test]
    fn pies_are_drawn_on_the_features() {
        let pts = table(vec![point!(x: 0.0, y: 0.0).into(), point!(x: 1000.0, y: 1000.0).into()]);
        let style = LayerStyle { fill: None, stroke: None, size: 1.0, ..LayerStyle::default() };
        let (blue, orange) = ([0x2a, 0x78, 0xd6, 255], [0xeb, 0x68, 0x34, 255]);
        let charts =
            MapCharts { kind: "pie".into(), values: vec![vec![1.0, 1.0], vec![0.0, 5.0]], sizes: vec![40.0, 40.0], colours: vec![blue, orange], top: 5.0 };
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&pts, style)],
            width: 200,
            margin: 0.2,
            background: Some([255, 255, 255, 255]),
            backdrop: None,
            attribution: None,
            furniture: Default::default(),
            names: Vec::new(),
            overview: None,
            map_style: String::new(),
            themes: vec![],
            labels: vec![],
            charts: vec![Some(charts)],
            icons: vec![],
            legend: None,
        };
        let img = draw(&req, &|| true).unwrap();
        let px = |x: usize, y: usize| &img.rgba[(y * img.width + x) * 4..][..3];
        let (cx, cy) = ((0.2 * 1000.0 / 7.0) as usize, (1200.0 / 7.0) as usize);
        // Half blue to the right of twelve o'clock, half orange to the left.
        assert_eq!(px(cx + 10, cy), &blue[..3]);
        assert_eq!(px(cx - 10, cy), &orange[..3]);
        // The second all orange.
        let (cx, cy) = ((1200.0 / 7.0) as usize, (200.0 / 7.0) as usize);
        assert_eq!(px(cx + 10, cy), &orange[..3]);
    }
}

// ---------- Ready map styles ----------

/// The ready styles a map can be drawn in, as they are stored and named.
pub const MAP_STYLES: &[(&str, &str)] = &[
    ("", "As the layers are set"),
    ("light", "Light"),
    ("dark", "Dark"),
    ("topo", "Old topographic"),
    ("bw", "Print in black and white"),
    ("hand", "Drawn by hand, in ink and watercolour"),
];

/// What a colour is used for, which a ready style treats in its own way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    /// The fill of areas and points.
    Area,
    /// Lines, outlines, edges and patterns.
    Line,
    /// The letters of labels.
    Text,
    /// The halo around labels.
    Halo,
    /// The background of the map.
    Paper,
    /// The pixels of a raster, such as relief or an orthophoto.
    Picture,
}

fn mix(c: [u8; 4], to: [u8; 3], t: f32) -> [u8; 4] {
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
    [m(c[0], to[0]), m(c[1], to[1]), m(c[2], to[2]), c[3]]
}

fn grey_of(c: [u8; 4]) -> f32 {
    (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32) / 255.0
}

/// Less of its colour, by a share from 0 (as it is) to 1 (grey).
fn duller(c: [u8; 4], t: f32) -> [u8; 4] {
    let g = (grey_of(c) * 255.0).round() as u8;
    mix(c, [g, g, g], t)
}

/// The paper of a ready style, for a map whose background is a colour.
pub fn style_paper(style: &str) -> Option<[u8; 4]> {
    match style {
        "light" => Some([0xf7, 0xf6, 0xf2, 255]),
        "dark" => Some([0x1d, 0x22, 0x28, 255]),
        "topo" => Some([0xf0, 0xe6, 0xcc, 255]),
        "bw" => Some([255, 255, 255, 255]),
        "hand" => Some([0xfb, 0xf7, 0xee, 255]),
        _ => None,
    }
}

/// A colour as a ready style draws it: the hue of the layer kept where the style has
/// colours, so that layers stay apart.
pub fn style_colour(style: &str, c: [u8; 4], role: Role) -> [u8; 4] {
    match (style, role) {
        (_, Role::Paper) => style_paper(style).map_or(c, |p| [p[0], p[1], p[2], c[3]]),
        ("light", Role::Picture) => mix(duller(c, 0.3), [255, 255, 255], 0.2),
        ("dark", Role::Picture) => mix(duller(c, 0.35), [0x1d, 0x22, 0x28], 0.45),
        ("topo", Role::Picture) => mix(duller(c, 0.6), [0xd9, 0xc4, 0x96], 0.3),
        ("bw", Role::Picture) => {
            let g = (grey_of(c) * 255.0).round() as u8;
            [g, g, g, c[3]]
        }
        ("light", Role::Area) => mix(duller(c, 0.35), [255, 255, 255], 0.45),
        ("light", Role::Line) => mix(duller(c, 0.25), [0x55, 0x55, 0x55], 0.15),
        ("light", Role::Text) => [0x3a, 0x3a, 0x3a, c[3]],
        ("light", Role::Halo) => [255, 255, 255, c[3]],
        ("dark", Role::Area) => mix(duller(c, 0.3), [0x1d, 0x22, 0x28], 0.5),
        ("dark", Role::Line) => mix(duller(c, 0.2), [255, 255, 255], 0.35),
        ("dark", Role::Text) => [0xe8, 0xe6, 0xe1, c[3]],
        ("dark", Role::Halo) => [0x1d, 0x22, 0x28, c[3]],
        // Ink and watercolour on yellowed paper: browns for lines, soft earth tones.
        ("topo", Role::Area) => mix(duller(c, 0.55), [0xe2, 0xd2, 0xaa], 0.45),
        ("topo", Role::Line) => mix(duller(c, 0.5), [0x5a, 0x3c, 0x22], 0.45),
        ("topo", Role::Text) => [0x3b, 0x2a, 0x1a, c[3]],
        ("topo", Role::Halo) => [0xf0, 0xe6, 0xcc, c[3]],
        // Greys only: areas light, so that lines and text stand out, and lines dark.
        ("bw", Role::Area) => {
            let g = (215.0 + grey_of(c) * 35.0).round() as u8;
            [g, g, g, c[3]]
        }
        ("bw", Role::Line) => {
            let g = (grey_of(c) * 110.0).round() as u8;
            [g, g, g, c[3]]
        }
        ("bw", Role::Text) => [0, 0, 0, c[3]],
        ("bw", Role::Halo) => [255, 255, 255, c[3]],
        // Watercolour, thin and a little see-through, and ink in sepia.
        ("hand", Role::Area) => {
            let w = mix(duller(c, 0.2), [0xfb, 0xf7, 0xee], 0.25);
            [w[0], w[1], w[2], (c[3] as f32 * 0.82) as u8]
        }
        ("hand", Role::Line) => mix(c, [0x3a, 0x30, 0x2a], 0.55),
        ("hand", Role::Picture) => mix(duller(c, 0.25), [0xfb, 0xf7, 0xee], 0.2),
        ("hand", Role::Text) => [0x33, 0x2b, 0x26, c[3]],
        ("hand", Role::Halo) => [0xfb, 0xf7, 0xee, c[3]],
        _ => c,
    }
}

impl LayerStyle {
    /// The style as a ready map style draws it.
    pub fn in_style(&self, style: &str) -> LayerStyle {
        if style.is_empty() {
            return self.clone();
        }
        let f = |c: Option<[u8; 4]>, role: Role| c.map(|c| style_colour(style, c, role));
        LayerStyle {
            fill: f(self.fill, Role::Area),
            stroke: f(self.stroke, Role::Line),
            casing: f(self.casing, Role::Line),
            pattern_colour: f(self.pattern_colour, Role::Line),
            // Watercolour gathers at the edge of an area as it dries.
            band: if style == "hand" && self.fill.is_some() { self.band.max(4.0) } else { self.band },
            ..self.clone()
        }
    }
}

// ---------- Overview map ----------

/// A small map in a corner that shows where the map lies in a larger area.
#[derive(Debug, Clone, Default)]
pub struct Overview {
    /// The corner, as for the legend: "bottom-left" and so on.
    pub corner: String,
    /// Its width, as a share of the width of the map.
    pub share: f32,
    /// How many times wider than the map it reaches, when it has no layers of its own.
    pub zoom: f64,
    /// Layers of its own, such as the outline of a county, in the coordinate system of the
    /// map. Empty: the lines and areas of the map itself.
    pub layers: Vec<(Table, LayerStyle)>,
    /// A background map under it, as large as its box in pixels.
    pub backdrop: Option<RasterImage>,
    /// The colours of its areas and lines, in place of those of the layers. None keeps them.
    pub fill: Option<[u8; 4]>,
    pub line: Option<[u8; 4]>,
    /// The colour of the frame around it, and of the box that shows where the map is.
    pub frame: Option<[u8; 4]>,
    pub mark: Option<[u8; 4]>,
}

impl Overview {
    /// What it shows, west, south, east and north: around its own layers, or around the map
    /// `main`, so many times wider.
    pub fn extent(&self, main: [f64; 4]) -> [f64; 4] {
        use geo::BoundingRect;
        let (cx, cy) = ((main[0] + main[2]) / 2.0, (main[1] + main[3]) / 2.0);
        let mut ext = if self.layers.is_empty() {
            let z = self.zoom.max(1.5);
            let (hw, hh) = ((main[2] - main[0]) / 2.0 * z, (main[3] - main[1]) / 2.0 * z);
            [cx - hw, cy - hh, cx + hw, cy + hh]
        } else {
            let mut e = main;
            for (t, _) in &self.layers {
                for f in &t.features {
                    if let Some(r) = f.geom.as_ref().and_then(|g| g.bounding_rect()) {
                        e = [e[0].min(r.min().x), e[1].min(r.min().y), e[2].max(r.max().x), e[3].max(r.max().y)];
                    }
                }
            }
            let pad = (e[2] - e[0]).max(e[3] - e[1]) * 0.05;
            [e[0] - pad, e[1] - pad, e[2] + pad, e[3] + pad]
        };
        if ext[2] - ext[0] <= 0.0 || ext[3] - ext[1] <= 0.0 {
            ext = [ext[0] - 1.0, ext[1] - 1.0, ext[2] + 1.0, ext[3] + 1.0];
        }
        ext
    }

    /// Its box in whole pixels, on a map of this size, for the extent it shows.
    pub fn size(&self, width: u32, height: u32, ext: [f64; 4]) -> (u32, u32) {
        let (ew, eh) = (ext[2] - ext[0], ext[3] - ext[1]);
        let mut w = width as f64 * self.share.clamp(0.08, 0.5) as f64;
        let mut h = w * eh / ew;
        if h > height as f64 * 0.5 {
            h = height as f64 * 0.5;
            w = h * ew / eh;
        }
        ((w.round() as u32).max(1), (h.round() as u32).max(1))
    }
}

/// The overview map in its corner: what lies around the map, and the map as a red frame.
fn draw_overview(pixmap: &mut Canvas, ov: &Overview, main: [f64; 4], map_layers: &[(&Table, LayerStyle)], paper: Option<[u8; 4]>, at: Option<(f32, f32)>) {
    use geo::BooleanOps;
    let (width, height) = (pixmap.width() as f32, pixmap.height() as f32);
    let own: Vec<(&Table, &LayerStyle)> = ov.layers.iter().map(|(t, s)| (t, s)).collect();
    let layers: Vec<(&Table, &LayerStyle)> =
        if own.is_empty() { map_layers.iter().filter(|(t, _)| t.image.is_none()).map(|(t, s)| (*t, s)).collect() } else { own.clone() };
    let ext = ov.extent(main);
    let (bw, bh) = ov.size(pixmap.width(), pixmap.height(), ext);
    let (w, h) = (bw as f32, bh as f32);
    let ew = ext[2] - ext[0];
    let (x0, y0) = at.unwrap_or_else(|| Slots::new(width, height).place(&ov.corner, w, h));
    let scale = w as f64 / ew;
    let to_px = |x: f64, y: f64| (x0 + ((x - ext[0]) * scale) as f32, y0 + ((ext[3] - y) * scale) as f32);
    let Some(rect) = tiny_skia::Rect::from_xywh(x0, y0, w, h) else { return };
    // On the paper of the map, or its background map.
    let p = paper.filter(|c| c[3] > 0).unwrap_or([255, 255, 255, 255]);
    pixmap.fill_rect(rect, &paint([p[0], p[1], p[2], 245]), Transform::identity(), None);
    if let Some(bd) = ov.backdrop.as_ref().filter(|bd| bd.width == bw as usize && bd.height == bh as usize) {
        let mut data = Vec::with_capacity(bd.rgba.len());
        for p in bd.rgba.chunks_exact(4) {
            let a = p[3] as u16;
            data.extend_from_slice(&[(p[0] as u16 * a / 255) as u8, (p[1] as u16 * a / 255) as u8, (p[2] as u16 * a / 255) as u8, p[3]]);
        }
        if let Some(under) = tiny_skia::IntSize::from_wh(bw, bh).and_then(|size| Pixmap::from_vec(data, size)) {
            pixmap.draw_pixmap(x0 as i32, y0 as i32, under.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
        }
    }
    // The layers, cut to the box, from the bottom up.
    let bounds = geo::Rect::new((ext[0], ext[1]), (ext[2], ext[3])).to_polygon();
    let thin = |s: &LayerStyle| Stroke { width: s.width.clamp(0.0, 1.0).max(0.5), line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
    for (t, style) in &layers {
        let mut areas = PathBuilder::new();
        let mut lines = PathBuilder::new();
        let ring = |pb: &mut PathBuilder, ls: &geo::LineString<f64>, close: bool| {
            for (k, c) in ls.0.iter().enumerate() {
                let (x, y) = to_px(c.x, c.y);
                if k == 0 {
                    pb.move_to(x, y);
                } else {
                    pb.line_to(x, y);
                }
            }
            if close && !ls.0.is_empty() {
                pb.close();
            }
        };
        for f in &t.features {
            let Some(g) = &f.geom else { continue };
            match g {
                Geometry::Polygon(_) | Geometry::MultiPolygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => {
                    let mp: geo::MultiPolygon<f64> = match g {
                        Geometry::Polygon(p) => p.clone().into(),
                        Geometry::MultiPolygon(m) => m.clone(),
                        Geometry::Rect(r) => r.to_polygon().into(),
                        Geometry::Triangle(t) => t.to_polygon().into(),
                        _ => continue,
                    };
                    for p in mp.intersection(&geo::MultiPolygon::from(bounds.clone())).0 {
                        ring(&mut areas, p.exterior(), true);
                        for i in p.interiors() {
                            ring(&mut areas, i, true);
                        }
                    }
                }
                Geometry::LineString(_) | Geometry::MultiLineString(_) | Geometry::Line(_) => {
                    let ml: geo::MultiLineString<f64> = match g {
                        Geometry::LineString(l) => l.clone().into(),
                        Geometry::MultiLineString(m) => m.clone(),
                        Geometry::Line(l) => geo::LineString::from(*l).into(),
                        _ => continue,
                    };
                    for l in bounds.clip(&ml, false).0 {
                        ring(&mut lines, &l, false);
                    }
                }
                _ => {}
            }
        }
        // The colours of the overview, where it has its own.
        let (fill, line) = (ov.fill.or(style.fill), ov.line.or(style.stroke));
        if let Some(path) = areas.finish() {
            if let Some(c) = fill {
                pixmap.fill_path(&path, &paint(c), FillRule::EvenOdd, Transform::identity(), None);
            }
            if let Some(c) = line {
                pixmap.stroke_path(&path, &paint(c), &thin(style), Transform::identity(), None);
            }
        }
        if let (Some(path), Some(c)) = (lines.finish(), line.or(fill)) {
            pixmap.stroke_path(&path, &paint(c), &thin(style), Transform::identity(), None);
        }
    }
    // The map itself, in red: a frame, or a ring around it when it is too small to see.
    let red = ov.mark.unwrap_or([0xd7, 0x30, 0x27, 255]);
    let (ax, ay) = to_px(main[0], main[3]);
    let (bx, by) = to_px(main[2], main[1]);
    let frame_stroke = Stroke { width: (w / 90.0).clamp(1.5, 4.0), line_join: tiny_skia::LineJoin::Miter, ..Stroke::default() };
    if bx - ax >= 6.0 && by - ay >= 6.0 {
        if let Some(r) = tiny_skia::Rect::from_ltrb(ax, ay, bx, by) {
            pixmap.stroke_path(&PathBuilder::from_rect(r), &paint(red), &frame_stroke, Transform::identity(), None);
        }
    } else {
        let mut pb = PathBuilder::new();
        pb.push_circle((ax + bx) / 2.0, (ay + by) / 2.0, (w / 40.0).max(4.0));
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &paint(red), &frame_stroke, Transform::identity(), None);
        }
    }
    pixmap.stroke_path(
        &PathBuilder::from_rect(rect),
        &paint(ov.frame.unwrap_or([0x44, 0x44, 0x44, 255])),
        &Stroke { width: (w / 200.0).clamp(1.0, 2.0), ..Stroke::default() },
        Transform::identity(),
        None,
    );
}
