---
name: qd
description: Drive, test and measure the running Quarry dev app with ./qd (start it, read the screen as text, click, type, run SQL, read logs and timings). Use to check a UI change in the real app, reproduce a bug, or measure performance, instead of screenshots or ad-hoc scripts.
---

# qd

The guide is [docs/DEVTOOLS.md](../../../docs/DEVTOOLS.md). Read it once, then:

1. `./qd up` (add `-f` for an empty sandbox). The first build takes minutes; run it in the background.
2. `./qd ui -a` once to see the screen with refs, then act by ref: `./qd click e4`.
   Each action prints only what changed. Run `./qd ui` again only when you need it.
3. Slow things (driver download, long query): `./qd wait "<text>"`.
4. Check state without the screen: `./qd ps`, `./qd state <path>`, `./qd ev`, `./qd logs`.
5. Measure with `./qd perf reset`, the steps, then `./qd perf`.
6. `./qd shot` only when you must see the layout; open the printed file.
7. `./qd down` when done.

Test databases: `docker compose -f tests/docker-compose.yml up -d --wait` (addresses and users in
README › Test database). Turn off "Save password" when saving a sandbox connection, so nothing goes
to the developer's keyring.
