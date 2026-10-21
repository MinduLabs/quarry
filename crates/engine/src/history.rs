//! Query history, kept in a file and read on the blocking pool.

use std::time::Instant;

use quarry_model::{ConnId, HistoryEntry};
use quarry_store::HistoryStore;

use crate::engine::Engine;
use crate::{Event, NoticeKind, RequestId};

impl Engine {
    /// Run `work` on the history file off the async runtime. `None` when there is no history
    /// store (no data directory) or the blocking task did not finish.
    pub(crate) async fn with_history<T: Send + 'static>(
        &self,
        work: impl FnOnce(&HistoryStore) -> quarry_store::Result<T> + Send + 'static,
    ) -> Option<quarry_store::Result<T>> {
        let history = self.history.clone()?;
        match tokio::task::spawn_blocking(move || work(&history)).await {
            Ok(result) => Some(result),
            Err(err) => {
                tracing::warn!(%err, "history task");
                None
            }
        }
    }

    pub(crate) async fn load_history(
        &self,
        request: RequestId,
        query: Option<String>,
        conn: Option<ConnId>,
        limit: u32,
    ) {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let started = Instant::now();
        let search = move |h: &HistoryStore| h.search(query.as_deref(), conn, limit);
        let found = self.with_history(search).await;
        let count = found.as_ref().and_then(|r| r.as_ref().ok()).map(Vec::len);
        self.perf(
            "history.search",
            started,
            serde_json::json!({ "entries": count }),
        );
        let entries = match found {
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

    /// Append to the history, then report it. Awaited, so events keep their order.
    pub(crate) async fn record(&self, entry: HistoryEntry) {
        let line = entry.clone();
        let started = Instant::now();
        let appended = self.with_history(move |h| h.append(&line)).await;
        self.perf("history.append", started, serde_json::Value::Null);
        match appended {
            Some(Ok(())) => self.emit(Event::HistoryAdded { entry }),
            Some(Err(err)) => tracing::warn!(%err, "history append"),
            None => {}
        }
    }

    // ---------- Workspace ----------
}
