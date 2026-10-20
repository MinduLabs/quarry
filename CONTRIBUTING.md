# Contributing to Quarry

Thanks for your interest! Quarry is early-stage, so please open an issue or discussion before large changes.

## Contributor License Agreement

All contributors must sign the [CLA](CLA.md). The CLA Assistant bot will ask you on your first pull request.
It lets the project relicense contributions in the future (for example, commercial licenses), while your
contribution stays available under AGPL-3.0-or-later.

## Development setup

`pnpm install` only installs the UI. The app also needs a Rust toolchain and, because Tauri uses the
system's own webview, the native libraries of that webview. Set these up once per machine.

### 1. Tools (every platform)

- Rust 1.92 or newer through [rustup](https://rustup.rs)
- Node 20+ and pnpm 9 (`corepack enable` gives you pnpm)
- [just](https://github.com/casey/just)
- Docker, for the test database (`just db-up`) and for Linux packages

### 2. Webview libraries

Linux uses WebKitGTK, which is built on GTK. Install its development packages and `pkg-config`, which
the Rust build uses to find them. The lists follow
[Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/).

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

macOS: Xcode Command Line Tools (`xcode-select --install`). WKWebView ships with the system.

`failed to run custom build command for glib-sys` with `The pkg-config command could not be found`
means this step is missing. Without these libraries you can still run `pnpm check`,
`just check-docker` (the Rust checks inside Docker) and the Docker package builds, but not `just dev`.

### 3. Run

```bash
pnpm install
just dev            # run the app with hot reload
just check          # Biome + tsc + cargo fmt/clippy/test (just check-docker without webkit2gtk)
just db-up          # start the test MySQL on 127.0.0.1:13306
just test-db        # driver and engine integration tests
just bindings       # regenerate ui/src/bindings after changing Command/Event or model types
just bench          # release benchmarks (history file; query latency needs just db-up)
```

### Measuring performance

`QUARRY_PERF=1 just dev` writes one JSON line per timing to `perf.jsonl` in the data dir (`~/.local/share/quarry`, `~/Library/Application Support/quarry`), or to the path you give (`QUARRY_PERF=/tmp/perf.jsonl`). The log is local only: any `*perf*.jsonl` in the repo is git-ignored. The engine logs every command, each query's phases (`wait_ms`, `prepare_ms`, `first_rows_ms`, `execute_ms`, rows, batches), history and schema loads; the UI adds its render times (`render.tab`, `render.grid`, `render.sidebar`, development builds only). `QUARRY_LOG=quarry::perf=debug` prints the same lines in the terminal. The log never holds SQL text or values.

`just bench` runs the benchmarks in a release build; compare its numbers before and after a change that touches a hot path.

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
