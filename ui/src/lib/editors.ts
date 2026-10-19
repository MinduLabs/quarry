import type { EditorView } from "@codemirror/view";

/** The SQL editor of each open tab, so other features (history) can write into it. */
const views = new Map<number, EditorView>();

export function registerEditor(tab: number, view: EditorView | null): void {
  if (view) views.set(tab, view);
  else views.delete(tab);
}

/**
 * Put `text` at the cursor of the tab's editor (replacing a selection), on its own line, and focus
 * it. Returns false when the tab has no editor.
 */
export function insertIntoEditor(tab: number, text: string): boolean {
  const view = views.get(tab);
  if (!view) return false;
  const { from, to } = view.state.selection.main;
  const before = from > 0 ? view.state.sliceDoc(from - 1, from) : "\n";
  const after = view.state.sliceDoc(to, to + 1);
  const insert = `${before === "\n" ? "" : "\n"}${text}${after && after !== "\n" ? "\n" : ""}`;
  view.dispatch({
    changes: { from, to, insert },
    selection: { anchor: from + insert.length },
    scrollIntoView: true,
  });
  view.focus();
  return true;
}
