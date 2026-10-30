//! Exports that stream every row: a query run again, a whole table, and the row count that goes
//! with a table. Each runs on a connection of its own, so the tab stays free and a cancel never
//! has to wait for it.
//!
//! Rows travel from the database to the file through a small queue: the reader waits when the
//! writer is behind, so memory holds a few batches whatever the table's size. The file is a
//! `.part` until the last row is in, so a cancelled or failed export leaves nothing behind.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use quarry_driver_api::{Dialect, DriverError, ExecItem, ExecOpts, RowSink, Session, SharedCancel};
use quarry_export::{Output, Summary, Writer};
use quarry_model::{BrowseQuery, ColumnMeta, ConnId, ExportFormat, ExportOptions, TableRef, Value};
use tokio::sync::mpsc;

use crate::engine::{Engine, describe, lock};
use crate::export::{RESULT_FORMATS, TABLE_FORMATS, read_statement};
use crate::query::{first_value, row_count_of};
use crate::{Event, RequestId};

/// Batches waiting between the database and the file. Small on purpose: the point of the queue
/// is that the reader waits.
const QUEUE_BATCHES: usize = 4;
/// Rows the driver hands over at a time.
const BATCH_ROWS: usize = 1000;
/// Progress events are at most this often, so a fast export does not flood the client.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

const STOPPED: &str = "The export stopped unexpectedly. Try again.";

/// What can stop an export or a row count from outside it.
#[derive(Default)]
pub(crate) struct ExportControl {
    is_cancelled: AtomicBool,
    /// Cancels the statement on the server, once the job has a session.
    kill: Mutex<Option<SharedCancel>>,
}

impl ExportControl {
    fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::SeqCst)
    }

    /// Stop the job: no more rows are taken, and the running statement is cancelled.
    pub(crate) fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
        if let Some(kill) = lock(&self.kill).clone() {
            kill.cancel();
        }
    }

    /// Give the job's session to `cancel`. A cancel that came first fires at once.
    fn arm(&self, kill: SharedCancel) {
        *lock(&self.kill) = Some(kill.clone());
        if self.is_cancelled() {
            kill.cancel();
        }
    }
}

/// Rows and bytes in the file so far, written by the writer and read for progress events.
#[derive(Default)]
struct Progress {
    rows: AtomicU64,
    bytes: AtomicU64,
}

/// The output, counting what goes into it.
struct Counted {
    output: Output,
    progress: Arc<Progress>,
}

impl Write for Counted {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.output.write(buf)?;
        self.progress
            .bytes
            .fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

/// What the reader tells the writer. A queue that closes without `Done` means "give up".
enum Msg {
    Columns(Vec<ColumnMeta>),
    Rows(Vec<Vec<Value>>),
    Done,
}

enum WriteEnd {
    Done(Summary),
    /// The queue closed without `Done`: nothing was committed.
    Aborted,
    Failed(String),
}

enum End {
    Finished(Summary),
    Cancelled,
    Failed(String),
}

/// Why the reader stopped before the statement ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    Cancelled,
    Limit,
    /// The writer is gone: it failed, and says why.
    WriterGone,
    /// A batch from the driver could not be decoded.
    Unreadable(String),
}

pub(crate) struct ExportJob {
    pub(crate) request: RequestId,
    pub(crate) conn: ConnId,
    pub(crate) database: Option<String>,
    pub(crate) sql: String,
    pub(crate) params: Vec<Value>,
    pub(crate) format: ExportFormat,
    pub(crate) options: ExportOptions,
    pub(crate) path: PathBuf,
    /// The columns the writer starts with. `None`: the ones the database sends.
    pub(crate) columns: Option<Vec<ColumnMeta>>,
    /// The names the first run gave the columns. A second run must give the same.
    pub(crate) expected: Option<Vec<String>>,
    pub(crate) limit: Option<u64>,
}

/// Feeds the writer's queue. It waits when the queue is full, which makes the driver wait.
struct ChannelSink {
    tx: mpsc::Sender<Msg>,
    control: Arc<ExportControl>,
    progress: Arc<Progress>,
    events: async_channel::Sender<Event>,
    request: RequestId,
    limit: Option<u64>,
    taken: u64,
    has_columns: bool,
    last_progress: Instant,
    stop: Option<Stop>,
}

