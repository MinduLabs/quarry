# CLAUDE.md

Guide for AI agents (Claude Code, Codex, …). `AGENTS.md` links here. Read it before any change.

Quarry is an open-source database client for Linux and macOS (AGPL-3.0-or-later, early stage, v0.0.x). Tauri 2 shell, React 19 + TypeScript UI, headless Rust core. Today: MySQL / MariaDB only. App ID `io.github.mindulabs.Quarry`.

## Principles

1. **Headless core.** Everything that touches a database, the keyring or the disk lives in `crates/engine` and below. The UI is one client of the engine; a CLI or MCP server would be another.
2. **One protocol.** UI ↔ core talk only through `Command` (requests) and `Event` (results). Rust types generate the TypeScript types, so the sides cannot drift.
3. **Capabilities, not names.** Drivers declare `Capabilities`; the UI reads them. No `if driver == "mysql"` in the UI or the engine.
4. **Bounded results.** A query streams rows in batches and stops at a row limit. Nothing loads a whole table into memory.
5. **Data safety lives in the engine.** Safe-mode confirmation is an engine round trip, so every client gets the same protection. Never move it into the UI.
6. **Secrets stay out.** Passwords live in the OS keyring, never in `connections.toml`, logs, errors or events.
7. **Fix the cause, not the symptom.** Reproduce, find the root cause, then fix. No workaround left behind.
8. **Tests define behavior.** A testable change ships with a test. When a test fails, fix the code, not the test. A test you change on purpose is named in the PR.
9. **Quiet, dense, fast UI.** Data first, one accent, tokens only, motion only on `opacity` and `transform` ([docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md)).

## Repository map

- `crates/model`: plain types, no IO (`Value`, `ColumnMeta`, `SchemaObject`, `ConnectionConfig`, `Capabilities`, `Environment`). Exports TS types (ts-rs).
- `crates/driver-api`: `Driver` / `Session` traits, `ExecItem`, `DriverError`, `DriverRegistry`.
- `crates/drivers/mysql`: MySQL / MariaDB via `mysql_async` + rustls. `convert.rs` (values), `diagnose.rs` (connect error messages).
- `crates/sql`: `split`, `statement_at`, `classify` (read / write / DDL / destructive).
- `crates/store`: `connections.toml` and `favorites.toml` (config dir, no secrets), `workspace.json` and `history.jsonl` (data dir), `SecretStore` (keyring: `oo7` on Linux, `security-framework` on macOS).
- `crates/tunnel`: SSH local port forwarding (`russh`): agent, key file or password; host key must be in `known_hosts`.
- `crates/engine`: owns the tokio runtime and all state. `command.rs`, `event.rs`, `lib.rs` (`EngineHandle`), `engine.rs` (`Engine` state), and one module per concern (`connections`, `query`, `session`, `changes`, `metadata`, `favorites`, `history`, `workspace`, `dispatch`). The only crate that knows a concrete driver (registered in `lib.rs`).
- `src-tauri/`: thin bridge, no business logic. Commands `engine_send`, `engine_subscribe`, `statement_at`. `desktop.rs` installs the Linux desktop entry for unpackaged runs.
- `ui/src/`: `features/{sidebar,connections,tabs,editor,query,grid,welcome,window}`, `components/ui/` (primitives), `store/app.ts` (zustand; engine events are reduced here), `lib/engine.ts` (bridge), `styles/tokens.css`. `bindings/` is generated.
- `docker/linux.Dockerfile` (AppImage, `.deb`, `check-docker`), `docker/flatpak.Dockerfile`, `packaging/` (Flatpak manifest, desktop entry, metainfo). `tests/`: MySQL compose file and fixtures.
- Docs: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (layers, flows, storage, packaging), [docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md) (read before touching `ui/`), [CONTRIBUTING.md](CONTRIBUTING.md).

## Architecture in brief

```
ui (React)  →  src-tauri (bridge)  →  engine  →  {driver-api, sql, store, tunnel}  →  model
```

- Dependencies point down only. Nothing above `engine` imports a driver crate. `model` imports nothing internal.
- `Command` and `Event` are `#[serde(tag = "type")]` enums in `crates/engine/src/{command,event}.rs`. Commands are fire and forget; results come back as events keyed by `TabId` (query tabs) or `RequestId` (one-off requests).
- Each query tab owns one database session; each connection also has a metadata session. A long query in one tab never blocks another. Cancel goes through the driver's cancel handle (`KILL QUERY` on a side connection), not the busy session.
- Rows go out in batches of about 500 and are capped at 1000 per result set (`truncated: true` beyond).
- `BIGINT`, `BIGINT UNSIGNED` and `DECIMAL` travel as strings so the UI shows them exactly.
- Safe mode: Local runs anything. Staging confirms `DROP`, `TRUNCATE`, and `UPDATE`/`DELETE` without `WHERE`. Production confirms every write and can be read-only. Flow: `Run{confirmed:false}` → `ConfirmRequired` → user agrees → `Run{confirmed:true}`.
- `connections.toml` has a `version` field. A format change adds a migration; it never silently rewrites old files.

