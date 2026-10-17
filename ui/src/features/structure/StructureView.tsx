import { KeyRound } from "lucide-react";
import type { ReactNode } from "react";
import type { TableInfo } from "@/bindings/TableInfo";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import type { TabStructure } from "@/store/app";

/** Columns, indexes, foreign keys and DDL of the table a browse tab shows. Read-only. */
export function StructureView({ structure }: { structure: TabStructure | null }) {
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
  return <Structure info={structure.info} />;
}

function Structure({ info }: { info: TableInfo }) {
  const keyColumns = new Set(info.primary_key);
  return (
    <div className="animate-fade h-full overflow-auto">
      <div className="flex max-w-5xl flex-col gap-6 p-4">
        <Section title="Columns" count={info.columns.length}>
          <Table head={["Name", "Type", "Null", "Default", "Extra", "Comment"]}>
            {info.columns.map((c) => (
              <tr key={c.name} className="border-line border-b last:border-b-0">
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
              </tr>
            ))}
          </Table>
        </Section>

        <Section title="Indexes" count={info.indexes.length}>
          {info.indexes.length === 0 ? (
            <Empty>No indexes</Empty>
          ) : (
            <Table head={["Name", "Columns", "Kind"]}>
              {info.indexes.map((i) => (
                <tr key={i.name} className="border-line border-b last:border-b-0">
                  <Cell>
                    <span className="font-semibold text-fg">{i.name}</span>
                  </Cell>
                  <Cell mono>{i.columns.join(", ")}</Cell>
                  <Cell>{i.primary ? "Primary key" : i.unique ? "Unique" : "Index"}</Cell>
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
          <Section title="DDL">
            <pre className="overflow-auto rounded-lg border border-line bg-surface p-3 font-mono text-sm text-fg">
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
  children,
}: {
  title: string;
  count?: number;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="font-semibold text-base text-fg">
        {title}
        {count !== undefined && <span className="ml-1.5 font-normal text-muted">{count}</span>}
      </h2>
      {children}
    </section>
  );
}

function Table({ head, children }: { head: string[]; children: ReactNode }) {
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
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
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
