<p align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.mindulabs.Quarry.svg" width="128" alt="Quarry logo">
</p>

<h1 align="center">Quarry</h1>

<p align="center">A fast, native, open-source database client for Linux.</p>

> **Status:** early development — nothing usable yet. Follow the [roadmap](docs/product/ROADMAP.md).

## Why Quarry

- **Native** — Rust + GTK4 + libadwaita. No Electron, no JVM.
- **Multi-database** — PostgreSQL, MySQL/MariaDB and SQLite first; more to follow.
- **Safe on production** — every write is previewed; production connections are clearly marked.
- **Open source** — AGPL-3.0-or-later.

## Install

Download the latest `.flatpak` from [Releases](https://github.com/MinduLabs/quarry/releases), then:

```bash
flatpak install --user quarry-*.flatpak
```

Flathub listing will come with v0.1.

## Build from source

See [docs/dev/SETUP.md](docs/dev/SETUP.md) and [docs/dev/BUILDING.md](docs/dev/BUILDING.md).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). Design and architecture docs live in [docs/](docs/README.md).

## License

Quarry is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE).
