use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What fits at the cursor of a SQL editor, read from the statement around it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompletionContext {
    /// Where the word being typed starts. Bytes in Rust; the bridge hands the UI UTF-16 units.
    pub word_start: u32,
    /// The names that fit here.
    pub names: CompletionNames,
    /// The tables the statement names, with their aliases, in order.
    pub tables: Vec<StatementTable>,
    /// Keywords that fit here, likeliest first.
    pub keywords: Vec<String>,
    /// Functions, where an expression goes.
    pub functions: Vec<String>,
    /// A name goes here (`WHERE |`, `ORDER BY |`) rather than the next keyword
    /// (`ORDER BY total |`): with nothing typed yet, names come first.
    pub expects_name: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum CompletionNames {
    /// Tables and views, of `database` or of the current one; databases too when none is named.
    Objects { database: Option<String> },
    /// The columns of these tables (`alias.`, `table.`, `db.table.`).
    Columns { tables: Vec<StatementTable> },
    /// An expression: the columns of the statement's tables.
    Expression,
    /// Nothing named fits: only keywords.
    Keywords,
}

/// A table a statement reads or writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatementTable {
    pub database: Option<String>,
    pub name: String,
    pub alias: Option<String>,
}
