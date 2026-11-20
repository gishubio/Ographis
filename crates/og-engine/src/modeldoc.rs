//! A model described as one web page, to hand over, review or keep beside it: a picture of
//! the model, what it reads and writes, its variables and connections, and each step in the
//! order the data goes through them, with its comment and settings. Added in 0.96.5.

use crate::model::{GbError, Result};
use crate::nodes::{node_type, Group, ParamKind, P};
use crate::run::{Node, Workspace};
use base64::Engine;
use std::collections::{BTreeSet, HashMap};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A setting as text: a list with commas, anything else as it is written in the model.
fn shown(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) if a.iter().all(|x| x.is_string()) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// A setting that may hold a secret is not written out, whatever it holds.
fn secret(key: &str) -> bool {
    let k = key.to_lowercase();
    ["password", "secret", "token", "api_key", "apikey"].iter().any(|w| k.contains(w))
}

/// The nodes in the order the data goes through them: each after those it takes from, and
/// otherwise from left to right as they stand on the canvas.
fn flow_order(ws: &Workspace) -> Vec<&Node> {
    let mut before: HashMap<&str, BTreeSet<&str>> = ws.nodes.iter().map(|n| (n.id.as_str(), BTreeSet::new())).collect();
    for e in &ws.edges {
        if let Some(b) = before.get_mut(e.to.0.as_str()) {
            b.insert(e.from.0.as_str());
        }
    }
    let mut left: Vec<&Node> = ws.nodes.iter().collect();
    left.sort_by(|a, b| (a.x, a.y).partial_cmp(&(b.x, b.y)).unwrap_or(std::cmp::Ordering::Equal));
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::new();
    while !left.is_empty() {
        // The first that has all it takes from; in a loop, the first that is left.
        let k = left.iter().position(|n| before[n.id.as_str()].iter().all(|b| done.contains(b) || !before.contains_key(b))).unwrap_or(0);
        let n = left.remove(k);
        done.insert(n.id.as_str());
        out.push(n);
    }
    out
}

/// The picture of the model as a PNG in the page itself.
fn picture(ws: &Workspace) -> Option<String> {
    let img = crate::modelpic::draw(ws, 1.5).ok()?;
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &img.rgba,
        img.width as u32,
        img.height as u32,
        image::ExtendedColorType::Rgba8,
    )
    .ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(png))
}

