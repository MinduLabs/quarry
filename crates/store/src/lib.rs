//! Lưu trữ cục bộ.
//!
//! - [`ConfigStore`]: danh sách connection trong `$XDG_CONFIG_HOME/quarry/connections.toml`,
//!   **không chứa secret**.
//! - [`SecretStore`]: password trong keyring hệ thống qua `oo7`
//!   (Secret Service ngoài sandbox, Secret portal trong Flatpak).
//! - Query history (SQLite + FTS5): chưa làm.
//!
//! Xem `docs/architecture/ARCHITECTURE.md` §6.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use quarry_model::{ConnId, ConnectionConfig};
use serde::{Deserialize, Serialize};

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
    Keyring(Box<oo7::Error>),
}

impl From<oo7::Error> for StoreError {
    fn from(err: oo7::Error) -> Self {
        Self::Keyring(Box::new(err))
    }
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// Thư mục config mặc định: `$XDG_CONFIG_HOME/quarry` (trong Flatpak: `~/.var/app/<id>/config/quarry`).
pub fn default_config_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.config_dir().join(APP_DIR))
        .ok_or(StoreError::NoConfigDir)
}

#[derive(Debug, Serialize, Deserialize)]
struct ConnectionsFile {
    version: u32,
    #[serde(default, rename = "connection")]
    connections: Vec<ConnectionConfig>,
}

/// Đọc/ghi danh sách connection.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn connections_path(&self) -> PathBuf {
        self.dir.join(CONNECTIONS_FILE)
    }

    /// File chưa tồn tại → danh sách rỗng.
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

/// Ghi ra file tạm rồi rename, để không bao giờ để lại file hỏng nếu app bị tắt giữa chừng.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let io_err = |source| StoreError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_err)?;
    }
    let tmp = path.with_extension("toml.tmp");
    let mut file = fs::File::create(&tmp).map_err(io_err)?;
    file.write_all(bytes).map_err(io_err)?;
    file.sync_all().map_err(io_err)?;
    fs::rename(&tmp, path).map_err(io_err)
}

/// Password của connection trong keyring. Khoá: `{application: quarry, connection: <uuid>}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SecretStore;

impl SecretStore {
    fn attributes(id: ConnId) -> [(&'static str, String); 2] {
        [
            ("application", APP_DIR.to_owned()),
            ("connection", id.0.to_string()),
        ]
    }

    pub async fn get_password(&self, id: ConnId) -> Result<Option<String>> {
        let keyring = oo7::Keyring::new().await?;
        let items = keyring.search_items(&Self::attributes(id)).await?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        let secret = item.secret().await?;
        Ok(Some(
            String::from_utf8_lossy(secret.as_bytes()).into_owned(),
        ))
    }

    pub async fn set_password(&self, id: ConnId, label: &str, password: &str) -> Result<()> {
        let keyring = oo7::Keyring::new().await?;
        keyring
            .create_item(
                &format!("Quarry: {label}"),
                &Self::attributes(id),
                oo7::Secret::text(password),
                true,
            )
            .await?;
        Ok(())
    }

    pub async fn delete_password(&self, id: ConnId) -> Result<()> {
        let keyring = oo7::Keyring::new().await?;
        keyring.delete(&Self::attributes(id)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_model::{DriverId, Environment, SslMode};

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

    #[test]
    fn roundtrip_and_missing_file() {
        let dir = std::env::temp_dir().join(format!("quarry-store-{}", uuid::Uuid::new_v4()));
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
        let dir = std::env::temp_dir().join(format!("quarry-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(CONNECTIONS_FILE), "version = 99\n").unwrap();
        assert!(matches!(
            ConfigStore::new(&dir).load_connections(),
            Err(StoreError::TooNew { .. })
        ));
        fs::remove_dir_all(dir).unwrap();
    }
}
