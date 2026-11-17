// `./qd`: drive the Quarry dev app from a terminal or an AI agent. Guide: docs/DEVTOOLS.md.
//
// `qd up` runs `pnpm tauri dev` in a sandbox (target/qd) with WebKit's remote inspector on.
// Every other command opens the inspector, calls `window.__qd` (ui/src/lib/devtools.ts) and
// prints a few lines. Linux (WebKitGTK) only for now.

import { spawn, spawnSync } from "node:child_process";
import { closeSync, existsSync, mkdirSync, openSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { connect as tcpConnect } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SANDBOX = join(ROOT, "target", "qd");
const STATE_FILE = join(SANDBOX, "qd.json");
const LOG_FILE = join(SANDBOX, "app.log");
const PERF_FILE = join(SANDBOX, "data", "quarry", "perf.jsonl");
const INSPECTOR_PORT = Number(process.env.QD_INSPECTOR_PORT ?? 9230);
const DRIVER_PORT = Number(process.env.QD_DRIVER_PORT ?? 47831);
const UP_TIMEOUT_MS = 20 * 60 * 1000;
const SETTLE_MS = 300;
const SETTLE_MAX_MS = 8000;

const HELP = `qd: drive the Quarry dev app (docs/DEVTOOLS.md)

  up [-f]               start the dev app in target/qd (-f: start from an empty sandbox)
  down                  stop it
  ps                    connections, tabs, dialog, driver downloads, toasts
  ui [-a]               what changed on screen since the last ui (-a: everything), with refs
  find <text>           elements whose line contains text
  click <ref|name>      click (--right: context menu, --double: double-click)
  type <ref|name> <text>  set an input's text (--append to add)
  pick <ref|name> <option>  open a select or menu and choose an option
  key <combo>           ctrl+n, escape, enter, f5, mod+k …
  wait <text> [sec]     until the text is on screen (default 30 s)
  shot [ref] [-o file]  screenshot of the window or one element; prints the file
  state [path]          part of the store as JSON (dialog, nodes, tabs.0.browse …)
  ev [-a]               Command/Event traffic since the last ev (-a: all kept)
  logs [-a]             warnings and errors since the last logs (-a: every line)
  perf [reset]          timings since the last perf: count, p50, p95, max per name
  open <table>          open a table of the active connection (database.table, connection/database.table)
  sql <query>           run in a new tab of the active connection; first rows and time
  text [word]           the readable text of the open dialog or the window (-a: no limit)
  seed                  save and connect the test databases that are up (up does it too)
  bench <query> [n]     run a query n times (default 20); p50, p95, max
  mem                   memory of the app, its drivers and dev servers
  run <file|->          run qd commands from a file, one per line; stop at the first failure
  exec <js>             evaluate JavaScript in the window; prints the result
`;

// ---------- Sandbox state ----------

function readState() {
  try {
    return JSON.parse(readFileSync(STATE_FILE, "utf8"));
  } catch {
    return {};
  }
}

function writeState(patch) {
  mkdirSync(SANDBOX, { recursive: true });
  writeFileSync(STATE_FILE, JSON.stringify({ ...readState(), ...patch }, null, 1));
}

function isAlive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** The line `qd run` is on, so a failure says where the script stopped. */
let runLine = null;

function fail(message) {
  console.error(runLine ? `✗ line ${runLine.number}: qd ${runLine.text}\n${message}` : message);
  process.exit(1);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------- Inspector (WebKit protocol) ----------

async function pageSocket() {
  const html = await fetch(`http://127.0.0.1:${INSPECTOR_PORT}/`).then((r) => r.text());
  const match = html.match(/localhost:5173[\s\S]*?(socket\/\d+\/\d+\/WebPage)/);
  return match ? `ws://127.0.0.1:${INSPECTOR_PORT}/${match[1]}` : null;
}

async function connect() {
  const url = await pageSocket().catch(() => null);
  if (!url) fail("The dev app is not running. Start it with: ./qd up");
  const ws = new WebSocket(url);
  await new Promise((ok, err) => {
    ws.onopen = ok;
    ws.onerror = () => err(new Error("cannot open the inspector socket"));
  });
  let id = 0;
  let target = null;
  const pending = new Map();
  let onTarget;
  const targetReady = new Promise((r) => {
    onTarget = r;
  });
  ws.onmessage = (m) => {
    const d = JSON.parse(String(m.data));
    if (d.method === "Target.targetCreated" && d.params.targetInfo.type === "page") {
      target = d.params.targetInfo.targetId;
      onTarget();
    } else if (d.method === "Target.dispatchMessageFromTarget") {
      const inner = JSON.parse(d.params.message);
      pending.get(inner.id)?.(inner);
      pending.delete(inner.id);
    } else if (d.id) {
      pending.get(d.id)?.(d);
      pending.delete(d.id);
    }
  };
  // The timer must not outlive the race: a pending one keeps Node alive, which made every
  // command wait 1.5 s before it could exit.
  let timer;
  await Promise.race([targetReady, new Promise((r) => (timer = setTimeout(r, 1500)))]);
  clearTimeout(timer);
  const call = (method, params = {}) =>
    new Promise((r) => {
      const inner = ++id;
      pending.set(inner, r);
      const message = { id: inner, method, params };
      ws.send(
        JSON.stringify(
          target
            ? { id: ++id, method: "Target.sendMessageToTarget", params: { targetId: target, message: JSON.stringify(message) } }
            : message,
        ),
      );
    });
  const evaluate = async (expression) => {
    const r = await call("Runtime.evaluate", { expression, returnByValue: true });
    if (r.error) fail(`inspector: ${r.error.message}`);
    if (r.result.wasThrown) fail(`error: ${r.result.result.description ?? JSON.stringify(r.result.result)}`);
    return r.result.result.value;
  };
  /** Call `window.__qd.<fn>(...args)` in the page. */
  const qd = async (fn, ...args) => {
    const ready = await evaluate("typeof window.__qd");
    if (ready !== "object") fail("The window has no qd bridge yet (still loading?). Try again, or ./qd up -f.");
    return evaluate(`window.__qd.${fn}(${args.map((a) => JSON.stringify(a)).join(",")})`);
  };
  return { call, evaluate, qd, close: () => ws.close() };
}

/** Wait until the screen holds still, so an action's result is there before it is read. */
async function settle(app) {
  const start = Date.now();
  let last = null;
  let since = Date.now();
  while (Date.now() - start < SETTLE_MAX_MS) {
    const now = await app.qd("fingerprint");
    if (now !== last) {
      last = now;
      since = Date.now();
    } else if (Date.now() - since >= SETTLE_MS) {
      return;
    }
    await sleep(80);
  }
}

// ---------- Commands ----------

async function up(fresh) {
  const state = readState();
  if (isAlive(state.pid)) {
    if (fresh) fail("The app is running; ./qd down first, then ./qd up -f.");
    // Running but not answering yet: `tauri dev` is rebuilding after a change. Wait for it
    // instead of starting a second app on the same ports.
    console.log("already running; waiting for it to answer …");
    return waitReady(state.pid);
  }
  if (fresh) rmSync(SANDBOX, { recursive: true, force: true });
  for (const dir of ["config", "data", "aws", "shots"]) mkdirSync(join(SANDBOX, dir), { recursive: true });
  for (const file of ["config", "credentials"]) {
    const path = join(SANDBOX, "aws", file);
    if (!existsSync(path)) writeFileSync(path, "");
  }
  const env = {
    ...process.env,
    XDG_CONFIG_HOME: join(SANDBOX, "config"),
    XDG_DATA_HOME: join(SANDBOX, "data"),
    QUARRY_DRIVER_MANIFEST: `http://127.0.0.1:${DRIVER_PORT}/drivers.json`,
    QUARRY_PERF: "1",
    // Test passwords stay in memory: the developer's keyring is never touched.
    QUARRY_NO_KEYRING: "1",
    WEBKIT_INSPECTOR_HTTP_SERVER: `127.0.0.1:${INSPECTOR_PORT}`,
    // Keep the developer's own AWS credentials out of the sandbox.
    AWS_CONFIG_FILE: join(SANDBOX, "aws", "config"),
    AWS_SHARED_CREDENTIALS_FILE: join(SANDBOX, "aws", "credentials"),
    AWS_EC2_METADATA_DISABLED: "true",
  };
  for (const name of ["AWS_PROFILE", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "AWS_REGION", "AWS_DEFAULT_REGION"]) {
    delete env[name];
  }
  console.log("building drivers …");
  const built = spawnSync("just", ["drivers"], { cwd: ROOT, stdio: ["ignore", "ignore", "inherit"] });
  if (built.status !== 0) fail("just drivers failed");
  // Installed drivers in the sandbox are replaced with the ones just built, as `just dev` does.
  spawnSync("python3", ["scripts/drivers.py", "refresh", "target/drivers"], { cwd: ROOT, env, stdio: "ignore" });
  const log = openSync(LOG_FILE, "a");
  const child = spawn(
    "bash",
    ["-c", `python3 scripts/drivers.py serve target/drivers ${DRIVER_PORT} & exec pnpm tauri dev`],
    { cwd: ROOT, env, detached: true, stdio: ["ignore", log, log] },
  );
  child.unref();
  closeSync(log);
  writeState({ pid: child.pid, evCursor: 0, logOffset: statSync(LOG_FILE).size, perfOffset: 0 });
  console.log("starting the app (the first build takes a few minutes) …");
  return waitReady(child.pid);
}

/** Wait until the app `pid` serves its window with the qd bridge, then print `ps`. */
async function waitReady(pid) {
  const start = Date.now();
  while (Date.now() - start < UP_TIMEOUT_MS) {
    if (!isAlive(pid)) {
      const tail = readFileSync(LOG_FILE, "utf8").split("\n").slice(-20).join("\n");
      fail(`the app stopped:\n${stripAnsi(tail)}`);
    }
    if (await pageSocket().catch(() => null)) {
      const app = await connect();
      const ready = await app.evaluate("typeof window.__qd").catch(() => "");
      app.close();
      if (ready === "object") {
        console.log(`up · sandbox target/qd · inspector 127.0.0.1:${INSPECTOR_PORT}`);
        // An app that restarted lost the test passwords it kept in memory: give them again.
        await seed();
        return ps();
      }
    }
    await sleep(1000);
  }
  fail("the app did not come up in time; see target/qd/app.log");
}

async function down() {
  const { pid } = readState();
  if (!isAlive(pid)) {
    console.log("not running");
    return;
  }
  // The app, Vite and the driver server share one process group.
  process.kill(-pid, "SIGTERM");
  for (let i = 0; i < 50 && isAlive(pid); i++) await sleep(100);
  if (isAlive(pid)) process.kill(-pid, "SIGKILL");
  writeState({ pid: null });
  console.log("down");
}

async function withApp(work) {
  const app = await connect();
  try {
    return await work(app);
  } finally {
    app.close();
  }
}

const ps = () => withApp(async (app) => console.log(await app.qd("ps")));

/** Run an action, wait for the screen to settle, then show what changed. */
const act = (fn, ...args) =>
  withApp(async (app) => {
    const result = await app.qd(fn, ...args);
    if (result !== "ok") fail(result);
    await settle(app);
    console.log(await app.qd("ui", true));
  });

/** Open a select or menu, wait for its options to render, then choose one. */
const pick = (target, option) =>
  withApp(async (app) => {
    const opened = await app.qd("click", target, "left");
    if (opened !== "ok") fail(opened);
    await settle(app);
    const chosen = await app.qd("choose", option);
    if (chosen !== "ok") {
      await app.qd("key", "escape");
      fail(chosen);
    }
    await settle(app);
    console.log(await app.qd("ui", true));
  });

async function wait(text, seconds) {
  await withApp(async (app) => {
    const until = Date.now() + seconds * 1000;
    while (Date.now() < until) {
      if (await app.qd("has", text)) {
        await settle(app);
        console.log(`found: ${(await app.qd("textLine", text)) ?? text}`);
        const changed = await app.qd("ui", true);
        if (changed !== "(no change)") console.log(changed);
        return;
      }
      await sleep(200);
    }
    fail(`"${text}" did not show in ${seconds} s`);
  });
}

async function shot(target, file) {
  await withApp(async (app) => {
    const box = target ? await app.qd("rect", target) : await app.evaluate("[0, 0, innerWidth, innerHeight].map(Math.round)");
    if (typeof box === "string") fail(box);
    const [x, y, width, height] = box;
    const r = await app.call("Page.snapshotRect", { x, y, width, height, coordinateSystem: "Viewport" });
    if (r.error) fail(`screenshot: ${r.error.message}`);
    const out = file ? resolve(file) : join(SANDBOX, "shots", `${Date.now()}.png`);
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, Buffer.from(r.result.dataURL.split(",")[1], "base64"));
    console.log(out);
  });
}

async function ev(all) {
  await withApp(async (app) => {
    const since = all ? 0 : (readState().evCursor ?? 0);
    const { next, lines } = await app.qd("events", since);
    writeState({ evCursor: next });
    console.log(lines.length ? lines.join("\n") : "(no traffic)");
  });
}

function stripAnsi(text) {
  // biome-ignore lint/suspicious/noControlCharactersInRegex: ANSI escapes are control characters
  return text.replace(/\x1b\[[0-9;]*m/g, "");
}

/** The bytes a file gained since `offset` (all of it when it shrank). */
function since(path, offset) {
  if (!existsSync(path)) return { text: "", size: 0 };
  const buffer = readFileSync(path);
  const from = offset <= buffer.length ? offset : 0;
  return { text: buffer.subarray(from).toString("utf8"), size: buffer.length };
}

function logs(all) {
  const { text, size } = since(LOG_FILE, all ? 0 : (readState().logOffset ?? 0));
  writeState({ logOffset: size });
  const lines = stripAnsi(text)
    .split("\n")
    .filter((l) => l.trim() && (all || /\b(WARN|ERROR)\b|panicked/.test(l)))
    // `2026-11-17T04:08:57.811821Z  WARN quarry_engine::drivers: …` → `04:08:57 WARN quarry_engine::drivers: …`
    .map((l) => l.replace(/^\d{4}-\d\d-\d\dT(\d\d:\d\d:\d\d)\.\d+Z\s+/, "$1 "));
  console.log(lines.length ? lines.slice(-200).join("\n") : "(no warnings or errors)");
}

function perf(reset) {
  const { text, size } = since(PERF_FILE, readState().perfOffset ?? 0);
  writeState({ perfOffset: size });
  if (reset) {
    console.log("perf reset");
    return;
  }
  const groups = new Map();
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    try {
      const { name, ms } = JSON.parse(line);
      if (!groups.has(name)) groups.set(name, []);
      groups.get(name).push(ms);
    } catch {
      // A line cut by a write in progress; the next perf reads it whole.
    }
  }
  if (groups.size === 0) {
    console.log("(no timings since the last perf)");
    return;
  }
  const pct = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))];
  const rows = [...groups].map(([name, values]) => {
    const sorted = [...values].sort((a, b) => a - b);
    const total = sorted.reduce((a, b) => a + b, 0);
    return { name, n: sorted.length, p50: pct(sorted, 0.5), p95: pct(sorted, 0.95), max: sorted.at(-1), total };
  });
  rows.sort((a, b) => b.total - a.total);
  const f = (ms) => `${ms.toFixed(1)}ms`;
  console.log("name  n  p50  p95  max");
  for (const r of rows.slice(0, 20)) console.log(`${r.name}  ${r.n}  ${f(r.p50)}  ${f(r.p95)}  ${f(r.max)}`);
}

