//! The interface to file formats and coordinate systems. This is all the engine knows,
//! not GDAL, so a backend can be replaced without touching the transformers.

use crate::model::{Field, Result, Table};
use std::path::{Path, PathBuf};

/// Information and warnings that a node collects while it runs.
#[derive(Debug, Default, Clone)]
pub struct Notes {
    pub info: Vec<String>,
    pub warnings: Vec<String>,
}

impl Notes {
    pub fn info(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        if !self.info.contains(&m) {
            self.info.push(m);
        }
    }
    pub fn warn(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        if !self.warnings.contains(&m) {
            self.warnings.push(m);
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ReadOpts {
    /// Layer name in files with several layers, such as GeoPackage. Empty means the first layer.
    pub layer: Option<String>,
    /// Character encoding for formats that do not state it, such as a Shapefile without .cpg.
    pub encoding: Option<String>,
    /// Coordinate system to use when the file does not state one.
    pub crs: Option<u32>,
    /// Read a file that states no coordinate system, and none is given, without one instead
    /// of failing: for drawings, which seldom say.
    pub allow_no_crs: bool,
    /// For spreadsheets: whether the first row holds the column names. `None` lets the
    /// backend decide.
    pub header: Option<bool>,
    /// The largest number of features to read. `None` reads all.
    pub limit: Option<usize>,
    /// For rasters: what becomes of the cells.
    pub raster: RasterMode,
    /// A query to read instead of a layer, such as SQL against a database.
    pub sql: Option<String>,
    /// For rasters read as a grid: the size of the cells to read them at. None reads them
    /// as they are, or coarser when there are too many.
    pub cell_size: Option<f64>,
    /// Only the features that reach into this rectangle (min x, min y, max x, max y), in the
    /// coordinate system given, or that of the data when None. A file with a spatial index,
    /// such as a GeoPackage, then reads only those.
    pub within: Option<([f64; 4], Option<u32>)>,
    /// Headers to send when the data is a web service, such as a key.
    pub headers: Vec<(String, String)>,
    /// DXF: blocks as points that carry the values of their attributes, instead of drawn out.
    pub dxf_blocks_as_points: bool,
    /// DXF: closed polylines as areas.
    pub dxf_areas: bool,
}

/// How a raster, such as a GeoTIFF, is read into features.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RasterMode {
    /// A point in the middle of each cell, with the values of the bands.
    #[default]
    Points,
    /// A square for each cell, with the values of the bands.
    Cells,
    /// One area: the outline of the raster, with its size and cell size.
    Extent,
    /// A picture: the bands as colours, with the outline as the one feature.
    Image,
    /// The values of the first band on a grid, such as heights for rendering terrain, with
    /// the outline as the one feature.
    Grid,
}

/// The file extensions that are read as rasters.
pub fn is_raster(ext: &str) -> bool {
    matches!(ext, "tif" | "tiff" | "asc" | "img" | "vrt" | "jp2" | "png" | "jpg" | "jpeg")
}

/// The primary key of a table that is written to a database or a GeoPackage.
#[derive(Debug, Clone, PartialEq)]
pub enum PrimaryKey {
    /// A new column of numbers, 1, 2, 3 …, by this name.
    New(String),
    /// An attribute whose values are all different and none empty.
    Attribute(String),
}

#[derive(Debug, Default, Clone)]
pub struct WriteOpts {
    pub layer: Option<String>,
    pub encoding: Option<String>,
    /// How wide chosen fields are to be in the file, by field name.
    pub sizes: std::collections::HashMap<String, FieldSize>,
    /// How a sheet in a workbook is to look. Other formats leave it alone.
    pub sheet: SheetStyle,
    /// Add the features to the layer, sheet or file that is there, instead of replacing it.
    pub append: bool,
    /// How a picture is laid out on a PDF page.
    pub pdf: PdfPage,
    /// The primary key, for a GeoPackage. None: a new column fid.
    pub key: Option<PrimaryKey>,
}

/// Where and how a stream is written to a table in a PostgreSQL database with PostGIS.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DbWrite {
    pub schema: String,
    pub table: String,
    /// "auto" for what the features are, or point, multipoint, line, multiline, polygon,
    /// multipolygon, or none for a table without geometry.
    pub geometry: String,
    /// The name of the geometry column.
    pub geom_column: String,
    /// Add to the table that is there instead of replacing it.
    pub append: bool,
    /// The primary key of a new table. None: a new column ogc_fid.
    pub key: Option<PrimaryKey>,
}

/// One layer of a PDF with layers: features, or a picture, with how they look.
pub struct PdfLayer<'a> {
    pub name: String,
    pub table: &'a crate::model::Table,
    pub style: crate::mapdraw::LayerStyle,
    /// Whether it is on when the PDF is opened. It can be turned on and off in the reader.
    pub visible: bool,
}

