//! Statistics on the features, added in 0.96.6: values put in classes for a map, shares,
//! ranks and running totals, change over time, rates per inhabitant with age
//! standardisation, smoothing and suppression of small numbers, and linear regression.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real, Feature, Field, FieldType, Table, Value};
use std::collections::HashMap;

/// Adds columns, or replaces those that have the same names.
fn add_columns(table: &Table, cols: Vec<(String, FieldType, Vec<Value>)>) -> Table {
    let mut out = table.clone();
    for (name, ty, values) in cols {
        let at = match out.field_index(&name) {
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
            f.attrs[at] = v;
        }
    }
    out
}

fn real(x: Option<f64>) -> Value {
    x.filter(|v| v.is_finite()).map_or(Value::Null, Value::Real)
}

/// A number of an attribute, also when it is written as text.
fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

/// A number rounded to so many decimals, for the names of classes.
fn round_text(x: f64, decimals: usize) -> String {
    crate::chart::number(x, decimals, true).replace('\u{a0}', " ")
}

// ---------- Classify values (Classifier) ----------

const CLASS_METHODS: &[(&str, &str)] = &[
    ("jenks", "Natural breaks (Jenks)"),
    ("quantile", "Quantiles: as many in each class"),
    ("equal", "Equal intervals"),
    ("stddev", "Standard deviations from the mean"),
    ("manual", "Breaks of my own"),
];

const CLASS_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "Values", kind: ParamKind::Attribute("Input"), default: "", hint: "The numbers to put in classes" },
    ParamSpec { key: "method", label: "Classes by", kind: ParamKind::Choice(CLASS_METHODS), default: "jenks", hint: "" },
    ParamSpec { key: "classes", label: "Number of classes", kind: ParamKind::Text, default: "5", hint: "From 2 to 12" },
    ParamSpec {
        key: "breaks",
        label: "Breaks",
        kind: ParamKind::Text,
        default: "",
        hint: "For breaks of your own: where one class ends and the next begins, such as 10, 50, 100",
    },
    ParamSpec { key: "name", label: "Attribute with the class", kind: ParamKind::Text, default: "class", hint: "1 for the lowest values" },
    ParamSpec {
        key: "label_name",
        label: "Attribute with its name",
        kind: ParamKind::Text,
        default: "class_label",
        hint: "Such as 10–50, for the legend of a map",
    },
    ParamSpec { key: "decimals", label: "Decimals in the names", kind: ParamKind::Text, default: "0", hint: "" },
];

/// The breaks of natural classes: Fisher's exact method, which Jenks's is, on sorted values.
/// Returns the upper bounds of each class but the last.
pub(super) fn jenks(sorted: &[f64], k: usize) -> Vec<f64> {
    let n = sorted.len();
    if k >= n {
        return sorted[..n.saturating_sub(1)].to_vec();
    }
    // Sums for the variance of any run of values.
    let mut s = vec![0.0; n + 1];
    let mut s2 = vec![0.0; n + 1];
    for (i, v) in sorted.iter().enumerate() {
        s[i + 1] = s[i] + v;
        s2[i + 1] = s2[i] + v * v;
    }
    let ssd = |a: usize, b: usize| -> f64 {
        // Values a..b (b excluded).
        let m = (b - a) as f64;
        let sum = s[b] - s[a];
        (s2[b] - s2[a]) - sum * sum / m
    };
    // cost[j][i]: the least squared deviations of the first i values in j + 1 classes.
    let mut cost = vec![vec![f64::INFINITY; n + 1]; k];
    let mut from = vec![vec![0usize; n + 1]; k];
    for (i, c) in cost[0].iter_mut().enumerate().skip(1) {
        *c = ssd(0, i);
    }
    for j in 1..k {
        for i in (j + 1)..=n {
            for m in j..i {
                let c = cost[j - 1][m] + ssd(m, i);
                if c < cost[j][i] {
                    cost[j][i] = c;
                    from[j][i] = m;
                }
            }
        }
    }
    let mut ends = Vec::new();
    let mut i = n;
    for j in (1..k).rev() {
        let m = from[j][i];
        ends.push(sorted[m - 1]);
        i = m;
    }
    ends.reverse();
    ends
}

