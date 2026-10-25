/**
 * App state. Engine events are applied in `handleEvent`; components call the actions.
 * Keep engine calls (`send`) in actions, not in components.
 */
import { create } from "zustand";
import type { BrowsePage } from "@/bindings/BrowsePage";
import type { BrowseQuery } from "@/bindings/BrowseQuery";
import type { Capabilities } from "@/bindings/Capabilities";
import type { CellValue } from "@/bindings/CellValue";
import type { ColumnInfo } from "@/bindings/ColumnInfo";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { ErrorLocation } from "@/bindings/ErrorLocation";
import type { Event } from "@/bindings/Event";
import type { Favorite } from "@/bindings/Favorite";
import type { HistoryEntry } from "@/bindings/HistoryEntry";
import type { ObjectKind } from "@/bindings/ObjectKind";
import type { RowChange } from "@/bindings/RowChange";
import type { SchemaChange } from "@/bindings/SchemaChange";
import type { SchemaObject } from "@/bindings/SchemaObject";
import type { ServerInfoKind } from "@/bindings/ServerInfoKind";
import type { ServerInfoRow } from "@/bindings/ServerInfoRow";
import type { TableColumns } from "@/bindings/TableColumns";
import type { TableInfo } from "@/bindings/TableInfo";
import type { TableRef } from "@/bindings/TableRef";
import type { Value } from "@/bindings/Value";
import type { Workspace } from "@/bindings/Workspace";
import { send } from "@/lib/engine";
import { setPerfEnabled } from "@/lib/perf";
import { applyTheme, loadTheme, saveTheme, type ThemePref } from "@/lib/theme";
import { quoteIdent } from "@/lib/values";

/** Rows per page in a browse tab. */
export const PAGE_SIZE = 1000;

export type ConnStatus =
  | { kind: "idle" }
  | { kind: "connecting" }
  | { kind: "connected"; version: string; capabilities: Capabilities }
  | { kind: "failed"; error: string };

export interface DbNode {
  name: string;
  expanded: boolean;
  loading: boolean;
  objects: SchemaObject[] | null;
  error: string | null;
  /** Column names per table, for completion. Loaded after `objects`; `null` until then. */
  columns: TableColumns[] | null;
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
  /** The statement of the run (from 0) that produced it. */
  statement: number;
}

/** Where the run is in its statements. */
export interface RunProgress {
  /** The statement running now, or the last one that started. */
  index: number;
  count: number;
}

/** The statement a failed run stopped at, and where the database found the error in it. */
export interface RunFailure {
  statement: number | null;
  location: ErrorLocation | null;
}

/** Structure of the table a browse tab shows, loaded the first time the Structure view opens. */
export interface TabStructure {
  loading: boolean;
  info: TableInfo | null;
  error: string | null;
}

export type TabView = "data" | "structure";

/** What the Structure view shows: one kind of thing at a time. */
export type StructurePart = "columns" | "indexes" | "foreign_keys" | "ddl";

/** Indexes and foreign keys of a table shown in the sidebar tree. */
export interface TreeTableInfo {
  loading: boolean;
  info: TableInfo | null;
  error: string | null;
}

/** Key of a node below a database in `treeOpen`: the group of a kind, a table, or a group of a table. */
export const treeKey = (conn: string, database: string, rest: string) =>
  `${conn}/${database}/${rest}`;

export const emptyQuery = (): BrowseQuery => ({ filters: [], sort: [] });

/** The statement whose plan the tab shows, and as what. */
export interface ExplainState {
  sql: string;
  json: boolean;
}

/** Rows of history shown in the history dialog. */
export const HISTORY_LIMIT = 200;

export interface HistoryState {
  entries: HistoryEntry[];
  loaded: boolean;
  query: string;
  /** Only this connection's queries; `null` for every connection. */
  conn: string | null;
}

/** The server variables or status dialog of one connection. */
export interface ServerInfoState {
  conn: string;
  kind: ServerInfoKind;
  request: number;
  loading: boolean;
  rows: ServerInfoRow[];
  error: string | null;
}

export type SidebarPane = "connections" | "favorites";

/** Grid edits not yet saved. Row and column numbers index the shown page. */
export interface Edits {
  /** row → column → new value */
  updates: Record<number, Record<number, Value>>;
  /** New rows: column → value; a missing column gets its default. */
  inserts: Array<Record<number, Value>>;
  deletes: number[];
}

export const noEdits = (): Edits => ({ updates: {}, inserts: [], deletes: [] });

/** Undo steps kept per tab. */
const UNDO_LIMIT = 200;

/** No pending edits and no undo history: the shown result was saved or replaced. */
const clearedEdits = () => ({ edits: null, editsPast: [], editsFuture: [] });

/** One cell's new value, for edits applied together as one undo step. */
export interface CellEdit {
  row: number;
  col: number;
  value: Value;
}

/** `edits` with `cell` set: a loaded row gets an update, a new row its value. */
function withCell(edits: Edits, loaded: number, { row, col, value }: CellEdit): Edits {
  if (row >= loaded) {
    const inserts = [...edits.inserts];
    inserts[row - loaded] = { ...inserts[row - loaded], [col]: value };
    return { ...edits, inserts };
  }
  return {
    ...edits,
    updates: { ...edits.updates, [row]: { ...edits.updates[row], [col]: value } },
  };
}

/** `edits` with `rows` marked for deletion or kept again; new rows are removed outright. */
function withToggledRows(edits: Edits, loaded: number, rows: number[]): Edits {
  const removed = new Set(rows.filter((r) => r >= loaded).map((r) => r - loaded));
  const toggled = new Set(rows.filter((r) => r < loaded));
  const deletes = [
    ...edits.deletes.filter((r) => !toggled.has(r)),
    ...[...toggled].filter((r) => !edits.deletes.includes(r)),
  ];
  return { ...edits, inserts: edits.inserts.filter((_, i) => !removed.has(i)), deletes };
}

export function editCount(edits: Edits | null): number {
  if (!edits) return 0;
  return Object.keys(edits.updates).length + edits.inserts.length + edits.deletes.length;
}

/** The table a typed query's result set comes from, when every column comes from one table. */
export interface ResultTable {
  set: number;
  table: TableRef;
  info: TableInfo | null;
}

/** Where grid edits of the shown result go, and how rows are matched. */
export interface EditTarget {
  table: TableRef;
  /** Result columns that identify a row. */
  key: number[];
  /** No primary or unique key: rows are matched by the values of `key` (several columns), and
   * the engine refuses a change that would hit more than one row. */
  matchAll: boolean;
  /** New rows can be added (table pages only; a query result has no place to show them). */
  canInsert: boolean;
}

