//! OpenStreetMap download: a small piece of OpenStreetMap fetched from an Overpass server,
//! within a rectangle or the extent of what comes in on Extent, made smaller to a size
//! that is allowed when it is too large: buildings, roads, water, land use, places of
//! interest, railways, everything or a filter of your own.

use super::*;
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use geo::{BoundingRect, Geometry, LineString, MultiPolygon, Polygon};
use serde_json::Value as Json;
use std::collections::HashMap;

pub(super) const OSM_KINDS: &[(&str, &str)] = &[
    ("buildings", "Buildings"),
    ("roads", "Roads and paths"),
    ("water", "Water: lakes, the sea and streams"),
    ("landuse", "Land use and nature"),
    ("pois", "Places of interest: amenities, shops and tourism"),
    ("railways", "Railways"),
    ("all", "Everything"),
    ("custom", "A filter of your own"),
];

pub(super) const OSM_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "what",
        label: "What",
        kind: ParamKind::Picks(OSM_KINDS),
        default: "buildings",
        hint: "Tick one or more. Each feature gets the attribute kind, which says what it was fetched as",
    },
    ParamSpec {
        key: "filter",
        label: "Filter",
        kind: ParamKind::Text,
        default: "",
        hint: "As in Overpass, such as [\"amenity\"=\"school\"] or [\"highway\"~\"primary|secondary\"]. For A filter of your own",
    },
    ParamSpec {
        key: "bbox",
        label: "Rectangle",
        kind: ParamKind::Text,
        default: "",
        hint: "West, south, east, north in degrees, such as 11.95, 57.69, 11.99, 57.71. Not needed when something comes in on Extent",
    },
    ParamSpec {
        key: "max_km2",
        label: "Largest area in km²",
        kind: ParamKind::Text,
        default: "25",
        hint: "A larger rectangle is made smaller around its middle to this size, so that the server is not asked for too much",
    },
    ParamSpec {
        key: "cut",
        label: "Large features",
        kind: ParamKind::Choice(CUTS),
        default: "cut",
        hint: "Land cover, lakes and borders can be very large: cut, only the part within the area comes along",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "EPSG:3006", hint: "Of what comes out. OpenStreetMap is in WGS 84" },
    ParamSpec {
        key: "server",
        label: "Overpass server",
        kind: ParamKind::Text,
        default: "https://overpass-api.de/api/interpreter",
        hint: "Another, such as https://overpass.kumi.systems/api/interpreter, or one of your own",
    },
];

/// The statements of a query for a kind of features, with {bbox} for the rectangle.
fn statements(what: &str, filter: &str) -> String {
    let s: &[&str] = match what {
        "roads" => &["way[\"highway\"]"],
        "water" => &["nwr[\"natural\"=\"water\"]", "way[\"waterway\"]", "nwr[\"natural\"=\"bay\"]"],
        "landuse" => &["nwr[\"landuse\"]", "nwr[\"natural\"]", "nwr[\"leisure\"]"],
        "pois" => &["nwr[\"amenity\"]", "nwr[\"shop\"]", "nwr[\"tourism\"]"],
        "railways" => &["way[\"railway\"]"],
        "all" => &["nwr"],
        "custom" => return format!("nwr{}({{bbox}});", filter.trim()),
        _ => &["nwr[\"building\"]"],
    };
    s.iter().map(|x| format!("{}({{bbox}});", x)).collect()
}

/// A rectangle (west, south, east, north) in degrees made smaller around its middle so that
/// it is at most `km2`. Gives it and whether it was made smaller.
pub(super) fn limit_bbox(b: [f64; 4], km2: f64) -> ([f64; 4], bool) {
    let lat = ((b[1] + b[3]) / 2.0).to_radians();
    let w_km = (b[2] - b[0]) * 111.32 * lat.cos();
    let h_km = (b[3] - b[1]) * 110.57;
    let area = w_km * h_km;
    if area <= km2 || area <= 0.0 {
        return (b, false);
    }
    let k = (km2 / area).sqrt();
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let (hw, hh) = ((b[2] - b[0]) / 2.0 * k, (b[3] - b[1]) / 2.0 * k);
    ([cx - hw, cy - hh, cx + hw, cy + hh], true)
}

