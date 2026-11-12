//! Tools added in 0.95: the outline of 3D objects seen from above, as in a plan.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Field, FieldType, Value};
use geo::{Area, BooleanOps, Coord, LineString, MultiPolygon, Polygon};

// ---------- Outline from above (OutlineFromAbove) ----------

const HEIGHT_FROM: &[(&str, &str)] = &[
    ("storey", "The floor of its storey, as IFC gives it"),
    ("bottom", "The lowest point of the object"),
    ("absolute", "The heights of the coordinate system"),
];

const CUT_SHOWS: &[(&str, &str)] =
    &[("below", "All of the object below the cut, seen from above"), ("section", "Only what the cut goes through, as in a plan drawing")];

const OUTLINE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "cut",
        label: "Seen from the height",
        kind: ParamKind::Text,
        default: "",
        hint: "In metres, such as 1 to leave out what is above doors and windows. Empty sees all of the object, as its shadow from above",
    },
    ParamSpec {
        key: "height_from",
        label: "Height from",
        kind: ParamKind::Choice(HEIGHT_FROM),
        default: "storey",
        hint: "An object without a storey is measured from its lowest point",
    },
    ParamSpec {
        key: "shows",
        label: "Shows",
        kind: ParamKind::Choice(CUT_SHOWS),
        default: "below",
        hint: "Below a window, the wall under it is there; through the cut, the window leaves a gap",
    },
    ParamSpec {
        key: "min_area",
        label: "Smallest part",
        kind: ParamKind::Text,
        default: "",
        hint: "In square metres: parts of an outline smaller than this are left out, and objects with nothing left go to Other. Empty keeps all",
    },
    ParamSpec {
        key: "plane_3d",
        label: "Cut in 3D",
        kind: ParamKind::Choice(&[("true", "Shown as a see-through plane"), ("false", "Not shown")]),
        default: "true",
        hint: "In the 3D view of the program: the model that comes in, with the height of the cut as a plane",
    },
];

/// A triangle cut to what lies at or below a height: a convex polygon of up to four corners.
fn below(t: &[[f64; 3]; 3], z: f64) -> Vec<[f64; 3]> {
    let mut out = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, b) = (t[i], t[(i + 1) % 3]);
        if a[2] <= z {
            out.push(a);
        }
        if (a[2] <= z) != (b[2] <= z) {
            let f = (z - a[2]) / (b[2] - a[2]);
            out.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, z]);
        }
    }
    out
}

/// A polygon seen from above, or None when it stands on edge and covers nothing.
fn flat(poly: &[[f64; 3]]) -> Option<Polygon<f64>> {
    if poly.len() < 3 {
        return None;
    }
    let mut ring: Vec<Coord<f64>> = poly.iter().map(|p| Coord { x: p[0], y: p[1] }).collect();
    ring.push(ring[0]);
    // All one way round, so that a top and a bottom face over each other add up, not cancel.
    let p = geo::Orient::orient(&Polygon::new(LineString::new(ring), Vec::new()), geo::orient::Direction::Default);
    // Upright faces, such as the sides of walls, are lines from above.
    let size = poly.iter().map(|p| (p[0] - poly[0][0]).abs() + (p[1] - poly[0][1]).abs()).fold(0.0, f64::max);
    (p.unsigned_area() > 1e-9 * size.max(1.0) * size.max(1.0)).then_some(p)
}

