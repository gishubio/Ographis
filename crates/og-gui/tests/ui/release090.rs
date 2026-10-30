//! What came in 0.90: rasters drawn on maps, the start screen, the measure button, updates
//! and reports of problems.

use crate::common::*;
use eframe::egui;
use og_gui::App;

/// A model that draws what two lookouts see over the example terrain.
fn view_model() -> App {
    let dir = scratch("view090");
    for f in ["terrain.tif", "lookouts.csv"] {
        std::fs::copy(root().join("examples").join(f), dir.join(f)).unwrap();
    }
    std::fs::write(
        dir.join("view.ographis"),
        r#"{"nodes": [
        {"id": "dem", "type": "Reader", "x": 20, "y": 20, "params": {"path": "terrain.tif", "raster_as": "grid"}},
        {"id": "lk", "type": "Reader", "x": 20, "y": 200, "params": {"path": "lookouts.csv", "x": "x", "y": "y", "crs": "EPSG:3006"}},
        {"id": "view", "type": "Viewshed", "x": 280, "y": 20, "params": {"eye": "eye", "radius": "4000", "result": "visible"}},
        {"id": "map", "type": "MapRenderer", "x": 540, "y": 20, "params": {"width": "400"}}],
      "edges": [
        {"from": ["dem", "Output"], "to": ["view", "Heights"]},
        {"from": ["lk", "Output"], "to": ["view", "Places"]},
        {"from": ["view", "Raster"], "to": ["map", "Layers"]}]}"#,
    )
    .unwrap();
    let mut app = App::new();
    app.load(&dir.join("view.ographis")).unwrap();
    app.map.set_tile_source(NO_TILES);
    app
}

#[test]
fn a_raster_layer_is_drawn_in_colours_chosen_for_its_values() {
    let mut h = harness_for(view_model(), egui::Theme::Light);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    h.state_mut().focus_node("map");
    h.run_steps(4);
    // The layer is a raster: it is drawn by its values, a colour each, not filled and lined.
    assert!(h.query_by_label("Draw as").is_some());
    assert!(h.query_by_label("Fill").is_none());
    assert!(h.query_all_by_label("0").count() >= 1 && h.query_all_by_label("1").count() >= 1, "the two values of the raster");
    // Not seen is not drawn.
    h.get_all_by_label("0").last().unwrap().click();
    h.run_steps(3);
    let layers = h.state().snarl.nodes().find(|n| n.id == "map").unwrap().params["layers"].clone();
    assert_eq!(layers[0]["raster_colors"][0][0], "0");
    assert_eq!(layers[0]["raster_colors"][0][1], "none");
    shot(&mut h, "090-raster-layer");
    h.state_mut().run_blocking();
    settle(&mut h);
    let r = h.state().result.clone().unwrap();
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let img = r.output("map", "Map").unwrap().image.clone().unwrap();
    // Most of the map is not seen, so it is the white background.
    let white = img.rgba.chunks_exact(4).filter(|p| p[0] == 255 && p[1] == 255 && p[2] == 255).count();
    assert!(white * 2 > img.width * img.height, "{} of {}", white, img.width * img.height);
}

#[test]
fn the_start_screen_opens_an_example_and_goes_away() {
    let mut app = App::new();
    app.start_screen = true;
    app.map.set_tile_source(NO_TILES);
    let mut h = harness_for(app, egui::Theme::Light);
    h.run_steps(4);
    assert!(h.query_by_label("Welcome to Ographis").is_some());
    shot(&mut h, "090-start-screen");
    h.get_by_label("Towns and routes").click();
    h.run_steps(6);
    assert_eq!(h.state().snarl.nodes().count(), 18, "the example is open");
    assert!(h.query_by_label("Welcome to Ographis").is_none());
    // Help → Start screen shows it again, on a new model.
    h.get_by_label("Help").click();
    h.run_steps(3);
    h.get_by_label("Start screen").click();
    h.run_steps(6);
    assert!(h.query_by_label("Welcome to Ographis").is_some());
    h.get_by_label("An empty model").click();
    h.run_steps(3);
    assert!(h.query_by_label("Welcome to Ographis").is_none() && !h.state().start_screen);
    assert!(h.query_by_label("The canvas is empty").is_some());
}

#[test]
fn a_problem_is_reported_with_the_version_and_the_errors() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.get_by_label("Help").click();
    h.run_steps(3);
    h.get_by_label("Report a problem…").click();
    h.run_steps(4);
    let (_, body) = h.state().report.clone().expect("the report is open");
    assert!(body.contains(&format!("Ographis {}", env!("CARGO_PKG_VERSION"))) && body.contains("GDAL"), "{}", body);
    assert!(h.query_by_label("Open as an issue on GitHub").is_some());
    shot(&mut h, "090-report");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.state().report.is_none());
}

#[test]
fn the_measure_button_starts_measuring() {
    let mut h = harness(egui::Theme::Light, NO_TILES);
    h.run_steps(4);
    h.state_mut().run_blocking();
    settle(&mut h);
    h.state_mut().focus_node("towns");
    h.run_steps(4);
    h.get_by_label("Measure").click();
    h.run_steps(3);
    h.get_by_label("An area").click();
    h.run_steps(3);
    assert!(h.state().map.measuring.as_ref().is_some_and(|m| m.what == og_gui::maptools::Measure::Area));
    assert!(h.query_by_label("Measuring…").is_some());
}
