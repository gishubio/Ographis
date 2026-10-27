//! A geospatial PDF with layers that can be turned on and off, made by GDAL from a
//! composition: each layer is written to a GeoPackage in memory, and the page refers to it.

use crate::{write_gpkg, write_image};
use geo::BoundingRect;
use og_engine::io::{Notes, PdfLayer, PdfPage};
use og_engine::mapdraw::LayerStyle;
use og_engine::model::{GbError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
}

/// How a layer is drawn, as OGR writes it: a pen for lines and outlines, a brush for
/// areas, and a round symbol for points. Pixels of the map are about three quarters of a point.
fn style(s: &LayerStyle) -> String {
    let mut parts = Vec::new();
    match s.stroke {
        Some(c) => parts.push(format!("PEN(c:{},w:{:.2}pt)", hex(c), (s.width * 0.75).max(0.1))),
        None => parts.push("PEN(c:#00000000,w:0pt)".to_string()),
    }
    if let Some(c) = s.fill {
        parts.push(format!("BRUSH(fc:{})", hex(c)));
    }
    let point = s.fill.or(s.stroke).unwrap_or([0, 0, 0, 255]);
    parts.push(format!("SYMBOL(id:\"ogr-sym-3\",c:{},s:{:.2}pt)", hex(point), (s.size * 0.75).max(1.0)));
    parts.join(";")
}

