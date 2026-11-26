//! Shade terrain, Quick terrain view and Tanaka contours: the heights come in on a stream, read
//! from a raster as a grid, and a map from Draw map can be laid over them.

use super::*;
use crate::model::{crs_label, err, Grid, RasterImage, Result, Table};
use crate::terrain::{self, Colours, Look, Shadows, View};
use std::borrow::Cow;

pub(super) const SHADOWS: &[(&str, &str)] = &[("soft", "Soft"), ("sharp", "Sharp"), ("none", "None")];
pub(super) const COLOURS: &[(&str, &str)] = &[
    ("land", "Land: green to light brown"),
    ("mountains", "Mountains: green to brown to snow"),
    ("greys", "Grey to white"),
    ("white", "Light all over"),
    ("rainbow", "Rainbow: violet lowest, through blue, green and yellow, to red"),
    ("sea", "Sea: navy lowest, through turquoise and green, to pale yellow"),
];
pub(super) const QUALITY: &[(&str, &str)] = &[("2", "Smooth"), ("3", "Smoothest, slower"), ("1", "Rough, fastest")];
pub(super) const BACKGROUND: &[(&str, &str)] = &[("sky", "A sky"), ("colour", "A colour, with the shadow of the block on it"), ("none", "See-through")];

macro_rules! look_params {
    () => {
        [
            ParamSpec {
                key: "sun_azimuth",
                label: "Sun from",
                kind: ParamKind::Text,
                default: "315",
                hint: "Degrees clockwise from north: 315 is from the north-west, as on most maps",
            },
            ParamSpec {
                key: "light",
                label: "Light",
                kind: ParamKind::Choice(&[("one", "From the sun alone"), ("several", "From several directions, softer and clearer")]),
                default: "one",
                hint:
                    "From several directions, slopes that face away from the sun are seen too, as on a good relief map. The shadows are still cast by the sun",
            },
            ParamSpec {
                key: "sun_altitude",
                label: "Sun height",
                kind: ParamKind::Text,
                default: "35",
                hint: "Degrees above the horizon. A low sun gives long shadows",
            },
            ParamSpec {
                key: "exaggeration",
                label: "Height exaggeration",
                kind: ParamKind::Text,
                default: "1.5",
                hint: "The heights are multiplied by this. Flat land needs more, such as 3",
            },
            ParamSpec { key: "shadows", label: "Shadows", kind: ParamKind::Choice(SHADOWS), default: "soft", hint: "Cast by the terrain, away from the sun" },
            ParamSpec { key: "shadow_darkness", label: "Shadow darkness in %", kind: ParamKind::Text, default: "60", hint: "" },
            ParamSpec {
                key: "sky_light",
                label: "Darker valleys",
                kind: ParamKind::Choice(YES_NO),
                default: "true",
                hint: "Valleys and hollows see less of the sky and get less light from it",
            },
            ParamSpec {
                key: "colours",
                label: "Colours",
                kind: ParamKind::Choice(COLOURS),
                default: "land",
                hint: "By the height, under the map if one is connected",
            },
            ParamSpec {
                key: "haze",
                label: "Haze in the valleys in %",
                kind: ParamKind::Text,
                default: "0",
                hint: "A light haze that lies in the low parts, as morning mist",
            },
            ParamSpec {
                key: "map_opacity",
                label: "Map opacity in %",
                kind: ParamKind::Text,
                default: "100",
                hint: "How much the map connected to Map covers the colours of the terrain",
            },
            ParamSpec {
                key: "steps",
                label: "Steps every",
                kind: ParamKind::Text,
                default: "",
                hint: "In metres: the heights in flat steps, as layers cut from card. Empty: smooth",
            },
        ]
    };
}

pub(super) const LOOK: [ParamSpec; 11] = look_params!();

pub(super) const SHADE_PARAMS: &[ParamSpec] = &{
    let mut all = [LOOK[0]; 12];
    let mut i = 0;
    while i < 11 {
        all[i] = LOOK[i];
        i += 1;
    }
    all[11] = ParamSpec {
        key: "width",
        label: "Width in pixels",
        kind: ParamKind::Text,
        default: "",
        hint: "Empty gives one pixel for each cell of the grid, at most 4000 across",
    };
    all
};

