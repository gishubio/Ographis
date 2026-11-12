//! Ready rules for travelling on roads from OpenStreetMap and NVDB: which roads may be used,
//! in which direction, and at what speed, by car, by bicycle or on foot, so that one-way
//! streets and roundabouts are followed without conditions written by hand.

/// How one travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Car,
    Bike,
    Walk,
}

impl Mode {
    pub fn of(s: Option<&str>) -> Mode {
        match s {
            Some("bike") => Mode::Bike,
            Some("walk") => Mode::Walk,
            _ => Mode::Car,
        }
    }

    /// The speed on a road of no speed of its own, in km/h, for bicycle and foot.
    pub fn own_speed(self) -> Option<f64> {
        match self {
            Mode::Car => None,
            Mode::Bike => Some(18.0),
            Mode::Walk => Some(5.0),
        }
    }
}

/// Roads to keep away from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Avoid {
    pub motorways: bool,
    pub ferries: bool,
    pub unpaved: bool,
    pub tolls: bool,
}

impl Avoid {
    pub fn of(list: &[String]) -> Avoid {
        let has = |k: &str| list.iter().any(|l| l == k);
        Avoid { motorways: has("motorway"), ferries: has("ferry"), unpaved: has("unpaved"), tolls: has("toll") }
    }
}

/// What a road allows: the speed in km/h along the line as it is drawn and against it, None
/// where it may not be travelled that way. A speed of 0 means the road may be used, at the
/// speed given where none is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ways {
    pub along: Option<f64>,
    pub against: Option<f64>,
}

impl Ways {
    pub const NONE: Ways = Ways { along: None, against: None };
    fn both(s: f64) -> Ways {
        Ways { along: Some(s), against: Some(s) }
    }
}

fn yes(v: Option<&str>) -> bool {
    matches!(v, Some("yes" | "true" | "1" | "designated" | "permissive"))
}

/// A speed limit in OpenStreetMap: "50", "30 mph", or a kind of road by country.
fn osm_speed(v: &str) -> Option<f64> {
    let v = v.trim();
    let first = v.split(';').next()?.trim();
    if let Some(n) = first.strip_suffix("mph").map(str::trim).and_then(|n| n.parse::<f64>().ok()) {
        return Some(n * 1.609);
    }
    if let Ok(n) = first.parse::<f64>() {
        return (n > 0.0).then_some(n);
    }
    match first {
        "SE:urban" | "DE:urban" | "NO:urban" | "DK:urban" | "FI:urban" => Some(50.0),
        "SE:rural" | "DK:rural" => Some(70.0),
        "NO:rural" | "FI:rural" => Some(80.0),
        "DE:rural" => Some(100.0),
        "SE:motorway" | "DK:motorway" => Some(110.0),
        "walk" | "SE:walk" | "DE:living_street" | "SE:living_street" => Some(7.0),
        _ => None,
    }
}

/// The speed by the kind of road, where no limit is given.
fn osm_kind_speed(highway: &str) -> f64 {
    match highway {
        "motorway" => 110.0,
        "trunk" => 90.0,
        "primary" => 80.0,
        "secondary" => 70.0,
        "tertiary" => 60.0,
        "motorway_link" | "trunk_link" => 60.0,
        "primary_link" | "secondary_link" | "tertiary_link" => 50.0,
        "unclassified" | "road" => 50.0,
        "residential" => 40.0,
        "service" | "track" => 20.0,
        "living_street" => 7.0,
        _ => 30.0,
    }
}

