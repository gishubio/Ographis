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
    s.trim().to_lowercase().chars().map(|c| if matches!(c, ' ' | '-' | '#' | '.') { '_' } else { c }).collect()
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
    let mut shaped =
        Table { features: table.features.iter().zip(geoms).map(|(f, g)| Feature { z: None, geom: g, attrs: f.attrs.clone() }).collect(), ..table.clone() };
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
    let mut txn = ds.start_transaction().map_err(|e| gerr("Cannot start writing", e))?;
    let written = if opts.append && there {
        let l = txn
            .layer_by_name(&full)
            .or_else(|_| txn.layer_by_name(&format!("{}.{}", schema, name)))
            .map_err(|e| gerr(format!("Cannot open the table {}", full), e))?;
        notes.info(format!("added to {}.{}", schema, name));
        append_to_layer(&l, &shaped, &features, &fields, notes)
    } else {
        create(&mut txn, &shaped, &features, &fields, ty, name, schema, column, &fid_column, key.fids.as_deref(), mssql)
    };
    // An attribute of other values than whole numbers is the key in place of the column of
    // numbers that GDAL makes.
    let written = match (written, &key.unique) {
        // SQL Server keeps the column of numbers as its key, and the attribute gets a unique
        // index: a key of other values would need the column to be made NOT NULL first.
        (Ok(()), Some(u)) if !(opts.append && there) && mssql => {
            let t = format!("[{}].[{}]", schema.replace(']', "]]"), name.replace(']', "]]"));
            let sql = format!("CREATE UNIQUE INDEX [ux_{n}_{c}] ON {t} ([{c}])", n = name.replace(']', "]]"), c = launder(u).replace(']', "]]"), t = t);
            notes.info(format!("{} is kept unique by an index; the key of the table is {}", u, fid_column));
            txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map(|_| ()).map_err(|e| gerr(format!("Cannot make {} unique", u), e))
        }
        (Ok(()), Some(u)) if !(opts.append && there) => {
            let t = format!("{}.{}", ident(schema), ident(name));
            let sql = format!("ALTER TABLE {t} DROP COLUMN {}; ALTER TABLE {t} ADD PRIMARY KEY ({})", ident(&fid_column), ident(&launder(u)));
            txn.execute_sql(sql, None, gdal::vector::sql::Dialect::DEFAULT).map(|_| ()).map_err(|e| gerr(format!("Cannot make {} the primary key", u), e))
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
    if let (Some(og_engine::io::PrimaryKey::New(_) | og_engine::io::PrimaryKey::Attribute(_)), true) = (&opts.key, opts.append && there) {
        if opts.key != Some(og_engine::io::PrimaryKey::New("ogc_fid".into())) {
            notes.info("the primary key is set when the table is made, so the table that was there keeps its own");
        }
    }
    if there && !opts.append {
        notes.info(format!("replaced {}.{}", schema, name));
    }
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
