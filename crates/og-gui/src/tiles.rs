//! Background maps: OpenStreetMap, or a map of one's own from the settings, as tiles, WMS or
//! WMTS. Tiles are fetched in the background and cached on disk. If the server cannot be
//! reached this is noticed, and the map falls back to a grid.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
/// Half the circumference of the Earth in Web Mercator, in metres.
pub const HALF_WORLD: f64 = 20_037_508.342_789_244;
const MAX_ZOOM: u8 = 19;
const MEMORY_LIMIT: usize = 256;
const WORKERS: usize = 4;

pub type Key = (u8, u32, u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Net {
    /// No attempt has been answered yet.
    Unknown,
    Online,
    Offline,
}

enum Slot {
    Loading,
    Ready(TextureHandle, u64),
    Failed(Instant),
}

/// Where the tiles come from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// An address with {z}, {x} and {y}: OpenStreetMap, or a server given for tests.
    Url(String),
    /// A background map of the settings.
    Map(Box<og_engine::settings::BackgroundMap>),
}

impl Source {
    fn request(&self, key: Key) -> (String, Vec<(String, String)>) {
        match self {
            Source::Url(u) => (u.replace("{z}", &key.0.to_string()).replace("{x}", &key.1.to_string()).replace("{y}", &key.2.to_string()), Vec::new()),
            Source::Map(m) => m.tile_request(key.0, key.1, key.2),
        }
    }
}

pub struct Tiles {
    url: String,
    slots: HashMap<Key, Slot>,
    jobs: Sender<Key>,
    done: Receiver<(Key, Option<ColorImage>)>,
    frame: u64,
    net: Net,
    last_failure: Option<Instant>,
    /// Failures in a row. A single error must not turn the background map off.
    failures: u32,
}

/// The zoom level whose tiles fit best at a given scale (screen points per metre).
pub fn zoom_for(scale: f64) -> u8 {
    let meters_per_px_at_zero = 2.0 * HALF_WORLD / 256.0;
    ((scale * meters_per_px_at_zero).log2().round()).clamp(0.0, MAX_ZOOM as f64) as u8
}

/// The extent of a tile in Web Mercator: [min_x, min_y, max_x, max_y].
pub fn tile_bounds(z: u8, x: u32, y: u32) -> [f64; 4] {
    let size = 2.0 * HALF_WORLD / (1u64 << z) as f64;
    let min_x = -HALF_WORLD + x as f64 * size;
    let max_y = HALF_WORLD - y as f64 * size;
    [min_x, max_y - size, min_x + size, max_y]
}

/// The tiles that cover a rectangle in Web Mercator: (x0, y0, x1, y1), bounds included.
pub fn tile_range(z: u8, bounds: [f64; 4]) -> (u32, u32, u32, u32) {
    let n = (1u64 << z) as f64;
    let size = 2.0 * HALF_WORLD / n;
    let clamp = |v: f64| v.floor().clamp(0.0, n - 1.0) as u32;
    (
        clamp((bounds[0] + HALF_WORLD) / size),
        clamp((HALF_WORLD - bounds[3]) / size),
        clamp((bounds[2] + HALF_WORLD) / size),
        clamp((HALF_WORLD - bounds[1]) / size),
    )
}

fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("Ographis").join("tiles"))
}

