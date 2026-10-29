use std::fmt;
use std::io::Write;

use quarry_driver_api::Dialect;
use quarry_model::{ColumnMeta, Value};

use crate::{ExportError, Options, Result};

/// Rows in one INSERT statement. A long statement is slow to run and awkward to open.
const ROWS_PER_INSERT: usize = 500;

static NULL: Value = Value::Null;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Inserts,
    Updates,
    WhereClause,
}

/// A column the statements refer to: its position in a row and its quoted name.
#[derive(Debug)]
struct Column {
    index: usize,
    name: String,
}

#[derive(Debug)]
enum Plan {
    Inserts {
        /// `INSERT INTO t (a, b) VALUES` and the newline and indent that follow.
        head: String,
        rows_in_statement: usize,
    },
    Updates {
        /// `UPDATE t SET `.
        head: String,
        set: Vec<Column>,
        key: Vec<Column>,
    },
    WhereClause {
        key: Vec<Column>,
        /// One key column: its values go in one `IN (...)` list.
        is_list_open: bool,
        /// One key column: NULL never matches `IN`, so it is added as `IS NULL` at the end.
        has_null: bool,
    },
}

/// SQL text built through the connection's dialect, so quoting and literals match the database
/// it will run on. Statements are written as rows arrive; nothing is kept.
pub struct Writer<'d, W: Write> {
    sink: W,
    dialect: &'d dyn Dialect,
    plan: Plan,
    width: usize,
    rows: u64,
    buf: String,
}

impl<W: Write> fmt::Debug for Writer<'_, W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("plan", &self.plan)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl<'d, W: Write> Writer<'d, W> {
    pub(crate) fn start(
        kind: Kind,
        options: &Options,
        dialect: &'d dyn Dialect,
        sink: W,
        columns: &[ColumnMeta],
    ) -> Result<Self> {
        let plan = match kind {
            Kind::Inserts => {
                let table = options.table.as_ref().ok_or(ExportError::TableRequired)?;
                let names: Vec<String> = columns
                    .iter()
                    .map(|column| dialect.quote_ident(&column.name))
                    .collect();
                Plan::Inserts {
                    head: format!(
                        "INSERT INTO {} ({}) VALUES\n  ",
                        dialect.qualify(table),
                        names.join(", ")
                    ),
                    rows_in_statement: 0,
                }
            }
            Kind::Updates => {
                let table = options.table.as_ref().ok_or(ExportError::TableRequired)?;
                let key = key_columns(options, dialect, columns)?;
                let set: Vec<Column> = columns
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| key.iter().all(|k| k.index != *index))
                    .map(|(index, column)| Column {
                        index,
                        name: dialect.quote_ident(&column.name),
                    })
                    .collect();
                if set.is_empty() {
                    return Err(ExportError::NothingToSet);
                }
                Plan::Updates {
                    head: format!("UPDATE {} SET ", dialect.qualify(table)),
                    set,
                    key,
                }
            }
            Kind::WhereClause => Plan::WhereClause {
                key: key_columns(options, dialect, columns)?,
                is_list_open: false,
                has_null: false,
            },
        };
        Ok(Self {
            sink,
            dialect,
            plan,
            width: columns.len(),
            rows: 0,
            buf: String::new(),
        })
    }

    pub(crate) fn write_batch(&mut self, rows: &[Vec<Value>]) -> Result<()> {
        for row in rows {
            self.push_row(row);
        }
        self.rows += rows.len() as u64;
        self.flush_buf()
    }

    pub(crate) fn rows(&self) -> u64 {
        self.rows
    }

    pub(crate) fn finish(mut self) -> Result<W> {
        match &mut self.plan {
            Plan::Inserts {
                rows_in_statement, ..
            } => {
                if *rows_in_statement > 0 {
                    self.buf.push_str(";\n");
                }
            }
            Plan::Updates { .. } => {}
            Plan::WhereClause {
                key,
                is_list_open,
                has_null,
            } => {
                if *is_list_open {
                    self.buf.push(')');
                }
                if *has_null {
                    // Only a single key column collects NULLs this way.
                    if let Some(column) = key.first() {
                        self.buf
                            .push_str(if *is_list_open { "\n   OR " } else { "WHERE " });
                        self.buf.push_str(&column.name);
                        self.buf.push_str(" IS NULL");
                    }
                }
                if *is_list_open || *has_null || self.rows > 0 {
                    self.buf.push('\n');
                }
            }
        }
        self.flush_buf()?;
        self.sink.flush()?;
        Ok(self.sink)
    }

    fn push_row(&mut self, row: &[Value]) {
        let dialect = self.dialect;
        let cell = |index: usize| row.get(index).unwrap_or(&NULL);
        match &mut self.plan {
            Plan::Inserts {
                head,
                rows_in_statement,
            } => {
                if *rows_in_statement == 0 {
                    self.buf.push_str(head);
                } else {
                    self.buf.push_str(",\n  ");
                }
                self.buf.push('(');
                for index in 0..self.width {
                    if index > 0 {
                        self.buf.push_str(", ");
                    }
                    self.buf.push_str(&dialect.literal(cell(index)));
                }
                self.buf.push(')');
                *rows_in_statement += 1;
                if *rows_in_statement == ROWS_PER_INSERT {
                    self.buf.push_str(";\n");
                    *rows_in_statement = 0;
                }
            }
            Plan::Updates { head, set, key } => {
                self.buf.push_str(head);
                for (position, column) in set.iter().enumerate() {
                    if position > 0 {
                        self.buf.push_str(", ");
                    }
                    self.buf.push_str(&column.name);
                    self.buf.push_str(" = ");
                    self.buf.push_str(&dialect.literal(cell(column.index)));
                }
                self.buf.push_str(" WHERE ");
                push_key_match(&mut self.buf, dialect, key, &cell);
                self.buf.push_str(";\n");
            }
            Plan::WhereClause {
                key,
                is_list_open,
                has_null,
            } => match key.as_slice() {
                [column] => {
                    let value = cell(column.index);
                    if value.is_null() {
                        *has_null = true;
                    } else {
                        if *is_list_open {
                            self.buf.push_str(", ");
                        } else {
                            self.buf.push_str("WHERE ");
                            self.buf.push_str(&column.name);
                            self.buf.push_str(" IN (");
                            *is_list_open = true;
                        }
                        self.buf.push_str(&dialect.literal(value));
                    }
                }
                _ => {
                    self.buf.push_str(if self.rows == 0 && self.buf.is_empty() {
                        "WHERE ("
                    } else {
                        "\n   OR ("
                    });
                    push_key_match(&mut self.buf, dialect, key, &cell);
                    self.buf.push(')');
                }
            },
        }
    }

    fn flush_buf(&mut self) -> Result<()> {
        self.sink.write_all(self.buf.as_bytes())?;
        self.buf.clear();
        Ok(())
    }
}

