use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ObjectKind;

/// An object in the schema tree (table, view, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SchemaObject {
    pub name: String,
    pub kind: ObjectKind,
}

/// Reference to a table or view. `database` is the MySQL database (or catalog).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TableRef {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub name: String,
}
