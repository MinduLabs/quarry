//! Headless application core, running on its own tokio runtime.
//!
//! A client (the Tauri UI today, a CLI or MCP server later) sends [`Command`]s through an
//! [`EngineHandle`] and receives [`Event`]s from [`EngineEvents`]. Every command runs in its
//! own task, so a long query never blocks other commands.

mod command;
mod event;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use quarry_driver_api::{
    DriverError, DriverRegistry, ExecItem, ExecOpts, ResolvedConfig, Session, SharedCancel,
};
use quarry_model::{ConnId, ConnectionConfig, Environment};
use quarry_store::{ConfigStore, SecretStore};

pub use command::{Command, Password, RequestId, TabId};
pub use event::{ConnState, Event, NoticeKind};
/// Pure SQL text helpers (statement splitting, …), shared with clients.
pub use quarry_sql as sql;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("failed to start engine runtime: {0}")]
    Runtime(#[from] std::io::Error),
    #[error("engine has stopped")]
    Stopped,
}

#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    /// Config directory; `None` = the platform default (see `quarry_store::default_config_dir`).
    pub config_dir: Option<PathBuf>,
    /// Do not touch the system keyring (tests, environments without one).
    pub disable_keyring: bool,
}

/// Sending side of the engine. Cheap to clone; the engine stops when every handle is dropped.
#[derive(Debug, Clone)]
pub struct EngineHandle {
    commands: async_channel::Sender<Command>,
}

/// Receiving side for engine events.
pub type EngineEvents = async_channel::Receiver<Event>;

impl EngineHandle {
    pub fn spawn() -> Result<(Self, EngineEvents), EngineError> {
        Self::spawn_with(EngineOptions::default())
    }

    /// Start the engine on a dedicated thread with its own tokio runtime.
    pub fn spawn_with(options: EngineOptions) -> Result<(Self, EngineEvents), EngineError> {
        let (cmd_tx, cmd_rx) = async_channel::unbounded::<Command>();
        let (event_tx, event_rx) = async_channel::unbounded::<Event>();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("quarry-engine")
            .enable_all()
            .build()?;

        thread::Builder::new()
            .name("quarry-engine-main".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let engine = Arc::new(Engine::new(options, event_tx));
                    run(engine, cmd_rx).await;
                });
            })?;

        Ok((Self { commands: cmd_tx }, event_rx))
    }

    /// Queue a command. Results arrive later as [`Event`]s.
    pub fn send(&self, command: Command) -> Result<(), EngineError> {
        self.commands
            .try_send(command)
            .map_err(|_| EngineError::Stopped)
    }
}

async fn run(engine: Arc<Engine>, commands: async_channel::Receiver<Command>) {
    tracing::info!(version = VERSION, "engine started");
    while let Ok(command) = commands.recv().await {
        tracing::debug!(?command, "command");
        tokio::spawn(engine.clone().handle(command));
    }
    tracing::info!("engine stopped");
}

type SharedSession = Arc<tokio::sync::Mutex<Box<dyn Session>>>;

struct TabSession {
    conn: ConnId,
    session: SharedSession,
    database: Option<String>,
}

struct Engine {
    events: async_channel::Sender<Event>,
    registry: DriverRegistry,
    config: Option<ConfigStore>,
    secrets: Option<SecretStore>,
    connections: Mutex<Vec<ConnectionConfig>>,
    /// Passwords entered during this run (also when not saved to the keyring).
    passwords: Mutex<HashMap<ConnId, String>>,
    /// One metadata session (schema tree) per connection.
    meta: Mutex<HashMap<ConnId, SharedSession>>,
    /// Per-tab sessions. The async mutex is held by `run_query` while a query runs.
    tabs: Mutex<HashMap<TabId, Arc<tokio::sync::Mutex<Option<TabSession>>>>>,
    /// Per-tab cancel handles, kept apart so a busy session can still be cancelled.
    cancels: Mutex<HashMap<TabId, (ConnId, SharedCancel)>>,
}