fn run_classify(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let ai = t.need_field(&p.need("attribute", "Choose the values to put in classes.")?)?;
    let method = p.get("method").unwrap_or_else(|| "jenks".into());
    let decimals = number_param(p, "decimals", "The decimals")?.unwrap_or(0.0).clamp(0.0, 6.0) as usize;
    let mut sorted: Vec<f64> = t.features.iter().filter_map(|f| num(&f.attrs[ai])).collect();
    if sorted.is_empty() {
        return err("No feature has a number to put in a class.");
    }
    sorted.sort_by(|a, b| a.total_cmp(b));
    let (lo, hi) = (sorted[0], sorted[sorted.len() - 1]);
    let k = number_param(p, "classes", "The number of classes")?.unwrap_or(5.0).round() as usize;
    if method != "manual" && !(2..=12).contains(&k) {
        return err("The number of classes must be from 2 to 12.");
    }
    // The upper bounds of every class but the last.
    let mut ends: Vec<f64> = match method.as_str() {
        "equal" => (1..k).map(|i| lo + (hi - lo) * i as f64 / k as f64).collect(),
        "quantile" => (1..k)
            .map(|i| {
                let at = (sorted.len() as f64 * i as f64 / k as f64).ceil() as usize;
                sorted[at.clamp(1, sorted.len()) - 1]
            })
            .collect(),
        "stddev" => {
            let n = sorted.len() as f64;
            let mean = sorted.iter().sum::<f64>() / n;
            let sd = (sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
            if sd == 0.0 {
                Vec::new()
            } else {
                // A class a standard deviation wide, the middle ones on each side of the mean.
                let half = (k as f64 - 2.0) / 2.0;
                (0..k - 1).map(|i| mean + (i as f64 - half) * sd).collect()
            }
        }
        "manual" => {
            let mut b: Vec<f64> = p
                .get("breaks")
                .unwrap_or_default()
                .split([',', ';', ' '])
                .filter(|x| !x.trim().is_empty())
                .map(|x| crate::model::parse_number(x.trim()).ok_or_else(|| GbError(format!("A break is a number, not \"{}\".", x.trim()))))
                .collect::<Result<_>>()?;
            if b.is_empty() {
                return err("Write the breaks, such as 10, 50, 100.");
            }
            b.sort_by(|a, c| a.total_cmp(c));
            b
        }
        _ => {
            // Fisher-Jenks is slow on very many values: they are thinned to 5 000 evenly
            // spread through the sorted values, which gives the same breaks for a map.
            let thinned: Vec<f64> = if sorted.len() > 5000 {
                ctx.notes.info(format!("the natural breaks found among 5 000 of the {} values, spread evenly", fmt_count(sorted.len() as u64)));
                (0..5000).map(|i| sorted[i * (sorted.len() - 1) / 4999]).collect()
            } else {
                sorted.clone()
            };
            jenks(&thinned, k)
        }
    };
    ends.dedup_by(|a, b| a == b);
    let class_of = |v: f64| ends.iter().position(|e| v <= *e).unwrap_or(ends.len()) + 1;
    let n_classes = ends.len() + 1;
    let bound = |i: usize| -> (f64, f64) {
        let from = if i == 0 { lo } else { ends[i - 1] };
        let to = if i == ends.len() { hi } else { ends[i] };
        (from, to)
    };
    let labels: Vec<String> = (0..n_classes)
        .map(|i| {
            let (a, b) = bound(i);
            format!("{} – {}", round_text(a, decimals), round_text(b, decimals))
        })
        .collect();
    let mut counts = vec![0usize; n_classes];
    let (mut classes, mut names) = (Vec::with_capacity(t.len()), Vec::with_capacity(t.len()));
    for f in &t.features {
        match num(&f.attrs[ai]) {
            Some(v) => {
                let c = class_of(v);
                counts[c - 1] += 1;
                classes.push(Value::Int(c as i64));
                names.push(Value::Text(labels[c - 1].clone()));
            }
            None => {
                classes.push(Value::Null);
                names.push(Value::Null);
            }
        }
    }
    let name = p.get("name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "class".into());
    let label_name = p.get("label_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "class_label".into());
    let out = add_columns(t, vec![(name.clone(), FieldType::Int, classes), (label_name.clone(), FieldType::Text, names)]);
    let mut summary = Table {
        fields: vec![
            Field::new(name, FieldType::Int),
            Field::new(label_name, FieldType::Text),
            Field::new("from", FieldType::Real),
            Field::new("to", FieldType::Real),
            Field::new("count", FieldType::Int),
        ],
        ..Default::default()
    };
    for i in 0..n_classes {
        let (a, b) = bound(i);
        summary.features.push(Feature::new(
            vec![Value::Int(i as i64 + 1), Value::Text(labels[i].clone()), Value::Real(a), Value::Real(b), Value::Int(counts[i] as i64)],
            None,
        ));
    }
    if counts.contains(&0) {
        ctx.notes.warn("A class has no features in it. Fewer classes, or another way to make them, may suit the values better.");
    }
    ctx.notes.info(format!("{} classes: {}", n_classes, labels.join(", ")));
    Ok(vec![("Classified".into(), out), ("Classes".into(), summary)])
}

// ---------- Shares, ranks and running totals (RunningStatistics) ----------

const RUNNING_KINDS: &[(&str, &str)] = &[
    ("share", "Share of the total, in %"),
    ("rank", "Rank, 1 for the largest"),
    ("cumulative", "Running total, in the order they come"),
    ("cumulative_share", "Running share of the total, in %"),
    ("moving", "Moving mean, in the order they come"),
];

const RUNNING_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "Values", kind: ParamKind::Attribute("Input"), default: "", hint: "The numbers to work with" },
    ParamSpec {
        key: "what",
        label: "Add",
        kind: ParamKind::Picks(RUNNING_KINDS),
        default: "share",
        hint: "Each becomes an attribute after the name of the values, such as people_share. Sort the features first for running totals and moving means",
    },
    ParamSpec { key: "group_by", label: "For each", kind: ParamKind::Attributes("Input"), default: "", hint: "Such as the municipality. Empty: all together" },
    ParamSpec { key: "window", label: "Moving mean of", kind: ParamKind::Text, default: "3", hint: "How many values, this one and those before it" },
];

fn run_running(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let attr = p.need("attribute", "Choose the values.")?;
    let ai = t.need_field(&attr)?;
    let gi: Vec<usize> = p.list("group_by").iter().map(|n| t.need_field(n)).collect::<Result<_>>()?;
    let mut what = p.list("what");
    if what.is_empty() {
        what = vec!["share".into()];
    }
    let window = number_param(p, "window", "The moving mean")?.unwrap_or(3.0).round().max(1.0) as usize;
    let n = t.len();
    let (mut share, mut rank, mut cum, mut cum_share, mut moving) =
        (vec![Value::Null; n], vec![Value::Null; n], vec![Value::Null; n], vec![Value::Null; n], vec![Value::Null; n]);
    for members in groups(t, &gi, |_| true) {
        let vals: Vec<Option<f64>> = members.iter().map(|m| num(&t.features[*m].attrs[ai])).collect();
        let total: f64 = vals.iter().flatten().sum();
        // Ranks: the same value gets the same rank, and the next gets the rank after the ties.
        let mut order: Vec<(usize, f64)> = vals.iter().enumerate().filter_map(|(i, v)| v.map(|v| (i, v))).collect();
        order.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut ranks = vec![None; vals.len()];
        for (pos, (i, v)) in order.iter().enumerate() {
            let r = if pos > 0 && order[pos - 1].1 == *v { ranks[order[pos - 1].0].unwrap_or(pos + 1) } else { pos + 1 };
            ranks[*i] = Some(r);
        }
        let mut running = 0.0;
        let mut last: Vec<f64> = Vec::new();
        for (k, m) in members.iter().enumerate() {
            let Some(v) = vals[k] else { continue };
            running += v;
            last.push(v);
            if last.len() > window {
                last.remove(0);
            }
            share[*m] = real((total != 0.0).then(|| v / total * 100.0));
            rank[*m] = ranks[k].map_or(Value::Null, |r| Value::Int(r as i64));
            cum[*m] = Value::Real(running);
            cum_share[*m] = real((total != 0.0).then(|| running / total * 100.0));
            moving[*m] = real((last.len() == window).then(|| last.iter().sum::<f64>() / window as f64));
        }
    }
    let mut cols = Vec::new();
    for w in &what {
        match w.as_str() {
            "share" => cols.push((format!("{}_share", attr), FieldType::Real, std::mem::take(&mut share))),
            "rank" => cols.push((format!("{}_rank", attr), FieldType::Int, std::mem::take(&mut rank))),
            "cumulative" => cols.push((format!("{}_cumulative", attr), FieldType::Real, std::mem::take(&mut cum))),
            "cumulative_share" => cols.push((format!("{}_cumulative_share", attr), FieldType::Real, std::mem::take(&mut cum_share))),
            "moving" => cols.push((format!("{}_moving", attr), FieldType::Real, std::mem::take(&mut moving))),
            other => return err(format!("\"{}\" is not one of the values that can be added.", other)),
        }
    }
    ctx.notes.info(format!("added {}", cols.iter().map(|c| c.0.clone()).collect::<Vec<_>>().join(", ")));
    Ok(vec![("Output".into(), add_columns(t, cols))])
}

// ---------- Change over time (ChangeOverTime) ----------

const CHANGE_KINDS: &[(&str, &str)] = &[
    ("change", "Change since the time before"),
    ("percent", "Change since the time before, in %"),
    ("index", "Index: the base time is 100"),
    ("growth", "Yearly growth since the base time, in %"),
];

const CHANGE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "Values", kind: ParamKind::Attribute("Input"), default: "", hint: "Such as the number of inhabitants" },
    ParamSpec { key: "time", label: "Time", kind: ParamKind::Attribute("Input"), default: "", hint: "A year, such as 2024, or a date" },
    ParamSpec {
        key: "series",
        label: "For each",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "What the values are of, such as the municipality. Empty: all are one series",
    },
    ParamSpec {
        key: "what",
        label: "Add",
        kind: ParamKind::Picks(CHANGE_KINDS),
        default: "percent",
        hint: "Each becomes an attribute after the name of the values",
    },
    ParamSpec {
        key: "base",
        label: "Base time",
        kind: ParamKind::Text,
        default: "",
        hint: "For the index and the yearly growth, such as 2015. Empty: the first time of each series",
    },
];

