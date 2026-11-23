//! Flow lines: a curved line from place to place for each flow, such as the people who
//! commute from one municipality to another, to draw as a flow map with widths after the
//! numbers.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, Feature, Field, FieldType, Table, Value};
use geo::{Coord, Geometry, InteriorPoint, LineString};
use std::collections::HashMap;

const FLOW_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "from", label: "From", kind: ParamKind::Attribute("Flows"), default: "", hint: "Where each flow starts: the name or code of a place" },
    ParamSpec { key: "to", label: "To", kind: ParamKind::Attribute("Flows"), default: "", hint: "Where it ends" },
    ParamSpec {
        key: "value",
        label: "How many",
        kind: ParamKind::Attribute("Flows"),
        default: "",
        hint: "Such as the number of commuters. Empty: each row counts as one",
    },
    ParamSpec {
        key: "id",
        label: "Names of the places",
        kind: ParamKind::Attribute("Places"),
        default: "",
        hint: "The attribute of Places that From and To name",
    },
    ParamSpec {
        key: "bend",
        label: "Bend in %",
        kind: ParamKind::Text,
        default: "20",
        hint: "How much the lines curve, as a share of their length. Each bends to its right, so that the flows both ways lie side by side. 0: straight",
    },
    ParamSpec {
        key: "smallest",
        label: "Leave out fewer than",
        kind: ParamKind::Text,
        default: "",
        hint: "Flows smaller than this are left out, so that the large ones can be seen. Empty: all",
    },
];

fn run_flows(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let flows = input(inp, "Flows");
    let places = input(inp, "Places");
    if places.is_empty() {
        return err("Connect the places, with their geometry, to Places.");
    }
    let fi = flows.need_field(&p.need("from", "Choose the attribute that says where each flow starts.")?)?;
    let ti = flows.need_field(&p.need("to", "Choose the attribute that says where each flow ends.")?)?;
    let vi = match p.get("value").filter(|v| !v.is_empty()) {
        Some(v) => Some(flows.need_field(&v)?),
        None => None,
    };
    let ii = places.need_field(&p.need("id", "Choose the attribute of Places that From and To name.")?)?;
    let bend = number_param(p, "bend", "Bend")?.unwrap_or(20.0).clamp(-100.0, 100.0) / 100.0;
    let smallest = number_param(p, "smallest", "Leave out fewer than")?;
    // A point in each place: the point itself, or one inside its area.
    let mut at: HashMap<String, Coord<f64>> = HashMap::new();
    for f in &places.features {
        let Some(g) = &f.geom else { continue };
        if let Some(pt) = g.interior_point() {
            at.entry(f.attrs[ii].to_text()).or_insert(pt.0);
        }
    }
    let mut rows: Vec<(String, String, f64, LineString<f64>)> = Vec::new();
    let (mut unknown, mut same, mut small) = (std::collections::BTreeSet::new(), 0usize, 0usize);
    for f in &flows.features {
        let (a, b) = (f.attrs[fi].to_text(), f.attrs[ti].to_text());
        let v = match vi {
            Some(vi) => match f.attrs[vi].as_f64().filter(|v| v.is_finite()) {
                Some(v) => v,
                None => continue,
            },
            None => 1.0,
        };
        if smallest.is_some_and(|s| v < s) || v <= 0.0 {
            small += 1;
            continue;
        }
        if a == b {
            same += 1;
            continue;
        }
        let (Some(pa), Some(pb)) = (at.get(&a), at.get(&b)) else {
            for n in [&a, &b] {
                if !at.contains_key(n) {
                    unknown.insert(n.clone());
                }
            }
            continue;
        };
        rows.push((a, b, v, arc(*pa, *pb, bend)));
    }
    // The largest last, so that they are drawn on top.
    rows.sort_by(|x, y| x.2.total_cmp(&y.2));
    let mut out = Table {
        fields: vec![Field::new("from", FieldType::Text), Field::new("to", FieldType::Text), Field::new("value", FieldType::Real)],
        crs: places.crs,
        ..Default::default()
    };
    for (a, b, v, line) in rows {
        out.features.push(Feature::new(vec![Value::Text(a), Value::Text(b), Value::Real(v)], Some(Geometry::LineString(line))));
    }
    if !unknown.is_empty() {
        let list: Vec<String> = unknown.iter().take(8).cloned().collect();
        ctx.notes.warn(format!(
            "{} names in the flows are not among the places, and their flows are left out: {}{}",
            fmt_count(unknown.len() as u64),
            list.join(", "),
            if unknown.len() > 8 { ", …" } else { "" }
        ));
    }
    if same > 0 {
        ctx.notes.info(format!("{} flows within one place left out", fmt_count(same as u64)));
    }
    if small > 0 && smallest.is_some() {
        ctx.notes.info(format!("{} small flows left out", fmt_count(small as u64)));
    }
    ctx.notes.info(format!("{} flow lines", fmt_count(out.len() as u64)));
    Ok(vec![("Lines".into(), out)])
}

