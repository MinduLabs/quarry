#![allow(clippy::unwrap_used)]

//! Benchmarks of the history file (`just bench`). Ignored by `cargo test`.

use std::time::Instant;

use quarry_model::{ConnId, HistoryEntry};
use quarry_store::HistoryStore;

const SQL: &str = "SELECT o.id, o.total, c.email FROM orders o JOIN customers c \
                   ON c.id = o.customer_id WHERE o.status = 'paid' ORDER BY o.created_at DESC";

fn ms(start: Instant, runs: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs)
}

#[test]
#[ignore = "benchmark: run with `just bench`"]
fn history_file() {
    let dir = std::env::temp_dir().join(format!("quarry-bench-{}", uuid::Uuid::new_v4()));
    let store = HistoryStore::new(&dir);
    let conns = [ConnId::new(), ConnId::new()];
    let entry = |i: usize| HistoryEntry {
        sql: format!("{SQL} -- {i}"),
        conn: conns[i % 2],
        database: Some("shop".into()),
        ran_at: chrono::Utc::now(),
        elapsed_ms: 3,
        error: None,
    };
    for i in 0..5000 {
        store.append(&entry(i)).unwrap();
    }
    let runs = 20;
    let start = Instant::now();
    for _ in 0..runs {
        store.search(None, None, 200).unwrap();
    }
    println!(
        "bench history search, 5000 entries: {:.2} ms",
        ms(start, runs)
    );
    let start = Instant::now();
    for _ in 0..runs {
        store
            .search(Some("customers"), Some(conns[0]), 200)
            .unwrap();
    }
    println!(
        "bench history search by text and connection: {:.2} ms",
        ms(start, runs)
    );
    let start = Instant::now();
    for i in 0..runs {
        store.append(&entry(i as usize)).unwrap();
    }
    println!("bench history append: {:.3} ms", ms(start, runs));
    // The append that passes 6000 lines rewrites the file with the newest 5000.
    for i in 0..(6000 - 5000 - 20 - 1) {
        store.append(&entry(i)).unwrap();
    }
    let start = Instant::now();
    store.append(&entry(0)).unwrap();
    store.append(&entry(1)).unwrap();
    println!(
        "bench history append that rewrites the file: {:.2} ms",
        ms(start, 1)
    );
    std::fs::remove_dir_all(dir).ok();
}