pub fn write_layered(layers: &[PdfLayer], crs: u32, page: &PdfPage, margin: f64, path: &Path, notes: &mut Notes) -> Result<Vec<PathBuf>> {
    // The area of the map: around everything that is drawn, with the margin.
    let mut bounds: Option<geo::Rect<f64>> = None;
    for l in layers {
        let boxes = l.table.features.iter().filter_map(|f| f.geom.as_ref()?.bounding_rect());
        for b in boxes {
            bounds = Some(match bounds {
                None => b,
                Some(a) => geo::Rect::new(
                    geo::coord! { x: a.min().x.min(b.min().x), y: a.min().y.min(b.min().y) },
                    geo::coord! { x: a.max().x.max(b.max().x), y: a.max().y.max(b.max().y) },
                ),
            });
        }
    }
    let b = bounds.ok_or_else(|| GbError("There is nothing to draw: the layers have no geometry.".into()))?;
    let side = b.width().max(b.height()).max(1e-9);
    let m = side * margin;
    let (x0, y0, x1, y1) = (b.min().x - m, b.min().y - m, b.max().x + m, b.max().y + m);
    // The page is laid out as for a picture of this shape.
    let px_w = 2000usize;
    let px_h = ((y1 - y0) / (x1 - x0) * px_w as f64).round().max(1.0) as usize;
    let lay = page.layout(px_w, px_h);
    let (left, bottom) = (lay.left, lay.bottom);
    let (right, top) = (lay.page_w - lay.right, lay.page_h - lay.top);

    // The axes of the coordinate system as GDAL wants them stated.
    let mut srs = gdal::spatial_ref::SpatialRef::from_epsg(crs).map_err(|e| GbError(format!("EPSG:{} is not known: {}", crs, e)))?;
    srs.set_axis_mapping_strategy(gdal::spatial_ref::AxisMappingStrategy::TraditionalGisOrder);
    // SAFETY: the reference is valid, and GDAL hands back a list of `n` numbers it owns.
    let mapping: Vec<String> = unsafe {
        let mut n = 0;
        let p = gdal_sys::OSRGetDataAxisToSRSAxisMapping(srs.to_c_hsrs(), &mut n);
        if p.is_null() {
            vec!["1".into(), "2".into()]
        } else {
            std::slice::from_raw_parts(p, n.max(0) as usize).iter().map(|a| a.to_string()).collect()
        }
    };

    let stamp = format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    let mut temp: Vec<String> = Vec::new();
    let mut temp_files: Vec<PathBuf> = Vec::new();
    let mut tree = String::new();
    let mut content = String::new();
    // From the bottom up, as they are drawn.
    for (k, l) in layers.iter().enumerate() {
        let id = format!("layer{}", k + 1);
        tree += &format!("    <Layer id=\"{}\" name=\"{}\" initiallyVisible=\"{}\"/>\n", id, xml(&l.name), l.visible);
        if let Some(img) = &l.table.image {
            let file = format!("/vsimem/ographis-pdf-{}-{}.tif", stamp, k);
            write_image(img, Some(crs), Path::new(&file), "tif", page, notes)?;
            temp.push(file.clone());
            content +=
                &format!("      <IfLayerOn layerId=\"{}\">\n        <Raster dataset=\"{}\" georeferencingId=\"map\"/>\n      </IfLayerOn>\n", id, xml(&file));
        } else {
            let file = format!("/vsimem/ographis-pdf-{}-{}.gpkg", stamp, k);
            // Features without geometry are not drawn, and GeoPackage cannot name an attribute fid or geom.
            let mut t = l.table.clone();
            t.features.retain(|f| f.geom.is_some());
            let bad: Vec<usize> = t
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| f.name.eq_ignore_ascii_case("fid") || f.name.eq_ignore_ascii_case("geom"))
                .map(|(i, _)| i)
                .collect();
            for i in bad.into_iter().rev() {
                t.fields.remove(i);
                for f in t.features.iter_mut() {
                    f.attrs.remove(i);
                }
            }
            t.crs = Some(crs);
            write_gpkg(&t, Path::new(&file), "features", &HashMap::new(), false, None, notes)?;
            temp.push(file.clone());
            content += &format!(
                "      <IfLayerOn layerId=\"{}\">\n        <Vector dataset=\"{}\" layer=\"features\" georeferencingId=\"map\" ogrStyleString=\"{}\">\n          <LogicalStructure/>\n        </Vector>\n      </IfLayerOn>\n",
                id,
                xml(&file),
                xml(&style(&l.style))
            );
        }
    }
    // The title is a small PDF of its own, as large as the page, with only the text on it.
    // GDAL writes text from a label without the letters å, ä and ö, so it is not used.
    if let Some(title) = page.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let file = std::env::temp_dir().join(format!("ographis-pdf-{}-title.pdf", stamp));
        // GDAL makes pages of whole points, and the two pages have to be the same size.
        title_page(&file, lay.page_w.round(), lay.page_h.round(), lay.title_at, lay.title_size, title)?;
        content += &format!("      <PDF dataset=\"{}\"/>\n", xml(&file.to_string_lossy()));
        temp_files.push(file);
    }
    let composition = format!(
        r#"<PDFComposition>
  <Metadata><Creator>Ographis</Creator>{title}</Metadata>
  <LayerTree>
{tree}  </LayerTree>
  <Page>
    <DPI>72</DPI>
    <Width>{pw:.2}</Width>
    <Height>{ph:.2}</Height>
    <Georeferencing id="map">
      <SRS dataAxisToSRSAxisMapping="{mapping}">EPSG:{crs}</SRS>
      <BoundingBox x1="0" y1="0" x2="{pw:.2}" y2="{ph:.2}"/>
      <ControlPoint x="{left:.2}" y="{bottom:.2}" GeoX="{x0}" GeoY="{y0}"/>
      <ControlPoint x="{left:.2}" y="{top:.2}" GeoX="{x0}" GeoY="{y1}"/>
      <ControlPoint x="{right:.2}" y="{bottom:.2}" GeoX="{x1}" GeoY="{y0}"/>
      <ControlPoint x="{right:.2}" y="{top:.2}" GeoX="{x1}" GeoY="{y1}"/>
    </Georeferencing>
    <Content>
{content}    </Content>
  </Page>
</PDFComposition>
"#,
        title = page.title.as_deref().map(|t| format!("<Title>{}</Title>", xml(t.trim()))).unwrap_or_default(),
        tree = tree,
        pw = lay.page_w.round(),
        ph = lay.page_h.round(),
        mapping = mapping.join(","),
        crs = crs,
        content = content,
    );
    let comp_file = std::env::temp_dir().join(format!("ographis-pdf-{}.xml", stamp));
    std::fs::write(&comp_file, &composition).map_err(|e| GbError(format!("Cannot write the page description: {}", e)))?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| GbError(format!("Cannot write over {}: {}. Is it open in another program?", path.display(), e)))?;
    }
    let driver = gdal::DriverManager::get_driver_by_name("PDF").map_err(|e| GbError(format!("GDAL has no PDF support: {}", e)))?;
    let opt = std::ffi::CString::new(format!("COMPOSITION_FILE={}", comp_file.display())).unwrap();
    let mut opts = [opt.as_ptr() as *mut std::ffi::c_char, std::ptr::null_mut()];
    let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|_| GbError("The file name cannot be used.".into()))?;
    // SAFETY: the driver is valid, the strings live through the call, and the dataset that
    // comes back is closed at once.
    let ds = unsafe { gdal_sys::GDALCreate(driver.c_driver(), c_path.as_ptr(), 0, 0, 0, gdal_sys::GDALDataType::GDT_Unknown, opts.as_mut_ptr()) };
    let made = !ds.is_null();
    if made {
        unsafe { gdal_sys::GDALClose(ds) };
    }
    for f in &temp {
        let _ = gdal::vsi::unlink_mem_file(f);
    }
    let _ = std::fs::remove_file(&comp_file);
    for f in &temp_files {
        let _ = std::fs::remove_file(f);
    }
    if !made || !path.exists() {
        let msg = crate::last_gdal_error().unwrap_or_else(|| "GDAL gave no reason".into());
        return Err(GbError(format!("Cannot write {}: {}", path.display(), msg)));
    }
    Ok(vec![path.to_path_buf()])
}

