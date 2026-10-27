//! Tools that let features through or send them to different outputs by their attributes:
//! Test, Filter by value, Remove duplicates, Check attributes and Take first.

use super::*;
use crate::model::{err, fmt_count, Feature, FieldType, Result, Table, Value};
use std::collections::HashMap;

// ---------- Test (Tester) ----------

pub(super) const TESTER_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "test",
    label: "Test",
    kind: ParamKind::Expression("Input"),
    default: "",
    hint: "For example: population >= 10000 AND municipality <> 'Mark'",
}];

pub(super) fn run_tester(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let test = expr::compile(&p.need("test", "There is no test. Write the condition that decides which features pass.")?, &table.field_names())?;
    let geographic = ctx.geographic(table.crs);
    let (mut pass, mut fail) = (table.like(), table.like());
    for (i, f) in table.features.iter().enumerate() {
        let ok = test.test(&Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?;
        (if ok { &mut pass } else { &mut fail }).features.push(f.clone());
    }
    Ok(vec![("Passed".into(), pass), ("Failed".into(), fail)])
}

pub(super) fn at_row(e: GbError, i: usize) -> GbError {
    GbError(format!("Feature {}: {}", i + 1, e.0))
}

// ---------- Filter by value (ValueFilter) ----------

pub(super) const FILTER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "Attribute", kind: ParamKind::Attribute("Input"), default: "", hint: "" },
    ParamSpec {
        key: "values",
        label: "Values",
        kind: ParamKind::Values("Input", "attribute"),
        default: "",
        hint: "Each value gets an output of its own. Features with any other value go to Other",
    },
];

/// The values a filter has outputs for, in order, without repeats.
pub(super) fn filter_values(p: &P) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in p.list_exact("values") {
        if !v.is_empty() && !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// The output for what matches none of the values. It is named so that it cannot be taken
/// for one of them.
pub(super) fn filter_rest(values: &[String]) -> String {
    let mut name = "Other".to_string();
    while values.contains(&name) {
        name = format!("{} values", name);
    }
    name
}

pub(super) fn filter_ports(p: &P) -> Vec<String> {
    let mut values = filter_values(p);
    let rest = filter_rest(&values);
    values.push(rest);
    values
}

pub(super) fn same_value(v: &Value, wanted: &str) -> bool {
    let text = v.to_text();
    if text == wanted {
        return true;
    }
    // 5, 5.0 and "5" are the same value, but "0114" is not 114.
    matches!((v.as_f64(), crate::model::parse_number(wanted)), (Some(a), Some(b)) if a == b && !matches!(v, Value::Text(_)))
}

pub(super) fn run_filter(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute to filter by.")?;
    let i = table.field_index(&attr).ok_or_else(|| GbError(format!("There is no attribute {}. The stream has: {}.", attr, table.field_names().join(", "))))?;
    let values = filter_values(p);
    if values.is_empty() {
        ctx.notes.warn("No values are given, so everything goes to Other.".to_string());
    }
    let mut outs: Vec<Table> = values.iter().map(|_| table.like()).collect();
    let mut rest = table.like();
    for f in &table.features {
        match values.iter().position(|w| same_value(&f.attrs[i], w)) {
            Some(k) => outs[k].features.push(f.clone()),
            None => rest.features.push(f.clone()),
        }
    }
    let rest_name = filter_rest(&values);
    let mut result: Outputs = values.into_iter().map(std::borrow::Cow::Owned).zip(outs).collect();
    result.push((rest_name.into(), rest));
    Ok(result)
}

// ---------- Remove duplicates (DuplicateFilter) ----------

pub(super) const DUPLICATE_GEOMETRY: &[(&str, &str)] =
    &[("no", "Not compared"), ("also", "Compared too: the same attributes and the same geometry"), ("only", "Only the geometry")];

pub(super) const DUPLICATE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "key", label: "Key attributes", kind: ParamKind::Attributes("Input"), default: "", hint: "Empty compares all attributes" },
    ParamSpec { key: "geometry", label: "Geometry", kind: ParamKind::Choice(DUPLICATE_GEOMETRY), default: "no", hint: "" },
    ParamSpec {
        key: "decimals",
        label: "Coordinates alike to decimals",
        kind: ParamKind::Text,
        default: "3",
        hint: "Geometries are the same when their coordinates are, rounded to this many decimals: 3 is a millimetre in metres",
    },
];

