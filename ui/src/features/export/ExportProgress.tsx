import { Button } from "@/components/ui/Button";
import { Spinner } from "@/components/ui/Spinner";
import { formatBytes } from "@/lib/bytes";
import { useApp } from "@/store/app";

/** The export that is writing a file: rows so far, size, and a way to stop it. It is a card
 * beside the work, not a dialog, so the app stays usable while a big table is written. */
export function ExportProgress() {
  const run = useApp((s) => s.exportRun);
  const cancel = useApp((s) => s.cancelExport);
  if (!run) return null;
  return (
    <div
      role="status"
      aria-live="polite"
      className="animate-toast fixed top-11 right-4 z-40 flex w-80 items-center gap-3 rounded-lg bg-elevated px-3 py-2.5 shadow-popover"
    >
      <Spinner size={14} />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-base text-fg">Exporting {run.name}</span>
        <span className="text-muted text-sm tabular-nums">
          {run.rows.toLocaleString()} rows{run.bytes > 0 && ` · ${formatBytes(run.bytes)}`}
        </span>
      </div>
      <Button size="sm" variant="ghost" onClick={cancel}>
        Cancel
      </Button>
    </div>
  );
}
