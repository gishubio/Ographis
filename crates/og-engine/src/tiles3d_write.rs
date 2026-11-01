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

/// Splits the objects into groups of at most `most` triangles, by halving the area they lie
/// in along its longer side.
fn split(mesh: &Mesh, objects: Vec<usize>, most: usize, out: &mut Vec<Vec<usize>>) {
    let mut tris = vec![0usize; mesh.objects.len().max(1)];
    let mut lo = vec![[f64::INFINITY; 2]; mesh.objects.len().max(1)];
    let mut hi = vec![[f64::NEG_INFINITY; 2]; mesh.objects.len().max(1)];
    for t in &mesh.tris {
        let o = t.object as usize;
        if o >= tris.len() {
            continue;
        }
        tris[o] += 1;
        for v in t.v {
            let p = mesh.positions[v as usize];
            lo[o] = [lo[o][0].min(p[0]), lo[o][1].min(p[1])];
            hi[o] = [hi[o][0].max(p[0]), hi[o][1].max(p[1])];
        }
    }
    fn go(objects: Vec<usize>, tris: &[usize], mid: &dyn Fn(usize) -> [f64; 2], most: usize, out: &mut Vec<Vec<usize>>) {
        let total: usize = objects.iter().map(|o| tris[*o]).sum();
        if total <= most || objects.len() <= 1 {
            if !objects.is_empty() {
                out.push(objects);
            }
            return;
        }
        let (mut a, mut b) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for o in &objects {
            let m = mid(*o);
            a = [a[0].min(m[0]), a[1].min(m[1])];
            b = [b[0].max(m[0]), b[1].max(m[1])];
        }
        let axis = if b[0] - a[0] >= b[1] - a[1] { 0 } else { 1 };
        let mut sorted = objects;
        sorted.sort_by(|x, y| mid(*x)[axis].total_cmp(&mid(*y)[axis]));
        let half = sorted.len() / 2;
        let right = sorted.split_off(half);
        go(sorted, tris, mid, most, out);
        go(right, tris, mid, most, out);
    }
    let mid = |o: usize| [(lo[o][0] + hi[o][0]) / 2.0, (lo[o][1] + hi[o][1]) / 2.0];
    let objects: Vec<usize> = objects.into_iter().filter(|o| *o < tris.len() && tris[*o] > 0).collect();
    go(objects, &tris, &mid, most, out);
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
    let mut groups = Vec::new();
    split(mesh, (0..mesh.objects.len()).collect(), o.most.max(1000), &mut groups);
    // A model without objects, such as a TIN, is one tile.
    if groups.is_empty() {
        groups.push(Vec::new());
    }
    let mut files = Vec::new();
    let mut children = Vec::new();
    let mut root_diag: f64 = 0.0;
    let opts = GltfOpts { georef: false, origin: Some(origin), ..Default::default() };
    for (k, objects) in groups.iter().enumerate() {
        let part = if objects.is_empty() {
            table.clone()
        } else {
            let mut t = table.like();
            t.fields = table.fields.clone();
            t.features = objects.iter().filter_map(|i| table.features.get(*i).cloned()).collect();
            t.mesh = Some(Arc::new(mesh.pick(objects)));
            t
        };
        let m = part.mesh.as_deref().expect("a model");
        let Some((bbox, diag)) = bounding_box(m, origin) else { continue };
        root_diag = root_diag.max(diag);
        let name = format!("tile_{:04}.glb", k);
        files.extend(write_glb_with(&part, &tiles_dir.join(&name), &opts)?);
        children.push(json!({
            "boundingVolume": { "box": bbox },
            "geometricError": 0.0,
            "content": { "uri": format!("tiles/{}", name) },
        }));
    }
    let (root_box, whole) = bounding_box(mesh, origin).expect("bounds");
    let root = json!({
        "transform": transform,
        "boundingVolume": { "box": root_box },
        "geometricError": root_diag.max(1.0),
        "refine": "ADD",
        "children": children,
    });
    let tileset = json!({
        "asset": { "version": "1.1", "generator": format!("Ographis {}", env!("CARGO_PKG_VERSION")), "extras": { "crs": format!("EPSG:{}", crs), "origin": origin } },
        "geometricError": whole.max(1.0) * 2.0,
        "root": root,
    });
    let path = folder.join("tileset.json");
    std::fs::write(&path, serde_json::to_string_pretty(&tileset).map_err(|e| GbError(e.to_string()))?)
        .map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    files.insert(0, path);
    Ok((files, groups.len()))
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
