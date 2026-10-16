# Kế hoạch thực thi — Quarry (giai đoạn khởi tạo)

> Phạm vi: dựng nền móng (repo, tài liệu, logo, base code, build ra file cài được, style guide).
> **Không bao gồm logic nghiệp vụ** (driver, grid, editor), phần đó theo lộ trình trong `ARCHITECTURE.md` §11.

## Trạng thái (2026-10-16)

| Phase | Trạng thái | Còn lại |
|---|---|---|
| P0 Tài liệu | ✅ Xong | — |
| P1 Repo | ✅ Đã đẩy lên https://github.com/MinduLabs/quarry | Bật CLA Assistant + branch protection, Discussions |
| P2 Logo | 🟡 3 concept, dùng tạm concept A | Chốt concept; wordmark; PNG (`just icons`) |
| P3 Base code | ✅ Xong: cửa sổ chạy, engine ↔ UI thông kênh, CI xanh trong Docker | — |
| P4 Flatpak | ✅ `just flatpak` ra `dist/quarry-0.0.1.flatpak`, đã cài + chạy thử | CI flatpak chưa chạy thật trên GitHub |
| P5 Style guide | ✅ `STYLE-GUIDE.md` + `style.css` | Mockup màn hình chính / connection dialog |
| P6 Landing | ⏸ Làm sau | — |

## Các quyết định đã chốt

| Hạng mục | Quyết định |
|---|---|
| Tên | **Quarry**, tên đầy đủ "Quarry — Database Client" |
| Org GitHub / App ID | `MinduLabs` → `io.github.mindulabs.Quarry` (chữ thường). Chưa cần domain |
| Ngôn ngữ / UI | Rust + GTK4 + libadwaita (Relm4) — [ADR 0001](architecture/adr/0001-rust-gtk4-libadwaita.md) |
| License | AGPL-3.0-or-later, 1 repo, Pro khoá bằng key, có CLA — [ADR 0002](architecture/adr/0002-agpl-single-repo-license-key.md) |
| Phân phối MVP | **Chỉ Flatpak** — [ADR 0004](architecture/adr/0004-flatpak-only-mvp.md) |
| Baseline API | GTK 4.14 / libadwaita 1.5 (tương đương Ubuntu 24.04) |
| Giao diện | Tối giản, sạch, chuyển động tinh tế (tinh thần TablePro), nhưng dựng trên libadwaita — xem P5 |

## Thứ tự và phụ thuộc

```
P0 Thống nhất tài liệu ──► P1 Dựng repo ──┬──► P3 Base code ──► P4 Packaging ──► (bản v0.0.1 gửi người khác cài)
                                          ├──► P2 Logo ───────────┘ (cần icon để đóng gói)
                                          └──► P5 Style guide ─────► P6 Landing page (làm sau)
```

| Phase | Ước lượng | Định nghĩa "xong" |
|---|---|---|
| P0 | 0.5 ngày | Mọi tài liệu khớp với các quyết định ở trên, đặt đúng chỗ |
| P1 | 0.5 ngày | Repo trên GitHub, CI xanh với workspace rỗng |
| P2 | 1 ngày | Icon app + icon symbolic + bộ PNG; xem tốt ở 16 → 512 px, cả nền sáng lẫn tối |
| P3 | 1–2 ngày | `just run` mở ra cửa sổ Quarry có sidebar + tab trống; engine ↔ UI đã thông kênh |
| P4 | 1 ngày | `just flatpak` ra được file `.flatpak`; push tag là CI tự đính kèm vào GitHub Release |
| P5 | 1 ngày | `docs/design/STYLE-GUIDE.md` + `style.css` base được app dùng |
| P6 | 2 ngày (làm sau) | Landing page tĩnh deploy lên Cloudflare/GitHub Pages |

---

## P0 — Thống nhất tài liệu

### Cấu trúc tài liệu cho một dự án open source

Mỗi file có **một người đọc chính**. Không viết trùng nội dung giữa các file; file nào cần thì link sang file kia.

