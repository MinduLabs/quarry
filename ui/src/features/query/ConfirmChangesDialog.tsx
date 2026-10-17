import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
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
    <Dialog
      open={Boolean(pending)}
      onClose={() => answer(false)}
      title="Save changes on production?"
      description={pending?.message}
      width={460}
      footer={
        <>
          <Button autoFocus onClick={() => answer(false)}>
            Cancel
          </Button>
          <Button variant="danger" onClick={() => answer(true)}>
            Save
          </Button>
        </>
      }
    >
      <p className="text-sm text-muted">Connection: {name}</p>
    </Dialog>
  );
}
