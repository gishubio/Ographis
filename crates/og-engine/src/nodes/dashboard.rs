//! Dashboard: a web page of its own with a map of the areas, coloured by a value, and a bar
//! chart of the same values beside it, linked: a bar pointed at lights up its area, and an
//! area its bar. Nothing is fetched; the page opens from a disk or a site.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, GbError};
use geo::{BoundingRect, Geometry, Simplify};

const RAMP_CHOICES: &[(&str, &str)] = &[
    ("blues", "Blues"),
    ("greens", "Greens"),
    ("reds", "Reds"),
    ("oranges", "Oranges"),
    ("purples", "Purples"),
    ("viridis", "Yellow to blue"),
    ("heat", "Yellow to red"),
    ("diverging", "Blue to red"),
];

const DASHBOARD_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "path", label: "Web page", kind: ParamKind::FileSave, default: "", hint: "The .html file to write, such as out/dashboard.html" },
    ParamSpec { key: "name", label: "Names", kind: ParamKind::Attribute("Input"), default: "", hint: "What each area is called, such as the municipality" },
    ParamSpec { key: "value", label: "Values", kind: ParamKind::Attribute("Input"), default: "", hint: "The number the map is coloured by and the bars show" },
    ParamSpec { key: "title", label: "Title", kind: ParamKind::Text, default: "", hint: "Say what the page shows, in a few words" },
    ParamSpec { key: "subtitle", label: "Explanation", kind: ParamKind::Text, default: "", hint: "A line under the title: what is measured, where and when" },
    ParamSpec { key: "note", label: "Note at the foot", kind: ParamKind::Text, default: "", hint: "Such as where the data comes from" },
    ParamSpec { key: "unit", label: "Unit", kind: ParamKind::Text, default: "", hint: "Written after the numbers, such as % or per 1 000" },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "0", hint: "" },
    ParamSpec { key: "format", label: "Numbers as", kind: ParamKind::Choice(super::chart::FORMATS), default: "sv", hint: "" },
    ParamSpec { key: "ramp", label: "Colours", kind: ParamKind::Choice(RAMP_CHOICES), default: "blues", hint: "From the smallest values to the largest" },
    ParamSpec { key: "classes", label: "Classes", kind: ParamKind::Text, default: "5", hint: "Natural breaks (Jenks), from 2 to 9" },
];

/// A ring as an SVG path in the page's coordinates.
fn ring_path(out: &mut String, ring: &geo::LineString<f64>, to: &dyn Fn(f64, f64) -> (f64, f64)) {
    for (i, c) in ring.0.iter().enumerate() {
        let (x, y) = to(c.x, c.y);
        out.push_str(&format!("{}{:.1} {:.1}", if i == 0 { "M" } else { "L" }, x, y));
    }
    out.push('Z');
}

fn svg_path(g: &Geometry<f64>, to: &dyn Fn(f64, f64) -> (f64, f64)) -> String {
    let mut s = String::new();
    let mut poly = |p: &geo::Polygon<f64>| {
        ring_path(&mut s, p.exterior(), to);
        p.interiors().iter().for_each(|r| ring_path(&mut s, r, to));
    };
    match g {
        Geometry::Polygon(p) => poly(p),
        Geometry::MultiPolygon(m) => m.0.iter().for_each(poly),
        Geometry::Rect(r) => poly(&r.to_polygon()),
        _ => {}
    }
    s
}

