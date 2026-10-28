//! Web APIs, against a server on this computer that answers as the real services do: pages,
//! keys, OAuth2 tokens, busy answers, JSON-stat, geometry as text and OGC services.

use og_engine::settings::{use_settings, Settings};
use og_engine::web::ApiConnection;
use og_engine::{run, Table, Value, Workspace};
use og_gdal::GdalBackend;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gb-api-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn col(t: &Table, name: &str) -> Vec<Value> {
    let i = t.field_index(name).unwrap_or_else(|| panic!("no {} among {:?}", name, t.field_names()));
    t.features.iter().map(|f| f.attrs[i].clone()).collect()
}

/// What a request was: method, path with query, headers in lower case, body.
#[derive(Clone, Debug)]
pub struct Req {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Req {
    fn query(&self, name: &str) -> Option<String> {
        let q = self.path.split_once('?')?.1;
        q.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k == name).then(|| v.to_string())
        })
    }
    fn route(&self) -> &str {
        self.path.split('?').next().unwrap_or("")
    }
}

type Handler = dyn Fn(&Req) -> (u16, Vec<(String, String)>, String) + Send + Sync;

/// A server that answers by `handler`, and keeps every request it got.
fn server(handler: Arc<Handler>) -> (String, Arc<Mutex<Vec<Req>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let kept = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let handler = handler.clone();
            let kept = kept.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    return;
                }
                let mut headers = HashMap::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        headers.insert(k.trim().to_lowercase(), v.trim().to_string());
                    }
                }
                let length = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0usize);
                let mut sent = vec![0u8; length];
                let _ = reader.read_exact(&mut sent);
                let req = Req {
                    method: first.split_whitespace().next().unwrap_or("").to_string(),
                    path: first.split_whitespace().nth(1).unwrap_or("/").to_string(),
                    headers,
                    body: String::from_utf8_lossy(&sent).into_owned(),
                };
                kept.lock().unwrap().push(req.clone());
                let (status, extra, body) = handler(&req);
                let mut s = stream;
                let mut head = format!("HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n", status, body.len());
                if !extra.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                    head.push_str("Content-Type: application/json; charset=utf-8\r\n");
                }
                for (k, v) in extra {
                    head.push_str(&format!("{}: {}\r\n", k, v));
                }
                let _ = write!(s, "{}\r\n{}", head, body);
            });
        }
    });
    (format!("http://{}", addr), log)
}

