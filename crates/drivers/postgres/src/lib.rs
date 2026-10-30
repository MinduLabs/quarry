//! PostgreSQL driver, built on `tokio-postgres` (TLS via rustls).
//!
//! The schema tree's databases are the schemas of the database the connection opens (the
//! connection's Default database): a table is `"schema"."table"`, and choosing a database in a
//! tab sets the `search_path`.

mod decode;
mod diagnose;
mod introspect;
mod params;
mod tls;

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::{StreamExt, TryStreamExt};
use quarry_driver_api::{
    CancelHandle, Dialect, Driver, ExecItem, ExecOpts, ExecSink, FnSink, ResolvedConfig, Result,
    RowSink, Session, SharedCancel, SpecDialect,
};
use quarry_model::{
    Capabilities, DialectSpec, DriverId, ExplainSyntax, IdentQuote, LiteralStyle, ObjectKind,
    PagingStyle, PlaceholderStyle, SchemaObject, ServerInfoKind, ServerInfoRow, TableColumns,
    TableInfo, TableRef, Value,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_postgres::types::Type;
use tokio_postgres::{AsyncMessage, CancelToken, Client, Row, Statement};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::decode::Raw;
use crate::params::TextParam;

pub const DRIVER_ID: &str = "postgres";
pub const DEFAULT_PORT: u16 = 5432;
const DEFAULT_HOST: &str = "127.0.0.1";
/// libpq signs in as the operating-system user; a desktop app has no meaningful one.
const DEFAULT_USER: &str = "postgres";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// PostgreSQL syntax, with `standard_conforming_strings` on (the default since 9.1).
pub(crate) fn dialect() -> &'static SpecDialect {
    static DIALECT: LazyLock<SpecDialect> = LazyLock::new(|| {
        SpecDialect::new(DialectSpec {
            quote: IdentQuote::Double,
            on_duplicate: quarry_model::DuplicateSyntax::OnConflict,
            paging: PagingStyle::LimitOffset,
            placeholder: PlaceholderStyle::Numbered,
            literals: LiteralStyle {
                backslash_escapes: false,
                bool_as_int: false,
                national_strings: false,
                hex_bytes: false,
            },
            explain: Some(ExplainSyntax {
                table: "EXPLAIN".to_owned(),
                json: Some("EXPLAIN (FORMAT JSON)".to_owned()),
            }),
        })
    });
    &DIALECT
}

#[derive(Debug, Default)]
pub struct PostgresDriver;

#[async_trait]
impl Driver for PostgresDriver {
    fn id(&self) -> DriverId {
        DriverId(DRIVER_ID.to_owned())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // Schemas are the tree's databases, so there is no level below them.
            schemas: false,
            transactions: true,
            editable_results: true,
            explain: true,
            schema_edit: false,
            server_info: true,
            cancel: true,
            ssh: true,
            ssl: true,
            object_kinds: vec![
                ObjectKind::Database,
                ObjectKind::Table,
                ObjectKind::View,
                ObjectKind::MaterializedView,
                ObjectKind::Sequence,
                ObjectKind::Function,
                ObjectKind::Procedure,
                ObjectKind::Trigger,
            ],
        }
    }

    fn dialect(&self) -> &SpecDialect {
        dialect()
    }

    async fn connect(&self, config: &ResolvedConfig) -> Result<Box<dyn Session>> {
        let c = &config.config;
        let host = c
            .host
            .as_deref()
            .filter(|h| !h.is_empty())
            .unwrap_or(DEFAULT_HOST);
        let port = c.port.unwrap_or(DEFAULT_PORT);
        let target = diagnose::Target { host, port };

        let mut pg = tokio_postgres::Config::new();
        pg.host(host)
            .port(port)
            .user(
                c.user
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .unwrap_or(DEFAULT_USER),
            )
            .application_name("Quarry")
            .connect_timeout(CONNECT_TIMEOUT);
        if let Some(password) = &config.password {
            pg.password(password);
        }
        if let Some(database) = c.database.as_deref().filter(|d| !d.is_empty()) {
            pg.dbname(database);
        }
        let (mode, tls) = tls::connector(c.ssl).map_err(|err| diagnose::tls_setup(&err))?;
        pg.ssl_mode(mode);

        let (client, connection) = tokio::time::timeout(CONNECT_TIMEOUT, pg.connect(tls.clone()))
            .await
            .map_err(|_| diagnose::timed_out(&target, CONNECT_TIMEOUT))?
            .map_err(|err| diagnose::connect_error(&target, &err))?;

        // The connection runs apart; notices (`RAISE NOTICE`) come back through a channel.
        let (notice_tx, notices) = mpsc::unbounded_channel();
        let connection = tokio::spawn(async move {
            let mut connection = connection;
            let mut messages = futures_util::stream::poll_fn(move |cx| connection.poll_message(cx));
            while let Some(message) = messages.next().await {
                match message {
                    Ok(AsyncMessage::Notice(notice)) => {
                        let _ =
                            notice_tx.send(format!("{}: {}", notice.severity(), notice.message()));
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::debug!(%err, "postgres connection ended");
                        break;
                    }
                }
            }
        });

        let sql = "SHOW server_version";
        let version: String = client
            .query_one(sql, &[])
            .await
            .and_then(|row| row.try_get(0))
            .map_err(|err| diagnose::query_error(&err, sql))?;

        Ok(Box::new(PostgresSession {
            cancel: client.cancel_token(),
            client,
            tls,
            version,
            notices,
            sources: HashMap::new(),
            connection,
        }))
    }
}

