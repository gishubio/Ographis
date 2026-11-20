//! Find what has changed between two versions of the same data: which features are new,
//! which are gone, which have changed and how, and which are as they were. Added in 0.96.5.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, Feature, Field, FieldType, Value};
use std::collections::HashMap;

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "key",
        label: "Key",
        kind: ParamKind::Attributes("Original"),
        default: "",
        hint: "One or more attributes that name a feature in both versions, such as an id. Empty compares the features whole: they are then only added, removed or unchanged",
    },
    ParamSpec {
        key: "compare",
        label: "Attributes to compare",
        kind: ParamKind::Attributes("Original"),
        default: "",
        hint: "Empty compares every attribute that both versions have",
    },
    ParamSpec { key: "geometry", label: "Compare the geometry", kind: ParamKind::Choice(YES_NO), default: "true", hint: "" },
    ParamSpec {
        key: "tolerance",
        label: "Tolerance for numbers",
        kind: ParamKind::Text,
        default: "",
        hint: "Numbers that differ by no more than this are the same, such as 0.001. Empty: only the same number",
    },
    ParamSpec {
        key: "changed_name",
        label: "Attribute with what changed",
        kind: ParamKind::Text,
        default: "_changed",
        hint: "On Changed: the names of the attributes that differ, and geometry when it does, with commas between",
    },
];

/// Whether two values are the same: numbers by their value, within the tolerance, and
/// anything else by its text, an empty text being the same as no value.
fn same(a: &Value, b: &Value, tolerance: f64) -> bool {
    match (a, b) {
        (Value::Int(_) | Value::Real(_), Value::Int(_) | Value::Real(_)) => {
            let (x, y) = (a.as_f64().unwrap_or(f64::NAN), b.as_f64().unwrap_or(f64::NAN));
            x == y || (x - y).abs() <= tolerance
        }
        _ => a.key() == b.key(),
    }
}

fn same_geometry(a: &Feature, b: &Feature) -> bool {
    a.geom == b.geom && a.z.as_deref() == b.z.as_deref()
}

/// A feature as a key of all its compared values and its geometry, for comparing whole.
fn whole_key(f: &Feature, idx: &[usize], geometry: bool) -> String {
    let mut k: Vec<String> = idx.iter().map(|i| f.attrs[*i].key().unwrap_or_default()).collect();
    if geometry {
        k.push(format!("{:?}{:?}", f.geom, f.z.as_deref()));
    }
    k.join("\u{1f}")
}

