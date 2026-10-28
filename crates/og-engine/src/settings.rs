//! What the user sets up once for all models: the connections to databases. They are kept
//! in settings.json in the folder for settings of the user, %APPDATA%\Ographis on Windows,
//! which the program and the command line both read. A model names a connection; the
//! server, the user and the password stay out of the model file.
//!
//! On Windows the passwords, keys and secrets are kept in the Credential Manager of the
//! user, not in the file: the file only names which of them are there. Elsewhere they stay
//! in the file, which only the user may read.

use crate::model::{GbError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Where passwords are kept apart from the settings file.
pub trait Vault: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, secret: &str) -> std::result::Result<(), String>;
    fn delete(&self, key: &str);
}

/// The Credential Manager of Windows, as generic credentials named Ographis/<key>.
struct SystemVault;

impl SystemVault {
    fn entry(key: &str) -> Option<keyring::Entry> {
        keyring::Entry::new("Ographis", key).ok()
    }
}

impl Vault for SystemVault {
    fn get(&self, key: &str) -> Option<String> {
        Self::entry(key)?.get_password().ok()
    }
    fn set(&self, key: &str, secret: &str) -> std::result::Result<(), String> {
        let e = Self::entry(key).ok_or_else(|| "the credential store cannot be reached".to_string())?;
        e.set_password(secret).map_err(|e| e.to_string())
    }
    fn delete(&self, key: &str) {
        if let Some(e) = Self::entry(key) {
            let _ = e.delete_credential();
        }
    }
}

/// A vault the tests put in place of that of the system.
static TEST_VAULT: RwLock<Option<Arc<dyn Vault>>> = RwLock::new(None);

/// Puts a vault in place of that of the system, or with None takes it away again. For tests.
pub fn use_vault(v: Option<Arc<dyn Vault>>) {
    if let Ok(mut g) = TEST_VAULT.write() {
        *g = v;
    }
}

/// The vault passwords are kept in: the Credential Manager on Windows, none elsewhere.
fn vault() -> Option<Arc<dyn Vault>> {
    if let Some(v) = TEST_VAULT.read().ok().and_then(|g| g.clone()) {
        return Some(v);
    }
    if cfg!(windows) && std::env::var_os("OGRAPHIS_SETTINGS_DIR").is_none() {
        Some(Arc::new(SystemVault))
    } else {
        None
    }
}

/// The folder where the program keeps what it remembers between sessions. The environment
/// variable OGRAPHIS_SETTINGS_DIR points it elsewhere, which the tests use; the name it had
/// before 0.75, GEOBOBBO_SETTINGS_DIR, still works.
pub fn settings_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OGRAPHIS_SETTINGS_DIR").or_else(|| std::env::var_os("GEOBOBBO_SETTINGS_DIR")) {
        return Some(PathBuf::from(dir));
    }
    let (base, name, old) = if let Some(appdata) = std::env::var_os("APPDATA") {
        (PathBuf::from(appdata), "Ographis", "GeoBobbo")
    } else if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        (PathBuf::from(config), "ographis", "geobobbo")
    } else {
        (PathBuf::from(std::env::var_os("HOME")?).join(".config"), "ographis", "geobobbo")
    };
    let dir = base.join(name);
    take_over(&base.join(old), &dir);
    Some(dir)
}

/// The program was called GeoBobbo before 0.75. The first time Ographis starts, it takes
/// the settings, the connections, the latest models and the crash log from that folder,
/// which is left as it was.
fn take_over(old: &std::path::Path, new: &std::path::Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if new.exists() || !old.is_dir() {
            return;
        }
        if std::fs::create_dir_all(new).is_err() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(old) {
            for e in entries.flatten() {
                let path = e.path();
                if path.is_file() {
                    let _ = std::fs::copy(&path, new.join(e.file_name()));
                }
            }
        }
    });
}

pub fn settings_file() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("settings.json"))
}

/// A connection to a database, by a name that models use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    /// The kind of database: "postgis" for now.
    #[serde(default = "postgis")]
    pub kind: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: String,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
}

