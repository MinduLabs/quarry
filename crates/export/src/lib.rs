//! Writes rows to files. No database access: a writer takes columns, then batches of rows, and
//! writes bytes, so it tests without a server and a new format never touches the engine.
//!
//! - [`Writer`]: one format over any [`std::io::Write`]. The same code writes the preview text
//!   and the file, so the two cannot differ.
//! - [`Output`]: the file itself. It writes `<path>.part` and renames it into place only on
//!   [`Output::commit`], so a cancelled or failed export never leaves a half-written file.

mod delimited;
mod json;
mod output;
mod sql;
mod xlsx;

use std::borrow::Cow;
use std::io::{self, Write};
use std::path::PathBuf;

use quarry_driver_api::Dialect;
use quarry_model::{ColumnMeta, Value};

pub use output::Output;

pub type Result<T, E = ExportError> = std::result::Result<T, E>;

/// Every message says what happened and what to do.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("The folder {} does not exist. Choose a folder that exists.", .0.display())]
    FolderNotFound(PathBuf),
    #[error(
        "Quarry is not allowed to write in {}. Choose another folder or change its permissions.",
        .0.display()
    )]
    PermissionDenied(PathBuf),
    #[error("The disk is full. Free some space and export again.")]
    DiskFull,
    #[error("The delimiter cannot be a quote or a line break. Choose another character.")]
    InvalidDelimiter,
    #[error("SQL needs a target table. Choose the table these rows belong to.")]
    TableRequired,
    #[error(
        "This format needs a primary key or unique key to find each row. Export from a table that has one."
    )]
    KeyRequired,
    #[error("The key column {0} is not in the result. Add it to the SELECT and export again.")]
    UnknownKeyColumn(String),
    #[error(
        "Every column is part of the key, so there is nothing to update. Choose another format."
    )]
    NothingToSet,
    #[error(
        "Excel holds at most {max} columns, but the result has {0}. Select fewer columns or choose another format.",
        max = xlsx::MAX_COLUMNS
    )]
    TooManyColumns(usize),
    #[error("Excel could not write the file ({0}). Try another format.")]
    Spreadsheet(String),
    #[error("Writing the file failed ({0}). Try again or choose another location.")]
    Io(#[source] io::Error),
}

impl From<io::Error> for ExportError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::StorageFull => Self::DiskFull,
            _ => Self::Io(err),
        }
    }
}

/// The format and options types cross the protocol, so they live in `quarry-model`.
pub use quarry_model::{ExportFormat as Format, ExportOptions as Options};

/// What the UI needs to know about a format, so it never matches on a format name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatInfo {
    pub extension: &'static str,
    /// The format is text, so a preview of the first rows can be shown.
    pub has_preview: bool,
    /// The format needs one source table with a usable key.
    pub is_key_required: bool,
    /// Rows a sheet holds, header included. The writer starts a new sheet after that many.
    pub max_rows_per_sheet: Option<u64>,
}

pub const fn format_info(format: Format) -> FormatInfo {
    let extension = match format {
        Format::Csv => "csv",
        Format::Tsv => "tsv",
        Format::Json => "json",
        Format::Ndjson => "ndjson",
        Format::SqlInserts | Format::SqlUpdates | Format::WhereClause => "sql",
        Format::Xlsx => "xlsx",
    };
    FormatInfo {
        extension,
        has_preview: !matches!(format, Format::Xlsx),
        is_key_required: matches!(format, Format::SqlUpdates | Format::WhereClause),
        max_rows_per_sheet: match format {
            Format::Xlsx => Some(xlsx::MAX_ROWS_PER_SHEET),
            _ => None,
        },
    }
}

/// What a finished (or running) export holds, for the message shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub rows: u64,
    /// Sheets written, 0 for formats that have none.
    pub sheets: u32,
    /// Excel: cells whose text was cut to Excel's limit of 32,767 characters.
    pub cut_cells: u64,
}

/// One format writing to `W`. Start it with the columns, feed it batches, then finish.
/// `dialect` is the connection's: only the SQL formats use it, to quote names and write values.
/// `W` is `Send` because Excel hands it to the library that zips the file.
#[derive(Debug)]
pub enum Writer<'d, W: Write + Send> {
    Delimited(delimited::Writer<W>),
    Json(json::Writer<W>),
    Sql(sql::Writer<'d, W>),
    /// Boxed: a workbook is far larger than the other writers.
    Xlsx(Box<xlsx::Writer<W>>),
}

