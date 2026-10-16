use std::fmt;

use quarry_model::{ConnId, ConnectionConfig};

/// ID của một tab query do client cấp (mỗi tab có session riêng).
pub type TabId = u64;
/// ID do client cấp để ghép request với kết quả.
pub type RequestId = u64;

/// Password đi qua kênh lệnh. `Debug` không bao giờ in giá trị.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(pub String);

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(***)")
    }
}

/// Lệnh gửi từ client (UI, CLI, MCP) tới engine.
///
/// Danh sách đầy đủ dự kiến ở `docs/architecture/ARCHITECTURE.md` §4.
#[derive(Debug, Clone)]
pub enum Command {
    /// Kiểm tra kênh client ↔ engine còn sống.
    Ping {
        id: u64,
    },
    /// Đọc danh sách connection đã lưu → `Event::Connections`.
    LoadConnections,
    /// Thêm hoặc cập nhật connection (theo `config.id`).
    /// `password: None` = giữ password hiện có.
    SaveConnection {
        config: ConnectionConfig,
        password: Option<Password>,
        remember_password: bool,
    },
    DeleteConnection {
        id: ConnId,
    },
    /// Thử kết nối với cấu hình chưa lưu → `Event::TestResult`.
    TestConnection {
        request: RequestId,
        config: ConnectionConfig,
        password: Option<Password>,
    },
    /// Mở session metadata, đọc danh sách database.
    Connect {
        id: ConnId,
    },
    Disconnect {
        id: ConnId,
    },
    /// Đọc bảng/view/routine của một database → `Event::Objects`.
    LoadObjects {
        id: ConnId,
        database: String,
    },
    /// Chạy SQL trên session của tab (tự mở session nếu chưa có).
    /// Với môi trường staging/production, engine có thể trả `Event::ConfirmRequired`
    /// thay vì chạy; client hỏi người dùng rồi gửi lại với `confirmed: true`.
    Run {
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
        confirmed: bool,
    },
    Cancel {
        tab: TabId,
    },
    /// Đóng session của tab.
    CloseTab {
        tab: TabId,
    },
}
