//! What Ographis reads and writes, and with what: GDAL, or code of its own, with the
//! libraries it uses. For the page about the formats that goes with the program.

/// One format or kind of source.
pub struct Format {
    pub name: &'static str,
    /// The file extensions, or how it is named, such as a database.
    pub files: &'static str,
    /// What reads it, or empty when it is not read.
    pub read: &'static str,
    /// What writes it, or empty when it is not written.
    pub write: &'static str,
    pub note: &'static str,
}

const GDAL: &str = "GDAL";
const OWN: &str = "Ographis";

pub const FORMATS: &[Format] = &[
    Format { name: "GeoPackage", files: ".gpkg", read: GDAL, write: GDAL, note: "Layers in one file; a layer that is written replaces only itself" },
    Format { name: "Shapefile", files: ".shp", read: GDAL, write: GDAL, note: "A stream with several geometry types is written as several files" },
    Format { name: "GeoJSON", files: ".geojson", read: GDAL, write: GDAL, note: "" },
    Format { name: "FlatGeobuf", files: ".fgb", read: GDAL, write: GDAL, note: "" },
    Format { name: "SpatiaLite", files: ".sqlite, .db", read: GDAL, write: "", note: "" },
    Format { name: "PostGIS", files: "tables in PostgreSQL", read: GDAL, write: GDAL, note: "With a primary key, schema and geometry type as asked" },
    Format {
        name: "SQL Server",
        files: "tables in Microsoft SQL Server",
        read: GDAL,
        write: GDAL,
        note: "Through ODBC, with geometry, a spatial index and a primary key; with a password or the login of Windows",
    },
    Format {
        name: "OpenStreetMap from Overpass",
        files: "an Overpass server on the web",
        read: OWN,
        write: "",
        note: "Within a rectangle, made smaller when it is larger than allowed",
    },
    Format {
        name: "GML, KML, MapInfo, File geodatabase",
        files: ".gml, .kml, .tab, .gdb",
        read: GDAL,
        write: "",
        note: "As far as the GDAL that is installed reads them",
    },
    Format {
        name: "OpenStreetMap",
        files: ".osm.pbf, .osm",
        read: GDAL,
        write: "",
        note: "Points, lines, areas and relations, with the common tags as attributes",
    },
    Format {
        name: "Vector tiles (Mapbox Vector Tiles)",
        files: ".mbtiles, .pmtiles, a folder of tiles, {z}/{x}/{y}.pbf on the web",
        read: "GDAL and Ographis",
        write: "",
        note: "As MapLibre draws maps from, at a zoom level; the pieces of a feature that the tiles cut apart are put together again. PMTiles on the web are read where they are. Not yet the newer MapLibre Tile (MLT)",
    },
    Format { name: "OGC API Features, WFS", files: "web services", read: GDAL, write: "", note: "Page by page, only what lies in a rectangle if asked" },
    Format { name: "ArcGIS feature services", files: "web services", read: "GDAL (ESRIJSON)", write: "", note: "Layer by layer" },
    Format {
        name: "CAD drawings",
        files: ".dxf",
        read: "GDAL and Ographis",
        write: "",
        note: "Layers, colours, line types, texts and block attributes as attributes; the header by Ographis",
    },
    Format { name: "CSV and text", files: ".csv, .txt, .tsv", read: OWN, write: OWN, note: "Coordinates or WKT in columns, encodings and separators" },
    Format {
        name: "Excel",
        files: ".xlsx",
        read: GDAL,
        write: "Ographis (umya-spreadsheet)",
        note: "Written with a font, a locked top row, widths and colours",
    },
    Format { name: "LibreOffice Calc", files: ".ods", read: GDAL, write: "Ographis (spreadsheet-ods)", note: "As Excel" },
    Format {
        name: "GeoParquet",
        files: ".parquet",
        read: "Ographis (Apache Parquet)",
        write: "",
        note: "Uncompressed, Snappy, Zstandard, LZ4, gzip and Brotli",
    },
    Format {
        name: "JSON",
        files: ".json, web addresses",
        read: OWN,
        write: "Ographis (Build JSON)",
        note: "Flattened, exploded and picked by path; JSON-stat from statistics offices",
    },
    Format {
        name: "GeoTIFF and other rasters",
        files: ".tif, .jp2, .asc and others",
        read: GDAL,
        write: GDAL,
        note: "Cloud Optimized GeoTIFF over the web; grids of values and pictures",
    },
    Format {
        name: "PNG and JPEG pictures",
        files: ".png, .jpg",
        read: GDAL,
        write: GDAL,
        note: "Maps, charts and 3D scenes, with a world file when they have coordinates",
    },
    Format { name: "PDF", files: ".pdf", read: "", write: GDAL, note: "A map on a page of a size, or a GeoSpatial PDF with layers" },
    Format {
        name: "IFC building models",
        files: ".ifc",
        read: OWN,
        write: "",
        note: "IFC2x3 and IFC4: extrusions, breps, advanced breps with curved surfaces, openings and property sets",
    },
    Format {
        name: "CityGML and CityJSON",
        files: ".gml, .city.json, .jsonl",
        read: OWN,
        write: "",
        note: "Buildings and other city objects in their levels of detail",
    },
    Format { name: "OBJ", files: ".obj", read: OWN, write: OWN, note: "With materials and textures" },
    Format { name: "glTF", files: ".gltf, .glb", read: OWN, write: OWN, note: "Written as .glb around an origin kept in the file" },
    Format { name: "3D Tiles", files: "tileset.json", read: OWN, write: "", note: "Batched and instanced models, from a folder or the web" },
    Format { name: "Point clouds", files: ".las, .laz, .copc.laz", read: "Ographis (las, laz-rs)", write: "", note: "COPC read from the web part by part" },
    Format {
        name: "Charts as web pages",
        files: ".html",
        read: "",
        write: OWN,
        note: "From Draw chart: the values under the pointer, sorted as the reader wants",
    },
    Format { name: "PowerPoint", files: ".pptx", read: "", write: OWN, note: "A slide for each picture, from Slides" },
    Format { name: "LibreOffice Impress", files: ".odp", read: "", write: OWN, note: "As PowerPoint" },
    Format { name: "Zip files", files: ".zip", read: OWN, write: OWN, note: "Read: the files in them. Written: what the outputs of a model wrote" },
    Format { name: "Pictures of models", files: ".png, .jpg, .pdf", read: "", write: OWN, note: "The canvas of a model, from the File menu or og picture" },
];

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The formats as text, for the command line.
pub fn text() -> String {
    let mut out = String::new();
    let w = FORMATS.iter().map(|f| f.name.chars().count()).max().unwrap_or(10);
    out.push_str(&format!("{:w$}  {:28}  {:28}  {}\n", "Format", "Reads", "Writes", "Files", w = w));
    for f in FORMATS {
        let r = if f.read.is_empty() { "–" } else { f.read };
        let wr = if f.write.is_empty() { "–" } else { f.write };
        out.push_str(&format!("{:w$}  {:28}  {:28}  {}\n", f.name, r, wr, f.files, w = w));
    }
    out
}

