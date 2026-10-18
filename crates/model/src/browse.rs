use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which rows of a table a browse tab shows, and in what order. The engine turns this into a
/// parameterized `WHERE` / `ORDER BY`; values never enter the SQL text.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BrowseQuery {
    /// All filters must match (`AND`).
    #[serde(default)]
    pub filters: Vec<ColumnFilter>,
    /// Sort keys, most significant first.
    #[serde(default)]
    pub sort: Vec<SortKey>,
}

impl BrowseQuery {
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty() && self.sort.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ColumnFilter {
    pub column: String,
    pub op: FilterOp,
    /// The operand as typed. Ignored by `is_null` / `is_not_null`; split on commas by `in`.
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FilterOp {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Contains,
    NotContains,
    StartsWith,
    In,
    IsNull,
    IsNotNull,
}

impl FilterOp {
    /// Whether the operator reads `value`.
    pub fn takes_value(self) -> bool {
        !matches!(self, Self::IsNull | Self::IsNotNull)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SortKey {
    pub column: String,
    pub descending: bool,
}
