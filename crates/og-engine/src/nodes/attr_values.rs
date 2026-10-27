//! Tools that make or change values: Create attribute, Round numbers, Count, Stamp date and
//! time, Classify by range and Map values.

use super::*;
use crate::model::{err, fmt_count, fmt_real, Field, FieldType, Result, Table, Value};
use std::collections::HashMap;

// ---------- Create attribute (AttributeCreator) ----------

pub(super) const CREATOR_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "", hint: "An attribute that already exists is overwritten" },
    ParamSpec { key: "expression", label: "Expression", kind: ParamKind::Expression("Input"), default: "", hint: "For example: round(population / 1000, 1)" },
];

pub(super) fn run_creator(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let name = p.need("name", "Give the attribute to create a name.")?;
    let e = expr::compile(&p.need("expression", "Give an expression for the value of the attribute.")?, &table.field_names())?;
    let geographic = ctx.geographic(table.crs);
    let values: Vec<Value> = table
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| e.eval(&Row { feature: f, index: i, geographic }).map(Value::stored).map_err(|e| at_row(e, i)))
        .collect::<Result<_>>()?;
    Ok(vec![("Output".into(), with_column(table, &name, values))])
}

/// Adds a column, or overwrites one that already has that name.
pub(super) fn with_column(table: &Table, name: &str, values: Vec<Value>) -> Table {
    let ty = infer_type(values.iter());
    let mut out = table.clone();
    let idx = match out.field_index(name) {
        Some(i) => {
            out.fields[i].ty = ty;
            i
        }
        None => {
            out.fields.push(Field::new(name, ty));
            out.features.iter_mut().for_each(|f| f.attrs.push(Value::Null));
            out.fields.len() - 1
        }
    };
    for (f, v) in out.features.iter_mut().zip(values) {
        f.attrs[idx] = coerce(v, ty);
    }
    out
}

// ---------- Round numbers (Rounder) ----------

/// A number of decimals as it is written in a form: a whole number from 0 to 15.
pub(super) fn decimals(text: &str) -> Option<u32> {
    text.trim().parse::<u32>().ok().filter(|d| *d <= 15)
}

/// A number rounded to a number of decimals, halves away from zero.
pub fn round_to(x: f64, decimals: u32) -> f64 {
    let scale = 10f64.powi(decimals as i32);
    let r = (x * scale).round() / scale;
    // Text that came out of a division is cleaned of what floating point adds, so that
    // 0.1 + 0.2 rounded to two decimals is shown as 0.3.
    format!("{:.*}", decimals as usize, r).parse().unwrap_or(r)
}

pub(super) const ROUNDER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attributes", label: "Attributes", kind: ParamKind::Attributes("Input"), default: "", hint: "The decimal numbers to round" },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "0", hint: "From 0 to 15. Halves are rounded away from zero" },
];

pub(super) fn run_rounder(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let names = p.list("attributes");
    if names.is_empty() {
        return err("Choose the attributes to round.");
    }
    let text = p.get("decimals").unwrap_or_default();
    let Some(d) = decimals(&text) else {
        return err(format!("The number of decimals must be a whole number from 0 to 15, not \"{}\".", text));
    };
    let mut out = table.clone();
    let mut text_fields = Vec::new();
    for name in &names {
        let i =
            table.field_index(name).ok_or_else(|| GbError(format!("There is no attribute {}. The stream has: {}.", name, table.field_names().join(", "))))?;
        match table.fields[i].ty {
            FieldType::Real => {
                for f in &mut out.features {
                    if let Value::Real(x) = f.attrs[i] {
                        f.attrs[i] = Value::Real(round_to(x, d));
                    }
                }
            }
            // Whole numbers have no decimals to round.
            FieldType::Int => {}
            FieldType::Text => text_fields.push(name.clone()),
        }
    }
    if !text_fields.is_empty() {
        ctx.notes.warn(format!("Left as they are, since they are text and not numbers: {}.", text_fields.join(", ")));
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Count (Counter) ----------

pub(super) const NUMBER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "name",
        label: "Attribute",
        kind: ParamKind::Text,
        default: "number",
        hint: "The attribute that gets the number. One that is there is written over",
    },
    ParamSpec { key: "start", label: "Start at", kind: ParamKind::Text, default: "1", hint: "The number of the first feature. Each next one gets one more" },
    ParamSpec {
        key: "group_by",
        label: "Start again for each",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "Features with the same values are counted on their own, each group from the start. Empty counts all in one",
    },
];

