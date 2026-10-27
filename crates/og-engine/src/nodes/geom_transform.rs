//! Tools that move geometry or change its coordinate system: Reproject, Set coordinate
//! system, Geometry to 2D, Scale, Move by x, y and z, Rotate, Reverse lines, Swap x and y and
//! Set height.

use super::*;
use crate::expr::{self, Row};
use crate::geom;
use crate::model::{err, fmt_count, GbError, Result, Value};
use geo::{BoundingRect, Geometry, Rotate};
use std::sync::Arc;

// ---------- Reproject (Reprojector) ----------

pub(super) const REPROJECTOR_PARAMS: &[ParamSpec] =
    &[ParamSpec { key: "crs", label: "To coordinate system", kind: ParamKind::Crs, default: "EPSG:3006", hint: "" }];

pub(super) fn run_reprojector(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let to = p.crs("crs")?.ok_or_else(|| GbError("Choose the coordinate system to reproject to.".into()))?;
    Ok(vec![("Reprojected".into(), reproject(table, to, ctx.backend)?)])
}

// ---------- Set coordinate system (CrsSetter) ----------

pub(super) const CRS_SETTER_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "crs",
    label: "Coordinate system",
    kind: ParamKind::Crs,
    default: "",
    hint: "As an EPSG code, for example EPSG:3006 for SWEREF 99 TM. The coordinates are not changed: use Reproject for that",
}];

pub(super) fn run_crs_setter(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let Some(crs) = p.crs("crs")? else {
        return err("Choose the coordinate system to set, for example EPSG:3006.");
    };
    if let Some(was) = table.crs.filter(|c| *c != crs) {
        ctx.notes.warn(format!(
            "The stream was in {}. It is now said to be in {}, with the same coordinates. Use Reproject to move the coordinates.",
            crs_label(Some(was)),
            crs_label(Some(crs))
        ));
    }
    let mut out = table.clone();
    out.crs = Some(crs);
    Ok(vec![("Output".into(), out)])
}

// ---------- Geometry to 2D (Force2D), Scale (Scaler), Move by x, y and z (Offsetter) ----------

pub(super) fn run_force2d(_p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    // The heights of the coordinates go, and the 3D model that goes with the stream, such as
    // the one of an IFC file: what is left is the plan of it.
    let mut out = table.clone();
    let with = out.features.iter().filter(|f| f.z.is_some()).count();
    for f in out.features.iter_mut() {
        f.z = None;
    }
    let model = out.mesh.take().is_some();
    ctx.notes.info(match (with, model) {
        (0, false) => "the geometry was 2D already".to_string(),
        (n, true) => format!("{} features made flat, and the 3D model left out", fmt_count(n as u64)),
        (n, false) => format!("{} features made flat", fmt_count(n as u64)),
    });
    Ok(vec![("Output".into(), out)])
}

/// A number from a setting, with a comma or a point for decimals.
pub(super) fn number_param(p: &P, key: &str, what: &str) -> Result<Option<f64>> {
    let Some(text) = p.get(key).filter(|t| !t.trim().is_empty()) else { return Ok(None) };
    let clean: String = text.trim().replace([' ', '\u{a0}'], "").replace(',', ".");
    clean.parse::<f64>().ok().filter(|x| x.is_finite()).map(Some).ok_or_else(|| GbError(format!("{} must be a number, not \"{}\".", what, text.trim())))
}

pub(super) const SCALER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "factor",
        label: "Factor",
        kind: ParamKind::Text,
        default: "1000",
        hint: "1000 makes the geometry a thousand times larger. 0.001 turns millimetres into metres",
    },
    ParamSpec { key: "factor_y", label: "Factor in y", kind: ParamKind::Text, default: "", hint: "Empty is the same as the factor" },
    ParamSpec {
        key: "around",
        label: "Around",
        kind: ParamKind::Choice(&[
            ("origin", "The origin, 0 0 in the coordinate system"),
            ("each", "The middle of each feature"),
            ("all", "The middle of all features together"),
        ]),
        default: "origin",
        hint: "The point that stays where it is",
    },
    ParamSpec {
        key: "heights",
        label: "Heights",
        kind: ParamKind::Choice(&[("scale", "Scaled by the factor too"), ("keep", "Kept as they are")]),
        default: "scale",
        hint:
            "For data with Z and 3D models. Scaled from 0, or from the lowest point when scaled around a middle, so that what stands on the ground stays there",
    },
];

