import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { useApp } from "@/store/app";

const plural = (n: number, noun: string) => `${n} ${noun}${n === 1 ? "" : "s"}`;

/** Shown only when quitting would lose work: a running query or SQL that was never saved. */
export function QuitDialog() {
  const quit = useApp((s) => s.quit);
  const dismiss = useApp((s) => s.dismissQuit);

  const lines: string[] = [];
  if (quit?.running) lines.push(`${plural(quit.running, "query")} still running will be stopped.`);
  if (quit?.unsaved) lines.push(`${plural(quit.unsaved, "tab")} with SQL you haven't saved.`);

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
          <Button variant="danger" onClick={() => void getCurrentWindow().destroy()}>
            Quit
          </Button>
        </>
      }
    />
  );
}
