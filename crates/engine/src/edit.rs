//! Turns grid edits into parameterized statements. Values never enter the SQL text.

use quarry_driver_api::Dialect;
use quarry_model::{CellValue, RowChange, TableRef, Value};

/// One statement to run, with its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub sql: String,
    pub params: Vec<Value>,
    /// UPDATE and DELETE must hit exactly one row; anything else rolls the batch back.
    pub one_row: bool,
}

/// Counts shown in the confirmation and the result message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub updates: usize,
    pub inserts: usize,
    pub deletes: usize,
}

impl Summary {
    pub fn of(changes: &[RowChange]) -> Self {
        let mut summary = Self::default();
        for change in changes {
            match change {
                RowChange::Update { .. } => summary.updates += 1,
                RowChange::Insert { .. } => summary.inserts += 1,
                RowChange::Delete { .. } => summary.deletes += 1,
            }
        }
        summary
    }

    pub fn describe(self) -> String {
        let parts: Vec<String> = [
            (self.updates, "updated"),
            (self.inserts, "inserted"),
            (self.deletes, "deleted"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, verb)| format!("{n} {} {verb}", if n == 1 { "row" } else { "rows" }))
        .collect();
        parts.join(", ")
    }
}

pub fn statements(
    dialect: &dyn Dialect,
    table: &TableRef,
    changes: &[RowChange],
) -> Result<Vec<Statement>, String> {
    let name = dialect.qualify(table);
    let mut out = Vec::with_capacity(changes.len());
    for change in changes {
        let mut params = Vec::new();
        let statement = match change {
            RowChange::Update { key, set } => {
                if set.is_empty() {
                    continue;
                }
                let set = assignments(dialect, set, &mut params, ", ");
                let filter = key_filter(dialect, key, &mut params)?;
                Statement {
                    sql: format!("UPDATE {name} SET {set} WHERE {filter}"),
                    params,
                    one_row: true,
                }
            }
            RowChange::Insert { values } => {
                if values.is_empty() {
                    return Err("A new row needs at least one value.".to_owned());
                }
                let columns: Vec<String> = values
                    .iter()
                    .map(|c| dialect.quote_ident(&c.column))
                    .collect();
                let marks: Vec<String> = values
                    .iter()
                    .map(|c| {
                        params.push(c.value.clone());
                        dialect.placeholder(params.len())
                    })
                    .collect();
                Statement {
                    sql: format!(
                        "INSERT INTO {name} ({}) VALUES ({})",
                        columns.join(", "),
                        marks.join(", ")
                    ),
                    params,
                    one_row: false,
                }
            }
            RowChange::Delete { key } => {
                let filter = key_filter(dialect, key, &mut params)?;
                Statement {
                    sql: format!("DELETE FROM {name} WHERE {filter}"),
                    params,
                    one_row: true,
                }
            }
        };
        out.push(statement);
    }
    Ok(out)
}

fn assignments(
    dialect: &dyn Dialect,
    cells: &[CellValue],
    params: &mut Vec<Value>,
    separator: &str,
) -> String {
    cells
        .iter()
        .map(|c| {
            params.push(c.value.clone());
            format!(
                "{} = {}",
                dialect.quote_ident(&c.column),
                dialect.placeholder(params.len())
            )
        })
        .collect::<Vec<_>>()
        .join(separator)
}

