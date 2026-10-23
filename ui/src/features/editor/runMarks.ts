/**
 * Marks of the last run in the editor: the statement running now, and the spot of the error the
 * run stopped at. Positions follow later edits; an edit inside the error clears it.
 */
import { type EditorState, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import type { ErrorLocation } from "@/bindings/ErrorLocation";

export interface Span {
  from: number;
  to: number;
}

export interface Problem extends Span {
  message: string;
}

interface RunMarks {
  /** The statements of the last run started from the editor, in order. */
  statements: Span[];
  active: number | null;
  error: Problem | null;
}

/** A new run from the editor: its statements, in document positions. */
export const setRunStatements = StateEffect.define<Span[]>();
/** The statement (index into the run's statements) that runs now, or null when none does. */
export const setActiveStatement = StateEffect.define<number | null>();
/** The run failed in `statement` (null: in the only one); `location` is the server's. */
export const setRunError = StateEffect.define<{
  statement: number | null;
  location: ErrorLocation | null;
  message: string;
} | null>();

const activeMark = Decoration.mark({ class: "cm-run-active" });
const errorMark = Decoration.mark({ class: "cm-sql-problem" });

export const runMarks = StateField.define<RunMarks>({
  create: () => ({ statements: [], active: null, error: null }),
  update(marks, tr) {
    let { statements, active, error } = marks;
    if (tr.docChanged) {
      const map = (span: Span): Span => ({
        from: tr.changes.mapPos(span.from, 1),
        to: tr.changes.mapPos(span.to, -1),
      });
      statements = statements.map(map);
      if (error) {
        let touched = false;
        tr.changes.iterChangedRanges((from, to) => {
          if (from <= (error as Problem).to && to >= (error as Problem).from) touched = true;
        });
        error = touched ? null : { ...error, ...map(error) };
      }
    }
    for (const effect of tr.effects) {
      if (effect.is(setRunStatements)) {
        statements = effect.value;
        active = null;
        error = null;
      } else if (effect.is(setActiveStatement)) {
        active = effect.value;
      } else if (effect.is(setRunError)) {
        const failed = effect.value;
        const statement = failed && statements[failed.statement ?? 0];
        error =
          failed && statement && (failed.statement !== null || statements.length === 1)
            ? { ...errorSpan(tr.state, statement, failed.location), message: failed.message }
            : null;
      }
    }
    return { statements, active, error };
  },
  provide: (field) =>
    EditorView.decorations.from(field, (marks): DecorationSet => {
      const ranges = [];
      const running = marks.active === null ? null : marks.statements[marks.active];
      if (running && running.to > running.from) {
        ranges.push(activeMark.range(running.from, running.to));
      }
      if (marks.error && marks.error.to > marks.error.from) {
        ranges.push(errorMark.range(marks.error.from, marks.error.to));
      }
      return Decoration.set(ranges, true);
    }),
});

/** The error of the last run, when the pointer is over it. */
export function runErrorAt(state: EditorState, pos: number): Problem | null {
  const error = state.field(runMarks, false)?.error;
  return error && pos >= error.from && pos <= error.to ? error : null;
}

/**
 * Where the server's error falls in `statement`. The server counts lines within the statement and
 * quotes the text from the error on, so the spot is the last place on that line where the quote
 * starts. Without a location, the whole statement is the error.
 */
function errorSpan(state: EditorState, statement: Span, location: ErrorLocation | null): Span {
  if (!location) return statement;
  const doc = state.doc;
  if (location.near === "") {
    return { from: Math.max(statement.from, statement.to - 1), to: statement.to };
  }
  const first = doc.lineAt(statement.from).number;
  const last = doc.lineAt(statement.to).number;
  const line = doc.line(Math.min(first + location.line - 1, last));
  const start = Math.max(line.from, statement.from);
  const end = Math.min(line.to, statement.to);
  const text = doc.sliceString(start, end);
  // The quote runs to the end of the line unless the server cut it short. A filled
  // `{{variable}}` makes the editor text differ from what ran; a short probe still finds the
  // spot when the change is further on.
  const rest = location.near.split("\n")[0] ?? "";
  const probe = rest.slice(0, 40);
  const at = !rest
    ? -1
    : text.trimEnd().endsWith(rest.trimEnd())
      ? text.trimEnd().length - rest.trimEnd().length
      : text.lastIndexOf(probe);
  if (at < 0) return { from: start, to: end };
  const from = start + at;
  const word = /^\S+/.exec(location.near)?.[0].length ?? 1;
  return { from, to: Math.min(from + word, statement.to) };
}
