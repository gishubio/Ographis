//! Tools that ask web services: HTTP request and STAC search.

use super::*;
use crate::json;
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Table, Value};
use crate::web::{encode, Client, Paging};

// ---------- HTTP request (HttpCaller) ----------

pub(super) const HTTP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "url",
        label: "Address",
        kind: ParamKind::Expression("Input"),
        default: "",
        hint: "A web address, or an expression that builds one from the attributes: 'https://api.example.org/item?id=' || id",
    },
    ParamSpec {
        key: "method",
        label: "Method",
        kind: ParamKind::Choice(HTTP_METHODS),
        default: "GET",
        hint: "GET asks for something; POST and PUT send something along",
    },
    ParamSpec {
        key: "body",
        label: "Body",
        kind: ParamKind::Expression("Input"),
        default: "",
        hint: "What is sent with POST, PUT or PATCH: an attribute such as json from Build JSON, or an expression. Empty sends nothing",
    },
    ParamSpec {
        key: "content_type",
        label: "Content type",
        kind: ParamKind::Text,
        default: "application/json",
        hint: "What the body is, sent as the header Content-Type",
    },
    ParamSpec {
        key: "headers",
        label: "Headers",
        kind: ParamKind::Text,
        default: "",
        hint: "For example Accept: application/json. Several are separated by ;",
    },
    ParamSpec { key: "response", label: "Response attribute", kind: ParamKind::Text, default: "_response", hint: "The answer, as text" },
    ParamSpec {
        key: "status",
        label: "Status attribute",
        kind: ParamKind::Text,
        default: "_status",
        hint: "The status code of the answer, such as 200 or 404",
    },
    ParamSpec { key: "timeout", label: "Timeout in seconds", kind: ParamKind::Text, default: "30", hint: "How long to wait for each answer" },
    ParamSpec {
        key: "connection",
        label: "API connection",
        kind: ParamKind::ApiConnection,
        default: "",
        hint: "The key, user or OAuth2 client, kept in the settings and not in the model. Where the key goes in the address or the body, write {{key}}",
    },
    ParamSpec {
        key: "paging",
        label: "Pages",
        kind: ParamKind::Choice(PAGINGS),
        default: "none",
        hint: "For an answer that comes in pages: each page becomes a feature, with its number in _page",
    },
    ParamSpec {
        key: "next_path",
        label: "Next page at",
        kind: ParamKind::Text,
        default: "",
        hint: "The path to the address of the next page, such as next_page, links[rel=next].href or @odata.nextLink. Empty uses the Link header",
    },
    ParamSpec { key: "page_param", label: "Parameter of the page", kind: ParamKind::Text, default: "", hint: "Such as offset, skip, start or page" },
    ParamSpec {
        key: "size_param",
        label: "Parameter of the page size",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as limit, per_page or pageSize. Empty sends no size",
    },
    ParamSpec { key: "page_size", label: "Page size", kind: ParamKind::Text, default: "100", hint: "How many items a page holds" },
    ParamSpec {
        key: "start",
        label: "First value",
        kind: ParamKind::Text,
        default: "",
        hint: "Where counting starts: 0 for an offset, 1 for a page number, if empty",
    },
    ParamSpec {
        key: "items_path",
        label: "Items at",
        kind: ParamKind::Text,
        default: "",
        hint: "The path to the list of items in a page, such as values or features, to know when the pages end. Empty takes the first list",
    },
    ParamSpec { key: "max_pages", label: "At most pages", kind: ParamKind::Text, default: "1000", hint: "A stop, should the pages never end" },
    ParamSpec {
        key: "per_second",
        label: "At most requests a second",
        kind: ParamKind::Text,
        default: "",
        hint: "As a service asks, such as 1 for Nominatim. Empty sends them as fast as the answers come",
    },
    ParamSpec {
        key: "answer_as",
        label: "The answer",
        kind: ParamKind::Choice(&[("text", "As text in an attribute"), ("file", "As a file, its path in _file")]),
        default: "text",
        hint: "A file for answers that are not text, such as a GeoTIFF of travel times. Read the files with Read files named in an attribute",
    },
    ParamSpec {
        key: "folder",
        label: "Folder for the files",
        kind: ParamKind::Text,
        default: "answers",
        hint: "Beside the model, or a whole path. Each file is named by its request, so the same request gives the same file",
    },
    ParamSpec {
        key: "parallel",
        label: "Requests at once",
        kind: ParamKind::Text,
        default: "1",
        hint: "Sends this many at the same time, such as 8 to a service of your own, such as OpenTripPlanner. A public service wants 1",
    },
    ParamSpec {
        key: "cache",
        label: "Keep the answers",
        kind: ParamKind::Choice(&[("none", "No"), ("disk", "Yes, in a folder beside the model")]),
        default: "none",
        hint: "Good answers are kept in the folder http-cache beside the model, and the same request is not sent again in later runs. Delete the folder to ask anew",
    },
    ParamSpec {
        key: "retries",
        label: "Tries again",
        kind: ParamKind::Text,
        default: "3",
        hint: "When the service is busy (429), briefly away (502, 503, 504) or cannot be reached: waits as the service says, or 1, 2, 4… seconds",
    },
];

