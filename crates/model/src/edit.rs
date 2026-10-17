use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Value;

/// A column and its value, used for row keys and for new values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CellValue {
    pub column: String,
    pub value: Value,
}

/// One pending change to a table row. `key` holds the row's primary key as it was loaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum RowChange {
    Update {
        key: Vec<CellValue>,
        set: Vec<CellValue>,
    },
    /// Columns left out get their default value.
    Insert {
        values: Vec<CellValue>,
    },
    Delete {
        key: Vec<CellValue>,
    },
}
