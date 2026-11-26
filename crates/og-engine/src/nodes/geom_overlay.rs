//! Tools where two streams of geometry meet: Clip, Dissolve, Join by location, Overlay
//! points and areas, Overlay areas, Find nearest and Split lines.

use super::*;
use crate::model::{crs_label, err, fmt_count, Feature, Field, FieldType, Result, Table, Value};
use geo::{BooleanOps, BoundingRect, Coord, Distance, Euclidean, Geometry, Intersects, LineString, MultiPolygon, Polygon};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{RTree, AABB};
use std::collections::HashMap;

// ---------- Clip (Clipper) ----------

pub(super) const CLIPPER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "each",
        label: "Clip areas",
        kind: ParamKind::Choice(&[("together", "All as one"), ("each", "Each on its own, with its attributes")]),
        default: "together",
        hint: "Each on its own: what lies in several clip areas, such as the squares of a grid, is cut into a piece for each, and each piece gets the attributes of its clip area",
    },
    ParamSpec {
        key: "attributes",
        label: "Attributes to fetch",
        kind: ParamKind::Attributes("Clipper"),
        default: "",
        hint: "From the clip area each piece lies in. Empty fetches all",
    },
    ParamSpec {
        key: "prefix",
        label: "Prefix",
        kind: ParamKind::Text,
        default: "",
        hint: "Put in front of the fetched names, for example square_. Without it, a name the features already have is not fetched",
    },
    ParamSpec {
        key: "crop",
        label: "A raster inside",
        kind: ParamKind::Choice(&[("yes", "Cut to the areas"), ("no", "As large as it was")]),
        default: "yes",
        hint: "For a raster on Clippee: a cell is inside when its middle is. The cells outside are on Outside, in a raster as large as it was",
    },
    ParamSpec {
        key: "close",
        label: "3D models along the edge",
        kind: ParamKind::Choice(&[("false", "Left open"), ("true", "Closed with a wall")]),
        default: "false",
        hint: "A building or another solid cut by the edge is closed with a new wall there, on each side, so that it stands whole in a 3D print or a picture",
    },
];

/// A raster clipped: the cells inside the areas on Inside, those outside on Outside.
fn clip_a_raster(p: &P, raster: &Table, areas: &Table, ctx: &mut NodeCtx) -> Result<Outputs> {
    if p.get("each").as_deref() == Some("each") {
        return err("A stream carries one raster, so the clip areas cannot each cut a raster of their own. Choose All as one, or clip the raster in a Loop over the areas.");
    }
    if !areas.features.iter().any(|f| f.geom.as_ref().is_some_and(|g| !geom::polygons_of(g).is_empty())) {
        return err("No areas came in on Clipper. Connect polygons to clip the raster with.");
    }
    let crop = p.get("crop").as_deref() != Some("no");
    let (inside, kept, all) = super::raster089::clip_raster(raster, areas, true, crop, ctx)?;
    let (outside, rest, _) = super::raster089::clip_raster(raster, areas, false, false, ctx)?;
    if inside.is_none() {
        ctx.notes.warn("No cell of the raster is inside the areas: they do not cover the middle of any cell.");
    }
    ctx.notes.info(format!("{} of {} cells inside, {} outside", fmt_count(kept as u64), fmt_count(all as u64), fmt_count(rest as u64)));
    Ok(vec![("Inside".into(), inside.unwrap_or_else(|| raster.like())), ("Outside".into(), outside.unwrap_or_else(|| raster.like()))])
}

/// The clip areas one by one, each with its row in Clipper, for clipping by each on its own.
fn each_area(clip_table: &Table) -> Vec<(usize, Clipper)> {
    clip_table
        .features
        .iter()
        .enumerate()
        .filter_map(|(k, f)| {
            let g = f.geom.as_ref().filter(|g| geom::family(g) == geom::Family::Area)?;
            Clipper::new(std::iter::once(g)).map(|c| (k, c))
        })
        .collect()
}

pub(super) fn run_clipper(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let clippee = input(inp, "Clippee");
    if clippee.grid.is_some() || clippee.image.is_some() {
        return clip_a_raster(p, clippee, input(inp, "Clipper"), ctx);
    }
    let mut clip_table = input(inp, "Clipper").clone();
    if let (Some(a), Some(b)) = (clippee.crs, clip_table.crs) {
        if a != b {
            ctx.notes.warn(format!("The clip areas were reprojected from {} to {} to match what is being clipped.", crs_label(Some(b)), crs_label(Some(a))));
            clip_table = reproject(&clip_table, a, ctx.backend)?;
        }
    }
    let not_area = clip_table.features.iter().filter(|f| f.geom.as_ref().is_some_and(|g| geom::family(g) != geom::Family::Area)).count();
    if not_area > 0 {
        ctx.notes.warn(format!("{} features in Clipper are not areas and were not used.", fmt_count(not_area as u64)));
    }
    let (mut inside, mut outside) = (clippee.like(), clippee.like());
    let clipper = match Clipper::new(clip_table.features.iter().filter_map(|f| f.geom.as_ref())) {
        Some(c) => c,
        None => {
            if !clippee.is_empty() {
                ctx.notes.warn("No clip areas came in, so everything goes to Outside.");
            }
            return Ok(vec![("Inside".into(), inside), ("Outside".into(), clippee.clone())]);
        }
    };
    let each = p.get("each").as_deref() == Some("each");
    if each {
        let fetched = fetched_fields(p, clippee, &clip_table, "Clipper", ctx)?;
        inside.fields.extend(fetched.iter().map(|(i, n)| Field::new(n.clone(), clip_table.fields[*i].ty)));
        let with_area = |attrs: &[Value], k: usize| -> Vec<Value> {
            let mut a = attrs.to_vec();
            a.extend(fetched.iter().map(|(i, _)| clip_table.features[k].attrs[*i].clone()));
            a
        };
        if let Some(mesh) = clippee.mesh.as_deref() {
            let areas = each_area(&clip_table);
            let close = p.flag("close");
            // What lies in none of the areas, as when they are clipped by as one.
            let (_, mut rest) = crate::clip3d::clip_mesh(mesh, &clipper);
            let mut walls = 0;
            if close {
                walls += crate::clip3d::close_cut(&mut rest, &clipper);
            }
            let outside = model_side(clippee, &rest, &clipper, false);
            let mut models = Vec::new();
            let mut pieces = 0usize;
            for (k, c) in &areas {
                if !ctx.gate.going() {
                    return err(crate::model::STOPPED);
                }
                let (mut m, _) = crate::clip3d::clip_mesh(mesh, c);
                if m.tris.is_empty() {
                    continue;
                }
                if close {
                    walls += crate::clip3d::close_cut(&mut m, c);
                }
                pieces += m.tris.len();
                let side = model_side(clippee, &m, c, true);
                inside.features.extend(side.features.iter().map(|f| Feature { attrs: with_area(&f.attrs, *k), ..f.clone() }));
                models.push(side.mesh.expect("a model on each side"));
            }
            let parts: Vec<&crate::mesh::Mesh> = models.iter().map(|m| m.as_ref()).collect();
            inside.mesh = Some(std::sync::Arc::new(crate::mesh::Mesh::merge(&parts)));
            ctx.notes.info(format!(
                "{} pieces in {} clip areas, {} triangles inside and {} outside, from {}",
                fmt_count(inside.features.len() as u64),
                fmt_count(areas.len() as u64),
                fmt_count(pieces as u64),
                fmt_count(rest.tris.len() as u64),
                fmt_count(mesh.tris.len() as u64)
            ));
            if close {
                ctx.notes.info(if walls > 0 {
                    format!("{} walls close what was cut", fmt_count(walls as u64))
                } else {
                    "nothing was cut open that could be closed".to_string()
                });
            }
            return Ok(vec![("Inside".into(), inside), ("Outside".into(), outside)]);
        }
        let (cut, rest) = cut_by_each(clippee, &clip_table, &clipper, &fetched, ctx)?;
        inside.features = cut.features;
        outside.features = rest.features;
        return Ok(vec![("Inside".into(), inside), ("Outside".into(), outside)]);
    }
    if let Some(mesh) = clippee.mesh.as_deref() {
        return clip_model(clippee, mesh, &clipper, p.flag("close"), ctx);
    }
    let mut unsupported = 0usize;
    // Each feature is clipped on a core of its own.
    let clipped = ctx.gate.each(clippee.len(), |i| Ok(clippee.features[i].geom.as_ref().map(|g| clipper.clip(g))))?;
    for (f, r) in clippee.features.iter().zip(clipped) {
        let (g, r) = match (&f.geom, r) {
            (Some(g), Some(r)) => (g, r),
            _ => {
                outside.features.push(f.clone());
                continue;
            }
        };
        if !r.supported {
            unsupported += 1;
        }
        // Heights go with the pieces: from the corners there were, along the edges and in
        // the plane of each area.
        let z_of = |part: &Geometry<f64>| -> Option<std::sync::Arc<[f64]>> {
            let z = f.heights()?;
            if part == g {
                return f.z.clone();
            }
            Some(std::sync::Arc::from(geom::lift_heights(g, z, part)))
        };
        if let Some(part) = r.inside {
            inside.features.push(Feature { z: z_of(&part), attrs: f.attrs.clone(), geom: Some(part) });
        }
        if let Some(part) = r.outside {
            outside.features.push(Feature { z: z_of(&part), attrs: f.attrs.clone(), geom: Some(part) });
        }
    }
    if unsupported > 0 {
        ctx.notes.warn(format!("{} geometry collections cannot be clipped and were sent to Outside.", unsupported));
    }
    Ok(vec![("Inside".into(), inside), ("Outside".into(), outside)])
}

