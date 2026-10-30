#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests against a real SQL Server. Skipped unless `QUARRY_TEST_SQLSERVER` is set.
//!
//!   docker compose -f tests/docker-compose.yml up -d --wait sqlserver
//!   QUARRY_TEST_SQLSERVER=127.0.0.1:11433 cargo test -p quarry-driver-sqlserver

use std::time::Duration;

use quarry_driver_api::testing::{self, Count, rows, run};
use quarry_driver_api::{Driver, DriverError, ExecItem, ExecOpts, ResolvedConfig, Session};
use quarry_driver_sqlserver::SqlServerDriver;
use quarry_model::{ObjectKind, ServerInfoKind, TableRef, Value};

fn config(password: &str) -> Option<ResolvedConfig> {
    // tiberius builds rustls with two providers in a workspace test; the binary installs ring.
    let _ = rustls::crypto::ring::default_provider().install_default();
    testing::config("QUARRY_TEST_SQLSERVER", "sqlserver", "sa", password)
}

async fn session() -> Option<Box<dyn Session>> {
    let cfg = config("Quarry-test-1")?;
    Some(SqlServerDriver.connect(&cfg).await.unwrap())
}

#[tokio::test]
async fn connects_and_lists_schemas_and_objects() {
    let Some(mut s) = session().await else { return };
    assert!(
        s.server_version().starts_with("16."),
        "{}",
        s.server_version()
    );
    let schemas = s.list_databases().await.unwrap();
    assert!(schemas.contains(&"dbo".to_owned()));
    assert!(schemas.contains(&"playground".to_owned()));
    assert!(!schemas.iter().any(|n| n == "sys" || n.starts_with("db_")));

    let objects = s.list_objects("dbo").await.unwrap();
    let kind = |name: &str| objects.iter().find(|o| o.name == name).map(|o| o.kind);
    assert_eq!(kind("orders"), Some(ObjectKind::Table));
    assert_eq!(kind("paid_orders"), Some(ObjectKind::View));
    assert_eq!(kind("order_count"), Some(ObjectKind::Function));
    assert_eq!(kind("touch_customer"), Some(ObjectKind::Procedure));
    assert_eq!(kind("customers_touch"), Some(ObjectKind::Trigger));

    let columns = s.list_columns("dbo").await.unwrap();
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
    assert_eq!(cell("c_bit"), Value::Bool(true));
    assert_eq!(cell("c_tinyint"), Value::Int(255));
    assert_eq!(cell("c_smallint"), Value::Int(-32768));
    assert_eq!(cell("c_bigint"), Value::Int(i64::MAX));
    assert_eq!(cell("c_real"), Value::Float(1.5));
    assert_eq!(cell("c_float"), Value::Float(0.1));
    assert_eq!(
        cell("c_decimal"),
        Value::Decimal("-1234567890123456789012345678.0123456789".into())
    );
    assert_eq!(cell("c_money"), Value::Decimal("12.3400".into()));
    assert_eq!(cell("c_char"), Value::Text("abcd".into()));
    assert_eq!(cell("c_nvarchar"), Value::Text("đã sửa ✓".into()));
    assert_eq!(cell("c_varbinary"), Value::Bytes(vec![0, 255]));
    assert_eq!(cell("c_date").display(64), "1999-12-31");
    assert_eq!(cell("c_time").display(64), "23:59:59.999999900");
    assert_eq!(cell("c_datetime").display(64), "2000-01-01 00:00:01.500");
    assert_eq!(cell("c_smalldatetime").display(64), "2000-01-01 12:30:00");
    assert_eq!(cell("c_datetime2").display(64), "2000-01-01 00:00:01.500");
    let Value::TimestampTz(at) = cell("c_datetimeoffset") else {
        panic!("datetimeoffset");
    };
    assert_eq!(at.to_rfc3339(), "2026-10-29T16:02:26.556+07:00");
    assert_eq!(
        cell("c_uniqueidentifier").display(64),
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    assert_eq!(cell("c_xml"), Value::Text("<a>1</a>".into()));
    assert_eq!(columns[1].type_name, "bit");
    // The second row is all NULL.
    assert!(rows[1][1..].iter().all(|v| *v == Value::Null));
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
    let items = run(&mut s, "SELECT count(*) FROM orders", ExecOpts::default()).await;
    assert_eq!(rows(&items)[0][0], Value::Int(5000));
}

#[tokio::test]
async fn several_result_sets_arrive_in_order() {
    let Some(mut s) = session().await else { return };
    let items = run(
        &mut s,
        "SELECT 1 AS a; SELECT 'x' AS b",
        ExecOpts::default(),
    )
    .await;
    let sets = items
        .iter()
        .filter(|i| matches!(i, ExecItem::Columns(_)))
        .count();
    assert_eq!(sets, 2);
    assert_eq!(
        rows(&items),
        vec![vec![Value::Int(1)], vec![Value::Text("x".into())]]
    );
}

#[tokio::test]
async fn text_parameters_take_the_column_type() {
    let Some(mut s) = session().await else { return };
    s.begin().await.unwrap();
    let changed = s
        .execute_params(
            "UPDATE orders SET total = @P1, note = @P2, ordered_at = @P3 WHERE id = @P4",
            &[
                Value::Text("12.50".into()),
                Value::Null,
                Value::Text("2026-10-29T16:00:00+07:00".into()),
                Value::Text("7".into()),
            ],
        )
        .await
        .unwrap();
    assert_eq!(changed, 1);
    let mut items = Vec::new();
    s.query(
        "SELECT total, note FROM orders WHERE id = @P1",
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
        "UPDATE orders SET note = N'x' WHERE customer_id = 1",
        ExecOpts::default(),
    )
    .await;
    assert!(
        items
            .iter()
            .any(|i| matches!(i, ExecItem::SetDone { affected: 1000, .. })),
        "{items:?}"
    );
    s.rollback().await.unwrap();
}

#[tokio::test]
async fn table_info_reads_the_catalog() {
    let Some(mut s) = session().await else { return };
    let table = TableRef {
        database: Some("dbo".into()),
        schema: None,
        name: "orders".into(),
    };
    let info = s.table_info(&table).await.unwrap();
    assert_eq!(info.primary_key, vec!["id"]);
    let id = &info.columns[0];
    assert!(id.auto_increment && !id.nullable);
    let note = info.columns.iter().find(|c| c.name == "note").unwrap();
    assert_eq!(note.type_name, "nvarchar(max)");
    let total = info.columns.iter().find(|c| c.name == "total").unwrap();
    assert_eq!(total.type_name, "decimal(12,2)");
    let status = info.columns.iter().find(|c| c.name == "status").unwrap();
    assert_eq!(status.default.as_deref(), Some("('pending')"));
    assert!(
        info.indexes
            .iter()
            .any(|i| i.name == "idx_orders_status" && i.columns == ["status"] && !i.unique)
    );
    let fk = &info.foreign_keys[0];
    assert_eq!(fk.name, "fk_orders_customer");
    assert_eq!(fk.columns, ["customer_id"]);
    assert_eq!(fk.references.name, "customers");
    assert_eq!(fk.on_delete.as_deref(), Some("CASCADE"));
    assert_eq!(info.estimated_rows, Some(5000));
    let ddl = info.ddl.unwrap();
    assert!(ddl.starts_with("CREATE TABLE [dbo].[orders] ("), "{ddl}");
    assert!(ddl.contains("PRIMARY KEY ([id])"), "{ddl}");
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
    assert!(message.contains("SQL Server error 207"), "{message}");
    let location = location.unwrap();
    assert_eq!(location.line, 2);
    assert_eq!(location.near, "nope FROM orders");
    // The session is still usable.
    let items = run(&mut s, "SELECT 1", ExecOpts::default()).await;
    assert_eq!(rows(&items)[0][0], Value::Int(1));

    let cfg = config("wrong").unwrap();
    let err = SqlServerDriver.connect(&cfg).await.err().unwrap();
    assert!(
        matches!(err, DriverError::Auth(ref m) if m.contains("18456")),
        "{err:?}"
    );
}

#[tokio::test]
async fn cancel_stops_the_statement_without_waiting() {
    let Some(mut s) = session().await else { return };
    let cancel = s.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let err = s
        .execute(
            "WAITFOR DELAY '00:00:10'",
            &ExecOpts::default(),
            &mut |_| {},
        )
        .await
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5));
    canceller.await.unwrap();
    // Confirmed once tiberius reads past the first message (tiberius#459); until then the
    // connection is given up and a new one works.
    match err {
        DriverError::Cancelled => {
            let items = run(&mut s, "SELECT 1", ExecOpts::default()).await;
            assert_eq!(rows(&items)[0][0], Value::Int(1));
        }
        DriverError::ConnectionLost(message) => {
            assert!(message.starts_with("Query cancelled"));
            let mut fresh = session().await.unwrap();
            let items = run(&mut fresh, "SELECT 1", ExecOpts::default()).await;
            assert_eq!(rows(&items)[0][0], Value::Int(1));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn server_info_lists_settings() {
    let Some(mut s) = session().await else { return };
    let settings = s.server_info(ServerInfoKind::Variables).await.unwrap();
    assert!(settings.iter().any(|r| r.name == "max server memory (MB)"));
    let status = s.server_info(ServerInfoKind::Status).await.unwrap();
    assert!(status.iter().any(|r| r.name == "User Connections"));
}

#[tokio::test]
async fn stream_reads_every_row_and_stops_when_told() {
    let Some(mut s) = session().await else { return };
    let opts = ExecOpts {
        row_limit: None,
        batch_size: 1000,
    };
    let sql = "SELECT id FROM orders ORDER BY id";
    let mut all = Count::default();
    s.stream(sql, &[], &opts, &mut all).await.unwrap();
    assert_eq!((all.rows, all.batches), (5000, 5));
    assert!(all.is_done);

    let Some(mut s) = session().await else { return };
    let mut some = Count {
        stop_at: Some(2000),
        ..Count::default()
    };
    s.stream(sql, &[], &opts, &mut some).await.unwrap();
    assert_eq!((some.rows, some.batches), (2000, 2));
    assert!(!some.is_done);
}
