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

/// The settings are the same for the whole program, so the tests that set them take turns.
static SETTINGS_TURN: Mutex<()> = Mutex::new(());

fn model(nodes: &str, edges: &str) -> Workspace {
    Workspace::from_json(&format!(r#"{{"nodes": [{}], "edges": [{}]}}"#, nodes, edges)).unwrap()
}

#[test]
fn places_in_answers_become_features() {
    let dir = tmp("places");
    let b = GdalBackend::new();
    let (base, _) = server(Arc::new(|r: &Req| {
        match r.route() {
        "/geojson" => (
            200,
            vec![],
            r#"{"type": "FeatureCollection", "features": [
                {"type": "Feature", "id": 7, "properties": {"name": "Borås", "people": 113000}, "geometry": {"type": "Point", "coordinates": [12.94, 57.72]}},
                {"type": "Feature", "properties": {"name": "Ale"}, "geometry": {"type": "Polygon", "coordinates": [[[12,58],[12.1,58],[12.1,58.1],[12,58]]]}}]}"#
                .into(),
        ),
        // As Nominatim: the latitude and longitude as text.
        "/search" => (200, vec![], r#"[{"place_id": 1, "lat": "57.7089", "lon": "11.9746", "display_name": "Göteborg"}]"#.into()),
        "/route" => (200, vec![], r#"{"route": {"path": [[11.97, 57.70], [12.0, 57.71], [12.94, 57.72]]}}"#.into()),
        _ => (200, vec![], r#"{"ok": true}"#.into()),
    }
    }));
    let ws = model(
        &format!(
            r#"{{"id": "gj", "type": "HttpCaller", "params": {{"url": "{b}/geojson", "as_features": "yes"}}}},
               {{"id": "nom", "type": "HttpCaller", "params": {{"url": "{b}/search", "as_features": "yes"}}}},
               {{"id": "path", "type": "HttpCaller", "params": {{"url": "{b}/route", "as_features": "yes", "items_path": "route.path"}}}},
               {{"id": "none", "type": "HttpCaller", "params": {{"url": "{b}/other", "as_features": "yes"}}}}"#,
            b = base
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    let gj = r.output("gj", "Output").unwrap();
    assert_eq!(gj.len(), 2);
    assert_eq!(col(gj, "name"), [Value::Text("Borås".into()), Value::Text("Ale".into())]);
    assert_eq!(col(gj, "people")[0], Value::Int(113000));
    assert_eq!(col(gj, "id")[0], Value::Int(7));
    assert!(gj.field_index("_response").is_none(), "the answer text is left out");
    assert_eq!(gj.crs, Some(4326));
    assert!(matches!(gj.features[1].geom, Some(geo::Geometry::Polygon(_))));
    let nom = r.output("nom", "Output").unwrap();
    let Some(geo::Geometry::Point(pt)) = nom.features[0].geom else { panic!("{:?}", nom.features[0].geom) };
    assert!((pt.x() - 11.9746).abs() < 1e-9 && (pt.y() - 57.7089).abs() < 1e-9);
    assert_eq!(col(nom, "display_name")[0], Value::Text("Göteborg".into()));
    assert_eq!(r.output("path", "Output").unwrap().len(), 3);
    assert_eq!(r.output("none", "Output").unwrap().len(), 0);
    assert_eq!(r.output("none", "Failed").unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
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
            // A cursor in the answer, sent back as a parameter: a number, then a text, then none.
            "/cursor" => match r.query("cursorId").as_deref() {
                None => (200, vec![], format!("{{\"items\": [{}], \"nextCursorId\": 4711}}", items(1, 2))),
                Some("4711") => (200, vec![], format!("{{\"items\": [{}], \"nextCursorId\": \"ab-12\"}}", items(3, 2))),
                Some("ab-12") => (200, vec![], format!("{{\"items\": [{}], \"nextCursorId\": null}}", items(5, 1))),
                _ => (400, vec![], "{}".into()),
            },
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
               {{"id": "cursor", "type": "HttpCaller", "params": {{"url": "{b}/cursor", "paging": "cursor", "cursor_path": "nextCursorId", "page_param": "cursorId"}}}},
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
    assert_eq!(pages("cursor").len(), 3, "until the cursor is null");
    assert_eq!(pages("most").len(), 2);
    let paths: Vec<String> = log.lock().unwrap().iter().map(|r| r.path.clone()).collect();
    assert!(paths.contains(&"/offset?offset=4&limit=2".to_string()), "{:?}", paths);
    assert!(paths.contains(&"/cursor?cursorId=4711".to_string()) && paths.contains(&"/cursor?cursorId=ab-12".to_string()), "{:?}", paths);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keys_tokens_and_busy_services() {
    let _turn = SETTINGS_TURN.lock().unwrap_or_else(|e| e.into_inner());
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
    let tokens_in_run = *tokens.lock().unwrap();
    let sent_elsewhere = log.lock().unwrap().iter().filter(|q| q.route() == "/header").count();
    // One request tried from the form: with the connection, and the key never shown.
    let params =
        |url: &str, conn: &str| -> og_engine::nodes::Params { serde_json::from_str(&format!(r#"{{"url": "{}", "connection": "{}"}}"#, url, conn)).unwrap() };
    let t = og_engine::nodes::try_request(&b, &params("", "Header"), &format!("{}/header", base), None).unwrap();
    assert_eq!((t.status, t.body.as_str(), t.method.as_str()), (200, "{\"ok\": 1}", "GET"));
    let t = og_engine::nodes::try_request(&b, &params("", "Query"), &format!("{}/query", base), None).unwrap();
    assert_eq!(t.status, 200);
    assert!(t.url.contains("key=•••") && !t.url.contains("s3cret"), "{}", t.url);
    assert!(og_engine::nodes::try_request(&b, &params("", "Header"), "not an address", None).is_err());
    assert_eq!(og_engine::nodes::constant_text(&format!("'{}/a?b=' || 2", base)), Some(format!("{}/a?b=2", base)));
    assert_eq!(og_engine::nodes::constant_text("'x' || name"), None, "an address built from attributes needs a feature");
    // Test the connection: a new token, and the test address with the key.
    let conn = |name: &str| og_engine::settings::current().unwrap().apis.iter().find(|c| c.name == name).cloned().unwrap();
    let mut header = conn("Header");
    header.test_url = format!("{}/header", base);
    let said = og_engine::web::try_connection(&b, &header).unwrap();
    assert!(said.contains("answered 200"), "{}", said);
    header.secret = "wrong".into();
    let said = og_engine::web::try_connection(&b, &header).unwrap_err();
    assert!(said.contains("answered 403") && said.contains("key was not accepted"), "{}", said);
    let lm = conn("Lantmäteriet");
    assert_eq!(og_engine::web::try_connection(&b, &lm).unwrap(), "A token was fetched. Write a test address to try a request too.");
    assert_eq!(*tokens.lock().unwrap(), tokens_in_run + 1, "the test fetches a new token");
    let mut wrong = lm.clone();
    wrong.secret = "nope".into();
    let said = og_engine::web::try_connection(&b, &wrong).unwrap_err();
    assert!(said.contains("could not be fetched") && said.contains("401") && !said.contains("nope"), "{}", said);
    assert!(og_engine::web::try_connection(&b, &conn("Text")).unwrap_err().contains("Write a test address"));
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
    assert_eq!(tokens_in_run, 2);
    assert_eq!(ok("busy"), [Value::Int(200)], "a busy service is asked again");
    // The key is not sent to an address it is not for, and is never shown.
    let why = |id: &str| match &col(r.output(id, "Failed").unwrap(), "_error")[0] {
        Value::Text(t) => t.clone(),
        other => panic!("{:?}", other),
    };
    let e = why("elsewhere");
    assert!(e.contains("begin with https://api.trafikinfo") && !e.contains("s3cret"), "{}", e);
    assert!(why("nokey").contains("{{key}} is written"));
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
        first.contains("bbox=11.8,57.6,12.1,57.8")
            && first.contains("collections=sentinel-2-l2a")
            && first.contains("datetime=2026-06-01T00%3A00%3A00Z%2F2026-06-30T23%3A59%3A59Z"),
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

/// The GitHub example on a made-up copy of the API: pages by the Link header, stars with the
/// time they were given, pull requests left out of the issues, and a chart of each.
#[test]
fn the_github_example_draws_its_charts_and_keeps_a_history() {
    let dir = tmp("github");
    let b = GdalBackend::new();
    let (url, log) = server(Arc::new(|r: &Req| {
        let host = r.headers.get("host").cloned().unwrap_or_default();
        let repo = "/repos/gishubio/Ographis";
        let route = r.route().strip_prefix(repo).unwrap_or("?");
        let page2 = r.query("page").as_deref() == Some("2");
        match route {
            "" => (200, vec![], r#"{"stargazers_count": 3, "subscribers_count": 2, "forks_count": 1, "open_issues_count": 2, "size": 900}"#.into()),
            "/issues" if !page2 => (
                200,
                vec![("Link".into(), format!("<http://{}{}/issues?state=all&per_page=100&page=2>; rel=\"next\"", host, repo))],
                r#"[{"number": 1, "state": "open", "created_at": "2026-10-11T23:30:00Z", "closed_at": null},
                    {"number": 2, "state": "closed", "created_at": "2026-10-13T08:00:00Z", "closed_at": "2026-10-20T09:00:00Z"},
                    {"number": 3, "state": "open", "created_at": "2026-10-14T08:00:00Z", "closed_at": null, "pull_request": {"url": "x"}}]"#
                    .into(),
            ),
            "/issues" => (200, vec![], r#"[{"number": 4, "state": "closed", "created_at": "2026-10-21T10:00:00Z", "closed_at": "2026-10-22T10:00:00Z"}]"#.into()),
            "/stargazers" if r.headers.get("accept").is_some_and(|a| a.contains("star+json")) => (
                200,
                vec![],
                r#"[{"starred_at": "2026-10-02T10:00:00Z"}, {"starred_at": "2026-10-01T10:00:00Z"}, {"starred_at": "2026-10-05T10:00:00Z"}]"#.into(),
            ),
            "/stargazers" => (200, vec![], r#"[{"login": "a"}]"#.into()),
            "/forks" => (200, vec![], r#"[{"created_at": "2026-10-03T10:00:00Z"}]"#.into()),
            "/releases" => (
                200,
                vec![],
                r#"[{"tag_name": "0.95", "assets": [{"name": "a.zip", "download_count": 12}, {"name": "b.zip", "download_count": 3}]},
                    {"tag_name": "0.94", "assets": [{"name": "a.zip", "download_count": 7}]}]"#
                    .into(),
            ),
            "/traffic/clones" => (200, vec![], r#"{"count": 9, "uniques": 4, "clones": [{"timestamp": "2026-11-01T00:00:00Z", "count": 5, "uniques": 2}, {"timestamp": "2026-11-02T00:00:00Z", "count": 4, "uniques": 2}]}"#.into()),
            "/traffic/views" => (200, vec![], r#"{"count": 30, "uniques": 9, "views": [{"timestamp": "2026-11-01T00:00:00Z", "count": 20, "uniques": 5}, {"timestamp": "2026-11-02T00:00:00Z", "count": 10, "uniques": 4}]}"#.into()),
            "/traffic/popular/referrers" => (200, vec![], r#"[{"referrer": "github.com", "count": 8, "uniques": 3}, {"referrer": "gishub.io", "count": 4, "uniques": 2}]"#.into()),
            _ => (404, vec![], r#"{"message": "Not Found"}"#.into()),
        }
    }));
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apis");
    // Without an API connection: the made-up server needs no token.
    let model = std::fs::read_to_string(examples.join("github_statistics.ographis"))
        .unwrap()
        .replace("https://api.github.com", &url)
        .replace(r#""value": "GitHub""#, r#""value": """#);
    let ws = Workspace::from_json(&model).unwrap();
    let r = run(&ws, Some(&b), &dir);
    let errors: Vec<_> = r.nodes.iter().filter_map(|(k, n)| n.error.clone().map(|e| (k.clone(), e))).collect();
    assert!(errors.is_empty(), "{:?}", errors);
    // Four issues on two pages, the pull request left out.
    assert_eq!(col(r.output("only_issues", "Passed").unwrap(), "number"), [Value::Int(1), Value::Int(2), Value::Int(4)]);
    // A week starts on a Monday, also for an issue opened late on a Sunday.
    let weeks: Vec<String> = col(r.output("opened", "Output").unwrap(), "week").iter().map(|v| format!("{:?}", v)).collect();
    assert!(weeks[0].contains("2026-10-05") && weeks[1].contains("2026-10-12"), "{:?}", weeks);
    // The stars in the order given, counted.
    let stars = r.output("stars_day", "Output").unwrap();
    assert_eq!(col(stars, "day"), [Value::Text("2026-10-01".into()), Value::Text("2026-10-02".into()), Value::Text("2026-10-05".into())]);
    assert_eq!(col(stars, "stars"), [Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert_eq!(r.output("assets", "Exploded").unwrap().len(), 3);
    for png in [
        "issues_per_week",
        "issues_open_and_closed",
        "stars_over_time",
        "forks_over_time",
        "downloads_per_release",
        "clones_per_day",
        "views_per_day",
        "referrers",
    ] {
        assert!(dir.join(format!("out/github/{}.png", png)).exists(), "{}", png);
    }
    let accept: Vec<_> = log.lock().unwrap().iter().filter(|q| q.route().ends_with("/stargazers")).filter_map(|q| q.headers.get("accept").cloned()).collect();
    assert_eq!(accept, ["application/vnd.github.star+json"]);
    // A second run adds to the history, and the history model draws it.
    let r = run(&ws, Some(&b), &dir);
    assert!(r.nodes.values().all(|n| n.error.is_none()));
    let history = std::fs::read_to_string(dir.join("out/github/repository_history.csv")).unwrap();
    assert_eq!(history.lines().count(), 3, "{}", history);
    let ws = Workspace::from_json(&std::fs::read_to_string(examples.join("github_history.ographis")).unwrap()).unwrap();
    let r = run(&ws, Some(&b), &dir);
    let errors: Vec<_> = r.nodes.iter().filter_map(|(k, n)| n.error.clone().map(|e| (k.clone(), e))).collect();
    assert!(errors.is_empty(), "{:?}", errors);
    assert_eq!(r.output("clones", "Output").unwrap().len(), 4);
    for png in ["repository_over_time", "clones_over_time", "views_over_time"] {
        assert!(dir.join(format!("out/github/{}.png", png)).exists(), "{}", png);
    }
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
                let file = path.split('?').next().unwrap().trim_start_matches("/tiles/").trim_start_matches('/');
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

/// Vector tiles written into a PMTiles file by Ographis itself, read back by GDAL: each
/// subregion whole again, with its attributes, at the largest zoom level and the smallest.
#[test]
fn features_are_written_as_pmtiles_and_read_back_by_gdal() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let dir = tmp("pmtiles-write");
    let out = dir.join("subregions.pmtiles");
    let ws = model(
        &format!(
            r#"{{"id": "in", "type": "Reader", "params": {{"path": "{}"}}}},
               {{"id": "w", "type": "Writer", "params": {{"path": "{}", "pm_min": "2", "pm_max": "9", "layer": "regions"}}}}"#,
            examples.join("subregions.gpkg").display().to_string().replace('\\', "/"),
            out.display().to_string().replace('\\', "/")
        ),
        r#"{"from": ["in", "Output"], "to": ["w", "Input"]}"#,
    );
    let r = run(&ws, Some(&GdalBackend::new()), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    assert!(r.nodes["w"].error.is_none(), "{:?}", r.nodes["w"].error);
    let source = r.output("in", "Output").unwrap().clone();
    for zoom in ["9", "2"] {
        let back = model(
            &format!(r#"{{"id": "pm", "type": "Reader", "params": {{"path": "{}", "vt_zoom": "{}"}}}}"#, out.display().to_string().replace('\\', "/"), zoom),
            "",
        );
        let r = run(&back, Some(&GdalBackend::new()), &dir);
        assert!(r.nodes["pm"].error.is_none(), "{:?}", r.nodes["pm"].error);
        let t = r.output("pm", "Output").unwrap();
        assert_eq!(t.len(), source.len(), "zoom {}", zoom);
        assert!(t.fields.iter().any(|f| f.name == "subregion"));
        let names = |t: &Table| {
            let at = t.fields.iter().position(|f| f.name == "subregion").unwrap();
            let mut v: Vec<String> = t.features.iter().map(|f| f.attrs[at].to_text()).collect();
            v.sort();
            v
        };
        assert_eq!(names(t), names(&source));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// GeoParquet within a rectangle: only the row groups whose boxes reach into it are read,
/// from a file on this computer and from one on the web, where only their bytes are fetched.
#[test]
fn geoparquet_is_read_in_parts_within_a_rectangle() {
    use og_engine::io::Backend;
    use og_engine::{Feature, Field, FieldType};
    let dir = tmp("parquet-parts");
    let serve = tmp("parquet-parts-serve");
    let mut t = Table { fields: vec![Field::new("n", FieldType::Int)], crs: Some(3006), ..Default::default() };
    for i in 0..40 {
        for j in 0..25 {
            let p = geo::Point::new(500_000.0 + i as f64 * 100.0, 6_400_000.0 + j as f64 * 100.0);
            t.features.push(Feature::new(vec![Value::Int(i * 100 + j)], Some(geo::Geometry::Point(p))));
        }
    }
    let b = GdalBackend::new();
    let projjson = b.crs_projjson(3006);
    og_engine::parquet_io::write_parquet_in(&t, &serve.join("points.parquet"), projjson.as_deref(), 100).unwrap();
    let (base, asked) = file_server(serve.clone());
    let rect = r#""within": "rectangle", "min_x": "500000", "min_y": "6400000", "max_x": "500900", "max_y": "6400900""#;
    let ws = model(
        &format!(
            r#"{{"id": "local", "type": "Reader", "params": {{"path": "{}", {rect}}}}},
               {{"id": "web", "type": "Reader", "params": {{"path": "{base}/tiles/points.parquet", {rect}}}}},
               {{"id": "deg", "type": "Reader", "params": {{"path": "{base}/tiles/points.parquet", "within": "rectangle", "min_x": "10", "min_y": "57", "max_x": "20", "max_y": "60", "within_crs": "EPSG:4326"}}}}"#,
            serve.join("points.parquet").display().to_string().replace('\\', "/")
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    for id in ["local", "web"] {
        assert!(r.nodes[id].error.is_none(), "{:?}", r.nodes[id].error);
        assert_eq!(r.output(id, "Output").unwrap().len(), 100, "{}", id);
        assert_eq!(r.output(id, "Output").unwrap().crs, Some(3006));
    }
    assert!(r.log.iter().any(|l| l.msg.contains("3 of the 10 row groups")), "the groups of x up to 1100 m");
    assert!(!asked.lock().unwrap().is_empty());
    assert!(r.nodes["deg"].error.is_none(), "{:?}", r.nodes["deg"].error);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&serve);
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
            ("routes.cpg", "routes.cpg"),
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
    assert!(r.nodes["first"].warnings.is_empty(), "{:?}", r.nodes["first"].warnings);
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

/// Overture Maps, as its catalog and its files are laid out, served from this computer: the
/// catalog names the latest release, the files are found by their extents, and of a file only
/// the parts with buildings within the rectangle are fetched.
#[test]
fn overture_maps_is_read_within_a_rectangle_in_parts() {
    use og_engine::{Feature, Field, FieldType, Table, Value};
    let dir = std::env::temp_dir().join(format!("og-overture-{}", std::process::id()));
    let kind = dir.join("2026-10-21.0/buildings/building");
    std::fs::create_dir_all(kind.join("00000")).unwrap();
    std::fs::create_dir_all(kind.join("00001")).unwrap();
    // A grid of 40 × 40 small buildings around Göteborg, sorted from west to east, in row
    // groups of 100: each group covers a strip.
    let mut t = Table { fields: vec![Field::new("id", FieldType::Text), Field::new("height", FieldType::Real)], crs: Some(4326), ..Default::default() };
    for i in 0..40 {
        for j in 0..40 {
            let (x, y) = (11.9 + i as f64 * 0.002, 57.68 + j as f64 * 0.001);
            let sq = geo::Rect::new(geo::coord! {x: x, y: y}, geo::coord! {x: x + 0.0005, y: y + 0.0003}).to_polygon();
            t.features.push(Feature::new(vec![Value::Text(format!("b{}_{}", i, j)), Value::Real(3.0 + j as f64)], Some(geo::Geometry::Polygon(sq))));
        }
    }
    og_engine::parquet_io::write_parquet_in(&t, &kind.join("part-0.parquet"), None, 100).unwrap();
    std::fs::write(kind.join("part-1.parquet"), b"not fetched, since it lies elsewhere").unwrap();
    let (base, asked) = file_server(dir.clone());
    let base = format!("{}/tiles", base);
    std::fs::write(dir.join("catalog.json"), r#"{"latest": "2026-10-21.0", "links": []}"#).unwrap();
    std::fs::write(
        kind.join("collection.json"),
        r#"{"links": [{"rel": "root", "href": "../../../catalog.json"}, {"rel": "item", "href": "./00000/00000.json"}, {"rel": "item", "href": "./00001/00001.json"}]}"#,
    )
    .unwrap();
    std::fs::write(kind.join("00000/00000.json"), r#"{"bbox": [11.9, 57.68, 11.98, 57.72], "assets": {"aws": {"href": "../part-0.parquet"}}}"#).unwrap();
    std::fs::write(kind.join("00001/00001.json"), r#"{"bbox": [20.0, 60.0, 21.0, 61.0], "assets": {"aws": {"href": "../part-1.parquet"}}}"#).unwrap();
    let ws = og_engine::Workspace::from_json(&format!(
        r#"{{"nodes": [{{"id": "o", "type": "OvertureDownloader", "params": {{"kind": "buildings/building", "bbox": "11.9195, 57.6895, 11.9245, 57.6955", "catalog": "{}", "crs": "EPSG:3006"}}}}], "edges": []}}"#,
        base
    ))
    .unwrap();
    let r = og_engine::run(&ws, Some(&og_gdal::GdalBackend::new()), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    let out = r.output("o", "Output").expect("buildings");
    // Columns 10 to 12 (x 11.92 to 11.924) and rows 10 to 15.
    assert_eq!((out.len(), out.crs), (18, Some(3006)), "{:?}", r.log);
    assert!(out.field_names().contains(&"height".to_string()));
    let fetched = asked.lock().unwrap().clone();
    assert!(!fetched.iter().any(|p| p.contains("part-1")), "the file elsewhere is not read: {:?}", fetched);
    assert!(r.log.iter().any(|l| l.msg.contains("parts of 1 files") && !l.msg.contains("16 of 16")), "only some row groups: {:?}", r.log);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A key made for this test only, for a service account that does not exist.
const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDZaAq2L5CXP873\nkZQgiv5+aVzZbVQQ7bu2owu+lOR4IO8VuCkuoeoYlGu+/nW3QEkKuP3zwYyVbS/X\n3ncdSs2tW5OolO1jPqoiJfP4cxWeuxaVyiu5HaPhqecu+oeLMOSSJr1z434XrtTN\nyFWhxdyPBCYSpppENhuLwBaWuTt6dHN/8WKOszhzygHlWuDp2GJcERaZThig7tWa\nlwlyMpdh2iscXGVjOGWMoo5NWZYhilt/4dWgUAQixiUaGeg9FLyEmWqdJRa3eqV/\ncCy+gP42qv+Ql5VEzZTfb9yiZzqyh971oA3Qjc9e5fKyd8SVfQJXv1WertyC0xpG\nkZ+9Hqf/AgMBAAECggEANvU1F/8IJyPrNuwnLN7WHLCX1yLrCGiAdlfbJS9QXgm9\nRplKV854dgQ3BI4gxdVLYZmIIPGBj1iNqp1DMTVyrusidZBIPjleGsnLVbgJ9BlJ\nSU5VJTOX9PjVRyvvzcDrVwLSC5SrkGMSyn5lH0L8UW70DOzuvGWNKxPA4hMrNPX5\n4NfuVjJteUyKDRffjPwgadZ1N4kddZsCC3sy79IQS52NU7Uz5zo9YIyU1maDyro/\n3VSygfCTjvgGvOF5Rj1uF3j8QcqOGPy8MtNYyWHu8h41mjyngbdiUbO4t5TejaZB\nrpeMHGJ6RcZO/9zh1+AfjXK7VGytyW8SOcTvvh+nkQKBgQDuFpP9yHQ2FkqwR8m6\nteSGierfqTcXD+qLIF5d7Cf06btoaKlexK+jTYQW73hOyKnniJLLAk9Ygwb748rd\nsk8EVoTA16c8r8ALTPDu++ehzvB8VeOEoqWKvjJhgyEbdV/fBdG91uuQC9iscIc+\n+MGz0AXNPzVWzoEzUAvKTotLZwKBgQDpwyVwDXxAkLzAjdJ42Dd08HgIxCNaSHXj\nD/eEJouw++aPoLM7lV5k9rG4wvbU8wONPLpMktwx7YphTkmhZJQcswwnL8ayL32T\n8iEcduL4nN3zBUPWjrwC4Z859GZefja8eVA1vO15l29qiYRUJx+5rwv1kptIU1+s\n8RDKDtt3qQKBgAezo0XqXhgB0JizOctYZvQTMg3aI3YThbrKaUMQiJpblTxjZlcy\ndZwKveMgC/xAcX6T5Gu8v2RVmLw9OirBwdyaD+FNGJPV+5hh/Mk+TNfnIdBj1uBL\ngp9iV67f1wpUWa2VOpuxgB1AxNmPKlDXKyzt5cIl8UvFklheloMM12SxAoGAYANd\nTyba0BIi1NvrhOMZ9BgI1WDqPFzivr6CJW9HlQwB6SBSPbjeKo4A9ShvBYxC+zHk\nOyte8crlOY5UQ8vx4c/QwA+phupmo+uhmfViyEZ2CykwceHyaySMp9swaMNibAtP\n3KKaz4RZqOKOzZm8OCIjwVvUF+LWu0vQX422qvECgYEAw4jQHc7Ds8k2zx0Cvo+y\nQT1FjMxQEp3UcsfrJSCfiQDExIsmf9GLq/IasbfPehbylVi8cQoPWuD/AlbE5/xA\nh2v/dv1DRZApLVxUFANmmZE21mH5Nox6jruLLi/TlqjujMV3E+qAeAQvB0HJpIft\nHDee/yfFxEJrdQWFflqfP4k=\n-----END PRIVATE KEY-----\n";

/// Google Sheets: a sheet shared with a link read as CSV, and a private one read, written
/// over and added to through the API with a service account, against a server on this
/// computer that answers as Google does.
#[test]
fn google_sheets_are_read_and_written() {
    let _turn = SETTINGS_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp("gsheets");
    let b = GdalBackend::new();
    type Sheets = Arc<Mutex<Vec<(String, Vec<Vec<serde_json::Value>>)>>>;
    let sheets: Sheets = Arc::new(Mutex::new(vec![(
        "Rooms".to_string(),
        vec![
            vec![serde_json::json!("name"), serde_json::json!("area"), serde_json::json!("wkt")],
            vec![serde_json::json!("Hall"), serde_json::json!(40.5), serde_json::json!("POINT (1 2)")],
            vec![serde_json::json!("Kitchen, small"), serde_json::json!(12), serde_json::json!("POINT (3 4)")],
        ],
    )]));
    let state = sheets.clone();
    let (base, log) = server(Arc::new(move |r: &Req| {
        let authed = r.headers.get("authorization").map(|a| a.as_str()) == Some("Bearer tok-1");
        let route = r.route().to_string();
        let json = |v: serde_json::Value| (200, vec![], v.to_string());
        if route == "/token" {
            let ok = r.body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer")
                && r.body.split("assertion=").nth(1).is_some_and(|a| a.split('.').count() == 3);
            return if ok { json(serde_json::json!({"access_token": "tok-1", "expires_in": 3600})) } else { (400, vec![], "{}".into()) };
        }
        if route == "/spreadsheets/d/PUB/export" {
            return (200, vec![("Content-Type".into(), "text/csv".into())], "name,count\nNorth,3\nSouth,5\n".into());
        }
        if route == "/spreadsheets/d/PRIV/export" {
            return (200, vec![("Content-Type".into(), "text/html".into())], "<html>Sign in</html>".into());
        }
        if !authed {
            return (401, vec![], "{\"error\": {\"message\": \"no token\"}}".into());
        }
        let mut s = state.lock().unwrap();
        let body: serde_json::Value = serde_json::from_str(&r.body).unwrap_or(serde_json::Value::Null);
        if route == "/v4/spreadsheets/ABC" {
            let list: Vec<serde_json::Value> = s.iter().enumerate().map(|(i, (t, _))| serde_json::json!({"properties": {"title": t, "sheetId": i}})).collect();
            return json(serde_json::json!({"sheets": list}));
        }
        if route == "/v4/spreadsheets/ABC:batchUpdate" {
            let title = body["requests"][0]["addSheet"]["properties"]["title"].as_str().unwrap().to_string();
            s.push((title, Vec::new()));
            return json(serde_json::json!({}));
        }
        let rest = route.strip_prefix("/v4/spreadsheets/ABC/values/").unwrap_or("");
        let title = rest.split(['!', ':']).next().unwrap_or("").to_string();
        let Some(k) = s.iter().position(|x| x.0 == title) else { return (400, vec![], "{\"error\": {\"message\": \"no such sheet\"}}".into()) };
        let rows =
            |v: &serde_json::Value| -> Vec<Vec<serde_json::Value>> { v["values"].as_array().unwrap().iter().map(|r| r.as_array().unwrap().clone()).collect() };
        match (r.method.as_str(), rest) {
            ("GET", x) if x.ends_with("!A1:A1") => {
                json(serde_json::json!({"values": s[k].1.iter().take(1).map(|r| r.iter().take(1).cloned().collect::<Vec<_>>()).collect::<Vec<_>>()}))
            }
            ("GET", _) => json(serde_json::json!({"values": s[k].1})),
            ("POST", x) if x.ends_with(":clear") => {
                s[k].1.clear();
                json(serde_json::json!({}))
            }
            ("PUT", _) => {
                s[k].1 = rows(&body);
                json(serde_json::json!({}))
            }
            ("POST", x) if x.ends_with(":append") => {
                let more = rows(&body);
                s[k].1.extend(more);
                json(serde_json::json!({}))
            }
            _ => (404, vec![], "{}".into()),
        }
    }));
    std::env::set_var("OGRAPHIS_GOOGLE_DOCS", &base);
    std::env::set_var("OGRAPHIS_GOOGLE_SHEETS_API", &base);
    let key = serde_json::json!({"type": "service_account", "client_email": "ographis-test@example.iam.gserviceaccount.com", "private_key": TEST_KEY, "token_uri": format!("{}/token", base)});
    use_settings(Some(Settings {
        apis: vec![ApiConnection { name: "Sheets".into(), auth: "google".into(), secret: key.to_string(), ..Default::default() }],
        ..Default::default()
    }));
    let ws = model(
        &format!(
            r#"{{"id": "shared", "type": "Reader", "params": {{"path": "{b}/spreadsheets/d/PUB/edit#gid=0"}}}},
               {{"id": "private", "type": "Reader", "params": {{"path": "{b}/spreadsheets/d/ABC/edit", "layer": "Rooms", "connection": "Sheets", "crs": "EPSG:3006"}}}},
               {{"id": "out", "type": "Writer", "params": {{"path": "{b}/spreadsheets/d/ABC/edit", "format": "gsheet", "layer": "Copy", "connection": "Sheets"}}}},
               {{"id": "closed", "type": "Reader", "params": {{"path": "{b}/spreadsheets/d/PRIV/edit"}}}}"#,
            b = base
        ),
        r#"{"from": ["private", "Output"], "to": ["out", "Input"]}"#,
    );
    let r = run(&ws, Some(&b), &dir);
    for l in &r.log {
        println!("{:?} {}", l.level, l.msg);
    }
    // Shared with a link: the CSV Google gives.
    let shared = r.output("shared", "Output").unwrap();
    assert_eq!(col(shared, "count"), [Value::Int(3), Value::Int(5)]);
    // Private, with the service account: numbers as numbers, text with a comma whole, WKT.
    let private = r.output("private", "Output").unwrap();
    assert_eq!(col(private, "name"), [Value::Text("Hall".into()), Value::Text("Kitchen, small".into())]);
    assert_eq!(col(private, "area")[0], Value::Real(40.5));
    assert!(private.features[1].geom.is_some());
    // Written to a new sheet: the names, and the rows with their geometry as WKT.
    let copy = sheets.lock().unwrap().iter().find(|s| s.0 == "Copy").cloned().expect("the sheet Copy was added");
    assert_eq!(copy.1.len(), 3);
    assert_eq!(copy.1[0].last().unwrap(), "wkt");
    assert_eq!(copy.1[2][0], "Kitchen, small");
    // Not shared: says how to share it.
    let closed = r.nodes["closed"].error.clone().unwrap();
    assert!(closed.contains("not shared with a link"), "{}", closed);
    assert!(log.lock().unwrap().iter().filter(|q| q.route() == "/token").count() <= 2, "the token is kept");
    // Added to: the rows after those there, without the names again.
    let ws = model(
        &format!(
            r#"{{"id": "private", "type": "Reader", "params": {{"path": "{b}/spreadsheets/d/ABC/edit#gid=0", "connection": "Sheets"}}}},
               {{"id": "out", "type": "Writer", "params": {{"path": "{b}/spreadsheets/d/ABC/edit", "format": "gsheet", "layer": "Copy", "connection": "Sheets", "mode": "append"}}}}"#,
            b = base
        ),
        r#"{"from": ["private", "Output"], "to": ["out", "Input"]}"#,
    );
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    let copy = sheets.lock().unwrap().iter().find(|s| s.0 == "Copy").cloned().unwrap();
    assert_eq!(copy.1.len(), 5);
    assert_eq!(copy.1[3][0], "Hall");
    std::env::remove_var("OGRAPHIS_GOOGLE_DOCS");
    std::env::remove_var("OGRAPHIS_GOOGLE_SHEETS_API");
    use_settings(None);
}

#[test]
fn every_request_is_written_to_the_log() {
    let dir = tmp("request_log");
    let b = GdalBackend::new();
    let (base, _) = server(Arc::new(|r: &Req| match r.route() {
        "/ok" => (200, vec![], "{\"ok\": 1}".into()),
        _ => (404, vec![], "{}".into()),
    }));
    let ws = model(
        &format!(
            r#"{{"id": "get", "type": "HttpCaller", "params": {{"url": "{b}/ok?q=1", "log": "logs/requests.log"}}}},
               {{"id": "post", "type": "HttpCaller", "params": {{"url": "{b}/missing", "method": "POST", "body": "'a\tb'", "log": "logs/requests.log"}}}}"#,
            b = base
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    assert_eq!(r.output("get", "Output").unwrap().len(), 1, "{:?}", r.log);
    let text = std::fs::read_to_string(dir.join("logs/requests.log")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "a heading and a line for each request: {}", text);
    assert!(lines[0].starts_with("time\tmethod\taddress\tstatus"));
    let get = lines.iter().find(|l| l.contains("/ok?q=1")).unwrap();
    let cells: Vec<&str> = get.split('\t').collect();
    assert_eq!((cells[1], cells[3], cells[4]), ("GET", "200", "9"));
    let post = lines.iter().find(|l| l.contains("/missing")).unwrap();
    assert!(post.contains("\t404\t") && post.ends_with("a\\tb"), "the body on one line: {}", post);
    // A later run adds to the file.
    run(&ws, Some(&b), &dir);
    assert_eq!(std::fs::read_to_string(dir.join("logs/requests.log")).unwrap().lines().count(), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A GeoTIFF of one band in degrees, made with GDAL, its values from a function of the
/// longitude and latitude of each cell.
fn degrees_tif(path: &std::path::Path, west: f64, north: f64, cell: f64, n: usize, value: impl Fn(f64, f64) -> f32) {
    let drv = gdal::DriverManager::get_driver_by_name("GTiff").unwrap();
    let mut ds = drv.create_with_band_type::<f32, _>(path, n, n, 1).unwrap();
    ds.set_geo_transform(&[west, cell, 0.0, north, 0.0, -cell]).unwrap();
    ds.set_spatial_ref(&gdal::spatial_ref::SpatialRef::from_epsg(4326).unwrap()).unwrap();
    let values: Vec<f32> = (0..n * n).map(|k| value(west + ((k % n) as f64 + 0.5) * cell, north - ((k / n) as f64 + 0.5) * cell)).collect();
    ds.rasterband(1).unwrap().write((0, 0), (n, n), &mut gdal::raster::Buffer::new((n, n), values)).unwrap();
}

#[test]
fn a_place_is_fetched_with_heights_and_a_clear_picture() {
    let dir = tmp("place");
    let web = dir.join("web");
    let tile = "Copernicus_DSM_COG_10_N19_00_W156_00_DEM";
    std::fs::create_dir_all(web.join("dem").join(tile)).unwrap();
    std::fs::create_dir_all(web.join("geo")).unwrap();
    std::fs::create_dir_all(web.join("stac")).unwrap();
    std::fs::create_dir_all(web.join("s2")).unwrap();
    // A volcano 1 200 m high at 19.41, -155.28, in the degree of the DEM it lies in.
    let peak = |lon: f64, lat: f64| (1200.0 * (-((lon + 155.28).powi(2) + (lat - 19.41).powi(2)) / 0.0004).exp()) as f32;
    degrees_tif(&web.join("dem").join(tile).join(format!("{}.tif", tile)), -156.0, 20.0, 1.0 / 360.0, 360, peak);
    // Two pictures: a clear one, red where the volcano is, and one under clouds.
    let near = |lon: f64, lat: f64| (lon + 155.28).abs() < 0.01 && (lat - 19.41).abs() < 0.01;
    for (name, v) in [("red", 2000.0f32), ("green", 1000.0), ("blue", 500.0)] {
        degrees_tif(
            &web.join("s2").join(format!("{}.tif", name)),
            -155.32,
            19.45,
            0.0001,
            800,
            |lon, lat| if near(lon, lat) || name != "red" { v } else { 300.0 },
        );
        degrees_tif(&web.join("s2").join(format!("cloudy_{}.tif", name)), -155.32, 19.45, 0.0001, 800, |_, _| 9000.0);
    }
    degrees_tif(&web.join("s2").join("scl.tif"), -155.32, 19.45, 0.0001, 800, |_, _| 4.0);
    degrees_tif(&web.join("s2").join("cloudy_scl.tif"), -155.32, 19.45, 0.0001, 800, |_, _| 9.0);
    let (base, asked) = file_server(web.clone());
    std::fs::write(web.join("geo/search"), r#"[{"lat": "19.41", "lon": "-155.28", "display_name": "Kīlauea, Hawaiʻi County, Hawaii"}]"#).unwrap();
    let item = |id: &str, cloud: f64, prefix: &str| {
        format!(
            r#"{{"id": "{id}", "properties": {{"eo:cloud_cover": {cloud}, "datetime": "2025-07-0{n}T20:00:00Z"}}, "assets": {{"red": {{"href": "{base}/s2/{prefix}red.tif"}}, "green": {{"href": "{base}/s2/{prefix}green.tif"}}, "blue": {{"href": "{base}/s2/{prefix}blue.tif"}}, "scl": {{"href": "{base}/s2/{prefix}scl.tif"}}}}}}"#,
            n = if prefix.is_empty() { 1 } else { 2 }
        )
    };
    std::fs::write(web.join("stac/search"), format!(r#"{{"features": [{}, {}]}}"#, item("cloudy", 25.0, "cloudy_"), item("clear", 3.0, ""))).unwrap();
    let ws = model(
        &format!(
            r#"{{"id": "place", "type": "PlaceFetcher", "params": {{"place": "Kilauea", "size": "4", "geocoder": "{b}/geo", "dem": "{b}/dem", "stac": "{b}/stac"}}}}"#,
            b = base
        ),
        "",
    );
    let r = run(&ws, Some(&GdalBackend::new()), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    // In UTM zone 5 north, a square of 4 km.
    let area = r.output("place", "Area").unwrap();
    assert_eq!((area.crs, area.features[0].attrs[3].clone()), (Some(32605), Value::Int(32605)));
    assert!(area.features[0].attrs[0].to_text().starts_with("Kīlauea"));
    let heights = r.output("place", "Heights").unwrap().grid.clone().unwrap();
    assert_eq!((heights.width, heights.height, heights.crs), (133, 133, Some(32605)));
    let top = heights.values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(top > 1100.0 && top <= 1200.0, "the top of the volcano: {}", top);
    // The picture: the clear one only, red at the volcano, 10 m pixels.
    let image = r.output("place", "Image").unwrap().image.clone().unwrap();
    // The side is whole cells of the heights, 3 990 m, so 399 pixels of 10 m.
    assert_eq!((image.width, image.height), (399, 399));
    let mid = (199 * 399 + 199) * 4;
    assert!(image.rgba[mid] > image.rgba[mid + 2], "red over blue at the volcano: {:?}", &image.rgba[mid..mid + 4]);
    assert!(r.log.iter().any(|l| l.msg.contains("2025-07-01 (3 % clouds)")), "the clear picture is named");
    assert!(r.log.iter().any(|l| l.msg.contains("Copernicus DEM")), "the source of the heights is named");
    // What it does is in the log as it goes, step by step.
    for step in [
        "Looking for Kilauea",
        "Found Kīlauea",
        "The area: 4 × 4 km",
        "UTM zone 5",
        "Heights 1 of",
        "Heights done in",
        "Picture: searching",
        "least cloudy are used",
        "Picture 1 of",
        "put together",
    ] {
        assert!(r.log.iter().any(|l| l.msg.contains(step)), "{}: {:?}", step, r.log.iter().map(|l| &l.msg).collect::<Vec<_>>());
    }
    let asked = asked.lock().unwrap();
    assert!(asked.iter().any(|a| a.starts_with("/geo/search?format=jsonv2&limit=1&q=Kilauea")), "{:?}", asked);
    assert!(asked.iter().any(|a| a.contains("eo%3Acloud_cover") || a.contains("eo:cloud_cover")), "{:?}", &asked[..asked.len().min(8)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The example of Hunneberg: the roads and lakes of OpenStreetMap drawn over the picture of
/// a place, and the map draped over its terrain, with the services of the web played by
/// servers on this computer.
#[test]
fn roads_and_lakes_are_drawn_over_the_picture_of_a_place_and_draped() {
    let dir = tmp("hunneberg");
    let web = dir.join("web");
    let tile = "Copernicus_DSM_COG_10_N58_00_E012_00_DEM";
    for d in [web.join("dem").join(tile), web.join("geo"), web.join("stac"), web.join("s2")] {
        std::fs::create_dir_all(d).unwrap();
    }
    // A table mountain 100 m high around 58.33, 12.47.
    let hill = |lon: f64, lat: f64| if ((lon - 12.47) / 0.04).powi(2) + ((lat - 58.33) / 0.02).powi(2) < 1.0 { 150.0 } else { 50.0 };
    degrees_tif(&web.join("dem").join(tile).join(format!("{}.tif", tile)), 12.0, 59.0, 1.0 / 1200.0, 1200, hill);
    for (name, v) in [("red", 500.0f32), ("green", 900.0), ("blue", 400.0)] {
        degrees_tif(&web.join("s2").join(format!("{}.tif", name)), 12.38, 58.38, 0.0002, 900, |_, _| v);
    }
    degrees_tif(&web.join("s2").join("scl.tif"), 12.38, 58.38, 0.0002, 900, |_, _| 4.0);
    let (base, _) = file_server(web.clone());
    let item = format!(
        r#"{{"features": [{{"id": "clear", "properties": {{"eo:cloud_cover": 1, "datetime": "2025-07-01T10:00:00Z"}}, "assets": {{"red": {{"href": "{b}/s2/red.tif"}}, "green": {{"href": "{b}/s2/green.tif"}}, "blue": {{"href": "{b}/s2/blue.tif"}}, "scl": {{"href": "{b}/s2/scl.tif"}}}}}}]}}"#,
        b = base
    );
    std::fs::write(web.join("stac/search"), item).unwrap();
    // Overpass: a main road across the middle, and a lake north of it.
    let (overpass, _) = server(Arc::new(|r: &Req| {
        let body = if r.body.contains("highway") {
            r#"{"elements": [{"type": "way", "id": 1, "tags": {"highway": "primary", "name": "Väg 44"}, "geometry": [{"lat": 58.330, "lon": 12.42}, {"lat": 58.330, "lon": 12.52}]}]}"#
        } else {
            r#"{"elements": [{"type": "way", "id": 2, "tags": {"natural": "water", "name": "Sjön"}, "geometry": [{"lat": 58.336, "lon": 12.46}, {"lat": 58.336, "lon": 12.48}, {"lat": 58.341, "lon": 12.48}, {"lat": 58.341, "lon": 12.46}, {"lat": 58.336, "lon": 12.46}]}]}"#
        };
        (200, vec![], body.into())
    }));
    // The example as it is, with the places of the services and without the slow rendering.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apis/hunneberg_roads.ographis");
    let mut m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for n in m["nodes"].as_array_mut().unwrap() {
        let id = n["id"].as_str().unwrap().to_string();
        let p = &mut n["params"];
        match id.as_str() {
            "place" => {
                p["size"] = serde_json::json!("6");
                p["dem"] = serde_json::json!(format!("{}/dem", base));
                p["stac"] = serde_json::json!(format!("{}/stac", base));
            }
            "roads" | "water" => p["server"] = serde_json::json!(format!("{}/api/interpreter", overpass)),
            "map" => p["width"] = serde_json::json!("1200"),
            "scene" | "png" => n["disabled"] = serde_json::json!(true),
            _ => {}
        }
    }
    let ws = Workspace::from_json(&m.to_string()).unwrap();
    let r = run(&ws, Some(&GdalBackend::new()), &dir);
    assert_eq!(r.errors, 0, "{:?}", r.log);
    // The map: in the system of the picture, with the road in its yellow and the lake in blue.
    let map = r.output("map", "Map").unwrap();
    assert_eq!(map.crs, Some(32633));
    let img = map.image.as_ref().unwrap();
    let count = |rgba: &[u8], c: [u8; 3]| rgba.chunks_exact(4).filter(|p| (0..3).all(|k| (p[k] as i32 - c[k] as i32).abs() < 12)).count();
    assert!(count(&img.rgba, [0xff, 0xd2, 0x7a]) > 200, "the road is drawn");
    assert!(count(&img.rgba, [0x3f, 0x6f, 0x9e]) > 200, "the lake is drawn");
    // The road crosses the middle of the map, as the place lies on it; in UTM a parallel leans
    // a little, as north on the grid is not quite north on the Earth.
    let (mx, my) = (img.width / 2, img.height / 2);
    let yellow = |x: usize, y: usize| {
        let p = &img.rgba[(y * img.width + x) * 4..][..3];
        (p[0] as i32 - 0xff).abs() < 12 && (p[1] as i32 - 0xd2).abs() < 12
    };
    assert!((my - 15..my + 15).any(|y| yellow(mx, y)), "the road in its place");
    // The terrain carries the map, and the glTF is written.
    let model = r.output("model", "Model").unwrap().mesh.clone().unwrap();
    let tex = model.materials[0].texture.as_ref().expect("the map draped");
    assert!(count(&tex.rgba, [0xff, 0xd2, 0x7a]) > 50, "the road on the terrain");
    assert!(dir.join("out/apis/hunneberg.glb").exists());
}

/// A coverage service as GeoServer answers in WCS 1.0.0, with heights of 10 m cells over
/// 10 × 10 km that rise to the east: a tenth of a metre for each metre. GetCoverage makes a
/// GeoTIFF of the part asked for, at the size asked for.
fn wcs_server() -> (String, Arc<Mutex<Vec<String>>>) {
    const CAP: &str = r#"<?xml version="1.0"?><WCS_Capabilities version="1.0.0" xmlns="http://www.opengis.net/wcs" xmlns:gml="http://www.opengis.net/gml" xmlns:xlink="http://www.w3.org/1999/xlink">
<Service><name>WCS</name><label>Heights</label></Service>
<Capability><Request><GetCapabilities><DCPType><HTTP><Get><OnlineResource xlink:href="BASE?"/></Get></HTTP></DCPType></GetCapabilities>
<DescribeCoverage><DCPType><HTTP><Get><OnlineResource xlink:href="BASE?"/></Get></HTTP></DCPType></DescribeCoverage>
<GetCoverage><DCPType><HTTP><Get><OnlineResource xlink:href="BASE?"/></Get></HTTP></DCPType></GetCoverage></Request>
<Exception><Format>application/vnd.ogc.se_xml</Format></Exception></Capability>
<ContentMetadata>
<CoverageOfferingBrief><name>dem:heights</name><label>Heights 2022</label><lonLatEnvelope srsName="urn:ogc:def:crs:OGC:1.3:CRS84"><gml:pos>11.7 57.6</gml:pos><gml:pos>11.9 57.8</gml:pos></lonLatEnvelope></CoverageOfferingBrief>
<CoverageOfferingBrief><name>dem:slope</name><label>Slope</label><lonLatEnvelope srsName="urn:ogc:def:crs:OGC:1.3:CRS84"><gml:pos>11.7 57.6</gml:pos><gml:pos>11.9 57.8</gml:pos></lonLatEnvelope></CoverageOfferingBrief>
</ContentMetadata></WCS_Capabilities>"#;
    const DESC: &str = r#"<?xml version="1.0"?><CoverageDescription version="1.0.0" xmlns="http://www.opengis.net/wcs" xmlns:gml="http://www.opengis.net/gml">
<CoverageOffering><name>NAME</name><label>Heights 2022</label>
<lonLatEnvelope srsName="urn:ogc:def:crs:OGC:1.3:CRS84"><gml:pos>11.7 57.6</gml:pos><gml:pos>11.9 57.8</gml:pos></lonLatEnvelope>
<domainSet><spatialDomain><gml:Envelope srsName="EPSG:3006"><gml:pos>300000 6400000</gml:pos><gml:pos>310000 6410000</gml:pos></gml:Envelope>
<gml:RectifiedGrid dimension="2" srsName="EPSG:3006"><gml:limits><gml:GridEnvelope><gml:low>0 0</gml:low><gml:high>999 999</gml:high></gml:GridEnvelope></gml:limits>
<gml:axisName>x</gml:axisName><gml:axisName>y</gml:axisName><gml:origin><gml:pos>300005 6409995</gml:pos></gml:origin>
<gml:offsetVector>10 0</gml:offsetVector><gml:offsetVector>0 -10</gml:offsetVector></gml:RectifiedGrid></spatialDomain></domainSet>
<rangeSet><RangeSet><name>height</name><label>height</label></RangeSet></rangeSet>
<supportedCRSs><requestResponseCRSs>EPSG:3006</requestResponseCRSs></supportedCRSs>
<supportedFormats><formats>GeoTIFF</formats></supportedFormats></CoverageOffering></CoverageDescription>"#;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}/geoserver/wcs", addr);
    let log = Arc::new(Mutex::new(Vec::new()));
    let kept = log.clone();
    let here = base.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (kept, here) = (kept.clone(), here.clone());
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    return;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                kept.lock().unwrap().push(path.clone());
                let query: HashMap<String, String> = path
                    .split_once('?')
                    .map(|(_, q)| q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_lowercase(), v.replace("%3A", ":"))).collect())
                    .unwrap_or_default();
                let (kind, body): (&str, Vec<u8>) = match query.get("request").map(|r| r.to_lowercase()).as_deref() {
                    Some("getcapabilities") => ("text/xml", CAP.replace("BASE", &here).into_bytes()),
                    Some("describecoverage") => ("text/xml", DESC.replace("NAME", query.get("coverage").map_or("dem:heights", |c| c.as_str())).into_bytes()),
                    Some("getcoverage") => {
                        let b: Vec<f64> = query["bbox"].split(',').map(|v| v.parse().unwrap()).collect();
                        let (w, h): (usize, usize) = (query["width"].parse().unwrap(), query["height"].parse().unwrap());
                        let (dx, dy) = ((b[2] - b[0]) / w as f64, (b[3] - b[1]) / h as f64);
                        let values: Vec<f32> = (0..w * h).map(|i| ((b[0] + ((i % w) as f64 + 0.5) * dx - 300000.0) / 10.0) as f32).collect();
                        let file = std::env::temp_dir().join(format!("og-wcs-{}-{}.tif", std::process::id(), kept.lock().unwrap().len()));
                        {
                            let driver = gdal::DriverManager::get_driver_by_name("GTiff").unwrap();
                            let mut ds = driver.create_with_band_type::<f32, _>(&file, w, h, 1).unwrap();
                            ds.set_geo_transform(&[b[0], dx, 0.0, b[3], 0.0, -dy]).unwrap();
                            ds.set_spatial_ref(&gdal::spatial_ref::SpatialRef::from_epsg(3006).unwrap()).unwrap();
                            let mut buf = gdal::raster::Buffer::new((w, h), values);
                            ds.rasterband(1).unwrap().write((0, 0), (w, h), &mut buf).unwrap();
                        }
                        let bytes = std::fs::read(&file).unwrap();
                        let _ = std::fs::remove_file(&file);
                        ("image/tiff", bytes)
                    }
                    _ => ("text/plain", b"?".to_vec()),
                };
                let mut s = stream;
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", kind, body.len());
                let _ = s.write_all(&body);
            });
        }
    });
    (base, log)
}

#[test]
fn a_coverage_of_a_wcs_is_read_within_a_rectangle() {
    let dir = tmp("wcs");
    let b = GdalBackend::new();
    let (url, log) = wcs_server();
    // The coverages are the layers of the service.
    use og_engine::io::Backend;
    let service = format!("WCS:{}?version=1.0.0", url);
    let names: Vec<String> = b.layers(std::path::Path::new(&service)).unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["dem:heights", "dem:slope"]);
    assert_eq!(og_engine::nodes::service_name(&format!("{}?service=WCS&request=GetCapabilities", url), None).as_deref().map(|s| &s[..4]), Some("WCS:"));
    let ws = model(
        &format!(
            r#"{{"id": "grid", "type": "Reader", "params": {{"path": "{u}?service=WCS&version=1.0.0", "layer": "dem:heights", "raster_as": "grid",
                  "within": "rectangle", "min_x": "302000", "min_y": "6405000", "max_x": "303000", "max_y": "6406000"}}}},
               {{"id": "points", "type": "Reader", "params": {{"path": "{u}", "web_as": "wcs", "raster_as": "points",
                  "within": "rectangle", "min_x": "302000", "min_y": "6405000", "max_x": "302100", "max_y": "6405100"}}}},
               {{"id": "whole", "type": "Reader", "params": {{"path": "{u}?service=WCS&version=1.0.0&coverage=dem:heights", "raster_as": "grid"}}}}"#,
            u = url
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    let grid = r.output("grid", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["grid"].error)).grid.clone().expect("a grid");
    assert_eq!((grid.width, grid.height), (100, 100));
    assert_eq!(grid.crs, Some(3006));
    assert!((grid.geotransform[0] - 302000.0).abs() < 1e-6 && (grid.geotransform[3] - 6406000.0).abs() < 1e-6, "{:?}", grid.geotransform);
    assert!((grid.values[0] - 200.5).abs() < 0.01 && (grid.values[99] - 299.5).abs() < 0.01, "{} {}", grid.values[0], grid.values[99]);
    // Points of the cells, from the first coverage when none is chosen.
    let points = r.output("points", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["points"].error));
    assert_eq!(points.len(), 100);
    assert!(r.nodes["points"].warnings.is_empty());
    assert!(r.log.iter().any(|l| l.msg.contains("the first of 2 coverages, dem:heights")), "{:?}", r.log);
    // Without a rectangle the whole coverage is not fetched.
    assert!(r.nodes["whole"].error.as_deref().is_some_and(|e| e.contains("within a rectangle")), "{:?}", r.nodes["whole"].error);
    // Only the parts were asked for, not the whole coverage of 1000 × 1000 cells.
    let asked: Vec<String> = log.lock().unwrap().iter().filter(|p| p.to_lowercase().contains("request=getcoverage")).cloned().collect();
    assert!(!asked.is_empty());
    for a in &asked {
        let size = |k: &str| a.split('&').find_map(|kv| kv.strip_prefix(k)).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        assert!(size("WIDTH=") <= 101 && size("HEIGHT=") <= 101, "{}", a);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server of files that answers HEAD and parts asked for with Range, as GDAL reads files on
/// the web, and wants a login for those under /dl/. Also the token, and JSON by address.
type Routes = Arc<Mutex<HashMap<String, (String, Vec<u8>)>>>;
fn ranged_server(log: Arc<Mutex<Vec<String>>>) -> (String, Routes) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes: Routes = Arc::new(Mutex::new(HashMap::new()));
    let kept = routes.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (routes, log) = (kept.clone(), log.clone());
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut first = String::new();
                    if reader.read_line(&mut first).unwrap_or(0) == 0 {
                        return;
                    }
                    let mut headers: HashMap<String, String> = HashMap::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some((k, v)) = line.split_once(':') {
                            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
                        }
                    }
                    let mut sent = String::new();
                    if let Some(n) = headers.get("content-length").and_then(|n| n.parse::<usize>().ok()) {
                        let mut body = vec![0u8; n];
                        let _ = reader.read_exact(&mut body);
                        sent = String::from_utf8_lossy(&body).into_owned();
                    }
                    let mut parts = first.split_whitespace();
                    let method = parts.next().unwrap_or("").to_string();
                    let path = parts.next().unwrap_or("/").to_string();
                    let route = path.split('?').next().unwrap_or("").to_string();
                    let authorized = headers.get("authorization").is_some_and(|a| a == "Bearer tok-123");
                    // A search that takes only POST, and only with the login, as those of the
                    // national geodata platform: the page asked for is in the body.
                    if route == "/ngp/v1/search" {
                        let mut s = stream.try_clone().unwrap();
                        let answer = if method != "POST" {
                            None
                        } else if !authorized {
                            Some(None)
                        } else {
                            let page = if sent.contains("\"page\":2") { "#2" } else { "#1" };
                            Some(routes.lock().unwrap().get(&format!("{}{}", route, page)).cloned())
                        };
                        log.lock().unwrap().push(format!("{} {} {}", method, path, sent));
                        let _ = match answer {
                            None => write!(s, "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n"),
                            Some(None) => write!(s, "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n"),
                            Some(Some((_, body))) => {
                                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/geo+json\r\nContent-Length: {}\r\n\r\n", body.len());
                                s.write_all(&body)
                            }
                        };
                        continue;
                    }
                    log.lock().unwrap().push(format!("{} {} {}", method, path, if authorized { "with login" } else { "" }));
                    // A file sent on to storage elsewhere, which refuses a login of its own,
                    // as storage with signed addresses does.
                    if route.starts_with("/dl/moved/") && authorized {
                        let to = route.replace("/dl/moved/", "/storage/");
                        let mut s = stream.try_clone().unwrap();
                        let _ = write!(s, "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\n\r\n", to);
                        continue;
                    }
                    if route.starts_with("/storage/") && headers.contains_key("authorization") {
                        let mut s = stream.try_clone().unwrap();
                        let _ = write!(s, "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
                        continue;
                    }
                    let found = if route == "/token" {
                        Some(("application/json".to_string(), br#"{"access_token": "tok-123", "expires_in": 3600}"#.to_vec()))
                    } else if route.starts_with("/dl/") && !authorized {
                        None
                    } else {
                        // JSON by the address with its query, or a file by its path.
                        let r = routes.lock().unwrap();
                        r.get(&path).or_else(|| r.get(&route)).cloned()
                    };
                    let mut s = stream.try_clone().unwrap();
                    let Some((kind, body)) = found else {
                        let status = if route.starts_with("/dl/") { "401 Unauthorized" } else { "404 Not Found" };
                        let _ = write!(s, "HTTP/1.1 {}\r\nContent-Length: 0\r\n\r\n", status);
                        continue;
                    };
                    let range = headers.get("range").and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.split_once('-')).map(|(a, b)| {
                        let a: usize = a.parse().unwrap_or(0);
                        let b: usize = b.parse().unwrap_or(body.len() - 1).min(body.len() - 1);
                        (a, b)
                    });
                    let (status, piece, extra) = match range {
                        Some((a, b)) if a < body.len() => ("206 Partial Content", &body[a..=b], format!("Content-Range: bytes {}-{}/{}\r\n", a, b, body.len())),
                        _ => ("200 OK", &body[..], String::new()),
                    };
                    // A kind written as type|name gives the name of the file as the server would.
                    let (kind, extra) = match kind.split_once('|') {
                        Some((k, name)) => (k.to_string(), format!("{}Content-Disposition: attachment; filename=\"{}\"\r\n", extra, name)),
                        None => (kind, extra),
                    };
                    let _ =
                        write!(s, "HTTP/1.1 {}\r\nContent-Type: {}\r\nAccept-Ranges: bytes\r\n{}Content-Length: {}\r\n\r\n", status, kind, extra, piece.len());
                    if method != "HEAD" {
                        let _ = s.write_all(piece);
                    }
                }
            });
        }
    });
    (base, routes)
}

/// A GeoTIFF in SWEREF 99 TM, of `bands` bands made by `value`, as bytes.
fn geotiff(gt: [f64; 6], w: usize, h: usize, bands: usize, byte: bool, value: impl Fn(usize, usize, usize) -> f64) -> Vec<u8> {
    let file = std::env::temp_dir().join(format!("og-geodata-{}-{}.tif", std::process::id(), gt[0] as i64 + gt[3] as i64 + bands as i64 * 7 + w as i64));
    {
        let driver = gdal::DriverManager::get_driver_by_name("GTiff").unwrap();
        let mut ds = if byte {
            // Red, green, blue and infrared, as photos from above are: the fourth is not alpha.
            let mut list = gdal::cpl::CslStringList::new();
            list.set_name_value("PHOTOMETRIC", "RGB").unwrap();
            list.set_name_value("ALPHA", "UNSPECIFIED").unwrap();
            driver.create_with_band_type_with_options::<u8, _>(&file, w, h, bands, &list).unwrap()
        } else {
            driver.create_with_band_type::<f32, _>(&file, w, h, bands).unwrap()
        };
        ds.set_geo_transform(&gt).unwrap();
        ds.set_spatial_ref(&gdal::spatial_ref::SpatialRef::from_epsg(3006).unwrap()).unwrap();
        for b in 0..bands {
            let mut band = ds.rasterband(b + 1).unwrap();
            if byte {
                let mut buf = gdal::raster::Buffer::new((w, h), (0..w * h).map(|i| value(b, i % w, i / w) as u8).collect());
                band.write((0, 0), (w, h), &mut buf).unwrap();
            } else {
                let mut buf = gdal::raster::Buffer::new((w, h), (0..w * h).map(|i| value(b, i % w, i / w) as f32).collect());
                band.write((0, 0), (w, h), &mut buf).unwrap();
            }
        }
    }
    let bytes = std::fs::read(&file).unwrap();
    let _ = std::fs::remove_file(&file);
    bytes
}

#[test]
fn geodata_is_fetched_from_the_catalogue_of_a_provider_with_its_login() {
    let _turn = SETTINGS_TURN.lock().unwrap_or_else(|e| e.into_inner());
    use og_engine::io::{Backend, WriteOpts};
    let dir = tmp("geodata");
    let b = GdalBackend::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let (base, routes) = ranged_server(log.clone());
    // Where a rectangle of SWEREF 99 TM lies in degrees, as the catalogue gives it.
    let degrees = |r: [f64; 4]| {
        let (mut xs, mut ys) = (vec![r[0], r[2], r[0], r[2]], vec![r[1], r[1], r[3], r[3]]);
        b.transform(3006, 4326, &mut xs, &mut ys).unwrap();
        let lo = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        [lo(&xs), lo(&ys), hi(&xs), hi(&ys)]
    };
    let item = |id: &str, collection: &str, when: &str, r: [f64; 4], file: &str| {
        let d = degrees(r);
        serde_json::json!({"type": "Feature", "id": id, "collection": collection, "bbox": d,
            "geometry": {"type": "Polygon", "coordinates": [[[d[0], d[1]], [d[2], d[1]], [d[2], d[3]], [d[0], d[3]], [d[0], d[1]]]]},
            "properties": {"datetime": when}, "assets": {"data": {"href": format!("{}/dl/{}", base, file)}}})
    };
    let json = |v: serde_json::Value| ("application/json".to_string(), v.to_string().into_bytes());
    {
        let mut r = routes.lock().unwrap();
        // Heights: two tiles of 2024 side by side, and an older one under both.
        r.insert(
            "/stac-hojd/v1/collections".into(),
            json(serde_json::json!({"collections": [
                {"id": "laserdata-skog", "title": "Laserdata skog", "license": "CC-BY-4.0"},
                {"id": "dtm-cog", "title": "Markhöjdmodell, 1 m", "license": "CC-BY-4.0"}]})),
        );
        let tiles = serde_json::json!({"type": "FeatureCollection", "features": [
            item("old", "dtm-cog", "2019-05-01T00:00:00Z", [300000.0, 6400000.0, 300200.0, 6400100.0], "old.tif"),
            item("a", "dtm-cog", "2024-05-01T00:00:00Z", [300000.0, 6400000.0, 300100.0, 6400100.0], "a.tif"),
            item("b", "dtm-cog", "2024-05-01T00:00:00Z", [300100.0, 6400000.0, 300200.0, 6400100.0], "b.tif")]});
        r.insert("/stac-hojd/v1/search".into(), json(tiles));
        // Another, read without the login.
        r.insert("/closed/v1/collections".into(), json(serde_json::json!({"collections": [{"id": "dtm-cog"}]})));
        r.insert(
            "/closed/v1/search".into(),
            json(serde_json::json!({"features": [item("c", "dtm-cog", "2024-05-01T00:00:00Z", [300000.0, 6400000.0, 300200.0, 6400100.0], "c.tif")]})),
        );
        r.insert("/moved/v1/collections".into(), json(serde_json::json!({"collections": [{"id": "dtm-cog"}]})));
        r.insert(
            "/moved/v1/search".into(),
            json(serde_json::json!({"features": [item("m", "dtm-cog", "2024-05-01T00:00:00Z", [300000.0, 6400000.0, 300200.0, 6400100.0], "moved/m.tif")]})),
        );
        r.insert(
            "/storage/m.tif".into(),
            ("image/tiff".into(), geotiff([300000.0, 1.0, 0.0, 6400100.0, 0.0, -1.0], 200, 100, 1, false, |_, x, _| 30.0 + x as f64)),
        );
        r.insert("/dl/c.tif".into(), ("image/tiff".into(), geotiff([300000.0, 1.0, 0.0, 6400100.0, 0.0, -1.0], 200, 100, 1, false, |_, _, _| 5.0)));
        r.insert("/dl/a.tif".into(), ("image/tiff".into(), geotiff([300000.0, 1.0, 0.0, 6400100.0, 0.0, -1.0], 100, 100, 1, false, |_, _, _| 10.0)));
        r.insert("/dl/b.tif".into(), ("image/tiff".into(), geotiff([300100.0, 1.0, 0.0, 6400100.0, 0.0, -1.0], 100, 100, 1, false, |_, _, _| 20.0)));
        r.insert("/dl/old.tif".into(), ("image/tiff".into(), geotiff([300000.0, 1.0, 0.0, 6400100.0, 0.0, -1.0], 200, 100, 1, false, |_, _, _| 999.0)));
        // A photo of red, green, blue and infrared.
        r.insert(
            "/stac-bild/v1/collections".into(),
            json(serde_json::json!({"collections": [{"id": "ortofoto-ir", "title": "Ortofoto IR"}, {"id": "ortofoto", "title": "Ortofoto färg"}]})),
        );
        r.insert(
            "/stac-bild/v1/search".into(),
            json(serde_json::json!({"type": "FeatureCollection", "features": [item("p", "ortofoto", "2023-06-01T00:00:00Z", [300000.0, 6400000.0, 300200.0, 6400100.0], "p.tif")]})),
        );
        r.insert(
            "/dl/p.tif".into(),
            ("image/tiff".into(), geotiff([300000.0, 0.5, 0.0, 6400100.0, 0.0, -0.5], 400, 200, 4, true, |band, _, _| [200.0, 100.0, 50.0, 7.0][band])),
        );
    }
    // A point cloud and a zip with a GeoPackage of two layers.
    let mut points = Table { fields: vec![og_engine::Field::new("classification", og_engine::FieldType::Int)], crs: Some(3006), ..Default::default() };
    for i in 0..100 {
        let (x, y) = (300000.0 + (i % 10) as f64 * 20.0 + 5.0, 6400000.0 + (i / 10) as f64 * 10.0 + 5.0);
        points.features.push(og_engine::Feature {
            attrs: vec![Value::Int(if i % 2 == 0 { 2 } else { 5 })],
            geom: Some(geo::Geometry::Point(geo::Point::new(x, y))),
            z: Some(vec![50.0].into()),
        });
    }
    let laz = dir.join("forest.laz");
    og_engine::pointcloud::write_las(&points, &laz, None).unwrap();
    let gpkg = dir.join("topo.gpkg");
    let mut notes = og_engine::io::Notes::default();
    let roads = Table {
        fields: vec![og_engine::Field::new("kind", og_engine::FieldType::Text)],
        crs: Some(3006),
        features: vec![
            og_engine::Feature {
                attrs: vec![Value::Text("road".into())],
                geom: Some(geo::Geometry::LineString(geo::LineString::from(vec![(300010.0, 6400050.0), (300090.0, 6400050.0)]))),
                z: None,
            },
            og_engine::Feature {
                attrs: vec![Value::Text("far".into())],
                geom: Some(geo::Geometry::LineString(geo::LineString::from(vec![(310010.0, 6410050.0), (310090.0, 6410050.0)]))),
                z: None,
            },
        ],
        ..Default::default()
    };
    b.write(&roads, &gpkg, &WriteOpts { layer: Some("vagnat".into()), ..Default::default() }, &mut notes).unwrap();
    let houses = Table {
        fields: vec![og_engine::Field::new("use", og_engine::FieldType::Text)],
        crs: Some(3006),
        features: vec![og_engine::Feature {
            attrs: vec![Value::Text("home".into())],
            geom: Some(geo::Geometry::Point(geo::Point::new(300050.0, 6400050.0))),
            z: None,
        }],
        ..Default::default()
    };
    b.write(&houses, &gpkg, &WriteOpts { layer: Some("byggnad".into()), ..Default::default() }, &mut notes).unwrap();
    let zip_file = dir.join("topo.zip");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_file).unwrap());
        z.start_file("topografi10_test.gpkg", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(&std::fs::read(&gpkg).unwrap()).unwrap();
        z.finish().unwrap();
    }
    {
        let mut r = routes.lock().unwrap();
        r.insert(
            "/stac-hojd/v1/search?collections=laserdata-skog".into(),
            ("application/json".into(), serde_json::json!({"features": [item("l", "laserdata-skog", "2022-01-01T00:00:00Z", [300000.0, 6400000.0, 300200.0, 6400100.0], "forest.laz")]}).to_string().into_bytes()),
        );
        r.insert("/dl/forest.laz".into(), ("application/octet-stream".into(), std::fs::read(&laz).unwrap()));
        r.insert(
            "/stac-vektor/v1/collections".into(),
            json(serde_json::json!({"collections": [{"id": "topografi-100", "title": "Topografi 100"}, {"id": "topografi-10", "title": "Topografi 10"}]})),
        );
        r.insert(
            "/stac-vektor/v1/search".into(),
            json(serde_json::json!({"features": [item("t", "topografi-10", "2025-01-01T00:00:00Z", [300000.0, 6400000.0, 300200.0, 6400100.0], "topo.zip")]})),
        );
        r.insert("/dl/topo.zip".into(), ("application/zip".into(), std::fs::read(&zip_file).unwrap()));
    }
    // The search for the laser data is told apart by its collection.
    let laser_search = {
        let r = routes.lock().unwrap();
        r.get("/stac-hojd/v1/search?collections=laserdata-skog").cloned().unwrap()
    };
    use_settings(Some(Settings {
        apis: vec![ApiConnection {
            name: "LM".into(),
            auth: "oauth2".into(),
            user: "key".into(),
            secret: "secret".into(),
            token_url: format!("{}/token", base),
            ..Default::default()
        }],
        ..Default::default()
    }));
    let area = r#""bbox": "300050, 6400020, 300150, 6400080", "connection": "LM""#;
    let ws = model(
        &format!(
            r#"{{"id": "heights", "type": "GeodataFetcher", "params": {{"product": "lm_heights", "catalogue": "{b}/stac-hojd/v1", {a}}}}},
               {{"id": "photo", "type": "GeodataFetcher", "params": {{"product": "lm_photo", "catalogue": "{b}/stac-bild/v1", "cell_size": "1", {a}}}}},
               {{"id": "topo", "type": "GeodataFetcher", "params": {{"product": "lm_topo10", "catalogue": "{b}/stac-vektor/v1", "layers": "vagnat, byggnad", {a}}}}},
               {{"id": "nologin", "type": "GeodataFetcher", "params": {{"product": "lm_heights", "catalogue": "{b}/closed/v1", "bbox": "300050, 6400020, 300150, 6400080"}}}},
               {{"id": "moved", "type": "GeodataFetcher", "params": {{"product": "lm_heights", "catalogue": "{b}/moved/v1", {a}}}}},
               {{"id": "large", "type": "GeodataFetcher", "params": {{"product": "lm_heights", "catalogue": "{b}/stac-hojd/v1", "bbox": "300000, 6400000, 310000, 6410000", "connection": "LM"}}}}"#,
            b = base,
            a = area
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    let ok = |id: &str| r.output(id, "Output").unwrap_or_else(|| panic!("{}: {:?}", id, r.nodes[id].error)).clone();
    // Heights: from the tiles of 2024, the older one not read where they cover it.
    let grid = ok("heights").grid.clone().expect("a grid");
    assert_eq!((grid.width, grid.height, grid.crs), (100, 60, Some(3006)));
    assert!(grid.values.iter().all(|v| *v == 10.0 || *v == 20.0), "{:?}", &grid.values[..5]);
    assert_eq!(grid.values[0], 10.0);
    assert_eq!(grid.values[99], 20.0);
    let files = r.output("heights", "Files").unwrap();
    assert_eq!(files.len(), 3);
    let used = files.field_index("used").unwrap();
    assert_eq!(files.features.iter().map(|f| f.attrs[used].clone()).collect::<Vec<_>>(), vec![Value::Int(1), Value::Int(1), Value::Int(0)]);
    // A file sent on to storage that refuses the login is fetched without it.
    let moved = ok("moved").grid.clone().expect("a grid");
    assert_eq!((moved.width, moved.height), (100, 60));
    assert!((moved.values[0] - 80.0).abs() < 1.0, "{}", moved.values[0]);
    assert!(r.log.iter().any(|l| l.node.as_deref() == Some("moved") && l.msg.contains("fetched whole instead")));
    // The photo: red, green and blue, not the infrared, at the cell size asked for.
    let photo = ok("photo").image.clone().expect("a picture");
    assert_eq!((photo.width, photo.height), (100, 60));
    let clear = photo.rgba.chunks(4).filter(|p| p[3] == 0).count();
    assert_eq!(
        &photo.rgba[..4],
        &[200, 100, 50, 255],
        "{} clear; {:?}; {:?}",
        clear,
        r.nodes["photo"].warnings,
        r.log.iter().filter(|l| l.node.as_deref() == Some("photo")).map(|l| &l.msg).collect::<Vec<_>>()
    );
    assert!(r.log.iter().any(|l| l.msg.contains("Collection: ortofoto, Ortofoto färg")), "{:?}", r.log.iter().map(|l| &l.msg).collect::<Vec<_>>());
    // Topografi 10, not 100: both layers, only what reaches into the area.
    let topo = ok("topo");
    let layer = topo.field_index("layer").unwrap();
    let mut layers: Vec<String> = topo.features.iter().map(|f| f.attrs[layer].to_text()).collect();
    layers.sort();
    assert_eq!(layers, ["byggnad", "vagnat"]);
    assert_eq!(topo.crs, Some(3006));
    // Without the login the files are refused, and it is said.
    assert!(r.nodes["nologin"].error.as_deref().is_some_and(|e| e.contains("401") && e.contains("choose an API connection")), "{:?}", r.nodes["nologin"].error);
    assert!(r.nodes["large"].error.as_deref().is_some_and(|e| e.contains("100 km²")), "{:?}", r.nodes["large"].error);
    // The token was fetched with the key, and the files with the token.
    let asked = log.lock().unwrap().clone();
    assert!(asked.iter().any(|l| l.starts_with("POST /token")));
    assert!(asked.iter().any(|l| l.contains("/dl/a.tif with login")));
    // Laser data: the search tells the collection; the file is fetched whole.
    {
        let mut r2 = routes.lock().unwrap();
        r2.insert("/stac-hojd/v1/search".into(), laser_search);
    }
    let ws = model(
        &format!(
            r#"{{"id": "laser", "type": "GeodataFetcher", "params": {{"product": "lm_laser_forest", "catalogue": "{b}/stac-hojd/v1", "las_classes": "2", {a}}}}}"#,
            b = base,
            a = area
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    let cloud = r.output("laser", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["laser"].error)).clone();
    let (c, _) = og_engine::cloud::cloud_of(&cloud);
    // The points of class 2 in the area: 5 of the 10 columns, 6 rows, every other point.
    assert!(!c.is_empty() && c.len() < 50, "{}", c.len());
    assert!(c.class.iter().all(|k| *k == 2));
    assert!(c.x.iter().all(|x| (300050.0..=300150.0).contains(x)));
    // How many points there will be is told before they are fetched; more than allowed are
    // not read, and it is said what to do.
    let ws = model(
        &format!(
            r#"{{"id": "many", "type": "GeodataFetcher", "params": {{"product": "lm_laser_forest", "catalogue": "{b}/stac-hojd/v1", "max_points": "10", {a}}}}},
               {{"id": "thin", "type": "GeodataFetcher", "params": {{"product": "lm_laser_forest", "catalogue": "{b}/stac-hojd/v1", "las_every": "2", {a}}}}},
               {{"id": "capped", "type": "GeodataFetcher", "params": {{"product": "lm_laser_forest", "catalogue": "{b}/stac-hojd/v1", "las_classes": "2, 5", "max_points": "5", {a}}}}}"#,
            b = base,
            a = area
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    let many = r.nodes["many"].error.clone().unwrap_or_default();
    assert!(many.contains("about") && many.contains("Most points"), "{}", many);
    assert!(r.log.iter().any(|l| l.node.as_deref() == Some("many") && l.msg.starts_with("About ")));
    let thin = og_engine::cloud::cloud_of(r.output("thin", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["thin"].error))).0.len();
    let full = og_engine::cloud::cloud_of(&cloud).0.len() * 2;
    assert!(thin > 0 && thin <= full, "{} of {}", thin, full);
    assert!(r.nodes["capped"].error.as_deref().is_some_and(|e| e.contains("more than the 5 points")), "{:?}", r.nodes["capped"].error);
    // Every file of the area kept in a folder, the photo and its PDF too, and read from there.
    {
        let mut r2 = routes.lock().unwrap();
        let d = degrees([300000.0, 6400000.0, 300200.0, 6400100.0]);
        r2.insert("/kept/v1/collections".into(), ("application/json".into(), br#"{"collections": [{"id": "ortofoto", "title": "Ortofoto"}]}"#.to_vec()));
        r2.insert(
            "/kept/v1/search".into(),
            (
                "application/json".into(),
                serde_json::json!({"features": [{"type": "Feature", "id": "k", "bbox": d, "properties": {"datetime": "2023-06-01T00:00:00Z"},
                    "assets": {"data": {"href": format!("{}/dl/k.tif", base)}, "map": {"href": format!("{}/dl/k.pdf", base)}}}]})
                .to_string()
                .into_bytes(),
            ),
        );
        r2.insert(
            "/dl/k.tif".into(),
            ("image/tiff".into(), geotiff([300000.0, 0.5, 0.0, 6400100.0, 0.0, -0.5], 400, 200, 3, true, |band, _, _| [10.0, 20.0, 30.0][band])),
        );
        r2.insert("/dl/k.pdf".into(), ("application/pdf".into(), b"%PDF-1.4 a map".to_vec()));
    }
    let ws = model(
        &format!(
            r#"{{"id": "kept", "type": "GeodataFetcher", "params": {{"product": "lm_photo", "catalogue": "{b}/kept/v1", "cell_size": "1", "keep_in": "kept", {a}}}}}"#,
            b = base,
            a = area
        ),
        "",
    );
    let r = run(&ws, Some(&b), &dir);
    let photo = r.output("kept", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["kept"].error)).image.clone().unwrap();
    assert_eq!(&photo.rgba[..4], &[10, 20, 30, 255]);
    assert!(dir.join("kept/ortofoto/k.tif").is_file() && dir.join("kept/ortofoto/k.pdf").is_file());
    let files = r.output("kept", "Files").unwrap();
    let file = files.field_index("file").unwrap();
    assert!(files.features[0].attrs[file].to_text().ends_with("k.tif"));
    // Again: from the folder, nothing fetched.
    let asked = log.lock().unwrap().len();
    let r = run(&ws, Some(&b), &dir);
    assert!(r.output("kept", "Output").is_some());
    assert!(log.lock().unwrap()[asked..].iter().all(|l| !l.contains("/dl/k.")), "{:?}", &log.lock().unwrap()[asked..]);
    assert!(r.log.iter().any(|l| l.msg.contains("0 files fetched and kept, 2 there already")));
    use_settings(None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The plans of the national geodata platform: a catalogue of one dataset, searched with POST
/// and the login, whose items are in SWEREF 99 TM and are the features themselves; their
/// files, given by the server's own names, kept beside. And a scanned map with a table of
/// colours, as the national military map.
#[test]
fn plans_are_items_and_their_files_are_kept_and_a_map_of_colours_is_a_picture() {
    let _turn = SETTINGS_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp("ngp");
    let b = GdalBackend::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let (base, routes) = ranged_server(log.clone());
    let plan = |id: &str, x: f64, name: &str, asset: &str| {
        serde_json::json!({"type": "Feature", "id": id, "bbox": [x, 6400000.0, x + 100.0, 6400100.0],
            "geometry": {"type": "Polygon", "coordinates": [[[x, 6400000.0], [x + 100.0, 6400000.0], [x + 100.0, 6400100.0], [x, 6400100.0], [x, 6400000.0]]]},
            "properties": {"namn": name, "status": "laga kraft", "antal": 2, "beslut": {"datum": "2024-01-01"}},
            "assets": {"plankarta": {"href": format!("{}/dl/asset/{}", base, asset), "title": format!("Plankarta {}", name), "type": "application/pdf", "roles": ["plankarta", "data"]},
                       "beskrivning": {"href": format!("{}/dl/asset/b", base), "title": "Planbeskrivning", "type": "application/pdf"}}})
    };
    {
        let mut r = routes.lock().unwrap();
        r.insert(
            "/ngp/v1/search#1".into(),
            (
                "".into(),
                serde_json::json!({"type": "FeatureCollection", "features": [plan("p1", 300000.0, "Centrum", "a1"), plan("p2", 300100.0, "Hamnen", "a2")],
                "links": [{"rel": "next", "href": format!("{}/ngp/v1/search", base), "method": "POST", "merge": true, "body": {"page": 2}}]})
                .to_string()
                .into_bytes(),
            ),
        );
        r.insert(
            "/ngp/v1/search#2".into(),
            ("".into(), serde_json::json!({"type": "FeatureCollection", "features": [plan("p3", 300050.0, "Torget", "a3")]}).to_string().into_bytes()),
        );
        for a in ["a1", "a2", "a3"] {
            r.insert(format!("/dl/asset/{}", a), ("application/pdf|plankarta.pdf".into(), format!("%PDF-1.4 {}", a).into_bytes()));
        }
        r.insert("/dl/asset/b".into(), ("application/pdf|planbeskrivning.pdf".into(), b"%PDF-1.4 b".to_vec()));
        // A map of four colours, two by two cells of 10 m.
        r.insert(
            "/stac-karta/v1/collections".into(),
            ("application/json".into(), br#"{"collections": [{"id": "nmk250"}, {"id": "nmk50", "title": "NMK50"}]}"#.to_vec()),
        );
    }
    let file = std::env::temp_dir().join(format!("og-nmk-{}.tif", std::process::id()));
    {
        let driver = gdal::DriverManager::get_driver_by_name("GTiff").unwrap();
        let mut ds = driver.create_with_band_type::<u8, _>(&file, 20, 10, 1).unwrap();
        ds.set_geo_transform(&[300000.0, 10.0, 0.0, 6400100.0, 0.0, -10.0]).unwrap();
        ds.set_spatial_ref(&gdal::spatial_ref::SpatialRef::from_epsg(3006).unwrap()).unwrap();
        let mut band = ds.rasterband(1).unwrap();
        let mut table = gdal::raster::ColorTable::new(gdal::raster::PaletteInterpretation::Rgba);
        table.set_color_entry(0, &gdal::raster::ColorEntry::rgba(0, 0, 0, 255));
        table.set_color_entry(1, &gdal::raster::ColorEntry::rgba(30, 120, 200, 255));
        band.set_color_table(&table);
        let mut buf = gdal::raster::Buffer::new((20, 10), vec![1u8; 200]);
        band.write((0, 0), (20, 10), &mut buf).unwrap();
    }
    let degrees = {
        use og_engine::io::Backend;
        let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = (vec![300000.0, 300200.0], vec![6400000.0, 6400100.0]);
        b.transform(3006, 4326, &mut xs, &mut ys).unwrap();
        [xs[0].min(xs[1]), ys[0].min(ys[1]), xs[0].max(xs[1]), ys[0].max(ys[1])]
    };
    {
        let mut r = routes.lock().unwrap();
        r.insert(
            "/stac-karta/v1/search".into(),
            ("application/json".into(), serde_json::json!({"features": [{"type": "Feature", "id": "m", "collection": "nmk50", "bbox": degrees, "geometry": null,
                "properties": {"datetime": "2020-01-01T00:00:00Z"}, "assets": {"data": {"href": format!("{}/dl/m.tif", base)}, "pdf": {"href": format!("{}/dl/m.pdf", base)}}}]}).to_string().into_bytes()),
        );
        r.insert("/dl/m.tif".into(), ("image/tiff".into(), std::fs::read(&file).unwrap()));
        r.insert("/dl/m.pdf".into(), ("application/pdf".into(), b"%PDF-1.4 map".to_vec()));
    }
    let _ = std::fs::remove_file(&file);
    use_settings(Some(Settings {
        apis: vec![ApiConnection {
            name: "LM".into(),
            auth: "oauth2".into(),
            user: "key".into(),
            secret: "secret".into(),
            token_url: format!("{}/token", base),
            ..Default::default()
        }],
        ..Default::default()
    }));
    // Two municipalities on Extent, each fetched on its own with its name.
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("areas.csv"),
        "kommun;wkt\nVästby;POLYGON((300000 6400000,300200 6400000,300200 6400100,300000 6400100,300000 6400000))\nÖstby;POLYGON((300050 6400000,300150 6400000,300150 6400100,300050 6400100,300050 6400000))\n",
    )
    .unwrap();
    let ws = model(
        &format!(
            r#"{{"id": "areas", "type": "Reader", "params": {{"path": "{d}/areas.csv", "wkt": "wkt", "crs": "EPSG:3006"}}}},
               {{"id": "each", "type": "GeodataFetcher", "params": {{"product": "ngp_detailed_plans", "catalogue": "{b}/ngp/v1", "areas": "each", "connection": "LM"}}}},
               {{"id": "plans", "type": "GeodataFetcher", "params": {{"product": "ngp_detailed_plans", "catalogue": "{b}/ngp/v1", "bbox": "300000, 6400000, 300200, 6400100", "connection": "LM", "keep_in": "kept"}}}},
               {{"id": "again", "type": "GeodataFetcher", "params": {{"product": "ngp_detailed_plans", "catalogue": "{b}/ngp/v1", "bbox": "300000, 6400000, 300200, 6400100", "connection": "LM", "keep_in": "kept"}}}},
               {{"id": "map", "type": "GeodataFetcher", "params": {{"product": "lm_nmk50", "catalogue": "{b}/stac-karta/v1", "bbox": "300000, 6400000, 300200, 6400100", "connection": "LM", "cell_size": "10", "keep_in": "kept"}}}}"#,
            b = base,
            d = dir.display()
        ),
        r#"{"from": ["areas", "Output"], "to": ["each", "Extent"]}"#,
    );
    let r = run(&ws, Some(&b), &dir);
    let each = r.output("each", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["each"].error)).clone();
    let k = each.field_index("kommun").expect("the attributes of the area");
    assert_eq!(each.len(), 6, "the made-up service gives the three plans to each search");
    assert_eq!(each.features.iter().map(|f| f.attrs[k].to_text()).collect::<Vec<_>>(), ["Västby", "Västby", "Västby", "Östby", "Östby", "Östby"]);
    let plans = r.output("plans", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["plans"].error)).clone();
    // All three plans, from both pages, in SWEREF 99 TM where they lie.
    assert_eq!(plans.len(), 3);
    assert_eq!(plans.crs, Some(3006));
    let namn = plans.field_index("namn").unwrap();
    assert_eq!(plans.features.iter().map(|f| f.attrs[namn].to_text()).collect::<Vec<_>>(), ["Centrum", "Hamnen", "Torget"]);
    assert_eq!(plans.features[0].attrs[plans.field_index("antal").unwrap()], Value::Int(2));
    assert!(plans.features[0].attrs[plans.field_index("beslut").unwrap()].to_text().contains("2024-01-01"));
    use geo::BoundingRect;
    let r0 = plans.features[0].geom.as_ref().unwrap().bounding_rect().unwrap();
    assert!((r0.min().x - 300000.0).abs() < 0.01 && (r0.max().y - 6400100.0).abs() < 0.01, "{:?}", r0);
    // The files, under the server's name, the second of the same name not written over.
    let kept = dir.join("kept/Detaljplan");
    let mut names: Vec<String> = std::fs::read_dir(&kept).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, [".ographis-files.json", "p2_plankarta.pdf", "p3_plankarta.pdf", "planbeskrivning.pdf", "plankarta.pdf"]);
    // The documents: one row each, however many plans refer to it, with what it is and where it lies.
    let docs = r.output("plans", "Files").unwrap();
    let col_of = |name: &str| col(docs, name).iter().map(|v| v.to_text()).collect::<Vec<_>>();
    let titles = col_of("title");
    assert_eq!(docs.len(), 4, "three maps and one description: {:?}", titles);
    let b = titles.iter().position(|t| t == "Planbeskrivning").unwrap();
    assert_eq!(col_of("items")[b], "3", "the description of all three plans is one row");
    let m = titles.iter().position(|t| t == "Plankarta Hamnen").unwrap();
    assert_eq!((col_of("type")[m].as_str(), col_of("roles")[m].as_str(), col_of("asset")[m].as_str()), ("application/pdf", "plankarta, data", "plankarta"));
    assert!(col_of("file")[m].ends_with("p2_plankarta.pdf"), "{:?}", col_of("file"));
    assert!(col_of("href")[b].ends_with("/dl/asset/b"));
    assert_eq!(std::fs::read_to_string(kept.join("p2_plankarta.pdf")).unwrap(), "%PDF-1.4 a2");
    // Not fetched again.
    let asked = log.lock().unwrap().clone();
    assert_eq!(asked.iter().filter(|l| l.starts_with("GET /dl/asset/a1 ")).count(), 1, "{:?}", asked);
    assert!(
        asked.iter().any(|l| l.starts_with("POST /ngp/v1/search") && l.contains("\"page\":2") && l.contains("bbox")),
        "the next page merges with the search"
    );
    assert!(
        asked.iter().filter(|l| l.contains("/ngp/v1/search")).all(|l| l.contains(",6400000,") || l.contains(",6400000.0,")),
        "the platform takes its rectangle in SWEREF 99 TM: {:?}",
        asked.iter().filter(|l| l.contains("/ngp/v1/search")).collect::<Vec<_>>()
    );
    // The map: its colours, not the numbers of them, and its PDF kept.
    let map = r.output("map", "Output").unwrap_or_else(|| panic!("{:?}", r.nodes["map"].error)).image.clone().expect("a picture");
    assert_eq!(&map.rgba[..4], &[30, 120, 200, 255]);
    assert!(dir.join("kept/nmk50/m.pdf").is_file() && dir.join("kept/nmk50/m.tif").is_file());
    use_settings(None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The models for Swedish public APIs, each on a made-up copy of its service: the paths,
/// the pages by offset and by cursor, the key in a header, and the geometry put together.
#[test]
fn the_swedish_api_examples_work_on_copies_of_the_services() {
    let dir = tmp("swedish-apis");
    let b = GdalBackend::new();
    let (url, log) = server(Arc::new(|r: &Req| {
        let num = |k: &str| r.query(k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        let body = match r.route() {
            "/ibww/api/version/1/warning.json" => r#"[{"id": 1, "event": {"sv": "Vind"}, "warningAreas": [
                {"id": 9, "approximateStart": "2026-11-17T10:00:00Z", "approximateEnd": "2026-11-18T10:00:00Z", "areaName": {"sv": "Skåne"}, "warningLevel": {"sv": "Gul"}, "eventDescription": {"sv": "Kuling"},
                 "area": {"type": "Feature", "geometry": {"type": "Polygon", "coordinates": [[[14.1, 55.7], [14.2, 55.7], [14.2, 55.8], [14.1, 55.7]]]}, "properties": {}}},
                {"id": 10, "approximateStart": "2026-11-17T10:00:00Z", "areaName": {"sv": "Halland"}, "warningLevel": {"sv": "Orange"}, "eventDescription": {"sv": "Storm"},
                 "area": {"type": "Feature", "geometry": {"type": "MultiPolygon", "coordinates": [[[[12.1, 56.7], [12.2, 56.7], [12.2, 56.8], [12.1, 56.7]]]]}, "properties": {}}}]}]"#
                .to_string(),
            "/api/data" => r#"{"Header": {"Column": []}, "Rows": [
                {"Cell": [{"Column": "ar", "Value": "2024", "IsMeasure": false}, {"Column": "reglan", "Name": "01", "Value": "Stockholms län", "IsMeasure": false}, {"Column": "nyregunder", "Value": "1234", "IsMeasure": true}]},
                {"Cell": [{"Column": "ar", "Value": "2024", "IsMeasure": false}, {"Column": "reglan", "Name": "14", "Value": "Västra Götalands län", "IsMeasure": false}, {"Column": "nyregunder", "Value": "987", "IsMeasure": true}]}]}"#
                .to_string(),
            "/store/search" => {
                let (off, lim) = (num("offset"), num("limit"));
                let children: Vec<String> = (off..(off + lim).min(230))
                    .map(|i| {
                        format!(
                            r#"{{"entryId": "{i}", "metadata": {{"@graph": [{{"@type": "schema:LodgingBusiness", "schema:name": {{"@value": "Hotel {i}", "@language": "sv"}}, "schema:url": {{"@id": "https://h/{i}"}}, "schema:geo": {{"@id": "_:g{i}"}}}}, {{"@id": "_:g{i}", "@type": "schema:GeoCoordinates", "schema:latitude": "55.{i:03}", "schema:longitude": "13.4"}}]}}}}"#
                        )
                    })
                    .collect();
                format!(r#"{{"offset": {off}, "limit": {lim}, "results": 230, "resource": {{"children": [{}]}}}}"#, children.join(","))
            }
            "/search" => {
                let (off, lim) = (num("offset"), num("limit"));
                let hits: Vec<String> = (off..(off + lim).min(150))
                    .map(|i| format!(r#"{{"id": "{i}", "headline": "Job {i}", "employer": {{"name": "E"}}, "workplace_address": {{"municipality": "Göteborg", "coordinates": [11.97, 57.7]}}}}"#))
                    .collect();
                format!(r#"{{"total": {{"value": 150}}, "hits": [{}]}}"#, hits.join(","))
            }
            "/api/events" => r#"[{"id": 1, "datetime": "2026-11-17 4:42:02 +01:00", "name": "x", "type": "Rån", "location": {"name": "Göteborg", "gps": "57.70887,11.97456"}}]"#.to_string(),
            "/v1/arbetsstallen" if r.headers.get("x-api-key").map(String::as_str) == Some("k3y") => match r.query("cursorId") {
                None => r#"{"pagination": {"nextCursorId": 50, "limit": 50, "hasMore": true}, "arbetsstallen": [{"cfarNr": 1, "namn": "A"}, {"cfarNr": 2, "namn": "B"}]}"#.to_string(),
                Some(_) => r#"{"pagination": {"limit": 50, "hasMore": false}, "arbetsstallen": [{"cfarNr": 3, "namn": "C"}]}"#.to_string(),
            },
            "/v1/arbetsstallen" => return (401, vec![], "{}".into()),
            _ => return (404, vec![], "{}".into()),
        };
        (200, vec![], body)
    }));
    let _turn = SETTINGS_TURN.lock().unwrap_or_else(|e| e.into_inner());
    use_settings(Some(
        serde_json::from_str(r#"{"apis": [{"name": "SCB företagsregister", "auth": "key_header", "param": "X-API-Key", "secret": "k3y"}]}"#).unwrap(),
    ));
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apis");
    let run_example = |file: &str, hosts: &[&str]| {
        let mut text = std::fs::read_to_string(examples.join(file)).unwrap().replace("\"cache\": \"disk\"", "\"cache\": \"none\"");
        for h in hosts {
            text = text.replace(h, &url);
        }
        let r = run(&Workspace::from_json(&text).unwrap(), Some(&b), &dir);
        // An Output turned off in the example is not a failure.
        let errors: Vec<_> = r.nodes.iter().filter_map(|(k, n)| n.error.clone().filter(|e| e != "The node is disabled.").map(|e| (k.clone(), e))).collect();
        assert!(errors.is_empty(), "{}: {:?}", file, errors);
        r
    };
    let r = run_example("smhi_warnings.ographis", &["https://opendata-download-warnings.smhi.se"]);
    let w = r.output("geom", "Output").unwrap();
    assert_eq!((w.len(), w.crs), (2, Some(4326)), "a polygon and a multipolygon, put together again");
    assert_eq!(col(w, "w.warningLevel.sv"), [Value::Text("Gul".into()), Value::Text("Orange".into())]);

    let r = run_example("trafa_new_cars.ographis", &["https://api.trafa.se"]);
    let t = r.output("keep", "Output").unwrap();
    assert_eq!(col(t, "county_code"), [Value::Text("01".into()), Value::Text("14".into())]);
    assert_eq!(col(t, "new_cars")[1].to_text(), "987");

    let r = run_example("visit_sweden_lodging.ographis", &["https://data.visitsweden.com"]);
    let p = r.output("points", "Points").unwrap();
    assert_eq!(p.len(), 230, "three pages of 100");
    assert_eq!(col(p, "name")[229], Value::Text("Hotel 229".into()));

    let r = run_example("jobsearch_gothenburg.ographis", &["https://jobsearch.api.jobtechdev.se"]);
    assert_eq!(r.output("points", "Points").unwrap().len(), 150);

    let r = run_example("police_events.ographis", &["https://polisen.se"]);
    let p = r.output("points", "Points").unwrap();
    assert_eq!((col(p, "lat")[0].to_text(), col(p, "lon")[0].to_text()), ("57.70887".into(), "11.97456".into()));
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|q| q.route() == "/api/events" && q.query("locationname").is_some_and(|l| l == "V%C3%A4stra%20G%C3%B6talands%20l%C3%A4n")),
        "åäö and spaces are sent encoded"
    );

    // Pages by a cursor deep in the answer, with a key in a header; and an answer kept in
    // http-cache with one setting for the pages is not taken for another.
    let scb = format!(
        r#"{{"nodes": [{{"id": "afr", "type": "HttpCaller", "params": {{"url": "'{url}/v1/arbetsstallen?kommun=1480'", "connection": "SCB företagsregister", "paging": "cursor",
               "cursor_path": "pagination.nextCursorId", "page_param": "cursorId", "size_param": "limit", "page_size": "5000", "cache": "disk"}}}},
             {{"id": "items", "type": "JsonExploder", "params": {{"attribute": "_response", "path": "arbetsstallen"}}}}],
           "edges": [{{"from": ["afr", "Output"], "to": ["items", "Input"]}}]}}"#
    );
    let wrong = scb.replace("\"pagination.nextCursorId\"", "\"nextCursorId\"");
    let rows = |text: &str| run(&Workspace::from_json(text).unwrap(), Some(&b), &dir).output("items", "Exploded").unwrap().len();
    assert_eq!(rows(&wrong), 2, "the cursor is not found: one page");
    assert_eq!(rows(&scb), 3, "the pages are followed, not the one page kept");
    assert_eq!(rows(&scb), 3, "and then taken from http-cache");
    // The size of a page is asked for with every page, the first too.
    let asked: Vec<_> = log.lock().unwrap().iter().filter(|q| q.route() == "/v1/arbetsstallen").map(|q| (q.query("limit"), q.query("cursorId"))).collect();
    assert!(asked.contains(&(Some("5000".into()), None)), "{:?}", asked);
    assert!(asked.contains(&(Some("5000".into()), Some("50".into()))), "{:?}", asked);
    use_settings(None);
    let _ = std::fs::remove_dir_all(&dir);
}