/// The page of a PDF with a picture on it.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfPage {
    /// "fit" for a page as large as the picture; a paper size such as "a4", "a3", "a0",
    /// "letter"; or "custom" for `custom_mm`.
    pub size: String,
    /// Width and height of a page of size "custom", in millimetres.
    pub custom_mm: (f64, f64),
    /// Pixels to the inch on a page as large as the picture.
    pub dpi: f64,
    /// How a picture is packed: "deflate" keeps every pixel, "jpeg" is smaller for photos and
    /// shaded relief, at `quality` from 1 to 100.
    pub compress: String,
    pub quality: u8,
    /// "auto" turns the page after the picture; or "portrait" or "landscape".
    pub orientation: String,
    /// Empty space around the picture, in millimetres.
    pub margin_mm: f64,
    /// A heading above the picture.
    pub title: Option<String>,
    /// Where the heading goes along the top of the picture: left, center or right.
    pub title_align: String,
}

impl Default for PdfPage {
    fn default() -> Self {
        PdfPage {
            size: "a4".into(),
            custom_mm: (210.0, 297.0),
            dpi: 150.0,
            compress: "deflate".into(),
            quality: 85,
            orientation: "auto".into(),
            margin_mm: 10.0,
            title: None,
            title_align: "left".into(),
        }
    }
}

/// Where things go on a PDF page, in points (1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfLayout {
    pub page_w: f64,
    pub page_h: f64,
    /// Points per pixel of the picture.
    pub scale: f64,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
    /// Where the title starts, from the lower left corner of the page, and its size.
    pub title_at: (f64, f64),
    pub title_size: f64,
}

/// The width and height of a paper size, in millimetres, the short side first. A size that
/// is not known is A4.
pub fn paper_mm(size: &str, custom: (f64, f64)) -> (f64, f64) {
    match size {
        "a0" => (841.0, 1189.0),
        "a1" => (594.0, 841.0),
        "a2" => (420.0, 594.0),
        "a3" => (297.0, 420.0),
        "a5" => (148.0, 210.0),
        "letter" => (215.9, 279.4),
        "legal" => (215.9, 355.6),
        "tabloid" => (279.4, 431.8),
        "custom" => (custom.0.clamp(20.0, 5000.0), custom.1.clamp(20.0, 5000.0)),
        _ => (210.0, 297.0),
    }
}

impl PdfPage {
    /// Lays out a picture of `w` × `h` pixels on the page. The picture keeps its shape and
    /// is as large as the page allows, in the middle. A page that fits the picture shows it
    /// at `dpi` pixels to the inch.
    pub fn layout(&self, w: usize, h: usize) -> PdfLayout {
        let mm = 72.0 / 25.4;
        let margin = self.margin_mm.max(0.0) * mm;
        let title_size = 16.0;
        let title_room = if self.title.as_deref().is_some_and(|t| !t.trim().is_empty()) { title_size * 2.0 } else { 0.0 };
        let (w, h) = (w.max(1) as f64, h.max(1) as f64);
        let wide = w > h;
        let (pw, ph, scale) = match self.size.as_str() {
            "fit" => {
                let scale = 72.0 / self.dpi.clamp(10.0, 2400.0);
                let pw = w * scale + 2.0 * margin;
                let ph = h * scale + 2.0 * margin + title_room;
                (pw, ph, scale)
            }
            size => {
                let (a, b) = paper_mm(size, self.custom_mm);
                let (short, long) = (a.min(b) * mm, a.max(b) * mm);
                let landscape = match self.orientation.as_str() {
                    "portrait" => false,
                    "landscape" => true,
                    // A size of one's own is as it was given.
                    _ if size == "custom" => a > b,
                    _ => wide,
                };
                let (pw, ph) = if landscape { (long, short) } else { (short, long) };
                let (aw, ah) = ((pw - 2.0 * margin).max(10.0), (ph - 2.0 * margin - title_room).max(10.0));
                (pw, ph, (aw / w).min(ah / h))
            }
        };
        let (iw, ih) = (w * scale, h * scale);
        let left = (pw - iw) / 2.0;
        let space = ph - 2.0 * margin - title_room - ih;
        let bottom = margin + space.max(0.0) / 2.0;
        let top = ph - bottom - ih;
        let tw = self.title.as_deref().map_or(0.0, |t| title_width(t.trim(), title_size));
        let tx = match self.title_align.as_str() {
            "center" => left + (iw - tw) / 2.0,
            "right" => left + iw - tw,
            _ => left,
        };
        // A title wider than the picture still starts on the page.
        let tx = tx.max(margin.min(left));
        PdfLayout { page_w: pw, page_h: ph, scale, left, bottom, right: pw - left - iw, top, title_at: (tx, bottom + ih + title_size * 0.75), title_size }
    }
}