/// Scales the corners of a 3D model as Scale does its features: x and y around a point, and
/// heights, when `fz` is given, from a base. Around the middle of each object, each is
/// scaled by itself.
pub(super) fn scale_mesh(m: &crate::mesh::Mesh, fx: f64, fy: f64, fz: Option<f64>, around: &str, all: Option<geo::Coord<f64>>) -> crate::mesh::Mesh {
    let mut out = m.clone();
    let uniform = fx == fy && fz.is_none_or(|z| z == fx);
    if !uniform {
        // Normals do not follow a stretch; the faces' own are used instead.
        for t in &mut out.tris {
            t.n = [crate::mesh::NONE; 3];
        }
        out.normals.clear();
    }
    let lowest = |ps: &mut dyn Iterator<Item = [f64; 3]>| ps.fold(f64::INFINITY, |a, p| a.min(p[2]));
    match around {
        "each" => {
            // Corners shared by objects are made one for each, so that each can move alone.
            let objects = m.tris.iter().map(|t| t.object as usize + 1).max().unwrap_or(0);
            let mut lo = vec![[f64::INFINITY; 3]; objects];
            let mut hi = vec![[f64::NEG_INFINITY; 3]; objects];
            for t in &m.tris {
                for &v in &t.v {
                    let p = m.positions[v as usize];
                    for k in 0..3 {
                        lo[t.object as usize][k] = lo[t.object as usize][k].min(p[k]);
                        hi[t.object as usize][k] = hi[t.object as usize][k].max(p[k]);
                    }
                }
            }
            let mut index: HashMap<(u32, u32), u32> = HashMap::new();
            out.positions.clear();
            for t in &mut out.tris {
                let o = t.object as usize;
                let (cx, cy, base) = ((lo[o][0] + hi[o][0]) / 2.0, (lo[o][1] + hi[o][1]) / 2.0, lo[o][2]);
                for v in &mut t.v {
                    let key = (*v, t.object);
                    *v = *index.entry(key).or_insert_with(|| {
                        let p = m.positions[key.0 as usize];
                        out.positions.push([cx + (p[0] - cx) * fx, cy + (p[1] - cy) * fy, fz.map_or(p[2], |f| base + (p[2] - base) * f)]);
                        (out.positions.len() - 1) as u32
                    });
                }
            }
        }
        _ => {
            let (c, base) = match (around, all) {
                ("all", Some(c)) => (c, lowest(&mut m.positions.iter().copied())),
                _ => (geo::coord! { x: 0.0, y: 0.0 }, 0.0),
            };
            for p in &mut out.positions {
                *p = [c.x + (p[0] - c.x) * fx, c.y + (p[1] - c.y) * fy, fz.map_or(p[2], |f| base + (p[2] - base) * f)];
            }
        }
    }
    out
}

