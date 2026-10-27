//! Rasters, such as GeoTIFF, read into features: a point or a square for each cell with the
//! values of the bands, or one area that is the outline of the raster.

use crate::epsg_of;
use gdal::raster::GdalDataType;
use gdal::Metadata;
use gdal::{Dataset, DatasetOptions, GdalOpenFlags};
use geo::{Coord, LineString, Point, Polygon};
use og_engine::io::{LayerInfo, Notes, RasterMode, ReadOpts};
use og_engine::{Feature, Field, FieldType, GbError, Result, Table, Value};
use std::ffi::CString;
use std::path::Path;

/// A raster with more cells than this is only read with a limit, or as its outline. Each
/// cell becomes a feature, and they are all held in memory.
pub const MAX_CELLS: usize = 5_000_000;

fn open(path: &Path) -> Result<Dataset> {
    let p = crate::gdal_path(path);
    Dataset::open_ex(&p, DatasetOptions { open_flags: GdalOpenFlags::GDAL_OF_RASTER | GdalOpenFlags::GDAL_OF_READONLY, ..Default::default() })
        .map_err(|e| GbError(format!("Cannot open {} as a raster: {}", path.display(), e)))
}

/// Where a point in cell coordinates lies: column and row from the upper left corner.
fn place(gt: &[f64; 6], col: f64, row: f64) -> Coord<f64> {
    Coord { x: gt[0] + col * gt[1] + row * gt[2], y: gt[3] + col * gt[4] + row * gt[5] }
}

fn square(gt: &[f64; 6], col: f64, row: f64, w: f64, h: f64) -> Polygon<f64> {
    let ring = vec![place(gt, col, row), place(gt, col + w, row), place(gt, col + w, row + h), place(gt, col, row + h), place(gt, col, row)];
    Polygon::new(LineString::new(ring), vec![])
}

/// The names of the bands as attributes: the description of a band when it has one that no
/// other band has, otherwise band_1, band_2 and so on.
fn band_names(ds: &Dataset) -> Vec<String> {
    let n = ds.raster_count();
    let described: Vec<String> = (1..=n)
        .map(|i| ds.rasterband(i).ok().map(|b| b.description().unwrap_or_default()).unwrap_or_default())
        .map(|d| d.trim().chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect::<String>())
        .collect();
    (0..n)
        .map(|i| {
            let d = &described[i];
            let fallback = format!("band_{}", i + 1);
            let clash = d.is_empty()
                || matches!(d.as_str(), "col" | "row")
                || described.iter().filter(|x| *x == d).count() > 1
                || (0..n).any(|k| k != i && format!("band_{}", k + 1) == *d);
            if clash {
                fallback
            } else {
                d.clone()
            }
        })
        .collect()
}

fn georeferencing(ds: &Dataset, path: &Path) -> Result<[f64; 6]> {
    match ds.geo_transform() {
        Ok(gt) if gt != [0.0, 1.0, 0.0, 0.0, 0.0, 1.0] => Ok(gt),
        _ => Err(GbError(format!("{} has no coordinates: the raster is not georeferenced.", path.display()))),
    }
}

fn crs_of(ds: &Dataset) -> Option<u32> {
    ds.spatial_ref().ok().filter(|s| !s.is_local()).as_ref().and_then(epsg_of)
}

fn type_name(t: GdalDataType) -> String {
    t.name().to_lowercase()
}

