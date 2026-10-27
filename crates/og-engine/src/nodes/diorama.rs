//! Make diorama: areas of land cover as a toy landscape, as in a picture book. The land
//! is rounded and stands on a board as a slab, with a beach along the shore and the kinds
//! of land as thin plates on top in clear colours; the water is the top of the board.

use super::geom_shape::need_metres;
use super::props;
use super::*;
use crate::mesh::{Material, Mesh, Tri, NONE};
use crate::model::{err, fmt_count, Feature, Field, FieldType, GbError, Result, Value};
use geo::{Area, BooleanOps, BoundingRect, Geometry, MultiPolygon, Polygon, TriangulateEarcut, Winding};
use std::collections::HashMap;
use std::sync::Arc;

pub(super) const THEMES: &[(&str, &str)] = &[("toy", "Toy: clear, bright colours"), ("pastel", "Pastel: soft and light")];
pub(super) const BOARDS: &[(&str, &str)] = &[
    ("outline", "Following the shape of the land and the water"),
    ("box", "A rounded rectangle around it all"),
    ("round", "Round, around the land"),
    ("none", "None: the land floats by itself"),
];

pub(super) const DIORAMA_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "class",
        label: "Kind of land from",
        kind: ParamKind::Attribute("Areas"),
        default: "",
        hint: "An attribute with the land cover, such as forest, field or water, in Swedish or English: it picks the colours. Empty: all is land",
    },
    ParamSpec {
        key: "water_values",
        label: "Water is",
        kind: ParamKind::Text,
        default: "",
        hint: "The values of that attribute that are water, separated by commas. Empty: values with words such as water, lake, sea, vatten, sjö, hav",
    },
    ParamSpec { key: "theme", label: "Colours", kind: ParamKind::Choice(THEMES), default: "toy", hint: "" },
    ParamSpec {
        key: "colours",
        label: "Own colours",
        kind: ParamKind::Text,
        default: "",
        hint: "For values of the attribute, such as Barrskog=#2f7d32, Åker=#e0d77a. The others take theirs from the colours above",
    },
    ParamSpec {
        key: "radius",
        label: "Rounding in metres",
        kind: ParamKind::Text,
        default: "",
        hint: "Corners become curves of this radius, and inlets and spits narrower than twice it go. Empty: a size that suits the area. 0: as they are",
    },
    ParamSpec {
        key: "smallest",
        label: "Smallest part in m²",
        kind: ParamKind::Text,
        default: "",
        hint: "Islands, lakes and patches smaller than this go. Empty: some ten times the rounding squared",
    },
    ParamSpec { key: "board", label: "Board", kind: ParamKind::Choice(BOARDS), default: "outline", hint: "The water lies on it" },
    ParamSpec {
        key: "thickness",
        label: "Thickness of the board in metres",
        kind: ParamKind::Text,
        default: "",
        hint: "Empty: 5% of the width of the area, which suits a picture of the whole",
    },
    ParamSpec {
        key: "lift",
        label: "Land above the water in metres",
        kind: ParamKind::Text,
        default: "",
        hint: "The edge of the land, as the side of a cake. Empty: 60% of the thickness of the board",
    },
    ParamSpec {
        key: "beach",
        label: "Beach in metres",
        kind: ParamKind::Text,
        default: "",
        hint: "A rim of sand along the shore. Empty: as wide as the rounding, and at least 1.2% of the width of the area. 0: none",
    },
    ParamSpec {
        key: "steps",
        label: "Terraces",
        kind: ParamKind::Text,
        default: "4",
        hint: "With a terrain model on Heights: the land rises in this many steps, as layers of a cake, from the shore to the highest 1%",
    },
    ParamSpec {
        key: "relief",
        label: "Height of the terraces in metres",
        kind: ParamKind::Text,
        default: "",
        hint: "Of them all together, larger than in life as on a toy. Empty: 1.6 times the thickness of the board",
    },
    ParamSpec {
        key: "trees",
        label: "Trees in %",
        kind: ParamKind::Text,
        default: "50",
        hint: "How close they stand in forest: 100 is a dense wood, 0 none. Pines in coniferous forest, round trees in other forest and a few in parks",
    },
    ParamSpec {
        key: "houses",
        label: "Houses in %",
        kind: ParamKind::Text,
        default: "50",
        hint: "How close they stand in built-up areas, with roofs in bright colours",
    },
    ParamSpec {
        key: "animals",
        label: "Animals",
        kind: ParamKind::Code(""),
        default: "",
        hint: "How many of each, as sheep 12, one or more on a line or with commas between: sheep, pig, cow, horse, rabbit, dog, cat, bear, elephant, giraffe, dinosaur, in English or Swedish. Farm animals and the large ones graze on fields, meadows and open land, dogs and cats are in towns and bears in the forest",
    },
    ParamSpec { key: "boats", label: "Boats", kind: ParamKind::Text, default: "0", hint: "How many sailing boats on the water of the board" },
    ParamSpec { key: "clouds", label: "Clouds", kind: ParamKind::Text, default: "0", hint: "How many fluffy clouds over the land. Their shadows fall on it" },
    ParamSpec {
        key: "object_size",
        label: "Size of a tree in metres",
        kind: ParamKind::Text,
        default: "",
        hint: "Houses, sheep and boats go with it. Empty: about 2% of the width of the area, large as on a toy",
    },
    ParamSpec {
        key: "pine_values",
        label: "Pines on",
        kind: ParamKind::Text,
        default: "",
        hint: "Values of the attribute, separated by commas, that get pines, as Barrskog. Empty: by words such as barr, gran, tall, conifer",
    },
    ParamSpec {
        key: "tree_values",
        label: "Round trees on",
        kind: ParamKind::Text,
        default: "",
        hint: "Values that get round trees, as Lövskog. Empty: by words such as skog, löv, forest, wood",
    },
    ParamSpec {
        key: "house_values",
        label: "Houses on",
        kind: ParamKind::Text,
        default: "",
        hint: "Values that get houses. Empty: by words such as bebyggelse, tätort, urban, town",
    },
    ParamSpec {
        key: "sheep_values",
        label: "Grazing animals on",
        kind: ParamKind::Text,
        default: "",
        hint: "Values where farm animals and the large ones graze. Empty: by words such as åker, äng, öppen, field, meadow",
    },
    ParamSpec { key: "seed", label: "Variation", kind: ParamKind::Text, default: "1", hint: "Another whole number puts the objects and clouds elsewhere" },
];

const CONIFER_WORDS: &[&str] = &["barr", "gran", "tall", "conifer", "needle"];
const FOREST_WORDS: &[&str] = &["skog", "forest", "wood", "löv", "lund"];
const PARK_WORDS: &[&str] = &["park", "garden", "trädgård", "golf", "recreation"];
const BUILT_WORDS: &[&str] = &["bebygg", "tätort", "urban", "residential", "industr", "commercial", "built", "town", "village", "retail", "hus"];
const GRAZING_WORDS: &[&str] = &["åker", "farm", "field", "äng", "bete", "pasture", "meadow", "öppen", "open", "grass", "heath", "hed"];

fn has_word(name: &str, words: &[&str]) -> bool {
    let n = name.to_lowercase();
    words.iter().any(|w| n.contains(w))
}

/// The heights over an extent, at most some 160 cells across, for terraces.
fn terrace_grid(g: &crate::model::Grid, r: &geo::Rect<f64>) -> crate::model::Grid {
    let gt = g.geotransform;
    let col = |x: f64| ((x - gt[0]) / gt[1]).floor();
    let row = |y: f64| ((y - gt[3]) / gt[5]).floor();
    let (ca, cb) = (col(r.min().x), col(r.max().x));
    let (ra, rb) = (row(r.max().y), row(r.min().y));
    let c0 = (ca.min(cb).max(0.0) as usize).min(g.width.saturating_sub(1));
    let c1 = ((ca.max(cb) + 2.0).max(0.0) as usize).min(g.width);
    let r0 = (ra.min(rb).max(0.0) as usize).min(g.height.saturating_sub(1));
    let r1 = ((ra.max(rb) + 2.0).max(0.0) as usize).min(g.height);
    let (w, h) = (c1.saturating_sub(c0).max(1), r1.saturating_sub(r0).max(1));
    let mut values = Vec::with_capacity(w * h);
    for rr in r0..r0 + h {
        for cc in c0..c0 + w {
            values.push(g.get(cc, rr).unwrap_or(f32::NAN));
        }
    }
    let cut = crate::model::Grid {
        width: w,
        height: h,
        values,
        geotransform: [gt[0] + c0 as f64 * gt[1], gt[1], 0.0, gt[3] + r0 as f64 * gt[5], 0.0, gt[5]],
        crs: g.crs,
        bands: Vec::new(),
    };
    crate::terrain::at_most(&cut, 160)
}