/// A time as a year with its part: 2024 for 2024, 2024.5 for the 2nd of July 2024.
fn year_of(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Text(s) => {
            let s = s.trim();
            if let Some(n) = crate::model::parse_number(s) {
                return Some(n);
            }
            let first10: String = s.chars().take(10).collect();
            let d = chrono::NaiveDate::parse_from_str(&first10, "%Y-%m-%d")
                .ok()
                .or_else(|| chrono::NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d").ok())?;
            use chrono::Datelike;
            let days = if d.leap_year() { 366.0 } else { 365.0 };
            Some(d.year() as f64 + (d.ordinal0() as f64) / days)
        }
        _ => None,
    }
}

fn run_change(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let attr = p.need("attribute", "Choose the values.")?;
    let ai = t.need_field(&attr)?;
    let ti = t.need_field(&p.need("time", "Choose the attribute with the time, such as the year.")?)?;
    let gi: Vec<usize> = p.list("series").iter().map(|n| t.need_field(n)).collect::<Result<_>>()?;
    let mut what = p.list("what");
    if what.is_empty() {
        what = vec!["percent".into()];
    }
    let base_given = match p.get("base").filter(|b| !b.trim().is_empty()) {
        Some(b) => Some(year_of(&Value::Text(b.clone())).ok_or_else(|| GbError(format!("The base time is a year or a date, not \"{}\".", b)))?),
        None => None,
    };
    let n = t.len();
    let (mut change, mut pct, mut index, mut growth) = (vec![Value::Null; n], vec![Value::Null; n], vec![Value::Null; n], vec![Value::Null; n]);
    let (mut no_base, mut twice) = (0usize, 0usize);
    for members in groups(t, &gi, |_| true) {
        let mut pts: Vec<(usize, f64, f64)> =
            members.iter().filter_map(|m| Some((*m, year_of(&t.features[*m].attrs[ti])?, num(&t.features[*m].attrs[ai])?))).collect();
        pts.sort_by(|a, b| a.1.total_cmp(&b.1));
        twice += pts.windows(2).filter(|w| w[0].1 == w[1].1).count();
        let base = match base_given {
            Some(b) => pts.iter().find(|x| (x.1 - b).abs() < 1e-9).copied(),
            None => pts.first().copied(),
        };
        if base.is_none() && !pts.is_empty() {
            no_base += 1;
        }
        for (k, (m, year, v)) in pts.iter().enumerate() {
            if k > 0 {
                let prev = pts[k - 1].2;
                change[*m] = Value::Real(v - prev);
                pct[*m] = real((prev != 0.0).then(|| (v - prev) / prev.abs() * 100.0));
            }
            if let Some((_, by, bv)) = base {
                index[*m] = real((bv != 0.0).then(|| v / bv * 100.0));
                let years = year - by;
                growth[*m] = real((bv > 0.0 && *v > 0.0 && years.abs() > 1e-9).then(|| ((v / bv).powf(1.0 / years) - 1.0) * 100.0));
            }
        }
    }
    if twice > 0 {
        ctx.notes.warn(format!(
            "{} times come twice in the same series. Choose under For each what the values are of, such as the municipality.",
            fmt_count(twice as u64)
        ));
    }
    if no_base > 0 {
        ctx.notes.warn(format!("{} series have no value at the base time, and get no index or growth.", fmt_count(no_base as u64)));
    }
    let mut cols = Vec::new();
    for w in &what {
        match w.as_str() {
            "change" => cols.push((format!("{}_change", attr), FieldType::Real, std::mem::take(&mut change))),
            "percent" => cols.push((format!("{}_change_pct", attr), FieldType::Real, std::mem::take(&mut pct))),
            "index" => cols.push((format!("{}_index", attr), FieldType::Real, std::mem::take(&mut index))),
            "growth" => cols.push((format!("{}_growth_pct", attr), FieldType::Real, std::mem::take(&mut growth))),
            other => return err(format!("\"{}\" is not one of the values that can be added.", other)),
        }
    }
    ctx.notes.info(format!("added {}", cols.iter().map(|c| c.0.clone()).collect::<Vec<_>>().join(", ")));
    Ok(vec![("Output".into(), add_columns(t, cols))])
}

