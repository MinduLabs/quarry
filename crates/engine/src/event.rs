use quarry_model::{ColumnMeta, ConnId, ConnectionConfig, SchemaObject, Value};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::command::{RequestId, TabId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum ConnState {
    Connecting,
    Connected { server_version: String },
    Disconnected,
    Failed { error: String },
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
}