/// The rules of a way of OpenStreetMap, by its tags.
pub fn osm(tag: &dyn Fn(&str) -> Option<String>, mode: Mode, avoid: Avoid) -> Ways {
    let get = |k: &str| tag(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let ferry = get("route").as_deref() == Some("ferry");
    let highway = get("highway").unwrap_or_default();
    if highway.is_empty() && !ferry {
        return Ways::NONE;
    }
    if ferry {
        return if avoid.ferries { Ways::NONE } else { Ways::both(get("maxspeed").as_deref().and_then(osm_speed).unwrap_or(15.0)) };
    }
    if matches!(highway.as_str(), "construction" | "proposed" | "abandoned" | "platform" | "raceway" | "bus_guideway" | "elevator" | "corridor") {
        return Ways::NONE;
    }
    let access = get("access");
    let closed = |v: Option<String>| matches!(v.as_deref(), Some("no" | "private"));
    let motorway = matches!(highway.as_str(), "motorway" | "motorway_link") || yes(get("motorroad").as_deref());
    if avoid.motorways && motorway {
        return Ways::NONE;
    }
    if avoid.tolls && yes(get("toll").as_deref()) {
        return Ways::NONE;
    }
    if avoid.unpaved {
        let surface = get("surface").unwrap_or_default();
        let loose = matches!(
            surface.as_str(),
            "unpaved" | "gravel" | "fine_gravel" | "compacted" | "dirt" | "earth" | "ground" | "grass" | "sand" | "mud" | "pebblestone" | "woodchips"
        );
        let track = highway == "track" && get("tracktype").as_deref() != Some("grade1");
        if loose || track {
            return Ways::NONE;
        }
    }
    let oneway_tag = get("oneway");
    let roundabout = matches!(get("junction").as_deref(), Some("roundabout" | "circular"));
    // One way, as tagged, as a roundabout, or as a motorway.
    let one_way = |tag: Option<String>| -> (bool, bool) {
        match tag.as_deref() {
            Some("-1" | "reverse") => (false, true),
            Some("yes" | "true" | "1") => (true, false),
            Some("no" | "false" | "0" | "alternating" | "reversible") => (true, true),
            _ if roundabout || matches!(highway.as_str(), "motorway") => (true, false),
            _ => (true, true),
        }
    };
    match mode {
        Mode::Car => {
            let car_road = matches!(
                highway.as_str(),
                "motorway"
                    | "motorway_link"
                    | "trunk"
                    | "trunk_link"
                    | "primary"
                    | "primary_link"
                    | "secondary"
                    | "secondary_link"
                    | "tertiary"
                    | "tertiary_link"
                    | "unclassified"
                    | "residential"
                    | "living_street"
                    | "service"
                    | "road"
                    | "track"
            );
            let motor = get("motor_vehicle").or_else(|| get("motorcar")).or_else(|| get("vehicle"));
            if !car_road || closed(motor.clone()) || (closed(access) && !yes(motor.as_deref()) && motor.as_deref() != Some("destination")) {
                return Ways::NONE;
            }
            if highway == "track" && !yes(motor.as_deref()) {
                return Ways::NONE;
            }
            let (along, against) = one_way(oneway_tag);
            let base = get("maxspeed").as_deref().and_then(osm_speed).unwrap_or_else(|| osm_kind_speed(&highway));
            let fwd = get("maxspeed:forward").as_deref().and_then(osm_speed).unwrap_or(base);
            let back = get("maxspeed:backward").as_deref().and_then(osm_speed).unwrap_or(base);
            Ways { along: along.then_some(fwd), against: against.then_some(back) }
        }
        Mode::Bike => {
            let bicycle = get("bicycle");
            if closed(bicycle.clone()) || highway == "steps" && !yes(bicycle.as_deref()) {
                return Ways::NONE;
            }
            let forbidden = matches!(highway.as_str(), "motorway" | "motorway_link" | "trunk" | "trunk_link") || yes(get("motorroad").as_deref());
            let foot_only = matches!(highway.as_str(), "footway" | "pedestrian" | "bridleway");
            if (forbidden || foot_only) && !yes(bicycle.as_deref()) {
                return Ways::NONE;
            }
            if closed(access) && !yes(bicycle.as_deref()) {
                return Ways::NONE;
            }
            // Cycles may often go both ways on a one-way street.
            let bike_both = matches!(get("oneway:bicycle").as_deref(), Some("no")) || get("cycleway").is_some_and(|c| c.starts_with("opposite"));
            let (along, against) = if bike_both { (true, true) } else { one_way(get("oneway:bicycle").or(oneway_tag)) };
            let s = Mode::Bike.own_speed().expect("a speed");
            Ways { along: along.then_some(s), against: against.then_some(s) }
        }
        Mode::Walk => {
            let foot = get("foot");
            if closed(foot.clone()) {
                return Ways::NONE;
            }
            let forbidden = matches!(highway.as_str(), "motorway" | "motorway_link") || yes(get("motorroad").as_deref());
            if (forbidden || closed(access)) && !yes(foot.as_deref()) {
                return Ways::NONE;
            }
            let (along, against) = if get("oneway:foot").as_deref() == Some("yes") { (true, false) } else { (true, true) };
            let s = Mode::Walk.own_speed().expect("a speed");
            Ways { along: along.then_some(s), against: against.then_some(s) }
        }
    }
}

/// The attributes of NVDB as they were found in the data, since their names differ between
/// exports from Lastkajen: each by a part of its name, without case, underscores or å, ä, ö.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NvdbFields {
    pub speed_along: Option<usize>,
    pub speed_against: Option<usize>,
    pub forbidden_along: Option<usize>,
    pub forbidden_against: Option<usize>,
    pub ferry: Option<usize>,
    pub motorway: Option<usize>,
    pub motorroad: Option<usize>,
    pub surface: Option<usize>,
    pub cycle_path: Option<usize>,
    pub toll: Option<usize>,
}

