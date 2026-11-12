//! Writing to PostGIS and to Microsoft SQL Server: a table in a schema, with the geometry
//! type, the geometry column and the primary key asked for.

use super::{append_to_layer, gerr, open, plan_fields, srs_from_epsg, write_features, OutField};
use gdal::vector::{FieldDefn, LayerAccess, LayerOptions, OGRwkbGeometryType};
use og_engine::geom::{self, Family};
use og_engine::io::{DbWrite, Notes};
use og_engine::model::{Feature, GbError, Result, Table};
use std::collections::HashMap;
use std::path::Path;

/// A name as PostgreSQL takes it in SQL, in double quotes.
fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// A text as PostgreSQL takes it in SQL, in single quotes.
fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A name in lower case, with spaces and dashes as underscores.
pub(crate) fn launder(s: &str) -> String {
    og_engine::io::db_name(s)
}

/// The geometry type of the table, and what each feature's geometry becomes in it.
fn plan_geometry(features: &[&Feature], asked: &str) -> Result<(OGRwkbGeometryType::Type, Vec<Option<geo::Geometry<f64>>>)> {
    use OGRwkbGeometryType::*;
    let (fam, multi, ty) = match asked {
        "point" => (Family::Point, false, wkbPoint),
        "multipoint" => (Family::Point, true, wkbMultiPoint),
        "line" => (Family::Line, false, wkbLineString),
        "multiline" => (Family::Line, true, wkbMultiLineString),
        "polygon" => (Family::Area, false, wkbPolygon),
        "multipolygon" => (Family::Area, true, wkbMultiPolygon),
        "none" => return Ok((wkbNone, features.iter().map(|_| None).collect())),
        _ => {
            // As the features are: one kind gives that type, multi if any is multi, and the
            // single ones are made multi. Mixed kinds go in a column of any geometry.
            let (ty, to_multi) = super::layer_geometry(features, false);
            let geoms = features.iter().map(|f| f.geom.as_ref().map(|g| if to_multi { super::promote(g) } else { g.clone() })).collect();
            return Ok((ty, geoms));
        }
    };
    let (mut other, mut many) = (0usize, 0usize);
    let mut geoms = Vec::with_capacity(features.len());
    for f in features {
        let Some(g) = &f.geom else {
            geoms.push(None);
            continue;
        };
        if geom::family(g) != fam {
            other += 1;
            geoms.push(None);
            continue;
        }
        let g = if multi {
            super::promote(g)
        } else {
            match g {
                geo::Geometry::MultiPoint(m) if m.0.len() == 1 => geo::Geometry::Point(m.0[0]),
                geo::Geometry::MultiLineString(m) if m.0.len() == 1 => geo::Geometry::LineString(m.0[0].clone()),
                geo::Geometry::MultiPolygon(m) if m.0.len() == 1 => geo::Geometry::Polygon(m.0[0].clone()),
                g if geom::is_multi(g) => {
                    many += 1;
                    geoms.push(None);
                    continue;
                }
                g => g.clone(),
            }
        };
        geoms.push(Some(g));
    }
    let kind = match fam {
        Family::Point => "points",
        Family::Line => "lines",
        _ => "areas",
    };
    if other > 0 {
        return Err(GbError(format!(
            "{} of the features are not {}, which the geometry type asks for. Choose another type, or As the features are.",
            other, kind
        )));
    }
    if many > 0 {
        return Err(GbError(format!("{} of the features have several parts. Choose the multi type, or split them first with Split parts.", many)));
    }
    Ok((ty, geoms))
}

