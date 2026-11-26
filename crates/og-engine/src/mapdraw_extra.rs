//! More of the craft of the mapmaker: illuminated contours, rivers that widen downstream,
//! bridges, tunnels and one-way streets, the signs of road numbers, areas faint by their
//! value, the symbols of marsh, forest and rock, and the colours as people with colour
//! blindness see them.

use super::*;

/// The width of each line of a layer, as a share of its width, from `width_by`: a third at
/// the least value and twice at the most. None: all as wide.
pub(super) fn widths(table: &Table, style: &LayerStyle) -> Option<Vec<f32>> {
    let k = table.field_index(&style.width_by)?;
    let values: Vec<Option<f64>> = table.features.iter().map(|f| f.attrs[k].as_f64()).collect();
    let (lo, hi) = values.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if !lo.is_finite() {
        return None;
    }
    Some(
        values
            .iter()
            .map(|v| match v {
                Some(v) if hi > lo => (0.33 + 1.67 * ((v - lo) / (hi - lo)).sqrt()) as f32,
                Some(_) => 1.0,
                None => 0.33,
            })
            .collect(),
    )
}

/// How solid each area of a layer is, from `alpha_by`: faint at the least value and solid
/// at the most, on a square-root scale so that the middle is not lost. None: all solid.
pub(super) fn alphas(table: &Table, style: &LayerStyle) -> Option<Vec<f32>> {
    let k = table.field_index(&style.alpha_by)?;
    let values: Vec<Option<f64>> = table.features.iter().map(|f| f.attrs[k].as_f64()).collect();
    let (lo, hi) = values.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if !lo.is_finite() {
        return None;
    }
    Some(
        values
            .iter()
            .map(|v| match v {
                Some(v) if hi > lo => (0.12 + 0.88 * ((v - lo) / (hi - lo)).sqrt()) as f32,
                Some(_) => 1.0,
                None => 0.12,
            })
            .collect(),
    )
}

pub(super) fn is_tunnel(style: &LayerStyle, k: usize) -> bool {
    style.marks.get(k).is_some_and(|m| m & 2 != 0)
}

fn is_bridge(style: &LayerStyle, k: usize) -> bool {
    style.marks.get(k).is_some_and(|m| m & 1 != 0)
}

fn is_oneway(style: &LayerStyle, k: usize) -> bool {
    style.marks.get(k).is_some_and(|m| m & 4 != 0)
}

/// The lines of a geometry in pixels.
fn lines_px(g: &Geometry<f64>, to_px: &dyn Fn(f64, f64) -> (f32, f32)) -> Vec<Vec<(f32, f32)>> {
    let one = |l: &LineString<f64>| l.0.iter().map(|c| to_px(c.x, c.y)).collect::<Vec<_>>();
    match g {
        Geometry::LineString(l) => vec![one(l)],
        Geometry::MultiLineString(m) => m.0.iter().map(one).collect(),
        Geometry::Line(l) => vec![vec![to_px(l.start.x, l.start.y), to_px(l.end.x, l.end.y)]],
        _ => Vec::new(),
    }
}

/// A line drawn on its own: as wide as its share says, widening from start to end when the
/// layer tapers, or as a tunnel, dashed and pale.
pub(super) fn own_line(pixmap: &mut Canvas, g: &Geometry<f64>, style: &LayerStyle, share: f32, tunnel: bool, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let Some(c) = style.stroke.filter(|_| style.width > 0.0) else { return };
    let width = style.width * share;
    for pts in lines_px(g, to_px) {
        if pts.len() < 2 {
            continue;
        }
        if tunnel {
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].0, pts[0].1);
            pts[1..].iter().for_each(|p| pb.line_to(p.0, p.1));
            if let Some(path) = pb.finish() {
                let dash = tiny_skia::StrokeDash::new(vec![width * 2.5 + 2.0, width * 1.6 + 2.0], 0.0);
                let st = Stroke { width: width.max(0.8), dash, line_cap: tiny_skia::LineCap::Butt, ..Stroke::default() };
                pixmap.stroke_path(&path, &paint([c[0], c[1], c[2], (c[3] as f32 * 0.55) as u8]), &st, Transform::identity(), None);
            }
            continue;
        }
        if !style.taper {
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].0, pts[0].1);
            pts[1..].iter().for_each(|p| pb.line_to(p.0, p.1));
            if let Some(path) = pb.finish() {
                let st = Stroke { width, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                pixmap.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
            }
            continue;
        }
        // Widening along its length: in short pieces, each a little wider than the last.
        let total: f32 = pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum::<f32>().max(1e-3);
        let mut run = 0.0f32;
        for w in pts.windows(2) {
            let len = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            let t = (run + len / 2.0) / total;
            run += len;
            let mut pb = PathBuilder::new();
            pb.move_to(w[0].0, w[0].1);
            pb.line_to(w[1].0, w[1].1);
            if let Some(path) = pb.finish() {
                let st = Stroke { width: width * (0.25 + 0.75 * t), line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
                pixmap.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
            }
        }
    }
}