/// `a = 1 AND b IS NULL`: a NULL key value never matches with `=`.
fn push_key_match<'v>(
    out: &mut String,
    dialect: &dyn Dialect,
    key: &[Column],
    cell: &dyn Fn(usize) -> &'v Value,
) {
    for (position, column) in key.iter().enumerate() {
        if position > 0 {
            out.push_str(" AND ");
        }
        out.push_str(&column.name);
        let value = cell(column.index);
        if value.is_null() {
            out.push_str(" IS NULL");
        } else {
            out.push_str(" = ");
            out.push_str(&dialect.literal(value));
        }
    }
}

fn key_columns(
    options: &Options,
    dialect: &dyn Dialect,
    columns: &[ColumnMeta],
) -> Result<Vec<Column>> {
    if options.key.is_empty() {
        return Err(ExportError::KeyRequired);
    }
    options
        .key
        .iter()
        .map(|name| {
            columns
                .iter()
                .position(|column| &column.name == name)
                .map(|index| Column {
                    index,
                    name: dialect.quote_ident(name),
                })
                .ok_or_else(|| ExportError::UnknownKeyColumn(name.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{Ansi, columns, orders};
    use crate::{Format, Writer as AnyWriter, format_info};

    fn options(key: &[&str]) -> Options {
        Options {
            table: Some(orders()),
            key: key.iter().map(|k| (*k).to_owned()).collect(),
            ..Options::default()
        }
    }

    fn try_write(
        format: Format,
        options: &Options,
        names: &[&str],
        batches: &[Vec<Vec<Value>>],
    ) -> Result<String> {
        let mut writer = AnyWriter::start(format, options, &Ansi, Vec::new(), &columns(names))?;
        for batch in batches {
            writer.write_batch(batch)?;
        }
        Ok(String::from_utf8(writer.finish()?).unwrap())
    }

    fn write(
        format: Format,
        options: &Options,
        names: &[&str],
        batches: &[Vec<Vec<Value>>],
    ) -> String {
        try_write(format, options, names, batches).unwrap()
    }

    fn text(value: &str) -> Value {
        Value::Text(value.into())
    }

    #[test]
    fn inserts_quote_names_and_write_dialect_literals() {
        let batches = vec![vec![
            vec![Value::Int(1), text("it's")],
            vec![Value::Int(2), Value::Null],
        ]];
        let out = write(Format::SqlInserts, &options(&[]), &["id", "note"], &batches);
        assert_eq!(
            out,
            "INSERT INTO \"shop\".\"orders\" (\"id\", \"note\") VALUES\n  \
             (1, 'it''s'),\n  (2, NULL);\n"
        );
    }

    #[test]
    fn inserts_start_a_new_statement_every_500_rows_across_batches() {
        let row = |n: i64| vec![Value::Int(n)];
        let batches = vec![
            (0..700).map(row).collect::<Vec<_>>(),
            (700..1200).map(row).collect::<Vec<_>>(),
        ];
        let out = write(Format::SqlInserts, &options(&[]), &["id"], &batches);
        assert_eq!(out.matches("INSERT INTO").count(), 3);
        let statements: Vec<&str> = out.split(";\n").filter(|s| !s.is_empty()).collect();
        let sizes: Vec<usize> = statements
            .iter()
            .map(|s| s.matches("\n  (").count())
            .collect();
        assert_eq!(sizes, vec![500, 500, 200]);
        assert!(out.ends_with(";\n"));
    }

    #[test]
    fn inserts_pad_a_short_row_with_null() {
        let out = write(
            Format::SqlInserts,
            &options(&[]),
            &["a", "b"],
            &[vec![vec![Value::Int(1)]]],
        );
        assert!(out.ends_with("(1, NULL);\n"), "{out}");
    }

    #[test]
    fn updates_set_every_non_key_column_and_find_the_row_by_key() {
        let batches = vec![vec![
            vec![Value::Int(1), text("a"), Value::Bool(true)],
            vec![Value::Int(2), Value::Null, Value::Bool(false)],
        ]];
        let out = write(
            Format::SqlUpdates,
            &options(&["id"]),
            &["id", "note", "paid"],
            &batches,
        );
        assert_eq!(
            out,
            "UPDATE \"shop\".\"orders\" SET \"note\" = 'a', \"paid\" = TRUE WHERE \"id\" = 1;\n\
             UPDATE \"shop\".\"orders\" SET \"note\" = NULL, \"paid\" = FALSE WHERE \"id\" = 2;\n"
        );
    }

    #[test]
    fn updates_with_a_composite_key_match_every_key_column() {
        let batches = vec![vec![vec![Value::Int(1), text("x"), text("new")]]];
        let out = write(
            Format::SqlUpdates,
            &options(&["a", "b"]),
            &["a", "b", "note"],
            &batches,
        );
        assert_eq!(
            out,
            "UPDATE \"shop\".\"orders\" SET \"note\" = 'new' WHERE \"a\" = 1 AND \"b\" = 'x';\n"
        );
    }

    #[test]
    fn a_null_key_value_matches_with_is_null_never_equals_null() {
        let batches = vec![vec![vec![Value::Int(1), Value::Null, text("v")]]];
        let out = write(
            Format::SqlUpdates,
            &options(&["a", "b"]),
            &["a", "b", "note"],
            &batches,
        );
        assert!(
            out.ends_with("WHERE \"a\" = 1 AND \"b\" IS NULL;\n"),
            "{out}"
        );
        assert!(!out.contains("= NULL"));
    }

    #[test]
    fn where_clause_with_one_key_is_an_in_list() {
        let batches = vec![
            vec![vec![Value::Int(1), text("a")]],
            vec![
                vec![Value::Int(2), text("b")],
                vec![Value::Int(3), text("c")],
            ],
        ];
        let out = write(
            Format::WhereClause,
            &options(&["id"]),
            &["id", "note"],
            &batches,
        );
        assert_eq!(out, "WHERE \"id\" IN (1, 2, 3)\n");
    }

    #[test]
    fn where_clause_with_one_key_adds_is_null_for_null_keys() {
        let batches = vec![vec![
            vec![Value::Int(1)],
            vec![Value::Null],
            vec![Value::Int(2)],
        ]];
        let out = write(Format::WhereClause, &options(&["id"]), &["id"], &batches);
        assert_eq!(out, "WHERE \"id\" IN (1, 2)\n   OR \"id\" IS NULL\n");

        let only_null = write(
            Format::WhereClause,
            &options(&["id"]),
            &["id"],
            &[vec![vec![Value::Null]]],
        );
        assert_eq!(only_null, "WHERE \"id\" IS NULL\n");
    }

    #[test]
    fn where_clause_with_a_composite_key_ors_one_group_per_row() {
        let batches = vec![
            vec![vec![Value::Int(1), text("x"), text("n1")]],
            vec![
                vec![Value::Int(2), Value::Null, text("n2")],
                vec![Value::Int(3), text("z"), text("n3")],
            ],
        ];
        let out = write(
            Format::WhereClause,
            &options(&["a", "b"]),
            &["a", "b", "note"],
            &batches,
        );
        assert_eq!(
            out,
            "WHERE (\"a\" = 1 AND \"b\" = 'x')\n   OR (\"a\" = 2 AND \"b\" IS NULL)\n   \
             OR (\"a\" = 3 AND \"b\" = 'z')\n"
        );
    }

    #[test]
    fn zero_rows_write_nothing() {
        for format in [Format::SqlInserts, Format::SqlUpdates, Format::WhereClause] {
            let out = write(format, &options(&["id"]), &["id", "note"], &[]);
            assert_eq!(out, "", "{format:?}");
        }
    }

    #[test]
    fn updates_and_where_clause_refuse_a_missing_key_with_a_reason() {
        for format in [Format::SqlUpdates, Format::WhereClause] {
            let err = try_write(format, &options(&[]), &["id", "note"], &[]).unwrap_err();
            assert!(matches!(err, ExportError::KeyRequired), "{format:?}");
            assert!(err.to_string().contains("primary key"));
        }
    }

    #[test]
    fn inserts_and_updates_need_a_target_table() {
        let no_table = Options {
            key: vec!["id".into()],
            ..Options::default()
        };
        for format in [Format::SqlInserts, Format::SqlUpdates] {
            let err = try_write(format, &no_table, &["id", "note"], &[]).unwrap_err();
            assert!(matches!(err, ExportError::TableRequired), "{format:?}");
        }
    }

    #[test]
    fn a_key_column_missing_from_the_result_is_named() {
        let err = try_write(Format::SqlUpdates, &options(&["id"]), &["note"], &[]).unwrap_err();
        match err {
            ExportError::UnknownKeyColumn(name) => assert_eq!(name, "id"),
            other => panic!("expected UnknownKeyColumn, got {other:?}"),
        }
    }

    #[test]
    fn updates_refuse_a_result_that_is_only_key_columns() {
        let err = try_write(Format::SqlUpdates, &options(&["id"]), &["id"], &[]).unwrap_err();
        assert!(matches!(err, ExportError::NothingToSet));
    }

    #[test]
    fn rows_counts_every_row_including_null_keys() {
        let mut writer = AnyWriter::start(
            Format::WhereClause,
            &options(&["id"]),
            &Ansi,
            Vec::new(),
            &columns(&["id"]),
        )
        .unwrap();
        writer
            .write_batch(&[vec![Value::Int(1)], vec![Value::Null]])
            .unwrap();
        assert_eq!(writer.rows(), 2);
    }

    #[test]
    fn format_info_marks_the_formats_that_need_a_key() {
        assert!(format_info(Format::SqlUpdates).is_key_required);
        assert!(format_info(Format::WhereClause).is_key_required);
        assert!(!format_info(Format::SqlInserts).is_key_required);
        assert_eq!(format_info(Format::SqlInserts).extension, "sql");
    }
}
