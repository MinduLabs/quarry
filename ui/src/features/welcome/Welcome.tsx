import { Button } from "@/components/ui/Button";
import { Kbd } from "@/components/ui/Kbd";
import { modKey } from "@/lib/platform";
import { useApp } from "@/store/app";

const SHORTCUTS: [string, string][] = [
  ["New connection", `${modKey} N`],
  ["New query", `${modKey} T`],
  ["Run statement", `${modKey} ↵`],
  ["Close tab", `${modKey} W`],
];

export function Welcome() {
  const hasConnections = useApp((s) => s.connections.length > 0);
  const openDialog = useApp((s) => s.openDialog);
  return (
    <div
      data-tauri-drag-region
      className="animate-fade flex h-full flex-col items-center justify-center gap-6"
    >
      <img src="/icon.svg" alt="" className="size-20" draggable={false} />
      <div className="flex flex-col items-center gap-1">
        <h1 className="font-semibold text-2xl text-fg">Quarry</h1>
        <p className="text-base text-muted">
          {hasConnections
            ? "Pick a table in the sidebar or open a new query."
            : "Connect to a database to get started."}
        </p>
      </div>
      {!hasConnections && (
        <Button variant="primary" onClick={() => openDialog({ mode: "new" })}>
          New connection
        </Button>
      )}
      <dl className="grid grid-cols-[auto_auto] items-center gap-x-8 gap-y-2 text-sm">
        {SHORTCUTS.map(([label, keys]) => (
          <div key={label} className="contents">
            <dt className="text-muted">{label}</dt>
            <dd className="flex justify-end">
              <Kbd keys={keys} />
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
