import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { useApp } from "@/store/app";

/** Deleting rows from the grid is staged until Save, but it is still asked first. */
export function ConfirmDeleteRowsDialog() {
  const pending = useApp((s) => s.deleteConfirm);
  const answer = useApp((s) => s.answerDeleteConfirm);
  const tab = useApp((s) => s.tabs.find((t) => t.id === pending?.tab));
  const connection = useApp((s) => s.connections.find((c) => c.id === tab?.connId));
  const loaded = tab?.sets[tab.activeSet]?.rows.length ?? 0;
  const count = pending?.rows.filter((r) => r < loaded).length ?? 0;
  const noun = `${count} ${count === 1 ? "row" : "rows"}`;

  return (
    <ConfirmDialog
      open={pending !== null}
      title={`Delete ${noun}?`}
      description="They are marked for deletion and removed from the table when you save. Undo or Discard brings them back."
      confirmLabel={`Delete ${noun}`}
      onConfirm={() => answer(true)}
      onCancel={() => answer(false)}
    >
      <p className="text-sm text-muted">
        {connection?.name} · {tab?.browse?.table.name ?? "query result"}
      </p>
    </ConfirmDialog>
  );
}