async function sql(query) {
  await withApp(async (app) => {
    const tab = await app.qd("sqlStart", query);
    if (typeof tab === "string") fail(tab);
    const until = Date.now() + 5 * 60 * 1000;
    while (Date.now() < until) {
      const result = await app.qd("sqlResult", tab);
      if (result !== null) {
        console.log(result);
        return;
      }
      // Safe mode asks first: the agent answers the dialog with qd click.
      const dialog = await app.evaluate(`document.querySelector('[role="dialog"]')?.innerText ?? null`);
      if (dialog) {
        console.log(`waiting for an answer:\n${dialog}`);
        return;
      }
      await sleep(100);
    }
    fail("the query is still running after 5 minutes");
  });
}

const exec = (js) => withApp(async (app) => console.log(JSON.stringify(await app.evaluate(js), null, 1) ?? "undefined"));

// ---------- Test databases ----------

/** The databases of tests/docker-compose.yml (test values only, as in README › Test database).
 * Fixed ids, so seeding again updates them instead of adding copies. */
const SEEDS = [
  { id: "00000000-0000-4000-8000-00000000a001", name: "MySQL test", driver: "mysql", port: 13306, user: "quarry", password: "quarry" },
  { id: "00000000-0000-4000-8000-00000000a002", name: "PostgreSQL test", driver: "postgres", port: 15432, user: "quarry", password: "quarry" },
  { id: "00000000-0000-4000-8000-00000000a003", name: "SQL Server test", driver: "sqlserver", port: 11433, user: "sa", password: "Quarry-test-1" },
];

