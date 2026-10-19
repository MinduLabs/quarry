# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Desktop app on Tauri 2 (Linux and macOS) with a React + TypeScript UI; the Rust core is shared.
- MySQL / MariaDB driver (`mysql_async` + rustls): connect, SSL modes, multi-statement scripts, cancel (`KILL QUERY`), schema introspection.
- Saved connections (`connections.toml` in the OS config dir), passwords in the system keyring (Secret Service on Linux, Keychain on macOS).
- Connection dialog with Test Connection; sidebar tree: connection → database → tables / views / routines.
- Query tabs: CodeMirror SQL editor with completion (Ctrl/⌘+Enter runs the statement at the cursor), virtualized result grid (first 1000 rows) with resizable columns and cell copy, multiple result sets, stop button.
- Exact `BIGINT` / `BIGINT UNSIGNED` / `DECIMAL` values in the UI.
- Click a table to browse it.
- Safe mode: production asks before any write, staging asks before DROP / TRUNCATE / UPDATE·DELETE without WHERE; read-only connections.
- Light and dark themes.
- Open tabs and their SQL are restored on the next launch (results are not saved).
- Table tabs page through rows 1,000 at a time and have a Structure view: columns, indexes, foreign keys and DDL.
- Edit table data in the grid: change cells, set NULL, add and delete rows, then save in one transaction. Production asks first.
- Query history (`Ctrl+H`, `⌘Y` on macOS, or the toolbar button): the SQL you ran, searchable, newest 5,000 kept. It shows the tab's connection by default and inserts a query at the cursor or opens it in a new tab.
- Favorites: pin tables and save named queries.
- Format SQL (`Shift+Ctrl/⌘+F` or the toolbar button): formats the selection, or the whole tab when nothing is selected, in one undo step.
- Completion suggests the columns of every table in the database, not only table names.
- Table tabs: filter rows (=, ≠, <, >, contains, starts with, in list, NULL checks) and sort by clicking a column header. The database does the work, with values sent as parameters; filters and sort are saved with the tab.
- Table tabs show the estimated row count and can count the matching rows exactly.
- Explain plan: shows the plan of the statement under the cursor as a tree or as a table, without running it.
- Settings (gear in the sidebar, `Ctrl/⌘+,`): theme (System, Light, Dark) and the list of keyboard shortcuts, with a search box over every section that finds shortcuts by name or by keys ("ctrl shift f", "⌘Y").
- Shortcuts are shown the platform's way: `⌘ ⇧ T` on macOS, `Ctrl Shift T` on Linux, on macOS-style key caps.
- `F5` / `Ctrl/⌘+R` refresh the active tab: the same table page with its filters, the plan, or the last query (safe mode still asks).
- Select a block of cells in the grid (drag, Shift+click, Shift+arrows, row numbers, `Ctrl/⌘+A`) and copy it as TSV, with headers, CSV, JSON or an INSERT statement.
- Open the full value of a cell (`Shift+Enter` or double-click on long values): long text, JSON with Format, bytes as hex or text. Editable when the result is.
- Staging and production connections show a line of their color along the top of the query view, next to the toolbar badge.
- The star on the toolbar pins a table tab's table to Favorites (or saves a query tab's SQL) and is filled once it is a favorite.
- Editing has its own bar right above the grid: Add row, Delete rows (the selected ones) and the pending changes with Discard / Save. The bar below the grid holds Data / Structure, result sets, status and pages; the toolbar above keeps the SQL actions, with Explain plan and Begin transaction in its ⋯ menu. The grid's right-click menu adds Insert row.
- Quick open (`Ctrl/⌘+P`) for tables, views, databases, connections and favorites; commands with `Ctrl/⌘+Shift+P` or `>`.
- Edit query results when all their columns come from one table, and tables without a primary key (rows are matched by their values; a change that would hit more than one row is not saved).
- Change the schema from Structure and the sidebar: add, edit and drop columns and indexes, create, rename, empty and drop tables, create and drop databases. Every change shows its SQL before it runs.
- Copy a table's DDL from Structure; copy names and refresh a database from the sidebar.
- Pin connections in Favorites (right-click a connection); opening one connects and shows it in the tree.
- The sidebar filter now works on the Connections pane: it matches connections, databases and the tables of open databases.
- Right-click a tab: Reopen closed tab (`Shift+Ctrl/⌘+T`, the last 20 closed tabs with their SQL and page), Close, Close other tabs.
- Show / hide button on the password fields of the connection dialog (what you typed; saved passwords stay in the keyring).
- Transactions per tab: begin, commit and roll back from the toolbar. Typed `BEGIN` / `COMMIT` are tracked too, grid edits inside a transaction stay uncommitted, and closing the tab or quitting asks first.
- SSH tunnels: ssh-agent, key file or password. The server must already be in `~/.ssh/known_hosts`. RSA key files work through ssh-agent only.
- Packaging: Flatpak, AppImage and `.deb` on Linux; universal `.dmg` on macOS. Linux packages are built on Ubuntu 22.04 so they also run on older distros; release builds are size-optimized.

### Fixed
- A tab with a running query can be closed: closing stops the query first.
- Right-click no longer opens the webview's own menu (Reload, Back), which restarted the whole app; `F5` / `Ctrl+R` no longer reload it either.
- The Name field of the connection dialog no longer shows the host as its placeholder, which looked like a value already typed in.
- Safe mode now asks before `EXPLAIN ANALYZE` of a write, which runs the statement. It was treated as a read.
- `F5` in a table's Structure view reloads the structure instead of switching to Data.
- A database refresh that fails keeps the tables already shown in the sidebar.
