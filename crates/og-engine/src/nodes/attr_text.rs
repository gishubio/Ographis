//! Tools for text in attributes: Split attribute, Extract substring, Concatenate attributes,
//! Change case and Replace text.

use super::*;

// ---------- Split attribute (AttributeSplitter) ----------

pub(super) const SPLIT_MODES: &[(&str, &str)] = &[("columns", "Into attributes: name_1, name_2 …"), ("rows", "One feature per part")];

pub(super) const SPLITTER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "Attribute", kind: ParamKind::Attribute("Input"), default: "", hint: "" },
    ParamSpec { key: "delimiter", label: "Split at", kind: ParamKind::Text, default: ",", hint: "A character or a text, for example , ; $ or |. Write \\t for a tab" },
    ParamSpec {
        key: "mode",
        label: "Parts go",
        kind: ParamKind::Choice(SPLIT_MODES),
        default: "columns",
        hint: "Into attributes suits a fixed pattern, such as Göteborg$1480. One feature per part suits a list, such as 1,4,7, that is then filtered, joined or counted",
    },
    ParamSpec {
        key: "name",
        label: "New name",
        kind: ParamKind::Text,
        default: "",
        hint: "Into attributes: the start of the new names. One feature per part: the attribute the part goes in. Empty uses the name of the split attribute",
    },
    ParamSpec { key: "max", label: "Largest number of parts", kind: ParamKind::Text, default: "", hint: "The last part keeps the rest. Empty splits at every one" },
    ParamSpec { key: "trim", label: "Remove spaces around the parts", kind: ParamKind::Choice(YES_NO), default: "true", hint: "" },
    ParamSpec { key: "keep_empty", label: "Keep empty parts", kind: ParamKind::Choice(YES_NO), default: "false", hint: "As in a,,b" },
    ParamSpec {
        key: "index",
        label: "Number attribute",
        kind: ParamKind::Text,
        default: "",
        hint: "One feature per part: an attribute with the number of the part, 1, 2 and so on. Empty adds none",
    },
];

/// The parts of a text, as the splitter is set up.
pub(super) fn split_parts(text: &str, delimiter: &str, max: Option<usize>, trim: bool, keep_empty: bool) -> Vec<String> {
    let raw: Vec<&str> = match max {
        Some(n) => text.splitn(n, delimiter).collect(),
        None => text.split(delimiter).collect(),
    };
    raw.into_iter().map(|s| if trim { s.trim() } else { s }).filter(|s| keep_empty || !s.is_empty()).map(str::to_string).collect()
}

