//! The node types: readers, tools and writers. Each type describes its ports and
//! parameters, so that a user interface can be built from the description, and has a
//! function that runs the node.

use crate::csv_io::{self, typed_column, CsvReadOpts, CsvWriteOpts, GeomMode};
use crate::expr::{self, Row};
use crate::geom::{self, AreaIndex, Center, Clipper, Family};
use crate::io::{Backend, FieldSize, Notes, RasterMode, ReadOpts, SheetStyle, WriteOpts};
use crate::model::{
    coerce, crs_label, err, fmt_count, infer_type, parse_crs, parse_number, Feature, Field, FieldType, GbError, Result, Table, Value, KNOWN_CRS,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type Params = serde_json::Map<String, serde_json::Value>;
pub type Inputs = HashMap<&'static str, Arc<Table>>;
/// What a node gives, by output port. Most ports have fixed names; some tools name them
/// after their settings.
pub type Outputs = Vec<(std::borrow::Cow<'static, str>, Table)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Reader,
    Transformer,
    Writer,
}

#[derive(Debug, Clone, Copy)]
pub enum ParamKind {
    /// The streams that come in on a port that keeps them apart, each with how it is drawn:
    /// colours, sizes and order.
    MapLayers(&'static str),
    /// The name of an attribute to add, or nothing. In the program a tick box, which fills in
    /// the name given here when it is ticked.
    OptionalName(&'static str),
    /// Every attribute of a port with what to do with it: keep, rename or remove, in an order.
    AttributeTable(&'static str),
    /// New attributes, each with a name and an expression for its value, on a port.
    NewAttributes(&'static str),
    /// A list of values of an attribute, one per line, with a way to take them from the data:
    /// (the port, the key of the parameter that names the attribute).
    Values(&'static str, &'static str),
    Text,
    /// Text that is not shown as it is written, such as a password.
    Secret,
    /// The name of a database connection from the settings of the user.
    Connection,
    /// Rows of a path into JSON and the attribute it goes to: (the port, the key of the
    /// parameter that names the attribute with the JSON).
    JsonPaths(&'static str, &'static str),
    /// A choice from a list of (value, label).
    Choice(&'static [(&'static str, &'static str)]),
    /// Some of a list of (value, label), ticked: kept as a list of the values.
    Picks(&'static [(&'static str, &'static str)]),
    /// The name of an API connection from the settings of the user.
    ApiConnection,
    Crs,
    /// One or more attributes from the stream on the named input port.
    Attributes(&'static str),
    /// One attribute from the stream on the named input port.
    Attribute(&'static str),
    /// An expression evaluated against the stream on the named input port.
    Expression(&'static str),
    /// Code, such as SQL, over several lines, with {attributes} of the stream on the named
    /// input port put in.
    Code(&'static str),
    FileOpen,
    /// A file, a folder, or a file name pattern with * in it.
    Source,
    FileSave,
    /// One or more layers of the file in the parameter `path`: tables in a GeoPackage,
    /// sheets in an Excel workbook.
    Layers,
    /// Text where spaces count, such as a separator or a text to search for.
    Exact,
    /// Pairs of old and new name.
    Renames(&'static str),
    /// Attributes of the stream on the named port, each with an order: up or down.
    SortKeys(&'static str),
    /// Checks of attribute values, a row for each attribute, on the named port.
    Checks(&'static str),
    /// Ranges of numbers, each with a value, for an attribute on the named port.
    Ranges(&'static str),
    /// Pairs of a value and what it becomes, with a way to take the values from the data:
    /// (the port, the key of the parameter that names the attribute).
    Mapping(&'static str, &'static str),
    /// The data type and size of chosen attributes from the stream on the named input port.
    Schema(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
    pub default: &'static str,
    pub hint: &'static str,
}

#[derive(Clone, Copy)]
pub struct NodeType {
    /// The name of the type in workspace files. It is never changed, so that old files
    /// can still be read.
    pub name: &'static str,
    /// The name that is shown: what the tool does, as a verb.
    pub title: &'static str,
    /// A type that still runs in old workspaces but is no longer offered in lists.
    pub hidden: bool,
    pub group: Group,
    /// Subheading for tools in a list: "Attributes" or "Geometry". Empty for readers and writers.
    pub category: &'static str,
    pub desc: &'static str,
    pub inputs: &'static [&'static str],
    pub outputs: &'static [&'static str],
    /// For a tool whose outputs follow its settings: the names of the output ports. The
    /// fixed `outputs` are then only an example.
    pub ports: Option<fn(&P) -> Vec<String>>,
    pub params: &'static [ParamSpec],
    pub run: fn(&P, &Inputs, &mut NodeCtx) -> Result<Outputs>,
    /// One short line about how the node is set up, to show under its name.
    pub summary: fn(&P) -> String,
}

impl NodeType {
    /// The output ports of a node of this type with the given parameters.
    pub fn outputs_for(&self, params: &Params) -> Vec<String> {
        match self.ports {
            Some(f) => f(&P::new(params, self.params)),
            None => self.outputs.iter().map(|p| p.to_string()).collect(),
        }
    }
}

/// What a node gets to know about its surroundings.
pub struct NodeCtx<'a> {
    pub backend: Option<&'a dyn Backend>,
    /// The folder that relative paths start from, normally where the workspace file is.
    pub base_dir: &'a Path,
    pub node_name: &'a str,
    pub notes: Notes,
    /// The files a writer has written.
    pub files: Vec<PathBuf>,
    /// The files a reader has read.
    pub sources: Vec<PathBuf>,
    /// The local time when the run started. All nodes in a run see the same time.
    pub now: chrono::NaiveDateTime,
    /// Tells the run how far a long step has come: what is done ("read"), how many so
    /// far, and how many in all when that is known. The run writes it in the log.
    pub progress: &'a dyn Fn(&str, u64, Option<u64>),
    /// Writes a line of its own in the log at once, as it is.
    pub say: &'a dyn Fn(crate::run::Level, &str),
    /// Whether the user has stopped or paused the run. Long steps look at it now and then.
    pub gate: Gate<'a>,
    /// Each connection into a port that keeps its streams apart, such as the layers of
    /// Draw map, in the order of the connections.
    pub parts: Vec<Part>,
    /// The input ports that nothing is connected to.
    pub open_ports: Vec<&'static str>,
    /// The files the outputs have written so far in the run, with the names of the outputs.
    /// Make zip file runs last and packs them.
    pub written: Vec<(String, PathBuf)>,
}

/// The flags by which a run is stopped and paused from outside, such as from the window.
#[derive(Clone, Copy)]
pub struct Gate<'a> {
    pub stop: &'a std::sync::atomic::AtomicBool,
    pub pause: &'a std::sync::atomic::AtomicBool,
}

impl Gate<'_> {
    /// Whether the run may go on. While it is paused this waits, until it is resumed or
    /// stopped.
    pub fn going(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        while self.pause.load(Relaxed) && !self.stop.load(Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        !self.stop.load(Relaxed)
    }
}

/// One stream into a port that keeps its streams apart.
#[derive(Debug, Clone)]
pub struct Part {
    pub port: &'static str,
    /// Where it comes from, as "node:port". The key of the stream in the settings.
    pub from: String,
    /// The name of the node it comes from, and the port when the node has several.
    pub label: String,
    pub table: Arc<Table>,
}

/// Whether the streams into a port are kept apart, each as a part, rather than put together
/// as one stream.
pub fn keeps_apart(kind: &str, port: &str) -> bool {
    matches!(
        (kind, port),
        ("MapRenderer", "Layers")
            | ("RasterMosaicker", "Rasters")
            | ("RasterCalculator", "Rasters")
            | ("LayeredPdfWriter", "Layers")
            | ("SlideWriter", "Pictures")
            | ("Viewer", "Input")
            | ("SceneRenderer", "Models")
    )
}

impl<'a> NodeCtx<'a> {
    /// Whether the run has been stopped. A step that sees it ends with the error `STOPPED`.
    /// While the run is paused this waits.
    pub fn stopped(&self) -> bool {
        !self.gate.going()
    }

    pub fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base_dir.join(p)
        }
    }
    fn backend(&self, what: &str) -> Result<&'a dyn Backend> {
        self.backend.ok_or_else(|| GbError(format!("{} needs the GDAL support, which is not loaded.", what)))
    }
    /// Is the coordinate system in degrees? `None` if that cannot be determined.
    pub fn geographic(&self, crs: Option<u32>) -> Option<bool> {
        is_geographic(self.backend, crs)
    }
}

pub fn is_geographic(backend: Option<&dyn Backend>, crs: Option<u32>) -> Option<bool> {
    let code = crs?;
    if let Some(c) = KNOWN_CRS.iter().find(|c| c.0 == code) {
        return Some(c.2);
    }
    backend.and_then(|b| b.crs_info(code)).map(|i| i.geographic)
}

/// Reprojects a table to another coordinate system.
pub fn reproject(table: &Table, to: u32, backend: Option<&dyn Backend>) -> Result<Table> {
    let from = match table.crs {
        Some(c) => c,
        None if !table.has_geometry() => return Ok(Table { crs: None, ..table.clone() }),
        None => return err("The stream has no coordinate system and cannot be reprojected."),
    };
    if from == to {
        return Ok(table.clone());
    }
    let b = backend.ok_or_else(|| GbError("Reprojecting between coordinate systems needs the GDAL support, which is not loaded.".into()))?;
    // A grid or a picture is warped into the new system, cell by cell, and the stream is its
    // new outline.
    if let Some(image) = &table.image {
        let grid = match &table.grid {
            Some(g) => Some(b.warp_grid(g, to)?),
            None => None,
        };
        return Ok(b.warp_image(image, from, to)?.into_table_with(Some(to), grid));
    }
    if let Some(grid) = &table.grid {
        return Ok(b.warp_grid(grid, to)?.into_table());
    }
    let mut out = table.clone();
    out.crs = Some(to);
    geom::transform_all(out.features.iter_mut().filter_map(|f| f.geom.as_mut()), |xs, ys| b.transform(from, to, xs, ys))?;
    // A model goes along: its corners are reprojected, and its heights stay.
    if let Some(m) = &table.mesh {
        let mut m = (**m).clone();
        let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = m.positions.iter().map(|p| (p[0], p[1])).unzip();
        b.transform(from, to, &mut xs, &mut ys)?;
        for (p, (x, y)) in m.positions.iter_mut().zip(xs.into_iter().zip(ys)) {
            p[0] = x;
            p[1] = y;
        }
        out.mesh = Some(Arc::new(m));
    }
    Ok(out)
}

/// The most cells a grid put together from several has.
pub const GRID_CELLS: usize = 40_000_000;

/// Merges several tables into one. The schemas are united, and tables in another
/// coordinate system are reprojected to the first one. `what` names the tables in the
/// warning about that: "incoming streams" or "files".
pub fn merge_tables(tables: Vec<Arc<Table>>, backend: Option<&dyn Backend>, what: &str, notes: &mut Notes) -> Result<Arc<Table>> {
    match tables.len() {
        0 => return Ok(Arc::new(Table::default())),
        1 => return Ok(tables.into_iter().next().unwrap()),
        _ => {}
    }
    // Pictures in one coordinate system, such as the tiles of an orthophoto, become one.
    if tables.iter().all(|t| t.image.is_some()) && tables.iter().all(|t| t.crs == tables[0].crs) {
        let images: Vec<&crate::model::RasterImage> = tables.iter().filter_map(|t| t.image.as_deref()).collect();
        let img = raster::mosaic_images(&images, 60_000_000).ok_or_else(|| GbError("There is no picture to put together.".into()))?;
        notes.info(format!("{} pictures put together into {} × {} pixels", images.len(), img.width, img.height));
        // The values of the cells are put together too, when every picture has them.
        let grid = if tables.iter().all(|t| t.grid.is_some()) {
            let grids: Vec<&crate::model::Grid> = tables.iter().filter_map(|t| t.grid.as_deref()).collect();
            crate::model::Grid::mosaic(&grids, GRID_CELLS)
        } else {
            None
        };
        return Ok(Arc::new(img.into_table_with(tables[0].crs, grid)));
    }
    // Grids, such as the tiles of a terrain model, are put together into one grid.
    if tables.iter().all(|t| t.grid.is_some()) {
        let grids: Vec<&crate::model::Grid> = tables.iter().filter_map(|t| t.grid.as_deref()).collect();
        let crs = grids[0].crs;
        let same: Vec<&crate::model::Grid> = grids.iter().copied().filter(|g| g.crs == crs).collect();
        if same.len() < grids.len() {
            notes.warn(format!(
                "{} grids are in another coordinate system than the first, and were left out. Grids are not reprojected.",
                grids.len() - same.len()
            ));
        }
        let sizes: Vec<f64> = same.iter().map(|g| g.cell_size().0).collect();
        if sizes.iter().any(|s| (s - sizes[0]).abs() > 1e-9 * sizes[0].abs().max(1.0)) {
            notes.warn("The grids have different cell sizes. They were put together with the coarsest.".to_string());
        }
        let grid = crate::model::Grid::mosaic(&same, GRID_CELLS).ok_or_else(|| GbError("There is no grid to put together.".into()))?;
        notes.info(format!("{} grids put together into {} × {} cells", same.len(), grid.width, grid.height));
        return Ok(Arc::new(grid.into_table()));
    }
    let crs = tables.iter().find(|t| t.crs.is_some() && !t.is_empty()).or_else(|| tables.iter().find(|t| t.crs.is_some())).and_then(|t| t.crs);
    let mut out = Table {
        mesh: None,
        fields: Vec::new(),
        crs,
        features: Vec::new(),
        raster: !tables.is_empty() && tables.iter().all(|t| t.raster),
        image: None,
        grid: None,
    };
    for t in &tables {
        for f in &t.fields {
            match out.field_index(&f.name) {
                Some(i) => out.fields[i].ty = out.fields[i].ty.widen(f.ty),
                None => out.fields.push(f.clone()),
            }
        }
    }
    // Models, such as several OBJ files, become one when every stream has one.
    let all_models = tables.iter().all(|t| t.mesh.is_some());
    let mut models: Vec<Arc<crate::mesh::Mesh>> = Vec::new();
    for t in tables {
        let t: Arc<Table> = match (t.crs, crs) {
            (Some(a), Some(b)) if a != b => {
                notes.warn(format!("The {} have different coordinate systems. {} was reprojected to {}.", what, crs_label(Some(a)), crs_label(Some(b))));
                Arc::new(reproject(&t, b, backend)?)
            }
            _ => t,
        };
        let map: Vec<Option<usize>> = out.fields.iter().map(|f| t.field_index(&f.name)).collect();
        for f in &t.features {
            let attrs = map.iter().zip(&out.fields).map(|(src, field)| src.map_or(Value::Null, |i| coerce(f.attrs[i].clone(), field.ty))).collect();
            out.features.push(Feature { z: f.z.clone(), attrs, geom: f.geom.clone() });
        }
        if let Some(m) = &t.mesh {
            models.push(m.clone());
        }
    }
    if all_models && !models.is_empty() {
        let refs: Vec<&crate::mesh::Mesh> = models.iter().map(|m| m.as_ref()).collect();
        out.mesh = Some(Arc::new(crate::mesh::Mesh::merge(&refs)));
    }
    Ok(Arc::new(out))
}

/// What a writer is told about one attribute: the type to write it as, and its size.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FieldRule {
    /// `None` keeps the type the attribute has.
    pub ty: Option<FieldType>,
    /// The largest number of characters in a text.
    pub length: Option<u32>,
    /// The number of decimals in a decimal number.
    pub decimals: Option<u32>,
}

/// Rewrites attributes as a writer has been told to: another type, a largest length for
/// text, a number of decimals. Returns the new table and the sizes to give the file.
pub fn apply_field_rules(table: &Table, rules: &[(String, FieldRule)], notes: &mut Notes) -> (Table, HashMap<String, FieldSize>) {
    let mut out = table.clone();
    let mut sizes = HashMap::new();
    for (name, rule) in rules {
        let Some(i) = out.field_index(name) else {
            notes.warn(format!("The attribute {} has settings in the output but is not in the stream.", name));
            continue;
        };
        let ty = rule.ty.unwrap_or(out.fields[i].ty);
        let (mut not_numbers, mut cut) = (0usize, 0usize);
        for f in &mut out.features {
            let v = std::mem::replace(&mut f.attrs[i], Value::Null);
            f.attrs[i] = match (ty, v) {
                (_, Value::Null) => Value::Null,
                (FieldType::Text, v) => {
                    let text = v.to_text();
                    match rule.length {
                        Some(n) if text.chars().count() > n as usize => {
                            cut += 1;
                            Value::Text(text.chars().take(n as usize).collect())
                        }
                        _ => Value::Text(text),
                    }
                }
                (FieldType::Int, v) => match v.as_f64().or_else(|| parse_number(&v.to_text())) {
                    Some(x) => Value::Int(x.round() as i64),
                    None => {
                        not_numbers += 1;
                        Value::Null
                    }
                },
                (FieldType::Real, v) => match v.as_f64().or_else(|| parse_number(&v.to_text())) {
                    Some(x) => match rule.decimals {
                        Some(d) => {
                            let k = 10f64.powi(d.min(15) as i32);
                            Value::Real((x * k).round() / k)
                        }
                        None => Value::Real(x),
                    },
                    None => {
                        not_numbers += 1;
                        Value::Null
                    }
                },
            };
        }
        out.fields[i].ty = ty;
        if not_numbers > 0 {
            notes.warn(format!("{} values in {} are not numbers and were left empty.", not_numbers, name));
        }
        if cut > 0 {
            notes.warn(format!("{} values in {} are longer than {} characters and were cut.", cut, name, rule.length.unwrap_or(0)));
        }
        let size = FieldSize { length: rule.length, decimals: if ty == FieldType::Real { rule.decimals } else { None } };
        if size != FieldSize::default() {
            sizes.insert(name.clone(), size);
        }
    }
    (out, sizes)
}

/// Parameters with the defaults filled in, and helpers for reading them.
pub struct P<'a> {
    pub values: &'a Params,
    specs: &'static [ParamSpec],
}

impl<'a> P<'a> {
    pub fn new(values: &'a Params, specs: &'static [ParamSpec]) -> P<'a> {
        P { values, specs }
    }
    /// The value as text. Empty text and a missing value give the default, and `None` if that is empty too.
    pub fn get(&self, key: &str) -> Option<String> {
        let own = match self.values.get(key) {
            Some(serde_json::Value::String(s)) => Some(s.trim().to_string()),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            Some(serde_json::Value::Bool(b)) => Some(b.to_string()),
            _ => None,
        };
        own.filter(|s| !s.is_empty()).or_else(|| self.specs.iter().find(|s| s.key == key).map(|s| s.default.to_string()).filter(|s| !s.is_empty()))
    }
    /// The value exactly as it was written, spaces included. For separators and search texts.
    pub fn exact(&self, key: &str) -> String {
        match self.values.get(key) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => self.specs.iter().find(|s| s.key == key).map(|s| s.default.to_string()).unwrap_or_default(),
        }
    }
    pub fn need(&self, key: &str, what: &str) -> Result<String> {
        self.get(key).ok_or_else(|| GbError(what.to_string()))
    }
    pub fn flag(&self, key: &str) -> bool {
        matches!(self.get(key).as_deref(), Some("true") | Some("1") | Some("yes") | Some("ja"))
    }
    /// A list: either a JSON list or comma-separated text.
    pub fn list(&self, key: &str) -> Vec<String> {
        match self.values.get(key) {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect(),
            _ => self.get(key).map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default(),
        }
    }
    /// A list of values that may hold commas: a JSON list, or one value per line.
    pub fn list_exact(&self, key: &str) -> Vec<String> {
        match self.values.get(key) {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(|v| match v {
                    serde_json::Value::String(s) => Some(s.trim().to_string()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .filter(|s| !s.is_empty())
                .collect(),
            Some(serde_json::Value::String(s)) => s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
            _ => Vec::new(),
        }
    }
    /// Renames: either a JSON object or lines of the form old=new.
    pub fn pairs(&self, key: &str) -> Vec<(String, String)> {
        match self.values.get(key) {
            Some(serde_json::Value::Object(o)) => o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.trim().to_string(), s.trim().to_string()))).collect(),
            Some(serde_json::Value::String(s)) => s
                .lines()
                .filter_map(|l| l.split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())))
                .filter(|(a, b)| !a.is_empty() && !b.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }
    /// Pairs of a value and what it becomes: a JSON list of [from, to], or lines of the
    /// form from=to. In the order they are stored, empty values allowed on the right.
    pub fn mapping(&self, key: &str) -> Vec<(String, String)> {
        let text = |v: &serde_json::Value| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        match self.values.get(key) {
            Some(serde_json::Value::Array(rows)) => rows
                .iter()
                .filter_map(|r| r.as_array().filter(|a| a.len() == 2).map(|a| (text(&a[0]), text(&a[1]))))
                .filter(|(a, _)| !a.trim().is_empty())
                .collect(),
            Some(serde_json::Value::Object(o)) => o.iter().map(|(k, v)| (k.clone(), text(v))).collect(),
            Some(serde_json::Value::String(s)) => {
                s.lines().filter_map(|l| l.split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))).filter(|(a, _)| !a.is_empty()).collect()
            }
            _ => Vec::new(),
        }
    }
    /// The settings for single attributes: a JSON object from attribute name to
    /// `{"type": …, "length": …, "decimals": …}`. In the order they are stored.
    pub fn schema(&self, key: &str) -> Vec<(String, FieldRule)> {
        let number = |v: Option<&serde_json::Value>| match v {
            Some(serde_json::Value::Number(n)) => n.as_u64().map(|n| n as u32),
            Some(serde_json::Value::String(s)) => s.trim().parse::<u32>().ok(),
            _ => None,
        };
        match self.values.get(key) {
            Some(serde_json::Value::Object(o)) => o
                .iter()
                .filter_map(|(name, v)| {
                    let v = v.as_object()?;
                    let ty = match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "text" => Some(FieldType::Text),
                        "integer" => Some(FieldType::Int),
                        "decimal" => Some(FieldType::Real),
                        _ => None,
                    };
                    Some((name.clone(), FieldRule { ty, length: number(v.get("length")), decimals: number(v.get("decimals")) }))
                })
                .filter(|(_, r)| *r != FieldRule::default())
                .collect(),
            _ => Vec::new(),
        }
    }
    pub fn crs(&self, key: &str) -> Result<Option<u32>> {
        self.get(key).map(|s| parse_crs(&s)).transpose()
    }
}

fn input<'a>(inp: &'a Inputs, port: &str) -> &'a Table {
    inp.get(port).map(|t| t.as_ref()).expect("the run always fills in every input port")
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_text_format(ext: &str) -> bool {
    matches!(ext, "csv" | "txt" | "tsv")
}

pub fn is_spreadsheet(ext: &str) -> bool {
    matches!(ext, "xlsx" | "ods")
}

/// The formats an output writes as a picture: the image of the stream, not its features.
pub fn is_image_format(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "tif" | "tiff" | "pdf")
}