/// The cells at or above a height, as areas: the runs of such cells in each row, joined.
fn above_level(g: &crate::model::Grid, level: f32) -> MultiPolygon<f64> {
    let gt = g.geotransform;
    let mut boxes: Vec<Polygon<f64>> = Vec::new();
    for r in 0..g.height {
        let mut c = 0;
        while c < g.width {
            if g.values[r * g.width + c] >= level {
                let start = c;
                while c < g.width && g.values[r * g.width + c] >= level {
                    c += 1;
                }
                let (x0, x1) = (gt[0] + start as f64 * gt[1], gt[0] + c as f64 * gt[1]);
                let (y0, y1) = (gt[3] + r as f64 * gt[5], gt[3] + (r + 1) as f64 * gt[5]);
                boxes.push(geo::Rect::new(geo::coord! { x: x0, y: y0 }, geo::coord! { x: x1, y: y1 }).to_polygon());
            } else {
                c += 1;
            }
        }
    }
    geo::unary_union(boxes.iter())
}

/// The animals asked for: a kind and how many, as sheep 12, pigs = 3, 2 horses, by lines or
/// commas. A number alone is sheep, as the setting held before 0.87.
fn animals_of(text: &str) -> Result<Vec<(&'static props::Animal, usize)>> {
    let mut out: Vec<(&'static props::Animal, usize)> = Vec::new();
    let text = text.trim();
    if let Ok(n) = text.parse::<f64>() {
        if n >= 1.0 {
            out.push((&props::ANIMALS[0], n.min(5000.0) as usize));
        }
        return Ok(out);
    }
    for part in text.split(['\n', ',', ';']).map(str::trim).filter(|p| !p.is_empty() && !p.starts_with('#')) {
        let words: Vec<&str> = part.split(|c: char| c.is_whitespace() || c == '=' || c == ':').filter(|w| !w.is_empty()).collect();
        let number = words.iter().find_map(|w| w.parse::<f64>().ok());
        let name: Vec<&str> = words.iter().copied().filter(|w| w.parse::<f64>().is_err()).collect();
        let Some(kind) = props::animal_by_word(&name.join(" ")) else {
            let known: Vec<&str> = props::ANIMALS.iter().map(|a| a.name).collect();
            return err(format!("\"{}\" is not an animal there is. There are: {}.", name.join(" "), known.join(", ")));
        };
        let n = number.unwrap_or(1.0).clamp(0.0, 5000.0) as usize;
        match out.iter_mut().find(|(k, _)| k.name == kind.name) {
            Some(e) => e.1 += n,
            None => out.push((kind, n)),
        }
    }
    Ok(out.into_iter().filter(|e| e.1 > 0).collect())
}

/// Points spread about evenly over the areas, one by chance in each square of `spacing`,
/// at most `most` of them.
fn scatter(area: &MultiPolygon<f64>, spacing: f64, rng: &mut props::Rng, most: usize) -> Vec<(f64, f64)> {
    use geo::Contains;
    let Some(r) = area.bounding_rect() else { return Vec::new() };
    if area.0.is_empty() || spacing <= 0.0 {
        return Vec::new();
    }
    // Never more than some million squares to try.
    let spacing = spacing.max((r.width() * r.height() / 4e6).sqrt());
    let (nx, ny) = ((r.width() / spacing).ceil() as usize, (r.height() / spacing).ceil() as usize);
    let index = geo::algorithm::indexed::IntervalTreeMultiPolygon::new(area);
    let mut out = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let x = r.min().x + (i as f64 + rng.next()) * spacing;
            let y = r.min().y + (j as f64 + rng.next()) * spacing;
            if index.contains(&geo::coord! { x: x, y: y }) {
                out.push((x, y));
            }
        }
    }
    if out.len() > most {
        let step = out.len() as f64 / most as f64;
        out = (0..most).map(|k| out[(k as f64 * step) as usize]).collect();
    }
    out
}

/// Takes `n` of the spots at random, in place: they are the first `n` after.
fn pick_some(spots: &mut [(f64, f64)], n: usize, rng: &mut props::Rng) -> usize {
    let n = n.min(spots.len());
    for k in 0..n {
        let j = k + ((rng.next() * (spots.len() - k) as f64) as usize).min(spots.len() - k - 1);
        spots.swap(k, j);
    }
    n
}

/// A colour as #rrggbb to sRGB from 0 to 1.
fn hex(c: &str) -> [f32; 3] {
    let v = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0x808080);
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}

fn hex_of(c: [f32; 3]) -> String {
    let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

const WATER_WORDS: &[&str] = &["water", "vatten", "sjö", "hav", "lake", "sea", "river", "älv", "ocean", "pond", "damm", "reservoir", "vattendrag", "fjärd"];

/// Kinds of land by words in their names, in Swedish and English, as in Lantmäteriet's
/// land cover and OpenStreetMap, and their colours.
const KINDS: &[(&[&str], &str)] = &[
    (WATER_WORDS, "#4cc3e6"),
    (&["glaci", "snö", "snow", "ice", "jökel"], "#f4f8fb"),
    (&["barr", "conifer", "needle", "gran", "tall"], "#2f8f4e"),
    (&["sank", "myr", "mosse", "kärr", "wetland", "marsh", "bog", "swamp"], "#9ccfa6"),
    (&["skog", "forest", "wood", "löv", "lund"], "#4daa4f"),
    (&["åker", "farm", "field", "crop", "arable", "orchard", "odl"], "#d6dc6a"),
    (&["bebygg", "tätort", "urban", "residential", "industr", "commercial", "built", "town", "village", "retail", "hus"], "#f0c9a0"),
    (&["sand", "beach", "strand", "dyn", "dune"], "#f3e2b1"),
    (&["fjäll", "berg", "rock", "bare", "kal", "häll", "cliff", "scree", "stone"], "#b9b3aa"),
    (&["park", "golf", "garden", "trädgård", "recreation", "sport"], "#7cc95f"),
    (&["öppen", "open", "grass", "meadow", "äng", "betes", "pasture", "heath", "hed", "scrub"], "#8ed36b"),
];
const OTHERS: &[&str] = &["#8ed36b", "#d6dc6a", "#f0c9a0", "#9ccfa6", "#c7a4d8", "#f4b6a6", "#a5d0e8"];
const BOARD_SIDE: &str = "#f0dfb8";
const EARTH: &str = "#d08a55";
const SAND: &str = "#f3e2b1";
const WATER: &str = "#4cc3e6";
const GRASS: &str = "#8ed36b";

fn is_water_word(name: &str) -> bool {
    let n = name.to_lowercase();
    WATER_WORDS.iter().any(|w| n.contains(w))
}

/// The colour of a kind of land, by the words in its name, or one of a few others.
fn kind_colour(name: &str) -> [f32; 3] {
    let n = name.to_lowercase();
    for (words, c) in KINDS {
        if words.iter().any(|w| n.contains(w)) {
            return hex(c);
        }
    }
    let k = n.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    hex(OTHERS[k % OTHERS.len()])
}

/// Lighter, for the pastel colours.
fn soft(c: [f32; 3], pastel: bool) -> [f32; 3] {
    if pastel {
        c.map(|v| v + (1.0 - v) * 0.35)
    } else {
        c
    }
}

fn darker(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| v * 0.8)
}

fn multi(g: Option<Geometry<f64>>) -> MultiPolygon<f64> {
    MultiPolygon::new(g.map(|g| geom::polygons_of(&g)).unwrap_or_default())
}

fn area_of(m: &MultiPolygon<f64>) -> f64 {
    m.unsigned_area()
}

