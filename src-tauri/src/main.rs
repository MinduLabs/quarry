//! Tauri shell: hosts the web UI and bridges it to the engine.
//!
//! The bridge is deliberately thin — two commands:
//! - `engine_send(command)`: forward a [`Command`] to the engine;
//! - `engine_subscribe(on_event)`: stream every engine [`Event`] to the UI over a channel.
//!
//! All application logic lives in `quarry-engine`; nothing here talks to a database.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use quarry_engine::{Command, EngineHandle, Event};
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tracing_subscriber::EnvFilter;

type Subscriber = Arc<Mutex<Option<Channel<Event>>>>;

struct Bridge {
    engine: EngineHandle,
    subscriber: Subscriber,
}

#[tauri::command]
fn engine_send(bridge: State<'_, Bridge>, command: Command) -> Result<(), String> {
    bridge.engine.send(command).map_err(|err| err.to_string())
}

/// Replaces the previous subscriber, so a reloaded webview simply subscribes again.
#[tauri::command]
fn engine_subscribe(bridge: State<'_, Bridge>, on_event: Channel<Event>) {
    let mut subscriber = bridge.subscriber.lock().unwrap_or_else(|p| p.into_inner());
    *subscriber = Some(on_event);
}

/// UTF-16 range `(start, end)` of the SQL statement under the cursor (`offset`, UTF-16).
#[tauri::command]
fn statement_at(text: String, offset: usize) -> Option<(usize, usize)> {
    let byte = utf16_to_byte(&text, offset);
    let range = quarry_engine::sql::statement_at(&text, byte)?;
    Some((byte_to_utf16(&text, range.start), byte_to_utf16(&text, range.end)))
}

fn utf16_to_byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= utf16 {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

fn byte_to_utf16(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].encode_utf16().count()
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("QUARRY_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let (engine, events) = match EngineHandle::spawn() {
        Ok(pair) => pair,
        Err(err) => {
            tracing::error!(%err, "failed to start engine");
            std::process::exit(1);
        }
    };
    let subscriber: Subscriber = Arc::default();

    let forward = subscriber.clone();
    let result = tauri::Builder::default()
        .manage(Bridge { engine, subscriber })
        .invoke_handler(tauri::generate_handler![engine_send, engine_subscribe, statement_at])
        .setup(move |app| {
            // Forward engine events to whichever webview subscribed last.
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let channel = forward.lock().unwrap_or_else(|p| p.into_inner()).clone();
                    if let Some(channel) = channel
                        && let Err(err) = channel.send(event)
                    {
                        tracing::warn!(%err, "event channel");
                    }
                }
            });
            if let Some(window) = app.get_webview_window("main") {
                tracing::debug!(label = window.label(), "window ready");
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        tracing::error!(%err, "tauri");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_roundtrip() {
        let text = "SELECT '👋';\nSELECT 2";
        let second = text.find("SELECT 2").unwrap();
        let utf16 = byte_to_utf16(text, second);
        assert_eq!(utf16_to_byte(text, utf16), second);
        assert_eq!(statement_at(text.to_owned(), utf16 + 1), Some((utf16, utf16 + 8)));
    }
}