/// A choice of yes or no, stored as true or false.
const YES_NO: &[(&str, &str)] = &[("false", "No"), ("true", "Yes")];
const ENCODINGS: &[(&str, &str)] = &[("", "Detect"), ("utf-8", "UTF-8"), ("windows-1252", "Windows-1252")];

pub use attr_filter::{checks_of, value_problems, Check};
pub use attr_table::{
    attribute_rules, attribute_types, manager_from_old, manager_is_old, new_attributes, rules_json, sort_cmp, sort_keys, type_of_name, AttrAction,
    SORT_DIRECTIONS,
};
pub use attr_text::{change_case, replace_text, substring};
pub use attr_values::round_to;
pub use attr_values::{make_ranges, ranges_of, Range};
pub use chart::alphabetical;
pub use database::{connection as postgis_connection, named_connection, sql_literal, SqlTemplate};
pub use geom_inspect::geometry_problem;
pub use input::{
    is_plain_json, is_service, is_tileset, is_url, osm_tags, reader_sources, service_name, source_ext, wildcard_match, Sources, DEFAULT_PATTERN, READ_FORMATS,
    SERVICE_PREFIXES, WEB_SERVICES,
};
pub use json::{geojson, json_paths_of, name_of_path};
pub use map::{arrange_layers, map_layers, map_layers_json, MapLayer};
pub use output::{output_kind, view_name, writer_file, NameTemplate, OUTPUT_FORMATS, SHEET_COLORS, SHEET_FONTS, WRITER_FORMATS};
pub use raster::mosaic_images;

