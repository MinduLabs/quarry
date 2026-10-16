# ADR 0002 — AGPL, một repo, tính năng Pro khoá bằng license key

- Trạng thái: Accepted · 2026-10-16

## Bối cảnh
Muốn vừa open source vừa bán subscription. Các phương án: open-core 2 repo, 1 repo kèm thư mục `pro/` có license riêng, 1 repo mở hết (cách của TablePro).

## Quyết định
- **Một repo public, AGPL-3.0-or-later**, code Pro cũng nằm trong repo, khoá bằng license key ký Ed25519.
- Bắt buộc **CLA** cho contributor.
- `cargo-deny` chặn dependency GPL/AGPL của bên thứ ba.
- Nguồn thu chính là dịch vụ sync/team chạy trên server.

## Hệ quả
- Đơn giản nhất cho dev làm một mình; được cộng đồng ủng hộ.
- Người khác gỡ được đoạn kiểm tra license. Chấp nhận rủi ro này. Khi cần, chuyển tính năng Pro mới vào `pro/` (CLA cho phép).
