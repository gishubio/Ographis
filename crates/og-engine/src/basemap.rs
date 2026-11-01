//! What lies under the layers of a map from Draw map: a background map of tiles, such as
//! OpenStreetMap, or a picture. Either is made a picture exactly as large as the map, in its
//! coordinate system, so that it is drawn first and the layers on top of it.

use crate::io::Backend;
use crate::mapdraw::Frame;
use crate::model::{GbError, RasterImage, Result};
use crate::settings::BackgroundMap;
use std::path::{Path, PathBuf};

/// The background maps that are there without settings: (value, name, address, who made it).
pub const BUILT_IN: &[(&str, &str, &str, &str)] = &[
    ("osm", "OpenStreetMap", "https://tile.openstreetmap.org/{z}/{x}/{y}.png", "© OpenStreetMap contributors"),
    ("carto_light", "Light grey (CARTO Positron)", "https://a.basemaps.cartocdn.com/light_all/{z}/{x}/{y}.png", "© OpenStreetMap contributors © CARTO"),
    ("carto_dark", "Dark (CARTO Dark Matter)", "https://a.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}.png", "© OpenStreetMap contributors © CARTO"),
    (
        "carto_voyager",
        "Soft colours (CARTO Voyager)",
        "https://a.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}.png",
        "© OpenStreetMap contributors © CARTO",
    ),
];

/// A background map by its name: one of those built in, one of the user's own in the
/// settings by its name, or an address with {z}, {x} and {y}.
pub fn find(name: &str) -> Result<BackgroundMap> {
    let n = name.trim();
    if let Some(b) = BUILT_IN.iter().find(|b| b.0.eq_ignore_ascii_case(n) || b.1.eq_ignore_ascii_case(n)) {
        return Ok(BackgroundMap { name: b.1.into(), kind: "xyz".into(), url: b.2.into(), attribution: b.3.into(), ..Default::default() });
    }
    if n.contains("{z}") && n.contains("{x}") && n.contains("{y}") {
        return Ok(BackgroundMap { name: n.into(), kind: "xyz".into(), url: n.into(), ..Default::default() });
    }
    let s = crate::settings::current()?;
    s.maps.iter().find(|m| m.name.trim().eq_ignore_ascii_case(n)).cloned().ok_or_else(|| {
        let own: Vec<&str> = s.maps.iter().map(|m| m.name.as_str()).collect();
        GbError(format!(
            "There is no background map named \"{}\". Choose one of those built in{}, or write an address with {{z}}, {{x}} and {{y}}.",
            n,
            if own.is_empty() { String::new() } else { format!(", or one of yours under Settings → Background maps: {}", own.join(", ")) }
        ))
    })
}

const HALF: f64 = 20_037_508.342_789_244;

/// The points of the map, every `step` pixels and along its edges, in another coordinate
/// system: a grid to find the place of each pixel in between.
struct Grid {
    step: usize,
    cols: usize,
    rows: usize,
    xs: Vec<f64>,
    ys: Vec<f64>,
}

impl Grid {
    fn new(f: &Frame, from: u32, to: u32, backend: Option<&dyn Backend>) -> Result<Grid> {
        let step = 16usize;
        let (cols, rows) = (f.width as usize / step + 2, f.height as usize / step + 2);
        let top = f.b[1] + f.height as f64 * f.px;
        let (mut xs, mut ys) = (Vec::with_capacity(cols * rows), Vec::with_capacity(cols * rows));
        for r in 0..rows {
            for c in 0..cols {
                xs.push(f.b[0] + (c * step) as f64 * f.px);
                ys.push(top - (r * step) as f64 * f.px);
            }
        }
        if from != to {
            let b = backend.ok_or_else(|| GbError("A background in another coordinate system needs the GDAL support.".into()))?;
            b.transform(from, to, &mut xs, &mut ys)?;
        }
        Ok(Grid { step, cols, rows, xs, ys })
    }
    /// Where a pixel lies, between the points of the grid.
    fn at(&self, px: usize, py: usize) -> (f64, f64) {
        let (fc, fr) = (px as f64 / self.step as f64, py as f64 / self.step as f64);
        let (c, r) = ((fc.floor() as usize).min(self.cols - 2), (fr.floor() as usize).min(self.rows - 2));
        let (u, v) = (fc - c as f64, fr - r as f64);
        let i = |c: usize, r: usize| r * self.cols + c;
        let lerp = |a: &[f64]| {
            let top = a[i(c, r)] * (1.0 - u) + a[i(c + 1, r)] * u;
            let bottom = a[i(c, r + 1)] * (1.0 - u) + a[i(c + 1, r + 1)] * u;
            top * (1.0 - v) + bottom * v
        };
        (lerp(&self.xs), lerp(&self.ys))
    }
    fn bounds(&self) -> [f64; 4] {
        let f = |v: &[f64], max: bool| {
            v.iter().copied().filter(|x| x.is_finite()).fold(if max { f64::NEG_INFINITY } else { f64::INFINITY }, if max { f64::max } else { f64::min })
        };
        [f(&self.xs, false), f(&self.ys, false), f(&self.xs, true), f(&self.ys, true)]
    }
}

