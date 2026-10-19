use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which list of server settings to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ServerInfoKind {
    /// Configuration (`SHOW VARIABLES` on MySQL).
    Variables,
    /// Counters since the server started (`SHOW STATUS` on MySQL).
    Status,
}

/// One setting or counter, as the server prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServerInfoRow {
    pub name: String,
    pub value: String,
}