/// The part of a raster that a rectangle covers, as (first column, first row, columns,
/// rows), when only that part is to be read. A rectangle in another coordinate system is
/// reprojected first. Only part of a raster on the web (a Cloud Optimized GeoTIFF) is then
/// fetched, and from its overviews when it is read coarser.
fn window(
    size: (usize, usize),
    gt: &[f64; 6],
    within: Option<([f64; 4], Option<u32>)>,
    crs: Option<u32>,
    path: &Path,
) -> Result<Option<(usize, usize, usize, usize)>> {
    let Some((r, rect_crs)) = within else { return Ok(None) };
    if gt[2] != 0.0 || gt[4] != 0.0 {
        return Ok(None);
    }
    let mut xs = vec![r[0], r[2], r[2], r[0], (r[0] + r[2]) / 2.0, (r[0] + r[2]) / 2.0, r[0], r[2]];
    let mut ys = vec![r[1], r[1], r[3], r[3], r[1], r[3], (r[1] + r[3]) / 2.0, (r[1] + r[3]) / 2.0];
    if let (Some(from), Some(to)) = (rect_crs, crs) {
        if from != to {
            let (s, t) = (crate::srs_from_epsg(from)?, crate::srs_from_epsg(to)?);
            let ct = gdal::spatial_ref::CoordTransform::new(&s, &t).map_err(|e| GbError(format!("Cannot reproject the rectangle: {}", e)))?;
            ct.transform_coords(&mut xs, &mut ys, &mut []).map_err(|e| GbError(format!("Cannot reproject the rectangle: {}", e)))?;
        }
    }
    let cols: Vec<f64> = xs.iter().map(|x| (x - gt[0]) / gt[1]).collect();
    let rows: Vec<f64> = ys.iter().map(|y| (y - gt[3]) / gt[5]).collect();
    let lo = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min).floor().max(0.0);
    let hi = |v: &[f64], n: usize| v.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil().min(n as f64);
    let (c0, c1, r0, r1) = (lo(&cols), hi(&cols, size.0), lo(&rows), hi(&rows, size.1));
    if c1 <= c0 || r1 <= r0 {
        return Err(GbError(format!("The rectangle does not reach into {}.", path.display())));
    }
    Ok(Some((c0 as usize, r0 as usize, (c1 - c0) as usize, (r1 - r0) as usize)))
}

