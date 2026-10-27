//! Tools for the attributes as a table: Keep attributes, Manage attributes, Join by
//! attribute, Calculate statistics and Sort.

use super::*;

// ---------- Keep attributes (AttributeKeeper) ----------

pub(super) const KEEPER_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "keep",
    label: "Attributes to keep",
    kind: ParamKind::Attributes("Input"),
    default: "",
    hint: "All others are removed. The order stays as it was",
}];

pub(super) fn run_keeper(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let keep = p.list("keep");
    if keep.is_empty() {
        return err("Choose the attributes to keep.");
    }
    let missing: Vec<&str> = keep.iter().map(|k| k.as_str()).filter(|k| table.field_index(k).is_none()).collect();
    if !missing.is_empty() && !table.fields.is_empty() {
        ctx.notes.warn(format!("Not in the stream, so not kept: {}.", missing.join(", ")));
    }
    let idx: Vec<usize> = (0..table.fields.len()).filter(|i| keep.contains(&table.fields[*i].name)).collect();
    let removed = table.fields.len() - idx.len();
    if removed > 0 {
        ctx.notes.info(format!("{} attributes removed", fmt_count(removed as u64)));
    }
    let out = Table {
        mesh: None,
        fields: idx.iter().map(|i| table.fields[*i].clone()).collect(),
        crs: table.crs,
        features: table
            .features
            .iter()
            .map(|f| Feature { z: f.z.clone(), attrs: idx.iter().map(|i| f.attrs[*i].clone()).collect(), geom: f.geom.clone() })
            .collect(),
        raster: table.raster,
        image: None,
        grid: None,
    };
    Ok(vec![("Output".into(), out)])
}

// ---------- Manage attributes (AttributeManager) ----------

pub(super) const MANAGER_OTHERS: &[(&str, &str)] = &[("keep", "Keep them"), ("remove", "Remove them")];

pub(super) const MANAGER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attributes",
        label: "Attributes",
        kind: ParamKind::AttributeTable("Input"),
        default: "",
        hint: "Keep, rename or remove each one. The arrows change the order",
    },
    ParamSpec {
        key: "others",
        label: "Attributes that are not listed",
        kind: ParamKind::Choice(MANAGER_OTHERS),
        default: "keep",
        hint: "Those that come in later, when what comes before this step is changed",
    },
    ParamSpec {
        key: "create",
        label: "New attributes",
        kind: ParamKind::NewAttributes("Input"),
        default: "",
        hint: "A value is an expression: 'text' in single quotes, a number, or for example population / 1000",
    },
];

/// What Manage attributes does with an attribute.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrAction {
    Keep,
    Rename(String),
    Remove,
}

/// The rows of the table in Manage attributes, in their order: (attribute, what to do).
pub fn attribute_rules(params: &Params) -> Vec<(String, AttrAction)> {
    let Some(serde_json::Value::Array(rows)) = params.get("attributes") else { return Vec::new() };
    rows.iter()
        .filter_map(|r| {
            let name = r.get("name")?.as_str()?.to_string();
            let action = match r.get("action").and_then(|a| a.as_str()).unwrap_or("keep") {
                "remove" => AttrAction::Remove,
                "rename" => AttrAction::Rename(r.get("to").and_then(|t| t.as_str()).unwrap_or("").trim().to_string()),
                _ => AttrAction::Keep,
            };
            Some((name, action))
        })
        .collect()
}

/// The rows of the table as they are stored.
pub fn rules_json(rules: &[(String, AttrAction)]) -> serde_json::Value {
    serde_json::Value::Array(
        rules
            .iter()
            .map(|(name, action)| match action {
                AttrAction::Keep => serde_json::json!({ "name": name }),
                AttrAction::Remove => serde_json::json!({ "name": name, "action": "remove" }),
                AttrAction::Rename(to) => serde_json::json!({ "name": name, "action": "rename", "to": to }),
            })
            .collect(),
    )
}

/// The new attributes of Manage attributes: (name, expression).
pub fn new_attributes(params: &Params) -> Vec<(String, String)> {
    let Some(serde_json::Value::Array(rows)) = params.get("create") else { return Vec::new() };
    rows.iter().filter_map(|r| Some((r.get("name")?.as_str()?.trim().to_string(), r.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string()))).collect()
}