/// Where a closed model is cut by a level plane: the rings the cut draws, made into areas.
fn section(tris: &[[[f64; 3]; 3]], z: f64) -> MultiPolygon<f64> {
    // The pieces of line where each triangle crosses the plane, end to end.
    let key = |p: [f64; 2]| ((p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64);
    let mut segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for t in tris {
        // A corner exactly on the plane is taken as a hair above, so that no edge counts twice.
        let above = |p: &[f64; 3]| p[2] > z || (p[2] - z).abs() < 1e-9;
        let mut pts = Vec::new();
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            if above(&a) != above(&b) {
                let f = (z - a[2]) / (b[2] - a[2]);
                pts.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
            }
        }
        if pts.len() == 2 && key(pts[0]) != key(pts[1]) {
            segs.push((pts[0], pts[1]));
        }
    }
    // Chains of pieces that meet, closed into rings.
    let mut from: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        from.entry(key(s.0)).or_default().push(i);
        from.entry(key(s.1)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut rings: Vec<Polygon<f64>> = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut ring = vec![segs[start].0, segs[start].1];
        loop {
            let end = key(*ring.last().unwrap_or(&[0.0, 0.0]));
            let next = from.get(&end).and_then(|c| c.iter().copied().find(|i| !used[*i]));
            let Some(i) = next else { break };
            used[i] = true;
            let s = segs[i];
            ring.push(if key(s.0) == end { s.1 } else { s.0 });
            if key(*ring.last().unwrap_or(&[0.0, 0.0])) == key(ring[0]) {
                break;
            }
        }
        if ring.len() < 4 {
            continue;
        }
        let coords: Vec<Coord<f64>> = ring.iter().map(|p| Coord { x: p[0], y: p[1] }).collect();
        let p = geo::Orient::orient(&Polygon::new(LineString::new(coords), Vec::new()), geo::orient::Direction::Default);
        if p.unsigned_area() > 0.0 {
            rings.push(p);
        }
    }
    // A ring inside an odd number of others is a hole; the others are solid, and solids that
    // touch, such as a door and its handle, are one area.
    use geo::Contains;
    let depth = |k: usize| rings.iter().enumerate().filter(|(j, r)| *j != k && r.contains(&rings[k].exterior().0[0])).count();
    let (mut solid, mut holes) = (Vec::new(), Vec::new());
    for (k, r) in rings.iter().enumerate() {
        if depth(k) % 2 == 0 {
            solid.push(r.clone());
        } else {
            holes.push(r.clone());
        }
    }
    geo::unary_union(solid.iter()).difference(&geo::unary_union(holes.iter()))
}

