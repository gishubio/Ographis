//! The map in the inspector. Draws the geometries of the stream over OpenStreetMap when
//! the network can be reached, otherwise over a coordinate grid in the stream's own
//! coordinate system.
//!
//! Every feature is drawn. When few are in view they are drawn as shapes, which can be
//! clicked. When many are in view they are drawn into an image in a thread of its own,
//! while the image from before is moved along with the map.

use crate::mapdata::{self, Building, Colors, InView, Kind, RenderView, Store, SHAPE_LIMIT};
use crate::theme::{self, Palette};
use crate::tiles::{self, Net, Tiles};
use eframe::egui::{self, epaint::Mesh, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use og_engine::io::Backend;
use og_engine::model::{fmt_count, fmt_real, Table};
use og_engine::nodes::is_geographic;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

const WEB_MERCATOR: u32 = 3857;
/// Streams up to this size are made ready for the map at once. Larger ones in a thread.
const AT_ONCE: usize = 50_000;

/// A stream made ready for drawing in a particular coordinate system.
struct Prepared {
    /// The table is kept so that the comparison by address cannot confuse two tables.
    table: Arc<Table>,
    mercator: bool,
    store: Arc<Store>,
    /// True if x and y are degrees in the display system.
    degrees: bool,
    /// Why the OSM background could not be used, if it was asked for.
    fallback: Option<String>,
    /// For a stream that carries an image: where a grid of points over it lies in the
    /// display system, so that it can be drawn bent to the background map.
    image_grid: Option<Vec<[f64; 2]>>,
    /// For a grid of heights: grey shaded relief to show for it.
    preview: Option<Arc<og_engine::model::RasterImage>>,
}

/// The image is placed by a grid of this many cells on a side.
const IMAGE_GRID: usize = 16;

/// The largest side of the image as it is shown. A larger image is shrunk for the screen.
const IMAGE_SIDE: usize = 4096;

/// An image for the screen: shrunk by averaging when it is larger than the screen can hold.
fn image_for_screen(img: &og_engine::model::RasterImage) -> egui::ColorImage {
    let step = img.width.max(img.height).div_ceil(IMAGE_SIDE).max(1);
    if step == 1 {
        return egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.rgba);
    }
    let (w, h) = (img.width.div_ceil(step), img.height.div_ceil(step));
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for yy in y * step..((y + 1) * step).min(img.height) {
                for xx in x * step..((x + 1) * step).min(img.width) {
                    let k = (yy * img.width + xx) * 4;
                    for (c, v) in sum.iter_mut().zip(&img.rgba[k..k + 4]) {
                        *c += *v as u32;
                    }
                    n += 1;
                }
            }
            for c in 0..4 {
                out[(y * w + x) * 4 + c] = (sum[c] / n.max(1)) as u8;
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &out)
}

/// Where the corners of the grid over an image lie, in the display system.
fn grid_of(img: &og_engine::model::RasterImage, from: Option<u32>, to: Option<u32>, backend: &dyn Backend) -> Option<Vec<[f64; 2]>> {
    let n = IMAGE_GRID;
    let (mut xs, mut ys) = (Vec::with_capacity((n + 1) * (n + 1)), Vec::with_capacity((n + 1) * (n + 1)));
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = img.at(img.width as f64 * i as f64 / n as f64, img.height as f64 * j as f64 / n as f64);
            xs.push(x);
            ys.push(y);
        }
    }
    if let (Some(a), Some(b)) = (from, to) {
        if a != b {
            backend.transform(a, b, &mut xs, &mut ys).ok()?;
        }
    }
    Some(xs.into_iter().zip(ys).map(|(x, y)| [x, y]).collect())
}

/// What a click at a place on a picture or a grid tells: the cell and its value, or the
/// colour of the pixel.
pub fn cell_text(t: &Table, x: f64, y: f64) -> Option<String> {
    use og_engine::model::fmt_real;
    if let Some(g) = &t.grid {
        let (c, r, v) = g.cell_at(x, y)?;
        // A value of 32 bits is written with the digits it has, not those of 64 bits: 46.6,
        // not 46.5999984741.
        let show = |v: Option<f32>| v.filter(|v| !v.is_nan()).map_or("no value".to_string(), |v| fmt_real(v.to_string().parse::<f64>().unwrap_or(v as f64)));
        if g.bands.is_empty() {
            return Some(format!("column {}, row {}: {}", c, r, show(v)));
        }
        // Several bands: each with its number.
        let mut parts = vec![format!("band 1: {}", show(v))];
        for (i, b) in g.bands.iter().enumerate() {
            parts.push(format!("band {}: {}", i + 2, show(b.get(r * g.width + c).copied())));
        }
        return Some(format!("column {}, row {}: {}", c, r, parts.join(", ")));
    }
    let (c, r, rgba) = t.image.as_ref()?.colour_at(x, y)?;
    if rgba[3] == 0 {
        return Some(format!("column {}, row {}: no value", c, r));
    }
    Some(format!("column {}, row {}: red {}, green {}, blue {}", c, r, rgba[0], rgba[1], rgba[2]))
}

/// A stream that is being made ready in a thread.
struct Preparing {
    table: Arc<Table>,
    mercator: bool,
    progress: Arc<Building>,
    total: usize,
    rx: Receiver<Prepared>,
}

/// An image of many features, and what part of the world it shows.
struct Picture {
    view: RenderView,
    texture: egui::TextureHandle,
}

/// An image that is being drawn in a thread.
struct Drawing {
    view: RenderView,
    rx: Receiver<Option<Vec<u8>>>,
}

