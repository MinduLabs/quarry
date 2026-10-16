use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Một giá trị trong ô dữ liệu.
///
/// Enum đóng, có nhánh `Other` làm fallback để grid luôn hiển thị được
/// kiểu lạ (enum, geometry, inet, range…) thay vì lỗi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    /// Giữ nguyên chuỗi để không mất độ chính xác.
    Decimal(String),
    Text(String),
    Bytes(Vec<u8>),
    Date(NaiveDate),
    Time(NaiveTime),
    Timestamp(NaiveDateTime),
    TimestampTz(DateTime<FixedOffset>),
    Uuid(Uuid),
    Json(String),
    Array(Vec<Value>),
    Other {
        type_name: String,
        text: String,
    },
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Giá trị số (căn phải trong grid).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Value::Int(_) | Value::UInt(_) | Value::Float(_) | Value::Decimal(_)
        )
    }

    /// Chuỗi hiển thị một dòng trong grid. Text dài bị cắt ở `max_chars` ký tự.
    pub fn display(&self, max_chars: usize) -> String {
        let text = match self {
            Value::Null => return "NULL".to_owned(),
            Value::Bool(b) => return if *b { "true" } else { "false" }.to_owned(),
            Value::Int(v) => return v.to_string(),
            Value::UInt(v) => return v.to_string(),
            Value::Float(v) => return v.to_string(),
            Value::Decimal(s) | Value::Text(s) | Value::Json(s) => s.clone(),
            Value::Bytes(b) => return bytes_preview(b),
            Value::Date(d) => return d.to_string(),
            Value::Time(t) => return t.to_string(),
            Value::Timestamp(t) => return t.to_string(),
            Value::TimestampTz(t) => return t.to_rfc3339(),
            Value::Uuid(u) => return u.to_string(),
            Value::Array(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.display(max_chars)).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Other { text, .. } => text.clone(),
        };
        one_line(&text, max_chars)
    }
}

fn bytes_preview(bytes: &[u8]) -> String {
    const SHOWN: usize = 16;
    let hex: String = bytes
        .iter()
        .take(SHOWN)
        .map(|b| format!("{b:02X}"))
        .collect();
    if bytes.len() > SHOWN {
        format!("0x{hex}… ({} bytes)", bytes.len())
    } else {
        format!("0x{hex}")
    }
}

fn one_line(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max_chars + 1));
    for (i, ch) in text.chars().enumerate() {
        if i == max_chars {
            out.push('…');
            break;
        }
        out.push(if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_single_line_and_truncated() {
        assert_eq!(Value::Text("a\nb".into()).display(10), "a b");
        assert_eq!(Value::Text("abcdef".into()).display(3), "abc…");
        assert_eq!(Value::Null.display(10), "NULL");
        assert_eq!(Value::Bytes(vec![0xDE, 0xAD]).display(10), "0xDEAD");
    }

    #[test]
    fn roundtrips_through_json() {
        let values = vec![
            Value::Null,
            Value::Int(-42),
            Value::Decimal("12345678901234567890.123".into()),
            Value::Array(vec![Value::Bool(true), Value::Text("x".into())]),
            Value::Other {
                type_name: "inet".into(),
                text: "10.0.0.1/8".into(),
            },
        ];
        let json = serde_json::to_string(&values).unwrap();
        let back: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(values, back);
    }
}
