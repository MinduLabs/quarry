//! Microsoft SQL Server driver, built on `tiberius` (TLS via rustls).
//!
//! The schema tree's databases are the schemas of the database the connection opens (the
//! connection's Default database): a table is `[schema].[table]`.

mod decode;
mod diagnose;
mod introspect;

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::TryStreamExt;
use quarry_driver_api::{
    CancelHandle, Driver, DriverError, ExecItem, ExecOpts, ExecSink, FnSink, ResolvedConfig,
    Result, RowSink, Session, SharedCancel, SpecDialect,
};
use quarry_model::{
    Capabilities, ColumnMeta, DialectSpec, DriverId, IdentQuote, LiteralStyle, ObjectKind,
    PagingStyle, PlaceholderStyle, SchemaObject, ServerInfoKind, ServerInfoRow, SslMode,
    TableColumns, TableInfo, TableRef, Value,
};
use tiberius::{AuthMethod, Client, Config, EncryptionLevel, QueryItem, ToSql};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::decode::Param;

pub const DRIVER_ID: &str = "sqlserver";
pub const DEFAULT_PORT: u16 = 1433;
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_USER: &str = "sa";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the server has to acknowledge a cancel before the connection counts as lost.
const CANCEL_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) type Conn = Client<Compat<TcpStream>>;

/// SQL Server syntax.
pub(crate) fn dialect() -> &'static SpecDialect {
    static DIALECT: LazyLock<SpecDialect> = LazyLock::new(|| {
        SpecDialect::new(DialectSpec {
            quote: IdentQuote::Brackets,
            placeholder: PlaceholderStyle::AtP,
            paging: PagingStyle::OffsetFetch,
            literals: LiteralStyle {
                backslash_escapes: false,
                bool_as_int: true,
                national_strings: true,
                hex_bytes: true,
            },
            // A plan needs SET SHOWPLAN_XML around the statement, not keywords before it.
            explain: None,
        })
    });
    &DIALECT
}

#[derive(Debug, Default)]
pub struct SqlServerDriver;

