import { ChevronLeft, ChevronRight } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { IconButton } from "@/components/ui/IconButton";
import { Spinner } from "@/components/ui/Spinner";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { PAGE_SIZE, type Tab, useApp } from "@/store/app";

/**
 * Previous / next page of a browse tab, in the status bar. Counting a large table is slow, so the
 * total is the table statistics' estimate (without filters) until the user asks for an exact
 * count by clicking the total itself; the exact number then takes its place, marked for a moment
 * so the change is seen.
 */
export function Pager({ tab }: { tab: Tab }) {
  const browse = useApp((s) => s.browse);
  const countRows = useApp((s) => s.countRows);
  const [isFresh, setFresh] = useState(false);
  const wasCounting = useRef(false);
  useEffect(() => {
    if (wasCounting.current && !tab.counting && tab.rowCount !== null) {
      setFresh(true);
      const timer = setTimeout(() => setFresh(false), 1600);
      wasCounting.current = false;
      return () => clearTimeout(timer);
    }
    wasCounting.current = tab.counting;
  }, [tab.counting, tab.rowCount]);
  if (!tab.browse || !tab.paged) return null;
  const rows = tab.sets[tab.activeSet]?.rows.length ?? 0;
  const offset = tab.browse.offset;
  const filtered = tab.browse.query.filters.length > 0;
  const estimate = filtered ? null : (tab.structure?.info?.estimated_rows ?? null);
  const label =
    rows === 0 ? "No rows" : `${(offset + 1).toLocaleString()}–${(offset + rows).toLocaleString()}`;
  const isLast = rows < PAGE_SIZE || (tab.rowCount !== null && offset + rows >= tab.rowCount);

  let total: ReactNode = null;
  if (tab.counting) {
    total = (
      <span className="flex items-center gap-1.5">
        of <Spinner size={11} />
      </span>
    );
  } else if (tab.rowCount !== null) {
    total = (
      <span className={cn(isFresh && "font-medium text-accent-text")}>
        of {tab.rowCount.toLocaleString()} rows
      </span>
    );
  } else if (rows > 0) {
    total = (
      <Tooltip content="Count exactly">
        <button
          type="button"
          disabled={tab.running}
          onClick={() => countRows(tab.id)}
          className="rounded-sm px-0.5 underline decoration-dotted underline-offset-2 transition-colors-fast hover:text-fg disabled:opacity-45"
        >
          {estimate !== null ? `of ~${estimate.toLocaleString()} rows` : "of ? rows"}
        </button>
      </Tooltip>
    );
  }

  return (
    <div className="flex items-center gap-1">
      <IconButton
        label="Previous page"
        size="sm"
        disabled={tab.running || offset === 0}
        onClick={() => browse(tab.id, offset - PAGE_SIZE)}
      >
        <ChevronLeft size={15} strokeWidth={1.75} />
      </IconButton>
      <span className="flex items-center gap-1 tabular-nums">
        {label}
        {total}
      </span>
      <IconButton
        label="Next page"
        size="sm"
        disabled={tab.running || isLast}
        onClick={() => browse(tab.id, offset + PAGE_SIZE)}
      >
        <ChevronRight size={15} strokeWidth={1.75} />
      </IconButton>
    </div>
  );
}