/// Whether a closed way is an area by its tags, as OpenStreetMap counts it.
fn is_area(tags: &serde_json::Map<String, Json>) -> bool {
    if tags.get("area").and_then(|v| v.as_str()) == Some("no") {
        return false;
    }
    if tags.get("area").and_then(|v| v.as_str()) == Some("yes") {
        return true;
    }
    const AREA_KEYS: &[&str] = &[
        "building",
        "landuse",
        "amenity",
        "leisure",
        "natural",
        "place",
        "shop",
        "tourism",
        "man_made",
        "military",
        "aeroway",
        "water",
        "boundary",
        "parking",
        "building:part",
        "landcover",
    ];
    if tags.keys().any(|k| AREA_KEYS.contains(&k.as_str())) {
        // Lines that are tagged with these keys.
        let line = matches!(tags.get("natural").and_then(|v| v.as_str()), Some("coastline" | "cliff" | "ridge" | "tree_row"))
            || matches!(tags.get("man_made").and_then(|v| v.as_str()), Some("pipeline" | "embankment" | "breakwater" | "pier"));
        return !line;
    }
    false
}

fn coords_of(geometry: &Json) -> Vec<geo::Coord<f64>> {
    geometry
        .as_array()
        .map(|a| a.iter().filter_map(|p| Some(geo::coord! { x: p.get("lon")?.as_f64()?, y: p.get("lat")?.as_f64()? })).collect())
        .unwrap_or_default()
}

/// Joins the parts of rings, such as the outer ways of a multipolygon, end to end into
/// closed rings.
fn rings(mut parts: Vec<Vec<geo::Coord<f64>>>) -> Vec<LineString<f64>> {
    let same = |a: geo::Coord<f64>, b: geo::Coord<f64>| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
    let mut out = Vec::new();
    parts.retain(|p| p.len() >= 2);
    while let Some(mut ring) = parts.pop() {
        loop {
            if ring.len() > 3 && same(ring[0], ring[ring.len() - 1]) {
                break;
            }
            let end = ring[ring.len() - 1];
            let Some(k) = parts.iter().position(|p| same(p[0], end) || same(p[p.len() - 1], end)) else { break };
            let mut next = parts.swap_remove(k);
            if !same(next[0], end) {
                next.reverse();
            }
            ring.extend(next.into_iter().skip(1));
        }
        if ring.len() >= 4 && same(ring[0], ring[ring.len() - 1]) {
            out.push(LineString::new(ring));
        }
    }
    out
}

/// The features of an answer of Overpass with out geom: their geometry in WGS 84, and
/// their tags.
/// An element of OpenStreetMap: its id, its geometry and its tags.
pub(super) type Element = (String, Geometry<f64>, serde_json::Map<String, Json>);

