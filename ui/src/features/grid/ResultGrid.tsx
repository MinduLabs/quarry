import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { Value } from "@/bindings/Value";
import { ContextMenuArea, type MenuItem } from "@/components/ui/Menu";
import { cn } from "@/lib/cn";
import { copyValue, displayValue, isNumericType } from "@/lib/values";
import type { Edits } from "@/store/app";
import { CellEditor } from "./CellEditor";

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

/** Grid editing, when the shown page belongs to a table with a primary key. */
export interface GridEditing {
  edits: Edits;
  onEdit: (row: number, col: number, value: Value) => void;
  onToggleDelete: (row: number) => void;
}

type Pos = { row: number; col: number };

/**
 * Virtualized result grid (docs/STYLE-GUIDE.md › Data grid). Read-only unless `editing` is set:
 * then double-click or Enter edits a cell, Delete marks the row, and new rows follow the loaded
 * ones. Rows are virtualized; columns are all rendered (fine for typical result widths).
 */
export function ResultGrid({
  columns,
  rows,
  resetKey,
  editing,
}: {
  columns: ColumnMeta[];
  rows: Value[][];
  resetKey: number;
  editing?: GridEditing;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [widths, setWidths] = useState<number[]>([]);
  const [cell, setCell] = useState<Pos | null>(null);
  const [editor, setEditor] = useState<Pos | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-measure when a new result arrives
  useEffect(() => {
    setWidths(measure(columns, rows));
    setCell(null);
    setEditor(null);
    scroller.current?.scrollTo({ top: 0, left: 0 });
  }, [columns, resetKey]);

  // Widen columns once the first batch lands (the header-only measurement is too narrow).
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the row count crosses the sample size
  useEffect(() => {
    if (rows.length > 0 && rows.length <= 500) setWidths(measure(columns, rows));
  }, [rows.length > 0]);

  const edits = editing?.edits;
  const inserted = edits?.inserts.length ?? 0;
  const count = rows.length + inserted;
  const deleted = useMemo(() => new Set(edits?.deletes ?? []), [edits?.deletes]);
  const numeric = useMemo(() => columns.map((c) => isNumericType(c.type_name)), [columns]);
  const template = `${GUTTER}px ${widths.map((w) => `${w}px`).join(" ")}`;
  const totalWidth = GUTTER + widths.reduce((a, b) => a + b, 0);

  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  // A new row was added: show it.
  useEffect(() => {
    if (inserted > 0) virtualizer.scrollToIndex(rows.length + inserted - 1);
  }, [inserted, rows.length, virtualizer]);

  /** The value shown in a cell: the pending edit if any, else the loaded one. */
  const valueAt = (row: number, col: number): Value | undefined => {
    if (row >= rows.length) return edits?.inserts[row - rows.length]?.[col];
    return edits?.updates[row]?.[col] ?? rows[row]?.[col];
  };
  const isModified = (row: number, col: number) =>
    row < rows.length && edits?.updates[row]?.[col] !== undefined;

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

  const canEdit = (pos: Pos | null): pos is Pos => Boolean(editing && pos && !deleted.has(pos.row));

  const commit = (pos: Pos, value: Value) => {
    editing?.onEdit(pos.row, pos.col, value);
    setEditor(null);
    scroller.current?.focus();
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (!cell || editor) return;
    const moves: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
    };
    const delta = moves[event.key];
    if (delta) {
      event.preventDefault();
      const row = Math.min(count - 1, Math.max(0, cell.row + delta[0]));
      const col = Math.min(columns.length - 1, Math.max(0, cell.col + delta[1]));
      setCell({ row, col });
      virtualizer.scrollToIndex(row);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === "c") {
      event.preventDefault();
      void navigator.clipboard.writeText(copyValue(valueAt(cell.row, cell.col)));
      return;
    }
    if (event.key === "Enter" && canEdit(cell)) {
      event.preventDefault();
      setEditor(cell);
      return;
    }
    if ((event.key === "Delete" || event.key === "Backspace") && editing) {
      event.preventDefault();
      editing.onToggleDelete(cell.row);
    }
  };

  const menu: MenuItem[] =
    editing && cell
      ? [
          ...(canEdit(cell)
            ? [
                { label: "Edit", onSelect: () => setEditor(cell) },
                { label: "Set NULL", onSelect: () => commit(cell, { t: "null" }) },
              ]
            : []),
          {
            label: "Copy",
            onSelect: () =>
              void navigator.clipboard.writeText(copyValue(valueAt(cell.row, cell.col))),
          },
          {
            label:
              cell.row >= rows.length
                ? "Remove new row"
                : deleted.has(cell.row)
                  ? "Keep row"
                  : "Delete row",
            onSelect: () => editing.onToggleDelete(cell.row),
            danger: cell.row < rows.length && !deleted.has(cell.row),
            separatorBefore: true,
          },
        ]
      : [];

  const grid = (
    <div
      ref={scroller}
      tabIndex={0}
      role="grid"
      aria-rowcount={count}
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
          const r = item.index;
          const isNew = r >= rows.length;
          const isDeleted = deleted.has(r);
          return (
            <div
              key={item.key}
              role="row"
              style={{
                gridTemplateColumns: template,
                height: ROW_HEIGHT,
                transform: `translateY(${HEADER_HEIGHT + item.start}px)`,
              }}
              className={cn(
                "absolute top-0 left-0 grid w-full border-line/70 border-b hover:bg-hover",
                isNew && "bg-cell-inserted",
                isDeleted && "bg-cell-deleted text-muted line-through",
              )}
            >
              <div className="sticky left-0 z-10 flex items-center justify-end border-line border-r bg-surface pr-2 font-sans text-faint text-xs tabular-nums">
                {isNew ? "+" : r + 1}
              </div>
              {columns.map((_, c) => {
                const value = valueAt(r, c);
                const selected = cell?.row === r && cell.col === c;
                const open = editor?.row === r && editor.col === c;
                return (
                  <div
                    // biome-ignore lint/suspicious/noArrayIndexKey: cells are positional
                    key={c}
                    role="gridcell"
                    aria-selected={selected}
                    onMouseDown={() => setCell({ row: r, col: c })}
                    onDoubleClick={() => {
                      const pos = { row: r, col: c };
                      if (canEdit(pos)) setEditor(pos);
                    }}
                    className={cn(
                      "relative flex items-center overflow-hidden px-2 tabular-nums",
                      numeric[c] && "justify-end",
                      (value === undefined || value.t === "null") && "text-faint italic",
                      isModified(r, c) && "bg-cell-modified",
                      selected && "bg-selected outline-1 outline-accent -outline-offset-1",
                    )}
                  >
                    {open ? (
                      <CellEditor
                        initial={value === undefined ? "" : copyValue(value)}
                        onCommit={(text) => commit({ row: r, col: c }, { t: "text", v: text })}
                        onCancel={() => {
                          setEditor(null);
                          scroller.current?.focus();
                        }}
                      />
                    ) : (
                      <span className="truncate">
                        {value === undefined ? "DEFAULT" : displayValue(value)}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );

  return editing ? <ContextMenuArea items={menu}>{grid}</ContextMenuArea> : grid;
}
