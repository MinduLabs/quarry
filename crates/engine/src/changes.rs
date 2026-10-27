//! Grid edits and schema changes: review, confirm, then run in one transaction.

use quarry_model::{ConnId, HistoryEntry, RowChange, SchemaChange, TableRef};

use crate::console::Logged;
use crate::engine::{Engine, describe, is_lost};
use crate::query::run_plain;
use crate::{Event, LogSource, RequestId, TabId, edit, schema};

/// Grid edits inside an open transaction run between these (standard SQL savepoints).
pub(crate) const EDIT_SAVEPOINT: &str = "SAVEPOINT quarry_grid_edit";
pub(crate) const EDIT_ROLLBACK: &str = "ROLLBACK TO SAVEPOINT quarry_grid_edit";
pub(crate) const EDIT_RELEASE: &str = "RELEASE SAVEPOINT quarry_grid_edit";

impl Engine {
    pub(crate) async fn apply_changes(
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
        let edit = LogSource::Edit;
        let started = if in_transaction {
            self.logged_step(
                tab,
                edit,
                EDIT_SAVEPOINT,
                run_plain(&mut **session, EDIT_SAVEPOINT),
            )
            .await
        } else {
            self.logged_step(tab, edit, "BEGIN", session.begin()).await
        };
        if let Err(err) = started {
            tab_session.is_lost |= is_lost(&err);
            return self.emit(failed(describe(&err)));
        }
        for statement in &statements {
            let mut logged = Logged::start(edit, &statement.sql);
            let result = session
                .execute_params(&statement.sql, &statement.params)
                .await;
            if let Ok(affected) = result {
                logged.affected(affected);
            }
            self.log(tab, logged, result.as_ref().err().map(describe));
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
                Err(err) => {
                    tab_session.is_lost |= is_lost(&err);
                    Some(format!("{}\nNothing was saved.", describe(&err)))
                }
            };
            if let Some(error) = error {
                let undone = if in_transaction {
                    let undo = run_plain(&mut **session, EDIT_ROLLBACK);
                    self.logged_step(tab, edit, EDIT_ROLLBACK, undo).await
                } else {
                    self.logged_step(tab, edit, "ROLLBACK", session.rollback())
                        .await
                };
                if let Err(err) = undone {
                    tracing::warn!(%err, "rollback after failed edit");
                }
                return self.emit(failed(error));
            }
        }
        if in_transaction {
            let release = run_plain(&mut **session, EDIT_RELEASE);
            if let Err(err) = self.logged_step(tab, edit, EDIT_RELEASE, release).await {
                tab_session.is_lost |= is_lost(&err);
                return self.emit(failed(describe(&err)));
            }
            return self.emit(Event::ChangesApplied {
                tab,
                message: format!("{summary} in the open transaction. Commit to keep them."),
            });
        }
        if let Err(err) = self
            .logged_step(tab, edit, "COMMIT", session.commit())
            .await
        {
            tab_session.is_lost |= is_lost(&err);
            if let Err(err) = self
                .logged_step(tab, edit, "ROLLBACK", session.rollback())
                .await
            {
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
    pub(crate) async fn alter_schema(
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
        let mut lost = false;
        for sql in statements {
            let ran_at = chrono::Utc::now();
            let started = std::time::Instant::now();
            let result = run_plain(&mut **session, &sql).await;
            let failed = result.err().map(|err| {
                lost |= is_lost(&err);
                describe(&err)
            });
            self.record(HistoryEntry {
                sql,
                conn,
                database: change.database().map(str::to_owned),
                ran_at,
                elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                error: failed
                    .as_ref()
                    .map(|e| e.lines().next().unwrap_or_default().to_owned()),
            })
            .await;
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
        // A schema change is not run twice on its own: the statement may have landed before the
        // connection dropped. The session is replaced so the user's next try works.
        if lost {
            let _ = self.reopen_meta(conn, &meta).await;
            error = error.map(|e| {
                format!("{e}\nCheck whether the change was applied before you try again.")
            });
        }
        if let Some(databases) = databases {
            self.emit(Event::Databases {
                id: conn,
                databases,
            });
        }
        self.emit(done(change, error));
    }
}
