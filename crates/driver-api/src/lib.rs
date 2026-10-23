//! The contract every database driver implements.
//!
//! The engine only talks to `dyn Driver` / `dyn Session`; the UI never sees a concrete
//! driver. Every type crossing this boundary is serializable, so drivers can later move
//! out of process as plugins.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use quarry_model::{
    Capabilities, ColumnMeta, ConnectionConfig, DriverId, ErrorLocation, SchemaChange,
    SchemaObject, ServerInfoKind, ServerInfoRow, TableColumns, TableInfo, TableRef, Value,
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
        location: Option<ErrorLocation>,
    },
    /// The session's connection is gone (network, server restart, killed). The session cannot
    /// be used again; the message says what happened.
    #[error("{0}")]
    ConnectionLost(String),
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
    /// `SELECT * FROM <table> [WHERE ..] [ORDER BY ..] LIMIT .. OFFSET ..`. `filter` and `order`
    /// are clauses built by the engine (identifiers quoted, values as placeholders), without
    /// their keywords.
    fn select_page(
        &self,
        table: &quarry_model::TableRef,
        filter: Option<&str>,
        order: Option<&str>,
        limit: u64,
        offset: u64,
    ) -> String;

    /// `SELECT COUNT(*)` of the rows of `table` that match `filter` (see [`Self::select_page`]).
    fn count_rows(&self, table: &quarry_model::TableRef, filter: Option<&str>) -> String {
        match filter {
            Some(filter) => format!(
                "SELECT COUNT(*) FROM {} WHERE {filter}",
                self.qualify(table)
            ),
            None => format!("SELECT COUNT(*) FROM {}", self.qualify(table)),
        }
    }

    /// The query plan statement for `statement` (as a table, or as JSON), or `None` when the
    /// database has no such form.
    fn explain(&self, _statement: &str, _json: bool) -> Option<String> {
        None
    }
    /// `value` as an SQL literal, for statements the user copies and runs themselves
    /// (`Copy as INSERT`). Statements Quarry runs use parameters instead.
    fn literal(&self, value: &Value) -> String {
        ansi_literal(value)
    }

    /// The statements that make `change`, in order, or `None` when the driver cannot change
    /// the schema. An error says what is wrong with the change (an empty name, …).
    fn schema_change(&self, _change: &SchemaChange) -> Option<Result<Vec<String>, String>> {
        None
    }

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

/// Standard SQL literal: strings in single quotes with `'` doubled, bytes as `X'..'`.
pub fn ansi_literal(value: &Value) -> String {
    let quoted = |text: &str| format!("'{}'", text.replace('\'', "''"));
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) if v.is_finite() => v.to_string(),
        Value::Float(v) => quoted(&v.to_string()),
        Value::Decimal(text) => text.clone(),
        Value::Bytes(bytes) => {
            let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
            format!("X'{hex}'")
        }
        Value::Timestamp(t) => quoted(&t.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
        Value::Text(text) | Value::Json(text) => quoted(text),
        Value::Other { text, .. } => quoted(text),
        other => quoted(&other.display(usize::MAX)),
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
    /// Like [`Self::execute`] for one statement with parameters, streaming its rows.
    async fn query(
        &mut self,
        _sql: &str,
        _params: &[Value],
        _opts: &ExecOpts,
        _sink: ExecSink<'_>,
    ) -> Result<()> {
        Err(DriverError::Unsupported("parameterized queries"))
    }
    async fn list_databases(&mut self) -> Result<Vec<String>>;
    async fn list_objects(&mut self, database: &str) -> Result<Vec<SchemaObject>>;
    /// Column names of every table and view in `database`, for completion.
    async fn list_columns(&mut self, _database: &str) -> Result<Vec<TableColumns>> {
        Err(DriverError::Unsupported("column list"))
    }
    /// Columns, keys, indexes and DDL of one table or view.
    async fn table_info(&mut self, _table: &TableRef) -> Result<TableInfo> {
        Err(DriverError::Unsupported("table structure"))
    }
    /// Server variables or status counters, sorted by name.
    async fn server_info(&mut self, _kind: ServerInfoKind) -> Result<Vec<ServerInfoRow>> {
        Err(DriverError::Unsupported("server information"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_literals() {
        assert_eq!(ansi_literal(&Value::Null), "NULL");
        assert_eq!(ansi_literal(&Value::Int(-3)), "-3");
        assert_eq!(ansi_literal(&Value::Decimal("1.50".into())), "1.50");
        assert_eq!(ansi_literal(&Value::Text("it's".into())), "'it''s'");
        assert_eq!(ansi_literal(&Value::Bytes(vec![0xde, 0x01])), "X'DE01'");
        assert_eq!(ansi_literal(&Value::Float(f64::NAN)), "'NaN'");
    }
}
