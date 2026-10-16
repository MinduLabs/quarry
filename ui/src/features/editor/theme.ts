/** CodeMirror theme built from the design tokens (docs/STYLE-GUIDE.md › SQL editor). */
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

export const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    backgroundColor: "var(--app)",
    color: "var(--fg)",
    fontSize: "var(--text-base)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.6" },
  ".cm-content": { padding: "8px 0", caretColor: "var(--accent)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
  ".cm-gutters": { backgroundColor: "var(--app)", color: "var(--faint)", border: "none" },
  ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 16px", minWidth: "40px" },
  ".cm-activeLine": { backgroundColor: "var(--hover)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--muted)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection":
    { backgroundColor: "var(--accent-soft) !important" },
  ".cm-matchingBracket": { backgroundColor: "var(--accent-soft)", outline: "none" },
  ".cm-run-flash": {
    backgroundColor: "var(--accent-soft)",
    transition: "background-color 400ms ease-out",
  },
  ".cm-tooltip": {
    border: "none",
    backgroundColor: "var(--elevated)",
    borderRadius: "var(--radius-lg)",
    boxShadow: "var(--shadow-popover)",
    overflow: "hidden",
  },
  ".cm-tooltip-autocomplete > ul": {
    fontFamily: "var(--font-mono)",
    maxHeight: "240px",
    padding: "4px",
  },
  ".cm-tooltip-autocomplete > ul > li": {
    borderRadius: "var(--radius-sm)",
    padding: "2px 8px !important",
  },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--accent)",
    color: "var(--accent-fg)",
  },
  ".cm-completionDetail": { color: "var(--muted)", fontStyle: "normal" },
  ".cm-panels": {
    backgroundColor: "var(--surface)",
    color: "var(--fg)",
    borderColor: "var(--line)",
  },
  ".cm-searchMatch": { backgroundColor: "var(--cell-modified)" },
});

export const editorHighlight = syntaxHighlighting(
  HighlightStyle.define([
    { tag: [t.keyword, t.operatorKeyword, t.modifier], color: "var(--syntax-keyword)" },
    { tag: [t.string, t.special(t.string)], color: "var(--syntax-string)" },
    { tag: [t.number], color: "var(--syntax-number)" },
    { tag: [t.bool, t.null, t.atom], color: "var(--syntax-literal)" },
    {
      tag: [t.comment, t.lineComment, t.blockComment],
      color: "var(--syntax-comment)",
      fontStyle: "italic",
    },
    { tag: [t.function(t.variableName), t.standard(t.name)], color: "var(--syntax-function)" },
    { tag: [t.typeName, t.className], color: "var(--syntax-type)" },
    { tag: [t.special(t.name), t.quote], color: "var(--fg)" },
    { tag: [t.operator, t.punctuation, t.bracket], color: "var(--muted)" },
  ]),
);
