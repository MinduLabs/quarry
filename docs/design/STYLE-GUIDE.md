# Style Guide — giao diện Quarry

> Áp dụng cho mọi UI trong app. Màu thương hiệu, logo và landing page: xem [BRAND.md](BRAND.md).
> CSS nền: `data/resources/style.css`. Baseline: GTK 4.14 / libadwaita 1.5 ([ADR 0004](../architecture/adr/0004-flatpak-only-mvp.md)).

## 1. Hướng thiết kế

**Tối giản, yên tĩnh, chuyển động tinh tế**, theo tinh thần TablePro/TablePlus nhưng dựng trên libadwaita để hợp với desktop Linux.

| Nguyên tắc | Làm | Không làm |
|---|---|---|
| **Dữ liệu là nhân vật chính** | Grid/editor chiếm tối đa diện tích; chrome mỏng | Toolbar nhiều hàng, panel thừa |
| **Ít mà đủ** | Mỗi màn hình 1 hành động chính (`suggested-action`); hành động phụ vào menu/phím tắt | Hàng chục nút icon |
| **Phân lớp bằng nền, không bằng đường kẻ** | Sidebar `@sidebar_bg_color`, nội dung `@view_bg_color` | Viền dày, khung lồng khung, gradient |
| **Tôn trọng hệ thống** | Accent, font, sáng/tối, cỡ chữ, tắt animation theo hệ thống | Theme riêng, ép màu accent |
| **Keyboard-first** | Mọi hành động có phím tắt, hiện trong tooltip và bảng phím tắt | Hành động chỉ làm được bằng chuột |
| **An toàn nhìn thấy được** | Môi trường prod: badge chữ "PROD" + viền đỏ; thao tác ghi luôn có preview | Chỉ dùng màu để báo nguy hiểm |

## 2. Layout

```
┌──────────────┬─────────────────────────────────────────────────┐
│ Sidebar      │ Header: [tab bar ............] [⋯] [≡]           │
│ 240–320 px   ├─────────────────────────────────────────────────┤
│              │                                                 │
│ Connections  │   Nội dung tab (editor / grid / table view)     │
│  └ schema    │                                                 │
│     tree     │                                                 │
│              ├─────────────────────────────────────────────────┤
│ footer       │ Status bar (24 px): rows · time · pending edits │
└──────────────┴─────────────────────────────────────────────────┘
```

- `AdwOverlaySplitView`: sidebar 240–320 px. Khi cửa sổ < 720 px thì sidebar chuyển thành overlay (breakpoint).
- Mỗi cột dùng `AdwToolbarView` + `AdwHeaderBar` riêng (sidebar có title, content thì không).
- Cửa sổ tối thiểu: 360 × 294 (yêu cầu của GNOME cho màn hình nhỏ).
- Dialog: `AdwDialog` / `AdwAlertDialog` / `AdwPreferencesDialog` (1.5), không dùng `GtkDialog`.

## 3. Spacing

Thang **6 px** của libadwaita: `6 · 12 · 18 · 24 · 36`.

| Chỗ | Giá trị |
|---|---|
| Khoảng cách giữa các control trong một nhóm | 6 |
| Padding trong card/list row | 12 |
| Khoảng cách giữa các nhóm | 18–24 |
| Lề trang/dialog | 24 (≥ 12 khi màn hình hẹp) |
| Grid cell padding | 0 × 6 (dọc × ngang) |

## 4. Typography

| Vai trò | Class / thiết lập |
|---|---|
| Tiêu đề trang trống | `.title-1` (welcome), `.title-2` |
| Tiêu đề nhóm, tên item | `.heading` |
| Nội dung | `.body` (mặc định) |
| Phụ, metadata | `.caption` + `.dim-label` |
| Dữ liệu, SQL | `.quarry-monospace` (font monospace hệ thống), `font-feature-settings: "tnum"` |

- Không đặt font cứng; dùng font của hệ thống (Cantarell/Adwaita Sans) và monospace của người dùng.
- Không dùng cỡ chữ tuyệt đối trong CSS cho chữ UI; muốn nhỏ hơn thì dùng `em`.

## 5. Màu

**Quy tắc:** trong code Rust và CSS chỉ dùng:
1. Named color của libadwaita: `@accent_color`, `@accent_bg_color`, `@window_bg_color`, `@view_bg_color`, `@view_fg_color`, `@sidebar_bg_color`, `@headerbar_bg_color`, `@card_bg_color`, `@borders`, `@success_color`, `@warning_color`, `@error_color`, …
2. Màu ngữ nghĩa của Quarry trong `style.css` (tiền tố `quarry_`):

| Token | Ý nghĩa |
|---|---|
| `quarry_env_local` / `_staging` / `_production` | Màu môi trường (green / yellow / red của GNOME) |
| `quarry_cell_modified` / `_inserted` / `_deleted` | Nền cell có thay đổi chưa commit |
| `quarry_cell_null` | Chữ `NULL` |
| `quarry_grid_line`, `quarry_row_alt` | Đường lưới, dòng xen kẽ (rất nhạt) |

- **Không hard-code hex** trong Rust. Cần màu mới → thêm token vào `style.css` và cập nhật bảng này.
- Màu hổ phách thương hiệu **không** dùng trong app (trừ icon); accent là của người dùng.
- Mọi cặp chữ/nền đạt tương phản ≥ 4.5:1 ở cả hai chế độ sáng và tối.

## 6. Chuyển động

Chuyển động để **giải thích thay đổi**, không để trang trí.

