//! Web services that Input reads: OGC API Features and WFS through GDAL's own drivers, which
//! follow the pages and ask only for what reaches into a rectangle; and ArcGIS feature
//! services, layer by layer, through GDAL's ESRIJSON driver, which pages too.
//!
//! A service comes as a name: OAPIF:<address>, WFS:<address> or ARCGIS:<address>.

use crate::{clear_messages, drain_messages, epsg_of, field_type, geometry_type_to_name, read_layer};
use gdal::vector::LayerAccess;
use gdal::{Dataset, DatasetOptions, GdalOpenFlags};
use og_engine::io::{LayerInfo, Notes, ReadOpts};
use og_engine::model::{GbError, Result, Table};
use std::path::Path;

pub(crate) enum Service {
    /// The landing page, and the collection the address pointed at, if it did.
    OgcApi {
        base: String,
        collection: Option<String>,
    },
    Wfs {
        url: String,
    },
    /// The service, and the layer the address pointed at, if it did.
    ArcGis {
        service: String,
        layer: Option<u32>,
    },
}

pub(crate) fn service(path: &Path) -> Option<Service> {
    let s = path.to_string_lossy();
    if let Some(u) = s.strip_prefix("OAPIF:") {
        // …/collections/roads/items?… is the collection roads of the landing page before it.
        let plain = u.split('?').next().unwrap_or(u);
        return Some(match plain.find("/collections") {
            Some(i) => {
                let rest = plain[i + "/collections".len()..].trim_matches('/');
                let id = rest.split('/').next().filter(|x| !x.is_empty()).map(|x| x.to_string());
                Service::OgcApi { base: plain[..i].to_string(), collection: id }
            }
            None => Service::OgcApi { base: plain.trim_end_matches('/').to_string(), collection: None },
        });
    }
    if let Some(u) = s.strip_prefix("WFS:") {
        return Some(Service::Wfs { url: u.to_string() });
    }
    if let Some(u) = s.strip_prefix("ARCGIS:") {
        let plain = u.split('?').next().unwrap_or(u).trim_end_matches('/');
        let plain = plain.strip_suffix("/query").unwrap_or(plain);
        let last = plain.rsplit('/').next().unwrap_or("");
        return Some(match last.parse::<u32>() {
            Ok(id) => Service::ArcGis { service: plain[..plain.len() - last.len() - 1].to_string(), layer: Some(id) },
            Err(_) => Service::ArcGis { service: plain.to_string(), layer: None },
        });
    }
    None
}

/// Headers for GDAL's own requests, set for this thread while it reads.
struct HeadersFor;

impl HeadersFor {
    fn set(headers: &[(String, String)]) -> Option<HeadersFor> {
        if headers.is_empty() {
            return None;
        }
        let text = headers.iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<_>>().join("\r\n");
        let _ = gdal::config::set_thread_local_config_option("GDAL_HTTP_HEADERS", &text);
        Some(HeadersFor)
    }
}

impl Drop for HeadersFor {
    fn drop(&mut self) {
        let _ = gdal::config::clear_thread_local_config_option("GDAL_HTTP_HEADERS");
    }
}

fn open(name: &str, shown: &str) -> Result<Dataset> {
    Dataset::open_ex(name, DatasetOptions { open_flags: GdalOpenFlags::GDAL_OF_VECTOR | GdalOpenFlags::GDAL_OF_READONLY, ..Default::default() })
        .map_err(|e| GbError(format!("Cannot read {} as a web service: {}", shown, crate::last_gdal_error().unwrap_or_else(|| e.to_string()))))
}

/// The layers of a service: the collections of OGC API, the feature types of WFS, the
/// layers and tables of an ArcGIS service.
pub(crate) fn layers(path: &Path, headers: &[(String, String)]) -> Result<Vec<LayerInfo>> {
    let _h = HeadersFor::set(headers);
    clear_messages();
    let out = match service(path) {
        Some(Service::OgcApi { base, .. }) => gdal_layers(&format!("OAPIF:{}", base), &base),
        Some(Service::Wfs { url }) => gdal_layers(&format!("WFS:{}", url), &url),
        Some(Service::ArcGis { service, .. }) => arcgis_layers(&service, headers),
        None => Err(GbError("This is not a web service.".into())),
    };
    clear_messages();
    out
}

fn gdal_layers(name: &str, shown: &str) -> Result<Vec<LayerInfo>> {
    let ds = open(name, shown)?;
    Ok(ds
        .layers()
        .map(|l| LayerInfo {
            name: l.name(),
            // Counting would ask the service for every feature.
            count: None,
            crs: l.spatial_ref().as_ref().and_then(epsg_of),
            crs_name: l.spatial_ref().and_then(|s| s.name()),
            geometry: geometry_type_to_name(l.defn().geometry_type()),
            fields: Vec::new(),
        })
        .collect())
}

