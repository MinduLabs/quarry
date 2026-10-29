//! Headless application core, running on its own tokio runtime.
//!
//! A client (the Tauri UI today, a CLI or MCP server later) sends [`Command`]s through an
//! [`EngineHandle`] and receives [`Event`]s from [`EngineEvents`]. Every command runs in its
//! own task, so a long query never blocks other commands.

mod browse;
mod changes;
mod command;
mod connections;
mod console;
mod data_sources;
mod dispatch;
mod drivers;
mod edit;
mod engine;
mod event;
mod favorites;
mod history;
mod metadata;
mod perf;
mod query;
mod schema;
mod session;
mod workspace;

#[cfg(test)]
mod mysql_tests;
#[cfg(test)]
mod postgres_tests;
#[cfg(test)]
mod sqlserver_tests;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crate::engine::Engine;

pub use command::{Command, Password, PerfMark, RequestId, TabId};
pub use event::{ConnState, Event, LogEntry, LogSource, NoticeKind, Rows};
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
    /// Data directory (open tabs, history); `None` = `quarry_store::default_data_dir`.
    pub data_dir: Option<PathBuf>,
    /// `known_hosts` for SSH tunnels; `None` = `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
    /// Do not touch the system keyring (tests, environments without one).
    pub disable_keyring: bool,
    /// Performance log (`perf.jsonl`); `None` = the `QUARRY_PERF` environment variable decides.
    pub perf_log: Option<PathBuf>,
    /// Where the driver binaries are; `None` = the `QUARRY_DRIVER_DIR` environment variable.
    pub driver_dir: Option<PathBuf>,
    /// Drivers run inside the engine instead of from a binary. Tests only: Quarry itself runs
    /// every driver from its binary.
    pub in_process_drivers: InProcessDrivers,
}

#[derive(Clone, Default)]
pub struct InProcessDrivers(pub Vec<Arc<dyn quarry_driver_api::Driver>>);

impl std::fmt::Debug for InProcessDrivers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|d| d.id()))
            .finish()
    }
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
    // Before the first command, so a restored tab never finds its driver missing.
    engine.load_drivers().await;
    while let Ok(command) = commands.recv().await {
        tracing::debug!(?command, "command");
        tokio::spawn(engine.clone().handle(command));
    }
    tracing::info!("engine stopped");
}
