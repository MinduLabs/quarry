import { CircleCheck, Download, RotateCw } from "lucide-react";
import type { DataSource } from "@/bindings/DataSource";
import type { DriverInstallState } from "@/bindings/DriverInstallState";
import { Button } from "@/components/ui/Button";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { Field } from "@/components/ui/Input";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { formatBytes } from "@/lib/bytes";
import { useApp } from "@/store/app";
import { DataSourceLogo } from "../DataSourceLogo";

/**
 * Which driver the connection uses and whether it is on this machine yet. Quarry ships without
 * drivers: the user downloads each one here, the first time it is needed.
 */
export function DriverField({
  source,
  driver,
}: {
  source: DataSource | undefined;
  driver: string;
}) {
  const isInstalled = source?.is_installed ?? true;
  const id = source?.driver ?? null;
  const install = useApp((s) => (id ? s.driverInstalls[id] : undefined));
  const { installDriver } = useApp.getState();
  // "Installed" stays a moment after the engine is done, so a fast download is still seen.
  const isBusy =
    install?.kind === "Downloading" ||
    install?.kind === "Verifying" ||
    install?.kind === "Installed";
  const hasFailed = install?.kind === "Failed";

  return (
    <Field label="Driver">
      <div className="flex flex-col gap-2">
        <div className="flex h-7 items-center gap-2">
          {source && <DataSourceLogo id={source.id} size="sm" />}
          <span className="text-base text-fg">{source?.name ?? driver}</span>
          {!isBusy && (
            <span className={isInstalled ? "text-muted text-sm" : "text-sm text-warning"}>
              {isInstalled ? "Installed" : "Not downloaded"}
            </span>
          )}
          {!isInstalled && id && !isBusy && (
            <Button size="sm" className="ml-auto" onClick={() => installDriver(id)}>
              {hasFailed ? (
                <RotateCw size={14} strokeWidth={1.75} />
              ) : (
                <Download size={14} strokeWidth={1.75} />
              )}
              {hasFailed ? "Retry" : "Download"}
            </Button>
          )}
        </div>
        {install && isBusy && <InstallProgress state={install} />}
        {install?.kind === "Failed" && <ErrorNotice message={install.error} />}
      </div>
    </Field>
  );
}

/** The bytes so far against the size, then the check that runs before the driver is used. */
function InstallProgress({ state }: { state: DriverInstallState }) {
  if (state.kind === "Installed") {
    return (
      <div role="status" aria-live="polite" className="flex flex-col gap-1.5">
        <div className="flex items-center justify-between text-sm">
          <span className="flex items-center gap-1.5 text-success">
            <CircleCheck size={14} strokeWidth={1.75} />
            Driver installed
          </span>
          <span className="text-muted tabular-nums">100%</span>
        </div>
        <ProgressBar value={1} />
      </div>
    );
  }
  if (state.kind === "Verifying") {
    return (
      <div role="status" aria-live="polite" className="flex flex-col gap-1.5">
        <div className="flex items-center justify-between text-sm">
          <span className="text-fg">Checking the driver…</span>
          <span className="text-muted tabular-nums">100%</span>
        </div>
        <ProgressBar value={1} />
      </div>
    );
  }
  if (state.kind !== "Downloading") return null;
  const { received, total } = state;
  const fraction = total ? received / total : null;
  return (
    <div role="status" aria-live="polite" className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between gap-3 text-sm">
        <span className="truncate text-fg">
          Downloading…
          <span className="ml-1.5 text-muted tabular-nums">
            {total
              ? `${formatBytes(received)} of ${formatBytes(total)}`
              : received > 0 && formatBytes(received)}
          </span>
        </span>
        {fraction !== null && (
          <span className="shrink-0 text-muted tabular-nums">{Math.round(fraction * 100)}%</span>
        )}
      </div>
      <ProgressBar value={fraction} />
    </div>
  );
}
