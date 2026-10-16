use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Environment;

/// ID ổn định của một connection (dùng làm khoá keyring, khoá sync).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Định danh driver: `"postgres"`, `"mysql"`, `"sqlite"`, …
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DriverId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Đường dẫn private key; `None` = dùng ssh-agent hoặc password.
    pub key_path: Option<String>,
}

/// Cấu hình connection lưu trên đĩa. **Không chứa secret** — password nằm trong keyring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: ConnId,
    pub name: String,
    pub driver: DriverId,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    /// Với SQLite: đường dẫn file.
    pub path: Option<String>,
    pub ssl: SslMode,
    pub ssh: Option<SshConfig>,
    pub environment: Environment,
    pub read_only: bool,
    pub color: Option<String>,
    pub group: Option<String>,
}