pub struct MapView {
    /// Whether a background map is shown.
    pub osm: bool,
    /// The background maps of the settings, and the one chosen by its name; None is
    /// OpenStreetMap.
    pub maps: Vec<og_engine::settings::BackgroundMap>,
    pub background: Option<String>,
    /// The map the tiles are fetched for now.
    tiles_map: Option<og_engine::settings::BackgroundMap>,
    /// A distance or an area being measured, by clicks on the map.
    pub measuring: Option<crate::maptools::Measuring>,
    /// Where the menu of the secondary button was opened, in the system of the map.
    menu_at: Option<[f64; 2]>,
    /// The latest coordinates copied from the menu.
    pub copied: Option<String>,
    /// The attribute the features are labelled with, and the attributes there are to choose.
    pub label_by: Option<String>,
    label_fields: Vec<String>,
    /// Rows picked besides the selected one, with Ctrl or Shift: drawn as picked too.
    pub also: Vec<usize>,
    /// A click with Ctrl or Shift, on a feature or beside: the table decides what it picks.
    pub multi_click: Option<(Option<usize>, egui::Modifiers)>,
    center: [f64; 2],
    /// Screen points per unit in the display system.
    scale: f64,
    /// Compression of x in degrees, so that the map is not stretched east–west.
    kx: f64,
    prepared: Option<Prepared>,
    preparing: Option<Preparing>,
    fit_pending: bool,
    /// The area the map was last fitted to. If it changes before the user has touched
    /// the map it is fitted again, so that the map follows when the panels are resized.
    fitted_rect: Rect,
    moved: bool,
    tiles: Option<Tiles>,
    tile_source: Option<String>,
    /// The image of many features, the one being drawn, and a count that tells a drawing
    /// that it is no longer wanted.
    picture: Option<Picture>,
    drawing: Option<Drawing>,
    generation: Arc<AtomicU64>,
    /// When the view last changed, so that an image is only drawn once it stands still.
    view_changed: f64,
    last_view: Option<RenderView>,
    /// The triangles of areas, for their fill, by feature.
    triangles: HashMap<u32, (Vec<[f32; 2]>, Vec<u32>)>,
    /// How many features were in view the last time: shapes, or an image.
    pub in_view: Option<(usize, bool)>,
    /// The image of the stream on the screen, by the address of the image.
    image_texture: Option<(usize, egui::TextureHandle)>,
    /// The value of the cell that was clicked, by the address of the stream: where it is
    /// on the map, and what it says.
    probe: Option<(usize, [f64; 2], String)>,
    /// Whether the map is shown in 3D, and how.
    pub three_d: bool,
    /// The features are drawn over a picture or a grid too, as when a View shows several
    /// streams together; otherwise they are only its outline.
    pub shapes_over_picture: bool,
    pub view3d: crate::view3d::View3d,
}

impl Default for MapView {
    fn default() -> Self {
        MapView {
            maps: Vec::new(),
            background: None,
            tiles_map: None,
            measuring: None,
            menu_at: None,
            copied: None,
            label_by: None,
            label_fields: Vec::new(),
            also: Vec::new(),
            multi_click: None,
            osm: true,
            center: [0.0, 0.0],
            scale: 1.0,
            kx: 1.0,
            prepared: None,
            preparing: None,
            fit_pending: true,
            fitted_rect: Rect::NOTHING,
            moved: false,
            tiles: None,
            tile_source: None,
            picture: None,
            drawing: None,
            generation: Arc::new(AtomicU64::new(0)),
            view_changed: 0.0,
            last_view: None,
            triangles: HashMap::new(),
            in_view: None,
            image_texture: None,
            probe: None,
            three_d: false,
            shapes_over_picture: false,
            view3d: Default::default(),
        }
    }
}

/// A line or a ring as it is drawn on the screen. Points that fall on the same spot are
/// dropped, and so are points where the line turns straight back. egui draws the corners of
/// a line as mitres, and at such points a mitre becomes a long spike. A ring is drawn as an
/// open line that ends where it began, since egui cuts the sharp corners of open lines but
/// not of closed ones.
pub fn screen_path(points: impl Iterator<Item = Pos2>, ring: bool) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = Vec::new();
    for p in points {
        if !p.x.is_finite() || !p.y.is_finite() {
            continue;
        }
        if out.last().is_some_and(|q| (*q - p).length_sq() < 0.25) {
            continue;
        }
        // A point between two that turns the line back on itself: the middle one goes.
        while out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let (u, v) = ((b - a).normalized(), (p - b).normalized());
            if u.dot(v) < -0.999 {
                out.pop();
            } else {
                break;
            }
        }
        if out.last().is_some_and(|q| (*q - p).length_sq() < 0.25) {
            continue;
        }
        out.push(p);
    }
    if ring && out.len() >= 3 {
        let first = out[0];
        if out.last().is_some_and(|q| (*q - first).length_sq() >= 0.25) {
            out.push(first);
        }
    }
    out
}

/// Makes a table ready for the map: reprojected to Web Mercator when the background map is
/// wanted, and flattened into a store. Returns None when it was cancelled.
fn prepare(table: Arc<Table>, want_mercator: bool, backend: &dyn Backend, progress: &Building) -> Option<Prepared> {
    // A grid of heights is shown as grey shaded relief, made small enough to be quick.
    let preview = match (&table.grid, &table.image) {
        (Some(grid), None) => {
            use og_engine::terrain::{self, Colours, Look, Shadows};
            let small = terrain::at_most(grid, 1200);
            let look = Look { colours: Colours::White, shadows: Shadows::None, sky_light: false, exaggeration: 2.0, ..Default::default() };
            Some(Arc::new(terrain::shade(&small, &look, None)))
        }
        _ => None,
    };
    let crs = table.crs;
    fn geoms(t: &Table) -> Vec<(usize, &geo::Geometry<f64>)> {
        t.features.iter().enumerate().filter_map(|(i, f)| f.geom.as_ref().map(|g| (i, g))).collect()
    }
    let (mut mercator, mut fallback) = (false, None);
    let mut store = None;
    if want_mercator {
        match crs {
            Some(WEB_MERCATOR) => mercator = true,
            Some(c) => {
                let transform = |xs: &mut [f64], ys: &mut [f64]| backend.transform(c, WEB_MERCATOR, xs, ys).map_err(|e| e.0);
                match Store::build(&mut geoms(&table).into_iter(), Some(&transform), progress) {
                    Ok(Some(s)) => {
                        mercator = true;
                        store = Some(s);
                    }
                    Ok(None) => return None,
                    Err(e) => fallback = Some(format!("The background map cannot be shown: {}", e)),
                }
            }
            None => {}
        }
    }
    let store = match store {
        Some(s) => s,
        None => match Store::build(&mut geoms(&table).into_iter(), None, progress) {
            Ok(Some(s)) => s,
            Ok(None) => return None,
            Err(e) => {
                fallback = Some(e);
                Store::build(&mut std::iter::empty(), None, progress).ok().flatten()?
            }
        },
    };
    let degrees = !mercator && is_geographic(Some(backend), crs).unwrap_or(false);
    let image_grid = table.image.as_ref().or(preview.as_ref()).and_then(|img| grid_of(img, crs, if mercator { Some(WEB_MERCATOR) } else { crs }, backend));
    Some(Prepared { table, mercator, store: Arc::new(store), degrees, fallback, image_grid, preview })
}

