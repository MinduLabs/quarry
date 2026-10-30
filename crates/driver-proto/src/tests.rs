use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use quarry_driver_api::{
    CancelHandle, Driver, DriverError, ExecItem, ExecOpts, ExecSink, ResolvedConfig, Result,
    RowSink, Session, SharedCancel, SpecDialect,
};
use quarry_model::{
    Capabilities, ColumnMeta, ConnId, ConnectionConfig, DialectSpec, DriverId, Environment,
    IdentQuote, LiteralStyle, PagingStyle, PlaceholderStyle, SchemaObject, SslMode, Value,
};

use crate::client::connect_over;
use crate::serve;

struct Fake {
    dialect: SpecDialect,
    cancelled: Arc<AtomicBool>,
    /// Row batches its streams have handed to the sink so far.
    produced: Arc<AtomicUsize>,
}

impl Fake {
    fn new() -> Self {
        Self {
            dialect: SpecDialect::new(DialectSpec {
                quote: IdentQuote::Double,
                on_duplicate: quarry_model::DuplicateSyntax::None,
                placeholder: PlaceholderStyle::Numbered,
                paging: PagingStyle::LimitOffset,
                literals: LiteralStyle {
                    backslash_escapes: false,
                    bool_as_int: false,
                    national_strings: false,
                    hex_bytes: false,
                },
                explain: None,
            }),
            cancelled: Arc::default(),
            produced: Arc::default(),
        }
    }
}

struct FakeCancel(Arc<AtomicBool>);

