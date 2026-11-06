//! How long large data takes, and how much memory: fixed cases made of made-up data, the same
//! every time, so that a version that is slower or takes more than the last shows. Not run
//! with the other tests; run them, in a release build, with
//!
//! ```text
//! cargo test --release -p og-gdal --test performance -- --ignored --nocapture
//! ```
//!
//! Each case runs in a process of its own, so that the peak of memory is that case's alone.
//! The results are printed as a table, and written to target/performance.csv. See
//! docs/PERFORMANCE.md for what they were for each version.

use og_engine::cloud::Cloud;
use og_engine::io::{Backend, Notes, WriteOpts};
use og_engine::model::{Grid, Table};
use og_engine::Workspace;
use og_gdal::GdalBackend;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A number that looks random, the same for the same seed.
fn noise(seed: u64) -> f64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// The largest the memory of this process has been, in megabytes, where it can be told.
fn peak_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

/// The data of a case, made before the clock starts.
fn prepare(case: &str, dir: &Path) {
    let b = GdalBackend::new();
    match case {
        "points_join" => {
            // A million points over 100 × 100 km, joined to the 10 × 10 squares they lie in.
            let mut csv = String::from("id;x;y;value\n");
            for i in 0..1_000_000u64 {
                csv.push_str(&format!(
                    "{};{:.1};{:.1};{:.2}\n",
                    i,
                    300_000.0 + noise(i) * 100_000.0,
                    6_400_000.0 + noise(i + 7) * 100_000.0,
                    noise(i + 13) * 100.0
                ));
            }
            std::fs::write(dir.join("points.csv"), csv).unwrap();
            let mut zones = String::from("zone;wkt\n");
            for r in 0..10 {
                for c in 0..10 {
                    let (x, y) = (300_000.0 + c as f64 * 10_000.0, 6_400_000.0 + r as f64 * 10_000.0);
                    zones.push_str(&format!(
                        "z{}_{};POLYGON(({x} {y},{} {y},{} {},{x} {},{x} {y}))\n",
                        r,
                        c,
                        x + 10_000.0,
                        x + 10_000.0,
                        y + 10_000.0,
                        y + 10_000.0
                    ));
                }
            }
            std::fs::write(dir.join("zones.csv"), zones).unwrap();
        }
        "polygons_buffer" => {
            // 200 000 small squares, as properties.
            let mut csv = String::from("id;wkt\n");
            for i in 0..200_000u64 {
                let (x, y) = (300_000.0 + (i % 500) as f64 * 40.0, 6_400_000.0 + (i / 500) as f64 * 40.0);
                let s = 10.0 + noise(i) * 20.0;
                csv.push_str(&format!("{};POLYGON(({x} {y},{} {y},{} {},{x} {},{x} {y}))\n", i, x + s, x + s, y + s, y + s));
            }
            std::fs::write(dir.join("squares.csv"), csv).unwrap();
        }
        "raster_slope" => {
            // A terrain of 4000 × 4000 cells of 2 m: 16 million heights.
            let (w, h) = (4000usize, 4000usize);
            let values: Vec<f32> = (0..w * h)
                .map(|k| {
                    let (c, r) = ((k % w) as f64, (k / w) as f64);
                    (100.0 + 30.0 * (c / 300.0).sin() * (r / 410.0).cos() + 5.0 * noise(k as u64)) as f32
                })
                .collect();
            let g = Grid {
                width: w,
                height: h,
                values,
                geotransform: [400_000.0, 2.0, 0.0, 6_480_000.0, 0.0, -2.0],
                crs: Some(3006),
                bands: Vec::new(),
                meta: Default::default(),
            };
            b.write(&g.into_table(), &dir.join("terrain.tif"), &WriteOpts::default(), &mut Notes::default()).unwrap();
        }
        "cloud_raster" => {
            // Five million points over 1 × 1 km, as a laser scan of 5 points a square metre.
            let n = 5_000_000usize;
            let mut c = Cloud::with_capacity(n, false);
            for i in 0..n {
                let (x, y) = (noise(i as u64) * 1000.0, noise(i as u64 + 3) * 1000.0);
                c.x.push(400_000.0 + x);
                c.y.push(6_480_000.0 + y);
                c.z.push(100.0 + (x / 50.0).sin() * 5.0 + noise(i as u64 + 5) * 0.3);
                c.class.push(2);
                c.intensity.push((noise(i as u64 + 9) * 1000.0) as u16);
                c.ret.push(1);
                c.rets.push(1);
            }
            let t = Table { crs: Some(3006), points: Some(std::sync::Arc::new(c)), ..Default::default() };
            og_engine::pointcloud::write_las(&t, &dir.join("scan.laz"), None).unwrap();
        }
        other => panic!("no case {}", other),
    }
}

