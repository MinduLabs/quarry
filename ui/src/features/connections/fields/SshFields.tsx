import type { SshAuth } from "@/bindings/SshAuth";
import { Field, Input, PasswordInput } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import { Switch } from "@/components/ui/Switch";
import type { FieldsProps } from "../form";

const SSH_AUTH: { value: SshAuth; label: string }[] = [
  { value: "agent", label: "SSH agent" },
  { value: "key", label: "Key file" },
  { value: "password", label: "Password" },
];

export function SshFields({ form, set, showInvalid, isEditing }: FieldsProps) {
  return (
    <>
      <Switch
        checked={form.ssh}
        onChange={(v) => set("ssh", v)}
        label="Connect over SSH"
        description="Host and port above are then as seen from the SSH server"
      />
      {form.ssh && (
        <>
          <div className="grid grid-cols-[1fr_96px] gap-3">
            <Field label="SSH host">
              <Input
                value={form.sshHost}
                placeholder="Host, IP address or ~/.ssh/config alias"
                aria-invalid={showInvalid && !form.sshHost.trim()}
                onChange={(e) => set("sshHost", e.target.value)}
              />
            </Field>
            <Field label="SSH port">
              <Input
                inputMode="numeric"
                value={form.sshPort}
                onChange={(e) => set("sshPort", e.target.value.replace(/\D/g, ""))}
              />
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Field label="SSH user">
              <Input
                value={form.sshUser}
                placeholder="From ~/.ssh/config"
                onChange={(e) => set("sshUser", e.target.value)}
              />
            </Field>
            <Field label="Sign in with">
              <Select value={form.sshAuth} options={SSH_AUTH} onChange={(v) => set("sshAuth", v)} />
            </Field>
          </div>
          {form.sshAuth === "key" && (
            <Field
              label="Private key file"
              hint="Empty: the alias's IdentityFile in ~/.ssh/config."
            >
              <Input value={form.sshKeyPath} onChange={(e) => set("sshKeyPath", e.target.value)} />
            </Field>
          )}
          {form.sshAuth !== "agent" && (
            <Field
              label={form.sshAuth === "key" ? "Key passphrase" : "SSH password"}
              hint={form.sshAuth === "key" ? "Leave empty if the key has none." : undefined}
            >
              <PasswordInput
                value={form.sshSecret}
                placeholder={isEditing ? "Unchanged" : ""}
                onChange={(e) => set("sshSecret", e.target.value)}
              />
            </Field>
          )}
          <Field
            label="Jump hosts"
            hint="Optional, like ProxyJump: user@host:port, comma separated. Signs in with ssh-agent or a key file, never the SSH password."
          >
            <Input
              value={form.sshJump}
              placeholder="From ~/.ssh/config"
              onChange={(e) => set("sshJump", e.target.value)}
            />
          </Field>
          <p className="text-muted text-sm">
            Settings left empty come from ~/.ssh/config. A server Quarry has not seen asks you to
            check its key once.
          </p>
        </>
      )}
    </>
  );
}
