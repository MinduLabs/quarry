//! MySQL statements for a [`SchemaChange`].

use quarry_driver_api::Dialect;
use quarry_model::{AlterOp, ColumnDef, IndexDef, SchemaChange, Value};

use crate::MysqlDialect;

pub fn statements(d: &MysqlDialect, change: &SchemaChange) -> Result<Vec<String>, String> {
    let sql = match change {
        SchemaChange::CreateDatabase { name } => {
            format!(
                "CREATE DATABASE {}",
                d.quote_ident(named(name, "database")?)
            )
        }
        SchemaChange::DropDatabase { name } => {
            format!("DROP DATABASE {}", d.quote_ident(named(name, "database")?))
        }
        SchemaChange::CreateTable {
            table,
            columns,
            primary_key,
        } => {
            named(&table.name, "table")?;
            if columns.is_empty() {
                return Err("A table needs at least one column.".to_owned());
            }
            let mut lines = columns
                .iter()
                .map(|c| column(d, c))
                .collect::<Result<Vec<_>, _>>()?;
            if !primary_key.is_empty() {
                lines.push(format!("PRIMARY KEY ({})", idents(d, primary_key)));
            }
            format!(
                "CREATE TABLE {} (\n  {}\n)",
                d.qualify(table),
                lines.join(",\n  ")
            )
        }
        SchemaChange::RenameTable { table, new_name } => {
            let mut target = table.clone();
            target.name = named(new_name, "table")?.to_owned();
            format!(
                "RENAME TABLE {} TO {}",
                d.qualify(table),
                d.qualify(&target)
            )
        }
        SchemaChange::TruncateTable { table } => format!("TRUNCATE TABLE {}", d.qualify(table)),
        SchemaChange::DropTable { table, view } => format!(
            "DROP {} {}",
            if *view { "VIEW" } else { "TABLE" },
            d.qualify(table)
        ),
        SchemaChange::AlterTable { table, ops } => {
            if ops.is_empty() {
                return Err("Nothing to change.".to_owned());
            }
            let clauses = ops
                .iter()
                .map(|op| alter(d, op))
                .collect::<Result<Vec<_>, _>>()?;
            format!(
                "ALTER TABLE {}\n  {}",
                d.qualify(table),
                clauses.join(",\n  ")
            )
        }
    };
    Ok(vec![sql])
}

fn alter(d: &MysqlDialect, op: &AlterOp) -> Result<String, String> {
    Ok(match op {
        AlterOp::AddColumn { column: c } => format!("ADD COLUMN {}", column(d, c)?),
        AlterOp::ModifyColumn { name, column: c } => format!(
            "CHANGE COLUMN {} {}",
            d.quote_ident(named(name, "column")?),
            column(d, c)?
        ),
        AlterOp::DropColumn { name } => {
            format!("DROP COLUMN {}", d.quote_ident(named(name, "column")?))
        }
        AlterOp::AddIndex { index } => add_index(d, index)?,
        AlterOp::DropIndex { name } if name == "PRIMARY" => "DROP PRIMARY KEY".to_owned(),
        AlterOp::DropIndex { name } => {
            format!("DROP INDEX {}", d.quote_ident(named(name, "index")?))
        }
    })
}

fn add_index(d: &MysqlDialect, index: &IndexDef) -> Result<String, String> {
    if index.columns.is_empty() {
        return Err("Pick at least one column for the index.".to_owned());
    }
    Ok(format!(
        "ADD {}INDEX {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        d.quote_ident(named(&index.name, "index")?),
        idents(d, &index.columns)
    ))
}

fn column(d: &MysqlDialect, c: &ColumnDef) -> Result<String, String> {
    named(&c.name, "column")?;
    let type_name = c.type_name.trim();
    if type_name.is_empty() {
        return Err(format!("Give the column “{}” a type.", c.name));
    }
    let mut sql = format!(
        "{} {type_name} {}",
        d.quote_ident(&c.name),
        if c.nullable { "NULL" } else { "NOT NULL" }
    );
    if let Some(default) = c
        .default
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sql.push_str(" DEFAULT ");
        sql.push_str(default);
    }
    if let Some(on_update) = c
        .on_update
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sql.push_str(" ON UPDATE ");
        sql.push_str(on_update);
    }
    if c.auto_increment {
        sql.push_str(" AUTO_INCREMENT");
    }
    if let Some(comment) = c.comment.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(" COMMENT ");
        sql.push_str(&d.literal(&Value::Text(comment.to_owned())));
    }
    Ok(sql)
}

