//! Imports of delimited files into a table. The file is read on a blocking thread and handed
//! over in batches through a small queue, so memory holds a few batches whatever the file's size.
//! Every row goes in through one transaction on a connection of its own: a bad row, a cancel or
//! a lost connection undoes the whole import, and the table is never left half filled.

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quarry_driver_api::Session;
use quarry_import::{ImportError, Options, Reader};
use quarry_model::{ConnId, ImportOptions, OnDuplicate, TableRef, Value};
use tokio::sync::mpsc;

use crate::engine::{Engine, describe, lock};
use crate::export_job::ExportControl;
use crate::query::{first_value, row_count_of, run_plain};
use crate::{Event, RequestId};

/// Rows the preview shows.
const PREVIEW_ROWS: usize = 20;
/// Rows read from the file at a time.
const BATCH_ROWS: usize = 1000;
/// Batches waiting between the file and the database. Small on purpose: the reader waits.
const QUEUE_BATCHES: usize = 2;
/// Parameters in one statement. SQL Server allows 2100; the others allow more.
const MAX_PARAMS: usize = 2000;
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Savepoints run as plain text: MySQL does not accept them as prepared statements.
/// Each batch runs after a savepoint, so a refused batch can be taken back and its rows tried one
/// by one to find the one that was refused, without undoing the batches before it.
const SAVEPOINT: &str = "SAVEPOINT quarry_import_batch";
const ROLLBACK_TO: &str = "ROLLBACK TO SAVEPOINT quarry_import_batch";
const RELEASE: &str = "RELEASE SAVEPOINT quarry_import_batch";

const STOPPED: &str = "The import stopped unexpectedly. Nothing was imported. Try again.";

type Batch = Vec<Vec<String>>;

enum End {
    /// Rows written, and rows left out because their key already existed.
    Finished {
        written: u64,
        skipped: u64,
    },
    Cancelled,
    Failed(String),
}

fn reader_options(options: &ImportOptions) -> Options {
    Options {
        delimiter: options.delimiter,
        has_header: options.has_header,
    }
}

fn open_reader(path: &str, options: &ImportOptions) -> Result<Reader<BufReader<File>>, String> {
    let file = File::open(path).map_err(|err| {
        format!("Couldn't open the file: {err}. Check that it exists and that you can read it.")
    })?;
    Reader::new(BufReader::new(file), reader_options(options)).map_err(|err| err.to_string())
}

