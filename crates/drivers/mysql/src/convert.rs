//! Chuyển đổi giữa kiểu của `mysql_async` và [`quarry_model::Value`].
//!
//! Query thường chạy bằng text protocol nên mọi giá trị (trừ NULL) về dạng bytes;
//! kiểu thật lấy từ metadata của cột.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::{Column, Value as MyValue};
use quarry_model::{ColumnMeta, Value};

/// Charset id 63 = `binary`.
const BINARY_CHARSET: u16 = 63;

pub fn column_meta(column: &Column) -> ColumnMeta {
    let table = column.org_table_str();
    let name = column.org_name_str();
    ColumnMeta {
        name: column.name_str().into_owned(),
        type_name: type_name(column),
        nullable: Some(!column.flags().contains(ColumnFlags::NOT_NULL_FLAG)),
        source_table: (!table.is_empty()).then(|| table.into_owned()),
        source_column: (!name.is_empty()).then(|| name.into_owned()),
    }
}

fn is_unsigned(column: &Column) -> bool {
    column.flags().contains(ColumnFlags::UNSIGNED_FLAG)
}

fn is_binary(column: &Column) -> bool {
    column.character_set() == BINARY_CHARSET
}

pub fn type_name(column: &Column) -> String {
    use ColumnType::*;
    let base = match column.column_type() {
        MYSQL_TYPE_TINY => "TINYINT",
        MYSQL_TYPE_SHORT => "SMALLINT",
        MYSQL_TYPE_INT24 => "MEDIUMINT",
        MYSQL_TYPE_LONG => "INT",
        MYSQL_TYPE_LONGLONG => "BIGINT",
        MYSQL_TYPE_FLOAT => "FLOAT",
        MYSQL_TYPE_DOUBLE => "DOUBLE",
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => "DATE",
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => "TIME",
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => "DATETIME",
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        MYSQL_TYPE_YEAR => "YEAR",
        MYSQL_TYPE_BIT => "BIT",
        MYSQL_TYPE_JSON => "JSON",
        MYSQL_TYPE_ENUM => "ENUM",
        MYSQL_TYPE_SET => "SET",
        MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB => {
            if is_binary(column) {
                "BLOB"
            } else {
                "TEXT"
            }
        }
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => {
            if is_binary(column) {
                "VARBINARY"
            } else {
                "VARCHAR"
            }
        }
        MYSQL_TYPE_STRING => {
            // ENUM/SET được server báo là STRING kèm flag.
            if column.flags().contains(ColumnFlags::ENUM_FLAG) {
                "ENUM"
            } else if column.flags().contains(ColumnFlags::SET_FLAG) {
                "SET"
            } else if is_binary(column) {
                "BINARY"
            } else {
                "CHAR"
            }
        }
        MYSQL_TYPE_NULL => "NULL",
        _ => "UNKNOWN",
    };
    if is_unsigned(column) {
        format!("{base} UNSIGNED")
    } else {
        base.to_owned()
    }
}

pub fn value(raw: MyValue, column: &Column) -> Value {
    use ColumnType::*;
    let bytes = match raw {
        MyValue::NULL => return Value::Null,
        MyValue::Bytes(bytes) => bytes,
        // Binary protocol (prepared statement) trả kiểu có sẵn.
        MyValue::Int(v) => return Value::Int(v),
        MyValue::UInt(v) => return Value::UInt(v),
        MyValue::Float(v) => return Value::Float(f64::from(v)),
        MyValue::Double(v) => return Value::Float(v),
        other => return Value::Text(other.as_sql(true).trim_matches('\'').to_owned()),
    };

    match column.column_type() {
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG
        | MYSQL_TYPE_LONGLONG | MYSQL_TYPE_YEAR => {
            let text = lossy(&bytes);
            let parsed = if is_unsigned(column) {
                text.parse().ok().map(Value::UInt)
            } else {
                text.parse().ok().map(Value::Int)
            };
            parsed.unwrap_or(Value::Text(text))
        }
        MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE => {
            let text = lossy(&bytes);
            text.parse().map(Value::Float).unwrap_or(Value::Text(text))
        }
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => Value::Decimal(lossy(&bytes)),
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => {
            let text = lossy(&bytes);
            NaiveDate::parse_from_str(&text, "%Y-%m-%d")
                .map(Value::Date)
                .unwrap_or(Value::Text(text))
        }
        MYSQL_TYPE_DATETIME
        | MYSQL_TYPE_DATETIME2
        | MYSQL_TYPE_TIMESTAMP
        | MYSQL_TYPE_TIMESTAMP2 => {
            let text = lossy(&bytes);
            // '0000-00-00 00:00:00' không parse được → giữ nguyên dạng text.
            NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S%.f")
                .map(Value::Timestamp)
                .unwrap_or(Value::Text(text))
        }
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => {
            let text = lossy(&bytes);
            // TIME của MySQL có thể âm hoặc > 24h ('838:59:59').
            NaiveTime::parse_from_str(&text, "%H:%M:%S%.f")
                .map(Value::Time)
                .unwrap_or(Value::Text(text))
        }
        MYSQL_TYPE_JSON => Value::Json(lossy(&bytes)),
        MYSQL_TYPE_BIT | MYSQL_TYPE_GEOMETRY => Value::Bytes(bytes),
        MYSQL_TYPE_TINY_BLOB
        | MYSQL_TYPE_MEDIUM_BLOB
        | MYSQL_TYPE_LONG_BLOB
        | MYSQL_TYPE_BLOB
        | MYSQL_TYPE_VARCHAR
        | MYSQL_TYPE_VAR_STRING
        | MYSQL_TYPE_STRING
            if is_binary(column) =>
        {
            Value::Bytes(bytes)
        }
        _ => match String::from_utf8(bytes) {
            Ok(text) => Value::Text(text),
            Err(err) => Value::Bytes(err.into_bytes()),
        },
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Chuyển [`Value`] thành tham số cho prepared statement.
pub fn to_mysql(value: &Value) -> MyValue {
    match value {
        Value::Null => MyValue::NULL,
        Value::Bool(b) => MyValue::Int(i64::from(*b)),
        Value::Int(v) => MyValue::Int(*v),
        Value::UInt(v) => MyValue::UInt(*v),
        Value::Float(v) => MyValue::Double(*v),
        Value::Bytes(b) => MyValue::Bytes(b.clone()),
        Value::Date(d) => MyValue::Bytes(d.to_string().into_bytes()),
        Value::Time(t) => MyValue::Bytes(t.to_string().into_bytes()),
        Value::Timestamp(t) => {
            MyValue::Bytes(t.format("%Y-%m-%d %H:%M:%S%.f").to_string().into_bytes())
        }
        Value::TimestampTz(t) => MyValue::Bytes(
            t.naive_utc()
                .format("%Y-%m-%d %H:%M:%S%.f")
                .to_string()
                .into_bytes(),
        ),
        Value::Uuid(u) => MyValue::Bytes(u.to_string().into_bytes()),
        Value::Decimal(s) | Value::Text(s) | Value::Json(s) => {
            MyValue::Bytes(s.clone().into_bytes())
        }
        Value::Other { text, .. } => MyValue::Bytes(text.clone().into_bytes()),
        Value::Array(_) => MyValue::Bytes(value.display(usize::MAX).into_bytes()),
    }
}