```
quarry/
├── README.md               # Người dùng + người mới: app là gì, ảnh chụp, cách cài, link docs
├── LICENSE                 # AGPL-3.0-or-later
├── CONTRIBUTING.md         # Contributor: setup dev, quy trình PR, CLA, commit convention
├── CODE_OF_CONDUCT.md      # Contributor Covenant
├── SECURITY.md             # Cách báo lỗ hổng (email riêng, không mở issue public)
├── CHANGELOG.md            # Người dùng: thay đổi theo version (Keep a Changelog)
├── CLAUDE.md               # AI agent (Claude Code…): lệnh build/test, quy ước, link tới docs. NGẮN
├── AGENTS.md → CLAUDE.md   # symlink, cho các agent khác đọc
└── docs/
    ├── README.md                   # Mục lục docs
    ├── product/
    │   ├── PRD-MVP.md              # Tính năng, phạm vi MVP
    │   ├── ROADMAP.md              # Các version, ưu tiên
    │   └── BUSINESS-MODEL.md       # License, Free/Pro, giá
    ├── architecture/
    │   ├── ARCHITECTURE.md         # Crate, runtime, luồng nghiệp vụ, cấu trúc repo
    │   └── adr/                    # Architecture Decision Records: mỗi quyết định lớn 1 file
    │       ├── 0001-rust-gtk4-libadwaita.md
    │       ├── 0002-agpl-single-repo-license-key.md
    │       ├── 0003-custom-data-grid.md
    │       └── 0004-distro-baseline-gtk414.md
    ├── design/
    │   ├── BRAND.md                # Tên, logo, màu thương hiệu, giọng văn
    │   └── STYLE-GUIDE.md          # Quy tắc UI trong app: layout, spacing, màu, typography, component
    └── dev/
        ├── SETUP.md                # Cài toolchain, chạy app
        ├── BUILDING.md             # Build từng định dạng package
        ├── RELEASING.md            # Checklist phát hành
        └── TESTING.md              # Test, conformance, docker
```

**ADR là gì:** mỗi file ghi lại một quyết định: *bối cảnh → quyết định → hệ quả*. Sau 6 tháng có người hỏi "sao không dùng Tauri?" thì chỉ cần đưa link ADR, không phải giải thích lại. Đây là chuẩn của các dự án open source lớn.

**CLAUDE.md chứa gì** (≤ 100 dòng):
- Một đoạn mô tả dự án, kèm link tới `ARCHITECTURE.md`
- Các lệnh: `just run`, `just test`, `just lint`, `just dist`
- Quy tắc cứng: `app` không import driver; không `unwrap()` ngoài `app`; UI không block; mọi chuỗi UI bọc `gettext`
- Quy ước: tên crate, commit theo Conventional Commits, file nào sinh tự động thì không sửa tay
- Link tới `STYLE-GUIDE.md` khi đụng vào UI

### Việc cần làm
1. Đổi codename thành tên chính thức, cập nhật app ID (`app.quarry.Quarry` nếu mua được domain, nếu không thì `io.github.<org>.Quarry`)
2. Sửa `BUSINESS-MODEL.md`: bỏ phần 2 repo; ghi rõ AGPL, 1 repo, license key, CLA, sync là nguồn thu chính
3. Sửa `ARCHITECTURE.md` §8 (cấu trúc repo, packaging) và §10 (Pro)
4. Sửa `PRD-MVP.md`: phân phối MVP chỉ Flatpak (DIST-01)
5. Viết 4 ADR đầu tiên, `ROADMAP.md`, `docs/README.md`
6. Chuyển các file vào đúng cây thư mục ở trên

---

## P1 — Dựng repo

| # | Việc | Chi tiết |
|---|---|---|
| 1 | Tạo thư mục, `git init` | `~/Dev/quarry` (cần mày xác nhận), nhánh `main` |
| 2 | GitHub | Org `MinduLabs`, repo `quarry` public. Bật Discussions, tắt Wiki |
| 3 | File nền | `LICENSE` (AGPL-3.0), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `CHANGELOG.md`, `CLAUDE.md`, `.editorconfig`, `.gitignore` |
| 4 | Toolchain | `rust-toolchain.toml` (stable, đặt MSRV), `rustfmt.toml`, `[workspace.lints]` trong `Cargo.toml`, `deny.toml` (chỉ cho phép license MIT/Apache/BSD/MPL/LGPL/Zlib/Unicode) |
| 5 | Task runner | `justfile` (lệnh `run`, `test`, `lint`, `fmt`, `deny`, `icons`, `flatpak`) |
| 6 | Template GitHub | `.github/ISSUE_TEMPLATE/{bug,feature}.yml`, `PULL_REQUEST_TEMPLATE.md`, `CODEOWNERS`, `dependabot.yml` |
| 7 | CI | `ci.yml`: fmt + clippy `-D warnings` + test + cargo-deny, chạy trong container `ubuntu:24.04` (đúng distro tối thiểu) |
| 8 | CLA | Cài CLA Assistant, file `CLA.md` (dựa trên Apache ICLA) |
| 9 | Bảo vệ nhánh | `main` yêu cầu CI xanh + review. Một mình thì cho phép tự merge |