#[async_trait]
impl Driver for SqlServerDriver {
    fn id(&self) -> DriverId {
        DriverId(DRIVER_ID.to_owned())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // Schemas are the tree's databases, so there is no level below them.
            schemas: false,
            transactions: true,
            editable_results: true,
            explain: false,
            schema_edit: false,
            server_info: true,
            cancel: true,
            ssh: true,
            ssl: true,
            object_kinds: vec![
                ObjectKind::Database,
                ObjectKind::Table,
                ObjectKind::View,
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

        let mut tds = Config::new();
        tds.host(host);
        tds.port(port);
        tds.application_name("Quarry");
        tds.authentication(AuthMethod::sql_server(
            c.user
                .as_deref()
                .filter(|u| !u.is_empty())
                .unwrap_or(DEFAULT_USER),
            config.password.as_deref().unwrap_or_default(),
        ));
        if let Some(database) = c.database.as_deref().filter(|d| !d.is_empty()) {
            tds.database(database);
        }
        // Like libpq's sslmode: prefer and require encrypt without checking the certificate.
        // tiberius cannot check the chain without the name, so verify-ca checks both.
        match c.ssl {
            SslMode::Disable => tds.encryption(EncryptionLevel::NotSupported),
            SslMode::Prefer => {
                tds.encryption(EncryptionLevel::On);
                tds.trust_cert();
            }
            SslMode::Require => {
                tds.encryption(EncryptionLevel::Required);
                tds.trust_cert();
            }
            SslMode::VerifyCa | SslMode::VerifyFull => {
                tds.encryption(EncryptionLevel::Required);
            }
        }

        let mut client = match login(&tds, host, port).await {
            // An Azure gateway or an availability group listener sends the client on, once.
            Err(LoginError::Routed { host, port }) => {
                tds.host(&host);
                tds.port(port);
                login(&tds, &host, port)
                    .await
                    .map_err(LoginError::into_error)?
            }
            Ok(client) => client,
            Err(other) => return Err(other.into_error()),
        };

        let sql = "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128))";
        let version = client
            .simple_query(sql)
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?
            .into_row()
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?
            .and_then(|row| row.get::<&str, _>(0).map(str::to_owned))
            .unwrap_or_default();

        let (cancel, cancelled) = watch::channel(0u64);
        Ok(Box::new(SqlServerSession {
            client,
            version,
            cancel,
            cancelled,
        }))
    }
}

/// A failed login, or the server's request to log in elsewhere.
enum LoginError {
    Failed(DriverError),
    Routed { host: String, port: u16 },
}

impl LoginError {
    fn into_error(self) -> DriverError {
        match self {
            Self::Failed(err) => err,
            Self::Routed { host, port } => DriverError::Connect(format!(
                "The server sent the connection on to {host}:{port} more than once.\nQuarry \
                 follows one redirect. Connect to the server it names directly.\nCode: routing"
            )),
        }
    }
}

async fn login(tds: &Config, host: &str, port: u16) -> std::result::Result<Conn, LoginError> {
    let target = diagnose::Target { host, port };
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(tds.get_addr()))
        .await
        .map_err(|_| LoginError::Failed(diagnose::timed_out(&target, CONNECT_TIMEOUT)))?
        .map_err(|err| LoginError::Failed(diagnose::io_error(&target, &err)))?;
    // Small requests (a cancel, a short query) should not wait for more bytes.
    let _ = tcp.set_nodelay(true);
    match tokio::time::timeout(
        CONNECT_TIMEOUT,
        Client::connect(tds.clone(), tcp.compat_write()),
    )
    .await
    {
        Err(_) => Err(LoginError::Failed(diagnose::timed_out(
            &target,
            CONNECT_TIMEOUT,
        ))),
        Ok(Ok(client)) => Ok(client),
        Ok(Err(tiberius::error::Error::Routing { host, port })) => {
            Err(LoginError::Routed { host, port })
        }
        Ok(Err(err)) => Err(LoginError::Failed(diagnose::connect_error(&target, &err))),
    }
}

struct SqlServerSession {
    client: Conn,
    version: String,
    /// Bumped by the cancel handle; a running statement watches it.
    cancel: watch::Sender<u64>,
    cancelled: watch::Receiver<u64>,
}

/// The result sets of one run, as they arrive.
struct Sets {
    limit: u64,
    batch_size: usize,
    open: Option<Vec<tiberius::ColumnType>>,
    batch: Vec<Vec<Value>>,
    rows: u64,
    truncated: bool,
    any: bool,
}

/// Whether the read goes on after an item: `false` once the sink stopped it.
type GoOn = bool;

impl Sets {
    async fn start(&mut self, columns: &[tiberius::Column], sink: &mut dyn RowSink) -> GoOn {
        if !self.finish(sink).await {
            return false;
        }
        self.any = true;
        self.open = Some(columns.iter().map(tiberius::Column::column_type).collect());
        sink.accept(ExecItem::Columns(
            columns
                .iter()
                .map(|c| ColumnMeta {
                    name: c.name().to_owned(),
                    type_name: decode::type_name(c.column_type()).to_owned(),
                    nullable: None,
                    source_database: None,
                    source_table: None,
                    source_column: None,
                })
                .collect(),
        ))
        .await
    }

    async fn row(&mut self, row: tiberius::Row, sink: &mut dyn RowSink) -> GoOn {
        let Some(types) = &self.open else {
            return true;
        };
        if self.rows >= self.limit {
            // Keep reading so the connection is free for the next statement.
            self.truncated = true;
            return true;
        }
        self.rows += 1;
        self.batch.push(
            row.into_iter()
                .zip(types.iter())
                .map(|(data, ty)| decode::value(data, *ty))
                .collect(),
        );
        if self.batch.len() < self.batch_size {
            return true;
        }
        let full = std::mem::replace(&mut self.batch, Vec::with_capacity(self.batch_size));
        sink.accept(ExecItem::Rows(full)).await
    }

