use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ConnId;

/// One query the user ran. Never holds a password: only the SQL text and where it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HistoryEntry {
    pub sql: String,
    pub conn: ConnId,
    pub database: Option<String>,
    #[ts(type = "string")]
    pub ran_at: DateTime<Utc>,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// `None` when it succeeded; the first line of the error otherwise.
    pub error: Option<String>,
}