pub fn read(path: &Path, opts: &ReadOpts, notes: &mut Notes) -> Result<Table> {
    let ds = open(path)?;
    let gt = georeferencing(&ds, path)?;
    let (w, h) = ds.raster_size();
    let bands = ds.raster_count();
    if bands == 0 {
        return Err(GbError(format!("{} has no bands.", path.display())));
    }
    let crs = match crs_of(&ds) {
        Some(c) => Some(c),
        None => {
            if opts.crs.is_none() {
                return Err(GbError(format!("{} does not say which coordinate system it is in. Set the coordinate system in the input.", path.display())));
            }
            opts.crs
        }
    };
    let first = ds.rasterband(1).map_err(|e| GbError(format!("Cannot read the first band: {}", e)))?;

    if matches!(opts.raster, RasterMode::Image | RasterMode::Grid) {
        let part = window((w, h), &gt, opts.within, crs, path)?;
        if let Some((_, _, pw, ph)) = part {
            notes.info(format!("read within the rectangle: {} × {} of {} × {} cells", pw, ph, w, h));
        }
        return if opts.raster == RasterMode::Image {
            read_image(&ds, gt, crs, part, path, notes)
        } else {
            read_grid(&ds, gt, crs, opts.cell_size, part, path, notes)
        };
    }
    if opts.raster == RasterMode::Extent {
        let fields = vec![
            Field::new("columns", FieldType::Int),
            Field::new("rows", FieldType::Int),
            Field::new("bands", FieldType::Int),
            Field::new("cell_width", FieldType::Real),
            Field::new("cell_height", FieldType::Real),
            Field::new("data_type", FieldType::Text),
            Field::new("nodata", FieldType::Real),
        ];
        let attrs = vec![
            Value::Int(w as i64),
            Value::Int(h as i64),
            Value::Int(bands as i64),
            Value::Real((gt[1] * gt[1] + gt[4] * gt[4]).sqrt()),
            Value::Real((gt[2] * gt[2] + gt[5] * gt[5]).sqrt()),
            Value::Text(type_name(first.band_type())),
            first.no_data_value().map(Value::Real).unwrap_or(Value::Null),
        ];
        let geom = square(&gt, 0.0, 0.0, w as f64, h as f64);
        return Ok(Table {
            mesh: None,
            fields,
            crs,
            features: vec![Feature { z: None, attrs, geom: Some(geo::Geometry::Polygon(geom)) }],
            raster: true,
            image: None,
            grid: None,
        });
    }

    let cells = w * h;
    if cells > MAX_CELLS && opts.limit.is_none_or(|l| l > MAX_CELLS) {
        return Err(GbError(format!(
            "{} has {} cells ({} × {}), and each would become a feature. Set the largest number of features in the input, or read the raster as its outline.",
            path.display(),
            cells,
            w,
            h
        )));
    }
    let names = band_names(&ds);
    let mut fields = vec![Field::new("col", FieldType::Int), Field::new("row", FieldType::Int)];
    let mut kinds = Vec::new();
    let mut nodata = Vec::new();
    let mut handles = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let band = ds.rasterband(i + 1).map_err(|e| GbError(format!("Cannot read band {}: {}", i + 1, e)))?;
        let integer = band.band_type().is_integer();
        fields.push(Field::new(name, if integer { FieldType::Int } else { FieldType::Real }));
        kinds.push(integer);
        nodata.push(band.no_data_value());
        handles.push(band);
    }
    let mut features = Vec::new();
    let mut skipped = 0usize;
    'rows: for row in 0..h {
        // One row at a time, so that a limit stops the reading early.
        let mut values = Vec::with_capacity(handles.len());
        for band in &handles {
            let buf = band.read_as::<f64>((0, row as isize), (w, 1), (w, 1), None).map_err(|e| GbError(format!("Cannot read row {}: {}", row + 1, e)))?;
            values.push(buf.into_shape_and_vec().1);
        }
        // The column is a place in every band at once, so it is counted rather than iterated.
        #[allow(clippy::needless_range_loop)]
        for col in 0..w {
            let empty = |b: usize| {
                let v = values[b][col];
                v.is_nan() || nodata[b].is_some_and(|n| v == n || (n.is_nan() && v.is_nan()))
            };
            // A cell without a value in any band is not part of the data.
            if (0..handles.len()).all(empty) {
                skipped += 1;
                continue;
            }
            if opts.limit.is_some_and(|l| features.len() >= l) {
                break 'rows;
            }
            let mut attrs = vec![Value::Int(col as i64), Value::Int(row as i64)];
            for b in 0..handles.len() {
                attrs.push(match (empty(b), kinds[b]) {
                    (true, _) => Value::Null,
                    (false, true) => Value::Int(values[b][col] as i64),
                    (false, false) => Value::Real(values[b][col]),
                });
            }
            let geom = match opts.raster {
                RasterMode::Cells => geo::Geometry::Polygon(square(&gt, col as f64, row as f64, 1.0, 1.0)),
                _ => geo::Geometry::Point(Point::from(place(&gt, col as f64 + 0.5, row as f64 + 0.5))),
            };
            features.push(Feature { z: None, attrs, geom: Some(geom) });
        }
    }
    if skipped > 0 {
        notes.info(format!("{} cells without a value were left out", skipped));
    }
    Ok(Table { mesh: None, fields, crs, features, raster: true, image: None, grid: None })
}

/// The raster as one layer, for the list of layers and for `og info`.
pub fn layers(path: &Path) -> Result<Vec<LayerInfo>> {
    let ds = open(path)?;
    let (w, h) = ds.raster_size();
    let srs = ds.spatial_ref().ok().filter(|s| !s.is_local());
    let mut fields = vec![Field::new("col", FieldType::Int), Field::new("row", FieldType::Int)];
    for (i, name) in band_names(&ds).into_iter().enumerate() {
        let integer = ds.rasterband(i + 1).map(|b| b.band_type().is_integer()).unwrap_or(false);
        fields.push(Field::new(name, if integer { FieldType::Int } else { FieldType::Real }));
    }
    Ok(vec![LayerInfo {
        name: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        count: Some((w * h) as u64),
        crs: srs.as_ref().and_then(epsg_of),
        crs_name: srs.as_ref().and_then(|s| s.name()),
        geometry: format!("raster, {} × {} cells, {} {}", w, h, ds.raster_count(), if ds.raster_count() == 1 { "band" } else { "bands" }),
        fields,
    }])
}