fn run_outline(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let Some(mesh) = t.mesh.as_deref() else {
        return err("Connect 3D objects, such as walls read from IFC, buildings from CityGML or a model from OBJ. This stream has no 3D model.");
    };
    let cut = number_param(p, "cut", "Seen from the height")?;
    let from = p.get("height_from").unwrap_or_else(|| "storey".into());
    let section_only = p.get("shows").as_deref() == Some("section");
    let min_area = number_param(p, "min_area", "Smallest part")?.unwrap_or(0.0);
    if section_only && cut.is_none() {
        return err("Say at what height the cut goes, such as 1, to show only what it goes through.");
    }
    let storey_at = t.field_index("storey_z");
    let low_at = t.field_index("z_min");
    // The triangles of each object.
    let mut of: Vec<Vec<[[f64; 3]; 3]>> = vec![Vec::new(); t.len()];
    for tri in &mesh.tris {
        if let Some(v) = of.get_mut(tri.object as usize) {
            v.push(tri.v.map(|i| mesh.positions[i as usize]));
        }
    }
    let mut out = t.like();
    out.mesh = None;
    out.fields = t.fields.clone();
    out.fields.push(Field::new("cut_z", FieldType::Real));
    let mut other = t.like();
    other.mesh = None;
    other.fields = t.fields.clone();
    let (mut no_storey, mut nothing) = (0usize, 0usize);
    for (row, (f, tris)) in t.features.iter().zip(&of).enumerate() {
        if row % 500 == 0 && !ctx.gate.going() {
            return err(crate::model::STOPPED);
        }
        let lowest = || tris.iter().flatten().map(|p| p[2]).fold(f64::INFINITY, f64::min);
        // The height of the cut, in the heights of the model.
        let z = cut.map(|h| {
            let base = match from.as_str() {
                "absolute" => 0.0,
                "bottom" => low_at.and_then(|k| f.attrs[k].as_f64()).unwrap_or_else(lowest),
                _ => match storey_at.and_then(|k| f.attrs[k].as_f64()) {
                    Some(s) => s,
                    None => {
                        no_storey += 1;
                        lowest()
                    }
                },
            };
            base + h
        });
        let shape = if tris.is_empty() {
            MultiPolygon::new(Vec::new())
        } else if section_only {
            section(tris, z.unwrap_or(0.0))
        } else {
            let pieces: Vec<Polygon<f64>> = tris.iter().filter_map(|t| flat(&z.map_or_else(|| t.to_vec(), |z| below(t, z)))).collect();
            geo::unary_union(pieces.iter())
        };
        let kept: Vec<Polygon<f64>> = shape.0.into_iter().filter(|p| p.unsigned_area() >= min_area.max(1e-12)).collect();
        if kept.is_empty() {
            nothing += 1;
            other.features.push(Feature { attrs: f.attrs.clone(), geom: None, z: None });
            continue;
        }
        let mut attrs = f.attrs.clone();
        attrs.push(z.map_or(Value::Null, |z| Value::Real((z * 1000.0).round() / 1000.0)));
        let g = if kept.len() == 1 {
            geo::Geometry::Polygon(kept.into_iter().next().unwrap_or_else(|| Polygon::new(LineString::new(vec![]), vec![])))
        } else {
            geo::Geometry::MultiPolygon(MultiPolygon::new(kept))
        };
        out.features.push(Feature { attrs, geom: Some(g), z: None });
    }
    if no_storey > 0 && cut.is_some() && from == "storey" {
        ctx.notes.info(format!("{} objects have no storey and were measured from their lowest point", fmt_count(no_storey as u64)));
    }
    if nothing > 0 {
        ctx.notes.info(format!(
            "{} objects have nothing {}, and went to Other",
            fmt_count(nothing as u64),
            if cut.is_some() { "at or below the cut" } else { "to see from above" }
        ));
    }
    ctx.notes.info(format!("{} outlines{}", fmt_count(out.len() as u64), cut.map_or(String::new(), |c| format!(", seen from {} m", fmt_real(c)))));
    Ok(vec![("Outlines".into(), out), ("Other".into(), other)])
}