## Commands

```bash
pnpm install          # first time
just dev              # app with hot reload (needs webkit2gtk-4.1 on Linux, Xcode CLT on macOS)
just check            # pnpm check + cargo fmt --check + clippy -D warnings + cargo test
just check-docker     # same, Rust part in Docker (hosts without webkit2gtk-4.1)
just db-up            # test MySQL on 127.0.0.1:13306 (user quarry / password quarry, test values)
just test-db          # driver + engine integration tests (QUARRY_TEST_MYSQL=127.0.0.1:13306)
just db-down
just bindings         # regenerate ui/src/bindings after Command / Event / model changes
just bench            # release benchmarks: history file, query latency on the test MySQL (db-up first)
just appimage         # dist/Quarry_<version>_amd64.AppImage (Docker, Ubuntu 22.04)
just deb              # dist/Quarry_<version>_amd64.deb (Docker, Ubuntu 22.04)
just flatpak          # dist/quarry-<version>.flatpak (Docker)
just dmg              # dist/Quarry_<version>_universal.dmg (macOS only)
just clean            # delete target/, dist/, ui/dist and the Flatpak cache volumes
pnpm check            # Biome + tsc (UI only)
pnpm format           # Biome format (UI)
```

- Runtime log: `QUARRY_LOG=debug just dev` (a `tracing` EnvFilter, default `info`).
- Performance log: `QUARRY_PERF=1 just dev` writes `perf.jsonl` in the data dir (or `QUARRY_PERF=<path>`): engine timings per command and query phase, and UI render times. Durations and counts only, never SQL text or values.
- Integration tests skip themselves unless `QUARRY_TEST_MYSQL` is set. A skipped run proves nothing about MySQL.
- Every package lands flat in `dist/`: one command, one file (`.AppImage`, `.deb`, `.flatpak`, `.dmg`), nothing else. Each build prints the full path.
- Linux package builds keep their caches inside Docker (BuildKit layers and cache mounts), never in the repo. `docker builder prune` clears them.
- `.dmg` is built only on macOS (a Mac or the release CI job). Tauri cannot cross-build it; do not add a Linux workaround.
- Do not run builds unless the user asks; editing build files does not mean running them.
- Toolchain: Rust stable 2024 edition (`rust-version` 1.92), Node 20+ (CI 22), pnpm 9.

## Invariants that apply everywhere

- **UI ↔ core only through `Command` / `Event`.** A new capability = a new `Command` / `Event` variant, then `just bindings`, then commit the regenerated `ui/src/bindings/`. CI fails on a stale diff.
- **Never edit `ui/src/bindings/`.** It is generated by `cargo test -p quarry-model -p quarry-engine` (ts-rs, dir set in `.cargo/config.toml`).
- **No `unwrap()` / `expect()` outside tests** (clippy `deny`). No `unsafe` (`deny`), no `dbg!`, no `todo!` left behind.
- **Never log or print a password.** `Password` and `ResolvedConfig` have a redacting `Debug`; keep it that way. `ConnectionConfig` never holds a secret.
- **Errors reach the user as text that says what happened and what to do** (see `diagnose.rs`). No raw driver dumps.
- **The engine and drivers never block.** No blocking IO on the tokio runtime; the UI never waits on IO.
- **Integers that do not fit a JS number are strings or `#[ts(type = "number")]` on purpose.** Do not widen a `u64` field without checking the TS type.
- **A refresh never clears what it refreshes.** Fetch, then replace; keep good data on failure.
- **New driver = new crate under `crates/drivers/`**, registered only in `crates/engine/src/engine.rs`, with `Capabilities` filled in honestly. Add its integration test and compose service.
- **UI strings and shortcuts:** sentence case, "…" on actions that open a dialog, shortcuts drawn with `Kbd`, never plain text.
- **A new color is a token** in `tokens.css` (light and dark), never a hex in a component.
- **Dependencies:** a new crate or npm package needs a reason in the PR. `cargo deny` blocks bad licenses (third-party GPL / AGPL is out because Quarry keeps the option to relicense) and yanked crates.

## Code style

