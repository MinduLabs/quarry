//! Plain data types shared by every Quarry crate.
//!
//! Rules: no IO, no async. Every type is `serde`-serializable and exported to TypeScript
//! (`ts-rs`), because the same types travel to the UI and, later, to out-of-process drivers.

mod capabilities;
mod column;
mod connection;
mod edit;
mod environment;
mod favorite;
mod history;
mod schema;
mod value;
mod workspace;

pub use capabilities::{Capabilities, ObjectKind};
pub use column::ColumnMeta;
pub use connection::{ConnId, ConnectionConfig, DriverId, SshAuth, SshConfig, SslMode};
pub use edit::{CellValue, RowChange};
pub use environment::Environment;
pub use favorite::{Favorite, FavoriteId, FavoriteItem};
pub use history::HistoryEntry;
pub use schema::{ColumnInfo, ForeignKey, IndexInfo, SchemaObject, TableInfo, TableRef};
pub use value::Value;
pub use workspace::{BrowsePage, SavedTab, Workspace};
