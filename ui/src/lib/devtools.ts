/**
 * Development bridge for `./qd` (docs/DEVTOOLS.md). `qd` evaluates these functions through
 * WebKit's remote inspector to read and drive the running app in few words: a text snapshot with
 * short refs instead of screenshots, actions by ref or name, the store, and the Command/Event
 * traffic. Imported only under `vite dev`; release builds never contain it.
 */
import type { Command } from "@/bindings/Command";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { Event } from "@/bindings/Event";
import { setDevTap } from "@/lib/engine";
import { displayValue } from "@/lib/values";
import { useApp } from "@/store/app";

const INTERACTIVE = [
  "button",
  "a[href]",
  "input",
  "textarea",
  "select",
  '[contenteditable="true"]',
  ...[
    "button",
    "tab",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "switch",
    "checkbox",
    "radio",
    "treeitem",
    "combobox",
    "link",
  ].map((role) => `[role="${role}"]`),
].join(",");
/** Text that is not interactive but says what is going on. */
const LANDMARK_TEXT = '[role="alert"], [role="status"]';
const NAME_MAX = 60;
const GRID_ROWS = 5;
const EVENT_LIMIT = 1000;
/** Fields that carry a secret (`Password` in a Command); their text is never printed. */
const SECRET = /password|secret|token/i;

// ---------- Refs ----------

const refOf = new WeakMap<Element, string>();
const byRef = new Map<string, WeakRef<Element>>();
let nextRef = 1;

function ref(el: Element): string {
  let id = refOf.get(el);
  if (!id) {
    id = `e${nextRef++}`;
    refOf.set(el, id);
    byRef.set(id, new WeakRef(el));
  }
  return id;
}

// ---------- Snapshot ----------

function clean(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > NAME_MAX ? `${flat.slice(0, NAME_MAX - 1)}…` : flat;
}

function isVisible(el: Element): boolean {
  if (el.closest('[aria-hidden="true"], [inert]')) return false;
  const rect = el.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}

/** The accessible name, the way a screen reader would mostly find it. */
function nameOf(el: Element): string {
  const label = el.getAttribute("aria-label");
  if (label) return clean(label);
  const labelledBy = el.getAttribute("aria-labelledby");
  if (labelledBy) {
    const text = labelledBy
      .split(" ")
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" ");
    if (text.trim()) return clean(text);
  }
  if (
    el instanceof HTMLInputElement ||
    el instanceof HTMLTextAreaElement ||
    el.getAttribute("role") === "combobox"
  ) {
    const field = el.closest("label")?.firstChild?.textContent;
    if (field?.trim()) return clean(field);
    if ((el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) && el.placeholder)
      return clean(el.placeholder);
  }
  const text = el instanceof HTMLElement ? el.innerText : (el.textContent ?? "");
  if (text.trim()) return clean(text);
  return clean(el.getAttribute("title") ?? "");
}

function roleOf(el: Element): string {
  const role = el.getAttribute("role");
  if (role) return role;
  if (el instanceof HTMLInputElement) return el.type === "checkbox" ? "checkbox" : "input";
  if (el instanceof HTMLTextAreaElement) return "textbox";
  if (el.getAttribute("contenteditable") === "true") return "editor";
  return el.tagName.toLowerCase();
}

function stateOf(el: Element): string {
  const parts: string[] = [];
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const isSecret = el instanceof HTMLInputElement && el.type === "password";
    if (el.value) parts.push(`= ${isSecret ? "•••" : JSON.stringify(clean(el.value))}`);
  }
  if (el.getAttribute("role") === "combobox" && el instanceof HTMLElement) {
    parts.push(`= ${JSON.stringify(clean(el.innerText))}`);
  }
  const attr = (name: string) => el.getAttribute(name);
  if ((el as HTMLButtonElement).disabled || attr("aria-disabled") === "true")
    parts.push("disabled");
  if (attr("aria-checked") === "true" || attr("data-state") === "checked") parts.push("on");
  if (attr("aria-selected") === "true" || attr("aria-current")) parts.push("selected");
  if (attr("aria-expanded") === "true") parts.push("open");
  if (attr("aria-invalid") === "true") parts.push("invalid");
  return parts.join(" ");
}