pub(crate) fn write(table: &Table, conn: &str, opts: &DbWrite, notes: &mut Notes) -> Result<u64> {
    // SQL Server through GDAL's MSSQLSpatial driver, or PostgreSQL.
    let mssql = conn.starts_with("MSSQL:");
    let schema = match opts.schema.trim() {
        "" | "public" if mssql => "dbo",
        "" => "public",
        s => s,
    };
    // In lower case and without spaces, as PostgreSQL likes its names and as GDAL makes the
    // names of the columns. Then no one has to put them in quotes in SQL.
    let name = &launder(&opts.table);
    let schema = &launder(schema);
    if name.is_empty() {
        return Err(GbError("Say which table to write to.".into()));
    }
    let column = if opts.geom_column.trim().is_empty() { "geom" } else { opts.geom_column.trim() };
    let key = crate::plan_key(table, if opts.append { None } else { opts.key.as_ref() }, "ogc_fid")?;
    let fid_column = launder(&key.column);
    let table = &key.table;
    let features: Vec<&Feature> = table.features.iter().collect();
    let (ty, geoms) = plan_geometry(&features, &opts.geometry)?;
    let mut shaped = Table {
        features: table.features.iter().zip(geoms).map(|(f, g)| Feature { z: None, geom: g, attrs: f.attrs.clone() }).collect(),
        fields: table.fields.clone(),
        crs: table.crs,
        ..Default::default()
    };
    let mut seen: Vec<String> = Vec::new();
    for f in shaped.fields.iter_mut() {
        let l = launder(&f.name);
        if seen.contains(&l) {
            return Err(GbError(format!("Two attributes become {} in the database. Rename one with Manage attributes.", l)));
        }
        seen.push(l.clone());
        f.name = l;
    }
    let features: Vec<&Feature> = shaped.features.iter().collect();
    let mut ds = open(Path::new(conn), true, &[])?;
    // The schema must be there: creating one is for the people who look after the database.
    let ask = if mssql {
        format!("SELECT 1 AS found FROM sys.schemas WHERE name = {}", literal(schema))
    } else {
        format!("SELECT 1 FROM pg_namespace WHERE nspname = {}", literal(schema))
    };
    let found = ds
        .execute_sql(ask, None, gdal::vector::sql::Dialect::DEFAULT)
        .map_err(|e| gerr("Cannot ask the database for its schemas", e))?
        .is_some_and(|mut rs| rs.features().next().is_some());
    if !found {
        return Err(GbError(format!("The database has no schema {}. Choose another, or ask for it to be created.", schema)));
    }
    let full = if schema == "public" || schema == "dbo" { name.to_string() } else { format!("{}.{}", schema, name) };
    let there = ds.layer_by_name(&full).is_ok() || ds.layer_by_name(&format!("{}.{}", schema, name)).is_ok();
    let names = shaped.field_names();
    if let Some(bad) = names.iter().find(|n| n.eq_ignore_ascii_case(column) || n.eq_ignore_ascii_case(&fid_column)) {
        return Err(GbError(format!("The attribute {} clashes with a column that the table gets. Rename it with Manage attributes.", bad)));
    }
    let fields = plan_fields(&shaped, &features, names, None, &HashMap::new(), notes);
    // Rows updated by a key, in a table that is there.
    if let (Some(key_attr), true) = (&opts.update_key, there) {
        let target = Target { schema, name, full: &full, column, mssql };
        return update_rows(&mut ds, &shaped, &features, &fields, ty, &target, key_attr, opts, notes);
    }
    if opts.update_key.is_some() {
        notes.info(format!("{}.{} was not there, so it was made; the next run updates its rows", schema, name));
    }
    // In parts, each committed, or all in one transaction.
    let chunked = opts.commit_every.filter(|n| *n > 0 && features.len() > *n);
    let chunked = match (chunked, key.fids.is_some()) {
        (Some(_), true) => {
            notes.info("the key is an attribute of whole numbers, so all is written in one transaction");
            None
        }
        (c, _) => c,
    };
    let parts: Vec<&[&Feature]> = match chunked {
        Some(n) => features.chunks(n).collect(),
        None => vec![&features[..]],
    };
    let first = parts[0];
    let mut txn = ds.start_transaction().map_err(|e| gerr("Cannot start writing", e))?;
    // {table} in the SQL of the user is the table, quoted as the database wants it.
    let table_sql = if mssql { format!("[{}].[{}]", schema.replace(']', "]]"), name.replace(']', "]]")) } else { format!("{}.{}", ident(schema), ident(name)) };
    let users_sql = |sql: &Option<String>| sql.as_ref().map(|s| s.replace("{table}", &table_sql));
    if let Some(sql) = users_sql(&opts.sql_before) {
        let failed = txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).err().map(|e| gerr("The SQL before failed, and nothing was written", e));
        if let Some(e) = failed {
            let _ = txn.rollback();
            return Err(e);
        }
        notes.info("ran the SQL before");
    }
    let emptied = opts.empty_first && !opts.append && there;
    let written = if emptied {
        // The rows go and come back in one transaction: the table itself stays, with its
        // indexes, rights and the views on it.
        let sql = if mssql {
            format!("DELETE FROM [{}].[{}]", schema.replace(']', "]]"), name.replace(']', "]]"))
        } else {
            format!("DELETE FROM {}.{}", ident(schema), ident(name))
        };
        match txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT) {
            Err(e) => Err(gerr(format!("Cannot empty {}.{}", schema, name), e)),
            Ok(_) => {
                let l = txn
                    .layer_by_name(&full)
                    .or_else(|_| txn.layer_by_name(&format!("{}.{}", schema, name)))
                    .map_err(|e| gerr(format!("Cannot open the table {}", full), e))?;
                notes.info(format!("emptied {}.{} and filled it again", schema, name));
                append_to_layer(&l, &shaped, first, &fields, notes)
            }
        }
    } else if opts.append && there {
        let l = txn
            .layer_by_name(&full)
            .or_else(|_| txn.layer_by_name(&format!("{}.{}", schema, name)))
            .map_err(|e| gerr(format!("Cannot open the table {}", full), e))?;
        notes.info(format!("added to {}.{}", schema, name));
        append_to_layer(&l, &shaped, first, &fields, notes)
    } else {
        create(&mut txn, &shaped, first, &fields, ty, name, schema, column, &fid_column, key.fids.as_deref(), mssql)
    };
    // An attribute of other values than whole numbers is the key in place of the column of
    // numbers that GDAL makes.
    let written = match (written, &key.unique) {
        // SQL Server keeps the column of numbers as its key, and the attribute gets a unique
        // index: a key of other values would need the column to be made NOT NULL first.
        (Ok(()), Some(u)) if !(opts.append && there) && !emptied && mssql => {
            let t = format!("[{}].[{}]", schema.replace(']', "]]"), name.replace(']', "]]"));
            let sql = format!("CREATE UNIQUE INDEX [ux_{n}_{c}] ON {t} ([{c}])", n = name.replace(']', "]]"), c = launder(u).replace(']', "]]"), t = t);
            notes.info(format!("{} is kept unique by an index; the key of the table is {}", u, fid_column));
            txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map(|_| ()).map_err(|e| gerr(format!("Cannot make {} unique", u), e))
        }
        (Ok(()), Some(u)) if !(opts.append && there) && !emptied => {
            let t = format!("{}.{}", ident(schema), ident(name));
            let sql = format!("ALTER TABLE {t} DROP COLUMN {}; ALTER TABLE {t} ADD PRIMARY KEY ({})", ident(&fid_column), ident(&launder(u)));
            txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map(|_| ()).map_err(|e| gerr(format!("Cannot make {} the primary key", u), e))
        }
        (w, _) => w,
    };
    // The SQL after, in the same transaction when all is written in one.
    let written = match (written, users_sql(&opts.sql_after)) {
        (Ok(()), Some(sql)) if parts.len() == 1 => {
            txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map(|_| ()).map_err(|e| gerr("The SQL after failed, and nothing was written", e))
        }
        (w, _) => w,
    };
    match written {
        Ok(()) => txn.commit().map_err(|e| gerr("Cannot finish writing", e))?,
        Err(e) => {
            let _ = txn.rollback();
            return Err(e);
        }
    }
    // The other parts, each in a transaction of its own.
    let mut done = first.len();
    for part in parts.iter().skip(1) {
        let txn = ds.start_transaction().map_err(|e| gerr("Cannot start writing", e))?;
        let l = txn
            .layer_by_name(&full)
            .or_else(|_| txn.layer_by_name(&format!("{}.{}", schema, name)))
            .map_err(|e| gerr(format!("Cannot open the table {}", full), e))?;
        let mut quiet = Notes::default();
        if let Err(e) = append_to_layer(&l, &shaped, part, &fields, &mut quiet) {
            let _ = txn.rollback();
            return Err(GbError(format!("{} The first {} rows were written and committed.", e.0, done)));
        }
        txn.commit().map_err(|e| gerr(format!("Cannot finish writing; the first {} rows were committed", done), e))?;
        done += part.len();
    }
    if parts.len() > 1 {
        notes.info(format!("committed in {} parts", parts.len()));
        if let Some(sql) = users_sql(&opts.sql_after) {
            ds.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT)
                .map_err(|e| gerr(format!("All {} rows were written and committed, but the SQL after failed", done), e))?;
        }
    }
    if opts.sql_after.is_some() {
        notes.info("ran the SQL after");
    }
    // The statistics of the table, so that the planner knows the new rows.
    if opts.analyze {
        let sql = if mssql { format!("UPDATE STATISTICS {}", table_sql) } else { format!("ANALYZE {}", table_sql) };
        if let Err(e) = ds.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT) {
            notes.warn(format!("The rows were written, but the statistics of the table could not be updated: {}", e));
        }
    }
    if let (Some(og_engine::io::PrimaryKey::New(_) | og_engine::io::PrimaryKey::Attribute(_)), true) = (&opts.key, opts.append && there) {
        if opts.key != Some(og_engine::io::PrimaryKey::New("ogc_fid".into())) {
            notes.info("the primary key is set when the table is made, so the table that was there keeps its own");
        }
    }
    if there && !opts.append {
        notes.info(format!("replaced {}.{}", schema, name));
    }
    comment_on(&ds, &table_sql, mssql, opts, notes)?;
    Ok(features.len() as u64)
}

