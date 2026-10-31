/**
 * Typed bridge to the Rust engine (src-tauri/src/main.rs).
 *
 * - `send(command)` queues a Command; results arrive as Events.
 * - `subscribe(handler)` receives every Event.
 *
 * Command/Event types are generated from Rust (`just bindings`); never edit ui/src/bindings.
 */
import { BundleType, getBundleType } from "@tauri-apps/api/app";
import { Channel, invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
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

/** Ask the user where to save an export, in the system's own dialog. The dialog only picks a
 * path; the engine writes the file. `null` when the user cancels. */
export async function chooseSavePath(
  fileName: string,
  extension: string,
  kind: string,
): Promise<string | null> {
  return save({
    title: "Export",
    defaultPath: fileName,
    filters: [{ name: kind, extensions: [extension] }],
  });
}

/** Ask the user which delimited file to import, in the system's own dialog. The dialog only
 * picks a path; the engine reads the file. `null` when the user cancels. */
export async function chooseImportPath(): Promise<string | null> {
  const path = await open({
    title: "Import",
    multiple: false,
    directory: false,
    filters: [{ name: "CSV or TSV", extensions: ["csv", "tsv", "txt"] }],
  });
  return typeof path === "string" ? path : null;
}

/** Open a release page in the default browser. The capability allows no other URL. */
export async function openReleasePage(url: string): Promise<void> {
  await openUrl(url);
}

/** Only the macOS app and the AppImage can replace themselves. A .deb or a Flatpak is updated by
 * its package manager, or by hand from the release page. */
export async function canSelfUpdate(): Promise<boolean> {
  const type = await getBundleType();
  return type === BundleType.App || type === BundleType.AppImage;
}

/** A newer release from the release's `latest.json`, or null. */
export async function findSelfUpdate(): Promise<Update | null> {
  return check();
}

/** Download `update`, check its signature and replace the installed app. `onProgress` gets the
 * share done (0 to 1), or null while the size is unknown. */
export async function installUpdate(
  update: Update,
  onProgress: (share: number | null) => void,
): Promise<void> {
  let total: number | null = null;
  let received = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") total = event.data.contentLength ?? null;
    if (event.event === "Progress") received += event.data.chunkLength;
    onProgress(total ? Math.min(received / total, 1) : null);
  });
}

/** Quit and start the app again, which runs the version just installed. */
export async function restartApp(): Promise<void> {
  await invoke("restart_app");
}
