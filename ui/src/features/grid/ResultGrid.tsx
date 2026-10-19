import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { SortKey } from "@/bindings/SortKey";
import type { Value } from "@/bindings/Value";
import { ContextMenuArea, type MenuItem } from "@/components/ui/Menu";
import { cn } from "@/lib/cn";
import { type CopyFormat, formatCells } from "@/lib/copy";
import { copyValue, displayValue, isLongValue, isNumericType } from "@/lib/values";
import { copyText, type Edits } from "@/store/app";
import { CellEditor } from "./CellEditor";
import { ValueDialog } from "./ValueDialog";

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

/** Grid editing, when the shown result can be written back to its table. */
export interface GridEditing {
  edits: Edits;
  onEdit: (row: number, col: number, value: Value) => void;
  onToggleDelete: (row: number) => void;
  /** Add a new row (only where rows can be inserted). */
  onInsertRow?: () => void;
}

/** Sorting by clicking a header, for table pages (the database sorts). */
export interface GridSorting {
  sort: SortKey[];
  onSort: (column: string) => void;
}

type Pos = { row: number; col: number };

/** A rectangle of cells: `anchor` is where it started, `focus` is the active cell. */
type Selection = { anchor: Pos; focus: Pos };

type Rect = { top: number; bottom: number; left: number; right: number };

function rectOf(sel: Selection): Rect {
  return {
    top: Math.min(sel.anchor.row, sel.focus.row),
    bottom: Math.max(sel.anchor.row, sel.focus.row),
    left: Math.min(sel.anchor.col, sel.focus.col),
    right: Math.max(sel.anchor.col, sel.focus.col),
  };
}

const inRect = (rect: Rect | null, row: number, col: number) =>
  rect !== null && row >= rect.top && row <= rect.bottom && col >= rect.left && col <= rect.right;

const range = (from: number, to: number) =>
  Array.from({ length: to - from + 1 }, (_, i) => from + i);

/**
 * Virtualized result grid (docs/STYLE-GUIDE.md › Data grid). Click, drag, Shift+click or
 * Shift+arrows select a block of cells; the row numbers select whole rows. The block copies as
 * TSV, CSV, JSON or INSERT. With `editing`, double-click or Enter edits a cell, Delete marks the
 * selected rows, and new rows follow the loaded ones. Rows are virtualized; columns are all
 * rendered (fine for typical result widths).
 */
