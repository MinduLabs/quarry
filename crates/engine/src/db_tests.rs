//! Integration tests that every SQL database passes, through its driver binary. Each runs when
//! its `QUARRY_TEST_*` address is set (`just test-db`); MySQL has its own, longer set.

use quarry_model::{BrowseQuery, CellValue, ColumnFilter, FilterOp, RowChange, TableRef, Value};

use super::*;
use crate::test_support::{db_config, next_matching, saved_engine};

/// What differs between the databases for the same test.
struct TestDb {
    env: &'static str,
    driver: &'static str,
    user: &'static str,
    password: &'static str,
    version_prefix: &'static str,
    /// The tree's "database" a tab works in: a schema of the connection's database.
    schema: &'static str,
    /// Creates `edit_scratch (id, name, born)` with rows 1 and 2.
    setup: &'static str,
}

const POSTGRES: TestDb = TestDb {
    env: "QUARRY_TEST_POSTGRES",
    driver: "postgres",
    user: "quarry",
    password: "quarry",
    version_prefix: "17",
    schema: "public",
    setup: "DROP TABLE IF EXISTS edit_scratch; \
        CREATE TABLE edit_scratch (id int PRIMARY KEY, name text, born date); \
        INSERT INTO edit_scratch VALUES (1, 'one', '2000-01-01'), (2, 'two', NULL);",
};

const SQLSERVER: TestDb = TestDb {
    env: "QUARRY_TEST_SQLSERVER",
    driver: "sqlserver",
    user: "sa",
    password: "Quarry-test-1",
    version_prefix: "16.",
    schema: "dbo",
    setup: "DROP TABLE IF EXISTS edit_scratch; \
        CREATE TABLE edit_scratch (id int PRIMARY KEY, name nvarchar(20), born date); \
        INSERT INTO edit_scratch VALUES (1, N'one', '2000-01-01'), (2, N'two', NULL);",
};

#[tokio::test]
async fn connect_browse_and_edit_a_postgres_table() {
    connect_browse_and_edit(&POSTGRES).await;
}

#[tokio::test]
async fn connect_browse_and_edit_a_sqlserver_table() {
    connect_browse_and_edit(&SQLSERVER).await;
}

async fn connect_browse_and_edit(db: &TestDb) {
    let Some(config) = db_config(db.env, db.driver, db.user, Some("shop")) else {
        return;
    };
    let id = config.id;
    let (engine, events, dir) = saved_engine(config, db.password).await;

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
        matches!(&event, Event::ConnState { state: ConnState::Connected { server_version, .. }, .. } if server_version.starts_with(db.version_prefix)),
        "{event:?}"
    );

    let run = |sql: &str| Command::Run {
        tab: 1,
        conn: id,
        database: Some(db.schema.into()),
        sql: sql.into(),
        confirmed: false,
    };
    engine.send(run(db.setup)).unwrap();
    let event = next_matching(&events, |e| {
        matches!(
            e,
            Event::QueryFinished { tab: 1, .. } | Event::QueryFailed { tab: 1, .. }
        )
    })
    .await;
    assert!(matches!(event, Event::QueryFinished { .. }), "{event:?}");

    let table = TableRef {
        database: Some(db.schema.into()),
        schema: None,
        name: "edit_scratch".into(),
    };
    // A page with a filter, in the driver's quoting, parameters and paging.
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
