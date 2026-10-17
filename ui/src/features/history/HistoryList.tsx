import { History } from "lucide-react";
import { useEffect, useState } from "react";
import type { HistoryEntry } from "@/bindings/HistoryEntry";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { ContextMenuArea } from "@/components/ui/Menu";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

/** Queries the user ran, newest first. `query` (the sidebar filter) searches the SQL. */
export function HistoryList({ query }: { query: string }) {
  const history = useApp((s) => s.history);
  const connections = useApp((s) => s.connections);
  const { loadHistory, clearHistory, openSql } = useApp.getState();
  const [confirmClear, setConfirmClear] = useState(false);

  // Search as the user types, a moment after they stop.
  useEffect(() => {
    const timer = setTimeout(() => loadHistory(query), history.loaded ? 200 : 0);
    return () => clearTimeout(timer);
  }, [query, loadHistory, history.loaded]);

  const nameOf = (id: string) => connections.find((c) => c.id === id)?.name;
  const open = (entry: HistoryEntry) => {
    if (!nameOf(entry.conn)) {
      useApp.getState().toast("info", "That connection no longer exists");
      return;
    }
    openSql(entry.conn, entry.database, entry.sql);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {history.loaded && history.entries.length === 0 ? (
        <div className="flex flex-col items-center gap-2 px-4 pt-16 text-center">
          <History size={28} strokeWidth={1.5} className="text-faint" />
          <p className="font-semibold text-base text-fg">
            {query ? "No matching queries" : "No history yet"}
          </p>
          <p className="text-sm text-muted">Queries you run show up here.</p>
        </div>
      ) : (
        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          {history.entries.map((entry) => (
            <li key={`${entry.ran_at}|${entry.conn}|${entry.sql}`}>
              <ContextMenuArea
                items={[
                  { label: "Open in new tab", onSelect: () => open(entry) },
                  {
                    label: "Copy SQL",
                    onSelect: () => void navigator.clipboard.writeText(entry.sql),
                  },
                ]}
              >
                <button
                  type="button"
                  title={entry.sql}
                  onClick={() => open(entry)}
                  className="flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left transition-colors-fast hover:bg-hover focus-visible:bg-hover focus-visible:outline-none"
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
                      {nameOf(entry.conn) ?? "Deleted connection"}
                      {entry.database ? ` · ${entry.database}` : ""} · {ago(entry.ran_at)}
                    </span>
                  </span>
                </button>
              </ContextMenuArea>
            </li>
          ))}
        </ul>
      )}
      {history.entries.length > 0 && !query && (
        <div className="shrink-0 px-2 pb-1">
          <Button size="sm" variant="ghost" onClick={() => setConfirmClear(true)}>
            Clear history…
          </Button>
        </div>
      )}
      <Dialog
        open={confirmClear}
        onClose={() => setConfirmClear(false)}
        title="Clear query history?"
        description="Every saved query is removed. This can't be undone."
        width={400}
        footer={
          <>
            <Button autoFocus onClick={() => setConfirmClear(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                clearHistory();
                setConfirmClear(false);
              }}
            >
              Clear
            </Button>
          </>
        }
      />
    </div>
  );
}

function firstLine(sql: string): string {
  return sql.trim().split("\n")[0] ?? "";
}

/** "just now", "5 min ago", "3 h ago", or the date. */
function ago(iso: string): string {
  const seconds = (Date.now() - new Date(iso).getTime()) / 1000;
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return new Date(iso).toLocaleDateString();
}
