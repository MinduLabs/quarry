import { getCurrentWindow } from "@tauri-apps/api/window";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { saveWorkspace, useApp } from "@/store/app";

const plural = (n: number, noun: string) => `${n} ${noun}${n === 1 ? "" : "s"}`;

/**
 * Shown only when quitting would stop a running query, drop grid edits or roll back a transaction.
 * Tabs and SQL are saved.
 */
export function QuitDialog() {
  const quit = useApp((s) => s.quit);
  const dismiss = useApp((s) => s.dismissQuit);

  const lines: string[] = [];
  if (quit?.running) lines.push(`${plural(quit.running, "query")} still running will be stopped.`);
  if (quit?.edits) lines.push(`${plural(quit.edits, "tab")} with data changes you haven't saved.`);
  if (quit?.transactions) {
    lines.push(`${plural(quit.transactions, "open transaction")} will be rolled back.`);
  }

  return (
    <ConfirmDialog
      open={Boolean(quit)}
      title="Quit Quarry?"
      description={lines.join(" ")}
      confirmLabel="Quit"
      width={420}
      onConfirm={() => void saveWorkspace().then(() => getCurrentWindow().destroy())}
      onCancel={dismiss}
    />
  );
}
