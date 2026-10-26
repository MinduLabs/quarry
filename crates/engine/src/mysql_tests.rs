//! Integration tests against a real MySQL (`QUARRY_TEST_MYSQL=127.0.0.1:13306`).

use std::time::Duration;

use quarry_model::{
    BrowseQuery, ConnId, ConnectionConfig, DriverId, Environment, RowChange, ServerInfoKind,
    SslMode, TableRef,
};

use super::*;

fn config() -> Option<ConnectionConfig> {
    let addr = std::env::var("QUARRY_TEST_MYSQL").ok()?;
    let (host, port) = addr.split_once(':')?;
    Some(ConnectionConfig {
        id: ConnId::new(),
        name: "Local MySQL".into(),
        driver: DriverId("mysql".into()),
        host: Some(host.into()),
        port: port.parse().ok(),
        database: None,
        user: Some("quarry".into()),
        path: None,
        ssl: SslMode::Prefer,
        ssh: None,
        environment: Environment::Local,
        read_only: false,
        color: None,
        group: None,
    })
}

async fn next_matching(events: &EngineEvents, pred: impl Fn(&Event) -> bool) -> Event {
    let wait = async {
        loop {
            let event = events.recv().await.unwrap();
            if pred(&event) {
                return event;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), wait)
        .await
        .unwrap()
}

#[tokio::test]
async fn save_connect_browse_and_query() {
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();

    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    let Event::Connections { connections } =
        next_matching(&events, |e| matches!(e, Event::Connections { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(connections.len(), 1);
    let saved = std::fs::read_to_string(dir.join("connections.toml")).unwrap();
    assert!(
        !saved.contains("password"),
        "passwords must never be written to disk"
    );

    engine.send(Command::Connect { id }).unwrap();
    let Event::Databases { databases, .. } =
        next_matching(&events, |e| matches!(e, Event::Databases { .. })).await
    else {
        unreachable!()
    };
    assert!(databases.contains(&"shop".to_owned()));

    engine
        .send(Command::LoadObjects {
            id,
            database: "shop".into(),
        })
        .unwrap();
    let Event::Objects { objects, .. } =
        next_matching(&events, |e| matches!(e, Event::Objects { .. })).await
    else {
        unreachable!()
    };
    assert!(objects.unwrap().iter().any(|o| o.name == "customers"));

    engine
        .send(Command::Run {
            tab: 1,
            conn: id,
            database: Some("shop".into()),
            sql: "SELECT COUNT(*) AS n FROM orders".into(),
            confirmed: false,
        })
        .unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(rows[0][0], quarry_model::Value::Int(5000));
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 1, .. })
    })
    .await;

    // Cancel a long query.
    engine
        .send(Command::Run {
            tab: 1,
            conn: id,
            database: None,
            sql: "SELECT SLEEP(10), 1".into(),
            confirmed: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::QueryStarted { tab: 1 })).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    engine.send(Command::Cancel { tab: 1 }).unwrap();
    let started = std::time::Instant::now();
    next_matching(&events, |e| {
        matches!(
            e,
            Event::QueryFinished { tab: 1, .. } | Event::QueryFailed { tab: 1, .. }
        )
    })
    .await;
    assert!(started.elapsed() < Duration::from_secs(5));
    // The cancelled query is still written to the history; wait so cleanup does not race it.
    next_matching(
        &events,
        |e| matches!(e, Event::HistoryAdded { entry } if entry.sql.contains("SLEEP")),
    )
    .await;

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn browse_pages_and_table_info() {
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;

    let table = TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "orders".into(),
    };
    engine
        .send(Command::Browse {
            tab: 2,
            conn: id,
            table: table.clone(),
            limit: 100,
            offset: 1000,
            query: BrowseQuery::default(),
        })
        .unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 2, .. })).await
    else {
        unreachable!()
    };
    assert_eq!(rows.len(), 100);
    assert_eq!(rows[0][0], quarry_model::Value::UInt(1001));
    let Event::ResultDone {
        rows, truncated, ..
    } = next_matching(&events, |e| matches!(e, Event::ResultDone { tab: 2, .. })).await
    else {
        unreachable!()
    };
    assert_eq!((rows, truncated), (100, false));

    // Table info connects on demand.
    engine
        .send(Command::LoadTableInfo {
            request: 9,
            conn: id,
            table,
        })
        .unwrap();
    let Event::TableInfo { info, error, .. } = next_matching(&events, |e| {
        matches!(e, Event::TableInfo { request: 9, .. })
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    assert_eq!(info.unwrap().primary_key, vec!["id".to_owned()]);

    engine
        .send(Command::LoadServerInfo {
            request: 10,
            conn: id,
            kind: ServerInfoKind::Variables,
        })
        .unwrap();
    let Event::ServerInfo { rows, error, .. } = next_matching(&events, |e| {
        matches!(e, Event::ServerInfo { request: 10, .. })
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    assert!(rows.unwrap().iter().any(|r| r.name == "max_connections"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn grid_edits_commit_or_roll_back_together() {
    use quarry_model::{CellValue, Value};
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;

    let run = |sql: &str| Command::Run {
        tab: 3,
        conn: id,
        database: Some("shop".into()),
        sql: sql.into(),
        confirmed: false,
    };
    engine
        .send(run("DROP TABLE IF EXISTS edit_scratch; \
             CREATE TABLE edit_scratch (id INT PRIMARY KEY, name VARCHAR(20) NULL); \
             INSERT INTO edit_scratch VALUES (1, 'one'), (2, 'two');"))
        .unwrap();
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 3, .. })
    })
    .await;
    let Event::HistoryAdded { entry } =
        next_matching(&events, |e| matches!(e, Event::HistoryAdded { .. })).await
    else {
        unreachable!()
    };
    assert!(entry.sql.contains("CREATE TABLE edit_scratch") && entry.error.is_none());

    let cell = |column: &str, value: Value| CellValue {
        column: column.into(),
        value,
    };
    let table = TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "edit_scratch".into(),
    };
    let apply = |changes: Vec<RowChange>| Command::ApplyChanges {
        tab: 3,
        conn: id,
        table: table.clone(),
        changes,
        confirmed: false,
    };

    // Update (to the same value too), insert and delete in one batch.
    engine
        .send(apply(vec![
            RowChange::Update {
                key: vec![cell("id", Value::Int(1))],
                set: vec![cell("name", Value::Text("uno".into()))],
            },
            RowChange::Update {
                key: vec![cell("id", Value::Int(2))],
                set: vec![cell("name", Value::Text("two".into()))],
            },
            RowChange::Insert {
                values: vec![cell("id", Value::Int(3)), cell("name", Value::Null)],
            },
            RowChange::Delete {
                key: vec![cell("id", Value::Int(2))],
            },
        ]))
        .unwrap();
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::ChangesApplied { tab: 3, .. } | Event::ChangesFailed { tab: 3, .. }
        )
    })
    .await;
    assert!(
        matches!(&event, Event::ChangesApplied { message, .. } if message == "2 rows updated, 1 row inserted, 1 row deleted"),
        "{event:?}"
    );

    // A missing row rolls back the insert in the same batch.
    engine
        .send(apply(vec![
            RowChange::Insert {
                values: vec![cell("id", Value::Int(4))],
            },
            RowChange::Delete {
                key: vec![cell("id", Value::Int(99))],
            },
        ]))
        .unwrap();
    next_matching(&events, |e| {
        matches!(e, Event::ChangesFailed { tab: 3, .. })
    })
    .await;

    engine
        .send(run("SELECT id, name FROM edit_scratch ORDER BY id"))
        .unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 3, .. })).await
    else {
        unreachable!()
    };
    assert_eq!(
        rows,
        vec![
            vec![Value::Int(1), Value::Text("uno".into())],
            vec![Value::Int(3), Value::Null],
        ]
    );
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 3, .. })
    })
    .await;
    engine.send(run("DROP TABLE edit_scratch")).unwrap();
    // The history entry is written after the query finishes; wait so cleanup does not race it.
    next_matching(
        &events,
        |e| matches!(e, Event::HistoryAdded { entry } if entry.sql == "DROP TABLE edit_scratch"),
    )
    .await;

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn schema_review_then_run_and_edits_without_a_key() {
    use quarry_model::{CellValue, SchemaChange, Value};
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;
    let run = |sql: &str| Command::Run {
        tab: 4,
        conn: id,
        database: Some("shop".into()),
        sql: sql.into(),
        confirmed: false,
    };
    engine
        .send(run("DROP TABLE IF EXISTS nokey_scratch; \
             CREATE TABLE nokey_scratch (a INT NULL, b VARCHAR(10) NULL); \
             INSERT INTO nokey_scratch VALUES (1, NULL), (2, 'x'), (2, 'x');"))
        .unwrap();
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 4, .. })
    })
    .await;
    let table = TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "nokey_scratch".into(),
    };
    let cell = |column: &str, value: Value| CellValue {
        column: column.into(),
        value,
    };
    let apply = |changes: Vec<RowChange>| Command::ApplyChanges {
        tab: 4,
        conn: id,
        table: table.clone(),
        changes,
        confirmed: false,
    };
    // Matched by all columns, NULL included.
    engine
        .send(apply(vec![RowChange::Update {
            key: vec![cell("a", Value::Int(1)), cell("b", Value::Null)],
            set: vec![cell("b", Value::Text("one".into()))],
        }]))
        .unwrap();
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::ChangesApplied { tab: 4, .. } | Event::ChangesFailed { tab: 4, .. }
        )
    })
    .await;
    assert!(matches!(event, Event::ChangesApplied { .. }), "{event:?}");
    // Two identical rows: the change would hit both, so nothing is saved.
    engine
        .send(apply(vec![RowChange::Delete {
            key: vec![cell("a", Value::Int(2)), cell("b", Value::Text("x".into()))],
        }]))
        .unwrap();
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::ChangesApplied { tab: 4, .. } | Event::ChangesFailed { tab: 4, .. }
        )
    })
    .await;
    assert!(
        matches!(&event, Event::ChangesFailed { error, .. } if error.contains("2 rows")),
        "{event:?}"
    );

    // A schema change is only reviewed until it is confirmed.
    let drop = SchemaChange::DropTable {
        table: table.clone(),
        view: false,
    };
    let alter = |confirmed| Command::AlterSchema {
        request: 21,
        conn: id,
        change: drop.clone(),
        confirmed,
    };
    engine.send(alter(false)).unwrap();
    let Event::SchemaReview {
        statements,
        warning,
        ..
    } = next_matching(&events, |e| {
        matches!(e, Event::SchemaReview { request: 21, .. })
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(
        statements,
        vec!["DROP TABLE `shop`.`nokey_scratch`".to_owned()]
    );
    assert!(warning.is_some());
    engine.send(alter(true)).unwrap();
    let Event::SchemaChanged { error, .. } = next_matching(&events, |e| {
        matches!(e, Event::SchemaChanged { request: 21, .. })
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    // Dropping it again fails: it is gone. Each statement is recorded as it runs, so the
    // failed one reaches the history before the change reports back.
    engine.send(alter(true)).unwrap();
    next_matching(
        &events,
        |e| matches!(e, Event::HistoryAdded { entry } if entry.error.is_some()),
    )
    .await;
    let Event::SchemaChanged { error, .. } = next_matching(&events, |e| {
        matches!(e, Event::SchemaChanged { request: 21, .. })
    })
    .await
    else {
        unreachable!()
    };
    assert!(error.is_some());

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn filters_counts_columns_and_plans() {
    use quarry_model::{ColumnFilter, FilterOp, SortKey, Value};
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;
    engine.send(Command::Connect { id }).unwrap();
    let Event::ConnState { state, .. } = next_matching(&events, |e| {
        matches!(
            e,
            Event::ConnState {
                state: ConnState::Connected { .. },
                ..
            }
        )
    })
    .await
    else {
        unreachable!()
    };
    assert!(matches!(state, ConnState::Connected { capabilities, .. } if capabilities.explain));

    let table = TableRef {
        database: Some("shop".into()),
        schema: None,
        name: "orders".into(),
    };
    let filter = |column: &str, op, value: &str| ColumnFilter {
        column: column.into(),
        op,
        value: value.into(),
    };
    let paid = BrowseQuery {
        filters: vec![filter("status", FilterOp::Eq, "paid")],
        sort: vec![SortKey {
            column: "id".into(),
            descending: true,
        }],
    };
    engine
        .send(Command::Browse {
            tab: 4,
            conn: id,
            table: table.clone(),
            limit: 3,
            offset: 0,
            query: paid.clone(),
        })
        .unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 4, .. })).await
    else {
        unreachable!()
    };
    // Order n has status ELT(1 + n % 4, …): paid when n % 4 = 1, so the newest is 4997.
    let ids: Vec<&Value> = rows.iter().map(|r| &r[0]).collect();
    assert_eq!(
        ids,
        [&Value::UInt(4997), &Value::UInt(4993), &Value::UInt(4989)]
    );
    assert!(rows.iter().all(|r| r[2] == Value::Text("paid".into())));
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 4, .. })
    })
    .await;

    let count = |query: BrowseQuery| Command::CountRows {
        tab: 4,
        conn: id,
        table: table.clone(),
        query,
    };
    engine.send(count(paid)).unwrap();
    let event = next_matching(&events, |e| matches!(e, Event::RowCount { tab: 4, .. })).await;
    assert!(
        matches!(
            event,
            Event::RowCount {
                count: Some(1250),
                ..
            }
        ),
        "{event:?}"
    );
    // `%` and `_` in the text match literally, so nothing matches.
    let literal = BrowseQuery {
        filters: vec![
            filter("note", FilterOp::Contains, "wrap #7_"),
            filter("note", FilterOp::IsNotNull, ""),
        ],
        sort: vec![],
    };
    engine.send(count(literal)).unwrap();
    let event = next_matching(&events, |e| matches!(e, Event::RowCount { tab: 4, .. })).await;
    assert!(
        matches!(event, Event::RowCount { count: Some(0), .. }),
        "{event:?}"
    );

    engine
        .send(Command::LoadColumns {
            id,
            database: "shop".into(),
        })
        .unwrap();
    let Event::Columns { columns, error, .. } =
        next_matching(&events, |e| matches!(e, Event::Columns { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    assert!(
        columns
            .unwrap()
            .iter()
            .any(|t| t.table == "customers" && t.columns.contains(&"email".to_owned()))
    );

    let explain = |sql: &str, json: bool| Command::Explain {
        tab: 4,
        conn: id,
        database: Some("shop".into()),
        sql: sql.into(),
        json,
    };
    engine
        .send(explain("DELETE FROM orders WHERE id = 1", true))
        .unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 4, .. })).await
    else {
        unreachable!()
    };
    assert!(matches!(&rows[0][0], Value::Text(t) | Value::Json(t) if t.contains("query_block")));
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 4, .. })
    })
    .await;
    engine.send(count(BrowseQuery::default())).unwrap();
    let event = next_matching(&events, |e| matches!(e, Event::RowCount { tab: 4, .. })).await;
    assert!(
        matches!(
            event,
            Event::RowCount {
                count: Some(5000),
                ..
            }
        ),
        "explaining a DELETE must not run it: {event:?}"
    );
    engine.send(explain("SELECT 1; SELECT 2", false)).unwrap();
    next_matching(&events, |e| matches!(e, Event::QueryFailed { tab: 4, .. })).await;
    engine
        .send(explain("ANALYZE DELETE FROM orders", false))
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::QueryFailed { tab: 4, .. })).await;

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn closing_a_busy_tab_stops_its_query() {
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;
    engine
        .send(Command::Run {
            tab: 8,
            conn: id,
            database: None,
            sql: "SELECT SLEEP(30), 1".into(),
            confirmed: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::QueryStarted { tab: 8 })).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = std::time::Instant::now();
    engine.send(Command::CloseTab { tab: 8 }).unwrap();
    next_matching(&events, |e| {
        matches!(
            e,
            Event::QueryFinished { tab: 8, .. } | Event::QueryFailed { tab: 8, .. }
        )
    })
    .await;
    assert!(started.elapsed() < Duration::from_secs(5));
    next_matching(
        &events,
        |e| matches!(e, Event::HistoryAdded { entry } if entry.sql.contains("SLEEP(30)")),
    )
    .await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn transactions_follow_commands_and_sql() {
    use quarry_model::{CellValue, Value};
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;

    let run = |sql: &str| Command::Run {
        tab: 6,
        conn: id,
        database: Some("shop".into()),
        sql: sql.into(),
        confirmed: false,
    };
    let finished = |e: &Event| matches!(e, Event::QueryFinished { tab: 6, .. });
    let state = |e: &Event| matches!(e, Event::TransactionState { tab: 6, .. });
    engine
        .send(run("DROP TABLE IF EXISTS tx_scratch; \
             CREATE TABLE tx_scratch (id INT PRIMARY KEY, name VARCHAR(20) NULL);"))
        .unwrap();
    next_matching(&events, finished).await;

    engine
        .send(Command::BeginTransaction {
            tab: 6,
            conn: id,
            database: Some("shop".into()),
        })
        .unwrap();
    let event = next_matching(&events, state).await;
    assert!(
        matches!(
            event,
            Event::TransactionState {
                open: true,
                error: None,
                ..
            }
        ),
        "{event:?}"
    );
    engine
        .send(run("INSERT INTO tx_scratch VALUES (1, 'one')"))
        .unwrap();
    next_matching(&events, finished).await;
    // Grid edits inside the transaction stay uncommitted.
    engine
        .send(Command::ApplyChanges {
            tab: 6,
            conn: id,
            table: TableRef {
                database: Some("shop".into()),
                schema: None,
                name: "tx_scratch".into(),
            },
            changes: vec![RowChange::Insert {
                values: vec![CellValue {
                    column: "id".into(),
                    value: Value::Int(2),
                }],
            }],
            confirmed: false,
        })
        .unwrap();
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::ChangesApplied { tab: 6, .. } | Event::ChangesFailed { tab: 6, .. }
        )
    })
    .await;
    assert!(
        matches!(&event, Event::ChangesApplied { message, .. } if message.contains("open transaction")),
        "{event:?}"
    );
    engine
        .send(Command::EndTransaction {
            tab: 6,
            commit: false,
        })
        .unwrap();
    let event = next_matching(&events, state).await;
    assert!(
        matches!(event, Event::TransactionState { open: false, .. }),
        "{event:?}"
    );
    engine.send(run("SELECT COUNT(*) FROM tx_scratch")).unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 6, .. })).await
    else {
        unreachable!()
    };
    assert_eq!(rows[0][0], Value::Int(0), "the rollback undid both inserts");
    next_matching(&events, finished).await;

    // Typed BEGIN and COMMIT are tracked too.
    engine
        .send(run("BEGIN; INSERT INTO tx_scratch VALUES (3, 'three')"))
        .unwrap();
    let event = next_matching(&events, state).await;
    assert!(matches!(event, Event::TransactionState { open: true, .. }));
    engine.send(run("COMMIT")).unwrap();
    let event = next_matching(&events, state).await;
    assert!(matches!(event, Event::TransactionState { open: false, .. }));

    engine.send(run("DROP TABLE tx_scratch")).unwrap();
    next_matching(
        &events,
        |e| matches!(e, Event::HistoryAdded { entry } if entry.sql == "DROP TABLE tx_scratch"),
    )
    .await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// Over the SSH test server (`QUARRY_TEST_SSH`), which forwards to the `mysql` service.
