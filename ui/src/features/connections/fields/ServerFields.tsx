import type { DbAuth } from "@/bindings/DbAuth";
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

const AUTH_METHODS: { value: DbAuth["kind"]; label: string }[] = [
  { value: "password", label: "Password" },
  { value: "aws_iam", label: "AWS IAM" },
];

/** Host, port, credentials and TLS of a server reached over the network. */
export function ServerFields({
  form,
  set,
  showInvalid,
  isEditing,
  supportsAwsIam,
}: FieldsProps & {
  /** The driver signs in to Amazon RDS with AWS IAM. */
  supportsAwsIam: boolean;
}) {
  const isAwsIam = form.auth === "aws_iam";
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

      {/* A saved AWS IAM connection keeps its choice while its driver is not downloaded. */}
      {(supportsAwsIam || isAwsIam) && (
        <Field label="Sign in with">
          <Select
            value={form.auth}
            options={AUTH_METHODS}
            onChange={(v) => {
              set("auth", v);
              // The token is sent in clear text inside TLS, so the engine refuses it without.
              // RDS certificates are not in the public trust store, so Required, not Verify.
              if (v === "aws_iam" && (form.ssl === "disable" || form.ssl === "prefer")) {
                set("ssl", "require");
              }
            }}
          />
        </Field>
      )}

      <div className="grid grid-cols-2 gap-3">
        <Field label="User">
          <Input
            value={form.user}
            aria-invalid={showInvalid && isAwsIam && !form.user.trim()}
            onChange={(e) => set("user", e.target.value)}
          />
        </Field>
        {isAwsIam ? (
          <Field label="AWS profile" hint="Optional. From ~/.aws/config.">
            <Input
              value={form.awsProfile}
              placeholder="default"
              onChange={(e) => set("awsProfile", e.target.value)}
            />
          </Field>
        ) : (
          <Field label="Password">
            <PasswordInput
              value={form.password}
              placeholder={isEditing ? "Unchanged" : ""}
              onChange={(e) => set("password", e.target.value)}
            />
          </Field>
        )}
      </div>

      {isAwsIam && (
        <Field
          label="AWS region"
          hint="Optional. Read from the RDS host name, then from the profile. A new token is signed for every session."
        >
          <Input
            value={form.awsRegion}
            placeholder="From the host name"
            onChange={(e) => set("awsRegion", e.target.value)}
          />
        </Field>
      )}

      <Field label="Default database" hint="Optional. Opened automatically after connecting.">
        <Input value={form.database} onChange={(e) => set("database", e.target.value)} />
      </Field>

      <Field label="SSL">
        <Select value={form.ssl} options={SSL_MODES} onChange={(v) => set("ssl", v)} />
      </Field>
    </>
  );
}
