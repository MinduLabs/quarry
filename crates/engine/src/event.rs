use quarry_driver_api::RowBatch;
use quarry_model::{
    Capabilities, ColumnMeta, ConnId, ConnectionConfig, DataSource, DriverId, ErrorLocation,
    ExportFormat, Favorite, HistoryEntry, SchemaChange, SchemaObject, ServerInfoKind,
    ServerInfoRow, TableColumns, TableInfo, TableRef, Value, Workspace,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::command::{RequestId, TabId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum ConnState {
    Connecting,
    Connected {
        server_version: String,
        /// What the connection's driver supports; clients enable features from it.
        capabilities: Capabilities,
    },
    Disconnected,
    Failed {
        error: String,
    },
}

/// Where a driver download is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum DriverInstallState {
    /// `total` is `null` when the server does not say the size.
    Downloading {
        #[ts(type = "number")]
        received: u64,
        #[ts(type = "number | null")]
        total: Option<u64>,
    },
    /// Downloaded; its checksum is checked and it is asked what it serves.
    Verifying,
    Installed,
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum NoticeKind {
    Info,
    Warning,
    Error,
}

/// What made a tab's session run a statement; the console filters on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LogSource {
    /// Typed in the editor, or a plan asked for from it.
    Query,
    /// A page of a table, or its row count.
    Page,
    /// Saving grid edits.
    Edit,
    /// Begin, commit or rollback from the toolbar.
    Transaction,
}

/// One statement a tab's session ran, for the tab's console.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogEntry {
    #[ts(type = "string")]
    pub at: chrono::DateTime<chrono::Utc>,
    pub source: LogSource,
    /// The statement, cut after a few kilobytes.
    pub sql: String,
    /// Until the server answered: the first rows, or the end of a statement without rows.
    pub execute_ms: f64,
    /// Receiving the rows after the first ones; `None` when the statement returned no rows.
    pub fetch_ms: Option<f64>,
    /// Rows returned, when the statement returned a result set.
    #[ts(type = "number | null")]
    pub rows: Option<u64>,
    /// Rows changed, when the statement changed any.
    #[ts(type = "number | null")]
    pub affected: Option<u64>,
    /// More rows came than a result keeps.
    pub truncated: bool,
    pub error: Option<String>,
}

/// One format an export offers, and whether it can be used for this source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExportFormatState {
    pub format: ExportFormat,
    /// File extension without the dot, for the save dialog.
    pub extension: String,
    /// A text format: `ExportPreview` can show it.
    pub has_preview: bool,
    /// Spreadsheets: rows after which a sheet is full.
    #[ts(type = "number | null")]
    pub max_rows_per_sheet: Option<u64>,
    pub is_available: bool,
    /// Why the format cannot be used, when `is_available` is false.
    pub reason: Option<String>,
}

