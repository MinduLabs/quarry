/**
 * Syntax errors while typing, from the MySQL grammar in `sqlParser.worker.ts`. Checks run a
 * moment after typing stops and never flag
 * what more typing would fix, so the statement being written is not marked for ending early.
 * Only the statements on screen are checked, one at a time, and the worker caches each, so a
 * long script costs no more than what is visible. The database still has the last word: a run's
 * own error is marked by `runMarks`.
 */
import { type EditorState, StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { scriptStatements } from "@/lib/engine";
import type { Problem } from "./runMarks";
import { validateStatement } from "./sqlParser";

/** Long enough not to flicker while typing. */
const CHECK_DELAY_MS = 500;
/** Statements this far around the visible text are checked too, so a short scroll finds them. */
const MARGIN = 2_000;

const setProblems = StateEffect.define<Problem[]>();
const problemMark = Decoration.mark({ class: "cm-sql-problem" });

const problemField = StateField.define<Problem[]>({
  create: () => [],
  update(problems, tr) {
    for (const effect of tr.effects) if (effect.is(setProblems)) return effect.value;
    if (!tr.docChanged) return problems;
    // An edit inside a problem may fix it; it stays hidden until the next check says.
    const kept: Problem[] = [];
    for (const p of problems) {
      let isTouched = false;
      tr.changes.iterChangedRanges((from, to) => {
        if (from <= p.to && to >= p.from) isTouched = true;
      });
      if (!isTouched) {
        kept.push({ ...p, from: tr.changes.mapPos(p.from, 1), to: tr.changes.mapPos(p.to, -1) });
      }
    }
    return kept;
  },
  provide: (field) =>
    EditorView.decorations.from(field, (problems) =>
      Decoration.set(
        problems.filter((p) => p.to > p.from).map((p) => problemMark.range(p.from, p.to)),
        true,
      ),
    ),
});

const checker = ViewPlugin.fromClass(
  class {
    timer: ReturnType<typeof setTimeout> | null = null;
    /** Bumped by every edit; a check whose text changed meanwhile is dropped. */
    generation = 0;
    view: EditorView;
    constructor(view: EditorView) {
      this.view = view;
      this.schedule();
    }
    update(update: ViewUpdate) {
      if (update.docChanged) this.generation += 1;
      if (update.docChanged || update.viewportChanged) this.schedule();
    }
    schedule() {
      if (this.timer) clearTimeout(this.timer);
      this.timer = setTimeout(() => {
        this.timer = null;
        void this.check();
      }, CHECK_DELAY_MS);
    }
    async check() {
      const generation = this.generation;
      const state = this.view.state;
      const window = checkedWindow(state, this.view.viewport);
      const text = state.sliceDoc(window.from, window.to);
      if (!text.trim()) return this.view.dispatch({ effects: setProblems.of([]) });
      const ranges = await scriptStatements(text);
      const problems: Problem[] = [];
      for (const [rangeFrom, rangeTo] of ranges) {
        // A statement cut by the window's edge is not what the user wrote.
        if (
          (rangeFrom === 0 && window.from > 0) ||
          (rangeTo >= text.length && window.to < state.doc.length)
        ) {
          continue;
        }
        const found = await validateStatement(text.slice(rangeFrom, rangeTo));
        if (found === null || generation !== this.generation) return;
        const start = window.from + rangeFrom;
        const end = window.from + rangeTo;
        const isEditing = isBeingWritten(state, start, end);
        for (const p of found) {
          // Running off the end of the statement under the cursor is what typing does.
          if (isEditing && p.from >= end - start) continue;
          problems.push({ from: start + p.from, to: start + p.to, message: p.message });
        }
      }
      if (generation === this.generation) {
        this.view.dispatch({ effects: setProblems.of(problems) });
      }
    }
    destroy() {
      if (this.timer) clearTimeout(this.timer);
      this.generation += 1;
    }
  },
);

/**
 * The text checked: the viewport and a margin, from line starts, so a long script costs what is
 * on screen. From the top when a `DELIMITER` line above changes how statements end there.
 */
function checkedWindow(state: EditorState, viewport: { from: number; to: number }) {
  const doc = state.doc;
  const from = doc.lineAt(Math.max(0, viewport.from - MARGIN)).from;
  const to = doc.lineAt(Math.min(doc.length, viewport.to + MARGIN)).to;
  for (const line of doc.iterLines(1, doc.lineAt(from).number)) {
    if (/^\s*delimiter\s/i.test(line)) return { from: 0, to };
  }
  return { from, to };
}

/** The cursor is in the statement or after it with no `;` between. */
function isBeingWritten(state: EditorState, from: number, to: number): boolean {
  const head = state.selection.main.head;
  if (head < from) return false;
  return !state.sliceDoc(to, head).includes(";");
}

/** The problem under `pos`, if any. */
export function problemAt(state: EditorState, pos: number): Problem | null {
  const problems = state.field(problemField, false) ?? [];
  return problems.find((p) => pos >= p.from && pos <= p.to) ?? null;
}

export const sqlDiagnostics = [problemField, checker];
