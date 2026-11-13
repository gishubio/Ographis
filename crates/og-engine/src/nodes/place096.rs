//! Fetch a place: heights and a picture from above of any place on Earth, by its name or
//! its coordinates, in the UTM zone it lies in. The heights come from the Copernicus DEM
//! GLO-30 (30 m, from the radar of TanDEM-X, open with its source named), and the picture
//! is made of the least cloudy Sentinel-2 pictures of a time, the clouds taken away and the
//! rest put together. Both are read over the web, only the part that is needed.

use super::*;
use crate::model::{err, fmt_count, fmt_real, Feature, Field, FieldType, GbError, Grid, GridMeta, RasterImage, Result, Table, Value};
use crate::web::Client;

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "place",
        label: "Place",
        kind: ParamKind::Text,
        default: "",
        hint: "A name, such as Kīlauea, Matterhorn or Isla Isabela, found with Nominatim of OpenStreetMap; or latitude and longitude, such as 19.41, -155.28",
    },
    ParamSpec { key: "size", label: "Size", kind: ParamKind::Text, default: "20", hint: "The side of the square around the place, in kilometres" },
    ParamSpec {
        key: "fetch",
        label: "Fetch",
        kind: ParamKind::Picks(&[("heights", "Heights"), ("image", "Picture from above")]),
        default: "heights, image",
        hint: "Heights from the Copernicus DEM, 30 m; a picture from Sentinel-2, 10 m",
    },
    ParamSpec {
        key: "time",
        label: "Time of the picture",
        kind: ParamKind::Text,
        default: "",
        hint: "From and to, such as 2025-06-01/2025-09-30, when the place is green or free of snow. Empty: the year before today",
    },
    ParamSpec { key: "cloud", label: "Most clouds", kind: ParamKind::Text, default: "30", hint: "In percent: pictures with more are not used" },
    ParamSpec { key: "most", label: "At most pictures", kind: ParamKind::Text, default: "6", hint: "The least cloudy, put together where they are clear" },
    ParamSpec {
        key: "brightness",
        label: "Brightness",
        kind: ParamKind::Text,
        default: "1",
        hint: "More than 1 for a lighter picture, such as for dark lava or forest",
    },
    ParamSpec {
        key: "geocoder",
        label: "Finder of places",
        kind: ParamKind::Text,
        default: "https://nominatim.openstreetmap.org",
        hint: "Nominatim, or another with the same questions. One question a run",
    },
    ParamSpec {
        key: "dem",
        label: "Heights from",
        kind: ParamKind::Text,
        default: "https://copernicus-dem-30m.s3.amazonaws.com",
        hint: "The Copernicus DEM GLO-30 as Cloud Optimized GeoTIFF, a file for each degree",
    },
    ParamSpec {
        key: "stac",
        label: "Pictures from",
        kind: ParamKind::Text,
        default: "https://earth-search.aws.element84.com/v1",
        hint: "A STAC API with sentinel-2-l2a, its bands red, green, blue and scl",
    },
];

/// The UTM zone of a place: EPSG 326xx in the north, 327xx in the south.
pub(super) fn utm_of(lat: f64, lon: f64) -> u32 {
    let zone = (((lon + 180.0) / 6.0).floor() as i64).rem_euclid(60) as u32 + 1;
    if lat >= 0.0 {
        32600 + zone
    } else {
        32700 + zone
    }
}

/// A Sentinel-2 picture: its clouds in percent, its day, its red, green, blue and classes,
/// and the scale and offset of its values.
type Scene = (f64, String, [String; 4], (f32, f32));

/// How the values of a Sentinel-2 picture become reflectance, from 0 to 1: the scale and the
/// offset its catalogue gives for the red band, or else 0.0001, less 0.1 for pictures made
/// since processing baseline 04.00 of January 2022, whose values have 1000 added; never when
/// the catalogue says that the offset was taken away already.
fn reflectance_of(item: &serde_json::Value) -> (f32, f32) {
    let band = item.pointer("/assets/red/raster:bands/0");
    let scale = band.and_then(|b| b.get("scale")).and_then(|v| v.as_f64());
    let offset = band.and_then(|b| b.get("offset")).and_then(|v| v.as_f64());
    let props = item.get("properties");
    // Earth Search takes the offset away in its files and says so, while its bands still
    // give the offset: taken away twice, the dark ground of forest and lava would be black.
    let applied = props.and_then(|p| p.get("earthsearch:boa_offset_applied")).and_then(|v| v.as_bool()).unwrap_or(false);
    if let Some(scale) = scale {
        return (scale as f32, if applied { 0.0 } else { offset.unwrap_or(0.0) as f32 });
    }
    let baseline = props
        .and_then(|p| p.get("s2:processing_baseline").or_else(|| p.get("processing:baseline")))
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse::<f64>().ok());
    let offset = if !applied && baseline.is_some_and(|b| b >= 4.0) { -0.1 } else { 0.0 };
    (0.0001, offset)
}

