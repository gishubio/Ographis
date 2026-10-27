//! The geometries of a stream, made ready for the map: every coordinate as a pair of f32
//! from an origin near the data, an index of where the features are, and a rasteriser that
//! draws millions of features into an image in a thread of its own.
//!
//! The map draws the features in view as shapes when they are few enough, and as an image
//! when they are many. Either way every feature is drawn: nothing is left out.

use geo::{Geometry, TriangulateEarcut};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// No more features than this in view are drawn as shapes. More are drawn as an image.
pub const SHAPE_LIMIT: usize = 40_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Point,
    Line,
    Area,
}

/// One drawable feature. A geometry collection becomes several, with the same row.
#[derive(Debug, Clone, Copy)]
pub struct Feat {
    /// The place of the feature in the table.
    pub row: u32,
    /// Its parts are `parts[first..end]`.
    pub first: u32,
    pub end: u32,
    pub kind: Kind,
    /// Bounding box in store coordinates: x0, y0, x1, y1.
    pub bbox: [f32; 4],
}

/// Features found by the index: few, listed, or many, counted roughly.
pub enum InView {
    Few(Vec<u32>),
    Many(usize),
}

pub struct Store {
    /// The world coordinate that store coordinates are counted from.
    pub origin: [f64; 2],
    pub coords: Vec<[f32; 2]>,
    /// Where each part starts in `coords`. One more than the number of parts.
    pub parts: Vec<u32>,
    /// Whether a part begins a new polygon: an exterior ring. Holes follow their polygon.
    pub new_polygon: Vec<bool>,
    pub feats: Vec<Feat>,
    /// All of it, in world coordinates.
    pub bbox: Option<[f64; 4]>,
    grid: Grid,
}

struct Grid {
    /// Store coordinates of the lower left corner, and the size of a cell.
    x0: f32,
    y0: f32,
    cell: f32,
    n: usize,
    cells: Vec<Vec<u32>>,
    /// Features that cover too many cells to be listed in each.
    big: Vec<u32>,
}

impl Grid {
    fn empty() -> Grid {
        Grid { x0: 0.0, y0: 0.0, cell: 1.0, n: 0, cells: Vec::new(), big: Vec::new() }
    }

    fn range(&self, b: [f32; 4]) -> (usize, usize, usize, usize) {
        let at = |v: f32, o: f32| (((v - o) / self.cell).floor().max(0.0) as usize).min(self.n.saturating_sub(1));
        (at(b[0], self.x0), at(b[1], self.y0), at(b[2], self.x0), at(b[3], self.y0))
    }

    fn build(feats: &[Feat]) -> Grid {
        if feats.is_empty() {
            return Grid::empty();
        }
        let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for f in feats {
            b = [b[0].min(f.bbox[0]), b[1].min(f.bbox[1]), b[2].max(f.bbox[2]), b[3].max(f.bbox[3])];
        }
        // About a few features per cell, within reason.
        let n = ((feats.len() as f64 / 4.0).sqrt().ceil() as usize).clamp(1, 512);
        let cell = ((b[2] - b[0]).max(b[3] - b[1]) / n as f32).max(1e-6);
        let mut grid = Grid { x0: b[0], y0: b[1], cell, n, cells: vec![Vec::new(); n * n], big: Vec::new() };
        for (i, f) in feats.iter().enumerate() {
            let (cx0, cy0, cx1, cy1) = grid.range(f.bbox);
            if (cx1 - cx0 + 1) * (cy1 - cy0 + 1) > 64 {
                grid.big.push(i as u32);
                continue;
            }
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    grid.cells[cy * n + cx].push(i as u32);
                }
            }
        }
        grid
    }
}

/// Reprojects coordinates in place, x and y.
pub type Reproject<'a> = dyn Fn(&mut [f64], &mut [f64]) -> Result<(), String> + 'a;

/// How far the building of a store has come, for a note on the map while it is going on.
#[derive(Default)]
pub struct Building {
    pub done: AtomicUsize,
    pub cancel: std::sync::atomic::AtomicBool,
}

fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[2] >= b[0] && a[0] <= b[2] && a[3] >= b[1] && a[1] <= b[3]
}

impl Store {
    /// Builds the store from the geometries of a table. `transform`, if given, reprojects
    /// coordinates in place, a large batch at a time. Returns None when it was cancelled.
    pub fn build(
        geoms: &mut dyn Iterator<Item = (usize, &Geometry<f64>)>,
        transform: Option<&Reproject<'_>>,
        progress: &Building,
    ) -> Result<Option<Store>, String> {
        let mut s = Store {
            origin: [f64::NAN, f64::NAN],
            coords: Vec::new(),
            parts: vec![0],
            new_polygon: Vec::new(),
            feats: Vec::new(),
            bbox: None,
            grid: Grid::empty(),
        };
        let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
        let mut pending = 0usize;
        // Flushes the coordinates gathered so far: reprojects them, turns them into store
        // coordinates, and works out the boxes of the features they belong to.
        let flush = |s: &mut Store, xs: &mut Vec<f64>, ys: &mut Vec<f64>, pending: &mut usize| -> Result<(), String> {
            if let Some(t) = transform {
                t(xs, ys)?;
            }
            if s.origin[0].is_nan() {
                if let Some(i) = xs.iter().zip(ys.iter()).position(|(x, y)| x.is_finite() && y.is_finite()) {
                    s.origin = [xs[i], ys[i]];
                }
            }
            let o = s.origin;
            s.coords.extend(xs.iter().zip(ys.iter()).map(|(x, y)| [(x - o[0]) as f32, (y - o[1]) as f32]));
            xs.clear();
            ys.clear();
            for f in &mut s.feats[*pending..] {
                let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
                let (from, to) = (s.parts[f.first as usize] as usize, s.parts[f.end as usize] as usize);
                for c in &s.coords[from..to] {
                    if c[0].is_finite() && c[1].is_finite() {
                        b = [b[0].min(c[0]), b[1].min(c[1]), b[2].max(c[0]), b[3].max(c[1])];
                    }
                }
                f.bbox = b;
            }
            *pending = s.feats.len();
            Ok(())
        };
        for (n, (row, g)) in geoms.enumerate() {
            if n % 4096 == 0 {
                if progress.cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                progress.done.store(n, Ordering::Relaxed);
            }
            add(&mut s, row as u32, g, &mut xs, &mut ys);
            if xs.len() >= 1 << 20 {
                flush(&mut s, &mut xs, &mut ys, &mut pending)?;
            }
        }
        flush(&mut s, &mut xs, &mut ys, &mut pending)?;
        // Features without a single usable coordinate are not drawn.
        s.feats.retain(|f| f.bbox[0].is_finite());
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for f in &s.feats {
            b = [b[0].min(f.bbox[0] as f64), b[1].min(f.bbox[1] as f64), b[2].max(f.bbox[2] as f64), b[3].max(f.bbox[3] as f64)];
        }
        if b[0].is_finite() {
            s.bbox = Some([b[0] + s.origin[0], b[1] + s.origin[1], b[2] + s.origin[0], b[3] + s.origin[1]]);
        }
        s.grid = Grid::build(&s.feats);
        Ok(Some(s))
    }

    /// The coordinates of a part.
    pub fn part(&self, p: usize) -> &[[f32; 2]] {
        &self.coords[self.parts[p] as usize..self.parts[p + 1] as usize]
    }

    /// A box in world coordinates as a box in store coordinates.
    pub fn local(&self, view: [f64; 4]) -> [f32; 4] {
        let o = self.origin;
        [(view[0] - o[0]) as f32, (view[1] - o[1]) as f32, (view[2] - o[0]) as f32, (view[3] - o[1]) as f32]
    }

