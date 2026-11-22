//! More charts, in the same quiet look as the bar and line charts: histograms, scatter plots
//! with a trend line, box plots, stacked and grouped bars and population pyramids, and any of
//! them, bars and lines too, as small multiples: a small chart for each group, side by side
//! on one shared scale, so that they can be compared at a glance.

use crate::chart::{bar, nice_step, number, paint, rect, Text, BASELINE, COLOURS, GRID, INK, MUTED, SECONDARY};
use crate::model::{GbError, RasterImage, Result};
use crate::vector::Canvas;
use tiny_skia::{Color, FillRule, PathBuilder, Transform};

/// What one panel shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Panel {
    /// Columns side by side: a name and a value each.
    Bars(Vec<(String, f64)>),
    /// Lines through points: a name and its points, ordered by x.
    Lines(Vec<(String, Vec<(f64, f64)>)>),
    /// The values, counted into bins of equal width.
    Histogram(Vec<f64>),
    /// Points: x, y and the group that gives the colour.
    Scatter(Vec<(f64, f64, usize)>),
    /// The values of each group, as a box from the lower to the upper quartile.
    Boxes(Vec<(String, Vec<f64>)>),
    /// A column per category, with a part for each series: values[series][category].
    Stacked { categories: Vec<String>, values: Vec<Vec<f64>> },
    /// A row per group, the first series to the left and the second to the right.
    Pyramid { groups: Vec<String>, left: Vec<f64>, right: Vec<f64> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plot {
    /// One panel draws one chart; more draw small multiples, each with its name over it.
    pub panels: Vec<(String, Panel)>,
    /// The names of the series, for the legend: of the lines, the parts of a stacked column,
    /// the colours of the points, or the two sides of a pyramid.
    pub series: Vec<String>,
    pub title: String,
    pub subtitle: String,
    pub note: String,
    pub unit: String,
    /// What is along the bottom of a histogram or a scatter plot, and up its side.
    pub x_title: String,
    pub y_title: String,
    pub decimals: usize,
    pub swedish: bool,
    /// The number of bins of a histogram; 0 chooses by the number of values.
    pub bins: usize,
    /// A straight line fitted through the points of a scatter plot, with its R².
    pub trend: bool,
    /// Grouped columns side by side, rather than stacked.
    pub grouped: bool,
    /// Stacked columns as shares of 100 %.
    pub percent: bool,
    /// x is a date, as days since 1970-01-01, for lines.
    pub dates: bool,
    /// A dot at each point of a line.
    pub markers: bool,
    /// Each panel has a scale of its own, rather than all one.
    pub free_scales: bool,
    pub colour: [u8; 3],
    pub width: u32,
    pub height: Option<u32>,
    pub background: Option<[u8; 3]>,
}

impl Default for Plot {
    fn default() -> Plot {
        Plot {
            panels: Vec::new(),
            series: Vec::new(),
            title: String::new(),
            subtitle: String::new(),
            note: String::new(),
            unit: String::new(),
            x_title: String::new(),
            y_title: String::new(),
            decimals: 0,
            swedish: true,
            bins: 0,
            trend: false,
            grouped: false,
            percent: false,
            dates: false,
            markers: false,
            free_scales: false,
            colour: COLOURS[0].2,
            width: 1200,
            height: None,
            background: Some(crate::chart::PAPER),
        }
    }
}

/// The colours in turn, the chosen one first.
pub fn palette(first: [u8; 3]) -> Vec<[u8; 3]> {
    let mut p = vec![first];
    p.extend(COLOURS.iter().map(|c| c.2).filter(|c| *c != first && c != &COLOURS[8].2));
    p
}

/// A value at a fraction of the sorted values, between the two nearest.
pub fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let at = q * (sorted.len() - 1) as f64;
    let (i, f) = (at.floor() as usize, at - at.floor());
    if i + 1 < sorted.len() {
        sorted[i] + (sorted[i + 1] - sorted[i]) * f
    } else {
        sorted[i]
    }
}