// ---------- Rates (RateCalculator) ----------

const RATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "count",
        label: "Cases",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "How many, such as the number of patients or of accidents",
    },
    ParamSpec { key: "population", label: "Population", kind: ParamKind::Attribute("Input"), default: "", hint: "Out of how many, such as the inhabitants" },
    ParamSpec { key: "per", label: "Per", kind: ParamKind::Text, default: "1000", hint: "Such as 1000 for cases per thousand inhabitants, or 100 for percent" },
    ParamSpec {
        key: "suppress",
        label: "Hide fewer cases than",
        kind: ParamKind::Text,
        default: "",
        hint:
            "Rates of from 1 to this many cases less 1 are left empty and marked in suppressed, so that no one can be told apart, such as 5. Empty: none hidden",
    },
    ParamSpec {
        key: "smooth",
        label: "Smoothed rate",
        kind: ParamKind::Choice(&[("no", "No"), ("yes", "Yes: small populations drawn towards the mean (empirical Bayes)")]),
        default: "no",
        hint: "Small populations give rates that leap by chance; smoothed, they lean on the mean of all as much as they are small",
    },
    ParamSpec {
        key: "area",
        label: "Area, for age standardisation",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "With a row for each area and age group: the attribute that names the area. Standardised gives a row for each area",
    },
    ParamSpec {
        key: "age",
        label: "Age group",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The attribute that names the age group, such as 0-14",
    },
    ParamSpec {
        key: "standard_age",
        label: "Age group in Standard",
        kind: ParamKind::Attribute("Standard"),
        default: "",
        hint: "The same names as in Age group",
    },
    ParamSpec {
        key: "standard_population",
        label: "Population in Standard",
        kind: ParamKind::Attribute("Standard"),
        default: "",
        hint: "The standard population of each age group, such as that of all of Sweden. Empty on Standard: all areas together",
    },
];

fn run_rates(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let count = p.need("count", "Choose the attribute with the number of cases.")?;
    let ci = t.need_field(&count)?;
    let pi = t.need_field(&p.need("population", "Choose the attribute with the population.")?)?;
    let per = number_param(p, "per", "Per")?.unwrap_or(1000.0);
    if per <= 0.0 {
        return err("Per must be more than 0, such as 1000.");
    }
    let suppress = number_param(p, "suppress", "Hide fewer cases than")?.filter(|s| *s > 1.0);
    let n = t.len();
    let pairs: Vec<Option<(f64, f64)>> = t.features.iter().map(|f| Some((num(&f.attrs[ci])?, num(&f.attrs[pi])?)).filter(|(_, pop)| *pop > 0.0)).collect();
    let rates: Vec<Option<f64>> = pairs.iter().map(|x| x.map(|(c, pop)| c / pop)).collect();
    // Empirical Bayes towards the mean of all (Marshall 1991): a rate leans on the mean as
    // much as its population is small compared with the spread of the rates.
    let smoothed: Vec<Option<f64>> = if p.get("smooth").as_deref() == Some("yes") {
        let (sc, sp) = pairs.iter().flatten().fold((0.0, 0.0), |a, (c, pop)| (a.0 + c, a.1 + pop));
        let k = pairs.iter().flatten().count() as f64;
        if sp > 0.0 && k > 0.0 {
            let m = sc / sp;
            let s2 = pairs.iter().flatten().map(|(c, pop)| pop * (c / pop - m).powi(2)).sum::<f64>() / sp - m / (sp / k);
            let s2 = s2.max(0.0);
            pairs.iter().map(|x| x.map(|(c, pop)| m + if s2 + m / pop > 0.0 { s2 / (s2 + m / pop) } else { 0.0 } * (c / pop - m))).collect()
        } else {
            vec![None; n]
        }
    } else {
        Vec::new()
    };
    let hidden: Vec<bool> = pairs.iter().map(|x| matches!((x, suppress), (Some((c, _)), Some(s)) if *c > 0.0 && *c < s)).collect();
    let n_hidden = hidden.iter().filter(|h| **h).count();
    let rate_vals: Vec<Value> = rates.iter().zip(&hidden).map(|(r, h)| if *h { Value::Null } else { real(r.map(|r| r * per)) }).collect();
    let mut cols = vec![(format!("{}_rate", count), FieldType::Real, rate_vals)];
    if !smoothed.is_empty() {
        cols.push((
            format!("{}_rate_smoothed", count),
            FieldType::Real,
            smoothed.iter().zip(&hidden).map(|(r, h)| if *h { Value::Null } else { real(r.map(|r| r * per)) }).collect(),
        ));
    }
    if suppress.is_some() {
        let mut out = add_columns(t, cols);
        // The cases themselves are hidden too, where the rate is.
        for (f, h) in out.features.iter_mut().zip(&hidden) {
            if *h {
                f.attrs[ci] = Value::Null;
            }
        }
        let out = add_columns(&out, vec![("suppressed".into(), FieldType::Int, hidden.iter().map(|h| Value::Int(*h as i64)).collect())]);
        ctx.notes.info(format!("{} rates hidden for having fewer than {} cases", fmt_count(n_hidden as u64), fmt_real(suppress.unwrap_or(0.0))));
        return Ok(vec![("Rates".into(), out), ("Standardised".into(), standardised(p, inp, &pairs, &hidden, per, ctx)?)]);
    }
    let missing = pairs.iter().filter(|x| x.is_none()).count();
    if missing > 0 {
        ctx.notes.warn(format!("{} features have no number of cases, or no population above 0, and get no rate.", fmt_count(missing as u64)));
    }
    let out = add_columns(t, cols);
    Ok(vec![("Rates".into(), out), ("Standardised".into(), standardised(p, inp, &pairs, &hidden, per, ctx)?)])
}

