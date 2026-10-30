import type { ConnectionConfig } from "@/bindings/ConnectionConfig";
import type { DataSource } from "@/bindings/DataSource";
import type { Environment } from "@/bindings/Environment";
import type { SshAuth } from "@/bindings/SshAuth";
import type { SslMode } from "@/bindings/SslMode";

/** What the connection step edits. Every field section reads and writes a slice of it. */
export interface ConnectionForm {
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
  ssh: boolean;
  sshHost: string;
  sshPort: string;
  sshUser: string;
  sshAuth: SshAuth;
  sshKeyPath: string;
  /** SSH password or key passphrase; empty keeps the saved one. */
  sshSecret: string;
}

export type SetField = <K extends keyof ConnectionForm>(key: K, value: ConnectionForm[K]) => void;

/** The props every field section takes from the connection step. */
export interface FieldsProps {
  form: ConnectionForm;
  set: SetField;
  /** Required fields left empty are marked once the user tried to save. */
  showInvalid: boolean;
  /** Editing a saved connection: empty secrets keep the saved ones. */
  isEditing: boolean;
}

const FALLBACK_PORT = 3306;

export function toForm(
  config: ConnectionConfig | undefined,
  source: DataSource | undefined,
): ConnectionForm {
  return {
    name: config?.name ?? "",
    environment: config?.environment ?? "local",
    host: config?.host ?? "127.0.0.1",
    port: String(config?.port ?? source?.default_port ?? FALLBACK_PORT),
    user: config?.user ?? "root",
    password: "",
    remember: true,
    database: config?.database ?? "",
    ssl: config?.ssl ?? "prefer",
    readOnly: config?.read_only ?? false,
    ssh: Boolean(config?.ssh),
    sshHost: config?.ssh?.host ?? "",
    sshPort: String(config?.ssh?.port ?? 22),
    sshUser: config?.ssh?.user ?? "",
    sshAuth: config?.ssh?.auth ?? "agent",
    sshKeyPath: config?.ssh?.key_path ?? "~/.ssh/id_ed25519",
    sshSecret: "",
  };
}

export function hasMissingFields(form: ConnectionForm): boolean {
  return !form.host.trim() || (form.ssh && (!form.sshHost.trim() || !form.sshUser.trim()));
}

/** The config to save or test; call `hasMissingFields` first. */
export function toConfig(
  form: ConnectionForm,
  driver: string,
  defaultPort: number | null,
  existing: ConnectionConfig | undefined,
): ConnectionConfig {
  const host = form.host.trim();
  const port = Number.parseInt(form.port, 10);
  const sshPort = Number.parseInt(form.sshPort, 10);
  return {
    id: existing?.id ?? crypto.randomUUID(),
    name: form.name.trim() || host,
    driver,
    host,
    port: Number.isFinite(port) ? port : (defaultPort ?? FALLBACK_PORT),
    database: form.database.trim() || null,
    user: form.user.trim() || null,
    path: null,
    ssl: form.ssl,
    ssh: form.ssh
      ? {
          host: form.sshHost.trim(),
          port: Number.isFinite(sshPort) ? sshPort : 22,
          user: form.sshUser.trim(),
          auth: form.sshAuth,
          key_path: form.sshAuth === "key" ? form.sshKeyPath.trim() || null : null,
        }
      : null,
    environment: form.environment,
    read_only: form.readOnly,
    color: existing?.color ?? null,
    group: existing?.group ?? null,
  };
}