mod attr_filter;
mod attr_table;
mod attr_text;
mod attr_values;
mod chart;
mod database;
mod diorama;
mod geom_inspect;
mod geom_make;
mod geom_overlay;
mod geom_shape;
mod geom_transform;
mod input;
mod json;
mod map;
mod models091;
mod more_tools;
mod network;
mod network091;
mod osm;
mod output;
mod overture_dl;
mod pointcloud089;
mod pointcloud090;
mod props;
mod raster;
mod raster089;
mod raster090;
mod scene;
mod solid;
#[cfg(test)]
mod tests086;
#[cfg(test)]
mod tests087;
mod tools088;
mod tools091;
pub mod vector_tiles;
#[cfg(test)]
pub(crate) use solid::add_block;
mod terrain;
mod web;
mod workflow;

// Some modules have helpers that others use, such as same_crs in geom_overlay. Every
// module reaches them, and what is here, through `use super::*`.
use attr_filter::*;
use attr_values::*;
use geom_inspect::*;
use geom_overlay::*;
use geom_transform::*;
use input::*;
use output::*;

// ---------- The registry ----------

fn choice_label(list: &[(&str, &str)], v: &str) -> String {
    list.iter().find(|c| c.0 == v).map(|c| c.1).unwrap_or(v).to_string()
}

