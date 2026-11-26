//! Generalization for smaller scales: buildings joined into blocks and made simpler, and
//! features moved away from lines so that both can be seen.

use super::*;
use crate::model::{err, fmt_count, Feature, Field, FieldType, Result, Value};
use geo::{Area, Buffer, Centroid, ClosestPoint, Distance, Euclidean, Geometry, Intersects, MultiPolygon, Simplify};

const BUILDING_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "join",
        label: "Join closer than",
        kind: ParamKind::Text,
        default: "5",
        hint: "In metres: buildings closer to each other than this become one block",
    },
    ParamSpec {
        key: "simplify",
        label: "Simplify by",
        kind: ParamKind::Text,
        default: "",
        hint: "In metres: corners that stick out less than this are taken away. Empty: half of Join closer than",
    },
    ParamSpec { key: "least", label: "Smallest area", kind: ParamKind::Text, default: "50", hint: "In square metres: blocks smaller than this are left out" },
];

/// Buildings joined into blocks where they stand close, simpler, without the smallest.
fn run_buildings(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Buildings");
    let join = number_param(p, "join", "Join closer than")?.unwrap_or(5.0).max(0.0);
    let simplify = number_param(p, "simplify", "Simplify by")?.unwrap_or(join / 2.0).max(0.0);
    let least = number_param(p, "least", "Smallest area")?.unwrap_or(50.0).max(0.0);
    if ctx.geographic(t.crs).unwrap_or(false) {
        return err("The buildings are in degrees: reproject them to metres first, such as to SWEREF 99 TM.");
    }
    let mut polys = Vec::new();
    let mut owner = Vec::new();
    for (i, f) in t.features.iter().enumerate() {
        match &f.geom {
            Some(Geometry::Polygon(p)) => {
                polys.push(p.clone());
                owner.push(i);
            }
            Some(Geometry::MultiPolygon(m)) => {
                for p in &m.0 {
                    polys.push(p.clone());
                    owner.push(i);
                }
            }
            _ => {}
        }
    }
    if polys.is_empty() {
        return err("No buildings came in as areas.");
    }
    // Grown by half the distance, joined, and shrunk again: what stood close is one.
    let grown: Vec<MultiPolygon<f64>> = polys.iter().map(|p| p.buffer(join / 2.0)).collect();
    let joined: MultiPolygon<f64> = geo::unary_union(grown.iter().flat_map(|m| m.0.iter()));
    let blocks = if join > 0.0 { joined.buffer(-join / 2.0) } else { joined };
    let mut out = t.like();
    out.fields.push(Field::new("buildings", FieldType::Int));
    let mut left_out = 0;
    for block in blocks.0 {
        let block = if simplify > 0.0 { block.simplify(simplify) } else { block };
        if block.unsigned_area() < least {
            left_out += 1;
            continue;
        }
        // The attributes of the largest building in the block.
        let inside: Vec<usize> = polys.iter().enumerate().filter(|(_, p)| p.centroid().is_some_and(|c| block.intersects(&c))).map(|(k, _)| k).collect();
        let Some(&largest) = inside.iter().max_by(|a, b| polys[**a].unsigned_area().total_cmp(&polys[**b].unsigned_area())) else {
            continue;
        };
        let mut attrs = t.features[owner[largest]].attrs.clone();
        attrs.push(Value::Int(inside.len() as i64));
        out.features.push(Feature { geom: Some(Geometry::Polygon(block)), attrs, z: None });
    }
    ctx.notes.info(format!("{} buildings became {} blocks", fmt_count(t.len() as u64), fmt_count(out.len() as u64)));
    if left_out > 0 {
        ctx.notes.info(format!("{} blocks smaller than {} m² were left out", fmt_count(left_out as u64), crate::model::fmt_real(least)));
    }
    Ok(vec![("Generalized".into(), out)])
}

const DISPLACE_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "distance",
    label: "At least",
    kind: ParamKind::Text,
    default: "15",
    hint: "In metres: how far from the lines each feature is to be, such as half the width of a road as drawn and a little more",
}];

