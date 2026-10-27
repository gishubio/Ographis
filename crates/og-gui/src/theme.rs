//! Colours, fonts and measures. A light and a dark theme that follow the system setting.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, Visuals};
use og_engine::nodes::Group;
use std::sync::Arc;

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// All colours of the interface. The neutrals lean slightly towards green, the accent is
/// the magenta overprint of maps, and the node groups have the yellow, blue and green of
/// a topographic map.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub line: Color32,
    pub grid: Color32,
    pub accent: Color32,
    pub accent_ink: Color32,
    pub accent_soft: Color32,
    pub reader: Color32,
    pub reader_soft: Color32,
    pub xform: Color32,
    pub xform_soft: Color32,
    pub writer: Color32,
    pub writer_soft: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub err_soft: Color32,
    pub wire: Color32,
    /// The paper and the edge of a free-standing note.
    pub note: Color32,
    pub note_line: Color32,
}

/// The colours a group box can have: (the name stored in the workspace, the label).
pub const GROUP_COLORS: &[(&str, &str)] = &[
    ("", "Grey"),
    ("yellow", "Yellow"),
    ("orange", "Orange"),
    ("red", "Red"),
    ("pink", "Pink"),
    ("purple", "Purple"),
    ("blue", "Blue"),
    ("teal", "Teal"),
    ("green", "Green"),
    ("brown", "Brown"),
];

/// The edge of the group box colours that have no colour of the nodes behind them.
fn named_edge(color: &str) -> Option<Color32> {
    Some(match color {
        "orange" => Color32::from_rgb(0xd9, 0x7a, 0x1e),
        "red" => Color32::from_rgb(0xc8, 0x3a, 0x32),
        "purple" => Color32::from_rgb(0x7d, 0x4f, 0xb5),
        "teal" => Color32::from_rgb(0x1f, 0x93, 0x8c),
        "brown" => Color32::from_rgb(0x8a, 0x62, 0x3c),
        _ => return None,
    })
}

/// A colour of its own, written as #rrggbb.
pub fn parse_hex(color: &str) -> Option<Color32> {
    let h = color.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        bg: hex(0xe7ecea),
        panel: hex(0xf4f7f6),
        surface: hex(0xffffff),
        ink: hex(0x14201d),
        muted: hex(0x55645f),
        line: hex(0xc6d1cd),
        grid: hex(0xd3dcd8),
        accent: hex(0xa3175c),
        accent_ink: hex(0xffffff),
        accent_soft: hex(0xf7dcea),
        reader: hex(0x7d5600),
        reader_soft: hex(0xf8ebc6),
        xform: hex(0x1c5596),
        xform_soft: hex(0xdbe7f6),
        writer: hex(0x1f6a3d),
        writer_soft: hex(0xd9efe1),
        ok: hex(0x1f6a3d),
        warn: hex(0x8f5600),
        err: hex(0xb3261e),
        err_soft: hex(0xfbe3e1),
        wire: hex(0x3a4542),
        note: hex(0xfff7cf),
        note_line: hex(0xd6bf63),
    };
    pub const DARK: Palette = Palette {
        bg: hex(0x0f1514),
        panel: hex(0x151c1b),
        surface: hex(0x1b2423),
        ink: hex(0xe3ebe8),
        muted: hex(0x97a6a2),
        line: hex(0x2d3937),
        grid: hex(0x232e2c),
        accent: hex(0xf37bb8),
        accent_ink: hex(0x2b0a1c),
        accent_soft: hex(0x4b1a34),
        reader: hex(0xe9c05f),
        reader_soft: hex(0x3a3014),
        xform: hex(0x86b7f0),
        xform_soft: hex(0x1a2b42),
        writer: hex(0x7bcb9a),
        writer_soft: hex(0x163222),
        ok: hex(0x7bcb9a),
        warn: hex(0xe6a94f),
        err: hex(0xf38b83),
        err_soft: hex(0x3d1a18),
        wire: hex(0xb4c4bf),
        note: hex(0x35311a),
        note_line: hex(0x7a6c2e),
    };

    pub fn of(dark: bool) -> Palette {
        if dark {
            Palette::DARK
        } else {
            Palette::LIGHT
        }
    }
    pub fn from_ui(ui: &egui::Ui) -> Palette {
        Palette::of(ui.visuals().dark_mode)
    }
    /// (strong, soft) colour for a node group.
    pub fn group(&self, g: Group) -> (Color32, Color32) {
        match g {
            Group::Reader => (self.reader, self.reader_soft),
            Group::Transformer => (self.xform, self.xform_soft),
            Group::Writer => (self.writer, self.writer_soft),
        }
    }
    /// (edge, fill) for a group box with the given colour name. The fill lets the grid of
    /// the canvas show through.
    pub fn group_box(&self, color: &str) -> (Color32, Color32) {
        // The other colours: the edge as it is, and a fill of it mixed into the background.
        let mixed = |edge: Color32| {
            let m = |a: u8, b: u8| ((a as u16 * 30 + b as u16 * 70) / 100) as u8;
            (edge, Color32::from_rgb(m(edge.r(), self.bg.r()), m(edge.g(), self.bg.g()), m(edge.b(), self.bg.b())))
        };
        let (edge, soft) = match color {
            "yellow" => (self.reader, self.reader_soft),
            "blue" => (self.xform, self.xform_soft),
            "green" => (self.writer, self.writer_soft),
            "pink" => (self.accent, self.accent_soft),
            other => match named_edge(other).or_else(|| parse_hex(other)) {
                Some(c) => mixed(c),
                None => (self.muted, self.line),
            },
        };
        (edge, Color32::from_rgba_unmultiplied(soft.r(), soft.g(), soft.b(), 150))
    }
    pub fn map_fill(&self) -> Color32 {
        let c = self.xform;
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 60)
    }
}

