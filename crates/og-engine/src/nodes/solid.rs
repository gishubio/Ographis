//! Tools that make 3D models: Extrude, which raises areas into blocks, and Generate TIN,
//! which turns a raster of heights or points with heights into triangles.

use super::*;
use crate::expr::{self, Row};
use crate::mesh::{Material, Mesh, Tri, NONE};
use crate::model::{err, fmt_count, Field, FieldType, GbError, Grid, Result, Value};
use geo::{Geometry, TriangulateEarcut};
use rayon::prelude::*;
use std::sync::Arc;

// ---------- Extrude ----------

const BASES: &[(&str, &str)] = &[
    ("value", "At the value below"),
    ("z", "At the lowest height the area has"),
    ("terrain", "On the terrain model on Terrain, at its lowest point under the area"),
];

const TOPS: &[(&str, &str)] = &[("above", "Above the base, as a height"), ("absolute", "As the height of the top, above the sea")];

const EXTRUDE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "height",
        label: "Height",
        kind: ParamKind::Expression("Input"),
        default: "20",
        hint: "A number, such as 20, an attribute with the height, or an expression such as floors * 3",
    },
    ParamSpec { key: "top", label: "The height is", kind: ParamKind::Choice(TOPS), default: "above", hint: "" },
    ParamSpec { key: "base", label: "Bottom", kind: ParamKind::Choice(BASES), default: "value", hint: "Where the block stands" },
    ParamSpec {
        key: "base_value",
        label: "Bottom value",
        kind: ParamKind::Expression("Input"),
        default: "0",
        hint: "A number or an attribute. With the terrain or the heights of the area, it is added to them",
    },
    ParamSpec { key: "wall_colour", label: "Colour of the walls", kind: ParamKind::Text, default: "#d9d4ca", hint: "As #rrggbb" },
    ParamSpec { key: "roof_colour", label: "Colour of the top", kind: ParamKind::Text, default: "#a39e96", hint: "" },
    ParamSpec {
        key: "name",
        label: "Names from",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "An attribute that names each block in the model, such as an id. Empty numbers them",
    },
];

/// A colour parameter as sRGB from 0 to 1.
pub(super) fn colour_param(p: &P, key: &str, default: [f32; 3]) -> Result<[f32; 3]> {
    match p.get(key).filter(|v| !v.trim().is_empty()) {
        None => Ok(default),
        Some(v) => match crate::mapdraw::parse_color(&v) {
            Ok(Some(c)) => Ok([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]),
            Ok(None) => Ok(default),
            Err(e) => Err(GbError(format!("{}: {}", key.replace('_', " "), e))),
        },
    }
}

/// The terrain model on Terrain, in the coordinate system `crs`.
fn terrain_in(inp: &Inputs, crs: Option<u32>, ctx: &NodeCtx) -> Result<Option<Grid>> {
    let t = input(inp, "Terrain");
    let Some(g) = t.grid.as_deref() else { return Ok(None) };
    Ok(match (g.crs, crs) {
        (Some(a), Some(b)) if a != b => {
            let warped = reproject(&crate::model::Table { grid: Some(Arc::new(g.clone())), crs: Some(a), ..Default::default() }, b, ctx.backend)?;
            warped.grid.map(|g| (*g).clone())
        }
        _ => Some(g.clone()),
    })
}

/// A number from an expression, or None when it gives no number.
fn number_of(e: &expr::Compiled, f: &crate::model::Feature, i: usize, geographic: Option<bool>) -> Result<Option<f64>> {
    let v = e.eval(&Row { feature: f, index: i, geographic }).map_err(|e| at_row(e, i))?;
    Ok(v.as_f64().or_else(|| crate::model::parse_number(&v.to_text())))
}

/// Adds a block to the model: walls around every ring of the polygons, a top and a bottom.
pub(crate) fn add_block(mesh: &mut Mesh, polys: &[&geo::Polygon<f64>], z0: f64, z1: f64, object: u32) {
    use geo::Winding;
    for poly in polys {
        // The outer ring counter-clockwise and the holes clockwise, so that the outside of a
        // wall is always to the right as the ring goes.
        let mut rings: Vec<geo::LineString<f64>> = Vec::with_capacity(1 + poly.interiors().len());
        let mut outer = poly.exterior().clone();
        outer.make_ccw_winding();
        rings.push(outer);
        for r in poly.interiors() {
            let mut r = r.clone();
            r.make_cw_winding();
            rings.push(r);
        }
        for ring in &rings {
            for w in ring.0.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len = (dx * dx + dy * dy).sqrt();
                if len < 1e-9 {
                    continue;
                }
                let n = mesh.normals.len() as u32;
                mesh.normals.push([(dy / len) as f32, (-dx / len) as f32, 0.0]);
                let v = mesh.positions.len() as u32;
                mesh.positions.extend_from_slice(&[[a.x, a.y, z0], [b.x, b.y, z0], [b.x, b.y, z1], [a.x, a.y, z1]]);
                for t in [[v, v + 1, v + 2], [v, v + 2, v + 3]] {
                    mesh.tris.push(Tri { v: t, n: [n; 3], t: [NONE; 3], material: 0, object });
                }
            }
        }
        // The top and the bottom, cut into triangles with the holes left open.
        let raw = (*poly).earcut_triangles_raw();
        let (up, down) = (mesh.normals.len() as u32, mesh.normals.len() as u32 + 1);
        mesh.normals.push([0.0, 0.0, 1.0]);
        mesh.normals.push([0.0, 0.0, -1.0]);
        let top = mesh.positions.len() as u32;
        mesh.positions.extend(raw.vertices.iter().map(|c| [c[0], c[1], z1]));
        let bottom = mesh.positions.len() as u32;
        mesh.positions.extend(raw.vertices.iter().map(|c| [c[0], c[1], z0]));
        for t in raw.triangle_indices.as_chunks::<3>().0.iter() {
            let (a, b, c) = (t[0] as u32, t[1] as u32, t[2] as u32);
            // Counter-clockwise seen from above for the top, the other way for the bottom.
            let (pa, pb, pc) = (raw.vertices[t[0]], raw.vertices[t[1]], raw.vertices[t[2]]);
            let ccw = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]) > 0.0;
            let (b, c) = if ccw { (b, c) } else { (c, b) };
            mesh.tris.push(Tri { v: [top + a, top + b, top + c], n: [up; 3], t: [NONE; 3], material: 1, object });
            mesh.tris.push(Tri { v: [bottom + a, bottom + c, bottom + b], n: [down; 3], t: [NONE; 3], material: 1, object });
        }
    }
}