impl ChannelSink {
    fn emit_progress(&mut self) {
        self.last_progress = Instant::now();
        let _ = self.events.try_send(Event::ExportProgress {
            request: self.request,
            rows: self.progress.rows.load(Ordering::Relaxed),
            bytes: self.progress.bytes.load(Ordering::Relaxed),
        });
    }
}

#[async_trait]
impl RowSink for ChannelSink {
    async fn accept(&mut self, item: ExecItem) -> bool {
        if self.control.is_cancelled() {
            self.stop = Some(Stop::Cancelled);
            return false;
        }
        match item {
            ExecItem::Columns(columns) => {
                if self.has_columns {
                    // A second result set: one statement exports one.
                    return false;
                }
                self.has_columns = true;
                if self.tx.send(Msg::Columns(columns)).await.is_err() {
                    self.stop = Some(Stop::WriterGone);
                    return false;
                }
                true
            }
            ExecItem::Rows(batch) => self.take_rows(batch).await,
            // Out-of-process drivers send rows encoded; the writer takes them as values.
            ExecItem::Batch(batch) => match batch.decode() {
                Ok(rows) => self.take_rows(rows).await,
                Err(err) => {
                    self.stop = Some(Stop::Unreadable(format!("Couldn't read the rows: {err}")));
                    false
                }
            },
            // The end of the first (only) result set.
            ExecItem::SetDone { .. } => false,
            ExecItem::Notice(_) | ExecItem::Done { .. } => true,
        }
    }
}

impl ChannelSink {
    async fn take_rows(&mut self, mut batch: Vec<Vec<Value>>) -> bool {
        if let Some(limit) = self.limit {
            let room = limit.saturating_sub(self.taken);
            batch.truncate(usize::try_from(room).unwrap_or(usize::MAX));
        }
        self.taken += batch.len() as u64;
        if self.tx.send(Msg::Rows(batch)).await.is_err() {
            self.stop = Some(Stop::WriterGone);
            return false;
        }
        if self.last_progress.elapsed() >= PROGRESS_EVERY {
            self.emit_progress();
        }
        if self.limit.is_some_and(|limit| self.taken >= limit) {
            self.stop = Some(Stop::Limit);
            return false;
        }
        true
    }
}