/// These `std` mutexes are only held briefly; a poisoned lock still holds valid data.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Engine {
    fn new(options: EngineOptions, events: async_channel::Sender<Event>) -> Self {
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

        Self {
            events,
            registry,
            config,
            secrets: (!options.disable_keyring).then_some(SecretStore),
            connections: Mutex::new(Vec::new()),
            passwords: Mutex::new(HashMap::new()),
            meta: Mutex::new(HashMap::new()),
            tabs: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
        }
    }

    fn emit(&self, event: Event) {
        // Unbounded channel: only fails once the client is gone, which is fine to ignore.
        let _ = self.events.try_send(event);
    }

    fn notice(&self, kind: NoticeKind, message: impl Into<String>) {
        self.emit(Event::Notice {
            kind,
            message: message.into(),
        });
    }

    async fn handle(self: Arc<Self>, command: Command) {
        match command {
            Command::Ping { id } => self.emit(Event::Pong {
                id,
                version: VERSION.to_owned(),
            }),
            Command::LoadConnections => self.load_connections(),
            Command::SaveConnection {
                config,
                password,
                remember_password,
            } => {
                self.save_connection(config, password, remember_password)
                    .await;
            }
            Command::DeleteConnection { id } => self.delete_connection(id).await,
            Command::TestConnection {
                request,
                config,
                password,
            } => {
                let (server_version, error) = match self.test_connection(config, password).await {
                    Ok(version) => (Some(version), None),
                    Err(err) => (None, Some(err)),
                };
                self.emit(Event::TestResult {
                    request,
                    server_version,
                    error,
                });
            }
            Command::Connect { id } => self.connect(id).await,
            Command::Disconnect { id } => self.disconnect(id).await,
            Command::LoadObjects { id, database } => self.load_objects(id, database).await,
            Command::Run {
                tab,
                conn,
                database,
                sql,
                confirmed,
            } => {
                if !confirmed
                    && let Some(message) = self
                        .config_of(conn)
                        .and_then(|c| safety_check(c.environment, &sql))
                {
                    self.emit(Event::ConfirmRequired {
                        tab,
                        conn,
                        database,
                        sql,
                        message,
                    });
                    return;
                }
                self.run_query(tab, conn, database, sql).await;
            }
            Command::Cancel { tab } => self.cancel(tab),
            Command::CloseTab { tab } => self.close_tab(tab).await,
        }
    }

    // ---------- Connection list ----------

    fn load_connections(&self) {
        let Some(store) = &self.config else {
            self.emit(Event::Connections {
                connections: Vec::new(),
            });
            return;
        };
        match store.load_connections() {
            Ok(list) => {
                *lock(&self.connections) = list.clone();
                self.emit(Event::Connections { connections: list });
            }
            Err(err) => self.notice(
                NoticeKind::Error,
                format!("Couldn't read saved connections: {err}"),
            ),
        }
    }

    fn persist(&self) -> Vec<ConnectionConfig> {
        let list = lock(&self.connections).clone();
        if let Some(store) = &self.config
            && let Err(err) = store.save_connections(&list)
        {
            self.notice(
                NoticeKind::Error,
                format!("Couldn't save connections: {err}"),
            );
        }
        list
    }

    async fn save_connection(
        &self,
        config: ConnectionConfig,
        password: Option<Password>,
        remember_password: bool,
    ) {
        let id = config.id;
        let label = config.name.clone();
        let was_connected = lock(&self.meta).contains_key(&id);
        {
            let mut list = lock(&self.connections);
            match list.iter_mut().find(|c| c.id == id) {
                Some(existing) => *existing = config,
                None => list.push(config),
            }
        }
        let list = self.persist();

        if let Some(Password(password)) = password {
            lock(&self.passwords).insert(id, password.clone());
            if let Some(secrets) = &self.secrets {
                let result = if remember_password {
                    secrets.set_password(id, &label, &password).await
                } else {
                    secrets.delete_password(id).await
                };
                if let Err(err) = result {
                    tracing::warn!(%err, "keyring");
                    if remember_password {
                        self.notice(
                            NoticeKind::Warning,
                            format!(
                                "Password kept for this session only — keyring unavailable: {err}"
                            ),
                        );
                    }
                }
            }
        }
        self.emit(Event::Connections { connections: list });

        // Settings changed: drop the old sessions so the next connect uses the new ones.
        if was_connected {
            self.disconnect(id).await;
        }
    }

    async fn delete_connection(&self, id: ConnId) {
        self.disconnect(id).await;
        lock(&self.connections).retain(|c| c.id != id);
        lock(&self.passwords).remove(&id);
        let list = self.persist();
        if let Some(secrets) = &self.secrets
            && let Err(err) = secrets.delete_password(id).await
        {
            tracing::warn!(%err, "keyring delete");
        }
        self.emit(Event::Connections { connections: list });
    }

    // ---------- Sessions ----------

    async fn resolve(
        &self,
        config: ConnectionConfig,
        password: Option<Password>,
    ) -> ResolvedConfig {
        let password = match password {
            Some(Password(p)) => Some(p),
            None => self.stored_password(config.id).await,
        };
        ResolvedConfig { config, password }
    }

    async fn stored_password(&self, id: ConnId) -> Option<String> {
        if let Some(p) = lock(&self.passwords).get(&id) {
            return Some(p.clone());
        }
        let secrets = self.secrets?;
        match secrets.get_password(id).await {
            Ok(Some(p)) => {
                lock(&self.passwords).insert(id, p.clone());
                Some(p)
            }
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(%err, "keyring read");
                None
            }
        }
    }

    async fn open_session(&self, resolved: &ResolvedConfig) -> Result<Box<dyn Session>, String> {
        let driver = self
            .registry
            .get(&resolved.config.driver)
            .ok_or_else(|| format!("No driver for “{}”", resolved.config.driver.0))?;
        let mut session = driver.connect(resolved).await.map_err(|e| describe(&e))?;
        if resolved.config.read_only {
            session
                .set_read_only(true)
                .await
                .map_err(|e| describe(&e))?;
        }
        Ok(session)
    }

    fn config_of(&self, id: ConnId) -> Option<ConnectionConfig> {
        lock(&self.connections).iter().find(|c| c.id == id).cloned()
    }

    async fn test_connection(
        &self,
        config: ConnectionConfig,
        password: Option<Password>,
    ) -> Result<String, String> {
        let resolved = self.resolve(config, password).await;
        let session = self.open_session(&resolved).await?;
        let version = session.server_version();
        let _ = session.close().await;
        Ok(version)
    }

    async fn connect(&self, id: ConnId) {
        let Some(config) = self.config_of(id) else {
            return;
        };
        self.emit(Event::ConnState {
            id,
            state: ConnState::Connecting,
        });

        let failed = |error: String| Event::ConnState {
            id,
            state: ConnState::Failed { error },
        };
        let resolved = self.resolve(config, None).await;
        let mut session = match self.open_session(&resolved).await {
            Ok(session) => session,
            Err(err) => return self.emit(failed(err)),
        };
        let databases = match session.list_databases().await {
            Ok(dbs) => dbs,
            Err(err) => return self.emit(failed(describe(&err))),
        };
        let server_version = session.server_version();
        lock(&self.meta).insert(id, Arc::new(tokio::sync::Mutex::new(session)));
        self.emit(Event::ConnState {
            id,
            state: ConnState::Connected { server_version },
        });
        self.emit(Event::Databases { id, databases });
    }

    async fn disconnect(&self, id: ConnId) {
        // Cancel running queries on this connection first, otherwise we would wait for them.
        let running: Vec<SharedCancel> = {
            let mut cancels = lock(&self.cancels);
            let ids: Vec<TabId> = cancels
                .iter()
                .filter(|(_, (c, _))| *c == id)
                .map(|(t, _)| *t)
                .collect();
            ids.iter()
                .filter_map(|t| cancels.remove(t))
                .map(|(_, c)| c)
                .collect()
        };
        for cancel in running {
            cancel.cancel();
        }
        let meta = lock(&self.meta).remove(&id);
        let tabs: Vec<_> = lock(&self.tabs).values().cloned().collect();
        let mut sessions: Vec<SharedSession> = meta.into_iter().collect();
        for slot in tabs {
            let mut slot = slot.lock().await;
            if slot.as_ref().is_some_and(|t| t.conn == id)
                && let Some(tab) = slot.take()
            {
                sessions.push(tab.session);
            }
        }
        let had_sessions = !sessions.is_empty();
        for session in sessions {
            close_shared(session).await;
        }
        if had_sessions {
            self.emit(Event::ConnState {
                id,
                state: ConnState::Disconnected,
            });
        }
    }

    async fn load_objects(&self, id: ConnId, database: String) {
        let Some(meta) = lock(&self.meta).get(&id).cloned() else {
            let error = Some("Not connected".to_owned());
            return self.emit(Event::Objects {
                id,
                database,
                objects: None,
                error,
            });
        };
        let result = meta.lock().await.list_objects(&database).await;
        let (objects, error) = match result {
            Ok(objects) => (Some(objects), None),
            Err(err) => (None, Some(describe(&err))),
        };
        self.emit(Event::Objects {
            id,
            database,
            objects,
            error,
        });
    }

    // ---------- Queries ----------

    fn tab_slot(&self, tab: TabId) -> Arc<tokio::sync::Mutex<Option<TabSession>>> {
        lock(&self.tabs).entry(tab).or_default().clone()
    }

    async fn run_query(&self, tab: TabId, conn: ConnId, database: Option<String>, sql: String) {
        self.emit(Event::QueryStarted { tab });
        let failed = |error: String| Event::QueryFailed {
            tab,
            error,
            cancelled: false,
        };
        let slot = self.tab_slot(tab);
        let mut slot = slot.lock().await;

        // The tab switched to another connection: reopen its session.
        if slot.as_ref().is_some_and(|t| t.conn != conn)
            && let Some(old) = slot.take()
        {
            close_shared(old.session).await;
        }
        if slot.is_none() {
            let Some(config) = self.config_of(conn) else {
                return self.emit(failed("Connection not found".into()));
            };
            let resolved = self.resolve(config, None).await;
            match self.open_session(&resolved).await {
                Ok(session) => {
                    lock(&self.cancels).insert(tab, (conn, session.cancel_handle()));
                    *slot = Some(TabSession {
                        conn,
                        session: Arc::new(tokio::sync::Mutex::new(session)),
                        database: None,
                    });
                }
                Err(error) => return self.emit(failed(error)),
            }
        }
        let Some(tab_session) = slot.as_mut() else {
            return;
        };
        let mut session = tab_session.session.lock().await;

        if let Some(db) = database.filter(|db| tab_session.database.as_ref() != Some(db)) {
            if let Err(err) = session.use_database(&db).await {
                return self.emit(failed(describe(&err)));
            }
            tab_session.database = Some(db);
        }

        let events = self.events.clone();
        let mut set = 0u32;
        let mut elapsed = Duration::ZERO;
        let mut sink = |item: ExecItem| {
            let event = match item {
                ExecItem::Columns(columns) => Event::ResultColumns { tab, set, columns },
                ExecItem::Rows(rows) => Event::ResultRows { tab, set, rows },
                ExecItem::SetDone {
                    affected,
                    rows,
                    truncated,
                } => {
                    let event = Event::ResultDone {
                        tab,
                        set,
                        affected,
                        rows,
                        truncated,
                    };
                    set += 1;
                    event
                }
                ExecItem::Notice(message) => Event::Notice {
                    kind: NoticeKind::Info,
                    message,
                },
                ExecItem::Done { elapsed: e } => {
                    elapsed = e;
                    return;
                }
            };
            let _ = events.try_send(event);
        };
        let result = session.execute(&sql, &ExecOpts::default(), &mut sink).await;
        match result {
            Ok(()) => {
                let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
                self.emit(Event::QueryFinished { tab, elapsed_ms });
            }
            Err(err) => self.emit(Event::QueryFailed {
                tab,
                cancelled: matches!(err, DriverError::Cancelled),
                error: describe(&err),
            }),
        }
    }

    fn cancel(&self, tab: TabId) {
        let cancel = lock(&self.cancels).get(&tab).map(|(_, c)| c.clone());
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
    }

    async fn close_tab(&self, tab: TabId) {
        lock(&self.cancels).remove(&tab);
        let slot = lock(&self.tabs).remove(&tab);
        if let Some(slot) = slot
            && let Some(tab_session) = slot.lock().await.take()
        {
            close_shared(tab_session.session).await;
        }
    }
}