fn fetch(agent: &ureq::Agent, source: &Source, dir: Option<&PathBuf>, key: Key) -> Option<ColorImage> {
    let file = dir.map(|d| d.join(key.0.to_string()).join(key.1.to_string()).join(format!("{}.png", key.2)));
    let cached = file.as_ref().and_then(|f| std::fs::read(f).ok());
    let bytes = match cached {
        Some(b) => b,
        None => {
            let (u, headers) = source.request(key);
            let mut req = agent.get(&u);
            for (k, v) in &headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let b = req.call().ok()?.body_mut().with_config().limit(8_000_000).read_to_vec().ok()?;
            // A service that answers with an error, as WMS does in XML, is no picture.
            image::load_from_memory(&b).ok()?;
            if let Some(f) = &file {
                if let Some(parent) = f.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(f, &b);
            }
            b
        }
    };
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

impl Tiles {
    /// `source` is your own tile server in the form http://…/{z}/{x}/{y}.png. Without it
    /// the environment variable OGRAPHIS_TILE_URL is used if set, otherwise OpenStreetMap.
    pub fn new(ctx: &egui::Context, source: Option<String>) -> Tiles {
        Tiles::from_source(ctx, source, None)
    }

    /// As `new`, or with a background map of the settings, which is cached on disk under
    /// its name.
    pub fn from_source(ctx: &egui::Context, source: Option<String>, map: Option<og_engine::settings::BackgroundMap>) -> Tiles {
        let custom = source.or_else(|| std::env::var("OGRAPHIS_TILE_URL").or_else(|_| std::env::var("GEOBOBBO_TILE_URL")).ok()).filter(|s| !s.is_empty());
        let (url, dir, source) = match map {
            Some(m) => {
                // Each map has a folder of its own, by its name and how it is asked for.
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (m.kind.as_str(), m.url.as_str(), m.layers.as_str(), m.style.as_str(), m.format.as_str(), m.matrix_set.as_str(), m.matrix.as_str())
                    .hash(&mut h);
                let dir = cache_dir().map(|d| d.join(format!("map-{:016x}", h.finish())));
                (m.url.clone(), dir, Source::Map(Box::new(m)))
            }
            None => {
                let dir = if custom.is_some() { None } else { cache_dir() };
                let url = custom.unwrap_or_else(|| DEFAULT_URL.to_string());
                (url.clone(), dir, Source::Url(url))
            }
        };
        let (jobs, job_rx) = channel::<Key>();
        let (done_tx, done) = channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for _ in 0..WORKERS {
            let (job_rx, done_tx, source, dir, ctx) = (job_rx.clone(), done_tx.clone(), source.clone(), dir.clone(), ctx.clone());
            std::thread::spawn(move || {
                // The OpenStreetMap tile usage policy requires the program to say what it is.
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
                    .timeout_global(Some(Duration::from_secs(8)))
                    .user_agent(concat!("Ographis/", env!("CARGO_PKG_VERSION"), " (desktop application)"))
                    .build()
                    .into();
                loop {
                    let key = match job_rx.lock().map(|rx| rx.recv()) {
                        Ok(Ok(k)) => k,
                        _ => return,
                    };
                    let img = fetch(&agent, &source, dir.as_ref(), key);
                    if done_tx.send((key, img)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Tiles { url, slots: HashMap::new(), jobs, done, frame: 0, net: Net::Unknown, last_failure: None, failures: 0 }
    }

    pub fn is_default_source(&self) -> bool {
        self.url == DEFAULT_URL
    }

    pub fn net(&self) -> Net {
        self.net
    }

    /// Receives finished tiles. Called once per frame before `get`.
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        while let Ok((key, img)) = self.done.try_recv() {
            match img {
                Some(img) => {
                    let tex = ctx.load_texture(format!("tile-{}-{}-{}", key.0, key.1, key.2), img, TextureOptions::LINEAR);
                    self.slots.insert(key, Slot::Ready(tex, self.frame));
                    self.net = Net::Online;
                    self.failures = 0;
                }
                None => {
                    self.slots.insert(key, Slot::Failed(Instant::now()));
                    self.last_failure = Some(Instant::now());
                    self.failures += 1;
                    if self.failures >= 3 {
                        self.net = Net::Offline;
                    }
                }
            }
        }
        if self.slots.len() > MEMORY_LIMIT {
            let mut used: Vec<(Key, u64)> = self.slots.iter().filter_map(|(k, s)| if let Slot::Ready(_, f) = s { Some((*k, *f)) } else { None }).collect();
            used.sort_by_key(|u| u.1);
            for (k, _) in used.iter().take(self.slots.len() - MEMORY_LIMIT * 3 / 4) {
                self.slots.remove(k);
            }
        }
    }

    /// Is it time to try again after a failure?
    pub fn should_retry(&self) -> bool {
        self.net == Net::Offline && self.last_failure.is_none_or(|t| t.elapsed() > Duration::from_secs(30))
    }

    /// Makes a new attempt against the server with a single tile.
    pub fn probe(&mut self) {
        self.slots.retain(|_, s| !matches!(s, Slot::Failed(_)));
        self.last_failure = Some(Instant::now());
        // One more error is enough to stay in grid mode.
        self.failures = 2;
        self.request((0, 0, 0));
    }

    fn request(&mut self, key: Key) {
        if self.jobs.send(key).is_ok() {
            self.slots.insert(key, Slot::Loading);
        }
    }

    /// The image of the tile if it has been fetched. Otherwise it is requested.
    pub fn get(&mut self, key: Key) -> Option<TextureHandle> {
        match self.slots.get_mut(&key) {
            Some(Slot::Ready(tex, used)) => {
                *used = self.frame;
                Some(tex.clone())
            }
            Some(Slot::Loading) => None,
            Some(Slot::Failed(when)) => {
                if when.elapsed() > Duration::from_secs(30) && self.net != Net::Offline {
                    self.request(key);
                }
                None
            }
            None => {
                if self.net != Net::Offline {
                    self.request(key);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_math() {
        assert_eq!(tile_bounds(0, 0, 0), [-HALF_WORLD, -HALF_WORLD, HALF_WORLD, HALF_WORLD]);
        // Level 1 has four tiles. The north-eastern one is x = 1, y = 0.
        assert_eq!(tile_bounds(1, 1, 0), [0.0, 0.0, HALF_WORLD, HALF_WORLD]);
        assert_eq!(tile_range(1, [1.0, 1.0, 2.0, 2.0]), (1, 0, 1, 0));
        assert_eq!(tile_range(1, [-2.0, -2.0, 2.0, 2.0]), (0, 0, 1, 1));
        // A tile at level 0 is 256 points wide, so the scale 256 / the Earth's circumference gives level 0.
        assert_eq!(zoom_for(256.0 / (2.0 * HALF_WORLD)), 0);
        assert_eq!(zoom_for(1024.0 / (2.0 * HALF_WORLD)), 2);
        assert_eq!(zoom_for(1e9), 19);
        // The tile for Gothenburg at level 14 must be the same as with the tile number
        // formulas on the OpenStreetMap wiki, which calculate directly in degrees.
        let (lon, lat) = (11.9746_f64, 57.7089_f64);
        let mx = lon.to_radians() * 6_378_137.0;
        let my = (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln() * 6_378_137.0;
        let n = 2f64.powi(14);
        let wiki_x = ((lon + 180.0) / 360.0 * n).floor() as u32;
        let wiki_y = ((1.0 - (lat.to_radians().tan() + 1.0 / lat.to_radians().cos()).ln() / std::f64::consts::PI) / 2.0 * n).floor() as u32;
        assert_eq!(tile_range(14, [mx, my, mx, my]), (wiki_x, wiki_y, wiki_x, wiki_y));
        let b = tile_bounds(14, wiki_x, wiki_y);
        assert!(b[0] <= mx && mx < b[2] && b[1] <= my && my < b[3]);
    }
}
