---
paths:
  - crates/**/*.rs
  - Cargo.toml
  - crates/**/Cargo.toml
  - deny.toml
  - clippy.toml
  - rustfmt.toml
---

# Rust core

## Dependency direction

`src-tauri → engine → {driver-api, sql, store, tunnel} → model`, and `engine → drivers/*`.

- `model` imports no internal crate and does no IO.
- `driver-api`, `sql`, `store`, `tunnel` import `model` (and `driver-api` for drivers), never `engine`.
- Only `engine` names a concrete driver. `src-tauri` reaches the core through `quarry_engine`.
- A new crate goes in `[workspace] members`, gets `[workspace.dependencies]` entry `quarry-<name>`, `[lints] workspace = true`, and an AGPL exception in `deny.toml`.

## Lints and style

- `unsafe_code`, `unwrap_used`, `expect_used`, `dbg_macro` are `deny`; `todo` and `missing_debug_implementations` warn. `clippy.toml` allows `unwrap` / `expect` in tests only.
- Handle the failure or return it. In places that cannot fail meaningfully, use `unwrap_or_else(|p| p.into_inner())` for poisoned locks (see `lock()` in the engine) or log and continue.
- `cargo fmt --all`, width 100, edition 2024. Let-chains (`if let … && …`) are fine on this toolchain.
- Library errors: `thiserror`. Boundary code converts to a user-facing `String` once, in the engine.
- Log with `tracing` (`warn!`, `error!` with fields). Never log a password, a DSN with a password, or a full SQL result.
- `missing_debug_implementations`: any type holding a secret implements `Debug` by hand and redacts it.

## Store

- `connections.toml` lives in the OS config dir (`~/.config/quarry`, `~/Library/Application Support/quarry`), has `version`, and never holds secrets.
- Passwords: keyring service `quarry`, account `<connection uuid>`. Linux uses `oo7` (Secret Service); macOS uses `security-framework`. A missing item is not an error.
- A format change bumps `SCHEMA_VERSION` and adds a migration with a test that loads an old file.

## Tests

- Unit tests beside the code; `unwrap` is allowed there.
- `cargo test --workspace` must pass without Docker. Anything needing MySQL checks `QUARRY_TEST_MYSQL` and returns early when it is unset.
- Run `just check` before you say a Rust change is done.