/// The comments of the table and its columns, as COMMENT ON, so that the database says what
/// they are, also to other programs. Columns by the names they got in the database.
fn comment_on(ds: &gdal::Dataset, table_sql: &str, mssql: bool, opts: &DbWrite, notes: &mut Notes) -> Result<()> {
    let table = opts.table_comment.as_deref().map(str::trim).filter(|c| !c.is_empty());
    let columns: Vec<&(String, String)> = opts.column_comments.iter().filter(|(c, t)| !c.trim().is_empty() && !t.trim().is_empty()).collect();
    if table.is_none() && columns.is_empty() {
        return Ok(());
    }
    if mssql {
        notes.warn("Comments are written to PostgreSQL; SQL Server keeps them otherwise, so they were left out.");
        return Ok(());
    }
    let mut sql = String::new();
    if let Some(c) = table {
        sql.push_str(&format!("COMMENT ON TABLE {} IS {};", table_sql, literal(c)));
    }
    for (col, text) in &columns {
        sql.push_str(&format!("COMMENT ON COLUMN {}.{} IS {};", table_sql, ident(&launder(col)), literal(text.trim())));
    }
    ds.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map_err(|e| gerr("The rows were written, but the comments could not be", e))?;
    notes.info(format!(
        "comments on {}{}",
        if table.is_some() { "the table" } else { "" },
        match columns.len() {
            0 => String::new(),
            n => format!("{}{} columns", if table.is_some() { " and " } else { "" }, n),
        }
    ));
    Ok(())
}