/// Every node type, from the modules by category. The order is that of the modules; the
/// palette sorts the tools by name.
pub static NODE_TYPES: std::sync::LazyLock<Vec<NodeType>> = std::sync::LazyLock::new(|| {
    [
        input::TYPES,
        output::TYPES,
        database::TYPES,
        attr_filter::TYPES,
        attr_values::TYPES,
        attr_text::TYPES,
        attr_table::TYPES,
        geom_overlay::TYPES,
        geom_shape::TYPES,
        geom_make::TYPES,
        geom_transform::TYPES,
        geom_inspect::TYPES,
        web::TYPES,
        json::TYPES,
        map::TYPES,
        chart::TYPES,
        scene::TYPES,
        solid::TYPES,
        diorama::TYPES,
        more_tools::TYPES,
        tools088::TYPES,
        tools091::TYPES,
        network091::TYPES,
        models091::TYPES,
        overture_dl::TYPES,
        raster089::TYPES,
        raster090::TYPES,
        pointcloud089::TYPES,
        pointcloud090::TYPES,
        osm::TYPES,
        terrain::TYPES,
        raster::TYPES,
        network::TYPES,
        workflow::TYPES,
    ]
    .concat()
});

pub fn node_type(name: &str) -> Option<&'static NodeType> {
    NODE_TYPES.iter().find(|t| t.name == name)
}

