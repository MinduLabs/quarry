/**
 * App state. Engine events are applied in `handleEvent`; components call the actions.
 * Keep engine calls (`send`) in actions, not in components.
 */
import { create } from "zustand";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { Event } from "@/bindings/Event";
import type { ObjectKind } from "@/bindings/ObjectKind";
import type { SchemaObject } from "@/bindings/SchemaObject";
import type { Value } from "@/bindings/Value";
import { send } from "@/lib/engine";
import { quoteIdent } from "@/lib/values";

export const BROWSE_LIMIT = 1000;

export type ConnStatus =
  | { kind: "idle" }
  | { kind: "connecting" }
  | { kind: "connected"; version: string }
  | { kind: "failed"; error: string };

export interface DbNode {
  name: string;
  expanded: boolean;
  loading: boolean;
  objects: SchemaObject[] | null;
  error: string | null;
}

export interface ConnNode {
  status: ConnStatus;
  expanded: boolean;
  databases: DbNode[];
}

export interface ResultSet {
  columns: ColumnMeta[] | null;
  rows: Value[][];
  affected: number;
  truncated: boolean;
}

export interface Tab {
  id: number;
  title: string;
  connId: string;
  database: string | null;
  sql: string;
  /** The user typed in this tab. Nothing is saved to disk yet, so edited SQL is lost on quit. */
  edited: boolean;
  running: boolean;
  sets: ResultSet[];
  activeSet: number;
  elapsedMs: number | null;
  error: string | null;
  /** Bumped on every run so views can reset scroll/selection. */
  runId: number;
}

export interface Toast {
  id: number;
  kind: "info" | "success" | "warning" | "error";
  message: string;
}

/** What would be lost by quitting now. */
export interface QuitInfo {
  running: number;
  unsaved: number;
}

/** Returns what quitting would lose, or null when it is safe to quit without asking. */
export function pendingWork(tabs: Tab[]): QuitInfo | null {
  const running = tabs.filter((t) => t.running).length;
  const unsaved = tabs.filter((t) => t.edited && t.sql.trim() !== "").length;
  return running + unsaved > 0 ? { running, unsaved } : null;
}

export interface PendingConfirm {
  tab: number;
  conn: string;
  database: string | null;
  sql: string;
  message: string;
}

export type DialogState =
  | { mode: "new" }
  | { mode: "edit"; id: string }
  | { mode: "delete"; id: string }
  | null;

interface State {
  ready: boolean;
  engineVersion: string | null;
  connections: ConnectionConfig[];
  nodes: Record<string, ConnNode>;
  /** Last connection/database the user worked with (target of "New query"). */
  active: { connId: string; database: string | null } | null;
  tabs: Tab[];
  activeTabId: number | null;
  dialog: DialogState;
  test: { request: number; pending: boolean; ok: boolean | null; message: string } | null;
  confirm: PendingConfirm | null;
  toasts: Toast[];

  handleEvent: (event: Event) => void;
  toast: (kind: Toast["kind"], message: string) => void;
  dismissToast: (id: number) => void;

  openDialog: (dialog: DialogState) => void;
  testConnection: (config: ConnectionConfig, password: string | null) => void;
  saveConnection: (config: ConnectionConfig, password: string | null, remember: boolean) => void;
  deleteConnection: (id: string) => void;

  toggleConnection: (id: string) => void;
  disconnect: (id: string) => void;
  toggleDatabase: (connId: string, database: string) => void;
  openObject: (connId: string, database: string, name: string, kind: ObjectKind) => void;

  newQuery: (target?: { connId: string; database: string | null }) => void;
  selectTab: (id: number) => void;
  closeTab: (id: number) => void;
  setSql: (id: number, sql: string) => void;
  run: (id: number, sql: string) => void;
  cancel: (id: number) => void;
  setActiveSet: (id: number, set: number) => void;
  answerConfirm: (run: boolean) => void;
  /** Sidebar visibility; remembered between launches. */
  sidebarOpen: boolean;
  toggleSidebar: () => void;
  quit: QuitInfo | null;
  askQuit: (info: QuitInfo) => void;
  dismissQuit: () => void;
}

const SIDEBAR_KEY = "quarry.sidebarOpen";

function loadSidebarOpen(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_KEY) !== "0";
  } catch {
    return true;
  }
}

let nextTabId = 1;
let nextQuery = 1;
let nextToast = 1;
let nextRequest = 1;

const emptyNode = (): ConnNode => ({ status: { kind: "idle" }, expanded: false, databases: [] });

