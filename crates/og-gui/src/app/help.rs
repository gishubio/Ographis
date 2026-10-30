//! For someone new to Ographis: the start screen with the latest models and examples, a
//! check for a newer version, and a report of a problem as an issue on GitHub.

use super::*;

/// Where Ographis is published.
pub const REPOSITORY: &str = "https://github.com/gishubio/Ographis";
const LATEST: &str = "https://api.github.com/repos/gishubio/Ographis/releases/latest";

/// Examples worth starting from: the file in the folder of examples, a title, and what it
/// shows.
pub const STARTERS: &[(&str, &str, &str)] = &[
    ("skaraborg.ographis", "Towns and routes", "Read, test, join by attribute and by location, and write three formats"),
    ("raster_tools.ographis", "Slopes, views and distances", "Measure the terrain: slope classes, what can be seen, the distance to water"),
    ("map.ographis", "A map", "Draw layers into a picture with a legend, and write it as PNG and PDF"),
    ("apis/nominatim_addresses.ographis", "Addresses on a map", "Ask a web service for the place of each address (needs the internet)"),
    ("house.ographis", "A house in 3D", "Build a house of walls and roofs, and render it in the afternoon sun"),
    ("chart.ographis", "Charts", "Bars, lines and pies of a table, as pictures"),
];