/// The width in points of a text in Helvetica Bold, the font of the title, from the widths
/// of its letters in thousandths of the size.
pub fn title_width(text: &str, size: f64) -> f64 {
    // The characters from space to tilde.
    const ASCII: [u16; 95] = [
        278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584,
        584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333,
        278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556,
        500, 389, 280, 389, 584,
    ];
    let units: u32 = text
        .chars()
        .map(|c| match c {
            ' '..='~' => ASCII[c as usize - 32] as u32,
            // Letters with marks, such as å, ä, ö and é, are about as wide as the letter itself.
            'å' | 'ä' | 'à' | 'á' | 'â' | 'ã' | 'é' | 'è' | 'ê' | 'ë' => 556,
            'Å' | 'Ä' | 'À' | 'Á' | 'Â' | 'Ã' => 722,
            'Ö' | 'Ø' | 'Ó' | 'Ò' => 778,
            c if c.is_uppercase() => 722,
            '–' => 556,
            '—' => 1000,
            _ => 611,
        })
        .sum();
    units as f64 / 1000.0 * size
}

/// How a sheet in a workbook is to look. The default is plain values, as GDAL writes them.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SheetStyle {
    /// The font of every cell. None leaves the font of the program that opens the file.
    pub font: Option<String>,
    /// The names of the columns in bold.
    pub bold_header: bool,
    /// The first row stays in place when the sheet is scrolled.
    pub freeze_header: bool,
    /// A colour scheme by name: the top row filled, and every second row tinted.
    pub colors: Option<String>,
}