fn model(case: &str) -> &'static str {
    match case {
        "points_join" => {
            r#"{"nodes": [
            {"id": "points", "type": "Reader", "params": {"path": "points.csv", "x": "x", "y": "y", "crs": "EPSG:3006"}},
            {"id": "zones", "type": "Reader", "params": {"path": "zones.csv", "crs": "EPSG:3006"}},
            {"id": "join", "type": "SpatialJoiner", "params": {"relation": "within"}},
            {"id": "out", "type": "Writer", "params": {"path": "out/joined.gpkg"}}],
          "edges": [{"from": ["points", "Output"], "to": ["join", "Features"]}, {"from": ["zones", "Output"], "to": ["join", "Join"]},
            {"from": ["join", "Joined"], "to": ["out", "Input"]}]}"#
        }
        "polygons_buffer" => {
            r#"{"nodes": [
            {"id": "squares", "type": "Reader", "params": {"path": "squares.csv", "crs": "EPSG:3006"}},
            {"id": "area", "type": "AreaCalculator", "params": {}},
            {"id": "buffer", "type": "Bufferer", "params": {"distance": "2"}},
            {"id": "out", "type": "Writer", "params": {"path": "out/buffered.gpkg"}}],
          "edges": [{"from": ["squares", "Output"], "to": ["area", "Input"]}, {"from": ["area", "Output"], "to": ["buffer", "Input"]},
            {"from": ["buffer", "Buffered"], "to": ["out", "Input"]}]}"#
        }
        "raster_slope" => {
            r#"{"nodes": [
            {"id": "dem", "type": "Reader", "params": {"path": "terrain.tif", "raster_as": "grid"}},
            {"id": "slope", "type": "TerrainAnalyzer", "params": {"measure": "slope"}},
            {"id": "out", "type": "Writer", "params": {"path": "out/slope.tif"}}],
          "edges": [{"from": ["dem", "Output"], "to": ["slope", "Heights"]}, {"from": ["slope", "Raster"], "to": ["out", "Input"]}]}"#
        }
        "cloud_raster" => {
            r#"{"nodes": [
            {"id": "scan", "type": "Reader", "params": {"path": "scan.laz"}},
            {"id": "dem", "type": "PointCloudRasterizer", "params": {"cell": "1", "stat": "mean"}},
            {"id": "out", "type": "Writer", "params": {"path": "out/dem.tif"}}],
          "edges": [{"from": ["scan", "Output"], "to": ["dem", "Points"]}, {"from": ["dem", "Raster"], "to": ["out", "Input"]}]}"#
        }
        other => panic!("no case {}", other),
    }
}

const CASES: &[(&str, &str)] = &[
    ("points_join", "1 000 000 points from CSV joined to 100 squares, to GeoPackage"),
    ("polygons_buffer", "200 000 areas from CSV, area and a buffer, to GeoPackage"),
    ("raster_slope", "Slope of 4000 × 4000 heights, GeoTIFF to GeoTIFF"),
    ("cloud_raster", "5 000 000 points from LAZ to a 1 m terrain model"),
];

/// One case, in this process: prepares, runs, and prints a line the parent reads.
fn one(case: &str) {
    let dir = std::env::temp_dir().join(format!("og-perf-{}-{}", case, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    prepare(case, &dir);
    let before = peak_mb();
    let ws = Workspace::from_json(model(case)).unwrap();
    let b = GdalBackend::new();
    let start = Instant::now();
    // As og run does it: what a step made is let go once the steps after it have used it.
    let r = og_engine::run_opts(&ws, Some(&b), &dir, &mut |_| {}, og_engine::RunOptions { lean: true, ..Default::default() });
    let seconds = start.elapsed().as_secs_f64();
    assert_eq!(r.errors, 0, "{}: {:?}", case, r.log);
    let peak = peak_mb();
    let _ = std::fs::remove_dir_all(&dir);
    // The peak while preparing may be higher than the run's; then the run's is not known.
    let run_peak = match (before, peak) {
        (Some(a), Some(b)) if b > a => format!("{:.0}", b),
        (Some(_), Some(b)) => format!("<{:.0}", b),
        _ => "–".to_string(),
    };
    println!("RESULT;{};{:.2};{}", case, seconds, run_peak);
}

#[test]
#[ignore]
fn performance() {
    // The child: one case.
    if let Ok(case) = std::env::var("OG_PERF_CASE") {
        one(&case);
        return;
    }
    let only = std::env::var("OG_PERF_ONLY").ok();
    let exe = std::env::current_exe().unwrap();
    let mut rows = Vec::new();
    for (case, what) in CASES {
        if only.as_deref().is_some_and(|o| !o.split(',').any(|x| x == *case)) {
            continue;
        }
        let out = std::process::Command::new(&exe)
            .args(["performance", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env("OG_PERF_CASE", case)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text
            .lines()
            .find_map(|l| l.find("RESULT;").map(|at| &l[at..]))
            .unwrap_or_else(|| panic!("{} gave no result:\n{}\n{}", case, text, String::from_utf8_lossy(&out.stderr)));
        let parts: Vec<&str> = line.split(';').collect();
        rows.push((case.to_string(), what.to_string(), parts[2].to_string(), parts[3].to_string()));
    }
    let version = env!("CARGO_PKG_VERSION");
    println!("\n| Case | What | Seconds | Peak memory, MB |\n| --- | --- | --- | --- |");
    for (case, what, s, m) in &rows {
        println!("| {} | {} | {} | {} |", case, what, s, m);
    }
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/performance.csv");
    let mut csv = String::from("version;case;seconds;peak_mb\n");
    for (case, _, s, m) in &rows {
        csv.push_str(&format!("{};{};{};{}\n", version, case, s, m));
    }
    let _ = std::fs::write(&target, csv);
}
