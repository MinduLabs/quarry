# ADR 0004 — MVP chỉ phát hành Flatpak; baseline GTK 4.14 / libadwaita 1.5

- Trạng thái: Accepted · 2026-10-16

## Bối cảnh
Duy trì nhiều định dạng package (deb, rpm, snap, AppImage) tốn công khi chỉ có một người làm. Cần một file duy nhất cài được trên mọi distro.

## Quyết định
- MVP **chỉ build Flatpak**: một file `.flatpak` để gửi tay cho người khác, và cùng manifest đó dùng để lên Flathub. Runtime: GNOME (bản ổn định trên Flathub).
- API dùng trong code giới hạn ở **GTK 4.14 / libadwaita 1.5** (feature `v4_14`, `v1_5`), tương đương Ubuntu 24.04. Nhờ vậy sau này làm thêm `.deb` mà không phải sửa code.

## Hệ quả
- Không dùng được các widget mới hơn (`AdwSpinner`, `AdwToggleGroup`, CSS variables của GTK 4.16). Thay bằng `GtkSpinner` và `@define-color`.
- Máy dev có GTK cũ hơn (Ubuntu 22.04) thì build và check qua Docker (`build-aux/docker/dev.Dockerfile`), hoặc qua Flatpak.
