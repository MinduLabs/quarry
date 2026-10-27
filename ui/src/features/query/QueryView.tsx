import { closeCompletion } from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import {
  AlignLeft,
  Ellipsis,
  History,
  ListTree,
  ListVideo,
  Play,
  Square,
  Star,
} from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { SchemaObject } from "@/bindings/SchemaObject";
import type { TableColumns } from "@/bindings/TableColumns";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { DropdownMenuButton, type MenuItem } from "@/components/ui/Menu";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { cn } from "@/lib/cn";
import { registerEditor } from "@/lib/editors";
import { scriptStatements, statementAt } from "@/lib/engine";
import { keysOf } from "@/lib/shortcuts";
import { fillVariables, queryVariables } from "@/lib/variables";
import { type Tab, useApp } from "@/store/app";
import type { SqlSchema } from "../editor/completion";
import { formatSql } from "../editor/format";
import { type Span, setActiveStatement, setRunError, setRunStatements } from "../editor/runMarks";
import { flashRange, SqlEditor } from "../editor/SqlEditor";
import { SaveQueryDialog } from "../favorites/SaveQueryDialog";
import { StructureView } from "../structure/StructureView";
import { ResultsPane } from "./ResultsPane";
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

/**
 * One tab. A table tab shows its data (or structure) under a toolbar; a query tab shows its SQL
 * editor, with the results in the panel at the bottom or, when asked, under the editor.
 * Memoized on `tab`: the store replaces only the tab that changed, so typing or streaming rows
 * in one tab never re-renders the others (every tab stays mounted).
 */
