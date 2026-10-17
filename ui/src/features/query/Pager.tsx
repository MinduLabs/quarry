import { ChevronLeft, ChevronRight } from "lucide-react";
import { IconButton } from "@/components/ui/IconButton";
import { PAGE_SIZE, type Tab, useApp } from "@/store/app";

/**
 * Previous / next page of a browse tab, in the status bar. There is no total: counting a large
 * table is slow, so "next" is off once a page comes back shorter than a full page.
 */
export function Pager({ tab }: { tab: Tab }) {
  const browse = useApp((s) => s.browse);
  if (!tab.browse || !tab.paged) return null;
  const rows = tab.sets[tab.activeSet]?.rows.length ?? 0;
  const offset = tab.browse.offset;
  const label =
    rows === 0
      ? "No rows"
      : `Rows ${(offset + 1).toLocaleString()}–${(offset + rows).toLocaleString()}`;

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
      <span className="tabular-nums">{label}</span>
      <IconButton
        label="Next page"
        size="sm"
        disabled={tab.running || rows < PAGE_SIZE}
        onClick={() => browse(tab.id, offset + PAGE_SIZE)}
      >
        <ChevronRight size={14} strokeWidth={1.75} />
      </IconButton>
    </div>
  );
}
