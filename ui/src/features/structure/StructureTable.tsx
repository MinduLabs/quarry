import { type ReactNode, useEffect, useRef } from "react";
import type { TableInfo } from "@/bindings/TableInfo";
import { ObjectIcon, type ObjectIconKind } from "@/components/ui/ObjectIcon";
import { cn } from "@/lib/cn";

/** A table row; the one the sidebar pointed at is marked and scrolled into view. */
export function Row({ children, focused }: { children: ReactNode; focused?: boolean }) {
  const ref = useRef<HTMLTableRowElement>(null);
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView({ block: "nearest" });
  }, [focused]);
  return (
    <tr
      ref={ref}
      className={cn("group border-line border-b last:border-b-0", focused && "bg-selected")}
    >
      {children}
    </tr>
  );
}

export function Table({
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
    <div>
      <table className="w-full border-collapse text-left text-sm">
        <thead className="sticky top-0 bg-surface text-muted">
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
export function RowActions({ children }: { children: ReactNode }) {
  return (
    <td className="h-7 px-1 align-middle">
      <span className="flex justify-end gap-0.5 opacity-0 transition-opacity duration-(--duration-fast) group-focus-within:opacity-100 group-hover:opacity-100">
        {children}
      </span>
    </td>
  );
}

export function Cell({
  children,
  mono,
  faint,
}: {
  children: ReactNode;
  mono?: boolean;
  faint?: boolean;
}) {
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

export function Empty({ children }: { children: ReactNode }) {
  return <p className="p-6 text-center text-sm text-muted">{children}</p>;
}

/** A name with the icon of what it names. */
export function NameCell({ icon, name }: { icon?: ObjectIconKind; name: string }) {
  return (
    <Cell>
      <span className="flex items-center gap-1.5">
        {icon && <ObjectIcon kind={icon} size={13} />}
        <span className="font-semibold text-fg">{name}</span>
      </span>
    </Cell>
  );
}

/** What every part of the structure gets: the table, the filter text and what may be changed. */
export interface PartProps {
  info: TableInfo;
  conn: string;
  /** Lower-cased filter text; empty shows everything. */
  needle: string;
  canEdit: boolean;
  /** The row the sidebar pointed at. */
  focus: string | null;
}

/** Whether any of `texts` holds the filter text. */
export function matchesNeedle(needle: string, ...texts: Array<string | null>): boolean {
  return needle === "" || texts.some((t) => t?.toLowerCase().includes(needle));
}
