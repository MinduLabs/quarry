//! Plain data types shared by every Quarry crate.
//!
//! Rules: no IO, no async. Every type is `serde`-serializable and exported to TypeScript
//! (`ts-rs`), because the same types travel to the UI and, later, to out-of-process drivers.

mod browse;
mod capabilities;
mod column;
mod completion;
mod connection;
mod data_source;
mod ddl;
mod edit;
mod environment;
mod error_location;
mod export;
mod favorite;
mod history;
mod schema;
mod server_info;
mod value;
mod workspace;

pub use browse::{BrowseQuery, ColumnFilter, FilterOp, SortKey};
pub use capabilities::{Capabilities, ObjectKind};
pub use column::ColumnMeta;
pub use completion::{CompletionContext, CompletionNames, StatementTable};
pub use connection::{ConnId, ConnectionConfig, DriverId, SshAuth, SshConfig, SslMode};
pub use data_source::{DataSource, DataSourceSection};
pub use ddl::{AlterOp, ColumnDef, IndexDef, SchemaChange};
pub use edit::{CellValue, RowChange};
pub use environment::Environment;
pub use error_location::ErrorLocation;
pub use export::{ExportFormat, ExportOptions};
pub use favorite::{Favorite, FavoriteId, FavoriteItem};
pub use history::HistoryEntry;
pub use schema::{
    ColumnInfo, ForeignKey, IndexInfo, SchemaObject, TableColumns, TableInfo, TableRef,
};
pub use server_info::{ServerInfoKind, ServerInfoRow};
pub use value::Value;
pub use workspace::{BrowsePage, SavedTab, Workspace};