/// Clips a 3D model, such as a TIN, a surface or buildings, by the clip areas from above.
/// Each object goes to the side where it has triangles, and to both when it is cut; its
/// feature is clipped with it.
fn clip_model(clippee: &Table, mesh: &crate::mesh::Mesh, clipper: &geom::Clipper, close: bool, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (mut mesh_in, mut mesh_out) = crate::clip3d::clip_mesh(mesh, clipper);
    if close {
        let walls = crate::clip3d::close_cut(&mut mesh_in, clipper) + crate::clip3d::close_cut(&mut mesh_out, clipper);
        ctx.notes.info(if walls > 0 {
            format!("{} walls close what was cut", fmt_count(walls as u64))
        } else {
            "nothing was cut open that could be closed: the models are not closed solids, or the edge does not cross them".to_string()
        });
    }
    let (inside, outside) = (model_side(clippee, &mesh_in, clipper, true), model_side(clippee, &mesh_out, clipper, false));
    ctx.notes.info(format!(
        "{} triangles inside and {} outside, from {}",
        fmt_count(mesh_in.tris.len() as u64),
        fmt_count(mesh_out.tris.len() as u64),
        fmt_count(mesh.tris.len() as u64)
    ));
    Ok(vec![("Inside".into(), inside), ("Outside".into(), outside)])
}

/// One side of a clipped 3D model: the features that have triangles in it, their outlines
/// clipped with it, and the model of those features.
fn model_side(clippee: &Table, m: &crate::mesh::Mesh, clipper: &Clipper, inside: bool) -> Table {
    let mut has = vec![false; clippee.features.len().max(m.objects.len())];
    for t in &m.tris {
        if let Some(h) = has.get_mut(t.object as usize) {
            *h = true;
        }
    }
    let rows: Vec<usize> = (0..clippee.features.len()).filter(|r| has[*r]).collect();
    let mut out = clippee.like();
    for r in &rows {
        let f = &clippee.features[*r];
        let geom_part = f.geom.as_ref().map(|g| (g, clipper.clip(g)));
        let feature = match geom_part {
            Some((g, c)) => {
                let part = if inside { c.inside } else { c.outside };
                match part {
                    Some(part) => {
                        let z = f.heights().map(|z| if &part == g { f.z.clone().unwrap() } else { std::sync::Arc::from(geom::lift_heights(g, z, &part)) });
                        Feature { attrs: f.attrs.clone(), geom: Some(part), z }
                    }
                    // The model reaches over, but its outline does not: the outline is kept.
                    None => f.clone(),
                }
            }
            None => f.clone(),
        };
        out.features.push(feature);
    }
    out.mesh = Some(std::sync::Arc::new(m.pick(&rows)));
    out
}

// ---------- Dissolve (Dissolver) ----------

pub(super) const DISSOLVER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "group_by",
        label: "Group by",
        kind: ParamKind::Attributes("Input"),
        default: "",
        hint: "Only areas with the same values are merged. Empty merges all that touch",
    },
    ParamSpec { key: "count", label: "Count attribute", kind: ParamKind::Text, default: "_count", hint: "The number of areas that were merged" },
    ParamSpec {
        key: "parts",
        label: "Areas apart",
        kind: ParamKind::Choice(&[("apart", "Each a feature of its own"), ("together", "One multi-area for each group")]),
        default: "apart",
        hint: "What is left apart after merging, such as buildings that do not touch",
    },
];

/// The values that make two features belong together. Null is a group of its own.
pub(super) fn group_key(f: &Feature, idx: &[usize]) -> Vec<Option<String>> {
    idx.iter().map(|i| f.attrs[*i].key()).collect()
}

/// Groups in order of first appearance: (key, indices of the features in the group).
pub(super) fn groups(table: &Table, idx: &[usize], keep: impl Fn(&Feature) -> bool) -> Vec<Vec<usize>> {
    let mut order: Vec<Vec<usize>> = Vec::new();
    let mut seen: HashMap<Vec<Option<String>>, usize> = HashMap::new();
    for (i, f) in table.features.iter().enumerate() {
        if !keep(f) {
            continue;
        }
        let slot = *seen.entry(group_key(f, idx)).or_insert_with(|| {
            order.push(Vec::new());
            order.len() - 1
        });
        order[slot].push(i);
    }
    order
}

pub(super) fn is_area(f: &Feature) -> bool {
    f.geom.as_ref().is_some_and(|g| geom::family(g) == Family::Area)
}

pub(super) fn run_dissolver(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let by = p.list("group_by");
    let count_name = p.get("count");
    let apart = p.get("parts").as_deref() != Some("together");
    let mut remnants = table.like();
    remnants.features = table.features.iter().filter(|f| !is_area(f)).cloned().collect();
    if table.fields.is_empty() && table.is_empty() {
        return Ok(vec![("Dissolved".into(), table.like()), ("Remnants".into(), remnants)]);
    }
    let idx: Vec<usize> = by.iter().map(|n| table.need_field(n)).collect::<Result<_>>()?;
    if let Some(c) = &count_name {
        if by.contains(c) {
            return err(format!("The count attribute \"{}\" has the same name as a group-by attribute. Rename one of them.", c));
        }
    }
    let mut out = Table {
        mesh: None,
        points: None,
        series: None,
        fields: idx.iter().map(|i| table.fields[*i].clone()).collect(),
        crs: table.crs,
        features: Vec::new(),
        raster: table.raster,
        image: None,
        grid: None,
    };
    if let Some(c) = &count_name {
        out.fields.push(Field::new(c.clone(), FieldType::Int));
    }
    // The groups are merged on the cores at once, and a large group is itself spread.
    let all = groups(table, &idx, is_area);
    let gate = ctx.gate;
    // The largest groups first, so that no core is left with a large one at the end.
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_key(|k| std::cmp::Reverse(all[*k].len()));
    let by_size: Vec<Vec<Feature>> = gate.each(order.len(), |j| {
        let members = &all[order[j]];
        let mut attrs: Vec<Value> = idx.iter().map(|i| table.features[members[0]].attrs[*i].clone()).collect();
        if count_name.is_some() {
            attrs.push(Value::Int(members.len() as i64));
        }
        let geoms = members.iter().filter_map(|i| table.features[*i].geom.as_ref());
        let r = Ok(if apart {
            // Areas that do not touch stay apart, each a feature with how many it was made of.
            geom::dissolve_parts(geoms)
                .into_iter()
                .map(|(part, n)| {
                    let mut attrs = attrs.clone();
                    if count_name.is_some() {
                        *attrs.last_mut().expect("the count") = Value::Int(n as i64);
                    }
                    Feature { z: None, attrs, geom: Some(Geometry::Polygon(part)) }
                })
                .collect()
        } else {
            match geom::dissolve(geoms) {
                Some(g) => vec![Feature { z: None, attrs, geom: Some(g) }],
                None => Vec::new(),
            }
        });
        r
    })?;
    // Back in the order the groups first came.
    let mut merged: Vec<Vec<Feature>> = vec![Vec::new(); all.len()];
    for (j, features) in by_size.into_iter().enumerate() {
        merged[order[j]] = features;
    }
    out.features = merged.into_iter().flatten().collect();
    if out.features.is_empty() && remnants.is_empty() {
        ctx.notes.warn("No areas came in, so there is nothing to dissolve.".to_string());
    } else if out.features.is_empty() {
        ctx.notes.warn(format!("None of the {} features that came in are areas, so they all went to Remnants.", fmt_count(remnants.len() as u64)));
    }
    if !remnants.is_empty() {
        ctx.notes.info(format!("{} features that are not areas went to Remnants", fmt_count(remnants.len() as u64)));
    }
    Ok(vec![("Dissolved".into(), out), ("Remnants".into(), remnants)])
}

