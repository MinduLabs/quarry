use quarry_model::{
    Capabilities, ColumnMeta, ConnId, ConnectionConfig, Favorite, HistoryEntry, SchemaChange,
    SchemaObject, ServerInfoKind, ServerInfoRow, TableColumns, TableInfo, Value, Workspace,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum NoticeKind {
    Info,
    Warning,
    Error,
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
    /// Start of result set number `set` (from 0) in the current run.
    ResultColumns {
        tab: TabId,
        set: u32,
        columns: Vec<ColumnMeta>,
    },
    ResultRows {
        tab: TabId,
        set: u32,
        rows: Vec<Vec<Value>>,
    },
    ResultDone {
        tab: TabId,
        set: u32,
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
    QueryFailed {
        tab: TabId,
        error: String,
        cancelled: bool,
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
