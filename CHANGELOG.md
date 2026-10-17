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
- Packaging: Flatpak, AppImage and `.deb` on Linux; universal `.dmg` on macOS. Linux packages are built on Ubuntu 22.04 so they also run on older distros; release builds are size-optimized.
