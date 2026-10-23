#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests against a real MySQL. Skipped unless `QUARRY_TEST_MYSQL` is set.
//!
//!   docker compose -f tests/docker-compose.yml up -d
//!   QUARRY_TEST_MYSQL=127.0.0.1:13306 cargo test -p quarry-driver-mysql

use quarry_driver_api::{Driver, DriverError, ExecItem, ExecOpts, ResolvedConfig};
use quarry_driver_mysql::MysqlDriver;
use quarry_model::{
    ConnId, ConnectionConfig, DriverId, Environment, ObjectKind, ServerInfoKind, SslMode, Value,
};

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
    assert!(
        objects
            .iter()
            .any(|o| o.name == "products_touch" && o.kind == ObjectKind::Trigger)
    );
    assert!(
        objects
            .iter()
            .any(|o| o.name == "nightly_report" && o.kind == ObjectKind::Event)
    );
}

#[tokio::test]
async fn lists_server_variables_and_status() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let variables = session
        .server_info(ServerInfoKind::Variables)
        .await
        .unwrap();
    assert!(variables.iter().any(|r| r.name == "max_connections"));
    let status = session.server_info(ServerInfoKind::Status).await.unwrap();
    assert!(status.iter().any(|r| r.name == "Uptime"));
    let names: Vec<_> = variables.iter().map(|r| r.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "sorted by name");
}

#[tokio::test]
async fn wrong_password_is_auth_error() {
    let Some(cfg) = config("nope") else { return };
    let Err(err) = MysqlDriver::default().connect(&cfg).await else {
        panic!("expected error")
    };
    assert!(matches!(err, DriverError::Auth(_)), "{err:?}");
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
async fn session_survives_bad_sql() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    for bad in [
        "SELEC 1",
        "SELECT 1; SELEC 2",
        "SELECT 1; SELECT * FROM nope; SELECT 3",
    ] {
        let mut items = Vec::new();
        let err = session
            .execute(bad, &ExecOpts::default(), &mut |item| items.push(item))
            .await
            .err()
            .unwrap_or_else(|| panic!("{bad} succeeded: {items:?}"));
        let DriverError::Query { location, .. } = &err else {
            panic!("{bad}: {err:?}");
        };
        if bad.contains("SELEC ") {
            let location = location.as_ref().expect("a syntax error has a location");
            assert_eq!(
                (location.line, location.near.as_str()),
                (1, bad.trim_start_matches("SELECT 1; "))
            );
        }
        let mut after = Vec::new();
        session
            .execute("SELECT 42", &ExecOpts::default(), &mut |item| {
                after.push(item)
            })
            .await
            .unwrap_or_else(|err| panic!("after {bad}: {err:?}"));
        assert!(
            after
                .iter()
                .any(|i| matches!(i, ExecItem::Rows(r) if r[0][0] == Value::Int(42))),
            "after {bad}: {after:?}"
        );
    }
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

#[tokio::test]
async fn table_info_reads_columns_keys_and_ddl() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let table = quarry_model::TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "orders".into(),
    };
    let info = session.table_info(&table).await.unwrap();
    assert_eq!(info.primary_key, vec!["id".to_owned()]);
    let id = &info.columns[0];
    assert_eq!(id.name, "id");
    assert_eq!(id.type_name, "bigint unsigned");
    assert!(!id.nullable);
    assert_eq!(id.extra.as_deref(), Some("auto_increment"));
    let status = info.columns.iter().find(|c| c.name == "status").unwrap();
    // Defaults come back as SQL, ready to put in DDL.
    assert_eq!(status.default.as_deref(), Some("'pending'"));
    assert!(id.auto_increment && !id.generated);
    assert!(!info.view);
    assert!(
        info.indexes
            .iter()
            .any(|i| i.name == "idx_orders_status" && !i.unique && i.columns == ["status"])
    );
    let fk = &info.foreign_keys[0];
    assert_eq!(fk.name, "fk_orders_customer");
    assert_eq!(fk.columns, ["customer_id"]);
    assert_eq!(fk.references.name, "customers");
    assert_eq!(fk.referenced_columns, ["id"]);
    assert!(info.ddl.unwrap().starts_with("CREATE TABLE `orders`"));

    let view = quarry_model::TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "paid_orders".into(),
    };
    let info = session.table_info(&view).await.unwrap();
    assert!(info.primary_key.is_empty());
    assert!(info.view);
    assert!(info.ddl.unwrap().contains("VIEW `paid_orders`"));
}

