use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The SQL syntax of a database, as data. A driver describes its dialect with it, so the engine
/// writes statements without calling into the driver, which may run in another process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DialectSpec {
    pub quote: IdentQuote,
    pub placeholder: PlaceholderStyle,
    pub paging: PagingStyle,
    pub literals: LiteralStyle,
    /// `None` when the database cannot show query plans.
    pub explain: Option<ExplainSyntax>,
}

/// How identifiers are quoted. The closing quote is doubled inside a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IdentQuote {
    /// `"name"`
    Double,
    /// `` `name` ``
    Backtick,
    /// `[name]`
    Brackets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PlaceholderStyle {
    /// `?` for every parameter.
    Question,
    /// `$1`, `$2`, … by position.
    Numbered,
    /// `@P1`, `@P2`, … by position.
    AtP,
}

/// How a page of rows is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PagingStyle {
    /// `LIMIT n OFFSET m`
    LimitOffset,
    /// `ORDER BY … OFFSET m ROWS FETCH NEXT n ROWS ONLY`, which needs an `ORDER BY`.
    OffsetFetch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LiteralStyle {
    /// `\` is an escape inside strings, so it must be doubled.
    pub backslash_escapes: bool,
    /// Booleans are written `1` / `0` instead of `TRUE` / `FALSE`.
    pub bool_as_int: bool,
    /// Strings are written `N'…'`, so they keep characters outside the code page.
    pub national_strings: bool,
    /// Bytes are written `0x…` instead of `X'…'`.
    pub hex_bytes: bool,
}

/// Keywords put before a statement to get its plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExplainSyntax {
    /// The plan as a table, e.g. `EXPLAIN`.
    pub table: String,
    /// The plan as JSON, e.g. `EXPLAIN FORMAT=JSON`, when the database has it.
    pub json: Option<String>,
}
