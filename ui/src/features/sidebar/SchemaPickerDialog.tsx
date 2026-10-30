import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
import { SearchInput } from "@/components/ui/SearchInput";
import { cn } from "@/lib/cn";
import { shownSchemas, useApp } from "@/store/app";

/**
 * Every schema of a connection with a tick box; the tree lists only the ticked ones. Ticks are a
 * draft until Done, so Cancel (or Escape) leaves the tree as it was.
 */
export function SchemaPickerDialog() {
  const conn = useApp((s) => s.schemaPicker);
  const config = useApp((s) => s.connections.find((c) => c.id === conn));
  const databases = useApp((s) => (conn ? s.nodes[conn]?.databases : undefined));
  const saved = useApp((s) => (conn ? s.schemaFilter[conn] : undefined));
  const { setShownSchemas, setSchemaPicker } = useApp.getState();
  const [filter, setFilter] = useState("");
  const [draft, setDraft] = useState<Set<string>>(new Set());

  const names = useMemo(() => (databases ?? []).map((db) => db.name), [databases]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a draft starts when the dialog opens
  useEffect(() => {
    if (!conn) return;
    setDraft(new Set(shownSchemas(config?.database ?? null, databases ?? [], saved)));
    setFilter("");
  }, [conn]);

  const listed = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return needle ? names.filter((n) => n.toLowerCase().includes(needle)) : names;
  }, [names, filter]);
  const tickedListed = listed.filter((n) => draft.has(n)).length;
  const isAllListed = listed.length > 0 && tickedListed === listed.length;

  const tick = (changed: string[], on: boolean) =>
    setDraft((current) => {
      const next = new Set(current);
      for (const name of changed) {
        if (on) next.add(name);
        else next.delete(name);
      }
      return next;
    });
  const close = () => setSchemaPicker(null);
  const save = () => {
    // In the server's order, so the saved list does not depend on the order of the clicks.
    if (conn)
      setShownSchemas(
        conn,
        names.filter((n) => draft.has(n)),
      );
    close();
  };

  return (
    <Dialog
      open={conn !== null}
      onClose={close}
      title="Choose schemas"
      description={
        config ? `The sidebar lists only the ticked schemas of ${config.name}.` : undefined
      }
      // A fixed size, so ticking or filtering never resizes it; the list scrolls inside.
      width={440}
      height={540}
      flush
      footer={
        <>
          <span className="mr-auto text-muted text-sm tabular-nums">
            {draft.size} of {names.length} shown
          </span>
          <Button onClick={close}>Cancel</Button>
          <Button variant="primary" onClick={save}>
            Done
          </Button>
        </>
      }
    >
      <div className="flex min-h-0 w-full flex-col">
        <div className="px-6 pt-4 pb-3">
          <SearchInput
            autoFocus
            label="Filter schemas"
            placeholder="Filter schemas"
            value={filter}
            onChange={setFilter}
          />
        </div>
        {listed.length > 0 && (
          // biome-ignore lint/a11y/noLabelWithoutControl: the checkbox is the control
          <label className="mx-3 flex h-8 cursor-pointer items-center gap-2.5 border-line border-b px-3 font-medium text-muted text-sm">
            <Checkbox
              checked={isAllListed}
              indeterminate={tickedListed > 0 && !isAllListed}
              onChange={() => tick(listed, !isAllListed)}
            />
            {filter.trim() ? `All ${listed.length} matching` : "All schemas"}
          </label>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto px-3 pt-1 pb-3">
          {listed.map((name) => (
            // biome-ignore lint/a11y/noLabelWithoutControl: the checkbox is the control
            <label
              key={name}
              className={cn(
                "flex h-8 cursor-pointer items-center gap-2.5 rounded-md px-3 text-base transition-colors-fast hover:bg-hover",
                draft.has(name) ? "text-fg" : "text-muted",
              )}
            >
              <Checkbox checked={draft.has(name)} onChange={(on) => tick([name], on)} />
              <span className="min-w-0 flex-1 truncate">{name}</span>
            </label>
          ))}
          {listed.length === 0 && (
            <p className="px-3 py-6 text-center text-muted text-sm">No schema matches.</p>
          )}
        </div>
      </div>
    </Dialog>
  );
}