async fn close_shared(session: SharedSession) {
    // If someone still holds it (a running query), it closes when they drop it.
    if let Ok(mutex) = Arc::try_unwrap(session)
        && let Err(err) = mutex.into_inner().close().await
    {
        tracing::debug!(%err, "session close");
    }
}

/// Safe mode: the reason the user must confirm, or `None` to run right away.
///
/// - staging: DROP / TRUNCATE / UPDATE or DELETE without WHERE
/// - production: any statement that is not a read
fn safety_check(environment: Environment, sql: &str) -> Option<String> {
    use quarry_sql::StatementKind;
    let worst = quarry_sql::split(sql)
        .into_iter()
        .map(|r| quarry_sql::classify(&sql[r]))
        .max()?;
    match (environment, worst) {
        (Environment::Local, _) | (_, StatementKind::Read) => None,
        (_, StatementKind::Destructive) => Some(
            "This can delete data: it contains DROP, TRUNCATE, or an UPDATE/DELETE without WHERE."
                .to_owned(),
        ),
        (Environment::Production, StatementKind::Schema) => {
            Some("This changes the schema of a production database.".to_owned())
        }
        (Environment::Production, StatementKind::Write) => {
            Some("This changes data in a production database.".to_owned())
        }
        (Environment::Staging, _) => None,
    }
}

/// A user-facing error message.
fn describe(err: &DriverError) -> String {
    match err {
        DriverError::Auth(msg) | DriverError::Connect(msg) => msg.clone(),
        DriverError::Cancelled => "Query cancelled".to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine() -> (EngineHandle, EngineEvents) {
        let dir = std::env::temp_dir().join(format!("quarry-engine-{}", uuid::Uuid::new_v4()));
        EngineHandle::spawn_with(EngineOptions {
            config_dir: Some(dir),
            disable_keyring: true,
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
            Event::Pong { id, version } => {
                assert_eq!(id, 7);
                assert_eq!(version, VERSION);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Integration tests against a real MySQL (`QUARRY_TEST_MYSQL=127.0.0.1:13306`).
#[cfg(test)]
mod mysql_tests {
    use quarry_model::{ConnId, ConnectionConfig, DriverId, Environment, SslMode};

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
            disable_keyring: true,
        })
        .unwrap();

        engine
            .send(Command::SaveConnection {
                config,
                password: Some(Password("quarry".into())),
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

        std::fs::remove_dir_all(dir).unwrap();
    }
}