impl Engine {
    /// Run `sql` again and write every row. The statement is checked to be one that reads.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn export_query(
        self: Arc<Self>,
        request: RequestId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
        columns: Vec<ColumnMeta>,
        format: ExportFormat,
        options: ExportOptions,
        path: String,
        limit: Option<u64>,
    ) {
        let control = self.register_export(request);
        let job = async {
            if !RESULT_FORMATS.contains(&format) {
                return Err("This format cannot export every row of a query.".to_owned());
            }
            let statement = read_statement(&sql)?.to_owned();
            let path = checked_path(&path)?;
            check_limit(limit)?;
            let expected = columns.iter().map(|c| c.name.clone()).collect();
            let (options, writer_columns) =
                self.writer_input(conn, format, &options, &columns).await?;
            Ok(ExportJob {
                request,
                conn,
                database,
                sql: statement,
                params: Vec::new(),
                format,
                options,
                path,
                columns: Some(writer_columns),
                expected: Some(expected),
                limit,
            })
        }
        .await;
        self.start_export(request, control, job).await;
    }

    /// Write a whole table, with its filter and sort.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn export_table(
        self: Arc<Self>,
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        query: BrowseQuery,
        format: ExportFormat,
        mut options: ExportOptions,
        path: String,
        limit: Option<u64>,
    ) {
        let control = self.register_export(request);
        let job = (|| {
            if !TABLE_FORMATS.contains(&format) {
                return Err("This format cannot export a whole table.".to_owned());
            }
            let path = checked_path(&path)?;
            check_limit(limit)?;
            // Every row: the statement of a page with no row limit.
            let page = self.page_query(conn, &table, &query, limit.unwrap_or(u64::MAX), 0)?;
            if options.table.is_none() {
                options.table = Some(table.clone());
            }
            Ok(ExportJob {
                request,
                conn,
                database: None,
                sql: page.sql,
                params: page.params,
                format,
                options,
                path,
                columns: None,
                expected: None,
                limit,
            })
        })();
        self.start_export(request, control, job).await;
    }

    /// The exact number of rows of a table with its filter, on a connection of its own.
    pub(crate) async fn export_count_rows(
        self: Arc<Self>,
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        query: BrowseQuery,
    ) {
        let control = self.register_export(request);
        let result = self
            .count_on_own_session(conn, &table, &query, &control)
            .await;
        lock(&self.exports).remove(&request);
        let (count, error) = match result {
            Ok(count) => (Some(count), None),
            Err(error) => (None, Some(error)),
        };
        self.emit(Event::ExportRowCount {
            request,
            count,
            error,
        });
    }

    pub(crate) fn cancel_export(&self, request: RequestId) {
        let control = lock(&self.exports).get(&request).cloned();
        if let Some(control) = control {
            control.cancel();
        }
    }

    /// A cancel can come before the job is ready, so the job is findable from the first moment.
    fn register_export(&self, request: RequestId) -> Arc<ExportControl> {
        let control = Arc::new(ExportControl::default());
        lock(&self.exports).insert(request, control.clone());
        control
    }

    async fn start_export(
        self: Arc<Self>,
        request: RequestId,
        control: Arc<ExportControl>,
        job: Result<ExportJob, String>,
    ) {
        let started = Instant::now();
        let end = match job {
            Ok(job) => {
                let path = job.path.display().to_string();
                let format = job.format;
                let end = self.stream_export(&job, &control).await;
                if let End::Finished(summary) = &end {
                    self.export_finished("export.stream", started, request, path, format, summary);
                }
                end
            }
            Err(error) => End::Failed(error),
        };
        lock(&self.exports).remove(&request);
        match end {
            End::Finished(_) => {}
            End::Cancelled => self.emit(Event::ExportFailed {
                request,
                error: "The export was cancelled. No file was written.".to_owned(),
                cancelled: true,
            }),
            End::Failed(error) => self.emit(Event::ExportFailed {
                request,
                error,
                cancelled: false,
            }),
        }
    }

    async fn stream_export(self: &Arc<Self>, job: &ExportJob, control: &Arc<ExportControl>) -> End {
        // The file first: a folder that does not exist is found before any work on the database.
        let path = job.path.clone();
        let output = match tokio::task::spawn_blocking(move || Output::create(&path)).await {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => return End::Failed(err.to_string()),
            Err(_) => return End::Failed(STOPPED.to_owned()),
        };
        let mut session = match self
            .open_export_session(job.conn, job.database.as_deref(), control)
            .await
        {
            Ok(session) => session,
            Err(_) if control.is_cancelled() => return End::Cancelled,
            Err(error) => return End::Failed(error),
        };

        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel::<Msg>(QUEUE_BATCHES);
        let writer = {
            let engine = self.clone();
            let input = WriterInput {
                conn: job.conn,
                format: job.format,
                options: job.options.clone(),
                columns: job.columns.clone(),
                expected: job.expected.clone(),
            };
            let counted = Counted {
                output,
                progress: progress.clone(),
            };
            let progress = progress.clone();
            tokio::task::spawn_blocking(move || {
                write_stream(&engine, &input, counted, rx, &progress)
            })
        };

        let mut sink = ChannelSink {
            tx: tx.clone(),
            control: control.clone(),
            progress: progress.clone(),
            events: self.events.clone(),
            request: job.request,
            limit: job.limit,
            taken: 0,
            has_columns: false,
            last_progress: Instant::now(),
            stop: None,
        };
        let opts = ExecOpts {
            row_limit: None,
            batch_size: BATCH_ROWS,
        };
        let result = session
            .stream(&job.sql, &job.params, &opts, &mut sink)
            .await;
        let stop = sink.stop.take();
        drop(sink);
        let is_cancelled = control.is_cancelled();
        if result.is_ok() && !is_cancelled && stop != Some(Stop::WriterGone) {
            // The writer may be gone already; then its own error is the answer.
            let _ = tx.send(Msg::Done).await;
        }
        drop(tx);

        // Rows still in flight (a limit, a failed writer) are ended on the server, or closing
        // would read them all. A cancel already did that.
        if stop.is_some() && !is_cancelled {
            session.cancel_handle().cancel();
        } else if result.is_ok() {
            let _ = session.rollback().await;
        }
        if let Err(err) = session.close().await {
            tracing::debug!(%err, "export session close");
        }

        let written = writer.await.unwrap_or(WriteEnd::Failed(STOPPED.to_owned()));
        if is_cancelled {
            return End::Cancelled;
        }
        if let Some(Stop::Unreadable(error)) = stop {
            return End::Failed(error);
        }
        match (written, result) {
            (WriteEnd::Failed(error), _) => End::Failed(error),
            (_, Err(DriverError::Unsupported(_))) => {
                End::Failed("This database cannot export every row yet.".to_owned())
            }
            (_, Err(err)) => End::Failed(describe(&err)),
            (WriteEnd::Done(summary), Ok(())) => End::Finished(summary),
            (WriteEnd::Aborted, Ok(())) => End::Failed(STOPPED.to_owned()),
        }
    }

    /// A connection of the export's own: read only, in one transaction, so every row comes from
    /// the same moment.
    async fn open_export_session(
        &self,
        conn: ConnId,
        database: Option<&str>,
        control: &ExportControl,
    ) -> Result<Box<dyn Session>, String> {
        let config = self
            .config_of(conn)
            .ok_or_else(|| "Connection not found".to_owned())?;
        let (resolved, _) = self
            .resolve(config, None, None, crate::connections::TunnelUse::Shared)
            .await?;
        let mut session = self.open_session(&resolved).await?;
        control.arm(session.cancel_handle());
        let ready = async {
            session.set_read_only(true).await?;
            if let Some(database) = database {
                session.use_database(database).await?;
            }
            session.begin().await
        }
        .await;
        if let Err(err) = ready {
            let _ = session.close().await;
            return Err(describe(&err));
        }
        if control.is_cancelled() {
            let _ = session.close().await;
            return Err("The export was cancelled.".to_owned());
        }
        Ok(session)
    }

    async fn count_on_own_session(
        &self,
        conn: ConnId,
        table: &TableRef,
        query: &BrowseQuery,
        control: &ExportControl,
    ) -> Result<u64, String> {
        let (sql, params) = self.count_statement(conn, table, query)?;
        let mut session = self.open_export_session(conn, None, control).await?;
        let result = first_value(session.as_mut(), &sql, &params).await;
        let _ = session.close().await;
        if control.is_cancelled() {
            return Err("Counting was cancelled.".to_owned());
        }
        row_count_of(result.map_err(|err| describe(&err))?)
    }
}