/// Something the engine reports to its client.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum Event {
    Pong {
        id: u32,
        version: String,
        /// The performance log is on: the client may send its own timings (`PerfMarks`).
        perf: bool,
    },
    /// General message, shown as a toast.
    Notice {
        kind: NoticeKind,
        message: String,
    },
    /// The full connection list, re-sent after every change.
    Connections {
        connections: Vec<ConnectionConfig>,
    },
    /// Answer to `LoadDataSources`, in display order.
    DataSources {
        sources: Vec<DataSource>,
    },
    /// Progress of `InstallDriver`.
    DriverInstall {
        driver: DriverId,
        state: DriverInstallState,
    },
    /// `error: null` means the connection worked; `server_version` is then set.
    TestResult {
        request: RequestId,
        server_version: Option<String>,
        error: Option<String>,
    },
    ConnState {
        id: ConnId,
        state: ConnState,
    },
    Databases {
        id: ConnId,
        databases: Vec<String>,
    },
    /// Either `objects` or `error` is set.
    Objects {
        id: ConnId,
        database: String,
        objects: Option<Vec<SchemaObject>>,
        error: Option<String>,
    },
    /// Either `columns` or `error` is set.
    Columns {
        id: ConnId,
        database: String,
        columns: Option<Vec<TableColumns>>,
        error: Option<String>,
    },
    /// Either `rows` or `error` is set.
    ServerInfo {
        request: RequestId,
        kind: ServerInfoKind,
        rows: Option<Vec<ServerInfoRow>>,
        error: Option<String>,
    },
    /// Either `info` or `error` is set.
    TableInfo {
        request: RequestId,
        info: Option<TableInfo>,
        error: Option<String>,
    },

    /// Safe mode: the user must confirm before `sql` runs.
    ConfirmRequired {
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
        message: String,
    },
    QueryStarted {
        tab: TabId,
    },
    /// A statement the tab's session ran, sent when it ended: typed queries, table pages, grid
    /// edits and transactions alike.
    StatementLog {
        tab: TabId,
        entry: LogEntry,
    },
    /// Statement `index` (from 0) of the `count` the run sends starts. A typed script runs its
    /// statements one at a time, in the order of `quarry_sql::script`; anything else is one
    /// statement.
    StatementStarted {
        tab: TabId,
        index: u32,
        count: u32,
    },
    /// Start of result set number `set` (from 0) in the current run, produced by statement
    /// `statement` (see `StatementStarted`).
    ResultColumns {
        tab: TabId,
        set: u32,
        statement: u32,
        columns: Vec<ColumnMeta>,
    },
    ResultRows {
        tab: TabId,
        set: u32,
        #[ts(as = "Vec<Vec<Value>>")]
        rows: Rows,
    },
    ResultDone {
        tab: TabId,
        set: u32,
        statement: u32,
        #[ts(type = "number")]
        affected: u64,
        #[ts(type = "number")]
        rows: u64,
        truncated: bool,
    },
    QueryFinished {
        tab: TabId,
        #[ts(type = "number")]
        elapsed_ms: u64,
    },
    /// The run stopped. `statement` is the statement that failed, when the run had several or
    /// the error has a `location` in it; the statements before it stay applied.
    QueryFailed {
        tab: TabId,
        error: String,
        cancelled: bool,
        statement: Option<u32>,
        location: Option<ErrorLocation>,
    },

    /// Answer to `CountRows`: either `count` or `error` is set.
    RowCount {
        tab: TabId,
        #[ts(type = "number | null")]
        count: Option<u64>,
        error: Option<String>,
    },
    /// Whether the tab's session has a transaction open. Sent after `BeginTransaction`,
    /// `EndTransaction`, and a `Run` that began or ended one (`BEGIN`, `COMMIT`, DDL, …).
    /// `error` is set when a begin, commit or rollback failed.
    TransactionState {
        tab: TabId,
        open: bool,
        error: Option<String>,
    },

    /// Safe mode for grid edits: ask the user, then resend `ApplyChanges` with `confirmed: true`.
    ChangesConfirmRequired {
        tab: TabId,
        message: String,
    },
    /// Grid edits were committed. `message` says what changed ("2 rows updated").
    ChangesApplied {
        tab: TabId,
        message: String,
    },
    /// Grid edits were rolled back; nothing was written.
    ChangesFailed {
        tab: TabId,
        error: String,
    },

    /// Answer to `InsertSql`: either `text` or `error` is set.
    InsertSql {
        request: RequestId,
        text: Option<String>,
        error: Option<String>,
    },
    /// Answer to `ExportPrepare`. Either `error` is set (and nothing else), or the rest is.
    ExportPrepared {
        request: RequestId,
        formats: Vec<ExportFormatState>,
        /// The one table the rows come from. `None` for a result of several tables: SQL Inserts
        /// then needs a table named by the user.
        table: Option<TableRef>,
        /// A table source: its columns and the first rows, for the preview. Empty for a result
        /// the client holds.
        columns: Vec<ColumnMeta>,
        rows: Vec<Vec<Value>>,
        /// A query source: what may make the second run differ from the first, or fail.
        warnings: Vec<String>,
        error: Option<String>,
    },
    /// Answer to `ExportPreview`: either `text` or `error` is set.
    ExportPreview {
        request: RequestId,
        text: Option<String>,
        error: Option<String>,
    },
    /// The file is complete at `path`.
    ExportFinished {
        request: RequestId,
        path: String,
        #[ts(type = "number")]
        rows: u64,
        /// Spreadsheet sheets written, 0 for other formats.
        sheets: u32,
        /// Spreadsheet cells whose text was cut to what a cell holds.
        #[ts(type = "number")]
        cut_cells: u64,
    },
    /// An export is under way: rows and bytes written so far, a few times a second at most.
    /// `bytes` is 0 for formats whose size is known only at the end (Excel).
    ExportProgress {
        request: RequestId,
        #[ts(type = "number")]
        rows: u64,
        #[ts(type = "number")]
        bytes: u64,
    },
    /// Answer to `ExportCountRows`: either `count` or `error` is set.
    ExportRowCount {
        request: RequestId,
        #[ts(type = "number | null")]
        count: Option<u64>,
        error: Option<String>,
    },
    /// The export stopped and left no file. `error` says what to do; `cancelled` means the user
    /// asked for it.
    ExportFailed {
        request: RequestId,
        error: String,
        cancelled: bool,
    },
    /// The statements a schema change would run. Nothing ran yet: resend `AlterSchema` with
    /// `confirmed: true` to run them. `warning` says what is risky about it.
    SchemaReview {
        request: RequestId,
        conn: ConnId,
        change: SchemaChange,
        statements: Vec<String>,
        warning: Option<String>,
    },
    /// A schema change ran (`error: null`), failed, or was refused before running.
    SchemaChanged {
        request: RequestId,
        conn: ConnId,
        change: SchemaChange,
        error: Option<String>,
    },

    /// The full favorites list, re-sent after every change.
    Favorites {
        favorites: Vec<Favorite>,
    },
    History {
        request: RequestId,
        entries: Vec<HistoryEntry>,
    },
    /// A query finished and was added to the history.
    HistoryAdded {
        entry: HistoryEntry,
    },

    /// The tabs saved at the last quit (empty when none or unreadable).
    Workspace {
        workspace: Workspace,
    },
    /// The workspace was written (or the write failed, with `error` set).
    WorkspaceSaved {
        request: RequestId,
        error: Option<String>,
    },
}

/// One batch of result rows. Rows from a driver process stay encoded up to the bridge, which
/// writes them as JSON straight from the bytes; the engine never rebuilds them as values.
#[derive(Debug, Clone)]
pub enum Rows {
    Values(Vec<Vec<Value>>),
    Batch(RowBatch),
}

impl Rows {
    /// The rows as values, for code that reads them (tests, clients that are not a webview).
    pub fn values(&self) -> Result<Vec<Vec<Value>>, quarry_driver_api::RowsError> {
        match self {
            Self::Values(rows) => Ok(rows.clone()),
            Self::Batch(batch) => batch.decode(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Values(rows) => rows.len(),
            Self::Batch(batch) => batch.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The same JSON either way: an array of rows of `Value`s.
impl Serialize for Rows {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Values(rows) => rows.serialize(serializer),
            Self::Batch(batch) => batch.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Rows {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::deserialize(deserializer).map(Self::Values)
    }
}
