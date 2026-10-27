//! Whole workspaces through the engine, without GDAL. Only CSV in and out.

use og_engine::{run, Level, Status, Table, Value, Workspace};
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gb-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn col(t: &Table, name: &str) -> Vec<Value> {
    let i = t.field_index(name).unwrap_or_else(|| panic!("no {} among {:?}", name, t.field_names()));
    t.features.iter().map(|f| f.attrs[i].clone()).collect()
}
fn text(s: &str) -> Value {
    Value::Text(s.to_string())
}
fn texts(v: &[&str]) -> Vec<Value> {
    v.iter().map(|s| text(s)).collect()
}
fn ints(v: &[i64]) -> Vec<Value> {
    v.iter().map(|i| Value::Int(*i)).collect()
}
fn reals(v: &[f64]) -> Vec<Value> {
    v.iter().map(|r| Value::Real(*r)).collect()
}
fn show(r: &og_engine::RunResult) {
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
}

const TOWNS: &str = "code;name;municipality;population;e;n\n\
1480;Göteborg;Göteborg;600000;319758;6400326\n\
1490;Borås;Borås;75000;377605;6399600\n\
1463;Kinna;Mark;16000;362270;6376250\n\
1466;Herrljunga;Herrljunga;4000;383560;6439160\n\
0114;Upplands Väsby;Upplands Väsby;48000;666100;6600500\n";

