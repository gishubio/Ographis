//! Information Delivery Specification (IDS 1.0): what objects of an IFC model must have, as
//! buildingSMART writes it. Each specification says which objects it applies to and what
//! they must, may or must not have; the objects are checked against the attributes that
//! reading the IFC file gave them: class, attributes, property sets, material and storey.

use crate::model::{Feature, GbError, Result, Table, Value};
use serde_json::Value as Json;

/// A value as IDS gives it: one value, or a restriction of the values allowed.
#[derive(Debug, Clone)]
pub enum Allowed {
    Simple(String),
    Restriction {
        enumeration: Vec<String>,
        pattern: Option<regex::Regex>,
        min: Option<(f64, bool)>,
        max: Option<(f64, bool)>,
        length: Option<usize>,
        min_length: Option<usize>,
        max_length: Option<usize>,
    },
}

impl Allowed {
    /// Whether a value is allowed. Numbers compare as numbers, TRUE and FALSE without case.
    pub fn allows(&self, v: &str) -> bool {
        let v = v.trim();
        match self {
            Allowed::Simple(s) => {
                let s = s.trim();
                if s.eq_ignore_ascii_case(v) && (s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false")) {
                    return true;
                }
                match (s.parse::<f64>(), v.parse::<f64>()) {
                    (Ok(a), Ok(b)) => (a - b).abs() <= 1e-6 * a.abs().max(1.0),
                    _ => s == v,
                }
            }
            Allowed::Restriction { enumeration, pattern, min, max, length, min_length, max_length } => {
                if !enumeration.is_empty() && !enumeration.iter().any(|e| Allowed::Simple(e.clone()).allows(v)) {
                    return false;
                }
                if let Some(p) = pattern {
                    if !p.is_match(v) {
                        return false;
                    }
                }
                let n = v.parse::<f64>().ok();
                if let Some((m, inclusive)) = min {
                    match n {
                        Some(x) if if *inclusive { x >= *m } else { x > *m } => {}
                        _ => return false,
                    }
                }
                if let Some((m, inclusive)) = max {
                    match n {
                        Some(x) if if *inclusive { x <= *m } else { x < *m } => {}
                        _ => return false,
                    }
                }
                let chars = v.chars().count();
                length.is_none_or(|l| chars == l) && min_length.is_none_or(|l| chars >= l) && max_length.is_none_or(|l| chars <= l)
            }
        }
    }

    fn text(&self) -> String {
        match self {
            Allowed::Simple(s) => s.clone(),
            Allowed::Restriction { enumeration, pattern, min, max, .. } => {
                let mut parts = Vec::new();
                if !enumeration.is_empty() {
                    parts.push(format!("one of {}", enumeration.join(", ")));
                }
                if let Some(p) = pattern {
                    parts.push(format!("like {}", p.as_str().trim_start_matches("^(?:").trim_end_matches(")$")));
                }
                if let Some((m, i)) = min {
                    parts.push(format!("{} {}", if *i { "at least" } else { "more than" }, m));
                }
                if let Some((m, i)) = max {
                    parts.push(format!("{} {}", if *i { "at most" } else { "less than" }, m));
                }
                if parts.is_empty() {
                    "a value".into()
                } else {
                    parts.join(", ")
                }
            }
        }
    }
}

