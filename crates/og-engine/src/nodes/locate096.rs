//! Choosing places for facilities along a network, added in 0.96: among candidate places,
//! the ones that give the least travel in all (the p-median of Hakimi, 1964) or that reach the
//! most within a limit (maximal covering, Church and ReVelle, 1974). Found by adding the best
//! place one at a time and then trying to swap each chosen one for each other (Teitz and Bart,
//! 1968), which finds the best or nearly so.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::network::{
    network_crs, network_from, P_AGAINST, P_ALONG, P_AVOID, P_COST, P_DEFAULT_SPEED, P_MODE, P_RULES, P_SNAP, P_SPEED, P_SPEED_AGAINST, P_TOLERANCE,
};
use super::network091::{in_crs, places_of};
use super::*;
#[cfg(test)]
use crate::model::Field;
use crate::model::{err, fmt_count, fmt_real, FieldType, Value};

const GOALS: &[(&str, &str)] =
    &[("least", "The least travel in all: the sum of each one's way to the nearest, weighed"), ("cover", "The most reached within a limit")];

const LOCATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "count",
        label: "How many to choose",
        kind: ParamKind::Text,
        default: "1",
        hint: "Places among Candidates, counting those that must be chosen",
    },
    ParamSpec { key: "goal", label: "Best is", kind: ParamKind::Choice(GOALS), default: "least", hint: "" },
    ParamSpec {
        key: "within",
        label: "Within",
        kind: ParamKind::Text,
        default: "10",
        hint: "For the most reached: minutes for the quickest, metres for the shortest",
    },
    ParamSpec {
        key: "weight",
        label: "Each on Demand counts as",
        kind: ParamKind::Attribute("Demand"),
        default: "",
        hint: "Such as the number of people in a square of a population grid. Empty: each counts as one",
    },
    ParamSpec {
        key: "required",
        label: "Must be chosen when",
        kind: ParamKind::Attribute("Candidates"),
        default: "",
        hint: "An attribute that is true, yes or 1 for the facilities that are there already. Empty: none",
    },
    ParamSpec {
        key: "direction",
        label: "Direction",
        kind: ParamKind::Choice(&[("to", "Travelling to the facilities, as customers do"), ("from", "Travelling from the facilities, as an ambulance does")]),
        default: "to",
        hint: "Matters where there are one-way roads",
    },
    P_RULES,
    P_MODE,
    P_AVOID,
    P_COST,
    P_SPEED,
    P_SPEED_AGAINST,
    P_DEFAULT_SPEED,
    P_ALONG,
    P_AGAINST,
    P_SNAP,
    P_TOLERANCE,
];

/// How good a choice is: for the least travel, the travel (less is better) and how many are
/// not reached at all; for the most reached, what is not reached and then the travel.
fn score(chosen: &[usize], cost: &[Vec<f32>], w: &[f64], cover: Option<f64>) -> (f64, f64) {
    let (mut missed, mut travel) = (0.0, 0.0);
    for (d, wd) in w.iter().enumerate() {
        let best = chosen.iter().map(|c| cost[*c][d]).fold(f32::INFINITY, f32::min) as f64;
        match cover {
            Some(limit) if best > limit => missed += wd,
            _ if !best.is_finite() => missed += wd,
            _ => travel += wd * best,
        }
    }
    (missed, travel)
}

fn better(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.0 - 1e-9 || ((a.0 - b.0).abs() <= 1e-9 && a.1 < b.1 - 1e-6)
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Real(r) => *r != 0.0,
        Value::Text(t) => matches!(t.trim().to_lowercase().as_str(), "true" | "yes" | "ja" | "1" | "y"),
        Value::Null => false,
    }
}

