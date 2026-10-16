use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Metadata of a result-set column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ColumnMeta {
    pub name: String,
    /// Database type name (`INT`, `VARCHAR`, …).
    pub type_name: String,
    pub nullable: Option<bool>,
    /// Source table/column when the driver knows them; decides whether a column is editable.
    pub source_table: Option<String>,
    pub source_column: Option<String>,
}
