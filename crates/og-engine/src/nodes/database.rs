//! PostGIS: input from and output to PostgreSQL with PostGIS, and Run SQL.
//! The connections are kept in the settings; a model names one.

use super::*;
use crate::io::ReadOpts;
use crate::model::{err, fmt_count, Field, FieldType, GbError, Result, Table, Value};
use std::path::Path;
use std::sync::Arc;

pub(super) const POSTGIS_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "connection",
        label: "Connection",
        kind: ParamKind::Connection,
        default: "",
        hint: "A database connection from Edit → Settings, by its name. The server, the user and the password are kept there, not in the model",
    },
    ParamSpec { key: "host", label: "Server", kind: ParamKind::Text, default: "localhost", hint: "The name or address of the database server" },
    ParamSpec { key: "port", label: "Port", kind: ParamKind::Text, default: "5432", hint: "" },
    ParamSpec { key: "database", label: "Database", kind: ParamKind::Text, default: "", hint: "" },
    ParamSpec { key: "user", label: "User", kind: ParamKind::Text, default: "", hint: "" },
    ParamSpec {
        key: "password",
        label: "Password",
        kind: ParamKind::Secret,
        default: "",
        hint: "Stored in the model file as it is. Write $(password) and set the variable to keep it out of the file",
    },
    ParamSpec {
        key: "tables",
        label: "Tables",
        kind: ParamKind::Layers,
        default: "",
        hint: "As schema.table. Several tables become one stream, with the name of each in the attribute table",
    },
    ParamSpec {
        key: "where",
        label: "Only rows where",
        kind: ParamKind::Text,
        default: "",
        hint: "A condition in SQL, such as kommun = 'Skövde', so that the database sends only those rows",
    },
    ParamSpec {
        key: "sql",
        label: "SQL query",
        kind: ParamKind::Text,
        default: "",
        hint: "Read what a query gives instead of tables, such as SELECT name, geom FROM vgr.sites WHERE ...",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "", hint: "Used when a table does not state its own" },
    ParamSpec {
        key: "limit",
        label: "Largest number of features",
        kind: ParamKind::Text,
        default: "",
        hint: "Reads only the first ones, for example 1000, to try a model out. Empty reads all",
    },
];

pub(super) const GEOMETRY_TYPES: &[(&str, &str)] = &[
    ("auto", "As the features are"),
    ("point", "Point"),
    ("multipoint", "MultiPoint"),
    ("line", "LineString"),
    ("multiline", "MultiLineString"),
    ("polygon", "Polygon"),
    ("multipolygon", "MultiPolygon"),
    ("none", "No geometry"),
];
pub(super) const WRITE_MODES: &[(&str, &str)] = &[("replace", "Replace the table"), ("append", "Add to the table")];

pub(super) const POSTGIS_WRITE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "format", label: "Format", kind: ParamKind::Choice(super::OUTPUT_FORMATS), default: "postgis", hint: "" },
    POSTGIS_PARAMS[0],
    POSTGIS_PARAMS[1],
    POSTGIS_PARAMS[2],
    POSTGIS_PARAMS[3],
    POSTGIS_PARAMS[4],
    POSTGIS_PARAMS[5],
    ParamSpec { key: "schema", label: "Schema", kind: ParamKind::Text, default: "public", hint: "It must be there in the database" },
    ParamSpec {
        key: "table",
        label: "Table",
        kind: ParamKind::Text,
        default: "",
        hint: "Made lower case, with spaces as _, as PostgreSQL prefers. The names of the columns too",
    },
    ParamSpec { key: "mode", label: "If the table is there", kind: ParamKind::Choice(WRITE_MODES), default: "replace", hint: "" },
    ParamSpec {
        key: "geometry",
        label: "Geometry type",
        kind: ParamKind::Choice(GEOMETRY_TYPES),
        default: "auto",
        hint: "A multi type takes single geometries too. A single type takes multi ones with one part",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "", hint: "The features are reprojected to it before they are written" },
    ParamSpec { key: "geom_column", label: "Geometry column", kind: ParamKind::Text, default: "geom", hint: "" },
    ParamSpec {
        key: "key",
        label: "Primary key",
        kind: ParamKind::Choice(super::KEYS),
        default: "new",
        hint: "The column that tells the rows apart. Only for a new table",
    },
    ParamSpec { key: "key_name", label: "Key column", kind: ParamKind::Text, default: "ogc_fid", hint: "The name of the new column of numbers" },
    ParamSpec {
        key: "key_attribute",
        label: "Key attribute",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "Its values must all be different and none empty",
    },
];

