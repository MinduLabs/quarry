use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::TableRef;

/// A change to the schema, turned into DDL by the driver's dialect. The client always sees the
/// statements before they run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum SchemaChange {
    CreateDatabase {
        name: String,
    },
    DropDatabase {
        name: String,
    },
    CreateTable {
        table: TableRef,
        columns: Vec<ColumnDef>,
        /// Primary key columns, in key order. Empty for a table without one.
        primary_key: Vec<String>,
    },
    RenameTable {
        table: TableRef,
        new_name: String,
    },
    TruncateTable {
        table: TableRef,
    },
    DropTable {
        table: TableRef,
        /// The object is a view (`DROP VIEW`).
        #[serde(default)]
        view: bool,
    },
    AlterTable {
        table: TableRef,
        ops: Vec<AlterOp>,
    },
}

impl SchemaChange {
    /// Whether the change can delete data or objects.
    pub fn is_destructive(&self) -> bool {
        match self {
            Self::DropDatabase { .. } | Self::TruncateTable { .. } | Self::DropTable { .. } => true,
            Self::AlterTable { ops, .. } => ops.iter().any(AlterOp::is_destructive),
            Self::CreateDatabase { .. } | Self::CreateTable { .. } | Self::RenameTable { .. } => {
                false
            }
        }
    }

    /// The database the change runs in, if any.
    pub fn database(&self) -> Option<&str> {
        match self {
            Self::CreateDatabase { name } | Self::DropDatabase { name } => Some(name),
            Self::CreateTable { table, .. }
            | Self::RenameTable { table, .. }
            | Self::TruncateTable { table }
            | Self::DropTable { table, .. }
            | Self::AlterTable { table, .. } => table.database.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum AlterOp {
    AddColumn {
        column: ColumnDef,
    },
    /// Change a column's definition; a different `column.name` renames it.
    ModifyColumn {
        name: String,
        column: ColumnDef,
    },
    DropColumn {
        name: String,
    },
    AddIndex {
        index: IndexDef,
    },
    DropIndex {
        name: String,
    },
}

impl AlterOp {
    /// Dropping a column loses its data; changing a type can truncate it.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::DropColumn { .. } | Self::ModifyColumn { .. })
    }
}

/// A column as the user defines it. `type_name` and `default` are SQL as typed (`varchar(255)`,
/// `CURRENT_TIMESTAMP`, `'new'`): types cannot be parameters, and the statement is shown before
/// it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ColumnDef {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    /// Default expression; `None` for no default.
    pub default: Option<String>,
    #[serde(default)]
    pub auto_increment: bool,
    /// `ON UPDATE` expression, e.g. `CURRENT_TIMESTAMP`.
    #[serde(default)]
    pub on_update: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IndexDef {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableRef {
        TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        }
    }

    #[test]
    fn destructive_changes() {
        assert!(SchemaChange::TruncateTable { table: table() }.is_destructive());
        assert!(
            SchemaChange::AlterTable {
                table: table(),
                ops: vec![AlterOp::DropColumn { name: "a".into() }],
            }
            .is_destructive()
        );
        assert!(
            !SchemaChange::AlterTable {
                table: table(),
                ops: vec![AlterOp::DropIndex { name: "i".into() }],
            }
            .is_destructive()
        );
        assert!(
            !SchemaChange::RenameTable {
                table: table(),
                new_name: "b".into(),
            }
            .is_destructive()
        );
    }

    #[test]
    fn tagged_json() {
        let change = SchemaChange::DropTable {
            table: table(),
            view: false,
        };
        let json = serde_json::to_string(&change).unwrap();
        assert!(json.starts_with(r#"{"kind":"drop_table""#), "{json}");
        let op: AlterOp = serde_json::from_str(r#"{"op":"drop_index","name":"i"}"#).unwrap();
        assert_eq!(op, AlterOp::DropIndex { name: "i".into() });
    }
}
