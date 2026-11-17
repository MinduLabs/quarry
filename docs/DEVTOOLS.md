# Devtools: `./qd`

`./qd` drives the dev app from a terminal: start it, read the screen, click, type, run SQL, read
logs and timings. It is made for AI agents as much as for people: every command prints a few
lines of text instead of a screenshot, and an action waits until the screen settles, then prints
what changed.

It is a development tool. Release builds do not contain it.

## Requirements

- Linux. `qd` talks to the window through WebKitGTK's remote inspector, which macOS does not have.
- What `just dev` needs (see [README › Development setup](../README.md#development-setup)).
- Node 20 or newer (already needed for the UI).
- For databases to test against: `docker compose -f tests/docker-compose.yml up -d --wait`
  ([README › Test database](../README.md#test-database)).

## Quick start

```bash
./qd up              # build the drivers, start the dev app in its sandbox, wait until it is ready
./qd ui -a           # the whole screen as text, with refs (e4, e12 …)
./qd click "New connection"
./qd pick "Sign in with" "AWS IAM"
./qd type "Host" "127.0.0.1"
./qd ps              # connections, tabs, dialog, toasts
./qd down
```

## Commands

| Command | What it does |
| --- | --- |
| `up [-f]` | Start the dev app in the sandbox and wait until it is ready. `-f` starts from an empty sandbox. When the app runs already (or is rebuilding after a Rust change), `up` waits for it. |
| `down` | Stop the app, Vite and the driver server. |
| `ps` | Connections and their status, tabs, the open dialog, driver downloads, toasts. |
| `ui [-a]` | What changed on screen since the last `ui`, as `+`/`-` lines. `-a` prints everything. |
| `find <text>` | Only the screen lines that contain the text. Exit code 1 when none does. |
| `click <ref\|name>` | Click. `--right` opens a context menu, `--double` double-clicks. |
| `type <ref\|name> <text>` | Set an input's text (`--append` adds to it). Also types into the SQL editor. |
| `pick <ref\|name> <option>` | Open a select or menu and choose an option by its name. |
| `key <combo>` | Press keys on the focused element: `ctrl+n`, `escape`, `enter`, `f5`, `mod+k`. |
| `wait <text> [sec]` | Wait until the text is on screen or in a toast (default 30 s). |
| `shot [ref] [-o file]` | Screenshot of the window or of one element. Prints the file. |
| `state [path]` | Part of the UI store as short JSON: `dialog`, `nodes`, `tabs.0.browse`. |
| `ev [-a]` | The Command/Event traffic since the last `ev`, one line each. `-a`: all that is kept. |
| `logs [-a]` | Warnings and errors from the app since the last `logs`. `-a`: every line. |
| `perf [reset]` | Timings since the last `perf`: count, p50, p95 and max for each name. |
| `open <table>` | Open a table of the active connection's database, wherever the tree is. `database.table` or `connection/database.table` picks another. |
| `sql <query>` | Run a query in a new tab of the active connection. Prints the time and the first rows. |
| `text [word] [-a]` | The readable text of the open dialog (or the window), for what `ui` leaves out: lists, results, explanations. With `word`, only the lines that contain it (exit code 1 when none). 60 lines unless `-a`. |
| `seed` | Save and connect the test databases that are up. `up` does it too. |
| `bench <query> [n]` | Run a query `n` times (default 20) after one warm-up run: p50, p95 and max. |
| `mem` | Resident memory of the app, its drivers and the dev servers. |
| `run <file\|->` | Run `qd` commands from a file (or standard input), one per line, and stop at the first failure. |
| `exec <js>` | Evaluate JavaScript in the window and print the result. |

### Screen lines

```
dialog "Download the driver?"
e3 button "Cancel"
e4 button "Download and connect"
e7 input "Host" = "127.0.0.1"
e8 combobox "SSL" = "Preferred"
e9 treeitem "Old saved MySQL" selected
grid · 3 rows shown · id | user_id | total
  e20 1 | 2 | 19.90
text "Checking the driver…"
toast error "Old saved MySQL: Access denied …"
```

- `eN` is a ref. It stays the same for the same element, so `click e4` works after any `ui`.
- A target can also be the name shown on screen (`click "Save"`). When a name matches more than
  one element, `qd` lists them with their refs.
- When a dialog or menu is open, the screen is only what the user can reach: the menu and the
  dialog.
- Grids show their header and the first rows. Read more rows with `state` or `sql`.

### Waiting

`click`, `type`, `pick` and `key` wait until the screen holds still (up to 8 s), then print the
change. Something slower, like a driver download or a long query, needs `wait <text>`. `sql` waits
for its query itself.

## Test databases

With the test databases up (`docker compose -f tests/docker-compose.yml up -d --wait`), `up` saves
a connection to each one that answers ("MySQL test", "PostgreSQL test", "SQL Server test", database
`shop`), downloads its driver and connects it. `seed` does the same at any time. The connections
have fixed ids, so seeding again updates them.

After a Rust change, `tauri dev` restarts the app and the engine forgets the passwords it held. Run
`./qd up`: it waits for the app and seeds again.

## Scripts

`run` takes one command per line, without `./qd`. Quote words with spaces. `#` starts a comment.
It prints each command and its output, and stops at the first one that fails, with its line.

```
# check-compare.qd
click "MySQL test" --right
click "New query"
sql "SELECT COUNT(*) FROM orders"
key ctrl+shift+p
type Search ">compare"
key enter
text differ
```

```bash
./qd run check-compare.qd
```

## The sandbox

`up` runs `pnpm tauri dev` with everything in `target/qd`:

| Path | What |
| --- | --- |
| `config/`, `data/` | The app's config and data folders (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`): saved connections, workspace, installed drivers. Your own are untouched. |
| `aws/` | Empty AWS config and credentials files. Your AWS profiles and `AWS_*` variables are kept out. |
| `app.log` | Everything the app, Vite and Cargo print. `logs` reads it. |
| `data/quarry/perf.jsonl` | The performance log (`QUARRY_PERF=1`). `perf` reads it. |
| `shots/` | Screenshots. |
| `qd.json` | The app's process and where `ev`, `logs` and `perf` last stopped. |

Drivers are built with `just drivers` and served by `scripts/drivers.py`, as in `just dev`. The
app downloads one the first time a connection needs it. `up -f` empties the sandbox, so it starts
with no connections and no drivers.

The sandbox app never touches the system keyring (`QUARRY_NO_KEYRING=1`): passwords stay in its
memory, whatever "Save password" says. `ev` and `state` print `***` in place of any password.

## Measuring

```bash
./qd perf reset
./qd click e9                 # open a table, run a query, scroll …
./qd perf
./qd bench "SELECT * FROM orders WHERE total > 100" 50
./qd mem
```

```
name  n  p50  p95  max
query  1  42.0ms  42.0ms  42.0ms
render.grid  6  2.1ms  7.9ms  7.9ms
```

The names are the engine's and the UI's timings in `perf.jsonl` (`crates/engine/src/perf.rs`,
`ui/src/lib/perf.ts`).

## Limits

- One app at a time, on Vite's port 5173: `just dev` and `qd up` cannot run together.
- Keys and clicks are events sent to the page, not real input. `key` does not type into a field:
  use `type`.
- The inspector port is 9230 and the driver server's is 47831 (`QD_INSPECTOR_PORT`,
  `QD_DRIVER_PORT`).

## How it works

`ui/src/lib/devtools.ts` puts a small bridge on `window.__qd` when the UI runs under `vite dev`.
It reads the page's roles and names, the store and the Command/Event traffic
(`setDevTap` in `ui/src/lib/engine.ts`). `scripts/qd.mjs` calls it through the inspector with
`Runtime.evaluate`, and takes screenshots with `Page.snapshotRect`. Nothing is added to the engine
or to the protocol.
