//! Google Sheets: a sheet shared with a link is read as the CSV Google gives of it; a
//! private sheet is read and written through the Sheets API, with a Google service account
//! that the sheet is shared with, kept as an API connection.

use crate::io::{Backend, Notes};
use crate::model::{err, GbError, Result, Table, Value};
use crate::web::{ApiConnection, Client};

/// Where the sheets are, and their API. Tests point them at a server on this computer.
fn docs_base() -> String {
    std::env::var("OGRAPHIS_GOOGLE_DOCS").unwrap_or_else(|_| "https://docs.google.com".into())
}
fn api_base() -> String {
    std::env::var("OGRAPHIS_GOOGLE_SHEETS_API").unwrap_or_else(|_| "https://sheets.googleapis.com".into())
}

/// Whether an address is that of a Google sheet.
pub fn is_sheet(url: &str) -> bool {
    let u = url.trim();
    (u.starts_with("https://docs.google.com/spreadsheets/d/") || u.starts_with(&format!("{}/spreadsheets/d/", docs_base()))) && sheet_id(u).is_some()
}

/// The id of the spreadsheet in an address, and the gid of the sheet when it names one.
pub fn sheet_id(url: &str) -> Option<(String, Option<String>)> {
    let rest = url.split("/spreadsheets/d/").nth(1)?;
    let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    if id.is_empty() {
        return None;
    }
    let gid = rest
        .split(['#', '?', '&'])
        .find_map(|part| part.strip_prefix("gid="))
        .map(|g| g.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .filter(|g| !g.is_empty());
    Some((id, gid))
}

fn quote_csv(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn cell_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        other => other.to_string(),
    }
}