    /// The features in a box of world coordinates.
    pub fn in_view(&self, view: [f64; 4], limit: usize) -> InView {
        let v = self.local(view);
        if self.grid.n == 0 {
            return InView::Few(Vec::new());
        }
        let g = &self.grid;
        let (cx0, cy0, cx1, cy1) = g.range(v);
        let outside = v[2] < g.x0 || v[3] < g.y0 || v[0] > g.x0 + g.cell * g.n as f32 || v[1] > g.y0 + g.cell * g.n as f32;
        let mut rough = g.big.len();
        if !outside {
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    rough += g.cells[cy * g.n + cx].len();
                }
            }
        }
        // The rough count counts a feature once for each cell it is in. Well above the limit
        // there is no need to be exact.
        if rough > limit * 3 {
            return InView::Many(rough);
        }
        let mut found: Vec<u32> = g.big.iter().copied().filter(|i| overlaps(self.feats[*i as usize].bbox, v)).collect();
        if !outside {
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    found.extend(g.cells[cy * g.n + cx].iter().copied().filter(|i| overlaps(self.feats[*i as usize].bbox, v)));
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        if found.len() > limit {
            InView::Many(found.len())
        } else {
            InView::Few(found)
        }
    }

    /// The triangles of the area of a feature, for its fill: the corners and, three by
    /// three, which of them make a triangle.
    pub fn triangles(&self, f: &Feat) -> (Vec<[f32; 2]>, Vec<u32>) {
        let (mut verts, mut tris) = (Vec::new(), Vec::new());
        let ring = |p: usize| geo::LineString::from(self.part(p).iter().map(|c| (c[0] as f64, c[1] as f64)).collect::<Vec<_>>());
        let mut p = f.first as usize;
        while p < f.end as usize {
            let ext = ring(p);
            let mut holes = Vec::new();
            let mut q = p + 1;
            while q < f.end as usize && !self.new_polygon[q] {
                holes.push(ring(q));
                q += 1;
            }
            let raw = geo::Polygon::new(ext, holes).earcut_triangles_raw();
            let base = verts.len() as u32;
            verts.extend(raw.vertices.iter().map(|c| [c[0] as f32, c[1] as f32]));
            tris.extend(raw.triangle_indices.iter().map(|i| base + *i as u32));
            p = q;
        }
        (verts, tris)
    }

    /// The feature at a point given in store coordinates, within `tol` store units for points
    /// and lines. Points and lines go before areas, and the last drawn before the first.
    pub fn pick(&self, at: [f32; 2], tol: f32) -> Option<u32> {
        let probe = [at[0] - tol, at[1] - tol, at[0] + tol, at[1] + tol];
        let mut near: Vec<u32> = self.in_view_local(probe)?;
        near.sort_unstable();
        let dist = |a: [f32; 2], b: [f32; 2]| {
            let (abx, aby, apx, apy) = (b[0] - a[0], b[1] - a[1], at[0] - a[0], at[1] - a[1]);
            let len = abx * abx + aby * aby;
            let t = if len > 0.0 { ((apx * abx + apy * aby) / len).clamp(0.0, 1.0) } else { 0.0 };
            let (dx, dy) = (a[0] + abx * t - at[0], a[1] + aby * t - at[1]);
            (dx * dx + dy * dy).sqrt()
        };
        for &i in near.iter().rev() {
            let f = &self.feats[i as usize];
            let hit = match f.kind {
                Kind::Point => (f.first..f.end).any(|p| self.part(p as usize).iter().any(|c| dist(*c, *c) <= tol)),
                Kind::Line => (f.first..f.end).any(|p| self.part(p as usize).windows(2).any(|s| dist(s[0], s[1]) <= tol)),
                Kind::Area => false,
            };
            if hit {
                return Some(f.row);
            }
        }
        for &i in near.iter().rev() {
            let f = &self.feats[i as usize];
            if f.kind != Kind::Area || at[0] < f.bbox[0] || at[0] > f.bbox[2] || at[1] < f.bbox[1] || at[1] > f.bbox[3] {
                continue;
            }
            // Even–odd over the rings of each polygon handles its holes; the polygons of a
            // multi area each count on their own, so that parts that overlap, as the faces
            // of a building model seen from above, are hits too.
            let (mut inside, mut any) = (false, false);
            for p in f.first..f.end {
                if self.new_polygon.get(p as usize).copied().unwrap_or(false) {
                    any |= inside;
                    inside = false;
                }
                let r = self.part(p as usize);
                let mut j = r.len().saturating_sub(1);
                for k in 0..r.len() {
                    let (a, b) = (r[k], r[j]);
                    if (a[1] > at[1]) != (b[1] > at[1]) && at[0] < (b[0] - a[0]) * (at[1] - a[1]) / (b[1] - a[1]) + a[0] {
                        inside = !inside;
                    }
                    j = k;
                }
            }
            if any || inside {
                return Some(f.row);
            }
        }
        None
    }

    fn in_view_local(&self, v: [f32; 4]) -> Option<Vec<u32>> {
        let g = &self.grid;
        if g.n == 0 {
            return None;
        }
        let (cx0, cy0, cx1, cy1) = g.range(v);
        let mut found: Vec<u32> = g.big.iter().copied().filter(|i| overlaps(self.feats[*i as usize].bbox, v)).collect();
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                found.extend(g.cells[cy * g.n + cx].iter().copied().filter(|i| overlaps(self.feats[*i as usize].bbox, v)));
            }
        }
        found.dedup();
        Some(found)
    }
}

