//! Shortest route: from each feature on From to the nearest feature on To, or to every one,
//! along the lines of a network such as the roads of NVDB.

use super::*;
use crate::expr::{self, Row};
use crate::model::{crs_label, err, fmt_count, fmt_real, Feature, Field, FieldType, GbError, Result, Table, Value};
use crate::network::{NetLine, Network, Place};
use geo::{Centroid, Coord, Geometry};

pub(super) const COSTS: &[(&str, &str)] = &[("length", "Shortest: the length"), ("time", "Quickest: the time, by the speed")];
pub(super) const TARGETS: &[(&str, &str)] = &[("nearest", "To the nearest on To"), ("all", "To every one on To")];

pub(super) const ROUTE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "to",
        label: "Routes",
        kind: ParamKind::Choice(TARGETS),
        default: "nearest",
        hint: "Every one gives a route for each pair, as many as From times To",
    },
    ParamSpec { key: "cost", label: "By", kind: ParamKind::Choice(COSTS), default: "length", hint: "" },
    ParamSpec {
        key: "speed",
        label: "Speed in km/h",
        kind: ParamKind::Expression("Network"),
        default: "",
        hint: "Along the line as it is drawn, for the quickest route: an attribute such as F_Hogst_225 in NVDB, or a number",
    },
    ParamSpec {
        key: "speed_against",
        label: "Speed against the line",
        kind: ParamKind::Expression("Network"),
        default: "",
        hint: "Against the way the line is drawn, such as B_Hogst_225 in NVDB. Empty uses the speed along it",
    },
    ParamSpec {
        key: "default_speed",
        label: "Speed where none is given",
        kind: ParamKind::Text,
        default: "50",
        hint: "In km/h, where the speed is empty or 0",
    },
    ParamSpec {
        key: "along",
        label: "May be travelled along the line when",
        kind: ParamKind::Expression("Network"),
        default: "",
        hint: "A condition for one-way roads, such as F_ForbjudenFardriktning is null. Empty: always",
    },
    ParamSpec {
        key: "against",
        label: "May be travelled against the line when",
        kind: ParamKind::Expression("Network"),
        default: "",
        hint: "Such as B_ForbjudenFardriktning is null. Empty: always",
    },
    ParamSpec {
        key: "snap",
        label: "Farthest from the network, in metres",
        kind: ParamKind::Text,
        default: "500",
        hint: "A point is put on the nearest place of the nearest line. One farther away than this is not reached",
    },
    ParamSpec {
        key: "tolerance",
        label: "Line ends that meet, within",
        kind: ParamKind::Text,
        default: "0.01",
        hint: "In metres. Lines are joined where their ends are this close, not where they only cross, as at a bridge",
    },
];

/// The point a feature stands at: a point, the first of several, or the middle of a line
/// or an area.
pub(super) fn point_of(g: &Geometry<f64>) -> Option<Coord<f64>> {
    match g {
        Geometry::Point(p) => Some(p.0),
        Geometry::MultiPoint(m) => m.0.first().map(|p| p.0),
        other => other.centroid().map(|p| p.0),
    }
}

/// The lines of a feature of the network.
pub(super) fn lines_of(g: &Geometry<f64>) -> Vec<geo::LineString<f64>> {
    match g {
        Geometry::LineString(l) => vec![l.clone()],
        Geometry::MultiLineString(m) => m.0.clone(),
        Geometry::Line(l) => vec![geo::LineString::from(*l)],
        _ => Vec::new(),
    }
}

