//! MySQL / MariaDB driver, built on `mysql_async` (TLS via rustls).

mod convert;
mod ddl;
mod diagnose;
mod introspect;

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, QueryResult, SslOpts};
use quarry_driver_api::{
    CancelHandle, Dialect, Driver, DriverError, ExecItem, ExecOpts, ExecSink, FnSink,
    ResolvedConfig, Result, RowSink, Session, SharedCancel, SpecDialect,
};
use quarry_model::{
    Capabilities, DialectSpec, DriverId, ErrorLocation, ExplainSyntax, IdentQuote, LiteralStyle,
    ObjectKind, PagingStyle, PlaceholderStyle, SchemaChange, SchemaObject, ServerInfoKind,
    ServerInfoRow, SslMode, TableColumns, TableInfo, TableRef, Value,
};

pub const DRIVER_ID: &str = "mysql";
pub const MARIADB_DRIVER_ID: &str = "mariadb";
pub const DEFAULT_PORT: u16 = 3306;
const DEFAULT_HOST: &str = "127.0.0.1";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Serves MySQL and, under its own id, MariaDB: the wire protocol is shared today, and the
/// separate id lets MariaDB grow its own behavior without touching MySQL connections.
#[derive(Debug)]
pub struct MysqlDriver {
    id: &'static str,
}

impl Default for MysqlDriver {
    fn default() -> Self {
        Self { id: DRIVER_ID }
    }
}

impl MysqlDriver {
    pub fn mariadb() -> Self {
        Self {
            id: MARIADB_DRIVER_ID,
        }
    }
}

