use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::TableRef;

/// A file format an export can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ExportFormat {
    Csv,
    Tsv,
    Json,
    Ndjson,
    SqlInserts,
    SqlUpdates,
    WhereClause,
    Xlsx,
}

/// How a file is written. A field a format does not use is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ExportOptions {
    /// CSV only. A tab is always the TSV delimiter.
    pub delimiter: char,
    /// CSV, TSV and Excel: the column names as the first line (on every sheet in Excel).
    pub has_header: bool,
    /// CSV and TSV: a UTF-8 byte order mark, so Excel reads non-ASCII text correctly.
    pub has_bom: bool,
    /// Excel: data rows per sheet. `None` fills a sheet to Excel's limit.
    #[ts(type = "number | null")]
    pub max_rows_per_sheet: Option<u64>,
    /// SQL Inserts: the table the rows go into. The engine fills it from the source table when
    /// the client leaves it empty; a result that comes from several tables needs the user's.
    pub table: Option<TableRef>,
    /// SQL Updates and Where Clause: the columns of the source table that identify a row. The
    /// engine always sets them from the table's key, whatever a client sends.
    pub key: Vec<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            has_header: true,
            has_bom: false,
            max_rows_per_sheet: None,
            table: None,
            key: Vec::new(),
        }
    }
}
