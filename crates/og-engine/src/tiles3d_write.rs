//! Writes the 3D model of a stream as 3D Tiles 1.1: a folder with tileset.json and the
//! models as glTF (.glb), as CesiumJS, Cesium ion, deck.gl and the 3D views of QGIS and
//! ArcGIS show them on the globe. The objects are split into tiles of a size that loads
//! quickly, and the tileset is placed on the globe where the model lies: an east, north and
//! up frame at its middle, turned as the grid of its coordinate system is turned.

use crate::io::Backend;
use crate::mesh::Mesh;
use crate::mesh_write::{write_glb_with, GltfOpts};
use crate::model::{GbError, Result, Table};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How a tileset is written.
pub struct TilesOpts {
    /// At most this many triangles in a tile.
    pub most: usize,
    /// Added to every height: such as the height of the geoid above the ellipsoid, about
    /// 20 to 40 metres in Sweden, as the globe of Cesium has ellipsoid heights.
    pub dz: f64,
    /// Coarser tiles further away: each tile of tiles also has a lighter model of what they
    /// hold, with fewer triangles and without the smallest objects, shown until one comes near.
    pub lod: bool,
    /// The models packed by meshoptimizer.
    pub compress: bool,
}

/// A part of the model: some objects, or some of the triangles of one object.
#[derive(Clone)]
struct Part {
    objects: Vec<usize>,
    tris: Option<Vec<usize>>,
}

/// The triangles of each object.
fn tris_of(mesh: &Mesh) -> Vec<Vec<usize>> {
    let mut by = vec![Vec::new(); mesh.objects.len().max(1)];
    for (i, t) in mesh.tris.iter().enumerate() {
        let o = if mesh.objects.is_empty() { 0 } else { t.object as usize };
        if let Some(v) = by.get_mut(o) {
            v.push(i);
        }
    }
    by
}

/// The middle of a triangle, east and north.
fn middle(mesh: &Mesh, i: usize) -> [f64; 2] {
    let t = &mesh.tris[i];
    let p = t.v.map(|k| mesh.positions[k as usize]);
    [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0]
}

