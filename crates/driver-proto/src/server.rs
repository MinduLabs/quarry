//! The driver side: serves one session over a pair of streams (stdin and stdout in a driver
//! binary).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use quarry_driver_api::{
    Driver, DriverError, ExecItem, ResolvedConfig, RowBatch, RowSink, Session, SharedCancel,
};
use quarry_model::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::wire::{
    Call, FrameError, FromDriver, Reply, STREAM_WINDOW, ToDriver, WireError, read_message,
    write_frame, write_rows,
};

/// What the writer sends. Rows go apart from other messages, in their binary form.
enum Outgoing {
    Message(FromDriver),
    Rows { id: u64, rows: Vec<Vec<Value>> },
    Batch { id: u64, batch: RowBatch },
}

/// The host's pacing of a `Stream` call, by call id.
enum Flow {
    Ack(u64),
    Stop(u64),
}

/// Answers calls until the host sends `Close` or closes `input`.
pub async fn serve<R, W>(
    driver: Arc<dyn Driver>,
    mut input: R,
    mut output: W,
) -> Result<(), FrameError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    // Row batches are pushed from the driver's synchronous sink, so the writer runs apart and
    // the sink only queues. `row_limit` bounds what a query can queue.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Outgoing>();
    let writer = tokio::spawn(async move {
        while let Some(outgoing) = out_rx.recv().await {
            match outgoing {
                Outgoing::Message(message) => write_frame(&mut output, &message).await?,
                // Encoded here, so the driver's sink stays a push into the queue.
                Outgoing::Rows { id, rows } => {
                    write_rows(&mut output, id, &RowBatch::encode(&rows)).await?
                }
                Outgoing::Batch { id, batch } => write_rows(&mut output, id, &batch).await?,
            }
        }
        Ok::<_, FrameError>(())
    });

    let cancel: Arc<Mutex<Option<SharedCancel>>> = Arc::default();
    let (call_tx, mut call_rx) = mpsc::unbounded_channel::<(u64, Call)>();
    // Apart from the calls: a stream waits for these while its call is still running.
    let (flow_tx, mut flow_rx) = mpsc::unbounded_channel::<Flow>();
    let reader_cancel = cancel.clone();
    let reader = tokio::spawn(async move {
        while let Some(message) = read_message::<_, ToDriver>(&mut input).await? {
            match message {
                ToDriver::Cancel => {
                    let handle = reader_cancel
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();
                    if let Some(handle) = handle {
                        handle.cancel();
                    }
                }
                ToDriver::Call { id, call } => {
                    if call_tx.send((id, *call)).is_err() {
                        break;
                    }
                }
                ToDriver::Ack { id } => {
                    let _ = flow_tx.send(Flow::Ack(id));
                }
                ToDriver::Stop { id } => {
                    let _ = flow_tx.send(Flow::Stop(id));
                }
            }
        }
        Ok::<_, FrameError>(())
    });

    let mut session: Option<Box<dyn Session>> = None;
    while let Some((id, call)) = call_rx.recv().await {
        let is_close = matches!(call, Call::Close);
        let result = answer(
            driver.as_ref(),
            &mut session,
            &cancel,
            id,
            call,
            &out_tx,
            &mut flow_rx,
        )
        .await;
        let reply = FromDriver::Reply {
            id,
            result: result.map_err(WireError::from),
        };
        if out_tx.send(Outgoing::Message(reply)).is_err() || is_close {
            break;
        }
    }
    if let Some(session) = session.take()
        && let Err(err) = session.close().await
    {
        tracing::debug!(%err, "session close");
    }
    drop(out_tx);
    reader.abort();
    match writer.await {
        Ok(result) => result,
        Err(err) => Err(FrameError::Io(std::io::Error::other(err))),
    }
}

