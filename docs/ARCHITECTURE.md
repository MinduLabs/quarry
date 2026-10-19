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
    │        (split,    (config,     (SSH port
    ▼        classify)   keyring,    forwarding)
                         history)
 drivers/mysql
    └───────────┴───────────┴───────────┘
                       ▼
                 crates/model   plain types, no IO
```

| Crate | Responsibility |
|---|---|
| `model` | `Value`, `ColumnMeta`, `SchemaObject`, `ConnectionConfig`, `Capabilities`, `Environment`. Exports TypeScript types (ts-rs). |
| `driver-api` | `Driver` / `Session` traits (including `table_info`), `Dialect` (quoting, placeholders, paging, literals, DDL for a `SchemaChange`), `ExecItem`, shared error type. |
| `drivers/mysql` | MySQL / MariaDB via `mysql_async` + rustls: connect, SSL modes, multi-statement scripts, cancel with `KILL QUERY` on a side connection, introspection through `information_schema`. |
| `sql` | Statement splitting, statement at cursor, classification (read / write / DDL / destructive), transaction effect (`BEGIN`, `COMMIT`, implicit commits). |
| `store` | Saved connections and favorites (TOML, no secrets), open tabs (`workspace.json`), query history (`history.jsonl`), secrets in the OS keyring. |
| `tunnel` | SSH local port forwarding via `russh`: ssh-agent, key file or password; the host key must be in `known_hosts`. |
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
Connect      Connect{id} → ConnState(Connecting) → ConnState(Connected{capabilities}) → Databases
Tree         LoadObjects{id, database} → Objects;  LoadColumns{id, database} → Columns (completion)
Query        Run{tab, sql} → QueryStarted → (ResultColumns → ResultRows* → ResultDone)* → QueryFinished | QueryFailed → HistoryAdded
Table page   Browse{tab, table, limit, offset, query} → same events as Query (the engine builds the SELECT for the dialect)
Row count    CountRows{tab, table, query} → RowCount (on the tab's session, so Cancel stops it)
Query plan   Explain{tab, sql, json} → same events as Query (one statement; it never runs)
Transaction  BeginTransaction{tab} / EndTransaction{tab, commit} → TransactionState
Structure    LoadTableInfo{request, conn, table} → TableInfo
Server       LoadServerInfo{request, conn, kind} → ServerInfo   (variables or status; Capabilities.server_info)
Grid edits   ApplyChanges{tab, table, changes} → ChangesApplied | ChangesFailed (one transaction)
Copy INSERT  InsertSql{request, conn, table, columns, rows} → InsertSql (text only; nothing runs)
Schema       AlterSchema{request, change, confirmed:false} → SchemaReview{statements, warning}
             → user agrees → AlterSchema{confirmed:true} → SchemaChanged (on the metadata session)
Safe mode    Run{confirmed:false} → ConfirmRequired → user agrees → Run{confirmed:true}
             ApplyChanges{confirmed:false} → ChangesConfirmRequired (production) → … {confirmed:true}
Tabs         LoadWorkspace → Workspace;  SaveWorkspace{request} → WorkspaceSaved
History      LoadHistory{request, query} → History;  ClearHistory
Favorites    LoadFavorites / SaveFavorite / DeleteFavorite → Favorites (the full list)
```

## Runtime

- The engine runs a multi-threaded tokio runtime on its own thread; the UI never blocks on IO.
- Each query tab owns one database session, so a long query in one tab does not block another.
  Each connection also has a metadata session for introspection.
- Rows are sent in batches of about 500 and capped at 1000 per result set (`truncated: true` beyond that).
- Cancel goes around the session (driver cancel handle), so it works while a query is running.
- A connection over SSH has one tunnel, shared by its metadata session, its tabs and the cancel
  connection. Disconnect closes it. Test Connection opens its own tunnel and drops it after.
- Table pages can be filtered and sorted. `BrowseQuery` holds column filters (`AND`-ed) and sort
  keys; the engine (`browse.rs`) quotes the column names through the dialect and sends every value
  as a parameter, so the page runs as a prepared statement. `LIKE` wildcards in a value match
  literally. Values are compared as text, which the database converts to the column type.
- The tab tracks whether its session has a transaction open: `BeginTransaction` / `EndTransaction`,
  and typed SQL (`BEGIN`, `START TRANSACTION`, `COMMIT`, `ROLLBACK`, statements that commit
  implicitly). Closing the tab or disconnecting ends the session, which rolls the transaction back;
  the UI asks first.
