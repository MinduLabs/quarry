# Linux build environment for the Tauri app (Ubuntu 24.04, WebKitGTK 4.1).
# Used by `just check-docker` and `just linux-bundles`.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git file \
      libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
      librsvg2-dev librsvg2-bin libssl-dev \
      xvfb xdotool x11-apps imagemagick dbus-x11 fonts-noto-color-emoji \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
      --component rustfmt,clippy && chmod -R a+w $RUSTUP_HOME $CARGO_HOME
WORKDIR /src