pub(super) fn run_duplicates(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let key = p.list("key");
    let by_geom = p.get("geometry").unwrap_or_else(|| "no".into());
    let idx: Vec<usize> = if by_geom == "only" {
        Vec::new()
    } else if key.is_empty() {
        (0..table.fields.len()).collect()
    } else {
        key.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?
    };
    let decimals = number_param(p, "decimals", "The decimals")?.unwrap_or(3.0).clamp(0.0, 12.0) as i32;
    let k = 10f64.powi(decimals);
    let (mut unique, mut duplicate) = (table.like(), table.like());
    let mut seen = std::collections::HashSet::new();
    for f in &table.features {
        let attrs = group_key(f, &idx);
        let kk = if by_geom != "no" {
            // The coordinates rounded, in their order, and the heights.
            let g = f.geom.as_ref().map(|g| format!("{:?}", geom::rounded(g, decimals))).unwrap_or_default();
            let z: Vec<i64> = f.heights().map(|z| z.iter().map(|v| (v * k).round() as i64).collect()).unwrap_or_default();
            format!("{:?}|{}|{:?}", attrs, g, z)
        } else {
            format!("{:?}", attrs)
        };
        (if seen.insert(kk) { &mut unique } else { &mut duplicate }).features.push(f.clone());
    }
    Ok(vec![("Unique".into(), unique), ("Duplicate".into(), duplicate)])
}

// ---------- Check attributes ----------

pub(super) const ATTRIBUTE_CHECK_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "checks",
        label: "Checks",
        kind: ParamKind::Checks("Input"),
        default: "",
        hint:
            "A row for each attribute: whether it must have a value, its type, its longest length, its smallest and largest value, and the values it may have",
    },
    ParamSpec { key: "problem", label: "Problem attribute", kind: ParamKind::Text, default: "problems", hint: "What was wrong, on Failed" },
];

/// What one attribute is checked for. Empty parts are not checked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Check {
    pub attribute: String,
    pub required: bool,
    /// "integer", "decimal", "text" or "date", or empty.
    pub kind: String,
    pub max_length: Option<usize>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// The values it may have, separated by commas.
    pub values: String,
}

