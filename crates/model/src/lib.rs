//! Plain data types shared by every Quarry crate.
//!
//! Rules: no IO, no async. Every type is `serde`-serializable and exported to TypeScript
//! (`ts-rs`), because the same types travel to the UI and, later, to out-of-process drivers.

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