/// What an object is checked for.
#[derive(Debug, Clone)]
pub enum Facet {
    Entity { name: Allowed, predefined: Option<Allowed> },
    Attribute { name: Allowed, value: Option<Allowed> },
    Property { set: Allowed, name: Allowed, value: Option<Allowed> },
    Material { value: Option<Allowed> },
    Classification { system: Option<Allowed>, value: Option<Allowed> },
    PartOf { entity: Option<Allowed> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cardinality {
    Required,
    Optional,
    Prohibited,
}

#[derive(Debug, Clone)]
pub struct Specification {
    pub name: String,
    /// At least this many objects must be there for it, 0 when none need be.
    pub min_occurs: u32,
    pub applicability: Vec<Facet>,
    pub requirements: Vec<(Facet, Cardinality)>,
}

fn list(v: &Json) -> Vec<&Json> {
    match v {
        Json::Array(a) => a.iter().collect(),
        Json::Null => Vec::new(),
        other => vec![other],
    }
}

fn text_of(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        Json::Object(o) => o.get("value").map(text_of).unwrap_or_default(),
        other => other.to_string(),
    }
}

/// A value of IDS: <simpleValue> or <restriction>.
fn allowed(v: &Json) -> Result<Option<Allowed>> {
    if v.is_null() {
        return Ok(None);
    }
    if let Some(s) = v.get("simpleValue") {
        return Ok(Some(Allowed::Simple(text_of(s))));
    }
    let Some(r) = v.get("restriction") else {
        // A value written straight in.
        return Ok(Some(Allowed::Simple(text_of(v))));
    };
    let vals = |key: &str| -> Vec<String> {
        list(r.get(key).unwrap_or(&Json::Null)).iter().map(|e| e.get("value").map(text_of).unwrap_or_else(|| text_of(e))).collect()
    };
    let num = |key: &str| vals(key).first().and_then(|s| s.parse::<f64>().ok());
    let int = |key: &str| vals(key).first().and_then(|s| s.parse::<usize>().ok());
    let pattern = match vals("pattern").first() {
        Some(p) => Some(regex::Regex::new(&format!("^(?:{})$", p)).map_err(|e| GbError(format!("The pattern {} of the IDS cannot be read: {}", p, e)))?),
        None => None,
    };
    Ok(Some(Allowed::Restriction {
        enumeration: vals("enumeration"),
        pattern,
        min: num("minInclusive").map(|m| (m, true)).or(num("minExclusive").map(|m| (m, false))),
        max: num("maxInclusive").map(|m| (m, true)).or(num("maxExclusive").map(|m| (m, false))),
        length: int("length"),
        min_length: int("minLength"),
        max_length: int("maxLength"),
    }))
}

fn facets(group: &Json, with_cardinality: bool) -> Result<Vec<(Facet, Cardinality)>> {
    let mut out = Vec::new();
    let card = |f: &Json| -> Cardinality {
        let c = f.get("cardinality").map(text_of).unwrap_or_default();
        let min = f.get("minOccurs").map(text_of).unwrap_or_default();
        let max = f.get("maxOccurs").map(text_of).unwrap_or_default();
        match c.as_str() {
            "prohibited" => Cardinality::Prohibited,
            "optional" => Cardinality::Optional,
            "required" => Cardinality::Required,
            _ if max == "0" => Cardinality::Prohibited,
            _ if min == "0" => Cardinality::Optional,
            _ => Cardinality::Required,
        }
    };
    let need = |a: Option<Allowed>, what: &str| a.ok_or_else(|| GbError(format!("A facet of the IDS has no {}.", what)));
    for f in list(group.get("entity").unwrap_or(&Json::Null)) {
        out.push((Facet::Entity { name: need(allowed(&f["name"])?, "name")?, predefined: allowed(&f["predefinedType"])? }, Cardinality::Required));
    }
    for f in list(group.get("attribute").unwrap_or(&Json::Null)) {
        out.push((Facet::Attribute { name: need(allowed(&f["name"])?, "name")?, value: allowed(&f["value"])? }, card(f)));
    }
    for f in list(group.get("property").unwrap_or(&Json::Null)) {
        out.push((
            Facet::Property {
                set: need(allowed(&f["propertySet"])?, "property set")?,
                name: need(allowed(&f["baseName"])?, "base name")?,
                value: allowed(&f["value"])?,
            },
            card(f),
        ));
    }
    for f in list(group.get("material").unwrap_or(&Json::Null)) {
        out.push((Facet::Material { value: allowed(&f["value"])? }, card(f)));
    }
    for f in list(group.get("classification").unwrap_or(&Json::Null)) {
        out.push((Facet::Classification { system: allowed(&f["system"])?, value: allowed(&f["value"])? }, card(f)));
    }
    for f in list(group.get("partOf").unwrap_or(&Json::Null)) {
        out.push((Facet::PartOf { entity: allowed(f.get("entity").map(|e| &e["name"]).unwrap_or(&Json::Null))? }, card(f)));
    }
    if !with_cardinality {
        for f in out.iter_mut() {
            f.1 = Cardinality::Required;
        }
    }
    Ok(out)
}

/// The specifications of an IDS file.
pub fn read(text: &str) -> Result<Vec<Specification>> {
    let v = crate::xml::to_json(text)?;
    let ids = v.get("ids").ok_or_else(|| GbError("This is not an IDS file: it has no <ids> element.".into()))?;
    let mut out = Vec::new();
    for (k, s) in list(ids.get("specifications").and_then(|s| s.get("specification")).unwrap_or(&Json::Null)).into_iter().enumerate() {
        let name = s.get("name").map(text_of).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Specification {}", k + 1));
        let app = s.get("applicability").cloned().unwrap_or(Json::Null);
        let min_occurs = app.get("minOccurs").map(text_of).and_then(|m| m.parse().ok()).unwrap_or(0);
        out.push(Specification {
            name,
            min_occurs,
            applicability: facets(&app, false)?.into_iter().map(|f| f.0).collect(),
            requirements: facets(s.get("requirements").unwrap_or(&Json::Null), true)?,
        });
    }
    if out.is_empty() {
        return Err(GbError("The IDS file has no specifications.".into()));
    }
    Ok(out)
}

/// The values of an object that a facet looks at, by the attributes reading IFC gives.
struct Object<'a> {
    table: &'a Table,
    f: &'a Feature,
}

