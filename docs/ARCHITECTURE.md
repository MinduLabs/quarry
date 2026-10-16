# Kiến trúc MVP — DB client native cho Linux (Rust + GTK4)

> Tên: **Quarry** — Database Client. Repo: `github.com/MinduLabs/quarry`. App ID: `io.github.mindulabs.Quarry`.

## 0. Phạm vi MVP

- DB: **PostgreSQL, MySQL/MariaDB, SQLite**
- Connection manager, SSH tunnel, SSL, password trong keyring
- Schema tree, browse table (filter/sort/paging server-side)
- SQL editor (highlight, autocomplete cơ bản, run statement tại cursor), multi-tab
- Data grid virtualized, inline edit → **ChangeSet preview → commit**
- Query history FTS, export CSV/JSON/SQL, import CSV
- Safe mode cho môi trường prod
- Phân phối: Flathub

**Không có trong MVP:** plugin host, AI, MCP/CLI, Pro. Nhưng ranh giới crate phải sẵn để thêm sau mà không phải đập lại.

---

## 1. Nguyên tắc kiến trúc

1. **Core headless.** Toàn bộ logic nằm trong `engine`, không import `gtk`. UI là một client của engine. Sau này CLI, MCP server hay TUI cũng chỉ là một client khác.
2. **Hướng phụ thuộc một chiều:** `app → engine → {driver-api, sql, store, tunnel} → model`. Crate `app` **không bao giờ** import trực tiếp driver cụ thể.
3. **Mọi type đi qua ranh giới driver đều serialize được** (`serde`). Như vậy khi chuyển driver sang plugin out-of-process (JSON-RPC) thì không phải đổi model.
4. **Driver khai báo capability, UI đọc capability.** Không viết `if driver == "postgres"` trong UI.
5. **Không bao giờ load toàn bộ result vào RAM.** Kết quả luôn stream hoặc chia trang.

---

## 2. Sơ đồ crate

```
                        ┌──────────────────────┐
                        │   app  (bin, GTK4)   │  gtk4, libadwaita, relm4, sourceview5
                        └──────────┬───────────┘
                                   │ Command / Event (async_channel)
                        ┌──────────▼───────────┐
                        │        engine        │  tokio runtime riêng
                        │ ConnectionManager    │
                        │ QueryExecutor        │
                        │ SchemaCache          │
                        │ ChangeSetService     │
                        │ HistoryService       │
                        │ SafetyGuard          │
                        └─┬──────┬──────┬────┬─┘
                          │      │      │    │
          ┌───────────────▼┐ ┌───▼───┐ ┌▼────▼──┐ ┌────────┐
          │   driver-api   │ │  sql  │ │ store  │ │ tunnel │
          │ trait Driver   │ │ split │ │ config │ │ russh  │
          │ trait Session  │ │ class │ │ history│ │ agent  │
          │ Dialect        │ │ compl.│ │ secrets│ │ known_ │
          └───────┬────────┘ └───┬───┘ └───┬────┘ │ hosts  │
                  │              │         │      └────────┘
   ┌──────────────┼──────────┐   │         │
   ▼              ▼          ▼   │         │
drivers/      drivers/   drivers/│         │
postgres      mysql      sqlite  │         │
(tokio-pg)  (mysql_async)(rusqlite)        │
   └──────────────┴──────────┴───┴─────────┘
                          ▼
                     ┌─────────┐
                     │  model  │  types thuần, không async, không IO
                     └─────────┘
```