struct WriterInput {
    conn: ConnId,
    format: ExportFormat,
    options: ExportOptions,
    columns: Option<Vec<ColumnMeta>>,
    expected: Option<Vec<String>>,
}

/// The writer's side, on the blocking pool: take what the reader queues and write it. Anything
/// but `Done` ends without committing, and dropping the output removes the `.part` file.
fn write_stream(
    engine: &Engine,
    input: &WriterInput,
    output: Counted,
    mut rx: mpsc::Receiver<Msg>,
    progress: &Progress,
) -> WriteEnd {
    let driver = match engine.driver_of(input.conn) {
        Ok(driver) => driver,
        Err(error) => return WriteEnd::Failed(error),
    };
    let dialect: &dyn Dialect = driver.dialect();
    let mut pending = Some(output);
    let mut writer: Option<Writer<'_, Counted>> = None;
    loop {
        match rx.blocking_recv() {
            None => return WriteEnd::Aborted,
            Some(Msg::Columns(found)) => {
                let Some(output) = pending.take() else {
                    continue;
                };
                if let Some(expected) = &input.expected
                    && found
                        .iter()
                        .map(|c| c.name.as_str())
                        .ne(expected.iter().map(String::as_str))
                {
                    return WriteEnd::Failed(
                        "The query now returns different columns than the grid shows. Run it \
                         again, then export again."
                            .to_owned(),
                    );
                }
                let columns = input.columns.clone().unwrap_or(found);
                match Writer::start(input.format, &input.options, dialect, output, &columns) {
                    Ok(started) => writer = Some(started),
                    Err(err) => return WriteEnd::Failed(err.to_string()),
                }
            }
            Some(Msg::Rows(batch)) => {
                let Some(writer) = writer.as_mut() else {
                    return WriteEnd::Failed("The query returned no result to export.".to_owned());
                };
                if let Err(err) = writer.write_batch(&batch) {
                    return WriteEnd::Failed(err.to_string());
                }
                progress.rows.store(writer.rows(), Ordering::Relaxed);
            }
            Some(Msg::Done) => break,
        }
    }
    let Some(writer) = writer else {
        return WriteEnd::Failed("The query returned no result to export.".to_owned());
    };
    let summary = writer.summary();
    let committed = writer.finish().and_then(|counted| counted.output.commit());
    match committed {
        Ok(()) => {
            progress.rows.store(summary.rows, Ordering::Relaxed);
            WriteEnd::Done(summary)
        }
        Err(err) => WriteEnd::Failed(err.to_string()),
    }
}

