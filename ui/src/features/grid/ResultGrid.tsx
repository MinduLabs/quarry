import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { Value } from "@/bindings/Value";
import { cn } from "@/lib/cn";
import { copyValue, displayValue, isNumericType } from "@/lib/values";

const ROW_HEIGHT = 26;
const HEADER_HEIGHT = 28;
const GUTTER = 48;
const CHAR_WIDTH = 7.3; // JetBrains Mono at 12px
const MIN_WIDTH = 64;
const MAX_WIDTH = 420;

/** Initial column widths from the header and the first rows. */
function measure(columns: ColumnMeta[], rows: Value[][]): number[] {
  return columns.map((column, c) => {
    let chars = column.name.length + 2;
    for (const row of rows.slice(0, 80)) chars = Math.max(chars, displayValue(row[c]).length);
    return Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, chars * CHAR_WIDTH + 18)));
  });
}

/**
 * Read-only, virtualized result grid (docs/STYLE-GUIDE.md › Data grid).
 * Rows are virtualized; columns are all rendered (fine for typical result widths).
 */
export function ResultGrid({
  columns,
  rows,
  resetKey,
}: {
  columns: ColumnMeta[];
  rows: Value[][];
  resetKey: number;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [widths, setWidths] = useState<number[]>([]);
  const [cell, setCell] = useState<{ row: number; col: number } | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-measure when a new result arrives
  useEffect(() => {
    setWidths(measure(columns, rows));
    setCell(null);
    scroller.current?.scrollTo({ top: 0, left: 0 });
  }, [columns, resetKey]);

  // Widen columns once the first batch lands (the header-only measurement is too narrow).
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the row count crosses the sample size
  useEffect(() => {
    if (rows.length > 0 && rows.length <= 500) setWidths(measure(columns, rows));
  }, [rows.length > 0]);

  const numeric = useMemo(() => columns.map((c) => isNumericType(c.type_name)), [columns]);
  const template = `${GUTTER}px ${widths.map((w) => `${w}px`).join(" ")}`;
  const totalWidth = GUTTER + widths.reduce((a, b) => a + b, 0);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  const startResize = (index: number, event: React.PointerEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const start = widths[index] ?? MIN_WIDTH;
    const move = (e: PointerEvent) =>
      setWidths((w) =>
        w.map((v, i) => (i === index ? Math.max(40, start + e.clientX - startX) : v)),
      );
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (!cell) return;
    const moves: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
    };
    const delta = moves[event.key];
    if (delta) {
      event.preventDefault();
      const row = Math.min(rows.length - 1, Math.max(0, cell.row + delta[0]));
      const col = Math.min(columns.length - 1, Math.max(0, cell.col + delta[1]));
      setCell({ row, col });
      virtualizer.scrollToIndex(row);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === "c") {
      event.preventDefault();
      void navigator.clipboard.writeText(copyValue(rows[cell.row]?.[cell.col]));
    }
  };

  return (
    <div
      ref={scroller}
      tabIndex={0}
      role="grid"
      aria-rowcount={rows.length}
      aria-colcount={columns.length}
      onKeyDown={onKeyDown}
      className="relative h-full min-h-0 overflow-auto bg-app font-mono text-sm outline-none"
    >
      <div
        style={{ width: totalWidth, height: HEADER_HEIGHT + virtualizer.getTotalSize() }}
        className="relative"
      >
        {/* Header */}
        <div
          role="row"
          style={{ gridTemplateColumns: template, height: HEADER_HEIGHT }}
          className="sticky top-0 z-20 grid border-line border-b bg-surface font-sans text-sm"
        >
          <div className="sticky left-0 z-10 border-line border-r bg-surface" />
          {columns.map((column, i) => (
            <div
              // biome-ignore lint/suspicious/noArrayIndexKey: column names may repeat; position is the identity
              key={`${column.name}:${i}`}
              role="columnheader"
              title={`${column.name} · ${column.type_name}`}
              className={cn(
                "relative flex items-center gap-1 border-line border-r px-2 font-semibold text-fg",
                numeric[i] && "justify-end",
              )}
            >
              <span className="truncate">{column.name}</span>
              <span
                onPointerDown={(e) => startResize(i, e)}
                className="absolute top-0 right-[-3px] z-10 h-full w-1.5 cursor-col-resize hover:bg-accent-soft"
              />
            </div>
          ))}
        </div>

        {/* Rows */}
        {virtualizer.getVirtualItems().map((item) => {
          const row = rows[item.index] ?? [];
          return (
            <div
              key={item.key}
              role="row"
              style={{
                gridTemplateColumns: template,
                height: ROW_HEIGHT,
                transform: `translateY(${HEADER_HEIGHT + item.start}px)`,
              }}
              className="absolute top-0 left-0 grid w-full border-line/70 border-b hover:bg-hover"
            >
              <div className="sticky left-0 z-10 flex items-center justify-end border-line border-r bg-surface pr-2 font-sans text-faint text-xs tabular-nums">
                {item.index + 1}
              </div>
              {columns.map((_, c) => {
                const value = row[c];
                const selected = cell?.row === item.index && cell.col === c;
                return (
                  <div
                    // biome-ignore lint/suspicious/noArrayIndexKey: cells are positional
                    key={c}
                    role="gridcell"
                    aria-selected={selected}
                    onMouseDown={() => setCell({ row: item.index, col: c })}
                    className={cn(
                      "flex items-center overflow-hidden px-2 tabular-nums",
                      numeric[c] && "justify-end",
                      value?.t === "null" && "text-faint italic",
                      selected && "bg-selected outline-1 outline-accent -outline-offset-1",
                    )}
                  >
                    <span className="truncate">{displayValue(value)}</span>
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}