| Crate | Trách nhiệm | Deps chính |
|---|---|---|
| `model` | `Value`, `ColumnMeta`, `RowBatch`, `SchemaNode`, `ConnectionConfig`, `Capabilities`, `Environment` | `serde`, `chrono`, `uuid` |
| `driver-api` | trait `Driver`, `Session`, `Dialect`; `DriverRegistry`; error type chung | `async-trait`, `thiserror` |
| `drivers/postgres` | Kết nối, execute stream, introspect qua `pg_catalog`, cancel | `tokio-postgres`, `postgres-types`, `tokio-postgres-rustls` |
| `drivers/mysql` | Như trên, introspect qua `information_schema`, cancel bằng `KILL QUERY` trên connection phụ | `mysql_async` |
| `drivers/sqlite` | Chạy trong `spawn_blocking`, cancel bằng `interrupt_handle()` | `rusqlite` (feature `bundled`) |
| `tunnel` | SSH local port-forward, ssh-agent, key file, `~/.ssh/config`, verify `known_hosts` | `russh`, `ssh2-config` |
| `sql` | Tách statement theo dialect (`$$`, `DELIMITER`, comment); tìm statement tại cursor; phân loại câu lệnh (read/write/DDL/destructive); ngữ cảnh autocomplete; format | `sqlformat`, tokenizer tự viết |
| `store` | Config (TOML, XDG), history (SQLite + FTS5), secrets | `rusqlite`, `oo7`, `directories` |
| `engine` | Điều phối toàn bộ; API dạng Command/Event | tất cả ở trên, `tokio` |
| `app` | UI GTK | `gtk4`, `libadwaita`, `relm4`, `sourceview5` |

---

## 3. Contract driver (phần quan trọng nhất, chốt sớm)

```rust
// driver-api/src/lib.rs
#[async_trait]
pub trait Driver: Send + Sync + 'static {
    fn id(&self) -> DriverId;                 // "postgres" | "mysql" | "sqlite"
    fn capabilities(&self) -> Capabilities;
    fn dialect(&self) -> Arc<dyn Dialect>;
    async fn connect(&self, cfg: &ResolvedConfig) -> Result<Box<dyn Session>>;
}

#[async_trait]
pub trait Session: Send {
    /// Trả về stream; caller kéo batch theo nhu cầu (backpressure).
    async fn execute(&mut self, sql: &str, opts: ExecOpts) -> Result<ExecStream>;
    async fn execute_params(&mut self, sql: &str, params: &[Value]) -> Result<ExecSummary>;
    async fn introspect(&mut self, req: IntrospectRequest) -> Result<Vec<SchemaNode>>;
    async fn begin(&mut self) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
    async fn rollback(&mut self) -> Result<()>;
    fn cancel_handle(&self) -> CancelHandle;   // dùng được từ task khác khi đang execute
    async fn set_read_only(&mut self, on: bool) -> Result<()>;
}

// Thực tế đã implement (MVP): `execute(&mut self, sql, &ExecOpts, sink: &mut dyn FnMut(ExecItem))`
// — driver đẩy kết quả qua callback thay vì trả Stream, vì `mysql_async::QueryResult`
// mượn connection nên khó trả về Stream 'static.
pub enum ExecItem {               // một script có thể sinh nhiều kết quả
    Columns(Vec<ColumnMeta>),
    Rows(RowBatch),               // ~500 rows/batch
    Affected(u64),
    Notice(String),
    Done { elapsed: Duration },
}

pub trait Dialect: Send + Sync {
    fn quote_ident(&self, s: &str) -> String;
    fn placeholder(&self, idx: usize) -> String;          // $1 | ? 
    fn paginate(&self, base: &str, limit: u64, offset: u64) -> String;
    fn read_only_stmt(&self) -> Option<&'static str>;
}

pub struct Capabilities {
    pub schemas: bool,            // sqlite = false
    pub transactions: bool,
    pub editable_results: bool,
    pub explain: bool,
    pub cancel: bool,
    pub ssh: bool, pub ssl: bool,
    pub object_kinds: Vec<ObjectKind>, // Table, View, MatView, Function, Trigger, Sequence...
}
```

**`Value`**: là một enum đóng, có nhánh fallback để không bao giờ panic khi gặp type lạ.

