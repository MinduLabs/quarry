# Setup môi trường dev

## Cách 1: host có GTK ≥ 4.14 (Ubuntu 24.04+, Fedora 40+, Arch)

```bash
# Ubuntu/Debian
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev gettext
# Fedora
sudo dnf install gcc pkgconf gtk4-devel libadwaita-devel gtksourceview5-devel gettext
```

Cài Rust (`rustup`) và `just` (`cargo install just`), sau đó:

```bash
just run
```

## Cách 2: host có GTK cũ (Ubuntu 22.04) — dùng Docker

Chỉ cần Docker, Rust và `just`:

```bash
just check        # fmt + clippy + test trong container Ubuntu 24.04
just screenshot   # chạy app trong màn hình ảo, xuất ảnh sáng/tối vào dist/screenshots/
just flatpak      # build file .flatpak để cài thử
```

Muốn chạy app có cửa sổ thật thì cài Flatpak (`sudo apt install flatpak`), rồi:

```bash
just flatpak && just flatpak-install
flatpak run io.github.mindulabs.Quarry
```

## Biến môi trường

| Biến | Ý nghĩa |
|---|---|
| `QUARRY_LOG` | Mức log, cú pháp `tracing` (`debug`, `quarry_engine=trace`) |
| `ADW_DEBUG_COLOR_SCHEME=prefer-dark` | Ép chế độ tối để test |
| `GTK_DEBUG=interactive` | Mở GTK Inspector |
