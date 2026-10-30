use std::fmt;

use quarry_model::{
    BrowseQuery, ColumnMeta, ConnId, ConnectionConfig, DriverId, ExportFormat, ExportOptions,
    Favorite, FavoriteId, ImportOptions, RowChange, SchemaChange, ServerInfoKind, TableRef, Value,
    Workspace,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Query tab id, assigned by the client. Each tab owns its own database session.
pub type TabId = u32;
/// Client-assigned id used to match a request with its result event.
pub type RequestId = u32;

/// A password travelling through the command channel. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(export, type = "string")]
pub struct Password(pub String);

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(***)")
    }
}

/// A request from a client (the UI, later a CLI or MCP server) to the engine.
/// Results arrive asynchronously as [`crate::Event`]s.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum Command {
    /// Liveness check → `Event::Pong`.
    Ping {
        id: u32,
    },
    /// Read saved connections → `Event::Connections`.
    LoadConnections,
    /// What the user can connect to, and which of it this build can open → `Event::DataSources`.
    LoadDataSources,
    /// Download the binary that serves `driver` for this machine, check it and load it →
    /// `Event::DriverInstall` while it runs, then `Event::DataSources`.
    InstallDriver {
        driver: DriverId,
    },
    /// Insert or update a connection (matched by `config.id`).
    /// `password: null` keeps the current password; same for `ssh_secret` (SSH password or key
    /// passphrase). `remember_password` applies to both.
    SaveConnection {
        config: ConnectionConfig,
        password: Option<Password>,
        ssh_secret: Option<Password>,
        remember_password: bool,
    },
    DeleteConnection {
        id: ConnId,
    },
    /// Try an unsaved configuration → `Event::TestResult`.
    TestConnection {
        request: RequestId,
        config: ConnectionConfig,
        password: Option<Password>,
        ssh_secret: Option<Password>,
    },
    /// Open the metadata session and list databases.
    Connect {
        id: ConnId,
    },
    Disconnect {
        id: ConnId,
    },
    /// List tables, views and routines of a database → `Event::Objects`.
    LoadObjects {
        id: ConnId,
        database: String,
    },
    /// Column names of every table in a database, for completion → `Event::Columns`.
    LoadColumns {
        id: ConnId,
        database: String,
    },
    /// Columns, keys, indexes and DDL of a table → `Event::TableInfo`.
    /// Connects first when the connection is not open yet.
    LoadTableInfo {
        request: RequestId,
        conn: ConnId,
        table: TableRef,
    },
    /// Server variables or status counters of a connection → `Event::ServerInfo`.
    /// Connects first when the connection is not open yet.
    LoadServerInfo {
        request: RequestId,
        conn: ConnId,
        kind: ServerInfoKind,
    },
    /// Run SQL on the tab's session (opened on first use).
    /// On staging/production the engine may answer `Event::ConfirmRequired` instead;
    /// the client asks the user and resends with `confirmed: true`.
    Run {
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
        confirmed: bool,
    },
    /// Show one page of a table in the tab: the engine builds the `SELECT` for the driver's
    /// dialect, then runs it like `Run` (results arrive as the same events).
    Browse {
        tab: TabId,
        conn: ConnId,
        table: TableRef,
        #[ts(type = "number")]
        limit: u64,
        #[ts(type = "number")]
        offset: u64,
        /// Filters and sort, applied by the database.
        #[serde(default)]
        query: BrowseQuery,
    },
    /// Exact number of rows of `table` that match `query.filters`, on the tab's session (so
    /// `Cancel` stops it) → `Event::RowCount`.
    CountRows {
        tab: TabId,
        conn: ConnId,
        table: TableRef,
        #[serde(default)]
        query: BrowseQuery,
    },
    /// Show the query plan of one statement in the tab, as rows (`json: false`) or as one JSON
    /// document. The statement itself never runs. Results arrive like `Run`.
    Explain {
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
        json: bool,
    },
    /// Start a transaction on the tab's session → `Event::TransactionState`.
    BeginTransaction {
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
    },
    /// Commit (`commit: true`) or roll back the tab's transaction → `Event::TransactionState`.
    EndTransaction {
        tab: TabId,
        commit: bool,
    },
    /// Write grid edits to `table` in one transaction on the tab's session → `ChangesApplied`
    /// or `ChangesFailed`. On production the engine first answers `ChangesConfirmRequired`;
    /// the client resends with `confirmed: true`.
    ApplyChanges {
        tab: TabId,
        conn: ConnId,
        table: TableRef,
        changes: Vec<RowChange>,
        confirmed: bool,
    },
    /// `rows` as one `INSERT` into `table`, in the connection's dialect, for the clipboard
    /// → `Event::InsertSql`. Nothing runs.
    InsertSql {
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    /// What an export of `source` offers → `Event::ExportPrepared`. Reads the source table's
    /// keys, so it may connect first.
    ExportPrepare {
        request: RequestId,
        conn: ConnId,
        source: ExportSource,
    },
    /// The first rows as `format` would write them → `Event::ExportPreview`. Nothing is
    /// written.
    ExportPreview {
        request: RequestId,
        conn: ConnId,
        format: ExportFormat,
        options: ExportOptions,
        columns: Vec<ColumnMeta>,
        rows: Vec<Vec<Value>>,
    },
    /// Write `rows`, which the client holds, to the file `path` → `Event::ExportFinished` or
    /// `Event::ExportFailed`. Exports only read, so safe mode never asks.
    ExportRows {
        request: RequestId,
        conn: ConnId,
        format: ExportFormat,
        options: ExportOptions,
        columns: Vec<ColumnMeta>,
        rows: Vec<Vec<Value>>,
        path: String,
    },
    /// Run `sql` again on a connection of its own and write every row, up to `limit`, to
    /// `path`, without holding them in memory → `Event::ExportProgress`, then
    /// `Event::ExportFinished` or `Event::ExportFailed`. `columns` are the ones the client holds
    /// from the first run. Only a statement that reads is accepted.
    ExportQuery {
        request: RequestId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
        columns: Vec<ColumnMeta>,
        format: ExportFormat,
        options: ExportOptions,
        path: String,
        #[ts(type = "number | null")]
        limit: Option<u64>,
    },
    /// Write a table, with its filter and sort, to `path` like `ExportQuery`: on a connection of
    /// its own, in one read-only transaction, so the file never mixes old and new rows.
    ExportTable {
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        query: BrowseQuery,
        format: ExportFormat,
        options: ExportOptions,
        path: String,
        #[ts(type = "number | null")]
        limit: Option<u64>,
    },
    /// Exact number of rows `ExportTable` would write without a limit → `Event::ExportRowCount`.
    /// Runs on a connection of its own and never delays an export.
    ExportCountRows {
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        query: BrowseQuery,
    },
    /// Stop the export or row count `request`: the statement is cancelled on the server and no
    /// file is left. Answers with `Event::ExportFailed { cancelled: true }`.
    CancelExport {
        request: RequestId,
    },
    /// The first rows of the delimited file `path`, split the way `options` says → `Event::ImportPreview`.
    /// Nothing is written. The client maps the file's columns to the table's from this.
    ImportPreview {
        request: RequestId,
        path: String,
        options: ImportOptions,
    },
    /// Insert every row of the file `path` into `table`, in one transaction on a connection of
    /// its own: a bad row undoes the whole import. `columns` has one entry per file column, the
    /// table column it goes to or `None` to skip it. In production the engine answers
    /// `Event::ImportConfirmRequired` first; resend with `confirmed: true`.
    /// → `Event::ImportProgress`, then `Event::ImportFinished` or `Event::ImportFailed`.
    ImportRun {
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        columns: Vec<Option<String>>,
        path: String,
        options: ImportOptions,
        confirmed: bool,
    },
    /// Stop the import `request`; its transaction is rolled back, so no row is kept. Answers
    /// with `Event::ImportFailed { cancelled: true }`.
    CancelImport {
        request: RequestId,
    },
    /// Change the schema. With `confirmed: false` the engine only answers `Event::SchemaReview`
    /// with the statements it would run; the client shows them and resends with
    /// `confirmed: true`, which runs them on the metadata session → `Event::SchemaChanged`.
    AlterSchema {
        request: RequestId,
        conn: ConnId,
        change: SchemaChange,
        confirmed: bool,
    },
    Cancel {
        tab: TabId,
    },
    /// Close the tab's session.
    CloseTab {
        tab: TabId,
    },
    /// Read pinned tables and saved queries → `Event::Favorites`.
    LoadFavorites,
    /// Insert or replace a favorite (matched by `id`) → `Event::Favorites`.
    SaveFavorite {
        favorite: Favorite,
    },
    DeleteFavorite {
        id: FavoriteId,
    },
    /// Newest queries first, optionally filtered by SQL text and connection → `Event::History`.
    LoadHistory {
        request: RequestId,
        query: Option<String>,
        #[serde(default)]
        conn: Option<ConnId>,
        limit: u32,
    },
    /// Delete the whole query history → `Event::History` with no entries.
    ClearHistory {
        request: RequestId,
    },
    /// Timings measured by the client (renders, …), written to the performance log next to the
    /// engine's own. Ignored when the log is off (`Event::Pong.perf`).
    PerfMarks {
        marks: Vec<PerfMark>,
    },
    /// Read the tabs saved at the last quit → `Event::Workspace`.
    LoadWorkspace,
    /// Save the open tabs → `Event::WorkspaceSaved` with the same `request`.
    SaveWorkspace {
        request: RequestId,
        workspace: Workspace,
    },
}

/// Where the rows of an export come from.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum ExportSource {
    /// The result on the grid: the client holds the rows and sends them with the export.
    Rows { columns: Vec<ColumnMeta> },
    /// A table, with the filter and sort applied to it.
    Table { table: TableRef, query: BrowseQuery },
    /// A typed query that `ExportQuery` would run again for every row. `columns` are the ones of
    /// its result on the grid.
    Query {
        sql: String,
        columns: Vec<ColumnMeta>,
    },
}

/// One client timing for the performance log.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PerfMark {
    /// What was measured, e.g. `render.grid`.
    pub name: String,
    pub ms: f64,
    /// Short context, e.g. `update`. Never SQL or values.
    pub detail: Option<String>,
}
