//! Contract mà mọi driver database phải implement.
//!
//! Engine chỉ nói chuyện với `dyn Driver` / `dyn Session`; UI không bao giờ
//! biết driver cụ thể. Mọi type đi qua ranh giới này đều serialize được để
//! sau này chuyển driver ra plugin ngoài process.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use quarry_model::{Capabilities, ColumnMeta, ConnectionConfig, DriverId, SchemaObject, Value};
use serde::{Deserialize, Serialize};

pub type Result<T, E = DriverError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("authentication failed")]
    Auth,
    #[error("query failed: {message}")]
    Query {
        message: String,
        position: Option<usize>,
    },
    #[error("query was cancelled")]
    Cancelled,
    #[error("not supported by this driver: {0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Cấu hình đã resolve: secret đã lấy từ keyring, host/port đã trỏ vào SSH tunnel nếu có.
#[derive(Clone)]
pub struct ResolvedConfig {
    pub config: ConnectionConfig,
    pub password: Option<String>,
}

// Không bao giờ in password ra log.
impl std::fmt::Debug for ResolvedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedConfig")
            .field("config", &self.config)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecOpts {
    /// Số dòng tối đa giữ lại cho mỗi result set (`None` = không giới hạn, chỉ dùng cho export).
    /// Dòng vượt giới hạn vẫn được đọc hết khỏi socket nhưng không gửi đi.
    pub row_limit: Option<u64>,
    /// Số dòng mỗi batch gửi qua `sink`.
    pub batch_size: usize,
}

impl Default for ExecOpts {
    fn default() -> Self {
        Self {
            row_limit: Some(1000),
            batch_size: 500,
        }
    }
}

/// Một phần tử kết quả, phát ra theo thứ tự qua `sink` của [`Session::execute`].
/// Một script có thể sinh nhiều result set: mỗi set bắt đầu bằng `Columns`
/// (hoặc chỉ có `Affected` với câu lệnh không trả dòng).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecItem {
    Columns(Vec<ColumnMeta>),
    Rows(Vec<Vec<Value>>),
    /// Kết thúc một result set.
    SetDone {
        affected: u64,
        rows: u64,
        truncated: bool,
    },
    Notice(String),
    Done {
        elapsed: Duration,
    },
}

/// Nơi nhận kết quả. Gọi đồng bộ, nên implementation phải nhanh (vd đẩy vào channel).
pub type ExecSink<'a> = &'a mut (dyn FnMut(ExecItem) + Send);

/// Huỷ query đang chạy, gọi được từ task khác.
pub trait CancelHandle: Send + Sync {
    fn cancel(&self);
}

pub type SharedCancel = Arc<dyn CancelHandle>;

/// Khác biệt cú pháp giữa các DB.
pub trait Dialect: Send + Sync {
    fn quote_ident(&self, ident: &str) -> String;
    /// Placeholder cho tham số thứ `index` (bắt đầu từ 1): `$1` hoặc `?`.
    fn placeholder(&self, index: usize) -> String;
    /// `SELECT * FROM <table> LIMIT .. OFFSET ..`
    fn select_page(&self, table: &quarry_model::TableRef, limit: u64, offset: u64) -> String;
}

#[async_trait]
pub trait Driver: Send + Sync + 'static {
    fn id(&self) -> DriverId;
    fn capabilities(&self) -> Capabilities;
    fn dialect(&self) -> &dyn Dialect;
    async fn connect(&self, config: &ResolvedConfig) -> Result<Box<dyn Session>>;
}

#[async_trait]
pub trait Session: Send {
    /// Phiên bản server, vd `8.4.3`.
    fn server_version(&self) -> String;
    /// Chạy một hoặc nhiều câu lệnh, phát kết quả qua `sink`.
    async fn execute(&mut self, sql: &str, opts: &ExecOpts, sink: ExecSink<'_>) -> Result<()>;
    async fn execute_params(&mut self, sql: &str, params: &[Value]) -> Result<u64>;
    async fn list_databases(&mut self) -> Result<Vec<String>>;
    async fn list_objects(&mut self, database: &str) -> Result<Vec<SchemaObject>>;
    /// Đổi database mặc định của session (`USE`).
    async fn use_database(&mut self, database: &str) -> Result<()>;
    async fn begin(&mut self) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
    async fn rollback(&mut self) -> Result<()>;
    async fn set_read_only(&mut self, read_only: bool) -> Result<()>;
    fn cancel_handle(&self) -> SharedCancel;
    async fn close(self: Box<Self>) -> Result<()>;
}

/// Danh sách driver có sẵn trong binary.
#[derive(Default)]
pub struct DriverRegistry {
    drivers: Vec<Box<dyn Driver>>,
}

impl std::fmt::Debug for DriverRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.drivers.iter().map(|d| d.id()))
            .finish()
    }
}

impl DriverRegistry {
    pub fn register(&mut self, driver: Box<dyn Driver>) {
        self.drivers.push(driver);
    }

    pub fn get(&self, id: &DriverId) -> Option<&dyn Driver> {
        self.drivers
            .iter()
            .find(|d| &d.id() == id)
            .map(|d| d.as_ref())
    }

    pub fn ids(&self) -> impl Iterator<Item = DriverId> + '_ {
        self.drivers.iter().map(|d| d.id())
    }
}
