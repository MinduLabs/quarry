import type { SslMode } from "@/bindings/SslMode";
import { Field, Input, PasswordInput } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import type { FieldsProps } from "../form";

const SSL_MODES: { value: SslMode; label: string }[] = [
  { value: "disable", label: "Disabled" },
  { value: "prefer", label: "Preferred" },
  { value: "require", label: "Required" },
  { value: "verify-ca", label: "Verify CA" },
  { value: "verify-full", label: "Verify identity" },
];

/** Host, port, credentials and TLS of a server reached over the network. */
export function ServerFields({ form, set, showInvalid, isEditing }: FieldsProps) {
  return (
    <>
      <div className="grid grid-cols-[1fr_96px] gap-3">
        <Field label="Host">
          <Input
            value={form.host}
            aria-invalid={showInvalid && !form.host.trim()}
            onChange={(e) => set("host", e.target.value)}
          />
        </Field>
        <Field label="Port">
          <Input
            inputMode="numeric"
            value={form.port}
            onChange={(e) => set("port", e.target.value.replace(/\D/g, ""))}
          />
        </Field>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <Field label="User">
          <Input value={form.user} onChange={(e) => set("user", e.target.value)} />
        </Field>
        <Field label="Password">
          <PasswordInput
            value={form.password}
            placeholder={isEditing ? "Unchanged" : ""}
            onChange={(e) => set("password", e.target.value)}
          />
        </Field>
      </div>

      <Field label="Default database" hint="Optional. Opened automatically after connecting.">
        <Input value={form.database} onChange={(e) => set("database", e.target.value)} />
      </Field>

      <Field label="SSL">
        <Select value={form.ssl} options={SSL_MODES} onChange={(v) => set("ssl", v)} />
      </Field>
    </>
  );
}
