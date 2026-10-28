//! What the tests share: the harness, the example models, pointer and keyboard helpers,
//! and a local tile server.

pub(crate) use eframe::egui;
pub(crate) use egui_kittest::kittest::Queryable;
pub(crate) use egui_kittest::Harness;
pub(crate) use og_engine::io::{Backend, Notes, WriteOpts};
pub(crate) use og_engine::{Feature, Field, FieldType, Table, Value};
pub(crate) use og_gdal::GdalBackend;
pub(crate) use og_gui::app::RunMsg;
pub(crate) use og_gui::App;
pub(crate) use std::io::{Read, Write};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::Arc;

/// An address that refuses the connection at once. The map should then fall back to a grid.
pub(crate) const NO_TILES: &str = "http://127.0.0.1:9/{z}/{x}/{y}.png";

pub(crate) fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The tests run side by side, but take turns with the graphics driver. Setting it up and
/// drawing with it from many threads at once has crashed the software renderer.
pub(crate) static GRAPHICS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn graphics() -> std::sync::MutexGuard<'static, ()> {
    // A test that failed while it held the lock must not stop the others.
    GRAPHICS.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn shot(h: &mut Harness<'_, App>, name: &str) {
    let dir = root().join("target/screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    let _turn = graphics();
    h.render().expect("rendering").save(dir.join(format!("{}.png", name))).unwrap();
}

/// The tests keep what the program remembers between sessions, such as the latest models,
/// in a folder of their own, and not among the settings of the one who runs them.
pub(crate) fn own_settings() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("og-gui-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OGRAPHIS_SETTINGS_DIR", &dir);
    });
}

/// A copy of the examples folder of its own, so that tests running at the same time do
/// not write to the same files.
pub(crate) fn example_copy() -> PathBuf {
    own_settings();
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("og-gui-examples-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    copy_folder(&root().join("examples"), &dir);
    dir
}

/// Copies the files of a folder, and its subfolders except the one with results.
pub(crate) fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let (path, target) = (entry.path(), to.join(entry.file_name()));
        if path.is_file() {
            std::fs::copy(&path, &target).unwrap();
        } else if entry.file_name() != "out" {
            copy_folder(&path, &target);
        }
    }
}

pub(crate) fn harness_for(app: App, theme: egui::Theme) -> Harness<'static, App> {
    own_settings();
    let _turn = graphics();
    Harness::builder().with_size(egui::vec2(1600.0, 960.0)).with_theme(theme).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), app)
}

pub(crate) fn harness_with(workspace: &str, theme: egui::Theme, tiles: &str) -> Harness<'static, App> {
    let mut app = App::new();
    app.load(&example_copy().join(workspace)).unwrap();
    app.map.set_tile_source(tiles);
    harness_for(app, theme)
}

pub(crate) fn harness(theme: egui::Theme, tiles: &str) -> Harness<'static, App> {
    harness_with("skaraborg.ographis", theme, tiles)
}

pub(crate) fn pointer(h: &mut Harness<'_, App>, pos: egui::Pos2, button: Option<(egui::PointerButton, bool)>) {
    h.event(egui::Event::PointerMoved(pos));
    if let Some((button, pressed)) = button {
        h.event(egui::Event::PointerButton { pos, button, pressed, modifiers: Default::default() });
    }
    h.run_steps(2);
}

/// A real mouse click at a point in the window.
pub(crate) fn click_at(h: &mut Harness<'_, App>, pos: egui::Pos2) {
    pointer(h, pos, Some((egui::PointerButton::Primary, true)));
    pointer(h, pos, Some((egui::PointerButton::Primary, false)));
}

/// Presses, moves in a few steps and releases, as a hand would.
pub(crate) fn drag(h: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2, button: egui::PointerButton) {
    pointer(h, from, None);
    pointer(h, from, Some((button, true)));
    for t in [0.2, 0.5, 0.8, 1.0] {
        pointer(h, from + (to - from) * t, None);
    }
    pointer(h, to, Some((button, false)));
}

