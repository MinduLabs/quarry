# Quarry tasks — `just --list`. Install just with `cargo install just`.

app_id := "io.github.mindulabs.Quarry"
version := `grep -m1 '^version' Cargo.toml | cut -d'"' -f2`
uid := `id -u`
gid := `id -g`
# Linux builds run in docker/linux.Dockerfile. Caches stay inside Docker (`docker builder prune`).
docker_linux := "docker buildx build -f docker/linux.Dockerfile"

# Every driver binary, for the recipes that build or test them
drivers := "-p quarry-driver-mysql -p quarry-driver-postgres -p quarry-driver-sqlserver"

default:
    @just --list

# Run the app with hot reload (needs Tauri's system deps: webkit2gtk-4.1 on Linux, Xcode CLT on macOS)
# Drivers live where the installed app keeps them (the data directory's `drivers`). None is loaded
# up front: download one from a connection's settings, as a user does. This build serves them,
# and each run replaces the installed ones with the ones just built.
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    root="{{justfile_directory()}}"
    port="${QUARRY_DEV_DRIVER_PORT:-47821}"
    cargo build {{drivers}} --bins
    python3 "$root/scripts/dev-drivers.py" refresh "$root/target/debug"
    python3 "$root/scripts/dev-drivers.py" serve "$root/target/debug" "$port" &
    server=$!
    trap 'kill "$server" 2>/dev/null || true' EXIT
    QUARRY_DRIVER_MANIFEST="http://127.0.0.1:$port/drivers.json" \
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

# Same as `check`, Rust part inside Docker (for hosts without webkit2gtk-4.1). Leaves no files.
check-docker:
    pnpm check
    {{docker_linux}} --target check --output type=cacheonly .

# Integration tests against the test databases and the SSH server (tests/docker-compose.yml,
# started here). `docker compose -f tests/docker-compose.yml down -v` drops them.
test-db:
    docker compose -f tests/docker-compose.yml up -d --wait
    cargo build {{drivers}} --bins
    QUARRY_TEST_MYSQL=127.0.0.1:13306 QUARRY_TEST_POSTGRES=127.0.0.1:15432 QUARRY_TEST_SQLSERVER=127.0.0.1:11433 QUARRY_TEST_SSH=127.0.0.1:12222 QUARRY_DRIVER_DIR="{{justfile_directory()}}/target/debug" cargo test {{drivers}} -p quarry-engine

# Benchmarks in a release build: the history file, and query latency on the test MySQL
bench:
    cargo test --release -q -p quarry-store --test bench -- --ignored --nocapture
    docker compose -f tests/docker-compose.yml up -d --wait mysql
    cargo build --release -q {{drivers}} --bins
    QUARRY_TEST_MYSQL=127.0.0.1:13306 QUARRY_DRIVER_DIR="{{justfile_directory()}}/target/release" cargo test --release -q -p quarry-engine --test bench -- --ignored --nocapture

# macOS universal .dmg → dist/Quarry_<version>_universal.dmg. macOS only: Tauri cannot cross-build it.
[macos]
dmg:
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    pnpm tauri build --target universal-apple-darwin --bundles dmg
    mkdir -p dist
    cp target/universal-apple-darwin/release/bundle/dmg/Quarry_{{version}}_universal.dmg dist/
    @echo "Built: {{justfile_directory()}}/dist/Quarry_{{version}}_universal.dmg"

# Linux AppImage → dist/Quarry_<version>_amd64.AppImage (Docker, Ubuntu 22.04)
appimage:
    {{docker_linux}} --target export --build-arg BUNDLE=appimage --output type=local,dest=dist .
    @echo "Built: {{justfile_directory()}}/dist/Quarry_{{version}}_amd64.AppImage"

# Linux .deb → dist/Quarry_<version>_amd64.deb (Docker, Ubuntu 22.04)
deb:
    {{docker_linux}} --target export --build-arg BUNDLE=deb --output type=local,dest=dist .
    @echo "Built: {{justfile_directory()}}/dist/Quarry_{{version}}_amd64.deb"

# flatpak-builder state lives in the `quarry-flatpak-cache` volume, not in the repo.
# Linux Flatpak → dist/quarry-<version>.flatpak (Docker)
flatpak:
    pnpm build
    docker build -t quarry-flatpak -f docker/flatpak.Dockerfile docker
    docker run --rm -u {{uid}}:{{gid}} -v {{justfile_directory()}}:/src -w /src quarry-flatpak python3 /usr/local/bin/flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
    mkdir -p dist
    docker run --rm --privileged -v {{justfile_directory()}}:/src -v quarry-flatpak-data:/root/.local/share/flatpak -v quarry-flatpak-cache:/cache -w /src quarry-flatpak sh -c '\
      set -e; \
      trap "rm -f packaging/flatpak/local.json" EXIT; \
      sed "s/\"command\": \"quarry\",/\"command\": \"quarry\", \"appstream-compose\": false,/" packaging/flatpak/{{app_id}}.json > packaging/flatpak/local.json; \
      flatpak-builder --user --install-deps-from=flathub --disable-rofiles-fuse --force-clean \
        --state-dir=/cache/state --repo=/cache/repo /cache/build packaging/flatpak/local.json; \
      flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo /cache/repo dist/quarry-{{version}}.flatpak {{app_id}}; \
      chown {{uid}}:{{gid}} dist/quarry-{{version}}.flatpak'
    @echo "Built: {{justfile_directory()}}/dist/quarry-{{version}}.flatpak"

# BuildKit caches of the Linux builds are cleared with `docker builder prune`.
# Delete target/, dist/, ui/dist and the Flatpak cache volumes
clean:
    rm -rf target dist ui/dist
    -docker volume rm quarry-flatpak-cache quarry-flatpak-data