/// Features moved straight away from the nearest line until they are as far from it as asked,
/// as a mapmaker moves houses off a road drawn wider than it is.
fn run_displace(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Features");
    let lines = input(inp, "Lines");
    let want = number_param(p, "distance", "At least")?.unwrap_or(15.0).max(0.0);
    let roads: Vec<Geometry<f64>> =
        lines.features.iter().filter_map(|f| f.geom.clone()).filter(|g| matches!(g, Geometry::LineString(_) | Geometry::MultiLineString(_))).collect();
    if roads.is_empty() {
        return err("No lines came in on Lines.");
    }
    let mut out = t.like();
    out.fields.push(Field::new("moved", FieldType::Real));
    let mut moved = 0;
    for f in &t.features {
        let mut f2 = f.clone();
        let mut shift = 0.0;
        if let Some(g) = &f.geom {
            if let Some(c) = g.centroid() {
                // The nearest line, and the nearest point on it.
                let near = roads.iter().map(|r| (Euclidean.distance(g, r), r)).min_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((d, r)) = near.filter(|(d, _)| *d < want) {
                    let to = match r.closest_point(&c) {
                        geo::Closest::Intersection(p) | geo::Closest::SinglePoint(p) => Some(p),
                        geo::Closest::Indeterminate => None,
                    };
                    if let Some(q) = to {
                        let (dx, dy) = (c.x() - q.x(), c.y() - q.y());
                        let len = dx.hypot(dy);
                        if len > 1e-9 {
                            let by = want - d;
                            let (mx, my) = (dx / len * by, dy / len * by);
                            use geo::Translate;
                            f2.geom = Some(g.translate(mx, my));
                            shift = by;
                            moved += 1;
                        }
                    }
                }
            }
        }
        f2.attrs.push(Value::Real(shift));
        out.features.push(f2);
    }
    ctx.notes.info(format!("{} of {} moved away from the lines", fmt_count(moved as u64), fmt_count(t.len() as u64)));
    Ok(vec![("Moved".into(), out)])
}

pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "BuildingGeneralizer",
        title: "Generalize buildings",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Buildings for a smaller scale: those closer to each other than a distance become one block, corners that stick out less than a distance are taken away, and the smallest blocks are left out. Each block keeps the attributes of its largest building and the number of buildings in it. In metres.",
        inputs: &["Buildings"],
        outputs: &["Generalized"],
        ports: None,
        params: BUILDING_PARAMS,
        run: run_buildings,
        summary: |p| format!("join closer than {} m", p.get("join").unwrap_or_else(|| "5".into())),
    },
    NodeType {
        name: "FeatureDisplacer",
        title: "Move away from lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Moves features, such as houses, straight away from the nearest of the lines, such as roads, until they are at least a distance from them, so that both are seen when the roads are drawn wider than they are. The attribute moved says how far, in metres.",
        inputs: &["Features", "Lines"],
        outputs: &["Moved"],
        ports: None,
        params: DISPLACE_PARAMS,
        run: run_displace,
        summary: |p| format!("at least {} m", p.get("distance").unwrap_or_else(|| "15".into())),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{line_string, polygon, BooleanOps};

    #[test]
    fn close_buildings_join_and_houses_move_off_the_road() {
        let a = polygon![(x: 0.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 10.0), (x: 0.0, y: 10.0), (x: 0.0, y: 0.0)];
        let b = polygon![(x: 12.0, y: 0.0), (x: 22.0, y: 0.0), (x: 22.0, y: 10.0), (x: 12.0, y: 10.0), (x: 12.0, y: 0.0)];
        let far = polygon![(x: 100.0, y: 0.0), (x: 110.0, y: 0.0), (x: 110.0, y: 10.0), (x: 100.0, y: 10.0), (x: 100.0, y: 0.0)];
        let joined = a.buffer(2.5).union(&b.buffer(2.5)).union(&far.buffer(2.5)).buffer(-2.5);
        assert_eq!(joined.0.len(), 2);
        let road: Geometry<f64> = line_string![(x: -50.0, y: 15.0), (x: 200.0, y: 15.0)].into();
        let house: Geometry<f64> = a.into();
        assert!(Euclidean.distance(&house, &road) < 10.0);
    }
}