impl Engine {
    pub(crate) async fn import_preview(
        &self,
        request: RequestId,
        path: String,
        options: ImportOptions,
    ) {
        let read = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let mut reader = open_reader(&path, &options)?;
            let mut rows = Vec::new();
            while rows.len() < PREVIEW_ROWS {
                match reader.next_record() {
                    Ok(Some(row)) => rows.push(row),
                    Ok(None) => break,
                    Err(err) => return Err(err.to_string()),
                }
            }
            let header = reader.header().map(<[String]>::to_vec).unwrap_or_default();
            let width = reader.width().unwrap_or(0);
            Ok((header, rows, width))
        })
        .await;
        let event = match read {
            Ok(Ok((header, rows, width))) => Event::ImportPreview {
                request,
                header,
                rows,
                width: u32::try_from(width).unwrap_or(u32::MAX),
                error: None,
            },
            Ok(Err(error)) => preview_error(request, error),
            Err(_) => preview_error(request, STOPPED.to_owned()),
        };
        self.emit(event);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn import_run(
        self: Arc<Self>,
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        columns: Vec<Option<String>>,
        path: String,
        options: ImportOptions,
        confirmed: bool,
    ) {
        let failed = |error: String| Event::ImportFailed {
            request,
            error,
            cancelled: false,
        };
        let Some(config) = self.config_of(conn) else {
            return self.emit(failed("Connection not found".to_owned()));
        };
        if config.read_only {
            return self.emit(failed("This connection is read-only.".to_owned()));
        }
        if let Err(error) = check_mapping(&columns) {
            return self.emit(failed(error));
        }
        if path.trim().is_empty() {
            return self.emit(failed("Choose a file to import.".to_owned()));
        }
        // Overwriting changes rows that are already there, so it asks in every environment.
        let is_overwrite = options.on_duplicate == OnDuplicate::Update;
        let is_production = config.environment.confirms_every_write();
        if !confirmed && (is_production || is_overwrite) {
            let mut message = Vec::new();
            if is_production {
                message.push("This inserts rows into a production database.");
            }
            if is_overwrite {
                message.push(
                    "Rows whose key is already in the table will be overwritten with the \
                     file's values. The old values cannot be brought back.",
                );
            }
            return self.emit(Event::ImportConfirmRequired {
                request,
                message: message.join(" "),
            });
        }

        let control = self.register_export(request);
        let end = self
            .run_import(request, conn, &table, &columns, &path, &options, &control)
            .await;
        lock(&self.exports).remove(&request);
        self.emit(match end {
            End::Finished { written, skipped } => Event::ImportFinished {
                request,
                rows: written,
                skipped,
            },
            End::Cancelled => Event::ImportFailed {
                request,
                error: "The import was cancelled. No row was imported.".to_owned(),
                cancelled: true,
            },
            End::Failed(error) => failed(error),
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_import(
        &self,
        request: RequestId,
        conn: ConnId,
        table: &TableRef,
        columns: &[Option<String>],
        path: &str,
        options: &ImportOptions,
        control: &Arc<ExportControl>,
    ) -> End {
        let Some(driver) = self
            .config_of(conn)
            .and_then(|config| self.driver(&config.driver))
        else {
            return End::Failed("Connection not found".to_owned());
        };
        // The file first: a wrong path or format is found before any work on the database.
        let reader = match open_reader(path, options) {
            Ok(reader) => reader,
            Err(error) => return End::Failed(error),
        };
        let width = match reader.width() {
            Some(width) => Some(width),
            // Without a header the width is the first record's, read on a reader of its own so
            // the one that imports still starts at the first row.
            None => open_reader(path, options)
                .ok()
                .and_then(|mut probe| first_width(&mut probe)),
        };
        let Some(width) = width else {
            return End::Failed("The file has no rows to import.".to_owned());
        };
        if width != columns.len() {
            return End::Failed(format!(
                "The file has {width} columns but {} were mapped. Reopen the import to map them again.",
                columns.len()
            ));
        }

        let mut session = match self
            .open_export_session(conn, table.database.as_deref(), control, false)
            .await
        {
            Ok(session) => session,
            Err(_) if control.is_cancelled() => return End::Cancelled,
            Err(error) => return End::Failed(error),
        };
        // A value the table cannot hold must fail the import, not be stored changed.
        if let Err(err) = session.set_strict().await {
            let _ = session.close().await;
            return End::Failed(describe(&err));
        }
        let end = self
            .insert_all(
                &mut *session,
                driver.dialect(),
                request,
                table,
                columns,
                options,
                control,
                reader,
            )
            .await;
        if !matches!(end, End::Finished { .. })
            && let Err(err) = session.rollback().await
        {
            tracing::warn!(%err, "rollback after failed import");
        }
        let _ = session.close().await;
        end
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_all(
        &self,
        session: &mut dyn Session,
        dialect: &dyn quarry_driver_api::Dialect,
        request: RequestId,
        table: &TableRef,
        columns: &[Option<String>],
        options: &ImportOptions,
        control: &Arc<ExportControl>,
        reader: Reader<BufReader<File>>,
    ) -> End {
        if let Ok(info) = session.table_info(table).await {
            let unknown = columns.iter().flatten().find(|name| {
                !info
                    .columns
                    .iter()
                    .any(|column| column.name.eq_ignore_ascii_case(name))
            });
            if let Some(name) = unknown {
                return End::Failed(format!(
                    "The table has no column \"{name}\". Choose another column and try again."
                ));
            }
        }
        let mapped: Vec<usize> = columns
            .iter()
            .enumerate()
            .filter_map(|(index, name)| name.as_ref().map(|_| index))
            .collect();
        let names: Vec<String> = columns
            .iter()
            .flatten()
            .map(|name| dialect.quote_ident(name))
            .collect();
        let per_statement = (MAX_PARAMS / mapped.len()).clamp(1, BATCH_ROWS);
        let tail = if options.on_duplicate == OnDuplicate::Fail {
            String::new()
        } else {
            let key = session.primary_key(table).await.unwrap_or_default();
            let named: Vec<String> = columns.iter().flatten().cloned().collect();
            match dialect.on_duplicate(options.on_duplicate, &named, &key) {
                Some(tail) => tail,
                None => {
                    return End::Failed(
                        "This database cannot skip or overwrite rows that already exist, or the \
                         table has no primary key to match them by. Choose \"Stop and keep \
                         nothing\" and try again."
                            .to_owned(),
                    );
                }
            }
        };
        let plan = Plan {
            dialect,
            head: format!(
                "INSERT INTO {} ({}) VALUES ",
                dialect.qualify(table),
                names.join(", ")
            ),
            tail,
            mapped: &mapped,
            options,
        };

        // Only a skip needs to know how many rows the table had: it counts the ones left out.
        let rows_before = if options.on_duplicate == OnDuplicate::Skip {
            match count_rows(session, dialect, table).await {
                Ok(count) => Some(count),
                Err(error) => return End::Failed(error),
            }
        } else {
            None
        };

        let (tx, mut rx) = mpsc::channel::<Result<Batch, String>>(QUEUE_BATCHES);
        let reading = tokio::task::spawn_blocking(move || read_batches(reader, &tx));

        let mut inserted: u64 = 0;
        let mut last_progress = Instant::now();
        // A database without these savepoints still imports; it only reports a range of rows.
        let mut can_pinpoint = true;
        let end = 'batches: loop {
            let batch = match rx.recv().await {
                Some(Ok(batch)) => batch,
                Some(Err(error)) => break End::Failed(format!("{error} Nothing was imported.")),
                None => {
                    break End::Finished {
                        written: inserted,
                        skipped: 0,
                    };
                }
            };
            for chunk in batch.chunks(per_statement) {
                if control.is_cancelled() {
                    break 'batches End::Cancelled;
                }
                if can_pinpoint && run_plain(&mut *session, SAVEPOINT).await.is_err() {
                    can_pinpoint = false;
                }
                let (sql, params) = plan.statement(chunk);
                if let Err(err) = session.execute_params(&sql, &params).await {
                    if control.is_cancelled() {
                        break 'batches End::Cancelled;
                    }
                    let first_row = inserted + 1;
                    let exact = if can_pinpoint {
                        find_refused_row(session, &plan, chunk, first_row).await
                    } else {
                        None
                    };
                    break 'batches End::Failed(exact.unwrap_or_else(|| {
                        format!(
                            "Rows {first_row}-{} of the file were refused: {}\nNothing was imported.",
                            inserted + chunk.len() as u64,
                            describe(&err)
                        )
                    }));
                }
                if can_pinpoint {
                    let _ = run_plain(&mut *session, RELEASE).await;
                }
                inserted += chunk.len() as u64;
            }
            if last_progress.elapsed() >= PROGRESS_EVERY {
                last_progress = Instant::now();
                let _ = self.events.try_send(Event::ImportProgress {
                    request,
                    rows: inserted,
                });
            }
        };
        // A closed queue stops the reader, which may be waiting to send.
        drop(rx);
        let _ = reading.await;
        let End::Finished { written, .. } = end else {
            return end;
        };
        // The rows the table gained say how many were left out: what a database reports as
        // affected by a duplicate differs (MySQL counts a matched row), so it cannot tell.
        let (written, skipped) = match rows_before {
            Some(before) => match count_rows(session, dialect, table).await {
                Ok(after) => {
                    let gained = after.saturating_sub(before).min(written);
                    (gained, written - gained)
                }
                Err(error) => return End::Failed(format!("{error}\nNothing was imported.")),
            },
            None => (written, 0),
        };
        match session.commit().await {
            Ok(()) => End::Finished { written, skipped },
            Err(err) => End::Failed(format!("{}\nNothing was imported.", describe(&err))),
        }
    }
}

