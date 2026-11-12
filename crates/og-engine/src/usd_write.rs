//! OpenUSD as text (.usda), and packed with its textures (.usdz), for Houdini, Blender,
//! NVIDIA Omniverse and the 3D viewers of Apple. Each feature is an object with its
//! attributes. The objects can be put in layers by an attribute, each a file of its own that
//! the main file takes in as a sublayer, and in variants by another, such as the plan and
//! what stands today, to switch between. The coordinate system and the origin the model is
//! written around are kept in the metadata of the main file. Z is up, in metres.

use crate::mesh::{Material, Mesh, NONE};
use crate::model::{err, GbError, Result, Table, Value};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// How a stream is written as USD.
#[derive(Debug, Clone, Default)]
pub struct UsdOpts {
    /// The attribute whose values become layers.
    pub layer_by: Option<usize>,
    /// The attribute whose values become variants.
    pub variant_by: Option<usize>,
    /// The origin the model is written around; by default near its middle.
    pub origin: Option<[f64; 3]>,
}

/// A name that USD takes for a prim: letters, digits and _, not starting with a digit.
fn prim_name(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| match c {
            'å' | 'ä' | 'Å' | 'Ä' => 'a',
            'ö' | 'Ö' => 'o',
            c if c.is_ascii_alphanumeric() || c == '_' => c,
            _ => '_',
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// A name that USD takes for a variant: letters, digits, _ and -, also first.
fn variant_name(s: &str) -> String {
    let out: String = s
        .trim()
        .chars()
        .map(|c| match c {
            'å' | 'ä' | 'Å' | 'Ä' => 'a',
            'ö' | 'Ö' => 'o',
            c if c.is_ascii_alphanumeric() || c == '_' || c == '-' => c,
            _ => '_',
        })
        .collect();
    if out.is_empty() {
        "_".into()
    } else {
        out
    }
}

/// Names that are each given only once, among the children of one prim.
#[derive(Default)]
struct Names(HashMap<String, usize>);

impl Names {
    fn unique(&mut self, wanted: &str) -> String {
        let base = prim_name(wanted);
        let n = self.0.entry(base.clone()).or_insert(0);
        *n += 1;
        if *n == 1 {
            base
        } else {
            format!("{}_{}", base, n)
        }
    }
}

fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn num(v: f64) -> String {
    if v.is_finite() {
        let r = (v * 1000.0).round() / 1000.0;
        if r == r.trunc() && r.abs() < 1e15 {
            format!("{}", r as i64)
        } else {
            format!("{}", r)
        }
    } else {
        "0".into()
    }
}

fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The parts of the stream to write: for each object, its triangles by material, or its
/// lines and points when there is no 3D model.
enum Shape<'a> {
    Mesh(&'a Mesh),
    Loose(&'a Table),
}

/// Writes the stream as USD at `path`, .usda or .usdz. Gives the files written.
pub fn write_usd(table: &Table, path: &Path, opts: &UsdOpts) -> Result<Vec<PathBuf>> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let packed = ext == "usdz";
    let shape = match table.mesh.as_deref() {
        Some(m) => Shape::Mesh(m),
        None if table.has_geometry() => Shape::Loose(table),
        None => return err("The stream has no 3D model, lines or points to write as USD."),
    };
    let origin = opts.origin.unwrap_or_else(|| match &shape {
        Shape::Mesh(m) => crate::mesh_write::origin_of(m),
        Shape::Loose(t) => loose_origin(t),
    });
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_string();
    // The files of the model: (name inside the folder or package, contents).
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let materials: Vec<Material> = match &shape {
        Shape::Mesh(m) => m.materials.clone(),
        Shape::Loose(_) => vec![Material { name: "lines".into(), diffuse: [0.15, 0.15, 0.15], ..Default::default() }],
    };
    let mut looks = String::new();
    let _ = writeln!(looks, "    def Scope \"Looks\"\n    {{");
    for (i, m) in materials.iter().enumerate() {
        let texture = m.texture.as_ref().map(|t| {
            let name = format!("textures/{}_{}.png", prim_name(&stem), i);
            let mut png = Vec::new();
            if let Some(img) = image::RgbaImage::from_raw(t.width as u32, t.height as u32, t.rgba.clone()) {
                let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            }
            files.push((name.clone(), png));
            name
        });
        looks.push_str(&material_usda(i, m, texture.as_deref()));
    }
    looks.push_str("    }\n");

    // Which object goes in which layer and which variant.
    let n = match &shape {
        Shape::Mesh(m) => m.objects.len().max(table.len()),
        Shape::Loose(t) => t.len(),
    };
    let text_of = |at: Option<usize>, k: usize| -> String { at.and_then(|i| table.features.get(k).map(|f| f.attrs[i].to_text())).unwrap_or_default() };
    let mut layers: Vec<(String, Vec<usize>)> = Vec::new();
    for k in 0..n {
        let layer = if opts.layer_by.is_some() { text_of(opts.layer_by, k) } else { String::new() };
        let layer = if opts.layer_by.is_some() && layer.trim().is_empty() { "other".to_string() } else { layer };
        match layers.iter_mut().find(|(l, _)| *l == layer) {
            Some((_, list)) => list.push(k),
            None => layers.push((layer, vec![k])),
        }
    }
    // The triangles of each object, by material.
    let mut by_object: Vec<Vec<usize>> = vec![Vec::new(); n];
    if let Shape::Mesh(m) = &shape {
        for (i, t) in m.tris.iter().enumerate() {
            if let Some(list) = by_object.get_mut(t.object as usize) {
                list.push(i);
            }
        }
    }
    let variant_set = opts.variant_by.and_then(|i| table.fields.get(i)).map(|f| prim_name(&f.name));
    let object_text = |k: usize, names: &mut Names, indent: usize| -> String {
        let label = match &shape {
            Shape::Mesh(m) => m.objects.get(k).cloned().filter(|o| !o.is_empty()).unwrap_or_else(|| format!("object_{}", k + 1)),
            Shape::Loose(_) => format!("feature_{}", k + 1),
        };
        let name = names.unique(&label);
        let body = match &shape {
            Shape::Mesh(m) => mesh_parts(m, &by_object[k], origin, indent + 4),
            Shape::Loose(t) => t.features.get(k).map(|f| loose_parts(f, origin, indent + 4)).unwrap_or_default(),
        };
        if body.is_empty() {
            return String::new();
        }
        let pad = " ".repeat(indent);
        let mut s = format!("{pad}def Xform {}\n{pad}{{\n", quoted(&name));
        let _ = writeln!(s, "{pad}    custom string ographis:name = {}", quoted(&label));
        if let Some(f) = table.features.get(k) {
            for (field, v) in table.fields.iter().zip(&f.attrs) {
                let key = format!("ographis:{}", prim_name(&field.name));
                match v {
                    Value::Null => {}
                    Value::Int(i) => {
                        let _ = writeln!(s, "{pad}    custom int64 {} = {}", key, i);
                    }
                    Value::Real(r) if r.is_finite() => {
                        let _ = writeln!(s, "{pad}    custom double {} = {}", key, r);
                    }
                    Value::Bool(b) => {
                        let _ = writeln!(s, "{pad}    custom bool {} = {}", key, b);
                    }
                    other => {
                        let _ = writeln!(s, "{pad}    custom string {} = {}", key, quoted(&other.to_text()));
                    }
                }
            }
        }
        s.push_str(&body);
        let _ = writeln!(s, "{pad}}}");
        s
    };
    // The objects of a layer, in variants when asked.
    let group_text = |objects: &[usize], indent: usize| -> String {
        let pad = " ".repeat(indent);
        let mut names = Names::default();
        let Some(set) = &variant_set else {
            return objects.iter().map(|k| object_text(*k, &mut names, indent)).collect();
        };
        let mut always = String::new();
        let mut variants: Vec<(String, String)> = Vec::new();
        for k in objects {
            let v = text_of(opts.variant_by, *k);
            if v.trim().is_empty() {
                always.push_str(&object_text(*k, &mut names, indent));
                continue;
            }
            let v = variant_name(&v);
            let at = match variants.iter().position(|(name, _)| *name == v) {
                Some(at) => at,
                None => {
                    variants.push((v, String::new()));
                    variants.len() - 1
                }
            };
            let text = object_text(*k, &mut names, indent + 8);
            variants[at].1.push_str(&text);
        }
        if variants.is_empty() {
            return always;
        }
        let mut s = always;
        let _ = writeln!(s, "{pad}def Xform \"variants\" (\n{pad}    variants = {{\n{pad}        string {set} = {}\n{pad}    }}\n{pad}    prepend variantSets = {}\n{pad})\n{pad}{{", quoted(&variants[0].0), quoted(set));
        let _ = writeln!(s, "{pad}    variantSet {} = {{", quoted(set));
        for (name, body) in &variants {
            let _ = writeln!(s, "{pad}        {} {{\n{body}{pad}        }}", quoted(name));
        }
        let _ = writeln!(s, "{pad}    }}\n{pad}}}");
        s
    };

    let header = |default_prim: bool, sublayers: &[String], meta: bool| -> String {
        let mut h = String::from("#usda 1.0\n(\n");
        if default_prim {
            h.push_str("    defaultPrim = \"World\"\n");
        }
        h.push_str("    metersPerUnit = 1\n    upAxis = \"Z\"\n");
        if meta {
            let _ = writeln!(
                h,
                "    doc = \"Written by Ographis {}. Z up, in metres from the origin in customLayerData, in the coordinate system there.\"",
                env!("CARGO_PKG_VERSION")
            );
            h.push_str("    customLayerData = {\n");
            if let Some(c) = table.crs {
                let _ = writeln!(h, "        string crs = \"EPSG:{}\"", c);
            }
            let _ = writeln!(h, "        double3 crsOrigin = ({}, {}, {})", num(origin[0]), num(origin[1]), num(origin[2]));
            let _ = writeln!(h, "        string creator = \"Ographis {}\"", env!("CARGO_PKG_VERSION"));
            h.push_str("    }\n");
        }
        if !sublayers.is_empty() {
            h.push_str("    subLayers = [\n");
            let list: Vec<String> = sublayers.iter().map(|l| format!("        @./{}@", l)).collect();
            h.push_str(&list.join(",\n"));
            h.push_str("\n    ]\n");
        }
        h.push_str(")\n\n");
        h
    };
    let geo_note = table.crs.map(|c| format!("    custom string ographis:crs = \"EPSG:{}\"\n", c)).unwrap_or_default();
    let world_open = format!(
        "def Xform \"World\" (\n    kind = \"assembly\"\n)\n{{\n{geo_note}    custom double3 ographis:crsOrigin = ({}, {}, {})\n",
        num(origin[0]),
        num(origin[1]),
        num(origin[2])
    );
    let mut main = String::new();
    if opts.layer_by.is_some() {
        // A file for each layer, taken in by the main file.
        let mut names = Names::default();
        let mut subs = Vec::new();
        for (layer, objects) in &layers {
            let name = names.unique(layer);
            let file = format!("{}_layers/{}.usda", prim_name(&stem), name);
            let mut text = header(false, &[], false);
            let _ = writeln!(text, "over \"World\"\n{{\n    def Xform {}\n    {{", quoted(&name));
            text.push_str(&group_text(objects, 8));
            text.push_str("    }\n}\n");
            files.push((file.clone(), text.into_bytes()));
            subs.push(file);
        }
        // The strongest layer comes first in USD; keep the order they came in.
        main.push_str(&header(true, &subs, true));
        main.push_str(&world_open);
        main.push_str(&looks);
        main.push_str("}\n");
    } else {
        main.push_str(&header(true, &[], true));
        main.push_str(&world_open);
        main.push_str(&looks);
        let all: Vec<usize> = (0..n).collect();
        main.push_str(&group_text(&all, 4));
        main.push_str("}\n");
    }
    let main_name = format!("{}.usda", stem);
    files.insert(0, (main_name, main.into_bytes()));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    if packed {
        write_usdz(path, &files)?;
        return Ok(vec![path.to_path_buf()]);
    }
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut written = Vec::new();
    for (k, (name, bytes)) in files.iter().enumerate() {
        let to = if k == 0 { path.to_path_buf() } else { dir.join(name) };
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| GbError(format!("Cannot create the folder {}: {}", d.display(), e)))?;
        }
        std::fs::write(&to, bytes).map_err(|e| GbError(format!("Cannot write {}: {}", to.display(), e)))?;
        written.push(to);
    }
    Ok(written)
}

