import { closeCompletion } from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import {
  AlignLeft,
  Ellipsis,
  Filter,
  History,
  ListVideo,
  Minus,
  Play,
  Plus,
  Square,
  Star,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EnvBadge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { DropdownMenuButton, type MenuItem } from "@/components/ui/Menu";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { registerEditor } from "@/lib/editors";
import { statementAt } from "@/lib/engine";
import { keysOf, shortcutText } from "@/lib/shortcuts";
import { editCount, editTarget, type Tab, useApp } from "@/store/app";
import { formatSql } from "../editor/format";
import { flashRange, SqlEditor } from "../editor/SqlEditor";
import { SaveQueryDialog } from "../favorites/SaveQueryDialog";
import { ResultGrid } from "../grid/ResultGrid";
import { StructureView } from "../structure/StructureView";
import { ExplainTree } from "./ExplainTree";
import { FilterBar } from "./FilterBar";
import { Pager } from "./Pager";

const ICON = { size: 16, strokeWidth: 1.75 } as const;

/** Toolbar + SQL editor + results + status bar for one tab. */
export function QueryView({ tab }: { tab: Tab }) {
  const config = useApp((s) => s.connections.find((c) => c.id === tab.connId));
  const db = useApp((s) => s.nodes[tab.connId]?.databases.find((d) => d.name === tab.database));
  const {
    setSql,
    run,
    cancel,
    setView,
    editCell,
    addRow,
    toggleDeleteRow,
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
  /** Rows the grid selection spans, for the edit bar's Delete. */
  const [rows, setRows] = useState<number[]>([]);

  useEffect(() => () => registerEditor(tab.id, null), [tab.id]);

  const objects = db?.objects;
  const columns = db?.columns;
  const schema = useMemo(() => {
    const byTable: Record<string, string[]> = {};
    for (const o of objects ?? []) {
      if (o.kind === "table" || o.kind === "view") byTable[o.name] = [];
    }
    for (const t of columns ?? []) byTable[t.table] = t.columns;
    return byTable;
  }, [objects, columns]);

  /** The selection, or the statement under the cursor (briefly highlighted). */
  const currentStatement = useCallback(async (): Promise<string | null> => {
    const view = editor.current;
    if (!view) return null;
    closeCompletion(view);
    const selection = view.state.selection.main;
    if (!selection.empty) return view.state.sliceDoc(selection.from, selection.to);
    const text = view.state.doc.toString();
    const range = await statementAt(text, selection.head);
    if (!range) return null;
    flashRange(view, range[0], range[1]);
    return text.slice(range[0], range[1]);
  }, []);

  const runFromEditor = useCallback(
    async (all: boolean) => {
      const view = editor.current;
      if (!view) return;
      if (all && view.state.selection.main.empty) return run(tab.id, view.state.doc.toString());
      const sql = await currentStatement();
      if (sql) run(tab.id, sql);
    },
    [tab.id, run, currentStatement],
  );

  const explainFromEditor = useCallback(async () => {
    const sql = await currentStatement();
    if (sql) explain(tab.id, sql, true);
  }, [tab.id, explain, currentStatement]);

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
    if (!tab.explain?.json || tab.running) return null;
    const cell = active?.rows[0]?.[0];
    return cell && (cell.t === "text" || cell.t === "json") ? cell.v : null;
  })();
  const grids = tab.sets.map((set, index) => ({ set, index })).filter(({ set }) => set.columns);
  const { transfer } = useApp.getState();
  const exportResult = () =>
    transfer(
      "export",
      tab.browse && tab.paged
        ? { kind: "table", conn: tab.connId, table: tab.browse.table }
        : { kind: "result", tab: tab.id },
    );
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

      {showFilters && <FilterBar tab={tab} columns={filterColumns} />}

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
        <EditBar tab={tab} canInsert={target.canInsert} matchAll={target.matchAll} rows={rows} />
      )}

      {/* Results */}
      <div className={cn("relative min-h-0 flex-1", structure && "hidden")}>
        {planText !== null && <ExplainTree text={planText} />}
        {tab.running && tab.sets.length === 0 && (
          <div className="animate-fade flex h-full items-center justify-center">
            <Spinner size={18} />
          </div>
        )}
        {!tab.running && tab.error && (
          <pre className="animate-fade h-full overflow-auto whitespace-pre-wrap p-4 font-mono text-danger text-sm">
            {tab.error}
          </pre>
        )}
        {active?.columns && planText === null && (
          <ResultGrid
            columns={active.columns}
            rows={active.rows}
            resetKey={tab.runId * 100 + tab.activeSet}
            editing={
              target && !tab.running
                ? {
                    edits: tab.edits ?? { updates: {}, inserts: [], deletes: [] },
                    onEdit: (row, col, value) => editCell(tab.id, row, col, value),
                    onToggleDelete: (row) => toggleDeleteRow(tab.id, row),
                    onInsertRow: target.canInsert ? () => addRow(tab.id) : undefined,
                  }
                : undefined
            }
            sorting={
              tab.browse && tab.paged
                ? {
                    sort: tab.browse.query.sort,
                    onSort: (column) => toggleSort(tab.id, column),
                  }
                : undefined
            }
            onCopyInsert={(names, rows) => copyInsert(tab.id, names, rows)}
            onExport={exportResult}
            onSelectRows={setRows}
          />
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
      <div className="flex h-8 shrink-0 items-center gap-3 border-line border-t bg-surface px-3 text-sm text-muted">
        {tab.browse && (
          <SegmentedTabs
            label="View"
            value={tab.view}
            onChange={(view) => setView(tab.id, view)}
            size="sm"
            className="w-36 shrink-0"
            tabs={[
              { value: "data", label: "Data" },
              { value: "structure", label: "Structure" },
            ]}
          />
        )}
        {!structure && grids.length > 1 && (
          <div className="flex shrink-0 items-center gap-0.5">
            {grids.map(({ index }, n) => (
              <button
                key={index}
                type="button"
                onClick={() => useApp.getState().setActiveSet(tab.id, index)}
                className={cn(
                  "h-6 rounded-md px-2 text-sm transition-colors-fast",
                  index === tab.activeSet ? "bg-selected text-fg" : "hover:bg-hover",
                )}
              >
                Result {n + 1}
              </button>
            ))}
          </div>
        )}
        {tab.running && <Spinner size={11} />}
        <span className="min-w-0 truncate">{status(tab)}</span>
        <div className="flex-1" />
        {!structure && <Pager tab={tab} />}
      </div>
      <SaveQueryDialog tabId={tab.id} open={saving} onClose={() => setSaving(false)} />
    </div>
  );
}

function plural(n: number, word: string): string {
  return `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;
}

function status(tab: Tab): string {
  if (tab.running) return "Running…";
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
  rows,
}: {
  tab: Tab;
  canInsert: boolean;
  matchAll: boolean;
  rows: number[];
}) {
  const { addRow, toggleDeleteRow, discardEdits, saveEdits } = useApp.getState();
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
        variant="ghost"
        disabled={busy || rows.length === 0}
        title={rows.length === 0 ? "Select rows in the grid first" : undefined}
        onClick={() => {
          // New rows are removed by position, so go from the bottom up.
          for (const row of [...rows].reverse()) toggleDeleteRow(tab.id, row);
        }}
      >
        <Minus size={14} strokeWidth={1.75} />
        {keeping ? `Keep ${count}` : `Delete ${count}`}
      </Button>
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
