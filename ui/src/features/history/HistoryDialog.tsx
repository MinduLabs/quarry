import { History } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { HistoryEntry } from "@/bindings/HistoryEntry";
import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { Kbd } from "@/components/ui/Kbd";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { cn } from "@/lib/cn";
import { insertIntoEditor } from "@/lib/editors";
import { isMac } from "@/lib/platform";
import { keysOf } from "@/lib/shortcuts";
import { copyText, useApp } from "@/store/app";

type Scope = "tab" | "all";

/**
 * SQL run in query tabs, newest first, with a search box. Enter puts the chosen query into the
 * current tab's editor; Ctrl/⌘+Enter opens it in a new tab.
 */
export function HistoryDialog() {
  const open = useApp((s) => s.historyOpen);
  const setOpen = useApp((s) => s.setHistoryOpen);
  return (
    <Dialog
      open={open}
      onClose={() => setOpen(false)}
      title="Query history"
      width={820}
      height={540}
      flush
    >
      {open && <HistoryBrowser onClose={() => setOpen(false)} />}
    </Dialog>
  );
}

function HistoryBrowser({ onClose }: { onClose: () => void }) {
  const history = useApp((s) => s.history);
  const connections = useApp((s) => s.connections);
  // The tab the dialog was opened from: where Insert writes, and whose connection is the default.
  const [tab] = useState(() => {
    const s = useApp.getState();
    return s.tabs.find((t) => t.id === s.activeTabId) ?? null;
  });
  const [scope, setScope] = useState<Scope>(tab ? "tab" : "all");
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [confirmClear, setConfirmClear] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const conn = scope === "tab" ? (tab?.connId ?? null) : null;
  const { loadHistory, clearHistory, openSql } = useApp.getState();

  // Search as the user types, a moment after they stop.
  useEffect(() => {
    const timer = setTimeout(() => loadHistory(query, conn), query ? 200 : 0);
    return () => clearTimeout(timer);
  }, [query, conn, loadHistory]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: back to the newest on every search
  useEffect(() => setIndex(0), [query, conn]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const entries = history.entries;
  const selected = entries[index];
  const nameOf = (id: string) => connections.find((c) => c.id === id)?.name;

  const inNewTab = (entry: HistoryEntry | undefined) => {
    if (!entry) return;
    if (!nameOf(entry.conn)) {
      useApp.getState().toast("info", "That connection no longer exists");
      return;
    }
    onClose();
    openSql(entry.conn, entry.database, entry.sql);
  };
  const insert = (entry: HistoryEntry | undefined) => {
    if (!entry) return;
    if (!tab || !useApp.getState().tabs.some((t) => t.id === tab.id)) return inNewTab(entry);
    onClose();
    // The dialog's focus returns first; then the editor takes it.
    requestAnimationFrame(() => {
      if (!insertIntoEditor(tab.id, entry.sql)) inNewTab(entry);
    });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-line border-b px-4 py-2">
        <Input
          autoFocus
          type="search"
          aria-label="Search the history"
          value={query}
          placeholder="Search SQL"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              const step = e.key === "ArrowDown" ? 1 : -1;
              setIndex((i) => Math.min(entries.length - 1, Math.max(0, i + step)));
            } else if (e.key === "Enter") {
              e.preventDefault();
              if (e.metaKey || e.ctrlKey) inNewTab(selected);
              else insert(selected);
            }
          }}
          className="[&::-webkit-search-cancel-button]:hidden"
        />
        {tab && (
          <SegmentedTabs
            label="Connections"
            value={scope}
            onChange={setScope}
            className="w-64 shrink-0"
            tabs={[
              { value: "tab", label: nameOf(tab.connId) ?? "This connection" },
              { value: "all", label: "All connections" },
            ]}
          />
        )}
      </div>

      {history.loaded && entries.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 text-center">
          <History size={28} strokeWidth={1.5} className="text-faint" />
          <p className="font-semibold text-base text-fg">
            {query ? "No matching queries" : "No queries yet"}
          </p>
          <p className="max-w-96 text-sm text-muted">
            SQL you run in a query tab is kept here, with its time and any error. Opening a table is
            not.
          </p>
          <span className="flex items-center gap-1.5 text-sm text-muted">
            Run the statement under the cursor with <Kbd keys={keysOf("run")} />
          </span>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1">
          <div
            ref={list}
            role="listbox"
            aria-label="Queries"
            className="w-2/5 min-w-0 shrink-0 overflow-y-auto border-line border-r p-1"
          >
            {entries.map((entry, i) => (
              <div
                key={`${entry.ran_at}|${entry.conn}|${entry.sql}`}
                role="option"
                data-index={i}
                aria-selected={i === index}
                tabIndex={-1}
                onClick={() => setIndex(i)}
                onDoubleClick={() => insert(entry)}
                onKeyDown={() => {}}
                className={cn(
                  "flex cursor-default flex-col gap-0.5 rounded-md px-2 py-1.5",
                  i === index ? "bg-selected" : "hover:bg-hover",
                )}
              >
                <span className="truncate font-mono text-sm text-fg">{firstLine(entry.sql)}</span>
                <span className="flex items-center gap-1.5 truncate text-xs text-muted">
                  <span
                    className={cn(
                      "size-1.5 shrink-0 rounded-full",
                      entry.error ? "bg-danger" : "bg-env-local",
                    )}
                  />
                  <span className="sr-only">{entry.error ? "Failed" : "Succeeded"}</span>
                  <span className="truncate">
                    {scope === "all" && `${nameOf(entry.conn) ?? "Deleted connection"} · `}
                    {entry.database ? `${entry.database} · ` : ""}
                    {ago(entry.ran_at)}
                  </span>
                </span>
              </div>
            ))}
          </div>
          <div className="flex min-w-0 flex-1 flex-col">
            {selected && (
              <>
                <pre className="min-h-0 flex-1 select-text overflow-auto whitespace-pre-wrap p-4 font-mono text-fg text-sm">
                  {selected.sql}
                </pre>
                <div className="flex shrink-0 flex-col gap-0.5 border-line border-t px-4 py-2 text-sm text-muted">
                  <span className="truncate">
                    {nameOf(selected.conn) ?? "Deleted connection"}
                    {selected.database ? ` · ${selected.database}` : ""} ·{" "}
                    {new Date(selected.ran_at).toLocaleString()} · {duration(selected.elapsed_ms)}
                  </span>
                  {selected.error && <span className="text-danger">{selected.error}</span>}
                </div>
              </>
            )}
          </div>
        </div>
      )}

      <div className="flex shrink-0 items-center gap-2 border-line border-t px-4 py-3">
        <Button
          variant="ghost"
          disabled={entries.length === 0}
          onClick={() => setConfirmClear(true)}
        >
          Clear history…
        </Button>
        <div className="flex-1" />
        <Button disabled={!selected} onClick={() => selected && copyText(selected.sql, "Copied")}>
          Copy
        </Button>
        <Button
          disabled={!selected}
          title={isMac ? "⌘↩" : "Ctrl+Enter"}
          onClick={() => inNewTab(selected)}
        >
          Open in new tab
        </Button>
        <Button variant="primary" disabled={!selected} onClick={() => insert(selected)}>
          {tab ? "Insert" : "Open"}
        </Button>
      </div>

      <ConfirmDialog
        open={confirmClear}
        title="Clear query history?"
        description="Every saved query of every connection is removed. This can't be undone."
        confirmLabel="Clear"
        width={400}
        onConfirm={() => {
          clearHistory();
          setConfirmClear(false);
        }}
        onCancel={() => setConfirmClear(false)}
      />
    </div>
  );
}

function firstLine(sql: string): string {
  return sql.trim().split("\n")[0] ?? "";
}

function duration(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${ms} ms`;
}

/** "just now", "5 min ago", "3 h ago", or the date. */
function ago(iso: string): string {
  const seconds = (Date.now() - new Date(iso).getTime()) / 1000;
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return new Date(iso).toLocaleDateString();
}