struct PostgresSession {
    client: Client,
    cancel: CancelToken,
    tls: MakeRustlsConnect,
    version: String,
    notices: mpsc::UnboundedReceiver<String>,
    /// Tables that result columns come from, by oid.
    sources: HashMap<u32, introspect::Source>,
    connection: JoinHandle<()>,
}

impl PostgresSession {
    async fn prepare(&self, sql: &str) -> Result<Statement> {
        self.client
            .prepare(sql)
            .await
            .map_err(|err| diagnose::query_error(&err, sql))
    }

    /// Runs `sql` into `sink`: rows in batches up to `opts.row_limit`, then the set's end. A sink
    /// that paces the read (an export) makes it wait; one that says stop ends it at once.
    async fn run(
        &mut self,
        sql: &str,
        params: &[Value],
        opts: &ExecOpts,
        sink: &mut dyn RowSink,
    ) -> Result<()> {
        let started = Instant::now();
        let statement = match self.client.prepare(sql).await {
            Ok(statement) => statement,
            // A DO block or a script the client could not split: run it as is, without results.
            Err(err) if params.is_empty() && is_multi_statement(&err) => {
                self.client
                    .batch_execute(sql)
                    .await
                    .map_err(|err| diagnose::query_error(&err, sql))?;
                if self.notices_to(sink).await && sink.accept(set_done(0, 0, false)).await {
                    sink.accept(done(started)).await;
                }
                return Ok(());
            }
            Err(err) => return Err(diagnose::query_error(&err, sql)),
        };
        let params: Vec<TextParam> = params.iter().map(TextParam).collect();

        if statement.columns().is_empty() {
            let affected = self
                .client
                .execute_raw(&statement, params.iter())
                .await
                .map_err(|err| diagnose::query_error(&err, sql))?;
            // A statement without rows may rename or drop a table.
            self.sources.clear();
            if self.notices_to(sink).await && sink.accept(set_done(affected, 0, false)).await {
                sink.accept(done(started)).await;
            }
            return Ok(());
        }

        let columns = statement.columns();
        let metas = introspect::column_metas(&self.client, &mut self.sources, columns).await;
        if !sink.accept(ExecItem::Columns(metas)).await {
            return Ok(());
        }
        let types: Vec<_> = columns.iter().map(|c| c.type_().clone()).collect();
        let rows = self
            .client
            .query_raw(&statement, params.iter())
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?;
        futures_util::pin_mut!(rows);

        let limit = opts.row_limit.unwrap_or(u64::MAX);
        let batch_size = opts.batch_size.max(1);
        let mut batch = Vec::with_capacity(batch_size);
        let mut count: u64 = 0;
        let mut truncated = false;
        while let Some(row) = rows
            .try_next()
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?
        {
            if count >= limit {
                // Keep reading so the connection is free for the next statement.
                truncated = true;
                continue;
            }
            count += 1;
            batch.push(values_of(&row, &types));
            if batch.len() == batch_size {
                let full = std::mem::replace(&mut batch, Vec::with_capacity(batch_size));
                if !sink.accept(ExecItem::Rows(full)).await {
                    return Ok(());
                }
            }
        }
        if !batch.is_empty() && !sink.accept(ExecItem::Rows(batch)).await {
            return Ok(());
        }
        if self.notices_to(sink).await && sink.accept(set_done(0, count, truncated)).await {
            sink.accept(done(started)).await;
        }
        Ok(())
    }

    /// The server's notices so far; `false` once the sink stopped the read.
    async fn notices_to(&mut self, sink: &mut dyn RowSink) -> bool {
        while let Ok(notice) = self.notices.try_recv() {
            if !sink.accept(ExecItem::Notice(notice)).await {
                return false;
            }
        }
        true
    }

    async fn simple(&self, sql: &str) -> Result<()> {
        self.client
            .batch_execute(sql)
            .await
            .map_err(|err| diagnose::query_error(&err, sql))
    }
}

fn set_done(affected: u64, rows: u64, truncated: bool) -> ExecItem {
    ExecItem::SetDone {
        affected,
        rows,
        truncated,
    }
}

fn done(started: Instant) -> ExecItem {
    ExecItem::Done {
        elapsed: started.elapsed(),
    }
}

