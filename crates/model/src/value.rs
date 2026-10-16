use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// One cell value.
///
/// A closed enum with an `Other` fallback, so the grid can always show unusual types
/// (enum, geometry, inet, range, …) instead of failing.
///
/// JSON form: `{ "t": "<kind>", "v": <value> }`. 64-bit integers are sent as strings
/// (JavaScript numbers lose precision above 2^53) and bytes as a hex string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
#[ts(export)]
pub enum Value {
    Null,
    Bool(bool),
    Int(
        #[serde(with = "as_string")]
        #[ts(type = "string")]
        i64,
    ),
    UInt(
        #[serde(with = "as_string")]
        #[ts(type = "string")]
        u64,
    ),
    Float(f64),
    /// Kept as text to avoid losing precision.
    Decimal(String),
    Text(String),
    Bytes(
        #[serde(with = "as_hex")]
        #[ts(type = "string")]
        Vec<u8>,
    ),
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

/// Serialize integers as decimal strings.
mod as_string {
    use std::fmt::Display;
    use std::str::FromStr;

    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<T: Display, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(value)
    }

    pub fn deserialize<'de, T, D>(d: D) -> Result<T, D::Error>
    where
        T: FromStr,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

/// Serialize bytes as a lowercase hex string.
mod as_hex {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        s.serialize_str(&hex)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let hex = String::deserialize(d)?;
        if hex.len() % 2 != 0 {
            return Err(D::Error::custom("odd-length hex string"));
        }
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(D::Error::custom))
            .collect()
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Numeric value (right-aligned in the grid).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Value::Int(_) | Value::UInt(_) | Value::Float(_) | Value::Decimal(_)
        )
    }

    /// Single-line display text for the grid, cut at `max_chars` characters.
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
    fn big_integers_and_bytes_are_strings() {
        let json = serde_json::to_string(&Value::UInt(u64::MAX)).unwrap();
        assert_eq!(json, r#"{"t":"u_int","v":"18446744073709551615"}"#);
        let json = serde_json::to_string(&Value::Bytes(vec![0xde, 0xad])).unwrap();
        assert_eq!(json, r#"{"t":"bytes","v":"dead"}"#);
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