fn arcgis_layers(service: &str, headers: &[(String, String)]) -> Result<Vec<LayerInfo>> {
    let a = crate::http_get(&format!("{}?f=json", service), headers, 60)?;
    if a.status >= 400 {
        return Err(GbError(format!("The ArcGIS service answered {}.", a.status)));
    }
    let v = og_engine::json::parse(&a.body).map_err(|_| GbError("The ArcGIS service did not answer with JSON.".into()))?;
    if let Some(e) = v.get("error") {
        return Err(GbError(format!("The ArcGIS service says: {}", e.get("message").and_then(|m| m.as_str()).unwrap_or("an error"))));
    }
    let wkid = v.get("spatialReference").and_then(|s| s.get("latestWkid").or_else(|| s.get("wkid"))).and_then(|w| w.as_u64()).map(|w| w as u32);
    let mut out = Vec::new();
    for key in ["layers", "tables"] {
        for l in v.get(key).and_then(|l| l.as_array()).into_iter().flatten() {
            if l.get("subLayerIds").is_some_and(|s| s.is_array()) {
                // A group of layers, not a layer of its own.
                continue;
            }
            let name = l.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let id = l.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            out.push(LayerInfo {
                name: format!("{} {}", id, name),
                count: None,
                crs: wkid.map(|w| if w == 102100 { 3857 } else { w }),
                crs_name: None,
                geometry: l
                    .get("geometryType")
                    .and_then(|g| g.as_str())
                    .map(|g| g.trim_start_matches("esriGeometry").to_string())
                    .unwrap_or_else(|| "no geometry".into()),
                fields: Vec::new(),
            });
        }
    }
    Ok(out)
}

/// Reads one layer of a service.
pub(crate) fn read(path: &Path, opts: &ReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let _h = HeadersFor::set(&opts.headers);
    clear_messages();
    let wanted = opts.layer.as_deref().map(str::trim).filter(|l| !l.is_empty());
    let out = match service(path) {
        Some(Service::OgcApi { base, collection }) => {
            let ds = open(&format!("OAPIF:{}", base), &base)?;
            let name = wanted.map(|s| s.to_string()).or(collection);
            read_named(&ds, name.as_deref(), opts, notes, progress, "collection")
        }
        Some(Service::Wfs { url }) => {
            let ds = open(&format!("WFS:{}", url), &url)?;
            // A type name in the address is the layer, if none is chosen.
            let from_url = url.split('?').nth(1).and_then(|q| {
                q.split('&').find_map(|kv| {
                    let (k, v) = kv.split_once('=')?;
                    matches!(k.to_lowercase().as_str(), "typename" | "typenames").then(|| v.to_string())
                })
            });
            read_named(&ds, wanted.map(|s| s.to_string()).or(from_url).as_deref(), opts, notes, progress, "feature type")
        }
        Some(Service::ArcGis { service, layer }) => {
            // A layer chosen by its name in the list, "3 Roads", or the one in the address.
            let id = match wanted {
                Some(w) => match w.split_whitespace().next().and_then(|n| n.parse::<u32>().ok()) {
                    Some(id) => Some(id),
                    None => arcgis_layers(&service, &opts.headers)?
                        .into_iter()
                        .find(|l| l.name.split_once(' ').is_some_and(|(_, n)| n.eq_ignore_ascii_case(w)))
                        .and_then(|l| l.name.split_whitespace().next().and_then(|n| n.parse().ok())),
                },
                None => layer,
            };
            let id = match id {
                Some(id) => id,
                None => {
                    let all = arcgis_layers(&service, &opts.headers)?;
                    let first = all.first().ok_or_else(|| GbError("The ArcGIS service has no layers.".into()))?;
                    if all.len() > 1 {
                        notes.info(format!("the service has {} layers, and the first, {}, was read. Choose another under Layers", all.len(), first.name));
                    }
                    first.name.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0)
                }
            };
            let query = format!("{}/{}/query?where=1%3D1&outFields=*&returnGeometry=true&f=json", service, id);
            let ds = open(&format!("ESRIJSON:{}", query), &service)?;
            let mut layer = ds.layer(0).map_err(|e| GbError(format!("The ArcGIS layer {} could not be read: {}", id, e)))?;
            read_layer(&mut layer, opts, notes, progress)
        }
        None => Err(GbError("This is not a web service.".into())),
    };
    drain_messages(notes);
    out
}

fn read_named(ds: &Dataset, name: Option<&str>, opts: &ReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool, what: &str) -> Result<Table> {
    let count = ds.layer_count();
    let mut layer = match name {
        Some(n) => ds.layer_by_name(n).map_err(|_| {
            let names: Vec<String> = ds.layers().map(|l| l.name()).take(30).collect();
            GbError(format!("The service has no {} {}. It has: {}.", what, n, names.join(", ")))
        })?,
        None => {
            let l = ds.layer(0).map_err(|_| GbError(format!("The service has no {}s.", what)))?;
            if count > 1 {
                notes.info(format!("the service has {} {}s, and the first, {}, was read. Choose another under Layers", count, what, l.name()));
            }
            l
        }
    };
    let _ = field_type;
    read_layer(&mut layer, opts, notes, progress)
}