**Commit convention:** Conventional Commits (`feat:`, `fix:`, `docs:`…). Sau này dùng `git-cliff` để tự sinh CHANGELOG.

---

## P2 — Logo

**Phong cách:** icon app theo **GNOME HIG**: khối 3D nhẹ, nhìn chính diện hơi chếch, bảng màu Adwaita, đặt trong lưới 128 px.

| # | Việc | Output |
|---|---|---|
| 1 | Vẽ 3 phương án SVG: (A) khối đá với mặt cắt là lưới bảng, có 1 ô màu hổ phách; (B) chữ "Q" lồng lưới bên trong; (C) bản kết hợp A+B | `branding/concepts/*.svg` |
| 2 | Xem thử cạnh icon GNOME khác, ở 16/32/64/128 px, trên nền sáng và tối | Ảnh so sánh |
| 3 | Mày chọn 1 → hoàn thiện | `data/icons/hicolor/scalable/apps/<app-id>.svg` |
| 4 | Icon symbolic (đơn sắc, 16 px, cho panel/thông báo) | `data/icons/hicolor/symbolic/apps/<app-id>-symbolic.svg` |
| 5 | Xuất PNG 16, 24, 32, 48, 64, 128, 256, 512 (cho About dialog và README) bằng `rsvg-convert` trong `just icons` | `data/icons/hicolor/<size>/apps/*.png` (file sinh tự động) |
| 6 | Brand mark: logo + chữ "Quarry", ngang và dọc, cho README/landing | `branding/wordmark*.svg` |
| 7 | Ghi vào `BRAND.md`: màu, khoảng trống tối thiểu quanh logo, cách dùng sai | — |

**Màu thương hiệu (dự kiến):** đá `#3D4A57` / `#5E6C7A`, mạch quặng (nhấn) `#E8A33D`. Chỉ dùng cho **logo, landing page, ảnh marketing**. Trong app thì tôn trọng màu accent của hệ thống (xem P5).

---

## P3 — Base code

Dựng **khung** theo `ARCHITECTURE.md` §8. Các crate có type và trait nhưng chưa có logic thật.

| # | Việc | Ghi chú |
|---|---|---|
| 1 | Workspace `Cargo.toml` + `[workspace.dependencies]` để khoá version một chỗ | gtk4 với feature `v4_14`, libadwaita với `v1_5` (khớp distro tối thiểu) |
| 2 | `crates/model`: `Value`, `ColumnMeta`, `ConnectionConfig`, `Environment`, `Capabilities` | Có serde, có test |
| 3 | `crates/driver-api`: trait `Driver`, `Session`, `Dialect`, `DriverRegistry` | Chưa có driver nào |
| 4 | `crates/sql`, `store`, `tunnel`: lib rỗng, có `lib.rs` + doc comment mô tả trách nhiệm | Giữ chỗ để đúng ranh giới phụ thuộc |
| 5 | `crates/engine`: `EngineHandle::spawn()` tạo tokio runtime riêng; `Command::Ping` → `Event::Pong` | **Chứng minh** kênh UI ↔ engine chạy được |
| 6 | `crates/app`: `AdwApplication` → `AdwApplicationWindow` gồm `AdwOverlaySplitView` (sidebar có trạng thái rỗng "No connections yet"), `AdwTabView` trống, header bar, About dialog có icon | Nút "Ping engine" ở chế độ debug để test kênh |
| 7 | `data/`: `.desktop`, `.metainfo.xml`, `gschema.xml`, gresource (icon + `style.css`) | Validate bằng `desktop-file-validate`, `appstreamcli validate` |
| 8 | i18n: cài sẵn `gettext-rs`, thư mục `po/`, bọc toàn bộ chuỗi UI | Chỉ ship tiếng Anh; tiếng Việt bổ sung sau |
| 9 | Logging: `tracing` + `tracing-subscriber`, đọc biến `QUARRY_LOG=debug` | — |
| 10 | Test: unit test cho `model`, smoke test khởi động engine | CI xanh |

