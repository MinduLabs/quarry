import { Download } from "lucide-react";
import type { DataSource } from "@/bindings/DataSource";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Input";
import { Tooltip } from "@/components/ui/Tooltip";
import { DataSourceLogo } from "../DataSourceLogo";

/** Which driver the connection uses and whether it is on this machine yet. */
export function DriverField({
  source,
  driver,
}: {
  source: DataSource | undefined;
  driver: string;
}) {
  const isInstalled = source?.is_installed ?? true;
  return (
    <Field label="Driver">
      <div className="flex h-7 items-center gap-2">
        {source && <DataSourceLogo id={source.id} size="sm" />}
        <span className="text-base text-fg">{source?.name ?? driver}</span>
        <span className={isInstalled ? "text-muted text-sm" : "text-sm text-warning"}>
          {isInstalled ? "Installed" : "Not downloaded"}
        </span>
        {!isInstalled && (
          <Tooltip content="Coming soon">
            {/* aria-disabled keeps the hover, so the tooltip still shows. */}
            <Button variant="ghost" size="sm" aria-disabled className="ml-auto opacity-45">
              <Download size={14} strokeWidth={1.75} />
              Download
            </Button>
          </Tooltip>
        )}
      </div>
    </Field>
  );
}