pub(super) fn parse_overpass(answer: &str) -> Result<Vec<Element>> {
    let j: Json = serde_json::from_str(answer).map_err(|e| GbError(format!("The answer of the Overpass server is not JSON: {}", e)))?;
    if let Some(remark) = j.get("remark").and_then(|r| r.as_str()).filter(|r| r.contains("error")) {
        return Err(GbError(format!("The Overpass server says: {}", remark)));
    }
    let mut out = Vec::new();
    for e in j.get("elements").and_then(|e| e.as_array()).map(|a| a.as_slice()).unwrap_or(&[]) {
        let kind = e.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let id = e.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
        let tags = e.get("tags").and_then(|t| t.as_object()).cloned().unwrap_or_default();
        let geom = match kind {
            "node" => {
                // The corners of ways come too when everything is asked for: they say nothing.
                if tags.is_empty() {
                    continue;
                }
                match (e.get("lon").and_then(|v| v.as_f64()), e.get("lat").and_then(|v| v.as_f64())) {
                    (Some(x), Some(y)) => Geometry::Point(geo::Point::new(x, y)),
                    _ => continue,
                }
            }
            "way" => {
                let c = coords_of(e.get("geometry").unwrap_or(&Json::Null));
                if c.len() < 2 {
                    continue;
                }
                let closed = c.len() >= 4 && c[0] == c[c.len() - 1];
                if closed && is_area(&tags) {
                    Geometry::Polygon(Polygon::new(LineString::new(c), vec![]))
                } else {
                    Geometry::LineString(LineString::new(c))
                }
            }
            "relation" => {
                let rel = tags.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if rel != "multipolygon" && rel != "boundary" {
                    continue;
                }
                let members = e.get("members").and_then(|m| m.as_array()).cloned().unwrap_or_default();
                let part = |role: &str| -> Vec<Vec<geo::Coord<f64>>> {
                    members
                        .iter()
                        .filter(|m| m.get("type").and_then(|t| t.as_str()) == Some("way"))
                        .filter(|m| {
                            m.get("role").and_then(|r| r.as_str()).unwrap_or("outer") == role
                                || (role == "outer" && m.get("role").and_then(|r| r.as_str()) == Some(""))
                        })
                        .map(|m| coords_of(m.get("geometry").unwrap_or(&Json::Null)))
                        .collect()
                };
                let outers = rings(part("outer"));
                let inners = rings(part("inner"));
                if outers.is_empty() {
                    continue;
                }
                // Each hole goes in the outer ring it lies in.
                let mut polys: Vec<Polygon<f64>> = outers.into_iter().map(|o| Polygon::new(o, vec![])).collect();
                for h in inners {
                    use geo::Contains;
                    if let Some(p) = polys.iter_mut().find(|p| h.0.first().is_some_and(|c| Polygon::new(p.exterior().clone(), vec![]).contains(c))) {
                        p.interiors_push(h);
                    }
                }
                if polys.len() == 1 {
                    Geometry::Polygon(polys.remove(0))
                } else {
                    Geometry::MultiPolygon(MultiPolygon::new(polys))
                }
            }
            _ => continue,
        };
        out.push((format!("{}/{}", kind, id), geom, tags));
    }
    Ok(out)
}

/// The features as a stream: the common tags as attributes of their own, at most 30, and
/// the rest as JSON in other_tags.
pub(super) fn osm_table(items: Vec<(String, Geometry<f64>, serde_json::Map<String, Json>)>) -> Table {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, _, t) in &items {
        for k in t.keys() {
            *counts.entry(k.clone()).or_default() += 1;
        }
    }
    let mut keys: Vec<(String, usize)> = counts.into_iter().filter(|(_, n)| *n * 50 >= items.len().max(1)).collect();
    keys.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    // The name first, after the id.
    let mut keys: Vec<String> = keys.into_iter().map(|k| k.0).filter(|k| k != "name").take(29).collect();
    keys.insert(0, "name".into());
    let mut fields = vec![Field::new("osm_id", FieldType::Text)];
    fields.extend(keys.iter().map(|k| Field::new(k.clone(), FieldType::Text)));
    fields.push(Field::new("other_tags", FieldType::Text));
    let features = items
        .into_iter()
        .map(|(id, g, mut tags)| {
            let mut attrs = vec![Value::Text(id)];
            for k in &keys {
                attrs.push(tags.remove(k).and_then(|v| v.as_str().map(|s| Value::Text(s.to_string()))).unwrap_or(Value::Null));
            }
            attrs.push(if tags.is_empty() { Value::Null } else { Value::Text(Json::Object(tags).to_string()) });
            Feature { attrs, geom: Some(g), z: None }
        })
        .collect();
    Table { fields, crs: Some(4326), features, ..Default::default() }
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// The rectangle to fetch, in degrees: the extent of what comes in on Extent, or the one
/// written in the setting bbox.
pub(super) const CUTS: &[(&str, &str)] = &[("cut", "Cut at the edge of the area"), ("whole", "Whole, when they reach into the area")];