/// Directly age-standardised rates for each area: the rate of each age group weighed by the
/// share of that age group in the standard population, as if every area had the same ages.
fn standardised(p: &P, inp: &Inputs, pairs: &[Option<(f64, f64)>], hidden: &[bool], per: f64, ctx: &mut NodeCtx) -> Result<Table> {
    let (Some(area), Some(age)) = (p.get("area").filter(|a| !a.is_empty()), p.get("age").filter(|a| !a.is_empty())) else {
        return Ok(Table::default());
    };
    let t = input(inp, "Input");
    let (ari, agi) = (t.need_field(&area)?, t.need_field(&age)?);
    // The weight of each age group: from Standard, or all areas together.
    let std_table = input(inp, "Standard");
    let mut weight: HashMap<String, f64> = HashMap::new();
    if !std_table.is_empty() {
        let sa = std_table.need_field(&p.need("standard_age", "Choose the attribute with the age group in Standard.")?)?;
        let sp = std_table.need_field(&p.need("standard_population", "Choose the attribute with the population in Standard.")?)?;
        for f in &std_table.features {
            if let Some(v) = num(&f.attrs[sp]) {
                *weight.entry(f.attrs[sa].to_text()).or_default() += v;
            }
        }
    } else {
        for (f, pr) in t.features.iter().zip(pairs) {
            if let Some((_, pop)) = pr {
                *weight.entry(f.attrs[agi].to_text()).or_default() += pop;
            }
        }
        ctx.notes.info("standardised to the ages of all areas together, as nothing is connected to Standard");
    }
    let total: f64 = weight.values().sum();
    if total <= 0.0 {
        return err("The standard population is empty.");
    }
    let mut areas: Vec<String> = Vec::new();
    let mut sums: HashMap<String, (f64, f64, f64, f64)> = HashMap::new();
    let mut unknown_ages: std::collections::BTreeSet<String> = Default::default();
    for (f, pr) in t.features.iter().zip(pairs) {
        let a = f.attrs[ari].to_text();
        if !sums.contains_key(&a) {
            areas.push(a.clone());
        }
        let e = sums.entry(a).or_default();
        let Some((c, pop)) = pr else { continue };
        let g = f.attrs[agi].to_text();
        let Some(w) = weight.get(&g) else {
            unknown_ages.insert(g);
            continue;
        };
        e.0 += c;
        e.1 += pop;
        e.2 += c / pop * w / total;
        e.3 += w / total;
    }
    if !unknown_ages.is_empty() {
        ctx.notes.warn(format!(
            "The age groups {} are not in the standard population and are left out of the standardised rates.",
            unknown_ages.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let hidden_any = hidden.iter().any(|h| *h);
    let mut out = Table {
        fields: vec![
            Field::new(area, FieldType::Text),
            Field::new("cases", FieldType::Real),
            Field::new("population", FieldType::Real),
            Field::new("crude_rate", FieldType::Real),
            Field::new("standardised_rate", FieldType::Real),
        ],
        ..Default::default()
    };
    for a in areas {
        let (c, pop, sr, cover) = sums[&a];
        // A rate that rests on only some of the age groups is told by its cover.
        let std_rate = (cover > 0.0).then(|| sr / cover * per);
        out.features.push(Feature::new(vec![Value::Text(a), Value::Real(c), Value::Real(pop), real((pop > 0.0).then(|| c / pop * per)), real(std_rate)], None));
    }
    if hidden_any {
        ctx.notes
            .info("the standardised rates are made from all cases, also those hidden in Rates; hide small numbers there too if they can tell someone apart");
    }
    Ok(out)
}

// ---------- Linear regression (Regression) ----------

const REGRESSION_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "y",
        label: "To explain",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The number that is to be explained, such as the price",
    },
    ParamSpec {
        key: "x",
        label: "Explained by",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "One or more numbers, such as the area and the year built",
    },
];

/// The regularised incomplete beta function, by its continued fraction.
fn inc_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_gamma = |z: f64| -> f64 {
        // Lanczos.
        let g = [76.18009172947146, -86.50532032941677, 24.01409824083091, -1.231739572450155, 0.1208650973866179e-2, -0.5395239384953e-5];
        let mut y = z;
        let tmp = z + 5.5;
        let tmp = tmp - (z + 0.5) * tmp.ln();
        let mut ser = 1.000000000190015;
        for c in g {
            y += 1.0;
            ser += c / y;
        }
        -tmp + (2.5066282746310005 * ser / z).ln()
    };
    let front = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    let cf = |a: f64, b: f64, x: f64| -> f64 {
        let (mut c, mut d) = (1.0, 1.0 - (a + b) * x / (a + 1.0));
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        d = 1.0 / d;
        let mut h = d;
        for m in 1..300 {
            let m = m as f64;
            let aa = m * (b - m) * x / ((a + 2.0 * m - 1.0) * (a + 2.0 * m));
            d = 1.0 + aa * d;
            d = if d.abs() < 1e-30 { 1e-30 } else { d };
            c = 1.0 + aa / c;
            c = if c.abs() < 1e-30 { 1e-30 } else { c };
            d = 1.0 / d;
            h *= d * c;
            let aa = -(a + m) * (a + b + m) * x / ((a + 2.0 * m) * (a + 2.0 * m + 1.0));
            d = 1.0 + aa * d;
            d = if d.abs() < 1e-30 { 1e-30 } else { d };
            c = 1.0 + aa / c;
            c = if c.abs() < 1e-30 { 1e-30 } else { c };
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-12 {
                break;
            }
        }
        h
    };
    if x < (a + 1.0) / (a + b + 2.0) {
        front * cf(a, b, x) / a
    } else {
        1.0 - front * cf(b, a, 1.0 - x) / b
    }
}

/// The two-sided p-value of t with so many degrees of freedom.
fn t_p(t: f64, df: f64) -> f64 {
    inc_beta(df / 2.0, 0.5, df / (df + t * t))
}

