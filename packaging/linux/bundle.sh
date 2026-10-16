#!/bin/sh
# Build dist/linux/*.AppImage and *.deb inside the dev container (see `just linux-bundles`).
# Expects ui/dist to be built.
#
# Default is a fast build (no LTO, parallel codegen, incremental) in target/docker-fast.
# `bundle.sh 1` uses the full release profile from Cargo.toml (target/docker).
set -eu

cd /src
if [ "${1:-0}" != 1 ]; then
    export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
        CARGO_PROFILE_RELEASE_INCREMENTAL=true
    export CARGO_TARGET_DIR=/src/target/docker-fast
fi

# Keep the bundler's downloaded tools (linuxdeploy, AppRun) between runs.
export HOME=/src/target/.home
mkdir -p "$HOME"

cli=/src/target/.tauri-cli/bin/cargo-tauri
[ -x "$cli" ] || cargo install --locked tauri-cli --version '^2.12' --root /src/target/.tauri-cli

APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 "$cli" build --bundles appimage,deb --config docker/tauri.prebuilt-ui.json

mkdir -p dist/linux
cp "$CARGO_TARGET_DIR"/release/bundle/appimage/*.AppImage "$CARGO_TARGET_DIR"/release/bundle/deb/*.deb dist/linux/
