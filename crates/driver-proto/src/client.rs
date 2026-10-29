//! The engine side: a [`Driver`] whose code runs in a driver binary, one process per session.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use quarry_driver_api::{
    CancelHandle, Driver, DriverError, ExecItem, ExecOpts, ExecSink, ResolvedConfig, Result,
    Session, SharedCancel, SpecDialect,
};
use quarry_model::{
    Capabilities, DriverId, SchemaChange, SchemaObject, ServerInfoKind, ServerInfoRow,
    TableColumns, TableInfo, TableRef, Value,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::wire::{
    Call, DriverInfo, Frame, FromDriver, Hello, PROTOCOL, Reply, ToDriver, read_frame,
    read_message, write_frame,
};

/// How long a driver binary may take to say what it serves, or to exit after `Close`.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// Asks the driver binary at `program` which drivers it serves. Fails when it does not start,
/// does not answer, or speaks another protocol.
pub async fn hello(program: &Path) -> std::result::Result<Hello, String> {
    let name = program.display();
    let run = Command::new(program)
        .arg("hello")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(START_TIMEOUT, run)
        .await
        .map_err(|_| format!("The driver {name} did not answer in time."))?
        .map_err(|err| format!("The driver {name} could not start: {err}."))?;
    if !output.status.success() {
        return Err(format!(
            "The driver {name} failed to start ({}).",
            output.status
        ));
    }
    let hello: Hello = read_message(&mut output.stdout.as_slice())
        .await
        .map_err(|err| format!("The driver {name} answered with something unreadable: {err}."))?
        .ok_or_else(|| format!("The driver {name} answered nothing."))?;
    if hello.protocol != PROTOCOL {
        return Err(format!(
            "The driver {name} is built for another version of Quarry (protocol {}, this Quarry \
             speaks {PROTOCOL}). Download the driver again.",
            hello.protocol
        ));
    }
    Ok(hello)
}

/// A driver served by a binary. Its capabilities and dialect come from the binary's `hello`.
#[derive(Debug)]
pub struct ProcessDriver {
    program: PathBuf,
    info: DriverInfo,
    dialect: SpecDialect,
}

impl ProcessDriver {
    pub fn new(program: PathBuf, info: DriverInfo) -> Self {
        let dialect = SpecDialect::new(info.dialect.clone());
        Self {
            program,
            info,
            dialect,
        }
    }

    async fn start(&self) -> Result<Remote> {
        let mut child = Command::new(&self.program)
            .arg("serve")
            .arg(&self.info.id.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| {
                DriverError::Connect(format!(
                    "The {} driver could not start ({}): {err}. Download the driver again.",
                    self.info.id.0,
                    self.program.display()
                ))
            })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(DriverError::Connect(format!(
                "The {} driver started without its pipes.",
                self.info.id.0
            )));
        };
        Ok(Remote::new(stdout, stdin, Some(child)))
    }
}

#[async_trait]
impl Driver for ProcessDriver {
    fn id(&self) -> DriverId {
        self.info.id.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.info.capabilities.clone()
    }

    fn dialect(&self) -> &SpecDialect {
        &self.dialect
    }

    async fn schema_change(
        &self,
        change: &SchemaChange,
    ) -> Option<std::result::Result<Vec<String>, String>> {
        let written = async {
            let mut remote = self.start().await?;
            let reply = remote
                .call(
                    Call::SchemaChange {
                        change: change.clone(),
                    },
                    None,
                )
                .await;
            remote.shutdown().await;
            match reply? {
                Reply::Statements(statements) => Ok(statements),
                other => Err(unexpected(&other)),
            }
        };
        written
            .await
            .unwrap_or_else(|err| Some(Err(err.to_string())))
    }

    async fn connect(&self, config: &ResolvedConfig) -> Result<Box<dyn Session>> {
        let mut remote = self.start().await?;
        let reply = remote
            .call(
                Call::Connect {
                    config: config.config.clone(),
                    password: config.password.clone(),
                },
                None,
            )
            .await?;
        let Reply::Connected { server_version } = reply else {
            remote.shutdown().await;
            return Err(unexpected(&reply));
        };
        Ok(Box::new(ProcessSession::new(remote, server_version)))
    }
}