pub(crate) const PAGINGS: &[(&str, &str)] =
    &[("none", "One answer"), ("next", "A link to the next page"), ("offset", "An offset: 0, 100, 200…"), ("page", "A page number: 1, 2, 3…")];

pub(super) const HTTP_METHODS: &[(&str, &str)] = &[("GET", "GET"), ("POST", "POST"), ("PUT", "PUT"), ("PATCH", "PATCH"), ("DELETE", "DELETE")];

/// The address of a request: an expression, or an address written without quotes.
pub(super) enum Address {
    Fixed(String),
    Built(expr::Compiled),
}

pub(super) fn run_http(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    if p.get("answer_as").as_deref() == Some("file") {
        return run_http_files(p, inp, ctx);
    }
    let b = ctx.backend("Web requests")?;
    let text = p.need("url", "Write the address to send the request to.")?;
    // With nothing connected, one request is sent, to the address as it is written.
    let alone;
    let table = if ctx.open_ports.contains(&"Input") {
        alone = Table { features: vec![Feature { z: None, attrs: Vec::new(), geom: None }], ..Table::default() };
        &alone
    } else {
        let t = input(inp, "Input");
        if t.is_empty() {
            ctx.notes.warn("Nothing came in on Input, so no request was sent. Disconnect Input to send one request to the address.");
        }
        t
    };
    let names = table.field_names();
    let address = match expr::compile(&text, &names) {
        Ok(e) => Address::Built(e),
        // An address written as it is, without quotes, is taken as it is.
        Err(_) if text.trim().starts_with("http://") || text.trim().starts_with("https://") => Address::Fixed(text.trim().to_string()),
        Err(e) => return Err(GbError(format!("The address: {} Write a fixed address in single quotes, as 'https://…'.", e.0))),
    };
    let headers: Vec<(String, String)> = p
        .get("headers")
        .unwrap_or_default()
        .split(';')
        .filter_map(|h| h.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    let whole = |k: &str, d: u64, what: &str| -> Result<u64> {
        match p.get(k).filter(|t| !t.trim().is_empty()) {
            None => Ok(d),
            Some(t) => t.trim().parse::<u64>().map_err(|_| GbError(format!("{} must be a whole number, not \"{}\".", what, t))),
        }
    };
    let timeout = whole("timeout", 30, "The timeout")?.max(1);
    let method = p.get("method").unwrap_or_else(|| "GET".into()).to_uppercase();
    let sends = matches!(method.as_str(), "POST" | "PUT" | "PATCH");
    let body = match p.get("body").filter(|b| sends && !b.trim().is_empty()) {
        Some(b) => Some(expr::compile(&b, &names).map_err(|e| GbError(format!("The body: {}", e.0)))?),
        None => None,
    };
    let mut headers = headers;
    if sends && !headers.iter().any(|h| h.0.eq_ignore_ascii_case("content-type")) {
        headers.push(("Content-Type".into(), p.get("content_type").unwrap_or_else(|| "application/json".into())));
    }
    let paging = crate::web::paging_of(&|k| p.get(k))?;
    let paged = paging != crate::web::Paging::None;
    let most = whole("max_pages", 1000, "At most pages")?.max(1) as usize;
    let auth = match p.get("connection").filter(|c| !c.trim().is_empty()) {
        Some(name) => Some(crate::web::connection(&name)?),
        None => None,
    };
    let per_second = match p.get("per_second").filter(|t| !t.trim().is_empty()) {
        None => 0.0,
        Some(t) => t.trim().replace(',', ".").parse::<f64>().map_err(|_| GbError(format!("At most requests a second must be a number, not \"{}\".", t)))?,
    };
    let gate = ctx.gate;
    let stopped = move || !gate.going();
    let response_name = p.get("response").unwrap_or_else(|| "_response".into());
    let status_name = p.get("status").unwrap_or_else(|| "_status".into());
    let geographic = ctx.geographic(table.crs);
    let parallel = whole("parallel", 1, "Requests at once")?.clamp(1, 64) as usize;
    let cache_dir = (p.get("cache").as_deref() == Some("disk")).then(|| ctx.base_dir.join("http-cache"));
    // Each feature's request, worked out first: the address and the body.
    let mut requests: Vec<(String, Option<String>)> = Vec::with_capacity(table.len());
    for (i, f) in table.features.iter().enumerate() {
        let url = match &address {
            Address::Fixed(u) => u.clone(),
            Address::Built(e) => e.eval(&Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?.to_text(),
        };
        let sent = match &body {
            Some(e) => Some(e.eval(&Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?.to_text()),
            None => None,
        };
        requests.push((url, sent));
    }
    // The same request is only sent once in a run, unless it changes something.
    let key_of = |(url, sent): &(String, Option<String>)| format!("{} {}\n{}", method, url, sent.as_deref().unwrap_or(""));
    let mut order: Vec<String> = Vec::new();
    let mut unique: HashMap<String, (String, Option<String>)> = HashMap::new();
    for (k, r) in requests.iter().enumerate() {
        let key = if method == "GET" { key_of(r) } else { format!("{}#{}", key_of(r), k) };
        if let std::collections::hash_map::Entry::Vacant(slot) = unique.entry(key.clone()) {
            order.push(key);
            slot.insert(r.clone());
        }
    }
    type Got = std::result::Result<Vec<crate::io::HttpAnswer>, String>;
    let mut answers: HashMap<String, Got> = HashMap::new();
    let mut from_cache = 0usize;
    if let Some(dir) = &cache_dir {
        for key in &order {
            if let Some(pages) = cache_read(dir, &key_of(&unique[key]), &headers) {
                answers.insert(key.clone(), Ok(pages));
                from_cache += 1;
            }
        }
    }
    let todo: Vec<&String> = order.iter().filter(|k| !answers.contains_key(*k)).collect();
    let total = todo.len() as u64;
    let progress = ctx.progress;
    let resent = std::sync::atomic::AtomicU32::new(0);
    let pages_stopped = std::sync::Mutex::new(Vec::<String>::new());
    // One request, with its pages, by a client of its own.
    let fetch = |client: &mut crate::web::Client, key: &str| -> std::result::Result<Got, GbError> {
        let (url, sent) = &unique[key];
        let mut pages = Vec::new();
        let r = client.pages(&method, url, &headers, sent.as_deref(), &paging, most, &mut |_, _, a| {
            pages.push(a.clone());
            true
        });
        Ok(match r {
            Ok(_) => Ok(pages),
            Err(e) if e.0 == crate::model::STOPPED => return Err(e),
            // Pages that came before a failure are kept, and the failure after them.
            Err(e) if !pages.is_empty() => {
                pages_stopped.lock().expect("not poisoned").push(format!("The pages stopped after page {}: {}", pages.len(), e.0));
                Ok(pages)
            }
            Err(e) => Err(e.0),
        })
    };
    let retries = whole("retries", 3, "Tries again")?.min(10) as u32;
    let new_client = |workers: usize| {
        let mut c = crate::web::Client::new(b, auth.clone(), &stopped).per_second(if per_second > 0.0 { per_second / workers as f64 } else { 0.0 });
        c.timeout_s = timeout;
        c.retries = retries;
        c
    };
    if parallel <= 1 || todo.len() <= 1 {
        let mut client = new_client(1);
        for (n, key) in todo.iter().enumerate() {
            progress("sent requests for", n as u64, Some(total));
            if stopped() {
                return err(crate::model::STOPPED);
            }
            let got = fetch(&mut client, key)?;
            answers.insert((*key).clone(), got);
        }
        resent.fetch_add(client.resent, std::sync::atomic::Ordering::Relaxed);
    } else {
        // Several workers take the next request until there are none left. This thread
        // tells how far they have come.
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let halted = AtomicBool::new(false);
        let results = std::sync::Mutex::new(Vec::<(String, Got)>::new());
        let workers = parallel.min(todo.len());
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    let mut client = new_client(workers);
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        if k >= todo.len() || halted.load(Ordering::Relaxed) {
                            break;
                        }
                        match fetch(&mut client, todo[k]) {
                            Ok(got) => results.lock().expect("not poisoned").push((todo[k].clone(), got)),
                            Err(_) => halted.store(true, Ordering::Relaxed),
                        }
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                    resent.fetch_add(client.resent, Ordering::Relaxed);
                });
            }
            while done.load(Ordering::Relaxed) < todo.len() && !halted.load(Ordering::Relaxed) {
                progress("sent requests for", done.load(Ordering::Relaxed) as u64, Some(total));
                if stopped() {
                    halted.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        if halted.load(Ordering::Relaxed) {
            return err(crate::model::STOPPED);
        }
        answers.extend(results.into_inner().expect("not poisoned"));
    }
    for w in pages_stopped.into_inner().expect("not poisoned") {
        ctx.notes.warn(w);
    }
    if let Some(dir) = &cache_dir {
        for key in &todo {
            if let Some(Ok(pages)) = answers.get(*key) {
                if pages.iter().all(|a| a.status < 400) {
                    cache_write(dir, &key_of(&unique[*key]), &headers, pages);
                }
            }
        }
        if from_cache > 0 {
            ctx.notes.info(format!("{} answers were taken from http-cache", fmt_count(from_cache as u64)));
        }
    }
    // Each answer, or page of an answer: the feature it is for, and what came.
    let mut rows: Vec<(usize, usize, std::result::Result<crate::io::HttpAnswer, String>)> = Vec::new();
    for (i, r) in requests.iter().enumerate() {
        let key = if method == "GET" { key_of(r) } else { format!("{}#{}", key_of(r), i) };
        match answers.get(&key).cloned().unwrap_or_else(|| Err("No answer".into())) {
            Ok(pages) => {
                for (n, a) in pages.into_iter().enumerate() {
                    rows.push((i, n + 1, Ok(a)));
                }
            }
            Err(e) => rows.push((i, 1, Err(e))),
        }
    }
    let client = new_client(1);
    let client_resent = resent.load(std::sync::atomic::Ordering::Relaxed);
    progress("sent requests for", total, Some(total));
    let page_count: usize = answers.values().map(|a| a.as_ref().map_or(1, |p| p.len())).sum();
    if paged {
        ctx.notes.info(format!("{} pages from {} requests", fmt_count(page_count as u64), fmt_count(answers.len() as u64)));
    } else if answers.len() < table.len() {
        ctx.notes.info(format!("{} requests to {} addresses", fmt_count(table.len() as u64), fmt_count(answers.len() as u64)));
    }
    if client_resent > 0 {
        ctx.notes.info(format!("{} requests were sent again, as the service was busy or away", fmt_count(client_resent as u64)));
    }
    // A feature for each answer, or for each page. An attribute with the name of the
    // response or the status, as when the answer of one request is asked again, is
    // written over rather than had twice.
    let mut fields = table.fields.clone();
    let slot = |fields: &mut Vec<Field>, name: &str, kind: FieldType| match fields.iter().position(|f| f.name == name) {
        Some(at) => {
            fields[at].ty = kind;
            at
        }
        None => {
            fields.push(Field::new(name, kind));
            fields.len() - 1
        }
    };
    let at_response = slot(&mut fields, &response_name, FieldType::Text);
    let at_status = slot(&mut fields, &status_name, FieldType::Int);
    let at_page = paged.then(|| slot(&mut fields, "_page", FieldType::Int));
    let good_fields = fields.clone();
    let at_error = slot(&mut fields, "_error", FieldType::Text);
    let mut good = Table { fields: good_fields.clone(), features: Vec::new(), ..table.like() };
    let mut bad = Table { fields, features: Vec::new(), ..table.like() };
    for (i, n, a) in rows {
        let mut f = table.features[i].clone();
        f.attrs.resize(good_fields.len(), Value::Null);
        let (fine, error) = match &a {
            Ok(a) if a.status < 400 => (true, Value::Null),
            Ok(a) => (false, Value::Text(format!("The server answered {}", a.status))),
            Err(e) => (false, Value::Text(client.hide(e))),
        };
        match a {
            Ok(a) => {
                f.attrs[at_response] = Value::Text(a.body);
                f.attrs[at_status] = Value::Int(a.status as i64);
            }
            Err(_) => {
                f.attrs[at_response] = Value::Null;
                f.attrs[at_status] = Value::Null;
            }
        }
        if let Some(at) = at_page {
            f.attrs[at] = Value::Int(n as i64);
        }
        if fine {
            good.features.push(f);
        } else {
            f.attrs.resize(bad.fields.len(), Value::Null);
            f.attrs[at_error] = error;
            bad.features.push(f);
        }
    }
    if !bad.is_empty() {
        let what = if bad.len() == 1 { "1 request got".to_string() } else { format!("{} requests got", fmt_count(bad.len() as u64)) };
        ctx.notes.warn(format!("{} no good answer and went to Failed. The reason is in the attribute _error.", what));
    }
    Ok(vec![("Output".into(), good), ("Failed".into(), bad)])
}

/// The kind of file the bytes are, by how they begin.
fn extension_of(bytes: &[u8]) -> &'static str {
    let t = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).trim_start().to_string();
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") || bytes.starts_with(b"II+\0") {
        "tif"
    } else if bytes.starts_with(b"\x89PNG") {
        "png"
    } else if bytes.starts_with(b"\xff\xd8") {
        "jpg"
    } else if bytes.starts_with(b"PK") {
        "zip"
    } else if bytes.starts_with(b"%PDF") {
        "pdf"
    } else if t.starts_with('{') && t.contains("\"type\"") && (t.contains("Feature") || t.contains("coordinates")) {
        "geojson"
    } else if t.starts_with('{') || t.starts_with('[') {
        "json"
    } else if t.starts_with('<') {
        "xml"
    } else {
        "txt"
    }
}

