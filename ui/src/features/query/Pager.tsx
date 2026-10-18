import { ChevronLeft, ChevronRight } from "lucide-react";
import { IconButton } from "@/components/ui/IconButton";
import { PAGE_SIZE, type Tab, useApp } from "@/store/app";

/**
 * Previous / next page of a browse tab, in the status bar. Counting a large table is slow, so the
 * total is the table statistics' estimate (without filters) until the user asks for an exact count.
 */
export function Pager({ tab }: { tab: Tab }) {
  const browse = useApp((s) => s.browse);
  const countRows = useApp((s) => s.countRows);
  if (!tab.browse || !tab.paged) return null;
  const rows = tab.sets[tab.activeSet]?.rows.length ?? 0;
  const offset = tab.browse.offset;
  const filtered = tab.browse.query.filters.length > 0;
  const estimate = filtered ? null : (tab.structure?.info?.estimated_rows ?? null);
  const label =
    rows === 0
      ? "No rows"
      : `Rows ${(offset + 1).toLocaleString()}–${(offset + rows).toLocaleString()}`;
  const total =
    tab.rowCount !== null
      ? ` of ${tab.rowCount.toLocaleString()}`
      : estimate !== null
        ? ` of ~${estimate.toLocaleString()}`
        : "";
  const isLast = rows < PAGE_SIZE || (tab.rowCount !== null && offset + rows >= tab.rowCount);

  return (
    <div className="flex items-center gap-1">
      <IconButton
        label="Previous page"
        size="sm"
        disabled={tab.running || offset === 0}
        onClick={() => browse(tab.id, offset - PAGE_SIZE)}
      >
        <ChevronLeft size={14} strokeWidth={1.75} />
      </IconButton>
      <span className="tabular-nums">
        {label}
        {total}
      </span>
      <IconButton
        label="Next page"
        size="sm"
        disabled={tab.running || isLast}
        onClick={() => browse(tab.id, offset + PAGE_SIZE)}
      >
        <ChevronRight size={14} strokeWidth={1.75} />
      </IconButton>
      {tab.rowCount === null && (
        <button
          type="button"
          disabled={tab.running || tab.counting}
          onClick={() => countRows(tab.id)}
          className="h-5 rounded-sm px-1.5 text-xs transition-colors-fast hover:bg-hover hover:text-fg disabled:opacity-45"
        >
          {tab.counting ? "Counting…" : "Count rows"}
        </button>
      )}
    </div>
  );
}
