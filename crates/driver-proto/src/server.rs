//! The driver side: serves one session over a pair of streams (stdin and stdout in a driver
//! binary).

use std::sync::{Arc, Mutex};

use quarry_driver_api::{Driver, DriverError, ExecItem, ResolvedConfig, Session, SharedCancel};
use quarry_model::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::wire::{
    Call, FrameError, FromDriver, Reply, ToDriver, WireError, read_message, write_frame, write_rows,
};

/// What the writer sends. Rows go apart from other messages, in their binary form.
enum Outgoing {
    Message(FromDriver),
    Rows { id: u64, rows: Vec<Vec<Value>> },
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
                Outgoing::Rows { id, rows } => write_rows(&mut output, id, &rows).await?,
            }
        }
        Ok::<_, FrameError>(())
    });

    let cancel: Arc<Mutex<Option<SharedCancel>>> = Arc::default();
    let (call_tx, mut call_rx) = mpsc::unbounded_channel::<(u64, Call)>();
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
            }
        }
        Ok::<_, FrameError>(())
    });

    let mut session: Option<Box<dyn Session>> = None;
    while let Some((id, call)) = call_rx.recv().await {
        let is_close = matches!(call, Call::Close);
        let result = answer(driver.as_ref(), &mut session, &cancel, id, call, &out_tx).await;
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
