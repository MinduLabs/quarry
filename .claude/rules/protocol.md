---
paths:
  - crates/engine/**
  - crates/model/**
  - src-tauri/src/**
  - ui/src/bindings/**
  - ui/src/lib/engine.ts
---

# Protocol and engine

Rust owns the contract; TypeScript is generated. Flows: [ARCHITECTURE.md › Protocol](../../docs/ARCHITECTURE.md#protocol).

## Protocol

- Never add a Tauri command that talks to a database, the keyring or the disk. Bridge commands are
  pure functions over text (like `statement_at`) or forward to the engine.
- Every exported type derives `TS` with `#[ts(export)]`. A renamed or retyped field changes every
  consumer in `ui/src` in the same commit.
- Keep the tag style when adding variants: `Command` / `Event` use `#[serde(tag = "type")]`, `Value`
  uses `t` / `v`, `ConnState` uses `kind`.
- `Int` and `UInt` serialize as strings. A `u64` in an event uses `#[ts(type = "number")]` only when
  it stays below 2^53 in practice (row counts, milliseconds).
- A new request-and-response pair carries a `RequestId`; tab work is keyed by `TabId`.
- `Password` is the only way a secret enters a `Command`. A new secret-carrying field gets the same
  redacting `Debug`.
- The UI sends UTF-16 offsets, Rust works in bytes. Convert only in `src-tauri`.
- The UI never builds SQL. The engine builds it through the driver's `Dialect` and sends values as
  parameters.
- Anything that can lose data asks first through the engine: the command carries
  `confirmed: false`, the engine may answer with a confirm or review event, the client resends with
  `confirmed: true`. A new risky command follows the same pattern.

## Engine

- Never hold a `std::sync::Mutex` across an `.await`. Lock through `lock()`, which recovers from
  poisoning. The per-tab `tokio::sync::Mutex` is held while a query runs, by design.
- Cancel never goes through the busy session. It uses the driver's cancel handle, kept apart.
- DDL from the UI runs on the metadata session, never a tab session, so it cannot commit a user's
  open transaction.
- A lost connection is never retried by running the statement again.
- Blocking work (file reads, history search) goes to `spawn_blocking`.

## Tests

- Safe-mode rules live in `safety_check`; a change to it adds a case in `safe_mode_rules`.
- A new command path gets a unit test that runs without a database (temp config dir,
  `disable_keyring: true`), and an integration test in `mysql_tests.rs` when MySQL behavior matters.