pub(super) const VIEW_PARAMS: &[ParamSpec] = &{
    const VIEW: [ParamSpec; 11] = [
        ParamSpec {
            key: "view_azimuth",
            label: "Looking from",
            kind: ParamKind::Text,
            default: "200",
            hint: "Degrees clockwise from north: 180 looks north from the south",
        },
        ParamSpec {
            key: "tilt",
            label: "Camera height in degrees",
            kind: ParamKind::Text,
            default: "30",
            hint: "How far above the horizon the camera looks down: 90 is straight down",
        },
        ParamSpec {
            key: "fov",
            label: "Lens angle",
            kind: ParamKind::Text,
            default: "30",
            hint: "Degrees from top to bottom. Less is like a telephoto lens, more like a wide angle",
        },
        ParamSpec { key: "zoom", label: "Zoom in %", kind: ParamKind::Text, default: "100", hint: "100 shows all of the terrain. More comes closer" },
        ParamSpec { key: "width", label: "Width in pixels", kind: ParamKind::Text, default: "1600", hint: "" },
        ParamSpec { key: "height", label: "Height in pixels", kind: ParamKind::Text, default: "1000", hint: "" },
        ParamSpec { key: "fog", label: "Fog in %", kind: ParamKind::Text, default: "30", hint: "Grows with the distance, as air does far away" },
        ParamSpec { key: "background", label: "Background", kind: ParamKind::Choice(BACKGROUND), default: "sky", hint: "" },
        ParamSpec { key: "background_colour", label: "Colour", kind: ParamKind::Text, default: "#0d1b3d", hint: "Such as #0d1b3d" },
        ParamSpec {
            key: "block",
            label: "As a block",
            kind: ParamKind::Choice(YES_NO),
            default: "true",
            hint: "The terrain stands on a block with sides, as in a block diagram",
        },
        ParamSpec { key: "quality", label: "Edges", kind: ParamKind::Choice(QUALITY), default: "2", hint: "" },
    ];
    let mut all = [LOOK[0]; 22];
    let mut i = 0;
    while i < 11 {
        all[i] = LOOK[i];
        i += 1;
    }
    let mut k = 0;
    while k < 11 {
        all[11 + k] = VIEW[k];
        k += 1;
    }
    all
};

pub(super) fn percent(p: &P, key: &str, what: &str, default: f64) -> Result<f64> {
    Ok((number_param(p, key, what)?.unwrap_or(default) / 100.0).clamp(0.0, 1.0))
}

pub(super) fn look(p: &P) -> Result<Look> {
    Ok(Look {
        sun_azimuth: number_param(p, "sun_azimuth", "Sun from")?.unwrap_or(315.0),
        sun_altitude: number_param(p, "sun_altitude", "Sun height")?.unwrap_or(35.0).clamp(1.0, 90.0),
        several: p.get("light").as_deref() == Some("several"),
        exaggeration: number_param(p, "exaggeration", "The height exaggeration")?.unwrap_or(1.5).max(0.01),
        shadows: match p.get("shadows").as_deref() {
            Some("none") => Shadows::None,
            Some("sharp") => Shadows::Sharp,
            _ => Shadows::Soft,
        },
        shadow_darkness: percent(p, "shadow_darkness", "The shadow darkness", 60.0)?,
        sky_light: p.get("sky_light").as_deref() != Some("false"),
        colours: match p.get("colours").as_deref() {
            Some("greys") => Colours::Greys,
            Some("white") => Colours::White,
            Some("mountains") => Colours::Mountains,
            Some("rainbow") => Colours::Rainbow,
            Some("sea") => Colours::Sea,
            _ => Colours::Land,
        },
        haze: percent(p, "haze", "The haze", 0.0)?,
        map_opacity: percent(p, "map_opacity", "The map opacity", 100.0)?,
        steps: match number_param(p, "steps", "Steps every")? {
            Some(s) if s < 0.0 => return err("Steps every must be more than 0 metres."),
            Some(s) => s,
            None => 0.0,
        },
    })
}

/// The heights, the map, and what degrees were multiplied by to give metres, across and down.
type Terrain<'a> = (Cow<'a, Grid>, Option<Cow<'a, RasterImage>>, Option<(f64, f64)>);

