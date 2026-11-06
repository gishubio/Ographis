//! Web map output (WebMapWriter): the streams connected to Layers as a web map, a folder
//! with index.html, to open in a browser or put on a web server. Leaflet or MapLibre GL
//! draws it, with the layers as Draw map styles them, a background map, a legend that turns
//! layers on and off, and the attributes of a feature when it is clicked. The features go
//! in WGS 84, as script files beside the page, which a browser also reads from the disk; with
//! MapLibre, large layers can go as PMTiles, which then need a web server.

use super::*;
use crate::model::{GbError, Value};
use serde_json::{json, Value as Json};

pub(in crate::nodes) const WEB_LIBRARIES: &[(&str, &str)] =
    &[("leaflet", "Leaflet: small and simple, for some ten thousand features"), ("maplibre", "MapLibre GL: drawn by the graphics card, for large data")];

pub(in crate::nodes) const WEB_DATA: &[(&str, &str)] = &[
    ("auto", "As fits: PMTiles for large layers with MapLibre"),
    ("geojson", "GeoJSON beside the page, opens from the disk"),
    ("pmtiles", "PMTiles, vector tiles: MapLibre, on a web server"),
];

pub(in crate::nodes) const WEBMAP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "Folder",
        kind: ParamKind::FileSave,
        default: "",
        hint: "The page is index.html in it, with the data in data. A name that ends in .html is the page, in the folder it is in",
    },
    ParamSpec { key: "format", label: "Format", kind: ParamKind::Choice(super::OUTPUT_FORMATS), default: "webmap", hint: "" },
    ParamSpec { key: "title", label: "Title", kind: ParamKind::Text, default: "", hint: "Of the page and over the legend. Empty: the name of the folder" },
    ParamSpec { key: "library", label: "Drawn with", kind: ParamKind::Choice(WEB_LIBRARIES), default: "leaflet", hint: "Loaded from unpkg.com when the page opens" },
    ParamSpec {
        key: "layers",
        label: "Layers",
        kind: ParamKind::MapLayers("Layers"),
        default: "",
        hint: "The top row is on top. A layer that is not ticked can be turned on in the legend",
    },
    ParamSpec { key: "basemap", label: "Background map", kind: ParamKind::Choice(QGIS_BASEMAPS), default: "osm", hint: "Tiles from the web under the layers" },
    ParamSpec {
        key: "basemap_own",
        label: "Name or address",
        kind: ParamKind::Text,
        default: "",
        hint: "The name of one of your background maps under Settings, of tiles or WMS, or an address with {z}, {x} and {y}. One that needs a password or a key is left out, so that it is not written into the page",
    },
    ParamSpec { key: "web_data", label: "The features as", kind: ParamKind::Choice(WEB_DATA), default: "auto", hint: "" },
    ParamSpec { key: "popups", label: "Attributes when clicked", kind: ParamKind::Choice(&[("true", "Shown"), ("false", "Not shown")]), default: "true", hint: "A click on a feature shows its attributes" },
    ParamSpec { key: "legend", label: "Legend", kind: ParamKind::Choice(&[("true", "Shown, with the layers to turn on and off"), ("false", "Not shown")]), default: "true", hint: "" },
];

/// Above this many features a layer goes as PMTiles, with MapLibre and the choice As fits.
const MANY: usize = 50_000;
/// Data larger than this, in GeoJSON, is slow to open; a warning says so.
const LARGE_BYTES: usize = 30_000_000;

/// A colour for the browser, or null for none.
fn css(c: Option<[u8; 4]>) -> Json {
    match c {
        Some([r, g, b, a]) if a > 0 => {
            Json::String(if a == 255 { format!("#{:02x}{:02x}{:02x}", r, g, b) } else { format!("rgba({},{},{},{:.3})", r, g, b, a as f64 / 255.0) })
        }
        _ => Json::Null,
    }
}

/// A coordinate in degrees, to about a centimetre.
fn deg(v: f64) -> Json {
    let r = (v * 1e7).round() / 1e7;
    serde_json::Number::from_f64(r).map(Json::Number).unwrap_or(Json::Null)
}

