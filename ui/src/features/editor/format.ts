import type { EditorView } from "@codemirror/view";
import { formatDialect, mysql } from "sql-formatter";
import { VARIABLE_PATTERN } from "@/lib/variables";
import { useApp } from "@/store/app";

/**
 * Reformats the selection, or the whole editor when nothing is selected, as one undo step.
 * Always returns `true` so the shortcut never falls through to the webview.
 */
export function formatSql(view: EditorView): boolean {
  const { main } = view.state.selection;
  const from = main.empty ? 0 : main.from;
  const to = main.empty ? view.state.doc.length : main.to;
  const source = view.state.sliceDoc(from, to);
  if (!source.trim()) return true;

  let formatted: string;
  try {
    // Importing one dialect keeps the other dialects out of the bundle.
    formatted = formatDialect(source, {
      dialect: mysql,
      keywordCase: "upper",
      tabWidth: 2,
      // Without `custom`, `{{name}}` is an unknown token and the whole format fails. `?` and `:name`
      // are the other marks the query variables dialog asks for.
      paramTypes: { positional: true, named: [":"], custom: [{ regex: VARIABLE_PATTERN }] },
    });
  } catch (err) {
    const reason = err instanceof Error ? err.message : String(err);
    useApp
      .getState()
      .toast("error", `Could not format the SQL. Check the syntax near: ${firstLine(reason)}`);
    return true;
  }
  if (formatted === source) return true;

  view.dispatch({
    changes: { from, to, insert: formatted },
    selection: main.empty
      ? { anchor: Math.min(main.head, formatted.length) }
      : { anchor: from, head: from + formatted.length },
    scrollIntoView: true,
    userEvent: "input.format",
  });
  return true;
}

function firstLine(text: string): string {
  return text.split("\n")[0] ?? text;
}
