//! Tools that measure or look at geometry: Calculate area and length, Filter by geometry type,
//! Check geometry and Extract coordinates.

use super::*;
use crate::geom;
use crate::model::{crs_label, err, fmt_count, Feature, Field, FieldType, Result, Table, Value};
use geo::Geometry;

// ---------- Calculate area (AreaCalculator) and Calculate length (LengthCalculator) ----------

pub(super) const AREA_UNITS: &[(&str, &str)] = &[("m2", "m²"), ("ha", "hectares"), ("km2", "km²")];
pub(super) const LENGTH_UNITS: &[(&str, &str)] = &[("m", "m"), ("km", "km")];

pub(super) const AREA_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "_area", hint: "" },
    ParamSpec { key: "unit", label: "Unit", kind: ParamKind::Choice(AREA_UNITS), default: "m2", hint: "" },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "", hint: "Rounds to this many decimals. Empty keeps them all" },
];
pub(super) const LENGTH_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "name", label: "Attribute", kind: ParamKind::Text, default: "_length", hint: "" },
    ParamSpec { key: "unit", label: "Unit", kind: ParamKind::Choice(LENGTH_UNITS), default: "m", hint: "" },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "", hint: "Rounds to this many decimals. Empty keeps them all" },
];

pub(super) fn measure(p: &P, inp: &Inputs, ctx: &mut NodeCtx, area: bool) -> Result<Outputs> {
    let table = input(inp, "Input");
    let name = p.get("name").unwrap_or_else(|| if area { "_area" } else { "_length" }.to_string());
    let unit = p.get("unit").unwrap_or_default();
    let div = match (area, unit.as_str()) {
        (true, "m2") | (false, "m") => 1.0,
        (true, "ha") => 1e4,
        (true, "km2") => 1e6,
        (false, "km") => 1e3,
        _ => return err(format!("Unknown unit: {}.", unit)),
    };
    // The value is rounded to a number of decimals, if one is given.
    let scale = match p.get("decimals") {
        None => None,
        Some(text) => match decimals(&text) {
            Some(d) => Some(10f64.powi(d as i32)),
            None => return err(format!("The number of decimals must be a whole number from 0 to 15, not \"{}\". Leave it empty to keep them all.", text)),
        },
    };
    let rounded = |v: f64| scale.map_or(v, |k| (v * k).round() / k);
    let geographic = match ctx.geographic(table.crs) {
        Some(g) => g,
        None if !table.has_geometry() => false,
        None => return err(format!("It is not known whether {} is in metres or degrees.", crs_label(table.crs))),
    };
    if geographic && table.has_geometry() {
        ctx.notes.info("calculated geodesically on the ellipsoid because the coordinates are in degrees");
    }
    let mut wrong = 0usize;
    let values: Vec<Value> = table
        .features
        .iter()
        .map(|f| match &f.geom {
            None => Value::Null,
            Some(g) => {
                let fam = geom::family(g);
                if (area && fam != geom::Family::Area) || (!area && !matches!(fam, geom::Family::Line | geom::Family::Area)) {
                    wrong += 1;
                }
                Value::Real(rounded(if area { geom::area(g, geographic) } else { geom::length(g, geographic) } / div))
            }
        })
        .collect();
    if wrong > 0 {
        ctx.notes.warn(format!("{} features are not {} and got the value 0.", fmt_count(wrong as u64), if area { "areas" } else { "lines or areas" }));
    }
    let mut out = with_column(table, &name, values);
    // The column is a decimal number even when the stream is empty or holds only nulls.
    if let Some(i) = out.field_index(&name) {
        out.fields[i].ty = FieldType::Real;
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) fn run_area(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    measure(p, inp, ctx, true)
}
pub(super) fn run_length(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    measure(p, inp, ctx, false)
}

// ---------- Filter by geometry type (GeometryFilter) ----------

pub(super) fn run_geometry_filter(_p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let (mut point, mut line, mut area, mut raster, mut other) = (table.like(), table.like(), table.like(), table.like(), table.like());
    for f in &table.features {
        // The cells of a raster go together, whatever shape they were read as.
        if table.raster {
            raster.features.push(f.clone());
            continue;
        }
        let target = match f.geom.as_ref().map(geom::family) {
            Some(Family::Point) => &mut point,
            Some(Family::Line) => &mut line,
            Some(Family::Area) => &mut area,
            Some(Family::Other) | None => &mut other,
        };
        target.features.push(f.clone());
    }
    Ok(vec![("Point".into(), point), ("Line".into(), line), ("Area".into(), area), ("Raster".into(), raster), ("Other".into(), other)])
}

pub(super) fn run_dimension_filter(_p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let (mut three, mut two, mut none) = (table.like(), table.like(), table.like());
    for f in &table.features {
        // The objects of a 3D model are 3D, as is all that has a height for each coordinate.
        let target = if f.geom.is_none() && table.mesh.is_none() {
            &mut none
        } else if table.mesh.is_some() || f.heights().is_some() {
            &mut three
        } else {
            &mut two
        };
        target.features.push(f.clone());
    }
    if !none.features.is_empty() {
        ctx.notes.info(format!("{} features without geometry went to No geometry", fmt_count(none.features.len() as u64)));
    }
    Ok(vec![("3D".into(), three), ("2D".into(), two), ("No geometry".into(), none)])
}

/// A new attribute at the end, or the one that is there, which is written over.
pub(super) fn column(t: &mut Table, name: &str, ty: FieldType) -> usize {
    match t.field_index(name) {
        Some(i) => {
            t.fields[i].ty = ty;
            i
        }
        None => {
            t.fields.push(Field::new(name, ty));
            for f in t.features.iter_mut() {
                f.attrs.push(Value::Null);
            }
            t.fields.len() - 1
        }
    }
}

// ---------- Check geometry ----------

pub(super) const GEOMETRY_CHECK_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "repair",
        label: "Repair what is wrong",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "Repaired features go to Repaired, as GDAL makes them valid. Those that cannot be repaired go to Invalid",
    },
    ParamSpec { key: "problem", label: "Problem attribute", kind: ParamKind::Text, default: "problem", hint: "What was wrong, on Invalid and Repaired" },
];