/** Column types whose values do not compare exactly, so they cannot match a row. */
const INEXACT_TYPE = /float|double|real|json|geometry|point|linestring|polygon/i;

/**
 * Where edits of the shown result go: a page of a table, or a query result whose columns all
 * come from one table. Rows are matched by the primary key, else by a unique key on NOT NULL
 * columns, else by all comparable columns. `null` when the result can't be edited.
 */
export function editTarget(tab: Tab, readOnly: boolean): EditTarget | null {
  const set = tab.sets[tab.activeSet];
  if (readOnly || !set?.columns) return null;
  let table: TableRef;
  let info: TableInfo | null | undefined;
  let canInsert: boolean;
  if (tab.browse && tab.paged) {
    table = tab.browse.table;
    info = tab.structure?.info;
    canInsert = true;
  } else if (!tab.explain && tab.resultTable?.set === tab.activeSet) {
    table = tab.resultTable.table;
    info = tab.resultTable.info;
    canInsert = false;
  } else {
    return null;
  }
  if (!info || info.view) return null;
  const names = set.columns.map((c) => c.source_column ?? c.name);
  const positions = (columns: string[]) => {
    const found = columns.map((k) => names.indexOf(k));
    return found.every((p) => p >= 0) ? found : null;
  };
  if (info.primary_key.length > 0) {
    const key = positions(info.primary_key);
    return key && { table, key, matchAll: false, canInsert };
  }
  const notNull = new Set(info.columns.filter((c) => !c.nullable).map((c) => c.name));
  for (const index of info.indexes) {
    if (!index.unique || !index.columns.every((c) => notNull.has(c))) continue;
    const key = positions(index.columns);
    if (key) return { table, key, matchAll: false, canInsert };
  }
  const types = new Map(info.columns.map((c) => [c.name, c.type_name]));
  const key = names
    .map((name, i) => ({ name, i }))
    .filter(({ name }) => types.has(name) && !INEXACT_TYPE.test(types.get(name) ?? ""))
    .map(({ i }) => i);
  return key.length > 0 ? { table, key, matchAll: true, canInsert } : null;
}

/** The one table every column of `set` comes from, or null. */
function sourceTable(set: ResultSet | undefined, database: string | null): TableRef | null {
  const columns = set?.columns;
  const first = columns?.[0];
  if (!columns || !first?.source_table) return null;
  const same = columns.every(
    (c) =>
      c.source_table === first.source_table &&
      c.source_database === first.source_database &&
      c.source_column,
  );
  if (!same) return null;
  return { database: first.source_database ?? database, schema: null, name: first.source_table };
}

/** What a schema form dialog edits. */
export type SchemaForm =
  | { kind: "column"; conn: string; table: TableRef; column: ColumnInfo | null }
  | { kind: "index"; conn: string; table: TableRef; columns: string[] }
  | { kind: "create_table"; conn: string; database: string }
  | { kind: "rename_table"; conn: string; table: TableRef }
  | { kind: "create_database"; conn: string };

/** Statements of a schema change, shown before they run. */
export interface SchemaReview {
  request: number;
  conn: string;
  change: SchemaChange;
  statements: string[];
  warning: string | null;
  running: boolean;
  error: string | null;
}

export type PaletteMode = "open" | "commands";

/** What an import or export works on. */
export type TransferTarget =
  | { kind: "table"; conn: string; table: TableRef }
  | { kind: "result"; tab: number };

export interface Tab {
  id: number;
  title: string;
  connId: string;
  database: string | null;
  /** The editor text. Saved with the workspace, so it survives a restart. */
  sql: string;
  /** Set for a table opened from the sidebar. */
  browse: BrowsePage | null;
  /** Pinned tabs sit first, are kept by "Close other tabs" and show no close button. */
  pinned: boolean;
  view: TabView;
  structure: TabStructure | null;
  /** The part of the structure shown, and the name of the row to point at (from the sidebar). */
  structurePart: StructurePart;
  structureFocus: string | null;
  /** The shown results are a page of `browse` (not a typed query), so the pager applies. */
  paged: boolean;
  edits: Edits | null;
  /** Earlier states of `edits`, oldest first, for Undo; and undone ones, for Redo. Cleared
   * whenever the edits are saved or the shown result is replaced. */
  editsPast: Array<Edits | null>;
  editsFuture: Array<Edits | null>;
  saving: boolean;
  running: boolean;
  sets: ResultSet[];
  activeSet: number;
  elapsedMs: number | null;
  error: string | null;
  progress: RunProgress | null;
  failure: RunFailure | null;
  /** Bumped on every run so views can reset scroll/selection. */
  runId: number;
  /** An exact row count of the browsed table (with its filters) is being computed. */
  counting: boolean;
  /** Exact row count for the current filters, once asked for. */
  rowCount: number | null;
  /** The tab's session has a transaction open. */
  txOpen: boolean;
  /** Set while the results are a query plan. */
  explain: ExplainState | null;
  /** The SQL last run in this tab, for Refresh. */
  lastRun: string | null;
  /** The table the shown query result comes from, which makes it editable. */
  resultTable: ResultTable | null;
  /** Names of the columns hidden in the grid. Kept across pages and runs; not saved on quit. */
  hiddenColumns: string[];
}

function blankTab(
  fields: Pick<Tab, "title" | "connId" | "database" | "sql" | "browse"> & { id: number },
): Tab {
  return {
    ...fields,
    pinned: false,
    view: "data",
    structure: null,
    structurePart: "columns",
    structureFocus: null,
    paged: false,
    edits: null,
    editsPast: [],
    editsFuture: [],
    saving: false,
    running: false,
    sets: [],
    activeSet: 0,
    elapsedMs: null,
    error: null,
    progress: null,
    failure: null,
    runId: 0,
    counting: false,
    rowCount: null,
    txOpen: false,
    explain: null,
    lastRun: null,
    resultTable: null,
    hiddenColumns: [],
  };
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
  /** Tabs with an open transaction, which quitting rolls back. */
  transactions: number;
}

