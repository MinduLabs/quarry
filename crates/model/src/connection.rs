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

/// Driver id: `"mysql"`, `"mariadb"`, later `"postgres"`, `"sqlite"`, …
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

/// How the SSH tunnel signs in. The password or key passphrase lives in the keyring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SshAuth {
    /// Keys loaded in the running ssh-agent (`SSH_AUTH_SOCK`).
    #[default]
    Agent,
    /// A private key file (`key_path`), optionally protected by a passphrase.
    Key,
    Password,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SshConfig {
    /// A host name, an IP address or a `~/.ssh/config` alias.
    pub host: String,
    /// 22 lets `~/.ssh/config` choose the port.
    pub port: u16,
    /// Empty: `~/.ssh/config`'s `User`, else the local user name.
    pub user: String,
    /// Absent in files written before SSH tunnels existed.
    #[serde(default)]
    pub auth: SshAuth,
    /// Private key path for `SshAuth::Key`; `~/` is the home directory. Empty: the alias's
    /// `IdentityFile`.
    pub key_path: Option<String>,
    /// Jump hosts to go through first, as OpenSSH's `ProxyJump`: `[user@]host[:port]`, comma
    /// separated. Empty: the alias's `ProxyJump`. Absent in files written before jump hosts.
    #[serde(default)]
    pub jump: Option<String>,
}

/// How the database user signs in. A password lives in the keyring; AWS IAM needs none.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum DbAuth {
    #[default]
    Password,
    /// A short-lived token signed with the user's AWS credentials, made fresh for every session.
    /// Amazon RDS and Aurora (MySQL, MariaDB, PostgreSQL).
    AwsIam {
        /// Profile in `~/.aws/config`; `None` uses the default credential chain.
        profile: Option<String>,
        /// `None` reads it from the RDS host name, then from the profile.
        region: Option<String>,
    },
}

impl DbAuth {
    pub fn is_password(&self) -> bool {
        matches!(self, Self::Password)
    }
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
    /// Absent in files written before AWS IAM sign-in existed, and not written for a password,
    /// so those files stay as they were.
    #[serde(default, skip_serializing_if = "DbAuth::is_password")]
    pub auth: DbAuth,
}