#[tokio::test]
async fn connects_through_an_ssh_tunnel() {
    use quarry_model::{SshAuth, SshConfig};
    let (Some(mut config), Ok(ssh)) = (config(), std::env::var("QUARRY_TEST_SSH")) else {
        return;
    };
    let Some((ssh_host, ssh_port)) = ssh.split_once(':') else {
        return;
    };
    let ssh_port: u16 = ssh_port.parse().unwrap();
    let Ok(scan) = std::process::Command::new("ssh-keyscan")
        .args(["-p", &ssh_port.to_string(), ssh_host])
        .output()
    else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let known_hosts = dir.join("known_hosts");
    let empty_known_hosts = dir.join("empty_known_hosts");
    std::fs::write(&known_hosts, &scan.stdout).unwrap();
    std::fs::write(&empty_known_hosts, "").unwrap();

    // The database as seen from the SSH server.
    config.host = Some("mysql".into());
    config.port = Some(3306);
    config.ssh = Some(SshConfig {
        host: ssh_host.into(),
        port: ssh_port,
        user: "quarry".into(),
        auth: SshAuth::Password,
        key_path: None,
    });
    let id = config.id;

    for (hosts, works) in [(&empty_known_hosts, false), (&known_hosts, true)] {
        let (engine, events) = EngineHandle::spawn_with(EngineOptions {
            config_dir: Some(dir.clone()),
            data_dir: Some(dir.clone()),
            known_hosts: Some(hosts.clone()),
            disable_keyring: true,
            perf_log: None,
        })
        .unwrap();
        engine
            .send(Command::SaveConnection {
                config: config.clone(),
                password: Some(Password("quarry".into())),
                ssh_secret: Some(Password("quarry".into())),
                remember_password: false,
            })
            .unwrap();
        next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;
        engine.send(Command::Connect { id }).unwrap();
        let event = next_matching(&events, |e| {
            matches!(
                e,
                Event::Databases { .. }
                    | Event::ConnState {
                        state: ConnState::Failed { .. },
                        ..
                    }
            )
        })
        .await;
        if works {
            let Event::Databases { databases, .. } = event else {
                panic!("expected databases, got {event:?}")
            };
            assert!(databases.contains(&"shop".to_owned()));

            // A query tab shares the tunnel.
            engine
                .send(Command::Run {
                    tab: 1,
                    conn: id,
                    database: Some("shop".into()),
                    sql: "SELECT COUNT(*) FROM customers".into(),
                    confirmed: false,
                })
                .unwrap();
            let Event::ResultRows { rows, .. } =
                next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 1, .. })).await
            else {
                unreachable!()
            };
            assert_eq!(rows[0][0], quarry_model::Value::Int(5));
            next_matching(&events, |e| matches!(e, Event::HistoryAdded { .. })).await;
        } else {
            let Event::ConnState {
                state: ConnState::Failed { error },
                ..
            } = event
            else {
                panic!("expected a failure, got {event:?}")
            };
            assert!(error.contains("not a known host"), "{error}");
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// An engine with `config` saved (password `quarry`), not connected yet.
async fn saved_engine(config: ConnectionConfig) -> (EngineHandle, EngineEvents) {
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;
    (engine, events)
}

/// Collects the run's events until it finishes or fails.
async fn run_events(
    engine: &EngineHandle,
    events: &EngineEvents,
    tab: TabId,
    conn: ConnId,
    sql: &str,
) -> Vec<Event> {
    engine
        .send(Command::Run {
            tab,
            conn,
            database: Some("shop".into()),
            sql: sql.into(),
            confirmed: false,
        })
        .unwrap();
    let mut seen = Vec::new();
    loop {
        let event = next_matching(events, |e| match e {
            Event::QueryStarted { tab: t }
            | Event::StatementStarted { tab: t, .. }
            | Event::ResultColumns { tab: t, .. }
            | Event::ResultRows { tab: t, .. }
            | Event::ResultDone { tab: t, .. }
            | Event::StatementLog { tab: t, .. }
            | Event::QueryFinished { tab: t, .. }
            | Event::QueryFailed { tab: t, .. } => *t == tab,
            _ => false,
        })
        .await;
        let end = matches!(
            event,
            Event::QueryFinished { .. } | Event::QueryFailed { .. }
        );
        seen.push(event);
        if end {
            return seen;
        }
    }
}

#[tokio::test]
async fn bad_sql_keeps_the_tab_usable_and_names_the_statement() {
    let Some(config) = config() else { return };
    let id = config.id;
    let (engine, events) = saved_engine(config).await;

    // One statement per run, in order; the failing one is named with its location.
    let seen = run_events(&engine, &events, 5, id, "SELECT 1;\nSELEC 2;\nSELECT 3").await;
    let started: Vec<(u32, u32)> = seen
        .iter()
        .filter_map(|e| match e {
            Event::StatementStarted { index, count, .. } => Some((*index, *count)),
            _ => None,
        })
        .collect();
    assert_eq!(
        started,
        [(0, 3), (1, 3)],
        "the third statement never starts"
    );
    // The console gets one entry per statement that ran, the failing one with its error.
    let logged: Vec<&LogEntry> = seen
        .iter()
        .filter_map(|e| match e {
            Event::StatementLog { entry, .. } => Some(entry),
            _ => None,
        })
        .collect();
    let [first, second] = logged.as_slice() else {
        panic!("{logged:?}")
    };
    assert_eq!(
        (first.sql.as_str(), first.rows, first.error.as_deref()),
        ("SELECT 1", Some(1), None)
    );
    assert_eq!(first.source, LogSource::Query);
    assert!(first.fetch_ms.is_some());
    assert_eq!(second.sql, "SELEC 2");
    assert!(
        second
            .error
            .as_deref()
            .is_some_and(|e| e.contains("syntax")),
        "{second:?}"
    );
    assert!(seen.iter().any(|e| matches!(
        e,
        Event::ResultColumns {
            statement: 0,
            set: 0,
            ..
        }
    )));
    let Some(Event::QueryFailed {
        statement,
        location,
        error,
        ..
    }) = seen.last()
    else {
        panic!("{seen:?}")
    };
    assert_eq!(*statement, Some(1), "{error}");
    let location = location.as_ref().expect("syntax errors have a location");
    assert_eq!((location.line, location.near.as_str()), (1, "SELEC 2"));

    // The next run on the same tab gets its own result, not the old error.
    let seen = run_events(&engine, &events, 5, id, "SELECT 42").await;
    assert!(
        matches!(seen.last(), Some(Event::QueryFinished { .. })),
        "{seen:?}"
    );

    // A dead connection is replaced once, and the tab works again.
    let seen = run_events(&engine, &events, 5, id, "SELECT CONNECTION_ID()").await;
    let connection = seen
        .iter()
        .find_map(|e| match e {
            Event::ResultRows { rows, .. } => Some(rows[0][0].display(64)),
            _ => None,
        })
        .expect("the connection id");
    let kill = format!("KILL {connection}");
    let seen = run_events(&engine, &events, 6, id, &kill).await;
    assert!(
        matches!(seen.last(), Some(Event::QueryFinished { .. })),
        "{seen:?}"
    );
    let seen = run_events(&engine, &events, 5, id, "SELECT 1").await;
    let Some(Event::QueryFailed { error, .. }) = seen.last() else {
        panic!("a killed connection fails the next run: {seen:?}")
    };
    assert!(error.contains("new connection"), "{error}");
    let seen = run_events(&engine, &events, 5, id, "SELECT 1").await;
    assert!(
        matches!(seen.last(), Some(Event::QueryFinished { .. })),
        "{seen:?}"
    );
}
