//! What kind of data a port gives or needs, as far as that can be told before a run: features,
//! a raster or a point cloud. Added in 0.96, so that a connection between ports that cannot go
//! together is pointed out before the model runs. Only what is sure is said: a port that may
//! give either, such as one that passes on what came in, is not judged.

use crate::nodes::{node_type, Params};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Data {
    Features,
    Raster,
    Cloud,
}

impl Data {
    pub fn name(self) -> &'static str {
        match self {
            Data::Features => "features",
            Data::Raster => "a raster",
            Data::Cloud => "a point cloud",
        }
    }
}

const RASTER_FILES: &[&str] = &["tif", "tiff", "asc", "vrt", "img", "jp2", "nc", "png", "jpg", "jpeg"];
const CLOUD_FILES: &[&str] = &["las", "laz", "e57", "copc"];
const FEATURE_FILES: &[&str] =
    &["gpkg", "shp", "geojson", "json", "csv", "kml", "gml", "fgb", "parquet", "xlsx", "ods", "dxf", "dwg", "tab", "mif", "sqlite", "gpx", "osm", "pbf"];

fn category(kind: &str) -> &'static str {
    node_type(kind).map_or("", |t| t.category)
}

/// The kind of data a port of a node gives, when that is sure.
pub fn gives(kind: &str, params: &Params, port: &str) -> Option<Data> {
    match kind {
        "Reader" => {
            let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            // A folder, a pattern or a variable may stand for anything.
            if path.contains('*') || path.contains("$(") {
                return None;
            }
            let ext = path.rsplit('.').next().unwrap_or("");
            // A raster file is read as points, squares or its outline unless it is read as an
            // image or a grid.
            if RASTER_FILES.contains(&ext) {
                let raster_as = params.get("raster_as").and_then(|v| v.as_str()).unwrap_or("points");
                return Some(if matches!(raster_as, "image" | "grid") { Data::Raster } else { Data::Features });
            }
            if CLOUD_FILES.contains(&ext) {
                return Some(Data::Cloud);
            }
            FEATURE_FILES.contains(&ext).then_some(Data::Features)
        }
        "PostgisReader" => Some(Data::Features),
        _ => match (category(kind), port) {
            ("Raster" | "Terrain" | "Point clouds", "Raster" | "Rasters" | "Composite" | "Depth") => Some(Data::Raster),
            ("Terrain", "Image") => Some(Data::Raster),
            ("Raster", "Areas" | "Output" | "Classes") => Some(Data::Features),
            ("Terrain", "Contours" | "Catchments" | "Streams" | "Profiles" | "Lines" | "Hollows" | "Volumes") => Some(Data::Features),
            ("Point clouds", "Points") => Some(Data::Cloud),
            ("Point clouds", "Buildings") => Some(Data::Features),
            ("Network" | "Statistics", _) => Some(Data::Features),
            _ => None,
        },
    }
}

/// The kind of data an input port of a node needs, when that is sure.
pub fn needs(kind: &str, port: &str) -> Option<Data> {
    match (category(kind), port) {
        (_, "Raster" | "Rasters" | "Heights" | "Cost" | "Terrain") if !matches!(kind, "Writer" | "View") => Some(Data::Raster),
        ("Point clouds", "Points" | "Ground") => Some(Data::Cloud),
        ("Point clouds", "Buildings" | "Areas") => Some(Data::Features),
        ("Raster", "Zones" | "Areas" | "Features" | "Extent" | "Items" | "Examples") => Some(Data::Features),
        ("Network" | "Statistics", _) => Some(Data::Features),
        _ => None,
    }
}

/// What is wrong with a connection from a port of one node to a port of another, in words,
/// when they surely do not go together.
pub fn mismatch(from: (&str, &Params, &str), from_name: &str, to: (&str, &str), to_name: &str) -> Option<String> {
    let given = gives(from.0, from.1, from.2)?;
    let wanted = needs(to.0, to.1)?;
    // A ground model on Ground may also be a raster.
    if to.1 == "Ground" && given == Data::Raster {
        return None;
    }
    (given != wanted).then(|| format!("{} gives {} on {}, but {} needs {} on {}.", from_name, given.name(), from.2, to_name, wanted.name(), to.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params(v: serde_json::Value) -> Params {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn ports_that_do_not_go_together_are_told() {
        let shp = params(json!({"path": "roads.shp"}));
        let tif = params(json!({"path": "dem.tif", "raster_as": "grid"}));
        assert!(mismatch(("Reader", &shp, "Output"), "Roads", ("TerrainAnalyzer", "Heights"), "Slope").is_some());
        assert!(mismatch(("Reader", &tif, "Output"), "Terrain", ("TerrainAnalyzer", "Heights"), "Slope").is_none());
        assert!(mismatch(("Reader", &tif, "Output"), "Terrain", ("PointThinner", "Points"), "Thin").is_some());
        // A GeoTIFF read as points is features.
        let points = params(json!({"path": "dem.tif"}));
        assert!(mismatch(("Reader", &points, "Output"), "Terrain", ("TerrainAnalyzer", "Heights"), "Slope").is_some());
        // What is not sure is not judged.
        let any = params(json!({"path": "data/*"}));
        assert!(mismatch(("Reader", &any, "Output"), "All", ("TerrainAnalyzer", "Heights"), "Slope").is_none());
        assert!(mismatch(("Reprojector", &Params::new(), "Reprojected"), "Reproject", ("TerrainAnalyzer", "Heights"), "Slope").is_none());
    }
}
