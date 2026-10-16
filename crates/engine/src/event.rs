use std::time::Duration;

use quarry_model::{ColumnMeta, ConnId, ConnectionConfig, SchemaObject, Value};

use crate::command::{RequestId, TabId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected { server_version: String },
    Disconnected,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warning,
    Error,
}

/// Sự kiện engine gửi về client.
#[derive(Debug, Clone)]
pub enum Event {
    Pong {
        id: u64,
        version: String,
    },
    /// Thông báo chung (hiện dạng toast).
    Notice {
        kind: NoticeKind,
        message: String,
    },
    /// Danh sách connection hiện tại (gửi lại toàn bộ sau mỗi thay đổi).
    Connections(Vec<ConnectionConfig>),
    TestResult {
        request: RequestId,
        result: Result<String, String>,
    },
    ConnState {
        id: ConnId,
        state: ConnState,
    },
    Databases {
        id: ConnId,
        databases: Vec<String>,
    },
    Objects {
        id: ConnId,
        database: String,
        result: Result<Vec<SchemaObject>, String>,
    },

    /// Safe mode: cần người dùng xác nhận trước khi chạy `sql`.
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
    /// Bắt đầu result set thứ `set` (đếm từ 0) trong lần chạy hiện tại.
    ResultColumns {
        tab: TabId,
        set: usize,
        columns: Vec<ColumnMeta>,
    },
    ResultRows {
        tab: TabId,
        set: usize,
        rows: Vec<Vec<Value>>,
    },
    ResultDone {
        tab: TabId,
        set: usize,
        affected: u64,
        rows: u64,
        truncated: bool,
    },
    QueryFinished {
        tab: TabId,
        elapsed: Duration,
    },
    QueryFailed {
        tab: TabId,
        error: String,
        cancelled: bool,
    },
}
