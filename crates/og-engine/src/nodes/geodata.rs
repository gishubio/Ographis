//! Fetch geodata: the products of a national mapping agency, fetched for an area and put
//! together, by one tool for all of them. A product is a line in a table here: where its
//! catalogue is, how its collection is found, and what it becomes: heights, a photo from
//! above, a point cloud, features, or the items of the catalogue themselves. A new product, or a new provider, is a new line, not
//! a new tool.
//!
//! The products are in STAC catalogues. Searching them is open; the files need a login of
//! the provider, an API connection in the settings.

use super::*;
use crate::model::{err, fmt_count, fmt_real, Feature, Field, FieldType, GbError, Grid, GridMeta, RasterImage, Result, Table, Value};
use crate::web::Client;
use std::collections::HashMap;

/// What a product becomes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// Heights as a grid, from GeoTIFF files read where they are.
    Heights,
    /// A photo from above as a picture, from GeoTIFF files read where they are.
    Photo,
    /// A point cloud from LAZ files.
    Points,
    /// Features from zip files with GeoPackages in them.
    Features,
    /// The items of the catalogue themselves as features, with their properties, as the
    /// plans of the national geodata platform; their files are kept when asked for.
    Items,
}

/// A product of a provider.
struct Product {
    provider: &'static str,
    key: &'static str,
    label: &'static str,
    /// The STAC API the product is in.
    catalogue: &'static str,
    /// How its collection is found among those of the catalogue: every word must be in its
    /// id or title, and none of the words after a minus. The first that matches is taken.
    finds: &'static [&'static str],
    kind: Kind,
    /// The size of a cell when none is given, in metres.
    cell: f64,
    /// The coordinate system of the product.
    crs: u32,
}

/// Where Lantmäteriet's tokens are fetched with a consumer key and secret.
const LM_TOKEN: &str = "https://apimanager.lantmateriet.se/oauth2/token";

const PRODUCTS: &[Product] = &[
    Product {
        provider: "lantmateriet",
        key: "lm_addresses",
        label: "Belägenhetsadress: addresses",
        catalogue: "https://api.lantmateriet.se/stac-vektor/v1",
        finds: &["belägenhetsadress", "belagenhetsadress", "adress"],
        kind: Kind::Features,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "ngp_detailed_plans",
        label: "Detaljplan: detailed plans, from the national geodata platform",
        catalogue: "https://api.lantmateriet.se/distribution/geodatakatalog/sokning/v1/detaljplan/v2",
        finds: &[],
        kind: Kind::Items,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_divisions",
        label: "Kommun, Län och Rike: municipalities, counties and the country",
        catalogue: "https://api.lantmateriet.se/stac-vektor/v1",
        finds: &["kommun län rike", "kommun lan rike", "kommun län", "kommun lan", "kommun"],
        kind: Kind::Features,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_laser_forest",
        label: "Laserdata skog: point cloud",
        catalogue: "https://api.lantmateriet.se/stac-hojd/v1",
        finds: &["laserdata skog", "skoglig", "laser skog", "skog"],
        kind: Kind::Points,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_heights",
        label: "Markhöjdmodell: heights of the ground, 1 m",
        catalogue: "https://api.lantmateriet.se/stac-hojd/v1",
        finds: &["dtm", "markhöjd", "mhm"],
        kind: Kind::Heights,
        cell: 1.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_nmk50",
        label: "NMK 50: Nationell militär karta 1:50 000, as a picture",
        catalogue: "https://api.lantmateriet.se/stac-karta/v1",
        finds: &["nmk50", "nmk 50"],
        kind: Kind::Photo,
        cell: 2.5,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_nmk250",
        label: "NMK 250: Nationell militär karta 1:250 000, as a picture",
        catalogue: "https://api.lantmateriet.se/stac-karta/v1",
        finds: &["nmk250", "nmk 250"],
        kind: Kind::Photo,
        cell: 12.5,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_photo",
        label: "Ortofoto: photo from above",
        catalogue: "https://api.lantmateriet.se/stac-bild/v1",
        // Not the historical photos of the old grid, which are black and white: they are
        // chosen by name under Collection.
        finds: &["ortofoto -ir -historiska", "orto -ir -historiska", "ortofoto", "orto"],
        kind: Kind::Photo,
        cell: 0.5,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_topo10",
        label: "Topografi 10: map features, 1:10 000",
        catalogue: "https://api.lantmateriet.se/stac-vektor/v1",
        finds: &["topografi 10", "topografi10", "topo10"],
        kind: Kind::Features,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_topo50",
        label: "Topografi 50: map features, 1:50 000",
        catalogue: "https://api.lantmateriet.se/stac-vektor/v1",
        finds: &["topografi 50", "topografi50", "topo50"],
        kind: Kind::Features,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_topo250",
        label: "Topografi 250: map features, 1:250 000",
        catalogue: "https://api.lantmateriet.se/stac-vektor/v1",
        finds: &["topografi 250", "topografi250", "topo250"],
        kind: Kind::Features,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "lm_topo1m",
        label: "Topografi 1M: map features, 1:1 000 000",
        catalogue: "https://api.lantmateriet.se/stac-vektor/v1",
        finds: &["topografi 1m", "topografi1m", "topo1m", "topografi 1000"],
        kind: Kind::Features,
        cell: 0.0,
        crs: 3006,
    },
    Product {
        provider: "lantmateriet",
        key: "ngp_comprehensive_plans",
        label: "Översiktsplan: comprehensive plans, from the national geodata platform",
        catalogue: "https://api.lantmateriet.se/distribution/geodatakatalog/sokning/v2/oversiktsplan/v2",
        finds: &[],
        kind: Kind::Items,
        cell: 0.0,
        crs: 3006,
    },
];

const PROVIDERS: &[(&str, &str)] = &[("lantmateriet", "Lantmäteriet, Sweden")];

/// In the order of the alphabet, and the scales of a series from the largest.
const PRODUCT_CHOICES: &[(&str, &str)] = &[
    ("lm_addresses", "Belägenhetsadress: addresses"),
    ("ngp_detailed_plans", "Detaljplan: detailed plans, from the national geodata platform"),
    ("lm_divisions", "Kommun, Län och Rike: municipalities, counties and the country"),
    ("lm_laser_forest", "Laserdata skog: point cloud"),
    ("lm_heights", "Markhöjdmodell: heights of the ground, 1 m"),
    ("lm_nmk50", "NMK 50: Nationell militär karta 1:50 000, as a picture"),
    ("lm_nmk250", "NMK 250: Nationell militär karta 1:250 000, as a picture"),
    ("lm_photo", "Ortofoto: photo from above"),
    ("lm_topo10", "Topografi 10: map features, 1:10 000"),
    ("lm_topo50", "Topografi 50: map features, 1:50 000"),
    ("lm_topo250", "Topografi 250: map features, 1:250 000"),
    ("lm_topo1m", "Topografi 1M: map features, 1:1 000 000"),
    ("ngp_comprehensive_plans", "Översiktsplan: comprehensive plans, from the national geodata platform"),
];

const PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "provider", label: "From", kind: ParamKind::Choice(PROVIDERS), default: "lantmateriet", hint: "Who the geodata comes from" },
    ParamSpec { key: "product", label: "Product", kind: ParamKind::Choice(PRODUCT_CHOICES), default: "lm_heights", hint: "What to fetch" },
    ParamSpec {
        key: "bbox",
        label: "Area",
        kind: ParamKind::Text,
        default: "",
        hint: "A rectangle: west, south, east, north, such as 319000, 6399000, 321000, 6401000. A stream on Extent is used instead, if one is connected",
    },
    ParamSpec { key: "bbox_crs", label: "Area in", kind: ParamKind::Crs, default: "EPSG:3006", hint: "The coordinate system of the rectangle" },
    ParamSpec {
        key: "areas",
        label: "Several on Extent",
        kind: ParamKind::Choice(AREAS),
        default: "together",
        hint: "Each on its own: a municipality at a time, say, each with its own rectangle and largest area, and what is fetched gets its attributes",
    },
    ParamSpec {
        key: "max_km2",
        label: "Largest area, km²",
        kind: ParamKind::Text,
        default: "25",
        hint: "A larger area is not fetched, as the files are many and large",
    },
    ParamSpec {
        key: "cell_size",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "",
        hint: "Heights and photos: the side of a cell in metres. Empty: 1 m for heights, 0.5 m for photos",
    },
    ParamSpec {
        key: "year",
        label: "Year",
        kind: ParamKind::Text,
        default: "",
        hint: "Heights and photos: only from this year, or from and to, such as 2021/2024. Empty: the latest of each place",
    },
    ParamSpec {
        key: "layers",
        label: "Layers",
        kind: ParamKind::Text,
        default: "",
        hint: "Features: the layers to read, separated by commas, such as vagnat, byggnad. Empty: all",
    },
    ParamSpec {
        key: "las_classes",
        label: "Classes",
        kind: ParamKind::Text,
        default: "",
        hint: "Point clouds: only these classes, such as 2 for the ground. Empty: all",
    },
    ParamSpec {
        key: "las_every",
        label: "Every n-th point",
        kind: ParamKind::Text,
        default: "1",
        hint: "Point clouds: thins them, 10 reads one point in ten. 1 reads all",
    },
    ParamSpec {
        key: "las_spacing",
        label: "Spacing between points",
        kind: ParamKind::Text,
        default: "",
        hint: "Point clouds: only the levels of the files down to about this many metres between points, such as 2 for an overview of a large area. Empty reads all",
    },
    ParamSpec {
        key: "max_points",
        label: "Most points",
        kind: ParamKind::Text,
        default: "50000000",
        hint: "Point clouds: more are not read, so that the memory of the computer is not filled. How many there will be is told before anything is fetched; thin them, or raise this if the computer has the memory",
    },
    ParamSpec {
        key: "keep_in",
        label: "Keep the files in",
        kind: ParamKind::Text,
        default: "",
        hint: "A folder where every file of the area is kept whole, all that the provider offers with it, such as pictures and PDF, under their own names, by collection: to read again later or in other programs. Relative to the folder of the model, such as data/lantmateriet. A file that is there already is not fetched again. Empty: only what is needed is read, and files that must be fetched whole are kept for a while in the temporary folder of the computer",
    },
    ParamSpec {
        key: "connection",
        label: "API connection",
        kind: ParamKind::ApiConnection,
        default: "",
        hint: "The login for the files. For Lantmäteriet: a user and a password, those of Geotorget; or OAuth2 with a key and secret of its API portal. The products must be ordered on Geotorget",
    },
    ParamSpec {
        key: "collection",
        label: "Collection",
        kind: ParamKind::Text,
        default: "",
        hint: "The collection of the catalogue, when another than the one found by itself. The log lists them",
    },
    ParamSpec {
        key: "catalogue",
        label: "Catalogue",
        kind: ParamKind::Text,
        default: "",
        hint: "Another address of the STAC API of the product. Empty: the provider's",
    },
];

/// A long list of names, as its first ones and how many more, so that a line of the log
/// stays readable. Under Collection, any of them can be chosen by name.
fn shortened(names: &[String]) -> String {
    const SHOWN: usize = 30;
    if names.len() <= SHOWN {
        names.join(", ")
    } else {
        format!("{} and {} more", names[..SHOWN].join(", "), names.len() - SHOWN)
    }
}

/// Does a collection match words such as "ortofoto -ir": all words in its id or title, and
/// none of those after a minus?
fn matches(words: &str, id: &str, title: &str) -> bool {
    let text = format!("{} {}", id, title).to_lowercase().replace(['-', '_'], " ");
    let tokens: Vec<&str> = text.split_whitespace().collect();
    words.split_whitespace().all(|w| match w.strip_prefix('-') {
        Some(not) => !tokens.contains(&not),
        // A word with a figure, such as 10 or 1m, is a whole word: 10 is not in 100.
        None if w.chars().any(|c| c.is_ascii_digit()) => tokens.contains(&w),
        None => text.contains(w),
    })
}

/// The years of a search as a STAC time.
fn years(text: &str) -> Result<String> {
    let parts: Vec<&str> = text.split('/').map(str::trim).collect();
    let year = |y: &str| y.parse::<u32>().ok().filter(|y| (1900..=2200).contains(y)).ok_or_else(|| GbError(format!("\"{}\" is not a year.", y)));
    match parts.as_slice() {
        [y] => {
            let y = year(y)?;
            Ok(format!("{}-01-01T00:00:00Z/{}-12-31T23:59:59Z", y, y))
        }
        [a, b] => Ok(format!("{}-01-01T00:00:00Z/{}-12-31T23:59:59Z", year(a)?, year(b)?)),
        _ => err("Write a year, such as 2023, or from and to, such as 2021/2024."),
    }
}

/// An item of the catalogue: its id, collection, time, rectangle in degrees, the address of
/// its file and its footprint.
struct Item {
    id: String,
    collection: String,
    when: String,
    bbox: [f64; 4],
    href: String,
    geometry: Option<geo::Geometry<f64>>,
    /// Every file of the item: the one read and the others, such as a PDF or a preview.
    assets: Vec<String>,
    /// What the catalogue says of each file: its title, its type and its roles.
    documents: Vec<Document>,
    /// What the catalogue says of it, for a product of its items.
    properties: serde_json::Map<String, serde_json::Value>,
}

/// A file of an item as the catalogue tells of it.
#[derive(Clone)]
struct Document {
    key: String,
    title: String,
    kind: String,
    roles: String,
    href: String,
}

/// The file of an item: the asset called data, or else the first that is a file of the kind.
fn file_of(item: &serde_json::Value, kind: Kind) -> Option<String> {
    let assets = item.get("assets")?.as_object()?;
    let href = |a: &serde_json::Value| a.get("href").and_then(|h| h.as_str()).map(str::to_string);
    let fits = |h: &str| {
        let h = h.to_lowercase();
        let h = h.split(['?', '#']).next().unwrap_or("");
        match kind {
            Kind::Heights | Kind::Photo => h.ends_with(".tif") || h.ends_with(".tiff") || h.ends_with(".jp2"),
            Kind::Points => h.ends_with(".laz") || h.ends_with(".las"),
            Kind::Features => h.ends_with(".zip") || h.ends_with(".gpkg"),
            Kind::Items => true,
        }
    };
    if let Some(h) = assets.get("data").and_then(href).filter(|h| fits(h)) {
        return Some(h);
    }
    // Photos: the colours rather than the infrared.
    let mut all: Vec<(&String, String)> = assets.iter().filter_map(|(k, a)| href(a).filter(|h| fits(h)).map(|h| (k, h))).collect();
    all.sort_by_key(|(k, _)| k.to_lowercase().contains("ir") && !k.to_lowercase().contains("rgb"));
    all.into_iter().next().map(|(_, h)| h)
}

/// The number of points and the rectangle (west, south, east, north) in the header of a LAS
/// or LAZ file, from its first 375 bytes.
fn las_header(h: &[u8]) -> Option<(u64, [f64; 4])> {
    if h.len() < 227 || &h[0..4] != b"LASF" {
        return None;
    }
    let f = |at: usize| h.get(at..at + 8).map(|b| f64::from_le_bytes(b.try_into().unwrap()));
    let (max_x, min_x, max_y, min_y) = (f(179)?, f(187)?, f(195)?, f(203)?);
    let legacy = u32::from_le_bytes(h[107..111].try_into().ok()?) as u64;
    // Version 1.4 keeps the number in 64 bits, as there may be more than 4 billion.
    let count = if h[25] >= 4 && h.len() >= 255 { u64::from_le_bytes(h[247..255].try_into().ok()?) } else { legacy };
    Some((count.max(legacy), [min_x, min_y, max_x, max_y]))
}

/// An error in fetching a file, with what to do when the provider wanted a login.
fn refused(e: GbError, client: &Client, login: bool) -> GbError {
    let text = client.hide(&e.0);
    if text.contains("401") || text.contains("403") {
        let what = if login {
            "The provider refused the file with the login given. Has this product been ordered for the account, and is the login right?"
        } else {
            "The provider wants a login for the file: choose an API connection with its key, and order the product from the provider."
        };
        GbError(format!("{} {}", text, what))
    } else {
        GbError(text)
    }
}

/// How several features on Extent are fetched.
const AREAS: &[(&str, &str)] = &[("together", "As one area: the rectangle around them all"), ("each", "Each on its own, with its attributes")];