/// The table that is written to.
struct Target<'a> {
    schema: &'a str,
    name: &'a str,
    /// The name GDAL knows the table by.
    full: &'a str,
    /// The geometry column of the rows that are written.
    column: &'a str,
    mssql: bool,
}

impl Target<'_> {
    /// A table in the schema, quoted as the database wants it.
    fn quoted(&self, name: &str) -> String {
        if self.mssql {
            format!("[{}].[{}]", self.schema.replace(']', "]]"), name.replace(']', "]]"))
        } else {
            format!("{}.{}", ident(self.schema), ident(name))
        }
    }
    fn col(&self, name: &str) -> String {
        if self.mssql {
            format!("[{}]", name.replace(']', "]]"))
        } else {
            ident(name)
        }
    }
}

/// The first value of the first row an SQL query gives, as a whole number.
fn count_of(ds: &gdal::Dataset, sql: &str) -> Result<u64> {
    let mut rs = ds
        .execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT)
        .map_err(|e| gerr("Cannot count the rows", e))?
        .ok_or_else(|| GbError("The database gave no count.".into()))?;
    let n = rs.features().next().and_then(|f| f.field(0).ok().flatten()).and_then(|v| match v {
        gdal::vector::FieldValue::IntegerValue(i) => Some(i as u64),
        gdal::vector::FieldValue::Integer64Value(i) => Some(i as u64),
        gdal::vector::FieldValue::RealValue(r) => Some(r as u64),
        gdal::vector::FieldValue::StringValue(s) => s.trim().parse().ok(),
        _ => None,
    });
    Ok(n.unwrap_or(0))
}

