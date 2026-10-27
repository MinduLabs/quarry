import { useVirtualizer } from "@tanstack/react-virtual";
import { Database, Server } from "lucide-react";
import { memo, useMemo, useRef } from "react";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { ForeignKey } from "@/bindings/ForeignKey";
import type { IndexInfo } from "@/bindings/IndexInfo";
import type { SchemaObject } from "@/bindings/SchemaObject";
import { EnvBadge } from "@/components/ui/Badge";
import { ContextMenuArea, type MenuItem } from "@/components/ui/Menu";
import { iconKindOf, ObjectIcon, type ObjectIconKind } from "@/components/ui/ObjectIcon";
import { Spinner } from "@/components/ui/Spinner";
import { TreeRow } from "@/components/ui/TreeRow";
import {
  type ConnNode,
  copyText,
  type DbNode,
  shownSchemas,
  type TreeTableInfo,
  treeKey,
  useApp,
} from "@/store/app";

const ICON = { size: 15, strokeWidth: 1.75 } as const;
/** `TreeRow` is 28px tall; every line of the tree is one row. */
const ROW_HEIGHT = 28;

/** One line of the tree, flattened so only the lines in view are rendered. */
type Line =
  | { kind: "conn"; key: string; config: ConnectionConfig; node?: ConnNode; expanded?: boolean }
  | { kind: "db"; key: string; config: ConnectionConfig; node: ConnNode; db: DbNode; open: boolean }
  | { kind: "note"; key: string; text: string; danger: boolean; depth?: number }
  | {
      kind: "group";
      key: string;
      config: ConnectionConfig;
      node: ConnNode;
      db: string;
      group: Group;
      count: number;
      open: boolean;
    }
  | {
      kind: "object";
      key: string;
      config: ConnectionConfig;
      node: ConnNode;
      db: string;
      object: SchemaObject;
      /** 3 under a group, 2 in a filtered list. */
      depth: number;
      /** A table in a group: whether its indexes and keys are shown. */
      open?: boolean;
    }
  | {
      kind: "table-part";
      key: string;
      config: ConnectionConfig;
      node: ConnNode;
      db: string;
      table: string;
      part: TablePart;
      count: number;
      open: boolean;
    }
  | {
      kind: "entry";
      key: string;
      config: ConnectionConfig;
      node: ConnNode;
      db: string;
      table: string;
      entry: { kind: "index"; index: IndexInfo } | { kind: "foreign_key"; foreignKey: ForeignKey };
    };

/** A group of a database's objects, in tree order. */
interface Group {
  id: string;
  label: string;
  icon: ObjectIconKind;
  kinds: string[];
}

const GROUPS: Group[] = [
  { id: "table", label: "Tables", icon: "table", kinds: ["table"] },
  { id: "view", label: "Views", icon: "view", kinds: ["view", "materialized_view"] },
  { id: "procedure", label: "Procedures", icon: "procedure", kinds: ["procedure"] },
  { id: "function", label: "Functions", icon: "function", kinds: ["function"] },
  { id: "trigger", label: "Triggers", icon: "trigger", kinds: ["trigger"] },
  { id: "event", label: "Events", icon: "event", kinds: ["event"] },
];
/** Objects of a kind no group lists (a driver's own kinds) are still shown. */
const OTHER: Group = { id: "other", label: "Other", icon: "function", kinds: [] };

/** What the tree shows under a table. */
type TablePart = "indexes" | "foreign_keys";
const PARTS: Array<{ id: TablePart; label: string; icon: ObjectIconKind }> = [
  { id: "indexes", label: "Indexes", icon: "index" },
  { id: "foreign_keys", label: "Foreign keys", icon: "foreign_key" },
];

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

