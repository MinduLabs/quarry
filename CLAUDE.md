# CLAUDE.md

Hướng dẫn cho AI agent (Claude Code, Codex, …) làm việc trong repo này. `AGENTS.md` là symlink tới file này.

## Dự án

Quarry: DB client native cho Linux. Rust + GTK4 + libadwaita (Relm4). Chỉ phát hành dạng Flatpak.
Kiến trúc: [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md). Mục lục tài liệu: [docs/README.md](docs/README.md).

## Lệnh

| Việc | Lệnh |
|---|---|
| Chạy app | `just run` (host cần GTK ≥ 4.14, libadwaita ≥ 1.5) |
| fmt + clippy + test | `just lint && just test`, hoặc `just check` (trong Docker Ubuntu 24.04) |
| Chụp màn hình UI sáng/tối | `just screenshot` → `dist/screenshots/` |
| Build Flatpak | `just flatpak` → `dist/quarry-<version>.flatpak` |
| Sau khi đổi `Cargo.lock` | `just cargo-sources` (nếu không, build Flatpak sẽ lỗi) |

Máy của maintainer chạy Ubuntu 22.04 (GTK quá cũ), nên build/test native phải chạy qua `just check` / `just screenshot`.

## Quy tắc cứng

- `crates/app` **chỉ** phụ thuộc `quarry-engine` (+ `model`). Không import driver, `store`, `tunnel` trực tiếp.
- Hướng phụ thuộc: `app → engine → {driver-api, sql, store, tunnel} → model`. `model` không có IO/async.
- Không `unwrap()`/`expect()` ngoài test (clippy deny). Không block UI thread; mọi IO đi qua engine (`Command` → `Event`).
- Chỉ dùng API GTK 4.14 / libadwaita 1.5 (feature `v4_14`, `v1_5`). Không bật feature version cao hơn khi chưa có ADR.
- Chuỗi hiển thị cho người dùng phải bọc `gettext(...)`; file mới có chuỗi thì thêm vào `po/POTFILES.in`.
- App ID `io.github.mindulabs.Quarry` xuất hiện ở: `crates/app/src/config.rs`, `data/*`, `build-aux/flatpak/*`, `justfile`. Đổi thì đổi hết.
- Dependency mới phải qua `cargo deny` (cấm GPL/AGPL của bên thứ ba, xem ADR 0002).

## UI

Trước khi đụng UI, đọc [docs/design/STYLE-GUIDE.md](docs/design/STYLE-GUIDE.md). Tóm tắt:
- Màu: chỉ dùng named color của libadwaita, cộng các màu `quarry_*` trong `data/resources/style.css`. Không hard-code hex trong Rust.
- Class CSS riêng có tiền tố `.quarry-`.
- Animation 150–250 ms, ease-out; ưu tiên animation có sẵn của libadwaita.
- Xong thì chạy `just screenshot` và tự xem ảnh sáng + tối.

## Quy ước

- Commit: Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`).
- Comment/doc trong code: tiếng Việt hoặc tiếng Anh đều được. File public ở root (README, CONTRIBUTING…) viết tiếng Anh.
- Quyết định kiến trúc mới → thêm ADR `docs/architecture/adr/NNNN-*.md`.
- File sinh tự động, không sửa tay: `build-aux/flatpak/cargo-sources.json`, `data/icons/hicolor/<size>x<size>/`.