| Loại | Thời lượng | Easing | Cách làm |
|---|---|---|---|
| Hover, focus, đổi màu | 150 ms | ease-out | CSS `transition` |
| Phần tử xuất hiện/biến mất (toast, banner, popover) | 200 ms | ease-out | Có sẵn trong libadwaita |
| Panel/sidebar/tab, chuyển trang | 250 ms | ease-out / spring | `AdwOverlaySplitView`, `AdwTabView`, `AdwNavigationView` có sẵn |
| Phần tự vẽ (grid: highlight cell vừa commit, cuộn tới dòng) | 150–250 ms | ease-out cubic | `AdwTimedAnimation` (`Easing::EaseOutCubic`) |
| Kéo thả, thả tay | — | spring (damping ≈ 1) | `AdwSpringAnimation` |

Quy tắc:
- Không animation > 300 ms. Không animation lặp vô hạn (trừ spinner khi đang chờ).
- Người dùng tắt animation trong hệ thống (`gtk-enable-animations = false`) thì mọi animation tự vẽ phải nhảy thẳng tới trạng thái cuối (`AdwAnimation` tự xử lý điều này).
- Feedback cho thao tác: commit thành công → cell nháy nền accent nhạt rồi tắt dần (250 ms); lỗi → toast + đánh dấu dòng lỗi, không rung lắc.

## 7. Component — khi nào dùng gì

| Tình huống | Dùng |
|---|---|
| Thông báo ngắn, có thể undo ("3 rows deleted · Undo") | `AdwToast` |
| Trạng thái kéo dài liên quan tới nội dung ("Connected to PRODUCTION", "Read-only") | `AdwBanner` ở đầu vùng nội dung |
| Xác nhận thao tác nguy hiểm | `AdwAlertDialog`, nút chính `destructive-action`, nút mặc định là **Cancel** |
| Form cấu hình (connection, settings) | `AdwPreferencesDialog` + `AdwEntryRow` / `AdwComboRow` / `AdwSwitchRow` |
| Trang trống lớn (vùng nội dung) | `AdwStatusPage` |
| Trạng thái trống trong panel hẹp (sidebar) | `gtk::Box` `.quarry-empty`: icon 32 px dim, `.heading`, `.caption .dim-label`, nút pill |
| Đang tải | `GtkSpinner` (chưa có `AdwSpinner` vì baseline 1.5) |
| Nút hành động chính | `.suggested-action` (tối đa 1 trên mỗi view) |
| Nút trên header bar | Chỉ icon (`*-symbolic`) + tooltip có phím tắt |

## 8. Data grid

| Thuộc tính | Giá trị |
|---|---|
| Chiều cao dòng | 28 px (compact, mặc định) · 32 px (comfortable) |
| Font | Monospace hệ thống, `tnum` |
| Căn lề | Số căn phải; text, ngày, UUID căn trái; bool căn giữa (✓ / ✗) |
| `NULL` | Chữ "NULL" nghiêng, màu `quarry_cell_null` |
| Giá trị dài | Cắt với "…" ở cuối, xem đầy đủ trong panel chi tiết |
| Đường lưới | Chỉ đường ngang `quarry_grid_line`; đường dọc chỉ ở header |
| Dòng xen kẽ | `quarry_row_alt`, tắt được trong settings |
| Chọn | Nền `@accent_bg_color` alpha 0.15; cell đang focus có viền `@accent_color` 1 px |
| Cell đã sửa / thêm / xoá | `quarry_cell_modified` / `_inserted` / `_deleted` (xoá thì thêm gạch ngang) |
| Header | `.heading` cỡ 0.9em, nền `@headerbar_bg_color`, mũi tên sort 12 px |

## 9. SQL editor

- Style scheme `quarry-light` / `quarry-dark` dựa trên Adwaita; đổi theo `AdwStyleManager`.
- Statement đang ở con trỏ: nền `@accent_bg_color` alpha 0.06.
- Lỗi: gạch chân sóng `@error_color` ở đúng vị trí lỗi mà DB trả về.

## 10. Icon trong UI

- Chỉ dùng icon symbolic (`*-symbolic`) của Adwaita / GNOME icon-development-kit.
- Icon riêng vẽ theo lưới 16 px, nét 2 px, một màu `#222` (GTK tự đổi màu theo theme); lưu vào `data/icons/hicolor/symbolic/actions/`.

## 11. Văn bản trong UI

- Nút và tiêu đề dùng **Title Case** ("New Connection"); mô tả và thông báo dùng câu bình thường.
- Thông báo lỗi = **chuyện gì xảy ra + làm gì tiếp**: "Couldn't connect to db.example.com: password rejected. Check the password and try again."
- Dùng "…" (dấu ba chấm một ký tự) cho hành động sẽ mở thêm dialog: "Export…".
- Mọi chuỗi bọc `gettext`.

## 12. Accessibility

- Điều khiển được hoàn toàn bằng bàn phím; focus luôn nhìn thấy được.
- Widget chỉ có icon phải có `tooltip_text` (đồng thời là accessible label).
- Không truyền đạt thông tin chỉ bằng màu (môi trường có chữ, cell sửa có cả chỉ báo bên lề).
- Cỡ chữ lớn (`text-scaling-factor` 1.5) không làm vỡ layout.

## 13. CSS

- File: `data/resources/style.css` (AdwApplication tự nạp). Override riêng cho chế độ tối đặt trong `style-dark.css` (chỉ khi thật sự cần).
- Class riêng có tiền tố `.quarry-`. Không style lại widget chuẩn của libadwaita theo tên node (`button { … }`).
- Không dùng `!important`.

## 14. Checklist review UI

- [ ] Chụp `just screenshot`: sáng + tối đều ổn
- [ ] Không hex trong Rust; màu mới đã thêm vào token
- [ ] Có phím tắt + tooltip cho hành động mới
- [ ] Cửa sổ hẹp (360 px) không vỡ
- [ ] Chuỗi đã bọc `gettext`
