//! The map under the canvas in 3D: the features of a stream with their heights, and a grid
//! of heights as a surface, seen from an angle that the user turns by dragging. The picture
//! is a parallel projection, as on a model under a lamp, coloured by the height.

use crate::theme::Palette;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use og_engine::model::{fmt_count, fmt_real, Table};
use std::collections::HashMap;
use std::sync::Arc;

/// The most coordinates that are drawn, so that turning the view stays smooth.
const MOST_COORDS: usize = 400_000;
/// A grid is drawn with at most this many cells on a side.
const GRID_SIDE: usize = 160;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Point,
    Line,
    Area,
}

struct Item {
    kind: Kind,
    feature: usize,
    /// x and y in metres from the middle, z in the units of the heights.
    pts: Vec<[f32; 3]>,
    /// For lines and areas: where each part or ring ends in `pts`.
    ends: Vec<usize>,
    /// For areas: triangles, as indices into `pts`.
    tris: Vec<u32>,
    flat: bool,
}

/// A grid of heights to draw: its width and height in cells, the heights, the middle of the
/// first cell, and the size of a cell.
type GridMesh = (usize, usize, Vec<f32>, [f32; 2], [f32; 2]);

struct Prepared {
    key: usize,
    items: Vec<Item>,
    /// The heights of a grid, at most GRID_SIDE on a side, with its corner and cell size in
    /// the same metres as the items.
    grid: Option<GridMesh>,
    /// Half the width and height of what is shown, and the lowest and highest height.
    half: [f32; 2],
    z: [f32; 2],
    with_heights: usize,
    left_out: usize,
    /// The smallest triangles of a model left out, when it has more than MOST_MODEL.
    tris_left_out: usize,
    /// The triangles of a 3D model, such as one read from OBJ: corners, colour (0 to 255)
    /// and the object, which is the feature, they belong to.
    model: Vec<([[f32; 3]; 3], [f32; 3], u32)>,
}

/// The most triangles of a model that are drawn. More are thinned out.
const MOST_MODEL: usize = 1_500_000;

pub struct View3d {
    /// Where the view is from, in degrees clockwise from north, and how high above the
    /// horizon, from 5 to 90.
    pub azimuth: f32,
    pub elevation: f32,
    pub exaggeration: f32,
    zoom: f32,
    pan: Vec2,
    prepared: Option<Prepared>,
    /// The surfaces drawn with a depth buffer, as a picture, and for each of its pixels the
    /// feature it shows, to pick by a click; with what they were drawn for.
    drawn: Option<Drawn>,
}

struct Drawn {
    key: Vec<u32>,
    texture: egui::TextureHandle,
    ids: Vec<u32>,
    size: [usize; 2],
}

/// No feature, in the buffer of features.
const NONE: u32 = u32::MAX;

/// Draws triangles with a depth buffer: the nearest at each pixel wins. The corners are in
/// pixels with their depth, larger farther away. Gives the colours, transparent where there
/// is nothing, and the feature of each pixel.
/// A triangle to draw: its corners in pixels with their depth, its colour and its feature.
type PixelFace = ([(f32, f32, f32); 3], Color32, u32);
/// A triangle on the screen: its corners with their depth, its colour and its feature.
type ScreenFace = ([(Pos2, f32); 3], Color32, u32);
/// A point to draw as a dot: where in pixels, its depth, its radius in pixels, its colour and
/// its feature.
type PixelDot = ([f32; 3], f32, Color32, u32);