fn run_locate(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use rayon::prelude::*;
    let net_t = input(inp, "Network");
    if net_t.is_empty() {
        return err("Nothing came in on Network. Connect the roads, built with Build network if they are from OpenStreetMap or Overture.");
    }
    let crs = network_crs(net_t, ctx)?;
    let cands = in_crs(input(inp, "Candidates"), crs, "Candidates", ctx)?;
    let demand = in_crs(input(inp, "Demand"), crs, "Demand", ctx)?;
    if cands.is_empty() || demand.is_empty() {
        return err(
            "Connect the places that could be chosen to Candidates, and those to be served, such as addresses or the squares of a population grid, to Demand.",
        );
    }
    let (net, time) = network_from(p, net_t, ctx)?;
    let unit = if time { "minutes" } else { "metres" };
    let snap = number_param(p, "snap", "The farthest from the network")?.unwrap_or(500.0).max(0.0);
    let k = number_param(p, "count", "How many to choose")?.unwrap_or(1.0).max(1.0) as usize;
    let cover = if p.get("goal").as_deref() == Some("cover") { Some(number_param(p, "within", "Within")?.unwrap_or(10.0)) } else { None };
    let backwards = p.get("direction").as_deref() != Some("from");
    let weight = match p.get("weight").filter(|s| !s.trim().is_empty()) {
        Some(a) => {
            let i = demand.need_field(&a)?;
            demand.features.iter().map(|f| f.attrs[i].as_f64().unwrap_or(0.0).max(0.0)).collect()
        }
        None => vec![1.0; demand.len()],
    };
    let required: Vec<bool> = match p.get("required").filter(|s| !s.trim().is_empty()) {
        Some(a) => {
            let i = cands.need_field(&a)?;
            cands.features.iter().map(|f| truthy(&f.attrs[i])).collect()
        }
        None => vec![false; cands.len()],
    };
    let (cplaces, cfar) = places_of(&net, &cands, snap);
    let (dplaces, dfar) = places_of(&net, &demand, snap);
    if cfar > 0 || dfar > 0 {
        ctx.notes.warn(format!(
            "{} candidates and {} on Demand are more than {} m from the network and were left out.",
            cfar,
            fmt_count(dfar as u64),
            fmt_real(snap)
        ));
    }
    let nc = cplaces.len();
    if nc == 0 || dplaces.is_empty() {
        return err("No candidate, or nothing on Demand, is near the network. Are they in the same area and coordinate system?");
    }
    if nc.saturating_mul(dplaces.len()) > 30_000_000 {
        return err(format!(
            "{} candidates and {} on Demand are too many to compare each with each. Put the demand together first, such as people counted in squares of 250 m with Reallocate between areas.",
            nc,
            fmt_count(dplaces.len() as u64)
        ));
    }
    // The travel from each place on Demand to each candidate.
    let w: Vec<f64> = dplaces.iter().map(|(_, d)| weight[*d]).collect();
    let gate = ctx.gate;
    let cost: Vec<Vec<f32>> = cplaces
        .par_iter()
        .map(|c| {
            if !gate.going() {
                return Vec::new();
            }
            let start = [*c];
            let s = net.search(&start, backwards, &|| gate.going());
            dplaces.iter().map(|(pl, _)| net.cost_to(&s, pl, &start).map_or(f32::INFINITY, |v| v as f32)).collect()
        })
        .collect();
    if !ctx.gate.going() {
        return err(crate::model::STOPPED);
    }
    let must: Vec<usize> = (0..nc).filter(|c| required[cplaces[*c].1]).collect();
    if must.len() > k {
        return err(format!("{} candidates must be chosen, more than the {} to choose.", must.len(), k));
    }
    let k = k.min(nc);
    // Add the best one at a time.
    let mut chosen = must.clone();
    let mut order: Vec<usize> = chosen.clone();
    while chosen.len() < k {
        let best = (0..nc)
            .into_par_iter()
            .filter(|c| !chosen.contains(c))
            .map(|c| {
                let mut with = chosen.clone();
                with.push(c);
                (c, score(&with, &cost, &w, cover))
            })
            .reduce_with(|a, b| if better(b.1, a.1) || (!better(a.1, b.1) && b.0 < a.0) { b } else { a });
        let Some((c, _)) = best else { break };
        chosen.push(c);
        order.push(c);
    }
    // Then swap each chosen one, that need not be, for each other, while it gets better.
    let mut now = score(&chosen, &cost, &w, cover);
    let mut swaps = 0;
    for _round in 0..50 {
        let mut improved = false;
        for slot in 0..chosen.len() {
            if must.contains(&chosen[slot]) {
                continue;
            }
            let best = (0..nc)
                .into_par_iter()
                .filter(|c| !chosen.contains(c))
                .map(|c| {
                    let mut with = chosen.clone();
                    with[slot] = c;
                    (c, score(&with, &cost, &w, cover))
                })
                .reduce_with(|a, b| if better(b.1, a.1) { b } else { a });
            if let Some((c, sc)) = best {
                if better(sc, now) {
                    chosen[slot] = c;
                    now = sc;
                    improved = true;
                    swaps += 1;
                }
            }
        }
        if !improved || !ctx.gate.going() {
            break;
        }
    }
    // Each on Demand to its nearest chosen.
    let nearest: Vec<Option<(usize, f32)>> =
        (0..dplaces.len()).map(|d| chosen.iter().map(|c| (*c, cost[*c][d])).filter(|(_, v)| v.is_finite()).min_by(|a, b| a.1.total_cmp(&b.1))).collect();
    let mut served = vec![(0.0f64, 0.0f64, 0.0f64); nc];
    for (d, n) in nearest.iter().enumerate() {
        if let Some((c, v)) = n {
            let within = cover.is_none_or(|l| (*v as f64) <= l);
            if within {
                served[*c].0 += w[d];
                served[*c].1 += w[d] * *v as f64;
            }
            served[*c].2 += w[d];
        }
    }
    // The chosen, the most served first.
    let mut ranked = chosen.clone();
    ranked.sort_by(|a, b| served[*b].0.total_cmp(&served[*a].0));
    let mut out = Table { fields: cands.fields.clone(), crs: Some(crs), ..Default::default() };
    let rank = column(&mut out, "rank", FieldType::Int);
    let serves = column(&mut out, "serves", FieldType::Real);
    let mean = column(&mut out, &format!("mean_{}", unit), FieldType::Real);
    let was = column(&mut out, "required", FieldType::Text);
    for (r, c) in ranked.iter().enumerate() {
        let f = &cands.features[cplaces[*c].1];
        let mut attrs = f.attrs.clone();
        attrs.resize(out.fields.len(), Value::Null);
        attrs[rank] = Value::Int(r as i64 + 1);
        attrs[serves] = Value::Real((served[*c].0 * 100.0).round() / 100.0);
        attrs[mean] = if served[*c].0 > 0.0 { Value::Real((served[*c].1 / served[*c].0 * 10.0).round() / 10.0) } else { Value::Null };
        attrs[was] = Value::Text(if must.contains(c) { "yes" } else { "no" }.into());
        out.features.push(Feature { attrs, ..f.clone() });
    }
    let mut dem = demand.clone();
    let (fac, how) = (column(&mut dem, "facility", FieldType::Int), column(&mut dem, unit, FieldType::Real));
    let reached = cover.map(|_| column(&mut dem, "reached", FieldType::Text));
    for (d, (_, f)) in dplaces.iter().enumerate() {
        if let Some((c, v)) = nearest[d] {
            let r = ranked.iter().position(|x| *x == c).expect("chosen") as i64 + 1;
            dem.features[*f].attrs[fac] = Value::Int(r);
            dem.features[*f].attrs[how] = Value::Real((v as f64 * 10.0).round() / 10.0);
            if let (Some(i), Some(l)) = (reached, cover) {
                dem.features[*f].attrs[i] = Value::Text(if (v as f64) <= l { "yes" } else { "no" }.into());
            }
        }
    }
    let total: f64 = w.iter().sum();
    let said = match cover {
        Some(l) => format!(
            "{} chosen of {} reach {} of {} within {} {}",
            ranked.len(),
            nc,
            fmt_real(((total - now.0) * 10.0).round() / 10.0),
            fmt_real((total * 10.0).round() / 10.0),
            fmt_real(l),
            unit
        ),
        None => format!(
            "{} chosen of {}: {} {} on average to the nearest",
            ranked.len(),
            nc,
            fmt_real((now.1 / (total - now.0).max(1e-9) * 10.0).round() / 10.0),
            unit
        ),
    };
    ctx.notes.info(format!("{}, after {} swaps", said, swaps));
    if cover.is_none() && now.0 > 0.0 {
        ctx.notes.warn(format!("{} on Demand cannot reach any candidate along the network.", fmt_real(now.0)));
    }
    Ok(vec![("Chosen".into(), out), ("Demand".into(), dem)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "LocationChooser",
    title: "Choose locations",
    hidden: false,
    group: Group::Transformer,
    category: "Network",
    desc: "Chooses the best places for one or more facilities among candidates, along the network: those that give the least travel in all for those on Demand, weighed such as by the people in each square, or that reach the most within some minutes. Facilities that are there already can be kept. Each chosen place gets its rank, how many it serves and their mean travel, and each on Demand its facility and travel. Where to put a new health centre, fire station or store.",
    inputs: &["Network", "Candidates", "Demand"],
    outputs: &["Chosen", "Demand"],
    ports: None,
    params: LOCATE_PARAMS,
    run: run_locate,
    summary: |p| {
        let k = p.get("count").unwrap_or_else(|| "1".into());
        if p.get("goal").as_deref() == Some("cover") {
            format!("{} reaching the most within {}", k, p.get("within").unwrap_or_else(|| "10".into()))
        } else {
            format!("{} with the least travel", k)
        }
    },
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::run_for_test;
    use geo::{line_string, Geometry};

    fn net() -> Table {
        // A grid of roads 100 m apart, 10 × 10.
        let mut t = Table { fields: vec![Field::new("id", FieldType::Int)], crs: Some(3006), ..Default::default() };
        for k in 0..=10 {
            let v = k as f64 * 100.0;
            for l in [line_string![(x: 0.0, y: v), (x: 1000.0, y: v)], line_string![(x: v, y: 0.0), (x: v, y: 1000.0)]] {
                t.features.push(Feature::new(vec![Value::Int(k)], Some(Geometry::LineString(l))));
            }
        }
        run_for_test("NetworkBuilder", &[], vec![("Lines", t)]).unwrap().0[0].1.clone()
    }

    fn points(name: &str, pts: &[(f64, f64, f64)]) -> Table {
        Table {
            fields: vec![Field::new(name, FieldType::Real)],
            crs: Some(3006),
            features: pts.iter().map(|(x, y, v)| Feature::new(vec![Value::Real(*v)], Some(Geometry::Point(geo::Point::new(*x, *y))))).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn the_places_nearest_the_people_are_chosen() {
        // People in two villages, 300 in the west and 100 in the east; candidates in a row.
        let demand = points("people", &[(100.0, 500.0, 150.0), (100.0, 600.0, 150.0), (900.0, 500.0, 100.0)]);
        let cands = points("n", &[(100.0, 550.0, 1.0), (500.0, 500.0, 2.0), (900.0, 500.0, 3.0)]);
        let (out, notes) = run_for_test(
            "LocationChooser",
            &[("count", "1"), ("weight", "people")],
            vec![("Network", net()), ("Candidates", cands.clone()), ("Demand", demand.clone())],
        )
        .unwrap();
        let chosen = &out[0].1;
        assert_eq!(chosen.len(), 1, "{:?}", notes.info);
        assert_eq!(chosen.features[0].attrs[0], Value::Real(1.0), "the west, where most live");
        // Two: both villages get theirs.
        let (out, _) = run_for_test(
            "LocationChooser",
            &[("count", "2"), ("weight", "people")],
            vec![("Network", net()), ("Candidates", cands.clone()), ("Demand", demand.clone())],
        )
        .unwrap();
        let mut got: Vec<f64> = out[0].1.features.iter().map(|f| f.attrs[0].as_f64().unwrap()).collect();
        got.sort_by(f64::total_cmp);
        assert_eq!(got, [1.0, 3.0]);
        let d = &out[1].1;
        assert_eq!(d.features[2].attrs[d.field_index("metres").unwrap()], Value::Real(0.0));
        // The most reached within 450 m from one place: the middle reaches no one, the west 300.
        let (out, notes) = run_for_test(
            "LocationChooser",
            &[("count", "1"), ("weight", "people"), ("goal", "cover"), ("within", "450")],
            vec![("Network", net()), ("Candidates", cands), ("Demand", demand)],
        )
        .unwrap();
        assert_eq!(out[0].1.features[0].attrs[0], Value::Real(1.0), "{:?}", notes.info);
        assert!(notes.info.iter().any(|n| n.contains("reach 300 of 400")), "{:?}", notes.info);
    }
}
