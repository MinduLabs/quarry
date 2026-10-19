import { Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { ColumnFilter } from "@/bindings/ColumnFilter";
import type { FilterOp } from "@/bindings/FilterOp";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { type Option, Select } from "@/components/ui/Select";
import { type Tab, useApp } from "@/store/app";

const OPS: Option<FilterOp>[] = [
  { value: "eq", label: "=" },
  { value: "not_eq", label: "≠" },
  { value: "lt", label: "<" },
  { value: "lte", label: "≤" },
  { value: "gt", label: ">" },
  { value: "gte", label: "≥" },
  { value: "contains", label: "contains" },
  { value: "not_contains", label: "does not contain" },
  { value: "starts_with", label: "starts with" },
  { value: "in", label: "in list" },
  { value: "is_null", label: "is NULL" },
  { value: "is_not_null", label: "is not NULL" },
];

const takesValue = (op: FilterOp) => op !== "is_null" && op !== "is_not_null";

/**
 * Filters of a browse tab, run by the database. Edits stay local until applied, so typing never
 * reloads the page.
 */
export function FilterBar({
  tab,
  columns,
  onClose,
}: {
  tab: Tab;
  columns: string[];
  /** Called when the last filter is removed, so the bar goes away with it. */
  onClose: () => void;
}) {
  const applied = tab.browse?.query.filters;
  const [draft, setDraft] = useState<ColumnFilter[]>(applied ?? []);

  useEffect(() => setDraft(applied ?? []), [applied]);

  if (!tab.browse) return null;
  const options = columns.map((c) => ({ value: c, label: c }));
  const first = columns[0] ?? "";
  const rows = draft.length > 0 ? draft : [{ column: first, op: "eq" as const, value: "" }];

  const update = (index: number, patch: Partial<ColumnFilter>) =>
    setDraft(rows.map((f, i) => (i === index ? { ...f, ...patch } : f)));
  const apply = (filters: ColumnFilter[]) => {
    const query = tab.browse?.query;
    if (query) useApp.getState().setBrowseQuery(tab.id, { ...query, filters });
  };
  const complete = rows.filter((f) => f.column && (!takesValue(f.op) || f.value.trim() !== ""));

  return (
    <form
      className="flex shrink-0 flex-col gap-1 border-line border-b bg-surface px-2 py-1.5"
      onSubmit={(e) => {
        e.preventDefault();
        apply(complete);
      }}
    >
      {rows.map((filter, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: filters have no identity besides position
        <div key={index} className="flex items-center gap-1.5">
          <div className="w-48 shrink-0">
            <Select
              ariaLabel="Column"
              value={filter.column}
              options={options}
              onChange={(column) => update(index, { column })}
            />
          </div>
          <div className="w-40 shrink-0">
            <Select
              ariaLabel="Condition"
              value={filter.op}
              options={OPS}
              onChange={(op) => update(index, { op })}
            />
          </div>
          {takesValue(filter.op) ? (
            <Input
              aria-label="Value"
              className="font-mono"
              placeholder={filter.op === "in" ? "a, b, c" : "Value"}
              value={filter.value}
              onChange={(e) => update(index, { value: e.target.value })}
            />
          ) : (
            <div className="flex-1" />
          )}
          <IconButton
            label="Remove filter"
            size="sm"
            onClick={() => {
              const rest = rows.filter((_, i) => i !== index);
              setDraft(rest);
              if (applied?.length) apply(rest.filter((f) => complete.includes(f)));
              // The form always shows one row, so removing the last one would only blank it.
              if (rest.length === 0) onClose();
            }}
          >
            <X size={14} strokeWidth={1.75} />
          </IconButton>
        </div>
      ))}
      <div className="flex items-center gap-1.5">
        <Button
          size="sm"
          variant="ghost"
          onClick={() => setDraft([...rows, { column: first, op: "eq", value: "" }])}
        >
          <Plus size={14} strokeWidth={1.75} />
          Add filter
        </Button>
        <div className="flex-1" />
        {(applied?.length ?? 0) > 0 && (
          <Button size="sm" variant="ghost" disabled={tab.running} onClick={() => apply([])}>
            Clear
          </Button>
        )}
        <Button type="submit" size="sm" variant="primary" disabled={tab.running}>
          Apply
        </Button>
      </div>
    </form>
  );
}
