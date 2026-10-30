//! What is in the database, read from the system catalogs (`pg_class`, `pg_attribute`, …).
//!
//! The schema tree's databases are PostgreSQL's schemas, inside the database the connection
//! opened: a [`TableRef`] names a table by `database` (its schema) and `name`.

use std::collections::HashMap;

use quarry_driver_api::{Dialect, DriverError, Result};
use quarry_model::{
    ColumnInfo, ColumnMeta, ForeignKey, IndexInfo, ObjectKind, SchemaObject, ServerInfoKind,
    ServerInfoRow, TableColumns, TableInfo, TableRef,
};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

use crate::{diagnose, dialect};

/// Schemas a person works in: not the catalogs, TOAST or other sessions' temporary schemas.
const USER_SCHEMAS: &str = "n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') \
     AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%'";

async fn rows(
    client: &Client,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Vec<tokio_postgres::Row>> {
    client
        .query(sql, params)
        .await
        .map_err(|err| diagnose::query_error(&err, sql))
}

fn get<'a, T: tokio_postgres::types::FromSql<'a>>(
    row: &'a tokio_postgres::Row,
    index: usize,
) -> Result<T> {
    row.try_get(index)
        .map_err(|err| DriverError::Other(format!("unexpected catalog value: {err}").into()))
}

pub async fn schemas(client: &Client) -> Result<Vec<String>> {
    let sql =
        format!("SELECT n.nspname FROM pg_namespace n WHERE {USER_SCHEMAS} ORDER BY n.nspname");
    rows(client, &sql, &[])
        .await?
        .iter()
        .map(|row| get(row, 0))
        .collect()
}

pub async fn objects(client: &Client, schema: &str) -> Result<Vec<SchemaObject>> {
    let relations = rows(
        client,
        "SELECT c.relname, c.relkind::text FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'f', 'v', 'm', 'S') \
         ORDER BY c.relname",
        &[&schema],
    )
    .await?;
    let routines = rows(
        client,
        "SELECT DISTINCT p.proname, p.prokind::text FROM pg_proc p \
         JOIN pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname = $1 AND p.prokind IN ('f', 'p') ORDER BY p.proname",
        &[&schema],
    )
    .await?;
    let triggers = rows(
        client,
        "SELECT DISTINCT t.tgname FROM pg_trigger t \
         JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND NOT t.tgisinternal ORDER BY t.tgname",
        &[&schema],
    )
    .await?;

    let mut objects = Vec::with_capacity(relations.len() + routines.len() + triggers.len());
    for row in &relations {
        let kind: String = get(row, 1)?;
        let kind = match kind.as_str() {
            "v" => ObjectKind::View,
            "m" => ObjectKind::MaterializedView,
            "S" => ObjectKind::Sequence,
            _ => ObjectKind::Table,
        };
        objects.push(SchemaObject {
            name: get(row, 0)?,
            kind,
        });
    }
    for row in &routines {
        let kind: String = get(row, 1)?;
        objects.push(SchemaObject {
            name: get(row, 0)?,
            kind: if kind == "p" {
                ObjectKind::Procedure
            } else {
                ObjectKind::Function
            },
        });
    }
    for row in &triggers {
        objects.push(SchemaObject {
            name: get(row, 0)?,
            kind: ObjectKind::Trigger,
        });
    }
    Ok(objects)
}

pub async fn columns(client: &Client, schema: &str) -> Result<Vec<TableColumns>> {
    let found = rows(
        client,
        "SELECT c.relname, a.attname FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'f', 'v', 'm') \
           AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY c.relname, a.attnum",
        &[&schema],
    )
    .await?;
    let mut tables: Vec<TableColumns> = Vec::new();
    for row in &found {
        let table: String = get(row, 0)?;
        let column: String = get(row, 1)?;
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

/// The schema a table reference points into; `public` when it names none.
fn schema_of(table: &TableRef) -> &str {
    table
        .schema
        .as_deref()
        .or(table.database.as_deref())
        .unwrap_or("public")
}

struct Relation {
    oid: u32,
    kind: String,
    estimated_rows: f32,
}

async fn relation(client: &Client, table: &TableRef) -> Result<Relation> {
    let schema = schema_of(table);
    let found = rows(
        client,
        "SELECT c.oid, c.relkind::text, c.reltuples FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2",
        &[&schema, &table.name],
    )
    .await?;
    let row = found.first().ok_or_else(|| DriverError::Query {
        message: format!("Table “{schema}.{}” not found.", table.name),
        location: None,
    })?;
    Ok(Relation {
        oid: get(row, 0)?,
        kind: get(row, 1)?,
        estimated_rows: get(row, 2)?,
    })
}

async fn key_columns(client: &Client, oid: u32) -> Result<Vec<String>> {
    rows(
        client,
        "SELECT a.attname FROM pg_index i \
         JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum \
         WHERE i.indrelid = $1 AND i.indisprimary ORDER BY k.ord",
        &[&oid],
    )
    .await?
    .iter()
    .map(|row| get(row, 0))
    .collect()
}

pub async fn primary_key(client: &Client, table: &TableRef) -> Result<Vec<String>> {
    let relation = relation(client, table).await?;
    key_columns(client, relation.oid).await
}

pub async fn table_info(client: &Client, table: &TableRef) -> Result<TableInfo> {
    let relation = relation(client, table).await?;
    let oid = relation.oid;
    let view = matches!(relation.kind.as_str(), "v" | "m");

    let mut columns = Vec::new();
    for row in &rows(
        client,
        "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
                pg_get_expr(d.adbin, d.adrelid), col_description(a.attrelid, a.attnum), \
                a.attidentity::text, a.attgenerated::text \
         FROM pg_attribute a \
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
        &[&oid],
    )
    .await?
    {
        let default: Option<String> = get(row, 3)?;
        let identity: String = get(row, 5)?;
        let generated = get::<String>(row, 6)? == "s";
        let serial = default
            .as_deref()
            .is_some_and(|d| d.starts_with("nextval("));
        let extra = match identity.as_str() {
            "a" => Some("GENERATED ALWAYS AS IDENTITY".to_owned()),
            "d" => Some("GENERATED BY DEFAULT AS IDENTITY".to_owned()),
            _ if generated => default
                .as_ref()
                .map(|e| format!("GENERATED ALWAYS AS ({e}) STORED")),
            _ => None,
        };
        columns.push(ColumnInfo {
            name: get(row, 0)?,
            type_name: get(row, 1)?,
            nullable: get(row, 2)?,
            default: if generated { None } else { default },
            extra,
            comment: get(row, 4)?,
            auto_increment: serial || !identity.is_empty(),
            on_update: None,
            generated,
        });
    }

    let primary_key = key_columns(client, oid).await?;

    let mut indexes = Vec::new();
    for row in &rows(
        client,
        "SELECT ic.relname, i.indisunique, i.indisprimary, \
                ARRAY(SELECT pg_get_indexdef(i.indexrelid, k, true) \
                      FROM generate_series(1, i.indnkeyatts) AS k ORDER BY k) \
         FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid \
         WHERE i.indrelid = $1 ORDER BY ic.relname",
        &[&oid],
    )
    .await?
    {
        indexes.push(IndexInfo {
            name: get(row, 0)?,
            unique: get(row, 1)?,
            primary: get(row, 2)?,
            columns: get(row, 3)?,
        });
    }

    let mut foreign_keys = Vec::new();
    for row in &rows(
        client,
        "SELECT con.conname, \
                ARRAY(SELECT a.attname FROM unnest(con.conkey) WITH ORDINALITY AS k(n, ord) \
                      JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n \
                      ORDER BY k.ord), \
                fn.nspname, fc.relname, \
                ARRAY(SELECT a.attname FROM unnest(con.confkey) WITH ORDINALITY AS k(n, ord) \
                      JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n \
                      ORDER BY k.ord), \
                con.confupdtype::text, con.confdeltype::text \
         FROM pg_constraint con \
         JOIN pg_class fc ON fc.oid = con.confrelid \
         JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
         WHERE con.conrelid = $1 AND con.contype = 'f' ORDER BY con.conname",
        &[&oid],
    )
    .await?
    {
        foreign_keys.push(ForeignKey {
            name: get(row, 0)?,
            columns: get(row, 1)?,
            references: TableRef {
                database: Some(get(row, 2)?),
                schema: None,
                name: get(row, 3)?,
            },
            referenced_columns: get(row, 4)?,
            on_update: action(&get::<String>(row, 5)?),
            on_delete: action(&get::<String>(row, 6)?),
        });
    }

    let ddl = if view {
        view_ddl(client, table, oid, relation.kind == "m").await?
    } else {
        table_ddl(client, table, oid, &columns).await?
    };

    Ok(TableInfo {
        table: table.clone(),
        columns,
        primary_key,
        indexes,
        foreign_keys,
        ddl: Some(ddl),
        // -1 until the table is first analyzed.
        estimated_rows: (!view && relation.estimated_rows >= 0.0)
            .then_some(relation.estimated_rows as u64),
        view,
    })
}

fn action(code: &str) -> Option<String> {
    Some(
        match code {
            "r" => "RESTRICT",
            "c" => "CASCADE",
            "n" => "SET NULL",
            "d" => "SET DEFAULT",
            _ => "NO ACTION",
        }
        .to_owned(),
    )
}

fn qualified(table: &TableRef) -> String {
    let d = dialect();
    format!(
        "{}.{}",
        d.quote_ident(schema_of(table)),
        d.quote_ident(&table.name)
    )
}

async fn view_ddl(
    client: &Client,
    table: &TableRef,
    oid: u32,
    materialized: bool,
) -> Result<String> {
    let found = rows(client, "SELECT pg_get_viewdef($1::oid, true)", &[&oid]).await?;
    let body: String = found
        .first()
        .map(|row| get(row, 0))
        .transpose()?
        .unwrap_or_default();
    let kind = if materialized {
        "MATERIALIZED VIEW"
    } else {
        "VIEW"
    };
    Ok(format!(
        "CREATE {kind} {} AS\n{}",
        qualified(table),
        body.trim_end()
    ))
}

/// A `CREATE TABLE` rebuilt from the catalog, with its constraints, then its other indexes.
async fn table_ddl(
    client: &Client,
    table: &TableRef,
    oid: u32,
    columns: &[ColumnInfo],
) -> Result<String> {
    let d = dialect();
    let mut lines: Vec<String> = columns
        .iter()
        .map(|c| {
            let mut line = format!("  {} {}", d.quote_ident(&c.name), c.type_name);
            if let Some(extra) = &c.extra {
                line.push(' ');
                line.push_str(extra);
            }
            if let Some(default) = &c.default {
                line.push_str(" DEFAULT ");
                line.push_str(default);
            }
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
            line
        })
        .collect();
    for row in &rows(
        client,
        "SELECT conname, pg_get_constraintdef(oid, true) FROM pg_constraint \
         WHERE conrelid = $1 ORDER BY contype <> 'p', conname",
        &[&oid],
    )
    .await?
    {
        let name: String = get(row, 0)?;
        let definition: String = get(row, 1)?;
        lines.push(format!(
            "  CONSTRAINT {} {definition}",
            d.quote_ident(&name)
        ));
    }
    let mut ddl = format!(
        "CREATE TABLE {} (\n{}\n);",
        qualified(table),
        lines.join(",\n")
    );
    for row in &rows(
        client,
        "SELECT pg_get_indexdef(i.indexrelid) FROM pg_index i \
         WHERE i.indrelid = $1 \
           AND NOT EXISTS (SELECT 1 FROM pg_constraint c WHERE c.conindid = i.indexrelid) \
         ORDER BY i.indexrelid",
        &[&oid],
    )
    .await?
    {
        let index: String = get(row, 0)?;
        ddl.push_str("\n\n");
        ddl.push_str(&index);
        ddl.push(';');
    }
    Ok(ddl)
}

pub async fn server_info(client: &Client, kind: ServerInfoKind) -> Result<Vec<ServerInfoRow>> {
    let sql = match kind {
        ServerInfoKind::Variables => "SELECT name, setting FROM pg_settings ORDER BY name",
        // Counters of the open database, one row per column of pg_stat_database.
        ServerInfoKind::Status => {
            "SELECT s.key, coalesce(s.value, '') FROM pg_stat_database d, \
             jsonb_each_text(to_jsonb(d)) AS s(key, value) \
             WHERE d.datname = current_database() ORDER BY s.key"
        }
    };
    rows(client, sql, &[])
        .await?
        .iter()
        .map(|row| {
            Ok(ServerInfoRow {
                name: get(row, 0)?,
                value: get(row, 1)?,
            })
        })
        .collect()
}

/// A table that result columns come from.
#[derive(Debug)]
pub struct Source {
    schema: String,
    table: String,
    columns: HashMap<i16, String>,
}

/// Result columns with the table and column each one reads, when it reads one. Tables are
/// looked up once per session and kept in `cache`.
pub async fn column_metas(
    client: &Client,
    cache: &mut HashMap<u32, Source>,
    columns: &[tokio_postgres::Column],
) -> Vec<ColumnMeta> {
    let mut missing: Vec<u32> = columns
        .iter()
        .filter_map(tokio_postgres::Column::table_oid)
        .filter(|oid| *oid != 0 && !cache.contains_key(oid))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if !missing.is_empty()
        && let Err(err) = load_sources(client, cache, &missing).await
    {
        // Without sources the result is shown, only not editable.
        tracing::debug!(%err, "result column sources");
    }
    columns
        .iter()
        .map(|column| {
            let source = column
                .table_oid()
                .and_then(|oid| cache.get(&oid))
                .and_then(|source| {
                    let name = source.columns.get(&column.column_id()?)?;
                    Some((source, name))
                });
            ColumnMeta {
                name: column.name().to_owned(),
                type_name: column.type_().name().to_owned(),
                nullable: None,
                source_database: source.map(|(s, _)| s.schema.clone()),
                source_table: source.map(|(s, _)| s.table.clone()),
                source_column: source.map(|(_, name)| name.clone()),
            }
        })
        .collect()
}

async fn load_sources(
    client: &Client,
    cache: &mut HashMap<u32, Source>,
    oids: &[u32],
) -> std::result::Result<(), tokio_postgres::Error> {
    let found = client
        .query(
            "SELECT c.oid, n.nspname, c.relname, a.attnum, a.attname FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
             WHERE c.oid = ANY($1)",
            &[&oids],
        )
        .await?;
    for row in &found {
        let oid: u32 = row.try_get(0)?;
        let source = cache.entry(oid).or_insert_with(|| Source {
            schema: row.try_get(1).unwrap_or_default(),
            table: row.try_get(2).unwrap_or_default(),
            columns: HashMap::new(),
        });
        source
            .columns
            .insert(row.try_get(3)?, row.try_get::<_, String>(4)?);
    }
    Ok(())
}