// ---------- Overlay points on areas (PointOnAreaOverlayer) ----------

pub(super) const OVERLAYER_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "count",
    label: "Count attribute",
    kind: ParamKind::Text,
    default: "_overlaps",
    hint: "Areas found for a point, and points found in an area",
}];

pub(super) fn run_overlayer(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (points, areas) = (input(inp, "Point"), input(inp, "Area"));
    let count_name = p.get("count").unwrap_or_else(|| "_overlaps".to_string());
    // The test is done in the coordinate system of the points. The areas that come out
    // keep their own geometry and coordinate system.
    let mut test_areas = areas.clone();
    if let (Some(a), Some(b)) = (points.crs, areas.crs) {
        if a != b {
            ctx.notes.warn(format!("The areas were reprojected from {} to {} to be compared with the points.", crs_label(Some(b)), crs_label(Some(a))));
            test_areas = reproject(&test_areas, a, ctx.backend)?;
        }
    }
    let index = AreaIndex::new(test_areas.features.iter().map(|f| f.geom.as_ref()));
    let not_area = areas.features.iter().filter(|f| f.geom.is_some() && !is_area(f)).count();
    if not_area > 0 {
        ctx.notes.warn(format!("{} features in Area are not areas and match no points.", fmt_count(not_area as u64)));
    }

    // Point attributes win when names clash, so only new names are taken from the areas.
    let extra: Vec<usize> =
        (0..areas.fields.len()).filter(|i| points.field_index(&areas.fields[*i].name).is_none() && areas.fields[*i].name != count_name).collect();
    let mut out_points = points.like();
    out_points.fields.retain(|f| f.name != count_name);
    let kept: Vec<usize> = (0..points.fields.len()).filter(|i| points.fields[*i].name != count_name).collect();
    out_points.fields.extend(extra.iter().map(|i| areas.fields[*i].clone()));
    out_points.fields.push(Field::new(count_name.clone(), FieldType::Int));

    let mut hits_per_area = vec![0i64; areas.len()];
    let (mut not_point, mut several) = (0usize, 0usize);
    for f in &points.features {
        let coord = match &f.geom {
            Some(Geometry::Point(pt)) => Some(pt.0),
            Some(Geometry::MultiPoint(m)) if m.0.len() == 1 => Some(m.0[0].0),
            Some(_) => {
                not_point += 1;
                None
            }
            None => None,
        };
        let hits = coord.map(|c| index.containing(c)).unwrap_or_default();
        for h in &hits {
            hits_per_area[*h] += 1;
        }
        if hits.len() > 1 {
            several += 1;
        }
        let mut attrs: Vec<Value> = kept.iter().map(|i| f.attrs[*i].clone()).collect();
        attrs.extend(extra.iter().map(|i| hits.first().map_or(Value::Null, |h| areas.features[*h].attrs[*i].clone())));
        attrs.push(Value::Int(hits.len() as i64));
        out_points.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
    }
    if not_point > 0 {
        ctx.notes.warn(format!("{} features in Point are not single points and match no areas.", fmt_count(not_point as u64)));
    }
    if several > 0 {
        ctx.notes.warn(format!("{} points lie in more than one area. They got the attributes of the first one.", fmt_count(several as u64)));
    }
    let counts: Vec<Value> = hits_per_area.into_iter().map(Value::Int).collect();
    let mut out_areas = with_column(areas, &count_name, counts);
    if let Some(i) = out_areas.field_index(&count_name) {
        out_areas.fields[i].ty = FieldType::Int;
    }
    Ok(vec![("Point".into(), out_points), ("Area".into(), out_areas)])
}

// ---------- Join by location (SpatialJoiner), which also counts points in areas ----------

/// An index of the areas of a table, for finding the ones a point lies in. The areas
/// are reprojected to the coordinate system of what they are compared with, and those
/// that are not areas are counted in a warning.
pub(super) fn area_lookup(areas: &Table, port: &str, other: Option<u32>, ctx: &mut NodeCtx) -> Result<AreaIndex> {
    let mut test_areas = None;
    if let (Some(a), Some(b)) = (other, areas.crs) {
        if a != b {
            ctx.notes.warn(format!("{} was reprojected from {} to {} for the comparison.", port, crs_label(Some(b)), crs_label(Some(a))));
            test_areas = Some(reproject(areas, a, ctx.backend)?);
        }
    }
    let not_area = areas.features.iter().filter(|f| f.geom.is_some() && !is_area(f)).count();
    if not_area > 0 {
        ctx.notes.warn(format!("{} features in {} are not areas and hold nothing.", fmt_count(not_area as u64), port));
    }
    Ok(AreaIndex::new(test_areas.as_ref().unwrap_or(areas).features.iter().map(|f| f.geom.as_ref())))
}

/// The coordinate of a feature that is a single point.
pub(super) fn single_point(f: &Feature) -> Option<geo::Coord<f64>> {
    match &f.geom {
        Some(Geometry::Point(pt)) => Some(pt.0),
        Some(Geometry::MultiPoint(m)) if m.0.len() == 1 => Some(m.0[0].0),
        _ => None,
    }
}

pub(super) const RELATIONS: &[(&str, &str)] = &[
    ("intersects", "Touch or overlap in any way"),
    ("within", "Lie within"),
    ("contains", "Contain"),
    ("crosses", "Cross"),
    ("touches", "Only touch at the edge"),
    ("overlaps", "Overlap, of the same kind"),
    ("near", "Lie within a distance"),
    ("point", "Have their point inside (one per feature)"),
];
pub(super) const JOIN_MODES: &[(&str, &str)] = &[
    ("first", "The first one found"),
    ("all", "Every one: a feature for each"),
    ("summary", "Summarized: count, and the values together"),
    ("count", "Counted only: how many, such as the points in each area"),
];
pub(super) const JOIN_NUMBERS: &[(&str, &str)] = &[("sum", "Sum"), ("mean", "Mean"), ("min", "Smallest"), ("max", "Largest"), ("list", "Listed, like text")];

pub(super) const JOINER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "relation",
        label: "Features that",
        kind: ParamKind::Choice(RELATIONS),
        default: "intersects",
        hint: "How a feature is to lie to a feature in Join to get its attributes. Within: a feature in Features lies within one in Join. Their point inside is how Join by location worked before 0.77: a line or an area is placed by one point on it",
    },
    ParamSpec {
        key: "distance",
        label: "Distance",
        kind: ParamKind::Text,
        default: "10",
        hint: "For Lie within a distance: how near, in the units of the coordinate system",
    },
    ParamSpec {
        key: "mode",
        label: "When several are found",
        kind: ParamKind::Choice(JOIN_MODES),
        default: "first",
        hint: "Summarized and counted keep every feature in Joined, those that found nothing with the count 0",
    },
    ParamSpec {
        key: "numbers",
        label: "Summarize numbers as",
        kind: ParamKind::Choice(JOIN_NUMBERS),
        default: "sum",
        hint: "Text is always listed, each value once, separated by commas",
    },
    ParamSpec {
        key: "count",
        label: "Count attribute",
        kind: ParamKind::Text,
        default: "join_count",
        hint: "When summarized or counted: how many were found",
    },
    ParamSpec {
        key: "attributes",
        label: "Attributes to fetch",
        kind: ParamKind::Attributes("Join"),
        default: "",
        hint: "From the features in Join that are found. Empty fetches all",
    },
    ParamSpec {
        key: "prefix",
        label: "Prefix",
        kind: ParamKind::Text,
        default: "",
        hint: "Put in front of the fetched names, for example area_. Without it, a name the features already have is not fetched",
    },
];