/// The folder of examples: beside the program, or, while it is built from its code, in the
/// folders above it.
pub fn examples_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent().map(Path::to_path_buf);
    for _ in 0..4 {
        let d = dir?;
        let ex = d.join("examples");
        if ex.join("skaraborg.ographis").exists() {
            return Some(ex);
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    let here = std::env::current_dir().ok()?.join("examples");
    here.join("skaraborg.ographis").exists().then_some(here)
}

/// A version as numbers: "v0.89.1" → [0, 89, 1].
pub fn version_numbers(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-']).map_while(|p| p.parse::<u32>().ok()).collect()
}

/// Whether `latest` is newer than `ours`. Missing parts count as 0: 0.89 is 0.89.0.
pub fn is_newer(latest: &str, ours: &str) -> bool {
    let (a, b) = (version_numbers(latest), version_numbers(ours));
    let n = a.len().max(b.len());
    let at = |v: &Vec<u32>, i: usize| v.get(i).copied().unwrap_or(0);
    (0..n).map(|i| (at(&a, i), at(&b, i))).find(|(x, y)| x != y).is_some_and(|(x, y)| x > y)
}

/// What the check for a newer version has found.
pub enum UpdateCheck {
    Asking(std::sync::mpsc::Receiver<Result<(String, String), String>>),
    /// The latest version and the page of its release.
    Found(String, String),
    Failed(String),
}

/// The latest release on GitHub: its version and its page.
fn ask_latest(backend: &dyn og_engine::io::Backend) -> Result<(String, String), String> {
    let headers = vec![("Accept".to_string(), "application/vnd.github+json".to_string()), ("User-Agent".to_string(), "Ographis".to_string())];
    let a = backend.http_get(LATEST, &headers, 20).map_err(|e| e.0)?;
    if a.status == 404 {
        return Err("No version has been published yet.".into());
    }
    if a.status != 200 {
        return Err(format!("GitHub answered {}.", a.status));
    }
    let v: serde_json::Value = serde_json::from_str(&a.body).map_err(|e| format!("The answer could not be read: {}", e))?;
    let tag = v.get("tag_name").and_then(|t| t.as_str()).ok_or("The answer has no version.")?;
    let page = v.get("html_url").and_then(|t| t.as_str()).unwrap_or(REPOSITORY).to_string();
    Ok((tag.to_string(), page))
}

/// Text put in an address, with what it cannot hold written as %xx.
pub fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// The text of a report of a problem: what to fill in, and the version, the system and the
/// latest errors, which the reporter can read and change before it is sent.
pub fn report_text(versions: &[(String, String)], errors: &[String], fault: Option<&str>) -> String {
    let mut t = String::from("**What happened**\n\n\n**What I expected**\n\n\n**How to make it happen again** (the model, or the steps)\n\n\n**Ographis**\n\n");
    t += &format!("- Ographis {}, built {}\n", env!("CARGO_PKG_VERSION"), env!("OGRAPHIS_BUILT"));
    t += &format!("- {} {}\n", std::env::consts::OS, std::env::consts::ARCH);
    for (name, v) in versions {
        t += &format!("- {} {}\n", name, v);
    }
    if !errors.is_empty() {
        t += "\n**The errors of the latest run**\n\n```\n";
        for e in errors.iter().take(10) {
            t += &format!("{}\n", e.chars().take(400).collect::<String>());
        }
        t += "```\n";
    }
    if let Some(f) = fault {
        t += "\n**The latest fault in Ographis itself**\n\n```\n";
        t += &f.chars().take(2500).collect::<String>();
        t += "\n```\n";
    }
    t
}

/// The latest fault in the log of faults, as written there, without the long trace.
fn latest_fault() -> Option<String> {
    let text = std::fs::read_to_string(og_engine::crash::log_file()?).ok()?;
    let last = text.rsplit("==== ").next()?.trim();
    if last.is_empty() {
        return None;
    }
    Some(last.lines().take(14).collect::<Vec<_>>().join("\n"))
}

impl App {
    /// Help → Check for updates: asks GitHub in the background.
    pub fn check_for_updates(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let backend = self.backend.clone();
        std::thread::spawn(move || {
            let _ = tx.send(ask_latest(&*backend));
        });
        self.update_check = Some(UpdateCheck::Asking(rx));
    }

    /// The window with what the check found.
    pub(super) fn update_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if let Some(UpdateCheck::Asking(rx)) = &self.update_check {
            match rx.try_recv() {
                Ok(Ok((v, page))) => self.update_check = Some(UpdateCheck::Found(v, page)),
                Ok(Err(e)) => self.update_check = Some(UpdateCheck::Failed(e)),
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(_) => self.update_check = Some(UpdateCheck::Failed("The check stopped.".into())),
            }
        }
        let Some(check) = &self.update_check else { return };
        let ours = env!("CARGO_PKG_VERSION");
        let mut close = false;
        let mut open = None;
        let modal = egui::Modal::new(egui::Id::new("update_check")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new("Updates").font(theme::strong(17.0)).color(pal.ink));
            ui.add_space(6.0);
            match check {
                UpdateCheck::Asking(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Asking GitHub for the latest version…");
                    });
                }
                UpdateCheck::Found(v, page) if is_newer(v, ours) => {
                    ui.label(format!("Ographis {} is out. This is {}.", v.trim_start_matches('v'), ours));
                    ui.label(
                        RichText::new("Download it from its page, unpack it, and use it instead of this one. Your settings and models stay where they are.")
                            .color(pal.muted),
                    );
                    ui.add_space(6.0);
                    if ui.button("Open the page of the new version").clicked() {
                        open = Some(page.clone());
                    }
                }
                UpdateCheck::Found(v, _) => {
                    ui.label(format!("This is the latest version, {}.", ours));
                    if is_newer(ours, v) {
                        ui.label(RichText::new(format!("Newer even than the latest published, {}.", v.trim_start_matches('v'))).color(pal.muted));
                    }
                }
                UpdateCheck::Failed(e) => {
                    ui.label("The latest version could not be found out.");
                    ui.label(RichText::new(e).color(pal.muted));
                    if ui.button("Open the page of the versions").clicked() {
                        open = Some(format!("{}/releases", REPOSITORY));
                    }
                }
            }
            ui.add_space(8.0);
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        if let Some(url) = open {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if close || modal.should_close() {
            self.update_check = None;
        }
    }

    /// Help → Report a problem: the report, to read and change, sent as a new issue on GitHub.
    pub fn open_report(&mut self) {
        let errors: Vec<String> =
            self.result.as_ref().map(|r| r.log.iter().filter(|l| l.level == og_engine::Level::Error).map(|l| l.msg.clone()).collect()).unwrap_or_default();
        self.report = Some(("".into(), report_text(&self.backend.versions(), &errors, latest_fault().as_deref())));
    }

    pub(super) fn report_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some((title, body)) = &mut self.report else { return };
        let mut close = false;
        let mut send = None;
        let modal = egui::Modal::new(egui::Id::new("report_problem")).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.label(RichText::new("Report a problem").font(theme::strong(17.0)).color(pal.ink));
            ui.label(
                RichText::new("The report becomes an issue on GitHub, which anyone can read. Read it through and take away what should not be there, such as names of files or servers. You need an account on GitHub to send it.")
                    .color(pal.muted),
            );
            ui.add_space(6.0);
            ui.label("In short");
            ui.add(egui::TextEdit::singleline(title).hint_text("Such as: Visible area gives an empty raster").desired_width(f32::INFINITY));
            ui.add_space(4.0);
            ui.label("The report");
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(body).font(theme::mono(12.0)).desired_width(f32::INFINITY).desired_rows(16));
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Open as an issue on GitHub").on_hover_text("Opens the browser with the report filled in. Nothing is sent until you press Submit there").clicked() {
                    send = Some(format!("{}/issues/new?title={}&body={}", REPOSITORY, url_encode(title.trim()), url_encode(body)));
                }
                if ui.button("Copy the report").on_hover_text("To paste in an e-mail instead").clicked() {
                    ui.ctx().copy_text(format!("{}\n\n{}", title.trim(), body));
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
        if let Some(mut url) = send {
            // Browsers and GitHub take addresses of some length; a longer report is cut, and
            // the whole of it is on the clipboard to paste.
            if url.len() > 7500 {
                ctx.copy_text(body.clone());
                let mut cut = 7500;
                while !url.is_char_boundary(cut) || url[..cut].ends_with('%') || url[..cut.saturating_sub(1)].ends_with('%') {
                    cut -= 1;
                }
                url.truncate(cut);
            }
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if close || modal.should_close() {
            self.report = None;
        }
    }

    /// The start screen on the empty canvas: the latest models, examples to begin from, and
    /// how to start a model of one's own.
    pub(super) fn start_screen_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let at = self.canvas_rect.center();
        let mut open: Option<PathBuf> = None;
        let mut close = false;
        let mut dialog = false;
        let examples = examples_dir();
        egui::Area::new(egui::Id::new("start_screen")).order(egui::Order::Middle).pivot(egui::Align2::CENTER_CENTER).fixed_pos(at).constrain(false).show(
            ctx,
            |ui| {
                egui::Frame::new()
                    .fill(pal.panel)
                    .stroke(egui::Stroke::new(1.0, pal.line))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(24, 18))
                    .show(ui, |ui| {
                        ui.set_width((self.canvas_rect.width() - 80.0).clamp(420.0, 760.0));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Welcome to Ographis").font(theme::strong(20.0)).color(pal.ink));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("×").on_hover_text("Start with an empty canvas. Help → Start screen shows this again").clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.label(RichText::new("Open a model, try an example, or begin a model of your own.").color(pal.muted));
                        ui.add_space(10.0);
                        ui.columns(2, |cols| {
                            let ui = &mut cols[0];
                            ui.label(RichText::new("Begin").font(theme::strong(14.0)).color(pal.ink));
                            if ui.button("Open a model…").clicked() {
                                dialog = true;
                            }
                            if ui
                                .button("An empty model")
                                .on_hover_text("Add an input from the list on the left, or drop a data file or a folder on the canvas")
                                .clicked()
                            {
                                close = true;
                            }
                            ui.add_space(8.0);
                            ui.label(RichText::new("Latest").font(theme::strong(14.0)).color(pal.ink));
                            if self.recent.list.is_empty() {
                                ui.label(RichText::new("The models you open and save are listed here.").small().color(pal.muted));
                            }
                            for path in &self.recent.list {
                                let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                let there = path.exists();
                                // With its folder, since models in different folders may share a name.
                                let folder = path.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(there, egui::Link::new(name)).on_hover_text(path.display().to_string()).clicked() {
                                        open = Some(path.clone());
                                    }
                                    ui.label(RichText::new(folder).small().color(pal.muted));
                                });
                            }
                            let ui = &mut cols[1];
                            ui.label(RichText::new("Examples").font(theme::strong(14.0)).color(pal.ink));
                            match &examples {
                                Some(dir) => {
                                    for (file, title, what) in STARTERS {
                                        let path = dir.join(file);
                                        if !path.exists() {
                                            continue;
                                        }
                                        if ui.link(*title).on_hover_text(path.display().to_string()).clicked() {
                                            open = Some(path);
                                        }
                                        ui.label(RichText::new(*what).small().color(pal.muted));
                                        ui.add_space(2.0);
                                    }
                                    ui.label(RichText::new("Press F5 to run an example. More are in the folder examples.").small().color(pal.muted));
                                }
                                None => {
                                    ui.label(RichText::new("The folder examples is not beside the program.").small().color(pal.muted));
                                }
                            }
                        });
                    });
            },
        );
        if dialog {
            self.request(Pending::Open);
        }
        if let Some(p) = open {
            self.request(Pending::OpenPath(p));
        }
        if close {
            self.start_screen = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_and_reports_written() {
        assert!(is_newer("v0.90", "0.89.0"));
        assert!(is_newer("v0.89.1", "0.89.0"));
        assert!(!is_newer("v0.89", "0.89.0"));
        assert!(!is_newer("v0.88.2", "0.89.0"));
        assert!(is_newer("1.0", "0.99.9"));
        assert_eq!(url_encode("a b&c=å"), "a%20b%26c%3D%C3%A5");
        let r = report_text(&[("GDAL".into(), "3.13.3".into())], &["Input: The file does not exist".into()], None);
        assert!(r.contains("- GDAL 3.13.3") && r.contains("The file does not exist") && r.contains("What happened"));
    }
}
