//! The contract every database driver implements.
//!
//! The engine only talks to `dyn Driver` / `dyn Session`; the UI never sees a concrete
//! driver. Every type crossing this boundary is serializable, so drivers can later move
//! out of process as plugins.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use quarry_model::{
    Capabilities, ColumnMeta, ConnectionConfig, DriverId, SchemaObject, TableInfo, TableRef, Value,
};
use serde::{Deserialize, Serialize};

pub type Result<T, E = DriverError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// A ready-to-show, multi-line message (see the MySQL driver's `diagnose` module).
    #[error("{0}")]
    Connect(String),
    #[error("{0}")]
    Auth(String),
    #[error("query failed: {message}")]
    Query {
        message: String,
        position: Option<usize>,
    },
    #[error("query was cancelled")]
    Cancelled,
    #[error("not supported by this driver: {0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Settings ready to connect: secrets read from the keyring, host/port pointing at the SSH tunnel if any.
#[derive(Clone)]
pub struct ResolvedConfig {
    pub config: ConnectionConfig,
    pub password: Option<String>,
}

// Never print the password.
impl std::fmt::Debug for ResolvedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedConfig")
            .field("config", &self.config)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecOpts {
    /// Maximum rows kept per result set (`None` = unlimited, for export only).
    /// Rows past the limit are still drained from the socket but not sent.
    pub row_limit: Option<u64>,
    /// Rows per batch passed to the `sink`.
    pub batch_size: usize,
}

impl Default for ExecOpts {
    fn default() -> Self {
        Self {
            row_limit: Some(1000),
            batch_size: 500,
        }
    }
}

/// One result item, emitted in order through the `sink` of [`Session::execute`].
/// A script can produce several result sets; each starts with `Columns`
/// (or is only a `SetDone` for statements that return no rows).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecItem {
    Columns(Vec<ColumnMeta>),
    Rows(Vec<Vec<Value>>),
    /// End of a result set.
    SetDone {
        affected: u64,
        rows: u64,
        truncated: bool,
    },
    Notice(String),
    Done {
        elapsed: Duration,
    },
}

/// Receives results. Called synchronously, so it must be fast (e.g. push into a channel).
pub type ExecSink<'a> = &'a mut (dyn FnMut(ExecItem) + Send);

/// Cancels the running query; callable from another task.
pub trait CancelHandle: Send + Sync {
    fn cancel(&self);
}

pub type SharedCancel = Arc<dyn CancelHandle>;

/// Syntax differences between databases.
pub trait Dialect: Send + Sync {
    fn quote_ident(&self, ident: &str) -> String;
    /// Placeholder for parameter number `index` (from 1): `$1` or `?`.
    fn placeholder(&self, index: usize) -> String;
    /// `SELECT * FROM <table> LIMIT .. OFFSET ..`
    fn select_page(&self, table: &quarry_model::TableRef, limit: u64, offset: u64) -> String;
    /// Quoted, qualified table name: `database.schema.name`, skipping the parts that are unset.
    fn qualify(&self, table: &quarry_model::TableRef) -> String {
        [&table.database, &table.schema, &Some(table.name.clone())]
            .into_iter()
            .flatten()
            .map(|part| self.quote_ident(part))
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[async_trait]
pub trait Driver: Send + Sync + 'static {
    fn id(&self) -> DriverId;
    fn capabilities(&self) -> Capabilities;
    fn dialect(&self) -> &dyn Dialect;
    async fn connect(&self, config: &ResolvedConfig) -> Result<Box<dyn Session>>;
}

#[async_trait]
pub trait Session: Send {
    /// Server version, e.g. `8.4.3`.
    fn server_version(&self) -> String;
    /// Run one or more statements, streaming results into `sink`.
    async fn execute(&mut self, sql: &str, opts: &ExecOpts, sink: ExecSink<'_>) -> Result<()>;
    async fn execute_params(&mut self, sql: &str, params: &[Value]) -> Result<u64>;
    async fn list_databases(&mut self) -> Result<Vec<String>>;
    async fn list_objects(&mut self, database: &str) -> Result<Vec<SchemaObject>>;
    /// Columns, keys, indexes and DDL of one table or view.
    async fn table_info(&mut self, _table: &TableRef) -> Result<TableInfo> {
        Err(DriverError::Unsupported("table structure"))
    }
    /// Change the session's default database (`USE`).
    async fn use_database(&mut self, database: &str) -> Result<()>;
    async fn begin(&mut self) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
    async fn rollback(&mut self) -> Result<()>;
    async fn set_read_only(&mut self, read_only: bool) -> Result<()>;
    fn cancel_handle(&self) -> SharedCancel;
    async fn close(self: Box<Self>) -> Result<()>;
}

/// Drivers compiled into the binary.
#[derive(Default)]
pub struct DriverRegistry {
    drivers: Vec<Box<dyn Driver>>,
}

impl std::fmt::Debug for DriverRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.drivers.iter().map(|d| d.id()))
            .finish()
    }
}

impl DriverRegistry {
    pub fn register(&mut self, driver: Box<dyn Driver>) {
        self.drivers.push(driver);
    }

    pub fn get(&self, id: &DriverId) -> Option<&dyn Driver> {
        self.drivers
            .iter()
            .find(|d| &d.id() == id)
            .map(|d| d.as_ref())
    }

    pub fn ids(&self) -> impl Iterator<Item = DriverId> + '_ {
        self.drivers.iter().map(|d| d.id())
    }
}