pub(super) fn run_scaler(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::{BoundingRect, Scale};
    let table = input(inp, "Input");
    let Some(fx) = number_param(p, "factor", "The factor")? else {
        return err("Write the factor, for example 1000, or 0.001 to turn millimetres into metres.");
    };
    let fy = number_param(p, "factor_y", "The factor in y")?.unwrap_or(fx);
    if fx == 0.0 || fy == 0.0 {
        return err("A factor of 0 would make every geometry a point. Write another factor.");
    }
    if table.image.is_some() {
        return err("Scale works on features, not on an image. Scale the data before Draw map.");
    }
    let around = p.get("around").unwrap_or_else(|| "origin".into());
    let middle = |r: geo::Rect<f64>| r.center();
    let all = if around == "all" {
        let mut bounds: Option<geo::Rect<f64>> = None;
        for g in table.features.iter().filter_map(|f| f.geom.as_ref()) {
            if let Some(r) = g.bounding_rect() {
                bounds = Some(match bounds {
                    None => r,
                    Some(b) => geo::Rect::new(
                        geo::coord! { x: b.min().x.min(r.min().x), y: b.min().y.min(r.min().y) },
                        geo::coord! { x: b.max().x.max(r.max().x), y: b.max().y.max(r.max().y) },
                    ),
                });
            }
        }
        bounds.map(middle)
    } else {
        None
    };
    // Heights are scaled too, unless the factors differ in x and y or they are to be kept.
    let fz = (p.get("heights").as_deref() != Some("keep") && fx == fy).then_some(fx);
    let all_base = table.features.iter().filter_map(|f| f.heights()).flat_map(|h| h.iter().copied()).fold(f64::INFINITY, f64::min);
    let mut out = table.clone();
    for f in &mut out.features {
        let heights = f.heights().map(|h| h.to_vec());
        if let Some(g) = f.geom.as_mut() {
            let origin = match around.as_str() {
                "each" => g.bounding_rect().map(middle),
                "all" => all,
                _ => Some(geo::coord! { x: 0.0, y: 0.0 }),
            };
            if let Some(o) = origin {
                g.scale_around_point_mut(fx, fy, o);
            }
        }
        if let (Some(k), Some(h)) = (fz, heights) {
            let base = match around.as_str() {
                "each" => h.iter().copied().fold(f64::INFINITY, f64::min),
                "all" => all_base,
                _ => 0.0,
            };
            f.z = Some(h.iter().map(|z| base + (z - base) * k).collect());
        }
    }
    // A 3D model is scaled with its objects, so that the 3D view and Render 3D scene show it
    // as the map does.
    if let Some(m) = &table.mesh {
        out.mesh = Some(Arc::new(scale_mesh(m, fx, fy, fz, &around, all)));
    }
    ctx.notes.info(if fx == fy {
        format!("scaled {} times", fmt_number(fx))
    } else {
        format!("scaled {} times in x and {} in y", fmt_number(fx), fmt_number(fy))
    });
    Ok(vec![("Output".into(), out)])
}

/// A number written without trailing zeros, and with a space between the thousands.
pub(super) fn fmt_number(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        let n = x.abs() as u64;
        format!("{}{}", if x < 0.0 { "-" } else { "" }, fmt_count(n))
    } else {
        let t = format!("{:.10}", x);
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub(super) const OFFSETTER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "dx",
        label: "Along x",
        kind: ParamKind::Text,
        default: "0",
        hint: "In the units of the stream's coordinate system, such as metres in SWEREF 99 TM. Negative moves west",
    },
    ParamSpec { key: "dy", label: "Along y", kind: ParamKind::Text, default: "0", hint: "Negative moves south" },
    ParamSpec {
        key: "dz",
        label: "Up",
        kind: ParamKind::Text,
        default: "0",
        hint: "Moves heights: of features with Z, of 3D models and of a grid of heights. Negative moves down",
    },
];

