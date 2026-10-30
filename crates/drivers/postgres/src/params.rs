//! Parameters sent in the text format, so the server reads each one as the type the statement
//! needs. A grid edit holds a `Value::Text("42")` for an `int4` column; in the binary format it
//! would have to be converted to that type first, which the driver does not know.

use std::fmt::Write as _;

use bytes::{BufMut, BytesMut};
use quarry_model::Value;
use tokio_postgres::types::{Format, IsNull, ToSql, Type, to_sql_checked};

#[derive(Debug)]
pub struct TextParam<'a>(pub &'a Value);

impl ToSql for TextParam<'_> {
    fn to_sql(
        &self,
        _: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        if matches!(self.0, Value::Null) {
            return Ok(IsNull::Yes);
        }
        out.put_slice(text(self.0).as_bytes());
        Ok(IsNull::No)
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    fn encode_format(&self, _: &Type) -> Format {
        Format::Text
    }

    to_sql_checked!();
}

/// `value` as PostgreSQL reads it in a parameter or an array literal.
pub fn text(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Bool(b) => if *b { "t" } else { "f" }.to_owned(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) if v.is_nan() => "NaN".to_owned(),
        Value::Float(v) if v.is_infinite() => {
            if *v > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
        }
        Value::Float(v) => v.to_string(),
        Value::Decimal(text) | Value::Text(text) | Value::Json(text) => text.clone(),
        Value::Other { text, .. } => text.clone(),
        Value::Bytes(bytes) => {
            let mut out = String::with_capacity(2 + bytes.len() * 2);
            out.push_str("\\x");
            for byte in bytes {
                let _ = write!(out, "{byte:02x}");
            }
            out
        }
        Value::Date(date) => date.format("%Y-%m-%d").to_string(),
        Value::Time(time) => time.format("%H:%M:%S%.f").to_string(),
        Value::Timestamp(at) => at.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
        Value::TimestampTz(at) => at.to_rfc3339(),
        Value::Uuid(uuid) => uuid.to_string(),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(array_item).collect();
            format!("{{{}}}", items.join(","))
        }
    }
}

/// An element of an array literal: `NULL`, a nested array, or a double-quoted string.
fn array_item(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Array(_) => text(value),
        other => format!(
            "\"{}\"",
            text(other).replace('\\', "\\\\").replace('"', "\\\"")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_back_as_postgres_text() {
        assert_eq!(text(&Value::Bool(true)), "t");
        assert_eq!(text(&Value::UInt(u64::MAX)), "18446744073709551615");
        assert_eq!(text(&Value::Float(f64::NEG_INFINITY)), "-Infinity");
        assert_eq!(text(&Value::Bytes(vec![0, 255])), "\\x00ff");
        assert_eq!(
            text(&Value::Array(vec![
                Value::Text("a\"b".into()),
                Value::Null,
                Value::Array(vec![Value::Int(1)]),
            ])),
            r#"{"a\"b",NULL,{"1"}}"#
        );
    }
}