pub(super) fn run_counter(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let name = p.get("name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "number".into());
    let text = p.get("start").unwrap_or_else(|| "1".into());
    let start: i64 = text.trim().parse().map_err(|_| GbError(format!("Start at must be a whole number, not \"{}\".", text.trim())))?;
    let by = p.list("group_by");
    if by.contains(&name) {
        return err(format!("The attribute {} is also what is grouped by. Give the number another name.", name));
    }
    let idx: Vec<usize> = by.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;
    let mut next: HashMap<Vec<Option<String>>, i64> = HashMap::new();
    let values: Vec<Value> = table
        .features
        .iter()
        .map(|f| {
            let key: Vec<Option<String>> = idx.iter().map(|i| f.attrs[*i].key()).collect();
            let n = next.entry(key).or_insert(start);
            let v = *n;
            *n += 1;
            Value::Int(v)
        })
        .collect();
    let mut out = with_column(table, &name, values);
    if let Some(i) = out.field_index(&name) {
        out.fields[i].ty = FieldType::Int;
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Stamp date and time (DateTimeStamper) ----------

pub(super) const STAMPS: &[(&str, &str)] = &[("date", "Date (2026-10-11)"), ("time", "Time (14:05:09)"), ("datetime", "Date and time (2026-10-11 14:05:09)")];
pub(super) const STAMPER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "value",
        label: "Add",
        kind: ParamKind::Choice(STAMPS),
        default: "date",
        hint: "The time when the run started, the same for all features and all nodes",
    },
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "", hint: "Empty uses _date, _time or _timestamp" },
];

pub(super) fn stamp_name(p: &P) -> String {
    p.get("name").unwrap_or_else(|| match p.get("value").as_deref() {
        Some("time") => "_time".to_string(),
        Some("datetime") => "_timestamp".to_string(),
        _ => "_date".to_string(),
    })
}

pub(super) fn run_stamper(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let format = match p.get("value").as_deref() {
        Some("time") => "%H:%M:%S",
        Some("datetime") => "%Y-%m-%d %H:%M:%S",
        _ => "%Y-%m-%d",
    };
    let text = ctx.now.format(format).to_string();
    let mut out = with_column(table, &stamp_name(p), vec![Value::Text(text); table.len()]);
    // With no features there is nothing to tell the type from, and a stamp is always text.
    let at = out.field_index(&stamp_name(p)).expect("the column was just added");
    out.fields[at].ty = FieldType::Text;
    Ok(vec![("Output".into(), out)])
}

// ---------- Map values ----------

pub(super) const OTHERWISE: &[(&str, &str)] = &[("keep", "Keep the value"), ("empty", "Leave empty"), ("text", "Write this:")];

pub(super) const MAPPER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "Attribute", kind: ParamKind::Attribute("Input"), default: "", hint: "The attribute whose values are looked up" },
    ParamSpec { key: "map", label: "Values", kind: ParamKind::Mapping("Input", "attribute"), default: "", hint: "Each value and what it becomes" },
    ParamSpec {
        key: "result",
        label: "Write to",
        kind: ParamKind::Text,
        default: "",
        hint: "A new attribute for the result. Empty writes over the attribute itself",
    },
    ParamSpec { key: "otherwise", label: "Values not in the list", kind: ParamKind::Choice(OTHERWISE), default: "keep", hint: "" },
    ParamSpec { key: "default", label: "Text for those", kind: ParamKind::Text, default: "", hint: "Written for a value that is not in the list" },
];

