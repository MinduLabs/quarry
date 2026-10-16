# Builds the Flatpak inside a container, so the host needs no flatpak-builder.
# flatpak-builder installs the runtime/SDK itself (--install-deps-from=flathub) into a cache volume.
# Run with --privileged: flatpak-builder uses bubblewrap.
# librsvg2-common: flatpak build-export must be able to read the SVG icon.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      flatpak flatpak-builder librsvg2-common ca-certificates git python3 python3-aiohttp python3-tomlkit curl \
    && rm -rf /var/lib/apt/lists/*
RUN flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo \
    && curl -sSfL -o /usr/local/bin/flatpak-cargo-generator.py \
       https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
WORKDIR /src