- Rust: `cargo fmt` (`rustfmt.toml`: width 100), clippy clean with `-D warnings`. Workspace lints in `Cargo.toml` are the source of truth. Library errors use `thiserror`; log with `tracing`, never `println!`.
- Rust tests sit in `#[cfg(test)] mod tests` beside the code; integration tests in `crates/*/tests/`. `unwrap` is fine there.
- TypeScript: strict, `noUncheckedIndexedAccess`. Biome formats and lints (2 spaces, width 100, organized imports). Import with the `@/` alias. No `any`.
- React: function components, state in `store/app.ts`, engine calls (`send`) in store actions, never in components. Primitives from `ui/src/components/ui/`.
- Comments say why, never what. Docs and comments in English. Delete what-comments in code you touch.
- Names: early returns, small functions, self-explanatory names. Booleans start with `is`, `has`, `can`, `should`.

## Performance pitfalls

- The grid and the sidebar tree are virtualized (`@tanstack/react-virtual`). Never render all rows, and never put per-row state in React state that re-renders the whole list.
- Every tab stays mounted, so big components (`QueryView`, `ResultGrid`, `Sidebar`) are memoized with stable props: a keystroke must not re-render other tabs, the grid's cells or the tree.
- WebKitGTK on Linux is slow at layout animation: animate `opacity` and `transform` only. No `transition: all`, no `backdrop-filter`, no big blurred shadows.
- Do not hold the engine's `Mutex` across an `.await`.
- Drain rows past the limit from the socket without sending them; do not buffer them.
- `statement_at` works in UTF-16 offsets on the UI side and bytes in Rust. Convert at the bridge only (`utf16_to_byte` / `byte_to_utf16`).

## Every change

1. **Understand first.** Read the code you will touch and the area rule below. For a bug, reproduce it and name the root cause before the fix. If the request is unclear, ask one precise question.
2. **Keep it small.** One logical change per PR. Do not refactor unrelated code. If you see something better but out of scope, say so in the PR; do not do it.
3. **Tests.** Unit test in the crate that owns the code; an integration test when MySQL behavior matters. UI has no test runner yet: verify by hand in light and dark.
4. **Bindings.** Touched `Command`, `Event` or a `model` type → `just bindings` and commit the output.
5. **Check.** `just check` (or `just check-docker`) must pass. Run `just test-db` when you touch `drivers/mysql` or `engine`.
6. **Docs.** Behavior or architecture changed → update [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). UI rule changed → [docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md).
7. **CHANGELOG.md.** A user-visible change adds a line under `[Unreleased]` (Keep a Changelog sections: Added, Changed, Fixed, Removed).
8. **UI changes** include screenshots in light and dark for the PR.
9. **Commits and PRs.** Conventional Commits (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`), imperative, under 72 characters. PR body follows `.github/PULL_REQUEST_TEMPLATE.md`. **Never add `Co-Authored-By` or any AI attribution line to commits or PRs.** Contributors sign the CLA ([CLA.md](CLA.md)).
10. **Do not commit, push or open a PR** unless the user asked.

## Writing style

Short sentences, plain words, specific names and numbers. No filler, no em dash. One home per fact: link to it instead of copying it. Do not put counts or timings in prose that will drift.

## CI

`.github/workflows/ci.yml` runs on every PR and push to `main`: UI (`pnpm check`, `pnpm build`), Rust on Linux (fmt, clippy, test, bindings up to date), Rust on macOS (clippy, test), Integration (MySQL), cargo-deny, desktop and AppStream validation. A PR merges when they are green. Read why a job failed before you re-run it.

Release: bump `version` in `Cargo.toml`, then push the tag `v<version>`. `.github/workflows/release.yml` checks the tag matches, runs CI, builds the AppImage, `.deb`, Flatpak and `.dmg` on GitHub runners, then publishes a public GitHub Release with exactly those files. A PR that edits `release.yml`, or a manual run, builds without publishing. Actions are pinned by commit SHA; details in [packaging.md](.claude/rules/packaging.md).

## Area rules

Claude Code loads a rule when it reads or edits a matching file. Under Codex, read the rule before editing a matching file.

| Rule | Read before editing |
| --- | --- |
| [protocol.md](.claude/rules/protocol.md) | `crates/engine`, `crates/model`, `src-tauri/src`, `ui/src/bindings`, `ui/src/lib/engine.ts` |
| [rust-core.md](.claude/rules/rust-core.md) | `crates/**/*.rs`, `Cargo.toml`, `deny.toml`, `clippy.toml` |
| [drivers.md](.claude/rules/drivers.md) | `crates/driver-api`, `crates/drivers`, `crates/sql`, `tests` |
| [ui.md](.claude/rules/ui.md) | `ui/src`, `biome.json`, `tsconfig.json`, `vite.config.ts` |
| [packaging.md](.claude/rules/packaging.md) | `packaging`, `docker`, `justfile`, `.github/workflows`, `src-tauri/tauri*.json`, `src-tauri/capabilities` |
