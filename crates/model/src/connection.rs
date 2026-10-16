use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::Environment;

/// Stable connection id (keyring key, future sync key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, type = "string")]
pub struct ConnId(pub Uuid);

impl ConnId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConnId {
    fn default() -> Self {
        Self::new()
    }
}

/// Driver id: `"mysql"`, later `"postgres"`, `"sqlite"`, …
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, type = "string")]
pub struct DriverId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Private key path; `None` = ssh-agent or password.
    pub key_path: Option<String>,
}

/// Connection settings as stored on disk. **Never contains secrets** — passwords live in the keyring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConnectionConfig {
    pub id: ConnId,
    pub name: String,
    pub driver: DriverId,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    /// SQLite: file path.
    pub path: Option<String>,
    pub ssl: SslMode,
    pub ssh: Option<SshConfig>,
    pub environment: Environment,
    pub read_only: bool,
    pub color: Option<String>,
    pub group: Option<String>,
}
