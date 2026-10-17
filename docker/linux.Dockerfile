# syntax=docker/dockerfile:1
# Linux build environment for Quarry. Used by `just appimage`, `just deb` and `just check-docker`.
#
# Ubuntu 22.04: Tauri's Linux guide says to build on the oldest distro you support that still ships
# WebKitGTK 4.1, because the binary requires the glibc it was built with.
#
# Caching: each step below is a cached layer, so the toolchain and npm packages are installed once.
# Cargo's registry and target dir live in BuildKit cache mounts (inside Docker, not in the repo).
# Clear them with `docker builder prune`.
#
# Output: the `export` stage holds only the finished bundle; `--output type=local,dest=dist`
# writes that single file to dist/ and nothing else.

FROM ubuntu:22.04 AS base
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates file patchelf xdg-utils \
      libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libssl-dev \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
      --component rustfmt,clippy
RUN corepack enable
WORKDIR /src

FROM base AS src
COPY package.json pnpm-lock.yaml ./
RUN --mount=type=cache,id=quarry-pnpm,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile
COPY . .

# `just check-docker`: fmt, clippy and tests. Produces no files.
FROM src AS check
RUN --mount=type=cache,id=quarry-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=quarry-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=quarry-target-debug,target=/src/target,sharing=locked \
    mkdir -p ui/dist \
    && cargo fmt --all --check \
    && cargo clippy --workspace --all-targets -- -D warnings \
    && cargo test --workspace

# `just appimage` / `just deb`. The bundle is copied out of the cache mount into /out.
FROM src AS bundle
ARG BUNDLE=appimage
RUN --mount=type=cache,id=quarry-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=quarry-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=quarry-target-release,target=/src/target,sharing=locked \
    --mount=type=cache,id=quarry-tauri-tools,target=/root/.cache/tauri \
    rm -rf "target/release/bundle/$BUNDLE" \
    && APPIMAGE_EXTRACT_AND_RUN=1 pnpm tauri build --bundles "$BUNDLE" \
    && mkdir -p /out \
    && find "target/release/bundle/$BUNDLE" -maxdepth 1 -type f \
         \( -name '*.AppImage' -o -name '*.deb' \) -exec cp {} /out/ \;

FROM scratch AS export
COPY --from=bundle /out/ /