/// A package of USD: a zip that is not compressed, with each file beginning at a multiple
/// of 64 bytes, the main layer first.
fn write_usdz(path: &Path, files: &[(String, Vec<u8>)]) -> Result<()> {
    use std::io::Write;
    let file = std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).with_alignment(64);
    for (name, bytes) in files {
        zip.start_file(name.as_str(), options).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        zip.write_all(bytes).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    }
    zip.finish().map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    Ok(())
}

/// A material as UsdPreviewSurface, with its texture when it has one.
fn material_usda(i: usize, m: &Material, texture: Option<&str>) -> String {
    let name = format!("material_{}", i);
    let c = m.diffuse.map(linear);
    let e = m.emission.map(linear);
    let rough = (1.0 - (m.specular.iter().sum::<f32>() / 3.0) * 0.8).clamp(0.05, 1.0);
    let mut s = String::new();
    let _ = writeln!(s, "        def Material {}\n        {{", quoted(&name));
    if !m.name.is_empty() {
        let _ = writeln!(s, "            custom string ographis:name = {}", quoted(&m.name));
    }
    let _ = writeln!(s, "            token outputs:surface.connect = </World/Looks/{}/surface.outputs:surface>", name);
    let _ = writeln!(s, "            def Shader \"surface\"\n            {{\n                uniform token info:id = \"UsdPreviewSurface\"");
    match texture {
        Some(_) => {
            let _ = writeln!(s, "                color3f inputs:diffuseColor.connect = </World/Looks/{}/texture.outputs:rgb>", name);
        }
        None => {
            let _ = writeln!(s, "                color3f inputs:diffuseColor = ({}, {}, {})", c[0], c[1], c[2]);
        }
    }
    let _ = writeln!(s, "                float inputs:roughness = {}\n                float inputs:metallic = 0", rough);
    if m.opacity < 1.0 {
        let _ = writeln!(s, "                float inputs:opacity = {}", m.opacity.clamp(0.0, 1.0));
    }
    if e != [0.0; 3] {
        let _ = writeln!(s, "                color3f inputs:emissiveColor = ({}, {}, {})", e[0], e[1], e[2]);
    }
    let _ = writeln!(s, "                token outputs:surface\n            }}");
    if let Some(t) = texture {
        let _ = writeln!(
            s,
            "            def Shader \"st\"\n            {{\n                uniform token info:id = \"UsdPrimvarReader_float2\"\n                string inputs:varname = \"st\"\n                float2 outputs:result\n            }}"
        );
        let _ = writeln!(
            s,
            "            def Shader \"texture\"\n            {{\n                uniform token info:id = \"UsdUVTexture\"\n                asset inputs:file = @./{}@\n                float2 inputs:st.connect = </World/Looks/{}/st.outputs:result>\n                token inputs:wrapS = \"repeat\"\n                token inputs:wrapT = \"repeat\"\n                float3 outputs:rgb\n            }}",
            t, name
        );
    }
    let _ = writeln!(s, "        }}");
    s
}

