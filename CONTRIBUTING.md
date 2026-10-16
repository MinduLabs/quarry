# Contributing to Quarry

Thanks for your interest! Quarry is early-stage, so please open an issue or discussion before large changes.

## Contributor License Agreement

All contributors must sign the [CLA](CLA.md). The CLA Assistant bot will ask you on your first pull request.
It lets the project relicense contributions in the future (for example, commercial licenses), while your
contribution stays available under AGPL-3.0-or-later.

## Development setup

Install Rust (stable), Node 20+ with pnpm, [just](https://github.com/casey/just) and Tauri's system
dependencies (`libwebkit2gtk-4.1-dev` on Linux, Xcode Command Line Tools on macOS), then:

```bash
pnpm install
just dev            # run the app with hot reload
just check          # Biome + tsc + cargo fmt/clippy/test (just check-docker without webkit2gtk)
just db-up          # start the test MySQL on 127.0.0.1:13306
just test-db        # driver and engine integration tests
just bindings       # regenerate ui/src/bindings after changing Command/Event or model types
```

## Pull requests

- One logical change per PR; keep it small.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`,
  `refactor:`, `test:`, `chore:`, `ci:`.
- CI must be green: Biome, TypeScript, `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, `cargo deny`.
- UI changes follow [docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md) and include screenshots (light + dark).

## Architecture rules (enforced in review)

- The UI talks to the core only through `Command` / `Event` (`crates/engine`). `src-tauri` stays a thin
  bridge with no business logic.
- Nothing above `crates/engine` depends on a driver crate.
- `ui/src/bindings/` is generated; never edit it.
- No `unwrap()` / `expect()` outside tests. Never log passwords.
- UI: design tokens only, primitives from `ui/src/components/ui/`, animate only `opacity` and `transform`.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
