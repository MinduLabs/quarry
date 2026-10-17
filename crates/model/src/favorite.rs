use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::ConnId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, type = "string")]
pub struct FavoriteId(pub Uuid);

impl FavoriteId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for FavoriteId {
    fn default() -> Self {
        Self::new()
    }
}

/// A pinned table or saved query, listed in the sidebar's Favorites pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Favorite {
    pub id: FavoriteId,
    pub name: String,
    pub conn: ConnId,
    pub database: Option<String>,
    pub item: FavoriteItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum FavoriteItem {
    Table { table: String },
    Query { sql: String },
}