export const QueryView = memo(function QueryView({ tab }: { tab: Tab }) {
  const config = useApp((s) => s.connections.find((c) => c.id === tab.connId));
  const databases = useApp((s) => s.nodes[tab.connId]?.databases);
  const {
    setSql,
    run,
    cancel,
    setView,
    explain,
    beginTransaction,
    endTransaction,
    setInlineResults,
    setRunTexts,
  } = useApp.getState();
  const connStatus = useApp((s) => s.nodes[tab.connId]?.status);
  const capabilities = connStatus?.kind === "connected" ? connStatus.capabilities : null;
  const structure = tab.browse !== null && tab.view === "structure";
  const isTable = tab.browse !== null;
  const editor = useRef<EditorView | null>(null);
  const [editorHeight, setEditorHeight] = useState(220);
  const [saving, setSaving] = useState(false);
  /** A query waiting for the values of its `{{variables}}`. */
  const [asking, setAsking] = useState<{
    sql: string;
    names: string[];
    then: (sql: string) => void;
  } | null>(null);
  /** The values typed last time, so running the same query again needs no retyping. */
  const lastValues = useRef<Record<string, string>>({});

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
      setRunTexts(tab.id, pending.texts);
    } else if (!isTyped) {
      view.dispatch({ effects: setRunStatements.of([]) });
      setRunTexts(tab.id, []);
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

  const viewSwitch = isTable && (
    <SegmentedTabs
      label="View"
      value={structure ? "structure" : "data"}
      onChange={(view) => setView(tab.id, view)}
      className="w-40 shrink-0"
      tabs={[
        { value: "data", label: "Data" },
        { value: "structure", label: "Structure" },
      ]}
    />
  );

  /** The schema the tab runs against. The environment shows as the line along the top. */
  const scope = tab.database && (
    <span className="ml-1 min-w-0 truncate pr-1 text-muted text-sm">{tab.database}</span>
  );
  /** The session's open transaction: shown only while there is one. */
  const transaction = capabilities?.transactions && tab.txOpen && (
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
      <Button size="sm" disabled={tab.running} onClick={() => endTransaction(tab.id, false)}>
        Roll back
      </Button>
    </div>
  );

  // What a query tab does now and then: one menu instead of a button each.
  const moreItems: MenuItem[] = [
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
      label: tab.inlineResults
        ? "Show results in the bottom panel"
        : "Show results under the editor",
      onSelect: () => setInlineResults(tab.id, !tab.inlineResults),
    },
  ];

  return (
    <div className="flex h-full min-h-0 flex-col">
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

      {/* A table tab is its data: the data bar carries the tab's own controls at its end. */}
      {isTable && !structure && (
        <div className="min-h-0 flex-1">
          <ResultsPane
            tab={tab}
            barStart={viewSwitch}
            barEnd={
              <>
                <FavoriteButton tab={tab} onSaveQuery={() => setSaving(true)} size="sm" />
                {transaction}
                {scope}
              </>
            }
          />
        </div>
      )}
      {isTable && structure && (
        <>
          <div className="min-h-0 flex-1">
            <StructureView tab={tab} />
          </div>
          <div className="flex h-9 shrink-0 items-center gap-3 border-line border-t bg-surface px-3">
            {viewSwitch}
            <div className="flex-1" />
            {scope}
          </div>
        </>
      )}

      {!isTable && (
        <>
          {/* Editor bar: what acts on the SQL. */}
          <div className="flex h-9 shrink-0 items-center gap-0.5 border-line border-b bg-surface px-2">
            {tab.running ? (
              <IconButton label="Stop" onClick={() => cancel(tab.id)}>
                <Square {...ICON} size={14} className="text-danger" />
              </IconButton>
            ) : (
              <IconButton
                label="Run statement"
                shortcut={keysOf("run")}
                onClick={() => runFromEditor(false)}
              >
                <Play {...ICON} className="text-accent-text" />
              </IconButton>
            )}
            <IconButton
              label="Run all"
              shortcut={keysOf("runAll")}
              disabled={tab.running}
              onClick={() => runFromEditor(true)}
            >
              <ListVideo {...ICON} />
            </IconButton>
            <div aria-hidden className="mx-1 h-4 w-px bg-line" />
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
            <FavoriteButton tab={tab} onSaveQuery={() => setSaving(true)} />
            <div aria-hidden className="mx-1 h-4 w-px bg-line" />
            {capabilities?.explain && (
              <IconButton
                label="Explain plan"
                disabled={tab.running}
                onClick={() => void explainFromEditor()}
              >
                <ListTree {...ICON} />
              </IconButton>
            )}
            <IconButton
              label="Query history…"
              shortcut={keysOf("history")}
              onClick={() => useApp.getState().setHistoryOpen(true)}
            >
              <History {...ICON} />
            </IconButton>
            {transaction}
            <div className="flex-1" />
            <DropdownMenuButton
              items={moreItems}
              trigger={
                <IconButton label="More">
                  <Ellipsis {...ICON} />
                </IconButton>
              }
            />
            {scope}
          </div>

          {/* Editor: the whole tab, or the top part when the results sit under it. */}
          <div
            style={tab.inlineResults ? { height: editorHeight } : undefined}
            className={cn(tab.inlineResults ? "shrink-0" : "min-h-0 flex-1")}
          >
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
          {tab.inlineResults && (
            <>
              <div
                role="separator"
                aria-orientation="horizontal"
                onPointerDown={startResize}
                className="group relative h-px shrink-0 cursor-row-resize bg-line"
              >
                <div className="absolute inset-x-0 -top-1 h-2 transition-colors-fast group-hover:bg-accent-soft" />
              </div>
              <div className="min-h-0 flex-1">
                <ResultsPane tab={tab} />
              </div>
            </>
          )}
        </>
      )}
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

/**
 * The star: on a table tab it pins the table in Favorites (filled once pinned, click again to
 * unpin); on a query tab it saves the SQL as a favorite (filled when this SQL is saved).
 */
function FavoriteButton({
  tab,
  onSaveQuery,
  size = "md",
}: {
  tab: Tab;
  onSaveQuery: () => void;
  size?: "sm" | "md";
}) {
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
      size={size}
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
        size={size === "sm" ? 14 : ICON.size}
        fill={favorite ? "currentColor" : "none"}
        className={cn(favorite && "text-warning")}
      />
    </IconButton>
  );
}