/// Bridges, over the rest of the layer, with dark edges and wings at their ends; and arrows
/// along one-way streets.
pub(super) fn bridges_and_arrows(pixmap: &mut Canvas, table: &Table, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    if style.marks.is_empty() {
        return;
    }
    let Some(c) = style.stroke else { return };
    let w = style.width.max(1.0) + if style.casing.is_some() { style.casing_width * 2.0 } else { 0.0 };
    let edge = [0x2a, 0x2a, 0x2a, 255];
    for (k, f) in table.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        if is_bridge(style, k) {
            for pts in lines_px(g, to_px) {
                if pts.len() < 2 {
                    continue;
                }
                let mut pb = PathBuilder::new();
                pb.move_to(pts[0].0, pts[0].1);
                pts[1..].iter().for_each(|p| pb.line_to(p.0, p.1));
                let Some(path) = pb.finish() else { continue };
                let butt = |width: f32| Stroke { width, line_cap: tiny_skia::LineCap::Butt, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                pixmap.stroke_path(&path, &paint(edge), &butt(w + 3.4), Transform::identity(), None);
                pixmap.stroke_path(&path, &paint(style.casing.unwrap_or([255, 255, 255, 255])), &butt(w + 1.4), Transform::identity(), None);
                pixmap.stroke_path(&path, &paint(c), &butt(style.width), Transform::identity(), None);
                // Wings: short strokes out from each corner, away from the bridge.
                let ends = [(pts[0], pts[1]), (pts[pts.len() - 1], pts[pts.len() - 2])];
                let mut wings = PathBuilder::new();
                for (at, next) in ends {
                    let (dx, dy) = (next.0 - at.0, next.1 - at.1);
                    let len = dx.hypot(dy).max(1e-3);
                    let (ux, uy) = (dx / len, dy / len);
                    let (nx, ny) = (-uy, ux);
                    let half = (w + 3.4) / 2.0;
                    for side in [-1.0f32, 1.0] {
                        let (sx, sy) = (at.0 + nx * half * side, at.1 + ny * half * side);
                        wings.move_to(sx, sy);
                        wings.line_to(sx + (nx * side - ux) * half * 0.9, sy + (ny * side - uy) * half * 0.9);
                    }
                }
                if let Some(p) = wings.finish() {
                    pixmap.stroke_path(&p, &paint(edge), &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
                }
            }
        }
        if is_oneway(style, k) {
            let arrow = if grey_of(c) > 0.6 { [0x33, 0x33, 0x33, 220] } else { [255, 255, 255, 230] };
            let size = (style.width * 0.8).clamp(3.0, 9.0);
            let mut pb = PathBuilder::new();
            for pts in lines_px(g, to_px) {
                let mut since = 30.0f32;
                for w in pts.windows(2) {
                    let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
                    let len = dx.hypot(dy);
                    if len < 1e-3 {
                        continue;
                    }
                    let (ux, uy) = (dx / len, dy / len);
                    let mut t = 60.0 - since;
                    while t < len {
                        let (x, y) = (w[0].0 + ux * t, w[0].1 + uy * t);
                        pb.move_to(x - ux * size - uy * size * 0.6, y - uy * size + ux * size * 0.6);
                        pb.line_to(x, y);
                        pb.line_to(x - ux * size + uy * size * 0.6, y - uy * size - ux * size * 0.6);
                        t += 60.0;
                    }
                    since = (since + len) % 60.0;
                }
            }
            if let Some(p) = pb.finish() {
                pixmap.stroke_path(
                    &p,
                    &paint(arrow),
                    &Stroke { width: 1.3, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() },
                    Transform::identity(),
                    None,
                );
            }
        }
    }
}

/// Illuminated contours (Tanaka, 1950): the edge of each area between contours light where
/// it faces the light from the north-west, dark where it faces away, and thicker the more
/// squarely it faces either way. Only the outer edges: each is the upper edge of the land
/// below it.
pub(super) fn tanaka(pixmap: &mut Canvas, table: &Table, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let light = (-std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2);
    let base = style.width.max(0.6);
    // Nine shades, from dark to light, each drawn at once.
    let mut buckets: Vec<PathBuilder> = (0..9).map(|_| PathBuilder::new()).collect();
    let mut ring = |pts: &[(f32, f32)]| {
        if pts.len() < 3 {
            return;
        }
        let area: f32 = pts.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum();
        for w in pts.windows(2) {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let len = dx.hypot(dy);
            if len < 1e-3 {
                continue;
            }
            let (nx, ny) = if area > 0.0 { (dy / len, -dx / len) } else { (-dy / len, dx / len) };
            let facing = nx * light.0 + ny * light.1;
            let j = ((facing + 1.0) / 2.0 * 8.0).round().clamp(0.0, 8.0) as usize;
            buckets[j].move_to(w[0].0, w[0].1);
            buckets[j].line_to(w[1].0, w[1].1);
        }
    };
    for f in &table.features {
        let polys: Vec<&geo::Polygon<f64>> = match &f.geom {
            Some(Geometry::Polygon(p)) => vec![p],
            Some(Geometry::MultiPolygon(m)) => m.0.iter().collect(),
            _ => continue,
        };
        for p in polys {
            let pts: Vec<(f32, f32)> = p.exterior().0.iter().map(|c| to_px(c.x, c.y)).collect();
            ring(&pts);
        }
    }
    for (j, pb) in buckets.into_iter().enumerate() {
        let Some(path) = pb.finish() else { continue };
        let facing = j as f32 / 8.0 * 2.0 - 1.0;
        let colour = if facing >= 0.0 { [255, 255, 250, (90.0 + 165.0 * facing) as u8] } else { [40, 36, 32, (90.0 + 165.0 * -facing) as u8] };
        let width = base * (0.5 + 1.3 * facing.abs());
        pixmap.stroke_path(&path, &paint(colour), &Stroke { width, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() }, Transform::identity(), None);
    }
}

/// The colours of the sign of a road number: E roads green, national roads blue, other
/// roads white, each with its text colour.
fn shield_colours(number: &str) -> ([u8; 4], [u8; 3]) {
    let n = number.trim();
    if n.starts_with(['E', 'e']) {
        ([0x00, 0x7a, 0x4d, 255], [255, 255, 255])
    } else if n.parse::<u32>().is_ok_and(|v| v < 100) {
        ([0x00, 0x5c, 0xa9, 255], [255, 255, 255])
    } else {
        ([255, 255, 255, 255], [0x22, 0x22, 0x22])
    }
}

/// Signs with the numbers of roads, every `shield_every` pixels along each road, but not
/// two with the same number close together.
pub(super) fn shields(pixmap: &mut Canvas, table: &Table, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let Some(k) = table.field_index(&style.shield) else { return };
    let Ok(text) = crate::chart::Text::new() else { return };
    let size = (pixmap.width().min(pixmap.height()) as f32 / 90.0).clamp(9.0, 22.0);
    let every = style.shield_every.max(60.0);
    let mut placed: Vec<(f32, f32, String)> = Vec::new();
    for f in &table.features {
        let number = f.attrs[k].to_text();
        let number = number.trim();
        if number.is_empty() {
            continue;
        }
        let Some(g) = &f.geom else { continue };
        for pts in lines_px(g, to_px) {
            let mut run = every / 2.0;
            for w in pts.windows(2) {
                let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
                let len = dx.hypot(dy);
                let mut t = every - run;
                while t <= len {
                    let (x, y) = (w[0].0 + dx / len * t, w[0].1 + dy / len * t);
                    let near = placed.iter().any(|p| (p.2 == number && (p.0 - x).hypot(p.1 - y) < every * 0.8) || (p.0 - x).hypot(p.1 - y) < size * 2.5);
                    let inside = x > size * 2.0 && y > size && x < pixmap.width() as f32 - size * 2.0 && y < pixmap.height() as f32 - size;
                    if !near && inside {
                        placed.push((x, y, number.to_string()));
                    }
                    t += every;
                }
                run = (run + len) % every;
            }
        }
    }
    for (x, y, number) in placed {
        let (fill, ink) = shield_colours(&number);
        let wide = text.width(&number, size, true) + size * 0.8;
        let (h, r) = (size * 1.35, size * 0.25);
        let Some(rect) = tiny_skia::Rect::from_xywh(x - wide / 2.0, y - h / 2.0, wide, h) else { continue };
        let mut pb = PathBuilder::new();
        let (l, t, rr, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        pb.move_to(l + r, t);
        pb.line_to(rr - r, t);
        pb.quad_to(rr, t, rr, t + r);
        pb.line_to(rr, b - r);
        pb.quad_to(rr, b, rr - r, b);
        pb.line_to(l + r, b);
        pb.quad_to(l, b, l, b - r);
        pb.line_to(l, t + r);
        pb.quad_to(l, t, l + r, t);
        pb.close();
        let Some(path) = pb.finish() else { continue };
        pixmap.fill_path(&path, &paint(fill), FillRule::Winding, Transform::identity(), None);
        let edge = if fill[0] > 200 { [0x33, 0x33, 0x33, 255] } else { [255, 255, 255, 255] };
        pixmap.stroke_path(&path, &paint(edge), &Stroke { width: (size / 9.0).max(1.0), ..Stroke::default() }, Transform::identity(), None);
        write_text(pixmap, &text, &number, x, y + size * 0.36, size, true, ink, 0.5);
    }
}

/// The symbols of a pattern over an area, `area` in pixels with `b` its bounds: tufts of
/// marsh, trees, orchards in rows, rock and sand, on a staggered lattice `every` apart.
pub(super) fn symbol_pattern(pixmap: &mut Canvas, area: &geo::Polygon<f64>, b: geo::Rect<f64>, kind: &str, every: f32, colour: [u8; 4]) {
    use geo::Contains;
    let step = match kind {
        "sand" => every.max(3.0) * 0.6,
        "marsh" | "conifers" | "deciduous" | "orchard" => every.max(6.0) * 1.8,
        _ => every.max(4.0) * 1.4,
    } as f64;
    let s = step as f32;
    let (mut lines, mut dots) = (PathBuilder::new(), PathBuilder::new());
    let mut row = 0u32;
    let mut y = (b.min().y / step).floor() * step + step / 2.0;
    while y <= b.max().y {
        // Rows of an orchard are straight; the rest are staggered and a little uneven.
        let shift = if kind == "orchard" || row.is_multiple_of(2) { 0.0 } else { step / 2.0 };
        let mut x = (b.min().x / step).floor() * step + shift;
        let mut col = 0u32;
        while x <= b.max().x {
            let h = ((col.wrapping_mul(73_856_093) ^ row.wrapping_mul(19_349_663)) % 1000) as f32 / 1000.0;
            let (jx, jy) = if kind == "orchard" { (0.0, 0.0) } else { ((h - 0.5) * s * 0.35, (((h * 7.0) % 1.0) - 0.5) * s * 0.3) };
            let (px, py) = (x as f32 + jx, y as f32 + jy);
            if area.contains(&geo::Point::new(px as f64, py as f64)) {
                match kind {
                    "marsh" => {
                        // A short line with three blades of grass up from it.
                        let w = s * 0.32;
                        lines.move_to(px - w, py);
                        lines.line_to(px + w, py);
                        for (dx, tip) in [(-0.5f32, -0.8f32), (0.0, 0.0), (0.5, 0.8)] {
                            lines.move_to(px + dx * w, py);
                            lines.line_to(px + dx * w + tip * w * 0.35, py - w * 0.9);
                        }
                    }
                    "conifers" => {
                        let w = s * 0.2;
                        lines.move_to(px, py + w * 1.2);
                        lines.line_to(px, py + w * 0.3);
                        dots.move_to(px, py - w * 1.4);
                        dots.line_to(px + w, py + w * 0.4);
                        dots.line_to(px - w, py + w * 0.4);
                        dots.close();
                    }
                    "deciduous" => {
                        let w = s * 0.2;
                        lines.move_to(px, py + w * 1.4);
                        lines.line_to(px, py + w * 0.4);
                        lines.push_circle(px, py - w * 0.3, w * 0.8);
                    }
                    "orchard" => {
                        dots.push_circle(px, py, s * 0.13);
                    }
                    "rock" => {
                        // Small sharp triangles, turned this way and that.
                        let w = s * (0.15 + 0.12 * h);
                        let a = h * std::f32::consts::TAU;
                        for k in 0..3 {
                            let t = a + k as f32 * std::f32::consts::TAU / 3.0;
                            if k == 0 {
                                lines.move_to(px + t.cos() * w, py + t.sin() * w);
                            } else {
                                lines.line_to(px + t.cos() * w, py + t.sin() * w);
                            }
                        }
                        lines.close();
                    }
                    _ => {
                        dots.push_circle(px, py, (s * 0.07).max(0.45));
                    }
                }
            }
            x += step;
            col += 1;
        }
        y += step * if kind == "orchard" { 1.0 } else { 0.8 };
        row += 1;
    }
    if let Some(p) = dots.finish() {
        pixmap.fill_path(&p, &paint(colour), FillRule::Winding, Transform::identity(), None);
    }
    if let Some(p) = lines.finish() {
        pixmap.stroke_path(
            &p,
            &paint(colour),
            &Stroke { width: (s / 14.0).max(0.7), line_cap: tiny_skia::LineCap::Round, ..Stroke::default() },
            Transform::identity(),
            None,
        );
    }
}

/// Smooth noise from -1 to 1 over the plane, the same at the same place: for the waver of a
/// line drawn by hand.
pub(super) fn wavy(x: f32, y: f32) -> f32 {
    let h = |i: i32, j: i32| {
        let n = (i.wrapping_mul(374_761_393) ^ j.wrapping_mul(668_265_263)) as u32;
        let n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
        ((n ^ (n >> 16)) & 0xffff) as f32 / 32767.5 - 1.0
    };
    let (i, j) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = h(i, j) + (h(i + 1, j) - h(i, j)) * sx;
    let bottom = h(i, j + 1) + (h(i + 1, j + 1) - h(i, j + 1)) * sx;
    top + (bottom - top) * sy
}

pub(super) fn is_border(style: &LayerStyle) -> bool {
    style.dash.starts_with("border_")
}

/// Borders in their line types, fitted to their corners: each line is cut where it turns
/// sharply, and the dashes of each piece are stretched a little so that a whole number of
/// them fit and a dash sits in each corner, as a mapmaker draws a border by hand.
pub(super) fn borders(pixmap: &mut Canvas, table: &Table, style: &LayerStyle, to_px: &dyn Fn(f64, f64) -> (f32, f32)) {
    let Some(c) = style.stroke.filter(|_| style.width > 0.0) else { return };
    let u = (style.width * 1.6).max(2.0);
    // Each pattern from the middle of its dash: dash, gap, dots and gaps, back to the dash.
    let pattern: Vec<f32> = match style.dash.as_str() {
        "border_country" => vec![u * 7.0, u * 1.8, 0.01, u * 1.8],
        "border_region" => vec![u * 5.0, u * 1.5, 0.01, u * 1.5, 0.01, u * 1.5],
        _ => vec![u * 3.0, u * 1.6],
    };
    let period: f32 = pattern.iter().sum();
    let mut rings: Vec<Vec<(f32, f32)>> = Vec::new();
    for f in &table.features {
        match &f.geom {
            Some(Geometry::Polygon(p)) => {
                rings.extend(std::iter::once(p.exterior()).chain(p.interiors()).map(|r| r.0.iter().map(|q| to_px(q.x, q.y)).collect()))
            }
            Some(Geometry::MultiPolygon(m)) => {
                for p in &m.0 {
                    rings.extend(std::iter::once(p.exterior()).chain(p.interiors()).map(|r| r.0.iter().map(|q| to_px(q.x, q.y)).collect()));
                }
            }
            Some(g) => rings.extend(lines_px(g, to_px)),
            None => {}
        }
    }
    // A border between two areas is in both of them: it is drawn once, or the dashes of the
    // two would fall between each other.
    let key = |a: (f32, f32), b: (f32, f32)| {
        let q = |p: (f32, f32)| ((p.0 * 4.0).round() as i64, (p.1 * 4.0).round() as i64);
        let (a, b) = (q(a), q(b));
        if a <= b {
            (a, b)
        } else {
            (b, a)
        }
    };
    let mut seen = std::collections::HashSet::new();
    let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
    for pts in rings {
        let mut run: Vec<(f32, f32)> = Vec::new();
        for w in pts.windows(2) {
            if seen.insert(key(w[0], w[1])) {
                if run.is_empty() {
                    run.push(w[0]);
                }
                run.push(w[1]);
            } else if run.len() > 1 {
                runs.push(std::mem::take(&mut run));
            } else {
                run.clear();
            }
        }
        if run.len() > 1 {
            runs.push(run);
        }
    }
    let st = |scale: f32| {
        let p: Vec<f32> = pattern.iter().map(|v| (v * scale).max(0.01)).collect();
        // Begin in the middle of a dash, so that a dash runs through the corner.
        let offset = p[0] / 2.0;
        Stroke {
            width: style.width,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            dash: tiny_skia::StrokeDash::new(p, offset),
            ..Stroke::default()
        }
    };
    for pts in runs {
        if pts.len() < 2 {
            continue;
        }
        // Cut where the line turns by more than about 35 degrees.
        let mut piece = vec![pts[0]];
        let mut pieces = Vec::new();
        for i in 1..pts.len() {
            piece.push(pts[i]);
            if i + 1 < pts.len() {
                let (a, b, n) = (pts[i - 1], pts[i], pts[i + 1]);
                let (d1, d2) = ((b.0 - a.0, b.1 - a.1), (n.0 - b.0, n.1 - b.1));
                let (l1, l2) = (d1.0.hypot(d1.1), d2.0.hypot(d2.1));
                if l1 > 0.5 && l2 > 0.5 && (d1.0 * d2.0 + d1.1 * d2.1) / (l1 * l2) < 0.82 {
                    pieces.push(std::mem::replace(&mut piece, vec![pts[i]]));
                }
            }
        }
        pieces.push(piece);
        for piece in pieces {
            let len: f32 = piece.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
            if len < 0.5 {
                continue;
            }
            let n = (len / period).round().max(1.0);
            let scale = len / (n * period);
            let mut pb = PathBuilder::new();
            pb.move_to(piece[0].0, piece[0].1);
            piece[1..].iter().for_each(|p| pb.line_to(p.0, p.1));
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint(c), &st(scale), Transform::identity(), None);
            }
        }
    }
}