/// The host end of one driver process.
struct Remote {
    to_driver: mpsc::UnboundedSender<ToDriver>,
    from_driver: Box<dyn AsyncRead + Unpin + Send>,
    next_id: u64,
    child: Option<Child>,
}

impl Remote {
    fn new<R, W>(reader: R, mut writer: W, child: Option<Child>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        // Writes go through one task, so a cancel can be sent while a call waits for its reply.
        let (to_driver, mut queue) = mpsc::unbounded_channel::<ToDriver>();
        tokio::spawn(async move {
            while let Some(message) = queue.recv().await {
                if let Err(err) = write_frame(&mut writer, &message).await {
                    tracing::debug!(%err, "driver input closed");
                    break;
                }
            }
        });
        Self {
            to_driver,
            from_driver: Box::new(reader),
            next_id: 1,
            child,
        }
    }

    /// Sends `call` and waits for its reply, passing result items to `sink` on the way.
    async fn call(&mut self, call: Call, mut sink: Option<ExecSink<'_>>) -> Result<Reply> {
        let id = self.next_id;
        self.next_id += 1;
        self.to_driver
            .send(ToDriver::Call {
                id,
                call: Box::new(call),
            })
            .map_err(|_| stopped())?;
        loop {
            // Messages of an earlier call that was given up on are skipped by their id.
            match read_frame::<_, FromDriver>(&mut self.from_driver).await {
                Ok(Some(Frame::Rows { id: of, rows })) if of == id => {
                    if let Some(sink) = sink.as_mut() {
                        sink(ExecItem::Rows(rows));
                    }
                }
                Ok(Some(Frame::Message(FromDriver::Item { id: of, item }))) if of == id => {
                    if let Some(sink) = sink.as_mut() {
                        sink(item);
                    }
                }
                Ok(Some(Frame::Message(FromDriver::Reply { id: of, result }))) if of == id => {
                    return result.map_err(DriverError::from);
                }
                Ok(Some(_)) => {}
                Ok(None) => return Err(stopped()),
                Err(err) => {
                    return Err(DriverError::ConnectionLost(format!(
                        "The driver sent a message Quarry cannot read: {err}. Reconnect to \
                         continue."
                    )));
                }
            }
        }
    }

    fn cancel_handle(&self) -> SharedCancel {
        std::sync::Arc::new(RemoteCancel(self.to_driver.clone()))
    }

    /// Ends the process: `Close` first, so the driver closes its connection, then a kill if it
    /// lingers.
    async fn shutdown(mut self) {
        let _ = self.call(Call::Close, None).await;
        let Some(mut child) = self.child.take() else {
            return;
        };
        drop(self.to_driver);
        if tokio::time::timeout(START_TIMEOUT, child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
        }
    }
}

struct RemoteCancel(mpsc::UnboundedSender<ToDriver>);

impl CancelHandle for RemoteCancel {
    fn cancel(&self) {
        let _ = self.0.send(ToDriver::Cancel);
    }
}

struct ProcessSession {
    remote: Remote,
    server_version: String,
    cancel: SharedCancel,
}

impl ProcessSession {
    fn new(remote: Remote, server_version: String) -> Self {
        let cancel = remote.cancel_handle();
        Self {
            remote,
            server_version,
            cancel,
        }
    }

    async fn done(&mut self, call: Call) -> Result<()> {
        match self.remote.call(call, None).await? {
            Reply::Done => Ok(()),
            other => Err(unexpected(&other)),
        }
    }
}

#[async_trait]
impl Session for ProcessSession {
    fn server_version(&self) -> String {
        self.server_version.clone()
    }

    async fn execute(&mut self, sql: &str, opts: &ExecOpts, sink: ExecSink<'_>) -> Result<()> {
        let call = Call::Execute {
            sql: sql.to_owned(),
            opts: opts.clone(),
        };
        match self.remote.call(call, Some(sink)).await? {
            Reply::Done => Ok(()),
            other => Err(unexpected(&other)),
        }
    }

    async fn execute_params(&mut self, sql: &str, params: &[Value]) -> Result<u64> {
        let call = Call::ExecuteParams {
            sql: sql.to_owned(),
            params: params.to_vec(),
        };
        match self.remote.call(call, None).await? {
            Reply::Affected(n) => Ok(n),
            other => Err(unexpected(&other)),
        }
    }

