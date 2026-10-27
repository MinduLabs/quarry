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

## Crates

- `model` imports no internal crate and does no IO.
- `driver-api`, `sql`, `store`, `tunnel` never import `engine`.
- A new crate: add it to `[workspace] members`, add a `quarry-<name>` entry in
  `[workspace.dependencies]`, set `[lints] workspace = true`, and add its AGPL exception in
  `deny.toml`.

## Errors and logging

- Handle the failure or return it. A poisoned lock recovers with `unwrap_or_else(|p| p.into_inner())`.
- Library errors use `thiserror`. Boundary code turns them into user-facing text once, in the engine.
- Log with `tracing` fields. Never log a password, a DSN with a password, or result values.
- A type holding a secret implements `Debug` by hand and redacts it.

## Store

- Config and data files are written through `write_atomic`. A file another task may write at the
  same time takes a lock.
- A format change bumps the file's `SCHEMA_VERSION` and adds a migration with a test that loads an
  old file.
- A missing keyring item is not an error. Keyring account names are part of the stored format:
  changing one needs a migration.

## Tests

- `cargo test --workspace` passes without Docker. A test that needs a server checks its
  `QUARRY_TEST_*` variable and returns early when it is unset.