/// A type as it is stored in the rows of Manage attributes.
pub fn type_of_name(t: &str) -> Option<FieldType> {
    match t {
        "integer" => Some(FieldType::Int),
        "decimal" => Some(FieldType::Real),
        "text" => Some(FieldType::Text),
        _ => None,
    }
}

/// The type each row of Manage attributes asks for, by the name of the row: the attributes
/// that are listed, and then the new ones.
pub fn attribute_types(params: &Params, key: &str) -> Vec<(String, FieldType)> {
    let Some(serde_json::Value::Array(rows)) = params.get(key) else { return Vec::new() };
    rows.iter()
        .filter_map(|r| {
            let name = r.get("name")?.as_str()?.trim().to_string();
            let ty = type_of_name(r.get("type")?.as_str()?)?;
            Some((name, ty))
        })
        .collect()
}

/// Whether a Manage attributes node still has the settings of the versions before 0.53:
/// lists to keep and to remove, and pairs to rename.
pub fn manager_is_old(params: &Params) -> bool {
    !params.contains_key("attributes") && !params.contains_key("create") && ["keep", "remove", "rename"].iter().any(|k| params.contains_key(*k))
}

/// The settings of before 0.53 as rows of the table, given the attributes that come in.
pub fn manager_from_old(params: &Params, fields: &[String]) -> (Vec<(String, AttrAction)>, bool) {
    let p = P::new(params, &[]);
    let (keep, remove, rename) = (p.list("keep"), p.list("remove"), p.pairs("rename"));
    let action = |name: &str| {
        if remove.iter().any(|r| r == name) || (!keep.is_empty() && !keep.iter().any(|k| k == name)) {
            AttrAction::Remove
        } else if let Some((_, to)) = rename.iter().find(|r| r.0 == name) {
            AttrAction::Rename(to.clone())
        } else {
            AttrAction::Keep
        }
    };
    // What was kept came in the order of the list, and the rest after it.
    let mut names: Vec<String> = keep.iter().filter(|k| fields.contains(k)).cloned().collect();
    let rest: Vec<String> = fields.iter().filter(|f| !names.contains(f)).cloned().collect();
    names.extend(rest);
    (names.iter().map(|n| (n.clone(), action(n))).collect(), !keep.is_empty())
}

