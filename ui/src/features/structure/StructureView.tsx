import { Copy, KeyRound, Pencil, Plus, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import type { TableInfo } from "@/bindings/TableInfo";
import { Button } from "@/components/ui/Button";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { IconButton } from "@/components/ui/IconButton";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { copyText, type Tab, useApp } from "@/store/app";

const ICON = { size: 14, strokeWidth: 1.75 } as const;

/**
 * Columns, indexes, foreign keys and DDL of the table a browse tab shows. Columns and indexes
 * can be added, changed and dropped when the driver supports it and the connection is writable;
 * every change is shown as SQL before it runs.
 */
export function StructureView({ tab }: { tab: Tab }) {
  const structure = tab.structure;
  const canChange = useApp((s) => {
    const config = s.connections.find((c) => c.id === tab.connId);
    const status = s.nodes[tab.connId]?.status;
    return status?.kind === "connected" && status.capabilities.schema_edit && !config?.read_only;
  });
  if (!structure || structure.loading) {
    return (
      <div className="animate-fade flex h-full items-center justify-center">
        <Spinner size={18} />
      </div>
    );
  }
  if (structure.error || !structure.info) {
    return (
      <div className="p-4">
        <ErrorNotice message={structure.error ?? "No structure available."} />
      </div>
    );
  }
  const info = structure.info;
  return <Structure info={info} conn={tab.connId} canEdit={canChange && !info.view} />;
}

function Structure({ info, conn, canEdit }: { info: TableInfo; conn: string; canEdit: boolean }) {
  const { openSchemaForm, alterSchema } = useApp.getState();
  const keyColumns = new Set(info.primary_key);
  const table = info.table;
  const drop = (op: { op: "drop_column" | "drop_index"; name: string }) =>
    alterSchema(conn, { kind: "alter_table", table, ops: [op] });

  return (
    <div className="animate-fade h-full overflow-auto">
      <div className="flex max-w-5xl flex-col gap-6 p-4">
        <Section
          title="Columns"
          count={info.columns.length}
          action={
            canEdit && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => openSchemaForm({ kind: "column", conn, table, column: null })}
              >
                <Plus {...ICON} />
                Add column…
              </Button>
            )
          }
        >
          <Table head={["Name", "Type", "Null", "Default", "Extra", "Comment"]} actions={canEdit}>
            {info.columns.map((c) => (
              <tr key={c.name} className="group border-line border-b last:border-b-0">
                <Cell>
                  <span className="flex items-center gap-1.5">
                    {keyColumns.has(c.name) && (
                      <KeyRound
                        size={13}
                        strokeWidth={1.75}
                        className="text-warning"
                        aria-label="Primary key"
                      />
                    )}
                    <span className="font-semibold text-fg">{c.name}</span>
                  </span>
                </Cell>
                <Cell mono>{c.type_name}</Cell>
                <Cell>{c.nullable ? "Yes" : "No"}</Cell>
                <Cell mono faint={c.default === null}>
                  {c.default ?? "NULL"}
                </Cell>
                <Cell mono>{c.extra ?? ""}</Cell>
                <Cell>{c.comment ?? ""}</Cell>
                {canEdit && (
                  <RowActions>
                    <IconButton
                      label={c.generated ? "Generated columns are edited in SQL" : "Edit column…"}
                      size="sm"
                      disabled={c.generated}
                      onClick={() => openSchemaForm({ kind: "column", conn, table, column: c })}
                    >
                      <Pencil {...ICON} />
                    </IconButton>
                    <IconButton
                      label="Drop column…"
                      size="sm"
                      disabled={info.columns.length === 1}
                      onClick={() => drop({ op: "drop_column", name: c.name })}
                    >
                      <Trash2 {...ICON} />
                    </IconButton>
                  </RowActions>
                )}
              </tr>
            ))}
          </Table>
        </Section>

        <Section
          title="Indexes"
          count={info.indexes.length}
          action={
            canEdit && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => openSchemaForm({ kind: "index", conn, table, columns: [] })}
              >
                <Plus {...ICON} />
                Add index…
              </Button>
            )
          }
        >
          {info.indexes.length === 0 ? (
            <Empty>No indexes</Empty>
          ) : (
            <Table head={["Name", "Columns", "Kind"]} actions={canEdit}>
              {info.indexes.map((i) => (
                <tr key={i.name} className="group border-line border-b last:border-b-0">
                  <Cell>
                    <span className="font-semibold text-fg">{i.name}</span>
                  </Cell>
                  <Cell mono>{i.columns.join(", ")}</Cell>
                  <Cell>{i.primary ? "Primary key" : i.unique ? "Unique" : "Index"}</Cell>
                  {canEdit && (
                    <RowActions>
                      <IconButton
                        label={i.primary ? "Drop primary key…" : "Drop index…"}
                        size="sm"
                        onClick={() => drop({ op: "drop_index", name: i.name })}
                      >
                        <Trash2 {...ICON} />
                      </IconButton>
                    </RowActions>
                  )}
                </tr>
              ))}
            </Table>
          )}
        </Section>

        <Section title="Foreign keys" count={info.foreign_keys.length}>
          {info.foreign_keys.length === 0 ? (
            <Empty>No foreign keys</Empty>
          ) : (
            <Table head={["Name", "Columns", "References", "On update", "On delete"]}>
              {info.foreign_keys.map((fk) => (
                <tr key={fk.name} className="border-line border-b last:border-b-0">
                  <Cell>
                    <span className="font-semibold text-fg">{fk.name}</span>
                  </Cell>
                  <Cell mono>{fk.columns.join(", ")}</Cell>
                  <Cell mono>
                    {fk.references.name}({fk.referenced_columns.join(", ")})
                  </Cell>
                  <Cell>{fk.on_update ?? ""}</Cell>
                  <Cell>{fk.on_delete ?? ""}</Cell>
                </tr>
              ))}
            </Table>
          )}
        </Section>

        {info.ddl && (
          <Section
            title="DDL"
            action={
              <Button
                size="sm"
                variant="ghost"
                onClick={() => info.ddl && copyText(info.ddl, "Copied DDL")}
              >
                <Copy {...ICON} />
                Copy
              </Button>
            }
          >
            <pre className="select-text overflow-auto rounded-lg border border-line bg-surface p-3 font-mono text-sm text-fg">
              {info.ddl}
            </pre>
          </Section>
        )}
      </div>
    </div>
  );
}

