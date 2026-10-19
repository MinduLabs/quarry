/** `{{name}}` marks a value the user fills in before the query runs. */
const VARIABLE = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;

/** The distinct variable names in `sql`, in order of first use. */
export function queryVariables(sql: string): string[] {
  return [...new Set([...sql.matchAll(VARIABLE)].map((m) => m[1] ?? ""))].filter(Boolean);
}

/**
 * Replace each variable with its text, as typed. Nothing is quoted or escaped: `'{{name}}'` and
 * `LIMIT {{n}}` both work because the user writes the quotes. The engine still checks the final
 * statement against safe mode, so a filled-in `DROP` asks like any other.
 */
export function fillVariables(sql: string, values: Record<string, string>): string {
  return sql.replace(VARIABLE, (match, name: string) => values[name] ?? match);
}