/// The inverse of a square matrix by Gauss-Jordan, or None when it has none.
fn invert(mut m: Vec<Vec<f64>>) -> Option<Vec<Vec<f64>>> {
    let n = m.len();
    let mut inv: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    for col in 0..n {
        let piv = (col..n).max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))?;
        if m[piv][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, piv);
        inv.swap(col, piv);
        let d = m[col][col];
        for j in 0..n {
            m[col][j] /= d;
            inv[col][j] /= d;
        }
        for r in 0..n {
            if r != col {
                let f = m[r][col];
                if f != 0.0 {
                    for j in 0..n {
                        m[r][j] -= f * m[col][j];
                        inv[r][j] -= f * inv[col][j];
                    }
                }
            }
        }
    }
    Some(inv)
}

fn run_regression(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let yname = p.need("y", "Choose the number to explain.")?;
    let yi = t.need_field(&yname)?;
    let xs = p.list("x");
    if xs.is_empty() {
        return err("Choose at least one number to explain it by.");
    }
    let xi: Vec<usize> = xs.iter().map(|n| t.need_field(n)).collect::<Result<_>>()?;
    // The rows with every value; the others are left out of the model.
    let rows: Vec<(usize, f64, Vec<f64>)> = t
        .features
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let y = num(&f.attrs[yi])?;
            let x: Option<Vec<f64>> = xi.iter().map(|k| num(&f.attrs[*k])).collect();
            Some((i, y, x?))
        })
        .collect();
    let k = xs.len() + 1;
    let n = rows.len();
    if n <= k {
        return err(format!("{} features have every value, which is too few for {} explaining numbers: there must be more than {}.", n, xs.len(), k));
    }
    if n < t.len() {
        ctx.notes.warn(format!("{} features without every value are left out of the model.", fmt_count((t.len() - n) as u64)));
    }
    // X'X and X'y, with a 1 first for the intercept.
    let mut xtx = vec![vec![0.0; k]; k];
    let mut xty = vec![0.0; k];
    for (_, y, x) in &rows {
        let row: Vec<f64> = std::iter::once(1.0).chain(x.iter().copied()).collect();
        for a in 0..k {
            xty[a] += row[a] * y;
            for b in 0..k {
                xtx[a][b] += row[a] * row[b];
            }
        }
    }
    let Some(inv) = invert(xtx) else {
        return err("The explaining numbers depend on each other, or one does not vary, so the model has no single answer. Leave one of them out.");
    };
    let beta: Vec<f64> = (0..k).map(|a| (0..k).map(|b| inv[a][b] * xty[b]).sum()).collect();
    let predict = |x: &[f64]| beta[0] + x.iter().zip(&beta[1..]).map(|(v, b)| v * b).sum::<f64>();
    let mean_y = rows.iter().map(|r| r.1).sum::<f64>() / n as f64;
    let (mut ss_res, mut ss_tot) = (0.0, 0.0);
    for (_, y, x) in &rows {
        ss_res += (y - predict(x)).powi(2);
        ss_tot += (y - mean_y).powi(2);
    }
    let df = (n - k) as f64;
    let sigma2 = ss_res / df;
    let r2 = if ss_tot > 0.0 { 1.0 - ss_res / ss_tot } else { 1.0 };
    let adj = 1.0 - (1.0 - r2) * (n as f64 - 1.0) / df;
    let sigma = sigma2.sqrt();
    // Every feature with what the model gives, and how far it is off.
    let mut predicted = vec![Value::Null; t.len()];
    let mut residual = vec![Value::Null; t.len()];
    let mut std_res = vec![Value::Null; t.len()];
    for (i, y, x) in &rows {
        let yhat = predict(x);
        predicted[*i] = Value::Real(yhat);
        residual[*i] = Value::Real(y - yhat);
        std_res[*i] = real((sigma > 0.0).then(|| (y - yhat) / sigma));
    }
    let features = add_columns(
        t,
        vec![
            (format!("{}_predicted", yname), FieldType::Real, predicted),
            (format!("{}_residual", yname), FieldType::Real, residual),
            (format!("{}_std_residual", yname), FieldType::Real, std_res),
        ],
    );
    let mut summary = Table {
        fields: vec![
            Field::new("term", FieldType::Text),
            Field::new("coefficient", FieldType::Real),
            Field::new("std_error", FieldType::Real),
            Field::new("t", FieldType::Real),
            Field::new("p", FieldType::Real),
            Field::new("r2", FieldType::Real),
            Field::new("adj_r2", FieldType::Real),
            Field::new("n", FieldType::Int),
        ],
        ..Default::default()
    };
    let names: Vec<String> = std::iter::once("(intercept)".to_string()).chain(xs.iter().cloned()).collect();
    for (a, name) in names.iter().enumerate() {
        let se = (sigma2 * inv[a][a]).max(0.0).sqrt();
        let tv = if se > 0.0 { beta[a] / se } else { f64::NAN };
        summary.features.push(Feature::new(
            vec![
                Value::Text(name.clone()),
                Value::Real(beta[a]),
                real(Some(se)),
                real(Some(tv)),
                real(tv.is_finite().then(|| t_p(tv, df))),
                Value::Real(r2),
                Value::Real(adj),
                Value::Int(n as i64),
            ],
            None,
        ));
    }
    ctx.notes.info(format!(
        "{} = {}{} · R² {} · {} features",
        yname,
        fmt_real((beta[0] * 1e4).round() / 1e4),
        xs.iter()
            .zip(&beta[1..])
            .map(|(x, b)| format!(" {} {} × {}", if *b < 0.0 { "−" } else { "+" }, fmt_real((b.abs() * 1e4).round() / 1e4), x))
            .collect::<String>(),
        fmt_real((r2 * 1000.0).round() / 1000.0),
        fmt_count(n as u64)
    ));
    Ok(vec![("Features".into(), features), ("Summary".into(), summary)])
}

pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "Classifier",
        title: "Classify values",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "Puts the features in classes by a number, for a map with a colour for each class: natural breaks (Jenks), where the classes differ as much as they can and the values within each as little; quantiles, with as many features in each; equal intervals; standard deviations from the mean; or breaks of your own. Each feature gets its class, 1 for the lowest, and its name, such as 10–50, for the legend; Classes lists them with how many are in each.",
        inputs: &["Input"],
        outputs: &["Classified", "Classes"],
        ports: None,
        params: CLASS_PARAMS,
        run: run_classify,
        summary: |p| {
            let method = CLASS_METHODS.iter().find(|m| Some(m.0) == p.get("method").as_deref()).map_or("natural breaks", |m| m.1);
            format!("{} · {} · {} classes", p.get("attribute").unwrap_or_default(), method.split(':').next().unwrap_or(method), p.get("classes").unwrap_or_else(|| "5".into()))
        },
    },
    NodeType {
        name: "RunningStatistics",
        title: "Shares, ranks and running totals",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "Gives each feature numbers about its place among the others: its share of the total in percent, its rank (1 for the largest), the running total and the running share in the order the features come, and a moving mean of the last few values, for all together or for each group, such as each municipality.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: RUNNING_PARAMS,
        run: run_running,
        summary: |p| format!("{} · {}", p.get("attribute").unwrap_or_default(), p.list("what").join(", ")),
    },
    NodeType {
        name: "ChangeOverTime",
        title: "Change over time",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "For values over time, such as the inhabitants of each municipality year by year: the change since the time before, in numbers and in percent, an index where the base time is 100, and the mean yearly growth since the base time in percent. The time is a year or a date; each series, such as each municipality, is counted by itself.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: CHANGE_PARAMS,
        run: run_change,
        summary: |p| format!("{} over {}", p.get("attribute").unwrap_or_default(), p.get("time").unwrap_or_default()),
    },
    NodeType {
        name: "RateCalculator",
        title: "Rates",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "Cases per inhabitant, such as patients per 1 000 inhabitants in each area. Rates from few cases can be hidden, so that no one can be told apart. A smoothed rate draws the rates of small populations towards the mean (empirical Bayes), where chance makes them leap. With a row for each area and age group, Standardised gives the rate of each area as if all had the same ages, directly standardised to the population on Standard, such as that of Sweden, or of all areas together.",
        inputs: &["Input", "Standard"],
        outputs: &["Rates", "Standardised"],
        ports: None,
        params: RATE_PARAMS,
        run: run_rates,
        summary: |p| format!("{} per {} {}", p.get("count").unwrap_or_default(), p.get("per").unwrap_or_else(|| "1000".into()), p.get("population").unwrap_or_default()),
    },
    NodeType {
        name: "Regression",
        title: "Linear regression",
        hidden: false,
        group: Group::Transformer,
        category: "Statistics",
        desc: "How a number depends on one or more others, by least squares: such as the price of a house by its area and age. Summary gives each coefficient with its standard error, t and p, and how much the model explains (R², and adjusted R²). Each feature gets what the model gives for it and how far it is off (the residual, also in standard deviations), to draw on a map: where the model misses tells what it leaves out.",
        inputs: &["Input"],
        outputs: &["Features", "Summary"],
        ports: None,
        params: REGRESSION_PARAMS,
        run: run_regression,
        summary: |p| format!("{} by {}", p.get("y").unwrap_or_default(), p.list("x").join(", ")),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn table(fields: &[(&str, FieldType)], rows: &[Vec<Value>]) -> Table {
        let mut t = Table { fields: fields.iter().map(|(n, ty)| Field::new(*n, *ty)).collect(), ..Default::default() };
        for r in rows {
            t.features.push(Feature::new(r.clone(), None));
        }
        t
    }

    fn col(t: &Table, name: &str) -> Vec<Value> {
        let i = t.field_index(name).unwrap_or_else(|| panic!("no {} in {:?}", name, t.field_names()));
        t.features.iter().map(|f| f.attrs[i].clone()).collect()
    }

    fn f(v: &Value) -> f64 {
        v.as_f64().unwrap()
    }

    #[test]
    fn values_are_put_in_natural_and_other_classes() {
        let vals = [1.0, 2.0, 3.0, 10.0, 11.0, 12.0, 30.0, 31.0, 32.0];
        let t = table(&[("v", FieldType::Real)], &vals.iter().map(|v| vec![Value::Real(*v)]).collect::<Vec<_>>());
        let (out, notes) = run_for_test("Classifier", &[("attribute", "v"), ("classes", "3")], vec![("Input", t.clone())]).unwrap();
        assert_eq!(col(&out[0].1, "class").iter().map(f).collect::<Vec<_>>(), [1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0]);
        assert_eq!(col(&out[0].1, "class_label")[0], Value::Text("1 – 3".into()));
        assert_eq!(out[1].1.len(), 3);
        assert!(notes.info.iter().any(|n| n.starts_with("3 classes")), "{:?}", notes.info);
        // Quantiles put as many in each.
        let (out, _) = run_for_test("Classifier", &[("attribute", "v"), ("classes", "3"), ("method", "quantile")], vec![("Input", t.clone())]).unwrap();
        assert_eq!(col(&out[1].1, "count").iter().map(f).collect::<Vec<_>>(), [3.0, 3.0, 3.0]);
        // Equal intervals of a third of 1 to 32 each.
        let (out, _) = run_for_test("Classifier", &[("attribute", "v"), ("classes", "3"), ("method", "equal")], vec![("Input", t.clone())]).unwrap();
        assert_eq!(col(&out[0].1, "class").iter().map(f).collect::<Vec<_>>(), [1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 3.0, 3.0, 3.0]);
        let (out, _) = run_for_test("Classifier", &[("attribute", "v"), ("method", "manual"), ("breaks", "5, 20")], vec![("Input", t)]).unwrap();
        assert_eq!(col(&out[1].1, "count").iter().map(f).collect::<Vec<_>>(), [3.0, 3.0, 3.0]);
    }

    #[test]
    fn shares_ranks_and_running_totals() {
        let t = table(&[("v", FieldType::Real)], &[vec![Value::Real(10.0)], vec![Value::Real(30.0)], vec![Value::Real(30.0)], vec![Value::Real(30.0)]]);
        let params = serde_json::json!({"attribute": "v", "what": ["share", "rank", "cumulative", "moving"], "window": "2"});
        let (out, _) = run_for_test_with("RunningStatistics", params.as_object().unwrap().clone().into_iter().collect(), vec![("Input", t)]).unwrap();
        let o = &out[0].1;
        assert_eq!(col(o, "v_share").iter().map(f).collect::<Vec<_>>(), [10.0, 30.0, 30.0, 30.0]);
        assert_eq!(col(o, "v_rank").iter().map(f).collect::<Vec<_>>(), [4.0, 1.0, 1.0, 1.0]);
        assert_eq!(col(o, "v_cumulative").iter().map(f).collect::<Vec<_>>(), [10.0, 40.0, 70.0, 100.0]);
        assert_eq!(col(o, "v_moving")[0], Value::Null);
        assert_eq!(f(&col(o, "v_moving")[1]), 20.0);
    }

    #[test]
    fn change_index_and_growth_over_years() {
        let rows: Vec<Vec<Value>> = [("A", 2020, 100.0), ("A", 2021, 110.0), ("A", 2022, 121.0), ("B", 2021, 50.0), ("B", 2020, 40.0)]
            .iter()
            .map(|(s, y, v)| vec![Value::Text(s.to_string()), Value::Int(*y), Value::Real(*v)])
            .collect();
        let t = table(&[("place", FieldType::Text), ("year", FieldType::Int), ("people", FieldType::Real)], &rows);
        let params = serde_json::json!({"attribute": "people", "time": "year", "series": ["place"], "what": ["change", "percent", "index", "growth"]});
        let (out, _) = run_for_test_with("ChangeOverTime", params.as_object().unwrap().clone().into_iter().collect(), vec![("Input", t)]).unwrap();
        let o = &out[0].1;
        assert_eq!(col(o, "people_change")[1], Value::Real(10.0));
        assert!((f(&col(o, "people_change_pct")[2]) - 10.0).abs() < 1e-9);
        assert!((f(&col(o, "people_index")[2]) - 121.0).abs() < 1e-9);
        assert!((f(&col(o, "people_growth_pct")[2]) - 10.0).abs() < 1e-9);
        // B comes out of order, and is put in order of its years.
        assert_eq!(col(o, "people_change")[3], Value::Real(10.0));
        assert_eq!(col(o, "people_change")[4], Value::Null);
    }

    #[test]
    fn rates_are_hidden_smoothed_and_standardised() {
        let rows: Vec<Vec<Value>> =
            [("North", "young", 2.0, 1000.0), ("North", "old", 30.0, 1000.0), ("South", "young", 10.0, 4000.0), ("South", "old", 8.0, 250.0)]
                .iter()
                .map(|(a, g, c, p)| vec![Value::Text(a.to_string()), Value::Text(g.to_string()), Value::Real(*c), Value::Real(*p)])
                .collect();
        let t = table(&[("area", FieldType::Text), ("age", FieldType::Text), ("cases", FieldType::Real), ("people", FieldType::Real)], &rows);
        let std = table(
            &[("age", FieldType::Text), ("pop", FieldType::Real)],
            &[vec![Value::Text("young".into()), Value::Real(1.0)], vec![Value::Text("old".into()), Value::Real(1.0)]],
        );
        let p = [
            ("count", "cases"),
            ("population", "people"),
            ("suppress", "5"),
            ("smooth", "yes"),
            ("area", "area"),
            ("age", "age"),
            ("standard_age", "age"),
            ("standard_population", "pop"),
        ];
        let (out, notes) = run_for_test("RateCalculator", &p, vec![("Input", t), ("Standard", std)]).unwrap();
        let r = &out[0].1;
        assert_eq!(col(r, "cases_rate")[0], Value::Null, "2 cases are fewer than 5");
        assert_eq!(col(r, "suppressed")[0], Value::Int(1));
        assert_eq!(col(r, "cases")[0], Value::Null);
        assert_eq!(f(&col(r, "cases_rate")[1]), 30.0);
        assert!(notes.info.iter().any(|n| n.starts_with("1 rates hidden")), "{:?}", notes.info);
        assert!(col(r, "cases_rate_smoothed")[1].as_f64().is_some());
        // As if both had equal numbers young and old: North (2 + 30) / 2, South (2.5 + 32) / 2.
        let s = &out[1].1;
        assert_eq!(s.len(), 2);
        assert!((f(&col(s, "standardised_rate")[0]) - 16.0).abs() < 1e-9);
        assert!((f(&col(s, "standardised_rate")[1]) - 17.25).abs() < 1e-9);
    }

    #[test]
    fn a_line_is_fitted_with_its_coefficients() {
        // y = 3 + 2 x1 - x2, with a little noise.
        let rows: Vec<Vec<Value>> = (0..20)
            .map(|i| {
                let (x1, x2) = (i as f64, ((i * 7) % 5) as f64);
                let noise = if i % 2 == 0 { 0.1 } else { -0.1 };
                vec![Value::Real(3.0 + 2.0 * x1 - x2 + noise), Value::Real(x1), Value::Real(x2)]
            })
            .collect();
        let t = table(&[("y", FieldType::Real), ("x1", FieldType::Real), ("x2", FieldType::Real)], &rows);
        let params = serde_json::json!({"y": "y", "x": ["x1", "x2"]});
        let (out, notes) = run_for_test_with("Regression", params.as_object().unwrap().clone().into_iter().collect(), vec![("Input", t)]).unwrap();
        let s = &out[1].1;
        let coef = col(s, "coefficient").iter().map(f).collect::<Vec<_>>();
        assert!((coef[0] - 3.0).abs() < 0.2 && (coef[1] - 2.0).abs() < 0.02 && (coef[2] + 1.0).abs() < 0.05, "{:?}", coef);
        assert!(f(&col(s, "r2")[0]) > 0.99);
        assert!(f(&col(s, "p")[1]) < 1e-6);
        assert!(col(&out[0].1, "y_residual")[0].as_f64().unwrap().abs() < 0.3);
        assert!(notes.info.iter().any(|n| n.contains("R²")), "{:?}", notes.info);
        // The p-value of t = 2 with 10 degrees of freedom is about 0.073.
        assert!((t_p(2.0, 10.0) - 0.0734).abs() < 0.001, "{}", t_p(2.0, 10.0));
    }

    #[test]
    fn natural_breaks_split_where_the_gaps_are() {
        assert_eq!(jenks(&[1.0, 2.0, 3.0, 50.0, 51.0, 100.0], 3), [3.0, 51.0]);
    }
}