pub(super) fn run_splitter(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.need("attribute", "Choose the attribute to split.")?;
    let i = table.field_index(&attr).ok_or_else(|| GbError(format!("There is no attribute {}. The stream has: {}.", attr, table.field_names().join(", "))))?;
    let delimiter = p.exact("delimiter").replace("\\t", "\t");
    if delimiter.is_empty() {
        return err("Say what to split at, for example a comma.");
    }
    let max = match p.get("max") {
        None => None,
        Some(t) => match t.parse::<usize>() {
            Ok(n) if n >= 1 => Some(n),
            _ => return err(format!("The largest number of parts must be a whole number above zero, not \"{}\".", t)),
        },
    };
    let (trim, keep_empty) = (p.flag("trim"), p.flag("keep_empty"));
    let parts: Vec<Option<Vec<String>>> = table
        .features
        .iter()
        .map(|f| match &f.attrs[i] {
            Value::Null => None,
            v => Some(split_parts(&v.to_text(), &delimiter, max, trim, keep_empty)),
        })
        .collect();
    let base = p.get("name").unwrap_or_else(|| attr.clone());
    if p.get("mode").as_deref() == Some("rows") {
        // One feature per part. A feature with nothing to split comes out once, with an empty part.
        let index = p.get("index");
        let mut texts = Vec::new();
        let mut numbers = Vec::new();
        let mut from = Vec::new();
        for (k, list) in parts.iter().enumerate() {
            match list.as_deref() {
                None | Some([]) => {
                    texts.push(None);
                    numbers.push(Value::Null);
                    from.push(k);
                }
                Some(list) => {
                    for (n, part) in list.iter().enumerate() {
                        texts.push(Some(part.clone()));
                        numbers.push(Value::Int(n as i64 + 1));
                        from.push(k);
                    }
                }
            }
        }
        let mut rows = table.like();
        rows.features = from.iter().map(|k| table.features[*k].clone()).collect();
        let mut out = with_column(&rows, &base, typed_column(&texts));
        if let Some(name) = index {
            out = with_column(&out, &name, numbers);
        }
        if out.len() > table.len() {
            ctx.notes.info(format!("{} features became {}", fmt_count(table.len() as u64), fmt_count(out.len() as u64)));
        }
        return Ok(vec![("Output".into(), out)]);
    }
    // Into attributes: as many as the most parts any feature has.
    let most = parts.iter().flatten().map(|l| l.len()).max().unwrap_or(0);
    let mut out = table.clone();
    for n in 0..most {
        let texts: Vec<Option<String>> = parts.iter().map(|l| l.as_ref().and_then(|l| l.get(n).cloned())).collect();
        out = with_column(&out, &format!("{}_{}", base, n + 1), typed_column(&texts));
    }
    if most == 0 {
        ctx.notes.warn(format!("There was nothing in {} to split, so no attributes were added.", attr));
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Extract substring (SubstringExtractor) ----------

pub(super) const SUBSTRING_PARTS: &[(&str, &str)] = &[("first", "The first characters"), ("last", "The last characters"), ("middle", "From a position")];
pub(super) const SUBSTRING_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attribute", label: "From attribute", kind: ParamKind::Attribute("Input"), default: "", hint: "" },
    ParamSpec { key: "part", label: "Take", kind: ParamKind::Choice(SUBSTRING_PARTS), default: "first", hint: "" },
    ParamSpec { key: "count", label: "Number of characters", kind: ParamKind::Text, default: "4", hint: "" },
    ParamSpec { key: "start", label: "Start position", kind: ParamKind::Text, default: "1", hint: "The first character has position 1" },
    ParamSpec { key: "name", label: "New attribute", kind: ParamKind::Text, default: "_substring", hint: "The name of the source attribute overwrites it" },
];

pub(super) fn whole_number(p: &P, key: &str, what: &str, least: usize) -> Result<usize> {
    let text = p.get(key).unwrap_or_default();
    match text.parse::<usize>() {
        Ok(n) if n >= least => Ok(n),
        _ => err(format!("{} must be a whole number, {} or more, not \"{}\".", what, least, text)),
    }
}

/// Part of a text, counted in characters and not in bytes, so that å, ä and ö count as one.
pub fn substring(s: &str, part: &str, count: usize, start: usize) -> String {
    let n = s.chars().count();
    match part {
        "last" => s.chars().skip(n.saturating_sub(count)).collect(),
        "middle" => s.chars().skip(start.saturating_sub(1)).take(count).collect(),
        _ => s.chars().take(count).collect(),
    }
}

pub(super) fn run_substring(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.list("attribute").into_iter().next().ok_or_else(|| GbError("Choose the attribute to take the characters from.".into()))?;
    let part = p.get("part").unwrap_or_default();
    let count = whole_number(p, "count", "The number of characters", 0)?;
    let start = if part == "middle" { whole_number(p, "start", "The start position", 1)? } else { 1 };
    let name = p.get("name").unwrap_or_else(|| "_substring".into());
    if table.fields.is_empty() && table.is_empty() {
        return Ok(vec![("Output".into(), table.clone())]);
    }
    let i = table.need_field(&attr)?;
    let values = table
        .features
        .iter()
        .map(|f| match &f.attrs[i] {
            Value::Null => Value::Null,
            v => Value::Text(substring(&v.to_text(), &part, count, start)),
        })
        .collect();
    let mut out = with_column(table, &name, values);
    let at = out.field_index(&name).expect("the column was just added");
    out.fields[at].ty = FieldType::Text;
    Ok(vec![("Output".into(), out)])
}

// ---------- Concatenate attributes (StringConcatenator) ----------

pub(super) const CONCAT_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attributes", label: "Attributes", kind: ParamKind::Attributes("Input"), default: "", hint: "Joined in the order they are chosen" },
    ParamSpec {
        key: "separator",
        label: "Separator",
        kind: ParamKind::Exact,
        default: "",
        hint: "Text to put between the values, such as a space or _ . Empty puts nothing",
    },
    ParamSpec {
        key: "skip_empty",
        label: "Skip empty values",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "An empty value then gives no separator either",
    },
    ParamSpec { key: "name", label: "New attribute", kind: ParamKind::Text, default: "_concatenated", hint: "" },
];

