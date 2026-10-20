import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp, ChevronDown, ChevronUp, X } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { SortKey } from "@/bindings/SortKey";
import type { Value } from "@/bindings/Value";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { ContextMenuArea, type MenuItem } from "@/components/ui/Menu";
import { cn } from "@/lib/cn";
import { type CopyFormat, formatCells } from "@/lib/copy";
import { copyValue, displayValue, isLongValue, isNumericType } from "@/lib/values";
import { type CellEdit, copyText, type Edits } from "@/store/app";
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
  /** Several cells at once, as one undo step. */
  onEditCells: (cells: CellEdit[]) => void;
  /** Mark or unmark rows for deletion, as one undo step. */
  onToggleDeleteRows: (rows: number[]) => void;
  onUndo: () => void;
  onRedo: () => void;
  /** Add a new row (only where rows can be inserted). */
  onInsertRow?: () => void;
}

/** Sorting by clicking a header, for table pages (the database sorts). */
export interface GridSorting {
  sort: SortKey[];
  onSort: (column: string) => void;
}

type Pos = { row: number; col: number };

/** First and last row a selection covers. */
export type RowSpan = { top: number; bottom: number };

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
export const ResultGrid = memo(function ResultGrid({
  columns,
  rows,
  resetKey,
  editing,
  sorting,
  onCopyInsert,
  onExport,
  onSelectRows,
  hidden,
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
  /** The rows the selection spans, when they change (`null` when nothing is selected). */
  onSelectRows?: (span: RowSpan | null) => void;
  /** Column positions (in `columns`) not shown. */
  hidden?: ReadonlySet<number>;
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

  // Columns on screen, left to right. Selection and focus count in these positions; edits,
  // values and widths use the column's index in `columns` (`colAt`).
  const shown = useMemo(
    () => columns.map((_, c) => c).filter((c) => !hidden?.has(c)),
    [columns, hidden],
  );
  const colAt = (position: number) => shown[position] ?? position;

  // biome-ignore lint/correctness/useExhaustiveDependencies: a changed column set invalidates positions
  useEffect(() => {
    setSel(null);
    setEditor(null);
  }, [shown]);

  const edits = editing?.edits;
  const inserted = edits?.inserts.length ?? 0;
  const count = rows.length + inserted;
  const lastCol = shown.length - 1;
  const deleted = useMemo(() => new Set(edits?.deletes ?? []), [edits?.deletes]);
  const numeric = useMemo(() => columns.map((c) => isNumericType(c.type_name)), [columns]);
  const shownWidths = shown.map((c) => widths[c] ?? MIN_WIDTH);
  const template = `${GUTTER}px ${shownWidths.map((w) => `${w}px`).join(" ")}`;
  const totalWidth = GUTTER + shownWidths.reduce((a, b) => a + b, 0);
  const rect = sel ? rectOf(sel) : null;

  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the spanned rows change
  useEffect(() => {
    onSelectRows?.(rect ? { top: rect.top, bottom: rect.bottom } : null);
  }, [rect?.top, rect?.bottom]);
  const cell = sel?.focus ?? null;

  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  // Find in results: cells whose full value contains the text, in the loaded rows only.
  const [finding, setFinding] = useState(false);
  /** What is searched: the find bar's text once typing pauses. */
  const [needle, setNeedle] = useState("");
  const [matchIndex, setMatchIndex] = useState(0);
  // Lower-case text of every shown cell, built once per result while the find bar is open, so a
  // keystroke only scans strings instead of formatting every value again.
  const searching = finding && needle !== "";
  const haystack = useMemo(() => {
    if (!searching) return [];
    const total = rows.length + (edits?.inserts.length ?? 0);
    const texts: string[] = new Array(total * shown.length);
    for (let row = 0; row < total; row++) {
      for (let col = 0; col < shown.length; col++) {
        const c = shown[col] ?? col;
        const value =
          row >= rows.length
            ? edits?.inserts[row - rows.length]?.[c]
            : (edits?.updates[row]?.[c] ?? rows[row]?.[c]);
        texts[row * shown.length + col] = value ? copyValue(value).toLowerCase() : "";
      }
    }
    return texts;
  }, [searching, rows, edits, shown]);
  /** Matching cells as `row * shown.length + position`, in reading order. */
  const matches = useMemo(() => {
    const found: number[] = [];
    if (!needle) return found;
    for (let i = 0; i < haystack.length; i++) {
      if (haystack[i]?.includes(needle)) found.push(i);
    }
    return found;
  }, [haystack, needle]);
  const matchKeys = useMemo(() => new Set(matches), [matches]);

  /** Select match `index` and scroll it into view, both ways. */
  const goToMatch = (index: number) => {
    const key = matches[index];
    if (key === undefined || shown.length === 0) return;
    const match = { row: Math.floor(key / shown.length), col: key % shown.length };
    setMatchIndex(index);
    setSel({ anchor: match, focus: match });
    virtualizer.scrollToIndex(match.row, { align: "center" });
    const el = scroller.current;
    if (!el) return;
    const left = GUTTER + shownWidths.slice(0, match.col).reduce((a, b) => a + b, 0);
    const right = left + (shownWidths[match.col] ?? MIN_WIDTH);
    if (left - GUTTER < el.scrollLeft) el.scrollLeft = left - GUTTER;
    else if (right > el.scrollLeft + el.clientWidth) el.scrollLeft = right - el.clientWidth;
  };

  // A new search starts at the first match.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the matches change
  useEffect(() => {
    setMatchIndex(0);
    if (matches.length > 0) goToMatch(0);
  }, [matches]);

  const openFind = () => {
    setFinding(true);
    // Already open: focus its field again.
    document.querySelector<HTMLInputElement>('input[aria-label="Find in results"]')?.select();
  };

  const closeFind = () => {
    setFinding(false);
    setNeedle("");
    scroller.current?.focus();
  };

  const stepMatch = (step: number) => {
    if (matches.length === 0) return;
    goToMatch((matchIndex + step + matches.length) % matches.length);
  };

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
    editing?.onEdit(pos.row, colAt(pos.col), value);
    setEditor(null);
    scroller.current?.focus();
  };

  /** Edit in place, or in the value dialog for values that don't fit one line. */
  const open = (pos: Pos) => {
    if (canEdit(pos) && !isLongValue(valueAt(pos.row, colAt(pos.col)))) setEditor(pos);
    else setViewer(pos);
  };

  /** The selected block: column names (as shown, or as in the table) and values. */
  const block = (sourceNames = false) => {
    if (!rect) return { names: [], values: [] };
    const cols = range(rect.left, rect.right).map(colAt);
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
    editing?.onToggleDeleteRows(selectedRows());
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (editor || viewer) return;
    const mod = event.metaKey || event.ctrlKey;
    const key = event.key.toLowerCase();
    if (mod && key === "f") {
      event.preventDefault();
      openFind();
      return;
    }
    if (mod && editing && (key === "z" || key === "y")) {
      event.preventDefault();
      if (key === "y" || event.shiftKey) editing.onRedo();
      else editing.onUndo();
      return;
    }
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
                  const cells = range(rect.top, rect.bottom)
                    .filter((r) => !deleted.has(r))
                    .flatMap((row) =>
                      range(rect.left, rect.right).map((position) => ({
                        row,
                        col: colAt(position),
                        value: { t: "null" as const },
                      })),
                    );
                  editing.onEditCells(cells);
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

  const viewed = viewer && columns[colAt(viewer.col)];

  const grid = (
    <div
      ref={scroller}
      tabIndex={0}
      role="grid"
      aria-rowcount={count}
      aria-colcount={shown.length}
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
          {shown.map((i) => {
            const column = columns[i] as ColumnMeta;
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
              {shown.map((c, p) => {
                const value = valueAt(r, c);
                const selected = inRect(rect, r, p);
                const focused = cell?.row === r && cell.col === p;
                const editingHere = editor?.row === r && editor.col === p;
                const found = matchKeys.has(r * shown.length + p);
                return (
                  <div
                    key={c}
                    role="gridcell"
                    aria-selected={selected}
                    onMouseDown={(e) => {
                      const pos = { row: r, col: p };
                      // A right-click inside the selection keeps it for the menu.
                      if (e.button === 2 && inRect(rect, r, p)) return;
                      if (e.button === 0) dragging.current = true;
                      setSel((s) => ({
                        anchor: e.shiftKey && e.button === 0 && s ? s.anchor : pos,
                        focus: pos,
                      }));
                    }}
                    onMouseEnter={() => {
                      if (dragging.current)
                        setSel((s) => s && { anchor: s.anchor, focus: { row: r, col: p } });
                    }}
                    onDoubleClick={() => open({ row: r, col: p })}
                    className={cn(
                      "relative flex items-center overflow-hidden px-2 tabular-nums",
                      numeric[c] && "justify-end",
                      (value === undefined || value.t === "null") && "text-faint italic",
                      isModified(r, c) && "bg-cell-modified",
                      found && "bg-cell-match",
                      selected && "bg-selected",
                      focused && "outline-1 outline-accent -outline-offset-1",
                    )}
                  >
                    {editingHere ? (
                      <CellEditor
                        initial={value === undefined ? "" : copyValue(value)}
                        onCommit={(text) => commit({ row: r, col: p }, { t: "text", v: text })}
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
    <div className="relative h-full min-h-0">
      <ContextMenuArea items={menu}>{grid}</ContextMenuArea>
      {finding && (
        <FindBar
          count={matches.length}
          current={matchIndex}
          searched={needle !== ""}
          onSearch={setNeedle}
          onStep={stepMatch}
          onClose={closeFind}
        />
      )}
      {viewer && viewed && (
        <ValueDialog
          column={viewed}
          value={valueAt(viewer.row, colAt(viewer.col))}
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
    </div>
  );
});

/** How long typing must pause before the grid searches. */
const FIND_DELAY_MS = 120;

/**
 * The find bar. It keeps what is typed itself, so a keystroke re-renders only the bar; the grid
 * searches once typing pauses, or at once on Enter.
 */
const FindBar = memo(function FindBar({
  count,
  current,
  searched,
  onSearch,
  onStep,
  onClose,
}: {
  count: number;
  current: number;
  searched: boolean;
  onSearch: (needle: string) => void;
  onStep: (step: number) => void;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pending = useRef(false);
  const search = (value: string) => {
    clearTimeout(timer.current);
    pending.current = false;
    onSearch(value.trim().toLowerCase());
  };
  useEffect(() => {
    input.current?.focus();
    return () => clearTimeout(timer.current);
  }, []);
  return (
    <div className="animate-pop absolute top-1 right-3 z-30 flex items-center gap-1 rounded-lg bg-elevated p-1 font-sans shadow-popover">
      <Input
        ref={input}
        aria-label="Find in results"
        placeholder="Find in results"
        value={text}
        onChange={(e) => {
          const value = e.target.value;
          setText(value);
          pending.current = true;
          clearTimeout(timer.current);
          timer.current = setTimeout(() => search(value), FIND_DELAY_MS);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            // Enter right after typing searches first; after that it steps through matches.
            if (pending.current) search(text);
            else onStep(e.shiftKey ? -1 : 1);
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          }
        }}
        className="w-52"
      />
      <span className="min-w-20 px-1 text-center text-muted text-sm tabular-nums">
        {!searched
          ? ""
          : count === 0
            ? "No results"
            : `${Math.min(current + 1, count)} of ${count.toLocaleString()}`}
      </span>
      <IconButton
        label="Previous match"
        size="sm"
        disabled={count === 0}
        onClick={() => onStep(-1)}
      >
        <ChevronUp size={14} strokeWidth={1.75} />
      </IconButton>
      <IconButton label="Next match" size="sm" disabled={count === 0} onClick={() => onStep(1)}>
        <ChevronDown size={14} strokeWidth={1.75} />
      </IconButton>
      <IconButton label="Close" size="sm" onClick={onClose}>
        <X size={14} strokeWidth={1.75} />
      </IconButton>
    </div>
  );
});
