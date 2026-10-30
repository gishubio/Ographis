//! Draws the streams of a model into a picture with coordinates: one layer per stream, each
//! with its colours, in the order the user has given. Anti-aliased, with tiny-skia.

use crate::model::{GbError, RasterImage, Result, Table};
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
        LayerStyle { fill: Some([fill[0], fill[1], fill[2], 255]), stroke: Some([line[0], line[1], line[2], 255]), width: 1.0, size: 6.0 }
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
    /// "auto": beside points, along lines and in the middle of areas; "center": on the
    /// point or in the middle; "xy": at the coordinates in two attributes.
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
pub fn draw_icon(pm: &mut Pixmap, name: &str, x: f32, y: f32, size: f32, style: &str, fill: Option<[u8; 4]>, stroke: Option<[u8; 4]>) {
    if style == "circle" {
        let mut pb = PathBuilder::new();
        pb.push_circle(x, y, size / 2.0);
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint(fill.unwrap_or([0x33, 0x33, 0x33, 255])), FillRule::Winding, Transform::identity(), None);
            if let Some(c) = stroke {
                pm.stroke_path(&path, &paint(c), &Stroke { width: (size / 14.0).max(1.0), ..Stroke::default() }, Transform::identity(), None);
            }
        }
        crate::icons::draw(pm, name, x, y, size * 0.62, [255, 255, 255, 255], None);
    } else {
        crate::icons::draw(pm, name, x, y, size, fill.or(stroke).unwrap_or([0x33, 0x33, 0x33, 255]), stroke.filter(|_| fill.is_some()));
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
fn draw_raster(pixmap: &mut Pixmap, table: &Table, r: &crate::rasterlook::Prepared, origin: [f64; 2], px: f64) {
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
            data[row * pw + col] = tiny_skia::ColorU8::from_rgba(c[0], c[1], c[2], c[3]).premultiply();
        }
    }
    let paint = tiny_skia::PixmapPaint { opacity: r.opacity, quality: tiny_skia::FilterQuality::Nearest, ..Default::default() };
    pixmap.draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
}