impl Object<'_> {
    fn get(&self, name: &str) -> Option<String> {
        let i = self.table.field_index(name)?;
        let v = &self.f.attrs[i];
        (!v.is_null() && !v.to_text().trim().is_empty()).then(|| v.to_text())
    }

    /// Whether the facet holds for the object; None when it cannot be told from what was read.
    fn holds(&self, facet: &Facet) -> Option<bool> {
        match facet {
            Facet::Entity { name, predefined } => {
                let class = self.get("IfcClass")?.to_uppercase();
                let ok = match name {
                    Allowed::Simple(s) => s.trim().to_uppercase() == class,
                    r => r.allows(&class),
                };
                match predefined {
                    Some(p) if ok => self.get("PredefinedType").map(|t| p.allows(&t)),
                    _ => Some(ok),
                }
            }
            Facet::Attribute { name, value } => Some(self.table.fields.iter().enumerate().any(|(i, fd)| {
                name.allows(&fd.name) && !fd.name.contains('.') && {
                    let v = &self.f.attrs[i];
                    !v.is_null() && !v.to_text().trim().is_empty() && value.as_ref().is_none_or(|a| a.allows(&v.to_text()))
                }
            })),
            Facet::Property { set, name, value } => Some(self.table.fields.iter().enumerate().any(|(i, fd)| {
                let Some((s, n)) = fd.name.split_once('.') else { return false };
                set.allows(s) && name.allows(n) && {
                    let v = &self.f.attrs[i];
                    !v.is_null() && !v.to_text().trim().is_empty() && value.as_ref().is_none_or(|a| a.allows(&v.to_text()))
                }
            })),
            Facet::Material { value } => {
                let m = self.get("material");
                Some(match (m, value) {
                    (None, _) => false,
                    (Some(_), None) => true,
                    (Some(m), Some(a)) => a.allows(&m) || m.split([';', ',']).any(|p| a.allows(p)),
                })
            }
            Facet::Classification { .. } => None,
            Facet::PartOf { entity } => match entity {
                Some(Allowed::Simple(e)) if e.trim().eq_ignore_ascii_case("IFCBUILDINGSTOREY") => Some(self.get("storey").is_some()),
                _ => None,
            },
        }
    }
}

