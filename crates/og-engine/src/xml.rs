//! XML as data: a file or a text made into the same JSON that the JSON tools read, so that
//! records are found, flattened and picked by path in the same way; and features written as
//! XML.

use crate::model::{GbError, Result, Table, Value};
use quick_xml::events::Event;
use serde_json::{Map, Value as Json};

/// A text that reads as a number: without a leading zero, so that codes such as 0123 stay
/// text, as they are meant.
fn number_of(s: &str) -> Option<Json> {
    let t = s.trim();
    if t.is_empty() || t.len() > 20 {
        return None;
    }
    let digits = t.trim_start_matches('-');
    if digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0.") {
        return None;
    }
    if !t.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | 'e' | 'E' | '+')) {
        return None;
    }
    if let Ok(i) = t.parse::<i64>() {
        return Some(Json::from(i));
    }
    t.parse::<f64>().ok().filter(|f| f.is_finite()).and_then(|f| serde_json::Number::from_f64(f).map(Json::Number))
}

fn local(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

/// An element being read: its attributes and children so far, and its text.
struct Open {
    name: String,
    obj: Map<String, Json>,
    /// The names in `obj` that are attributes.
    attrs: Vec<String>,
    text: String,
}

impl Open {
    fn new(name: String, attrs: Map<String, Json>) -> Open {
        let names = attrs.keys().cloned().collect();
        Open { name, obj: attrs, attrs: names, text: String::new() }
    }

    /// A child element: a name that comes again becomes a list of its values, and an
    /// attribute of the same name gets an @ in front.
    fn child(&mut self, key: String, v: Json) {
        if let Some(i) = self.attrs.iter().position(|a| *a == key) {
            self.attrs.remove(i);
            if let Some(a) = self.obj.remove(&key) {
                self.obj.insert(format!("@{}", key), a);
            }
        }
        match self.obj.get_mut(&key) {
            Some(old) => {
                let first = old.take();
                *old = match first {
                    Json::Array(mut a) if a.len() > 1 => {
                        a.push(v);
                        Json::Array(a)
                    }
                    first => Json::Array(vec![first, v]),
                };
            }
            None => {
                self.obj.insert(key, v);
            }
        }
    }
}

/// What an element becomes: its text as a value when it has nothing else, or an object of
/// its attributes and children, with its text as `value`.
fn close(e: Open) -> Json {
    let text = e.text.trim();
    if e.obj.is_empty() {
        return number_of(text).unwrap_or_else(|| if text.is_empty() { Json::Null } else { Json::String(text.to_string()) });
    }
    let mut obj = e.obj;
    if !text.is_empty() {
        obj.insert("value".into(), number_of(text).unwrap_or_else(|| Json::String(text.to_string())));
    }
    Json::Object(obj)
}

/// An XML text as JSON: an object with the root element under its name. Attributes are
/// values under their names (with @ in front when a child element has the same name),
/// elements that come again are lists, and prefixes of namespaces are left off.
pub fn to_json(text: &str) -> Result<Json> {
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Open> = Vec::new();
    let mut root: Option<(String, Json)> = None;
    let decoder = reader.decoder();
    let attrs_of = |e: &quick_xml::events::BytesStart| -> Map<String, Json> {
        let mut m = Map::new();
        for a in e.attributes().flatten() {
            let key = local(a.key.as_ref());
            if a.key.as_ref().starts_with(b"xmlns") || key == "schemaLocation" {
                continue;
            }
            let v = a.decode_and_unescape_value(decoder).map(|c| c.into_owned()).unwrap_or_default();
            m.insert(key, number_of(&v).unwrap_or(Json::String(v)));
        }
        m
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(Open::new(local(e.name().as_ref()), attrs_of(&e))),
            Ok(Event::Empty(e)) => {
                let name = local(e.name().as_ref());
                let v = close(Open::new(name.clone(), attrs_of(&e)));
                match stack.last_mut() {
                    Some(parent) => parent.child(name, v),
                    None => root = Some((name, v)),
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    let raw = String::from_utf8_lossy(t.as_ref()).into_owned();
                    top.text.push_str(&quick_xml::escape::unescape(&raw).map(|c| c.into_owned()).unwrap_or(raw));
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some(top) = stack.last_mut() {
                    let name = String::from_utf8_lossy(r.as_ref()).into_owned();
                    let c = quick_xml::escape::unescape(&format!("&{};", name)).map(|c| c.into_owned()).unwrap_or_default();
                    top.text.push_str(&c);
                }
            }
            Ok(Event::End(_)) => {
                let Some(e) = stack.pop() else { break };
                let name = e.name.clone();
                let v = close(e);
                match stack.last_mut() {
                    Some(parent) => parent.child(name, v),
                    None => root = Some((name, v)),
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(GbError(format!("The XML could not be read at byte {}: {}", reader.buffer_position(), e))),
        }
    }
    let (name, v) = root.ok_or_else(|| GbError("The XML has no element.".into()))?;
    let mut m = Map::new();
    m.insert(name, v);
    Ok(Json::Object(m))
}

/// Whether a file is XML to read as records: not CityGML or LandXML, which are read as
/// what they are.
pub fn is_plain_xml(file: &std::path::Path) -> bool {
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    ext == "xml" && !crate::citymodel::is_citygml(file) && !crate::landxml::is_landxml(file)
}

/// Writes features as XML: a root element, an element for each feature, and an element for
/// each attribute, names with a dot as elements within elements, and the geometry as WKT.
pub fn write(table: &Table, root: &str, row: &str, geometry: bool) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let tag = |s: &str| -> String {
        let t: String = s.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' }).collect();
        if t.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
            t
        } else {
            format!("_{}", t)
        }
    };
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{}>\n", tag(root));
    for f in &table.features {
        out.push_str(&format!("  <{}>\n", tag(row)));
        // The open elements of the dotted names, so that a.b and a.c share <a>.
        let mut open: Vec<String> = Vec::new();
        for (k, field) in table.fields.iter().enumerate() {
            let parts: Vec<&str> = field.name.split('.').filter(|p| !p.is_empty()).collect();
            let (path, last) = match parts.split_last() {
                Some((last, path)) => (path.to_vec(), *last),
                None => continue,
            };
            let same = open.iter().zip(&path).take_while(|(a, b)| a == b).count();
            while open.len() > same {
                let name = open.pop().unwrap_or_default();
                out.push_str(&format!("{}</{}>\n", "  ".repeat(open.len() + 2), tag(&name)));
            }
            for p in &path[same..] {
                out.push_str(&format!("{}<{}>\n", "  ".repeat(open.len() + 2), tag(p)));
                open.push(p.to_string());
            }
            let v = &f.attrs[k];
            let indent = "  ".repeat(open.len() + 2);
            match v {
                Value::Null => out.push_str(&format!("{}<{}/>\n", indent, tag(last))),
                v => out.push_str(&format!("{}<{}>{}</{}>\n", indent, tag(last), esc(&v.to_text()), tag(last))),
            }
        }
        while let Some(name) = open.pop() {
            out.push_str(&format!("{}</{}>\n", "  ".repeat(open.len() + 2), tag(&name)));
        }
        if geometry {
            if let Some(g) = &f.geom {
                out.push_str(&format!("    <geometry>{}</geometry>\n", esc(&wkt::ToWkt::wkt_string(g))));
            }
        }
        out.push_str(&format!("  </{}>\n", tag(row)));
    }
    out.push_str(&format!("</{}>\n", tag(root)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_becomes_json_and_features_become_xml() {
        let x = r#"<?xml version="1.0"?><reg:Fastigheter xmlns:reg="urn:x"><Fastighet id="5"><Namn>Borås 1:2</Namn><Kod>0123</Kod><Area>40.5</Area><Adress typ="post"><Ort>Borås</Ort></Adress><Tel>1</Tel><Tel>2</Tel></Fastighet><Fastighet id="6"><Namn>A &amp; B</Namn></Fastighet></reg:Fastigheter>"#;
        let v = to_json(x).unwrap();
        let list = &v["Fastigheter"]["Fastighet"];
        assert_eq!(list[0]["id"], 5);
        assert_eq!(list[0]["Kod"], "0123", "a code with a leading zero stays text");
        assert_eq!(list[0]["Area"], 40.5);
        assert_eq!(list[0]["Adress"]["Ort"], "Borås");
        assert_eq!(list[0]["Adress"]["typ"], "post");
        assert_eq!(list[0]["Tel"], serde_json::json!([1, 2]));
        assert_eq!(list[1]["Namn"], "A & B");
        let t = crate::json::rows_to_table(vec![vec![
            ("namn".into(), Value::Text("A<B".into())),
            ("adress.ort".into(), Value::Text("Borås".into())),
            ("adress.nr".into(), Value::Int(3)),
        ]]);
        let w = write(&t, "rows", "row", false);
        assert!(w.contains("<namn>A&lt;B</namn>") && w.contains("<adress>") && w.contains("<nr>3</nr>") && w.matches("<adress>").count() == 1, "{}", w);
        assert!(to_json(&w).is_ok());
    }
}