```rust
pub enum Value {
    Null, Bool(bool), Int(i64), UInt(u64), Float(f64),
    Decimal(String),              // giữ nguyên chuỗi, không làm tròn
    Text(String), Bytes(Vec<u8>),
    Date(NaiveDate), Time(NaiveTime), Timestamp(NaiveDateTime), TimestampTz(DateTime<FixedOffset>),
    Uuid(Uuid), Json(String),
    Array(Vec<Value>),
    Other { type_name: String, text: String },   // enum, geometry, inet, range, ...
}
```

Postgres: decode theo OID; với OID lạ, decode dạng text thành `Other`. Nhờ vậy grid luôn hiển thị được dữ liệu.

**Session theo tab:** mỗi query tab có `Session` riêng (transaction và query dài không chặn nhau). Mỗi connection có thêm **1 session metadata** dùng cho introspect và autocomplete. Engine giới hạn số session tối đa cho mỗi connection.

---

## 4. Mô hình runtime (threading)

```
 GTK main thread (glib MainContext)          Engine (tokio multi-thread runtime)
 ─────────────────────────────────           ───────────────────────────────────
  Relm4 components                             EngineHandle
     │  cmd_tx.send(Command::RunQuery{..})  ──►   dispatcher ──► spawn task / session actor
     │                                               │
     │  glib::spawn_future_local(               ◄── event_tx.send(Event::Rows{tab, batch})
     │     async { while let Ok(ev) =                │
     │        event_rx.recv().await { .. } })        └── mỗi Session là 1 actor (mpsc inbox)
```

- **Kênh:** dùng `async_channel` (không phụ thuộc runtime, nên chạy được cả ở phía glib lẫn phía tokio).
- **Mỗi session là một actor:** task tokio sở hữu `Box<dyn Session>`, nhận lệnh qua mpsc. Nhờ vậy không cần `Mutex` quanh session. Riêng cancel đi đường tắt qua `CancelHandle`.
- **UI không bao giờ `block_on`.** Mọi IO đều đi qua engine.

```rust
pub enum Command {
    Connect { conn: ConnId },
    Disconnect { conn: ConnId },
    OpenSession { conn: ConnId, tab: TabId },
    Run { tab: TabId, sql: String, limit: Option<u64> },
    FetchMore { tab: TabId, result: ResultId },
    Cancel { tab: TabId },
    BrowseTable { tab: TabId, table: TableRef, filter: Filter, sort: Vec<Sort>, page: Page },
    Introspect { conn: ConnId, req: IntrospectRequest },
    ApplyChangeSet { tab: TabId, set: ChangeSet, dry_run: bool },
    // ...
}

pub enum Event {
    ConnState { conn: ConnId, state: ConnState },
    ResultStarted { tab: TabId, result: ResultId, columns: Vec<ColumnMeta> },
    Rows { tab: TabId, result: ResultId, batch: RowBatch },
    ResultDone { tab: TabId, result: ResultId, summary: ExecSummary },
    SchemaLoaded { conn: ConnId, nodes: Vec<SchemaNode> },
    ChangeSetPreview { tab: TabId, statements: Vec<String> },
    ConfirmRequired { tab: TabId, reason: SafetyReason, token: ConfirmToken },
    Error { scope: Scope, err: UiError },
}
```

---

## 5. Các luồng nghiệp vụ chính

### 5.1 Connect (có SSH)

```
UI Connect → engine.ConnectionManager
  1. store.config.load(conn_id)       → ConnectionConfig (không có secret)
  2. store.secrets.get(conn_id)       → password / passphrase (oo7: Secret portal hoặc Secret Service)
  3. nếu có ssh: tunnel.open(profile) → 127.0.0.1:<random_port>
       - verify known_hosts; host lạ → Event::ConfirmRequired(HostKey fingerprint)
  4. ResolvedConfig { host=127.0.0.1, port=random, ... }
  5. driver.connect() → metadata session
  6. Introspect level 1 (schemas/tables) → SchemaCache → Event::SchemaLoaded
```

Tunnel được đếm tham chiếu (refcount) theo connection; session cuối cùng đóng thì tunnel đóng. Có keepalive và tự reconnect khi mất kết nối (báo trạng thái `Reconnecting`).

