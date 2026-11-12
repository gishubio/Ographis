//! Shortest route: from each feature on From to the nearest feature on To, or to every one,
//! along the lines of a network such as the roads of NVDB.

use super::*;
use crate::expr::{self, Row};
use crate::model::{crs_label, err, fmt_count, fmt_real, Feature, Field, FieldType, GbError, Result, Table, Value};
use crate::network::{NetLine, Network, Place};
use geo::{Centroid, Coord, Geometry};

pub(super) const COSTS: &[(&str, &str)] = &[("length", "Shortest: the length"), ("time", "Quickest: the time, by the speed")];
pub(super) const TARGETS: &[(&str, &str)] = &[("nearest", "To the nearest on To"), ("all", "To every one on To")];

pub(super) const P_TO: ParamSpec = ParamSpec {
    key: "to",
    label: "Routes",
    kind: ParamKind::Choice(TARGETS),
    default: "nearest",
    hint: "Every one gives a route for each pair, as many as From times To",
};
pub(super) const RULES: &[(&str, &str)] = &[("own", "My own, below"), ("osm", "OpenStreetMap"), ("nvdb", "NVDB from Lastkajen")];
pub(super) const MODES: &[(&str, &str)] = &[("car", "By car"), ("bike", "By bicycle"), ("walk", "On foot")];
pub(super) const AVOIDS: &[(&str, &str)] = &[("motorway", "Motorways"), ("ferry", "Ferries"), ("unpaved", "Gravel and other unpaved roads"), ("toll", "Tolls")];
pub(super) const P_RULES: ParamSpec = ParamSpec {
    key: "rules",
    label: "Rules of the road",
    kind: ParamKind::Choice(RULES),
    default: "own",
    hint: "Ready rules for OpenStreetMap or NVDB: one-way streets, roundabouts, speed limits and which roads may be used, by how one travels. My own: the conditions and speeds below",
};
pub(super) const P_MODE: ParamSpec = ParamSpec {
    key: "mode",
    label: "Travelling",
    kind: ParamKind::Choice(MODES),
    default: "car",
    hint: "With ready rules: by car at the speed limits, by bicycle at 18 km/h, on foot at 5 km/h",
};
pub(super) const P_AVOID: ParamSpec = ParamSpec {
    key: "avoid",
    label: "Keep away from",
    kind: ParamKind::Picks(AVOIDS),
    default: "",
    hint: "With ready rules, these roads are left out of the network",
};
pub(super) const P_COST: ParamSpec = ParamSpec { key: "cost", label: "By", kind: ParamKind::Choice(COSTS), default: "length", hint: "" };
pub(super) const P_SPEED: ParamSpec = ParamSpec {
    key: "speed",
    label: "Speed in km/h",
    kind: ParamKind::Expression("Network"),
    default: "",
    hint: "Along the line as it is drawn, for the quickest route: an attribute such as F_Hogst_225 in NVDB, or a number",
};
pub(super) const P_SPEED_AGAINST: ParamSpec = ParamSpec {
    key: "speed_against",
    label: "Speed against the line",
    kind: ParamKind::Expression("Network"),
    default: "",
    hint: "Against the way the line is drawn, such as B_Hogst_225 in NVDB. Empty uses the speed along it",
};
pub(super) const P_DEFAULT_SPEED: ParamSpec = ParamSpec {
    key: "default_speed",
    label: "Speed where none is given",
    kind: ParamKind::Text,
    default: "50",
    hint: "In km/h, where the speed is empty or 0",
};
pub(super) const P_ALONG: ParamSpec = ParamSpec {
    key: "along",
    label: "May be travelled along the line when",
    kind: ParamKind::Expression("Network"),
    default: "",
    hint: "A condition for one-way roads, such as F_ForbjudenFardriktning is null. Empty: always",
};
pub(super) const P_AGAINST: ParamSpec = ParamSpec {
    key: "against",
    label: "May be travelled against the line when",
    kind: ParamKind::Expression("Network"),
    default: "",
    hint: "Such as B_ForbjudenFardriktning is null. Empty: always",
};
pub(super) const P_SNAP: ParamSpec = ParamSpec {
    key: "snap",
    label: "Farthest from the network, in metres",
    kind: ParamKind::Text,
    default: "500",
    hint: "A point is put on the nearest place of the nearest line. One farther away than this is not reached",
};
pub(super) const P_TOLERANCE: ParamSpec = ParamSpec {
    key: "tolerance",
    label: "Line ends that meet, within",
    kind: ParamKind::Text,
    default: "0.01",
    hint: "In metres. Lines are joined where their ends are this close, not where they only cross, as at a bridge",
};

pub(super) const ROUTE_PARAMS: &[ParamSpec] =
    &[P_TO, P_RULES, P_MODE, P_AVOID, P_COST, P_SPEED, P_SPEED_AGAINST, P_DEFAULT_SPEED, P_ALONG, P_AGAINST, P_SNAP, P_TOLERANCE];

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