    async fn finish(&mut self, sink: &mut dyn RowSink) -> GoOn {
        if self.open.take().is_none() {
            return true;
        }
        if !self.batch.is_empty()
            && !sink
                .accept(ExecItem::Rows(std::mem::take(&mut self.batch)))
                .await
        {
            return false;
        }
        let item = ExecItem::SetDone {
            affected: 0,
            rows: self.rows,
            truncated: self.truncated,
        };
        self.rows = 0;
        self.truncated = false;
        sink.accept(item).await
    }
}

/// How a read ended.
enum End {
    Finished,
    Cancelled,
    /// The sink wanted no more.
    Stopped,
}

impl SqlServerSession {
    /// Runs `sql` into `sink`, which may pace the read (an export) or stop it.
    async fn run(
        &mut self,
        sql: &str,
        params: &[Value],
        opts: &ExecOpts,
        sink: &mut dyn RowSink,
    ) -> Result<()> {
        let started = Instant::now();
        let params: Vec<Param> = params.iter().map(Param::new).collect();
        let refs: Vec<&dyn ToSql> = params.iter().map(|p| p as &dyn ToSql).collect();
        let mut sets = Sets {
            limit: opts.row_limit.unwrap_or(u64::MAX),
            batch_size: opts.batch_size.max(1),
            open: None,
            batch: Vec::new(),
            rows: 0,
            truncated: false,
            any: false,
        };
        let mut cancelled = self.cancelled.clone();
        cancelled.borrow_and_update();

        let end = 'run: {
            // tiberius waits for the server's first answer before it hands over the stream, so
            // a statement that runs long before its first row is cancelled here.
            let start = async {
                if refs.is_empty() {
                    self.client.simple_query(sql).await
                } else {
                    self.client.query(sql, &refs).await
                }
            };
            let mut stream = tokio::select! {
                biased;
                Ok(()) = cancelled.changed() => break 'run End::Cancelled,
                stream = start => stream.map_err(|err| diagnose::query_error(&err, sql))?,
            };
            loop {
                let item = tokio::select! {
                    biased;
                    // The sender lives in this session, so `changed` only fails once it is gone.
                    Ok(()) = cancelled.changed() => break End::Cancelled,
                    item = stream.try_next() => item.map_err(|err| diagnose::query_error(&err, sql))?,
                };
                let go_on = match item {
                    Some(QueryItem::Metadata(meta)) => sets.start(meta.columns(), sink).await,
                    Some(QueryItem::Row(row)) => sets.row(row, sink).await,
                    None => break End::Finished,
                };
                if !go_on {
                    break End::Stopped;
                }
            }
        };
        match end {
            End::Finished => {}
            // The rest of the answer is still on the wire; the caller cancels and closes.
            End::Stopped => return Ok(()),
            End::Cancelled => return self.cancelled_query().await,
        }
        if !sets.finish(sink).await {
            return Ok(());
        }
        if !sets.any {
            // The stream keeps the server's row counts to itself; @@ROWCOUNT still has the last
            // statement's in the next batch.
            let affected = self.rowcount().await?;
            let item = ExecItem::SetDone {
                affected,
                rows: 0,
                truncated: false,
            };
            if !sink.accept(item).await {
                return Ok(());
            }
        }
        sink.accept(ExecItem::Done {
            elapsed: started.elapsed(),
        })
        .await;
        Ok(())
    }

    async fn cancelled_query(&mut self) -> Result<()> {
        // An attention signal: the server stops the batch and the connection stays usable.
        // tiberius 0.13 reads only up to the end of the current message, and SQL Server
        // confirms an attention in the next one, so the confirmation is never read
        // (tiberius#459). The server has stopped, but the connection is out of step: it is
        // given up, and the tab opens a new one.
        match tokio::time::timeout(CANCEL_TIMEOUT, self.client.cancel_query()).await {
            Ok(Ok(())) => Err(DriverError::Cancelled),
            Ok(Err(_)) | Err(_) => Err(DriverError::ConnectionLost(
                "Query cancelled. The connection was closed to stop it; the next query opens a \
                 new one."
                    .to_owned(),
            )),
        }
    }

    async fn rowcount(&mut self) -> Result<u64> {
        let sql = "SELECT CAST(@@ROWCOUNT AS bigint)";
        let row = self
            .client
            .simple_query(sql)
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?
            .into_row()
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?;
        Ok(row
            .and_then(|row| row.get::<i64, _>(0))
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0))
    }

    /// Runs `sql` and reads its answer to the end.
    async fn batch(&mut self, sql: &str) -> Result<()> {
        self.client
            .simple_query(sql)
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?
            .into_results()
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?;
        Ok(())
    }
}

