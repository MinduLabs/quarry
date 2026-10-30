//! Tauri shell: hosts the web UI and bridges it to the engine.
//!
//! The bridge is deliberately thin — two commands:
//! - `engine_send(command)`: forward a [`Command`] to the engine;
//! - `engine_subscribe(on_event)`: stream every engine [`Event`] to the UI over a channel.
//!
//! plus pure text helpers that convert offsets between bytes and UTF-16 (`statement_at`,
//! `script_statements`, `completion_context`).
//!
//! All application logic lives in `quarry-engine`; nothing here talks to a database.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod desktop;

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
    Some((
        byte_to_utf16(&text, range.start),
        byte_to_utf16(&text, range.end),
    ))
}

/// UTF-16 ranges of the statements a `Run` of `text` sends, in order (`quarry_sql::script`),
/// so the UI can point at the statement an event names.
#[tauri::command]
fn script_statements(text: String) -> Vec<(usize, usize)> {
    quarry_engine::sql::script(&text)
        .into_iter()
        .map(|range| {
            (
                byte_to_utf16(&text, range.start),
                byte_to_utf16(&text, range.end),
            )
        })
        .collect()
}

/// What fits at UTF-16 `offset` of `text` in a SQL editor (`quarry_sql::completion_context`),
/// with `word_start` in UTF-16 units. Pure and fast enough to answer every keystroke.
#[tauri::command]
fn completion_context(
    text: String,
    offset: usize,
) -> Option<quarry_engine::sql::CompletionContext> {
    let byte = utf16_to_byte(&text, offset);
    let mut context = quarry_engine::sql::completion_context(&text, byte)?;
    let word_start = usize::try_from(context.word_start).unwrap_or(text.len());
    context.word_start = u32::try_from(byte_to_utf16(&text, word_start)).unwrap_or(u32::MAX);
    Some(context)
}

/// Two fingers on the touchpad zoom the whole page in WebKitGTK, which turns the app into a
/// magnified picture (the layout reflows to a fraction of the window and `devicePixelRatio`
/// jumps). The page gets no zoom event it could cancel, so the gesture is stopped on the webview
/// before the engine reads it.
#[cfg(target_os = "linux")]
fn block_pinch_zoom(window: &tauri::WebviewWindow) {
    use gtk::glib::Propagation;
    use gtk::prelude::WidgetExt;
    let result = window.with_webview(|webview| {
        webview.inner().connect_event(|_, event| {
            if event.event_type() == gtk::gdk::EventType::TouchpadPinch {
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        });
    });
    if let Err(err) = result {
        tracing::warn!(%err, "pinch zoom guard");
    }
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
        .with_env_filter(
            EnvFilter::try_from_env("QUARRY_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    #[cfg(target_os = "linux")]
    desktop::integrate();

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
        // Where to save an export. The capability allows the save dialog only: the UI cannot open
        // files or read the disk through it, since the engine does all file work.
        .plugin(tauri_plugin_dialog::init())
        // Size, position and maximized state come back on the next launch. Not `VISIBLE` (the
        // plugin would show the window before the splash is drawn, see `ui/public/show.js`) and
        // not `DECORATIONS` (Linux draws its own window controls).
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .manage(Bridge { engine, subscriber })
        .invoke_handler(tauri::generate_handler![
            engine_send,
            engine_subscribe,
            statement_at,
            script_statements,
            completion_context
        ])
        // The window opens hidden and the page shows it once the splash is drawn
        // (`ui/public/show.js`), so it never shows an empty white window. A page that failed to
        // run that still gets its window shown when it has loaded.
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && let Err(err) = webview.window().show()
            {
                tracing::warn!(%err, "show window");
            }
        })
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
            #[cfg(target_os = "linux")]
            if let Some(window) = app.get_webview_window("main") {
                block_pinch_zoom(&window);
            }
            // X11 and dev runs on macOS read the icon from the window/app, not the bundle.
            // 256px: GDK silently drops a 512px _NET_WM_ICON.
            if let Some(window) = app.get_webview_window("main")
                && let Err(err) = window.set_icon(tauri::include_image!("icons/128x128@2x.png"))
            {
                tracing::warn!(%err, "window icon");
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
        assert_eq!(
            statement_at(text.to_owned(), utf16 + 1),
            Some((utf16, utf16 + 8))
        );
        assert_eq!(
            script_statements(text.to_owned()),
            [(0, 11), (utf16, utf16 + 8)]
        );
        let typed = format!("{text} WHERE x");
        let end = typed.encode_utf16().count();
        let context = completion_context(typed, end).expect("a context");
        assert_eq!(context.word_start as usize, end - 1);
    }
}