/** What the user can act on now: the open menu or dialog, else the whole window. */
function scopes(): Element[] {
  // A popup (a select's list, a context menu) floats outside any dialog and takes every click
  // until it closes. A list inside a dialog (the command palette's) is part of the dialog.
  const menus = [...document.querySelectorAll('[role="menu"], [role="listbox"]')].filter(
    (el) => isVisible(el) && !el.closest('[role="dialog"]'),
  );
  if (menus.length > 0) return menus;
  const dialogs = [...document.querySelectorAll('[role="dialog"], [role="alertdialog"]')];
  return [dialogs.at(-1) ?? document.body];
}

function gridLines(grid: Element): string[] {
  const header = [...grid.querySelectorAll('[role="columnheader"]')].map((h) => nameOf(h));
  const rows = [...grid.querySelectorAll('[role="row"]')].filter(
    (row) => row.querySelector('[role="gridcell"]') && isVisible(row),
  );
  const lines = [`grid · ${rows.length} rows shown · ${header.join(" | ")}`];
  for (const row of rows.slice(0, GRID_ROWS)) {
    const cells = [...row.querySelectorAll('[role="gridcell"]')].map(
      (cell) => (cell instanceof HTMLElement ? cell.innerText : "").trim() || "∅",
    );
    lines.push(`  ${ref(row)} ${cells.join(" | ")}`);
  }
  return lines;
}

function snapshotLines(): string[] {
  const lines: string[] = [];
  const seen = new Set<Element>();
  for (const scope of scopes()) {
    if (scope !== document.body) {
      const title = scope.querySelector("h2")?.textContent ?? "";
      lines.push(`${roleOf(scope)}${title ? ` "${clean(title)}"` : ""}`);
      // A dialog's description often holds its warning ("This can delete data").
      const describedBy = scope.getAttribute("aria-describedby");
      const description = describedBy && document.getElementById(describedBy)?.textContent;
      if (description?.trim()) lines.push(`text "${clean(description)}"`);
    }
    // Inside a dialog its sentences matter too ("Connect to a database first."); the page's
    // would be too many.
    const prose = scope === document.body ? "" : ", p";
    const nodes = scope.querySelectorAll(`${INTERACTIVE}, ${LANDMARK_TEXT}, [role="grid"]${prose}`);
    for (const el of nodes) {
      if (seen.has(el) || !isVisible(el)) continue;
      // A control inside another control (a chevron in a tree row) is reached through its parent.
      const parent = el.parentElement?.closest(INTERACTIVE);
      if (parent && parent !== el && scope.contains(parent) && !el.matches("input, textarea"))
        continue;
      seen.add(el);
      if (el.getAttribute("role") === "grid") {
        lines.push(...gridLines(el));
        continue;
      }
      if ((el.matches(LANDMARK_TEXT) || el.tagName === "P") && !el.matches(INTERACTIVE)) {
        const line = `text "${nameOf(el)}"`;
        // The dialog's description is already under its title.
        if (nameOf(el) && !lines.includes(line)) lines.push(line);
        continue;
      }
      const state = stateOf(el);
      lines.push(`${ref(el)} ${roleOf(el)} "${nameOf(el)}"${state ? ` ${state}` : ""}`);
    }
    if (scope !== document.body) lines.push("");
  }
  for (const toast of useApp.getState().toasts) {
    lines.push(`toast ${toast.kind} "${clean(toast.message)}"`);
  }
  return lines;
}

/** The last snapshot of each surface (the page, a dialog), so coming back to one shows only what
 * changed on it. */
const lastByScope = new Map<string, string[]>();
let lastScope = "";

