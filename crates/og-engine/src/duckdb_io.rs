//! DuckDB databases (.duckdb, .ddb), with DuckDB built in: the tables are read and written
//! with their geometry as GEOMETRY, which DuckDB keeps without any extension, and a query
//! can stand in for a table, also one that reads Parquet, CSV or JSON files with DuckDB's
//! own functions. The coordinate system is kept as the comment of the geometry column.
//! Added in 0.96.5.

use crate::io::{LayerInfo, Notes};
use crate::model::{Feature, Field, FieldType, GbError, Result, Table, Value};
use crate::parquet_io::{from_wkb, to_wkb};
use duckdb::types::ValueRef;
use duckdb::{AccessMode, Config, Connection};
use std::path::Path;

fn named(path: &Path) -> impl Fn(duckdb::Error) -> GbError + '_ {
    move |e| GbError(format!("{}: {}", path.display(), e))
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn open_read(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(GbError(format!("There is no DuckDB database {}.", path.display())));
    }
    let config = Config::default().access_mode(AccessMode::ReadOnly).map_err(named(path))?;
    let conn = Connection::open_with_flags(path, config).map_err(named(path))?;
    offline(&conn, path)?;
    Ok(conn)
}

/// DuckDB fetches no extensions from the internet by itself: what it needs is built in.
fn offline(conn: &Connection, path: &Path) -> Result<()> {
    conn.execute_batch("SET autoinstall_known_extensions = false;").map_err(named(path))
}

