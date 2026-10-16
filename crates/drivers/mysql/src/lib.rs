//! MySQL / MariaDB driver, built on `mysql_async` (TLS via rustls).

mod convert;

use std::time::{Duration, Instant};

use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, SslOpts};
use quarry_driver_api::{
    CancelHandle, Dialect, Driver, DriverError, ExecItem, ExecOpts, ExecSink, ResolvedConfig,
    Result, Session, SharedCancel,
};
use quarry_model::{Capabilities, DriverId, ObjectKind, SchemaObject, SslMode, TableRef, Value};

pub const DRIVER_ID: &str = "mysql";
pub const DEFAULT_PORT: u16 = 3306;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Default)]
pub struct MysqlDriver {
    dialect: MysqlDialect,
}

#[async_trait]
impl Driver for MysqlDriver {
    fn id(&self) -> DriverId {
        DriverId(DRIVER_ID.to_owned())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            schemas: false,
            transactions: true,
            editable_results: true,
            explain: true,
            cancel: true,
            ssh: true,
            ssl: true,
            object_kinds: vec![
                ObjectKind::Database,
                ObjectKind::Table,
                ObjectKind::View,
                ObjectKind::Function,
                ObjectKind::Procedure,
            ],
        }
    }

    fn dialect(&self) -> &dyn Dialect {
        &self.dialect
    }

    async fn connect(&self, config: &ResolvedConfig) -> Result<Box<dyn Session>> {
        let opts = build_opts(config);
        let mut conn = tokio::time::timeout(CONNECT_TIMEOUT, Conn::new(opts.clone()))
            .await
            .map_err(|_| DriverError::Connect("timed out".to_owned()))?
            .map_err(map_connect_err)?;
        let version: Option<String> = conn
            .query_first("SELECT VERSION()")
            .await
            .map_err(map_err)?;
        let session = MysqlSession {
            id: conn.id(),
            conn,
            opts,
            version: version.unwrap_or_default(),
        };
        Ok(Box::new(session))
    }
}

fn build_opts(config: &ResolvedConfig) -> Opts {
    let c = &config.config;
    let ssl = match c.ssl {
        SslMode::Disable | SslMode::Prefer => None,
        // Encrypt without verifying the certificate (like libpq's `sslmode=require`).
        SslMode::Require => Some(
            SslOpts::default()
                .with_danger_accept_invalid_certs(true)
                .with_danger_skip_domain_validation(true),
        ),
        SslMode::VerifyCa => Some(SslOpts::default().with_danger_skip_domain_validation(true)),
        SslMode::VerifyFull => Some(SslOpts::default()),
    };
    OptsBuilder::default()
        .ip_or_hostname(c.host.clone().unwrap_or_else(|| "127.0.0.1".to_owned()))
        .tcp_port(c.port.unwrap_or(DEFAULT_PORT))
        .user(c.user.clone())
        .pass(config.password.clone())
        .db_name(c.database.clone().filter(|d| !d.is_empty()))
        // The host's unix socket is not reachable from a Flatpak sandbox.
        .prefer_socket(false)
        .ssl_opts(ssl)
        .init(vec!["SET NAMES utf8mb4"])
        .into()
}

#[derive(Debug)]
struct MysqlSession {
    conn: Conn,
    id: u32,
    opts: Opts,
    version: String,
}

#[async_trait]
impl Session for MysqlSession {
    fn server_version(&self) -> String {
        self.version.clone()
    }

    async fn execute(&mut self, sql: &str, opts: &ExecOpts, sink: ExecSink<'_>) -> Result<()> {
        let started = Instant::now();
        let mut result = self.conn.query_iter(sql).await.map_err(map_err)?;
        let limit = opts.row_limit.unwrap_or(u64::MAX);
        let batch_size = opts.batch_size.max(1);

        // One iteration per result set; `next()` moves to the next set when the current one ends.
        while let Some(columns) = result.columns() {
            if columns.is_empty() {
                let affected = result.affected_rows();
                result.next().await.map_err(map_err)?;
                sink(ExecItem::SetDone {
                    affected,
                    rows: 0,
                    truncated: false,
                });
                continue;
            }

            sink(ExecItem::Columns(
                columns.iter().map(convert::column_meta).collect(),
            ));
            let mut batch = Vec::with_capacity(batch_size);
            let mut rows: u64 = 0;
            let mut truncated = false;
            while let Some(row) = result.next().await.map_err(map_err)? {
                if rows >= limit {
                    // Keep draining so the connection is freed, but drop the data.
                    truncated = true;
                    continue;
                }
                rows += 1;
                let values = row.unwrap();
                batch.push(
                    values
                        .into_iter()
                        .zip(columns.iter())
                        .map(|(v, c)| convert::value(v, c))
                        .collect(),
                );
                if batch.len() == batch_size {
                    sink(ExecItem::Rows(std::mem::replace(
                        &mut batch,
                        Vec::with_capacity(batch_size),
                    )));
                }
            }
            if !batch.is_empty() {
                sink(ExecItem::Rows(batch));
            }
            sink(ExecItem::SetDone {
                affected: 0,
                rows,
                truncated,
            });
        }

        sink(ExecItem::Done {
            elapsed: started.elapsed(),
        });
        Ok(())
    }

