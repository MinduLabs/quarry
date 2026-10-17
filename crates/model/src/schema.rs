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

/// Structure of a table or view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TableInfo {
    pub table: TableRef,
    pub columns: Vec<ColumnInfo>,
    /// Primary key columns, in key order. Empty when the table has none.
    pub primary_key: Vec<String>,
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKey>,
    /// `CREATE TABLE` / `CREATE VIEW` statement, when the driver can produce it.
    pub ddl: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ColumnInfo {
    pub name: String,
    /// Full type as declared, e.g. `varchar(255)` or `int unsigned`.
    pub type_name: String,
    pub nullable: bool,
    /// Default expression as the server reports it (`NULL` default is `None`).
    pub default: Option<String>,
    /// Extra attributes, e.g. `auto_increment`.
    pub extra: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IndexInfo {
    pub name: String,
    /// Indexed columns in order; an expression index shows its expression.
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub references: TableRef,
    pub referenced_columns: Vec<String>,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}