fn describe(f: &Facet) -> String {
    let v = |a: &Option<Allowed>| a.as_ref().map(|a| format!(" = {}", a.text())).unwrap_or_default();
    match f {
        Facet::Entity { name, .. } => format!("the class {}", name.text()),
        Facet::Attribute { name, value } => format!("the attribute {}{}", name.text(), v(value)),
        Facet::Property { set, name, value } => format!("the property {}.{}{}", set.text(), name.text(), v(value)),
        Facet::Material { value } => format!("a material{}", v(value)),
        Facet::Classification { .. } => "a classification".into(),
        Facet::PartOf { entity } => format!("being part of {}", entity.as_ref().map(|e| e.text()).unwrap_or_default()),
    }
}

/// The result of a check: for each object, the specifications that applied to it and what
/// it lacked; for each specification, how many it applied to and how many failed; and the
/// requirements that could not be checked from what was read.
pub struct Checked {
    pub applied: Vec<usize>,
    pub failures: Vec<Vec<String>>,
    pub per_spec: Vec<(String, usize, usize, u32)>,
    pub unchecked: Vec<String>,
}

pub fn check(specs: &[Specification], table: &Table) -> Checked {
    let mut applied = vec![0usize; table.len()];
    let mut failures: Vec<Vec<String>> = vec![Vec::new(); table.len()];
    let mut per_spec = Vec::new();
    let mut unchecked: Vec<String> = Vec::new();
    for spec in specs {
        let (mut n, mut failed) = (0usize, 0usize);
        for (row, f) in table.features.iter().enumerate() {
            let o = Object { table, f };
            let applies = spec.applicability.iter().all(|a| o.holds(a) == Some(true));
            if !applies || spec.applicability.is_empty() {
                continue;
            }
            n += 1;
            applied[row] += 1;
            let mut lacks = Vec::new();
            for (req, card) in &spec.requirements {
                match (o.holds(req), card) {
                    (None, _) => {
                        let what = format!("{}: {}", spec.name, describe(req));
                        if !unchecked.contains(&what) {
                            unchecked.push(what);
                        }
                    }
                    (Some(false), Cardinality::Required) => lacks.push(format!("{}: lacks {}", spec.name, describe(req))),
                    (Some(true), Cardinality::Prohibited) => lacks.push(format!("{}: must not have {}", spec.name, describe(req))),
                    // Optional: when it is there, its value must be allowed.
                    (Some(false), Cardinality::Optional) => {
                        let present = match req {
                            Facet::Attribute { name, .. } => table.fields.iter().enumerate().any(|(i, fd)| name.allows(&fd.name) && !f.attrs[i].is_null()),
                            Facet::Property { set, name, .. } => table
                                .fields
                                .iter()
                                .enumerate()
                                .any(|(i, fd)| fd.name.split_once('.').is_some_and(|(s, nm)| set.allows(s) && name.allows(nm)) && !f.attrs[i].is_null()),
                            Facet::Material { .. } => o.get("material").is_some(),
                            _ => false,
                        };
                        if present {
                            lacks.push(format!("{}: has {} with a value that is not allowed", spec.name, describe(req)));
                        }
                    }
                    _ => {}
                }
            }
            if !lacks.is_empty() {
                failed += 1;
                failures[row].extend(lacks);
            }
        }
        per_spec.push((spec.name.clone(), n, failed, spec.min_occurs));
    }
    Checked { applied, failures, per_spec, unchecked }
}

/// The text of a value for the attribute of failures.
pub fn joined(lacks: &[String]) -> Value {
    if lacks.is_empty() {
        Value::Null
    } else {
        Value::Text(lacks.join("; "))
    }
}
