//! Command routing: one task per command, each calling the part of the engine that owns it.

use std::sync::Arc;

use crate::engine::{Engine, lock};
use crate::query::{Query, RUN_ROW_LIMIT, safety_check};
use crate::{Command, Event, NoticeKind, VERSION, browse, schema};

/// The command's name for the performance log (never its fields: some hold passwords).
fn command_kind(command: &Command) -> &'static str {
    match command {
        Command::Ping { .. } => "Ping",
        Command::LoadConnections => "LoadConnections",
        Command::LoadDataSources => "LoadDataSources",
        Command::SaveConnection { .. } => "SaveConnection",
        Command::DeleteConnection { .. } => "DeleteConnection",
        Command::TestConnection { .. } => "TestConnection",
        Command::Connect { .. } => "Connect",
        Command::Disconnect { .. } => "Disconnect",
        Command::LoadObjects { .. } => "LoadObjects",
        Command::LoadColumns { .. } => "LoadColumns",
        Command::LoadTableInfo { .. } => "LoadTableInfo",
        Command::LoadServerInfo { .. } => "LoadServerInfo",
        Command::Run { .. } => "Run",
        Command::Browse { .. } => "Browse",
        Command::CountRows { .. } => "CountRows",
        Command::Explain { .. } => "Explain",
        Command::BeginTransaction { .. } => "BeginTransaction",
        Command::EndTransaction { .. } => "EndTransaction",
        Command::ApplyChanges { .. } => "ApplyChanges",
        Command::InsertSql { .. } => "InsertSql",
        Command::AlterSchema { .. } => "AlterSchema",
        Command::Cancel { .. } => "Cancel",
        Command::CloseTab { .. } => "CloseTab",
        Command::LoadFavorites => "LoadFavorites",
        Command::SaveFavorite { .. } => "SaveFavorite",
        Command::DeleteFavorite { .. } => "DeleteFavorite",
        Command::LoadHistory { .. } => "LoadHistory",
        Command::ClearHistory { .. } => "ClearHistory",
        Command::PerfMarks { .. } => "PerfMarks",
        Command::LoadWorkspace => "LoadWorkspace",
        Command::SaveWorkspace { .. } => "SaveWorkspace",
    }
}

impl Engine {
    pub(crate) async fn handle(self: Arc<Self>, command: Command) {
        let timed = self.perf.is_some() && command_kind(&command) != "PerfMarks";
        let kind = command_kind(&command);
        let started = std::time::Instant::now();
        self.clone().dispatch(command).await;
        if timed {
            self.perf("command", started, serde_json::json!({ "command": kind }));
        }
    }

    pub(crate) async fn dispatch(self: Arc<Self>, command: Command) {
        match command {
            Command::Ping { id } => self.emit(Event::Pong {
                id,
                version: VERSION.to_owned(),
                perf: self.perf.is_some(),
            }),
            Command::PerfMarks { marks } => {
                if let Some(log) = &self.perf {
                    for mark in marks {
                        log.record(
                            "ui",
                            &mark.name,
                            mark.ms,
                            serde_json::json!({ "detail": mark.detail }),
                        );
                    }
                }
            }
            Command::LoadConnections => self.load_connections(),
            Command::LoadDataSources => self.load_data_sources(),
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
            Command::LoadServerInfo {
                request,
                conn,
                kind,
            } => self.load_server_info(request, conn, kind).await,
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
            } => {
                let query = if query.sort.is_empty() {
                    query
                } else {
                    let key = self.primary_key_on_tab(tab, conn, &table).await;
                    browse::with_tie_breakers(query, &key)
                };
                match self.page_query(conn, &table, &query, limit, offset) {
                    Ok(query) => {
                        self.run_query(tab, conn, table.database.clone(), query, limit)
                            .await;
                    }
                    Err(error) => self.emit(Event::QueryFailed {
                        tab,
                        error,
                        cancelled: false,
                        statement: None,
                        location: None,
                    }),
                }
            }
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
                    statement: None,
                    location: None,
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
            } => self.load_history(request, query, conn, limit).await,
            Command::ClearHistory { request } => {
                if let Some(Err(err)) = self.with_history(|h| h.clear()).await {
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
}