fn idents(d: &MysqlDialect, names: &[String]) -> String {
    names
        .iter()
        .map(|n| d.quote_ident(n))
        .collect::<Vec<_>>()
        .join(", ")
}

fn named<'a>(name: &'a str, what: &str) -> Result<&'a str, String> {
    if name.trim().is_empty() {
        Err(format!("Give the {what} a name."))
    } else {
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use quarry_model::TableRef;

    use super::*;

    fn table() -> TableRef {
        TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        }
    }

    fn col(name: &str, type_name: &str, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            type_name: type_name.into(),
            nullable,
            default: None,
            auto_increment: false,
            on_update: None,
            comment: None,
        }
    }

    fn one(change: SchemaChange) -> String {
        let mut out = statements(&MysqlDialect, &change).unwrap();
        assert_eq!(out.len(), 1);
        out.remove(0)
    }

    #[test]
    fn create_table() {
        let mut id = col("id", "bigint unsigned", false);
        id.auto_increment = true;
        let mut note = col("note", "varchar(255)", true);
        note.default = Some("'new'".into());
        note.comment = Some("it's a \\ note".into());
        let sql = one(SchemaChange::CreateTable {
            table: table(),
            columns: vec![id, note],
            primary_key: vec!["id".into()],
        });
        assert_eq!(
            sql,
            "CREATE TABLE `shop`.`orders` (\n  \
             `id` bigint unsigned NOT NULL AUTO_INCREMENT,\n  \
             `note` varchar(255) NULL DEFAULT 'new' COMMENT 'it''s a \\\\ note',\n  \
             PRIMARY KEY (`id`)\n)"
        );
    }

    #[test]
    fn alter_table() {
        let sql = one(SchemaChange::AlterTable {
            table: table(),
            ops: vec![
                AlterOp::AddColumn {
                    column: col("paid", "tinyint(1)", false),
                },
                AlterOp::ModifyColumn {
                    name: "old".into(),
                    column: col("new", "int", true),
                },
                AlterOp::DropColumn { name: "x`y".into() },
                AlterOp::AddIndex {
                    index: IndexDef {
                        name: "by_paid".into(),
                        columns: vec!["paid".into(), "id".into()],
                        unique: true,
                    },
                },
                AlterOp::DropIndex {
                    name: "PRIMARY".into(),
                },
            ],
        });
        assert_eq!(
            sql,
            "ALTER TABLE `shop`.`orders`\n  \
             ADD COLUMN `paid` tinyint(1) NOT NULL,\n  \
             CHANGE COLUMN `old` `new` int NULL,\n  \
             DROP COLUMN `x``y`,\n  \
             ADD UNIQUE INDEX `by_paid` (`paid`, `id`),\n  \
             DROP PRIMARY KEY"
        );
    }

    #[test]
    fn simple_changes() {
        assert_eq!(
            one(SchemaChange::RenameTable {
                table: table(),
                new_name: "sales".into()
            }),
            "RENAME TABLE `shop`.`orders` TO `shop`.`sales`"
        );
        assert_eq!(
            one(SchemaChange::DropTable {
                table: table(),
                view: true
            }),
            "DROP VIEW `shop`.`orders`"
        );
        assert_eq!(
            one(SchemaChange::CreateDatabase { name: "new".into() }),
            "CREATE DATABASE `new`"
        );
    }

    #[test]
    fn rejects_incomplete_changes() {
        let empty_name = SchemaChange::CreateTable {
            table: TableRef {
                name: " ".into(),
                ..table()
            },
            columns: vec![col("id", "int", false)],
            primary_key: vec![],
        };
        assert!(statements(&MysqlDialect, &empty_name).is_err());
        let no_type = SchemaChange::AlterTable {
            table: table(),
            ops: vec![AlterOp::AddColumn {
                column: col("a", "", true),
            }],
        };
        assert!(statements(&MysqlDialect, &no_type).is_err());
        let no_ops = SchemaChange::AlterTable {
            table: table(),
            ops: vec![],
        };
        assert!(statements(&MysqlDialect, &no_ops).is_err());
    }
}
