import { Plus, Search, Server, Settings, X } from "lucide-react";
import { memo, useState } from "react";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { keysOf } from "@/lib/shortcuts";
import { useApp } from "@/store/app";
import { FavoritesList } from "../favorites/FavoritesList";
import { ConnectionTree } from "./ConnectionTree";

const ICON = { size: 15, strokeWidth: 1.75 } as const;

/** Memoized: it takes no props, so typing in a tab or streaming rows never re-renders it. */
export const Sidebar = memo(function Sidebar() {
  const connections = useApp((s) => s.connections);
  const ready = useApp((s) => s.ready);
  const version = useApp((s) => s.engineVersion);
  const openDialog = useApp((s) => s.openDialog);
  const pane = useApp((s) => s.sidebarPane);
  const setPane = useApp((s) => s.setSidebarPane);
  const [selected, setSelected] = useState<string | null>(null);
  // Filters connections, databases and loaded tables, or favorites, by name.
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();

  return (
    <aside className="flex h-full flex-col bg-sidebar">
      {/* Same height as the tab bar, so the top edge lines up across the window. The left of this
          row stays empty: the window buttons (macOS) and the sidebar toggle are drawn over it. */}
      <header data-tauri-drag-region className="flex h-9 shrink-0 items-center justify-end pr-2">
        <IconButton
          label="New connection"
          shortcut={keysOf("newConnection")}
          onClick={() => openDialog({ mode: "new" })}
        >
          <Plus {...ICON} />
        </IconButton>
      </header>

      <div className="flex shrink-0 flex-col gap-2 px-2 pb-2">
        <SegmentedTabs
          label="Sidebar"
          value={pane}
          onChange={setPane}
          tabs={[
            { value: "connections", label: "Connections" },
            { value: "favorites", label: "Favorites" },
          ]}
        />
        <div className="relative">
          <Search
            size={13}
            strokeWidth={1.75}
            className="pointer-events-none absolute top-2.5 left-3 text-faint"
          />
          <Input
            type="search"
            aria-label="Search"
            value={query}
            inputSize="lg"
            placeholder={pane === "connections" ? "Filter connections and tables" : "Filter"}
            onChange={(e) => setQuery(e.target.value)}
            className="pr-8 pl-8 [&::-webkit-search-cancel-button]:hidden"
          />
          {query && (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => setQuery("")}
              className="absolute top-1.5 right-1.5 inline-flex size-5 items-center justify-center rounded-sm text-faint transition-colors-fast hover:bg-hover hover:text-fg"
            >
              <X size={12} strokeWidth={2} />
            </button>
          )}
        </div>
      </div>

      {pane === "connections" ? (
        ready && connections.length === 0 ? (
          <div className="min-h-0 flex-1">
            <EmptyState onAdd={() => openDialog({ mode: "new" })} />
          </div>
        ) : (
          <ConnectionTree needle={needle} selected={selected} onSelect={setSelected} />
        )
      ) : (
        <FavoritesList query={query} />
      )}

      <footer className="flex h-8 shrink-0 items-center justify-between pr-1.5 pl-3 text-xs text-faint">
        <span>{version ? `Quarry ${version}` : ""}</span>
        <IconButton
          label="Settings…"
          shortcut={keysOf("settings")}
          size="sm"
          onClick={() => useApp.getState().setSettingsOpen(true)}
        >
          <Settings size={14} strokeWidth={1.75} />
        </IconButton>
      </footer>
    </aside>
  );
});

function EmptyState({ onAdd }: { onAdd: () => void }) {
  return (
    <div className="flex flex-col items-center gap-2 px-4 pt-16 text-center">
      <Server size={28} strokeWidth={1.5} className="text-faint" />
      <p className="font-semibold text-base text-fg">No connections</p>
      <p className="text-sm text-muted">Add a database to get started.</p>
      <Button variant="primary" size="sm" className="mt-2" onClick={onAdd}>
        New connection
      </Button>
    </div>
  );
}
