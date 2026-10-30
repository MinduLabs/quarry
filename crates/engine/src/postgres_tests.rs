//! Integration tests against a real PostgreSQL (`QUARRY_TEST_POSTGRES=127.0.0.1:15432`), through
//! the driver binary.

use std::time::Duration;

use quarry_model::{
    BrowseQuery, CellValue, ColumnFilter, ConnId, ConnectionConfig, DriverId, Environment,
    FilterOp, RowChange, SslMode, TableRef, Value,
};

use super::*;

fn config() -> Option<ConnectionConfig> {
    let addr = std::env::var("QUARRY_TEST_POSTGRES").ok()?;
    let (host, port) = addr.split_once(':')?;
    Some(ConnectionConfig {
        id: ConnId::new(),
        name: "Local PostgreSQL".into(),
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
async fn connect_browse_and_edit_a_postgres_table() {
    let Some(config) = config() else { return };
    let id = config.id;
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
        driver_dir: None,
        driver_manifest: None,
        in_process_drivers: Default::default(),
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
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::ConnState {
                state: ConnState::Connected { .. } | ConnState::Failed { .. },
                ..
            }
        )
    })
    .await;
    assert!(
        matches!(&event, Event::ConnState { state: ConnState::Connected { server_version, .. }, .. } if server_version.starts_with("17")),
        "{event:?}"
    );

    let run = |sql: &str| Command::Run {
        tab: 1,
        conn: id,
        database: Some("public".into()),
        sql: sql.into(),
        confirmed: false,
    };
    engine
        .send(run("DROP TABLE IF EXISTS edit_scratch; \
             CREATE TABLE edit_scratch (id int PRIMARY KEY, name text, born date); \
             INSERT INTO edit_scratch VALUES (1, 'one', '2000-01-01'), (2, 'two', NULL);"))
        .unwrap();
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::QueryFinished { tab: 1, .. } | Event::QueryFailed { tab: 1, .. }
        )
    })
    .await;
    assert!(matches!(event, Event::QueryFinished { .. }), "{event:?}");

    let table = TableRef {
        database: Some("public".into()),
        schema: None,
        name: "edit_scratch".into(),
    };
    // A page with a filter: quoted names and a numbered parameter.
    engine
        .send(Command::Browse {
            tab: 2,
            conn: id,
            table: table.clone(),
            limit: 100,
            offset: 0,
            query: BrowseQuery {
                filters: vec![ColumnFilter {
                    column: "name".into(),
                    op: FilterOp::Eq,
                    value: "two".into(),
                }],
                sort: Vec::new(),
            },
        })
        .unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 2, .. })).await
    else {
        unreachable!()
    };
    assert_eq!(
        rows.values().unwrap(),
        vec![vec![Value::Int(2), Value::Text("two".into()), Value::Null]]
    );
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 2, .. })
    })
    .await;

    // Grid edits hold text for a number and a date; the server reads them as the column types.
    let cell = |column: &str, value: Value| CellValue {
        column: column.into(),
        value,
    };
    engine
        .send(Command::ApplyChanges {
            tab: 2,
            conn: id,
            table: table.clone(),
            changes: vec![
                RowChange::Update {
                    key: vec![cell("id", Value::Int(2))],
                    set: vec![cell("born", Value::Text("1999-12-31".into()))],
                },
                RowChange::Insert {
                    values: vec![
                        cell("id", Value::Text("3".into())),
                        cell("name", Value::Null),
                    ],
                },
            ],
            confirmed: false,
        })
        .unwrap();
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::ChangesApplied { tab: 2, .. } | Event::ChangesFailed { tab: 2, .. }
        )
    })
    .await;
    assert!(matches!(event, Event::ChangesApplied { .. }), "{event:?}");

    engine
        .send(run("SELECT id, born FROM edit_scratch ORDER BY id"))
        .unwrap();
    let Event::ResultRows { rows, .. } =
        next_matching(&events, |e| matches!(e, Event::ResultRows { tab: 1, .. })).await
    else {
        unreachable!()
    };
    let rows = rows.values().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1][1].display(64), "1999-12-31");
    next_matching(&events, |e| {
        matches!(e, Event::QueryFinished { tab: 1, .. })
    })
    .await;

    engine.send(run("DROP TABLE edit_scratch")).unwrap();
    next_matching(
        &events,
        |e| matches!(e, Event::HistoryAdded { entry } if entry.sql == "DROP TABLE edit_scratch"),
    )
    .await;
    std::fs::remove_dir_all(dir).unwrap();
}
