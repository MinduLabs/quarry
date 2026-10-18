import { cn } from "@/lib/cn";

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/** Keys that say the most about a step, shown next to its name (MySQL and MariaDB spellings). */
const HIGHLIGHTS = [
  "access_type",
  "key",
  "rows_examined_per_scan",
  "rows",
  "filtered",
  "using_filesort",
  "using_temporary_table",
];

/**
 * A query plan in the JSON form (`EXPLAIN FORMAT=JSON`), as a tree that opens a few levels deep.
 * The JSON layout differs between servers, so this shows any shape and only picks out common keys.
 */
export function ExplainTree({ text }: { text: string }) {
  let plan: Json;
  try {
    plan = JSON.parse(text) as Json;
  } catch {
    return (
      <pre className="h-full overflow-auto p-4 font-mono text-sm text-fg whitespace-pre-wrap">
        {text}
      </pre>
    );
  }
  return (
    <div className="h-full overflow-auto p-3 font-mono text-sm">
      <Node name="plan" value={plan} depth={0} />
    </div>
  );
}

function Node({ name, value, depth }: { name: string; value: Json; depth: number }) {
  if (value === null || typeof value !== "object") {
    return (
      <div className="flex gap-2 py-px pl-5">
        <span className="text-muted">{name}</span>
        <span className={cn("text-fg", typeof value === "number" && "tabular-nums")}>
          {String(value)}
        </span>
      </div>
    );
  }
  const entries: [string, Json][] = Array.isArray(value)
    ? value.map((v, i) => [String(i), v])
    : Object.entries(value);
  const facts = Array.isArray(value)
    ? []
    : HIGHLIGHTS.filter((k) => k in value && typeof value[k] !== "object").map((k) => ({
        key: k,
        text: String(value[k]),
      }));
  const table = !Array.isArray(value) && typeof value.table_name === "string" && value.table_name;
  return (
    <details open={depth < 6} className="pl-2">
      <summary className="flex cursor-default items-center gap-2 rounded-sm py-px outline-none hover:bg-hover focus-visible:bg-hover">
        <span className="font-semibold text-fg">{name}</span>
        {table && <span className="text-accent-text">{table}</span>}
        {facts.map((f) => (
          <span
            key={f.key}
            className={cn(
              "rounded-sm bg-surface px-1 text-xs text-muted",
              f.key === "access_type" && f.text === "ALL" && "text-warning",
            )}
          >
            {f.key}: {f.text}
          </span>
        ))}
      </summary>
      <div className="border-line border-l pl-1">
        {entries.map(([key, child]) => (
          <Node key={key} name={key} value={child} depth={depth + 1} />
        ))}
      </div>
    </details>
  );
}
