import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
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
    <Dialog
      open={Boolean(quit)}
      onClose={dismiss}
      title="Quit Quarry?"
      description={lines.join(" ")}
      width={420}
      footer={
        <>
          <Button autoFocus onClick={dismiss}>
            Cancel
          </Button>
          <Button
            variant="danger"
            onClick={() => void saveWorkspace().then(() => getCurrentWindow().destroy())}
          >
            Quit
          </Button>
        </>
      }
    />
  );
}