    async fn query(
        &mut self,
        sql: &str,
        params: &[Value],
        opts: &ExecOpts,
        sink: ExecSink<'_>,
    ) -> Result<()> {
        let call = Call::Query {
            sql: sql.to_owned(),
            params: params.to_vec(),
            opts: opts.clone(),
        };
        match self.remote.call(call, Some(sink)).await? {
            Reply::Done => Ok(()),
            other => Err(unexpected(&other)),
        }
    }

    async fn list_databases(&mut self) -> Result<Vec<String>> {
        match self.remote.call(Call::ListDatabases, None).await? {
            Reply::Names(names) => Ok(names),
            other => Err(unexpected(&other)),
        }
    }

    async fn list_objects(&mut self, database: &str) -> Result<Vec<SchemaObject>> {
        let call = Call::ListObjects {
            database: database.to_owned(),
        };
        match self.remote.call(call, None).await? {
            Reply::Objects(objects) => Ok(objects),
            other => Err(unexpected(&other)),
        }
    }

    async fn list_columns(&mut self, database: &str) -> Result<Vec<TableColumns>> {
        let call = Call::ListColumns {
            database: database.to_owned(),
        };
        match self.remote.call(call, None).await? {
            Reply::Columns(columns) => Ok(columns),
            other => Err(unexpected(&other)),
        }
    }

    async fn table_info(&mut self, table: &TableRef) -> Result<TableInfo> {
        let call = Call::TableInfo {
            table: table.clone(),
        };
        match self.remote.call(call, None).await? {
            Reply::Table(info) => Ok(info),
            other => Err(unexpected(&other)),
        }
    }

    async fn primary_key(&mut self, table: &TableRef) -> Result<Vec<String>> {
        let call = Call::PrimaryKey {
            table: table.clone(),
        };
        match self.remote.call(call, None).await? {
            Reply::Names(names) => Ok(names),
            other => Err(unexpected(&other)),
        }
    }

    async fn server_info(&mut self, kind: ServerInfoKind) -> Result<Vec<ServerInfoRow>> {
        match self.remote.call(Call::ServerInfo { kind }, None).await? {
            Reply::ServerInfo(rows) => Ok(rows),
            other => Err(unexpected(&other)),
        }
    }

    async fn use_database(&mut self, database: &str) -> Result<()> {
        self.done(Call::UseDatabase {
            database: database.to_owned(),
        })
        .await
    }

    async fn begin(&mut self) -> Result<()> {
        self.done(Call::Begin).await
    }

    async fn commit(&mut self) -> Result<()> {
        self.done(Call::Commit).await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.done(Call::Rollback).await
    }

    async fn set_read_only(&mut self, read_only: bool) -> Result<()> {
        self.done(Call::SetReadOnly { read_only }).await
    }

    fn cancel_handle(&self) -> SharedCancel {
        self.cancel.clone()
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.remote.shutdown().await;
        Ok(())
    }
}

fn stopped() -> DriverError {
    DriverError::ConnectionLost("The driver process stopped. Reconnect to continue.".to_owned())
}

fn unexpected(reply: &Reply) -> DriverError {
    let name = match reply {
        Reply::Done => "Done",
        Reply::Connected { .. } => "Connected",
        Reply::Affected(_) => "Affected",
        Reply::Names(_) => "Names",
        Reply::Objects(_) => "Objects",
        Reply::Columns(_) => "Columns",
        Reply::Table(_) => "Table",
        Reply::ServerInfo(_) => "ServerInfo",
        Reply::Statements(_) => "Statements",
    };
    DriverError::Other(format!("The driver answered with an unexpected {name}.").into())
}

/// A session over any pair of streams, connected like [`ProcessDriver::connect`] does.
#[cfg(test)]
pub(crate) async fn connect_over<R, W>(
    reader: R,
    writer: W,
    config: &ResolvedConfig,
) -> Result<Box<dyn Session>>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut remote = Remote::new(reader, writer, None);
    let reply = remote
        .call(
            Call::Connect {
                config: config.config.clone(),
                password: config.password.clone(),
            },
            None,
        )
        .await?;
    let Reply::Connected { server_version } = reply else {
        return Err(unexpected(&reply));
    };
    Ok(Box::new(ProcessSession::new(remote, server_version)))
}
