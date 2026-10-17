/**
 * App state. Engine events are applied in `handleEvent`; components call the actions.
 * Keep engine calls (`send`) in actions, not in components.
 */
import { create } from "zustand";
import type { BrowsePage } from "@/bindings/BrowsePage";
import type { CellValue } from "@/bindings/CellValue";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { Event } from "@/bindings/Event";
import type { Favorite } from "@/bindings/Favorite";
import type { HistoryEntry } from "@/bindings/HistoryEntry";
import type { ObjectKind } from "@/bindings/ObjectKind";
import type { RowChange } from "@/bindings/RowChange";
import type { SchemaObject } from "@/bindings/SchemaObject";
import type { TableInfo } from "@/bindings/TableInfo";
import type { Value } from "@/bindings/Value";
import type { Workspace } from "@/bindings/Workspace";
import { send } from "@/lib/engine";
import { quoteIdent } from "@/lib/values";

/** Rows per page in a browse tab. */
export const PAGE_SIZE = 1000;

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

/** Structure of the table a browse tab shows, loaded the first time the Structure view opens. */
export interface TabStructure {
  loading: boolean;
  info: TableInfo | null;
  error: string | null;
}

export type TabView = "data" | "structure";

/** Rows of history shown in the sidebar. */
export const HISTORY_LIMIT = 200;

export interface HistoryState {
  entries: HistoryEntry[];
  loaded: boolean;
  query: string;
}

/** Grid edits not yet saved. Row and column numbers index the shown page. */
export interface Edits {
  /** row → column → new value */
  updates: Record<number, Record<number, Value>>;
  /** New rows: column → value; a missing column gets its default. */
  inserts: Array<Record<number, Value>>;
  deletes: number[];
}

export const noEdits = (): Edits => ({ updates: {}, inserts: [], deletes: [] });

export function editCount(edits: Edits | null): number {
  if (!edits) return 0;
  return Object.keys(edits.updates).length + edits.inserts.length + edits.deletes.length;
}

/**
 * Column positions of the primary key when the shown page can be edited: a page of a table (not
 * a typed query) with a primary key whose columns are all in the result, on a writable
 * connection. `null` otherwise.
 */
export function editableKey(tab: Tab, readOnly: boolean): number[] | null {
  const set = tab.sets[tab.activeSet];
  const key = tab.structure?.info?.primary_key ?? [];
  if (readOnly || !tab.browse || !tab.paged || !set?.columns || key.length === 0) return null;
  const names = set.columns.map((c) => c.source_column ?? c.name);
  const positions = key.map((k) => names.indexOf(k));
  return positions.every((p) => p >= 0) ? positions : null;
}

export interface Tab {
  id: number;
  title: string;
  connId: string;
  database: string | null;
  /** The editor text. Saved with the workspace, so it survives a restart. */
  sql: string;
  /** Set for a table opened from the sidebar. */
  browse: BrowsePage | null;
  view: TabView;
  structure: TabStructure | null;
  /** The shown results are a page of `browse` (not a typed query), so the pager applies. */
  paged: boolean;
  edits: Edits | null;
  saving: boolean;
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

/** What would be lost by quitting now. Tabs and their SQL are saved; running queries and
 * unsaved grid edits are not. */
export interface QuitInfo {
  running: number;
  edits: number;
}

/** Returns what quitting would lose, or null when it is safe to quit without asking. */
export function pendingWork(tabs: Tab[]): QuitInfo | null {
  const running = tabs.filter((t) => t.running).length;
  const edits = tabs.filter((t) => editCount(t.edits) > 0).length;
  return running + edits > 0 ? { running, edits } : null;
}

export interface PendingChangesConfirm {
  tab: number;
  message: string;
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
  changesConfirm: PendingChangesConfirm | null;
  history: HistoryState;
  favorites: Favorite[];
  toasts: Toast[];

  handleEvent: (event: Event) => void;
  toast: (kind: Toast["kind"], message: string) => void;
  dismissToast: (id: number) => void;