pub(super) fn run_concat(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attrs = p.list("attributes");
    if attrs.is_empty() {
        return err("Choose the attributes to join.");
    }
    let name = p.get("name").unwrap_or_else(|| "_concatenated".into());
    if table.fields.is_empty() && table.is_empty() {
        return Ok(vec![("Output".into(), table.clone())]);
    }
    let idx: Vec<usize> = attrs.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;
    let (separator, skip) = (p.exact("separator"), p.flag("skip_empty"));
    let values = table
        .features
        .iter()
        .map(|f| {
            let parts: Vec<String> = idx.iter().map(|i| f.attrs[*i].to_text()).filter(|t| !(skip && t.is_empty())).collect();
            Value::Text(parts.join(&separator))
        })
        .collect();
    let mut out = with_column(table, &name, values);
    let at = out.field_index(&name).expect("the column was just added");
    out.fields[at].ty = FieldType::Text;
    Ok(vec![("Output".into(), out)])
}

// ---------- Replace text (StringReplacer) ----------

pub(super) const CASES: &[(&str, &str)] =
    &[("upper", "UPPER CASE: AAA"), ("lower", "lower case: aaa"), ("first", "First letter capital: Aaa bbb"), ("words", "Every word capital: Aaa Bbb")];

pub(super) const CASE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attributes", label: "In attributes", kind: ParamKind::Attributes("Input"), default: "", hint: "" },
    ParamSpec {
        key: "case",
        label: "Make it",
        kind: ParamKind::Choice(CASES),
        default: "upper",
        hint: "Every word capital also counts the parts of names with - as words, as in Dals-Ed",
    },
];

/// A text in upper or lower case, with the first letter capital, or every word.
pub fn change_case(s: &str, case: &str) -> String {
    match case {
        "lower" => s.to_lowercase(),
        "first" => {
            let lower = s.to_lowercase();
            let mut out = String::with_capacity(lower.len());
            let mut done = false;
            for c in lower.chars() {
                if !done && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    done = true;
                } else {
                    out.push(c);
                }
            }
            out
        }
        "words" => crate::expr::initcap(s),
        _ => s.to_uppercase(),
    }
}

pub(super) fn run_case(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attrs = p.list("attributes");
    if attrs.is_empty() {
        return err("Choose the attributes to change.");
    }
    let case = p.get("case").unwrap_or_else(|| "upper".into());
    let mut out = table.clone();
    for attr in &attrs {
        let i = table.need_field(attr)?;
        // Only text changes; numbers and empty values stay as they are.
        for f in out.features.iter_mut() {
            if let Value::Text(t) = &f.attrs[i] {
                f.attrs[i] = Value::Text(change_case(t, &case));
            }
        }
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) const REPLACER_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "attributes", label: "In attributes", kind: ParamKind::Attributes("Input"), default: "", hint: "" },
    ParamSpec { key: "find", label: "Find", kind: ParamKind::Exact, default: "", hint: "The text to replace. Spaces count" },
    ParamSpec { key: "replace", label: "Replace with", kind: ParamKind::Exact, default: "", hint: "Empty removes what was found" },
    ParamSpec { key: "match_case", label: "Tell A from a", kind: ParamKind::Choice(YES_NO), default: "true", hint: "" },
];