fn values_of(row: &Row, types: &[Type]) -> Vec<Value> {
    types
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let raw: Option<Raw> = row.try_get(i).unwrap_or(None);
            decode::value(ty, raw.map(|r| r.0))
        })
        .collect()
}

/// `cannot insert multiple commands into a prepared statement`.
fn is_multi_statement(err: &tokio_postgres::Error) -> bool {
    err.as_db_error()
        .is_some_and(|db| db.code().code() == "42601" && db.message().contains("multiple commands"))
}

#[async_trait]
impl Session for PostgresSession {
    fn server_version(&self) -> String {
        self.version.clone()
    }

    async fn execute(&mut self, sql: &str, opts: &ExecOpts, sink: ExecSink<'_>) -> Result<()> {
        self.run(sql, &[], opts, &mut FnSink(sink)).await
    }

    async fn query(
        &mut self,
        sql: &str,
        params: &[Value],
        opts: &ExecOpts,
        sink: ExecSink<'_>,
    ) -> Result<()> {
        self.run(sql, params, opts, &mut FnSink(sink)).await
    }

    async fn stream(
        &mut self,
        sql: &str,
        params: &[Value],
        opts: &ExecOpts,
        sink: &mut dyn RowSink,
    ) -> Result<()> {
        self.run(sql, params, opts, sink).await
    }

    async fn execute_params(&mut self, sql: &str, params: &[Value]) -> Result<u64> {
        let statement = self.prepare(sql).await?;
        let params: Vec<TextParam> = params.iter().map(TextParam).collect();
        self.client
            .execute_raw(&statement, params.iter())
            .await
            .map_err(|err| diagnose::query_error(&err, sql))
    }

    async fn list_databases(&mut self) -> Result<Vec<String>> {
        introspect::schemas(&self.client).await
    }

    async fn list_objects(&mut self, database: &str) -> Result<Vec<SchemaObject>> {
        introspect::objects(&self.client, database).await
    }

    async fn list_columns(&mut self, database: &str) -> Result<Vec<TableColumns>> {
        introspect::columns(&self.client, database).await
    }

    async fn table_info(&mut self, table: &TableRef) -> Result<TableInfo> {
        introspect::table_info(&self.client, table).await
    }

    async fn primary_key(&mut self, table: &TableRef) -> Result<Vec<String>> {
        introspect::primary_key(&self.client, table).await
    }

    async fn server_info(&mut self, kind: ServerInfoKind) -> Result<Vec<ServerInfoRow>> {
        introspect::server_info(&self.client, kind).await
    }

    async fn use_database(&mut self, database: &str) -> Result<()> {
        self.simple(&format!(
            "SET search_path TO {}",
            dialect().quote_ident(database)
        ))
        .await
    }

    async fn begin(&mut self) -> Result<()> {
        self.simple("BEGIN").await
    }

    async fn commit(&mut self) -> Result<()> {
        self.simple("COMMIT").await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.simple("ROLLBACK").await
    }

    async fn set_read_only(&mut self, read_only: bool) -> Result<()> {
        let mode = if read_only { "READ ONLY" } else { "READ WRITE" };
        self.simple(&format!(
            "SET SESSION CHARACTERISTICS AS TRANSACTION {mode}"
        ))
        .await
    }

    fn cancel_handle(&self) -> SharedCancel {
        std::sync::Arc::new(PostgresCancel {
            token: self.cancel.clone(),
            tls: self.tls.clone(),
        })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        let Self {
            client, connection, ..
        } = *self;
        drop(client);
        if tokio::time::timeout(Duration::from_secs(2), connection)
            .await
            .is_err()
        {
            tracing::debug!("postgres connection did not end in time");
        }
        Ok(())
    }
}

/// Cancels through PostgreSQL's cancel request, on a new connection to the same address (the
/// SSH tunnel's when there is one).
struct PostgresCancel {
    token: CancelToken,
    tls: MakeRustlsConnect,
}

impl CancelHandle for PostgresCancel {
    fn cancel(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("cancel called outside of a tokio runtime");
            return;
        };
        let token = self.token.clone();
        let tls = self.tls.clone();
        handle.spawn(async move {
            if let Err(err) = token.cancel_query(tls).await {
                tracing::warn!(%err, "cancel request failed");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_and_numbers_like_postgres() {
        let table = TableRef {
            database: Some("public".into()),
            schema: None,
            name: "or\"ders".into(),
        };
        assert_eq!(
            dialect().select_page(&table, Some("\"a\" = $1"), None, 10, 20),
            "SELECT * FROM \"public\".\"or\"\"ders\" WHERE \"a\" = $1 LIMIT 10 OFFSET 20"
        );
        assert_eq!(
            dialect().literal(&Value::Text("a\\b'c".into())),
            "'a\\b''c'"
        );
        assert_eq!(
            dialect().explain("SELECT 1", true).as_deref(),
            Some("EXPLAIN (FORMAT JSON) SELECT 1")
        );
    }
}