/// The grid of heights on Heights, and the map on Map when there is one that fits. Heights
/// in degrees of latitude and longitude come back with their cells in metres, and the map
/// with them, together with what the coordinates were multiplied by across and down.
pub(super) fn inputs<'a>(inp: &'a Inputs, ctx: &mut NodeCtx) -> Result<Terrain<'a>> {
    let heights: &Table = input(inp, "Heights");
    let Some(grid) = heights.grid.as_deref() else {
        if heights.features.is_empty() {
            return err("Nothing came in on Heights. Connect an input that reads a terrain model, with Cells as set to a grid.");
        }
        return err("The stream on Heights has no grid. Read the terrain model with Cells as set to Heights or other values on a grid.");
    };
    let maps: &Table = input(inp, "Map");
    let map = match maps.image.as_deref() {
        Some(m) if maps.crs != grid.crs && maps.crs.is_some() && grid.crs.is_some() => {
            ctx.notes.warn(format!(
                "The map is in {} and the heights in {}, so the map was left out. Draw the map in the coordinate system of the heights.",
                crs_label(maps.crs),
                crs_label(grid.crs)
            ));
            let _ = m;
            None
        }
        Some(m) => Some(m),
        None if !maps.features.is_empty() => {
            ctx.notes.warn("The stream on Map is not a picture. Connect Draw map to it.".to_string());
            None
        }
        None => None,
    };
    if ctx.geographic(grid.crs) == Some(true) {
        // Degrees as metres where the terrain lies: a degree of latitude is about 110.6 km,
        // one of longitude that times the cosine of the latitude.
        let gt = grid.geotransform;
        let mid = gt[3] + gt[5] * grid.height as f64 / 2.0;
        let k = (111_320.0 * mid.to_radians().cos().max(0.01), 110_574.0);
        let scale = |gt: [f64; 6]| [gt[0] * k.0, gt[1] * k.0, gt[2] * k.0, gt[3] * k.1, gt[4] * k.1, gt[5] * k.1];
        let mut g = grid.clone();
        g.geotransform = scale(gt);
        let map = map.map(|m| {
            let mut m = m.clone();
            m.geotransform = scale(m.geotransform);
            Cow::Owned(m)
        });
        ctx.notes.info(format!(
            "The heights are in {}, in degrees: their cells were taken as {:.0} × {:.0} m, as they are around latitude {:.1}°",
            crs_label(grid.crs),
            gt[1].abs() * k.0,
            gt[5].abs() * k.1,
            mid
        ));
        return Ok((Cow::Owned(g), map, Some(k)));
    }
    Ok((Cow::Borrowed(grid), map.map(Cow::Borrowed), None))
}

pub(super) fn run_shade(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (grid, map, scaled) = inputs(inp, ctx)?;
    let (grid, map) = (grid.as_ref(), map.as_deref());
    let look = look(p)?;
    let width = number_param(p, "width", "The width")?.map(|w| w.round() as usize);
    let image = match width {
        Some(w) if w > 8000 => return err("The width can be at most 8000 pixels."),
        Some(w) if w >= 2 => terrain::shade_width(grid, &look, map, w),
        _ => {
            if grid.width > 4000 {
                ctx.notes.info("The grid is wider than 4000 cells, so the picture was made 4000 pixels wide. Set the width to choose.".to_string());
            }
            terrain::shade(&terrain::at_most(grid, 4000), &look, map)
        }
    };
    ctx.notes.info(format!("{} × {} pixels", image.width, image.height));
    let mut image = image;
    if let Some((kx, ky)) = scaled {
        // Back to degrees, so that the picture lies where the heights do.
        let g = &mut image.geotransform;
        *g = [g[0] / kx, g[1] / kx, g[2] / kx, g[3] / ky, g[4] / ky, g[5] / ky];
    }
    Ok(vec![("Image".into(), image.into_table(grid.crs))])
}