/// A curve from a to b, bent to its right by a share of its length.
fn arc(a: Coord<f64>, b: Coord<f64>, bend: f64) -> LineString<f64> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let mid = Coord { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
    // To the right of the way from a to b.
    let c = Coord { x: mid.x + dy * bend, y: mid.y - dx * bend };
    let n = if bend == 0.0 { 1 } else { 24 };
    let pts: Vec<Coord<f64>> = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            Coord { x: u * u * a.x + 2.0 * u * t * c.x + t * t * b.x, y: u * u * a.y + 2.0 * u * t * c.y + t * t * b.y }
        })
        .collect();
    LineString::new(pts)
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "FlowLines",
    title: "Flow lines",
    hidden: false,
    group: Group::Transformer,
    category: "Maps",
    desc: "A curved line from place to place for each flow, such as the people who commute from one municipality to another, with how many as an attribute: for a flow map in Draw map, with the width of the lines by value. The flows come in as rows with where they start and end, named as the places on Places are, which can be points or areas. Each line bends to its right, so that the flows both ways lie side by side, and the largest come last, so that they are drawn on top.",
    inputs: &["Flows", "Places"],
    outputs: &["Lines"],
    ports: None,
    params: FLOW_PARAMS,
    run: run_flows,
    summary: |p| format!("{} → {}{}", p.get("from").unwrap_or_default(), p.get("to").unwrap_or_default(), p.get("value").filter(|v| !v.is_empty()).map(|v| format!(" · {}", v)).unwrap_or_default()),
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flows_become_curves_between_the_places() {
        let mut places = Table { fields: vec![Field::new("name", FieldType::Text)], crs: Some(3006), ..Default::default() };
        for (n, x) in [("Borås", 0.0), ("Göteborg", 1000.0)] {
            places.features.push(Feature::new(vec![Value::Text(n.into())], Some(Geometry::Point(geo::Point::new(x, 0.0)))));
        }
        let mut flows = Table {
            fields: vec![Field::new("from", FieldType::Text), Field::new("to", FieldType::Text), Field::new("n", FieldType::Int)],
            ..Default::default()
        };
        for (a, b, n) in [("Borås", "Göteborg", 9000), ("Göteborg", "Borås", 3000), ("Borås", "Borås", 5), ("Borås", "Ale", 100)] {
            flows.features.push(Feature::new(vec![Value::Text(a.into()), Value::Text(b.into()), Value::Int(n)], None));
        }
        let (out, notes) =
            run_for_test("FlowLines", &[("from", "from"), ("to", "to"), ("value", "n"), ("id", "name")], vec![("Flows", flows), ("Places", places)]).unwrap();
        let t = &out[0].1;
        assert_eq!(t.len(), 2);
        // The largest last; the two ways bend to opposite sides.
        assert_eq!(t.features[1].attrs[2], Value::Real(9000.0));
        let mid_y = |i: usize| match &t.features[i].geom {
            Some(Geometry::LineString(l)) => l.0[l.0.len() / 2].y,
            _ => panic!(),
        };
        assert!(mid_y(0) * mid_y(1) < 0.0, "{} {}", mid_y(0), mid_y(1));
        assert!(notes.warnings.iter().any(|w| w.contains("Ale")), "{:?}", notes.warnings);
        assert_eq!(t.crs, Some(3006));
    }
}