pub(super) fn run_mapper(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute whose values are to be looked up.")?;
    let from = table.need_field(&attr)?;
    let pairs = p.mapping("map");
    if pairs.is_empty() {
        ctx.notes.warn("The list of values is empty, so nothing was changed.".to_string());
    }
    let lookup: HashMap<String, String> = pairs.iter().map(|(a, b)| (a.trim().to_string(), b.clone())).collect();
    let otherwise = p.get("otherwise").unwrap_or_else(|| "keep".into());
    let fallback = p.exact("default");
    let target = p.get("result").filter(|r| !r.trim().is_empty()).unwrap_or_else(|| attr.clone());
    let mut missing = 0usize;
    let texts: Vec<Option<String>> = table
        .features
        .iter()
        .map(|f| {
            let v = &f.attrs[from];
            let key = v.key().unwrap_or_default();
            match lookup.get(key.trim()) {
                Some(to) => Some(to.clone()),
                None => {
                    missing += usize::from(!v.is_null());
                    match otherwise.as_str() {
                        "empty" => None,
                        "text" => Some(fallback.clone()),
                        _ => v.key(),
                    }
                }
            }
        })
        .collect();
    if missing > 0 && !pairs.is_empty() {
        ctx.notes.info(format!("{} values were not in the list", fmt_count(missing as u64)));
    }
    // Numbers stay numbers when every result is one.
    let all_int = texts.iter().flatten().all(|t| t.trim().parse::<i64>().is_ok());
    let all_num = texts.iter().flatten().all(|t| t.trim().replace(',', ".").parse::<f64>().is_ok());
    let any = texts.iter().any(|t| t.is_some());
    let (ty, values): (FieldType, Vec<Value>) = if any && all_int {
        (FieldType::Int, texts.iter().map(|t| t.as_ref().and_then(|t| t.trim().parse().ok()).map_or(Value::Null, Value::Int)).collect())
    } else if any && all_num {
        (FieldType::Real, texts.iter().map(|t| t.as_ref().and_then(|t| t.trim().replace(',', ".").parse().ok()).map_or(Value::Null, Value::Real)).collect())
    } else {
        (FieldType::Text, texts.into_iter().map(|t| t.map_or(Value::Null, Value::Text)).collect())
    };
    let mut out = table.clone();
    let at = match out.field_index(&target) {
        Some(i) => i,
        None => {
            out.fields.push(Field::new(target.clone(), ty));
            for f in out.features.iter_mut() {
                f.attrs.push(Value::Null);
            }
            out.fields.len() - 1
        }
    };
    out.fields[at].ty = ty;
    for (f, v) in out.features.iter_mut().zip(values) {
        f.attrs[at] = v;
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Classify by range ----------

pub(super) const METHODS: &[(&str, &str)] = &[("own", "Ranges you give"), ("equal", "Equal intervals"), ("quantile", "The same number in each")];

pub(super) const RANGE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "Attribute", kind: ParamKind::Attribute("Input"), default: "", hint: "A number to look up in the ranges" },
    ParamSpec { key: "method", label: "Ranges", kind: ParamKind::Choice(METHODS), default: "own", hint: "" },
    ParamSpec {
        key: "ranges",
        label: "From, to and value",
        kind: ParamKind::Ranges("Input"),
        default: "",
        hint: "From and to are both in the range: 1 to 100 and 101 to 200. An empty from or to has no end. The first range that fits is used",
    },
    ParamSpec {
        key: "classes",
        label: "Number of classes",
        kind: ParamKind::Text,
        default: "5",
        hint: "Equal intervals split from the smallest to the largest value; the same number in each puts as many features in each",
    },
    ParamSpec { key: "decimals", label: "Decimals in the labels", kind: ParamKind::Text, default: "0", hint: "" },
    ParamSpec { key: "result", label: "Write to", kind: ParamKind::Text, default: "class", hint: "The attribute that gets the value of the range" },
    ParamSpec {
        key: "otherwise",
        label: "Outside the ranges",
        kind: ParamKind::Text,
        default: "",
        hint: "Written for a value that is in no range, and for an empty one. Empty leaves it empty",
    },
];

/// A range: from and to, either open, and the value it gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub value: String,
}

