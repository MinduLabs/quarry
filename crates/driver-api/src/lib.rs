//! The contract every database driver implements.
//!
//! The engine only talks to `dyn Driver` / `dyn Session`; the UI never sees a concrete
//! driver. Every type crossing this boundary is serializable, so a driver can run in its own
//! process (`quarry-driver-proto`).

pub mod diagnose;
mod rows;
#[cfg(feature = "testing")]
pub mod testing;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use quarry_model::{
    Capabilities, ColumnMeta, ConnectionConfig, DialectSpec, DriverId, ErrorLocation, IdentQuote,
    PagingStyle, PlaceholderStyle, SchemaChange, SchemaObject, ServerInfoKind, ServerInfoRow,
    TableColumns, TableInfo, TableRef, Value,
};
use serde::{Deserialize, Serialize};

pub use rows::{RowBatch, RowsError};

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
            row_limit: Some(500),
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
    /// Rows still encoded, as a driver process sends them. Only made inside the engine.
    #[serde(skip)]
    Batch(RowBatch),
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

/// Receives the items of one statement as they arrive, for work that must not outrun its
/// consumer (an export). `accept` is async: a sink that waits (a full queue) makes the driver
/// wait, so the database is read no faster than the sink consumes.
#[async_trait]
pub trait RowSink: Send {
    /// `false` stops the read: the driver returns at once without reading what is left. A
    /// connection stopped that way still has rows in flight, so the caller cancels the statement
    /// through [`Session::cancel_handle`] before closing it.
    async fn accept(&mut self, item: ExecItem) -> bool;
}

/// A plain callback as a [`RowSink`]: it never waits and never stops the read. A driver reads
/// `execute` and `stream` with one loop through it.
pub struct FnSink<'a>(pub ExecSink<'a>);

impl std::fmt::Debug for FnSink<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FnSink")
    }
}

#[async_trait]
impl RowSink for FnSink<'_> {
    async fn accept(&mut self, item: ExecItem) -> bool {
        (self.0)(item);
        true
    }
}

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

    /// The text that goes after the `VALUES` of an insert so a row whose key exists is skipped
    /// or overwrites the table's row, or `None` when this database has no such form here (it has
    /// none, or an overwrite needs a key and `key` is empty). `columns` are the columns the
    /// insert names and `key` the table's primary key, both unquoted. `OnDuplicate::Fail` has no
    /// text: the insert is left as it is.
    fn on_duplicate(
        &self,
        _mode: quarry_model::OnDuplicate,
        _columns: &[String],
        _key: &[String],
    ) -> Option<String> {
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

/// A [`Dialect`] written entirely from a [`DialectSpec`], so the engine never calls into a driver
/// to build SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecDialect {
    spec: DialectSpec,
}

impl SpecDialect {
    pub fn new(spec: DialectSpec) -> Self {
        Self { spec }
    }

    pub fn spec(&self) -> &DialectSpec {
        &self.spec
    }
}