    async fn execute_params(&mut self, sql: &str, params: &[Value]) -> Result<u64> {
        let params: Vec<mysql_async::Value> = params.iter().map(convert::to_mysql).collect();
        self.conn.exec_drop(sql, params).await.map_err(map_err)?;
        Ok(self.conn.affected_rows())
    }

    async fn list_databases(&mut self) -> Result<Vec<String>> {
        self.conn.query("SHOW DATABASES").await.map_err(map_err)
    }

    async fn list_objects(&mut self, database: &str) -> Result<Vec<SchemaObject>> {
        let tables: Vec<(String, String)> = self
            .conn
            .exec(
                "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
                (database,),
            )
            .await
            .map_err(map_err)?;
        let routines: Vec<(String, String)> = self
            .conn
            .exec(
                "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES \
                 WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME",
                (database,),
            )
            .await
            .map_err(map_err)?;

        let objects = tables
            .into_iter()
            .map(|(name, kind)| SchemaObject {
                name,
                kind: if kind.contains("VIEW") {
                    ObjectKind::View
                } else {
                    ObjectKind::Table
                },
            })
            .chain(routines.into_iter().map(|(name, kind)| SchemaObject {
                name,
                kind: if kind == "FUNCTION" {
                    ObjectKind::Function
                } else {
                    ObjectKind::Procedure
                },
            }))
            .collect();
        Ok(objects)
    }

    async fn use_database(&mut self, database: &str) -> Result<()> {
        let sql = format!("USE {}", MysqlDialect.quote_ident(database));
        self.conn.query_drop(sql).await.map_err(map_err)
    }

    async fn begin(&mut self) -> Result<()> {
        self.conn
            .query_drop("START TRANSACTION")
            .await
            .map_err(map_err)
    }

    async fn commit(&mut self) -> Result<()> {
        self.conn.query_drop("COMMIT").await.map_err(map_err)
    }

    async fn rollback(&mut self) -> Result<()> {
        self.conn.query_drop("ROLLBACK").await.map_err(map_err)
    }

    async fn set_read_only(&mut self, read_only: bool) -> Result<()> {
        let mode = if read_only { "READ ONLY" } else { "READ WRITE" };
        self.conn
            .query_drop(format!("SET SESSION TRANSACTION {mode}"))
            .await
            .map_err(map_err)
    }

    fn cancel_handle(&self) -> SharedCancel {
        std::sync::Arc::new(MysqlCancel {
            opts: self.opts.clone(),
            id: self.id,
        })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.conn.disconnect().await.map_err(map_err)
    }
}

/// Cancels via `KILL QUERY <id>` on a second connection.
#[derive(Debug)]
struct MysqlCancel {
    opts: Opts,
    id: u32,
}

impl CancelHandle for MysqlCancel {
    fn cancel(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("cancel called outside of a tokio runtime");
            return;
        };
        let opts = self.opts.clone();
        let id = self.id;
        handle.spawn(async move {
            match Conn::new(opts).await {
                Ok(mut conn) => {
                    if let Err(err) = conn.query_drop(format!("KILL QUERY {id}")).await {
                        tracing::warn!(%err, "KILL QUERY failed");
                    }
                    let _ = conn.disconnect().await;
                }
                Err(err) => tracing::warn!(%err, "cancel connection failed"),
            }
        });
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MysqlDialect;

impl Dialect for MysqlDialect {
    fn quote_ident(&self, ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }

    fn placeholder(&self, _index: usize) -> String {
        "?".to_owned()
    }

    fn select_page(&self, table: &TableRef, limit: u64, offset: u64) -> String {
        let name = match &table.database {
            Some(db) => format!("{}.{}", self.quote_ident(db), self.quote_ident(&table.name)),
            None => self.quote_ident(&table.name),
        };
        if offset == 0 {
            format!("SELECT * FROM {name} LIMIT {limit}")
        } else {
            format!("SELECT * FROM {name} LIMIT {limit} OFFSET {offset}")
        }
    }
}

fn map_connect_err(err: mysql_async::Error) -> DriverError {
    match err {
        mysql_async::Error::Server(ref e) if e.code == 1045 => DriverError::Auth,
        mysql_async::Error::Server(e) => DriverError::Connect(e.message),
        mysql_async::Error::Io(e) => DriverError::Connect(e.to_string()),
        other => DriverError::Connect(other.to_string()),
    }
}

fn map_err(err: mysql_async::Error) -> DriverError {
    match err {
        // 1317: query interrupted by KILL QUERY
        mysql_async::Error::Server(e) if e.code == 1317 => DriverError::Cancelled,
        mysql_async::Error::Server(e) => DriverError::Query {
            message: format!("{} (MySQL {}, SQLSTATE {})", e.message, e.code, e.state),
            position: None,
        },
        other => DriverError::Other(Box::new(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers() {
        assert_eq!(MysqlDialect.quote_ident("a`b"), "`a``b`");
        let table = TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        };
        assert_eq!(
            MysqlDialect.select_page(&table, 100, 0),
            "SELECT * FROM `shop`.`orders` LIMIT 100"
        );
        assert_eq!(
            MysqlDialect.select_page(&table, 100, 200),
            "SELECT * FROM `shop`.`orders` LIMIT 100 OFFSET 200"
        );
    }
}
