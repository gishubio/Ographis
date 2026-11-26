//! Make a cartogram: areas drawn by the size of a value rather than of the land, as circles
//! that push each other apart from where the areas are (Dorling), or as squares of a size,
//! one for each area, laid on a grid as near their places as they can be (a tile map).

use super::*;
use crate::model::{err, fmt_count, Feature, Field, FieldType, Result, Value};
use geo::{Area, BoundingRect, Centroid, Geometry};

const KINDS: &[(&str, &str)] = &[("circles", "Circles by the value, pushed apart (Dorling)"), ("squares", "A square for each area, on a grid (a tile map)")];

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "kind",
        label: "Kind",
        kind: ParamKind::Choice(KINDS),
        default: "circles",
        hint: "Circles show the value by their size; squares give each area as much room, so that small areas are seen",
    },
    ParamSpec {
        key: "value",
        label: "Value",
        kind: ParamKind::Attribute("Areas"),
        default: "",
        hint: "The number each circle is as large as, such as the people living there. Not needed for squares",
    },
    ParamSpec {
        key: "share",
        label: "How much of the land",
        kind: ParamKind::Text,
        default: "50",
        hint: "In percent: the circles together cover this much of the area of the land, or the squares are this large",
    },
    ParamSpec {
        key: "rounds",
        label: "Rounds",
        kind: ParamKind::Text,
        default: "300",
        hint: "How many times the circles are pushed apart. More: fewer overlaps, further from their places",
    },
];

/// The area, the middle and the value of each area.
struct Place {
    area: f64,
    at: (f64, f64),
    value: f64,
}

fn run_cartogram(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Areas");
    let kind = p.get("kind").unwrap_or_else(|| "circles".into());
    let share = (number_param(p, "share", "How much of the land")?.unwrap_or(50.0) / 100.0).clamp(0.02, 1.0);
    let value_at = match p.get("value").filter(|v| !v.trim().is_empty()) {
        Some(v) => Some(t.field_index(&v).ok_or_else(|| crate::model::GbError(format!("The areas have no attribute {}.", v)))?),
        None if kind == "circles" => return err("Choose the attribute whose value sets the size of the circles."),
        None => None,
    };
    let mut places = Vec::new();
    let mut kept = Vec::new();
    for (i, f) in t.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        if !matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) {
            continue;
        }
        let Some(c) = g.centroid() else { continue };
        let value = value_at.and_then(|k| f.attrs[k].as_f64()).unwrap_or(1.0).max(0.0);
        places.push(Place { area: g.unsigned_area(), at: (c.x(), c.y()), value });
        kept.push(i);
    }
    if places.is_empty() {
        return err("No areas came in on Areas.");
    }
    let land: f64 = places.iter().map(|p| p.area).sum();
    let mut out = t.like();
    let extra = if kind == "circles" { "radius" } else { "column" };
    out.fields.push(Field::new(extra, FieldType::Real));
    if kind == "squares" {
        out.fields.push(Field::new("row", FieldType::Real));
    }
    match kind.as_str() {
        "squares" => {
            let side = (land * share / places.len() as f64).sqrt();
            // The grid as dense as the areas lie: one cell for each, over the land they cover.
            let b = t
                .features
                .iter()
                .filter_map(|f| f.geom.as_ref()?.bounding_rect())
                .reduce(|a, b| geo::Rect::new((a.min().x.min(b.min().x), a.min().y.min(b.min().y)), (a.max().x.max(b.max().x), a.max().y.max(b.max().y))));
            let Some(b) = b else { return err("The areas have no extent.") };
            let spacing = (b.width() * b.height() / places.len() as f64).sqrt().max(1e-9);
            let origin = (b.min().x, b.min().y);
            let cells = squares(&places, spacing, origin);
            let (mx, my) = (b.center().x, b.center().y);
            let (cx, cy) = (b.width() / spacing / 2.0, b.height() / spacing / 2.0);
            for (i, (c, r)) in kept.iter().zip(cells) {
                // Laid out around the middle of the areas, each cell as large as a square.
                let (x, y) = (mx + (c as f64 + 0.5 - cx) * side - side / 2.0, my + (r as f64 + 0.5 - cy) * side - side / 2.0);
                let gap = side * 0.04;
                let poly = geo::Rect::new((x + gap, y + gap), (x + side - gap, y + side - gap)).to_polygon();
                let mut attrs = t.features[*i].attrs.clone();
                attrs.push(Value::Real(c as f64));
                attrs.push(Value::Real(r as f64));
                out.features.push(Feature { geom: Some(Geometry::Polygon(poly)), attrs, z: None });
            }
            ctx.notes.info(format!("{} squares of {:.0} m", fmt_count(out.len() as u64), side));
        }
        _ => {
            let total: f64 = places.iter().map(|p| p.value).sum();
            if total <= 0.0 {
                return err("The values are all 0, so the circles would have no size.");
            }
            // Together as large as the share of the land.
            let k = (land * share / (std::f64::consts::PI * total)).sqrt();
            let radii: Vec<f64> = places.iter().map(|p| k * p.value.sqrt()).collect();
            let rounds = number_param(p, "rounds", "Rounds")?.unwrap_or(300.0).clamp(1.0, 5000.0) as usize;
            let at = dorling(&places, &radii, rounds);
            for ((i, (x, y)), r) in kept.iter().zip(at).zip(&radii) {
                let ring: Vec<(f64, f64)> = (0..=64)
                    .map(|j| {
                        let a = j as f64 / 64.0 * std::f64::consts::TAU;
                        (x + r * a.cos(), y + r * a.sin())
                    })
                    .collect();
                let mut attrs = t.features[*i].attrs.clone();
                attrs.push(Value::Real(*r));
                out.features.push(Feature { geom: Some(Geometry::Polygon(geo::Polygon::new(ring.into(), Vec::new()))), attrs, z: None });
            }
            let left = overlaps(&out);
            ctx.notes.info(format!("{} circles", fmt_count(out.len() as u64)));
            if left > 0 {
                ctx.notes.info(format!("{} pairs still overlap a little: more rounds, or a smaller share, part them", left));
            }
        }
    }
    Ok(vec![("Cartogram".into(), out)])
}