fn rasterize(w: usize, h: usize, faces: &[PixelFace], dots: &[PixelDot]) -> (Vec<Color32>, Vec<u32>) {
    let mut colour = vec![Color32::TRANSPARENT; w * h];
    let mut depth = vec![f32::INFINITY; w * h];
    let mut ids = vec![NONE; w * h];
    for (t, col, id) in faces {
        let [a, b, c] = *t;
        let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if area.abs() < 1e-6 {
            continue;
        }
        let x0 = a.0.min(b.0).min(c.0).floor().max(0.0) as usize;
        let x1 = (a.0.max(b.0).max(c.0).ceil() as isize).min(w as isize - 1);
        let y0 = a.1.min(b.1).min(c.1).floor().max(0.0) as usize;
        let y1 = (a.1.max(b.1).max(c.1).ceil() as isize).min(h as isize - 1);
        if x1 < 0 || y1 < 0 {
            continue;
        }
        let inv = 1.0 / area;
        // A little over the edges, so that no seam shows between triangles.
        let slack = -1e-3;
        for y in y0..=y1 as usize {
            let py = y as f32 + 0.5;
            for x in x0..=x1 as usize {
                let px = x as f32 + 0.5;
                let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) * inv;
                let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) * inv;
                let w2 = 1.0 - w0 - w1;
                if w0 < slack || w1 < slack || w2 < slack {
                    continue;
                }
                let z = w0 * a.2 + w1 * b.2 + w2 * c.2;
                let k = y * w + x;
                if z < depth[k] {
                    depth[k] = z;
                    colour[k] = *col;
                    ids[k] = *id;
                }
            }
        }
    }
    // Points as dots, a little in front of what they lie on, so that a point on a surface shows.
    for (p, r, col, id) in dots {
        let (x0, x1) = ((p[0] - r).floor().max(0.0) as isize, ((p[0] + r).ceil() as isize).min(w as isize - 1));
        let (y0, y1) = ((p[1] - r).floor().max(0.0) as isize, ((p[1] + r).ceil() as isize).min(h as isize - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - p[0], y as f32 + 0.5 - p[1]);
                if dx * dx + dy * dy > r * r {
                    continue;
                }
                let k = y as usize * w + x as usize;
                let z = p[2] - 1e-3;
                if z < depth[k] {
                    depth[k] = z;
                    colour[k] = *col;
                    ids[k] = *id;
                }
            }
        }
    }
    (colour, ids)
}

impl Default for View3d {
    fn default() -> Self {
        View3d { azimuth: 200.0, elevation: 35.0, exaggeration: 1.0, zoom: 1.0, pan: Vec2::ZERO, prepared: None, drawn: None }
    }
}

/// A colour for a height, `t` from 0 at the lowest to 1 at the highest: blue, green, yellow,
/// brown and white, as on a map of heights.
fn ramp(t: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 5] =
        [(0.0, [52.0, 101.0, 164.0]), (0.25, [74.0, 152.0, 98.0]), (0.5, [214.0, 196.0, 92.0]), (0.75, [168.0, 112.0, 64.0]), (1.0, [240.0, 238.0, 232.0])];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        if t <= w[1].0 {
            let k = (t - w[0].0) / (w[1].0 - w[0].0);
            return [0, 1, 2].map(|i| w[0].1[i] + (w[1].1[i] - w[0].1[i]) * k);
        }
    }
    STOPS[4].1
}

fn colour(rgb: [f32; 3], light: f32, alpha: u8) -> Color32 {
    let c = |v: f32| (v * light).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(c(rgb[0]), c(rgb[1]), c(rgb[2]), alpha)
}