fn run_extrude(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let names = table.field_names();
    let height_text = p.need("height", "Write the height: a number such as 20, or an attribute with the height.")?;
    let height = expr::compile(&height_text, &names).map_err(|e| GbError(format!("The height: {}", e.0)))?;
    let base_text = p.get("base_value").filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "0".into());
    let base_value = expr::compile(&base_text, &names).map_err(|e| GbError(format!("The bottom value: {}", e.0)))?;
    let base = p.get("base").unwrap_or_else(|| "value".into());
    let absolute = p.get("top").as_deref() == Some("absolute");
    let terrain = if base == "terrain" {
        match terrain_in(inp, table.crs, ctx)? {
            Some(g) => Some(g),
            None => return err("Connect a terrain model to Terrain: an input that reads it with Cells as a grid of heights."),
        }
    } else {
        None
    };
    let name_at = p.get("name").filter(|v| !v.is_empty()).and_then(|n| table.field_index(&n));
    let walls = colour_param(p, "wall_colour", [0.85, 0.83, 0.79])?;
    let roof = colour_param(p, "roof_colour", [0.64, 0.62, 0.59])?;
    let mut mesh = Mesh {
        materials: vec![
            Material { name: "walls".into(), diffuse: walls, ..Default::default() },
            Material { name: "top".into(), diffuse: roof, ..Default::default() },
        ],
        ..Default::default()
    };
    let mut out = table.like();
    for n in ["z_base", "z_top", "height"] {
        if out.field_index(n).is_none() {
            out.fields.push(Field::new(n, FieldType::Real));
        }
    }
    let (at_base, at_top, at_height) = (out.field_index("z_base").unwrap(), out.field_index("z_top").unwrap(), out.field_index("height").unwrap());
    let geographic = ctx.geographic(table.crs);
    let (mut not_areas, mut no_height, mut outside, mut flat) = (0usize, 0usize, 0usize, 0usize);
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    for (i, f) in table.features.iter().enumerate() {
        let polys: Vec<&geo::Polygon<f64>> = match &f.geom {
            Some(Geometry::Polygon(p)) => vec![p],
            Some(Geometry::MultiPolygon(m)) => m.0.iter().collect(),
            Some(_) => {
                not_areas += 1;
                continue;
            }
            None => continue,
        };
        if polys.is_empty() {
            continue;
        }
        let Some(h) = number_of(&height, f, i, geographic)? else {
            no_height += 1;
            continue;
        };
        let add = number_of(&base_value, f, i, geographic)?.unwrap_or(0.0);
        let z0 = match base.as_str() {
            "z" => f.heights().map_or(0.0, |z| z.iter().copied().fold(f64::INFINITY, f64::min)) + add,
            "terrain" => {
                let g = terrain.as_ref().expect("a terrain");
                let lowest =
                    polys.iter().flat_map(|p| p.exterior().0.iter()).filter_map(|c| crate::terrain::height_at(g, c.x, c.y)).fold(f64::INFINITY, f64::min);
                if lowest.is_finite() {
                    lowest + add
                } else {
                    outside += 1;
                    add
                }
            }
            _ => add,
        };
        let z1 = if absolute { h } else { z0 + h };
        if z1 <= z0 {
            flat += 1;
            continue;
        }
        let object = out.features.len() as u32;
        add_block(&mut mesh, &polys, z0, z1, object);
        mesh.objects.push(match name_at.map(|k| &f.attrs[k]) {
            Some(v) if !matches!(v, Value::Null) => v.to_text(),
            _ => (i + 1).to_string(),
        });
        let mut attrs = f.attrs.clone();
        attrs.resize(out.fields.len(), Value::Null);
        attrs[at_base] = Value::Real(round(z0));
        attrs[at_top] = Value::Real(round(z1));
        attrs[at_height] = Value::Real(round(z1 - z0));
        let geom = f.geom.clone();
        let n = geom.as_ref().map_or(0, crate::geom::coord_count);
        out.features.push(crate::model::Feature { attrs, geom, z: Some(Arc::from(vec![z1; n])) });
    }
    if not_areas > 0 {
        ctx.notes.warn(format!("{} features are not areas and were left out.", fmt_count(not_areas as u64)));
    }
    if no_height > 0 {
        ctx.notes.warn(format!("{} areas have no height that is a number and were left out.", fmt_count(no_height as u64)));
    }
    if flat > 0 {
        ctx.notes.warn(format!("{} areas have a top that is not above the bottom and were left out.", fmt_count(flat as u64)));
    }
    if outside > 0 {
        ctx.notes.warn(format!("{} areas lie outside the terrain model and stand at the bottom value.", fmt_count(outside as u64)));
    }
    ctx.notes.info(format!("{} blocks, {} triangles", fmt_count(out.features.len() as u64), fmt_count(mesh.tris.len() as u64)));
    out.raster = false;
    out.mesh = Some(Arc::new(mesh));
    Ok(vec![("Output".into(), out)])
}

fn extrude_summary(p: &P) -> String {
    let h = p.get("height").unwrap_or_else(|| "20".into());
    let h = if h.trim().parse::<f64>().is_ok() { format!("{} m", h.trim()) } else { h };
    match p.get("base").as_deref() {
        Some("terrain") => format!("{} on the terrain", h),
        _ => h,
    }
}

// ---------- Generate TIN ----------

const TIN_KINDS: &[(&str, &str)] = &[
    ("3d", "3D: triangles with heights and a 3D model, for Render 3D scene and the 3D view"),
    ("2d", "2D: flat triangles with their heights, slope and aspect as attributes"),
];