/// The area to fetch within, in degrees: the areas that come in on Extent, or the rectangle
/// when what comes in is not areas.
pub(super) fn area_in_degrees(extent: &Table, bbox: [f64; 4], b: &dyn Backend) -> Result<geo::MultiPolygon<f64>> {
    let rect = || geo::MultiPolygon::new(vec![geo::Rect::new(geo::coord! { x: bbox[0], y: bbox[1] }, geo::coord! { x: bbox[2], y: bbox[3] }).to_polygon()]);
    let areas: Vec<geo::Polygon<f64>> = extent.features.iter().filter_map(|f| f.geom.as_ref()).flat_map(geom::polygons_of).collect();
    if areas.is_empty() {
        return Ok(rect());
    }
    let t =
        Table { features: areas.into_iter().map(|a| Feature::new(vec![], Some(geo::Geometry::Polygon(a)))).collect(), crs: extent.crs, ..Default::default() };
    let t = match extent.crs.filter(|c| *c != 4326) {
        Some(_) => reproject(&t, 4326, Some(b))?,
        None => t,
    };
    let polys: Vec<geo::Polygon<f64>> = t.features.iter().filter_map(|f| f.geom.as_ref()).flat_map(geom::polygons_of).collect();
    let joined = geo::unary_union(polys.iter());
    // Within the rectangle, which may have been made smaller.
    use geo::BooleanOps;
    Ok(joined.intersection(&rect()))
}

/// Leaves out the features that do not reach into the area, though their rectangles do, and
/// cuts those that reach out of it at its edge when `cut`. Gives how many were left out and
/// how many cut.
pub(super) fn keep_within(t: &mut Table, area: &geo::MultiPolygon<f64>, cut: bool) -> (usize, usize) {
    use geo::{BooleanOps, BoundingRect, Contains, Intersects};
    let Some(env) = area.bounding_rect() else { return (0, 0) };
    let (mut out, mut cuts) = (0usize, 0usize);
    let before = t.features.len();
    t.features.retain_mut(|f| {
        let Some(g) = &f.geom else { return true };
        if !g.intersects(area) {
            return false;
        }
        if !cut || g.bounding_rect().is_some_and(|r| env.contains(&r) && area.contains(g)) {
            return true;
        }
        let polys = geom::polygons_of(g);
        let lines: Vec<geo::LineString<f64>> = match g {
            geo::Geometry::LineString(l) => vec![l.clone()],
            geo::Geometry::MultiLineString(m) => m.0.clone(),
            _ => Vec::new(),
        };
        let new = if !polys.is_empty() {
            let m = area.intersection(&geo::MultiPolygon::new(polys));
            match m.0.len() {
                0 => None,
                1 => Some(geo::Geometry::Polygon(m.0.into_iter().next().expect("one"))),
                _ => Some(geo::Geometry::MultiPolygon(m)),
            }
        } else if !lines.is_empty() {
            let m = area.clip(&geo::MultiLineString::new(lines), false);
            match m.0.len() {
                0 => None,
                1 => Some(geo::Geometry::LineString(m.0.into_iter().next().expect("one"))),
                _ => Some(geo::Geometry::MultiLineString(m)),
            }
        } else {
            return true;
        };
        match new {
            Some(n) => {
                f.geom = Some(n);
                f.z = None;
                cuts += 1;
                true
            }
            None => false,
        }
    });
    out += before - t.features.len();
    (out, cuts)
}

