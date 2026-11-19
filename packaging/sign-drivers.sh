#!/bin/sh
# Signs drivers.json (the argument) with the updater key, writing drivers.json.sig next to it. A
# release build of Quarry runs only drivers listed in a list signed with this key. A tag build
# without the key fails: its users could not install any driver.
set -eu
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  pnpm tauri signer sign "$1"
  exit 0
fi
case "${GITHUB_REF:-}" in
  refs/tags/v*)
    echo "::error::TAURI_SIGNING_PRIVATE_KEY is not set: a release build of Quarry refuses unsigned drivers."
    exit 1
    ;;
esac
echo "::warning::No TAURI_SIGNING_PRIVATE_KEY: drivers.json is not signed."
