---
paths:
  - crates/driver-api/**
  - crates/drivers/**
  - crates/sql/**
  - tests/**
---

# Drivers and SQL

## Driver contract

- A driver runs in its own process (`quarry-driver-proto`); the engine talks only to
  `dyn Driver` / `dyn Session` through it. Types crossing the boundary stay serializable.
- SQL syntax is data: a `DialectSpec`. Only DDL (`Driver::schema_change`) is driver code.
- The `execute` sink is called synchronously: keep it cheap (push to a channel).
- Respect `ExecOpts.row_limit`: past it, drain rows from the socket without sending or buffering
  them, and set `truncated`.
- `cancel_handle()` works from another task while `execute` runs.
- Set a `Capabilities` flag only when the driver really supports it. The UI trusts it.
- Map native values to `Value` exactly: never go through `f64` for integers or decimals. An unknown
  type falls back to `Other`, never a failure.
- Connect and auth errors are ready to show: what happened and what to do (see `diagnose.rs`). A new
  failure mode gets a case and a test.

## Adding a driver

1. New crate under `crates/drivers/<name>` (see [rust-core.md](rust-core.md) for the crate steps).
2. Implement `Driver` and `Session`, and describe the syntax with a `DialectSpec`.
3. Add `src/main.rs` that installs rustls's ring provider as the default, then calls
   `quarry_driver_proto::driver_main`. A workspace build gives rustls two providers, and a
   dependency that asks for the default panics without one. The engine finds the binary in the
   driver directory; nothing registers it. Add it to `driver_packages` in the justfile.
4. Add a service to `tests/docker-compose.yml`, fixtures in `tests/fixtures/<name>/`, an integration
   test gated by `QUARRY_TEST_<NAME>`, and a CI step.
5. Update the crate table in `docs/ARCHITECTURE.md`.

## `quarry-sql`

- Offsets are UTF-8 bytes.
- `classify` is a safe-mode decision. An unknown keyword counts as `Write`. A change adds tests for
  the keyword and for lookalikes inside comments and strings.
- A splitting change comes with tests for quotes, comments and the delimiter forms it touches.
- Never panic on malformed or huge input.

## Fixtures

- A feature that needs a kind of data the playground table lacks adds it to
  `tests/fixtures/mysql/02-playground.sql` and to `playground_samples_cover_every_type`.