#[tokio::test]
async fn schema_changes_round_trip() {
    use quarry_model::{AlterOp, ColumnDef, IndexDef, SchemaChange, TableRef};
    let Some(cfg) = config("quarry") else { return };
    let driver = MysqlDriver::default();
    let mut session = driver.connect(&cfg).await.unwrap();
    let table = TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "ddl_scratch".into(),
    };
    let column = |name: &str, type_name: &str, default: Option<&str>| ColumnDef {
        name: name.into(),
        type_name: type_name.into(),
        nullable: true,
        default: default.map(Into::into),
        auto_increment: false,
        on_update: None,
        comment: None,
    };
    let run = async |session: &mut Box<dyn quarry_driver_api::Session>, change: SchemaChange| {
        for sql in driver.dialect().schema_change(&change).unwrap().unwrap() {
            session
                .execute(&sql, &ExecOpts::default(), &mut |_| {})
                .await
                .unwrap();
        }
    };
    session
        .execute(
            "DROP TABLE IF EXISTS ddl_scratch",
            &ExecOpts::default(),
            &mut |_| {},
        )
        .await
        .unwrap();
    let mut id = column("id", "INT", None);
    id.nullable = false;
    id.auto_increment = true;
    run(
        &mut session,
        SchemaChange::CreateTable {
            table: table.clone(),
            columns: vec![id, column("note", "VARCHAR(20)", Some("'it''s'"))],
            primary_key: vec!["id".into()],
        },
    )
    .await;
    let mut updated = column("updated", "TIMESTAMP", Some("CURRENT_TIMESTAMP"));
    updated.on_update = Some("CURRENT_TIMESTAMP".into());
    run(
        &mut session,
        SchemaChange::AlterTable {
            table: table.clone(),
            ops: vec![
                AlterOp::AddColumn { column: updated },
                AlterOp::AddIndex {
                    index: IndexDef {
                        name: "by_note".into(),
                        columns: vec!["note".into()],
                        unique: true,
                    },
                },
            ],
        },
    )
    .await;
    let info = session.table_info(&table).await.unwrap();
    let note = info.columns.iter().find(|c| c.name == "note").unwrap();
    assert_eq!(note.default.as_deref(), Some("'it''s'"));
    let updated = info.columns.iter().find(|c| c.name == "updated").unwrap();
    assert_eq!(updated.default.as_deref(), Some("CURRENT_TIMESTAMP"));
    assert_eq!(updated.on_update.as_deref(), Some("CURRENT_TIMESTAMP"));
    assert!(info.indexes.iter().any(|i| i.name == "by_note" && i.unique));

    // Changing a column with what table_info read keeps its default and ON UPDATE.
    let def = ColumnDef {
        name: "changed".into(),
        type_name: updated.type_name.clone(),
        nullable: updated.nullable,
        default: updated.default.clone(),
        auto_increment: updated.auto_increment,
        on_update: updated.on_update.clone(),
        comment: Some("renamed".into()),
    };
    run(
        &mut session,
        SchemaChange::AlterTable {
            table: table.clone(),
            ops: vec![
                AlterOp::ModifyColumn {
                    name: "updated".into(),
                    column: def,
                },
                AlterOp::DropIndex {
                    name: "by_note".into(),
                },
            ],
        },
    )
    .await;
    let info = session.table_info(&table).await.unwrap();
    let changed = info.columns.iter().find(|c| c.name == "changed").unwrap();
    assert_eq!(changed.on_update.as_deref(), Some("CURRENT_TIMESTAMP"));
    assert_eq!(changed.comment.as_deref(), Some("renamed"));
    assert!(info.indexes.iter().all(|i| i.name != "by_note"));

    run(&mut session, SchemaChange::DropTable { table, view: false }).await;
}

#[tokio::test]
async fn parameterized_query_converts_like_plain_query() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let rows_of = |items: Vec<ExecItem>| -> Vec<Vec<Value>> {
        items
            .into_iter()
            .filter_map(|i| match i {
                ExecItem::Rows(r) => Some(r),
                _ => None,
            })
            .flatten()
            .collect()
    };
    let mut plain = Vec::new();
    session
        .execute(
            "SELECT * FROM all_types ORDER BY id",
            &ExecOpts::default(),
            &mut |i| plain.push(i),
        )
        .await
        .unwrap();
    let mut prepared = Vec::new();
    session
        .query(
            "SELECT * FROM all_types WHERE id >= ? ORDER BY id",
            &[Value::Text("0".into())],
            &ExecOpts::default(),
            &mut |i| prepared.push(i),
        )
        .await
        .unwrap();
    // The binary protocol returns typed values; they must come out the same as text ones.
    assert_eq!(rows_of(prepared), rows_of(plain));
}

#[tokio::test]
async fn lists_columns_and_estimates_rows() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let tables = session.list_columns("shop").await.unwrap();
    let orders = tables.iter().find(|t| t.table == "orders").unwrap();
    assert_eq!(orders.columns[..3], ["id", "customer_id", "status"]);
    assert!(tables.iter().any(|t| t.table == "paid_orders"));
    let table = quarry_model::TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "orders".into(),
    };
    let info = session.table_info(&table).await.unwrap();
    assert!(info.estimated_rows.is_some());
}

#[tokio::test]
async fn parameterized_query_with_no_rows_ends() {
    let Some(cfg) = config("quarry") else { return };
    let mut session = MysqlDriver::default().connect(&cfg).await.unwrap();
    let mut items = Vec::new();
    let mut sink = |i| items.push(i);
    let params = [Value::Text("0".into())];
    let opts = ExecOpts::default();
    let run = session.query(
        "SELECT * FROM orders WHERE id = ?",
        &params,
        &opts,
        &mut sink,
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .expect("an empty result must end")
        .unwrap();
    let sets = items
        .iter()
        .filter(|i| matches!(i, ExecItem::SetDone { .. }))
        .count();
    assert_eq!(sets, 1, "one empty result set");
}