/// A value in the connection string of PostgreSQL, quoted.
pub(super) fn quoted(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The connection to the database as GDAL wants it, and the same without the password for
/// messages and the log.
pub fn connection(p: &P) -> Result<(String, String)> {
    // A named connection from the settings of the user, or, as in models from 0.61 and
    // 0.62, the settings written in the node.
    let named = match p.get("connection").filter(|c| !c.is_empty()) {
        Some(name) => Some(crate::settings::connection(&name)?),
        None => None,
    };
    let field = |key: &str| -> Option<String> {
        match &named {
            Some(c) => {
                let v = match key {
                    "host" => &c.host,
                    "port" => &c.port,
                    "database" => &c.database,
                    "user" => &c.user,
                    _ => &c.password,
                };
                Some(v.trim().to_string()).filter(|v| !v.is_empty())
            }
            None => p.get(key).filter(|v| !v.is_empty()),
        }
    };
    let database = match (field("database"), &named) {
        (Some(d), _) => d,
        (None, Some(c)) => return err(format!("The connection {} has no database. Set it under Edit → Settings → Database connections.", c.name)),
        (None, None) => return err("Choose a database connection."),
    };
    let host = field("host").unwrap_or_else(|| "localhost".into());
    let shown = match &named {
        Some(c) => format!("the connection {} ({} on {})", c.name, database, host),
        None => format!("the database {} on {}", database, host),
    };
    if named.as_ref().is_some_and(|c| c.kind == "mssql") {
        return Ok((mssql_string(&host, field("port").as_deref(), &database, field("user").as_deref(), field("password").as_deref()), shown));
    }
    let port = field("port").unwrap_or_else(|| "5432".into());
    let mut parts = vec![format!("host={}", quoted(&host)), format!("port={}", quoted(&port)), format!("dbname={}", quoted(&database))];
    if let Some(user) = field("user") {
        parts.push(format!("user={}", quoted(&user)));
    }
    if let Some(pw) = field("password") {
        parts.push(format!("password={}", quoted(&pw)));
    }
    // A server that does not answer is given up on after a while.
    parts.push("connect_timeout=10".into());
    Ok((format!("PG:{}", parts.join(" ")), shown))
}

/// The connection string of SQL Server for GDAL's MSSQLSpatial driver, through ODBC. Without
/// a user, the login of Windows is used. A value with ; or braces in it is put in braces.
pub fn mssql_string(host: &str, port: Option<&str>, database: &str, user: Option<&str>, password: Option<&str>) -> String {
    let v = |s: &str| if s.contains([';', '{', '}', '=']) { format!("{{{}}}", s.replace('}', "}}")) } else { s.to_string() };
    let server = match port.filter(|p| !p.trim().is_empty() && p.trim() != "1433") {
        Some(p) => format!("{},{}", host, p.trim()),
        None => host.to_string(),
    };
    let mut s = format!("MSSQL:server={};database={}", v(&server), v(database));
    match user {
        Some(u) => {
            s.push_str(&format!(";uid={};pwd={}", v(u), v(password.unwrap_or(""))));
        }
        None => s.push_str(";trusted_connection=yes"),
    }
    s
}

/// The connection string for a connection of the settings, for listing its tables.
pub fn named_connection(c: &crate::settings::Connection) -> Result<String> {
    if c.kind == "mssql" {
        let some = |s: &String| Some(s.trim().to_string()).filter(|v| !v.is_empty());
        if c.database.trim().is_empty() {
            return err(format!("The connection {} has no database.", c.name));
        }
        return Ok(mssql_string(c.host.trim(), some(&c.port).as_deref(), c.database.trim(), some(&c.user).as_deref(), some(&c.password).as_deref()));
    }
    let mut params = super::Params::new();
    for (k, v) in [("host", &c.host), ("port", &c.port), ("database", &c.database), ("user", &c.user), ("password", &c.password)] {
        params.insert(k.into(), serde_json::Value::String(v.clone()));
    }
    let p = P::new(&params, POSTGIS_PARAMS);
    Ok(connection(&p)?.0)
}

/// An error from GDAL, without the password, which the connection string holds.
pub(super) fn clean(e: GbError, conn: &str, shown: &str) -> GbError {
    let mut text = e.0.replace(conn, shown);
    if let Some(at) = text.find("password=") {
        let end = text[at..].find(' ').map_or(text.len(), |k| at + k);
        text.replace_range(at..end, "password=…");
    }
    if let Some(at) = text.find("pwd=") {
        let end = text[at..].find([';', ' ']).map_or(text.len(), |k| at + k);
        text.replace_range(at..end, "pwd=…");
    }
    GbError(text)
}

/// A table name for SQL: schema.table with each part quoted when it needs to be.
pub(super) fn sql_name(table: &str) -> String {
    table
        .split('.')
        .map(|part| {
            if part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && !part.is_empty() {
                part.to_string()
            } else {
                format!("\"{}\"", part.trim_matches('"').replace('"', "\"\""))
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn run_postgis(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (conn, shown) = connection(p)?;
    let b = ctx.backend("Reading from PostGIS")?;
    let crs = match p.get("crs") {
        Some(c) => Some(parse_crs(&c)?),
        None => None,
    };
    let limit = number_param(p, "limit", "The largest number of features")?.map(|l| l.max(0.0) as usize);
    let sql = p.get("sql");
    let tables = p.list("tables");
    let filter = p.get("where");
    let (progress, gate) = (ctx.progress, ctx.gate);
    let going = |done, total| {
        progress("read", done, total);
        gate.going()
    };
    let path = Path::new(&conn);
    let mut read: Vec<(Table, Option<String>)> = Vec::new();
    if let Some(q) = sql {
        let opts = ReadOpts { crs, limit, sql: Some(q), ..Default::default() };
        let t = b.read_with(path, &opts, &mut ctx.notes, &going).map_err(|e| clean(e, &conn, &shown))?;
        read.push((t, None));
    } else {
        if tables.is_empty() {
            return err("Choose the tables to read, or write an SQL query.");
        }
        for table in &tables {
            // A condition is sent to the database as a query, so that only those rows come.
            let opts = match &filter {
                Some(w) => ReadOpts { crs, limit, sql: Some(format!("SELECT * FROM {} WHERE {}", sql_name(table), w)), ..Default::default() },
                None => ReadOpts { crs, limit, layer: Some(table.clone()), ..Default::default() },
            };
            let t = b.read_with(path, &opts, &mut ctx.notes, &going).map_err(|e| clean(e, &conn, &shown))?;
            read.push((t, Some(table.clone())));
        }
    }
    let many = read.len() > 1;
    let tables: Vec<Arc<Table>> = read
        .into_iter()
        .map(|(mut t, name)| {
            if many {
                t.fields.push(Field::new("table", FieldType::Text));
                for f in t.features.iter_mut() {
                    f.attrs.push(name.clone().map_or(Value::Null, Value::Text));
                }
            }
            Arc::new(t)
        })
        .collect();
    let out = merge_tables(tables, ctx.backend, "tables", &mut ctx.notes)?;
    ctx.notes.info(format!("{} features from {}", fmt_count(out.len() as u64), shown));
    Ok(vec![("Output".into(), (*out).clone())])
}

pub(super) fn run_postgis_writer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (conn, shown) = connection(p)?;
    let table = super::input(inp, "Input");
    let name = p.need("table", "Say which table to write to.")?;
    let opts = crate::io::DbWrite {
        schema: p.get("schema").unwrap_or_else(|| "public".into()),
        table: name,
        geometry: p.get("geometry").unwrap_or_else(|| "auto".into()),
        geom_column: p.get("geom_column").unwrap_or_else(|| "geom".into()),
        append: p.get("mode").as_deref() == Some("append"),
        key: super::primary_key(p, "ogc_fid")?,
    };
    if table.image.is_some() || table.grid.is_some() {
        return err("A picture or a grid cannot be written to PostGIS. Write it to a file.");
    }
    let b = ctx.backend("Writing to PostGIS")?;
    let reprojected;
    let table = match p.get("crs") {
        Some(c) => {
            let to = parse_crs(&c)?;
            match table.crs {
                Some(from) if from != to => {
                    reprojected = super::reproject(table, to, Some(b))?;
                    &reprojected
                }
                Some(_) => table,
                None => return err("The stream has no coordinate system, so it cannot be reprojected. Set it on the input."),
            }
        }
        None => table,
    };
    let n = b.write_postgis(table, &conn, &opts, &mut ctx.notes).map_err(|e| clean(e, &conn, &shown))?;
    ctx.notes.info(format!("{} rows written to {} in {}", fmt_count(n), opts.table, shown));
    Ok(Vec::new())
}

// ---------- Run SQL ----------

pub(super) const SQL_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "connection",
        label: "Connection",
        kind: ParamKind::Connection,
        default: "",
        hint: "A database connection from Edit → Settings, by its name. The server, the user and the password are kept there, not in the model",
    },
    ParamSpec { key: "host", label: "Server", kind: ParamKind::Text, default: "localhost", hint: "The name or address of the database server" },
    ParamSpec { key: "port", label: "Port", kind: ParamKind::Text, default: "5432", hint: "" },
    ParamSpec { key: "database", label: "Database", kind: ParamKind::Text, default: "", hint: "" },
    ParamSpec { key: "user", label: "User", kind: ParamKind::Text, default: "", hint: "" },
    ParamSpec {
        key: "password",
        label: "Password",
        kind: ParamKind::Secret,
        default: "",
        hint: "Stored in the model file as it is. Write $(password) and set the variable to keep it out of the file",
    },
    ParamSpec {
        key: "sql",
        label: "SQL",
        kind: ParamKind::Code("Input"),
        default: "",
        hint: "Run once for each feature that comes in, or once if nothing is connected. {kommun} puts in the value of the attribute, quoted as SQL wants it: WHERE name = {kommun}, not '{kommun}'",
    },
    ParamSpec {
        key: "keep",
        label: "Keep the attributes that come in",
        kind: ParamKind::Choice(super::YES_NO),
        default: "true",
        hint: "Each row of the result gets the attributes of the feature it was asked for. Those of the result win when the names are the same",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "", hint: "Used when the result does not state its own" },
];

/// A value as SQL writes it: text in single quotes, numbers as they are, empty as NULL.
pub fn sql_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Int(i) => i.to_string(),
        Value::Real(x) if x.is_finite() => format!("{}", x),
        Value::Real(_) => "NULL".into(),
        Value::Text(t) => format!("'{}'", t.replace('\'', "''")),
    }
}

