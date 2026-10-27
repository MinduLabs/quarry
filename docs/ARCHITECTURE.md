# Architecture

How Quarry is built and why. What the app does is in [CHANGELOG.md](../CHANGELOG.md); how to run it
is in [README.md](../README.md).

Quarry is a Tauri 2 shell around a React UI, with all logic in a headless Rust core.

## Principles

1. **Headless core.** Everything that talks to a database, the keyring or the disk lives in
   `crates/engine` and below. The UI is one client of the engine; a CLI or MCP server would be another.
2. **One-way dependencies:** `src-tauri → engine → {driver-api, sql, store, tunnel} → model`.
   Only the engine names a concrete driver.
3. **One protocol.** UI ↔ core talk only through `Command` (requests) and `Event` (results).
   TypeScript types are generated from Rust, so the two sides cannot drift.
4. **Drivers declare capabilities; the UI reads them.** No `if driver == "mysql"` anywhere.
5. **Results are bounded.** A query streams rows in batches and stops at a row limit; nothing loads a
   whole table into memory.
6. **Decisions that protect data live in the engine.** Safe-mode confirmations are an engine round
   trip, so every client gets the same protection.

## Layers

```
┌──────────────────────────────────────────────┐
│ ui/  React + TypeScript (Vite, Tailwind v4)  │  CodeMirror editor, virtualized grid, zustand store
└──────────────────────┬───────────────────────┘
                       │ invoke("engine_send") / Channel<Event>   (JSON)
┌──────────────────────▼───────────────────────┐
│ src-tauri/  thin bridge, no business logic   │  forwards commands; pure text helpers from quarry-sql
└──────────────────────┬───────────────────────┘
                       │ Command / Event (async_channel)
┌──────────────────────▼───────────────────────┐
│ crates/engine  own tokio runtime             │  connections, per-tab sessions, safe mode
└───┬───────────┬───────────┬───────────┬──────┘
    ▼           ▼           ▼           ▼
 driver-api    sql        store       tunnel
    │
    ▼
 drivers/mysql
    └───────────┴───────────┴───────────┘
                       ▼
                 crates/model   plain types, no IO
```

| Crate | Responsibility |
|---|---|
| `model` | Plain types that cross the boundary (`Value`, `ColumnMeta`, `ConnectionConfig`, `Capabilities`, …). Exports TypeScript types (ts-rs). |
| `driver-api` | `Driver` / `Session` traits, `Dialect` (quoting, placeholders, paging, DDL), `ExecItem`, the shared error type. |
| `drivers/mysql` | MySQL / MariaDB via `mysql_async` + rustls. Introspection through `information_schema`, cancel with `KILL QUERY` on a side connection. |
| `sql` | Statement splitting, statement at cursor, classification (read / write / schema / destructive), transaction effect, completion context. No IO. |
| `store` | Saved connections and favorites (TOML, no secrets), open tabs, query history, secrets in the OS keyring. |
| `tunnel` | SSH local port forwarding via `russh`. |
| `engine` | Owns the runtime and all state; turns `Command`s into `Event`s. Exports TypeScript types. |
| `src-tauri` | Window, bundling, the bridge. |

## Protocol

`Command` and `Event` (`crates/engine/src/{command,event}.rs`) are tagged JSON enums
(`{"type": "Run", ...}`); those files are the full list. `just bindings` regenerates
`ui/src/bindings/`.

- Commands are fire-and-forget. Results arrive as events matched by `TabId` (query tabs, assigned by
  the client) or `RequestId` (one-off requests).
- `engine_subscribe` hands the engine a Tauri `Channel`. A reloaded webview subscribes again and
  replaces the old channel.
- Integers that do not fit a JavaScript number (`BIGINT`, `BIGINT UNSIGNED`, `DECIMAL`) travel as
  strings, so values are shown exactly.
- Passwords travel in `Password`, whose `Debug` never prints the secret.
- Text helpers that need no state (statement at cursor, script ranges, completion context) are bridge
  commands calling `quarry-sql` directly. The UI works in UTF-16 offsets, Rust in bytes; the bridge
  converts.

Typical flows:

```
Connect      Connect{id} → ConnState(Connecting) → ConnState(Connected{capabilities}) → Databases
Query        Run{tab, sql} → QueryStarted → (StatementStarted → (ResultColumns → ResultRows* → ResultDone)*)* → QueryFinished | QueryFailed
Safe mode    Run{confirmed:false} → ConfirmRequired → user agrees → Run{confirmed:true}
Schema       AlterSchema{change, confirmed:false} → SchemaReview{statements} → user agrees → AlterSchema{confirmed:true} → SchemaChanged
```

## Runtime

- The engine runs a multi-threaded tokio runtime on its own thread; the UI never blocks on IO.
  Blocking work (history files) goes to `spawn_blocking`, so it never holds a worker that streams
  results.
- **Sessions.** Each query tab owns one database session, so a long query in one tab does not block
  another. Each connection also has a metadata session for introspection and DDL. A connection over
  SSH has one tunnel shared by all of them.
