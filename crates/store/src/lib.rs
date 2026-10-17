//! Local persistence.
//!
//! - [`ConfigStore`]: saved connections in `<config dir>/quarry/connections.toml`.
//!   **Never contains secrets.**
//! - [`SecretStore`]: passwords in the OS keyring — Secret Service / Secret portal on
//!   Linux (works inside Flatpak), Keychain on macOS.
//! - [`FavoritesStore`]: pinned tables and saved queries in `<config dir>/quarry/favorites.toml`.
//! - [`WorkspaceStore`]: open tabs in `<data dir>/quarry/workspace.json`.
//! - [`HistoryStore`]: queries the user ran, in `<data dir>/quarry/history.jsonl`.

mod favorites;
mod history;
mod secrets;
mod workspace;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use quarry_model::ConnectionConfig;
use serde::{Deserialize, Serialize};

pub use favorites::FavoritesStore;
pub use history::HistoryStore;
pub use secrets::{SecretKind, SecretStore};
pub use workspace::WorkspaceStore;

const APP_DIR: &str = "quarry";
const CONNECTIONS_FILE: &str = "connections.toml";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot determine the user's config directory")]
    NoConfigDir,
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: invalid file: {source}")]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("cannot serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("{path}: written by a newer version of Quarry (format {found})")]
    TooNew { path: PathBuf, found: u32 },
    #[error("keyring: {0}")]
    Keyring(String),
    #[error("{path}: invalid file: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// Default config directory: `$XDG_CONFIG_HOME/quarry` on Linux (inside Flatpak:
/// `~/.var/app/<id>/config/quarry`), `~/Library/Application Support/quarry` on macOS.
pub fn default_config_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.config_dir().join(APP_DIR))
        .ok_or(StoreError::NoConfigDir)
}

/// Default data directory for state that is not configuration (open tabs, history):
/// `$XDG_DATA_HOME/quarry` on Linux, `~/Library/Application Support/quarry` on macOS.
pub fn default_data_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_dir().join(APP_DIR))
        .ok_or(StoreError::NoConfigDir)
}

#[derive(Debug, Serialize, Deserialize)]
struct ConnectionsFile {
    version: u32,
    #[serde(default, rename = "connection")]
    connections: Vec<ConnectionConfig>,
}

/// Reads and writes the saved connection list.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory this store reads and writes; other config files live next to it.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn connections_path(&self) -> PathBuf {
        self.dir.join(CONNECTIONS_FILE)
    }

    /// A missing file means no connections yet.
    pub fn load_connections(&self) -> Result<Vec<ConnectionConfig>> {
        let path = self.connections_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let file: ConnectionsFile = toml::from_str(&text).map_err(|source| StoreError::Parse {
            path: path.clone(),
            source: Box::new(source),
        })?;
        if file.version > SCHEMA_VERSION {
            return Err(StoreError::TooNew {
                path,
                found: file.version,
            });
        }
        Ok(file.connections)
    }

    pub fn save_connections(&self, connections: &[ConnectionConfig]) -> Result<()> {
        let file = ConnectionsFile {
            version: SCHEMA_VERSION,
            connections: connections.to_vec(),
        };
        let text = toml::to_string_pretty(&file)?;
        write_atomic(&self.connections_path(), text.as_bytes())
    }
}

/// Write to a temp file, then rename, so a crash never leaves a half-written file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let io_err = |source| StoreError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_err)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut file = fs::File::create(&tmp).map_err(io_err)?;
    file.write_all(bytes).map_err(io_err)?;
    file.sync_all().map_err(io_err)?;
    fs::rename(&tmp, path).map_err(io_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_model::{ConnId, DriverId, Environment, SslMode};

    fn sample(name: &str) -> ConnectionConfig {
        ConnectionConfig {
            id: ConnId::new(),
            name: name.into(),
            driver: DriverId("mysql".into()),
            host: Some("127.0.0.1".into()),
            port: Some(3306),
            database: None,
            user: Some("root".into()),
            path: None,
            ssl: SslMode::Prefer,
            ssh: None,
            environment: Environment::Production,
            read_only: true,
            color: None,
            group: None,
        }
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("quarry-store-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn roundtrip_and_missing_file() {
        let dir = temp_dir();
        let store = ConfigStore::new(&dir);
        assert!(store.load_connections().unwrap().is_empty());

        let list = vec![sample("local"), sample("prod")];
        store.save_connections(&list).unwrap();
        let text = fs::read_to_string(dir.join(CONNECTIONS_FILE)).unwrap();
        assert!(text.contains("version = 1") && !text.contains("password"));
        assert_eq!(store.load_connections().unwrap(), list);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_newer_format() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(CONNECTIONS_FILE), "version = 99\n").unwrap();
        assert!(matches!(
            ConfigStore::new(&dir).load_connections(),
            Err(StoreError::TooNew { .. })
        ));
        fs::remove_dir_all(dir).unwrap();
    }
}