pub(super) fn run_view(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let (grid, map, _) = inputs(inp, ctx)?;
    let (grid, map) = (grid.as_ref(), map.as_deref());
    let look = look(p)?;
    let size = |key: &str, what: &str, default: f64| -> Result<usize> {
        let v = number_param(p, key, what)?.unwrap_or(default).round();
        if !(16.0..=10000.0).contains(&v) {
            return err(format!("{} must be from 16 to 10000 pixels.", what));
        }
        Ok(v as usize)
    };
    let view = View {
        azimuth: number_param(p, "view_azimuth", "Looking from")?.unwrap_or(200.0),
        tilt: number_param(p, "tilt", "The camera height")?.unwrap_or(30.0),
        fov: number_param(p, "fov", "The lens angle")?.unwrap_or(30.0),
        zoom: number_param(p, "zoom", "The zoom")?.unwrap_or(100.0) / 100.0,
        width: size("width", "The width", 1600.0)?,
        height: size("height", "The height", 1000.0)?,
        fog: percent(p, "fog", "The fog", 30.0)?,
        sky: p.get("background").as_deref() != Some("none"),
        background: match p.get("background").as_deref() {
            Some("colour") => {
                let c = super::solid::colour_param(p, "background_colour", [13.0 / 255.0, 27.0 / 255.0, 61.0 / 255.0])?;
                Some([(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8])
            }
            _ => None,
        },
        block: p.get("block").as_deref() != Some("false"),
        quality: p.get("quality").and_then(|q| q.parse().ok()).unwrap_or(2),
    };
    if view.width * view.height > 40_000_000 {
        return err("The picture can have at most 40 million pixels.");
    }
    let image = terrain::render(grid, &look, map, &view);
    // A view from the side has no coordinates on the map.
    let mut table = image.into_table(None);
    table.crs = None;
    Ok(vec![("Image".into(), table)])
}

// ---------- Tanaka contours ----------

pub(super) const TINTS: &[(&str, &str)] =
    &[("grey", "Grey bands"), ("terrain", "Land: green to brown to light"), ("blue", "Blue bands"), ("none", "None, see-through")];

pub(super) const TANAKA_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "interval", label: "Height between lines", kind: ParamKind::Text, default: "10", hint: "In the units of the heights, such as metres" },
    ParamSpec {
        key: "base",
        label: "Lines through the height",
        kind: ParamKind::Text,
        default: "0",
        hint: "One line goes through this height, the others every interval from it",
    },
    ParamSpec {
        key: "sun_azimuth",
        label: "Light from",
        kind: ParamKind::Text,
        default: "315",
        hint: "Degrees clockwise from north. Lines on slopes that face it are light, the others dark",
    },
    ParamSpec {
        key: "line_width",
        label: "Line width",
        kind: ParamKind::Text,
        default: "2",
        hint: "The widest line, in pixels, where a slope faces the light or turns from it squarely",
    },
    ParamSpec {
        key: "smooth",
        label: "Smoothing",
        kind: ParamKind::Text,
        default: "1",
        hint: "How many times the heights are evened out over their neighbours first, for calm lines over flat land. 0 keeps every bump",
    },
    ParamSpec {
        key: "tint",
        label: "Bands",
        kind: ParamKind::Choice(TINTS),
        default: "grey",
        hint: "The colour between the lines. Grey lets both the light and the dark lines show",
    },
    ParamSpec { key: "width", label: "Width in pixels", kind: ParamKind::Text, default: "", hint: "Empty makes it as wide as the grid, at most 4000" },
];