/// A name as letters and digits only, small, with å, ä and ö as a, a and o.
fn plain(name: &str) -> String {
    name.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'å' | 'ä' => 'a',
            'ö' => 'o',
            c => c,
        })
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

impl NvdbFields {
    pub fn find(names: &[String]) -> NvdbFields {
        let plain: Vec<String> = names.iter().map(|n| plain(n)).collect();
        // The forward and backward kind of an attribute: F_ or _F, B_ or _B.
        let side = |i: usize, f: bool| {
            let raw = names[i].to_lowercase();
            let (p, s) = if f { ("f_", "_f") } else { ("b_", "_b") };
            raw.starts_with(p) || raw.ends_with(s) || raw.contains(&format!("_{}_", &p[..1]))
        };
        let first = |words: &[&str], f: Option<bool>| -> Option<usize> {
            (0..names.len()).find(|i| words.iter().any(|w| plain[*i].contains(w)) && f.is_none_or(|f| side(*i, f)))
        };
        NvdbFields {
            speed_along: first(&["hogst", "hastighetsgrans", "hogstatillaten"], Some(true)),
            speed_against: first(&["hogst", "hastighetsgrans", "hogstatillaten"], Some(false)),
            forbidden_along: first(&["forbjudenfardriktning"], Some(true)),
            forbidden_against: first(&["forbjudenfardriktning"], Some(false)),
            ferry: first(&["farjeled", "farjestrak"], None),
            motorway: first(&["motorvag"], None),
            motorroad: first(&["motortrafikled"], None),
            surface: first(&["slitlager"], None),
            cycle_path: first(&["gcmtyp", "gcmvagtyp", "gcm"], None),
            toll: first(&["vagavgift", "trangselskatt", "infrastrukturavgift"], None),
        }
    }

    /// The names of what was found and what was not, for the log.
    pub fn found(&self, names: &[String]) -> (Vec<String>, Vec<&'static str>) {
        let all = [
            (self.speed_along, "speed along"),
            (self.speed_against, "speed against"),
            (self.forbidden_along, "forbidden direction along"),
            (self.forbidden_against, "forbidden direction against"),
            (self.ferry, "ferry"),
            (self.motorway, "motorway"),
            (self.motorroad, "motor road"),
            (self.surface, "surface"),
            (self.cycle_path, "cycle path"),
            (self.toll, "toll"),
        ];
        let found = all.iter().filter_map(|(i, w)| i.map(|i| format!("{} ({})", names[i], w))).collect();
        let missing = all.iter().filter(|(i, _)| i.is_none()).map(|(_, w)| *w).collect();
        (found, missing)
    }
}

