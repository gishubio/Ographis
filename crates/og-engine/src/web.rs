//! Talking to web APIs: what HTTP request and the inputs share. A request is sent with the
//! credentials of an API connection, at the pace a service allows, sent again when the
//! service is busy or briefly away, and followed page by page when the answer comes in
//! pages.
//!
//! API connections are kept among the settings of the user, like database connections, so
//! that keys and secrets stay out of model files. A model names a connection; where a key
//! goes in an address or a body, it is written {{key}}.

use crate::io::{Backend, HttpAnswer};
use crate::json::{self, Step};
use crate::model::{err, GbError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A request with the connection put in: the address, the headers and the body.
type Prepared = (String, Vec<(String, String)>, Option<String>);

/// How a service wants to know who asks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiConnection {
    pub name: String,
    /// "key_header": the key in a header, such as X-Api-Key; "key_query": the key as a
    /// parameter of the address, such as ?key=; "key_text": only where {{key}} is written;
    /// "bearer": Authorization: Bearer and the key; "basic": a user and a password;
    /// "oauth2": a token fetched with a client id and a secret (client credentials).
    #[serde(default)]
    pub auth: String,
    /// The name of the header or of the parameter of the address.
    #[serde(default)]
    pub param: String,
    /// The key, the password or the client secret.
    #[serde(default)]
    pub secret: String,
    /// The user, or the client id.
    #[serde(default)]
    pub user: String,
    /// Where an OAuth2 token is fetched.
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub scope: String,
    /// The addresses the connection is for, so that it is not sent elsewhere by mistake.
    /// Empty sends it to any address.
    #[serde(default)]
    pub base_url: String,
}

pub const AUTH_KINDS: &[(&str, &str)] = &[
    ("key_header", "A key in a header, such as X-Api-Key"),
    ("key_query", "A key in the address, such as ?key="),
    ("key_text", "A key only where {{key}} is written"),
    ("bearer", "A token: Authorization: Bearer"),
    ("basic", "A user and a password"),
    ("oauth2", "OAuth2: a client id and a secret"),
];

impl ApiConnection {
    /// The key that {{key}} stands for: the key, or the token for a connection that fetches one.
    fn key(&self) -> &str {
        &self.secret
    }
}

/// An API connection by its name, or an error that says where they are made.
pub fn connection(name: &str) -> Result<ApiConnection> {
    let s = crate::settings::current()?;
    let name = name.trim();
    s.apis.iter().find(|c| c.name.trim() == name).or_else(|| s.apis.iter().find(|c| c.name.trim().eq_ignore_ascii_case(name))).cloned().ok_or_else(|| {
        let known: Vec<&str> = s.apis.iter().map(|c| c.name.as_str()).collect();
        let list = if known.is_empty() { "There are none yet.".to_string() } else { format!("There are: {}.", known.join(", ")) };
        GbError(format!("There is no API connection called {} on this computer. {} Add it under Edit → Settings → API connections.", name, list))
    })
}

/// How the pages of an answer are followed.
#[derive(Debug, Clone, PartialEq)]
pub enum Paging {
    None,
    /// The address of the next page is in the answer, at a path such as next_page or
    /// links[rel=next].href, or else in the Link header.
    Next {
        path: Option<Vec<Step>>,
    },
    /// An offset that grows by the size of a page: ?offset=0&limit=100, then 100, 200…
    Offset {
        offset_param: String,
        limit_param: String,
        size: u64,
        start: u64,
        items: Option<Vec<Step>>,
    },
    /// A page number: ?page=1, 2, 3…, with the size of a page if the service takes one.
    Page {
        page_param: String,
        size_param: String,
        size: u64,
        start: u64,
        items: Option<Vec<Step>>,
    },
}

/// Sends requests for one node: with its connection, its pace and its patience.
pub struct Client<'a> {
    backend: &'a dyn Backend,
    auth: Option<ApiConnection>,
    pub timeout_s: u64,
    /// How many times a request is sent again when the service is busy or away.
    pub retries: u32,
    /// The least time between two requests.
    pub gap: Duration,
    last: Option<Instant>,
    stopped: &'a dyn Fn() -> bool,
    /// How many requests were sent again, for the log.
    pub resent: u32,
}

static TOKENS: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);