pub(super) fn run_offsetter(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::Translate;
    let table = input(inp, "Input");
    let dx = number_param(p, "dx", "The distance along x")?.unwrap_or(0.0);
    let dy = number_param(p, "dy", "The distance along y")?.unwrap_or(0.0);
    let dz = number_param(p, "dz", "The distance up")?.unwrap_or(0.0);
    let mut out = table.clone();
    let mut flat = 0usize;
    for f in out.features.iter_mut() {
        if let Some(g) = f.geom.as_mut() {
            g.translate_mut(dx, dy);
        }
        if dz != 0.0 && f.geom.is_some() && !table.raster && table.grid.is_none() {
            match &f.z {
                Some(z) => f.z = Some(z.iter().map(|v| v + dz).collect()),
                None => flat += 1,
            }
        }
    }
    if flat > 0 {
        ctx.notes.warn(format!("{} features have no heights and were not moved up. Give them heights with Set height first.", fmt_count(flat as u64)));
    }
    // A 3D model moves with its objects, so that the 3D view and Render 3D scene show it
    // where the map does.
    if let Some(m) = &table.mesh {
        let mut m = (**m).clone();
        for p in &mut m.positions {
            p[0] += dx;
            p[1] += dy;
            p[2] += dz;
        }
        out.mesh = Some(Arc::new(m));
    }
    // A picture moves with its outline.
    if let Some(img) = &table.image {
        let mut moved = img.as_ref().clone();
        moved.geotransform[0] += dx;
        moved.geotransform[3] += dy;
        out.image = Some(Arc::new(moved));
    }
    if let Some(g) = &table.grid {
        let mut moved = g.as_ref().clone();
        moved.geotransform[0] += dx;
        moved.geotransform[3] += dy;
        if dz != 0.0 {
            moved.values.iter_mut().for_each(|v| *v += dz as f32);
        }
        out.grid = Some(Arc::new(moved));
    }
    let unit = if table.crs.is_some_and(|c| c == 4326 || c == 4258 || c == 4619) { "degrees" } else { "units" };
    ctx.notes.info(if dz != 0.0 {
        format!("moved {} {} along x, {} along y and {} up", fmt_number(dx), unit, fmt_number(dy), fmt_number(dz))
    } else {
        format!("moved {} {} along x and {} along y", fmt_number(dx), unit, fmt_number(dy))
    });
    if table.crs.is_none() {
        ctx.notes.warn("The stream has no coordinate system, so it is not known what the distances are in, and it is not shown on the background map. Set one with Set coordinate system, or in Input.".to_string());
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) const MODES: &[(&str, &str)] = &[
    ("value", "From the value below: all coordinates at that height"),
    ("terrain", "From the terrain model on Terrain, plus the value below"),
    ("add", "The heights they have, plus the value below"),
    ("remove", "Take the heights away: flat again"),
];

pub(super) const HEIGHT_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "mode", label: "Height", kind: ParamKind::Choice(MODES), default: "value", hint: "" },
    ParamSpec {
        key: "height",
        label: "Value",
        kind: ParamKind::Expression("Input"),
        default: "",
        hint: "An attribute, such as the height of a contour line, a number, or an expression such as floors * 3. Empty is 0",
    },
];

pub(super) fn run_height(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let mode = p.get("mode").unwrap_or_else(|| "value".into());
    let mut out = table.clone();
    if mode == "remove" {
        let had = out.features.iter().filter(|f| f.z.is_some()).count();
        out.features.iter_mut().for_each(|f| f.z = None);
        ctx.notes.info(format!("{} features made flat", fmt_count(had as u64)));
        return Ok(vec![("Output".into(), out)]);
    }
    let value = match p.get("height").filter(|v| !v.trim().is_empty()) {
        Some(v) => Some(expr::compile(&v, &table.field_names()).map_err(|e| GbError(format!("The value: {}", e.0)))?),
        None => None,
    };
    if value.is_none() && mode == "value" {
        return err("Write the height: an attribute such as the height of a contour line, or a number.");
    }
    // The terrain, in the coordinate system of the stream.
    let terrain = if mode == "terrain" {
        let t = input(inp, "Terrain");
        let Some(g) = t.grid.as_deref() else {
            return err("Connect a terrain model to Terrain: an input that reads it with Cells as a grid of heights.");
        };
        match (g.crs, table.crs) {
            (Some(a), Some(b)) if a != b => {
                let warped = reproject(&crate::model::Table { grid: Some(Arc::new(g.clone())), crs: Some(a), ..Default::default() }, b, ctx.backend)?;
                warped.grid.map(|g| (*g).clone())
            }
            _ => Some(g.clone()),
        }
    } else {
        None
    };
    let geographic = ctx.geographic(table.crs);
    let (mut outside, mut not_numbers) = (0usize, 0usize);
    for (i, f) in out.features.iter_mut().enumerate() {
        let Some(g) = &f.geom else { continue };
        let coords = crate::geom::coords(g);
        let add = match &value {
            Some(e) => {
                let v = e.eval(&Row { feature: &table.features[i], index: i, geographic }).map_err(|e| at_row(e, i))?;
                match v.as_f64().or_else(|| crate::model::parse_number(&v.to_text())) {
                    Some(x) => x,
                    None => {
                        if !matches!(v, Value::Null) || mode == "value" {
                            not_numbers += 1;
                        }
                        if mode == "value" {
                            continue;
                        }
                        0.0
                    }
                }
            }
            None => 0.0,
        };
        let z: Vec<f64> = match mode.as_str() {
            "terrain" => {
                let grid = terrain.as_ref().expect("a terrain");
                let mut missing = false;
                let z = coords
                    .iter()
                    .map(|c| {
                        crate::terrain::height_at(grid, c.x, c.y).unwrap_or_else(|| {
                            missing = true;
                            0.0
                        }) + add
                    })
                    .collect();
                if missing {
                    outside += 1;
                }
                z
            }
            "add" => match f.heights() {
                Some(h) => h.iter().map(|z| z + add).collect(),
                None => vec![add; coords.len()],
            },
            _ => vec![add; coords.len()],
        };
        f.z = Some(Arc::from(z));
    }
    if not_numbers > 0 {
        ctx.notes.warn(format!("{} features have a value that is not a number, and keep the heights they had.", fmt_count(not_numbers as u64)));
    }
    if outside > 0 {
        ctx.notes.warn(format!("{} features reach outside the terrain model, and are at 0 there.", fmt_count(outside as u64)));
    }
    Ok(vec![("Output".into(), out)])
}