/// The rules of a road of NVDB, by the text of its attributes: None for an empty value.
pub fn nvdb(fields: &NvdbFields, value: &dyn Fn(usize) -> Option<String>, mode: Mode, avoid: Avoid) -> Ways {
    let has = |f: Option<usize>| f.and_then(value).is_some_and(|v| !matches!(v.trim().to_lowercase().as_str(), "" | "0" | "nej" | "no" | "false"));
    let speed = |f: Option<usize>| f.and_then(value).and_then(|v| crate::model::parse_number(&v)).filter(|s| *s > 0.0);
    let ferry = has(fields.ferry);
    let motorway = has(fields.motorway) || has(fields.motorroad);
    let cycle = has(fields.cycle_path);
    if (avoid.ferries && ferry) || (avoid.motorways && motorway) || (avoid.tolls && has(fields.toll)) {
        return Ways::NONE;
    }
    if avoid.unpaved {
        let s = fields.surface.and_then(value).unwrap_or_default().to_lowercase();
        if s.contains("grus") || s.contains("obelagd") || s.contains("unpaved") || s.trim() == "2" {
            return Ways::NONE;
        }
    }
    let along_ok = !has(fields.forbidden_along);
    let against_ok = !has(fields.forbidden_against);
    match mode {
        Mode::Car => {
            if cycle {
                return Ways::NONE;
            }
            let (fwd, back) = (speed(fields.speed_along), speed(fields.speed_against));
            let fwd = fwd.or(back).unwrap_or(0.0);
            let back = back.or(Some(fwd)).unwrap_or(0.0);
            Ways { along: along_ok.then_some(fwd), against: against_ok.then_some(back) }
        }
        Mode::Bike => {
            if motorway {
                return Ways::NONE;
            }
            let s = Mode::Bike.own_speed().expect("a speed");
            // A cycle path may be travelled both ways; a street as it is signed.
            if cycle {
                Ways::both(s)
            } else {
                Ways { along: along_ok.then_some(s), against: against_ok.then_some(s) }
            }
        }
        Mode::Walk => {
            if motorway {
                return Ways::NONE;
            }
            Ways::both(Mode::Walk.own_speed().expect("a speed"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn osm_of(pairs: &[(&str, &str)], mode: Mode, avoid: Avoid) -> Ways {
        let t = tags(pairs);
        osm(&|k| t.get(k).cloned(), mode, avoid)
    }

    #[test]
    fn osm_roads_go_one_way_round_roundabouts_and_at_their_speed() {
        let car = |p: &[(&str, &str)]| osm_of(p, Mode::Car, Avoid::default());
        assert_eq!(car(&[("highway", "residential")]), Ways { along: Some(40.0), against: Some(40.0) });
        assert_eq!(car(&[("highway", "primary"), ("oneway", "yes"), ("maxspeed", "60")]), Ways { along: Some(60.0), against: None });
        assert_eq!(car(&[("highway", "secondary"), ("oneway", "-1")]).along, None);
        assert_eq!(car(&[("highway", "tertiary"), ("junction", "roundabout")]), Ways { along: Some(60.0), against: None });
        assert_eq!(car(&[("highway", "motorway")]).against, None, "a motorway is one way");
        assert_eq!(car(&[("highway", "footway")]), Ways::NONE);
        assert_eq!(car(&[("highway", "service"), ("access", "private")]), Ways::NONE);
        assert_eq!(car(&[("highway", "primary"), ("maxspeed", "SE:rural")]).along, Some(70.0));
        // A cycle may often go against a one-way street, and walking ignores it.
        let s = [("highway", "residential"), ("oneway", "yes"), ("oneway:bicycle", "no")];
        assert_eq!(osm_of(&s, Mode::Bike, Avoid::default()), Ways { along: Some(18.0), against: Some(18.0) });
        assert_eq!(osm_of(&[("highway", "residential"), ("oneway", "yes")], Mode::Walk, Avoid::default()).against, Some(5.0));
        assert_eq!(osm_of(&[("highway", "motorway")], Mode::Bike, Avoid::default()), Ways::NONE);
        // What is avoided is left out.
        let avoid = Avoid { motorways: true, ferries: true, unpaved: true, tolls: true };
        assert_eq!(osm_of(&[("highway", "motorway")], Mode::Car, avoid), Ways::NONE);
        assert_eq!(osm_of(&[("route", "ferry")], Mode::Car, avoid), Ways::NONE);
        assert!(osm_of(&[("route", "ferry")], Mode::Car, Avoid::default()).along.is_some());
        assert_eq!(osm_of(&[("highway", "unclassified"), ("surface", "gravel")], Mode::Car, avoid), Ways::NONE);
    }

    #[test]
    fn nvdb_attributes_are_found_by_their_names() {
        let names: Vec<String> =
            ["ELEMENT_ID", "F_Hogst_225", "B_Hogst_225", "F_ForbjudenFardriktning", "B_ForbjudenFardriktning", "Farjeled", "Motorvag", "GCM_typ", "Slitlager"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let f = NvdbFields::find(&names);
        assert_eq!((f.speed_along, f.speed_against, f.forbidden_along, f.forbidden_against), (Some(1), Some(2), Some(3), Some(4)));
        assert_eq!((f.ferry, f.motorway, f.cycle_path, f.surface), (Some(5), Some(6), Some(7), Some(8)));
        fn row(v: Vec<&'static str>) -> impl Fn(usize) -> Option<String> {
            move |i: usize| v.get(i).filter(|s| !s.is_empty()).map(|s| s.to_string())
        }
        // A one-way street, 50 one way.
        let one_way = row(vec!["1", "50", "", "", "1", "", "", "", "Belagd"]);
        assert_eq!(nvdb(&f, &one_way, Mode::Car, Avoid::default()), Ways { along: Some(50.0), against: None });
        // A cycle path: not for cars, both ways by bicycle.
        let path = row(vec!["2", "", "", "", "", "", "", "Cykelbana", ""]);
        assert_eq!(nvdb(&f, &path, Mode::Car, Avoid::default()), Ways::NONE);
        assert_eq!(nvdb(&f, &path, Mode::Bike, Avoid::default()), Ways { along: Some(18.0), against: Some(18.0) });
        // A gravel road, avoided when asked.
        let gravel = row(vec!["3", "70", "70", "", "", "", "", "", "Grus"]);
        assert_eq!(nvdb(&f, &gravel, Mode::Car, Avoid { unpaved: true, ..Avoid::default() }), Ways::NONE);
    }
}
