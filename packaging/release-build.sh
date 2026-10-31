#!/bin/sh
# `pnpm tauri build` for a release; arguments pass through. With TAURI_SIGNING_PRIVATE_KEY set (a
# repository secret), the build also writes the signed self-update bundles and their `.sig`. A tag
# build without the key fails: its release could never update itself. Writes `signed=true|false`
# to $GITHUB_OUTPUT when it is set.
set -eu
signed=false
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  signed=true
  pnpm tauri build "$@" --config '{"bundle":{"createUpdaterArtifacts":true}}'
else
  case "${GITHUB_REF:-}" in
    refs/tags/v*)
      echo "::error::TAURI_SIGNING_PRIVATE_KEY is not set: a release without signed updates cannot update itself."
      exit 1
      ;;
  esac
  echo "::warning::No TAURI_SIGNING_PRIVATE_KEY: building without the self-update bundles."
  pnpm tauri build "$@"
fi
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "signed=$signed" >> "$GITHUB_OUTPUT"
fi