/** Which surface is in front (the page, a dialog, a menu): a new one is shown whole. */
function scopeKey(): string {
  return scopes()
    .map((scope) =>
      scope === document.body
        ? "page"
        : `${roleOf(scope)}:${scope.querySelector("h2")?.textContent ?? ""}`,
    )
    .join("|");
}

/** The snapshot, or with `diff` only the lines that changed since the last one. */
function ui(diff: boolean): string {
  const lines = snapshotLines();
  const scope = scopeKey();
  const previous = lastByScope.get(scope);
  const isNewScope = scope !== lastScope;
  lastByScope.set(scope, lines);
  lastScope = scope;
  if (!diff || !previous) return lines.join("\n");
  const before = new Set(previous);
  const after = new Set(lines);
  const changes = [
    ...previous.filter((l) => l && !after.has(l)).map((l) => `- ${l}`),
    ...lines.filter((l) => l && !before.has(l)).map((l) => `+ ${l}`),
  ];
  // Back on a surface seen before (a dialog closed): say which, then what changed on it.
  const header = isNewScope ? [`→ ${scope.replace(":", " ")}`] : [];
  if (changes.length === 0) return [...header, "(no change)"].join("\n");
  // When most of the screen changed, the new screen is shorter than the list of changes.
  const body = changes.length > lines.length ? lines : changes;
  return [...header, ...body].join("\n");
}

function find(text: string): string {
  const needle = text.toLowerCase();
  const lines = snapshotLines().filter((l) => l.toLowerCase().includes(needle));
  return lines.length ? lines.join("\n") : `nothing matches "${text}"`;
}

// ---------- Actions ----------

/** The element a ref or a name points at, or why there is none. */
function resolve(target: string): Element | string {
  if (/^e\d+$/.test(target)) {
    const el = byRef.get(target)?.deref();
    return el?.isConnected ? el : `${target} is gone; run qd ui again`;
  }
  const needle = target.toLowerCase();
  const candidates = scopes().flatMap((scope) =>
    [...scope.querySelectorAll(INTERACTIVE)].filter(isVisible),
  );
  const named = (test: (name: string) => boolean) =>
    candidates.filter((el) => test(nameOf(el).toLowerCase()));
  // The closest kind of match wins: the whole name, then its start, then anywhere in it.
  const exact = named((name) => name === needle);
  const prefix = named((name) => name.startsWith(needle));
  const loose = exact.length
    ? exact
    : prefix.length
      ? prefix
      : named((name) => name.includes(needle));
  const [first, ...rest] = loose;
  if (!first) return `nothing named "${target}"`;
  if (rest.length > 0 && exact.length !== 1) {
    const list = loose.slice(0, 5).map((el) => `${ref(el)} ${roleOf(el)} "${nameOf(el)}"`);
    return `"${target}" matches ${loose.length}; use a ref:\n${list.join("\n")}`;
  }
  return first;
}

function point(el: Element): { clientX: number; clientY: number } {
  const before = el.getBoundingClientRect();
  const isOnScreen =
    before.top >= 0 &&
    before.left >= 0 &&
    before.bottom <= innerHeight &&
    before.right <= innerWidth;
  if (!isOnScreen) {
    el.scrollIntoView({ block: "nearest", inline: "nearest" });
    // The window itself never scrolls in the app; only the element's own lists may.
    document.documentElement.scrollTop = 0;
    document.body.scrollTop = 0;
  }
  const rect = el.getBoundingClientRect();
  return { clientX: rect.left + rect.width / 2, clientY: rect.top + rect.height / 2 };
}

type ClickKind = "left" | "right" | "double";