/// Whether feature a stands in the relation to b.
fn related(relation: &str, a: &Geometry<f64>, b: &Geometry<f64>, distance: f64) -> bool {
    use geo::{Contains, Relate};
    match relation {
        "intersects" => a.intersects(b),
        "within" => b.contains(a),
        "contains" => a.contains(b),
        "near" => Euclidean.distance(a, b) <= distance,
        "crosses" => a.intersects(b) && a.relate(b).is_crosses(),
        "touches" => a.intersects(b) && a.relate(b).is_touches(),
        "overlaps" => a.intersects(b) && a.relate(b).is_overlaps(),
        _ => false,
    }
}

pub(super) fn run_spatial_joiner(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (features, join) = (input(inp, "Features"), input(inp, "Join"));
    let relation = p.get("relation").unwrap_or_else(|| "intersects".into());
    let mode = p.get("mode").unwrap_or_else(|| "first".into());
    let distance = if relation == "near" {
        match number_param(p, "distance", "The distance")? {
            Some(d) if d >= 0.0 => d,
            _ => return err("Give the distance as a number of 0 or more."),
        }
    } else {
        0.0
    };
    if relation == "near" && ctx.geographic(features.crs) == Some(true) {
        ctx.notes.warn(format!("The distance is in degrees, since Features are in {}. Reproject first to count in metres.", crs_label(features.crs)));
    }
    // Counted only: how many, and no attributes.
    let fetched = if mode == "count" { Vec::new() } else { fetched_fields(p, features, join, "Join", ctx)? };
    // For each feature, the features of Join it stands in the relation to, in order.
    let mut hits: Vec<Vec<usize>> = vec![Vec::new(); features.len()];
    if relation == "point" {
        let index = area_lookup(join, "Join", features.crs, ctx)?;
        // Each feature on a core of its own: the areas it lies in, and whether it was placed
        // by a point inside it.
        let found = ctx.gate.each(features.len(), |k| {
            let f = &features.features[k];
            // A point is where it is. Anything else is placed by a point that lies on it.
            let (at, by_inside) = match single_point(f) {
                Some(c) => (Some(c), false),
                None => match f.geom.as_ref().and_then(|g| geom::center(g, Center::Inside)) {
                    Some(Geometry::Point(pt)) => (Some(pt.0), true),
                    Some(_) => (None, true),
                    None => (None, false),
                },
            };
            Ok((at.map(|c| index.containing(c)).unwrap_or_default(), by_inside))
        })?;
        let placed = found.iter().filter(|f| f.1).count();
        hits = found.into_iter().map(|f| f.0).collect();
        if placed > 0 {
            ctx.notes.info(format!("{} features that are not points were placed by a point inside them", fmt_count(placed as u64)));
        }
    } else {
        let moved = same_crs(join, "Join", features.crs, ctx)?;
        let other = moved.as_ref().unwrap_or(join);
        let index = box_index(other.features.iter().map(|f| f.geom.as_ref()));
        let reach = if relation == "near" { distance } else { 0.0 };
        // Each feature on a core of its own.
        hits = ctx.gate.each(features.len(), |k| {
            let f = &features.features[k];
            let Some(g) = &f.geom else { return Ok(Vec::new()) };
            let Some(r) = g.bounding_rect() else { return Ok(Vec::new()) };
            let env = AABB::from_corners([r.min().x - reach, r.min().y - reach], [r.max().x + reach, r.max().y + reach]);
            let mut found: Vec<usize> = index
                .locate_in_envelope_intersecting(&env)
                .map(|b| b.data)
                .filter(|j| other.features[*j].geom.as_ref().is_some_and(|h| related(&relation, g, h, distance)))
                .collect();
            // In the order of the stream, so that the first is the same from run to run.
            found.sort_unstable();
            Ok(found)
        })?;
    }
    let mut joined = features.like();
    let mut unjoined = features.like();
    if mode == "summary" || mode == "count" {
        let count_name = p.get("count").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "join_count".into());
        let numbers = p.get("numbers").unwrap_or_else(|| "sum".into());
        let numeric = |i: usize| matches!(join.fields[i].ty, FieldType::Int | FieldType::Real) && numbers != "list";
        joined.fields.push(Field::new(count_name.clone(), FieldType::Int));
        for (i, name) in &fetched {
            let ty = if !numeric(*i) {
                FieldType::Text
            } else if numbers == "mean" || join.fields[*i].ty == FieldType::Real {
                FieldType::Real
            } else {
                FieldType::Int
            };
            joined.fields.push(Field::new(name.clone(), ty));
        }
        for (k, f) in features.features.iter().enumerate() {
            let mut attrs = f.attrs.clone();
            attrs.push(Value::Int(hits[k].len() as i64));
            for (i, _) in &fetched {
                let values: Vec<&Value> = hits[k].iter().map(|h| &join.features[*h].attrs[*i]).filter(|v| !v.is_null()).collect();
                attrs.push(if values.is_empty() {
                    Value::Null
                } else if numeric(*i) {
                    let nums: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
                    let x = match numbers.as_str() {
                        "mean" => nums.iter().sum::<f64>() / nums.len().max(1) as f64,
                        "min" => nums.iter().copied().fold(f64::INFINITY, f64::min),
                        "max" => nums.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        _ => nums.iter().sum(),
                    };
                    if numbers != "mean" && join.fields[*i].ty == FieldType::Int {
                        Value::Int(x as i64)
                    } else {
                        Value::Real(x)
                    }
                } else {
                    let mut seen: Vec<String> = Vec::new();
                    for v in values {
                        let t = v.to_text();
                        if !seen.contains(&t) {
                            seen.push(t);
                        }
                    }
                    Value::Text(seen.join(", "))
                });
            }
            joined.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
        }
    } else {
        joined.fields.extend(fetched.iter().map(|(i, name)| Field::new(name.clone(), join.fields[*i].ty)));
        let mut several = 0usize;
        for (k, f) in features.features.iter().enumerate() {
            if hits[k].is_empty() {
                unjoined.features.push(f.clone());
                continue;
            }
            several += usize::from(hits[k].len() > 1);
            let take = if mode == "all" { hits[k].len() } else { 1 };
            for h in &hits[k][..take] {
                let mut attrs = f.attrs.clone();
                attrs.extend(fetched.iter().map(|(i, _)| join.features[*h].attrs[*i].clone()));
                joined.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
            }
        }
        if several > 0 && mode != "all" {
            ctx.notes.info(format!(
                "{} features found more than one and got the attributes of the first. Choose Every one or Summarized to get them all",
                fmt_count(several as u64)
            ));
        }
    }
    Ok(vec![("Joined".into(), joined), ("Unjoined".into(), unjoined)])
}

pub(super) type Boxed = GeomWithData<Rectangle<[f64; 2]>, usize>;

/// An index of the bounding boxes of geometries, by their place in a list.
pub(super) fn box_index<'a>(geoms: impl Iterator<Item = Option<&'a Geometry<f64>>>) -> RTree<Boxed> {
    let boxes: Vec<Boxed> = geoms
        .enumerate()
        .filter_map(|(i, g)| {
            let r = g?.bounding_rect()?;
            Some(GeomWithData::new(Rectangle::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]), i))
        })
        .collect();
    RTree::bulk_load(boxes)
}

/// The other stream in the coordinate system of the first. Says so in the log when it had
/// to be reprojected.
pub(super) fn same_crs(other: &Table, port: &str, to: Option<u32>, ctx: &mut NodeCtx) -> Result<Option<Table>> {
    match (to, other.crs) {
        (Some(a), Some(b)) if a != b && other.has_geometry() => {
            ctx.notes.warn(format!("{} was reprojected from {} to {} for the comparison.", port, crs_label(Some(b)), crs_label(Some(a))));
            Ok(Some(reproject(other, a, ctx.backend)?))
        }
        _ => Ok(None),
    }
}

/// The attributes to take from another stream: the chosen ones, or all, with a prefix.
/// Those whose names are already taken are left out, with a warning.
pub(super) fn fetched_fields(p: &P, have: &Table, from: &Table, port: &str, ctx: &mut NodeCtx) -> Result<Vec<(usize, String)>> {
    let prefix = p.get("prefix").unwrap_or_default();
    let wanted = p.list("attributes");
    let chosen: Vec<usize> =
        if wanted.is_empty() { (0..from.fields.len()).collect() } else { wanted.iter().map(|n| from.need_field(n)).collect::<Result<_>>()? };
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut clashes = Vec::new();
    for i in chosen {
        let name = format!("{}{}", prefix, from.fields[i].name);
        if have.field_index(&name).is_some() || out.iter().any(|f| f.1 == name) {
            clashes.push(name);
        } else {
            out.push((i, name));
        }
    }
    if !clashes.is_empty() {
        ctx.notes.warn(format!("Not taken from {}, since the names are already there: {}. Give a prefix to take them.", port, clashes.join(", ")));
    }
    Ok(out)
}