/// The least-squares line through the points: (intercept, slope, R²).
pub fn fit_line(pts: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = pts.len() as f64;
    if pts.len() < 2 {
        return None;
    }
    let (mx, my) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let syy: f64 = pts.iter().map(|p| (p.1 - my).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let b = sxy / sxx;
    let r2 = if syy > 0.0 { sxy * sxy / (sxx * syy) } else { 1.0 };
    Some((my - b * mx, b, r2))
}

/// A range made round, with its step.
fn round_range(lo: f64, hi: f64, lines: f64) -> (f64, f64, f64) {
    let (lo, hi) = if hi > lo { (lo, hi) } else { (lo - 1.0, hi + 1.0) };
    let step = nice_step(hi - lo, lines);
    ((lo / step).floor() * step, (hi / step).ceil() * step, step)
}

/// A range a little wider than the values, with a round step for its ticks.
fn snug(r: (f64, f64), lines: f64) -> (f64, f64, f64) {
    let (lo, hi) = if r.1 > r.0 { r } else { (r.0 - 1.0, r.1 + 1.0) };
    let pad = (hi - lo) * 0.04;
    (lo - pad, hi + pad, nice_step(hi - lo, lines))
}

/// The scales a panel is drawn on: x (for numbers along the bottom) and y.
#[derive(Clone, Copy, Debug)]
struct Scales {
    x: (f64, f64, f64),
    y: (f64, f64, f64),
}

/// The edges of the bins of a histogram over lo..hi: round numbers, about `bins` of them.
fn bin_edges(lo: f64, hi: f64, bins: usize) -> Vec<f64> {
    let (lo, hi) = if hi > lo { (lo, hi) } else { (lo - 0.5, hi + 0.5) };
    let step = nice_step(hi - lo, bins as f64);
    let start = (lo / step).floor() * step;
    let mut e = vec![start];
    while *e.last().unwrap_or(&hi) < hi || e.len() < 2 {
        let next = start + step * e.len() as f64;
        e.push(next);
    }
    e
}

fn counts(values: &[f64], edges: &[f64]) -> Vec<f64> {
    let mut c = vec![0.0; edges.len() - 1];
    let step = edges[1] - edges[0];
    for v in values {
        let i = (((v - edges[0]) / step).floor() as usize).min(c.len() - 1);
        c[i] += 1.0;
    }
    c
}

/// The lowest and highest value a panel needs along x and along y.
fn extent(p: &Plot, panel: &Panel, edges: &[f64]) -> ((f64, f64), (f64, f64)) {
    let fold = |it: &mut dyn Iterator<Item = f64>| it.fold((f64::MAX, f64::MIN), |a, v| (a.0.min(v), a.1.max(v)));
    match panel {
        Panel::Bars(b) => ((0.0, 1.0), {
            let (lo, hi) = fold(&mut b.iter().map(|x| x.1));
            (lo.min(0.0), hi.max(0.0))
        }),
        Panel::Lines(s) => {
            let x = fold(&mut s.iter().flat_map(|l| l.1.iter().map(|q| q.0)));
            let y = fold(&mut s.iter().flat_map(|l| l.1.iter().map(|q| q.1)));
            (x, y)
        }
        Panel::Histogram(v) => ((edges[0], edges[edges.len() - 1]), (0.0, counts(v, edges).into_iter().fold(0.0, f64::max))),
        Panel::Scatter(pts) => (fold(&mut pts.iter().map(|q| q.0)), fold(&mut pts.iter().map(|q| q.1))),
        Panel::Boxes(g) => ((0.0, 1.0), fold(&mut g.iter().flat_map(|x| x.1.iter().copied()))),
        Panel::Stacked { values, categories } => {
            let hi = if p.percent {
                100.0
            } else if p.grouped {
                values.iter().flatten().copied().fold(0.0, f64::max)
            } else {
                (0..categories.len()).map(|c| values.iter().map(|s| s[c].max(0.0)).sum::<f64>()).fold(0.0, f64::max)
            };
            ((0.0, 1.0), (0.0, hi))
        }
        Panel::Pyramid { left, right, .. } => ((0.0, 1.0), (0.0, left.iter().chain(right).copied().fold(0.0, f64::max))),
    }
}

fn scales_for(p: &Plot, panel: &Panel, edges: &[f64], xr: (f64, f64), yr: (f64, f64)) -> Scales {
    let lines = 5.0;
    let x = match panel {
        Panel::Histogram(_) => (xr.0, xr.1, edges.get(1).map_or(1.0, |e| e - edges[0])),
        // The points of a scatter plot fill it, with a little air at the edges.
        Panel::Scatter(_) => snug(xr, 6.0),
        // Whole numbers along the bottom, such as years, get whole steps.
        Panel::Lines(s) if s.iter().flat_map(|l| l.1.iter()).all(|q| q.0.fract() == 0.0) => {
            let (a, b, st) = round_range(xr.0, xr.1, 6.0);
            if st < 1.0 {
                (xr.0, xr.1.max(xr.0 + 1.0), 1.0)
            } else {
                (a, b, st)
            }
        }
        _ => round_range(xr.0, xr.1, 6.0),
    };
    let y = match panel {
        Panel::Lines(_) => round_range(yr.0, yr.1, lines),
        Panel::Scatter(_) => snug(yr, lines),
        Panel::Boxes(_) => round_range(yr.0, yr.1, lines),
        Panel::Pyramid { .. } => round_range(0.0, yr.1.max(f64::MIN_POSITIVE), 3.0),
        _ => {
            let (a, b, s) = round_range(yr.0.min(0.0), yr.1.max(0.0), lines);
            (a, b.max(s), s)
        }
    };
    let _ = p;
    Scales { x, y }
}

/// The digits a tick needs at this step.
fn tick_decimals(step: f64) -> usize {
    if step < 1.0 {
        (-step.log10()).ceil() as usize
    } else {
        0
    }
}

struct Ctx<'a> {
    text: &'a Text,
    k: f32,
    p: &'a Plot,
    tick: f32,
    label: f32,
}