impl<'d, W: Write + Send> Writer<'d, W> {
    /// Write whatever comes before the first row (byte order mark, header) to `sink`.
    pub fn start(
        format: Format,
        options: &Options,
        dialect: &'d dyn Dialect,
        sink: W,
        columns: &[ColumnMeta],
    ) -> Result<Self> {
        match format {
            Format::Csv => delimited::Writer::start(delimited::Kind::Csv, options, sink, columns)
                .map(Self::Delimited),
            Format::Tsv => delimited::Writer::start(delimited::Kind::Tsv, options, sink, columns)
                .map(Self::Delimited),
            Format::Json => json::Writer::start(json::Kind::Array, sink, columns).map(Self::Json),
            Format::Ndjson => json::Writer::start(json::Kind::Lines, sink, columns).map(Self::Json),
            Format::SqlInserts => {
                sql::Writer::start(sql::Kind::Inserts, options, dialect, sink, columns)
                    .map(Self::Sql)
            }
            Format::SqlUpdates => {
                sql::Writer::start(sql::Kind::Updates, options, dialect, sink, columns)
                    .map(Self::Sql)
            }
            Format::WhereClause => {
                sql::Writer::start(sql::Kind::WhereClause, options, dialect, sink, columns)
                    .map(Self::Sql)
            }
            Format::Xlsx => xlsx::Writer::start(options, sink, columns)
                .map(|writer| Self::Xlsx(Box::new(writer))),
        }
    }

    /// A row shorter than the columns is padded with NULL.
    pub fn write_batch(&mut self, rows: &[Vec<Value>]) -> Result<()> {
        match self {
            Self::Delimited(writer) => writer.write_batch(rows),
            Self::Json(writer) => writer.write_batch(rows),
            Self::Sql(writer) => writer.write_batch(rows),
            Self::Xlsx(writer) => writer.write_batch(rows),
        }
    }

    /// Rows written so far.
    pub fn rows(&self) -> u64 {
        self.summary().rows
    }

    /// Rows, sheets and cut cells so far. Read it before [`Self::finish`], which consumes the
    /// writer.
    pub fn summary(&self) -> Summary {
        match self {
            Self::Delimited(writer) => Summary {
                rows: writer.rows(),
                ..Summary::default()
            },
            Self::Json(writer) => Summary {
                rows: writer.rows(),
                ..Summary::default()
            },
            Self::Sql(writer) => Summary {
                rows: writer.rows(),
                ..Summary::default()
            },
            Self::Xlsx(writer) => writer.summary(),
        }
    }

    /// Write whatever closes the file and flush. Gives the sink back, e.g. to commit it.
    pub fn finish(self) -> Result<W> {
        match self {
            Self::Delimited(writer) => writer.finish(),
            Self::Json(writer) => writer.finish(),
            Self::Sql(writer) => writer.finish(),
            Self::Xlsx(writer) => writer.finish(),
        }
    }
}

/// A value as plain text, the same way the clipboard copy writes it. NULL is empty.
pub(crate) fn cell_text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::Null => Cow::Borrowed(""),
        Value::Bool(b) => Cow::Borrowed(if *b { "true" } else { "false" }),
        Value::Int(v) => Cow::Owned(v.to_string()),
        Value::UInt(v) => Cow::Owned(v.to_string()),
        Value::Float(v) => Cow::Owned(float_text(*v)),
        Value::Decimal(text) | Value::Text(text) | Value::Json(text) => Cow::Borrowed(text),
        Value::Other { text, .. } => Cow::Borrowed(text),
        Value::Bytes(bytes) => Cow::Owned(bytes.iter().map(|b| format!("{b:02x}")).collect()),
        Value::Date(v) => Cow::Owned(v.to_string()),
        Value::Time(v) => Cow::Owned(v.to_string()),
        Value::Timestamp(v) => Cow::Owned(v.to_string()),
        Value::TimestampTz(v) => Cow::Owned(v.to_rfc3339()),
        Value::Uuid(v) => Cow::Owned(v.to_string()),
        Value::Array(items) => {
            let mut out = String::from("[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_json_string(&mut out, &cell_text(item));
            }
            out.push(']');
            Cow::Owned(out)
        }
    }
}

