import { closeCompletion } from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import { ListVideo, Play, Plus, Square, Star } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { EnvBadge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { statementAt } from "@/lib/engine";
import { modKey } from "@/lib/platform";
import { editableKey, editCount, type Tab, useApp } from "@/store/app";
import { flashRange, SqlEditor } from "../editor/SqlEditor";
import { SaveQueryDialog } from "../favorites/SaveQueryDialog";
import { ResultGrid } from "../grid/ResultGrid";
import { StructureView } from "../structure/StructureView";
import { Pager } from "./Pager";

const ICON = { size: 16, strokeWidth: 1.75 } as const;

/** Toolbar + SQL editor + results + status bar for one tab. */
export function QueryView({ tab }: { tab: Tab }) {
  const config = useApp((s) => s.connections.find((c) => c.id === tab.connId));
  const objects = useApp(
    (s) => s.nodes[tab.connId]?.databases.find((d) => d.name === tab.database)?.objects,
  );
  const {
    setSql,
    run,
    cancel,
    setView,
    editCell,
    addRow,
    toggleDeleteRow,
    discardEdits,
    saveEdits,
  } = useApp.getState();
  const structure = tab.browse !== null && tab.view === "structure";
  const editKey = editableKey(tab, config?.read_only ?? true);
  const pending = editCount(tab.edits);
  const editor = useRef<EditorView | null>(null);
  const [editorHeight, setEditorHeight] = useState(220);
  const [saving, setSaving] = useState(false);

  const tables = useMemo(() => (objects ?? []).map((o) => o.name), [objects]);

  const runFromEditor = useCallback(
    async (all: boolean) => {
      const view = editor.current;
      if (!view) return;
      closeCompletion(view);
      const text = view.state.doc.toString();
      const selection = view.state.selection.main;
      if (!selection.empty) return run(tab.id, view.state.sliceDoc(selection.from, selection.to));
      if (all) return run(tab.id, text);
      const range = await statementAt(text, selection.head);
      if (!range) return;
      flashRange(view, range[0], range[1]);
      run(tab.id, text.slice(range[0], range[1]));
    },
    [tab.id, run],
  );

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
  const grids = tab.sets.map((set, index) => ({ set, index })).filter(({ set }) => set.columns);

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
      {/* Toolbar */}
      <div className="flex h-9 shrink-0 items-center gap-0.5 border-line border-b bg-surface px-2">
        <IconButton
          label="Run statement"
          shortcut={`${modKey} ↵`}
          disabled={tab.running}
          onClick={() => runFromEditor(false)}
        >
          <Play {...ICON} className="text-accent-text" />
        </IconButton>
        <IconButton
          label="Run all"
          shortcut={`⇧ ${modKey} ↵`}
          disabled={tab.running}
          onClick={() => runFromEditor(true)}
        >
          <ListVideo {...ICON} />
        </IconButton>
        <IconButton label="Stop" disabled={!tab.running} onClick={() => cancel(tab.id)}>
          <Square {...ICON} size={14} />
        </IconButton>
        {tab.browse && (
          <SegmentedTabs
            label="View"
            value={tab.view}
            onChange={(view) => setView(tab.id, view)}
            className="ml-2 h-7 w-44"
            tabs={[
              { value: "data", label: "Data" },
              { value: "structure", label: "Structure" },
            ]}
          />
        )}
        {editKey && !structure && (
          <IconButton label="Add row" disabled={tab.running} onClick={() => addRow(tab.id)}>
            <Plus {...ICON} />
          </IconButton>
        )}
        {pending > 0 && !structure && (
          <div className="ml-2 flex items-center gap-1.5">
            <span className="text-sm text-muted">
              {pending} {pending === 1 ? "change" : "changes"}
            </span>
            <Button size="sm" disabled={tab.saving} onClick={() => discardEdits(tab.id)}>
              Discard
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={tab.saving}
              title={`Save (${modKey} S)`}
              onClick={() => saveEdits(tab.id)}
            >
              {tab.saving ? "Saving…" : "Save"}
            </Button>
          </div>
        )}
        <div className="flex-1" />
        {!structure && (
          <IconButton
            label="Save query…"
            disabled={!tab.sql.trim()}
            onClick={() => setSaving(true)}
          >
            <Star {...ICON} />
          </IconButton>
        )}
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
          <StructureView structure={tab.structure} />
        </div>
      )}

      {/* Editor. Kept mounted in the Structure view so its text and undo history survive. */}
      <div style={{ height: editorHeight }} className={cn("shrink-0", structure && "hidden")}>
        <SqlEditor
          initialValue={tab.sql}
          tables={tables}
          onChange={(sql) => setSql(tab.id, sql)}
          onRun={(all) => void runFromEditor(all)}
          onReady={(view) => {
            editor.current = view;
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

      {/* Results */}
      <div className={cn("relative min-h-0 flex-1", structure && "hidden")}>
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
        {active?.columns && (
          <ResultGrid
            columns={active.columns}
            rows={active.rows}
            resetKey={tab.runId * 100 + tab.activeSet}
            editing={
              editKey && !tab.running
                ? {
                    edits: tab.edits ?? { updates: {}, inserts: [], deletes: [] },
                    onEdit: (row, col, value) => editCell(tab.id, row, col, value),
                    onToggleDelete: (row) => toggleDeleteRow(tab.id, row),
                  }
                : undefined
            }
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

      {/* Status bar */}
      <div className="flex h-6 shrink-0 items-center gap-3 border-line border-t bg-surface px-3 text-sm text-muted">
        {tab.running && <Spinner size={11} />}
        <span className="truncate">{status(tab)}</span>
        <div className="flex-1" />
        {!structure && <Pager tab={tab} />}
        {grids.length > 1 &&
          grids.map(({ index }, n) => (
            <button
              key={index}
              type="button"
              onClick={() => useApp.getState().setActiveSet(tab.id, index)}
              className={cn(
                "h-5 rounded-sm px-2 text-xs transition-colors-fast",
                index === tab.activeSet ? "bg-selected text-fg" : "hover:bg-hover",
              )}
            >
              Result {n + 1}
            </button>
          ))}
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