/// What is wrong with a geometry, or None when it is valid.
pub fn geometry_problem(g: &Geometry<f64>) -> Option<String> {
    use geo::algorithm::Validation;
    let coords_ok = geo::CoordsIter::coords_iter(g).all(|c| c.x.is_finite() && c.y.is_finite());
    if !coords_ok {
        return Some("coordinates that are not numbers".into());
    }
    let errors = match g {
        Geometry::Point(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect::<Vec<_>>(),
        Geometry::MultiPoint(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::LineString(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::MultiLineString(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::Polygon(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::MultiPolygon(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::GeometryCollection(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::Line(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::Rect(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
        Geometry::Triangle(x) => x.validation_errors().into_iter().map(|e| e.to_string()).collect(),
    };
    if errors.is_empty() {
        None
    } else {
        let mut e = errors;
        e.dedup();
        Some(e.join("; "))
    }
}

pub(super) fn run_geometry_check(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let name = p.get("problem").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "problem".into());
    let repair = p.get("repair").as_deref() != Some("false");
    let mut valid = table.like();
    let mut invalid = table.like();
    let at = column(&mut invalid, &name, FieldType::Text);
    let mut repaired = invalid.like();
    let mut broken: Vec<(Feature, String)> = Vec::new();
    for f in &table.features {
        let problem = match &f.geom {
            None => Some("no geometry".to_string()),
            Some(g) => geometry_problem(g),
        };
        match problem {
            None => valid.features.push(f.clone()),
            Some(why) => broken.push((f.clone(), why)),
        }
    }
    let fixable: Vec<Geometry<f64>> = broken.iter().filter_map(|(f, _)| f.geom.clone()).collect();
    let fixed: Vec<Option<Geometry<f64>>> = if repair && !fixable.is_empty() {
        let b = ctx.backend("Repairing geometries")?;
        b.make_valid(&fixable)?
    } else {
        vec![None; fixable.len()]
    };
    let mut fixed = fixed.into_iter();
    for (f, why) in broken {
        let mut attrs = f.attrs.clone();
        attrs.resize(at, Value::Null);
        attrs.insert(at, Value::Text(why.clone()));
        attrs.truncate(invalid.fields.len());
        let mended = if f.geom.is_some() { fixed.next().flatten() } else { None };
        // A repair that turns a line into a point, or an area into a line, is no repair.
        let same_kind = |g: &Geometry<f64>| f.geom.as_ref().is_some_and(|was| geom::family(was) == geom::family(g));
        match mended.filter(|g| geometry_problem(g).is_none() && geo::CoordsIter::coords_count(g) > 0 && same_kind(g)) {
            Some(g) => repaired.features.push(Feature { z: None, attrs, geom: Some(g) }),
            None => invalid.features.push(Feature { z: f.z, attrs, geom: f.geom }),
        }
    }
    if !repaired.features.is_empty() {
        ctx.notes.info(format!("{} repaired", fmt_count(repaired.len() as u64)));
    }
    Ok(vec![("Valid".into(), valid), ("Repaired".into(), repaired), ("Invalid".into(), invalid)])
}

// ---------- Extract coordinates ----------

pub(super) const OTHER_GEOMETRIES: &[(&str, &str)] =
    &[("inside", "A point inside it"), ("centroid", "Its centre of mass"), ("first", "Its first point"), ("none", "No coordinates")];

pub(super) const COORDINATE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "x_name", label: "Attribute for x", kind: ParamKind::Text, default: "x", hint: "East, or longitude in degrees" },
    ParamSpec { key: "y_name", label: "Attribute for y", kind: ParamKind::Text, default: "y", hint: "North, or latitude in degrees" },
    ParamSpec {
        key: "decimals",
        label: "Decimals",
        kind: ParamKind::Text,
        default: "",
        hint: "Rounds the coordinates, such as 0 for whole metres or 6 for degrees. Empty keeps them as they are",
    },
    ParamSpec { key: "other", label: "For lines and areas", kind: ParamKind::Choice(OTHER_GEOMETRIES), default: "inside", hint: "" },
];

pub(super) fn run_coordinates(p: &P, inp: &Inputs, _ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::Centroid;
    let table = input(inp, "Input");
    let xn = p.get("x_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "x".into());
    let yn = p.get("y_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "y".into());
    if xn == yn {
        return err("x and y need attributes of their own, with different names.");
    }
    let decimals = number_param(p, "decimals", "The decimals")?.map(|d| d.clamp(0.0, 12.0) as i32);
    let other = p.get("other").unwrap_or_else(|| "inside".into());
    let mut out = table.clone();
    let xi = column(&mut out, &xn, FieldType::Real);
    let yi = column(&mut out, &yn, FieldType::Real);
    let round = |v: f64| match decimals {
        Some(d) => {
            let k = 10f64.powi(d);
            (v * k).round() / k
        }
        None => v,
    };
    for f in out.features.iter_mut() {
        let at: Option<geo::Coord<f64>> = match &f.geom {
            None => None,
            Some(Geometry::Point(pt)) => Some(pt.0),
            Some(Geometry::MultiPoint(m)) if m.0.len() == 1 => Some(m.0[0].0),
            Some(g) => match other.as_str() {
                "none" => None,
                "first" => geo::CoordsIter::coords_iter(g).next(),
                "centroid" => g.centroid().map(|c| c.0),
                _ => match geom::center(g, geom::Center::Inside) {
                    Some(Geometry::Point(pt)) => Some(pt.0),
                    _ => None,
                },
            },
        };
        let (x, y) = at.map_or((Value::Null, Value::Null), |c| (Value::Real(round(c.x)), Value::Real(round(c.y))));
        f.attrs[xi] = x;
        f.attrs[yi] = y;
    }
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "GeometryValidator",
        title: "Check geometry",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Finds geometries that are not valid, such as areas whose boundary crosses itself, and says what is wrong. They can be repaired: the repaired ones go to Repaired and those that cannot be go to Invalid. Features without geometry are Invalid.",
        inputs: &["Input"],
        outputs: &["Valid", "Repaired", "Invalid"],
        ports: None,
        params: GEOMETRY_CHECK_PARAMS,
        run: run_geometry_check,
        summary: |p| if p.get("repair").as_deref() == Some("false") { "find only".into() } else { "find and repair".into() },
    },
    NodeType {
        name: "CoordinateExtractor",
        title: "Extract coordinates from point",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Puts the x and y of each point in attributes of their own. Lines and areas give a point inside them, their centre or their first point.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: COORDINATE_PARAMS,
        run: run_coordinates,
        summary: |p| format!("{}, {}", p.get("x_name").unwrap_or_else(|| "x".into()), p.get("y_name").unwrap_or_else(|| "y".into())),
    },
    NodeType {
        name: "GeometryFilter",
        title: "Filter by geometry type",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Sends points, lines and areas to an output each. The cells of a raster go to Raster, and features without geometry to Other. The points of a point cloud go to Point: to take its ground or buildings out, use Filter by value or Test on classification.",
        inputs: &["Input"],
        outputs: &["Point", "Line", "Area", "Raster", "Other"],
        ports: None,
        params: &[],
        run: run_geometry_filter,
        summary: |_| String::new(),
    },
    NodeType {
        name: "DimensionFilter",
        title: "Filter 2D/3D",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Sends features with heights to 3D and flat ones to 2D: geometry with a height for each coordinate, as read with Z or given heights with Set height, and the objects of a 3D model, which keep their part of the model. Features without geometry go to No geometry.",
        inputs: &["Input"],
        outputs: &["3D", "2D", "No geometry"],
        ports: None,
        params: &[],
        run: run_dimension_filter,
        summary: |_| String::new(),
    },
    NodeType {
        name: "AreaCalculator",
        title: "Calculate area",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Writes the area to an attribute. In metre-based systems the area is planar, in degrees it is geodesic on the ellipsoid.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: AREA_PARAMS,
        run: run_area,
        summary: |p| format!("{} in {}", p.get("name").unwrap_or_default(), choice_label(AREA_UNITS, &p.get("unit").unwrap_or_default())),
    },
    NodeType {
        name: "LengthCalculator",
        title: "Calculate length",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Writes the length of a line, or the perimeter of an area, to an attribute.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: LENGTH_PARAMS,
        run: run_length,
        summary: |p| format!("{} in {}", p.get("name").unwrap_or_default(), choice_label(LENGTH_UNITS, &p.get("unit").unwrap_or_default())),
    },
];
