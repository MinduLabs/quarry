import { CircleCheck, Database, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Dialog } from "@/components/ui/Dialog";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";
import { type ConnectionForm, hasMissingFields, toConfig, toForm } from "./form";
import { ConnectionStep } from "./step/ConnectionStep";
import { DriverSelectStep } from "./step/DriverSelectStep";

type Step = "source" | "details";

const STEPS: { value: Step; label: string; icon: ReactNode }[] = [
  { value: "source", label: "Data source", icon: <Database size={16} strokeWidth={1.75} /> },
  {
    value: "details",
    label: "Connection",
    icon: <SlidersHorizontal size={16} strokeWidth={1.75} />,
  },
];

/**
 * Create or edit a connection in two steps: pick a data source, then fill in its settings.
 * Fixed size, so switching steps never resizes the window. Picking a source opens its settings
 * with a fresh form; editing starts at the settings.
 */
export function ConnectionDialog() {
  const dialog = useApp((s) => s.dialog);
  const connections = useApp((s) => s.connections);
  const sources = useApp((s) => s.dataSources);
  const test = useApp((s) => s.test);
  const { openDialog, testConnection, saveConnection } = useApp.getState();

  const open = dialog?.mode === "new" || dialog?.mode === "edit";
  const existing =
    dialog?.mode === "edit" ? connections.find((c) => c.id === dialog.id) : undefined;
  const sourceOf = (driver: string | undefined) =>
    driver === undefined ? undefined : sources.find((s) => s.driver === driver);

  const [step, setStep] = useState<Step>("source");
  const [sourceId, setSourceId] = useState<string | null>(null);
  const [form, setForm] = useState<ConnectionForm>(() => toForm(existing, undefined));
  const [showInvalid, setShowInvalid] = useState(false);
  const source = sources.find((s) => s.id === sourceId);
  const driver = source?.driver ?? existing?.driver ?? null;

  // biome-ignore lint/correctness/useExhaustiveDependencies: reset only when the dialog opens
  useEffect(() => {
    if (!open) return;
    const saved = sourceOf(existing?.driver);
    setStep(existing ? "details" : "source");
    setSourceId(saved?.id ?? null);
    setForm(toForm(existing, saved));
    setShowInvalid(false);
  }, [open, existing?.id]);

  const set = <K extends keyof ConnectionForm>(key: K, value: ConnectionForm[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  /** Another source starts a fresh form with its defaults; the same one keeps what was typed. */
  const pick = (id: string) => {
    setStep("details");
    if (id === sourceId) return;
    setSourceId(id);
    setForm(
      toForm(
        undefined,
        sources.find((s) => s.id === id),
      ),
    );
    setShowInvalid(false);
    // Drops the previous source's test result.
    openDialog({ mode: "new" });
  };

  const build = () => {
    if (!driver || hasMissingFields(form)) {
      setShowInvalid(true);
      return null;
    }
    return toConfig(form, driver, source?.default_port ?? null, existing);
  };

  const password = form.password ? form.password : null;
  const sshSecret = form.ssh && form.sshAuth !== "agent" && form.sshSecret ? form.sshSecret : null;
  const save = () => {
    const config = build();
    if (config) saveConnection(config, password, sshSecret, form.remember);
  };

  return (
    <Dialog
      open={open}
      onClose={() => openDialog(null)}
      title={existing ? "Edit connection" : "New connection"}
      // Editing has a single step, so it has no step list and needs less width.
      width={existing ? 560 : 760}
      height={520}
      flush
    >
      {!existing && (
        <nav
          aria-label="Steps"
          className="flex w-48 shrink-0 flex-col gap-0.5 border-line border-r p-2"
        >
          {STEPS.map((item) => (
            <button
              key={item.value}
              type="button"
              // The settings need a source first; going back to it keeps what was typed.
              disabled={item.value === "details" && !source}
              aria-current={item.value === step ? "step" : undefined}
              onClick={() => setStep(item.value)}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md px-2 text-left text-base transition-colors-fast",
                "outline-none focus-visible:ring-3 focus-visible:ring-accent-soft",
                "disabled:pointer-events-none disabled:opacity-45",
                item.value === step
                  ? "bg-selected text-fg"
                  : "text-muted hover:bg-hover hover:text-fg",
              )}
            >
              {item.icon}
              <span className="flex-1 truncate">{item.label}</span>
            </button>
          ))}
        </nav>
      )}
      {/* The footer lives in this column, so it never runs under the step list. */}
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          <h3 className="mb-4 font-semibold text-base text-fg">
            {step === "source" ? "Choose what to connect to" : "Connection settings"}
          </h3>
          {step === "source" ? (
            <DriverSelectStep sources={sources} selectedId={sourceId} onPick={pick} />
          ) : (
            <ConnectionStep
              source={source}
              driver={driver ?? ""}
              form={form}
              set={set}
              showInvalid={showInvalid}
              isEditing={Boolean(existing)}
              error={
                test && !test.pending && !test.ok
                  ? { key: test.request, message: test.message }
                  : null
              }
              onSubmit={save}
            />
          )}
        </div>
        {step === "source" && (
          <div className="flex items-center justify-end gap-2 border-line border-t px-6 py-3">
            {/* Back to the settings already typed, without picking the source again. */}
            <Button variant="primary" disabled={!source} onClick={() => setStep("details")}>
              Next
            </Button>
          </div>
        )}
        {step === "details" && (
          <div className="flex items-center gap-2 border-line border-t px-6 py-3">
            {!existing && <Button onClick={() => setStep("source")}>Previous</Button>}
            <div className="mr-auto flex min-w-0 items-center gap-2 text-sm">
              {test?.pending && <Spinner size={12} />}
              {test?.ok && (
                <span className="flex min-w-0 items-center gap-1.5 text-success">
                  <CircleCheck size={14} strokeWidth={1.75} />
                  <span className="truncate" title={test.message}>
                    {test.message}
                  </span>
                </span>
              )}
            </div>
            <Button
              disabled={test?.pending}
              onClick={() => {
                const config = build();
                if (config) testConnection(config, password, sshSecret);
              }}
            >
              Test
            </Button>
            <Button variant="primary" onClick={save}>
              Save
            </Button>
          </div>
        )}
      </div>
    </Dialog>
  );
}

/** Confirmation before deleting a saved connection. */
export function DeleteConnectionDialog() {
  const dialog = useApp((s) => s.dialog);
  const connections = useApp((s) => s.connections);
  const { openDialog, deleteConnection } = useApp.getState();
  const target =
    dialog?.mode === "delete" ? connections.find((c) => c.id === dialog.id) : undefined;

  return (
    <ConfirmDialog
      open={Boolean(target)}
      title="Delete connection?"
      description={`“${target?.name ?? ""}” and its saved password will be removed. The database itself is not affected.`}
      confirmLabel="Delete"
      width={400}
      onConfirm={() => target && deleteConnection(target.id)}
      onCancel={() => openDialog(null)}
    />
  );
}