fn css(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn run_dashboard(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let t = input(inp, "Input");
    let stored = p.need("path", "Say where the web page should be written, such as out/dashboard.html.")?;
    let ni = t.need_field(&p.need("name", "Choose the attribute that names the areas.")?)?;
    let vname = p.need("value", "Choose the values to show.")?;
    let vi = t.need_field(&vname)?;
    let decimals = number_param(p, "decimals", "The decimals")?.unwrap_or(0.0).clamp(0.0, 6.0) as usize;
    let swedish = p.get("format").as_deref() != Some("en");
    let k = number_param(p, "classes", "The classes")?.unwrap_or(5.0).round().clamp(2.0, 9.0) as usize;
    let ramp = p.get("ramp").unwrap_or_else(|| "blues".into());
    // The areas, and the points as small circles: the extent of all, to fit the map.
    let mut ext = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for g in t.features.iter().filter_map(|f| f.geom.as_ref()) {
        if let Some(r) = g.bounding_rect() {
            ext = [ext[0].min(r.min().x), ext[1].min(r.min().y), ext[2].max(r.max().x), ext[3].max(r.max().y)];
        }
    }
    if ext[0] > ext[2] {
        return err("No feature has a geometry to draw on the map.");
    }
    // Degrees are drawn as wide as they are at the middle of the map.
    let squeeze = if t.crs == Some(4326) || (ext[0] >= -180.0 && ext[2] <= 180.0 && ext[1] >= -90.0 && ext[3] <= 90.0 && t.crs.is_none()) {
        ((ext[1] + ext[3]) / 2.0).to_radians().cos().max(0.2)
    } else {
        1.0
    };
    let (gw, gh) = (((ext[2] - ext[0]) * squeeze).max(1e-9), (ext[3] - ext[1]).max(1e-9));
    let (w, h) = if gw >= gh { (1000.0, (1000.0 * gh / gw).max(200.0)) } else { ((1000.0 * gw / gh).max(300.0), 1000.0) };
    let scale = (w / gw).min(h / gh);
    let to = move |x: f64, y: f64| (((x - ext[0]) * squeeze * scale), (ext[3] - y) * scale);
    // Half a pixel at the scale of the page is as fine as the outlines need to be.
    let tol = 0.5 / scale;
    let mut rows = Vec::new();
    let mut values = Vec::new();
    let mut points = 0usize;
    for f in &t.features {
        let v = f.attrs[vi].as_f64().or_else(|| crate::model::parse_number(&f.attrs[vi].to_text())).filter(|v| v.is_finite());
        let name = f.attrs[ni].to_text();
        let (path, dot) = match f.geom.as_ref() {
            Some(Geometry::Point(pt)) => {
                points += 1;
                (String::new(), Some(to(pt.x(), pt.y())))
            }
            Some(Geometry::Polygon(pg)) => (svg_path(&Geometry::Polygon(pg.simplify(tol)), &to), None),
            Some(Geometry::MultiPolygon(m)) => (svg_path(&Geometry::MultiPolygon(m.simplify(tol)), &to), None),
            Some(g) => match g.bounding_rect() {
                Some(r) => {
                    let c = r.center();
                    (String::new(), Some(to(c.x, c.y)))
                }
                None => (String::new(), None),
            },
            None => (String::new(), None),
        };
        if let Some(v) = v {
            values.push(v);
        }
        rows.push(serde_json::json!({
            "n": name,
            "v": v,
            "d": path,
            "p": dot.map(|(x, y)| vec![(x * 10.0).round() / 10.0, (y * 10.0).round() / 10.0]),
        }));
    }
    if values.is_empty() {
        return err(format!("No feature has a number in {}.", vname));
    }
    // Natural breaks for the colours.
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let thinned: Vec<f64> = if sorted.len() > 3000 { (0..3000).map(|i| sorted[i * (sorted.len() - 1) / 2999]).collect() } else { sorted.clone() };
    let mut breaks = super::statistics::jenks(&thinned, k);
    breaks.dedup();
    let n = breaks.len() + 1;
    let colours: Vec<String> =
        (0..n).map(|i| css(crate::thematic::ramp_at(&ramp, if n == 1 { 0.5 } else { 0.08 + 0.92 * i as f64 / (n - 1) as f64 }))).collect();
    let data = serde_json::json!({
        "title": p.exact("title").trim(),
        "subtitle": p.exact("subtitle").trim(),
        "note": p.exact("note").trim(),
        "unit": p.exact("unit").trim(),
        "value": vname,
        "decimals": decimals,
        "swedish": swedish,
        "w": w.round(),
        "h": h.round(),
        "breaks": breaks,
        "lo": sorted[0],
        "hi": sorted[sorted.len() - 1],
        "colours": colours,
        "rows": rows,
    });
    let title = p.exact("title").trim().to_string();
    let page = PAGE.replace("{title}", &html_escape(if title.is_empty() { &vname } else { &title })).replace("{data}", &data.to_string().replace("</", "<\\/"));
    let mut path = ctx.resolve(&stored);
    if path.extension().is_none() {
        path.set_extension("html");
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    std::fs::write(&path, page).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    ctx.notes.info(format!(
        "{} areas{} in {} classes, written to {}",
        fmt_count((t.len() - points) as u64),
        if points > 0 { format!(" and {} points", fmt_count(points as u64)) } else { String::new() },
        n,
        path.display()
    ));
    let missing = t.len() - values.len();
    if missing > 0 {
        ctx.notes.warn(format!("{} features have no number in {}; they are drawn grey and have no bar.", fmt_count(missing as u64), vname));
    }
    ctx.files.push(path);
    Ok(vec![("Output".into(), t.clone())])
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const PAGE: &str = r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root { --paper:#fcfcfb; --ink:#0b0b0b; --second:#52514e; --muted:#898781; --grid:#e1e0d9; --edge:#ffffff; --hi:#0b0b0b; --none:#d9d8d2; }
@media (prefers-color-scheme: dark) { :root { --paper:#161615; --ink:#f2f1ed; --second:#bdbbb2; --muted:#898781; --grid:#33322f; --edge:#161615; --hi:#ffffff; --none:#3a3936; } }
* { box-sizing: border-box; }
body { margin:0; background:var(--paper); color:var(--ink); font:14px/1.45 Inter, system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width:1400px; margin:0 auto; padding:28px; }
h1 { font-size:24px; margin:0 0 4px; font-weight:600; }
.sub { color:var(--second); font-size:15px; margin:0 0 18px; }
.panes { display:grid; grid-template-columns: minmax(0,3fr) minmax(280px,2fr); gap:28px; align-items:start; }
@media (max-width: 860px) { .panes { grid-template-columns: 1fr; } }
#map svg { width:100%; height:auto; max-height:72vh; display:block; }
#map path, #map circle { stroke:var(--edge); stroke-width:0.6; vector-effect:non-scaling-stroke; cursor:pointer; }
#map .on { stroke:var(--hi); stroke-width:2.5; }
.legend { display:flex; flex-wrap:wrap; gap:4px 14px; margin:10px 0 0; color:var(--second); font-size:12.5px; }
.legend span::before { content:""; display:inline-block; width:12px; height:12px; border-radius:2px; margin-right:6px; vertical-align:-1px; background:var(--c); }
.tools { display:flex; gap:6px; margin:0 0 8px; }
.tools button { font:inherit; font-size:12.5px; padding:3px 10px; border:1px solid var(--grid); background:transparent; color:var(--second); border-radius:6px; cursor:pointer; }
.tools button.sel { color:var(--ink); border-color:var(--muted); }
#bars { max-height:75vh; overflow:auto; }
.row { display:grid; grid-template-columns: minmax(90px,38%) 1fr auto; gap:8px; align-items:center; padding:2px 4px; border-radius:4px; cursor:pointer; }
.row:hover, .row.on { background:var(--grid); }
.row .n { color:var(--second); font-size:13px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
.row .b { height:14px; border-radius:0 4px 4px 0; min-width:1px; }
.row .v { font-size:12.5px; font-variant-numeric:tabular-nums; }
#tip { position:fixed; pointer-events:none; background:var(--paper); border:1px solid var(--grid); border-radius:6px; padding:6px 10px; font-size:13px; display:none; box-shadow:0 2px 8px rgba(0,0,0,.12); }
.note { color:var(--muted); font-size:12px; margin-top:20px; }
</style></head><body><main>
<h1 id="title"></h1><p class="sub" id="sub"></p>
<div class="panes"><div><div id="map"></div><div class="legend" id="legend"></div></div>
<div><div class="tools"><button data-o="v" class="sel">Largest first</button><button data-o="u">Smallest first</button><button data-o="n">Alphabetical</button></div><div id="bars"></div></div></div>
<p class="note" id="note"></p></main><div id="tip"></div>
<script>
const D = {data};
const NS = "http://www.w3.org/2000/svg";
const num = v => v.toLocaleString(D.swedish ? "sv-SE" : "en-GB", { minimumFractionDigits: D.decimals, maximumFractionDigits: D.decimals });
const withUnit = v => v == null ? "–" : num(v) + (D.unit ? " " + D.unit : "");
const colourOf = v => { if (v == null) return "var(--none)"; let i = D.breaks.findIndex(b => v <= b); return D.colours[i < 0 ? D.breaks.length : i]; };
document.getElementById("title").textContent = D.title || D.value;
document.getElementById("sub").textContent = D.subtitle;
document.getElementById("note").textContent = D.note;
const svg = document.createElementNS(NS, "svg");
svg.setAttribute("viewBox", "-4 -4 " + (D.w + 8) + " " + (D.h + 8));
const shapes = [], rows = [];
const tip = document.getElementById("tip");
function show(i, on, e) {
  shapes[i] && shapes[i].classList.toggle("on", on);
  rows[i] && rows[i].classList.toggle("on", on);
  if (on && shapes[i]) shapes[i].parentNode.appendChild(shapes[i]);
  if (on && e) { const r = D.rows[i]; tip.innerHTML = "<b></b><br>"; tip.firstChild.textContent = r.n; tip.append(withUnit(r.v)); tip.style.display = "block"; tip.style.left = (e.clientX + 14) + "px"; tip.style.top = (e.clientY + 10) + "px"; }
  if (!on) tip.style.display = "none";
}
D.rows.forEach((r, i) => {
  let s;
  if (r.d) { s = document.createElementNS(NS, "path"); s.setAttribute("d", r.d); s.setAttribute("fill-rule", "evenodd"); }
  else if (r.p) { s = document.createElementNS(NS, "circle"); s.setAttribute("cx", r.p[0]); s.setAttribute("cy", r.p[1]); s.setAttribute("r", 6); }
  else return;
  s.style.fill = colourOf(r.v);
  s.addEventListener("mousemove", e => show(i, true, e));
  s.addEventListener("mouseleave", () => show(i, false));
  s.addEventListener("click", () => rows[i] && rows[i].scrollIntoView({ block: "nearest", behavior: "smooth" }));
  svg.appendChild(s); shapes[i] = s;
});
document.getElementById("map").appendChild(svg);
const legend = document.getElementById("legend");
D.colours.forEach((c, i) => {
  const from = i == 0 ? D.lo : D.breaks[i - 1], to = i == D.breaks.length ? D.hi : D.breaks[i];
  const s = document.createElement("span"); s.style.setProperty("--c", c); s.textContent = num(from) + " – " + withUnit(to); legend.appendChild(s);
});
const bars = document.getElementById("bars");
const most = Math.max(...D.rows.map(r => Math.abs(r.v || 0)), 1e-12);
D.rows.forEach((r, i) => {
  if (r.v == null) return;
  const row = document.createElement("div"); row.className = "row";
  const n = document.createElement("span"); n.className = "n"; n.textContent = r.n;
  const b = document.createElement("span"); b.className = "b"; b.style.width = (Math.abs(r.v) / most * 100) + "%"; b.style.background = colourOf(r.v);
  const v = document.createElement("span"); v.className = "v"; v.textContent = withUnit(r.v);
  row.append(n, b, v);
  row.addEventListener("mouseenter", () => show(i, true));
  row.addEventListener("mouseleave", () => show(i, false));
  rows[i] = row;
});
function order(o) {
  const idx = D.rows.map((r, i) => i).filter(i => rows[i]);
  if (o == "v") idx.sort((a, b) => D.rows[b].v - D.rows[a].v);
  if (o == "u") idx.sort((a, b) => D.rows[a].v - D.rows[b].v);
  if (o == "n") idx.sort((a, b) => D.rows[a].n.localeCompare(D.rows[b].n, D.swedish ? "sv" : "en"));
  idx.forEach(i => bars.appendChild(rows[i]));
  document.querySelectorAll(".tools button").forEach(x => x.classList.toggle("sel", x.dataset.o == o));
}
document.querySelectorAll(".tools button").forEach(x => x.addEventListener("click", () => order(x.dataset.o)));
order("v");
</script></body></html>
"##;

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "DashboardWriter",
    title: "Dashboard web page",
    hidden: false,
    group: Group::Transformer,
    category: "Charts",
    desc: "Writes a web page with a map of the areas, coloured by a value in natural classes, and a bar for each area beside it, the two linked: the pointer on a bar lights up its area, and on an area its bar, with the name and the value. The bars sort by size or by name. Light or dark as the reader's computer is set, and nothing to fetch, so it opens from a disk or on any site. The features pass on unchanged.",
    inputs: &["Input"],
    outputs: &["Output"],
    ports: None,
    params: DASHBOARD_PARAMS,
    run: run_dashboard,
    summary: |p| format!("{} by {} → {}", p.get("value").unwrap_or_default(), p.get("name").unwrap_or_default(), p.get("path").unwrap_or_default()),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, FieldType, Table, Value};

    #[test]
    fn a_page_is_written_with_the_map_and_the_bars() {
        let dir = std::env::temp_dir().join(format!("og-dashboard-{}", std::process::id()));
        let mut t = Table { fields: vec![Field::new("name", FieldType::Text), Field::new("people", FieldType::Real)], crs: Some(3006), ..Default::default() };
        for (i, (n, v)) in [("Borås", 113000.0), ("Skövde", 57000.0), ("Ale", 32000.0)].iter().enumerate() {
            let x = i as f64 * 1000.0;
            let poly = geo::Polygon::new(geo::LineString::from(vec![(x, 0.0), (x + 900.0, 0.0), (x + 900.0, 900.0), (x, 900.0), (x, 0.0)]), vec![]);
            t.features.push(Feature::new(vec![Value::Text(n.to_string()), Value::Real(*v)], Some(Geometry::Polygon(poly))));
        }
        let path = dir.join("d.html");
        let (out, notes) =
            run_for_test("DashboardWriter", &[("path", path.to_str().unwrap()), ("name", "name"), ("value", "people"), ("classes", "3")], vec![("Input", t)])
                .unwrap();
        assert_eq!(out[0].1.len(), 3);
        let page = std::fs::read_to_string(&path).unwrap();
        assert!(page.contains("\"n\":\"Skövde\"") && page.contains("M0.0 "), "{}", &page[..400.min(page.len())]);
        assert!(notes.info.iter().any(|n| n.starts_with("3 areas in 3 classes")), "{:?}", notes.info);
        let _ = std::fs::remove_dir_all(dir);
    }
}
