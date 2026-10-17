use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ConnId, TableRef};

/// Open tabs, saved between launches. Results are never saved, only what the user typed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Workspace {
    pub tabs: Vec<SavedTab>,
    /// Index of the active tab in `tabs`.
    pub active: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SavedTab {
    pub title: String,
    pub conn: ConnId,
    pub database: Option<String>,
    pub sql: String,
    /// Set when the tab browses a table opened from the sidebar.
    #[serde(default)]
    pub browse: Option<BrowsePage>,
}

/// One page of a table, as shown in a browse tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BrowsePage {
    pub table: TableRef,
    /// First row of the page, from 0.
    #[ts(type = "number")]
    pub offset: u64,
}
