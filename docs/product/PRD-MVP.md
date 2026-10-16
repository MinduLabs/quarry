# PRD — MVP (v0.1)

> Trạng thái: **Draft để chốt**. Mọi thứ không có trong mục "Must" thì không làm trong MVP.

## 1. Vấn đề và người dùng

**Vấn đề:** trên Linux chưa có DB client nào vừa **native, nhẹ, hỗ trợ nhiều DB** vừa có **UI hiện đại**. Hiện trạng các lựa chọn:
- DBeaver và DataGrip chạy JVM, nặng.
- Beekeeper và DbGate là Electron.
- TablePlus Linux bị bỏ bê.
- pgAdmin chỉ dành cho Postgres.

**Người dùng mục tiêu (theo thứ tự ưu tiên):**
1. **Backend dev dùng Linux.** Dùng Postgres/MySQL hằng ngày, cần mở nhanh, query, sửa vài dòng dữ liệu.
2. **DevOps/SRE.** Vào DB prod qua bastion SSH, cần an toàn (không lỡ tay xoá dữ liệu).
3. **Fullstack/indie dev.** Làm việc với SQLite và DB local.

**Không nhắm đến trong MVP:** data analyst (cần chart, BI) và DBA chuyên sâu (cần quản lý user, replication, backup).

## 2. Mục tiêu MVP

| Mục tiêu | Đo bằng |
|---|---|
| Khởi động nhanh | Cold start < 1 s, RAM khi rảnh < 150 MB |
| Mượt với dữ liệu lớn | Browse bảng 10M dòng không lag; cuộn 60 fps |
| Đủ dùng hằng ngày cho PG/MySQL/SQLite | Dev dùng app thay DBeaver suốt 1 tuần mà không phải mở app khác |
| An toàn trên prod | 0 trường hợp ghi dữ liệu mà không có bước review/confirm khi ở môi trường prod |
| Lên Flathub | App được duyệt và listing public |

## 3. Phạm vi tính năng (MoSCoW)

### Must (chốt làm, thiếu cái nào thì không release)

| ID | Tính năng | Tiêu chí chấp nhận |
|---|---|---|
| **Kết nối** | | |
| CONN-01 | Thêm/sửa/xoá/nhân bản connection; nhóm theo group; màu; tag môi trường `local/staging/prod` | Lưu vào config, khởi động lại vẫn còn |
| CONN-02 | Hỗ trợ PostgreSQL, MySQL/MariaDB, SQLite | Bộ conformance test pass cho cả 3 |
| CONN-03 | SSL/TLS: các mode disable/require/verify-ca/verify-full, chọn file CA/cert/key | Kết nối được tới RDS và Supabase |
| CONN-04 | SSH tunnel: password, key file (có passphrase), ssh-agent; đọc `~/.ssh/config`; verify `known_hosts` | Host lạ hoặc key thay đổi thì hiện fingerprint và hỏi |
| CONN-05 | Password lưu trong keyring hệ thống, không bao giờ nằm trong file config | `grep` file config không thấy secret |
| CONN-06 | Nút Test connection, thông báo lỗi dễ hiểu (sai password, timeout, sai SSL) | — |
| **Schema** | | |
| SCH-01 | Cây: database → schema → table/view/materialized view/function/trigger/sequence/index | Load lười, cây 5000 bảng vẫn mượt |
| SCH-02 | Xem cấu trúc bảng: cột, kiểu, null, default, PK/FK/index | — |
| SCH-03 | Xem/copy DDL của object | — |
| SCH-04 | Tìm nhanh object (lọc theo tên) | — |
| **Dữ liệu** | | |
| DATA-01 | Browse bảng có phân trang, đếm dòng (ước lượng nhanh + đếm chính xác khi bấm) | Mở bảng 10M dòng < 1 s |
| DATA-02 | Filter bar (cột, toán tử, giá trị; nhiều điều kiện AND/OR) và sort theo cột, đều chạy ở server | SQL dùng param binding, không nối chuỗi |
| DATA-03 | Sửa inline, thêm dòng, xoá dòng, set NULL/DEFAULT | Chỉ cho sửa khi bảng có PK hoặc unique key |
| DATA-04 | **Review ChangeSet:** xem SQL sắp chạy, rồi Commit/Discard; chạy trong transaction | Một câu lỗi thì rollback toàn bộ |
| DATA-05 | Undo/redo cho các thay đổi chưa commit | — |
| DATA-06 | Chọn cell/dòng/vùng; copy dạng TSV/CSV/JSON/`INSERT` | — |
| DATA-07 | Xem/sửa giá trị dài (text, JSON có format, hex cho binary) | — |
| **SQL editor** | | |
| SQL-01 | Syntax highlight, số dòng, theme sáng/tối | — |
| SQL-02 | Autocomplete: keyword, schema, table, column (hiểu alias cơ bản) | Gợi ý < 50 ms |
| SQL-03 | Chạy câu tại cursor (`Ctrl+Enter`), chạy hết (`Ctrl+Shift+Enter`), chạy phần bôi đen; **Cancel** | Cancel dừng được query đang chạy trên cả 3 DB |
| SQL-04 | Nhiều result set, tab Messages (rows affected, thời gian chạy, notice, lỗi kèm vị trí) | — |
| SQL-05 | Format SQL | — |
| SQL-06 | Query history có full-text search, lọc theo connection | Tìm trong 100k entry < 100 ms |
| **UI/UX** | | |
| UI-01 | Nhiều tab, nhiều cửa sổ; khôi phục các tab khi mở lại app | — |
| UI-02 | Theo theme sáng/tối của hệ thống (libadwaita) | — |
| UI-03 | Bảng phím tắt (`Ctrl+?`) | — |
| **An toàn** | | |
| SAFE-01 | Safe mode theo môi trường (staging: confirm câu nguy hiểm; prod: confirm mọi câu ghi) | `DELETE`/`UPDATE` không có `WHERE` luôn bị chặn và hỏi lại |
| SAFE-02 | Connection read-only | Mọi câu ghi đều bị DB từ chối |
| SAFE-03 | Chỉ báo môi trường rõ ràng (màu viền/header theo tag) | — |
| **Xuất dữ liệu** | | |
| IO-01 | Export kết quả/bảng ra CSV, JSON, SQL `INSERT` (dạng stream, không load hết vào RAM) | Export 1M dòng không bị OOM |
| **Phân phối** | | |
| DIST-01 | Bản Flatpak: file `.flatpak` cài được trên mọi distro, sau đó lên Flathub | `flatpak install quarry.flatpak` chạy được trên Ubuntu/Fedora/Arch |

