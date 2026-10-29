#![allow(clippy::unwrap_used, clippy::expect_used)]

//! End-to-end benchmarks against the test MySQL (`just db-up && just bench`). Ignored by
//! `cargo test`, and skipped when `QUARRY_TEST_MYSQL` is unset.

use std::time::{Duration, Instant};

use quarry_engine::{Command, EngineEvents, EngineHandle, EngineOptions, Event, Password};
use quarry_model::{ConnId, ConnectionConfig, DriverId, Environment, HistoryEntry, SslMode};

async fn until(events: &EngineEvents, pred: impl Fn(&Event) -> bool) {
    let wait = async {
        loop {
            let event = events.recv().await.unwrap();
            // What the Tauri bridge does with every event, so the time covers the rows'
            // trip to JSON wherever it happens.
            std::hint::black_box(serde_json::to_string(&event).unwrap());
            if pred(&event) {
                return;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(30), wait)
        .await
        .expect("event in time");
}

fn summary(mut times: Vec<f64>) -> String {
    times.sort_by(f64::total_cmp);
    let at = |p: usize| times[(times.len() * p / 100).min(times.len() - 1)];
    format!(
        "median {:.1} ms, p90 {:.1} ms, max {:.1} ms",
        at(50),
        at(90),
        at(100)
    )
}

#[tokio::test]
#[ignore = "benchmark: run with `just bench`"]
async fn query_latency() {
    let Ok(addr) = std::env::var("QUARRY_TEST_MYSQL") else {
        println!("bench skipped: set QUARRY_TEST_MYSQL (just db-up)");
        return;
    };
    let (host, port) = addr.split_once(':').unwrap();
    let dir = std::env::temp_dir().join(format!("quarry-bench-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let id = ConnId::new();
    // A full history, as after months of use.
    let entry = HistoryEntry {
        sql: "SELECT o.id, o.total FROM orders o WHERE o.status = 'paid' ORDER BY o.id DESC".into(),
        conn: id,
        database: Some("shop".into()),
        ran_at: chrono::Utc::now(),
        elapsed_ms: 3,
        error: None,
    };
    let line = format!("{}\n", serde_json::to_string(&entry).unwrap());
    std::fs::write(dir.join("history.jsonl"), line.repeat(5000)).unwrap();

    let (engine, events) = EngineHandle::spawn_with(EngineOptions {
        config_dir: Some(dir.clone()),
        data_dir: Some(dir.clone()),
        known_hosts: None,
        disable_keyring: true,
        perf_log: None,
        driver_dir: None,
        driver_manifest: None,
        in_process_drivers: Default::default(),
    })
    .unwrap();
    engine
        .send(Command::SaveConnection {
            config: ConnectionConfig {
                id,
                name: "bench".into(),
                driver: DriverId("mysql".into()),
                host: Some(host.into()),
                port: port.parse().ok(),
                database: Some("shop".into()),
                user: Some("quarry".into()),
                path: None,
                ssl: SslMode::Prefer,
                ssh: None,
                environment: Environment::Local,
                read_only: false,
                color: None,
                group: None,
            },
            password: Some(Password("quarry".into())),
            ssh_secret: None,
            remember_password: false,
        })
        .unwrap();
    until(&events, |e| matches!(e, Event::Connections { .. })).await;

    let run = || Command::Run {
        tab: 1,
        conn: id,
        database: Some("shop".into()),
        sql: "SELECT * FROM orders LIMIT 1000".into(),
        confirmed: false,
    };
    let finished = |e: &Event| matches!(e, Event::QueryFinished { tab: 1, .. });
    // Warm up: open the session, fill caches.
    for _ in 0..3 {
        engine.send(run()).unwrap();
        until(&events, finished).await;
    }
    for searching in [false, true] {
        let mut times = Vec::new();
        for i in 0..20u32 {
            if searching {
                // Typing in the history search: a few searches in flight.
                for r in 0..6 {
                    engine
                        .send(Command::LoadHistory {
                            request: 1000 + i * 10 + r,
                            query: None,
                            conn: None,
                            limit: 200,
                        })
                        .unwrap();
                }
            }
            let start = Instant::now();
            engine.send(run()).unwrap();
            until(&events, finished).await;
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let label = if searching {
            "while the history is searched"
        } else {
            "alone"
        };
        println!("bench query 1000 rows, {label}: {}", summary(times));
    }
    std::fs::remove_dir_all(dir).ok();
}
