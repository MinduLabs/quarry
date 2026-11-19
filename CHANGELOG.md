# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Sign in to Amazon RDS and Aurora (MySQL, MariaDB, PostgreSQL) with AWS IAM instead of a password: choose AWS IAM in the connection's settings. Quarry signs a token with your AWS profile (SSO and roles included) for every session.
- Compare schemas: pick two databases of the same kind and see which tables, columns, indexes, primary and foreign keys differ, with the script that makes the target match the source. Apply it after reviewing its SQL. From the command palette or a database's menu in the sidebar. Scripts are written for MySQL and MariaDB.

### Fixed
- A saved connection whose driver is not downloaded is marked in the sidebar, and connecting to it offers to download the driver, then connects.
- PostgreSQL: a new query no longer runs in a schema named after the connection's database, where it found no table. It uses the server's default schema until you pick one in the sidebar.
- PostgreSQL and SQL Server: the sidebar starts at the server's current schema (`public`, `dbo`) instead of showing none.
### Security
- Drivers are signed: Quarry runs only drivers listed in its own release's signed driver list, checked at download and at every start. After an update, download your drivers again when asked.

### Known issues
- Quarry 0.1.0 cannot update itself to this version: its updates were signed with a different key than the one it checks. Download this version once from the release page; later versions update themselves again.

## [0.1.0] - 2026-10-31

The first public release: a fast, open-source database client for MySQL, MariaDB, PostgreSQL and
Microsoft SQL Server on Linux and macOS. Install: [README › Install](https://github.com/MinduLabs/quarry#install).

### Download

| Your system | File |
| --- | --- |
| Linux, any distro | `Quarry_0.1.0_amd64.AppImage` (updates itself) |
| Debian, Ubuntu | `Quarry_0.1.0_amd64.deb` |
| Flatpak | `quarry-0.1.0.flatpak` |
| macOS | `Quarry_0.1.0_universal.dmg` |

Do not download the rest. `*.app.tar.gz`, `*.sig` and `latest.json` are for the in-app updater;
`quarry-driver-*` and `drivers.json` are the database drivers Quarry fetches by itself.

### Features
- **Databases:** MySQL, MariaDB, PostgreSQL and SQL Server. Drivers download on first use and are checked before they run.
- **Query editor:** SQL completion that knows your schema, syntax errors underlined before you run, format, explain plan, query history, favorites, split editor.
- **Results:** fast grid for big tables, filter and sort, find, copy as TSV, CSV, JSON or INSERT, export every row.
- **Edit data:** change cells in the grid and save in one transaction. Import CSV and TSV into a table.
- **Edit schema:** columns, indexes, tables and databases, with the SQL shown before it runs.
- **Safe mode:** production and staging ask before risky writes. Read-only connections.
- **Everything else:** quick open (Ctrl+K), tabs that come back after a restart, light and dark themes, passwords in the system keyring.
- **Updates:** the macOS app and the AppImage update themselves. The `.deb` and the Flatpak link to new releases.

### Notes
- The macOS build is not notarized yet. Right-click the app and choose **Open**, or run `xattr -cr /Applications/Quarry.app`.

[Unreleased]: https://github.com/MinduLabs/quarry/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/MinduLabs/quarry/releases/tag/v0.1.0
