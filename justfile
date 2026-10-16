# Quarry task runner — `just --list` để xem tất cả lệnh.
# Cài: `cargo install just` (Ubuntu 24.04+: `sudo apt install just`).

app_id := "io.github.mindulabs.Quarry"
version := `grep -m1 '^version' Cargo.toml | cut -d'"' -f2`
manifest := "build-aux/flatpak/" + app_id + ".json"
# Bản build trong Docker tắt appstream compose: bước này render icon qua glycin (sandbox lồng nhau)
# và không chạy được trong container. CI (flathub-infra) và Flathub vẫn dùng manifest gốc.
local_manifest := "build-aux/flatpak/local.json"
uid := `id -u`
gid := `id -g`

# Chạy trong container Ubuntu 24.04 (GTK 4.14 / libadwaita 1.5) — dùng khi máy host có GTK cũ hơn.
docker_dev := "docker run --rm -u " + uid + ":" + gid + " -v " + justfile_directory() + ":/src -e CARGO_HOME=/src/target/.cargo-home -e CARGO_TARGET_DIR=/src/target/docker quarry-dev"

default:
    @just --list

# ---------- Native (cần GTK >= 4.14, libadwaita >= 1.5 trên host) ----------

# Chạy app (QUARRY_LOG=debug để xem log chi tiết)
run:
    cargo run -p quarry

# Format code
fmt:
    cargo fmt --all

# fmt check + clippy
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# Chạy test
test:
    cargo test --workspace

# Bật database test (MySQL 127.0.0.1:13306, dữ liệu mẫu trong tests/fixtures)
db-up:
    docker compose -f tests/docker-compose.yml up -d --wait

# Tắt và xoá database test
db-down:
    docker compose -f tests/docker-compose.yml down -v

# Integration test với database thật (cần `just db-up`)
test-db:
    QUARRY_TEST_MYSQL=127.0.0.1:13306 cargo test -p quarry-driver-mysql -p quarry-engine

# Kiểm tra license + advisory (cần `cargo install cargo-deny`)
deny:
    cargo deny check

# ---------- Docker (máy dev GTK cũ, ví dụ Ubuntu 22.04) ----------

# Build image môi trường dev
dev-image:
    docker build -t quarry-dev -f build-aux/docker/dev.Dockerfile build-aux/docker

# fmt + clippy + test trong container Ubuntu 24.04
check: dev-image
    {{docker_dev}} sh -c "cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace"

# Như `check` nhưng chạy cả integration test với database test (cần `just db-up`)
check-db: dev-image
    docker run --rm --network host -u {{uid}}:{{gid}} -v {{justfile_directory()}}:/src -e CARGO_HOME=/src/target/.cargo-home -e CARGO_TARGET_DIR=/src/target/docker -e QUARRY_TEST_MYSQL=127.0.0.1:13306 quarry-dev cargo test --workspace

# Chụp màn hình app (sáng + tối) vào dist/screenshots/ để review UI
screenshot: dev-image
    mkdir -p dist/screenshots
    {{docker_dev}} cargo build -p quarry
    docker run --rm -u {{uid}}:{{gid}} -v {{justfile_directory()}}:/src quarry-dev sh -c 'for s in light dark; do ( [ $s = dark ] && export ADW_DEBUG_COLOR_SCHEME=prefer-dark; xvfb-run -a -s "-screen 0 1280x800x24" sh -c "dbus-launch --exit-with-session /src/target/docker/debug/quarry >/dev/null 2>&1 & sleep 6; xwd -root -silent | convert xwd:- -crop 1200x760+0+0 /src/dist/screenshots/$s.png" ); done'

# Xuất icon PNG từ SVG (cho README/landing)
icons: dev-image
    {{docker_dev}} sh -c 'for s in 16 32 64 128 256 512; do mkdir -p data/icons/hicolor/${s}x${s}/apps && rsvg-convert -w $s -h $s data/icons/hicolor/scalable/apps/{{app_id}}.svg -o data/icons/hicolor/${s}x${s}/apps/{{app_id}}.png; done'

# ---------- Flatpak ----------

# Build image chứa flatpak-builder
flatpak-image:
    docker build -t quarry-flatpak -f build-aux/docker/flatpak.Dockerfile build-aux/docker

# Sinh lại cargo-sources.json (chạy mỗi khi Cargo.lock thay đổi)
cargo-sources: flatpak-image
    docker run --rm -u {{uid}}:{{gid}} -v {{justfile_directory()}}:/src -w /src quarry-flatpak python3 /usr/local/bin/flatpak-cargo-generator.py Cargo.lock -o build-aux/flatpak/cargo-sources.json

# Build file dist/quarry-<version>.flatpak (không cần cài flatpak-builder trên host)
flatpak: cargo-sources
    mkdir -p dist
    docker run --rm --privileged -v {{justfile_directory()}}:/src -v quarry-flatpak-data:/root/.local/share/flatpak -w /src quarry-flatpak sh -c '\
      set -e; \
      sed "s/\"command\": \"quarry\",/\"command\": \"quarry\", \"appstream-compose\": false,/" {{manifest}} > {{local_manifest}}; \
      flatpak-builder --user --install-deps-from=flathub --disable-rofiles-fuse --force-clean \
        --state-dir=/src/.flatpak-builder --repo=/src/.flatpak-repo /src/.flatpak-builder/build {{local_manifest}}; \
      flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
        /src/.flatpak-repo dist/quarry-{{version}}.flatpak {{app_id}}; \
      chown -R {{uid}}:{{gid}} /src/dist /src/.flatpak-builder /src/.flatpak-repo'
    @echo "→ dist/quarry-{{version}}.flatpak"

# Cài bản vừa build vào máy (cần flatpak trên host)
flatpak-install:
    flatpak install --user -y --reinstall dist/quarry-{{version}}.flatpak

# Xoá output build
clean:
    cargo clean
    rm -rf dist .flatpak-builder .flatpak-repo