- `Explain` accepts one statement that is not already `EXPLAIN` / `ANALYZE`, so it can never turn
  into `EXPLAIN ANALYZE` (which runs the statement). Safe mode treats `EXPLAIN ANALYZE <stmt>` like
  `<stmt>`.
- Grid edits run on the tab's session in one transaction (a savepoint when the user has one open, so
  their changes stay uncommitted). They are parameterized (`?` per value, `IS NULL` for a NULL key
  value), and every UPDATE/DELETE must hit exactly one row or the batch rolls back. The UI picks
  the key (`editTarget` in `store/app.ts`): the primary key, else a unique key on NOT NULL columns,
  else every column whose values compare exactly (not float, JSON or geometry). A page of a table
  and a query result whose columns all come from one table (`ColumnMeta.source_*`) are editable;
  views and read-only connections are not. A saved query result is updated in place, never run
  again, since the query may be a script with writes.
- Schema changes are a `SchemaChange` (create or drop a database, create, rename, empty or drop a
  table, add, change or drop columns and indexes). The driver's `Dialect::schema_change` writes the
  DDL; the engine checks each statement is exactly one statement (a typed type or default cannot
  add another) and always answers `SchemaReview` first, whatever the environment. The client
  resends with `confirmed: true`; the engine builds the statements again from the change and runs
  them on the metadata session, never a tab's session, so a user's open transaction is not
  committed by DDL. Each statement goes into the history. Column types and defaults are SQL as
  typed; `ColumnInfo.default` is already SQL (`'text'`, `CURRENT_TIMESTAMP`), so editing a column
  keeps its default and `ON UPDATE`.

## Safe mode

| Environment | Behaviour |
|---|---|
| Local | Runs everything. |
| Staging | Asks before `DROP`, `TRUNCATE`, and `UPDATE`/`DELETE` without `WHERE`. |
| Production | Asks before every write, including grid edits; can be set read-only. The UI marks the connection in red. |

Classification comes from `quarry-sql`; the decision is made by the engine. Schema changes made
from the UI are always reviewed as SQL first; the review warns on production and before anything
that drops data.

## Storage

| Data | Location |
|---|---|
| Connections | `connections.toml` in the OS config dir (`~/.config/quarry`, `~/Library/Application Support/quarry`). Never contains secrets. Has a `version` field for migrations. |
| Favorites | `favorites.toml` in the same config dir. Version 2 adds connection favorites; version 1 files load as they are. |
| Open tabs | `workspace.json` in the OS data dir (`~/.local/share/quarry`, `~/Library/Application Support/quarry`): title, connection, database, SQL, pinned flag, and page with its filters and sort per tab. Results are not saved. An unreadable file is moved to `workspace.json.broken`. |
| Query history | `history.jsonl` in the data dir: SQL, connection, database, time, duration, first error line. Newest 5,000 kept. Never passwords. |
| Secrets | Linux: Secret Service / Secret portal via `oo7`. macOS: Keychain via `security-framework`. Account `<connection uuid>` for the database password, `<connection uuid>:ssh` for the SSH password or key passphrase. |
| SSH host keys | Read from `~/.ssh/known_hosts`, never written. The Flatpak gets `~/.ssh` read-only and the ssh-agent socket. |

## UI

`ui/src` is organized by feature: `features/{sidebar,connections,tabs,editor,query,grid,structure,schema,palette,history,favorites,settings,welcome,window}`,
shared primitives in `components/ui`, global state in `store/app.ts` (zustand; it is also where engine
events are reduced), and the bridge in `lib/engine.ts`. Look and motion follow
[STYLE-GUIDE.md](STYLE-GUIDE.md).

- **View preferences** (theme, sidebar shown or hidden) are per machine and live in the webview's
  `localStorage`, so they apply before the engine answers. Anything another client would need goes
  through the engine instead.
- **Shortcuts** are listed once in `ui/src/lib/shortcuts.ts`; Settings › Keyboard shows that list.
  Settings has one search over every section; shortcuts match by name or by their keys.
  The webview's own reload is turned off: `F5` / `Ctrl/⌘+R` refresh the active tab instead.

- **Editor:** CodeMirror 6 with SQL highlighting and schema-aware completion; `Ctrl/⌘+Enter` runs the
  statement under the cursor (range computed in Rust by `statement_at`).