export const useApp = create<State>()((set, get) => {
  /** Update one tab by id. */
  const patchTab = (id: number, patch: (tab: Tab) => Partial<Tab>) =>
    set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, ...patch(t) } : t)) }));

  /** Update one result set of a tab, creating empty sets up to `index`. */
  const patchSet = (id: number, index: number, patch: (rs: ResultSet) => Partial<ResultSet>) =>
    patchTab(id, (tab) => {
      const sets = [...tab.sets];
      while (sets.length <= index)
        sets.push({ columns: null, rows: [], affected: 0, truncated: false });
      const current = sets[index] as ResultSet;
      sets[index] = { ...current, ...patch(current) };
      return { sets };
    });

  const patchNode = (id: string, patch: (node: ConnNode) => Partial<ConnNode>) =>
    set((s) => {
      const node = s.nodes[id] ?? emptyNode();
      return { nodes: { ...s.nodes, [id]: { ...node, ...patch(node) } } };
    });

  const patchDb = (id: string, database: string, patch: (db: DbNode) => Partial<DbNode>) =>
    patchNode(id, (node) => ({
      databases: node.databases.map((db) => (db.name === database ? { ...db, ...patch(db) } : db)),
    }));

  const openTab = (connId: string, database: string | null, title: string, sql: string) => {
    const tab: Tab = {
      id: nextTabId++,
      title,
      connId,
      database,
      sql,
      edited: false,
      running: false,
      sets: [],
      activeSet: 0,
      elapsedMs: null,
      error: null,
      runId: 0,
    };
    set((s) => ({ tabs: [...s.tabs, tab], activeTabId: tab.id, active: { connId, database } }));
    return tab.id;
  };

  return {
    ready: false,
    engineVersion: null,
    connections: [],
    nodes: {},
    active: null,
    tabs: [],
    activeTabId: null,
    dialog: null,
    test: null,
    sidebarOpen: loadSidebarOpen(),
    toggleSidebar: () =>
      set((s) => {
        try {
          localStorage.setItem(SIDEBAR_KEY, s.sidebarOpen ? "0" : "1");
        } catch {
          // Not remembered; the toggle still works.
        }
        return { sidebarOpen: !s.sidebarOpen };
      }),
    quit: null,
    askQuit: (info) => set({ quit: info }),
    dismissQuit: () => set({ quit: null }),
    confirm: null,
    toasts: [],

    handleEvent: (event) => {
      switch (event.type) {
        case "Pong":
          set({ engineVersion: event.version });
          break;
        case "Notice":
          get().toast(
            event.kind === "Error" ? "error" : event.kind === "Warning" ? "warning" : "info",
            event.message,
          );
          break;
        case "Connections":
          set((s) => ({
            ready: true,
            connections: [...event.connections].sort((a, b) => a.name.localeCompare(b.name)),
            nodes: Object.fromEntries(
              event.connections.map((c) => [c.id, s.nodes[c.id] ?? emptyNode()]),
            ),
          }));
          break;
        case "TestResult": {
          const test = get().test;
          if (test?.request !== event.request) break;
          set({
            test: {
              request: event.request,
              pending: false,
              ok: event.error === null,
              message: event.error ?? `Connected · server ${event.server_version ?? ""}`,
            },
          });
          break;
        }
        case "ConnState": {
          const state = event.state;
          if (state.kind === "Failed") {
            const name = get().connections.find((c) => c.id === event.id)?.name ?? "";
            get().toast("error", `${name}: ${state.error}`);
          }
          patchNode(event.id, (node) => {
            switch (state.kind) {
              case "Connecting":
                return { status: { kind: "connecting" } };
              case "Connected":
                return {
                  status: { kind: "connected", version: state.server_version },
                  expanded: true,
                };
              case "Disconnected":
                return { status: { kind: "idle" }, databases: [], expanded: false };
              case "Failed":
                return { status: { kind: "failed", error: state.error }, expanded: node.expanded };
            }
          });
          break;
        }
        case "Databases": {
          const config = get().connections.find((c) => c.id === event.id);
          const preferred = config?.database ?? null;
          patchNode(event.id, () => ({
            databases: event.databases.map((name) => ({
              name,
              expanded: name === preferred,
              loading: name === preferred,
              objects: null,
              error: null,
            })),
          }));
          if (preferred && event.databases.includes(preferred)) {
            send({ type: "LoadObjects", id: event.id, database: preferred });
          }
          break;
        }
        case "Objects":
          patchDb(event.id, event.database, () => ({
            loading: false,
            objects: event.objects,
            error: event.error,
          }));
          break;
        case "ConfirmRequired":
          set({ confirm: { ...event } });
          break;
        case "QueryStarted":
          patchTab(event.tab, (t) => ({
            running: true,
            sets: [],
            activeSet: 0,
            elapsedMs: null,
            error: null,
            runId: t.runId + 1,
          }));
          break;
        case "ResultColumns":
          patchSet(event.tab, event.set, () => ({ columns: event.columns }));
          break;
        case "ResultRows":
          patchSet(event.tab, event.set, (rs) => ({ rows: rs.rows.concat(event.rows) }));
          break;
        case "ResultDone":
          patchSet(event.tab, event.set, () => ({
            affected: event.affected,
            truncated: event.truncated,
          }));
          break;
        case "QueryFinished":
          patchTab(event.tab, (t) => {
            // Show the last result set that has rows, like most SQL clients.
            const lastGrid = t.sets
              .map((s, i) => (s.columns ? i : -1))
              .filter((i) => i >= 0)
              .pop();
            return { running: false, elapsedMs: event.elapsed_ms, activeSet: lastGrid ?? 0 };
          });
          break;
        case "QueryFailed":
          patchTab(event.tab, () => ({
            running: false,
            error: event.cancelled ? "Query cancelled" : event.error,
          }));
          break;
      }
    },

    toast: (kind, message) => {
      const id = nextToast++;
      set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, message }] }));
      if (kind !== "error") setTimeout(() => get().dismissToast(id), 4000);
    },
    dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

    openDialog: (dialog) => set({ dialog, test: null }),
    testConnection: (config, password) => {
      const request = nextRequest++;
      set({ test: { request, pending: true, ok: null, message: "" } });
      send({ type: "TestConnection", request, config, password });
    },
    saveConnection: (config, password, remember) => {
      send({ type: "SaveConnection", config, password, remember_password: remember });
      set({ dialog: null, test: null });
    },
    deleteConnection: (id) => {
      send({ type: "DeleteConnection", id });
      set((s) => ({ dialog: null, tabs: s.tabs.filter((t) => t.connId !== id) }));
    },

    toggleConnection: (id) => {
      const node = get().nodes[id] ?? emptyNode();
      const config = get().connections.find((c) => c.id === id);
      set({ active: { connId: id, database: config?.database ?? null } });
      if (node.status.kind === "connected") {
        patchNode(id, (n) => ({ expanded: !n.expanded }));
      } else if (node.status.kind !== "connecting") {
        send({ type: "Connect", id });
      }
    },
    disconnect: (id) => send({ type: "Disconnect", id }),
    toggleDatabase: (connId, database) => {
      set({ active: { connId, database } });
      const db = get().nodes[connId]?.databases.find((d) => d.name === database);
      if (!db) return;
      const load = !db.expanded && db.objects === null && !db.loading;
      patchDb(connId, database, (d) => ({
        expanded: !d.expanded,
        loading: d.loading || load,
        error: null,
      }));
      if (load) send({ type: "LoadObjects", id: connId, database });
    },
    openObject: (connId, database, name, kind) => {
      const sql =
        kind === "function"
          ? `SHOW CREATE FUNCTION ${quoteIdent(name)};`
          : kind === "procedure"
            ? `SHOW CREATE PROCEDURE ${quoteIdent(name)};`
            : `SELECT * FROM ${quoteIdent(name)} LIMIT ${BROWSE_LIMIT};`;
      const existing = get().tabs.find(
        (t) => t.connId === connId && t.database === database && t.title === name,
      );
      if (existing) {
        set({ activeTabId: existing.id });
        return;
      }
      const id = openTab(connId, database, name, sql);
      get().run(id, sql);
    },

    newQuery: (target) => {
      const s = get();
      const connected = s.connections.find((c) => s.nodes[c.id]?.status.kind === "connected");
      const t =
        target ??
        s.active ??
        (connected ? { connId: connected.id, database: connected.database } : null);
      if (!t) {
        get().toast("info", "Connect to a database first");
        return;
      }
      openTab(t.connId, t.database, `Query ${nextQuery++}`, "");
    },
    selectTab: (id) => set({ activeTabId: id }),
    closeTab: (id) => {
      send({ type: "CloseTab", tab: id });
      set((s) => {
        const index = s.tabs.findIndex((t) => t.id === id);
        const tabs = s.tabs.filter((t) => t.id !== id);
        const neighbour = tabs[Math.min(index, tabs.length - 1)];
        return {
          tabs,
          activeTabId: s.activeTabId === id ? (neighbour?.id ?? null) : s.activeTabId,
        };
      });
    },
    setSql: (id, sql) => patchTab(id, () => ({ sql, edited: true })),
    run: (id, sql) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.running || !sql.trim()) return;
      send({
        type: "Run",
        tab: id,
        conn: tab.connId,
        database: tab.database,
        sql,
        confirmed: false,
      });
    },
    cancel: (id) => send({ type: "Cancel", tab: id }),
    setActiveSet: (id, index) => patchTab(id, () => ({ activeSet: index })),
    answerConfirm: (run) => {
      const pending = get().confirm;
      set({ confirm: null });
      if (!run || !pending) return;
      const { tab, conn, database, sql } = pending;
      send({ type: "Run", tab, conn, database, sql, confirmed: true });
    },
  };
});
