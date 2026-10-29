use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The SQL syntax of a database, as data. A driver describes its dialect with it, so the engine
/// writes statements without calling into the driver, which may run in another process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DialectSpec {
    /// Quotes identifiers on both sides; doubled inside a name.
    pub quote: char,
    pub placeholder: PlaceholderStyle,
    pub literals: LiteralStyle,
    /// `None` when the database cannot show query plans.
    pub explain: Option<ExplainSyntax>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PlaceholderStyle {
    /// `?` for every parameter.
    Question,
    /// `$1`, `$2`, … by position.
    Numbered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LiteralStyle {
    /// `\` is an escape inside strings, so it must be doubled.
    pub backslash_escapes: bool,
    /// Booleans are written `1` / `0` instead of `TRUE` / `FALSE`.
    pub bool_as_int: bool,
}

/// Keywords put before a statement to get its plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExplainSyntax {
    /// The plan as a table, e.g. `EXPLAIN`.
    pub table: String,
    /// The plan as JSON, e.g. `EXPLAIN FORMAT=JSON`, when the database has it.
    pub json: Option<String>,
}
