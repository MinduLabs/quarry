#!/bin/sh
# Fails unless TAURI_SIGNING_PRIVATE_KEY is the private half of `plugins.updater.pubkey` in
# tauri.conf.json (the argument, default src-tauri/tauri.conf.json). Installed copies check every
# update against the public key they were built with, so a release signed with another key can
# never be installed by them. 0.1.0 shipped like that: the key was made again after its public key
# was committed. It signs a scratch file and compares the minisign key IDs.
set -eu
conf="${1:-src-tauri/tauri.conf.json}"
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "::warning::No TAURI_SIGNING_PRIVATE_KEY: the updater key was not checked."
  exit 0
fi
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
echo "quarry updater key check" > "$scratch/probe"
pnpm tauri signer sign "$scratch/probe" > /dev/null
python3 - "$conf" "$scratch/probe.sig" <<'PY'
import base64, json, sys

def key_id(b64):
    """The 8-byte key ID of a minisign public key or signature, as Tauri writes them (base64)."""
    lines = base64.b64decode(b64).decode().splitlines()
    return base64.b64decode(lines[1])[2:10][::-1].hex().upper()

conf, sig = sys.argv[1], sys.argv[2]
public = key_id(json.load(open(conf))["plugins"]["updater"]["pubkey"])
secret = key_id(open(sig).read())
if public != secret:
    print(
        f"::error::TAURI_SIGNING_PRIVATE_KEY (key {secret}) is not the key of "
        f"plugins.updater.pubkey in {conf} (key {public}). Installed copies would refuse this "
        "release's updates. Put the public key that was made with the secret's private key in "
        f"{conf}, or the matching private key in the secret. Never make a new key without "
        "changing both."
    )
    sys.exit(1)
print(f"updater key {public}: the secret matches tauri.conf.json")
PY
