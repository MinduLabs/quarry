# Cross-builds the macOS app from Linux: zig as linker + the macOS 11.3 SDK (from the base image),
# rcodesign for ad-hoc signing, genisoimage for the .dmg. Output is unsigned by Apple (not notarized);
# release builds come from the macOS CI job (`just dmg`).
FROM ghcr.io/rust-cross/cargo-zigbuild:latest
RUN apt-get update && apt-get install -y --no-install-recommends genisoimage zip \
    && rm -rf /var/lib/apt/lists/*
ARG RCODESIGN=0.29.0
RUN curl -sSfL https://github.com/indygreg/apple-platform-rs/releases/download/apple-codesign%2F${RCODESIGN}/apple-codesign-${RCODESIGN}-x86_64-unknown-linux-musl.tar.gz \
      | tar -xz --strip-components=1 -C /usr/local/bin apple-codesign-${RCODESIGN}-x86_64-unknown-linux-musl/rcodesign
# rust-toolchain.toml asks for `stable`; install it with both macOS targets.
RUN rustup toolchain install stable --profile minimal --component rustfmt,clippy \
      --target aarch64-apple-darwin,x86_64-apple-darwin
WORKDIR /src
