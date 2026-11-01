//! DWG drawings, the native format of AutoCAD, read with acadrust and written as DXF, so
//! that they are read as DXF drawings are, with the same attributes.

use crate::model::{GbError, Result};
use std::path::{Path, PathBuf};

/// The DWG as a DXF file in the folder for temporary files, which the caller removes, and the
/// version of AutoCAD the drawing was saved in.
pub fn to_dxf(path: &Path) -> Result<(PathBuf, String)> {
    let named = |e: acadrust::DxfError| GbError(format!("{} cannot be read as a DWG drawing: {}", path.display(), e));
    let mut reader = acadrust::DwgReader::from_file_with_options(path, acadrust::DwgReadOptions::failsafe()).map_err(named)?;
    let mut doc = reader.read().map_err(named)?;
    let version = format!("{:?}", doc.version);
    mirror_clockwise_arcs(&mut doc);
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "drawing".into());
    let out = std::env::temp_dir().join(format!("ographis-dwg-{}-{}-{}.dxf", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst), stem));
    acadrust::DxfWriter::new(&doc).write_to_file(&out).map_err(|e| GbError(format!("{} could not be made ready to read: {}", path.display(), e)))?;
    Ok((out, version))
}

/// The clockwise arcs of the edges of hatches as the DXF writer of acadrust expects them.
/// Read from DWG, their angles come as AutoCAD keeps them in DXF too, and the writer turns
/// them once more, so that every clockwise arc came out as its mirror image: the rest of the
/// circle, and with a large radius a circle across the whole drawing.
fn mirror_clockwise_arcs(doc: &mut acadrust::CadDocument) {
    use acadrust::entities::hatch::BoundaryEdge;
    use acadrust::entities::EntityType;
    let tau = std::f64::consts::TAU;
    for e in doc.entities_mut() {
        if let EntityType::Hatch(h) = e {
            for path in h.paths.iter_mut() {
                for edge in path.edges.iter_mut() {
                    if let BoundaryEdge::CircularArc(a) = edge {
                        if !a.counter_clockwise {
                            a.start_angle = (tau - a.start_angle).rem_euclid(tau);
                            a.end_angle = (tau - a.end_angle).rem_euclid(tau);
                        }
                    }
                }
            }
        }
    }
}

/// Writes a DXF drawing as DWG, for the tests and for those who need to.
pub fn dxf_to_dwg(dxf: &Path, dwg: &Path) -> Result<()> {
    let named = |e: acadrust::DxfError| GbError(format!("{}: {}", dxf.display(), e));
    let doc = acadrust::DxfReader::from_file(dxf).map_err(named)?.read().map_err(named)?;
    acadrust::DwgWriter::write_to_file(dwg, &doc).map_err(named)
}

#[cfg(test)]
mod tests {
    /// Writes the DXF of the drawing in OG_DWG beside it, to look at: cargo test -- --ignored.
    #[test]
    #[ignore]
    fn a_drawing_given_is_written_as_dxf() {
        let Ok(p) = std::env::var("OG_DWG") else { return };
        let (dxf, v) = super::to_dxf(std::path::Path::new(&p)).unwrap();
        std::fs::copy(&dxf, format!("{}.dxf", p)).unwrap();
        println!("{} {}", v, dxf.display());
    }
}