/** Something listens on 127.0.0.1:`port`. */
const isListening = (port) =>
  new Promise((done) => {
    const socket = tcpConnect({ host: "127.0.0.1", port, timeout: 300 });
    const finish = (ok) => {
      socket.destroy();
      done(ok);
    };
    socket.on("connect", () => finish(true));
    socket.on("timeout", () => finish(false));
    socket.on("error", () => finish(false));
  });

/** Save, install the driver of, and connect every test database that is up. */
async function seed() {
  const up = [];
  // The last one connected becomes the active connection; MySQL test goes last, so it is.
  for (const db of [...SEEDS].reverse()) if (await isListening(db.port)) up.push(db);
  if (up.length === 0) {
    console.log("seed: no test database is up (docker compose -f tests/docker-compose.yml up -d --wait)");
    return;
  }
  await withApp(async (app) => {
    for (const db of up) {
      const config = {
        id: db.id,
        name: db.name,
        driver: db.driver,
        host: "127.0.0.1",
        port: db.port,
        database: "shop",
        user: db.user,
        path: null,
        ssl: "prefer",
        ssh: null,
        environment: "local",
        read_only: false,
        color: null,
        group: null,
      };
      const result = await app.qd("seed", config, db.password);
      if (result !== "ok") fail(`seed ${db.name}: ${result}`);
      const until = Date.now() + 120_000;
      let status = "";
      while (Date.now() < until) {
        status = await app.qd("connState", db.id);
        if (status === "connected" || status.startsWith("failed")) break;
        await sleep(200);
      }
      console.log(`seed: ${db.name} · ${status}`);
    }
  });
}

