import { Database, Eye, Plus, Search, Server, SquareFunction, Star, Table2, X } from "lucide-react";
import { useState } from "react";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import { EnvBadge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { ContextMenuArea } from "@/components/ui/Menu";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { TreeRow } from "@/components/ui/TreeRow";
import { modKey } from "@/lib/platform";
import { type ConnNode, useApp } from "@/store/app";

const ICON = { size: 15, strokeWidth: 1.75 } as const;

type Pane = "connections" | "favorites";

export function Sidebar() {
  const connections = useApp((s) => s.connections);
  const ready = useApp((s) => s.ready);
  const version = useApp((s) => s.engineVersion);
  const openDialog = useApp((s) => s.openDialog);
  const [selected, setSelected] = useState<string | null>(null);
  const [pane, setPane] = useState<Pane>("connections");
  // Search is not wired up yet; the box only holds text.
  const [query, setQuery] = useState("");

  return (
    <aside className="flex h-full flex-col bg-sidebar">
      {/* Same height as the tab bar, so the top edge lines up across the window. The left of this
          row stays empty: the window buttons (macOS) and the sidebar toggle are drawn over it. */}
      <header data-tauri-drag-region className="flex h-9 shrink-0 items-center justify-end pr-2">
        <IconButton
          label="New connection"
          shortcut={`${modKey} N`}
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
            placeholder="Filter"
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
        <div role="tree" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          {ready && connections.length === 0 && (
            <EmptyState onAdd={() => openDialog({ mode: "new" })} />
          )}
          {connections.map((config) => (
            <ConnectionBranch
              key={config.id}
              config={config}
              selected={selected}
              onSelect={setSelected}
            />
          ))}
        </div>
      ) : (
        <FavoritesEmpty />
      )}

      <footer className="shrink-0 px-3 pb-2 text-xs text-faint">
        {version ? `Quarry ${version}` : ""}
      </footer>
    </aside>
  );
}

function FavoritesEmpty() {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center gap-2 px-4 pt-16 text-center">
      <Star size={28} strokeWidth={1.5} className="text-faint" />
      <p className="font-semibold text-base text-fg">No favorites yet</p>
      <p className="text-sm text-muted">Pin tables and queries here for quick access.</p>
    </div>
  );
}

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

function ConnectionBranch({
  config,
  selected,
  onSelect,
}: {
  config: ConnectionConfig;
  selected: string | null;
  onSelect: (key: string) => void;
}) {
  const node = useApp((s) => s.nodes[config.id]) as ConnNode | undefined;
  const { toggleConnection, toggleDatabase, openObject, disconnect, newQuery, openDialog } =
    useApp.getState();
  const status = node?.status.kind ?? "idle";
  const connected = status === "connected";

  const menu = [
    ...(connected
      ? [
          {
            label: "New query",
            onSelect: () => newQuery({ connId: config.id, database: config.database }),
          },
          { label: "Disconnect", onSelect: () => disconnect(config.id) },
        ]
      : [{ label: "Connect", onSelect: () => toggleConnection(config.id) }]),
    {
      label: "Edit…",
      onSelect: () => openDialog({ mode: "edit", id: config.id }),
      separatorBefore: true,
    },
    {
      label: "Delete…",
      onSelect: () => openDialog({ mode: "delete", id: config.id }),
      danger: true,
    },
  ];

  return (
    <div role="group">
      <ContextMenuArea items={menu}>
        <TreeRow
          depth={0}
          expanded={connected ? node?.expanded : undefined}
          icon={<Server {...ICON} />}
          label={config.name}
          active={selected === config.id}
          title={node?.status.kind === "failed" ? node.status.error : undefined}
          onClick={() => {
            onSelect(config.id);
            toggleConnection(config.id);
          }}
          trailing={
            <span className="flex items-center gap-1.5">
              <EnvBadge environment={config.environment} />
              {status === "connecting" && <Spinner size={12} />}
              {status === "connected" && <span className="size-1.5 rounded-full bg-env-local" />}
              {status === "failed" && <span className="size-1.5 rounded-full bg-danger" />}
            </span>
          }
        />
      </ContextMenuArea>

      {connected &&
        node?.expanded &&
        node.databases.map((db) => {
          const dbKey = `${config.id}/${db.name}`;
          return (
            <div key={db.name} role="group">
              <TreeRow
                depth={1}
                expanded={db.expanded}
                icon={<Database {...ICON} />}
                label={db.name}
                active={selected === dbKey}
                onClick={() => {
                  onSelect(dbKey);
                  toggleDatabase(config.id, db.name);
                }}
                trailing={db.loading ? <Spinner size={12} /> : undefined}
              />
              {db.expanded && db.error && (
                <p className="py-1 pl-12 text-sm text-danger">{db.error}</p>
              )}
              {db.expanded && db.objects?.length === 0 && (
                <p className="py-1 pl-12 text-sm text-faint">No tables</p>
              )}
              {db.expanded &&
                sortObjects(db.objects ?? []).map((object) => {
                  const key = `${dbKey}/${object.name}`;
                  return (
                    <TreeRow
                      key={`${object.kind}:${object.name}`}
                      depth={2}
                      icon={objectIcon(object.kind)}
                      label={object.name}
                      active={selected === key}
                      onClick={() => {
                        onSelect(key);
                        openObject(config.id, db.name, object.name, object.kind);
                      }}
                    />
                  );
                })}
            </div>
          );
        })}
    </div>
  );
}

const RANK: Record<string, number> = { table: 0, view: 1, materialized_view: 1 };

function sortObjects<T extends { name: string; kind: string }>(objects: T[]): T[] {
  return [...objects].sort(
    (a, b) => (RANK[a.kind] ?? 2) - (RANK[b.kind] ?? 2) || a.name.localeCompare(b.name),
  );
}

function objectIcon(kind: string) {
  if (kind === "table") return <Table2 {...ICON} />;
  if (kind === "view" || kind === "materialized_view") return <Eye {...ICON} />;
  return <SquareFunction {...ICON} />;
}
