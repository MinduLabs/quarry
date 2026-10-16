# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- MySQL / MariaDB driver (`mysql_async` + rustls): connect, SSL modes, multi-statement scripts, cancel (`KILL QUERY`), schema introspection.
- Saved connections (`~/.config/quarry/connections.toml`), passwords in the system keyring.
- Connection dialog with Test Connection; sidebar tree: connection → database → tables / views / routines.
- Query tabs: SQL editor (GtkSourceView, Ctrl+Enter runs the statement at the cursor, Ctrl+Shift+Enter runs all), read-only result grid (first 1000 rows), multiple result sets, stop button.
- Click a table to browse it.
- Safe mode: production asks before any write, staging asks before DROP / TRUNCATE / UPDATE·DELETE without WHERE; read-only connections.
- Project skeleton: Cargo workspace, GTK4/libadwaita window, engine ↔ UI channel.
- Flatpak packaging and CI.