/// Adds the parts of a geometry to the store, with its coordinates in the batch. `parts`
/// always ends with the end of the last part, which is where the next one starts.
fn add(s: &mut Store, row: u32, g: &Geometry<f64>, xs: &mut Vec<f64>, ys: &mut Vec<f64>) {
    fn part(s: &mut Store, xs: &mut Vec<f64>, ys: &mut Vec<f64>, coords: &mut dyn Iterator<Item = (f64, f64)>, new_polygon: bool) {
        for (x, y) in coords {
            xs.push(x);
            ys.push(y);
        }
        s.parts.push((s.coords.len() + xs.len()) as u32);
        s.new_polygon.push(new_polygon);
    }
    fn ring(s: &mut Store, xs: &mut Vec<f64>, ys: &mut Vec<f64>, ls: &geo::LineString<f64>, new_polygon: bool) {
        part(s, xs, ys, &mut ls.0.iter().map(|c| (c.x, c.y)), new_polygon);
    }
    fn polygon(s: &mut Store, xs: &mut Vec<f64>, ys: &mut Vec<f64>, p: &geo::Polygon<f64>) {
        ring(s, xs, ys, p.exterior(), true);
        for h in p.interiors() {
            ring(s, xs, ys, h, false);
        }
    }
    let first = (s.parts.len() - 1) as u32;
    let kind = match g {
        Geometry::Point(p) => {
            part(s, xs, ys, &mut std::iter::once((p.x(), p.y())), true);
            Kind::Point
        }
        Geometry::MultiPoint(m) => {
            part(s, xs, ys, &mut m.0.iter().map(|p| (p.x(), p.y())), true);
            Kind::Point
        }
        Geometry::LineString(l) => {
            ring(s, xs, ys, l, true);
            Kind::Line
        }
        Geometry::Line(l) => {
            ring(s, xs, ys, &geo::LineString::from(*l), true);
            Kind::Line
        }
        Geometry::MultiLineString(m) => {
            for l in &m.0 {
                ring(s, xs, ys, l, true);
            }
            Kind::Line
        }
        Geometry::Polygon(p) => {
            polygon(s, xs, ys, p);
            Kind::Area
        }
        Geometry::MultiPolygon(m) => {
            for p in &m.0 {
                polygon(s, xs, ys, p);
            }
            Kind::Area
        }
        Geometry::Rect(r) => {
            polygon(s, xs, ys, &r.to_polygon());
            Kind::Area
        }
        Geometry::Triangle(t) => {
            polygon(s, xs, ys, &t.to_polygon());
            Kind::Area
        }
        Geometry::GeometryCollection(c) => {
            for g in &c.0 {
                add(s, row, g, xs, ys);
            }
            return;
        }
    };
    let end = (s.parts.len() - 1) as u32;
    if end > first {
        s.feats.push(Feat { row, first, end, kind, bbox: [f32::NAN; 4] });
    }
}

