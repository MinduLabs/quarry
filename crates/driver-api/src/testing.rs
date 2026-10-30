//! What every driver's integration tests share. Built only with the `testing` feature.

use async_trait::async_trait;
use quarry_model::{ConnId, ConnectionConfig, DriverId, Environment, SslMode, Value};

use crate::{ExecItem, ExecOpts, ResolvedConfig, RowSink, Session};

/// A connection to the test database at `$env` (`host:port`, database `shop`), or `None` when
/// it is not set: the test then skips.
pub fn config(env: &str, driver: &str, user: &str, password: &str) -> Option<ResolvedConfig> {
    let addr = std::env::var(env).ok()?;
    let (host, port) = addr.split_once(':')?;
    Some(ResolvedConfig {
        config: ConnectionConfig {
            id: ConnId::new(),
            name: "test".into(),
            driver: DriverId(driver.into()),
            host: Some(host.into()),
            port: port.parse().ok(),
            database: Some("shop".into()),
            user: Some(user.into()),
            path: None,
            ssl: SslMode::Prefer,
            ssh: None,
            environment: Environment::Local,
            read_only: false,
            color: None,
            group: None,
        },
        password: Some(password.into()),
    })
}

/// Every item `sql` produces. Panics when it fails.
pub async fn run(session: &mut Box<dyn Session>, sql: &str, opts: ExecOpts) -> Vec<ExecItem> {
    let mut items = Vec::new();
    session
        .execute(sql, &opts, &mut |item| items.push(item))
        .await
        .unwrap_or_else(|err| panic!("{sql}: {err}"));
    items
}

/// The rows among `items`, in order.
pub fn rows(items: &[ExecItem]) -> Vec<Vec<Value>> {
    items
        .iter()
        .filter_map(|item| match item {
            ExecItem::Rows(rows) => Some(rows.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Counts what a stream sends, and stops it once it has `stop_at` rows when set.
#[derive(Debug, Default)]
pub struct Count {
    pub rows: usize,
    pub batches: usize,
    pub is_done: bool,
    pub stop_at: Option<usize>,
}

#[async_trait]
impl RowSink for Count {
    async fn accept(&mut self, item: ExecItem) -> bool {
        match item {
            ExecItem::Rows(rows) => {
                self.rows += rows.len();
                self.batches += 1;
            }
            ExecItem::SetDone { .. } => self.is_done = true,
            _ => {}
        }
        self.stop_at.is_none_or(|stop| self.rows < stop)
    }
}