/// The checks as they are stored: a JSON list of objects.
pub fn checks_of(params: &Params, key: &str) -> Vec<Check> {
    let Some(serde_json::Value::Array(rows)) = params.get(key) else { return Vec::new() };
    let text = |v: Option<&serde_json::Value>| match v {
        Some(serde_json::Value::String(s)) => s.trim().to_string(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let number = |v: Option<&serde_json::Value>| text(v).replace(',', ".").parse::<f64>().ok();
    rows.iter()
        .filter_map(|r| {
            let o = r.as_object()?;
            Some(Check {
                attribute: text(o.get("attribute")),
                required: o.get("required").is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true")),
                kind: text(o.get("type")),
                max_length: number(o.get("max_length")).map(|n| n.max(0.0) as usize),
                min: number(o.get("min")),
                max: number(o.get("max")),
                values: text(o.get("values")),
            })
        })
        .filter(|c| !c.attribute.is_empty())
        .collect()
}

/// A date as yyyy-mm-dd, with or without a time after it.
pub(super) fn is_date(s: &str) -> bool {
    let d = s.trim().get(..10).unwrap_or("");
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok() && (s.trim().len() == 10 || s.trim().as_bytes().get(10).is_some_and(|c| *c == b' ' || *c == b'T'))
}

/// What is wrong with a value, by the check.
pub fn value_problems(c: &Check, v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let text = v.to_text();
    let empty = v.is_null() || (matches!(v, Value::Text(_)) && text.trim().is_empty());
    if empty {
        if c.required {
            out.push(format!("{} is empty", c.attribute));
        }
        return out;
    }
    let number = match v {
        Value::Int(i) => Some(*i as f64),
        Value::Real(r) => Some(*r),
        Value::Bool(b) => Some(*b as i64 as f64),
        Value::Text(t) => t.trim().replace(',', ".").parse::<f64>().ok(),
        Value::Null => None,
    };
    match c.kind.as_str() {
        "integer" if !number.is_some_and(|n| n.fract() == 0.0) => out.push(format!("{} is not a whole number", c.attribute)),
        "decimal" if number.is_none() => out.push(format!("{} is not a number", c.attribute)),
        "date" if !is_date(&text) => out.push(format!("{} is not a date as yyyy-mm-dd", c.attribute)),
        _ => {}
    }
    if let Some(m) = c.max_length {
        let n = text.chars().count();
        if n > m {
            out.push(format!("{} has {} characters, more than {}", c.attribute, n, m));
        }
    }
    if let (Some(lo), Some(n)) = (c.min, number) {
        if n < lo {
            out.push(format!("{} is less than {}", c.attribute, crate::model::fmt_real(lo)));
        }
    }
    if let (Some(hi), Some(n)) = (c.max, number) {
        if n > hi {
            out.push(format!("{} is more than {}", c.attribute, crate::model::fmt_real(hi)));
        }
    }
    if (c.min.is_some() || c.max.is_some()) && number.is_none() && c.kind.is_empty() {
        out.push(format!("{} is not a number", c.attribute));
    }
    if !c.values.trim().is_empty() {
        let allowed: Vec<&str> = c.values.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
        if !allowed.contains(&text.trim()) {
            out.push(format!("{} is {}, which is not one of {}", c.attribute, text.trim(), allowed.join(", ")));
        }
    }
    out
}

pub(super) fn run_attribute_check(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let checks = checks_of(p.values, "checks");
    if checks.is_empty() {
        return err("Add a check: choose an attribute and what it must be.");
    }
    let name = p.get("problem").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "problems".into());
    // An attribute that is not in the stream at all is empty in every feature.
    let idx: Vec<Option<usize>> = checks.iter().map(|c| table.field_index(&c.attribute)).collect();
    for (c, i) in checks.iter().zip(&idx) {
        if i.is_none() {
            ctx.notes.warn(format!("The stream has no attribute {}. It is taken as empty.", c.attribute));
        }
    }
    let mut passed = table.like();
    let mut failed = table.like();
    let at = column(&mut failed, &name, FieldType::Text);
    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in &table.features {
        let mut problems = Vec::new();
        for (c, i) in checks.iter().zip(&idx) {
            let v = i.map_or(&Value::Null, |i| &f.attrs[i]);
            for pr in value_problems(c, v) {
                *counts.entry(c.attribute.clone()).or_default() += 1;
                problems.push(pr);
            }
        }
        if problems.is_empty() {
            passed.features.push(f.clone());
        } else {
            let mut attrs = f.attrs.clone();
            attrs.resize(failed.fields.len(), Value::Null);
            attrs[at] = Value::Text(problems.join("; "));
            failed.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
        }
    }
    if !failed.features.is_empty() {
        let mut by: Vec<(String, usize)> = counts.into_iter().collect();
        by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let list: Vec<String> = by.iter().map(|(a, n)| format!("{} {}", a, fmt_count(*n as u64))).collect();
        ctx.notes.info(format!("problems by attribute: {}", list.join(", ")));
    }
    Ok(vec![("Passed".into(), passed), ("Failed".into(), failed)])
}

// ---------- Take first ----------

pub(super) const TAKE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "count",
        label: "How many",
        kind: ParamKind::Text,
        default: "5",
        hint: "The first ones in the order they come. Put Sort before to choose the order",
    },
    ParamSpec {
        key: "group_by",
        label: "For each",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "Takes that many of each group of features with the same values, such as the five largest towns in each municipality",
    },
];

