import { Switch } from "@/components/ui/Switch";
import type { FieldsProps } from "../form";

export function OptionFields({ form, set }: FieldsProps) {
  return (
    <>
      <Switch
        checked={form.remember}
        onChange={(v) => set("remember", v)}
        label="Save password"
        description="Stored in the system keychain"
      />
      <Switch
        checked={form.readOnly}
        onChange={(v) => set("readOnly", v)}
        label="Read-only"
        description="The database rejects every write"
      />
    </>
  );
}