/// The size of the profile in a corner: a third of the width, low.
pub(super) fn profile_size(pw: f32, ph: f32, share: f32) -> (f32, f32) {
    let w = (pw * share.clamp(0.15, 0.9)).clamp(160.0, 1600.0);
    (w, (w * 0.42).min(ph * 0.4))
}

/// The heights along a line as a profile in a box: the ground under it, the water over it in
/// the colours of its depth when it is under the sea, heights at round steps on the left
/// with faint lines across, the places along the line over it, the length and how much the
/// heights are made larger under it.
pub(super) fn draw_profile(pixmap: &mut Canvas, pr: &Profile, at: (f32, f32)) -> Result<()> {
    let (w, h) = profile_size(pixmap.width() as f32, pixmap.height() as f32, pr.share);
    let (x0, y0) = at;
    let t = crate::chart::Text::new()?;
    let k = (w / 320.0).clamp(0.7, 3.0);
    let size = 10.0 * k;
    let ink = [0x33, 0x33, 0x33];
    let faint = [0x88, 0x88, 0x88];
    if let Some(r) = tiny_skia::Rect::from_xywh(x0, y0, w, h) {
        let path = PathBuilder::from_rect(r);
        pixmap.fill_path(&path, &paint([0xfb, 0xf8, 0xf2, 236]), FillRule::Winding, Transform::identity(), None);
        pixmap.stroke_path(&path, &paint([0xb0, 0xaa, 0x9e, 255]), &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
    }
    let pts: Vec<(f64, f64)> = pr.points.iter().copied().filter(|p| p.1.is_finite()).collect();
    if pts.len() < 2 {
        return Ok(());
    }
    let (mut lo, mut hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    // Under the sea, the surface is the top; on land, the lowest point the bottom of a ground
    // that is shown down to round steps.
    let sea = lo < 0.0;
    if sea {
        hi = hi.max(0.0);
    }
    let step = crate::chart::nice_step((hi - lo).max(1.0), 4.0);
    lo = (lo / step).floor() * step;
    if !sea {
        hi = (hi / step).ceil() * step;
    }
    let total = pts.last().map_or(1.0, |p| p.0).max(1e-9);
    let label = |z: f64| format!("{} m", if z < 0.0 { crate::model::fmt_count((-z).round() as u64) } else { crate::model::fmt_count(z.round() as u64) });
    let lw = t.width(&label(lo), size, false).max(t.width(&label(hi), size, false)) + size * 0.9;
    let places = !pr.places.is_empty();
    let top = size * if pr.title.is_empty() { 0.8 } else { 2.2 } + if places { size * 1.5 } else { 0.0 };
    let (px0, px1) = (x0 + lw, x0 + w - size * 0.9);
    let (py0, py1) = (y0 + top, y0 + h - size * 2.0);
    let span = (hi - lo).max(1.0);
    let at_xy = |d: f64, z: f64| (px0 + (px1 - px0) * (d / total) as f32, py1 - (py1 - py0) * ((z - lo) / span) as f32);
    // The water, column by column, in the colour of its depth from the surface down to the
    // floor, so that it darkens downwards as on the map.
    if let (true, Some(paint_of)) = (sea, pr.colours.as_ref()) {
        let (sx0, _) = at_xy(0.0, 0.0);
        let cols = ((px1 - sx0).ceil() as usize).max(1);
        let rows = 24;
        let (_, surface) = at_xy(0.0, 0.0);
        for c in 0..cols {
            let d = total * c as f64 / cols as f64;
            let z = height_along(&pts, d);
            if z >= 0.0 {
                continue;
            }
            let (x, floor) = at_xy(d, z);
            for r in 0..rows {
                let (ya, yb) = (surface + (floor - surface) * r as f32 / rows as f32, surface + (floor - surface) * (r + 1) as f32 / rows as f32);
                let zz = z * (r as f64 + 0.5) / rows as f64;
                let col = crate::rasterlook::colour_of(paint_of, zz as f32).unwrap_or([0x6f, 0xa8, 0xd2, 255]);
                if let Some(rect) = tiny_skia::Rect::from_xywh(x, ya, (px1 - px0) / cols as f32 + 0.8, (yb - ya).max(0.5)) {
                    pixmap.fill_rect(rect, &paint(col), Transform::identity(), None);
                }
            }
        }
    }
    // The ground under the line.
    let mut ground = PathBuilder::new();
    ground.move_to(px0, py1);
    let mut line = PathBuilder::new();
    for (i, (d, z)) in pts.iter().enumerate() {
        let (x, y) = at_xy(*d, *z);
        ground.line_to(x, y);
        if i == 0 {
            line.move_to(x, y);
        } else {
            line.line_to(x, y);
        }
    }
    ground.line_to(px1, py1);
    ground.close();
    if let Some(p) = ground.finish() {
        pixmap.fill_path(&p, &paint([0xd9, 0xc8, 0xa4, 255]), FillRule::Winding, Transform::identity(), None);
    }
    if let Some(p) = line.finish() {
        pixmap.stroke_path(
            &p,
            &paint([0x3e, 0x36, 0x2c, 255]),
            &Stroke { width: 1.2 * k, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() },
            Transform::identity(),
            None,
        );
    }
    // Heights at round steps, with faint dashed lines across.
    let dashed = Stroke { width: 0.6 * k, dash: tiny_skia::StrokeDash::new(vec![3.0 * k, 3.0 * k], 0.0), ..Stroke::default() };
    let mut z = lo;
    while z <= hi + step * 1e-6 {
        let (_, y) = at_xy(0.0, z);
        let mut pb = PathBuilder::new();
        pb.move_to(px0, y);
        pb.line_to(px1, y);
        if let Some(p) = pb.finish() {
            pixmap.stroke_path(&p, &paint([0x99, 0x99, 0x99, 160]), &dashed, Transform::identity(), None);
        }
        write_text(pixmap, &t, &label(z), px0 - size * 0.4, y + size * 0.35, size * 0.95, false, ink, 1.0);
        z += step;
    }
    // The places along the line: a thin line down from the name to the profile.
    if places {
        let row = py0 - size * 0.7;
        let mut last_right = f32::NEG_INFINITY;
        for (d, name) in &pr.places {
            let z = height_along(&pts, *d);
            let (x, y) = at_xy(*d, z.max(if sea { lo } else { z }));
            let nw = t.width(name, size * 0.95, false);
            let lx = (x - nw / 2.0).clamp(px0, px1 - nw);
            // Each on its own, or left out where it would run into the one before.
            if lx < last_right + size * 0.4 {
                continue;
            }
            last_right = lx + nw;
            write_text(pixmap, &t, name, lx, row, size * 0.95, false, ink, 0.0);
            let mut pb = PathBuilder::new();
            pb.move_to(x, row + size * 0.3);
            pb.line_to(x, if sea { at_xy(*d, 0.0).1.max(row + size * 0.3) } else { y });
            if let Some(p) = pb.finish() {
                pixmap.stroke_path(&p, &paint([0x77, 0x77, 0x77, 255]), &Stroke { width: 0.7 * k, ..Stroke::default() }, Transform::identity(), None);
            }
        }
    }
    // Under it: A and B, the length and how much higher the heights are drawn than they are.
    let length = if total >= 2000.0 { format!("{} km", crate::model::fmt_count((total / 1000.0).round() as u64)) } else { format!("{:.0} m", total) };
    let px_per_m_x = (px1 - px0) as f64 / total;
    let px_per_m_y = (py1 - py0) as f64 / span;
    let exaggeration = px_per_m_y / px_per_m_x.max(1e-12);
    let foot = y0 + h - size * 0.65;
    write_text(pixmap, &t, "A", px0, foot, size, true, ink, 0.0);
    write_text(pixmap, &t, &format!("B  ·  {}", length), px1, foot, size, false, ink, 1.0);
    if exaggeration > 1.5 {
        let round = crate::chart::nice_step(exaggeration, 1.0).max(1.0);
        let shown = if exaggeration > 20.0 { (exaggeration / (round / 2.0)).round() * (round / 2.0) } else { exaggeration.round() };
        write_text(
            pixmap,
            &t,
            &format!("heights drawn ×{} their scale", crate::model::fmt_count(shown as u64)),
            (px0 + px1) / 2.0,
            foot,
            size * 0.9,
            false,
            faint,
            0.5,
        );
    }
    if !pr.title.is_empty() {
        write_text(pixmap, &t, &pr.title, x0 + size * 0.7, y0 + size * 1.5, size * 1.15, true, ink, 0.0);
    }
    Ok(())
}

/// The height at a distance along a profile, between the points either side of it.
fn height_along(pts: &[(f64, f64)], d: f64) -> f64 {
    let i = pts.partition_point(|p| p.0 < d);
    if i == 0 {
        return pts[0].1;
    }
    if i >= pts.len() {
        return pts[pts.len() - 1].1;
    }
    let (a, b) = (pts[i - 1], pts[i]);
    let f = if b.0 > a.0 { (d - a.0) / (b.0 - a.0) } else { 0.0 };
    a.1 + (b.1 - a.1) * f
}

/// The start and the end of the line of a height profile on the map, marked A and B.
pub(super) fn profile_ends(pixmap: &mut Canvas, pr: &Profile, to_px: &dyn Fn(f64, f64) -> (f32, f32)) -> Result<()> {
    let t = crate::chart::Text::new()?;
    let size = (pixmap.width().min(pixmap.height()) as f32 / 800.0).clamp(0.7, 3.0) * 11.0;
    for (p, name) in pr.ends.iter().zip(["A", "B"]) {
        let (x, y) = to_px(p.0, p.1);
        end_mark(pixmap, &t, name, x, y, size)?;
    }
    Ok(())
}

/// A letter in a small white circle with a dark edge.
fn end_mark(pixmap: &mut Canvas, t: &crate::chart::Text, name: &str, x: f32, y: f32, size: f32) -> Result<()> {
    let r = size * 0.72;
    let mut pb = PathBuilder::new();
    pb.push_circle(x, y, r);
    if let Some(c) = pb.finish() {
        pixmap.fill_path(&c, &paint([255, 255, 255, 240]), FillRule::Winding, Transform::identity(), None);
        pixmap.stroke_path(&c, &paint([0x33, 0x33, 0x33, 255]), &Stroke { width: (size * 0.1).max(1.0), ..Stroke::default() }, Transform::identity(), None);
    }
    let w = t.width(name, size * 0.85, true);
    write_text(pixmap, t, name, x - w / 2.0, y + size * 0.3, size * 0.85, true, [0x22, 0x22, 0x22], 0.0);
    Ok(())
}

/// The land of the world, coarse, from Natural Earth: each ring as longitudes and latitudes.
fn world_land() -> &'static Vec<Vec<(f64, f64)>> {
    static LAND: std::sync::OnceLock<Vec<Vec<(f64, f64)>>> = std::sync::OnceLock::new();
    LAND.get_or_init(|| {
        include_str!("world_land.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                l.split(' ')
                    .filter_map(|p| {
                        let (x, y) = p.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    })
                    .collect()
            })
            .collect()
    })
}