pub(super) fn run_manager(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    if manager_is_old(p.values) {
        return run_manager_old(p, inp, ctx);
    }
    let table = input(inp, "Input");
    let rules = attribute_rules(p.values);
    let others_go = p.get("others").as_deref() == Some("remove");
    let missing: Vec<&str> = rules.iter().map(|r| r.0.as_str()).filter(|n| table.field_index(n).is_none()).collect();
    if !missing.is_empty() && !table.fields.is_empty() {
        ctx.notes.warn(format!("Attributes that are not in the stream were skipped: {}.", missing.join(", ")));
    }
    // The listed attributes in the order of the list, then the others in their own order.
    let mut order: Vec<(usize, AttrAction)> = rules.iter().filter_map(|(n, a)| table.field_index(n).map(|i| (i, a.clone()))).collect();
    for i in 0..table.fields.len() {
        if !order.iter().any(|(k, _)| *k == i) {
            order.push((i, if others_go { AttrAction::Remove } else { AttrAction::Keep }));
        }
    }
    let wanted_types = attribute_types(p.values, "attributes");
    let mut picked: Vec<(usize, String)> = Vec::new();
    let mut retype: Vec<(String, FieldRule)> = Vec::new();
    for (i, action) in order {
        let name = &table.fields[i].name;
        let new = match action {
            AttrAction::Remove => continue,
            AttrAction::Keep => name.clone(),
            AttrAction::Rename(to) if to.is_empty() => {
                ctx.notes.warn(format!("{} is to be renamed but has no new name, so it keeps its name.", name));
                name.clone()
            }
            AttrAction::Rename(to) => to,
        };
        if picked.iter().any(|x| x.1 == new) {
            return err(format!("Two attributes would be named \"{}\". Rename or remove one of them.", new));
        }
        if let Some((_, ty)) = wanted_types.iter().find(|(n, _)| n == name).filter(|(_, ty)| *ty != table.fields[i].ty) {
            retype.push((new.clone(), FieldRule { ty: Some(*ty), length: None, decimals: None }));
        }
        picked.push((i, new));
    }
    let mut out = Table {
        mesh: None,
        fields: picked.iter().map(|(i, n)| Field::new(n.clone(), table.fields[*i].ty)).collect(),
        crs: table.crs,
        features: table
            .features
            .iter()
            .map(|f| Feature { z: f.z.clone(), attrs: picked.iter().map(|(i, _)| f.attrs[*i].clone()).collect(), geom: f.geom.clone() })
            .collect(),
        raster: table.raster,
        image: None,
        grid: None,
    };
    // A type that was chosen: the values are made into it. What cannot be is left empty.
    if !retype.is_empty() {
        out = apply_field_rules(&out, &retype, &mut ctx.notes).0;
    }
    // New attributes are calculated from what comes in, by the names it has there. One
    // without a value is empty, of the type chosen, to be filled in later.
    let geographic = ctx.geographic(table.crs);
    let new_types = attribute_types(p.values, "create");
    for (name, value) in new_attributes(p.values) {
        if name.is_empty() {
            continue;
        }
        let ty = new_types.iter().find(|t| t.0 == name).map(|t| t.1);
        if value.trim().is_empty() {
            let ty = ty.unwrap_or(FieldType::Text);
            out = with_column(&out, &name, vec![Value::Null; out.len()]);
            if let Some(i) = out.field_index(&name) {
                out.fields[i].ty = ty;
            }
            continue;
        }
        let e = expr::compile(&value, &table.field_names()).map_err(|e| GbError(format!("The value of {}: {}", name, e.0)))?;
        let values: Vec<Value> = table
            .features
            .iter()
            .enumerate()
            .map(|(i, f)| e.eval(&Row { feature: f, index: i, geographic }).map(Value::stored).map_err(|e| at_row(e, i)))
            .collect::<Result<_>>()?;
        if out.field_index(&name).is_some() {
            ctx.notes.info(format!("the new {} replaced the attribute with that name", name));
        }
        out = with_column(&out, &name, values);
        if let Some(ty) = ty {
            out = apply_field_rules(&out, &[(name.clone(), FieldRule { ty: Some(ty), length: None, decimals: None })], &mut ctx.notes).0;
        }
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) fn run_manager_old(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let (keep, remove, rename) = (p.list("keep"), p.list("remove"), p.pairs("rename"));
    let missing: Vec<&String> = keep.iter().chain(remove.iter()).chain(rename.iter().map(|r| &r.0)).filter(|n| table.field_index(n).is_none()).collect();
    if !missing.is_empty() && !table.fields.is_empty() {
        ctx.notes.warn(format!("Attributes that are not in the stream were skipped: {}.", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let mut picked: Vec<(usize, String)> = Vec::new();
    for (i, f) in table.fields.iter().enumerate() {
        if (!keep.is_empty() && !keep.contains(&f.name)) || remove.contains(&f.name) {
            continue;
        }
        let new = rename.iter().find(|r| r.0 == f.name).map(|r| r.1.clone()).unwrap_or_else(|| f.name.clone());
        if picked.iter().any(|x| x.1 == new) {
            return err(format!("Two attributes would be named \"{}\". Rename or remove one of them.", new));
        }
        picked.push((i, new));
    }
    // The order follows the Keep list when it is filled in.
    if !keep.is_empty() {
        picked.sort_by_key(|(i, _)| keep.iter().position(|k| *k == table.fields[*i].name).unwrap_or(usize::MAX));
    }
    let out = Table {
        mesh: None,
        fields: picked.iter().map(|(i, n)| Field::new(n.clone(), table.fields[*i].ty)).collect(),
        crs: table.crs,
        features: table
            .features
            .iter()
            .map(|f| Feature { z: f.z.clone(), attrs: picked.iter().map(|(i, _)| f.attrs[*i].clone()).collect(), geom: f.geom.clone() })
            .collect(),
        raster: table.raster,
        image: None,
        grid: None,
    };
    Ok(vec![("Output".into(), out)])
}

// ---------- Join by attribute (FeatureMerger) ----------

pub(super) const JOIN_KINDS: &[(&str, &str)] = &[
    ("inner", "Inner: only those that match, in Merged"),
    ("left", "Left: all of Requestor, those without a match with empty attributes"),
    ("full", "Full: all of both, Supplier without a match too"),
];
pub(super) const JOIN_MATCHES: &[(&str, &str)] =
    &[("first", "The first match: one feature for each in Requestor"), ("all", "Every match: a feature for each pair, one to many")];

pub(super) const MERGER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "requestor_key", label: "Requestor key", kind: ParamKind::Attributes("Requestor"), default: "", hint: "One or more attributes" },
    ParamSpec { key: "supplier_key", label: "Supplier key", kind: ParamKind::Attributes("Supplier"), default: "", hint: "As many as in Requestor" },
    ParamSpec {
        key: "join",
        label: "Join",
        kind: ParamKind::Choice(JOIN_KINDS),
        default: "inner",
        hint: "As in SQL. Those left out still come out on their own ports",
    },
    ParamSpec { key: "matches", label: "When several match", kind: ParamKind::Choice(JOIN_MATCHES), default: "first", hint: "" },
    ParamSpec {
        key: "prefix",
        label: "Prefix for Supplier's attributes",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as s_. With a prefix, attributes of Supplier with the same name as one of Requestor are taken too, under the new name",
    },
];

pub(super) fn key_of(f: &Feature, idx: &[usize]) -> Option<Vec<String>> {
    idx.iter().map(|i| f.attrs[*i].key()).collect()
}

pub(super) fn run_merger(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (req, sup) = (input(inp, "Requestor"), input(inp, "Supplier"));
    let (rk, sk) = (p.list("requestor_key"), p.list("supplier_key"));
    if rk.is_empty() || sk.is_empty() {
        return err("Choose key attributes for both Requestor and Supplier.");
    }
    if rk.len() != sk.len() {
        return err(format!("Requestor has {} key attributes and Supplier {}. They must be the same number.", rk.len(), sk.len()));
    }
    let empty = |t: &Table| t.fields.is_empty() && t.is_empty();
    if empty(req) || empty(sup) {
        return Ok(vec![("Merged".into(), req.like()), ("UnmergedRequestor".into(), req.clone()), ("UnusedSupplier".into(), sup.clone())]);
    }
    let ri: Vec<usize> = rk.iter().map(|n| req.need_field(n)).collect::<Result<_>>()?;
    let si: Vec<usize> = sk.iter().map(|n| sup.need_field(n)).collect::<Result<_>>()?;
    let kind = p.get("join").unwrap_or_else(|| "inner".into());
    let all = p.get("matches").as_deref() == Some("all");
    let prefix = p.get("prefix").unwrap_or_default();

    let mut index: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
    for (i, f) in sup.features.iter().enumerate() {
        if let Some(k) = key_of(f, &si) {
            index.entry(k).or_default().push(i);
        }
    }
    let dup: usize = index.values().map(|v| v.len() - 1).sum();
    if dup > 0 && !all {
        ctx.notes.warn(format!(
            "{} features in Supplier have a key that already exists. The first one with each key is used; choose Every match to get them all.",
            fmt_count(dup as u64)
        ));
    }
    // Requestor attributes win when names clash, so only new names are taken from Supplier,
    // unless they are given a prefix.
    let name_of = |n: &str| format!("{}{}", prefix, n);
    let extra: Vec<usize> = (0..sup.fields.len()).filter(|i| req.field_index(&name_of(&sup.fields[*i].name)).is_none()).collect();
    let mut merged = req.like();
    merged.fields.extend(extra.iter().map(|i| crate::model::Field { name: name_of(&sup.fields[*i].name), ..sup.fields[*i].clone() }));
    let mut unmerged = req.like();
    let mut used = vec![false; sup.len()];
    let mut matched = vec![false; sup.len()];
    let width = merged.fields.len();
    for f in &req.features {
        let hits: &[usize] = key_of(f, &ri).and_then(|k| index.get(&k)).map(|v| v.as_slice()).unwrap_or(&[]);
        // A feature of Supplier whose key matches is not left over in a full join, also when
        // only the first is taken; the others still come out on UnusedSupplier.
        for &s in hits {
            matched[s] = true;
        }
        let hits = if all { hits } else { &hits[..hits.len().min(1)] };
        for &s in hits {
            used[s] = true;
            let mut attrs = f.attrs.clone();
            attrs.extend(extra.iter().map(|i| sup.features[s].attrs[*i].clone()));
            merged.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
        }
        if hits.is_empty() {
            unmerged.features.push(f.clone());
            if kind != "inner" {
                let mut attrs = f.attrs.clone();
                attrs.resize(width, Value::Null);
                merged.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
            }
        }
    }
    let mut unused = sup.like();
    unused.features = sup.features.iter().zip(&used).filter(|(_, u)| !**u).map(|(f, _)| f.clone()).collect();
    if kind == "full" {
        // Supplier without a match: its own attributes where they go, and its geometry when
        // Requestor has none.
        for f in sup.features.iter().zip(&matched).filter(|(_, m)| !**m).map(|(f, _)| f) {
            let mut attrs = vec![Value::Null; width];
            for (j, i) in extra.iter().enumerate() {
                attrs[req.fields.len() + j] = f.attrs[*i].clone();
            }
            for (k, &r) in ri.iter().enumerate() {
                attrs[r] = f.attrs[si[k]].clone();
            }
            let geom = if req.has_geometry() { None } else { f.geom.clone() };
            merged.features.push(Feature { z: None, attrs, geom });
        }
        if merged.crs.is_none() {
            merged.crs = sup.crs;
        }
    }
    Ok(vec![("Merged".into(), merged), ("UnmergedRequestor".into(), unmerged), ("UnusedSupplier".into(), unused)])
}

// ---------- Calculate statistics (StatisticsCalculator) ----------

pub(super) const STATS_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attributes",
        label: "Attributes to analyse",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "Numbers. Empty only counts features",
    },
    ParamSpec { key: "group_by", label: "Group by", kind: ParamKind::Attributes("Input"), default: "", hint: "Empty gives one summary for the whole stream" },
    ParamSpec {
        key: "percentiles",
        label: "Percentiles",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as 10, 90 for the values a tenth lie below and a tenth above, as _p10 and _p90. Empty gives none",
    },
];

