import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { missingDriver, useApp } from "@/store/app";
import { InstallProgress, isInstalling } from "./fields/DriverField";

/**
 * Shown instead of connecting when the connection's driver is not on this computer yet, so a
 * saved connection can download it without opening its settings. Once the driver is installed
 * the store closes this and connects.
 */
export function DriverDialog() {
  const dialog = useApp((s) => s.dialog);
  const connections = useApp((s) => s.connections);
  const sources = useApp((s) => s.dataSources);
  const target =
    dialog?.mode === "driver" ? connections.find((c) => c.id === dialog.id) : undefined;
  const source = target ? missingDriver(sources, target.driver) : undefined;
  const id = source?.driver ?? null;
  const install = useApp((s) => (id ? s.driverInstalls[id] : undefined));
  const { openDialog, installDriver } = useApp.getState();
  const isBusy = isInstalling(install);
  const hasFailed = install?.kind === "Failed";

  return (
    <Dialog
      open={Boolean(target && source)}
      onClose={() => openDialog(null)}
      title="Download the driver?"
      description={`“${target?.name ?? ""}” needs the ${source?.name ?? ""} driver, which is not on this computer yet. Quarry downloads it once and checks it before it runs.`}
      width={440}
      footer={
        <>
          <Button autoFocus onClick={() => openDialog(null)}>
            {isBusy ? "Close" : "Cancel"}
          </Button>
          <Button variant="primary" disabled={isBusy} onClick={() => id && installDriver(id)}>
            {hasFailed ? "Retry" : "Download and connect"}
          </Button>
        </>
      }
    >
      {install && isBusy && <InstallProgress state={install} />}
      {install?.kind === "Failed" && <ErrorNotice message={install.error} />}
    </Dialog>
  );
}