/// SQL with {expressions} in it: the text around them, and each expression. Braces whose
/// content is not an expression, such as in '{"a": 1}'::jsonb, are left as they are.
pub struct SqlTemplate {
    parts: Vec<(String, Option<crate::expr::Compiled>)>,
}

impl SqlTemplate {
    pub fn parse(text: &str, fields: &[String]) -> Result<SqlTemplate> {
        let mut parts = Vec::new();
        let mut rest = text;
        let mut plain = String::new();
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}').map(|c| open + c) else { break };
            let inside = &rest[open + 1..close];
            match crate::expr::compile(inside, fields) {
                Ok(e) => {
                    plain.push_str(&rest[..open]);
                    parts.push((std::mem::take(&mut plain), Some(e)));
                }
                Err(e) => {
                    // A plain name that is not an attribute is a mistake, not JSON.
                    let word = inside.trim().trim_matches('"');
                    if !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        return err(format!("{{{}}} in the SQL: {}", inside.trim(), e.0));
                    }
                    plain.push_str(&rest[..close + 1]);
                }
            }
            rest = &rest[close + 1..];
        }
        plain.push_str(rest);
        parts.push((plain, None));
        Ok(SqlTemplate { parts })
    }

    pub fn uses_attributes(&self) -> bool {
        self.parts.iter().any(|p| p.1.is_some())
    }

    pub fn fill(&self, f: &crate::model::Feature, i: usize, geographic: Option<bool>) -> Result<String> {
        let mut out = String::new();
        for (text, e) in &self.parts {
            out.push_str(text);
            if let Some(e) = e {
                out.push_str(&sql_literal(&e.eval(&crate::expr::Row { feature: f, index: i, geographic })?));
            }
        }
        Ok(out)
    }
}

