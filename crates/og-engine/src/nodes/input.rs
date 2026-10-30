//! Input: reads files, folders of files, files from the web and web services, and OpenStreetMap.

use super::*;

// ---------- Input (Reader) ----------

pub(super) const FILENAME_PARTS: &[(&str, &str)] = &[("stem", "Name without extension"), ("name", "Name with extension"), ("path", "Full path")];
/// What the cells of a raster become.
pub(super) const OBJ_UNITS: &[(&str, &str)] =
    &[("auto", "As the format says, or from its size"), ("m", "Metres"), ("cm", "Centimetres"), ("mm", "Millimetres"), ("ft", "Feet"), ("in", "Inches")];
pub(super) const OBJ_UP: &[(&str, &str)] = &[("auto", "As the format says: y for glTF, z for OBJ"), ("z", "z, as on maps"), ("y", "y, as in many 3D programs")];
pub(super) const DXF_BLOCKS: &[(&str, &str)] = &[("points", "Points with their attributes"), ("drawn", "Drawn out in lines")];
pub(super) const DXF_CLOSED: &[(&str, &str)] = &[("areas", "As areas"), ("lines", "As lines")];
pub(super) const CITY_LODS: &[(&str, &str)] =
    &[("finest", "The finest each has"), ("coarsest", "The coarsest each has"), ("0", "LoD0"), ("1", "LoD1"), ("2", "LoD2"), ("3", "LoD3")];
pub(super) const IFC_OPENINGS: &[(&str, &str)] = &[("cut", "Cut out"), ("leave", "Left as they are")];
pub(super) const IFC_SPACES: &[(&str, &str)] = &[("false", "Left out"), ("true", "Read, as volumes")];
pub(super) const IFC_PROPERTIES: &[(&str, &str)] = &[("all", "All, as attributes"), ("none", "None")];
pub(super) const WITHIN: &[(&str, &str)] = &[("all", "Everything"), ("rectangle", "Only within a rectangle")];
pub(super) const RASTER_MODES: &[(&str, &str)] = &[
    ("points", "Points, one per cell"),
    ("cells", "Squares, one per cell"),
    ("extent", "The outline of the raster"),
    ("image", "An image: shown as a picture, a click gives the values of a cell"),
    ("grid", "Heights or other values on a grid, for terrain"),
];
/// The file name filter a folder gets when none is given.
pub const DEFAULT_PATTERN: &str = "*.shp";