/// A percentile of sorted numbers, between the two nearest as spreadsheets count it.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let at = (sorted.len() - 1) as f64 * p / 100.0;
    let (lo, hi) = (at.floor() as usize, at.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (at - lo as f64)
}

pub(super) const STATS: &[&str] = &["sum", "min", "max", "mean", "median"];

pub(super) fn run_stats(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let (attrs, by) = (p.list("attributes"), p.list("group_by"));
    if table.fields.is_empty() && table.is_empty() {
        return Ok(vec![("Summary".into(), Table::default()), ("Complete".into(), table.clone())]);
    }
    let ai: Vec<usize> = attrs.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;
    let gi: Vec<usize> = by.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;

    let percentiles: Vec<f64> = p
        .get("percentiles")
        .unwrap_or_default()
        .split([',', ';', ' '])
        .filter(|t| !t.trim().is_empty())
        .map(|t| match crate::model::parse_number(t.trim()) {
            Some(v) if (0.0..=100.0).contains(&v) => Ok(v),
            _ => err(format!("A percentile is a number from 0 to 100, not \"{}\".", t.trim())),
        })
        .collect::<Result<_>>()?;
    let mut stat_fields: Vec<Field> = vec![Field::new("_count", FieldType::Int)];
    for a in &attrs {
        stat_fields.extend(STATS.iter().map(|s| Field::new(format!("{}_{}", a, s), FieldType::Real)));
        stat_fields.extend(percentiles.iter().map(|q| Field::new(format!("{}_p{}", a, crate::model::fmt_real(*q).replace(['.', ','], "_")), FieldType::Real)));
    }
    if let Some(clash) = stat_fields.iter().find(|f| by.contains(&f.name)) {
        return err(format!("The result attribute \"{}\" has the same name as a group-by attribute. Rename it first.", clash.name));
    }

    let mut summary = Table {
        mesh: None,
        fields: gi.iter().map(|i| table.fields[*i].clone()).chain(stat_fields.iter().cloned()).collect(),
        crs: None,
        features: Vec::new(),
        raster: false,
        image: None,
        grid: None,
    };
    let mut per_feature: Vec<Vec<Value>> = vec![Vec::new(); table.len()];
    let mut not_numbers = 0usize;
    for members in groups(table, &gi, |_| true) {
        let mut stats: Vec<Value> = vec![Value::Int(members.len() as i64)];
        for a in &ai {
            let mut nums: Vec<f64> = Vec::with_capacity(members.len());
            for m in &members {
                match &table.features[*m].attrs[*a] {
                    Value::Null => {}
                    v => match v.as_f64() {
                        Some(x) => nums.push(x),
                        None => not_numbers += 1,
                    },
                }
            }
            if nums.is_empty() {
                stats.extend(STATS.iter().map(|_| Value::Null));
                stats.extend(percentiles.iter().map(|_| Value::Null));
                continue;
            }
            nums.sort_by(|x, y| x.total_cmp(y));
            let sum: f64 = nums.iter().sum();
            let mid = nums.len() / 2;
            let median = if nums.len() % 2 == 1 { nums[mid] } else { (nums[mid - 1] + nums[mid]) / 2.0 };
            stats.extend([sum, nums[0], nums[nums.len() - 1], sum / nums.len() as f64, median].map(Value::Real));
            stats.extend(percentiles.iter().map(|q| Value::Real(percentile(&nums, *q))));
        }
        let mut row: Vec<Value> = gi.iter().map(|i| table.features[members[0]].attrs[*i].clone()).collect();
        row.extend(stats.iter().cloned());
        summary.features.push(Feature { z: None, attrs: row, geom: None });
        for m in members {
            per_feature[m] = stats.clone();
        }
    }
    if not_numbers > 0 {
        ctx.notes.warn(format!("{} values are not numbers and were left out of the statistics.", not_numbers));
    }

    // Complete: every feature with the statistics of its group. Attributes with the same
    // names are replaced.
    let mut complete = table.clone();
    for (k, field) in stat_fields.iter().enumerate() {
        let values: Vec<Value> = per_feature.iter().map(|s| s.get(k).cloned().unwrap_or(Value::Null)).collect();
        complete = with_column(&complete, &field.name, values);
        let at = complete.field_index(&field.name).expect("the column was just added");
        complete.fields[at].ty = field.ty;
    }
    Ok(vec![("Summary".into(), summary), ("Complete".into(), complete)])
}

