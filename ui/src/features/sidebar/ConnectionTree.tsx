import { useVirtualizer } from "@tanstack/react-virtual";
import { CalendarClock, Database, Eye, Server, SquareFunction, Table2, Zap } from "lucide-react";
import { memo, useMemo, useRef } from "react";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { SchemaObject } from "@/bindings/SchemaObject";
import { EnvBadge } from "@/components/ui/Badge";
import { ContextMenuArea, type MenuItem } from "@/components/ui/Menu";
import { Spinner } from "@/components/ui/Spinner";
import { TreeRow } from "@/components/ui/TreeRow";
import { type ConnNode, copyText, type DbNode, useApp } from "@/store/app";

const ICON = { size: 15, strokeWidth: 1.75 } as const;
/** `TreeRow` is 28px tall; every line of the tree is one row. */
const ROW_HEIGHT = 28;

/** One line of the tree, flattened so only the lines in view are rendered. */
type Line =
  | { kind: "conn"; key: string; config: ConnectionConfig; node?: ConnNode; expanded?: boolean }
  | { kind: "db"; key: string; config: ConnectionConfig; node: ConnNode; db: DbNode; open: boolean }
  | { kind: "note"; key: string; text: string; danger: boolean }
  | {
      kind: "object";
      key: string;
      config: ConnectionConfig;
      node: ConnNode;
      db: string;
      object: SchemaObject;
    };

/** Tree order of kinds: tables, views, routines, triggers, events. */
const RANK: Record<string, number> = {
  table: 0,
  view: 1,
  materialized_view: 1,
  trigger: 3,
  event: 4,
};

/** One collator for every comparison: `localeCompare` sets one up per call, slow on 5000 names. */
const byName = new Intl.Collator().compare;

/** Objects in tree order (by kind, then by name), sorted once per list. */
const sorted = new WeakMap<SchemaObject[], SchemaObject[]>();
function sortObjects(objects: SchemaObject[]): SchemaObject[] {
  let list = sorted.get(objects);
  if (!list) {
    list = [...objects].sort(
      (a, b) => (RANK[a.kind] ?? 2) - (RANK[b.kind] ?? 2) || byName(a.name, b.name),
    );
    sorted.set(objects, list);
  }
  return list;
}

/**
 * The lines of the tree. With a filter (`needle`, lower case), a connection whose name matches
 * shows as usual; otherwise only the databases whose name matches, and the loaded objects that
 * match (their database shown open). Only loaded databases are searched.
 */
function linesOf(
  connections: ConnectionConfig[],
  nodes: Record<string, ConnNode>,
  needle: string,
): Line[] {
  const lines: Line[] = [];
  for (const config of connections) {
    const node = nodes[config.id];
    const connected = node?.status.kind === "connected";
    const nameMatches = !needle || config.name.toLowerCase().includes(needle);
    const databases: Array<{ db: DbNode; objects: SchemaObject[] | null }> = [];
    if (connected && node && (nameMatches ? node.expanded : true)) {
      for (const db of node.databases) {
        if (nameMatches || db.name.toLowerCase().includes(needle)) {
          databases.push({ db, objects: null });
          continue;
        }
        const objects = (db.objects ?? []).filter((o) => o.name.toLowerCase().includes(needle));
        if (objects.length > 0) databases.push({ db, objects });
      }
    }
    if (!nameMatches && databases.length === 0) continue;
    lines.push({
      kind: "conn",
      key: config.id,
      config,
      node,
      expanded: connected ? node?.expanded || !nameMatches : undefined,
    });
    if (!node) continue;
    for (const { db, objects } of databases) {
      // A database shown for its matching objects is open and lists only those.
      const open = objects !== null || db.expanded;
      const dbKey = `${config.id}/${db.name}`;
      lines.push({ kind: "db", key: dbKey, config, node, db, open });
      if (!open) continue;
      if (db.error) {
        lines.push({ kind: "note", key: `${dbKey}#error`, text: db.error, danger: true });
      }
      if (db.objects?.length === 0) {
        lines.push({ kind: "note", key: `${dbKey}#empty`, text: "No tables", danger: false });
      }
      for (const object of objects ?? sortObjects(db.objects ?? [])) {
        lines.push({
          kind: "object",
          key: `${dbKey}/${object.kind}:${object.name}`,
          config,
          node,
          db: db.name,
          object,
        });
      }
    }
  }
  return lines;
}