fn joined<T>(items: impl Iterator<Item = T>, f: impl Fn(T) -> String) -> String {
    items.map(f).collect::<Vec<_>>().join(", ")
}

/// The meshes of one object, one for each of its materials.
fn mesh_parts(m: &Mesh, tris: &[usize], o: [f64; 3], indent: usize) -> String {
    let pad = " ".repeat(indent);
    let mut by_mat: Vec<(u32, Vec<usize>)> = Vec::new();
    for t in tris {
        let mat = m.tris[*t].material;
        match by_mat.iter_mut().find(|(k, _)| *k == mat) {
            Some((_, l)) => l.push(*t),
            None => by_mat.push((mat, vec![*t])),
        }
    }
    let mut s = String::new();
    for (part, (mat, list)) in by_mat.iter().enumerate() {
        // The corners of these triangles, each once.
        let mut map: HashMap<u32, usize> = HashMap::new();
        let mut points: Vec<[f64; 3]> = Vec::new();
        let mut index: Vec<usize> = Vec::with_capacity(list.len() * 3);
        for t in list {
            for v in m.tris[*t].v {
                let at = *map.entry(v).or_insert_with(|| {
                    points.push(m.positions[v as usize]);
                    points.len() - 1
                });
                index.push(at);
            }
        }
        let normals = list.iter().all(|t| m.tris[*t].n.iter().all(|n| *n != NONE && (*n as usize) < m.normals.len()));
        let uvs = list.iter().all(|t| m.tris[*t].t.iter().all(|n| *n != NONE && (*n as usize) < m.uvs.len()));
        let _ = writeln!(s, "{pad}def Mesh \"part_{}\" (\n{pad}    prepend apiSchemas = [\"MaterialBindingAPI\"]\n{pad})\n{pad}{{", part + 1);
        let _ = writeln!(s, "{pad}    uniform bool doubleSided = 1\n{pad}    uniform token subdivisionScheme = \"none\"");
        let _ = writeln!(s, "{pad}    int[] faceVertexCounts = [{}]", vec!["3"; list.len()].join(", "));
        let _ = writeln!(s, "{pad}    int[] faceVertexIndices = [{}]", list_of(index.iter(), |i| i.to_string()));
        let _ = writeln!(
            s,
            "{pad}    point3f[] points = [{}]",
            joined(points.iter(), |p: &[f64; 3]| format!("({}, {}, {})", num(p[0] - o[0]), num(p[1] - o[1]), num(p[2] - o[2])))
        );
        if normals {
            let all = list.iter().flat_map(|t| m.tris[*t].n.map(|n| m.normals[n as usize]));
            let _ = writeln!(
                s,
                "{pad}    normal3f[] normals = [{}] (\n{pad}        interpolation = \"faceVarying\"\n{pad}    )",
                joined(all, |n: [f32; 3]| format!("({}, {}, {})", n[0], n[1], n[2]))
            );
        }
        if uvs {
            let all = list.iter().flat_map(|t| m.tris[*t].t.map(|n| m.uvs[n as usize]));
            let _ = writeln!(
                s,
                "{pad}    texCoord2f[] primvars:st = [{}] (\n{pad}        interpolation = \"faceVarying\"\n{pad}    )",
                joined(all, |u: [f32; 2]| format!("({}, {})", u[0], u[1]))
            );
        }
        if let Some(mm) = m.materials.get(*mat as usize) {
            let c = mm.diffuse.map(linear);
            let _ = writeln!(s, "{pad}    color3f[] primvars:displayColor = [({}, {}, {})]", c[0], c[1], c[2]);
        }
        let _ = writeln!(s, "{pad}    rel material:binding = </World/Looks/material_{}>", if (*mat as usize) < m.materials.len() { *mat } else { 0 });
        let _ = writeln!(s, "{pad}}}");
    }
    s
}