/// The file of the Copernicus DEM for the degree whose south-west corner is at `lat`, `lon`.
pub(super) fn dem_tile(base: &str, lat: i32, lon: i32) -> String {
    let ns = if lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", -lat) };
    let ew = if lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", -lon) };
    let name = format!("Copernicus_DSM_COG_10_{}_00_{}_00_DEM", ns, ew);
    format!("{}/{}/{}.tif", base.trim_end_matches('/'), name, name)
}

/// Latitude and longitude written as two numbers.
fn coordinates(text: &str) -> Option<(f64, f64)> {
    let n: Vec<f64> =
        text.split([',', ' ', ';']).filter(|s| !s.trim().is_empty()).map(|s| s.trim().parse::<f64>()).collect::<std::result::Result<_, _>>().ok()?;
    match n.as_slice() {
        [lat, lon] if lat.abs() <= 90.0 && lon.abs() <= 180.0 => Some((*lat, *lon)),
        _ => None,
    }
}

fn run_place(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let b = ctx.backend("Fetching a place")?;
    let place = p.need("place", "Write a place, such as Kīlauea, or its latitude and longitude, such as 19.41, -155.28.")?;
    let km = number_param(p, "size", "The size")?.unwrap_or(20.0);
    if !(0.1..=200.0).contains(&km) {
        return err("The size must be from 0.1 to 200 km.");
    }
    let fetch = p.list("fetch");
    let want = |w: &str| fetch.is_empty() || fetch.iter().any(|f| f == w);
    let gate = ctx.gate;
    let stopped = move || !gate.going();
    // What it does, in the log as it goes, with the seconds each step took.
    let say_now = ctx.say;
    let say = |m: &str| say_now(crate::run::Level::Info, m);
    let secs = |t: std::time::Instant| format!("{:.1} s", t.elapsed().as_secs_f64());
    // Where the place is.
    let (lat, lon, label) = match coordinates(&place) {
        Some((lat, lon)) => (lat, lon, place.trim().to_string()),
        None => {
            let base = p.get("geocoder").unwrap_or_else(|| "https://nominatim.openstreetmap.org".into());
            let t = std::time::Instant::now();
            say(&format!("Looking for {} with the finder of places at {}", place.trim(), base.trim_end_matches('/')));
            let url = format!("{}/search?format=jsonv2&limit=1&q={}", base.trim_end_matches('/'), crate::web::encode(place.trim()));
            let mut client = Client::new(b, None, &stopped);
            let headers =
                vec![("User-Agent".to_string(), format!("Ographis/{}", env!("CARGO_PKG_VERSION"))), ("Accept".to_string(), "application/json".to_string())];
            let a = client.send("GET", &url, &headers, None)?;
            if a.status >= 400 {
                return err(format!("The finder of places answered {}.", a.status));
            }
            let v: serde_json::Value = serde_json::from_str(&a.body).map_err(|_| GbError("The finder of places did not answer with JSON.".into()))?;
            let hit = v
                .as_array()
                .and_then(|l| l.first())
                .ok_or_else(|| GbError(format!("No place called {} was found. Try another spelling, or its latitude and longitude.", place.trim())))?;
            let num = |k: &str| hit.get(k).and_then(|x| x.as_str().and_then(|s| s.parse().ok()).or_else(|| x.as_f64()));
            let (Some(lat), Some(lon)) = (num("lat"), num("lon")) else { return err("The finder of places gave no coordinates.") };
            let name = hit.get("display_name").and_then(|n| n.as_str()).unwrap_or(place.trim()).to_string();
            say(&format!("Found {} at {:.4}, {:.4}, in {}", name, lat, lon, secs(t)));
            ctx.notes.info(format!("found {}", name));
            (lat, lon, name)
        }
    };
    let crs = utm_of(lat, lon);
    let (mut cx, mut cy) = (vec![lon], vec![lat]);
    b.transform(4326, crs, &mut cx, &mut cy)?;
    let half = km * 500.0;
    // On whole metres of 30, so that the cells of the heights fall on even numbers.
    let snap = |v: f64| (v / 30.0).round() * 30.0;
    let rect = [snap(cx[0] - half), snap(cy[0] - half), snap(cx[0] + half), snap(cy[0] + half)];
    // The area in degrees, for the files to read and the search.
    let (mut xs, mut ys) = (vec![], vec![]);
    for (fx, fy) in [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)] {
        xs.push(rect[0] + fx * (rect[2] - rect[0]));
        ys.push(rect[1] + fy * (rect[3] - rect[1]));
    }
    b.transform(crs, 4326, &mut xs, &mut ys)?;
    let lo = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let bbox = [lo(&xs), lo(&ys), hi(&xs), hi(&ys)];
    say(&format!(
        "The area: {} × {} km around {:.4}, {:.4}, in UTM zone {}{} (EPSG:{})",
        fmt_real(km),
        fmt_real(km),
        lat,
        lon,
        crs % 100,
        if crs / 100 == 327 { " south" } else { "" },
        crs
    ));
    let mut outputs: Outputs = Vec::new();
    let (progress, gate2) = (ctx.progress, ctx.gate);

    // The heights: each degree's file read onto the grid; where there is none, the sea.
    let mut heights_table = Table { crs: Some(crs), raster: true, ..Table::default() };
    if want("heights") {
        let cell = 30.0;
        let (w, h) = (((rect[2] - rect[0]) / cell).round() as usize, ((rect[3] - rect[1]) / cell).round() as usize);
        let gt = [rect[0], cell, 0.0, rect[3], 0.0, -cell];
        let base = p.get("dem").unwrap_or_else(|| "https://copernicus-dem-30m.s3.amazonaws.com".into());
        let mut values = vec![f32::NAN; w * h];
        let (mut read, mut sea) = (0usize, 0usize);
        let mut first_problem = None;
        let tiles: Vec<(i32, i32)> = (bbox[1].floor() as i32..=bbox[3].floor() as i32)
            .flat_map(|la| (bbox[0].floor() as i32..=bbox[2].floor() as i32).map(move |lo| (la, lo)))
            .collect();
        say(&format!(
            "Heights: {} × {} cells of 30 m from {} {} of the Copernicus DEM at {}",
            w,
            h,
            tiles.len(),
            if tiles.len() == 1 { "file" } else { "files" },
            base.trim_end_matches('/')
        ));
        let all = std::time::Instant::now();
        for (k, (la, lo)) in tiles.iter().enumerate() {
            if !gate2.going() {
                return err(crate::model::STOPPED);
            }
            progress("heights", k as u64, Some(tiles.len() as u64));
            let href = dem_tile(&base, *la, *lo);
            let name = href.rsplit('/').next().unwrap_or(&href).to_string();
            say(&format!("Heights {} of {}: reading {}", k + 1, tiles.len(), name));
            let t = std::time::Instant::now();
            match b.read_onto(&href, gt, w, h, crs, false) {
                Ok(v) => {
                    say(&format!("Heights {} of {}: read in {}", k + 1, tiles.len(), secs(t)));
                    read += 1;
                    for (a, x) in values.iter_mut().zip(v) {
                        if !x.is_nan() {
                            *a = x;
                        }
                    }
                }
                Err(e) => {
                    // The DEM has no files where there is only sea.
                    say(&format!("Heights {} of {}: no file, which is sea", k + 1, tiles.len()));
                    sea += 1;
                    first_problem.get_or_insert(e.0);
                }
            }
        }
        if read == 0 {
            return err(format!("No heights could be read for {}. {}", label, first_problem.map(|e| format!("The first answer: {}", e)).unwrap_or_default()));
        }
        let empty = values.iter().filter(|v| v.is_nan()).count();
        for v in values.iter_mut().filter(|v| v.is_nan()) {
            *v = 0.0;
        }
        let (zlo, zhi) = values.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
        say(&format!("Heights done in {}: {} to {} m", secs(all), fmt_real(zlo.round() as f64), fmt_real(zhi.round() as f64)));
        ctx.notes.info(format!(
            "heights of {} × {} cells of 30 m from {} files of the Copernicus DEM, {} to {} m",
            w,
            h,
            read,
            fmt_real(zlo.round() as f64),
            fmt_real(zhi.round() as f64)
        ));
        if sea > 0 || empty > 0 {
            ctx.notes.info(format!("{} of the cells are sea, where the DEM has no file; they are at 0 m", fmt_count(empty as u64)));
        }
        ctx.notes.info("heights: Copernicus DEM GLO-30, © DLR e.V. 2010–2014 and © Airbus Defence and Space GmbH 2014–2018, provided under COPERNICUS by the European Union and ESA");
        let grid = Grid { width: w, height: h, values, geotransform: gt, crs: Some(crs), bands: Vec::new(), meta: GridMeta::default() };
        heights_table = grid.into_table();
    }
    outputs.push(("Heights".into(), heights_table));

    // The picture: the least cloudy of the time, the clouds taken away, the rest put together.
    let mut image_table = Table { crs: Some(crs), raster: true, ..Table::default() };
    if want("image") {
        let cell = 10.0;
        let (w, h) = (((rect[2] - rect[0]) / cell).round() as usize, ((rect[3] - rect[1]) / cell).round() as usize);
        if w * h > 64_000_000 {
            return err("The picture would be too large: more than 8 000 × 8 000 pixels. Choose a smaller size, or heights only.");
        }
        let gt = [rect[0], cell, 0.0, rect[3], 0.0, -cell];
        let stac = p.get("stac").unwrap_or_else(|| "https://earth-search.aws.element84.com/v1".into());
        let time = match p.get("time").filter(|t| !t.trim().is_empty()) {
            Some(t) => super::web::stac_datetime(&t)?,
            None => {
                let now = chrono::Utc::now().date_naive();
                format!("{}T00:00:00Z/{}T23:59:59Z", now - chrono::Duration::days(365), now)
            }
        };
        let cloud = number_param(p, "cloud", "Most clouds")?.unwrap_or(30.0);
        let most = number_param(p, "most", "At most pictures")?.unwrap_or(6.0).clamp(1.0, 30.0) as usize;
        let brightness = number_param(p, "brightness", "Brightness")?.unwrap_or(1.0).max(0.05) as f32;
        let url = format!(
            "{}/search?collections=sentinel-2-l2a&limit=100&bbox={},{},{},{}&datetime={}&filter-lang=cql2-text&filter={}",
            stac.trim_end_matches('/').trim_end_matches("/search"),
            bbox[0],
            bbox[1],
            bbox[2],
            bbox[3],
            crate::web::encode(&time),
            crate::web::encode(&format!("eo:cloud_cover < {}", cloud))
        );
        say(&format!(
            "Picture: searching the Sentinel-2 pictures of {} with less than {} % clouds at {}",
            time.split('/').map(|t| t.chars().take(10).collect::<String>()).collect::<Vec<_>>().join(" to "),
            fmt_real(cloud),
            stac.trim_end_matches('/')
        ));
        let t = std::time::Instant::now();
        let mut client = Client::new(b, None, &stopped);
        let a = client.send("GET", &url, &[("Accept".to_string(), "application/geo+json".to_string())], None)?;
        if a.status >= 400 {
            return err(format!("The catalogue of pictures answered {}: {}", a.status, a.body.chars().take(200).collect::<String>()));
        }
        let v: serde_json::Value = serde_json::from_str(&a.body).map_err(|_| GbError("The catalogue of pictures did not answer with JSON.".into()))?;
        let mut items: Vec<Scene> = Vec::new();
        for it in v.get("features").and_then(|f| f.as_array()).into_iter().flatten() {
            let href = |k: &str| it.get("assets").and_then(|a| a.get(k)).and_then(|a| a.get("href")).and_then(|h| h.as_str()).map(str::to_string);
            let (Some(r), Some(g), Some(bl), Some(s)) = (href("red"), href("green"), href("blue"), href("scl")) else { continue };
            let props = it.get("properties");
            let cc = props.and_then(|p| p.get("eo:cloud_cover")).and_then(|c| c.as_f64()).unwrap_or(100.0);
            let when = props.and_then(|p| p.get("datetime")).and_then(|d| d.as_str()).unwrap_or("").chars().take(10).collect();
            items.push((cc, when, [r, g, bl, s], reflectance_of(it)));
        }
        if items.is_empty() {
            return err(format!(
                "No Sentinel-2 picture with less than {} % clouds was found for {} in {}. Allow more clouds, or another time.",
                fmt_real(cloud),
                label,
                time
            ));
        }
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        let found = items.len();
        items.truncate(most);
        say(&format!(
            "Picture: {} {} found in {}; the {} least cloudy are used: {}",
            found,
            if found == 1 { "picture" } else { "pictures" },
            secs(t),
            items.len(),
            items.iter().map(|i| format!("{} ({} %)", i.1, fmt_real(i.0.round()))).collect::<Vec<_>>().join(", ")
        ));
        // For each pixel the clear values of each picture.
        let n = w * h;
        let mut layers: Vec<[Vec<f32>; 3]> = Vec::new();
        let mut used = Vec::new();
        for (k, (cc, when, hrefs, (scale, offset))) in items.iter().enumerate() {
            if !gate2.going() {
                return err(crate::model::STOPPED);
            }
            progress("pictures", k as u64, Some(items.len() as u64));
            say(&format!("Picture {} of {}: reading red, green, blue and the classes of {}", k + 1, items.len(), when));
            let t = std::time::Instant::now();
            let read = |i: usize, nearest: bool| b.read_onto(&hrefs[i], gt, w, h, crs, nearest);
            let (Ok(r), Ok(g), Ok(bl), Ok(scl)) = (read(0, false), read(1, false), read(2, false), read(3, true)) else {
                ctx.notes.warn(format!("The picture of {} could not be read and was left out.", when));
                continue;
            };
            let mut l = [r, g, bl];
            // As reflectance, 0 to 1, by the scale and offset of the picture.
            for band in l.iter_mut() {
                for x in band.iter_mut() {
                    *x = *x * scale + offset;
                }
            }
            let mut cloudy = 0usize;
            for i in 0..n {
                // No data, clouds, their shadows and thin cirrus; snow is kept, as it is there.
                let bad = matches!(scl[i] as i32, 0 | 1 | 3 | 8 | 9 | 10) || scl[i].is_nan();
                if bad {
                    cloudy += 1;
                    for band in l.iter_mut() {
                        band[i] = f32::NAN;
                    }
                }
            }
            say(&format!(
                "Picture {} of {}: read in {}; {} % of it under clouds or their shadows is left out",
                k + 1,
                items.len(),
                secs(t),
                fmt_real((cloudy as f64 / n.max(1) as f64 * 100.0).round())
            ));
            used.push(format!("{} ({} % clouds)", when, fmt_real(cc.round())));
            layers.push(l);
        }
        if layers.is_empty() {
            return err("None of the pictures could be read.");
        }
        // The middle value of the clear ones; where none is clear, the least cloudy as it is.
        let mut rgba = vec![0u8; n * 4];
        let mut filled = 0usize;
        say(&format!("Picture: the clear values of {} pictures put together, the middle one in each pixel", layers.len()));
        // Reflectance of 0.3 is white, and the dark ground, forest and lava of some hundredths,
        // is lifted by a curve, as the eye sees it.
        let tone = |refl: f32| -> u8 { ((refl * brightness / 0.3).clamp(0.0, 1.0).powf(1.0 / 1.8) * 255.0).round() as u8 };
        let mut vals: Vec<f32> = Vec::with_capacity(layers.len());
        for i in 0..n {
            for c in 0..3 {
                vals.clear();
                vals.extend(layers.iter().map(|l| l[c][i]).filter(|v| !v.is_nan()));
                let v = if vals.is_empty() {
                    if c == 0 {
                        filled += 1;
                    }
                    0.0
                } else {
                    vals.sort_by(|a, b| a.total_cmp(b));
                    vals[vals.len() / 2]
                };
                rgba[i * 4 + c] = tone(v);
            }
            rgba[i * 4 + 3] = 255;
        }
        ctx.notes.info(format!("a picture of {} × {} pixels of 10 m from {} Sentinel-2 pictures: {}", w, h, layers.len(), used.join(", ")));
        if filled > 0 {
            ctx.notes.warn(format!("{} pixels were cloudy in every picture and are black. Allow more pictures, or another time.", fmt_count(filled as u64)));
        }
        ctx.notes.info("picture: contains modified Copernicus Sentinel data, processed by ESA");
        image_table = RasterImage { width: w, height: h, rgba, geotransform: gt, vector: None }.into_table(Some(crs));
    }
    outputs.push(("Image".into(), image_table));

    // The place, as the square around it.
    let square = geo::Polygon::new(
        geo::LineString::from(vec![(rect[0], rect[1]), (rect[2], rect[1]), (rect[2], rect[3]), (rect[0], rect[3]), (rect[0], rect[1])]),
        vec![],
    );
    let area = Table {
        fields: vec![
            Field::new("name", FieldType::Text),
            Field::new("lat", FieldType::Real),
            Field::new("lon", FieldType::Real),
            Field::new("epsg", FieldType::Int),
            Field::new("size_km", FieldType::Real),
        ],
        features: vec![Feature {
            z: None,
            attrs: vec![
                Value::Text(label),
                Value::Real((lat * 1e6).round() / 1e6),
                Value::Real((lon * 1e6).round() / 1e6),
                Value::Int(crs as i64),
                Value::Real(km),
            ],
            geom: Some(geo::Geometry::Polygon(square)),
        }],
        crs: Some(crs),
        ..Table::default()
    };
    outputs.push(("Area".into(), area));
    Ok(outputs)
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "PlaceFetcher",
    title: "Fetch a place",
    hidden: false,
    group: Group::Reader,
    category: "Terrain",
    desc: "Fetches heights and a picture from above of any place on Earth, by its name or its latitude and longitude: a square of the size given around it, in the UTM zone it lies in. The heights come from the Copernicus DEM GLO-30, 30 m, and the picture is made of the least cloudy Sentinel-2 pictures of a time, 10 m, the clouds taken away and the rest put together. Both are read over the web, only the part that is needed. Connect them to Shade terrain, Terrain model or Render 3D scene, and write a model for Blender.",
    inputs: &[],
    outputs: &["Heights", "Image", "Area"],
    ports: None,
    params: PARAMS,
    run: run_place,
    summary: |p| p.get("place").unwrap_or_default(),
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_find_their_zone_and_files() {
        // Reflectance by the catalogue, or by the processing baseline.
        let given = serde_json::json!({"assets": {"red": {"raster:bands": [{"scale": 0.0001, "offset": -0.1}]}}});
        assert_eq!(reflectance_of(&given), (0.0001, -0.1));
        let new = serde_json::json!({"properties": {"s2:processing_baseline": "05.10"}});
        assert_eq!(reflectance_of(&new), (0.0001, -0.1));
        let old = serde_json::json!({"properties": {"s2:processing_baseline": "03.01"}});
        assert_eq!(reflectance_of(&old), (0.0001, 0.0));
        let applied = serde_json::json!({"properties": {"s2:processing_baseline": "05.10", "earthsearch:boa_offset_applied": true}});
        assert_eq!(reflectance_of(&applied), (0.0001, 0.0));
        // As Earth Search has it: taken away, though the band still gives the offset.
        let earth_search = serde_json::json!({"assets": {"red": {"raster:bands": [{"scale": 0.0001, "offset": -0.1}]}}, "properties": {"earthsearch:boa_offset_applied": true}});
        assert_eq!(reflectance_of(&earth_search), (0.0001, 0.0));
        assert_eq!(utm_of(19.41, -155.28), 32605);
        assert_eq!(utm_of(-0.83, -91.13), 32715);
        assert_eq!(utm_of(45.98, 7.66), 32632);
        assert_eq!(dem_tile("https://x", 19, -156), "https://x/Copernicus_DSM_COG_10_N19_00_W156_00_DEM/Copernicus_DSM_COG_10_N19_00_W156_00_DEM.tif");
        assert_eq!(dem_tile("https://x/", -1, 91), "https://x/Copernicus_DSM_COG_10_S01_00_E091_00_DEM/Copernicus_DSM_COG_10_S01_00_E091_00_DEM.tif");
        assert_eq!(coordinates("19.41, -155.28"), Some((19.41, -155.28)));
        assert_eq!(coordinates("Kīlauea"), None);
    }
}