fn checked_path(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err("Choose a file to save to.".to_owned());
    }
    Ok(PathBuf::from(path))
}

fn check_limit(limit: Option<u64>) -> Result<(), String> {
    if limit == Some(0) {
        return Err("Choose at least one row to export.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use async_trait::async_trait;
    use quarry_driver_api::{
        CancelHandle, Driver, ExecSink, ResolvedConfig, Result as DriverResult, SpecDialect,
    };
    use quarry_model::{
        Capabilities, ConnectionConfig, DriverId, Environment, SchemaObject, SslMode,
    };

    use super::*;
    use crate::edit::tests::ansi_spec;
    use crate::{Command, EngineOptions, InProcessDrivers};

    /// What a fake database does with a statement, and what happened to its connection.
    #[derive(Default)]
    struct Script {
        batches: Vec<Vec<Vec<Value>>>,
        /// Wait this long before each batch, so a test can act while the export runs.
        delay: Duration,
        /// Fail with a lost connection after this many batches.
        lose_after: Option<usize>,
        is_unsupported: bool,
        columns: Vec<&'static str>,
        connects: AtomicUsize,
        is_killed: AtomicBool,
        is_closed: AtomicBool,
        sent: AtomicUsize,
    }

    struct FakeDriver {
        script: Arc<Script>,
        dialect: SpecDialect,
    }

    #[async_trait]
    impl Driver for FakeDriver {
        fn id(&self) -> DriverId {
            DriverId("fake".into())
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                schemas: false,
                transactions: true,
                editable_results: false,
                explain: false,
                schema_edit: false,
                server_info: false,
                cancel: true,
                ssh: false,
                ssl: false,
                object_kinds: Vec::new(),
            }
        }
        fn dialect(&self) -> &SpecDialect {
            &self.dialect
        }
        async fn connect(&self, _: &ResolvedConfig) -> DriverResult<Box<dyn Session>> {
            self.script.connects.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeSession {
                script: self.script.clone(),
            }))
        }
    }

    struct FakeSession {
        script: Arc<Script>,
    }

    struct FakeKill(Arc<Script>);

    impl CancelHandle for FakeKill {
        fn cancel(&self) {
            self.0.is_killed.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl Session for FakeSession {
        fn server_version(&self) -> String {
            "fake".into()
        }
        async fn execute(&mut self, _: &str, _: &ExecOpts, _: ExecSink<'_>) -> DriverResult<()> {
            Ok(())
        }
        async fn execute_params(&mut self, _: &str, _: &[Value]) -> DriverResult<u64> {
            Ok(0)
        }
        async fn stream(
            &mut self,
            _: &str,
            _: &[Value],
            _: &ExecOpts,
            sink: &mut dyn RowSink,
        ) -> DriverResult<()> {
            let script = self.script.clone();
            if script.is_unsupported {
                return Err(DriverError::Unsupported("streaming every row"));
            }
            let columns = script
                .columns
                .iter()
                .map(|name| ColumnMeta {
                    name: (*name).into(),
                    type_name: "INT".into(),
                    nullable: None,
                    source_database: None,
                    source_table: None,
                    source_column: None,
                })
                .collect();
            if !sink.accept(ExecItem::Columns(columns)).await {
                return Ok(());
            }
            for (index, batch) in script.batches.iter().enumerate() {
                if script.lose_after == Some(index) {
                    return Err(DriverError::ConnectionLost(
                        "The connection dropped.".into(),
                    ));
                }
                if !script.delay.is_zero() {
                    tokio::time::sleep(script.delay).await;
                }
                if script.is_killed.load(Ordering::SeqCst) {
                    return Err(DriverError::Cancelled);
                }
                script.sent.fetch_add(1, Ordering::SeqCst);
                if !sink.accept(ExecItem::Rows(batch.clone())).await {
                    return Ok(());
                }
            }
            sink.accept(ExecItem::SetDone {
                affected: 0,
                rows: 0,
                truncated: false,
            })
            .await;
            Ok(())
        }
        async fn list_databases(&mut self) -> DriverResult<Vec<String>> {
            Ok(Vec::new())
        }
        async fn list_objects(&mut self, _: &str) -> DriverResult<Vec<SchemaObject>> {
            Ok(Vec::new())
        }
        async fn use_database(&mut self, _: &str) -> DriverResult<()> {
            Ok(())
        }
        async fn begin(&mut self) -> DriverResult<()> {
            Ok(())
        }
        async fn commit(&mut self) -> DriverResult<()> {
            Ok(())
        }
        async fn rollback(&mut self) -> DriverResult<()> {
            Ok(())
        }
        async fn set_read_only(&mut self, _: bool) -> DriverResult<()> {
            Ok(())
        }
        fn cancel_handle(&self) -> SharedCancel {
            Arc::new(FakeKill(self.script.clone()))
        }
        async fn close(self: Box<Self>) -> DriverResult<()> {
            self.script.is_closed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Fixture {
        engine: Arc<Engine>,
        events: async_channel::Receiver<Event>,
        script: Arc<Script>,
        conn: ConnId,
        dir: PathBuf,
    }

    fn fixture(script: Script) -> Fixture {
        let dir = std::env::temp_dir().join(format!("quarry-export-job-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, events) = async_channel::unbounded();
        let script = Arc::new(script);
        let engine = Engine::new(
            EngineOptions {
                driver_dir: Some(dir.clone()),
                in_process_drivers: InProcessDrivers(vec![Arc::new(FakeDriver {
                    script: script.clone(),
                    dialect: ansi_spec(),
                })]),
                ..crate::test_support::options(&dir)
            },
            tx,
        );
        let config = ConnectionConfig {
            id: ConnId::new(),
            name: "fake".into(),
            driver: DriverId("fake".into()),
            host: None,
            port: None,
            database: None,
            user: None,
            path: None,
            ssl: SslMode::Disable,
            ssh: None,
            environment: Environment::Local,
            read_only: false,
            color: None,
            group: None,
        };
        let conn = config.id;
        lock(&engine.connections).push(config);
        Fixture {
            engine: Arc::new(engine),
            events,
            script,
            conn,
            dir,
        }
    }

    fn rows(from: i64, count: i64) -> Vec<Vec<Value>> {
        (from..from + count).map(|n| vec![Value::Int(n)]).collect()
    }

    fn query_command(f: &Fixture, request: RequestId, limit: Option<u64>) -> Command {
        Command::ExportQuery {
            request,
            conn: f.conn,
            database: None,
            sql: "SELECT n FROM numbers".into(),
            columns: vec![ColumnMeta {
                name: "n".into(),
                type_name: "INT".into(),
                nullable: None,
                source_database: None,
                source_table: None,
                source_column: None,
            }],
            format: ExportFormat::Csv,
            options: ExportOptions::default(),
            path: f.dir.join("out.csv").display().to_string(),
            limit,
        }
    }

    /// The events up to and including the one that ends the export.
    async fn until_end(events: &async_channel::Receiver<Event>) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(20), events.recv())
                .await
                .expect("the export ends")
                .unwrap();
            let is_end = matches!(
                event,
                Event::ExportFinished { .. } | Event::ExportFailed { .. }
            );
            seen.push(event);
            if is_end {
                return seen;
            }
        }
    }

    fn files(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("out."))
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn every_row_reaches_the_file_and_the_connection_is_closed() {
        let f = fixture(Script {
            columns: vec!["n"],
            batches: vec![rows(1, 2), rows(3, 2), rows(5, 2)],
            ..Script::default()
        });
        f.engine.clone().dispatch(query_command(&f, 1, None)).await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFinished { rows, sheets, .. } => assert_eq!((*rows, *sheets), (6, 0)),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(f.dir.join("out.csv")).unwrap(),
            "n\r\n1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n"
        );
        assert_eq!(files(&f.dir), ["out.csv"], "no .part file is left");
        assert!(f.script.is_closed.load(Ordering::SeqCst));
        assert!(
            !f.script.is_killed.load(Ordering::SeqCst),
            "nothing was in flight"
        );
        assert!(lock(&f.engine.exports).is_empty());
    }

    #[tokio::test]
    async fn a_limit_stops_the_read_and_ends_the_statement_on_the_server() {
        let f = fixture(Script {
            columns: vec!["n"],
            batches: vec![rows(1, 4), rows(5, 4), rows(9, 4)],
            ..Script::default()
        });
        f.engine
            .clone()
            .dispatch(query_command(&f, 2, Some(5)))
            .await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFinished { rows, .. } => assert_eq!(*rows, 5),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(f.dir.join("out.csv")).unwrap(),
            "n\r\n1\r\n2\r\n3\r\n4\r\n5\r\n"
        );
        assert_eq!(
            f.script.sent.load(Ordering::SeqCst),
            2,
            "the third batch was never read"
        );
        assert!(
            f.script.is_killed.load(Ordering::SeqCst),
            "rows in flight are ended"
        );
        assert!(f.script.is_closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn covers_ae5_cancel_midway_leaves_no_file() {
        let f = fixture(Script {
            columns: vec!["n"],
            batches: (0..200).map(|b| rows(b * 10, 10)).collect(),
            delay: Duration::from_millis(20),
            ..Script::default()
        });
        let engine = f.engine.clone();
        let running = tokio::spawn(engine.clone().dispatch(query_command(&f, 3, None)));
        // Wait until the file has rows, then cancel.
        for _ in 0..200 {
            if f.script.sent.load(Ordering::SeqCst) >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        engine.cancel_export(3);
        running.await.unwrap();
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFailed {
                request, cancelled, ..
            } => assert_eq!((*request, *cancelled), (3, true)),
            other => panic!("unexpected {other:?}"),
        }
        assert!(files(&f.dir).is_empty(), "{:?}", files(&f.dir));
        assert!(f.script.is_killed.load(Ordering::SeqCst));
        assert!(f.script.is_closed.load(Ordering::SeqCst));
        assert!(
            f.script.sent.load(Ordering::SeqCst) < 200,
            "the read stopped"
        );
    }

    #[tokio::test]
    async fn a_lost_connection_fails_without_a_file_and_without_a_kill() {
        let f = fixture(Script {
            columns: vec!["n"],
            batches: vec![rows(1, 3), rows(4, 3), rows(7, 3)],
            lose_after: Some(2),
            ..Script::default()
        });
        f.engine.clone().dispatch(query_command(&f, 4, None)).await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFailed {
                error, cancelled, ..
            } => {
                assert!(!cancelled);
                assert!(error.contains("connection dropped"), "{error}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(files(&f.dir).is_empty(), "{:?}", files(&f.dir));
        assert!(!f.script.is_killed.load(Ordering::SeqCst));
        assert!(f.script.is_closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_missing_folder_fails_before_the_database_is_touched() {
        let f = fixture(Script::default());
        let mut command = query_command(&f, 5, None);
        if let Command::ExportQuery { path, .. } = &mut command {
            *path = f.dir.join("nope").join("out.csv").display().to_string();
        }
        f.engine.clone().dispatch(command).await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFailed { error, .. } => assert!(error.contains("nope"), "{error}"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(f.script.connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_statement_that_writes_is_refused_without_connecting() {
        let f = fixture(Script::default());
        let mut command = query_command(&f, 6, None);
        if let Command::ExportQuery { sql, .. } = &mut command {
            *sql = "UPDATE numbers SET n = 1".into();
        }
        f.engine.clone().dispatch(command).await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFailed { error, .. } => assert!(error.contains("reads"), "{error}"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(f.script.connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_second_run_with_other_columns_is_refused_and_leaves_no_file() {
        let f = fixture(Script {
            columns: vec!["other"],
            batches: vec![rows(1, 2)],
            ..Script::default()
        });
        f.engine.clone().dispatch(query_command(&f, 7, None)).await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFailed { error, .. } => {
                assert!(error.contains("different columns"), "{error}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(files(&f.dir).is_empty());
        assert!(f.script.is_closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_driver_that_cannot_stream_says_so() {
        let f = fixture(Script {
            columns: vec!["n"],
            is_unsupported: true,
            ..Script::default()
        });
        f.engine.clone().dispatch(query_command(&f, 8, None)).await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFailed { error, .. } => {
                assert!(error.contains("cannot export every row"), "{error}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(files(&f.dir).is_empty());
    }

    #[tokio::test]
    async fn a_limit_of_zero_is_refused() {
        let f = fixture(Script::default());
        f.engine
            .clone()
            .dispatch(query_command(&f, 9, Some(0)))
            .await;
        let seen = until_end(&f.events).await;
        match seen.last().unwrap() {
            Event::ExportFailed { error, .. } => assert!(error.contains("at least one"), "{error}"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn progress_is_throttled_however_fast_rows_arrive() {
        let f = fixture(Script {
            columns: vec!["n"],
            batches: (0..300).map(|b| rows(b * 100, 100)).collect(),
            ..Script::default()
        });
        f.engine.clone().dispatch(query_command(&f, 10, None)).await;
        let seen = until_end(&f.events).await;
        let progress = seen
            .iter()
            .filter(|e| matches!(e, Event::ExportProgress { .. }))
            .count();
        match seen.last().unwrap() {
            Event::ExportFinished { rows, .. } => assert_eq!(*rows, 30_000),
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            progress <= 3,
            "{progress} progress events for a fast export"
        );
    }

    #[tokio::test]
    async fn the_reader_waits_when_the_writer_is_behind() {
        let (tx, mut rx) = mpsc::channel::<Msg>(2);
        let (events, _kept) = async_channel::unbounded();
        let mut sink = ChannelSink {
            tx,
            control: Arc::new(ExportControl::default()),
            progress: Arc::new(Progress::default()),
            events,
            request: 1,
            limit: None,
            taken: 0,
            has_columns: true,
            last_progress: Instant::now(),
            stop: None,
        };
        assert!(sink.accept(ExecItem::Rows(rows(0, 1))).await);
        assert!(sink.accept(ExecItem::Rows(rows(1, 1))).await);
        // The queue holds two batches and nobody reads: the third waits.
        let third = tokio::time::timeout(
            Duration::from_millis(100),
            sink.accept(ExecItem::Rows(rows(2, 1))),
        )
        .await;
        assert!(third.is_err(), "a full queue must make the reader wait");
        // Taking one batch makes room, and the read goes on.
        assert!(rx.recv().await.is_some());
        assert!(sink.accept(ExecItem::Rows(rows(2, 1))).await);
    }

    #[test]
    fn a_queue_closed_without_done_commits_nothing() {
        let dir = std::env::temp_dir().join(format!("quarry-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = fixture(Script::default());
        let target = dir.join("out.csv");
        let progress = Arc::new(Progress::default());
        let counted = Counted {
            output: Output::create(&target).unwrap(),
            progress: progress.clone(),
        };
        let (tx, rx) = mpsc::channel::<Msg>(4);
        tx.blocking_send(Msg::Columns(vec![ColumnMeta {
            name: "n".into(),
            type_name: "INT".into(),
            nullable: None,
            source_database: None,
            source_table: None,
            source_column: None,
        }]))
        .unwrap();
        tx.blocking_send(Msg::Rows(rows(1, 3))).unwrap();
        drop(tx);
        let input = WriterInput {
            conn: f.conn,
            format: ExportFormat::Csv,
            options: ExportOptions::default(),
            columns: None,
            expected: None,
        };
        let end = write_stream(&f.engine, &input, counted, rx, &progress);
        assert!(matches!(end, WriteEnd::Aborted));
        assert!(!target.exists());
        assert!(!dir.join("out.csv.part").exists());
        assert!(
            progress.bytes.load(Ordering::Relaxed) > 0,
            "bytes were counted"
        );
        assert_eq!(progress.rows.load(Ordering::Relaxed), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