function click(target: string, kind: ClickKind): string {
  const el = resolve(target);
  if (typeof el === "string") return el;
  const at = { bubbles: true, cancelable: true, composed: true, ...point(el) };
  if (kind === "right") {
    el.dispatchEvent(new MouseEvent("contextmenu", { ...at, button: 2, buttons: 2 }));
    return "ok";
  }
  const pointer = { ...at, button: 0, pointerId: 1, pointerType: "mouse", isPrimary: true };
  el.dispatchEvent(new PointerEvent("pointerdown", { ...pointer, buttons: 1 }));
  el.dispatchEvent(new MouseEvent("mousedown", { ...at, button: 0, buttons: 1 }));
  el.dispatchEvent(new PointerEvent("pointerup", pointer));
  el.dispatchEvent(new MouseEvent("mouseup", { ...at, button: 0 }));
  el.dispatchEvent(new MouseEvent("click", { ...at, button: 0, detail: 1 }));
  if (kind === "double") el.dispatchEvent(new MouseEvent("dblclick", { ...at, detail: 2 }));
  return "ok";
}

function type(target: string, text: string, append: boolean): string {
  const el = resolve(target);
  if (typeof el === "string") return el;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    el.focus();
    const proto = el instanceof HTMLInputElement ? HTMLInputElement : HTMLTextAreaElement;
    const setValue = Object.getOwnPropertyDescriptor(proto.prototype, "value")?.set;
    setValue?.call(el, append ? el.value + text : text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
    return "ok";
  }
  // The SQL editor (CodeMirror) and other editable text take input as typed text.
  const editable =
    el.closest('[contenteditable="true"]') ?? el.querySelector('[contenteditable="true"]');
  if (!(editable instanceof HTMLElement)) return `${target} does not take text`;
  editable.focus();
  if (!append) document.execCommand("selectAll");
  document.execCommand("insertText", false, text);
  return "ok";
}

/** Choose an option of the open select or menu by name (`qd pick` opens it first). */
function choose(option: string): string {
  const needle = option.toLowerCase();
  const options = [
    ...document.querySelectorAll('[role="option"], [role="menuitem"], [role="menuitemradio"]'),
  ].filter(isVisible);
  if (options.length === 0) return "nothing is open to choose from";
  const match =
    options.find((o) => nameOf(o).toLowerCase() === needle) ??
    options.find((o) => nameOf(o).toLowerCase().startsWith(needle)) ??
    options.find((o) => nameOf(o).toLowerCase().includes(needle));
  if (!match) return `no option "${option}"; options: ${options.map(nameOf).join(", ")}`;
  // Radix Select drops the first pointer-up after it opens when the pointer has not moved, so a
  // press on the trigger does not choose at once. Spend that one before pressing the option.
  document.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "mouse" }));
  return click(ref(match), "left");
}

const KEY_NAMES: Record<string, string> = {
  enter: "Enter",
  esc: "Escape",
  escape: "Escape",
  tab: "Tab",
  space: " ",
  backspace: "Backspace",
  delete: "Delete",
  up: "ArrowUp",
  down: "ArrowDown",
  left: "ArrowLeft",
  right: "ArrowRight",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
};

/** `ctrl+shift+p`, `enter`, `f5`. `mod` is Ctrl here (Linux), ⌘ on macOS. */
function key(combo: string): string {
  const parts = combo.toLowerCase().split("+");
  const name = parts.pop() ?? "";
  const has = (m: string) => parts.includes(m);
  const isMac = navigator.platform.toLowerCase().includes("mac");
  const keyName = KEY_NAMES[name] ?? (/^f\d{1,2}$/.test(name) ? name.toUpperCase() : name);
  const init: KeyboardEventInit = {
    key: keyName,
    bubbles: true,
    cancelable: true,
    ctrlKey: has("ctrl") || (has("mod") && !isMac),
    metaKey: has("meta") || has("cmd") || (has("mod") && isMac),
    shiftKey: has("shift"),
    altKey: has("alt"),
  };
  const target = document.activeElement ?? document.body;
  target.dispatchEvent(new KeyboardEvent("keydown", init));
  target.dispatchEvent(new KeyboardEvent("keyup", init));
  return "ok";
}

