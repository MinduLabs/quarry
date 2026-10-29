//! A batch of rows in a compact binary form, the form rows take between a driver process and
//! the engine. JSON there made a 1000-row query more than twice as slow. The batch stays in this
//! form up to the UI: serializing it writes each value straight from the bytes, as the same JSON
//! a [`Value`] serializes to, without building the values.
//!
//! Each value is a tag byte, then its payload, little-endian: lengths and counts are `u32`,
//! strings are UTF-8.

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use quarry_model::Value;
use serde::ser::{Error as _, SerializeSeq};
use serde::{Serialize, Serializer};

/// Arrays nest at most this deep, so a malformed batch cannot overflow the stack.
const MAX_DEPTH: usize = 32;

const NULL: u8 = 0;
const BOOL: u8 = 1;
const INT: u8 = 2;
const UINT: u8 = 3;
const FLOAT: u8 = 4;
const DECIMAL: u8 = 5;
const TEXT: u8 = 6;
const BYTES: u8 = 7;
const DATE: u8 = 8;
const TIME: u8 = 9;
const TIMESTAMP: u8 = 10;
const TIMESTAMP_TZ: u8 = 11;
const UUID: u8 = 12;
const JSON: u8 = 13;
const ARRAY: u8 = 14;
const OTHER: u8 = 15;

#[derive(Debug, thiserror::Error)]
#[error("malformed row batch: {0}")]
pub struct RowsError(&'static str);

/// Encoded rows. Checked when read, by [`Self::decode`] or [`Self::write_json`].
#[derive(Clone, PartialEq, Eq)]
pub struct RowBatch {
    bytes: Vec<u8>,
}

// The bytes are result values: never print them.
impl std::fmt::Debug for RowBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RowBatch({} rows)", self.len())
    }
}

