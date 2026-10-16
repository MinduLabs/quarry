#!/bin/sh
# Build dist/macos/Quarry.app and Quarry_<version>_universal.app.zip from Linux (see `just mac-cross`).
# Expects ui/dist to be built and the SDK at $SDKROOT (docker/mac-cross.Dockerfile).
#
# No .dmg here on purpose: a real DMG (UDIF) can only be produced by macOS `hdiutil`, and
# genisoimage yields an ISO 9660 image that macOS refuses to mount. Use `just dmg` on a Mac.
#
# Default is a fast build (no LTO, parallel codegen, incremental): ~40 s when warm instead of ~3 min.
# RELEASE=1 uses the full release profile from Cargo.toml.
set -eu

if [ "${RELEASE:-0}" != 1 ]; then
    export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
        CARGO_PROFILE_RELEASE_INCREMENTAL=true CARGO_PROFILE_RELEASE_STRIP=false
    export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/src/target}/fast
fi
bin_dir=${CARGO_TARGET_DIR:-target}

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
target=universal2-apple-darwin
out=dist/macos
app="$out/Quarry.app"

export MACOSX_DEPLOYMENT_TARGET=11.0
cargo zigbuild --release --locked -p quarry --features custom-protocol --target "$target"

rm -rf "$out" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin_dir/$target/release/quarry" "$app/Contents/MacOS/quarry"
cp src-tauri/icons/icon.icns "$app/Contents/Resources/Quarry.icns"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Ad-hoc signature: required for arm64 binaries to launch at all.
rcodesign sign "$app"

(cd "$out" && zip -qry "Quarry_${version}_universal.app.zip" Quarry.app)
echo "Built $out"
