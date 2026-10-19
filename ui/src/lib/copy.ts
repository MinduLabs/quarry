import type { Value } from "@/bindings/Value";
import { copyValue } from "@/lib/values";

/** Text formats a block of cells can be copied as. `insert` is built by the engine. */
export type CopyFormat = "tsv" | "tsv_headers" | "csv" | "json";

/** `rows` (already cut to the selected columns) as text in `format`. NULL is empty in TSV and
 * CSV, `null` in JSON. */
export function formatCells(format: CopyFormat, columns: string[], rows: Value[][]): string {
  switch (format) {
    case "tsv":
      return rows.map((row) => row.map(tsvCell).join("\t")).join("\n");
    case "tsv_headers":
      return [columns.map(tsvCell), ...rows.map((row) => row.map(tsvCell))]
        .map((line) => line.join("\t"))
        .join("\n");
    case "csv":
      return [columns.map(csvField), ...rows.map((row) => row.map(csvField))]
        .map((line) => line.join(","))
        .join("\r\n");
    case "json":
      return JSON.stringify(
        rows.map((row) => Object.fromEntries(columns.map((c, i) => [c, jsonValue(row[i])]))),
        null,
        2,
      );
  }
}

function tsvCell(value: Value | string | undefined): string {
  const text = typeof value === "string" ? value : copyValue(value);
  // A tab or newline inside a value would split it into several cells.
  return text.replace(/[\t\r\n]+/g, " ");
}

function csvField(value: Value | string | undefined): string {
  const text = typeof value === "string" ? value : copyValue(value);
  if (typeof value !== "string" && (value === undefined || value.t === "null")) return "";
  return /[",\r\n]/.test(text) || text !== text.trim() ? `"${text.replaceAll('"', '""')}"` : text;
}

/** JSON keeps numbers that fit a JS number exactly, and the rest as strings. */
function jsonValue(value: Value | undefined): unknown {
  if (!value) return null;
  switch (value.t) {
    case "null":
      return null;
    case "bool":
      return value.v;
    case "float":
      return Number.isFinite(value.v) ? value.v : String(value.v);
    case "int":
    case "u_int": {
      const n = Number(value.v);
      return Number.isSafeInteger(n) ? n : value.v;
    }
    case "json":
      try {
        return JSON.parse(value.v);
      } catch {
        return value.v;
      }
    case "array":
      return value.v.map(jsonValue);
    default:
      return copyValue(value);
  }
}