pub(crate) fn float_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else {
        value.to_string()
    }
}

/// `text` as a quoted JSON string.
pub(crate) fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if u32::from(ch) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::path::PathBuf;

    use quarry_driver_api::Dialect;
    use quarry_model::{ColumnMeta, TableRef};

    /// Quotes with `"` and numbers placeholders, so tests catch dialect mix-ups.
    #[derive(Debug)]
    pub struct Ansi;

    impl Dialect for Ansi {
        fn quote_ident(&self, ident: &str) -> String {
            format!("\"{}\"", ident.replace('"', "\"\""))
        }
        fn placeholder(&self, index: usize) -> String {
            format!("${index}")
        }
        fn select_page(
            &self,
            _: &TableRef,
            _: Option<&str>,
            _: Option<&str>,
            _: u64,
            _: u64,
        ) -> String {
            String::new()
        }
    }

    pub fn orders() -> TableRef {
        TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        }
    }

    pub fn column(name: &str) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            type_name: "TEXT".into(),
            nullable: None,
            source_database: None,
            source_table: None,
            source_column: None,
        }
    }

    pub fn columns(names: &[&str]) -> Vec<ColumnMeta> {
        names.iter().map(|name| column(name)).collect()
    }

    pub fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quarry-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_text_matches_the_clipboard_copy() {
        assert_eq!(cell_text(&Value::Null), "");
        assert_eq!(cell_text(&Value::Bool(true)), "true");
        assert_eq!(cell_text(&Value::Int(i64::MIN)), "-9223372036854775808");
        assert_eq!(cell_text(&Value::UInt(u64::MAX)), "18446744073709551615");
        assert_eq!(cell_text(&Value::Float(1.0)), "1");
        assert_eq!(cell_text(&Value::Float(f64::NAN)), "NaN");
        assert_eq!(cell_text(&Value::Float(f64::NEG_INFINITY)), "-Infinity");
        assert_eq!(cell_text(&Value::Bytes(vec![0xde, 0xad])), "dead");
        assert_eq!(
            cell_text(&Value::Array(vec![
                Value::Text("a\"b".into()),
                Value::Int(1)
            ])),
            r#"["a\"b","1"]"#
        );
    }

    #[test]
    fn json_strings_escape_quotes_and_control_characters() {
        let mut out = String::new();
        push_json_string(&mut out, "a\"b\\c\nd\u{1}é");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    #[test]
    fn a_writer_into_an_output_gives_the_whole_file_only_after_commit() {
        let dir = test_util::temp_dir();
        let target = dir.join("orders.ndjson");
        let output = Output::create(&target).unwrap();
        let mut writer = Writer::start(
            Format::Ndjson,
            &Options::default(),
            &test_util::Ansi,
            output,
            &test_util::columns(&["id"]),
        )
        .unwrap();
        writer
            .write_batch(&[vec![Value::Int(1)], vec![Value::Int(2)]])
            .unwrap();
        writer.write_batch(&[vec![Value::Int(3)]]).unwrap();
        assert_eq!(writer.rows(), 3);
        assert!(!target.exists());
        writer.finish().unwrap().commit().unwrap();
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "{\"id\":1}\n{\"id\":2}\n{\"id\":3}\n"
        );
        assert!(!dir.join("orders.ndjson.part").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_cancelled_export_leaves_no_file() {
        let dir = test_util::temp_dir();
        let target = dir.join("orders.csv");
        let mut writer = Writer::start(
            Format::Csv,
            &Options::default(),
            &test_util::Ansi,
            Output::create(&target).unwrap(),
            &test_util::columns(&["id"]),
        )
        .unwrap();
        writer.write_batch(&[vec![Value::Int(1)]]).unwrap();
        writer.write_batch(&[vec![Value::Int(2)]]).unwrap();
        // The job stops: the writer and its output are dropped without a commit.
        drop(writer);
        assert!(!target.exists());
        assert!(!dir.join("orders.csv.part").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn options_read_missing_fields_as_defaults() {
        let options: Options = serde_json::from_str(r#"{"delimiter":";"}"#).expect("options");
        assert_eq!(
            options,
            Options {
                delimiter: ';',
                ..Options::default()
            }
        );
    }
}