/** Returns what quitting would lose, or null when it is safe to quit without asking. */
export function pendingWork(tabs: Tab[]): QuitInfo | null {
  const running = tabs.filter((t) => t.running).length;
  const edits = tabs.filter((t) => editCount(t.edits) > 0).length;
  const transactions = tabs.filter((t) => t.txOpen).length;
  return running + edits + transactions > 0 ? { running, edits, transactions } : null;
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
  /** Tabs waiting for the user to agree that closing them rolls back an open transaction. */
  closeConfirm: number[] | null;
  /** Which nodes of the sidebar tree below a database are open, by key (`treeKey`); a node not
   * listed is closed, except the Tables group. */
  treeOpen: Record<string, boolean>;
  /** Indexes and foreign keys of the tables opened in the tree, by `${conn}/${db}/${table}`. */
  tableInfos: Record<string, TreeTableInfo>;
  /** Rows of a tab waiting for the answer to "Delete rows?". */
  deleteConfirm: { tab: number; rows: number[] } | null;
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
  /** Load a database's tables and columns for completion, unless they are loaded or loading. */
  loadCompletion: (connId: string, database: string) => void;
  openObject: (connId: string, database: string, name: string, kind: ObjectKind) => void;

  newQuery: (target?: { connId: string; database: string | null }) => void;
  selectTab: (id: number) => void;
  /** Close a tab; with an open transaction, ask first unless `force`. */
  closeTab: (id: number, force?: boolean) => void;
  /** Close several tabs; asks once if any has an open transaction, unless `force`. */
  closeTabs: (ids: number[], force?: boolean) => void;
  /** Close the tabs after `id`, except pinned ones. */
  closeTabsToRight: (id: number) => void;
  /** Close the other tabs, except pinned ones. */
  closeOtherTabs: (id: number) => void;
  /** Select the next (`1`) or previous (`-1`) tab, wrapping around. */
  cycleTab: (step: number) => void;
  /** Select tab number `n` from 1; 9 is always the last tab. */
  selectTabAt: (n: number) => void;
  /** Tabs closed this session, newest last, for "Reopen closed tab". */
  closedTabs: ClosedTab[];
  reopenClosedTab: () => void;
  /** Pin or unpin a tab. Pinned tabs move to the front. */
  togglePin: (id: number) => void;
  /** Move a tab to `index`; a tab never crosses the line between pinned and unpinned. */
  moveTab: (id: number, index: number) => void;
  answerCloseConfirm: (close: boolean) => void;
  /** Open or close a node of the tree below a database; opening a table loads its keys. */
  toggleTreeNode: (key: string, table?: { conn: string; database: string; name: string }) => void;
  /** Mark rows for deletion; asks first when that would delete rows of the table. */
  requestDeleteRows: (id: number, rows: number[]) => void;
  answerDeleteConfirm: (confirmed: boolean) => void;
  setSql: (id: number, sql: string) => void;
  run: (id: number, sql: string) => void;
  /** Load the page of a browse tab that starts at row `offset`. */
  browse: (id: number, offset: number) => void;
  /** Replace the filters and sort of a browse tab and reload from the first row. */
  setBrowseQuery: (id: number, query: BrowseQuery) => void;
  /** Sort by `column`: ascending, then descending, then unsorted. */
  toggleSort: (id: number, column: string) => void;
  /** Count the rows that match the browse tab's filters exactly. */
  countRows: (id: number) => void;
  /** Show the plan of `sql` (one statement) as a table or as a JSON tree. */
  explain: (id: number, sql: string, json: boolean) => void;
  beginTransaction: (id: number) => void;
  endTransaction: (id: number, commit: boolean) => void;
  /** Load the tab's results again: the same page of a table, plan, or last query. */
  refresh: (id: number) => void;
  cancel: (id: number) => void;
  setActiveSet: (id: number, set: number) => void;
  /** Hide the grid columns named in `names` (all others shown). */
  setHiddenColumns: (id: number, names: string[]) => void;
  /** Data or Structure, for browse tabs. */
  setView: (id: number, view: TabView) => void;
  setStructurePart: (id: number, part: StructurePart) => void;
  answerConfirm: (run: boolean) => void;

  editCell: (id: number, row: number, col: number, value: Value) => void;
  /** Several cells at once (Set NULL on a selection), as one undo step. */
  editCells: (id: number, cells: CellEdit[]) => void;
  addRow: (id: number) => void;
  /** Mark a loaded row for deletion, or unmark it; removes a new row outright. */
  toggleDeleteRow: (id: number, row: number) => void;
  /** Mark or unmark several rows (the selection), as one undo step. */
  toggleDeleteRows: (id: number, rows: number[]) => void;
  discardEdits: (id: number) => void;
  /** Step back or forward through the pending edits. */
  undoEdit: (id: number) => void;
  redoEdit: (id: number) => void;
  saveEdits: (id: number, confirmed?: boolean) => void;
  answerChangesConfirm: (save: boolean) => void;
  /** Copy rows as one INSERT statement (built by the engine for the connection's dialect). */
  copyInsert: (id: number, columns: string[], rows: Value[][]) => void;

  schemaForm: SchemaForm | null;
  openSchemaForm: (form: SchemaForm | null) => void;
  schemaReview: SchemaReview | null;
  /** Ask the engine for the statements of `change`; they are shown before anything runs. */
  alterSchema: (conn: string, change: SchemaChange) => void;
  /** Run the reviewed change, or close the review (back to its form, if one made it). */
  answerSchemaReview: (run: boolean) => void;
  /** Load a database's objects again, keeping the shown ones until the new list arrives. */
  reloadObjects: (connId: string, database: string) => void;
  /** Open a table's Structure view. */
  openStructure: (
    connId: string,
    database: string,
    name: string,
    part?: StructurePart,
    focus?: string,
  ) => void;

  palette: PaletteMode | null;
  setPalette: (mode: PaletteMode | null) => void;
  /** Import into, or export from, a table or a tab's result. Not built yet: the buttons and menu
   * items are in place and call this. */
  transfer: (kind: "import" | "export", target: TransferTarget) => void;

  addTableFavorite: (connId: string, database: string, table: string) => void;
  saveQueryFavorite: (tabId: number, name: string) => void;
  deleteFavorite: (id: string) => void;
  openFavorite: (favorite: Favorite) => void;

  serverInfo: ServerInfoState | null;
  /** Open the server variables or status of a connection; calling it again reloads. */
  openServerInfo: (conn: string, kind: ServerInfoKind) => void;
  closeServerInfo: () => void;

  loadHistory: (query: string, conn: string | null) => void;
  /** The query history dialog, opened from the editor toolbar. */
  historyOpen: boolean;
  setHistoryOpen: (open: boolean) => void;
  sidebarPane: SidebarPane;
  setSidebarPane: (pane: SidebarPane) => void;
  /** Pin the connection in Favorites, or unpin it. */
  toggleConnectionFavorite: (connId: string) => void;
  clearHistory: () => void;
  /** Open `sql` in a new tab on that connection and database (history, favorites). */
  openSql: (connId: string, database: string | null, sql: string, title?: string) => void;
  /** Light, dark, or follow the OS; remembered between launches. */
  theme: ThemePref;
  setTheme: (theme: ThemePref) => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
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

/** What "Reopen closed tab" brings back: the same things a restart restores. */
export type ClosedTab = Pick<Tab, "title" | "connId" | "database" | "sql" | "browse">;

/** Closed tabs kept for reopening. */
const CLOSED_TAB_LIMIT = 20;

let nextTabId = 1;
let nextQuery = 1;
let nextToast = 1;
let nextRequest = 1;

/** Only the newest history request is shown, so typing fast never shows stale results. */
let historyRequest = 0;

/** Table structure requests waiting for `TableInfo`: request id → tab id, and whether the
 * structure is the browsed table's or the source table of a query result. */
const structureRequests = new Map<number, { tab: number; kind: "browse" | "result" }>();

/** `LoadTableInfo` requests made by the tree, waiting for their answer: request id → table key. */
const treeInfoRequests = new Map<number, string>();

/** `InsertSql` requests waiting for their text: request id → rows copied. */
const insertRequests = new Map<number, number>();

/** Schema changes waiting for their first `SchemaReview` or `SchemaChanged`. */
const schemaRequests = new Set<number>();

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
      pinned: t.pinned,
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
        sets.push({ columns: null, rows: [], affected: 0, truncated: false, statement: 0 });
      const current = sets[index] as ResultSet;
      sets[index] = { ...current, ...patch(current) };
      return { sets };
    });

  /** Change a tab's pending edits as one undo step; a new step drops what was undone. */
  const changeEdits = (id: number, change: (edits: Edits, loaded: number) => Edits | null) =>
    patchTab(id, (tab) => {
      if (tab.saving) return {};
      const loaded = tab.sets[tab.activeSet]?.rows.length ?? 0;
      return {
        edits: change(tab.edits ?? noEdits(), loaded),
        editsPast: [...tab.editsPast, tab.edits].slice(-UNDO_LIMIT),
        editsFuture: [],
      };
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

  /** Ask the engine for the indexes and keys of a table opened in the sidebar tree. */
  const loadTreeTable = (conn: string, database: string, name: string) => {
    const infoKey = `${conn}/${database}/${name}`;
    set((s) => ({
      tableInfos: { ...s.tableInfos, [infoKey]: { loading: true, info: null, error: null } },
    }));
    const request = nextRequest++;
    treeInfoRequests.set(request, infoKey);
    send({
      type: "LoadTableInfo",
      request,
      conn,
      table: { database, schema: null, name },
    });
  };

  /** Read again what the tree shows of the open tables of a connection, or of one database. */
  const reloadOpenTables = (conn: string, database?: string) => {
    const prefix = database === undefined ? `${conn}/` : `${conn}/${database}/`;
    set((s) => ({
      tableInfos: Object.fromEntries(
        Object.entries(s.tableInfos).filter(([key]) => !key.startsWith(prefix)),
      ),
    }));
    for (const [key, open] of Object.entries(get().treeOpen)) {
      const at = key.indexOf("/table:");
      if (!open || !key.startsWith(prefix) || at < 0 || key.includes("#")) continue;
      loadTreeTable(conn, key.slice(`${conn}/`.length, at), key.slice(at + "/table:".length));
    }
  };

  /** Ask the engine for the structure of a browse tab's table (`TableInfo` fills it in). */
  const loadStructure = (id: number) => {
    const tab = get().tabs.find((t) => t.id === id);
    if (!tab?.browse) return;
    patchTab(id, () => ({ structure: { loading: true, info: null, error: null } }));
    const request = nextRequest++;
    structureRequests.set(request, { tab: id, kind: "browse" });
    send({ type: "LoadTableInfo", request, conn: tab.connId, table: tab.browse.table });
  };

  /** Find the table the shown query result comes from and load its keys, so it can be edited. */
  const detectResultTable = (id: number) => {
    const tab = get().tabs.find((t) => t.id === id);
    if (!tab || tab.paged || tab.explain) return;
    const table = sourceTable(tab.sets[tab.activeSet], tab.database);
    if (!table) {
      if (tab.resultTable) patchTab(id, () => ({ resultTable: null }));
      return;
    }
    const known = tab.resultTable;
    const same =
      known &&
      known.table.name === table.name &&
      known.table.database === table.database &&
      known.info !== null;
    patchTab(id, () => ({
      resultTable: { set: tab.activeSet, table, info: same ? known.info : null },
    }));
    if (same) return;
    const request = nextRequest++;
    structureRequests.set(request, { tab: id, kind: "result" });
    send({ type: "LoadTableInfo", request, conn: tab.connId, table });
  };

  /** Show what a successful schema change changed: the tree, open structures and pages. */
  const afterSchemaChange = (conn: string, change: SchemaChange) => {
    if (change.kind === "create_database" || change.kind === "drop_database") return;
    const { table } = change;
    const database = table.database;
    if (database) get().reloadObjects(conn, database);
    if (change.kind === "create_table") return;
    const onTable = (t: Tab) =>
      t.connId === conn &&
      t.browse?.table.name === table.name &&
      t.browse.table.database === table.database;
    const tabs = get().tabs.filter(onTable);
    if (change.kind === "drop_table") {
      get().closeTabs(tabs.map((t) => t.id));
      return;
    }
    if (change.kind === "rename_table") {
      const renamed = { ...table, name: change.new_name };
      set((s) => ({
        tabs: s.tabs.map((t) =>
          onTable(t) && t.browse
            ? { ...t, title: change.new_name, browse: { ...t.browse, table: renamed } }
            : t,
        ),
      }));
      return;
    }
    for (const tab of tabs) {
      if (change.kind === "alter_table") loadStructure(tab.id);
      if (tab.paged && editCount(tab.edits) === 0) get().browse(tab.id, tab.browse?.offset ?? 0);
    }
  };

  const openTab = (
    connId: string,
    database: string | null,
    title: string,
    sql: string,
    browse: BrowsePage | null = null,
  ) => {
    const tab = blankTab({ id: nextTabId++, title, connId, database, sql, browse });
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
    theme: loadTheme(),
    setTheme: (theme) => {
      saveTheme(theme);
      applyTheme(theme);
      set({ theme });
    },
    settingsOpen: false,
    setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
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
    closeConfirm: null,
    deleteConfirm: null,
    treeOpen: {},
    tableInfos: {},
    history: { entries: [], loaded: false, query: "", conn: null },
    historyOpen: false,
    setHistoryOpen: (historyOpen) => set({ historyOpen }),
    serverInfo: null,
    openServerInfo: (conn, kind) => {
      const request = nextRequest++;
      // Reloading the same list keeps its rows on screen until the new ones arrive.
      const same = get().serverInfo?.conn === conn && get().serverInfo?.kind === kind;
      set((s) => ({
        serverInfo: {
          conn,
          kind,
          request,
          loading: true,
          rows: same ? (s.serverInfo?.rows ?? []) : [],
          error: null,
        },
      }));
      send({ type: "LoadServerInfo", request, conn, kind });
    },
    closeServerInfo: () => set({ serverInfo: null }),
    sidebarPane: "connections",
    setSidebarPane: (sidebarPane) => set({ sidebarPane }),
    favorites: [],
    toasts: [],

    handleEvent: (event) => {
      switch (event.type) {
        case "Pong":
          set({ engineVersion: event.version });
          setPerfEnabled(event.perf);
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
          // The engine closed the connection's sessions, which rolls back their transactions.
          if (state.kind === "Disconnected") {
            set((s) => ({
              tabs: s.tabs.map((t) => (t.connId === event.id ? { ...t, txOpen: false } : t)),
              // Opened nodes stay open for the next connection; what they showed is read again.
              tableInfos: Object.fromEntries(
                Object.entries(s.tableInfos).filter(([key]) => !key.startsWith(`${event.id}/`)),
              ),
            }));
          }
          if (state.kind === "Connected") {
            reloadOpenTables(event.id);
          }
          patchNode(event.id, (node) => {
            switch (state.kind) {
              case "Connecting":
                return { status: { kind: "connecting" } };
              case "Connected":
                return {
                  status: {
                    kind: "connected",
                    version: state.server_version,
                    capabilities: state.capabilities,
                  },
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
          // A refreshed list (after creating or dropping a database) keeps what is open.
          const known = new Map(
            (get().nodes[event.id]?.databases ?? []).map((db) => [db.name, db] as const),
          );
          const fresh = known.size === 0;
          patchNode(event.id, () => ({
            databases: event.databases.map(
              (name) =>
                known.get(name) ?? {
                  name,
                  expanded: fresh && name === preferred,
                  loading: fresh && name === preferred,
                  objects: null,
                  error: null,
                  columns: null,
                },
            ),
          }));
          if (fresh && preferred && event.databases.includes(preferred)) {
            send({ type: "LoadObjects", id: event.id, database: preferred });
          }
          break;
        }
        case "Objects":
          // A failed reload keeps the objects already shown.
          patchDb(event.id, event.database, (db) => ({
            loading: false,
            objects: event.objects ?? db.objects,
            error: event.error,
          }));
          // Completion works without columns, so they load after the tree is shown.
          if (event.objects) send({ type: "LoadColumns", id: event.id, database: event.database });
          break;
        case "Columns":
          // Keep the previous columns when a reload fails.
          if (event.columns) patchDb(event.id, event.database, () => ({ columns: event.columns }));
          break;
        case "RowCount":
          patchTab(event.tab, () => ({ counting: false, rowCount: event.count }));
          if (event.error) get().toast("error", event.error);
          break;
        case "TransactionState":
          patchTab(event.tab, () => ({ txOpen: event.open }));
          if (event.error) get().toast("error", event.error);
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
            progress: null,
            failure: null,
            runId: t.runId + 1,
          }));
          break;
        case "StatementStarted":
          patchTab(event.tab, () => ({ progress: { index: event.index, count: event.count } }));
          break;
        case "ResultColumns":
          patchSet(event.tab, event.set, () => ({
            columns: event.columns,
            statement: event.statement,
          }));
          break;
        case "ResultRows":
          patchSet(event.tab, event.set, (rs) => ({ rows: rs.rows.concat(event.rows) }));
          break;
        case "ResultDone":
          patchSet(event.tab, event.set, () => ({
            affected: event.affected,
            truncated: event.truncated,
            statement: event.statement,
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
          detectResultTable(event.tab);
          break;
        case "QueryFailed":
          patchTab(event.tab, () => ({
            running: false,
            error: event.cancelled ? "Query cancelled" : event.error,
            failure: event.cancelled
              ? null
              : { statement: event.statement, location: event.location },
          }));
          break;
        case "Workspace": {
          if (workspaceRestored) break;
          const known = new Set(get().connections.map((c) => c.id));
          const restored: Tab[] = event.workspace.tabs
            .filter((t) => known.has(t.conn))
            .map((t) => ({
              ...blankTab({
                id: nextTabId++,
                title: t.title,
                connId: t.conn,
                database: t.database,
                sql: t.sql,
                browse: t.browse,
              }),
              pinned: t.pinned,
            }));
          for (const t of restored) {
            const n = /^Query (\d+)$/.exec(t.title)?.[1];
            if (n) nextQuery = Math.max(nextQuery, Number(n) + 1);
          }
          const active = restored[event.workspace.active ?? -1] ?? restored[0];
          set((s) => ({
            tabs: pinnedFirst([...restored, ...s.tabs]),
            activeTabId: s.activeTabId ?? active?.id ?? null,
          }));
          workspaceRestored = true;
          lastSaved = JSON.stringify(workspaceOf(get().tabs, get().activeTabId));
          break;
        }
        case "ServerInfo":
          set((s) =>
            s.serverInfo?.request === event.request
              ? {
                  serverInfo: {
                    ...s.serverInfo,
                    loading: false,
                    rows: event.rows ?? s.serverInfo.rows,
                    error: event.error,
                  },
                }
              : s,
          );
          break;
        case "TableInfo": {
          const treeTable = treeInfoRequests.get(event.request);
          if (treeTable !== undefined) {
            treeInfoRequests.delete(event.request);
            set((s) => ({
              tableInfos: {
                ...s.tableInfos,
                [treeTable]: { loading: false, info: event.info, error: event.error },
              },
            }));
            break;
          }
          const pending = structureRequests.get(event.request);
          structureRequests.delete(event.request);
          if (pending === undefined) break;
          if (pending.kind === "result") {
            // Without the table's keys the result just stays read-only.
            patchTab(pending.tab, (t) => ({
              resultTable: t.resultTable && { ...t.resultTable, info: event.info },
            }));
            break;
          }
          patchTab(pending.tab, () => ({
            structure: { loading: false, info: event.info, error: event.error },
          }));
          break;
        }
        case "ChangesConfirmRequired":
          set({ changesConfirm: { tab: event.tab, message: event.message } });
          break;
        case "ChangesApplied": {
          const tab = get().tabs.find((t) => t.id === event.tab);
          if (tab && !tab.paged) {
            // A query result is not run again (it may be a script with writes): show the saved
            // values in place instead.
            patchTab(tab.id, (t) => ({ sets: withEdits(t.sets, t.activeSet, t.edits) }));
          }
          patchTab(event.tab, () => ({ ...clearedEdits(), saving: false }));
          get().toast("success", event.message);
          if (tab?.browse && tab.paged) get().browse(tab.id, tab.browse.offset);
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
        case "History": {
          if (event.request !== historyRequest) break;
          set((s) => ({ history: { ...s.history, entries: event.entries, loaded: true } }));
          break;
        }
        case "HistoryAdded":
          set((s) => {
            const { history } = s;
            const q = history.query.trim().toLowerCase();
            const other = history.conn !== null && history.conn !== event.entry.conn;
            if (!history.loaded || other || (q && !event.entry.sql.toLowerCase().includes(q))) {
              return {};
            }
            const entries = [event.entry, ...history.entries].slice(0, HISTORY_LIMIT);
            return { history: { ...history, entries } };
          });
          break;
        case "InsertSql": {
          const rows = insertRequests.get(event.request);
          insertRequests.delete(event.request);
          if (rows === undefined) break;
          if (event.error || event.text === null) {
            get().toast("error", event.error ?? "Couldn't copy the rows");
            break;
          }
          copyText(event.text, `Copied ${rows === 1 ? "1 row" : `${rows} rows`} as INSERT`);
          break;
        }
        case "SchemaReview": {
          if (!schemaRequests.delete(event.request)) break;
          // The review opens over the form, so going back keeps what was typed.
          set({
            schemaReview: {
              request: event.request,
              conn: event.conn,
              change: event.change,
              statements: event.statements,
              warning: event.warning,
              running: false,
              error: null,
            },
          });
          break;
        }
        case "SchemaChanged": {
          const review = get().schemaReview;
          if (review?.request === event.request) {
            if (event.error) {
              set({ schemaReview: { ...review, running: false, error: event.error } });
              break;
            }
            set({ schemaReview: null, schemaForm: null });
            get().toast("success", "Schema updated");
            afterSchemaChange(event.conn, event.change);
            break;
          }
          // Refused before review (an empty name, a read-only connection): the form stays open.
          if (schemaRequests.delete(event.request) && event.error) {
            get().toast("error", event.error);
          }
          break;
        }
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
    loadCompletion: (connId, database) => {
      const db = get().nodes[connId]?.databases.find((d) => d.name === database);
      if (!db || db.objects !== null || db.loading) return;
      patchDb(connId, database, () => ({ loading: true, error: null }));
      send({ type: "LoadObjects", id: connId, database });
    },
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
        const browse = { table: { database, schema: null, name }, offset: 0, query: emptyQuery() };
        const id = openTab(connId, database, name, "", browse);
        get().browse(id, 0);
        return;
      }
      const what: Partial<Record<ObjectKind, string>> = {
        function: "FUNCTION",
        trigger: "TRIGGER",
        event: "EVENT",
      };
      const sql = `SHOW CREATE ${what[kind] ?? "PROCEDURE"} ${quoteIdent(name)};`;
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
    closeTabsToRight: (id) => {
      const { tabs } = get();
      const at = tabs.findIndex((t) => t.id === id);
      if (at < 0) return;
      get().closeTabs(
        tabs
          .slice(at + 1)
          .filter((t) => !t.pinned)
          .map((t) => t.id),
      );
    },
    closeOtherTabs: (id) =>
      get().closeTabs(
        get()
          .tabs.filter((t) => t.id !== id && !t.pinned)
          .map((t) => t.id),
      ),
    cycleTab: (step) => {
      const { tabs, activeTabId } = get();
      if (tabs.length === 0) return;
      const at = tabs.findIndex((t) => t.id === activeTabId);
      const next = tabs[(Math.max(at, 0) + step + tabs.length) % tabs.length];
      if (next) set({ activeTabId: next.id });
    },
    selectTabAt: (n) => {
      const { tabs } = get();
      const tab = n >= 9 ? tabs[tabs.length - 1] : tabs[n - 1];
      if (tab) set({ activeTabId: tab.id });
    },
    closeTab: (id, force = false) => get().closeTabs([id], force),
    closeTabs: (ids, force = false) => {
      const closing = new Set(ids);
      const { tabs } = get();
      if (!force && tabs.some((t) => closing.has(t.id) && t.txOpen)) {
        set({ closeConfirm: ids });
        return;
      }
      // The engine cancels a running query before it closes the session.
      for (const id of ids) send({ type: "CloseTab", tab: id });
      set((s) => {
        const closed: ClosedTab[] = s.tabs
          .filter((t) => closing.has(t.id))
          .map(({ title, connId, database, sql, browse }) => ({
            title,
            connId,
            database,
            sql,
            browse,
          }));
        const first = s.tabs.findIndex((t) => closing.has(t.id));
        const rest = s.tabs.filter((t) => !closing.has(t.id));
        const neighbour = rest[Math.min(Math.max(first, 0), rest.length - 1)];
        const activeClosed = s.activeTabId !== null && closing.has(s.activeTabId);
        return {
          tabs: rest,
          activeTabId: activeClosed ? (neighbour?.id ?? null) : s.activeTabId,
          closedTabs: [...s.closedTabs, ...closed].slice(-CLOSED_TAB_LIMIT),
        };
      });
    },
    closedTabs: [],
    togglePin: (id) =>
      set((s) => {
        const tabs = s.tabs.map((t) => (t.id === id ? { ...t, pinned: !t.pinned } : t));
        // Stable: pinned first, everything else keeps its order.
        return { tabs: [...tabs.filter((t) => t.pinned), ...tabs.filter((t) => !t.pinned)] };
      }),
    moveTab: (id, index) =>
      set((s) => {
        const from = s.tabs.findIndex((t) => t.id === id);
        const tab = s.tabs[from];
        if (!tab || from === index) return s;
        const pinnedCount = s.tabs.filter((t) => t.pinned).length;
        const [low, high] = tab.pinned ? [0, pinnedCount - 1] : [pinnedCount, s.tabs.length - 1];
        const target = Math.min(Math.max(index, low), high);
        if (target === from) return s;
        const tabs = [...s.tabs];
        tabs.splice(from, 1);
        tabs.splice(target, 0, tab);
        return { tabs };
      }),
    reopenClosedTab: () => {
      const closed = get().closedTabs;
      // Skip tabs whose connection was deleted since.
      const known = new Set(get().connections.map((c) => c.id));
      const index = closed.findLastIndex((t) => known.has(t.connId));
      const tab = closed[index];
      set({ closedTabs: index >= 0 ? closed.slice(0, index) : [] });
      if (!tab) return;
      const id = openTab(tab.connId, tab.database, tab.title, tab.sql, tab.browse);
      if (tab.browse) get().browse(id, tab.browse.offset);
    },
    requestDeleteRows: (id, rows) => {
      const tab = get().tabs.find((t) => t.id === id);
      const loaded = tab?.sets[tab.activeSet]?.rows.length ?? 0;
      if (!tab || rows.length === 0) return;
      const deletes = tab.edits?.deletes ?? [];
      const existing = rows.filter((r) => r < loaded);
      const added = rows.filter((r) => r >= loaded);
      // Rows of the table that this would newly mark for deletion.
      const fresh = existing.filter((r) => !deletes.includes(r));
      if (fresh.length === 0) {
        // Taking a deletion back, or dropping a row that was only just added, loses nothing.
        get().toggleDeleteRows(id, existing.length > 0 ? existing : added);
        return;
      }
      set({ deleteConfirm: { tab: id, rows: [...fresh, ...added] } });
    },
    answerDeleteConfirm: (confirmed) => {
      const pending = get().deleteConfirm;
      set({ deleteConfirm: null });
      if (confirmed && pending) get().toggleDeleteRows(pending.tab, pending.rows);
    },
    toggleTreeNode: (key, table) => {
      const open = !(get().treeOpen[key] ?? key.endsWith("/group:table"));
      set((s) => ({ treeOpen: { ...s.treeOpen, [key]: open } }));
      if (!open || !table) return;
      const known = get().tableInfos[`${table.conn}/${table.database}/${table.name}`];
      if (known && (known.loading || known.info)) return;
      loadTreeTable(table.conn, table.database, table.name);
    },
    answerCloseConfirm: (close) => {
      const ids = get().closeConfirm;
      set({ closeConfirm: null });
      if (close && ids) get().closeTabs(ids, true);
    },
    setSql: (id, sql) => patchTab(id, () => ({ sql })),
    run: (id, sql) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.running || !sql.trim()) return;
      if (editCount(tab.edits) > 0) {
        get().toast("info", "Save or discard your changes first");
        return;
      }
      patchTab(id, () => ({
        paged: false,
        ...clearedEdits(),
        explain: null,
        lastRun: sql,
        resultTable: null,
      }));
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
      patchTab(id, () => ({ browse, paged: true, ...clearedEdits(), explain: null }));
      // The primary key decides whether the page can be edited.
      if (tab.structure === null) loadStructure(id);
      send({
        type: "Browse",
        tab: id,
        conn: tab.connId,
        table: browse.table,
        limit: PAGE_SIZE,
        offset: browse.offset,
        query: browse.query,
      });
    },
    setBrowseQuery: (id, query) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab?.browse || tab.running) return;
      if (editCount(tab.edits) > 0) {
        get().toast("info", "Save or discard your changes first");
        return;
      }
      patchTab(id, (t) => ({ browse: t.browse && { ...t.browse, query }, rowCount: null }));
      get().browse(id, 0);
    },
    toggleSort: (id, column) => {
      const query = get().tabs.find((t) => t.id === id)?.browse?.query;
      if (!query) return;
      const current = query.sort[0]?.column === column ? query.sort[0] : null;
      const sort = !current
        ? [{ column, descending: false }]
        : current.descending
          ? []
          : [{ column, descending: true }];
      get().setBrowseQuery(id, { ...query, sort });
    },
    countRows: (id) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab?.browse || tab.counting || tab.running) return;
      patchTab(id, () => ({ counting: true }));
      send({
        type: "CountRows",
        tab: id,
        conn: tab.connId,
        table: tab.browse.table,
        query: tab.browse.query,
      });
    },
    explain: (id, sql, json) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.running || !sql.trim()) return;
      if (editCount(tab.edits) > 0) {
        get().toast("info", "Save or discard your changes first");
        return;
      }
      patchTab(id, () => ({ paged: false, ...clearedEdits(), explain: { sql, json } }));
      send({ type: "Explain", tab: id, conn: tab.connId, database: tab.database, sql, json });
    },
    beginTransaction: (id) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.running || tab.txOpen) return;
      send({ type: "BeginTransaction", tab: id, conn: tab.connId, database: tab.database });
    },
    endTransaction: (id, commit) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab?.txOpen || tab.running) return;
      send({ type: "EndTransaction", tab: id, commit });
    },
    refresh: (id) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.running) return;
      if (tab.browse && tab.view === "structure") return loadStructure(id);
      // Safe mode still applies: a rerun write on staging or production asks again.
      if (tab.explain) get().explain(id, tab.explain.sql, tab.explain.json);
      else if (tab.browse && (tab.paged || !tab.lastRun)) get().browse(id, tab.browse.offset);
      else if (tab.lastRun) get().run(id, tab.lastRun);
    },
    cancel: (id) => send({ type: "Cancel", tab: id }),
    setHiddenColumns: (id, names) => patchTab(id, () => ({ hiddenColumns: names })),
    setActiveSet: (id, index) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || tab.activeSet === index) return;
      if (editCount(tab.edits) > 0) {
        get().toast("info", "Save or discard your changes first");
        return;
      }
      patchTab(id, () => ({ activeSet: index, ...clearedEdits() }));
      detectResultTable(id);
    },
    setView: (id, view) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab?.browse) return;
      patchTab(id, () => ({ view }));
      if (view === "structure" && (tab.structure === null || tab.structure.error !== null)) {
        loadStructure(id);
      }
    },
    setStructurePart: (id, part) =>
      patchTab(id, () => ({ structurePart: part, structureFocus: null })),
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
      switch (favorite.item.kind) {
        case "connection": {
          // Show it in the tree, connected and open.
          const node = get().nodes[favorite.conn];
          set({ sidebarPane: "connections" });
          if (!get().sidebarOpen) get().toggleSidebar();
          if (node?.status.kind !== "connected" || !node.expanded) {
            get().toggleConnection(favorite.conn);
          }
          break;
        }
        case "table":
          get().openObject(favorite.conn, favorite.database ?? "", favorite.item.table, "table");
          break;
        case "query":
          get().openSql(favorite.conn, favorite.database, favorite.item.sql, favorite.name);
          break;
      }
    },
    toggleConnectionFavorite: (connId) => {
      const existing = get().favorites.find(
        (f) => f.conn === connId && f.item.kind === "connection",
      );
      if (existing) {
        send({ type: "DeleteFavorite", id: existing.id });
        return;
      }
      const name = get().connections.find((c) => c.id === connId)?.name ?? "Connection";
      send({
        type: "SaveFavorite",
        favorite: {
          id: crypto.randomUUID(),
          name,
          conn: connId,
          database: null,
          item: { kind: "connection" },
        },
      });
      get().toast("success", `Added ${name} to favorites`);
    },

    loadHistory: (query, conn) => {
      historyRequest = nextRequest++;
      set((s) => ({ history: { ...s.history, query, conn } }));
      send({
        type: "LoadHistory",
        request: historyRequest,
        query: query.trim() || null,
        conn,
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

    editCell: (id, row, col, value) => get().editCells(id, [{ row, col, value }]),
    editCells: (id, cells) =>
      changeEdits(id, (edits, loaded) =>
        cells.reduce((e, cell) => withCell(e, loaded, cell), edits),
      ),
    addRow: (id) => changeEdits(id, (edits) => ({ ...edits, inserts: [...edits.inserts, {}] })),
    toggleDeleteRow: (id, row) => get().toggleDeleteRows(id, [row]),
    toggleDeleteRows: (id, rows) =>
      changeEdits(id, (edits, loaded) => withToggledRows(edits, loaded, rows)),
    discardEdits: (id) => {
      const tab = get().tabs.find((t) => t.id === id);
      // Discard can be undone, like any other step.
      if (tab && editCount(tab.edits) > 0) changeEdits(id, () => null);
    },
    undoEdit: (id) =>
      patchTab(id, (tab) => {
        if (tab.saving || tab.editsPast.length === 0) return {};
        return {
          edits: tab.editsPast[tab.editsPast.length - 1] ?? null,
          editsPast: tab.editsPast.slice(0, -1),
          editsFuture: [tab.edits, ...tab.editsFuture],
        };
      }),
    redoEdit: (id) =>
      patchTab(id, (tab) => {
        if (tab.saving || tab.editsFuture.length === 0) return {};
        return {
          edits: tab.editsFuture[0] ?? null,
          editsPast: [...tab.editsPast, tab.edits],
          editsFuture: tab.editsFuture.slice(1),
        };
      }),
    saveEdits: (id, confirmed = false) => {
      const tab = get().tabs.find((t) => t.id === id);
      const set = tab?.sets[tab.activeSet];
      const readOnly = get().connections.find((c) => c.id === tab?.connId)?.read_only ?? true;
      if (!tab || !set?.columns || !tab.edits || tab.saving) return;
      const target = editTarget(tab, readOnly);
      if (!target) return;
      const { key } = target;
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
        table: target.table,
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
    copyInsert: (id, columns, rows) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (!tab || rows.length === 0) return;
      const readOnly = get().connections.find((c) => c.id === tab.connId)?.read_only ?? true;
      // The table the rows come from; a result from several tables gets a name to replace.
      const table = editTarget(tab, readOnly)?.table ??
        (tab.browse && tab.paged ? tab.browse.table : null) ??
        sourceTable(tab.sets[tab.activeSet], tab.database) ?? {
          database: null,
          schema: null,
          name: "table_name",
        };
      const request = nextRequest++;
      insertRequests.set(request, rows.length);
      send({ type: "InsertSql", request, conn: tab.connId, table, columns, rows });
    },

    schemaForm: null,
    openSchemaForm: (schemaForm) => set({ schemaForm }),
    schemaReview: null,
    alterSchema: (conn, change) => {
      const request = nextRequest++;
      schemaRequests.add(request);
      send({ type: "AlterSchema", request, conn, change, confirmed: false });
    },
    answerSchemaReview: (run) => {
      const review = get().schemaReview;
      if (!review || review.running) return;
      if (!run) {
        set({ schemaReview: null });
        return;
      }
      set({ schemaReview: { ...review, running: true, error: null } });
      send({
        type: "AlterSchema",
        request: review.request,
        conn: review.conn,
        change: review.change,
        confirmed: true,
      });
    },
    reloadObjects: (connId, database) => {
      // What the tree showed of the database's tables may be out of date.
      reloadOpenTables(connId, database);
      patchDb(connId, database, () => ({ loading: true, error: null }));
      send({ type: "LoadObjects", id: connId, database });
    },
    openStructure: (connId, database, name, part, focus) => {
      get().openObject(connId, database, name, "table");
      const tab = get().tabs.find(
        (t) => t.connId === connId && t.database === database && t.title === name,
      );
      if (!tab) return;
      get().setView(tab.id, "structure");
      patchTab(tab.id, (t) => ({
        structurePart: part ?? t.structurePart,
        structureFocus: focus ?? null,
      }));
    },

    palette: null,
    setPalette: (palette) => set({ palette }),
    transfer: (kind) =>
      get().toast("info", `${kind === "import" ? "Import" : "Export"} is not available yet`),
  };
});