/// HTTP request with the answers kept as files: each fetched with GET, as bytes.
fn run_http_files(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let b = ctx.backend("Web requests")?;
    let method = p.get("method").unwrap_or_else(|| "GET".into()).to_uppercase();
    if method != "GET" {
        return err("Answers as files are fetched with GET. Choose GET, or take the answer as text.");
    }
    let text = p.need("url", "Write the address to send the request to.")?;
    let alone;
    let table = if ctx.open_ports.contains(&"Input") {
        alone = Table { features: vec![Feature { z: None, attrs: Vec::new(), geom: None }], ..Table::default() };
        &alone
    } else {
        input(inp, "Input")
    };
    let names = table.field_names();
    let address = match expr::compile(&text, &names) {
        Ok(e) => Address::Built(e),
        Err(_) if text.trim().starts_with("http://") || text.trim().starts_with("https://") => Address::Fixed(text.trim().to_string()),
        Err(e) => return Err(GbError(format!("The address: {} Write a fixed address in single quotes, as 'https://…'.", e.0))),
    };
    let headers: Vec<(String, String)> = p
        .get("headers")
        .unwrap_or_default()
        .split(';')
        .filter_map(|h| h.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    let folder = ctx.resolve(&p.get("folder").filter(|f| !f.trim().is_empty()).unwrap_or_else(|| "answers".into()));
    std::fs::create_dir_all(&folder).map_err(|e| GbError(format!("Cannot create the folder {}: {}", folder.display(), e)))?;
    let keep = p.get("cache").as_deref() == Some("disk");
    let geographic = ctx.geographic(table.crs);
    let mut out = Table { fields: table.fields.clone(), features: Vec::new(), ..table.like() };
    out.fields.push(Field::new("_file", FieldType::Text));
    let mut failed = Table { fields: table.fields.clone(), features: Vec::new(), ..table.like() };
    failed.fields.push(Field::new("_error", FieldType::Text));
    let mut done: HashMap<String, std::result::Result<String, String>> = HashMap::new();
    let (mut fetched, mut kept) = (0usize, 0usize);
    let total = table.len() as u64;
    for (i, f) in table.features.iter().enumerate() {
        (ctx.progress)("fetched files for", i as u64, Some(total));
        if ctx.stopped() {
            return err(crate::model::STOPPED);
        }
        let url = match &address {
            Address::Fixed(u) => u.clone(),
            Address::Built(e) => e.eval(&Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?.to_text(),
        };
        let got = match done.get(&url) {
            Some(g) => g.clone(),
            None => {
                let stem = cache_file(&folder, &format!("GET {}", url), &headers);
                let stem = stem.with_extension("");
                // A file from an earlier run, when the answers are kept.
                let earlier = keep
                    .then(|| std::fs::read_dir(&folder).ok())
                    .flatten()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .find(|path| path.with_extension("") == stem);
                let g = match earlier {
                    Some(path) => {
                        kept += 1;
                        Ok(path.to_string_lossy().into_owned())
                    }
                    None => match b.fetch_bytes(&url, &headers) {
                        Ok(bytes) => {
                            fetched += 1;
                            let path = stem.with_extension(extension_of(&bytes));
                            std::fs::write(&path, &bytes).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
                            ctx.files.push(path.clone());
                            Ok(path.to_string_lossy().into_owned())
                        }
                        Err(e) => Err(e.0),
                    },
                };
                done.insert(url, g.clone());
                g
            }
        };
        let mut attrs = f.attrs.clone();
        match got {
            Ok(path) => {
                attrs.push(Value::Text(path));
                out.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
            }
            Err(e) => {
                attrs.push(Value::Text(e));
                failed.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
            }
        }
    }
    ctx.notes.info(format!(
        "{} files fetched{}",
        fmt_count(fetched as u64),
        if kept > 0 { format!(", {} kept from before", fmt_count(kept as u64)) } else { String::new() }
    ));
    if !failed.is_empty() {
        ctx.notes.warn(format!("{} requests got no file and went to Failed. The reason is in the attribute _error.", fmt_count(failed.len() as u64)));
    }
    Ok(vec![("Output".into(), out), ("Failed".into(), failed)])
}

/// The file of a request in the cache: a number made from the request, the same from run
/// to run and between computers (FNV-1a).
fn cache_file(dir: &std::path::Path, key: &str, headers: &[(String, String)]) -> std::path::PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes().chain(headers.iter().flat_map(|(k, v)| k.bytes().chain(v.bytes()))) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    dir.join(format!("{:016x}.json", h))
}

fn cache_read(dir: &std::path::Path, key: &str, headers: &[(String, String)]) -> Option<Vec<crate::io::HttpAnswer>> {
    let text = std::fs::read_to_string(cache_file(dir, key, headers)).ok()?;
    let j: serde_json::Value = serde_json::from_str(&text).ok()?;
    // The request is kept too, so that two that happen to share a number are told apart.
    if j.get("request").and_then(|r| r.as_str()) != Some(key) {
        return None;
    }
    j.get("pages")?
        .as_array()?
        .iter()
        .map(|p| Some(crate::io::HttpAnswer { status: p.get("status")?.as_u64()? as u16, body: p.get("body")?.as_str()?.to_string(), headers: Vec::new() }))
        .collect()
}

fn cache_write(dir: &std::path::Path, key: &str, headers: &[(String, String)], pages: &[crate::io::HttpAnswer]) {
    let _ = std::fs::create_dir_all(dir);
    let j = serde_json::json!({
        "request": key,
        "pages": pages.iter().map(|a| serde_json::json!({"status": a.status, "body": a.body})).collect::<Vec<_>>(),
    });
    let _ = std::fs::write(cache_file(dir, key, headers), j.to_string());
}

pub(super) const STAC_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "url",
        label: "STAC API",
        kind: ParamKind::Text,
        default: "",
        hint: "The address of the catalogue, such as https://earth-search.aws.element84.com/v1. /search is added",
    },
    ParamSpec {
        key: "collections",
        label: "Collections",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as sentinel-2-l2a. Several are separated by commas. Empty searches all",
    },
    ParamSpec { key: "datetime", label: "Time", kind: ParamKind::Text, default: "", hint: "A date, or from and to: 2026-06-01/2026-08-31. Empty: any time" },
    ParamSpec {
        key: "bbox",
        label: "Area, in WGS 84",
        kind: ParamKind::Text,
        default: "",
        hint: "west, south, east, north in degrees, such as 11.8, 57.6, 12.1, 57.8. A stream on Area is used instead, if one is connected",
    },
    ParamSpec {
        key: "query",
        label: "Filter",
        kind: ParamKind::Text,
        default: "",
        hint: "A CQL2 filter the catalogue understands, such as eo:cloud_cover < 20",
    },
    ParamSpec { key: "max_items", label: "At most items", kind: ParamKind::Text, default: "100", hint: "" },
    ParamSpec {
        key: "connection",
        label: "API connection",
        kind: ParamKind::ApiConnection,
        default: "",
        hint: "For a catalogue that needs a login, such as Lantmäteriet's",
    },
];