/// `skip`: how many features first the model shows already; when not given, all of them
/// when there is a model, and the outline when there is a grid.
fn prepare(table: &Table, key: usize, skip: Option<usize>) -> Prepared {
    use geo::BoundingRect;
    let geographic = og_engine::nodes::is_geographic(None, table.crs).unwrap_or(false);
    // The middle of the data, and metres for degrees.
    let mut bbox: Option<geo::Rect<f64>> = None;
    let mut grow = |r: geo::Rect<f64>| {
        bbox = Some(match bbox {
            None => r,
            Some(b) => geo::Rect::new(
                geo::coord! { x: b.min().x.min(r.min().x), y: b.min().y.min(r.min().y) },
                geo::coord! { x: b.max().x.max(r.max().x), y: b.max().y.max(r.max().y) },
            ),
        })
    };
    for f in &table.features {
        if let Some(r) = f.geom.as_ref().and_then(|g| g.bounding_rect()) {
            grow(r);
        }
    }
    if let Some(g) = &table.grid {
        let gt = g.geotransform;
        let (x1, y1) = (gt[0] + gt[1] * g.width as f64, gt[3] + gt[5] * g.height as f64);
        grow(geo::Rect::new(geo::coord! { x: gt[0], y: gt[3] }, geo::coord! { x: x1, y: y1 }));
    }
    let b = bbox.unwrap_or_else(|| geo::Rect::new(geo::coord! { x: 0.0, y: 0.0 }, geo::coord! { x: 1.0, y: 1.0 }));
    let mid = b.center();
    let (kx, ky) = if geographic { (111_320.0 * mid.y.to_radians().cos(), 110_540.0) } else { (1.0, 1.0) };
    let local = |x: f64, y: f64| [((x - mid.x) * kx) as f32, ((y - mid.y) * ky) as f32];

    let mut items = Vec::new();
    let (mut coords, mut left_out, mut with_heights) = (0usize, 0usize, 0usize);
    let mut tris_left_out = 0usize;
    let (mut zlo, mut zhi) = (f32::INFINITY, f32::NEG_INFINITY);
    // A 3D model is drawn from its triangles, in the colours of its materials.
    let mut model = Vec::new();
    if let Some(m) = table.mesh.as_deref() {
        // A model with too many triangles keeps its largest: the shape stays, and what goes
        // is small things such as the screws of a chair.
        let area = |t: &og_engine::mesh::Tri| {
            let p = t.v.map(|i| m.positions[i as usize]);
            let (u, v) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
            let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            c[0] * c[0] + c[1] * c[1] + c[2] * c[2]
        };
        let least = if m.tris.len() > MOST_MODEL {
            let mut areas: Vec<f64> = m.tris.iter().map(area).collect();
            let k = m.tris.len() - MOST_MODEL;
            areas.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
            tris_left_out = k;
            areas[k]
        } else {
            f64::NEG_INFINITY
        };
        let colours: Vec<[f32; 3]> = m.materials.iter().map(|x| x.colour().map(|c| c * 255.0)).collect();
        for t in m.tris.iter().filter(|t| least == f64::NEG_INFINITY || area(t) >= least) {
            let q = t.v.map(|i| {
                let p = m.positions[i as usize];
                let l = local(p[0], p[1]);
                [l[0], l[1], p[2] as f32]
            });
            for p in &q {
                zlo = zlo.min(p[2]);
                zhi = zhi.max(p[2]);
            }
            model.push((q, colours.get(t.material as usize).copied().unwrap_or([200.0; 3]), t.object));
        }
        with_heights = table.len();
    }
    // More coordinates than are drawn: every n-th feature, spread over all of them, and not
    // only the first, which in a point cloud may lie in one corner.
    let step = {
        use geo::CoordsIter;
        let total: usize = table.features.iter().skip(skip.unwrap_or(0)).filter_map(|f| f.geom.as_ref()).map(|g| g.coords_count()).sum();
        total.div_ceil(MOST_COORDS).max(1)
    };
    for (fi, f) in table.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        match skip {
            Some(n) if fi < n => continue,
            Some(_) => {}
            // The model shows these faces already, and the outline of a grid is the grid
            // itself, drawn below.
            None if table.mesh.is_some() || table.grid.is_some() => break,
            None => {}
        }
        if step > 1 && fi % step != 0 {
            left_out += 1;
            continue;
        }
        let all = og_engine::geom::coords(g);
        if coords + all.len() > MOST_COORDS {
            left_out += 1;
            continue;
        }
        coords += all.len();
        let heights = f.heights();
        if heights.is_some() {
            with_heights += 1;
        }
        let zs: Vec<f32> = match heights {
            Some(h) => h.iter().map(|z| *z as f32).collect(),
            None => vec![f32::NAN; all.len()],
        };
        for z in zs.iter().filter(|z| !z.is_nan()) {
            zlo = zlo.min(*z);
            zhi = zhi.max(*z);
        }
        let pts: Vec<[f32; 3]> = all
            .iter()
            .zip(&zs)
            .map(|(c, z)| {
                let p = local(c.x, c.y);
                [p[0], p[1], *z]
            })
            .collect();
        // The parts and rings, by the same walk as the heights.
        let mut ends = Vec::new();
        let mut tris = Vec::new();
        let kind = match g {
            geo::Geometry::Point(_) | geo::Geometry::MultiPoint(_) => Kind::Point,
            geo::Geometry::Line(_) => {
                ends.push(2);
                Kind::Line
            }
            geo::Geometry::LineString(l) => {
                ends.push(l.0.len());
                Kind::Line
            }
            geo::Geometry::MultiLineString(m) => {
                let mut n = 0;
                for l in &m.0 {
                    n += l.0.len();
                    ends.push(n);
                }
                Kind::Line
            }
            geo::Geometry::Polygon(_) | geo::Geometry::MultiPolygon(_) | geo::Geometry::Rect(_) | geo::Geometry::Triangle(_) => {
                let polys: Vec<geo::Polygon<f64>> = match g {
                    geo::Geometry::Polygon(p) => vec![p.clone()],
                    geo::Geometry::MultiPolygon(m) => m.0.clone(),
                    geo::Geometry::Rect(r) => vec![r.to_polygon()],
                    geo::Geometry::Triangle(t) => vec![t.to_polygon()],
                    _ => Vec::new(),
                };
                let mut n = 0usize;
                for p in &polys {
                    let start = n;
                    let mut ring_starts = vec![start];
                    n += p.exterior().0.len();
                    ends.push(n);
                    for r in p.interiors() {
                        ring_starts.push(n);
                        n += r.0.len();
                        ends.push(n);
                    }
                    // An area with heights, such as the wall of a building, is cut into
                    // triangles in its own plane; a flat one from the plan.
                    if let Some(h) = heights {
                        let ext = p.exterior().0.len().saturating_sub(1);
                        let ring: Vec<[f64; 3]> = (start..start + ext).map(|k| [all[k].x, all[k].y, h[k]]).collect();
                        for [a, b, c] in og_engine::mesh::triangulate(&ring) {
                            tris.extend_from_slice(&[(start + a) as u32, (start + b) as u32, (start + c) as u32]);
                        }
                        continue;
                    }
                    // Triangles from the plan, each corner the point it stands on.
                    use geo::TriangulateEarcut;
                    let raw = p.earcut_triangles_raw();
                    let lookup: HashMap<(u64, u64), u32> = (start..n).map(|k| ((all[k].x.to_bits(), all[k].y.to_bits()), k as u32)).collect();
                    let corners: Vec<Option<u32>> = raw.vertices.iter().map(|v| lookup.get(&(v[0].to_bits(), v[1].to_bits())).copied()).collect();
                    for t in raw.triangle_indices.chunks(3) {
                        if let (Some(a), Some(b), Some(c)) = (corners[t[0]], corners[t[1]], corners[t[2]]) {
                            tris.extend_from_slice(&[a, b, c]);
                        }
                    }
                }
                Kind::Area
            }
            geo::Geometry::GeometryCollection(_) => {
                ends.push(pts.len());
                Kind::Line
            }
        };
        items.push(Item { kind, feature: fi, pts, ends, tris, flat: heights.is_none() });
    }
    // A grid of heights, made small enough to turn smoothly.
    let grid = table.grid.as_deref().map(|g| {
        let step = (g.width.max(g.height) as f32 / GRID_SIDE as f32).ceil().max(1.0) as usize;
        let (w, h) = (g.width.div_ceil(step), g.height.div_ceil(step));
        let mut v = Vec::with_capacity(w * h);
        for r in 0..h {
            for c in 0..w {
                let z = g.get((c * step).min(g.width - 1), (r * step).min(g.height - 1)).unwrap_or(f32::NAN);
                if !z.is_nan() {
                    zlo = zlo.min(z);
                    zhi = zhi.max(z);
                }
                v.push(z);
            }
        }
        let gt = g.geotransform;
        let corner = local(gt[0] + gt[1] * step as f64 * 0.5, gt[3] + gt[5] * step as f64 * 0.5);
        let cell = [(gt[1] * step as f64 * kx) as f32, (gt[5] * step as f64 * ky) as f32];
        (w, h, v, corner, cell)
    });
    if !zlo.is_finite() {
        (zlo, zhi) = (0.0, 0.0);
    }
    // Features without heights lie at the lowest height.
    for it in items.iter_mut().filter(|i| i.flat) {
        it.pts.iter_mut().for_each(|p| p[2] = zlo);
    }
    let half = [((b.width() * kx) as f32 / 2.0).max(1.0), ((b.height() * ky) as f32 / 2.0).max(1.0)];
    Prepared { key, items, grid, half, z: [zlo, zhi], with_heights, left_out, tris_left_out, model }
}

