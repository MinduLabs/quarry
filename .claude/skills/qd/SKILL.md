---
name: qd
description: Drive, test and measure the running Quarry dev app with ./qd (start it, read the screen as text, click, type, run SQL, read logs and timings). Use to check a UI change in the real app, reproduce a bug, or measure performance, instead of screenshots or ad-hoc scripts.
---

# qd

The guide is [docs/DEVTOOLS.md](../../../docs/DEVTOOLS.md). Read it once, then:

1. Start the test databases, then `./qd up` (add `-f` for an empty sandbox). The first build
   takes minutes; run it in the background. `up` connects "MySQL test" and the other test
   databases that are up. After a Rust change, run `./qd up` again: it waits and reconnects.
2. `./qd ui -a` once to see the screen with refs, then act by ref: `./qd click e4`.
   Open a table with `./qd open <table>` rather than through the tree (a click on a
   connection that is open folds it).
   Each action prints only what changed. Run `./qd ui` again only when you need it.
3. Slow things (driver download, long query): `./qd wait "<text>"`.
4. Check state without the screen: `./qd ps`, `./qd text [word]`, `./qd state <path>`,
   `./qd ev`, `./qd logs`.
5. A flow of several steps: write them in a file and `./qd run <file>`; it stops at the first
   failure. Do not hide output with `| head` when chaining commands.
6. Measure with `./qd perf reset`, the steps, then `./qd perf`; `./qd bench <query> [n]`;
   `./qd mem`.
7. `./qd shot` only when you must see the layout; open the printed file.
8. `./qd down` when done.

Test databases: `docker compose -f tests/docker-compose.yml up -d --wait`. The sandbox never
touches the developer's keyring.