/// A background map of one's own, for the map of the program.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackgroundMap {
    pub name: String,
    /// "xyz": tiles from an address with {z}, {x} and {y}; "wms": pictures from a WMS;
    /// "wmts": tiles from a WMTS.
    #[serde(default = "xyz")]
    pub kind: String,
    /// The address: a template for tiles, or the address of the service.
    #[serde(default)]
    pub url: String,
    /// WMS and WMTS: the layer or layers, separated by commas for WMS.
    #[serde(default)]
    pub layers: String,
    #[serde(default)]
    pub style: String,
    /// Such as image/png. Empty: image/png.
    #[serde(default)]
    pub format: String,
    /// WMTS: the tile matrix set in Web Mercator, such as GoogleMapsCompatible or 3857.
    #[serde(default)]
    pub matrix_set: String,
    /// WMTS: the name of a tile matrix, with {z} for the level. Empty: the level itself.
    #[serde(default)]
    pub matrix: String,
    /// How to log in: "" none, "basic" a user and a password, "key_query" a key in the
    /// address, "key_header" a key in a header.
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub user: String,
    /// The password or the key, kept in the vault.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret: String,
    /// The parameter or the header the key goes in.
    #[serde(default)]
    pub param: String,
    /// Who made the map, written in a corner of it.
    #[serde(default)]
    pub attribution: String,
}

fn xyz() -> String {
    "xyz".to_string()
}

impl BackgroundMap {
    /// The address of a tile in Web Mercator, and the headers to send with it.
    pub fn tile_request(&self, z: u8, x: u32, y: u32) -> (String, Vec<(String, String)>) {
        const HALF: f64 = 20_037_508.342_789_244;
        let format = if self.format.trim().is_empty() { "image/png" } else { self.format.trim() };
        let mut url = match self.kind.as_str() {
            "wms" => {
                let size = 2.0 * HALF / (1u64 << z) as f64;
                let (x0, y1) = (-HALF + x as f64 * size, HALF - y as f64 * size);
                let base = self.url.trim();
                let sep = if base.contains('?') {
                    if base.ends_with('?') || base.ends_with('&') {
                        ""
                    } else {
                        "&"
                    }
                } else {
                    "?"
                };
                format!(
                    "{}{}SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS={}&STYLES={}&CRS=EPSG:3857&BBOX={:.3},{:.3},{:.3},{:.3}&WIDTH=256&HEIGHT=256&FORMAT={}&TRANSPARENT=TRUE",
                    base,
                    sep,
                    crate::web::encode(self.layers.trim()),
                    crate::web::encode(self.style.trim()),
                    x0,
                    y1 - size,
                    x0 + size,
                    y1,
                    crate::web::encode(format)
                )
            }
            "wmts" => {
                let matrix = if self.matrix.trim().is_empty() { z.to_string() } else { self.matrix.trim().replace("{z}", &z.to_string()) };
                let base = self.url.trim();
                if base.contains("{TileMatrix}") || base.contains("{TileRow}") {
                    // A REST address of the service.
                    base.replace("{TileMatrixSet}", self.matrix_set.trim())
                        .replace("{TileMatrix}", &matrix)
                        .replace("{TileRow}", &y.to_string())
                        .replace("{TileCol}", &x.to_string())
                        .replace("{Style}", self.style.trim())
                        .replace("{Layer}", self.layers.trim())
                } else {
                    let sep = if base.contains('?') {
                        if base.ends_with('?') || base.ends_with('&') {
                            ""
                        } else {
                            "&"
                        }
                    } else {
                        "?"
                    };
                    format!(
                        "{}{}SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER={}&STYLE={}&TILEMATRIXSET={}&TILEMATRIX={}&TILEROW={}&TILECOL={}&FORMAT={}",
                        base,
                        sep,
                        crate::web::encode(self.layers.trim()),
                        crate::web::encode(if self.style.trim().is_empty() { "default" } else { self.style.trim() }),
                        crate::web::encode(self.matrix_set.trim()),
                        crate::web::encode(&matrix),
                        y,
                        x,
                        crate::web::encode(format)
                    )
                }
            }
            _ => self.url.trim().replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string()),
        };
        let mut headers = Vec::new();
        let param = self.param.trim();
        match self.auth.as_str() {
            "basic" => headers.push(("Authorization".to_string(), format!("Basic {}", crate::web::base64(&format!("{}:{}", self.user, self.secret))))),
            "key_query" => url = crate::web::set_query(&url, if param.is_empty() { "key" } else { param }, &self.secret),
            "key_header" => headers.push((if param.is_empty() { "X-Api-Key".to_string() } else { param.to_string() }, self.secret.clone())),
            _ => {}
        }
        url = url.replace("{{key}}", &crate::web::encode(&self.secret));
        (url, headers)
    }
}

