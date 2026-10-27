//! Tests of what came in 0.87: bounds, coordinates in attributes, the ends of lines,
//! random numbers, Geometry to 2D and Reverse geometry.

use super::*;
use crate::model::{Feature, Field, FieldType, Table, Value};
use geo::Geometry;
use std::sync::Arc;

fn port<'a>(out: &'a Outputs, name: &str) -> &'a Table {
    &out.iter().find(|o| o.0 == name).unwrap().1
}

fn line_table() -> Table {
    let line = geo::LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)]);
    let square = geo::Polygon::new(geo::LineString::from(vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0), (0.0, 0.0)]), vec![]);
    Table {
        fields: vec![Field::new("name", FieldType::Text)],
        crs: Some(3006),
        features: vec![
            Feature { attrs: vec![Value::Text("road".into())], geom: Some(Geometry::LineString(line)), z: Some(Arc::from(vec![1.0, 2.0, 3.0])) },
            Feature { attrs: vec![Value::Text("plot".into())], geom: Some(Geometry::Polygon(square)), z: None },
        ],
        ..Default::default()
    }
}

#[test]
fn bounds_ends_and_random_numbers() {
    let (out, _) = run_for_test("BoundsExtractor", &[("heights", "true")], vec![("Input", line_table())]).unwrap();
    let t = &out[0].1;
    let get = |row: usize, name: &str| t.features[row].attrs[t.field_index(name).unwrap()].clone();
    assert_eq!((get(0, "xmin"), get(0, "xmax"), get(0, "ymax")), (Value::Real(0.0), Value::Real(10.0), Value::Real(5.0)));
    assert_eq!((get(0, "zmin"), get(0, "zmax")), (Value::Real(1.0), Value::Real(3.0)));
    assert_eq!(get(1, "zmin"), Value::Null);
    let (out, _) = run_for_test("LineEndsExtractor", &[], vec![("Input", line_table())]).unwrap();
    let first = &port(&out, "First").features[0];
    assert_eq!(first.geom, Some(Geometry::Point(geo::Point::new(0.0, 0.0))));
    assert_eq!(first.heights(), Some(&[1.0][..]));
    assert_eq!(port(&out, "Last").features[0].geom, Some(Geometry::Point(geo::Point::new(10.0, 5.0))));
    assert_eq!(port(&out, "Other").features.len(), 1);
    let many = Table { fields: vec![], features: (0..500).map(|_| Feature { attrs: vec![], geom: None, z: None }).collect(), ..Default::default() };
    let (out, _) = run_for_test("RandomNumber", &[("min", "1"), ("max", "6"), ("decimals", "0"), ("seed", "7")], vec![("Input", many.clone())]).unwrap();
    let dice: Vec<i64> = out[0]
        .1
        .features
        .iter()
        .map(|f| match f.attrs[0] {
            Value::Int(v) => v,
            _ => panic!("a whole number"),
        })
        .collect();
    assert!(dice.iter().all(|d| (1..=6).contains(d)));
    for side in 1..=6 {
        assert!(dice.contains(&side), "{} never came", side);
    }
    let (again, _) = run_for_test("RandomNumber", &[("min", "1"), ("max", "6"), ("decimals", "0"), ("seed", "7")], vec![("Input", many.clone())]).unwrap();
    assert_eq!(again[0].1.features[10].attrs, out[0].1.features[10].attrs);
    let (out, _) = run_for_test("RandomNumber", &[("min", "-1"), ("max", "1"), ("decimals", "3")], vec![("Input", many)]).unwrap();
    assert!(out[0].1.features.iter().all(|f| matches!(f.attrs[0], Value::Real(v) if (-1.0..=1.0).contains(&v) && (v * 1000.0).fract().abs() < 1e-6)));
}

#[test]
fn geometry_to_2d_leaves_heights_and_the_model_behind_and_areas_are_reversed() {
    let mut t = line_table();
    t.mesh = Some(Arc::new(crate::mesh::Mesh { objects: vec!["a".into(), "b".into()], ..Default::default() }));
    let (out, notes) = run_for_test("Force2D", &[], vec![("Input", t.clone())]).unwrap();
    assert!(out[0].1.mesh.is_none() && out[0].1.features.iter().all(|f| f.z.is_none()), "{:?}", notes);
    // Reverse geometry turns areas too, as it is set up from now on.
    let (out, _) = run_for_test("LineReverser", &[], vec![("Input", t)]).unwrap();
    match &out[0].1.features[1].geom {
        Some(Geometry::Polygon(p)) => assert_eq!(p.exterior().0[1], geo::coord! { x: 0.0, y: 4.0 }),
        g => panic!("{:?}", g),
    }
    assert_eq!(out[0].1.features[0].heights(), Some(&[3.0, 2.0, 1.0][..]));
}

/// Every stop of a journey planner is one answer of megabytes, and tens of thousands of
/// elements. Each element once got its own copy of the whole answer: 25 GB here, and the
/// program was stopped by the system. Now the answer is not copied unless it is kept.
#[test]
fn a_large_list_is_exploded_without_a_copy_of_the_answer_per_element() {
    let stops: Vec<String> = (0..20_000).map(|i| format!(r#"{{"id":"1:{i}","name":"Stop {i}","lat":57.7,"lon":11.9,"desc":null}}"#)).collect();
    let answer = format!("[{}]", stops.join(","));
    assert!(answer.len() > 1_200_000);
    let table = Table {
        fields: vec![Field::new("_response", FieldType::Text), Field::new("_status", FieldType::Int)],
        features: vec![Feature { attrs: vec![Value::Text(answer), Value::Int(200)], geom: None, z: None }],
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let (out, _) = run_for_test("JsonExploder", &[("attribute", "_response")], vec![("Input", table)]).unwrap();
    let t = port(&out, "Exploded");
    assert_eq!(t.len(), 20_000);
    assert_eq!(t.field_names(), ["_status", "id", "name", "lat", "lon", "desc"]);
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}
