//! Render 3D scene: pictures of 3D models, such as buildings from OBJ files, lit by the
//! sun and the sky, on a floor or on the terrain with a map laid over it.

use super::*;
use crate::model::{err, GbError, Result};
use crate::render3d::{Background, Look, Settings, Water};

pub(super) const SUN_MODES: &[(&str, &str)] = &[
    ("angles", "From where you say"),
    ("date", "From the date and time, at the place of the model"),
    ("study", "A sun study: a picture for each time of each date"),
    ("day", "A whole day: a picture every so many hours from sunrise to sunset of the date"),
];
pub(super) const CLOCKS: &[(&str, &str)] = &[("swedish", "Swedish time"), ("utc", "UTC"), ("computer", "As this computer's clock")];
pub(super) const SOFTNESS: &[(&str, &str)] = &[("0.5", "Sharp, as on a clear day"), ("3", "Soft"), ("8", "Very soft, as through thin cloud")];
pub(super) const QUALITY: &[(&str, &str)] = &[("16", "Good"), ("4", "Draft, quick"), ("64", "Best, slow")];
pub(super) const TERRAIN_COLOURS: &[(&str, &str)] = &[
    ("grass", "Grass green"),
    ("land", "Land: green to light brown"),
    ("mountains", "Mountains: green to brown to snow"),
    ("greys", "Grey to white"),
    ("white", "Light all over"),
];
pub(super) const BACKGROUNDS: &[(&str, &str)] = &[
    ("studio", "A light studio, with a floor"),
    ("sky", "The sky, as it is with the sun where it stands"),
    ("clear", "See-through"),
    ("colour", "A colour of your own"),
];
pub(super) const MOODS: &[(&str, &str)] = &[
    ("neutral", "Neutral daylight"),
    ("warm", "Warm sun and cool shade, livelier"),
    ("golden", "Golden: warm, low light, strong colours"),
    ("soft", "Soft and dreamy: gentle shadows, pastel light"),
];

/// The tints of the sun and the sky, how much livelier the colours are made and how much
/// wider the sun is, for a mood.
fn mood_of(m: &str) -> ([f32; 3], [f32; 3], f32, f64) {
    match m {
        "warm" => ([1.08, 0.95, 0.80], [0.82, 0.92, 1.18], 0.12, 0.0),
        "golden" => ([1.22, 0.88, 0.58], [0.72, 0.82, 1.25], 0.2, 0.0),
        "soft" => ([0.85, 0.82, 0.80], [1.2, 1.18, 1.3], 0.05, 6.0),
        _ => ([1.0; 3], [1.0; 3], 0.0, 0.0),
    }
}
pub(super) const LOOKS: &[(&str, &str)] = &[
    ("materials", "Their own materials and textures"),
    ("white", "White, as an architect's model"),
    ("wood", "Light wood, as a model of a city"),
    ("sketch", "A drawing: paper white, with lines along the edges"),
];
pub(super) const NIGHTS: &[(&str, &str)] =
    &[("day", "Day, as the sun gives"), ("moon", "Night, under the moon and the stars"), ("aurora", "Night with northern lights")];
pub(super) const LIGHTNINGS: &[(&str, &str)] = &[("none", "None"), ("bolt", "A bolt of lightning"), ("storm", "A thunderstorm: a dark sky and three bolts")];
pub(super) const SEASONS: &[(&str, &str)] = &[
    ("summer", "Summer: green"),
    ("spring", "Spring: light green, some in blossom"),
    ("autumn", "Autumn: yellow, orange and red"),
    ("winter", "Winter: bare branches, the conifers green"),
];
pub(super) const BOUNCES: &[(&str, &str)] =
    &[("1", "Once: quick"), ("2", "Twice: lighter in courtyards and arcades, about 1.7 times the time"), ("3", "Three times: about 2.5 times the time")];
pub(super) const WATERS: &[(&str, &str)] =
    &[("none", "None"), ("level", "A surface at a height: a sea, or the lakes of the terrain"), ("areas", "In the areas connected to Water")];
pub(super) const PROJECTIONS: &[(&str, &str)] =
    &[("perspective", "Perspective, as a camera sees"), ("parallel", "Parallel, as an isometric drawing: far is as large as near")];
pub(super) const WAVES: &[(&str, &str)] = &[("0", "Still, as a mirror"), ("0.35", "Ripples"), ("1", "A fresh breeze")];

