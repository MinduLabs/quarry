use serde::{Deserialize, Serialize};

/// Metadata của một cột trong result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    /// Tên kiểu theo DB (`int4`, `varchar(255)`, …).
    pub type_name: String,
    pub nullable: Option<bool>,
    /// Bảng/cột gốc nếu driver biết — dùng để quyết định cột có sửa được không.
    pub source_table: Option<String>,
    pub source_column: Option<String>,
}