/** Changes whenever something the user would notice changes; `qd` waits until it holds still. */
function fingerprint(): string {
  const s = useApp.getState();
  return [
    document.body.innerText.length,
    document.querySelectorAll('[role="dialog"], [role="menu"], [role="listbox"]').length,
    s.tabs.filter((t) => t.running).length,
    Object.values(s.driverInstalls)
      .map((d) => d.kind)
      .join(),
    Object.values(s.nodes)
      .map((n) => n.status.kind)
      .join(),
    s.toasts.length,
  ].join("|");
}

function has(text: string): boolean {
  return (
    document.body.innerText.includes(text) ||
    useApp.getState().toasts.some((t) => t.message.includes(text))
  );
}

/** The smallest element showing `text`, as one line, so `qd wait` can say what it found. */
function textLine(text: string): string | null {
  const toast = useApp.getState().toasts.find((t) => t.message.includes(text));
  if (toast) return `toast ${toast.kind} "${clean(toast.message)}"`;
  let best: HTMLElement | null = null;
  for (const el of document.body.querySelectorAll<HTMLElement>("*")) {
    if (el.innerText?.includes(text) && isVisible(el)) best = el;
  }
  return best ? clean(best.innerText) : null;
}

function rect(target: string): number[] | string {
  const el = resolve(target);
  if (typeof el === "string") return el;
  const r = el.getBoundingClientRect();
  return [r.left, r.top, r.width, r.height].map(Math.round);
}

// ---------- State ----------

function compact(value: unknown, depth = 0): unknown {
  if (typeof value === "function") return undefined;
  if (typeof value === "string") return value.length > 200 ? `${value.slice(0, 199)}…` : value;
  if (value === null || typeof value !== "object") return value;
  if (depth >= 5) return Array.isArray(value) ? `[${value.length}]` : "{…}";
  if (Array.isArray(value)) {
    const shown = value.slice(0, 20).map((v) => compact(v, depth + 1));
    return value.length > 20 ? [...shown, `… ${value.length - 20} more`] : shown;
  }
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(value)) {
    const c = SECRET.test(k) && typeof v === "string" && v ? "***" : compact(v, depth + 1);
    if (c !== undefined) out[k] = c;
  }
  return out;
}

/** A part of the store by dotted path (`dialog`, `nodes`, `tabs.0.browse`), as short JSON. */
function state(path: string): string {
  let value: unknown = useApp.getState();
  for (const part of path.split(".").filter(Boolean)) {
    if (value === null || typeof value !== "object") return `no "${part}" in ${path}`;
    value = (value as Record<string, unknown>)[part];
  }
  return JSON.stringify(compact(value)) ?? "undefined";
}

function ps(): string {
  const s = useApp.getState();
  const lines: string[] = [];
  for (const c of s.connections) {
    const status = s.nodes[c.id]?.status;
    const error = status?.kind === "failed" ? ` · ${clean(status.error)}` : "";
    lines.push(`conn "${c.name}" ${c.driver} ${c.environment} · ${status?.kind ?? "idle"}${error}`);
  }
  const active = s.tabs.find((t) => t.id === s.activeTabId);
  lines.push(
    `tabs ${s.tabs.length}${active ? ` · active "${active.title}"${active.running ? " running" : ""}` : ""}`,
  );
  const dialog = document.querySelector('[role="dialog"] h2')?.textContent;
  if (dialog) lines.push(`dialog "${clean(dialog)}"`);
  for (const [driver, install] of Object.entries(s.driverInstalls)) {
    lines.push(`driver ${driver} ${install.kind}`);
  }
  for (const t of s.toasts) lines.push(`toast ${t.kind} "${clean(t.message)}"`);
  return lines.join("\n");
}

// ---------- Traffic ----------

interface Traffic {
  seq: number;
  line: string;
}
const traffic: Traffic[] = [];
let seq = 0;