### 5.2 Browse table

- Engine tự sinh `SELECT * FROM t WHERE <filter> ORDER BY <sort> LIMIT n OFFSET m`. **Filter và sort luôn chạy ở server.**
- Đếm số dòng: dùng ước lượng nhanh trước (pg: `reltuples`, mysql: `information_schema.TABLES.TABLE_ROWS`), chỉ `COUNT(*)` chính xác khi người dùng bấm.
- Filter UI được build thành AST `Filter`, rồi `Dialect` render ra SQL với **param binding** (không nối chuỗi).

### 5.3 Ad-hoc query

- `sql::split` tách script thành statement. "Run" chạy statement tại cursor, "Run all" chạy hết.
- Mỗi statement nhận ≤ `limit` dòng (mặc định 1000, có nút "fetch more"). Stream được dừng giữa chừng nhờ backpressure.
- Trước khi chạy, `SafetyGuard` gọi `sql::classify`. Xem 5.5.

### 5.4 Inline edit → ChangeSet

```rust
pub struct ChangeSet {
    pub table: TableRef,
    pub key: RowKey,                     // PK, hoặc unique NOT NULL; không có → read-only
    pub ops: Vec<RowOp>,                 // Update{key_vals, col→(old,new)} | Insert{..} | Delete{key_vals}
}
```

1. Grid ghi lại edit vào `PendingEdits` ở phía UI (undo/redo diễn ra ngay ở đây, không round-trip).
2. Bấm "Review" → `ApplyChangeSet{dry_run:true}` → engine sinh SQL và gửi lại `ChangeSetPreview`.
3. Bấm "Commit" → `BEGIN` → chạy từng câu với param → `COMMIT`. Câu nào lỗi thì `ROLLBACK` toàn bộ và chỉ ra dòng gây lỗi.
4. Tuỳ chọn **optimistic check**: `WHERE pk=$1 AND col=$old`. Nếu affected=0 thì báo "dòng đã bị người khác sửa".
5. Kết quả không gắn được với một bảng có key (join, aggregate) thì grid ở chế độ read-only. Engine biết điều này nhờ `ColumnMeta.source_table/source_column`; pg trả thông tin này qua `table_oid`/`column_id`.

### 5.5 Safe mode

| Environment | Hành vi |
|---|---|
| `local` | Không hỏi gì |
| `staging` | Confirm với `DELETE`/`UPDATE` không có `WHERE`, `DROP`, `TRUNCATE` |
| `prod` | Như staging, cộng thêm confirm **mọi** câu ghi; có tuỳ chọn read-only (`SET SESSION ... READ ONLY` / sqlite `SQLITE_OPEN_READ_ONLY`); viền cửa sổ đỏ |

Confirm là một vòng `ConfirmRequired → UI → Command::Confirm{token}`, nên quyết định nằm ở engine, không nằm ở UI. CLI/MCP sau này cũng được bảo vệ y như vậy.

---

## 6. Lưu trữ (XDG)

| Dữ liệu | Vị trí | Định dạng |
|---|---|---|
| Connections, groups, settings | `$XDG_CONFIG_HOME/quarry/connections.toml`, `settings.toml` | TOML, **không có secret** |
| Password, SSH passphrase | Keyring, key = `quarry/<conn_uuid>/<kind>` | `oo7` (Secret portal trong Flatpak, Secret Service ngoài Flatpak) |
| History, saved queries | `$XDG_DATA_HOME/quarry/quarry.db` | SQLite + FTS5, migrate bằng `rusqlite_migration` |
| Schema cache (tuỳ chọn) | `$XDG_CACHE_HOME/quarry/` | bincode/json, xoá thoải mái |
| Session (các tab đang mở) | `$XDG_STATE_HOME/quarry/session.json` | JSON |

Có `schema_version` trong mọi file config để migrate. Đây cũng là chuẩn bị cho tính năng Sync (Pro) sau này: mỗi entity có `id: Uuid` và `updated_at`.

---

## 7. UI (crate `app`)