fn geometry(g: &geo::Geometry<f64>) -> Json {
    use geo::Geometry as G;
    let pt = |c: geo::Coord<f64>| json!([deg(c.x), deg(c.y)]);
    let line = |l: &geo::LineString<f64>| Json::Array(l.0.iter().map(|c| pt(*c)).collect());
    let poly = |p: &geo::Polygon<f64>| Json::Array(std::iter::once(p.exterior()).chain(p.interiors()).map(line).collect());
    match g {
        G::Point(p) => json!({"type": "Point", "coordinates": pt(p.0)}),
        G::MultiPoint(m) => json!({"type": "MultiPoint", "coordinates": m.0.iter().map(|p| pt(p.0)).collect::<Vec<_>>()}),
        G::LineString(l) => json!({"type": "LineString", "coordinates": line(l)}),
        G::Line(l) => json!({"type": "LineString", "coordinates": [pt(l.start), pt(l.end)]}),
        G::MultiLineString(m) => json!({"type": "MultiLineString", "coordinates": m.0.iter().map(line).collect::<Vec<_>>()}),
        G::Polygon(p) => json!({"type": "Polygon", "coordinates": poly(p)}),
        G::MultiPolygon(m) => json!({"type": "MultiPolygon", "coordinates": m.0.iter().map(poly).collect::<Vec<_>>()}),
        G::Rect(r) => geometry(&G::Polygon(r.to_polygon())),
        G::Triangle(t) => geometry(&G::Polygon(t.to_polygon())),
        G::GeometryCollection(c) => json!({"type": "GeometryCollection", "geometries": c.0.iter().map(geometry).collect::<Vec<_>>()}),
    }
}

/// A value of an attribute for the page: numbers as numbers, the rest as text.
fn value(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Int(i) => json!(i),
        Value::Real(r) => serde_json::Number::from_f64(*r).map(Json::Number).unwrap_or(Json::Null),
        Value::Bool(b) => json!(b),
        other => Json::String(other.to_text()),
    }
}

/// A name for a file and an id in the page: letters, digits and _.
fn file_name(s: &str) -> String {
    let t: String = s.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    let t = t.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_");
    if t.is_empty() {
        "layer".into()
    } else {
        t
    }
}

/// The page itself, with what it shows in `config`.
fn page(title: &str, library: &str, config: &Json, scripts: &[String], pmtiles: bool) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let (css_link, js) = if library == "maplibre" {
        let mut js = vec!["https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.js".to_string()];
        if pmtiles {
            js.push("https://unpkg.com/pmtiles@3.2.1/dist/pmtiles.js".to_string());
        }
        ("https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.css", js)
    } else {
        ("https://unpkg.com/leaflet@1.9.4/dist/leaflet.css", vec!["https://unpkg.com/leaflet@1.9.4/dist/leaflet.js".to_string()])
    };
    let mut s = String::new();
    s.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    s.push_str(&format!("<title>{}</title>\n<meta name=\"generator\" content=\"Ographis\">\n", esc(title)));
    s.push_str(&format!("<link rel=\"stylesheet\" href=\"{}\">\n", css_link));
    s.push_str(STYLE);
    s.push_str("</head>\n<body>\n<div id=\"map\"></div>\n<div id=\"legend\" hidden></div>\n");
    for src in js {
        s.push_str(&format!("<script src=\"{}\"></script>\n", src));
    }
    s.push_str("<script>\nwindow.OG_DATA = {};\n</script>\n");
    for src in scripts {
        s.push_str(&format!("<script src=\"{}\"></script>\n", esc(src)));
    }
    // The settings go in as JSON; a "</" in a text would end the script, so it is broken up.
    let config_text = serde_json::to_string(config).unwrap_or_default().replace("</", "<\\/");
    s.push_str(&format!("<script>\nconst OG = {};\n", config_text));
    s.push_str(COMMON_JS);
    s.push_str(if library == "maplibre" { MAPLIBRE_JS } else { LEAFLET_JS });
    s.push_str("</script>\n</body>\n</html>\n");
    s
}

