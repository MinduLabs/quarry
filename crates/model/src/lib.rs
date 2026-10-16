//! Kiểu dữ liệu thuần dùng chung cho mọi crate của Quarry.
//!
//! Quy tắc: không IO, không async, mọi type đều `serde` được — để sau này
//! driver chạy ngoài process (JSON-RPC) mà không phải đổi model.

mod capabilities;
mod column;
mod connection;
mod environment;
mod schema;
mod value;

pub use capabilities::{Capabilities, ObjectKind};
pub use column::ColumnMeta;
pub use connection::{ConnId, ConnectionConfig, DriverId, SshConfig, SslMode};
pub use environment::Environment;
pub use schema::{SchemaObject, TableRef};
pub use value::Value;
