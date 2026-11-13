//! Buildings that look like buildings, added in 0.96.3: footprints raised to their height
//! from the number of storeys, with the roof their attributes give (flat, shed, gable or
//! hipped), facades with rows of windows a storey and a bay at a time, and the colours and
//! materials of their kind, such as plaster for houses, glass for offices and sheet metal for
//! industry. The attributes of OpenStreetMap, Overture and the property map are recognised.

use super::geom_inspect::column;
use super::geom_transform::number_param;
use super::models091::{planes_of, Frame, Roof};
use super::*;
use crate::mesh::{Material, Mesh, Texture, Tri, NONE};
use crate::model::{err, fmt_count, Feature};
use geo::{BooleanOps, Coord, MultiPolygon, Polygon, TriangulateEarcut};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

const ROOF_DEFAULTS: &[(&str, &str)] =
    &[("auto", "Gable on houses of up to three storeys and small buildings, else flat"), ("flat", "Flat"), ("gabled", "Gable"), ("hipped", "Hipped")];

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "kind_by",
        label: "Kind by",
        kind: ParamKind::Attribute("Buildings"),
        default: "",
        hint: "What the building is, such as house, apartments, office, industrial, church or garage, or Bostad and Industri. Empty: building, class or objekttyp, whichever there is",
    },
    ParamSpec {
        key: "height_by",
        label: "Height by",
        kind: ParamKind::Attribute("Buildings"),
        default: "",
        hint: "The height to the top of the roof in metres. Empty: height, if there is one; else from the storeys",
    },
    ParamSpec {
        key: "levels_by",
        label: "Storeys by",
        kind: ParamKind::Attribute("Buildings"),
        default: "",
        hint: "The number of storeys below the roof. Empty: building:levels, num_floors or levels, whichever there is",
    },
    ParamSpec {
        key: "roof_by",
        label: "Roof shape by",
        kind: ParamKind::Attribute("Buildings"),
        default: "",
        hint: "flat, skillion, gabled, hipped, pyramidal and the other shapes of OpenStreetMap. Empty: roof:shape or roof_shape",
    },
    ParamSpec {
        key: "levels",
        label: "Storeys when not known",
        kind: ParamKind::Text,
        default: "2",
        hint: "For buildings with neither a height nor storeys",
    },
    ParamSpec { key: "storey", label: "Height of a storey", kind: ParamKind::Text, default: "3", hint: "In metres, from floor to floor" },
    ParamSpec {
        key: "roof",
        label: "Roof when not known",
        kind: ParamKind::Choice(ROOF_DEFAULTS),
        default: "auto",
        hint: "For buildings whose roof shape is not given",
    },
    ParamSpec {
        key: "pitch",
        label: "Pitch of the roofs",
        kind: ParamKind::Text,
        default: "35",
        hint: "In degrees, for sloping roofs whose height is not given",
    },
    ParamSpec {
        key: "windows",
        label: "Windows",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "Facades with a row of windows on each storey, as a texture. No: plain walls in the colours of their kind",
    },
];

/// The look of a facade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Style {
    /// Plaster or brick with a window in each bay.
    House,
    /// Bands of glass between the floors.
    Office,
    /// Sheet metal in ribs, without windows.
    Industry,
    /// Stone with tall, narrow windows.
    Civic,
    /// Plain walls, as of a garage or a shed.
    Plain,
}

impl Style {
    fn name(&self) -> &'static str {
        match self {
            Style::House => "house",
            Style::Office => "office",
            Style::Industry => "industry",
            Style::Civic => "civic",
            Style::Plain => "plain",
        }
    }
    /// The colour of the walls and of the roof, flat and sloping.
    fn colours(&self) -> ([u8; 3], [u8; 3], [u8; 3]) {
        match self {
            Style::House => ([0xe6, 0xdc, 0xcb], [0x6a, 0x6a, 0x68], [0x8a, 0x3b, 0x2e]),
            Style::Office => ([0xc9, 0xcb, 0xcc], [0x6b, 0x6b, 0x6b], [0x4a, 0x4f, 0x55]),
            Style::Industry => ([0xa9, 0xb0, 0xb5], [0x7d, 0x83, 0x89], [0x6f, 0x76, 0x7d]),
            Style::Civic => ([0xcf, 0xc3, 0xad], [0x6b, 0x6b, 0x6b], [0x4a, 0x4d, 0x52]),
            Style::Plain => ([0xb9, 0xa5, 0x8c], [0x5a, 0x5a, 0x5a], [0x4b, 0x4b, 0x4b]),
        }
    }
}

