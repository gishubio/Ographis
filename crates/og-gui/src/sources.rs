//! What the files behind the readers contain: the layers of a file and the files of a
//! folder. Both are looked up on disk, so the answers are kept for a while. The canvas and
//! the forms ask for them in every frame.

use crate::graph::GNode;
use og_engine::io::{Backend, LayerInfo};
use og_engine::nodes::{reader_sources, Sources, P};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How long an answer about a folder is trusted. Files may come and go while the program
/// runs, but listing a large folder on a network drive takes time, and it is done in the
/// thread that draws the window.
const FOLDER_AGE: Duration = Duration::from_secs(10);

pub type Layers = Result<Vec<LayerInfo>, String>;

struct Folder {
    key: String,
    at: Instant,
    sources: Result<Sources, String>,
}

#[derive(Default)]
pub struct Lookup {
    layers: HashMap<PathBuf, (Option<SystemTime>, Layers)>,
    folders: HashMap<String, Folder>,
    /// The layers of web services being listed, in threads, since a service takes a while.
    listing: HashMap<PathBuf, std::sync::mpsc::Receiver<Layers>>,
}

pub fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Can a file with this extension hold several layers? Text files and Shapefiles cannot,
/// and are not opened to find out.
pub fn may_have_layers(ext: &str) -> bool {
    !matches!(ext, "" | "csv" | "txt" | "tsv" | "shp" | "dbf" | "json" | "geojson") && !og_engine::io::is_raster(ext)
}

impl Lookup {
    /// Looks up the layers of a file: tables in a GeoPackage, sheets in a workbook. The
    /// file is only read again when it has changed.
    /// Lists the layers of a web service in a thread, once. `poll` takes the answer in.
    pub fn find_service_layers(&mut self, backend: std::sync::Arc<og_gdal::GdalBackend>, path: &Path) {
        if self.layers.contains_key(path) || self.listing.contains_key(path) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let p = path.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(backend.layers(&p).map_err(|e| e.0));
        });
        self.listing.insert(path.to_path_buf(), rx);
    }

    /// Takes in the layers of services that have been listed. True while some are coming.
    pub fn poll(&mut self) -> bool {
        let done: Vec<(PathBuf, Layers)> = self.listing.iter().filter_map(|(p, rx)| rx.try_recv().ok().map(|l| (p.clone(), l))).collect();
        for (p, l) in done {
            self.listing.remove(&p);
            self.layers.insert(p, (None, l));
        }
        !self.listing.is_empty()
    }

    pub fn find_layers(&mut self, backend: &dyn Backend, path: &Path) {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if self.layers.get(path).is_some_and(|(at, _)| *at == modified) {
            return;
        }
        let found = if may_have_layers(&ext_of(path)) { backend.layers(path).map_err(|e| e.0) } else { Ok(Vec::new()) };
        self.layers.insert(path.to_path_buf(), (modified, found));
    }

    /// Looks up the tables of a database, by its connection. Asked for by the user, since
    /// a server can take a while to answer.
    pub fn find_db_layers(&mut self, backend: &dyn Backend, connection: &str) {
        let found = db_layers(backend, connection);
        self.put_db_layers(connection, found);
    }

    /// Keeps the tables of a database that were listed elsewhere, such as in a thread.
    pub fn put_db_layers(&mut self, connection: &str, found: Layers) {
        self.layers.insert(PathBuf::from(connection), (None, found));
    }

    /// The layers of a file that `find_layers` has looked at.
    pub fn layers(&self, path: &Path) -> Option<&Layers> {
        self.layers.get(path).map(|l| &l.1)
    }

    /// Looks up the files a reader stands for. They are looked up again when its settings
    /// change, and otherwise every ten seconds.
    pub fn find_sources(&mut self, node: &GNode, base_dir: &Path) {
        let text = |k: &str| node.params.get(k).map(|v| v.to_string()).unwrap_or_default();
        let key = format!("{}|{}|{}|{}", base_dir.display(), text("path"), text("pattern"), text("recursive"));
        if self.folders.get(&node.id).is_some_and(|f| f.key == key && f.at.elapsed() < FOLDER_AGE) {
            return;
        }
        let sources = match node.ty() {
            Some(ty) => reader_sources(&P::new(&node.params, ty.params), base_dir).map_err(|e| e.0),
            None => Err("unknown node type".to_string()),
        };
        self.folders.insert(node.id.clone(), Folder { key, at: Instant::now(), sources });
    }

    /// The files of a reader that `find_sources` has looked at.
    pub fn sources(&self, node_id: &str) -> Option<&Result<Sources, String>> {
        self.folders.get(node_id).map(|f| &f.sources)
    }
}

/// The tables of a database with geometry, by its connection. A password is kept out of the
/// error, and the tables of PostGIS itself are left out.
pub fn db_layers(backend: &dyn Backend, connection: &str) -> Layers {
    let found = backend.layers(Path::new(connection)).map_err(|e| {
        let text = e.0.replace(connection, "the database");
        match text.find("password=") {
            Some(at) => text[..at].to_string(),
            None => text,
        }
    });
    found.map(|l| l.into_iter().filter(|x| !x.name.starts_with("topology.") && x.name != "spatial_ref_sys").collect())
}

/// The filter that suits a folder: the kind of data file there is most of. `None` when the
/// folder holds no data files that are recognised.
pub fn guess_pattern(dir: &Path) -> Option<String> {
    const KNOWN: &[&str] =
        &["shp", "gpkg", "csv", "xlsx", "txt", "geojson", "tab", "gml", "kml", "ods", "tif", "fgb", "parquet", "dxf", "sqlite", "ifc", "las", "laz"];
    let mut counts: Vec<(usize, usize)> = KNOWN.iter().enumerate().map(|(i, _)| (i, 0)).collect();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let ext = ext_of(&entry.path());
        if let Some(i) = KNOWN.iter().position(|k| *k == ext) {
            counts[i].1 += 1;
        }
    }
    // Most files wins. A tie goes to the kind that comes first in the list.
    let best = counts.iter().filter(|c| c.1 > 0).max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))?;
    Some(format!("*.{}", KNOWN[best.0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_follows_what_the_folder_holds() {
        let dir = std::env::temp_dir().join(format!("og-gui-guess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(guess_pattern(&dir), None);
        for name in ["a.csv", "b.CSV", "c.csv", "one.shp", "one.dbf", "one.shx", "notes.docx"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        assert_eq!(guess_pattern(&dir).as_deref(), Some("*.csv"));
        for name in ["two.shp", "three.shp"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        // As many of each: the Shapefiles win.
        assert_eq!(guess_pattern(&dir).as_deref(), Some("*.shp"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