#[async_trait]
impl Driver for MysqlDriver {
    fn id(&self) -> DriverId {
        DriverId(self.id.to_owned())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            schemas: false,
            transactions: true,
            editable_results: true,
            explain: true,
            schema_edit: true,
            server_info: true,
            cancel: true,
            ssh: true,
            ssl: true,
            object_kinds: vec![
                ObjectKind::Database,
                ObjectKind::Table,
                ObjectKind::View,
                ObjectKind::Function,
                ObjectKind::Procedure,
                ObjectKind::Trigger,
                ObjectKind::Event,
            ],
        }
    }

    fn dialect(&self) -> &SpecDialect {
        dialect()
    }

    async fn schema_change(
        &self,
        change: &SchemaChange,
    ) -> Option<std::result::Result<Vec<String>, String>> {
        Some(ddl::statements(dialect(), change))
    }

    async fn connect(&self, config: &ResolvedConfig) -> Result<Box<dyn Session>> {
        let opts = build_opts(config);
        let c = &config.config;
        let target = diagnose::Target {
            host: c.host.as_deref().unwrap_or(DEFAULT_HOST),
            port: c.port.unwrap_or(DEFAULT_PORT),
        };
        let mut conn = tokio::time::timeout(CONNECT_TIMEOUT, Conn::new(opts.clone()))
            .await
            .map_err(|_| diagnose::timed_out(&target, CONNECT_TIMEOUT))?
            .map_err(|err| diagnose::connect_error(&target, err))?;
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
        .ip_or_hostname(c.host.clone().unwrap_or_else(|| DEFAULT_HOST.to_owned()))
        .tcp_port(c.port.unwrap_or(DEFAULT_PORT))
        .user(c.user.clone())
        .pass(config.password.clone())
        .db_name(c.database.clone().filter(|d| !d.is_empty()))
        // The host's unix socket is not reachable from a Flatpak sandbox.
        .prefer_socket(false)
        // UPDATE reports matched rows, not changed rows, so a grid edit that keeps the old
        // value still counts as one row (the edit check needs exactly one).
        .client_found_rows(true)
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

impl MysqlSession {
    /// One column of names for `database`, or none when the query fails (missing privilege,
    /// older server); the failure is logged.
    async fn names(&mut self, sql: &str, database: &str) -> Vec<String> {
        match self.conn.exec(sql, (database,)).await {
            Ok(names) => names,
            Err(err) => {
                tracing::debug!(%err, "optional object list");
                Vec::new()
            }
        }
    }
}

#[async_trait]
impl Session for MysqlSession {
    fn server_version(&self) -> String {
        self.version.clone()
    }

    async fn execute(&mut self, sql: &str, opts: &ExecOpts, sink: ExecSink<'_>) -> Result<()> {
        let started = Instant::now();
        let result = self.conn.query_iter(sql).await.map_err(map_err)?;
        let mut sink = FnSink(sink);
        stream(result, opts, &mut sink).await?;
        (sink.0)(ExecItem::Done {
            elapsed: started.elapsed(),
        });
        Ok(())
    }

    async fn query(
        &mut self,
        sql: &str,
        params: &[Value],
        opts: &ExecOpts,
        sink: ExecSink<'_>,
    ) -> Result<()> {
        let started = Instant::now();
        let params: Vec<mysql_async::Value> = params.iter().map(convert::to_mysql).collect();
        let result = self.conn.exec_iter(sql, params).await.map_err(map_err)?;
        let mut sink = FnSink(sink);
        stream(result, opts, &mut sink).await?;
        (sink.0)(ExecItem::Done {
            elapsed: started.elapsed(),
        });
        Ok(())
    }

    async fn stream(
        &mut self,
        sql: &str,
        params: &[Value],
        opts: &ExecOpts,
        sink: &mut dyn RowSink,
    ) -> Result<()> {
        let started = Instant::now();
        if params.is_empty() {
            let result = self.conn.query_iter(sql).await.map_err(map_err)?;
            stream(result, opts, sink).await?;
        } else {
            let params: Vec<mysql_async::Value> = params.iter().map(convert::to_mysql).collect();
            let result = self.conn.exec_iter(sql, params).await.map_err(map_err)?;
            stream(result, opts, sink).await?;
        }
        sink.accept(ExecItem::Done {
            elapsed: started.elapsed(),
        })
        .await;
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

    async fn server_info(&mut self, kind: ServerInfoKind) -> Result<Vec<ServerInfoRow>> {
        let sql = match kind {
            ServerInfoKind::Variables => "SHOW GLOBAL VARIABLES",
            ServerInfoKind::Status => "SHOW GLOBAL STATUS",
        };
        let rows: Vec<(String, String)> = self.conn.query(sql).await.map_err(map_err)?;
        Ok(rows
            .into_iter()
            .map(|(name, value)| ServerInfoRow { name, value })
            .collect())
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

        // Triggers and events are listed when the user may see them; a server or account that
        // refuses the query just shows none, rather than failing the whole tree.
        let triggers = self
            .names(
                "SELECT TRIGGER_NAME FROM information_schema.TRIGGERS \
                 WHERE TRIGGER_SCHEMA = ? ORDER BY TRIGGER_NAME",
                database,
            )
            .await;
        let events = self
            .names(
                "SELECT EVENT_NAME FROM information_schema.EVENTS \
                 WHERE EVENT_SCHEMA = ? ORDER BY EVENT_NAME",
                database,
            )
            .await;

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
            .chain(triggers.into_iter().map(|name| SchemaObject {
                name,
                kind: ObjectKind::Trigger,
            }))
            .chain(events.into_iter().map(|name| SchemaObject {
                name,
                kind: ObjectKind::Event,
            }))
            .collect();
        Ok(objects)
    }

    async fn list_columns(&mut self, database: &str) -> Result<Vec<TableColumns>> {
        introspect::columns(&mut self.conn, database).await
    }

    async fn table_info(&mut self, table: &TableRef) -> Result<TableInfo> {
        introspect::table_info(&mut self.conn, table, self.version.contains("MariaDB")).await
    }

    async fn primary_key(&mut self, table: &TableRef) -> Result<Vec<String>> {
        introspect::primary_key(&mut self.conn, table).await
    }

    async fn use_database(&mut self, database: &str) -> Result<()> {
        let sql = format!("USE {}", dialect().quote_ident(database));
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

    async fn set_strict(&mut self) -> Result<()> {
        // Added to the session's own mode, so the rest of what the server configured stays.
        self.conn
            .query_drop(
                "SET SESSION sql_mode = CONCAT_WS(',', @@SESSION.sql_mode, \
                 'STRICT_ALL_TABLES')",
            )
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

/// Send every result set of `result` to `sink`, keeping at most `opts.row_limit` rows per set.
/// Stops as soon as the sink says so.
async fn stream<P: mysql_async::prelude::Protocol>(
    mut result: QueryResult<'_, '_, P>,
    opts: &ExecOpts,
    sink: &mut dyn RowSink,
) -> Result<()> {
    let limit = opts.row_limit.unwrap_or(u64::MAX);
    let batch_size = opts.batch_size.max(1);

    // One iteration per result set; `next()` moves to the next set when the current one ends.
    loop {
        let Some(columns) = result.columns() else {
            // A later statement of a script that failed is kept as the pending result, which
            // `columns()` reads as "no more sets". `next()` hands its error over and clears it;
            // left there, it would fail the session's next query instead.
            result.next().await.map_err(map_err)?;
            break;
        };
        if columns.is_empty() {
            let affected = result.affected_rows();
            result.next().await.map_err(map_err)?;
            let item = ExecItem::SetDone {
                affected,
                rows: 0,
                truncated: false,
            };
            if !sink.accept(item).await {
                return Ok(());
            }
            continue;
        }

        let item = ExecItem::Columns(columns.iter().map(convert::column_meta).collect());
        if !sink.accept(item).await {
            return Ok(());
        }
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
                let full = std::mem::replace(&mut batch, Vec::with_capacity(batch_size));
                if !sink.accept(ExecItem::Rows(full)).await {
                    return Ok(());
                }
            }
        }
        if !batch.is_empty() && !sink.accept(ExecItem::Rows(batch)).await {
            return Ok(());
        }
        let item = ExecItem::SetDone {
            affected: 0,
            rows,
            truncated,
        };
        if !sink.accept(item).await {
            return Ok(());
        }
    }
    Ok(())
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

/// MySQL syntax. MariaDB shares it.
pub(crate) fn dialect() -> &'static SpecDialect {
    static DIALECT: LazyLock<SpecDialect> = LazyLock::new(|| {
        SpecDialect::new(DialectSpec {
            quote: IdentQuote::Backtick,
            on_duplicate: quarry_model::DuplicateSyntax::OnDuplicateKey,
            paging: PagingStyle::LimitOffset,
            placeholder: PlaceholderStyle::Question,
            // MySQL reads `\` as an escape inside strings (unless NO_BACKSLASH_ESCAPES).
            literals: LiteralStyle {
                backslash_escapes: true,
                bool_as_int: true,
                national_strings: false,
                hex_bytes: false,
            },
            // MySQL 5.6+ and MariaDB 10.1+ both accept FORMAT=JSON.
            explain: Some(ExplainSyntax {
                table: "EXPLAIN".to_owned(),
                json: Some("EXPLAIN FORMAT=JSON".to_owned()),
            }),
        })
    });
    &DIALECT
}

fn map_err(err: mysql_async::Error) -> DriverError {
    use mysql_async::DriverError as Native;
    match err {
        // 1317: query interrupted by KILL QUERY
        mysql_async::Error::Server(e) if e.code == 1317 => DriverError::Cancelled,
        // 1053: server shutdown, 1152: connection aborted, 1927: connection killed,
        // 4031: closed for inactivity (MySQL 8.0.24+).
        mysql_async::Error::Server(e) if matches!(e.code, 1053 | 1152 | 1927 | 4031) => {
            DriverError::ConnectionLost(format!(
                "The server closed the connection: {} (MySQL {}).",
                e.message, e.code
            ))
        }
        mysql_async::Error::Server(e) => DriverError::Query {
            location: syntax_location(&e.message),
            message: format!("{} (MySQL {}, SQLSTATE {})", e.message, e.code, e.state),
        },
        mysql_async::Error::Io(e) => {
            DriverError::ConnectionLost(format!("The connection to the server was lost: {e}."))
        }
        mysql_async::Error::Driver(
            e @ (Native::ConnectionClosed
            | Native::PacketOutOfOrder
            | Native::UnexpectedPacket { .. }),
        ) => DriverError::ConnectionLost(format!("The connection to the server was lost: {e}")),
        other => DriverError::Other(Box::new(other)),
    }
}

/// The spot a MySQL / MariaDB syntax error points at: `… near 'SELEC 2' at line 1`.
fn syntax_location(message: &str) -> Option<ErrorLocation> {
    const NEAR: &str = "near '";
    const AT_LINE: &str = "' at line ";
    let tail = message.rfind(AT_LINE)?;
    let line = message[tail + AT_LINE.len()..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    let start = message[..tail].find(NEAR)? + NEAR.len();
    Some(ErrorLocation {
        line,
        near: message[start..tail].to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_syntax_error_location() {
        let at = |m| syntax_location(m).map(|l| (l.line, l.near));
        assert_eq!(
            at(
                "You have an error in your SQL syntax; check the manual that corresponds to your \
                MySQL server version for the right syntax to use near 'SELEC 2' at line 1"
            ),
            Some((1, "SELEC 2".to_owned()))
        );
        assert_eq!(
            at("… for the right syntax to use near 'it's' at line 3' at line 12"),
            Some((12, "it's' at line 3".to_owned()))
        );
        assert_eq!(at("… to use near '' at line 2"), Some((2, String::new())));
        assert_eq!(at("Table 'shop.nope' doesn't exist"), None);
    }

    #[test]
    fn quotes_identifiers() {
        assert_eq!(dialect().quote_ident("a`b"), "`a``b`");
        let table = TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        };
        assert_eq!(
            dialect().select_page(&table, None, None, 100, 0),
            "SELECT * FROM `shop`.`orders` LIMIT 100"
        );
        assert_eq!(
            dialect().select_page(&table, Some("`a` = ?"), Some("`b` DESC"), 100, 200),
            "SELECT * FROM `shop`.`orders` WHERE `a` = ? ORDER BY `b` DESC LIMIT 100 OFFSET 200"
        );
        assert_eq!(
            dialect().count_rows(&table, Some("`a` IS NULL")),
            "SELECT COUNT(*) FROM `shop`.`orders` WHERE `a` IS NULL"
        );
    }

    #[test]
    fn literals_escape_backslashes() {
        assert_eq!(
            dialect().literal(&Value::Text("a\\b'c".into())),
            "'a\\\\b''c'"
        );
        assert_eq!(dialect().literal(&Value::Bool(true)), "1");
        assert_eq!(
            dialect().literal(&Value::UInt(u64::MAX)),
            "18446744073709551615"
        );
    }

    #[test]
    fn explains_as_a_table_or_json() {
        assert_eq!(
            dialect().explain("SELECT 1", false).as_deref(),
            Some("EXPLAIN SELECT 1")
        );
        assert_eq!(
            dialect().explain("SELECT 1", true).as_deref(),
            Some("EXPLAIN FORMAT=JSON SELECT 1")
        );
    }
}