/// The style of a kind of building, as OpenStreetMap, Overture or the property map name it.
fn style_of(kind: &str) -> Style {
    let k = kind.trim().to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| k.contains(w));
    if has(&["garage", "shed", "carport", "hut", "roof", "parking", "service", "bunker", "komplement", "uthus", "förråd"]) {
        Style::Plain
    } else if has(&["church", "cathedral", "chapel", "mosque", "temple", "synagogue", "religious", "kyrk", "shrine", "monastery"]) {
        Style::Civic
    } else if has(&["industr", "warehouse", "manufacture", "hangar", "barn", "farm_auxiliary", "storage", "ekonomi", "lager", "stable", "silo"]) {
        Style::Industry
    } else if has(&[
        "office",
        "commercial",
        "retail",
        "supermarket",
        "hospital",
        "university",
        "school",
        "college",
        "public",
        "government",
        "civic",
        "train_station",
        "transportation",
        "verksamhet",
        "samhällsfunktion",
        "kontor",
        "handel",
        "sports",
        "stadium",
    ]) {
        Style::Office
    } else {
        Style::House
    }
}

/// A colour named as OpenStreetMap has it, or as #rrggbb.
fn named_colour(text: &str) -> Option<[u8; 3]> {
    let t = text.trim().to_lowercase();
    if let Ok(Some([r, g, b, _])) = crate::mapdraw::parse_color(&t) {
        return Some([r, g, b]);
    }
    Some(match t.as_str() {
        "white" => [0xf2, 0xf0, 0xea],
        "grey" | "gray" => [0x9a, 0x9a, 0x9a],
        "lightgrey" | "lightgray" | "silver" => [0xc8, 0xc8, 0xc8],
        "darkgrey" | "darkgray" => [0x5c, 0x5c, 0x5c],
        "black" => [0x2a, 0x2a, 0x2a],
        "red" => [0xa8, 0x3a, 0x2c],
        "darkred" | "maroon" => [0x6e, 0x24, 0x1e],
        "brown" => [0x7a, 0x52, 0x38],
        "beige" | "tan" => [0xd8, 0xc8, 0xa4],
        "yellow" => [0xe3, 0xc5, 0x6a],
        "orange" => [0xd5, 0x85, 0x3c],
        "green" => [0x6f, 0x8a, 0x5a],
        "blue" => [0x5a, 0x74, 0x9a],
        "pink" => [0xe0, 0xae, 0xa8],
        _ => return None,
    })
}

/// The colour of a material of walls or roofs.
fn material_colour(text: &str, roof: bool) -> Option<[u8; 3]> {
    let t = text.trim().to_lowercase();
    Some(match t.as_str() {
        "brick" => [0x9c, 0x4a, 0x35],
        "wood" | "timber_framing" => [0xb0, 0x8a, 0x5a],
        "concrete" => [0xbd, 0xbb, 0xb5],
        "stone" | "sandstone" | "limestone" => [0xb8, 0xad, 0x9a],
        "plaster" => [0xe8, 0xe0, 0xd0],
        "metal" | "steel" | "metal_sheet" => {
            if roof {
                [0x6f, 0x76, 0x7d]
            } else {
                [0xa9, 0xb0, 0xb5]
            }
        }
        "glass" => [0x8f, 0xa8, 0xb8],
        "roof_tiles" | "tile" | "tiles" | "clay" => [0x9a, 0x45, 0x32],
        "slate" => [0x4a, 0x4d, 0x52],
        "grass" => [0x6f, 0x8f, 0x4a],
        "thatch" => [0xa0, 0x8a, 0x5a],
        "copper" => [0x5f, 0x8f, 0x7a],
        "tar_paper" | "asphalt" | "roofing_felt" | "bitumen" => [0x4f, 0x4f, 0x4f],
        _ => return None,
    })
}

/// The attribute of a feature by the first of the names that the stream has, ignoring case.
struct Lookup {
    slot: Option<usize>,
}

impl Lookup {
    fn new(table: &Table, chosen: Option<String>, names: &[&str]) -> Result<Lookup> {
        if let Some(c) = chosen.filter(|c| !c.trim().is_empty()) {
            return Ok(Lookup { slot: Some(table.need_field(c.trim())?) });
        }
        let slot = names.iter().find_map(|n| table.fields.iter().position(|f| f.name.eq_ignore_ascii_case(n)));
        Ok(Lookup { slot })
    }
    fn text(&self, f: &Feature) -> Option<String> {
        let v = f.attrs.get(self.slot?)?;
        let t = v.to_text();
        (!t.trim().is_empty()).then_some(t)
    }
    fn number(&self, f: &Feature) -> Option<f64> {
        let v = f.attrs.get(self.slot?)?;
        match v {
            Value::Text(t) => {
                // Such as "12 m" or "12.5".
                let n: String = t.trim().trim_end_matches('m').trim().replace(',', ".");
                n.parse::<f64>().ok()
            }
            _ => v.as_f64(),
        }
        .filter(|x| x.is_finite())
    }
}

