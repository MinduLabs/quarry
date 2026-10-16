use serde::{Deserialize, Serialize};

use crate::ObjectKind;

/// Một object trong cây schema (bảng, view, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaObject {
    pub name: String,
    pub kind: ObjectKind,
}

/// Tham chiếu tới một bảng/view. `database` là database (MySQL) hoặc catalog.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableRef {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub name: String,
}