// ---------- Reading and measuring ----------

async function text(word, all) {
  await withApp(async (app) => {
    const result = await app.qd("text", word ?? "", all);
    if (word && result === null) fail(`no line contains "${word}"`);
    console.log(result);
  });
}

const percentile = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))];

async function bench(query, times) {
  await withApp(async (app) => {
    const tab = await app.qd("sqlStart", query);
    if (typeof tab === "string") fail(tab);
    const runOnce = async () => {
      const until = Date.now() + 5 * 60 * 1000;
      while (Date.now() < until) {
        const run = await app.qd("tabRun", tab);
        if (!run.running) return run;
        await sleep(20);
      }
      fail("a run took over 5 minutes");
    };
    // The first run warms the session and the server's caches; it is not counted.
    const first = await runOnce();
    if (first.error) fail(`error: ${first.error}`);
    const times_ms = [];
    for (let i = 0; i < times; i++) {
      const again = await app.qd("rerun", tab);
      if (again !== "ok") fail(again);
      const run = await runOnce();
      if (run.error) fail(`error on run ${i + 1}: ${run.error}`);
      times_ms.push(run.elapsed);
    }
    const sorted = [...times_ms].sort((a, b) => a - b);
    const ms = (v) => `${v.toFixed(2)} ms`;
    console.log(`${times} runs · ${first.rows} rows · p50 ${ms(percentile(sorted, 0.5))} · p95 ${ms(percentile(sorted, 0.95))} · max ${ms(sorted.at(-1))}`);
  });
}

