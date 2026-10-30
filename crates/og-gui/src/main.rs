//! Ographis as a desktop program.
//!
//!     ographis [model.ographis]

// No console window behind the program on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use og_gui::{theme, App};

struct Window {
    app: App,
    title: String,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        let title = og_gui::app::window_title(&self.app);
        if title != self.title {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

fn main() -> eframe::Result {
    og_engine::crash::install(concat!("Ographis ", env!("CARGO_PKG_VERSION")));
    let mut app = App::new();
    app.look_for_recovery();
    match std::env::args().nth(1) {
        Some(path) => {
            if let Err(e) = app.load(std::path::Path::new(&path)) {
                eprintln!("{}", e);
            }
        }
        // Without a model, the start screen shows the latest models and the examples.
        None => app.start_screen = true,
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Ographis").with_inner_size([1600.0, 960.0]).with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Ographis",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(Window { app, title: String::new() }))
        }),
    )
}
