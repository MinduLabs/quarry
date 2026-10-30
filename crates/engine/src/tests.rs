use quarry_model::{
    ConnId, ConnectionConfig, Environment, Favorite, RowChange, TableRef, Workspace,
};

use super::*;
use crate::query::{safety_check, single_statement};
use crate::session::transaction_after;

fn test_engine() -> (EngineHandle, EngineEvents) {
    let dir = crate::test_support::temp_dir();
    EngineHandle::spawn_with(EngineOptions {
        // A directory without driver binaries, so `QUARRY_DRIVER_DIR` cannot add any.
        driver_dir: Some(dir.clone()),
        in_process_drivers: InProcessDrivers(vec![
            Arc::new(quarry_driver_mysql::MysqlDriver::default()),
            Arc::new(quarry_driver_mysql::MysqlDriver::mariadb()),
        ]),
        ..crate::test_support::options(&dir)
    })
    .unwrap()
}

#[test]
fn safe_mode_rules() {
    use Environment::*;
    assert_eq!(safety_check(Local, "DROP TABLE t"), None);
    assert_eq!(
        safety_check(Staging, "UPDATE t SET a = 1 WHERE id = 1"),
        None
    );
    assert!(safety_check(Staging, "SELECT 1; DELETE FROM t").is_some());
    assert_eq!(safety_check(Production, "SELECT 1; SHOW TABLES"), None);
    assert!(safety_check(Production, "INSERT INTO t VALUES (1)").is_some());
    assert!(safety_check(Production, "ALTER TABLE t ADD c INT").is_some());
    // A `DELIMITER` line is not a statement, and the statements after it are still checked.
    assert_eq!(
        safety_check(
            Production,
            "DELIMITER //\nSELECT 1 //\nDELIMITER ;\nSHOW TABLES"
        ),
        None
    );
    assert!(safety_check(Staging, "DELIMITER //\nSELECT 1 //\nTRUNCATE t //").is_some());
}

#[test]
fn explain_takes_one_plain_statement() {
    assert_eq!(single_statement("  SELECT 1;  "), Ok("SELECT 1"));
    assert!(single_statement("SELECT 1; SELECT 2").is_err());
    assert!(single_statement("").is_err());
    for already in [
        "EXPLAIN SELECT 1",
        "analyze delete from t",
        "DESC t",
        "describe t",
    ] {
        assert!(single_statement(already).is_err(), "{already}");
    }
    // What the engine sends is always a read, so safe mode never needs to ask.
    for statement in [
        "DELETE FROM t",
        "DROP TABLE t",
        "/* x */ UPDATE t SET a = 1",
    ] {
        let sql = format!("EXPLAIN {}", single_statement(statement).unwrap());
        assert_eq!(quarry_sql::classify(&sql), quarry_sql::StatementKind::Read);
    }
}

#[test]
fn transaction_state_follows_statements() {
    assert!(transaction_after(false, "BEGIN; INSERT INTO t VALUES (1)"));
    assert!(!transaction_after(true, "INSERT INTO t VALUES (1); COMMIT"));
    assert!(transaction_after(
        true,
        "SAVEPOINT a; ROLLBACK TO SAVEPOINT a"
    ));
    assert!(!transaction_after(true, "ALTER TABLE t ADD c INT"));
    assert!(!transaction_after(false, "SELECT 'BEGIN'"));
}

