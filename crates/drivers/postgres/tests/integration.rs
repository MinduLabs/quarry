#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests against a real PostgreSQL. Skipped unless `QUARRY_TEST_POSTGRES` is set.
//!
//!   docker compose -f tests/docker-compose.yml up -d postgres
//!   QUARRY_TEST_POSTGRES=127.0.0.1:15432 cargo test -p quarry-driver-postgres

use std::time::Duration;

use quarry_driver_api::{
    Driver, DriverError, ExecItem, ExecOpts, ResolvedConfig, RowSink, Session,
};
use quarry_driver_postgres::PostgresDriver;
use quarry_model::{
    ConnId, ConnectionConfig, DriverId, Environment, ObjectKind, ServerInfoKind, SslMode, TableRef,
    Value,
};

fn config(password: &str) -> Option<ResolvedConfig> {
    let addr = std::env::var("QUARRY_TEST_POSTGRES").ok()?;
    let (host, port) = addr.split_once(':')?;
    Some(ResolvedConfig {
        config: ConnectionConfig {
            id: ConnId::new(),
            name: "test".into(),
            driver: DriverId("postgres".into()),
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

async fn session() -> Option<Box<dyn Session>> {
    let cfg = config("quarry")?;
    Some(PostgresDriver.connect(&cfg).await.unwrap())
}

async fn run(session: &mut Box<dyn Session>, sql: &str, opts: ExecOpts) -> Vec<ExecItem> {
    let mut items = Vec::new();
    session
        .execute(sql, &opts, &mut |item| items.push(item))
        .await
        .unwrap();
    items
}

fn rows(items: &[ExecItem]) -> Vec<Vec<Value>> {
    items
        .iter()
        .filter_map(|item| match item {
            ExecItem::Rows(rows) => Some(rows.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[tokio::test]
async fn connects_and_lists_schemas_and_objects() {
    let Some(mut s) = session().await else { return };
    assert!(s.server_version().starts_with("17"));
    let schemas = s.list_databases().await.unwrap();
    assert!(schemas.contains(&"public".to_owned()));
    assert!(schemas.contains(&"playground".to_owned()));
    assert!(!schemas.iter().any(|n| n == "pg_catalog"));

    let objects = s.list_objects("public").await.unwrap();
    let kind = |name: &str| objects.iter().find(|o| o.name == name).map(|o| o.kind);
    assert_eq!(kind("orders"), Some(ObjectKind::Table));
    assert_eq!(kind("paid_orders"), Some(ObjectKind::View));
    assert_eq!(kind("order_totals"), Some(ObjectKind::MaterializedView));
    assert_eq!(kind("invoice_numbers"), Some(ObjectKind::Sequence));
    assert_eq!(kind("order_count"), Some(ObjectKind::Function));
    assert_eq!(kind("touch_customer"), Some(ObjectKind::Procedure));
    assert_eq!(kind("customers_touch"), Some(ObjectKind::Trigger));

    let columns = s.list_columns("public").await.unwrap();
    let orders = columns.iter().find(|t| t.table == "orders").unwrap();
    assert_eq!(orders.columns[..2], ["id", "customer_id"]);
    s.close().await.unwrap();
}

#[tokio::test]
async fn every_sample_type_decodes() {
    let Some(mut s) = session().await else { return };
    let items = run(
        &mut s,
        "SELECT * FROM playground.samples ORDER BY id",
        ExecOpts::default(),
    )
    .await;
    let ExecItem::Columns(columns) = &items[0] else {
        panic!("columns first");
    };
    let rows = rows(&items);
    let cell = |name: &str| {
        let i = columns.iter().position(|c| c.name == name).unwrap();
        rows[0][i].clone()
    };
    assert_eq!(cell("c_bool"), Value::Bool(true));
    assert_eq!(cell("c_int2"), Value::Int(-32768));
    assert_eq!(cell("c_int8"), Value::Int(i64::MAX));
    assert_eq!(cell("c_oid"), Value::UInt(4294967295));
    assert_eq!(cell("c_float4"), Value::Float(1.5));
    assert_eq!(
        cell("c_numeric"),
        Value::Decimal("-123456789012345678901234567890.0123456789".into())
    );
    assert_eq!(cell("c_money"), Value::Decimal("12.34".into()));
    assert_eq!(cell("c_text"), Value::Text("đã sửa ✓".into()));
    assert_eq!(cell("c_bytea"), Value::Bytes(vec![0, 255]));
    assert_eq!(cell("c_date").display(64), "1999-12-31");
    assert_eq!(cell("c_time").display(64), "23:59:59.999999");
    assert_eq!(
        cell("c_timetz"),
        Value::Other {
            type_name: "timetz".into(),
            text: "12:00:00+07".into()
        }
    );
    assert_eq!(cell("c_timestamp").display(64), "2000-01-01 00:00:01.500");
    let Value::TimestampTz(at) = cell("c_timestamptz") else {
        panic!("timestamptz");
    };
    assert_eq!(at.to_rfc3339(), "2026-10-29T09:02:26.556+00:00");
    assert_eq!(
        cell("c_interval"),
        Value::Other {
            type_name: "interval".into(),
            text: "1 year 2 mons 3 days 04:00:05".into()
        }
    );
    assert_eq!(
        cell("c_uuid").display(64),
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    assert_eq!(cell("c_json"), Value::Json(r#"{"a": [1, 2]}"#.into()));
    assert_eq!(cell("c_jsonb"), Value::Json(r#"{"b": true}"#.into()));
    assert_eq!(
        cell("c_inet"),
        Value::Other {
            type_name: "inet".into(),
            text: "10.0.0.1".into()
        }
    );
    assert_eq!(
        cell("c_cidr"),
        Value::Other {
            type_name: "cidr".into(),
            text: "10.0.0.0/24".into()
        }
    );
    assert_eq!(
        cell("c_bit"),
        Value::Other {
            type_name: "bit".into(),
            text: "1010".into()
        }
    );
    assert_eq!(
        cell("c_int_array"),
        Value::Array(vec![
            Value::Array(vec![Value::Int(1), Value::Int(2)]),
            Value::Array(vec![Value::Int(3), Value::Null]),
        ])
    );
    assert_eq!(
        cell("c_text_array"),
        Value::Array(vec![Value::Text("x".into()), Value::Text("y \"z\"".into())])
    );
    assert!(matches!(cell("c_point"), Value::Other { ref type_name, .. } if type_name == "point"));
    // The second row is all NULL.
    assert!(rows[1][1..].iter().all(|v| *v == Value::Null));
}

#[tokio::test]
async fn result_columns_name_their_table() {
    let Some(mut s) = session().await else { return };
    let items = run(
        &mut s,
        "SELECT o.id AS order_id, 1 AS one FROM orders o LIMIT 1",
        ExecOpts::default(),
    )
    .await;
    let ExecItem::Columns(columns) = &items[0] else {
        panic!("columns first");
    };
    assert_eq!(columns[0].source_database.as_deref(), Some("public"));
    assert_eq!(columns[0].source_table.as_deref(), Some("orders"));
    assert_eq!(columns[0].source_column.as_deref(), Some("id"));
    assert_eq!(columns[1].source_table, None);
}

#[tokio::test]
async fn row_limit_stops_sending_and_marks_truncated() {
    let Some(mut s) = session().await else { return };
    let items = run(
        &mut s,
        "SELECT id FROM orders ORDER BY id",
        ExecOpts {
            row_limit: Some(120),
            batch_size: 50,
        },
    )
    .await;
    let batches: Vec<usize> = items
        .iter()
        .filter_map(|i| match i {
            ExecItem::Rows(r) => Some(r.len()),
            _ => None,
        })
        .collect();
    assert_eq!(batches, vec![50, 50, 20]);
    assert!(items.iter().any(|i| matches!(
        i,
        ExecItem::SetDone {
            rows: 120,
            truncated: true,
            ..
        }
    )));
    // The session is still usable.
    let items = run(&mut s, "SELECT count(*) FROM orders", ExecOpts::default()).await;
    assert_eq!(rows(&items)[0][0], Value::Int(5000));
}

#[tokio::test]
async fn text_parameters_take_the_column_type() {
    let Some(mut s) = session().await else { return };
    s.begin().await.unwrap();
    // Values as the grid holds them: text for a number, a timestamp.
    let changed = s
        .execute_params(
            "UPDATE orders SET total = $1, note = $2 WHERE id = $3",
            &[
                Value::Text("12.50".into()),
                Value::Null,
                Value::Text("7".into()),
            ],
        )
        .await
        .unwrap();
    assert_eq!(changed, 1);
    let mut items = Vec::new();
    s.query(
        "SELECT total, note FROM orders WHERE id = $1",
        &[Value::Int(7)],
        &ExecOpts::default(),
        &mut |item| items.push(item),
    )
    .await
    .unwrap();
    assert_eq!(
        rows(&items),
        vec![vec![Value::Decimal("12.50".into()), Value::Null]]
    );
    s.rollback().await.unwrap();
}

#[tokio::test]
async fn statements_without_rows_report_affected() {
    let Some(mut s) = session().await else { return };
    s.begin().await.unwrap();
    let items = run(
        &mut s,
        "UPDATE orders SET note = 'x' WHERE customer_id = 1",
        ExecOpts::default(),
    )
    .await;
    assert!(
        items
            .iter()
            .any(|i| matches!(i, ExecItem::SetDone { affected: 1000, .. }))
    );
    s.rollback().await.unwrap();
}

#[tokio::test]
async fn table_info_reads_the_catalog() {
    let Some(mut s) = session().await else { return };
    let table = TableRef {
        database: Some("public".into()),
        schema: None,
        name: "orders".into(),
    };
    let info = s.table_info(&table).await.unwrap();
    assert_eq!(info.primary_key, vec!["id"]);
    let id = &info.columns[0];
    assert!(id.auto_increment && !id.nullable);
    let note = info.columns.iter().find(|c| c.name == "note").unwrap();
    assert_eq!(
        note.comment.as_deref(),
        Some("Written by the customer at checkout")
    );
    assert_eq!(note.type_name, "text");
    let status = info.columns.iter().find(|c| c.name == "status").unwrap();
    assert_eq!(status.default.as_deref(), Some("'pending'::order_status"));
    assert!(
        info.indexes
            .iter()
            .any(|i| i.name == "idx_orders_status" && i.columns == ["status"] && !i.unique)
    );
    let fk = &info.foreign_keys[0];
    assert_eq!(fk.columns, ["customer_id"]);
    assert_eq!(fk.references.name, "customers");
    assert_eq!(fk.on_delete.as_deref(), Some("CASCADE"));
    let ddl = info.ddl.unwrap();
    assert!(
        ddl.starts_with("CREATE TABLE \"public\".\"orders\" ("),
        "{ddl}"
    );
    assert!(ddl.contains("PRIMARY KEY (id)"), "{ddl}");
    assert!(ddl.contains("CREATE INDEX idx_orders_status"), "{ddl}");
    assert!(!info.view);
    assert_eq!(s.primary_key(&table).await.unwrap(), vec!["id"]);

    let view = s
        .table_info(&TableRef {
            name: "paid_orders".into(),
            ..table
        })
        .await
        .unwrap();
    assert!(view.view);
    assert!(view.ddl.unwrap().starts_with("CREATE VIEW"));
}

#[tokio::test]
async fn search_path_follows_the_tab_database() {
    let Some(mut s) = session().await else { return };
    s.use_database("playground").await.unwrap();
    let items = run(&mut s, "SELECT count(*) FROM samples", ExecOpts::default()).await;
    assert_eq!(rows(&items)[0][0], Value::Int(2));
}

#[tokio::test]
async fn errors_say_where_and_why() {
    let Some(mut s) = session().await else { return };
    let err = s
        .execute(
            "SELECT 1,\n  nope FROM orders",
            &ExecOpts::default(),
            &mut |_| {},
        )
        .await
        .unwrap_err();
    let DriverError::Query { message, location } = err else {
        panic!("query error");
    };
    assert!(message.contains("SQLSTATE 42703"), "{message}");
    let location = location.unwrap();
    assert_eq!(location.line, 2);
    assert_eq!(location.near, "nope FROM orders");

    let cfg = config("wrong").unwrap();
    let err = PostgresDriver.connect(&cfg).await.err().unwrap();
    assert!(
        matches!(err, DriverError::Auth(ref m) if m.contains("28P01")),
        "{err:?}"
    );
}

#[tokio::test]
async fn read_only_sessions_refuse_writes() {
    let Some(mut s) = session().await else { return };
    s.set_read_only(true).await.unwrap();
    let err = s
        .execute("DELETE FROM orders", &ExecOpts::default(), &mut |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::Query { ref message, .. } if message.contains("25006")));
}

#[tokio::test]
async fn notices_and_do_blocks_reach_the_sink() {
    let Some(mut s) = session().await else { return };
    let items = run(
        &mut s,
        "DO $$ BEGIN RAISE NOTICE 'hello %', 42; END $$",
        ExecOpts::default(),
    )
    .await;
    assert!(
        items
            .iter()
            .any(|i| matches!(i, ExecItem::Notice(n) if n.contains("hello 42"))),
        "{items:?}"
    );
}

#[tokio::test]
async fn cancel_stops_a_running_statement() {
    let Some(mut s) = session().await else { return };
    let cancel = s.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let err = s
        .execute("SELECT pg_sleep(10)", &ExecOpts::default(), &mut |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::Cancelled), "{err:?}");
    canceller.await.unwrap();
    let items = run(&mut s, "SELECT 1", ExecOpts::default()).await;
    assert_eq!(rows(&items)[0][0], Value::Int(1));
}

#[tokio::test]
async fn server_info_lists_settings_and_counters() {
    let Some(mut s) = session().await else { return };
    let settings = s.server_info(ServerInfoKind::Variables).await.unwrap();
    assert!(settings.iter().any(|r| r.name == "max_connections"));
    let status = s.server_info(ServerInfoKind::Status).await.unwrap();
    assert!(status.iter().any(|r| r.name == "xact_commit"));
}

/// Counts the rows a stream sends; returns `false` once it has `stop_at` rows, when set.
struct Count {
    rows: usize,
    batches: usize,
    is_done: bool,
    stop_at: Option<usize>,
}

#[async_trait::async_trait]
impl RowSink for Count {
    async fn accept(&mut self, item: ExecItem) -> bool {
        match item {
            ExecItem::Rows(rows) => {
                self.rows += rows.len();
                self.batches += 1;
            }
            ExecItem::SetDone { .. } => self.is_done = true,
            _ => {}
        }
        self.stop_at.is_none_or(|stop| self.rows < stop)
    }
}

#[tokio::test]
async fn stream_reads_every_row_and_stops_when_told() {
    let Some(mut s) = session().await else { return };
    let opts = ExecOpts {
        row_limit: None,
        batch_size: 1000,
    };
    let sql = "SELECT id FROM orders ORDER BY id";
    let mut all = Count {
        rows: 0,
        batches: 0,
        is_done: false,
        stop_at: None,
    };
    s.stream(sql, &[], &opts, &mut all).await.unwrap();
    assert_eq!((all.rows, all.batches), (5000, 5));
    assert!(all.is_done);

    let Some(mut s) = session().await else { return };
    let mut some = Count {
        rows: 0,
        batches: 0,
        is_done: false,
        stop_at: Some(2000),
    };
    s.stream(sql, &[], &opts, &mut some).await.unwrap();
    assert_eq!((some.rows, some.batches), (2000, 2));
    assert!(!some.is_done);
}
