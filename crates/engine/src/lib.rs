//! Headless application core, running on its own tokio runtime.
//!
//! A client (the Tauri UI today, a CLI or MCP server later) sends [`Command`]s through an
//! [`EngineHandle`] and receives [`Event`]s from [`EngineEvents`]. Every command runs in its
//! own task, so a long query never blocks other commands.

mod browse;
mod command;
mod edit;
mod event;
mod schema;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use quarry_driver_api::{
    DriverError, DriverRegistry, ExecItem, ExecOpts, ResolvedConfig, Session, SharedCancel,
};
use quarry_model::{
    BrowseQuery, ConnId, ConnectionConfig, Environment, Favorite, HistoryEntry, RowChange,
    SchemaChange, TableRef, Value, Workspace,
};
use quarry_store::{
    ConfigStore, FavoritesStore, HistoryStore, SecretKind, SecretStore, WorkspaceStore,
};
use quarry_tunnel::{Tunnel, TunnelSpec};

pub use command::{Command, Password, RequestId, TabId};
pub use event::{ConnState, Event, NoticeKind};
/// Pure SQL text helpers (statement splitting, …), shared with clients.
pub use quarry_sql as sql;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Rows kept per result set of a typed query; more are drained but not sent (`truncated`).
const RUN_ROW_LIMIT: u64 = 1000;

/// Grid edits inside an open transaction run between these (standard SQL savepoints).
const EDIT_SAVEPOINT: &str = "SAVEPOINT quarry_grid_edit";
const EDIT_ROLLBACK: &str = "ROLLBACK TO SAVEPOINT quarry_grid_edit";
const EDIT_RELEASE: &str = "RELEASE SAVEPOINT quarry_grid_edit";

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
    /// Data directory (open tabs, history); `None` = `quarry_store::default_data_dir`.
    pub data_dir: Option<PathBuf>,
    /// `known_hosts` for SSH tunnels; `None` = `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
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
    /// A transaction is open on `session` (started by the user, not by grid edits).
    in_transaction: bool,
}

struct Engine {
    events: async_channel::Sender<Event>,
    registry: DriverRegistry,
    config: Option<ConfigStore>,
    favorite_store: Option<FavoritesStore>,
    favorites: Mutex<Vec<Favorite>>,
    workspace: Option<WorkspaceStore>,
    history: Option<HistoryStore>,
    secrets: Option<SecretStore>,
    connections: Mutex<Vec<ConnectionConfig>>,
    /// Secrets entered during this run (also when not saved to the keyring).
    passwords: Mutex<HashMap<(ConnId, SecretKind), String>>,
    /// One SSH tunnel per connection that has one, shared by all its sessions.
    tunnels: Mutex<HashMap<ConnId, Arc<Tunnel>>>,
    known_hosts: Option<PathBuf>,
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

