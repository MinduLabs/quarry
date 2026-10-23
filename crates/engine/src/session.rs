//! Per-tab sessions: open, cancel, close and the transactions a tab holds.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use quarry_model::ConnId;

use crate::connections::TunnelUse;
use crate::engine::{Engine, TabCancel, TabSession, close_shared, describe, is_lost, lock};
use crate::{Event, TabId};

/// Whether a transaction is open after `sql` ran successfully, given whether one was open.
pub(crate) fn transaction_after(mut open: bool, sql: &str) -> bool {
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

impl Engine {
    pub(crate) fn tab_slot(&self, tab: TabId) -> Arc<tokio::sync::Mutex<Option<TabSession>>> {
        lock(&self.tabs).entry(tab).or_default().clone()
    }

    /// Open the tab's session on `conn`, replacing one on another connection or one whose
    /// connection is gone.
    pub(crate) async fn ensure_tab_session(
        &self,
        slot: &mut Option<TabSession>,
        tab: TabId,
        conn: ConnId,
    ) -> Result<(), String> {
        if slot.as_ref().is_some_and(|t| t.conn != conn || t.is_lost)
            && let Some(old) = slot.take()
        {
            lock(&self.cancels).remove(&tab);
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
        let stopped = Arc::new(AtomicBool::new(false));
        let cancel = TabCancel {
            driver: session.cancel_handle(),
            stopped: stopped.clone(),
        };
        lock(&self.cancels).insert(tab, (conn, Arc::new(cancel)));
        *slot = Some(TabSession {
            conn,
            session: Arc::new(tokio::sync::Mutex::new(session)),
            database: None,
            in_transaction: false,
            stopped,
            is_lost: false,
        });
        Ok(())
    }

    pub(crate) fn cancel(&self, tab: TabId) {
        let cancel = lock(&self.cancels).get(&tab).map(|(_, c)| c.clone());
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
    }

    pub(crate) async fn close_tab(&self, tab: TabId) {
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

    pub(crate) async fn begin_transaction(
        &self,
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
    ) {
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
                tab_session.is_lost |= is_lost(&err);
                return self.emit(state(false, Some(describe(&err))));
            }
            tab_session.database = Some(db);
        }
        match session.begin().await {
            Ok(()) => {
                tab_session.in_transaction = true;
                self.emit(state(true, None));
            }
            Err(err) => {
                tab_session.is_lost |= is_lost(&err);
                self.emit(state(false, Some(describe(&err))));
            }
        }
    }

    pub(crate) async fn end_transaction(&self, tab: TabId, commit: bool) {
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
            // The connection went with the transaction: the server keeps nothing uncommitted,
            // but a commit may have landed before the answer was lost.
            Err(err) if is_lost(&err) => {
                tab_session.is_lost = true;
                tab_session.in_transaction = false;
                let outcome = if commit {
                    "The commit may or may not have been saved: check the data."
                } else {
                    "The server rolled the transaction back."
                };
                self.emit(state(false, Some(format!("{}\n{outcome}", describe(&err)))));
            }
            // A failed COMMIT leaves the transaction open; the user can retry or roll back.
            Err(err) => self.emit(state(tab_session.in_transaction, Some(describe(&err)))),
        }
    }
}