pub(super) const AROUND: &[(&str, &str)] =
    &[("each", "The middle of each feature"), ("all", "The middle of all features together"), ("point", "A point you give")];

pub(super) const ROTATE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "angle",
        label: "Angle, in degrees",
        kind: ParamKind::Expression("Input"),
        default: "90",
        hint: "Counter-clockwise; negative turns clockwise. A number, or an expression such as an attribute with the angle of each feature",
    },
    ParamSpec { key: "around", label: "Around", kind: ParamKind::Choice(AROUND), default: "each", hint: "The point that stays where it is" },
    ParamSpec { key: "x", label: "Point, x", kind: ParamKind::Text, default: "0", hint: "In the coordinate system of the stream" },
    ParamSpec { key: "y", label: "Point, y", kind: ParamKind::Text, default: "0", hint: "" },
];

pub(super) fn middle(r: geo::Rect<f64>) -> geo::Coord<f64> {
    r.center()
}

/// The middle of the bounds of every geometry in the stream.
pub(super) fn middle_of_all<'a>(geoms: impl Iterator<Item = &'a Geometry<f64>>) -> Option<geo::Coord<f64>> {
    let mut bounds: Option<geo::Rect<f64>> = None;
    for r in geoms.filter_map(|g| g.bounding_rect()) {
        bounds = Some(match bounds {
            None => r,
            Some(b) => geo::Rect::new(
                geo::coord! { x: b.min().x.min(r.min().x), y: b.min().y.min(r.min().y) },
                geo::coord! { x: b.max().x.max(r.max().x), y: b.max().y.max(r.max().y) },
            ),
        });
    }
    bounds.map(middle)
}

