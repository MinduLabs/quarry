use quarry_model::{
    ConnId, ConnectionConfig, Environment, Favorite, RowChange, ServerInfoKind, TableRef, Workspace,
};

use super::*;
use crate::query::{safety_check, single_statement};
use crate::session::transaction_after;

fn test_engine() -> (EngineHandle, EngineEvents) {
    let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
    EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
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