pub(super) fn run_stac(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let b = ctx.backend("STAC search")?;
    let base = p.need("url", "Write the address of the STAC API.")?;
    let base = base.trim().trim_end_matches('/').trim_end_matches("/search").to_string();
    let most = p.get("max_items").and_then(|m| m.trim().parse::<usize>().ok()).unwrap_or(100).max(1);
    // The area: the stream on Area, in degrees, or the four numbers.
    let bbox: Option<[f64; 4]> = if ctx.open_ports.contains(&"Area") {
        match p.get("bbox").filter(|t| !t.trim().is_empty()) {
            Some(t) => {
                let n: Vec<f64> = t
                    .split([',', ' ', ';'])
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().parse::<f64>())
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|_| GbError(format!("The area must be four numbers, not \"{}\".", t)))?;
                if n.len() != 4 {
                    return err("The area must be four numbers: west, south, east and north.");
                }
                Some([n[0], n[1], n[2], n[3]])
            }
            None => None,
        }
    } else {
        let area = input(inp, "Area");
        let area = match area.crs {
            Some(4326) => area.clone(),
            Some(_) => reproject(area, 4326, Some(b))?,
            None if area.has_geometry() => return err("The stream on Area has no coordinate system, so where it lies is not known."),
            None => area.clone(),
        };
        use geo::BoundingRect;
        let mut r: Option<[f64; 4]> = None;
        for g in area.features.iter().filter_map(|f| f.geom.as_ref()) {
            if let Some(b) = g.bounding_rect() {
                r = Some(match r {
                    None => [b.min().x, b.min().y, b.max().x, b.max().y],
                    Some(o) => [o[0].min(b.min().x), o[1].min(b.min().y), o[2].max(b.max().x), o[3].max(b.max().y)],
                });
            }
        }
        r
    };
    let mut url = format!("{}/search?limit={}", base, most.min(250));
    if let Some(r) = bbox {
        url.push_str(&format!("&bbox={},{},{},{}", r[0], r[1], r[2], r[3]));
    }
    if let Some(c) = p.get("collections").filter(|c| !c.trim().is_empty()) {
        let list: Vec<String> = c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        url.push_str(&format!("&collections={}", encode(&list.join(","))));
    }
    if let Some(d) = p.get("datetime").filter(|d| !d.trim().is_empty()) {
        let d = d.trim();
        // A date alone is the whole day.
        let d = if d.len() == 10 && !d.contains('/') { format!("{}T00:00:00Z/{}T23:59:59Z", d, d) } else { d.to_string() };
        url.push_str(&format!("&datetime={}", encode(&d)));
    }
    if let Some(q) = p.get("query").filter(|q| !q.trim().is_empty()) {
        url.push_str(&format!("&filter-lang=cql2-text&filter={}", encode(q.trim())));
    }
    let auth = match p.get("connection").filter(|c| !c.trim().is_empty()) {
        Some(name) => Some(crate::web::connection(&name)?),
        None => None,
    };
    let gate = ctx.gate;
    let stopped = move || !gate.going();
    let mut client = Client::new(b, auth, &stopped);
    let headers = vec![("Accept".to_string(), "application/geo+json, application/json".to_string())];
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut failure = None;
    let paging = Paging::Next { path: Some(json::parse_path("links[rel=next].href")?) };
    client.pages("GET", &url, &headers, None, &paging, 1000, &mut |_, _, a| {
        if a.status >= 400 {
            failure = Some(format!("The catalogue answered {}: {}", a.status, a.body.chars().take(300).collect::<String>()));
            return false;
        }
        match json::parse(&a.body) {
            Ok(v) => {
                for f in v.get("features").and_then(|f| f.as_array()).into_iter().flatten() {
                    if items.len() < most {
                        items.push(f.clone());
                    }
                }
                items.len() < most
            }
            Err(_) => {
                failure = Some("The catalogue did not answer with JSON. Is the address that of a STAC API?".into());
                false
            }
        }
    })?;
    if let Some(f) = failure {
        return err(client.hide(&f));
    }
    // An attribute for each kind of file the items have, with its address.
    let mut asset_names: Vec<String> = Vec::new();
    for it in &items {
        for k in it.get("assets").and_then(|a| a.as_object()).into_iter().flat_map(|a| a.keys()) {
            if !asset_names.contains(k) {
                asset_names.push(k.clone());
            }
        }
    }
    let mut fields = vec![
        Field::new("id", FieldType::Text),
        Field::new("collection", FieldType::Text),
        Field::new("datetime", FieldType::Text),
        Field::new("cloud_cover", FieldType::Real),
    ];
    fields.extend(asset_names.iter().map(|a| Field::new(format!("asset_{}", a), FieldType::Text)));
    let mut out = Table { fields, crs: Some(4326), ..Table::default() };
    for it in &items {
        let props = it.get("properties");
        let s = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_str()).map_or(Value::Null, |x| Value::Text(x.to_string()));
        let mut attrs = vec![
            s(it.get("id")),
            s(it.get("collection")),
            s(props.and_then(|p| p.get("datetime").filter(|d| !d.is_null()).or_else(|| p.get("start_datetime")))),
            props.and_then(|p| p.get("eo:cloud_cover")).and_then(|c| c.as_f64()).map_or(Value::Null, Value::Real),
        ];
        for a in &asset_names {
            attrs.push(s(it.get("assets").and_then(|x| x.get(a)).and_then(|x| x.get("href"))));
        }
        let geom = it
            .get("geometry")
            .filter(|g| !g.is_null())
            .and_then(|g| crate::geomtext::parse(&g.to_string(), Some(crate::geomtext::Kind::GeoJson)).ok())
            .map(|g| g.0);
        out.features.push(Feature { attrs, geom, z: None });
    }
    ctx.notes.info(format!("{} items", fmt_count(out.len() as u64)));
    if out.len() >= most {
        ctx.notes.info(format!("stopped at {} items; raise At most items for more", most));
    }
    Ok(vec![("Items".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "StacSearch",
        title: "STAC search",
        hidden: false,
        group: Group::Transformer,
        category: "Web",
        desc: "Finds satellite images, aerial photos and height models in a STAC catalogue, such as those of Lantmäteriet, Copernicus or Element 84: in an area, at a time, in chosen collections. Each item becomes its outline, with its date, its cloud cover and the address of each of its files, which Input can read.",
        inputs: &["Area"],
        outputs: &["Items"],
        ports: None,
        params: STAC_PARAMS,
        run: run_stac,
        summary: |p| p.get("collections").filter(|c| !c.is_empty()).unwrap_or_else(|| "all collections".into()),
    },
    NodeType {
        name: "HttpCaller",
        title: "HTTP request",
        hidden: false,
        group: Group::Transformer,
        category: "Web",
        desc: "Sends a request for each feature that comes in, or one request when nothing is connected to Input, and puts the answer in an attribute, for example JSON from an API or a web page. GET, or POST, PUT, PATCH and DELETE with a body. Those that get no good answer go to Failed.",
        inputs: &["Input"],
        outputs: &["Output", "Failed"],
        ports: None,
        params: HTTP_PARAMS,
        run: run_http,
        summary: |p| p.get("url").unwrap_or_default(),
    },
];
