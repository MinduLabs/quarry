import { RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import type { ServerInfoKind } from "@/bindings/ServerInfoKind";
import { Dialog } from "@/components/ui/Dialog";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { copyText, useApp } from "@/store/app";

/** Variables and status counters of the server, read-only, with a filter by name or value. */
export function ServerInfoDialog() {
  const info = useApp((s) => s.serverInfo);
  const name = useApp((s) => s.connections.find((c) => c.id === info?.conn)?.name);
  const { openServerInfo, closeServerInfo } = useApp.getState();
  const [filter, setFilter] = useState("");
  const rows = info?.rows;
  const shown = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!rows || !needle) return rows ?? [];
    return rows.filter(
      (r) => r.name.toLowerCase().includes(needle) || r.value.toLowerCase().includes(needle),
    );
  }, [rows, filter]);

  return (
    <Dialog
      open={info !== null}
      onClose={closeServerInfo}
      title={name ? `Server · ${name}` : "Server"}
      width={720}
      height={560}
      flush
    >
      {info && (
        <div className="flex h-full min-h-0 flex-col">
          <div className="flex items-center gap-2 border-line border-b p-3">
            <SegmentedTabs<ServerInfoKind>
              label="Server information"
              className="w-48 shrink-0"
              value={info.kind}
              onChange={(kind) => openServerInfo(info.conn, kind)}
              tabs={[
                { value: "variables", label: "Variables" },
                { value: "status", label: "Status" },
              ]}
            />
            <Input
              autoFocus
              value={filter}
              placeholder="Filter by name or value"
              aria-label="Filter"
              onChange={(e) => setFilter(e.target.value)}
              className="flex-1"
            />
            <IconButton
              label="Reload"
              size="sm"
              disabled={info.loading}
              onClick={() => openServerInfo(info.conn, info.kind)}
            >
              {info.loading ? <Spinner size={14} /> : <RefreshCw size={14} strokeWidth={1.75} />}
            </IconButton>
          </div>
          {info.error && (
            <p className="border-line border-b p-3 text-danger text-sm">{info.error}</p>
          )}
          <div className="min-h-0 flex-1 overflow-auto">
            {shown.map((row) => (
              <button
                key={row.name}
                type="button"
                title="Copy value"
                onClick={() => copyText(row.value)}
                className="flex w-full items-baseline gap-3 px-3 py-1 text-left text-sm hover:bg-hover"
              >
                <span className="w-64 shrink-0 truncate font-mono">{row.name}</span>
                <span className="min-w-0 flex-1 truncate font-mono text-muted">{row.value}</span>
              </button>
            ))}
            {!info.loading && !info.error && shown.length === 0 && (
              <p className="p-3 text-muted text-sm">Nothing matches.</p>
            )}
          </div>
          <div className="border-line border-t px-3 py-1.5 text-muted text-xs">
            {shown.length} of {info.rows.length}
          </div>
        </div>
      )}
    </Dialog>
  );
}