/// The node types of this file.
pub const TYPES: &[NodeType] = &[NodeType {
    name: "OutlineFromAbove",
    title: "Outline from above",
    hidden: false,
    group: Group::Transformer,
    category: "3D models and views",
    desc: "Makes 3D objects, such as walls, doors and furniture read from IFC, into areas in 2D: the shadow each object casts when lit from straight above, as one area with all that sticks out, such as the handle of a door. Seen from a height, such as 1 m above the floor of the storey, what is above it is left out, so that the wall over a door leaves a gap, as in a plan. Or only what the cut at that height goes through.",
    inputs: &["Input"],
    outputs: &["Outlines", "Other"],
    ports: None,
    params: OUTLINE_PARAMS,
    run: run_outline,
    summary: |p| p.get("cut").filter(|c| !c.trim().is_empty()).map_or_else(|| "from above".into(), |c| format!("seen from {} m", c)),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Mesh, Tri, NONE};

    /// A box as a closed model, from (x0, y0, z0) to (x1, y1, z1).
    fn add_box(m: &mut Mesh, object: u32, lo: [f64; 3], hi: [f64; 3]) {
        let c = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
        let base = m.positions.len() as u32;
        m.positions.extend((0..8).map(c));
        // Twelve triangles, facing out.
        for f in [[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]] {
            m.tris.push(Tri { v: f.map(|i| base + i), n: [NONE; 3], t: [NONE; 3], material: 0, object });
        }
    }

    /// A wall 5 m long, 0.2 m thick and 2.7 m high with an opening for a door 1 m wide and 2.1 m
    /// high, standing on a storey at 10 m; and a door with a handle in the opening.
    fn wall_and_door() -> Table {
        let mut m = Mesh { objects: vec!["wall".into(), "door".into()], ..Default::default() };
        let z = 10.0;
        add_box(&mut m, 0, [0.0, 0.0, z], [2.0, 0.2, z + 2.7]);
        add_box(&mut m, 0, [3.0, 0.0, z], [5.0, 0.2, z + 2.7]);
        add_box(&mut m, 0, [2.0, 0.0, z + 2.1], [3.0, 0.2, z + 2.7]);
        add_box(&mut m, 1, [2.0, 0.08, z], [3.0, 0.12, z + 2.1]);
        // The handle sticks out 6 cm, at 1 m above the floor.
        add_box(&mut m, 1, [2.8, 0.12, z + 1.0], [2.9, 0.18, z + 1.05]);
        Table {
            fields: vec![Field::new("Name", FieldType::Text), Field::new("storey_z", FieldType::Real)],
            features: vec![
                Feature { attrs: vec![Value::Text("wall".into()), Value::Real(z)], geom: None, z: None },
                Feature { attrs: vec![Value::Text("door".into()), Value::Real(z)], geom: None, z: None },
            ],
            crs: Some(3006),
            mesh: Some(std::sync::Arc::new(m)),
            ..Default::default()
        }
    }

    fn areas(o: &Outputs) -> Vec<(f64, usize)> {
        o[0].1
            .features
            .iter()
            .map(|f| {
                let g = f.geom.as_ref().unwrap();
                let parts = match g {
                    geo::Geometry::MultiPolygon(m) => m.0.len(),
                    _ => 1,
                };
                ((g.unsigned_area() * 1e4).round() / 1e4, parts)
            })
            .collect()
    }

    #[test]
    fn the_outline_from_above_leaves_a_gap_where_the_cut_is_below_the_door() {
        // From above, all of it: the wall is one area over the door, 5 by 0.2 m.
        let (o, _) = run_for_test("OutlineFromAbove", &[], vec![("Input", wall_and_door())]).unwrap();
        assert_eq!(areas(&o), [(1.0, 1), (0.04 + 0.006, 1)]);
        // Seen from 1 m: the wall in two parts with the gap of the door; the door with its handle.
        let (o, _) = run_for_test("OutlineFromAbove", &[("cut", "1")], vec![("Input", wall_and_door())]).unwrap();
        assert_eq!(areas(&o), [(0.8, 2), (0.046, 1)]);
        assert_eq!(o[0].1.features[0].attrs.last(), Some(&Value::Real(11.0)));
        // Seen from 0.5 m the handle, at 1 m, is not there.
        let (o, _) = run_for_test("OutlineFromAbove", &[("cut", "0.5")], vec![("Input", wall_and_door())]).unwrap();
        assert_eq!(areas(&o)[1], (0.04, 1));
        // Only what the cut at 1.02 m goes through: the handle is cut, a part of its own.
        let (o, _) = run_for_test("OutlineFromAbove", &[("cut", "1.02"), ("shows", "section")], vec![("Input", wall_and_door())]).unwrap();
        assert_eq!(areas(&o), [(0.8, 2), (0.046, 1)]);
        // Measured from the lowest point, which is the floor here too.
        let (o, _) = run_for_test(
            "OutlineFromAbove",
            &[("cut", "1.02"), ("shows", "section"), ("min_area", "0.01"), ("height_from", "bottom")],
            vec![("Input", wall_and_door())],
        )
        .unwrap();
        assert_eq!(areas(&o), [(0.8, 2), (0.046, 1)]);
        let (o, _) = run_for_test("OutlineFromAbove", &[("cut", "1"), ("min_area", "0.5")], vec![("Input", wall_and_door())]).unwrap();
        assert_eq!((o[0].1.len(), o[1].1.len()), (0, 2));
        let (o, _) = run_for_test("OutlineFromAbove", &[("cut", "1"), ("min_area", "0.2")], vec![("Input", wall_and_door())]).unwrap();
        assert_eq!((o[0].1.len(), o[1].1.len()), (1, 1));
    }
}