/// Updates the rows of a table that is there whose key is in the stream, adds the others and,
/// when asked, deletes the rows whose key is not. The stream is written to a table of its own
/// first, and all of it happens in one transaction: if anything fails, the table is as it was.
#[allow(clippy::too_many_arguments)]
fn update_rows(
    ds: &mut gdal::Dataset,
    shaped: &Table,
    features: &[&Feature],
    fields: &[OutField],
    ty: OGRwkbGeometryType::Type,
    target: &Target,
    key_attr: &str,
    opts: &DbWrite,
    notes: &mut Notes,
) -> Result<u64> {
    let key = launder(key_attr);
    let ki = shaped.field_index(&key).ok_or_else(|| GbError(format!("There is no attribute {} to update the rows by.", key_attr)))?;
    // Each row of the stream is one row of the table: no key twice, and none empty.
    let mut seen = std::collections::HashSet::new();
    for f in features {
        let v = &f.attrs[ki];
        if v.is_null() || v.to_text().trim().is_empty() {
            return Err(GbError(format!("A feature has no {}, so it cannot be told which row it is. Nothing was written.", key_attr)));
        }
        if !seen.insert(v.to_text()) {
            return Err(GbError(format!(
                "{} is {} in two features, so it cannot be told which one the row should be. Nothing was written.",
                key_attr,
                v.to_text()
            )));
        }
    }
    // The columns of the table, and its geometry column.
    let (columns, geom_col) = {
        let l = ds
            .layer_by_name(target.full)
            .or_else(|_| ds.layer_by_name(&format!("{}.{}", target.schema, target.name)))
            .map_err(|e| gerr(format!("Cannot open the table {}", target.full), e))?;
        let defn = l.defn();
        let cols: Vec<String> = defn.fields().map(|f| f.name().to_lowercase()).collect();
        let geom = defn.geom_fields().next().map(|g| g.name()).filter(|g| !g.is_empty());
        (cols, geom)
    };
    if !columns.contains(&key.to_lowercase()) {
        return Err(GbError(format!("The table {}.{} has no column {} to update the rows by.", target.schema, target.name, key)));
    }
    let set: Vec<String> = fields.iter().map(|f| f.name.clone()).filter(|n| *n != key && columns.contains(&n.to_lowercase())).collect();
    let left: Vec<String> = fields.iter().map(|f| f.name.clone()).filter(|n| *n != key && !columns.contains(&n.to_lowercase())).collect();
    if !left.is_empty() {
        notes.warn(format!("The table has no column for {}, so it is not written. Add the columns, or replace the table.", left.join(", ")));
    }
    let with_geom = ty != OGRwkbGeometryType::wkbNone && geom_col.is_some();
    let stage = launder(&format!("og_update_{}", target.name)).chars().take(60).collect::<String>();
    let (t, st) = (target.quoted(target.name), target.quoted(&stage));
    let kc = target.col(&key);
    let mut txn = ds.start_transaction().map_err(|e| gerr("Cannot start writing", e))?;
    let users_sql = |sql: &Option<String>| sql.as_ref().map(|s| s.replace("{table}", &t));
    let run = |txn: &mut gdal::Dataset, sql: String, what: &str| {
        txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map(|_| ()).map_err(|e| gerr(what, e))
    };
    let outcome = (|| -> Result<(u64, u64, u64)> {
        if let Some(sql) = users_sql(&opts.sql_before) {
            run(&mut txn, sql, "The SQL before failed, and nothing was written")?;
            notes.info("ran the SQL before");
        }
        create(&mut txn, shaped, features, fields, ty, &stage, target.schema, target.column, "og_row", None, target.mssql)?;
        let matched = format!("EXISTS (SELECT 1 FROM {t} t WHERE t.{kc} = s.{kc})");
        let updated = count_of(&txn, &format!("SELECT COUNT(*) FROM {st} s WHERE {matched}"))?;
        let deleted = if opts.delete_missing {
            count_of(&txn, &format!("SELECT COUNT(*) FROM {t} t WHERE NOT EXISTS (SELECT 1 FROM {st} s WHERE s.{kc} = t.{kc})"))?
        } else {
            0
        };
        let mut pairs: Vec<(String, String)> = set.iter().map(|c| (target.col(c), format!("s.{}", target.col(c)))).collect();
        if with_geom {
            pairs.push((target.col(geom_col.as_deref().unwrap_or(target.column)), format!("s.{}", target.col(target.column))));
        }
        if !pairs.is_empty() {
            let sql = if target.mssql {
                let assign: Vec<String> = pairs.iter().map(|(c, v)| format!("t.{} = {}", c, v)).collect();
                format!("UPDATE t SET {} FROM {t} t JOIN {st} s ON t.{kc} = s.{kc}", assign.join(", "))
            } else {
                let assign: Vec<String> = pairs.iter().map(|(c, v)| format!("{} = {}", c, v)).collect();
                format!("UPDATE {t} AS t SET {} FROM {st} AS s WHERE t.{kc} = s.{kc}", assign.join(", "))
            };
            run(&mut txn, sql, "Cannot update the rows")?;
        }
        let mut cols: Vec<String> = vec![kc.clone()];
        let mut vals: Vec<String> = vec![format!("s.{}", kc)];
        for (c, v) in &pairs {
            cols.push(c.clone());
            vals.push(v.clone());
        }
        let sql = format!("INSERT INTO {t} ({}) SELECT {} FROM {st} s WHERE NOT {matched}", cols.join(", "), vals.join(", "));
        run(&mut txn, sql, "Cannot add the new rows")?;
        if opts.delete_missing {
            let sql = if target.mssql {
                format!("DELETE t FROM {t} t WHERE NOT EXISTS (SELECT 1 FROM {st} s WHERE s.{kc} = t.{kc})")
            } else {
                format!("DELETE FROM {t} AS t WHERE NOT EXISTS (SELECT 1 FROM {st} s WHERE s.{kc} = t.{kc})")
            };
            run(&mut txn, sql, "Cannot delete the rows that are not in the stream")?;
        }
        run(&mut txn, format!("DROP TABLE {st}"), "Cannot remove the table the rows came through")?;
        if let Some(sql) = users_sql(&opts.sql_after) {
            run(&mut txn, sql, "The SQL after failed, and nothing was written")?;
            notes.info("ran the SQL after");
        }
        Ok((updated, features.len() as u64 - updated, deleted))
    })();
    let (updated, added, deleted) = match outcome {
        Ok(n) => {
            txn.commit().map_err(|e| gerr("Cannot finish writing", e))?;
            n
        }
        Err(e) => {
            let _ = txn.rollback();
            return Err(e);
        }
    };
    let mut said =
        format!("updated {} rows and added {} in {}.{}", og_engine::model::fmt_count(updated), og_engine::model::fmt_count(added), target.schema, target.name);
    if opts.delete_missing {
        said.push_str(&format!(", and deleted {} that were not in the stream", og_engine::model::fmt_count(deleted)));
    }
    notes.info(said);
    if opts.analyze {
        let sql = if target.mssql { format!("UPDATE STATISTICS {}", t) } else { format!("ANALYZE {}", t) };
        if let Err(e) = ds.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT) {
            notes.warn(format!("The rows were written, but the statistics of the table could not be updated: {}", e));
        }
    }
    comment_on(ds, &t, target.mssql, opts, notes)?;
    Ok(features.len() as u64)
}