// ---------- Sort (Sorter) ----------

pub(super) const SORT_ORDERS: &[(&str, &str)] = &[("ascending", "Ascending"), ("descending", "Descending")];
pub(super) const SORTER_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "sort_keys",
    label: "Sort by",
    kind: ParamKind::SortKeys("Input"),
    default: "",
    hint: "The first attribute decides, the next breaks ties, and so on. Each has its own order. Empty values always come last",
}];
/// The orders of a Sort, for the form.
pub const SORT_DIRECTIONS: &[(&str, &str)] = SORT_ORDERS;

/// The attributes to sort by, each with whether it goes downwards: from the rows of the
/// form, or from sort_by and order as models before 0.61 have them.
pub fn sort_keys(params: &Params) -> Vec<(String, bool)> {
    if let Some(serde_json::Value::Array(rows)) = params.get("sort_keys") {
        return rows
            .iter()
            .filter_map(|r| {
                let a = r.as_array()?;
                let name = a.first()?.as_str()?.trim().to_string();
                let down = a.get(1).and_then(|d| d.as_str()) == Some("descending");
                (!name.is_empty()).then_some((name, down))
            })
            .collect();
    }
    let p = P::new(params, &[]);
    let down = p.get("order").as_deref() == Some("descending");
    p.list("sort_by").into_iter().map(|n| (n, down)).collect()
}