/// A PDF page of the given size with a title at a place, in Helvetica Bold, and nothing else
/// that shows: GDAL makes a page from a picture, so the picture is one see-through pixel.
fn title_page(path: &Path, w: f64, h: f64, at: (f64, f64), size: f64, title: &str) -> Result<()> {
    let mem = gdal::DriverManager::get_driver_by_name("MEM").map_err(|e| GbError(e.to_string()))?;
    let ds = mem.create_with_band_type::<u8, _>("", 1, 1, 4).map_err(|e| GbError(e.to_string()))?;
    if let Ok(mut alpha) = ds.rasterband(4) {
        let _ = alpha.set_color_interpretation(gdal::raster::ColorInterpretation::AlphaBand);
    }
    let pdf = gdal::DriverManager::get_driver_by_name("PDF").map_err(|e| GbError(format!("GDAL has no PDF support: {}", e)))?;
    let opts: gdal::raster::RasterCreationOptions = [
        "DPI=72".to_string(),
        "LEFT_MARGIN=0".to_string(),
        "BOTTOM_MARGIN=0".to_string(),
        format!("RIGHT_MARGIN={:.0}", (w - 1.0).max(0.0)),
        format!("TOP_MARGIN={:.0}", (h - 1.0).max(0.0)),
        "GEO_ENCODING=NONE".to_string(),
        format!("EXTRA_STREAM=BT /FHelveticaBold {:.1} Tf 1 0 0 1 {:.2} {:.2} Tm ({}) Tj ET", size, at.0, at.1, crate::pdf_text(title)),
    ]
    .into_iter()
    .collect();
    let out = ds.create_copy(&pdf, path, &opts).map_err(|e| GbError(format!("Cannot write the title: {}", e)))?;
    drop(out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_are_written_as_ogr_wants_them() {
        let s = LayerStyle { fill: Some([255, 0, 0, 255]), stroke: Some([0, 0, 255, 128]), width: 2.0, size: 8.0 };
        assert_eq!(style(&s), "PEN(c:#0000FF80,w:1.50pt);BRUSH(fc:#FF0000FF);SYMBOL(id:\"ogr-sym-3\",c:#FF0000FF,s:6.00pt)");
        assert_eq!(xml("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
    }
}