pub(super) fn run_tanaka(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let heights: &Table = input(inp, "Heights");
    let Some(grid) = heights.grid.as_deref() else {
        return err("The stream on Heights has no grid. Read the terrain model with Cells as set to Heights or other values on a grid.");
    };
    let interval = number_param(p, "interval", "The height between lines")?.unwrap_or(10.0);
    if interval <= 0.0 {
        return err("The height between lines must be more than 0.");
    }
    let width = match number_param(p, "width", "The width")? {
        Some(w) if !(16.0..=12000.0).contains(&w) => return err("The width must be from 16 to 12000 pixels."),
        Some(w) => w as usize,
        None => grid.width.min(4000),
    };
    let t = crate::tanaka::Tanaka {
        interval,
        base: number_param(p, "base", "The height the lines go through")?.unwrap_or(0.0),
        sun: number_param(p, "sun_azimuth", "Light from")?.unwrap_or(315.0),
        width: number_param(p, "line_width", "The line width")?.unwrap_or(2.0).clamp(0.1, 50.0) as f32,
        tint: p.get("tint").unwrap_or_else(|| "grey".into()),
        pixels: width,
    };
    let times = number_param(p, "smooth", "The smoothing")?.unwrap_or(1.0).clamp(0.0, 20.0) as usize;
    let smoothed;
    let grid = if times > 0 {
        smoothed = crate::tanaka::smooth(grid, times);
        &smoothed
    } else {
        grid
    };
    let pieces = crate::tanaka::pieces(grid, &t);
    if pieces.len() > 5_000_000 {
        return err(format!("That gives {} pieces of line. Make the height between lines larger.", crate::model::fmt_count(pieces.len() as u64)));
    }
    let image = crate::tanaka::draw(grid, &t, &pieces);
    // The lines too, in the coordinates of the grid, with their height and how they face
    // the light, for other maps.
    let gt = grid.geotransform;
    let world = |q: (f64, f64)| geo::Coord { x: gt[0] + (q.0 + 0.5) * gt[1], y: gt[3] + (q.1 + 0.5) * gt[5] };
    let lines = Table {
        fields: vec![crate::model::Field::new("height", crate::model::FieldType::Real), crate::model::Field::new("light", crate::model::FieldType::Real)],
        crs: grid.crs,
        features: pieces
            .iter()
            .map(|pc| crate::model::Feature {
                z: None,
                attrs: vec![crate::model::Value::Real(pc.height), crate::model::Value::Real((pc.light * 1000.0).round() / 1000.0)],
                geom: Some(geo::Geometry::Line(geo::Line::new(world(pc.a), world(pc.b)))),
            })
            .collect(),
        ..Default::default()
    };
    ctx.notes.info(format!("{} pieces of line · {} × {} pixels", crate::model::fmt_count(pieces.len() as u64), image.width, image.height));
    Ok(vec![("Image".into(), image.into_table(grid.crs)), ("Lines".into(), lines)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "TanakaContours",
        title: "Tanaka contours",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "For maps: illuminated contours, after Kitiro Tanaka: the contour lines of the heights on Heights, light where the slope faces the light and dark where it faces away, widest where they face it squarely, over bands of colour between the heights. The terrain stands out as in relief. The picture has coordinates; the pieces of line come out on Lines with their height and light.",
        inputs: &["Heights"],
        outputs: &["Image", "Lines"],
        ports: None,
        params: TANAKA_PARAMS,
        run: run_tanaka,
        summary: |p| format!("every {} · light from {}°", p.get("interval").unwrap_or_else(|| "10".into()), p.get("sun_azimuth").unwrap_or_else(|| "315".into())),
    },
    NodeType {
        name: "TerrainShader",
        title: "Shade terrain",
        hidden: false,
        group: Group::Transformer,
        category: "Terrain",
        desc: "For maps: shaded relief from the heights on Heights, seen from above: lit by a sun, with shadows the terrain casts, darker valleys, colours by the height and haze in the low parts. A map from Draw map connected to Map is laid on the terrain and shaded with it. The picture has coordinates, for PNG, GeoTIFF or PDF.",
        inputs: &["Heights", "Map"],
        outputs: &["Image"],
        ports: None,
        params: SHADE_PARAMS,
        run: run_shade,
        summary: |p| format!("sun from {}°, {}° high", p.get("sun_azimuth").unwrap_or_else(|| "315".into()), p.get("sun_altitude").unwrap_or_else(|| "35".into())),
    },
    NodeType {
        name: "TerrainRenderer",
        title: "Quick terrain view",
        hidden: false,
        group: Group::Transformer,
        category: "3D models and views",
        desc: "A quick view of terrain alone from the side, in seconds, as a block diagram or under a sky: the heights on Heights with the same light, shadows and colours as Shade terrain, a map from Draw map laid on it, fog with the distance and a sky behind. The terrain can stand on a block, as in a block diagram. The picture has no coordinates. For buildings and other models, glass, water and the sun of a date, use Render 3D scene.",
        inputs: &["Heights", "Map"],
        outputs: &["Image"],
        ports: None,
        params: VIEW_PARAMS,
        run: run_view,
        summary: |p| format!("from {}°, {}° down", p.get("view_azimuth").unwrap_or_else(|| "200".into()), p.get("tilt").unwrap_or_else(|| "30".into())),
    },
];

#[cfg(test)]
mod tests {
    use super::super::run_for_test;
    use crate::model::Grid;

    #[test]
    fn heights_in_degrees_are_seen_as_metres_where_they_lie() {
        // A hill 2000 m high, 2° across, in degrees around latitude 35°.
        let n = 80;
        let values: Vec<f32> = (0..n * n).map(|i| (2000.0 - (((i % n) as f32 - 40.0).hypot((i / n) as f32 - 40.0)) * 60.0).max(0.0)).collect();
        let g = Grid {
            width: n,
            height: n,
            values,
            geotransform: [139.0, 0.025, 0.0, 36.0, 0.0, -0.025],
            crs: Some(4326),
            bands: Vec::new(),
            meta: Default::default(),
        };
        let heights = crate::model::Table { grid: Some(std::sync::Arc::new(g)), crs: Some(4326), ..Default::default() };
        let (out, notes) =
            run_for_test("TerrainRenderer", &[("background", "none"), ("width", "200"), ("height", "120")], vec![("Heights", heights.clone())]).unwrap();
        assert!(notes.warnings.is_empty(), "{:?}", notes.warnings);
        let img = out[0].1.image.as_deref().unwrap();
        // The terrain fills much of the picture, rather than standing as a thin needle.
        let seen = img.rgba.chunks(4).filter(|p| p[3] > 0).count();
        assert!(seen > img.width * img.height / 5, "{} of {}", seen, img.width * img.height);
        // The relief seen from above keeps the coordinates of the heights, in degrees.
        let (out, _) = run_for_test("TerrainShader", &[], vec![("Heights", heights)]).unwrap();
        let img = out[0].1.image.as_deref().unwrap();
        assert!((img.geotransform[0] - 139.0).abs() < 1e-9 && (img.geotransform[3] - 36.0).abs() < 1e-9);
        assert!((img.geotransform[1] * img.width as f64 - 2.0).abs() < 1e-6);
    }
}
