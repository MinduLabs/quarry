import type { DataSource } from "@/bindings/DataSource";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { DriverField } from "../fields/DriverField";
import { GeneralFields } from "../fields/GeneralFields";
import { OptionFields } from "../fields/OptionFields";
import { ServerFields } from "../fields/ServerFields";
import { SshFields } from "../fields/SshFields";
import type { FieldsProps } from "../form";

/** Step 2: the connection's settings. The dialog owns the form; this only lays out the sections. */
export function ConnectionStep({
  source,
  driver,
  error,
  onSubmit,
  ...fields
}: FieldsProps & {
  /** The picked data source; missing when a saved connection's driver is not in the catalog. */
  source: DataSource | undefined;
  driver: string;
  /** Last failed test, keyed so the notice replays for a new failure. */
  error: { key: number; message: string } | null;
  onSubmit: () => void;
}) {
  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
      }}
    >
      {error && <ErrorNotice key={error.key} message={error.message} />}
      <GeneralFields {...fields} />
      <DriverField source={source} driver={driver} />
      <ServerFields {...fields} />
      <div className="flex flex-col gap-3 border-line border-t pt-4">
        <SshFields {...fields} />
        <OptionFields {...fields} />
      </div>
      <button type="submit" hidden />
    </form>
  );
}