/**
 * Connections → databases → objects. Virtualized: a database with thousands of tables renders
 * only the rows in view, and the tree never re-renders for changes elsewhere in the app.
 */
export const ConnectionTree = memo(function ConnectionTree({
  needle,
  selected,
  onSelect,
}: {
  needle: string;
  selected: string | null;
  onSelect: (key: string) => void;
}) {
  const connections = useApp((s) => s.connections);
  const nodes = useApp((s) => s.nodes);
  const lines = useMemo(() => linesOf(connections, nodes, needle), [connections, nodes, needle]);
  const scroller = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  return (
    <div ref={scroller} role="tree" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      {needle && lines.length === 0 && connections.length > 0 && (
        <p className="px-2 pt-6 text-center text-sm text-muted">
          Nothing matches. Only connected databases that are open are searched.
        </p>
      )}
      <div style={{ height: virtualizer.getTotalSize() }} className="relative">
        {virtualizer.getVirtualItems().map((item) => {
          const line = lines[item.index];
          if (!line) return null;
          return (
            <div
              key={line.key}
              style={{ height: ROW_HEIGHT, transform: `translateY(${item.start}px)` }}
              className="absolute top-0 left-0 w-full"
            >
              <TreeLine line={line} active={selected === line.key} onSelect={onSelect} />
            </div>
          );
        })}
      </div>
    </div>
  );
});

function TreeLine({
  line,
  active,
  onSelect,
}: {
  line: Line;
  active: boolean;
  onSelect: (key: string) => void;
}) {
  switch (line.kind) {
    case "conn":
      return <ConnectionLine line={line} active={active} onSelect={onSelect} />;
    case "db":
      return <DatabaseLine line={line} active={active} onSelect={onSelect} />;
    case "object":
      return <ObjectLine line={line} active={active} onSelect={onSelect} />;
    case "note":
      return (
        <p
          title={line.text}
          className={`truncate py-1.5 pl-12 text-sm ${line.danger ? "text-danger" : "text-faint"}`}
        >
          {line.text}
        </p>
      );
  }
}

/** The connection is connected, its driver changes schemas, and it is writable. */
function canChangeSchema(config: ConnectionConfig, node: ConnNode | undefined): boolean {
  return (
    node?.status.kind === "connected" && node.status.capabilities.schema_edit && !config.read_only
  );
}