- **Bounded rows.** Rows are sent in batches and capped per result set (`RUN_ROW_LIMIT`);
  `truncated: true` beyond. The driver drains the rest from the socket without buffering it.
- **Cancel** goes around the session through the driver's cancel handle, so it works while a query
  runs.
- **Scripts** run one statement at a time on the tab's session and stop at the first failure; the
  statements before it stay applied. A compound statement without `DELIMITER` goes to the server
  whole, since only the server can tell where its body ends.
- **Lost connections.** A tab session whose connection is gone is replaced once, and the user is told
  what happened to an open transaction. A statement is never run again.
- **Transactions** are tracked per tab session, from both commands and typed SQL. Closing the tab or
  disconnecting rolls back; the UI asks first.
- **Grid edits** run on the tab's session in one transaction (a savepoint inside a user's open one).
  They are parameterized and keyed by the primary key, else a unique NOT NULL key, else every column
  that compares exactly. Each UPDATE / DELETE must hit exactly one row or the batch rolls back.
- **Schema changes** are a `SchemaChange` value; the driver's `Dialect` writes the DDL. The engine
  always answers with a review first, rebuilds the statements from the change on confirm, and runs
  them on the metadata session so a user's open transaction is not committed by DDL.
- **Table pages** are built by the engine through the dialect, with every filter value sent as a
  parameter. The UI never writes SQL for a driver.
- **Statement log.** Every statement a tab's session runs ends with a `StatementLog` event, so every
  client gets the same log. It is kept in memory only.
- **Performance log** (`perf.rs`): with `QUARRY_PERF` set, the engine appends timings from its own
  thread and the UI adds its render times through `PerfMarks`. Durations and counts only.

## Safe mode

| Environment | Behaviour |
|---|---|
| Local | Runs everything. |
| Staging | Asks before `DROP`, `TRUNCATE`, and `UPDATE` / `DELETE` without `WHERE`. |
| Production | Asks before every write, including grid edits; can be read-only. |

Classification comes from `quarry-sql`; an unknown statement counts as a write. The decision is made
by `safety_check` in the engine. `EXPLAIN ANALYZE <stmt>` is treated like `<stmt>`, since it runs it.

## Storage

| Data | Location |
|---|---|
| Connections, favorites | `connections.toml`, `favorites.toml` in the OS config dir (`~/.config/quarry`, `~/Library/Application Support/quarry`). Never secrets. |
| Open tabs | `workspace.json` in the OS data dir (`~/.local/share/quarry`, `~/Library/Application Support/quarry`). Results are not saved. An unreadable file is moved aside, not overwritten. |
| Query history | `history.jsonl` in the data dir, capped (`MAX_ENTRIES`). |
| Secrets | OS keyring, service `quarry`: Secret Service via `oo7` on Linux, Keychain via `security-framework` on macOS. Account `<connection uuid>` for the database password, `<connection uuid>:ssh` for the SSH secret. |
| SSH host keys | Read from `~/.ssh/known_hosts`, never written. |
| View preferences | The webview's `localStorage` (theme, sidebar), so they apply before the engine answers. Anything another client would need goes through the engine instead. |

`connections.toml`, `favorites.toml` and `workspace.json` have a `version` field. A file newer than
the app knows is refused, never rewritten; an older one is read and migrated.

## UI

- State lives in one zustand store (`store/app.ts`), which also reduces engine events. Components
  read state and call actions; only actions call the engine.
- Every tab stays mounted so switching is instant. Big components are memoized with stable props, and
  the grid and sidebar tree are virtualized, so a keystroke or a large result never re-renders more
  than what is on screen.
- Completion context and statement ranges are computed in Rust, so the editor and the engine agree
  on where statements start and end. Syntax checks parse in a Web Worker so typing never waits on
  them. None of this decides anything about safety.
- The window opens hidden behind a splash drawn by `index.html` before any app code runs, so the user
  never sees an empty white window while WebKit starts.

## Packaging

- **Linux base: Ubuntu 22.04.** Tauri's guide says to build on the oldest distro you support that
  still ships WebKitGTK 4.1; the binary needs the glibc it was built with.
- **Caching.** `docker/linux.Dockerfile` is multi-stage: toolchains and packages are cached layers,
  Cargo's registry and target are BuildKit cache mounts. The final `scratch` stage holds only the
  bundle, so the build writes just that file.
- **`.dmg` only on macOS.** Tauri cannot cross-build macOS bundles.
- **Size.** The release profile follows Tauri's size guide (<https://v2.tauri.app/concept/size/>).
  `panic = "abort"` is left out so a panic in one engine task does not kill the app.
- **Desktop entry.** On Wayland the dock takes the icon from the desktop entry named after the app ID.
  Packages install it; unpackaged runs install their own on startup (`src-tauri/src/desktop.rs`).
- **Release.** A `v*` tag runs CI, builds every package on GitHub runners and publishes them.