pub(super) const READER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "path",
        label: "File or folder",
        kind: ParamKind::Source,
        default: "",
        hint: "A file, also zipped; a folder, whose files that match the filter are read as one; or a web address (https://…)",
    },
    ParamSpec {
        key: "web_as",
        label: "The address is",
        kind: ParamKind::Choice(WEB_SERVICES),
        default: "auto",
        hint: "A service is read through its own pages, and only what reaches into a rectangle when one is set: OGC API Features (/collections), WFS, or an ArcGIS FeatureServer. A GeoTIFF (.tif) and a COPC point cloud (.copc.laz) are read where they are: only the rectangle, and a GeoTIFF from its overviews when read coarser",
    },
    ParamSpec {
        key: "connection",
        label: "API connection",
        kind: ParamKind::ApiConnection,
        default: "",
        hint: "A key or a login for the address, kept in the settings. Where the key goes in the address, write {{key}}",
    },
    ParamSpec {
        key: "keep_download",
        label: "Keep the fetched file",
        kind: ParamKind::Choice(YES_NO),
        default: "false",
        hint: "No fetches the file again in every run. Yes fetches it once and reads the copy after that",
    },
    ParamSpec {
        key: "pattern",
        label: "Files in the folder",
        kind: ParamKind::Text,
        default: DEFAULT_PATTERN,
        hint: "A filter such as *.shp. Several are separated by semicolons",
    },
    ParamSpec { key: "recursive", label: "Include subfolders", kind: ParamKind::Choice(YES_NO), default: "false", hint: "" },
    ParamSpec {
        key: "zip_file",
        label: "File in the zip",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as roads.gpkg, data/roads.gpkg or *.gpkg. Empty: the Shapefiles in it, or else the first data file",
    },
    ParamSpec {
        key: "layer",
        label: "Layers",
        kind: ParamKind::Layers,
        default: "",
        hint: "Tables in a GeoPackage, sheets in Excel. Empty reads the first one, * reads all",
    },
    ParamSpec { key: "header", label: "First row has names (Excel)", kind: ParamKind::Choice(YES_NO), default: "true", hint: "" },
    ParamSpec { key: "within", label: "Read", kind: ParamKind::Choice(WITHIN), default: "all", hint: "Only what reaches into a rectangle is read. In a GeoPackage, through its spatial index, which is quick" },
    ParamSpec { key: "min_x", label: "West, smallest x", kind: ParamKind::Text, default: "", hint: "A number such as 290000, or a variable of the model such as $(xmin)" },
    ParamSpec { key: "min_y", label: "South, smallest y", kind: ParamKind::Text, default: "", hint: "Such as 6390000 or $(ymin)" },
    ParamSpec { key: "max_x", label: "East, largest x", kind: ParamKind::Text, default: "", hint: "Such as 340000 or $(xmax)" },
    ParamSpec { key: "max_y", label: "North, largest y", kind: ParamKind::Text, default: "", hint: "Such as 6420000 or $(ymax)" },
    ParamSpec { key: "within_crs", label: "Rectangle in", kind: ParamKind::Crs, default: "", hint: "The coordinate system of the rectangle. Empty: that of the data" },
    ParamSpec {
        key: "limit",
        label: "Largest number of features",
        kind: ParamKind::Text,
        default: "",
        hint: "Reads only the first ones, for example 1000, to try a model out on large data. Empty reads all",
    },
    ParamSpec {
        key: "raster_as",
        label: "Cells as",
        kind: ParamKind::Choice(RASTER_MODES),
        default: "points",
        hint: "Points and squares get the values of the bands. The outline is one area with the size of the raster. An image is shown as a picture on the map under the canvas, and a click on it tells the values of the cell in every band; a large raster is read smaller. A grid keeps the values of the first band for Shade terrain and Quick terrain view",
    },
    ParamSpec {
        key: "tiles_detail",
        label: "Level of detail",
        kind: ParamKind::Choice(TILES_DETAIL),
        default: "coarsest",
        hint: "Which tiles of the tree are read. The finest of a large tileset can be very many: read within a rectangle, or down to an error",
    },
    ParamSpec {
        key: "tiles_error",
        label: "Largest error, in metres",
        kind: ParamKind::Text,
        default: "1",
        hint: "Tiles are read down to where the tileset says they are this close to the finest. A smaller number is more detail and more tiles",
    },
    ParamSpec { key: "tiles_most", label: "At most this many tiles", kind: ParamKind::Text, default: "500", hint: "More is an error that says so, rather than a long wait" },
    ParamSpec {
        key: "vt_zoom",
        label: "Zoom level of the vector tiles",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as 14. A higher level has more detail and more tiles. Empty: the most detailed in a file, 14 on the web",
    },
    ParamSpec {
        key: "vt_pieces",
        label: "Features the tiles cut apart",
        kind: ParamKind::Choice(VT_PIECES),
        default: "join",
        hint: "Tiles cut a lake or a road at their edges. Put together, each is one feature again: the pieces with the same mvt_id and values",
    },
    ParamSpec {
        key: "tiles_crs",
        label: "Coordinate system to place them in",
        kind: ParamKind::Crs,
        default: "",
        hint: "Such as EPSG:3006 for SWEREF 99 TM. Empty is the UTM zone where the tiles are",
    },
    ParamSpec {
        key: "tiles_dz",
        label: "Move the heights by",
        kind: ParamKind::Text,
        default: "0",
        hint: "3D Tiles have their heights above the ellipsoid. In Sweden the geoid lies some 20 to 40 metres above it, so for heights in RH 2000 write the geoid height of the place with a minus, such as −32",
    },
    ParamSpec {
        key: "las_classes",
        label: "Only these classes",
        kind: ParamKind::Text,
        default: "",
        hint: "Such as 2 for the ground, or 2, 6, 9 for ground, buildings and water. Empty reads all: 1 unclassified, 3 to 5 vegetation, 7 and 18 noise, 17 bridges",
    },
    ParamSpec {
        key: "las_every",
        label: "Every n-th point",
        kind: ParamKind::Text,
        default: "1",
        hint: "Thins a dense cloud: 10 reads one point in ten. 1 reads all",
    },
    ParamSpec {
        key: "las_spacing",
        label: "Spacing between points (COPC)",
        kind: ParamKind::Text,
        default: "",
        hint: "For a COPC file: only the levels of its octree down to about this many metres between points, such as 2 for an overview. Empty reads all. With a rectangle, only that part is read, also from the web",
    },
    ParamSpec { key: "obj_unit", label: "Units of the model", kind: ParamKind::Choice(OBJ_UNITS), default: "auto", hint: "glTF is in metres. OBJ files do not say: more than 200 across is taken as centimetres" },
    ParamSpec { key: "obj_up", label: "Upwards in the model", kind: ParamKind::Choice(OBJ_UP), default: "auto", hint: "A building lying on its side has the other" },
    ParamSpec {
        key: "dxf_blocks",
        label: "Blocks",
        kind: ParamKind::Choice(DXF_BLOCKS),
        default: "points",
        hint: "A block, such as a well or a tree, as a point with its name and the values of its attributes as columns, or drawn out in lines",
    },
    ParamSpec { key: "dxf_closed", label: "Closed polylines", kind: ParamKind::Choice(DXF_CLOSED), default: "areas", hint: "Such as the outlines of buildings" },
    ParamSpec {
        key: "city_lod",
        label: "Level of detail",
        kind: ParamKind::Choice(CITY_LODS),
        default: "finest",
        hint: "Of each city object in CityGML and CityJSON: LoD1 is blocks, LoD2 has roof shapes, LoD3 windows and more",
    },
    ParamSpec { key: "ifc_openings", label: "Openings", kind: ParamKind::Choice(IFC_OPENINGS), default: "cut", hint: "For windows and doors: cut out of the walls and slabs where they are boxes or other convex shapes" },
    ParamSpec { key: "ifc_spaces", label: "Spaces", kind: ParamKind::Choice(IFC_SPACES), default: "false", hint: "The rooms as volumes. They fill the building, so they are left out unless asked for" },
    ParamSpec { key: "ifc_properties", label: "Property sets", kind: ParamKind::Choice(IFC_PROPERTIES), default: "all", hint: "Each property as an attribute, such as Pset_WallCommon.IsExternal, also quantities and those of the type" },
    ParamSpec { key: "obj_east", label: "Origin of the model, east", kind: ParamKind::Text, default: "0", hint: "Where 0, 0, 0 of the model goes on the map, such as 319500 in SWEREF 99 TM" },
    ParamSpec { key: "obj_north", label: "Origin of the model, north", kind: ParamKind::Text, default: "0", hint: "Such as 6399800" },
    ParamSpec { key: "obj_height", label: "Origin of the model, height", kind: ParamKind::Text, default: "0", hint: "The height of the ground, such as 52.5" },
    ParamSpec { key: "obj_rotation", label: "Turned clockwise, in degrees", kind: ParamKind::Text, default: "0", hint: "Seen from above, around the origin" },
    ParamSpec {
        key: "osm_tags",
        label: "Tags as attributes",
        kind: ParamKind::Choice(OSM_TAGS),
        default: "common",
        hint: "Each tag of OpenStreetMap, such as amenity or opening_hours, can become an attribute of its own. A whole country has thousands of different tags, so by default only those on at least one feature in a hundred get a column, and the rest stay together in other_tags, as GDAL reads them",
    },
    ParamSpec {
        key: "json_records",
        label: "Records at (JSON)",
        kind: ParamKind::Text,
        default: "",
        hint: "Where the list of records is in the file, such as results or data.items. Empty finds it: the list itself, or a list such as results",
    },
    ParamSpec {
        key: "json_nested",
        label: "Objects in a record (JSON)",
        kind: ParamKind::Choice(JSON_NESTED),
        default: "flatten",
        hint: "An address inside a record becomes adress.gata and adress.ort, or stays as JSON text for Flatten JSON",
    },
    ParamSpec {
        key: "cell_size",
        label: "Cell size",
        kind: ParamKind::Text,
        default: "",
        hint: "Reads the grid at coarser cells, such as 10 for 10 metres, which saves memory. Empty reads it as it is, or coarser when it is very large",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "", hint: "Used when the file does not state its own" },
    ParamSpec { key: "encoding", label: "Encoding", kind: ParamKind::Choice(ENCODINGS), default: "", hint: "For CSV, and for Shapefiles without .cpg" },
    ParamSpec { key: "x", label: "X column (CSV)", kind: ParamKind::Text, default: "", hint: "Empty looks for x, lon, e, east" },
    ParamSpec { key: "y", label: "Y column (CSV)", kind: ParamKind::Text, default: "", hint: "Empty looks for y, lat, n, north" },
    ParamSpec { key: "wkt", label: "WKT column (CSV)", kind: ParamKind::Text, default: "", hint: "Empty looks for wkt, geom, geometry" },
    ParamSpec { key: "delimiter", label: "Delimiter (CSV)", kind: ParamKind::Text, default: "", hint: "Empty detects ; , tab or |" },
    ParamSpec {
        key: "filename_attr",
        label: "Add the file name as an attribute",
        kind: ParamKind::OptionalName("file_name"),
        default: "",
        hint: "Each feature gets an attribute that says which file it came from. The name of the attribute can be changed",
    },
    ParamSpec { key: "filename_part", label: "File name as", kind: ParamKind::Choice(FILENAME_PARTS), default: "stem", hint: "" },
    ParamSpec {
        key: "layer_attr",
        label: "Add the layer name as an attribute",
        kind: ParamKind::OptionalName("layer_name"),
        default: "",
        hint: "Each feature gets an attribute with the name of the layer or sheet it came from. The name of the attribute can be changed",
    },
];

pub(super) fn parse_delimiter(s: Option<String>) -> Result<Option<char>> {
    match s.as_deref() {
        None => Ok(None),
        Some("tab") | Some("\\t") | Some("tabb") => Ok(Some('\t')),
        Some(s) if s.chars().count() == 1 => Ok(s.chars().next()),
        Some(s) => err(format!("The delimiter must be a single character, not \"{}\".", s)),
    }
}

/// Does the name match the pattern? `*` stands for any run of characters and `?` for one.
/// Upper and lower case are not told apart, as in Windows file names.
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| go(&p[1..], &n[i..])),
            Some('?') => !n.is_empty() && go(&p[1..], &n[1..]),
            Some(c) => n.first() == Some(c) && go(&p[1..], &n[1..]),
        }
    }
    let (p, n): (Vec<char>, Vec<char>) = (pattern.to_lowercase().chars().collect(), name.to_lowercase().chars().collect());
    go(&p, &n)
}