pub(super) fn run_rotate(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    if table.image.is_some() {
        return Err(GbError("Rotate works on features, not on an image. Rotate the data before Draw map.".into()));
    }
    let text = p.need("angle", "Write the angle in degrees, such as 90.")?;
    let e = expr::compile(&text, &table.field_names())?;
    // The angle is the same for all when it does not depend on the features.
    let fixed: Option<f64> = text.trim().replace(',', ".").parse().ok();
    let around = p.get("around").unwrap_or_else(|| "each".into());
    let point = match around.as_str() {
        "point" => Some(geo::coord! {
            x: number_param(p, "x", "The point's x")?.unwrap_or(0.0),
            y: number_param(p, "y", "The point's y")?.unwrap_or(0.0)
        }),
        "all" => middle_of_all(table.features.iter().filter_map(|f| f.geom.as_ref())),
        _ => None,
    };
    let geographic = ctx.geographic(table.crs);
    let mut out = table.clone();
    let mut no_angle = 0usize;
    for (i, f) in table.features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        let angle = match fixed {
            Some(a) => a,
            None => match e.eval(&Row { feature: f, index: i, geographic }).map_err(|e| super::at_row(e, i))?.as_f64() {
                Some(a) => a,
                None => {
                    no_angle += 1;
                    continue;
                }
            },
        };
        let Some(origin) = point.or_else(|| g.bounding_rect().map(middle)) else { continue };
        out.features[i].geom = Some(g.rotate_around_point(angle, geo::Point::from(origin)));
    }
    if no_angle > 0 {
        ctx.notes.warn(format!("{} features have no angle and were passed on unturned.", fmt_count(no_angle as u64)));
    }
    // A 3D model turns as one, around the middle of it or the point given.
    if let Some(m) = &table.mesh {
        match fixed {
            Some(a) => {
                let mut m = (**m).clone();
                let c = point.unwrap_or_else(|| {
                    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
                    for q in &m.positions {
                        for k in 0..2 {
                            lo[k] = lo[k].min(q[k]);
                            hi[k] = hi[k].max(q[k]);
                        }
                    }
                    geo::coord! { x: (lo[0] + hi[0]) / 2.0, y: (lo[1] + hi[1]) / 2.0 }
                });
                let (s, co) = a.to_radians().sin_cos();
                let turn = |x: f64, y: f64| (co * x - s * y, s * x + co * y);
                for q in &mut m.positions {
                    let (x, y) = turn(q[0] - c.x, q[1] - c.y);
                    q[0] = c.x + x;
                    q[1] = c.y + y;
                }
                for n in &mut m.normals {
                    let (x, y) = turn(n[0] as f64, n[1] as f64);
                    n[0] = x as f32;
                    n[1] = y as f32;
                }
                out.mesh = Some(Arc::new(m));
            }
            None => ctx.notes.warn("A 3D model is turned by a fixed angle only, not one from the attributes. It was left as it was.".to_string()),
        }
    }
    if table.grid.is_some() {
        out.grid = None;
        ctx.notes.warn(
            "A grid of cells cannot be turned and still be a grid. The cells are passed on as features, without the grid for Shade terrain and Quick terrain view."
                .to_string(),
        );
    }
    ctx.notes.info(match (fixed, around.as_str()) {
        (Some(a), "point") => {
            format!("turned {}° around {}, {}", fmt_number(a), fmt_number(point.map_or(0.0, |c| c.x)), fmt_number(point.map_or(0.0, |c| c.y)))
        }
        (Some(a), "all") => format!("turned {}° around the middle of all", fmt_number(a)),
        (Some(a), _) => format!("turned {}° each around its middle", fmt_number(a)),
        (None, _) => format!("turned by {}", text.trim()),
    });
    Ok(vec![("Output".into(), out)])
}

pub(super) fn rotate_summary(p: &P) -> String {
    let a = p.get("angle").unwrap_or_else(|| "90".into());
    let a = if a.trim().parse::<f64>().is_ok() { format!("{}°", a.trim()) } else { a };
    match p.get("around").as_deref() {
        Some("all") => format!("{} around the middle of all", a),
        Some("point") => format!("{} around {}, {}", a, p.get("x").unwrap_or_default(), p.get("y").unwrap_or_default()),
        _ => a,
    }
}

// ---------- Reverse lines ----------

pub(super) const REVERSE_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "areas",
    label: "Areas",
    kind: ParamKind::Choice(&[("reverse", "Their outlines turned too"), ("keep", "Kept as they are")]),
    default: "reverse",
    hint: "The outline of an area runs one way round. Some formats want it clockwise, others not",
}];