/// Adds a slab to the model: walls around every ring of the areas, from z0 to z1, a top
/// and, if asked, a bottom. The walls are shaded smoothly where the rings curve, so that
/// rounded shores look round.
#[allow(clippy::too_many_arguments)]
fn add_slab(mesh: &mut Mesh, areas: &MultiPolygon<f64>, z0: f64, z1: f64, top: u32, side: u32, bottom: Option<u32>, object: u32) {
    let smooth = 40f64.to_radians().cos();
    for poly in &areas.0 {
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
            let mut pts: Vec<geo::Coord<f64>> = Vec::with_capacity(ring.0.len());
            for c in &ring.0 {
                if pts.last().is_none_or(|l: &geo::Coord<f64>| (l.x - c.x).hypot(l.y - c.y) > 1e-9) {
                    pts.push(*c);
                }
            }
            while pts.len() > 1 && (pts[0].x - pts[pts.len() - 1].x).hypot(pts[0].y - pts[pts.len() - 1].y) <= 1e-9 {
                pts.pop();
            }
            let n = pts.len();
            if n < 3 {
                continue;
            }
            // The outward normal of each edge, and of each corner where the ring bends gently.
            let edge: Vec<[f64; 2]> = (0..n)
                .map(|i| {
                    let (a, b) = (pts[i], pts[(i + 1) % n]);
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    let len = dx.hypot(dy).max(1e-12);
                    [dy / len, -dx / len]
                })
                .collect();
            let corner: Vec<Option<[f64; 2]>> = (0..n)
                .map(|i| {
                    let (p, q) = (edge[(i + n - 1) % n], edge[i]);
                    if p[0] * q[0] + p[1] * q[1] < smooth {
                        return None;
                    }
                    let (x, y) = (p[0] + q[0], p[1] + q[1]);
                    let len = x.hypot(y).max(1e-12);
                    Some([x / len, y / len])
                })
                .collect();
            for i in 0..n {
                let j = (i + 1) % n;
                let (a, b) = (pts[i], pts[j]);
                let na = corner[i].unwrap_or(edge[i]);
                let nb = corner[j].unwrap_or(edge[i]);
                let k = mesh.normals.len() as u32;
                mesh.normals.push([na[0] as f32, na[1] as f32, 0.0]);
                mesh.normals.push([nb[0] as f32, nb[1] as f32, 0.0]);
                let v = mesh.positions.len() as u32;
                mesh.positions.extend_from_slice(&[[a.x, a.y, z0], [b.x, b.y, z0], [b.x, b.y, z1], [a.x, a.y, z1]]);
                mesh.tris.push(Tri { v: [v, v + 1, v + 2], n: [k, k + 1, k + 1], t: [NONE; 3], material: side, object });
                mesh.tris.push(Tri { v: [v, v + 2, v + 3], n: [k, k + 1, k], t: [NONE; 3], material: side, object });
            }
        }
        let raw = poly.earcut_triangles_raw();
        let (up, down) = (mesh.normals.len() as u32, mesh.normals.len() as u32 + 1);
        mesh.normals.push([0.0, 0.0, 1.0]);
        mesh.normals.push([0.0, 0.0, -1.0]);
        let first_top = mesh.positions.len() as u32;
        mesh.positions.extend(raw.vertices.iter().map(|c| [c[0], c[1], z1]));
        let first_bottom = mesh.positions.len() as u32;
        if bottom.is_some() {
            mesh.positions.extend(raw.vertices.iter().map(|c| [c[0], c[1], z0]));
        }
        for t in raw.triangle_indices.as_chunks::<3>().0.iter() {
            let (a, b, c) = (t[0] as u32, t[1] as u32, t[2] as u32);
            let (pa, pb, pc) = (raw.vertices[t[0]], raw.vertices[t[1]], raw.vertices[t[2]]);
            let ccw = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]) > 0.0;
            let (b, c) = if ccw { (b, c) } else { (c, b) };
            mesh.tris.push(Tri { v: [first_top + a, first_top + b, first_top + c], n: [up; 3], t: [NONE; 3], material: top, object });
            if let Some(m) = bottom {
                mesh.tris.push(Tri { v: [first_bottom + a, first_bottom + c, first_bottom + b], n: [down; 3], t: [NONE; 3], material: m, object });
            }
        }
    }
}

