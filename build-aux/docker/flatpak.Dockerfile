# Build Flatpak trong container (máy host không cần cài flatpak-builder).
# Runtime/SDK được flatpak-builder tự cài (--install-deps-from=flathub) vào volume cache.
# Chạy với --privileged vì flatpak-builder dùng bubblewrap.
# librsvg2-common: flatpak build-export cần đọc được icon SVG để kiểm tra.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      flatpak flatpak-builder librsvg2-common ca-certificates git python3 python3-aiohttp python3-tomlkit curl \
    && rm -rf /var/lib/apt/lists/*
RUN flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo \
    && curl -sSfL -o /usr/local/bin/flatpak-cargo-generator.py \
       https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
WORKDIR /src
