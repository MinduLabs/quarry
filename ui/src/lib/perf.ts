/**
 * Client timings for the development performance log. Off unless the engine says its log is on
 * (`Pong.perf`, set by `QUARRY_PERF`); then renders are batched and sent once a second as
 * `PerfMarks`, so they land in the same `perf.jsonl` as the engine's own timings.
 *
 * React only reports renders from development builds (`just dev`); a release build sends nothing.
 */
import type { ProfilerOnRenderCallback } from "react";
import type { PerfMark } from "@/bindings/PerfMark";
import { send } from "@/lib/engine";

/** Renders shorter than this are not worth a line. */
const MIN_MS = 1;
const FLUSH_MS = 1000;

let enabled = false;
let pending: PerfMark[] = [];
let timer: ReturnType<typeof setInterval> | undefined;

export function setPerfEnabled(on: boolean): void {
  enabled = on;
  clearInterval(timer);
  timer = undefined;
  pending = [];
  if (on) timer = setInterval(flush, FLUSH_MS);
}

/** Record a timing; `detail` is short context, never SQL or values. */
export function perfMark(name: string, ms: number, detail: string | null = null): void {
  if (enabled) pending.push({ name, ms, detail });
}

/** Pass to `<Profiler onRender>`: logs `render.<id>` with the commit phase. */
export const onRender: ProfilerOnRenderCallback = (id, phase, actualDuration) => {
  if (actualDuration >= MIN_MS) perfMark(`render.${id}`, actualDuration, phase);
};

function flush(): void {
  if (pending.length === 0) return;
  send({ type: "PerfMarks", marks: pending });
  pending = [];
}