/// Replaces every occurrence of `find`. Without `match_case`, A and a count as the same.
pub fn replace_text(s: &str, find: &str, with: &str, match_case: bool) -> String {
    if find.is_empty() {
        return s.to_string();
    }
    if match_case {
        return s.replace(find, with);
    }
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let (text, find): (Vec<char>, Vec<char>) = (s.chars().collect(), find.chars().collect());
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < text.len() {
        if i + find.len() <= text.len() && find.iter().zip(&text[i..]).all(|(a, b)| same(*a, *b)) {
            out.push_str(with);
            i += find.len();
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

pub(super) fn run_replacer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attrs = p.list("attributes");
    if attrs.is_empty() {
        return err("Choose the attributes to replace text in.");
    }
    let (find, with, match_case) = (p.exact("find"), p.exact("replace"), p.get("match_case").as_deref() != Some("false"));
    if find.is_empty() {
        return err("Give the text to find.");
    }
    if table.fields.is_empty() && table.is_empty() {
        return Ok(vec![("Output".into(), table.clone())]);
    }
    let mut out = table.clone();
    let mut changed = 0usize;
    for attr in &attrs {
        let i = table.need_field(attr)?;
        // A value that is not touched keeps its type, so that a number stays a number.
        let values: Vec<Value> = table
            .features
            .iter()
            .map(|f| match &f.attrs[i] {
                Value::Null => Value::Null,
                v => {
                    let (before, after) = (v.to_text(), replace_text(&v.to_text(), &find, &with, match_case));
                    if before == after {
                        v.clone()
                    } else {
                        changed += 1;
                        Value::Text(after)
                    }
                }
            })
            .collect();
        let ty = table.fields[i].ty;
        out = with_column(&out, attr, values);
        // The type only changes when a value did. An untouched column keeps what it had.
        let ty = out.fields[i].ty.widen(ty);
        out.fields[i].ty = ty;
        out.features.iter_mut().for_each(|f| f.attrs[i] = coerce(std::mem::replace(&mut f.attrs[i], Value::Null), ty));
    }
    ctx.notes.info(format!("{} values changed", changed));
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "SubstringExtractor",
        title: "Extract substring",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Takes part of a text into a new attribute: the first characters, the last, or a number of characters from a position.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: SUBSTRING_PARAMS,
        run: run_substring,
        summary: |p| {
            let (attr, count) = (p.list("attribute").into_iter().next().unwrap_or_else(|| "?".into()), p.get("count").unwrap_or_default());
            match p.get("part").as_deref() {
                Some("last") => format!("last {} of {}", count, attr),
                Some("middle") => format!("{} from position {} of {}", count, p.get("start").unwrap_or_default(), attr),
                _ => format!("first {} of {}", count, attr),
            }
        },
    },
    NodeType {
        name: "StringConcatenator",
        title: "Concatenate attributes",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Joins several attributes into one new text attribute, with a separator between them if you want one.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: CONCAT_PARAMS,
        run: run_concat,
        summary: |p| {
            let attrs = p.list("attributes");
            if attrs.is_empty() {
                "nothing chosen".to_string()
            } else {
                format!("{} = {}", p.get("name").unwrap_or_default(), attrs.join(&format!(" {} ", if p.exact("separator").is_empty() { "+".to_string() } else { format!("\"{}\"", p.exact("separator")) })))
            }
        },
    },
    NodeType {
        name: "CaseChanger",
        title: "Change case",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Makes the text of the chosen attributes UPPER CASE, lower case, with the first letter capital, or every word capital: SKÖVDE, skövde, Skövde, Lilla Edet.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: CASE_PARAMS,
        run: run_case,
        summary: |p| {
            let case = p.get("case").unwrap_or_else(|| "upper".into());
            let shown = match case.as_str() {
                "lower" => "lower case",
                "first" => "First letter",
                "words" => "Every Word",
                _ => "UPPER CASE",
            };
            format!("{} · {}", p.list("attributes").join(", "), shown)
        },
    },
    NodeType {
        name: "StringReplacer",
        title: "Replace text",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Replaces a text with another in the chosen attributes: every A becomes B, or every space becomes nothing.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: REPLACER_PARAMS,
        run: run_replacer,
        summary: |p| format!("\"{}\" → \"{}\"", p.exact("find"), p.exact("replace")),
    },
    NodeType {
        name: "AttributeSplitter",
        title: "Split attribute",
        hidden: false,
        group: Group::Transformer,
        category: "Attributes",
        desc: "Splits a text at a character, such as a comma or $. The parts become attributes of their own, or one feature each.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: SPLITTER_PARAMS,
        run: run_splitter,
        summary: |p| match p.get("attribute") {
            Some(a) => {
                let how = if p.get("mode").as_deref() == Some("rows") { "one feature per part" } else { "into attributes" };
                format!("{} at \"{}\" · {}", a, p.exact("delimiter"), how)
            }
            None => String::new(),
        },
    },
];
