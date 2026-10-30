/** `{{name}}` marks a value the user fills in before the query runs. */
const VARIABLE = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;

/** The same pattern as a source string, for tools that take their own regex (the SQL formatter). */
export const VARIABLE_PATTERN = VARIABLE.source;

/** `:name` outside a string or comment. `:=` and `@x:=1` are assignments, not variables. */
const NAMED = /^:([A-Za-z_][A-Za-z0-9_]*)/;

type Slot = { from: number; to: number; name: string; label: string; hint: string };

/** A value the user fills in. `name` keys the typed value; `label` and `hint` are what the dialog shows. */
export type QueryVariable = Pick<Slot, "name" | "label" | "hint">;

/**
 * Every `?` and `:name` that is code. One inside a string, a quoted identifier or a comment is
 * text and stays as it is. A bare `?` is named by its position (`?1`, `?2`, …) and shown as `#1`
 * with the field it is compared with, so the user can tell which one it is.
 */
function codePlaceholders(sql: string): Slot[] {
  const found: Slot[] = [];
  let positional = 0;
  let i = 0;
  while (i < sql.length) {
    const c = sql[i];
    const next = sql[i + 1];
    if (c === "'" || c === '"' || c === "`") {
      i = endOfQuoted(sql, i, c);
    } else if (c === "#" || (c === "-" && next === "-" && /\s|^$/.test(sql[i + 2] ?? ""))) {
      const eol = sql.indexOf("\n", i);
      i = eol === -1 ? sql.length : eol + 1;
    } else if (c === "/" && next === "*") {
      const end = sql.indexOf("*/", i + 2);
      i = end === -1 ? sql.length : end + 2;
    } else if (c === "?") {
      positional += 1;
      found.push({
        from: i,
        to: i + 1,
        name: `?${positional}`,
        label: `#${positional}`,
        hint: fieldBefore(sql, i),
      });
      i += 1;
    } else if (c === ":" && !isWordChar(sql[i - 1]) && sql[i - 1] !== ":") {
      const named = NAMED.exec(sql.slice(i, i + 80));
      if (named?.[1]) {
        const name = named[1];
        found.push({
          from: i,
          to: i + named[0].length,
          name: `:${name}`,
          label: `:${name}`,
          hint: "",
        });
        i += named[0].length;
      } else {
        i += 1;
      }
    } else {
      i += 1;
    }
  }
  return found;
}

function isWordChar(c: string | undefined): boolean {
  return c !== undefined && /[A-Za-z0-9_]/.test(c);
}

const IDENT = "(?:`[^`]+`|[A-Za-z_][\\w$]*)(?:\\.(?:`[^`]+`|[A-Za-z_][\\w$]*))*";
/** A column, or a call on one such as `DATE(created_at)`. */
const OPERAND = `(?:[A-Za-z_]\\w*\\([^()]*\\)|${IDENT})`;
const COMPARE = "(?:<=>|<>|!=|>=|<=|=|<|>|(?:NOT\\s+)?(?:LIKE|REGEXP|RLIKE)|IS(?:\\s+NOT)?)";

/** What the text before a `?` says the value is compared with, most specific first. */
const FIELD_BEFORE = [
  new RegExp(`(${OPERAND})\\s*${COMPARE}\\s*$`, "i"),
  new RegExp(`(${OPERAND})\\s+(?:NOT\\s+)?IN\\s*\\([^()]*$`, "i"),
  new RegExp(`(${OPERAND})\\s+(?:NOT\\s+)?BETWEEN\\s+(?:\\S+\\s+AND\\s+)?$`, "i"),
  /\b(LIMIT|OFFSET)\s*(?:\d+\s*,\s*)?$/i,
];

/** The field the `?` at `at` is compared with (`id`, `a.employee_id`, `DATE(created_at)`), or "". */
function fieldBefore(sql: string, at: number): string {
  const before = sql.slice(Math.max(0, at - 200), at);
  for (const pattern of FIELD_BEFORE) {
    const field = pattern.exec(before)?.[1];
    if (field) return field.replaceAll("`", "");
  }
  return "";
}

/** Index just past the quoted run that opens at `start`. A backslash escapes; a doubled quote stays inside. */
function endOfQuoted(sql: string, start: number, quote: string): number {
  let i = start + 1;
  while (i < sql.length) {
    if (sql[i] === "\\" && quote !== "`") i += 2;
    else if (sql[i] === quote && sql[i + 1] === quote) i += 2;
    else if (sql[i] === quote) return i + 1;
    else i += 1;
  }
  return sql.length;
}

/** Every placeholder in `sql`, left to right. */
function slots(sql: string): Slot[] {
  const braces: Slot[] = [...sql.matchAll(VARIABLE)].map((m) => ({
    from: m.index,
    to: m.index + m[0].length,
    name: m[1] ?? "",
    label: m[1] ?? "",
    hint: "",
  }));
  return [...braces, ...codePlaceholders(sql)]
    .filter((s) => s.name)
    .sort((a, b) => a.from - b.from);
}

/** The distinct variables in `sql`, in order of first use. */
export function queryVariables(sql: string): QueryVariable[] {
  const seen = new Map<string, QueryVariable>();
  for (const { name, label, hint } of slots(sql)) {
    if (!seen.has(name)) seen.set(name, { name, label, hint });
  }
  return [...seen.values()];
}

/**
 * Replace each variable with its text, as typed. Nothing is quoted or escaped: `'{{name}}'` and
 * `LIMIT {{n}}` both work because the user writes the quotes. The engine still checks the final
 * statement against safe mode, so a filled-in `DROP` asks like any other.
 */
export function fillVariables(sql: string, values: Record<string, string>): string {
  let out = "";
  let at = 0;
  for (const slot of slots(sql)) {
    out += sql.slice(at, slot.from) + (values[slot.name] ?? sql.slice(slot.from, slot.to));
    at = slot.to;
  }
  return out + sql.slice(at);
}