/// Draws the layers. `going` is asked now and then whether to go on, and the drawing ends
/// with an error when it answers false.
pub fn draw(req: &Request, going: &dyn Fn() -> bool) -> Result<RasterImage> {
    let Some(mut b) = extent(&req.layers) else {
        return Err(GbError("There is nothing to draw: the layers have no geometry.".into()));
    };
    // A single point, or features in a line, still get a picture with some room.
    let side = (b[2] - b[0]).max(b[3] - b[1]);
    let pad = if side > 0.0 { side * req.margin.max(0.0) } else { 100.0 };
    b = [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad];
    let width = req.width.clamp(1, MAX_SIDE);
    let px = (b[2] - b[0]) / width as f64;
    let height = ((b[3] - b[1]) / px).ceil().max(1.0);
    if height > MAX_SIDE as f64 {
        return Err(GbError(format!(
            "The picture would be {} pixels high, and at most {} are allowed. Make it narrower, or draw a smaller area.",
            height as u64, MAX_SIDE
        )));
    }
    let height = height as u32;
    // The extent is made as high as the whole number of rows.
    let top = b[1] + height as f64 * px;
    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| GbError("There is not memory enough for a picture that large.".into()))?;
    if let Some(c) = req.background {
        pixmap.fill(Color::from_rgba8(c[0], c[1], c[2], c[3]));
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
        // A raster is drawn cell by cell, rather than its outline.
        if let Some(r) = req.rasters.get(li).and_then(|r| r.as_ref()) {
            draw_raster(&mut pixmap, table, r, [b[0], top], px);
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
        // A layer that shows its values is drawn a feature at a time, in its own order.
        if let Some(theme) = theme.filter(|t| t.looks.is_some() || t.order.is_some() || t.cluster > 0.0) {
            draw_themed(&mut pixmap, table, style, theme, &to_px, going)?;
            if let Some(ic) = icons {
                draw_icons(&mut pixmap, req.layers[li].0, style, Some(theme), ic, &to_px);
            }
            continue;
        }
        let stroke = Stroke { width: style.width.max(0.0), line_join: tiny_skia::LineJoin::Round, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
        let line_paint = style.stroke.filter(|_| style.width > 0.0).map(paint);
        let fill_paint = style.fill.map(paint);
        // Lines and points are gathered in batches and drawn a batch at a time, which is
        // much quicker than one at a time. Areas are drawn one by one, so that each is
        // filled on its own and areas that overlap do not cancel out.
        let mut lines = PathBuilder::new();
        let mut dots = PathBuilder::new();
        let mut in_batch = 0usize;
        let radius = (style.size / 2.0).max(0.5);
        let flush = |pixmap: &mut Pixmap, lines: &mut PathBuilder, dots: &mut PathBuilder| {
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
        let add = |pixmap: &mut Pixmap, g: &Geometry<f64>, lines: &mut PathBuilder, dots: &mut PathBuilder, in_batch: &mut usize| {
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
            place_labels(&mut pixmap, &text, table, lb, point_r, &to_px, &mut taken);
        }
    }
    if let Some(legend) = &req.legend {
        draw_legend(&mut pixmap, legend)?;
    }
    // tiny-skia keeps the colours premultiplied by their opacity. Files want them plain.
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(RasterImage { width: width as usize, height: height as usize, rgba, geotransform: [b[0], px, 0.0, top, 0.0, -px] })
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

/// Draws a layer a feature at a time, each in its own look and in the order of the theme,
/// with points that lie close together drawn as one with their number.
fn draw_themed(
    pixmap: &mut Pixmap,
    table: &Table,
    style: &LayerStyle,
    theme: &crate::thematic::Theme,
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
    going: &dyn Fn() -> bool,
) -> Result<()> {
    let base = crate::thematic::Look { fill: style.fill, stroke: style.stroke, size: style.size };
    let look = |i: usize| theme.looks.as_ref().map_or(base, |l| l[i]);
    let order: Vec<usize> = theme.order.clone().unwrap_or_else(|| (0..table.len()).collect());
    let stroke = Stroke { width: style.width.max(0.0), line_join: tiny_skia::LineJoin::Round, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
    let dot = |pixmap: &mut Pixmap, x: f32, y: f32, r: f32, l: &crate::thematic::Look| {
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
            text.draw(pixmap, &n.to_string(), x, y + size * 0.36, size, true, ink, 0.5);
        }
    }
    Ok(())
}

/// Draws the points of a layer that have icons, in the order and the looks of the theme.
fn draw_icons(
    pm: &mut Pixmap,
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
}

/// The places a label may go, the best first.
fn spots(g: &Geometry<f64>, spec: &LabelSpec, at: Option<(f64, f64)>, w: f32, size: f32, point_r: f32, to_px: &dyn Fn(f64, f64) -> (f32, f32)) -> Vec<Spot> {
    use geo::{InteriorPoint, Length};
    let mid = size * 0.35;
    let centred = |x: f32, y: f32| Spot { x, y: y + mid, angle: 0.0, align: 0.5 };
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
            Spot { x: x + r, y: y + mid, angle: 0.0, align: 0.0 },
            Spot { x: x + d, y: y - d, angle: 0.0, align: 0.0 },
            Spot { x: x + d, y: y + d + size * 0.75, angle: 0.0, align: 0.0 },
            Spot { x: x - r, y: y + mid, angle: 0.0, align: 1.0 },
            Spot { x, y: y - r - size * 0.2, angle: 0.0, align: 0.5 },
            Spot { x, y: y + r + size * 0.8, angle: 0.0, align: 0.5 },
            Spot { x: x - d, y: y - d, angle: 0.0, align: 1.0 },
            Spot { x: x - d, y: y + d + size * 0.75, angle: 0.0, align: 1.0 },
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
            out.push(Spot { x: cx - t.sin() * mid, y: cy + t.cos() * mid, angle, align: 0.5 });
        }
        return out;
    }
    match g.interior_point() {
        Some(p) => {
            let (x, y) = to_px(p.x(), p.y());
            vec![centred(x, y)]
        }
        None => Vec::new(),
    }
}

/// Places the labels of a layer where there is room, or all of them, and draws them.
fn place_labels(
    pm: &mut Pixmap,
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
                &spots(&Geometry::GeometryCollection(Default::default()), spec, item.at, 0.0, item.size, point_r, to_px),
                taken,
                pw,
                ph,
            );
            continue;
        };
        let w = text.width(&item.text, item.size, spec.bold);
        let candidates = spots(g, spec, item.at, w, item.size, point_r, to_px);
        draw_one(pm, text, item, spec, &candidates, taken, pw, ph);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_one(pm: &mut Pixmap, text: &crate::chart::Text, item: &LabelItem, spec: &LabelSpec, candidates: &[Spot], taken: &mut Vec<[f32; 4]>, pw: f32, ph: f32) {
    let w = text.width(&item.text, item.size, spec.bold);
    let h = item.size;
    let pad = spec.halo.map_or(0.0, |_| spec.halo_width) + 1.0;
    for spot in candidates {
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

/// Draws the legend in a box in its corner.
fn draw_legend(pixmap: &mut Pixmap, legend: &Legend) -> Result<()> {
    let text = crate::chart::Text::new()?;
    let k = (pixmap.width().min(pixmap.height()) as f32 / 900.0).clamp(0.8, 3.0);
    let (size, small, pad, sym) = (14.0 * k, 12.5 * k, 12.0 * k, 16.0 * k);
    let line = size * 1.5;
    // The rows: (indent, text, bold, symbol)
    enum Sym {
        None,
        Look(crate::thematic::Look, &'static str, f32),
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
            rows.push((0.0, item.name.clone(), false, Sym::Look(look, item.shape, item.style.width), size));
        } else {
            let head = if item.heading.is_empty() { item.name.clone() } else { format!("{} · {}", item.name, item.heading) };
            rows.push((0.0, head, true, Sym::None, size));
            for key in &item.keys {
                rows.push((0.0, key.text.clone(), false, Sym::Look(key.look, item.shape, item.style.width), small));
            }
        }
        if !item.size_keys.is_empty() {
            rows.push((0.0, item.size_heading.clone(), true, Sym::None, small));
            for key in &item.size_keys {
                rows.push((0.0, key.text.clone(), false, Sym::Look(key.look, item.shape, item.style.width), small));
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
            Sym::Look(l, "point", _) => (l.size * k.min(1.5)).max(sym),
            Sym::Icon(..) => icon_px,
            _ => sym,
        })
        .fold(sym, f32::max);
    let heights: Vec<f32> = rows
        .iter()
        .map(|r| match &r.3 {
            Sym::Look(l, "point", _) => line.max(l.size * k.min(1.5) + 4.0),
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
        if let Sym::Look(l, shape, width) = &row.3 {
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
                "line" => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(x, mid);
                    pb.line_to(x + sym_w, mid);
                    if let (Some(p), Some(c)) = (pb.finish(), l.stroke) {
                        pixmap.stroke_path(&p, &paint(c), &Stroke { width: width.max(1.5), ..st }, Transform::identity(), None);
                    }
                }
                _ => {
                    if let Some(r) = tiny_skia::Rect::from_xywh(x, mid - sym * 0.4, sym_w, sym * 0.8) {
                        let p = PathBuilder::from_rect(r);
                        if let Some(c) = l.fill {
                            pixmap.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
                        }
                        if let (Some(c), true) = (l.stroke, *width > 0.0) {
                            pixmap.stroke_path(&p, &paint(c), &Stroke { width: width.min(2.0), ..st }, Transform::identity(), None);
                        }
                    }
                }
            }
            x += sym_w + 8.0 * k;
        }
        text.draw(pixmap, &row.1, x, mid + row.4 * 0.36, row.4, row.2, [0x0b, 0x0b, 0x0b], 0.0);
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
        let blue = LayerStyle { fill: Some([166, 206, 227, 255]), stroke: Some([31, 120, 180, 255]), width: 2.0, size: 6.0 };
        let red = LayerStyle { fill: None, stroke: Some([255, 0, 0, 255]), width: 4.0, size: 1.0 };
        let black = LayerStyle { fill: Some([0, 0, 0, 255]), stroke: None, width: 0.0, size: 10.0 };
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&area, blue), (&line, red), (&dot, black)],
            width: 100,
            margin: 0.0,
            background: Some([255, 255, 255, 255]),
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
        let style = LayerStyle { fill: Some([0, 0, 0, 255]), stroke: None, width: 0.0, size: 6.0 };
        let ink = |img: &RasterImage| img.rgba.chunks(4).filter(|p| p[3] > 0 && p[0] < 100).count();
        let run = |labels: Labels| {
            let req = Request {
                rasters: Vec::new(),
                layers: vec![(&pts, style.clone())],
                width: 400,
                margin: 0.1,
                background: None,
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
            layers: vec![(&line, LayerStyle { fill: None, stroke: Some([200, 200, 200, 255]), width: 1.0, size: 1.0 })],
            width: 300,
            margin: 0.0,
            background: Some([255, 255, 255, 255]),
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
        let style = LayerStyle { fill: Some([200, 0, 0, 255]), stroke: Some([255, 255, 255, 255]), width: 1.0, size: 40.0 };
        let req = Request {
            rasters: Vec::new(),
            layers: vec![(&pts, style.clone())],
            width: 200,
            margin: 0.2,
            background: Some([255, 255, 255, 255]),
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
