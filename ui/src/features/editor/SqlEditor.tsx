import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { MySQL, sql } from "@codemirror/lang-sql";
import { bracketMatching, indentOnInput } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  placeholder,
} from "@codemirror/view";
import { useEffect, useRef } from "react";
import { formatSql } from "./format";
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

export function SqlEditor({
  initialValue,
  schema,
  onChange,
  onRun,
  onReady,
}: {
  initialValue: string;
  /** Table name → column names, for completion. A table whose columns are not loaded maps to []. */
  schema: Record<string, string[]>;
  onChange: (value: string) => void;
  onRun: (all: boolean) => void;
  onReady: (view: EditorView) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
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
          autocompletion({ icons: false }),
          language.current.of(sql({ dialect: MySQL, upperCaseKeywords: true, schema })),
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

  useEffect(() => {
    view.current?.dispatch({
      effects: language.current.reconfigure(
        sql({ dialect: MySQL, upperCaseKeywords: true, schema }),
      ),
    });
  }, [schema]);

  return <div ref={host} className="h-full min-h-0 overflow-hidden" />;
}
