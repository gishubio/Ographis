//! Draws the streams of a model into a picture with coordinates: one layer per stream, each
//! with its colours, in the order the user has given. Anti-aliased, with tiny-skia.

use crate::model::{GbError, RasterImage, Result, Table};
use crate::vector::Canvas;
use geo::Geometry;
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
];

/// The ways a line can be drawn, as they are stored and named.
pub const DASHES: &[(&str, &str)] = &[("", "Solid"), ("dash", "Dashed"), ("dot", "Dotted"), ("dashdot", "Dash and dot")];

impl LayerStyle {
    /// The stroke of lines and outlines in this style, with its dashes.
    pub fn stroke(&self) -> Stroke {
        let w = self.width.max(0.0);
        let unit = w.max(1.0);
        let pattern: Option<Vec<f32>> = match self.dash.as_str() {
            "dash" => Some(vec![unit * 4.0, unit * 2.5]),
            "dot" => Some(vec![0.01, unit * 2.0]),
            "dashdot" => Some(vec![unit * 4.0, unit * 2.0, 0.01, unit * 2.0]),
            _ => None,
        };
        let cap = if self.dash == "dot" || self.dash == "dashdot" {
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
    pub colour: [u8; 4],
    /// An edge around the letters, so that they can be read on anything. None: none.
    pub halo: Option<[u8; 4]>,
    pub halo_width: f32,
    /// The letters spread out over an area, as the name of a lake or a district.
    pub spaced: bool,
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
            colour: [0x22, 0x22, 0x22, 255],
            halo: Some([255, 255, 255, 255]),
            halo_width: 2.0,
            spaced: false,
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
pub struct Request<'a> {
    pub layers: Vec<(&'a Table, LayerStyle)>,
    /// How each layer shows its values, in the order of `layers`. Empty: none does.
    pub themes: Vec<crate::thematic::Theme>,
    /// The labels of each layer, in the order of `layers`. Empty: none has labels.
    pub labels: Vec<Option<Labels>>,
    /// The icons of the points of each layer, in the order of `layers`. Empty: none.
    pub icons: Vec<Option<Icons>>,
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
}

impl Legend {
    fn icon_style(&self, item: &LegendItem) -> String {
        self.icon_styles.iter().find(|(n, _)| *n == item.name).map_or_else(|| "plain".to_string(), |(_, s)| s.clone())
    }
}

#[derive(Debug, Clone)]
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
    /// The colour ramp of a heatmap, drawn as a band from little to most.
    pub heat_ramp: Option<String>,
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
    let (image, grid): (Option<&RasterImage>, Option<&crate::model::Grid>) = match (&r.paint, &table.grid, &table.image) {
        (RP::Image, _, Some(img)) => (Some(img.as_ref()), None),
        (RP::Shade, Some(g), _) => {
            shaded = crate::rasterlook::picture(g, &RP::Shade);
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

/// Draws the layers. `going` is asked now and then whether to go on, and the drawing ends
/// with an error when it answers false.
pub fn draw(req: &Request, going: &dyn Fn() -> bool) -> Result<RasterImage> {
    let Frame { b, px, width, height } = frame(&req.layers, req.width, req.margin)?;
    // The extent is made as high as the whole number of rows.
    let top = b[1] + height as f64 * px;
    let mut pixmap = Canvas::new(width, height, true).ok_or_else(|| GbError("There is not memory enough for a picture that large.".into()))?;
    let name_of = |li: usize| req.names.get(li).filter(|n| !n.trim().is_empty()).cloned().unwrap_or_else(|| format!("Layer {}", li + 1));
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
    let to_px = |x: f64, y: f64| (((x - b[0]) / px) as f32, ((top - y) / px) as f32);
    let ring = |pb: &mut PathBuilder, ls: &geo::LineString<f64>, close: bool| {
        let mut first = true;
        for c in &ls.0 {
            let (x, y) = to_px(c.x, c.y);
            if first {
                pb.move_to(x, y);
                first = false;
            } else {
                pb.line_to(x, y);
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
    for (li, (table, style)) in req.layers.iter().enumerate() {
        if !going() {
            return Err(GbError(crate::model::STOPPED.into()));
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
            if let Some(ic) = icons {
                draw_icons(&mut pixmap, req.layers[li].0, style, Some(theme), ic, &to_px);
            }
            continue;
        }
        let stroke = style.stroke();
        draw_casing(&mut pixmap, table, style, &to_px);
        let line_paint = style.stroke.filter(|_| style.width > 0.0).map(paint);
        let fill_paint = style.fill.map(paint);
        // Lines and points are gathered in batches and drawn a batch at a time, which is
        // much quicker than one at a time. Areas are drawn one by one, so that each is
        // filled on its own and areas that overlap do not cancel out.
        let mut lines = PathBuilder::new();
        let mut dots = PathBuilder::new();
        let mut in_batch = 0usize;
        let radius = (style.size / 2.0).max(0.5);
        let flush = |pixmap: &mut Canvas, lines: &mut PathBuilder, dots: &mut PathBuilder| {
            let (l, d) = (std::mem::take(lines), std::mem::take(dots));
            if let (Some(path), Some(p)) = (l.finish(), &line_paint) {
                pixmap.stroke_path(&path, p, &stroke, Transform::identity(), None);
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
        let add = |pixmap: &mut Canvas, g: &Geometry<f64>, lines: &mut PathBuilder, dots: &mut PathBuilder, in_batch: &mut usize| {
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
                        if let Some(p) = &fill_paint {
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
            match g {
                Geometry::GeometryCollection(c) => {
                    for part in &c.0 {
                        add(&mut pixmap, part, &mut lines, &mut dots, &mut in_batch);
                    }
                }
                _ => add(&mut pixmap, g, &mut lines, &mut dots, &mut in_batch),
            }
            if in_batch >= 2_000 {
                flush(&mut pixmap, &mut lines, &mut dots);
                in_batch = 0;
            }
        }
        flush(&mut pixmap, &mut lines, &mut dots);
        if let Some(ic) = icons {
            draw_icons(&mut pixmap, req.layers[li].0, style, theme, ic, &to_px);
        }
    }
    pixmap.set_opacity(1.0);
    // The labels, on top of every layer: those of the top layer first, so that they get
    // the room when there is not enough for all.
    if req.labels.iter().any(|l| l.is_some()) {
        let text = crate::chart::Text::new()?;
        let mut taken: Vec<[f32; 4]> = Vec::new();
        for li in (0..req.layers.len()).rev() {
            let Some(lb) = req.labels.get(li).and_then(|l| l.as_ref()) else { continue };
            let (table, style) = &req.layers[li];
            let point_r = match req.icons.get(li).and_then(|i| i.as_ref()) {
                Some(_) => icon_size(style.size) / 2.0,
                None => style.size / 2.0,
            };
            pixmap.layer(Some(&format!("{} labels", name_of(li))));
            place_labels(&mut pixmap, &text, table, lb, point_r, &to_px, &mut taken);
        }
    }
    let credit = req.attribution.as_ref().filter(|a| !a.trim().is_empty());
    pixmap.layer(Some("Frame and legend"));
    draw_furniture(&mut pixmap, &req.furniture, [b[0], b[1], b[0] + width as f64 * px, top], px, credit.is_some())?;
    if let Some(ov) = &req.overview {
        draw_overview(&mut pixmap, ov, [b[0], b[1], b[0] + width as f64 * px, top], &req.layers, req.background);
    }
    if let Some(legend) = &req.legend {
        draw_legend(&mut pixmap, legend)?;
    }
    if let Some(a) = credit {
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
    let look = |i: usize| theme.looks.as_ref().map_or(base, |l| l[i]);
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
        let Some((path, area)) = path_of(g, to_px) else { continue };
        if let (Some(c), true) = (l.fill, area) {
            pixmap.fill_path(&path, &paint(c), FillRule::EvenOdd, Transform::identity(), None);
        }
        if area {
            draw_pattern(pixmap, g, style, to_px);
        }
        if let (Some(c), true) = (l.stroke, style.width > 0.0) {
            pixmap.stroke_path(&path, &paint(c), &stroke, Transform::identity(), None);
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
        // left, above, below, upper left, lower left.
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
        if total < w * 1.1 && !spec.all {
            return Vec::new();
        }
        let mut out = Vec::new();
        for frac in [0.5f32, 0.33, 0.67, 0.2, 0.8] {
            let c = total * frac;
            // Letter by letter along the line, so that the label bends with a road or a
            // river. It reads from left to right, and is not used where the line bends too
            // sharply for the letters to follow it.
            if spec.place != "straight" && adv.len() > 1 {
                let (a, b) = (along(c - w / 2.0), along(c + w / 2.0));
                let backwards = b.0 < a.0;
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
            // Never upside down.
            if angle > 90.0 {
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
            vec![centred(x, y)]
        }
        None => Vec::new(),
    }
}

/// Places the labels of a layer where there is room, or all of them, and draws them.
fn place_labels(
    pm: &mut Canvas,
    text: &crate::chart::Text,
    table: &Table,
    labels: &Labels,
    point_r: f32,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
    taken: &mut Vec<[f32; 4]>,
) {
    let spec = &labels.spec;
    let mut order: Vec<&LabelItem> = labels.items.iter().collect();
    order.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    let (pw, ph) = (pm.width() as f32, pm.height() as f32);
    for item in order {
        let Some(g) = table.features.get(item.feature).and_then(|f| f.geom.as_ref()) else {
            if item.at.is_none() {
                continue;
            }
            // Placed by attributes, a feature needs no geometry of its own.
            draw_one(
                pm,
                text,
                item,
                spec,
                &spots(&Geometry::GeometryCollection(Default::default()), spec, item.at, 0.0, &[], item.size, point_r, to_px),
                taken,
                pw,
                ph,
            );
            continue;
        };
        let w = text.width(&item.text, item.size, spec.bold);
        let adv: Vec<f32> = item.text.chars().map(|c| text.width(&c.to_string(), item.size, spec.bold)).collect();
        let candidates = spots(g, spec, item.at, w, &adv, item.size, point_r, to_px);
        draw_one(pm, text, item, spec, &candidates, taken, pw, ph);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_one(pm: &mut Canvas, text: &crate::chart::Text, item: &LabelItem, spec: &LabelSpec, candidates: &[Spot], taken: &mut Vec<[f32; 4]>, pw: f32, ph: f32) {
    let w = text.width(&item.text, item.size, spec.bold);
    let h = item.size;
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
            let letters: Vec<String> = item.text.chars().map(|c| c.to_string()).collect();
            let halo = spec.halo.filter(|_| spec.halo_width > 0.0);
            let st = Stroke { width: spec.halo_width * 2.0, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
            let paths: Vec<Option<(tiny_skia::Path, Transform)>> = letters
                .iter()
                .zip(&spot.glyphs)
                .map(|(l, (x, y, a))| {
                    let lw = text.width(l, item.size, spec.bold);
                    text.path(l, item.size, spec.bold).map(|p| (p, Transform::from_translate(*x, *y).pre_rotate(*a).pre_translate(-lw / 2.0, 0.0)))
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
            taken.extend(boxes);
            return;
        }
        // The box the label takes, turned, as the box around that.
        let t = spot.angle.to_radians();
        let (c, s) = (t.cos(), t.sin());
        let x0 = -w * spot.align - pad;
        let x1 = x0 + w + 2.0 * pad;
        let (y0, y1) = (-h * 0.8 - pad, h * 0.25 + pad);
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
        if let Some(path) = text.path(&item.text, item.size, spec.bold) {
            let tf = Transform::from_translate(spot.x, spot.y).pre_rotate(spot.angle).pre_translate(-w * spot.align, 0.0);
            if let Some(hc) = spec.halo.filter(|_| spec.halo_width > 0.0) {
                let st = Stroke { width: spec.halo_width * 2.0, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                pm.stroke_path(&path, &paint(hc), &st, tf, None);
            }
            pm.fill_path(&path, &paint(spec.colour), FillRule::Winding, tf, None);
        }
        taken.push(bx);
        return;
    }
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
fn draw_furniture(pixmap: &mut Canvas, f: &Furniture, b: [f64; 4], px: f64, credit: bool) -> Result<()> {
    let on = |c: &str| !c.is_empty() && c != "none";
    if !on(&f.scale_bar) && !on(&f.north) && !on(&f.grid) {
        return Ok(());
    }
    let t = crate::chart::Text::new()?;
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let k = (pw.min(ph) / 900.0).clamp(0.8, 3.0);
    let ink = [0x22, 0x22, 0x22, 255];
    let size = 12.0 * k;
    let pad = 16.0 * k;
    // Where a thing of this size goes in a corner.
    let place = |corner: &str, w: f32, h: f32| -> (f32, f32) {
        let low = if credit && corner == "bottom-right" { 24.0 * k } else { 0.0 };
        match corner {
            "top-left" => (pad, pad),
            "top-right" => (pw - pad - w, pad),
            "bottom-left" => (pad, ph - pad - h),
            _ => (pw - pad - w, ph - pad - h - low),
        }
    };
    // Where the scale bar and the north arrow go, so that the coordinates keep clear of them.
    let metres_per_px = if f.geographic { px * 111_320.0 * ((b[1] + b[3]) / 2.0).to_radians().cos().abs().max(0.01) } else { px };
    let want = metres_per_px * pw as f64 / 5.0;
    let p10 = 10f64.powf(want.log10().floor());
    let length = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * p10).rfind(|l| *l <= want).unwrap_or(p10);
    let bar_w = (length / metres_per_px) as f32;
    let bar_h = 6.0 * k;
    let last = if length >= 1000.0 { format!("{} km", crate::model::fmt_real(length / 1000.0)) } else { format!("{} m", crate::model::fmt_real(length)) };
    let lw = t.width(&last, size, false);
    let bar_box = (bar_w + lw / 2.0 + 4.0, bar_h + size * 1.6);
    let bar_at = on(&f.scale_bar).then(|| place(&f.scale_bar, bar_box.0, bar_box.1));
    let (aw, ah, nh) = (16.0 * k, 30.0 * k, size * 1.4);
    let north_at = on(&f.north).then(|| place(&f.north, aw, ah + nh));
    let mut keep_clear: Vec<[f32; 4]> = Vec::new();
    if let Some((x, y)) = bar_at {
        keep_clear.push([x - 12.0 * k, y - 8.0 * k, x + bar_box.0 + 12.0 * k, y + bar_box.1 + 14.0 * k]);
    }
    if let Some((x, y)) = north_at {
        keep_clear.push([x - 8.0 * k, y - 8.0 * k, x + aw + 8.0 * k, y + ah + nh + 8.0 * k]);
    }
    let clear = |r: [f32; 4]| !keep_clear.iter().any(|c| r[0] < c[2] && r[2] > c[0] && r[1] < c[3] && r[3] > c[1]);
    // The grid, under the rest: lines at round coordinates, and the coordinates in the edge.
    if on(&f.grid) {
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
            // East along the top edge, north along the left one, turned to run up it.
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
    if let Some((x, y)) = bar_at {
        let (w, h) = bar_box;
        let top = y + size * 1.3;
        // A box behind it, so that it can be read on a background map.
        if let Some(r) = tiny_skia::Rect::from_xywh(x - 8.0 * k, y - 4.0 * k, w + 16.0 * k, h + 10.0 * k) {
            pixmap.fill_rect(r, &paint([255, 255, 255, 200]), Transform::identity(), None);
        }
        for part in 0..4 {
            let pw4 = bar_w / 4.0;
            if let Some(r) = tiny_skia::Rect::from_xywh(x + part as f32 * pw4, top, pw4, bar_h) {
                let c = if part % 2 == 0 { ink } else { [255, 255, 255, 255] };
                pixmap.fill_rect(r, &paint(c), Transform::identity(), None);
            }
        }
        if let Some(r) = tiny_skia::Rect::from_xywh(x, top, bar_w, bar_h) {
            let path = PathBuilder::from_rect(r);
            pixmap.stroke_path(&path, &paint(ink), &Stroke { width: (k * 0.8).max(0.6), ..Stroke::default() }, Transform::identity(), None);
        }
        halo_text(pixmap, &t, "0", x - t.width("0", size, false) / 2.0, top - size * 0.35, size, false, ink);
        halo_text(pixmap, &t, &last, x + bar_w - lw / 2.0, top - size * 0.35, size, false, ink);
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
fn draw_legend(pixmap: &mut Canvas, legend: &Legend) -> Result<()> {
    let text = crate::chart::Text::new()?;
    let k = (pixmap.width().min(pixmap.height()) as f32 / 900.0).clamp(0.8, 3.0);
    let (size, small, pad, sym) = (14.0 * k, 12.5 * k, 12.0 * k, 16.0 * k);
    let line = size * 1.5;
    // The rows: (indent, text, bold, symbol)
    enum Sym<'a> {
        None,
        Look(crate::thematic::Look, &'static str, f32, Option<&'a LayerStyle>),
        Icon(&'static str, crate::thematic::Look, String),
    }
    let mut rows: Vec<(f32, String, bool, Sym, f32)> = Vec::new();
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
        } else if item.keys.is_empty() {
            rows.push((0.0, item.name.clone(), false, Sym::Look(look, item.shape, item.style.width, Some(&item.style)), size));
        } else {
            let head = if item.heading.is_empty() { item.name.clone() } else { format!("{} · {}", item.name, item.heading) };
            rows.push((0.0, head, true, Sym::None, size));
            for key in &item.keys {
                rows.push((0.0, key.text.clone(), false, Sym::Look(key.look, item.shape, item.style.width, Some(&item.style)), small));
            }
        }
        if !item.size_keys.is_empty() {
            rows.push((0.0, item.size_heading.clone(), true, Sym::None, small));
            for key in &item.size_keys {
                rows.push((0.0, key.text.clone(), false, Sym::Look(key.look, item.shape, item.style.width, Some(&item.style)), small));
            }
        }
    }
    if rows.is_empty() {
        return Ok(());
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
            Sym::Icon(..) => line.max(icon_px + 4.0),
            _ => line,
        })
        .collect();
    let text_w = rows.iter().map(|r| text.width(&r.1, r.4, r.2) + if matches!(r.3, Sym::None) { 0.0 } else { sym_w + 8.0 * k }).fold(0.0f32, f32::max);
    let (w, h) = (text_w + 2.0 * pad, heights.iter().sum::<f32>() + 2.0 * pad - (line - size) * 0.6);
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let margin = 14.0 * k;
    let x0 = if legend.corner.ends_with("right") { pw - w - margin } else { margin };
    let y0 = if legend.corner.starts_with("top") { margin } else { ph - h - margin };
    if let Some(r) = tiny_skia::Rect::from_xywh(x0, y0, w, h) {
        let bg = PathBuilder::from_rect(r);
        pixmap.fill_path(&bg, &paint([255, 255, 255, 230]), FillRule::Winding, Transform::identity(), None);
        pixmap.stroke_path(&bg, &paint([0xc3, 0xc2, 0xb7, 255]), &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
    }
    let mut y = y0 + pad;
    for (row, rh) in rows.iter().zip(&heights) {
        let mid = y + rh / 2.0 - (line - size) * 0.3;
        let mut x = x0 + pad + row.0;
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
                "line" => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(x, mid);
                    pb.line_to(x + sym_w, mid);
                    if let Some(p) = pb.finish() {
                        // With its edge and its dashes, as on the map.
                        let w = width.max(1.5).min(4.0 * k);
                        if let Some((c, e)) = style.and_then(|s| s.casing.map(|c| (c, s.casing_width))) {
                            pixmap.stroke_path(&p, &paint(c), &Stroke { width: w + 2.0 * e, ..st.clone() }, Transform::identity(), None);
                        }
                        if let Some(c) = l.stroke {
                            let dashed = style.map(|s| LayerStyle { width: w, ..s.clone() }.stroke()).unwrap_or(Stroke { width: w, ..st.clone() });
                            pixmap.stroke_path(&p, &paint(c), &dashed, Transform::identity(), None);
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
                        if let (Some(c), true) = (l.stroke, *width > 0.0) {
                            pixmap.stroke_path(&p, &paint(c), &Stroke { width: width.min(2.0), ..st }, Transform::identity(), None);
                        }
                    }
                }
            }
            x += sym_w + 8.0 * k;
        }
        write_text(pixmap, &text, &row.1, x, mid + row.4 * 0.36, row.4, row.2, [0x0b, 0x0b, 0x0b], 0.0);
        y += rh;
    }
    Ok(())
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
            items: (0..n).map(|i| LabelItem { feature: i, text: "Lidköping".into(), size: 16.0, priority: -(i as f64), at: None }).collect(),
        }
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
        corner.items = vec![LabelItem { feature: 4, text: "Hjo".into(), size: 16.0, priority: 0.0, at: None }];
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
                items: vec![LabelItem { feature: 0, text: "MMMM".into(), size: 20.0, priority: 0.0, at: None }],
            })],
            icons: vec![],
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
}

// ---------- Ready map styles ----------

/// The ready styles a map can be drawn in, as they are stored and named.
pub const MAP_STYLES: &[(&str, &str)] =
    &[("", "As the layers are set"), ("light", "Light"), ("dark", "Dark"), ("topo", "Old topographic"), ("bw", "Print in black and white")];

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
fn draw_overview(pixmap: &mut Canvas, ov: &Overview, main: [f64; 4], map_layers: &[(&Table, LayerStyle)], paper: Option<[u8; 4]>) {
    use geo::BooleanOps;
    let (width, height) = (pixmap.width() as f32, pixmap.height() as f32);
    let own: Vec<(&Table, &LayerStyle)> = ov.layers.iter().map(|(t, s)| (t, s)).collect();
    let layers: Vec<(&Table, &LayerStyle)> =
        if own.is_empty() { map_layers.iter().filter(|(t, _)| t.image.is_none()).map(|(t, s)| (*t, s)).collect() } else { own.clone() };
    let ext = ov.extent(main);
    let (bw, bh) = ov.size(pixmap.width(), pixmap.height(), ext);
    let (w, h) = (bw as f32, bh as f32);
    let ew = ext[2] - ext[0];
    let pad = (width.min(height) / 50.0).max(6.0).round();
    let x0 = if ov.corner.ends_with("left") { pad } else { width - pad - w };
    let y0 = if ov.corner.starts_with("top") { pad } else { height - pad - h };
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
        if let Some(path) = areas.finish() {
            if let Some(c) = style.fill {
                pixmap.fill_path(&path, &paint(c), FillRule::EvenOdd, Transform::identity(), None);
            }
            if let Some(c) = style.stroke {
                pixmap.stroke_path(&path, &paint(c), &thin(style), Transform::identity(), None);
            }
        }
        if let (Some(path), Some(c)) = (lines.finish(), style.stroke.or(style.fill)) {
            pixmap.stroke_path(&path, &paint(c), &thin(style), Transform::identity(), None);
        }
    }
    // The map itself, in red: a frame, or a ring around it when it is too small to see.
    let red = [0xd7, 0x30, 0x27, 255];
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
        &paint([0x44, 0x44, 0x44, 255]),
        &Stroke { width: (w / 200.0).clamp(1.0, 2.0), ..Stroke::default() },
        Transform::identity(),
        None,
    );
}
