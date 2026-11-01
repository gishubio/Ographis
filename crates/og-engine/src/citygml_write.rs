//! Writes buildings with a 3D model as CityGML 2.0: a bldg:Building for each feature, its
//! surfaces as walls, roofs and ground by which way they face, and its attributes as generic
//! attributes. For buildings from Extrude (LoD1) and LoD2 buildings from points, to read in
//! FZK Viewer, QGIS, 3DCityDB and Ographis itself.

use crate::mesh::Mesh;
use crate::model::{GbError, Result, Table, Value};
use std::io::Write;
use std::path::Path;

/// What a triangle is, by which way it faces: ground, wall or roof.
fn kind_of(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<&'static str> {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l < 1e-12 {
        return None;
    }
    let nz = n[2] / l;
    Some(if nz < -0.7 {
        "GroundSurface"
    } else if nz.abs() < 0.17 {
        "WallSurface"
    } else {
        "RoofSurface"
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Writes the buildings of a stream to a CityGML 2.0 file. Gives how many were written.
pub fn write_citygml(table: &Table, path: &Path, lod: u8) -> Result<usize> {
    let Some(mesh) = table.mesh.as_deref() else {
        return Err(GbError("The stream has no 3D model to write as CityGML. Make the buildings with Extrude or LoD2 buildings from points.".into()));
    };
    let lod = lod.clamp(1, 2);
    let srs = table.crs.map(|c| format!("EPSG:{}", c)).unwrap_or_default();
    let mut by_object: Vec<Vec<usize>> = vec![Vec::new(); mesh.objects.len().max(table.len())];
    for (i, t) in mesh.tris.iter().enumerate() {
        if let Some(v) = by_object.get_mut(t.object as usize) {
            v.push(i);
        }
    }
    let (lo, hi) = mesh.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?);
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:bldg="http://www.opengis.net/citygml/building/2.0" xmlns:gen="http://www.opengis.net/citygml/generics/2.0" xmlns:gml="http://www.opengis.net/gml" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.opengis.net/citygml/building/2.0 http://schemas.opengis.net/citygml/building/2.0/building.xsd http://www.opengis.net/citygml/generics/2.0 http://schemas.opengis.net/citygml/generics/2.0/generics.xsd">
"#);
    out.push_str(&format!(
        "  <gml:boundedBy><gml:Envelope srsName=\"{}\" srsDimension=\"3\"><gml:lowerCorner>{} {} {}</gml:lowerCorner><gml:upperCorner>{} {} {}</gml:upperCorner></gml:Envelope></gml:boundedBy>\n",
        srs, lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
    ));
    let p = |m: &Mesh, i: u32| m.positions[i as usize];
    let ring = |m: &Mesh, t: [u32; 3]| {
        let pts = [p(m, t[0]), p(m, t[1]), p(m, t[2]), p(m, t[0])];
        pts.iter().map(|q| format!("{:.3} {:.3} {:.3}", q[0], q[1], q[2])).collect::<Vec<_>>().join(" ")
    };
    let mut written = 0usize;
    let height_at = table.field_index("height");
    for (k, tris) in by_object.iter().enumerate() {
        if tris.is_empty() {
            continue;
        }
        let id = mesh.objects.get(k).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("building_{}", k + 1));
        out.push_str(&format!("  <core:cityObjectMember>\n    <bldg:Building gml:id=\"{}\">\n", esc(&id.replace(' ', "_"))));
        if let Some(feat) = table.features.get(k) {
            for (field, v) in table.fields.iter().zip(&feat.attrs) {
                let (tag, text) = match v {
                    Value::Null => continue,
                    Value::Int(i) => ("intAttribute", i.to_string()),
                    Value::Real(r) => ("doubleAttribute", r.to_string()),
                    Value::Bool(b) => ("stringAttribute", b.to_string()),
                    Value::Text(s) => ("stringAttribute", s.clone()),
                };
                out.push_str(&format!("      <gen:{} name=\"{}\"><gen:value>{}</gen:value></gen:{}>\n", tag, esc(&field.name), esc(&text), tag));
            }
            if let Some(h) = height_at.and_then(|i| feat.attrs[i].as_f64()) {
                out.push_str(&format!("      <bldg:measuredHeight uom=\"m\">{}</bldg:measuredHeight>\n", h));
            }
        }
        if lod == 1 {
            out.push_str("      <bldg:lod1Solid><gml:Solid><gml:exterior><gml:CompositeSurface>\n");
            for i in tris {
                let t = &mesh.tris[*i];
                out.push_str(&format!(
                    "        <gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing><gml:posList srsDimension=\"3\">{}</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>\n",
                    ring(mesh, t.v)
                ));
            }
            out.push_str("      </gml:CompositeSurface></gml:exterior></gml:Solid></bldg:lod1Solid>\n");
        } else {
            for kind in ["GroundSurface", "WallSurface", "RoofSurface"] {
                let these: Vec<&usize> = tris
                    .iter()
                    .filter(|i| {
                        let t = &mesh.tris[**i];
                        kind_of(p(mesh, t.v[0]), p(mesh, t.v[1]), p(mesh, t.v[2])) == Some(kind)
                    })
                    .collect();
                if these.is_empty() {
                    continue;
                }
                out.push_str(&format!("      <bldg:boundedBy><bldg:{}><bldg:lod2MultiSurface><gml:MultiSurface>\n", kind));
                for i in these {
                    let t = &mesh.tris[*i];
                    out.push_str(&format!(
                        "        <gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing><gml:posList srsDimension=\"3\">{}</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>\n",
                        ring(mesh, t.v)
                    ));
                }
                out.push_str(&format!("      </gml:MultiSurface></bldg:lod2MultiSurface></bldg:{}></bldg:boundedBy>\n", kind));
            }
        }
        out.push_str("    </bldg:Building>\n  </core:cityObjectMember>\n");
        written += 1;
        if out.len() > 1 << 20 {
            f.write_all(out.as_bytes()).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
            out.clear();
        }
    }
    out.push_str("</core:CityModel>\n");
    f.write_all(out.as_bytes()).and_then(|_| f.flush()).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_are_told_by_which_way_they_face() {
        assert_eq!(kind_of([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), Some("RoofSurface"));
        assert_eq!(kind_of([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]), Some("GroundSurface"));
        assert_eq!(kind_of([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]), Some("WallSurface"));
    }
}
