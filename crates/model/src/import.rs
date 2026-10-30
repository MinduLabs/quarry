use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What an import does with a row whose key is already in the table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OnDuplicate {
    /// The import stops and keeps nothing.
    #[default]
    Fail,
    /// The row is left out and the table's row stays as it was.
    Skip,
    /// The table's row takes the file's values. This changes existing data.
    Update,
}

/// How a delimited file is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ImportOptions {
    /// A tab for TSV.
    pub delimiter: char,
    /// The first line names the columns and is not data.
    pub has_header: bool,
    /// An empty value is stored as NULL. Off, it is an empty string, which a number or date
    /// column rejects.
    pub is_empty_null: bool,
    pub on_duplicate: OnDuplicate,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            has_header: true,
            is_empty_null: true,
            on_duplicate: OnDuplicate::Fail,
        }
    }
}