/// The formats as a web page of its own, to go with the program.
pub fn html(version: &str, gdal: &str) -> String {
    let mut rows = String::new();
    for f in FORMATS {
        let cell = |s: &str| {
            if s.is_empty() {
                "<td class=\"no\">–</td>".to_string()
            } else {
                let class = if s.starts_with("GDAL") { "gdal" } else { "own" };
                format!("<td><span class=\"tag {}\">{}</span></td>", class, esc(s))
            }
        };
        rows.push_str(&format!(
            "<tr><th scope=\"row\">{}<div class=\"files\">{}</div></th>{}{}<td class=\"note\">{}</td></tr>\n",
            esc(f.name),
            esc(f.files),
            cell(f.read),
            cell(f.write),
            esc(f.note)
        ));
    }
    let reads = FORMATS.iter().filter(|f| !f.read.is_empty()).count();
    let writes = FORMATS.iter().filter(|f| !f.write.is_empty()).count();
    let own = FORMATS.iter().filter(|f| f.read.starts_with(OWN) || f.write.starts_with(OWN)).count();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Ographis formats</title>
<style>
:root {{ --bg: #f4f7f6; --surface: #ffffff; --ink: #14201d; --muted: #55645f; --line: #c6d1cd; --gdal: #dbe7f6; --gdal-ink: #1c5596; --own: #d9efe1; --own-ink: #1f6a3d; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #0f1514; --surface: #1b2423; --ink: #e3ebe8; --muted: #97a6a2; --line: #2d3937; --gdal: #16263c; --gdal-ink: #8db8ec; --own: #163222; --own-ink: #7bcb9a; }} }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 15px/1.45 "Segoe UI", system-ui, sans-serif; }}
main {{ max-width: 1100px; margin: 0 auto; padding: 28px 16px 40px; }}
h1 {{ margin: 0 0 4px; font-size: 1.8em; }}
p.lead {{ color: var(--muted); margin: 0 0 18px; max-width: 70ch; }}
.sum {{ display: flex; gap: 12px; flex-wrap: wrap; margin-bottom: 18px; }}
.sum div {{ background: var(--surface); border: 1px solid var(--line); border-radius: 8px; padding: 8px 14px; }}
.sum b {{ font-size: 1.4em; display: block; }}
.wrap {{ overflow-x: auto; background: var(--surface); border: 1px solid var(--line); border-radius: 10px; }}
table {{ border-collapse: collapse; width: 100%; min-width: 720px; }}
th, td {{ text-align: left; vertical-align: top; padding: 9px 12px; border-bottom: 1px solid var(--line); }}
thead th {{ position: sticky; top: 0; background: var(--surface); font-size: .85em; color: var(--muted); text-transform: uppercase; letter-spacing: .04em; }}
tbody th {{ font-weight: 600; }}
.files {{ font-weight: 400; color: var(--muted); font-size: .85em; }}
.tag {{ display: inline-block; padding: 1px 8px; border-radius: 10px; font-size: .85em; white-space: nowrap; }}
.gdal {{ background: var(--gdal); color: var(--gdal-ink); }}
.own {{ background: var(--own); color: var(--own-ink); }}
td.no {{ color: var(--muted); }}
td.note {{ color: var(--muted); font-size: .9em; }}
footer {{ color: var(--muted); font-size: .85em; margin-top: 14px; }}
</style>
</head>
<body>
<main>
<h1>What Ographis reads and writes</h1>
<p class="lead">Each format with what handles it: <span class="tag gdal">GDAL</span>, the geodata library installed with the program, or <span class="tag own">Ographis</span>, code of the program's own, with the library it uses in brackets.</p>
<div class="sum"><div><b>{reads}</b>read</div><div><b>{writes}</b>written</div><div><b>{own}</b>by Ographis itself</div></div>
<div class="wrap">
<table>
<thead><tr><th scope="col">Format</th><th scope="col">Reads</th><th scope="col">Writes</th><th scope="col">Notes</th></tr></thead>
<tbody>
{rows}</tbody>
</table>
</div>
<footer>Ographis {version} · {gdal}. Which further formats GDAL reads depends on how it was built; <code>og info</code> tells whether a file can be read.</footer>
</main>
</body>
</html>
"#,
        reads = reads,
        writes = writes,
        own = own,
        rows = rows,
        version = esc(version),
        gdal = esc(gdal)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_the_program_offers_is_on_the_page() {
        // What a writer can be told to write, and what an output can, by extension.
        let page = html("0", "GDAL");
        let listed = |ext: &str| FORMATS.iter().any(|f| f.files.split(", ").any(|x| x.trim_start_matches('.') == ext) && !f.write.is_empty());
        for (ext, _) in crate::nodes::WRITER_FORMATS.iter().chain(crate::nodes::OUTPUT_FORMATS) {
            match *ext {
                "" | "geopdf" | "postgis" | "tif" => {}
                e => assert!(listed(e), "{} is not on the page", e),
            }
        }
        assert!(page.contains("<td><span class=\"tag gdal\">GDAL</span></td>") && page.contains("prefers-color-scheme: dark"));
        assert!(page.contains("SQL Server") && page.contains("Overpass"));
        // Each kind of input named for the user has a row.
        for item in crate::nodes::READ_FORMATS {
            let first = item.split(['(', ',']).next().unwrap().trim().to_lowercase();
            let key = first.split_whitespace().next().unwrap().to_string();
            assert!(
                FORMATS.iter().any(|f| f.name.to_lowercase().contains(&key) || f.files.to_lowercase().contains(&key))
                    || ["files", "zip", "postgis"].contains(&key.as_str()),
                "{} is not on the page",
                item
            );
        }
    }
}