**Bố cục:** `AdwApplicationWindow` → `AdwOverlaySplitView`

- **Sidebar:** danh sách connection theo group, cây schema (`GtkListView` + `GtkTreeListModel`, load lười).
- **Content:** `AdwTabView` + `AdwTabBar`. Mỗi tab là `QueryTab` hoặc `TableTab`.
  - `QueryTab` = `GtkPaned` dọc: `SqlEditor` ở trên, `ResultsPane` ở dưới (nhiều kết quả thì chia tab con, có log/message).
  - `TableTab` = thanh filter + `DataGrid` + thanh trạng thái (số dòng, pending edits, nút Review/Commit).
- **Dialog:** ConnectionDialog (`AdwPreferencesDialog`), ChangeSetReview, Confirm, Export/Import.

### 7.1 `DataGrid`: widget tự vẽ (rủi ro kỹ thuật lớn nhất → làm spike ở tuần 3)

`GtkColumnView` không đáp ứng được: edit theo cell, chọn vùng, hiệu năng với hàng trăm cột. Nên làm widget riêng:

- Subclass `gtk::Widget` + implement interface `gtk::Scrollable` (h/v adjustment), đặt trong `GtkScrolledWindow`.
- **Chiều cao dòng cố định**, nên từ scroll offset tính ra ngay khoảng dòng đang hiển thị, O(1).
- `snapshot()`: chỉ vẽ cell đang hiển thị. Nền và lưới vẽ bằng `append_color`, chữ bằng `append_layout` (Pango). Cache `pango::Layout` theo `(row, col)` trong cửa sổ hiển thị (LRU).
- Header là widget con riêng (resize/reorder cột, click để sort, thực chất là gửi `BrowseTable` với sort mới).
- **Edit:** khi double-click hoặc Enter, đặt một `GtkEntry` (hoặc `GtkTextView` cho text dài/JSON) làm widget con đúng vị trí cell, commit vào `PendingEdits`.
- Selection: cell, dòng, vùng chữ nhật; copy ra TSV, CSV hoặc `INSERT`.
- Màu cho cell đã sửa/dòng mới/dòng bị xoá; hiển thị `NULL` mờ.
- **Data source** là trait để grid không biết nguồn dữ liệu:

```rust
pub trait GridSource {
    fn columns(&self) -> &[ColumnMeta];
    fn row_count(&self) -> RowCount;                 // Known(n) | AtLeast(n)
    fn cell(&self, row: usize, col: usize) -> CellRef<'_>; // Loaded(&Value) | Pending
    fn request_range(&self, rows: Range<usize>);     // trigger FetchMore / page load
}
```

  - `PagedSource` (cho browse table): mỗi trang 500 dòng, giữ tối đa N trang theo LRU.
  - `StreamSource` (cho ad-hoc): append-only, có giới hạn bộ nhớ.

**Tiêu chí của spike:** 1 triệu dòng × 50 cột dữ liệu giả, scroll đạt 60 fps trên Wayland, RAM < 300 MB. Không đạt thì phải xem lại hướng tiếp cận trước khi đi tiếp.

### 7.2 `SqlEditor`

- `sourceview5::View` dùng language `sql` có sẵn. Thêm style scheme sáng/tối theo `AdwStyleManager`.
- Autocomplete: implement `CompletionProvider`. Ngữ cảnh lấy từ `sql::completion_context(text, cursor)`, cho ra keyword, schema, table, column (resolve alias trong `FROM`/`JOIN` ở mức cơ bản), dữ liệu lấy từ `SchemaCache`.
- Gạch chân statement đang ở cursor; `Ctrl+Enter` chạy statement đó, `Ctrl+Shift+Enter` chạy tất cả; Format bằng `sqlformat`.
- Vim mode: sourceview5 có `VimIMContext`, gần như free.

---

## 8. Cấu trúc repo

> Đây là cấu trúc **đích** của MVP. Skeleton hiện tại đã có `model`, `driver-api`, `sql`, `store`, `tunnel`, `engine`, `app` (chưa có `drivers/`). Tài liệu nằm trong `docs/` theo [docs/README.md](../README.md).

