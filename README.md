<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" alt="Quarry logo">
</p>

<h1 align="center">Quarry</h1>

<p align="center">A fast, open-source database client for Linux and macOS.</p>

> **Status:** early development. MySQL, MariaDB, PostgreSQL and Microsoft SQL Server work today. What changed is in
> [CHANGELOG.md](CHANGELOG.md).

- **Light:** Tauri 2 with the system webview and a Rust core. No bundled Chromium, no JVM.
- **Safe on production:** production connections are marked in red, and every write asks first.
- **Keyboard-first:** `Ctrl/⌘+Enter` runs the statement under the cursor.
- **Open source:** AGPL-3.0-or-later.

## Features

- **Databases:** MySQL, MariaDB, PostgreSQL and Microsoft SQL Server, over SSL or an SSH tunnel.
  Each driver is downloaded the first time you need it and checked before it runs. Passwords stay
  in the system keyring.
- **SQL editor:** completion that reads the statement you are in (tables, columns, aliases, the
  keywords that fit), syntax errors underlined before you run, Format SQL, and explain plans. A
  script runs one statement at a time and shows which one is running.
- **Data:** browse a table page by page with filters and sort, count its rows, open the full value
  of a cell, and copy a block of cells as TSV, CSV, JSON or INSERT statements.
- **Editing:** change cells, set NULL, add and delete rows, then save them in one transaction.
  Query results are editable when they come from one table.
- **Schema:** a sidebar tree of tables, views, routines, triggers and events; a Structure view of
  columns, indexes, foreign keys and DDL; add, change and drop columns, indexes, tables and
  databases. Every change shows its SQL before it runs.
- **Export:** a query result to SQL (INSERT, UPDATE, WHERE clause), Excel or CSV; a whole table
  of any size to CSV, TSV, JSON, NDJSON, SQL INSERT or Excel, with progress and cancel.
- **Import:** CSV and TSV into a table, with column mapping and a preview. Rows go in one
  transaction, so a refused row keeps nothing, and the import names that row. On a duplicate key
  it stops, skips the row or overwrites it.
- **Safe mode:** production asks before any write, staging before `DROP`, `TRUNCATE` and
  `UPDATE` / `DELETE` without `WHERE`; read-only connections refuse writes.
- **Workspace:** tabs and split editor groups come back on the next launch. The output panel lists
  every statement with its timing. Quick open (`Ctrl/⌘+K`), the command palette, query history
  and favorites are a key away. Light and dark themes.
- **Updates:** the macOS app and the AppImage download a new release and restart into it. Other
  packages show that one is out and link to it.

## Install