pub(super) fn run_route(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let net_t = input(inp, "Network");
    if net_t.is_empty() {
        return err("Nothing came in on Network. Connect the roads, such as NVDB, to Network.");
    }
    let crs = net_t.crs.ok_or_else(|| GbError("The network has no coordinate system. Set it on the input.".into()))?;
    if ctx.geographic(Some(crs)) == Some(true) {
        return err(format!("The network is in {}, in degrees. Reproject it to metres first, such as to SWEREF 99 TM (EPSG:3006).", crs_label(Some(crs))));
    }
    // From and To in the system of the network.
    let in_crs = |t: &Table, what: &str| -> Result<Table> {
        match t.crs {
            Some(c) if c != crs => reproject(t, crs, ctx.backend),
            None if !t.is_empty() => err(format!("{} has no coordinate system. Set it on the input.", what)),
            _ => Ok(t.clone()),
        }
    };
    let from_t = in_crs(input(inp, "From"), "From")?;
    let to_t = in_crs(input(inp, "To"), "To")?;
    if to_t.is_empty() {
        return err("Nothing came in on To. Connect the places to go to, such as the hospitals.");
    }
    let names = net_t.field_names();
    let compile = |key: &str, what: &str| -> Result<Option<expr::Compiled>> {
        match p.get(key).filter(|v| !v.trim().is_empty()) {
            Some(v) => expr::compile(&v, &names).map(Some).map_err(|e| GbError(format!("{}: {}", what, e.0))),
            None => Ok(None),
        }
    };
    let time = p.get("cost").as_deref() == Some("time");
    let speed = compile("speed", "The speed")?;
    let speed_back = compile("speed_against", "The speed against the line")?;
    if time && speed.is_none() {
        return err("Give the speed, an attribute of the network in km/h, for the quickest route. Or choose the shortest.");
    }
    let along_if = compile("along", "Along the line")?;
    let against_if = compile("against", "Against the line")?;
    let default_speed = number_param(p, "default_speed", "The speed where none is given")?.unwrap_or(50.0);
    if default_speed <= 0.0 {
        return err("The speed where none is given must be more than 0.");
    }
    let snap = number_param(p, "snap", "The farthest from the network")?.unwrap_or(500.0).max(0.0);
    let tolerance = number_param(p, "tolerance", "The tolerance")?.unwrap_or(0.01).max(1e-6);

    // The cost of a metre: 1 for the length, or minutes at the speed.
    let rate = |f: &Feature, e: &Option<expr::Compiled>, i: usize| -> Result<f64> {
        if !time {
            return Ok(1.0);
        }
        let v = match e {
            Some(e) => e.eval(&Row { feature: f, index: i, geographic: Some(false) }).map_err(|err| at_row(err, i))?,
            None => Value::Null,
        };
        let kmh = v.as_f64().or_else(|| crate::model::parse_number(&v.to_text())).filter(|s| *s > 0.0).unwrap_or(default_speed);
        Ok(60.0 / (kmh * 1000.0))
    };
    let allowed = |f: &Feature, e: &Option<expr::Compiled>, i: usize| -> Result<bool> {
        match e {
            Some(e) => e.test(&Row { feature: f, index: i, geographic: Some(false) }).map_err(|err| at_row(err, i)),
            None => Ok(true),
        }
    };
    let mut lines = Vec::new();
    let mut not_lines = 0usize;
    for (i, f) in net_t.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        let ls = lines_of(g);
        if ls.is_empty() {
            not_lines += 1;
            continue;
        }
        let along = if allowed(f, &along_if, i)? { Some(rate(f, &speed, i)?) } else { None };
        let against = if allowed(f, &against_if, i)? { Some(rate(f, if speed_back.is_some() { &speed_back } else { &speed }, i)?) } else { None };
        for l in ls {
            lines.push(NetLine { feature: i, line: l, along, against });
        }
    }
    if not_lines > 0 {
        ctx.notes.warn(format!("{} features on Network are not lines and were left out.", fmt_count(not_lines as u64)));
    }
    let net = Network::build(lines, tolerance);
    if net.edges.is_empty() {
        return err("The network has no lines to travel along.");
    }

    // The destinations on the network.
    let mut targets: Vec<(Place, usize)> = Vec::new();
    let mut far = 0usize;
    for (k, f) in to_t.features.iter().enumerate() {
        match f.geom.as_ref().and_then(point_of).and_then(|c| net.place(c, snap)) {
            Some(pl) => targets.push((pl, k)),
            None => far += 1,
        }
    }
    if far > 0 {
        ctx.notes.warn(format!(
            "{} features on To are more than {} m from the network, or have no geometry, and cannot be reached.",
            fmt_count(far as u64),
            fmt_real(snap)
        ));
    }
    if targets.is_empty() {
        return err(format!("No feature on To is within {} m of the network. Are they in the same area, and in the same coordinate system?", fmt_real(snap)));
    }

    // The routes: the attributes of From, those of To with to_ before, and the route.
    let mut fields = from_t.fields.clone();
    fields.extend(to_t.fields.iter().map(|f| Field::new(format!("to_{}", f.name), f.ty)));
    fields.push(Field::new("length_m", FieldType::Real));
    if time {
        fields.push(Field::new("minutes", FieldType::Real));
    }
    fields.push(Field::new("start_gap_m", FieldType::Real));
    fields.push(Field::new("end_gap_m", FieldType::Real));
    let mut routes = Table { fields, crs: Some(crs), ..Table::default() };
    let mut unreached = from_t.like();
    unreached.fields.push(Field::new("_error", FieldType::Text));
    let round = |x: f64, d: i32| {
        let k = 10f64.powi(d);
        (x * k).round() / k
    };
    let mut push = |o: &Feature, d: &Feature, cost: f64, line: geo::LineString<f64>, a: &Place, b: &Place| {
        let len: f64 = line.0.windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum();
        let mut attrs = o.attrs.clone();
        attrs.extend(d.attrs.iter().cloned());
        attrs.push(Value::Real(round(len, 1)));
        if time {
            attrs.push(Value::Real(round(cost, 2)));
        }
        attrs.push(Value::Real(round(a.off, 1)));
        attrs.push(Value::Real(round(b.off, 1)));
        routes.features.push(Feature { z: None, attrs, geom: Some(Geometry::LineString(line)) });
    };
    let miss = |unreached: &mut Table, f: &Feature, why: String| {
        let mut row = f.clone();
        row.attrs.push(Value::Text(why));
        unreached.features.push(row);
    };
    let gate = ctx.gate;
    let going = || gate.going();
    let total = from_t.len() as u64;
    let all = p.get("to").as_deref() == Some("all");
    if all && from_t.len() * targets.len() > 5_000_000 {
        return err(format!("That is {} routes. Choose the nearest, or fewer on From or To.", fmt_count((from_t.len() * targets.len()) as u64)));
    }
    let backwards = if all { None } else { Some(net.search(&targets, true, &going)) };
    for (i, f) in from_t.features.iter().enumerate() {
        (ctx.progress)("found routes for", i as u64, Some(total));
        if !going() {
            return err(crate::model::STOPPED);
        }
        let Some(place) = f.geom.as_ref().and_then(point_of).and_then(|c| net.place(c, snap)) else {
            miss(&mut unreached, f, format!("More than {} m from the network, or without geometry", fmt_real(snap)));
            continue;
        };
        match &backwards {
            Some(s) => match net.route(s, &place, &targets) {
                Some((cost, k, line)) => {
                    let dest = targets.iter().find(|t| t.1 == k).expect("a destination");
                    push(f, &to_t.features[k], cost, line, &place, &dest.0);
                }
                None => miss(&mut unreached, f, "No way along the network to any feature on To".into()),
            },
            None => {
                let start = [(place, 0usize)];
                let s = net.search(&start, false, &going);
                let mut found = 0;
                for (dp, k) in &targets {
                    if let Some((cost, _, line)) = net.route(&s, dp, &start) {
                        push(f, &to_t.features[*k], cost, line, &place, dp);
                        found += 1;
                    }
                }
                if found == 0 {
                    miss(&mut unreached, f, "No way along the network to any feature on To".into());
                }
            }
        }
    }
    (ctx.progress)("found routes for", total, Some(total));
    ctx.notes.info(format!(
        "{} routes over a network of {} pieces and {} junctions",
        fmt_count(routes.len() as u64),
        fmt_count(net.edges.len() as u64),
        fmt_count(net.nodes as u64)
    ));
    if !unreached.is_empty() {
        ctx.notes.warn(format!("{} features on From have no route and went to Unreached, with the reason.", fmt_count(unreached.len() as u64)));
    }
    Ok(vec![("Routes".into(), routes), ("Unreached".into(), unreached)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "ShortestRoute",
        title: "Shortest route",
        hidden: false,
        group: Group::Transformer,
        category: "Network",
        desc: "Finds the shortest or quickest route along a network of lines, such as the roads of NVDB, from each feature on From to the nearest feature on To, or to every one, with Dijkstra's algorithm. The lines are joined where their ends meet. Points are put on the nearest place of the nearest line. One-way roads and speeds come from attributes of the network. The routes are lines with the attributes of both ends, the length and the time; those with no route go to Unreached.",
        inputs: &["Network", "From", "To"],
        outputs: &["Routes", "Unreached"],
        ports: None,
        params: ROUTE_PARAMS,
        run: run_route,
        summary: |p| {
            let by = if p.get("cost").as_deref() == Some("time") { "quickest" } else { "shortest" };
            let to = if p.get("to").as_deref() == Some("all") { "to every one" } else { "to the nearest" };
            format!("{} {}", by, to)
        },
    },
];