/** Resident memory of every process the sandbox started (the app, drivers, Vite, Cargo). */
function mem() {
  const { pid } = readState();
  if (!isAlive(pid)) fail("The dev app is not running. Start it with: ./qd up");
  const page = 4096;
  const rows = [];
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const stat = readFileSync(`/proc/${entry}/stat`, "utf8");
      // `pid (comm) state ppid pgrp …`: comm may hold spaces, so read after its last ")".
      const close = stat.lastIndexOf(")");
      // The kernel cuts `comm` at 15 characters; the program path is whole.
      const argv0 = readFileSync(`/proc/${entry}/cmdline`, "utf8").split("\0")[0] ?? "";
      const name = argv0.split("/").at(-1) || stat.slice(stat.indexOf("(") + 1, close);
      const pgrp = Number(stat.slice(close + 2).split(" ")[2]);
      if (pgrp !== pid) continue;
      const rss = Number(readFileSync(`/proc/${entry}/statm`, "utf8").split(" ")[1]) * page;
      rows.push({ name, pid: entry, mb: rss / 1024 / 1024 });
    } catch {
      // The process ended while it was read.
    }
  }
  rows.sort((a, b) => b.mb - a.mb);
  // The window's web content runs in WebKit's own processes: they are part of the app.
  const isApp = (r) => r.name === "quarry" || r.name.startsWith("quarry-driver") || r.name.startsWith("WebKit");
  const app = rows.filter(isApp);
  for (const r of rows.filter((r) => r.mb >= 5)) console.log(`${r.name}  ${r.pid}  ${r.mb.toFixed(0)} MB`);
  const total = app.reduce((sum, r) => sum + r.mb, 0);
  console.log(`app (quarry, WebKit, drivers): ${total.toFixed(0)} MB · the rest is Vite, Cargo and the driver server`);
}

