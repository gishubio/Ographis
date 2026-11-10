//! The length of the lines in each area, added in 0.96: such as the length of the road network
//! in each municipality, in all or for each kind of road.

use super::geom_inspect::{unit_size, LENGTH_UNITS};
use super::vector093::lines_of;
use super::*;
use crate::model::{crs_label, err, fmt_count, fmt_real, Feature, Field, FieldType, Value};
use geo::{BooleanOps, BoundingRect, Geometry, MultiLineString, MultiPolygon};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{RTree, AABB};

const LAYOUTS: &[(&str, &str)] = &[("columns", "An attribute for each kind, on the areas"), ("rows", "A feature for each area and kind")];

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "group_by",
        label: "For each",
        kind: ParamKind::Attributes("Lines"),
        default: "",
        hint: "Attributes of the lines, such as the kind of road. Empty sums all lines together",
    },
    ParamSpec { key: "unit", label: "Unit", kind: ParamKind::Choice(LENGTH_UNITS), default: "km", hint: "" },
    ParamSpec {
        key: "name",
        label: "Attribute",
        kind: ParamKind::Text,
        default: "length",
        hint: "With a kind, the kind is put after it, such as length_motorway",
    },
    ParamSpec { key: "layout", label: "Kinds as", kind: ParamKind::Choice(LAYOUTS), default: "columns", hint: "" },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "3", hint: "Empty keeps them all" },
];

/// A name made safe as an attribute: letters, digits and _.
fn safe(text: &str) -> String {
    let s: String = text.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "empty".into()
    } else {
        s
    }
}