const TIN_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "kind", label: "Make", kind: ParamKind::Choice(TIN_KINDS), default: "3d", hint: "" },
    ParamSpec {
        key: "tolerance",
        label: "Tolerance",
        kind: ParamKind::Text,
        default: "1",
        hint: "From a raster: how far the triangles may lie from the heights of the cells, in metres. Smaller gives more triangles",
    },
    ParamSpec {
        key: "most",
        label: "At most corners",
        kind: ParamKind::Text,
        default: "50000",
        hint: "From a raster: the most points the triangles may have, however rough the terrain",
    },
    ParamSpec { key: "colour", label: "Colour", kind: ParamKind::Text, default: "#8f9c72", hint: "Of the 3D model, as #rrggbb" },
];

/// A corner of the TIN, with its height.
#[derive(Debug, Clone, Copy)]
struct Corner {
    pos: spade::Point2<f64>,
    z: f64,
}

impl spade::HasPosition for Corner {
    type Scalar = f64;
    fn position(&self) -> spade::Point2<f64> {
        self.pos
    }
}

type Tin = spade::DelaunayTriangulation<Corner>;

/// The triangles of a TIN as corners, counter-clockwise seen from above.
fn triangles_of(t: &Tin) -> Vec<[Corner; 3]> {
    use spade::Triangulation;
    t.inner_faces().map(|f| f.vertices().map(|v| *v.data())).collect()
}

/// The cell that lies furthest from the triangle, above or below, and how far: (distance,
/// column, row). The corners are in cell coordinates, where the middle of the first cell is
/// 0, 0.
fn worst_cell(g: &Grid, tri: &[[f64; 3]; 3]) -> Option<(f64, usize, usize)> {
    let [a, b, c] = *tri;
    let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if det.abs() < 1e-12 {
        return None;
    }
    let lo_x = a[0].min(b[0]).min(c[0]).ceil().max(0.0) as usize;
    let hi_x = (a[0].max(b[0]).max(c[0]).floor() as usize).min(g.width - 1);
    let lo_y = a[1].min(b[1]).min(c[1]).ceil().max(0.0) as usize;
    let hi_y = (a[1].max(b[1]).max(c[1]).floor() as usize).min(g.height - 1);
    let mut worst: Option<(f64, usize, usize)> = None;
    for row in lo_y..=hi_y {
        let y = row as f64;
        for col in lo_x..=hi_x {
            let x = col as f64;
            let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
            let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
            let l3 = 1.0 - l1 - l2;
            if l1 < -1e-9 || l2 < -1e-9 || l3 < -1e-9 {
                continue;
            }
            let v = g.values[row * g.width + col];
            if v.is_nan() {
                continue;
            }
            let d = (v as f64 - (l1 * a[2] + l2 * b[2] + l3 * c[2])).abs();
            if worst.is_none_or(|w| d > w.0) {
                worst = Some((d, col, row));
            }
        }
    }
    worst
}

/// A TIN from a grid of heights by greedy insertion: it starts from the edge of the cells
/// that have a value, and in rounds adds the cell that lies furthest from each triangle,
/// until every cell is within `tolerance` or there are `most` corners.
fn tin_of_grid(g: &Grid, tolerance: f64, most: usize, going: &dyn Fn(usize) -> bool) -> Result<Tin> {
    use spade::Triangulation;
    let gt = g.geotransform;
    let at = |c: usize, r: usize| Corner {
        pos: spade::Point2::new(gt[0] + (c as f64 + 0.5) * gt[1], gt[3] + (r as f64 + 0.5) * gt[5]),
        z: g.values[r * g.width + c] as f64,
    };
    let valid = |c: usize, r: usize| !g.values[r * g.width + c].is_nan();
    let mut tin = Tin::default();
    let mut taken = std::collections::HashSet::new();
    let mut put = |tin: &mut Tin, c: usize, r: usize| -> Result<()> {
        if taken.insert((c, r)) {
            tin.insert(at(c, r)).map_err(|e| GbError(format!("A point could not be added to the triangles: {:?}", e)))?;
        }
        Ok(())
    };
    // The edge: the corners of the raster, and where the cells with a value begin and end
    // in every few rows and columns, when that is not at the edge of the raster. The rest
    // of the edge is filled in as the inside is.
    let (w, h) = (g.width, g.height);
    for (c, r) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        if valid(c, r) {
            put(&mut tin, c, r)?;
        }
    }
    let step = (w.max(h) / 32).max(1);
    for r in (0..h).step_by(step).chain(std::iter::once(h - 1)) {
        if let (Some(a), Some(b)) = ((0..w).find(|c| valid(*c, r)), (0..w).rev().find(|c| valid(*c, r))) {
            for c in [a, b] {
                if (c != 0 && c != w - 1) || r == 0 || r == h - 1 {
                    put(&mut tin, c, r)?;
                }
            }
        }
    }
    for c in (0..w).step_by(step).chain(std::iter::once(w - 1)) {
        if let (Some(a), Some(b)) = ((0..h).find(|r| valid(c, *r)), (0..h).rev().find(|r| valid(c, *r))) {
            for r in [a, b] {
                if (r != 0 && r != h - 1) || c == 0 || c == w - 1 {
                    put(&mut tin, c, r)?;
                }
            }
        }
    }
    if tin.num_vertices() < 3 {
        return err("The raster has too few cells with a value to make triangles of.");
    }
    let cell = |p: spade::Point2<f64>| [(p.x - gt[0]) / gt[1] - 0.5, (p.y - gt[3]) / gt[5] - 0.5];
    loop {
        let room = most.saturating_sub(tin.num_vertices());
        if room == 0 {
            break;
        }
        if !going(tin.num_vertices()) {
            return err(crate::model::STOPPED);
        }
        let tris = triangles_of(&tin);
        let mut worst: Vec<(f64, usize, usize)> = tris
            .par_iter()
            .filter_map(|t| {
                let q = t.map(|k| {
                    let c = cell(k.pos);
                    [c[0], c[1], k.z]
                });
                worst_cell(g, &q).filter(|w| w.0 > tolerance)
            })
            .collect();
        if worst.is_empty() {
            break;
        }
        worst.sort_by(|a, b| b.0.total_cmp(&a.0));
        worst.truncate(room);
        for (_, c, r) in worst {
            put(&mut tin, c, r)?;
        }
    }
    Ok(tin)
}