/// A picture with its place: its pixels and its geotransform, sampled bilinearly.
struct Source {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    gt: [f64; 6],
}

impl Source {
    fn sample(&self, x: f64, y: f64) -> [u8; 4] {
        let (c, r) = ((x - self.gt[0]) / self.gt[1] - 0.5, (y - self.gt[3]) / self.gt[5] - 0.5);
        if !c.is_finite() || !r.is_finite() || c < -0.5 || r < -0.5 || c > self.width as f64 - 0.5 || r > self.height as f64 - 0.5 {
            return [0, 0, 0, 0];
        }
        let (c0, r0) = (c.floor().max(0.0) as usize, r.floor().max(0.0) as usize);
        let (c1, r1) = ((c0 + 1).min(self.width - 1), (r0 + 1).min(self.height - 1));
        let (u, v) = ((c - c0 as f64).clamp(0.0, 1.0) as f32, (r - r0 as f64).clamp(0.0, 1.0) as f32);
        let px = |c: usize, r: usize| &self.rgba[(r * self.width + c) * 4..(r * self.width + c) * 4 + 4];
        let mut out = [0u8; 4];
        for (k, o) in out.iter_mut().enumerate() {
            let top = px(c0, r0)[k] as f32 * (1.0 - u) + px(c1, r0)[k] as f32 * u;
            let bottom = px(c0, r1)[k] as f32 * (1.0 - u) + px(c1, r1)[k] as f32 * u;
            *o = (top * (1.0 - v) + bottom * v).round() as u8;
        }
        out
    }
}

/// The map as a picture: each pixel from the source where it lies, made paler by `fade`
/// (0 to 1).
fn resample(f: &Frame, grid: &Grid, src: &Source, fade: f32) -> RasterImage {
    let (w, h) = (f.width as usize, f.height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for py in 0..h {
        for px in 0..w {
            let (x, y) = grid.at(px, py);
            // The middle of the pixel: half a step on in the grid is close enough.
            let mut c = src.sample(x, y);
            if fade > 0.0 {
                for v in c.iter_mut().take(3) {
                    *v = (*v as f32 + (255.0 - *v as f32) * fade).round() as u8;
                }
            }
            rgba[(py * w + px) * 4..(py * w + px) * 4 + 4].copy_from_slice(&c);
        }
    }
    RasterImage { width: w, height: h, rgba, geotransform: f.geotransform() }
}

/// Where tiles are kept between runs.
fn cache_dir(map: &BackgroundMap) -> Option<PathBuf> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{}|{}|{}|{}", map.kind, map.url, map.layers, map.style).bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    crate::settings::settings_dir().map(|d| d.join("map-tiles").join(format!("{:016x}", h)))
}