#[test]
fn commands_and_events_use_tagged_json() {
    let command: Command =
        serde_json::from_str(r#"{"type":"Cancel","tab":3}"#).expect("valid command");
    assert!(matches!(command, Command::Cancel { tab: 3 }));
    let json = serde_json::to_string(&Event::QueryStarted { tab: 3 }).expect("serializable");
    assert_eq!(json, r#"{"type":"QueryStarted","tab":3}"#);
}

#[tokio::test]
async fn ping_pong() {
    let (engine, events) = test_engine();
    engine.send(Command::Ping { id: 7 }).unwrap();
    match events.recv().await.unwrap() {
        Event::Pong { id, version, .. } => {
            assert_eq!(id, 7);
            assert_eq!(version, VERSION);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn data_sources_mark_what_this_build_can_open() {
    let (engine, events) = test_engine();
    engine.send(Command::LoadDataSources).unwrap();
    let Event::DataSources { sources } = events.recv().await.unwrap() else {
        panic!("expected DataSources");
    };
    let find = |id: &str| sources.iter().find(|s| s.id == id).expect(id);
    for id in ["mysql", "mariadb"] {
        let source = find(id);
        assert_eq!(source.driver.as_ref().map(|d| d.0.as_str()), Some(id));
        assert!(source.is_installed);
    }
    // Quarry has a driver for these, not yet downloaded here.
    for (id, driver) in [("postgresql", "postgres"), ("sqlserver", "sqlserver")] {
        let source = find(id);
        assert_eq!(source.driver.as_ref().map(|d| d.0.as_str()), Some(driver));
        assert!(!source.is_installed, "{id}");
    }
    // Coming soon.
    for id in ["aws", "google-cloud", "demo-sqlite", "demo-mysql"] {
        let source = find(id);
        assert_eq!(source.driver, None, "{id}");
        assert!(!source.is_installed, "{id}");
    }
}

#[tokio::test]
async fn production_edits_ask_first_and_read_only_refuses() {
    use quarry_model::{CellValue, DriverId, SslMode, Value};
    let (engine, events) = test_engine();
    let mut config = ConnectionConfig {
        id: ConnId::new(),
        name: "prod".into(),
        driver: DriverId("mysql".into()),
        host: Some("127.0.0.1".into()),
        port: Some(1),
        database: None,
        user: None,
        path: None,
        ssl: SslMode::Disable,
        ssh: None,
        environment: Environment::Production,
        read_only: false,
        color: None,
        group: None,
    };
    let change = || ApplyChangesFixture {
        key: vec![CellValue {
            column: "id".into(),
            value: Value::Int(1),
        }],
    };
    engine
        .send(Command::SaveConnection {
            config: config.clone(),
            password: None,
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Event::Connections { .. }
    ));
    engine.send(change().command(config.id)).unwrap();
    match events.recv().await.unwrap() {
        Event::ChangesConfirmRequired { tab, message } => {
            assert_eq!(tab, 5);
            assert!(message.contains("1 row deleted"), "{message}");
        }
        other => panic!("unexpected {other:?}"),
    }

    config.read_only = true;
    engine
        .send(Command::SaveConnection {
            config: config.clone(),
            password: None,
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Event::Connections { .. }
    ));
    engine.send(change().command(config.id)).unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Event::ChangesFailed { tab: 5, .. }
    ));
}

struct ApplyChangesFixture {
    key: Vec<quarry_model::CellValue>,
}

impl ApplyChangesFixture {
    fn command(self, conn: ConnId) -> Command {
        Command::ApplyChanges {
            tab: 5,
            conn,
            table: TableRef {
                database: Some("shop".into()),
                schema: None,
                name: "orders".into(),
            },
            changes: vec![RowChange::Delete { key: self.key }],
            confirmed: false,
        }
    }
}

#[tokio::test]
async fn favorites_are_saved_and_deleted() {
    use quarry_model::{FavoriteId, FavoriteItem};
    let (engine, events) = test_engine();
    let favorite = Favorite {
        id: FavoriteId::new(),
        name: "orders".into(),
        conn: ConnId::new(),
        database: Some("shop".into()),
        item: FavoriteItem::Table {
            table: "orders".into(),
        },
    };
    engine
        .send(Command::SaveFavorite {
            favorite: favorite.clone(),
        })
        .unwrap();
    match events.recv().await.unwrap() {
        Event::Favorites { favorites } => assert_eq!(favorites, vec![favorite.clone()]),
        other => panic!("unexpected {other:?}"),
    }
    engine.send(Command::LoadFavorites).unwrap();
    match events.recv().await.unwrap() {
        Event::Favorites { favorites } => assert_eq!(favorites, vec![favorite.clone()]),
        other => panic!("unexpected {other:?}"),
    }
    engine
        .send(Command::DeleteFavorite { id: favorite.id })
        .unwrap();
    match events.recv().await.unwrap() {
        Event::Favorites { favorites } => assert!(favorites.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn workspace_is_saved_and_restored() {
    let (engine, events) = test_engine();
    let workspace = Workspace {
        tabs: vec![quarry_model::SavedTab {
            title: "Query 1".into(),
            conn: ConnId::new(),
            database: None,
            sql: "SELECT 1".into(),
            browse: None,
            pinned: false,
        }],
        active: Some(0),
    };
    engine
        .send(Command::SaveWorkspace {
            request: 4,
            workspace: workspace.clone(),
        })
        .unwrap();
    match events.recv().await.unwrap() {
        Event::WorkspaceSaved { request, error } => {
            assert_eq!(request, 4);
            assert_eq!(error, None);
        }
        other => panic!("unexpected {other:?}"),
    }
    engine.send(Command::LoadWorkspace).unwrap();
    match events.recv().await.unwrap() {
        Event::Workspace { workspace: loaded } => assert_eq!(loaded, workspace),
        other => panic!("unexpected {other:?}"),
    }
}

// ---------- Export ----------

fn export_column(
    name: &str,
    table: Option<&str>,
    source: Option<&str>,
) -> quarry_model::ColumnMeta {
    quarry_model::ColumnMeta {
        name: name.into(),
        type_name: "TEXT".into(),
        nullable: None,
        source_database: table.map(|_| "shop".to_owned()),
        source_table: table.map(str::to_owned),
        source_column: source.map(str::to_owned),
    }
}

/// A saved connection that no server answers: enough for commands that never connect.
async fn saved_unreachable_connection(engine: &EngineHandle, events: &EngineEvents) -> ConnId {
    use quarry_model::{DriverId, SslMode};
    let config = ConnectionConfig {
        id: ConnId::new(),
        name: "nowhere".into(),
        driver: DriverId("mysql".into()),
        host: Some("127.0.0.1".into()),
        port: Some(1),
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
    let id = config.id;
    engine
        .send(Command::SaveConnection {
            config,
            password: None,
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Event::Connections { .. }
    ));
    id
}

fn export_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("quarry-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn export_rows_writes_what_it_is_given_and_opens_no_session() {
    use quarry_model::{ExportFormat, ExportOptions, Value};
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    let dir = export_dir();
    let path = dir.join("orders.csv");
    engine
        .send(Command::ExportRows {
            request: 9,
            conn,
            format: ExportFormat::Csv,
            options: ExportOptions::default(),
            columns: vec![
                export_column("id", None, None),
                export_column("note", None, None),
            ],
            rows: (1..=3)
                .map(|n| vec![Value::Int(n), Value::Text(format!("n{n}"))])
                .collect(),
            path: path.display().to_string(),
        })
        .unwrap();
    // Covers AE1: the next event is the answer, so nothing connected or ran a statement.
    match events.recv().await.unwrap() {
        Event::ExportFinished {
            request,
            rows,
            sheets,
            cut_cells,
            path: written,
        } => {
            assert_eq!((request, rows, sheets, cut_cells), (9, 3, 0, 0));
            assert_eq!(written, path.display().to_string());
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "id,note\r\n1,n1\r\n2,n2\r\n3,n3\r\n"
    );
    assert!(
        events.try_recv().is_err(),
        "no connection state or log event"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn export_preview_shows_what_the_file_holds() {
    use quarry_model::{ExportFormat, ExportOptions, TableRef, Value};
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    let dir = export_dir();
    let path = dir.join("orders.sql");
    let columns = vec![
        export_column("id", None, None),
        export_column("note", None, None),
    ];
    let rows = vec![
        vec![Value::Int(1), Value::Text("it's".into())],
        vec![Value::Int(2), Value::Null],
    ];
    let options = ExportOptions {
        table: Some(TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        }),
        ..ExportOptions::default()
    };
    engine
        .send(Command::ExportPreview {
            request: 1,
            conn,
            format: ExportFormat::SqlInserts,
            options: options.clone(),
            columns: columns.clone(),
            rows: rows.clone(),
        })
        .unwrap();
    let Event::ExportPreview { text, error, .. } = events.recv().await.unwrap() else {
        panic!("expected a preview");
    };
    assert_eq!(error, None);
    engine
        .send(Command::ExportRows {
            request: 2,
            conn,
            format: ExportFormat::SqlInserts,
            options,
            columns,
            rows,
            path: path.display().to_string(),
        })
        .unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Event::ExportFinished { .. }
    ));
    let file = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.as_deref(), Some(file.as_str()));
    assert!(file.contains("(1, 'it''s')"), "{file}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn export_to_a_missing_folder_fails_naming_it_and_creates_nothing() {
    use quarry_model::{ExportFormat, ExportOptions, Value};
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    let dir = export_dir();
    let missing = dir.join("nope");
    engine
        .send(Command::ExportRows {
            request: 3,
            conn,
            format: ExportFormat::Csv,
            options: ExportOptions::default(),
            columns: vec![export_column("id", None, None)],
            rows: vec![vec![Value::Int(1)]],
            path: missing.join("out.csv").display().to_string(),
        })
        .unwrap();
    match events.recv().await.unwrap() {
        Event::ExportFailed { request, error, .. } => {
            assert_eq!(request, 3);
            assert!(
                error.contains("nope") && error.contains("Choose a folder"),
                "{error}"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!missing.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn export_refuses_an_empty_path_and_an_empty_result() {
    use quarry_model::{ExportFormat, ExportOptions, Value};
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    let send = |columns: Vec<quarry_model::ColumnMeta>, path: &str| {
        engine
            .send(Command::ExportRows {
                request: 4,
                conn,
                format: ExportFormat::Csv,
                options: ExportOptions::default(),
                columns,
                rows: vec![vec![Value::Int(1)]],
                path: path.to_owned(),
            })
            .unwrap();
    };
    send(vec![export_column("id", None, None)], "  ");
    match events.recv().await.unwrap() {
        Event::ExportFailed { error, .. } => assert!(error.contains("Choose a file"), "{error}"),
        other => panic!("unexpected {other:?}"),
    }
    send(Vec::new(), "/tmp/never.csv");
    match events.recv().await.unwrap() {
        Event::ExportFailed { error, .. } => {
            assert!(error.contains("nothing to export"), "{error}")
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn excel_has_no_preview() {
    use quarry_model::{ExportFormat, ExportOptions, Value};
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    engine
        .send(Command::ExportPreview {
            request: 5,
            conn,
            format: ExportFormat::Xlsx,
            options: ExportOptions::default(),
            columns: vec![export_column("id", None, None)],
            rows: vec![vec![Value::Int(1)]],
        })
        .unwrap();
    match events.recv().await.unwrap() {
        Event::ExportPreview { text, error, .. } => {
            assert_eq!(text, None);
            assert!(error.unwrap().contains("no preview"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_join_cannot_export_updates_but_keeps_the_other_formats() {
    use quarry_model::ExportFormat;
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    engine
        .send(Command::ExportPrepare {
            request: 6,
            conn,
            source: ExportSource::Rows {
                columns: vec![
                    export_column("id", Some("orders"), Some("id")),
                    export_column("name", Some("customers"), Some("name")),
                ],
            },
        })
        .unwrap();
    // Covers AE2. No lookup was needed, so the answer is the next event.
    let Event::ExportPrepared {
        formats,
        table,
        error,
        ..
    } = events.recv().await.unwrap()
    else {
        panic!("expected the prepared formats");
    };
    assert_eq!((table, error), (None, None));
    let state = |format| formats.iter().find(|f| f.format == format).unwrap();
    for blocked in [ExportFormat::SqlUpdates, ExportFormat::WhereClause] {
        assert!(!state(blocked).is_available, "{blocked:?}");
        assert!(
            state(blocked)
                .reason
                .as_deref()
                .unwrap()
                .contains("several tables")
        );
    }
    for open in [
        ExportFormat::Csv,
        ExportFormat::Xlsx,
        ExportFormat::SqlInserts,
    ] {
        assert!(state(open).is_available, "{open:?}");
    }
    let order: Vec<ExportFormat> = formats.iter().map(|f| f.format).collect();
    assert_eq!(
        order,
        [
            ExportFormat::SqlInserts,
            ExportFormat::SqlUpdates,
            ExportFormat::WhereClause,
            ExportFormat::Xlsx,
            ExportFormat::Csv
        ]
    );
}

#[tokio::test]
async fn updates_stay_unavailable_with_a_reason_when_the_keys_cannot_be_read() {
    use quarry_model::ExportFormat;
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    engine
        .send(Command::ExportPrepare {
            request: 7,
            conn,
            source: ExportSource::Rows {
                columns: vec![export_column("id", Some("orders"), Some("id"))],
            },
        })
        .unwrap();
    let prepared = loop {
        match events.recv().await.unwrap() {
            Event::ExportPrepared {
                formats,
                table,
                error,
                ..
            } => break (formats, table, error),
            _ => continue, // the connection attempt reports its own state
        }
    };
    let (formats, table, error) = prepared;
    assert_eq!(error, None);
    assert_eq!(table.map(|t| t.name), Some("orders".to_owned()));
    let updates = formats
        .iter()
        .find(|f| f.format == ExportFormat::SqlUpdates)
        .unwrap();
    assert!(!updates.is_available);
    assert!(updates.reason.as_deref().unwrap().contains("orders"));
    let csv = formats
        .iter()
        .find(|f| f.format == ExportFormat::Csv)
        .unwrap();
    assert!(csv.is_available);
}

#[test]
fn export_commands_and_events_use_tagged_json() {
    let command: Command = serde_json::from_str(
        r#"{"type":"ExportPrepare","request":1,"conn":"00000000-0000-0000-0000-000000000000",
            "source":{"kind":"Table","table":{"database":"shop","schema":null,"name":"orders"},
            "query":{}}}"#,
    )
    .expect("valid command");
    assert!(matches!(
        command,
        Command::ExportPrepare {
            source: ExportSource::Table { .. },
            ..
        }
    ));
    let json = serde_json::to_string(&Event::ExportFailed {
        request: 2,
        error: "x".into(),
        cancelled: false,
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"type":"ExportFailed","request":2,"error":"x","cancelled":false}"#
    );
}

#[tokio::test]
async fn exporting_every_row_of_a_query_warns_about_session_state() {
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    let prepare = |request, sql: &str| {
        engine
            .send(Command::ExportPrepare {
                request,
                conn,
                source: ExportSource::Query {
                    sql: sql.into(),
                    columns: vec![export_column("n", None, None)],
                },
            })
            .unwrap();
    };
    prepare(1, "SELECT n FROM numbers WHERE n > @min");
    let Event::ExportPrepared {
        formats,
        warnings,
        error,
        ..
    } = events.recv().await.unwrap()
    else {
        panic!("expected the prepared formats");
    };
    assert_eq!(error, None);
    assert_eq!(formats.len(), 5, "the formats of a result on the grid");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("@name"), "{warnings:?}");

    prepare(2, "SELECT n FROM numbers");
    let Event::ExportPrepared { warnings, .. } = events.recv().await.unwrap() else {
        panic!("expected the prepared formats");
    };
    assert!(warnings.is_empty());
}

#[tokio::test]
async fn exporting_every_row_refuses_a_statement_that_writes() {
    let (engine, events) = test_engine();
    let conn = saved_unreachable_connection(&engine, &events).await;
    for (request, sql, reason) in [
        (1, "DELETE FROM numbers WHERE n = 1", "reads"),
        (2, "SELECT 1; SELECT 2", "one statement"),
    ] {
        engine
            .send(Command::ExportPrepare {
                request,
                conn,
                source: ExportSource::Query {
                    sql: sql.into(),
                    columns: vec![export_column("n", None, None)],
                },
            })
            .unwrap();
        let Event::ExportPrepared { formats, error, .. } = events.recv().await.unwrap() else {
            panic!("expected an answer");
        };
        assert!(formats.is_empty(), "{sql}");
        assert!(error.unwrap().contains(reason), "{sql}");
    }
}

/// The JSON the UI sends for each export command (the shapes of `ui/src/bindings`), so a
/// renamed field or a different tag fails here and not only in the app.
#[test]
fn export_commands_parse_as_the_ui_sends_them() {
    let conn = "00000000-0000-0000-0000-000000000000";
    let options = r#"{"delimiter":",","has_header":true,"has_bom":false,"max_rows_per_sheet":null,"table":null,"key":[]}"#;
    let column = r#"{"name":"id","type_name":"INT","nullable":false,"source_database":"shop","source_table":"orders","source_column":"id"}"#;
    let parse = |json: String| serde_json::from_str::<Command>(&json).expect(&json);

    assert!(matches!(
        parse(format!(
            r#"{{"type":"ExportRows","request":1,"conn":"{conn}","format":"csv","options":{options},
                "columns":[{column}],"rows":[[{{"t":"int","v":"1"}}]],"path":"/tmp/a.csv"}}"#
        )),
        Command::ExportRows { .. }
    ));
    assert!(matches!(
        parse(format!(
            r#"{{"type":"ExportQuery","request":2,"conn":"{conn}","database":"shop","sql":"SELECT 1",
                "columns":[{column}],"format":"sql_inserts","options":{options},"path":"/tmp/a.sql",
                "limit":null}}"#
        )),
        Command::ExportQuery { limit: None, .. }
    ));
    assert!(matches!(
        parse(format!(
            r#"{{"type":"ExportTable","request":3,"conn":"{conn}",
                "table":{{"database":"shop","schema":null,"name":"orders"}},
                "query":{{"filters":[{{"column":"status","op":"eq","value":"paid"}}],"sort":[]}},
                "format":"xlsx","options":{options},"path":"/tmp/a.xlsx","limit":1000}}"#
        )),
        Command::ExportTable {
            limit: Some(1000),
            ..
        }
    ));
    assert!(matches!(
        parse(format!(
            r#"{{"type":"ExportCountRows","request":4,"conn":"{conn}",
                "table":{{"database":"shop","schema":null,"name":"orders"}},"query":{{"filters":[],"sort":[]}}}}"#
        )),
        Command::ExportCountRows { .. }
    ));
    assert!(matches!(
        parse(r#"{"type":"CancelExport","request":5}"#.to_owned()),
        Command::CancelExport { request: 5 }
    ));
    assert!(matches!(
        parse(format!(
            r#"{{"type":"ExportPrepare","request":6,"conn":"{conn}",
                "source":{{"kind":"Query","sql":"SELECT 1","columns":[{column}]}}}}"#
        )),
        Command::ExportPrepare {
            source: ExportSource::Query { .. },
            ..
        }
    ));
    assert!(matches!(
        parse(format!(
            r#"{{"type":"ExportPreview","request":7,"conn":"{conn}","format":"where_clause",
                "options":{options},"columns":[{column}],"rows":[]}}"#
        )),
        Command::ExportPreview { .. }
    ));
}