/// Turns the direction of the lines in a geometry, and the heights with them. Parts of a
/// multi line come in the opposite order too, so the whole runs back from the end.
pub(super) fn reverse(g: &mut Geometry<f64>, z: Option<&mut [f64]>, areas: bool) -> bool {
    match g {
        Geometry::Line(l) => {
            std::mem::swap(&mut l.start, &mut l.end);
            if let Some(z) = z {
                z.reverse();
            }
            true
        }
        Geometry::LineString(l) => {
            l.0.reverse();
            if let Some(z) = z {
                z.reverse();
            }
            true
        }
        Geometry::MultiLineString(m) => {
            m.0.reverse();
            for l in &mut m.0 {
                l.0.reverse();
            }
            // Every part reversed and the parts in the opposite order: the heights all back to front.
            if let Some(z) = z {
                z.reverse();
            }
            true
        }
        Geometry::Polygon(p) if areas => {
            if let Some(z) = z {
                let mut at = 0;
                for n in std::iter::once(p.exterior().0.len()).chain(p.interiors().iter().map(|r| r.0.len())) {
                    z[at..at + n].reverse();
                    at += n;
                }
            }
            p.exterior_mut(|r| r.0.reverse());
            p.interiors_mut(|rs| rs.iter_mut().for_each(|r| r.0.reverse()));
            true
        }
        Geometry::MultiPolygon(m) if areas => {
            let mut z = z;
            let mut at = 0;
            for p in &mut m.0 {
                let mut g = Geometry::Polygon(std::mem::replace(p, geo::Polygon::new(geo::LineString::new(vec![]), vec![])));
                let n = geom::coord_count(&g);
                reverse(&mut g, z.as_deref_mut().map(|z| &mut z[at..at + n]), true);
                at += n;
                if let Geometry::Polygon(back) = g {
                    *p = back;
                }
            }
            true
        }
        Geometry::GeometryCollection(c) => {
            let mut z = z;
            let mut at = 0;
            let mut any = false;
            for g in &mut c.0 {
                let n = geom::coord_count(g);
                any |= reverse(g, z.as_deref_mut().map(|z| &mut z[at..at + n]), areas);
                at += n;
            }
            any
        }
        _ => false,
    }
}

pub(super) fn run_reverse(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let areas = p.get("areas").as_deref() == Some("reverse");
    let mut out = table.clone();
    let mut turned = 0usize;
    for f in out.features.iter_mut() {
        let Some(g) = f.geom.as_mut() else { continue };
        let fits = f.z.as_ref().is_some_and(|z| z.len() == geom::coord_count(g));
        let mut z: Option<Vec<f64>> = if fits { f.z.as_ref().map(|z| z.to_vec()) } else { None };
        if reverse(g, z.as_deref_mut(), areas) {
            turned += 1;
            if let Some(z) = z {
                f.z = Some(z.into());
            }
        }
    }
    ctx.notes.info(format!("{} {} turned", fmt_count(turned as u64), if areas { "lines and areas" } else { "lines" }));
    if turned == 0 && !table.features.is_empty() {
        ctx.notes.warn(if areas { "There are no lines or areas in the stream to turn." } else { "There are no lines in the stream to turn." }.to_string());
    }
    Ok(vec![("Output".into(), out)])
}

// ---------- Swap x and y (XySwapper) ----------