/// The ranges as they are stored: a JSON list of [from, to, value].
pub fn ranges_of(params: &Params, key: &str) -> Vec<Range> {
    let Some(serde_json::Value::Array(rows)) = params.get(key) else { return Vec::new() };
    let text = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let number = |v: &serde_json::Value| crate::model::parse_number(&text(v));
    rows.iter()
        .filter_map(|r| {
            let a = r.as_array()?;
            Some(Range { from: a.first().and_then(number), to: a.get(1).and_then(number), value: a.get(2).map(text).unwrap_or_default() })
        })
        .filter(|r| r.from.is_some() || r.to.is_some())
        .collect()
}

pub(super) fn label(lo: f64, hi: f64, decimals: usize) -> String {
    let f = |x: f64| fmt_real((x * 10f64.powi(decimals as i32)).round() / 10f64.powi(decimals as i32));
    format!("{} – {}", f(lo), f(hi))
}

/// Ranges worked out from the values: equal steps from the smallest to the largest, or
/// breaks with as many values in each class.
pub fn make_ranges(values: &[f64], classes: usize, quantile: bool, decimals: usize) -> Vec<Range> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() || classes == 0 {
        return Vec::new();
    }
    v.sort_by(f64::total_cmp);
    let (lo, hi) = (v[0], v[v.len() - 1]);
    let breaks: Vec<f64> = if quantile {
        (0..=classes).map(|k| v[((v.len() - 1) as f64 * k as f64 / classes as f64).round() as usize]).collect()
    } else {
        (0..=classes).map(|k| lo + (hi - lo) * k as f64 / classes as f64).collect()
    };
    let mut out: Vec<Range> = Vec::new();
    for k in 0..classes {
        let (a, b) = (breaks[k], breaks[k + 1]);
        if k > 0 && b <= breaks[k] && quantile {
            continue;
        }
        // Each range ends just below the next, and the last one at the largest value.
        out.push(Range { from: Some(a), to: if k + 1 == classes { Some(b) } else { None }, value: label(a, b, decimals) });
    }
    // An open end is closed by the start of the next range, which is checked first.
    let starts: Vec<f64> = out.iter().map(|r| r.from.unwrap_or(f64::NEG_INFINITY)).collect();
    for (i, r) in out.iter_mut().enumerate() {
        if r.to.is_none() {
            r.to = starts.get(i + 1).map(|s| s - f64::EPSILON * s.abs().max(1.0));
        }
    }
    out
}