impl Dialect for SpecDialect {
    fn on_duplicate(
        &self,
        mode: quarry_model::OnDuplicate,
        columns: &[String],
        key: &[String],
    ) -> Option<String> {
        use quarry_model::{DuplicateSyntax, OnDuplicate};
        let first = self.quote_ident(columns.first()?);
        let quoted = |name: &String| self.quote_ident(name);
        match (self.spec.on_duplicate, mode) {
            (_, OnDuplicate::Fail) | (DuplicateSyntax::None, _) => None,
            // Setting a column to itself changes nothing but still accepts the duplicate; unlike
            // `INSERT IGNORE`, other errors (a value that does not fit) are still errors.
            (DuplicateSyntax::OnDuplicateKey, OnDuplicate::Skip) => {
                Some(format!(" ON DUPLICATE KEY UPDATE {first} = {first}"))
            }
            (DuplicateSyntax::OnDuplicateKey, OnDuplicate::Update) => {
                let sets: Vec<String> = columns
                    .iter()
                    .map(quoted)
                    .map(|name| format!("{name} = VALUES({name})"))
                    .collect();
                Some(format!(" ON DUPLICATE KEY UPDATE {}", sets.join(", ")))
            }
            (DuplicateSyntax::OnConflict, OnDuplicate::Skip) => {
                Some(" ON CONFLICT DO NOTHING".to_owned())
            }
            (DuplicateSyntax::OnConflict, OnDuplicate::Update) => {
                // The conflict target is the key, so a table without one cannot overwrite.
                if key.is_empty() {
                    return None;
                }
                let sets: Vec<String> = columns
                    .iter()
                    .filter(|name| !key.iter().any(|k| k.eq_ignore_ascii_case(name)))
                    .map(quoted)
                    .map(|name| format!("{name} = EXCLUDED.{name}"))
                    .collect();
                let target: Vec<String> = key.iter().map(quoted).collect();
                if sets.is_empty() {
                    return Some(format!(" ON CONFLICT ({}) DO NOTHING", target.join(", ")));
                }
                Some(format!(
                    " ON CONFLICT ({}) DO UPDATE SET {}",
                    target.join(", "),
                    sets.join(", ")
                ))
            }
        }
    }

    fn quote_ident(&self, ident: &str) -> String {
        let (open, close) = match self.spec.quote {
            IdentQuote::Double => ('"', '"'),
            IdentQuote::Backtick => ('`', '`'),
            IdentQuote::Brackets => ('[', ']'),
        };
        let doubled: String = [close, close].iter().collect();
        format!("{open}{}{close}", ident.replace(close, &doubled))
    }

    fn placeholder(&self, index: usize) -> String {
        match self.spec.placeholder {
            PlaceholderStyle::Question => "?".to_owned(),
            PlaceholderStyle::Numbered => format!("${index}"),
            PlaceholderStyle::AtP => format!("@P{index}"),
        }
    }

    fn select_page(
        &self,
        table: &quarry_model::TableRef,
        filter: Option<&str>,
        order: Option<&str>,
        limit: u64,
        offset: u64,
    ) -> String {
        let mut sql = format!("SELECT * FROM {}", self.qualify(table));
        if let Some(filter) = filter {
            sql.push_str(" WHERE ");
            sql.push_str(filter);
        }
        match self.spec.paging {
            PagingStyle::LimitOffset => {
                if let Some(order) = order {
                    sql.push_str(" ORDER BY ");
                    sql.push_str(order);
                }
                sql.push_str(&format!(" LIMIT {limit}"));
                if offset > 0 {
                    sql.push_str(&format!(" OFFSET {offset}"));
                }
            }
            PagingStyle::OffsetFetch => {
                // OFFSET needs an ORDER BY; without a sort the server picks the order.
                sql.push_str(" ORDER BY ");
                sql.push_str(order.unwrap_or("(SELECT NULL)"));
                sql.push_str(&format!(
                    " OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY"
                ));
            }
        }
        sql
    }

    fn explain(&self, statement: &str, json: bool) -> Option<String> {
        let syntax = self.spec.explain.as_ref()?;
        let keywords = if json {
            syntax.json.as_deref()?
        } else {
            &syntax.table
        };
        Some(format!("{keywords} {statement}"))
    }