const STYLE: &str = r##"<style>
html, body { margin: 0; height: 100%; font: 13px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif; color: #222; }
#map { position: absolute; inset: 0; background: #f2f1ed; }
#legend { position: absolute; top: 10px; right: 10px; z-index: 1000; max-height: calc(100% - 40px); overflow: auto;
  background: rgba(255,255,255,0.95); border-radius: 6px; box-shadow: 0 1px 4px rgba(0,0,0,0.3); padding: 8px 10px; min-width: 160px; max-width: 280px; }
#legend h1 { font-size: 14px; margin: 0 0 6px; }
#legend .layer { margin: 4px 0; }
#legend label { display: flex; align-items: center; gap: 6px; cursor: pointer; font-weight: 600; }
#legend .keys { margin: 2px 0 6px 22px; }
#legend .heading { color: #666; font-size: 12px; }
#legend .key { display: flex; align-items: center; gap: 6px; }
.og-swatch { display: inline-block; width: 14px; height: 14px; box-sizing: border-box; flex: none; }
.og-popup table { border-collapse: collapse; }
.og-popup th { text-align: left; padding: 1px 8px 1px 0; color: #555; font-weight: 600; vertical-align: top; }
.og-popup td { padding: 1px 0; }
.og-label { background: none; border: none; box-shadow: none; padding: 0; white-space: nowrap; }
.og-label::before { display: none; }
@media (max-width: 600px) { #legend { max-width: 60%; } }
</style>
"##;

const COMMON_JS: &str = r##"
// What the page shows, from the settings above: the layers from the bottom up.
function ogEscape(s) {
  return String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}
function ogPopup(props) {
  const rows = Object.keys(props).filter(k => k[0] !== "~").map(k => {
    const v = props[k];
    return "<tr><th>" + ogEscape(k) + "</th><td>" + (v === null || v === undefined ? "" : ogEscape(v)) + "</td></tr>";
  });
  return "<div class=\"og-popup\"><table>" + rows.join("") + "</table></div>";
}
function ogSwatch(look, kind) {
  const fill = look.fill || "transparent", stroke = look.stroke || "transparent";
  if (kind === "line") return "<span class=\"og-swatch\" style=\"height:0;border-top:3px solid " + stroke + "\"></span>";
  const round = kind === "point" ? "border-radius:50%;" : "";
  const size = kind === "point" && look.size ? Math.max(6, Math.min(24, look.size)) : 14;
  return "<span class=\"og-swatch\" style=\"" + round + "width:" + size + "px;height:" + size + "px;background:" + fill + ";border:1.5px solid " + stroke + "\"></span>";
}
function ogLegend(toggle) {
  if (!OG.legend) return;
  const box = document.getElementById("legend");
  let html = OG.title ? "<h1>" + ogEscape(OG.title) + "</h1>" : "";
  // The top layer first, as in the form.
  OG.layers.slice().reverse().forEach(l => {
    html += "<div class=\"layer\"><label><input type=\"checkbox\" data-id=\"" + l.id + "\"" + (l.show ? " checked" : "") + ">";
    html += (l.keys.length ? "" : ogSwatch(l.look, l.kind)) + ogEscape(l.name) + "</label>";
    if (l.keys.length) {
      html += "<div class=\"keys\">" + (l.heading ? "<div class=\"heading\">" + ogEscape(l.heading) + "</div>" : "");
      l.keys.forEach(k => { html += "<div class=\"key\">" + ogSwatch(k, l.kind) + ogEscape(k.text) + "</div>"; });
      html += "</div>";
    }
    html += "</div>";
  });
  box.innerHTML = html;
  box.hidden = false;
  box.querySelectorAll("input").forEach(i => i.addEventListener("change", () => toggle(i.dataset.id, i.checked)));
}
"##;

const LEAFLET_JS: &str = r##"
const map = L.map("map", { preferCanvas: true, zoomSnap: 0.25 });
if (OG.basemap) {
  const b = OG.basemap;
  (b.wms ? L.tileLayer.wms(b.url, { layers: b.layers, format: b.format, transparent: false, attribution: b.attribution })
         : L.tileLayer(b.url, { maxZoom: 19, attribution: b.attribution })).addTo(map);
}
const shown = {};
OG.layers.forEach(l => {
  const data = OG_DATA[l.id];
  if (!data) return;
  const layer = L.geoJSON(data, {
    style: f => {
      const p = f.properties;
      return { color: p["~stroke"] || "#000", opacity: p["~stroke"] ? 1 : 0, weight: p["~width"],
               fillColor: p["~fill"] || "#000", fillOpacity: p["~fill"] ? 1 : 0, fill: l.kind !== "line" };
    },
    pointToLayer: (f, at) => {
      const p = f.properties;
      return L.circleMarker(at, { radius: p["~size"] / 2, color: p["~stroke"] || "#000", opacity: p["~stroke"] ? 1 : 0, weight: p["~width"],
                                  fillColor: p["~fill"] || "#000", fillOpacity: p["~fill"] ? 1 : 0 });
    },
    onEachFeature: (f, item) => {
      if (OG.popups) item.bindPopup(ogPopup(f.properties), { maxWidth: 400 });
      const text = f.properties["~label"];
      if (text) {
        const lb = l.labels;
        const shadow = lb.halo ? "text-shadow:" + [[-1,-1],[1,-1],[-1,1],[1,1],[0,-1],[0,1],[-1,0],[1,0]].map(d => (d[0] * lb.halo_width) + "px " + (d[1] * lb.halo_width) + "px 0 " + lb.halo).join(",") + ";" : "";
        const html = "<span style=\"font-size:" + (f.properties["~label_size"] || lb.size) + "px;color:" + lb.colour + ";" + (lb.bold ? "font-weight:700;" : "") + shadow + "\">" + ogEscape(text) + "</span>";
        item.bindTooltip(html, { permanent: true, direction: lb.center || f.geometry.type !== "Point" ? "center" : "right", className: "og-label", offset: [0, 0] });
      }
    }
  });
  shown[l.id] = layer;
  if (l.show) layer.addTo(map);
});
// Labels that would cover one already shown are hidden, the top layer first, unless the
// layer shows them all.
function ogDeclutter() {
  const taken = [];
  OG.layers.slice().reverse().forEach(l => {
    const layer = shown[l.id];
    if (!layer || !map.hasLayer(layer)) return;
    layer.eachLayer(item => {
      const tip = item.getTooltip && item.getTooltip();
      const el = tip && tip.getElement && tip.getElement();
      if (!el) return;
      el.style.visibility = "";
      if (l.labels.all) return;
      const r = el.getBoundingClientRect();
      if (taken.some(t => r.left < t.right && r.right > t.left && r.top < t.bottom && r.bottom > t.top)) el.style.visibility = "hidden";
      else taken.push(r);
    });
  });
}
map.on("zoomend moveend", ogDeclutter);
if (OG.bounds) map.fitBounds([[OG.bounds[1], OG.bounds[0]], [OG.bounds[3], OG.bounds[2]]], { padding: [20, 20] });
else map.setView([0, 0], 2);
ogDeclutter();
ogLegend((id, on) => { const l = shown[id]; if (!l) return; if (on) l.addTo(map); else map.removeLayer(l); ogDeclutter(); });
"##;

const MAPLIBRE_JS: &str = r##"
if (OG.pmtiles) {
  const protocol = new pmtiles.Protocol();
  maplibregl.addProtocol("pmtiles", protocol.tile);
}
const style = { version: 8, sources: {}, layers: [{ id: "background", type: "background", paint: { "background-color": "#f2f1ed" } }],
                glyphs: "https://protomaps.github.io/basemaps-assets/fonts/{fontstack}/{range}.pbf" };
if (OG.basemap) {
  const b = OG.basemap;
  const url = b.wms
    ? b.url + (b.url.includes("?") ? "&" : "?") + "SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=" + encodeURIComponent(b.format) + "&LAYERS=" + encodeURIComponent(b.layers) + "&BBOX={bbox-epsg-3857}"
    : b.url;
  const tiles = url.includes("{s}") ? ["a", "b", "c"].map(s => url.replace("{s}", s)) : [url];
  style.sources.basemap = { type: "raster", tiles: tiles, tileSize: 256, attribution: b.attribution, maxzoom: 19 };
  style.layers.push({ id: "basemap", type: "raster", source: "basemap" });
}
const map = new maplibregl.Map({ container: "map", style: style, center: [0, 0], zoom: 1 });
map.addControl(new maplibregl.NavigationControl(), "top-left");
const parts = {};
map.on("load", () => {
  OG.layers.forEach(l => {
    const src = "src_" + l.id;
    if (l.pmtiles) map.addSource(src, { type: "vector", url: "pmtiles://" + new URL(l.pmtiles, location.href).href });
    else if (OG_DATA[l.id]) map.addSource(src, { type: "geojson", data: OG_DATA[l.id] });
    else return;
    const on = { visibility: l.show ? "visible" : "none" };
    const common = l.pmtiles ? { source: src, "source-layer": l.tile_layer } : { source: src };
    const ids = [];
    const add = (id, def) => { map.addLayer(Object.assign({ id: id }, common, def, { layout: Object.assign({}, on, def.layout || {}) })); ids.push(id); };
    const area = ["match", ["geometry-type"], ["Polygon", "MultiPolygon"], true, false];
    const line = ["match", ["geometry-type"], ["LineString", "MultiLineString"], true, false];
    const point = ["match", ["geometry-type"], ["Point", "MultiPoint"], true, false];
    const has = k => ["has", k];
    add(l.id + "_fill", { type: "fill", filter: ["all", area, has("~fill")], paint: { "fill-color": ["get", "~fill"] } });
    add(l.id + "_outline", { type: "line", filter: ["all", area, has("~stroke")], paint: { "line-color": ["get", "~stroke"], "line-width": ["get", "~width"] } });
    add(l.id + "_line", { type: "line", filter: ["all", line, has("~stroke")], paint: { "line-color": ["get", "~stroke"], "line-width": ["get", "~width"] },
                          layout: { "line-join": "round", "line-cap": "round" } });
    add(l.id + "_point", { type: "circle", filter: point, paint: {
      "circle-radius": ["/", ["get", "~size"], 2],
      "circle-color": ["coalesce", ["get", "~fill"], "rgba(0,0,0,0)"],
      "circle-stroke-color": ["coalesce", ["get", "~stroke"], "rgba(0,0,0,0)"],
      "circle-stroke-width": ["case", has("~stroke"), ["get", "~width"], 0] } });
    if (l.labels.on) {
      // The labels read from a source of their own, so that if the letters cannot be fetched,
      // the features are still drawn.
      const lsrc = src + "_labels";
      if (l.pmtiles) map.addSource(lsrc, { type: "vector", url: "pmtiles://" + new URL(l.pmtiles, location.href).href });
      else map.addSource(lsrc, { type: "geojson", data: OG_DATA[l.id] });
      add(l.id + "_label", { type: "symbol", filter: has("~label"), source: lsrc,
        layout: { "text-field": ["get", "~label"], "text-size": ["coalesce", ["get", "~label_size"], l.labels.size],
                  "text-font": [l.labels.bold ? "Noto Sans Medium" : "Noto Sans Regular"],
                  "text-allow-overlap": l.labels.all, "text-anchor": l.labels.center ? "center" : "left", "text-offset": l.labels.center ? [0, 0] : [0.6, 0],
                  "symbol-placement": "point" },
        paint: { "text-color": l.labels.colour, "text-halo-color": l.labels.halo || "rgba(0,0,0,0)", "text-halo-width": l.labels.halo ? l.labels.halo_width : 0 } });
    }
    parts[l.id] = ids;
    if (OG.popups) {
      ids.forEach(id => {
        map.on("click", id, e => {
          const f = e.features[0];
          new maplibregl.Popup({ maxWidth: "400px" }).setLngLat(e.lngLat).setHTML(ogPopup(f.properties)).addTo(map);
        });
        map.on("mouseenter", id, () => { map.getCanvas().style.cursor = "pointer"; });
        map.on("mouseleave", id, () => { map.getCanvas().style.cursor = ""; });
      });
    }
  });
  if (OG.bounds) map.fitBounds([[OG.bounds[0], OG.bounds[1]], [OG.bounds[2], OG.bounds[3]]], { padding: 20, duration: 0 });
});
ogLegend((id, on) => (parts[id] || []).forEach(p => map.setLayoutProperty(p, "visibility", on ? "visible" : "none")));
"##;

/// The kind of features a layer has most of, for its key in the legend.
fn kind_of(t: &Table) -> &'static str {
    let mut n = [0usize; 3];
    for f in &t.features {
        if let Some(g) = &f.geom {
            match crate::geom::family(g) {
                crate::geom::Family::Area => n[0] += 1,
                crate::geom::Family::Line => n[1] += 1,
                crate::geom::Family::Point => n[2] += 1,
                crate::geom::Family::Other => {}
            }
        }
    }
    if n[0] >= n[1] && n[0] >= n[2] {
        "area"
    } else if n[1] >= n[2] {
        "line"
    } else {
        "point"
    }
}

fn look_json(l: &crate::thematic::Look) -> Json {
    json!({"fill": css(l.fill), "stroke": css(l.stroke), "size": l.size})
}

pub(in crate::nodes) fn run_webmap(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use super::{arrange_layers, map_layers, reproject, writer_file};
    let parts: Vec<super::Part> = ctx.parts.iter().filter(|x| x.port == "Layers").cloned().collect();
    if parts.is_empty() {
        return err("Connect the streams to Layers. Each connection is a layer of the web map.");
    }
    let stored = p.need("path", "Say where the web map should be written: a folder.")?;
    let target = ctx.resolve(&writer_file(&stored, Some("webmap")));
    let (dir, page_name) = if matches!(ext_of(&target).as_str(), "html" | "htm") {
        (
            target.parent().map(Path::to_path_buf).unwrap_or_default(),
            target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "index.html".into()),
        )
    } else {
        (target.clone(), "index.html".to_string())
    };
    let library = match p.get("library").as_deref() {
        Some("maplibre") => "maplibre",
        _ => "leaflet",
    };
    let data_as = p.get("web_data").unwrap_or_else(|| "auto".into());
    if data_as == "pmtiles" && library != "maplibre" {
        return err("PMTiles are drawn by MapLibre GL. Choose it under Drawn with, or GeoJSON for Leaflet.");
    }
    let title = p.get("title").filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        let base = if page_name == "index.html" { dir.file_name() } else { Path::new(&page_name).file_stem() };
        base.map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Map".into())
    });
    let data_dir = dir.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", data_dir.display(), e)))?;
    let connected: Vec<String> = parts.iter().map(|x| x.from.clone()).collect();
    let rows = arrange_layers(&map_layers(p.values), &connected);
    let mut layers_json = Vec::new();
    let mut scripts = Vec::new();
    let mut files = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut any_pmtiles = false;
    let mut geojson_bytes = 0usize;
    // The bottom layer first: it is added to the map first.
    for row in rows.iter().rev() {
        let Some(part) = parts.iter().find(|x| x.from == row.from) else { continue };
        let name = if row.name.trim().is_empty() { part.label.clone() } else { row.name.trim().to_string() };
        if part.table.image.is_some() || part.table.grid.is_some() {
            ctx.notes.warn(format!("{} is a picture or a raster, which a web map does not take yet; it was left out.", part.label));
            continue;
        }
        if !part.table.features.iter().any(|f| f.geom.is_some()) {
            ctx.notes.info(format!("{} has no geometry and was left out", part.label));
            continue;
        }
        let t = match part.table.crs {
            Some(4326) => (*part.table).clone(),
            Some(_) => reproject(&part.table, 4326, Some(ctx.backend("A web map")?)).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?,
            None => return err(format!("{} has no coordinate system, and a web map needs one to place it. Set it on Input.", part.label)),
        };
        let mut id = file_name(&name);
        let mut k = 2;
        while used.contains(&id) {
            id = format!("{}_{}", file_name(&name), k);
            k += 1;
        }
        used.push(id.clone());
        let theme = crate::thematic::prepare(&t, &row.style, &row.theme).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?;
        let labels = super::super::map::prepare_labels(&t, &row.labels, ctx).map_err(|e| GbError(format!("{}: {}", part.label, e.0)))?;
        let mut label_of: std::collections::HashMap<usize, (String, f32)> = std::collections::HashMap::new();
        if let Some(l) = &labels {
            for it in &l.items {
                label_of.insert(it.feature, (it.text.clone(), it.size));
            }
        }
        let order: Vec<usize> = theme.order.clone().unwrap_or_else(|| (0..t.len()).collect());
        let base = crate::thematic::Look { fill: row.style.fill, stroke: row.style.stroke, size: row.style.size };
        let as_tiles = library == "maplibre" && (data_as == "pmtiles" || (data_as == "auto" && t.len() > MANY));
        let mut features = Vec::with_capacity(t.len());
        let mut styled = t.like();
        styled.fields = t.fields.clone();
        for key in ["~fill", "~stroke", "~width", "~size", "~label", "~label_size"] {
            let ty = if matches!(key, "~width" | "~size" | "~label_size") { crate::model::FieldType::Real } else { crate::model::FieldType::Text };
            styled.fields.push(crate::model::Field::new(key, ty));
        }
        for &i in &order {
            let f = &t.features[i];
            let Some(g) = &f.geom else { continue };
            if let Some(r) = geo::BoundingRect::bounding_rect(g) {
                bounds = [bounds[0].min(r.min().x), bounds[1].min(r.min().y), bounds[2].max(r.max().x), bounds[3].max(r.max().y)];
            }
            let look = theme.looks.as_ref().and_then(|l| l.get(i)).copied().unwrap_or(base);
            let label = label_of.get(&i);
            if as_tiles {
                let mut attrs = f.attrs.clone();
                let text = |c: Json| c.as_str().map(|s| Value::Text(s.to_string())).unwrap_or(Value::Null);
                attrs.push(text(css(look.fill)));
                attrs.push(text(css(look.stroke)));
                attrs.push(Value::Real(row.style.width as f64));
                attrs.push(Value::Real(look.size as f64));
                attrs.push(label.map(|l| Value::Text(l.0.clone())).unwrap_or(Value::Null));
                attrs.push(label.map(|l| Value::Real(l.1 as f64)).unwrap_or(Value::Null));
                styled.features.push(crate::model::Feature { attrs, geom: Some(g.clone()), z: None });
                continue;
            }
            let mut props = serde_json::Map::new();
            for (k, fld) in t.fields.iter().enumerate() {
                props.insert(fld.name.clone(), value(&f.attrs[k]));
            }
            for (k, v) in [("~fill", css(look.fill)), ("~stroke", css(look.stroke))] {
                if !v.is_null() {
                    props.insert(k.into(), v);
                }
            }
            props.insert("~width".into(), json!(row.style.width));
            props.insert("~size".into(), json!(look.size));
            if let Some((text, size)) = label {
                props.insert("~label".into(), json!(text));
                props.insert("~label_size".into(), json!(size));
            }
            features.push(json!({"type": "Feature", "properties": props, "geometry": geometry(g)}));
        }
        let mut layer = json!({
            "id": id,
            "name": name,
            "show": row.show,
            "kind": kind_of(&t),
            "look": look_json(&base),
            "heading": theme.heading,
            "keys": theme.keys.iter().map(|k| { let mut j = look_json(&k.look); j["text"] = json!(k.text); j }).collect::<Vec<_>>(),
            "labels": {
                "on": labels.is_some(),
                "size": row.labels.size,
                "colour": css(Some(row.labels.colour)),
                "halo": css(row.labels.halo),
                "halo_width": row.labels.halo_width,
                "bold": row.labels.bold,
                "all": row.labels.all,
                "center": row.labels.place == "center",
            },
        });
        if as_tiles {
            let file = data_dir.join(format!("{}.pmtiles", id));
            let o = crate::pmtiles_write::PmOpts { min_zoom: 0, max_zoom: 14, layer: id.clone() };
            let (progress, gate) = (ctx.progress, ctx.gate);
            let s = crate::pmtiles_write::write_pmtiles(&styled, &file, &o, &|i, n| progress("zoom levels", i, Some(n)), &|| gate.going())?;
            ctx.notes.info(format!("{}: {} features in {} tiles", name, fmt_count(s.features), fmt_count(s.tiles)));
            layer["pmtiles"] = json!(format!("data/{}.pmtiles", id));
            layer["tile_layer"] = json!(id);
            any_pmtiles = true;
            files.push(file);
        } else {
            let collection = json!({"type": "FeatureCollection", "features": features});
            let text = format!("OG_DATA[{}] = {};\n", serde_json::to_string(&id).unwrap_or_default(), serde_json::to_string(&collection).unwrap_or_default());
            geojson_bytes += text.len();
            let file = data_dir.join(format!("{}.js", id));
            std::fs::write(&file, text).map_err(|e| GbError(format!("Cannot write {}: {}", file.display(), e)))?;
            scripts.push(format!("data/{}.js", id));
            files.push(file);
        }
        layers_json.push(layer);
    }
    if layers_json.is_empty() {
        return err("No layer has anything to put on the web map.");
    }
    if geojson_bytes > LARGE_BYTES {
        ctx.notes.warn(format!(
            "The features are {} MB as GeoJSON, which is slow to open in a browser. MapLibre GL with PMTiles is made for that much.",
            fmt_count((geojson_bytes as u64).div_ceil(1_000_000))
        ));
    }
    let basemap = match p.get("basemap").as_deref() {
        None | Some("none") | Some("") => Json::Null,
        Some(which) => {
            let name = if which == "own" {
                p.need("basemap_own", "Write the name of one of your background maps, or an address with {z}, {x} and {y}.")?
            } else {
                which.to_string()
            };
            let m = crate::basemap::find(&name)?;
            if !m.auth.trim().is_empty() {
                ctx.notes.warn(format!("{} needs a password or a key, which would be written into the page for anyone to read; it was left out.", m.name));
                Json::Null
            } else {
                match m.kind.as_str() {
                    "xyz" => json!({"url": m.url, "attribution": m.attribution}),
                    "wms" => {
                        json!({"wms": true, "url": m.url, "layers": m.layers, "format": if m.format.is_empty() { "image/png".to_string() } else { m.format.clone() }, "attribution": m.attribution})
                    }
                    other => {
                        ctx.notes.warn(format!("{} is a {} service, which the web map does not take; use one of tiles or WMS.", m.name, other.to_uppercase()));
                        Json::Null
                    }
                }
            }
        }
    };
    let config = json!({
        "title": title,
        "layers": layers_json,
        "basemap": basemap,
        "popups": p.get("popups").as_deref() != Some("false"),
        "legend": p.get("legend").as_deref() != Some("false"),
        "bounds": if bounds[0].is_finite() { json!(bounds) } else { Json::Null },
        "pmtiles": any_pmtiles,
    });
    let html = page(&title, library, &config, &scripts, any_pmtiles);
    let page_path = dir.join(&page_name);
    std::fs::write(&page_path, html).map_err(|e| GbError(format!("Cannot write {}: {}", page_path.display(), e)))?;
    let n = layers_json.len();
    ctx.notes.info(format!(
        "{} {} on a web map drawn with {}",
        n,
        if n == 1 { "layer" } else { "layers" },
        if library == "maplibre" { "MapLibre GL" } else { "Leaflet" }
    ));
    if any_pmtiles {
        ctx.notes.info("PMTiles are read part by part over the web: put the folder on a web server, or start one in it, such as python -m http.server");
    }
    ctx.files.push(page_path);
    ctx.files.extend(files);
    Ok(Vec::new())
}
