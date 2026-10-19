import { useId, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { useApp } from "@/store/app";

/** Name the tab's SQL and keep it in Favorites. */
export function SaveQueryDialog({
  tabId,
  open,
  onClose,
}: {
  tabId: number;
  open: boolean;
  onClose: () => void;
}) {
  const save = useApp((s) => s.saveQueryFavorite);
  const [name, setName] = useState("");
  const inputId = useId();
  const submit = () => {
    if (!name.trim()) return;
    save(tabId, name);
    setName("");
    onClose();
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Save query"
      description="It appears under Favorites in the sidebar."
      width={400}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!name.trim()} onClick={submit}>
            Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-1">
        <label htmlFor={inputId} className="text-sm text-muted">
          Name
        </label>
        <Input
          id={inputId}
          autoFocus
          value={name}
          placeholder="Query name"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
          }}
        />
      </div>
    </Dialog>
  );
}