pub(super) fn globe_size(pw: f32, ph: f32) -> f32 {
    (pw.min(ph) * 0.2).clamp(80.0, 420.0)
}

/// A globe seen from straight above the middle of the map (an orthographic projection):
/// the sea, the land, lines of latitude and longitude every 30 degrees, and the map's area in
/// red, or a red ring where it is too small to be seen.
pub(super) fn draw_globe(pixmap: &mut Canvas, g: &Globe, at: (f32, f32)) {
    let d = globe_size(pixmap.width() as f32, pixmap.height() as f32);
    let r = d / 2.0 - 2.0;
    let (cx, cy) = (at.0 + d / 2.0, at.1 + d / 2.0);
    let (l0, p0) = (g.centre.0.to_radians(), g.centre.1.to_radians());
    // Where a place is on the globe, and whether it is on the side that faces us.
    let proj = |lon: f64, lat: f64| -> ((f32, f32), bool) {
        let (l, p) = (lon.to_radians(), lat.to_radians());
        let cos_c = p0.sin() * p.sin() + p0.cos() * p.cos() * (l - l0).cos();
        let x = p.cos() * (l - l0).sin();
        let y = p0.cos() * p.sin() - p0.sin() * p.cos() * (l - l0).cos();
        ((cx + x as f32 * r, cy - y as f32 * r), cos_c >= 0.0)
    };
    let mut sea = PathBuilder::new();
    sea.push_circle(cx, cy, r);
    let Some(disc) = sea.finish() else { return };
    pixmap.fill_path(&disc, &paint([0xd6, 0xe6, 0xf0, 255]), FillRule::Winding, Transform::identity(), None);
    pixmap.set_clip(Some(&disc), false);
    // The land: each ring as far as it is on our side, closed along the edge of the globe.
    let mut land = PathBuilder::new();
    for ring in world_land() {
        let mut open = false;
        for (lon, lat) in ring {
            let ((x, y), seen) = proj(*lon, *lat);
            // A point behind is put on the rim, where the ring goes out of sight.
            let (x, y) = if seen {
                (x, y)
            } else {
                let (dx, dy) = (x - cx, y - cy);
                let n = dx.hypot(dy).max(1e-6);
                (cx + dx / n * r, cy + dy / n * r)
            };
            if open {
                land.line_to(x, y);
            } else {
                land.move_to(x, y);
                open = true;
            }
        }
        land.close();
    }
    if let Some(p) = land.finish() {
        pixmap.fill_path(&p, &paint([0xe8, 0xe2, 0xcf, 255]), FillRule::Winding, Transform::identity(), None);
        pixmap.stroke_path(&p, &paint([0x9a, 0x92, 0x80, 255]), &Stroke { width: 0.6, ..Stroke::default() }, Transform::identity(), None);
    }
    // Lines of latitude and longitude every 30 degrees.
    let mut grat = PathBuilder::new();
    let mut seg = |pts: Vec<(f64, f64)>| {
        let mut open = false;
        for (lon, lat) in pts {
            let ((x, y), seen) = proj(lon, lat);
            if !seen {
                open = false;
                continue;
            }
            if open {
                grat.line_to(x, y);
            } else {
                grat.move_to(x, y);
                open = true;
            }
        }
    };
    for k in -5..=6 {
        let lon = k as f64 * 30.0;
        seg((0..=72).map(|i| (lon, -90.0 + i as f64 * 2.5)).collect());
    }
    for k in -2..=2 {
        let lat = k as f64 * 30.0;
        seg((0..=144).map(|i| (-180.0 + i as f64 * 2.5, lat)).collect());
    }
    if let Some(p) = grat.finish() {
        pixmap.stroke_path(&p, &paint([0x70, 0x80, 0x90, 90]), &Stroke { width: 0.6, ..Stroke::default() }, Transform::identity(), None);
    }
    pixmap.set_clip(None, false);
    // The map's area, or a ring around its place.
    let red = [0xd6, 0x2d, 0x20, 255];
    let pts: Vec<(f32, f32)> = g.area.iter().map(|(lon, lat)| proj(*lon, *lat).0).collect();
    let (minx, maxx) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (miny, maxy) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    if pts.len() > 2 && (maxx - minx).max(maxy - miny) > 5.0 {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        pts[1..].iter().for_each(|p| pb.line_to(p.0, p.1));
        pb.close();
        if let Some(p) = pb.finish() {
            pixmap.fill_path(&p, &paint([0xd6, 0x2d, 0x20, 90]), FillRule::Winding, Transform::identity(), None);
            pixmap.stroke_path(&p, &paint(red), &Stroke { width: 1.4, ..Stroke::default() }, Transform::identity(), None);
        }
    } else {
        let ((x, y), _) = proj(g.centre.0, g.centre.1);
        let mut pb = PathBuilder::new();
        pb.push_circle(x, y, (d * 0.045).max(3.0));
        if let Some(p) = pb.finish() {
            pixmap.stroke_path(&p, &paint(red), &Stroke { width: 2.0, ..Stroke::default() }, Transform::identity(), None);
        }
    }
    pixmap.stroke_path(&disc, &paint([0x55, 0x60, 0x6a, 255]), &Stroke { width: 1.2, ..Stroke::default() }, Transform::identity(), None);
}

