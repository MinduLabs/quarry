use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Kind of object in the schema tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ObjectKind {
    Database,
    Schema,
    Table,
    View,
    MaterializedView,
    Function,
    Procedure,
    Trigger,
    Sequence,
    Index,
}

/// What a driver supports. Clients enable features from this instead of checking the driver id.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Capabilities {
    pub schemas: bool,
    pub transactions: bool,
    pub editable_results: bool,
    pub explain: bool,
    /// Tables, columns, indexes and databases can be created, changed and dropped through
    /// `SchemaChange`.
    #[serde(default)]
    pub schema_edit: bool,
    pub cancel: bool,
    pub ssh: bool,
    pub ssl: bool,
    pub object_kinds: Vec<ObjectKind>,
}