// ---------- Find nearest ----------

pub(super) const NEAREST_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "max_distance",
        label: "Largest distance",
        kind: ParamKind::Text,
        default: "",
        hint: "Features with nothing this near go to Unmatched. Empty looks as far as it takes. In the units of the coordinate system",
    },
    ParamSpec { key: "distance", label: "Distance attribute", kind: ParamKind::Text, default: "distance", hint: "The distance to the nearest one" },
    ParamSpec {
        key: "attributes",
        label: "Attributes to fetch",
        kind: ParamKind::Attributes("Candidates"),
        default: "",
        hint: "From the nearest candidate. Empty fetches all",
    },
    ParamSpec {
        key: "prefix",
        label: "Prefix",
        kind: ParamKind::Text,
        default: "",
        hint: "Put in front of the fetched names, for example near_. Without it, a name the features already have is not fetched",
    },
];

pub(super) fn run_nearest(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (base, cands) = (input(inp, "Base"), input(inp, "Candidates"));
    let moved = same_crs(cands, "Candidates", base.crs, ctx)?;
    let test = moved.as_ref().unwrap_or(cands);
    if ctx.geographic(base.crs) == Some(true) {
        ctx.notes.warn(format!("The distances are in degrees, since the stream is in {}. Reproject first to get metres.", crs_label(base.crs)));
    }
    let limit = number_param(p, "max_distance", "The largest distance")?.filter(|d| *d >= 0.0);
    let dist_name = p.get("distance").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "distance".into());
    let fetched = fetched_fields(p, base, cands, "Candidates", ctx)?;
    let index = box_index(test.features.iter().map(|f| f.geom.as_ref()));
    let mut matched = base.like();
    let dist_at = match matched.field_index(&dist_name) {
        Some(i) => {
            matched.fields[i].ty = FieldType::Real;
            Some(i)
        }
        None => {
            matched.fields.push(Field::new(dist_name.clone(), FieldType::Real));
            None
        }
    };
    matched.fields.extend(fetched.iter().map(|(i, n)| Field::new(n.clone(), cands.fields[*i].ty)));
    let mut unmatched = base.like();
    for f in &base.features {
        let Some(g) = &f.geom else {
            unmatched.features.push(f.clone());
            continue;
        };
        let Some(r) = g.bounding_rect() else {
            unmatched.features.push(f.clone());
            continue;
        };
        // The boxes come nearest first, measured from the middle of the box of the feature.
        // A box that is farther than the best distance plus half the diagonal of that box
        // cannot hold anything nearer.
        let c = r.center();
        let reach = (r.width().powi(2) + r.height().powi(2)).sqrt() / 2.0;
        let mut best: Option<(f64, usize)> = None;
        for (cand, d2) in index.nearest_neighbor_iter_with_distance_2(&[c.x, c.y]) {
            let lower = d2.sqrt() - reach;
            if best.is_some_and(|b| lower > b.0) || limit.is_some_and(|l| lower > l) {
                break;
            }
            let Some(other) = &test.features[cand.data].geom else { continue };
            let d = Euclidean.distance(g, other);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, cand.data));
            }
        }
        match best.filter(|b| limit.is_none_or(|l| b.0 <= l)) {
            Some((d, k)) => {
                let mut attrs = f.attrs.clone();
                match dist_at {
                    Some(i) => attrs[i] = Value::Real(d),
                    None => attrs.push(Value::Real(d)),
                }
                attrs.extend(fetched.iter().map(|(i, _)| cands.features[k].attrs[*i].clone()));
                matched.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
            }
            None => unmatched.features.push(f.clone()),
        }
    }
    Ok(vec![("Matched".into(), matched), ("Unmatched".into(), unmatched)])
}

// ---------- Split lines ----------

pub(super) const SPLIT_AT: &[(&str, &str)] =
    &[("both", "Points and crossings"), ("points", "Points only"), ("crossings", "Crossings only"), ("corners", "Every corner: a line for each segment")];

pub(super) const SPLITTER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "at",
        label: "Split at",
        kind: ParamKind::Choice(SPLIT_AT),
        default: "both",
        hint: "The points connected to Points, the places where lines cross or meet each other, or both. Or every corner, so that each segment is a line of its own",
    },
    ParamSpec {
        key: "areas",
        label: "Areas",
        kind: ParamKind::Choice(&[("keep", "To Other, as they are"), ("outline", "Their outlines split too")]),
        default: "keep",
        hint: "At every corner",
    },
    ParamSpec {
        key: "tolerance",
        label: "Tolerance",
        kind: ParamKind::Text,
        default: "0.01",
        hint: "How far from a line a point may be and still split it, in the units of the coordinate system",
    },
    ParamSpec {
        key: "part",
        label: "Part number",
        kind: ParamKind::OptionalName("part"),
        default: "",
        hint: "Numbers the parts of each line from 1, in the order along the line",
    },
];

/// A place along a line: the segment and how far along it, from 0 to 1.
pub(super) type At = (usize, f64);

/// Cuts a line at the places, which may be in any order. Places at the ends are skipped.
pub(super) fn cut_line(line: &LineString<f64>, mut at: Vec<At>) -> Vec<LineString<f64>> {
    let n = line.0.len();
    if n < 2 {
        return vec![line.clone()];
    }
    // A place at the end of a segment is the start of the next one.
    for a in at.iter_mut() {
        if a.1 >= 1.0 && a.0 + 1 < n - 1 {
            *a = (a.0 + 1, 0.0);
        }
    }
    at.retain(|a| !(a.0 == 0 && a.1 <= 0.0) && !(a.0 == n - 2 && a.1 >= 1.0));
    at.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    at.dedup_by(|a, b| a.0 == b.0 && (a.1 - b.1).abs() < 1e-9);
    let point = |a: At| {
        let (p, q) = (line.0[a.0], line.0[a.0 + 1]);
        Coord { x: p.x + (q.x - p.x) * a.1, y: p.y + (q.y - p.y) * a.1 }
    };
    let mut parts = Vec::new();
    let mut current = vec![line.0[0]];
    let mut seg = 0;
    for a in at {
        while seg < a.0 {
            seg += 1;
            current.push(line.0[seg]);
        }
        let c = point(a);
        if current.last() != Some(&c) {
            current.push(c);
        }
        if current.len() >= 2 {
            parts.push(LineString::new(std::mem::take(&mut current)));
        } else {
            current.clear();
        }
        current.push(c);
    }
    for c in &line.0[seg + 1..] {
        if current.last() != Some(c) {
            current.push(*c);
        }
    }
    if current.len() >= 2 {
        parts.push(LineString::new(current));
    }
    parts
}

/// The nearest place on a segment to a point, and how far away it is.
pub(super) fn on_segment(p: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> (f64, f64) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) };
    let (x, y) = (a.x + dx * t, a.y + dy * t);
    (t, ((p.x - x).powi(2) + (p.y - y).powi(2)).sqrt())
}