/// Names in the middle of the cells of a grid, small and grey, such as the names of sheets.
pub(super) fn draw_cell_names(pixmap: &mut Canvas, names: &[((f64, f64), String)], to_px: &dyn Fn(f64, f64) -> (f32, f32)) -> Result<()> {
    let t = crate::chart::Text::new()?;
    let size = (pixmap.width().min(pixmap.height()) as f32 / 70.0).clamp(9.0, 28.0);
    for ((x, y), name) in names {
        let (u, v) = to_px(*x, *y);
        if u < 0.0 || v < 0.0 || u > pixmap.width() as f32 || v > pixmap.height() as f32 {
            continue;
        }
        halo_text(pixmap, &t, name, u - t.width(name, size, true) / 2.0, v + size * 0.35, size, true, [0x55, 0x55, 0x55, 200]);
    }
    Ok(())
}

/// The colours of the picture as someone with a kind of colour blindness sees them, or in
/// grey, by the matrices of Machado, Oliveira and Fernandes (2009).
pub(super) fn as_seen_by(rgba: &mut [u8], vision: &str) {
    let m: [[f32; 3]; 3] = match vision {
        "deuteranopia" => [[0.367, 0.861, -0.228], [0.280, 0.673, 0.047], [-0.012, 0.043, 0.969]],
        "protanopia" => [[0.152, 1.053, -0.205], [0.115, 0.786, 0.099], [-0.004, -0.048, 1.052]],
        "tritanopia" => [[1.256, -0.077, -0.179], [-0.078, 0.931, 0.148], [0.005, 0.691, 0.304]],
        "grey" => [[0.2126, 0.7152, 0.0722], [0.2126, 0.7152, 0.0722], [0.2126, 0.7152, 0.0722]],
        _ => return,
    };
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    };
    for p in rgba.chunks_exact_mut(4) {
        let c = [lin(p[0]), lin(p[1]), lin(p[2])];
        for (j, row) in m.iter().enumerate() {
            p[j] = srgb(row[0] * c[0] + row[1] * c[1] + row[2] * c[2]);
        }
    }
}

