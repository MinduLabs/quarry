# Môi trường build/check native trên distro tối thiểu (Ubuntu 24.04: GTK 4.14, libadwaita 1.5).
# Dùng cho CI và cho máy dev có GTK cũ hơn (vd Ubuntu 22.04).
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git \
      libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev gettext \
      desktop-file-utils appstream librsvg2-bin librsvg2-common adwaita-icon-theme libglib2.0-dev-bin xvfb x11-apps imagemagick dbus-x11 fonts-cantarell xdotool \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
      --component rustfmt,clippy && chmod -R a+w $RUSTUP_HOME $CARGO_HOME
WORKDIR /src