### Should (làm nếu còn thời gian, nếu không thì sang v0.2)

| ID | Tính năng |
|---|---|
| IO-02 | Import CSV vào bảng (map cột) |
| SQL-07 | Saved queries / snippets |
| SQL-08 | Vim mode (sourceview có sẵn, gần như free) |
| SQL-09 | `EXPLAIN` xem dạng cây |

### Won't (chốt KHÔNG làm trong MVP)

- Định dạng đóng gói khác ngoài Flatpak (`.deb`, `.rpm`, Snap, AppImage, AUR) → từ v0.2 nếu có nhu cầu
- Plugin system, các DB ngoài 3 DB đã chốt (Redis, Mongo, ClickHouse, MSSQL… để từ v0.2)
- AI chat/explain, MCP server, CLI
- Table designer GUI, ER diagram
- Schema compare / data compare, chart (những thứ này là ứng viên cho Pro)
- Sync, team, license (MVP **miễn phí 100%**, mục đích là kéo người dùng)
- Quản lý user/role DB, backup/restore

## 4. Release plan

| Version | Nội dung | Mục đích |
|---|---|---|
| v0.1 (MVP) | Toàn bộ mục Must | Lên Flathub, gom feedback, đạt 500 star |
| v0.2 | Mục Should + ClickHouse, Redis, MSSQL | Mở rộng người dùng |
| v0.3 | Plugin host, AI dùng key riêng (BYOK), MCP server | Tạo khác biệt |
| v1.0 | **Ra mắt gói Pro** (sync, compare, chart, ER) | Bắt đầu có doanh thu |

## 5. Câu hỏi còn mở

- Tên app và app ID chính thức?
- Có cần hỗ trợ Postgres < 12 không? (Đề xuất: không; tối thiểu PG 12, MySQL 5.7, MariaDB 10.6.)
- Ngôn ngữ UI: chỉ tiếng Anh, hay tiếng Anh + tiếng Việt từ đầu? (Đề xuất: dùng gettext ngay từ đầu, nhưng chỉ ship tiếng Anh.)
