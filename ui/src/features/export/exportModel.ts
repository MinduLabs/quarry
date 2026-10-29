import type { BrowseQuery } from "@/bindings/BrowseQuery";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { ExportFormat } from "@/bindings/ExportFormat";
import type { ExportFormatState } from "@/bindings/ExportFormatState";
import type { ExportOptions } from "@/bindings/ExportOptions";
import type { TableRef } from "@/bindings/TableRef";
import type { Value } from "@/bindings/Value";

export const FORMAT_LABELS: Record<ExportFormat, string> = {
  csv: "CSV",
  tsv: "TSV",
  json: "JSON",
  ndjson: "NDJSON (one JSON object per line)",
  sql_inserts: "SQL Inserts",
  sql_updates: "SQL Updates",
  where_clause: "Where Clause",
  xlsx: "Excel (xlsx)",
};

/** Excel opens a sheet of about this many rows slowly. */
export const EXCEL_SLOW_ROWS = 1_000_000;

export const defaultExportOptions = (): ExportOptions => ({
  delimiter: ",",
  has_header: true,
  has_bom: false,
  max_rows_per_sheet: null,
  table: null,
  key: [],
});

/** Where the rows of an export come from. */
export type ExportFrom =
  | {
      kind: "rows";
      tab: number;
      conn: string;
      /** A table name or `result`: the suggested file name. */
      name: string;
      columns: ColumnMeta[];
      /** The rows the grid holds. */
      rows: Value[][];
      /** The query that made them, when the grid holds only part of its result and the query can
       * run again for the rest. */
      query: { sql: string; database: string | null } | null;
    }
  | {
      kind: "table";
      conn: string;
      name: string;
      table: TableRef;
      query: BrowseQuery;
    };

/** `grid`: the rows the grid holds. `query`: every row, from running the query again. */
export type ExportScope = "grid" | "query";

export interface ExportPreviewState {
  text: string | null;
  error: string | null;
  loading: boolean;
}

export interface ExportDialogState {
  from: ExportFrom;
  /** The engine answered `ExportPrepare`. */
  isPrepared: boolean;
  /** Why the export cannot be offered at all. */
  error: string | null;
  formats: ExportFormatState[];
  /** The one table the rows come from, if there is one. */
  table: TableRef | null;
  /** A table source: its columns and first rows, which the preview shows. */
  sampleColumns: ColumnMeta[];
  sampleRows: Value[][];
  /** What may make a second run of the query differ from the first. */
  warnings: string[];
  /** Why "every row" cannot be offered, when it cannot. */
  queryError: string | null;
  /** A table source: its exact row count, once the engine has counted. */
  count: number | null;
  isCounting: boolean;
  format: ExportFormat;
  options: ExportOptions;
  scope: ExportScope;
  /** The number of rows typed; empty means all. */
  limit: string;
  /** The table SQL Inserts goes into, typed, when the rows come from no single table. */
  targetTable: string;
  preview: ExportPreviewState;
  /** The save dialog is open. */
  isChoosingFile: boolean;
}

/** An export that is running, for the progress card. */
export interface ExportRun {
  request: number;
  name: string;
  rows: number;
  bytes: number;
}

export function initialExportDialog(from: ExportFrom): ExportDialogState {
  return {
    from,
    isPrepared: false,
    error: null,
    formats: [],
    table: null,
    sampleColumns: [],
    sampleRows: [],
    warnings: [],
    queryError: null,
    count: null,
    isCounting: from.kind === "table",
    format: "csv",
    options: defaultExportOptions(),
    scope: "grid",
    limit: "",
    targetTable: "",
    preview: { text: null, error: null, loading: false },
    isChoosingFile: false,
  };
}

/** CSV when it can be used, since it is what most people want; else the first format offered. */
export function firstFormat(formats: ExportFormatState[]): ExportFormat | null {
  const usable = formats.filter((f) => f.is_available);
  return (usable.find((f) => f.format === "csv") ?? usable[0])?.format ?? null;
}

export function formatState(d: ExportDialogState): ExportFormatState | undefined {
  return d.formats.find((f) => f.format === d.format);
}

/** The rows typed: `null` for all of them, `"invalid"` for text that is not a whole number
 * above zero. */
export function parseLimit(text: string): number | null | "invalid" {
  const trimmed = text.trim().replaceAll(",", "");
  if (trimmed === "") return null;
  if (!/^\d+$/.test(trimmed)) return "invalid";
  const n = Number(trimmed);
  return Number.isSafeInteger(n) && n > 0 ? n : "invalid";
}

/** How many rows the choices would write, when that is known. */
export function expectedRows(d: ExportDialogState): number | null {
  const limit = parseLimit(d.limit);
  const available =
    d.from.kind === "table" ? d.count : d.scope === "grid" ? d.from.rows.length : null;
  if (limit === "invalid") return available;
  if (limit === null) return available;
  return available === null ? limit : Math.min(limit, available);
}

/** `db.table` or `table`, as typed, as a table reference. */
export function tableFromName(text: string): TableRef | null {
  const parts = text
    .split(".")
    .map((part) => part.trim().replace(/^`|`$/g, ""))
    .filter((part) => part !== "");
  const name = parts.at(-1);
  if (!name) return null;
  return { database: parts.length > 1 ? (parts.at(-2) ?? null) : null, schema: null, name };
}

/** SQL Inserts needs a table the user names when the rows come from none. */
export function needsTargetTable(d: ExportDialogState): boolean {
  return d.format === "sql_inserts" && d.table === null;
}

/** The options the engine gets: the dialog's, with the typed table where one is needed. */
export function exportOptions(d: ExportDialogState): ExportOptions {
  return { ...d.options, table: needsTargetTable(d) ? tableFromName(d.targetTable) : null };
}

/** The columns and rows the preview is made of. */
export function previewInput(d: ExportDialogState): { columns: ColumnMeta[]; rows: Value[][] } {
  return d.from.kind === "table"
    ? { columns: d.sampleColumns, rows: d.sampleRows }
    : { columns: d.from.columns, rows: d.from.rows };
}

/** The reasons formats cannot be used, each with the formats it applies to. */
export function unavailableReasons(
  formats: ExportFormatState[],
): Array<{ labels: string[]; reason: string }> {
  const byReason = new Map<string, string[]>();
  for (const f of formats) {
    if (f.is_available || !f.reason) continue;
    byReason.set(f.reason, [...(byReason.get(f.reason) ?? []), FORMAT_LABELS[f.format]]);
  }
  return [...byReason].map(([reason, labels]) => ({ labels, reason }));
}

/** `1.2 MB`: what `bytes` is, in the unit a person reads. */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = bytes;
  let unit = 0;
  while (size >= 1000 && unit < units.length - 1) {
    size /= 1000;
    unit += 1;
  }
  return `${unit === 0 ? size : size.toFixed(size < 10 ? 1 : 0)} ${units[unit]}`;
}