fn nice(x: f64) -> f64 {
    let p = 10f64.powf(x.log10().floor());
    let f = x / p;
    (if f < 1.5 {
        1.0
    } else if f < 3.5 {
        2.0
    } else if f < 7.5 {
        5.0
    } else {
        10.0
    }) * p
}

fn thousands(v: f64) -> String {
    let s = format!("{}", v.round() as i64);
    let (neg, digits) = s.strip_prefix('-').map_or((false, s.as_str()), |d| (true, d));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if neg {
        format!("-{}", out)
    } else {
        out
    }
}

/// The size of the image, written at the top left of the map.
/// With values, what the cell under the pointer holds, or how to see it.
fn notes_for_image(painter: &egui::Painter, rect: Rect, pal: &Palette, img: &og_engine::model::RasterImage, values: Option<Option<String>>) {
    let mut text = format!("Image {} × {} pixels", fmt_count(img.width as u64), fmt_count(img.height as u64));
    match values {
        Some(Some(v)) => text = format!("{} · {}", text, v),
        Some(None) => text.push_str(" · point at it for the value of a cell, click to mark it"),
        None => {}
    }
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.5), pal.ink);
    let r = Rect::from_min_size(rect.min + Vec2::new(8.0, 8.0), galley.size() + Vec2::new(10.0, 4.0));
    painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(pal.surface.r(), pal.surface.g(), pal.surface.b(), 215));
    painter.galley(r.min + Vec2::new(5.0, 2.0), galley, pal.ink);
}

impl MapView {
    pub fn fit(&mut self) {
        self.fit_pending = true;
    }

    /// Changes the tile server. Must be called before the map is drawn for the first time.
    pub fn set_tile_source(&mut self, url: impl Into<String>) {
        self.tile_source = Some(url.into());
        self.tiles = None;
    }

    /// True when the map is drawn over background tiles, false when it shows a grid.
    /// What the latest click on a picture or a grid told.
    pub fn probe_text(&self) -> Option<&str> {
        self.probe.as_ref().map(|p| p.2.as_str())
    }

    pub fn shows_tiles(&self) -> bool {
        self.prepared.as_ref().is_some_and(|p| p.mercator)
    }

    /// Forgets the data of the map without freeing it, as the program closes.
    pub fn let_go_of_data(&mut self) {
        std::mem::forget(self.prepared.take());
        std::mem::forget(self.preparing.take());
    }

    /// True while the stream is made ready, or an image of it is drawn.
    /// Where the map is on the screen, as of the latest fit to the data.
    pub fn fitted_rect(&self) -> Rect {
        self.fitted_rect
    }

    pub fn is_busy(&self) -> bool {
        self.preparing.is_some() || self.drawing.is_some()
    }

    /// True when the stream is an image, such as a drawn map, and it is shown.
    pub fn shows_image(&self) -> bool {
        self.image_texture.is_some() && self.prepared.as_ref().is_some_and(|p| p.image_grid.is_some())
    }

    /// True when the features in view are shown as an image.
    pub fn shows_picture(&self) -> bool {
        self.in_view.is_some_and(|v| v.1) && self.picture.is_some()
    }

    fn to_screen(&self, rect: Rect, c: [f64; 2]) -> Pos2 {
        Pos2::new(rect.center().x + ((c[0] - self.center[0]) * self.kx * self.scale) as f32, rect.center().y - ((c[1] - self.center[1]) * self.scale) as f32)
    }
    fn to_world(&self, rect: Rect, p: Pos2) -> [f64; 2] {
        [self.center[0] + (p.x - rect.center().x) as f64 / (self.kx * self.scale), self.center[1] - (p.y - rect.center().y) as f64 / self.scale]
    }