fn run_changes(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (old, new) = (input(inp, "Original"), input(inp, "Revised"));
    let keys = p.list("key");
    let geometry = p.flag("geometry");
    let tolerance = number_param(p, "tolerance", "The tolerance")?.unwrap_or(0.0).abs();
    let changed_name = p.get("changed_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "_changed".into());

    // The attributes compared: those chosen, or every one that both versions have.
    let chosen = p.list("compare");
    let names: Vec<String> = if chosen.is_empty() {
        old.fields.iter().map(|f| f.name.clone()).filter(|n| new.field_index(n).is_some() && !keys.contains(n)).collect()
    } else {
        chosen
    };
    for n in &names {
        if old.field_index(n).is_none() || new.field_index(n).is_none() {
            return err(format!("{} is not an attribute of both Original and Revised.", n));
        }
    }
    let only_new: Vec<&str> = new.fields.iter().map(|f| f.name.as_str()).filter(|n| old.field_index(n).is_none()).collect();
    let only_old: Vec<&str> = old.fields.iter().map(|f| f.name.as_str()).filter(|n| new.field_index(n).is_none()).collect();
    if !only_new.is_empty() {
        ctx.notes.info(format!("Revised has attributes that Original has not, which are not compared: {}", only_new.join(", ")));
    }
    if !only_old.is_empty() {
        ctx.notes.info(format!("Original has attributes that Revised has not, which are not compared: {}", only_old.join(", ")));
    }
    let oi: Vec<usize> = names.iter().map(|n| old.field_index(n).expect("checked")).collect();
    let ni: Vec<usize> = names.iter().map(|n| new.field_index(n).expect("checked")).collect();

    let mut added = new.like();
    let mut removed = old.like();
    let mut unchanged = new.like();
    let mut changed = new.like();
    if changed.field_index(&changed_name).is_none() {
        changed.fields.push(Field::new(changed_name.clone(), FieldType::Text));
    }
    let at = changed.field_index(&changed_name).expect("added");

    if keys.is_empty() {
        // Without a key a feature is the same only as one with every value and the geometry
        // alike; each is matched once, so that two alike count as two.
        let mut left: HashMap<String, usize> = HashMap::new();
        for f in &old.features {
            *left.entry(whole_key(f, &oi, geometry)).or_default() += 1;
        }
        for f in &new.features {
            match left.get_mut(&whole_key(f, &ni, geometry)) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    unchanged.features.push(f.clone());
                }
                _ => added.features.push(f.clone()),
            }
        }
        for f in &old.features {
            if let Some(n) = left.get_mut(&whole_key(f, &oi, geometry)).filter(|n| **n > 0) {
                *n -= 1;
                removed.features.push(f.clone());
            }
        }
    } else {
        let ok: Vec<usize> = keys.iter().map(|n| old.need_field(n)).collect::<Result<_>>()?;
        let nk: Vec<usize> = keys.iter().map(|n| new.need_field(n)).collect::<Result<_>>()?;
        let mut index: HashMap<Vec<String>, usize> = HashMap::new();
        let mut twice = 0usize;
        let mut no_key = 0usize;
        for (i, f) in old.features.iter().enumerate() {
            match attr_table::key_of(f, &ok) {
                Some(k) => {
                    if index.insert(k, i).is_some() {
                        twice += 1;
                    }
                }
                None => no_key += 1,
            }
        }
        let mut seen = vec![false; old.len()];
        let mut new_twice = 0usize;
        let mut new_seen: std::collections::HashSet<Vec<String>> = std::collections::HashSet::new();
        for f in &new.features {
            let Some(k) = attr_table::key_of(f, &nk) else {
                no_key += 1;
                added.features.push(f.clone());
                continue;
            };
            if !new_seen.insert(k.clone()) {
                new_twice += 1;
            }
            let Some(&i) = index.get(&k) else {
                added.features.push(f.clone());
                continue;
            };
            seen[i] = true;
            let o = &old.features[i];
            let mut what: Vec<&str> =
                names.iter().enumerate().filter(|(j, _)| !same(&o.attrs[oi[*j]], &f.attrs[ni[*j]], tolerance)).map(|(_, n)| n.as_str()).collect();
            if geometry && !same_geometry(o, f) {
                what.push("geometry");
            }
            if what.is_empty() {
                unchanged.features.push(f.clone());
            } else {
                let mut attrs = f.attrs.clone();
                attrs.resize(changed.fields.len(), Value::Null);
                attrs[at] = Value::Text(what.join(", "));
                changed.features.push(Feature { attrs, geom: f.geom.clone(), z: f.z.clone() });
            }
        }
        // Those of Original without a key go to Removed, as nothing can be found for them.
        removed.features = old.features.iter().zip(&seen).filter(|(_, s)| !**s).map(|(f, _)| f.clone()).collect();
        if twice > 0 {
            ctx.notes.warn(format!(
                "{} features of Original have a key that another has too. Only the last of each is compared; the others are Removed.",
                fmt_count(twice as u64)
            ));
        }
        if new_twice > 0 {
            ctx.notes.warn(format!(
                "{} features of Revised have a key that another has too. Each is compared with the same feature of Original.",
                fmt_count(new_twice as u64)
            ));
        }
        if no_key > 0 {
            ctx.notes.warn(format!("{} features have no value in the key, and are counted as added or removed.", fmt_count(no_key as u64)));
        }
    }
    ctx.notes.info(format!(
        "{} added, {} changed, {} removed, {} unchanged",
        fmt_count(added.len() as u64),
        fmt_count(changed.len() as u64),
        fmt_count(removed.len() as u64),
        fmt_count(unchanged.len() as u64)
    ));
    Ok(vec![("Added".into(), added), ("Changed".into(), changed), ("Removed".into(), removed), ("Unchanged".into(), unchanged)])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "ChangeDetector",
    title: "Find changes",
    hidden: false,
    group: Group::Transformer,
    category: "Attributes",
    desc: "Compares two versions of the same data, such as last month's register and this month's, by a key: the features that are new come out on Added, those that are gone on Removed, those whose attributes or geometry differ on Changed, with an attribute that names what changed, and the rest on Unchanged. Without a key the features are compared whole. To keep another copy up to date, write only what changed.",
    inputs: &["Original", "Revised"],
    outputs: &["Added", "Changed", "Removed", "Unchanged"],
    ports: None,
    params: PARAMS,
    run: run_changes,
    summary: |p| {
        let k = p.list("key");
        if k.is_empty() {
            "the features whole".to_string()
        } else {
            format!("by {}", k.join(" + "))
        }
    },
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Table;
    use geo::{point, Geometry};

    fn table(rows: &[(i64, &str, f64, f64)]) -> Table {
        let mut t = Table {
            fields: vec![Field::new("id", FieldType::Int), Field::new("name", FieldType::Text), Field::new("beds", FieldType::Real)],
            crs: Some(3006),
            ..Default::default()
        };
        for (id, name, beds, x) in rows {
            t.features.push(Feature::new(
                vec![Value::Int(*id), Value::Text(name.to_string()), Value::Real(*beds)],
                Some(Geometry::Point(point!(x: *x, y: 6_400_000.0))),
            ));
        }
        t
    }

    fn ids(out: &Outputs, port: &str) -> Vec<String> {
        let t = &out.iter().find(|(p, _)| p == port).unwrap().1;
        t.features.iter().map(|f| f.attrs[0].to_text()).collect()
    }

    #[test]
    fn the_changes_between_two_versions_are_found_by_a_key() {
        let old = table(&[(1, "Sahlgrenska", 1700.0, 1.0), (2, "Östra", 500.0, 2.0), (3, "Mölndal", 300.0, 3.0), (4, "Kungälv", 200.0, 4.0)]);
        // 2 has more beds, 3 has moved, 4 is gone and 5 is new; 1 has a difference within the tolerance.
        let new = table(&[(1, "Sahlgrenska", 1700.0004, 1.0), (2, "Östra", 520.0, 2.0), (3, "Mölndal", 300.0, 30.0), (5, "Alingsås", 150.0, 5.0)]);
        let (out, notes) =
            run_for_test("ChangeDetector", &[("key", "id"), ("tolerance", "0.001")], vec![("Original", old.clone()), ("Revised", new.clone())]).unwrap();
        assert_eq!(ids(&out, "Added"), ["5"]);
        assert_eq!(ids(&out, "Removed"), ["4"]);
        assert_eq!(ids(&out, "Unchanged"), ["1"]);
        assert_eq!(ids(&out, "Changed"), ["2", "3"]);
        let changed = &out.iter().find(|(p, _)| p == "Changed").unwrap().1;
        let what: Vec<String> = changed.features.iter().map(|f| f.attrs[changed.field_index("_changed").unwrap()].to_text()).collect();
        assert_eq!(what, ["beds", "geometry"]);
        assert!(notes.info.iter().any(|l| l.contains("1 added, 2 changed, 1 removed, 1 unchanged")), "{:?}", notes.info);

        // Without the geometry, the hospital that moved is as it was.
        let (out, _) =
            run_for_test("ChangeDetector", &[("key", "id"), ("geometry", "false")], vec![("Original", old.clone()), ("Revised", new.clone())]).unwrap();
        assert_eq!(ids(&out, "Changed"), ["1", "2"], "without a tolerance 1700.0004 is not 1700");
    }

    #[test]
    fn without_a_key_the_features_are_compared_whole() {
        let old = table(&[(1, "A", 1.0, 1.0), (2, "B", 2.0, 2.0), (2, "B", 2.0, 2.0)]);
        let new = table(&[(2, "B", 2.0, 2.0), (3, "C", 3.0, 3.0)]);
        let (out, _) = run_for_test("ChangeDetector", &[], vec![("Original", old), ("Revised", new)]).unwrap();
        assert_eq!(ids(&out, "Unchanged"), ["2"]);
        assert_eq!(ids(&out, "Added"), ["3"]);
        assert_eq!(ids(&out, "Removed"), ["1", "2"], "of two alike, one is still there");
        assert!(ids(&out, "Changed").is_empty());
    }
}