/// A background map under a map of `frame` in `crs`: the tiles at the zoom level that suits
/// its scale, fetched or taken from the cache, put together and drawn into the coordinate
/// system of the map. Gives the picture, how many tiles it took and how many could not be had.
pub fn tiles(map: &BackgroundMap, f: &Frame, crs: u32, backend: &dyn Backend, fade: f32, going: &dyn Fn() -> bool) -> Result<(RasterImage, usize, usize)> {
    let grid = Grid::new(f, crs, 3857, Some(backend))?;
    let b = grid.bounds();
    if !b[0].is_finite() {
        return Err(GbError("The map lies where Web Mercator cannot show it, so there is no background map.".into()));
    }
    let (w, h) = (f.width as f64, f.height as f64);
    let res = ((b[2] - b[0]) / w).min((b[3] - b[1]) / h).max(1e-6);
    let mut z = ((2.0 * HALF / 256.0) / res).log2().round().clamp(0.0, 19.0) as u32;
    let range = |z: u32| {
        let n = (1u64 << z) as f64;
        let size = 2.0 * HALF / n;
        let last = (1u64 << z) as i64 - 1;
        let col = |x: f64| (((x + HALF) / size).floor() as i64).clamp(0, last);
        let row = |y: f64| (((HALF - y) / size).floor() as i64).clamp(0, last);
        (col(b[0]), col(b[2]), row(b[3]), row(b[1]))
    };
    while z > 0 {
        let (c0, c1, r0, r1) = range(z);
        if (c1 - c0 + 1) * (r1 - r0 + 1) <= 300 {
            break;
        }
        z -= 1;
    }
    let (c0, c1, r0, r1) = range(z);
    let cache = cache_dir(map);
    let mut tiles: Vec<(i64, i64, image::RgbaImage)> = Vec::new();
    let mut failed = 0usize;
    let mut last_error = String::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            if !going() {
                return Err(GbError(crate::model::STOPPED.into()));
            }
            let file = cache.as_ref().map(|d| d.join(z.to_string()).join(c.to_string()).join(format!("{}.tile", r)));
            let fresh = file
                .as_ref()
                .filter(|f| f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 30 * 86_400));
            let bytes = match fresh.and_then(|f| std::fs::read(f).ok()) {
                Some(b) => b,
                None => {
                    let (url, headers) = map.tile_request(z as u8, c as u32, r as u32);
                    match backend.fetch_bytes(&url, &headers) {
                        Ok(b) => {
                            if let Some(f) = &file {
                                if let Some(d) = f.parent() {
                                    let _ = std::fs::create_dir_all(d);
                                }
                                let _ = std::fs::write(f, &b);
                            }
                            b
                        }
                        Err(e) => {
                            failed += 1;
                            last_error = e.0;
                            continue;
                        }
                    }
                }
            };
            match image::load_from_memory(&bytes) {
                Ok(img) => tiles.push((c, r, img.to_rgba8())),
                Err(e) => {
                    failed += 1;
                    last_error = format!("a tile is not a picture: {}", e);
                }
            }
        }
    }
    if tiles.is_empty() {
        return Err(GbError(format!("No tiles of the background map could be fetched: {}", last_error)));
    }
    // The tiles side by side in one picture, as large as the tiles are.
    let ts = tiles[0].2.width() as usize;
    let (cols, rows) = ((c1 - c0 + 1) as usize, (r1 - r0 + 1) as usize);
    let (mw, mh) = (cols * ts, rows * ts);
    let mut rgba = vec![0u8; mw * mh * 4];
    for (c, r, img) in &tiles {
        let img =
            if img.width() as usize != ts { image::imageops::resize(img, ts as u32, ts as u32, image::imageops::FilterType::Triangle) } else { img.clone() };
        let (ox, oy) = ((*c - c0) as usize * ts, (*r - r0) as usize * ts);
        for y in 0..ts {
            let from = &img.as_raw()[y * ts * 4..(y + 1) * ts * 4];
            let at = ((oy + y) * mw + ox) * 4;
            rgba[at..at + ts * 4].copy_from_slice(from);
        }
    }
    let size = 2.0 * HALF / (1u64 << z) as f64;
    let src = Source { width: mw, height: mh, rgba, gt: [-HALF + c0 as f64 * size, size / ts as f64, 0.0, HALF - r0 as f64 * size, 0.0, -size / ts as f64] };
    // Where a tile is missing the picture is see-through, and the background shows.
    Ok((resample(f, &grid, &src, fade), tiles.len(), failed))
}

