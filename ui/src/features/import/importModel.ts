import type { ImportOptions } from "@/bindings/ImportOptions";
import type { TableRef } from "@/bindings/TableRef";

export const defaultImportOptions = (): ImportOptions => ({
  delimiter: ",",
  has_header: true,
  is_empty_null: true,
  on_duplicate: "fail",
});

/** Where each column of the file goes: a table column, or `null` to leave it out. */
export type ImportMapping = (string | null)[];

export interface ImportDialogState {
  conn: string;
  table: TableRef;
  /** The file the user chose, or `null` before one is. */
  path: string | null;
  options: ImportOptions;
  /** The table's column names, or `null` while they load. */
  tableColumns: string[] | null;
  header: string[];
  rows: string[][];
  width: number;
  mapping: ImportMapping;
  /** What went wrong reading the file or the table; the dialog shows it in place of the form. */
  error: string | null;
  isChoosingFile: boolean;
  isPreviewing: boolean;
  /** The engine asked to confirm a write to a production database. */
  confirm: string | null;
}

export const initialImportDialog = (conn: string, table: TableRef): ImportDialogState => ({
  conn,
  table,
  path: null,
  options: defaultImportOptions(),
  tableColumns: null,
  header: [],
  rows: [],
  width: 0,
  mapping: [],
  error: null,
  isChoosingFile: false,
  isPreviewing: false,
  confirm: null,
});

/** Pair each file column with the table column of the same name, ignoring case. A table column
 * is used once, so two file columns never land in one. */
export function autoMap(header: string[], width: number, tableColumns: string[]): ImportMapping {
  const taken = new Set<string>();
  return Array.from({ length: width }, (_, index) => {
    const name = header[index]?.trim().toLowerCase();
    if (!name) return null;
    const match = tableColumns.find((c) => c.toLowerCase() === name && !taken.has(c));
    if (match === undefined) return null;
    taken.add(match);
    return match;
  });
}

/** Whether at least one column goes somewhere, and none goes twice. */
export function isMappingValid(mapping: ImportMapping): boolean {
  const used = mapping.filter((c): c is string => c !== null);
  return used.length > 0 && new Set(used.map((c) => c.toLowerCase())).size === used.length;
}

export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** TSV by extension, otherwise CSV: the guess the delimiter starts from. */
export function guessDelimiter(path: string): string {
  return path.toLowerCase().endsWith(".tsv") ? "\t" : ",";
}
