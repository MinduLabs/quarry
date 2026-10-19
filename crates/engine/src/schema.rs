//! Schema changes (reviewed, then run) and `INSERT` text for copying rows.

use quarry_driver_api::Dialect;
use quarry_model::{Environment, SchemaChange, TableRef, Value};

/// The statements for `change`, each checked to be exactly one statement: a type or default typed
/// by the user must not smuggle in a second one.
pub fn statements(dialect: &dyn Dialect, change: &SchemaChange) -> Result<Vec<String>, String> {
    let statements = dialect
        .schema_change(change)
        .ok_or_else(|| "This database cannot change its schema from Quarry yet.".to_owned())??;
    for statement in &statements {
        if quarry_sql::split(statement).len() != 1 {
            return Err(
                "A type or default contains a “;” that ends the statement. Remove it.".to_owned(),
            );
        }
    }
    Ok(statements)
}

/// What the review of a schema change warns about, or `None`.
pub fn warning(environment: Environment, change: &SchemaChange) -> Option<String> {
    let destructive = change.is_destructive();
    match (environment, destructive) {
        (Environment::Production, true) => Some(
            "This changes a production database and can delete data. It can't be undone."
                .to_owned(),
        ),
        (Environment::Production, false) => {
            Some("This changes the schema of a production database.".to_owned())
        }
        (_, true) => Some("This can delete data. It can't be undone.".to_owned()),
        (_, false) => None,
    }
}

/// One multi-row `INSERT` for `rows`, with values as literals of the dialect.
pub fn insert_text(
    dialect: &dyn Dialect,
    table: &TableRef,
    columns: &[String],
    rows: &[Vec<Value>],
) -> Result<String, String> {
    if columns.is_empty() || rows.is_empty() {
        return Err("Select some cells to copy.".to_owned());
    }
    let names: Vec<String> = columns.iter().map(|c| dialect.quote_ident(c)).collect();
    let values: Vec<String> = rows
        .iter()
        .map(|row| {
            let cells: Vec<String> = row.iter().map(|v| dialect.literal(v)).collect();
            format!("({})", cells.join(", "))
        })
        .collect();
    Ok(format!(
        "INSERT INTO {} ({}) VALUES\n  {};",
        dialect.qualify(table),
        names.join(", "),
        values.join(",\n  ")
    ))
}

#[cfg(test)]
mod tests {
    use quarry_model::{AlterOp, ColumnDef};

    use super::*;
    use crate::edit::tests::Ansi;

    fn table() -> TableRef {
        TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        }
    }

    #[test]
    fn insert_text_quotes_names_and_values() {
        let text = insert_text(
            &Ansi,
            &table(),
            &["id".into(), "note".into()],
            &[
                vec![Value::Int(1), Value::Text("it's".into())],
                vec![Value::Int(2), Value::Null],
            ],
        )
        .unwrap();
        assert_eq!(
            text,
            "INSERT INTO \"shop\".\"orders\" (\"id\", \"note\") VALUES\n  (1, 'it''s'),\n  (2, NULL);"
        );
        assert!(insert_text(&Ansi, &table(), &[], &[]).is_err());
    }

    #[test]
    fn schema_warnings() {
        let drop = SchemaChange::DropTable {
            table: table(),
            view: false,
        };
        let add = SchemaChange::AlterTable {
            table: table(),
            ops: vec![AlterOp::AddColumn {
                column: ColumnDef {
                    name: "a".into(),
                    type_name: "int".into(),
                    nullable: true,
                    default: None,
                    auto_increment: false,
                    on_update: None,
                    comment: None,
                },
            }],
        };
        assert_eq!(warning(Environment::Local, &add), None);
        assert!(warning(Environment::Local, &drop).is_some());
        assert!(warning(Environment::Staging, &drop).is_some());
        assert!(warning(Environment::Production, &add).is_some());
    }

    #[test]
    fn a_default_cannot_add_a_statement() {
        let column = |default: &str| ColumnDef {
            name: "a".into(),
            type_name: "varchar(10)".into(),
            nullable: true,
            default: Some(default.into()),
            auto_increment: false,
            on_update: None,
            comment: None,
        };
        let change = |default: &str| SchemaChange::AlterTable {
            table: table(),
            ops: vec![AlterOp::AddColumn {
                column: column(default),
            }],
        };
        let mysql = quarry_driver_mysql::MysqlDialect;
        assert!(statements(&mysql, &change("'a;b'")).is_ok());
        assert!(statements(&mysql, &change("0; DROP TABLE orders")).is_err());
    }

    #[test]
    fn dialects_without_ddl_say_so() {
        let change = SchemaChange::CreateDatabase { name: "x".into() };
        assert!(statements(&Ansi, &change).is_err());
    }
}