// ---------- The image ----------

/// What an image is drawn for: its size in pixels and the part of the world it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderView {
    pub w: usize,
    pub h: usize,
    /// The world coordinate in the middle of the image.
    pub center: [f64; 2],
    /// Pixels per world unit, north–south, and the compression east–west.
    pub px_per_unit: f64,
    pub kx: f64,
}

impl RenderView {
    /// The world box the image covers: x0, y0, x1, y1.
    pub fn world(&self) -> [f64; 4] {
        let (hw, hh) = (self.w as f64 / 2.0 / (self.px_per_unit * self.kx), self.h as f64 / 2.0 / self.px_per_unit);
        [self.center[0] - hw, self.center[1] - hh, self.center[0] + hw, self.center[1] + hh]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub stroke: [u8; 4],
    pub fill: [u8; 4],
    pub point: [u8; 4],
}

const FILL: u8 = 1;
const LINE: u8 = 2;
const POINT: u8 = 4;

/// Draws every feature in view into an image, RGBA without premultiplied alpha. Returns
/// None when `generation` moved on while drawing, since the image is then not wanted.
pub fn rasterize(store: &Store, view: RenderView, colors: Colors, thickness: usize, generation: &AtomicU64, mine: u64) -> Option<Vec<u8>> {
    let (w, h) = (view.w, view.h);
    if w == 0 || h == 0 {
        return Some(Vec::new());
    }
    let mut cover = vec![0u8; w * h];
    let sx = view.px_per_unit * view.kx;
    let sy = view.px_per_unit;
    // Store coordinates to pixels: x right, y down.
    let ox = (store.origin[0] - view.center[0]) * sx + w as f64 / 2.0;
    let oy = h as f64 / 2.0 - (store.origin[1] - view.center[1]) * sy;
    let to_px = |c: [f32; 2]| -> (f64, f64) { (ox + c[0] as f64 * sx, oy - c[1] as f64 * sy) };
    let world = view.world();
    let v = store.local(world);
    let list: Vec<u32> = match store.in_view(world, usize::MAX / 4) {
        InView::Few(l) => l,
        InView::Many(_) => (0..store.feats.len() as u32).collect(),
    };
    let t = thickness.max(1) as i64;
    let set = |cover: &mut Vec<u8>, x: i64, y: i64, bit: u8| {
        for dy in 0..t {
            for dx in 0..t {
                let (px, py) = (x + dx, y + dy);
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    cover[py as usize * w + px as usize] |= bit;
                }
            }
        }
    };
    let (fw, fh) = (w as f64, h as f64);
    for (n, &i) in list.iter().enumerate() {
        if n % 20_000 == 0 && generation.load(Ordering::Relaxed) != mine {
            return None;
        }
        let f = &store.feats[i as usize];
        if !overlaps(f.bbox, v) {
            continue;
        }
        match f.kind {
            Kind::Point => {
                let r = (t + 1).max(2);
                for p in f.first..f.end {
                    for c in store.part(p as usize) {
                        let (x, y) = to_px(*c);
                        let (x, y) = (x.floor() as i64, y.floor() as i64);
                        for dy in -r..=r {
                            for dx in -r..=r {
                                if dx * dx + dy * dy <= r * r + 1 {
                                    set(&mut cover, x + dx, y + dy, POINT);
                                }
                            }
                        }
                    }
                }
            }
            Kind::Line | Kind::Area => {
                if f.kind == Kind::Area {
                    fill_area(store, f, &to_px, &mut cover, w, h);
                }
                for p in f.first..f.end {
                    let pts = store.part(p as usize);
                    if pts.len() == 1 {
                        let (x, y) = to_px(pts[0]);
                        set(&mut cover, x.floor() as i64, y.floor() as i64, LINE);
                    }
                    for s in pts.windows(2) {
                        let (a, b) = (to_px(s[0]), to_px(s[1]));
                        if let Some((a, b)) = clip(a, b, fw, fh) {
                            line(a, b, &mut |x, y| set(&mut cover, x, y, LINE));
                        }
                    }
                }
            }
        }
    }
    let mut rgba = vec![0u8; w * h * 4];
    for (k, c) in cover.iter().enumerate() {
        let color = if c & POINT != 0 {
            colors.point
        } else if c & LINE != 0 {
            colors.stroke
        } else if c & FILL != 0 {
            colors.fill
        } else {
            continue;
        };
        rgba[k * 4..k * 4 + 4].copy_from_slice(&color);
    }
    Some(rgba)
}