/// The network of the lines on a table, with the cost of each piece: its length, or the
/// minutes at its speed, and the ways it may be travelled, as the settings of the cost, the
/// speeds, along, against and the tolerance say. Gives it, and whether the cost is time.
pub(super) fn network_from(p: &P, net_t: &Table, ctx: &mut NodeCtx) -> Result<(Network, bool)> {
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
    let default_speed_given = number_param(p, "default_speed", "The speed where none is given")?;
    if time && speed.is_none() && p.get("rules").is_none_or(|r| r == "own") {
        // Without a speed of its own, every road is as quick: walking, or a rough guess.
        ctx.notes.info(format!("every road at {} km/h, since no speed is given", fmt_real(default_speed_given.unwrap_or(50.0))));
    }
    let along_if = compile("along", "Along the line")?;
    let against_if = compile("against", "Against the line")?;
    let default_speed = number_param(p, "default_speed", "The speed where none is given")?.unwrap_or(50.0);
    if default_speed <= 0.0 {
        return err("The speed where none is given must be more than 0.");
    }
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
    // Ready rules of the road, in place of the conditions and speeds of the form.
    let rules = p.get("rules").unwrap_or_else(|| "own".into());
    let mode = crate::road_rules::Mode::of(p.get("mode").as_deref());
    let avoid = crate::road_rules::Avoid::of(&p.list("avoid"));
    let nvdb_fields = (rules == "nvdb").then(|| crate::road_rules::NvdbFields::find(&names));
    if let Some(found) = &nvdb_fields {
        let (have, missing) = found.found(&names);
        if have.is_empty() {
            ctx.notes
                .warn("None of the attributes of NVDB were found on Network, such as F_Hogst_225 or F_ForbjudenFardriktning, so every road is used both ways.");
        } else {
            ctx.notes.info(format!("NVDB attributes: {}", have.join(", ")));
            if time && found.speed_along.is_none() && mode == crate::road_rules::Mode::Car {
                ctx.notes.warn(format!("No speed limit was found on Network, so every road is taken at {} km/h.", fmt_real(default_speed)));
            }
            if !missing.is_empty() {
                ctx.notes.info(format!("Not found, and so not used: {}", missing.join(", ")));
            }
        }
    }
    let other_tags = names.iter().position(|n| n == "other_tags");
    if rules == "osm" && !names.iter().any(|n| n == "highway") {
        ctx.notes.warn(
            "There is no attribute highway on Network, as roads from OpenStreetMap have. Read them with the tool for OpenStreetMap, or choose other rules.",
        );
    }
    let ready = |f: &Feature| -> Option<crate::road_rules::Ways> {
        let text = |i: usize| f.attrs.get(i).filter(|v| !v.is_null()).map(|v| v.to_text());
        match rules.as_str() {
            "osm" => {
                let extra = other_tags.and_then(text).map(|t| osm_other_tags(&t)).unwrap_or_default();
                let tag = |k: &str| names.iter().position(|n| n == k).and_then(text).or_else(|| extra.get(k).cloned());
                Some(crate::road_rules::osm(&tag, mode, avoid))
            }
            "nvdb" => Some(crate::road_rules::nvdb(nvdb_fields.as_ref().expect("found"), &text, mode, avoid)),
            _ => None,
        }
    };
    let per_metre = |kmh: f64| if time { 60.0 / ((if kmh > 0.0 { kmh } else { default_speed }) * 1000.0) } else { 1.0 };
    let mut lines = Vec::new();
    let mut not_lines = 0usize;
    let mut left_out = 0usize;
    for (i, f) in net_t.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        let ls = lines_of(g);
        if ls.is_empty() {
            not_lines += 1;
            continue;
        }
        let (along, against) = match ready(f) {
            Some(w) => {
                if w.along.is_none() && w.against.is_none() {
                    left_out += 1;
                }
                (w.along.map(per_metre), w.against.map(per_metre))
            }
            None => (
                if allowed(f, &along_if, i)? { Some(rate(f, &speed, i)?) } else { None },
                if allowed(f, &against_if, i)? { Some(rate(f, if speed_back.is_some() { &speed_back } else { &speed }, i)?) } else { None },
            ),
        };
        for l in ls {
            lines.push(NetLine { feature: i, line: l, along, against });
        }
    }
    if not_lines > 0 {
        ctx.notes.warn(format!("{} features on Network are not lines and were left out.", fmt_count(not_lines as u64)));
    }
    if left_out > 0 {
        let how = match mode {
            crate::road_rules::Mode::Car => "by car",
            crate::road_rules::Mode::Bike => "by bicycle",
            crate::road_rules::Mode::Walk => "on foot",
        };
        ctx.notes.info(format!(
            "{} of {} lines may not be travelled {} or are kept away from, and were left out",
            fmt_count(left_out as u64),
            fmt_count(net_t.len() as u64),
            how
        ));
    }
    let net = Network::build(lines, tolerance);
    if net.edges.is_empty() {
        return err("The network has no lines to travel along.");
    }

    Ok((net, time))
}

/// The tags of OpenStreetMap that have no attribute of their own: as JSON, or as GDAL writes
/// them, "key"=>"value","key"=>"value".
fn osm_other_tags(text: &str) -> std::collections::HashMap<String, String> {
    let t = text.trim();
    if t.starts_with('{') {
        if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(t) {
            return m.into_iter().map(|(k, v)| (k, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect();
        }
    }
    let mut out = std::collections::HashMap::new();
    let mut rest = t;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        let key = &after[..end];
        let tail = &after[end + 1..];
        let Some(arrow) = tail.find("=>\"") else { break };
        let value_part = &tail[arrow + 3..];
        let Some(vend) = value_part.find('"') else { break };
        out.insert(key.to_string(), value_part[..vend].to_string());
        rest = &value_part[vend + 1..];
    }
    out
}

/// The coordinate system of a network, which must be in metres.
pub(super) fn network_crs(net_t: &Table, ctx: &NodeCtx) -> Result<u32> {
    let crs = net_t.crs.ok_or_else(|| GbError("The network has no coordinate system. Set it on the input.".into()))?;
    if ctx.geographic(Some(crs)) == Some(true) {
        return err(format!("The network is in {}, in degrees. Reproject it to metres first, such as to SWEREF 99 TM (EPSG:3006).", crs_label(Some(crs))));
    }
    Ok(crs)
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
    let (net, time) = network_from(p, net_t, ctx)?;
    let snap = number_param(p, "snap", "The farthest from the network")?.unwrap_or(500.0).max(0.0);
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