/// The contrast of two colours, from 1 (the same) to 21 (black on white), as WCAG reckons it.
pub(super) fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    let lum = |c: [u8; 3]| {
        let l = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, FieldType, Value};
    use geo::{line_string, polygon};

    fn table(geoms: Vec<Geometry<f64>>, values: Vec<f64>) -> Table {
        Table {
            mesh: None,
            points: None,
            series: None,
            fields: vec![Field::new("v", FieldType::Real)],
            crs: Some(3006),
            features: geoms.into_iter().zip(values).map(|(g, v)| Feature { z: None, attrs: vec![Value::Real(v)], geom: Some(g) }).collect(),
            raster: false,
            image: None,
            grid: None,
        }
    }

    fn colours(c: &Canvas) -> Vec<[u8; 4]> {
        c.pixels()
            .iter()
            .map(|p| {
                let d = p.demultiply();
                [d.red(), d.green(), d.blue(), d.alpha()]
            })
            .collect()
    }

    #[test]
    fn lit_contours_are_light_to_the_north_west_and_dark_away() {
        let hill: Geometry<f64> = polygon![(x: 20.0, y: 20.0), (x: 80.0, y: 20.0), (x: 80.0, y: 80.0), (x: 20.0, y: 80.0), (x: 20.0, y: 20.0)].into();
        let t = table(vec![hill], vec![1.0]);
        let mut c = Canvas::new(100, 100, false).unwrap();
        let style = LayerStyle { width: 2.0, ..Default::default() };
        // The map's y grows north, the picture's down.
        tanaka(&mut c, &t, &style, &|x, y| (x as f32, 100.0 - y as f32));
        let px = colours(&c);
        let at = |x: usize, y: usize| px[y * 100 + x];
        // The north edge (top of the picture) is lit, the south edge dark.
        assert!(at(50, 20)[0] > 200 && at(50, 20)[3] > 0, "{:?}", at(50, 20));
        assert!(at(50, 80)[0] < 80 && at(50, 80)[3] > 0, "{:?}", at(50, 80));
    }

    #[test]
    fn widths_faintness_and_marks_follow_the_values() {
        let l: Geometry<f64> = line_string![(x: 0.0, y: 0.0), (x: 10.0, y: 0.0)].into();
        let t = table(vec![l.clone(), l.clone(), l], vec![1.0, 4.0, 9.0]);
        let style = LayerStyle { width_by: "v".into(), alpha_by: "v".into(), marks: std::sync::Arc::new(vec![1, 2, 4]), ..Default::default() };
        let w = widths(&t, &style).unwrap();
        assert!(w[0] < w[1] && w[1] < w[2] && (w[2] - 2.0).abs() < 1e-4);
        let a = alphas(&t, &style).unwrap();
        assert!(a[0] < a[1] && (a[2] - 1.0).abs() < 1e-4);
        assert!(is_bridge(&style, 0) && is_tunnel(&style, 1) && is_oneway(&style, 2) && !is_tunnel(&style, 0));
        assert_eq!(shield_colours("E20").0, [0x00, 0x7a, 0x4d, 255]);
        assert_eq!(shield_colours("49").0, [0x00, 0x5c, 0xa9, 255]);
        assert_eq!(shield_colours("2690").1, [0x22, 0x22, 0x22]);
    }

    #[test]
    fn symbols_fill_an_area_and_colours_are_seen_as_others_see_them() {
        let area = polygon![(x: 0.0, y: 0.0), (x: 100.0, y: 0.0), (x: 100.0, y: 100.0), (x: 0.0, y: 100.0), (x: 0.0, y: 0.0)];
        let b = geo::Rect::new((0.0, 0.0), (100.0, 100.0));
        for kind in ["marsh", "conifers", "deciduous", "orchard", "rock", "sand"] {
            let mut c = Canvas::new(100, 100, false).unwrap();
            symbol_pattern(&mut c, &area, b, kind, 8.0, [0, 0, 0, 255]);
            let inked = colours(&c).iter().filter(|p| p[3] > 0).count();
            assert!(inked > 100, "{kind}: {inked}");
        }
        let mut red = vec![200u8, 30, 30, 255, 30, 160, 40, 255];
        as_seen_by(&mut red, "grey");
        assert!(red[0] == red[1] && red[1] == red[2]);
        let mut both = vec![200u8, 30, 30, 255, 30, 160, 40, 255];
        as_seen_by(&mut both, "deuteranopia");
        // Red and green come nearer each other.
        let apart = |p: &[u8]| (p[0] as i32 - p[4] as i32).abs() + (p[1] as i32 - p[5] as i32).abs();
        assert!(apart(&both) < apart(&[200, 30, 30, 255, 30, 160, 40, 255]));
        assert!((contrast([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.1);
        assert!(contrast([120, 120, 120], [130, 130, 130]) < 1.2);
    }

    #[test]
    fn spaced_letters_follow_the_angle_and_names_follow_the_coast() {
        // A label straight up the picture: its letters one above the other, not along x.
        let up = Spot { x: 50.0, y: 50.0, angle: -90.0, align: 0.5, glyphs: Vec::new() };
        let g = tracked(up, &[10.0, 10.0, 10.0], 30.0).glyphs;
        assert!((g[0].0 - g[2].0).abs() < 1e-3 && (g[0].1 - g[2].1).abs() > 15.0, "{:?}", g);
        // The line along the inside of the coast of an island.
        let island: Geometry<f64> = polygon![(x: 0.0, y: 0.0), (x: 100.0, y: 0.0), (x: 100.0, y: 60.0), (x: 0.0, y: 60.0), (x: 0.0, y: 0.0)].into();
        let line = coast_line(&island, 5.0).expect("a line inside the coast");
        assert!(line.0.iter().all(|c| c.x >= 4.9 && c.x <= 95.1 && c.y >= 4.9 && c.y <= 55.1));
        assert!(coast_line(&island, 40.0).is_none(), "too small for its name");
    }

    #[test]
    fn small_capitals_and_the_line_of_a_border_in_the_legend() {
        let (l, size) = small_cap('a', true, 10.0);
        assert!(l == "A" && (size - 7.8).abs() < 1e-4);
        assert_eq!(small_cap('G', true, 10.0), ("G".to_string(), 10.0));
        assert_eq!(small_cap('a', false, 10.0), ("a".to_string(), 10.0));
        let county = LayerStyle { dash: "border_region".into(), width: 1.0, ..LayerStyle::default() };
        assert!(county.stroke().dash.is_some(), "the legend shows the dashes of a border");
    }
}