function ConnectionLine({
  line,
  active,
  onSelect,
}: {
  line: Extract<Line, { kind: "conn" }>;
  active: boolean;
  onSelect: (key: string) => void;
}) {
  const { config, node } = line;
  const conn = config.id;
  const isFavorite = useApp((s) =>
    s.favorites.some((f) => f.conn === conn && f.item.kind === "connection"),
  );
  const s = useApp.getState();
  const status = node?.status.kind ?? "idle";
  const connected = status === "connected";
  const menu: MenuItem[] = [
    ...(connected
      ? [
          {
            label: "New query",
            onSelect: () => s.newQuery({ connId: conn, database: config.database }),
          },
          ...(canChangeSchema(config, node)
            ? [
                {
                  label: "New database…",
                  onSelect: () => s.openSchemaForm({ kind: "create_database", conn }),
                },
              ]
            : []),
          ...(node?.status.kind === "connected" && node.status.capabilities.server_info
            ? [
                {
                  label: "Server variables…",
                  onSelect: () => s.openServerInfo(conn, "variables"),
                },
                { label: "Server status…", onSelect: () => s.openServerInfo(conn, "status") },
              ]
            : []),
          { label: "Disconnect", onSelect: () => s.disconnect(conn) },
        ]
      : [{ label: "Connect", onSelect: () => s.toggleConnection(conn) }]),
    {
      label: isFavorite ? "Remove from favorites" : "Add to favorites",
      onSelect: () => s.toggleConnectionFavorite(conn),
      separatorBefore: true,
    },
    { label: "Copy name", onSelect: () => copyText(config.name) },
    {
      label: "Edit…",
      onSelect: () => s.openDialog({ mode: "edit", id: conn }),
      separatorBefore: true,
    },
    { label: "Delete…", onSelect: () => s.openDialog({ mode: "delete", id: conn }), danger: true },
  ];
  return (
    <ContextMenuArea items={menu}>
      <TreeRow
        depth={0}
        expanded={line.expanded}
        icon={<Server {...ICON} />}
        label={config.name}
        active={active}
        title={node?.status.kind === "failed" ? node.status.error : undefined}
        onClick={() => {
          onSelect(line.key);
          s.toggleConnection(conn);
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
  );
}

function DatabaseLine({
  line,
  active,
  onSelect,
}: {
  line: Extract<Line, { kind: "db" }>;
  active: boolean;
  onSelect: (key: string) => void;
}) {
  const { config, node, db } = line;
  const conn = config.id;
  const s = useApp.getState();
  const schema = canChangeSchema(config, node);
  const menu: MenuItem[] = [
    { label: "New query", onSelect: () => s.newQuery({ connId: conn, database: db.name }) },
    ...(schema
      ? [
          {
            label: "New table…",
            onSelect: () => s.openSchemaForm({ kind: "create_table", conn, database: db.name }),
          },
        ]
      : []),
    { label: "Refresh", onSelect: () => s.reloadObjects(conn, db.name) },
    { label: "Copy name", onSelect: () => copyText(db.name) },
    ...(schema
      ? [
          {
            label: "Drop database…",
            danger: true,
            separatorBefore: true,
            onSelect: () => s.alterSchema(conn, { kind: "drop_database", name: db.name }),
          },
        ]
      : []),
  ];
  return (
    <ContextMenuArea items={menu}>
      <TreeRow
        depth={1}
        expanded={line.open}
        icon={<Database {...ICON} />}
        label={db.name}
        active={active}
        onClick={() => {
          onSelect(line.key);
          s.toggleDatabase(conn, db.name);
        }}
        trailing={db.loading ? <Spinner size={12} /> : undefined}
      />
    </ContextMenuArea>
  );
}

function ObjectLine({
  line,
  active,
  onSelect,
}: {
  line: Extract<Line, { kind: "object" }>;
  active: boolean;
  onSelect: (key: string) => void;
}) {
  const { config, node, db, object } = line;
  const conn = config.id;
  const s = useApp.getState();
  const table = { database: db, schema: null, name: object.name };
  const isTable = object.kind === "table";
  const isView = object.kind === "view";
  const schema = canChangeSchema(config, node);
  const menu: MenuItem[] = [
    { label: "Open", onSelect: () => s.openObject(conn, db, object.name, object.kind) },
    ...(isTable || isView
      ? [
          { label: "Open structure", onSelect: () => s.openStructure(conn, db, object.name) },
          { label: "Add to favorites", onSelect: () => s.addTableFavorite(conn, db, object.name) },
        ]
      : []),
    { label: "Copy name", onSelect: () => copyText(object.name) },
    ...(isTable || isView
      ? [
          {
            label: "Export…",
            separatorBefore: true,
            onSelect: () => s.transfer("export", { kind: "table", conn, table }),
          },
          ...(isTable && !config.read_only
            ? [
                {
                  label: "Import…",
                  onSelect: () => s.transfer("import", { kind: "table", conn, table }),
                },
              ]
            : []),
        ]
      : []),
    ...(schema && isTable
      ? [
          {
            label: "Rename…",
            separatorBefore: true,
            onSelect: () => s.openSchemaForm({ kind: "rename_table", conn, table }),
          },
          {
            label: "Empty table…",
            danger: true,
            onSelect: () => s.alterSchema(conn, { kind: "truncate_table", table }),
          },
        ]
      : []),
    ...(schema && (isTable || isView)
      ? [
          {
            label: isView ? "Drop view…" : "Drop table…",
            danger: true,
            separatorBefore: isView,
            onSelect: () => s.alterSchema(conn, { kind: "drop_table", table, view: isView }),
          },
        ]
      : []),
  ];
  return (
    <ContextMenuArea items={menu}>
      <TreeRow
        depth={2}
        icon={objectIcon(object.kind)}
        label={object.name}
        active={active}
        onClick={() => {
          onSelect(line.key);
          s.openObject(conn, db, object.name, object.kind);
        }}
      />
    </ContextMenuArea>
  );
}

function objectIcon(kind: string) {
  if (kind === "table") return <Table2 {...ICON} />;
  if (kind === "view" || kind === "materialized_view") return <Eye {...ICON} />;
  if (kind === "trigger") return <Zap {...ICON} />;
  if (kind === "event") return <CalendarClock {...ICON} />;
  return <SquareFunction {...ICON} />;
}
