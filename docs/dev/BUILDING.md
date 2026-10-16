# Build & phát hành

MVP chỉ phát hành **Flatpak** ([ADR 0004](../architecture/adr/0004-flatpak-only-mvp.md)).

## Build file `.flatpak` trên máy

```bash
just flatpak
# → dist/quarry-<version>.flatpak
```

Toàn bộ quá trình chạy trong Docker (`build-aux/docker/flatpak.Dockerfile`), máy host không cần flatpak-builder. Lần đầu sẽ tải GNOME SDK 51 (~1–2 GB) vào Docker volume `quarry-flatpak-data`; các lần sau dùng lại cache.

> Bản build trong Docker dùng `build-aux/flatpak/local.json` (sinh tự động, đã bỏ qua trong git) với `"appstream-compose": false`. Lý do: bước compose của GNOME 51 render icon qua glycin, mà glycin cần sandbox lồng nhau (`flatpak-spawn --sandbox`) nên không chạy được trong container. Hệ quả duy nhất là bundle không có metadata cho GNOME Software; app, icon và menu vẫn bình thường. CI (`flathub-infra`) và Flathub dùng manifest gốc.

**Người nhận cài** (mọi distro, kể cả Ubuntu 22.04):

```bash
# Một lần duy nhất: cài Flatpak + thêm Flathub (nơi lấy GNOME runtime)
sudo apt install flatpak
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo

# Cài app
flatpak install --user quarry-0.0.1.flatpak
flatpak run io.github.mindulabs.Quarry   # hoặc mở từ menu ứng dụng (có thể cần đăng xuất/đăng nhập lại lần đầu)
```

Distro khác: xem https://flatpak.org/setup/. Đã thử: cài bundle và chạy được trên GNOME runtime 51 (Xvfb trong container).

## Các file liên quan

| File | Vai trò |
|---|---|
| `build-aux/flatpak/io.github.mindulabs.Quarry.json` | Manifest: runtime GNOME 51 + rust-stable 26.08, quyền sandbox |
| `build-aux/flatpak/cargo-sources.json` | Danh sách crate để build offline. **Sinh lại bằng `just cargo-sources` mỗi khi `Cargo.lock` thay đổi** |
| `data/*.desktop`, `data/*.metainfo.xml`, `data/icons/` | Cài vào `/app/share/...` |

## Phát hành một version

1. Tăng `version` trong `Cargo.toml` (workspace) → `cargo check` để cập nhật `Cargo.lock` → `just cargo-sources`
2. Thêm `<release>` trong `data/io.github.mindulabs.Quarry.metainfo.xml`, cập nhật `CHANGELOG.md`
3. Commit, tag `vX.Y.Z`, push tag
4. Workflow `release.yml` build Flatpak và tạo **draft** GitHub Release kèm `.flatpak` + `SHA256SUMS`
5. Cài thử bản trên release (Ubuntu + Fedora), rồi publish release

## Lên Flathub (từ v0.1)

Mở PR vào `flathub/flathub` với manifest trong `build-aux/flatpak/` (đổi source `dir` thành `git` với tag + commit). Mỗi quyền sandbox mới (ví dụ `--socket=ssh-auth` khi làm SSH) cần giải thích trong PR.
