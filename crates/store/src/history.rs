//! Query history in `<data dir>/history.jsonl`: one JSON entry per line, oldest first.
//!
//! Appending is one write per query. Once the file holds a fifth more than [`MAX_ENTRIES`], it is
//! rewritten with the newest [`MAX_ENTRIES`], so it never grows without bound.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use quarry_model::{ConnId, HistoryEntry};

use crate::{Result, StoreError, write_atomic};

const HISTORY_FILE: &str = "history.jsonl";
pub const MAX_ENTRIES: usize = 5000;

#[derive(Debug)]
pub struct HistoryStore {
    dir: PathBuf,
    /// Lines in the file, counted on first use. Also serializes writes.
    lines: Mutex<Option<usize>>,
}

impl HistoryStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            lines: Mutex::new(None),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(HISTORY_FILE)
    }

    fn io_err(&self) -> impl Fn(std::io::Error) -> StoreError + '_ {
        move |source| StoreError::Io {
            path: self.path(),
            source,
        }
    }

    /// Every readable entry, oldest first. Lines that do not parse are skipped.
    fn read_all(&self) -> Result<Vec<HistoryEntry>> {
        let text = match fs::read_to_string(self.path()) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(self.io_err()(err)),
        };
        Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    pub fn append(&self, entry: &HistoryEntry) -> Result<()> {
        let mut lines = self.lines.lock().unwrap_or_else(|p| p.into_inner());
        let count = match *lines {
            Some(count) => count,
            None => self.read_all()?.len(),
        };
        let mut line = serde_json::to_string(entry).map_err(|source| StoreError::Json {
            path: self.path(),
            source,
        })?;
        line.push('\n');
        fs::create_dir_all(&self.dir).map_err(self.io_err())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())
            .map_err(self.io_err())?;
        file.write_all(line.as_bytes()).map_err(self.io_err())?;
        let mut count = count + 1;

        if count > MAX_ENTRIES + MAX_ENTRIES / 5 {
            let all = self.read_all()?;
            let keep = &all[all.len().saturating_sub(MAX_ENTRIES)..];
            let mut text = String::new();
            for entry in keep {
                if let Ok(json) = serde_json::to_string(entry) {
                    text.push_str(&json);
                    text.push('\n');
                }
            }
            write_atomic(&self.path(), text.as_bytes())?;
            count = keep.len();
        }
        *lines = Some(count);
        Ok(())
    }

    /// Newest first. `query` matches the SQL, ignoring case; `conn` keeps one connection's.
    pub fn search(
        &self,
        query: Option<&str>,
        conn: Option<ConnId>,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>> {
        let needle = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_lowercase);
        let _guard = self.lines.lock().unwrap_or_else(|p| p.into_inner());
        Ok(self
            .read_all()?
            .into_iter()
            .rev()
            .filter(|e| conn.is_none_or(|c| e.conn == c))
            .filter(|e| {
                needle
                    .as_ref()
                    .is_none_or(|n| e.sql.to_lowercase().contains(n))
            })
            .take(limit)
            .collect())
    }

    pub fn clear(&self) -> Result<()> {
        let mut lines = self.lines.lock().unwrap_or_else(|p| p.into_inner());
        match fs::remove_file(self.path()) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(self.io_err()(err)),
        }
        *lines = Some(0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sql: &str) -> HistoryEntry {
        HistoryEntry {
            sql: sql.into(),
            conn: ConnId::new(),
            database: None,
            ran_at: chrono::Utc::now(),
            elapsed_ms: 3,
            error: None,
        }
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("quarry-history-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn appends_and_searches_newest_first() {
        let dir = temp_dir();
        let store = HistoryStore::new(&dir);
        assert!(store.search(None, None, 10).unwrap().is_empty());
        store.append(&entry("SELECT * FROM orders")).unwrap();
        store
            .append(&entry("select count(*) from customers"))
            .unwrap();
        store
            .append(&entry("UPDATE orders SET note = 'x'"))
            .unwrap();

        let all = store.search(None, None, 10).unwrap();
        assert_eq!(all[0].sql, "UPDATE orders SET note = 'x'");
        let orders = store.search(Some("ORDERS"), None, 10).unwrap();
        assert_eq!(orders.len(), 2);
        assert_eq!(store.search(None, None, 1).unwrap().len(), 1);
        let first = all[2].conn;
        let one = store.search(None, Some(first), 10).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].sql, "SELECT * FROM orders");

        store.clear().unwrap();
        assert!(store.search(None, None, 10).unwrap().is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn keeps_the_newest_entries_only() {
        let dir = temp_dir();
        let store = HistoryStore::new(&dir);
        for i in 0..(MAX_ENTRIES + MAX_ENTRIES / 5 + 1) {
            store.append(&entry(&format!("SELECT {i}"))).unwrap();
        }
        let all = store.search(None, None, usize::MAX).unwrap();
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(
            all[0].sql,
            format!("SELECT {}", MAX_ENTRIES + MAX_ENTRIES / 5)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skips_broken_lines() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let good = serde_json::to_string(&entry("SELECT 1")).unwrap();
        fs::write(dir.join(HISTORY_FILE), format!("{{broken\n{good}\n")).unwrap();
        let store = HistoryStore::new(&dir);
        assert_eq!(store.search(None, None, 10).unwrap().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