pub(super) fn has_wildcard(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

/// A folder that is one dataset in itself, not a collection of files.
pub(super) fn is_dataset_folder(path: &Path) -> bool {
    ext_of(path) == "gdb" || super::vector_tiles::is_tile_folder(path)
}

/// What a reader's path stands for: one file, or the files of a folder that match a filter.
#[derive(Debug, Clone, PartialEq)]
pub struct Sources {
    pub files: Vec<PathBuf>,
    /// The folder and the filters, when the path is a folder or holds a wildcard.
    pub folder: Option<(PathBuf, Vec<String>)>,
    /// The web address, when the path is one. The file is then fetched when the reader runs.
    pub url: Option<String>,
}

/// Kinds of web services that Input reads through GDAL, page by page, rather than as a file.
pub const WEB_SERVICES: &[(&str, &str)] = &[
    ("auto", "As the address shows"),
    ("file", "A file: fetched, then read"),
    ("ogcapi", "OGC API Features"),
    ("wfs", "WFS"),
    ("arcgis", "ArcGIS feature service (FeatureServer, MapServer)"),
    ("3dtiles", "3D Tiles (a tileset.json)"),
    ("vectortiles", "Vector tiles ({z}/{x}/{y}.pbf)"),
    ("cog", "Cloud Optimized GeoTIFF: read where it is"),
    ("copc", "COPC point cloud: read where it is"),
];

/// What becomes of the pieces of a feature that vector tiles cut apart.
pub(super) const VT_PIECES: &[(&str, &str)] = &[("join", "Put together"), ("keep", "Kept as pieces, one per tile")];

/// Which tiles of a 3D Tiles tileset are read.
pub(super) const TILES_DETAIL: &[(&str, &str)] =
    &[("coarsest", "The coarsest: the first tiles from the top"), ("error", "Down to a largest error"), ("finest", "The finest: the tiles at the bottom")];

/// Whether a path or an address is a 3D Tiles tileset, or a single tile of one: by the
/// choice in the input, by its name (tileset.json, .b3dm, .cmpt), or for a .json on this
/// computer by what it begins with.
pub fn is_tileset(path: &str, web_as: Option<&str>) -> bool {
    if web_as == Some("3dtiles") {
        return true;
    }
    let plain = path.split(['?', '#']).next().unwrap_or(path).to_lowercase();
    if plain.ends_with("tileset.json") || plain.ends_with(".b3dm") || plain.ends_with(".cmpt") || plain.ends_with(".pnts") {
        return true;
    }
    if plain.ends_with(".json") && !is_url(path) {
        use std::io::Read;
        let mut head = vec![0u8; 8192];
        let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
        let text = String::from_utf8_lossy(&head[..n]);
        return text.contains("\"geometricError\"") && text.contains("\"asset\"");
    }
    false
}

/// The prefixes of the names a service is read by.
pub const SERVICE_PREFIXES: [&str; 3] = ["OAPIF:", "WFS:", "ARCGIS:"];

/// The service an address is, by the choice or by how the address looks, as the name GDAL
/// reads it by: OAPIF:…, WFS:… or ARCGIS:…. None for a file.
pub fn service_name(url: &str, choice: Option<&str>) -> Option<String> {
    let u = url.trim();
    let low = u.to_lowercase();
    let kind = match choice.unwrap_or("auto") {
        "file" => return None,
        "ogcapi" => "OAPIF:",
        "wfs" => "WFS:",
        "arcgis" => "ARCGIS:",
        _ => {
            let path = low.split('?').next().unwrap_or("");
            if path.contains("/featureserver") || path.contains("/mapserver") {
                "ARCGIS:"
            } else if (low.contains("service=wfs") || path.ends_with("/wfs") || path.contains("/wfs/")) && !low.contains("request=getfeature") {
                // An address that asks for the features themselves, such as GeoJSON from a
                // GeoServer, is a file; the service itself is read through its pages.
                "WFS:"
            } else if path.contains("/collections") || path.ends_with("/ogc/features") || path.contains("/ogcapi") {
                "OAPIF:"
            } else {
                return None;
            }
        }
    };
    Some(format!("{}{}", kind, u))
}

/// Is this the name of a web service rather than of a file?
pub fn is_service(path: &Path) -> bool {
    let s = path.to_string_lossy();
    SERVICE_PREFIXES.iter().any(|p| s.starts_with(p))
}

/// Is this a web address rather than a file on the computer?
pub fn is_url(path: &str) -> bool {
    let p = path.trim().to_lowercase();
    p.starts_with("http://") || p.starts_with("https://")
}

/// The file extension of a path or a web address, in lower case. What follows a ? or a #
/// in an address is not part of the name.
pub fn source_ext(path: &str) -> String {
    let name = if is_url(path) { path.split(['?', '#']).next().unwrap_or("") } else { path };
    ext_of(Path::new(name.trim()))
}

impl Sources {
    pub fn is_many(&self) -> bool {
        self.folder.is_some()
    }
}

pub(super) fn list_matching(dir: &Path, patterns: &[String], recursive: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| GbError(format!("Cannot read the folder {}: {}", dir.display(), e)))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = path.is_dir();
        if (!is_dir || is_dataset_folder(&path)) && patterns.iter().any(|p| wildcard_match(p, &name)) {
            out.push(path);
        } else if is_dir && recursive {
            list_matching(&path, patterns, recursive, out)?;
        }
    }
    Ok(())
}