/// A texture of one bay and one storey of a facade, 64 × 64 pixels, in the colour of the walls.
fn facade_texture(style: Style, wall: [u8; 3]) -> Texture {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    let shade =
        |c: [u8; 3], f: f32| -> [u8; 3] { [(c[0] as f32 * f).min(255.0) as u8, (c[1] as f32 * f).min(255.0) as u8, (c[2] as f32 * f).min(255.0) as u8] };
    // A little unevenness, the same in every tile.
    let grain = |x: usize, y: usize| -> f32 {
        let h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
        let h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        0.97 + ((h >> 24) as f32 / 255.0) * 0.06
    };
    let frame = [0xee, 0xec, 0xe6];
    for y in 0..N {
        for x in 0..N {
            // y from the top of the storey down; h is the height above the floor, of 3 m.
            let h = (N - y) as f32 / N as f32 * 3.0;
            let along = (x as f32 + 0.5) / N as f32;
            let glass = |top: f32| -> [u8; 3] {
                // Darker below, with the sky in the upper part.
                let t = ((h - (top - 1.6)) / 1.6).clamp(0.0, 1.0);
                [(0x2e as f32 + t * 40.0) as u8, (0x3a as f32 + t * 45.0) as u8, (0x48 as f32 + t * 50.0) as u8]
            };
            let c = match style {
                Style::House => {
                    // A window of 1.2 × 1.4 m with a frame and a post in the middle.
                    let (x0, x1, h0, h1) = (0.3, 0.7, 0.9, 2.3);
                    if along >= x0 && along <= x1 && h >= h0 && h <= h1 {
                        let edge = along < x0 + 0.03 || along > x1 - 0.03 || h < h0 + 0.08 || h > h1 - 0.08 || (along - 0.5).abs() < 0.015;
                        if edge {
                            frame
                        } else {
                            glass(h1)
                        }
                    } else if h >= h0 - 0.08 && h < h0 && along >= x0 - 0.03 && along <= x1 + 0.03 {
                        shade(wall, 0.8)
                    } else {
                        shade(wall, grain(x, y))
                    }
                }
                Style::Office => {
                    // Glass from 0.8 to 2.7 m, with mullions every 0.75 m.
                    if (0.8..=2.7).contains(&h) {
                        if (along * 4.0).fract() < 0.04 {
                            [0x9a, 0x9e, 0xa2]
                        } else {
                            let g = glass(2.7);
                            [g[0].saturating_add(18), g[1].saturating_add(26), g[2].saturating_add(34)]
                        }
                    } else {
                        shade(wall, grain(x, y))
                    }
                }
                Style::Industry => {
                    // Ribs every 0.19 m.
                    let rib = (along * 16.0).fract();
                    shade(
                        wall,
                        if rib < 0.25 {
                            0.86
                        } else if rib < 0.5 {
                            1.04
                        } else {
                            0.97
                        },
                    )
                }
                Style::Civic => {
                    // A tall window with a round head.
                    let (cx, r, h0, h1) = (0.5, 0.11, 0.6, 2.3);
                    let dx = (along - cx) * 3.0;
                    let inside = dx.abs() <= r * 3.0 && h >= h0 && (h <= h1 - r * 3.0 || dx.hypot(h - (h1 - r * 3.0)) <= r * 3.0);
                    if inside {
                        glass(h1)
                    } else {
                        shade(wall, grain(x, y) * if (h * 3.0).fract() < 0.03 { 0.9 } else { 1.0 })
                    }
                }
                Style::Plain => shade(wall, grain(x, y)),
            };
            let i = (y * N + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    Texture { width: N, height: N, rgba }
}

fn srgb(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// The materials made so far, one for each look and colour.
#[derive(Default)]
struct Palette {
    made: HashMap<(String, [u8; 3]), u32>,
}

impl Palette {
    fn get(&mut self, mesh: &mut Mesh, what: &str, colour: [u8; 3], texture: Option<Style>) -> u32 {
        // Colours a little apart are the same, so that there are not too many materials.
        let q = colour.map(|c| ((c as u32 + 4) / 8 * 8).min(255) as u8);
        if let Some(m) = self.made.get(&(what.to_string(), q)) {
            return *m;
        }
        let name = format!("{} #{:02x}{:02x}{:02x}", what, q[0], q[1], q[2]);
        let mut m = Material { name, diffuse: srgb(q), ..Default::default() };
        if let Some(style) = texture {
            // The colour is in the texture; the material is white, so that it is not twice.
            m.texture = Some(Arc::new(facade_texture(style, q)));
            m.diffuse = [1.0; 3];
        }
        if what.contains("glass") {
            m.specular = [0.5; 3];
        }
        mesh.materials.push(m);
        let k = mesh.materials.len() as u32 - 1;
        self.made.insert((what.to_string(), q), k);
        k
    }
}

/// The materials of a building.
struct Looks {
    facade: u32,
    wall: u32,
    roof: u32,
    textured: bool,
}

/// What a building is made of: where it stands, its walls, its roof and its look.
struct Plan {
    ground: f64,
    eaves: f64,
    roof: Roof,
    roof_height: f64,
    storeys: f64,
}

/// The roof of a shape as OpenStreetMap names it, at the eaves `e` and `h` higher at the top.
/// The second is whether the shape was only approached.
fn roof_of(shape: &str, across: bool, e: f64, h: f64, frame: &Frame) -> (Roof, bool) {
    let (l, w) = (frame.l.max(0.01), frame.w.max(0.01));
    let gable = || if across { Roof::Gable { a: e, b: h / l, along: false } } else { Roof::Gable { a: e, b: h / w, along: true } };
    if h <= 0.01 {
        return (Roof::Flat(e), false);
    }
    match shape.trim().to_lowercase().as_str() {
        "flat" | "" => (Roof::Flat(e), false),
        "skillion" | "lean_to" | "shed" | "pent" => (Roof::Shed(e + h / 2.0, 0.0, -h / (2.0 * w)), false),
        "gabled" | "gable" | "saddle" | "sadeltak" => (gable(), false),
        "hipped" | "hip" | "pyramidal" | "valmat" | "tältak" => (Roof::Hip(e, h / w), false),
        "gambrel" | "saltbox" | "sawtooth" | "mansard_gable" => (gable(), true),
        "half-hipped" | "half_hipped" | "mansard" | "round" | "dome" | "onion" | "cone" | "side_hipped" | "crosspitched" => (Roof::Hip(e, h / w), true),
        _ => (gable(), true),
    }
}

/// Adds a building: the roof a plane at a time, the walls in two parts, the facade up to the
/// eaves with its windows and the gables above it plain, and a floor.
fn add_house(mesh: &mut Mesh, poly: &Polygon<f64>, frame: &Frame, plan: &Plan, looks: &Looks, bay: f64, object: u32) {
    use geo::Winding;
    let ground = plan.ground;
    let z_at = |c: Coord<f64>| {
        let (u, v) = frame.uv(c.x, c.y);
        plan.roof.z(u, v, frame.l, frame.w).max(plan.eaves).min(plan.eaves + plan.roof_height)
    };
    let fp = MultiPolygon::new(vec![poly.clone()]);
    let mut on_outline: Vec<Coord<f64>> = Vec::new();
    for part in planes_of(&plan.roof, frame.l, frame.w) {
        let mut ring: Vec<Coord<f64>> = part.iter().map(|(u, v)| frame.xy(*u, *v)).collect();
        ring.push(ring[0]);
        let cut = fp.intersection(&MultiPolygon::new(vec![Polygon::new(geo::LineString(ring), vec![])]));
        for piece in &cut.0 {
            on_outline.extend(piece.exterior().0.iter().copied());
            let raw = piece.earcut_triangles_raw();
            let base = mesh.positions.len() as u32;
            mesh.positions.extend(raw.vertices.iter().map(|c| [c[0], c[1], z_at(Coord { x: c[0], y: c[1] })]));
            for t in raw.triangle_indices.as_chunks::<3>().0.iter() {
                let v = |i: usize| (raw.vertices[i][0], raw.vertices[i][1]);
                let (pa, pb, pc) = (v(t[0]), v(t[1]), v(t[2]));
                let ccw = (pb.0 - pa.0) * (pc.1 - pa.1) - (pb.1 - pa.1) * (pc.0 - pa.0) > 0.0;
                let (b, c) = if ccw { (t[1], t[2]) } else { (t[2], t[1]) };
                mesh.tris.push(Tri { v: [base + t[0] as u32, base + b as u32, base + c as u32], n: [NONE; 3], t: [NONE; 3], material: looks.roof, object });
            }
        }
    }
    let mut rings: Vec<geo::LineString<f64>> = Vec::new();
    let mut outer = poly.exterior().clone();
    outer.make_ccw_winding();
    rings.push(outer);
    for r in poly.interiors() {
        let mut r = r.clone();
        r.make_cw_winding();
        rings.push(r);
    }
    let wall_h = plan.eaves - ground;
    for ring in &rings {
        for wpair in ring.0.windows(2) {
            let (a, b) = (wpair[0], wpair[1]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            if len2 < 1e-12 {
                continue;
            }
            let len = len2.sqrt();
            // Whole bays along the wall and whole storeys up it, so that windows are not cut.
            let bays = (len / bay).round().max(1.0);
            let windows = looks.textured && len >= 1.6 && wall_h >= 2.2;
            let mut ts: Vec<f64> = vec![0.0, 1.0];
            for c in &on_outline {
                let t = ((c.x - a.x) * dx + (c.y - a.y) * dy) / len2;
                if t > 1e-6 && t < 1.0 - 1e-6 && (a.x + dx * t - c.x).hypot(a.y + dy * t - c.y) < 1e-6 {
                    ts.push(t);
                }
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            for k in ts.windows(2) {
                let p = Coord { x: a.x + dx * k[0], y: a.y + dy * k[0] };
                let q = Coord { x: a.x + dx * k[1], y: a.y + dy * k[1] };
                // The facade, from the ground to the eaves.
                let v = mesh.positions.len() as u32;
                mesh.positions.extend_from_slice(&[[p.x, p.y, ground], [q.x, q.y, ground], [q.x, q.y, plan.eaves], [p.x, p.y, plan.eaves]]);
                let (material, t) = if windows {
                    let uv = mesh.uvs.len() as u32;
                    let (u0, u1) = ((k[0] * bays) as f32, (k[1] * bays) as f32);
                    let top = plan.storeys as f32;
                    mesh.uvs.extend_from_slice(&[[u0, 0.0], [u1, 0.0], [u1, top], [u0, top]]);
                    (looks.facade, [[uv, uv + 1, uv + 2], [uv, uv + 2, uv + 3]])
                } else {
                    (looks.wall, [[NONE; 3], [NONE; 3]])
                };
                for (i, tri) in [[v, v + 1, v + 2], [v, v + 2, v + 3]].into_iter().enumerate() {
                    mesh.tris.push(Tri { v: tri, n: [NONE; 3], t: t[i], material, object });
                }
                // The gable above it, plain.
                let (zp, zq) = (z_at(p), z_at(q));
                if zp > plan.eaves + 0.01 || zq > plan.eaves + 0.01 {
                    let v = mesh.positions.len() as u32;
                    mesh.positions.extend_from_slice(&[[p.x, p.y, plan.eaves], [q.x, q.y, plan.eaves], [q.x, q.y, zq], [p.x, p.y, zp]]);
                    if zq > plan.eaves + 0.01 {
                        mesh.tris.push(Tri { v: [v, v + 1, v + 2], n: [NONE; 3], t: [NONE; 3], material: looks.wall, object });
                    }
                    if zp > plan.eaves + 0.01 {
                        mesh.tris.push(Tri { v: [v, v + 2, v + 3], n: [NONE; 3], t: [NONE; 3], material: looks.wall, object });
                    }
                }
            }
        }
    }
    // The floor.
    let raw = poly.earcut_triangles_raw();
    let base = mesh.positions.len() as u32;
    mesh.positions.extend(raw.vertices.iter().map(|c| [c[0], c[1], ground]));
    for t in raw.triangle_indices.as_chunks::<3>().0.iter() {
        let v = |i: usize| (raw.vertices[i][0], raw.vertices[i][1]);
        let (pa, pb, pc) = (v(t[0]), v(t[1]), v(t[2]));
        let ccw = (pb.0 - pa.0) * (pc.1 - pa.1) - (pb.1 - pa.1) * (pc.0 - pa.0) > 0.0;
        let (b, c) = if ccw { (t[2], t[1]) } else { (t[1], t[2]) };
        mesh.tris.push(Tri { v: [base + t[0] as u32, base + b as u32, base + c as u32], n: [NONE; 3], t: [NONE; 3], material: looks.wall, object });
    }
}

fn run_buildings(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let buildings = input(inp, "Buildings");
    if ctx.geographic(buildings.crs) == Some(true) {
        return err("The buildings are in degrees. Reproject them to a system in metres first, such as SWEREF 99 TM or UTM.");
    }
    let terrain = super::solid::terrain_in(inp, buildings.crs, ctx)?;
    let storey = number_param(p, "storey", "The height of a storey")?.unwrap_or(3.0).max(1.0);
    let default_levels = number_param(p, "levels", "The storeys when not known")?.unwrap_or(2.0).max(1.0);
    let pitch = number_param(p, "pitch", "The pitch of the roofs")?.unwrap_or(35.0).clamp(5.0, 70.0).to_radians();
    let default_roof = p.get("roof").unwrap_or_else(|| "auto".into());
    let textured = p.get("windows").as_deref() != Some("false");
    let kind = Lookup::new(buildings, p.get("kind_by"), &["building", "class", "objekttyp", "andamal", "ändamål", "subtype", "type"])?;
    let height = Lookup::new(buildings, p.get("height_by"), &["height", "building:height", "hojd", "höjd"])?;
    let levels = Lookup::new(buildings, p.get("levels_by"), &["building:levels", "num_floors", "levels", "floors", "storeys", "vaningar", "våningar"])?;
    let shape = Lookup::new(buildings, p.get("roof_by"), &["roof:shape", "roof_shape", "takform"])?;
    let min_height = Lookup::new(buildings, None, &["min_height", "building:min_height"])?;
    let roof_height = Lookup::new(buildings, None, &["roof:height", "roof_height"])?;
    let roof_levels = Lookup::new(buildings, None, &["roof:levels", "roof_levels"])?;
    let orientation = Lookup::new(buildings, None, &["roof:orientation", "roof_orientation"])?;
    let wall_colour = Lookup::new(buildings, None, &["building:colour", "building:color", "facade_color", "facade_colour"])?;
    let roof_colour = Lookup::new(buildings, None, &["roof:colour", "roof:color", "roof_color", "roof_colour"])?;
    let wall_material = Lookup::new(buildings, None, &["building:material", "facade_material"])?;
    let roof_material = Lookup::new(buildings, None, &["roof:material", "roof_material"])?;

    let mut mesh = Mesh::default();
    let mut palette = Palette::default();
    let mut out = buildings.clone();
    let slots: Vec<usize> = [
        ("ground", FieldType::Real),
        ("wall_height", FieldType::Real),
        ("roof_height", FieldType::Real),
        ("height", FieldType::Real),
        ("storeys", FieldType::Int),
        ("roof_shape", FieldType::Text),
        ("look", FieldType::Text),
    ]
    .iter()
    .map(|(n, t)| column(&mut out, n, *t))
    .collect();
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let (mut made, mut guessed_height, mut approached, mut outside) = (0usize, 0usize, 0usize, 0usize);
    let mut roofs: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut styles: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (k, f) in out.features.iter_mut().enumerate() {
        mesh.objects.push(kind.text(&buildings.features[k]).map_or_else(|| format!("building {}", k + 1), |t| format!("{} {}", t, k + 1)));
        let src = &buildings.features[k];
        let Some(g) = &src.geom else { continue };
        let polys = geom::polygons_of(g);
        let Some(frame) = Frame::of(g).filter(|_| !polys.is_empty()) else { continue };
        // Where it stands: the lowest ground under its outline.
        let mut ground = 0.0;
        if let Some(t) = &terrain {
            let low = polys.iter().flat_map(|p| p.exterior().0.iter()).filter_map(|c| crate::terrain::height_at(t, c.x, c.y)).fold(f64::INFINITY, f64::min);
            if low.is_finite() {
                ground = low;
            } else {
                outside += 1;
            }
        }
        let bottom = ground + min_height.number(src).unwrap_or(0.0).max(0.0);
        let style = style_of(&kind.text(src).unwrap_or_default());
        let n_levels = levels.number(src).filter(|l| *l >= 0.5);
        let given = height.number(src).filter(|h| *h > 0.5);
        // The shape of the roof: as given, or by the default.
        let small = frame.w <= 8.0;
        let shape_text = shape.text(src).unwrap_or_else(|| match default_roof.as_str() {
            "flat" => "flat".into(),
            "gabled" => "gabled".into(),
            "hipped" => "hipped".into(),
            _ => {
                let low = n_levels.map_or_else(|| given.is_none_or(|h| h <= 12.0), |l| l <= 3.0);
                if matches!(style, Style::House | Style::Plain | Style::Civic) && low && small {
                    "gabled".into()
                } else {
                    "flat".into()
                }
            }
        });
        let flat = matches!(shape_text.trim().to_lowercase().as_str(), "flat" | "");
        let sloping_h = || {
            let half = if matches!(shape_text.trim().to_lowercase().as_str(), "skillion" | "lean_to" | "shed" | "pent") { 2.0 * frame.w } else { frame.w };
            (half * pitch.tan()).min(12.0)
        };
        let mut rh = if flat { 0.0 } else { roof_height.number(src).or_else(|| roof_levels.number(src).map(|l| l * storey)).unwrap_or_else(sloping_h) };
        // The walls: the height less the roof, or the storeys.
        let walls = match (given, n_levels) {
            (Some(h), _) => {
                let top = h - min_height.number(src).unwrap_or(0.0).max(0.0);
                if top - rh < 2.0 {
                    rh = (top * 0.4).max(0.0);
                }
                (top - rh).max(1.0)
            }
            (None, Some(l)) => l * storey,
            (None, None) => {
                guessed_height += 1;
                default_levels * storey
            }
        };
        let across = orientation.text(src).is_some_and(|o| o.trim().eq_ignore_ascii_case("across"));
        let eaves = bottom + walls;
        let (roof, approx) = roof_of(&shape_text, across, eaves, rh, &frame);
        if approx {
            approached += 1;
        }
        let storeys = n_levels.unwrap_or((walls / storey).round()).max(1.0);
        // The colours: as given, else of the material, else of the kind.
        let (wall_c, flat_c, slope_c) = style.colours();
        let glass_wall = wall_material.text(src).is_some_and(|m| m.trim().eq_ignore_ascii_case("glass"));
        let style = if glass_wall { Style::Office } else { style };
        let wc =
            wall_colour.text(src).and_then(|t| named_colour(&t)).or_else(|| wall_material.text(src).and_then(|t| material_colour(&t, false))).unwrap_or(wall_c);
        let rc = roof_colour
            .text(src)
            .and_then(|t| named_colour(&t))
            .or_else(|| roof_material.text(src).and_then(|t| material_colour(&t, true)))
            .unwrap_or(if matches!(roof, Roof::Flat(_)) { flat_c } else { slope_c });
        let windows = textured && style != Style::Plain;
        let looks = Looks {
            facade: if windows { palette.get(&mut mesh, &format!("facade {}", style.name()), wc, Some(style)) } else { 0 },
            wall: palette.get(&mut mesh, "wall", wc, None),
            roof: palette.get(&mut mesh, "roof", rc, None),
            textured: windows,
        };
        let plan = Plan { ground: bottom, eaves, roof, roof_height: rh, storeys };
        for poly in &polys {
            add_house(&mut mesh, poly, &frame, &plan, &looks, 3.0, k as u32);
        }
        *roofs.entry(roof.name()).or_default() += 1;
        *styles.entry(style.name()).or_default() += 1;
        f.attrs[slots[0]] = Value::Real(round(ground));
        f.attrs[slots[1]] = Value::Real(round(walls));
        f.attrs[slots[2]] = Value::Real(round(rh));
        f.attrs[slots[3]] = Value::Real(round(eaves + rh - ground));
        f.attrs[slots[4]] = Value::Int(storeys as i64);
        f.attrs[slots[5]] = Value::Text(roof.name().into());
        f.attrs[slots[6]] = Value::Text(style.name().into());
        made += 1;
    }
    let list = |m: &BTreeMap<&'static str, usize>| m.iter().map(|(n, c)| format!("{} {}", fmt_count(*c as u64), n)).collect::<Vec<_>>().join(", ");
    ctx.notes.info(format!(
        "{} buildings, with roofs {}; looks {}; {} triangles",
        fmt_count(made as u64),
        list(&roofs),
        list(&styles),
        fmt_count(mesh.tris.len() as u64)
    ));
    if guessed_height > 0 {
        ctx.notes.info(format!("{} buildings had neither a height nor storeys, and got {} storeys.", fmt_count(guessed_height as u64), default_levels));
    }
    if approached > 0 {
        ctx.notes.info(format!("{} roofs of other shapes, such as mansard or dome, are made as gable or hipped roofs.", fmt_count(approached as u64)));
    }
    if outside > 0 {
        ctx.notes.warn(format!("{} buildings lie outside the terrain model and stand at 0.", fmt_count(outside as u64)));
    }
    out.mesh = Some(Arc::new(mesh));
    Ok(vec![("Buildings".into(), out)])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "BuildingMaker",
    title: "Make buildings",
    hidden: false,
    group: Group::Transformer,
    category: "3D models and views",
    desc: "Makes buildings that look like buildings from their footprints on Buildings: as high as their height or storeys, with the roof their attributes give (flat, shed, gable or hipped), facades with a window in every bay of every storey, and the colours and materials of their kind: plaster for houses, glass for offices and shops, sheet metal for industry, stone for churches and plain walls for garages. The attributes of OpenStreetMap (building, building:levels, height, roof:shape, roof:height, roof:orientation, building:colour, roof:material), Overture (class, num_floors, roof_shape) and the property map (objekttyp) are recognised by themselves. They stand on the terrain on Terrain, or at 0. For the 3D view, Render 3D scene, glTF and the walk-through.",
    inputs: &["Buildings", "Terrain"],
    outputs: &["Buildings"],
    ports: None,
    params: PARAMS,
    run: run_buildings,
    summary: |p| if p.get("windows").as_deref() == Some("false") { "plain walls".into() } else { "with windows".into() },
}];

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, w: f64, l: f64) -> geo::Geometry<f64> {
        geo::Geometry::Polygon(geo::Polygon::new(geo::LineString::from(vec![(x, y), (x + l, y), (x + l, y + w), (x, y + w), (x, y)]), vec![]))
    }

    #[test]
    fn buildings_get_their_height_roof_and_look() {
        let fields = vec![
            crate::model::Field::new("building", FieldType::Text),
            crate::model::Field::new("building:levels", FieldType::Text),
            crate::model::Field::new("roof:shape", FieldType::Text),
            crate::model::Field::new("height", FieldType::Real),
        ];
        let row = |g, kind: &str, levels: &str, roof: &str, h: Option<f64>| {
            Feature::new(vec![Value::Text(kind.into()), Value::Text(levels.into()), Value::Text(roof.into()), h.map_or(Value::Null, Value::Real)], Some(g))
        };
        let table = Table {
            fields,
            features: vec![
                row(square(0.0, 0.0, 8.0, 12.0), "house", "2", "gabled", None),
                row(square(30.0, 0.0, 20.0, 30.0), "office", "", "", Some(24.0)),
                row(square(80.0, 0.0, 40.0, 60.0), "industrial", "", "", None),
                row(square(0.0, 40.0, 3.0, 6.0), "garage", "1", "", None),
            ],
            crs: Some(3006),
            ..Default::default()
        };
        let (out, _notes) = run_for_test("BuildingMaker", &[], vec![("Buildings", table)]).unwrap();
        let t = &out[0].1;
        let at = |i: usize, name: &str| t.features[i].attrs[t.field_index(name).unwrap()].clone();
        // A house of two storeys with a gable at 35°: 6 m of walls and 4 × tan 35° of roof.
        assert_eq!(at(0, "wall_height"), Value::Real(6.0));
        assert_eq!(at(0, "roof_shape"), Value::Text("gable".into()));
        assert_eq!(at(0, "roof_height"), Value::Real(2.8));
        assert_eq!(at(0, "look"), Value::Text("house".into()));
        // An office of 24 m is flat, of eight storeys, with glass.
        assert_eq!(at(1, "height"), Value::Real(24.0));
        assert_eq!(at(1, "roof_shape"), Value::Text("flat".into()));
        assert_eq!(at(1, "storeys"), Value::Int(8));
        assert_eq!(at(2, "look"), Value::Text("industry".into()));
        assert_eq!(at(3, "look"), Value::Text("plain".into()));
        let mesh = t.mesh.as_deref().unwrap();
        // The top of the house is at its ridge, and its facade has its windows.
        let top = mesh.tris.iter().filter(|tr| tr.object == 0).flat_map(|tr| tr.v).map(|v| mesh.positions[v as usize][2]).fold(0.0, f64::max);
        assert!((top - 8.8).abs() < 0.01, "{}", top);
        let facades: Vec<&Material> = mesh.materials.iter().filter(|m| m.texture.is_some()).collect();
        assert_eq!(facades.len(), 3, "{:?}", mesh.materials.iter().map(|m| &m.name).collect::<Vec<_>>());
        let textured = mesh.tris.iter().filter(|tr| tr.t[0] != NONE).count();
        assert!(textured > 0 && mesh.uvs.iter().all(|uv| uv[0] >= 0.0 && uv[1] >= 0.0));
        // The long wall of the house, 12 m, has four bays, and its two storeys.
        assert!(mesh.uvs.iter().any(|uv| (uv[0] - 4.0).abs() < 1e-6 && (uv[1] - 2.0).abs() < 1e-6));
        // The garage is plain, and every material is used.
        assert!(mesh.tris.iter().filter(|tr| tr.object == 3).all(|tr| mesh.materials[tr.material as usize].texture.is_none()));
    }

    #[test]
    fn kinds_have_their_styles_and_colours_their_names() {
        assert_eq!(style_of("Bostad"), Style::House);
        assert_eq!(style_of("Industri"), Style::Industry);
        assert_eq!(style_of("Samhällsfunktion"), Style::Office);
        assert_eq!(style_of("Komplementbyggnad"), Style::Plain);
        assert_eq!(style_of("church"), Style::Civic);
        assert_eq!(style_of("yes"), Style::House);
        assert_eq!(named_colour("#ff0000"), Some([255, 0, 0]));
        assert_eq!(named_colour("white"), Some([0xf2, 0xf0, 0xea]));
        assert_eq!(material_colour("roof_tiles", true), Some([0x9a, 0x45, 0x32]));
        let t = facade_texture(Style::House, [200, 190, 170]);
        assert_eq!(t.rgba.len(), 64 * 64 * 4);
        // The middle of the window is glass, darker than the wall.
        let px = |x: usize, y: usize| t.rgba[(y * 64 + x) * 4] as i32;
        assert!(px(24, 20) < 120 && px(4, 4) > 180);
    }
}
