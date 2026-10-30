import { Button } from "@/components/ui/Button";
import { Spinner } from "@/components/ui/Spinner";
import { useApp } from "@/store/app";

/** The import that is inserting rows: rows so far and a way to stop it. A card beside the work,
 * not a dialog, so the app stays usable while a big file goes in. */
export function ImportProgress() {
  const run = useApp((s) => s.importRun);
  const cancel = useApp((s) => s.cancelImport);
  if (!run) return null;
  return (
    <div
      role="status"
      aria-live="polite"
      className="animate-toast fixed top-11 right-4 z-40 flex w-80 items-center gap-3 rounded-lg bg-elevated px-3 py-2.5 shadow-popover"
    >
      <Spinner size={14} />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-base text-fg">Importing into {run.name}</span>
        <span className="text-muted text-sm tabular-nums">
          {run.rows.toLocaleString()} rows · kept only when all rows are in
        </span>
      </div>
      <Button size="sm" variant="ghost" onClick={cancel}>
        Cancel
      </Button>
    </div>
  );
}
