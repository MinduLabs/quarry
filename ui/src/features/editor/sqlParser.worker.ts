/// <reference lib="webworker" />
/**
 * MySQL grammar (ANTLR4) off the UI thread: the syntax errors of one statement. The parser is
 * several megabytes, so it lives here and loads when the first statement is checked.
 */
import { MySQL } from "dt-sql-parser";
import type { ParserReply, ParserRequest, RelativeProblem } from "./sqlParser";

const parser = new MySQL();

self.onmessage = (event: MessageEvent<ParserRequest>) => {
  const request = event.data;
  let reply: ParserReply;
  try {
    reply = { id: request.id, kind: "validate", problems: validate(request.statement) };
  } catch (error) {
    reply = { id: request.id, kind: "error", message: String(error) };
  }
  self.postMessage(reply);
};

function validate(statement: string): RelativeProblem[] {
  const starts = lineStarts(statement);
  const at = (line: number, column: number) =>
    Math.min(statement.length, (starts[line - 1] ?? statement.length) + column - 1);
  return parser.validate(statement).map((error) => {
    const from = at(error.startLine, error.startColumn);
    const to = Math.max(from + 1, at(error.endLine, error.endColumn));
    return { from, to: Math.min(to, Math.max(statement.length, from + 1)), message: error.message };
  });
}

function lineStarts(text: string): number[] {
  const starts = [0];
  for (let i = text.indexOf("\n"); i >= 0; i = text.indexOf("\n", i + 1)) starts.push(i + 1);
  return starts;
}