pub(super) fn run_ranges(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute whose values are to be put in ranges.")?;
    let at = table.need_field(&attr)?;
    let result = p.get("result").filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "class".into());
    let otherwise = p.exact("otherwise");
    let decimals = number_param(p, "decimals", "The decimals")?.unwrap_or(0.0).clamp(0.0, 10.0) as usize;
    let number = |v: &Value| match v {
        Value::Int(i) => Some(*i as f64),
        Value::Real(r) => Some(*r),
        Value::Text(t) => crate::model::parse_number(t),
        _ => None,
    };
    let ranges = match p.get("method").as_deref() {
        Some(m @ ("equal" | "quantile")) => {
            let n = number_param(p, "classes", "The number of classes")?.unwrap_or(5.0);
            if !(1.0..=100.0).contains(&n) {
                return err("The number of classes must be from 1 to 100.");
            }
            let values: Vec<f64> = table.features.iter().filter_map(|f| number(&f.attrs[at])).collect();
            let r = make_ranges(&values, n as usize, m == "quantile", decimals);
            ctx.notes.info(format!("ranges: {}", r.iter().map(|x| x.value.clone()).collect::<Vec<_>>().join(", ")));
            r
        }
        _ => ranges_of(p.values, "ranges"),
    };
    if ranges.is_empty() {
        return err("Give at least one range, with a from or a to.");
    }
    let mut out = table.clone();
    let texts: Vec<Option<String>> = table
        .features
        .iter()
        .map(|f| {
            let v = number(&f.attrs[at]);
            let hit = v.and_then(|x| ranges.iter().find(|r| r.from.is_none_or(|a| x >= a) && r.to.is_none_or(|b| x <= b)));
            match hit {
                Some(r) => Some(r.value.clone()),
                None if otherwise.is_empty() => None,
                None => Some(otherwise.clone()),
            }
        })
        .collect();
    let outside = texts.iter().zip(&table.features).filter(|(t, f)| t.is_none() && !f.attrs[at].is_null()).count();
    if outside > 0 {
        ctx.notes.info(format!("{} values are in no range", fmt_count(outside as u64)));
    }
    // Numbers stay numbers when every value is one.
    let all_int = texts.iter().flatten().all(|t| t.trim().parse::<i64>().is_ok());
    let any = texts.iter().any(|t| t.is_some());
    let (ty, values): (FieldType, Vec<Value>) = if any && all_int {
        (FieldType::Int, texts.iter().map(|t| t.as_ref().and_then(|t| t.trim().parse().ok()).map_or(Value::Null, Value::Int)).collect())
    } else {
        (FieldType::Text, texts.into_iter().map(|t| t.map_or(Value::Null, Value::Text)).collect())
    };
    let i = match out.field_index(&result) {
        Some(i) => i,
        None => {
            out.fields.push(Field::new(result.clone(), ty));
            for f in out.features.iter_mut() {
                f.attrs.push(Value::Null);
            }
            out.fields.len() - 1
        }
    };
    out.fields[i].ty = ty;
    for (f, v) in out.features.iter_mut().zip(values) {
        f.attrs[i] = v;
    }
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "AttributeCreator",
        title: "Create attribute",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Creates or overwrites an attribute with the value of an expression, as the field calculator of QGIS: other attributes, text, arithmetic and powers, CASE WHEN, functions for text, numbers, dates and regular expressions, and $area, $length, $perimeter, $x, $y and $id. The value for the first feature is shown under the expression.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: CREATOR_PARAMS,
        run: run_creator,
        summary: |p| format!("{} = {}", p.get("name").unwrap_or_else(|| "?".into()), p.get("expression").unwrap_or_else(|| "?".into())),
    },
    NodeType {
        name: "DateTimeStamper",
        title: "Stamp date and time",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Adds the date, the time, or both, as an attribute. It is the time when the run started, so every feature in a run gets the same value.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: STAMPER_PARAMS,
        run: run_stamper,
        summary: |p| format!("{} = {}", stamp_name(p), p.get("value").unwrap_or_default()),
    },
    NodeType {
        name: "Rounder",
        title: "Round numbers",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Rounds decimal numbers in one or more attributes to a number of decimals.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: ROUNDER_PARAMS,
        run: run_rounder,
        summary: |p| {
            let attrs = p.list("attributes");
            if attrs.is_empty() {
                String::new()
            } else {
                format!("{} · {} decimals", attrs.join(", "), p.get("decimals").unwrap_or_default())
            }
        },
    },
    NodeType {
        name: "Counter",
        title: "Count",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Numbers the features in the order they come: 1, 2, 3 and so on, from the number you choose, in an attribute. Can start again for each group.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: NUMBER_PARAMS,
        run: run_counter,
        summary: |p| format!("{} from {}", p.get("name").unwrap_or_else(|| "number".into()), p.get("start").unwrap_or_else(|| "1".into())),
    },
    NodeType {
        name: "RangeMapper",
        title: "Classify by range",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Gives each feature a value by the range its number falls in: 1 to 100 gives A, 101 to 200 gives B, and so on. The ranges can be given, or made from the data as equal intervals or with as many features in each.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: RANGE_PARAMS,
        run: run_ranges,
        summary: |p| {
            let a = p.get("attribute").unwrap_or_default();
            match p.get("method").as_deref() {
                Some("equal") => format!("{} · {} equal intervals", a, p.get("classes").unwrap_or_else(|| "5".into())),
                Some("quantile") => format!("{} · {} classes", a, p.get("classes").unwrap_or_else(|| "5".into())),
                _ => format!("{} · {} ranges", a, ranges_of(p.values, "ranges").len()),
            }
        },
    },
    NodeType {
        name: "ValueMapper",
        title: "Map values",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Looks up each value of an attribute in a list and writes what it becomes, such as 1 to Health centre. The values can be taken from the data. Values not in the list are kept, left empty or given a text.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: MAPPER_PARAMS,
        run: run_mapper,
        summary: |p| {
            let n = p.mapping("map").len();
            match p.get("attribute") {
                Some(a) => format!("{} · {} values", a, n),
                None => String::new(),
            }
        },
    },
];