impl<'a> Client<'a> {
    pub fn new(backend: &'a dyn Backend, auth: Option<ApiConnection>, stopped: &'a dyn Fn() -> bool) -> Self {
        Client { backend, auth, timeout_s: 30, retries: 3, gap: Duration::ZERO, last: None, stopped, resent: 0 }
    }

    /// At most this many requests a second. 0 or less is as fast as the service answers.
    pub fn per_second(mut self, n: f64) -> Self {
        self.gap = if n > 0.0 { Duration::from_secs_f64(1.0 / n) } else { Duration::ZERO };
        self
    }

    /// Hides the secrets of the connection in a text, such as an error with the address.
    pub fn hide(&self, text: &str) -> String {
        match &self.auth {
            Some(a) if a.secret.len() >= 4 => text.replace(&a.secret, "•••"),
            _ => text.to_string(),
        }
    }

    /// Sends a request, with the connection, and sends it again when the service is busy.
    /// An answer with any status is Ok; an error is when no answer came at all.
    pub fn send(&mut self, method: &str, url: &str, headers: &[(String, String)], body: Option<&str>) -> Result<HttpAnswer> {
        let mut tried_token = false;
        let mut attempt = 0u32;
        loop {
            let (u, h, b) = self.prepare(url, headers, body)?;
            self.wait_turn();
            let answer = self.backend.http_send(method, &u, &h, b.as_deref(), self.timeout_s);
            match answer {
                // A token that has run out is fetched again, once.
                Ok(a) if a.status == 401 && !tried_token && self.auth.as_ref().is_some_and(|c| c.auth == "oauth2") => {
                    tried_token = true;
                    forget_token(self.auth.as_ref().expect("checked"));
                }
                Ok(a) if matches!(a.status, 429 | 502 | 503 | 504) && attempt < self.retries => {
                    attempt += 1;
                    self.resent += 1;
                    let wait = retry_after(&a).unwrap_or_else(|| backoff(attempt));
                    self.sleep(wait)?;
                }
                Ok(a) => return Ok(a),
                Err(e) if attempt < self.retries && !(self.stopped)() && (e.0.starts_with("Cannot reach") || e.0.starts_with("No answer")) => {
                    attempt += 1;
                    self.resent += 1;
                    self.sleep(backoff(attempt))?;
                }
                Err(e) => return Err(GbError(self.hide(&e.0))),
            }
        }
    }

    /// The address and the headers with the connection put in, for those who send the
    /// request themselves, such as GDAL reading a web service.
    pub fn prepared(&mut self, url: &str) -> Result<(String, Vec<(String, String)>)> {
        let (u, h, _) = self.prepare(url, &[], None)?;
        Ok((u, h))
    }

    /// The address, headers and body with the connection put in.
    fn prepare(&mut self, url: &str, headers: &[(String, String)], body: Option<&str>) -> Result<Prepared> {
        let mut h: Vec<(String, String)> = headers.to_vec();
        let Some(c) = self.auth.clone() else {
            if url.contains("{{key}}") || body.is_some_and(|b| b.contains("{{key}}")) {
                return err("{{key}} is written, but no API connection is chosen. Choose the connection with the key.");
            }
            return Ok((url.to_string(), h, body.map(|b| b.to_string())));
        };
        let base = c.base_url.trim();
        if !base.is_empty() && !url.trim().starts_with(base) {
            return err(format!("The API connection {} is for addresses that begin with {}, so it is not sent to {}.", c.name, base, self.hide(url)));
        }
        let key = match c.auth.as_str() {
            "oauth2" => self.token(&c)?,
            _ => c.key().to_string(),
        };
        let mut u = url.replace("{{key}}", &encode(&key));
        let b = body.map(|b| b.replace("{{key}}", &key));
        let param = c.param.trim();
        match c.auth.as_str() {
            "key_header" => h.push((if param.is_empty() { "X-Api-Key".into() } else { param.to_string() }, key)),
            "key_query" => u = set_query(&u, if param.is_empty() { "key" } else { param }, &key),
            "bearer" | "oauth2" => h.push(("Authorization".into(), format!("Bearer {}", key))),
            "basic" => h.push(("Authorization".into(), format!("Basic {}", base64(&format!("{}:{}", c.user, c.secret))))),
            _ => {}
        }
        Ok((u, h, b))
    }