async fn answer(
    driver: &dyn Driver,
    session: &mut Option<Box<dyn Session>>,
    cancel: &Mutex<Option<SharedCancel>>,
    id: u64,
    call: Call,
    out: &mpsc::UnboundedSender<Outgoing>,
    flow: &mut mpsc::UnboundedReceiver<Flow>,
) -> Result<Reply, DriverError> {
    let call = match call {
        Call::Connect { config, password } => {
            let opened = driver.connect(&ResolvedConfig { config, password }).await?;
            let server_version = opened.server_version();
            *cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(opened.cancel_handle());
            *session = Some(opened);
            return Ok(Reply::Connected { server_version });
        }
        Call::SchemaChange { change } => {
            return Ok(Reply::Statements(driver.schema_change(&change).await));
        }
        Call::Close => {
            if let Some(open) = session.take() {
                open.close().await?;
            }
            return Ok(Reply::Done);
        }
        call => call,
    };
    let Some(open) = session.as_mut() else {
        return Err(DriverError::ConnectionLost(
            "The driver has no open connection.".to_owned(),
        ));
    };
    let mut sink = |item| {
        let outgoing = match item {
            ExecItem::Rows(rows) => Outgoing::Rows { id, rows },
            ExecItem::Batch(batch) => Outgoing::Batch { id, batch },
            item => Outgoing::Message(FromDriver::Item { id, item }),
        };
        let _ = out.send(outgoing);
    };
    Ok(match call {
        Call::Execute { sql, opts } => {
            open.execute(&sql, &opts, &mut sink).await?;
            Reply::Done
        }
        Call::ExecuteParams { sql, params } => {
            Reply::Affected(open.execute_params(&sql, &params).await?)
        }
        Call::Query { sql, params, opts } => {
            open.query(&sql, &params, &opts, &mut sink).await?;
            Reply::Done
        }
        Call::Stream { sql, params, opts } => {
            let mut sink = PacedSink {
                id,
                out,
                flow,
                in_flight: 0,
                is_stopped: false,
            };
            open.stream(&sql, &params, &opts, &mut sink).await?;
            Reply::Done
        }
        Call::ListDatabases => Reply::Names(open.list_databases().await?),
        Call::ListObjects { database } => Reply::Objects(open.list_objects(&database).await?),
        Call::ListColumns { database } => Reply::Columns(open.list_columns(&database).await?),
        Call::TableInfo { table } => Reply::Table(open.table_info(&table).await?),
        Call::PrimaryKey { table } => Reply::Names(open.primary_key(&table).await?),
        Call::ServerInfo { kind } => Reply::ServerInfo(open.server_info(kind).await?),
        Call::UseDatabase { database } => {
            open.use_database(&database).await?;
            Reply::Done
        }
        Call::Begin => {
            open.begin().await?;
            Reply::Done
        }
        Call::Commit => {
            open.commit().await?;
            Reply::Done
        }
        Call::Rollback => {
            open.rollback().await?;
            Reply::Done
        }
        Call::SetReadOnly { read_only } => {
            open.set_read_only(read_only).await?;
            Reply::Done
        }
        Call::Connect { .. } | Call::SchemaChange { .. } | Call::Close => Reply::Done,
    })
}

/// Sends a stream's items, holding back once `STREAM_WINDOW` row batches wait for the host's
/// `Ack`, so the database is read no faster than the host writes.
struct PacedSink<'a> {
    id: u64,
    out: &'a mpsc::UnboundedSender<Outgoing>,
    flow: &'a mut mpsc::UnboundedReceiver<Flow>,
    in_flight: usize,
    is_stopped: bool,
}

impl PacedSink<'_> {
    /// Applies one pacing message. Those of an earlier stream are left over and ignored.
    fn apply(&mut self, flow: Flow) {
        match flow {
            Flow::Ack(of) if of == self.id => self.in_flight = self.in_flight.saturating_sub(1),
            Flow::Stop(of) if of == self.id => self.is_stopped = true,
            _ => {}
        }
    }
}

#[async_trait]
impl RowSink for PacedSink<'_> {
    async fn accept(&mut self, item: ExecItem) -> bool {
        while let Ok(flow) = self.flow.try_recv() {
            self.apply(flow);
        }
        let is_rows = matches!(item, ExecItem::Rows(_) | ExecItem::Batch(_));
        while is_rows && !self.is_stopped && self.in_flight >= STREAM_WINDOW {
            match self.flow.recv().await {
                Some(flow) => self.apply(flow),
                // The host is gone: nobody reads what would be sent.
                None => self.is_stopped = true,
            }
        }
        if self.is_stopped {
            return false;
        }
        let id = self.id;
        let outgoing = match item {
            ExecItem::Rows(rows) => Outgoing::Rows { id, rows },
            ExecItem::Batch(batch) => Outgoing::Batch { id, batch },
            item => Outgoing::Message(FromDriver::Item { id, item }),
        };
        if is_rows {
            self.in_flight += 1;
        }
        self.out.send(outgoing).is_ok()
    }
}
