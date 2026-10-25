import type { ReactNode } from "react";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

/**
 * A question with two answers, before something the user may not expect or cannot take back.
 * Cancel has the focus, so Enter and the first Space never confirm by accident. `children` holds
 * what the answer depends on (the SQL, the connection); the title and description say what
 * happens.
 */
export function ConfirmDialog({
  open,
  title,
  description,
  children,
  confirmLabel,
  cancelLabel = "Cancel",
  tone = "danger",
  width = 440,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  description?: string;
  children?: ReactNode;
  /** What the confirming button does, in the user's words: "Delete 3 rows", not "OK". */
  confirmLabel: string;
  cancelLabel?: string;
  /** `danger` for what loses or changes data; `primary` for a question that does not. */
  tone?: "danger" | "primary";
  width?: number;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog
      open={open}
      onClose={onCancel}
      title={title}
      description={description}
      width={width}
      footer={
        <>
          <Button autoFocus onClick={onCancel}>
            {cancelLabel}
          </Button>
          <Button variant={tone} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Dialog>
  );
}