/// Numbers sort as numbers. Text sorts without regard to case, and by case only to break ties.
pub fn sort_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    let number = |v: &Value| match v {
        Value::Int(_) | Value::Real(_) | Value::Bool(_) => v.as_f64(),
        _ => None,
    };
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        _ => {
            let (x, y) = (a.to_text(), b.to_text());
            x.to_lowercase().cmp(&y.to_lowercase()).then_with(|| x.cmp(&y))
        }
    }
}

pub(super) fn run_sorter(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let by = sort_keys(p.values);
    if by.is_empty() {
        return err("Choose at least one attribute to sort by.");
    }
    if table.fields.is_empty() && table.is_empty() {
        return Ok(vec![("Sorted".into(), table.clone())]);
    }
    let idx: Vec<(usize, bool)> = by.iter().map(|(n, d)| table.need_field(n).map(|i| (i, *d))).collect::<Result<_>>()?;
    let mut out = table.clone();
    out.features.sort_by(|a, b| {
        for (i, descending) in &idx {
            let descending = *descending;
            let (x, y) = (&a.attrs[*i], &b.attrs[*i]);
            let o = match (x.is_null(), y.is_null()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => {
                    let o = sort_cmp(x, y);
                    if descending {
                        o.reverse()
                    } else {
                        o
                    }
                }
            };
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(vec![("Sorted".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "AttributeManager",
        title: "Manage attributes",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Keeps, removes, renames and orders attributes, and creates new ones.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: MANAGER_PARAMS,
        run: run_manager,
        summary: |p| {
            if manager_is_old(p.values) {
                let (k, r, n) = (p.list("keep").len(), p.list("remove").len(), p.pairs("rename").len());
                let mut parts = vec![if k > 0 { format!("keep {}", k) } else { "all".to_string() }];
                if r > 0 {
                    parts.push(format!("remove {}", r));
                }
                if n > 0 {
                    parts.push(format!("rename {}", n));
                }
                return parts.join(", ");
            }
            let rules = attribute_rules(p.values);
            let count = |f: fn(&AttrAction) -> bool| rules.iter().filter(|r| f(&r.1)).count();
            let (removed, renamed) = (count(|a| *a == AttrAction::Remove), count(|a| matches!(a, AttrAction::Rename(_))));
            let created = new_attributes(p.values).iter().filter(|n| !n.0.is_empty()).count();
            let mut parts = Vec::new();
            if p.get("others").as_deref() == Some("remove") {
                parts.push("only the listed".to_string());
            }
            for (n, what) in [(removed, "remove"), (renamed, "rename"), (created, "new")] {
                if n > 0 {
                    parts.push(format!("{} {}", what, n));
                }
            }
            if parts.is_empty() {
                "all as they are".to_string()
            } else {
                parts.join(", ")
            }
        },
    },
    NodeType {
        name: "FeatureMerger",
        title: "Join by attribute",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Joins attributes from Supplier where the key matches, as a join in SQL: inner, only those that match; left, all of Requestor; or full, all of both. The first match, or every match as one to many. The geometry comes from Requestor, and its attributes win when names clash, unless Supplier's are given a prefix. The number 1480 and the text 1480 count as the same key.",
        inputs: &["Requestor", "Supplier"],
        outputs: &["Merged", "UnmergedRequestor", "UnusedSupplier"],
        ports: None,
        params: MERGER_PARAMS,
        run: run_merger,
        summary: |p| {
            let kind = p.get("join").unwrap_or_else(|| "inner".into());
            let all = if p.get("matches").as_deref() == Some("all") { ", every match" } else { "" };
            format!("{} = {} · {}{}", p.list("requestor_key").join("+"), p.list("supplier_key").join("+"), kind, all)
        },
    },
    NodeType {
        name: "Sorter",
        title: "Sort",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Sorts the features by one or more attributes. Numbers sort as numbers, text without regard to case. Empty values come last.",
        inputs: &["Input"],
        outputs: &["Sorted"],
        ports: None,
        params: SORTER_PARAMS,
        run: run_sorter,
        summary: |p| {
            let keys = sort_keys(p.values);
            keys.iter().map(|(n, d)| if *d { format!("{} ▼", n) } else { n.clone() }).collect::<Vec<_>>().join(", ")
        },
    },
    NodeType {
        name: "StatisticsCalculator",
        title: "Calculate statistics",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Counts features and calculates sum, min, max, mean, median and percentiles of numeric attributes, for the whole stream or per group. Summary has one row per group, Complete has every feature with the figures of its group.",
        inputs: &["Input"],
        outputs: &["Summary", "Complete"],
        ports: None,
        params: STATS_PARAMS,
        run: run_stats,
        summary: |p| {
            let (a, g) = (p.list("attributes"), p.list("group_by"));
            let what = if a.is_empty() { "count".to_string() } else { a.join(", ") };
            if g.is_empty() {
                what
            } else {
                format!("{} by {}", what, g.join(", "))
            }
        },
    },
    NodeType {
        name: "AttributeKeeper",
        title: "Keep attributes",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Keeps the attributes you choose and removes all others. Quicker than Manage attributes when only a few of many are wanted.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: KEEPER_PARAMS,
        run: run_keeper,
        summary: |p| p.list("keep").join(", "),
    },
];
