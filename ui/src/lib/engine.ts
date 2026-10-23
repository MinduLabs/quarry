/**
 * Typed bridge to the Rust engine (src-tauri/src/main.rs).
 *
 * - `send(command)` queues a Command; results arrive as Events.
 * - `subscribe(handler)` receives every Event.
 *
 * Command/Event types are generated from Rust (`just bindings`); never edit ui/src/bindings.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Command } from "@/bindings/Command";
import type { CompletionContext } from "@/bindings/CompletionContext";
import type { Event } from "@/bindings/Event";

export function send(command: Command): void {
  invoke("engine_send", { command }).catch((err: unknown) => {
    console.error("engine_send failed", err);
  });
}

export async function subscribe(handler: (event: Event) => void): Promise<void> {
  const channel = new Channel<Event>();
  channel.onmessage = handler;
  await invoke("engine_subscribe", { onEvent: channel });
}

/** UTF-16 range of the SQL statement under the cursor, or null. */
export async function statementAt(text: string, offset: number): Promise<[number, number] | null> {
  return invoke<[number, number] | null>("statement_at", { text, offset });
}

/** UTF-16 ranges of the statements a `Run` of `text` sends, in the order the engine runs them. */
export async function scriptStatements(text: string): Promise<[number, number][]> {
  return invoke<[number, number][]>("script_statements", { text });
}

/** What fits at UTF-16 `offset` of `text` in a SQL editor, read in Rust; `null` where nothing
 * completes (a string, a comment). */
export async function completionContext(
  text: string,
  offset: number,
): Promise<CompletionContext | null> {
  return invoke<CompletionContext | null>("completion_context", { text, offset });
}