**Định nghĩa xong:** `just run` mở được cửa sổ đúng layout, đổi theme hệ thống thì app đổi theo, About dialog hiện logo, CI xanh.

---

## P4 — Packaging (chỉ Flatpak)

| # | Việc | Chi tiết |
|---|---|---|
| 1 | Manifest | `build-aux/flatpak/io.github.mindulabs.Quarry.json`: runtime `org.gnome.Platform` (bản ổn định), SDK extension `rust-stable`, build offline từ `cargo-sources.json` |
| 2 | Sinh nguồn cargo | `flatpak-cargo-generator.py Cargo.lock` → `cargo-sources.json` (chạy lại mỗi khi `Cargo.lock` thay đổi) |
| 3 | Build trong Docker | `build-aux/docker/flatpak.Dockerfile` + `just flatpak`: máy host không cần cài flatpak-builder |
| 4 | Ra file | `flatpak build-bundle` → `dist/quarry-<ver>.flatpak` |
| 5 | CI | Tag `v*` → workflow `flatpak/flatpak-github-actions` build bundle → đính kèm vào GitHub Release |
| 6 | Người nhận cài | `flatpak install --user quarry-<ver>.flatpak` (bản đóng gói đã khai báo Flathub là nơi lấy runtime) |

**Định nghĩa xong:** có file `.flatpak`; cài trên máy khác thì icon Quarry hiện trong menu ứng dụng và mở được.

---

## P5 — Hướng giao diện và style guide

### Hướng giao diện (đã chốt)

**"Tối giản, yên tĩnh, chuyển động tinh tế"**: tinh thần giống TablePro/TablePlus, nhưng dựng trên libadwaita để hợp với desktop Linux. Chi tiết ở `design/STYLE-GUIDE.md`.

- **Tối giản:** chỉ hiện những gì cần. Toolbar ít nút, phần lớn hành động nằm trong phím tắt / menu / command palette. Không viền dày, không gradient, phân lớp bằng nền chứ không bằng đường kẻ.
- **Mật độ cao ở vùng dữ liệu:** grid và editor dày như công cụ dev chuyên nghiệp; phần chrome xung quanh thì thoáng.
- **Chuyển động tinh tế:** 150–250 ms, easing ease-out, dùng animation có sẵn của libadwaita (sidebar, tab, toast, dialog) và `AdwTimedAnimation`/`AdwSpringAnimation` cho phần tự vẽ. Không chuyển động trang trí. Khi hệ thống tắt animation thì tắt theo.

| Nguyên tắc | Ý nghĩa thực tế |
|---|---|
| Tôn trọng hệ thống | Dùng accent color, font, sáng/tối, scale chữ của hệ thống. Không tự ép theme riêng |
| Dữ liệu là nhân vật chính | Chrome (header, sidebar) tối giản; grid/editor chiếm tối đa diện tích |
| Keyboard-first | Mọi hành động có phím tắt và có trong command palette (`Ctrl+K`, từ v0.2) |
| An toàn phải nhìn thấy được | Màu môi trường (local/staging/prod) luôn hiện trên header tab và viền cửa sổ |
| Không bất ngờ | Mọi thao tác ghi đều có preview; thao tác phá huỷ dùng nút `destructive-action` |

### Nội dung `docs/design/STYLE-GUIDE.md`

1. **Layout:** sơ đồ cửa sổ (header bar, sidebar 260 px có thể thu gọn, tab view, status bar); breakpoint khi cửa sổ hẹp (sidebar thành overlay)
2. **Spacing:** thang 6 px (6 / 12 / 18 / 24) theo libadwaita; padding chuẩn cho dialog, list, toolbar
3. **Typography:**
   - UI dùng font hệ thống. Class: `.title-1…4`, `.heading`, `.body`, `.caption`, `.dim-label`
   - Dữ liệu và editor dùng font monospace của hệ thống (`org.gnome.desktop.interface monospace-font-name`)
   - Số trong grid: `font-feature-settings: "tnum"` để các số canh thẳng cột
