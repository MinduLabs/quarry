import { Columns3, Download, Filter, Minus, Plus, RotateCw, Square } from "lucide-react";
import { memo, Profiler, type ReactNode, useCallback, useMemo, useState } from "react";
import type { Value } from "@/bindings/Value";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { DropdownMenuButton, type MenuItem } from "@/components/ui/Menu";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { onRender } from "@/lib/perf";
import { shortcutText } from "@/lib/shortcuts";
import { type Edits, editCount, editTarget, type Tab, useApp } from "@/store/app";
import { ConsoleView } from "../console/ConsoleView";
import { type GridEditing, type GridSorting, ResultGrid, type RowSpan } from "../grid/ResultGrid";
import { ExplainTree } from "./ExplainTree";
import { FilterBar } from "./FilterBar";
import { Pager } from "./Pager";
import { ResultTabs } from "./ResultTabs";

/** Pending edits shown when there are none yet; one object, so the grid's props stay equal. */
const NO_EDITS: Edits = { updates: {}, inserts: [], deletes: [] };

/**
 * The data view of one tab: its data bar (what acts on the rows), the grid, and the status bar
 * (what the rows are, and paging), under tabs for its result sets. In the results panel the
 * tab's output comes first; inside a table tab, or under a query's editor, there is no output and
 * a single result needs no tabs.
 */