/// A part in two, by the middles of its objects, or of its triangles when it is one object,
/// along the longer side.
fn halve(mesh: &Mesh, by: &[Vec<usize>], part: &Part) -> (Part, Part) {
    let tris: Vec<usize> = match &part.tris {
        Some(t) => t.clone(),
        None if part.objects.len() == 1 => by[part.objects[0]].clone(),
        None => Vec::new(),
    };
    if !tris.is_empty() {
        let mids: Vec<[f64; 2]> = tris.iter().map(|i| middle(mesh, *i)).collect();
        let axis = axis_of(&mids);
        let mut order: Vec<usize> = (0..tris.len()).collect();
        order.sort_by(|a, b| mids[*a][axis].total_cmp(&mids[*b][axis]));
        let half = order.len() / 2;
        let pick = |r: &[usize]| Part { objects: part.objects.clone(), tris: Some(r.iter().map(|k| tris[*k]).collect()) };
        return (pick(&order[..half]), pick(&order[half..]));
    }
    let mids: Vec<[f64; 2]> = part
        .objects
        .iter()
        .map(|o| {
            let (mut a, mut b) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for i in &by[*o] {
                let m = middle(mesh, *i);
                a = [a[0].min(m[0]), a[1].min(m[1])];
                b = [b[0].max(m[0]), b[1].max(m[1])];
            }
            [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
        })
        .collect();
    // An object with more than half of the triangles goes by itself; else the objects are
    // halved where half of the triangles lie on each side.
    let total: usize = part.objects.iter().map(|o| by[*o].len()).sum();
    if let Some(big) = part.objects.iter().position(|o| by[*o].len() * 2 > total) {
        let rest: Vec<usize> = part.objects.iter().enumerate().filter(|(k, _)| *k != big).map(|(_, o)| *o).collect();
        return (Part { objects: vec![part.objects[big]], tris: None }, Part { objects: rest, tris: None });
    }
    let axis = axis_of(&mids);
    let mut order: Vec<usize> = (0..part.objects.len()).collect();
    order.sort_by(|a, b| mids[*a][axis].total_cmp(&mids[*b][axis]));
    let (mut sum, mut half) = (0usize, 0usize);
    for (k, i) in order.iter().enumerate() {
        if sum * 2 >= total {
            break;
        }
        sum += by[part.objects[*i]].len();
        half = k + 1;
    }
    let half = half.clamp(1, order.len() - 1);
    let pick = |r: &[usize]| Part { objects: r.iter().map(|k| part.objects[*k]).collect(), tris: None };
    (pick(&order[..half]), pick(&order[half..]))
}

fn axis_of(mids: &[[f64; 2]]) -> usize {
    let (mut a, mut b) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for m in mids {
        a = [a[0].min(m[0]), a[1].min(m[1])];
        b = [b[0].max(m[0]), b[1].max(m[1])];
    }
    if b[0] - a[0] >= b[1] - a[1] {
        0
    } else {
        1
    }
}

/// Some of the triangles of one object, as a model of that object alone with only the
/// corners they use.
fn some_triangles(mesh: &Mesh, tris: &[usize], object: usize) -> Mesh {
    use crate::mesh::{Tri, NONE};
    let mut out = Mesh { materials: mesh.materials.clone(), objects: mesh.objects.get(object).cloned().into_iter().collect(), ..Default::default() };
    let (mut pos, mut nor, mut uv) = (std::collections::HashMap::new(), std::collections::HashMap::new(), std::collections::HashMap::new());
    for &i in tris {
        let t = mesh.tris[i];
        let v = t.v.map(|k| {
            *pos.entry(k).or_insert_with(|| {
                out.positions.push(mesh.positions[k as usize]);
                (out.positions.len() - 1) as u32
            })
        });
        let n = t.n.map(|k| {
            if k == NONE || k as usize >= mesh.normals.len() {
                return NONE;
            }
            *nor.entry(k).or_insert_with(|| {
                out.normals.push(mesh.normals[k as usize]);
                (out.normals.len() - 1) as u32
            })
        });
        let tx = t.t.map(|k| {
            if k == NONE || k as usize >= mesh.uvs.len() {
                return NONE;
            }
            *uv.entry(k).or_insert_with(|| {
                out.uvs.push(mesh.uvs[k as usize]);
                (out.uvs.len() - 1) as u32
            })
        });
        out.tris.push(Tri { v, n, t: tx, material: t.material, object: 0 });
    }
    out
}

/// The stream of a part: its features and its model.
fn part_table(table: &Table, mesh: &Mesh, part: &Part) -> Table {
    let mut t = table.like();
    t.fields = table.fields.clone();
    match &part.tris {
        None if table.features.is_empty() && part.objects.len() <= 1 => return table.clone(),
        None => {
            t.features = part.objects.iter().filter_map(|i| table.features.get(*i).cloned()).collect();
            t.mesh = Some(Arc::new(mesh.pick(&part.objects)));
        }
        Some(tris) => {
            // One object, some of its triangles.
            let o = part.objects.first().copied().unwrap_or(0);
            t.features = table.features.get(o).cloned().into_iter().collect();
            t.mesh = Some(Arc::new(some_triangles(mesh, tris, o)));
        }
    }
    t
}

/// A point in degrees and metres above the ellipsoid, in earth-centred coordinates
/// (EPSG:4978), on GRS 80.
fn ecef(lon: f64, lat: f64, h: f64) -> [f64; 3] {
    let (a, f) = (6_378_137.0, 1.0 / 298.257_222_101);
    let e2 = f * (2.0 - f);
    let (lo, la) = (lon.to_radians(), lat.to_radians());
    let n = a / (1.0 - e2 * la.sin().powi(2)).sqrt();
    [(n + h) * la.cos() * lo.cos(), (n + h) * la.cos() * lo.sin(), (n * (1.0 - e2) + h) * la.sin()]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The transform of the tileset, by columns: the x, y and up of the coordinate system of the
/// model at `origin`, in earth-centred coordinates.
fn frame(origin: [f64; 3], crs: u32, backend: &dyn Backend) -> Result<Vec<f64>> {
    let (mut xs, mut ys) = (vec![origin[0], origin[0] + 100.0, origin[0]], vec![origin[1], origin[1], origin[1] + 100.0]);
    if crs != 4326 {
        backend.transform(crs, 4326, &mut xs, &mut ys)?;
    } else {
        return Err(GbError("The model is in degrees. Reproject it to metres, such as SWEREF 99 TM, before it is written as 3D Tiles.".into()));
    }
    let o = ecef(xs[0], ys[0], origin[2]);
    let up = norm(sub(ecef(xs[0], ys[0], origin[2] + 1.0), o));
    // The grid's x and y, laid flat on the ground there.
    let flat = |p: [f64; 3]| {
        let d = sub(p, o);
        norm(sub(d, [up[0] * dot(d, up), up[1] * dot(d, up), up[2] * dot(d, up)]))
    };
    let ex = flat(ecef(xs[1], ys[1], origin[2]));
    let ny = norm(cross(up, ex));
    Ok(vec![ex[0], ex[1], ex[2], 0.0, ny[0], ny[1], ny[2], 0.0, up[0], up[1], up[2], 0.0, o[0], o[1], o[2], 1.0])
}

/// The box around a model in the frame of the tileset, as 3D Tiles has it: the middle and
/// three half axes.
fn bounding_box(m: &Mesh, origin: [f64; 3]) -> Option<(Vec<f64>, f64)> {
    let (lo, hi) = m.bounds()?;
    let c = [(lo[0] + hi[0]) / 2.0 - origin[0], (lo[1] + hi[1]) / 2.0 - origin[1], (lo[2] + hi[2]) / 2.0 - origin[2]];
    let h = [((hi[0] - lo[0]) / 2.0).max(0.5), ((hi[1] - lo[1]) / 2.0).max(0.5), ((hi[2] - lo[2]) / 2.0).max(0.5)];
    let diag = 2.0 * dot(h, h).sqrt();
    Some((vec![c[0], c[1], c[2], h[0], 0.0, 0.0, 0.0, h[1], 0.0, 0.0, 0.0, h[2]], diag))
}

/// Writes the tileset into `folder`: tileset.json and the tiles in tiles/. Gives the files and
/// how many tiles.
pub fn write_tileset(table: &Table, folder: &Path, o: &TilesOpts, backend: &dyn Backend) -> Result<(Vec<PathBuf>, usize)> {
    let Some(mesh) = table.mesh.as_deref() else {
        return Err(GbError(
            "The stream has no 3D model to write as 3D Tiles. Make one with Extrude, LoD2 buildings from points or Terrain to 3D model.".into(),
        ));
    };
    let crs = table.crs.ok_or_else(|| GbError("The model has no coordinate system, so it cannot be placed on the globe. Set it on the input.".into()))?;
    let (lo, hi) = mesh.bounds().ok_or_else(|| GbError("The 3D model is empty.".into()))?;
    let origin = [((lo[0] + hi[0]) / 2.0).round(), ((lo[1] + hi[1]) / 2.0).round(), 0.0];
    let transform = frame([origin[0], origin[1], o.dz], crs, backend)?;
    let tiles_dir = folder.join("tiles");
    std::fs::create_dir_all(&tiles_dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", tiles_dir.display(), e)))?;
    // Tiles left from an earlier writing go first.
    if let Ok(entries) = std::fs::read_dir(&tiles_dir) {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|x| x == "glb") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let by = tris_of(mesh);
    let all = Part { objects: (0..mesh.objects.len().max(1)).filter(|o| !by[*o].is_empty()).collect(), tris: None };
    let mut w =
        Writing { table, mesh, by: &by, origin, most: o.most.max(1000), lod: o.lod, compress: o.compress, dir: &tiles_dir, files: Vec::new(), tiles: 0 };
    let (top, top_error) = w.node(&all)?;
    let (root_box, whole) = bounding_box(mesh, origin).expect("bounds");
    let root = json!({
        "transform": transform,
        "boundingVolume": { "box": root_box },
        "geometricError": top_error.max(whole).max(1.0),
        "refine": if o.lod { "REPLACE" } else { "ADD" },
        "children": [top],
    });
    let (mut files, tiles) = (w.files, w.tiles);
    let tileset = json!({
        "asset": { "version": "1.1", "generator": format!("Ographis {}", env!("CARGO_PKG_VERSION")), "extras": { "crs": format!("EPSG:{}", crs), "origin": origin } },
        "geometricError": whole.max(1.0) * 2.0,
        "root": root,
    });
    let path = folder.join("tileset.json");
    std::fs::write(&path, serde_json::to_string_pretty(&tileset).map_err(|e| GbError(e.to_string()))?)
        .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    files.insert(0, path);
    Ok((files, tiles))
}

/// The writing of the tiles.
struct Writing<'a> {
    table: &'a Table,
    mesh: &'a Mesh,
    by: &'a [Vec<usize>],
    origin: [f64; 3],
    most: usize,
    lod: bool,
    compress: bool,
    dir: &'a Path,
    files: Vec<PathBuf>,
    tiles: usize,
}

impl Writing<'_> {
    fn count(&self, part: &Part) -> usize {
        part.tris.as_ref().map_or_else(|| part.objects.iter().map(|o| self.by[*o].len()).sum(), |t| t.len())
    }

    fn write(&mut self, t: &Table) -> Result<String> {
        let name = format!("tile_{:04}.glb", self.tiles);
        self.tiles += 1;
        let opts = GltfOpts { georef: false, origin: Some(self.origin), compress: self.compress, ..Default::default() };
        self.files.extend(write_glb_with(t, &self.dir.join(&name), &opts)?);
        Ok(format!("tiles/{}", name))
    }

    /// A tile of a part, and the error of what it shows: 0 when it holds the whole part; else
    /// how far its lighter model lies from it, or the size of the tile when it has none.
    fn node(&mut self, part: &Part) -> Result<(serde_json::Value, f64)> {
        let t = part_table(self.table, self.mesh, part);
        let m = t.mesh.as_deref().expect("a model");
        let (bbox, diag) = bounding_box(m, self.origin).ok_or_else(|| GbError("A part of the model is empty.".into()))?;
        let total = self.count(part);
        let one_object = part.tris.is_some() || part.objects.len() <= 1;
        if total <= self.most || (one_object && total < 4) {
            let uri = self.write(&t)?;
            return Ok((json!({ "boundingVolume": { "box": bbox }, "geometricError": 0.0, "content": { "uri": uri } }), 0.0));
        }
        let (a, b) = halve(self.mesh, self.by, part);
        let (ja, ea) = self.node(&a)?;
        let (jb, eb) = self.node(&b)?;
        let below = ea.max(eb);
        let mut node = json!({ "boundingVolume": { "box": bbox }, "children": [ja, jb] });
        let mut error = diag.max(below * 2.0).max(1.0);
        if self.lod {
            // A lighter model: the smallest objects left out, the rest simplified.
            let small = diag / 60.0;
            let mut keep: Vec<usize> = Vec::new();
            let mut dropped = 0f64;
            let sizes = object_sizes(m);
            for (k, size) in sizes.into_iter().enumerate() {
                if size >= small || m.objects.len() <= 1 {
                    keep.push(k);
                } else {
                    dropped = dropped.max(size);
                }
            }
            let kept = if m.objects.len() > 1 { m.pick(&keep) } else { m.clone() };
            let s = crate::lighten::Simplify { most: Some(self.most), error: None, keep_edges: false };
            let (light, done) = crate::lighten::simplify(&kept, &s);
            if !light.tris.is_empty() && light.tris.len() < total * 4 / 5 {
                let mut lt = t.like();
                lt.fields = t.fields.clone();
                lt.features = if m.objects.len() > 1 { keep.iter().filter_map(|k| t.features.get(*k).cloned()).collect() } else { t.features.clone() };
                lt.mesh = Some(Arc::new(light));
                let uri = self.write(&lt)?;
                node["content"] = json!({ "uri": uri });
                node["refine"] = json!("REPLACE");
                error = (done.error.max(dropped)).max(below * 1.5).max(0.05);
            }
        }
        if node.get("content").is_none() {
            node["refine"] = json!("ADD");
        }
        node["geometricError"] = json!(error);
        Ok((node, error))
    }
}

/// The largest side of the box around each object.
fn object_sizes(m: &Mesh) -> Vec<f64> {
    let n = m.objects.len().max(1);
    let (mut lo, mut hi) = (vec![[f64::INFINITY; 3]; n], vec![[f64::NEG_INFINITY; 3]; n]);
    for t in &m.tris {
        let o = (t.object as usize).min(n - 1);
        for v in t.v {
            let p = m.positions[v as usize];
            for k in 0..3 {
                lo[o][k] = lo[o][k].min(p[k]);
                hi[o][k] = hi[o][k].max(p[k]);
            }
        }
    }
    (0..n).map(|o| (0..3).map(|k| hi[o][k] - lo[o][k]).fold(0.0, f64::max).max(0.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_on_the_globe() {
        // Göteborg in earth-centred coordinates, as PROJ gives it.
        let p = ecef(11.97, 57.71, 0.0);
        assert!((p[0] - 3_341_147.3).abs() < 0.5 && (p[1] - 708_354.5).abs() < 0.5 && (p[2] - 5_368_552.7).abs() < 0.5, "{:?}", p);
    }
}
