import { closeCompletion } from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import {
  AlignLeft,
  Columns3,
  Ellipsis,
  Filter,
  History,
  ListVideo,
  Minus,
  Play,
  Plus,
  Redo2,
  Square,
  Star,
  Undo2,
} from "lucide-react";
import { memo, Profiler, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { SchemaObject } from "@/bindings/SchemaObject";
import type { TableColumns } from "@/bindings/TableColumns";
import type { Value } from "@/bindings/Value";
import { EnvBadge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { DropdownMenuButton, type MenuItem } from "@/components/ui/Menu";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { registerEditor } from "@/lib/editors";
import { scriptStatements, statementAt } from "@/lib/engine";
import { onRender } from "@/lib/perf";
import { keysOf, shortcutText } from "@/lib/shortcuts";
import { fillVariables, queryVariables } from "@/lib/variables";
import { type Edits, editCount, editTarget, type Tab, useApp } from "@/store/app";
import type { SqlSchema } from "../editor/completion";
import { formatSql } from "../editor/format";
import { type Span, setActiveStatement, setRunError, setRunStatements } from "../editor/runMarks";
import { flashRange, SqlEditor } from "../editor/SqlEditor";
import { SaveQueryDialog } from "../favorites/SaveQueryDialog";
import { type GridEditing, type GridSorting, ResultGrid, type RowSpan } from "../grid/ResultGrid";
import { StructureView } from "../structure/StructureView";
import { ExplainTree } from "./ExplainTree";
import { FilterBar } from "./FilterBar";
import { Pager } from "./Pager";
import { VariablesDialog } from "./VariablesDialog";

const ICON = { size: 16, strokeWidth: 1.75 } as const;

/** Table name → column names of one database. */
type DatabaseSchema = SqlSchema[string];

/** Completion schemas by the database's object and column lists (both come from the store). */
const schemas = new WeakMap<object, WeakMap<object, DatabaseSchema>>();
const NO_LIST: never[] = [];

/** Table name → column names for completion; the same object for the same lists. */
function schemaOf(objects: SchemaObject[] | null, columns: TableColumns[] | null): DatabaseSchema {
  const byObjects = objects ?? NO_LIST;
  const byColumns = columns ?? NO_LIST;
  let inner = schemas.get(byObjects);
  if (!inner) {
    inner = new WeakMap();
    schemas.set(byObjects, inner);
  }
  let schema = inner.get(byColumns);
  if (!schema) {
    schema = {};
    for (const o of byObjects) {
      if (o.kind === "table" || o.kind === "view") schema[o.name] = [];
    }
    for (const t of byColumns) schema[t.table] = t.columns;
    inner.set(byColumns, schema);
  }
  return schema;
}

/** Pending edits shown when there are none yet; one object, so the grid's props stay equal. */
const NO_EDITS: Edits = { updates: {}, inserts: [], deletes: [] };

/**
 * Toolbar, SQL editor, results and status bar for one tab. Memoized on `tab`: the store replaces only the tab that changed, so typing or streaming rows in
 * one tab never re-renders the others (every tab stays mounted).
 */
export const QueryView = memo(function QueryView({ tab }: { tab: Tab }) {
  const config = useApp((s) => s.connections.find((c) => c.id === tab.connId));
  const databases = useApp((s) => s.nodes[tab.connId]?.databases);
  const {
    setSql,
    run,
    cancel,
    setView,
    editCell,
    addRow,
    editCells,
    requestDeleteRows,
    undoEdit,
    redoEdit,
    saveEdits,
    explain,
    toggleSort,
    beginTransaction,
    endTransaction,
    copyInsert,
  } = useApp.getState();
  const connStatus = useApp((s) => s.nodes[tab.connId]?.status);
  const capabilities = connStatus?.kind === "connected" ? connStatus.capabilities : null;
  const structure = tab.browse !== null && tab.view === "structure";
  const target = editTarget(tab, config?.read_only ?? true);
  const pending = editCount(tab.edits);
  const editor = useRef<EditorView | null>(null);
  const [editorHeight, setEditorHeight] = useState(220);
  const [saving, setSaving] = useState(false);
  const [filtering, setFiltering] = useState(false);
  /** A query waiting for the values of its `{{variables}}`. */
  const [asking, setAsking] = useState<{
    sql: string;
    names: string[];
    then: (sql: string) => void;
  } | null>(null);
  /** The values typed last time, so running the same query again needs no retyping. */
  const lastValues = useRef<Record<string, string>>({});
  /** Rows the grid selection spans, for the edit bar's Delete. */
  const [span, setSpan] = useState<RowSpan | null>(null);

  useEffect(() => () => registerEditor(tab.id, null), [tab.id]);

  // Every database loaded so far completes (`other_db.table`); the tab's own without a prefix.
  // Each database's part is shared by every tab on it (`schemaOf`), so this only regroups them.
  const schema = useMemo(() => {
    const byDatabase: SqlSchema = {};
    for (const db of databases ?? []) {
      if (db.objects) byDatabase[db.name] = schemaOf(db.objects, db.columns);
    }
    return byDatabase;
  }, [databases]);

  // The tab's database completes even when the sidebar never opened it.
  const isConnected = connStatus?.kind === "connected";
  const hasDatabase = Boolean(databases?.some((d) => d.name === tab.database));
  useEffect(() => {
    if (isConnected && hasDatabase && tab.database) {
      useApp.getState().loadCompletion(tab.connId, tab.database);
    }
  }, [isConnected, hasDatabase, tab.connId, tab.database]);

  /** The selection, or the statement under the cursor (briefly highlighted), or with `all`
   * and no selection the whole text; with where it starts in the editor. */
  const currentText = useCallback(
    async (all: boolean): Promise<{ from: number; text: string } | null> => {
      const view = editor.current;
      if (!view) return null;
      closeCompletion(view);
      const selection = view.state.selection.main;
      if (!selection.empty) {
        return { from: selection.from, text: view.state.sliceDoc(selection.from, selection.to) };
      }
      const text = view.state.doc.toString();
      if (all) return { from: 0, text };
      const range = await statementAt(text, selection.head);
      if (!range) return null;
      flashRange(view, range[0], range[1]);
      return { from: range[0], text: text.slice(range[0], range[1]) };
    },
    [],
  );

  /** The statements of a run started from the editor, until its `QueryStarted` arrives. */
  const pendingRun = useRef<{ sql: string; spans: Span[]; texts: string[] } | null>(null);
  /** The statements of the run shown now, as typed, when it started from the editor. */
  const [runTexts, setRunTexts] = useState<string[]>([]);

  /** Run `sql` now, or ask for its `{{variables}}` first. */
  const withVariables = useCallback((sql: string, then: (sql: string) => void) => {
    const names = queryVariables(sql);
    if (names.length === 0) return then(sql);
    setAsking({ sql, names, then });
  }, []);

  const runFromEditor = useCallback(
    async (all: boolean) => {
      const current = await currentText(all);
      if (!current?.text.trim()) return;
      const { from, text } = current;
      const ranges = await scriptStatements(text);
      withVariables(text, (sql) => {
        pendingRun.current = {
          sql,
          spans: ranges.map(([start, end]) => ({ from: from + start, to: from + end })),
          texts: ranges.map(([start, end]) => text.slice(start, end)),
        };
        run(tab.id, sql);
      });
    },
    [tab.id, run, currentText, withVariables],
  );

  const explainFromEditor = useCallback(async () => {
    const current = await currentText(false);
    if (current) withVariables(current.text, (filled) => explain(tab.id, filled, true));
  }, [tab.id, explain, currentText, withVariables]);

  // A new run: point the editor at its statements when it came from the editor. A refresh runs
  // the same text again, so the marks of the run before still fit.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only a new run resets the marks
  useEffect(() => {
    const view = editor.current;
    if (!view || tab.runId === 0) return;
    const pending = pendingRun.current;
    const isTyped = !tab.paged && !tab.explain && pending?.sql === tab.lastRun;
    if (pending && isTyped) {
      pendingRun.current = null;
      view.dispatch({ effects: setRunStatements.of(pending.spans) });
      setRunTexts(pending.texts);
    } else if (!isTyped) {
      view.dispatch({ effects: setRunStatements.of([]) });
      setRunTexts([]);
    }
  }, [tab.runId]);

  const runningIndex = tab.running ? (tab.progress?.index ?? null) : null;
  useEffect(() => {
    editor.current?.dispatch({ effects: setActiveStatement.of(runningIndex) });
  }, [runningIndex]);

  useEffect(() => {
    const failure = tab.failure;
    editor.current?.dispatch({
      effects: setRunError.of(failure && tab.error ? { ...failure, message: tab.error } : null),
    });
  }, [tab.failure, tab.error]);

  const startResize = (event: React.PointerEvent) => {
    event.preventDefault();
    const startY = event.clientY;
    const start = editorHeight;
    const move = (e: PointerEvent) =>
      setEditorHeight(Math.max(80, Math.min(window.innerHeight - 220, start + e.clientY - startY)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const active = tab.sets[tab.activeSet];
  const filters = tab.browse?.query.filters.length ?? 0;
  const showFilters = Boolean(tab.browse) && !structure && (filtering || filters > 0);
  const filterColumns =
    tab.structure?.info?.columns.map((c) => c.name) ??
    active?.columns?.map((c) => c.source_column ?? c.name) ??
    [];
  const planText = (() => {
    if (tab.running) return null;
    if (tab.explain?.json) {
      const cell = active?.rows[0]?.[0];
      return cell && (cell.t === "text" || cell.t === "json") ? cell.v : null;
    }
    // EXPLAIN ANALYZE and EXPLAIN FORMAT=TREE typed in the editor come back as one column of
    // multi-line text. A grid cell would cut it to one line, so show it as plain text.
    const columns = active?.columns;
    if (columns?.length !== 1 || columns[0]?.name.toUpperCase() !== "EXPLAIN") return null;
    if (!active || active.rows.length === 0) return null;
    const lines: string[] = [];
    for (const row of active.rows) {
      const cell = row[0];
      if (cell?.t !== "text") return null;
      lines.push(cell.v);
    }
    return lines.join("\n");
  })();
  const grids = tab.sets.map((set, index) => ({ set, index })).filter(({ set }) => set.columns);
  const { transfer } = useApp.getState();
  const exportResult = useCallback(
    () =>
      transfer(
        "export",
        tab.browse && tab.paged
          ? { kind: "table", conn: tab.connId, table: tab.browse.table }
          : { kind: "result", tab: tab.id },
      ),
    [tab.id, tab.connId, tab.browse, tab.paged, transfer],
  );
  // The grid is memoized: its props only change when what they show changes, so typing in the
  // editor or moving the toolbar never re-renders the visible cells.
  const editable = target !== null && !tab.running;
  const canInsert = target?.canInsert ?? false;
  const editing = useMemo<GridEditing | undefined>(
    () =>
      editable
        ? {
            edits: tab.edits ?? NO_EDITS,
            onEdit: (row, col, value) => editCell(tab.id, row, col, value),
            onEditCells: (cells) => editCells(tab.id, cells),
            onDeleteRows: (rows) => requestDeleteRows(tab.id, rows),
            onUndo: () => undoEdit(tab.id),
            onRedo: () => redoEdit(tab.id),
            onInsertRow: canInsert ? () => addRow(tab.id) : undefined,
          }
        : undefined,
    [
      editable,
      canInsert,
      tab.edits,
      tab.id,
      editCell,
      editCells,
      requestDeleteRows,
      undoEdit,
      redoEdit,
      addRow,
    ],
  );
  const sort = tab.browse && tab.paged ? tab.browse.query.sort : null;
  const sorting = useMemo<GridSorting | undefined>(
    () => (sort ? { sort, onSort: (column) => toggleSort(tab.id, column) } : undefined),
    [sort, tab.id, toggleSort],
  );
  const onCopyInsert = useCallback(
    (names: string[], rows: Value[][]) => copyInsert(tab.id, names, rows),
    [tab.id, copyInsert],
  );

  // Hidden columns are kept by name, so they stay hidden on the next page or run.
  const shownColumns = active?.columns;
  const hiddenNames = tab.hiddenColumns;
  const hidden = useMemo(() => {
    const names = new Set(hiddenNames);
    return new Set(
      (shownColumns ?? []).flatMap((column, i) => (names.has(column.name) ? [i] : [])),
    );
  }, [shownColumns, hiddenNames]);
  const { setHiddenColumns } = useApp.getState();
  const columnItems: MenuItem[] = [
    {
      label: "Show all",
      disabled: hiddenNames.length === 0,
      onSelect: () => setHiddenColumns(tab.id, []),
    },
    ...(shownColumns ?? []).map((column, i) => ({
      label: column.name,
      checked: !hidden.has(i),
      separatorBefore: i === 0,
      onSelect: () =>
        setHiddenColumns(
          tab.id,
          hidden.has(i)
            ? hiddenNames.filter((name) => name !== column.name)
            : [...hiddenNames, column.name],
        ),
    })),
  ];

  // Actions used now and then: one menu instead of a button each.
  const moreItems: MenuItem[] = [
    ...(capabilities?.explain && !structure
      ? [
          {
            label: "Explain plan",
            disabled: tab.running,
            onSelect: () => void explainFromEditor(),
          },
        ]
      : []),
    ...(capabilities?.transactions && !tab.txOpen
      ? [
          {
            label: "Begin transaction",
            disabled: tab.running,
            onSelect: () => beginTransaction(tab.id),
          },
        ]
      : []),
    {
      label: "Export…",
      separatorBefore: true,
      disabled: tab.running || !(active?.columns || tab.browse),
      onSelect: exportResult,
    },
    ...(target?.canInsert && tab.browse
      ? [
          {
            label: "Import…",
            disabled: tab.running,
            onSelect: () =>
              tab.browse &&
              transfer("import", { kind: "table", conn: tab.connId, table: tab.browse.table }),
          },
        ]
      : []),
  ];

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s" && pending > 0) {
          e.preventDefault();
          saveEdits(tab.id);
        }
      }}
    >
      {/* Staging and production tabs carry their color along the top edge. */}
      {config && config.environment !== "local" && (
        <div
          aria-hidden
          className={cn(
            "h-0.5 shrink-0",
            config.environment === "production" ? "bg-env-production" : "bg-env-staging",
          )}
        />
      )}
      {/* Toolbar */}
      <div className="flex h-9 shrink-0 items-center gap-0.5 border-line border-b bg-surface px-2">
        <IconButton
          label="Run statement"
          shortcut={keysOf("run")}
          disabled={tab.running}
          onClick={() => runFromEditor(false)}
        >
          <Play {...ICON} className="text-accent-text" />
        </IconButton>
        <IconButton
          label="Run all"
          shortcut={keysOf("runAll")}
          disabled={tab.running}
          onClick={() => runFromEditor(true)}
        >
          <ListVideo {...ICON} />
        </IconButton>
        <IconButton
          label="Stop"
          disabled={!tab.running && !tab.counting}
          onClick={() => cancel(tab.id)}
        >
          <Square {...ICON} size={14} />
        </IconButton>
        {!structure && (
          <IconButton
            label="Format SQL"
            shortcut={keysOf("format")}
            disabled={!tab.sql.trim()}
            onClick={() => {
              const view = editor.current;
              if (!view) return;
              formatSql(view);
              view.focus();
            }}
          >
            <AlignLeft {...ICON} />
          </IconButton>
        )}
        {!structure && (
          <IconButton
            label="Query history…"
            shortcut={keysOf("history")}
            onClick={() => useApp.getState().setHistoryOpen(true)}
          >
            <History {...ICON} />
          </IconButton>
        )}
        {capabilities?.transactions &&
          (tab.txOpen ? (
            <div className="ml-1 flex items-center gap-1.5">
              <span className="font-medium text-sm text-warning">In transaction</span>
              <Button
                size="sm"
                variant="primary"
                disabled={tab.running}
                onClick={() => endTransaction(tab.id, true)}
              >
                Commit
              </Button>
              <Button
                size="sm"
                disabled={tab.running}
                onClick={() => endTransaction(tab.id, false)}
              >
                Roll back
              </Button>
            </div>
          ) : null)}
        {tab.browse && !structure && (
          <IconButton
            label={filters > 0 ? `Filters (${filters})` : "Filter rows"}
            aria-pressed={showFilters}
            className={cn("ml-1", filters > 0 && "text-accent-text")}
            onClick={() => setFiltering((f) => !f)}
          >
            <Filter {...ICON} />
          </IconButton>
        )}
        {active?.columns && !structure && planText === null && (
          <DropdownMenuButton
            items={columnItems}
            trigger={
              <IconButton
                label={
                  tab.hiddenColumns.length > 0
                    ? `Columns (${tab.hiddenColumns.length} hidden)`
                    : "Columns"
                }
                className={cn(tab.hiddenColumns.length > 0 && "text-accent-text")}
              >
                <Columns3 {...ICON} />
              </IconButton>
            }
          />
        )}
        <div className="flex-1" />
        <FavoriteButton tab={tab} onSaveQuery={() => setSaving(true)} />
        <DropdownMenuButton
          items={moreItems}
          trigger={
            <IconButton label="More">
              <Ellipsis {...ICON} />
            </IconButton>
          }
        />
        <div className="flex min-w-0 items-center gap-2 pr-1 text-sm text-muted">
          {config && <EnvBadge environment={config.environment} />}
          <span className="truncate">
            {config?.name}
            {tab.database && <span className="text-faint"> · {tab.database}</span>}
          </span>
        </div>
      </div>

      {structure && (
        <div className="min-h-0 flex-1">
          <StructureView tab={tab} />
        </div>
      )}

      {/* Editor. Kept mounted in the Structure view so its text and undo history survive. */}
      <div style={{ height: editorHeight }} className={cn("shrink-0", structure && "hidden")}>
        <SqlEditor
          initialValue={tab.sql}
          schema={schema}
          database={tab.database}
          onChange={(sql) => setSql(tab.id, sql)}
          onRun={(all) => void runFromEditor(all)}
          onReady={(view) => {
            editor.current = view;
            registerEditor(tab.id, view);
          }}
        />
      </div>

      {/* Split handle */}
      <div
        role="separator"
        aria-orientation="horizontal"
        onPointerDown={startResize}
        className={cn(
          "group relative h-px shrink-0 cursor-row-resize bg-line",
          structure && "hidden",
        )}
      >
        <div className="absolute inset-x-0 -top-1 h-2 transition-colors-fast group-hover:bg-accent-soft" />
      </div>

      {showFilters && (
        <FilterBar tab={tab} columns={filterColumns} onClose={() => setFiltering(false)} />
      )}

      {tab.explain && !structure && (
        <div className="flex h-8 shrink-0 items-center gap-2 border-line border-b bg-surface px-2 text-sm text-muted">
          <span>Query plan</span>
          <SegmentedTabs
            label="Plan view"
            value={tab.explain.json ? "tree" : "table"}
            onChange={(view) => tab.explain && explain(tab.id, tab.explain.sql, view === "tree")}
            size="sm"
            className="w-32"
            tabs={[
              { value: "tree", label: "Tree" },
              { value: "table", label: "Table" },
            ]}
          />
        </div>
      )}

      {/* Edit bar: right above the grid, where the eye is while editing. */}
      {target && !structure && active?.columns && planText === null && (
        <EditBar tab={tab} canInsert={target.canInsert} matchAll={target.matchAll} span={span} />
      )}

      {/* Results */}
      <div className={cn("relative min-h-0 flex-1", structure && "hidden")}>
        {planText !== null && <ExplainTree text={planText} />}
        {tab.running && tab.sets.length === 0 && (
          <div className="animate-fade flex h-full flex-col items-center justify-center gap-2">
            <Spinner size={18} />
            {tab.progress && (
              <p className="max-w-full truncate px-4 text-muted text-sm">
                {statementLabel(tab.progress.index, tab.progress.count, runTexts)}
              </p>
            )}
          </div>
        )}
        {!tab.running && tab.error && (
          <div className="animate-fade h-full overflow-auto p-4">
            {tab.failure?.statement != null && tab.progress && (
              <p className="mb-2 truncate text-muted text-sm">
                Failed at{" "}
                {statementLabel(tab.failure.statement, tab.progress.count, runTexts, "statement")}
              </p>
            )}
            <pre className="whitespace-pre-wrap font-mono text-danger text-sm">{tab.error}</pre>
          </div>
        )}
        {active?.columns && planText === null && (
          <Profiler id="grid" onRender={onRender}>
            <ResultGrid
              columns={active.columns}
              rows={active.rows}
              resetKey={tab.runId * 100 + tab.activeSet}
              editing={editing}
              sorting={sorting}
              onCopyInsert={onCopyInsert}
              onExport={exportResult}
              onSelectRows={setSpan}
              hidden={hidden}
            />
          </Profiler>
        )}
        {!tab.running && !tab.error && tab.sets.length > 0 && grids.length === 0 && (
          <p className="animate-fade p-4 text-base text-muted">
            {plural(tab.sets.length, "statement")} executed ·{" "}
            {plural(
              tab.sets.reduce((n, s) => n + s.affected, 0),
              "row",
            )}{" "}
            affected
          </p>
        )}
        {!tab.running && !tab.error && tab.sets.length === 0 && !tab.browse && (
          <p className="flex h-full items-center justify-center text-base text-faint">
            Run a query to see results
          </p>
        )}
        {!tab.running && !tab.error && tab.sets.length === 0 && tab.browse && (
          <div className="flex h-full items-center justify-center">
            <Button
              size="sm"
              onClick={() => useApp.getState().browse(tab.id, tab.browse?.offset ?? 0)}
            >
              Load rows
            </Button>
          </div>
        )}
      </div>

      {/* Results bar: everything about the result below the grid, the SQL actions stay above. */}
      <div className="flex h-9 shrink-0 items-center gap-3 border-line border-t bg-surface px-3 text-base text-muted">
        {tab.browse && (
          <SegmentedTabs
            label="View"
            value={tab.view}
            onChange={(view) => setView(tab.id, view)}
            className="w-40 shrink-0"
            tabs={[
              { value: "data", label: "Data" },
              { value: "structure", label: "Structure" },
            ]}
          />
        )}
        {!structure && grids.length > 1 && (
          <div className="flex shrink-0 items-center gap-0.5">
            {grids.map(({ set, index }, n) => (
              <button
                key={index}
                type="button"
                title={runTexts[set.statement] && shorten(runTexts[set.statement] ?? "", 200)}
                onClick={() => useApp.getState().setActiveSet(tab.id, index)}
                className={cn(
                  "h-7 rounded-md px-2 text-base transition-colors-fast",
                  index === tab.activeSet ? "bg-selected text-fg" : "hover:bg-hover",
                )}
              >
                {resultLabel(grids, n, (tab.progress?.count ?? 1) > 1)}
              </button>
            ))}
          </div>
        )}
        {tab.running && <Spinner size={14} />}
        <span className="min-w-0 truncate">{status(tab, runTexts)}</span>
        <div className="flex-1" />
        {!structure && <Pager tab={tab} />}
      </div>
      <SaveQueryDialog tabId={tab.id} open={saving} onClose={() => setSaving(false)} />
      {asking && (
        <VariablesDialog
          names={asking.names}
          initial={lastValues.current}
          onClose={() => setAsking(null)}
          onRun={(values) => {
            lastValues.current = { ...lastValues.current, ...values };
            const { sql, then } = asking;
            setAsking(null);
            then(fillVariables(sql, values));
          }}
        />
      )}
    </div>
  );
});

function plural(n: number, word: string): string {
  return `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;
}

/** Whitespace folded, cut to `max` characters. */
function shorten(sql: string, max: number): string {
  const flat = sql.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

/** "Statement 2 of 5 · SELECT …", or the statement alone when the run had one. */
function statementLabel(index: number, count: number, texts: string[], word = "Statement"): string {
  const short = shorten(texts[index] ?? "", 60);
  if (count <= 1) return short;
  const which = `${word} ${index + 1} of ${count}`;
  return short ? `${which} · ${short}` : which;
}

/** "Result 2", or the statement that made it when the run had several. */
function resultLabel(
  grids: { set: { statement: number }; index: number }[],
  n: number,
  isScript: boolean,
): string {
  const set = grids[n]?.set;
  if (!set || !isScript) return `Result ${n + 1}`;
  const same = grids.filter((g) => g.set.statement === set.statement);
  const part =
    same.length > 1 ? ` · ${same.findIndex((g) => g.index === grids[n]?.index) + 1}` : "";
  return `Statement ${set.statement + 1}${part}`;
}

function status(tab: Tab, texts: string[]): string {
  if (tab.running) {
    if (!tab.progress) return "Running…";
    const label = statementLabel(tab.progress.index, tab.progress.count, texts, "statement");
    return label ? `Running ${label}` : "Running…";
  }
  if (tab.error) return tab.error.split("\n")[0] ?? "";
  if (tab.elapsedMs === null) return "";
  const time =
    tab.elapsedMs >= 1000 ? `${(tab.elapsedMs / 1000).toFixed(2)} s` : `${tab.elapsedMs} ms`;
  const set = tab.sets[tab.activeSet];
  if (!set?.columns) return time;
  if (tab.paged) return time;
  const more = set.truncated ? " (first rows only)" : "";
  return `${plural(set.rows.length, "row")}${more} · ${time}`;
}

/**
 * The star: on a table tab it pins the table in Favorites (filled once pinned, click again to
 * unpin); on a query tab it saves the SQL as a favorite (filled when this SQL is saved).
 */
function FavoriteButton({ tab, onSaveQuery }: { tab: Tab; onSaveQuery: () => void }) {
  const table = tab.browse?.table.name ?? null;
  const favorite = useApp((s) =>
    s.favorites.find((f) =>
      table !== null
        ? f.conn === tab.connId &&
          f.database === tab.database &&
          f.item.kind === "table" &&
          f.item.table === table
        : f.conn === tab.connId && f.item.kind === "query" && f.item.sql === tab.sql,
    ),
  );
  const { addTableFavorite, deleteFavorite, toast } = useApp.getState();
  const label =
    table !== null
      ? favorite
        ? "Remove from favorites"
        : "Add table to favorites"
      : favorite
        ? "Saved in favorites"
        : "Save query…";
  return (
    <IconButton
      label={label}
      aria-pressed={Boolean(favorite)}
      onClick={() => {
        if (table !== null) {
          if (favorite) deleteFavorite(favorite.id);
          else addTableFavorite(tab.connId, tab.database ?? "", table);
        } else if (!tab.sql.trim()) {
          toast("info", "Write a query first, then save it");
        } else {
          onSaveQuery();
        }
      }}
    >
      <Star
        {...ICON}
        fill={favorite ? "currentColor" : "none"}
        className={cn(favorite && "text-warning")}
      />
    </IconButton>
  );
}

/** Add and delete rows, then save or discard: one group, above the grid. */
function EditBar({
  tab,
  canInsert,
  matchAll,
  span,
}: {
  tab: Tab;
  canInsert: boolean;
  matchAll: boolean;
  span: RowSpan | null;
}) {
  const rows = span
    ? Array.from({ length: span.bottom - span.top + 1 }, (_, i) => span.top + i)
    : [];
  const { addRow, requestDeleteRows, discardEdits, saveEdits, undoEdit, redoEdit } =
    useApp.getState();
  const pending = editCount(tab.edits);
  const deletes = tab.edits?.deletes ?? [];
  const loaded = tab.sets[tab.activeSet]?.rows.length ?? 0;
  const keeping = rows.length > 0 && rows.every((r) => r < loaded && deletes.includes(r));
  const count = rows.length > 1 ? `${rows.length} rows` : "row";
  const busy = tab.running || tab.saving;
  return (
    <div className="flex h-8 shrink-0 items-center gap-1 border-line border-b bg-surface px-2 text-sm">
      {canInsert && (
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => addRow(tab.id)}>
          <Plus size={14} strokeWidth={1.75} />
          Add row
        </Button>
      )}
      <Button
        size="sm"
        variant={keeping ? "ghost" : "danger-ghost"}
        disabled={busy || rows.length === 0}
        title={rows.length === 0 ? "Select rows in the grid first" : undefined}
        onClick={() => requestDeleteRows(tab.id, rows)}
      >
        <Minus size={14} strokeWidth={1.75} />
        {keeping ? `Keep ${count}` : `Delete ${count}`}
      </Button>
      <IconButton
        label="Undo change"
        shortcut={keysOf("undoEdit")}
        size="sm"
        disabled={busy || tab.editsPast.length === 0}
        onClick={() => undoEdit(tab.id)}
      >
        <Undo2 size={14} strokeWidth={1.75} />
      </IconButton>
      <IconButton
        label="Redo change"
        shortcut={keysOf("redoEdit")}
        size="sm"
        disabled={busy || tab.editsFuture.length === 0}
        onClick={() => redoEdit(tab.id)}
      >
        <Redo2 size={14} strokeWidth={1.75} />
      </IconButton>
      {pending > 0 && (
        <div className="ml-2 flex items-center gap-1.5 border-line border-l pl-3">
          <span className="text-fg">
            {pending} {pending === 1 ? "change" : "changes"}
          </span>
          <Button size="sm" disabled={tab.saving} onClick={() => discardEdits(tab.id)}>
            Discard
          </Button>
          <Button
            size="sm"
            variant="primary"
            disabled={tab.saving}
            title={`Save (${shortcutText("saveChanges")})`}
            onClick={() => saveEdits(tab.id)}
          >
            {tab.saving ? "Saving…" : "Save"}
          </Button>
        </div>
      )}
      <div className="flex-1" />
      {matchAll && (
        <Tooltip content="No primary key: rows are matched by all their values">
          <span className="cursor-default text-faint">No key</span>
        </Tooltip>
      )}
    </div>
  );
}