#[test]
fn test_filter_calculate_join_write() {
    let dir = tmp("join");
    std::fs::write(dir.join("towns.csv"), TOWNS).unwrap();
    std::fs::write(
        dir.join("municipalities.csv"),
        "muni_code;county;area_km2\n1480;Västra Götaland;447,8\n1490;Västra Götaland;915,2\n1463;Västra Götaland;935,7\n0114;Stockholm;75,4\n0114;Duplicate;1\n9999;Nowhere;0\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "format": "ographis/1", "name": "test",
      "nodes": [
        {"id": "t", "type": "Reader", "name": "Towns", "params": {"path": "towns.csv", "crs": "EPSG:3006"}},
        {"id": "m", "type": "Reader", "name": "Municipalities", "params": {"path": "municipalities.csv"}},
        {"id": "big", "type": "Tester", "params": {"test": "population >= 10000 AND municipality <> 'Mark'"}},
        {"id": "calc", "type": "AttributeCreator", "params": {"name": "thousands", "expression": "round(population / 1000, 1)"}},
        {"id": "join", "type": "FeatureMerger", "params": {"requestor_key": "code", "supplier_key": "muni_code"}},
        {"id": "dens", "type": "AttributeCreator", "params": {"name": "pop_per_km2", "expression": "round(population / area_km2)"}},
        {"id": "am", "type": "AttributeManager", "params": {"keep": "code, name, pop_per_km2, county", "rename": {"county": "län"}}},
        {"id": "out", "type": "Writer", "name": "Out", "params": {"path": "out/result.csv", "decimal_comma": "true", "geometry": "auto"}}
      ],
      "edges": [
        {"from": ["t", "Output"], "to": ["big", "Input"]},
        {"from": ["big", "Passed"], "to": ["calc", "Input"]},
        {"from": ["calc", "Output"], "to": ["join", "Requestor"]},
        {"from": ["m", "Output"], "to": ["join", "Supplier"]},
        {"from": ["join", "Merged"], "to": ["dens", "Input"]},
        {"from": ["dens", "Output"], "to": ["am", "Input"]},
        {"from": ["am", "Output"], "to": ["out", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);

    // The Tester: Kinna fails on the municipality, Herrljunga on the population.
    assert_eq!(col(r.output("big", "Passed").unwrap(), "name"), texts(&["Göteborg", "Borås", "Upplands Väsby"]));
    assert_eq!(col(r.output("big", "Failed").unwrap(), "name"), texts(&["Kinna", "Herrljunga"]));
    assert_eq!(col(r.output("calc", "Output").unwrap(), "thousands"), reals(&[600.0, 75.0, 48.0]));

    // The join: the text 1480 meets the text 1480, and 0114 meets 0114. The code column
    // became text because 0114 has a leading zero, and so did muni_code.
    let merged = r.output("join", "Merged").unwrap();
    assert_eq!(merged.len(), 3);
    assert_eq!(col(merged, "county"), texts(&["Västra Götaland", "Västra Götaland", "Stockholm"]));
    assert_eq!(r.output("join", "UnmergedRequestor").unwrap().len(), 0);
    assert_eq!(col(r.output("join", "UnusedSupplier").unwrap(), "muni_code"), texts(&["1463", "0114", "9999"]));
    assert_eq!(r.nodes["join"].status, Status::Warn);
    assert!(r.nodes["join"].warnings[0].contains("already exists"));

    assert_eq!(col(r.output("dens", "Output").unwrap(), "pop_per_km2"), ints(&[1340, 82, 637]));
    // A renamed attribute may have non-ASCII letters in its name.
    let am = r.output("am", "Output").unwrap();
    assert_eq!(am.field_names(), ["code", "name", "pop_per_km2", "län"]);

    let written = std::fs::read_to_string(dir.join("out/result.csv")).unwrap();
    assert_eq!(
        written,
        "code;name;pop_per_km2;län;x;y\n1480;Göteborg;1340;Västra Götaland;319758;6400326\n1490;Borås;82;Västra Götaland;377605;6399600\n0114;Upplands Väsby;637;Stockholm;666100;6600500\n"
    );
    assert_eq!(r.nodes["out"].files, [dir.join("out/result.csv")]);
    // The log is in English, with a decimal point.
    let last = &r.log.last().unwrap().msg;
    assert!(last.starts_with("Run finished: 8 nodes, 1 warning, ") && last.ends_with(" s"), "{}", last);
    let time = last.rsplit(", ").next().unwrap().trim_end_matches(" s");
    assert!(time.contains('.') && time.parse::<f64>().is_ok(), "{}", time);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clip_and_measure() {
    let dir = tmp("clip");
    std::fs::write(dir.join("areas.csv"), "name;wkt\nsquare;\"POLYGON ((0 0, 1000 0, 1000 1000, 0 1000, 0 0))\"\n").unwrap();
    std::fs::write(
        dir.join("mixed.csv"),
        "id;wkt\n1;POINT (500 500)\n2;POINT (1500 500)\n3;\"LINESTRING (-500 500, 1500 500)\"\n4;\"POLYGON ((500 500, 1500 500, 1500 800, 500 800, 500 500))\"\n5;\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "a", "type": "Reader", "params": {"path": "areas.csv", "crs": 3006}},
        {"id": "m", "type": "Reader", "params": {"path": "mixed.csv", "crs": 3006}},
        {"id": "c", "type": "Clipper"},
        {"id": "area", "type": "AreaCalculator", "params": {"unit": "ha"}},
        {"id": "len", "type": "LengthCalculator", "params": {"name": "length_km", "unit": "km"}},
        {"id": "e", "type": "AttributeCreator", "params": {"name": "by_expression", "expression": "$length"}},
        {"id": "types", "type": "GeometryFilter"},
        {"id": "out", "type": "Writer", "params": {"path": "inside.csv", "geometry": "auto"}}
      ],
      "edges": [
        {"from": ["a", "Output"], "to": ["c", "Clipper"]},
        {"from": ["m", "Output"], "to": ["c", "Clippee"]},
        {"from": ["c", "Inside"], "to": ["area", "Input"]},
        {"from": ["area", "Output"], "to": ["len", "Input"]},
        {"from": ["len", "Output"], "to": ["e", "Input"]},
        {"from": ["e", "Output"], "to": ["out", "Input"]},
        {"from": ["c", "Outside"], "to": ["types", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    let inside = r.output("c", "Inside").unwrap();
    let outside = r.output("c", "Outside").unwrap();
    assert_eq!(col(inside, "id"), ints(&[1, 3, 4]));
    // Feature 5 has no geometry and goes to Outside together with the leftovers.
    assert_eq!(col(outside, "id"), ints(&[2, 3, 4, 5]));

    let out = r.output("e", "Output").unwrap();
    // The point has no area, the line has 1 km inside, the area is 500 × 300 m = 15 ha.
    assert_eq!(col(out, "_area"), reals(&[0.0, 0.0, 15.0]));
    assert_eq!(col(out, "length_km"), reals(&[0.0, 1.0, 1.6]));
    assert_eq!(col(out, "by_expression"), reals(&[0.0, 1000.0, 1600.0]));
    let csv = std::fs::read_to_string(dir.join("inside.csv")).unwrap();
    assert!(csv.starts_with("id;_area;length_km;by_expression;wkt\n1;0;0;0;POINT(500 500)\n"), "{}", csv);

    // The GeometryFilter sorts the leftovers by type, and what has no geometry goes to Other.
    assert_eq!(col(r.output("types", "Point").unwrap(), "id"), ints(&[2]));
    assert_eq!(col(r.output("types", "Line").unwrap(), "id"), ints(&[3]));
    assert_eq!(col(r.output("types", "Area").unwrap(), "id"), ints(&[4]));
    assert_eq!(col(r.output("types", "Other").unwrap(), "id"), ints(&[5]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn errors_stay_local_and_messages_are_readable() {
    let dir = tmp("errors");
    std::fs::write(dir.join("a.csv"), "x_attr;v\n1;a\n2;b\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "ok", "type": "Reader", "name": "Exists", "params": {"path": "a.csv"}},
        {"id": "missing", "type": "Reader", "name": "Missing", "params": {"path": "not_there.csv"}},
        {"id": "t1", "type": "Tester", "name": "Misspelt", "params": {"test": "xattr > 1"}},
        {"id": "t2", "type": "Tester", "name": "Downstream", "params": {"test": "v = 'a'"}},
        {"id": "t3", "type": "Tester", "name": "Works", "params": {"test": "x_attr > 1"}},
        {"id": "s1", "type": "Tester", "name": "Loop 1", "params": {"test": "true"}},
        {"id": "s2", "type": "Tester", "name": "Loop 2", "params": {"test": "true"}},
        {"id": "unknown", "type": "Teleporter"},
        {"id": "port", "type": "Tester", "name": "Wrong port", "params": {"test": "true"}},
        {"id": "gpkg", "type": "Reader", "name": "No GDAL", "params": {"path": "a.gpkg"}}
      ],
      "edges": [
        {"from": ["ok", "Output"], "to": ["t1", "Input"]},
        {"from": ["t1", "Passed"], "to": ["t2", "Input"]},
        {"from": ["missing", "Output"], "to": ["t2", "Input"]},
        {"from": ["ok", "Output"], "to": ["t3", "Input"]},
        {"from": ["s1", "Passed"], "to": ["s2", "Input"]},
        {"from": ["s2", "Passed"], "to": ["s1", "Input"]},
        {"from": ["ok", "Exit"], "to": ["port", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    std::fs::write(dir.join("a.gpkg"), "x").unwrap();
    let r = run(&ws, None, &dir);
    let errors: Vec<&str> = r.log.iter().filter(|l| l.level == Level::Error).map(|l| l.msg.as_str()).collect();
    for e in &errors {
        println!("{}", e);
    }
    assert_eq!(r.nodes["ok"].status, Status::Ok);
    assert_eq!(r.nodes["t3"].status, Status::Ok);
    assert_eq!(r.output("t3", "Passed").unwrap().len(), 1);
    assert!(r.nodes["missing"].error.as_ref().unwrap().contains("The file does not exist"));
    assert!(r.nodes["t1"].error.as_ref().unwrap().contains("There is no attribute \"xattr\". Available: x_attr, v."));
    assert_eq!(r.nodes["t2"].status, Status::Skipped);
    assert!(r.nodes["s1"].error.as_ref().unwrap().contains("loop"));
    assert!(r.nodes["s2"].error.as_ref().unwrap().contains("loop"));
    assert!(r.nodes["unknown"].error.as_ref().unwrap().contains("Unknown node type: Teleporter"));
    assert!(r.nodes["port"].error.as_ref().unwrap().contains("There is no output port Exit on Exists"));
    assert!(r.nodes["gpkg"].error.as_ref().unwrap().contains("needs the GDAL support"));
    assert_eq!(r.errors, 7);
    assert!(r.log.last().unwrap().msg.starts_with("Run ended with 7 errors: 10 nodes"));
    assert!(Workspace::from_json("{\"nodes\": [{\"id\": \"a\", \"type\": \"Tester\"}, {\"id\": \"a\", \"type\": \"Tester\"}]}").is_err());
    assert!(Workspace::from_json("not json").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_streams_into_one_port_are_merged() {
    let dir = tmp("concat");
    std::fs::write(dir.join("a.csv"), "id;name;v\n1;a;10\n").unwrap();
    std::fs::write(dir.join("b.csv"), "id;v;extra\n2;2,5;x\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "a", "type": "Reader", "params": {"path": "a.csv"}},
        {"id": "b", "type": "Reader", "params": {"path": "b.csv"}},
        {"id": "m", "type": "AttributeManager"}
      ],
      "edges": [
        {"from": ["a", "Output"], "to": ["m", "Input"]},
        {"from": ["b", "Output"], "to": ["m", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    assert_eq!(r.errors, 0);
    let out = r.output("m", "Output").unwrap();
    assert_eq!(out.field_names(), ["id", "name", "v", "extra"]);
    // Integers and decimals in the same column become decimals, missing values null.
    assert_eq!(col(out, "v"), reals(&[10.0, 2.5]));
    assert_eq!(col(out, "name"), [text("a"), Value::Null]);
    assert_eq!(col(out, "extra"), [Value::Null, text("x")]);
    // A workspace that is saved and read again is unchanged.
    let again = Workspace::from_json(&ws.to_json()).unwrap();
    assert_eq!(again.edges, ws.edges);
    assert_eq!(again.format, "ographis/1");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn workspaces_from_before_the_rename_are_still_read() {
    let ws = Workspace::from_json(r#"{"format": "flodesbanken/1", "name": "old", "nodes": [{"id": "a", "type": "Tester"}]}"#).unwrap();
    // It is saved in the new format from then on.
    assert_eq!(ws.format, "ographis/1");
    assert!(ws.to_json().contains("\"ographis/1\""));
    let e = Workspace::from_json(r#"{"format": "something/9", "nodes": []}"#).unwrap_err();
    assert!(e.0.contains("Unknown format: something/9"), "{}", e.0);
}

#[test]
fn sort_deduplicate_and_statistics() {
    let dir = tmp("attrs");
    std::fs::write(
        dir.join("towns.csv"),
        "name;county;population;note\nUddevalla;VG;36000;x\nborås;VG;75000;\nAlingsås;VG;28000;x\nHalmstad;Halland;72000;x\nVarberg;Halland;;x\nBorås;VG;75000;again\nKungsbacka;Halland;many;x\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "towns.csv"}},
        {"id": "by_name", "type": "Sorter", "params": {"sort_by": "name"}},
        {"id": "by_pop", "type": "Sorter", "params": {"sort_by": "county, population", "order": "descending"}},
        {"id": "dup", "type": "DuplicateFilter", "params": {"key": "county, population"}},
        {"id": "dup_all", "type": "DuplicateFilter"},
        {"id": "stats", "type": "StatisticsCalculator", "params": {"attributes": "population", "group_by": "county"}},
        {"id": "count", "type": "StatisticsCalculator"}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["by_name", "Input"]},
        {"from": ["in", "Output"], "to": ["by_pop", "Input"]},
        {"from": ["in", "Output"], "to": ["dup", "Input"]},
        {"from": ["in", "Output"], "to": ["dup_all", "Input"]},
        {"from": ["in", "Output"], "to": ["stats", "Input"]},
        {"from": ["in", "Output"], "to": ["count", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);

    // Text sorts without regard to case, so "borås" does not end up after everything in
    // upper case. Of the two, upper case comes first.
    assert_eq!(col(r.output("by_name", "Sorted").unwrap(), "name"), texts(&["Alingsås", "Borås", "borås", "Halmstad", "Kungsbacka", "Uddevalla", "Varberg"]));
    // The population column is text here, because of "many". Descending by county, then
    // by population as text, with the empty value last within its county.
    let by_pop = r.output("by_pop", "Sorted").unwrap();
    assert_eq!(col(by_pop, "county"), texts(&["VG", "VG", "VG", "VG", "Halland", "Halland", "Halland"]));
    assert_eq!(col(by_pop, "name"), texts(&["borås", "Borås", "Uddevalla", "Alingsås", "Kungsbacka", "Halmstad", "Varberg"]));

    // Same county and population: the second Borås is a duplicate. With all attributes
    // compared it is not, because the name and the note differ.
    assert_eq!(col(r.output("dup", "Duplicate").unwrap(), "name"), texts(&["Borås"]));
    assert_eq!(r.output("dup", "Unique").unwrap().len(), 6);
    assert_eq!(r.output("dup_all", "Duplicate").unwrap().len(), 0);

    // Statistics per county. The empty value is skipped silently, "many" with a warning.
    let s = r.output("stats", "Summary").unwrap();
    assert_eq!(s.field_names(), ["county", "_count", "population_sum", "population_min", "population_max", "population_mean", "population_median"]);
    assert_eq!(col(s, "county"), texts(&["VG", "Halland"]));
    assert_eq!(col(s, "_count"), ints(&[4, 3]));
    assert_eq!(col(s, "population_sum"), reals(&[214000.0, 72000.0]));
    assert_eq!(col(s, "population_min"), reals(&[28000.0, 72000.0]));
    assert_eq!(col(s, "population_max"), reals(&[75000.0, 72000.0]));
    assert_eq!(col(s, "population_mean"), reals(&[53500.0, 72000.0]));
    assert_eq!(col(s, "population_median"), reals(&[55500.0, 72000.0]));
    assert_eq!(s.crs, None);
    assert!(r.nodes["stats"].warnings[0].starts_with("1 values are not numbers"), "{:?}", r.nodes["stats"].warnings);
    // Complete: every feature, with the figures of its group.
    let c = r.output("stats", "Complete").unwrap();
    assert_eq!(c.len(), 7);
    assert_eq!(col(c, "population_max"), reals(&[75000.0, 75000.0, 75000.0, 72000.0, 72000.0, 75000.0, 72000.0]));
    // Without attributes and groups it simply counts.
    let n = r.output("count", "Summary").unwrap();
    assert_eq!(n.field_names(), ["_count"]);
    assert_eq!(col(n, "_count"), ints(&[7]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn buffer_dissolve_overlay_center_and_generalize() {
    let dir = tmp("geometry");
    // Two squares that share an edge and belong to the same zone, and one apart in another.
    std::fs::write(
        dir.join("areas.csv"),
        "zone;part;wkt\nA;1;\"POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0))\"\nA;2;\"POLYGON ((100 0, 200 0, 200 100, 100 100, 100 0))\"\nB;3;\"POLYGON ((500 0, 600 0, 600 100, 500 100, 500 0))\"\n",
    )
    .unwrap();
    // One point in each square of zone A, one on their shared edge, one outside everything.
    std::fs::write(dir.join("points.csv"), "id;zone;x;y\n1;mine;50;50\n2;mine;150;50\n3;mine;100;50\n4;mine;300;300\n").unwrap();
    std::fs::write(dir.join("line.csv"), "id;width;wkt\n1;10;\"LINESTRING (0 0, 50 0.5, 100 0, 100 100)\"\n2;;\"LINESTRING (0 0, 10 0)\"\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "areas", "type": "Reader", "params": {"path": "areas.csv", "crs": 3006}},
        {"id": "points", "type": "Reader", "params": {"path": "points.csv", "crs": 3006}},
        {"id": "line", "type": "Reader", "params": {"path": "line.csv", "crs": 3006}},
        {"id": "dissolve", "type": "Dissolver", "params": {"group_by": "zone"}},
        {"id": "all", "type": "Dissolver"},
        {"id": "overlay", "type": "PointOnAreaOverlayer"},
        {"id": "buffer", "type": "Bufferer", "params": {"distance": "width / 2"}},
        {"id": "shrink", "type": "Bufferer", "params": {"distance": "-10"}},
        {"id": "center", "type": "CenterPointReplacer", "params": {"mode": "bbox"}},
        {"id": "simple", "type": "Generalizer", "params": {"tolerance": "1"}},
        {"id": "measure", "type": "AreaCalculator"}
      ],
      "edges": [
        {"from": ["areas", "Output"], "to": ["dissolve", "Input"]},
        {"from": ["areas", "Output"], "to": ["all", "Input"]},
        {"from": ["line", "Output"], "to": ["all", "Input"]},
        {"from": ["points", "Output"], "to": ["overlay", "Point"]},
        {"from": ["areas", "Output"], "to": ["overlay", "Area"]},
        {"from": ["line", "Output"], "to": ["buffer", "Input"]},
        {"from": ["areas", "Output"], "to": ["shrink", "Input"]},
        {"from": ["shrink", "Buffered"], "to": ["measure", "Input"]},
        {"from": ["dissolve", "Dissolved"], "to": ["center", "Input"]},
        {"from": ["line", "Output"], "to": ["simple", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);

    // Dissolver: zone A becomes one area of 200 × 100, zone B stays as it is.
    let d = r.output("dissolve", "Dissolved").unwrap();
    assert_eq!(d.field_names(), ["zone", "_count"]);
    assert_eq!(col(d, "zone"), texts(&["A", "B"]));
    assert_eq!(col(d, "_count"), ints(&[2, 1]));
    let areas: Vec<f64> = d.features.iter().map(|f| og_engine::geom::area(f.geom.as_ref().unwrap(), false)).collect();
    assert_eq!(areas, [20000.0, 10000.0]);
    assert!(matches!(d.features[0].geom, Some(geo::Geometry::Polygon(_))));
    // Without groups everything is merged, and the lines go to Remnants.
    let all = r.output("all", "Dissolved").unwrap();
    assert_eq!(col(all, "_count"), ints(&[3]));
    assert_eq!(og_engine::geom::area(all.features[0].geom.as_ref().unwrap(), false), 30000.0);
    assert_eq!(r.output("all", "Remnants").unwrap().len(), 2);

    // Overlayer: the point's own "zone" attribute wins over the area's, "part" is new.
    let p = r.output("overlay", "Point").unwrap();
    assert_eq!(p.field_names(), ["id", "zone", "x", "y", "part", "_overlaps"]);
    assert_eq!(col(p, "zone"), texts(&["mine", "mine", "mine", "mine"]));
    // Point 3 is on the shared edge and matches both squares. It gets the first one.
    assert_eq!(col(p, "part"), [Value::Int(1), Value::Int(2), Value::Int(1), Value::Null]);
    assert_eq!(col(p, "_overlaps"), ints(&[1, 1, 2, 0]));
    assert!(r.nodes["overlay"].warnings[0].starts_with("1 points lie in more than one area"));
    let a = r.output("overlay", "Area").unwrap();
    assert_eq!(col(a, "_overlaps"), ints(&[2, 2, 0]));
    assert_eq!(col(a, "part"), ints(&[1, 2, 3]));

    // Bufferer: the distance comes from an attribute. The line without a width passes unchanged.
    let b = r.output("buffer", "Buffered").unwrap();
    assert!(matches!(b.features[0].geom, Some(geo::Geometry::Polygon(_))));
    let corridor = og_engine::geom::area(b.features[0].geom.as_ref().unwrap(), false);
    // About 200 m long and 10 m wide, plus the two round ends.
    assert!(corridor > 2000.0 && corridor < 2100.0, "{}", corridor);
    assert!(matches!(b.features[1].geom, Some(geo::Geometry::LineString(_))));
    assert!(r.nodes["buffer"].warnings[0].starts_with("1 features have no distance"));
    // Shrinking a 100 m square by 10 m leaves 80 × 80.
    assert_eq!(col(r.output("measure", "Output").unwrap(), "_area"), reals(&[6400.0, 6400.0, 6400.0]));

    // CenterPointReplacer: the bounding box centres of the dissolved zones.
    let c = r.output("center", "Point").unwrap();
    let xy: Vec<(f64, f64)> = c.features.iter().map(|f| if let Some(geo::Geometry::Point(p)) = &f.geom { (p.x(), p.y()) } else { panic!() }).collect();
    assert_eq!(xy, [(100.0, 50.0), (550.0, 50.0)]);

    // Generalizer: the vertex 0.5 m off the line goes, the corner stays.
    let g = r.output("simple", "Generalized").unwrap();
    assert_eq!(og_engine::geom::vertex_count(g.features[0].geom.as_ref().unwrap()), 3);
    assert!(r.log.iter().any(|l| l.msg.contains("6 vertices became 5")), "the log should say how many vertices were removed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn distances_in_degrees_are_refused() {
    let dir = tmp("degrees");
    std::fs::write(dir.join("p.csv"), "id;lat;lon\n1;57,7;11,97\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "p.csv"}},
        {"id": "buffer", "type": "Bufferer", "params": {"distance": "100"}},
        {"id": "sort", "type": "Sorter"}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["buffer", "Input"]},
        {"from": ["in", "Output"], "to": ["sort", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    let e = r.nodes["buffer"].error.as_ref().unwrap();
    assert!(e.contains("works in metres") && e.contains("WGS 84") && e.contains("Put Reproject before it"), "{}", e);
    assert!(r.nodes["sort"].error.as_ref().unwrap().contains("Choose at least one attribute"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_of_files_is_one_reader() {
    let dir = tmp("folder");
    std::fs::create_dir_all(dir.join("data/deeper")).unwrap();
    std::fs::write(dir.join("data/b_south.csv"), "id;name;e;n\n3;Kinna;362270;6376250\n").unwrap();
    std::fs::write(dir.join("data/A_north.CSV"), "id;name;e;n\n1;Skara;400000;6470000\n2;Lidköping;395000;6485000\n").unwrap();
    // Another column and no "name": the schemas are united.
    std::fs::write(dir.join("data/c_east.csv"), "id;e;n;note\n4;430000;6400000;new\n").unwrap();
    std::fs::write(dir.join("data/readme.txt"), "not data\n").unwrap();
    std::fs::write(dir.join("data/deeper/d_west.csv"), "id;name;e;n\n5;Öckerö;300000;6400000\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "all", "type": "Reader", "params": {"path": "data", "pattern": "*.csv", "crs": "EPSG:3006", "filename_attr": "source"}},
        {"id": "deep", "type": "Reader", "params": {"path": "data", "pattern": "*.csv", "recursive": "true", "crs": "EPSG:3006", "filename_attr": "source", "filename_part": "name"}},
        {"id": "wild", "type": "Reader", "params": {"path": "data/?_*.csv", "crs": "EPSG:3006"}},
        {"id": "default", "type": "Reader", "params": {"path": "data"}},
        {"id": "one", "type": "Reader", "params": {"path": "data/b_south.csv", "crs": "EPSG:3006", "filename_attr": "source", "filename_part": "path"}},
        {"id": "bad", "type": "Reader", "params": {"path": "data", "pattern": "*.csv;*.txt", "crs": "EPSG:3006", "x": "e", "y": "n"}}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);

    // The files come in name order, whatever the case of the letters.
    let all = r.output("all", "Output").unwrap();
    assert_eq!(col(all, "source"), texts(&["A_north", "A_north", "b_south", "c_east"]));
    assert_eq!(col(all, "id"), ints(&[1, 2, 3, 4]));
    assert_eq!(all.field_names(), ["id", "name", "e", "n", "source", "note"]);
    assert_eq!(col(all, "note"), [Value::Null, Value::Null, Value::Null, text("new")]);
    assert_eq!(all.crs, Some(3006));
    assert_eq!(r.nodes["all"].sources.len(), 3);
    assert!(r.log.iter().any(|l| l.msg.starts_with("Input: 4 out") && l.msg.contains("3 files")), "the log should say how many files were read");

    assert_eq!(col(r.output("deep", "Output").unwrap(), "source"), texts(&["A_north.CSV", "A_north.CSV", "b_south.csv", "c_east.csv", "d_west.csv"]));
    assert_eq!(r.output("wild", "Output").unwrap().len(), 4);
    // Without a filter a folder is searched for Shapefiles, and here there are none.
    let e = r.nodes["default"].error.as_ref().unwrap();
    assert!(e.starts_with("No files match *.shp in "), "{}", e);
    // One file works as before, and the full path can be had as well.
    let one = r.output("one", "Output").unwrap();
    assert_eq!(col(one, "source"), [text(&dir.join("data/b_south.csv").to_string_lossy())]);
    assert_eq!(r.nodes["one"].sources, [dir.join("data/b_south.csv")]);
    // An error in one of many files says which file.
    let e = r.nodes["bad"].error.as_ref().unwrap();
    assert_eq!(e, "readme.txt: There is no column \"e\". Columns: not data.");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_name_patterns() {
    use og_engine::nodes::wildcard_match as m;
    assert!(m("*.shp", "roads.shp") && m("*.shp", "ROADS.SHP") && m("*.SHP", "roads.shp"));
    assert!(!m("*.shp", "roads.shx") && !m("*.shp", "roads.shp.xml"));
    assert!(m("roads_????.shp", "roads_2024.shp") && !m("roads_????.shp", "roads_24.shp"));
    assert!(m("*väg*", "Allmän_Väg_1.gpkg") && m("*", "") && !m("?", ""));
}

#[test]
fn stamp_cut_join_and_replace_text() {
    let dir = tmp("strings");
    std::fs::write(dir.join("t.csv"), "code;name;part;size\n0114;Upplands Väsby;;12,5\n1480;Göteborg;A;7\n;;B;\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "date", "type": "DateTimeStamper"},
        {"id": "time", "type": "DateTimeStamper", "params": {"value": "time"}},
        {"id": "both", "type": "DateTimeStamper", "params": {"value": "datetime", "name": "read_at"}},
        {"id": "first", "type": "SubstringExtractor", "params": {"attribute": "code", "part": "first", "count": 2, "name": "county"}},
        {"id": "last", "type": "SubstringExtractor", "params": {"attribute": "name", "part": "last", "count": 5}},
        {"id": "mid", "type": "SubstringExtractor", "params": {"attribute": "name", "part": "middle", "start": 2, "count": 3, "name": "name"}},
        {"id": "join", "type": "StringConcatenator", "params": {"attributes": ["code", "name", "part"], "separator": " - ", "name": "label"}},
        {"id": "join_all", "type": "StringConcatenator", "params": {"attributes": "size,part", "separator": "_", "skip_empty": "false"}},
        {"id": "a_to_b", "type": "StringReplacer", "params": {"attributes": ["part", "name"], "find": "A", "replace": "B"}},
        {"id": "no_spaces", "type": "StringReplacer", "params": {"attributes": ["name", "size"], "find": " ", "replace": ""}},
        {"id": "any_case", "type": "StringReplacer", "params": {"attributes": ["name"], "find": "ÖTE", "replace": "*", "match_case": "false"}},
        {"id": "digits", "type": "StringReplacer", "params": {"attributes": ["size"], "find": "7", "replace": "seven"}},
        {"id": "nothing", "type": "StringReplacer", "params": {"attributes": ["name"]}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["date", "Input"]},
        {"from": ["date", "Output"], "to": ["time", "Input"]},
        {"from": ["time", "Output"], "to": ["both", "Input"]},
        {"from": ["in", "Output"], "to": ["first", "Input"]},
        {"from": ["first", "Output"], "to": ["last", "Input"]},
        {"from": ["last", "Output"], "to": ["mid", "Input"]},
        {"from": ["in", "Output"], "to": ["join", "Input"]},
        {"from": ["in", "Output"], "to": ["join_all", "Input"]},
        {"from": ["in", "Output"], "to": ["a_to_b", "Input"]},
        {"from": ["in", "Output"], "to": ["no_spaces", "Input"]},
        {"from": ["in", "Output"], "to": ["any_case", "Input"]},
        {"from": ["in", "Output"], "to": ["digits", "Input"]},
        {"from": ["in", "Output"], "to": ["nothing", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let before = chrono::Local::now().naive_local();
    let r = run(&ws, None, &dir);
    let after = chrono::Local::now().naive_local();
    show(&r);

    // The stamps: three attributes with one and the same moment, taken when the run started.
    let s = r.output("both", "Output").unwrap();
    assert_eq!(s.field_names(), ["code", "name", "part", "size", "_date", "_time", "read_at"]);
    let (date, time, both) = (col(s, "_date")[0].to_text(), col(s, "_time")[0].to_text(), col(s, "read_at")[0].to_text());
    assert_eq!(format!("{} {}", date, time), both);
    let stamp = chrono::NaiveDateTime::parse_from_str(&both, "%Y-%m-%d %H:%M:%S").unwrap();
    assert!(stamp >= before - chrono::Duration::seconds(1) && stamp <= after, "{} is not between {} and {}", stamp, before, after);
    assert!(col(s, "read_at").iter().all(|v| v.to_text() == both));

    // Parts of a text. An empty value stays empty, and å, ä, ö count as one character each.
    let t = r.output("mid", "Output").unwrap();
    assert_eq!(col(t, "county"), [text("01"), text("14"), Value::Null]);
    assert_eq!(col(t, "_substring"), [text("Väsby"), text("eborg"), Value::Null]);
    // The same name as the source overwrites it.
    assert_eq!(col(t, "name"), [text("ppl"), text("öte"), Value::Null]);
    assert_eq!(t.field_names(), ["code", "name", "part", "size", "county", "_substring"]);

    // Joined, with and without the empty values.
    assert_eq!(col(r.output("join", "Output").unwrap(), "label"), texts(&["0114 - Upplands Väsby", "1480 - Göteborg - A", "B"]));
    assert_eq!(col(r.output("join_all", "Output").unwrap(), "_concatenated"), texts(&["12.5_", "7_A", "_B"]));

    // Replaced. Only what matches changes, and a number that is not touched stays a number.
    let a = r.output("a_to_b", "Output").unwrap();
    assert_eq!(col(a, "part"), [Value::Null, text("B"), text("B")]);
    assert_eq!(col(a, "name"), [text("Upplands Väsby"), text("Göteborg"), Value::Null]);
    assert!(r.log.iter().any(|l| l.msg.contains("1 values changed")), "the log should say how many values were changed");
    let n = r.output("no_spaces", "Output").unwrap();
    assert_eq!(col(n, "name"), [text("UpplandsVäsby"), text("Göteborg"), Value::Null]);
    assert_eq!(col(n, "size"), [Value::Real(12.5), Value::Real(7.0), Value::Null]);
    assert_eq!(col(r.output("any_case", "Output").unwrap(), "name"), [text("Upplands Väsby"), text("G*borg"), Value::Null]);
    // When a number does change, the whole attribute becomes text.
    assert_eq!(col(r.output("digits", "Output").unwrap(), "size"), [text("12.5"), text("seven"), Value::Null]);
    assert_eq!(r.nodes["nothing"].error.as_deref(), Some("Give the text to find."));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn progress_is_reported_node_by_node() {
    let dir = tmp("progress");
    std::fs::write(dir.join("t.csv"), "id;v\n1;5\n2;50\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "big", "type": "Tester", "params": {"test": "v > 10"}},
        {"id": "broken", "type": "Tester", "params": {"test": "nothing > 10"}},
        {"id": "after", "type": "Sorter", "params": {"sort_by": "id"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["big", "Input"]},
        {"from": ["in", "Output"], "to": ["broken", "Input"]},
        {"from": ["broken", "Passed"], "to": ["after", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let mut events: Vec<String> = Vec::new();
    let r = og_engine::run_with(&ws, None, &dir, &mut |p| {
        events.push(match p {
            og_engine::Progress::Started { id } => format!("start {}", id),
            og_engine::Progress::Finished { id, result } => {
                let mut counts: Vec<String> = result.outputs.iter().map(|(port, t)| format!("{}={}", port, t.len())).collect();
                counts.sort();
                format!("end {} {:?} {}", id, result.status, counts.join(","))
            }
            og_engine::Progress::Log { entry } => format!("log {}", entry.msg),
        });
    });
    // The lines of the log come too, each once, and in the same order as in the result.
    let lines: Vec<String> = events.iter().filter_map(|e| e.strip_prefix("log ")).map(str::to_string).collect();
    let all: Vec<String> = r.log.iter().map(|l| l.msg.clone()).collect();
    assert_eq!(lines, all);
    events.retain(|e| !e.starts_with("log "));
    // Every node is reported, in the order they run, and a node is done before the next starts.
    let ends: Vec<&String> = events.iter().filter(|e| e.starts_with("end ")).collect();
    assert_eq!(ends.len(), 4, "{:?}", events);
    assert_eq!(events[0], "start in");
    assert_eq!(events[1], "end in Ok Output=2");
    assert!(events.contains(&"end big Ok Failed=1,Passed=1".to_string()), "{:?}", events);
    assert!(events.iter().any(|e| e.starts_with("end broken Error")), "{:?}", events);
    assert!(events.iter().any(|e| e.starts_with("end after Skipped")), "{:?}", events);
    for pair in events.chunks(2).filter(|p| p[0].starts_with("start ")) {
        assert_eq!(pair[1].split(' ').nth(1), pair[0].split(' ').nth(1), "{:?}", events);
    }
    assert_eq!(r.nodes.len(), 4);
    assert_eq!(r.nodes["after"].status, Status::Skipped);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn groups_notes_and_comments_are_kept_in_the_file() {
    let json = r#"{
      "format": "ographis/1", "name": "kept",
      "nodes": [
        {"id": "in", "type": "Reader", "comment": "Updated every Monday", "params": {"path": "t.csv"}},
        {"id": "sort", "type": "Sorter", "params": {"sort_by": "id"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["sort", "Input"]}],
      "groups": [
        {"name": "Input", "x": 10, "y": 20, "w": 300, "h": 200, "color": "green"},
        {"name": "Loose", "x": 0, "y": 0, "w": 50, "h": 50, "locked": false}
      ],
      "notes": [{"text": "Remember to ask about the codes", "x": 400, "y": -30}]
    }"#;
    let ws = Workspace::from_json(json).unwrap();
    assert_eq!(ws.nodes[0].comment, "Updated every Monday");
    assert_eq!(ws.nodes[1].comment, "");
    assert_eq!((ws.groups.len(), ws.groups[0].name.as_str(), ws.groups[0].locked, ws.groups[1].locked), (2, "Input", true, false));
    assert_eq!(ws.notes[0].text, "Remember to ask about the codes");
    // Written and read again, they are the same.
    let again = Workspace::from_json(&ws.to_json()).unwrap();
    assert_eq!(again.groups, ws.groups);
    assert_eq!(again.notes, ws.notes);
    assert_eq!(again.nodes[0].comment, ws.nodes[0].comment);
    // A workspace without them does not mention them.
    let plain = Workspace::from_json(r#"{"nodes": [{"id": "in", "type": "Reader"}]}"#).unwrap();
    let text = plain.to_json();
    assert!(!text.contains("groups") && !text.contains("notes") && !text.contains("comment"), "{}", text);
}

#[test]
fn a_reader_can_stop_at_a_number_of_features() {
    let dir = tmp("limit");
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    let rows = |from: usize, to: usize| (from..to).map(|i| format!("{};row {}\n", i, i)).collect::<String>();
    std::fs::write(dir.join("all.csv"), format!("id;name\n{}", rows(0, 50))).unwrap();
    for (k, name) in ["a.csv", "b.csv", "c.csv"].iter().enumerate() {
        std::fs::write(dir.join("parts").join(name), format!("id;name\n{}", rows(k * 10, k * 10 + 10))).unwrap();
    }
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "all", "type": "Reader", "params": {"path": "all.csv"}},
        {"id": "some", "type": "Reader", "params": {"path": "all.csv", "limit": "20"}},
        {"id": "more", "type": "Reader", "params": {"path": "all.csv", "limit": 1000}},
        {"id": "folder", "type": "Reader", "params": {"path": "parts", "pattern": "*.csv", "limit": "15", "filename_attr": "file"}},
        {"id": "wrong", "type": "Reader", "params": {"path": "all.csv", "limit": "many"}},
        {"id": "zero", "type": "Reader", "params": {"path": "all.csv", "limit": "0"}}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // Without a limit everything is read.
    assert_eq!(r.output("all", "Output").unwrap().len(), 50);
    let some = r.output("some", "Output").unwrap();
    assert_eq!((some.len(), col(some, "id")[19].clone()), (20, Value::Int(19)));
    assert!(r.log.iter().any(|l| l.msg.starts_with("Input: 20 out") && l.msg.contains("stopped at the limit of 20 features")));
    // A limit above what there is changes nothing, and says nothing.
    assert_eq!(r.output("more", "Output").unwrap().len(), 50);
    assert!(!r.log.iter().any(|l| l.msg.contains("limit of 1000")));
    // The limit is for the reader as a whole: the third file is never opened.
    let folder = r.output("folder", "Output").unwrap();
    assert_eq!(folder.len(), 15);
    assert_eq!(col(folder, "file")[9..11], [text("a"), text("b")]);
    assert_eq!(col(folder, "id")[14], Value::Int(14));
    for id in ["wrong", "zero"] {
        assert!(r.nodes[id].error.as_ref().unwrap().starts_with("The largest number of features must be a whole number above zero"), "{:?}", r.nodes[id].error);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_disabled_node_lets_nothing_through() {
    let dir = tmp("disabled");
    std::fs::write(dir.join("a.csv"), "id;v\n1;5\n2;50\n").unwrap();
    std::fs::write(dir.join("b.csv"), "id;v\n3;7\n").unwrap();
    std::fs::write(dir.join("kept.csv"), "written earlier\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "a", "type": "Reader", "params": {"path": "a.csv"}},
        {"id": "b", "type": "Reader", "params": {"path": "b.csv"}},
        {"id": "off", "type": "Tester", "disabled": true, "params": {"test": "v > 10"}},
        {"id": "after", "type": "Sorter", "params": {"sort_by": "id"}},
        {"id": "kept", "type": "Writer", "params": {"path": "kept.csv"}},
        {"id": "both", "type": "Sorter", "params": {"sort_by": "id"}},
        {"id": "out", "type": "Writer", "params": {"path": "both.csv"}},
        {"id": "broken", "type": "Tester", "disabled": true, "params": {"test": "this is not ( an expression"}}
      ],
      "edges": [
        {"from": ["a", "Output"], "to": ["off", "Input"]},
        {"from": ["off", "Passed"], "to": ["after", "Input"]},
        {"from": ["after", "Sorted"], "to": ["kept", "Input"]},
        {"from": ["off", "Passed"], "to": ["both", "Input"]},
        {"from": ["b", "Output"], "to": ["both", "Input"]},
        {"from": ["both", "Sorted"], "to": ["out", "Input"]},
        {"from": ["a", "Output"], "to": ["broken", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    assert!(ws.nodes[2].disabled && !ws.nodes[0].disabled);
    let r = run(&ws, None, &dir);
    show(&r);
    // Being switched off is neither an error nor a warning, and a node that is off is
    // not even looked at.
    assert_eq!((r.errors, r.warnings), (0, 0));
    assert_eq!(r.nodes["off"].status, Status::Disabled);
    assert_eq!(r.nodes["broken"].status, Status::Disabled);
    assert!(r.nodes["off"].outputs.is_empty());
    // What is fed only from there rests, all the way down, and the file is left alone.
    assert_eq!(r.nodes["after"].status, Status::Skipped);
    assert_eq!(r.nodes["kept"].status, Status::Skipped);
    assert_eq!(r.nodes["after"].error.as_deref(), Some("Skipped because what feeds Input is disabled."));
    assert_eq!(std::fs::read_to_string(dir.join("kept.csv")).unwrap(), "written earlier\n");
    // A port that also gets something from elsewhere runs on that.
    assert_eq!(r.nodes["both"].status, Status::Ok);
    assert_eq!(col(r.output("both", "Sorted").unwrap(), "id"), ints(&[3]));
    assert_eq!(std::fs::read_to_string(dir.join("both.csv")).unwrap(), "id;v\n3;7\n");
    assert!(r.log.iter().any(|l| l.msg == "Test: disabled"));

    // The setting is kept in the file, and only where it is on.
    let text = ws.to_json();
    assert_eq!(text.matches("\"disabled\": true").count(), 2);
    assert!(!text.contains("\"disabled\": false"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_writer_can_be_told_the_format_types_and_sizes() {
    use og_engine::nodes::writer_file as f;
    // The chosen format decides the extension.
    assert_eq!(f("out/result.gpkg", None), "out/result.gpkg");
    assert_eq!(f("out/result.gpkg", Some("")), "out/result.gpkg");
    assert_eq!(f("out/result", Some("gpkg")), "out/result.gpkg");
    assert_eq!(f("out/result.shp", Some("gpkg")), "out/result.gpkg");
    assert_eq!(f("out/result.CSV", Some("csv")), "out/result.CSV");
    // An extension that is no format of its own is part of the name.
    assert_eq!(f("out/roads.2026", Some("xlsx")), "out/roads.2026.xlsx");

    let dir = tmp("fields");
    std::fs::write(dir.join("t.csv"), "code;name;share;count;note\n0114;Upplands Väsby;12,456;48000;7\n1480;Göteborg;3,1;;many\n;;;;\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "plain", "type": "Writer", "params": {"path": "plain", "format": "csv"}},
        {"id": "typed", "type": "Writer", "params": {"path": "typed.txt", "format": "csv", "fields": {
          "name": {"length": 8},
          "share": {"type": "decimal", "decimals": 1},
          "count": {"type": "text"},
          "note": {"type": "integer"},
          "code": {"type": "integer"},
          "gone": {"type": "text"},
          "untouched": {}
        }}},
        {"id": "none", "type": "Writer", "params": {"path": "nothing"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["plain", "Input"]},
        {"from": ["in", "Output"], "to": ["typed", "Input"]},
        {"from": ["in", "Output"], "to": ["none", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(
        std::fs::read_to_string(dir.join("plain.csv")).unwrap(),
        "code;name;share;count;note\n0114;Upplands Väsby;12.456;48000;7\n1480;Göteborg;3.1;;many\n;;;;\n"
    );
    // The name is cut, the share rounded, the code becomes a number and loses its zero,
    // and the note that is not a number is left empty.
    assert_eq!(std::fs::read_to_string(dir.join("typed.csv")).unwrap(), "code;name;share;count;note\n114;Upplands;12.5;48000;7\n1480;Göteborg;3.1;;\n;;;;\n");
    assert_eq!(r.nodes["typed"].files, [dir.join("typed.csv")]);
    let w = &r.nodes["typed"].warnings;
    assert!(w.contains(&"1 values in name are longer than 8 characters and were cut.".to_string()), "{:?}", w);
    assert!(w.contains(&"1 values in note are not numbers and were left empty.".to_string()), "{:?}", w);
    assert!(w.contains(&"The attribute gone has settings in the output but is not in the stream.".to_string()), "{:?}", w);
    assert_eq!(w.len(), 3, "{:?}", w);
    // What came in is not changed by what a writer does with it.
    assert_eq!(col(r.output("in", "Output").unwrap(), "code")[0], text("0114"));
    assert!(r.nodes["none"].error.as_ref().unwrap().contains("has no extension. Choose a format"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_addresses_are_told_from_files() {
    use og_engine::nodes::{is_url, source_ext};
    assert!(is_url("https://example.org/data.gpkg") && is_url(" HTTP://example.org/x") && !is_url("data/https.csv") && !is_url("C:\\data\\x.csv"));
    assert_eq!(source_ext("https://example.org/open/roads.GPKG?version=2#top"), "gpkg");
    assert_eq!(source_ext("https://example.org/geoserver/wfs?service=WFS&outputFormat=csv"), "");
    assert_eq!(source_ext("data/towns.csv"), "csv");
    // Without a backend there is nothing to fetch with, and the error says so.
    let ws = Workspace::from_json(r#"{"nodes": [{"id": "in", "type": "Reader", "params": {"path": "https://example.org/data.csv"}}]}"#).unwrap();
    let r = run(&ws, None, std::path::Path::new("."));
    assert!(r.nodes["in"].error.as_ref().unwrap().contains("Reading from the web needs the GDAL support"), "{:?}", r.nodes["in"].error);
}

#[test]
fn area_and_length_are_rounded_when_asked() {
    let dir = tmp("decimals");
    // A triangle with sides 3, 4 and 5 hundred metres, and a line of 1234.5678 m.
    std::fs::write(dir.join("a.csv"), "id;wkt\n1;POLYGON ((0 0, 333.333 0, 0 444.444, 0 0))\n2;LINESTRING (0 0, 1234.5678 0)\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "a.csv", "crs": "EPSG:3006"}},
        {"id": "all", "type": "AreaCalculator"},
        {"id": "two", "type": "AreaCalculator", "params": {"decimals": "2", "unit": "ha"}},
        {"id": "none", "type": "AreaCalculator", "params": {"decimals": 0}},
        {"id": "len", "type": "LengthCalculator", "params": {"decimals": "1", "unit": "km"}},
        {"id": "bad", "type": "LengthCalculator", "params": {"decimals": "two"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["all", "Input"]},
        {"from": ["in", "Output"], "to": ["two", "Input"]},
        {"from": ["in", "Output"], "to": ["none", "Input"]},
        {"from": ["in", "Output"], "to": ["len", "Input"]},
        {"from": ["in", "Output"], "to": ["bad", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let area = |id: &str| col(r.output(id, "Output").unwrap(), "_area")[0].clone();
    // Without a setting nothing is rounded.
    let Value::Real(full) = area("all") else { panic!("the area should be a decimal number") };
    assert!((full - 74073.925926).abs() < 1e-5 && full.fract() != 0.0, "{}", full);
    assert_eq!(area("two"), Value::Real(7.41));
    assert_eq!(area("none"), Value::Real(74074.0));
    assert_eq!(col(r.output("len", "Output").unwrap(), "_length")[1], Value::Real(1.2));
    assert!(r.nodes["bad"].error.as_ref().unwrap().starts_with("The number of decimals must be a whole number from 0 to 15"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn points_are_counted_and_joined_by_location() {
    let dir = tmp("location");
    // Two squares side by side and one apart, and a line through the first two.
    std::fs::write(
        dir.join("areas.csv"),
        "code;name;wkt\n10;West;POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0))\n20;East;POLYGON ((100 0, 200 0, 200 100, 100 100, 100 0))\n30;Far;POLYGON ((500 0, 600 0, 600 100, 500 100, 500 0))\n40;A line;LINESTRING (0 50, 200 50)\n",
    )
    .unwrap();
    // Two points in West, one in East, one on the edge between them, one outside all.
    std::fs::write(dir.join("points.csv"), "id;name;x;y\n1;a;10;10\n2;b;90;90\n3;c;150;50\n4;d;100;50\n5;e;300;50\n").unwrap();
    std::fs::write(
        dir.join("lines.csv"),
        "id;wkt\n1;LINESTRING (510 10, 590 90)\n2;POLYGON ((120 20, 180 20, 180 80, 120 80, 120 20))\n3;LINESTRING (900 0, 950 0)\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "areas", "type": "Reader", "params": {"path": "areas.csv", "crs": "EPSG:3006"}},
        {"id": "points", "type": "Reader", "params": {"path": "points.csv", "crs": "EPSG:3006"}},
        {"id": "lines", "type": "Reader", "params": {"path": "lines.csv", "crs": "EPSG:3006"}},
        {"id": "count", "type": "PointCounter"},
        {"id": "named", "type": "PointCounter", "params": {"name": "stops"}},
        {"id": "join", "type": "SpatialJoiner"},
        {"id": "some", "type": "SpatialJoiner", "params": {"attributes": "code, name", "prefix": "area_"}},
        {"id": "other", "type": "SpatialJoiner", "params": {"attributes": "code"}},
        {"id": "wrong", "type": "SpatialJoiner", "params": {"attributes": "population"}}
      ],
      "edges": [
        {"from": ["areas", "Output"], "to": ["count", "Polygons"]},
        {"from": ["points", "Output"], "to": ["count", "Points"]},
        {"from": ["areas", "Output"], "to": ["named", "Polygons"]},
        {"from": ["lines", "Output"], "to": ["named", "Points"]},
        {"from": ["points", "Output"], "to": ["join", "Features"]},
        {"from": ["areas", "Output"], "to": ["join", "Polygons"]},
        {"from": ["points", "Output"], "to": ["some", "Features"]},
        {"from": ["areas", "Output"], "to": ["some", "Polygons"]},
        {"from": ["lines", "Output"], "to": ["other", "Features"]},
        {"from": ["areas", "Output"], "to": ["other", "Polygons"]},
        {"from": ["points", "Output"], "to": ["wrong", "Features"]},
        {"from": ["areas", "Output"], "to": ["wrong", "Polygons"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);

    // The polygons come out as they went in, with the number of points in each. The point
    // on the shared edge is in both, and the line holds nothing.
    let c = r.output("count", "Polygons").unwrap();
    assert_eq!(c.field_names(), ["code", "name", "point_count"]);
    assert_eq!(col(c, "point_count"), ints(&[3, 2, 0, 0]));
    assert_eq!(c.len(), 4);
    let w = &r.nodes["count"].warnings;
    assert!(w.contains(&"1 features in Polygons are not areas and hold nothing.".to_string()), "{:?}", w);
    assert!(w.contains(&"1 points lie in more than one polygon and were counted in each of them.".to_string()), "{:?}", w);
    // Lines are not points, so nothing is counted, under the name that was asked for.
    assert_eq!(col(r.output("named", "Polygons").unwrap(), "stops"), ints(&[0, 0, 0, 0]));
    assert!(r.nodes["named"].warnings.iter().any(|w| w.starts_with("3 features in Points are not single points")));

    // Each point gets the attributes of its polygon, but keeps its own name.
    let j = r.output("join", "Joined").unwrap();
    assert_eq!(j.field_names(), ["id", "name", "x", "y", "code"]);
    assert_eq!(col(j, "id"), ints(&[1, 2, 3, 4]));
    assert_eq!(col(j, "name"), texts(&["a", "b", "c", "d"]));
    assert_eq!(col(j, "code"), ints(&[10, 10, 20, 10]));
    assert_eq!(col(r.output("join", "Unjoined").unwrap(), "id"), ints(&[5]));
    assert_eq!(r.output("join", "Unjoined").unwrap().field_names(), ["id", "name", "x", "y"]);
    let w = &r.nodes["join"].warnings;
    assert!(w.iter().any(|w| w.starts_with("Not taken from Join, since the names are already there: name. Give a prefix to take them.")), "{:?}", w);
    assert!(r.log.iter().any(|l| l.msg.contains("1 features found more than one and got the attributes of the first")));

    // Chosen attributes with a prefix: nothing clashes, so the name of the polygon comes along.
    let s = r.output("some", "Joined").unwrap();
    assert_eq!(s.field_names(), ["id", "name", "x", "y", "area_code", "area_name"]);
    assert_eq!(col(s, "area_name"), texts(&["West", "West", "East", "West"]));
    assert!(!r.nodes["some"].warnings.iter().any(|w| w.starts_with("Not taken")));

    // A line or an area is placed by a point that lies on it.
    let o = r.output("other", "Joined").unwrap();
    assert_eq!((col(o, "id"), col(o, "code")), (ints(&[1, 2]), ints(&[30, 20])));
    assert_eq!(col(r.output("other", "Unjoined").unwrap(), "id"), ints(&[3]));
    assert!(matches!(o.features[0].geom, Some(geo::Geometry::LineString(_))), "the geometry of the feature is kept");
    assert!(r.log.iter().any(|l| l.msg.contains("3 features that are not points were placed by a point inside them")));
    assert!(r.nodes["wrong"].error.as_ref().unwrap().contains("population"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lines_points_and_areas_are_joined_by_how_they_lie() {
    let dir = tmp("location2");
    // Two zones side by side, a road through both, a road in the west one, and a path that
    // touches the road at its end.
    std::fs::write(
        dir.join("zones.csv"),
        "zone;people;wkt\nWest;100;POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0))\nEast;250;POLYGON ((100 0, 200 0, 200 100, 100 100, 100 0))\n",
    )
    .unwrap();
    std::fs::write(dir.join("roads.csv"), "road;wkt\nLong;LINESTRING (10 50, 190 50)\nShort;LINESTRING (10 10, 60 10)\nPath;LINESTRING (60 10, 60 -40)\n")
        .unwrap();
    std::fs::write(dir.join("stops.csv"), "stop;x;y\nS1;50;50\nS2;150;53\nS3;150;90\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "zones", "type": "Reader", "params": {"path": "zones.csv", "crs": "EPSG:3006"}},
        {"id": "roads", "type": "Reader", "params": {"path": "roads.csv", "crs": "EPSG:3006"}},
        {"id": "stops", "type": "Reader", "params": {"path": "stops.csv", "crs": "EPSG:3006"}},
        {"id": "every", "type": "SpatialJoiner", "params": {"relation": "intersects", "mode": "all", "attributes": "zone"}},
        {"id": "summed", "type": "SpatialJoiner", "params": {"relation": "intersects", "mode": "summary", "count": "zones"}},
        {"id": "within", "type": "SpatialJoiner", "params": {"relation": "within", "attributes": "zone"}},
        {"id": "touch", "type": "SpatialJoiner", "params": {"relation": "touches", "mode": "all", "prefix": "other_"}},
        {"id": "near", "type": "SpatialJoiner", "params": {"relation": "near", "distance": "5", "attributes": "road"}}
      ],
      "edges": [
        {"from": ["roads", "Output"], "to": ["every", "Features"]},
        {"from": ["zones", "Output"], "to": ["every", "Join"]},
        {"from": ["roads", "Output"], "to": ["summed", "Features"]},
        {"from": ["zones", "Output"], "to": ["summed", "Join"]},
        {"from": ["roads", "Output"], "to": ["within", "Features"]},
        {"from": ["zones", "Output"], "to": ["within", "Join"]},
        {"from": ["roads", "Output"], "to": ["touch", "Features"]},
        {"from": ["roads", "Output"], "to": ["touch", "Join"]},
        {"from": ["stops", "Output"], "to": ["near", "Features"]},
        {"from": ["roads", "Output"], "to": ["near", "Join"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // Every zone a road reaches into: the long one gets two features.
    let e = r.output("every", "Joined").unwrap();
    assert_eq!(col(e, "road"), texts(&["Long", "Long", "Short", "Path"]));
    assert_eq!(col(e, "zone"), texts(&["West", "East", "West", "West"]));
    // Summarized: counted, the people added up and the names listed.
    let s = r.output("summed", "Joined").unwrap();
    assert_eq!(col(s, "zones"), ints(&[2, 1, 1]));
    assert_eq!(col(s, "people"), ints(&[350, 100, 100]));
    assert_eq!(col(s, "zone"), texts(&["West, East", "West", "West"]));
    assert_eq!(r.output("summed", "Unjoined").unwrap().len(), 0);
    // Within: only the short road lies wholly in a zone.
    assert_eq!(col(r.output("within", "Joined").unwrap(), "road"), texts(&["Short"]));
    assert_eq!(col(r.output("within", "Unjoined").unwrap(), "road"), texts(&["Long", "Path"]));
    // Line to line: the path meets the short road at its end, and touches nothing else.
    let t = r.output("touch", "Joined").unwrap();
    assert_eq!((col(t, "road"), col(t, "other_road")), (texts(&["Short", "Path"]), texts(&["Path", "Short"])));
    // Line to point: the stop within 5 m of a road gets it.
    let n = r.output("near", "Joined").unwrap();
    assert_eq!((col(n, "stop"), col(n, "road")), (texts(&["S1", "S2"]), texts(&["Long", "Long"])));
    assert_eq!(col(r.output("near", "Unjoined").unwrap(), "stop"), texts(&["S3"]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn swap_explode_hulls_and_intersect() {
    let dir = tmp("newgeom");
    std::fs::write(
        dir.join("zones.csv"),
        "zone;wkt\nWest;POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0))\nEast;POLYGON ((100 0, 200 0, 200 100, 100 100, 100 0))\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("things.csv"),
        "id;wkt\n1;LINESTRING (50 50, 150 50, 150 150)\n2;POINT (20 20)\n3;POINT (500 500)\n4;POLYGON ((90 10, 110 10, 110 20, 90 20, 90 10))\n",
    )
    .unwrap();
    // Points in a U: the convex hull is the square, the concave one leaves the gap out.
    let mut u = String::from("id;x;y\n");
    let mut n = 0;
    for i in 0..=10 {
        for j in 0..=10 {
            if !(3..=7).contains(&i) || j <= 2 {
                n += 1;
                u.push_str(&format!("{};{};{}\n", n, i * 10, j * 10));
            }
        }
    }
    std::fs::write(dir.join("u.csv"), u).unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "zones", "type": "Reader", "params": {"path": "zones.csv", "crs": "EPSG:3006"}},
        {"id": "things", "type": "Reader", "params": {"path": "things.csv", "crs": "EPSG:3006"}},
        {"id": "u", "type": "Reader", "params": {"path": "u.csv", "crs": "EPSG:3006"}},
        {"id": "swap", "type": "XySwapper"},
        {"id": "segments", "type": "LineExploder"},
        {"id": "outlines", "type": "LineExploder", "params": {"areas": "outline", "number": ""}},
        {"id": "convex", "type": "ConvexHuller"},
        {"id": "each", "type": "ConvexHuller", "params": {"for": "each"}},
        {"id": "concave", "type": "ConcaveHuller", "params": {"concavity": "0.5"}},
        {"id": "cut", "type": "Intersector"}
      ],
      "edges": [
        {"from": ["things", "Output"], "to": ["swap", "Input"]},
        {"from": ["things", "Output"], "to": ["segments", "Input"]},
        {"from": ["zones", "Output"], "to": ["outlines", "Input"]},
        {"from": ["u", "Output"], "to": ["convex", "Input"]},
        {"from": ["things", "Output"], "to": ["each", "Input"]},
        {"from": ["u", "Output"], "to": ["concave", "Input"]},
        {"from": ["things", "Output"], "to": ["cut", "Input"]},
        {"from": ["zones", "Output"], "to": ["cut", "Overlay"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    use geo::{Area, CoordsIter, Length};
    // Swapped: the point at 20 20 stays, the line now starts at 50 50 and turns at 50 150.
    let sw = geoms(&r, "swap", "Output");
    assert_eq!(sw[0].coords_iter().nth(1).unwrap(), geo::coord! { x: 50.0, y: 150.0 });
    // Two segments of the line, numbered; the points and the area pass as they are.
    let seg = r.output("segments", "Output").unwrap();
    assert_eq!(col(seg, "id"), ints(&[1, 1, 2, 3, 4]));
    assert_eq!(col(seg, "segment"), vec![Value::Int(1), Value::Int(2), Value::Null, Value::Null, Value::Null]);
    // Two squares give eight sides.
    assert_eq!(r.output("outlines", "Output").unwrap().len(), 8);
    // The hulls around the U.
    let convex = geoms(&r, "convex", "Output");
    assert_eq!((convex.len(), convex[0].unsigned_area()), (1, 10_000.0));
    assert_eq!(col(r.output("convex", "Output").unwrap(), "count"), ints(&[n]));
    let concave = geoms(&r, "concave", "Output")[0].unsigned_area();
    assert!(concave < 6_000.0 && concave > 4_000.0, "the gap of the U is left out: {}", concave);
    // A hull for each: the line's triangle, two points, the square.
    let each = geoms(&r, "each", "Output");
    assert_eq!(each[0].unsigned_area(), 5_000.0);
    assert!(matches!(each[1], geo::Geometry::Point(_)));
    // Intersect: the line in two pieces, one per zone, the bit above both to Outside; the
    // point in West; the small square in two halves.
    let cut = r.output("cut", "Intersection").unwrap();
    assert_eq!(col(cut, "id"), ints(&[1, 1, 2, 4, 4]));
    assert_eq!(col(cut, "zone"), texts(&["West", "East", "West", "West", "East"]));
    let pieces = geoms(&r, "cut", "Intersection");
    assert!(
        (geo::Euclidean.length(match &pieces[0] {
            geo::Geometry::LineString(l) => l,
            other => panic!("{:?}", other),
        }) - 50.0)
            .abs()
            < 1e-9
    );
    assert!((pieces[3].unsigned_area() - 100.0).abs() < 1e-9 && (pieces[4].unsigned_area() - 100.0).abs() < 1e-9);
    let out = r.output("cut", "Outside").unwrap();
    assert_eq!(col(out, "id"), ints(&[1, 3]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tiles_of_3d_tiles_1_1_are_read() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiles11");
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "meta", "type": "Reader", "params": {"path": "house_metadata.json"}},
        {"id": "packed", "type": "Reader", "params": {"path": "house_meshopt.json"}},
        {"id": "finest", "type": "Reader", "params": {"path": "quadtree/tileset.json", "tiles_detail": "finest"}},
        {"id": "few", "type": "Reader", "params": {"path": "quadtree/tileset.json", "tiles_detail": "finest", "tiles_most": "10"}},
        {"id": "coarse", "type": "Reader", "params": {"path": "quadtree/tileset.json", "tiles_most": "10"}},
        {"id": "complex", "type": "Reader", "params": {"path": "complex/tileset.json"}}
      ],
      "edges": []
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // The name and the year from the property table.
    let m = r.output("meta", "Output").unwrap();
    assert_eq!(col(m, "name"), texts(&["Ekbacken"]));
    assert_eq!(col(m, "built"), ints(&[1932]));
    // Packed with meshopt and quantized: the same house, to the centimetre.
    let p = r.output("packed", "Output").unwrap();
    assert_eq!(col(p, "triangles"), col(m, "triangles"));
    use geo::BoundingRect;
    let (a, b) = (m.features[0].geom.as_ref().unwrap().bounding_rect().unwrap(), p.features[0].geom.as_ref().unwrap().bounding_rect().unwrap());
    for (x, y) in [(a.min().x, b.min().x), (a.min().y, b.min().y), (a.max().x, b.max().x), (a.max().y, b.max().y)] {
        assert!((x - y).abs() < 0.01, "{} {}", x, y);
    }
    for k in ["z_min", "z_max"] {
        let (x, y) = (col(m, k)[0].as_f64().unwrap(), col(p, k)[0].as_f64().unwrap());
        assert!((x - y).abs() < 0.01, "{}: {} {}", k, x, y);
    }
    // Implicit tiling: 32 tiles at level 5, found through the subtrees.
    let f = r.output("finest", "Output").unwrap();
    assert_eq!(f.len(), 32);
    assert!(col(f, "level").iter().all(|l| *l == Value::Int(5)));
    assert!(r.nodes["few"].error.as_ref().unwrap().contains("More than 10 tiles"));
    // As in 3DBAG, only the smallest tiles have content, so the error says a coarser level
    // of detail does not help.
    assert!(r.nodes["coarse"].error.as_ref().unwrap().contains("content only in its smallest tiles"), "{:?}", r.nodes["coarse"].error);
    // Arrays, booleans, strings and enums of a property table, as text.
    let c = r.output("complex", "Output").unwrap();
    assert_eq!(col(c, "example_variable_length_ARRAY_STRING"), texts(&["One", "One, Two", "One, Two, Three", "One, Two, Theee, Four"]));
    assert_eq!(col(c, "example_fixed_length_ARRAY_ENUM")[0], Value::Text("ExampleEnumValueA, ExampleEnumValueB".into()));
    assert_eq!(col(c, "example_variable_length_ARRAY_normalized_UINT8")[1], Value::Text("0, 0.5019608, 1".into()));
}

#[test]
fn point_clouds_are_read_from_las_and_laz() {
    let dir = tmp("pointcloud");
    // A square of 40 × 25 points, every other one ground and building, with colour in 16 bits
    // and the coordinate system as WKT, as LAS 1.4 keeps it.
    let wkt = r#"PROJCS["SWEREF99 TM",GEOGCS["SWEREF99",DATUM["SWEREF99",SPHEROID["GRS 1980",6378137,298.257222101]],AUTHORITY["EPSG","4619"]],PROJECTION["Transverse_Mercator"],UNIT["metre",1],AUTHORITY["EPSG","3006"]]"#;
    for name in ["cloud.las", "cloud.laz"] {
        let mut b = las::Builder::from((1, 4));
        b.point_format = las::point::Format::new(7).unwrap();
        b.transforms = las::Vector {
            x: las::Transform { scale: 0.01, offset: 319000.0 },
            y: las::Transform { scale: 0.01, offset: 6399000.0 },
            z: las::Transform { scale: 0.01, offset: 0.0 },
        };
        let mut header = b.into_header().unwrap();
        header.set_wkt_crs(wkt.as_bytes().to_vec()).unwrap();
        let mut w = las::Writer::from_path(dir.join(name), header).unwrap();
        for i in 0..1000u32 {
            let (x, y) = ((i % 40) as f64, (i / 40) as f64);
            let class = if i % 2 == 0 { las::point::Classification::Ground } else { las::point::Classification::Building };
            w.write_point(las::Point {
                x: 319000.0 + x,
                y: 6399000.0 + y,
                z: 50.0 + x / 10.0,
                intensity: i as u16,
                return_number: 1,
                number_of_returns: 1,
                classification: class,
                color: Some(las::Color { red: 65535, green: 32768, blue: 0 }),
                gps_time: Some(0.0),
                ..Default::default()
            })
            .unwrap();
        }
        w.close().unwrap();
    }
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "all", "type": "Reader", "params": {"path": "cloud.las"}},
        {"id": "packed", "type": "Reader", "params": {"path": "cloud.laz"}},
        {"id": "buildings", "type": "Reader", "params": {"path": "cloud.laz", "las_classes": "6"}},
        {"id": "thin", "type": "Reader", "params": {"path": "cloud.laz", "las_every": "10"}},
        {"id": "corner", "type": "Reader", "params": {"path": "cloud.laz", "within": "rectangle", "min_x": "319000", "min_y": "6399000", "max_x": "319009.5", "max_y": "6399004.5"}},
        {"id": "few", "type": "Reader", "params": {"path": "cloud.laz", "limit": "100"}},
        {"id": "wrong", "type": "Reader", "params": {"path": "cloud.laz", "las_classes": "ground"}}
      ],
      "edges": []
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let all = r.output("all", "Output").unwrap();
    assert_eq!((all.len(), all.crs), (1000, Some(3006)));
    assert_eq!(all.field_names(), ["classification", "class", "intensity", "return_number", "returns", "red", "green", "blue"]);
    // The point with its height, and its colour in 8 bits.
    let f = &all.features[41];
    assert!(matches!(f.geom, Some(geo::Geometry::Point(p)) if (p.x() - 319001.0).abs() < 1e-6 && (p.y() - 6399001.0).abs() < 1e-6));
    assert!((f.z.as_ref().unwrap()[0] - 50.1).abs() < 1e-6);
    assert_eq!(&f.attrs[..2], &[Value::Int(6), Value::Text("building".into())]);
    assert_eq!(&f.attrs[5..], &[Value::Int(255), Value::Int(128), Value::Int(0)]);
    // LAZ is the same.
    let packed = r.output("packed", "Output").unwrap();
    assert_eq!(packed.len(), 1000);
    assert_eq!(packed.features[41].attrs, f.attrs);
    assert_eq!(r.output("buildings", "Output").unwrap().len(), 500);
    assert_eq!(r.output("thin", "Output").unwrap().len(), 100);
    assert_eq!(r.output("corner", "Output").unwrap().len(), 50);
    assert_eq!(r.output("few", "Output").unwrap().len(), 100);
    assert!(r.nodes["few"].warnings.iter().any(|w| w.starts_with("Only the first 100 points")));
    assert!(r.nodes["wrong"].error.as_ref().unwrap().contains("not the number of a class"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn point_clouds_of_3d_tiles_are_read() {
    let dir = tmp("pnts");
    // Four points, quantized in a volume of 10 m, with colours, around a centre.
    let ft = r#"{"POINTS_LENGTH":4,"POSITION_QUANTIZED":{"byteOffset":0},"RGB":{"byteOffset":24},"QUANTIZED_VOLUME_OFFSET":[0,0,0],"QUANTIZED_VOLUME_SCALE":[10,10,10],"RTC_CENTER":[100,200,300]}"#;
    let mut json = ft.as_bytes().to_vec();
    while !(28 + json.len()).is_multiple_of(8) {
        json.push(b' ');
    }
    let mut bin: Vec<u8> = Vec::new();
    for q in [[0u16, 0, 0], [65535, 0, 0], [0, 65535, 0], [0, 0, 65535]] {
        for v in q {
            bin.extend(v.to_le_bytes());
        }
    }
    bin.extend([255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9]);
    let mut file = b"pnts".to_vec();
    file.extend(1u32.to_le_bytes());
    file.extend(((28 + json.len() + bin.len()) as u32).to_le_bytes());
    file.extend((json.len() as u32).to_le_bytes());
    file.extend((bin.len() as u32).to_le_bytes());
    file.extend(0u32.to_le_bytes());
    file.extend(0u32.to_le_bytes());
    file.extend(&json);
    file.extend(&bin);
    std::fs::write(dir.join("cloud.pnts"), file).unwrap();
    let ws = Workspace::from_json(r#"{"nodes": [{"id": "c", "type": "Reader", "params": {"path": "cloud.pnts"}}], "edges": []}"#).unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let c = r.output("c", "Output").unwrap();
    assert_eq!(c.len(), 4);
    assert_eq!(col(c, "red"), ints(&[255, 0, 0, 9]));
    let xs: Vec<f64> = c
        .features
        .iter()
        .map(|f| match f.geom {
            Some(geo::Geometry::Point(p)) => p.x(),
            _ => f64::NAN,
        })
        .collect();
    assert_eq!(xs, [100.0, 110.0, 100.0, 100.0]);
    assert_eq!(c.features[3].z.as_ref().unwrap()[0], 310.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sun_study_and_water_in_a_scene() {
    let dir = tmp("study");
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    for f in ["house.gltf", "house.bin"] {
        std::fs::copy(data.join(f), dir.join(f)).unwrap();
    }
    // A pond in front of the house.
    std::fs::write(dir.join("pond.csv"), "name;wkt\nPond;POLYGON ((-30 -30, 30 -30, 30 -8, -30 -8, -30 -30))\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "house", "type": "Reader", "params": {"path": "house.gltf"}},
        {"id": "pond", "type": "Reader", "params": {"path": "pond.csv"}},
        {"id": "study", "type": "SceneRenderer", "params": {"width": "64", "height": "40", "quality": "4", "sun": "study",
            "study_dates": "2026-03-20, 2026-06-21", "study_from": "10:00", "study_to": "14:00", "study_every": "4", "study_folder": "frames"}},
        {"id": "water", "type": "SceneRenderer", "params": {"width": "64", "height": "40", "quality": "4", "background": "sky", "water": "areas",
            "clouds": "40", "haze": "20", "bounces": "2", "reflections": "true", "look": "white"}},
        {"id": "nowater", "type": "SceneRenderer", "params": {"width": "64", "height": "40", "water": "areas"}}
      ],
      "edges": [
        {"from": ["house", "Output"], "to": ["study", "Models"]},
        {"from": ["house", "Output"], "to": ["water", "Models"]},
        {"from": ["pond", "Output"], "to": ["water", "Water"]},
        {"from": ["house", "Output"], "to": ["nowater", "Models"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // Two dates of two times: a sheet of 2 × 2 pictures, each with its date and time.
    let sheet = r.output("study", "Image").unwrap().image.clone().unwrap();
    assert_eq!(sheet.width, 16 + 2 * (64 + 16));
    assert!(sheet.height > 2 * 40 + 2 * 26);
    for name in ["sun_2026-03-20_10_00.png", "sun_2026-06-21_14_00.png"] {
        assert!(dir.join("frames").join(name).exists(), "{} should be written", name);
    }
    assert!(r.log.iter().any(|l| l.msg.contains("4 pictures")));
    assert_eq!(r.nodes["water"].status, Status::Ok, "{:?}", r.nodes["water"].error);
    assert!(r.log.iter().any(|l| l.msg.contains("water at")));
    assert!(r.nodes["nowater"].error.as_ref().unwrap().contains("Connect the lakes or the sea to Water"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tools_have_names_that_say_what_they_do() {
    use og_engine::{node_type, NODE_TYPES};
    assert_eq!(node_type("AttributeManager").unwrap().title, "Manage attributes");
    assert_eq!(node_type("Sorter").unwrap().title, "Sort");
    assert_eq!(node_type("Reader").unwrap().title, "Input");
    assert_eq!(node_type("Writer").unwrap().title, "Output");
    // Every type has a name of its own to show, and the names in files are unchanged.
    let mut titles: Vec<&str> = NODE_TYPES.iter().map(|t| t.title).collect();
    titles.sort();
    titles.dedup();
    assert_eq!(titles.len(), NODE_TYPES.len());
    assert!(NODE_TYPES.iter().all(|t| !t.title.is_empty() && !t.name.contains(' ')));
    // The tool the two new ones replace still runs, but is not offered. PostGIS and the
    // GeoSpatial PDF are reached from Input and Output instead of the list.
    let mut hidden: Vec<&str> = NODE_TYPES.iter().filter(|t| t.hidden).map(|t| t.name).collect();
    hidden.sort();
    assert_eq!(hidden, ["LayeredPdfWriter", "PointOnAreaOverlayer", "PostgisReader", "PostgisWriter"]);

    // A workspace saved before the change has the old names on the nodes the user did not
    // name. They are read as the new ones, and a name the user chose is left alone.
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "a", "type": "Sorter", "name": "Sorter"},
        {"id": "b", "type": "Sorter", "name": "Sorter_2"},
        {"id": "c", "type": "Sorter", "name": "Sorter_by_name"},
        {"id": "d", "type": "Sorter", "name": "Largest first"},
        {"id": "e", "type": "Sorter"},
        {"id": "f", "type": "AttributeManager", "name": "AttributeManager_12"},
        {"id": "g", "type": "Sorter", "name": "Sort 3"},
        {"id": "h", "type": "Mystery", "name": "Mystery"}
      ]}"#,
    )
    .unwrap();
    let labels: Vec<&str> = ws.nodes.iter().map(|n| n.label()).collect();
    assert_eq!(labels, ["Sort", "Sort 2", "Sorter_by_name", "Largest first", "Sort", "Manage attributes 12", "Sort 3", "Mystery"]);
    assert_eq!(ws.nodes[0].kind, "Sorter", "the type in the file is the same as before");
    use og_engine::run::default_name_today;
    assert_eq!(default_name_today("Sorter", "Sort 2").as_deref(), Some("Sort 2"));
    assert_eq!(default_name_today("Sorter", "Sorting"), None);
    assert_eq!(default_name_today("Reader", "Reader"), Some("Input".to_string()));
    assert_eq!(default_name_today("Reader", "Reader 2"), Some("Input 2".to_string()), "as 0.52 named them");
    assert_eq!(default_name_today("Writer", "Writer_3"), Some("Output 3".to_string()));
}

#[test]
fn numbers_are_rounded_in_chosen_attributes() {
    let dir = tmp("rounder");
    std::fs::write(dir.join("t.csv"), "name;share;ratio;count\nA;12,3456;0,125;3\nB;;-2,5;4\nC;0,1;1,005;5\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "two", "type": "Rounder", "params": {"attributes": "share, ratio", "decimals": "2"}},
        {"id": "none", "type": "Rounder", "params": {"attributes": ["ratio", "count", "name"]}},
        {"id": "bad", "type": "Rounder", "params": {"attributes": "share", "decimals": "-1"}},
        {"id": "missing", "type": "Rounder", "params": {"attributes": "height"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["two", "Input"]},
        {"from": ["in", "Output"], "to": ["none", "Input"]},
        {"from": ["in", "Output"], "to": ["bad", "Input"]},
        {"from": ["in", "Output"], "to": ["missing", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let two = r.output("two", "Output").unwrap();
    assert_eq!(col(two, "share"), [Value::Real(12.35), Value::Null, Value::Real(0.1)]);
    // Halves go away from zero.
    assert_eq!(col(two, "ratio"), [Value::Real(0.13), Value::Real(-2.5), Value::Real(1.0)]);
    let none = r.output("none", "Output").unwrap();
    assert_eq!(col(none, "ratio"), [Value::Real(0.0), Value::Real(-3.0), Value::Real(1.0)], "no decimals is the default");
    assert_eq!(col(none, "count"), ints(&[3, 4, 5]), "whole numbers are left as they are");
    assert_eq!(r.nodes["none"].warnings, ["Left as they are, since they are text and not numbers: name."]);
    assert!(r.nodes["bad"].error.as_ref().unwrap().starts_with("The number of decimals must be a whole number from 0 to 15"));
    assert!(r.nodes["missing"].error.as_ref().unwrap().starts_with("There is no attribute height."));
    assert_eq!(og_engine::nodes::round_to(0.1 + 0.2, 2), 0.3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_attribute_is_split_into_attributes_or_features() {
    let dir = tmp("splitter");
    std::fs::write(dir.join("stops.csv"), "stop|code|lines|path\nTorget|Göteborg$1480|1, 4,7|a\nStationen|Borås$1490|4|x\nByn|||\nKyrkan|Mark$0114|2,,3|\n")
        .unwrap();
    std::fs::write(dir.join("tabs.csv"), "id;path\n1;a\tb\tc\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "stops.csv"}},
        {"id": "code", "type": "AttributeSplitter", "params": {"attribute": "code", "delimiter": "$", "name": "muni"}},
        {"id": "lines", "type": "AttributeSplitter", "params": {"attribute": "lines", "mode": "rows", "name": "line", "index": "line_no"}},
        {"id": "replace", "type": "AttributeSplitter", "params": {"attribute": "lines", "mode": "rows"}},
        {"id": "two", "type": "AttributeSplitter", "params": {"attribute": "lines", "max": "2", "keep_empty": "true"}},
        {"id": "raw", "type": "AttributeSplitter", "params": {"attribute": "lines", "trim": "false", "keep_empty": "true"}},
        {"id": "tabs_in", "type": "Reader", "params": {"path": "tabs.csv"}},
        {"id": "tabs", "type": "AttributeSplitter", "params": {"attribute": "path", "delimiter": "\\t"}},
        {"id": "nothing", "type": "AttributeSplitter", "params": {"attribute": "lines", "delimiter": ""}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["code", "Input"]},
        {"from": ["in", "Output"], "to": ["lines", "Input"]},
        {"from": ["in", "Output"], "to": ["replace", "Input"]},
        {"from": ["in", "Output"], "to": ["two", "Input"]},
        {"from": ["in", "Output"], "to": ["raw", "Input"]},
        {"from": ["tabs_in", "Output"], "to": ["tabs", "Input"]},
        {"from": ["in", "Output"], "to": ["nothing", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);

    // Into attributes: the name and the code. The code keeps its leading zero, so the whole
    // column stays text.
    let c = r.output("code", "Output").unwrap();
    assert_eq!(c.field_names(), ["stop", "code", "lines", "path", "muni_1", "muni_2"]);
    assert_eq!(col(c, "muni_1"), [Value::Text("Göteborg".into()), Value::Text("Borås".into()), Value::Null, Value::Text("Mark".into())]);
    assert_eq!(
        col(c, "muni_2"),
        texts(&["1480", "1490", "", "0114"]).into_iter().map(|v| if v == Value::Text("".into()) { Value::Null } else { v }).collect::<Vec<_>>()
    );

    // One feature per part, with the number of the part. Spaces are taken away, empty parts
    // left out, and a feature with nothing to split comes out once.
    let l = r.output("lines", "Output").unwrap();
    assert_eq!(col(l, "stop"), texts(&["Torget", "Torget", "Torget", "Stationen", "Byn", "Kyrkan", "Kyrkan"]));
    assert_eq!(col(l, "line"), ints(&[1, 4, 7, 4]).into_iter().chain([Value::Null, Value::Int(2), Value::Int(3)]).collect::<Vec<_>>());
    assert_eq!(col(l, "line_no"), [Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(1), Value::Null, Value::Int(1), Value::Int(2)]);
    assert!(r.log.iter().any(|x| x.msg.contains("4 features became 7")));
    // Without a new name the parts take the place of the list.
    let rep = r.output("replace", "Output").unwrap();
    assert_eq!(rep.field_names(), ["stop", "code", "lines", "path"]);
    assert_eq!(col(rep, "lines")[0], Value::Int(1));

    // At most two parts: the last one keeps the rest.
    let two = r.output("two", "Output").unwrap();
    assert_eq!(col(two, "lines_1"), [Value::Int(1), Value::Int(4), Value::Null, Value::Int(2)]);
    assert_eq!(
        col(two, "lines_2"),
        texts(&["4,7", "", "", ",3"]).into_iter().map(|v| if v == Value::Text("".into()) { Value::Null } else { v }).collect::<Vec<_>>()
    );
    let raw = r.output("raw", "Output").unwrap();
    assert_eq!(col(raw, "lines_2")[0], Value::Text(" 4".into()), "spaces are kept when asked");
    assert_eq!(raw.field_names().len(), 4 + 3);

    let tabs = r.output("tabs", "Output").unwrap();
    assert_eq!(col(tabs, "path_3"), texts(&["c"]));
    assert!(r.nodes["nothing"].error.as_ref().unwrap().starts_with("Say what to split at"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn features_are_filtered_by_value_to_an_output_each() {
    use og_engine::node_type;
    let dir = tmp("valuefilter");
    std::fs::write(dir.join("t.csv"), "id;class;code\n1;A;1\n2;B;2\n3;C;3\n4;A;2\n5;;4\n6;Other;1\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "class", "type": "ValueFilter", "params": {"attribute": "class", "values": ["A", "B", "A", " "]}},
        {"id": "code", "type": "ValueFilter", "params": {"attribute": "code", "values": ["1", "2.0"]}},
        {"id": "clash", "type": "ValueFilter", "params": {"attribute": "class", "values": "Other\nB"}},
        {"id": "none", "type": "ValueFilter", "params": {"attribute": "class"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["class", "Input"]},
        {"from": ["in", "Output"], "to": ["code", "Input"]},
        {"from": ["in", "Output"], "to": ["clash", "Input"]},
        {"from": ["in", "Output"], "to": ["none", "Input"]},
        {"from": ["class", "Other"], "to": ["none", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    // The ports follow the values: one each, without repeats, and Other.
    let t = node_type("ValueFilter").unwrap();
    assert_eq!(t.outputs_for(&ws.nodes[1].params), ["A", "B", "Other"]);
    assert_eq!(t.outputs_for(&ws.nodes[3].params), ["Other", "B", "Other values"]);
    assert_eq!(t.outputs_for(&ws.nodes[4].params), ["Other"]);
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    assert_eq!(col(r.output("class", "A").unwrap(), "id"), ints(&[1, 4]));
    assert_eq!(col(r.output("class", "B").unwrap(), "id"), ints(&[2]));
    assert_eq!(col(r.output("class", "Other").unwrap(), "id"), ints(&[3, 5, 6]), "empty values go to Other too");
    // Numbers match numbers, whatever way they are written.
    assert_eq!(col(r.output("code", "1").unwrap(), "id"), ints(&[1, 6]));
    assert_eq!(col(r.output("code", "2.0").unwrap(), "id"), ints(&[2, 4]));
    assert_eq!(col(r.output("code", "Other").unwrap(), "id"), ints(&[3, 5]));
    assert_eq!(col(r.output("clash", "Other").unwrap(), "id"), ints(&[6]));
    assert_eq!(col(r.output("clash", "Other values").unwrap(), "id"), ints(&[1, 3, 4, 5]));
    assert!(r.nodes["none"].warnings[0].starts_with("No values are given"));
    let line = r.log.iter().find(|l| l.node.as_deref() == Some("class") && l.level == Level::Info).unwrap();
    assert!(line.msg.contains("6 in → \"A\" 2, \"B\" 1, Other 3 out"), "{}", line.msg);

    // A connection from a port that the values no longer give is an error that says so.
    let ws2 = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "f", "type": "ValueFilter", "params": {"attribute": "class", "values": ["A"]}},
        {"id": "t", "type": "Tester", "params": {"test": "id > 0"}}
      ],
      "edges": [{"from": ["in", "Output"], "to": ["f", "Input"]}, {"from": ["f", "B"], "to": ["t", "Input"]}]
    }"#,
    )
    .unwrap();
    let r = run(&ws2, None, &dir);
    assert!(r.nodes["t"].error.as_ref().unwrap().contains("There is no output port B on Filter by value"), "{:?}", r.nodes["t"].error);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn attributes_are_managed_as_a_table() {
    use og_engine::nodes::{manager_from_old, manager_is_old, AttrAction};
    let dir = tmp("manager");
    std::fs::write(dir.join("t.csv"), "name;pop;x_old;code\nSkövde;39000;1;1496\nSkara;13000;2;1495\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "table", "type": "AttributeManager", "params": {
          "attributes": [{"name": "code"}, {"name": "pop", "action": "rename", "to": "population"}, {"name": "x_old", "action": "remove"}, {"name": "gone", "action": "remove"}],
          "create": [{"name": "county", "value": "'Västra Götaland'"}, {"name": "thousands", "value": "pop / 1000"}, {"name": "", "value": "1"}]
        }},
        {"id": "only", "type": "AttributeManager", "params": {"attributes": [{"name": "name"}, {"name": "pop"}], "others": "remove"}},
        {"id": "old", "type": "AttributeManager", "params": {"keep": "pop, name", "rename": {"pop": "population"}}},
        {"id": "clash", "type": "AttributeManager", "params": {"attributes": [{"name": "pop", "action": "rename", "to": "name"}]}},
        {"id": "badexpr", "type": "AttributeManager", "params": {"create": [{"name": "y", "value": "nosuch + 1"}]}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["table", "Input"]},
        {"from": ["in", "Output"], "to": ["only", "Input"]},
        {"from": ["in", "Output"], "to": ["old", "Input"]},
        {"from": ["in", "Output"], "to": ["clash", "Input"]},
        {"from": ["in", "Output"], "to": ["badexpr", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // The listed ones in the order of the list, then the rest, and then the new ones.
    let t = r.output("table", "Output").unwrap();
    assert_eq!(t.field_names(), ["code", "population", "name", "county", "thousands"]);
    assert_eq!(col(t, "population"), ints(&[39000, 13000]));
    assert_eq!(col(t, "county"), texts(&["Västra Götaland", "Västra Götaland"]));
    assert_eq!(col(t, "thousands"), [Value::Real(39.0), Value::Real(13.0)], "a new attribute is calculated from the names that came in");
    assert_eq!(r.nodes["table"].warnings, ["Attributes that are not in the stream were skipped: gone."]);
    // Only what is listed.
    assert_eq!(r.output("only", "Output").unwrap().field_names(), ["name", "pop"]);
    // A model from before 0.53 still runs as it did.
    assert_eq!(r.output("old", "Output").unwrap().field_names(), ["population", "name"]);
    assert!(r.nodes["clash"].error.as_ref().unwrap().contains("Two attributes would be named \"name\""));
    assert!(r.nodes["badexpr"].error.as_ref().unwrap().starts_with("The value of y:"));

    // The old settings become rows of the table, in the order they had.
    let old = &ws.nodes[3].params;
    assert!(manager_is_old(old) && !manager_is_old(&ws.nodes[1].params));
    let fields: Vec<String> = ["name", "pop", "x_old", "code"].map(String::from).to_vec();
    let (rows, only_listed) = manager_from_old(old, &fields);
    assert!(only_listed);
    assert_eq!(
        rows,
        [
            ("pop".to_string(), AttrAction::Rename("population".into())),
            ("name".to_string(), AttrAction::Keep),
            ("x_old".to_string(), AttrAction::Remove),
            ("code".to_string(), AttrAction::Remove)
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn neighbours_stay_together_when_generalized() {
    let dir = tmp("topology");
    // Two areas that share a wiggly boundary along x = 100, and a line along part of it.
    // The rings start in different places, so each would be simplified differently alone.
    let wiggle = "100 0, 101 10, 99 20, 102 30, 93 40, 101 50, 99 60, 108 70, 97 80, 101 90, 100 100";
    let wiggle_back = "100 100, 101 90, 97 80, 108 70, 99 60, 101 50, 93 40, 102 30, 99 20, 101 10, 100 0";
    std::fs::write(
        dir.join("areas.csv"),
        format!(
            "id;wkt\n1;\"POLYGON ((0 0, {}, 0 100, 0 0))\"\n2;\"POLYGON ((200 50, 200 100, {}, 200 0, 200 50))\"\n3;\"LINESTRING (100 0, 101 10, 99 20, 102 30)\"\n",
            wiggle, wiggle_back
        ),
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "areas.csv", "crs": "EPSG:3006"}},
        {"id": "alone", "type": "Generalizer", "params": {"tolerance": "5"}},
        {"id": "together", "type": "Generalizer", "params": {"tolerance": "5", "topology": "true"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["alone", "Input"]},
        {"from": ["in", "Output"], "to": ["together", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    // The points of each area that lie on the shared boundary, in order of y.
    let shared = |t: &Table, i: usize| -> Vec<(i64, i64)> {
        let Some(geo::Geometry::Polygon(p)) = &t.features[i].geom else { panic!("an area") };
        let mut pts: Vec<(i64, i64)> = p
            .exterior()
            .0
            .iter()
            .filter(|c| (90.0..=110.0).contains(&c.x) && c.y > 0.0 && c.y < 100.0 || (c.x == 100.0))
            .map(|c| (c.x as i64, c.y as i64))
            .collect();
        pts.sort_by_key(|p| p.1);
        pts.dedup();
        pts
    };
    let area = |t: &Table| -> f64 { t.features[..2].iter().map(|f| geo::Area::unsigned_area(f.geom.as_ref().unwrap())).sum() };
    let together = r.output("together", "Generalized").unwrap();
    assert_eq!(shared(together, 0), shared(together, 1), "both areas have the same boundary");
    assert!(shared(together, 0).len() < 11, "and it is simplified: {:?}", shared(together, 0));
    assert!((area(together) - 20_000.0).abs() < 1e-6, "no gap and no overlap: {}", area(together));
    // The line that runs along the boundary gets the same points as it, where it runs.
    let Some(geo::Geometry::LineString(l)) = &together.features[2].geom else { panic!("a line") };
    let line: Vec<(i64, i64)> = l.0.iter().map(|c| (c.x as i64, c.y as i64)).collect();
    assert!(line.iter().all(|p| shared(together, 0).contains(p)), "{:?} lies on {:?}", line, shared(together, 0));
    // Alone, the line is simplified on its own and leaves the boundary.
    let alone = r.output("alone", "Generalized").unwrap();
    let Some(geo::Geometry::LineString(l)) = &alone.features[2].geom else { panic!("a line") };
    assert_eq!(l.0.len(), 2, "alone the line becomes straight, off the boundary that keeps (102, 30)");
    assert!(shared(alone, 0).contains(&(102, 30)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn features_are_aggregated_and_deaggregated() {
    let dir = tmp("aggregate");
    std::fs::write(
        dir.join("t.csv"),
        "id;kind;wkt\n1;bus;POINT (1 1)\n2;bus;POINT (2 2)\n3;tram;POINT (3 3)\n4;bus;LINESTRING (0 0, 5 5)\n5;tram;\"MULTIPOINT ((7 7), (8 8))\"\n6;;\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv", "crs": "EPSG:3006"}},
        {"id": "by", "type": "Aggregator", "params": {"group_by": "kind", "count": "count"}},
        {"id": "all", "type": "Aggregator"},
        {"id": "parts", "type": "Deaggregator", "params": {"part": "part"}},
        {"id": "back", "type": "Deaggregator"},
        {"id": "set", "type": "CrsSetter", "params": {"crs": "EPSG:3007"}},
        {"id": "unset", "type": "CrsSetter"}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["by", "Input"]},
        {"from": ["in", "Output"], "to": ["all", "Input"]},
        {"from": ["in", "Output"], "to": ["parts", "Input"]},
        {"from": ["by", "Aggregated"], "to": ["back", "Input"]},
        {"from": ["in", "Output"], "to": ["set", "Input"]},
        {"from": ["in", "Output"], "to": ["unset", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // By kind: the bus points become one multipoint and the bus line its own feature,
    // since points and lines do not go into one geometry.
    let by = r.output("by", "Aggregated").unwrap();
    assert_eq!(by.field_names(), ["kind", "count"]);
    let rows: Vec<(String, i64, &str)> = by
        .features
        .iter()
        .map(|f| (f.attrs[0].to_text(), f.attrs[1].as_f64().unwrap() as i64, f.geom.as_ref().map_or("none", og_engine::geom::type_name)))
        .collect();
    assert_eq!(
        rows,
        [("bus".to_string(), 2, "MultiPoint"), ("bus".to_string(), 1, "MultiLineString"), ("tram".to_string(), 2, "MultiPoint"), (String::new(), 1, "none")]
    );
    let Some(geo::Geometry::MultiPoint(m)) = &by.features[2].geom else { panic!() };
    assert_eq!(m.0.len(), 3, "a multipoint that comes in adds all its points");
    assert!(r.nodes["by"].warnings[0].starts_with("1 groups held more than one kind of geometry"));
    let all = r.output("all", "Aggregated").unwrap();
    assert_eq!((all.fields.len(), all.len()), (0, 3), "points, lines and no geometry");

    // Deaggregated: one feature per part, with the attributes of the whole and a number.
    let parts = r.output("parts", "Parts").unwrap();
    assert_eq!(col(parts, "id"), ints(&[1, 2, 3, 4, 5, 5, 6]));
    assert_eq!(col(parts, "part"), [Value::Int(1), Value::Int(1), Value::Int(1), Value::Int(1), Value::Int(1), Value::Int(2), Value::Null]);
    assert!(parts.features.iter().all(|f| !matches!(f.geom, Some(geo::Geometry::MultiPoint(_)))));
    assert_eq!(r.output("back", "Parts").unwrap().len(), 7, "aggregated and deaggregated gives the parts back");

    // Setting the coordinate system changes the label, not the coordinates.
    let set = r.output("set", "Output").unwrap();
    assert_eq!(set.crs, Some(3007));
    assert_eq!(set.features[0].geom, r.output("in", "Output").unwrap().features[0].geom);
    assert!(r.nodes["set"].warnings[0].contains("with the same coordinates"));
    assert!(r.nodes["unset"].error.as_ref().unwrap().starts_with("Choose the coordinate system"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn variables_are_put_in_the_settings() {
    use og_engine::run::{expand_variables, is_variable_name};
    use og_engine::Variable;
    let vars = vec![Variable { name: "year".into(), value: "2026".into() }, Variable { name: "dir".into(), value: "out".into() }];
    assert_eq!(expand_variables("$(dir)/roads_$(year).gpkg", &vars).unwrap(), "out/roads_2026.gpkg");
    assert_eq!(expand_variables("$( year )", &vars).unwrap(), "2026");
    assert_eq!(expand_variables("no variables here", &vars).unwrap(), "no variables here");
    assert_eq!(expand_variables("costs $(5 a) and $(", &vars).unwrap(), "costs $(5 a) and $(", "what is not a name is kept");
    let e = expand_variables("$(month)", &vars).unwrap_err().0;
    assert!(e.starts_with("The variable month is not defined. The model has: year, dir."), "{}", e);
    assert!(is_variable_name("_a1") && is_variable_name("år") && !is_variable_name("1a") && !is_variable_name("a b") && !is_variable_name(""));

    let dir = tmp("variables");
    std::fs::write(dir.join("t.csv"), "name;pop\nA;10\nB;30\n").unwrap();
    let mut ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "$(file)"}},
        {"id": "big", "type": "Tester", "params": {"test": "pop > $(least)"}},
        {"id": "tag", "type": "AttributeCreator", "params": {"name": "src_$(least)", "expression": "'$(file)'"}},
        {"id": "out", "type": "Writer", "params": {"path": "out/$(least).csv"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["big", "Input"]},
        {"from": ["big", "Passed"], "to": ["tag", "Input"]},
        {"from": ["tag", "Output"], "to": ["out", "Input"]}
      ],
      "variables": [{"name": "file", "value": "t.csv"}, {"name": "least", "value": "5"}]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.output("big", "Passed").unwrap().len(), 2);
    assert_eq!(r.output("tag", "Output").unwrap().field_names(), ["name", "pop", "src_5"]);
    assert_eq!(col(r.output("tag", "Output").unwrap(), "src_5"), texts(&["t.csv", "t.csv"]));
    assert!(dir.join("out/5.csv").exists());
    // A value from the outside, as from the command line, takes the place of the one in the model.
    ws.set_variable("least", "20");
    let r = run(&ws, None, &dir);
    assert_eq!(r.output("big", "Passed").unwrap().len(), 1);
    assert!(dir.join("out/20.csv").exists());
    // The model keeps its variables when it is saved.
    let again = Workspace::from_json(&ws.to_json()).unwrap();
    assert_eq!(again.variables, ws.variables);
    ws.variables.clear();
    let r = run(&ws, None, &dir);
    assert!(r.nodes["in"].error.as_ref().unwrap().starts_with("The variable file is not defined. The model has no variables."));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keep_attributes_and_remove_geometry() {
    let dir = tmp("keep");
    std::fs::write(dir.join("t.csv"), "a;b;c;d;x;y\n1;2;3;4;400000;6400000\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv", "crs": "EPSG:3006"}},
        {"id": "keep", "type": "AttributeKeeper", "params": {"keep": "d, b, zz"}},
        {"id": "none", "type": "AttributeKeeper"},
        {"id": "flat", "type": "GeometryRemover"},
        {"id": "out", "type": "Writer", "params": {"path": "flat.gpkg"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["keep", "Input"]},
        {"from": ["in", "Output"], "to": ["none", "Input"]},
        {"from": ["keep", "Output"], "to": ["flat", "Input"]},
        {"from": ["flat", "Output"], "to": ["out", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // The order of the stream is kept, whatever the order of the list.
    let k = r.output("keep", "Output").unwrap();
    assert_eq!(k.field_names(), ["b", "d"]);
    assert!(k.features[0].geom.is_some(), "the geometry is not an attribute and stays");
    assert_eq!(r.nodes["keep"].warnings, ["Not in the stream, so not kept: zz."]);
    assert!(r.nodes["none"].error.as_ref().unwrap().starts_with("Choose the attributes to keep."));
    let f = r.output("flat", "Output").unwrap();
    assert_eq!((f.crs, f.features[0].geom.is_none()), (None, true));
    assert_eq!(f.field_names(), ["b", "d"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sheets_and_csv_get_no_geometry_unless_asked_and_csv_can_be_added_to() {
    let dir = tmp("append_csv");
    std::fs::write(dir.join("a.csv"), "name;pop;wkt\nA;10;POINT (400000 6400000)\n").unwrap();
    std::fs::write(dir.join("b.csv"), "pop;name;extra\n30;B;yes\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "a", "type": "Reader", "params": {"path": "a.csv", "crs": "EPSG:3006"}},
        {"id": "b", "type": "Reader", "params": {"path": "b.csv"}},
        {"id": "plain", "type": "Writer", "params": {"path": "plain.csv"}},
        {"id": "xy", "type": "Writer", "params": {"path": "xy.csv", "geometry": "xy"}},
        {"id": "first", "type": "Writer", "params": {"path": "all.csv", "mode": "append"}}
      ],
      "edges": [
        {"from": ["a", "Output"], "to": ["plain", "Input"]},
        {"from": ["a", "Output"], "to": ["xy", "Input"]},
        {"from": ["a", "Output"], "to": ["first", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(std::fs::read_to_string(dir.join("plain.csv")).unwrap(), "name;pop\nA;10\n", "no geometry unless asked");
    assert_eq!(std::fs::read_to_string(dir.join("xy.csv")).unwrap(), "name;pop;x;y\nA;10;400000;6400000\n");
    assert_eq!(std::fs::read_to_string(dir.join("all.csv")).unwrap(), "name;pop\nA;10\n", "the first time the file is written whole");
    // The second stream has its columns in another order, and one the file lacks.
    let ws2 = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "b", "type": "Reader", "params": {"path": "b.csv"}},
        {"id": "more", "type": "Writer", "params": {"path": "all.csv", "mode": "append"}}
      ],
      "edges": [{"from": ["b", "Output"], "to": ["more", "Input"]}]
    }"#,
    )
    .unwrap();
    let r = run(&ws2, None, &dir);
    show(&r);
    assert_eq!(std::fs::read_to_string(dir.join("all.csv")).unwrap(), "name;pop\nA;10\nB;30\n");
    assert_eq!(r.nodes["more"].warnings, ["The file has no column for extra, so they were left out. Write to a new file to get them."]);
    assert!(r.log.iter().any(|l| l.msg.contains("added to what was there")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn times_are_written_in_seconds_and_minutes() {
    use og_engine::model::fmt_duration as d;
    assert_eq!(d(3.2), "0.00 s");
    assert_eq!(d(42.0), "0.04 s");
    assert_eq!(d(230.0), "0.2 s");
    assert_eq!(d(2_345.0), "2.3 s");
    assert_eq!(d(45_400.0), "45 s");
    assert_eq!(d(59_600.0), "1 min 0 s");
    assert_eq!(d(252_000.0), "4 min 12 s");
    assert_eq!(d(3_780_000.0), "1 h 3 min");
}

#[test]
fn a_run_is_stopped_where_it_is() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tmp("stop");
    std::fs::write(dir.join("small.csv"), "id;v\n1;1\n").unwrap();
    let mut big = String::from("id;v\n");
    for i in 0..60_000 {
        big.push_str(&format!("{};{}\n", i, i % 7));
    }
    std::fs::write(dir.join("big.csv"), big).unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "a", "type": "Reader", "name": "Small", "params": {"path": "small.csv"}},
        {"id": "b", "type": "Reader", "name": "Big", "params": {"path": "big.csv"}},
        {"id": "c", "type": "Sorter", "params": {"sort_by": "v"}},
        {"id": "d", "type": "Writer", "params": {"path": "out.csv"}}
      ],
      "edges": [
        {"from": ["b", "Output"], "to": ["c", "Input"]},
        {"from": ["c", "Sorted"], "to": ["d", "Input"]},
        {"from": ["a", "Output"], "to": ["d", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    // The stop comes when the big file starts to be read, as when the user presses Stop.
    let stop = AtomicBool::new(false);
    let r = og_engine::run_stoppable(
        &ws,
        None,
        &dir,
        &mut |p| {
            if let og_engine::Progress::Started { id } = p {
                if id == "b" {
                    stop.store(true, Ordering::Relaxed);
                }
            }
        },
        &stop,
    );
    show(&r);
    assert_eq!(r.nodes["a"].status, Status::Ok, "what ran before keeps its result");
    assert_eq!(r.output("a", "Output").unwrap().len(), 1);
    assert_eq!((r.nodes["b"].status, r.nodes["b"].error.as_deref()), (Status::Skipped, Some("Stopped while it ran.")));
    for id in ["c", "d"] {
        assert_eq!((r.nodes[id].status, r.nodes[id].error.as_deref()), (Status::Skipped, Some("Stopped before it ran.")), "{}", id);
    }
    assert_eq!(r.errors, 0, "stopping is not an error");
    assert!(!dir.join("out.csv").exists(), "nothing is written after the stop");
    assert!(r.log.iter().any(|l| l.msg.starts_with("Big: stopped after ")));
    assert!(r.log.last().unwrap().msg.starts_with("Run stopped, 3 nodes did not run: 4 nodes"), "{}", r.log.last().unwrap().msg);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn points_are_made_from_x_and_y() {
    let dir = tmp("points_xy");
    std::fs::write(dir.join("t.csv"), "id;v1;v2;place\n1;319758;6400326;a\n2;377605,5;6399600;b\n3;;6400000;c\n4;abc;6400000;\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "t.csv"}},
        {"id": "p", "type": "PointCreator", "params": {"x": "v1", "y": "v2", "crs": "EPSG:3006"}},
        {"id": "none", "type": "PointCreator", "params": {"x": "v1", "y": "v2"}},
        {"id": "wrong", "type": "PointCreator", "params": {"x": "east", "y": "v2", "crs": "EPSG:3006"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["p", "Input"]},
        {"from": ["in", "Output"], "to": ["none", "Input"]},
        {"from": ["in", "Output"], "to": ["wrong", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let p = r.output("p", "Points").unwrap();
    assert_eq!((p.len(), p.crs), (2, Some(3006)));
    assert_eq!(p.field_names(), ["id", "v1", "v2", "place"], "the attributes are kept");
    assert!(matches!(p.features[1].geom, Some(geo::Geometry::Point(pt)) if pt.x() == 377605.5 && pt.y() == 6399600.0));
    assert_eq!(col(r.output("p", "Failed").unwrap(), "id"), ints(&[3, 4]));
    assert!(r.nodes["p"].warnings[0].starts_with("2 features have no x or y that is a number"));
    assert!(r.nodes["none"].error.as_ref().unwrap().starts_with("Choose the coordinate system of the x and y values"));
    assert!(r.nodes["wrong"].error.as_ref().unwrap().contains("east"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_is_drawn_from_the_layers_connected_to_it() {
    let dir = tmp("map");
    std::fs::write(dir.join("areas.csv"), "name;wkt\nA;POLYGON ((0 0, 1000 0, 1000 1000, 0 1000, 0 0))\n").unwrap();
    std::fs::write(dir.join("dots.csv"), "id;wkt\n1;POINT (500 500)\n2;POINT (250 250)\n").unwrap();
    std::fs::write(dir.join("plain.csv"), "id;v\n1;2\n").unwrap();
    let ws = Workspace::from_json(
        r##"{
      "nodes": [
        {"id": "areas", "type": "Reader", "name": "Areas", "params": {"path": "areas.csv", "crs": "EPSG:3006"}},
        {"id": "dots", "type": "Reader", "name": "Dots", "params": {"path": "dots.csv", "crs": "EPSG:3006"}},
        {"id": "plain", "type": "Reader", "name": "Plain", "params": {"path": "plain.csv"}},
        {"id": "map", "type": "MapRenderer", "params": {"width": "200", "margin": "0", "layers": [
          {"from": "dots:Output", "fill": "#000000", "stroke": "none", "width": 0, "size": 20},
          {"from": "areas:Output", "fill": "#a6cee3", "stroke": "#1f78b4", "width": 2, "size": 6}
        ]}},
        {"id": "hidden", "type": "MapRenderer", "params": {"width": "100", "layers": [{"from": "dots:Output", "show": false}]}},
        {"id": "nothing", "type": "MapRenderer"},
        {"id": "wide", "type": "MapRenderer", "params": {"width": "0"}}
      ],
      "edges": [
        {"from": ["areas", "Output"], "to": ["map", "Layers"]},
        {"from": ["dots", "Output"], "to": ["map", "Layers"]},
        {"from": ["plain", "Output"], "to": ["map", "Layers"]},
        {"from": ["dots", "Output"], "to": ["hidden", "Layers"]},
        {"from": ["areas", "Output"], "to": ["wide", "Layers"]}
      ]
    }"##,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let m = r.output("map", "Map").unwrap();
    let img = m.image.as_ref().expect("the stream carries the picture");
    assert_eq!((img.width, img.height, m.crs), (200, 200, Some(3006)));
    assert_eq!(m.len(), 1, "one feature: the outline");
    assert_eq!(col(m, "width"), [Value::Int(200)]);
    let px = |x: usize, y: usize| img.rgba[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4].to_vec();
    assert_eq!(px(100, 100), [0, 0, 0, 255], "the dot is drawn over the area, as the list says");
    assert_eq!(px(150, 50), [0xa6, 0xce, 0xe3, 255]);
    // The stream without geometry is not drawn, and it is said.
    assert!(r.nodes["map"].warnings.iter().any(|w| w == "Plain has no geometry, so it is not drawn."), "{:?}", r.nodes["map"].warnings);
    assert!(r.log.iter().any(|l| l.msg.starts_with("Map: 4 in → 1 out") || l.msg.contains("drew 3 features in 2 layers, 200 × 200 pixels")), "{:?}", r.log);
    assert!(r.nodes["hidden"].error.as_ref().unwrap().starts_with("Every layer is hidden"));
    assert!(r.nodes["nothing"].error.as_ref().unwrap().starts_with("Connect the streams to draw to Layers"));
    assert!(r.nodes["wide"].error.as_ref().unwrap().starts_with("The width must be a whole number of pixels"));
    // A picture cannot be written as CSV-free: an output for an image wants one.
    let ws2 = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "plain", "type": "Reader", "params": {"path": "plain.csv"}},
        {"id": "png", "type": "Writer", "params": {"path": "out.png"}}
      ],
      "edges": [{"from": ["plain", "Output"], "to": ["png", "Input"]}]
    }"#,
    )
    .unwrap();
    let r = run(&ws2, None, &dir);
    assert!(r.nodes["png"].error.as_ref().unwrap().starts_with("The stream has no image to write as .png."), "{:?}", r.nodes["png"].error);
    // The layer list as it is stored, and new streams on top of it.
    use og_engine::nodes::{arrange_layers, map_layers};
    let stored = map_layers(&ws.nodes[3].params);
    assert_eq!(stored.len(), 2);
    let rows = arrange_layers(&stored, &["areas:Output".into(), "dots:Output".into(), "plain:Output".into()]);
    let order: Vec<&str> = rows.iter().map(|r| r.from.as_str()).collect();
    assert_eq!(order, ["plain:Output", "dots:Output", "areas:Output"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- Version 0.57 ----------

const CHAIN: &str = r#"{
  "nodes": [
    {"id": "a", "type": "Reader", "name": "Towns", "params": {"path": "towns.csv", "crs": "EPSG:3006"}},
    {"id": "big", "type": "Tester", "name": "Big", "params": {"test": "population >= 10000"}},
    {"id": "calc", "type": "AttributeCreator", "name": "Thousands", "params": {"name": "thousands", "expression": "population / 1000"}},
    {"id": "out", "type": "Writer", "name": "Big towns", "params": {"path": "big.csv"}},
    {"id": "side", "type": "Sorter", "name": "Sorted", "params": {"sort_by": "name"}}
  ],
  "edges": [
    {"from": ["a", "Output"], "to": ["big", "Input"]},
    {"from": ["big", "Passed"], "to": ["calc", "Input"]},
    {"from": ["calc", "Output"], "to": ["out", "Input"]},
    {"from": ["a", "Output"], "to": ["side", "Input"]}
  ]
}"#;

#[test]
fn a_run_is_paused_and_goes_on_when_resumed() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tmp("pause");
    std::fs::write(dir.join("towns.csv"), TOWNS).unwrap();
    let ws = Workspace::from_json(CHAIN).unwrap();
    let (stop, pause) = (AtomicBool::new(false), AtomicBool::new(false));
    let started = std::sync::Mutex::new(Vec::<String>::new());
    std::thread::scope(|s| {
        let runner = s.spawn(|| {
            og_engine::run_opts(
                &ws,
                None,
                &dir,
                &mut |p| {
                    if let og_engine::Progress::Started { id } = p {
                        started.lock().unwrap().push(id.to_string());
                        // The user pauses as the test starts.
                        if id == "big" {
                            pause.store(true, Ordering::Relaxed);
                        }
                    }
                },
                og_engine::RunOptions { stop: Some(&stop), pause: Some(&pause), ..Default::default() },
            )
        });
        // While paused, nothing more starts.
        std::thread::sleep(std::time::Duration::from_millis(400));
        let so_far = started.lock().unwrap().clone();
        assert!(so_far.len() <= 3 && so_far.contains(&"big".to_string()), "{:?}", so_far);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(started.lock().unwrap().len(), so_far.len(), "nothing ran while paused");
        assert!(!runner.is_finished());
        pause.store(false, Ordering::Relaxed);
        let r = runner.join().unwrap();
        show(&r);
        assert_eq!(r.errors, 0);
        assert_eq!(started.lock().unwrap().len(), 5, "all ran after the resume");
        assert_eq!(r.output("calc", "Output").unwrap().len(), 4);
    });
}

#[test]
fn parts_of_a_model_are_run_with_results_from_the_cache() {
    use std::collections::HashSet;
    let dir = tmp("cache");
    std::fs::write(dir.join("towns.csv"), TOWNS).unwrap();
    let mut ws = Workspace::from_json(CHAIN).unwrap();
    let first = run(&ws, None, &dir);
    assert_eq!(first.errors, 0);
    let keys = og_engine::node_keys(&ws, &dir);
    assert!(first.nodes.iter().all(|(id, r)| r.key == keys[id] && !r.cached));

    // Run from here: the node and what is after it run again, what is before it comes from
    // the cache, and the branch beside it is left out.
    let set = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
    let mut fresh = set(&["calc"]);
    fresh.extend(og_engine::downstream_of(&ws, "calc"));
    assert_eq!(fresh, set(&["calc", "out"]));
    let r = og_engine::run_opts(
        &ws,
        None,
        &dir,
        &mut |_| {},
        og_engine::RunOptions { fresh: Some(fresh.clone()), cache: Some(&first.nodes), ..Default::default() },
    );
    show(&r);
    assert!(r.nodes["a"].cached && r.nodes["big"].cached);
    assert!(!r.nodes["calc"].cached && !r.nodes["out"].cached);
    assert!(!r.nodes.contains_key("side"), "a branch that is not needed is left out");
    assert!(r.log.iter().any(|l| l.msg == "Towns: 5 out, from the cache"), "{:?}", r.log.iter().map(|l| &l.msg).collect::<Vec<_>>());
    assert!(r.log.iter().any(|l| l.msg == "Big: Passed 4, Failed 1 out, from the cache"));
    assert!(r.log.last().unwrap().msg.starts_with("Run finished: 4 nodes"), "{}", r.log.last().unwrap().msg);

    // A changed setting upstream means that node runs again, and so does what follows it.
    ws.nodes.iter_mut().find(|n| n.id == "big").unwrap().params.insert("test".into(), serde_json::json!("population >= 50000"));
    let r = og_engine::run_opts(
        &ws,
        None,
        &dir,
        &mut |_| {},
        og_engine::RunOptions { fresh: Some(fresh.clone()), cache: Some(&first.nodes), ..Default::default() },
    );
    assert!(r.nodes["a"].cached && !r.nodes["big"].cached);
    assert_eq!(r.output("calc", "Output").unwrap().len(), 2);

    // A changed file is read again.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(dir.join("towns.csv"), TOWNS.lines().take(3).collect::<Vec<_>>().join("\n")).unwrap();
    let r = og_engine::run_opts(&ws, None, &dir, &mut |_| {}, og_engine::RunOptions { fresh: Some(fresh), cache: Some(&first.nodes), ..Default::default() });
    assert!(!r.nodes["a"].cached);
    assert_eq!(r.output("a", "Output").unwrap().len(), 2);

    // Run to here: the node and what it needs, nothing after it.
    let r = og_engine::run_opts(&ws, None, &dir, &mut |_| {}, og_engine::RunOptions { fresh: Some(set(&["big"])), ..Default::default() });
    let mut ran: Vec<&String> = r.nodes.keys().collect();
    ran.sort();
    assert_eq!(ran, ["a", "big"]);
    assert_eq!(og_engine::upstream_of(&ws, &set(&["out"])), set(&["a", "big", "calc"]));
}

#[test]
fn the_log_says_how_much_each_output_wrote() {
    let dir = tmp("written");
    std::fs::write(dir.join("towns.csv"), TOWNS).unwrap();
    let r = run(&Workspace::from_json(CHAIN).unwrap(), None, &dir);
    show(&r);
    let msgs: Vec<&str> = r.log.iter().map(|l| l.msg.as_str()).collect();
    assert!(msgs.iter().any(|m| m.starts_with("Big towns: 4 features written → big.csv")), "{:?}", msgs);
    let n = msgs.len();
    assert_eq!(msgs[n - 2], "Big towns wrote 4 features to big.csv");
}

#[test]
fn geometry_is_scaled_moved_and_made_2d() {
    let dir = tmp("scale");
    std::fs::write(dir.join("p.csv"), "id;x;y\n1;1000;2000\n2;3000;4000\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "p.csv", "crs": "EPSG:3006"}},
        {"id": "mm", "type": "Scaler", "params": {"factor": "0,001"}},
        {"id": "each", "type": "Scaler", "params": {"factor": "2", "factor_y": "3", "around": "all"}},
        {"id": "zero", "type": "Scaler", "params": {"factor": "0"}},
        {"id": "move", "type": "Offsetter", "params": {"dx": "-500", "dy": "100.5"}},
        {"id": "flat", "type": "Force2D", "params": {}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["mm", "Input"]},
        {"from": ["in", "Output"], "to": ["each", "Input"]},
        {"from": ["in", "Output"], "to": ["zero", "Input"]},
        {"from": ["in", "Output"], "to": ["move", "Input"]},
        {"from": ["in", "Output"], "to": ["flat", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let xy = |id: &str| -> Vec<(f64, f64)> {
        r.output(id, "Output")
            .unwrap()
            .features
            .iter()
            .map(|f| match f.geom.as_ref().unwrap() {
                geo::Geometry::Point(p) => ((p.x() * 1000.0).round() / 1000.0, (p.y() * 1000.0).round() / 1000.0),
                g => panic!("{:?}", g),
            })
            .collect()
    };
    assert_eq!(xy("mm"), [(1.0, 2.0), (3.0, 4.0)]);
    // Around the middle of all: (2000, 3000) stays.
    assert_eq!(xy("each"), [(0.0, 0.0), (4000.0, 6000.0)]);
    assert_eq!(xy("move"), [(500.0, 2100.5), (2500.0, 4100.5)]);
    assert_eq!(xy("flat"), [(1000.0, 2000.0), (3000.0, 4000.0)]);
    assert_eq!(r.nodes["zero"].status, Status::Error);
    assert!(r.log.iter().any(|l| l.msg.contains("scaled 0.001 times")));
    assert!(r.log.iter().any(|l| l.msg.contains("the geometry was 2D already")));
    assert!(r.log.iter().any(|l| l.level == Level::Info && l.msg.contains("moved -500 units along x and 100.5 along y")));
}

// ---------- Version 0.58 ----------

#[test]
fn features_are_numbered_from_a_start_and_per_group() {
    let dir = tmp("count");
    std::fs::write(dir.join("towns.csv"), TOWNS).unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "a", "type": "Reader", "params": {"path": "towns.csv", "crs": "EPSG:3006"}},
        {"id": "all", "type": "Counter", "params": {}},
        {"id": "per", "type": "Counter", "params": {"name": "nr", "start": "10", "group_by": "municipality"}},
        {"id": "bad", "type": "Counter", "params": {"start": "one"}}
      ],
      "edges": [
        {"from": ["a", "Output"], "to": ["all", "Input"]},
        {"from": ["a", "Output"], "to": ["per", "Input"]},
        {"from": ["a", "Output"], "to": ["bad", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(col(r.output("all", "Output").unwrap(), "number"), ints(&[1, 2, 3, 4, 5]));
    // Every municipality is its own group here, so each starts at 10.
    assert_eq!(col(r.output("per", "Output").unwrap(), "nr"), ints(&[10, 10, 10, 10, 10]));
    assert!(r.nodes["bad"].error.as_deref().unwrap().contains("whole number"));
}

fn geoms(r: &og_engine::RunResult, id: &str, port: &str) -> Vec<geo::Geometry<f64>> {
    r.output(id, port).unwrap().features.iter().map(|f| f.geom.clone().unwrap()).collect()
}

#[test]
fn boxes_rings_lines_and_longer_lines() {
    use geo::{Area, Euclidean, Length};
    let dir = tmp("shapes");
    std::fs::write(
        dir.join("g.csv"),
        "id;kind;wkt\n\
         1;a;POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 4 2, 4 4, 2 4, 2 2))\n\
         2;a;LINESTRING (0 0, 3 4)\n\
         3;b;POINT (20 20)\n\
         4;b;MULTILINESTRING ((0 0, 10 0, 10 10), (10 10, 0 10, 0 0.05))\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "g.csv", "crs": "EPSG:3006"}},
        {"id": "each", "type": "BoundingBoxReplacer", "params": {"corners": "true"}},
        {"id": "all", "type": "BoundingBoxAccumulator", "params": {}},
        {"id": "per", "type": "BoundingBoxAccumulator", "params": {"group_by": "kind", "count": "n"}},
        {"id": "rings", "type": "PolygonToLine", "params": {}},
        {"id": "areas", "type": "LineToPolygon", "params": {"tolerance": "0.1"}},
        {"id": "strict", "type": "LineToPolygon", "params": {}},
        {"id": "longer", "type": "LineExtender", "params": {"distance": "1"}},
        {"id": "end", "type": "LineExtender", "params": {"distance": "5", "ends": "end"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["each", "Input"]},
        {"from": ["in", "Output"], "to": ["all", "Input"]},
        {"from": ["in", "Output"], "to": ["per", "Input"]},
        {"from": ["in", "Output"], "to": ["rings", "Input"]},
        {"from": ["in", "Output"], "to": ["areas", "Input"]},
        {"from": ["in", "Output"], "to": ["strict", "Input"]},
        {"from": ["in", "Output"], "to": ["longer", "Input"]},
        {"from": ["in", "Output"], "to": ["end", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    let each = r.output("each", "Output").unwrap();
    assert_eq!(geo::Geometry::Polygon(geo::Rect::new((0., 0.), (3., 4.)).to_polygon()), geoms(&r, "each", "Output")[1]);
    assert_eq!(geoms(&r, "each", "Output")[2], geo::Geometry::Point(geo::Point::new(20., 20.)));
    assert_eq!(col(each, "xmax"), reals(&[10.0, 3.0, 20.0, 10.0]));
    let all = r.output("all", "Output").unwrap();
    assert_eq!((all.len(), col(all, "count")), (1, ints(&[4])));
    assert_eq!(geoms(&r, "all", "Output")[0].unsigned_area(), 400.0);
    let per = r.output("per", "Output").unwrap();
    assert_eq!((col(per, "kind"), col(per, "n")), (texts(&["a", "b"]), ints(&[2, 2])));
    // A polygon with a hole becomes two rings; the rest are not areas.
    match &geoms(&r, "rings", "Lines")[0] {
        geo::Geometry::MultiLineString(m) => assert_eq!(m.0.len(), 2),
        g => panic!("{:?}", g),
    }
    assert_eq!(r.output("rings", "Other").unwrap().len(), 3);
    // Two parts that meet, and nearly close, become one area within the tolerance.
    assert_eq!(r.output("areas", "Polygons").unwrap().len(), 1);
    assert!((geoms(&r, "areas", "Polygons")[0].unsigned_area() - 100.0).abs() < 1e-9);
    assert_eq!(r.output("strict", "Polygons").unwrap().len(), 0);
    assert_eq!(r.output("strict", "Failed").unwrap().len(), 4);
    // The line from 0 0 to 3 4 is 5 long; one more at each end makes 7.
    assert!(
        (Euclidean.length(match &geoms(&r, "longer", "Output")[1] {
            geo::Geometry::LineString(l) => l,
            g => panic!("{:?}", g),
        }) - 7.0)
            .abs()
            < 1e-9
    );
    match &geoms(&r, "end", "Output")[1] {
        geo::Geometry::LineString(l) => assert_eq!((l.0[0], l.0[1]), (geo::coord! { x: 0., y: 0. }, geo::coord! { x: 6., y: 8. })),
        g => panic!("{:?}", g),
    }
    assert!(r.nodes["longer"].warnings.iter().any(|w| w.contains("are not lines")));
}

#[test]
fn thiessen_polygons_share_the_area_between_points() {
    use geo::{Area, Contains};
    let dir = tmp("thiessen");
    std::fs::write(dir.join("p.csv"), "id;x;y\n1;0;0\n2;10;0\n3;0;10\n4;10;10\n5;5;5\n6;5;5\n").unwrap();
    std::fs::write(dir.join("b.csv"), "id;wkt\n1;POLYGON ((-5 -5, 15 -5, 15 15, -5 15, -5 -5))\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "p", "type": "Reader", "params": {"path": "p.csv", "crs": "EPSG:3006"}},
        {"id": "b", "type": "Reader", "params": {"path": "b.csv", "crs": "EPSG:3006"}},
        {"id": "box", "type": "VoronoiDiagrammer", "params": {"margin": "50"}},
        {"id": "cut", "type": "VoronoiDiagrammer", "params": {}}
      ],
      "edges": [
        {"from": ["p", "Output"], "to": ["box", "Points"]},
        {"from": ["p", "Output"], "to": ["cut", "Points"]},
        {"from": ["b", "Output"], "to": ["cut", "Boundary"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    for id in ["box", "cut"] {
        let t = r.output(id, "Polygons").unwrap();
        assert_eq!(t.len(), 6, "{}", id);
        assert_eq!(col(t, "id"), ints(&[1, 2, 3, 4, 5, 6]));
        // The cells cover the area to clip to, 20 × 20, once; two points share one cell.
        let total: f64 = t.features.iter().take(5).map(|f| f.geom.as_ref().unwrap().unsigned_area()).sum();
        assert!((total - 400.0).abs() < 1e-6, "{} {}", id, total);
        // Each cell holds its own point.
        let pts = [(0., 0.), (10., 0.), (0., 10.), (10., 10.), (5., 5.)];
        for (f, (x, y)) in t.features.iter().zip(pts) {
            assert!(f.geom.as_ref().unwrap().contains(&geo::Point::new(x, y)), "{} {} {}", id, x, y);
        }
    }
    assert!(r.nodes["box"].warnings.iter().any(|w| w.contains("share its area")));
}

#[test]
fn nearest_split_overlay_map_and_grid() {
    use geo::{Area, Length};
    let dir = tmp("tools59");
    std::fs::write(dir.join("homes.csv"), "id;wkt\n1;POINT (1 1)\n2;POINT (9 0)\n3;POINT (100 100)\n").unwrap();
    std::fs::write(dir.join("centres.csv"), "name;kind;wkt\nNorth;1;POINT (0 3)\nEast;2;LINESTRING (10 -5, 10 5)\nOther;9;POINT (50 50)\n").unwrap();
    std::fs::write(dir.join("roads.csv"), "road;wkt\nA;LINESTRING (0 0, 10 0)\nB;LINESTRING (5 -5, 5 5)\nC;LINESTRING (20 0, 30 0)\n").unwrap();
    std::fs::write(dir.join("stops.csv"), "s;wkt\n1;POINT (2 0.001)\n2;POINT (25 0)\n3;POINT (50 50)\n").unwrap();
    std::fs::write(dir.join("a.csv"), "a;wkt\nleft;POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))\n").unwrap();
    std::fs::write(dir.join("b.csv"), "a;b;wkt\nX;right;POLYGON ((5 0, 15 0, 15 10, 5 10, 5 0))\nY;far;POLYGON ((50 50, 60 50, 60 60, 50 60, 50 50))\n")
        .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "homes", "type": "Reader", "params": {"path": "homes.csv", "crs": "EPSG:3006"}},
        {"id": "centres", "type": "Reader", "params": {"path": "centres.csv", "crs": "EPSG:3006"}},
        {"id": "near", "type": "NeighborFinder", "params": {"max_distance": "20", "attributes": ["name"], "prefix": "c_"}},
        {"id": "roads", "type": "Reader", "params": {"path": "roads.csv", "crs": "EPSG:3006"}},
        {"id": "stops", "type": "Reader", "params": {"path": "stops.csv", "crs": "EPSG:3006"}},
        {"id": "split", "type": "LineSplitter", "params": {"part": "part"}},
        {"id": "only_points", "type": "LineSplitter", "params": {"at": "points"}},
        {"id": "a", "type": "Reader", "params": {"path": "a.csv", "crs": "EPSG:3006"}},
        {"id": "b", "type": "Reader", "params": {"path": "b.csv", "crs": "EPSG:3006"}},
        {"id": "over", "type": "AreaOnAreaOverlayer", "params": {"prefix": "o_"}},
        {"id": "map", "type": "ValueMapper", "params": {"attribute": "kind", "map": [["1", "Health centre"], ["2", "Hospital"]], "otherwise": "text", "default": "Other", "result": "what"}},
        {"id": "nums", "type": "ValueMapper", "params": {"attribute": "name", "map": [["North", "10"], ["East", "20"]], "otherwise": "empty"}},
        {"id": "grid", "type": "GridCreator", "params": {"size": "4"}},
        {"id": "all", "type": "GridCreator", "params": {"size": "4", "keep": "all"}},
        {"id": "hex", "type": "GridCreator", "params": {"size": "4", "shape": "hexagon"}}
      ],
      "edges": [
        {"from": ["homes", "Output"], "to": ["near", "Base"]},
        {"from": ["centres", "Output"], "to": ["near", "Candidates"]},
        {"from": ["roads", "Output"], "to": ["split", "Lines"]},
        {"from": ["stops", "Output"], "to": ["split", "Points"]},
        {"from": ["roads", "Output"], "to": ["only_points", "Lines"]},
        {"from": ["stops", "Output"], "to": ["only_points", "Points"]},
        {"from": ["a", "Output"], "to": ["over", "Areas"]},
        {"from": ["b", "Output"], "to": ["over", "Overlay"]},
        {"from": ["centres", "Output"], "to": ["map", "Input"]},
        {"from": ["centres", "Output"], "to": ["nums", "Input"]},
        {"from": ["a", "Output"], "to": ["grid", "Extent"]},
        {"from": ["roads", "Output"], "to": ["all", "Extent"]},
        {"from": ["a", "Output"], "to": ["hex", "Extent"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);

    // Find nearest: (1 1) is 2.24 from North, (9 0) is 1 from the line East, and (100 100)
    // has nothing within 20.
    let near = r.output("near", "Matched").unwrap();
    assert_eq!(col(near, "c_name"), texts(&["North", "East"]));
    let d = col(near, "distance");
    assert!((d[0].as_f64().unwrap() - 5f64.sqrt()).abs() < 1e-9 && (d[1].as_f64().unwrap() - 1.0).abs() < 1e-9, "{:?}", d);
    assert_eq!(r.output("near", "Unmatched").unwrap().len(), 1);

    // Split lines: A is split where B crosses it and at the stop near (2 0); B where A
    // crosses it; C at its stop. The stop at (50 50) is on no line.
    let split = r.output("split", "Lines").unwrap();
    assert_eq!(col(split, "road"), texts(&["A", "A", "A", "B", "B", "C", "C"]));
    assert_eq!(col(split, "part"), ints(&[1, 2, 3, 1, 2, 1, 2]));
    let lens: Vec<f64> = geoms(&r, "split", "Lines")
        .iter()
        .map(|g| match g {
            geo::Geometry::LineString(l) => geo::Euclidean.length(l),
            _ => panic!("not a line"),
        })
        .collect();
    assert!((lens[0] - 2.0).abs() < 0.01 && (lens[1] - 3.0).abs() < 0.01 && (lens[2] - 5.0).abs() < 1e-9, "{:?}", lens);
    assert_eq!(r.output("only_points", "Lines").unwrap().len(), 5, "A at its stop, B whole, C at its stop");

    // Overlay areas: the halves that overlap, and the halves that do not.
    let both = r.output("over", "Both").unwrap();
    assert_eq!(both.len(), 1);
    assert_eq!(col(both, "o_b"), texts(&["right"]));
    assert_eq!(col(both, "a"), texts(&["left"]), "the Areas keep their own attribute");
    assert!((geoms(&r, "over", "Both")[0].unsigned_area() - 50.0).abs() < 1e-6);
    assert!((geoms(&r, "over", "Areas only")[0].unsigned_area() - 50.0).abs() < 1e-6);
    let b_only = r.output("over", "Overlay only").unwrap();
    assert_eq!(col(b_only, "b"), texts(&["right", "far"]));
    assert!((geoms(&r, "over", "Overlay only")[1].unsigned_area() - 100.0).abs() < 1e-6);

    // Map values: text results with a text for the rest, and numbers stay numbers.
    assert_eq!(col(r.output("map", "Output").unwrap(), "what"), texts(&["Health centre", "Hospital", "Other"]));
    assert_eq!(col(r.output("nums", "Output").unwrap(), "name"), vec![Value::Int(10), Value::Int(20), Value::Null]);

    // Create grid: squares of 4 from 0, over 0..10, that touch the square: 3 × 3. All cells
    // over the roads, from 0 to 32 across and -8 to 8 up and down: 8 × 4.
    let grid = r.output("grid", "Grid").unwrap();
    assert_eq!(grid.len(), 9);
    assert_eq!(col(grid, "cell_id")[8], Value::Int(9));
    assert!(geoms(&r, "grid", "Grid").iter().all(|g| (g.unsigned_area() - 16.0).abs() < 1e-9));
    assert_eq!(r.output("all", "Grid").unwrap().len(), 8 * 4);
    let hex = geoms(&r, "hex", "Grid");
    assert!(!hex.is_empty());
    // A hexagon 4 across the flats has an area of √3/2 · 4².
    assert!(hex.iter().all(|g| (g.unsigned_area() - 3f64.sqrt() / 2.0 * 16.0).abs() < 1e-6));
    // The hexagons cover the whole square, without gaps.
    let cells: Vec<geo::Polygon<f64>> = hex.iter().map(|g| if let geo::Geometry::Polygon(p) = g { p.clone() } else { panic!() }).collect();
    let square = geo::Rect::new((0., 0.), (10., 10.)).to_polygon();
    let covered = geo::BooleanOps::intersection(&geo::unary_union(cells.iter()), &geo::MultiPolygon::new(vec![square]));
    assert!((covered.unsigned_area() - 100.0).abs() < 1e-6, "{}", covered.unsigned_area());
}

#[test]
fn checks_coordinates_first_ones_and_sorting() {
    let dir = tmp("tools61");
    std::fs::write(
        dir.join("sites.csv"),
        "id;name;code;kind;kommun;wkt\n\
         1;Borås;14900;A;Borås;POINT (377605.4 6399600.66)\n\
         2;Kinna;14630;B;Mark;POINT (362270 6376250)\n\
         3;;12;A;Mark;LINESTRING (0 0, 10 0)\n\
         4;Herrljunga;1466x;C;Herrljunga;POINT (383560 6439160)\n\
         5;Alingsås;1489;A;Alingsås;POLYGON ((0 0, 4 0, 4 4, 0 4, 0 0))\n\
         6;Skene;14631;B;Mark;POINT (1 2)\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "sites.csv", "crs": "EPSG:3006"}},
        {"id": "att", "type": "AttributeValidator", "params": {"checks": [
            {"attribute": "name", "required": true},
            {"attribute": "code", "type": "integer", "max_length": "5", "min": "100"},
            {"attribute": "kind", "values": "A, B"}]}},
        {"id": "xy", "type": "CoordinateExtractor", "params": {"decimals": "0", "x_name": "e", "y_name": "n"}},
        {"id": "first", "type": "CoordinateExtractor", "params": {"other": "first"}},
        {"id": "take", "type": "Sampler", "params": {"count": "2"}},
        {"id": "per", "type": "Sampler", "params": {"count": "1", "group_by": ["kommun"]}},
        {"id": "sort", "type": "Sorter", "params": {"sort_keys": [["kommun", "ascending"], ["id", "descending"]]}},
        {"id": "old", "type": "Sorter", "params": {"sort_by": "kind, id", "order": "descending"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["att", "Input"]},
        {"from": ["in", "Output"], "to": ["xy", "Input"]},
        {"from": ["in", "Output"], "to": ["first", "Input"]},
        {"from": ["in", "Output"], "to": ["take", "Input"]},
        {"from": ["in", "Output"], "to": ["per", "Input"]},
        {"from": ["in", "Output"], "to": ["sort", "Input"]},
        {"from": ["in", "Output"], "to": ["old", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    // Check attributes: 3 has no name and a code below 100, 4 a code that is not a number
    // and a kind not in the list, 6 is fine, and 1 too.
    let failed = r.output("att", "Failed").unwrap();
    assert_eq!(col(failed, "id"), ints(&[3, 4]));
    let problems = col(failed, "problems");
    assert_eq!(problems[0], text("name is empty; code is less than 100"));
    assert_eq!(problems[1], text("code is not a whole number; kind is C, which is not one of A, B"));
    assert_eq!(col(r.output("att", "Passed").unwrap(), "id"), ints(&[1, 2, 5, 6]));
    // Extract coordinates: points as they are, rounded; a line and an area by a point on them.
    let xy = r.output("xy", "Output").unwrap();
    assert_eq!(col(xy, "e")[0], Value::Real(377605.0));
    assert_eq!(col(xy, "n")[0], Value::Real(6399601.0));
    assert_eq!(col(r.output("first", "Output").unwrap(), "x")[2], Value::Real(0.0));
    assert_eq!(col(xy, "e")[4], Value::Real(2.0));
    // Take first: the first two, and the first of each municipality.
    assert_eq!(col(r.output("take", "Taken").unwrap(), "id"), ints(&[1, 2]));
    assert_eq!(r.output("take", "Rest").unwrap().len(), 4);
    assert_eq!(col(r.output("per", "Taken").unwrap(), "id"), ints(&[1, 2, 4, 5]));
    // Sort: by municipality up, and within it by id down. A model from before sorts as it did.
    assert_eq!(col(r.output("sort", "Sorted").unwrap(), "id"), ints(&[5, 1, 4, 6, 3, 2]));
    assert_eq!(col(r.output("old", "Sorted").unwrap(), "id"), ints(&[4, 6, 2, 5, 3, 1]));
}

#[test]
fn pictures_are_put_together() {
    let dir = tmp("mosaic");
    std::fs::write(dir.join("a.csv"), "id;wkt\n1;POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0))\n").unwrap();
    std::fs::write(dir.join("b.csv"), "id;wkt\n1;POLYGON ((50 50, 200 50, 200 150, 50 150, 50 50))\n").unwrap();
    let ws = Workspace::from_json(
        r##"{
      "nodes": [
        {"id": "a", "type": "Reader", "params": {"path": "a.csv", "crs": "EPSG:3006"}},
        {"id": "b", "type": "Reader", "params": {"path": "b.csv", "crs": "EPSG:3006"}},
        {"id": "ma", "type": "MapRenderer", "params": {"width": "100", "margin": "0", "background": "none", "layers": [{"from": "a:Output", "fill": "#ff0000", "stroke": "none"}]}},
        {"id": "mb", "type": "MapRenderer", "params": {"width": "150", "margin": "0", "background": "none", "layers": [{"from": "b:Output", "fill": "#0000ff", "stroke": "none"}]}},
        {"id": "mos", "type": "RasterMosaicker", "params": {}},
        {"id": "both", "type": "RasterMosaicker", "params": {}}
      ],
      "edges": [
        {"from": ["a", "Output"], "to": ["ma", "Layers"]},
        {"from": ["b", "Output"], "to": ["mb", "Layers"]},
        {"from": ["ma", "Map"], "to": ["mos", "Rasters"]},
        {"from": ["mb", "Map"], "to": ["mos", "Rasters"]},
        {"from": ["a", "Output"], "to": ["both", "Rasters"]}
      ]
    }"##,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let img = r.output("mos", "Raster").unwrap().image.clone().unwrap();
    // 200 × 150 units in pixels of 1, with red on top where they overlap, blue beside it
    // and nothing where neither is.
    assert_eq!((img.width, img.height), (200, 150));
    let px = |x: usize, y: usize| &img.rgba[(y * img.width + x) * 4..][..4];
    assert_eq!(px(75, 75), [255, 0, 0, 255]);
    assert_eq!(px(150, 75), [0, 0, 255, 255]);
    assert_eq!(px(150, 140)[3], 0, "below the blue, which starts at 50, is nothing");
    assert_eq!(px(5, 5)[3], 0);
    assert!(r.nodes["both"].error.as_deref().unwrap().contains("no picture or grid"), "{:?}", r.nodes["both"].error);
}

#[test]
fn ranges_snapping_and_charts() {
    let dir = tmp("tools62");
    std::fs::write(
        dir.join("areas.csv"),
        "name;area;wkt\nBorås;915,2;POINT (0.4 0.3)\nMark;935,7;POINT (10 0.8)\nÅmål;542,1;POINT (50 50)\nAle;317,6;POINT (20 3)\nMark;10;POINT (5 5)\nTom;;POINT (1 1)\n",
    )
    .unwrap();
    std::fs::write(dir.join("fixed.csv"), "id;wkt\n1;LINESTRING (0 0, 20 0)\n").unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "areas.csv", "crs": "EPSG:3006"}},
        {"id": "fix", "type": "Reader", "params": {"path": "fixed.csv", "crs": "EPSG:3006"}},
        {"id": "own", "type": "RangeMapper", "params": {"attribute": "area", "ranges": [["", "500", "small"], ["500", "900", "medium"], ["900", "", "large"]], "otherwise": "none"}},
        {"id": "eq", "type": "RangeMapper", "params": {"attribute": "area", "method": "equal", "classes": "2", "result": "half"}},
        {"id": "vx", "type": "Snapper", "params": {"tolerance": "1"}},
        {"id": "ed", "type": "Snapper", "params": {"tolerance": "1", "to": "edges"}},
        {"id": "chart", "type": "ChartDrawer", "params": {"category": "name", "value": "area", "sort": "name", "title": "Area", "unit": "km²", "width": "600"}},
        {"id": "count", "type": "ChartDrawer", "params": {"category": "name", "limit": "2"}},
        {"id": "bad", "type": "ChartDrawer", "params": {"category": "nothing"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["own", "Input"]},
        {"from": ["in", "Output"], "to": ["eq", "Input"]},
        {"from": ["in", "Output"], "to": ["vx", "Input"]},
        {"from": ["fix", "Output"], "to": ["vx", "Fixed"]},
        {"from": ["in", "Output"], "to": ["ed", "Input"]},
        {"from": ["fix", "Output"], "to": ["ed", "Fixed"]},
        {"from": ["in", "Output"], "to": ["chart", "Input"]},
        {"from": ["in", "Output"], "to": ["count", "Input"]},
        {"from": ["in", "Output"], "to": ["bad", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    // Classify by range: the first range that fits, both ends in it, and the empty value outside.
    assert_eq!(col(r.output("own", "Output").unwrap(), "class"), texts(&["large", "large", "medium", "small", "small", "none"]));
    let half = col(r.output("eq", "Output").unwrap(), "half");
    assert_eq!(half[0], half[1]);
    assert_ne!(half[0], half[4]);
    // Snap to vertices: only Borås lies within 1 of an end of the line.
    let vx = geoms(&r, "vx", "Snapped");
    assert_eq!(vx[0], geo::Geometry::Point(geo::Point::new(0.0, 0.0)));
    assert_eq!(vx[1], geo::Geometry::Point(geo::Point::new(10.0, 0.8)));
    // To edges: Mark at 0.8 from the line goes onto it; Ale at 3 stays.
    let ed = geoms(&r, "ed", "Snapped");
    assert_eq!(ed[1], geo::Geometry::Point(geo::Point::new(10.0, 0.0)));
    assert_eq!(ed[3], geo::Geometry::Point(geo::Point::new(20.0, 3.0)));
    // Draw chart: four names (Mark twice is summed, Tom has no number), a picture with no place.
    let chart = r.output("chart", "Chart").unwrap();
    let img = chart.image.clone().unwrap();
    assert_eq!(img.width, 600);
    assert!(chart.crs.is_none());
    assert!(r.log.iter().any(|l| l.msg.contains("4 bars")), "{:?}", r.log.iter().map(|l| &l.msg).collect::<Vec<_>>());
    assert!(r.log.iter().any(|l| l.msg.contains("2 bars")));
    assert!(r.nodes["bad"].error.is_some());
}

#[test]
fn json_is_read_flattened_exploded_extracted_and_built() {
    let dir = tmp("json63");
    // Shaped as the open data of Västra Götalandsregionen: a list in results, a byte order
    // mark before the first key, and the northing in xkoord. The places are made up.
    std::fs::write(
        dir.join("units.json"),
        "{\"resultCount\": 3, \"offset\": 0, \"results\": [\
         {\"\u{feff}id\": \"1\", \"namn\": \"Exempelsjukhuset\", \"xkoord\": 6400000.5, \"ykoord\": 320000.25, \"postnummer\": \"41345\", \"svar\": \"{\\\"adress\\\": {\\\"ort\\\": \\\"Borås\\\"}, \\\"mottagningar\\\": [{\\\"namn\\\": \\\"Hud\\\", \\\"tel\\\": \\\"010-1\\\"}, {\\\"namn\\\": \\\"Öga\\\"}]}\"},\
         {\"\u{feff}id\": \"2\", \"namn\": \"Provhälsan\", \"xkoord\": 6410000, \"ykoord\": 330000, \"postnummer\": \"05123\", \"svar\": \"{\\\"adress\\\": {\\\"ort\\\": \\\"Skövde\\\"}, \\\"mottagningar\\\": []}\"},\
         {\"\u{feff}id\": \"3\", \"namn\": \"Trasig\", \"xkoord\": 6420000, \"ykoord\": 340000, \"postnummer\": \"\", \"svar\": \"inte json\"}]}",
    )
    .unwrap();
    std::fs::write(dir.join("plain.geojson"), r#"{"type": "FeatureCollection", "features": []}"#).unwrap();
    let ws = Workspace::from_json(
        r#"{
      "nodes": [
        {"id": "in", "type": "Reader", "params": {"path": "units.json"}},
        {"id": "at", "type": "Reader", "params": {"path": "units.json", "json_records": "results[0]"}},
        {"id": "bad", "type": "Reader", "params": {"path": "units.json", "json_records": "data"}},
        {"id": "pts", "type": "PointCreator", "params": {"x": "ykoord", "y": "xkoord", "crs": "EPSG:3006"}},
        {"id": "flat", "type": "JsonFlattener", "params": {"attribute": "svar"}},
        {"id": "explode", "type": "JsonExploder", "params": {"attribute": "svar", "path": "mottagningar", "index_attr": "nr", "prefix": "mott"}},
        {"id": "extract", "type": "JsonExtractor", "params": {"attribute": "svar", "paths": [["adress.ort", ""], ["mottagningar[0].tel", "tel"], ["saknas.helt", ""]]}},
        {"id": "build", "type": "JsonBuilder", "params": {"attributes": ["namn", "mott.namn"], "result": "ut", "geometry": "geojson"}}
      ],
      "edges": [
        {"from": ["in", "Output"], "to": ["pts", "Input"]},
        {"from": ["in", "Output"], "to": ["flat", "Input"]},
        {"from": ["in", "Output"], "to": ["extract", "Input"]},
        {"from": ["in", "Output"], "to": ["explode", "Input"]},
        {"from": ["explode", "Exploded"], "to": ["build", "Input"]}
      ]
    }"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let t = r.output("in", "Output").unwrap();
    assert_eq!(t.field_names(), ["id", "namn", "xkoord", "ykoord", "postnummer", "svar"], "the byte order mark is gone from id");
    assert_eq!(col(t, "postnummer")[1], text("05123"), "a code in quotes keeps its zero");
    assert!(r.log.iter().any(|l| l.msg.contains("3 records from results")));
    assert_eq!(r.output("at", "Output").unwrap().len(), 1);
    assert!(r.nodes["bad"].error.as_deref().unwrap().contains("At the top it has: resultCount, offset, results"));
    // Points from the Swedish x (north) and y (east).
    let p = geoms(&r, "pts", "Points");
    assert_eq!(p[0], geo::Geometry::Point(geo::Point::new(320000.25, 6400000.5)));
    // Flatten: the object becomes adress.ort; the list stays JSON text; the broken one fails.
    let flat = r.output("flat", "Output").unwrap();
    assert_eq!(col(flat, "adress.ort"), texts(&["Borås", "Skövde"]));
    assert!(flat.field_index("svar").is_none() && flat.field_index("mottagningar").is_some());
    assert_eq!(col(r.output("flat", "Failed").unwrap(), "id"), texts(&["3"]));
    // Explode: one feature per clinic, numbered, with the attributes of the hospital.
    let ex = r.output("explode", "Exploded").unwrap();
    assert_eq!(col(ex, "mott.namn"), texts(&["Hud", "Öga"]));
    assert_eq!(col(ex, "nr"), ints(&[0, 1]));
    assert_eq!(col(ex, "namn"), texts(&["Exempelsjukhuset", "Exempelsjukhuset"]));
    assert_eq!(r.output("explode", "Other").unwrap().len(), 2, "an empty list and no JSON");
    // Extract: by path, named after the last part or as given; what is missing is empty.
    let x = r.output("extract", "Output").unwrap();
    assert_eq!(col(x, "ort"), texts(&["Borås", "Skövde"]));
    assert_eq!(col(x, "tel"), vec![text("010-1"), Value::Null]);
    assert_eq!(col(x, "helt"), vec![Value::Null, Value::Null]);
    assert_eq!(r.output("extract", "Failed").unwrap().len(), 1);
    // Build: names with dots as objects, and no geometry where there is none.
    let b = r.output("build", "Output").unwrap();
    assert_eq!(col(b, "ut")[0], text(r#"{"namn":"Exempelsjukhuset","mott":{"namn":"Hud"},"geometry":null}"#));
}

#[test]
fn maps_show_values_and_attributes_change_type() {
    let dir = tmp("thematic64");
    std::fs::write(
        dir.join("quakes.csv"),
        "id;mag;depth;region;wkt\n1;6,1;10;A;POINT (0 0)\n2;4,6;300;B;POINT (0.1 0.1)\n3;5,0;80;A;POINT (50 50)\n4;4,5;;C;POINT (100 0)\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("areas.csv"),
        "name;people;wkt\nNorth;10;POLYGON ((0 50, 50 50, 50 100, 0 100, 0 50))\nSouth;900;POLYGON ((50 0, 100 0, 100 50, 50 50, 50 0))\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r##"{
      "nodes": [
        {"id": "q", "type": "Reader", "params": {"path": "quakes.csv", "crs": "EPSG:3006"}},
        {"id": "a", "type": "Reader", "params": {"path": "areas.csv", "crs": "EPSG:3006"}},
        {"id": "map", "type": "MapRenderer", "params": {"width": "200", "margin": "0", "background": "#ffffff", "legend": "top-right", "legend_title": "Test",
          "layers": [
            {"from": "q:Output", "fill": "#ff0000", "stroke": "#000000", "width": 1, "size": 6, "size_by": "mag", "size_min": 4, "size_max": 20, "order": "large_below", "cluster": 10},
            {"from": "a:Output", "fill": "#cccccc", "stroke": "none", "width": 0, "size": 6, "color_by": "people", "ramp": "reds", "classes": 2, "method": "equal"}]}},
        {"id": "bad", "type": "MapRenderer", "params": {"layers": [{"from": "q:Output", "size_by": "nothing"}]}},
        {"id": "m", "type": "AttributeManager", "params": {
          "attributes": [{"name": "mag", "type": "text"}, {"name": "id", "action": "rename", "to": "nr", "type": "decimal"}, {"name": "region", "type": "integer"}],
          "create": [{"name": "checked", "value": "", "type": "integer"}, {"name": "note", "value": ""}, {"name": "big", "value": "mag > 5", "type": "text"}]}}
      ],
      "edges": [
        {"from": ["q", "Output"], "to": ["map", "Layers"]},
        {"from": ["a", "Output"], "to": ["map", "Layers"]},
        {"from": ["q", "Output"], "to": ["bad", "Layers"]},
        {"from": ["q", "Output"], "to": ["m", "Input"]}
      ]
    }"##,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let img = r.output("map", "Map").unwrap().image.clone().unwrap();
    let px = |x: usize, y: usize| &img.rgba[(y * img.width + x) * 4..][..4];
    // The areas get the colours of their classes: the light and the dark end of the reds.
    let (north, south) = (px(10, 60), px(115, 192));
    assert!(north[0] > 200 && north[1] > 180, "a light red for few people: {:?}", north);
    assert!(south[0] > 100 && south[1] < 60, "a dark red for many: {:?}", south);
    // The legend lies in the upper right corner, white with dark text.
    assert!((150..198).any(|x| px(x, 6) == [255, 255, 255, 255]) || (150..198).any(|x| px(x, 6)[0] > 240));
    assert!(r.nodes["bad"].error.as_deref().unwrap().contains("no attribute nothing"));
    // Manage attributes: a type for a listed attribute, a renamed one too, and new empty ones.
    let m = r.output("m", "Output").unwrap();
    assert_eq!(m.field_names()[..2], ["mag", "nr"]);
    assert_eq!(m.field_names()[m.fields.len() - 3..], ["checked", "note", "big"]);
    use og_engine::model::FieldType;
    let ty = |n: &str| m.fields[m.field_index(n).unwrap()].ty;
    assert_eq!(
        (ty("mag"), ty("nr"), ty("region"), ty("checked"), ty("note"), ty("big")),
        (FieldType::Text, FieldType::Real, FieldType::Int, FieldType::Int, FieldType::Text, FieldType::Text)
    );
    assert_eq!(col(m, "mag")[0], text("6.1"));
    assert_eq!(col(m, "nr")[1], Value::Real(2.0));
    assert_eq!(col(m, "region"), vec![Value::Null; 4], "letters are not whole numbers");
    assert_eq!(col(m, "checked"), vec![Value::Null; 4]);
    assert_eq!(col(m, "big")[0], text("1"), "true is stored as 1");
    assert!(r.nodes["m"].warnings.iter().any(|w| w.contains("region are not numbers")), "{:?}", r.nodes["m"].warnings);
}

#[test]
fn shortest_routes_go_along_the_network_to_the_nearest_hospital() {
    let dir = tmp("route65");
    // Roads in metres: along y = 0 from x 0 to 1000 in two pieces, and up x = 500 to y 1000.
    // The eastern piece may only be driven westwards.
    std::fs::write(
        dir.join("roads.csv"),
        "id;speed;oneway;wkt\n1;50;;LINESTRING (0 0, 500 0)\n2;50;against;LINESTRING (500 0, 1000 0)\n3;100;;LINESTRING (500 0, 500 1000)\n",
    )
    .unwrap();
    std::fs::write(dir.join("centres.csv"), "name;wkt\nNorth;POINT (100 10)\nFar away;POINT (5000 5000)\n").unwrap();
    std::fs::write(dir.join("hospitals.csv"), "hospital;wkt\nEast;POINT (1000 -5)\nTop;POINT (500 1000)\n").unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "roads", "type": "Reader", "params": {"path": "roads.csv", "crs": "EPSG:3006"}},
        {"id": "vc", "type": "Reader", "params": {"path": "centres.csv", "crs": "EPSG:3006"}},
        {"id": "h", "type": "Reader", "params": {"path": "hospitals.csv", "crs": "EPSG:3006"}},
        {"id": "short", "type": "ShortestRoute", "params": {}},
        {"id": "oneway", "type": "ShortestRoute", "params": {"along": "oneway is null"}},
        {"id": "quick", "type": "ShortestRoute", "params": {"cost": "time", "speed": "speed"}},
        {"id": "all", "type": "ShortestRoute", "params": {"to": "all"}}],
      "edges": [
        {"from": ["roads", "Output"], "to": ["short", "Network"]}, {"from": ["vc", "Output"], "to": ["short", "From"]}, {"from": ["h", "Output"], "to": ["short", "To"]},
        {"from": ["roads", "Output"], "to": ["oneway", "Network"]}, {"from": ["vc", "Output"], "to": ["oneway", "From"]}, {"from": ["h", "Output"], "to": ["oneway", "To"]},
        {"from": ["roads", "Output"], "to": ["quick", "Network"]}, {"from": ["vc", "Output"], "to": ["quick", "From"]}, {"from": ["h", "Output"], "to": ["quick", "To"]},
        {"from": ["roads", "Output"], "to": ["all", "Network"]}, {"from": ["vc", "Output"], "to": ["all", "From"]}, {"from": ["h", "Output"], "to": ["all", "To"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    let routes = r.output("short", "Routes").unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!((col(routes, "to_hospital")[0].clone(), col(routes, "length_m")[0].clone()), (text("East"), Value::Real(900.0)));
    assert_eq!(col(routes, "start_gap_m")[0], Value::Real(10.0));
    let unreached = r.output("short", "Unreached").unwrap();
    assert_eq!(col(unreached, "name"), [text("Far away")]);
    // Eastwards is forbidden on the eastern piece, so the hospital at the top is nearest.
    let one = r.output("oneway", "Routes").unwrap();
    assert_eq!((col(one, "to_hospital")[0].clone(), col(one, "length_m")[0].clone()), (text("Top"), Value::Real(1400.0)));
    // By time: 900 m at 50 km/h is 1.08 minutes; 400 m at 50 and 1000 m at 100 is 1.08 too,
    // so both are as quick; the minutes are what counts.
    let q = r.output("quick", "Routes").unwrap();
    assert_eq!(col(q, "minutes")[0], Value::Real(1.08));
    // To every one: a route for each pair that can be reached.
    let all = r.output("all", "Routes").unwrap();
    let mut lengths: Vec<f64> = col(all, "length_m").iter().map(|v| v.as_f64().unwrap()).collect();
    lengths.sort_by(f64::total_cmp);
    assert_eq!(lengths, [900.0, 1400.0]);
    // The route follows the roads: from the centre's place on the road to the hospital's.
    if let Some(geo::Geometry::LineString(l)) = &routes.features[0].geom {
        assert_eq!((l.0.first().unwrap().x, l.0.last().unwrap().x), (100.0, 1000.0));
    } else {
        panic!("a line");
    }
}

#[test]
fn case_is_changed_in_the_chosen_attributes() {
    let dir = tmp("case65");
    std::fs::write(dir.join("t.csv"), "name;code\nlilla EDET;ab\nDALS-ED;cd\n").unwrap();
    let node = |id: &str, case: &str| format!(r#"{{"id": "{}", "type": "CaseChanger", "params": {{"attributes": ["name"], "case": "{}"}}}}"#, id, case);
    let ws = Workspace::from_json(&format!(
        r#"{{"nodes": [{{"id": "in", "type": "Reader", "params": {{"path": "t.csv"}}}}, {}, {}, {}, {}],
          "edges": [{{"from": ["in", "Output"], "to": ["u", "Input"]}}, {{"from": ["in", "Output"], "to": ["l", "Input"]}},
                    {{"from": ["in", "Output"], "to": ["f", "Input"]}}, {{"from": ["in", "Output"], "to": ["w", "Input"]}}]}}"#,
        node("u", "upper"),
        node("l", "lower"),
        node("f", "first"),
        node("w", "words")
    ))
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    let names = |id: &str| col(r.output(id, "Output").unwrap(), "name");
    assert_eq!(names("u"), [text("LILLA EDET"), text("DALS-ED")]);
    assert_eq!(names("l"), [text("lilla edet"), text("dals-ed")]);
    assert_eq!(names("f"), [text("Lilla edet"), text("Dals-ed")]);
    assert_eq!(names("w"), [text("Lilla Edet"), text("Dals-Ed")]);
    assert_eq!(col(r.output("u", "Output").unwrap(), "code")[0], text("ab"), "other attributes stay as they are");
}

#[test]
fn a_view_shows_each_stream_connected_to_it() {
    let dir = tmp("view66");
    std::fs::write(dir.join("a.csv"), "num\n1\n2\n").unwrap();
    std::fs::write(dir.join("b.csv"), "m\nx\n").unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "a", "type": "Reader", "name": "Numbers", "params": {"path": "a.csv"}},
        {"id": "b", "type": "Reader", "name": "Letters", "params": {"path": "b.csv"}},
        {"id": "v", "type": "Viewer", "params": {}}],
      "edges": [{"from": ["a", "Output"], "to": ["v", "Input"]}, {"from": ["b", "Output"], "to": ["v", "Input"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    assert_eq!(r.output("v", "1 · Numbers").unwrap().len(), 2);
    assert_eq!(r.output("v", "2 · Letters").unwrap().field_names(), ["m"]);
    assert_eq!(r.warnings, 0);
}

#[test]
fn the_same_house_as_obj_glb_and_gltf_stands_in_the_same_place() {
    let dir = tmp("gltf68");
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/house");
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    // The .gltf has its buffer and its pictures beside it.
    for f in ["house.gltf", "house.bin"] {
        std::fs::copy(data.join(f), dir.join(f)).unwrap();
    }
    for f in ["plaster.png", "tiles.png"] {
        std::fs::copy(examples.join(f), dir.join(f)).unwrap();
    }
    let reader = |id: &str, path: std::path::PathBuf| {
        format!(
            r#"{{"id": "{}", "type": "Reader", "params": {{"path": "{}", "crs": "EPSG:3006", "obj_east": "1000", "obj_north": "2000", "obj_rotation": "30"}}}}"#,
            id,
            path.display().to_string().replace('\\', "/")
        )
    };
    let ws = Workspace::from_json(&format!(
        r#"{{"nodes": [{}, {}, {}], "edges": []}}"#,
        reader("obj", examples.join("house.obj")),
        reader("glb", examples.join("house.glb")),
        reader("gltf", dir.join("house.gltf"))
    ))
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    let mesh = |id: &str| r.output(id, "Output").unwrap().mesh.clone().unwrap();
    let (obj, glb, gltf) = (mesh("obj"), mesh("glb"), mesh("gltf"));
    assert_eq!((glb.tris.len(), gltf.tris.len()), (obj.tris.len(), obj.tris.len()));
    // Every corner of a triangle of the glTF is a corner of the OBJ, placed alike.
    let key = |p: [f64; 3]| ((p[0] * 1000.0).round() as i64, (p[1] * 1000.0).round() as i64, (p[2] * 1000.0).round() as i64);
    let corners: std::collections::HashSet<_> = obj.positions.iter().map(|p| key(*p)).collect();
    for m in [&glb, &gltf] {
        assert!(m.positions.iter().all(|p| corners.contains(&key(*p))), "the glTF stands where the OBJ does");
        assert_eq!(m.materials.iter().filter(|x| x.texture.is_some()).count(), 2, "the two textures are read");
    }
    let glass = glb.materials.iter().find(|m| m.name == "Glass").unwrap();
    assert!(glass.opacity < 0.5);
    // The objects are the nodes of the scene, with their names.
    let names = col(r.output("glb", "Output").unwrap(), "name");
    assert!(names.contains(&text("Roof")) && names.contains(&text("Chimney")), "{:?}", names);
}

#[test]
fn a_model_moved_to_gothenburg_takes_its_3d_shape_along() {
    let dir = tmp("move69");
    let glb = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/house/house.glb");
    let ws = Workspace::from_json(&format!(
        r#"{{"nodes": [
            {{"id": "in", "type": "Reader", "params": {{"path": "{}"}}}},
            {{"id": "crs", "type": "CrsSetter", "params": {{"crs": "EPSG:3006"}}}},
            {{"id": "move", "type": "Offsetter", "params": {{"dx": "319500", "dy": "6399800", "dz": "12"}}}}
        ], "edges": [
            {{"from": ["in", "Output"], "to": ["crs", "Input"]}},
            {{"from": ["crs", "Output"], "to": ["move", "Input"]}}
        ]}}"#,
        glb.display().to_string().replace('\\', "/")
    ))
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    let t = r.output("move", "Output").unwrap();
    assert_eq!(t.crs, Some(3006));
    let m = t.mesh.clone().unwrap();
    let (lo, hi) = m.bounds().unwrap();
    assert!(lo[0] > 319_480.0 && hi[0] < 319_520.0 && lo[1] > 6_399_780.0 && hi[1] < 6_399_820.0, "{lo:?} {hi:?}");
    // Up too: the model and the heights of its objects.
    let zmin = |t: &og_engine::model::Table| t.features.iter().filter_map(|f| f.heights()).flat_map(|h| h.iter().copied()).fold(f64::INFINITY, f64::min);
    let was = r.output("in", "Output").unwrap();
    let low = was.mesh.as_ref().unwrap().bounds().unwrap().0[2];
    assert!((lo[2] - low - 12.0).abs() < 1e-5, "{lo:?} {low}");
    assert!((zmin(t) - zmin(was) - 12.0).abs() < 1e-5);
    use geo::BoundingRect;
    let r0 = t.features[0].geom.as_ref().unwrap().bounding_rect().unwrap();
    assert!(r0.min().x >= lo[0] - 1e-6 && r0.max().x <= hi[0] + 1e-6);
}

#[test]
fn a_model_runs_another_and_waits_for_it() {
    let dir = tmp("runner74");
    std::fs::create_dir_all(dir.join("steg")).unwrap();
    std::fs::write(dir.join("steg/towns.csv"), "name;pop\nBorås;115000\nSkövde;57000\nAlingsås;42000\n").unwrap();
    // The other model: keeps the towns above a size, given as a variable, and writes them.
    std::fs::write(
        dir.join("steg/filter.json"),
        r#"{"format": "ographis/1", "name": "Filter towns", "variables": [{"name": "least", "value": "100000"}],
            "nodes": [{"id": "in", "type": "Reader", "name": "Towns", "params": {"path": "towns.csv"}},
                      {"id": "big", "type": "Tester", "name": "Large enough", "params": {"test": "pop >= $(least)"}},
                      {"id": "out", "type": "Writer", "name": "Large towns", "params": {"path": "large.csv"}}],
            "edges": [{"from": ["in", "Output"], "to": ["big", "Input"]}, {"from": ["big", "Passed"], "to": ["out", "Input"]}]}"#,
    )
    .unwrap();
    // This model runs it, and counts what came of it; and one node runs this model itself.
    let parent = r#"{"variables": [{"name": "size", "value": "50000"}],
            "nodes": [{"id": "run", "type": "ModelRunner", "name": "Filter first", "params": {"path": "steg/filter.json", "variables": "least=$(size)"}},
                      {"id": "count", "type": "Counter", "params": {}},
                      {"id": "ring", "type": "ModelRunner", "params": {"path": "self.json"}}],
            "edges": [{"from": ["run", "Result"], "to": ["count", "Input"]}]}"#;
    std::fs::write(dir.join("self.json"), parent).unwrap();
    let ws = Workspace::from_json(parent).unwrap();
    let r = run(&ws, None, &dir);
    let result = r.output("run", "Result").unwrap();
    assert_eq!(result.len(), 3, "a row for each node of the other model");
    assert_eq!(col(result, "status"), [text("ok"), text("ok"), text("ok")]);
    assert_eq!(col(result, "features")[1], Value::Int(2), "Borås and Skövde pass 50 000");
    let written = std::fs::read_to_string(dir.join("steg/large.csv")).unwrap();
    assert!(written.contains("Skövde") && !written.contains("Alingsås"), "{}", written);
    // The log of the other model comes in this one, under its name.
    assert!(r.log.iter().any(|l| l.msg.contains("Filter towns › Large towns")), "{:?}", r.log);
    assert!(r.log.iter().any(|l| l.msg.starts_with("← Filter first: Filter towns ran: 3 nodes, 0 errors")), "{:?}", r.log);
    // A model that runs itself is stopped at once.
    let e = r.nodes["ring"].error.clone().unwrap();
    assert!(e.contains("in a ring") || e.contains("deep"), "{}", e);
}

#[test]
fn rotate_and_reverse() {
    let dir = tmp("turn");
    std::fs::write(
        dir.join("shapes.csv"),
        "id;deg;shape\n1;90;LINESTRING Z (0 0 1, 10 0 2)\n2;-90;LINESTRING (0 0, 0 10)\n3;180;POLYGON ((0 0, 4 0, 4 2, 0 2, 0 0))\n4;0;MULTILINESTRING ((0 0, 1 0), (5 0, 6 0))\n",
    )
    .unwrap();
    let ws = Workspace::from_json(
        r#"{"nodes": [
        {"id": "r", "type": "Reader", "params": {"path": "shapes.csv"}},
        {"id": "g", "type": "GeometryFromText", "params": {"attribute": "shape", "crs": "EPSG:3006"}},
        {"id": "each", "type": "Rotator", "params": {"angle": "deg"}},
        {"id": "origin", "type": "Rotator", "params": {"angle": "90", "around": "point", "x": "0", "y": "0"}},
        {"id": "back", "type": "LineReverser", "params": {}},
        {"id": "areas", "type": "LineReverser", "params": {"areas": "reverse"}},
        {"id": "lines", "type": "LineReverser", "params": {"areas": "keep"}}],
      "edges": [{"from": ["r", "Output"], "to": ["g", "Input"]},
        {"from": ["g", "Output"], "to": ["each", "Input"]}, {"from": ["g", "Output"], "to": ["origin", "Input"]},
        {"from": ["g", "Output"], "to": ["back", "Input"]}, {"from": ["g", "Output"], "to": ["areas", "Input"]},
        {"from": ["g", "Output"], "to": ["lines", "Input"]}]}"#,
    )
    .unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!(r.errors, 0);
    let coords = |node: &str, i: usize| -> Vec<(f64, f64)> {
        let t = r.output(node, "Output").unwrap();
        let mut out = Vec::new();
        use geo::CoordsIter;
        for c in t.features[i].geom.as_ref().unwrap().coords_iter() {
            out.push(((c.x * 1e6).round() / 1e6, (c.y * 1e6).round() / 1e6));
        }
        out
    };
    // Turned around its middle, 5 0: from along x to along y.
    assert_eq!(coords("each", 0), [(5.0, -5.0), (5.0, 5.0)]);
    assert_eq!(coords("each", 1), [(-5.0, 5.0), (5.0, 5.0)]);
    assert_eq!(coords("origin", 0), [(0.0, 0.0), (0.0, 10.0)]);
    // The heights stay with their points.
    assert_eq!(r.output("each", "Output").unwrap().features[0].heights(), Some(&[1.0, 2.0][..]));
    // Reversed: the heights too, and the parts of a multi line in the other order.
    assert_eq!(coords("back", 0), [(10.0, 0.0), (0.0, 0.0)]);
    assert_eq!(r.output("back", "Output").unwrap().features[0].heights(), Some(&[2.0, 1.0][..]));
    assert_eq!(coords("back", 3), [(6.0, 0.0), (5.0, 0.0), (1.0, 0.0), (0.0, 0.0)]);
    assert_eq!(coords("back", 2), coords("areas", 2), "areas are turned too, unless asked not to");
    assert_eq!(coords("lines", 2), coords("g", 2));
    assert_eq!(coords("areas", 2), [(0.0, 0.0), (0.0, 2.0), (4.0, 2.0), (4.0, 0.0), (0.0, 0.0)]);
}

#[test]
fn only_the_common_tags_of_a_large_openstreetmap_file_get_columns() {
    use og_engine::model::{Feature, Field, FieldType};
    // Ten thousand points: all are shops, a few have a rare tag each, one has a name.
    let mut t = Table { fields: vec![Field::new("name", FieldType::Text), Field::new("other_tags", FieldType::Text)], ..Default::default() };
    for i in 0..10_000 {
        let mut tags = format!(r#""shop"=>"bakery","ref:{}"=>"x""#, i % 3_000);
        if i == 7 {
            tags.push_str(r#","name"=>"Bageriet","note"=>"with \"quotes\"""#);
        }
        t.features.push(Feature { attrs: vec![Value::Null, text(&tags)], geom: None, z: None });
    }
    let (common, note) = og_engine::nodes::osm_tags(t.clone(), false);
    assert_eq!(common.field_names(), ["name", "shop", "other_tags"]);
    let note = note.unwrap();
    assert!(note.starts_with("1 of the 3") && note.contains("002 different tags"), "{note}");
    assert_eq!(col(&common, "shop")[0], text("bakery"));
    assert_eq!(col(&common, "name")[7], text("Bageriet"));
    assert_eq!(col(&common, "other_tags")[7], text(r#""ref:7"=>"x","note"=>"with \"quotes\"""#));
    let (every, _) = og_engine::nodes::osm_tags(t, true);
    assert_eq!(every.fields.len(), 3_003);
    assert!(every.field_index("other_tags").is_none());
}

#[test]
fn a_3d_tiles_tileset_is_read_in_degrees_without_gdal() {
    let set = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tiles/tileset.json");
    let ws = Workspace::from_json(&format!(
        r#"{{"nodes": [
        {{"id": "all", "type": "Reader", "params": {{"path": {path}, "tiles_detail": "finest", "tiles_crs": "EPSG:4326", "tiles_dz": "-32"}}}},
        {{"id": "top", "type": "Reader", "params": {{"path": {path}, "tiles_most": "2"}}}},
        {{"id": "first", "type": "Reader", "params": {{"path": {path}, "tiles_crs": "EPSG:4326",
           "within": "rectangle", "min_x": "13.6014", "min_y": "58.3853", "max_x": "13.60155", "max_y": "58.3854"}}}}],
      "edges": []}}"#,
        path = serde_json::to_string(&set.to_string_lossy()).unwrap()
    ))
    .unwrap();
    let r = run(&ws, None, &set.parent().unwrap().parent().unwrap().join("out"));
    show(&r);
    let all = r.output("all", "Output").unwrap();
    assert_eq!(all.crs, Some(4326));
    assert_eq!(col(all, "name"), texts(&["Ekbacken", "Lindgården", "Björkhagen"]));
    assert_eq!(col(all, "built"), ints(&[1932, 1958, 1974]));
    let m = all.mesh.as_ref().unwrap();
    let (lo, hi) = m.bounds().unwrap();
    // The houses near Skövde, with the ground in RH 2000 once the 32 m are taken away.
    assert!(lo[0] > 13.600 && hi[0] < 13.603 && lo[1] > 58.384 && hi[1] < 58.386, "{:?} {:?}", lo, hi);
    assert!((84.0..85.0).contains(&lo[2]) && (92.0..95.0).contains(&hi[2]), "{:?} {:?}", lo, hi);
    // Upright: each house is 6.7 m from the foot of its walls to its chimney.
    assert!(col(all, "height").iter().all(|h| (h.as_f64().unwrap() - 6.7).abs() < 0.05), "{:?}", col(all, "height"));
    let error = r.nodes["top"].error.clone().unwrap_or_default();
    assert!(error.contains("More than 2 tiles"), "{error}");
    assert_eq!(col(r.output("first", "Output").unwrap(), "name"), texts(&["Ekbacken"]));
}

#[test]
fn a_house_is_read_from_ifc_in_its_place_with_its_properties() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let ws = Workspace::from_json(
        r#"{"nodes": [
            {"id": "villa", "type": "Reader", "params": {"path": "house/villa.ifc"}},
            {"id": "moved", "type": "Reader", "params": {"path": "house/villa.ifc", "obj_east": "1000", "obj_north": "2000", "obj_height": "10", "crs": "EPSG:3006", "ifc_openings": "leave", "ifc_properties": "none"}}
        ], "edges": []}"#,
    )
    .unwrap();
    let r = run(&ws, None, &root);
    show(&r);
    assert_eq!(r.errors, 0);
    let t = r.output("villa", "Output").unwrap();
    assert_eq!(t.crs, Some(3006), "from IfcProjectedCRS");
    assert_eq!(t.len(), 17);
    let classes = col(t, "IfcClass");
    assert_eq!(classes.iter().filter(|c| **c == text("IfcWindow")).count(), 7);
    assert_eq!(classes.iter().filter(|c| **c == text("IfcDoor")).count(), 1);
    assert!(col(t, "Name").contains(&text("Yttervägg söder")));
    assert!(col(t, "storey").iter().all(|s| *s == text("Plan 1")), "the roof slabs by the roof they are part of");
    assert!(col(t, "Pset_WallCommon.FireRating").contains(&text("EI 30")));
    assert!(col(t, "Qto_WallBaseQuantities.Length").contains(&Value::Real(11.0)));
    // By the map conversion: around 418150, 6472440, standing at 84.3 m.
    let m = t.mesh.as_ref().unwrap();
    let (lo, hi) = m.bounds().unwrap();
    assert!(lo[0] > 418_130.0 && hi[0] < 418_170.0 && lo[1] > 6_472_430.0 && hi[1] < 6_472_460.0, "{:?} {:?}", lo, hi);
    assert!((lo[2] - (84.3 - 0.25)).abs() < 1e-6, "the bottom of the slab: {}", lo[2]);
    assert!(m.materials.iter().any(|x| x.name == "IfcWindow glass" && x.opacity < 0.5), "the panes are glass");
    // Placed by the input instead, without openings and properties: fewer attributes, more wall.
    let moved = r.output("moved", "Output").unwrap();
    let (lo, _) = moved.mesh.as_ref().unwrap().bounds().unwrap();
    assert!(lo[0] > 990.0 && lo[0] < 1001.0 && lo[1] > 1990.0 && (lo[2] - 9.75).abs() < 1e-6, "{:?}", lo);
    assert!(moved.field_index("Pset_WallCommon.FireRating").is_none());
    assert!(moved.mesh.as_ref().unwrap().tris.len() < m.tris.len(), "walls without holes have fewer triangles");
}

#[test]
fn a_city_model_without_a_coordinate_system_is_read_with_a_warning() {
    // As the two buildings of the getting started of cityjson.org: no referenceSystem.
    let dir = std::env::temp_dir().join(format!("og-cj-nocrs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("two.city.json"),
        r#"{"type": "CityJSON", "version": "2.0", "transform": {"scale": [0.001, 0.001, 0.001], "translate": [300578.235, 5041258.061, 13.688]},
        "CityObjects": {"b": {"type": "Building", "geometry": [{"type": "MultiSurface", "lod": "1", "boundaries": [[[0, 1, 2, 3]], [[4, 5, 6, 7]]]}]}},
        "vertices": [[0, 0, 0], [10000, 0, 0], [10000, 10000, 0], [0, 10000, 0], [0, 0, 6000], [10000, 0, 6000], [10000, 10000, 6000], [0, 10000, 6000]]}"#,
    )
    .unwrap();
    let ws = Workspace::from_json(r#"{"nodes": [{"id": "in", "type": "Reader", "params": {"path": "two.city.json"}}], "edges": []}"#).unwrap();
    let r = run(&ws, None, &dir);
    show(&r);
    assert_eq!((r.errors, r.warnings), (0, 1));
    let t = r.output("in", "Output").unwrap();
    assert_eq!((t.len(), t.crs), (1, None));
    assert!(r.log.iter().any(|l| l.msg.contains("does not say which coordinate system") && l.msg.contains("UTM")), "{:?}", r.log);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn city_models_and_geoparquet_are_read_without_gdal() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let ws = Workspace::from_json(
        r#"{"nodes": [
            {"id": "json", "type": "Reader", "params": {"path": "formats/blocks.city.json"}},
            {"id": "gml", "type": "Reader", "params": {"path": "formats/blocks.gml"}},
            {"id": "gml1", "type": "Reader", "params": {"path": "formats/blocks.gml", "city_lod": "1"}},
            {"id": "towns", "type": "Reader", "params": {"path": "formats/towns.parquet"}}
        ], "edges": []}"#,
    )
    .unwrap();
    let r = run(&ws, None, &root);
    show(&r);
    assert_eq!(r.errors, 0);
    let json = r.output("json", "Output").unwrap();
    let gml = r.output("gml", "Output").unwrap();
    assert_eq!((json.len(), gml.len(), json.crs, gml.crs), (12, 12, Some(3006), Some(3006)));
    // The rectangles have LoD2 with a roof; the others only LoD1. The same from both formats.
    assert_eq!(col(json, "lod").iter().filter(|l| **l == text("2")).count(), 9);
    assert_eq!(col(json, "lod"), col(gml, "lod"));
    assert_eq!(col(json, "storeysAboveGround"), col(gml, "storeysAboveGround").iter().map(|v| Value::Int(v.as_f64().unwrap() as i64)).collect::<Vec<_>>());
    assert_eq!(col(gml, "objekttyp")[0], text("Bostadshus"));
    let tris = |t: &Table| t.mesh.as_ref().unwrap().tris.len();
    assert_eq!(tris(json), tris(gml));
    let lod1 = r.output("gml1", "Output").unwrap();
    assert!(col(lod1, "lod").iter().all(|l| *l == text("1")) && tris(lod1) < tris(gml));
    assert!(json.mesh.as_ref().unwrap().materials.iter().any(|m| m.name == "roof"));
    // GeoParquet: the towns in longitude and latitude.
    let towns = r.output("towns", "Output").unwrap();
    assert_eq!((towns.len(), towns.crs), (39, Some(4326)));
    assert_eq!(col(towns, "name")[0], text("Göteborg"));
    assert_eq!(col(towns, "population")[0], Value::Int(600000));
}

#[test]
fn a_filter_keeps_the_3d_model_of_what_passes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let ws = Workspace::from_json(
        r#"{"nodes": [
            {"id": "city", "type": "Reader", "params": {"path": "formats/blocks.city.json"}},
            {"id": "roofs", "type": "Tester", "params": {"test": "lod = '2'"}},
            {"id": "split", "type": "ValueFilter", "params": {"attribute": "lod", "values": ["1", "2"]}},
            {"id": "sorted", "type": "Sorter", "params": {"sort_by": "height", "descending": "true"}}
        ], "edges": [
            {"from": ["city", "Output"], "to": ["roofs", "Input"]},
            {"from": ["city", "Output"], "to": ["split", "Input"]},
            {"from": ["city", "Output"], "to": ["sorted", "Input"]}
        ]}"#,
    )
    .unwrap();
    let r = run(&ws, None, &root);
    show(&r);
    assert_eq!(r.errors, 0);
    let all = r.output("city", "Output").unwrap();
    let passed = r.output("roofs", "Passed").unwrap();
    let failed = r.output("roofs", "Failed").unwrap();
    assert_eq!((passed.len(), failed.len()), (9, 3));
    let tris = |t: &Table| t.mesh.as_ref().map_or(0, |m| m.tris.len());
    assert!(tris(passed) > 0 && tris(failed) > 0);
    assert_eq!(tris(passed) + tris(failed), tris(all), "every triangle goes with its building");
    // The objects of the model follow the features: the first of Passed is its first object.
    let m = passed.mesh.as_ref().unwrap();
    assert_eq!(m.objects.len(), 9);
    assert!(m.tris.iter().all(|t| (t.object as usize) < 9));
    assert_eq!(r.output("split", "2").unwrap().mesh.as_ref().map(|m| m.tris.len()), Some(tris(passed)));
    // A sort keeps the whole model, in the new order.
    let sorted = r.output("sorted", "Sorted").unwrap();
    assert_eq!(tris(sorted), tris(all));
}