fn run_lengths(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let areas = input(inp, "Areas");
    let lines_t = input(inp, "Lines");
    if areas.is_empty() {
        return err("Nothing came in on Areas. Connect the areas to sum in, such as the municipalities.");
    }
    // The lines in the coordinate system of the areas.
    let lines_t = match (areas.crs, lines_t.crs) {
        (Some(a), Some(l)) if a != l => {
            ctx.notes.info(format!("the lines were reprojected from {} to {}", crs_label(Some(l)), crs_label(Some(a))));
            reproject(lines_t, a, ctx.backend)?
        }
        _ => lines_t.clone(),
    };
    let geographic = match ctx.geographic(areas.crs) {
        Some(g) => g,
        None => return err(format!("It is not known whether {} is in metres or degrees. Set the coordinate system on the input.", crs_label(areas.crs))),
    };
    let unit = p.get("unit").unwrap_or_else(|| "km".into());
    let Some(div) = unit_size(&unit).filter(|_| LENGTH_UNITS.iter().any(|u| u.0 == unit)) else {
        return err(format!("Unknown unit: {}.", unit));
    };
    let scale = match p.get("decimals") {
        None => None,
        Some(d) => match d.trim().parse::<u32>() {
            Ok(d) if d <= 15 => Some(10f64.powi(d as i32)),
            _ => return err(format!("The number of decimals must be a whole number from 0 to 15, not \"{}\".", d)),
        },
    };
    let rounded = |v: f64| scale.map_or(v, |k| (v * k).round() / k);
    let by = p.list("group_by");
    let idx: Vec<usize> = by.iter().map(|n| lines_t.need_field(n)).collect::<Result<_>>()?;
    let name = p.get("name").unwrap_or_else(|| "length".into());
    let rows = p.get("layout").as_deref() == Some("rows");

    // The lines, each with its kind, in an index by their boxes.
    let mut not_lines = 0usize;
    let mut kinds: Vec<String> = Vec::new();
    let mut kind_values: Vec<Vec<Value>> = Vec::new();
    let mut lines: Vec<(MultiLineString<f64>, usize)> = Vec::new();
    for f in &lines_t.features {
        let Some(g) = &f.geom else { continue };
        let ls = lines_of(g);
        if ls.is_empty() {
            not_lines += 1;
            continue;
        }
        let key: Vec<String> = idx.iter().map(|i| f.attrs[*i].to_text()).collect();
        let joined = key.join("_");
        let k = match kinds.iter().position(|x| *x == joined) {
            Some(k) => k,
            None => {
                kinds.push(joined);
                kind_values.push(idx.iter().map(|i| f.attrs[*i].clone()).collect());
                kinds.len() - 1
            }
        };
        lines.push((MultiLineString::new(ls), k));
    }
    if not_lines > 0 {
        ctx.notes.warn(format!("{} features on Lines are not lines and were left out.", fmt_count(not_lines as u64)));
    }
    // The kinds in order, so that the columns come in an order one can find them in.
    let mut order: Vec<usize> = (0..kinds.len()).collect();
    order.sort_by_key(|k| alphabetical(&kinds[*k]));
    let tree: RTree<GeomWithData<Rectangle<[f64; 2]>, usize>> = RTree::bulk_load(
        lines
            .iter()
            .enumerate()
            .filter_map(|(i, (l, _))| l.bounding_rect().map(|r| GeomWithData::new(Rectangle::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]), i)))
            .collect(),
    );

    // The length of each kind in each area.
    let mut not_areas = 0usize;
    let mut sums: Vec<Vec<f64>> = Vec::with_capacity(areas.len());
    for (n, f) in areas.features.iter().enumerate() {
        if n % 64 == 0 {
            (ctx.progress)("areas", n as u64, Some(areas.len() as u64));
        }
        let mut sum = vec![0.0; kinds.len()];
        let polys = f.geom.as_ref().map(geom::polygons_of).unwrap_or_default();
        if polys.is_empty() {
            if f.geom.is_some() {
                not_areas += 1;
            }
            sums.push(sum);
            continue;
        }
        let area = MultiPolygon::new(polys);
        if let Some(r) = area.bounding_rect() {
            let env = AABB::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]);
            for hit in tree.locate_in_envelope_intersecting(&env) {
                let (l, k) = &lines[hit.data];
                let inside = area.clip(l, false);
                sum[*k] += geom::length(&Geometry::MultiLineString(inside), geographic);
            }
        }
        sums.push(sum);
    }
    if not_areas > 0 {
        ctx.notes.warn(format!("{} features on Areas are not areas and got no length.", fmt_count(not_areas as u64)));
    }
    let total: f64 = sums.iter().flatten().sum::<f64>() / div;

    let out = if rows || by.is_empty() {
        // A feature for each area, and kind if there are kinds.
        let mut t = areas.like();
        t.fields = areas.fields.clone();
        for (k, n) in by.iter().enumerate() {
            if t.field_index(n).is_none() {
                t.fields.push(Field::new(n.clone(), lines_t.fields[idx[k]].ty));
            }
        }
        let at: Vec<usize> = by.iter().map(|n| t.field_index(n).expect("added")).collect();
        t.fields.push(Field::new(name.clone(), FieldType::Real));
        for (f, sum) in areas.features.iter().zip(&sums) {
            let groups: Vec<Option<usize>> =
                if by.is_empty() { vec![None] } else { order.iter().map(|k| Some(*k)).filter(|k| sum[k.expect("a kind")] > 0.0).collect() };
            for k in groups {
                let mut attrs = f.attrs.clone();
                attrs.resize(t.fields.len(), Value::Null);
                if let Some(k) = k {
                    for (j, a) in at.iter().enumerate() {
                        attrs[*a] = kind_values[k][j].clone();
                    }
                }
                let v = match k {
                    Some(k) => sum[k],
                    None => sum.iter().sum(),
                };
                *attrs.last_mut().expect("the length") = Value::Real(rounded(v / div));
                t.features.push(Feature { attrs, geom: f.geom.clone(), z: f.z.clone() });
            }
        }
        t
    } else {
        // An attribute for each kind, on the areas.
        let mut t = areas.like();
        t.fields = areas.fields.clone();
        let mut names: Vec<String> = Vec::new();
        for k in &order {
            let mut n = format!("{}_{}", name, safe(&kinds[*k]));
            while t.field_index(&n).is_some() || names.contains(&n) {
                n.push('_');
            }
            names.push(n);
        }
        for n in &names {
            t.fields.push(Field::new(n.clone(), FieldType::Real));
        }
        t.fields.push(Field::new(name.clone(), FieldType::Real));
        for (f, sum) in areas.features.iter().zip(&sums) {
            let mut attrs = f.attrs.clone();
            attrs.extend(order.iter().map(|k| Value::Real(rounded(sum[*k] / div))));
            attrs.push(Value::Real(rounded(sum.iter().sum::<f64>() / div)));
            t.features.push(Feature { attrs, geom: f.geom.clone(), z: f.z.clone() });
        }
        if kinds.len() > 50 {
            ctx.notes.warn(format!("{} kinds gave {} attributes. Was the right attribute chosen?", kinds.len(), kinds.len()));
        }
        t
    };
    if lines.is_empty() {
        ctx.notes.warn("No lines came in on Lines, so every area has the length 0.".to_string());
    } else if total == 0.0 {
        ctx.notes.warn("None of the lines lies in any of the areas. Are they in the same place and coordinate system?".to_string());
    }
    ctx.notes.info(format!(
        "{} {} of lines in {} areas{}",
        fmt_real((total * 1000.0).round() / 1000.0),
        unit,
        fmt_count(areas.len() as u64),
        if kinds.len() > 1 { format!(", {} kinds", kinds.len()) } else { String::new() }
    ));
    Ok(vec![("Areas".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "LineLengthInAreas",
    title: "Sum line lengths in areas",
    hidden: false,
    group: Group::Transformer,
    category: "Geometry",
    desc: "The length of the lines that lie in each area, as the length of the road network in each municipality: in all, or for each kind of line, such as each kind of road, as attributes on the areas or as a feature for each area and kind. A line that crosses a border counts in each area by the part of it inside. In metres or other units, on the ellipsoid when the coordinates are in degrees.",
    inputs: &["Areas", "Lines"],
    outputs: &["Areas"],
    ports: None,
    params: PARAMS,
    run: run_lengths,
    summary: |p| {
        let by = p.list("group_by");
        let unit = p.get("unit").unwrap_or_else(|| "km".into());
        if by.is_empty() {
            format!("in {}", unit)
        } else {
            format!("in {} for each {}", unit, by.join(", "))
        }
    },
}];

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{line_string, polygon};

    #[test]
    fn the_lines_in_each_area_are_summed_by_kind() {
        let mut areas = Table { fields: vec![Field::new("name", FieldType::Text)], crs: Some(3006), ..Default::default() };
        for (n, x) in [("West", 0.0), ("East", 1000.0)] {
            areas.features.push(Feature::new(
                vec![Value::Text(n.into())],
                Some(Geometry::Polygon(polygon![(x: x, y: 0.0), (x: x + 1000.0, y: 0.0), (x: x + 1000.0, y: 1000.0), (x: x, y: 1000.0), (x: x, y: 0.0)])),
            ));
        }
        let mut lines = Table { fields: vec![Field::new("kind", FieldType::Text)], crs: Some(3006), ..Default::default() };
        // A main road across both, 1500 m in West and East together, and a small road in West.
        lines
            .features
            .push(Feature::new(vec![Value::Text("main road".into())], Some(Geometry::LineString(line_string![(x: 500.0, y: 500.0), (x: 2000.0, y: 500.0)]))));
        lines
            .features
            .push(Feature::new(vec![Value::Text("small".into())], Some(Geometry::LineString(line_string![(x: 100.0, y: 100.0), (x: 100.0, y: 400.0)]))));
        let (out, _) =
            run_for_test("LineLengthInAreas", &[("group_by", "kind"), ("unit", "m")], vec![("Areas", areas.clone()), ("Lines", lines.clone())]).unwrap();
        let t = &out[0].1;
        assert_eq!(t.field_names(), ["name", "length_main_road", "length_small", "length"]);
        let row = |i: usize| t.features[i].attrs.iter().skip(1).map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
        assert_eq!(row(0), [500.0, 300.0, 800.0]);
        assert_eq!(row(1), [1000.0, 0.0, 1000.0]);
        // As a feature for each area and kind.
        let (out, _) =
            run_for_test("LineLengthInAreas", &[("group_by", "kind"), ("unit", "km"), ("layout", "rows")], vec![("Areas", areas), ("Lines", lines)]).unwrap();
        let t = &out[0].1;
        assert_eq!(t.field_names(), ["name", "kind", "length"]);
        assert_eq!(t.len(), 3);
        assert_eq!(t.features[0].attrs[2], Value::Real(0.5));
    }
}
