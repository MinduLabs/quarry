import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { useApp } from "@/store/app";

/** Safe mode: ask before running risky SQL on staging/production. */
export function ConfirmRunDialog() {
  const confirm = useApp((s) => s.confirm);
  const config = useApp((s) => s.connections.find((c) => c.id === confirm?.conn));
  const answer = useApp((s) => s.answerConfirm);
  const production = config?.environment === "production";

  return (
    <ConfirmDialog
      open={Boolean(confirm)}
      title={production ? "Run on production?" : "Run this statement?"}
      description={confirm?.message}
      confirmLabel="Run"
      width={460}
      onConfirm={() => answer(true)}
      onCancel={() => answer(false)}
    >
      <p className="mb-2 text-sm text-muted">Connection: {config?.name}</p>
      <pre className="max-h-40 overflow-auto rounded-md bg-surface p-3 font-mono text-sm text-fg whitespace-pre-wrap">
        {confirm?.sql}
      </pre>
    </ConfirmDialog>
  );
}