```
quarry/
├── Cargo.toml                      # [workspace], [workspace.dependencies], lints
├── rust-toolchain.toml
├── deny.toml                       # cargo-deny: license + advisory
├── LICENSE                         # AGPL-3.0-or-later (xem §10)
├── CONTRIBUTING.md                 # kèm CLA / DCO
├── ARCHITECTURE.md
├── crates/
│   ├── model/
│   │   └── src/{lib.rs, value.rs, column.rs, schema.rs, connection.rs, env.rs, filter.rs}
│   ├── driver-api/
│   │   └── src/{lib.rs, driver.rs, session.rs, dialect.rs, capabilities.rs, error.rs, registry.rs}
│   ├── drivers/
│   │   ├── postgres/src/{lib.rs, connect.rs, exec.rs, decode.rs, introspect.rs, dialect.rs}
│   │   ├── mysql/src/{...cùng layout}
│   │   └── sqlite/src/{...cùng layout}
│   ├── tunnel/
│   │   └── src/{lib.rs, forward.rs, auth.rs, known_hosts.rs, ssh_config.rs}
│   ├── sql/
│   │   ├── src/{lib.rs, lexer.rs, split.rs, classify.rs, completion.rs, format.rs}
│   │   └── tests/snapshots/        # insta
│   ├── store/
│   │   ├── src/{lib.rs, paths.rs, config.rs, secrets.rs, history.rs}
│   │   └── migrations/
│   ├── engine/
│   │   └── src/{lib.rs, handle.rs, command.rs, event.rs,
│   │            connection_manager.rs, session_actor.rs, executor.rs,
│   │            schema_cache.rs, changeset.rs, safety.rs, history.rs, export.rs, import.rs}
│   └── app/                        # binary
│       ├── build.rs                # compile gresource
│       └── src/
│           ├── main.rs             # init adw, tracing, spawn engine
│           ├── app.rs              # root Relm4 component, actions, shortcuts
│           ├── bridge.rs           # Event → component messages
│           ├── window/{mod.rs, sidebar.rs, tabs.rs}
│           ├── components/
│           │   ├── connection_dialog.rs
│           │   ├── schema_tree.rs
│           │   ├── query_tab.rs
│           │   ├── table_tab.rs
│           │   ├── results_pane.rs
│           │   ├── filter_bar.rs
│           │   ├── changeset_review.rs
│           │   └── confirm_dialog.rs
│           └── widgets/
│               ├── data_grid/{mod.rs, imp.rs, source.rs, layout.rs, render.rs, edit.rs, selection.rs}
│               └── sql_editor/{mod.rs, completion.rs, statement_marker.rs}
├── data/
│   ├── io.github.mindulabs.Quarry.desktop
│   ├── io.github.mindulabs.Quarry.metainfo.xml   # bắt buộc cho Flathub
│   ├── io.github.mindulabs.Quarry.gschema.xml    # kích thước cửa sổ, ...
│   ├── icons/hicolor/scalable/apps/
│   └── resources/{style.css, icons/, resources.gresource.xml}
├── build-aux/
│   └── flatpak/
│       ├── io.github.mindulabs.Quarry.json       # GNOME runtime + rust-stable SDK extension
│       └── cargo-sources.json                # flatpak-cargo-generator
├── tests/
│   ├── docker-compose.yml          # pg 13/16/17, mysql 8, mariadb 11 + sshd
│   └── fixtures/                   # schema mẫu: mọi kiểu dữ liệu, bảng không PK, ...
├── benches/
│   └── grid_source.rs
└── .github/workflows/
    ├── ci.yml                      # fmt, clippy -D warnings, test, cargo-deny
    ├── integration.yml             # docker services → driver tests
    └── flatpak.yml                 # build bundle .flatpak làm artifact
```

**Flatpak finish-args cần có:**

