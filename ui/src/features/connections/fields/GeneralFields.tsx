import type { Environment } from "@/bindings/Environment";
import { Field, Input } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import type { FieldsProps } from "../form";

const ENVIRONMENTS: { value: Environment; label: string }[] = [
  { value: "local", label: "Local" },
  { value: "staging", label: "Staging" },
  { value: "production", label: "Production" },
];

export function GeneralFields({ form, set }: FieldsProps) {
  return (
    <div className="grid grid-cols-[1fr_140px] gap-3">
      <Field label="Name" hint="Left empty, the host is used.">
        <Input
          autoFocus
          value={form.name}
          placeholder="Connection name"
          onChange={(e) => set("name", e.target.value)}
        />
      </Field>
      <Field label="Environment">
        <Select
          value={form.environment}
          options={ENVIRONMENTS}
          onChange={(v) => set("environment", v)}
        />
      </Field>
    </div>
  );
}