export function ResultGrid({
  columns,
  rows,
  resetKey,
  editing,
  sorting,
  onCopyInsert,
  onExport,
  onSelectRows,
}: {
  columns: ColumnMeta[];
  rows: Value[][];
  resetKey: number;
  editing?: GridEditing;
  sorting?: GridSorting;
  /** Copy the given rows as an INSERT (the engine writes it). */
  onCopyInsert?: (columns: string[], rows: Value[][]) => void;
  /** Export the whole result (or table), not just the selection. */
  onExport?: () => void;
  /** The rows the selection spans, whenever they change (empty when nothing is selected). */
  onSelectRows?: (rows: number[]) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const [widths, setWidths] = useState<number[]>([]);
  const [sel, setSel] = useState<Selection | null>(null);
  const [editor, setEditor] = useState<Pos | null>(null);
  const [viewer, setViewer] = useState<Pos | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-measure when a new result arrives
  useEffect(() => {
    setWidths(measure(columns, rows));
    setSel(null);
    setEditor(null);
    setViewer(null);
    scroller.current?.scrollTo({ top: 0, left: 0 });
  }, [columns, resetKey]);

  // Widen columns once the first batch lands (the header-only measurement is too narrow).
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the row count crosses the sample size
  useEffect(() => {
    if (rows.length > 0 && rows.length <= 500) setWidths(measure(columns, rows));
  }, [rows.length > 0]);

  useEffect(() => {
    const stop = () => {
      dragging.current = false;
    };
    window.addEventListener("mouseup", stop);
    return () => window.removeEventListener("mouseup", stop);
  }, []);

  const edits = editing?.edits;
  const inserted = edits?.inserts.length ?? 0;
  const count = rows.length + inserted;
  const lastCol = columns.length - 1;
  const deleted = useMemo(() => new Set(edits?.deletes ?? []), [edits?.deletes]);
  const numeric = useMemo(() => columns.map((c) => isNumericType(c.type_name)), [columns]);
  const template = `${GUTTER}px ${widths.map((w) => `${w}px`).join(" ")}`;
  const totalWidth = GUTTER + widths.reduce((a, b) => a + b, 0);
  const rect = sel ? rectOf(sel) : null;

  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the spanned rows change
  useEffect(() => {
    onSelectRows?.(rect ? range(rect.top, rect.bottom) : []);
  }, [rect?.top, rect?.bottom]);
  const cell = sel?.focus ?? null;

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

  /** Edit in place, or in the value dialog for values that don't fit one line. */
  const open = (pos: Pos) => {
    if (canEdit(pos) && !isLongValue(valueAt(pos.row, pos.col))) setEditor(pos);
    else setViewer(pos);
  };

  /** The selected block: column names (as shown, or as in the table) and values. */
  const block = (sourceNames = false) => {
    if (!rect) return { names: [], values: [] };
    const cols = range(rect.left, rect.right);
    const names = cols.map((c) => {
      const column = columns[c];
      return (sourceNames ? column?.source_column : null) ?? column?.name ?? "";
    });
    const values = range(rect.top, rect.bottom).map((r) =>
      cols.map((c) => valueAt(r, c) ?? { t: "null" as const }),
    );
    return { names, values };
  };

  const copy = (format: CopyFormat) => {
    const { names, values } = block();
    if (values.length === 0) return;
    const one = values.length === 1 && names.length === 1;
    // A single cell copies as its full value, like before.
    if (format === "tsv" && one) return copyText(copyValue(values[0]?.[0]));
    copyText(formatCells(format, names, values));
  };

  const select = (focus: Pos, extend: boolean) => {
    setSel((s) => ({ anchor: extend && s ? s.anchor : focus, focus }));
    virtualizer.scrollToIndex(focus.row);
  };

  const selectedRows = () => (rect ? range(rect.top, rect.bottom) : []);

  const toggleDeleteSelected = () => {
    if (!editing) return;
    // New rows are removed by index, so remove from the bottom up.
    for (const row of selectedRows().reverse()) editing.onToggleDelete(row);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (editor || viewer) return;
    const mod = event.metaKey || event.ctrlKey;
    if (mod && event.key.toLowerCase() === "a" && count > 0) {
      event.preventDefault();
      setSel({ anchor: { row: 0, col: 0 }, focus: { row: count - 1, col: lastCol } });
      return;
    }
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
      const row = Math.min(count - 1, Math.max(0, cell.row + delta[0]));
      const col = Math.min(lastCol, Math.max(0, cell.col + delta[1]));
      select({ row, col }, event.shiftKey);
      return;
    }
    if (mod && event.key.toLowerCase() === "c") {
      event.preventDefault();
      copy("tsv");
      return;
    }
    if (event.key === "Enter" && event.shiftKey) {
      event.preventDefault();
      setViewer(cell);
      return;
    }
    if (event.key === "Enter" && canEdit(cell)) {
      event.preventDefault();
      open(cell);
      return;
    }
    if (event.key === "Escape" && sel) {
      setSel({ anchor: cell, focus: cell });
      return;
    }
    if ((event.key === "Delete" || event.key === "Backspace") && editing) {
      event.preventDefault();
      toggleDeleteSelected();
    }
  };

  const rowsSelected = selectedRows();
  const deletingNew = rowsSelected.length > 0 && rowsSelected.every((r) => r >= rows.length);
  const keeping = rowsSelected.length > 0 && rowsSelected.every((r) => deleted.has(r));
  const rowWord = rowsSelected.length === 1 ? "row" : `${rowsSelected.length} rows`;

  const menu: MenuItem[] = cell
    ? [
        {
          label: canEdit(cell) ? "Edit value…" : "Open value…",
          onSelect: () => setViewer(cell),
        },
        ...(canEdit(cell)
          ? [
              {
                label: "Set NULL",
                onSelect: () => {
                  if (!rect || !editing) return;
                  for (const r of range(rect.top, rect.bottom)) {
                    if (deleted.has(r)) continue;
                    for (const c of range(rect.left, rect.right)) {
                      editing.onEdit(r, c, { t: "null" });
                    }
                  }
                },
              },
            ]
          : []),
        {
          label: "Copy",
          onSelect: () => copy("tsv"),
          separatorBefore: true,
        },
        { label: "Copy with headers", onSelect: () => copy("tsv_headers") },
        { label: "Copy as CSV", onSelect: () => copy("csv") },
        { label: "Copy as JSON", onSelect: () => copy("json") },
        ...(onCopyInsert
          ? [
              {
                label: "Copy as INSERT",
                onSelect: () => {
                  const { names, values } = block(true);
                  onCopyInsert(names, values);
                },
              },
            ]
          : []),
        ...(onExport ? [{ label: "Export…", onSelect: onExport }] : []),
        ...(editing?.onInsertRow
          ? [{ label: "Insert row", onSelect: editing.onInsertRow, separatorBefore: true }]
          : []),
        ...(editing
          ? [
              {
                label: deletingNew
                  ? `Remove new ${rowWord}`
                  : keeping
                    ? `Keep ${rowWord}`
                    : `Delete ${rowWord}`,
                onSelect: toggleDeleteSelected,
                danger: !deletingNew && !keeping,
                separatorBefore: !editing.onInsertRow,
              },
            ]
          : []),
      ]
    : [];

  const viewed = viewer && columns[viewer.col];

  const grid = (
    <div
      ref={scroller}
      tabIndex={0}
      role="grid"
      aria-rowcount={count}
      aria-colcount={columns.length}
      aria-multiselectable
      onKeyDown={onKeyDown}
      className="relative h-full min-h-0 select-none overflow-auto bg-app font-mono text-sm outline-none"
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
          <button
            type="button"
            aria-label="Select all"
            tabIndex={-1}
            onClick={() =>
              count > 0 &&
              setSel({ anchor: { row: 0, col: 0 }, focus: { row: count - 1, col: lastCol } })
            }
            className="sticky left-0 z-10 border-line border-r bg-surface hover:bg-hover"
          />
          {columns.map((column, i) => {
            const source = column.source_column ?? column.name;
            const key = sorting?.sort.find((k) => k.column === source);
            const Arrow = key?.descending ? ArrowDown : ArrowUp;
            const label = (
              <>
                <span className="truncate">{column.name}</span>
                {key && <Arrow size={12} strokeWidth={2} className="shrink-0 text-accent-text" />}
              </>
            );
            return (
              <div
                // biome-ignore lint/suspicious/noArrayIndexKey: column names may repeat; position is the identity
                key={`${column.name}:${i}`}
                role="columnheader"
                aria-sort={key ? (key.descending ? "descending" : "ascending") : undefined}
                title={`${column.name} · ${column.type_name}`}
                className="relative flex border-line border-r font-semibold text-fg"
              >
                {sorting ? (
                  <button
                    type="button"
                    onClick={() => sorting.onSort(source)}
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-1 px-2 text-left outline-none",
                      "transition-colors-fast hover:bg-hover focus-visible:bg-hover",
                      numeric[i] && "justify-end",
                    )}
                  >
                    {label}
                  </button>
                ) : (
                  <div
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-1 px-2",
                      numeric[i] && "justify-end",
                    )}
                  >
                    {label}
                  </div>
                )}
                <span
                  onPointerDown={(e) => startResize(i, e)}
                  className="absolute top-0 right-[-3px] z-10 h-full w-1.5 cursor-col-resize hover:bg-accent-soft"
                />
              </div>
            );
          })}
        </div>

        {/* Rows */}
        {virtualizer.getVirtualItems().map((item) => {
          const r = item.index;
          const isNew = r >= rows.length;
          const isDeleted = deleted.has(r);
          const rowSelected = rect !== null && r >= rect.top && r <= rect.bottom;
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
              <div
                onMouseDown={(e) => {
                  if (e.button !== 0) return;
                  dragging.current = true;
                  setSel((s) => ({
                    anchor: e.shiftKey && s ? { row: s.anchor.row, col: 0 } : { row: r, col: 0 },
                    focus: { row: r, col: lastCol },
                  }));
                  scroller.current?.focus();
                }}
                onMouseEnter={() => {
                  if (dragging.current)
                    setSel((s) => s && { anchor: s.anchor, focus: { row: r, col: lastCol } });
                }}
                className={cn(
                  "sticky left-0 z-10 flex cursor-default items-center justify-end border-line border-r bg-surface pr-2 font-sans text-faint text-xs tabular-nums",
                  rowSelected && "text-fg",
                )}
              >
                {isNew ? "+" : r + 1}
              </div>
              {columns.map((_, c) => {
                const value = valueAt(r, c);
                const selected = inRect(rect, r, c);
                const focused = cell?.row === r && cell.col === c;
                const editingHere = editor?.row === r && editor.col === c;
                return (
                  <div
                    // biome-ignore lint/suspicious/noArrayIndexKey: cells are positional
                    key={c}
                    role="gridcell"
                    aria-selected={selected}
                    onMouseDown={(e) => {
                      const pos = { row: r, col: c };
                      // A right-click inside the selection keeps it for the menu.
                      if (e.button === 2 && inRect(rect, r, c)) return;
                      if (e.button === 0) dragging.current = true;
                      setSel((s) => ({
                        anchor: e.shiftKey && e.button === 0 && s ? s.anchor : pos,
                        focus: pos,
                      }));
                    }}
                    onMouseEnter={() => {
                      if (dragging.current)
                        setSel((s) => s && { anchor: s.anchor, focus: { row: r, col: c } });
                    }}
                    onDoubleClick={() => open({ row: r, col: c })}
                    className={cn(
                      "relative flex items-center overflow-hidden px-2 tabular-nums",
                      numeric[c] && "justify-end",
                      (value === undefined || value.t === "null") && "text-faint italic",
                      isModified(r, c) && "bg-cell-modified",
                      selected && "bg-selected",
                      focused && "outline-1 outline-accent -outline-offset-1",
                    )}
                  >
                    {editingHere ? (
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

  return (
    <>
      <ContextMenuArea items={menu}>{grid}</ContextMenuArea>
      {viewer && viewed && (
        <ValueDialog
          column={viewed}
          value={valueAt(viewer.row, viewer.col)}
          editable={canEdit(viewer)}
          onSave={(value) => {
            commit(viewer, value);
            setViewer(null);
          }}
          onClose={() => {
            setViewer(null);
            scroller.current?.focus();
          }}
        />
      )}
    </>
  );
}