/// The most cells a grid is read with. A larger raster is read with coarser cells.
pub const MAX_GRID_CELLS: usize = 40_000_000;

/// The first band as values on a grid, at the cell size asked for, or coarser when the
/// raster has more cells than a grid can hold. GDAL averages the cells when it reads them
/// coarser. Cells without a value are NaN.
#[allow(clippy::too_many_arguments)]
fn read_grid(
    ds: &Dataset,
    gt: [f64; 6],
    crs: Option<u32>,
    cell_size: Option<f64>,
    part: Option<(usize, usize, usize, usize)>,
    path: &Path,
    notes: &mut Notes,
) -> Result<Table> {
    let (full_w, full_h) = ds.raster_size();
    let (x0, y0, w, h) = part.unwrap_or((0, 0, full_w, full_h));
    let gt = [gt[0] + x0 as f64 * gt[1], gt[1], gt[2], gt[3] + y0 as f64 * gt[5], gt[4], gt[5]];
    let own = (gt[1] * gt[1] + gt[4] * gt[4]).sqrt();
    if gt[2] != 0.0 || gt[4] != 0.0 {
        return Err(GbError(format!("{} is turned against north, and a grid has to be straight. Warp it with GDAL first.", path.display())));
    }
    // How many cells of the file go into one cell of the grid, across and down.
    let mut factor = cell_size.map_or(1.0, |c| (c / own).max(1.0));
    if (w as f64 / factor) * (h as f64 / factor) > MAX_GRID_CELLS as f64 {
        factor = ((w * h) as f64 / MAX_GRID_CELLS as f64).sqrt().ceil().max(factor);
        notes.warn(format!(
            "{} has {} × {} cells, more than a grid holds, so it was read with cells of {}. Set the cell size in the input to choose.",
            path.display(),
            w,
            h,
            og_engine::model::fmt_real(own * factor)
        ));
    }
    let (gw, gh) = (((w as f64 / factor).round() as usize).max(1), ((h as f64 / factor).round() as usize).max(1));
    let band = ds.rasterband(1).map_err(|e| GbError(format!("Cannot read the first band: {}", e)))?;
    let nodata = band.no_data_value();
    let resample = if gw < w { Some(gdal::raster::ResampleAlg::Average) } else { None };
    let buf =
        band.read_as::<f32>((x0 as isize, y0 as isize), (w, h), (gw, gh), resample).map_err(|e| GbError(format!("Cannot read {}: {}", path.display(), e)))?;
    let mut values = buf.into_shape_and_vec().1;
    if let Some(n) = nodata {
        let n = n as f32;
        for v in values.iter_mut() {
            if *v == n {
                *v = f32::NAN;
            }
        }
    }
    let sx = gt[1] * w as f64 / gw as f64;
    let sy = gt[5] * h as f64 / gh as f64;
    let grid = og_engine::model::Grid { bands: Vec::new(), width: gw, height: gh, values, geotransform: [gt[0], sx, 0.0, gt[3], 0.0, sy], crs };
    if let Some((lo, hi)) = grid.range() {
        notes.info(format!("a grid of {} × {} cells of {}, values from {} to {}", gw, gh, og_engine::model::fmt_real(sx.abs()), lo, hi));
    }
    Ok(grid.into_table())
}

/// The largest picture that is read, in pixels.
pub const MAX_PIXELS: usize = 60_000_000;