/// Keeps what reaches into the area, and says so.
pub(super) fn within_area(p: &P, t: &mut Table, extent: &Table, bbox: [f64; 4], b: &dyn Backend, notes: &mut crate::io::Notes) -> Result<()> {
    let area = area_in_degrees(extent, bbox, b)?;
    let (out, cut) = keep_within(t, &area, p.get("cut").as_deref() != Some("whole"));
    if out > 0 {
        notes.info(format!("{} whose rectangles reach into the area, but not they themselves, were left out", fmt_count(out as u64)));
    }
    if cut > 0 {
        notes.info(format!("{} that reach out of the area were cut at its edge", fmt_count(cut as u64)));
    }
    Ok(())
}

pub(super) fn rect_in_degrees(p: &P, extent: &Table, b: &dyn Backend) -> Result<[f64; 4]> {
    Ok(if extent.has_geometry() && !extent.features.is_empty() {
        let mut r: Option<geo::Rect<f64>> = None;
        for f in &extent.features {
            if let Some(fr) = f.geom.as_ref().and_then(|g| g.bounding_rect()) {
                r = Some(match r {
                    Some(a) => geo::Rect::new(
                        geo::coord! { x: a.min().x.min(fr.min().x), y: a.min().y.min(fr.min().y) },
                        geo::coord! { x: a.max().x.max(fr.max().x), y: a.max().y.max(fr.max().y) },
                    ),
                    None => fr,
                });
            }
        }
        let r = r.ok_or_else(|| GbError("What comes in on Extent has no geometry.".into()))?;
        let (mut xs, mut ys) = (vec![r.min().x, r.max().x, r.min().x, r.max().x], vec![r.min().y, r.min().y, r.max().y, r.max().y]);
        if let Some(c) = extent.crs.filter(|c| *c != 4326) {
            b.transform(c, 4326, &mut xs, &mut ys)?;
        }
        let f = |v: &[f64], max: bool| v.iter().copied().fold(if max { f64::NEG_INFINITY } else { f64::INFINITY }, if max { f64::max } else { f64::min });
        [f(&xs, false), f(&ys, false), f(&xs, true), f(&ys, true)]
    } else {
        let text = p.need("bbox", "Write the rectangle as west, south, east, north in degrees, or connect something to Extent.")?;
        let v: Vec<f64> = text.split([',', ';', ' ']).filter(|t| !t.trim().is_empty()).filter_map(|t| t.trim().parse().ok()).collect();
        if v.len() != 4 || v[0] >= v[2] || v[1] >= v[3] {
            return err(format!("\"{}\" is not a rectangle. Write west, south, east, north, such as 11.95, 57.69, 11.99, 57.71.", text));
        }
        [v[0], v[1], v[2], v[3]]
    })
}