fn run_diorama(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Areas");
    if table.features.is_empty() {
        return err("Connect areas to Areas, such as land cover with forest, fields and water.");
    }
    need_metres(ctx, table, "Make diorama")?;
    let class_at = match p.get("class") {
        Some(name) => Some(table.field_index(&name).ok_or_else(|| GbError(format!("The stream has no attribute \"{}\".", name)))?),
        None => None,
    };
    let number = |k: &str, what: &str| number_param(p, k, what);
    // The areas of each kind of land, in the order they first come.
    let mut kinds: Vec<(String, Vec<Polygon<f64>>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut not_areas = 0usize;
    for f in &table.features {
        let Some(g) = &f.geom else { continue };
        let polys = geom::polygons_of(g);
        if polys.is_empty() {
            not_areas += 1;
            continue;
        }
        let name = match class_at.map(|k| &f.attrs[k]) {
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_text().trim().to_string(),
        };
        let k = *index.entry(name.clone()).or_insert_with(|| {
            kinds.push((name, Vec::new()));
            kinds.len() - 1
        });
        kinds[k].1.extend(polys);
    }
    if not_areas > 0 {
        ctx.notes.warn(format!("{} features are not areas and were left out.", fmt_count(not_areas as u64)));
    }
    if kinds.is_empty() {
        return err("None of the features on Areas is an area.");
    }
    let water_values: Option<Vec<String>> =
        p.get("water_values").map(|v| v.split([',', ';']).map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect());
    let is_water = |name: &str| match &water_values {
        Some(list) => list.iter().any(|w| *w == name.to_lowercase()),
        None => is_water_word(name),
    };
    let pastel = p.get("theme").as_deref() == Some("pastel");
    let mut own: HashMap<String, [f32; 3]> = HashMap::new();
    if let Some(text) = p.get("colours") {
        for part in text.split([',', ';', '\n']).map(str::trim).filter(|t| !t.is_empty()) {
            let Some((k, v)) = part.split_once('=') else {
                return err(format!("Own colours are written as value=#rrggbb, such as Åker=#e0d77a, not \"{}\".", part));
            };
            match crate::mapdraw::parse_color(v.trim()) {
                Ok(Some(c)) => {
                    own.insert(k.trim().to_lowercase(), [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]);
                }
                _ => return err(format!("\"{}\" is not a colour such as #e0d77a.", v.trim())),
            }
        }
    }
    let colour_of = |name: &str| own.get(&name.to_lowercase()).copied().unwrap_or_else(|| soft(kind_colour(name), pastel));

    // The sizes, from the extent unless given.
    let all: Vec<Polygon<f64>> = kinds.iter().flat_map(|k| k.1.iter().cloned()).collect();
    let Some(rect) = MultiPolygon::new(all.clone()).bounding_rect() else { return err("The areas have no extent.") };
    let size = rect.width().max(rect.height()).max(1e-6);
    let radius = number("radius", "The rounding")?.unwrap_or(size / 150.0).max(0.0);
    let smallest = number("smallest", "The smallest part")?.unwrap_or(radius * radius * 10.0).max(0.0);
    let thick = match number("thickness", "The thickness of the board")? {
        Some(t) if t <= 0.0 => return err("The thickness of the board must be more than 0 metres."),
        t => t.unwrap_or(size * 0.05),
    };
    let lift = number("lift", "The height of the land")?.unwrap_or(thick * 0.6).max(0.0);
    let beach = number("beach", "The width of the beach")?.unwrap_or(radius.max(size * 0.012)).max(0.0);
    let plate = thick * 0.18;
    let thin = thick * 0.1;

    let union = |polys: &[Polygon<f64>]| -> MultiPolygon<f64> { geo::unary_union(polys.iter()) };
    let soften = |m: &MultiPolygon<f64>, r: f64| multi(geom::soften(&Geometry::MultiPolygon(m.clone()), r, smallest));
    let land_polys: Vec<Polygon<f64>> = kinds.iter().filter(|k| !is_water(&k.0)).flat_map(|k| k.1.iter().cloned()).collect();
    let water_polys: Vec<Polygon<f64>> = kinds.iter().filter(|k| is_water(&k.0)).flat_map(|k| k.1.iter().cloned()).collect();
    if land_polys.is_empty() {
        return err("All the areas are water, so there is no land to make. Check Water is.");
    }
    let mut land = union(&land_polys);
    // Lakes are cut out of the land under them, but not a sea drawn under it all, as a
    // rectangle behind the islands.
    let land_area = area_of(&land);
    let cut: Vec<Polygon<f64>> = water_polys.iter().filter(|w| w.unsigned_area() < land_area * 0.5).cloned().collect();
    if !cut.is_empty() {
        land = land.difference(&union(&cut));
    }
    let mut land = soften(&land, radius);
    let board_kind = p.get("board").unwrap_or_else(|| "outline".into());
    let board: Option<MultiPolygon<f64>> = match board_kind.as_str() {
        "none" => None,
        "box" => {
            let m = (2.0 * radius).max(size * 0.02);
            let r = geo::Rect::new(geo::coord! { x: rect.min().x - m, y: rect.min().y - m }, geo::coord! { x: rect.max().x + m, y: rect.max().y + m });
            Some(multi(geom::soften(&Geometry::Polygon(r.to_polygon()), size * 0.05, 0.0)))
        }
        "round" => {
            // A circle around the middle of the land, out to its furthest point and a little more.
            let lr = land.bounding_rect().unwrap_or(rect);
            let (cx, cy) = (lr.center().x, lr.center().y);
            let far = land.0.iter().flat_map(|q| q.exterior().0.iter()).map(|c| (c.x - cx).hypot(c.y - cy)).fold(0.0f64, f64::max);
            let r = far + (2.0 * radius).max(size * 0.04);
            let ring: Vec<geo::Coord<f64>> = (0..=160)
                .map(|k| {
                    let a = 2.0 * std::f64::consts::PI * k as f64 / 160.0;
                    geo::coord! { x: cx + r * a.cos(), y: cy + r * a.sin() }
                })
                .collect();
            Some(MultiPolygon::new(vec![Polygon::new(geo::LineString::new(ring), vec![])]))
        }
        _ => {
            let grown = multi(geom::buffer(&Geometry::MultiPolygon(union(&all)), 2.0 * radius.max(size * 0.005)));
            Some(soften(&grown, (2.0 * radius).max(size * 0.03)))
        }
    };
    if let Some(b) = &board {
        land = land.intersection(b);
    }
    if area_of(&land) <= 0.0 {
        return err("Nothing is left of the land after the rounding. Make the rounding or the smallest part smaller.");
    }
    let inner = if beach > 0.0 { multi(geom::buffer(&Geometry::MultiPolygon(land.clone()), -beach)) } else { land.clone() };

    // The kinds of land, the largest first: it is the ground, and the others lie on it.
    let mut land_kinds: Vec<(String, MultiPolygon<f64>, f64)> = kinds
        .iter()
        .filter(|k| !is_water(&k.0))
        .map(|k| {
            let m = union(&k.1).intersection(&inner);
            let a = area_of(&m);
            (k.0.clone(), m, a)
        })
        .collect();
    land_kinds.sort_by(|a, b| b.2.total_cmp(&a.2));

    let mut mesh = Mesh::default();
    let mut names: HashMap<String, u32> = HashMap::new();
    let mut material = |mesh: &mut Mesh, name: &str, colour: [f32; 3], shiny: bool| -> u32 {
        *names.entry(name.to_string()).or_insert_with(|| {
            mesh.materials.push(Material {
                name: name.to_string(),
                diffuse: colour,
                specular: if shiny { [0.25; 3] } else { [0.0; 3] },
                shininess: if shiny { 80.0 } else { 10.0 },
                ..Default::default()
            });
            mesh.materials.len() as u32 - 1
        })
    };
    let fields = vec![
        Field::new("layer", FieldType::Text),
        Field::new("kind", FieldType::Text),
        Field::new("colour", FieldType::Text),
        Field::new("z_base", FieldType::Real),
        Field::new("z_top", FieldType::Real),
    ];
    let mut out = Table { fields, crs: table.crs, ..Default::default() };
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let piece = |mesh: &mut Mesh,
                 out: &mut Table,
                 layer: &str,
                 kind: &str,
                 colour: [f32; 3],
                 areas: &MultiPolygon<f64>,
                 z: (f64, f64),
                 mats: (u32, u32, Option<u32>)| {
        if areas.0.is_empty() {
            return;
        }
        let object = out.features.len() as u32;
        add_slab(mesh, areas, z.0, z.1, mats.0, mats.1, mats.2, object);
        mesh.objects.push(if kind.is_empty() { layer.to_string() } else { format!("{} {}", layer, kind) });
        let geom = if areas.0.len() == 1 { Geometry::Polygon(areas.0[0].clone()) } else { Geometry::MultiPolygon(areas.clone()) };
        out.features.push(Feature {
            attrs: vec![Value::Text(layer.into()), Value::Text(kind.into()), Value::Text(hex_of(colour)), Value::Real(round(z.0)), Value::Real(round(z.1))],
            geom: Some(geom),
            z: None,
        });
    };

    let water_colour = own.get("water").or_else(|| own.get("vatten")).copied().unwrap_or_else(|| soft(hex(WATER), pastel));
    if let Some(b) = &board {
        let side = material(&mut mesh, "board", soft(hex(BOARD_SIDE), pastel), false);
        let top = material(&mut mesh, "water", water_colour, true);
        piece(&mut mesh, &mut out, "board", "water", water_colour, b, (-thick, 0.0), (top, side, Some(side)));
    }
    let earth = material(&mut mesh, "earth", soft(hex(EARTH), pastel), false);
    let sand_colour = soft(hex(SAND), pastel);
    let sand = material(&mut mesh, "sand", sand_colour, false);
    let land_bottom = if board.is_some() { 0.0 } else { -thick };
    piece(&mut mesh, &mut out, "land", "beach", sand_colour, &land, (land_bottom, lift), (sand, earth, board.is_none().then_some(earth)));
    let ground_name = land_kinds.first().map(|k| k.0.clone()).unwrap_or_default();
    let ground_colour = if ground_name.is_empty() && class_at.is_none() { soft(hex(GRASS), pastel) } else { colour_of(&ground_name) };
    let g_top = material(&mut mesh, &format!("ground {}", ground_name), ground_colour, false);
    let g_side = material(&mut mesh, &format!("ground {} side", ground_name), darker(ground_colour), false);
    piece(&mut mesh, &mut out, "ground", &ground_name, ground_colour, &inner, (lift, lift + plate), (g_top, g_side, None));
    let mut placed: Option<MultiPolygon<f64>> = None;
    let mut covers: Vec<(String, MultiPolygon<f64>, f64)> = Vec::new();
    for (name, areas, _) in land_kinds.iter().skip(1) {
        let mut m = soften(areas, radius * 0.5).intersection(&inner);
        if let Some(pl) = &placed {
            m = m.difference(pl);
        }
        let m = MultiPolygon::new(m.0.into_iter().filter(|q| q.unsigned_area() >= smallest).collect());
        if m.0.is_empty() {
            continue;
        }
        let c = colour_of(name);
        let top = material(&mut mesh, name, c, false);
        let side = material(&mut mesh, &format!("{} side", name), darker(c), false);
        piece(&mut mesh, &mut out, "cover", name, c, &m, (lift + plate, lift + plate + thin), (top, side, None));
        covers.push((name.clone(), m.clone(), lift + plate + thin));
        placed = Some(match placed {
            Some(pl) => pl.union(&m),
            None => m,
        });
    }
    // Where the objects stand: on the plates of each kind, and on the ground between them.
    let ground_left = match &placed {
        Some(pl) => inner.difference(pl),
        None => inner.clone(),
    };
    let mut regions = covers;
    regions.push((ground_name.clone(), ground_left, lift + plate));
    // The terrain: terraces of the heights on Heights, as layers of a cake, each rounded,
    // with the kinds of land on them again.
    let mut terraces: Vec<(geo::algorithm::indexed::IntervalTreeMultiPolygon<f64>, f64)> = Vec::new();
    let mut relief_top = 0.0f64;
    if let Some(g) = input(inp, "Heights").grid.as_deref() {
        if let (Some(a), Some(b)) = (g.crs, table.crs) {
            if a != b {
                return err(format!(
                    "The heights are in {} and the areas in {}. Reproject the areas, or read the heights in {}.",
                    crate::model::crs_label(Some(a)),
                    crate::model::crs_label(Some(b)),
                    crate::model::crs_label(Some(b))
                ));
            }
        }
        let steps = number("steps", "The steps")?.unwrap_or(4.0).clamp(1.0, 12.0) as usize;
        let relief = match number("relief", "The height of the terrain")? {
            Some(v) if v <= 0.0 => return err("The height of the terrain must be more than 0 metres."),
            v => v.unwrap_or(thick * 1.6),
        };
        let lr = inner.bounding_rect().unwrap_or(rect);
        let grid = terrace_grid(g, &lr);
        // The heights over the land, low to high.
        let index = geo::algorithm::indexed::IntervalTreeMultiPolygon::new(&inner);
        let mut zs: Vec<f32> = Vec::new();
        {
            use geo::Contains;
            let gt = grid.geotransform;
            for r in 0..grid.height {
                for c in 0..grid.width {
                    let v = grid.values[r * grid.width + c];
                    let (x, y) = (gt[0] + (c as f64 + 0.5) * gt[1], gt[3] + (r as f64 + 0.5) * gt[5]);
                    if !v.is_nan() && index.contains(&geo::coord! { x: x, y: y }) {
                        zs.push(v);
                    }
                }
            }
        }
        zs.sort_by(|a, b| a.total_cmp(b));
        if zs.len() < 4 {
            ctx.notes.warn("The heights on Heights do not cover the land, so it stays flat.");
        } else {
            let lo = zs[0].max(0.0) as f64;
            let hi = zs[((zs.len() - 1) as f64 * 0.99) as usize] as f64;
            let step_h = relief / steps as f64;
            let mut below = inner.clone();
            let side_c = darker(darker(ground_colour));
            let t_side = material(&mut mesh, "terrace side", side_c, false);
            let mut made = 0;
            for k in 1..=steps {
                if hi <= lo {
                    break;
                }
                let level = lo + (hi - lo) * k as f64 / (steps as f64 + 0.5);
                let raw = above_level(&grid, level as f32);
                let mut area = soften(&raw.intersection(&below), radius * 0.6).intersection(&below);
                area = MultiPolygon::new(area.0.into_iter().filter(|q| q.unsigned_area() >= smallest).collect());
                if area.0.is_empty() {
                    break;
                }
                let z0 = lift + plate + (k - 1) as f64 * step_h;
                let z1 = z0 + step_h;
                piece(
                    &mut mesh,
                    &mut out,
                    "terrace",
                    &format!("{} m", crate::model::fmt_real(level.round())),
                    ground_colour,
                    &area,
                    (z0, z1),
                    (g_top, t_side, None),
                );
                for (name, m, _) in &regions[..regions.len() - 1] {
                    let on = m.intersection(&area);
                    let on = MultiPolygon::new(on.0.into_iter().filter(|q| q.unsigned_area() >= smallest).collect());
                    if on.0.is_empty() {
                        continue;
                    }
                    let c = colour_of(name);
                    let top = material(&mut mesh, name, c, false);
                    let side = material(&mut mesh, &format!("{} side", name), darker(c), false);
                    piece(&mut mesh, &mut out, "cover", name, c, &on, (z1, z1 + thin), (top, side, None));
                }
                terraces.push((geo::algorithm::indexed::IntervalTreeMultiPolygon::new(&area), z1 - (lift + plate)));
                below = area;
                relief_top = z1 - (lift + plate);
                made += 1;
            }
            ctx.notes.info(format!("{} terraces from {} to {} m above the sea", made, crate::model::fmt_real(lo.round()), crate::model::fmt_real(hi.round())));
        }
    }
    // How much higher a place is, for the terrace it stands on.
    let up = |x: f64, y: f64| -> f64 {
        use geo::Contains;
        terraces.iter().rev().find(|(t, _)| t.contains(&geo::coord! { x: x, y: y })).map_or(0.0, |(_, h)| *h)
    };
    // Whether a kind is in a list of values given in a setting, or None when none is given.
    let listed = |key: &str, name: &str| -> Option<bool> {
        p.get(key).map(|v| v.split([',', ';']).map(|w| w.trim().to_lowercase()).any(|w| !w.is_empty() && w == name.to_lowercase()))
    };
    let count = |k: &str, what: &str| -> Result<usize> { Ok(number(k, what)?.unwrap_or(0.0).clamp(0.0, 10_000.0).round() as usize) };
    let share = |k: &str, what: &str, d: f64| -> Result<f64> { Ok(number(k, what)?.unwrap_or(d).clamp(0.0, 100.0) / 100.0) };
    let (trees, houses) = (share("trees", "Trees", 50.0)?, share("houses", "Houses", 50.0)?);
    let (boats, clouds) = (count("boats", "Boats")?, count("clouds", "Clouds")?);
    let herd = animals_of(&p.exact("animals"))?;
    let ts = match number("object_size", "The size of a tree")? {
        Some(v) if v <= 0.0 => return err("The size of a tree must be more than 0 metres."),
        v => v.unwrap_or((size * 0.022).max(radius * 0.8)),
    };
    let mut rng = props::Rng::new(number("seed", "The variation")?.unwrap_or(1.0).abs() as u64);
    let mut paint = |m: &mut Mesh, name: &str, c: [f32; 3]| material(m, name, c, false);
    let mut counts: Vec<(&str, usize)> = Vec::new();
    // Each object is a feature: a point where it stands, with its layer and its kind.
    let thing = |mesh: &mut Mesh, out: &mut Table, layer: &str, kind: &str, at: [f64; 3], top: f64, make: &mut dyn FnMut(&mut props::Shapes) -> [f32; 3]| {
        let object = out.features.len() as u32;
        let colour = make(&mut props::Shapes { mesh, object });
        mesh.objects.push(format!("{} {}", layer, object + 1));
        out.features.push(Feature {
            attrs: vec![Value::Text(layer.into()), Value::Text(kind.into()), Value::Text(hex_of(colour)), Value::Real(round(at[2])), Value::Real(round(top))],
            geom: Some(Geometry::Point(geo::Point::new(at[0], at[1]))),
            z: Some(Arc::from(vec![at[2]])),
        });
    };
    let shrink = |m: &MultiPolygon<f64>, by: f64| multi(geom::buffer(&Geometry::MultiPolygon(m.clone()), -by));
    let (mut n_trees, mut n_houses) = (0usize, 0usize);
    for (name, area, z) in &regions {
        let z = *z;
        let conifer = listed("pine_values", name).unwrap_or_else(|| has_word(name, CONIFER_WORDS));
        let round_trees = listed("tree_values", name).unwrap_or_else(|| has_word(name, FOREST_WORDS) && !has_word(name, CONIFER_WORDS));
        let forest = conifer || round_trees;
        let park = p.get("tree_values").is_none() && has_word(name, PARK_WORDS);
        if trees > 0.0 && (forest || park) {
            let spacing = ts * 0.8 / trees.sqrt() * if park { 3.0 } else { 1.0 };
            let mixed = name.to_lowercase().contains("bland") || name.to_lowercase().contains("mixed");
            for (x, y) in scatter(&shrink(area, ts * 0.35), spacing, &mut rng, 8000usize.saturating_sub(n_trees)) {
                let z = z + up(x, y);
                let s = ts * rng.range(0.8, 1.2);
                let kind =
                    if conifer && !(round_trees && rng.next() < 0.5) && !(mixed && rng.next() < 0.35) { props::Tree::Pine } else { props::Tree::Broadleaf };
                let label = if kind == props::Tree::Pine { "pine" } else { "broadleaf" };
                thing(&mut mesh, &mut out, "tree", label, [x, y, z], z + s * 1.1, &mut |sh| props::tree(sh, &mut paint, [x, y, z], s, kind, &mut rng));
                n_trees += 1;
            }
        }
        if houses > 0.0 && listed("house_values", name).unwrap_or_else(|| has_word(name, BUILT_WORDS)) {
            let hs = ts;
            for (x, y) in scatter(&shrink(area, hs * 0.6), hs * 1.25 / houses.sqrt(), &mut rng, 3000usize.saturating_sub(n_houses)) {
                let z = z + up(x, y);
                thing(&mut mesh, &mut out, "house", "house", [x, y, z], z + hs * 1.1, &mut |sh| props::house(sh, &mut paint, [x, y, z], hs, &mut rng));
                n_houses += 1;
            }
        }
    }
    // Say why, when there are none: most often the kinds of land are named otherwise.
    let names = || regions.iter().map(|r| if r.0.is_empty() { "(empty)".to_string() } else { format!("\"{}\"", r.0) }).collect::<Vec<_>>().join(", ");
    if trees > 0.0 && n_trees == 0 {
        ctx.notes.warn(format!(
            "No trees: none of the kinds of land is forest by its name{}. The kinds are {}. Give those that are forest in Pines on or Round trees on.",
            if class_at.is_none() { ", since Kind of land from is not chosen" } else { "" },
            names()
        ));
    }
    if houses > 0.0 && n_houses == 0 && regions.iter().any(|r| p.get("house_values").is_some() || has_word(&r.0, BUILT_WORDS)) {
        ctx.notes.warn("No houses: the built-up areas are too small for them. Make the size of a tree smaller.");
    }
    counts.push(("trees", n_trees));
    counts.push(("houses", n_houses));
    // The animals, each kind where it lives, spread over the areas there by their size.
    for (kind, want_all) in &herd {
        let home: Vec<&(String, MultiPolygon<f64>, f64)> = regions
            .iter()
            .filter(|r| match kind.home {
                props::Home::Grazing => listed("sheep_values", &r.0).unwrap_or_else(|| has_word(&r.0, GRAZING_WORDS)),
                props::Home::Town => listed("house_values", &r.0).unwrap_or_else(|| has_word(&r.0, BUILT_WORDS)),
                props::Home::Forest => has_word(&r.0, FOREST_WORDS) || has_word(&r.0, CONIFER_WORDS),
            })
            .collect();
        // Without such land they are put anywhere on the land.
        let home: Vec<&(String, MultiPolygon<f64>, f64)> = if home.is_empty() {
            ctx.notes.info(format!("no land of their own for the {}: they are put anywhere", kind.name));
            regions.iter().collect()
        } else {
            home
        };
        let total: f64 = home.iter().map(|r| area_of(&r.1)).sum();
        let size = ts * kind.size;
        let mut n = 0;
        for (_, area, z) in &home {
            let z = *z;
            let want = ((area_of(area) / total.max(1e-9)) * *want_all as f64).round() as usize;
            if want == 0 {
                continue;
            }
            let a = shrink(area, size * 0.6);
            let spacing = (area_of(&a) / (want as f64 * 3.0)).sqrt().max(size * 0.8);
            let mut spots = scatter(&a, spacing, &mut rng, usize::MAX);
            let k = pick_some(&mut spots, want, &mut rng);
            for &(x, y) in &spots[..k] {
                let z = z + up(x, y);
                thing(&mut mesh, &mut out, "animal", kind.name, [x, y, z], z + size, &mut |sh| {
                    props::animal(sh, &mut paint, kind.name, [x, y, z], size, &mut rng)
                });
                n += 1;
            }
        }
        if n == 0 {
            ctx.notes.warn(format!("No {}: the land is too small for them.", kind.name));
        }
        counts.push((kind.name, n));
    }
    if boats > 0 {
        match &board {
            Some(b) => {
                let water = shrink(&b.difference(&multi(geom::buffer(&Geometry::MultiPolygon(land.clone()), ts * 0.6))), ts * 0.8);
                let a = area_of(&water);
                let mut spots = scatter(&water, (a / (boats as f64 * 4.0)).sqrt().max(ts), &mut rng, usize::MAX);
                let n = pick_some(&mut spots, boats, &mut rng);
                for &(x, y) in &spots[..n] {
                    thing(&mut mesh, &mut out, "boat", "sailing boat", [x, y, 0.0], ts * 1.1, &mut |sh| {
                        props::boat(sh, &mut paint, [x, y, 0.0], ts * 1.5, &mut rng);
                        [0.98, 0.97, 0.94]
                    });
                }
                counts.push(("boats", n));
            }
            None => ctx.notes.warn("Boats need water: choose a board, whose top is the water."),
        }
    }
    if clouds > 0 {
        // Over the land, high enough to cast their shadows on it, spread out.
        let lr = land.bounding_rect().unwrap_or(rect);
        let cs = size * 0.1;
        let height = lift + plate + relief_top + ts * 7.0;
        let mut spots: Vec<(f64, f64)> = Vec::new();
        for _ in 0..clouds * 40 {
            if spots.len() >= clouds {
                break;
            }
            let (x, y) = (rng.range(lr.min().x, lr.max().x), rng.range(lr.min().y, lr.max().y));
            if spots.iter().all(|(a, b)| (a - x).hypot(b - y) > cs * 0.9) {
                spots.push((x, y));
            }
        }
        for &(x, y) in &spots {
            let z = height * rng.range(0.9, 1.25);
            let s = cs * rng.range(0.75, 1.2);
            thing(&mut mesh, &mut out, "cloud", "cloud", [x, y, z], z + s * 0.5, &mut |sh| {
                props::cloud(sh, &mut paint, [x, y, z], s, &mut rng);
                [0.99, 0.99, 1.0]
            });
        }
        counts.push(("clouds", spots.len()));
    }
    let things: Vec<String> = counts.iter().filter(|c| c.1 > 0).map(|(w, n)| format!("{} {}", fmt_count(*n as u64), w)).collect();
    ctx.notes.info(format!(
        "rounded by {} m, the board {} m thick, {} kinds of land{}, {} triangles",
        crate::model::fmt_real(round(radius)),
        crate::model::fmt_real(round(thick)),
        land_kinds.len(),
        if things.is_empty() { String::new() } else { format!(", {}", things.join(", ")) },
        fmt_count(mesh.tris.len() as u64)
    ));
    out.mesh = Some(Arc::new(mesh));
    Ok(vec![("Diorama".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "DioramaMaker",
    title: "Make diorama",
    hidden: false,
    group: Group::Transformer,
    category: "3D models and views",
    desc: "Turns areas of land cover into a toy landscape, as in a picture book or a board game: a 3D model for Render 3D scene, the 3D view and 3D files. The land is rounded, with inlets and spits narrower than the rounding gone, and stands as a slab on a board whose top is the water, with a beach along the shore. The kinds of land, such as forest, fields and towns, lie on it as thin plates in clear or pastel colours, picked by the words in their names in Swedish or English, or as you give them. The largest kind is the ground. Trees stand in the forest, pines where it is coniferous, houses with bright roofs in towns, animals of many kinds graze on fields and meadows, play in towns and roam the forest, sailing boats lie on the water, and fluffy clouds float above, all casting their shadows. The board can be round. Render it with parallel projection, no floor and a see-through background for a picture like a diorama. The pieces come out as areas with their layer, kind, colour and heights. On Heights goes a terrain model, read by an Input with Cells as heights on a grid, such as a GeoTIFF of Lantmäteriet's elevation model, in the coordinate system of the areas: the land then rises in terraces.",
    inputs: &["Areas", "Heights"],
    outputs: &["Diorama"],
    ports: None,
    params: DIORAMA_PARAMS,
    run: run_diorama,
    summary: |p| {
        let r = p.get("radius").map(|r| format!("rounded {} m", r)).unwrap_or_else(|| "rounded to suit".into());
        format!("{}, {}", r, choice_label(THEMES, &p.get("theme").unwrap_or_else(|| "toy".into())).split(':').next().unwrap_or("").to_lowercase())
    },
}];

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Polygon<f64> {
        Polygon::new(geo::LineString::from(vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s), (x, y)]), vec![])
    }

    fn areas(parts: &[(&str, Polygon<f64>)]) -> Table {
        Table {
            fields: vec![Field::new("kind", FieldType::Text)],
            crs: Some(3006),
            features: parts
                .iter()
                .map(|(k, p)| Feature { attrs: vec![Value::Text(k.to_string())], geom: Some(Geometry::Polygon(p.clone())), z: None })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn kinds_of_land_get_their_colours_by_their_names() {
        assert_eq!(hex_of(kind_colour("Barr- och blandskog")), "#2f8f4e");
        assert_eq!(hex_of(kind_colour("Åker")), "#d6dc6a");
        assert_eq!(hex_of(kind_colour("forest")), "#4daa4f");
        assert!(is_water_word("Sjö") && is_water_word("Hav") && is_water_word("water") && !is_water_word("Åker"));
        // Unknown kinds get one of the others, the same each time.
        assert_eq!(kind_colour("Xyz"), kind_colour("xyz"));
    }

    #[test]
    fn an_island_becomes_a_rounded_slab_on_a_board_of_water() {
        let t = areas(&[("Skog", square(0.0, 0.0, 1000.0)), ("Åker", square(200.0, 200.0, 300.0)), ("Hav", square(-500.0, -500.0, 2000.0))]);
        let (out, notes) = run_for_test("DioramaMaker", &[("class", "kind"), ("radius", "50"), ("trees", "0")], vec![("Areas", t)]).unwrap();
        let d = &out[0].1;
        let layers: Vec<String> = d.features.iter().map(|f| f.attrs[0].to_text()).collect();
        assert_eq!(layers, ["board", "land", "ground", "cover"], "{:?}", notes);
        let mesh = d.mesh.as_ref().unwrap();
        assert_eq!(mesh.objects.len(), d.features.len());
        // The forest is the ground, the field lies on it.
        assert_eq!(d.features[2].attrs[1].to_text(), "Skog");
        assert_eq!(d.features[3].attrs[1].to_text(), "Åker");
        // The board is the extent of the sea, its top the water at 0, its bottom 5% of 2 km down.
        assert_eq!(d.features[0].attrs[3].as_f64(), Some(-100.0));
        let (lo, hi) = mesh.bounds().unwrap();
        assert!((lo[2] + 100.0).abs() < 1e-6 && hi[2] > 60.0, "{:?} {:?}", lo, hi);
        // The corners of the island are rounded: its area is less than the square's, but not by much.
        let land = geom::area(d.features[1].geom.as_ref().unwrap(), false);
        assert!(land < 1_000_000.0 && land > 990_000.0, "{}", land);
        // Every wall faces out: the normals of the sides of the land point away from its middle.
        let water = mesh.materials.iter().position(|m| m.name == "water").unwrap();
        assert!(mesh.materials[water].specular[0] > 0.0);
    }

    #[test]
    fn without_a_board_the_land_floats_and_has_a_bottom() {
        let t = areas(&[("forest", square(0.0, 0.0, 1000.0))]);
        let (out, _) = run_for_test("DioramaMaker", &[("class", "kind"), ("board", "none"), ("beach", "0"), ("trees", "0")], vec![("Areas", t)]).unwrap();
        let d = &out[0].1;
        assert_eq!(d.features.iter().map(|f| f.attrs[0].to_text()).collect::<Vec<_>>(), ["land", "ground"]);
        let mesh = d.mesh.as_ref().unwrap();
        assert!(mesh.normals.iter().any(|n| n[2] < -0.99), "a bottom");
    }

    #[test]
    fn round_shapes_rounds_corners_and_cuts_narrow_spits() {
        // A square with a spit 20 m wide sticking out of it.
        let spit = Polygon::new(geo::LineString::from(vec![(1000.0, 480.0), (1600.0, 480.0), (1600.0, 500.0), (1000.0, 500.0), (1000.0, 480.0)]), vec![]);
        let shape = square(0.0, 0.0, 1000.0).union(&spit);
        let t = Table {
            fields: vec![],
            crs: Some(3006),
            features: vec![Feature { attrs: vec![], geom: Some(Geometry::MultiPolygon(shape)), z: None }],
            ..Default::default()
        };
        let (out, _) = run_for_test("ShapeRounder", &[("radius", "50")], vec![("Input", t)]).unwrap();
        let g = out[0].1.features[0].geom.as_ref().unwrap();
        let r = geo::BoundingRect::bounding_rect(g).unwrap();
        assert!(r.max().x < 1001.0, "the spit is gone: {:?}", r);
        // Each corner loses the area between the square and a quarter circle: (4 - π) r².
        let a = geom::area(g, false);
        let lost = 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 2500.0;
        assert!((1_000_000.0 - a - lost).abs() < lost * 0.1, "{}", a);
    }

    #[test]
    fn features_are_created_from_coordinates_and_wkt() {
        let (out, _) = run_for_test(
            "FeatureCreator",
            &[("kind", "polygon"), ("coordinates", "0 0, 100 0, 100 100, 0 100\n# a comment\n200,200; 300,200; 300,300"), ("name", "A, B")],
            vec![],
        )
        .unwrap();
        let t = &out[0].1;
        assert_eq!(t.crs, Some(3006));
        assert_eq!(t.features.len(), 2);
        assert_eq!(geom::area(t.features[0].geom.as_ref().unwrap(), false), 10_000.0);
        assert_eq!(t.features[1].attrs[1].to_text(), "B");
        let (out, _) = run_for_test("FeatureCreator", &[("kind", "point"), ("coordinates", "POINT Z (1 2 3)\n5 6"), ("crs", "3007")], vec![]).unwrap();
        let t = &out[0].1;
        assert_eq!(t.crs, Some(3007));
        assert_eq!(t.features[0].heights(), Some(&[3.0][..]));
        assert!(matches!(t.features[1].geom, Some(Geometry::Point(_))));
        let e = run_for_test("FeatureCreator", &[("kind", "line"), ("coordinates", "1 2 3")], vec![]).unwrap_err();
        assert!(e.0.contains("Line 1") && e.0.contains("pairs"), "{}", e.0);
    }

    #[test]
    fn features_with_heights_go_to_3d_and_flat_ones_to_2d() {
        let flat = Feature { attrs: vec![Value::Text("a".into())], geom: Some(Geometry::Point(geo::Point::new(1.0, 2.0))), z: None };
        let high = Feature { attrs: vec![Value::Text("b".into())], geom: Some(Geometry::Point(geo::Point::new(1.0, 2.0))), z: Some(Arc::from(vec![5.0])) };
        let none = Feature { attrs: vec![Value::Text("c".into())], geom: None, z: None };
        let t = Table { fields: vec![Field::new("kind", FieldType::Text)], crs: Some(3006), features: vec![flat, high, none], ..Default::default() };
        let (out, _) = run_for_test("DimensionFilter", &[], vec![("Input", t)]).unwrap();
        let count = |port: &str| out.iter().find(|o| o.0 == port).unwrap().1.features.len();
        assert_eq!((count("3D"), count("2D"), count("No geometry")), (1, 1, 1));
        // The objects of a 3D model are 3D.
        let island = areas(&[("forest", square(0.0, 0.0, 1000.0))]);
        let (d, _) = run_for_test("DioramaMaker", &[("class", "kind")], vec![("Areas", island)]).unwrap();
        let n = d[0].1.features.len();
        let (out, _) = run_for_test("DimensionFilter", &[], vec![("Input", d[0].1.clone())]).unwrap();
        assert_eq!(out.iter().find(|o| o.0 == "3D").unwrap().1.features.len(), n);
    }

    #[test]
    fn trees_houses_sheep_boats_and_clouds_stand_where_they_belong() {
        let t = areas(&[
            ("Barrskog", square(0.0, 0.0, 2000.0)),
            ("Bebyggelse", square(1200.0, 1200.0, 600.0)),
            ("Åker", square(200.0, 200.0, 700.0)),
            ("Hav", square(-1500.0, -1500.0, 5000.0)),
        ]);
        let params = [("class", "kind"), ("board", "round"), ("animals", "6"), ("boats", "3"), ("clouds", "2"), ("trees", "60"), ("houses", "80")];
        let (out, notes) = run_for_test("DioramaMaker", &params, vec![("Areas", t.clone())]).unwrap();
        let d = &out[0].1;
        let layer = |name: &str| d.features.iter().filter(|f| f.attrs[0].to_text() == name).collect::<Vec<_>>();
        let (trees, houses, sheep, boats, clouds) = (layer("tree"), layer("house"), layer("animal"), layer("boat"), layer("cloud"));
        assert!(sheep.iter().all(|f| f.attrs[1].to_text() == "sheep"));
        assert!(trees.len() > 20 && !houses.is_empty(), "{} {} {:?}", trees.len(), houses.len(), notes);
        assert_eq!((sheep.len(), boats.len(), clouds.len()), (6, 3, 2));
        let at = |f: &Feature| match f.geom {
            Some(Geometry::Point(p)) => (p.x(), p.y()),
            _ => panic!("a point"),
        };
        // Pines in the coniferous forest, not in the town or on the field; houses in the town.
        for f in &trees {
            let (x, y) = at(f);
            assert!(!(x > 1200.0 && x < 1800.0 && y > 1200.0 && y < 1800.0) && !(x > 200.0 && x < 900.0 && y > 200.0 && y < 900.0), "{} {}", x, y);
            assert_eq!(f.attrs[1].to_text(), "pine");
        }
        for f in houses.iter().chain(&sheep) {
            let (x, y) = at(f);
            let town = x > 1200.0 && x < 1800.0 && y > 1200.0 && y < 1800.0;
            let field = x > 200.0 && x < 900.0 && y > 200.0 && y < 900.0;
            assert!(if f.attrs[0].to_text() == "house" { town } else { field }, "{} at {} {}", f.attrs[0].to_text(), x, y);
        }
        // Animals of many kinds, each where it lives.
        let mut zoo = params.to_vec();
        zoo.retain(|p| p.0 != "animals");
        zoo.push(("animals", "pigs 3, 2 hästar\ndinosaur 1\nkatt = 4\nbear 2"));
        let (out, _) = run_for_test("DioramaMaker", &zoo, vec![("Areas", t.clone())]).unwrap();
        let kinds: Vec<(String, (f64, f64))> =
            out[0].1.features.iter().filter(|f| f.attrs[0].to_text() == "animal").map(|f| (f.attrs[1].to_text(), at(f))).collect();
        let n = |k: &str| kinds.iter().filter(|a| a.0 == k).count();
        assert_eq!((n("pig"), n("horse"), n("dinosaur"), n("cat"), n("bear")), (3, 2, 1, 4, 2), "{:?}", kinds);
        for (k, (x, y)) in &kinds {
            let town = *x > 1200.0 && *x < 1800.0 && *y > 1200.0 && *y < 1800.0;
            if k == "cat" {
                assert!(town, "a cat at {} {}", x, y);
            }
            if k == "bear" {
                assert!(!town, "a bear in town at {} {}", x, y);
            }
        }
        assert!(run_for_test("DioramaMaker", &[("class", "kind"), ("animals", "unicorn 3")], vec![("Areas", t.clone())]).unwrap_err().0.contains("unicorn"));
        // Boats on the water, off the land; clouds above everything.
        for f in &boats {
            let (x, y) = at(f);
            assert!(!(-50.0..2050.0).contains(&x) || !(-50.0..2050.0).contains(&y), "{} {}", x, y);
        }
        let land_top = d.features.iter().filter(|f| f.attrs[0].to_text() == "cover").map(|f| f.attrs[4].as_f64().unwrap()).fold(0.0, f64::max);
        assert!(clouds.iter().all(|f| f.attrs[3].as_f64().unwrap() > land_top * 2.0));
        // The model has an object for every feature, and the round board is a circle.
        assert_eq!(d.mesh.as_ref().unwrap().objects.len(), d.features.len());
        let board = geom::area(d.features[0].geom.as_ref().unwrap(), false);
        let r = (board / std::f64::consts::PI).sqrt();
        let rect = geo::BoundingRect::bounding_rect(d.features[0].geom.as_ref().unwrap()).unwrap();
        assert!((rect.width() / 2.0 - r).abs() < r * 0.01, "{} {}", rect.width(), r);
        // The same variation puts them in the same places, another elsewhere.
        let (again, _) = run_for_test("DioramaMaker", &params, vec![("Areas", t.clone())]).unwrap();
        assert_eq!(again[0].1.features.len(), d.features.len());
        let mut other = params.to_vec();
        other.push(("seed", "2"));
        let (moved, _) = run_for_test("DioramaMaker", &other, vec![("Areas", t)]).unwrap();
        let first_tree = |t: &Table| t.features.iter().find(|f| f.attrs[0].to_text() == "tree").map(at);
        assert_eq!(first_tree(&again[0].1), first_tree(d));
        assert_ne!(first_tree(&moved[0].1), first_tree(d));
    }

    #[test]
    fn a_diorama_is_rendered_on_a_colour_with_a_title_and_warm_light() {
        let t = areas(&[("Skog", square(0.0, 0.0, 1000.0))]);
        let (d, _) = run_for_test("DioramaMaker", &[("class", "kind"), ("trees", "0")], vec![("Areas", t)]).unwrap();
        let shot = |mood: &str| {
            let params = [
                ("width", "160"),
                ("height", "120"),
                ("quality", "4"),
                ("projection", "parallel"),
                ("floor", "false"),
                ("background", "colour"),
                ("background_colour", "#336699"),
                ("mood", mood),
                ("title", "Ön"),
            ];
            let (out, _) = run_for_test("SceneRenderer", &params, vec![("Models", d[0].1.clone())]).unwrap();
            out[0].1.image.as_ref().unwrap().as_ref().clone()
        };
        let img = shot("neutral");
        // The corner is the colour, solid.
        let px = |x: usize, y: usize| &img.rgba[(y * img.width + x) * 4..][..4];
        assert_eq!(px(1, img.height - 2), &[0x33, 0x66, 0x99, 255]);
        // The title is written near the top: light letters on the dark colour.
        let lit = (0..img.width * 20).filter(|i| img.rgba[i * 4] > 200).count();
        assert!(lit > 10, "{}", lit);
        // Warm light makes the land warmer: more red against blue in the middle.
        let warm = shot("warm");
        let tint = |im: &crate::model::RasterImage| {
            let i = ((im.height / 2) * im.width + im.width / 2) * 4;
            im.rgba[i] as f32 - im.rgba[i + 2] as f32
        };
        assert!(tint(&warm) > tint(&img), "{} {}", tint(&warm), tint(&img));
    }

    #[test]
    fn without_forest_by_name_trees_are_asked_for_by_value() {
        let t = areas(&[("Klass 11", square(0.0, 0.0, 1000.0)), ("Klass 3", square(100.0, 100.0, 300.0))]);
        let (out, notes) = run_for_test("DioramaMaker", &[("class", "kind")], vec![("Areas", t.clone())]).unwrap();
        assert!(out[0].1.features.iter().all(|f| f.attrs[0].to_text() != "tree"));
        assert!(notes.warnings.iter().any(|w| w.contains("No trees") && w.contains("\"Klass 11\"")), "{:?}", notes.warnings);
        let (out, _) = run_for_test("DioramaMaker", &[("class", "kind"), ("pine_values", "klass 11")], vec![("Areas", t)]).unwrap();
        assert!(out[0].1.features.iter().filter(|f| f.attrs[0].to_text() == "tree").all(|f| f.attrs[1].to_text() == "pine"));
        assert!(out[0].1.features.iter().any(|f| f.attrs[0].to_text() == "tree"));
    }

    #[test]
    fn a_diorama_written_as_a_picture_asks_for_render_3d_scene() {
        let (d, _) = run_for_test("DioramaMaker", &[("class", "kind"), ("trees", "0")], vec![("Areas", areas(&[("Skog", square(0.0, 0.0, 1000.0))]))]).unwrap();
        let e = run_for_test("Writer", &[("path", "nowhere/diorama.jpg")], vec![("Input", d[0].1.clone())]).unwrap_err();
        assert!(e.0.contains("Put Render 3D scene between them"), "{}", e.0);
    }

    #[test]
    fn all_water_is_said_plainly() {
        let t = areas(&[("Sjö", square(0.0, 0.0, 1000.0))]);
        let e = run_for_test("DioramaMaker", &[("class", "kind")], vec![("Areas", t)]).unwrap_err();
        assert!(e.0.contains("All the areas are water"), "{}", e.0);
    }

    #[test]
    fn own_colours_are_used() {
        let t = areas(&[("Skog", square(0.0, 0.0, 1000.0))]);
        let (out, _) = run_for_test("DioramaMaker", &[("class", "kind"), ("colours", "skog=#123456")], vec![("Areas", t)]).unwrap();
        assert_eq!(out[0].1.features.iter().find(|f| f.attrs[0].to_text() == "ground").unwrap().attrs[2].to_text(), "#123456");
        let bad =
            run_for_test("DioramaMaker", &[("class", "kind"), ("colours", "skog")], vec![("Areas", areas(&[("Skog", square(0.0, 0.0, 10.0))]))]).unwrap_err();
        assert!(bad.0.contains("value=#rrggbb"), "{}", bad.0);
    }
}
