#!/bin/sh
# Build dist/Quarry.app and dist/Quarry_<version>_universal.dmg from Linux (see `just mac-cross`).
# Expects ui/dist to be built and the SDK at $SDKROOT (docker/mac-cross.Dockerfile).
set -eu

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
target=universal2-apple-darwin
out=dist/macos
app="$out/Quarry.app"

export MACOSX_DEPLOYMENT_TARGET=11.0
cargo zigbuild --release --locked -p quarry --features custom-protocol --target "$target"

rm -rf "$out" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$target/release/quarry" "$app/Contents/MacOS/quarry"
cp src-tauri/icons/icon.icns "$app/Contents/Resources/Quarry.icns"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Ad-hoc signature: required for arm64 binaries to launch at all.
rcodesign sign "$app"

staging="$out/dmg"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
genisoimage -quiet -V Quarry -D -R -apple -no-pad -o "$out/Quarry_${version}_universal.dmg" "$staging"
rm -rf "$staging"
(cd "$out" && zip -qry "Quarry_${version}_universal.app.zip" Quarry.app)
echo "Built $out"
