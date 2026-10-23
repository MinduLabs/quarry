/**
 * The UI side of `sqlParser.worker.ts`, which checks statements for syntax errors. The worker
 * starts on first use, so the parser never weighs on startup, and answers one statement at a time.
 */

/** A problem in a statement, in UTF-16 offsets from the statement's start. */
export interface RelativeProblem {
  from: number;
  to: number;
  message: string;
}

export type ParserRequest = { id: number; kind: "validate"; statement: string };

export type ParserReply =
  | { id: number; kind: "validate"; problems: RelativeProblem[] }
  | { id: number; kind: "error"; message: string };

let worker: Worker | null = null;
let failed = false;
let nextId = 1;
const waiting = new Map<number, (reply: ParserReply | null) => void>();

function start(): Worker | null {
  if (worker || failed) return worker;
  try {
    worker = new Worker(new URL("./sqlParser.worker.ts", import.meta.url), { type: "module" });
  } catch (error) {
    failed = true;
    console.error("SQL parser worker did not start", error);
    return null;
  }
  worker.onmessage = (event: MessageEvent<ParserReply>) => {
    const reply = event.data;
    if (reply.kind === "error") console.error("SQL parser", reply.message);
    waiting.get(reply.id)?.(reply.kind === "error" ? null : reply);
    waiting.delete(reply.id);
  };
  worker.onerror = (event) => {
    console.error("SQL parser worker failed", event.message);
    failed = true;
    worker?.terminate();
    worker = null;
    for (const resolve of waiting.values()) resolve(null);
    waiting.clear();
  };
  return worker;
}

/** Problems by statement text, so an unchanged statement costs no round trip to the worker. */
const validated = new Map<string, RelativeProblem[]>();
const MAX_VALIDATED = 2_000;

export async function validateStatement(statement: string): Promise<RelativeProblem[] | null> {
  const cached = validated.get(statement);
  if (cached) return cached;
  const target = start();
  if (!target) return null;
  const id = nextId++;
  const reply = await new Promise<ParserReply | null>((resolve) => {
    waiting.set(id, resolve);
    target.postMessage({ id, kind: "validate", statement } satisfies ParserRequest);
  });
  if (reply?.kind !== "validate") return null;
  if (validated.size >= MAX_VALIDATED) validated.clear();
  validated.set(statement, reply.problems);
  return reply.problems;
}