/// A raster as a picture. Three or four bands of bytes are its colours. Anything else is
/// shown in grey from the first band, from its lowest value to its highest. Cells without a
/// value are see-through.
fn read_image(ds: &Dataset, gt: [f64; 6], crs: Option<u32>, part: Option<(usize, usize, usize, usize)>, _path: &Path, notes: &mut Notes) -> Result<Table> {
    let (x0, y0, full_w, full_h) = part.unwrap_or((0, 0, ds.raster_size().0, ds.raster_size().1));
    let gt = [gt[0] + x0 as f64 * gt[1], gt[1], gt[2], gt[3] + y0 as f64 * gt[5], gt[4], gt[5]];
    // A large raster is read smaller, as GDAL makes it from its overviews or by averaging,
    // so that it can be shown. The cells then cover more ground.
    let k = if full_w * full_h > MAX_PIXELS { ((full_w * full_h) as f64 / MAX_PIXELS as f64).sqrt().ceil() as usize } else { 1 };
    let (w, h) = (full_w.div_ceil(k).max(1), full_h.div_ceil(k).max(1));
    let gt = if k > 1 {
        notes.info(format!("{} × {} cells is more than can be shown, so it was read at {} × {}, each cell {} times as wide", full_w, full_h, w, h, k));
        let (sx, sy) = (full_w as f64 / w as f64, full_h as f64 / h as f64);
        [gt[0], gt[1] * sx, gt[2] * sy, gt[3], gt[4] * sx, gt[5] * sy]
    } else {
        gt
    };
    let bands = ds.raster_count();
    let read = |i: usize| -> Result<(Vec<f64>, Option<f64>, GdalDataType)> {
        let band = ds.rasterband(i).map_err(|e| GbError(format!("Cannot read band {}: {}", i, e)))?;
        let resample = if k > 1 { Some(gdal::raster::ResampleAlg::Average) } else { None };
        let buf = band
            .read_as::<f64>((x0 as isize, y0 as isize), (full_w, full_h), (w, h), resample)
            .map_err(|e| GbError(format!("Cannot read band {}: {}", i, e)))?;
        Ok((buf.into_shape_and_vec().1, band.no_data_value(), band.band_type()))
    };
    let mut rgba = vec![0u8; w * h * 4];
    let first = read(1)?;
    let colours = bands >= 3 && first.2 == GdalDataType::UInt8;
    let to_f32 = |b: &(Vec<f64>, Option<f64>, GdalDataType)| -> Vec<f32> {
        b.0.iter().map(|v| if v.is_nan() || b.1.is_some_and(|n| *v == n) { f32::NAN } else { *v as f32 }).collect()
    };
    // The values of the bands, for a click on the map: all of them, as many as fit.
    let mut kept: Vec<Vec<f32>> = Vec::new();
    let keep_bands = if gt[2] == 0.0 && gt[4] == 0.0 { (MAX_GRID_CELLS / (w * h).max(1)).min(bands).min(32) } else { 0 };
    if colours {
        let (g, b) = (read(2)?, read(3)?);
        let a = if bands >= 4 { Some(read(4)?) } else { None };
        for k in 0..w * h {
            let empty = first.1.is_some_and(|n| first.0[k] == n) && g.1.is_some_and(|n| g.0[k] == n) && b.1.is_some_and(|n| b.0[k] == n);
            rgba[k * 4] = first.0[k] as u8;
            rgba[k * 4 + 1] = g.0[k] as u8;
            rgba[k * 4 + 2] = b.0[k] as u8;
            rgba[k * 4 + 3] = if empty { 0 } else { a.as_ref().map_or(255, |a| a.0[k] as u8) };
        }
        // A picture of colours with more bands than red, green, blue and alpha keeps them all.
        if bands > 4 && keep_bands > 0 {
            for i in 1..=keep_bands {
                kept.push(to_f32(&read(i)?));
            }
        }
    } else {
        let (values, nodata, _) = &first;
        let empty = |v: f64| v.is_nan() || nodata.is_some_and(|n| v == n);
        let (lo, hi) = values.iter().filter(|v| !empty(**v)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
        let span = if hi > lo { hi - lo } else { 1.0 };
        for (k, v) in values.iter().enumerate() {
            if empty(*v) {
                continue;
            }
            let grey = (((v - lo) / span) * 255.0).round().clamp(0.0, 255.0) as u8;
            rgba[k * 4..k * 4 + 4].copy_from_slice(&[grey, grey, grey, 255]);
        }
        if lo.is_finite() {
            let others = if bands > 1 { format!("; a click on the map gives the values of all {} bands", keep_bands.max(1)) } else { String::new() };
            notes.info(format!("band 1 shown in grey from {} to {}{}", og_engine::model::fmt_real(lo), og_engine::model::fmt_real(hi), others));
        }
        if keep_bands > 0 {
            kept.push(to_f32(&first));
            for i in 2..=keep_bands {
                kept.push(to_f32(&read(i)?));
            }
        }
    }
    let image = og_engine::model::RasterImage { width: w, height: h, rgba, geotransform: gt };
    // The values of the bands go along, so that a click on the map tells the values of a
    // cell. A picture of red, green and blue carries its colours, which tell them already.
    let grid = if kept.is_empty() {
        None
    } else {
        let values = kept.remove(0);
        Some(og_engine::model::Grid { width: w, height: h, values, geotransform: gt, crs, bands: kept })
    };
    Ok(image.into_table_with(crs, grid))
}

/// A raster in memory with its place and coordinate system, to warp.
fn mem_dataset<T: gdal::raster::GdalType + Copy>(width: usize, height: usize, bands: &[Vec<T>], gt: [f64; 6], epsg: u32) -> Result<Dataset> {
    let mem = gdal::DriverManager::get_driver_by_name("MEM").map_err(|e| GbError(format!("GDAL has no in-memory driver: {}", e)))?;
    let mut ds = mem.create_with_band_type::<T, _>("", width, height, bands.len()).map_err(|e| GbError(format!("Cannot make a raster in memory: {}", e)))?;
    ds.set_geo_transform(&gt).map_err(|e| GbError(format!("Cannot place the raster: {}", e)))?;
    let mut srs = gdal::spatial_ref::SpatialRef::from_epsg(epsg).map_err(|e| GbError(format!("EPSG:{} is not known: {}", epsg, e)))?;
    // East is x, also in systems such as SWEREF 99 12 00 whose official order is north first.
    srs.set_axis_mapping_strategy(gdal::spatial_ref::AxisMappingStrategy::TraditionalGisOrder);
    ds.set_spatial_ref(&srs).map_err(|e| GbError(format!("Cannot give the raster its coordinate system: {}", e)))?;
    for (i, data) in bands.iter().enumerate() {
        let mut band = ds.rasterband(i + 1).map_err(|e| GbError(format!("Cannot write band {}: {}", i + 1, e)))?;
        let mut buf = gdal::raster::Buffer::new((width, height), data.clone());
        band.write((0, 0), (width, height), &mut buf).map_err(|e| GbError(format!("Cannot write band {}: {}", i + 1, e)))?;
    }
    Ok(ds)
}

/// Where a raster lands in another coordinate system, as GDAL suggests: the place of its
/// upper left corner and its cells, and its size. The cells keep about their size.
fn suggested(src: &Dataset, to: u32) -> Result<([f64; 6], usize, usize)> {
    let opt = CString::new(format!("DST_SRS=EPSG:{}", to)).unwrap();
    let mut opts = [opt.as_ptr() as *mut std::ffi::c_char, std::ptr::null_mut()];
    // SAFETY: the dataset is open, the options are a list ended by null that lives through
    // the call, and the transformer is destroyed after use.
    unsafe {
        let t = gdal_sys::GDALCreateGenImgProjTransformer2(src.c_dataset(), std::ptr::null_mut(), opts.as_mut_ptr());
        if t.is_null() {
            return Err(GbError(format!("Cannot reproject the raster to EPSG:{}", to)));
        }
        let mut gt = [0.0f64; 6];
        let (mut w, mut h) = (0i32, 0i32);
        let rv = gdal_sys::GDALSuggestedWarpOutput(src.c_dataset(), Some(gdal_sys::GDALGenImgProjTransform), t, gt.as_mut_ptr(), &mut w, &mut h);
        gdal_sys::GDALDestroyGenImgProjTransformer(t);
        if rv != gdal_sys::CPLErr::CE_None || w <= 0 || h <= 0 {
            return Err(GbError(format!("Cannot work out where the raster lands in EPSG:{}", to)));
        }
        Ok((gt, w as usize, h as usize))
    }
}

/// A grid of values in another coordinate system, with the values between cells taken
/// from the four nearest. Cells outside the grid get no value.
pub fn warp_grid(g: &og_engine::model::Grid, to: u32) -> Result<og_engine::model::Grid> {
    let from = g.crs.ok_or_else(|| GbError("The grid has no coordinate system and cannot be reprojected.".into()))?;
    if from == to {
        return Ok(g.clone());
    }
    let src = mem_dataset::<f32>(g.width, g.height, std::slice::from_ref(&g.values), g.geotransform, from)?;
    src.rasterband(1).and_then(|mut b| b.set_no_data_value(Some(f64::NAN))).map_err(|e| GbError(format!("Cannot mark the cells without a value: {}", e)))?;
    let (gt, w, h) = suggested(&src, to)?;
    if w * h > crate::raster::MAX_GRID_CELLS * 2 {
        return Err(GbError(format!("The grid would have {} × {} cells in EPSG:{}, which is too many.", w, h, to)));
    }
    let dst = mem_dataset::<f32>(w, h, &[vec![f32::NAN; w * h]], gt, to)?;
    dst.rasterband(1).and_then(|mut b| b.set_no_data_value(Some(f64::NAN))).map_err(|e| GbError(format!("Cannot mark the cells without a value: {}", e)))?;
    gdal::raster::reproject(&src, &dst).map_err(|e| GbError(format!("Cannot reproject the grid: {}", e)))?;
    let values = dst
        .rasterband(1)
        .and_then(|b| b.read_as::<f32>((0, 0), (w, h), (w, h), None))
        .map_err(|e| GbError(format!("Cannot read the reprojected grid: {}", e)))?;
    Ok(og_engine::model::Grid { bands: Vec::new(), width: w, height: h, values: values.into_shape_and_vec().1, geotransform: gt, crs: Some(to) })
}

/// A picture in another coordinate system. What lies outside the picture is see-through.
pub fn warp_image(img: &og_engine::model::RasterImage, from: u32, to: u32) -> Result<og_engine::model::RasterImage> {
    if from == to {
        return Ok(img.clone());
    }
    let (w0, h0) = (img.width, img.height);
    let bands: Vec<Vec<u8>> = (0..4).map(|b| img.rgba.iter().skip(b).step_by(4).copied().collect()).collect();
    let src = mem_dataset::<u8>(w0, h0, &bands, img.geotransform, from)?;
    {
        let mut alpha = src.rasterband(4).map_err(|e| GbError(e.to_string()))?;
        alpha.set_color_interpretation(gdal::raster::ColorInterpretation::AlphaBand).map_err(|e| GbError(e.to_string()))?;
    }
    let (gt, w, h) = suggested(&src, to)?;
    if w * h > crate::raster::MAX_PIXELS * 2 {
        return Err(GbError(format!("The picture would be {} × {} pixels in EPSG:{}, which is too large.", w, h, to)));
    }
    let dst = mem_dataset::<u8>(w, h, &[vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h]], gt, to)?;
    {
        let mut alpha = dst.rasterband(4).map_err(|e| GbError(e.to_string()))?;
        alpha.set_color_interpretation(gdal::raster::ColorInterpretation::AlphaBand).map_err(|e| GbError(e.to_string()))?;
    }
    gdal::raster::reproject(&src, &dst).map_err(|e| GbError(format!("Cannot reproject the picture: {}", e)))?;
    let mut rgba = vec![0u8; w * h * 4];
    for b in 0..4 {
        let band = dst
            .rasterband(b + 1)
            .and_then(|x| x.read_as::<u8>((0, 0), (w, h), (w, h), None))
            .map_err(|e| GbError(format!("Cannot read the picture: {}", e)))?;
        for (k, v) in band.into_shape_and_vec().1.into_iter().enumerate() {
            rgba[k * 4 + b] = v;
        }
    }
    Ok(og_engine::model::RasterImage { width: w, height: h, rgba, geotransform: gt })
}