#[allow(clippy::too_many_arguments)]
fn create(
    ds: &mut gdal::Dataset,
    table: &Table,
    features: &[&Feature],
    fields: &[OutField],
    ty: OGRwkbGeometryType::Type,
    name: &str,
    schema: &str,
    column: &str,
    fid_column: &str,
    fids: Option<&[i64]>,
    mssql: bool,
) -> Result<()> {
    let srs = match table.crs {
        Some(c) if ty != OGRwkbGeometryType::wkbNone => Some(srs_from_epsg(c)?),
        _ => None,
    };
    let options = if mssql {
        vec![
            "OVERWRITE=YES".to_string(),
            format!("SCHEMA={}", schema),
            format!("GEOM_NAME={}", column),
            "GEOM_TYPE=geometry".to_string(),
            "SPATIAL_INDEX=YES".to_string(),
            format!("FID={}", fid_column),
        ]
    } else {
        vec![
            "OVERWRITE=YES".to_string(),
            format!("SCHEMA={}", schema),
            format!("GEOMETRY_NAME={}", column),
            "SPATIAL_INDEX=GIST".to_string(),
            format!("FID={}", fid_column),
        ]
    };
    let refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
    let layer = ds
        .create_layer(LayerOptions { name, srs: srs.as_ref(), ty, options: Some(&refs) })
        .map_err(|e| gerr(format!("Cannot create the table {}.{}", ident(schema), ident(name)), e))?;
    for of in fields {
        let d = FieldDefn::new(&of.name, of.ty).map_err(|e| gerr("Cannot create a column", e))?;
        if let Some(w) = of.width {
            d.set_width(w);
        }
        if let Some(p) = of.precision {
            d.set_precision(p);
        }
        d.add_to_layer(&layer).map_err(|e| gerr(format!("Cannot create the column {}", of.name), e))?;
    }
    write_features(&layer, table, features, fields, false, fids)
}