/// A TIN of points with heights: every point is a corner.
fn tin_of_points(table: &crate::model::Table) -> Result<(Tin, usize)> {
    use spade::Triangulation;
    let mut tin = Tin::default();
    let mut without = 0usize;
    let mut add = |x: f64, y: f64, z: f64| -> Result<()> {
        tin.insert(Corner { pos: spade::Point2::new(x, y), z })
            .map(|_| ())
            .map_err(|e| GbError(format!("A point could not be added to the triangles: {:?}", e)))
    };
    for f in &table.features {
        let Some(g) = &f.geom else { continue };
        let pts: Vec<geo::Coord<f64>> = match g {
            Geometry::Point(p) => vec![p.0],
            Geometry::MultiPoint(m) => m.0.iter().map(|p| p.0).collect(),
            _ => continue,
        };
        match f.heights() {
            Some(z) => {
                for (c, z) in pts.iter().zip(z) {
                    add(c.x, c.y, *z)?;
                }
            }
            None => without += pts.len(),
        }
    }
    Ok((tin, without))
}

fn run_tin(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Heights");
    let three = p.get("kind").as_deref() != Some("2d");
    let (tin, crs, grid) = match table.grid.as_deref() {
        Some(g) => {
            let tolerance = number_param(p, "tolerance", "The tolerance")?.unwrap_or(1.0).max(0.0);
            let most = number_param(p, "most", "The most corners")?.unwrap_or(50_000.0).max(3.0) as usize;
            let (progress, gate) = (ctx.progress, ctx.gate);
            let going = |n: usize| {
                progress("corners", n as u64, Some(most as u64));
                gate.going()
            };
            let tin = tin_of_grid(g, tolerance, most, &going)?;
            (tin, g.crs.or(table.crs), Some(g))
        }
        None => {
            let (tin, without) = tin_of_points(table)?;
            if without > 0 {
                ctx.notes.warn(format!("{} points have no height and were left out. Give them heights with Set height.", fmt_count(without as u64)));
            }
            (tin, table.crs, None)
        }
    };
    let mut tris = triangles_of(&tin);
    // Triangles over cells without a value, as beside a lake or outside the edge of the
    // data, are left out.
    if let Some(g) = grid {
        tris.retain(|t| {
            let (x, y) = ((t[0].pos.x + t[1].pos.x + t[2].pos.x) / 3.0, (t[0].pos.y + t[1].pos.y + t[2].pos.y) / 3.0);
            g.cell_at(x, y).is_some_and(|(_, _, v)| v.is_some_and(|v| !v.is_nan()))
        });
    }
    if tris.is_empty() {
        return err("No triangles could be made. A TIN needs a raster of heights read with Cells as a grid of heights, or at least three points with heights.");
    }
    let fields = vec![
        Field::new("z_min", FieldType::Real),
        Field::new("z_max", FieldType::Real),
        Field::new("z_mean", FieldType::Real),
        Field::new("slope", FieldType::Real),
        Field::new("aspect", FieldType::Real),
    ];
    let round = |v: f64, k: f64| (v * k).round() / k;
    let mut features = Vec::with_capacity(tris.len());
    let mut mesh = Mesh::default();
    let colour = colour_param(p, "colour", [0.56, 0.61, 0.45])?;
    let mut index: std::collections::HashMap<(u64, u64), u32> = std::collections::HashMap::new();
    for (k, t) in tris.iter().enumerate() {
        let [a, b, c] = *t;
        let z = [a.z, b.z, c.z];
        let (zlo, zhi) = (z.iter().copied().fold(f64::INFINITY, f64::min), z.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        // The normal, for the slope and which way it faces.
        let (u, v) = ([b.pos.x - a.pos.x, b.pos.y - a.pos.y, b.z - a.z], [c.pos.x - a.pos.x, c.pos.y - a.pos.y, c.z - a.z]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let flat_len = (n[0] * n[0] + n[1] * n[1]).sqrt();
        let slope = flat_len.atan2(n[2].abs()).to_degrees();
        let aspect = if flat_len < 1e-12 { Value::Null } else { Value::Real(round((n[0].atan2(n[1]).to_degrees() + 360.0) % 360.0, 10.0)) };
        let ring: Vec<geo::Coord<f64>> = [a, b, c, a].iter().map(|q| geo::coord! { x: q.pos.x, y: q.pos.y }).collect();
        let geom = Geometry::Polygon(geo::Polygon::new(geo::LineString::new(ring), Vec::new()));
        features.push(crate::model::Feature {
            attrs: vec![
                Value::Real(round(zlo, 1000.0)),
                Value::Real(round(zhi, 1000.0)),
                Value::Real(round((z[0] + z[1] + z[2]) / 3.0, 1000.0)),
                Value::Real(round(slope, 10.0)),
                aspect,
            ],
            geom: Some(geom),
            z: three.then(|| Arc::from(vec![a.z, b.z, c.z, a.z])),
        });
        if three {
            let v = [a, b, c].map(|q| {
                *index.entry((q.pos.x.to_bits(), q.pos.y.to_bits())).or_insert_with(|| {
                    mesh.positions.push([q.pos.x, q.pos.y, q.z]);
                    (mesh.positions.len() - 1) as u32
                })
            });
            mesh.tris.push(Tri { v, n: [NONE; 3], t: [NONE; 3], material: 0, object: k as u32 });
            mesh.objects.push((k + 1).to_string());
        }
    }
    let corners = tris.iter().flatten().map(|q| (q.pos.x.to_bits(), q.pos.y.to_bits())).collect::<std::collections::HashSet<_>>().len();
    ctx.notes.info(match grid {
        Some(g) => format!(
            "{} triangles on {} corners, from {} cells",
            fmt_count(tris.len() as u64),
            fmt_count(corners as u64),
            fmt_count(g.values.iter().filter(|v| !v.is_nan()).count() as u64)
        ),
        None => format!("{} triangles on {} points", fmt_count(tris.len() as u64), fmt_count(corners as u64)),
    });
    let mut out = crate::model::Table { fields, crs, features, ..Default::default() };
    if three {
        mesh.materials.push(Material { name: "terrain".into(), diffuse: colour, ..Default::default() });
        out.mesh = Some(Arc::new(mesh));
    }
    Ok(vec![("TIN".into(), out)])
}

// ---------- TIN to raster (TinRasterizer) ----------

const TIN_RASTER_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "cell_size",
    label: "Cell size in metres",
    kind: ParamKind::Text,
    default: "",
    hint: "Empty: a size that gives about 1000 cells across the TIN",
}];