/// Fills an area, even–odd over all its rings, one row of pixels at a time.
fn fill_area(store: &Store, f: &Feat, to_px: &dyn Fn([f32; 2]) -> (f64, f64), cover: &mut [u8], w: usize, h: usize) {
    let mut edges: Vec<(f64, f64, f64, f64)> = Vec::new();
    let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in f.first..f.end {
        let r = store.part(p as usize);
        for s in r.windows(2) {
            let (a, b) = (to_px(s[0]), to_px(s[1]));
            if a.1 != b.1 {
                edges.push((a.0, a.1, b.0, b.1));
            }
            y0 = y0.min(a.1.min(b.1));
            y1 = y1.max(a.1.max(b.1));
        }
    }
    if edges.is_empty() || y1 < 0.0 || y0 > h as f64 {
        return;
    }
    let (r0, r1) = ((y0.floor().max(0.0)) as usize, (y1.ceil().min(h as f64)) as usize);
    let mut xs: Vec<f64> = Vec::new();
    for row in r0..r1 {
        let y = row as f64 + 0.5;
        xs.clear();
        for &(ax, ay, bx, by) in &edges {
            if (ay > y) != (by > y) {
                xs.push(ax + (y - ay) * (bx - ax) / (by - ay));
            }
        }
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let (from, to) = ((pair[0] + 0.5).floor().max(0.0) as usize, ((pair[1] + 0.5).floor().min(w as f64)).max(0.0) as usize);
            for x in from..to {
                cover[row * w + x] |= FILL;
            }
        }
    }
}

/// A segment cut to the image, or None when it lies outside.
fn clip(a: (f64, f64), b: (f64, f64), w: f64, h: f64) -> Option<((f64, f64), (f64, f64))> {
    if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) {
        return None;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0 + 1.0), (dx, w + 1.0 - a.0), (-dy, a.1 + 1.0), (dy, h + 1.0 - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(((a.0 + t0 * dx, a.1 + t0 * dy), (a.0 + t1 * dx, a.1 + t1 * dy)))
}

