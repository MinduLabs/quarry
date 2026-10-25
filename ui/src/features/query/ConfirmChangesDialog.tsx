import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { useApp } from "@/store/app";

/** Safe mode for grid edits: production asks before any change is written. */
export function ConfirmChangesDialog() {
  const pending = useApp((s) => s.changesConfirm);
  const name = useApp((s) => {
    const tab = s.tabs.find((t) => t.id === pending?.tab);
    return s.connections.find((c) => c.id === tab?.connId)?.name;
  });
  const answer = useApp((s) => s.answerChangesConfirm);

  return (
    <ConfirmDialog
      open={Boolean(pending)}
      title="Save changes on production?"
      description={pending?.message}
      confirmLabel="Save"
      width={460}
      onConfirm={() => answer(true)}
      onCancel={() => answer(false)}
    >
      <p className="text-sm text-muted">Connection: {name}</p>
    </ConfirmDialog>
  );
}