impl View3d {
    /// Draws the stream in 3D in all the space that is left, and turns, moves and zooms it
    /// as the user drags and scrolls.
    /// Whether the view shows all of it as at first: not turned, moved or zoomed.
    pub fn at_home(&self) -> bool {
        let d = View3d::default();
        self.azimuth == d.azimuth && self.elevation == d.elevation && self.zoom == d.zoom && self.pan == d.pan
    }

    /// Back to all of it, seen from where it was first seen, with the heights as drawn.
    pub fn reset(&mut self) {
        *self = View3d { exaggeration: self.exaggeration, prepared: self.prepared.take(), drawn: self.drawn.take(), ..View3d::default() };
    }

    pub fn show(&mut self, ui: &mut Ui, table: Option<&Arc<Table>>, selected: Option<usize>) -> Option<Option<usize>> {
        self.show_skipping(ui, table, selected, None)
    }

    /// As `show`, with how many features first the models show already (see `prepare`).
    /// Gives what a click picked: a feature, or nothing when it was beside them.
    pub fn show_skipping(&mut self, ui: &mut Ui, table: Option<&Arc<Table>>, selected: Option<usize>, skip: Option<usize>) -> Option<Option<usize>> {
        let pal = Palette::from_ui(ui);
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, pal.surface);
        painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, pal.line));
        let Some(table) = table else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Run the model, then click a node to see it in 3D.",
                egui::FontId::proportional(13.0),
                pal.muted,
            );
            return None;
        };
        let key = Arc::as_ptr(table) as usize;
        if self.prepared.as_ref().is_none_or(|p| p.key != key) {
            self.prepared = Some(prepare(table, key, skip));
        }
        // Turning, moving and zooming.
        if resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            self.azimuth = (self.azimuth - d.x * 0.4).rem_euclid(360.0);
            self.elevation = (self.elevation + d.y * 0.3).clamp(5.0, 90.0);
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            self.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            self.zoom = (self.zoom * (scroll * 0.0035).exp() * pinch).clamp(0.05, 50.0);
        }
        if resp.double_clicked() {
            self.reset();
        }
        let prep = self.prepared.as_ref().expect("prepared");
        let ex = self.exaggeration;
        let zmid = (prep.z[0] + prep.z[1]) / 2.0;
        let span = (prep.z[1] - prep.z[0]).max(1e-6);
        let (a, e) = (self.azimuth.to_radians(), self.elevation.to_radians());
        let (sa, ca, se, ce) = (a.sin(), a.cos(), e.sin(), e.cos());
        // The size that makes everything fit, at any angle.
        let radius = (prep.half[0].hypot(prep.half[1])).max((prep.z[1] - prep.z[0]) * ex / 2.0).max(1.0);
        let scale = rect.width().min(rect.height()) * 0.46 / radius * self.zoom;
        let mid = rect.center() + self.pan;
        // From the place to the screen, and how far it is from the eye.
        let project = |p: [f32; 3]| -> (Pos2, f32) {
            let (x, y, z) = (p[0], p[1], (p[2] - zmid) * ex);
            // The viewer stands at the azimuth and looks towards the middle: to the right
            // is across that, and ahead is away from the viewer.
            let right = -x * ca + y * sa;
            let ahead = -(x * sa + y * ca);
            let sx = right;
            let sy = ahead * se + z * ce;
            let depth = ahead * ce - z * se;
            (Pos2::new(mid.x + sx * scale, mid.y - sy * scale), depth)
        };
        let t_of = |z: f32| if prep.z[1] > prep.z[0] { (z - prep.z[0]) / span } else { 0.4 };
        // The direction of the light, from the north-west and high, in the turned frame it
        // does not depend on: the normal of a surface is taken in place coordinates.
        let sun = {
            let (az, alt) = (315f32.to_radians(), 45f32.to_radians());
            [az.sin() * alt.cos(), az.cos() * alt.cos(), alt.sin()]
        };
        let shade = |n: [f32; 3]| {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
            0.55 + 0.55 * ((n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]) / l).max(0.0)
        };
        // Drawn again only when something has changed: the view, the size or the choice.
        let ppp = ui.ctx().pixels_per_point();
        let (pw, ph) = ((rect.width() * ppp).round().max(1.0) as usize, (rect.height() * ppp).round().max(1.0) as usize);
        let key: Vec<u32> = [
            key as u64 as u32,
            (key as u64 >> 32) as u32,
            self.azimuth.to_bits(),
            self.elevation.to_bits(),
            ex.to_bits(),
            self.zoom.to_bits(),
            self.pan.x.to_bits(),
            self.pan.y.to_bits(),
            pw as u32,
            ph as u32,
            selected.map_or(NONE, |s| s as u32),
            ppp.to_bits(),
            mid.x.to_bits(),
            mid.y.to_bits(),
        ]
        .to_vec();
        let need = self.drawn.as_ref().is_none_or(|d| d.key != key);
        // What is drawn, only when it is to be drawn again: on a large point cloud it is many.
        let (faces, dots) = if need {
            // The surfaces: triangles of areas and cells of the grid, drawn from far to near.
            let mut faces: Vec<ScreenFace> = Vec::new();
            if let Some((w, h, v, corner, cell)) = &prep.grid {
                let at = |c: usize, r: usize| [corner[0] + c as f32 * cell[0], corner[1] + r as f32 * cell[1], v[r * w + c]];
                for r in 0..h.saturating_sub(1) {
                    for c in 0..w.saturating_sub(1) {
                        let q = [at(c, r), at(c + 1, r), at(c + 1, r + 1), at(c, r + 1)];
                        if q.iter().any(|p| p[2].is_nan()) {
                            continue;
                        }
                        // The slope of the cell, with the heights as they are drawn.
                        let dzx = ((q[1][2] + q[2][2]) - (q[0][2] + q[3][2])) / 2.0 * ex;
                        let dzy = ((q[3][2] + q[2][2]) - (q[0][2] + q[1][2])) / 2.0 * ex;
                        let n = [-dzx / cell[0], -dzy / cell[1], 1.0];
                        let light = shade(n);
                        let zavg = (q[0][2] + q[1][2] + q[2][2] + q[3][2]) / 4.0;
                        let col = colour(ramp(t_of(zavg)), light, 255);
                        let p: Vec<(Pos2, f32)> = q.iter().map(|x| project(*x)).collect();
                        faces.push(([p[0], p[1], p[2]], col, NONE));
                        faces.push(([p[0], p[2], p[3]], col, NONE));
                    }
                }
            }
            // The triangles of a model, lit from their side towards the viewer.
            let toward = [sa * ce, ca * ce, se];
            for (q, rgb, object) in &prep.model {
                let u = [q[1][0] - q[0][0], q[1][1] - q[0][1], (q[1][2] - q[0][2]) * ex];
                let w = [q[2][0] - q[0][0], q[2][1] - q[0][1], (q[2][2] - q[0][2]) * ex];
                let mut n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                if n[0] * toward[0] + n[1] * toward[1] + n[2] * toward[2] < 0.0 {
                    n = n.map(|x| -x);
                }
                let chosen = selected == Some(*object as usize);
                let rgb = if chosen { [pal.accent.r() as f32, pal.accent.g() as f32, pal.accent.b() as f32] } else { *rgb };
                let p: Vec<(Pos2, f32)> = q.iter().map(|x| project(*x)).collect();
                faces.push(([p[0], p[1], p[2]], colour(rgb, shade(n), 255), *object));
            }
            for it in prep.items.iter().filter(|i| i.kind == Kind::Area) {
                let chosen = selected == Some(it.feature);
                for t in it.tris.chunks(3) {
                    let q = [it.pts[t[0] as usize], it.pts[t[1] as usize], it.pts[t[2] as usize]];
                    // The normal of the triangle, for the light.
                    let u = [q[1][0] - q[0][0], q[1][1] - q[0][1], (q[1][2] - q[0][2]) * ex];
                    let w = [q[2][0] - q[0][0], q[2][1] - q[0][1], (q[2][2] - q[0][2]) * ex];
                    let mut n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                    if n[2] < 0.0 {
                        n = n.map(|x| -x);
                    }
                    let zavg = (q[0][2] + q[1][2] + q[2][2]) / 3.0;
                    let rgb = if chosen { [pal.accent.r() as f32, pal.accent.g() as f32, pal.accent.b() as f32] } else { ramp(t_of(zavg)) };
                    let p: Vec<(Pos2, f32)> = q.iter().map(|x| project(*x)).collect();
                    faces.push(([p[0], p[1], p[2]], colour(rgb, shade(n), 255), it.feature as u32));
                }
            }
            // The points, as dots.
            let mut dots: Vec<PixelDot> = Vec::new();
            for it in prep.items.iter().filter(|i| i.kind == Kind::Point) {
                let chosen = selected == Some(it.feature);
                for p in &it.pts {
                    let (sp, z) = project(*p);
                    let col = if chosen {
                        pal.accent
                    } else if it.flat {
                        pal.xform
                    } else {
                        colour(ramp(t_of(p[2])), 0.85, 255)
                    };
                    let r = if chosen {
                        5.0
                    } else if it.pts.len() > 1 || prep.items.len() > 20_000 {
                        1.6
                    } else {
                        3.5
                    };
                    dots.push(([(sp.x - rect.left()) * ppp, (sp.y - rect.top()) * ppp, z], r * ppp, col, it.feature as u32));
                }
            }
            (faces, dots)
        } else {
            (Vec::new(), Vec::new())
        };
        if need {
            let px: Vec<PixelFace> =
                faces.iter().map(|(t, c, id)| (t.map(|(p, z)| ((p.x - rect.left()) * ppp, (p.y - rect.top()) * ppp, z)), *c, *id)).collect();
            let (colours, ids) = rasterize(pw, ph, &px, &dots);
            let image = egui::ColorImage { size: [pw, ph], source_size: Vec2::new(pw as f32, ph as f32), pixels: colours };
            let texture = match self.drawn.take() {
                Some(mut d) => {
                    d.texture.set(image, egui::TextureOptions::LINEAR);
                    d.texture
                }
                None => ui.ctx().load_texture("view3d", image, egui::TextureOptions::LINEAR),
            };
            self.drawn = Some(Drawn { key, texture, ids, size: [pw, ph] });
        }
        let drawn = self.drawn.as_ref().expect("drawn");
        painter.image(drawn.texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        // A click picks what is drawn under it.
        let mut picked = None;
        if resp.clicked() {
            if let Some(at) = resp.interact_pointer_pos() {
                let (x, y) = (((at.x - rect.left()) * ppp) as usize, ((at.y - rect.top()) * ppp) as usize);
                if x < drawn.size[0] && y < drawn.size[1] {
                    let id = drawn.ids[y * drawn.size[0] + x];
                    picked = Some((id != NONE).then_some(id as usize));
                }
            }
        }
        // Lines and outlines, coloured by their height, and points on top.
        let outline = Color32::from_rgba_unmultiplied(pal.ink.r(), pal.ink.g(), pal.ink.b(), 110);
        for it in &prep.items {
            let chosen = selected == Some(it.feature);
            match it.kind {
                // Points are dots in the picture, drawn with the surfaces.
                Kind::Point => {}
                Kind::Line | Kind::Area => {
                    let mut start = 0;
                    for &end in &it.ends {
                        let part = &it.pts[start..end.min(it.pts.len())];
                        for w in part.windows(2) {
                            let (a, b) = (project(w[0]).0, project(w[1]).0);
                            let col = match (chosen, it.kind, it.flat) {
                                (true, _, _) => pal.accent,
                                (false, Kind::Area, _) => outline,
                                (false, _, true) => pal.xform,
                                (false, _, false) => colour(ramp(t_of((w[0][2] + w[1][2]) / 2.0)), 0.8, 255),
                            };
                            painter.line_segment(
                                [a, b],
                                Stroke::new(
                                    if chosen {
                                        2.5
                                    } else if it.kind == Kind::Line {
                                        1.8
                                    } else {
                                        0.8
                                    },
                                    col,
                                ),
                            );
                        }
                        start = end;
                    }
                }
            }
        }
        // North, as an arrow in a corner, turned as the view is.
        let n0 = rect.right_top() + Vec2::new(-28.0, 30.0);
        let (nx, ny) = (sa, -ca * se);
        let tip = n0 + Vec2::new(nx, -ny) * 16.0;
        painter.arrow(n0 - Vec2::new(nx, -ny) * 8.0, Vec2::new(nx, -ny) * 24.0, Stroke::new(1.5, pal.ink));
        painter.text(tip + Vec2::new(nx, -ny) * 10.0, Align2::CENTER_CENTER, "N", egui::FontId::proportional(11.0), pal.ink);
        // What is shown, and how to turn it.
        let mut lines = vec!["Drag to turn · right-drag to move · scroll to zoom".to_string()];
        if prep.with_heights == 0 && prep.grid.is_none() {
            lines.push("This stream has no heights, so it lies flat. Set height gives it some.".to_string());
        } else if prep.z[1] > prep.z[0] {
            let clean = |v: f32| fmt_real(v.to_string().parse::<f64>().unwrap_or(v as f64));
            lines.push(format!("Heights {} to {} · drawn × {}", clean(prep.z[0]), clean(prep.z[1]), clean(ex)));
        }
        if prep.left_out > 0 {
            lines.push(format!("{} features are left out, to keep it quick", fmt_count(prep.left_out as u64)));
        }
        if prep.tris_left_out > 0 {
            lines.push(format!("The {} smallest triangles are left out, to keep it quick", fmt_count(prep.tris_left_out as u64)));
        }
        let mut y = rect.bottom() - 8.0;
        for l in lines.iter().rev() {
            let galley = painter.layout_no_wrap(l.clone(), egui::FontId::proportional(11.5), pal.ink);
            let r = Rect::from_min_size(Pos2::new(rect.left() + 8.0, y - galley.size().y - 4.0), galley.size() + Vec2::new(10.0, 4.0));
            painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(pal.surface.r(), pal.surface.g(), pal.surface.b(), 215));
            painter.galley(r.min + Vec2::new(5.0, 2.0), galley, pal.ink);
            y = r.top() - 3.0;
        }
        picked
    }
}
