import { Download } from "lucide-react";
import type { DataSource } from "@/bindings/DataSource";
import { Button } from "@/components/ui/Button";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { Field } from "@/components/ui/Input";
import { Spinner } from "@/components/ui/Spinner";
import { useApp } from "@/store/app";
import { DataSourceLogo } from "../DataSourceLogo";

/**
 * Which driver the connection uses and whether it is on this machine yet. Quarry ships without
 * drivers: each is downloaded here the first time it is needed.
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
  const isDownloading = install?.kind === "Downloading";
  const fraction = install?.kind === "Downloading" ? install.fraction : null;

  return (
    <Field label="Driver">
      <div className="flex flex-col gap-2">
        <div className="flex h-7 items-center gap-2">
          {source && <DataSourceLogo id={source.id} size="sm" />}
          <span className="text-base text-fg">{source?.name ?? driver}</span>
          {isDownloading ? (
            <span className="flex items-center gap-1.5 text-muted text-sm">
              <Spinner size={12} />
              Downloading…
              {fraction !== null && (
                <span className="tabular-nums">{Math.round(fraction * 100)}%</span>
              )}
            </span>
          ) : (
            <span className={isInstalled ? "text-muted text-sm" : "text-sm text-warning"}>
              {isInstalled ? "Installed" : "Not downloaded"}
            </span>
          )}
          {!isInstalled && id && !isDownloading && (
            <Button variant="ghost" size="sm" className="ml-auto" onClick={() => installDriver(id)}>
              <Download size={14} strokeWidth={1.75} />
              {install?.kind === "Failed" ? "Retry" : "Download"}
            </Button>
          )}
        </div>
        {fraction !== null && (
          <div className="h-1 overflow-hidden rounded-full bg-hover">
            {/* Scaled, not resized: only transform animates smoothly in WebKitGTK. */}
            <div
              className="h-full origin-left bg-accent transition-transform duration-150"
              style={{ transform: `scaleX(${fraction})` }}
            />
          </div>
        )}
        {install?.kind === "Failed" && <ErrorNotice message={install.error} />}
      </div>
    </Field>
  );
}