pub(super) fn run_take(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let n = number_param(p, "count", "How many")?.unwrap_or(5.0);
    if n < 0.0 || n.fract() != 0.0 {
        return err("How many must be a whole number, 0 or more.");
    }
    let n = n as usize;
    let by: Vec<usize> = p.list("group_by").iter().map(|a| table.need_field(a)).collect::<Result<_>>()?;
    let mut taken = table.like();
    let mut rest = table.like();
    let mut seen: HashMap<Vec<Option<String>>, usize> = HashMap::new();
    for f in &table.features {
        let key: Vec<Option<String>> = by.iter().map(|i| f.attrs[*i].key()).collect();
        let k = seen.entry(key).or_insert(0);
        if *k < n {
            taken.features.push(f.clone());
        } else {
            rest.features.push(f.clone());
        }
        *k += 1;
    }
    Ok(vec![("Taken".into(), taken), ("Rest".into(), rest)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "Tester",
        title: "Test",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Splits the stream by a test. Features that meet the test go to Passed, the rest to Failed. Use only Passed to filter.",
        inputs: &["Input"],
        outputs: &["Passed", "Failed"],
        ports: None,
        params: TESTER_PARAMS,
        run: run_tester,
        summary: |p| p.get("test").unwrap_or_else(|| "no test".into()),
    },
    NodeType {
        name: "DuplicateFilter",
        title: "Remove duplicates",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Sends the first feature with each key to Unique and the repeats to Duplicate. Without key attributes, all attributes are compared. The geometry can be compared too, or alone, with the coordinates rounded to some decimals: points in the same place, lines drawn twice.",
        inputs: &["Input"],
        outputs: &["Unique", "Duplicate"],
        ports: None,
        params: DUPLICATE_PARAMS,
        run: run_duplicates,
        summary: |p| {
            let key = p.list("key");
            let attrs = if key.is_empty() { "all attributes".to_string() } else { key.join(" + ") };
            match p.get("geometry").as_deref() {
                Some("also") => format!("{} and geometry", attrs),
                Some("only") => "geometry".to_string(),
                _ => attrs,
            }
        },
    },
    NodeType {
        name: "ValueFilter",
        title: "Filter by value",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Sends the features to an output for each value of an attribute that you list, for example A, B and C. The rest go to Other.",
        inputs: &["Input"],
        outputs: &["Other"],
        ports: Some(filter_ports),
        params: FILTER_PARAMS,
        run: run_filter,
        summary: |p| p.get("attribute").unwrap_or_default(),
    },
    NodeType {
        name: "AttributeValidator",
        title: "Check attributes",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Checks the values of attributes: that they are there, of a type, no longer than a length, within a range, or one of a list. Features that pass go to Passed, the others to Failed with what is wrong.",
        inputs: &["Input"],
        outputs: &["Passed", "Failed"],
        ports: None,
        params: ATTRIBUTE_CHECK_PARAMS,
        run: run_attribute_check,
        summary: |p| {
            let n = checks_of(p.values, "checks").len();
            if n == 1 { "1 attribute".into() } else { format!("{} attributes", n) }
        },
    },
    NodeType {
        name: "Sampler",
        title: "Take first",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Takes the first features that come, such as 5, or the first of each group. The rest go to Rest. Sort first to take the largest or the newest.",
        inputs: &["Input"],
        outputs: &["Taken", "Rest"],
        ports: None,
        params: TAKE_PARAMS,
        run: run_take,
        summary: |p| {
            let n = p.get("count").unwrap_or_else(|| "5".into());
            match p.list("group_by").as_slice() {
                [] => format!("first {}", n),
                by => format!("first {} of each {}", n, by.join("+")),
            }
        },
    },
];
