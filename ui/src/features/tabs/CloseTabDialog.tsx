import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { useApp } from "@/store/app";

/** Closing a tab ends its session, which rolls back a transaction left open. */
export function CloseTabDialog() {
  const ids = useApp((s) => s.closeConfirm);
  const tabs = useApp((s) => s.tabs);
  const open = tabs.filter((t) => ids?.includes(t.id) && t.txOpen);
  const answer = useApp((s) => s.answerCloseConfirm);
  const first = open[0];
  const description =
    open.length > 1
      ? `${open.length} of these tabs have a transaction open. Closing them rolls the transactions back. Commit first to keep the changes.`
      : `“${first?.title ?? ""}” has a transaction open. Closing the tab rolls it back. Commit first to keep the changes.`;

  return (
    <ConfirmDialog
      open={ids !== null}
      title={ids && ids.length > 1 ? "Close tabs?" : "Close tab?"}
      description={description}
      confirmLabel="Roll back and close"
      onConfirm={() => answer(true)}
      onCancel={() => answer(false)}
    />
  );
}