    /// A token for an OAuth2 connection, fetched again a minute before it runs out.
    fn token(&mut self, c: &ApiConnection) -> Result<String> {
        if let Some((t, until)) = TOKENS.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&c.name).cloned())) {
            if Instant::now() + Duration::from_secs(60) < until {
                return Ok(t);
            }
        }
        if c.token_url.trim().is_empty() {
            return err(format!("The API connection {} has no address to fetch a token from.", c.name));
        }
        let mut form = String::from("grant_type=client_credentials");
        if !c.scope.trim().is_empty() {
            form.push_str(&format!("&scope={}", encode(c.scope.trim())));
        }
        // The client id and the secret go both as Basic and in the form, as services differ.
        form.push_str(&format!("&client_id={}&client_secret={}", encode(&c.user), encode(&c.secret)));
        let headers = vec![
            ("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string()),
            ("Authorization".to_string(), format!("Basic {}", base64(&format!("{}:{}", c.user, c.secret)))),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        self.wait_turn();
        let a = self.backend.http_send("POST", c.token_url.trim(), &headers, Some(&form), self.timeout_s).map_err(|e| GbError(self.hide(&e.0)))?;
        if a.status >= 400 {
            return err(format!(
                "The token for {} could not be fetched: the server answered {}. Check the client id and the secret. {}",
                c.name,
                a.status,
                self.hide(&a.body.chars().take(200).collect::<String>())
            ));
        }
        let v = json::parse(&a.body).map_err(|_| GbError(format!("The answer with the token for {} is not JSON.", c.name)))?;
        let token =
            v.get("access_token").and_then(|t| t.as_str()).ok_or_else(|| GbError(format!("The answer for {} has no access_token.", c.name)))?.to_string();
        let life = v.get("expires_in").and_then(|e| e.as_f64().or_else(|| e.as_str().and_then(|s| s.parse().ok()))).unwrap_or(3600.0);
        if let Ok(mut g) = TOKENS.lock() {
            g.get_or_insert_with(HashMap::new).insert(c.name.clone(), (token.clone(), Instant::now() + Duration::from_secs_f64(life.max(1.0))));
        }
        Ok(token)
    }

    fn wait_turn(&mut self) {
        if let Some(last) = self.last {
            let since = last.elapsed();
            if since < self.gap {
                std::thread::sleep(self.gap - since);
            }
        }
        self.last = Some(Instant::now());
    }

    fn sleep(&self, d: Duration) -> Result<()> {
        let until = Instant::now() + d.min(Duration::from_secs(120));
        while Instant::now() < until {
            if (self.stopped)() {
                return err(crate::model::STOPPED);
            }
            std::thread::sleep(Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())));
        }
        Ok(())
    }

    /// Sends a request and follows its pages. `each` gets every page as it comes, with its
    /// number from 1, and says whether to go on. Pages stop when there is no next one, a
    /// page has no items or fewer than a full page, an answer is not a success, or at
    /// `most` pages.
    #[allow(clippy::too_many_arguments)]
    pub fn pages(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<&str>,
        paging: &Paging,
        most: usize,
        each: &mut dyn FnMut(usize, &str, &HttpAnswer) -> bool,
    ) -> Result<usize> {
        let mut address = match paging {
            Paging::Offset { offset_param, limit_param, size, start, .. } => {
                let u = set_query(url, offset_param, &start.to_string());
                if limit_param.is_empty() {
                    u
                } else {
                    set_query(&u, limit_param, &size.to_string())
                }
            }
            Paging::Page { page_param, size_param, size, start, .. } => {
                let u = set_query(url, page_param, &start.to_string());
                if size_param.is_empty() {
                    u
                } else {
                    set_query(&u, size_param, &size.to_string())
                }
            }
            _ => url.to_string(),
        };
        let mut seen = std::collections::HashSet::new();
        let mut n = 0usize;
        loop {
            if (self.stopped)() {
                return err(crate::model::STOPPED);
            }
            n += 1;
            seen.insert(address.clone());
            let a = self.send(method, &address, headers, body)?;
            let fine = a.status < 400;
            if !each(n, &address, &a) || !fine || matches!(paging, Paging::None) || n >= most {
                return Ok(n);
            }
            let parsed = json::parse(&a.body).ok();
            let count = |items: &Option<Vec<Step>>| -> Option<usize> {
                let v = parsed.as_ref()?;
                let list = match items {
                    Some(p) => json::get(v, p)?,
                    None => v,
                };
                list.as_array().map(|l| l.len()).or_else(|| first_list(list).map(|l| l.len()))
            };
            let next = match paging {
                Paging::None => None,
                Paging::Next { path } => {
                    let from_body = match (path, &parsed) {
                        (Some(p), Some(v)) => json::get(v, p).and_then(|x| x.as_str()).map(|s| s.to_string()),
                        _ => None,
                    };
                    from_body.or_else(|| link_next(&a)).filter(|s| !s.trim().is_empty()).map(|s| resolve(&address, s.trim()))
                }
                Paging::Offset { offset_param, size, start, items, .. } => match count(items) {
                    Some(k) if k > 0 && (k as u64) >= *size => Some(set_query(&address, offset_param, &(start + size * n as u64).to_string())),
                    _ => None,
                },
                Paging::Page { page_param, size, size_param, start, items } => match count(items) {
                    Some(k) if k > 0 && (size_param.is_empty() || (k as u64) >= *size) => {
                        Some(set_query(&address, page_param, &(start + n as u64).to_string()))
                    }
                    _ => None,
                },
            };
            match next {
                Some(u) if !seen.contains(&u) => address = u,
                _ => return Ok(n),
            }
        }
    }
}

