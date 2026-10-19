import {
  Database,
  Eye,
  Plus,
  Search,
  Server,
  Settings,
  SquareFunction,
  Table2,
  X,
} from "lucide-react";
import { useState } from "react";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { SchemaObject } from "@/bindings/SchemaObject";
import { EnvBadge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { ContextMenuArea, type MenuItem } from "@/components/ui/Menu";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { TreeRow } from "@/components/ui/TreeRow";
import { keysOf } from "@/lib/shortcuts";
import { type ConnNode, copyText, type DbNode, useApp } from "@/store/app";
import { FavoritesList } from "../favorites/FavoritesList";

const ICON = { size: 15, strokeWidth: 1.75 } as const;

/**
 * What a connection shows under the sidebar filter `needle` (lower case): everything (`all`), or
 * only some databases, each with all its objects (`objects: null`) or only the matching ones.
 * `null` when nothing in it matches. Only loaded databases are searched.
 */
type Match =
  | { all: true }
  | { all: false; databases: Array<{ db: DbNode; objects: SchemaObject[] | null }> };

function matchOf(
  config: ConnectionConfig,
  node: ConnNode | undefined,
  needle: string,
): Match | null {
  if (!needle || config.name.toLowerCase().includes(needle)) return { all: true };
  type Shown = { db: DbNode; objects: SchemaObject[] | null };
  const databases = (node?.databases ?? []).flatMap((db): Shown[] => {
    if (db.name.toLowerCase().includes(needle)) return [{ db, objects: null }];
    const objects = (db.objects ?? []).filter((o) => o.name.toLowerCase().includes(needle));
    return objects.length > 0 ? [{ db, objects }] : [];
  });
  return databases.length > 0 ? { all: false, databases } : null;
}

export function Sidebar() {
  const connections = useApp((s) => s.connections);
  const ready = useApp((s) => s.ready);
  const version = useApp((s) => s.engineVersion);
  const openDialog = useApp((s) => s.openDialog);
  const nodes = useApp((s) => s.nodes);
  const pane = useApp((s) => s.sidebarPane);
  const setPane = useApp((s) => s.setSidebarPane);
  const [selected, setSelected] = useState<string | null>(null);
  // Filters connections, databases and loaded tables, or favorites, by name.
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const matches = connections
    .map((config) => ({ config, match: matchOf(config, nodes[config.id], needle) }))
    .filter((m) => m.match !== null);

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
        <div role="tree" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          {ready && connections.length === 0 && (
            <EmptyState onAdd={() => openDialog({ mode: "new" })} />
          )}
          {matches.map(({ config, match }) => (
            <ConnectionBranch
              key={config.id}
              config={config}
              match={match ?? { all: true }}
              selected={selected}
              onSelect={setSelected}
            />
          ))}
          {needle && connections.length > 0 && matches.length === 0 && (
            <p className="px-2 pt-6 text-center text-sm text-muted">
              Nothing matches. Only connected databases that are open are searched.
            </p>
          )}
        </div>
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
  match,
  selected,
  onSelect,
}: {
  config: ConnectionConfig;
  match: Match;
  selected: string | null;
  onSelect: (key: string) => void;
}) {
  const isFavorite = useApp((s) =>
    s.favorites.some((f) => f.conn === config.id && f.item.kind === "connection"),
  );
  const node = useApp((s) => s.nodes[config.id]) as ConnNode | undefined;
  const {
    toggleConnection,
    toggleDatabase,
    openObject,
    disconnect,
    newQuery,
    openDialog,
    addTableFavorite,
    openSchemaForm,
    alterSchema,
    reloadObjects,
    openStructure,
    toggleConnectionFavorite,
    transfer,
  } = useApp.getState();
  const status = node?.status.kind ?? "idle";
  const connected = status === "connected";
  const canChangeSchema =
    node?.status.kind === "connected" && node.status.capabilities.schema_edit && !config.read_only;
  const conn = config.id;

  const menu: MenuItem[] = [
    ...(connected
      ? [
          {
            label: "New query",
            onSelect: () => newQuery({ connId: config.id, database: config.database }),
          },
          ...(canChangeSchema
            ? [
                {
                  label: "New database…",
                  onSelect: () => openSchemaForm({ kind: "create_database", conn }),
                },
              ]
            : []),
          { label: "Disconnect", onSelect: () => disconnect(config.id) },
        ]
      : [{ label: "Connect", onSelect: () => toggleConnection(config.id) }]),
    {
      label: isFavorite ? "Remove from favorites" : "Add to favorites",
      onSelect: () => toggleConnectionFavorite(conn),
      separatorBefore: true,
    },
    { label: "Copy name", onSelect: () => copyText(config.name) },
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
          expanded={connected ? node?.expanded || !match.all : undefined}
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
        node &&
        (node.expanded || !match.all) &&
        (match.all
          ? node.databases.map((db) => ({ db, objects: null as SchemaObject[] | null }))
          : match.databases
        ).map(({ db, objects: matched }) => {
          // A database shown for its matching tables is open and lists only those.
          const open = matched !== null || db.expanded;
          const shown = matched ?? db.objects ?? [];
          const dbKey = `${config.id}/${db.name}`;
          const dbMenu: MenuItem[] = [
            { label: "New query", onSelect: () => newQuery({ connId: conn, database: db.name }) },
            ...(canChangeSchema
              ? [
                  {
                    label: "New table…",
                    onSelect: () =>
                      openSchemaForm({ kind: "create_table", conn, database: db.name }),
                  },
                ]
              : []),
            { label: "Refresh", onSelect: () => reloadObjects(conn, db.name) },
            { label: "Copy name", onSelect: () => copyText(db.name) },
            ...(canChangeSchema
              ? [
                  {
                    label: "Drop database…",
                    danger: true,
                    separatorBefore: true,
                    onSelect: () => alterSchema(conn, { kind: "drop_database", name: db.name }),
                  },
                ]
              : []),
          ];
          return (
            <div key={db.name} role="group">
              <ContextMenuArea items={dbMenu}>
                <TreeRow
                  depth={1}
                  expanded={open}
                  icon={<Database {...ICON} />}
                  label={db.name}
                  active={selected === dbKey}
                  onClick={() => {
                    onSelect(dbKey);
                    toggleDatabase(config.id, db.name);
                  }}
                  trailing={db.loading ? <Spinner size={12} /> : undefined}
                />
              </ContextMenuArea>
              {open && db.error && <p className="py-1 pl-12 text-sm text-danger">{db.error}</p>}
              {open && db.objects?.length === 0 && (
                <p className="py-1 pl-12 text-sm text-faint">No tables</p>
              )}
              {open &&
                sortObjects(shown).map((object) => {
                  const key = `${dbKey}/${object.name}`;
                  const table = { database: db.name, schema: null, name: object.name };
                  const isTable = object.kind === "table";
                  const isView = object.kind === "view";
                  const objectMenu: MenuItem[] = [
                    {
                      label: "Open",
                      onSelect: () => openObject(config.id, db.name, object.name, object.kind),
                    },
                    ...(isTable || isView
                      ? [
                          {
                            label: "Open structure",
                            onSelect: () => openStructure(conn, db.name, object.name),
                          },
                          {
                            label: "Add to favorites",
                            onSelect: () => addTableFavorite(config.id, db.name, object.name),
                          },
                        ]
                      : []),
                    { label: "Copy name", onSelect: () => copyText(object.name) },
                    ...(isTable || isView
                      ? [
                          {
                            label: "Export…",
                            separatorBefore: true,
                            onSelect: () => transfer("export", { kind: "table", conn, table }),
                          },
                          ...(isTable && !config.read_only
                            ? [
                                {
                                  label: "Import…",
                                  onSelect: () =>
                                    transfer("import", { kind: "table", conn, table }),
                                },
                              ]
                            : []),
                        ]
                      : []),
                    ...(canChangeSchema && isTable
                      ? [
                          {
                            label: "Rename…",
                            separatorBefore: true,
                            onSelect: () => openSchemaForm({ kind: "rename_table", conn, table }),
                          },
                          {
                            label: "Empty table…",
                            danger: true,
                            onSelect: () => alterSchema(conn, { kind: "truncate_table", table }),
                          },
                        ]
                      : []),
                    ...(canChangeSchema && (isTable || isView)
                      ? [
                          {
                            label: isView ? "Drop view…" : "Drop table…",
                            danger: true,
                            separatorBefore: isView,
                            onSelect: () =>
                              alterSchema(conn, { kind: "drop_table", table, view: isView }),
                          },
                        ]
                      : []),
                  ];
                  return (
                    <ContextMenuArea key={`${object.kind}:${object.name}`} items={objectMenu}>
                      <TreeRow
                        depth={2}
                        icon={objectIcon(object.kind)}
                        label={object.name}
                        active={selected === key}
                        onClick={() => {
                          onSelect(key);
                          openObject(config.id, db.name, object.name, object.kind);
                        }}
                      />
                    </ContextMenuArea>
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