fn postgis() -> String {
    "postgis".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub connections: Vec<Connection>,
    /// Connections to web APIs: keys, users and OAuth2 clients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apis: Vec<crate::web::ApiConnection>,
    /// The passwords and keys kept in the vault rather than here, as db:<name> and
    /// api:<name>.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vault: Vec<String>,
    /// Background maps of one's own for the map of the program: WMS, WMTS or tiles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<BackgroundMap>,
    /// What other parts of the program keep here, left as it is.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// The settings that a run uses when the program has given them, instead of the file. The
/// program sets them as they are edited; the tests set their own.
static IN_USE: RwLock<Option<Settings>> = RwLock::new(None);

impl Settings {
    /// The settings from the file. A missing file gives none; a broken one is an error, so
    /// that it is not written over.
    pub fn load() -> Result<Settings> {
        match settings_file() {
            Some(file) => Settings::load_from(&file),
            None => Ok(Settings::default()),
        }
    }

    fn load_from(file: &std::path::Path) -> Result<Settings> {
        let mut s: Settings = match std::fs::read_to_string(file) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| GbError(format!("The settings in {} cannot be read: {}", file.display(), e)))?,
            Err(_) => return Ok(Settings::default()),
        };
        if let Some(v) = vault() {
            for c in s.connections.iter_mut() {
                let key = format!("db:{}", c.name);
                if c.password.is_empty() && s.vault.contains(&key) {
                    c.password = v.get(&key).unwrap_or_default();
                }
            }
            for a in s.apis.iter_mut() {
                let key = format!("api:{}", a.name);
                if a.secret.is_empty() && s.vault.contains(&key) {
                    a.secret = v.get(&key).unwrap_or_default();
                }
            }
            for m in s.maps.iter_mut() {
                let key = format!("map:{}", m.name);
                if m.secret.is_empty() && s.vault.contains(&key) {
                    m.secret = v.get(&key).unwrap_or_default();
                }
            }
            // Settings from before 0.9 have their passwords in the file: they move to the vault.
            let in_file = s.connections.iter().any(|c| !c.password.is_empty() && !s.vault.contains(&format!("db:{}", c.name)))
                || s.apis.iter().any(|a| !a.secret.is_empty() && !s.vault.contains(&format!("api:{}", a.name)))
                || s.maps.iter().any(|m| !m.secret.is_empty() && !s.vault.contains(&format!("map:{}", m.name)));
            if in_file {
                let _ = s.write_to(file);
            }
        }
        Ok(s)
    }

    /// The settings as the file holds them: the passwords put in the vault where there is
    /// one, and those no longer used taken out of it.
    fn write(&self) -> Result<()> {
        let Some(file) = settings_file() else { return Err(GbError("There is no folder for settings.".into())) };
        self.write_to(&file)
    }

    fn write_to(&self, file: &std::path::Path) -> Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create {}: {}", dir.display(), e)))?;
        }
        let mut disk = self.clone();
        disk.vault.clear();
        if let Some(v) = vault() {
            // Those the file named before.
            let before: Vec<String> = std::fs::read_to_string(file)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|j| j.get("vault").cloned())
                .and_then(|j| serde_json::from_value(j).ok())
                .unwrap_or_default();
            for c in disk.connections.iter_mut() {
                let key = format!("db:{}", c.name);
                if !c.password.is_empty() && v.set(&key, &c.password).is_ok() {
                    c.password.clear();
                    disk.vault.push(key);
                }
            }
            for a in disk.apis.iter_mut() {
                let key = format!("api:{}", a.name);
                if !a.secret.is_empty() && v.set(&key, &a.secret).is_ok() {
                    a.secret.clear();
                    disk.vault.push(key);
                }
            }
            for m in disk.maps.iter_mut() {
                let key = format!("map:{}", m.name);
                if !m.secret.is_empty() && v.set(&key, &m.secret).is_ok() {
                    m.secret.clear();
                    disk.vault.push(key);
                }
            }
            for old in before.iter().filter(|k| !disk.vault.contains(k)) {
                v.delete(old);
            }
        }
        let text = serde_json::to_string_pretty(&disk).map_err(|e| GbError(e.to_string()))?;
        // Written beside it and moved in place, so that a crash never leaves half a file.
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| GbError(format!("Cannot write {}: {}", tmp.display(), e)))?;
        // Only the user may read it, where the passwords stay in it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, file).map_err(|e| GbError(format!("Cannot write {}: {}", file.display(), e)))
    }

    /// Writes the settings, and makes them the ones runs use.
    pub fn save(&self) -> Result<()> {
        use_settings(Some(self.clone()));
        self.write()
    }

    pub fn connection(&self, name: &str) -> Option<&Connection> {
        let name = name.trim();
        self.connections.iter().find(|c| c.name.trim() == name).or_else(|| self.connections.iter().find(|c| c.name.trim().eq_ignore_ascii_case(name)))
    }
}