fn forget_token(c: &ApiConnection) {
    if let Ok(mut g) = TOKENS.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(&c.name);
        }
    }
}

/// The first list in an object, for a page whose items are not pointed out.
fn first_list(v: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    v.as_object()?.values().find_map(|x| x.as_array())
}

/// How long to wait after a busy answer: Retry-After in seconds, if the service says.
fn retry_after(a: &HttpAnswer) -> Option<Duration> {
    let v = a.header("retry-after")?;
    v.trim().parse::<f64>().ok().map(|s| Duration::from_secs_f64(s.clamp(0.0, 120.0)))
}

/// 1, 2, 4, 8 … seconds, at most 30.
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.saturating_sub(1).min(5)).min(30))
}

/// The address of the next page in a Link header: <https://…>; rel="next".
fn link_next(a: &HttpAnswer) -> Option<String> {
    let v = a.header("link")?;
    v.split(',').find(|part| part.split(';').skip(1).any(|p| p.trim().trim_start_matches("rel=").trim_matches('"') == "next")).and_then(|part| {
        let u = part.split(';').next()?.trim();
        Some(u.trim_start_matches('<').trim_end_matches('>').to_string())
    })
}

/// An address relative to another, as a next link may be.
pub fn resolve(base: &str, next: &str) -> String {
    if next.starts_with("http://") || next.starts_with("https://") {
        return next.to_string();
    }
    let scheme_end = base.find("://").map_or(0, |i| i + 3);
    if next.starts_with("//") {
        return format!("{}{}", &base[..scheme_end.saturating_sub(2)], next);
    }
    let host_end = base[scheme_end..].find('/').map_or(base.len(), |i| scheme_end + i);
    if next.starts_with('/') {
        return format!("{}{}", &base[..host_end], next);
    }
    if next.starts_with('?') {
        return format!("{}{}", base.split('?').next().unwrap_or(base), next);
    }
    let path = base.split('?').next().unwrap_or(base);
    let dir = path.rfind('/').filter(|i| *i >= host_end).map_or(path.len(), |i| i + 1);
    format!("{}{}", &path[..dir.max(host_end)], if dir <= host_end { format!("/{}", next) } else { next.to_string() })
}

/// The address with a parameter set, replaced if it is there.
pub fn set_query(url: &str, name: &str, value: &str) -> String {
    let (path, frag) = match url.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (url, None),
    };
    let (base, query) = path.split_once('?').unwrap_or((path, ""));
    let mut parts: Vec<String> = query.split('&').filter(|q| !q.is_empty()).map(|q| q.to_string()).collect();
    let pair = format!("{}={}", name, encode(value));
    match parts.iter().position(|q| q.split('=').next() == Some(name)) {
        Some(i) => parts[i] = pair,
        None => parts.push(pair),
    }
    let mut out = format!("{}?{}", base, parts.join("&"));
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

