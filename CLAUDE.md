# CLAUDE.md

Rules for AI agents (Claude Code, Codex, …) working on Quarry. `AGENTS.md` links here.

This file holds rules only. It does not change when a feature lands.

- What the project is, its layout and every command: [README.md](README.md).
- Design and the reasons behind it: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- Look and motion: [docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md).
- PR process: [CONTRIBUTING.md](CONTRIBUTING.md).

## How to work

1. **Understand first.** Read the code you will touch and its area rule. For a bug, reproduce it
   and name the root cause before the fix. If the request is unclear, ask one precise question.
2. **Fix the cause, not the symptom.** No workaround left behind.
3. **Keep it small.** One logical change. Do not refactor unrelated code; mention it instead.
4. **Tests define behavior.** A testable change ships with a test. When a test fails, fix the
   code, not the test.
5. **Check before you say done.** `just check` (or `just check-docker`). `just test-db` when you
   touch `crates/engine` or `crates/drivers`. Integration tests skip without `QUARRY_TEST_MYSQL`;
   a skipped run proves nothing, so say when you could not run them.
6. **Do not run builds** (`just appimage`, `deb`, `flatpak`, `dmg`, Docker) unless asked. Editing
   a build file is not a reason to run it.
7. **Do not commit, push or open a PR** unless asked. Commits follow Conventional Commits.
   **Never add `Co-Authored-By` or any AI attribution** to commits or PRs.
8. **Docs:** follow the PR checklist in [CONTRIBUTING.md](CONTRIBUTING.md). Each fact has one home;
   link to it instead of copying it. Do not add feature descriptions, file lists or counts to this
   file or to `.claude/rules/`.

## Invariants

- **Headless core.** Database, keyring and disk access live in `crates/engine` and below.
  Dependencies point down: `src-tauri → engine → {driver-api, sql, store, tunnel} → model`. Only
  the engine names a concrete driver. `src-tauri` forwards and decides nothing.
- **One protocol.** UI ↔ core only through `Command` / `Event`. A new capability is a new variant,
  then `just bindings`, then commit `ui/src/bindings/`. Never edit `ui/src/bindings/`.
- **Capabilities, not names.** No `if driver == "mysql"` in the UI or the engine.
- **Data safety lives in the engine.** Safe-mode confirmation is an engine round trip. No client
  classifies SQL to decide safety.
- **Bounded results.** Rows stream in batches up to a row limit. Nothing loads a whole table.
- **Secrets stay out.** Passwords live in the OS keyring, never in config files, logs, errors or
  events. Types holding a secret keep a redacting `Debug`.
- **Exact numbers.** Integers that do not fit a JS number travel as strings. Do not widen a `u64`
  field without checking its TS type.
- **Never block.** No blocking IO on the tokio runtime; the UI never waits on IO.
- **A refresh never clears what it refreshes.** Fetch, then replace; keep good data on failure.
- **File formats are versioned.** A format change adds a migration; old files are never silently
  rewritten.
- **Errors say what happened and what to do.** No raw driver dumps.

## Code style

- Rust: `cargo fmt`, clippy clean with `-D warnings`; lints in `Cargo.toml` are the source of truth.
  No `unwrap()` / `expect()` / `unsafe` / `dbg!` / `todo!` outside tests. Errors with `thiserror`,
  logs with `tracing`, never `println!`.
- TypeScript: strict, no `any`, imports through `@/`. Biome formats.
- Tests sit beside the code in `#[cfg(test)] mod tests`; integration tests in `crates/*/tests/`.
- Comments say why, never what. Delete what-comments in code you touch. English only.
- Early returns, small functions, self-explanatory names. Booleans start with `is`, `has`, `can`,
  `should`.
- Docs: short sentences, plain words, no em dash, no counts or timings that will drift.

## Area rules

Claude Code loads a rule when it reads or edits a matching file. Under Codex, read the rule before
editing a matching file.

| Rule | Applies to |
| --- | --- |
| [protocol.md](.claude/rules/protocol.md) | `crates/engine`, `crates/model`, `src-tauri/src`, `ui/src/lib/engine.ts` |
| [rust-core.md](.claude/rules/rust-core.md) | `crates/**/*.rs`, `Cargo.toml`, `deny.toml`, `clippy.toml` |
| [drivers.md](.claude/rules/drivers.md) | `crates/driver-api`, `crates/drivers`, `crates/sql`, `tests` |
| [ui.md](.claude/rules/ui.md) | `ui/src`, `biome.json`, `tsconfig.json`, `vite.config.ts` |
| [packaging.md](.claude/rules/packaging.md) | `packaging`, `docker`, `justfile`, `.github/workflows`, `src-tauri/tauri*.json`, `src-tauri/capabilities` |
