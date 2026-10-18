//! Table structure from `information_schema` and `SHOW CREATE TABLE`.

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row};
use quarry_driver_api::{Dialect, Result};
use quarry_model::{ColumnInfo, ForeignKey, IndexInfo, TableColumns, TableInfo, TableRef};

use crate::{MysqlDialect, map_err};

const PRIMARY: &str = "PRIMARY";

pub async fn table_info(conn: &mut Conn, table: &TableRef) -> Result<TableInfo> {
    let database = match &table.database {
        Some(db) => db.clone(),
        None => conn
            .query_first::<Option<String>, _>("SELECT DATABASE()")
            .await
            .map_err(map_err)?
            .flatten()
            .unwrap_or_default(),
    };
    let key = (database.as_str(), table.name.as_str());

    type ColumnRow = (String, String, String, Option<String>, String, String);
    let columns: Vec<ColumnRow> = conn
        .exec(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            key,
        )
        .await
        .map_err(map_err)?;
    let columns = columns
        .into_iter()
        .map(
            |(name, type_name, nullable, default, extra, comment)| ColumnInfo {
                name,
                type_name,
                nullable: nullable == "YES",
                default,
                extra: non_empty(extra),
                comment: non_empty(comment),
            },
        )
        .collect();

    // EXPRESSION exists from MySQL 8.0.13; COLUMN_NAME is NULL for expression indexes.
    let index_rows: Vec<(String, i64, Option<String>)> = conn
        .exec(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY INDEX_NAME = 'PRIMARY' DESC, \
             INDEX_NAME, SEQ_IN_INDEX",
            key,
        )
        .await
        .map_err(map_err)?;
    let mut indexes: Vec<IndexInfo> = Vec::new();
    for (name, non_unique, column) in index_rows {
        let column = column.unwrap_or_else(|| "(expression)".to_owned());
        match indexes.last_mut() {
            Some(last) if last.name == name => last.columns.push(column),
            _ => indexes.push(IndexInfo {
                primary: name == PRIMARY,
                unique: non_unique == 0,
                columns: vec![column],
                name,
            }),
        }
    }
    let primary_key = indexes
        .iter()
        .find(|i| i.primary)
        .map(|i| i.columns.clone())
        .unwrap_or_default();

    type ForeignKeyRow = (String, String, String, String, String, String, String);
    let fk_rows: Vec<ForeignKeyRow> = conn
        .exec(
            "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, \
             k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE \
             FROM information_schema.KEY_COLUMN_USAGE k \
             JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
               ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
               AND r.TABLE_NAME = k.TABLE_NAME \
             WHERE k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
            key,
        )
        .await
        .map_err(map_err)?;
    let mut foreign_keys: Vec<ForeignKey> = Vec::new();
    for (name, column, ref_db, ref_table, ref_column, on_update, on_delete) in fk_rows {
        match foreign_keys.last_mut() {
            Some(last) if last.name == name => {
                last.columns.push(column);
                last.referenced_columns.push(ref_column);
            }
            _ => foreign_keys.push(ForeignKey {
                name,
                columns: vec![column],
                references: TableRef {
                    database: Some(ref_db),
                    schema: None,
                    name: ref_table,
                },
                referenced_columns: vec![ref_column],
                on_update: Some(on_update),
                on_delete: Some(on_delete),
            }),
        }
    }

    let qualified = format!(
        "{}.{}",
        MysqlDialect.quote_ident(&database),
        MysqlDialect.quote_ident(&table.name)
    );
    // Column 1 is the statement for both tables (`Create Table`) and views (`Create View`).
    let ddl = conn
        .query_first::<Row, _>(format!("SHOW CREATE TABLE {qualified}"))
        .await
        .map_err(map_err)?
        .and_then(|row| row.get_opt::<String, _>(1).and_then(|v| v.ok()));

    // InnoDB samples this, so it can be far off; views report NULL.
    let estimated_rows: Option<u64> = conn
        .exec_first::<Option<u64>, _, _>(
            "SELECT TABLE_ROWS FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
            key,
        )
        .await
        .map_err(map_err)?
        .flatten();

    Ok(TableInfo {
        table: TableRef {
            database: Some(database),
            schema: None,
            name: table.name.clone(),
        },
        columns,
        primary_key,
        indexes,
        foreign_keys,
        ddl,
        estimated_rows,
    })
}

/// Column names of every table and view in `database`, in table and column order.
pub async fn columns(conn: &mut Conn, database: &str) -> Result<Vec<TableColumns>> {
    let rows: Vec<(String, String)> = conn
        .exec(
            "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (database,),
        )
        .await
        .map_err(map_err)?;
    let mut tables: Vec<TableColumns> = Vec::new();
    for (table, column) in rows {
        match tables.last_mut() {
            Some(last) if last.table == table => last.columns.push(column),
            _ => tables.push(TableColumns {
                table,
                columns: vec![column],
            }),
        }
    }
    Ok(tables)
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}