/// The size of a field in a file, for formats that have such a thing.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FieldSize {
    /// The number of characters in a text, or of digits in a number.
    pub length: Option<u32>,
    /// The number of decimals in a decimal number.
    pub decimals: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LayerInfo {
    pub name: String,
    pub count: Option<u64>,
    pub crs: Option<u32>,
    pub crs_name: Option<String>,
    pub geometry: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub struct CrsInfo {
    pub name: String,
    /// True if the coordinates are in degrees.
    pub geographic: bool,
}

/// A coordinate system from the database of PROJ, for choosing among all of them.
#[derive(Debug, Clone, PartialEq)]
pub struct CrsEntry {
    pub code: u32,
    pub name: String,
    /// "projected", "geographic" or "compound" (with heights).
    pub kind: &'static str,
    /// Where it is meant to be used, such as "Sweden".
    pub area: String,
}

/// Sync, so that web requests can be sent from several threads at once.
pub trait Backend: Sync {
    /// One line about what the backend is, for example "GDAL 3.8.4".
    fn describe(&self) -> String;
    fn read(&self, path: &Path, opts: &ReadOpts, notes: &mut Notes) -> Result<Table>;
    /// Reads as `read` does, and tells `progress` how far it has come now and then: the
    /// number of features read, and how many there are when that is known beforehand.
    /// When `progress` answers false the run has been stopped, and reading ends with the
    /// error `STOPPED`.
    fn read_with(&self, path: &Path, opts: &ReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
        let _ = progress;
        self.read(path, opts, notes)
    }
    /// Runs SQL statements one after the other on one connection to a database, such as
    /// "PG:host=...". Each gives the rows it returned, None for a statement that returns
    /// none, such as UPDATE, or the error the database gave. `going` is asked between them.
    fn run_sql(
        &self,
        path: &Path,
        statements: &[String],
        opts: &ReadOpts,
        notes: &mut Notes,
        going: &dyn Fn(u64, Option<u64>) -> bool,
    ) -> Result<Vec<std::result::Result<Option<Table>, String>>> {
        let _ = (statements, opts, notes, going);
        Err(crate::model::GbError(format!("This build cannot run SQL against {}.", path.display())))
    }
    /// Writes the table and returns the files that were created.
    fn write(&self, table: &Table, path: &Path, opts: &WriteOpts, notes: &mut Notes) -> Result<Vec<PathBuf>>;
    /// Reprojects coordinates in place. The axis order is always x = east/longitude, y = north/latitude.
    fn transform(&self, from: u32, to: u32, xs: &mut [f64], ys: &mut [f64]) -> Result<()>;
    fn crs_info(&self, epsg: u32) -> Option<CrsInfo>;
    /// Every EPSG coordinate system PROJ knows that maps can be in: projected, geographic and
    /// compound ones. Empty without PROJ.
    fn crs_list(&self) -> Vec<CrsEntry> {
        Vec::new()
    }
    /// The libraries behind the backend and their versions, such as GDAL and PROJ.
    fn versions(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Writes a geospatial PDF with a layer for each of `layers`, from the bottom up, all in
    /// the coordinate system `crs`. `margin` is the room around the features, as a share.
    fn write_layered_pdf(&self, layers: &[PdfLayer], crs: u32, page: &PdfPage, margin: f64, path: &Path, notes: &mut Notes) -> Result<Vec<PathBuf>> {
        let _ = (layers, crs, page, margin, path, notes);
        Err(crate::model::GbError("A PDF with layers needs the GDAL support.".into()))
    }
    /// Writes the table to a database table, through the connection `conn` in the form GDAL
    /// takes ("PG:host=..."). Returns how many features were written.
    fn write_postgis(&self, table: &Table, conn: &str, opts: &DbWrite, notes: &mut Notes) -> Result<u64> {
        let _ = (table, conn, opts, notes);
        Err(crate::model::GbError("Writing to PostGIS needs the GDAL support.".into()))
    }
    /// Each geometry made valid, as GDAL does it, or None where that fails.
    fn make_valid(&self, geoms: &[geo::Geometry<f64>]) -> Result<Vec<Option<geo::Geometry<f64>>>> {
        let _ = geoms;
        Err(crate::model::GbError("Repairing geometries needs the GDAL support.".into()))
    }
    /// Contour lines of a grid, at every `interval` from `base`, with the height in the
    /// attribute `name`. With `areas`, the areas between the lines instead, with name_min and
    /// name_max.
    fn contours(&self, grid: &crate::model::Grid, interval: f64, base: f64, areas: bool, name: &str) -> Result<crate::model::Table> {
        let _ = (grid, interval, base, areas, name);
        Err(crate::model::GbError("Contour lines need the GDAL support.".into()))
    }
    /// A grid of values in another coordinate system.
    fn warp_grid(&self, grid: &crate::model::Grid, to: u32) -> Result<crate::model::Grid> {
        let _ = (grid, to);
        Err(crate::model::GbError("Reprojecting a grid needs the GDAL support.".into()))
    }
    /// A picture in another coordinate system.
    fn warp_image(&self, image: &crate::model::RasterImage, from: u32, to: u32) -> Result<crate::model::RasterImage> {
        let _ = (image, from, to);
        Err(crate::model::GbError("Reprojecting a picture needs the GDAL support.".into()))
    }
    fn layers(&self, path: &Path) -> Result<Vec<LayerInfo>>;
    /// Fetches a file from a web address and returns where the copy is. With `keep`, a
    /// copy from an earlier run is used if there is one.
    fn fetch(&self, url: &str, keep: bool, notes: &mut Notes) -> Result<PathBuf> {
        let _ = (keep, notes);
        Err(crate::model::GbError(format!("This build cannot read from the web: {}", url)))
    }
    /// As `fetch`, with headers sent along, such as a key.
    fn fetch_with(&self, url: &str, headers: &[(String, String)], keep: bool, notes: &mut Notes) -> Result<PathBuf> {
        if headers.is_empty() {
            self.fetch(url, keep, notes)
        } else {
            Err(crate::model::GbError(format!("This build cannot send keys to {}", url)))
        }
    }
    /// Sends a GET request. An answer with any status code is Ok; an error is when no
    /// answer came, such as when the server cannot be reached or does not answer in time.
    fn http_get(&self, url: &str, headers: &[(String, String)], timeout_s: u64) -> Result<HttpAnswer> {
        let _ = (headers, timeout_s);
        Err(crate::model::GbError(format!("This build cannot send web requests: {}", url)))
    }
    /// Fetches an address and gives back what came, as bytes, such as a tile of a 3D Tiles
    /// set. An answer with an error status is an error.
    fn fetch_bytes(&self, url: &str, headers: &[(String, String)]) -> Result<Vec<u8>> {
        let _ = headers;
        Err(crate::model::GbError(format!("This build cannot read from the web: {}", url)))
    }
    /// A piece of what an address holds, from byte `start` and `len` bytes long, and how
    /// long the whole is when the server says. For files on the web that are read in parts,
    /// such as COPC point clouds.
    fn fetch_range(&self, url: &str, headers: &[(String, String)], start: u64, len: u64) -> Result<(Vec<u8>, Option<u64>)> {
        let _ = (headers, start, len);
        Err(crate::model::GbError(format!("This build cannot read from the web: {}", url)))
    }
    /// Sends a request with a method, such as POST, and a body to send. GET sends no body.
    fn http_send(&self, method: &str, url: &str, headers: &[(String, String)], body: Option<&str>, timeout_s: u64) -> Result<HttpAnswer> {
        let _ = body;
        if method == "GET" {
            self.http_get(url, headers, timeout_s)
        } else {
            Err(crate::model::GbError(format!("This build cannot send {} requests: {}", method, url)))
        }
    }
}

/// What a web server answered.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpAnswer {
    pub status: u16,
    pub body: String,
    /// The headers of the answer, as they came.
    pub headers: Vec<(String, String)>,
}

impl HttpAnswer {
    /// A header by its name, in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A guess at the coordinate system from the extent of the coordinates, for a file that
/// does not say: maps of Sweden are mostly in SWEREF 99 TM or a local zone of SWEREF 99.
pub fn guess_crs(extent: [f64; 4]) -> Option<&'static str> {
    let (x, y) = ((extent[0] + extent[2]) / 2.0, (extent[1] + extent[3]) / 2.0);
    if !x.is_finite() || !y.is_finite() {
        None
    } else if (6_100_000.0..7_700_000.0).contains(&y) && (250_000.0..950_000.0).contains(&x) {
        Some("They look like SWEREF 99 TM (EPSG:3006), or a local zone of SWEREF 99 such as 12 00 (EPSG:3007)")
    } else if (6_100_000.0..7_700_000.0).contains(&y) && (50_000.0..250_000.0).contains(&x) {
        Some("They look like a local zone of SWEREF 99, such as 12 00 (EPSG:3007) or 13 30 (EPSG:3008)")
    } else if (6_100_000.0..7_700_000.0).contains(&x) && (1_200_000.0..1_900_000.0).contains(&y) {
        Some("They look like RT 90 2.5 gon V (EPSG:3021), north and east the other way round")
    } else if extent[0] >= -180.0 && extent[2] <= 180.0 && extent[1] >= -90.0 && extent[3] <= 90.0 && extent[2] - extent[0] < 5.0 {
        Some("They look like longitude and latitude (EPSG:4326)")
    } else if (100_000.0..900_000.0).contains(&x) && (0.0..10_000_000.0).contains(&y) {
        Some("They look like metres in a UTM zone or a similar projection, but not one of Sweden")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_placed_left_in_the_middle_or_right() {
        let page = |align: &str| PdfPage { title: Some("Vårdcentraler".into()), title_align: align.into(), ..Default::default() };
        let left = page("left").layout(1000, 600);
        let center = page("center").layout(1000, 600);
        let right = page("right").layout(1000, 600);
        let tw = title_width("Vårdcentraler", left.title_size);
        let iw = left.page_w - left.left - left.right;
        assert!((left.title_at.0 - left.left).abs() < 0.01);
        assert!((center.title_at.0 + tw / 2.0 - (left.left + iw / 2.0)).abs() < 0.01);
        assert!((right.title_at.0 + tw - (left.left + iw)).abs() < 0.01);
        assert_eq!(left.title_at.1, right.title_at.1);
        // Widths follow the font: a W is wider than an i.
        assert!(title_width("W", 10.0) > 3.0 * title_width("i", 10.0));
        assert!((title_width("Hello", 1000.0) - (722.0 + 556.0 + 278.0 + 278.0 + 611.0)).abs() < 0.01);
    }
}