4. **Màu:**
   - **Chỉ dùng named color của libadwaita** (`@accent_bg_color`, `@window_bg_color`, `@view_fg_color`…). Không hard-code hex trong UI
   - Màu ngữ nghĩa riêng của app (khai báo một chỗ trong `style.css` bằng `@define-color`):
     - `env_local` = green, `env_staging` = yellow/orange, `env_prod` = red (lấy từ bảng màu GNOME, có biến thể cho chế độ tối)
     - Trạng thái cell: `cell_modified`, `cell_inserted`, `cell_deleted`, `cell_null_fg`
   - Lý do dùng `@define-color` thay vì CSS variables: CSS variables chỉ có từ GTK 4.16, mà distro tối thiểu là 4.14
5. **Data grid:**
   - Chiều cao dòng 28 px (compact) hoặc 32 px (mặc định); padding cell 6 px
   - `NULL` hiển thị chữ nghiêng, màu `dim`
   - Số căn phải, text căn trái, bool hiện ✓/✗
   - Dòng chẵn lẻ tô màu xen kẽ, có thể tắt
6. **Editor:** style scheme sourceview `quarry-light` và `quarry-dark` dựa trên Adwaita; danh sách màu cho từng loại token
7. **Component:** khi nào dùng `AdwToast`, `AdwAlertDialog`, `AdwBanner` (vd: "Connected to PRODUCTION"), `AdwStatusPage` (trạng thái rỗng); nút `suggested-action` và `destructive-action`
8. **Icon trong UI:** chỉ dùng icon symbolic của Adwaita/icon-development-kit; icon riêng thì vẽ theo lưới 16 px
9. **Giọng văn:** câu ngắn, viết hoa theo kiểu tiêu đề cho nút/tiêu đề (theo HIG), thông báo lỗi nói **chuyện gì xảy ra + làm gì tiếp theo**
10. **Accessibility:** tương phản ≥ 4.5:1; mọi widget có label cho accessibility; không truyền đạt thông tin chỉ bằng màu (môi trường prod có cả **chữ** "PROD", không chỉ màu đỏ)
11. **Tên class CSS:** tiền tố `quarry-` cho class riêng (`.quarry-grid`, `.quarry-env-badge`)

### Việc cần làm
| # | Việc |
|---|---|
| 1 | Viết `STYLE-GUIDE.md` theo dàn ý trên |
| 2 | `data/resources/style.css`: khai báo màu ngữ nghĩa và class riêng; app load lúc khởi động |
| 3 | Mockup màn hình chính (sáng + tối) và màn hình connection dialog, dùng làm chuẩn để so khi code |
| 4 | Thêm một mục trong `CLAUDE.md`: "Đụng UI → đọc STYLE-GUIDE.md trước" |

---

## P6 — Landing page (làm sau P4)

| Hạng mục | Quyết định |
|---|---|
| Stack | **Astro**, xuất site tĩnh, không cần server |
| Hosting | Cloudflare Pages (miễn phí, dùng domain riêng) |
| Repo | Thư mục `website/` trong cùng repo (đơn giản; CI chỉ deploy khi thư mục này thay đổi) |
| Các section | Hero (tên, slogan, ảnh chụp, nút Download) → các DB hỗ trợ → tính năng (kèm ảnh/GIF) → "Native & open source" → Free vs Pro (để sau) → cách cài theo từng distro → footer (GitHub, Discord, license) |
| Nút Download | Tự nhận biết distro theo user-agent nếu được, nếu không thì hiện tab Flatpak (Flathub) và file `.flatpak` |
| Thương hiệu | Lấy màu, logo, wordmark từ `BRAND.md` |
| Đo lường | Plausible hoặc Umami (tôn trọng privacy, hợp với cộng đồng Linux) |

---

## Câu hỏi cần mày chốt trước khi chạy P1

1. **Thư mục dự án:** `~/Dev/quarry` được không?
2. **Tên org/user GitHub** để đặt repo và app ID?
3. **Domain:** đã mua chưa (`quarry.app` / `getquarry.app` / …)? Chưa mua thì tạm dùng `io.github.<org>.Quarry`. **Lưu ý:** app ID gần như không đổi được sau khi lên Flathub.
4. **Distro tối thiểu** Ubuntu 24.04 có ổn không? Hạ xuống 22.04 thì phải bỏ libadwaita 1.5, rất thiệt.
