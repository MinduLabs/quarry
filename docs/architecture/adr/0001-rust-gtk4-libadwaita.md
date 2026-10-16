# ADR 0001 — Rust + GTK4 + libadwaita (Relm4)

- Trạng thái: Accepted · 2026-10-16

## Bối cảnh
Cần một DB client native, nhẹ, chỉ nhắm tới Linux. Các phương án đã xem: Tauri/Electron (webview: chậm trên WebKitGTK, mất điểm bán "native"), Qt6 (bridge sang Rust khá cực), GPUI (API chưa ổn định).

## Quyết định
- Core viết bằng **Rust**, chạy headless.
- UI dùng **GTK4 + libadwaita** qua `gtk4-rs`, quản lý component bằng **Relm4**.

## Hệ quả
- Hợp với GNOME, Flathub và Wayland. Driver async lấy được từ hệ sinh thái Rust.
- Data grid phải tự vẽ (xem ADR 0003).
- Nếu sau này cần Windows/macOS thì phải viết lại UI. Core vẫn dùng lại được.
