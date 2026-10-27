//! The icons of the toolbar, drawn as thin outlines in the manner of Windows 11, so that they
//! look the same on every computer and need no files.

use crate::theme::Palette;
use eframe::egui::{self, pos2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo, WidgetType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    New,
    Open,
    Save,
    Run,
    Stop,
    Pause,
    Undo,
    Redo,
}

/// Draws an icon in a square. The drawing is made on a grid of 16 by 16.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, ink: Color32, accent: Color32) {
    let s = rect.width() / 16.0;
    let at = |x: f32, y: f32| -> Pos2 { rect.min + Vec2::new(x * s, y * s) };
    let stroke = Stroke::new(1.3 * s.max(1.0), ink);
    let line = |pts: &[(f32, f32)]| Shape::line(pts.iter().map(|(x, y)| at(*x, *y)).collect(), stroke);
    match icon {
        Icon::New => {
            painter.add(line(&[(3.5, 1.5), (9.5, 1.5), (12.5, 4.5), (12.5, 14.5), (3.5, 14.5), (3.5, 1.5)]));
            painter.add(line(&[(9.5, 1.5), (9.5, 4.5), (12.5, 4.5)]));
        }
        Icon::Open => {
            painter.add(line(&[(1.5, 12.5), (1.5, 3.0), (6.0, 3.0), (7.5, 4.5), (13.0, 4.5), (13.0, 7.0)]));
            painter.add(line(&[(1.5, 12.5), (4.0, 7.0), (15.0, 7.0), (12.5, 12.5), (1.5, 12.5)]));
        }
        Icon::Save => {
            painter.add(line(&[(2.0, 2.0), (11.5, 2.0), (14.0, 4.5), (14.0, 14.0), (2.0, 14.0), (2.0, 2.0)]));
            painter.add(line(&[(5.0, 2.0), (5.0, 5.5), (10.5, 5.5), (10.5, 2.0)]));
            painter.add(line(&[(4.5, 14.0), (4.5, 9.0), (11.5, 9.0), (11.5, 14.0)]));
        }
        Icon::Run => {
            let pts = vec![at(4.5, 2.5), at(13.0, 8.0), at(4.5, 13.5)];
            painter.add(Shape::convex_polygon(pts, accent, Stroke::new(1.0 * s.max(1.0), accent)));
        }
        Icon::Stop => {
            let r = Rect::from_min_max(at(3.5, 3.5), at(12.5, 12.5));
            painter.rect_filled(r, 1.5 * s, if ink == accent { ink } else { Color32::from_rgb(196, 43, 28) });
        }
        Icon::Pause => {
            let fill = if ink == accent { ink } else { Color32::from_rgb(214, 140, 0) };
            painter.rect_filled(Rect::from_min_max(at(4.0, 3.0), at(7.0, 13.0)), 1.0 * s, fill);
            painter.rect_filled(Rect::from_min_max(at(9.0, 3.0), at(12.0, 13.0)), 1.0 * s, fill);
        }
        Icon::Undo | Icon::Redo => {
            // An arc over the top, from the left to the lower right, with the head of the
            // arrow at its start. Redo is the same, mirrored.
            let flip = |p: Pos2| if icon == Icon::Redo { pos2(rect.min.x + rect.max.x - p.x, p.y) } else { p };
            let (cx, cy, r) = (8.5, 9.0, 4.8);
            let arc: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = (180.0 - 225.0 * i as f32 / 24.0).to_radians();
                    flip(at(cx + r * a.cos(), cy - r * a.sin()))
                })
                .collect();
            let start = arc[0];
            painter.add(Shape::line(arc, stroke));
            let tip = start + Vec2::new(0.0, 1.5 * s);
            let wing = 3.0 * s;
            painter.add(Shape::line(vec![tip + Vec2::new(-wing, -wing), tip, tip + Vec2::new(wing, -wing)], stroke));
        }
    }
}

/// A button with an icon and no text. The text is shown when the pointer rests on it, and it
/// is the name the button has for screen readers.
pub fn button(ui: &mut Ui, pal: &Palette, icon: Icon, name: &str, hint: &str, enabled: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 30.0), if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if enabled && (response.hovered() || response.has_focus()) {
            let fill = if response.is_pointer_button_down_on() { pal.line } else { pal.bg };
            painter.rect_filled(rect, 4.0, fill);
        }
        let (ink, accent) = if enabled { (pal.ink, pal.ok) } else { (pal.line, pal.line) };
        let r = Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0));
        paint(painter, r, icon, ink, accent);
    }
    response.on_hover_text(hint)
}