    fn literal(&self, value: &Value) -> String {
        let style = self.spec.literals;
        match value {
            Value::Text(text) | Value::Json(text) | Value::Other { text, .. } => {
                let text = if style.backslash_escapes {
                    text.replace('\\', "\\\\")
                } else {
                    text.clone()
                };
                let prefix = if style.national_strings { "N" } else { "" };
                format!("{prefix}'{}'", text.replace('\'', "''"))
            }
            Value::Bool(b) if style.bool_as_int => if *b { "1" } else { "0" }.to_owned(),
            Value::Bytes(bytes) if style.hex_bytes => {
                let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
                format!("0x{hex}")
            }
            other => ansi_literal(other),
        }
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
    fn dialect(&self) -> &SpecDialect;
    /// The statements that make `change`, in order, or `None` when the driver cannot change
    /// the schema. An error says what is wrong with the change (an empty name, …).
    async fn schema_change(&self, _change: &SchemaChange) -> Option<Result<Vec<String>, String>> {
        None
    }
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
    /// Like [`Self::query`] (or [`Self::execute`] when `params` is empty), but into a
    /// [`RowSink`] that can make the read wait or stop it. `opts.row_limit` is `None` here: the
    /// sink decides how much to take.
    async fn stream(
        &mut self,
        _sql: &str,
        _params: &[Value],
        _opts: &ExecOpts,
        _sink: &mut dyn RowSink,
    ) -> Result<()> {
        Err(DriverError::Unsupported("streaming every row"))
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
    /// Primary key columns of a table, in key order; empty for a view or a table without one.
    /// Drivers with a cheaper query than [`Self::table_info`] override this.
    async fn primary_key(&mut self, table: &TableRef) -> Result<Vec<String>> {
        Ok(self.table_info(table).await?.primary_key)
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
    /// Make the session refuse a value the column cannot hold exactly (text in a number
    /// column, a string too long) instead of storing a changed one. Databases that are strict
    /// already keep this default; MySQL, which can be configured to truncate and coerce
    /// silently, overrides it. Used by imports, where a silently changed value is lost data.
    async fn set_strict(&mut self) -> Result<()> {
        Ok(())
    }
    fn cancel_handle(&self) -> SharedCancel;
    async fn close(self: Box<Self>) -> Result<()>;
}

/// The drivers the engine can use.
#[derive(Default)]
pub struct DriverRegistry {
    drivers: Vec<Arc<dyn Driver>>,
}

impl std::fmt::Debug for DriverRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.drivers.iter().map(|d| d.id()))
            .finish()
    }
}

impl DriverRegistry {
    /// Adds `driver`, replacing one with the same id.
    pub fn register(&mut self, driver: Arc<dyn Driver>) {
        let id = driver.id();
        self.drivers.retain(|d| d.id() != id);
        self.drivers.push(driver);
    }