/// The page, as HTML that needs nothing else.
pub fn html(ws: &Workspace, file: Option<&str>) -> String {
    let title = if ws.name.trim().is_empty() { file.unwrap_or("A model").to_string() } else { ws.name.trim().to_string() };
    let mut h = String::new();
    h.push_str(&format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>{}</style>\n</head>\n<body>\n<main>\n",
        esc(&title),
        STYLE
    ));
    h.push_str(&format!("<h1>{}</h1>\n", esc(&title)));
    let mut meta = vec![format!("Ographis {}", env!("CARGO_PKG_VERSION")), chrono::Local::now().format("%Y-%m-%d").to_string()];
    if let Some(f) = file {
        meta.insert(0, esc(f));
    }
    h.push_str(&format!("<p class=\"meta\">{}</p>\n", meta.join(" · ")));
    if !ws.description.trim().is_empty() {
        h.push_str(&format!("<p class=\"lead\">{}</p>\n", esc(ws.description.trim()).replace('\n', "<br>")));
    }
    if let Some(png) = picture(ws) {
        h.push_str(&format!("<figure><img src=\"data:image/png;base64,{}\" alt=\"The model on its canvas\"></figure>\n", png));
    }

    let ty_of = |n: &Node| node_type(&n.kind);
    let order = flow_order(ws);
    let group_of = |n: &Node| ty_of(n).map(|t| t.group);
    // What it reads and what it writes, with where.
    let place = |n: &Node| -> String {
        let p = |k: &str| n.params.get(k).map(shown).filter(|s| !s.trim().is_empty());
        match (p("path"), p("connection"), p("table")) {
            (Some(path), _, _) => path,
            (None, Some(c), Some(t)) => format!("{} · {}", c, t),
            (None, Some(c), None) => c,
            _ => p("url").or_else(|| p("address")).unwrap_or_default(),
        }
    };
    for (heading, group) in [("What it reads", Group::Reader), ("What it writes", Group::Writer)] {
        let nodes: Vec<&&Node> =
            order.iter().filter(|n| matches!((group_of(n), group), (Some(Group::Reader), Group::Reader) | (Some(Group::Writer), Group::Writer))).collect();
        if nodes.is_empty() {
            continue;
        }
        h.push_str(&format!("<h2>{}</h2>\n<table>\n<tr><th>Node</th><th>Tool</th><th>Where</th><th>Comment</th></tr>\n", heading));
        for n in nodes {
            let tool = ty_of(n).map_or(n.kind.as_str(), |t| t.title);
            let off = if n.disabled { " <span class=\"off\">off</span>" } else { "" };
            h.push_str(&format!(
                "<tr><td><a href=\"#{}\">{}</a>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>\n",
                esc(&n.id),
                esc(n.label()),
                off,
                esc(tool),
                esc(&place(n)),
                esc(&n.comment)
            ));
        }
        h.push_str("</table>\n");
    }
    if !ws.variables.is_empty() {
        h.push_str("<h2>Variables</h2>\n<table>\n<tr><th>Name</th><th>Value</th><th></th></tr>\n");
        for v in &ws.variables {
            let ask = if v.ask { "asked for before each run" } else { "" };
            h.push_str(&format!("<tr><td><code>$({})</code></td><td><code>{}</code></td><td>{}</td></tr>\n", esc(&v.name), esc(&v.value), ask));
        }
        h.push_str("</table>\n");
    }
    // The connections are named: what they hold is in the settings of whoever runs it.
    let connections: BTreeSet<String> = ws.nodes.iter().filter_map(|n| n.params.get("connection").map(shown)).filter(|c| !c.trim().is_empty()).collect();
    if !connections.is_empty() {
        h.push_str("<h2>Connections</h2>\n<p>The model names these connections. Their servers, users, passwords and keys are kept in the settings of Ographis on each computer, not in the model.</p>\n<ul>\n");
        for c in connections {
            h.push_str(&format!("<li>{}</li>\n", esc(&c)));
        }
        h.push_str("</ul>\n");
    }

    h.push_str("<h2>Steps</h2>\n<p>In the order the data goes through them.</p>\n");
    for (k, n) in order.iter().enumerate() {
        let ty = ty_of(n);
        let tool = ty.map_or(n.kind.as_str(), |t| t.title);
        let off = if n.disabled { " <span class=\"off\">off</span>" } else { "" };
        h.push_str(&format!("<section id=\"{}\">\n<h3><span class=\"num\">{}</span> {}{}</h3>\n", esc(&n.id), k + 1, esc(n.label()), off));
        let mut line = format!("<span class=\"tool\">{}</span>", esc(tool));
        if let Some(t) = ty {
            let summary = (t.summary)(&P::new(&n.params, t.params));
            if !summary.trim().is_empty() {
                line.push_str(&format!(" · {}", esc(&summary)));
            }
        }
        h.push_str(&format!("<p class=\"meta\">{}</p>\n", line));
        if !n.comment.trim().is_empty() {
            h.push_str(&format!("<p>{}</p>\n", esc(n.comment.trim()).replace('\n', "<br>")));
        }
        let from: Vec<String> = ws
            .edges
            .iter()
            .filter(|e| e.to.0 == n.id)
            .map(|e| format!("{} from <a href=\"#{}\">{}</a> · {}", esc(&e.to.1), esc(&e.from.0), esc(label_of(ws, &e.from.0)), esc(&e.from.1)))
            .collect();
        if !from.is_empty() {
            h.push_str(&format!("<p class=\"wires\">{}</p>\n", from.join("<br>")));
        }
        // The settings that are set, by their labels, choices by what they mean.
        let mut rows = Vec::new();
        for (key, value) in &n.params {
            let text = shown(value);
            if text.trim().is_empty() {
                continue;
            }
            let spec = ty.and_then(|t| t.params.iter().find(|s| s.key == key));
            let label = spec.map_or(key.as_str(), |s| s.label);
            let text = if secret(key) {
                "(not shown)".to_string()
            } else {
                match spec.map(|s| s.kind) {
                    Some(ParamKind::Choice(list)) => list.iter().find(|c| c.0 == text).map_or(text.clone(), |c| c.1.to_string()),
                    _ => text,
                }
            };
            rows.push(format!("<tr><td>{}</td><td><code>{}</code></td></tr>", esc(label), esc(&text)));
        }
        if !rows.is_empty() {
            h.push_str(&format!("<details><summary>Settings ({})</summary>\n<table>\n{}\n</table>\n</details>\n", rows.len(), rows.join("\n")));
        }
        h.push_str("</section>\n");
    }
    h.push_str("</main>\n</body>\n</html>\n");
    h
}