/// The files a reader with these parameters reads, in the order it reads them.
pub fn reader_sources(p: &P, base_dir: &Path) -> Result<Sources> {
    let raw = p.need("path", "The input has no file. Choose a file or a folder.")?;
    if is_url(&raw) {
        return Ok(Sources { files: Vec::new(), folder: None, url: Some(raw) });
    }
    let given = Path::new(&raw);
    let path = if given.is_absolute() { given.to_path_buf() } else { base_dir.join(given) };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (dir, patterns) = if has_wildcard(&name) {
        (path.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")), vec![name])
    } else if path.is_dir() && !is_dataset_folder(&path) {
        let text = p.get("pattern").unwrap_or_else(|| DEFAULT_PATTERN.to_string());
        (path.clone(), text.split([';', ',']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
    } else if path.exists() {
        return Ok(Sources { files: vec![path], folder: None, url: None });
    } else {
        return err(format!("The file does not exist: {}", path.display()));
    };
    if !dir.is_dir() {
        return err(format!("The folder does not exist: {}", dir.display()));
    }
    let mut files = Vec::new();
    list_matching(&dir, &patterns, p.flag("recursive"), &mut files)?;
    // Sorted without regard to case, so that the order is the same on every system.
    files.sort_by_key(|f| f.to_string_lossy().to_lowercase());
    Ok(Sources { files, folder: Some((dir, patterns)), url: None })
}

pub(super) fn run_reader(p: &P, _inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let mut sources = reader_sources(p, ctx.base_dir)?;
    let mut web_headers: Vec<(String, String)> = Vec::new();
    if let Some(url) = sources.url.clone() {
        let b = ctx.backend("Reading from the web")?;
        // The key or login of a connection goes along, by the address or in headers.
        let mut url = url;
        if let Some(name) = p.get("connection").filter(|c| !c.trim().is_empty()) {
            let gate = ctx.gate;
            let stopped = move || !gate.going();
            let mut client = crate::web::Client::new(b, Some(crate::web::connection(&name)?), &stopped);
            let (u, h) = client.prepared(&url)?;
            url = u;
            web_headers = h;
        } else if url.contains("{{key}}") {
            return err("{{key}} is written in the address, but no API connection is chosen. Choose the connection with the key.");
        }
        if is_tileset(&url, p.get("web_as").as_deref()) {
            return run_tiles(p, ctx, &url, &web_headers);
        }
        if super::vector_tiles::is_template(&url, p.get("web_as").as_deref()) {
            return super::vector_tiles::run_web(p, ctx, &url, &web_headers);
        }
        let ext = source_ext(&url);
        let as_file = p.get("web_as").as_deref() == Some("file");
        if !as_file && (url.to_lowercase().split(['?', '#']).next().unwrap_or("").ends_with(".copc.laz") || p.get("web_as").as_deref() == Some("copc")) {
            // A COPC point cloud is read in parts, where it is, and not fetched whole.
            sources.files = vec![PathBuf::from(url.clone())];
        } else if !as_file && ext == "pmtiles" {
            // PMTiles on the web are read where they are, a tile at a time, as maps do.
            sources.files = vec![PathBuf::from(format!("/vsicurl/{}", url))];
        } else if !as_file && (matches!(ext.as_str(), "tif" | "tiff") || p.get("web_as").as_deref() == Some("cog")) {
            // A GeoTIFF on the web is read where it is through GDAL, which fetches only the
            // parts it needs of a Cloud Optimized GeoTIFF: the rectangle, from its overviews.
            sources.files = vec![PathBuf::from(format!("/vsicurl/{}", url))];
        }
        if !sources.files.is_empty() {
        } else {
            match service_name(&url, p.get("web_as").as_deref()) {
                // A service is read by GDAL, page by page.
                Some(name) => sources.files = vec![PathBuf::from(name)],
                // A file is fetched to this computer first, and then read like any other.
                None => sources.files = vec![b.fetch_with(&url, &web_headers, p.flag("keep_download"), &mut ctx.notes)?],
            }
        }
    }
    if sources.files.is_empty() {
        let (dir, patterns) = sources.folder.clone().unwrap_or_default();
        return err(format!("No files match {} in {}.", patterns.join("; "), dir.display()));
    }
    if let [one] = sources.files.as_slice() {
        if sources.url.is_none() && is_tileset(&one.to_string_lossy(), p.get("web_as").as_deref()) {
            let one = one.to_string_lossy().into_owned();
            return run_tiles(p, ctx, &one, &[]);
        }
    }
    let limit = match p.get("limit") {
        None => None,
        Some(text) => match text.replace(' ', "").parse::<usize>() {
            Ok(n) if n > 0 => Some(n),
            _ => return err(format!("The largest number of features must be a whole number above zero, not \"{}\". Leave it empty to read all.", text)),
        },
    };
    // Only within a rectangle: the four sides as numbers, or the variables of the model.
    let within = match p.get("within").as_deref() {
        Some("rectangle") | Some("variables") => {
            let side = |k: &str, what: &str| -> Result<f64> {
                let v = p.get(k).unwrap_or_default();
                crate::model::parse_number(v.trim()).ok_or_else(|| GbError(format!("{} of the rectangle must be a number, not \"{}\".", what, v)))
            };
            let r = [side("min_x", "The smallest x")?, side("min_y", "The smallest y")?, side("max_x", "The largest x")?, side("max_y", "The largest y")?];
            if r[0] >= r[2] || r[1] >= r[3] {
                return err("The rectangle is empty: the smallest x and y must be less than the largest.");
            }
            Some((r, p.crs("within_crs")?))
        }
        _ => None,
    };
    // How many more features the limit allows.
    let mut left = limit;
    let many = sources.files.len() > 1;
    let crs = p.crs("crs")?;
    let wanted = p.list("layer");
    let (name_attr, layer_attr) = (p.get("filename_attr"), p.get("layer_attr"));
    let part = p.get("filename_part").unwrap_or_default();
    let header = Some(p.get("header").as_deref() != Some("false"));

    let mut tables: Vec<Arc<Table>> = Vec::new();
    let mut layer_count = 0usize;
    for file in &sources.files {
        if left == Some(0) {
            break;
        }
        let short = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // With several files, a message says which file it is about.
        let named = |e: GbError| if many { GbError(format!("{}: {}", short, e.0)) } else { e };
        let ext = if is_service(file) { String::new() } else { ext_of(file) };
        let mut notes = Notes::default();
        let mut read: Vec<(Table, Option<String>)> = Vec::new();
        if is_service(file) {
            let b = ctx.backend("Reading a web service")?;
            let layers: Vec<Option<String>> = match wanted.as_slice() {
                [] => vec![None],
                [all] if all == "*" => b.layers(file).map_err(named)?.into_iter().map(|l| Some(l.name)).collect(),
                names => names.iter().cloned().map(Some).collect(),
            };
            for layer in layers {
                let taken: usize = read.iter().map(|r: &(Table, Option<String>)| r.0.len()).sum();
                let room = left.map(|l| l.saturating_sub(taken));
                if room == Some(0) {
                    break;
                }
                let opts = ReadOpts { layer: layer.clone(), crs, limit: room, within, headers: web_headers.clone(), ..Default::default() };
                let (progress, gate) = (ctx.progress, ctx.gate);
                let going = |done, total| {
                    progress("read", done, total);
                    gate.going()
                };
                let t = b.read_with(file, &opts, &mut notes, &going).map_err(|e| GbError(e.0.replace(&file.to_string_lossy().to_string(), "the service")))?;
                read.push((t, layer));
            }
        } else if matches!(ext.as_str(), "las" | "laz") {
            let classes = match p.get("las_classes").filter(|c| !c.trim().is_empty()) {
                None => None,
                Some(text) => Some(
                    text.split([',', ' ', ';'])
                        .filter(|c| !c.is_empty())
                        .map(|c| c.parse::<u8>().map_err(|_| GbError(format!("\"{}\" is not the number of a class. Write them as 2, 6.", c))))
                        .collect::<Result<Vec<u8>>>()?,
                ),
            };
            let every = match number_param(p, "las_every", "Every n-th point")? {
                None => 1,
                Some(n) if n >= 1.0 && n.fract() == 0.0 => n as usize,
                Some(_) => return err("Every n-th point must be a whole number of 1 or more."),
            };
            // A rectangle in the coordinates of the file is used while reading, and saves
            // memory; one in other coordinates is applied after.
            let rect = within.filter(|(_, c)| c.is_none() || *c == crs).map(|(r, _)| r);
            // Each point is a feature, so without a limit at most two million are read.
            let most = Some(left.unwrap_or(2_000_000));
            let spacing = number_param(p, "las_spacing", "The spacing")?.filter(|s| *s > 0.0);
            let opts = crate::pointcloud::LasOpts { classes, every, most, within: rect, crs, spacing };
            let (progress, gate) = (ctx.progress, ctx.gate);
            let going = |done, total| {
                progress("read points", done, total);
                gate.going()
            };
            let address = file.to_string_lossy();
            let t = if is_url(&address) {
                // COPC on the web: only the parts of the octree that are needed are fetched.
                crate::pointcloud::read_copc_url(ctx.backend("Reading from the web")?, &address, &web_headers, &opts, &mut notes)
            } else {
                crate::pointcloud::read_las(file, &opts, &mut notes, &going)
            };
            read.push((t.map_err(named)?, None));
        } else if (matches!(ext.as_str(), "json" | "jsonl") && crate::citymodel::is_cityjson(file))
            || (matches!(ext.as_str(), "gml" | "xml") && crate::citymodel::is_citygml(file))
        {
            // A city model: buildings and more in 3D, by level of detail.
            let lod = match p.get("city_lod").as_deref() {
                Some("coarsest") => crate::citymodel::Lod::Coarsest,
                Some(l) if l != "finest" => crate::citymodel::Lod::Level(l.to_string()),
                _ => crate::citymodel::Lod::Finest,
            };
            let (progress, gate) = (ctx.progress, ctx.gate);
            let going = |done, total| {
                progress("read", done, total);
                gate.going()
            };
            let t = if matches!(ext.as_str(), "json" | "jsonl") {
                crate::citymodel::read_cityjson(file, &lod, crs, &mut notes)
            } else {
                crate::citymodel::read_citygml(file, &lod, crs, &mut notes, &going)
            };
            read.push((t.map_err(named)?, None));
        } else if matches!(ext.as_str(), "parquet" | "geoparquet") {
            // GeoParquet is read here, not by GDAL, which is often built without it.
            let (progress, gate) = (ctx.progress, ctx.gate);
            let going = |done, total| {
                progress("read", done, total);
                gate.going()
            };
            let t = crate::parquet_io::read_parquet(file, crs, left, &mut notes, &going).map_err(named)?;
            read.push((t, None));
        } else if ext == "ifc" {
            // A building model: placed where the file says, or where the input puts it.
            let num = |k: &str, what: &str| number_param(p, k, what).map(|v| v.unwrap_or(0.0));
            let origin = [num("obj_east", "East")?, num("obj_north", "North")?, num("obj_height", "The height")?];
            let rotation = num("obj_rotation", "The rotation")?;
            let opts = crate::ifc_io::IfcOpts {
                spaces: p.flag("ifc_spaces"),
                cut_openings: p.get("ifc_openings").as_deref() != Some("leave"),
                properties: p.get("ifc_properties").as_deref() != Some("none"),
            };
            let (progress, gate) = (ctx.progress, ctx.gate);
            let going = |done, total| {
                progress("read the elements", done, total);
                gate.going()
            };
            // What the reading is busy with goes to the log as it happens.
            let say = ctx.say;
            let file_name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let tell = |msg: &str| say(crate::run::Level::Info, &format!("{}: {}", file_name, msg));
            let mut model = crate::ifc_io::read_ifc(file, &opts, &mut notes, &going, &tell).map_err(named)?;
            let g = model.georef.clone();
            let own = origin != [0.0; 3] || rotation != 0.0;
            if own {
                crate::ifc_io::place(&mut model, Some((origin, rotation)));
                model.table.crs = crs;
            } else if let Some((at, _, _)) = g.map {
                crate::ifc_io::place(&mut model, None);
                model.table.crs = g.crs.or(crs);
                notes.info(format!("placed by its map conversion at {:.1}, {:.1}", at[0], at[1]));
                if model.table.crs.is_none() {
                    notes.warn("The file places the model on a map, but does not say in which coordinate system. Set it in the input.".to_string());
                }
            } else if let (Some((lat, lon)), Some(c)) = (g.lat_lon, crs) {
                let b = ctx.backend("Placing the model by the latitude and longitude of its site")?;
                let (mut xs, mut ys) = (vec![lon], vec![lat]);
                b.transform(4326, c, &mut xs, &mut ys)?;
                crate::ifc_io::place(&mut model, Some(([xs[0], ys[0], 0.0], g.true_north.to_degrees())));
                model.table.crs = Some(c);
                notes.info(format!("placed by the latitude and longitude of its site, {:.6}, {:.6}, which are seldom exact", lat, lon));
            } else {
                model.table.crs = crs;
                notes.info(if g.lat_lon.is_some() {
                    "in its own coordinates. The site has a latitude and longitude: choose a coordinate system in the input to place it by them, or give the origin".to_string()
                } else {
                    "in its own coordinates: give the origin in the input to place it on a map".to_string()
                });
            }
            read.push((model.table, None));
        } else if matches!(ext.as_str(), "obj" | "gltf" | "glb") {
            // A 3D model: where it stands, and in which units, is set in the input.
            let num = |k: &str, what: &str| number_param(p, k, what).map(|v| v.unwrap_or(0.0));
            let unit = match p.get("obj_unit").as_deref() {
                Some("m") => Some(1.0),
                Some("cm") => Some(0.01),
                Some("mm") => Some(0.001),
                Some("ft") => Some(0.3048),
                Some("in") => Some(0.0254),
                _ => None,
            };
            let gltf = ext != "obj";
            let opts = crate::obj_io::ObjOpts {
                unit,
                // glTF has y up by its rules; OBJ files most often z, from GIS and CAD.
                y_up: match p.get("obj_up").as_deref() {
                    Some("y") => true,
                    Some("z") => false,
                    _ => gltf,
                },
                origin: [num("obj_east", "East")?, num("obj_north", "North")?, num("obj_height", "The height")?],
                rotation: num("obj_rotation", "The rotation")?,
                crs,
            };
            let (progress, gate) = (ctx.progress, ctx.gate);
            let going = |done, total| {
                progress("read the file", done, total);
                gate.going()
            };
            let t = if gltf { crate::gltf_io::read_gltf(file, &opts, &mut notes) } else { crate::obj_io::read_obj(file, &opts, &mut notes, &going) };
            read.push((t.map_err(named)?, None));
        } else if is_plain_json(file) {
            let (lists, text) = match p.get("json_nested").as_deref() {
                Some("columns") => (crate::json::Lists::Columns, false),
                Some("text") => (crate::json::Lists::Text, true),
                _ => (crate::json::Lists::Text, false),
            };
            let t = crate::json::read_table(file, p.get("json_records").as_deref(), lists, text, &mut notes).map_err(named)?;
            read.push((t, None));
        } else if is_text_format(&ext) {
            let opts = CsvReadOpts {
                delimiter: parse_delimiter(p.get("delimiter"))?,
                encoding: p.get("encoding"),
                x: p.get("x"),
                y: p.get("y"),
                wkt: p.get("wkt"),
                crs,
            };
            let (progress, gate) = (ctx.progress, ctx.gate);
            let going = |done, total| {
                progress("read", done, total);
                gate.going()
            };
            read.push((csv_io::read_csv_with(file, &opts, &mut notes, &going).map_err(named)?, None));
        } else {
            let b = ctx.backend(&format!("The format .{}", ext))?;
            // Which layers to read: the first one, the named ones, or all.
            let layers: Vec<Option<String>> = match wanted.as_slice() {
                [] => vec![None],
                [all] if all == "*" => b.layers(file).map_err(named)?.into_iter().map(|l| Some(l.name)).collect(),
                names => names.iter().cloned().map(Some).collect(),
            };
            for layer in layers {
                let taken: usize = read.iter().map(|r: &(Table, Option<String>)| r.0.len()).sum();
                let room = left.map(|l| l.saturating_sub(taken));
                if room == Some(0) {
                    break;
                }
                let raster = match p.get("raster_as").as_deref() {
                    Some("cells") => RasterMode::Cells,
                    Some("extent") => RasterMode::Extent,
                    Some("image") => RasterMode::Image,
                    Some("grid") => RasterMode::Grid,
                    _ => RasterMode::Points,
                };
                let cell_size = number_param(p, "cell_size", "The cell size")?.filter(|c| *c > 0.0);
                let opts = ReadOpts {
                    layer: layer.clone(),
                    encoding: p.get("encoding"),
                    crs,
                    allow_no_crs: false,
                    header,
                    limit: room,
                    raster,
                    cell_size,
                    sql: None,
                    within,
                    headers: Vec::new(),
                    dxf_blocks_as_points: p.get("dxf_blocks").as_deref() != Some("drawn"),
                    dxf_areas: p.get("dxf_closed").as_deref() != Some("lines"),
                    open_options: if super::vector_tiles::is_vector_tiles(file) { super::vector_tiles::zoom_options(p)? } else { Vec::new() },
                    zip_member: p.get("zip_file").filter(|z| !z.trim().is_empty()).map(|z| z.trim().to_string()),
                };
                let (progress, gate) = (ctx.progress, ctx.gate);
                let going = |done, total| {
                    progress("read", done, total);
                    gate.going()
                };
                let t = b.read_with(file, &opts, &mut notes, &going).map_err(named)?;
                // The name of the first layer is only looked up when it is asked for.
                let name = match (&layer, &layer_attr) {
                    (Some(l), _) => Some(l.clone()),
                    (None, Some(_)) => b.layers(file).ok().and_then(|l| l.into_iter().next()).map(|l| l.name),
                    (None, None) => None,
                };
                read.push((t, name));
            }
        }
        layer_count += read.len();
        for (mut t, layer) in read {
            // Vector tiles cut features at their edges; the pieces are put together again.
            if super::vector_tiles::is_vector_tiles(file) && p.get("vt_pieces").as_deref() != Some("keep") {
                let pieces = super::vector_tiles::join_pieces(&mut t);
                if pieces > t.len() {
                    notes.info(format!("{} pieces of the tiles put together into {} features", fmt_count(pieces as u64), fmt_count(t.len() as u64)));
                }
            }
            // What lies outside the rectangle goes, also from formats GDAL does not filter,
            // such as CSV.
            if let (Some((rect, rect_crs)), Some(data_crs)) = (within, t.crs) {
                let r = match rect_crs.filter(|c| *c != data_crs) {
                    Some(from) => {
                        let b = ctx.backend("Reprojecting the rectangle")?;
                        let (mut xs, mut ys) = (vec![rect[0], rect[2], rect[2], rect[0]], vec![rect[1], rect[1], rect[3], rect[3]]);
                        b.transform(from, data_crs, &mut xs, &mut ys)?;
                        [
                            xs.iter().cloned().fold(f64::INFINITY, f64::min),
                            ys.iter().cloned().fold(f64::INFINITY, f64::min),
                            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                        ]
                    }
                    None => rect,
                };
                let before = t.len();
                t.features.retain(|f| {
                    use geo::BoundingRect;
                    f.geom
                        .as_ref()
                        .and_then(|g| g.bounding_rect())
                        .is_some_and(|b| b.min().x <= r[2] && b.max().x >= r[0] && b.min().y <= r[3] && b.max().y >= r[1])
                });
                if t.len() < before {
                    notes.info(format!("{} outside the rectangle were left out", fmt_count((before - t.len()) as u64)));
                }
            }
            // The tags of OpenStreetMap that GDAL puts together in other_tags become
            // attributes of their own.
            let tags = p.get("osm_tags").unwrap_or_else(|| "common".into());
            if matches!(ext_of(file).as_str(), "pbf" | "osm") && tags != "false" {
                let (with, note) = osm_tags(t, tags == "all");
                t = with;
                if let Some(n) = note {
                    notes.info(n);
                }
            }
            if let Some(l) = left {
                t.features.truncate(l);
                left = Some(l - t.len());
            }
            if let Some(attr) = &name_attr {
                let value = match part.as_str() {
                    "name" => short.clone(),
                    "path" => file.to_string_lossy().into_owned(),
                    _ => file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                };
                let values = vec![Value::Text(value); t.len()];
                t = with_column(&t, attr, values);
            }
            if let Some(attr) = &layer_attr {
                let values = vec![layer.clone().map_or(Value::Null, Value::Text); t.len()];
                t = with_column(&t, attr, values);
            }
            tables.push(Arc::new(t));
        }
        if many {
            notes.warnings.iter().for_each(|w| ctx.notes.warn(format!("{}: {}", short, w)));
        } else {
            notes.info.iter().for_each(|i| ctx.notes.info(i.clone()));
            notes.warnings.iter().for_each(|w| ctx.notes.warn(w.clone()));
        }
    }
    if many {
        ctx.notes.info(format!("{} files", fmt_count(sources.files.len() as u64)));
    }
    if layer_count > sources.files.len() {
        ctx.notes.info(format!("{} layers", fmt_count(layer_count as u64)));
    }
    if let (Some(n), Some(0)) = (limit, left) {
        ctx.notes.info(format!("stopped at the limit of {} features", fmt_count(n as u64)));
    }
    let table = merge_tables(tables, ctx.backend, "files", &mut ctx.notes)?;
    let table = Arc::try_unwrap(table).unwrap_or_else(|t| (*t).clone());
    ctx.sources = sources.files;
    let missing = table.features.iter().filter(|f| f.geom.is_none()).count();
    if table.crs.is_some() && missing > 0 && missing < table.len() {
        ctx.notes.warn(format!("{} features have no geometry.", fmt_count(missing as u64)));
    }
    if table.crs.is_some() {
        ctx.notes.info(crs_label(table.crs));
    }
    Ok(vec![("Output".into(), table)])
}

/// Reads a 3D Tiles tileset, from this computer or the web, as one model.
fn run_tiles(p: &P, ctx: &mut NodeCtx, root: &str, headers: &[(String, String)]) -> Result<Outputs> {
    let detail = match p.get("tiles_detail").as_deref() {
        Some("finest") => crate::tiles3d::Detail::Finest,
        Some("error") => match number_param(p, "tiles_error", "The largest error")? {
            Some(e) if e >= 0.0 => crate::tiles3d::Detail::Error(e),
            _ => return err("Write the largest error in metres, such as 1, or choose another level of detail."),
        },
        _ => crate::tiles3d::Detail::Coarsest,
    };
    let most = match p.get("tiles_most") {
        None => 500,
        Some(t) => match t.replace(' ', "").parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => return err(format!("The largest number of tiles must be a whole number above zero, not \"{}\".", t)),
        },
    };
    let crs = p.crs("tiles_crs")?;
    // The rectangle in degrees, from the coordinate system it is written in: its own, or
    // that the tiles are placed in.
    let within = match p.get("within").as_deref() {
        Some("rectangle") => {
            let side = |k: &str, what: &str| -> Result<f64> {
                let v = p.get(k).unwrap_or_default();
                crate::model::parse_number(v.trim()).ok_or_else(|| GbError(format!("{} of the rectangle must be a number, not \"{}\".", what, v)))
            };
            let r = [side("min_x", "The smallest x")?, side("min_y", "The smallest y")?, side("max_x", "The largest x")?, side("max_y", "The largest y")?];
            let from = p.crs("within_crs")?.or(crs).unwrap_or(4326);
            if from == 4326 {
                Some(r)
            } else {
                let b = ctx.backend("Reprojecting the rectangle")?;
                let (mut xs, mut ys) = (vec![r[0], r[2], r[2], r[0]], vec![r[1], r[1], r[3], r[3]]);
                b.transform(from, 4326, &mut xs, &mut ys)?;
                let fold = |v: &[f64], f: fn(f64, f64) -> f64, s: f64| v.iter().copied().fold(s, f);
                Some([
                    fold(&xs, f64::min, f64::INFINITY),
                    fold(&ys, f64::min, f64::INFINITY),
                    fold(&xs, f64::max, f64::NEG_INFINITY),
                    fold(&ys, f64::max, f64::NEG_INFINITY),
                ])
            }
        }
        _ => None,
    };
    let dz = number_param(p, "tiles_dz", "The move of the heights")?.unwrap_or(0.0);
    let opts = crate::tiles3d::TilesOpts { detail, most, crs, within, dz, headers };
    let (progress, gate) = (ctx.progress, ctx.gate);
    let going = |done, total| {
        progress("read tiles", done, total);
        gate.going()
    };
    let t = crate::tiles3d::read_tiles(root, &opts, ctx.backend, &mut ctx.notes, &going)?;
    if t.crs.is_some() {
        ctx.notes.info(crs_label(t.crs));
    }
    Ok(vec![("Output".into(), t)])
}

/// A reader shows its file, and for a folder the filter as well. The layers come after.
pub(super) fn reader_summary(p: &P) -> String {
    let Some(path) = p.get("path") else { return "no file".into() };
    let mut text = if is_url(&path) {
        // The server and the last part of the address say enough.
        let address = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
        let mut parts = address.splitn(4, '/');
        let host = parts.nth(2).unwrap_or("");
        match parts.next().and_then(|rest| rest.rsplit('/').next()).filter(|n| !n.is_empty()) {
            Some(last) => format!("{} · {}", host, last),
            None => host.to_string(),
        }
    } else {
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
        // A path without an extension is taken to be a folder. The summary cannot look on disk.
        if !has_wildcard(&name) && Path::new(&name).extension().is_none() {
            format!("{}/{}", name, p.get("pattern").unwrap_or_else(|| DEFAULT_PATTERN.into()))
        } else {
            name
        }
    };
    match p.list("layer").as_slice() {
        [] => {}
        [all] if all == "*" => text.push_str(" · all layers"),
        [one] => text.push_str(&format!(" · {}", one)),
        many => text.push_str(&format!(" · {} layers", many.len())),
    }
    if let Some(limit) = p.get("limit") {
        text.push_str(&format!(" · first {}", limit));
    }
    text
}

// ---------- Formats, and the tags of OpenStreetMap ----------

/// What an input can read, in words, for the window and the documentation.
pub(super) const JSON_NESTED: &[(&str, &str)] =
    &[("flatten", "Attributes of their own, lists as JSON text"), ("columns", "Attributes of their own, lists too"), ("text", "JSON text")];

/// Whether a file is JSON to read as a table, and not GeoJSON for GDAL.
pub fn is_plain_json(file: &Path) -> bool {
    match ext_of(file).as_str() {
        "ndjson" | "jsonl" => true,
        "json" => !crate::json::looks_like_geojson(file),
        // A file without an extension, such as one fetched from an address that ends in /json,
        // is JSON when it starts as JSON does.
        "" => crate::json::starts_as_json(file) && !crate::json::looks_like_geojson(file),
        _ => false,
    }
}

pub const READ_FORMATS: &[&str] = &[
    "3D models (.obj, .gltf, .glb)",
    "3D Tiles (tileset.json)",
    "IFC building models (.ifc)",
    "CAD drawings (.dxf, .dwg)",
    "CityGML and CityJSON (.gml, .city.json)",
    "CSV and text (.csv, .txt)",
    "Excel (.xlsx)",
    "FlatGeobuf (.fgb)",
    "GeoJSON (.geojson)",
    "GeoPackage (.gpkg)",
    "GeoParquet (.parquet)",
    "GeoTIFF, also Cloud Optimized (.tif)",
    "JSON (.json)",
    "LibreOffice Calc (.ods)",
    "OpenStreetMap (.osm.pbf, .osm)",
    "Point clouds (.las, .laz, .copc.laz)",
    "PostGIS tables",
    "Shapefile (.shp)",
    "Vector tiles (.mbtiles, .pmtiles, {z}/{x}/{y}.pbf)",
    "SpatiaLite (.sqlite, .db)",
    "zip files of these",
    "files on the web, and folders",
];
/// How many tags of OpenStreetMap become attributes of their own.
pub(super) const OSM_TAGS: &[(&str, &str)] =
    &[("common", "The common ones; the rest stay in other_tags"), ("all", "All of them"), ("false", "None; all stay in other_tags")];

/// A table smaller than this gets a column for every tag when only the common ones are asked for.
pub(super) const OSM_SMALL: usize = 5_000;
/// The most columns the common tags get, and the share of the features a tag must be on.
pub(super) const OSM_MOST_COMMON: usize = 100;
pub(super) const OSM_SHARE: f64 = 0.01;
/// Cells (features × tags) beyond which "all" gives only the common ones, since a country
/// has thousands of different tags and every feature would hold a value for each.
pub(super) const OSM_MOST_CELLS: usize = 200_000_000;

/// Reads the tags in other_tags as GDAL writes them: "amenity"=>"cafe","name:en"=>"Café".
pub(super) fn parse_hstore(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    let quoted = |chars: &mut std::iter::Peekable<std::str::Chars>| -> Option<String> {
        while chars.peek().is_some_and(|c| *c != '"') {
            chars.next();
        }
        chars.next()?;
        let mut v = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => v.extend(chars.next()),
                '"' => return Some(v),
                c => v.push(c),
            }
        }
        None
    };
    while let Some(k) = quoted(&mut chars) {
        let Some(v) = quoted(&mut chars) else { break };
        out.push((k, v));
    }
    out
}

