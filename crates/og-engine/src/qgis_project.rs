//! A QGIS project of the layers of a map: the features in a GeoPackage beside it, pictures
//! as GeoTIFF, and each layer with the look it has in Ographis: its colours and widths, the
//! classes of colour it shows its values by, sizes after a number and its labels. To open in
//! QGIS 3 and go on from there.

use crate::mapdraw::{LabelSpec, LayerStyle};
use crate::model::{GbError, Result};
use crate::thematic::{ClassKey, Look, Theme, ThemeSpec};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

/// What a layer of the project is.
pub enum Source {
    /// Features in a layer of the GeoPackage: its name there, and point, line or polygon.
    Vector { gpkg_layer: String, geometry: &'static str, size_range: Option<(f64, f64)> },
    /// A picture in a GeoTIFF beside the project, or values in grey.
    Picture { file: String, grey: bool },
}

/// A layer of the project, from the top.
pub struct ProjectLayer {
    pub name: String,
    pub source: Source,
    pub visible: bool,
    pub style: LayerStyle,
    pub theme: Theme,
    pub spec: ThemeSpec,
    pub labels: LabelSpec,
}

/// A background map of tiles under the layers.
pub struct Basemap {
    pub name: String,
    pub url: String,
}

pub struct Project<'a> {
    pub title: String,
    pub crs: u32,
    pub crs_wkt: Option<String>,
    pub extent: [f64; 4],
    /// The GeoPackage, by its file name beside the project.
    pub gpkg: &'a str,
    pub layers: Vec<ProjectLayer>,
    pub basemap: Option<Basemap>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn rgba(c: Option<[u8; 4]>) -> String {
    let c = c.unwrap_or([0, 0, 0, 0]);
    format!("{},{},{},{}", c[0], c[1], c[2], c[3])
}

fn srs(epsg: u32, wkt: Option<&str>) -> String {
    let wkt = wkt.map(|w| format!("<wkt>{}</wkt>", esc(w))).unwrap_or_default();
    format!("<spatialrefsys nativeFormat=\"Wkt\">{}<authid>EPSG:{}</authid></spatialrefsys>", wkt, epsg)
}

fn opt(name: &str, value: &str) -> String {
    format!("<Option name=\"{}\" value=\"{}\" type=\"QString\"/>", name, esc(value))
}

/// A symbol of QGIS with one simple layer: a fill, a line or a circle, in pixels as
/// Ographis draws them.
fn symbol(name: &str, geometry: &str, look: &Look, width: f32, size_expr: Option<&str>) -> String {
    let (kind, class, props) = match geometry {
        "polygon" => (
            "fill",
            "SimpleFill",
            vec![
                opt("color", &rgba(look.fill)),
                opt("style", if look.fill.is_some() { "solid" } else { "no" }),
                opt("outline_color", &rgba(look.stroke)),
                opt("outline_style", if look.stroke.is_some() && width > 0.0 { "solid" } else { "no" }),
                opt("outline_width", &width.to_string()),
                opt("outline_width_unit", "Pixel"),
                opt("joinstyle", "round"),
            ],
        ),
        "line" => (
            "line",
            "SimpleLine",
            vec![
                opt("line_color", &rgba(look.stroke)),
                opt("line_style", if look.stroke.is_some() { "solid" } else { "no" }),
                opt("line_width", &width.max(0.1).to_string()),
                opt("line_width_unit", "Pixel"),
                opt("capstyle", "round"),
                opt("joinstyle", "round"),
            ],
        ),
        _ => (
            "marker",
            "SimpleMarker",
            vec![
                opt("name", "circle"),
                opt("color", &rgba(look.fill)),
                opt("outline_color", &rgba(look.stroke)),
                opt("outline_style", if look.stroke.is_some() && width > 0.0 { "solid" } else { "no" }),
                opt("outline_width", &width.to_string()),
                opt("outline_width_unit", "Pixel"),
                opt("size", &look.size.to_string()),
                opt("size_unit", "Pixel"),
            ],
        ),
    };
    let defined = match size_expr {
        Some(e) if kind == "marker" => format!(
            "<data_defined_properties><Option type=\"Map\">{}<Option name=\"properties\" type=\"Map\"><Option name=\"size\" type=\"Map\"><Option name=\"active\" value=\"true\" type=\"bool\"/>{}<Option name=\"type\" value=\"3\" type=\"int\"/></Option></Option>{}</Option></data_defined_properties>",
            opt("name", ""),
            opt("expression", e),
            opt("type", "collection")
        ),
        _ => String::new(),
    };
    format!(
        "<symbol type=\"{}\" name=\"{}\" alpha=\"1\" clip_to_extent=\"1\" force_rhr=\"0\"><layer class=\"{}\" enabled=\"1\" pass=\"0\" locked=\"0\"><Option type=\"Map\">{}</Option>{}</layer></symbol>",
        kind,
        esc(name),
        class,
        props.join(""),
        defined
    )
}

fn field_ref(name: &str) -> String {
    format!("\"{}\"", name.trim().replace('"', "\"\""))
}

/// How the features of a layer are drawn: one symbol, classes of a number, or a colour for
/// each text.
fn renderer(l: &ProjectLayer, geometry: &str, size_range: Option<(f64, f64)>) -> String {
    let width = l.style.width;
    let size_expr = size_range.filter(|_| !l.spec.size_by.trim().is_empty()).map(|(lo, hi)| {
        let (smin, smax) = (l.spec.size_min.max(1.0), l.spec.size_max.max(l.spec.size_min.max(1.0)));
        if hi > lo {
            // By area, as in Ographis: the diameter with the square root.
            format!("coalesce(scale_exp({}, {}, {}, {}, {}, 0.5), {})", field_ref(&l.spec.size_by), lo, hi, smin, smax, smin)
        } else {
            format!("{}", (smin + smax) / 2.0)
        }
    });
    let base = Look { fill: l.style.fill, stroke: l.style.stroke, size: l.style.size };
    let single = |look: &Look| {
        format!(
            "<renderer-v2 type=\"singleSymbol\" symbollevels=\"0\" enableorderby=\"0\" forceraster=\"0\"><symbols>{}</symbols></renderer-v2>",
            symbol("0", geometry, look, width, size_expr.as_deref())
        )
    };
    let classes = &l.theme.classes;
    if classes.is_empty() || l.spec.color_by.trim().is_empty() {
        return single(&base);
    }
    let attr = esc(l.spec.color_by.trim());
    // The look of a class with the size and width of the layer.
    let look_of = |c: &Look| Look { fill: c.fill, stroke: c.stroke, size: base.size };
    if classes.iter().any(|c| c.0 == ClassKey::Written) {
        // Colours as written in the attribute: drawn by it in QGIS too.
        let mut s = single(&base);
        let prop = if geometry == "line" { "outlineColor" } else { "fillColor" };
        let defined = format!(
            "<data_defined_properties><Option type=\"Map\">{}<Option name=\"properties\" type=\"Map\"><Option name=\"{}\" type=\"Map\"><Option name=\"active\" value=\"true\" type=\"bool\"/>{}<Option name=\"type\" value=\"3\" type=\"int\"/></Option></Option>{}</Option></data_defined_properties>",
            opt("name", ""),
            prop,
            opt("expression", &field_ref(&l.spec.color_by)),
            opt("type", "collection")
        );
        s = s.replacen("</layer>", &format!("{}</layer>", defined), 1);
        return s;
    }
    let mut symbols = String::new();
    let mut items = String::new();
    let numeric = classes.iter().any(|c| matches!(c.0, ClassKey::Range(..)));
    for (k, (key, look)) in classes.iter().enumerate() {
        symbols.push_str(&symbol(&k.to_string(), geometry, &look_of(look), width, size_expr.as_deref()));
        let label = l.theme.keys.get(k).map(|x| x.text.clone()).unwrap_or_default();
        match key {
            ClassKey::Range(lo, hi) => {
                let _ = write!(items, "<range lower=\"{}\" upper=\"{}\" symbol=\"{}\" label=\"{}\" render=\"true\"/>", lo, hi, k, esc(&label));
            }
            ClassKey::Value(v) => {
                let _ = write!(items, "<category value=\"{}\" symbol=\"{}\" label=\"{}\" render=\"true\" type=\"string\"/>", esc(v), k, esc(&label));
            }
            // Other is the class of all values without one of their own; no value, of the
            // empty ones.
            ClassKey::Other => {
                let _ = write!(items, "<category value=\"\" symbol=\"{}\" label=\"{}\" render=\"true\" type=\"string\"/>", k, esc(&label));
            }
            // Without an Other class, the empty ones are the class of all other values.
            ClassKey::NoValue if !numeric && !classes.iter().any(|c| c.0 == ClassKey::Other) => {
                let _ = write!(items, "<category value=\"\" symbol=\"{}\" label=\"{}\" render=\"true\" type=\"string\"/>", k, esc(&label));
            }
            _ => {}
        }
    }
    if numeric {
        format!(
            "<renderer-v2 type=\"graduatedSymbol\" attr=\"{}\" graduatedMethod=\"GraduatedColor\" symbollevels=\"0\" enableorderby=\"0\" forceraster=\"0\"><ranges>{}</ranges><symbols>{}</symbols></renderer-v2>",
            attr, items, symbols
        )
    } else {
        format!(
            "<renderer-v2 type=\"categorizedSymbol\" attr=\"{}\" symbollevels=\"0\" enableorderby=\"0\" forceraster=\"0\"><categories>{}</categories><symbols>{}</symbols></renderer-v2>",
            attr, items, symbols
        )
    }
}

/// The labels of a layer, as QGIS places them.
fn labeling(spec: &LabelSpec, geometry: &str) -> String {
    let text = spec.text.trim();
    if text.is_empty() {
        return String::new();
    }
    // A plain attribute, or an expression of Ographis, which for the most part reads the
    // same in QGIS.
    let plain = text.chars().all(|c| c.is_alphanumeric() || c == '_');
    let (field, is_expr) = if plain { (text.to_string(), 0) } else { (text.to_string(), 1) };
    let c = spec.colour;
    let halo = match spec.halo {
        Some(h) => format!(
            "<text-buffer bufferDraw=\"1\" bufferSize=\"{}\" bufferSizeUnits=\"Pixel\" bufferColor=\"{},{},{},{}\" bufferOpacity=\"1\" bufferJoinStyle=\"128\"/>",
            spec.halo_width.max(0.5),
            h[0],
            h[1],
            h[2],
            h[3]
        ),
        None => "<text-buffer bufferDraw=\"0\"/>".to_string(),
    };
    // Around points, along lines and inside areas.
    let placement = match geometry {
        "line" => 2,
        // In the middle of areas, or around it when something is in the way.
        "polygon" => {
            if spec.place == "center" {
                1
            } else {
                0
            }
        }
        _ => {
            if spec.place == "center" {
                1
            } else {
                6
            }
        }
    };
    format!(
        "<labeling type=\"simple\"><settings calloutType=\"simple\"><text-style fieldName=\"{}\" isExpression=\"{}\" fontSize=\"{}\" fontSizeUnit=\"Pixel\" fontWeight=\"{}\" textColor=\"{},{},{},{}\" textOpacity=\"1\" fontCapitals=\"{}\" fontFamily=\"Open Sans\">{}</text-style><placement placement=\"{}\" dist=\"1\" distUnits=\"MM\"/><rendering drawLabels=\"1\" obstacle=\"1\" obstacleType=\"1\" displayAll=\"{}\" upsidedownLabels=\"0\"/></settings></labeling>",
        esc(&field),
        is_expr,
        spec.size.max(4.0),
        if spec.bold { 75 } else { 50 },
        c[0],
        c[1],
        c[2],
        c[3],
        if spec.upper { 1 } else { 0 },
        halo,
        placement,
        if spec.all { 1 } else { 0 }
    )
}

/// The text of the project file (.qgs).
pub fn project_xml(p: &Project) -> String {
    let crs = srs(p.crs, p.crs_wkt.as_deref());
    let mut tree = String::new();
    let mut maps = String::new();
    let mut order = String::new();
    let ids: Vec<String> = (0..p.layers.len()).map(|i| format!("ographis_layer_{}", i + 1)).collect();
    for (l, id) in p.layers.iter().zip(&ids) {
        let checked = if l.visible { "Qt::Checked" } else { "Qt::Unchecked" };
        let (source, provider, kind, geometry) = match &l.source {
            Source::Vector { gpkg_layer, geometry, .. } => (format!("./{}|layername={}", p.gpkg, gpkg_layer), "ogr", "vector", *geometry),
            Source::Picture { file, .. } => (format!("./{}", file), "gdal", "raster", ""),
        };
        let _ = write!(
            tree,
            "<layer-tree-layer id=\"{}\" name=\"{}\" source=\"{}\" providerKey=\"{}\" checked=\"{}\" expanded=\"1\"><customproperties/></layer-tree-layer>",
            id,
            esc(&l.name),
            esc(&source),
            provider,
            checked
        );
        let _ = write!(order, "<item>{}</item>", id);
        let body = match &l.source {
            Source::Vector { size_range, .. } => {
                let labels = labeling(&l.labels, geometry);
                format!("{}{}", renderer(l, geometry, *size_range), labels)
            }
            Source::Picture { grey: true, .. } => "<pipe><rasterrenderer type=\"singlebandgray\" grayBand=\"1\" gradient=\"BlackToWhite\" opacity=\"1\"><rasterTransparency/><minMaxOrigin><limits>MinMax</limits><extent>WholeRaster</extent><statAccuracy>Estimated</statAccuracy></minMaxOrigin><contrastEnhancement><algorithm>StretchToMinimumMaximum</algorithm></contrastEnhancement></rasterrenderer></pipe>".to_string(),
            Source::Picture { .. } => "<pipe><rasterrenderer type=\"multibandcolor\" redBand=\"1\" greenBand=\"2\" blueBand=\"3\" alphaBand=\"4\" opacity=\"1\" nodataColor=\"\"><rasterTransparency/><minMaxOrigin><limits>None</limits><extent>WholeRaster</extent><statAccuracy>Estimated</statAccuracy><cumulativeCutLower>0.02</cumulativeCutLower><cumulativeCutUpper>0.98</cumulativeCutUpper><stdDevFactor>2</stdDevFactor></minMaxOrigin><redContrastEnhancement><minValue>0</minValue><maxValue>255</maxValue><algorithm>NoEnhancement</algorithm></redContrastEnhancement><greenContrastEnhancement><minValue>0</minValue><maxValue>255</maxValue><algorithm>NoEnhancement</algorithm></greenContrastEnhancement><blueContrastEnhancement><minValue>0</minValue><maxValue>255</maxValue><algorithm>NoEnhancement</algorithm></blueContrastEnhancement></rasterrenderer></pipe>".to_string(),
        };
        let geom_attr = if kind == "vector" {
            format!(
                " geometry=\"{}\" wkbType=\"{}\"",
                match geometry {
                    "polygon" => "Polygon",
                    "line" => "Line",
                    _ => "Point",
                },
                match geometry {
                    "polygon" => "MultiPolygon",
                    "line" => "MultiLineString",
                    _ => "Point",
                }
            )
        } else {
            String::new()
        };
        let labels_on = if kind == "vector" && !l.labels.text.trim().is_empty() { " labelsEnabled=\"1\"" } else { "" };
        let _ = write!(
            maps,
            "<maplayer type=\"{}\"{}{} hasScaleBasedVisibilityFlag=\"0\" autoRefreshMode=\"Disabled\" refreshOnNotifyEnabled=\"0\" styleCategories=\"AllStyleCategories\"><id>{}</id><datasource>{}</datasource><layername>{}</layername><srs>{}</srs><provider encoding=\"UTF-8\">{}</provider>{}<blendMode>0</blendMode><layerOpacity>1</layerOpacity></maplayer>",
            kind,
            geom_attr,
            labels_on,
            id,
            esc(&source),
            esc(&l.name),
            crs,
            provider,
            body
        );
    }
    if let Some(b) = &p.basemap {
        let id = "ographis_basemap";
        let url = b.url.replace('{', "%7B").replace('}', "%7D").replace('&', "%26").replace('=', "%3D");
        let source = format!("type=xyz&url={}&zmax=19&zmin=0", url);
        let _ = write!(
            tree,
            "<layer-tree-layer id=\"{}\" name=\"{}\" source=\"{}\" providerKey=\"wms\" checked=\"Qt::Checked\" expanded=\"1\"><customproperties/></layer-tree-layer>",
            id,
            esc(&b.name),
            esc(&source)
        );
        let _ = write!(order, "<item>{}</item>", id);
        let _ = write!(
            maps,
            "<maplayer type=\"raster\" hasScaleBasedVisibilityFlag=\"0\" styleCategories=\"AllStyleCategories\"><id>{}</id><datasource>{}</datasource><layername>{}</layername><srs>{}</srs><provider>wms</provider><pipe><rasterrenderer type=\"singlebandcolordata\" band=\"1\" opacity=\"1\" alphaBand=\"-1\"><rasterTransparency/></rasterrenderer></pipe><blendMode>0</blendMode></maplayer>",
            id,
            esc(&source),
            esc(&b.name),
            srs(3857, None)
        );
    }
    let e = p.extent;
    format!(
        "<!DOCTYPE qgis PUBLIC 'http://mrcc.com/qgis.dtd' 'SYSTEM'>\n<qgis projectname=\"{title}\" version=\"3.34.0-Prizren\">\n<homePath path=\"\"/>\n<title>{title}</title>\n<projectCrs>{crs}</projectCrs>\n<layer-tree-group><customproperties/>{tree}<custom-order enabled=\"0\">{order}</custom-order></layer-tree-group>\n<mapcanvas name=\"theMapCanvas\" annotationsVisible=\"1\"><units>{units}</units><extent><xmin>{x0}</xmin><ymin>{y0}</ymin><xmax>{x1}</xmax><ymax>{y1}</ymax></extent><rotation>0</rotation><destinationsrs>{crs}</destinationsrs><rendermaptile>0</rendermaptile><expressionContextScope/></mapcanvas>\n<projectlayers>{maps}</projectlayers>\n<layerorder>{order}</layerorder>\n<properties><Paths><Absolute type=\"bool\">false</Absolute></Paths><Measurement><DistanceUnits type=\"QString\">meters</DistanceUnits><AreaUnits type=\"QString\">m2</AreaUnits></Measurement></properties>\n<ProjectViewSettings UseProjectScales=\"0\"><Scales/><DefaultViewExtent xmin=\"{x0}\" ymin=\"{y0}\" xmax=\"{x1}\" ymax=\"{y1}\">{crs}</DefaultViewExtent></ProjectViewSettings>\n</qgis>\n",
        title = esc(&p.title),
        crs = crs,
        tree = tree,
        order = order,
        maps = maps,
        units = if p.crs == 4326 { "degrees" } else { "meters" },
        x0 = e[0],
        y0 = e[1],
        x1 = e[2],
        y1 = e[3]
    )
}

/// Writes the project as .qgz: the project file zipped, as QGIS saves it, or as .qgs when
/// that is the name.
pub fn write_project(p: &Project, path: &Path) -> Result<()> {
    let xml = project_xml(p);
    let fail = |e: String| GbError(format!("Cannot write {}: {}", path.display(), e));
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("qgs")) {
        return std::fs::write(path, xml).map_err(|e| fail(e.to_string()));
    }
    let file = std::fs::File::create(path).map_err(|e| fail(e.to_string()))?;
    let mut z = zip::ZipWriter::new(file);
    let name = format!("{}.qgs", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into()));
    z.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).map_err(|e| fail(e.to_string()))?;
    z.write_all(xml.as_bytes()).map_err(|e| fail(e.to_string()))?;
    z.finish().map_err(|e| fail(e.to_string()))?;
    Ok(())
}
