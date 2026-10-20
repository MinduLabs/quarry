import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { MySQL, sql } from "@codemirror/lang-sql";
import { bracketMatching, indentOnInput, type LanguageSupport } from "@codemirror/language";
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
import { snippetSource } from "./snippets";
import { editorHighlight, editorTheme } from "./theme";

/** Table name → column names, for completion. A table whose columns are not loaded maps to []. */
export type CompletionSchema = Record<string, string[]>;

/**
 * SQL language with completion for `schema`, built once per schema object and shared by every
 * editor using it: building it for thousands of tables is slow.
 */
// claude: lang-sql 6.10 builds its completion tree in O(tables²) (`CompletionLevel.child` scans
// the list for each new table), ~100 ms for 5000 tables in a dev build. A completion source of our
// own keyed by Map would remove it; until then the cache keeps it to once per database load.
const languages = new WeakMap<CompletionSchema, LanguageSupport>();
function sqlLanguage(schema: CompletionSchema): LanguageSupport {
  let language = languages.get(schema);
  if (!language) {
    language = sql({ dialect: MySQL, upperCaseKeywords: true, schema });
    languages.set(schema, language);
  }
  return language;
}

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
  schema: CompletionSchema;
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
          language.current.of(sqlLanguage(schema)),
          MySQL.language.data.of({ autocomplete: snippetSource }),
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
    view.current?.dispatch({ effects: language.current.reconfigure(sqlLanguage(schema)) });
  }, [schema]);

  return <div ref={host} className="h-full min-h-0 overflow-hidden" />;
}
