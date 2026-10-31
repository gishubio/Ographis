//! Overture Maps download: buildings, places, roads and more from the open map data of the
//! Overture Maps Foundation, only within a rectangle, read from its files on the web.

use super::geom_transform::number_param;
use super::osm::{limit_bbox, rect_in_degrees, within_area, CUTS};
use super::*;
use crate::model::{err, fmt_count};

const OVERTURE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "kind",
        label: "What",
        kind: ParamKind::Picks(crate::overture::KINDS),
        default: "buildings/building",
        hint: "Tick one or more. Each feature gets the attribute kind, such as buildings/building",
    },
    ParamSpec {
        key: "bbox",
        label: "Rectangle",
        kind: ParamKind::Text,
        default: "",
        hint: "West, south, east, north in degrees, such as 11.95, 57.69, 11.99, 57.71. Not needed when something comes in on Extent",
    },
    ParamSpec {
        key: "max_km2",
        label: "Largest area in km²",
        kind: ParamKind::Text,
        default: "25",
        hint: "A larger rectangle is made smaller around its middle to this size. The files are large, and each part of one that reaches into the rectangle is fetched",
    },
    ParamSpec {
        key: "cut",
        label: "Large features",
        kind: ParamKind::Choice(CUTS),
        default: "cut",
        hint: "Land cover, land use, water and borders can be very large: cut, only the part within the area comes along",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "EPSG:3006", hint: "Of what comes out. Overture Maps is in WGS 84" },
    ParamSpec { key: "release", label: "Release", kind: ParamKind::Text, default: "", hint: "Such as 2026-10-21.0. Empty takes the latest; a release is kept for a few months" },
    ParamSpec { key: "catalog", label: "Catalog", kind: ParamKind::Text, default: crate::overture::CATALOG, hint: "Where the catalog of the releases is" },
];

/// Where the extents of the files of a release are kept between runs.
fn cache_dir() -> Option<std::path::PathBuf> {
    crate::settings::settings_dir().map(|d| d.join("overture"))
}

fn run_overture(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let extent = input(inp, "Extent");
    let b = ctx.backend("Overture Maps download")?;
    let bbox = rect_in_degrees(p, extent, b)?;
    let max_km2 = number_param(p, "max_km2", "The largest area")?.unwrap_or(25.0).max(0.01);
    let (bbox, smaller) = limit_bbox(bbox, max_km2);
    if smaller {
        ctx.notes.warn(format!(
            "The rectangle was larger than {} km² and was made smaller around its middle: {:.5}, {:.5}, {:.5}, {:.5}.",
            crate::model::fmt_real(max_km2),
            bbox[0],
            bbox[1],
            bbox[2],
            bbox[3]
        ));
    }
    let mut kinds = p.list("kind");
    if kinds.is_empty() {
        kinds.push("buildings/building".into());
    }
    if let Some(bad) = kinds.iter().find(|k| !crate::overture::KINDS.iter().any(|o| o.0 == k.as_str())) {
        return err(format!(
            "\"{}\" is not a kind of Overture Maps. Choose among {}.",
            bad,
            crate::overture::KINDS.iter().map(|o| o.0).collect::<Vec<_>>().join(", ")
        ));
    }
    let base = p.get("catalog").filter(|c| !c.is_empty()).unwrap_or_else(|| crate::overture::CATALOG.into());
    let base = base.trim_end_matches('/').to_string();
    let release = match p.get("release").filter(|r| !r.is_empty()) {
        Some(r) => r,
        None => crate::overture::latest_release(b, &base)?,
    };
    let gate = ctx.gate;
    let mut all = Vec::new();
    for kind in &kinds {
        ctx.notes.info(format!("Overture Maps {}, {}", release, kind));
        let files = crate::overture::files(b, &base, &release, kind, cache_dir().as_deref(), &mut ctx.notes)?;
        let reach: Vec<&([f64; 4], String)> =
            files.iter().filter(|(fb, _)| fb[0] <= bbox[2] && fb[2] >= bbox[0] && fb[1] <= bbox[3] && fb[3] >= bbox[1]).collect();
        let (mut groups, mut bytes) = ((0usize, 0usize), 0u64);
        let mut tables = Vec::new();
        for (k, (_, url)) in reach.iter().enumerate() {
            (ctx.progress)("files", k as u64, Some(reach.len() as u64));
            let r = crate::overture::read_within(b, url, bbox, &|| gate.going())?;
            groups = (groups.0 + r.groups.0, groups.1 + r.groups.1);
            bytes += r.bytes;
            tables.push(r.table);
        }
        let mut t = crate::overture::merge(tables);
        t.fields.insert(0, Field::new("kind", FieldType::Text));
        for f in t.features.iter_mut() {
            f.attrs.insert(0, Value::Text(kind.clone()));
        }
        ctx.notes.info(format!(
            "{}: {} features from {} of {} parts of {} files, {} fetched",
            kind,
            fmt_count(t.len() as u64),
            groups.1,
            groups.0,
            reach.len(),
            crate::overture::size_text(bytes)
        ));
        let license = match kind.split('/').next().unwrap_or("") {
            "base" | "transportation" | "buildings" => "ODbL, from OpenStreetMap and others",
            "places" => "CDLA Permissive 2.0, and Apache 2.0",
            "addresses" => "that of each source, as the attribute sources says",
            _ => "ODbL",
        };
        ctx.notes.info(format!("{} is © Overture Maps Foundation and its sources; the licence is {}", kind, license));
        all.push(t);
    }
    if all.iter().all(|t| t.is_empty()) && all.len() == kinds.len() {
        ctx.notes.warn("Nothing of what was ticked lies within the rectangle.");
    }
    let mut table = crate::overture::merge(all);
    within_area(p, &mut table, extent, bbox, b, &mut ctx.notes)?;
    if let Some(to) = p.crs("crs")?.filter(|c| *c != 4326) {
        if !table.is_empty() {
            table = reproject(&table, to, Some(b))?;
        } else {
            table.crs = Some(to);
        }
    }
    Ok(vec![("Output".into(), table)])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "OvertureDownloader",
    title: "Overture Maps download",
    hidden: false,
    group: Group::Transformer,
    category: "Web",
    desc: "Fetches open map data from Overture Maps: buildings with their heights, places such as shops and schools with their categories, addresses, roads and paths, land use, water, and the borders of countries and municipalities. Only what reaches into a rectangle, or the extent of what comes in on Extent, is fetched, by reading only the parts of the large files on the web that hold it. Needs the internet.",
    inputs: &["Extent"],
    outputs: &["Output"],
    ports: None,
    params: OVERTURE_PARAMS,
    run: run_overture,
    summary: |p| {
        let kinds = p.list("kind");
        let kinds = if kinds.is_empty() { vec!["buildings/building".to_string()] } else { kinds };
        kinds.iter().map(|k| choice_label(crate::overture::KINDS, k).split(':').next().unwrap_or("").to_string()).collect::<Vec<_>>().join(", ")
    },
}];