function Section({
  title,
  count,
  action,
  children,
}: {
  title: string;
  count?: number;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <div className="flex h-6 items-center justify-between">
        <h2 className="font-semibold text-base text-fg">
          {title}
          {count !== undefined && <span className="ml-1.5 font-normal text-muted">{count}</span>}
        </h2>
        {action}
      </div>
      {children}
    </section>
  );
}

function Table({
  head,
  actions,
  children,
}: {
  head: string[];
  /** Rows end with a cell of buttons. */
  actions?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="overflow-auto rounded-lg border border-line">
      <table className="w-full border-collapse text-left text-sm">
        <thead className="bg-surface text-muted">
          <tr className="border-line border-b">
            {head.map((h) => (
              <th key={h} className="h-7 px-2 font-semibold">
                {h}
              </th>
            ))}
            {actions && (
              <th className="w-16">
                <span className="sr-only">Actions</span>
              </th>
            )}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

/** Buttons at the end of a row, shown on hover or keyboard focus. */
function RowActions({ children }: { children: ReactNode }) {
  return (
    <td className="h-7 px-1 align-middle">
      <span className="flex justify-end gap-0.5 opacity-0 transition-opacity duration-(--duration-fast) group-focus-within:opacity-100 group-hover:opacity-100">
        {children}
      </span>
    </td>
  );
}

function Cell({ children, mono, faint }: { children: ReactNode; mono?: boolean; faint?: boolean }) {
  return (
    <td
      className={cn(
        "h-7 px-2 align-middle",
        mono && "font-mono",
        faint ? "text-faint italic" : "text-fg",
      )}
    >
      {children}
    </td>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <p className="text-sm text-muted">{children}</p>;
}
