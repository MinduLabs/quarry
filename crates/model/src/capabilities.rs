use serde::{Deserialize, Serialize};

/// Loại object trong cây schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Driver khai báo nó làm được gì; UI bật/tắt tính năng theo đây
/// thay vì `if driver == "postgres"`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub schemas: bool,
    pub transactions: bool,
    pub editable_results: bool,
    pub explain: bool,
    pub cancel: bool,
    pub ssh: bool,
    pub ssl: bool,
    pub object_kinds: Vec<ObjectKind>,
}