// Save the open tabs about a second after they change. Result rows also change `tabs`, but the
// saved shape (titles, SQL, pages) stays the same, so `saveWorkspace` skips those writes.
useApp.subscribe((state, prev) => {
  if (state.tabs === prev.tabs && state.activeTabId === prev.activeTabId) return;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void saveWorkspace(), 1000);
});

/** Write `text` to the clipboard and say so; a refused write says that instead. */
export function copyText(text: string, message?: string): void {
  navigator.clipboard.writeText(text).then(
    () => {
      if (message) useApp.getState().toast("success", message);
    },
    (err: unknown) => useApp.getState().toast("error", `Couldn't copy: ${String(err)}`),
  );
}

/** `sets` with the saved edits of set `index` written into its rows (deleted rows removed). */
function withEdits(sets: ResultSet[], index: number, edits: Edits | null): ResultSet[] {
  if (!edits) return sets;
  return sets.map((rs, i) => {
    if (i !== index) return rs;
    const deleted = new Set(edits.deletes);
    const rows = rs.rows
      .map((row, r) => {
        const update = edits.updates[r];
        return update ? row.map((value, c) => update[c] ?? value) : row;
      })
      .filter((_, r) => !deleted.has(r));
    return { ...rs, rows };
  });
}

/** Pinned tabs first, each group keeping its order. */
function pinnedFirst(tabs: Tab[]): Tab[] {
  return [...tabs.filter((t) => t.pinned), ...tabs.filter((t) => !t.pinned)];
}