/// `key = ? AND …`, with `IS NULL` for NULL values. A key is required: without one the
/// statement could touch every row. The key is the primary key, or every comparable column of a
/// table without one; either way the statement must then hit exactly one row (`one_row`).
fn key_filter(
    dialect: &dyn Dialect,
    key: &[CellValue],
    params: &mut Vec<Value>,
) -> Result<String, String> {
    if key.is_empty() {
        return Err("This row has no key columns, so it can't be changed safely.".to_owned());
    }
    Ok(key
        .iter()
        .map(|c| {
            let column = dialect.quote_ident(&c.column);
            if c.value.is_null() {
                return format!("{column} IS NULL");
            }
            params.push(c.value.clone());
            format!("{column} = {}", dialect.placeholder(params.len()))
        })
        .collect::<Vec<_>>()
        .join(" AND "))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Quotes with `"` and numbers placeholders, so tests catch dialect mix-ups.
    pub(crate) struct Ansi;

    /// `Ansi` as a driver declares it, for fake drivers.
    pub(crate) fn ansi_spec() -> quarry_driver_api::SpecDialect {
        use quarry_model::{DialectSpec, IdentQuote, LiteralStyle, PagingStyle, PlaceholderStyle};
        quarry_driver_api::SpecDialect::new(DialectSpec {
            quote: IdentQuote::Double,
            on_duplicate: quarry_model::DuplicateSyntax::None,
            placeholder: PlaceholderStyle::Numbered,
            paging: PagingStyle::LimitOffset,
            literals: LiteralStyle {
                backslash_escapes: false,
                bool_as_int: false,
                national_strings: false,
                hex_bytes: false,
            },
            explain: None,
        })
    }

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

    fn cell(column: &str, value: Value) -> CellValue {
        CellValue {
            column: column.into(),
            value,
        }
    }

    fn table() -> TableRef {
        TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        }
    }

    #[test]
    fn builds_parameterized_statements() {
        let changes = vec![
            RowChange::Update {
                key: vec![cell("id", Value::Int(7))],
                set: vec![
                    cell("note", Value::Text("it's \"ok\"".into())),
                    cell("total", Value::Null),
                ],
            },
            RowChange::Insert {
                values: vec![cell("note", Value::Text("new".into()))],
            },
            RowChange::Delete {
                key: vec![cell("a", Value::Int(1)), cell("b", Value::Int(2))],
            },
        ];
        let out = statements(&Ansi, &table(), &changes).unwrap();
        assert_eq!(
            out[0].sql,
            r#"UPDATE "shop"."orders" SET "note" = $1, "total" = $2 WHERE "id" = $3"#
        );
        assert_eq!(
            out[0].params,
            vec![
                Value::Text("it's \"ok\"".into()),
                Value::Null,
                Value::Int(7)
            ]
        );
        assert!(out[0].one_row);
        assert_eq!(
            out[1].sql,
            r#"INSERT INTO "shop"."orders" ("note") VALUES ($1)"#
        );
        assert!(!out[1].one_row);
        assert_eq!(
            out[2].sql,
            r#"DELETE FROM "shop"."orders" WHERE "a" = $1 AND "b" = $2"#
        );
    }

    #[test]
    fn identifiers_are_quoted() {
        let changes = vec![RowChange::Delete {
            key: vec![cell("we\"ird", Value::Int(1))],
        }];
        let out = statements(&Ansi, &table(), &changes).unwrap();
        assert!(out[0].sql.ends_with(r#"WHERE "we""ird" = $1"#));
    }

    #[test]
    fn refuses_rows_without_a_usable_key() {
        let no_key = vec![RowChange::Delete { key: vec![] }];
        assert!(statements(&Ansi, &table(), &no_key).is_err());
        let empty_insert = vec![RowChange::Insert { values: vec![] }];
        assert!(statements(&Ansi, &table(), &empty_insert).is_err());
    }

    #[test]
    fn null_key_values_match_with_is_null() {
        // A table without a primary key is keyed by all its columns, some of them NULL.
        let changes = vec![RowChange::Update {
            key: vec![cell("a", Value::Int(1)), cell("b", Value::Null)],
            set: vec![cell("c", Value::Int(2))],
        }];
        let out = statements(&Ansi, &table(), &changes).unwrap();
        assert_eq!(
            out[0].sql,
            r#"UPDATE "shop"."orders" SET "c" = $1 WHERE "a" = $2 AND "b" IS NULL"#
        );
        assert_eq!(out[0].params, vec![Value::Int(2), Value::Int(1)]);
        assert!(out[0].one_row);
    }

    #[test]
    fn summary_counts_changes() {
        let changes = vec![
            RowChange::Delete {
                key: vec![cell("id", Value::Int(1))],
            },
            RowChange::Delete {
                key: vec![cell("id", Value::Int(2))],
            },
            RowChange::Insert {
                values: vec![cell("id", Value::Int(3))],
            },
        ];
        assert_eq!(
            Summary::of(&changes).describe(),
            "1 row inserted, 2 rows deleted"
        );
    }
}