fn list_of<'a>(items: impl Iterator<Item = &'a usize>, f: impl Fn(&usize) -> String) -> String {
    items.map(f).collect::<Vec<_>>().join(", ")
}

/// The middle of the lines and points of a stream, in whole metres, at the lowest height.
fn loose_origin(t: &Table) -> [f64; 3] {
    use geo::BoundingRect;
    let mut b: Option<geo::Rect<f64>> = None;
    for g in t.features.iter().filter_map(|f| f.geom.as_ref()) {
        if let Some(r) = g.bounding_rect() {
            b = Some(match b {
                Some(a) => geo::Rect::new(
                    geo::coord! { x: a.min().x.min(r.min().x), y: a.min().y.min(r.min().y) },
                    geo::coord! { x: a.max().x.max(r.max().x), y: a.max().y.max(r.max().y) },
                ),
                None => r,
            });
        }
    }
    let low = t.features.iter().filter_map(|f| f.z.as_ref()).flat_map(|z| z.iter().copied()).filter(|z| z.is_finite()).fold(f64::INFINITY, f64::min);
    match b {
        Some(r) => [r.center().x.round(), r.center().y.round(), if low.is_finite() { low.floor() } else { 0.0 }],
        None => [0.0; 3],
    }
}

/// The lines and points of a feature without a 3D model, as curves and points.
fn loose_parts(f: &crate::model::Feature, o: [f64; 3], indent: usize) -> String {
    let Some(g) = &f.geom else { return String::new() };
    let pad = " ".repeat(indent);
    let z = |i: usize| f.z.as_ref().and_then(|z| z.get(i).copied()).filter(|v| v.is_finite()).unwrap_or(0.0);
    let mut lines: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut i = 0usize;
    let mut line = |l: &geo::LineString<f64>, i: &mut usize| {
        let pts: Vec<[f64; 3]> =
            l.0.iter()
                .map(|c| {
                    let p = [c.x, c.y, z(*i)];
                    *i += 1;
                    p
                })
                .collect();
        if pts.len() >= 2 {
            lines.push(pts);
        }
    };
    match g {
        geo::Geometry::Point(p) => points.push([p.x(), p.y(), z(0)]),
        geo::Geometry::MultiPoint(m) => points.extend(m.0.iter().enumerate().map(|(j, p)| [p.x(), p.y(), z(j)])),
        geo::Geometry::LineString(l) => line(l, &mut i),
        geo::Geometry::MultiLineString(m) => m.0.iter().for_each(|l| line(l, &mut i)),
        geo::Geometry::Polygon(p) => {
            line(p.exterior(), &mut i);
            p.interiors().iter().for_each(|r| line(r, &mut i));
        }
        geo::Geometry::MultiPolygon(m) => m.0.iter().for_each(|p| {
            line(p.exterior(), &mut i);
            p.interiors().iter().for_each(|r| line(r, &mut i));
        }),
        _ => {}
    }
    let at = |p: &[f64; 3]| format!("({}, {}, {})", num(p[0] - o[0]), num(p[1] - o[1]), num(p[2] - o[2]));
    let mut s = String::new();
    if !lines.is_empty() {
        let _ = writeln!(s, "{pad}def BasisCurves \"lines\" (\n{pad}    prepend apiSchemas = [\"MaterialBindingAPI\"]\n{pad})\n{pad}{{");
        let _ = writeln!(s, "{pad}    uniform token type = \"linear\"");
        let _ = writeln!(s, "{pad}    int[] curveVertexCounts = [{}]", joined(lines.iter(), |l: &Vec<[f64; 3]>| l.len().to_string()));
        let _ = writeln!(s, "{pad}    point3f[] points = [{}]", joined(lines.iter().flatten(), at));
        let _ = writeln!(s, "{pad}    float[] widths = [0.5] (\n{pad}        interpolation = \"constant\"\n{pad}    )");
        let _ = writeln!(s, "{pad}    rel material:binding = </World/Looks/material_0>\n{pad}}}");
    }
    if !points.is_empty() {
        let _ = writeln!(s, "{pad}def Points \"points\"\n{pad}{{");
        let _ = writeln!(s, "{pad}    point3f[] points = [{}]", joined(points.iter(), at));
        let _ = writeln!(s, "{pad}    float[] widths = [1] (\n{pad}        interpolation = \"constant\"\n{pad}    )\n{pad}}}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Feature, Field, FieldType};

    fn two_houses() -> Table {
        // Two boxes of one triangle each are enough to see where they go.
        let mesh = Mesh {
            positions: vec![[100.0, 200.0, 10.0], [101.0, 200.0, 10.0], [100.0, 201.0, 10.0], [110.0, 200.0, 10.0], [111.0, 200.0, 10.0], [110.0, 201.0, 12.0]],
            tris: vec![
                crate::mesh::Tri { v: [0, 1, 2], n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 },
                crate::mesh::Tri { v: [3, 4, 5], n: [NONE; 3], t: [NONE; 3], material: 0, object: 1 },
            ],
            materials: vec![Material { name: "roof".into(), diffuse: [0.8, 0.2, 0.1], ..Default::default() }],
            objects: vec!["Hus 1".into(), "Hus 2".into()],
            ..Default::default()
        };
        Table {
            fields: vec![Field::new("kind", FieldType::Text), Field::new("scenario", FieldType::Text), Field::new("floors", FieldType::Int)],
            features: vec![
                Feature { z: None, attrs: vec![Value::Text("buildings".into()), Value::Text("today".into()), Value::Int(2)], geom: None },
                Feature { z: None, attrs: vec![Value::Text("buildings".into()), Value::Text("plan".into()), Value::Int(5)], geom: None },
            ],
            crs: Some(3006),
            mesh: Some(std::sync::Arc::new(mesh)),
            ..Table::default()
        }
    }

    #[test]
    fn a_model_is_written_as_usd_with_layers_variants_and_its_place() {
        let dir = std::env::temp_dir().join(format!("og-usd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let t = two_houses();
        let path = dir.join("town.usda");
        let files = write_usd(&t, &path, &UsdOpts { layer_by: Some(0), variant_by: Some(1), origin: Some([100.0, 200.0, 0.0]) }).unwrap();
        assert_eq!(files.len(), 2);
        let main = std::fs::read_to_string(&path).unwrap();
        assert!(main.starts_with("#usda 1.0"));
        assert!(main.contains("string crs = \"EPSG:3006\"") && main.contains("double3 crsOrigin = (100, 200, 0)"), "{}", main);
        assert!(main.contains("@./town_layers/buildings.usda@") && main.contains("upAxis = \"Z\""));
        let layer = std::fs::read_to_string(dir.join("town_layers/buildings.usda")).unwrap();
        assert!(layer.contains("variantSet \"scenario\"") && layer.contains("\"plan\" {") && layer.contains("string scenario = \"today\""), "{}", layer);
        assert!(layer.contains("def Xform \"Hus_1\"") && layer.contains("custom int64 ographis:floors = 5"));
        // Around the origin: the second house begins 10 m east.
        assert!(layer.contains("point3f[] points = [(10, 0, 10), (11, 0, 10), (10, 1, 12)]"), "{}", layer);
        // Packed: a zip that is not compressed, the main layer first, at 64 bytes.
        let z = dir.join("town.usdz");
        write_usd(&t, &z, &UsdOpts::default()).unwrap();
        let bytes = std::fs::read(&z).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let first = archive.by_index(0).unwrap();
        assert_eq!((first.name(), first.compression()), ("town.usda", zip::CompressionMethod::Stored));
        assert_eq!(first.data_start().unwrap() % 64, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
