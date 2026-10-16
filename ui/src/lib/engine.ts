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