/// The sheets of a spreadsheet, as (title, gid), through the API.
fn sheets_of(client: &mut Client, id: &str) -> Result<Vec<(String, String)>> {
    let a = client.send("GET", &format!("{}/v4/spreadsheets/{}?fields=sheets.properties", api_base(), id), &[], None)?;
    if a.status >= 400 {
        return err(answer_error("The spreadsheet could not be opened", a.status, &a.body));
    }
    let v: serde_json::Value = serde_json::from_str(&a.body).map_err(|_| GbError("The answer of Google Sheets is not JSON.".into()))?;
    Ok(v["sheets"]
        .as_array()
        .map(|s| {
            s.iter()
                .map(|sh| {
                    (
                        sh["properties"]["title"].as_str().unwrap_or("").to_string(),
                        sh["properties"]["sheetId"].as_i64().map(|g| g.to_string()).unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default())
}

fn answer_error(what: &str, status: u16, body: &str) -> String {
    let said = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or_default();
    let hint = match status {
        401 => " The key of the service account was not taken: check the API connection.",
        403 => " Share the sheet with the e-mail address of the service account (client_email in its key), and turn on the Google Sheets API for its project.",
        404 => " Check the address of the sheet.",
        _ => "",
    };
    format!("{}: Google answered {}{}.{}", what, status, if said.is_empty() { String::new() } else { format!(", {}", said) }, hint)
}

/// The sheet to use: by its name, by the gid of the address, or the first.
fn choose(sheets: &[(String, String)], name: Option<&str>, gid: Option<&str>) -> Result<String> {
    if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
        return sheets.iter().find(|s| s.0 == n).or_else(|| sheets.iter().find(|s| s.0.eq_ignore_ascii_case(n))).map(|s| s.0.clone()).ok_or_else(|| {
            GbError(format!("The spreadsheet has no sheet called {}. It has: {}.", n, sheets.iter().map(|s| s.0.as_str()).collect::<Vec<_>>().join(", ")))
        });
    }
    if let Some(g) = gid {
        if let Some(s) = sheets.iter().find(|s| s.1 == g) {
            return Ok(s.0.clone());
        }
    }
    sheets.first().map(|s| s.0.clone()).ok_or_else(|| GbError("The spreadsheet has no sheets.".into()))
}

/// A sheet as CSV text, and its name: through the API with a service account, or as Google
/// gives it of a sheet shared with a link.
pub fn read_csv(url: &str, conn: Option<ApiConnection>, sheet: Option<&str>, backend: &dyn Backend, stopped: &dyn Fn() -> bool) -> Result<(String, String)> {
    let (id, gid) = sheet_id(url).ok_or_else(|| GbError("That is not the address of a Google sheet.".into()))?;
    match conn {
        Some(c) => {
            let mut client = Client::new(backend, Some(c), stopped);
            let title = choose(&sheets_of(&mut client, &id)?, sheet, gid.as_deref())?;
            let range = crate::web::encode(&title);
            let a = client.send(
                "GET",
                &format!("{}/v4/spreadsheets/{}/values/{}?valueRenderOption=UNFORMATTED_VALUE&dateTimeRenderOption=FORMATTED_STRING", api_base(), id, range),
                &[],
                None,
            )?;
            if a.status >= 400 {
                return err(answer_error(&format!("The sheet {} could not be read", title), a.status, &a.body));
            }
            let v: serde_json::Value = serde_json::from_str(&a.body).map_err(|_| GbError("The answer of Google Sheets is not JSON.".into()))?;
            let rows = v["values"].as_array().cloned().unwrap_or_default();
            let width = rows.iter().filter_map(|r| r.as_array()).map(|r| r.len()).max().unwrap_or(0);
            let mut csv = String::new();
            for r in &rows {
                let cells = r.as_array().cloned().unwrap_or_default();
                let line: Vec<String> = (0..width).map(|k| quote_csv(&cells.get(k).map(cell_text).unwrap_or_default())).collect();
                csv.push_str(&line.join(","));
                csv.push('\n');
            }
            Ok((title, csv))
        }
        None => {
            if sheet.is_some_and(|s| !s.trim().is_empty()) {
                return err("A sheet chosen by its name is read through an API connection with a Google service account. Without one, put the gid of the sheet in the address, as Google shows it.");
            }
            let address = format!("{}/spreadsheets/d/{}/export?format=csv&gid={}", docs_base(), id, gid.as_deref().unwrap_or("0"));
            let mut client = Client::new(backend, None, stopped);
            let a = client.send("GET", &address, &[], None)?;
            let html = a.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.contains("html")) || a.body.trim_start().starts_with('<');
            if a.status >= 400 || html {
                return err("The sheet is not shared with a link, so Google did not give it. Share it as anyone with the link can view, or choose an API connection with a Google service account that the sheet is shared with.");
            }
            Ok((format!("sheet {}", gid.unwrap_or_else(|| "0".into())), a.body))
        }
    }
}

/// The value of an attribute as a cell.
fn cell(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::String(String::new()),
        Value::Bool(b) => serde_json::json!(if *b { 1 } else { 0 }),
        Value::Int(i) => serde_json::json!(i),
        Value::Real(r) if r.is_finite() => serde_json::json!(r),
        Value::Real(_) => serde_json::Value::String(String::new()),
        Value::Text(t) => serde_json::Value::String(t.clone()),
    }
}

/// Writes a stream to a sheet through the API, over what is there or after it: a row of
/// names and a row for each feature, with the geometry as WKT. Gives the rows and the sheet.
#[allow(clippy::too_many_arguments)]
pub fn write(
    table: &Table,
    url: &str,
    conn: ApiConnection,
    sheet: Option<&str>,
    append: bool,
    backend: &dyn Backend,
    stopped: &dyn Fn() -> bool,
    notes: &mut Notes,
) -> Result<(usize, String)> {
    let (id, gid) = sheet_id(url).ok_or_else(|| GbError("That is not the address of a Google sheet.".into()))?;
    let mut client = Client::new(backend, Some(conn), stopped);
    let sheets = sheets_of(&mut client, &id)?;
    let json = [("Content-Type".to_string(), "application/json".to_string())];
    // A sheet named that is not there is added.
    let title = match sheet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) if !sheets.iter().any(|s| s.0 == n) => {
            let body = serde_json::json!({"requests": [{"addSheet": {"properties": {"title": n}}}]}).to_string();
            let a = client.send("POST", &format!("{}/v4/spreadsheets/{}:batchUpdate", api_base(), id), &json, Some(&body))?;
            if a.status >= 400 {
                return err(answer_error(&format!("The sheet {} could not be added", n), a.status, &a.body));
            }
            notes.info(format!("the sheet {} was added", n));
            n.to_string()
        }
        Some(n) => n.to_string(),
        None => choose(&sheets, None, gid.as_deref())?,
    };
    let with_geom = table.features.iter().any(|f| f.geom.is_some());
    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(table.len() + 1);
    let mut names: Vec<serde_json::Value> = table.fields.iter().map(|f| serde_json::Value::String(f.name.clone())).collect();
    if with_geom {
        names.push(serde_json::Value::String("wkt".into()));
    }
    for f in &table.features {
        let mut r: Vec<serde_json::Value> = f.attrs.iter().map(cell).collect();
        if with_geom {
            r.push(serde_json::Value::String(f.geom.as_ref().map(wkt::ToWkt::wkt_string).unwrap_or_default()));
        }
        rows.push(serde_json::Value::Array(r));
    }
    let range = crate::web::encode(&title);
    if append {
        // After the rows that are there; the names only when the sheet is empty.
        let a = client.send("GET", &format!("{}/v4/spreadsheets/{}/values/{}!A1:A1", api_base(), id, range), &[], None)?;
        let empty =
            a.status < 400 && serde_json::from_str::<serde_json::Value>(&a.body).ok().is_none_or(|v| v["values"].as_array().is_none_or(|x| x.is_empty()));
        if empty {
            rows.insert(0, serde_json::Value::Array(names));
        }
        let body = serde_json::json!({ "values": rows }).to_string();
        let a = client.send(
            "POST",
            &format!("{}/v4/spreadsheets/{}/values/{}!A1:append?valueInputOption=RAW&insertDataOption=INSERT_ROWS", api_base(), id, range),
            &json,
            Some(&body),
        )?;
        if a.status >= 400 {
            return err(answer_error(&format!("The rows could not be added to {}", title), a.status, &a.body));
        }
    } else {
        let a = client.send("POST", &format!("{}/v4/spreadsheets/{}/values/{}:clear", api_base(), id, range), &json, Some("{}"))?;
        if a.status >= 400 {
            return err(answer_error(&format!("The sheet {} could not be emptied", title), a.status, &a.body));
        }
        rows.insert(0, serde_json::Value::Array(names));
        let body = serde_json::json!({ "values": rows }).to_string();
        let a = client.send("PUT", &format!("{}/v4/spreadsheets/{}/values/{}!A1?valueInputOption=RAW", api_base(), id, range), &json, Some(&body))?;
        if a.status >= 400 {
            return err(answer_error(&format!("The sheet {} could not be written", title), a.status, &a.body));
        }
    }
    Ok((table.len(), title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_and_the_sheet_are_taken_from_the_address() {
        let u = "https://docs.google.com/spreadsheets/d/1AbC_d-9/edit#gid=123456";
        assert!(is_sheet(u));
        assert_eq!(sheet_id(u), Some(("1AbC_d-9".to_string(), Some("123456".to_string()))));
        assert_eq!(sheet_id("https://docs.google.com/spreadsheets/d/XYZ/edit?usp=sharing"), Some(("XYZ".to_string(), None)));
        assert!(!is_sheet("https://example.com/data.csv"));
        assert_eq!(quote_csv("a,b"), "\"a,b\"");
    }
}
