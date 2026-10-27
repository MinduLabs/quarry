import { useMemo, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { shownSchemas, useApp } from "@/store/app";

/** Every schema of a connection with a tick box; the tree lists only the ticked ones. */
export function SchemaPickerDialog() {
  const conn = useApp((s) => s.schemaPicker);
  const config = useApp((s) => s.connections.find((c) => c.id === conn));
  const databases = useApp((s) => (conn ? s.nodes[conn]?.databases : undefined));
  const saved = useApp((s) => (conn ? s.schemaFilter[conn] : undefined));
  const { setShownSchemas, setSchemaPicker } = useApp.getState();
  const [filter, setFilter] = useState("");

  const names = useMemo(() => (databases ?? []).map((db) => db.name), [databases]);
  const ticked = useMemo(
    () => shownSchemas(config?.database ?? null, databases ?? [], saved),
    [config, databases, saved],
  );
  const listed = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return needle ? names.filter((n) => n.toLowerCase().includes(needle)) : names;
  }, [names, filter]);

  const close = () => {
    setSchemaPicker(null);
    setFilter("");
  };
  const change = (changed: string[], on: boolean) => {
    if (!conn) return;
    const next = new Set(ticked);
    for (const name of changed) {
      if (on) next.add(name);
      else next.delete(name);
    }
    // In the server's order, so the saved list does not depend on the order of the clicks.
    setShownSchemas(
      conn,
      names.filter((n) => next.has(n)),
    );
  };

  return (
    <Dialog
      open={conn !== null}
      onClose={close}
      title={config ? `Schemas · ${config.name}` : "Schemas"}
      width={420}
      height={520}
      flush
      footer={
        <Button variant="primary" onClick={close}>
          Done
        </Button>
      }
    >
      <div className="flex h-full min-h-0 w-full flex-col">
        <div className="flex items-center gap-2 border-line border-b p-3">
          <Input
            autoFocus
            value={filter}
            placeholder="Filter schemas"
            aria-label="Filter schemas"
            onChange={(e) => setFilter(e.target.value)}
            className="flex-1"
          />
          <Button size="sm" variant="ghost" onClick={() => change(listed, true)}>
            Select all
          </Button>
          <Button size="sm" variant="ghost" onClick={() => change(listed, false)}>
            Clear
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto py-1">
          {listed.map((name) => (
            // biome-ignore lint/a11y/noLabelWithoutControl: the checkbox is the control
            <label
              key={name}
              className="flex h-7 cursor-pointer items-center gap-2 px-3 text-base hover:bg-hover"
            >
              <Checkbox checked={ticked.has(name)} onChange={(on) => change([name], on)} />
              <span className="min-w-0 flex-1 truncate">{name}</span>
            </label>
          ))}
          {listed.length === 0 && <p className="p-3 text-muted text-sm">Nothing matches.</p>}
        </div>
        <div className="border-line border-t px-3 py-1.5 text-muted text-xs">
          {ticked.size} of {names.length} shown in the tree
        </div>
      </div>
    </Dialog>
  );
}