/// Fetches for the area on Extent, or for each feature on it on its own when asked: then
/// what is fetched for each gets the attributes of its area, and all is put together.
fn run_geodata(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let extent = if ctx.open_ports.contains(&"Extent") { None } else { inp.get("Extent").cloned() };
    let each = p.get("areas").as_deref() == Some("each");
    let Some(extent) = extent.filter(|e| each && e.features.iter().filter(|f| f.geom.is_some()).count() > 1) else {
        return fetch_area(p, inp, ctx);
    };
    let areas: Vec<&Feature> = extent.features.iter().filter(|f| f.geom.is_some()).collect();
    let mut got: Vec<(std::borrow::Cow<'static, str>, Vec<Arc<Table>>)> = Vec::new();
    for (i, area) in areas.iter().enumerate() {
        let say_now = ctx.say;
        say_now(crate::run::Level::Info, &format!("Area {} of {}", i + 1, areas.len()));
        let mut one = extent.like();
        one.features.push((*area).clone());
        let mut inp1 = inp.clone();
        inp1.insert("Extent", Arc::new(one));
        let outs = fetch_area(p, &inp1, ctx).map_err(|e| GbError(format!("Area {} of {}: {}", i + 1, areas.len(), e.0)))?;
        for (port, mut t) in outs {
            // What is fetched as features gets the attributes of its area.
            if !t.features.is_empty() && t.points.is_none() {
                for (k, field) in extent.fields.iter().enumerate() {
                    if t.field_index(&field.name).is_none() {
                        t.fields.push(field.clone());
                        for f in t.features.iter_mut() {
                            f.attrs.push(area.attrs.get(k).cloned().unwrap_or(Value::Null));
                        }
                    }
                }
            }
            match got.iter_mut().find(|(n, _)| *n == port) {
                Some((_, list)) => list.push(Arc::new(t)),
                None => got.push((port, vec![Arc::new(t)])),
            }
        }
    }
    let mut outs = Vec::new();
    for (port, tables) in got {
        let merged = crate::nodes::merge_tables(tables, ctx.backend, "areas", &mut ctx.notes)?;
        outs.push((port, Arc::try_unwrap(merged).unwrap_or_else(|a| (*a).clone())));
    }
    ctx.notes.info(format!("{} areas fetched one at a time", areas.len()));
    Ok(outs)
}

fn fetch_area(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let b = ctx.backend("Fetching geodata")?;
    let provider = p.get("provider").unwrap_or_else(|| "lantmateriet".into());
    let key = p.get("product").unwrap_or_else(|| "lm_heights".into());
    let product = PRODUCTS
        .iter()
        .find(|x| x.key == key)
        .ok_or_else(|| GbError(format!("\"{}\" is not a product. Choose among {}.", key, PRODUCTS.iter().map(|x| x.key).collect::<Vec<_>>().join(", "))))?;
    if product.provider != provider {
        return err(format!("{} is not a product of {}.", product.label, PROVIDERS.iter().find(|x| x.0 == provider).map_or(provider.as_str(), |x| x.1)));
    }
    let crs = product.crs;
    let say_now = ctx.say;
    let say = |m: &str| say_now(crate::run::Level::Info, m);
    let secs = |t: std::time::Instant| format!("{:.1} s", t.elapsed().as_secs_f64());

    // The area, in the coordinates of the product and in degrees.
    let extent = if ctx.open_ports.contains(&"Extent") { None } else { Some(input(inp, "Extent")) };
    let corners = |r: [f64; 4]| (vec![r[0], r[2], r[0], r[2], (r[0] + r[2]) / 2.0], vec![r[1], r[1], r[3], r[3], (r[1] + r[3]) / 2.0]);
    let span = |xs: &[f64], ys: &[f64]| {
        let lo = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        [lo(xs), lo(ys), hi(xs), hi(ys)]
    };
    let (given, given_crs) = if let Some(extent) = extent.filter(|e| e.has_geometry() && !e.features.is_empty()) {
        use geo::BoundingRect;
        let mut r: Option<[f64; 4]> = None;
        for g in extent.features.iter().filter_map(|f| f.geom.as_ref()) {
            if let Some(x) = g.bounding_rect() {
                r = Some(match r {
                    None => [x.min().x, x.min().y, x.max().x, x.max().y],
                    Some(o) => [o[0].min(x.min().x), o[1].min(x.min().y), o[2].max(x.max().x), o[3].max(x.max().y)],
                });
            }
        }
        let crs_in = extent.crs.ok_or_else(|| GbError("What comes in on Extent has no coordinate system, so where it lies is not known.".into()))?;
        (r.ok_or_else(|| GbError("What comes in on Extent has no geometry.".into()))?, crs_in)
    } else {
        let text = p.need("bbox", "Write the area as west, south, east, north, or connect something to Extent.")?;
        let v: Vec<f64> = text.split([',', ';', ' ']).filter(|t| !t.trim().is_empty()).filter_map(|t| t.trim().parse().ok()).collect();
        if v.len() != 4 || v[0] >= v[2] || v[1] >= v[3] {
            return err(format!("\"{}\" is not a rectangle. Write west, south, east, north, such as 319000, 6399000, 321000, 6401000.", text));
        }
        let crs_in = match p.get("bbox_crs").filter(|c| !c.trim().is_empty()) {
            Some(c) => crate::model::parse_crs(&c)?,
            None => 3006,
        };
        ([v[0], v[1], v[2], v[3]], crs_in)
    };
    let (mut xs, mut ys) = corners(given);
    b.transform(given_crs, crs, &mut xs, &mut ys)?;
    let rect = span(&xs, &ys);
    let km2 = (rect[2] - rect[0]) * (rect[3] - rect[1]) / 1e6;
    let most = number_param(p, "max_km2", "The largest area")?.unwrap_or(25.0).max(0.001);
    if km2 > most {
        return err(format!(
            "The area is {} km², more than the {} km² allowed. Choose a smaller area, or raise Largest area if you mean it.",
            fmt_real((km2 * 100.0).round() / 100.0),
            fmt_real(most)
        ));
    }
    let (mut dx, mut dy) = corners(rect);
    b.transform(crs, 4326, &mut dx, &mut dy)?;
    let degrees = span(&dx, &dy);
    say(&format!(
        "{}: {} × {} m, {} km², in EPSG:{}",
        product.label,
        fmt_real((rect[2] - rect[0]).round()),
        fmt_real((rect[3] - rect[1]).round()),
        fmt_real((km2 * 100.0).round() / 100.0),
        crs
    ));

    // The catalogue and the collection in it.
    let gate = ctx.gate;
    let stopped = move || !gate.going();
    let base = p.get("catalogue").filter(|c| !c.trim().is_empty()).unwrap_or_else(|| product.catalogue.to_string());
    let base = base.trim().trim_end_matches('/').trim_end_matches("/search").to_string();
    let auth = match p.get("connection").filter(|c| !c.trim().is_empty()) {
        Some(name) => {
            let mut c = crate::web::connection(&name)?;
            if c.auth == "oauth2" && c.token_url.trim().is_empty() && provider == "lantmateriet" {
                c.token_url = LM_TOKEN.into();
            }
            Some(c)
        }
        None => None,
    };
    let json = [("Accept".to_string(), "application/json, application/geo+json".to_string())];
    let mut open = Client::new(b, None, &stopped);
    let mut login = Client::new(b, auth.clone(), &stopped);
    // A catalogue that wants the login for searching too gets it, from then on.
    let mut with_login = false;
    let mut ask = |method: &str, url: &str, body: Option<&str>, with_login: &mut bool| -> Result<crate::io::HttpAnswer> {
        let mut headers = json.to_vec();
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        if !*with_login {
            let a = open.send(method, url, &headers, body)?;
            if !(matches!(a.status, 401 | 403) && auth.is_some()) {
                return Ok(a);
            }
            *with_login = true;
        }
        login.send(method, url, &headers, body)
    };
    // The collection: found by its words among those of the catalogue, or none for a
    // catalogue of one dataset, as those of the national geodata platform.
    let asked = p.get("collection").filter(|c| !c.trim().is_empty()).map(|c| c.trim().to_string());
    // Every collection the words fit, as a catalogue with one collection for each year has:
    // all are searched, and the latest file of a place is used.
    let mut also: Vec<String> = Vec::new();
    let chosen: (String, String, String) = if product.finds.is_empty() && asked.is_none() {
        (String::new(), product.label.split(':').next().unwrap_or(product.label).to_string(), String::new())
    } else {
        let t = std::time::Instant::now();
        let a = ask("GET", &format!("{}/collections", base), None, &mut with_login)?;
        if a.status >= 400 {
            return Err(refused(
                GbError(format!("The catalogue {} answered {}: {}", base, a.status, a.body.chars().take(200).collect::<String>())),
                &login,
                auth.is_some(),
            ));
        }
        let v: serde_json::Value = serde_json::from_str(&a.body).map_err(|_| GbError(format!("The catalogue {} did not answer with JSON.", base)))?;
        let collections: Vec<(String, String, String)> = v
            .get("collections")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let id = c.get("id")?.as_str()?.to_string();
                let title = c.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let license = c.get("license").and_then(|t| t.as_str()).unwrap_or("").to_string();
                Some((id, title, license))
            })
            .collect();
        let list = collections.iter().map(|c| if c.1.is_empty() { c.0.clone() } else { format!("{} ({})", c.0, c.1) }).collect::<Vec<_>>().join(", ");
        say(&format!(
            "The catalogue {} has {} collections, in {}: {}",
            base,
            collections.len(),
            secs(t),
            shortened(&collections.iter().map(|c| c.0.clone()).collect::<Vec<_>>())
        ));
        match &asked {
            Some(c) => {
                collections.iter().find(|x| x.0 == *c).cloned().ok_or_else(|| GbError(format!("The catalogue has no collection {}. It has: {}.", c, list)))?
            }
            None => {
                let fit: Vec<&(String, String, String)> = product
                    .finds
                    .iter()
                    .map(|w| collections.iter().filter(|c| matches(w, &c.0, &c.1)).collect::<Vec<_>>())
                    .find(|f| !f.is_empty())
                    .unwrap_or_default();
                also = fit.iter().skip(1).map(|c| c.0.clone()).collect();
                fit.first().map(|c| (*c).clone()).ok_or_else(|| {
                    GbError(format!("No collection of {} was found in the catalogue. Choose one under Collection; it has: {}.", product.label, list))
                })?
            }
        }
    };
    if !chosen.0.is_empty() {
        say(&format!("Collection: {}{}", chosen.0, if chosen.1.is_empty() { String::new() } else { format!(", {}", chosen.1) }));
    }
    if !also.is_empty() {
        say(&format!("Also searched, {} more: {}. The latest file of each place is used", also.len(), shortened(&also)));
    }
    let searched: Vec<String> = std::iter::once(chosen.0.clone()).filter(|c| !c.is_empty()).chain(also.iter().cloned()).collect();

    // The items that reach into the area: asked for with GET, or with POST by a catalogue
    // that takes only that.
    let datetime = match p.get("year").filter(|y| !y.trim().is_empty()) {
        Some(y) => Some(years(&y)?),
        None => None,
    };
    // The search of the national geodata platform takes its rectangle in SWEREF 99 TM, as
    // its products are; other catalogues take degrees, as STAC has it.
    let platform = product.key.starts_with("ngp_") || base.contains("/geodatakatalog/sokning/");
    let bbox = if platform && crs == 3006 { rect } else { degrees };
    let mut url = format!("{}/search?bbox={},{},{},{}&limit=100", base, bbox[0], bbox[1], bbox[2], bbox[3]);
    if !searched.is_empty() {
        url.push_str(&format!("&collections={}", searched.iter().map(|c| crate::web::encode(c)).collect::<Vec<_>>().join(",")));
    }
    if let Some(d) = &datetime {
        url.push_str(&format!("&datetime={}", crate::web::encode(d)));
    }
    let mut post = serde_json::json!({"bbox": bbox, "limit": 100});
    if !searched.is_empty() {
        post["collections"] = serde_json::json!(searched);
    }
    if let Some(d) = &datetime {
        post["datetime"] = serde_json::json!(d);
    }
    let t = std::time::Instant::now();
    let mut found: Vec<serde_json::Value> = Vec::new();
    let mut next: Option<(String, String, Option<serde_json::Value>)> = Some(("GET".into(), url, None));
    while let Some((method, u, body)) = next.take() {
        if !gate.going() {
            return err(crate::model::STOPPED);
        }
        let text = body.as_ref().map(|b| b.to_string());
        let mut a = ask(&method, &u, text.as_deref(), &mut with_login)?;
        let mut sent = (method, body);
        if sent.0 == "GET" && matches!(a.status, 404 | 405) && found.is_empty() {
            let search = format!("{}/search", base);
            a = ask("POST", &search, Some(&post.to_string()), &mut with_login)?;
            sent = ("POST".into(), Some(post.clone()));
        }
        if a.status >= 400 {
            return Err(refused(
                GbError(format!("The search answered {}: {}", a.status, a.body.chars().take(300).collect::<String>())),
                &login,
                auth.is_some(),
            ));
        }
        let page: serde_json::Value = serde_json::from_str(&a.body).map_err(|_| GbError("The search did not answer with JSON.".into()))?;
        let got = page.get("features").and_then(|f| f.as_array()).map_or(0, |f| f.len());
        found.extend(page.get("features").and_then(|f| f.as_array()).into_iter().flatten().cloned());
        // The next page: an address, and for POST a body, whole or to merge with the last.
        next =
            page.get("links").and_then(|l| l.as_array()).into_iter().flatten().find(|l| l.get("rel").and_then(|r| r.as_str()) == Some("next")).and_then(|l| {
                let href = l.get("href").and_then(|h| h.as_str())?.to_string();
                let method = l.get("method").and_then(|m| m.as_str()).unwrap_or("GET").to_uppercase();
                let body = if method == "POST" {
                    let mut b = if l.get("merge").and_then(|m| m.as_bool()) == Some(true) {
                        sent.1.clone().unwrap_or(serde_json::json!({}))
                    } else {
                        serde_json::json!({})
                    };
                    if let (Some(to), Some(from)) = (b.as_object_mut(), l.get("body").and_then(|x| x.as_object())) {
                        for (k, v) in from {
                            to.insert(k.clone(), v.clone());
                        }
                    }
                    Some(b)
                } else {
                    None
                };
                Some((method, href, body))
            });
        // Files are many megabytes each, items such as plans are small: a city has thousands.
        let cap = if product.kind == Kind::Items { 50_000 } else { 2000 };
        if got == 0 || found.len() >= cap {
            if found.len() >= cap {
                ctx.notes.warn(format!("The search was stopped at {} items; choose a smaller area.", fmt_count(cap as u64)));
            }
            break;
        }
    }
    let mut items: Vec<Item> = Vec::new();
    let mut without = 0usize;
    for it in &found {
        let href = match file_of(it, product.kind) {
            Some(h) => h,
            // An item of a product of items need have no file.
            None if product.kind == Kind::Items => String::new(),
            None => {
                without += 1;
                continue;
            }
        };
        let bb: Vec<f64> = it.get("bbox").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|v| v.as_f64()).collect()).unwrap_or_default();
        let props = it.get("properties");
        let when = props
            .and_then(|p| p.get("datetime").filter(|d| !d.is_null()).or_else(|| p.get("start_datetime")))
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        let mut geometry = it
            .get("geometry")
            .filter(|g| !g.is_null())
            .and_then(|g| crate::geomtext::parse(&g.to_string(), Some(crate::geomtext::Kind::GeoJson)).ok())
            .map(|g| g.0);
        // A catalogue may give its items in the coordinates of the product rather than in
        // degrees: then they are brought to degrees, as the others.
        let mut bb = bb;
        if bb.iter().any(|v| v.abs() > 360.0) || geometry.as_ref().is_some_and(|g| geo::BoundingRect::bounding_rect(g).is_some_and(|r| r.max().x.abs() > 360.0))
        {
            if let Some(g) = geometry.take() {
                let one = Table { features: vec![Feature { z: None, attrs: vec![], geom: Some(g) }], crs: Some(crs), ..Table::default() };
                geometry = reproject(&one, 4326, Some(b))?.features.pop().and_then(|f| f.geom);
            }
            bb = match geometry.as_ref().and_then(geo::BoundingRect::bounding_rect) {
                Some(r) => vec![r.min().x, r.min().y, r.max().x, r.max().y],
                None if bb.len() >= 4 => {
                    let (mut xs, mut ys) = corners([bb[0], bb[1], bb[2], bb[3]]);
                    b.transform(crs, 4326, &mut xs, &mut ys)?;
                    span(&xs, &ys).to_vec()
                }
                None => Vec::new(),
            };
        }
        items.push(Item {
            id: it.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
            collection: it.get("collection").and_then(|c| c.as_str()).unwrap_or(if chosen.0.is_empty() { &chosen.1 } else { &chosen.0 }).to_string(),
            when,
            bbox: if bb.len() >= 4 { [bb[0], bb[1], bb[2], bb[3]] } else { degrees },
            href,
            geometry,
            assets: it
                .get("assets")
                .and_then(|a| a.as_object())
                .map(|a| a.values().filter_map(|v| v.get("href").and_then(|h| h.as_str()).map(str::to_string)).collect())
                .unwrap_or_default(),
            documents: it
                .get("assets")
                .and_then(|a| a.as_object())
                .map(|a| {
                    a.iter()
                        .filter_map(|(k, v)| {
                            let text = |n: &str| v.get(n).and_then(|x| x.as_str()).unwrap_or("").to_string();
                            Some(Document {
                                key: k.clone(),
                                title: text("title"),
                                kind: text("type"),
                                roles: v
                                    .get("roles")
                                    .and_then(|r| r.as_array())
                                    .map(|r| r.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
                                    .unwrap_or_default(),
                                href: v.get("href")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            properties: props.and_then(|p| p.as_object()).cloned().unwrap_or_default(),
        });
    }
    // The latest first: where files of several years cover a place, the latest is used.
    items.sort_by(|a, b| b.when.cmp(&a.when));
    let (one, many) = if product.kind == Kind::Items { ("item", "items") } else { ("file", "files") };
    say(&format!("{} {} found in {}", items.len(), if items.len() == 1 { one } else { many }, secs(t)));
    if without > 0 {
        ctx.notes.warn(format!("{} items of the catalogue had no file that Ographis reads, and were left out.", without));
    }
    if items.is_empty() {
        return err(format!(
            "{} has nothing in the area{}. Is the area in Sweden, and in the coordinate system given?",
            if searched.is_empty() { product.label.split(':').next().unwrap_or(product.label).to_string() } else { searched.join(", ") },
            p.get("year").filter(|y| !y.trim().is_empty()).map(|y| format!(" from {}", y)).unwrap_or_default()
        ));
    }
    if auth.is_none() {
        ctx.notes.info("no API connection is chosen; the files are fetched without a login");
    }

    // The files are fetched with the login.
    let mut client = Client::new(b, auth.clone(), &stopped);
    let (progress, gate2) = (ctx.progress, ctx.gate);
    let total = items.len();
    let mut used: Vec<bool> = vec![false; total];
    let name_of = |href: &str| href.split(['?', '#']).next().unwrap_or(href).rsplit('/').next().unwrap_or("").to_string();

    // Every file of the area kept whole in a folder, when asked for, and read from there.
    let keep_dir = p.get("keep_in").filter(|d| !d.trim().is_empty()).map(|d| ctx.resolve(d.trim()));
    let mut kept: Vec<Option<std::path::PathBuf>> = vec![None; total];
    // Where each file kept lies, by its address.
    let mut kept_files: HashMap<String, std::path::PathBuf> = HashMap::new();
    if let Some(dir) = &keep_dir {
        let (mut fetched, mut there) = (0usize, 0usize);
        let t = std::time::Instant::now();
        for (k, it) in items.iter().enumerate() {
            if !gate2.going() {
                return err(crate::model::STOPPED);
            }
            progress("files kept", k as u64, Some(total as u64));
            let folder = dir.join(it.collection.replace(['/', '\\', ':'], "_"));
            std::fs::create_dir_all(&folder).map_err(|e| GbError(format!("Cannot make the folder {}: {}", folder.display(), e)))?;
            // Which address each file there came from, for the files whose address does not
            // end in their name, such as /asset/{id}.
            let index_file = folder.join(".ographis-files.json");
            let mut index: serde_json::Map<String, serde_json::Value> =
                std::fs::read_to_string(&index_file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            for href in &it.assets {
                let from_address = name_of(href);
                let known = index.get(href).and_then(|v| v.as_str()).map(str::to_string);
                let name = known.clone().unwrap_or_else(|| from_address.clone());
                let target = folder.join(&name);
                // There already: kept from this address, or of this name and from no other.
                let elsewhere = known.is_none() && index.values().any(|v| v.as_str() == Some(name.as_str()));
                if !name.is_empty() && (known.is_some() || (name.contains('.') && !elsewhere)) && target.is_file() {
                    there += 1;
                } else {
                    say(&format!("Keeping {} of item {} of {}", if from_address.is_empty() { href.as_str() } else { from_address.as_str() }, k + 1, total));
                    let (u, headers) = client.prepared(href)?;
                    let mut notes = Notes::default();
                    let file = b.fetch_with(&u, &headers, false, &mut notes).map_err(|e| refused(e, &client, auth.is_some()))?;
                    // The name the server gives, when the address does not end in one.
                    let name = if from_address.contains('.') {
                        from_address.clone()
                    } else {
                        file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| format!("{}.bin", from_address))
                    };
                    // Another file of the same name, from another item, is not written over.
                    let name = if folder.join(&name).exists() { format!("{}_{}", it.id.replace(['/', '\\', ':'], "_"), name) } else { name };
                    let target = folder.join(&name);
                    // Moved from where it was fetched, or copied when that is on another disk.
                    if std::fs::rename(&file, &target).is_err() {
                        std::fs::copy(&file, &target).map_err(|e| GbError(format!("Cannot keep {} in {}: {}", name, folder.display(), e)))?;
                        let _ = std::fs::remove_file(&file);
                    }
                    index.insert(href.clone(), serde_json::Value::String(name.clone()));
                    fetched += 1;
                }
                let name = index.get(href).and_then(|v| v.as_str()).map(str::to_string).unwrap_or(name);
                kept_files.insert(href.clone(), folder.join(&name));
                if *href == it.href {
                    kept[k] = Some(folder.join(name));
                }
            }
            if !index.is_empty() {
                let _ = std::fs::write(&index_file, serde_json::Value::Object(index).to_string());
            }
        }
        say(&format!("{} files fetched and kept, {} there already, in {}: {}", fetched, there, secs(t), dir.display()));
        ctx.notes.info(format!("the files are kept in {}", dir.display()));
    }
    let output = match product.kind {
        Kind::Heights | Kind::Photo => {
            let cell = number_param(p, "cell_size", "The cell size")?.filter(|c| *c > 0.0).unwrap_or(product.cell);
            let snap = |v: f64, up: bool| if up { (v / cell).ceil() * cell } else { (v / cell).floor() * cell };
            let r = [snap(rect[0], false), snap(rect[1], false), snap(rect[2], true), snap(rect[3], true)];
            let (w, h) = (((r[2] - r[0]) / cell).round() as usize, ((r[3] - r[1]) / cell).round() as usize);
            if w * h > 100_000_000 {
                return err(format!("{} × {} cells of {} m are too many. Choose a larger cell size or a smaller area.", w, h, fmt_real(cell)));
            }
            let photo = product.kind == Kind::Photo;
            let n = w * h;
            let mut whole = false;
            // Heights as numbers; a photo as its colours, a quarter of the memory. A cell
            // without a value is NaN, or see-through.
            let mut heights: Vec<f32> = if photo { Vec::new() } else { vec![f32::NAN; n] };
            let mut rgba: Vec<u8> = if photo { vec![0u8; n * 4] } else { Vec::new() };
            let empty_at = |heights: &[f32], rgba: &[u8], at: usize| if photo { rgba[at * 4 + 3] == 0 } else { heights[at].is_nan() };
            for (k, it) in items.iter().enumerate() {
                if !gate2.going() {
                    return err(crate::model::STOPPED);
                }
                progress("files", k as u64, Some(total as u64));
                // The part of the area the file covers, on whole cells.
                let (mut ix, mut iy) = corners(it.bbox);
                b.transform(4326, crs, &mut ix, &mut iy)?;
                let fb = span(&ix, &iy);
                let c0 = (((fb[0] - r[0]) / cell).floor().max(0.0) as usize).min(w);
                let c1 = (((fb[2] - r[0]) / cell).ceil().max(0.0) as usize).min(w);
                let r0 = (((r[3] - fb[3]) / cell).floor().max(0.0) as usize).min(h);
                let r1 = (((r[3] - fb[1]) / cell).ceil().max(0.0) as usize).min(h);
                if c1 <= c0 || r1 <= r0 {
                    continue;
                }
                // Covered already by a later file.
                if (r0..r1).all(|row| (c0..c1).all(|col| !empty_at(&heights, &rgba, row * w + col))) {
                    continue;
                }
                let (pw, ph) = (c1 - c0, r1 - r0);
                let gt = [r[0] + c0 as f64 * cell, cell, 0.0, r[3] - r0 as f64 * cell, 0.0, -cell];
                let name = name_of(&it.href);
                say(&format!(
                    "File {} of {}: reading {} × {} cells of {}{}",
                    k + 1,
                    total,
                    pw,
                    ph,
                    name,
                    if it.when.is_empty() { String::new() } else { format!(", {}", &it.when[..it.when.len().min(10)]) }
                ));
                let t = std::time::Instant::now();
                let bands = if let Some(file) = &kept[k] {
                    b.read_bands_onto(&file.to_string_lossy(), &[], gt, pw, ph, crs, false)?
                } else {
                    let (u, headers) = client.prepared(&it.href)?;
                    // Read where it is, only the part needed; when the server does not let it
                    // be read so, such as when it sends the file on from somewhere else, the
                    // file is fetched whole and kept for a while, and so are the next.
                    let remote = if whole { None } else { Some(b.read_bands_onto(&u, &headers, gt, pw, ph, crs, false)) };
                    match remote {
                        Some(Ok(bands)) => bands,
                        Some(Err(e)) if e.0.contains(" 401") || e.0.contains(" 403") => return Err(refused(e, &client, auth.is_some())),
                        other => {
                            if let Some(Err(e)) = other {
                                let why = client.hide(&e.0);
                                say(&format!(
                                    "File {} of {}: it cannot be read where it is ({}); it is fetched whole instead",
                                    k + 1,
                                    total,
                                    why.chars().take(200).collect::<String>()
                                ));
                                whole = true;
                            }
                            let mut notes = Notes::default();
                            let file = b.fetch_with(&u, &headers, true, &mut notes).map_err(|e| refused(e, &client, auth.is_some()))?;
                            b.read_bands_onto(&file.to_string_lossy(), &[], gt, pw, ph, crs, false)?
                        }
                    }
                };
                used[k] = true;
                // Photos of 16 bits are brought down to 8.
                let deep = photo && bands.iter().take(3).any(|band| band.iter().any(|v| *v > 255.0));
                for row in 0..ph {
                    for col in 0..pw {
                        let at = (r0 + row) * w + c0 + col;
                        let from = row * pw + col;
                        if !empty_at(&heights, &rgba, at) || bands[0][from].is_nan() {
                            continue;
                        }
                        if photo {
                            for c in 0..3 {
                                let x = bands.get(c).unwrap_or(&bands[0])[from];
                                rgba[at * 4 + c] = (if deep { x / 256.0 } else { x }).clamp(0.0, 255.0) as u8;
                            }
                            rgba[at * 4 + 3] = 255;
                        } else {
                            heights[at] = bands[0][from];
                        }
                    }
                }
                say(&format!("File {} of {}: read in {}", k + 1, total, secs(t)));
            }
            let empty = (0..n).filter(|at| empty_at(&heights, &rgba, *at)).count();
            let gt = [r[0], cell, 0.0, r[3], 0.0, -cell];
            ctx.notes.info(format!("{} × {} cells of {} m from {} files of {}", w, h, fmt_real(cell), used.iter().filter(|u| **u).count(), chosen.0));
            if empty > 0 {
                ctx.notes.warn(format!("{} cells of the area are not covered by any file and have no value.", fmt_count(empty as u64)));
            }
            if photo {
                RasterImage { width: w, height: h, rgba, geotransform: gt, vector: None }.into_table(Some(crs))
            } else {
                let values = heights;
                let (lo, hi) = values.iter().filter(|v| !v.is_nan()).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
                if lo.is_finite() {
                    say(&format!("Heights from {} to {} m", fmt_real((lo as f64 * 10.0).round() / 10.0), fmt_real((hi as f64 * 10.0).round() / 10.0)));
                }
                Grid { width: w, height: h, values, geotransform: gt, crs: Some(crs), bands: Vec::new(), meta: GridMeta::default() }.into_table()
            }
        }
        Kind::Points => {
            let classes = match p.get("las_classes").filter(|c| !c.trim().is_empty()) {
                None => None,
                Some(text) => Some(
                    text.split([',', ' ', ';'])
                        .filter(|c| !c.is_empty())
                        .map(|c| c.parse::<u8>().map_err(|_| GbError(format!("\"{}\" is not the number of a class. Write them as 2, 6.", c))))
                        .collect::<Result<Vec<u8>>>()?,
                ),
            };
            let every = match number_param(p, "las_every", "Every n-th point")? {
                None => 1,
                Some(n) if n >= 1.0 && n.fract() == 0.0 => n as usize,
                Some(_) => return err("Every n-th point must be a whole number of 1 or more."),
            };
            let spacing = number_param(p, "las_spacing", "The spacing")?.filter(|s| *s > 0.0);
            let most = number_param(p, "max_points", "Most points")?.filter(|m| *m >= 1.0).unwrap_or(50_000_000.0) as usize;
            // How many points there will be, from the headers of the files, before any is
            // fetched: the number in each, by the share of it in the area.
            let mut about = 0f64;
            let mut known = true;
            for (it, keep) in items.iter().zip(&kept) {
                let header = match keep {
                    Some(file) => std::fs::File::open(file).ok().and_then(|mut f| {
                        use std::io::Read;
                        let mut buf = vec![0u8; 375];
                        f.read_exact(&mut buf).ok().map(|_| buf)
                    }),
                    None => {
                        let (u, headers) = client.prepared(&it.href)?;
                        b.fetch_range(&u, &headers, 0, 375).ok().map(|(bytes, _)| bytes)
                    }
                };
                match header.as_deref().and_then(las_header) {
                    Some((count, bounds)) => {
                        let inside = [bounds[0].max(rect[0]), bounds[1].max(rect[1]), bounds[2].min(rect[2]), bounds[3].min(rect[3])];
                        let share = |r: [f64; 4]| (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0);
                        let all = share(bounds).max(1e-9);
                        let mut n = count as f64 * (share(inside) / all).min(1.0) / every as f64;
                        if let Some(s) = spacing {
                            n = n.min(share(inside) / (s * s));
                        }
                        about += n;
                    }
                    None => known = false,
                }
            }
            if known {
                say(&format!(
                    "About {} points in the area{}",
                    fmt_count(about.round() as u64),
                    if classes.is_some() { ", of all classes; fewer of those chosen" } else { "" }
                ));
                if about > most as f64 && classes.is_none() {
                    return err(format!(
                        "There are about {} points in the area, more than the {} of Most points. Thin them with Every n-th point or Spacing between points, choose a smaller area, or raise Most points if the computer has the memory.",
                        fmt_count(about.round() as u64),
                        fmt_count(most as u64)
                    ));
                }
            }
            let mut all: Option<crate::cloud::Cloud> = None;
            for (k, it) in items.iter().enumerate() {
                if !gate2.going() {
                    return err(crate::model::STOPPED);
                }
                progress("files", k as u64, Some(total as u64));
                let have = all.as_ref().map_or(0, |c| c.len());
                // One more than room is left for, to know when there are more.
                let opts =
                    crate::pointcloud::LasOpts { classes: classes.clone(), every, most: Some(most - have + 1), within: Some(rect), crs: Some(crs), spacing };
                let name = name_of(&it.href);
                let t = std::time::Instant::now();
                let mut notes = Notes::default();
                let table = if let Some(file) = &kept[k] {
                    say(&format!("File {} of {}: reading the part in the area of {}", k + 1, total, name));
                    crate::pointcloud::read_las(file, &opts, &mut notes, &|_, _| gate2.going())
                } else {
                    let (u, headers) = client.prepared(&it.href)?;
                    if name.to_lowercase().ends_with(".copc.laz") {
                        say(&format!("File {} of {}: reading the part in the area of {}", k + 1, total, name));
                        crate::pointcloud::read_copc_url(b, &u, &headers, &opts, &mut notes)
                    } else {
                        say(&format!("File {} of {}: fetching {}", k + 1, total, name));
                        let file = b.fetch_with(&u, &headers, true, &mut notes).map_err(|e| refused(e, &client, auth.is_some()))?;
                        crate::pointcloud::read_las(&file, &opts, &mut notes, &|_, _| gate2.going())
                    }
                }
                .map_err(|e| refused(e, &client, auth.is_some()))?;
                used[k] = true;
                let (cloud, _) = crate::cloud::cloud_of(&table);
                say(&format!("File {} of {}: {} points in the area, in {}", k + 1, total, fmt_count(cloud.len() as u64), secs(t)));
                if have + cloud.len() > most {
                    return err(format!(
                        "There are more than the {} points of Most points in the area. Thin them with Every n-th point or Spacing between points, choose a smaller area, or raise Most points if the computer has the memory.",
                        fmt_count(most as u64)
                    ));
                }
                match all.as_mut() {
                    None => all = Some(cloud.into_owned()),
                    Some(a) => a.append(&cloud),
                }
            }
            let cloud = all.unwrap_or_default();
            ctx.notes.info(format!("{} points from {} files of {}", fmt_count(cloud.len() as u64), total, chosen.0));
            cloud.into_table(Some(crs))
        }
        Kind::Items => {
            // The items themselves, with what the catalogue says of each: a value of its own
            // as an attribute, a list or an object as JSON.
            let mut names: Vec<String> = Vec::new();
            for it in &items {
                for k in it.properties.keys() {
                    if !names.contains(k) {
                        names.push(k.clone());
                    }
                }
            }
            let mut kinds: Vec<FieldType> = vec![FieldType::Int; names.len()];
            for it in &items {
                for (i, k) in names.iter().enumerate() {
                    match it.properties.get(k) {
                        None | Some(serde_json::Value::Null) => {}
                        Some(serde_json::Value::Number(n)) if n.is_i64() => {}
                        Some(serde_json::Value::Number(_)) if kinds[i] != FieldType::Text => kinds[i] = FieldType::Real,
                        _ => kinds[i] = FieldType::Text,
                    }
                }
            }
            let mut fields = vec![Field::new("id", FieldType::Text)];
            fields.extend(names.iter().zip(&kinds).map(|(n, k)| Field::new(n, *k)));
            fields.push(Field::new("files", FieldType::Text));
            let mut t = Table { fields, crs: Some(4326), ..Table::default() };
            for (k, it) in items.iter().enumerate() {
                let mut attrs = vec![Value::Text(it.id.clone())];
                for (i, n) in names.iter().enumerate() {
                    attrs.push(match it.properties.get(n) {
                        None | Some(serde_json::Value::Null) => Value::Null,
                        Some(serde_json::Value::String(x)) => Value::Text(x.clone()),
                        Some(serde_json::Value::Number(x)) if kinds[i] == FieldType::Int => x.as_i64().map_or(Value::Null, Value::Int),
                        Some(serde_json::Value::Number(x)) if kinds[i] == FieldType::Real => x.as_f64().map_or(Value::Null, Value::Real),
                        Some(other) => Value::Text(other.to_string()),
                    });
                }
                attrs.push(if it.assets.is_empty() { Value::Null } else { Value::Text(it.assets.join(" ")) });
                t.features.push(Feature { z: None, attrs, geom: it.geometry.clone() });
                used[k] = true;
            }
            let t = reproject(&t, crs, Some(b))?;
            ctx.notes.info(format!("{} {} of {}", fmt_count(t.len() as u64), many, chosen.1));
            t
        }
        Kind::Features => {
            let wanted: Vec<String> =
                p.get("layers").map(|l| l.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
            let mut tables = Vec::new();
            let mut seen_layers: Vec<String> = Vec::new();
            for (k, it) in items.iter().enumerate() {
                if !gate2.going() {
                    return err(crate::model::STOPPED);
                }
                progress("files", k as u64, Some(total as u64));
                let name = it.href.split(['?', '#']).next().unwrap_or(&it.href).rsplit('/').next().unwrap_or("").to_string();
                let t = std::time::Instant::now();
                let mut notes = Notes::default();
                let file = match &kept[k] {
                    Some(file) => file.clone(),
                    None => {
                        say(&format!("File {} of {}: fetching {}", k + 1, total, name));
                        let (u, headers) = client.prepared(&it.href)?;
                        b.fetch_with(&u, &headers, true, &mut notes).map_err(|e| refused(e, &client, auth.is_some()))?
                    }
                };
                let layers: Vec<String> = b.layers(&file)?.into_iter().map(|l| l.name).collect();
                for l in &layers {
                    if !seen_layers.contains(l) {
                        seen_layers.push(l.clone());
                    }
                }
                let read: Vec<&String> = if wanted.is_empty() {
                    layers.iter().collect()
                } else {
                    layers.iter().filter(|l| wanted.iter().any(|w| w.eq_ignore_ascii_case(l))).collect()
                };
                let mut count = 0usize;
                for layer in read {
                    let opts = ReadOpts { layer: Some(layer.clone()), within: Some((rect, Some(crs))), ..Default::default() };
                    let mut t2 = b.read_with(&file, &opts, &mut notes, &|_, _| gate2.going())?;
                    if t2.crs.is_some_and(|c| c != crs) {
                        t2 = reproject(&t2, crs, Some(b))?;
                    }
                    t2.fields.insert(0, Field::new("layer", FieldType::Text));
                    for f in t2.features.iter_mut() {
                        f.attrs.insert(0, Value::Text(layer.clone()));
                    }
                    count += t2.len();
                    tables.push(t2);
                }
                used[k] = true;
                say(&format!("File {} of {}: {} features in the area, in {}", k + 1, total, fmt_count(count as u64), secs(t)));
            }
            if let Some(missing) = wanted.iter().find(|w| !seen_layers.iter().any(|l| l.eq_ignore_ascii_case(w))) {
                ctx.notes.warn(format!("There is no layer {}. The files have: {}.", missing, seen_layers.join(", ")));
            }
            let mut t = crate::overture::merge(tables);
            t.crs = Some(crs);
            ctx.notes.info(format!("{} features from {} files of {}; the layers: {}", fmt_count(t.len() as u64), total, chosen.0, seen_layers.join(", ")));
            t
        }
    };
    let license = if chosen.2.is_empty() { String::new() } else { format!(", licence {}", chosen.2) };
    ctx.notes.info(format!("© {}{}", PROVIDERS.iter().find(|x| x.0 == provider).map_or(provider.as_str(), |x| x.1.split(',').next().unwrap_or(x.1)), license));

    // The files, with where they lie.
    let mut tiles = Table {
        fields: vec![
            Field::new("id", FieldType::Text),
            Field::new("collection", FieldType::Text),
            Field::new("datetime", FieldType::Text),
            Field::new("used", FieldType::Int),
            Field::new("href", FieldType::Text),
            Field::new("file", FieldType::Text),
        ],
        crs: Some(4326),
        ..Table::default()
    };
    for ((it, u), keep) in items.iter().zip(&used).zip(&kept) {
        let geom = it.geometry.clone().or_else(|| {
            let [w, s, e, n] = it.bbox;
            Some(geo::Geometry::Polygon(geo::Polygon::new(geo::LineString::from(vec![(w, s), (e, s), (e, n), (w, n), (w, s)]), vec![])))
        });
        tiles.features.push(Feature {
            z: None,
            attrs: vec![
                Value::Text(it.id.clone()),
                Value::Text(it.collection.clone()),
                Value::Text(it.when.clone()),
                Value::Int(*u as i64),
                Value::Text(it.href.clone()),
                keep.as_ref().map_or(Value::Null, |f| Value::Text(f.to_string_lossy().into_owned())),
            ],
            geom,
        });
    }
    // For items such as plans, the files are their documents: one row for each, however many
    // items refer to it, with what the catalogue says of it and where it lies when kept.
    if product.kind == Kind::Items {
        tiles = Table {
            fields: vec![
                Field::new("title", FieldType::Text),
                Field::new("type", FieldType::Text),
                Field::new("roles", FieldType::Text),
                Field::new("asset", FieldType::Text),
                Field::new("items", FieldType::Int),
                Field::new("first_item", FieldType::Text),
                Field::new("collection", FieldType::Text),
                Field::new("href", FieldType::Text),
                Field::new("file", FieldType::Text),
            ],
            crs: Some(4326),
            ..Table::default()
        };
        let mut row_of: HashMap<String, usize> = HashMap::new();
        let mut boxes: Vec<[f64; 4]> = Vec::new();
        for it in &items {
            for d in &it.documents {
                match row_of.get(&d.href) {
                    Some(&r) => {
                        if let Value::Int(n) = &mut tiles.features[r].attrs[4] {
                            *n += 1;
                        }
                        let b = &mut boxes[r];
                        *b = [b[0].min(it.bbox[0]), b[1].min(it.bbox[1]), b[2].max(it.bbox[2]), b[3].max(it.bbox[3])];
                    }
                    None => {
                        row_of.insert(d.href.clone(), tiles.features.len());
                        boxes.push(it.bbox);
                        let text = |s: &str| if s.is_empty() { Value::Null } else { Value::Text(s.to_string()) };
                        tiles.features.push(Feature {
                            z: None,
                            attrs: vec![
                                text(&d.title),
                                text(&d.kind),
                                text(&d.roles),
                                Value::Text(d.key.clone()),
                                Value::Int(1),
                                Value::Text(it.id.clone()),
                                Value::Text(it.collection.clone()),
                                Value::Text(d.href.clone()),
                                kept_files.get(&d.href).map_or(Value::Null, |f| Value::Text(f.to_string_lossy().into_owned())),
                            ],
                            geom: None,
                        });
                    }
                }
            }
        }
        // Each document where the items that refer to it lie, as a rectangle around them.
        for (f, [w, s, e, n]) in tiles.features.iter_mut().zip(boxes) {
            f.geom = Some(geo::Geometry::Polygon(geo::Polygon::new(geo::LineString::from(vec![(w, s), (e, s), (e, n), (w, n), (w, s)]), vec![])));
        }
        ctx.notes.info(format!("{} documents of {} items", fmt_count(tiles.len() as u64), fmt_count(items.len() as u64)));
    }
    let tiles = reproject(&tiles, crs, Some(b))?;
    Ok(vec![("Output".into(), output), ("Files".into(), tiles)])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "GeodataFetcher",
    title: "Fetch geodata",
    hidden: false,
    group: Group::Reader,
    category: "Web",
    desc: "Fetches the geodata of a national mapping agency for an area and puts it together: heights of the ground as a grid, a photo from above as a picture, a point cloud, features such as addresses and map features, maps as pictures, or the plans of the national geodata platform with their documents. One tool for all the products, chosen from a list; from Lantmäteriet first. The files are found in the provider's catalogue, read only where they reach into the area, and the photos and heights only the part that is needed. The files need a login: an API connection with the provider's key, and the products ordered from the provider. Files gives where each file lies, and where it is kept.",
    inputs: &["Extent"],
    outputs: &["Output", "Files"],
    ports: None,
    params: PARAMS,
    run: run_geodata,
    summary: |p| PRODUCT_CHOICES.iter().find(|c| Some(c.0.to_string()) == p.get("product")).map(|c| c.1.split(':').next().unwrap_or(c.1).to_string()).unwrap_or_default(),
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collections_are_found_by_their_words() {
        assert!(matches("ortofoto -ir", "ortofoto-rgb", "Ortofoto färg"));
        assert!(!matches("ortofoto -ir", "ortofoto-ir", "Ortofoto IR"));
        assert!(matches("orto -ir -historiska", "orto-h-2010", "Ortofoto H (2010)"));
        assert!(!matches("orto -ir -historiska", "orto-historiska-2005-2009", "Ortofoto historiska RT90 (2005-2009)"));
        let many: Vec<String> = (0..100).map(|i| format!("orto-{}", i)).collect();
        assert!(shortened(&many).ends_with("and 70 more"));
        assert!(matches("topografi 10", "topografi-10", "Topografi 10"));
        assert!(!matches("topografi 10", "topografi-100", "Topografi 100"));
        // The collections of Lantmäteriet's stac-hojd: the heights of the ground, not of the
        // surface, and the laser data of the forest.
        let hojd = [("dsm-cog", "Ythöjdmodell"), ("dsm-skoglig-copc", "Laserdata Skog"), ("dtm-cog", "Markhöjdmodell")];
        let pick = |key: &str| {
            let p = PRODUCTS.iter().find(|p| p.key == key).unwrap();
            p.finds.iter().find_map(|w| hojd.iter().find(|c| matches(w, c.0, c.1))).map(|c| c.0)
        };
        assert_eq!(pick("lm_heights"), Some("dtm-cog"));
        assert_eq!(pick("lm_laser_forest"), Some("dsm-skoglig-copc"));
        assert_eq!(years("2023").unwrap(), "2023-01-01T00:00:00Z/2023-12-31T23:59:59Z");
        assert_eq!(years("2021/2024").unwrap(), "2021-01-01T00:00:00Z/2024-12-31T23:59:59Z");
        assert!(years("last").is_err());
        // Every product is in the list to choose from.
        assert_eq!(PRODUCTS.len(), PRODUCT_CHOICES.len());
        assert!(PRODUCTS.iter().all(|p| PRODUCT_CHOICES.iter().any(|c| c.0 == p.key && c.1 == p.label)));
        // In the order of the alphabet, and a series by its scale: 10, 50, 250, 1M.
        let order = |l: &str| {
            let l = l.to_lowercase();
            let name = l.split(':').next().unwrap_or("").to_string();
            let (word, n) = name.rsplit_once(' ').map_or((name.clone(), 0.0), |(a, b)| match b.strip_suffix('m').unwrap_or(b).parse::<f64>() {
                Ok(v) => (a.to_string(), if b.ends_with('m') { v * 1000.0 } else { v }),
                Err(_) => (name.clone(), 0.0),
            });
            (word, (n * 10.0) as i64)
        };
        let keys: Vec<_> = PRODUCT_CHOICES.iter().map(|c| order(c.1)).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        // The national military maps, each its own scale.
        let karta = [("nmk250", ""), ("nmk50", "NMK50")];
        for (key, id) in [("lm_nmk50", "nmk50"), ("lm_nmk250", "nmk250")] {
            let p = PRODUCTS.iter().find(|p| p.key == key).unwrap();
            assert_eq!(p.finds.iter().find_map(|w| karta.iter().find(|c| matches(w, c.0, c.1))).map(|c| c.0), Some(id), "{}", key);
        }
        // The scales of Topografi, each its own.
        let topo = [
            ("topografi-10", "Topografi 10"),
            ("topografi-100", "Topografi 100"),
            ("topografi-50", "Topografi 50"),
            ("topografi-250", "Topografi 250"),
            ("topografi-1m", "Topografi 1M"),
        ];
        for (key, id) in [("lm_topo10", "topografi-10"), ("lm_topo50", "topografi-50"), ("lm_topo250", "topografi-250"), ("lm_topo1m", "topografi-1m")] {
            let p = PRODUCTS.iter().find(|p| p.key == key).unwrap();
            assert_eq!(p.finds.iter().find_map(|w| topo.iter().find(|c| matches(w, c.0, c.1))).map(|c| c.0), Some(id), "{}", key);
        }
        // The divisions of the country, not the districts or the parishes.
        let vektor = [("distriktsindelning", "Distriktsindelning"), ("kommun-lan-rike", "Kommun, län och rike"), ("socken-stad", "Socken och stad")];
        let p = PRODUCTS.iter().find(|p| p.key == "lm_divisions").unwrap();
        assert_eq!(p.finds.iter().find_map(|w| vektor.iter().find(|c| matches(w, c.0, c.1))).map(|c| c.0), Some("kommun-lan-rike"));
    }
}