pub(super) fn run_splitter(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::line_intersection::{line_intersection, LineIntersection};
    let (lines, points) = (input(inp, "Lines"), input(inp, "Points"));
    let how = p.get("at").unwrap_or_else(|| "both".into());
    if how == "corners" {
        return super::geom_make::split_at_corners(p, lines, ctx);
    }
    let tol = number_param(p, "tolerance", "The tolerance")?.unwrap_or(0.01).max(0.0);
    let part_name = p.get("part").filter(|n| !n.trim().is_empty());
    let moved = same_crs(points, "Points", lines.crs, ctx)?;
    let points = moved.as_ref().unwrap_or(points);

    // Every line of every feature, as (feature, line).
    let mut strings: Vec<(usize, LineString<f64>)> = Vec::new();
    let mut other = lines.like();
    for (i, f) in lines.features.iter().enumerate() {
        match &f.geom {
            Some(Geometry::LineString(l)) => strings.push((i, l.clone())),
            Some(Geometry::MultiLineString(m)) => strings.extend(m.0.iter().map(|l| (i, l.clone()))),
            Some(Geometry::Line(l)) => strings.push((i, LineString::new(vec![l.start, l.end]))),
            _ => other.features.push(f.clone()),
        }
    }
    // Every segment, by (line, segment).
    let segs: Vec<(usize, usize, Coord<f64>, Coord<f64>)> =
        strings.iter().enumerate().flat_map(|(k, (_, l))| l.0.windows(2).enumerate().map(move |(s, w)| (k, s, w[0], w[1]))).collect();
    let boxes: Vec<GeomWithData<Rectangle<[f64; 2]>, usize>> = segs
        .iter()
        .enumerate()
        .map(|(i, (_, _, a, b))| GeomWithData::new(Rectangle::from_corners([a.x.min(b.x), a.y.min(b.y)], [a.x.max(b.x), a.y.max(b.y)]), i))
        .collect();
    let index = RTree::bulk_load(boxes);
    let mut cuts: Vec<Vec<At>> = vec![Vec::new(); strings.len()];

    if how != "crossings" {
        let mut unused = 0usize;
        for f in &points.features {
            let coords: Vec<Coord<f64>> = match &f.geom {
                Some(Geometry::Point(pt)) => vec![pt.0],
                Some(Geometry::MultiPoint(m)) => m.0.iter().map(|p| p.0).collect(),
                _ => continue,
            };
            for c in coords {
                let env = AABB::from_corners([c.x - tol, c.y - tol], [c.x + tol, c.y + tol]);
                let mut hit = false;
                for b in index.locate_in_envelope_intersecting(&env) {
                    let (k, s, a, bb) = segs[b.data];
                    let (t, d) = on_segment(c, a, bb);
                    if d <= tol {
                        cuts[k].push((s, t));
                        hit = true;
                    }
                }
                unused += usize::from(!hit);
            }
        }
        if unused > 0 {
            ctx.notes.info(format!("{} points lie on no line and split nothing", fmt_count(unused as u64)));
        }
        if how == "points" && points.features.is_empty() {
            ctx.notes.warn("Nothing is connected to Points, so no line was split.".to_string());
        }
    }
    if how != "points" {
        for (i, (k, s, a, b)) in segs.iter().enumerate() {
            let env = AABB::from_corners([a.x.min(b.x), a.y.min(b.y)], [a.x.max(b.x), a.y.max(b.y)]);
            for o in index.locate_in_envelope_intersecting(&env) {
                // Each pair once, and not a segment with its neighbours on the same line.
                if o.data <= i {
                    continue;
                }
                let (k2, s2, a2, b2) = segs[o.data];
                if k2 == *k && s2.abs_diff(*s) <= 1 {
                    continue;
                }
                let found = line_intersection(geo::Line::new(*a, *b), geo::Line::new(a2, b2));
                let place = match found {
                    Some(LineIntersection::SinglePoint { intersection, .. }) => vec![intersection],
                    // Lines that run along each other are split where the shared part starts and ends.
                    Some(LineIntersection::Collinear { intersection }) => vec![intersection.start, intersection.end],
                    None => continue,
                };
                for c in place {
                    cuts[*k].push((*s, on_segment(c, *a, *b).0));
                    cuts[k2].push((s2, on_segment(c, a2, b2).0));
                }
            }
        }
    }

    let mut out = lines.like();
    out.raster = false;
    let part_at = part_name.as_ref().map(|name| match out.field_index(name) {
        Some(i) => {
            out.fields[i].ty = FieldType::Int;
            i
        }
        None => {
            out.fields.push(Field::new(name.clone(), FieldType::Int));
            out.fields.len() - 1
        }
    });
    let mut numbers: HashMap<usize, i64> = HashMap::new();
    let (mut before, mut after) = (0usize, 0usize);
    for ((i, line), at) in strings.iter().zip(cuts) {
        before += 1;
        for part in cut_line(line, at) {
            after += 1;
            let mut attrs = lines.features[*i].attrs.clone();
            if let Some(pi) = part_at {
                let n = numbers.entry(*i).or_insert(0);
                *n += 1;
                if pi < attrs.len() {
                    attrs[pi] = Value::Int(*n);
                } else {
                    attrs.push(Value::Int(*n));
                }
            }
            out.features.push(Feature { z: None, attrs, geom: Some(Geometry::LineString(part)) });
        }
    }
    ctx.notes.info(format!("{} lines became {}", fmt_count(before as u64), fmt_count(after as u64)));
    Ok(vec![("Lines".into(), out), ("Other".into(), other)])
}

// ---------- Overlay areas ----------

pub(super) const OVERLAY_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attributes",
        label: "Attributes to fetch",
        kind: ParamKind::Attributes("Overlay"),
        default: "",
        hint: "From the Overlay areas, for the parts in Both. Empty fetches all",
    },
    ParamSpec {
        key: "prefix",
        label: "Prefix",
        kind: ParamKind::Text,
        default: "",
        hint: "Put in front of the fetched names. Without it, a name the Areas already have is not fetched",
    },
];

pub(super) fn as_areas(g: &Geometry<f64>) -> Option<MultiPolygon<f64>> {
    match g {
        Geometry::Polygon(p) => Some(MultiPolygon::new(vec![p.clone()])),
        Geometry::MultiPolygon(m) => Some(m.clone()),
        Geometry::Rect(r) => Some(MultiPolygon::new(vec![r.to_polygon()])),
        _ => None,
    }
}

/// The parts of an overlay as a geometry, without slivers: thin strips left where the
/// edges of two areas lie on each other, which rounding makes into areas of their own. A part
/// thinner than a ten-millionth of `size`, the size of the area it came from, is a sliver.
pub(super) fn area_geom(m: MultiPolygon<f64>, size: f64) -> Option<Geometry<f64>> {
    use geo::{Area, Euclidean, Length};
    let thin = (size * 1e-7).max(1e-9);
    let parts: Vec<Polygon<f64>> =
        m.0.into_iter()
            .filter(|p| {
                let area = p.unsigned_area();
                let around = Euclidean.length(p.exterior()) + p.interiors().iter().map(|r| Euclidean.length(r)).sum::<f64>();
                area > 0.0 && around > 0.0 && 2.0 * area / around > thin
            })
            .collect();
    match parts.len() {
        0 => None,
        1 => parts.into_iter().next().map(Geometry::Polygon),
        _ => Some(Geometry::MultiPolygon(MultiPolygon::new(parts))),
    }
}

pub(super) fn run_overlay(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (a, b) = (input(inp, "Areas"), input(inp, "Overlay"));
    let moved = same_crs(b, "Overlay", a.crs, ctx)?;
    let bt = moved.as_ref().unwrap_or(b);
    let fetched = fetched_fields(p, a, b, "Overlay", ctx)?;
    let a_areas: Vec<Option<MultiPolygon<f64>>> = a.features.iter().map(|f| f.geom.as_ref().and_then(as_areas)).collect();
    let b_areas: Vec<Option<MultiPolygon<f64>>> = bt.features.iter().map(|f| f.geom.as_ref().and_then(as_areas)).collect();
    let not_areas = a_areas.iter().zip(&a.features).filter(|(m, f)| m.is_none() && f.geom.is_some()).count()
        + b_areas.iter().zip(&bt.features).filter(|(m, f)| m.is_none() && f.geom.is_some()).count();
    if not_areas > 0 {
        ctx.notes.warn(format!("{} features are not areas and were left out", fmt_count(not_areas as u64)));
    }
    let a_index = box_index(a.features.iter().map(|f| f.geom.as_ref()));
    let b_index = box_index(bt.features.iter().map(|f| f.geom.as_ref()));
    let size = |m: &MultiPolygon<f64>| m.bounding_rect().map_or(1.0, |r| r.width().max(r.height()));
    let env = |m: &MultiPolygon<f64>| m.bounding_rect().map(|r| AABB::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]));

    let mut both = a.like();
    both.raster = false;
    both.fields.extend(fetched.iter().map(|(i, n)| Field::new(n.clone(), b.fields[*i].ty)));
    let mut a_only = a.like();
    a_only.raster = false;
    let mut b_only = b.like();
    b_only.raster = false;
    b_only.crs = a.crs.or(b.crs);

    for (i, am) in a_areas.iter().enumerate() {
        let Some(am) = am else { continue };
        let Some(e) = env(am) else { continue };
        let mut rest = am.clone();
        for hit in b_index.locate_in_envelope_intersecting(&e) {
            let Some(bm) = &b_areas[hit.data] else { continue };
            if !am.intersects(bm) {
                continue;
            }
            if let Some(g) = area_geom(am.intersection(bm), size(am)) {
                let mut attrs = a.features[i].attrs.clone();
                attrs.extend(fetched.iter().map(|(k, _)| b.features[hit.data].attrs[*k].clone()));
                both.features.push(Feature { z: None, attrs, geom: Some(g) });
            }
            rest = rest.difference(bm);
        }
        if let Some(g) = area_geom(rest, size(am)) {
            a_only.features.push(Feature { z: None, attrs: a.features[i].attrs.clone(), geom: Some(g) });
        }
        if ctx.stopped() {
            return err("Stopped.");
        }
    }
    for (j, bm) in b_areas.iter().enumerate() {
        let Some(bm) = bm else { continue };
        let Some(e) = env(bm) else { continue };
        let mut rest = bm.clone();
        for hit in a_index.locate_in_envelope_intersecting(&e) {
            if let Some(am) = &a_areas[hit.data] {
                if bm.intersects(am) {
                    rest = rest.difference(am);
                }
            }
        }
        if let Some(g) = area_geom(rest, size(bm)) {
            b_only.features.push(Feature { z: None, attrs: b.features[j].attrs.clone(), geom: Some(g) });
        }
    }
    Ok(vec![("Both".into(), both), ("Areas only".into(), a_only), ("Overlay only".into(), b_only)])
}