  openDialog: (dialog: DialogState) => void;
  testConnection: (
    config: ConnectionConfig,
    password: string | null,
    sshSecret: string | null,
  ) => void;
  saveConnection: (
    config: ConnectionConfig,
    password: string | null,
    sshSecret: string | null,
    remember: boolean,
  ) => void;
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
  /** Load the page of a browse tab that starts at row `offset`. */
  browse: (id: number, offset: number) => void;
  cancel: (id: number) => void;
  setActiveSet: (id: number, set: number) => void;
  /** Data or Structure, for browse tabs. */
  setView: (id: number, view: TabView) => void;
  answerConfirm: (run: boolean) => void;

  editCell: (id: number, row: number, col: number, value: Value) => void;
  addRow: (id: number) => void;
  /** Mark a loaded row for deletion, or unmark it; removes a new row outright. */
  toggleDeleteRow: (id: number, row: number) => void;
  discardEdits: (id: number) => void;
  saveEdits: (id: number, confirmed?: boolean) => void;
  answerChangesConfirm: (save: boolean) => void;

  addTableFavorite: (connId: string, database: string, table: string) => void;
  saveQueryFavorite: (tabId: number, name: string) => void;
  deleteFavorite: (id: string) => void;
  openFavorite: (favorite: Favorite) => void;

