//! Performance log for development: one JSON object per line in `perf.jsonl`.
//!
//! Off unless `QUARRY_PERF` is set (`1` for `<data dir>/perf.jsonl`, or a file path) or the client
//! passes `EngineOptions::perf_log`. Lines are written by a thread of their own, so timing never
//! adds IO to the runtime. Records hold durations and counts, never SQL text or values.

use std::fs::OpenOptions;
use std::io::{LineWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use serde_json::{Map, Value, json};

/// Where `QUARRY_PERF` asks for the log, if anywhere.
pub fn path_from_env(data_dir: Option<&Path>) -> Option<PathBuf> {
    resolve(&std::env::var("QUARRY_PERF").ok()?, data_dir)
}

fn resolve(value: &str, data_dir: Option<&Path>) -> Option<PathBuf> {
    match value.trim() {
        "" | "0" | "false" => None,
        "1" | "true" => data_dir.map(|dir| dir.join("perf.jsonl")),
        path => Some(PathBuf::from(path)),
    }
}

#[derive(Debug)]
pub struct PerfLog {
    lines: mpsc::Sender<String>,
}

impl PerfLog {
    /// Open (append) the log and start its writer thread.
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let (lines, rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("quarry-perf".into())
            .spawn(move || {
                let mut out = LineWriter::new(file);
                while let Ok(line) = rx.recv() {
                    if let Err(err) = writeln!(out, "{line}") {
                        tracing::warn!(%err, "perf log write");
                        return;
                    }
                }
            })?;
        tracing::info!(path = %path.display(), "performance log on");
        Ok(Self { lines })
    }

    /// Log `name` taking `ms`, with extra `fields` (an object; anything else is dropped).
    pub fn record(&self, source: &str, name: &str, ms: f64, fields: Value) {
        let mut line = Map::new();
        line.insert("ts".into(), json!(chrono::Utc::now().to_rfc3339()));
        line.insert("source".into(), json!(source));
        line.insert("name".into(), json!(name));
        line.insert("ms".into(), json!((ms * 1000.0).round() / 1000.0));
        if let Value::Object(extra) = fields {
            line.extend(extra);
        }
        let line = Value::Object(line).to_string();
        tracing::debug!(target: "quarry::perf", "{line}");
        // The writer only stops if its file failed; then there is nothing left to log to.
        let _ = self.lines.send(line);
    }
}

/// Milliseconds since `start`, as a float.
pub fn ms_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// Milliseconds from `start` to `end`.
pub fn ms_between(start: Instant, end: Instant) -> f64 {
    end.saturating_duration_since(start).as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_one_json_object_per_line() {
        let dir = std::env::temp_dir().join(format!("quarry-perf-{}", uuid::Uuid::new_v4()));
        let path = dir.join("perf.jsonl");
        let log = PerfLog::open(path.clone()).unwrap();
        log.record("engine", "query", 12.3456, json!({ "rows": 1000 }));
        log.record("ui", "render.grid", 3.0, json!(null));
        drop(log);
        // The writer thread ends once the sender is dropped; give it a moment to flush.
        let mut text = String::new();
        for _ in 0..50 {
            text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.lines().count() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["name"], "query");
        assert_eq!(lines[0]["ms"], 12.346);
        assert_eq!(lines[0]["rows"], 1000);
        assert_eq!(lines[1]["source"], "ui");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn env_values() {
        let dir = Some(Path::new("/data"));
        assert_eq!(resolve("1", dir), Some(PathBuf::from("/data/perf.jsonl")));
        assert_eq!(resolve(" 0 ", dir), None);
        assert_eq!(resolve("1", None), None);
        assert_eq!(
            resolve("/tmp/p.jsonl", dir),
            Some(PathBuf::from("/tmp/p.jsonl"))
        );
    }
}