```
--share=network  --share=ipc  --socket=wayland  --socket=fallback-x11  --device=dri
--socket=ssh-auth                 # ssh-agent
--filesystem=~/.ssh:ro            # key, config, known_hosts
```

File SQLite mở qua `GtkFileDialog` (portal), nên không cần `--filesystem=home`.

**Lint workspace:** `unsafe_code = "forbid"` (riêng crate `app` cho phép nếu subclass cần), `clippy::unwrap_used = "deny"` cho các crate ngoài `app`.

---

## 9. Chiến lược test

| Tầng | Cách test |
|---|---|
| `model`, `sql` | Unit + snapshot (`insta`) cho split, classify, completion context, render filter |
| `engine/changeset` | Snapshot SQL sinh ra theo từng dialect |
| `drivers/*` | Integration với `docker-compose` (CI service containers). Một bộ **conformance test** dùng chung, chạy trên cả 3 driver: mọi kiểu dữ liệu round-trip, cancel, transaction, introspect |
| `tunnel` | Container `sshd`: thử auth bằng key, agent, password và trường hợp host key sai |
| `data_grid` | Bench `GridSource` + đo fps thủ công theo checklist; test logic `selection`/`edit` không cần GTK |
| UI | Smoke test chạy headless (`GDK_BACKEND=broadway` hoặc dưới `weston --backend=headless`) |

Bộ conformance test chính là "spec" cho plugin driver sau này.

---

## 10. Chuẩn bị cho Pro (làm ngay từ đầu, rẻ)

Mô hình đã chốt: **1 repo public, AGPL-3.0-or-later**, code Pro nằm chung repo, khoá bằng license key (giống TablePro). Chi tiết ở `docs/product/BUSINESS-MODEL.md`.

- **CLA từ PR đầu tiên** (CLA Assistant): giữ quyền relicense / tách `pro/` về sau.
- `cargo-deny` chặn dependency GPL/AGPL của bên thứ ba.
- Trait `Entitlements` trong `engine` (`fn has(&self, f: Feature) -> bool`). MVP luôn trả `false` cho feature Pro.
- License payload ký **Ed25519**, public key nhúng trong app, xác thực offline; kích hoạt/đếm máy qua server (v1.0).
- Mọi entity có `Uuid` + `updated_at`, sẵn cho Sync (nguồn thu chính).

---

## 11. Lộ trình MVP (~16 tuần, 1 người full-time)

| Tuần | Milestone | Tiêu chí xong |
|---|---|---|
| 1–2 | **M0 khung:** workspace, CI, Flatpak chạy được cửa sổ trống, `model` | Cài bản `.flatpak` từ CI chạy được |
| 3–4 | **M1 spike rủi ro:** `DataGrid` với 1M dòng giả; `driver-api` + sqlite | Đạt tiêu chí perf ở §7.1 |
| 5–6 | **M2 driver:** postgres + mysql, conformance test xanh, cancel chạy được | Bộ conformance pass trên CI |
| 7–8 | **M3 connection:** dialog, keyring, SSH tunnel, known_hosts, schema tree | Kết nối được pg qua bastion bằng ssh-agent |
| 9–10 | **M4 query:** editor, split/run tại cursor, multi-result, history FTS | Chạy được script 50 câu, cancel giữa chừng |
| 11–12 | **M5 browse:** filter bar, sort, paging, PagedSource | Browse mượt bảng 10M dòng |
| 13–14 | **M6 edit:** PendingEdits, undo/redo, ChangeSet review/commit, optimistic check | Sửa, thêm, xoá dòng an toàn trên cả 3 DB |
| 15 | **M7:** safe mode, export/import CSV/JSON/SQL | Prod chặn được `DELETE` không có `WHERE` |
| 16 | **M8:** polish, metainfo, screenshot, gửi lên Flathub | PR lên flathub/flathub |

**Thứ tự ưu tiên khi bị trễ:** cắt import CSV và mysql trước. **Không cắt** ChangeSet review và safe mode, vì đó là lý do người ta tin dùng app trên prod.