fn model(nodes: &str, edges: &str) -> Workspace {
    Workspace::from_json(&format!(r#"{{"nodes": [{}], "edges": [{}]}}"#, nodes, edges)).unwrap()
}

#[test]
fn pages_are_followed_by_link_offset_and_number() {
    let dir = tmp("pages");
    let b = GdalBackend::new();
    let (base, log) = server(Arc::new(|r: &Req| {
        let items = |from: usize, n: usize| (from..from + n).map(|i| format!("{{\"id\": {}}}", i)).collect::<Vec<_>>().join(",");
        match r.route() {
            // As Kolada: the address of the next page in the answer.
            "/kolada" => {
                let page: usize = r.query("page").and_then(|p| p.parse().ok()).unwrap_or(1);
                let next = if page < 3 { format!("\"/kolada?page={}\"", page + 1) } else { "null".into() };
                (200, vec![], format!("{{\"count\": 3, \"values\": [{}], \"next_page\": {}}}", items(page * 10, 2), next))
            }
            // An offset and a limit, until a page is not full.
            "/offset" => {
                let off: usize = r.query("offset").unwrap().parse().unwrap();
                let lim: usize = r.query("limit").unwrap().parse().unwrap();
                let n = if off >= 4 { 1 } else { lim };
                (200, vec![], format!("{{\"results\": [{}]}}", items(off, n)))
            }
            // A page number, until a page is empty.
            "/numbered" => {
                let p: usize = r.query("p").unwrap().parse().unwrap();
                (200, vec![], format!("[{}]", if p <= 2 { items(p, 1) } else { String::new() }))
            }
            // The Link header, as GitHub and many others.
            "/linked" => {
                let p: usize = r.query("page").and_then(|p| p.parse().ok()).unwrap_or(1);
                let link = if p < 2 { vec![("Link".to_string(), "</linked?page=2>; rel=\"next\"".to_string())] } else { vec![] };
                (200, link, format!("[{}]", items(p, 1)))
            }
            _ => (404, vec![], "{}".into()),
        }
    }));
    let ws = model(
        &format!(
            r#"{{"id": "kolada", "type": "HttpCaller", "params": {{"url": "{b}/kolada?page=1", "paging": "next", "next_path": "next_page"}}}},
               {{"id": "offset", "type": "HttpCaller", "params": {{"url": "{b}/offset", "paging": "offset", "page_param": "offset", "size_param": "limit", "page_size": "2", "items_path": "results"}}}},
               {{"id": "numbered", "type": "HttpCaller", "params": {{"url": "{b}/numbered", "paging": "page", "page_param": "p"}}}},
               {{"id": "linked", "type": "HttpCaller", "params": {{"url": "{b}/linked", "paging": "next"}}}},
               {{"id": "most", "type": "HttpCaller", "params": {{"url": "{b}/kolada?page=1", "paging": "next", "next_path": "next_page", "max_pages": "2"}}}}"#,
            b = base
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let pages = |id: &str| col(r.output(id, "Output").unwrap(), "_page");
    assert_eq!(pages("kolada"), [Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert!(matches!(&col(r.output("kolada", "Output").unwrap(), "_response")[2], Value::Text(t) if t.contains("\"id\": 30")));
    // 0, 2, 4: the third page holds one, not two, so it is the last.
    assert_eq!(pages("offset").len(), 3);
    assert_eq!(pages("numbered").len(), 3, "the empty page is the third and last");
    assert_eq!(pages("linked").len(), 2);
    assert_eq!(pages("most").len(), 2);
    let paths: Vec<String> = log.lock().unwrap().iter().map(|r| r.path.clone()).collect();
    assert!(paths.contains(&"/offset?offset=4&limit=2".to_string()), "{:?}", paths);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keys_tokens_and_busy_services() {
    let dir = tmp("keys");
    let b = GdalBackend::new();
    let busy = Arc::new(Mutex::new(0));
    let busy2 = busy.clone();
    let tokens = Arc::new(Mutex::new(0));
    let tokens2 = tokens.clone();
    let (base, log) = server(Arc::new(move |r: &Req| match r.route() {
        "/header" if r.headers.get("x-api-key").map(|s| s.as_str()) == Some("s3cret-key") => (200, vec![], "{\"ok\": 1}".into()),
        "/query" if r.query("key").as_deref() == Some("s3cret-key") => (200, vec![], "{\"ok\": 2}".into()),
        // As Trafikverket: the key in the XML that is sent.
        "/xml" if r.body.contains("authenticationkey=\"s3cret-key\"") => (200, vec![], "{\"RESPONSE\": {}}".into()),
        "/token" => {
            let mut n = tokens2.lock().unwrap();
            *n += 1;
            let ok =
                r.headers.get("authorization").map(|s| s.as_str()) == Some("Basic Y2xpZW50OnM3Y3JldA==") && r.body.contains("grant_type=client_credentials");
            if ok {
                (200, vec![], format!("{{\"access_token\": \"tok{}\", \"expires_in\": 3600, \"token_type\": \"Bearer\"}}", n))
            } else {
                (401, vec![], "{\"error\": \"invalid_client\"}".into())
            }
        }
        // The first token has been taken back: the second works.
        "/oauth" => match r.headers.get("authorization").map(|s| s.as_str()) {
            Some("Bearer tok2") => (200, vec![], "{\"ok\": 3}".into()),
            _ => (401, vec![], "{}".into()),
        },
        "/busy" => {
            let mut n = busy2.lock().unwrap();
            *n += 1;
            if *n < 3 {
                (429, vec![("Retry-After".into(), "0".into())], "{}".into())
            } else {
                (200, vec![], "{\"ok\": 4}".into())
            }
        }
        _ => (403, vec![], "{\"error\": \"no key\"}".into()),
    }));
    let api = |name: &str, auth: &str, param: &str| ApiConnection {
        name: name.into(),
        auth: auth.into(),
        param: param.into(),
        secret: "s3cret-key".into(),
        ..Default::default()
    };
    use_settings(Some(Settings {
        apis: vec![
            api("Header", "key_header", ""),
            api("Query", "key_query", "key"),
            api("Text", "key_text", ""),
            ApiConnection {
                name: "Elsewhere".into(),
                auth: "key_header".into(),
                secret: "s3cret-key".into(),
                base_url: "https://api.trafikinfo.trafikverket.se/".into(),
                ..Default::default()
            },
            ApiConnection {
                name: "Lantmäteriet".into(),
                auth: "oauth2".into(),
                user: "client".into(),
                secret: "s7cret".into(),
                token_url: format!("{}/token", base),
                ..Default::default()
            },
        ],
        ..Default::default()
    }));
    let ws = model(
        &format!(
            r#"{{"id": "header", "type": "HttpCaller", "params": {{"url": "{b}/header", "connection": "Header"}}}},
               {{"id": "query", "type": "HttpCaller", "params": {{"url": "{b}/query", "connection": "Query"}}}},
               {{"id": "xml", "type": "HttpCaller", "params": {{"url": "{b}/xml", "method": "POST", "content_type": "text/xml", "connection": "Text",
                  "body": "'<REQUEST><LOGIN authenticationkey=\"{{{{key}}}}\" /></REQUEST>'"}}}},
               {{"id": "elsewhere", "type": "HttpCaller", "params": {{"url": "{b}/header", "connection": "Elsewhere"}}}},
               {{"id": "oauth", "type": "HttpCaller", "params": {{"url": "{b}/oauth", "connection": "Lantmäteriet"}}}},
               {{"id": "busy", "type": "HttpCaller", "params": {{"url": "{b}/busy"}}}},
               {{"id": "nokey", "type": "HttpCaller", "params": {{"url": "{b}/query?key={{{{key}}}}"}}}}"#,
            b = base
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    use_settings(None);
    for (id, n) in &r.nodes {
        if let Some(e) = &n.error {
            eprintln!("{}: {}", id, e);
        }
    }
    let ok = |id: &str| col(r.output(id, "Output").unwrap(), "_status");
    assert_eq!(ok("header"), [Value::Int(200)]);
    assert_eq!(ok("query"), [Value::Int(200)]);
    assert_eq!(ok("xml"), [Value::Int(200)], "{:?}", r.log);
    assert_eq!(ok("oauth"), [Value::Int(200)], "a token that is refused is fetched again");
    assert_eq!(*tokens.lock().unwrap(), 2);
    assert_eq!(ok("busy"), [Value::Int(200)], "a busy service is asked again");
    // The key is not sent to an address it is not for, and is never shown.
    let why = |id: &str| match &col(r.output(id, "Failed").unwrap(), "_error")[0] {
        Value::Text(t) => t.clone(),
        other => panic!("{:?}", other),
    };
    let e = why("elsewhere");
    assert!(e.contains("begin with https://api.trafikinfo") && !e.contains("s3cret"), "{}", e);
    assert!(why("nokey").contains("{{key}} is written"));
    let sent_elsewhere = log.lock().unwrap().iter().filter(|q| q.route() == "/header").count();
    assert_eq!(sent_elsewhere, 1, "only the node with the right connection sent to /header");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn requests_keep_a_pace() {
    let dir = tmp("pace");
    let b = GdalBackend::new();
    let (base, _log) = server(Arc::new(|_r: &Req| (200, vec![], "{}".into())));
    std::fs::write(dir.join("n.csv"), "n\n1\n2\n3\n4\n").unwrap();
    let ws = model(
        &format!(
            r#"{{"id": "in", "type": "Reader", "params": {{"path": "n.csv"}}}},
               {{"id": "paced", "type": "HttpCaller", "params": {{"url": "'{}/x?n=' || n", "per_second": "20"}}}}"#,
            base
        ),
        r#"{"from": ["in", "Output"], "to": ["paced", "Input"]}"#,
    );
    let started = std::time::Instant::now();
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    assert_eq!(r.output("paced", "Output").unwrap().len(), 4);
    assert!(started.elapsed() >= std::time::Duration::from_millis(150), "{:?}", started.elapsed());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A feature as GeoJSON, a point at x, y with an id.
fn point(id: usize, x: f64, y: f64) -> String {
    format!(r#"{{"type": "Feature", "id": {id}, "properties": {{"id": {id}, "name": "n{id}"}}, "geometry": {{"type": "Point", "coordinates": [{x}, {y}]}}}}"#)
}

#[test]
fn ogc_api_features_are_read_page_by_page() {
    let dir = tmp("oapif");
    let b = GdalBackend::new();
    let base = Arc::new(Mutex::new(String::new()));
    let base2 = base.clone();
    let (url, log) = server(Arc::new(move |r: &Req| {
        let b = base2.lock().unwrap().clone();
        let json = |s: String| (200, vec![("Content-Type".to_string(), "application/json".to_string())], s);
        match r.route() {
            "/ogc" | "/ogc/" => json(format!(
                r#"{{"title": "Test", "links": [{{"rel": "conformance", "href": "{b}/ogc/conformance", "type": "application/json"}}, {{"rel": "data", "href": "{b}/ogc/collections", "type": "application/json"}}]}}"#
            )),
            "/ogc/conformance" => json(r#"{"conformsTo": ["http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/core", "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/geojson"]}"#.into()),
            "/ogc/collections" => json(format!(
                r#"{{"collections": [{{"id": "vardcentraler", "title": "Vårdcentraler", "links": [{{"rel": "items", "href": "{b}/ogc/collections/vardcentraler/items", "type": "application/geo+json"}}]}},
                   {{"id": "sjukhus", "title": "Sjukhus", "links": [{{"rel": "items", "href": "{b}/ogc/collections/sjukhus/items", "type": "application/geo+json"}}]}}]}}"#
            )),
            "/ogc/collections/vardcentraler" => json(format!(
                r#"{{"id": "vardcentraler", "links": [{{"rel": "items", "href": "{b}/ogc/collections/vardcentraler/items", "type": "application/geo+json"}}]}}"#
            )),
            "/ogc/collections/vardcentraler/items" => {
                let second = r.query("page").as_deref() == Some("2");
                let feats = if second { point(3, 12.9, 57.7) } else { format!("{}, {}", point(1, 12.0, 57.7), point(2, 13.0, 58.4)) };
                let next = if second { String::new() } else { format!(r#", {{"rel": "next", "href": "{b}/ogc/collections/vardcentraler/items?page=2", "type": "application/geo+json"}}"#) };
                (
                    200,
                    vec![("Content-Type".to_string(), "application/geo+json".to_string())],
                    format!(r#"{{"type": "FeatureCollection", "features": [{feats}], "links": [{{"rel": "self", "href": "x"}}{next}]}}"#),
                )
            }
            _ => (404, vec![], "{}".into()),
        }
    }));
    *base.lock().unwrap() = url.clone();
    let ws = model(
        &format!(
            r#"{{"id": "all", "type": "Reader", "params": {{"path": "{u}/ogc/collections/vardcentraler/items"}}}},
               {{"id": "landing", "type": "Reader", "params": {{"path": "{u}/ogc", "web_as": "ogcapi", "layer": "vardcentraler"}}}}"#,
            u = url
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?} {:?}", r.log, r.nodes.iter().filter_map(|(k, n)| n.error.clone().map(|e| (k.clone(), e))).collect::<Vec<_>>());
    let t = r.output("all", "Output").unwrap();
    assert_eq!(t.len(), 3, "both pages");
    assert_eq!(t.crs, Some(4326));
    assert_eq!(r.output("landing", "Output").unwrap().len(), 3);
    // The layers of the service, as the program lists them.
    use og_engine::io::Backend;
    let names: Vec<String> = b.layers(std::path::Path::new(&format!("OAPIF:{}/ogc", url))).unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["vardcentraler", "sjukhus"]);
    assert!(log.lock().unwrap().iter().any(|q| q.path.contains("page=2")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn arcgis_feature_services_are_read_layer_by_layer() {
    let dir = tmp("arcgis");
    let b = GdalBackend::new();
    let (url, log) = server(Arc::new(|r: &Req| {
        let esri = |id: usize, x: f64| {
            format!(
                r#"{{"attributes": {{"OBJECTID": {id}, "Namn": "Skog {id}"}}, "geometry": {{"rings": [[[{x},6400000],[{x},6400100],[{},6400100],[{},6400000],[{x},6400000]]]}}}}"#,
                x + 100.0,
                x + 100.0
            )
        };
        match r.route() {
            "/arcgis/rest/services/Skog/FeatureServer" => (
                200,
                vec![],
                r#"{"layers": [{"id": 0, "name": "Nyckelbiotoper", "geometryType": "esriGeometryPolygon"}, {"id": 2, "name": "Sumpskog", "geometryType": "esriGeometryPolygon"}],
                    "tables": [], "spatialReference": {"wkid": 3006}}"#
                    .into(),
            ),
            "/arcgis/rest/services/Skog/FeatureServer/2/query" => {
                let offset: usize = r.query("resultOffset").and_then(|o| o.parse().ok()).unwrap_or(0);
                let (feats, more) = if offset == 0 { (format!("{}, {}", esri(1, 300000.0), esri(2, 301000.0)), true) } else { (esri(3, 302000.0), false) };
                (
                    200,
                    vec![],
                    format!(
                        r#"{{"objectIdFieldName": "OBJECTID", "geometryType": "esriGeometryPolygon", "spatialReference": {{"wkid": 3006}},
                           "fields": [{{"name": "OBJECTID", "type": "esriFieldTypeOID"}}, {{"name": "Namn", "type": "esriFieldTypeString", "length": 50}}],
                           "features": [{feats}], "exceededTransferLimit": {more}}}"#
                    ),
                )
            }
            _ => (404, vec![], "{}".into()),
        }
    }));
    let ws = model(
        &format!(
            r#"{{"id": "byname", "type": "Reader", "params": {{"path": "{u}/arcgis/rest/services/Skog/FeatureServer", "layer": "Sumpskog"}}}},
               {{"id": "inurl", "type": "Reader", "params": {{"path": "{u}/arcgis/rest/services/Skog/FeatureServer/2"}}}}"#,
            u = url
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    let errors: Vec<_> = r.nodes.iter().filter_map(|(k, n)| n.error.clone().map(|e| (k.clone(), e))).collect();
    assert_eq!(r.errors, 0, "{:?}", errors);
    let t = r.output("byname", "Output").unwrap();
    assert_eq!(t.crs, Some(3006));
    let paths: Vec<String> = log.lock().unwrap().iter().map(|q| q.path.clone()).collect();
    assert_eq!(t.len(), 3, "both pages: {:?}", paths);
    assert!(col(t, "Namn").contains(&Value::Text("Skog 3".into())));
    assert_eq!(r.output("inurl", "Output").unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn statistics_geometry_as_text_time_series_and_stac() {
    let dir = tmp("mixed");
    let b = GdalBackend::new();
    let (url, log) = server(Arc::new(|r: &Req| match r.route() {
        // As SCB with json-stat2.
        "/scb" => (
            200,
            vec![],
            r#"{"class": "dataset", "version": "2.0", "id": ["Region", "Tid"], "size": [2, 2],
                "dimension": {"Region": {"category": {"index": {"1490": 0, "1480": 1}, "label": {"1490": "Borås", "1480": "Göteborg"}}},
                              "Tid": {"category": {"index": {"2024": 0, "2025": 1}}}},
                "value": [114000, 115000, 600000, 604000]}"#
                .into(),
        ),
        // As Trafikverket: geometry as WKT in an attribute.
        "/trv" => (
            200,
            vec![],
            r#"{"RESPONSE": {"RESULT": [{"Situation": [
                {"Id": "a", "Geometry": {"SWEREF99TM": "POINT (319500 6399800)"}},
                {"Id": "b", "Geometry": {"SWEREF99TM": "LINESTRING (319000 6399000, 320000 6400000)"}}]}]}}"#
                .into(),
        ),
        // As Open-Meteo: a time series as lists side by side.
        "/meteo" => {
            (200, vec![], r#"{"hourly": {"time": ["2026-10-17T00:00", "2026-10-17T01:00", "2026-10-17T02:00"], "temperature_2m": [7.1, 6.8, 6.5]}}"#.into())
        }
        "/stac/search" => {
            let second = r.query("token").is_some();
            let item = |id: &str, x: f64| {
                format!(
                    r#"{{"type": "Feature", "id": "{id}", "collection": "sentinel-2-l2a", "geometry": {{"type": "Polygon", "coordinates": [[[{x},57],[{},57],[{},58],[{x},58],[{x},57]]]}},
                       "properties": {{"datetime": "2026-06-0{}T10:20:00Z", "eo:cloud_cover": 12.5}}, "assets": {{"visual": {{"href": "https://data.example/{id}/TCI.tif"}}}}}}"#,
                    x + 1.0,
                    x + 1.0,
                    if second { 2 } else { 1 }
                )
            };
            let next = if second { String::new() } else { r#", {"rel": "next", "href": "/stac/search?token=abc"}"#.to_string() };
            let feats = if second { item("S2B_2", 12.0) } else { format!("{}, {}", item("S2A_1", 11.0), item("S2A_2", 12.0)) };
            (200, vec![], format!(r#"{{"type": "FeatureCollection", "features": [{feats}], "links": [{{"rel": "self", "href": "x"}}{next}]}}"#))
        }
        _ => (404, vec![], "{}".into()),
    }));
    let ws = model(
        &format!(
            r#"{{"id": "scb", "type": "HttpCaller", "params": {{"url": "{u}/scb"}}}},
               {{"id": "stat", "type": "JsonStatReader", "params": {{"attribute": "_response"}}}},
               {{"id": "wide", "type": "JsonStatReader", "params": {{"attribute": "_response", "columns_from": "Tid", "labels": "false"}}}},
               {{"id": "file", "type": "Reader", "params": {{"path": "{u}/scb"}}}},
               {{"id": "trv", "type": "HttpCaller", "params": {{"url": "{u}/trv"}}}},
               {{"id": "situations", "type": "JsonExploder", "params": {{"attribute": "_response", "path": "RESPONSE.RESULT[0].Situation"}}}},
               {{"id": "geom", "type": "GeometryFromText", "params": {{"attribute": "Geometry.SWEREF99TM", "crs": "EPSG:3006"}}}},
               {{"id": "meteo", "type": "HttpCaller", "params": {{"url": "{u}/meteo"}}}},
               {{"id": "series", "type": "JsonExploder", "params": {{"attribute": "_response", "path": "hourly.time, hourly.temperature_2m"}}}},
               {{"id": "when", "type": "AttributeCreator", "params": {{"name": "kl", "expression": "format_time(time, '%H:%M')"}}}},
               {{"id": "stac", "type": "StacSearch", "params": {{"url": "{u}/stac", "collections": "sentinel-2-l2a", "bbox": "11.8, 57.6, 12.1, 57.8", "datetime": "2026-06-01/2026-06-30"}}}},
               {{"id": "lines", "type": "PointsToLines", "params": {{"group": "collection", "order": "datetime"}}}},
               {{"id": "centres", "type": "CenterPointReplacer", "params": {{}}}}"#,
            u = url
        ),
        r#"{"from": ["scb", "Output"], "to": ["stat", "Input"]}, {"from": ["scb", "Output"], "to": ["wide", "Input"]},
           {"from": ["trv", "Output"], "to": ["situations", "Input"]}, {"from": ["situations", "Exploded"], "to": ["geom", "Input"]},
           {"from": ["meteo", "Output"], "to": ["series", "Input"]}, {"from": ["series", "Exploded"], "to": ["when", "Input"]},
           {"from": ["stac", "Items"], "to": ["centres", "Input"]}, {"from": ["centres", "Point"], "to": ["lines", "Input"]}"#,
    );
    let r = run(&ws, Some(&b), &dir);
    let errors: Vec<_> = r.nodes.iter().filter_map(|(k, n)| n.error.clone().map(|e| (k.clone(), e))).collect();
    assert!(errors.is_empty(), "{:?}", errors);
    let stat = r.output("stat", "Output").unwrap();
    assert_eq!(stat.len(), 4);
    assert_eq!(col(stat, "Region_label")[3], Value::Text("Göteborg".into()));
    assert_eq!(col(stat, "value")[3], Value::Int(604000));
    let wide = r.output("wide", "Output").unwrap();
    assert_eq!(wide.len(), 2);
    assert_eq!(col(wide, "2025"), [Value::Int(115000), Value::Int(604000)]);
    assert_eq!(col(wide, "Region"), [Value::Text("1490".into()), Value::Text("1480".into())], "codes stay text");
    // Input reads JSON-stat from an address as a table too.
    assert_eq!(r.output("file", "Output").unwrap().len(), 4);
    let g = r.output("geom", "Output").unwrap();
    assert_eq!(g.len(), 2);
    assert_eq!(g.crs, Some(3006));
    assert!(matches!(g.features[1].geom, Some(geo::Geometry::LineString(_))));
    let series = r.output("when", "Output").unwrap();
    assert_eq!(col(series, "temperature_2m"), [Value::Real(7.1), Value::Real(6.8), Value::Real(6.5)]);
    assert_eq!(col(series, "kl")[1], Value::Text("01:00".into()));
    let items = r.output("stac", "Items").unwrap();
    assert_eq!(items.len(), 3, "both pages");
    assert_eq!(col(items, "asset_visual")[0], Value::Text("https://data.example/S2A_1/TCI.tif".into()));
    assert_eq!(col(items, "cloud_cover")[0], Value::Real(12.5));
    let first = log.lock().unwrap().iter().find(|q| q.route() == "/stac/search").unwrap().path.clone();
    assert!(
        first.contains("bbox=11.8,57.6,12.1,57.8") && first.contains("collections=sentinel-2-l2a") && first.contains("datetime=2026-06-01%2F2026-06-30"),
        "{}",
        first
    );
    let lines = r.output("lines", "Output").unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(col(lines, "points"), [Value::Int(3)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_coordinate_system_of_proj_can_be_chosen() {
    let all = og_gdal::crs_list();
    assert!(all.len() > 4000, "{}", all.len());
    let find = |c: u32| all.iter().find(|e| e.code == c).unwrap_or_else(|| panic!("no EPSG:{}", c));
    assert_eq!(find(3006).name, "SWEREF99 TM");
    assert_eq!(find(3006).kind, "projected");
    assert_eq!(find(4326).kind, "geographic");
    // SWEREF 99 TM with heights in RH 2000.
    assert_eq!(find(5845).kind, "compound");
    assert!(find(3007).area.contains("Sweden"), "{}", find(3007).area);
}

#[test]
fn the_kolada_example_works_with_api_v3() {
    let dir = tmp("kolada3");
    let b = GdalBackend::new();
    let (url, _log) = server(Arc::new(|r: &Req| {
        match r.route() {
        "/v3/municipality" => {
            let page2 = r.query("page").as_deref() == Some("2");
            let next = if page2 { "null".to_string() } else { "\"/v3/municipality?region_type=municipality&page=2\"".to_string() };
            let vals = if page2 { r#"{"id": "1490", "title": "Borås", "type": "K"}"# } else { r#"{"id": "1480", "title": "Göteborg", "type": "K"}, {"id": "0180", "title": "Stockholm", "type": "K"}"# };
            (200, vec![], format!(r#"{{"values": [{vals}], "count": 3, "next_url": {next}, "previous_url": null}}"#))
        }
        "/v3/data/kpi/N01951/year/2025" => (
            200,
            vec![],
            r#"{"values": [
                {"kpi": "N01951", "period": 2025, "municipality": "1480", "values": [{"gender": "K", "count": 1, "status": null, "value": 305000, "isdeleted": false}, {"gender": "T", "count": 1, "status": null, "value": 604000, "isdeleted": false}]},
                {"kpi": "N01951", "period": 2025, "municipality": "1490", "values": [{"gender": "T", "count": 1, "status": null, "value": 115000, "isdeleted": false}]},
                {"kpi": "N01951", "period": 2025, "municipality": "0180", "values": [{"gender": "T", "count": 1, "status": null, "value": 990000, "isdeleted": false}]}],
                "count": 3, "next_url": null, "previous_url": null}"#
                .into(),
        ),
        _ => (404, vec![], r#"{"error": "This endpoint is deprecated. Please use /v3 instead."}"#.into()),
    }
    }));
    let model = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apis/kolada_chart.ographis"))
        .unwrap()
        .replace("https://api.kolada.se", &url);
    let ws = Workspace::from_json(&model).unwrap();
    let r = run(&ws, Some(&b), &dir);
    let errors: Vec<_> = r.nodes.iter().filter_map(|(k, n)| n.error.clone().map(|e| (k.clone(), e))).collect();
    assert!(errors.is_empty(), "{:?}", errors);
    let vg = r.output("vg", "Passed").unwrap();
    assert_eq!(col(vg, "m.title"), [Value::Text("Göteborg".into()), Value::Text("Borås".into())]);
    assert_eq!(col(vg, "value"), [Value::Int(604000), Value::Int(115000)]);
    assert!(dir.join("out/kolada_N01951_2025.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server of the files in a folder, as they are, that keeps the addresses it was asked for.
fn file_server(dir: PathBuf) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let kept = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (dir, kept) = (dir.clone(), kept.clone());
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    return;
                }
                let mut range: Option<(usize, Option<usize>)> = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    // Parts of a file, as GDAL asks for them: Range: bytes=0-16383.
                    if let Some(r) = line.to_lowercase().strip_prefix("range: bytes=").map(|r| r.trim().to_string()) {
                        if let Some((a, b)) = r.split_once('-') {
                            range = a.parse().ok().map(|a| (a, b.parse().ok()));
                        }
                    }
                }
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                kept.lock().unwrap().push(path.clone());
                let file = path.split('?').next().unwrap().trim_start_matches("/tiles/");
                let mut s = stream;
                let head = first.starts_with("HEAD");
                match std::fs::read(dir.join(file)) {
                    Ok(body) if range.is_some() => {
                        let (a, b) = range.unwrap();
                        let end = b.map_or(body.len() - 1, |b| b.min(body.len() - 1));
                        let part = &body[a.min(body.len())..=end];
                        let _ = write!(
                            s,
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            part.len(),
                            a,
                            end,
                            body.len()
                        );
                        let _ = s.write_all(part);
                    }
                    Ok(body) if head => {
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n", body.len());
                    }
                    Ok(body) => {
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(&body);
                    }
                    Err(_) => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            });
        }
    });
    (format!("http://{}", addr), log)
}

#[test]
fn a_3d_tiles_tileset_is_read_from_the_web_and_placed_on_the_map() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tiles");
    let (base, asked) = file_server(examples);
    let dir = tmp("tiles");
    let ws = model(
        &format!(
            r#"{{"id": "web", "type": "Reader", "params": {{"path": "{base}/tiles/tileset.json?key=abc", "tiles_detail": "finest", "tiles_crs": "EPSG:3006", "tiles_dz": "-32"}}}},
            {{"id": "near", "type": "Reader", "params": {{"path": "{base}/tiles/tileset.json", "tiles_crs": "EPSG:3006",
               "within": "rectangle", "min_x": "418262", "min_y": "6472420", "max_x": "418275", "max_y": "6472435"}}}}"#
        ),
        "",
    );
    let r = run(&ws, Some(&GdalBackend::new()), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert_eq!(r.errors, 0);
    let t = r.output("web", "Output").unwrap();
    assert_eq!(t.crs, Some(3006));
    // Each house where it was put, in SWEREF 99 TM, with its walls on the ground.
    let m = t.mesh.as_ref().unwrap();
    for (o, (e, n, ground)) in [(418232.0, 6472462.0, 86.8), (418250.0, 6472445.0, 85.5), (418268.0, 6472428.0, 84.9)].into_iter().enumerate() {
        let corners: Vec<[f64; 3]> = m.tris.iter().filter(|t| t.object == o as u32).flat_map(|t| t.v.map(|i| m.positions[i as usize])).collect();
        let (lo, hi) = corners.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        });
        assert!(lo[0] < e && e < hi[0] && lo[1] < n && n < hi[1] && hi[0] - lo[0] < 20.0, "house {}: {:?} {:?}", o + 1, lo, hi);
        assert!((lo[2] - (ground - 0.4)).abs() < 0.05, "house {}: {:?}", o + 1, lo);
    }
    // The key of the tileset's address goes along to its tiles.
    let asked = asked.lock().unwrap().clone();
    assert!(asked.iter().any(|a| a == "/tiles/house_2.b3dm?key=abc"), "{:?}", asked);
    let near = r.output("near", "Output").unwrap();
    assert_eq!(col(near, "name"), [Value::Text("Björkhagen".into())]);
}

#[test]
fn a_piece_of_openstreetmap_is_fetched_from_an_overpass_server() {
    let dir = tmp("osm");
    let b = GdalBackend::new();
    let (base, log) = server(Arc::new(|r: &Req| {
        assert_eq!(r.method, "POST");
        (
            200,
            vec![],
            r#"{"elements": [
                {"type": "way", "id": 10, "tags": {"building": "yes", "name": "Huset"}, "geometry": [{"lat": 57.70, "lon": 11.97}, {"lat": 57.70, "lon": 11.971}, {"lat": 57.701, "lon": 11.971}, {"lat": 57.70, "lon": 11.97}]},
                {"type": "node", "id": 1, "lat": 57.7005, "lon": 11.9705, "tags": {"building": "yes"}}
            ]}"#
            .into(),
        )
    }));
    // A rectangle of some 120 km², more than allowed.
    let ws = model(
        &format!(r#"{{"id": "osm", "type": "OsmDownloader", "params": {{"bbox": "11.8, 57.65, 12.1, 57.75", "server": "{}/api/interpreter"}}}}"#, base),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let t = r.output("osm", "Output").unwrap();
    assert_eq!(t.crs, Some(3006));
    assert_eq!(t.len(), 2);
    assert_eq!(col(t, "name")[0], Value::Text("Huset".into()));
    assert!(r.nodes["osm"].warnings.iter().any(|w| w.contains("made smaller")), "{:?}", r.nodes["osm"].warnings);
    let body = log.lock().unwrap()[0].body.clone();
    let query = body
        .trim_start_matches("data=")
        .replace("%22", "\"")
        .replace("%5B", "[")
        .replace("%5D", "]")
        .replace("%28", "(")
        .replace("%29", ")")
        .replace("%2C", ",")
        .replace("%3B", ";")
        .replace("%3A", ":");
    assert!(query.contains("nwr[\"building\"](57.6"), "{}", query);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn coordinates_in_attributes_are_reprojected_without_geometry() {
    let dir = tmp("xy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("places.csv"), "name;lat;lon\nGöteborg;57.7089;11.9746\nIngen;;\n").unwrap();
    let b = GdalBackend::new();
    let ws = model(
        &format!(
            r#"{{"id": "in", "type": "Reader", "params": {{"path": "{}"}}}},
               {{"id": "xy", "type": "CoordinateReprojector", "params": {{"x": "lon", "y": "lat", "x_out": "e", "y_out": "n", "decimals": "0"}}}}"#,
            dir.join("places.csv").to_string_lossy().replace('\\', "/")
        ),
        r#"{"from": ["in", "Output"], "to": ["xy", "Input"]}"#,
    );
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let t = r.output("xy", "Output").unwrap();
    assert!(matches!(col(t, "e")[0], Value::Real(e) if (e - 319_500.0).abs() < 1_000.0), "{:?}", col(t, "e"));
    assert!(matches!(col(t, "n")[0], Value::Real(n) if (n - 6_400_000.0).abs() < 5_000.0), "{:?}", col(t, "n"));
    assert_eq!(col(t, "e")[1], Value::Null);
    assert!(r.nodes["xy"].warnings.iter().any(|w| w.contains("1 features")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The example for OpenTripPlanner 2.4, against a server that answers as its REST API does:
/// stops, a planned trip with encoded lines, the departures of a stop, and the stops of each
/// trip. A departure every half hour from 06:00; every other one goes on to Chalmers.
#[test]
fn the_opentripplanner_example_plans_lists_departures_and_counts() {
    let dir = tmp("otp");
    let b = GdalBackend::new();
    const DAY: i64 = 1793142000; // 2026-10-28 at midnight, +01:00
    let (url, log) = server(Arc::new(|r: &Req| {
        let base = "/otp/routers/default";
        let route = r.route().strip_prefix(base).unwrap_or("").to_string();
        let stops = r#"[{"id":"1:A","code":"A","name":"Brunnsparken","lat":57.7069,"lon":11.968},
            {"id":"1:B","code":"B","name":"Chalmers","lat":57.6898,"lon":11.9734},
            {"id":"1:C","code":"C","name":"Korsvägen","lat":57.6969,"lon":11.9868}]"#;
        let body = if route == "/index/stops" {
            stops.to_string()
        } else if route == "/plan" {
            let leg = |mode: &str, s: i64, e: i64| {
                format!(
                    r#"{{"mode":"{mode}","routeShortName":"6","startTime":{s},"endTime":{e},"distance":3000,"legGeometry":{{"points":"c{{e_J_o`hAn}}@otBjk@vrA","length":3}}}}"#
                )
            };
            let s = (DAY + 8 * 3600) * 1000;
            let its: Vec<String> = (0..3)
                .map(|k| {
                    let s = s + k * 600_000;
                    format!(
                        r#"{{"duration":1200,"startTime":{s},"endTime":{},"transfers":0,"legs":[{},{}]}}"#,
                        s + 1_200_000,
                        leg("WALK", s, s + 180_000),
                        leg("TRAM", s + 180_000, s + 1_200_000)
                    )
                })
                .collect();
            format!(r#"{{"plan":{{"itineraries":[{}]}}}}"#, its.join(","))
        } else if route == "/index/stops/1:A/stoptimes" {
            assert_eq!(r.query("startTime").as_deref(), Some(DAY.to_string().as_str()));
            let times: Vec<String> = (0..40)
                .map(|k| {
                    let dep = 6 * 3600 + k * 1800;
                    format!(r#"{{"stopId":"1:A","stopIndex":0,"scheduledDeparture":{dep},"realtimeDeparture":{},"serviceDay":{DAY},"tripId":"1:T{k}","headsign":"x"}}"#, dep + 60)
                })
                .collect();
            format!(r#"[{{"pattern":{{"id":"1:P1","desc":"6"}},"times":[{}]}}]"#, times.join(","))
        } else if let Some(trip) = route.strip_prefix("/index/trips/1:T").and_then(|t| t.strip_suffix("/stoptimes")) {
            let k: i64 = trip.parse().unwrap();
            let dep = 6 * 3600 + k * 1800;
            let along = if k % 2 == 0 { vec!["1:A", "1:C", "1:B"] } else { vec!["1:A", "1:C"] };
            let st: Vec<String> =
                along.iter().enumerate().map(|(i, s)| format!(r#"{{"stopId":"{s}","stopIndex":{i},"scheduledArrival":{}}}"#, dep + 360 * i as i64)).collect();
            format!("[{}]", st.join(","))
        } else {
            return (404, vec![], String::new());
        };
        (200, vec![], body)
    }));
    let model = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apis/otp_routes_and_departures.ographis"))
        .unwrap()
        .replace("http://localhost:8080", &url);
    let ws = Workspace::from_json(&model).unwrap();
    let r = run(&ws, Some(&b), &dir);
    assert_eq!((r.errors, r.warnings), (0, 0), "{:?}", r.log);
    let legs = r.output("leg_3006", "Reprojected").unwrap();
    assert_eq!(legs.len(), 6);
    assert_eq!(col(legs, "leaves")[1], Value::Text("08:03".into()));
    assert!(legs.features.iter().all(|f| f.geom.is_some()));
    let deps = r.output("dep_sort", "Sorted").unwrap();
    assert_eq!(deps.len(), 40);
    assert_eq!(col(deps, "departure")[0], Value::Text("06:00".into()));
    assert_eq!(col(deps, "delay_s")[0], Value::Int(60));
    // 09:00 to 13:30 are ten, and five of them reach Chalmers.
    assert_eq!(r.output("window", "Passed").unwrap().len(), 10);
    let ab = r.output("ab_keep", "Output").unwrap();
    assert_eq!(col(ab, "departure"), ["09:00", "10:00", "11:00", "12:00", "13:00"].map(|t| Value::Text(t.into())));
    assert_eq!(col(ab, "arrives_b")[0], Value::Text("09:12".into()));
    assert_eq!(col(r.output("count", "Summary").unwrap(), "_count"), [Value::Int(5)]);
    assert_eq!(log.lock().unwrap().iter().filter(|q| q.path.contains("/index/trips/")).count(), 10);
    for f in ["otp_stops.gpkg", "otp_departures.csv", "otp_count_a_to_b.csv", "otp_departures_a_to_b.csv"] {
        assert!(dir.join("out").join(f).exists(), "{}", f);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Vector tiles from the web: those of a rectangle from an address with {z}/{x}/{y}, and a
/// PMTiles file read where it is, a part at a time. The pieces of each subregion that the
/// tiles cut apart are put together again.
#[test]
fn vector_tiles_are_read_from_the_web_by_their_address_and_as_pmtiles() {
    let formats = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/formats");
    let (base, asked) = file_server(formats);
    let dir = tmp("vector-tiles");
    let ws = model(
        &format!(
            r#"{{"id": "xyz", "type": "Reader", "params": {{"path": "{base}/tiles/subregions_tiles/{{z}}/{{x}}/{{y}}.pbf", "vt_zoom": "6",
                 "within": "rectangle", "min_x": "11.0", "min_y": "57.2", "max_x": "14.7", "max_y": "59.3"}}}},
               {{"id": "pieces", "type": "Reader", "params": {{"path": "{base}/tiles/subregions_tiles/{{z}}/{{x}}/{{y}}.pbf", "vt_zoom": "6", "vt_pieces": "keep",
                 "within": "rectangle", "min_x": "11.0", "min_y": "57.2", "max_x": "14.7", "max_y": "59.3"}}}},
               {{"id": "far", "type": "Reader", "params": {{"path": "{base}/tiles/subregions_tiles/{{z}}/{{x}}/{{y}}.pbf", "vt_zoom": "18",
                 "within": "rectangle", "min_x": "11.0", "min_y": "57.2", "max_x": "14.7", "max_y": "59.3"}}}},
               {{"id": "pm", "type": "Reader", "params": {{"path": "{base}/tiles/subregions.pmtiles", "vt_zoom": "7"}}}}"#
        ),
        "",
    );
    let r = run(&ws, Some(&GdalBackend::new()), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    let t = r.output("xyz", "Output").unwrap();
    assert_eq!(t.crs, Some(3857));
    assert_eq!(t.len(), 4, "four subregions, each whole");
    assert!(col(t, "layer").iter().all(|v| *v == Value::Text("subregions".into())));
    assert!(r.output("pieces", "Output").unwrap().len() > 4, "kept as pieces");
    assert!(r.nodes["far"].error.as_deref().unwrap_or("").contains("more than 500"), "{:?}", r.nodes["far"].error);
    assert_eq!(r.output("pm", "Output").unwrap().len(), 4);
    let asked = asked.lock().unwrap().clone();
    assert!(asked.iter().any(|a| a == "/tiles/subregions_tiles/6/33/18.pbf"), "{:?}", asked);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A zip file on the web is fetched and read as one on the computer. With several data files
/// in it, the input says which it read, and File in the zip chooses another, by name, path
/// or pattern.
#[test]
fn a_zip_file_on_the_web_is_read_and_a_file_in_it_chosen() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let serve = tmp("zip-serve");
    {
        let f = std::fs::File::create(serve.join("data.zip")).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (inside, from) in [
            ("data/subregions.gpkg", "subregions.gpkg"),
            ("towns.csv", "apis/towns.csv"),
            ("routes.shp", "routes.shp"),
            ("routes.shx", "routes.shx"),
            ("routes.dbf", "routes.dbf"),
            ("routes.prj", "routes.prj"),
        ] {
            z.start_file(inside, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&std::fs::read(examples.join(from)).unwrap()).unwrap();
        }
        z.finish().unwrap();
    }
    let (base, _) = file_server(serve.clone());
    let dir = tmp("zip-web");
    let ws = model(
        &format!(
            r#"{{"id": "first", "type": "Reader", "params": {{"path": "{base}/tiles/data.zip"}}}},
               {{"id": "gpkg", "type": "Reader", "params": {{"path": "{base}/tiles/data.zip", "zip_file": "*.gpkg"}}}},
               {{"id": "path", "type": "Reader", "params": {{"path": "{base}/tiles/data.zip", "zip_file": "data/subregions.gpkg"}}}},
               {{"id": "none", "type": "Reader", "params": {{"path": "{base}/tiles/data.zip", "zip_file": "roads.gpkg"}}}}"#
        ),
        "",
    );
    let r = run(&ws, Some(&GdalBackend::new()), &dir);
    let routes = r.output("first", "Output").unwrap();
    assert_eq!(routes.len(), 5, "the Shapefiles first, as before");
    assert!(r.nodes["first"].warnings.is_empty());
    assert!(
        r.log.iter().any(|l| l.msg.contains("data.zip holds 3 data files") && l.msg.contains("Read the Shapefiles")),
        "{:?}",
        r.log.iter().map(|l| &l.msg).collect::<Vec<_>>()
    );
    assert_eq!(r.output("gpkg", "Output").unwrap().len(), 4);
    assert_eq!(r.output("path", "Output").unwrap().len(), 4);
    let e = r.nodes["none"].error.clone().unwrap_or_default();
    assert!(e.contains("holds no file roads.gpkg") && e.contains("data/subregions.gpkg"), "{}", e);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&serve);
}