async fn count_rows(
    session: &mut dyn Session,
    dialect: &dyn quarry_driver_api::Dialect,
    table: &TableRef,
) -> Result<u64, String> {
    let sql = dialect.count_rows(table, None);
    let first = first_value(session, &sql, &[])
        .await
        .map_err(|err| describe(&err))?;
    row_count_of(first)
}

/// Take back the refused batch and insert its rows one at a time until the database refuses one.
/// Rows count from 1, without the header. `None` when every row went in alone: the batch failed
/// for a reason that is not one row's, and the caller reports the range.
async fn find_refused_row(
    session: &mut dyn Session,
    plan: &Plan<'_>,
    chunk: &[Vec<String>],
    first_row: u64,
) -> Option<String> {
    run_plain(&mut *session, ROLLBACK_TO).await.ok()?;
    for (offset, row) in chunk.iter().enumerate() {
        let (sql, params) = plan.statement(std::slice::from_ref(row));
        if let Err(err) = session.execute_params(&sql, &params).await {
            return Some(format!(
                "Row {} of the file (not counting the header) was refused: {}\nNothing was imported.",
                first_row + offset as u64,
                describe(&err)
            ));
        }
    }
    None
}

fn preview_error(request: RequestId, error: String) -> Event {
    Event::ImportPreview {
        request,
        header: Vec::new(),
        rows: Vec::new(),
        width: 0,
        error: Some(error),
    }
}

