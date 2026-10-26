//! The console of a tab: one entry per statement its session runs, with its timing and outcome.

use std::time::Instant;

use quarry_driver_api::DriverError;

use crate::engine::{Engine, describe, round};
use crate::{Event, LogEntry, LogSource, TabId, perf};

/// Statements longer than this are cut in the console; the run itself is not affected.
const SQL_LIMIT: usize = 4 * 1024;

/// A statement being timed for the console.
#[derive(Debug)]
pub(crate) struct Logged {
    started: Instant,
    first_rows: Option<Instant>,
    has_rows: bool,
    entry: LogEntry,
}

impl Logged {
    pub(crate) fn start(source: LogSource, sql: &str) -> Self {
        Self {
            started: Instant::now(),
            first_rows: None,
            has_rows: false,
            entry: LogEntry {
                at: chrono::Utc::now(),
                source,
                sql: cut(sql.trim()),
                execute_ms: 0.0,
                fetch_ms: None,
                rows: None,
                affected: None,
                truncated: false,
                error: None,
            },
        }
    }

    /// The statement returned a result set (its columns arrived).
    pub(crate) fn columns(&mut self) {
        self.has_rows = true;
    }

    /// A batch of rows arrived: the first one ends the execute time.
    pub(crate) fn rows(&mut self) {
        self.first_rows.get_or_insert_with(Instant::now);
    }

    /// A result set ended. A procedure call can end several; their counts add up.
    pub(crate) fn set_done(&mut self, affected: u64, rows: u64, truncated: bool) {
        if self.has_rows {
            self.entry.rows = Some(self.entry.rows.unwrap_or(0) + rows);
        }
        if affected > 0 {
            self.entry.affected = Some(self.entry.affected.unwrap_or(0) + affected);
        }
        self.entry.truncated |= truncated;
    }

    pub(crate) fn affected(&mut self, affected: u64) {
        self.set_done(affected, 0, false);
    }

    pub(crate) fn finish(mut self, error: Option<String>) -> LogEntry {
        let end = Instant::now();
        let answered = self.first_rows.unwrap_or(end);
        self.entry.execute_ms = round(perf::ms_between(self.started, answered));
        self.entry.fetch_ms = self
            .first_rows
            .map(|first| round(perf::ms_between(first, end)));
        self.entry.error = error;
        self.entry
    }
}

impl Engine {
    pub(crate) fn log(&self, tab: TabId, logged: Logged, error: Option<String>) {
        self.emit(Event::StatementLog {
            tab,
            entry: logged.finish(error),
        });
    }

    /// Run a step that only succeeds or fails (begin, commit, a savepoint), and log it as `sql`.
    pub(crate) async fn logged_step(
        &self,
        tab: TabId,
        source: LogSource,
        sql: &str,
        step: impl Future<Output = Result<(), DriverError>>,
    ) -> Result<(), DriverError> {
        let logged = Logged::start(source, sql);
        let result = step.await;
        self.log(tab, logged, result.as_ref().err().map(describe));
        result
    }
}

/// `sql` cut at `SQL_LIMIT` bytes, on a character boundary.
fn cut(sql: &str) -> String {
    if sql.len() <= SQL_LIMIT {
        return sql.to_owned();
    }
    let mut end = SQL_LIMIT;
    while !sql.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &sql[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_statements_are_cut_on_a_character_boundary() {
        let sql = format!("SELECT '{}'", "é".repeat(SQL_LIMIT));
        let shown = cut(&sql);
        assert!(shown.len() <= SQL_LIMIT + "…".len());
        assert!(shown.ends_with('…'));
        assert_eq!(cut("SELECT 1"), "SELECT 1");
    }

    #[test]
    fn rows_count_only_for_result_sets() {
        let mut update = Logged::start(LogSource::Query, "UPDATE t SET a = 1");
        update.set_done(3, 0, false);
        let entry = update.finish(None);
        assert_eq!((entry.rows, entry.affected), (None, Some(3)));
        assert_eq!(entry.fetch_ms, None);

        let mut select = Logged::start(LogSource::Page, "SELECT * FROM t");
        select.columns();
        select.rows();
        select.set_done(0, 0, false);
        select.columns();
        select.set_done(0, 2, true);
        let entry = select.finish(None);
        assert_eq!(
            (entry.rows, entry.affected, entry.truncated),
            (Some(2), None, true)
        );
        assert!(entry.fetch_ms.is_some());
    }
}