impl CancelHandle for FakeCancel {
    fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct FakeSession {
    cancelled: Arc<AtomicBool>,
    produced: Arc<AtomicUsize>,
}

/// Batches of one row each that a fake `stream` sends: more than the window, so pacing shows.
const STREAM_BATCHES: i64 = 20;

#[async_trait]
impl Driver for Fake {
    fn id(&self) -> DriverId {
        DriverId("fake".into())
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn dialect(&self) -> &SpecDialect {
        &self.dialect
    }
    async fn connect(&self, config: &ResolvedConfig) -> Result<Box<dyn Session>> {
        if config.password.as_deref() != Some("right") {
            return Err(DriverError::Auth("Access denied".into()));
        }
        Ok(Box::new(FakeSession {
            cancelled: self.cancelled.clone(),
            produced: self.produced.clone(),
        }))
    }
}

#[async_trait]
impl Session for FakeSession {
    fn server_version(&self) -> String {
        "1.2.3".into()
    }
    async fn execute(&mut self, sql: &str, _: &ExecOpts, sink: ExecSink<'_>) -> Result<()> {
        match sql {
            "slow" => {
                while !self.cancelled.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Err(DriverError::Cancelled)
            }
            "bad" => Err(DriverError::Query {
                message: "syntax".into(),
                location: None,
            }),
            _ => {
                sink(ExecItem::Columns(vec![ColumnMeta {
                    name: "n".into(),
                    type_name: "INT".into(),
                    nullable: None,
                    source_database: None,
                    source_table: None,
                    source_column: None,
                }]));
                sink(ExecItem::Rows(vec![
                    vec![Value::Int(1)],
                    vec![Value::Int(2)],
                ]));
                sink(ExecItem::SetDone {
                    affected: 0,
                    rows: 2,
                    truncated: false,
                });
                Ok(())
            }
        }
    }
    async fn execute_params(&mut self, _: &str, params: &[Value]) -> Result<u64> {
        Ok(params.len() as u64)
    }
    async fn stream(
        &mut self,
        _: &str,
        _: &[Value],
        _: &ExecOpts,
        sink: &mut dyn RowSink,
    ) -> Result<()> {
        if !sink.accept(ExecItem::Columns(Vec::new())).await {
            return Ok(());
        }
        for n in 0..STREAM_BATCHES {
            self.produced.fetch_add(1, Ordering::SeqCst);
            if !sink.accept(ExecItem::Rows(vec![vec![Value::Int(n)]])).await {
                return Ok(());
            }
        }
        sink.accept(ExecItem::SetDone {
            affected: 0,
            rows: STREAM_BATCHES as u64,
            truncated: false,
        })
        .await;
        Ok(())
    }
    async fn list_databases(&mut self) -> Result<Vec<String>> {
        Ok(vec!["shop".into()])
    }
    async fn list_objects(&mut self, _: &str) -> Result<Vec<SchemaObject>> {
        Err(DriverError::Unsupported("objects"))
    }
    async fn use_database(&mut self, _: &str) -> Result<()> {
        Ok(())
    }
    async fn begin(&mut self) -> Result<()> {
        Ok(())
    }
    async fn commit(&mut self) -> Result<()> {
        Ok(())
    }
    async fn rollback(&mut self) -> Result<()> {
        Ok(())
    }
    async fn set_read_only(&mut self, _: bool) -> Result<()> {
        Ok(())
    }
    fn cancel_handle(&self) -> SharedCancel {
        Arc::new(FakeCancel(self.cancelled.clone()))
    }
    async fn close(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

fn config(password: &str) -> ResolvedConfig {
    ResolvedConfig {
        config: ConnectionConfig {
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
        },
        password: Some(password.into()),
    }
}

async fn connect(password: &str) -> Result<Box<dyn Session>> {
    connect_to(Arc::new(Fake::new()), password).await
}

async fn connect_to(fake: Arc<Fake>, password: &str) -> Result<Box<dyn Session>> {
    let (host_out, driver_in) = tokio::io::duplex(64 * 1024);
    let (driver_out, host_in) = tokio::io::duplex(64 * 1024);
    tokio::spawn(serve(fake, driver_in, driver_out));
    connect_over(host_in, host_out, &config(password)).await
}

/// Keeps the rows a stream sends; slow, so the driver must wait for it, and done after
/// `stop_after` batches when set.
struct Collect {
    produced: Arc<AtomicUsize>,
    rows: Vec<i64>,
    /// The most batches the driver was ever ahead of this sink.
    ahead: usize,
    stop_after: Option<usize>,
    is_done: bool,
}

#[async_trait]
impl RowSink for Collect {
    async fn accept(&mut self, item: ExecItem) -> bool {
        match item {
            ExecItem::Batch(batch) => {
                tokio::time::sleep(Duration::from_millis(2)).await;
                for row in batch.decode().unwrap() {
                    let [Value::Int(n)] = row.as_slice() else {
                        panic!("one int per row");
                    };
                    self.rows.push(*n);
                }
                let produced = self.produced.load(Ordering::SeqCst);
                self.ahead = self.ahead.max(produced - self.rows.len());
                self.stop_after.is_none_or(|stop| self.rows.len() < stop)
            }
            ExecItem::SetDone { .. } => {
                self.is_done = true;
                true
            }
            _ => true,
        }
    }
}

fn collect(fake: &Fake, stop_after: Option<usize>) -> Collect {
    Collect {
        produced: fake.produced.clone(),
        rows: Vec::new(),
        ahead: 0,
        stop_after,
        is_done: false,
    }
}

#[tokio::test]
async fn a_stream_sends_every_row_no_faster_than_the_host_takes_them() {
    let fake = Arc::new(Fake::new());
    let mut sink = collect(&fake, None);
    let mut session = connect_to(fake.clone(), "right").await.unwrap();
    session
        .stream("SELECT", &[], &ExecOpts::default(), &mut sink)
        .await
        .unwrap();
    assert_eq!(sink.rows, (0..STREAM_BATCHES).collect::<Vec<_>>());
    assert!(sink.is_done);
    // The window, plus the batch the driver produced before it waited.
    assert!(
        sink.ahead <= crate::wire::STREAM_WINDOW + 1,
        "{} ahead",
        sink.ahead
    );
}

#[tokio::test]
async fn a_stopped_stream_ends_early_and_the_session_goes_on() {
    let fake = Arc::new(Fake::new());
    let mut sink = collect(&fake, Some(3));
    let mut session = connect_to(fake.clone(), "right").await.unwrap();
    session
        .stream("SELECT", &[], &ExecOpts::default(), &mut sink)
        .await
        .unwrap();
    assert_eq!(sink.rows, vec![0, 1, 2]);
    assert!(!sink.is_done);
    let produced = fake.produced.load(Ordering::SeqCst);
    assert!(produced < STREAM_BATCHES as usize, "{produced} produced");
    assert_eq!(session.list_databases().await.unwrap(), vec!["shop"]);
}

#[tokio::test]
async fn a_session_streams_results_and_answers_calls() {
    let mut session = connect("right").await.unwrap();
    assert_eq!(session.server_version(), "1.2.3");
    let mut items = Vec::new();
    session
        .execute("SELECT", &ExecOpts::default(), &mut |item| items.push(item))
        .await
        .unwrap();
    assert!(matches!(items[0], ExecItem::Columns(_)));
    let ExecItem::Batch(batch) = &items[1] else {
        panic!("rows expected");
    };
    assert_eq!(
        batch.decode().unwrap(),
        vec![vec![Value::Int(1)], vec![Value::Int(2)]]
    );
    assert!(matches!(items[2], ExecItem::SetDone { rows: 2, .. }));
    assert_eq!(
        session
            .execute_params("UPDATE", &[Value::Null, Value::Null])
            .await
            .unwrap(),
        2
    );
    assert_eq!(session.list_databases().await.unwrap(), vec!["shop"]);
    session.close().await.unwrap();
}

#[tokio::test]
async fn errors_keep_their_kind() {
    let err = connect("wrong").await.err().expect("auth error");
    assert!(matches!(err, DriverError::Auth(ref m) if m == "Access denied"));

    let mut session = connect("right").await.unwrap();
    let err = session
        .execute("bad", &ExecOpts::default(), &mut |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::Query { ref message, .. } if message == "syntax"));
    let err = session.list_objects("shop").await.unwrap_err();
    assert_eq!(err.to_string(), "not supported by this driver: objects");
    // A failed call leaves the session usable.
    assert_eq!(session.list_databases().await.unwrap(), vec!["shop"]);
}

#[tokio::test]
async fn cancel_reaches_a_running_call() {
    let mut session = connect("right").await.unwrap();
    let cancel = session.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancel.cancel();
    });
    let opts = ExecOpts::default();
    let mut sink = |_| {};
    let run = session.execute("slow", &opts, &mut sink);
    let err = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("the cancel must not wait behind the call")
        .unwrap_err();
    assert!(matches!(err, DriverError::Cancelled));
    canceller.await.unwrap();
}

#[tokio::test]
async fn a_driver_that_goes_away_loses_the_connection() {
    let (host_out, driver_in) = tokio::io::duplex(1024);
    let (driver_out, host_in) = tokio::io::duplex(1024);
    let serving = tokio::spawn(serve(Arc::new(Fake::new()), driver_in, driver_out));
    let mut session = connect_over(host_in, host_out, &config("right"))
        .await
        .unwrap();
    serving.abort();
    let _ = serving.await;
    let err = session.list_databases().await.unwrap_err();
    assert!(matches!(err, DriverError::ConnectionLost(_)));
}
