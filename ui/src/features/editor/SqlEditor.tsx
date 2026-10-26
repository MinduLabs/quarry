import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { MySQL } from "@codemirror/lang-sql";
import { bracketMatching, indentOnInput, LanguageSupport } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { EditorState, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  hoverTooltip,
  keymap,
  lineNumbers,
  placeholder,
  tooltips,
} from "@codemirror/view";
import { useEffect, useRef } from "react";
import { type SqlSchema, sqlCompletion } from "./completion";
import { problemAt, sqlDiagnostics } from "./diagnostics";
import { formatSql } from "./format";
import { runErrorAt, runMarks } from "./runMarks";
import { snippetSource } from "./snippets";
import { editorHighlight, editorTheme } from "./theme";

const flashEffect = StateEffect.define<{ from: number; to: number } | null>();

/** Briefly highlights the statement that is about to run. */
const flashField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(flashEffect)) {
        return effect.value && effect.value.to > effect.value.from
          ? Decoration.set([
              Decoration.mark({ class: "cm-run-flash" }).range(effect.value.from, effect.value.to),
            ])
          : Decoration.none;
      }
    }
    return value.map(tr.changes);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export function flashRange(view: EditorView, from: number, to: number) {
  view.dispatch({ effects: flashEffect.of({ from, to }) });
  setTimeout(() => view.dispatch({ effects: flashEffect.of(null) }), 450);
}

/** The message of an error mark under the pointer: the last run's error first. */
const problemTooltip = hoverTooltip((view, pos) => {
  const problem = runErrorAt(view.state, pos) ?? problemAt(view.state, pos);
  if (!problem) return null;
  return {
    pos: problem.from,
    end: problem.to,
    above: true,
    create: () => {
      const dom = document.createElement("div");
      dom.className = "cm-problem-tooltip";
      dom.textContent = problem.message;
      return { dom };
    },
  };
});

/** MySQL highlighting from lang-sql. Its completion (a flat keyword list) is left out:
 * `completion.ts` knows what fits where. */
const mysql = new LanguageSupport(MySQL.language);

export function SqlEditor({
  initialValue,
  schema,
  database,
  onChange,
  onRun,
  onReady,
}: {
  initialValue: string;
  /** Database → table → column names, for completion. A table whose columns are not loaded
   * maps to []. */
  schema: SqlSchema;
  /** The tab's database: its tables complete without a prefix. */
  database: string | null;
  onChange: (value: string) => void;
  onRun: (all: boolean) => void;
  onReady: (view: EditorView) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  // The completion source reads the current schema without being rebuilt.
  const completion = useRef({ schema, database });
  completion.current = { schema, database };
  // Keep callbacks current without recreating the editor.
  const handlers = useRef({ onChange, onRun });
  handlers.current = { onChange, onRun };

  // biome-ignore lint/correctness/useExhaustiveDependencies: the editor is created once per mount
  useEffect(() => {
    if (!host.current) return;
    const runKeys = keymap.of([
      {
        key: "Mod-Enter",
        run: () => {
          handlers.current.onRun(false);
          return true;
        },
      },
      {
        key: "Shift-Mod-Enter",
        run: () => {
          handlers.current.onRun(true);
          return true;
        },
      },
      { key: "Shift-Mod-f", run: formatSql },
    ]);
    view.current = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: initialValue,
        extensions: [
          runKeys,
          lineNumbers(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          history(),
          indentOnInput(),
          bracketMatching(),
          closeBrackets(),
          highlightSelectionMatches(),
          // Rust answers in microseconds, so the wait after a keystroke can be half of
          // CodeMirror's default 100 ms.
          autocompletion({ icons: false, activateOnTypingDelay: 50 }),
          mysql,
          MySQL.language.data.of({ autocomplete: snippetSource }),
          MySQL.language.data.of({
            autocomplete: sqlCompletion(() => completion.current),
          }),
          keymap.of([
            ...closeBracketsKeymap,
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
            ...completionKeymap,
            indentWithTab,
          ]),
          placeholder("Write SQL…  Run the statement under the cursor with Ctrl/⌘ Enter"),
          editorTheme,
          editorHighlight,
          flashField,
          runMarks,
          sqlDiagnostics,
          problemTooltip,
          // In the page body: the editor clips its own box, which hid a tooltip shown above the
          // first line (or a completion list below the last).
          tooltips({ parent: document.body }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) handlers.current.onChange(update.state.doc.toString());
          }),
        ],
      }),
    });
    onReady(view.current);
    view.current.focus();
    return () => view.current?.destroy();
  }, []);

  return <div ref={host} className="h-full min-h-0 overflow-hidden" />;
}
