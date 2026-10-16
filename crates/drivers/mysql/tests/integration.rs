#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests against a real MySQL. Skipped unless `QUARRY_TEST_MYSQL` is set.
//!
//!   docker compose -f tests/docker-compose.yml up -d
//!   QUARRY_TEST_MYSQL=127.0.0.1:13306 cargo test -p quarry-driver-mysql

use quarry_driver_api::{Driver, DriverError, ExecItem, ExecOpts, ResolvedConfig};
use quarry_driver_mysql::MysqlDriver;
use quarry_model::{ConnId, ConnectionConfig, DriverId, Environment, ObjectKind, SslMode, Value};

fn config(password: &str) -> Option<ResolvedConfig> {
    let addr = std::env::var("QUARRY_TEST_MYSQL").ok()?;
    let (host, port) = addr.split_once(':')?;
    Some(ResolvedConfig {
        config: ConnectionConfig {
            id: ConnId::new(),
            name: "test".into(),
            driver: DriverId("mysql".into()),
            host: Some(host.into()),
            port: port.parse().ok(),
            database: Some("shop".into()),
            user: Some("quarry".into()),
            path: None,
            ssl: SslMode::Prefer,
            ssh: None,
            environment: Environment::Local,
            read_only: false,
            color: None,
            group: None,
        },
        password: Some(password.into()),
    })
}

async fn run(sql: &str, opts: ExecOpts) -> Vec<ExecItem> {
    let cfg = config("quarry").expect("QUARRY_TEST_MYSQL");
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let mut items = Vec::new();
    session
        .execute(sql, &opts, &mut |item| items.push(item))
        .await
        .unwrap();
    items
}

#[tokio::test]
async fn connects_and_lists_schema() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    assert!(session.server_version().starts_with('8'));
    let dbs = session.list_databases().await.unwrap();
    assert!(dbs.contains(&"shop".to_owned()) && dbs.contains(&"inventory".to_owned()));
    let objects = session.list_objects("shop").await.unwrap();
    assert!(
        objects
            .iter()
            .any(|o| o.name == "orders" && o.kind == ObjectKind::Table)
    );
    assert!(
        objects
            .iter()
            .any(|o| o.name == "paid_orders" && o.kind == ObjectKind::View)
    );
}

#[tokio::test]
async fn wrong_password_is_auth_error() {
    let Some(cfg) = config("nope") else { return };
    let Err(err) = MysqlDriver::default().connect(&cfg).await else {
        panic!("expected error")
    };
    assert!(matches!(err, DriverError::Auth), "{err:?}");
}

#[tokio::test]
async fn multi_statement_script_and_row_limit() {
    if config("quarry").is_none() {
        return;
    }
    let items = run(
        "SELECT id FROM orders ORDER BY id; UPDATE customers SET country = country WHERE id = 1; SELECT 1 AS one",
        ExecOpts { row_limit: Some(1200), batch_size: 500 },
    )
    .await;
    let set_done: Vec<_> = items
        .iter()
        .filter_map(|i| match i {
            ExecItem::SetDone {
                rows, truncated, ..
            } => Some((*rows, *truncated)),
            _ => None,
        })
        .collect();
    assert_eq!(set_done, [(1200, true), (0, false), (1, false)]);
    let batches = items
        .iter()
        .filter(|i| matches!(i, ExecItem::Rows(_)))
        .count();
    assert_eq!(batches, 4); // 500 + 500 + 200, then the single row of SELECT 1
    assert!(matches!(items.last(), Some(ExecItem::Done { .. })));
}

#[tokio::test]
async fn converts_types() {
    if config("quarry").is_none() {
        return;
    }
    let items = run("SELECT * FROM all_types ORDER BY id", ExecOpts::default()).await;
    let rows: Vec<Vec<Value>> = items
        .into_iter()
        .filter_map(|i| {
            if let ExecItem::Rows(r) = i {
                Some(r)
            } else {
                None
            }
        })
        .flatten()
        .collect();
    let first = &rows[0];
    assert_eq!(first[1], Value::Int(-128));
    assert_eq!(first[3], Value::UInt(u64::MAX));
    assert_eq!(
        first[6],
        Value::Decimal("12345678901234567890.0123456789".into())
    );
    assert_eq!(first[8], Value::Text("Xin chào 👋".into()));
    assert_eq!(first[10], Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]));
    assert!(matches!(first[11], Value::Date(_)));
    assert!(matches!(first[13], Value::Timestamp(_)));
    assert!(matches!(first[17], Value::Json(_)));
    assert!(rows[1][1..].iter().all(Value::is_null));
}

#[tokio::test]
async fn query_error_has_message() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let err = session
        .execute("SELECT * FROM nope", &ExecOpts::default(), &mut |_| {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("doesn't exist"), "{err}");
}

#[tokio::test]
async fn cancel_long_query() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let cancel = session.cancel_handle();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let res = session
        .execute("SELECT SLEEP(10)", &ExecOpts::default(), &mut |_| {})
        .await;
    assert!(started.elapsed().as_secs() < 5);
    // A killed SLEEP() returns 1 instead of an error; other statements fail with 1317.
    assert!(res.is_ok() || matches!(res, Err(DriverError::Cancelled)));
}
