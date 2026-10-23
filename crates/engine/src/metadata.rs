//! Schema tree, columns, table info and server variables, read on the metadata session.

use std::time::Instant;

use quarry_model::{ConnId, ServerInfoKind, TableRef};

use crate::engine::{Engine, SharedSession, describe, lock};
use crate::{Event, RequestId};

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
        let result = meta.lock().await.list_objects(&database).await;
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
        let result = meta.lock().await.list_columns(&database).await;
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
    // claude: the metadata session has no lost-connection recovery yet: after a server restart
    // the schema tree keeps failing until Disconnect + Connect. Same `is_lost` idea as tabs.
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

    pub(crate) async fn load_table_info(&self, request: RequestId, conn: ConnId, table: TableRef) {
        let started = Instant::now();
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
            Ok(meta) => meta
                .lock()
                .await
                .server_info(kind)
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
