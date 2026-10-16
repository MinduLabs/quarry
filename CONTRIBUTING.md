# Contributing to Quarry

Thanks for your interest! Quarry is early-stage, so please open an issue or discussion before large changes.

## Contributor License Agreement

All contributors must sign the [CLA](CLA.md). The CLA Assistant bot will ask you on your first pull request.
It lets the project relicense contributions in the future (for example, commercial licenses), while your contribution stays available under AGPL-3.0-or-later.

## Development setup

See [docs/dev/SETUP.md](docs/dev/SETUP.md). Common commands:

```bash
just run      # run the app (needs GTK >= 4.14 and libadwaita >= 1.5 on the host)
just check    # fmt + clippy + test inside the Ubuntu 24.04 dev container
just flatpak  # build dist/quarry-<version>.flatpak
```

## Pull requests

- One logical change per PR; keep it small.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`.
- CI must be green: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, `cargo deny`.
- UI changes follow [docs/design/STYLE-GUIDE.md](docs/design/STYLE-GUIDE.md) and include a screenshot (light + dark).
- Big design decisions get an ADR in `docs/architecture/adr/`.

## Architecture rules (enforced in review)

- `crates/app` never depends on driver crates directly — it talks to `crates/engine` only.
- No `unwrap()`/`expect()` outside `crates/app` and tests.
- The UI thread never blocks on IO.
- All user-facing strings go through `gettext`.

See [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md).
