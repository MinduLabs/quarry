import { CircleAlert, CircleCheck } from "lucide-react";
import { useEffect, useState } from "react";
import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { Environment } from "@/bindings/Environment";
import type { SslMode } from "@/bindings/SslMode";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Field, Input } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import { Spinner } from "@/components/ui/Spinner";
import { Switch } from "@/components/ui/Switch";
import { useApp } from "@/store/app";

const ENVIRONMENTS: { value: Environment; label: string }[] = [
  { value: "local", label: "Local" },
  { value: "staging", label: "Staging" },
  { value: "production", label: "Production" },
];

const SSL_MODES: { value: SslMode; label: string }[] = [
  { value: "disable", label: "Disabled" },
  { value: "prefer", label: "Preferred" },
  { value: "require", label: "Required" },
  { value: "verify-ca", label: "Verify CA" },
  { value: "verify-full", label: "Verify identity" },
];

interface Form {
  name: string;
  environment: Environment;
  host: string;
  port: string;
  user: string;
  password: string;
  remember: boolean;
  database: string;
  ssl: SslMode;
  readOnly: boolean;
}

function toForm(config: ConnectionConfig | undefined): Form {
  return {
    name: config?.name ?? "",
    environment: config?.environment ?? "local",
    host: config?.host ?? "127.0.0.1",
    port: String(config?.port ?? 3306),
    user: config?.user ?? "root",
    password: "",
    remember: true,
    database: config?.database ?? "",
    ssl: config?.ssl ?? "prefer",
    readOnly: config?.read_only ?? false,
  };
}

/** Create / edit a MySQL connection. */
export function ConnectionDialog() {
  const dialog = useApp((s) => s.dialog);
  const connections = useApp((s) => s.connections);
  const test = useApp((s) => s.test);
  const { openDialog, testConnection, saveConnection } = useApp.getState();

  const open = dialog?.mode === "new" || dialog?.mode === "edit";
  const existing =
    dialog?.mode === "edit" ? connections.find((c) => c.id === dialog.id) : undefined;
  const [form, setForm] = useState<Form>(() => toForm(existing));
  const [invalid, setInvalid] = useState(false);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reset only when the dialog opens
  useEffect(() => {
    if (open) {
      setForm(toForm(existing));
      setInvalid(false);
    }
  }, [open, existing?.id]);

  const set = <K extends keyof Form>(key: K, value: Form[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const build = (): ConnectionConfig | null => {
    const host = form.host.trim();
    if (!host) {
      setInvalid(true);
      return null;
    }
    const port = Number.parseInt(form.port, 10);
    return {
      id: existing?.id ?? crypto.randomUUID(),
      name: form.name.trim() || host,
      driver: "mysql",
      host,
      port: Number.isFinite(port) ? port : 3306,
      database: form.database.trim() || null,
      user: form.user.trim() || null,
      path: null,
      ssl: form.ssl,
      ssh: null,
      environment: form.environment,
      read_only: form.readOnly,
      color: existing?.color ?? null,
      group: existing?.group ?? null,
    };
  };

  const password = form.password ? form.password : null;

  return (
    <Dialog
      open={open}
      onClose={() => openDialog(null)}
      title={existing ? "Edit connection" : "New connection"}
      description="MySQL or MariaDB"
      width={480}
      footer={
        <>
          <div className="mr-auto flex min-w-0 items-center gap-2 text-sm">
            {test?.pending && <Spinner size={12} />}
            {test && !test.pending && (
              <span
                className={`flex min-w-0 items-center gap-1.5 ${test.ok ? "text-success" : "text-danger"}`}
              >
                {test.ok ? (
                  <CircleCheck size={14} strokeWidth={1.75} />
                ) : (
                  <CircleAlert size={14} strokeWidth={1.75} />
                )}
                <span className="truncate" title={test.message}>
                  {test.message}
                </span>
              </span>
            )}
          </div>
          <Button
            disabled={test?.pending}
            onClick={() => {
              const config = build();
              if (config) testConnection(config, password);
            }}
          >
            Test
          </Button>
          <Button
            variant="primary"
            onClick={() => {
              const config = build();
              if (config) saveConnection(config, password, form.remember);
            }}
          >
            Save
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          const config = build();
          if (config) saveConnection(config, password, form.remember);
        }}
      >
        <div className="grid grid-cols-[1fr_140px] gap-3">
          <Field label="Name">
            <Input
              autoFocus
              value={form.name}
              placeholder={form.host || "My database"}
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

        <div className="grid grid-cols-[1fr_96px] gap-3">
          <Field label="Host">
            <Input
              value={form.host}
              aria-invalid={invalid && !form.host.trim()}
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
            <Input
              type="password"
              value={form.password}
              placeholder={existing ? "Unchanged" : ""}
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

        <div className="flex flex-col gap-3 border-line border-t pt-4">
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
        </div>
        <button type="submit" hidden />
      </form>
    </Dialog>
  );
}

/** Confirmation before deleting a saved connection. */
export function DeleteConnectionDialog() {
  const dialog = useApp((s) => s.dialog);
  const connections = useApp((s) => s.connections);
  const { openDialog, deleteConnection } = useApp.getState();
  const target =
    dialog?.mode === "delete" ? connections.find((c) => c.id === dialog.id) : undefined;

  return (
    <Dialog
      open={Boolean(target)}
      onClose={() => openDialog(null)}
      title="Delete connection?"
      description={`“${target?.name ?? ""}” and its saved password will be removed. The database itself is not affected.`}
      width={400}
      footer={
        <>
          <Button autoFocus onClick={() => openDialog(null)}>
            Cancel
          </Button>
          <Button variant="danger" onClick={() => target && deleteConnection(target.id)}>
            Delete
          </Button>
        </>
      }
    />
  );
}