/** A database's objects split by group, each sorted by name; once per list. */
const grouped = new WeakMap<SchemaObject[], Map<string, SchemaObject[]>>();
function groupObjects(objects: SchemaObject[]): Map<string, SchemaObject[]> {
  let groups = grouped.get(objects);
  if (!groups) {
    groups = new Map();
    const groupOf = new Map<string, string>();
    for (const g of GROUPS) for (const kind of g.kinds) groupOf.set(kind, g.id);
    for (const object of sortObjects(objects)) {
      const id = groupOf.get(object.kind) ?? OTHER.id;
      const list = groups.get(id);
      if (list) list.push(object);
      else groups.set(id, [object]);
    }
    grouped.set(objects, groups);
  }
  return groups;
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
  treeOpen: Record<string, boolean>,
  tableInfos: Record<string, TreeTableInfo>,
  schemaFilter: Record<string, string[]>,
): Line[] {
  const lines: Line[] = [];
  for (const config of connections) {
    const node = nodes[config.id];
    const connected = node?.status.kind === "connected";
    const nameMatches = !needle || config.name.toLowerCase().includes(needle);
    const databases: Array<{ db: DbNode; objects: SchemaObject[] | null }> = [];
    // Only the ticked schemas are listed; the rest are behind the count next to the connection.
    const shown =
      connected && node
        ? shownSchemas(config.database, node.databases, schemaFilter[config.id])
        : null;
    if (connected && node && (nameMatches ? node.expanded : true)) {
      for (const db of node.databases) {
        if (shown && !shown.has(db.name)) continue;
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
    if (connected && node.expanded && !needle && shown?.size === 0) {
      lines.push({
        kind: "note",
        key: `${config.id}#no-schemas`,
        text: "No schemas shown. Click the count to choose.",
        danger: false,
        depth: 0,
      });
    }
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
      if (objects) {
        for (const object of objects) {
          lines.push({
            kind: "object",
            key: `${dbKey}/${object.kind}:${object.name}`,
            config,
            node,
            db: db.name,
            object,
            depth: 2,
          });
        }
        continue;
      }
      const byGroup = groupObjects(db.objects ?? []);
      for (const group of [...GROUPS, OTHER]) {
        const list = byGroup.get(group.id);
        if (!list) continue;
        const groupKey = treeKey(config.id, db.name, `group:${group.id}`);
        const groupOpen = treeOpen[groupKey] ?? group.id === "table";
        lines.push({
          kind: "group",
          key: groupKey,
          config,
          node,
          db: db.name,
          group,
          count: list.length,
          open: groupOpen,
        });
        if (!groupOpen) continue;
        for (const object of list) {
          const tableKey = treeKey(config.id, db.name, `table:${object.name}`);
          const tableOpen = object.kind === "table" ? treeOpen[tableKey] === true : undefined;
          lines.push({
            kind: "object",
            key: `${dbKey}/${object.kind}:${object.name}`,
            config,
            node,
            db: db.name,
            object,
            depth: 3,
            open: tableOpen,
          });
          if (tableOpen)
            pushTableParts(lines, config, node, db.name, object.name, treeOpen, tableInfos);
        }
      }
    }
  }
  return lines;
}

/** The indexes and foreign keys of an open table, or why they are not there yet. */
function pushTableParts(
  lines: Line[],
  config: ConnectionConfig,
  node: ConnNode,
  db: string,
  table: string,
  treeOpen: Record<string, boolean>,
  tableInfos: Record<string, TreeTableInfo>,
) {
  const tableKey = treeKey(config.id, db, `table:${table}`);
  const loaded = tableInfos[`${config.id}/${db}/${table}`];
  if (!loaded?.info) {
    const text = loaded?.error ?? "Loading…";
    lines.push({
      kind: "note",
      key: `${tableKey}#note`,
      text,
      danger: Boolean(loaded?.error),
      depth: 3,
    });
    return;
  }
  const { indexes, foreign_keys } = loaded.info;
  for (const part of PARTS) {
    const entries = part.id === "indexes" ? indexes : foreign_keys;
    const key = `${tableKey}#${part.id}`;
    const open = treeOpen[key] === true;
    lines.push({
      kind: "table-part",
      key,
      config,
      node,
      db,
      table,
      part: part.id,
      count: entries.length,
      open,
    });
    if (!open) continue;
    if (part.id === "indexes") {
      // The primary key first, then the others by name.
      const ordered = [...indexes].sort(
        (a, b) => Number(b.primary) - Number(a.primary) || byName(a.name, b.name),
      );
      for (const index of ordered) {
        lines.push({
          kind: "entry",
          key: `${key}/${index.name}`,
          config,
          node,
          db,
          table,
          entry: { kind: "index", index },
        });
      }
    } else {
      for (const foreignKey of foreign_keys) {
        lines.push({
          kind: "entry",
          key: `${key}/${foreignKey.name}`,
          config,
          node,
          db,
          table,
          entry: { kind: "foreign_key", foreignKey },
        });
      }
    }
  }
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
  const treeOpen = useApp((s) => s.treeOpen);
  const tableInfos = useApp((s) => s.tableInfos);
  const schemaFilter = useApp((s) => s.schemaFilter);
  const lines = useMemo(
    () => linesOf(connections, nodes, needle, treeOpen, tableInfos, schemaFilter),
    [connections, nodes, needle, treeOpen, tableInfos, schemaFilter],
  );
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
    case "group":
      return <GroupLine line={line} active={active} onSelect={onSelect} />;
    case "object":
      return <ObjectLine line={line} active={active} onSelect={onSelect} />;
    case "table-part":
      return <TablePartLine line={line} active={active} onSelect={onSelect} />;
    case "entry":
      return <EntryLine line={line} active={active} onSelect={onSelect} />;
    case "note":
      return (
        <p
          title={line.text}
          style={line.depth === undefined ? undefined : { paddingLeft: 6 + line.depth * 12 + 22 }}
          className={`truncate py-1.5 text-sm ${line.depth === undefined ? "pl-12" : ""} ${line.danger ? "text-danger" : "text-faint"}`}
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
            {connected && node && <SchemaCount config={config} node={node} />}
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

/** "3/20": the schemas shown of all the connection has. A click opens the list to tick them. */
function SchemaCount({ config, node }: { config: ConnectionConfig; node: ConnNode }) {
  const saved = useApp((s) => s.schemaFilter[config.id]);
  const shown = shownSchemas(config.database, node.databases, saved).size;
  return (
    <button
      type="button"
      title="Choose the schemas to show"
      aria-label={`${shown} of ${node.databases.length} schemas shown. Choose schemas`}
      onClick={(event) => {
        event.stopPropagation();
        useApp.getState().setSchemaPicker(config.id);
      }}
      className="rounded-sm px-1 text-muted text-xs tabular-nums transition-colors-fast hover:bg-hover hover:text-fg"
    >
      {shown}/{node.databases.length}
    </button>
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
        depth={line.depth}
        expanded={line.open}
        onToggle={
          line.open === undefined
            ? undefined
            : () =>
                s.toggleTreeNode(treeKey(conn, db, `table:${object.name}`), {
                  conn,
                  database: db,
                  name: object.name,
                })
        }
        icon={<ObjectIcon kind={iconKindOf(object.kind)} />}
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

function GroupLine({
  line,
  active,
  onSelect,
}: {
  line: Extract<Line, { kind: "group" }>;
  active: boolean;
  onSelect: (key: string) => void;
}) {
  const { group } = line;
  return (
    <TreeRow
      depth={2}
      expanded={line.open}
      icon={<ObjectIcon kind={group.icon} />}
      label={group.label}
      active={active}
      onClick={() => {
        onSelect(line.key);
        useApp.getState().toggleTreeNode(line.key);
      }}
      trailing={<span className="text-faint text-sm tabular-nums">{line.count}</span>}
    />
  );
}

function TablePartLine({
  line,
  active,
  onSelect,
}: {
  line: Extract<Line, { kind: "table-part" }>;
  active: boolean;
  onSelect: (key: string) => void;
}) {
  const part = PARTS.find((p) => p.id === line.part);
  return (
    <TreeRow
      depth={4}
      expanded={line.open}
      icon={<ObjectIcon kind={part?.icon ?? "index"} />}
      label={part?.label ?? ""}
      active={active}
      onClick={() => {
        onSelect(line.key);
        useApp.getState().toggleTreeNode(line.key);
      }}
      trailing={<span className="text-faint text-sm tabular-nums">{line.count}</span>}
    />
  );
}

/** An index or a foreign key of a table. Opening one shows the table's structure. */
function EntryLine({
  line,
  active,
  onSelect,
}: {
  line: Extract<Line, { kind: "entry" }>;
  active: boolean;
  onSelect: (key: string) => void;
}) {
  const { config, db, table, entry } = line;
  const s = useApp.getState();
  const open = () => {
    onSelect(line.key);
    if (entry.kind === "index") s.openStructure(config.id, db, table, "indexes", entry.index.name);
    else s.openStructure(config.id, db, table, "foreign_keys", entry.foreignKey.name);
  };
  if (entry.kind === "index") {
    const { index } = entry;
    const detail = `${index.unique && !index.primary ? "unique · " : ""}${index.columns.join(", ")}`;
    return (
      <ContextMenuArea
        items={[
          { label: "Open structure", onSelect: open },
          { label: "Copy name", onSelect: () => copyText(index.name) },
        ]}
      >
        <TreeRow
          depth={5}
          icon={<ObjectIcon kind={index.primary ? "primary_key" : "index"} />}
          label={index.name}
          title={`${index.primary ? "Primary key" : index.unique ? "Unique index" : "Index"} (${index.columns.join(", ")})`}
          active={active}
          onClick={open}
          trailing={<EntryDetail text={detail} />}
        />
      </ContextMenuArea>
    );
  }
  const { foreignKey } = entry;
  const target = `${foreignKey.references.name}(${foreignKey.referenced_columns.join(", ")})`;
  return (
    <ContextMenuArea
      items={[
        { label: "Open structure", onSelect: open },
        {
          label: `Open ${foreignKey.references.name}`,
          onSelect: () =>
            s.openObject(
              config.id,
              foreignKey.references.database ?? db,
              foreignKey.references.name,
              "table",
            ),
        },
        { label: "Copy name", onSelect: () => copyText(foreignKey.name) },
      ]}
    >
      <TreeRow
        depth={5}
        icon={<ObjectIcon kind="foreign_key" />}
        label={foreignKey.name}
        title={`${foreignKey.columns.join(", ")} → ${target}`}
        active={active}
        onClick={open}
        trailing={<EntryDetail text={`→ ${target}`} />}
      />
    </ContextMenuArea>
  );
}

/** What an index or foreign key covers, after its name; it gives way to the name when short of room. */
function EntryDetail({ text }: { text: string }) {
  return <span className="max-w-[45%] truncate text-faint text-sm">{text}</span>;
}
