//! Statistics of the attributes of a stream, for the Data tab.

use og_engine::model::{FieldType, Table, Value};
use std::collections::HashSet;

/// What there is to say about one attribute, or about the geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnStats {
    pub name: String,
    pub kind: String,
    /// Features with a value.
    pub values: usize,
    /// Features without one.
    pub empty: usize,
    pub unique: usize,
    pub min: Option<String>,
    pub max: Option<String>,
    pub mean: Option<f64>,
    pub sum: Option<f64>,
}

/// One row per attribute, and first one for the geometry when the stream has it.
pub fn column_stats(t: &Table) -> Vec<ColumnStats> {
    let mut out = Vec::new();
    if t.crs.is_some() || t.features.iter().any(|f| f.geom.is_some()) {
        let with = t.features.iter().filter(|f| f.geom.is_some()).count();
        let kinds: HashSet<&str> = t.features.iter().filter_map(|f| f.geom.as_ref()).map(og_engine::geom::type_name).collect();
        let mut kinds: Vec<&str> = kinds.into_iter().collect();
        kinds.sort();
        out.push(ColumnStats {
            name: "geometry".into(),
            kind: kinds.join(", "),
            values: with,
            empty: t.len() - with,
            unique: kinds.len(),
            min: None,
            max: None,
            mean: None,
            sum: None,
        });
    }
    for (c, field) in t.fields.iter().enumerate() {
        let numeric = matches!(field.ty, FieldType::Int | FieldType::Real);
        let mut seen: HashSet<String> = HashSet::new();
        let (mut values, mut empty) = (0, 0);
        let (mut lo, mut hi): (Option<f64>, Option<f64>) = (None, None);
        let (mut tlo, mut thi): (Option<String>, Option<String>) = (None, None);
        let mut sum = 0.0;
        let mut numbers = 0usize;
        for f in &t.features {
            let v = f.attrs.get(c).unwrap_or(&Value::Null);
            let text = v.to_text();
            if v.is_null() || (matches!(v, Value::Text(_)) && text.trim().is_empty()) {
                empty += 1;
                continue;
            }
            values += 1;
            let number = match v {
                Value::Int(i) => Some(*i as f64),
                Value::Real(r) if r.is_finite() => Some(*r),
                _ => None,
            };
            if let Some(x) = number.filter(|_| numeric) {
                numbers += 1;
                sum += x;
                lo = Some(lo.map_or(x, |l| l.min(x)));
                hi = Some(hi.map_or(x, |h| h.max(x)));
            } else {
                if tlo.as_ref().is_none_or(|l| text < *l) {
                    tlo = Some(text.clone());
                }
                if thi.as_ref().is_none_or(|h| text > *h) {
                    thi = Some(text.clone());
                }
            }
            seen.insert(text);
        }
        let as_text = |x: f64| if field.ty == FieldType::Int && x.fract() == 0.0 { format!("{}", x as i64) } else { fmt_number(x) };
        let (min, max, mean, total) =
            if numbers > 0 { (lo.map(as_text), hi.map(as_text), Some(sum / numbers as f64), Some(sum)) } else { (tlo.map(short), thi.map(short), None, None) };
        out.push(ColumnStats { name: field.name.clone(), kind: field.ty.label().to_string(), values, empty, unique: seen.len(), min, max, mean, sum: total });
    }
    out
}

/// The beginning of a long text, on one line, as a cell can show it: the answer of an API can
/// be megabytes.
fn short(t: String) -> String {
    const MOST: usize = 80;
    if t.chars().count() <= MOST && !t.contains('\n') {
        return t;
    }
    let mut s: String = t.chars().take(MOST).map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    if t.chars().nth(MOST).is_some() {
        s.push('…');
    }
    s
}

/// A number with as many decimals as it needs, at most four.
pub fn fmt_number(x: f64) -> String {
    let s = format!("{:.4}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_engine::model::{Feature, Field};

    #[test]
    fn numbers_and_texts_are_summed_up() {
        let t = Table {
            fields: vec![Field::new("n", FieldType::Int), Field::new("name", FieldType::Text)],
            crs: None,
            features: vec![
                Feature { z: None, attrs: vec![Value::Int(4), Value::Text("Borås".into())], geom: None },
                Feature { z: None, attrs: vec![Value::Int(10), Value::Text("Alingsås".into())], geom: None },
                Feature { z: None, attrs: vec![Value::Null, Value::Text("Borås".into())], geom: None },
                Feature { z: None, attrs: vec![Value::Int(4), Value::Text(" ".into())], geom: None },
            ],
            ..Default::default()
        };
        let s = column_stats(&t);
        assert_eq!(s.len(), 2, "no geometry row without geometry");
        assert_eq!((s[0].values, s[0].empty, s[0].unique), (3, 1, 2));
        assert_eq!((s[0].min.as_deref(), s[0].max.as_deref(), s[0].sum), (Some("4"), Some("10"), Some(18.0)));
        assert_eq!(s[0].mean, Some(6.0));
        assert_eq!((s[1].values, s[1].empty, s[1].unique), (3, 1, 2));
        assert_eq!((s[1].min.as_deref(), s[1].max.as_deref(), s[1].mean), (Some("Alingsås"), Some("Borås"), None));
        assert_eq!(fmt_number(2.5), "2.5");
        assert_eq!(fmt_number(1.0 / 3.0), "0.3333");
    }
}