/// The coordinate system written in a comment or a type, such as EPSG:3006.
fn epsg_in(text: &str) -> Option<u32> {
    let u = text.to_uppercase();
    if u.contains("CRS84") {
        return Some(4326);
    }
    let at = u.find("EPSG:")? + 5;
    u[at..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
}

/// The columns of a query: their names and types.
fn columns(conn: &Connection, sql: &str, path: &Path) -> Result<Vec<(String, String)>> {
    let mut st = conn.prepare(&format!("DESCRIBE {}", sql)).map_err(named(path))?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(named(path))?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(named(path))
}

/// Whether a column holds geometry: of the type GEOMETRY, or bytes by a name that geometry has.
fn is_geometry(name: &str, ty: &str) -> bool {
    let t = ty.to_uppercase();
    t.starts_with("GEOMETRY") || (t == "BLOB" && matches!(name.to_lowercase().as_str(), "geom" | "geometry" | "wkb" | "wkb_geometry" | "geom_wkb" | "the_geom"))
}

fn field_type(ty: &str) -> FieldType {
    let t = ty.to_uppercase();
    if matches!(t.as_str(), "TINYINT" | "SMALLINT" | "INTEGER" | "BIGINT" | "UTINYINT" | "USMALLINT" | "UINTEGER" | "UBIGINT" | "BOOLEAN" | "HUGEINT") {
        FieldType::Int
    } else if t == "FLOAT" || t == "DOUBLE" || t.starts_with("DECIMAL") {
        FieldType::Real
    } else {
        FieldType::Text
    }
}

/// The tables and views of a database, as layers, with their geometry and coordinate systems.
pub fn tables(path: &Path) -> Result<Vec<LayerInfo>> {
    let conn = open_read(path)?;
    let mut st = conn.prepare("SELECT table_name FROM information_schema.tables WHERE table_schema = 'main' ORDER BY table_name").map_err(named(path))?;
    let names: Vec<String> = st.query_map([], |r| r.get(0)).map_err(named(path))?.collect::<std::result::Result<_, _>>().map_err(named(path))?;
    let mut out = Vec::new();
    for name in names {
        let cols = columns(&conn, &quote(&name), path)?;
        let count: Option<i64> = conn.query_row(&format!("SELECT count(*) FROM {}", quote(&name)), [], |r| r.get(0)).ok();
        let geom = cols.iter().find(|(n, t)| is_geometry(n, t));
        let crs = geom.and_then(|(n, t)| epsg_in(t).or_else(|| comment_of(&conn, &name, n)));
        out.push(LayerInfo {
            name: name.clone(),
            count: count.map(|c| c as u64),
            crs,
            crs_name: None,
            geometry: if geom.is_some() { "Geometry".into() } else { "None".into() },
            fields: cols.iter().filter(|(n, t)| !is_geometry(n, t)).map(|(n, t)| Field::new(n.clone(), field_type(t))).collect(),
        });
    }
    Ok(out)
}

/// The comment of a column, where Ographis keeps the coordinate system.
fn comment_of(conn: &Connection, table: &str, column: &str) -> Option<u32> {
    let c: Option<String> =
        conn.query_row("SELECT comment FROM duckdb_columns() WHERE table_name = ? AND column_name = ?", [table, column], |r| r.get(0)).ok().flatten();
    c.as_deref().and_then(epsg_in)
}

/// Reads a table of a database, or what a query gives: the first table when none is named.
pub fn read(path: &Path, table: Option<&str>, sql: Option<&str>, crs_given: Option<u32>, limit: Option<usize>, notes: &mut Notes) -> Result<Table> {
    let conn = open_read(path)?;
    let (source, table_name) = match (sql.filter(|s| !s.trim().is_empty()), table) {
        (Some(q), _) => (format!("({})", q.trim().trim_end_matches(';')), None),
        (None, Some(t)) => (quote(t), Some(t.to_string())),
        (None, None) => {
            let first: String = conn
                .query_row("SELECT table_name FROM information_schema.tables WHERE table_schema = 'main' ORDER BY table_name LIMIT 1", [], |r| r.get(0))
                .map_err(|_| GbError(format!("{} has no tables.", path.display())))?;
            (quote(&first), Some(first))
        }
    };
    let cols = columns(&conn, &format!("SELECT * FROM {}", source), path)?;
    let geom = cols.iter().position(|(n, t)| is_geometry(n, t));
    if cols.iter().filter(|(n, t)| is_geometry(n, t)).count() > 1 {
        notes.info(format!("the geometry is taken from {}, the first of its geometry columns", cols[geom.expect("there is one")].0));
    }
    let crs = crs_given.or_else(|| {
        let (n, t) = &cols[geom?];
        epsg_in(t).or_else(|| table_name.as_deref().and_then(|tn| comment_of(&conn, tn, n)))
    });
    // Each value as one that Ographis keeps: whole numbers, decimals, text and WKB.
    let select: Vec<String> = cols
        .iter()
        .enumerate()
        .map(|(i, (n, t))| {
            let q = quote(n);
            if Some(i) == geom {
                q
            } else {
                match field_type(t) {
                    FieldType::Int => format!("CAST({} AS BIGINT)", q),
                    FieldType::Real => format!("CAST({} AS DOUBLE)", q),
                    FieldType::Text => format!("CAST({} AS VARCHAR)", q),
                }
            }
        })
        .collect();
    let mut sql = format!("SELECT {} FROM {}", select.join(", "), source);
    if let Some(l) = limit {
        sql.push_str(&format!(" LIMIT {}", l));
    }
    let mut t = Table { crs, ..Default::default() };
    let attr: Vec<usize> = (0..cols.len()).filter(|i| Some(*i) != geom).collect();
    t.fields = attr.iter().map(|i| Field::new(cols[*i].0.clone(), field_type(&cols[*i].1))).collect();
    let mut st = conn.prepare(&sql).map_err(named(path))?;
    let mut rows = st.query([]).map_err(named(path))?;
    let mut unread = 0usize;
    while let Some(r) = rows.next().map_err(named(path))? {
        let mut attrs = Vec::with_capacity(attr.len());
        for i in &attr {
            attrs.push(match r.get_ref(*i).map_err(named(path))? {
                ValueRef::BigInt(v) => Value::Int(v),
                ValueRef::Double(v) => Value::Real(v),
                ValueRef::Text(b) => Value::Text(String::from_utf8_lossy(b).into_owned()),
                _ => Value::Null,
            });
        }
        let mut f = Feature::new(attrs, None);
        if let Some(g) = geom {
            match r.get_ref(g).map_err(named(path))? {
                ValueRef::Geometry(b) | ValueRef::Blob(b) => match from_wkb(b) {
                    Ok((geom, z)) => {
                        f.geom = Some(geom);
                        f.z = z.map(Into::into);
                    }
                    Err(_) => unread += 1,
                },
                _ => {}
            }
        }
        t.features.push(f);
    }
    if unread > 0 {
        notes.warn(format!("{} geometries could not be read, and the features have none.", unread));
    }
    Ok(t)
}

/// Writes a table into a database, made if it is not there: the table written over, or
/// added to. The geometry as GEOMETRY in a column named geom, with the coordinate system as
/// its comment. Returns the number of rows.
pub fn write(t: &Table, path: &Path, name: &str, append: bool) -> Result<u64> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let conn = Connection::open(path).map_err(named(path))?;
    offline(&conn, path)?;
    let has_geom = t.has_geometry();
    let geom_name = if t.field_index("geom").is_some() { "geom_ographis" } else { "geom" };
    let exists: bool = conn
        .query_row("SELECT count(*) > 0 FROM information_schema.tables WHERE table_schema = 'main' AND table_name = ?", [name], |r| r.get(0))
        .map_err(named(path))?;
    if exists && !append {
        conn.execute_batch(&format!("DROP TABLE {}", quote(name))).map_err(named(path))?;
    }
    if !exists || !append {
        let mut cols: Vec<String> = t
            .fields
            .iter()
            .map(|f| {
                let ty = match f.ty {
                    FieldType::Int => "BIGINT",
                    FieldType::Real => "DOUBLE",
                    FieldType::Text => "VARCHAR",
                };
                format!("{} {}", quote(&f.name), ty)
            })
            .collect();
        if has_geom {
            cols.push(format!("{} GEOMETRY", quote(geom_name)));
        }
        conn.execute_batch(&format!("CREATE TABLE {} ({})", quote(name), cols.join(", "))).map_err(named(path))?;
        if let (true, Some(c)) = (has_geom, t.crs) {
            conn.execute_batch(&format!("COMMENT ON COLUMN {}.{} IS 'EPSG:{}'", quote(name), quote(geom_name), c)).map_err(named(path))?;
        }
    }
    let mut app = conn.appender(name).map_err(named(path))?;
    for f in &t.features {
        let mut row: Vec<duckdb::types::Value> = t
            .fields
            .iter()
            .zip(&f.attrs)
            .map(|(field, v)| match (v, field.ty) {
                (Value::Null, _) => duckdb::types::Value::Null,
                (v, FieldType::Int) => v.as_f64().map_or(duckdb::types::Value::Null, |x| duckdb::types::Value::BigInt(x as i64)),
                (v, FieldType::Real) => v.as_f64().map_or(duckdb::types::Value::Null, duckdb::types::Value::Double),
                (v, FieldType::Text) => duckdb::types::Value::Text(v.to_text()),
            })
            .collect();
        if has_geom {
            row.push(match &f.geom {
                Some(g) => duckdb::types::Value::Geometry(to_wkb(g, f.z.as_deref())),
                None => duckdb::types::Value::Null,
            });
        }
        app.append_row(duckdb::appender_params_from_iter(row)).map_err(named(path))?;
    }
    app.flush().map_err(named(path))?;
    Ok(t.features.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{point, Geometry};

    #[test]
    fn a_table_goes_through_duckdb_and_back() {
        let dir = std::env::temp_dir().join(format!("og-duckdb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("register.duckdb");
        let mut t = Table {
            fields: vec![Field::new("id", FieldType::Int), Field::new("name", FieldType::Text), Field::new("beds", FieldType::Real)],
            crs: Some(3006),
            ..Default::default()
        };
        t.features.push(Feature::new(
            vec![Value::Int(1), Value::Text("Sahlgrenska".into()), Value::Real(1700.5)],
            Some(Geometry::Point(point!(x: 319500.0, y: 6399600.0))),
        ));
        t.features.push(Feature::new(vec![Value::Int(2), Value::Null, Value::Null], None));
        assert_eq!(write(&t, &path, "hospitals", false).unwrap(), 2);
        // Written over, then added to.
        assert_eq!(write(&t, &path, "hospitals", false).unwrap(), 2);
        assert_eq!(write(&t, &path, "more", false).unwrap(), 2);
        write(&t, &path, "more", true).unwrap();

        let layers = tables(&path).unwrap();
        assert_eq!(
            layers.iter().map(|l| (l.name.as_str(), l.count, l.crs)).collect::<Vec<_>>(),
            [("hospitals", Some(2), Some(3006)), ("more", Some(4), Some(3006))]
        );
        let mut notes = Notes::default();
        let back = read(&path, Some("hospitals"), None, None, None, &mut notes).unwrap();
        assert_eq!(back.crs, Some(3006));
        assert_eq!(back.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["id", "name", "beds"]);
        for (a, b) in t.features.iter().zip(&back.features) {
            assert_eq!(a.attrs, b.attrs);
            assert_eq!(a.geom, b.geom);
        }
        // A query in place of a table, also with DuckDB's own functions.
        let q = read(&path, None, Some("SELECT name, beds * 2 AS twice, geom FROM hospitals WHERE id = 1"), Some(3006), None, &mut notes).unwrap();
        assert_eq!(q.features.len(), 1);
        assert_eq!(q.features[0].attrs, [Value::Text("Sahlgrenska".into()), Value::Real(3401.0)]);
        assert!(q.features[0].geom.is_some());
        assert_eq!(read(&path, None, None, None, Some(1), &mut notes).unwrap().features.len(), 1, "the first table, with a limit");
        // Parquet is read by DuckDB itself, built in.
        let pq = dir.join("hospitals.parquet");
        crate::parquet_io::write_parquet(&t, &pq, None).unwrap();
        let sql = format!("SELECT name FROM read_parquet('{}') WHERE id = 1", pq.display().to_string().replace('\\', "/"));
        let q = read(&path, None, Some(&sql), None, None, &mut notes).unwrap();
        assert_eq!(q.features[0].attrs, [Value::Text("Sahlgrenska".into())]);
    }
}