- **Grid:** row virtualization (`@tanstack/react-virtual`), sticky header and row-number column,
  resizable columns. A block of cells is selected by dragging, Shift+click, Shift+arrows, the row
  numbers or `Ctrl/⌘+A`, and copies as TSV, CSV or JSON (`lib/copy.ts`) or as INSERT (the engine
  writes it in the connection's dialect). `Shift+Enter` opens the full value: long text, JSON
  (formatted on request) or bytes as hex or text. On an editable result: double-click or Enter to
  edit, Set NULL, add (table pages) and delete rows, then Save (`Ctrl/⌘+S`) or Discard.
- **Table tabs:** a Data | Structure switch (columns, indexes, foreign keys, DDL) and a pager of
  1,000 rows without a total count. Structure adds, changes and drops columns and indexes when
  `Capabilities.schema_edit` is set and the connection is writable.
- **Environment:** the toolbar shows a STAGING / PROD badge and the query view a line of that
  color along its top. Tabs stay neutral, so color never reads as a state of the tab.
- **Toolbar and results bar:** frequent SQL actions (run, stop, format, history, filter, favorite)
  sit above the editor, and the ⋯ menu holds the occasional ones (explain, begin transaction,
  export, import). An edit bar right above an editable grid groups Add row, Delete rows and the
  pending changes with Discard / Save; the bar below the grid holds Data / Structure, result sets,
  status and the pager. Export and import are also in the right-click menus of tables and
  of the grid, and in the command palette. Their logic is not built yet (`transfer` in
  `store/app.ts` says so). The star pins a table tab's table, or
  saves a query tab's SQL, and is filled once it is a favorite.
- **Quick open:** `Ctrl/⌘+P` finds loaded tables, views, routines and databases, connections and
  favorites; `>` or `Ctrl/⌘+Shift+P` lists commands.
- **Sidebar:** Connections and Favorites (pinned connections and tables, saved queries), with one
  filter box: by name for favorites; for the tree, connections, databases and the tables of loaded
  databases. Right-click menus pin connections, create databases and tables, refresh a database,
  and rename, empty or drop tables.
- **History:** a dialog from the editor toolbar (`Ctrl+H`, `⌘Y` on macOS), not a sidebar pane: it
  is used while writing SQL. It searches the SQL typed in query tabs (table pages are not
  recorded), shows the tab's connection by default, inserts at the cursor or opens a new tab.

## Packaging

| Platform | Output | How |
|---|---|---|
| Linux | Flatpak (GNOME 51 runtime, WebKitGTK 4.1) | `just flatpak` → `dist/quarry-<version>.flatpak` |
| Linux | AppImage | `just appimage` (Docker, Ubuntu 22.04) → `dist/Quarry_<version>_amd64.AppImage` |
| Linux | `.deb` | `just deb` (Docker, Ubuntu 22.04) → `dist/Quarry_<version>_amd64.deb` |
| macOS | universal `.dmg` | `just dmg` on a Mac → `dist/Quarry_<version>_universal.dmg` |

Every package lands flat in `dist/`, one file per command, nothing else. Each build prints the full path.

- **Linux base: Ubuntu 22.04.** Tauri's guide says to build on the oldest distro you support that still
  ships WebKitGTK 4.1; the binary needs the glibc it was built with.
- **Caching.** `docker/linux.Dockerfile` is multi-stage. Apt packages, Rust, Node and npm packages are
  cached layers; Cargo's registry and target dir are BuildKit cache mounts. Caches stay inside Docker,
  not in the repo (`docker builder prune` to clear). The final `scratch` stage holds only the bundle,
  and `--output type=local,dest=dist` writes just that file.
- **`.dmg` only on macOS.** Tauri cannot cross-build macOS bundles. Release builds come from the
  macOS runner in `release.yml`.
- **Size.** The release profile follows Tauri's size guide (<https://v2.tauri.app/concept/size/>):
  `lto = true`, `codegen-units = 1`, `opt-level = "s"`, `strip = true`. `panic = "abort"` is left out
  so a panic in one engine task does not kill the app.
- **Release.** Push a `v*` tag: `release.yml` runs CI, builds every package on GitHub runners and
  publishes a public GitHub Release with the AppImage, `.deb`, Flatpak and `.dmg`.

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