/// At least one column goes somewhere, and none goes twice.
fn check_mapping(columns: &[Option<String>]) -> Result<(), String> {
    let mut seen: Vec<String> = Vec::new();
    for name in columns.iter().flatten() {
        let lower = name.to_lowercase();
        if seen.contains(&lower) {
            return Err(format!(
                "The column \"{name}\" is mapped twice. Map each table column once."
            ));
        }
        seen.push(lower);
    }
    if seen.is_empty() {
        return Err("Map at least one column to import.".to_owned());
    }
    Ok(())
}

/// The width of a file without a header: the number of values in its first record.
fn first_width(reader: &mut Reader<BufReader<File>>) -> Option<usize> {
    reader.next_record().ok().flatten().map(|row| row.len())
}

fn read_batches(mut reader: Reader<BufReader<File>>, tx: &mpsc::Sender<Result<Batch, String>>) {
    let mut batch: Batch = Vec::with_capacity(BATCH_ROWS);
    loop {
        match reader.next_record() {
            Ok(Some(row)) => {
                batch.push(row);
                if batch.len() == BATCH_ROWS
                    && tx.blocking_send(Ok(std::mem::take(&mut batch))).is_err()
                {
                    return;
                }
            }
            Ok(None) => {
                if !batch.is_empty() {
                    let _ = tx.blocking_send(Ok(batch));
                }
                return;
            }
            Err(err) => {
                let _ = tx.blocking_send(Err(describe_file_error(&err)));
                return;
            }
        }
    }
}

fn describe_file_error(err: &ImportError) -> String {
    err.to_string()
}

/// What every `INSERT` of one import has in common.
struct Plan<'a> {
    dialect: &'a dyn quarry_driver_api::Dialect,
    /// `INSERT INTO table (columns) VALUES `
    head: String,
    /// What follows the values: how a key that exists is met. Empty to stop at one.
    tail: String,
    /// The file columns that go into the table, in the order of the insert's columns.
    mapped: &'a [usize],
    options: &'a ImportOptions,
}

