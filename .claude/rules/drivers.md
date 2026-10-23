---
paths:
  - crates/driver-api/**
  - crates/drivers/**
  - crates/sql/**
  - tests/**
---

# Drivers and SQL

## The contract

`quarry-driver-api` defines `Driver` (`id`, `capabilities`, `dialect`, `connect`) and `Session` (`execute`, `execute_params`, `list_databases`, `list_objects`, `use_database`, transactions, `set_read_only`, `cancel_handle`, `close`). The engine talks only to `dyn Driver` / `dyn Session`. Every type crossing the boundary is serializable so drivers can move out of process later.

- `execute` streams `ExecItem`s into a sink: `Columns`, `Rows` (batches), `SetDone`, `Notice`, `Done`. A script yields several result sets. The sink is called synchronously, so it must be cheap (push to a channel).
- Respect `ExecOpts.row_limit`: past the limit, drain rows from the socket without sending them and set `truncated`.
- `cancel_handle()` must work from another task while `execute` runs. For MySQL it opens a side connection and sends `KILL QUERY`.
- `Capabilities` is the truth the UI reads. Set a flag only when the driver really supports it.
- Errors: use the `DriverError` variants. `Connect` / `Auth` carry ready-to-show multi-line messages (see `crates/drivers/mysql/src/diagnose.rs`: say what happened and what to do).

## Adding a driver

1. New crate `crates/drivers/<name>`, workspace member, `quarry-driver-<name>` dependency entry, AGPL exception in `deny.toml`.
2. Implement `Driver`, `Session`, `Dialect`; map native values to `Value` (unknown types → the `Other` fallback, never a failure).
3. Register it in `Engine::new` (`crates/engine/src/engine.rs`). Nowhere else.
4. Add a service to `tests/docker-compose.yml`, fixtures in `tests/fixtures/<name>/`, an integration test gated by `QUARRY_TEST_<NAME>`, and a CI job step.
5. Update the crate table in `docs/ARCHITECTURE.md` and add a CHANGELOG line.

## MySQL driver

- `mysql_async` with rustls (`default-rustls-ring`). SSL modes: `Disable`, `Prefer`, `Require`, `VerifyCa`, `VerifyFull`.
- `convert.rs` maps MySQL values: `BIGINT` / `BIGINT UNSIGNED` → `Int` / `UInt`, `DECIMAL` → `Decimal(String)`, binary → `Bytes`. Keep exactness; never go through `f64` for integers or decimals.
- `diagnose.rs` turns raw connect errors into guidance. A new failure mode gets a case and a test.
- Introspection goes through `information_schema`.
- Errors: a server error that means the connection is gone maps to `DriverError::ConnectionLost`, never `Query`. A syntax error carries its `ErrorLocation`. After a multi-statement call, read every pending result, errors included: one left behind fails the session's next query.

## `quarry-sql`

- `split`, `statement_at` and `classify` split on `;` outside `'..'`, `".."`, backticks and `--`, `#`, `/* */` comments, and work in UTF-8 byte offsets. A line `DELIMITER <token>` changes the terminator and is not a statement. `$$` quoting is not supported yet; adding it is a `split` change with tests.
- `completion_context` reads what fits at a cursor from tokens: pure, linear in the statement, called on every keystroke; a case it gets wrong gets a test in its module.
- `script` is what the engine runs one by one: `split`, except a script with a compound statement (`CREATE … BEGIN`, `BEGIN NOT ATOMIC`) and no `DELIMITER` line, which goes whole.
- `classify` returns `Read`, `Write`, `Schema` or `Destructive` from the first keyword. Unknown keywords count as `Write` so safe mode errs on the safe side. A change to `classify` is a safe-mode change: add tests for the new keyword and for lookalikes in comments and strings.
- It must never panic on malformed or huge input.

## Integration tests

```bash
just db-up && just test-db     # MySQL 8.4 on 127.0.0.1:13306, user quarry / password quarry
```

Tests skip themselves when `QUARRY_TEST_MYSQL` is unset. Say so when you could not run them.
