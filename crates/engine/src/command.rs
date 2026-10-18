use std::fmt;

use quarry_model::{
    BrowseQuery, ConnId, ConnectionConfig, Favorite, FavoriteId, RowChange, TableRef, Workspace,
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
    /// Newest queries first, optionally filtered by SQL text → `Event::History`.
    LoadHistory {
        request: RequestId,
        query: Option<String>,
        limit: u32,
    },
    /// Delete the whole query history → `Event::History` with no entries.
    ClearHistory {
        request: RequestId,
    },
    /// Read the tabs saved at the last quit → `Event::Workspace`.
    LoadWorkspace,
    /// Save the open tabs → `Event::WorkspaceSaved` with the same `request`.
    SaveWorkspace {
        request: RequestId,
        workspace: Workspace,
    },
}