/// The pixels of a segment, one per step along its longer side.
fn line(a: (f64, f64), b: (f64, f64), plot: &mut dyn FnMut(i64, i64)) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0);
    let (sx, sy) = (dx / steps, dy / steps);
    let (mut x, mut y) = (a.0, a.1);
    for _ in 0..=steps as i64 {
        plot(x.floor() as i64, y.floor() as i64);
        x += sx;
        y += sy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{line_string, point, polygon};

    fn store(geoms: Vec<Geometry<f64>>) -> Store {
        let b = Building::default();
        Store::build(&mut geoms.iter().enumerate(), None, &b).unwrap().unwrap()
    }

    #[test]
    fn every_feature_is_in_the_store_and_found_by_the_index() {
        let mut geoms: Vec<Geometry<f64>> = Vec::new();
        for i in 0..10_000 {
            let x = 400_000.0 + (i % 100) as f64 * 100.0;
            let y = 6_400_000.0 + (i / 100) as f64 * 100.0;
            geoms.push(line_string![(x: x, y: y), (x: x + 50.0, y: y + 50.0)].into());
        }
        geoms.push(
            polygon![(x: 399_000.0, y: 6_399_000.0), (x: 411_000.0, y: 6_399_000.0), (x: 411_000.0, y: 6_411_000.0), (x: 399_000.0, y: 6_399_000.0)].into(),
        );
        let s = store(geoms);
        assert_eq!(s.feats.len(), 10_001);
        assert_eq!(s.bbox, Some([399_000.0, 6_399_000.0, 411_000.0, 6_411_000.0]));
        // A small view finds a few lines and the large area; the whole finds them all.
        match s.in_view([400_000.0, 6_400_000.0, 400_160.0, 6_400_160.0], 1000) {
            InView::Few(l) => assert_eq!(l.len(), 2 * 2 + 1, "{:?}", l),
            InView::Many(_) => panic!("few are in view"),
        }
        assert!(matches!(s.in_view([0.0, 0.0, 1e7, 1e7], 1000), InView::Many(_)));
        match s.in_view([0.0, 0.0, 1e7, 1e7], 20_000) {
            InView::Few(l) => assert_eq!(l.len(), 10_001),
            InView::Many(n) => panic!("all {} fit under the limit", n),
        }
        // A click on a line finds it, a click in the area finds the area.
        let at = |x: f64, y: f64| [(x - s.origin[0]) as f32, (y - s.origin[1]) as f32];
        assert_eq!(s.pick(at(400_025.0, 6_400_025.0), 1.0), Some(0));
        assert_eq!(s.pick(at(400_075.0, 6_400_025.0), 1.0), Some(10_000));
    }

    /// The footprint of a building from IFC is a multipolygon of parts that lie on top of each
    /// other: a click where two overlap is still on the building, not in a hole.
    #[test]
    fn a_click_where_parts_of_a_multipolygon_overlap_finds_it() {
        let square = |x: f64| polygon![(x: x, y: 0.0), (x: x + 10.0, y: 0.0), (x: x + 10.0, y: 10.0), (x: x, y: 10.0), (x: x, y: 0.0)];
        let walls = geo::MultiPolygon(vec![square(0.0), square(5.0), square(5.0)]);
        let s = store(vec![walls.into()]);
        let at = |x: f64, y: f64| [(x - s.origin[0]) as f32, (y - s.origin[1]) as f32];
        assert_eq!(s.pick(at(2.0, 5.0), 0.1), Some(0), "in one part");
        assert_eq!(s.pick(at(7.0, 5.0), 0.1), Some(0), "where three parts overlap");
        assert_eq!(s.pick(at(30.0, 5.0), 0.1), None);
    }

    #[test]
    fn many_features_are_drawn_into_an_image() {
        let geoms: Vec<Geometry<f64>> = vec![
            polygon![(x: 0.0, y: 0.0), (x: 100.0, y: 0.0), (x: 100.0, y: 100.0), (x: 0.0, y: 100.0), (x: 0.0, y: 0.0)].into(),
            line_string![(x: 0.0, y: 150.0), (x: 200.0, y: 150.0)].into(),
            point!(x: 150.0, y: 50.0).into(),
        ];
        let s = store(geoms);
        let view = RenderView { w: 200, h: 200, center: [100.0, 100.0], px_per_unit: 1.0, kx: 1.0 };
        let colors = Colors { stroke: [1, 0, 0, 255], fill: [2, 0, 0, 100], point: [3, 0, 0, 255] };
        let gen = AtomicU64::new(7);
        let img = rasterize(&s, view, colors, 1, &gen, 7).unwrap();
        let px = |x: usize, y: usize| img[(y * 200 + x) * 4];
        assert_eq!(px(50, 150), 2, "inside the area: filled");
        assert_eq!(px(150, 150), 3, "the point");
        assert_eq!(px(100, 50), 1, "the line");
        assert_eq!(px(150, 100), 0, "nothing");
        assert_eq!(px(0, 120), 1, "the outline of the area");
        // A drawing that is no longer wanted stops.
        assert!(rasterize(&s, view, colors, 1, &gen, 6).is_none());
    }
}