fn run_osm(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let extent = input(inp, "Extent");
    let b = ctx.backend("OpenStreetMap download")?;
    let bbox = rect_in_degrees(p, extent, b)?;
    let max_km2 = number_param(p, "max_km2", "The largest area")?.unwrap_or(25.0).max(0.01);
    let (bbox, smaller) = limit_bbox(bbox, max_km2);
    if smaller {
        ctx.notes.warn(format!("The rectangle was larger than {} km² and was made smaller around its middle.", crate::model::fmt_real(max_km2)));
    }
    let mut kinds = p.list("what");
    if kinds.is_empty() {
        kinds.push("buildings".into());
    }
    let filter = p.get("filter").unwrap_or_default();
    if kinds.iter().any(|k| k == "custom") && filter.trim().is_empty() {
        return err("Write the filter, such as [\"amenity\"=\"school\"].");
    }
    if let Some(bad) = kinds.iter().find(|k| !OSM_KINDS.iter().any(|o| o.0 == k.as_str())) {
        return err(format!("\"{}\" is not something to fetch. Choose among {}.", bad, OSM_KINDS.iter().map(|o| o.0).collect::<Vec<_>>().join(", ")));
    }
    let rect = format!("{:.6},{:.6},{:.6},{:.6}", bbox[1], bbox[0], bbox[3], bbox[2]);
    let server = p.get("server").unwrap_or_else(|| "https://overpass-api.de/api/interpreter".into());
    let headers = vec![("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string()), ("User-Agent".to_string(), "Ographis".to_string())];
    // One question for each kind, so that each feature can say what it was fetched as.
    let mut tables: Vec<std::sync::Arc<Table>> = Vec::new();
    for what in &kinds {
        let query = format!("[out:json][timeout:90];({});out geom;", statements(what, &filter).replace("{bbox}", &rect));
        (ctx.say)(crate::run::Level::Info, &format!("asking {} for {} within {}", server, what, rect));
        let answer = b.http_send("POST", &server, &headers, Some(&format!("data={}", url_encode(&query))), 180)?;
        match answer.status {
            200 => {}
            429 => return err("The Overpass server has too many requests from here just now. Wait a minute and run again."),
            504 => return err("The Overpass server is busy. Run again in a while, or ask for less."),
            s => return err(format!("The Overpass server answered {}: {}", s, answer.body.chars().take(300).collect::<String>())),
        }
        let mut t = osm_table(parse_overpass(&answer.body)?);
        t.fields.insert(0, Field::new("kind", FieldType::Text));
        for f in t.features.iter_mut() {
            f.attrs.insert(0, Value::Text(what.clone()));
        }
        tables.push(std::sync::Arc::new(t));
    }
    let mut table = (*merge_tables(tables, None, "kinds", &mut ctx.notes)?).clone();
    within_area(p, &mut table, extent, bbox, b, &mut ctx.notes)?;
    let n = table.features.len();
    if let Some(to) = p.crs("crs")?.filter(|c| *c != 4326) {
        table = reproject(&table, to, Some(b))?;
    }
    ctx.notes.info(format!("{} features within {}", fmt_count(n as u64), rect));
    Ok(vec![("Output".into(), table)])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "OsmDownloader",
    title: "OpenStreetMap download",
    hidden: false,
    group: Group::Transformer,
    category: "Web",
    desc: "Fetches a small piece of OpenStreetMap from an Overpass server: buildings, roads, water, land use, places of interest, railways, everything, or a filter of your own. Within a rectangle you write, or the extent of what comes in on Extent, such as a municipality; a rectangle larger than a size you give is made smaller around its middle, so that the server is not asked for too much. Points, lines and areas, with the common tags as attributes and the rest in other_tags. Needs the internet; the data is © OpenStreetMap contributors, ODbL.",
    inputs: &["Extent"],
    outputs: &["Output"],
    ports: None,
    params: OSM_PARAMS,
    run: run_osm,
    summary: |p| {
        let kinds = p.list("what");
        let kinds = if kinds.is_empty() { vec!["buildings".to_string()] } else { kinds };
        kinds.iter().map(|k| choice_label(OSM_KINDS, k).split(':').next().unwrap_or("").to_string()).collect::<Vec<_>>().join(", ")
    },
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_only_their_rectangles_reach_in_are_left_out_and_large_ones_cut() {
        use geo::Area;
        let area = MultiPolygon::new(vec![geo::Rect::new(geo::coord! { x: 18.0, y: 59.3 }, geo::coord! { x: 18.1, y: 59.35 }).to_polygon()]);
        // An L-shaped mountain area whose rectangle covers the area, but which lies far away;
        // a forest over the edge; a house inside; a road through.
        let l = Polygon::new(vec![(5.0, 58.0), (31.0, 58.0), (31.0, 71.0), (30.0, 71.0), (30.0, 59.0), (5.0, 59.0), (5.0, 58.0)].into(), vec![]);
        let forest = Polygon::new(vec![(17.9, 59.2), (18.05, 59.2), (18.05, 59.32), (17.9, 59.32), (17.9, 59.2)].into(), vec![]);
        let house = Polygon::new(vec![(18.05, 59.31), (18.051, 59.31), (18.051, 59.311), (18.05, 59.311), (18.05, 59.31)].into(), vec![]);
        let road = LineString::from(vec![(17.0, 59.33), (19.0, 59.33)]);
        let mut t = Table {
            features: vec![Geometry::Polygon(l), Geometry::Polygon(forest), Geometry::Polygon(house.clone()), Geometry::LineString(road)]
                .into_iter()
                .map(|g| Feature::new(vec![], Some(g)))
                .collect(),
            ..Default::default()
        };
        let (out, cut) = keep_within(&mut t, &area, true);
        assert_eq!((out, cut), (1, 2));
        assert_eq!(t.features[1].geom, Some(Geometry::Polygon(house)));
        let forest_area = t.features[0].geom.as_ref().unwrap().unsigned_area();
        assert!((forest_area - 0.05 * 0.02).abs() < 1e-9, "{}", forest_area);
    }

    #[test]
    fn an_answer_of_overpass_becomes_points_lines_and_areas() {
        let answer = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 57.70, "lon": 11.97, "tags": {"amenity": "cafe", "name": "Da Matteo"}},
            {"type": "node", "id": 2, "lat": 57.70, "lon": 11.98},
            {"type": "way", "id": 10, "tags": {"building": "yes"}, "geometry": [{"lat": 57.0, "lon": 11.0}, {"lat": 57.0, "lon": 11.1}, {"lat": 57.1, "lon": 11.1}, {"lat": 57.0, "lon": 11.0}]},
            {"type": "way", "id": 11, "tags": {"highway": "residential", "name": "Storgatan"}, "geometry": [{"lat": 57.0, "lon": 11.0}, {"lat": 57.2, "lon": 11.2}]},
            {"type": "relation", "id": 20, "tags": {"type": "multipolygon", "natural": "water"}, "members": [
                {"type": "way", "role": "outer", "geometry": [{"lat": 0, "lon": 0}, {"lat": 0, "lon": 10}]},
                {"type": "way", "role": "outer", "geometry": [{"lat": 10, "lon": 10}, {"lat": 0, "lon": 10}]},
                {"type": "way", "role": "outer", "geometry": [{"lat": 10, "lon": 10}, {"lat": 0, "lon": 0}]},
                {"type": "way", "role": "inner", "geometry": [{"lat": 1, "lon": 6}, {"lat": 1, "lon": 8}, {"lat": 2, "lon": 8}, {"lat": 1, "lon": 6}]}
            ]}
        ]}"#;
        let items = parse_overpass(answer).unwrap();
        let kinds: Vec<&str> = items.iter().map(|i| crate::geom::type_name(&i.1)).collect();
        assert_eq!(kinds, ["Point", "Polygon", "LineString", "Polygon"]);
        match &items[3].1 {
            Geometry::Polygon(p) => assert_eq!(p.interiors().len(), 1),
            _ => unreachable!(),
        }
        let t = osm_table(items);
        assert_eq!(t.fields[0].name, "osm_id");
        assert_eq!(t.fields[1].name, "name");
        assert_eq!(t.features[0].attrs[1], Value::Text("Da Matteo".into()));
        assert_eq!(t.crs, Some(4326));
        assert!(parse_overpass(r#"{"remark": "runtime error: Query timed out"}"#).is_err());
    }

    #[test]
    fn a_large_rectangle_is_made_smaller_around_its_middle() {
        let (b, smaller) = limit_bbox([11.0, 57.0, 13.0, 58.0], 25.0);
        assert!(smaller);
        let lat = 57.5f64.to_radians();
        let area = (b[2] - b[0]) * 111.32 * lat.cos() * (b[3] - b[1]) * 110.57;
        assert!((area - 25.0).abs() < 0.5, "{}", area);
        assert!(((b[0] + b[2]) / 2.0 - 12.0).abs() < 1e-9);
        assert_eq!(limit_bbox([11.97, 57.70, 11.98, 57.71], 25.0), ([11.97, 57.70, 11.98, 57.71], false));
        assert!(statements("custom", "[\"amenity\"=\"school\"]").starts_with("nwr[\"amenity\"=\"school\"]({bbox});"));
    }
}
