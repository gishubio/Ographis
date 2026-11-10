//! Movement, added in 0.96: points with a time, such as from a GPS (GPX) or the AIS of ships,
//! made into tracks with their speed, and the places where they stopped.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::*;
use crate::expr::{iso_of, seconds_of};
use crate::model::{err, fmt_count, fmt_real, Field, FieldType, Value};
use geo::{Centroid, Coord, Geometry, LineString, MultiPoint, Point};

const TRACK_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "time",
        label: "Time",
        kind: ParamKind::Attribute("Points"),
        default: "time",
        hint: "When each point was taken: ISO 8601 such as 2026-06-01T10:15:00Z, or seconds since 1970",
    },
    ParamSpec {
        key: "id",
        label: "Of whom",
        kind: ParamKind::Attribute("Points"),
        default: "",
        hint: "What tells the movers apart, such as mmsi for ships or the name of a device. Empty: one mover",
    },
    ParamSpec {
        key: "gap",
        label: "A new track after a pause of",
        kind: ParamKind::Text,
        default: "30",
        hint: "Minutes without a point. The track is broken there, so that no line is drawn across what is not known",
    },
    ParamSpec { key: "stop_radius", label: "A stop is within", kind: ParamKind::Text, default: "50", hint: "Metres from where the mover came to it" },
    ParamSpec { key: "stop_minutes", label: "For at least", kind: ParamKind::Text, default: "5", hint: "Minutes. Shorter halts are not stops" },
    ParamSpec {
        key: "fastest",
        label: "Faster than this is a fault",
        kind: ParamKind::Text,
        default: "",
        hint: "Km/h. A point the mover could not have reached so fast is left out, as GPS points that jump. Empty keeps all",
    },
];

/// A point of a mover: where, when, and which feature it is.
#[derive(Clone, Copy)]
struct Fix {
    at: Coord<f64>,
    t: f64,
    feature: usize,
}

/// Metres between two places: on the earth for degrees, straight for metres.
fn metres(a: Coord<f64>, b: Coord<f64>, degrees: bool) -> f64 {
    if degrees {
        let (la1, la2) = (a.y.to_radians(), b.y.to_radians());
        let (dla, dlo) = (la2 - la1, (b.x - a.x).to_radians());
        let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
        2.0 * 6_371_008.8 * h.sqrt().asin()
    } else {
        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
    }
}

