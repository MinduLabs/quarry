/**
 * Completion: what fits at the cursor (tables, columns, databases, keywords, functions) is read in
 * Rust from the statement around it (`quarry_sql::completion_context`); the names come from the
 * loaded schema. A column shared by joined tables is offered once.
 */
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import type { CompletionContext as SqlContext } from "@/bindings/CompletionContext";
import type { StatementTable } from "@/bindings/StatementTable";
import { completionContext } from "@/lib/engine";

/** Column names by database, then by table. */
export type SqlSchema = Record<string, Record<string, string[]>>;

/** How much text around the cursor goes to Rust: a big file is never copied whole on a
 * keystroke, and a statement is rarely longer. */
const CONTEXT_BEFORE = 64 * 1024;
const CONTEXT_AFTER = 16 * 1024;

const BARE_NAME = /^[A-Za-z_][\w$]*$/;

export function sqlCompletion(getSchema: () => { schema: SqlSchema; database: string | null }) {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const { state, pos } = context;
    const start = Math.max(0, pos - CONTEXT_BEFORE);
    const text = state.sliceDoc(start, Math.min(state.doc.length, pos + CONTEXT_AFTER));
    const sql = await completionContext(text, pos - start);
    if (!sql || context.aborted) return null;
    const options = optionsFor(sql, getSchema());
    if (options.length === 0) return null;
    return { from: start + sql.word_start, options, validFor: /^[\w$]*$/ };
  };
}

function optionsFor(
  sql: SqlContext,
  { schema, database }: { schema: SqlSchema; database: string | null },
): Completion[] {
  // With nothing typed, what the slot wants leads: a name after `WHERE`, the next keyword after
  // `ORDER BY total`.
  const nameBoost = sql.expects_name ? 50 : 0;
  const keywordBase = sql.expects_name ? 0 : 50;
  const options: Completion[] = [];
  const name = (label: string, type: string, detail?: string, boost = nameBoost): Completion => ({
    label,
    type,
    detail,
    apply: BARE_NAME.test(label) ? undefined : `\`${label.replaceAll("`", "``")}\``,
    boost,
  });
  const tablesOf = (db: string | null) => (db ? find(schema, db) : undefined);
  const columnsOf = (table: StatementTable) => {
    const tables = tablesOf(table.database ?? database);
    return (tables && find(tables, table.name)) ?? [];
  };

  const names = sql.names;
  if (names.kind === "objects") {
    for (const table of Object.keys(tablesOf(names.database ?? database) ?? {})) {
      options.push(name(table, "class", names.database ?? undefined));
    }
    if (names.database === null) {
      for (const db of Object.keys(schema)) {
        options.push(name(db, "namespace", undefined, nameBoost - 1));
      }
    }
  } else if (names.kind === "columns") {
    for (const table of names.tables) {
      for (const column of columnsOf(table)) options.push(name(column, "property", table.name));
    }
  } else if (names.kind === "expression") {
    const seen = new Set<string>();
    for (const table of sql.tables) {
      for (const column of columnsOf(table)) {
        if (seen.has(column)) continue;
        seen.add(column);
        options.push(name(column, "property", table.alias ?? table.name));
      }
    }
    // The alias or table name, to write `alias.` before a column.
    for (const table of sql.tables) {
      const prefix = table.alias ?? table.name;
      if (!seen.has(prefix)) options.push(name(prefix, "variable", table.name, nameBoost - 1));
    }
  }
  sql.keywords.forEach((keyword, index) => {
    options.push({ label: keyword, type: "keyword", boost: Math.max(-99, keywordBase - index) });
  });
  for (const fn of sql.functions) {
    options.push({ label: fn, type: "function", apply: `${fn}(`, boost: nameBoost - 2 });
  }
  return options;
}

function find<T>(record: Record<string, T>, name: string): T | undefined {
  if (name in record) return record[name];
  const lower = name.toLowerCase();
  const key = Object.keys(record).find((k) => k.toLowerCase() === lower);
  return key === undefined ? undefined : record[key];
}