        let data_dir = options
            .data_dir
            .map(Ok)
            .unwrap_or_else(quarry_store::default_data_dir);
        let (workspace, history) = match data_dir {
            Ok(dir) => (
                Some(WorkspaceStore::new(&dir)),
                Some(HistoryStore::new(dir)),
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
                ssh_secret,
                remember_password,
            } => {
                self.save_connection(config, password, ssh_secret, remember_password)
                    .await;
            }
            Command::DeleteConnection { id } => self.delete_connection(id).await,
            Command::TestConnection {
                request,
                config,
                password,
                ssh_secret,
            } => {
                let result = self.test_connection(config, password, ssh_secret).await;
                let (server_version, error) = match result {
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
            Command::LoadColumns { id, database } => self.load_columns(id, database).await,
            Command::LoadTableInfo {
                request,
                conn,
                table,
            } => self.load_table_info(request, conn, table).await,
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
                let query = Query::user(sql);
                self.run_query(tab, conn, database, query, RUN_ROW_LIMIT)
                    .await;
            }
            Command::Browse {
                tab,
                conn,
                table,
                limit,
                offset,
                query,
            } => match self.page_query(conn, &table, &query, limit, offset) {
                Ok(query) => {
                    self.run_query(tab, conn, table.database.clone(), query, limit)
                        .await;
                }
                Err(error) => self.emit(Event::QueryFailed {
                    tab,
                    error,
                    cancelled: false,
                }),
            },
            Command::CountRows {
                tab,
                conn,
                table,
                query,
            } => self.count_rows(tab, conn, table, query).await,
            Command::Explain {
                tab,
                conn,
                database,
                sql,
                json,
            } => match self.explain_query(conn, &sql, json) {
                Ok(query) => {
                    self.run_query(tab, conn, database, query, RUN_ROW_LIMIT)
                        .await;
                }
                Err(error) => self.emit(Event::QueryFailed {
                    tab,
                    error,
                    cancelled: false,
                }),
            },
            Command::BeginTransaction {
                tab,
                conn,
                database,
            } => self.begin_transaction(tab, conn, database).await,
            Command::EndTransaction { tab, commit } => self.end_transaction(tab, commit).await,
            Command::ApplyChanges {
                tab,
                conn,
                table,
                changes,
                confirmed,
            } => {
                self.apply_changes(tab, conn, table, changes, confirmed)
                    .await;
            }
            Command::InsertSql {
                request,
                conn,
                table,
                columns,
                rows,
            } => {
                let result = self.driver_of(conn).and_then(|driver| {
                    schema::insert_text(driver.dialect(), &table, &columns, &rows)
                });
                let (text, error) = match result {
                    Ok(text) => (Some(text), None),
                    Err(error) => (None, Some(error)),
                };
                self.emit(Event::InsertSql {
                    request,
                    text,
                    error,
                });
            }
            Command::AlterSchema {
                request,
                conn,
                change,
                confirmed,
            } => self.alter_schema(request, conn, change, confirmed).await,
            Command::Cancel { tab } => self.cancel(tab),
            Command::CloseTab { tab } => self.close_tab(tab).await,
            Command::LoadFavorites => self.load_favorites(),
            Command::SaveFavorite { favorite } => {
                {
                    let mut list = lock(&self.favorites);
                    match list.iter_mut().find(|f| f.id == favorite.id) {
                        Some(existing) => *existing = favorite,
                        None => list.push(favorite),
                    }
                }
                self.persist_favorites();
            }
            Command::DeleteFavorite { id } => self.delete_favorites(|f| f.id == id),
            Command::LoadHistory {
                request,
                query,
                conn,
                limit,
            } => self.load_history(request, query.as_deref(), conn, limit),
            Command::ClearHistory { request } => {
                if let Some(Err(err)) = self.history.as_ref().map(HistoryStore::clear) {
                    self.notice(
                        NoticeKind::Error,
                        format!("Couldn't clear the history: {err}"),
                    );
                }
                self.emit(Event::History {
                    request,
                    entries: Vec::new(),
                });
            }
            Command::LoadWorkspace => self.load_workspace(),
            Command::SaveWorkspace { request, workspace } => {
                self.save_workspace(request, &workspace);
            }
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
        ssh_secret: Option<Password>,
        remember_password: bool,
    ) {
        let id = config.id;
        let label = config.name.clone();
        let uses_ssh = config.ssh.is_some();
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
            self.store_secret(
                id,
                SecretKind::Database,
                &label,
                password,
                remember_password,
            )
            .await;
        }
        if let Some(Password(secret)) = ssh_secret {
            self.store_secret(id, SecretKind::Ssh, &label, secret, remember_password)
                .await;
        } else if !uses_ssh {
            lock(&self.passwords).remove(&(id, SecretKind::Ssh));
            self.forget_secret(id, SecretKind::Ssh).await;
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
        lock(&self.passwords).retain(|(conn, _), _| *conn != id);
        let list = self.persist();
        self.delete_favorites(|f| f.conn == id);
        self.forget_secret(id, SecretKind::Database).await;
        self.forget_secret(id, SecretKind::Ssh).await;
        self.emit(Event::Connections { connections: list });
    }

    /// Keep a secret for this run, and in the keyring when `remember` is set.
    async fn store_secret(
        &self,
        id: ConnId,
        kind: SecretKind,
        label: &str,
        secret: String,
        remember: bool,
    ) {
        lock(&self.passwords).insert((id, kind), secret.clone());
        let Some(secrets) = &self.secrets else { return };
        let result = if remember {
            secrets.set_password(id, kind, label, &secret).await
        } else {
            secrets.delete_password(id, kind).await
        };
        if let Err(err) = result {
            tracing::warn!(%err, "keyring");
            if remember {
                self.notice(
                    NoticeKind::Warning,
                    format!("Password kept for this session only — keyring unavailable: {err}"),
                );
            }
        }
    }

    async fn forget_secret(&self, id: ConnId, kind: SecretKind) {
        if let Some(secrets) = &self.secrets
            && let Err(err) = secrets.delete_password(id, kind).await
        {
            tracing::warn!(%err, "keyring delete");
        }
    }

    // ---------- Favorites ----------

    fn load_favorites(&self) {
        let favorites = match self.favorite_store.as_ref().map(FavoritesStore::load) {
            Some(Ok(list)) => list,
            Some(Err(err)) => {
                self.notice(NoticeKind::Error, format!("Couldn't read favorites: {err}"));
                Vec::new()
            }
            None => Vec::new(),
        };
        *lock(&self.favorites) = favorites.clone();
        self.emit(Event::Favorites { favorites });
    }

    fn persist_favorites(&self) {
        let favorites = lock(&self.favorites).clone();
        if let Some(store) = &self.favorite_store
            && let Err(err) = store.save(&favorites)
        {
            self.notice(NoticeKind::Error, format!("Couldn't save favorites: {err}"));
        }
        self.emit(Event::Favorites { favorites });
    }

    fn delete_favorites(&self, matches: impl Fn(&Favorite) -> bool) {
        let removed = {
            let mut list = lock(&self.favorites);
            let before = list.len();
            list.retain(|f| !matches(f));
            list.len() != before
        };
        if removed {
            self.persist_favorites();
        }
    }

    // ---------- History ----------

    fn load_history(
        &self,
        request: RequestId,
        query: Option<&str>,
        conn: Option<ConnId>,
        limit: u32,
    ) {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let entries = match self.history.as_ref().map(|h| h.search(query, conn, limit)) {
            Some(Ok(entries)) => entries,
            Some(Err(err)) => {
                self.notice(
                    NoticeKind::Error,
                    format!("Couldn't read the history: {err}"),
                );
                Vec::new()
            }
            None => Vec::new(),
        };
        self.emit(Event::History { request, entries });
    }

    fn record(&self, entry: HistoryEntry) {
        let Some(history) = &self.history else { return };
        match history.append(&entry) {
            Ok(()) => self.emit(Event::HistoryAdded { entry }),
            Err(err) => tracing::warn!(%err, "history append"),
        }
    }

    // ---------- Workspace ----------

    fn load_workspace(&self) {
        let workspace = match self.workspace.as_ref().map(WorkspaceStore::load) {
            Some(Ok(workspace)) => workspace,
            Some(Err(err)) => {
                self.notice(
                    NoticeKind::Warning,
                    format!("Couldn't restore the tabs from the last session: {err}"),
                );
                Workspace::default()
            }
            None => Workspace::default(),
        };
        self.emit(Event::Workspace { workspace });
    }

    fn save_workspace(&self, request: RequestId, workspace: &Workspace) {
        let error = match &self.workspace {
            Some(store) => store.save(workspace).err().map(|err| err.to_string()),
            None => Some("No data directory".to_owned()),
        };
        if let Some(err) = &error {
            tracing::warn!(%err, "workspace save");
        }
        self.emit(Event::WorkspaceSaved { request, error });
    }

    // ---------- Sessions ----------

    /// Settings ready for the driver: secrets filled in and, for a connection over SSH, the
    /// address pointed at the tunnel. `tunnel` decides which tunnel: the connection's shared one
    /// (`Shared`, opened if needed; Disconnect drops it) or one owned by the caller
    /// (`Temporary`, for Test connection).
    async fn resolve(
        &self,
        mut config: ConnectionConfig,
        password: Option<Password>,
        ssh_secret: Option<Password>,
        tunnel: TunnelUse,
    ) -> Result<(ResolvedConfig, Option<Arc<Tunnel>>), String> {
        let password = match password {
            Some(Password(p)) => Some(p),
            None => self.stored_secret(config.id, SecretKind::Database).await,
        };
        let Some(ssh) = config.ssh.clone() else {
            return Ok((ResolvedConfig { config, password }, None));
        };
        let id = config.id;
        let existing = match tunnel {
            TunnelUse::Shared => lock(&self.tunnels).get(&id).cloned(),
            TunnelUse::Temporary => None,
        };
        let tunnel = match existing {
            Some(tunnel) => tunnel,
            None => {
                let secret = match ssh_secret {
                    Some(Password(s)) => Some(s),
                    None => self.stored_secret(id, SecretKind::Ssh).await,
                };
                let known_hosts = self
                    .known_hosts
                    .clone()
                    .ok_or_else(|| "No home directory, so no ~/.ssh/known_hosts.".to_owned())?;
                let spec = TunnelSpec {
                    ssh,
                    secret,
                    target_host: config.host.clone().unwrap_or_else(|| "127.0.0.1".into()),
                    target_port: config
                        .port
                        .ok_or_else(|| "Set the database port to connect over SSH.".to_owned())?,
                    known_hosts,
                };
                let opened = Arc::new(quarry_tunnel::open(spec).await.map_err(|e| e.to_string())?);
                if tunnel == TunnelUse::Shared {
                    lock(&self.tunnels).insert(id, opened.clone());
                }
                opened
            }
        };
        config.host = Some("127.0.0.1".into());
        config.port = Some(tunnel.local_port());
        Ok((ResolvedConfig { config, password }, Some(tunnel)))
    }

    async fn stored_secret(&self, id: ConnId, kind: SecretKind) -> Option<String> {
        if let Some(p) = lock(&self.passwords).get(&(id, kind)) {
            return Some(p.clone());
        }
        let secrets = self.secrets?;
        match secrets.get_password(id, kind).await {
            Ok(Some(p)) => {
                lock(&self.passwords).insert((id, kind), p.clone());
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
        ssh_secret: Option<Password>,
    ) -> Result<String, String> {
        // The temporary tunnel lives until this function returns.
        let (resolved, _tunnel) = self
            .resolve(config, password, ssh_secret, TunnelUse::Temporary)
            .await?;
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
        let resolved = match self.resolve(config, None, None, TunnelUse::Shared).await {
            Ok((resolved, _)) => resolved,
            Err(err) => return self.emit(failed(err)),
        };
        let mut session = match self.open_session(&resolved).await {
            Ok(session) => session,
            Err(err) => return self.emit(failed(err)),
        };
        let databases = match session.list_databases().await {
            Ok(dbs) => dbs,
            Err(err) => return self.emit(failed(describe(&err))),
        };
        let server_version = session.server_version();
        let capabilities = self
            .registry
            .get(&resolved.config.driver)
            .map(|d| d.capabilities())
            .unwrap_or_default();
        lock(&self.meta).insert(id, Arc::new(tokio::sync::Mutex::new(session)));
        self.emit(Event::ConnState {
            id,
            state: ConnState::Connected {
                server_version,
                capabilities,
            },
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
        // Sessions are closed, so the tunnel can go.
        lock(&self.tunnels).remove(&id);
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

    async fn load_columns(&self, id: ConnId, database: String) {
        let Some(meta) = lock(&self.meta).get(&id).cloned() else {
            let error = Some("Not connected".to_owned());
            return self.emit(Event::Columns {
                id,
                database,
                columns: None,
                error,
            });
        };
        let result = meta.lock().await.list_columns(&database).await;
        let (columns, error) = match result {
            Ok(columns) => (Some(columns), None),
            Err(err) => (None, Some(describe(&err))),
        };
        self.emit(Event::Columns {
            id,
            database,
            columns,
            error,
        });
    }

    /// The connection's metadata session, connecting first when needed.
    async fn meta_session(&self, id: ConnId) -> Result<SharedSession, String> {
        if let Some(meta) = lock(&self.meta).get(&id).cloned() {
            return Ok(meta);
        }
        self.connect(id).await;
        lock(&self.meta)
            .get(&id)
            .cloned()
            .ok_or_else(|| "Not connected".to_owned())
    }

    async fn load_table_info(&self, request: RequestId, conn: ConnId, table: TableRef) {
        let result = match self.meta_session(conn).await {
            Ok(meta) => meta
                .lock()
                .await
                .table_info(&table)
                .await
                .map_err(|e| describe(&e)),
            Err(error) => Err(error),
        };
        let (info, error) = match result {
            Ok(info) => (Some(info), None),
            Err(error) => (None, Some(error)),
        };
        self.emit(Event::TableInfo {
            request,
            info,
            error,
        });
    }

    // ---------- Queries ----------

    fn tab_slot(&self, tab: TabId) -> Arc<tokio::sync::Mutex<Option<TabSession>>> {
        lock(&self.tabs).entry(tab).or_default().clone()
    }

    /// Open the tab's session on `conn`, replacing one on another connection.
    async fn ensure_tab_session(
        &self,
        slot: &mut Option<TabSession>,
        tab: TabId,
        conn: ConnId,
    ) -> Result<(), String> {
        if slot.as_ref().is_some_and(|t| t.conn != conn)
            && let Some(old) = slot.take()
        {
            close_shared(old.session).await;
        }
        if slot.is_some() {
            return Ok(());
        }
        let config = self
            .config_of(conn)
            .ok_or_else(|| "Connection not found".to_owned())?;
        let (resolved, _) = self.resolve(config, None, None, TunnelUse::Shared).await?;
        let session = self.open_session(&resolved).await?;
        lock(&self.cancels).insert(tab, (conn, session.cancel_handle()));
        *slot = Some(TabSession {
            conn,
            session: Arc::new(tokio::sync::Mutex::new(session)),
            database: None,
            in_transaction: false,
        });
        Ok(())
    }

    async fn run_query(
        &self,
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
        query: Query,
        row_limit: u64,
    ) {
        let Query {
            sql,
            params,
            from_user,
        } = query;
        self.emit(Event::QueryStarted { tab });
        let failed = |error: String| Event::QueryFailed {
            tab,
            error,
            cancelled: false,
        };
        let slot = self.tab_slot(tab);
        let mut slot = slot.lock().await;
        if let Err(error) = self.ensure_tab_session(&mut slot, tab, conn).await {
            return self.emit(failed(error));
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
        let opts = ExecOpts {
            row_limit: Some(row_limit),
            ..ExecOpts::default()
        };
        let ran_at = chrono::Utc::now();
        let result = if params.is_empty() {
            session.execute(&sql, &opts, &mut sink).await
        } else {
            session.query(&sql, &params, &opts, &mut sink).await
        };
        drop(session);
        let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        let error = match result {
            Ok(()) => {
                self.emit(Event::QueryFinished { tab, elapsed_ms });
                if from_user {
                    let open = transaction_after(tab_session.in_transaction, &sql);
                    if open != tab_session.in_transaction {
                        tab_session.in_transaction = open;
                        self.emit(Event::TransactionState {
                            tab,
                            open,
                            error: None,
                        });
                    }
                }
                None
            }
            Err(err) => {
                let error = describe(&err);
                self.emit(Event::QueryFailed {
                    tab,
                    cancelled: matches!(err, DriverError::Cancelled),
                    error: error.clone(),
                });
                Some(error.lines().next().unwrap_or_default().to_owned())
            }
        };
        if from_user {
            self.record(HistoryEntry {
                sql,
                conn,
                database: tab_session.database.clone(),
                ran_at,
                elapsed_ms,
                error,
            });
        }
    }

    async fn apply_changes(
        &self,
        tab: TabId,
        conn: ConnId,
        table: TableRef,
        changes: Vec<RowChange>,
        confirmed: bool,
    ) {
        let failed = |error: String| Event::ChangesFailed { tab, error };
        let Some(config) = self.config_of(conn) else {
            return self.emit(failed("Connection not found".into()));
        };
        if config.read_only {
            return self.emit(failed("This connection is read-only.".into()));
        }
        let Some(driver) = self.registry.get(&config.driver) else {
            return self.emit(failed(format!("No driver for “{}”", config.driver.0)));
        };
        let statements = match edit::statements(driver.dialect(), &table, &changes) {
            Ok(statements) if statements.is_empty() => {
                return self.emit(Event::ChangesApplied {
                    tab,
                    message: "Nothing to save".into(),
                });
            }
            Ok(statements) => statements,
            Err(error) => return self.emit(failed(error)),
        };
        let summary = edit::Summary::of(&changes).describe();
        if !confirmed && config.environment.confirms_every_write() {
            return self.emit(Event::ChangesConfirmRequired {
                tab,
                message: format!("This changes data in a production database: {summary}."),
            });
        }

        let slot = self.tab_slot(tab);
        let mut slot = slot.lock().await;
        if let Err(error) = self.ensure_tab_session(&mut slot, tab, conn).await {
            return self.emit(failed(error));
        }
        let Some(tab_session) = slot.as_mut() else {
            return;
        };
        let in_transaction = tab_session.in_transaction;
        let mut session = tab_session.session.lock().await;
        // Inside the user's transaction a savepoint keeps the batch all-or-nothing without
        // committing their other work.
        let started = if in_transaction {
            run_plain(&mut **session, EDIT_SAVEPOINT).await
        } else {
            session.begin().await
        };
        if let Err(err) = started {
            return self.emit(failed(describe(&err)));
        }
        for statement in &statements {
            let result = session
                .execute_params(&statement.sql, &statement.params)
                .await;
            let error = match result {
                Ok(1) => None,
                Ok(_) if !statement.one_row => None,
                Ok(0) => Some(
                    "A row was not found: it may have been changed or deleted since it was \
                     loaded. Nothing was saved; reload and try again."
                        .to_owned(),
                ),
                Ok(n) => Some(format!(
                    "A change would affect {n} rows instead of one. Nothing was saved."
                )),
                Err(err) => Some(format!("{}\nNothing was saved.", describe(&err))),
            };
            if let Some(error) = error {
                let undone = if in_transaction {
                    run_plain(&mut **session, EDIT_ROLLBACK).await
                } else {
                    session.rollback().await
                };
                if let Err(err) = undone {
                    tracing::warn!(%err, "rollback after failed edit");
                }
                return self.emit(failed(error));
            }
        }
        if in_transaction {
            if let Err(err) = run_plain(&mut **session, EDIT_RELEASE).await {
                return self.emit(failed(describe(&err)));
            }
            return self.emit(Event::ChangesApplied {
                tab,
                message: format!("{summary} in the open transaction. Commit to keep them."),
            });
        }
        if let Err(err) = session.commit().await {
            if let Err(err) = session.rollback().await {
                tracing::warn!(%err, "rollback after failed commit");
            }
            return self.emit(failed(describe(&err)));
        }
        self.emit(Event::ChangesApplied {
            tab,
            message: summary,
        });
    }

    /// Review (`confirmed: false`) or run a schema change. The statements are always rebuilt
    /// here from `change`, never taken from the client.
    async fn alter_schema(
        &self,
        request: RequestId,
        conn: ConnId,
        change: SchemaChange,
        confirmed: bool,
    ) {
        let done = |change: SchemaChange, error: Option<String>| Event::SchemaChanged {
            request,
            conn,
            change,
            error,
        };
        let Some(config) = self.config_of(conn) else {
            return self.emit(done(change, Some("Connection not found".into())));
        };
        if config.read_only {
            return self.emit(done(change, Some("This connection is read-only.".into())));
        }
        let statements = match self
            .driver_of(conn)
            .and_then(|driver| schema::statements(driver.dialect(), &change))
        {
            Ok(statements) => statements,
            Err(error) => return self.emit(done(change, Some(error))),
        };
        if !confirmed {
            let warning = schema::warning(config.environment, &change);
            return self.emit(Event::SchemaReview {
                request,
                conn,
                change,
                statements,
                warning,
            });
        }

        let meta = match self.meta_session(conn).await {
            Ok(meta) => meta,
            Err(error) => return self.emit(done(change, Some(error))),
        };
        let mut session = meta.lock().await;
        let mut error = None;
        for sql in statements {
            let ran_at = chrono::Utc::now();
            let started = std::time::Instant::now();
            let result = run_plain(&mut **session, &sql).await;
            let failed = result.err().map(|err| describe(&err));
            self.record(HistoryEntry {
                sql,
                conn,
                database: change.database().map(str::to_owned),
                ran_at,
                elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                error: failed
                    .as_ref()
                    .map(|e| e.lines().next().unwrap_or_default().to_owned()),
            });
            if failed.is_some() {
                error = failed;
                break;
            }
        }
        // A new or dropped database changes the tree's first level.
        let databases = match &change {
            SchemaChange::CreateDatabase { .. } | SchemaChange::DropDatabase { .. } => {
                session.list_databases().await.ok()
            }
            _ => None,
        };
        drop(session);
        if let Some(databases) = databases {
            self.emit(Event::Databases {
                id: conn,
                databases,
            });
        }
        self.emit(done(change, error));
    }

    fn driver_of(&self, conn: ConnId) -> Result<&dyn quarry_driver_api::Driver, String> {
        let config = self
            .config_of(conn)
            .ok_or_else(|| "Connection not found".to_owned())?;
        self.registry
            .get(&config.driver)
            .ok_or_else(|| format!("No driver for “{}”", config.driver.0))
    }

    /// `SELECT` for one page of `table`, in the dialect of the connection's driver.
    fn page_query(
        &self,
        conn: ConnId,
        table: &TableRef,
        query: &BrowseQuery,
        limit: u64,
        offset: u64,
    ) -> Result<Query, String> {
        let dialect = self.driver_of(conn)?.dialect();
        let clauses = browse::clauses(dialect, query)?;
        let sql = dialect.select_page(
            table,
            clauses.filter.as_deref(),
            clauses.order.as_deref(),
            limit,
            offset,
        );
        Ok(Query {
            sql,
            params: clauses.params,
            from_user: false,
        })
    }

    /// The plan statement for `sql`, which must be a single statement that is not already an
    /// `EXPLAIN` (so it can never become `EXPLAIN ANALYZE`, which runs the statement).
    fn explain_query(&self, conn: ConnId, sql: &str, json: bool) -> Result<Query, String> {
        let dialect = self.driver_of(conn)?.dialect();
        let statement = single_statement(sql)?;
        let sql = dialect
            .explain(statement, json)
            .ok_or_else(|| "This database cannot show query plans.".to_owned())?;
        Ok(Query {
            sql,
            params: Vec::new(),
            from_user: false,
        })
    }

    async fn count_rows(&self, tab: TabId, conn: ConnId, table: TableRef, query: BrowseQuery) {
        let result = self.count(tab, conn, &table, &query).await;
        let (count, error) = match result {
            Ok(count) => (Some(count), None),
            Err(error) => (None, Some(error)),
        };
        self.emit(Event::RowCount { tab, count, error });
    }

    async fn count(
        &self,
        tab: TabId,
        conn: ConnId,
        table: &TableRef,
        query: &BrowseQuery,
    ) -> Result<u64, String> {
        let dialect = self.driver_of(conn)?.dialect();
        let clauses = browse::clauses(dialect, query)?;
        let sql = dialect.count_rows(table, clauses.filter.as_deref());
        let slot = self.tab_slot(tab);
        let mut slot = slot.lock().await;
        self.ensure_tab_session(&mut slot, tab, conn).await?;
        let Some(tab_session) = slot.as_mut() else {
            return Err("The tab has no session.".to_owned());
        };
        let mut first: Option<Value> = None;
        let mut sink = |item: ExecItem| {
            if let ExecItem::Rows(rows) = item
                && first.is_none()
            {
                first = rows.into_iter().next().and_then(|r| r.into_iter().next());
            }
        };
        let opts = ExecOpts {
            row_limit: Some(1),
            ..ExecOpts::default()
        };
        let mut session = tab_session.session.lock().await;
        let result = if clauses.params.is_empty() {
            session.execute(&sql, &opts, &mut sink).await
        } else {
            session.query(&sql, &clauses.params, &opts, &mut sink).await
        };
        result.map_err(|e| describe(&e))?;
        match first {
            Some(Value::Int(n)) => u64::try_from(n).map_err(|_| "Unexpected row count".to_owned()),
            Some(Value::UInt(n)) => Ok(n),
            Some(Value::Decimal(text) | Value::Text(text)) => {
                text.parse().map_err(|_| "Unexpected row count".to_owned())
            }
            _ => Err("The database returned no row count.".to_owned()),
        }
    }

    async fn begin_transaction(&self, tab: TabId, conn: ConnId, database: Option<String>) {
        let slot = self.tab_slot(tab);
        let mut slot = slot.lock().await;
        let state =
            |open: bool, error: Option<String>| Event::TransactionState { tab, open, error };
        if let Err(error) = self.ensure_tab_session(&mut slot, tab, conn).await {
            return self.emit(state(false, Some(error)));
        }
        let Some(tab_session) = slot.as_mut() else {
            return;
        };
        if tab_session.in_transaction {
            return self.emit(state(true, None));
        }
        let mut session = tab_session.session.lock().await;
        if let Some(db) = database.filter(|db| tab_session.database.as_ref() != Some(db)) {
            if let Err(err) = session.use_database(&db).await {
                return self.emit(state(false, Some(describe(&err))));
            }
            tab_session.database = Some(db);
        }
        match session.begin().await {
            Ok(()) => {
                tab_session.in_transaction = true;
                self.emit(state(true, None));
            }
            Err(err) => self.emit(state(false, Some(describe(&err)))),
        }
    }

    async fn end_transaction(&self, tab: TabId, commit: bool) {
        let slot = self.tab_slot(tab);
        let mut slot = slot.lock().await;
        let state =
            |open: bool, error: Option<String>| Event::TransactionState { tab, open, error };
        let Some(tab_session) = slot.as_mut() else {
            return self.emit(state(false, None));
        };
        let mut session = tab_session.session.lock().await;
        let result = if commit {
            session.commit().await
        } else {
            session.rollback().await
        };
        match result {
            Ok(()) => {
                tab_session.in_transaction = false;
                self.emit(state(false, None));
            }
            // A failed COMMIT leaves the transaction open; the user can retry or roll back.
            Err(err) => self.emit(state(tab_session.in_transaction, Some(describe(&err)))),
        }
    }

    fn cancel(&self, tab: TabId) {
        let cancel = lock(&self.cancels).get(&tab).map(|(_, c)| c.clone());
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
    }

    async fn close_tab(&self, tab: TabId) {
        let cancel = lock(&self.cancels).remove(&tab);
        let slot = lock(&self.tabs).remove(&tab);
        let Some(slot) = slot else { return };
        // A busy slot means a query (or count) is running and holds the session: stop it,
        // otherwise closing would wait until it ends. An idle session needs no KILL.
        let mut guard = match slot.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                if let Some((_, cancel)) = cancel {
                    cancel.cancel();
                }
                slot.lock().await
            }
        };
        if let Some(tab_session) = guard.take() {
            drop(guard);
            close_shared(tab_session.session).await;
        }
    }
}

/// SQL for a tab, with its parameters.
#[derive(Debug, Clone)]
struct Query {
    sql: String,
    params: Vec<Value>,
    /// Typed by the user: recorded in the history and may begin or end a transaction.
    from_user: bool,
}

impl Query {
    fn user(sql: String) -> Self {
        Self {
            sql,
            params: Vec::new(),
            from_user: true,
        }
    }
}

/// Run a statement whose result does not matter (the text protocol accepts any statement).
async fn run_plain(session: &mut dyn Session, sql: &str) -> Result<(), DriverError> {
    session
        .execute(sql, &ExecOpts::default(), &mut |_: ExecItem| {})
        .await
}

/// Whether a transaction is open after `sql` ran successfully, given whether one was open.
fn transaction_after(mut open: bool, sql: &str) -> bool {
    use quarry_sql::TransactionEffect;
    for range in quarry_sql::split(sql) {
        match quarry_sql::transaction_effect(&sql[range]) {
            Some(TransactionEffect::Begin) => open = true,
            Some(TransactionEffect::End) => open = false,
            None => {}
        }
    }
    open
}

/// The only statement in `sql`, refusing scripts and statements that already explain.
fn single_statement(sql: &str) -> Result<&str, String> {
    let ranges = quarry_sql::split(sql);
    let [range] = ranges.as_slice() else {
        return Err(
            "Explain works on one statement. Put the cursor on it, or select it.".to_owned(),
        );
    };
    let statement = &sql[range.clone()];
    let first = quarry_sql::first_keyword(statement).unwrap_or_default();
    if matches!(first.as_str(), "EXPLAIN" | "DESCRIBE" | "DESC" | "ANALYZE") {
        return Err("This statement is already a plan request. Run it instead.".to_owned());
    }
    Ok(statement)
}

/// Which SSH tunnel `Engine::resolve` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TunnelUse {
    Shared,
    Temporary,
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
            config_dir: Some(dir.clone()),
            data_dir: Some(dir),
            known_hosts: None,
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
            Event::Pong { id, version } => {
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
            data_dir: Some(dir.clone()),
            known_hosts: None,
            disable_keyring: true,
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
        next_matching(&events, |e| {
            matches!(e, Event::HistoryAdded { entry } if entry.sql == "DROP TABLE edit_scratch")
        })
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
        // Dropping it again fails: it is gone.
        engine.send(alter(true)).unwrap();
        let Event::SchemaChanged { error, .. } = next_matching(&events, |e| {
            matches!(e, Event::SchemaChanged { request: 21, .. })
        })
        .await
        else {
            unreachable!()
        };
        assert!(error.is_some());
        next_matching(
            &events,
            |e| matches!(e, Event::HistoryAdded { entry } if entry.error.is_some()),
        )
        .await;

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
        assert!(
            matches!(&rows[0][0], Value::Text(t) | Value::Json(t) if t.contains("query_block"))
        );
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
}