pub(super) const SCENE_PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "width", label: "Width in pixels", kind: ParamKind::Text, default: "1600", hint: "" },
    ParamSpec { key: "height", label: "Height in pixels", kind: ParamKind::Text, default: "1000", hint: "" },
    ParamSpec {
        key: "view_azimuth",
        label: "Looking from",
        kind: ParamKind::Text,
        default: "215",
        hint: "Degrees clockwise from north: 180 looks north from the south",
    },
    ParamSpec {
        key: "camera",
        label: "Camera",
        kind: ParamKind::Choice(super::scene096::CAMERAS),
        default: "",
        hint: "A ready place for the camera, which sets its height, projection and lens. Looking from and the zoom still count",
    },
    ParamSpec { key: "elevation", label: "Camera height in degrees", kind: ParamKind::Text, default: "25", hint: "Above the horizon. 90 looks straight down" },
    ParamSpec {
        key: "projection",
        label: "Projection",
        kind: ParamKind::Choice(PROJECTIONS),
        default: "perspective",
        hint: "Parallel suits a diorama or a game map. With the camera height at 35 it is isometric, at 30 as in many games",
    },
    ParamSpec {
        key: "fov",
        label: "Lens angle",
        kind: ParamKind::Text,
        default: "35",
        hint: "In degrees, from top to bottom. Small is a telephoto, large a wide angle",
    },
    ParamSpec { key: "zoom", label: "Zoom in %", kind: ParamKind::Text, default: "100", hint: "100 shows the models whole" },
    ParamSpec {
        key: "distance",
        label: "Camera distance",
        kind: ParamKind::Text,
        default: "",
        hint: "From the middle of the models, in metres. Empty stands the camera where the models are seen whole, by the zoom",
    },
    ParamSpec { key: "sun", label: "Sun", kind: ParamKind::Choice(SUN_MODES), default: "angles", hint: "From the date and time, for shadow studies" },
    ParamSpec { key: "sun_azimuth", label: "Sun from", kind: ParamKind::Text, default: "160", hint: "Degrees clockwise from north" },
    ParamSpec { key: "sun_altitude", label: "Sun height", kind: ParamKind::Text, default: "45", hint: "Degrees above the horizon" },
    ParamSpec {
        key: "date",
        label: "Date",
        kind: ParamKind::Text,
        default: "2026-03-20",
        hint: "Such as 2026-03-20 for the spring equinox, or 2026-06-21 for midsummer",
    },
    ParamSpec { key: "time", label: "Time", kind: ParamKind::Text, default: "12:00", hint: "Such as 15:00" },
    ParamSpec {
        key: "study_dates",
        label: "Dates",
        kind: ParamKind::Text,
        default: "2026-03-20, 2026-06-21",
        hint: "A row of pictures for each, such as the spring equinox and midsummer. Add 2026-12-21 for midwinter",
    },
    ParamSpec { key: "study_from", label: "From", kind: ParamKind::Text, default: "09:00", hint: "The first time of each date" },
    ParamSpec { key: "study_to", label: "To", kind: ParamKind::Text, default: "17:00", hint: "The last time" },
    ParamSpec {
        key: "study_every",
        label: "Every so many hours",
        kind: ParamKind::Text,
        default: "2",
        hint: "Such as 1 or 2. Each picture takes as long as one, so draft quality and a smaller size are good while trying",
    },
    ParamSpec {
        key: "study_folder",
        label: "Each picture also in",
        kind: ParamKind::Text,
        default: "",
        hint: "A folder beside the model where each picture is written as PNG, named by date and time. Empty: only the sheet of them all",
    },
    ParamSpec {
        key: "gif",
        label: "Also as a moving picture",
        kind: ParamKind::Text,
        default: "",
        hint: "A file beside the model, such as out/the_day.gif, where the pictures of the sun study or the day are shown one after another. Empty: none",
    },
    ParamSpec { key: "clock", label: "Time zone", kind: ParamKind::Choice(CLOCKS), default: "swedish", hint: "Swedish time includes summer time" },
    ParamSpec { key: "softness", label: "Shadows", kind: ParamKind::Choice(SOFTNESS), default: "3", hint: "" },
    ParamSpec { key: "sun_strength", label: "Sunlight in %", kind: ParamKind::Text, default: "100", hint: "" },
    ParamSpec {
        key: "sky_strength",
        label: "Skylight in %",
        kind: ParamKind::Text,
        default: "100",
        hint: "The light from the sky, that reaches into the shade",
    },
    ParamSpec {
        key: "clouds",
        label: "Cloud cover in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "0 is clear. More dims the sun and softens the shadows; 100 is overcast, with an even light and hardly any shadows. Seen in the sky when the background is the sky",
    },
    ParamSpec {
        key: "haze",
        label: "Haze in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Far things fade into the colour of the horizon, as over a landscape: 20 is a summer day, 60 a hazy one. The sun gets warmer too",
    },
    ParamSpec { key: "background", label: "Background", kind: ParamKind::Choice(BACKGROUNDS), default: "studio", hint: "" },
    ParamSpec {
        key: "background_colour",
        label: "Background colour",
        kind: ParamKind::Text,
        default: "#ffd6e0",
        hint: "As #rrggbb, such as #57cfda. Given while the background is the studio, it is used instead",
    },
    ParamSpec {
        key: "mood",
        label: "Light",
        kind: ParamKind::Choice(MOODS),
        default: "neutral",
        hint: "The colour of the sunlight and of the shade. Warm suits a diorama or a picture for a presentation",
    },
    ParamSpec {
        key: "title",
        label: "Title",
        kind: ParamKind::Text,
        default: "",
        hint: "Written at the top of the picture, as on a finished map",
    },
    ParamSpec {
        key: "floor",
        label: "A floor under the models",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "That takes their shadows. Without it they float, as a diorama on a see-through background",
    },
    ParamSpec { key: "look", label: "The models as", kind: ParamKind::Choice(LOOKS), default: "materials", hint: "Glass stays glass" },
    ParamSpec {
        key: "glass_names",
        label: "Glass in materials named",
        kind: ParamKind::Text,
        default: "",
        hint: "Words in the names of materials that are to be glass, such as glass, window, glas, fönster: models from CAD and IFC often have windows that cannot be seen through",
    },
    ParamSpec {
        key: "glow_names",
        label: "Light in materials named",
        kind: ParamKind::Text,
        default: "",
        hint: "Words in the names of materials that give off light in their own colour, such as lamp, neon, sign. Materials with emission in their file glow too",
    },
    ParamSpec {
        key: "roof_by",
        label: "Roofs coloured by",
        kind: ParamKind::Attribute("Models"),
        default: "",
        hint: "An attribute of the models with the kind of roof, such as roof_type from LoD2 buildings: flat, gable, hipped, shed or mansard, in English or Swedish (platt, sadel, valm, pulpet), or a colour as #rrggbb. Empty keeps their materials",
    },
    ParamSpec { key: "tree_height", label: "Trees, height", kind: ParamKind::Text, default: "10", hint: "In metres, for the points on Trees" },
    ParamSpec {
        key: "tree_height_by",
        label: "Trees, height from",
        kind: ParamKind::Attribute("Trees"),
        default: "",
        hint: "An attribute with the height of each tree. Empty gives all the height above",
    },
    ParamSpec {
        key: "tree_kind",
        label: "Trees, kind",
        kind: ParamKind::Choice(&[("leafy", "Leafy, with a round crown"), ("conifer", "Conifers, as cones")]),
        default: "leafy",
        hint: "",
    },
    ParamSpec {
        key: "tree_kind_by",
        label: "Trees, kind from",
        kind: ParamKind::Attribute("Trees"),
        default: "",
        hint: "An attribute where words such as conifer, pine, spruce, barr, gran or tall make a conifer, and other values a leafy tree",
    },
    ParamSpec {
        key: "lit_windows",
        label: "Lit windows in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "The share of the windows lit from inside, warm as at dusk: panes of glass, and the windows on plain walls. Best with the sun low or below the horizon",
    },
    ParamSpec {
        key: "windows",
        label: "Windows on plain walls",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "Rows of windows on walls without a texture, a floor every 3 m, as on blocks from Extrude",
    },
    ParamSpec {
        key: "light_strength",
        label: "Lamps, strength in %",
        kind: ParamKind::Text,
        default: "100",
        hint: "For the points on Lights: 100 is a street lamp, that lights the ground some 10 m around it",
    },
    ParamSpec { key: "light_colour", label: "Lamps, colour", kind: ParamKind::Text, default: "#ffc78a", hint: "Warm white is #ffc78a, cold white #e8f0ff" },
    ParamSpec {
        key: "light_height",
        label: "Lamps, height",
        kind: ParamKind::Text,
        default: "5",
        hint: "In metres above the terrain or the foot of the models, for points without a height of their own",
    },
    ParamSpec {
        key: "blur",
        label: "Depth of field, blur in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "0 is sharp all through. More blurs what is nearer and further than the focus: 30 looks like a model photographed close up. Takes more rays to be smooth",
    },
    ParamSpec {
        key: "focus",
        label: "Focus distance",
        kind: ParamKind::Text,
        default: "",
        hint: "In metres from the camera. Empty: the middle of what is seen",
    },
    ParamSpec {
        key: "reflections",
        label: "Reflections",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "Smooth and metallic surfaces mirror what is around them, not only the sky. About 1.3 times the time",
    },
    ParamSpec { key: "bounces", label: "Light bounces", kind: ParamKind::Choice(BOUNCES), default: "1", hint: "Between surfaces before it reaches the eye" },
    ParamSpec { key: "water", label: "Water", kind: ParamKind::Choice(WATERS), default: "none", hint: "Mirrors the sky and what stands by it, with the sun glittering on it" },
    ParamSpec {
        key: "water_level",
        label: "Water height",
        kind: ParamKind::Text,
        default: "",
        hint: "In metres, as the heights of the models and the terrain. Empty: the foot of the models, or for areas the terrain where they are",
    },
    ParamSpec { key: "waves", label: "Waves", kind: ParamKind::Choice(WAVES), default: "0.35", hint: "" },
    ParamSpec {
        key: "terrain_colours",
        label: "Terrain colours",
        kind: ParamKind::Choice(TERRAIN_COLOURS),
        default: "grass",
        hint: "For the terrain on Terrain, where no map from Map covers it",
    },
    ParamSpec {
        key: "night",
        label: "Day or night",
        kind: ParamKind::Choice(NIGHTS),
        default: "day",
        hint: "At night the moon stands where the sun would, by Sun from and Sun height, with stars around it. Lit windows and lamps light the streets. The sky is used as background",
    },
    ParamSpec {
        key: "snow_fall",
        label: "Falling snow in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Flakes in the air in front of the camera: 20 is a few, 100 a snowstorm. The sky clouds over",
    },
    ParamSpec {
        key: "snow_cover",
        label: "Snow on the ground in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Snow lying on roofs, the ground and the trees, on what is flat first: 30 is a dusting, 100 white all over",
    },
    ParamSpec {
        key: "rain",
        label: "Rain in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Streaks of rain in the air, a grey sky, and the ground wet. 50 is a shower, 100 a downpour",
    },
    ParamSpec {
        key: "wet",
        label: "Wet ground in %",
        kind: ParamKind::Text,
        default: "",
        hint: "Darker ground with puddles that mirror the buildings and the sky, as after rain. Empty: as wet as the rain makes it",
    },
    ParamSpec {
        key: "lightning",
        label: "Lightning",
        kind: ParamKind::Choice(LIGHTNINGS),
        default: "none",
        hint: "Bolts beyond the models, whose flash lights the scene from there",
    },
    ParamSpec {
        key: "fog",
        label: "Valley fog in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Fog lying in the low parts, so that the hills and the tall buildings stand out of it. 40 is a morning mist",
    },
    ParamSpec {
        key: "fog_height",
        label: "Fog, how deep",
        kind: ParamKind::Text,
        default: "",
        hint: "In metres: how high the fog reaches before it is thin. Empty: a quarter of the heights of the scene",
    },
    ParamSpec {
        key: "wind",
        label: "Wind in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Slants the rain and the snow, raises waves, lays down the smoke, bends the trees and moves the shadows of the clouds. 30 is a breeze, 100 a gale",
    },
    ParamSpec { key: "wind_from", label: "Wind from", kind: ParamKind::Text, default: "225", hint: "Degrees clockwise from north: 225 is a south-west wind" },
    ParamSpec {
        key: "sun_rays",
        label: "Rays of sun in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Rays that fan out from the sun through gaps in clouds, haze and fog. Best with the sun low in the picture and some cloud",
    },
    ParamSpec {
        key: "cloud_shadows",
        label: "Shadows of clouds in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Large soft shadows of clouds over the land, as on a day of sun and cloud. Seen best over a large terrain",
    },
    ParamSpec {
        key: "rainbow",
        label: "Rainbow in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "A bow opposite the sun, 42° around the point straight away from it, with a fainter second one outside it. Seen when the sun is lower than 42° and behind the camera. The bow is wide: a Lens angle of 70 to 90 shows it, and a lower sun lifts it higher. In a studio it stands on the room behind the models",
    },
    ParamSpec {
        key: "frost",
        label: "Frost and rime in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "White rime on roofs, walls, trees and the ground, glinting, and ferns of ice on the windows. 30 is a frosty morning, 100 all white",
    },
    ParamSpec {
        key: "ice",
        label: "Ice on the water in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "The water frozen: grey-blue ice with cracks, and snow blown over it as there is snow on the ground. Needs Water",
    },
    ParamSpec {
        key: "icicles",
        label: "Icicles in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Icicles hanging under the eaves of the roofs: 30 a few, 100 a row along each",
    },
    ParamSpec {
        key: "smoke",
        label: "Smoke from chimneys in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "The share of the buildings with smoke or steam rising from their highest point, as on a cold still morning. The wind lays it down",
    },
    ParamSpec {
        key: "advent",
        label: "Advent lights in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "The share of the lit windows with an advent candlestick on the sill or a star hanging in it. Needs lit windows",
    },
    ParamSpec {
        key: "lamp_cones",
        label: "Light cones under lamps in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "The air lit under each lamp on Lights, as cones of light, stronger in fog, haze, snow and rain",
    },
    ParamSpec {
        key: "trails",
        label: "Car light trails in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Lines on Lights are roads: along them the lights of cars make streaks, white one way and red the other, as in a long exposure at night",
    },
    ParamSpec {
        key: "fireworks",
        label: "Fireworks",
        kind: ParamKind::Text,
        default: "0",
        hint: "How many bursts in the sky over the scene, such as 5. Best at night",
    },
    ParamSpec {
        key: "tilt_shift",
        label: "Tilt-shift in %",
        kind: ParamKind::Text,
        default: "0",
        hint: "Sharp in a band across the middle and blurred above and below it, with livelier colours, so that a town looks like a model of itself. Best from above",
    },
    ParamSpec {
        key: "season",
        label: "Trees, season",
        kind: ParamKind::Choice(SEASONS),
        default: "summer",
        hint: "For the trees on Trees",
    },
    ParamSpec {
        key: "outlines",
        label: "Lines along edges",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "Dark lines where one thing stands in front of another and where a surface turns, as in a drawing. On with the look A drawing",
    },
    ParamSpec {
        key: "names_by",
        label: "Names from",
        kind: ParamKind::Attribute("Names"),
        default: "",
        hint: "The attribute with the names of the points on Names, written floating over their places, with a line down to each",
    },
    ParamSpec {
        key: "names_height",
        label: "Names, how high",
        kind: ParamKind::Text,
        default: "30",
        hint: "In metres above the ground, or above the height of the point when it has one",
    },
    ParamSpec {
        key: "quality",
        label: "Quality",
        kind: ParamKind::Choice(QUALITY),
        default: "16",
        hint: "Rays per pixel. More gives smoother shadows and edges, and takes longer: Best is four times Good. With reflections, more bounces, water and clouds, a picture can take some minutes",
    },
];