pub(super) fn run_sql(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (conn, shown) = connection(p)?;
    let b = ctx.backend("Running SQL")?;
    let text = p.need("sql", "Write the SQL to run.")?;
    let crs = match p.get("crs") {
        Some(c) => Some(parse_crs(&c)?),
        None => None,
    };
    let alone_table;
    let alone = ctx.open_ports.contains(&"Input");
    let table: &Table = if alone {
        alone_table = Table { features: vec![crate::model::Feature { z: None, attrs: Vec::new(), geom: None }], ..Table::default() };
        &alone_table
    } else {
        super::input(inp, "Input")
    };
    let template = SqlTemplate::parse(&text, &table.field_names())?;
    if !alone && table.is_empty() {
        ctx.notes.warn("Nothing came in on Input, so the SQL was not run. Disconnect Input to run it once.");
        return Ok(vec![("Result".into(), Table::default()), ("Sent".into(), table.like()), ("Failed".into(), table.like())]);
    }
    if !alone && !template.uses_attributes() && table.len() > 1 {
        ctx.notes.info(format!("the SQL uses no attributes, and was run once for each of {} features", fmt_count(table.len() as u64)));
    }
    let geographic = ctx.geographic(table.crs);
    let statements: Vec<String> =
        table.features.iter().enumerate().map(|(i, f)| template.fill(f, i, geographic).map_err(|e| super::at_row(e, i))).collect::<Result<_>>()?;
    let (progress, gate) = (ctx.progress, ctx.gate);
    let going = |done, total| {
        progress("ran statements for", done, total);
        gate.going()
    };
    let opts = ReadOpts { crs, ..Default::default() };
    let answers = b.run_sql(Path::new(&conn), &statements, &opts, &mut ctx.notes, &going).map_err(|e| clean(e, &conn, &shown))?;
    let keep = !alone && p.get("keep").as_deref() != Some("false");
    let (mut sent, mut failed) = (table.like(), table.like());
    failed.fields.push(Field::new("_error", FieldType::Text));
    let mut parts: Vec<Arc<Table>> = Vec::new();
    let mut rows = 0usize;
    for (f, answer) in table.features.iter().zip(answers) {
        match answer {
            Ok(None) => sent.features.push(f.clone()),
            Ok(Some(t)) => {
                sent.features.push(f.clone());
                rows += t.len();
                let mut part = t;
                if part.crs.is_none() && part.has_geometry() {
                    part.crs = crs;
                }
                if keep {
                    // The attributes of the feature after those of the result, and its
                    // geometry where the result has none.
                    let extra: Vec<usize> = (0..table.fields.len()).filter(|i| part.field_index(&table.fields[*i].name).is_none()).collect();
                    part.fields.extend(extra.iter().map(|i| table.fields[*i].clone()));
                    let own_geometry = part.has_geometry();
                    for row in part.features.iter_mut() {
                        row.attrs.extend(extra.iter().map(|i| f.attrs[*i].clone()));
                        if !own_geometry {
                            row.geom = f.geom.clone();
                        }
                    }
                    if !own_geometry {
                        part.crs = table.crs;
                    }
                }
                // An empty answer only gives the result its attributes, when no other does.
                if !part.is_empty() || parts.is_empty() {
                    if parts.len() == 1 && parts[0].is_empty() {
                        parts.clear();
                    }
                    parts.push(Arc::new(part));
                }
            }
            Err(e) => {
                let mut row = f.clone();
                row.attrs.push(Value::Text(e.trim().to_string()));
                failed.features.push(row);
            }
        }
    }
    let result = merge_tables(parts, ctx.backend, "results", &mut ctx.notes)?;
    let n = statements.len();
    ctx.notes.info(format!(
        "{} {} run on {}, {} rows back",
        fmt_count(n as u64),
        if n == 1 { "statement" } else { "statements" },
        shown,
        fmt_count(rows as u64)
    ));
    if !failed.is_empty() {
        let first = failed.features[0].attrs.last().map(|v| v.to_text()).unwrap_or_default();
        ctx.notes.warn(format!("{} of them failed and went to Failed. The first: {}", fmt_count(failed.len() as u64), first));
    }
    Ok(vec![("Result".into(), (*result).clone()), ("Sent".into(), sent), ("Failed".into(), failed)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "PostgisReader",
        title: "PostGIS input",
        hidden: true,
        group: Group::Reader,
        category: "",
        desc: "Reads tables from a PostgreSQL database with PostGIS, or what an SQL query gives. Several tables become one stream. A condition can be sent along, so that the database sends only the rows that are wanted. A connection of the kind SQL Server reads from Microsoft SQL Server in the same way, with its geometry.",
        inputs: &[],
        outputs: &["Output"],
        ports: None,
        params: POSTGIS_PARAMS,
        run: run_postgis,
        summary: |p| {
            let db = p.get("connection").filter(|c| !c.is_empty()).or_else(|| p.get("database")).unwrap_or_else(|| "no connection".into());
            match (p.get("sql"), p.list("tables").as_slice()) {
                (Some(_), _) => format!("{} · SQL query", db),
                (None, []) => db,
                (None, [one]) => format!("{} · {}", db, one),
                (None, many) => format!("{} · {} tables", db, many.len()),
            }
        },
    },
    NodeType {
        name: "SqlExecutor",
        title: "Run SQL",
        hidden: false,
        group: Group::Transformer,
        category: "Database",
        desc: "Runs SQL against a PostgreSQL database with PostGIS: once for each feature that comes in, with its attributes put in where the SQL says {attribute}, or once if nothing is connected to Input. The rows the database sends back come out on Result, with geometry if they have it, for the rest of the model. Statements that send nothing back, such as UPDATE, can be run too. The features whose SQL ran go on to Sent; those it failed for to Failed, with the reason.",
        inputs: &["Input"],
        outputs: &["Result", "Sent", "Failed"],
        ports: None,
        params: SQL_PARAMS,
        run: run_sql,
        summary: |p| {
            let db = p.get("connection").filter(|c| !c.is_empty()).or_else(|| p.get("database")).unwrap_or_else(|| "no connection".into());
            let sql = p.get("sql").unwrap_or_default();
            let first = sql.split_whitespace().next().unwrap_or("").to_uppercase();
            if first.is_empty() { db } else { format!("{} · {}", db, first) }
        },
    },
    NodeType {
        name: "PostgisWriter",
        title: "PostGIS output",
        hidden: true,
        group: Group::Writer,
        category: "",
        desc: "Writes the stream to a table in a PostgreSQL database with PostGIS, in a schema, with the geometry type and coordinate system asked for. A connection of the kind SQL Server writes to Microsoft SQL Server, in dbo unless another schema is given, with a spatial index.",
        inputs: &["Input"],
        outputs: &[],
        ports: None,
        params: POSTGIS_WRITE_PARAMS,
        run: run_postgis_writer,
        summary: |p| {
            let db = p.get("connection").filter(|c| !c.is_empty()).or_else(|| p.get("database")).unwrap_or_else(|| "no connection".into());
            match p.get("table") {
                Some(t) => format!("{} · {}.{}", db, p.get("schema").unwrap_or_else(|| "public".into()), t),
                None => db,
            }
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_server_is_reached_through_odbc_with_a_user_or_the_login_of_windows() {
        let c = crate::settings::Connection {
            name: "MS".into(),
            kind: "mssql".into(),
            host: "sql01".into(),
            port: "1433".into(),
            database: "GIS".into(),
            user: "magnus".into(),
            password: "a;b".into(),
        };
        assert_eq!(named_connection(&c).unwrap(), "MSSQL:server=sql01;database=GIS;uid=magnus;pwd={a;b}");
        let windows = crate::settings::Connection { user: String::new(), password: String::new(), port: "14330".into(), ..c };
        assert_eq!(named_connection(&windows).unwrap(), "MSSQL:server=sql01,14330;database=GIS;trusted_connection=yes");
        let e = clean(GbError("Login failed for MSSQL:server=x;uid=magnus;pwd=hemligt;x".into()), "nothing", "the connection");
        assert!(!e.0.contains("hemligt"), "{}", e.0);
    }

    #[test]
    fn the_connection_is_quoted_and_the_password_kept_out_of_messages() {
        let mut params = super::super::Params::new();
        for (k, v) in [("database", "gis"), ("host", "db.vgr.se"), ("user", "magnus"), ("password", "hem'ligt")] {
            params.insert(k.into(), serde_json::Value::String(v.into()));
        }
        let p = P::new(&params, POSTGIS_PARAMS);
        let (conn, shown) = connection(&p).unwrap();
        assert_eq!(conn, "PG:host='db.vgr.se' port='5432' dbname='gis' user='magnus' password='hem\\'ligt' connect_timeout=10");
        assert_eq!(shown, "the database gis on db.vgr.se");
        let e = clean(GbError(format!("Cannot open {} as vector data: no answer", conn)), &conn, &shown);
        assert!(!e.0.contains("ligt") && e.0.contains("the database gis on db.vgr.se"), "{}", e.0);
        let e = clean(GbError("FATAL: password=secret failed".into()), &conn, &shown);
        assert!(!e.0.contains("secret"), "{}", e.0);
        assert_eq!(sql_name("vgr.subregions"), "vgr.subregions");
        assert_eq!(sql_name("Vård.Enheter i VGR"), "\"Vård\".\"Enheter i VGR\"");
    }
}