/** One message in a line: arrays become their length, long strings are cut, secrets hidden. */
function summarize(message: Command | Event): string {
  const short = (value: unknown, depth: number): unknown => {
    if (Array.isArray(value))
      return value.length > 3 ? `[${value.length}]` : value.map((v) => short(v, depth + 1));
    if (typeof value === "string") return value.length > 60 ? `${value.slice(0, 59)}…` : value;
    if (value === null || typeof value !== "object") return value;
    if (depth >= 2) return "{…}";
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value)) {
      if (k === "type") continue;
      out[k] = SECRET.test(k) && typeof v === "string" ? "***" : short(v, depth + 1);
    }
    return out;
  };
  return `${message.type} ${JSON.stringify(short(message, 0))}`;
}

setDevTap((direction, message) => {
  if (message.type === "Ping" || message.type === "Pong" || message.type === "PerfMarks") return;
  traffic.push({ seq: ++seq, line: `${direction === "out" ? "→" : "←"} ${summarize(message)}` });
  if (traffic.length > EVENT_LIMIT) traffic.shift();
});

function events(since: number): { next: number; lines: string[] } {
  return { next: seq, lines: traffic.filter((t) => t.seq > since).map((t) => t.line) };
}

// ---------- SQL ----------

/** Run `sql` in a new query tab of the active connection; returns the tab, or why not. */
function sqlStart(sql: string): number | string {
  const s = useApp.getState();
  if (!s.active) return "no active connection; connect first (qd click the connection)";
  s.newQuery(s.active);
  const id = useApp.getState().activeTabId;
  if (id === null) return "no tab opened";
  s.setSql(id, sql);
  s.run(id, sql);
  return id;
}

function sqlResult(id: number): string | null {
  const tab = useApp.getState().tabs.find((t) => t.id === id);
  if (!tab) return "tab is gone";
  if (tab.running) return null;
  if (tab.error) return `error: ${tab.error}`;
  const lines = [`${tab.elapsedMs ?? "?"} ms`];
  for (const set of tab.sets.slice(0, 3)) {
    if (!set.columns) {
      lines.push(`${set.affected} rows affected`);
      continue;
    }
    lines.push(
      `${set.rows.length} rows${set.truncated ? " (truncated)" : ""} · ${set.columns.map((c) => c.name).join(" | ")}`,
    );
    for (const row of set.rows.slice(0, 10)) lines.push(`  ${row.map(displayValue).join(" | ")}`);
  }
  return lines.join("\n");
}

// ---------- Sandbox and measuring ----------

/** Run `action` once, as soon as `isReady` holds for the store. */
function once(isReady: () => boolean, action: () => void): void {
  if (isReady()) {
    action();
    return;
  }
  const stop = useApp.subscribe(() => {
    if (!isReady()) return;
    stop();
    action();
  });
}

/**
 * Save a test connection (its password kept in memory only), download its driver when needed,
 * then connect. Commands run concurrently in the engine, so each step waits for the store to show
 * the one before it.
 */
function seed(config: ConnectionConfig, password: string): string {
  const s = useApp.getState();
  // Saving a connection closes its sessions: one that is connected is left as it is.
  if (s.nodes[config.id]?.status.kind === "connected") return "ok";
  const before = s.connections;
  s.saveConnection(config, password, null, false);
  // The engine's answer to this save is a new list. Connecting before it would race the save,
  // which closes the connection's sessions when it lands.
  const isSaved = () => {
    const now = useApp.getState().connections;
    return now !== before && now.some((c) => c.id === config.id);
  };
  const isInstalled = () =>
    useApp.getState().dataSources.some((d) => d.driver === config.driver && d.is_installed);
  once(isSaved, () => {
    if (!isInstalled()) useApp.getState().installDriver(config.driver);
    once(isInstalled, () => {
      const status = useApp.getState().nodes[config.id]?.status.kind;
      if (status !== "connected" && status !== "connecting") {
        useApp.getState().toggleConnection(config.id);
      }
    });
  });
  return "ok";
}

