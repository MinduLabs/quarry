import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { useApp } from "@/store/app";

/**
 * Quarry has not seen this SSH server before. Trusting it saves its key to ~/.ssh/known_hosts;
 * the user should compare the fingerprint first. A changed key never comes here: the engine
 * refuses it outright.
 */
export function SshTrustDialog() {
  const trust = useApp((s) => s.sshTrust);
  const accept = useApp((s) => s.trustSshHost);
  const dismiss = useApp((s) => s.dismissSshTrust);
  const address = trust ? `${trust.host}:${trust.port}` : "";

  return (
    <ConfirmDialog
      open={Boolean(trust)}
      title="Trust this SSH server?"
      description={`Quarry has not connected to ${address} before. Compare its key fingerprint with the one the server's admin gives you. Trust it only if they match: a server you cannot check may be someone else's.`}
      confirmLabel="Trust and connect"
      tone="primary"
      width={500}
      onConfirm={accept}
      onCancel={dismiss}
    >
      <p className="mb-1 text-sm text-muted">{trust?.keyType} key fingerprint</p>
      <pre className="rounded-md bg-surface p-3 font-mono text-sm text-fg break-all whitespace-pre-wrap select-text">
        {trust?.fingerprint}
      </pre>
      <p className="mt-2 text-sm text-muted">
        On the server,{" "}
        <code className="font-mono">
          ssh-keygen -lf /etc/ssh/ssh_host_{hostKeyFile(trust?.keyType)}_key.pub
        </code>{" "}
        prints it. Trusting saves the key to ~/.ssh/known_hosts.
      </p>
    </ConfirmDialog>
  );
}

/** The part of the server's host key file name for a key type: `ssh-ed25519` → `ed25519`. */
function hostKeyFile(keyType: string | undefined): string {
  if (keyType?.startsWith("ecdsa")) return "ecdsa";
  if (keyType?.includes("rsa")) return "rsa";
  return "ed25519";
}