/// The triangles of a TIN: of its 3D model, or of the areas with a height at each corner.
fn tin_triangles(t: &crate::model::Table) -> Vec<[[f64; 3]; 3]> {
    if let Some(m) = &t.mesh {
        return m.tris.iter().map(|tri| tri.v.map(|i| m.positions[i as usize])).collect();
    }
    let mut out = Vec::new();
    for f in &t.features {
        let (Some(g), Some(z)) = (&f.geom, f.heights()) else { continue };
        // The coordinates in the order the heights have them: each ring of each polygon.
        let mut k = 0usize;
        for poly in crate::geom::polygons_of(g) {
            let ext = &poly.exterior().0;
            let pts: Vec<[f64; 3]> = ext.iter().enumerate().map(|(i, c)| [c.x, c.y, z.get(k + i).copied().unwrap_or(f64::NAN)]).collect();
            k += ext.len() + poly.interiors().iter().map(|r| r.0.len()).sum::<usize>();
            let open = if pts.len() > 1 && pts[0][..2] == pts[pts.len() - 1][..2] { &pts[..pts.len() - 1] } else { &pts[..] };
            for t in crate::mesh::triangulate(open) {
                out.push(t.map(|i| open[i]));
            }
        }
    }
    out.retain(|t| t.iter().all(|p| p.iter().all(|v| v.is_finite())));
    out
}

fn run_tin_raster(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "TIN");
    let tris = tin_triangles(table);
    if tris.is_empty() {
        return err("No triangles with heights came in. Connect a TIN in 3D from Generate TIN, or another 3D model, to TIN.");
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for t in &tris {
        for q in t {
            for d in 0..2 {
                lo[d] = lo[d].min(q[d]);
                hi[d] = hi[d].max(q[d]);
            }
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let size = match number_param(p, "cell_size", "The cell size")? {
        Some(c) if c <= 0.0 => return err("The cell size must be more than 0."),
        Some(c) => c,
        None => {
            // A round number near a thousandth of the width.
            let raw = (span / 1000.0).max(1e-9);
            let mag = 10f64.powf(raw.log10().floor());
            [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|v| *v >= raw).unwrap_or(10.0 * mag)
        }
    };
    let left = (lo[0] / size).floor() * size;
    let top = (hi[1] / size).ceil() * size;
    let width = (((hi[0] - left) / size).ceil() as usize).max(1);
    let height = (((top - lo[1]) / size).ceil() as usize).max(1);
    if width.saturating_mul(height) > 200_000_000 {
        return err(format!("That would be {} × {} cells. Take a larger cell size.", width, height));
    }
    let mut values = vec![f32::NAN; width * height];
    for (k, t) in tris.iter().enumerate() {
        let [a, b, c] = *t;
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-12 {
            continue;
        }
        let (x0, x1) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
        let (y0, y1) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
        let c0 = ((x0 - left) / size - 0.5).ceil().max(0.0) as i64;
        let c1 = ((x1 - left) / size - 0.5).floor().min(width as f64 - 1.0) as i64;
        let r0 = ((top - y1) / size - 0.5).ceil().max(0.0) as i64;
        let r1 = ((top - y0) / size - 0.5).floor().min(height as f64 - 1.0) as i64;
        let eps = -1e-9;
        for r in r0..=r1 {
            let y = top - (r as f64 + 0.5) * size;
            for col in c0..=c1 {
                let x = left + (col as f64 + 0.5) * size;
                let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
                let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
                let l3 = 1.0 - l1 - l2;
                if l1 < eps || l2 < eps || l3 < eps {
                    continue;
                }
                let z = (l1 * a[2] + l2 * b[2] + l3 * c[2]) as f32;
                // Where triangles lie over each other, as roofs over floors, the highest counts.
                let v = &mut values[r as usize * width + col as usize];
                if v.is_nan() || z > *v {
                    *v = z;
                }
            }
        }
        if k % 4096 == 0 && !ctx.gate.going() {
            return err(crate::model::STOPPED);
        }
    }
    let filled = values.iter().filter(|v| !v.is_nan()).count();
    ctx.notes.info(format!(
        "{} triangles on {} × {} cells of {} m, {} with a height",
        fmt_count(tris.len() as u64),
        width,
        height,
        crate::model::fmt_real(size),
        fmt_count(filled as u64)
    ));
    let grid = Grid { width, height, values, geotransform: [left, size, 0.0, top, 0.0, -size], crs: table.crs, bands: Vec::new() };
    Ok(vec![("Raster".into(), grid.into_table())])
}

// ---------- Raise points (PointRaiser) ----------

const RAISE_BASES: &[(&str, &str)] = &[("zero", "At 0"), ("terrain", "On the terrain model on Terrain"), ("own", "At the heights the points have")];

const RAISE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "height",
        label: "Height",
        kind: ParamKind::Expression("Points"),
        default: "100",
        hint: "How high above the foot: a number, an attribute such as population, or an expression such as population / 100",
    },
    ParamSpec { key: "base", label: "Foot", kind: ParamKind::Choice(RAISE_BASES), default: "zero", hint: "Where each stick starts" },
    ParamSpec { key: "ball_colour", label: "Colour of the balls", kind: ParamKind::Text, default: "#e0533d", hint: "" },
    ParamSpec { key: "stick_colour", label: "Colour of the sticks", kind: ParamKind::Text, default: "#5a5a5a", hint: "" },
    ParamSpec {
        key: "ball_size",
        label: "Size of the balls in metres",
        kind: ParamKind::Text,
        default: "",
        hint: "Across. Empty: a twentieth of the highest stick. 0: no balls",
    },
];

