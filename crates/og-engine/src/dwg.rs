//! DWG drawings, the native format of AutoCAD, read with acadrust and written as DXF, so
//! that they are read as DXF drawings are, with the same attributes.

use crate::model::{GbError, Result};
use std::path::{Path, PathBuf};

/// The DWG as a DXF file in the folder for temporary files, which the caller removes, and the
/// version of AutoCAD the drawing was saved in.
pub fn to_dxf(path: &Path) -> Result<(PathBuf, String)> {
    let named = |e: acadrust::DxfError| GbError(format!("{} cannot be read as a DWG drawing: {}", path.display(), e));
    let mut reader = acadrust::DwgReader::from_file_with_options(path, acadrust::DwgReadOptions::failsafe()).map_err(named)?;
    let doc = reader.read().map_err(named)?;
    let version = format!("{:?}", doc.version);
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "drawing".into());
    let out = std::env::temp_dir().join(format!("ographis-dwg-{}-{}-{}.dxf", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst), stem));
    acadrust::DxfWriter::new(&doc).write_to_file(&out).map_err(|e| GbError(format!("{} could not be made ready to read: {}", path.display(), e)))?;
    Ok((out, version))
}

/// Writes a DXF drawing as DWG, for the tests and for those who need to.
pub fn dxf_to_dwg(dxf: &Path, dwg: &Path) -> Result<()> {
    let named = |e: acadrust::DxfError| GbError(format!("{}: {}", dxf.display(), e));
    let doc = acadrust::DxfReader::from_file(dxf).map_err(named)?.read().map_err(named)?;
    acadrust::DwgWriter::write_to_file(dwg, &doc).map_err(named)
}