/// The world file beside a picture, as .pgw, .pngw, .jgw, .jpgw or .wld: its geotransform.
fn world_file(path: &Path) -> Option<[f64; 6]> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let candidates: Vec<String> = match ext.as_str() {
        "png" => vec!["pgw".into(), "pngw".into(), "wld".into()],
        "jpg" | "jpeg" => vec!["jgw".into(), "jpgw".into(), "jpegw".into(), "wld".into()],
        _ => vec!["wld".into()],
    };
    for c in candidates {
        for e in [c.clone(), c.to_uppercase()] {
            if let Ok(text) = std::fs::read_to_string(path.with_extension(&e)) {
                let v: Vec<f64> = text.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() >= 6 {
                    // A, D, B, E, C, F: the middle of the upper left pixel.
                    return Some([v[4] - v[0] / 2.0 - v[2] / 2.0, v[0], v[2], v[5] - v[1] / 2.0 - v[3] / 2.0, v[1], v[3]]);
                }
            }
        }
    }
    None
}

/// A picture under the map: in its place when it has a world file, in the coordinate system
/// `picture_crs` (that of the map if None); otherwise stretched to cover the whole map,
/// keeping its proportions and cutting what reaches beyond.
pub fn picture(path: &Path, f: &Frame, crs: u32, picture_crs: Option<u32>, backend: Option<&dyn Backend>, fade: f32) -> Result<(RasterImage, bool)> {
    let img = image::open(path).map_err(|e| GbError(format!("The picture {} cannot be read: {}", path.display(), e)))?.to_rgba8();
    let (iw, ih) = (img.width() as usize, img.height() as usize);
    match world_file(path) {
        Some(gt) => {
            if gt[2] != 0.0 || gt[4] != 0.0 {
                return Err(GbError("The picture is turned in its world file, which a background cannot be yet.".into()));
            }
            let grid = Grid::new(f, crs, picture_crs.unwrap_or(crs), backend)?;
            let src = Source { width: iw, height: ih, rgba: img.into_raw(), gt };
            Ok((resample(f, &grid, &src, fade), true))
        }
        None => {
            // Covering the map: the scale that fills both sides, the picture in the middle.
            let (w, h) = (f.width as f64, f.height as f64);
            let scale = (w / iw as f64).max(h / ih as f64);
            let (ox, oy) = ((w - iw as f64 * scale) / 2.0, (h - ih as f64 * scale) / 2.0);
            let top = f.b[1] + f.height as f64 * f.px;
            let gt = [f.b[0] + ox * f.px, f.px * scale, 0.0, top - oy * f.px, 0.0, -f.px * scale];
            let grid = Grid::new(f, crs, crs, None)?;
            let src = Source { width: iw, height: ih, rgba: img.into_raw(), gt };
            Ok((resample(f, &grid, &src, fade), false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_without_a_world_file_covers_the_map() {
        let dir = std::env::temp_dir().join(format!("og_basemap_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("paper.png");
        let mut img = image::RgbaImage::new(40, 20);
        for (x, _, px) in img.enumerate_pixels_mut() {
            *px = if x < 20 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) };
        }
        img.save(&p).unwrap();
        let f = Frame { b: [0.0, 0.0, 100.0, 100.0], px: 1.0, width: 100, height: 100 };
        let (out, placed) = picture(&p, &f, 3006, None, None, 0.0).unwrap();
        assert!(!placed);
        assert_eq!((out.width, out.height), (100, 100));
        // Red at the left, blue at the right, and no see-through pixels.
        assert_eq!(&out.rgba[(50 * 100 + 5) * 4..(50 * 100 + 5) * 4 + 4], &[255, 0, 0, 255]);
        assert_eq!(&out.rgba[(50 * 100 + 95) * 4..(50 * 100 + 95) * 4 + 4], &[0, 0, 255, 255]);
        assert!(out.rgba.chunks(4).all(|p| p[3] == 255));
        // With a world file it lies where it says, and is see-through around.
        std::fs::write(dir.join("paper.pgw"), "1\n0\n0\n-1\n10.5\n59.5\n").unwrap();
        let (out, placed) = picture(&p, &f, 3006, None, None, 0.5).unwrap();
        assert!(placed);
        assert_eq!(out.rgba[(70 * 100 + 5) * 4 + 3], 0, "outside the picture");
        assert_eq!(&out.rgba[(50 * 100 + 15) * 4..(50 * 100 + 15) * 4 + 4], &[255, 128, 128, 255], "red, faded half way to white");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn built_in_maps_and_addresses_are_found() {
        assert_eq!(find("osm").unwrap().attribution, "© OpenStreetMap contributors");
        assert_eq!(find("https://x.se/{z}/{x}/{y}.png").unwrap().kind, "xyz");
    }
}
