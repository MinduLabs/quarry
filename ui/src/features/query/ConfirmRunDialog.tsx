import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { useApp } from "@/store/app";

/** Safe mode: ask before running risky SQL on staging/production. */
export function ConfirmRunDialog() {
  const confirm = useApp((s) => s.confirm);
  const config = useApp((s) => s.connections.find((c) => c.id === confirm?.conn));
  const answer = useApp((s) => s.answerConfirm);
  const production = config?.environment === "production";

  return (
    <Dialog
      open={Boolean(confirm)}
      onClose={() => answer(false)}
      title={production ? "Run on production?" : "Run this statement?"}
      description={confirm?.message}
      width={460}
      footer={
        <>
          <Button autoFocus onClick={() => answer(false)}>
            Cancel
          </Button>
          <Button variant="danger" onClick={() => answer(true)}>
            Run
          </Button>
        </>
      }
    >
      <p className="mb-2 text-sm text-muted">Connection: {config?.name}</p>
      <pre className="max-h-40 overflow-auto rounded-md bg-surface p-3 font-mono text-sm text-fg whitespace-pre-wrap">
        {confirm?.sql}
      </pre>
    </Dialog>
  );
}
