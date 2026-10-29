<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" alt="Quarry logo">
</p>

<h1 align="center">Quarry</h1>

<p align="center">A fast, open-source database client for Linux and macOS.</p>

> **Status:** early development. MySQL / MariaDB work today. What changed is in
> [CHANGELOG.md](CHANGELOG.md).

- **Light:** Tauri 2 with the system webview and a Rust core. No bundled Chromium, no JVM.
- **Safe on production:** production connections are marked in red, and every write asks first.
- **Keyboard-first:** `Ctrl/⌘+Enter` runs the statement under the cursor.
- **Open source:** AGPL-3.0-or-later.

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
| `crates/drivers/*` | One crate per database (today `mysql`). |
| `crates/sql` | Statement splitting, statement at cursor, classification, completion context. |
| `crates/store` | Config and data files, secrets in the OS keyring. |
| `crates/tunnel` | SSH port forwarding. |
| `crates/engine` | Owns the runtime and all state; turns `Command`s into `Event`s. |
| `src-tauri/` | Thin bridge between the webview and the engine. |
| `ui/src/` | React UI: `features/` by feature, `components/ui/` primitives, `store/` state, `bindings/` generated. |
| `docker/`, `packaging/` | Linux package builds, Flatpak manifest, desktop entry, metainfo. |
| `tests/` | Test MySQL and SSH server (Docker Compose) and their fixtures. |
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
just db-up            # test MySQL on 127.0.0.1:13306 and SSH server on 127.0.0.1:12222
just test-db          # driver and engine integration tests against them
just db-down          # stop them and drop their data
just bench            # release benchmarks (query latency needs just db-up)
pnpm format           # format the UI with Biome
```

Integration tests skip themselves unless `QUARRY_TEST_MYSQL` is set, which `just test-db` does.

### Test database

The test MySQL user is `quarry` / `quarry` (test values only); so is the test PostgreSQL's, on
`127.0.0.1:15432`, database `shop`, with fixtures in `tests/fixtures/postgres/`. It loads the fixtures in
`tests/fixtures/mysql/`, including `playground.samples`: one column of every MySQL type, edge values
in the first rows, explained in
[02-playground.sql](tests/fixtures/mysql/02-playground.sql). A feature that needs a kind of data the
table lacks adds it there, with a line in the test `playground_samples_cover_every_type`. Fixtures
load only into a fresh container: `just db-down && just db-up` after changing them.

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

Bump `version` in `Cargo.toml` and `package.json`, commit, then push the tag `v<version>`. The release workflow checks
the tag, runs CI, builds every package and publishes a GitHub Release.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before your first pull request.

## License

Quarry is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE).
