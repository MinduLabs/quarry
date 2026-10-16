# ADR 0003 — Data grid tự vẽ

- Trạng thái: Accepted · 2026-10-16

## Bối cảnh
`GtkColumnView` (mỗi cell là một widget) không đáp ứng được: edit theo từng cell, chọn vùng, hàng trăm cột, hàng triệu dòng.

## Quyết định
Viết widget riêng: implement `gtk::Scrollable`, vẽ bằng `snapshot()`, chiều cao dòng cố định, chỉ render những cell đang hiển thị. Dữ liệu lấy qua trait `GridSource`.

## Hệ quả
- Đây là rủi ro kỹ thuật lớn nhất, nên làm spike sớm (M1). Tiêu chí: 1M dòng × 50 cột, cuộn 60 fps.
- Phải tự làm phần accessibility cho grid.
