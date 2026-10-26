//! Schema tree, columns, table info and server variables, read on the metadata session.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use quarry_driver_api::{DriverError, Session};
use quarry_model::{ConnId, ServerInfoKind, TableRef};

use crate::connections::TunnelUse;
use crate::engine::{Engine, SharedSession, describe, is_lost, lock};
use crate::{Event, RequestId};

/// A read on a metadata session, as a future that borrows the session.
pub(crate) type MetaRead<'a, T> = Pin<Box<dyn Future<Output = Result<T, DriverError>> + Send + 'a>>;

impl Engine {
    pub(crate) async fn load_objects(&self, id: ConnId, database: String) {
        let Some(meta) = lock(&self.meta).get(&id).cloned() else {
            let error = Some("Not connected".to_owned());
            return self.emit(Event::Objects {
                id,
                database,
                objects: None,
                error,
            });
        };
        let started = Instant::now();
        let result = self
            .with_meta(id, meta, |s| {
                let name = database.clone();
                Box::pin(async move { s.list_objects(&name).await })
            })
            .await;
        let count = result.as_ref().map(Vec::len).ok();
        self.perf(
            "schema.objects",
            started,
            serde_json::json!({ "objects": count }),
        );
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

    pub(crate) async fn load_columns(&self, id: ConnId, database: String) {
        let Some(meta) = lock(&self.meta).get(&id).cloned() else {
            let error = Some("Not connected".to_owned());
            return self.emit(Event::Columns {
                id,
                database,
                columns: None,
                error,
            });
        };
        let started = Instant::now();
        let result = self
            .with_meta(id, meta, |s| {
                let name = database.clone();
                Box::pin(async move { s.list_columns(&name).await })
            })
            .await;
        let count = result.as_ref().map(Vec::len).ok();
        self.perf(
            "schema.columns",
            started,
            serde_json::json!({ "tables": count }),
        );
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
    pub(crate) async fn meta_session(&self, id: ConnId) -> Result<SharedSession, String> {
        if let Some(meta) = lock(&self.meta).get(&id).cloned() {
            return Ok(meta);
        }
        self.connect(id).await;
        lock(&self.meta)
            .get(&id)
            .cloned()
            .ok_or_else(|| "Not connected".to_owned())
    }

    /// Run a read on the connection's metadata session. When its connection turns out to be
    /// gone (server restart, idle timeout, network), open a new session and run it once more,
    /// so the schema tree recovers without a Disconnect and Connect. Only for reads: running
    /// `op` twice must be harmless.
    pub(crate) async fn with_meta<T: Send>(
        &self,
        id: ConnId,
        meta: SharedSession,
        op: impl for<'a> Fn(&'a mut dyn Session) -> MetaRead<'a, T>,
    ) -> Result<T, DriverError> {
        let first = {
            let mut session = meta.lock().await;
            op(&mut **session).await
        };
        let lost = match &first {
            Err(err) if is_lost(err) => err.to_string(),
            _ => return first,
        };
        let fresh = self.reopen_meta(id, &meta).await.map_err(|error| {
            DriverError::ConnectionLost(format!("{lost} Reconnecting failed: {error}"))
        })?;
        let mut session = fresh.lock().await;
        op(&mut **session).await
    }

    /// Replace the metadata session `stale` with a new one. A command that lost the race to
    /// another one finds the new session already there and uses it.
    pub(crate) async fn reopen_meta(
        &self,
        id: ConnId,
        stale: &SharedSession,
    ) -> Result<SharedSession, String> {
        let current = lock(&self.meta).get(&id).cloned();
        match current {
            // Disconnected in the meantime: do not bring the connection back.
            None => return Err("Not connected".to_owned()),
            Some(current) if !Arc::ptr_eq(&current, stale) => return Ok(current),
            Some(_) => {}
        }
        let config = self
            .config_of(id)
            .ok_or_else(|| "Connection not found".to_owned())?;
        let (resolved, _) = self.resolve(config, None, None, TunnelUse::Shared).await?;
        let session = self.open_session(&resolved).await?;
        let fresh: SharedSession = Arc::new(tokio::sync::Mutex::new(session));
        let old = lock(&self.meta).insert(id, fresh.clone());
        tracing::info!("metadata session reopened after the connection was lost");
        if let Some(old) = old {
            crate::engine::close_shared(old).await;
        }
        Ok(fresh)
    }

    pub(crate) async fn load_table_info(&self, request: RequestId, conn: ConnId, table: TableRef) {
        let started = Instant::now();
        let result = match self.meta_session(conn).await {
            Ok(meta) => self
                .with_meta(conn, meta, |s| {
                    let table = table.clone();
                    Box::pin(async move { s.table_info(&table).await })
                })
                .await
                .map_err(|e| describe(&e)),
            Err(error) => Err(error),
        };
        let (info, error) = match result {
            Ok(info) => (Some(info), None),
            Err(error) => (None, Some(error)),
        };
        self.perf(
            "schema.table_info",
            started,
            serde_json::json!({ "columns": info.as_ref().map(|i| i.columns.len()) }),
        );
        self.emit(Event::TableInfo {
            request,
            info,
            error,
        });
    }

    pub(crate) async fn load_server_info(
        &self,
        request: RequestId,
        conn: ConnId,
        kind: ServerInfoKind,
    ) {
        let result = match self.meta_session(conn).await {
            Ok(meta) => self
                .with_meta(conn, meta, |s| {
                    Box::pin(async move { s.server_info(kind).await })
                })
                .await
                .map_err(|e| describe(&e)),
            Err(error) => Err(error),
        };
        let (rows, error) = match result {
            Ok(rows) => (Some(rows), None),
            Err(error) => (None, Some(error)),
        };
        self.emit(Event::ServerInfo {
            request,
            kind,
            rows,
            error,
        });
    }

    // ---------- Queries ----------
}