/** `connected`, `connecting`, `idle`, `downloading the driver`, or `failed: <why>`. */
function connState(id: string): string {
  const s = useApp.getState();
  const config = s.connections.find((c) => c.id === id);
  const install = config ? s.driverInstalls[config.driver] : undefined;
  if (install?.kind === "Failed") return `failed: ${clean(install.error)}`;
  if (install && install.kind !== "Installed") return "downloading the driver";
  const status = s.nodes[id]?.status;
  if (status?.kind === "failed") return `failed: ${clean(status.error)}`;
  return status?.kind ?? "idle";
}

const TEXT_LINES = 60;

/** The readable text of what is in front (a dialog, else the window), or its lines with `word`. */
function text(word: string, all: boolean): string | null {
  const [scope] = scopes();
  const raw = scope instanceof HTMLElement ? scope.innerText : "";
  const lines: string[] = [];
  for (const line of raw.split("\n").map((l) => l.trim())) {
    if (line && line !== lines.at(-1)) lines.push(line);
  }
  if (word) {
    const needle = word.toLowerCase();
    const found = lines.filter((l) => l.toLowerCase().includes(needle));
    return found.length ? found.join("\n") : null;
  }
  if (all || lines.length <= TEXT_LINES) return lines.join("\n");
  return `${lines.slice(0, TEXT_LINES).join("\n")}\n… ${lines.length - TEXT_LINES} more lines (-a)`;
}

function tabRun(id: number): {
  running: boolean;
  elapsed: number;
  error: string | null;
  rows: number;
} {
  const tab = useApp.getState().tabs.find((t) => t.id === id);
  if (!tab) return { running: false, elapsed: 0, error: "the tab is gone", rows: 0 };
  // The engine's own timing of the statement, to the microsecond; the tab's is whole ms.
  const last = tab.log.at(-1);
  return {
    running: tab.running,
    elapsed: last ? last.execute_ms + (last.fetch_ms ?? 0) : (tab.elapsedMs ?? 0),
    error: tab.error,
    rows: tab.sets[0]?.rows.length ?? 0,
  };
}

/** Run a query tab's SQL again. */
function rerun(id: number): string {
  const tab = useApp.getState().tabs.find((t) => t.id === id);
  if (!tab) return "the tab is gone";
  if (tab.running) return "the tab is still running";
  useApp.getState().run(id, tab.sql);
  return "ok";
}

/** Open a table of the active connection's database, wherever the tree is scrolled or folded.
 * `connection/database.table` and `database.table` pick another one. */
function open(target: string): string {
  const s = useApp.getState();
  const [connName, rest] = target.includes("/") ? target.split("/", 2) : [null, target];
  const conn = connName
    ? s.connections.find((c) => c.name.toLowerCase() === connName.toLowerCase())
    : s.connections.find((c) => c.id === s.active?.connId);
  if (!conn)
    return connName ? `no connection "${connName}"` : "no active connection; connect first";
  if (s.nodes[conn.id]?.status.kind !== "connected") return `"${conn.name}" is not connected`;
  const dot = (rest ?? "").lastIndexOf(".");
  const database =
    dot >= 0
      ? (rest ?? "").slice(0, dot)
      : ((s.active?.connId === conn.id ? s.active.database : null) ?? conn.database);
  const name = dot >= 0 ? (rest ?? "").slice(dot + 1) : (rest ?? "");
  const listed = s.nodes[conn.id]?.databases.map((d) => d.name) ?? [];
  if (!database || !listed.includes(database)) {
    return `say which database (or schema): qd open "${conn.name}/<one of ${listed.join(", ")}>.${name}"`;
  }
  s.openObject(conn.id, database, name, "table");
  return "ok";
}

const bridge = {
  open,
  seed,
  connState,
  text,
  tabRun,
  rerun,
  ui,
  find,
  click,
  type,
  choose,
  key,
  fingerprint,
  has,
  textLine,
  rect,
  state,
  ps,
  events,
  sqlStart,
  sqlResult,
};

declare global {
  interface Window {
    __qd?: typeof bridge;
  }
}

window.__qd = bridge;
