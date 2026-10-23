use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where in a statement the database found an error, as the server reports it. Lines count from
/// 1 within the statement that failed, so a client can find the spot in any text encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorLocation {
    pub line: u32,
    /// The text from the error on, cut short by the server. Empty when the statement ended
    /// before the server expected it to.
    pub near: String,
}