    fn fit_to(&mut self, rect: Rect) {
        let Some(p) = &self.prepared else { return };
        let Some(b) = p.store.bbox else { return };
        self.kx = if p.degrees { ((b[1] + b[3]) / 2.0).to_radians().cos().max(0.05) } else { 1.0 };
        let (w, h) = ((b[2] - b[0]) * self.kx, b[3] - b[1]);
        // More room at the bottom, where the scale bar and the notes are.
        let (side, top, bottom) = (44.0, 30.0, 58.0);
        let (aw, ah) = ((rect.width() - 2.0 * side).max(40.0) as f64, (rect.height() - top - bottom).max(40.0) as f64);
        self.scale = if w <= 0.0 && h <= 0.0 {
            // A single point: show about two kilometres around it.
            if p.degrees {
                20_000.0
            } else {
                aw.min(ah) / 2_000.0
            }
        } else {
            (aw / w.max(1e-12)).min(ah / h.max(1e-12))
        };
        self.center = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0 - ((bottom - top) / 2.0) as f64 / self.scale];
    }

    /// Starts making a table ready, at once for a small one and in a thread for a large one.
    fn start_preparing(&mut self, table: &Arc<Table>, want_mercator: bool, backend: &Arc<dyn Backend + Send + Sync>, ctx: &egui::Context) {
        if let Some(p) = self.preparing.take() {
            p.progress.cancel.store(true, Ordering::Relaxed);
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.picture = None;
        self.drawing = None;
        self.triangles.clear();
        self.last_view = None;
        let old_box = self.prepared.as_ref().filter(|p| p.mercator == want_mercator).and_then(|p| p.store.bbox);
        let total = table.len();
        if total <= AT_ONCE {
            let new = prepare(table.clone(), want_mercator, &**backend, &Building::default());
            if let Some(new) = new {
                // The same extent as before means the user just ran again. Keep the view then.
                if !(old_box.is_some() && old_box == new.store.bbox) {
                    self.fit_pending = true;
                }
                self.prepared = Some(new);
            }
            return;
        }
        let (tx, rx) = channel();
        let progress = Arc::new(Building::default());
        let (t, b, p, c) = (table.clone(), backend.clone(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(done) = prepare(t, want_mercator, &*b, &p) {
                let _ = tx.send(done);
                c.request_repaint();
            }
        });
        crate::app::drop_later(self.prepared.take());
        self.fit_pending = true;
        crate::app::drop_later(self.preparing.take());
        self.preparing = Some(Preparing { table: table.clone(), mercator: want_mercator, progress, total, rx });
    }

    /// Draws the map in all the space that is left. `selected` is the feature selected
    /// in the table, and changes when the user clicks on the map.
    pub fn show(&mut self, ui: &mut Ui, table: Option<&Arc<Table>>, selected: &mut Option<usize>, backend: &Arc<dyn Backend + Send + Sync>) {
        let pal = Palette::from_ui(ui);
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, pal.surface);
        painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, pal.line));

        // The background map is only used if the server answers.
        let source = self.tile_source.clone();
        // A background map of one's own, when one is chosen and still in the settings.
        let wanted = self.background.as_ref().and_then(|n| self.maps.iter().find(|m| &m.name == n)).cloned();
        if wanted != self.tiles_map {
            self.tiles = None;
            self.tiles_map = wanted.clone();
        }
        let tiles = self.tiles.get_or_insert_with(|| Tiles::from_source(ui.ctx(), source, wanted));
        tiles.poll(ui.ctx());
        if self.osm && tiles.should_retry() {
            tiles.probe();
        }
        let net = tiles.net();
        let want_mercator = self.osm && net != Net::Offline;

        // A picture without coordinates, such as a 3D view, is shown as it is, to fit.
        if let Some(img) = table.filter(|t| t.crs.is_none()).and_then(|t| t.image.as_ref()) {
            self.prepared = None;
            self.preparing = None;
            self.picture = None;
            self.in_view = None;
            let key = Arc::as_ptr(img) as usize;
            if self.image_texture.as_ref().is_none_or(|t| t.0 != key) {
                let texture = ui.ctx().load_texture("map-image", image_for_screen(img), egui::TextureOptions::LINEAR);
                self.image_texture = Some((key, texture));
            }
            let tex = self.image_texture.as_ref().expect("made above").1.id();
            let room = rect.shrink(8.0);
            let k = (room.width() / img.width as f32).min(room.height() / img.height as f32);
            let r = Rect::from_center_size(room.center(), egui::vec2(img.width as f32 * k, img.height as f32 * k));
            painter.image(tex, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            return;
        }
        // Shapes without a coordinate system, such as a 3D model of a chair, are drawn as they
        // are, in their own units and without a background map.
        let Some(table) = table.filter(|t| t.crs.is_some() || t.features.iter().any(|f| f.geom.is_some())) else {
            self.prepared = None;
            self.preparing = None;
            self.picture = None;
            self.in_view = None;
            let msg = if table.is_some() { "This stream has no geometry." } else { "" };
            painter.text(rect.center(), Align2::CENTER_CENTER, msg, egui::FontId::proportional(13.0), pal.muted);
            return;
        };

        let want_mercator = want_mercator && table.crs.is_some();
        // A stream made ready in a thread is taken in when it is done.
        if let Some(p) = &self.preparing {
            if let Ok(done) = p.rx.try_recv() {
                crate::app::drop_later(self.prepared.replace(done));
                self.preparing = None;
            }
        }
        // Ready when it is the same stream, prepared for the background that is wanted, or
        // for the grid because the background could not be used.
        let ready = self.prepared.as_ref().is_some_and(|p| Arc::ptr_eq(&p.table, table) && (p.mercator == want_mercator || p.fallback.is_some()));
        let coming = self.preparing.as_ref().is_some_and(|p| Arc::ptr_eq(&p.table, table) && p.mercator == want_mercator);
        if !ready && !coming {
            self.start_preparing(table, want_mercator, backend, ui.ctx());
        }
        if let Some(p) = &self.preparing {
            // A grid while the stream is made ready, and how far it has come.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
            let done = p.progress.done.load(Ordering::Relaxed).min(p.total);
            let text = format!("Preparing the map: {} of {} features", fmt_count(done as u64), fmt_count(p.total as u64));
            painter.text(rect.center(), Align2::CENTER_CENTER, text, egui::FontId::proportional(13.0), pal.muted);
            return;
        }
        if self.fit_pending || (!self.moved && rect != self.fitted_rect) {
            self.fit_to(rect);
            self.fit_pending = false;
            self.fitted_rect = rect;
            self.moved = false;
        }
        let Some(prep) = self.prepared.as_ref() else { return };
        if prep.store.bbox.is_none() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "No geometries to draw.", egui::FontId::proportional(13.0), pal.muted);
            return;
        }
        let mercator = prep.mercator;
        // The system the map is drawn in, for the coordinates and the measuring.
        let shown_crs = if mercator { Some(WEB_MERCATOR) } else { table.crs };
        let names = table.field_names();
        if names != self.label_fields {
            if self.label_by.as_ref().is_some_and(|l| !names.contains(l)) {
                self.label_by = None;
            }
            self.label_fields = names;
        }

        // Pan, zoom, pick.
        if resp.dragged() {
            let d = resp.drag_delta();
            self.center[0] -= d.x as f64 / (self.kx * self.scale);
            self.center[1] += d.y as f64 / self.scale;
            self.moved = true;
        }
        if let Some(pos) = resp.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll as f64 * 0.0035).exp() * pinch as f64;
            if (factor - 1.0).abs() > 1e-6 {
                let before = self.to_world(rect, pos);
                self.scale = (self.scale * factor).clamp(1e-9, 1e9);
                let after = self.to_world(rect, pos);
                self.center = [self.center[0] + before[0] - after[0], self.center[1] + before[1] - after[1]];
                self.moved = true;
            }
        }
        let prep = self.prepared.as_ref().expect("prepared above");
        let store = prep.store.clone();
        let view = {
            let (a, b) = (self.to_world(rect, rect.left_bottom()), self.to_world(rect, rect.right_top()));
            [a[0], a[1], b[0], b[1]]
        };

        // Background: tiles or grid.
        if mercator {
            let z = tiles::zoom_for(self.scale);
            let (x0, y0, x1, y1) = tiles::tile_range(z, view);
            if (x1 - x0 + 1) as u64 * (y1 - y0 + 1) as u64 <= 80 {
                let tint = if ui.visuals().dark_mode { Color32::from_gray(150) } else { Color32::WHITE };
                let mut wanted = Vec::new();
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        let b = tiles::tile_bounds(z, x, y);
                        wanted.push(((z, x, y), Rect::from_min_max(self.to_screen(rect, [b[0], b[3]]), self.to_screen(rect, [b[2], b[1]]))));
                    }
                }
                let tiles = self.tiles.as_mut().expect("created above");
                for (key, r) in wanted {
                    if let Some(tex) = tiles.get(key) {
                        painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                    }
                }
            }
        } else {
            let degrees = prep.degrees;
            self.draw_grid(&painter, rect, &pal, degrees, view);
        }

        // A stream that carries an image, such as a map that has been drawn: the image is
        // shown, laid on the background where it belongs.
        if let (Some(img), Some(grid)) = (table.image.as_ref().or(prep.preview.as_ref()), prep.image_grid.clone()) {
            let key = Arc::as_ptr(img) as usize;
            if self.image_texture.as_ref().is_none_or(|t| t.0 != key) {
                let texture = ui.ctx().load_texture("map-image", image_for_screen(img), egui::TextureOptions::LINEAR);
                self.image_texture = Some((key, texture));
            }
            let tex = self.image_texture.as_ref().expect("made above").1.id();
            let n = IMAGE_GRID;
            let mut mesh = Mesh::with_texture(tex);
            for (k, c) in grid.iter().enumerate() {
                let (i, j) = (k % (n + 1), k / (n + 1));
                let uv = Pos2::new(i as f32 / n as f32, j as f32 / n as f32);
                mesh.vertices.push(egui::epaint::Vertex { pos: self.to_screen(rect, *c), uv, color: Color32::WHITE });
            }
            for j in 0..n as u32 {
                for i in 0..n as u32 {
                    let a = j * (n as u32 + 1) + i;
                    let (b, c, d) = (a + 1, a + n as u32 + 1, a + n as u32 + 2);
                    mesh.indices.extend_from_slice(&[a, b, d, a, d, c]);
                }
            }
            painter.add(Shape::mesh(mesh));
            self.in_view = None;
            // The value under the pointer, read from the cells as it moves.
            let values = (table.grid.is_some() || table.image.is_some()).then(|| {
                let pos = resp.hover_pos().filter(|p| rect.contains(*p))?;
                let at = self.to_world(rect, pos);
                let (mut xs, mut ys) = (vec![at[0]], vec![at[1]]);
                if let (true, Some(c)) = (mercator, table.crs) {
                    if c != WEB_MERCATOR {
                        backend.transform(WEB_MERCATOR, c, &mut xs, &mut ys).ok()?;
                    }
                }
                cell_text(table, xs[0], ys[0])
            });
            notes_for_image(&painter, rect, &pal, img, values);
        }
        // The features: as shapes when few are in view, as an image when many are.
        let found = if (table.image.is_some() || prep.preview.is_some()) && !self.shapes_over_picture {
            InView::Few(Vec::new())
        } else {
            store.in_view(view, SHAPE_LIMIT)
        };
        match &found {
            InView::Few(list) => {
                self.in_view = Some((list.len(), false));
                self.generation.fetch_add(1, Ordering::Relaxed);
                self.drawing = None;
                self.draw_shapes(&painter, rect, &pal, &store, list, *selected);
                self.draw_labels(&painter, rect, &pal, &store, list, table);
            }
            InView::Many(n) => {
                self.in_view = Some((*n, true));
                self.draw_picture(ui, &painter, rect, &pal, &store);
                // The selected feature on top, as a shape.
                if selected.is_some() || !self.also.is_empty() {
                    let on: std::collections::HashSet<usize> = self.also.iter().copied().chain(*selected).collect();
                    let mine: Vec<u32> = store.feats.iter().enumerate().filter(|(_, f)| on.contains(&(f.row as usize))).map(|(i, _)| i as u32).collect();
                    self.draw_shapes(&painter, rect, &pal, &store, &mine, *selected);
                }
            }
        }

        // Scale bar. In Web Mercator a metre on the ground shrinks with the cosine of the latitude.
        let prep = self.prepared.as_ref().expect("prepared above");
        let meters_per_px = if mercator {
            let lat = (2.0 * (self.center[1] / 6_378_137.0).exp().atan() - std::f64::consts::FRAC_PI_2).cos();
            lat / self.scale
        } else if prep.degrees {
            111_320.0 / self.scale
        } else {
            1.0 / self.scale
        };
        let len = nice(90.0 * meters_per_px);
        let w = (len / meters_per_px) as f32;
        let (bx, by) = (rect.right() - 14.0 - w, rect.bottom() - 26.0);
        let label = if len >= 1000.0 { format!("{} km", fmt_real(len / 1000.0)) } else { format!("{} m", fmt_real(len)) };
        let plate = Rect::from_min_max(Pos2::new(bx - 6.0, by - 20.0), Pos2::new(bx + w + 6.0, by + 6.0));
        painter.rect_filled(plate, 3.0, Color32::from_rgba_unmultiplied(pal.surface.r(), pal.surface.g(), pal.surface.b(), 215));
        painter.line(vec![Pos2::new(bx, by - 4.0), Pos2::new(bx, by), Pos2::new(bx + w, by), Pos2::new(bx + w, by - 4.0)], Stroke::new(1.5, pal.ink));
        painter.text(Pos2::new(bx + w, by - 6.0), Align2::RIGHT_BOTTOM, label, theme::mono(11.0), pal.ink);

        // What is being measured: the points clicked, the line between them, and the result.
        if let Some(m) = &self.measuring {
            let pts: Vec<Pos2> = m.points.iter().map(|p| self.to_screen(rect, *p)).collect();
            let ink = Color32::from_rgb(214, 40, 110);
            if pts.len() >= 2 {
                let mut line = pts.clone();
                if m.what == crate::maptools::Measure::Area && pts.len() >= 3 {
                    line.push(pts[0]);
                    let fill = Color32::from_rgba_unmultiplied(214, 40, 110, 40);
                    painter.add(Shape::convex_polygon(pts.clone(), fill, Stroke::NONE));
                }
                painter.add(Shape::line(line, Stroke::new(2.0, ink)));
            }
            for p in &pts {
                painter.circle(*p, 4.0, pal.surface, Stroke::new(2.0, ink));
            }
            let text = format!("{}   ·   Escape ends", crate::maptools::result(m, shown_crs, &**backend));
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.5), pal.ink);
            let r = Rect::from_min_size(rect.left_top() + Vec2::new(8.0, 8.0), galley.size() + Vec2::new(14.0, 8.0));
            painter.rect(r, 3.0, pal.surface, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            painter.galley(r.min + Vec2::new(7.0, 4.0), galley, pal.ink);
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.measuring = None;
            }
        }

        // The menu of the secondary button: the coordinates of the point, and measuring.
        if resp.secondary_clicked() {
            self.menu_at = resp.interact_pointer_pos().map(|p| self.to_world(rect, p));
        }
        let (menu_at, data_crs) = (self.menu_at, table.crs);
        let mut copied = None;
        let mut start: Option<crate::maptools::Measure> = None;
        let mut stop_measuring = false;
        let measuring = self.measuring.is_some();
        resp.context_menu(|ui| {
            if let Some(at) = menu_at {
                for (label, text) in crate::maptools::coordinates(at, shown_crs, data_crs, &**backend) {
                    if ui.button(format!("Copy {}", label)).clicked() {
                        ui.ctx().copy_text(text.clone());
                        copied = Some(text);
                        ui.close();
                    }
                }
                ui.separator();
            }
            if ui.button("Measure a distance").on_hover_text("Click the points on the map; Escape ends").clicked() {
                start = Some(crate::maptools::Measure::Distance);
                ui.close();
            }
            if ui.button("Measure an area").on_hover_text("Click the corners on the map; Escape ends").clicked() {
                start = Some(crate::maptools::Measure::Area);
                ui.close();
            }
            if measuring && ui.button("Stop measuring").clicked() {
                stop_measuring = true;
                ui.close();
            }
        });
        if copied.is_some() {
            self.copied = copied;
        }
        if let Some(what) = start {
            self.measuring = Some(crate::maptools::Measuring { what, points: Vec::new() });
        }
        if stop_measuring {
            self.measuring = None;
        }

        // Source, how the features are drawn, and errors, at the bottom left.
        let mut notes: Vec<String> = Vec::new();
        match &self.tiles_map {
            Some(m) => {
                if mercator && !m.attribution.trim().is_empty() {
                    notes.push(m.attribution.trim().to_string());
                }
                if self.osm && net == Net::Offline {
                    notes.push(format!("{} cannot be reached, or does not answer with pictures. The map shows a grid.", m.name));
                }
            }
            None => {
                if mercator && self.tiles.as_ref().is_some_and(|t| t.is_default_source()) {
                    notes.push("© OpenStreetMap contributors".into());
                }
                if self.osm && net == Net::Offline {
                    notes.push("OpenStreetMap cannot be reached. The map shows a grid.".into());
                }
            }
        }
        if let Some(f) = &prep.fallback {
            notes.push(f.clone());
        }
        if let InView::Many(_) = found {
            let total = prep.store.feats.len();
            if self.drawing.is_some() {
                notes.push(format!("Drawing {} features…", fmt_count(total as u64)));
            } else {
                notes.push(format!("{} features, drawn as an image. Zoom in to see them one by one.", fmt_count(total as u64)));
            }
        }
        let mut y = rect.bottom() - 8.0;
        for n in notes.iter().rev() {
            let galley = painter.layout_no_wrap(n.clone(), egui::FontId::proportional(11.5), pal.ink);
            let r = Rect::from_min_size(Pos2::new(rect.left() + 8.0, y - galley.size().y - 4.0), galley.size() + Vec2::new(10.0, 4.0));
            painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(pal.surface.r(), pal.surface.g(), pal.surface.b(), 215));
            painter.galley(r.min + Vec2::new(5.0, 2.0), galley, pal.ink);
            y = r.top() - 3.0;
        }

        // The value of the clicked cell, with a mark where it is.
        let here = Some(Arc::as_ptr(table) as *const u8 as usize);
        if let Some((key, at, text)) = self.probe.clone().filter(|p| Some(p.0) == here) {
            let _ = key;
            let p = self.to_screen(rect, at);
            if rect.contains(p) {
                painter.circle(p, 4.5, pal.accent, Stroke::new(1.5, pal.surface));
                let galley = painter.layout_no_wrap(text, egui::FontId::monospace(12.0), pal.ink);
                let mut r = Rect::from_min_size(p + Vec2::new(10.0, -galley.size().y - 10.0), galley.size() + Vec2::new(12.0, 6.0));
                if r.right() > rect.right() - 4.0 {
                    r = r.translate(Vec2::new(-(r.width() + 20.0), 0.0));
                }
                if r.top() < rect.top() + 4.0 {
                    r = r.translate(Vec2::new(0.0, r.height() + 20.0));
                }
                painter.rect(r, 3.0, pal.surface, Stroke::new(1.0, pal.line), egui::StrokeKind::Inside);
                painter.galley(r.min + Vec2::new(6.0, 3.0), galley, pal.ink);
            }
        }

        // While measuring, a click is a point of what is measured.
        let measured = match (&mut self.measuring, resp.clicked(), resp.interact_pointer_pos()) {
            (Some(m), true, Some(pos)) => {
                let at = [
                    self.center[0] + (pos.x - rect.center().x) as f64 / (self.kx * self.scale),
                    self.center[1] - (pos.y - rect.center().y) as f64 / self.scale,
                ];
                m.points.push(at);
                true
            }
            _ => false,
        };
        if resp.clicked() && !measured {
            if let Some(pos) = resp.interact_pointer_pos() {
                let hit = self.pick(rect, pos, &store);
                let keys = ui.input(|i| i.modifiers);
                if keys.command || keys.shift {
                    self.multi_click = Some((hit, keys));
                } else {
                    *selected = if hit == *selected { None } else { hit };
                }
                // On a picture or a grid, the click tells the value of the cell under it.
                self.probe = None;
                if let (Some(t), Some(key)) = (Some(table).filter(|t| t.grid.is_some() || t.image.is_some()), here) {
                    let at = self.to_world(rect, pos);
                    let (mut xs, mut ys) = (vec![at[0]], vec![at[1]]);
                    let ok = match (mercator, t.crs) {
                        (true, Some(c)) if c != WEB_MERCATOR => backend.transform(WEB_MERCATOR, c, &mut xs, &mut ys).is_ok(),
                        _ => true,
                    };
                    if let Some(text) = ok.then(|| cell_text(t, xs[0], ys[0])).flatten() {
                        self.probe = Some((key, at, text));
                    }
                }
            }
        }
    }

    /// Labels of the features in view by the chosen attribute, at the middle of each, left
    /// out where they would lie on one already placed.
    fn draw_labels(&self, painter: &egui::Painter, rect: Rect, pal: &Palette, store: &Store, list: &[u32], table: &Table) {
        let Some(at) = self.label_by.as_ref().and_then(|l| table.field_index(l)) else { return };
        let o = store.origin;
        let mut placed: Vec<Rect> = Vec::new();
        for &i in list.iter().take(3000) {
            let f = &store.feats[i as usize];
            let Some(feature) = table.features.get(f.row as usize) else { continue };
            let text = feature.attrs.get(at).map(|v| if v.is_null() { String::new() } else { v.to_text() }).unwrap_or_default();
            let text: String = text.chars().take(40).collect();
            if text.trim().is_empty() {
                continue;
            }
            let b = f.bbox;
            let mid = [o[0] + ((b[0] + b[2]) / 2.0) as f64, o[1] + ((b[1] + b[3]) / 2.0) as f64];
            let p = self.to_screen(rect, mid);
            if !rect.contains(p) {
                continue;
            }
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.5), pal.ink);
            let r = Rect::from_center_size(p + Vec2::new(0.0, if f.kind == Kind::Point { -12.0 } else { 0.0 }), galley.size() + Vec2::new(6.0, 2.0));
            if placed.iter().any(|q| q.intersects(r)) {
                continue;
            }
            painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(pal.surface.r(), pal.surface.g(), pal.surface.b(), 190));
            painter.galley(r.min + Vec2::new(3.0, 1.0), galley, pal.ink);
            placed.push(r);
        }
    }

    /// Draws features as shapes: areas filled and outlined, lines, and points.
    fn draw_shapes(&mut self, painter: &egui::Painter, rect: Rect, pal: &Palette, store: &Store, list: &[u32], selected: Option<usize>) {
        let o = store.origin;
        // Store coordinates to the screen, without holding on to self.
        let (cx, cy, sx, sy) = (self.center[0], self.center[1], self.kx * self.scale, self.scale);
        let mid = rect.center();
        let at = move |c: [f32; 2]| Pos2::new(mid.x + ((o[0] + c[0] as f64 - cx) * sx) as f32, mid.y - ((o[1] + c[1] as f64 - cy) * sy) as f32);
        let stroke_color = pal.xform;
        let mut fills = Mesh::default();
        let mut outlines: Vec<Shape> = Vec::new();
        let mut points: Vec<(Pos2, bool)> = Vec::new();
        let many = list.len() > 4_000;
        let also: std::collections::HashSet<usize> = self.also.iter().copied().collect();
        // The triangles are kept between frames. Too many kept are let go.
        if self.triangles.len() > 4 * SHAPE_LIMIT {
            self.triangles.clear();
        }
        for &i in list {
            let f = &store.feats[i as usize];
            let on = selected == Some(f.row as usize) || also.contains(&(f.row as usize));
            let color = if on { pal.accent } else { stroke_color };
            match f.kind {
                Kind::Area => {
                    let fill = if on { Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 80) } else { pal.map_fill() };
                    let (verts, tris) = self.triangles.entry(i).or_insert_with(|| store.triangles(f));
                    let base = fills.vertices.len() as u32;
                    for v in verts.iter() {
                        fills.colored_vertex(at(*v), fill);
                    }
                    fills.indices.extend(tris.iter().map(|t| base + t));
                    for p in f.first..f.end {
                        let path = screen_path(store.part(p as usize).iter().map(|c| at(*c)), true);
                        if path.len() >= 2 {
                            outlines.push(Shape::line(path, Stroke::new(if on { 2.5 } else { 1.5 }, color)));
                        }
                    }
                }
                Kind::Line => {
                    for p in f.first..f.end {
                        let path = screen_path(store.part(p as usize).iter().map(|c| at(*c)), false);
                        if path.len() >= 2 {
                            outlines.push(Shape::line(path, Stroke::new(if on { 3.5 } else { 2.0 }, color)));
                        }
                    }
                }
                Kind::Point => {
                    for p in f.first..f.end {
                        points.extend(store.part(p as usize).iter().map(|c| (at(*c), on)));
                    }
                }
            }
        }
        if !fills.is_empty() {
            painter.add(Shape::mesh(fills));
        }
        painter.extend(outlines);
        for (p, on) in &points {
            if !rect.expand(8.0).contains(*p) {
                continue;
            }
            if *on {
                painter.circle(*p, 6.5, pal.accent, Stroke::new(1.5, pal.surface));
            } else if many {
                painter.circle_filled(*p, 2.5, stroke_color);
            } else {
                painter.circle(*p, 4.5, stroke_color, Stroke::new(1.5, pal.surface));
            }
        }
    }

    /// Draws many features as an image. The image from before is shown where it belongs
    /// while the map moves, and a new one is drawn in a thread once the view stands still.
    fn draw_picture(&mut self, ui: &Ui, painter: &egui::Painter, rect: Rect, pal: &Palette, store: &Arc<Store>) {
        let ppp = ui.ctx().pixels_per_point() as f64;
        let wanted = RenderView {
            w: (rect.width() as f64 * ppp).round().max(1.0) as usize,
            h: (rect.height() as f64 * ppp).round().max(1.0) as usize,
            center: self.to_world(rect, rect.center()),
            px_per_unit: self.scale * ppp,
            kx: self.kx,
        };
        let now = ui.input(|i| i.time);
        if self.last_view != Some(wanted) {
            self.last_view = Some(wanted);
            self.view_changed = now;
        }
        // A finished image is taken in.
        if let Some(d) = &self.drawing {
            if let Ok(done) = d.rx.try_recv() {
                let view = d.view;
                self.drawing = None;
                if let Some(rgba) = done {
                    let image = egui::ColorImage::from_rgba_unmultiplied([view.w, view.h], &rgba);
                    let texture = ui.ctx().load_texture("map-picture", image, egui::TextureOptions::LINEAR);
                    self.picture = Some(Picture { view, texture });
                }
            }
        }
        let current = self.picture.as_ref().is_some_and(|p| p.view == wanted) || self.drawing.as_ref().is_some_and(|d| d.view == wanted);
        if !current {
            // The view has stood still for a moment, or there is nothing to show yet.
            let still = now - self.view_changed > 0.12 || self.picture.is_none();
            if still {
                let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
                let (tx, rx) = channel();
                let (s, g, c) = (store.clone(), self.generation.clone(), ui.ctx().clone());
                let rgba = |c: Color32| [c.r(), c.g(), c.b(), c.a()];
                let fill = pal.xform;
                let colors = Colors { stroke: rgba(pal.xform), fill: [fill.r(), fill.g(), fill.b(), 70], point: rgba(pal.xform) };
                let thickness = ppp.round().max(1.0) as usize;
                std::thread::spawn(move || {
                    let image = mapdata::rasterize(&s, wanted, colors, thickness, &g, generation);
                    if g.load(Ordering::Relaxed) == generation {
                        let _ = tx.send(image);
                        c.request_repaint();
                    }
                });
                self.drawing = Some(Drawing { view: wanted, rx });
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
        if self.drawing.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(p) = &self.picture {
            let b = p.view.world();
            let r = Rect::from_min_max(self.to_screen(rect, [b[0], b[3]]), self.to_screen(rect, [b[2], b[1]]));
            painter.image(p.texture.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect, pal: &Palette, degrees: bool, view: [f64; 4]) {
        let (sx, sy) = (self.scale * self.kx, self.scale);
        let (step_x, step_y) = (nice(120.0 / sx), nice(80.0 / sy));
        let fmt = |v: f64, step: f64, axis: &str| {
            if degrees {
                let d = (-step.log10().floor()).max(0.0) as usize;
                format!("{:.*}°", d, v)
            } else {
                format!("{} {}", thousands(v), axis)
            }
        };
        let stroke = Stroke::new(1.0, pal.grid);
        let font = theme::mono(10.5);
        let mut x = (view[0] / step_x).ceil() * step_x;
        while x <= view[2] {
            let px = self.to_screen(rect, [x, self.center[1]]).x.round() + 0.5;
            painter.line_segment([Pos2::new(px, rect.top()), Pos2::new(px, rect.bottom())], stroke);
            if px > rect.left() + 70.0 && px < rect.right() - 60.0 {
                painter.text(Pos2::new(px + 3.0, rect.top() + 4.0), Align2::LEFT_TOP, fmt(x, step_x, "E"), font.clone(), pal.muted);
            }
            x += step_x;
        }
        let mut y = (view[1] / step_y).ceil() * step_y;
        while y <= view[3] {
            let py = self.to_screen(rect, [self.center[0], y]).y.round() + 0.5;
            painter.line_segment([Pos2::new(rect.left(), py), Pos2::new(rect.right(), py)], stroke);
            if py > rect.top() + 24.0 && py < rect.bottom() - 40.0 {
                painter.text(Pos2::new(rect.left() + 4.0, py - 2.0), Align2::LEFT_BOTTOM, fmt(y, step_y, "N"), font.clone(), pal.muted);
            }
            y += step_y;
        }
    }

    /// The feature under a point on the screen. Points and lines go before areas.
    fn pick(&self, rect: Rect, pos: Pos2, store: &Store) -> Option<usize> {
        let w = self.to_world(rect, pos);
        let at = [(w[0] - store.origin[0]) as f32, (w[1] - store.origin[1]) as f32];
        // Seven pixels around the pointer, in store units.
        let tol = (7.0 / self.scale) as f32;
        store.pick(at, tol).map(|r| r as usize)
    }

    /// The buttons above the map. Drawn from right to left.
    pub fn controls(&mut self, ui: &mut Ui) {
        if self.three_d {
            ui.checkbox(&mut self.three_d, "3D").on_hover_text("Seen from an angle, with the heights. Drag to turn it");
            ui.add(egui::DragValue::new(&mut self.view3d.exaggeration).range(0.1..=50.0).speed(0.05).max_decimals(1).prefix("heights × "))
                .on_hover_text("How much the heights are drawn larger than they are, for flat land");
            if ui
                .button("Zoom to data")
                .on_hover_text("Show all of it again, from the side it was first seen from. A double click in the view does the same")
                .clicked()
            {
                self.view3d.reset();
            }
            return;
        }
        ui.checkbox(&mut self.three_d, "3D").on_hover_text("Seen from an angle, with the heights of the features or of a grid. Drag to turn it");
        // The background map: none, OpenStreetMap, or one of one's own from the settings.
        let shown = if !self.osm { "No background".to_string() } else { self.background.clone().unwrap_or_else(|| "OpenStreetMap".into()) };
        egui::ComboBox::from_id_salt("map-background")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(!self.osm, "No background").clicked() {
                    self.osm = false;
                }
                if ui.selectable_label(self.osm && self.background.is_none(), "OpenStreetMap").clicked() {
                    self.osm = true;
                    self.background = None;
                }
                for m in &self.maps {
                    if ui.selectable_label(self.osm && self.background.as_deref() == Some(m.name.as_str()), &m.name).clicked() {
                        self.osm = true;
                        self.background = Some(m.name.clone());
                    }
                }
                if self.maps.is_empty() {
                    ui.label(egui::RichText::new("Maps of your own, from WMS, WMTS or tiles, are added under Settings → Background maps").small());
                }
            })
            .response
            .on_hover_text("Background map when the network can be reached. Otherwise a coordinate grid is shown.");
        // Labels from an attribute.
        let shown = self.label_by.clone().unwrap_or_else(|| "No labels".into());
        egui::ComboBox::from_id_salt("map-labels")
            .selected_text(shown)
            .width(110.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(self.label_by.is_none(), "No labels").clicked() {
                    self.label_by = None;
                }
                for name in self.label_fields.clone() {
                    if ui.selectable_label(self.label_by.as_deref() == Some(name.as_str()), &name).clicked() {
                        self.label_by = Some(name);
                    }
                }
            })
            .response
            .on_hover_text("The features labelled with the values of an attribute, where there is room");
        if ui.button("Zoom to data").on_hover_text("Show all features").clicked() {
            self.fit();
        }
        if ui.button("−").on_hover_text("Zoom out").clicked() {
            self.scale /= 1.5;
            self.moved = true;
        }
        if ui.button("+").on_hover_text("Zoom in").clicked() {
            self.scale *= 1.5;
            self.moved = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::screen_path;
    use eframe::egui::Pos2;

    #[test]
    fn lines_on_the_screen_have_no_turns_that_make_spikes() {
        let p = |x: f32, y: f32| Pos2::new(x, y);
        // A ring with its closing point, a point repeated three times, and a spike out and back.
        let ring = [p(0., 0.), p(10., 0.), p(10., 0.), p(10.1, 0.1), p(10., 10.), p(20., 10.), p(10., 10.), p(0., 10.), p(0., 0.)];
        let path = screen_path(ring.into_iter(), true);
        assert_eq!(path, [p(0., 0.), p(10., 0.), p(10., 10.), p(0., 10.), p(0., 0.)]);
        // A line keeps its ends, and a ring without its closing point gets one.
        assert_eq!(screen_path([p(0., 0.), p(5., 5.)].into_iter(), false), [p(0., 0.), p(5., 5.)]);
        assert_eq!(screen_path([p(0., 0.), p(5., 0.), p(5., 5.)].into_iter(), true).last(), Some(&p(0., 0.)));
        assert!(screen_path([p(f32::NAN, 0.), p(1., 1.)].into_iter(), false).len() == 1);
    }
}