// ---------- Clip by each area on its own (Intersect before 0.96.8) ----------

/// Features cut by each area of `over` on its own: a piece for each area a feature reaches,
/// with the attributes `fetched` from it, and what lies in none of them. `over` is in the
/// coordinate system of the features. Called Intersect before 0.96.8.
fn cut_by_each(features: &Table, over: &Table, all: &Clipper, fetched: &[(usize, String)], ctx: &mut NodeCtx) -> Result<(Table, Table)> {
    let overlay = over;
    let index = box_index(over.features.iter().map(|f| f.geom.as_ref().filter(|g| geom::family(g) == geom::Family::Area)));
    // One clipper for each area, made when it is first needed (by whichever core needs it),
    // and one for all of them to find what lies outside.
    let clippers: Vec<std::sync::OnceLock<Option<geom::Clipper>>> = (0..over.len()).map(|_| std::sync::OnceLock::new()).collect();
    let everything = Some(all);
    let mut cut = features.like();
    cut.raster = false;
    cut.fields.extend(fetched.iter().map(|(i, n)| Field::new(n.clone(), overlay.fields[*i].ty)));
    let mut outside = features.like();
    outside.raster = false;
    let mut unsupported = 0usize;
    // Each feature on a core of its own: its pieces in the areas, what is left outside, and
    // whether it could be cut.
    type Pieces = (Vec<Feature>, Option<Feature>, bool);
    let done: Vec<Pieces> = ctx.gate.each(features.len(), |k| {
        let f = &features.features[k];
        let (Some(g), Some(r)) = (&f.geom, f.geom.as_ref().and_then(|g| g.bounding_rect())) else { return Ok((Vec::new(), None, true)) };
        let mut found: Vec<usize> =
            index.locate_in_envelope_intersecting(&AABB::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y])).map(|b| b.data).collect();
        found.sort_unstable();
        let mut pieces = Vec::new();
        let mut fine = true;
        for j in found {
            let clipper = clippers[j].get_or_init(|| geom::Clipper::new(over.features[j].geom.iter()));
            let Some(c) = clipper else { continue };
            let clipped = c.clip(g);
            if !clipped.supported {
                fine = false;
                break;
            }
            if let Some(piece) = clipped.inside {
                // Heights go with the pieces: from the corners there were, along the edges
                // and in the plane of each area.
                let z = if clipped.outside.is_none() { f.z.clone() } else { f.heights().map(|z| std::sync::Arc::from(geom::lift_heights(g, z, &piece))) };
                let mut attrs = f.attrs.clone();
                attrs.extend(fetched.iter().map(|(i, _)| over.features[j].attrs[*i].clone()));
                pieces.push(Feature { z, attrs, geom: Some(piece) });
            }
        }
        let rest = match &everything {
            None => Some(g.clone()),
            Some(c) => c.clip(g).outside,
        };
        let rest = rest.map(|rest| {
            let z = if &rest == g { f.z.clone() } else { f.heights().map(|z| std::sync::Arc::from(geom::lift_heights(g, z, &rest))) };
            Feature { z, attrs: f.attrs.clone(), geom: Some(rest) }
        });
        Ok((pieces, rest, fine))
    })?;
    for (pieces, rest, fine) in done {
        if !fine {
            unsupported += 1;
        }
        cut.features.extend(pieces);
        outside.features.extend(rest);
    }
    if unsupported > 0 {
        ctx.notes.warn(format!("{} features have a kind of geometry that cannot be cut, and are left out of Inside.", fmt_count(unsupported as u64)));
    }
    Ok((cut, outside))
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "NeighborFinder",
        title: "Find nearest",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Finds for each feature in Base the nearest feature in Candidates, and gives it the distance and the attributes of that one. Features with nothing within the largest distance go to Unmatched.",
        inputs: &["Base", "Candidates"],
        outputs: &["Matched", "Unmatched"],
        ports: None,
        params: NEAREST_PARAMS,
        run: run_nearest,
        summary: |p| match p.get("max_distance") {
            Some(d) => format!("within {}", d),
            None => String::new(),
        },
    },
    NodeType {
        name: "LineSplitter",
        title: "Split lines",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Splits lines where points lie on them, where they cross or meet each other, or both, or at every corner, so that each segment is a line of its own, which was Split lines into segments before 0.96.8; the outlines of areas can be split that way too. Each part keeps the attributes of its line.",
        inputs: &["Lines", "Points"],
        outputs: &["Lines", "Other"],
        ports: None,
        params: SPLITTER_PARAMS,
        run: run_splitter,
        summary: |p| match p.get("at").as_deref() {
            Some("points") => "at points".into(),
            Some("crossings") => "at crossings".into(),
            Some("corners") => "at every corner".into(),
            _ => "at points and crossings".into(),
        },
    },
    NodeType {
        name: "AreaOnAreaOverlayer",
        title: "Overlay areas",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Lays two sets of areas over each other. Where they overlap, the part gets the attributes of both and goes to Both. What is left of each goes to Areas only and Overlay only.",
        inputs: &["Areas", "Overlay"],
        outputs: &["Both", "Areas only", "Overlay only"],
        ports: None,
        params: OVERLAY_PARAMS,
        run: run_overlay,
        summary: |_| String::new(),
    },
    NodeType {
        name: "Clipper",
        title: "Clip",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Clips points, lines, areas, 3D models and rasters against the clip areas. Lines and areas are split at the boundary. Points exactly on the boundary count as inside. All clip areas are merged into one before clipping, or each clips on its own, as the squares of a grid, and gives its attributes to the pieces in it. Heights are kept: a new corner gets its height from the edge or the plane it was cut from. 3D models, such as a TIN, a surface or buildings, are cut from above, each triangle in its own plane, and walls by upright planes along the boundary. A raster, a grid or a picture, keeps the cells whose middles are inside, cut to the areas, and gives the rest on Outside. Called Clip raster for rasters before 0.96.8.",
        inputs: &["Clipper", "Clippee"],
        outputs: &["Inside", "Outside"],
        ports: None,
        params: CLIPPER_PARAMS,
        run: run_clipper,
        summary: |_| String::new(),
    },
    NodeType {
        name: "Dissolver",
        title: "Dissolve",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Merges areas that touch or overlap, in each group. Shared boundaries disappear and overlaps count once. Areas that do not touch stay features of their own, or become one multi-area for each group as a choice. Only the group-by attributes are kept. Features that are not areas go to Remnants.",
        inputs: &["Input"],
        outputs: &["Dissolved", "Remnants"],
        ports: None,
        params: DISSOLVER_PARAMS,
        run: run_dissolver,
        summary: |p| {
            let by = p.list("group_by");
            let together = p.get("parts").as_deref() == Some("together");
            match (by.is_empty(), together) {
                (true, true) => "everything into one".to_string(),
                (true, false) => "all that touch".to_string(),
                (false, true) => format!("one for each {}", by.join(", ")),
                (false, false) => format!("by {}, all that touch", by.join(", ")),
            }
        },
    },
    NodeType {
        name: "SpatialJoiner",
        title: "Join by location",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "A spatial join, as Join attributes by location in QGIS: gives each feature the attributes of the features in Join it lies within, crosses, touches or is near: points, lines and areas in both. Each point the municipality it is in, each road the zones it crosses, each stop the nearest line. One, every one, summarized, or only counted, such as the points in each area, which was Count points in polygons before 0.96.8. Features that find nothing go to Unjoined.",
        inputs: &["Features", "Join"],
        outputs: &["Joined", "Unjoined"],
        ports: None,
        params: JOINER_PARAMS,
        run: run_spatial_joiner,
        summary: |p| {
            let attrs = p.list("attributes");
            let what = if attrs.is_empty() { "all attributes".to_string() } else { attrs.join(", ") };
            let what = match p.get("prefix") {
                Some(prefix) => format!("{} as {}…", what, prefix),
                None => what,
            };
            // Counted only: the attribute with the count.
            let what = if p.get("mode").as_deref() == Some("count") { format!("counted in {}", p.get("count").unwrap_or_else(|| "join_count".into())) } else { what };
            let how = match p.get("relation").as_deref().unwrap_or("intersects") {
                "within" => "within".to_string(),
                "contains" => "containing".to_string(),
                "crosses" => "crossing".to_string(),
                "touches" => "touching".to_string(),
                "overlaps" => "overlapping".to_string(),
                "near" => format!("within {}", p.get("distance").unwrap_or_else(|| "10".into())),
                "point" => "by a point".to_string(),
                _ => "touching or overlapping".to_string(),
            };
            let mode = match p.get("mode").as_deref() {
                Some("all") => ", every one",
                Some("summary") => ", summarized",
                _ => "",
            };
            format!("{}, {}{}", what, how, mode)
        },
    },
    NodeType {
        name: "PointOnAreaOverlayer",
        title: "Overlay points on areas",
        hidden: true,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Replaced by Join by location, which gives each point the attributes of the area it lies in, and counts the points in each area.",
        inputs: &["Point", "Area"],
        outputs: &["Point", "Area"],
        ports: None,
        params: OVERLAYER_PARAMS,
        run: run_overlayer,
        summary: |_| String::new(),
    },
];

