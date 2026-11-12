//! Make Gaussian splats: a point cloud from a scanner or a drone made a scene of splats, to
//! show as a continuous surface and to write as PLY or SPZ for viewers of splats.

use super::*;
use crate::model::{err, fmt_count, Result};

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "neighbours",
        label: "Neighbours",
        kind: ParamKind::Text,
        default: "8",
        hint: "How many of the nearest points give each splat its size and shape",
    },
    ParamSpec { key: "size", label: "Size", kind: ParamKind::Text, default: "1", hint: "Larger than 1 to close gaps between the points, smaller for sharper" },
    ParamSpec { key: "opacity", label: "Opacity", kind: ParamKind::Text, default: "0.9", hint: "From 0, clear, to 1, solid" },
    ParamSpec {
        key: "shape",
        label: "Shape",
        kind: ParamKind::Choice(&[("flat", "Flat along the surface, as a coin"), ("round", "Round, as a ball")]),
        default: "flat",
        hint: "Flat splats follow walls and ground, and look like a surface",
    },
];

fn run_make(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    if t.is_empty() && t.points.as_ref().is_none_or(|c| c.is_empty()) {
        return err("Nothing came in on Points. Connect a point cloud, such as one read from LAS, LAZ or PLY.");
    }
    if ctx.geographic(t.crs) == Some(true) {
        return err("The points are in a coordinate system in degrees. Reproject them first, such as to EPSG:3006, so that sizes are in metres.");
    }
    let whole = |key: &str, d: f64, what: &str| -> Result<f64> { Ok(number_param(p, key, what)?.unwrap_or(d)) };
    let making = crate::splats::Making {
        neighbours: whole("neighbours", 8.0, "Neighbours")?.max(3.0) as usize,
        size: whole("size", 1.0, "Size")?.max(0.01) as f32,
        opacity: whole("opacity", 0.9, "Opacity")?.clamp(0.01, 1.0) as f32,
        flat: p.get("shape").as_deref() != Some("round"),
    };
    let (cloud, _) = crate::cloud::cloud_of(t);
    let gate = ctx.gate;
    let going = || gate.going();
    let mut made = crate::splats::make_splats(&cloud, &making, &going)?;
    crate::splats::describe(&mut made);
    ctx.notes.info(format!(
        "{} points made Gaussian splats{}",
        fmt_count(made.len() as u64),
        if cloud.rgb.is_none() { ", coloured by their height as they have no colours" } else { "" }
    ));
    Ok(vec![("Splats".into(), made.into_table(t.crs))])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "SplatMaker",
    title: "Make Gaussian splats",
    hidden: false,
    group: Group::Transformer,
    category: "Point clouds",
    desc: "Makes a point cloud, such as from a laser scanner or photos from a drone, a scene of Gaussian splats: each point the middle of a small, soft ellipse in its colour, as large as the spacing to its neighbours and, when flat, lying along the surface they make, so that walls, roofs and ground look whole. Write it as PLY or SPZ for viewers of splats, or see it in 3D with map data.",
    inputs: &["Points"],
    outputs: &["Splats"],
    ports: None,
    params: PARAMS,
    run: run_make,
    summary: |p| if p.get("shape").as_deref() == Some("round") { "round".into() } else { "flat along surfaces".into() },
}];