fn run_raise(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Points");
    let names = table.field_names();
    let height = expr::compile(&p.need("height", "Write the height: a number, or an attribute such as population.")?, &names)
        .map_err(|e| GbError(format!("The height: {}", e.0)))?;
    let base = p.get("base").unwrap_or_else(|| "zero".into());
    let terrain = if base == "terrain" {
        match terrain_in(inp, table.crs, ctx)? {
            Some(g) => Some(g),
            None => return err("Connect a terrain model to Terrain: an input that reads it with Cells as a grid of heights."),
        }
    } else {
        None
    };
    let geographic = ctx.geographic(table.crs);
    let mut tops = table.like();
    for n in ["z_base", "z_top"] {
        if tops.field_index(n).is_none() {
            tops.fields.push(Field::new(n, FieldType::Real));
        }
    }
    let (zb, zt) = (tops.field_index("z_base").unwrap(), tops.field_index("z_top").unwrap());
    let mut sticks = tops.like();
    let (mut not_points, mut no_height, mut outside) = (0usize, 0usize, 0usize);
    let mut pairs: Vec<([f64; 3], f64)> = Vec::new();
    for (i, f) in table.features.iter().enumerate() {
        let pts: Vec<geo::Coord<f64>> = match &f.geom {
            Some(Geometry::Point(q)) => vec![q.0],
            Some(Geometry::MultiPoint(m)) => m.0.iter().map(|q| q.0).collect(),
            Some(_) => {
                not_points += 1;
                continue;
            }
            None => continue,
        };
        let Some(h) = number_of(&height, f, i, geographic)? else {
            no_height += 1;
            continue;
        };
        let own = f.heights();
        for (k, c) in pts.iter().enumerate() {
            let z0 = match base.as_str() {
                "terrain" => match crate::terrain::height_at(terrain.as_ref().expect("a terrain"), c.x, c.y) {
                    Some(z) => z,
                    None => {
                        outside += 1;
                        0.0
                    }
                },
                "own" => own.and_then(|z| z.get(k).copied()).unwrap_or(0.0),
                _ => 0.0,
            };
            let z1 = z0 + h;
            let mut attrs = f.attrs.clone();
            attrs.resize(tops.fields.len(), Value::Null);
            attrs[zb] = Value::Real((z0 * 1000.0).round() / 1000.0);
            attrs[zt] = Value::Real((z1 * 1000.0).round() / 1000.0);
            tops.features.push(crate::model::Feature { attrs: attrs.clone(), geom: Some(Geometry::Point((*c).into())), z: Some(Arc::from(vec![z1])) });
            let line = geo::LineString::new(vec![*c, *c]);
            sticks.features.push(crate::model::Feature { attrs, geom: Some(Geometry::LineString(line)), z: Some(Arc::from(vec![z0, z1])) });
            pairs.push(([c.x, c.y, z0], z1));
        }
    }
    if not_points > 0 {
        ctx.notes
            .warn(format!("{} features are not points and were left out. Make points of them with Replace with center point.", fmt_count(not_points as u64)));
    }
    if no_height > 0 {
        ctx.notes.warn(format!("{} points have no height that is a number and were left out.", fmt_count(no_height as u64)));
    }
    if outside > 0 {
        ctx.notes.warn(format!("{} points lie outside the terrain model and stand at 0.", fmt_count(outside as u64)));
    }
    // The sticks as a 3D model: a thin post and a ball on top of each, for the 3D view and
    // Render 3D scene.
    let tallest = pairs.iter().map(|(b, t)| (t - b[2]).abs()).fold(0.0f64, f64::max);
    let ball = match number_param(p, "ball_size", "The size of the balls")? {
        Some(v) if v < 0.0 => return err("The size of the balls must be 0 or more."),
        v => v.unwrap_or(tallest / 20.0),
    };
    let stick = if ball > 0.0 { ball * 0.12 } else { tallest / 200.0 };
    let mut mesh = Mesh::default();
    mesh.materials.push(Material { name: "stick".into(), diffuse: colour_param(p, "stick_colour", [0.35, 0.35, 0.35])?, ..Default::default() });
    mesh.materials.push(Material {
        name: "ball".into(),
        diffuse: colour_param(p, "ball_colour", [0.88, 0.33, 0.24])?,
        specular: [0.3; 3],
        shininess: 60.0,
        ..Default::default()
    });
    for (k, (b, top)) in pairs.iter().enumerate() {
        let mut sh = super::props::Shapes { mesh: &mut mesh, object: k as u32 };
        let (lo, hi) = (b[2].min(*top), b[2].max(*top));
        if hi > lo && stick > 0.0 {
            sh.post([b[0], b[1], lo], stick, hi - lo, 0, 8);
        }
        if ball > 0.0 {
            sh.ball([b[0], b[1], *top], [ball / 2.0; 3], 1, 14, 9);
        }
        mesh.objects.push((k + 1).to_string());
    }
    ctx.notes.info(format!("{} points raised, the highest {} m", fmt_count(pairs.len() as u64), crate::model::fmt_real((tallest * 10.0).round() / 10.0)));
    sticks.mesh = Some(Arc::new(mesh));
    Ok(vec![("Tops".into(), tops), ("Sticks".into(), sticks)])
}

fn tin_summary(p: &P) -> String {
    format!("{} · within {} m", if p.get("kind").as_deref() == Some("2d") { "2D" } else { "3D" }, p.get("tolerance").unwrap_or_else(|| "1".into()))
}

pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "Extruder",
        title: "Extrude",
        hidden: false,
        group: Group::Transformer,
        category: "3D models and views",
        desc: "Raises areas into blocks: a 3D model with walls, a top and a bottom, for Render 3D scene, the 3D view and the 3D files OBJ and glTF. The height is a number, such as 20, or comes from an attribute or an expression; the blocks stand at a height you give, at the heights the areas have, or on the terrain on Terrain. The areas come out with their attributes and z_base, z_top and height.",
        inputs: &["Input", "Terrain"],
        outputs: &["Output"],
        ports: None,
        params: EXTRUDE_PARAMS,
        run: run_extrude,
        summary: extrude_summary,
    },
    NodeType {
        name: "TinGenerator",
        title: "Generate TIN",
        hidden: false,
        group: Group::Transformer,
        category: "3D models and views",
        desc: "A triangulated irregular network: triangles that follow the heights of a raster on Heights, read with Cells as a grid of heights, with many where the terrain is rough and few where it is even, or that join points with heights, such as from a point cloud. In 3D the triangles carry their heights and make a 3D model, for Render 3D scene, the 3D view and 3D files; in 2D they are flat areas with their heights, slope and aspect.",
        inputs: &["Heights"],
        outputs: &["TIN"],
        ports: None,
        params: TIN_PARAMS,
        run: run_tin,
        summary: tin_summary,
    },
    NodeType {
        name: "TinRasterizer",
        title: "TIN to raster",
        hidden: false,
        group: Group::Transformer,
        category: "3D models and views",
        desc: "A raster of heights from a TIN: the height of the triangles at the middle of each cell, between their corners. The TIN is one in 3D from Generate TIN, or any 3D model, where the highest surface counts where they lie over each other, as roofs over floors. Cells outside the triangles have no value. Write it as GeoTIFF, or use it for Shade terrain, contours and Render 3D scene.",
        inputs: &["TIN"],
        outputs: &["Raster"],
        ports: None,
        params: TIN_RASTER_PARAMS,
        run: run_tin_raster,
        summary: |p| p.get("cell_size").map(|c| format!("cells of {} m", c)).unwrap_or_else(|| "cells to suit".into()),
    },
    NodeType {
        name: "PointRaiser",
        title: "Raise points",
        hidden: false,
        group: Group::Transformer,
        category: "3D models and views",
        desc: "Raises points to a height, such as from an attribute, with a stick from where they stand up to the raised point: lollipops for a 3D map of places, as the number of inhabitants of each town or the height of each tree. The sticks start at 0, on the terrain on Terrain, or at the heights the points have. Tops gives the raised points; Sticks the vertical lines, with a 3D model of a post and a ball on each for the 3D view and Render 3D scene.",
        inputs: &["Points", "Terrain"],
        outputs: &["Tops", "Sticks"],
        ports: None,
        params: RAISE_PARAMS,
        run: run_raise,
        summary: |p| format!("{} m up", p.get("height").unwrap_or_default()),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Table;

    fn run(kind: &str, params: &[(&str, &str)], inputs: Vec<(&'static str, Table)>) -> Table {
        let (mut out, _) = run_for_test(kind, params, inputs).unwrap();
        out.remove(0).1
    }

    fn square(x: f64, size: f64, floors: i64) -> crate::model::Feature {
        let ring = vec![(x, 0.0), (x + size, 0.0), (x + size, size), (x, size), (x, 0.0)];
        crate::model::Feature::new(vec![Value::Int(floors)], Some(Geometry::Polygon(geo::Polygon::new(ring.into(), vec![]))))
    }

    #[test]
    fn a_tin_becomes_a_raster_of_its_heights_and_a_block_its_top() {
        // A slope, z = x / 10, on a grid of 50 × 40 cells of 2 m.
        let (w, h) = (50usize, 40usize);
        let values: Vec<f32> = (0..w * h).map(|i| ((i % w) as f32 * 2.0 + 1.0) / 10.0).collect();
        let g = Grid { width: w, height: h, values, geotransform: [0.0, 2.0, 0.0, 80.0, 0.0, -2.0], crs: Some(3006), bands: Vec::new() };
        let tin = run("TinGenerator", &[("tolerance", "0.01")], vec![("Heights", Table { grid: Some(Arc::new(g)), crs: Some(3006), ..Default::default() })]);
        let r = run("TinRasterizer", &[("cell_size", "4")], vec![("TIN", tin.clone())]);
        let grid = r.grid.as_ref().unwrap();
        assert_eq!(grid.geotransform[1], 4.0);
        let mut seen = 0;
        for row in 0..grid.height {
            for col in 0..grid.width {
                if let Some(z) = grid.get(col, row) {
                    let x = grid.geotransform[0] + (col as f64 + 0.5) * 4.0;
                    assert!((z as f64 - x / 10.0).abs() < 0.02, "{} at {}", z, x);
                    seen += 1;
                }
            }
        }
        assert!(seen > grid.width * grid.height * 8 / 10, "{} of {}", seen, grid.width * grid.height);
        // Without a cell size, a round one near a thousandth of the width.
        let auto = run("TinRasterizer", &[], vec![("TIN", tin)]);
        assert!(auto.grid.as_ref().unwrap().geotransform[1] == 0.1, "{:?}", auto.grid.as_ref().unwrap().geotransform);
        // A block: the raster has its top where the block stands, and nothing beside it.
        let mut t = Table { fields: vec![Field::new("floors", FieldType::Int)], crs: Some(3006), ..Default::default() };
        t.features = vec![square(0.0, 10.0, 2)];
        let block = run("Extruder", &[("height", "floors * 3")], vec![("Input", t)]);
        let r = run("TinRasterizer", &[("cell_size", "1")], vec![("TIN", block)]);
        let grid = r.grid.as_ref().unwrap();
        assert_eq!((grid.width, grid.height), (10, 10));
        assert!(grid.values.iter().all(|v| *v == 6.0), "{:?}", &grid.values[..10]);
        // Nothing with heights is said plainly.
        let e = run_for_test("TinRasterizer", &[], vec![("TIN", Table::default())]).unwrap_err();
        assert!(e.0.contains("No triangles"), "{}", e.0);
    }

    #[test]
    fn extrude_makes_closed_blocks_of_the_height_given() {
        let mut t = Table { fields: vec![Field::new("floors", FieldType::Int)], crs: Some(3006), ..Default::default() };
        t.features = vec![square(0.0, 10.0, 2), square(20.0, 5.0, 4)];
        let out = run("Extruder", &[("height", "floors * 3")], vec![("Input", t)]);
        let m = out.mesh.as_ref().unwrap();
        // Four walls of two triangles, and two triangles each for the top and the bottom.
        assert_eq!(m.tris.len(), 2 * (8 + 4));
        assert_eq!(m.objects, vec!["1", "2"]);
        let (lo, hi) = m.bounds().unwrap();
        assert_eq!((lo[2], hi[2]), (0.0, 12.0));
        let h = out.field_index("height").unwrap();
        assert_eq!(out.features[0].attrs[h], Value::Real(6.0));
        // Every wall faces out: its normal points away from the middle of the block.
        for t in m.tris.iter().filter(|t| t.object == 0 && t.material == 0) {
            let n = m.normals[t.n[0] as usize];
            let p = m.positions[t.v[0] as usize];
            let q = m.positions[t.v[1] as usize];
            let mid = [(p[0] + q[0]) / 2.0 - 5.0, (p[1] + q[1]) / 2.0 - 5.0];
            assert!(n[0] as f64 * mid[0] + n[1] as f64 * mid[1] > 0.0);
        }
        // The triangles of the top face up, by the order of their corners.
        for t in m.tris.iter().filter(|t| t.material == 1 && m.positions[t.v[0] as usize][2] > 1.0) {
            let [a, b, c] = t.v.map(|i| m.positions[i as usize]);
            assert!((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0);
        }
    }

    #[test]
    fn extrude_with_a_hole_and_an_absolute_top() {
        let outer: geo::LineString<f64> = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)].into();
        let hole: geo::LineString<f64> = vec![(3.0, 3.0), (3.0, 7.0), (7.0, 7.0), (7.0, 3.0), (3.0, 3.0)].into();
        let mut t = Table { fields: vec![], crs: Some(3006), ..Default::default() };
        t.features = vec![crate::model::Feature::new(vec![], Some(Geometry::Polygon(geo::Polygon::new(outer, vec![hole]))))];
        let out = run("Extruder", &[("height", "30"), ("top", "absolute"), ("base_value", "10")], vec![("Input", t)]);
        let m = out.mesh.as_ref().unwrap();
        // Eight walls, and the ring between the hole and the outside, top and bottom.
        assert_eq!(m.tris.iter().filter(|t| t.material == 0).count(), 16);
        let top: f64 = m
            .tris
            .iter()
            .filter(|t| t.material == 1)
            .map(|t| {
                let [a, b, c] = t.v.map(|i| m.positions[i as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum();
        assert!((top - 2.0 * (100.0 - 16.0)).abs() < 1e-9);
        assert_eq!(m.bounds().unwrap().0[2], 10.0);
        assert_eq!(m.bounds().unwrap().1[2], 30.0);
    }

    fn hill(n: usize) -> Grid {
        let mut values = Vec::with_capacity(n * n);
        for r in 0..n {
            for c in 0..n {
                let (x, y) = (c as f64 - n as f64 / 2.0, r as f64 - n as f64 / 2.0);
                values.push((50.0 * (-(x * x + y * y) / (n as f64 * 3.0)).exp()) as f32);
            }
        }
        Grid { width: n, height: n, values, geotransform: [1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0], crs: Some(3006), bands: vec![] }
    }

    #[test]
    fn a_tin_follows_the_raster_within_the_tolerance() {
        let g = hill(80);
        let t = Table { grid: Some(Arc::new(g.clone())), crs: Some(3006), ..Default::default() };
        let out = run("TinGenerator", &[("tolerance", "0.5")], vec![("Heights", t)]);
        let m = out.mesh.as_ref().unwrap();
        assert!(m.positions.len() < 80 * 80 / 4, "{} corners", m.positions.len());
        // Every cell lies within the tolerance of the triangles.
        for t in &out.features {
            let z = t.z.as_ref().unwrap();
            let Some(Geometry::Polygon(p)) = &t.geom else { panic!() };
            let c = &p.exterior().0;
            let q = [0, 1, 2].map(|k| {
                let (col, row) = ((c[k].x - 1000.0) / 2.0 - 0.5, (c[k].y - 2000.0) / -2.0 - 0.5);
                [col, row, z[k]]
            });
            if let Some((d, _, _)) = worst_cell(&g, &q) {
                assert!(d <= 0.5 + 1e-6, "{}", d);
            }
        }
        // The triangles cover the cells: the area of the middles of the cells.
        let area: f64 = out.features.iter().map(|f| geo::Area::unsigned_area(f.geom.as_ref().unwrap())).sum();
        assert!((area - (79.0 * 2.0) * (79.0 * 2.0)).abs() < 1e-6, "{}", area);
        let flat = run("TinGenerator", &[("kind", "2d"), ("tolerance", "0.5")], vec![("Heights", Table { grid: Some(Arc::new(g)), ..Default::default() })]);
        assert!(flat.mesh.is_none() && flat.features[0].z.is_none());
        assert_eq!(flat.features.len(), out.features.len());
        assert_eq!(flat.crs, Some(3006));
    }

    #[test]
    fn a_tin_leaves_out_the_cells_without_a_value_and_joins_points() {
        let mut g = hill(40);
        for r in 0..40 {
            for c in 0..40 {
                if c < 10 || (r > 30 && c > 30) {
                    g.values[r * 40 + c] = f32::NAN;
                }
            }
        }
        let out = run("TinGenerator", &[("tolerance", "0.5")], vec![("Heights", Table { grid: Some(Arc::new(g.clone())), ..Default::default() })]);
        for f in &out.features {
            let c = geo::Centroid::centroid(f.geom.as_ref().unwrap()).unwrap();
            assert!(g.cell_at(c.x(), c.y()).unwrap().2.is_some_and(|v| !v.is_nan()));
        }
        let pts: Vec<crate::model::Feature> = [(0.0, 0.0, 1.0), (10.0, 0.0, 2.0), (0.0, 10.0, 3.0), (10.0, 10.0, 4.0)]
            .iter()
            .map(|(x, y, z)| crate::model::Feature { attrs: vec![], geom: Some(Geometry::Point(geo::Point::new(*x, *y))), z: Some(Arc::from(vec![*z])) })
            .collect();
        let out = run("TinGenerator", &[], vec![("Heights", Table { features: pts, crs: Some(3006), ..Default::default() })]);
        assert_eq!(out.features.len(), 2);
        assert_eq!(out.mesh.as_ref().unwrap().positions.len(), 4);
    }
}