/// Circles pushed apart where they overlap and drawn back towards their places, a little at
/// each round (after Dorling, 1996).
fn dorling(places: &[Place], radii: &[f64], rounds: usize) -> Vec<(f64, f64)> {
    let mut at: Vec<(f64, f64)> = places.iter().map(|p| p.at).collect();
    let n = at.len();
    for round in 0..rounds {
        let mut moves = vec![(0.0f64, 0.0f64); n];
        for i in 0..n {
            for j in (i + 1)..n {
                let (dx, dy) = (at[j].0 - at[i].0, at[j].1 - at[i].1);
                let d = dx.hypot(dy).max(1e-9);
                let overlap = radii[i] + radii[j] - d;
                if overlap > 0.0 {
                    // The smaller circle moves more.
                    let (wi, wj) = (radii[j] / (radii[i] + radii[j]).max(1e-9), radii[i] / (radii[i] + radii[j]).max(1e-9));
                    let (ux, uy) = if d > 1e-6 { (dx / d, dy / d) } else { ((i as f64).cos(), (i as f64).sin()) };
                    moves[i].0 -= ux * overlap * wi * 0.5;
                    moves[i].1 -= uy * overlap * wi * 0.5;
                    moves[j].0 += ux * overlap * wj * 0.5;
                    moves[j].1 += uy * overlap * wj * 0.5;
                }
            }
        }
        // Drawn back to its place, less as the rounds go on.
        let pull = 0.06 * (1.0 - round as f64 / rounds as f64);
        for i in 0..n {
            at[i].0 += moves[i].0 + (places[i].at.0 - at[i].0) * pull;
            at[i].1 += moves[i].1 + (places[i].at.1 - at[i].1) * pull;
        }
    }
    at
}