Download from [Releases](https://github.com/MinduLabs/quarry/releases):

- **Linux:** `quarry-<version>.flatpak`, then `flatpak install --user quarry-*.flatpak`
  (or the AppImage / `.deb`).
- **macOS:** `Quarry_<version>_universal.dmg`. Builds are not notarized yet: the first time,
  right-click the app → **Open**, or run `xattr -cr /Applications/Quarry.app`.

## How the project is built

A Tauri 2 shell around a React 19 + TypeScript UI. Everything that touches a database, the keyring
or the disk lives in a headless Rust core; the UI talks to it only through `Command` and `Event`
messages whose TypeScript types are generated from Rust. Details and the reasons behind them:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

| Path | What it holds |
| --- | --- |
| `crates/model` | Plain shared types, no IO. Exports TypeScript types. |
| `crates/driver-api` | `Driver` / `Session` / `Dialect` traits every driver implements. |
| `crates/drivers/*` | One driver binary per database: `mysql` (also MariaDB), `postgres`, `sqlserver`. |
| `crates/driver-proto` | The wire between the engine and a driver binary. |
| `crates/sql` | Statement splitting, statement at cursor, classification, completion context. |
| `crates/store` | Config and data files, secrets in the OS keyring. |
| `crates/tunnel` | SSH port forwarding. |
| `crates/export` | File writers for exports: CSV, TSV, JSON, NDJSON, SQL, Excel. |
| `crates/import` | Reads CSV and TSV one record at a time for imports. |
| `crates/engine` | Owns the runtime and all state; turns `Command`s into `Event`s. |
| `src-tauri/` | Thin bridge between the webview and the engine. |
| `ui/src/` | React UI: `features/` by feature, `components/ui/` primitives, `store/` state, `bindings/` generated. |
| `docker/`, `packaging/` | Linux package builds, Flatpak manifest, desktop entry, metainfo. |
| `tests/` | Test databases (MySQL, PostgreSQL, SQL Server) and SSH server (Docker Compose), with fixtures. |
| `docs/` | [Architecture](docs/ARCHITECTURE.md), [UI style guide](docs/STYLE-GUIDE.md). |

## Development setup

You need, once per machine:

- Rust 1.92 or newer through [rustup](https://rustup.rs)
- Node 20+ and pnpm 9 (`corepack enable` gives you pnpm)
- [just](https://github.com/casey/just)
- Docker, for the test database and the Linux packages
- The system webview's libraries (below)

`pnpm install` only installs the UI. Linux uses WebKitGTK; install its development packages and
`pkg-config` (lists from [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/)).

Debian, Ubuntu (22.04 or newer):

```bash
sudo apt install pkg-config build-essential curl wget file libwebkit2gtk-4.1-dev libssl-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev
```

Fedora:

```bash
sudo dnf install pkgconf-pkg-config webkit2gtk4.1-devel openssl-devel curl wget file libxdo-devel libappindicator-gtk3-devel librsvg2-devel && sudo dnf group install c-development
```

Arch:

```bash
sudo pacman -S --needed base-devel pkgconf webkit2gtk-4.1 openssl curl wget file xdotool libappindicator-gtk3 librsvg
```

macOS: Xcode Command Line Tools (`xcode-select --install`).

`failed to run custom build command for glib-sys` with `The pkg-config command could not be found`
means the webview libraries are missing. Without them you can still run `pnpm check`,
`just check-docker` and the Docker package builds, but not `just dev`.

## Commands

`just --list` shows every recipe with its description.

```bash
pnpm install          # first time
just dev              # run the app with hot reload
just check            # Biome + tsc + cargo fmt + clippy -D warnings + cargo test
just check-docker     # same, Rust part in Docker (hosts without WebKitGTK)
just bindings         # regenerate ui/src/bindings after changing Command, Event or model types
just test-db          # start the test databases, then the driver and engine integration tests
just bench            # release benchmarks
pnpm format           # format the UI with Biome
```

Drivers live in one place for `just dev` and the installed app: `drivers` in the data dir
(`~/.local/share/quarry/drivers`, `~/Library/Application Support/quarry/drivers`).
`QUARRY_DRIVER_DIR` points elsewhere. `just dev` loads only what is there and downloads the rest
from a connection's settings, as a user does: `scripts/drivers.py` serves the drivers in
`target/drivers` on `127.0.0.1:47821` (`QUARRY_DEV_DRIVER_PORT` changes it). `just drivers`
builds them there; `just dev`, `just test-db` and `just bench` do it first. Each run replaces the installed
drivers with the ones just built. Delete a file there to download that driver again.

Integration tests skip themselves unless `QUARRY_TEST_MYSQL`, `QUARRY_TEST_POSTGRES` or
`QUARRY_TEST_SQLSERVER` is set, which `just test-db` does.

### Test database

`tests/docker-compose.yml` runs the test databases (test values only):

| Database | Address | User / password | Fixtures |
|---|---|---|---|
| MySQL | `127.0.0.1:13306` | `quarry` / `quarry` | `tests/fixtures/mysql/` |
| PostgreSQL | `127.0.0.1:15432`, database `shop` | `quarry` / `quarry` | `tests/fixtures/postgres/` |
| SQL Server | `127.0.0.1:11433`, database `shop` | `sa` / `Quarry-test-1` | `tests/fixtures/sqlserver/` |

Each has a `playground.samples` table: one column of most of its types, edge values in the first
row, explained in the fixture. A feature that needs a kind of data the table lacks adds it there,
with a line in the test that reads it (`playground_samples_cover_every_type` for MySQL). Fixtures
load only into a fresh container: `docker compose -f tests/docker-compose.yml down -v` after
changing them, and the next `just test-db` loads them.

### Logs

- `QUARRY_LOG=debug just dev`: runtime log (a `tracing` EnvFilter, default `info`).
- `QUARRY_PERF=1 just dev`: writes `perf.jsonl` to the data dir (`~/.local/share/quarry`,
  `~/Library/Application Support/quarry`), or to the path you give (`QUARRY_PERF=/tmp/perf.jsonl`).
  Engine timings per command and query phase, and UI render times in development builds. Never SQL
  text or values. Any `*perf*.jsonl` in the repo is git-ignored.

### Packages

Each command writes one file flat in `dist/` and prints its path.

```bash
just appimage         # dist/Quarry_<version>_amd64.AppImage (Docker, Ubuntu 22.04)
just deb              # dist/Quarry_<version>_amd64.deb (Docker, Ubuntu 22.04)
just flatpak          # dist/quarry-<version>.flatpak (Docker)
just dmg              # dist/Quarry_<version>_universal.dmg (macOS only)
just clean            # delete target/, dist/, ui/dist and the Flatpak cache volumes
```

Linux build caches stay inside Docker; `docker builder prune` clears them.

### Release

Bump `version` in `Cargo.toml` (the only place it is written), move the `[Unreleased]` notes of
`CHANGELOG.md` under `## [<version>] - <date>` (that section becomes the release notes), commit,
then push the tag `v<version>`. The release workflow checks
the tag, runs CI, builds every package and publishes a GitHub Release.

The macOS app and the AppImage update themselves from the release's `latest.json`. Its bundles are
signed with a key made once with `pnpm tauri signer generate -w ~/.tauri/quarry.key`: the public key
goes in `plugins.updater.pubkey` of `src-tauri/tauri.conf.json`, the private key and its password in
the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. A tag
is refused without them. Keep the private key safe: without it, installed copies never update again.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before your first pull request.

## License

Quarry is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE).