/// Contour lines of a grid, or the areas between them, through GDAL. They are made in a
/// GeoPackage in memory and read from there.
pub fn contours(g: &og_engine::model::Grid, interval: f64, base: f64, areas: bool, name: &str) -> Result<Table> {
    use gdal::vector::LayerAccess;
    let epsg = g.crs.unwrap_or(0);
    let src = if epsg > 0 {
        mem_dataset::<f32>(g.width, g.height, std::slice::from_ref(&g.values), g.geotransform, epsg)?
    } else {
        let mem = gdal::DriverManager::get_driver_by_name("MEM").map_err(|e| GbError(e.to_string()))?;
        let mut ds = mem.create_with_band_type::<f32, _>("", g.width, g.height, 1).map_err(|e| GbError(e.to_string()))?;
        ds.set_geo_transform(&g.geotransform).map_err(|e| GbError(e.to_string()))?;
        let mut band = ds.rasterband(1).map_err(|e| GbError(e.to_string()))?;
        let mut buf = gdal::raster::Buffer::new((g.width, g.height), g.values.clone());
        band.write((0, 0), (g.width, g.height), &mut buf).map_err(|e| GbError(e.to_string()))?;
        ds
    };
    let band = src.rasterband(1).map_err(|e| GbError(e.to_string()))?;
    let path = format!("/vsimem/ographis-contours-{}-{:p}.gpkg", std::process::id(), g as *const _);
    let gpkg = gdal::DriverManager::get_driver_by_name("GPKG").map_err(|e| GbError(format!("GDAL has no GeoPackage support: {}", e)))?;
    let mut ds = gpkg.create_vector_only(&path).map_err(|e| GbError(format!("Cannot make the contour lines: {}", e)))?;
    let srs = if epsg > 0 { gdal::spatial_ref::SpatialRef::from_epsg(epsg).ok() } else { None };
    let ty = if areas { gdal::vector::OGRwkbGeometryType::wkbMultiPolygon } else { gdal::vector::OGRwkbGeometryType::wkbLineString };
    let mut layer = ds
        .create_layer(gdal::vector::LayerOptions { name: "contours", srs: srs.as_ref(), ty, options: None })
        .map_err(|e| GbError(format!("Cannot make the contour lines: {}", e)))?;
    let (lo, hi) = (format!("{}_min", name), format!("{}_max", name));
    let fields: Vec<&str> = if areas { vec![lo.as_str(), hi.as_str()] } else { vec![name] };
    for f in &fields {
        layer.create_defn_fields(&[(f, gdal::vector::OGRFieldType::OFTReal)]).map_err(|e| GbError(e.to_string()))?;
    }
    let mut opts = vec![format!("LEVEL_INTERVAL={}", interval), format!("LEVEL_BASE={}", base)];
    if areas {
        opts.push("POLYGONIZE=YES".into());
        opts.push("ELEV_FIELD_MIN=0".into());
        opts.push("ELEV_FIELD_MAX=1".into());
    } else {
        opts.push("ELEV_FIELD=0".into());
    }
    if let Some(n) = band.no_data_value() {
        opts.push(format!("NODATA={}", n));
    }
    let c_opts: Vec<CString> = opts.iter().map(|o| CString::new(o.as_str()).unwrap()).collect();
    let mut ptrs: Vec<*mut std::ffi::c_char> = c_opts.iter().map(|c| c.as_ptr() as *mut std::ffi::c_char).collect();
    ptrs.push(std::ptr::null_mut());
    // SAFETY: the band and the layer are open and live through the call, and the options are
    // a list ended by null.
    let rv = unsafe { gdal_sys::GDALContourGenerateEx(band.c_rasterband(), layer.c_layer(), ptrs.as_mut_ptr(), None, std::ptr::null_mut()) };
    if rv != gdal_sys::CPLErr::CE_None {
        return Err(GbError("GDAL could not make the contour lines.".into()));
    }
    let mut notes = og_engine::io::Notes::default();
    let mut t = crate::read_layer(&mut layer, &og_engine::io::ReadOpts::default(), &mut notes, &|_, _| true);
    drop(ds);
    let _ = gdal::vsi::unlink_mem_file(&path);
    if let Ok(t) = &mut t {
        // GDAL adds a running number as ID. It says nothing here.
        if let Some(i) = t.field_index("ID").or_else(|| t.field_index("fid")) {
            t.fields.remove(i);
            for f in t.features.iter_mut() {
                f.attrs.remove(i);
            }
        }
    }
    t
}
