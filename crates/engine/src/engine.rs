//! The engine's state, shared by every module that implements a part of it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use quarry_driver_api::{DriverError, DriverRegistry, Session, SharedCancel};
use quarry_model::{ConnId, ConnectionConfig, Favorite};
use quarry_store::{
    ConfigStore, FavoritesStore, HistoryStore, SecretKind, SecretStore, WorkspaceStore,
};
use quarry_tunnel::Tunnel;

use crate::{EngineOptions, Event, NoticeKind, TabId, perf};

pub(crate) type SharedSession = Arc<tokio::sync::Mutex<Box<dyn Session>>>;

pub(crate) struct TabSession {
    pub(crate) conn: ConnId,
    pub(crate) session: SharedSession,
    pub(crate) database: Option<String>,
    /// A transaction is open on `session` (started by the user, not by grid edits).
    pub(crate) in_transaction: bool,
}

pub(crate) struct Engine {
    pub(crate) events: async_channel::Sender<Event>,
    pub(crate) registry: DriverRegistry,
    pub(crate) config: Option<ConfigStore>,
    pub(crate) favorite_store: Option<FavoritesStore>,
    pub(crate) favorites: Mutex<Vec<Favorite>>,
    pub(crate) workspace: Option<WorkspaceStore>,
    /// File IO, so it runs on the blocking pool (`spawn_blocking`), never on the runtime's two
    /// workers that stream query results.
    pub(crate) history: Option<Arc<HistoryStore>>,
    pub(crate) secrets: Option<SecretStore>,
    pub(crate) connections: Mutex<Vec<ConnectionConfig>>,
    /// Secrets entered during this run (also when not saved to the keyring).
    pub(crate) passwords: Mutex<HashMap<(ConnId, SecretKind), String>>,
    /// One SSH tunnel per connection that has one, shared by all its sessions.
    pub(crate) tunnels: Mutex<HashMap<ConnId, Arc<Tunnel>>>,
    pub(crate) known_hosts: Option<PathBuf>,
    /// One metadata session (schema tree) per connection.
    pub(crate) meta: Mutex<HashMap<ConnId, SharedSession>>,
    /// Per-tab sessions. The async mutex is held by `run_query` while a query runs.
    pub(crate) tabs: Mutex<HashMap<TabId, Arc<tokio::sync::Mutex<Option<TabSession>>>>>,
    /// Per-tab cancel handles, kept apart so a busy session can still be cancelled.
    pub(crate) cancels: Mutex<HashMap<TabId, (ConnId, SharedCancel)>>,
    /// Development performance log; `None` when off.
    pub(crate) perf: Option<perf::PerfLog>,
}

/// These `std` mutexes are only held briefly; a poisoned lock still holds valid data.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
/// Milliseconds rounded to microseconds, for the performance log.
pub(crate) fn round(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

/// A user-facing error message.
pub(crate) fn describe(err: &DriverError) -> String {
    match err {
        DriverError::Auth(msg) | DriverError::Connect(msg) => msg.clone(),
        DriverError::Cancelled => "Query cancelled".to_owned(),
        other => other.to_string(),
    }
}

pub(crate) async fn close_shared(session: SharedSession) {
    // If someone still holds it (a running query), it closes when they drop it.
    if let Ok(mutex) = Arc::try_unwrap(session)
        && let Err(err) = mutex.into_inner().close().await
    {
        tracing::debug!(%err, "session close");
    }
}

impl Engine {
    pub(crate) fn new(options: EngineOptions, events: async_channel::Sender<Event>) -> Self {
        let mut registry = DriverRegistry::default();
        registry.register(Box::new(quarry_driver_mysql::MysqlDriver::default()));

        let config = match options
            .config_dir
            .map(Ok)
            .unwrap_or_else(quarry_store::default_config_dir)
        {
            Ok(dir) => Some(ConfigStore::new(dir)),
            Err(err) => {
                tracing::error!(%err, "no config directory; connections will not be saved");
                None
            }
        };

        let data_dir = options
            .data_dir
            .map(Ok)
            .unwrap_or_else(quarry_store::default_data_dir);
        let perf = options
            .perf_log
            .or_else(|| perf::path_from_env(data_dir.as_deref().ok()))
            .and_then(|path| match perf::PerfLog::open(path) {
                Ok(log) => Some(log),
                Err(err) => {
                    tracing::warn!(%err, "performance log unavailable");
                    None
                }
            });
        let (workspace, history) = match data_dir {
            Ok(dir) => (
                Some(WorkspaceStore::new(&dir)),
                Some(Arc::new(HistoryStore::new(dir))),
            ),
            Err(err) => {
                tracing::error!(%err, "no data directory; open tabs and history will not be saved");
                (None, None)
            }
        };

        let favorite_store = config.as_ref().map(|c| FavoritesStore::new(c.dir()));

        Self {
            events,
            registry,
            config,
            favorite_store,
            favorites: Mutex::new(Vec::new()),
            workspace,
            history,
            secrets: (!options.disable_keyring).then_some(SecretStore),
            connections: Mutex::new(Vec::new()),
            passwords: Mutex::new(HashMap::new()),
            tunnels: Mutex::new(HashMap::new()),
            known_hosts: options
                .known_hosts
                .or_else(quarry_tunnel::default_known_hosts),
            meta: Mutex::new(HashMap::new()),
            tabs: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            perf,
        }
    }

    /// Log a timing when the performance log is on.
    pub(crate) fn perf(&self, name: &str, started: std::time::Instant, fields: serde_json::Value) {
        if let Some(log) = &self.perf {
            log.record("engine", name, perf::ms_since(started), fields);
        }
    }

    pub(crate) fn emit(&self, event: Event) {
        // Unbounded channel: only fails once the client is gone, which is fine to ignore.
        let _ = self.events.try_send(event);
    }

    pub(crate) fn notice(&self, kind: NoticeKind, message: impl Into<String>) {
        self.emit(Event::Notice {
            kind,
            message: message.into(),
        });
    }

    pub(crate) fn driver_of(&self, conn: ConnId) -> Result<&dyn quarry_driver_api::Driver, String> {
        let config = self
            .config_of(conn)
            .ok_or_else(|| "Connection not found".to_owned())?;
        self.registry
            .get(&config.driver)
            .ok_or_else(|| format!("No driver for “{}”", config.driver.0))
    }
}