impl Plan<'_> {
    fn statement(&self, rows: &[Vec<String>]) -> (String, Vec<Value>) {
        let mut params = Vec::with_capacity(rows.len() * self.mapped.len());
        let mut groups = Vec::with_capacity(rows.len());
        for row in rows {
            let mut marks = Vec::with_capacity(self.mapped.len());
            for &index in self.mapped {
                params.push(match row.get(index) {
                    Some(text) if !(self.options.is_empty_null && text.is_empty()) => {
                        Value::Text(text.clone())
                    }
                    _ => Value::Null,
                });
                marks.push(self.dialect.placeholder(params.len()));
            }
            groups.push(format!("({})", marks.join(", ")));
        }
        (
            format!("{}{}{}", self.head, groups.join(", "), self.tail),
            params,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use async_trait::async_trait;
    use quarry_driver_api::{
        CancelHandle, Driver, DriverError, ExecOpts, ExecSink, ResolvedConfig,
        Result as DriverResult, SharedCancel, SpecDialect,
    };
    use quarry_model::{
        Capabilities, ConnectionConfig, DriverId, Environment, SchemaObject, SslMode,
    };

    use super::*;
    use crate::edit::tests::ansi_spec;
    use crate::{Command, EngineOptions, InProcessDrivers};

    #[derive(Default)]
    struct Script {
        /// The inserts the session ran, with their parameters. Savepoints are not listed.
        statements: Mutex<Vec<(String, Vec<Value>)>>,
        /// The database refuses an insert that holds this value.
        refuse: Option<&'static str>,
        connects: AtomicUsize,
        is_committed: AtomicBool,
        is_rolled_back: AtomicBool,
        is_strict: AtomicBool,
        /// How this database meets a key that exists.
        syntax: quarry_model::DuplicateSyntax,
        /// An insert row holding this value is a duplicate: the database takes no row for it.
        duplicate: Option<&'static str>,
    }

    use std::sync::Mutex;

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

    struct NoKill;

    impl CancelHandle for NoKill {
        fn cancel(&self) {}
    }

    #[async_trait]
    impl Session for FakeSession {
        fn server_version(&self) -> String {
            "fake".into()
        }
        async fn execute(
            &mut self,
            sql: &str,
            _: &ExecOpts,
            sink: ExecSink<'_>,
        ) -> DriverResult<()> {
            if sql.starts_with("SELECT COUNT") {
                // The rows the table took: every insert value that is not a duplicate. The tests
                // that count use one column.
                let taken: usize = lock(&self.script.statements)
                    .iter()
                    .flat_map(|(_, params)| params)
                    .filter(|p| {
                        self.script
                            .duplicate
                            .is_none_or(|d| **p != Value::Text(d.into()))
                    })
                    .count();
                sink(quarry_driver_api::ExecItem::Rows(vec![vec![Value::Int(
                    taken as i64,
                )]]));
            }
            Ok(())
        }
        async fn execute_params(&mut self, sql: &str, params: &[Value]) -> DriverResult<u64> {
            if !sql.starts_with("INSERT") {
                return Ok(0);
            }
            if let Some(bad) = self.script.refuse
                && params.contains(&Value::Text(bad.into()))
            {
                return Err(DriverError::Query {
                    message: "Incorrect integer value".into(),
                    location: None,
                });
            }
            lock(&self.script.statements).push((sql.to_owned(), params.to_vec()));
            let taken = params
                .iter()
                .filter(|p| {
                    self.script
                        .duplicate
                        .is_none_or(|d| **p != Value::Text(d.into()))
                })
                .count();
            Ok(taken as u64)
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
            self.script.is_committed.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn rollback(&mut self) -> DriverResult<()> {
            self.script.is_rolled_back.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn set_read_only(&mut self, _: bool) -> DriverResult<()> {
            Ok(())
        }
        async fn set_strict(&mut self) -> DriverResult<()> {
            self.script.is_strict.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn cancel_handle(&self) -> SharedCancel {
            Arc::new(NoKill)
        }
        async fn close(self: Box<Self>) -> DriverResult<()> {
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

    fn fixture(script: Script, environment: Environment) -> Fixture {
        let dir = std::env::temp_dir().join(format!("quarry-import-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, events) = async_channel::unbounded();
        let script = Arc::new(script);
        let engine = Engine::new(
            EngineOptions {
                driver_dir: Some(dir.clone()),
                in_process_drivers: InProcessDrivers(vec![Arc::new(FakeDriver {
                    script: script.clone(),
                    dialect: SpecDialect::new(quarry_model::DialectSpec {
                        on_duplicate: script.syntax,
                        ..ansi_spec().spec().clone()
                    }),
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
            environment,
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

    fn table() -> TableRef {
        TableRef {
            database: None,
            schema: None,
            name: "people".into(),
        }
    }

    fn write_file(f: &Fixture, text: &str) -> String {
        let path = f.dir.join("people.csv");
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    }

    fn names(columns: &[Option<&str>]) -> Vec<Option<String>> {
        columns.iter().map(|c| c.map(str::to_owned)).collect()
    }

    fn run(f: &Fixture, path: String, columns: Vec<Option<String>>, confirmed: bool) -> Command {
        Command::ImportRun {
            request: 1,
            conn: f.conn,
            table: table(),
            columns,
            path,
            options: ImportOptions::default(),
            confirmed,
        }
    }

    async fn until_end(events: &async_channel::Receiver<Event>) -> Event {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(20), events.recv())
                .await
                .expect("the import ends")
                .unwrap();
            if matches!(
                event,
                Event::ImportFinished { .. }
                    | Event::ImportFailed { .. }
                    | Event::ImportConfirmRequired { .. }
            ) {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn every_row_goes_in_one_transaction_that_commits() {
        let f = fixture(Script::default(), Environment::Local);
        let path = write_file(&f, "id,name\n1,Ann\n2,\n3,Cy\n");
        let command = run(&f, path, names(&[Some("id"), Some("name")]), false);
        f.engine.clone().dispatch(command).await;
        assert!(matches!(
            until_end(&f.events).await,
            Event::ImportFinished { rows: 3, .. }
        ));
        let statements = lock(&f.script.statements);
        assert_eq!(statements.len(), 1, "three rows fit one statement");
        let (sql, params) = &statements[0];
        assert!(sql.starts_with("INSERT INTO \"people\" (\"id\", \"name\") VALUES "));
        assert_eq!(params.len(), 6);
        assert_eq!(params[0], Value::Text("1".into()));
        assert_eq!(params[3], Value::Null, "an empty value is NULL");
        assert!(f.script.is_committed.load(Ordering::SeqCst));
        assert!(!f.script.is_rolled_back.load(Ordering::SeqCst));
        assert!(
            f.script.is_strict.load(Ordering::SeqCst),
            "the session must refuse values it cannot store exactly"
        );
        assert!(lock(&f.engine.exports).is_empty());
    }

    #[tokio::test]
    async fn an_unmapped_column_is_left_out() {
        let f = fixture(Script::default(), Environment::Local);
        let path = write_file(&f, "id,junk,name\n1,x,Ann\n");
        let command = run(&f, path, names(&[Some("id"), None, Some("name")]), false);
        f.engine.clone().dispatch(command).await;
        assert!(matches!(
            until_end(&f.events).await,
            Event::ImportFinished { rows: 1, .. }
        ));
        let statements = lock(&f.script.statements);
        assert_eq!(statements[0].1.len(), 2);
        assert!(!statements[0].0.contains("junk"));
    }

    #[tokio::test]
    async fn a_refused_row_undoes_the_whole_import() {
        let f = fixture(
            Script {
                refuse: Some("abc"),
                ..Script::default()
            },
            Environment::Local,
        );
        let path = write_file(&f, "id\n1\n2\nabc\n4\n");
        let command = run(&f, path, names(&[Some("id")]), false);
        f.engine.clone().dispatch(command).await;
        match until_end(&f.events).await {
            Event::ImportFailed {
                error, cancelled, ..
            } => {
                assert!(!cancelled);
                assert!(error.contains("Row 3 "), "names the refused row: {error}");
                assert!(error.contains("Nothing was imported"), "{error}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(f.script.is_rolled_back.load(Ordering::SeqCst));
        assert!(!f.script.is_committed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn production_asks_before_it_connects() {
        let f = fixture(Script::default(), Environment::Production);
        let path = write_file(&f, "id\n1\n");
        let command = run(&f, path.clone(), names(&[Some("id")]), false);
        f.engine.clone().dispatch(command).await;
        assert!(matches!(
            until_end(&f.events).await,
            Event::ImportConfirmRequired { .. }
        ));
        assert_eq!(f.script.connects.load(Ordering::SeqCst), 0);

        let command = run(&f, path, names(&[Some("id")]), true);
        f.engine.clone().dispatch(command).await;
        assert!(matches!(
            until_end(&f.events).await,
            Event::ImportFinished { rows: 1, .. }
        ));
    }

    fn with_mode(mut command: Command, mode: OnDuplicate) -> Command {
        if let Command::ImportRun { options, .. } = &mut command {
            options.on_duplicate = mode;
        }
        command
    }

    #[tokio::test]
    async fn skipping_leaves_the_existing_rows_and_counts_them() {
        let f = fixture(
            Script {
                syntax: quarry_model::DuplicateSyntax::OnConflict,
                duplicate: Some("2"),
                ..Script::default()
            },
            Environment::Local,
        );
        let path = write_file(&f, "id\n1\n2\n3\n");
        let command = with_mode(
            run(&f, path, names(&[Some("id")]), false),
            OnDuplicate::Skip,
        );
        f.engine.clone().dispatch(command).await;
        assert!(matches!(
            until_end(&f.events).await,
            Event::ImportFinished {
                rows: 2,
                skipped: 1,
                ..
            }
        ));
        let statements = lock(&f.script.statements);
        assert!(statements[0].0.ends_with(" ON CONFLICT DO NOTHING"));
    }

    #[tokio::test]
    async fn overwriting_asks_first_even_on_a_local_connection() {
        let f = fixture(
            Script {
                syntax: quarry_model::DuplicateSyntax::OnDuplicateKey,
                ..Script::default()
            },
            Environment::Local,
        );
        let path = write_file(&f, "id,name\n1,Ann\n");
        let columns = names(&[Some("id"), Some("name")]);
        let command = with_mode(
            run(&f, path.clone(), columns.clone(), false),
            OnDuplicate::Update,
        );
        f.engine.clone().dispatch(command).await;
        match until_end(&f.events).await {
            Event::ImportConfirmRequired { message, .. } => {
                assert!(message.contains("overwritten"), "{message}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(f.script.connects.load(Ordering::SeqCst), 0);

        let command = with_mode(run(&f, path, columns, true), OnDuplicate::Update);
        f.engine.clone().dispatch(command).await;
        assert!(matches!(
            until_end(&f.events).await,
            Event::ImportFinished { rows: 1, .. }
        ));
        let statements = lock(&f.script.statements);
        assert!(statements[0].0.contains("ON DUPLICATE KEY UPDATE"));
    }

    #[tokio::test]
    async fn a_database_without_a_duplicate_form_says_so_and_changes_nothing() {
        let f = fixture(Script::default(), Environment::Local);
        let path = write_file(&f, "id\n1\n");
        let command = with_mode(
            run(&f, path, names(&[Some("id")]), false),
            OnDuplicate::Skip,
        );
        f.engine.clone().dispatch(command).await;
        match until_end(&f.events).await {
            Event::ImportFailed { error, .. } => {
                assert!(error.contains("cannot skip or overwrite"), "{error}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(f.script.is_rolled_back.load(Ordering::SeqCst));
        assert!(lock(&f.script.statements).is_empty());
    }

    #[tokio::test]
    async fn a_file_without_header_imports_its_first_row() {
        let f = fixture(Script::default(), Environment::Local);
        let path = write_file(&f, "1,Ann\n2,Bob\n");
        let mut command = run(&f, path, names(&[Some("id"), Some("name")]), false);
        if let Command::ImportRun { options, .. } = &mut command {
            options.has_header = false;
        }
        f.engine.clone().dispatch(command).await;
        assert!(matches!(
            until_end(&f.events).await,
            Event::ImportFinished { rows: 2, .. }
        ));
    }

    #[tokio::test]
    async fn preview_shows_the_header_and_the_first_rows() {
        let f = fixture(Script::default(), Environment::Local);
        let path = write_file(&f, "id,name\n1,Ann\n");
        f.engine
            .clone()
            .dispatch(Command::ImportPreview {
                request: 1,
                path,
                options: ImportOptions::default(),
            })
            .await;
        match f.events.recv().await.unwrap() {
            Event::ImportPreview {
                header,
                rows,
                width,
                error,
                ..
            } => {
                assert_eq!(error, None);
                assert_eq!(header, ["id", "name"]);
                assert_eq!(rows, [["1", "Ann"]]);
                assert_eq!(width, 2);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(f.script.connects.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_mapping_needs_a_column_and_no_repeats() {
        assert!(check_mapping(&names(&[None, None])).is_err());
        assert!(check_mapping(&names(&[Some("a"), Some("A")])).is_err());
        assert!(check_mapping(&names(&[Some("a"), None, Some("b")])).is_ok());
    }
}
