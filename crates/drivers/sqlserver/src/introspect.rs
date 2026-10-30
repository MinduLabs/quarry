//! What is in the database, read from the catalog views (`sys.objects`, `sys.columns`, …).
//!
//! The schema tree's databases are SQL Server's schemas, inside the database the connection
//! opened: a [`TableRef`] names a table by `database` (its schema) and `name`.

use quarry_driver_api::{Dialect, DriverError, Result};
use quarry_model::{
    ColumnInfo, ForeignKey, IndexInfo, ObjectKind, SchemaObject, ServerInfoKind, ServerInfoRow,
    TableColumns, TableInfo, TableRef,
};
use tiberius::{Row, ToSql};

use crate::{Conn, diagnose, dialect};

async fn rows(client: &mut Conn, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<Row>> {
    let stream = client
        .query(sql, params)
        .await
        .map_err(|err| diagnose::query_error(&err, sql))?;
    stream
        .into_first_result()
        .await
        .map_err(|err| diagnose::query_error(&err, sql))
}

fn text(row: &Row, index: usize) -> Result<String> {
    Ok(opt_text(row, index)?.unwrap_or_default())
}

fn opt_text(row: &Row, index: usize) -> Result<Option<String>> {
    row.try_get::<&str, _>(index)
        .map(|v| v.map(str::to_owned))
        .map_err(catalog)
}

fn flag(row: &Row, index: usize) -> Result<bool> {
    Ok(row
        .try_get::<bool, _>(index)
        .map_err(catalog)?
        .unwrap_or(false))
}

fn int(row: &Row, index: usize) -> Result<i64> {
    let value = match row.try_get::<i64, _>(index) {
        Ok(v) => v,
        Err(_) => row
            .try_get::<i32, _>(index)
            .map_err(catalog)?
            .map(i64::from),
    };
    Ok(value.unwrap_or(0))
}

fn catalog(err: tiberius::error::Error) -> DriverError {
    DriverError::Other(format!("unexpected catalog value: {err}").into())
}

pub async fn schemas(client: &mut Conn) -> Result<Vec<String>> {
    // Schemas owned by fixed roles (db_owner, …) have principal ids from 16384.
    rows(
        client,
        "SELECT name FROM sys.schemas \
         WHERE principal_id < 16384 AND name NOT IN ('sys', 'INFORMATION_SCHEMA', 'guest') \
         ORDER BY name",
        &[],
    )
    .await?
    .iter()
    .map(|row| text(row, 0))
    .collect()
}

pub async fn objects(client: &mut Conn, schema: &str) -> Result<Vec<SchemaObject>> {
    let found = rows(
        client,
        "SELECT o.name, RTRIM(o.type) FROM sys.objects o \
         JOIN sys.schemas s ON s.schema_id = o.schema_id \
         WHERE s.name = @P1 AND o.is_ms_shipped = 0 \
           AND o.type IN ('U', 'V', 'FN', 'IF', 'TF', 'P', 'TR', 'SO') \
         ORDER BY o.name",
        &[&schema],
    )
    .await?;
    found
        .iter()
        .map(|row| {
            let kind = match text(row, 1)?.as_str() {
                "V" => ObjectKind::View,
                "P" => ObjectKind::Procedure,
                "TR" => ObjectKind::Trigger,
                "SO" => ObjectKind::Sequence,
                "FN" | "IF" | "TF" => ObjectKind::Function,
                _ => ObjectKind::Table,
            };
            Ok(SchemaObject {
                name: text(row, 0)?,
                kind,
            })
        })
        .collect()
}

pub async fn columns(client: &mut Conn, schema: &str) -> Result<Vec<TableColumns>> {
    let found = rows(
        client,
        "SELECT o.name, c.name FROM sys.columns c \
         JOIN sys.objects o ON o.object_id = c.object_id \
         JOIN sys.schemas s ON s.schema_id = o.schema_id \
         WHERE s.name = @P1 AND o.type IN ('U', 'V') \
         ORDER BY o.name, c.column_id",
        &[&schema],
    )
    .await?;
    let mut tables: Vec<TableColumns> = Vec::new();
    for row in &found {
        let table = text(row, 0)?;
        let column = text(row, 1)?;
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

/// The schema a table reference points into; `dbo` when it names none.
fn schema_of(table: &TableRef) -> &str {
    table
        .schema
        .as_deref()
        .or(table.database.as_deref())
        .unwrap_or("dbo")
}

fn qualified(table: &TableRef) -> String {
    let d = dialect();
    format!(
        "{}.{}",
        d.quote_ident(schema_of(table)),
        d.quote_ident(&table.name)
    )
}

struct Object {
    id: i32,
    view: bool,
}

async fn object(client: &mut Conn, table: &TableRef) -> Result<Object> {
    let name = qualified(table);
    let found = rows(
        client,
        "SELECT o.object_id, RTRIM(o.type) FROM sys.objects o WHERE o.object_id = OBJECT_ID(@P1)",
        &[&name.as_str()],
    )
    .await?;
    let row = found.first().ok_or_else(|| DriverError::Query {
        message: format!("Table {name} not found."),
        location: None,
    })?;
    Ok(Object {
        id: row
            .try_get::<i32, _>(0)
            .map_err(catalog)?
            .unwrap_or_default(),
        view: text(row, 1)? == "V",
    })
}

async fn key_columns(client: &mut Conn, id: i32) -> Result<Vec<String>> {
    rows(
        client,
        "SELECT c.name FROM sys.indexes i \
         JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
         JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
         WHERE i.object_id = @P1 AND i.is_primary_key = 1 ORDER BY ic.key_ordinal",
        &[&id],
    )
    .await?
    .iter()
    .map(|row| text(row, 0))
    .collect()
}

pub async fn primary_key(client: &mut Conn, table: &TableRef) -> Result<Vec<String>> {
    let object = object(client, table).await?;
    key_columns(client, object.id).await
}

/// `nvarchar(50)`, `decimal(12,2)`, `datetime2(7)`, `varbinary(max)`, as written in DDL.
fn full_type(name: &str, max_length: i64, precision: i64, scale: i64) -> String {
    let length = |unit: i64| {
        if max_length < 0 {
            "max".to_owned()
        } else {
            (max_length / unit).to_string()
        }
    };
    match name {
        "varchar" | "char" | "varbinary" | "binary" => format!("{name}({})", length(1)),
        "nvarchar" | "nchar" => format!("{name}({})", length(2)),
        "decimal" | "numeric" => format!("{name}({precision},{scale})"),
        "datetime2" | "time" | "datetimeoffset" => format!("{name}({scale})"),
        _ => name.to_owned(),
    }
}

pub async fn table_info(client: &mut Conn, table: &TableRef) -> Result<TableInfo> {
    let object = object(client, table).await?;
    let id = object.id;

    let mut columns = Vec::new();
    for row in &rows(
        client,
        "SELECT c.name, TYPE_NAME(c.user_type_id), CAST(c.max_length AS int), \
                CAST(c.precision AS int), CAST(c.scale AS int), c.is_nullable, \
                OBJECT_DEFINITION(c.default_object_id), c.is_identity, c.is_computed, \
                cc.definition, CAST(ep.value AS nvarchar(4000)) \
         FROM sys.columns c \
         LEFT JOIN sys.computed_columns cc \
           ON cc.object_id = c.object_id AND cc.column_id = c.column_id \
         LEFT JOIN sys.extended_properties ep ON ep.class = 1 AND ep.major_id = c.object_id \
           AND ep.minor_id = c.column_id AND ep.name = 'MS_Description' \
         WHERE c.object_id = @P1 ORDER BY c.column_id",
        &[&id],
    )
    .await?
    {
        let identity = flag(row, 7)?;
        let generated = flag(row, 8)?;
        columns.push(ColumnInfo {
            name: text(row, 0)?,
            type_name: full_type(&text(row, 1)?, int(row, 2)?, int(row, 3)?, int(row, 4)?),
            nullable: flag(row, 5)?,
            default: opt_text(row, 6)?,
            extra: if identity {
                Some("IDENTITY".to_owned())
            } else if generated {
                opt_text(row, 9)?.map(|definition| format!("AS {definition}"))
            } else {
                None
            },
            comment: opt_text(row, 10)?,
            auto_increment: identity,
            on_update: None,
            generated,
        });
    }

    let primary_key = key_columns(client, id).await?;

    let mut indexes: Vec<IndexInfo> = Vec::new();
    for row in &rows(
        client,
        "SELECT i.name, i.is_unique, i.is_primary_key, c.name FROM sys.indexes i \
         JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
         JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
         WHERE i.object_id = @P1 AND i.name IS NOT NULL AND ic.is_included_column = 0 \
         ORDER BY i.name, ic.key_ordinal",
        &[&id],
    )
    .await?
    {
        let name = text(row, 0)?;
        let column = text(row, 3)?;
        match indexes.last_mut() {
            Some(last) if last.name == name => last.columns.push(column),
            _ => indexes.push(IndexInfo {
                name,
                unique: flag(row, 1)?,
                primary: flag(row, 2)?,
                columns: vec![column],
            }),
        }
    }

    let mut foreign_keys: Vec<ForeignKey> = Vec::new();
    for row in &rows(
        client,
        "SELECT fk.name, pc.name, rs.name, ro.name, rc.name, \
                fk.update_referential_action_desc, fk.delete_referential_action_desc \
         FROM sys.foreign_keys fk \
         JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
         JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id \
           AND pc.column_id = fkc.parent_column_id \
         JOIN sys.objects ro ON ro.object_id = fkc.referenced_object_id \
         JOIN sys.schemas rs ON rs.schema_id = ro.schema_id \
         JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id \
           AND rc.column_id = fkc.referenced_column_id \
         WHERE fk.parent_object_id = @P1 ORDER BY fk.name, fkc.constraint_column_id",
        &[&id],
    )
    .await?
    {
        let name = text(row, 0)?;
        let column = text(row, 1)?;
        let referenced = text(row, 4)?;
        match foreign_keys.last_mut() {
            Some(last) if last.name == name => {
                last.columns.push(column);
                last.referenced_columns.push(referenced);
            }
            _ => foreign_keys.push(ForeignKey {
                name,
                columns: vec![column],
                references: TableRef {
                    database: Some(text(row, 2)?),
                    schema: None,
                    name: text(row, 3)?,
                },
                referenced_columns: vec![referenced],
                on_update: Some(text(row, 5)?.replace('_', " ")),
                on_delete: Some(text(row, 6)?.replace('_', " ")),
            }),
        }
    }

    let ddl = if object.view {
        let found = rows(client, "SELECT OBJECT_DEFINITION(@P1)", &[&id]).await?;
        // The definition keeps whatever preceded CREATE in the batch that made it.
        found
            .first()
            .map(|row| opt_text(row, 0).map(|d| d.map(|d| d.trim().to_owned())))
            .transpose()?
            .flatten()
    } else {
        Some(table_ddl(table, &columns, &primary_key, &foreign_keys))
    };

    let estimated_rows = if object.view {
        None
    } else {
        let found = rows(
            client,
            "SELECT CAST(SUM(p.rows) AS bigint) FROM sys.partitions p \
             WHERE p.object_id = @P1 AND p.index_id IN (0, 1)",
            &[&id],
        )
        .await?;
        match found.first() {
            Some(row) => u64::try_from(int(row, 0)?).ok(),
            None => None,
        }
    };

    Ok(TableInfo {
        table: table.clone(),
        columns,
        primary_key,
        indexes,
        foreign_keys,
        ddl,
        estimated_rows,
        view: object.view,
    })
}

/// A `CREATE TABLE` rebuilt from the catalog: columns, the primary key, foreign keys.
fn table_ddl(
    table: &TableRef,
    columns: &[ColumnInfo],
    primary_key: &[String],
    foreign_keys: &[ForeignKey],
) -> String {
    let d = dialect();
    let names = |names: &[String]| {
        names
            .iter()
            .map(|n| d.quote_ident(n))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines: Vec<String> = columns
        .iter()
        .map(|c| {
            if c.generated {
                return format!(
                    "  {} {}",
                    d.quote_ident(&c.name),
                    c.extra.as_deref().unwrap_or_default()
                );
            }
            let mut line = format!("  {} {}", d.quote_ident(&c.name), c.type_name);
            if c.auto_increment {
                line.push_str(" IDENTITY");
            }
            if let Some(default) = &c.default {
                line.push_str(" DEFAULT ");
                line.push_str(default);
            }
            line.push_str(if c.nullable { " NULL" } else { " NOT NULL" });
            line
        })
        .collect();
    if !primary_key.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", names(primary_key)));
    }
    for fk in foreign_keys {
        lines.push(format!(
            "  CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
            d.quote_ident(&fk.name),
            names(&fk.columns),
            qualified(&fk.references),
            names(&fk.referenced_columns),
            fk.on_delete.as_deref().unwrap_or("NO ACTION"),
            fk.on_update.as_deref().unwrap_or("NO ACTION"),
        ));
    }
    format!(
        "CREATE TABLE {} (\n{}\n);",
        qualified(table),
        lines.join(",\n")
    )
}

pub async fn server_info(client: &mut Conn, kind: ServerInfoKind) -> Result<Vec<ServerInfoRow>> {
    let sql = match kind {
        ServerInfoKind::Variables => {
            "SELECT name, CAST(value_in_use AS nvarchar(256)) FROM sys.configurations ORDER BY name"
        }
        // Needs VIEW SERVER STATE.
        ServerInfoKind::Status => {
            "SELECT RTRIM(counter_name), CAST(cntr_value AS nvarchar(32)) \
             FROM sys.dm_os_performance_counters \
             WHERE object_name LIKE '%:General Statistics%' ORDER BY counter_name"
        }
    };
    rows(client, sql, &[])
        .await?
        .iter()
        .map(|row| {
            Ok(ServerInfoRow {
                name: text(row, 0)?,
                value: opt_text(row, 1)?.unwrap_or_default(),
            })
        })
        .collect()
}
