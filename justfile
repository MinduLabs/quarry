# Quarry tasks — `just --list`. Install just with `cargo install just`.

app_id := "io.github.mindulabs.Quarry"
version := `grep -m1 '^version' Cargo.toml | cut -d'"' -f2`
uid := `id -u`
gid := `id -g`
docker_dev := "docker run --rm --network host -u " + uid + ":" + gid + " -v " + justfile_directory() + ":/src -e CARGO_HOME=/src/target/.cargo-home -e CARGO_TARGET_DIR=/src/target/docker quarry-tauri-dev"

default:
    @just --list

# Run the app with hot reload (needs Tauri's system deps: webkit2gtk-4.1 on Linux, Xcode CLT on macOS)
dev:
    pnpm tauri dev

# Lint and test frontend + Rust
check:
    pnpm check
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# Regenerate ui/src/bindings from Rust types
bindings:
    cargo test -q -p quarry-model -p quarry-engine

# Build the Linux dev image (for hosts without webkit2gtk-4.1)
dev-image:
    docker build -t quarry-tauri-dev -f docker/dev.Dockerfile docker

# Same as `check`, Rust part inside Docker
check-docker: dev-image
    pnpm check
    {{docker_dev}} sh -c "cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace"

# Start / stop the test MySQL (127.0.0.1:13306)
db-up:
    docker compose -f tests/docker-compose.yml up -d --wait

db-down:
    docker compose -f tests/docker-compose.yml down -v

# Integration tests against the test database
test-db:
    QUARRY_TEST_MYSQL=127.0.0.1:13306 cargo test -p quarry-driver-mysql -p quarry-engine

# macOS: build the universal .dmg (target/universal-apple-darwin/release/bundle/dmg). Run on a Mac.
dmg:
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    pnpm tauri build --target universal-apple-darwin --bundles dmg

# macOS from Linux: dist/macos/Quarry.app + .dmg (universal, ad-hoc signed, not notarized).
# Fast build by default (~40 s warm); `just mac-cross 1` for the full-LTO release profile.
mac-cross release="0":
    pnpm build
    docker build -t quarry-mac-cross -f docker/mac-cross.Dockerfile docker
    docker run --rm -u {{uid}}:{{gid}} -e HOME=/tmp -v {{justfile_directory()}}:/src -w /src \
      -e CARGO_HOME=/src/target/.cargo-home-mac -e CARGO_TARGET_DIR=/src/target -e RELEASE={{release}} quarry-mac-cross packaging/macos/cross-build.sh

# Linux: dist/linux/*.AppImage + *.deb, built inside Docker (Ubuntu 24.04).
# Fast build by default; `just linux-bundles 1` for the full-LTO release profile.
linux-bundles release="0": dev-image
    pnpm build
    {{docker_dev}} packaging/linux/bundle.sh {{release}}

# Linux: build dist/quarry-<version>.flatpak inside Docker
flatpak:
    pnpm build
    docker build -t quarry-flatpak -f docker/flatpak.Dockerfile docker
    docker run --rm -u {{uid}}:{{gid}} -v {{justfile_directory()}}:/src -w /src quarry-flatpak python3 /usr/local/bin/flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
    mkdir -p dist
    docker run --rm --privileged -v {{justfile_directory()}}:/src -v quarry-flatpak-data:/root/.local/share/flatpak -w /src quarry-flatpak sh -c '\
      set -e; \
      sed "s/\"command\": \"quarry\",/\"command\": \"quarry\", \"appstream-compose\": false,/" packaging/flatpak/{{app_id}}.json > packaging/flatpak/local.json; \
      flatpak-builder --user --install-deps-from=flathub --disable-rofiles-fuse --force-clean \
        --state-dir=/src/.flatpak-builder --repo=/src/.flatpak-repo /src/.flatpak-builder/build packaging/flatpak/local.json; \
      flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo /src/.flatpak-repo dist/quarry-{{version}}.flatpak {{app_id}}; \
      chown -R {{uid}}:{{gid}} /src/dist /src/.flatpak-builder /src/.flatpak-repo'