pub(super) fn run_xy_swapper(_p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    use geo::MapCoordsInPlace;
    let table = input(inp, "Input");
    let mut out = table.clone();
    let mut swapped = 0usize;
    for f in out.features.iter_mut() {
        if let Some(g) = f.geom.as_mut() {
            g.map_coords_in_place(|c| geo::Coord { x: c.y, y: c.x });
            swapped += 1;
        }
    }
    if swapped > 0 && ctx.geographic(table.crs) == Some(true) {
        ctx.notes.info("In degrees, x is the longitude and y the latitude: in Sweden x is about 11 to 24 and y 55 to 69");
    }
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "CrsSetter",
        title: "Set coordinate system",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Says which coordinate system the stream is in, without changing the coordinates. For data that lacks one, or states the wrong one.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: CRS_SETTER_PARAMS,
        run: run_crs_setter,
        summary: |p| p.get("crs").unwrap_or_default(),
    },
    NodeType {
        name: "Force2D",
        title: "Geometry to 2D",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes the geometry two-dimensional: the heights (Z) of the coordinates go, and the 3D model that comes with a stream, such as from an IFC, CityGML or OBJ file, so that what is left is the plan, shown and written in 2D. Measures (M) are dropped when data is read.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: &[],
        run: run_force2d,
        summary: |_| String::new(),
    },
    NodeType {
        name: "Scaler",
        title: "Scale",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Makes the geometry larger or smaller by a factor, around the origin or the middle of the features. For example 0.001 to turn CAD drawings in millimetres into metres.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: SCALER_PARAMS,
        run: run_scaler,
        summary: |p| {
            let f = p.get("factor").unwrap_or_default();
            match p.get("factor_y").filter(|y| !y.trim().is_empty()) {
                Some(y) => format!("× {} in x, × {} in y", f, y),
                None => format!("× {}", f),
            }
        },
    },
    NodeType {
        name: "Offsetter",
        title: "Move by x, y and z",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Moves the geometry a distance along x and along y, in the units of the stream's coordinate system, and up or down: features with Z, 3D models and grids of heights.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: OFFSETTER_PARAMS,
        run: run_offsetter,
        summary: |p| {
            let (x, y) = (p.get("dx").unwrap_or_else(|| "0".into()), p.get("dy").unwrap_or_else(|| "0".into()));
            match p.get("dz").filter(|z| !z.trim().is_empty() && z.trim() != "0") {
                Some(z) => format!("x {} · y {} · z {}", x, y, z),
                None => format!("x {} · y {}", x, y),
            }
        },
    },
    NodeType {
        name: "Rotator",
        title: "Rotate",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Turns the geometry by an angle in degrees, counter-clockwise, around the middle of each feature, the middle of all of them or a point you give. The angle can come from an attribute. Heights stay as they are, and a 3D model turns with its features.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: ROTATE_PARAMS,
        run: run_rotate,
        summary: rotate_summary,
    },
    NodeType {
        name: "XySwapper",
        title: "Swap x and y",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Swaps the x and y coordinates of all geometry, for data where they have been mixed up: latitude given as x, or northing and easting in the wrong order. Heights stay as they are. Lines and areas are swapped too.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: &[],
        run: run_xy_swapper,
        summary: |_| String::new(),
    },
    NodeType {
        name: "LineReverser",
        title: "Reverse geometry",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Turns the direction of lines and of the outlines of areas, holes too, so that they start where they ended, with their heights: clockwise becomes counter-clockwise. The parts of a multi geometry come in the opposite order. Areas can be kept as they are; points stay as they are.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: REVERSE_PARAMS,
        run: run_reverse,
        summary: |p| if p.get("areas").as_deref() == Some("keep") { "lines only".into() } else { "lines and areas".into() },
    },
    NodeType {
        name: "Reprojector",
        title: "Reproject",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Reprojects the geometry to another coordinate system. The axis order is always east, north.",
        inputs: &["Input"],
        outputs: &["Reprojected"],
        ports: None,
        params: REPROJECTOR_PARAMS,
        run: run_reprojector,
        summary: |p| p.crs("crs").ok().flatten().map(|c| format!("→ {}", crs_label(Some(c)))).unwrap_or_else(|| "?".into()),
    },
    NodeType {
        name: "HeightSetter",
        title: "Set height",
        hidden: false,
        group: Group::Transformer,
        category: "Geometry",
        desc: "Gives every coordinate of each feature a height (Z): from an attribute or an expression, such as the height of a contour line; from the terrain model connected to Terrain, such as roads laid on the ground; or the heights they have and a value more. Can make them flat again. The heights are kept by tools that do not change the geometry, shown in 3D on the map and written to formats that take them.",
        inputs: &["Input", "Terrain"],
        outputs: &["Output"],
        ports: None,
        params: HEIGHT_PARAMS,
        run: run_height,
        summary: |p| match p.get("mode").as_deref() {
            Some("remove") => "flat".into(),
            Some("terrain") => format!("on the terrain{}", p.get("height").map(|h| format!(" + {}", h)).unwrap_or_default()),
            Some("add") => format!("+ {}", p.get("height").unwrap_or_default()),
            _ => format!("at {}", p.get("height").unwrap_or_else(|| "?".into())),
        },
    },
];
