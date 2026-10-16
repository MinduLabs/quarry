use std::fmt;

use quarry_model::{ConnId, ConnectionConfig};
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
    Ping { id: u32 },
    /// Read saved connections → `Event::Connections`.
    LoadConnections,
    /// Insert or update a connection (matched by `config.id`).
    /// `password: null` keeps the current password.
    SaveConnection { config: ConnectionConfig, password: Option<Password>, remember_password: bool },
    DeleteConnection { id: ConnId },
    /// Try an unsaved configuration → `Event::TestResult`.
    TestConnection { request: RequestId, config: ConnectionConfig, password: Option<Password> },
    /// Open the metadata session and list databases.
    Connect { id: ConnId },
    Disconnect { id: ConnId },
    /// List tables, views and routines of a database → `Event::Objects`.
    LoadObjects { id: ConnId, database: String },
    /// Run SQL on the tab's session (opened on first use).
    /// On staging/production the engine may answer `Event::ConfirmRequired` instead;
    /// the client asks the user and resends with `confirmed: true`.
    Run { tab: TabId, conn: ConnId, database: Option<String>, sql: String, confirmed: bool },
    Cancel { tab: TabId },
    /// Close the tab's session.
    CloseTab { tab: TabId },
}