impl RowBatch {
    pub fn encode(rows: &[Vec<Value>]) -> Self {
        let mut bytes = Vec::new();
        encode(rows, &mut bytes);
        Self { bytes }
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The row count the batch announces.
    pub fn len(&self) -> usize {
        self.bytes
            .get(..4)
            .and_then(|head| head.try_into().ok())
            .map_or(0, |head| u32::from_le_bytes(head) as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn decode(&self) -> Result<Vec<Vec<Value>>, RowsError> {
        decode(&self.bytes)
    }
}

/// The same JSON as the decoded `Vec<Vec<Value>>`, written from the bytes. A malformed batch
/// fails the serialization.
impl Serialize for RowBatch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut input = Input { bytes: &self.bytes };
        let count = input.len().map_err(S::Error::custom)?;
        let mut seq = serializer.serialize_seq(Some(count))?;
        for _ in 0..count {
            let cells = input.len().map_err(S::Error::custom)?;
            let mut row = Vec::with_capacity(cells.min(input.bytes.len()));
            for _ in 0..cells {
                row.push(input.value(0).map_err(S::Error::custom)?);
            }
            seq.serialize_element(&row)?;
        }
        if !input.bytes.is_empty() {
            return Err(S::Error::custom(RowsError("bytes after the last row")));
        }
        seq.end()
    }
}

fn encode(rows: &[Vec<Value>], out: &mut Vec<u8>) {
    put_len(out, rows.len());
    for row in rows {
        put_len(out, row.len());
        for value in row {
            put_value(out, value);
        }
    }
}

fn decode(bytes: &[u8]) -> Result<Vec<Vec<Value>>, RowsError> {
    let mut input = Input { bytes };
    let count = input.len()?;
    // Every row takes at least its 4-byte length, so a count beyond that is a lie.
    let mut rows = Vec::with_capacity(count.min(input.bytes.len() / 4));
    for _ in 0..count {
        let cells = input.len()?;
        let mut row = Vec::with_capacity(cells.min(input.bytes.len()));
        for _ in 0..cells {
            row.push(input.value(0)?.into_value());
        }
        rows.push(row);
    }
    if !input.bytes.is_empty() {
        return Err(RowsError("bytes after the last row"));
    }
    Ok(rows)
}

fn put_len(out: &mut Vec<u8>, len: usize) {
    // A batch is far below 4 GiB: `ExecOpts.batch_size` rows of bounded cells.
    out.extend_from_slice(&(len as u32).to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, text: &str) {
    put_len(out, text.len());
    out.extend_from_slice(text.as_bytes());
}

fn put_date(out: &mut Vec<u8>, date: NaiveDate) {
    out.extend_from_slice(&date.num_days_from_ce().to_le_bytes());
}

fn put_time(out: &mut Vec<u8>, time: NaiveTime) {
    out.extend_from_slice(&time.num_seconds_from_midnight().to_le_bytes());
    // Above 999_999_999 during a leap second.
    out.extend_from_slice(&time.nanosecond().to_le_bytes());
}

fn put_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.push(NULL),
        Value::Bool(b) => out.extend_from_slice(&[BOOL, u8::from(*b)]),
        Value::Int(v) => {
            out.push(INT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::UInt(v) => {
            out.push(UINT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Float(v) => {
            out.push(FLOAT);
            out.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        Value::Decimal(text) => {
            out.push(DECIMAL);
            put_str(out, text);
        }
        Value::Text(text) => {
            out.push(TEXT);
            put_str(out, text);
        }
        Value::Bytes(bytes) => {
            out.push(BYTES);
            put_len(out, bytes.len());
            out.extend_from_slice(bytes);
        }
        Value::Date(date) => {
            out.push(DATE);
            put_date(out, *date);
        }
        Value::Time(time) => {
            out.push(TIME);
            put_time(out, *time);
        }
        Value::Timestamp(at) => {
            out.push(TIMESTAMP);
            put_date(out, at.date());
            put_time(out, at.time());
        }
        Value::TimestampTz(at) => {
            out.push(TIMESTAMP_TZ);
            let utc = at.naive_utc();
            put_date(out, utc.date());
            put_time(out, utc.time());
            out.extend_from_slice(&at.offset().local_minus_utc().to_le_bytes());
        }
        Value::Uuid(uuid) => {
            out.push(UUID);
            out.extend_from_slice(uuid.as_bytes());
        }
        Value::Json(text) => {
            out.push(JSON);
            put_str(out, text);
        }
        Value::Array(items) => {
            out.push(ARRAY);
            put_len(out, items.len());
            for item in items {
                put_value(out, item);
            }
        }
        Value::Other { type_name, text } => {
            out.push(OTHER);
            put_str(out, type_name);
            put_str(out, text);
        }
    }
}

struct Input<'a> {
    bytes: &'a [u8],
}

impl<'a> Input<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], RowsError> {
        if self.bytes.len() < n {
            return Err(RowsError("cut short"));
        }
        let (head, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RowsError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn len(&mut self) -> Result<usize, RowsError> {
        Ok(u32::from_le_bytes(self.array()?) as usize)
    }

    fn str(&mut self) -> Result<&'a str, RowsError> {
        let len = self.len()?;
        std::str::from_utf8(self.take(len)?).map_err(|_| RowsError("text is not UTF-8"))
    }

    fn date(&mut self) -> Result<NaiveDate, RowsError> {
        NaiveDate::from_num_days_from_ce_opt(i32::from_le_bytes(self.array()?))
            .ok_or(RowsError("date out of range"))
    }

    fn time(&mut self) -> Result<NaiveTime, RowsError> {
        let seconds = u32::from_le_bytes(self.array()?);
        let nanos = u32::from_le_bytes(self.array()?);
        NaiveTime::from_num_seconds_from_midnight_opt(seconds, nanos)
            .ok_or(RowsError("time out of range"))
    }

    fn value(&mut self, depth: usize) -> Result<ValueRef<'a>, RowsError> {
        let [tag] = self.array()?;
        Ok(match tag {
            NULL => ValueRef::Null,
            BOOL => ValueRef::Bool(self.array::<1>()?[0] != 0),
            INT => ValueRef::Int(i64::from_le_bytes(self.array()?)),
            UINT => ValueRef::UInt(u64::from_le_bytes(self.array()?)),
            FLOAT => ValueRef::Float(f64::from_bits(u64::from_le_bytes(self.array()?))),
            DECIMAL => ValueRef::Decimal(self.str()?),
            TEXT => ValueRef::Text(self.str()?),
            BYTES => {
                let len = self.len()?;
                ValueRef::Bytes(Hex(self.take(len)?))
            }
            DATE => ValueRef::Date(self.date()?),
            TIME => ValueRef::Time(self.time()?),
            TIMESTAMP => ValueRef::Timestamp(NaiveDateTime::new(self.date()?, self.time()?)),
            TIMESTAMP_TZ => {
                let utc = NaiveDateTime::new(self.date()?, self.time()?);
                let offset = FixedOffset::east_opt(i32::from_le_bytes(self.array()?))
                    .ok_or(RowsError("time zone offset out of range"))?;
                ValueRef::TimestampTz(DateTime::from_naive_utc_and_offset(utc, offset))
            }
            UUID => ValueRef::Uuid(uuid::Uuid::from_bytes(self.array()?)),
            JSON => ValueRef::Json(self.str()?),
            ARRAY => {
                if depth >= MAX_DEPTH {
                    return Err(RowsError("arrays nested too deep"));
                }
                let count = self.len()?;
                let mut items = Vec::with_capacity(count.min(self.bytes.len()));
                for _ in 0..count {
                    items.push(self.value(depth + 1)?);
                }
                ValueRef::Array(items)
            }
            OTHER => ValueRef::Other {
                type_name: self.str()?,
                text: self.str()?,
            },
            _ => return Err(RowsError("unknown value kind")),
        })
    }
}

/// A [`Value`] borrowing its text from the batch. Its serde form is the same as `Value`'s: the
/// same tags, 64-bit integers as strings, bytes as lowercase hex.
#[derive(Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
enum ValueRef<'a> {
    Null,
    Bool(bool),
    Int(#[serde(serialize_with = "as_string")] i64),
    UInt(#[serde(serialize_with = "as_string")] u64),
    Float(f64),
    Decimal(&'a str),
    Text(&'a str),
    Bytes(Hex<'a>),
    Date(NaiveDate),
    Time(NaiveTime),
    Timestamp(NaiveDateTime),
    TimestampTz(DateTime<FixedOffset>),
    Uuid(uuid::Uuid),
    Json(&'a str),
    Array(Vec<ValueRef<'a>>),
    Other { type_name: &'a str, text: &'a str },
}

impl ValueRef<'_> {
    fn into_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(b),
            Self::Int(v) => Value::Int(v),
            Self::UInt(v) => Value::UInt(v),
            Self::Float(v) => Value::Float(v),
            Self::Decimal(text) => Value::Decimal(text.to_owned()),
            Self::Text(text) => Value::Text(text.to_owned()),
            Self::Bytes(Hex(bytes)) => Value::Bytes(bytes.to_vec()),
            Self::Date(date) => Value::Date(date),
            Self::Time(time) => Value::Time(time),
            Self::Timestamp(at) => Value::Timestamp(at),
            Self::TimestampTz(at) => Value::TimestampTz(at),
            Self::Uuid(uuid) => Value::Uuid(uuid),
            Self::Json(text) => Value::Json(text.to_owned()),
            Self::Array(items) => Value::Array(items.into_iter().map(Self::into_value).collect()),
            Self::Other { type_name, text } => Value::Other {
                type_name: type_name.to_owned(),
                text: text.to_owned(),
            },
        }
    }
}

fn as_string<T: std::fmt::Display, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(value)
}

/// Bytes, serialized as a lowercase hex string.
struct Hex<'a>(&'a [u8]);

impl Serialize for Hex<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl std::fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_kind() -> Vec<Value> {
        let date = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        let leap = NaiveTime::from_hms_nano_opt(23, 59, 59, 1_500_000_000).unwrap();
        vec![
            Value::Null,
            Value::Bool(true),
            Value::Int(i64::MIN),
            Value::UInt(u64::MAX),
            Value::Float(0.1 + 0.2),
            Value::Float(-0.0),
            Value::Decimal("-12345678901234567890.123456789".into()),
            Value::Text("đã sửa ✓ \u{0}".into()),
            Value::Bytes(vec![0, 1, 255]),
            Value::Date(NaiveDate::MIN),
            Value::Time(leap),
            Value::Timestamp(NaiveDateTime::new(date, leap)),
            Value::TimestampTz(
                DateTime::parse_from_rfc3339("2026-10-29T16:02:26.556+07:00").unwrap(),
            ),
            Value::Uuid(uuid::Uuid::from_u128(
                0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
            )),
            Value::Json(r#"{"a":[1,2]}"#.into()),
            Value::Array(vec![Value::Int(1), Value::Array(vec![Value::Null])]),
            Value::Other {
                type_name: "GEOMETRY".into(),
                text: "POINT(1 2)".into(),
            },
        ]
    }

    #[test]
    fn every_kind_of_value_round_trips_exactly() {
        let rows = vec![every_kind(), Vec::new(), vec![Value::Null]];
        let mut bytes = Vec::new();
        encode(&rows, &mut bytes);
        assert_eq!(decode(&bytes).unwrap(), rows);
    }

    #[test]
    fn json_matches_what_serde_writes() {
        let rows = vec![
            every_kind(),
            Vec::new(),
            vec![Value::Float(f64::NAN), Value::Null],
        ];
        assert_eq!(
            serde_json::to_string(&RowBatch::encode(&rows)).unwrap(),
            serde_json::to_string(&rows).unwrap()
        );
        assert_eq!(serde_json::to_string(&RowBatch::encode(&[])).unwrap(), "[]");
    }

    #[test]
    fn a_batch_knows_its_size_and_hides_its_values() {
        let batch = RowBatch::encode(&[vec![Value::Text("secret".into())], vec![]]);
        assert_eq!(batch.len(), 2);
        assert_eq!(format!("{batch:?}"), "RowBatch(2 rows)");
        assert!(RowBatch::from_bytes(Vec::new()).is_empty());
    }

    #[test]
    fn nan_keeps_its_bits() {
        let mut bytes = Vec::new();
        encode(&[vec![Value::Float(f64::NAN)]], &mut bytes);
        let rows = decode(&bytes).unwrap();
        let Value::Float(v) = rows[0][0] else {
            panic!("float expected");
        };
        assert_eq!(v.to_bits(), f64::NAN.to_bits());
    }

    #[test]
    fn malformed_batches_are_refused() {
        let mut bytes = Vec::new();
        encode(&[every_kind()], &mut bytes);
        for cut in [1, 5, bytes.len() / 2, bytes.len() - 1] {
            assert!(decode(&bytes[..cut]).is_err(), "cut at {cut}");
            let batch = RowBatch::from_bytes(bytes[..cut].to_vec());
            assert!(serde_json::to_string(&batch).is_err(), "json cut at {cut}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        // One row of one cell with an unknown tag.
        assert!(decode(&[1, 0, 0, 0, 1, 0, 0, 0, 99]).is_err());
        // A row count far beyond the bytes does not allocate it.
        assert!(decode(&u32::MAX.to_le_bytes()).is_err());
    }

    #[test]
    fn deep_arrays_are_refused() {
        let mut value = Value::Null;
        for _ in 0..=MAX_DEPTH {
            value = Value::Array(vec![value]);
        }
        let mut bytes = Vec::new();
        encode(&[vec![value]], &mut bytes);
        assert!(decode(&bytes).is_err());
    }
}
