use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ConnId, SchemaChange};

/// One side of a schema comparison: a database (a schema, for PostgreSQL) of a connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SchemaSide {
    pub conn: ConnId,
    pub database: String,
}

/// How a table of the source differs from the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TableDiffKind {
    /// Only in the source: the script creates it in the target.
    Missing,
    /// Only in the target: the script drops it.
    Extra,
    /// In both, with different columns, indexes or keys.
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TableDiff {
    pub name: String,
    pub kind: TableDiffKind,
    /// What differs, one line each (`+ column note varchar(255)`, `~ column total: …`).
    pub details: Vec<String>,
}

/// The result of comparing two databases: what to change in the target so it matches the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SchemaDiff {
    pub source: SchemaSide,
    pub target: SchemaSide,
    /// Tables that differ, by name.
    pub tables: Vec<TableDiff>,
    /// Tables that are the same on both sides.
    pub same: u32,
    /// What makes the target match, in the order it runs: foreign keys are dropped first and
    /// added last, around the tables' own changes. Generated columns are only in the details.
    pub changes: Vec<SchemaChange>,
    /// The script for the target, as the target's driver writes it.
    pub statements: Vec<String>,
}