/// Runs one tool on tables in memory, for the tests of the tools: its outputs and notes.
#[cfg(test)]
pub(crate) fn run_for_test(kind: &str, params: &[(&str, &str)], given: Vec<(&'static str, Table)>) -> Result<(Outputs, Notes)> {
    let values: Params = params.iter().map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string()))).collect();
    run_for_test_with(kind, values, given)
}

/// As run_for_test, with the parameters as JSON, for those that are lists.
#[cfg(test)]
pub(crate) fn run_for_test_with(kind: &str, values: Params, given: Vec<(&'static str, Table)>) -> Result<(Outputs, Notes)> {
    let t = node_type(kind).expect("a tool of that name");
    let p = P::new(&values, t.params);
    let mut inp: Inputs = HashMap::new();
    for port in t.inputs {
        inp.insert(port, Arc::new(Table::default()));
    }
    // The streams also come as parts, as the run gives them to tools that keep them apart.
    let mut parts = Vec::new();
    for (port, table) in given {
        let table = Arc::new(table);
        parts.push(Part { port, from: format!("test:{}", port), label: port.to_string(), table: table.clone() });
        inp.insert(port, table);
    }
    let (stop, pause) = (std::sync::atomic::AtomicBool::new(false), std::sync::atomic::AtomicBool::new(false));
    let mut ctx = NodeCtx {
        backend: None,
        base_dir: Path::new("."),
        node_name: kind,
        notes: Notes::default(),
        files: Vec::new(),
        sources: Vec::new(),
        now: chrono::NaiveDateTime::default(),
        progress: &|_, _, _| {},
        say: &|_, _| {},
        gate: Gate { stop: &stop, pause: &pause },
        parts,
        open_ports: Vec::new(),
        written: Vec::new(),
    };
    let out = (t.run)(&p, &inp, &mut ctx)?;
    Ok((out, ctx.notes))
}