// ---------- Scripts ----------

/** Words of one script line, with "double quotes" around words that hold spaces. */
function splitLine(line) {
  const words = [];
  const pattern = /"((?:[^"\\]|\\.)*)"|(\S+)/g;
  for (const match of line.matchAll(pattern)) {
    words.push(match[1] !== undefined ? match[1].replace(/\\(.)/g, "$1") : match[2]);
  }
  return words;
}

async function run(file) {
  const source = file === "-" ? readFileSync(0, "utf8") : readFileSync(resolve(file), "utf8");
  const lines = source.split("\n");
  for (const [index, raw] of lines.entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const words = splitLine(line.replace(/^(\.\/)?qd\s+/, ""));
    if (words[0] === "run") fail("run cannot run another script");
    runLine = { number: index + 1, text: line };
    console.log(`$ qd ${line}`);
    await dispatch(words);
    console.log();
  }
  runLine = null;
}

// ---------- Arguments ----------

async function dispatch(argv) {
// A lone "-" is standard input (`run -`), not a flag.
const flags = new Set(argv.filter((a) => a.startsWith("-") && a !== "-" && !/^-?\d/.test(a)));
const args = argv.filter((a, i) => !flags.has(a) && argv[i - 1] !== "-o");
const option = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const [command, a1, a2] = args;
const need = (value, usage) => value ?? fail(`usage: qd ${usage}`);

switch (command) {
  case "open":
    await act("open", need(a1, "open <table|database.table|connection/database.table>"));
    break;
  case "seed":
    await seed();
    break;
  case "text":
    await text(a1, flags.has("-a"));
    break;
  case "bench":
    await bench(need(a1, "bench <query> [n]"), Number(a2 ?? 20));
    break;
  case "mem":
    mem();
    break;
  case "run":
    await run(need(a1, "run <file|->"));
    break;
  case "up":
    await up(flags.has("-f"));
    break;
  case "down":
    await down();
    break;
  case "ps":
    await ps();
    break;
  case "ui":
    await withApp(async (app) => console.log(await app.qd("ui", !flags.has("-a"))));
    break;
  case "find":
    await withApp(async (app) => {
      const found = await app.qd("find", need(a1, "find <text>"));
      // Like grep: nothing found is exit code 1, so a script can branch on it.
      if (found.startsWith("nothing matches")) fail(found);
      console.log(found);
    });
    break;
  case "click":
    await act("click", need(a1, "click <ref|name>"), flags.has("--right") ? "right" : flags.has("--double") ? "double" : "left");
    break;
  case "type":
    await act("type", need(a1, "type <ref|name> <text>"), need(a2, "type <ref|name> <text>"), flags.has("--append"));
    break;
  case "pick":
    await pick(need(a1, "pick <ref|name> <option>"), need(a2, "pick <ref|name> <option>"));
    break;
  case "key":
    await act("key", need(a1, "key <combo>"));
    break;
  case "wait":
    await wait(need(a1, "wait <text> [sec]"), Number(a2 ?? 30));
    break;
  case "shot":
    await shot(a1, option("-o"));
    break;
  case "state":
    await withApp(async (app) => console.log(await app.qd("state", a1 ?? "")));
    break;
  case "ev":
    await ev(flags.has("-a"));
    break;
  case "logs":
    logs(flags.has("-a"));
    break;
  case "perf":
    perf(a1 === "reset");
    break;
  case "sql":
    await sql(need(a1, "sql <query>"));
    break;
  case "exec":
    await exec(need(a1, "exec <js>"));
    break;
  default:
    console.log(HELP);
    if (command && command !== "help") fail(`unknown command: ${command}`);
}
}

await dispatch(process.argv.slice(2));