/// The pairs of circles that still overlap by more than a hundredth of the smaller.
fn overlaps(t: &crate::model::Table) -> usize {
    let circles: Vec<(f64, f64, f64)> = t
        .features
        .iter()
        .filter_map(|f| {
            let g = f.geom.as_ref()?;
            let r = g.bounding_rect()?;
            Some((r.center().x, r.center().y, r.width() / 2.0))
        })
        .collect();
    let mut n = 0;
    for i in 0..circles.len() {
        for j in (i + 1)..circles.len() {
            let (a, b) = (circles[i], circles[j]);
            if (a.0 - b.0).hypot(a.1 - b.1) < a.2 + b.2 - a.2.min(b.2) * 0.01 {
                n += 1;
            }
        }
    }
    n
}

/// A cell of a grid for each place, as near it as can be: the places nearest the middle
/// first, each to the free cell nearest it.
fn squares(places: &[Place], side: f64, origin: (f64, f64)) -> Vec<(i64, i64)> {
    let n = places.len();
    let (mx, my) = (places.iter().map(|p| p.at.0).sum::<f64>() / n as f64, places.iter().map(|p| p.at.1).sum::<f64>() / n as f64);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        let da = (places[*a].at.0 - mx).hypot(places[*a].at.1 - my);
        let db = (places[*b].at.0 - mx).hypot(places[*b].at.1 - my);
        da.total_cmp(&db)
    });
    let mut taken = std::collections::HashSet::new();
    let mut out = vec![(0i64, 0i64); n];
    for i in order {
        let (fx, fy) = ((places[i].at.0 - origin.0) / side - 0.5, (places[i].at.1 - origin.1) / side - 0.5);
        let (cx, cy) = (fx.round() as i64, fy.round() as i64);
        let mut best = None;
        'search: for ring in 0i64..200 {
            let mut found: Vec<(f64, (i64, i64))> = Vec::new();
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let cell = (cx + dx, cy + dy);
                    if taken.contains(&cell) {
                        continue;
                    }
                    found.push(((cell.0 as f64 - fx).hypot(cell.1 as f64 - fy), cell));
                }
            }
            if let Some(f) = found.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
                best = Some(f.1);
                break 'search;
            }
        }
        let cell = best.unwrap_or((cx, cy));
        taken.insert(cell);
        out[i] = cell;
    }
    out
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "CartogramMaker",
    title: "Make a cartogram",
    hidden: false,
    group: Group::Transformer,
    category: "Geometry",
    desc: "Draws areas by a value rather than by the land: as circles as large as the value, such as the people living there, pushed apart from where the areas are so that they do not overlap (a Dorling cartogram); or as a square for each area on a grid, as near its place as it can be, so that small areas are seen as well as large ones (a tile map). The attributes go with them. Works in metres.",
    inputs: &["Areas"],
    outputs: &["Cartogram"],
    ports: None,
    params: PARAMS,
    run: run_cartogram,
    summary: |p| match p.get("kind").as_deref() {
        Some("squares") => "squares on a grid".into(),
        _ => format!("circles by {}", p.get("value").unwrap_or_default()),
    },
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circles_part_and_squares_take_a_cell_each() {
        let places: Vec<Place> = (0..6).map(|i| Place { area: 100.0, at: (i as f64 * 5.0, 0.0), value: 1.0 + i as f64 }).collect();
        let radii: Vec<f64> = places.iter().map(|p| p.value.sqrt() * 4.0).collect();
        let at = dorling(&places, &radii, 400);
        for i in 0..6 {
            for j in (i + 1)..6 {
                let d = (at[i].0 - at[j].0).hypot(at[i].1 - at[j].1);
                assert!(d > (radii[i] + radii[j]) * 0.95, "{} {} {}", i, j, d);
            }
        }
        let cells = squares(&places, 4.0, (0.0, 0.0));
        let unique: std::collections::HashSet<_> = cells.iter().collect();
        assert_eq!(unique.len(), 6);
    }
}