    pub fn get(&self, id: &DriverId) -> Option<Arc<dyn Driver>> {
        self.drivers.iter().find(|d| &d.id() == id).cloned()
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

    fn ansi_spec() -> SpecDialect {
        SpecDialect::new(DialectSpec {
            quote: IdentQuote::Double,
            on_duplicate: quarry_model::DuplicateSyntax::None,
            placeholder: PlaceholderStyle::Numbered,
            paging: PagingStyle::LimitOffset,
            literals: quarry_model::LiteralStyle {
                backslash_escapes: false,
                bool_as_int: false,
                national_strings: false,
                hex_bytes: false,
            },
            explain: None,
        })
    }

    #[test]
    fn spec_dialect_follows_its_spec() {
        let dialect = ansi_spec();
        assert_eq!(dialect.quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(dialect.placeholder(2), "$2");
        assert_eq!(dialect.literal(&Value::Text("a\\b'c".into())), "'a\\b''c'");
        assert_eq!(dialect.literal(&Value::Bool(true)), "TRUE");
        assert_eq!(dialect.explain("SELECT 1", false), None);
        let table = TableRef {
            database: None,
            schema: Some("public".into()),
            name: "t".into(),
        };
        assert_eq!(
            dialect.select_page(&table, Some("\"a\" = $1"), None, 10, 0),
            "SELECT * FROM \"public\".\"t\" WHERE \"a\" = $1 LIMIT 10"
        );
    }

    #[test]
    fn sql_server_syntax_from_its_spec() {
        let dialect = SpecDialect::new(DialectSpec {
            quote: IdentQuote::Brackets,
            on_duplicate: quarry_model::DuplicateSyntax::None,
            placeholder: PlaceholderStyle::AtP,
            paging: PagingStyle::OffsetFetch,
            literals: quarry_model::LiteralStyle {
                backslash_escapes: false,
                bool_as_int: true,
                national_strings: true,
                hex_bytes: true,
            },
            explain: None,
        });
        assert_eq!(dialect.quote_ident("a]b[c"), "[a]]b[c]");
        assert_eq!(dialect.placeholder(3), "@P3");
        assert_eq!(dialect.literal(&Value::Text("it's ✓".into())), "N'it''s ✓'");
        assert_eq!(dialect.literal(&Value::Bytes(vec![0xde, 0x01])), "0xDE01");
        let table = TableRef {
            database: Some("dbo".into()),
            schema: None,
            name: "t".into(),
        };
        assert_eq!(
            dialect.select_page(&table, None, None, 100, 0),
            "SELECT * FROM [dbo].[t] ORDER BY (SELECT NULL) OFFSET 0 ROWS FETCH NEXT 100 ROWS ONLY"
        );
        assert_eq!(
            dialect.select_page(&table, Some("[a] = @P1"), Some("[b] DESC"), 50, 200),
            "SELECT * FROM [dbo].[t] WHERE [a] = @P1 ORDER BY [b] DESC OFFSET 200 ROWS FETCH NEXT 50 ROWS ONLY"
        );
    }

    #[test]
    fn json_plans_need_their_own_syntax() {
        let dialect = SpecDialect::new(DialectSpec {
            explain: Some(quarry_model::ExplainSyntax {
                table: "EXPLAIN".into(),
                json: None,
            }),
            ..ansi_spec().spec().clone()
        });
        assert_eq!(
            dialect.explain("SELECT 1", false).as_deref(),
            Some("EXPLAIN SELECT 1")
        );
        assert_eq!(dialect.explain("SELECT 1", true), None);
    }

    #[test]
    fn duplicate_clauses_follow_the_dialect() {
        use quarry_model::{DuplicateSyntax, OnDuplicate};
        let columns = ["id".to_owned(), "name".to_owned()];
        let key = ["id".to_owned()];
        let with = |syntax| {
            let mut spec = ansi_spec().spec().clone();
            spec.on_duplicate = syntax;
            SpecDialect::new(spec)
        };

        let mysql = with(DuplicateSyntax::OnDuplicateKey);
        assert_eq!(mysql.on_duplicate(OnDuplicate::Fail, &columns, &key), None);
        assert_eq!(
            mysql
                .on_duplicate(OnDuplicate::Skip, &columns, &key)
                .unwrap(),
            " ON DUPLICATE KEY UPDATE \"id\" = \"id\""
        );
        assert_eq!(
            mysql
                .on_duplicate(OnDuplicate::Update, &columns, &[])
                .unwrap(),
            " ON DUPLICATE KEY UPDATE \"id\" = VALUES(\"id\"), \"name\" = VALUES(\"name\")"
        );

        let postgres = with(DuplicateSyntax::OnConflict);
        assert_eq!(
            postgres
                .on_duplicate(OnDuplicate::Skip, &columns, &[])
                .unwrap(),
            " ON CONFLICT DO NOTHING"
        );
        assert_eq!(
            postgres
                .on_duplicate(OnDuplicate::Update, &columns, &key)
                .unwrap(),
            " ON CONFLICT (\"id\") DO UPDATE SET \"name\" = EXCLUDED.\"name\""
        );
        assert_eq!(
            postgres.on_duplicate(OnDuplicate::Update, &columns, &[]),
            None,
            "an overwrite needs a key to match by"
        );

        let none = with(DuplicateSyntax::None);
        assert_eq!(none.on_duplicate(OnDuplicate::Skip, &columns, &key), None);
    }
}