export const ResultsPane = memo(function ResultsPane({
  tab,
  withOutput = false,
  barStart,
  barEnd,
  actions,
}: {
  tab: Tab;
  /** In the results panel: the output is the first tab. */
  withOutput?: boolean;
  /** Leading part of the status bar (the Data / Structure switch of a table tab). */
  barStart?: ReactNode;
  /** Trailing part of the data bar (a table tab's favorite, transaction and connection). */
  barEnd?: ReactNode;
  /** Buttons at the right of the result tabs. */
  actions?: ReactNode;
}) {
  const config = useApp((s) => s.connections.find((c) => c.id === tab.connId));
  const {
    editCell,
    addRow,
    editCells,
    requestDeleteRows,
    undoEdit,
    redoEdit,
    explain,
    toggleSort,
    copyInsert,
    transfer,
    setHiddenColumns,
  } = useApp.getState();
  const panelOutput = useApp((s) => s.panelOutput);
  const consoleOpen = withOutput && panelOutput;
  const target = editTarget(tab, config?.read_only ?? true);
  /** Rows the grid selection spans, for the data bar's Delete. */
  const [span, setSpan] = useState<RowSpan | null>(null);
  const [isFiltering, setFiltering] = useState(false);
  const runTexts = tab.runTexts;

  const active = tab.sets[tab.activeSet];
  const filters = tab.browse?.query.filters.length ?? 0;
  const showFilters = Boolean(tab.browse) && (isFiltering || filters > 0);
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
  const exportResult = useCallback(
    () =>
      transfer(
        "export",
        tab.browse && tab.paged
          ? { kind: "table", conn: tab.connId, table: tab.browse.table, query: tab.browse.query }
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

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s" && editCount(tab.edits) > 0) {
          e.preventDefault();
          useApp.getState().saveEdits(tab.id);
        }
      }}
    >
      {(withOutput || grids.filter(({ index }) => !tab.closedSets.includes(index)).length > 1) && (
        <ResultTabs
          tab={tab}
          grids={grids}
          label={(n) => resultLabel(grids, n, (tab.progress?.count ?? 1) > 1)}
          output={withOutput ? { active: consoleOpen } : undefined}
          actions={actions}
        />
      )}

      {tab.explain && !consoleOpen && (
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

      {/* Data bar: what acts on the rows, right above them. */}
      {!consoleOpen && (tab.browse || (active?.columns && planText === null)) && (
        <DataBar
          tab={tab}
          target={target}
          span={span}
          isFiltering={showFilters}
          onToggleFilters={() => setFiltering((f) => !f)}
          columnItems={active?.columns && planText === null ? columnItems : null}
          onExport={exportResult}
          end={barEnd}
        />
      )}

      {showFilters && !consoleOpen && (
        <FilterBar tab={tab} columns={filterColumns} onClose={() => setFiltering(false)} />
      )}

      {consoleOpen && (
        <div className="min-h-0 flex-1">
          <ConsoleView tab={tab.id} log={tab.log} />
        </div>
      )}

      {/* Results. Kept mounted under the output so the grid keeps its scroll and selection. */}
      <div className={cn("relative min-h-0 flex-1", consoleOpen && "hidden")}>
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

      {/* Status bar: everything about the result below the grid. */}
      <div className="flex h-9 shrink-0 items-center gap-3 border-line border-t bg-surface px-3 text-base text-muted">
        {barStart}
        {tab.running && <Spinner size={14} />}
        <span className="min-w-0 truncate">{status(tab, runTexts)}</span>
        <div className="flex-1" />
        {!consoleOpen && <Pager tab={tab} />}
      </div>
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
 * What acts on the rows shown, in one bar right above them: reload (stop while it runs), filter
 * and columns; adding and deleting rows; and, only while there are changes, discard and save.
 * Undo and redo are keys and the grid's menu, not buttons.
 */
function DataBar({
  tab,
  target,
  span,
  isFiltering,
  onToggleFilters,
  columnItems,
  onExport,
  end,
}: {
  tab: Tab;
  target: ReturnType<typeof editTarget>;
  span: RowSpan | null;
  isFiltering: boolean;
  onToggleFilters: () => void;
  /** The column visibility menu, when the rows have columns. */
  columnItems: MenuItem[] | null;
  /** Open the export dialog for what the data bar shows. */
  onExport: () => void;
  end?: ReactNode;
}) {
  const rows = span
    ? Array.from({ length: span.bottom - span.top + 1 }, (_, i) => span.top + i)
    : [];
  const { addRow, requestDeleteRows, discardEdits, saveEdits, refresh, cancel } = useApp.getState();
  const pending = editCount(tab.edits);
  const deletes = tab.edits?.deletes ?? [];
  const loaded = tab.sets[tab.activeSet]?.rows.length ?? 0;
  const keeping = rows.length > 0 && rows.every((r) => r < loaded && deletes.includes(r));
  const count = rows.length > 1 ? `${rows.length} rows` : "row";
  const busy = tab.running || tab.saving;
  const filters = tab.browse?.query.filters.length ?? 0;
  const hidden = tab.hiddenColumns.length;
  return (
    <div className="flex h-8 shrink-0 items-center gap-0.5 border-line border-b bg-surface px-1.5 text-sm">
      {tab.running || tab.counting ? (
        <IconButton label="Stop" size="sm" onClick={() => cancel(tab.id)}>
          <Square size={13} strokeWidth={1.75} />
        </IconButton>
      ) : (
        <IconButton
          label={tab.browse ? "Reload" : "Run again"}
          shortcut={["F5"]}
          size="sm"
          onClick={() => refresh(tab.id)}
        >
          <RotateCw size={14} strokeWidth={1.75} />
        </IconButton>
      )}
      {tab.browse && (
        <IconButton
          label={filters > 0 ? `Filters (${filters})` : "Filter rows"}
          size="sm"
          aria-pressed={isFiltering}
          className={cn(filters > 0 && "text-accent-text")}
          onClick={onToggleFilters}
        >
          <Filter size={14} strokeWidth={1.75} />
        </IconButton>
      )}
      {columnItems && (
        <DropdownMenuButton
          items={columnItems}
          trigger={
            <IconButton
              label={hidden > 0 ? `Columns (${hidden} hidden)` : "Columns"}
              size="sm"
              className={cn(hidden > 0 && "text-accent-text")}
            >
              <Columns3 size={14} strokeWidth={1.75} />
            </IconButton>
          }
        />
      )}
      <IconButton label="Export" size="sm" disabled={tab.running} onClick={onExport}>
        <Download size={14} strokeWidth={1.75} />
      </IconButton>
      {target && (
        <div className="ml-1 flex items-center gap-0.5 border-line border-l pl-1.5">
          {target.canInsert && (
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => addRow(tab.id)}>
              <Plus size={14} strokeWidth={1.75} />
              Row
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
        </div>
      )}
      {pending > 0 && (
        <div className="ml-1 flex items-center gap-1.5 border-line border-l pl-2">
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
      {target?.matchAll && (
        <Tooltip content="No primary key: rows are matched by all their values">
          <span className="ml-2 cursor-default text-faint">No key</span>
        </Tooltip>
      )}
      <div className="flex-1" />
      {end}
    </div>
  );
}