fn visuals(p: Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let radius = CornerRadius::same(4);
    v.panel_fill = p.panel;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.line);
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(6);
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.panel;
    v.code_bg_color = p.bg;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.err;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);

    v.widgets.noninteractive.bg_fill = p.panel;
    v.widgets.noninteractive.weak_bg_fill = p.panel;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.ink);
    v.widgets.noninteractive.corner_radius = radius;

    v.widgets.inactive.bg_fill = p.surface;
    v.widgets.inactive.weak_bg_fill = p.surface;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.ink);
    v.widgets.inactive.corner_radius = radius;

    v.widgets.hovered.bg_fill = p.surface;
    v.widgets.hovered.weak_bg_fill = p.surface;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.muted);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, p.ink);
    v.widgets.hovered.corner_radius = radius;
    v.widgets.hovered.expansion = 0.0;

    v.widgets.active.bg_fill = p.accent_soft;
    v.widgets.active.weak_bg_fill = p.accent_soft;
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.5, p.ink);
    v.widgets.active.corner_radius = radius;
    v.widgets.active.expansion = 0.0;

    v.widgets.open.bg_fill = p.surface;
    v.widgets.open.weak_bg_fill = p.surface;
    v.widgets.open.bg_stroke = Stroke::new(1.0, p.muted);
    v.widgets.open.fg_stroke = Stroke::new(1.0, p.ink);
    v.widgets.open.corner_radius = radius;
    v
}

/// The font family for headings and node names. Semibold Segoe UI on Windows, otherwise
/// the same as the body text.
pub fn strong_family() -> FontFamily {
    FontFamily::Name("strong".into())
}
pub fn strong(size: f32) -> FontId {
    FontId::new(size, strong_family())
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let base = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    defs.families.insert(strong_family(), base);
    // On Windows the system's own fonts are used, so that the program looks at home there.
    let Some(dir) = std::env::var_os("WINDIR").map(|w| std::path::PathBuf::from(w).join("Fonts")) else {
        return defs;
    };
    for (name, file, family) in [
        ("segoe", "segoeui.ttf", FontFamily::Proportional),
        ("segoe-semibold", "seguisb.ttf", strong_family()),
        ("consolas", "consola.ttf", FontFamily::Monospace),
    ] {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            defs.font_data.insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
            defs.families.entry(family).or_default().insert(0, name.to_owned());
        }
    }
    // Without the semibold file the headings still get Segoe UI.
    if defs.font_data.contains_key("segoe") && !defs.font_data.contains_key("segoe-semibold") {
        defs.families.entry(strong_family()).or_default().insert(0, "segoe".to_owned());
    }
    defs
}

/// Makes sure the theme is installed. Returns false in the frame where it is installed,
/// because the fonts only take effect in the next one. Nothing should be drawn then.
pub fn ensure(ctx: &egui::Context) -> bool {
    if ctx.data(|d| d.get_temp::<bool>(egui::Id::new("gb-theme"))).unwrap_or(false) {
        return true;
    }
    install(ctx);
    false
}

/// Sets fonts, measures and both themes. Called once at start.
pub fn install(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("gb-theme"), true));
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(Theme::Light, visuals(Palette::LIGHT, false));
    ctx.set_visuals_of(Theme::Dark, visuals(Palette::DARK, true));
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, strong(17.0)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Monospace, mono(12.5)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.menu_margin = egui::Margin::same(6);
        // Text that can be selected captures clicks and drags. It would then be impossible
        // to pick a row in the table or drag a node by its text, so selection is off everywhere.
        style.interaction.selectable_labels = false;
    });
}