#[async_trait]
impl Session for SqlServerSession {
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
        let params: Vec<Param> = params.iter().map(Param::new).collect();
        let refs: Vec<&dyn ToSql> = params.iter().map(|p| p as &dyn ToSql).collect();
        let result = self
            .client
            .execute(sql, &refs)
            .await
            .map_err(|err| diagnose::query_error(&err, sql))?;
        Ok(result.total())
    }

    async fn list_databases(&mut self) -> Result<Vec<String>> {
        introspect::schemas(&mut self.client).await
    }

    async fn list_objects(&mut self, database: &str) -> Result<Vec<SchemaObject>> {
        introspect::objects(&mut self.client, database).await
    }

    async fn list_columns(&mut self, database: &str) -> Result<Vec<TableColumns>> {
        introspect::columns(&mut self.client, database).await
    }

    async fn table_info(&mut self, table: &TableRef) -> Result<TableInfo> {
        introspect::table_info(&mut self.client, table).await
    }

    async fn primary_key(&mut self, table: &TableRef) -> Result<Vec<String>> {
        introspect::primary_key(&mut self.client, table).await
    }

    async fn server_info(&mut self, kind: ServerInfoKind) -> Result<Vec<ServerInfoRow>> {
        introspect::server_info(&mut self.client, kind).await
    }

    /// Names resolve through the login's default schema, which a session cannot change, so
    /// choosing a schema in a tab changes nothing on the server.
    async fn use_database(&mut self, _database: &str) -> Result<()> {
        Ok(())
    }

    async fn begin(&mut self) -> Result<()> {
        self.batch("BEGIN TRANSACTION").await
    }

    async fn commit(&mut self) -> Result<()> {
        self.batch("COMMIT TRANSACTION").await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.batch("IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION").await
    }

    /// SQL Server has no read-only session. The engine still refuses grid and schema edits on a
    /// read-only connection; only a login without write permissions stops typed SQL.
    async fn set_read_only(&mut self, read_only: bool) -> Result<()> {
        if read_only {
            tracing::info!("SQL Server sessions cannot be read-only; the login's rights decide");
        }
        Ok(())
    }

    fn cancel_handle(&self) -> SharedCancel {
        std::sync::Arc::new(SqlServerCancel(self.cancel.clone()))
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.client
            .close()
            .await
            .map_err(|err| DriverError::Other(err.to_string().into()))
    }
}

/// Asks the running statement to stop; the session sends the attention signal itself, since the
/// client is busy with the statement's results.
struct SqlServerCancel(watch::Sender<u64>);

impl CancelHandle for SqlServerCancel {
    fn cancel(&self) {
        self.0.send_modify(|n| *n += 1);
    }
}

#[cfg(test)]
mod tests {
    use quarry_driver_api::Dialect;

    use super::*;

    #[test]
    fn pages_with_offset_fetch() {
        let table = TableRef {
            database: Some("dbo".into()),
            schema: None,
            name: "orders".into(),
        };
        assert_eq!(
            dialect().select_page(&table, None, Some("[id]"), 10, 20),
            "SELECT * FROM [dbo].[orders] ORDER BY [id] OFFSET 20 ROWS FETCH NEXT 10 ROWS ONLY"
        );
    }
}