  loadHistory: (query: string) => void;
  clearHistory: () => void;
  /** Open `sql` in a new tab on that connection and database (history, favorites). */
  openSql: (connId: string, database: string | null, sql: string, title?: string) => void;
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

/** Only the newest history request is shown, so typing fast never shows stale results. */
let historyRequest = 0;

/** Table structure requests waiting for `TableInfo`: request id → tab id. */
const structureRequests = new Map<number, number>();

/** Workspace writes waiting for `WorkspaceSaved`, by request id. */
const pendingSaves = new Map<number, () => void>();
/** False until the saved workspace is restored, so an empty start never overwrites it. */
let workspaceRestored = false;
let lastSaved = "";
let saveTimer: ReturnType<typeof setTimeout> | undefined;

function workspaceOf(tabs: Tab[], activeTabId: number | null): Workspace {
  const active = tabs.findIndex((t) => t.id === activeTabId);
  return {
    tabs: tabs.map((t) => ({
      title: t.title,
      conn: t.connId,
      database: t.database,
      sql: t.sql,
      browse: t.browse,
    })),
    active: active >= 0 ? active : null,
  };
}

/**
 * Write the open tabs now. Resolves once the engine has written them, or after 2 s, so quitting
 * never hangs on a slow disk.
 */
export function saveWorkspace(): Promise<void> {
  clearTimeout(saveTimer);
  if (!workspaceRestored) return Promise.resolve();
  const { tabs, activeTabId } = useApp.getState();
  const workspace = workspaceOf(tabs, activeTabId);
  const text = JSON.stringify(workspace);
  if (text === lastSaved) return Promise.resolve();
  lastSaved = text;
  const request = nextRequest++;
  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      pendingSaves.delete(request);
      resolve();
    }, 2000);
    pendingSaves.set(request, () => {
      clearTimeout(timeout);
      resolve();
    });
    send({ type: "SaveWorkspace", request, workspace });
  });
}

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

  /** Ask the engine for the structure of a browse tab's table (`TableInfo` fills it in). */
  const loadStructure = (id: number) => {
    const tab = get().tabs.find((t) => t.id === id);
    if (!tab?.browse) return;
    patchTab(id, () => ({ structure: { loading: true, info: null, error: null } }));
    const request = nextRequest++;
    structureRequests.set(request, id);
    send({ type: "LoadTableInfo", request, conn: tab.connId, table: tab.browse.table });
  };

  const openTab = (
    connId: string,
    database: string | null,
    title: string,
    sql: string,
    browse: BrowsePage | null = null,
  ) => {
    const tab: Tab = {
      id: nextTabId++,
      title,
      connId,
      database,
      sql,
      browse,
      view: "data",
      structure: null,
      paged: false,
      edits: null,
      saving: false,
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
    changesConfirm: null,
    history: { entries: [], loaded: false, query: "" },
    favorites: [],
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
          // The saved tabs point at connections, so restore them once the list is known.
          if (!get().ready) {
            send({ type: "LoadWorkspace" });
            send({ type: "LoadFavorites" });
          }
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
        case "Workspace": {
          if (workspaceRestored) break;
          const known = new Set(get().connections.map((c) => c.id));
          const restored: Tab[] = event.workspace.tabs
            .filter((t) => known.has(t.conn))
            .map((t) => ({
              id: nextTabId++,
              title: t.title,
              connId: t.conn,
              database: t.database,
              sql: t.sql,
              browse: t.browse,
              view: "data" as const,
              structure: null,
              paged: false,
              edits: null,
              saving: false,
              running: false,
              sets: [],
              activeSet: 0,
              elapsedMs: null,
              error: null,
              runId: 0,
            }));
          for (const t of restored) {
            const n = /^Query (\d+)$/.exec(t.title)?.[1];
            if (n) nextQuery = Math.max(nextQuery, Number(n) + 1);
          }
          const active = restored[event.workspace.active ?? -1] ?? restored[0];
          set((s) => ({
            tabs: [...restored, ...s.tabs],
            activeTabId: s.activeTabId ?? active?.id ?? null,
          }));
          workspaceRestored = true;
          lastSaved = JSON.stringify(workspaceOf(get().tabs, get().activeTabId));
          break;
        }
        case "TableInfo": {
          const tab = structureRequests.get(event.request);
          structureRequests.delete(event.request);
          if (tab === undefined) break;
          patchTab(tab, () => ({
            structure: { loading: false, info: event.info, error: event.error },
          }));
          break;
        }
        case "ChangesConfirmRequired":
          set({ changesConfirm: { tab: event.tab, message: event.message } });
          break;
        case "ChangesApplied": {
          patchTab(event.tab, () => ({ edits: null, saving: false }));
          get().toast("success", event.message);
          const tab = get().tabs.find((t) => t.id === event.tab);
          if (tab?.browse) get().browse(tab.id, tab.browse.offset);
          break;
        }
        case "ChangesFailed":
          patchTab(event.tab, () => ({ saving: false }));
          get().toast("error", event.error);
          break;
        case "Favorites":
          set({
            favorites: [...event.favorites].sort((a, b) => a.name.localeCompare(b.name)),
          });
          break;
        case "History":
          if (event.request !== historyRequest) break;
          set((s) => ({ history: { ...s.history, entries: event.entries, loaded: true } }));
          break;
        case "HistoryAdded":
          set((s) => {
            const { history } = s;
            const q = history.query.trim().toLowerCase();
            if (!history.loaded || (q && !event.entry.sql.toLowerCase().includes(q))) return {};
            const entries = [event.entry, ...history.entries].slice(0, HISTORY_LIMIT);
            return { history: { ...history, entries } };
          });
          break;
        case "WorkspaceSaved":
          if (event.error) get().toast("warning", `Couldn't save open tabs: ${event.error}`);
          pendingSaves.get(event.request)?.();
          pendingSaves.delete(event.request);
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
    testConnection: (config, password, sshSecret) => {
      const request = nextRequest++;
      set({ test: { request, pending: true, ok: null, message: "" } });
      send({ type: "TestConnection", request, config, password, ssh_secret: sshSecret });
    },
    saveConnection: (config, password, sshSecret, remember) => {
      send({
        type: "SaveConnection",
        config,
        password,
        ssh_secret: sshSecret,
        remember_password: remember,
      });
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
      const existing = get().tabs.find(
        (t) => t.connId === connId && t.database === database && t.title === name,
      );
      if (existing) {
        set({ activeTabId: existing.id });
        return;
      }
      if (kind === "table" || kind === "view") {
        const browse = { table: { database, schema: null, name }, offset: 0 };
        const id = openTab(connId, database, name, "", browse);
        get().browse(id, 0);
        return;
      }
      const sql =
        kind === "function"
          ? `SHOW CREATE FUNCTION ${quoteIdent(name)};`
          : `SHOW CREATE PROCEDURE ${quoteIdent(name)};`;
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
    setSql: (id, sql) => patchTab(id, () => ({ sql })),
    run: (id, sql) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.running || !sql.trim()) return;
      if (editCount(tab.edits) > 0) {
        get().toast("info", "Save or discard your changes first");
        return;
      }
      patchTab(id, () => ({ paged: false, edits: null }));
      send({
        type: "Run",
        tab: id,
        conn: tab.connId,
        database: tab.database,
        sql,
        confirmed: false,
      });
    },
    browse: (id, offset) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab?.browse || tab.running) return;
      if (editCount(tab.edits) > 0) {
        get().toast("info", "Save or discard your changes first");
        return;
      }
      const browse = { ...tab.browse, offset: Math.max(0, offset) };
      patchTab(id, () => ({ browse, paged: true, view: "data", edits: null }));
      // The primary key decides whether the page can be edited.
      if (tab.structure === null) loadStructure(id);
      send({
        type: "Browse",
        tab: id,
        conn: tab.connId,
        table: browse.table,
        limit: PAGE_SIZE,
        offset: browse.offset,
      });
    },
    cancel: (id) => send({ type: "Cancel", tab: id }),
    setActiveSet: (id, index) => patchTab(id, () => ({ activeSet: index })),
    setView: (id, view) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab?.browse) return;
      patchTab(id, () => ({ view }));
      if (view === "structure" && (tab.structure === null || tab.structure.error !== null)) {
        loadStructure(id);
      }
    },
    answerConfirm: (run) => {
      const pending = get().confirm;
      set({ confirm: null });
      if (!run || !pending) return;
      const { tab, conn, database, sql } = pending;
      send({ type: "Run", tab, conn, database, sql, confirmed: true });
    },

    addTableFavorite: (connId, database, table) => {
      const exists = get().favorites.some(
        (f) =>
          f.conn === connId &&
          f.database === database &&
          f.item.kind === "table" &&
          f.item.table === table,
      );
      if (exists) {
        get().toast("info", `${table} is already a favorite`);
        return;
      }
      send({
        type: "SaveFavorite",
        favorite: {
          id: crypto.randomUUID(),
          name: table,
          conn: connId,
          database,
          item: { kind: "table", table },
        },
      });
      get().toast("success", `Added ${table} to favorites`);
    },
    saveQueryFavorite: (tabId, name) => {
      const tab = get().tabs.find((t) => t.id === tabId);
      if (!tab || !name.trim()) return;
      send({
        type: "SaveFavorite",
        favorite: {
          id: crypto.randomUUID(),
          name: name.trim(),
          conn: tab.connId,
          database: tab.database,
          item: { kind: "query", sql: tab.sql },
        },
      });
      get().toast("success", `Saved “${name.trim()}” to favorites`);
    },
    deleteFavorite: (id) => send({ type: "DeleteFavorite", id }),
    openFavorite: (favorite) => {
      if (!get().connections.some((c) => c.id === favorite.conn)) {
        get().toast("info", "That connection no longer exists");
        return;
      }
      if (favorite.item.kind === "table") {
        get().openObject(favorite.conn, favorite.database ?? "", favorite.item.table, "table");
      } else {
        get().openSql(favorite.conn, favorite.database, favorite.item.sql, favorite.name);
      }
    },

    loadHistory: (query) => {
      historyRequest = nextRequest++;
      set((s) => ({ history: { ...s.history, query } }));
      send({
        type: "LoadHistory",
        request: historyRequest,
        query: query.trim() || null,
        limit: HISTORY_LIMIT,
      });
    },
    clearHistory: () => {
      historyRequest = nextRequest++;
      send({ type: "ClearHistory", request: historyRequest });
    },
    openSql: (connId, database, sql, title) => {
      openTab(connId, database, title ?? `Query ${nextQuery++}`, sql);
    },

    editCell: (id, row, col, value) =>
      patchTab(id, (tab) => {
        const edits = tab.edits ?? noEdits();
        const loaded = tab.sets[tab.activeSet]?.rows.length ?? 0;
        if (row >= loaded) {
          const inserts = [...edits.inserts];
          inserts[row - loaded] = { ...inserts[row - loaded], [col]: value };
          return { edits: { ...edits, inserts } };
        }
        return {
          edits: {
            ...edits,
            updates: { ...edits.updates, [row]: { ...edits.updates[row], [col]: value } },
          },
        };
      }),
    addRow: (id) =>
      patchTab(id, (tab) => {
        const edits = tab.edits ?? noEdits();
        return { edits: { ...edits, inserts: [...edits.inserts, {}] } };
      }),
    toggleDeleteRow: (id, row) =>
      patchTab(id, (tab) => {
        const edits = tab.edits ?? noEdits();
        const loaded = tab.sets[tab.activeSet]?.rows.length ?? 0;
        if (row >= loaded) {
          return {
            edits: { ...edits, inserts: edits.inserts.filter((_, i) => i !== row - loaded) },
          };
        }
        const deletes = edits.deletes.includes(row)
          ? edits.deletes.filter((r) => r !== row)
          : [...edits.deletes, row];
        return { edits: { ...edits, deletes } };
      }),
    discardEdits: (id) => patchTab(id, () => ({ edits: null })),
    saveEdits: (id, confirmed = false) => {
      const tab = get().tabs.find((t) => t.id === id);
      const set = tab?.sets[tab.activeSet];
      const readOnly = get().connections.find((c) => c.id === tab?.connId)?.read_only ?? true;
      if (!tab?.browse || !set?.columns || !tab.edits || tab.saving) return;
      const key = editableKey(tab, readOnly);
      if (!key) return;
      const names = set.columns.map((c) => c.source_column ?? c.name);
      const cell = (col: number, value: Value): CellValue => ({ column: names[col] ?? "", value });
      const keyOf = (row: number): CellValue[] =>
        key.map((col) => cell(col, set.rows[row]?.[col] ?? { t: "null" }));
      const valuesOf = (record: Record<number, Value>): CellValue[] =>
        Object.entries(record).map(([col, value]) => cell(Number(col), value));
      const { updates, inserts, deletes } = tab.edits;
      const changes: RowChange[] = [
        ...Object.entries(updates)
          .filter(([row]) => !deletes.includes(Number(row)))
          .map(([row, record]) => ({
            kind: "update" as const,
            key: keyOf(Number(row)),
            set: valuesOf(record),
          })),
        ...inserts.map((record) => ({ kind: "insert" as const, values: valuesOf(record) })),
        ...deletes.map((row) => ({ kind: "delete" as const, key: keyOf(row) })),
      ];
      patchTab(id, () => ({ saving: true }));
      send({
        type: "ApplyChanges",
        tab: id,
        conn: tab.connId,
        table: tab.browse.table,
        changes,
        confirmed,
      });
    },
    answerChangesConfirm: (save) => {
      const pending = get().changesConfirm;
      set({ changesConfirm: null });
      if (!pending) return;
      if (!save) {
        patchTab(pending.tab, () => ({ saving: false }));
        return;
      }
      // Rebuild the same batch, now confirmed.
      patchTab(pending.tab, () => ({ saving: false }));
      get().saveEdits(pending.tab, true);
    },
  };
});

// Save the open tabs about a second after they change. Result rows also change `tabs`, but the
// saved shape (titles, SQL, pages) stays the same, so `saveWorkspace` skips those writes.
useApp.subscribe((state, prev) => {
  if (state.tabs === prev.tabs && state.activeTabId === prev.activeTabId) return;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void saveWorkspace(), 1000);
});
