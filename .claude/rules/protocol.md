---
paths:
  - crates/engine/**
  - crates/model/**
  - src-tauri/src/**
  - ui/src/bindings/**
  - ui/src/lib/engine.ts
---

# Protocol: Command, Event, bindings

The UI and the engine share one contract. Rust owns it; TypeScript is generated.

## Where it lives

- `crates/engine/src/command.rs`: `Command`, `Password`, `TabId`, `RequestId`.
- `crates/engine/src/event.rs`: `Event`, `ConnState`, `NoticeKind`.
- `crates/model/src/`: types that cross the boundary (`Value`, `ColumnMeta`, `ConnectionConfig`, `Capabilities`, `Environment`, `SchemaObject`).
- `src-tauri/src/main.rs`: the bridge (`engine_send`, `engine_subscribe`, `statement_at`, `script_statements`, `completion_context`). It forwards; it decides nothing.
- `ui/src/lib/engine.ts`: `send`, `subscribe`, `statementAt`. `ui/src/store/app.ts` reduces events into state.

## Rules

- Add a capability as a new `Command` and `Event` variant. Never add a Tauri command that talks to a database or the keyring.
- Every exported type derives `TS` with `#[ts(export)]`. Rename or retype a field → every consumer in `ui/src` changes in the same commit.
- Run `just bindings` and commit `ui/src/bindings/`. CI runs `git diff --exit-code ui/src/bindings`.
- Tagged enums: `Command` and `Event` use `#[serde(tag = "type")]`, `Value` uses `t` / `v`, `ConnState` uses `kind`. Keep the tag style when adding variants.
- 64-bit integers: `Int` and `UInt` serialize as strings. `u64` counters in events use `#[ts(type = "number")]` only when they stay below 2^53 in practice (row counts, milliseconds).
- Results are matched by `TabId` (query tabs, assigned by the client) or `RequestId` (one-off). A new request-and-response pair carries a `RequestId`.
- `Password` is the only way a secret enters a `Command`. Its `Debug` prints `Password(***)`. A new secret-carrying field gets the same treatment.
- `engine_subscribe` replaces the previous channel, so a reloaded webview just subscribes again. Do not assume one subscription per run.
- `statement_at`, `script_statements` and `completion_context` take and return UTF-16 offsets (CodeMirror) and convert to bytes at the bridge. Keep the conversion in `src-tauri`. Events name statements by index, never by offset.
- Safe mode stays in the engine: `Run{confirmed:false}` may answer `ConfirmRequired`, and `ApplyChanges{confirmed:false}` may answer `ChangesConfirmRequired`; the client resends with `confirmed:true`. No client may skip this by classifying SQL itself.
- The UI never builds SQL for a driver. Table pages go through `Browse` (the engine uses `Dialect::select_page`), grid edits through `ApplyChanges` (parameterized, keyed by the primary key).

## Engine internals to respect

- `Engine` state: `connections`, `passwords`, `meta` (one metadata session per connection), `tabs` (one session per tab), `cancels` (kept apart so a busy session can still be cancelled).
- The `std::sync::Mutex` fields are held briefly through `lock()`; never across an `.await`. The per-tab `tokio::sync::Mutex` is held while a query runs; that is by design.
- `emit` ignores a closed channel on purpose: it only fails after the client is gone.
- Session lifecycle: a tab session opens on first `Run`, closes on `CloseTab` and on `Disconnect` of its connection (running queries are cancelled first).

## Tests

`crates/engine/src/tests.rs` has unit tests (`safety_check` rules, tagged-JSON shape, ping) that spawn an engine with a temp config dir and `disable_keyring: true`, plus `mysql_tests.rs` gated by `QUARRY_TEST_MYSQL`. Safe-mode logic is the `safety_check` function; a change to it needs a case in `safe_mode_rules`. A new command path gets a unit test where it can run without a database, and an integration test when real MySQL behavior matters.