pub(super) fn hstore(tags: &[(String, String)]) -> String {
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    tags.iter().map(|(k, v)| format!("{}=>{}", q(k), q(v))).collect::<Vec<_>>().join(",")
}

/// The tags in other_tags, as GDAL reads OpenStreetMap, as attributes of their own, in the
/// order they are first seen. With `all` every tag gets a column; otherwise only those on at
/// least one feature in a hundred (all of them in a small table), and the rest stay in
/// other_tags. Returns what is worth telling about it.
pub fn osm_tags(mut t: Table, all: bool) -> (Table, Option<String>) {
    let Some(at) = t.field_index("other_tags") else { return (t, None) };
    // How often each tag is used, and where it was first seen; parsed again below, so that
    // the tags of millions of features are never held twice.
    let mut seen: HashMap<String, (usize, usize)> = HashMap::new();
    for f in &t.features {
        if let Some(v) = f.attrs.get(at).filter(|v| !v.is_null()) {
            for (k, _) in parse_hstore(&v.to_text()) {
                let order = seen.len();
                seen.entry(k).or_insert((order, 0)).1 += 1;
            }
        }
    }
    seen.retain(|k, _| t.field_index(k).is_none() || k == "other_tags");
    seen.remove("other_tags");
    let n = t.len();
    let mut note = None;
    let mut keys: Vec<(String, usize, usize)> = seen.into_iter().map(|(k, (o, c))| (k, o, c)).collect();
    let too_many = keys.len().saturating_mul(n) > OSM_MOST_CELLS;
    if all && too_many {
        note = Some(format!(
            "{} different tags on {} features are too many to have a column each: the common ones got columns, and the rest are in other_tags",
            fmt_count(keys.len() as u64),
            fmt_count(n as u64)
        ));
    }
    if !all || too_many {
        let total = keys.len();
        if n >= OSM_SMALL {
            keys.retain(|(_, _, c)| *c as f64 >= OSM_SHARE * n as f64);
        }
        keys.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
        keys.truncate(OSM_MOST_COMMON);
        if keys.len() < total && note.is_none() {
            note =
                Some(format!("{} of the {} different tags are common enough to get a column; the rest are in other_tags", keys.len(), fmt_count(total as u64)));
        }
    }
    keys.sort_by_key(|k| k.1);
    let column: HashMap<&str, usize> = keys.iter().enumerate().map(|(i, k)| (k.0.as_str(), i)).collect();
    let old = t.fields.len();
    let mut any_left = false;
    for f in t.features.iter_mut() {
        let tags = match f.attrs.get(at) {
            Some(v) if !v.is_null() => parse_hstore(&v.to_text()),
            _ => Vec::new(),
        };
        f.attrs.resize(old + keys.len(), Value::Null);
        let mut left = Vec::new();
        for (k, v) in tags {
            match column.get(k.as_str()) {
                Some(i) => f.attrs[old + i] = Value::Text(v),
                None => match t.fields.iter().position(|x| x.name == k) {
                    // A tag that already has a column of its own, such as name, fills it if empty.
                    Some(i) => {
                        if f.attrs[i].is_null() {
                            f.attrs[i] = Value::Text(v);
                        }
                    }
                    None => left.push((k, v)),
                },
            }
        }
        f.attrs[at] = if left.is_empty() { Value::Null } else { Value::Text(hstore(&left)) };
        any_left |= !left.is_empty();
        f.attrs.shrink_to_fit();
    }
    t.fields.extend(keys.iter().map(|k| Field::new(k.0.clone(), FieldType::Text)));
    if !any_left {
        for f in t.features.iter_mut() {
            f.attrs.remove(at);
        }
        t.fields.remove(at);
    } else {
        // other_tags goes last, after the tags that got columns.
        for f in t.features.iter_mut() {
            let v = f.attrs.remove(at);
            f.attrs.push(v);
        }
        let field = t.fields.remove(at);
        t.fields.push(field);
    }
    (t, note)
}