/// Text made safe for an address: letters, digits and -_.~ stay, the rest is %XX.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub fn base64(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Reads the paging settings of a node: paging, next_path, items_path, page_param,
/// size_param, page_size and start.
pub fn paging_of(get: &dyn Fn(&str) -> Option<String>) -> Result<Paging> {
    let path = |k: &str| -> Result<Option<Vec<Step>>> {
        match get(k).filter(|s| !s.trim().is_empty()) {
            Some(p) => Ok(Some(json::parse_path(&p)?)),
            None => Ok(None),
        }
    };
    let num = |k: &str, d: u64| -> Result<u64> {
        match get(k).filter(|s| !s.trim().is_empty()) {
            None => Ok(d),
            Some(s) => s.trim().parse().map_err(|_| GbError(format!("\"{}\" is not a whole number.", s))),
        }
    };
    let text = |k: &str, d: &str| get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| d.to_string());
    Ok(match get("paging").as_deref() {
        Some("next") => Paging::Next { path: path("next_path")? },
        Some("offset") => Paging::Offset {
            offset_param: text("page_param", "offset"),
            limit_param: get("size_param").map(|s| s.trim().to_string()).unwrap_or_else(|| "limit".into()),
            size: num("page_size", 100)?.max(1),
            start: num("start", 0)?,
            items: path("items_path")?,
        },
        Some("page") => Paging::Page {
            page_param: text("page_param", "page"),
            size_param: get("size_param").map(|s| s.trim().to_string()).unwrap_or_default(),
            size: num("page_size", 100)?.max(1),
            start: num("start", 1)?,
            items: path("items_path")?,
        },
        _ => Paging::None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_built_and_followed() {
        assert_eq!(set_query("https://a.se/x?offset=0&limit=10", "offset", "20"), "https://a.se/x?offset=20&limit=10");
        assert_eq!(set_query("https://a.se/x", "q", "Borås kommun"), "https://a.se/x?q=Bor%C3%A5s%20kommun");
        assert_eq!(resolve("https://a.se/v2/kpi?page=1", "https://b.se/n"), "https://b.se/n");
        assert_eq!(resolve("https://a.se/v2/kpi?page=1", "/v2/kpi?page=2"), "https://a.se/v2/kpi?page=2");
        assert_eq!(resolve("https://a.se/v2/kpi?page=1", "?page=2"), "https://a.se/v2/kpi?page=2");
        assert_eq!(resolve("https://a.se/v2/kpi", "items?p=2"), "https://a.se/v2/items?p=2");
        assert_eq!(base64("Aladdin:open sesame"), "QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
        assert_eq!(base64("ab"), "YWI=");
        let a = HttpAnswer {
            status: 200,
            body: String::new(),
            headers: vec![("Link".into(), r#"<https://a.se/p?page=1>; rel="prev", <https://a.se/p?page=3>; rel="next""#.into())],
        };
        assert_eq!(link_next(&a).as_deref(), Some("https://a.se/p?page=3"));
        assert_eq!(backoff(1), Duration::from_secs(1));
        assert_eq!(backoff(3), Duration::from_secs(4));
    }
}

#[cfg(test)]
mod service_tests {
    use crate::nodes::service_name;

    #[test]
    fn addresses_tell_what_service_they_are() {
        let s = |u: &str| service_name(u, None).map(|n| n.split(':').next().unwrap().to_string());
        assert_eq!(s("https://api.lantmateriet.se/ogc-features/v1/collections/byggnad/items").as_deref(), Some("OAPIF"));
        assert_eq!(s("https://geodata.skogsstyrelsen.se/arcgis/rest/services/Publikt/Nyckelbiotoper/MapServer/0").as_deref(), Some("ARCGIS"));
        assert_eq!(
            s("https://ext-geodata.lansstyrelsen.se/arcgis/services/x/MapServer/WFSServer?service=WFS").as_deref(),
            Some("ARCGIS"),
            "the ArcGIS path decides"
        );
        assert_eq!(s("https://karta.example.se/geoserver/wfs?service=WFS&request=GetCapabilities").as_deref(), Some("WFS"));
        assert_eq!(s("https://karta.example.se/geoserver/wfs?service=WFS&request=GetFeature&outputFormat=application/json"), None);
        assert_eq!(s("https://example.se/data/kommuner.geojson"), None);
        assert_eq!(service_name("https://example.se/x/collections/a/items", Some("file")), None);
    }
}