#[cfg(test)]
mod tests {
    use super::super::run_for_test;
    use crate::mesh::{Material, Mesh, Tri, NONE};
    use crate::model::{Feature, Field, FieldType, Table, Value};
    use geo::{polygon, Geometry, Length, LineString};

    /// Two squares side by side, a and b, as the clip areas of a grid.
    fn squares() -> Table {
        let sq = |x0: f64, name: &str| Feature {
            attrs: vec![Value::Text(name.into()), Value::Text("from the grid".into())],
            geom: Some(Geometry::Polygon(polygon![(x: x0, y: 0.0), (x: x0 + 10.0, y: 0.0), (x: x0 + 10.0, y: 10.0), (x: x0, y: 10.0)])),
            z: None,
        };
        Table {
            fields: vec![Field::new("square", FieldType::Text), Field::new("name", FieldType::Text)],
            features: vec![sq(0.0, "a"), sq(10.0, "b")],
            ..Default::default()
        }
    }

    #[test]
    fn each_clip_area_cuts_its_own_piece_and_gives_it_its_attributes() {
        // A line across both squares and on out of them, and a point in b.
        let lines = Table {
            fields: vec![Field::new("name", FieldType::Text)],
            features: vec![
                Feature { attrs: vec![Value::Text("road".into())], geom: Some(Geometry::LineString(LineString::from(vec![(2.0, 5.0), (25.0, 5.0)]))), z: None },
                Feature { attrs: vec![Value::Text("well".into())], geom: Some(Geometry::Point((15.0, 3.0).into())), z: None },
            ],
            ..Default::default()
        };
        let (out, _) = run_for_test("Clipper", &[("each", "each")], vec![("Clipper", squares()), ("Clippee", lines.clone())]).unwrap();
        let inside = &out.iter().find(|o| o.0 == "Inside").unwrap().1;
        let outside = &out.iter().find(|o| o.0 == "Outside").unwrap().1;
        // The name of the features is theirs: the grid's name is not fetched without a prefix.
        assert_eq!(inside.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["name", "square"]);
        let got: Vec<(String, String, f64)> = inside
            .features
            .iter()
            .map(|f| {
                let len = match f.geom.as_ref().unwrap() {
                    Geometry::LineString(l) => geo::Euclidean.length(l),
                    _ => 0.0,
                };
                (f.attrs[0].to_text(), f.attrs[1].to_text(), (len * 1000.0).round() / 1000.0)
            })
            .collect();
        assert_eq!(got, [("road".into(), "a".into(), 8.0), ("road".into(), "b".into(), 10.0), ("well".into(), "b".into(), 0.0)]);
        // What lies in no square goes to Outside, as when they clip as one.
        assert_eq!(outside.features.len(), 1);
        assert!(matches!(outside.features[0].geom, Some(Geometry::LineString(ref l)) if (l.0[0].x - 20.0).abs() < 1e-9));
        // With a prefix, the grid's name comes too.
        let (out, _) = run_for_test("Clipper", &[("each", "each"), ("prefix", "grid_")], vec![("Clipper", squares()), ("Clippee", lines)]).unwrap();
        let names: Vec<String> = out[0].1.fields.iter().map(|f| f.name.clone()).collect();
        assert_eq!(names, ["name", "grid_square", "grid_name"]);
    }

    #[test]
    fn a_3d_model_is_cut_into_a_piece_for_each_clip_area() {
        // A flat plate 5 m up from x 0 to 20, over both squares, as a mesh from a DWG.
        let mesh = Mesh {
            positions: vec![[0.0, 2.0, 5.0], [20.0, 2.0, 5.0], [20.0, 8.0, 5.0], [0.0, 8.0, 5.0]],
            tris: vec![
                Tri { v: [0, 1, 2], n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 },
                Tri { v: [0, 2, 3], n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 },
            ],
            materials: vec![Material::default()],
            objects: vec!["plate".into()],
            ..Default::default()
        };
        let model = Table {
            fields: vec![Field::new("layer", FieldType::Text)],
            features: vec![Feature {
                attrs: vec![Value::Text("decks".into())],
                geom: Some(Geometry::Polygon(polygon![(x: 0.0, y: 2.0), (x: 20.0, y: 2.0), (x: 20.0, y: 8.0), (x: 0.0, y: 8.0)])),
                z: None,
            }],
            mesh: Some(std::sync::Arc::new(mesh)),
            ..Default::default()
        };
        let (out, _) = run_for_test("Clipper", &[("each", "each")], vec![("Clipper", squares()), ("Clippee", model.clone())]).unwrap();
        let inside = &out[0].1;
        let m = inside.mesh.as_deref().unwrap();
        // Two pieces, one for each square, each its own object with the square's name.
        assert_eq!(inside.features.len(), 2);
        assert_eq!(m.objects.len(), 2);
        let squares_of: Vec<String> = inside.features.iter().map(|f| f.attrs[1].to_text()).collect();
        assert_eq!(squares_of, ["a", "b"]);
        for k in 0..2u32 {
            let xs: Vec<f64> = m.tris.iter().filter(|t| t.object == k).flat_map(|t| t.v.map(|i| m.positions[i as usize][0])).collect();
            assert!(!xs.is_empty());
            let (lo, hi) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
            assert!((lo - k as f64 * 10.0).abs() < 1e-6 && (hi - (k as f64 + 1.0) * 10.0).abs() < 1e-6, "{} {}", lo, hi);
        }
        // Nothing lies outside the squares.
        assert!(out[1].1.features.is_empty());
        // Filter by geometry type sends the model to Surface, with its triangles.
        let (out, _) = run_for_test("GeometryFilter", &[], vec![("Input", model)]).unwrap();
        let surface = &out.iter().find(|o| o.0 == "Surface").unwrap().1;
        assert_eq!(surface.features.len(), 1);
        assert_eq!(surface.mesh.as_deref().map(|m| m.tris.len()), Some(2));
        assert!(out.iter().find(|o| o.0 == "Area").unwrap().1.features.is_empty());
    }

    #[test]
    fn area_and_length_at_once() {
        let mut t = squares();
        t.crs = Some(3006);
        let (out, _) = run_for_test("AreaCalculator", &[("what", "both"), ("length_unit", "km")], vec![("Input", t)]).unwrap();
        let o = &out[0].1;
        let at = |n: &str| o.features[0].attrs[o.field_index(n).unwrap()].clone();
        assert_eq!((at("_area"), at("_length")), (Value::Real(100.0), Value::Real(0.04)));
    }
}