/// Where an element with a given text is in the window. `nth` picks among several.
/// The name of a node is also in the navigator and sometimes in the panel on the right.
/// The canvas is drawn after them, so on the canvas the last match is taken. Elements there
/// report their place in graph coordinates, which is converted to window coordinates.
pub(crate) fn place_of(h: &Harness<'_, App>, label: &str, nth: usize, on_canvas: bool) -> egui::Pos2 {
    let mut all = h.get_all_by_label(label);
    let found = if on_canvas { all.last() } else { all.nth(nth) };
    let pos = found.unwrap_or_else(|| panic!("cannot find {}", label)).rect().center();
    if on_canvas {
        h.state().graph_to_screen(pos)
    } else {
        pos
    }
}

/// A real mouse click in the middle of the element with a given text.
pub(crate) fn click_text(h: &mut Harness<'_, App>, label: &str, nth: usize, on_canvas: bool) {
    let pos = place_of(h, label, nth, on_canvas);
    click_at(h, pos);
}

/// The ids of the selected nodes, sorted.
pub(crate) fn selected(h: &Harness<'_, App>) -> Vec<String> {
    let app = h.state();
    let mut ids: Vec<String> = app.selected.iter().filter_map(|n| app.snarl.get_node(*n)).map(|n| n.id.clone()).collect();
    ids.sort();
    ids
}

/// A point on the canvas, given in graph coordinates.
pub(crate) fn on_canvas(h: &Harness<'_, App>, x: f32, y: f32) -> egui::Pos2 {
    h.state().graph_to_screen(egui::pos2(x, y))
}

/// Runs some frames with pauses, so that background threads have time to answer.
pub(crate) fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..6 {
        h.run_steps(3);
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    h.run_steps(3);
}

/// A tile server that draws a chequerboard, to try the background mode of the map without a network.
pub(crate) fn tile_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || {
                // Read the whole request before answering, or the client may see a reset.
                let (mut buf, mut n) = ([0u8; 4096], 0);
                while n < buf.len() && !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf[n..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => n += k,
                    }
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/0/0/0.png").trim_end_matches(".png").to_string();
                let parts: Vec<u32> = path.split('/').filter_map(|p| p.parse().ok()).collect();
                let (x, y) = (parts.get(1).copied().unwrap_or(0), parts.get(2).copied().unwrap_or(0));
                let base = if (x + y) % 2 == 0 { [232u8, 238, 226] } else { [214, 226, 236] };
                let img = image::RgbImage::from_fn(256, 256, |px, py| if px == 0 || py == 0 { image::Rgb([150, 160, 150]) } else { image::Rgb(base) });
                let mut png = Vec::new();
                img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len());
                let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&png));
            });
        }
    });
    format!("http://127.0.0.1:{}/{{z}}/{{x}}/{{y}}.png", port)
}

/// A file as the window system hands it over when it is dropped on the window.
#[derive(Debug)]
pub(crate) struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Folds out a list in the palette, if it is folded in.
pub(crate) fn unfold(h: &mut Harness<'_, App>, heading: &str) {
    if !h.state().open_sections.contains(heading) {
        // A heading below the edge of the list has to be brought into view to be clicked.
        h.get_by_label(heading).scroll_to_me();
        h.run_steps(3);
        h.get_by_label(heading).click();
        h.run_steps(3);
    }
    assert!(h.state().open_sections.contains(heading), "{} did not fold out", heading);
}

/// Adds a tool by clicking it in the palette, under the heading it is listed.
pub(crate) fn add_from_palette(h: &mut Harness<'_, App>, heading: &str, tool: &str) {
    unfold(h, heading);
    // The palette is drawn before the canvas, so the first match is the one in the list.
    // A tool far down a long list is scrolled up into view first.
    let mut at = place_of(h, tool, 0, false);
    for _ in 0..10 {
        if at.y < 860.0 {
            break;
        }
        h.event(egui::Event::PointerMoved(egui::pos2(at.x, 700.0)));
        h.run_steps(1);
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        });
        h.run_steps(4);
        at = place_of(h, tool, 0, false);
    }
    click_at(h, at);
    h.run_steps(3);
}

pub(crate) fn drop_files(h: &mut Harness<'_, App>, files: &[PathBuf]) {
    for f in files {
        h.input_mut().dropped_files.push(Arc::new(Dropped(f.clone())));
    }
    h.run_steps(3);
}

// ---------- Version 0.4 ----------