// ---------- Read files named in an attribute (AttributeFileReader) ----------

pub(super) const FILE_READER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "attribute",
        label: "Path in",
        kind: ParamKind::Attribute("Input"),
        default: "_file",
        hint: "The attribute with the path of each file, such as _file from HTTP request",
    },
    ParamSpec {
        key: "raster_as",
        label: "Cells as",
        kind: ParamKind::Choice(RASTER_MODES),
        default: "grid",
        hint: "For rasters, such as GeoTIFF. Grids of several files are put together into one",
    },
    ParamSpec { key: "crs", label: "Coordinate system", kind: ParamKind::Crs, default: "", hint: "For files that do not say. Empty: as the files say" },
];

pub(super) fn run_file_reader(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let attr = p.get("attribute").unwrap_or_else(|| "_file".into());
    let at = table.need_field(&attr)?;
    let raster = match p.get("raster_as").as_deref() {
        Some("cells") => RasterMode::Cells,
        Some("extent") => RasterMode::Extent,
        Some("image") => RasterMode::Image,
        Some("points") => RasterMode::Points,
        _ => RasterMode::Grid,
    };
    let crs = p.crs("crs")?;
    let mut parts: Vec<Table> = Vec::new();
    let mut origins: Vec<usize> = Vec::new();
    let total = table.len() as u64;
    for (i, f) in table.features.iter().enumerate() {
        (ctx.progress)("read files for", i as u64, Some(total));
        if ctx.stopped() {
            return err(crate::model::STOPPED);
        }
        let path = f.attrs[at].to_text();
        if path.trim().is_empty() {
            continue;
        }
        let file = ctx.resolve(&path);
        let ext = ext_of(&file);
        let mut notes = Notes::default();
        let t = if matches!(ext.as_str(), "las" | "laz") {
            crate::pointcloud::read_las(
                &file,
                &crate::pointcloud::LasOpts { every: 1, most: Some(2_000_000), crs, ..Default::default() },
                &mut notes,
                &|_, _| true,
            )
        } else if is_text_format(&ext) {
            csv_io::read_csv_with(&file, &CsvReadOpts { delimiter: None, encoding: None, x: None, y: None, wkt: None, crs }, &mut notes, &|_, _| true)
        } else {
            let opts = ReadOpts { crs, raster, ..Default::default() };
            ctx.backend(&format!("The format .{}", ext))?.read_with(&file, &opts, &mut notes, &|_, _| true)
        }
        .map_err(|e| GbError(format!("{}: {}", file.display(), e.0)))?;
        for w in notes.warnings {
            ctx.notes.warn(w);
        }
        parts.push(t);
        origins.push(i);
    }
    // Grids: put together into one, as tiles of a raster.
    let grids: Vec<&crate::model::Grid> = parts.iter().filter_map(|t| t.grid.as_deref()).collect();
    if !grids.is_empty() && grids.len() == parts.len() {
        if grids.len() == 1 {
            let mut t = parts.remove(0);
            // The one feature of a grid gets the attributes of the row too.
            for (k, field) in table.fields.iter().enumerate() {
                if t.field_index(&field.name).is_none() {
                    t.fields.push(field.clone());
                    for g in t.features.iter_mut() {
                        g.attrs.push(table.features[origins[0]].attrs[k].clone());
                    }
                }
            }
            return Ok(vec![("Output".into(), t)]);
        }
        let g = crate::model::Grid::mosaic(&grids, super::GRID_CELLS).ok_or_else(|| GbError("There is nothing to put together.".into()))?;
        ctx.notes.info(format!("{} grids put together into one", grids.len()));
        return Ok(vec![("Output".into(), g.into_table())]);
    }
    // Features: those of each file with the attributes of its row, the row's first.
    let mut out = Table { fields: table.fields.clone(), crs: parts.iter().find_map(|t| t.crs), ..Table::default() };
    for t in &parts {
        for f in &t.fields {
            if out.field_index(&f.name).is_none() {
                out.fields.push(f.clone());
            }
        }
    }
    for (t, row) in parts.iter().zip(&origins) {
        let moved;
        let t = match (out.crs, t.crs) {
            (Some(a), Some(b)) if a != b && t.has_geometry() => {
                moved = reproject(t, a, ctx.backend)?;
                &moved
            }
            _ => t,
        };
        let map: Vec<Option<usize>> = out.fields.iter().map(|f| t.field_index(&f.name)).collect();
        for f in &t.features {
            let attrs: Vec<Value> = map
                .iter()
                .enumerate()
                .map(|(k, m)| if k < table.fields.len() { table.features[*row].attrs[k].clone() } else { m.map_or(Value::Null, |m| f.attrs[m].clone()) })
                .collect();
            out.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
        }
    }
    ctx.notes.info(format!("{} files", fmt_count(parts.len() as u64)));
    Ok(vec![("Output".into(), out)])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "AttributeFileReader",
        title: "Read files named in an attribute",
        hidden: false,
        group: Group::Transformer,
        category: "Input and output",
        desc: "Reads the file whose path is in an attribute of each feature, such as the answers HTTP request keeps as files: GeoJSON, GeoTIFF and anything Input reads. The features of each file get the attributes of its row; grids are put together into one.",
        inputs: &["Input"],
        outputs: &["Output"],
        ports: None,
        params: FILE_READER_PARAMS,
        run: run_file_reader,
        summary: |p| p.get("attribute").unwrap_or_else(|| "_file".into()),
    },
    NodeType {
        name: "Reader",
        title: "Input",
        hidden: false,
        group: Group::Reader,
        category: "",
        desc: "Reads a file, a folder of files or a web address: geodata, tables, rasters, CAD drawings, city and building models, 3D models and point clouds. The formats are listed when nothing is selected.",
        inputs: &[],
        outputs: &["Output"],
        ports: None,
        params: READER_PARAMS,
        run: run_reader,
        summary: reader_summary,
    },
];
