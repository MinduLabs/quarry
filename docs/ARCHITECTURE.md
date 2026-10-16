# Architecture

Quarry is a desktop database client for Linux and macOS: a Tauri 2 shell around a React UI, with all
logic in a headless Rust core. App ID: `io.github.mindulabs.Quarry`.

## Principles

1. **Headless core.** Everything that talks to a database, the keyring or the disk lives in
   `crates/engine` and below. The UI is one client of the engine; a CLI or MCP server would be another.
2. **One-way dependencies:** `src-tauri → engine → {driver-api, sql, store, tunnel} → model`.
   Nothing above `engine` imports a concrete driver.
3. **One protocol.** UI ↔ core talk only through `Command` (requests) and `Event` (results), both
   serde-serializable. TypeScript types are generated from Rust, so the two sides cannot drift.
4. **Drivers declare capabilities; the UI reads them.** No `if driver == "mysql"` in the UI.
5. **Results are bounded.** A query streams rows in batches and stops at a row limit; nothing loads a
   whole table into memory.
6. **Decisions that protect data live in the engine.** Safe-mode confirmations are an engine round trip,
   so every client gets the same protection.

## Layers

```
┌──────────────────────────────────────────────┐
│ ui/  React + TypeScript (Vite, Tailwind v4)  │  CodeMirror editor, virtualized grid, zustand store
└──────────────────────┬───────────────────────┘
                       │ invoke("engine_send") / Channel<Event>   (JSON)
┌──────────────────────▼───────────────────────┐
│ src-tauri/  thin bridge, no business logic   │  engine_send · engine_subscribe · statement_at
└──────────────────────┬───────────────────────┘
                       │ Command / Event (async_channel)
┌──────────────────────▼───────────────────────┐
│ crates/engine  own tokio runtime             │  connections, per-tab sessions, safe mode
└───┬───────────┬───────────┬───────────┬──────┘
    ▼           ▼           ▼           ▼
 driver-api    sql        store       tunnel
    │        (split,    (config,     (SSH, planned)
    ▼        classify)   keyring)
 drivers/mysql
    └───────────┴───────────┴───────────┘
                       ▼
                 crates/model   plain types, no IO
```

| Crate | Responsibility |
|---|---|
| `model` | `Value`, `ColumnMeta`, `SchemaObject`, `ConnectionConfig`, `Capabilities`, `Environment`. Exports TypeScript types (ts-rs). |
| `driver-api` | `Driver` / `Session` traits, `ExecItem`, shared error type. |
| `drivers/mysql` | MySQL / MariaDB via `mysql_async` + rustls: connect, SSL modes, multi-statement scripts, cancel with `KILL QUERY` on a side connection, introspection through `information_schema`. |
| `sql` | Statement splitting, statement at cursor, classification (read / write / DDL / destructive). |
| `store` | Saved connections (TOML, no secrets) and passwords in the OS keyring. |
| `tunnel` | SSH port forwarding (planned). |
| `engine` | Owns the runtime and all state; turns `Command`s into `Event`s. Exports TypeScript types. |
| `src-tauri` | Window, bundling, and the three bridge commands. |

## Protocol

`Command` and `Event` (`crates/engine/src/{command,event}.rs`) are tagged JSON enums
(`{"type": "Run", ...}`). `cargo test -p quarry-model -p quarry-engine` (`just bindings`) regenerates
`ui/src/bindings/*.ts`; never edit those files by hand.

- Commands are fire-and-forget. Results arrive as events, matched by `TabId` (query tabs) or
  `RequestId` (one-off requests such as Test Connection).
- `engine_subscribe` hands the engine a Tauri `Channel`. A reloaded webview subscribes again and replaces
  the old channel.
- Integers that do not fit a JavaScript number (`BIGINT`, `BIGINT UNSIGNED`, `DECIMAL`) travel as
  strings, so values are shown exactly.
- Passwords travel in `Password`, whose `Debug` impl never prints the secret.

Main flows:

```
Connect      Connect{id} → ConnState(Connecting) → ConnState(Connected) → Databases
Browse       LoadObjects{id, database} → Objects
Query        Run{tab, sql} → QueryStarted → (ResultColumns → ResultRows* → ResultDone)* → QueryFinished | QueryFailed
Safe mode    Run{confirmed:false} → ConfirmRequired → user agrees → Run{confirmed:true}
```

## Runtime

- The engine runs a multi-threaded tokio runtime on its own thread; the UI never blocks on IO.
- Each query tab owns one database session, so a long query in one tab does not block another.
  Each connection also has a metadata session for introspection.
- Rows are sent in batches of about 500 and capped at 1000 per result set (`truncated: true` beyond that).
- Cancel goes around the session (driver cancel handle), so it works while a query is running.

## Safe mode

| Environment | Behaviour |
|---|---|
| Local | Runs everything. |
| Staging | Asks before `DROP`, `TRUNCATE`, and `UPDATE`/`DELETE` without `WHERE`. |
| Production | Asks before every write; can be set read-only. The UI marks the connection in red. |

Classification comes from `quarry-sql`; the decision is made by the engine.

## Storage

| Data | Location |
|---|---|
| Connections | `connections.toml` in the OS config dir (`~/.config/quarry`, `~/Library/Application Support/quarry`). Never contains secrets. Has a `version` field for migrations. |
| Passwords | Linux: Secret Service / Secret portal via `oo7`. macOS: Keychain via `security-framework`. |

## UI

`ui/src` is organized by feature: `features/{sidebar,connections,tabs,editor,query,grid,welcome}`,
shared primitives in `components/ui`, global state in `store/app.ts` (zustand; it is also where engine
events are reduced), and the bridge in `lib/engine.ts`. Look and motion follow
[STYLE-GUIDE.md](STYLE-GUIDE.md).

- **Editor:** CodeMirror 6 with SQL highlighting and schema-aware completion; `Ctrl/⌘+Enter` runs the
  statement under the cursor (range computed in Rust by `statement_at`).
- **Grid:** row virtualization (`@tanstack/react-virtual`), sticky header and row-number column,
  resizable columns, cell selection and copy.

## Packaging

| Platform | Output | How |
|---|---|---|
| Linux | Flatpak (GNOME 51 runtime, WebKitGTK 4.1) | `just flatpak` → `dist/quarry-<version>.flatpak` |
| Linux | AppImage, `.deb` | `just linux-bundles` (inside Docker) → `dist/linux/` |
| macOS | `.app` / `.dmg` (universal) | `just dmg` on a Mac (release, CI), or `just mac-cross` from Linux → `dist/macos/` (ad-hoc signed, not notarized) |

The desktop entry, AppStream metadata and icons live in `packaging/linux/` and `src-tauri/icons/`.
On Wayland the dock takes the icon from the desktop entry named after the app ID, not from the window.
Flatpak and system packages install that entry; for unpackaged runs (`just dev`, AppImage) the app
installs its own entry and icon into `~/.local/share` on startup (`src-tauri/src/desktop.rs`).

## Testing

| Layer | How |
|---|---|
| `model`, `sql`, `store`, `engine` | Unit tests (`cargo test --workspace`) |
| `drivers/mysql`, engine end-to-end | Integration tests against MySQL in Docker (`just db-up && just test-db`) |
| UI | `pnpm check` (Biome + TypeScript); manual check in light and dark |
