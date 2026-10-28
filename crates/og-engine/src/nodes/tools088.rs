//! Tools added in 0.88: copies of each feature (Create duplicate).

use super::geom_inspect::column;
use super::*;
use crate::model::{err, fmt_count, FieldType, Result, Value};

// ---------- Create duplicate (Duplicator) ----------

const DUPLICATE_START: &[(&str, &str)] = &[("1", "1, 2, 3 …"), ("0", "0, 1, 2 …")];

pub(super) const DUPLICATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "count",
        label: "How many",
        kind: ParamKind::Expression("Input"),
        default: "2",
        hint: "How many features each feature becomes, the feature itself included: a number, or an attribute or expression such as inhabitants. 1 keeps it as it is, 0 or less leaves it out",
    },
    ParamSpec { key: "name", label: "Number of the copy in", kind: ParamKind::Text, default: "copy", hint: "An attribute with the number of each copy. Empty adds none" },
    ParamSpec { key: "start", label: "Counted from", kind: ParamKind::Choice(DUPLICATE_START), default: "1", hint: "" },
    ParamSpec {
        key: "most",
        label: "At most",
        kind: ParamKind::Text,
        default: "1000000",
        hint: "The most features that may come out in all. A stop, so that a wrong attribute does not fill the memory of the computer",
    },
];

fn run_duplicate(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let text = p.get("count").filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "2".into());
    let names = table.field_names();
    let count = crate::expr::compile(&text, &names).map_err(|e| GbError(format!("How many: {}", e.0)))?;
    let most: usize = match p.get("most").filter(|t| !t.trim().is_empty()) {
        Some(t) => t.trim().replace([' ', '\u{a0}'], "").parse().map_err(|_| GbError(format!("At most must be a whole number, not \"{}\".", t)))?,
        None => 1_000_000,
    };
    let first: i64 = if p.get("start").as_deref() == Some("0") { 0 } else { 1 };
    let name = p.get("name").map(|n| n.trim().to_string()).unwrap_or_else(|| "copy".into());
    let geographic = ctx.geographic(table.crs);
    // How many of each first, so that too many is said before anything is copied.
    let mut counts = Vec::with_capacity(table.len());
    let mut total = 0usize;
    for (i, f) in table.features.iter().enumerate() {
        let v = count.eval(&crate::expr::Row { feature: f, index: i, geographic })?;
        let n = match v {
            Value::Null => 0,
            v => {
                v.as_f64().ok_or_else(|| GbError(format!("How many must be a number. Feature {} gives \"{}\".", i + 1, v.to_text())))?.floor().max(0.0) as usize
            }
        };
        total = total.saturating_add(n);
        if total > most {
            return err(format!(
                "That makes more than {} features (feature {} asks for {}). Raise At most if that many are wanted, or check How many.",
                fmt_count(most as u64),
                i + 1,
                fmt_count(n as u64)
            ));
        }
        counts.push(n);
    }
    let mut out = table.like();
    out.fields = table.fields.clone();
    let at = (!name.is_empty()).then(|| column(&mut out, &name, FieldType::Int));
    out.features.reserve(total);
    for (f, n) in table.features.iter().zip(counts) {
        for k in 0..n {
            let mut g = f.clone();
            if let Some(at) = at {
                g.attrs.resize(out.fields.len(), Value::Null);
                g.attrs[at] = Value::Int(first + k as i64);
            }
            out.features.push(g);
        }
    }
    ctx.notes.info(format!("{} features from {}", fmt_count(out.len() as u64), fmt_count(table.len() as u64)));
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "Duplicator",
    title: "Create duplicate",
    hidden: false,
    group: Group::Transformer,
    category: "Attributes",
    desc: "Makes several features of each feature, all with its attributes and geometry, and the number of each copy in an attribute. As many as a number, or as an attribute says: a point per inhabitant, a row per year, the same trip at every quarter of an hour with date_add and the number of the copy.",
    inputs: &["Input"],
    outputs: &["Output"],
    ports: None,
    params: DUPLICATE_PARAMS,
    run: run_duplicate,
    summary: |p| format!("{} of each", p.get("count").filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "2".into())),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, Table};

    fn towns() -> Table {
        Table {
            fields: vec![Field::new("name", FieldType::Text), Field::new("n", FieldType::Int)],
            features: vec![
                Feature { attrs: vec![Value::Text("A".into()), Value::Int(3)], geom: None, z: None },
                Feature { attrs: vec![Value::Text("B".into()), Value::Int(0)], geom: None, z: None },
                Feature { attrs: vec![Value::Text("C".into()), Value::Null], geom: None, z: None },
                Feature { attrs: vec![Value::Text("D".into()), Value::Int(1)], geom: None, z: None },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn each_feature_becomes_as_many_as_asked_for() {
        let (out, _) = run_for_test("Duplicator", &[("count", "n")], vec![("Input", towns())]).unwrap();
        let t = &out[0].1;
        let names: Vec<String> = t.features.iter().map(|f| f.attrs[0].to_text()).collect();
        assert_eq!(names, ["A", "A", "A", "D"]);
        let copies: Vec<Value> = t.features.iter().map(|f| f.attrs[2].clone()).collect();
        assert_eq!(copies, [Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(1)]);
        // A fixed number, counted from 0, in another attribute.
        let (out, _) = run_for_test("Duplicator", &[("count", "2"), ("start", "0"), ("name", "k")], vec![("Input", towns())]).unwrap();
        let t = &out[0].1;
        assert_eq!(t.len(), 8);
        assert_eq!(t.field_names(), ["name", "n", "k"]);
        assert_eq!(t.features[1].attrs[2], Value::Int(1));
        // Too many is said before anything is made.
        let e = run_for_test("Duplicator", &[("count", "n * 1000"), ("most", "2000")], vec![("Input", towns())]).unwrap_err();
        assert!(e.0.contains("more than 2 000 features"), "{}", e.0);
    }
}