/// An empty folder for one test.
pub(crate) fn scratch(name: &str) -> PathBuf {
    own_settings();
    let dir = std::env::temp_dir().join(format!("og-gui-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small point table in SWEREF 99 TM, with ids starting at `first`.
pub(crate) fn points(first: i64, n: i64) -> Table {
    Table {
        mesh: None,
        fields: vec![Field::new("id", FieldType::Int), Field::new("code", FieldType::Text)],
        crs: Some(3006),
        features: (first..first + n)
            .map(|i| Feature {
                z: None,
                attrs: vec![Value::Int(i), Value::Text(format!("{:04}", i))],
                geom: Some(geo::Geometry::Point(geo::Point::new(300000. + i as f64 * 100., 6400000.))),
            })
            .collect(),
        raster: false,
        image: None,
        grid: None,
    }
}

pub(crate) fn empty_app() -> Harness<'static, App> {
    let mut app = App::new();
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h
}

/// Clicks in a text field and types.
pub(crate) fn type_at(h: &mut Harness<'_, App>, pos: egui::Pos2, text: &str) {
    click_at(h, pos);
    h.event(egui::Event::Text(text.to_string()));
    h.run_steps(3);
}

/// The nodes of the workspace as (name, value of a parameter), sorted by name.
pub(crate) fn param_of(h: &Harness<'_, App>, key: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        h.state().snarl.nodes().map(|n| (n.name.clone(), n.params.get(key).map(|v| v.to_string()).unwrap_or_default())).collect();
    out.sort();
    out
}

/// A click with the secondary button, which opens the menu of what is under the pointer.
pub(crate) fn right_click_at(h: &mut Harness<'_, App>, pos: egui::Pos2) {
    pointer(h, pos, Some((egui::PointerButton::Secondary, true)));
    pointer(h, pos, Some((egui::PointerButton::Secondary, false)));
    h.run_steps(2);
}

/// The value of a parameter of the node with this id, as it is stored.
pub(crate) fn param(h: &Harness<'_, App>, id: &str, key: &str) -> String {
    let node = h.state().snarl.nodes().find(|n| n.id == id).unwrap_or_else(|| panic!("no node {}", id));
    node.params.get(key).map(|v| v.as_str().map_or(v.to_string(), |s| s.to_string())).unwrap_or_default()
}

/// Today's date as the DateTimeStamper writes it.
pub(crate) fn chrono_today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ---------- Version 0.5 ----------

/// A reader, a writer and a connection between them, far enough apart for a node to be
/// put in between. Returns the harness and the folder with the data.
pub(crate) fn two_nodes() -> (Harness<'static, App>, PathBuf) {
    let dir = example_copy();
    std::fs::write(
        dir.join("pair.json"),
        r#"{
      "format": "ographis/1", "name": "Pair",
      "nodes": [
        {"id": "in", "type": "Reader", "name": "Towns", "x": 0, "y": 0, "params": {"path": "towns.csv"}},
        {"id": "out", "type": "Writer", "name": "Result", "x": 700, "y": 0, "params": {"path": "out/pair.csv"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["out", "Input"]}]
    }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("pair.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    app.quiet = true;
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(6);
    (h, dir)
}

/// The connections of the workspace as text: "from.port > to.port".
pub(crate) fn wires(h: &Harness<'_, App>) -> Vec<String> {
    h.state().workspace().edges.iter().map(|e| format!("{}.{} > {}.{}", e.from.0, e.from.1, e.to.0, e.to.1)).collect()
}

/// The middle of the connection between an output port and an input port, in window
/// coordinates. A connection is the same on both sides of its middle, so that is half
/// way between its ends: the right edge of one node and the left edge of the other, at
/// the height of the names of the ports.
pub(crate) fn between(h: &Harness<'_, App>, from: (&str, &str), to: (&str, &str)) -> egui::Pos2 {
    let (a, b) = (h.state().node_rect(from.0).unwrap(), h.state().node_rect(to.0).unwrap());
    // The name of a port on the canvas, in graph coordinates. The one inside the node.
    let port = |name: &str, node: egui::Rect| {
        let found = h.get_all_by_label(name).map(|n| n.rect().center()).find(|c| node.contains(*c));
        found.unwrap_or_else(|| panic!("no port {} in {:?}", name, node)).y
    };
    h.state().graph_to_screen(egui::pos2((a.right() + b.left()) / 2.0, (port(from.1, a) + port(to.1, b)) / 2.0))
}

/// A web server for one test, serving the files it is given by path.
pub(crate) fn file_server(files: Vec<(&'static str, &'static str, Vec<u8>)>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (mut buf, mut n) = ([0u8; 4096], 0);
            while n < buf.len() && !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf[n..]) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => n += k,
                }
            }
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let asked = request.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_string();
            match files.iter().find(|f| f.0 == asked) {
                Some((_, kind, body)) => {
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", kind, body.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                }
                None => {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        }
    });
    format!("http://127.0.0.1:{}", port)
}

// ---------- Version 0.55 ----------

/// Waits until the map has made the stream ready and drawn what it is to draw.
pub(crate) fn wait_for_map(h: &mut Harness<'_, App>) {
    let start = std::time::Instant::now();
    h.run_steps(3);
    while h.state().map.is_busy() && start.elapsed() < std::time::Duration::from_secs(120) {
        std::thread::sleep(std::time::Duration::from_millis(100));
        h.run_steps(2);
    }
    // One more round, so that an image that just came is shown.
    std::thread::sleep(std::time::Duration::from_millis(200));
    h.run_steps(3);
    assert!(!h.state().map.is_busy(), "the map did not finish in two minutes");
}

/// A model with one input whose result is the given table, as if it had been run.
pub(crate) fn app_with_result(table: Table) -> Harness<'static, App> {
    let dir = scratch("result");
    std::fs::write(
        dir.join("m.json"),
        r#"{"format": "ographis/1", "name": "Many", "nodes": [{"id": "in", "type": "Reader", "name": "Roads", "x": 0, "y": 0, "params": {"path": "roads.gpkg"}}], "edges": []}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("m.json")).unwrap();
    app.map.set_tile_source(NO_TILES);
    let node = og_engine::NodeResult {
        status: og_engine::Status::Ok,
        error: None,
        warnings: vec![],
        outputs: [("Output".to_string(), Arc::new(table))].into_iter().collect(),
        inputs: Default::default(),
        files: vec![],
        sources: vec![],
        ms: 0.0,
        ..Default::default()
    };
    app.result = Some(Arc::new(og_engine::RunResult {
        nodes: [("in".to_string(), node)].into_iter().collect(),
        log: vec![],
        order: vec!["in".into()],
        errors: 0,
        warnings: 0,
        ms: 0.0,
    }));
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().focus_node("in");
    h
}

// ---------- Version 0.57 ----------

/// Waits for a run that goes on in its own thread.
pub(crate) fn wait_for_run(h: &mut Harness<'_, App>) {
    let start = std::time::Instant::now();
    // A run may start a few frames after it is asked for, so it is over only when
    // it is still over after some more frames.
    loop {
        while h.state().is_running() && start.elapsed() < std::time::Duration::from_secs(120) {
            std::thread::sleep(std::time::Duration::from_millis(30));
            h.run_steps(1);
        }
        h.run_steps(3);
        if !h.state().is_running() || start.elapsed() >= std::time::Duration::from_secs(120) {
            break;
        }
    }
    assert!(!h.state().is_running(), "the run did not end in two minutes");
}

pub(crate) fn graph_id(h: &Harness<'_, App>, id: &str) -> egui_snarl::NodeId {
    h.state().snarl.nodes_ids_data().find(|(_, n)| n.value.id == id).map(|(n, _)| n).unwrap()
}

// ---------- Version 0.63 ----------

/// Where a port is on the screen: on the left edge of the node for an input, the right edge
/// for an output, level with its name.
pub(crate) fn pin_at(h: &Harness<'_, App>, node: &str, port: &str, input: bool) -> egui::Pos2 {
    let r = h.state().node_rect(node).unwrap();
    let y = h.get_all_by_label(port).map(|n| n.rect().center()).find(|c| r.contains(*c)).unwrap_or_else(|| panic!("no port {} on {}", port, node)).y;
    h.state().graph_to_screen(egui::pos2(if input { r.left() } else { r.right() }, y))
}
