<p align="center">
  <img src="src-tauri/icons/icon.svg" width="128" alt="Quarry logo">
</p>

<h1 align="center">Quarry</h1>

<p align="center">A fast, open-source database client for Linux and macOS.</p>

> **Status:** early development. MySQL / MariaDB work today; PostgreSQL and SQLite are next.

## Why Quarry

- **Light** — Tauri 2 with the system webview and a Rust core. No bundled Chromium, no JVM.
- **Safe on production** — production connections are marked in red, and every write asks first.
- **Keyboard-first** — `Ctrl/⌘+Enter` runs the statement under the cursor.
- **Open source** — AGPL-3.0-or-later.

What works now: saved connections with passwords in the system keyring, schema tree, SQL editor with
completion, multiple query tabs, a virtualized result grid, cancel, and safe mode.

## Install

Download from [Releases](https://github.com/MinduLabs/quarry/releases):

- **Linux:** `quarry-<version>.flatpak`, then `flatpak install --user quarry-*.flatpak`
  (or the AppImage / `.deb`).
- **macOS:** `Quarry_<version>_universal.dmg`. Development builds are not notarized yet: the first time,
  right-click the app → **Open**, or run `xattr -cr /Applications/Quarry.app`.

## Build from source

Besides Rust, Node with pnpm and [just](https://github.com/casey/just), the app needs the system
webview's libraries: WebKitGTK and `pkg-config` on Linux, Xcode Command Line Tools on macOS.
`pnpm install` does not install them. Install commands for each platform are in
[CONTRIBUTING.md › Development setup](CONTRIBUTING.md#development-setup). Then:

```bash
pnpm install
just dev        # run with hot reload
just check      # lint + tests
```

No WebKitGTK on your Linux host? `just check-docker` runs the Rust part in Docker. Packaging:
`just appimage`, `just deb`, `just flatpak` (Docker) or `just dmg` (macOS only).
Each writes one file to `dist/` and prints its path.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and
[docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md).

## License

Quarry is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE).