/// Makes these the settings runs use, or with None the file again.
pub fn use_settings(s: Option<Settings>) {
    if let Ok(mut g) = IN_USE.write() {
        *g = s;
    }
}

/// The settings runs use: those the program gave, or else the file.
pub fn current() -> Result<Settings> {
    if let Some(s) = IN_USE.read().ok().and_then(|g| g.clone()) {
        return Ok(s);
    }
    Settings::load()
}

/// A connection by its name, or an error that says where connections are made.
pub fn connection(name: &str) -> Result<Connection> {
    let s = current()?;
    s.connection(name).cloned().ok_or_else(|| {
        let known: Vec<&str> = s.connections.iter().map(|c| c.name.as_str()).collect();
        let list = if known.is_empty() { "There are none yet.".to_string() } else { format!("There are: {}.", known.join(", ")) };
        GbError(format!(
            "There is no database connection called {} on this computer. {} Add it under Edit → Settings → Database connections.",
            name.trim(),
            list
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vault in memory, for the test.
    #[derive(Default)]
    struct Memory(std::sync::Mutex<std::collections::HashMap<String, String>>);

    impl Vault for Memory {
        fn get(&self, key: &str) -> Option<String> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn set(&self, key: &str, secret: &str) -> std::result::Result<(), String> {
            self.0.lock().unwrap().insert(key.to_string(), secret.to_string());
            Ok(())
        }
        fn delete(&self, key: &str) {
            self.0.lock().unwrap().remove(key);
        }
    }

    #[test]
    fn passwords_go_to_the_vault_and_not_into_the_file() {
        let dir = std::env::temp_dir().join(format!("og-vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Settings from before, with the password in the file.
        std::fs::write(
            dir.join("settings.json"),
            r#"{"connections": [{"name": "Prod", "host": "db", "password": "hemligt"}], "apis": [{"name": "SCB", "auth": "key_header", "secret": "k3y"}],
                "maps": [{"name": "Ortofoto", "kind": "wms", "url": "https://maps.example.se/wms", "auth": "basic", "user": "m", "secret": "kartl0sen"}]}"#,
        )
        .unwrap();
        let file = dir.join("settings.json");
        let memory = Arc::new(Memory::default());
        use_vault(Some(memory.clone()));
        let s = Settings::load_from(&file).unwrap();
        assert_eq!(s.connections[0].password, "hemligt");
        assert_eq!(s.apis[0].secret, "k3y");
        // Moved to the vault when read.
        let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(!text.contains("hemligt") && !text.contains("k3y") && !text.contains("kartl0sen"), "{}", text);
        assert_eq!(memory.get("map:Ortofoto").as_deref(), Some("kartl0sen"));
        assert_eq!(Settings::load_from(&file).unwrap().maps[0].secret, "kartl0sen");
        assert!(text.contains("db:Prod") && text.contains("api:SCB"));
        assert_eq!(memory.get("db:Prod").as_deref(), Some("hemligt"));
        // Renamed and changed: the old one leaves the vault.
        let mut s2 = s.clone();
        s2.connections[0].name = "Produktion".into();
        s2.connections[0].password = "nytt".into();
        s2.write_to(&file).unwrap();
        assert_eq!(memory.get("db:Prod"), None);
        assert_eq!(memory.get("db:Produktion").as_deref(), Some("nytt"));
        let again = Settings::load_from(&file).unwrap();
        assert_eq!(again.connections[0].password, "nytt");
        // Without a vault the passwords stay in the file, which only the user may read.
        use_vault(None);
        s2.write_to(&file).unwrap();
        assert!(std::fs::read_to_string(dir.join("settings.json")).unwrap().contains("nytt"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("settings.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_keep_what_they_do_not_know() {
        let s: Settings = serde_json::from_str(r#"{"connections": [{"name": "VGR prod", "host": "db"}], "theme": "dark"}"#).unwrap();
        assert_eq!(s.connections[0].kind, "postgis");
        assert_eq!(s.connection("vgr PROD").unwrap().host, "db");
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["theme"], "dark");
    }
}

#[cfg(test)]
mod map_tests {
    use super::*;

    #[test]
    fn a_background_map_asks_for_its_tiles_as_the_service_wants() {
        let wms = BackgroundMap {
            name: "Ortofoto".into(),
            kind: "wms".into(),
            url: "https://maps.example.se/wms?map=foto".into(),
            layers: "ortofoto".into(),
            auth: "basic".into(),
            user: "magnus".into(),
            secret: "hemligt".into(),
            ..Default::default()
        };
        let (url, headers) = wms.tile_request(1, 1, 0);
        assert!(url.starts_with("https://maps.example.se/wms?map=foto&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ortofoto"), "{}", url);
        assert!(url.contains("&CRS=EPSG:3857&BBOX=0.000,0.000,20037508.343,20037508.343&WIDTH=256&HEIGHT=256&FORMAT=image%2Fpng"), "{}", url);
        assert_eq!(headers, [("Authorization".to_string(), format!("Basic {}", crate::web::base64("magnus:hemligt")))]);
        let wmts = BackgroundMap {
            kind: "wmts".into(),
            url: "https://maps.example.se/wmts".into(),
            layers: "topowebb".into(),
            matrix_set: "3857".into(),
            auth: "key_query".into(),
            param: "apikey".into(),
            secret: "abc".into(),
            ..Default::default()
        };
        let (url, _) = wmts.tile_request(5, 17, 9);
        assert!(
            url.contains("REQUEST=GetTile")
                && url.contains("LAYER=topowebb")
                && url.contains("TILEMATRIXSET=3857&TILEMATRIX=5&TILEROW=9&TILECOL=17")
                && url.ends_with("apikey=abc"),
            "{}",
            url
        );
        let rest = BackgroundMap {
            kind: "wmts".into(),
            url: "https://x.se/1.0.0/topo/default/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png".into(),
            matrix_set: "3857".into(),
            matrix: "EPSG:3857:{z}".into(),
            ..Default::default()
        };
        assert_eq!(rest.tile_request(3, 4, 2).0, "https://x.se/1.0.0/topo/default/3857/EPSG:3857:3/2/4.png");
        let xyz = BackgroundMap { url: "https://t.se/{z}/{x}/{y}.png?token={{key}}".into(), auth: "".into(), secret: "s".into(), ..Default::default() };
        assert_eq!(xyz.tile_request(2, 1, 3).0, "https://t.se/2/1/3.png?token=s");
    }
}