pub(super) fn run_scene(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    // The streams on Models, kept as they came, so that the log can count them.
    let parts: Vec<std::sync::Arc<crate::model::Table>> = ctx.parts.iter().filter(|x| x.port == "Models").map(|x| x.table.clone()).collect();
    let models: Vec<&crate::model::Table> = parts.iter().map(|t| t.as_ref()).collect();
    let meshes: Vec<&crate::mesh::Mesh> = models.iter().filter_map(|t| t.mesh.as_deref()).collect();
    if models.len() > meshes.len() {
        ctx.notes.warn(format!(
            "{} streams on Models have no 3D model and were left out. Read an OBJ or glTF file, or make one with Extrude or Generate TIN.",
            models.len() - meshes.len()
        ));
    }
    // Models in different coordinate systems stand far apart; say so.
    let systems: std::collections::BTreeSet<u32> = models.iter().filter(|t| t.mesh.is_some()).filter_map(|t| t.crs).collect();
    if systems.len() > 1 {
        ctx.notes.warn(format!(
            "The models on Models are in {} coordinate systems ({}), so they stand far apart. Reproject them to one before they are made 3D.",
            systems.len(),
            systems.iter().map(|c| format!("EPSG:{}", c)).collect::<Vec<_>>().join(", ")
        ));
    }
    let terrain = input(inp, "Terrain").grid.as_deref();
    // Roofs coloured by their kind, in a copy of each model that has the attribute.
    let roof_by = p.get("roof_by").filter(|s| !s.trim().is_empty());
    let roofed: Vec<Option<crate::mesh::Mesh>> = models
        .iter()
        .map(|t| match (&roof_by, t.mesh.as_deref()) {
            (Some(a), Some(m)) => super::scene096::colour_roofs(m, t, a).map(|(m, kinds)| {
                ctx.notes.info(format!("roofs of {} kinds coloured by {}", kinds, a));
                m
            }),
            _ => None,
        })
        .collect();
    if let Some(a) = &roof_by {
        if roofed.iter().all(|r| r.is_none()) {
            ctx.notes.warn(format!("No model on Models has the attribute {}, so the roofs keep their materials.", a));
        }
    }
    let mut meshes: Vec<&crate::mesh::Mesh> = models.iter().zip(&roofed).filter_map(|(t, r)| r.as_ref().or(t.mesh.as_deref())).collect();
    // Trees on the points on Trees, on the terrain, or on the lowest of the models.
    let tree_table = input(inp, "Trees");
    let tree_mesh = if tree_table.features.is_empty() {
        None
    } else {
        let crs = models.iter().find_map(|t| t.crs).or_else(|| terrain.and_then(|g| g.crs));
        let moved;
        let tt = match (crs, tree_table.crs) {
            (Some(a), Some(b)) if a != b => {
                moved = reproject(tree_table, a, ctx.backend)?;
                &moved
            }
            _ => tree_table,
        };
        let pts = super::scene096::tree_points(p, tt)?;
        let foot = meshes.iter().filter_map(|m| m.bounds()).map(|(lo, _)| lo[2]).fold(f64::INFINITY, f64::min);
        let foot = if foot.is_finite() { foot } else { 0.0 };
        let ground = |x: f64, y: f64| terrain.and_then(|g| crate::terrain::height_at(g, x, y)).unwrap_or(foot);
        if pts.is_empty() {
            ctx.notes.warn("Trees has no points, so no trees are drawn.");
            None
        } else {
            ctx.notes.info(format!("{} trees", fmt_count(pts.len() as u64)));
            let mut trees = super::scene096::trees(&pts, &ground, &p.get("season").unwrap_or_default());
            let wind = number_param(p, "wind", "The wind")?.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
            if wind > 0.0 {
                super::scene096::bend(&mut trees, number_param(p, "wind_from", "Wind from")?.unwrap_or(225.0), wind);
            }
            Some(trees)
        }
    };
    if let Some(t) = &tree_mesh {
        meshes.push(t);
    }
    // A TIN, as from Generate TIN, is a terrain of triangles: drawn as a model.
    if terrain.is_none() {
        if let Some(tin) = input(inp, "Terrain").mesh.as_deref() {
            meshes.push(tin);
            ctx.notes.info("the TIN on Terrain is drawn as it is, with its triangles".to_string());
        }
    }
    let map = input(inp, "Map").image.as_deref();
    if meshes.is_empty() && terrain.is_none() {
        return err("Connect a 3D model, such as an input that reads an OBJ file, to Models, or a terrain model to Terrain.");
    }
    let num = |k: &str, what: &str, d: f64| number_param(p, k, what).map(|v| v.unwrap_or(d));
    let pct = |k: &str, what: &str| -> Result<f32> { Ok((num(k, what, 0.0)?.clamp(0.0, 100.0) / 100.0) as f32) };
    let size = |k: &str, what: &str, d: f64| -> Result<usize> {
        let v = num(k, what, d)?.round();
        if !(16.0..=12000.0).contains(&v) {
            return err(format!("{} must be from 16 to 12000 pixels.", what));
        }
        Ok(v as usize)
    };
    // A background colour given while the background is still the studio is what is wanted.
    let own_colour = p.values.get("background_colour").and_then(|v| v.as_str()).is_some_and(|c| !c.trim().is_empty());
    let background = match p.get("background").unwrap_or_else(|| "studio".into()) {
        b if b == "studio" && own_colour => "colour".to_string(),
        b => b,
    };
    let mut s = Settings {
        width: size("width", "The width", 1600.0)?,
        height: size("height", "The height", 1000.0)?,
        azimuth: num("view_azimuth", "Looking from", 215.0)?,
        elevation: num("elevation", "The camera height", 25.0)?,
        fov: num("fov", "The lens angle", 35.0)?,
        zoom: num("zoom", "The zoom", 100.0)? / 100.0,
        distance: match number_param(p, "distance", "The camera distance")? {
            Some(d) if d <= 0.0 => return err("The camera distance must be more than 0 metres, or empty."),
            d => d,
        },
        terrain_colours: match p.get("terrain_colours").as_deref() {
            Some("land") => Some(crate::terrain::Colours::Land),
            Some("mountains") => Some(crate::terrain::Colours::Mountains),
            Some("greys") => Some(crate::terrain::Colours::Greys),
            Some("white") => Some(crate::terrain::Colours::White),
            _ => None,
        },
        sun_azimuth: num("sun_azimuth", "Sun from", 160.0)?,
        sun_altitude: num("sun_altitude", "The sun height", 45.0)?,
        sun_size: p.get("softness").and_then(|v| v.parse().ok()).unwrap_or(3.0),
        sun_strength: (num("sun_strength", "The sunlight", 100.0)? / 100.0) as f32,
        sky_strength: (num("sky_strength", "The skylight", 100.0)? / 100.0) as f32,
        samples: p.get("quality").and_then(|v| v.parse().ok()).unwrap_or(16),
        background: match background.as_str() {
            "sky" => Background::Sky,
            "clear" | "colour" => Background::Clear,
            _ => Background::Studio,
        },
        ground: p.get("floor").as_deref() != Some("false"),
        clouds: (num("clouds", "The cloud cover", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        haze: (num("haze", "The haze", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        bounces: p.get("bounces").and_then(|v| v.parse().ok()).unwrap_or(1),
        reflections: p.flag("reflections"),
        look: match p.get("look").as_deref() {
            Some("white") => Look::White,
            Some("wood") => Look::Wood,
            Some("sketch") => Look::Sketch,
            _ => Look::Materials,
        },
        glass_names: p.get("glass_names").unwrap_or_default().split([',', ';']).map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect(),
        water: None,
        glow_names: p.get("glow_names").unwrap_or_default().split([',', ';']).map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect(),
        lit_windows: (num("lit_windows", "The lit windows", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        windows: p.flag("windows"),
        lights: Vec::new(),
        blur: (num("blur", "The blur", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        focus: match number_param(p, "focus", "The focus distance")? {
            Some(f) if f <= 0.0 => return err("The focus distance must be more than 0 metres, or empty."),
            f => f,
        },
        parallel: p.get("projection").as_deref() == Some("parallel"),
        sun_tint: [1.0; 3],
        sky_tint: [1.0; 3],
        vivid: 0.0,
        snow_fall: (num("snow_fall", "The falling snow", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        rain: (num("rain", "The rain", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        snow_cover: (num("snow_cover", "The snow on the ground", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        wet: 0.0,
        lightning: match p.get("lightning").as_deref() {
            Some("bolt") => 1,
            Some("storm") => 3,
            _ => 0,
        },
        fog: (num("fog", "The valley fog", 0.0)?.clamp(0.0, 100.0) / 100.0) as f32,
        fog_height: match number_param(p, "fog_height", "How deep the fog is")? {
            Some(h) if h <= 0.0 => return err("How deep the fog is must be more than 0 metres, or empty."),
            h => h,
        },
        night: matches!(p.get("night").as_deref(), Some("moon" | "aurora")),
        aurora: if p.get("night").as_deref() == Some("aurora") { 1.0 } else { 0.0 },
        outlines: p.flag("outlines") || p.get("look").as_deref() == Some("sketch"),
        names: Vec::new(),
        names_height: num("names_height", "How high the names are", 30.0)?,
        frost: pct("frost", "The frost")?,
        ice: pct("ice", "The ice")?,
        icicles: pct("icicles", "The icicles")?,
        smoke: pct("smoke", "The smoke")?,
        advent: pct("advent", "The advent lights")?,
        sun_rays: pct("sun_rays", "The rays of sun")?,
        cloud_shadows: pct("cloud_shadows", "The shadows of clouds")?,
        rainbow: pct("rainbow", "The rainbow")?,
        lamp_cones: pct("lamp_cones", "The light cones")?,
        lamp_height: number_param(p, "light_height", "The height of the lamps")?.unwrap_or(5.0) as f32,
        wind: pct("wind", "The wind")?,
        wind_from: num("wind_from", "Wind from", 225.0)?,
        trails: Vec::new(),
        trail_strength: pct("trails", "The car light trails")?,
        fireworks: num("fireworks", "The fireworks", 0.0)?.clamp(0.0, 40.0).round() as u32,
        tilt_shift: pct("tilt_shift", "The tilt-shift")?,
    };
    if s.ice > 0.0 && p.get("water").is_none_or(|w| w == "none") {
        ctx.notes.warn("Ice needs water: choose Water, a surface at a height or the areas on Water.");
    }
    if s.advent > 0.0 && s.lit_windows <= 0.0 {
        ctx.notes.warn("Advent lights stand in lit windows: set Lit windows in % too.");
    }
    if let Some((elevation, parallel, fov)) = super::scene096::camera_of(&p.get("camera").unwrap_or_default()) {
        s.elevation = elevation;
        s.parallel = parallel;
        s.fov = fov;
    }
    let (sun_tint, sky_tint, vivid, wider) = mood_of(&p.get("mood").unwrap_or_default());
    s.sun_tint = sun_tint;
    s.sky_tint = sky_tint;
    s.vivid = vivid;
    s.sun_size += wider;
    if s.look == Look::Sketch {
        // A drawing: paper, and soft light from all round rather than hard shadows.
        s.sky_strength *= 1.25;
    }
    s.wet = match number_param(p, "wet", "The wet ground")? {
        Some(v) => (v.clamp(0.0, 100.0) / 100.0) as f32,
        None => s.rain,
    };
    // Weather clouds the sky over, and a storm darkens it.
    if s.rain > 0.0 {
        s.clouds = s.clouds.max(0.7 + 0.3 * s.rain);
        s.haze = s.haze.max(0.15 + 0.25 * s.rain);
    }
    if s.snow_fall > 0.0 {
        s.clouds = s.clouds.max(0.65 + 0.35 * s.snow_fall);
        s.haze = s.haze.max(0.1 + 0.4 * s.snow_fall);
    }
    if s.lightning >= 3 {
        s.clouds = s.clouds.max(0.92);
        s.sun_strength *= 0.3;
        s.sky_strength *= 0.45;
        s.sky_tint = [s.sky_tint[0] * 0.8, s.sky_tint[1] * 0.85, s.sky_tint[2] * 1.05];
    }
    if s.night && s.background == Background::Studio {
        s.background = Background::Sky;
    }
    let backdrop = if background == "colour" {
        match crate::mapdraw::parse_color(&p.get("background_colour").unwrap_or_else(|| "#ffd6e0".into())) {
            Ok(Some(c)) => Some([c[0], c[1], c[2]]),
            Ok(None) => Some([255, 214, 224]),
            Err(e) => return err(format!("The background colour: {}", e)),
        }
    } else {
        None
    };
    let title = p.get("title");
    s.water = water_of(p, inp, &meshes, terrain, models.iter().find_map(|t| t.crs), ctx)?;
    (s.lights, s.trails) = lights_of(p, inp, &meshes, terrain, models.iter().find_map(|t| t.crs), ctx)?;
    if !s.trails.is_empty() && s.trail_strength <= 0.0 {
        ctx.notes.warn("The lines on Lights are roads for car light trails: set Car light trails in % to see them.");
    }
    s.names = names_of(p, inp, &meshes, terrain, models.iter().find_map(|t| t.crs), ctx)?;
    if s.width * s.height > 40_000_000 {
        return err("The picture can have at most 40 million pixels.");
    }
    // Where the models are, as latitude and longitude, for the sun of a date and a time.
    let place = |ctx: &mut NodeCtx| -> Result<(f64, f64)> {
        let crs = models.iter().find_map(|t| t.crs).or_else(|| terrain.and_then(|g| g.crs));
        let middle = meshes.iter().filter_map(|m| m.bounds()).next().map(|(lo, hi)| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]).or_else(|| {
            terrain.map(|g| {
                let gt = g.geotransform;
                [gt[0] + gt[1] * g.width as f64 / 2.0, gt[3] + gt[5] * g.height as f64 / 2.0]
            })
        });
        Ok(match (crs, middle) {
            (Some(c), Some(m)) => {
                let b = ctx.backend("Finding the place of the model")?;
                let (mut xs, mut ys) = (vec![m[0]], vec![m[1]]);
                b.transform(c, 4326, &mut xs, &mut ys)?;
                (ys[0], xs[0])
            }
            _ => {
                ctx.notes.warn("The model has no coordinate system, so the sun stands as over Göteborg. Set the coordinate system and the origin of the model in its input.");
                (57.7, 12.0)
            }
        })
    };
    let clock = match p.get("clock").as_deref() {
        Some("utc") => crate::sun::Clock::Utc,
        Some("computer") => crate::sun::Clock::Computer,
        _ => crate::sun::Clock::Swedish,
    };
    let sun_at = |date: &str, time: &str, lat: f64, lon: f64| -> Result<(f64, f64)> {
        let when = crate::sun::time_on(date, time, clock)
            .ok_or_else(|| GbError(format!("\"{} {}\" is not a date and a time. Write them as 2026-03-20 and 12:00.", date, time)))?;
        Ok(crate::sun::position(when, lat, lon))
    };
    // The pictures to render: one, or one for each time of each date of a sun study.
    let mut frames: Vec<(String, String)> = Vec::new();
    match p.get("sun").as_deref() {
        Some("date") => frames.push((p.get("date").unwrap_or_default(), p.get("time").unwrap_or_else(|| "12:00".into()))),
        Some("study") => {
            let dates: Vec<String> =
                p.get("study_dates").unwrap_or_default().split([',', ';', ' ']).map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect();
            if dates.is_empty() {
                return err("Give the dates of the sun study, such as 2026-03-20, 2026-06-21.");
            }
            let minutes = |t: &str, what: &str| -> Result<i64> {
                let t = t.trim();
                let (h, m) = t.split_once(':').unwrap_or((t, "0"));
                match (h.parse::<i64>(), m.parse::<i64>()) {
                    (Ok(h), Ok(m)) if (0..=24).contains(&h) && (0..60).contains(&m) => Ok(h * 60 + m),
                    _ => err(format!("{} must be a time such as 09:00, not \"{}\".", what, t)),
                }
            };
            let from = minutes(&p.get("study_from").unwrap_or_else(|| "09:00".into()), "From")?;
            let to = minutes(&p.get("study_to").unwrap_or_else(|| "17:00".into()), "To")?;
            let every = match number_param(p, "study_every", "Every so many hours")? {
                None => 120,
                Some(h) if h > 0.0 => (h * 60.0).round() as i64,
                Some(_) => return err("Every so many hours must be more than 0."),
            };
            if to < from {
                return err("To must come after From.");
            }
            for d in &dates {
                let mut m = from;
                while m <= to {
                    frames.push((d.clone(), format!("{:02}:{:02}", m / 60, m % 60)));
                    m += every.max(10);
                }
            }
            if frames.len() > 60 {
                return err(format!("That would be {} pictures. Take fewer dates, or more hours between them.", frames.len()));
            }
        }
        Some("day") => {
            // From sunrise to sunset of the date, every so many hours, with one picture just
            // after the sun is up and one just before it sets.
            let date = p.get("date").unwrap_or_else(|| "2026-03-20".into());
            let (lat, lon) = place(ctx)?;
            let every = match number_param(p, "study_every", "Every so many hours")? {
                None => 60,
                Some(h) if h > 0.0 => (h * 60.0).round().max(10.0) as i64,
                Some(_) => return err("Every so many hours must be more than 0."),
            };
            let up = |m: i64| -> Result<bool> { Ok(sun_at(&date, &format!("{:02}:{:02}", m / 60, m % 60), lat, lon)?.1 > 0.5) };
            let mut first = None;
            let mut last = None;
            let mut m = 0;
            while m < 24 * 60 {
                if up(m)? {
                    first.get_or_insert(m);
                    last = Some(m);
                }
                m += 10;
            }
            let (Some(a), Some(b)) = (first, last) else {
                return err(format!("The sun does not rise on {} at the place of the model.", date));
            };
            let mut m = a;
            while m <= b {
                frames.push((date.clone(), format!("{:02}:{:02}", m / 60, m % 60)));
                m += every;
            }
            if frames.last().is_some_and(|f| f.1 != format!("{:02}:{:02}", b / 60, b % 60)) {
                frames.push((date.clone(), format!("{:02}:{:02}", b / 60, b % 60)));
            }
            if frames.len() > 60 {
                return err(format!("That would be {} pictures. Take more hours between them.", frames.len()));
            }
        }
        _ => {}
    }
    let lat_lon = if frames.is_empty() { None } else { Some(place(ctx)?) };
    // The picture is rendered on other threads; this one tells how far it has come, and
    // stops it when the run is stopped.
    let rows = std::sync::atomic::AtomicU64::new(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let started = std::time::Instant::now();
    let count = frames.len().max(1);
    let total = (s.height * count) as u64;
    let mut pictures: Vec<(String, crate::model::RasterImage)> = Vec::new();
    for k in 0..count {
        let mut fs = s.clone();
        let mut label = String::new();
        if let (Some((date, time)), Some((lat, lon))) = (frames.get(k), lat_lon) {
            let (az, alt) = sun_at(date, time, lat, lon)?;
            fs.sun_azimuth = az;
            fs.sun_altitude = alt;
            label = format!("{} {}", date, time);
            if count == 1 {
                ctx.notes.info(format!("the sun from {:.0}°, {:.1}° high", az, alt));
                if alt <= 0.0 {
                    ctx.notes.warn("The sun is below the horizon then, so only the sky lights the scene.");
                }
            }
            if alt <= -1.0 {
                fs.sun_strength = 0.0;
                if count > 1 {
                    label.push_str(", sun below the horizon");
                }
            }
        }
        let image = std::thread::scope(|scope| {
            let worker = scope.spawn(|| crate::render3d::render(&meshes, terrain, map, &fs, &rows, &stop));
            while !worker.is_finished() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                (ctx.progress)("rendered", rows.load(std::sync::atomic::Ordering::Relaxed), Some(total));
                if ctx.stopped() {
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            worker.join().ok().flatten()
        })
        .ok_or_else(|| GbError(crate::model::STOPPED.into()))?;
        let mut image = image;
        if let Some(bg) = backdrop {
            for px in image.rgba.as_chunks_mut::<4>().0.iter_mut() {
                let a = px[3] as u32;
                for k in 0..3 {
                    px[k] = ((px[k] as u32 * a + bg[k] as u32 * (255 - a) + 127) / 255) as u8;
                }
                px[3] = 255;
            }
        }
        if let Some(t) = &title {
            write_title(&mut image, t)?;
        }
        pictures.push((label, image));
    }
    let tris: usize = meshes.iter().map(|m| m.tris.len()).sum();
    ctx.notes.info(format!(
        "{} triangles, {} × {} pixels, {} rays per pixel{}, {:.1} s",
        crate::model::fmt_count(tris as u64),
        s.width,
        s.height,
        s.samples,
        if count > 1 { format!(", {} pictures", count) } else { String::new() },
        started.elapsed().as_secs_f64()
    ));
    if count == 1 {
        let mut table = pictures.remove(0).1.into_table(None);
        table.crs = None;
        return Ok(vec![("Image".into(), table)]);
    }
    // Each picture in the folder, if one is given.
    if let Some(folder) = p.get("study_folder").filter(|f| !f.trim().is_empty()) {
        let dir = ctx.resolve(&folder);
        std::fs::create_dir_all(&dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        for (label, img) in &pictures {
            let name = format!("sun_{}.png", label.split(',').next().unwrap_or(label).replace([' ', ':'], "_"));
            let path = dir.join(name);
            image::save_buffer(&path, &img.rgba, img.width as u32, img.height as u32, image::ColorType::Rgba8)
                .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
            ctx.files.push(path);
        }
    }
    // The pictures one after another, as a moving picture.
    if let Some(gif) = p.get("gif").filter(|f| !f.trim().is_empty()) {
        let path = ctx.resolve(&gif);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        write_gif(&path, &pictures, title.is_none())?;
        ctx.notes.info(format!("{} pictures one after another in {}", pictures.len(), path.display()));
        ctx.files.push(path);
    }
    let dates_count = frames.iter().map(|f| &f.0).collect::<std::collections::BTreeSet<_>>().len().max(1);
    let columns = if p.get("sun").as_deref() == Some("day") { (count as f64).sqrt().ceil() as usize } else { count.div_ceil(dates_count) };
    let sheet = sun_sheet(&pictures, columns)?;
    let mut table = sheet.into_table(None);
    table.crs = None;
    Ok(vec![("Image".into(), table)])
}

/// The pictures one after another in a GIF that runs round, each with its time written on
/// it, a little longer on the last.
pub(crate) fn write_gif(path: &std::path::Path, pictures: &[(String, crate::model::RasterImage)], labels: bool) -> Result<()> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let file = std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    let mut enc = GifEncoder::new_with_speed(std::io::BufWriter::new(file), 10);
    enc.set_repeat(Repeat::Infinite).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    for (k, (label, img)) in pictures.iter().enumerate() {
        let mut img = img.clone();
        if labels && !label.is_empty() {
            write_title(&mut img, label)?;
        }
        let buf = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.rgba).ok_or_else(|| GbError("A picture of the wrong size.".into()))?;
        let ms = if k + 1 == pictures.len() { 1600 } else { 700 };
        let frame = image::Frame::from_parts(buf, 0, 0, image::Delay::from_numer_denom_ms(ms, 1));
        enc.encode_frame(frame).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    Ok(())
}

/// A title at the top of a picture, in the middle, dark on a light picture and light on a
/// dark one, with a soft shadow.
fn write_title(img: &mut crate::model::RasterImage, title: &str) -> Result<()> {
    let (w, h) = (img.width, img.height);
    let mut pm = tiny_skia::Pixmap::new(w as u32, h as u32).ok_or_else(|| GbError("The picture is too large for a title.".into()))?;
    // The pixmap holds colours times their opacity.
    for (d, q) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(img.rgba.as_chunks::<4>().0.iter()) {
        let a = q[3] as u32;
        for k in 0..3 {
            d[k] = ((q[k] as u32 * a + 127) / 255) as u8;
        }
        d[3] = q[3];
    }
    let text = crate::chart::Text::new()?;
    let size = (h as f32 / 16.0).clamp(14.0, 120.0);
    let width = text.width(title, size, true);
    let x = (w as f32 - width) / 2.0;
    let y = size * 1.6;
    // How light the picture is where the title goes.
    let band = &img.rgba[..(w * (size * 2.0) as usize).min(w * h) * 4];
    let lum = band
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| (p[0] as f32 * 0.3 + p[1] as f32 * 0.59 + p[2] as f32 * 0.11) * p[3] as f32 / 255.0 + 255.0 * (1.0 - p[3] as f32 / 255.0))
        .sum::<f32>()
        / (band.len() / 4).max(1) as f32;
    let (ink, shade) = if lum > 140.0 { ([52, 48, 64], [255, 255, 255]) } else { ([255, 255, 255], [30, 30, 40]) };
    let off = (size / 18.0).max(1.0);
    text.draw(&mut pm, title, x + off, y + off, size, true, shade, 0.0);
    text.draw(&mut pm, title, x, y, size, true, ink, 0.0);
    for (q, d) in img.rgba.as_chunks_mut::<4>().0.iter_mut().zip(pm.data().as_chunks::<4>().0.iter()) {
        let a = d[3] as u32;
        for k in 0..3 {
            q[k] = (d[k] as u32 * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
        }
        q[3] = d[3];
    }
    Ok(())
}

/// The pictures of a sun study on one sheet: a row for each date, each picture with its
/// date and time above it.
fn sun_sheet(pictures: &[(String, crate::model::RasterImage)], columns: usize) -> Result<crate::model::RasterImage> {
    let (w, h) = (pictures[0].1.width, pictures[0].1.height);
    let columns = columns.max(1);
    let rows = pictures.len().div_ceil(columns);
    let (gap, band) = (16usize, (h / 14).clamp(26, 60));
    let (sw, sh) = (gap + columns * (w + gap), gap + rows * (h + band + gap));
    let mut pm = tiny_skia::Pixmap::new(sw as u32, sh as u32).ok_or_else(|| GbError("The sheet of the sun study is too large.".into()))?;
    pm.fill(tiny_skia::Color::WHITE);
    let text = crate::chart::Text::new()?;
    let size = band as f32 * 0.55;
    for (k, (label, img)) in pictures.iter().enumerate() {
        let (c, r) = (k % columns, k / columns);
        let (x0, y0) = (gap + c * (w + gap), gap + r * (h + band + gap));
        text.draw(&mut pm, label, x0 as f32, (y0 + band) as f32 - band as f32 * 0.3, size, true, [40, 40, 40], 0.0);
        let data = pm.data_mut();
        for y in 0..h {
            for x in 0..w {
                let s = (y * w + x) * 4;
                let d = ((y0 + band + y) * sw + x0 + x) * 4;
                let a = img.rgba[s + 3] as u32;
                // Laid on white, by how opaque it is.
                for i in 0..3 {
                    data[d + i] = ((img.rgba[s + i] as u32 * a + 255 * (255 - a)) / 255) as u8;
                }
                data[d + 3] = 255;
            }
        }
    }
    Ok(crate::model::RasterImage { vector: None, width: sw, height: sh, rgba: pm.data().to_vec(), geotransform: [0.0, 1.0, 0.0, sh as f64, 0.0, -1.0] })
}

/// The water of a scene: none, a surface at a height, or the areas on Water.
/// The lamps on Lights, in the coordinate system of the models, at their own height or
/// some metres above the ground.
/// The lamps on Lights, and its lines as roads, each a row of points on the ground.
type LampsAndRoads = (Vec<crate::render3d::PointLight>, Vec<Vec<[f64; 3]>>);

fn lights_of(
    p: &P,
    inp: &Inputs,
    meshes: &[&crate::mesh::Mesh],
    terrain: Option<&crate::model::Grid>,
    crs: Option<u32>,
    ctx: &mut NodeCtx,
) -> Result<LampsAndRoads> {
    let t = input(inp, "Lights");
    if t.features.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let moved;
    let t = match (crs.or_else(|| terrain.and_then(|g| g.crs)), t.crs) {
        (Some(a), Some(b)) if a != b => {
            moved = reproject(t, a, ctx.backend)?;
            &moved
        }
        _ => t,
    };
    let strength = (number_param(p, "light_strength", "The strength of the lamps")?.unwrap_or(100.0).max(0.0) / 100.0) as f32;
    let colour = match crate::mapdraw::parse_color(&p.get("light_colour").unwrap_or_else(|| "#ffc78a".into())) {
        Ok(Some(c)) => [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0],
        Ok(None) => crate::render3d::WARM_LIGHT,
        Err(e) => return err(format!("The colour of the lamps: {}", e)),
    };
    let above = number_param(p, "light_height", "The height of the lamps")?.unwrap_or(5.0);
    let foot = meshes.iter().filter_map(|m| m.bounds()).map(|(lo, _)| lo[2]).fold(f64::INFINITY, f64::min);
    let foot = if foot.is_finite() { foot } else { 0.0 };
    // Without a terrain, the ground is the lowest surface of the models that faces up there,
    // such as a TIN, when that is not too much to look through.
    let tris: usize = meshes.iter().map(|m| m.tris.len()).sum();
    let look = terrain.is_none() && tris.saturating_mul(t.features.len()) < 400_000_000;
    let on_models = |x: f64, y: f64| -> Option<f64> {
        let mut best: Option<f64> = None;
        for m in meshes {
            for tri in &m.tris {
                let [a, b, c] = tri.v.map(|i| m.positions[i as usize]);
                let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if det.abs() < 1e-12 {
                    continue;
                }
                let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
                let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
                let l3 = 1.0 - l1 - l2;
                if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                    continue;
                }
                let z = l1 * a[2] + l2 * b[2] + l3 * c[2];
                best = Some(best.map_or(z, |v: f64| v.min(z)));
            }
        }
        best
    };
    let ground = |x: f64, y: f64| match terrain {
        Some(g) => crate::terrain::height_at(g, x, y).unwrap_or(foot),
        None if look => on_models(x, y).unwrap_or(foot),
        None => foot,
    };
    let mut out = Vec::new();
    let mut roads: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut not_points = 0usize;
    // A road on the ground, a point every 5 m or so, so that it follows the terrain.
    let mut road = |l: &geo::LineString<f64>| {
        use geo::Densify;
        let l = geo::Euclidean.densify(l, 5.0);
        roads.push(l.0.iter().map(|c| [c.x, c.y, ground(c.x, c.y)]).collect());
    };
    for f in &t.features {
        let pts: Vec<geo::Coord<f64>> = match &f.geom {
            Some(geo::Geometry::Point(q)) => vec![q.0],
            Some(geo::Geometry::MultiPoint(m)) => m.0.iter().map(|q| q.0).collect(),
            Some(geo::Geometry::LineString(l)) => {
                road(l);
                continue;
            }
            Some(geo::Geometry::MultiLineString(m)) => {
                m.0.iter().for_each(&mut road);
                continue;
            }
            Some(_) => {
                not_points += 1;
                continue;
            }
            None => continue,
        };
        let own = f.heights();
        for (k, c) in pts.iter().enumerate() {
            let z = own.and_then(|z| z.get(k).copied()).unwrap_or_else(|| ground(c.x, c.y) + above);
            out.push(crate::render3d::PointLight { at: [c.x, c.y, z], colour, strength });
        }
    }
    if not_points > 0 {
        ctx.notes.warn(format!("{} features on Lights are neither points nor lines and were left out.", fmt_count(not_points as u64)));
    }
    if roads.iter().map(|r| r.len()).sum::<usize>() > 2_000_000 {
        return err("The roads on Lights are too long for car light trails. Take a smaller area.");
    }
    if out.len() > 100_000 {
        return err(format!("{} lamps are too many. Take at most 100 000.", fmt_count(out.len() as u64)));
    }
    Ok((out, roads))
}

fn water_of(
    p: &P,
    inp: &Inputs,
    meshes: &[&crate::mesh::Mesh],
    terrain: Option<&crate::model::Grid>,
    crs: Option<u32>,
    ctx: &mut NodeCtx,
) -> Result<Option<Water>> {
    let kind = p.get("water").unwrap_or_else(|| "none".into());
    if kind == "none" {
        return Ok(None);
    }
    let waves = p.get("waves").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.35);
    let foot = meshes.iter().filter_map(|m| m.bounds()).map(|(lo, _)| lo[2]).fold(f64::INFINITY, f64::min);
    let given = number_param(p, "water_level", "The water height")?;
    let mut areas: Vec<geo::Polygon<f64>> = Vec::new();
    if kind == "areas" {
        let t = input(inp, "Water");
        if t.is_empty() {
            return err("Water is set to the areas on Water, but no areas came in. Connect the lakes or the sea to Water.");
        }
        let moved;
        let t = match (crs.or_else(|| terrain.and_then(|g| g.crs)), t.crs) {
            (Some(a), Some(b)) if a != b => {
                ctx.notes.warn(format!("The water was reprojected from {} to {}.", crate::model::crs_label(Some(b)), crate::model::crs_label(Some(a))));
                moved = reproject(t, a, ctx.backend)?;
                &moved
            }
            _ => t,
        };
        for f in &t.features {
            if let Some(g) = &f.geom {
                areas.extend(crate::geom::polygons_of(g));
            }
        }
        if areas.is_empty() {
            return err("None of the features on Water is an area.");
        }
    }
    let level = match given {
        Some(z) => z,
        None => {
            // The terrain where the areas are, as a lake lies flat in a terrain model.
            let from_terrain = terrain.and_then(|g| {
                use geo::InteriorPoint;
                let zs: Vec<f64> = areas.iter().filter_map(|a| a.interior_point()).filter_map(|pt| g.at(pt.x(), pt.y())).map(|z| z as f64).collect();
                zs.iter().copied().reduce(f64::min).map(|z| z + 0.05)
            });
            match from_terrain {
                Some(z) => z,
                None if foot.is_finite() => foot,
                None => return err("Give the water height: there are neither models nor water areas on the terrain to take it from."),
            }
        }
    };
    ctx.notes.info(format!("water at {} m", crate::model::fmt_real((level * 100.0).round() / 100.0)));
    Ok(Some(Water { level, areas, waves }))
}

/// The names to float over the points on Names: each point on the ground, or at its own
/// height, and its name.
fn names_of(
    p: &P,
    inp: &Inputs,
    meshes: &[&crate::mesh::Mesh],
    terrain: Option<&crate::model::Grid>,
    crs: Option<u32>,
    ctx: &mut NodeCtx,
) -> Result<Vec<([f64; 3], String)>> {
    let t = input(inp, "Names");
    if t.features.is_empty() {
        return Ok(Vec::new());
    }
    let Some(by) = p.get("names_by").filter(|v| !v.trim().is_empty()) else {
        ctx.notes.warn("Points came in on Names, but no attribute is chosen under Names from, so no names are written.");
        return Ok(Vec::new());
    };
    let Some(k) = t.field_index(&by) else {
        return err(format!("The points on Names have no attribute {}.", by));
    };
    let crs = crs.or_else(|| terrain.and_then(|g| g.crs));
    let moved;
    let t = match (crs, t.crs) {
        (Some(a), Some(b)) if a != b => {
            moved = reproject(t, a, ctx.backend)?;
            &moved
        }
        _ => t,
    };
    let foot = meshes.iter().filter_map(|m| m.bounds()).map(|(lo, _)| lo[2]).fold(f64::INFINITY, f64::min);
    let foot = if foot.is_finite() { foot } else { 0.0 };
    let mut out = Vec::new();
    for f in &t.features {
        let name = f.attrs[k].to_text();
        let Some(geo::Geometry::Point(pt)) = &f.geom else { continue };
        if name.trim().is_empty() {
            continue;
        }
        let z =
            f.z.as_ref().and_then(|z| z.first().copied()).unwrap_or_else(|| terrain.and_then(|g| crate::terrain::height_at(g, pt.x(), pt.y())).unwrap_or(foot));
        out.push(([pt.x(), pt.y(), z], name.trim().to_string()));
    }
    if out.is_empty() {
        ctx.notes.warn("Names has no points with a name, so no names are written.");
    } else {
        ctx.notes.info(format!("{} names", fmt_count(out.len() as u64)));
    }
    Ok(out)
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "SceneRenderer",
        title: "Render 3D scene",
        hidden: false,
        group: Group::Transformer,
        category: "3D models and views",
        desc: "The picture of a 3D scene for presenting: models such as buildings read from OBJ, glTF, 3D Tiles or IFC, or made with Extrude or Generate TIN, rendered by following rays of light: lit by the sun, with soft shadows, and by the sky, which reaches into the shade and darkens corners, with their materials, textures and glass. On a light floor, under the sky as it is at that hour, or on the terrain connected to Terrain with the map on Map laid over it. Clouds, haze, water that mirrors, reflections and more bounces of the light, or the models white or in wood as a model. At dusk, lit windows, lamps at the points on Lights and materials that glow; depth of field blurs what is not in focus. The sun can stand where it does on a date and at a time, and a sun study gives a sheet of pictures through the day. A picture can take some minutes. For a quick look at terrain alone, use Quick terrain view.",
        inputs: &["Models", "Terrain", "Map", "Water", "Lights", "Trees", "Names"],
        outputs: &["Image"],
        ports: None,
        params: SCENE_PARAMS,
        run: run_scene,
        summary: |p| match p.get("sun").as_deref() {
            Some("study") => format!("sun study, {}", p.get("study_dates").unwrap_or_default()),
            Some("date") => format!("sun at {} {}", p.get("date").unwrap_or_default(), p.get("time").unwrap_or_else(|| "12:00".into())),
            _ => format!("from {}°, sun from {}°", p.get("view_azimuth").unwrap_or_else(|| "215".into()), p.get("sun_azimuth").unwrap_or_else(|| "225".into())),
        },
    },
];