impl Ctx<'_> {
    fn num(&self, v: f64, d: usize) -> String {
        number(v, d, self.p.swedish)
    }

    fn with_unit(&self, s: String) -> String {
        if self.p.unit.is_empty() {
            s
        } else {
            format!("{} {}", s, self.p.unit)
        }
    }

    fn x_text(&self, v: f64, step: f64) -> String {
        if self.p.dates {
            crate::chart::date_text(v, step)
        } else {
            // Years are written without a separator for the thousands.
            let s = self.num(v, tick_decimals(step));
            if v.abs() < 10000.0 {
                s.replace(if self.p.swedish { '\u{a0}' } else { ',' }, "")
            } else {
                s
            }
        }
    }
}

/// Draws one panel into the box x, y, w, h.
#[allow(clippy::too_many_arguments)]
fn draw_panel(pm: &mut Canvas, c: &Ctx, name: &str, panel: &Panel, s: Scales, edges: &[f64], x: f32, y: f32, w: f32, h: f32, small: bool, colours: &[[u8; 3]]) {
    let (k, text, p) = (c.k, c.text, c.p);
    let mut top = y;
    if small {
        let t = text.fit(name, c.label, true, w);
        text.draw_on(pm, &t, x, y + c.label, c.label, true, INK, 0.0);
        top += c.label * 1.8;
    }
    let (y0, y1, ystep) = s.y;
    let pyramid = matches!(panel, Panel::Pyramid { .. });
    let categorical = matches!(panel, Panel::Bars(_) | Panel::Boxes(_) | Panel::Stacked { .. });
    // Room to the left for the numbers up the side, or the names of a pyramid's rows.
    let ytick = |v: f64| c.with_unit(c.num(v, tick_decimals(ystep)));
    let left_room = if pyramid {
        match panel {
            Panel::Pyramid { groups, .. } => groups.iter().map(|g| text.width(g, c.tick, false)).fold(0.0f32, f32::max).min(w * 0.25) + 10.0 * k,
            _ => 0.0,
        }
    } else {
        [y0, y1].iter().map(|v| text.width(&ytick(*v), c.tick, false)).fold(0.0f32, f32::max) + 10.0 * k
    };
    let x_title_room = if !p.x_title.is_empty() && !small && matches!(panel, Panel::Histogram(_) | Panel::Scatter(_)) { c.tick * 1.8 } else { 0.0 };
    let (px0, px1) = (x + left_room, x + w - 4.0 * k);
    let (py0, py1) = (top + c.tick * 0.6, y + h - c.tick * 2.2 - x_title_room);
    if px1 - px0 < 20.0 || py1 - py0 < 20.0 {
        return;
    }
    let at_y = |v: f64| py1 - ((v - y0) / (y1 - y0)) as f32 * (py1 - py0);
    let (x0, x1, xstep) = s.x;
    let at_x = |v: f64| px0 + ((v - x0) / (x1 - x0)) as f32 * (px1 - px0);
    let hair = k.max(1.0);

    if !pyramid {
        // Grid lines across, with their numbers.
        let mut v = (y0 / ystep - 1e-9).ceil() * ystep;
        while v <= y1 + ystep * 1e-9 {
            let yy = at_y(v).round() + 0.5;
            rect(pm, px0, yy - 0.5, px1 - px0, hair, if v == 0.0 { BASELINE } else { GRID });
            text.draw_on(pm, &ytick(v), px0 - 6.0 * k, yy + c.tick * 0.35, c.tick, false, MUTED, 1.0);
            v += ystep;
        }
    }
    if !categorical && !pyramid {
        // Ticks along the bottom.
        let step = if let Panel::Histogram(_) = panel {
            // Every edge if they fit, else every second or fifth.
            let room = text.width(&c.x_text(x1, xstep), c.tick, false) * 1.6;
            let n = edges.len() as f32;
            let every = [1usize, 2, 5, 10, 20].into_iter().find(|e| (px1 - px0) / (n / *e as f32) >= room).unwrap_or(50);
            xstep * every as f64
        } else if p.dates {
            let span = x1 - x0;
            [1.0, 7.0, 30.4375, 91.3125, 365.25, 730.5, 1826.25, 3652.5].into_iter().find(|s| span / s <= 6.0).unwrap_or(3652.5)
        } else {
            xstep
        };
        let mut v = (x0 / step).ceil() * step;
        let mut guard = 0;
        while v <= x1 + step * 1e-9 && guard < 200 {
            let xx = at_x(v);
            rect(pm, xx - 0.5, py1, hair, 4.0 * k, BASELINE);
            text.draw_on(pm, &c.x_text(v, step), xx, py1 + c.tick * 1.5, c.tick, false, MUTED, 0.5);
            v += step;
            guard += 1;
        }
        if x_title_room > 0.0 {
            text.draw_on(pm, &p.x_title, (px0 + px1) / 2.0, py1 + c.tick * 3.2, c.tick, false, SECONDARY, 0.5);
        }
    }

    match panel {
        Panel::Bars(bars) => {
            let slot = (px1 - px0) / bars.len().max(1) as f32;
            let thick = (slot * 0.62).max(1.5);
            for (i, (n, v)) in bars.iter().enumerate() {
                let cx = px0 + slot * (i as f32 + 0.5);
                let (a, b) = (at_y(0.0f64.max(y0)), at_y(*v));
                if *v >= 0.0 {
                    bar(pm, cx - thick / 2.0, b, thick, a - b, false, colours[0]);
                } else {
                    rect(pm, cx - thick / 2.0, a, thick, b - a, colours[0]);
                }
                let t = text.fit(n, c.tick, false, slot * 0.96);
                text.draw_on(pm, &t, cx, py1 + c.tick * 1.5, c.tick, false, MUTED, 0.5);
            }
        }
        Panel::Lines(series) => {
            let stroke = tiny_skia::Stroke {
                width: (if small { 2.0 } else { 2.5 }) * k,
                line_join: tiny_skia::LineJoin::Round,
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            };
            for (i, (sname, pts)) in series.iter().enumerate() {
                let at = p.series.iter().position(|n| n == sname).unwrap_or(i);
                let rgb = colours[at % colours.len()];
                let mut pb = PathBuilder::new();
                for (j, (xv, yv)) in pts.iter().enumerate() {
                    if j == 0 {
                        pb.move_to(at_x(*xv), at_y(*yv));
                    } else {
                        pb.line_to(at_x(*xv), at_y(*yv));
                    }
                }
                if pts.len() == 1 {
                    pb.push_circle(at_x(pts[0].0), at_y(pts[0].1), 3.0 * k);
                }
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint(rgb), &stroke, Transform::identity(), None);
                }
                if p.markers {
                    let mut dots = PathBuilder::new();
                    for (xv, yv) in pts {
                        dots.push_circle(at_x(*xv), at_y(*yv), 3.2 * k);
                    }
                    if let Some(path) = dots.finish() {
                        pm.fill_path(&path, &paint(rgb), FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
        }
        Panel::Histogram(values) => {
            let cnt = counts(values, edges);
            let gap = (1.0 * k).max(1.0);
            for (i, n) in cnt.iter().enumerate() {
                if *n <= 0.0 {
                    continue;
                }
                let (a, b) = (at_x(edges[i]), at_x(edges[i + 1]));
                let top = at_y(*n);
                rect(pm, a + gap / 2.0, top, (b - a - gap).max(1.0), at_y(0.0) - top, colours[0]);
            }
        }
        Panel::Scatter(pts) => {
            let r = (if pts.len() > 2000 {
                1.8
            } else if pts.len() > 300 {
                2.6
            } else {
                3.6
            }) * k;
            let mut by: Vec<PathBuilder> = (0..colours.len()).map(|_| PathBuilder::new()).collect();
            for (xv, yv, g) in pts {
                by[g % colours.len()].push_circle(at_x(*xv), at_y(*yv), r);
            }
            for (i, pb) in by.into_iter().enumerate() {
                if let Some(path) = pb.finish() {
                    let mut pt = paint(colours[i]);
                    pt.set_color(Color::from_rgba8(colours[i][0], colours[i][1], colours[i][2], 190));
                    pm.fill_path(&path, &pt, FillRule::Winding, Transform::identity(), None);
                }
            }
            if p.trend {
                let xy: Vec<(f64, f64)> = pts.iter().map(|q| (q.0, q.1)).collect();
                if let Some((a, b, r2)) = fit_line(&xy) {
                    let (lo, hi) = xy.iter().fold((f64::MAX, f64::MIN), |m, q| (m.0.min(q.0), m.1.max(q.0)));
                    let mut pb = PathBuilder::new();
                    pb.move_to(at_x(lo), at_y(a + b * lo));
                    pb.line_to(at_x(hi), at_y(a + b * hi));
                    if let Some(path) = pb.finish() {
                        let stroke = tiny_skia::Stroke { width: 2.0 * k, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
                        pm.stroke_path(&path, &paint(INK), &stroke, Transform::identity(), None);
                    }
                    let d = |v: f64| {
                        let mag = v.abs().max(1e-12).log10().floor() as i32;
                        c.num(v, (2 - mag).clamp(0, 6) as usize)
                    };
                    let label = format!("R² {}  ·  y = {} {} {} x", c.num(r2, 2), d(a), if b < 0.0 { "−" } else { "+" }, d(b.abs()));
                    text.draw_on(pm, &label, px1, py0 + c.label, c.label, true, INK, 1.0);
                }
            }
        }
        Panel::Boxes(groups) => {
            let slot = (px1 - px0) / groups.len().max(1) as f32;
            let half = (slot * 0.3).min(30.0 * k);
            for (i, (gname, vals)) in groups.iter().enumerate() {
                let cx = px0 + slot * (i as f32 + 0.5);
                let t = text.fit(gname, c.tick, false, slot * 0.96);
                text.draw_on(pm, &t, cx, py1 + c.tick * 1.5, c.tick, false, MUTED, 0.5);
                let mut v = vals.clone();
                v.sort_by(|a, b| a.total_cmp(b));
                if v.is_empty() {
                    continue;
                }
                let (q1, med, q3) = (quantile(&v, 0.25), quantile(&v, 0.5), quantile(&v, 0.75));
                let iqr = q3 - q1;
                // Whiskers to the farthest values within one and a half box lengths.
                let lo = v.iter().copied().find(|x| *x >= q1 - 1.5 * iqr).unwrap_or(v[0]);
                let hi = v.iter().rev().copied().find(|x| *x <= q3 + 1.5 * iqr).unwrap_or(v[v.len() - 1]);
                let rgb = colours[0];
                rect(pm, cx - 0.75 * k, at_y(hi), 1.5 * k, at_y(q3) - at_y(hi), SECONDARY);
                rect(pm, cx - 0.75 * k, at_y(q1), 1.5 * k, at_y(lo) - at_y(q1), SECONDARY);
                rect(pm, cx - half * 0.5, at_y(hi) - 0.75 * k, half, 1.5 * k, SECONDARY);
                rect(pm, cx - half * 0.5, at_y(lo) - 0.75 * k, half, 1.5 * k, SECONDARY);
                rect(pm, cx - half, at_y(q3), half * 2.0, (at_y(q1) - at_y(q3)).max(1.0), rgb);
                rect(pm, cx - half, at_y(med) - 1.25 * k, half * 2.0, 2.5 * k, INK);
                // The values beyond the whiskers, as rings.
                let mut pb = PathBuilder::new();
                for x in v.iter().filter(|x| **x < lo || **x > hi) {
                    pb.push_circle(cx, at_y(*x), 3.0 * k);
                }
                if let Some(path) = pb.finish() {
                    let stroke = tiny_skia::Stroke { width: 1.3 * k, ..Default::default() };
                    pm.stroke_path(&path, &paint(SECONDARY), &stroke, Transform::identity(), None);
                }
            }
        }
        Panel::Stacked { categories, values } => {
            let slot = (px1 - px0) / categories.len().max(1) as f32;
            let ns = values.len().max(1);
            let thick = (slot * 0.66).max(2.0);
            for (ci, cname) in categories.iter().enumerate() {
                let cx = px0 + slot * (ci as f32 + 0.5);
                let t = text.fit(cname, c.tick, false, slot * 0.96);
                text.draw_on(pm, &t, cx, py1 + c.tick * 1.5, c.tick, false, MUTED, 0.5);
                let total: f64 = values.iter().map(|s| s[ci].max(0.0)).sum();
                let mut base = 0.0;
                for (si, s) in values.iter().enumerate() {
                    let mut v = s[ci].max(0.0);
                    if p.percent && total > 0.0 {
                        v = v / total * 100.0;
                    }
                    let rgb = colours[si % colours.len()];
                    if p.grouped {
                        let w1 = thick / ns as f32;
                        let xx = cx - thick / 2.0 + w1 * si as f32;
                        let (a, b) = (at_y(0.0), at_y(v));
                        bar(pm, xx + 0.5 * k, b, (w1 - 1.0 * k).max(1.0), a - b, false, rgb);
                    } else {
                        let (a, b) = (at_y(base), at_y(base + v));
                        // A hairline of the paper between the parts.
                        rect(pm, cx - thick / 2.0, b, thick, (a - b - 1.0 * k).max(0.0), rgb);
                        base += v;
                    }
                }
            }
        }
        Panel::Pyramid { groups, left, right } => {
            let mid = (px0 + px1) / 2.0;
            let gap = 4.0 * k;
            let half = (px1 - px0) / 2.0 - gap;
            let at = |v: f64| (v / y1) as f32 * half;
            let band = (py1 - py0) / groups.len().max(1) as f32;
            let thick = (band * 0.8).max(1.0);
            // The scale below each side.
            let mut v = 0.0;
            while v <= y1 + ystep * 1e-9 {
                for side in [-1.0f32, 1.0] {
                    let xx = mid + side * (gap + at(v));
                    rect(pm, xx - 0.5, py0, hair, py1 - py0, if v == 0.0 { BASELINE } else { GRID });
                    if v > 0.0 || side > 0.0 {
                        text.draw_on(pm, &c.with_unit(c.num(v, tick_decimals(ystep))), xx, py1 + c.tick * 1.5, c.tick, false, MUTED, 0.5);
                    }
                }
                v += ystep;
            }
            // The first group at the foot, as ages are drawn.
            for (i, g) in groups.iter().enumerate() {
                let cy = py1 - band * (i as f32 + 0.5);
                let (l, r) = (at(left[i].max(0.0)), at(right[i].max(0.0)));
                rect(pm, mid - gap - l, cy - thick / 2.0, l, thick, colours[0]);
                rect(pm, mid + gap, cy - thick / 2.0, r, thick, colours[1 % colours.len()]);
                let t = text.fit(g, c.tick, false, left_room - 10.0 * k);
                text.draw_on(pm, &t, px0 - 6.0 * k, cy + c.tick * 0.35, c.tick, false, SECONDARY, 1.0);
            }
            if !small {
                for (side, i) in [(-1.0f32, 0usize), (1.0, 1)] {
                    if let Some(n) = p.series.get(i) {
                        text.draw_on(pm, n, mid + side * (gap + half * 0.5), py0 - c.tick * 0.4, c.label, true, colours[i % colours.len()], 0.5);
                    }
                }
            }
        }
    }
}

/// Draws the plot: one chart, or a small chart for each panel in rows.
pub fn draw(p: &Plot) -> Result<RasterImage> {
    if p.panels.is_empty() {
        return Err(GbError("There is nothing to draw.".into()));
    }
    let text = Text::new()?;
    let k = (p.width as f32 / 1200.0).clamp(0.4, 6.0);
    let (title_size, sub_size, tick, label, note_size) = (24.0 * k, 15.0 * k, 13.0 * k, 13.5 * k, 12.0 * k);
    let pad = 28.0 * k;
    let n = p.panels.len();
    let small = n > 1;
    let cols = if small { (n as f32).sqrt().ceil().clamp(2.0, 6.0) as usize } else { 1 };
    let rows = n.div_ceil(cols);
    let w = p.width as f32;
    let mut top = pad;
    if !p.title.is_empty() {
        top += title_size * 1.3;
    }
    if !p.subtitle.is_empty() {
        top += sub_size * 1.5;
    }
    let legend = p.series.len() > 1 && !p.panels.iter().all(|x| matches!(x.1, Panel::Pyramid { .. }) && !small);
    if legend {
        top += label * 1.8;
    }
    top += 14.0 * k;
    let foot = pad + if p.note.is_empty() { 0.0 } else { note_size * 2.0 };
    let gap = 26.0 * k;
    let cell_w = (w - 2.0 * pad - gap * (cols - 1) as f32) / cols as f32;
    let cell_h = if small { (cell_w * 0.68).max(140.0 * k) } else { 0.0 };
    let h = match p.height {
        Some(h) => h as f32,
        None if small => top + rows as f32 * cell_h + (rows - 1) as f32 * gap + foot,
        None => match &p.panels[0].1 {
            Panel::Pyramid { groups, .. } => top + (groups.len() as f32 * 22.0 * k).max(300.0 * k) + tick * 3.0 + foot,
            _ => w * 0.58,
        },
    };
    let cell_h = if small { (h - top - foot - (rows - 1) as f32 * gap) / rows as f32 } else { h - top - foot };
    let mut pm = Canvas::new(p.width, h.round() as u32, true).ok_or_else(|| GbError("There is not memory enough for a chart that large.".into()))?;
    if let Some(bg) = p.background {
        pm.fill(Color::from_rgba8(bg[0], bg[1], bg[2], 255));
    }
    let colours = palette(p.colour);

    // The bins of histograms, the same for every panel.
    let all: Vec<f64> = p.panels.iter().flat_map(|x| if let Panel::Histogram(v) = &x.1 { v.clone() } else { Vec::new() }).collect();
    let edges = if all.is_empty() {
        vec![0.0, 1.0]
    } else {
        let (lo, hi) = all.iter().fold((f64::MAX, f64::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
        let bins = if p.bins > 0 { p.bins } else { ((all.len() as f64).log2().ceil() as usize + 1).clamp(5, 40) };
        bin_edges(lo, hi, bins)
    };
    // One scale for all, from the extents of all the panels together.
    let ext: Vec<((f64, f64), (f64, f64))> = p.panels.iter().map(|x| extent(p, &x.1, &edges)).collect();
    let joined = ext
        .iter()
        .fold(((f64::MAX, f64::MIN), (f64::MAX, f64::MIN)), |a, e| ((a.0 .0.min(e.0 .0), a.0 .1.max(e.0 .1)), (a.1 .0.min(e.1 .0), a.1 .1.max(e.1 .1))));
    let c = Ctx { text: &text, k, p, tick, label };
    for (i, (name, panel)) in p.panels.iter().enumerate() {
        let (xr, yr) = if p.free_scales { ext[i] } else { joined };
        let s = scales_for(p, panel, &edges, xr, yr);
        let (col, row) = (i % cols, i / cols);
        let x = pad + col as f32 * (cell_w + gap);
        let y = top + row as f32 * (cell_h + gap);
        draw_panel(&mut pm, &c, name, panel, s, &edges, x, y, cell_w, cell_h, small, &colours);
    }

    let mut y = pad + title_size * 0.95;
    if !p.title.is_empty() {
        let t = text.fit(&p.title, title_size, true, w - 2.0 * pad);
        text.draw_on(&mut pm, &t, pad, y, title_size, true, INK, 0.0);
        y += sub_size * 1.6;
    } else {
        y = pad + sub_size;
    }
    if !p.subtitle.is_empty() {
        let t = text.fit(&p.subtitle, sub_size, false, w - 2.0 * pad);
        text.draw_on(&mut pm, &t, pad, y, sub_size, false, SECONDARY, 0.0);
        y += sub_size * 1.6;
    }
    if legend {
        // A square of colour and the name of each series, in a row.
        let mut x = pad;
        let y = y + label * 0.2;
        for (i, s) in p.series.iter().enumerate() {
            let tw = text.width(s, label, false);
            if x + tw + 20.0 * k > w - pad {
                break;
            }
            rect(&mut pm, x, y - label * 0.75, label * 0.8, label * 0.8, colours[i % colours.len()]);
            text.draw_on(&mut pm, s, x + label * 1.2, y, label, false, SECONDARY, 0.0);
            x += label * 1.2 + tw + 18.0 * k;
        }
    }
    if !p.note.is_empty() {
        let t = text.fit(&p.note, note_size, false, w - 2.0 * pad);
        text.draw_on(&mut pm, &t, pad, h - pad * 0.7, note_size, false, MUTED, 0.0);
    }
    let mut rgba = Vec::with_capacity((pm.width() * pm.height() * 4) as usize);
    for px in pm.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let vector = pm.picture().map(std::sync::Arc::new);
    Ok(RasterImage { vector, width: pm.width() as usize, height: pm.height() as usize, rgba, geotransform: [0.0, 1.0, 0.0, pm.height() as f64, 0.0, -1.0] })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pixels in the first colour, blue, or blue laid over the paper.
    fn painted(img: &RasterImage, _rgb: [u8; 3]) -> usize {
        img.rgba.chunks_exact(4).filter(|p| p[2] as i32 - p[0] as i32 > 80).count()
    }

    #[test]
    fn every_kind_draws_alone_and_as_small_multiples() {
        let blue = COLOURS[0].2;
        let kinds = vec![
            Panel::Bars(vec![("a".into(), 3.0), ("b".into(), 5.0)]),
            Panel::Lines(vec![("s".into(), vec![(2020.0, 1.0), (2021.0, 3.0)])]),
            Panel::Histogram((0..100).map(|i| (i % 17) as f64).collect()),
            Panel::Scatter((0..50).map(|i| (i as f64, 2.0 * i as f64 + 1.0, 0)).collect()),
            Panel::Boxes(vec![("a".into(), vec![1.0, 2.0, 3.0, 4.0, 40.0]), ("b".into(), vec![2.0, 3.0, 5.0])]),
            Panel::Stacked { categories: vec!["x".into(), "y".into()], values: vec![vec![1.0, 2.0], vec![3.0, 1.0]] },
            Panel::Pyramid { groups: vec!["0-9".into(), "10-19".into()], left: vec![10.0, 12.0], right: vec![11.0, 9.0] },
        ];
        for k in kinds {
            let one = Plot { panels: vec![("".into(), k.clone())], series: vec!["men".into(), "women".into()], trend: true, width: 600, ..Default::default() };
            let img = draw(&one).unwrap();
            assert!(painted(&img, blue) > 20, "{:?}", k);
            let many = Plot { panels: (0..5).map(|i| (format!("panel {}", i), k.clone())).collect(), width: 900, ..one };
            let img = draw(&many).unwrap();
            assert_eq!(img.width, 900);
            assert!(painted(&img, blue) > 100, "{:?}", k);
        }
    }

    #[test]
    fn a_line_fits_and_quartiles_lie_between() {
        let (a, b, r2) = fit_line(&[(0.0, 1.0), (1.0, 3.0), (2.0, 5.0)]).unwrap();
        assert!((a - 1.0).abs() < 1e-12 && (b - 2.0).abs() < 1e-12 && (r2 - 1.0).abs() < 1e-12);
        assert_eq!(quantile(&[1.0, 2.0, 3.0, 4.0, 5.0], 0.25), 2.0);
        assert_eq!(bin_edges(0.0, 16.0, 8), [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0]);
    }
}