fn run_tracks(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Points");
    let time = p.get("time").unwrap_or_else(|| "time".into());
    let ti = t.need_field(&time)?;
    let id = p.get("id").filter(|s| !s.trim().is_empty());
    let ii = id.as_ref().map(|a| t.need_field(a)).transpose()?;
    let gap = number_param(p, "gap", "A new track after a pause of")?.unwrap_or(30.0).max(0.0) * 60.0;
    let radius = number_param(p, "stop_radius", "A stop is within")?.unwrap_or(50.0).max(0.0);
    let dwell = number_param(p, "stop_minutes", "For at least")?.unwrap_or(5.0).max(0.0) * 60.0;
    let fastest = number_param(p, "fastest", "Faster than this is a fault")?.map(|k| k / 3.6);
    let degrees = ctx.geographic(t.crs).unwrap_or(false);
    // A time with a zone is in UTC, and the times written out say so.
    let zoned = t.features.iter().find_map(|f| match &f.attrs[ti] {
        Value::Text(s) if !s.trim().is_empty() => Some(s.trim().ends_with('Z') || s.trim().get(19..).is_some_and(|r| r.contains('+') || r.contains('-'))),
        _ => None,
    });
    let when = |s: f64| if zoned == Some(true) { format!("{}Z", iso_of(s)) } else { iso_of(s) };
    // The points of each mover, in the order of their times.
    let mut movers: Vec<(Value, Vec<Fix>)> = Vec::new();
    let mut which: HashMap<String, usize> = HashMap::new();
    let mut untimed = 0;
    for (k, f) in t.features.iter().enumerate() {
        let (Some(at), Some(s)) = (f.geom.as_ref().and_then(|g| g.centroid()).map(|p| p.0), seconds_of(&f.attrs[ti])) else {
            untimed += 1;
            continue;
        };
        let who = ii.map_or(Value::Null, |i| f.attrs[i].clone());
        let n = movers.len();
        let m = *which.entry(who.to_text()).or_insert(n);
        if m == n {
            movers.push((who, Vec::new()));
        }
        movers[m].1.push(Fix { at, t: s, feature: k });
    }
    if movers.is_empty() {
        return err(format!("No point has both a place and a time in {}.", time));
    }
    let mut points = t.clone();
    let (pi_track, pi_dist, pi_secs, pi_kmh) = (
        column(&mut points, "track", FieldType::Int),
        column(&mut points, "metres", FieldType::Real),
        column(&mut points, "seconds", FieldType::Real),
        column(&mut points, "kmh", FieldType::Real),
    );
    let mut kept = vec![false; t.len()];
    let id_field = id.clone().unwrap_or_else(|| "mover".into());
    let id_type = ii.map_or(FieldType::Text, |i| t.fields[i].ty);
    let mut tracks = Table {
        fields: vec![
            Field::new(&id_field, id_type),
            Field::new("track", FieldType::Int),
            Field::new("start", FieldType::Text),
            Field::new("end", FieldType::Text),
            Field::new("minutes", FieldType::Real),
            Field::new("km", FieldType::Real),
            Field::new("mean_kmh", FieldType::Real),
            Field::new("max_kmh", FieldType::Real),
            Field::new("points", FieldType::Int),
        ],
        crs: t.crs,
        ..Default::default()
    };
    let mut stops = Table {
        fields: vec![
            Field::new(&id_field, id_type),
            Field::new("track", FieldType::Int),
            Field::new("start", FieldType::Text),
            Field::new("end", FieldType::Text),
            Field::new("minutes", FieldType::Real),
            Field::new("points", FieldType::Int),
        ],
        crs: t.crs,
        ..Default::default()
    };
    let (mut faults, mut number) = (0usize, 0i64);
    for (who, mut fixes) in movers {
        fixes.sort_by(|a, b| a.t.total_cmp(&b.t));
        // Points that would need an impossible speed to reach are left out.
        if let Some(most) = fastest {
            let mut clean: Vec<Fix> = Vec::with_capacity(fixes.len());
            for f in fixes {
                match clean.last() {
                    Some(prev) if f.t > prev.t && metres(prev.at, f.at, degrees) / (f.t - prev.t) > most => faults += 1,
                    _ => clean.push(f),
                }
            }
            fixes = clean;
        }
        // Broken where the pause is long.
        let mut runs: Vec<Vec<Fix>> = Vec::new();
        for f in fixes {
            match runs.last_mut() {
                Some(run) if f.t - run.last().expect("a point").t <= gap || gap == 0.0 => run.push(f),
                _ => runs.push(vec![f]),
            }
        }
        for run in runs {
            number += 1;
            let mut km = 0.0;
            let mut top: f64 = 0.0;
            for (k, f) in run.iter().enumerate() {
                kept[f.feature] = true;
                let attrs = &mut points.features[f.feature].attrs;
                attrs[pi_track] = Value::Int(number);
                if k > 0 {
                    let prev = run[k - 1];
                    let (d, s) = (metres(prev.at, f.at, degrees), f.t - prev.t);
                    km += d / 1000.0;
                    attrs[pi_dist] = Value::Real((d * 10.0).round() / 10.0);
                    attrs[pi_secs] = Value::Real(s);
                    if s > 0.0 {
                        let kmh = d / s * 3.6;
                        top = top.max(kmh);
                        attrs[pi_kmh] = Value::Real((kmh * 10.0).round() / 10.0);
                    }
                }
            }
            let (first, last) = (run[0], *run.last().expect("a point"));
            let minutes = (last.t - first.t) / 60.0;
            let line = (run.len() >= 2).then(|| Geometry::LineString(LineString::new(run.iter().map(|f| f.at).collect())));
            let mean = if minutes > 0.0 { km / (minutes / 60.0) } else { 0.0 };
            tracks.features.push(Feature::new(
                vec![
                    who.clone(),
                    Value::Int(number),
                    Value::Text(when(first.t)),
                    Value::Text(when(last.t)),
                    Value::Real((minutes * 10.0).round() / 10.0),
                    Value::Real((km * 1000.0).round() / 1000.0),
                    Value::Real((mean * 10.0).round() / 10.0),
                    Value::Real((top * 10.0).round() / 10.0),
                    Value::Int(run.len() as i64),
                ],
                line.or(Some(Geometry::Point(Point(first.at)))),
            ));
            // Stops: the mover stays within the radius of where it came, for long enough.
            let mut i = 0;
            while i < run.len() {
                let mut j = i;
                while j + 1 < run.len() && metres(run[i].at, run[j + 1].at, degrees) <= radius {
                    j += 1;
                }
                if j > i && run[j].t - run[i].t >= dwell {
                    let mid = MultiPoint::from(run[i..=j].iter().map(|f| Point(f.at)).collect::<Vec<_>>()).centroid().map(Geometry::Point);
                    stops.features.push(Feature::new(
                        vec![
                            who.clone(),
                            Value::Int(number),
                            Value::Text(when(run[i].t)),
                            Value::Text(when(run[j].t)),
                            Value::Real(((run[j].t - run[i].t) / 60.0 * 10.0).round() / 10.0),
                            Value::Int((j - i + 1) as i64),
                        ],
                        mid,
                    ));
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
        }
    }
    // The points in the order of the movers and their times.
    let mut order: Vec<usize> = (0..t.len()).filter(|i| kept[*i]).collect();
    let tr = |i: usize| points.features[i].attrs[pi_track].as_f64().unwrap_or(0.0);
    let tm = |i: usize| seconds_of(&t.features[i].attrs[ti]).unwrap_or(0.0);
    order.sort_by(|a, b| tr(*a).total_cmp(&tr(*b)).then(tm(*a).total_cmp(&tm(*b))));
    points.features = order.into_iter().map(|i| points.features[i].clone()).collect();
    ctx.notes.info(format!(
        "{} tracks of {} points, {} km in all, and {} stops",
        number,
        fmt_count(points.len() as u64),
        fmt_real((tracks.features.iter().map(|f| f.attrs[5].as_f64().unwrap_or(0.0)).sum::<f64>() * 10.0).round() / 10.0),
        stops.len()
    ));
    if untimed > 0 {
        ctx.notes.warn(format!("{} points have no time that can be read in {}, or no place, and were left out.", fmt_count(untimed as u64), time));
    }
    if faults > 0 {
        ctx.notes.info(format!("{} points that would need more than the fastest speed were left out", fmt_count(faults as u64)));
    }
    Ok(vec![("Tracks".into(), tracks), ("Points".into(), points), ("Stops".into(), stops)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "TrackBuilder",
    title: "Tracks and stops",
    hidden: false,
    group: Group::Transformer,
    category: "Statistics",
    desc: "Makes points with a time into tracks, as from a GPS (GPX) or the AIS of ships: a line for each mover, broken where a pause is long, with its start, end, length and speed; each point with how far and how fast it came from the one before; and the stops, where the mover stayed within a radius for some minutes. Points that jump faster than the mover can go can be left out.",
    inputs: &["Points"],
    outputs: &["Tracks", "Points", "Stops"],
    ports: None,
    params: TRACK_PARAMS,
    run: run_tracks,
    summary: |p| p.get("id").filter(|s| !s.is_empty()).map(|i| format!("each {}", i)).unwrap_or_else(|| "one mover".into()),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::run_for_test;

    #[test]
    fn points_with_times_become_tracks_with_speeds_and_stops() {
        // Two boats in metres. Boat A goes east 10 m a second for a minute, waits ten minutes,
        // and after an hour's silence goes on. Boat B has a point that jumps far away.
        let mut t = Table { fields: vec![Field::new("mmsi", FieldType::Int), Field::new("time", FieldType::Text)], crs: Some(3006), ..Default::default() };
        let mut add = |boat: i64, s: i64, x: f64, y: f64| {
            let time = format!("2026-06-01T{:02}:{:02}:{:02}Z", 10 + s / 3600, (s % 3600) / 60, s % 60);
            t.features.push(Feature::new(vec![Value::Int(boat), Value::Text(time)], Some(Geometry::Point(Point::new(x, y)))));
        };
        for k in 0..=6 {
            add(1, k * 10, k as f64 * 100.0, 0.0);
        }
        for k in 1..=10 {
            add(1, 60 + k * 60, 600.0 + (k % 2) as f64 * 5.0, 0.0);
        }
        add(1, 60 + 600 + 3600, 1000.0, 0.0);
        add(1, 60 + 600 + 3660, 1600.0, 0.0);
        for k in 0..5 {
            add(2, k * 60, 0.0, 1000.0 + k as f64 * 60.0);
        }
        add(2, 150, 50_000.0, 50_000.0);
        let (out, notes) = run_for_test("TrackBuilder", &[("id", "mmsi"), ("fastest", "100")], vec![("Points", t)]).unwrap();
        let tracks = &out[0].1;
        assert_eq!(tracks.len(), 3, "{:?}", notes.info);
        let get = |i: usize, n: &str| tracks.features[i].attrs[tracks.field_index(n).unwrap()].clone();
        assert_eq!(get(0, "mmsi"), Value::Int(1));
        assert_eq!(get(0, "max_kmh"), Value::Real(36.0));
        assert_eq!(get(0, "start"), Value::Text("2026-06-01T10:00:00Z".into()));
        assert_eq!(get(1, "km"), Value::Real(0.6));
        assert_eq!(get(2, "points"), Value::Int(5), "the point that jumped is left out");
        let stops = &out[2].1;
        assert_eq!(stops.len(), 1);
        assert_eq!(stops.features[0].attrs[stops.field_index("minutes").unwrap()], Value::Real(10.0));
        let pts = &out[1].1;
        assert_eq!(pts.features[1].attrs[pts.field_index("kmh").unwrap()], Value::Real(36.0));
    }
}
