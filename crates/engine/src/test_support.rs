//! What the engine's tests share: an engine in a temporary folder, a saved connection to a test
//! database, and waiting for an event.

use std::path::{Path, PathBuf};
use std::time::Duration;

use quarry_model::{ConnId, ConnectionConfig, DriverId, Environment, SslMode};

use crate::{Command, EngineEvents, EngineHandle, EngineOptions, Event, Password};

/// An engine kept in `dir`, without a keyring, loading drivers from `QUARRY_DRIVER_DIR`.
pub(crate) fn options(dir: &Path) -> EngineOptions {
    EngineOptions {
        config_dir: Some(dir.to_path_buf()),
        data_dir: Some(dir.to_path_buf()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
        driver_dir: None,
        driver_manifest: None,
        update_url: None,
        in_process_drivers: Default::default(),
    }
}

pub(crate) fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()))
}

/// A connection to the test database at `$env` (`host:port`), or `None` when it is not set.
pub(crate) fn db_config(
    env: &str,
    driver: &str,
    user: &str,
    database: Option<&str>,
) -> Option<ConnectionConfig> {
    let addr = std::env::var(env).ok()?;
    let (host, port) = addr.split_once(':')?;
    Some(ConnectionConfig {
        id: ConnId::new(),
        name: format!("Local {driver}"),
        driver: DriverId(driver.into()),
        host: Some(host.into()),
        port: port.parse().ok(),
        database: database.map(Into::into),
        user: Some(user.into()),
        path: None,
        ssl: SslMode::Prefer,
        ssh: None,
        environment: Environment::Local,
        read_only: false,
        color: None,
        group: None,
    })
}

/// An engine in a new folder with `config` saved, not connected yet.
pub(crate) async fn saved_engine(
    config: ConnectionConfig,
    password: &str,
) -> (EngineHandle, EngineEvents, PathBuf) {
    let dir = temp_dir();
    let (engine, events) = EngineHandle::spawn_with(options(&dir)).unwrap();
    engine
        .send(Command::SaveConnection {
            config,
            password: Some(Password(password.into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    next_matching(&events, |e| matches!(e, Event::Connections { .. })).await;
    (engine, events, dir)
}

/// The next event `pred` accepts; fails the test after 20 seconds.
pub(crate) async fn next_matching(events: &EngineEvents, pred: impl Fn(&Event) -> bool) -> Event {
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