fn label_of<'a>(ws: &'a Workspace, id: &str) -> &'a str {
    ws.nodes.iter().find(|n| n.id == id).map_or("?", |n| n.label())
}

/// Writes the page of a model to a file.
pub fn write(ws: &Workspace, model: Option<&std::path::Path>, path: &std::path::Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let file = model.and_then(|m| m.file_name()).map(|n| n.to_string_lossy().into_owned());
    std::fs::write(path, html(ws, file.as_deref())).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))
}

const STYLE: &str = r#"
:root { --bg: #fbfaf7; --ink: #23221f; --muted: #6b675f; --line: #e2ded5; --accent: #2f6f6a; --panel: #ffffff; }
@media (prefers-color-scheme: dark) { :root { --bg: #1b1c1e; --ink: #e9e6df; --muted: #a29d93; --line: #3a3b3e; --accent: #7cc4bc; --panel: #232427; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--ink); font: 15px/1.55 "Segoe UI", system-ui, -apple-system, sans-serif; }
main { max-width: 960px; margin: 0 auto; padding: 32px 16px 64px; }
h1 { font-size: 28px; margin: 0 0 4px; }
h2 { font-size: 19px; margin: 36px 0 10px; padding-bottom: 6px; border-bottom: 1px solid var(--line); }
h3 { font-size: 16px; margin: 0 0 2px; }
.meta { color: var(--muted); margin: 0 0 10px; font-size: 13px; }
.lead { font-size: 16px; }
figure { margin: 20px 0; background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 8px; overflow-x: auto; }
figure img { display: block; max-width: 100%; height: auto; margin: 0 auto; }
table { border-collapse: collapse; width: 100%; font-size: 14px; display: block; overflow-x: auto; }
th, td { text-align: left; vertical-align: top; padding: 6px 10px; border-bottom: 1px solid var(--line); }
th { color: var(--muted); font-weight: 600; font-size: 12px; text-transform: uppercase; letter-spacing: .03em; }
code { font-family: Consolas, "Cascadia Mono", ui-monospace, monospace; font-size: 13px; word-break: break-word; }
section { background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 12px 16px; margin: 10px 0; }
.num { display: inline-block; min-width: 24px; color: var(--muted); font-weight: 400; }
.tool { color: var(--accent); font-weight: 600; }
.wires { color: var(--muted); font-size: 13px; }
.off { font-size: 11px; color: var(--muted); border: 1px solid var(--line); border-radius: 4px; padding: 0 5px; margin-left: 6px; font-weight: 400; }
details summary { cursor: pointer; color: var(--muted); font-size: 13px; margin-top: 4px; }
a { color: var(--accent); }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_described_as_a_page() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let ws = Workspace::load(&root.join("skaraborg.ographis")).unwrap();
        let page = html(&ws, Some("skaraborg.ographis"));
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("data:image/png;base64,"), "the picture is in the page");
        assert!(page.contains("<h2>What it reads</h2>") && page.contains("<h2>What it writes</h2>") && page.contains("<h2>Steps</h2>"));
        // Every node is a step, after those it takes from.
        assert_eq!(page.matches("<section id=").count(), ws.nodes.len());
        let at = |id: &str| page.find(&format!("<section id=\"{}\"", id)).unwrap();
        for e in &ws.edges {
            assert!(at(&e.from.0) < at(&e.to.0), "{} comes before {}", e.from.0, e.to.0);
        }

        // A password given in a setting is not written out.
        let mut secret = ws.clone();
        secret.nodes[0].params.insert("password".into(), serde_json::json!("hunter2"));
        assert!(!html(&secret, None).contains("hunter2"));
    }
}
