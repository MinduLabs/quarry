//! Running and paging queries, counting rows, explain and safe mode.

use std::time::{Duration, Instant};

use quarry_driver_api::{DriverError, ExecItem, ExecOpts, Session};
use quarry_model::{BrowseQuery, ConnId, Environment, HistoryEntry, TableRef, Value};

use crate::engine::{Engine, describe, round};
use crate::session::transaction_after;
use crate::{Event, NoticeKind, TabId, browse, perf};

/// Rows kept per result set of a typed query; more are drained but not sent (`truncated`).
pub(crate) const RUN_ROW_LIMIT: u64 = 1000;

/// SQL for a tab, with its parameters.
#[derive(Debug, Clone)]
pub(crate) struct Query {
    pub(crate) sql: String,
    pub(crate) params: Vec<Value>,
    /// Typed by the user: recorded in the history and may begin or end a transaction.
    pub(crate) from_user: bool,
}

impl Query {
    pub(crate) fn user(sql: String) -> Self {
        Self {
            sql,
            params: Vec::new(),
            from_user: true,
        }
    }
}

/// Run a statement whose result does not matter (the text protocol accepts any statement).
pub(crate) async fn run_plain(session: &mut dyn Session, sql: &str) -> Result<(), DriverError> {
    session
        .execute(sql, &ExecOpts::default(), &mut |_: ExecItem| {})
        .await
}

/// The only statement in `sql`, refusing scripts and statements that already explain.
pub(crate) fn single_statement(sql: &str) -> Result<&str, String> {
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

/// Safe mode: the reason the user must confirm, or `None` to run right away.
///
/// - staging: DROP / TRUNCATE / UPDATE or DELETE without WHERE
/// - production: any statement that is not a read
pub(crate) fn safety_check(environment: Environment, sql: &str) -> Option<String> {
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

impl Engine {
    pub(crate) async fn run_query(
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
        let started = Instant::now();
        let failed = |error: String| Event::QueryFailed {
            tab,
            error,
            cancelled: false,
        };
        let slot = self.tab_slot(tab);
        let mut slot = slot.lock().await;
        // Time spent waiting for the tab (a previous command still holding it).
        let wait_ms = perf::ms_since(started);
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

        // Opening the session and switching the database, if this query had to.
        let prepare_ms = perf::ms_since(started) - wait_ms;
        let events = self.events.clone();
        let mut set = 0u32;
        let mut elapsed = Duration::ZERO;
        let mut first_rows: Option<Instant> = None;
        let mut rows_sent = 0usize;
        let mut batches = 0u32;
        let mut sink = |item: ExecItem| {
            let event = match item {
                ExecItem::Columns(columns) => Event::ResultColumns { tab, set, columns },
                ExecItem::Rows(rows) => {
                    first_rows.get_or_insert_with(Instant::now);
                    rows_sent += rows.len();
                    batches += 1;
                    Event::ResultRows { tab, set, rows }
                }
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
        let executing = Instant::now();
        let result = if params.is_empty() {
            session.execute(&sql, &opts, &mut sink).await
        } else {
            session.query(&sql, &params, &opts, &mut sink).await
        };
        drop(session);
        if self.perf.is_some() {
            // Durations and counts only: never the SQL text or its values.
            self.perf(
                "query",
                started,
                serde_json::json!({
                    "tab": tab,
                    "from_user": from_user,
                    "statement": quarry_sql::first_keyword(&sql),
                    "sql_bytes": sql.len(),
                    "wait_ms": round(wait_ms),
                    "prepare_ms": round(prepare_ms),
                    "first_rows_ms": first_rows.map(|t| round(perf::ms_between(executing, t))),
                    "execute_ms": round(perf::ms_since(executing)),
                    "rows": rows_sent,
                    "batches": batches,
                    "sets": set,
                    "ok": result.is_ok(),
                }),
            );
        }
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
            let entry = HistoryEntry {
                sql,
                conn,
                database: tab_session.database.clone(),
                ran_at,
                elapsed_ms,
                error,
            };
            // Free the tab before writing the history: the write can wait behind history searches,
            // and the tab's next query must not wait with it.
            drop(slot);
            self.record(entry).await;
        }
    }

    /// `SELECT` for one page of `table`, in the dialect of the connection's driver.
    pub(crate) fn page_query(
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
    pub(crate) fn explain_query(
        &self,
        conn: ConnId,
        sql: &str,
        json: bool,
    ) -> Result<Query, String> {
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

    pub(crate) async fn count_rows(
        &self,
        tab: TabId,
        conn: ConnId,
        table: TableRef,
        query: BrowseQuery,
    ) {
        let result = self.count(tab, conn, &table, &query).await;
        let (count, error) = match result {
            Ok(count) => (Some(count), None),
            Err(error) => (None, Some(error)),
        };
        self.emit(Event::RowCount { tab, count, error });
    }

    pub(crate) async fn count(
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
}
