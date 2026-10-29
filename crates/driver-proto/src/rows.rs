//! A batch of rows in a compact binary form. Rows are most of what crosses the wire, and JSON
//! made a 1000-row query more than twice as slow.
//!
//! Each value is a tag byte, then its payload, little-endian: lengths and counts are `u32`,
//! strings are UTF-8.

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use quarry_model::Value;

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

pub fn encode(rows: &[Vec<Value>], out: &mut Vec<u8>) {
    put_len(out, rows.len());
    for row in rows {
        put_len(out, row.len());
        for value in row {
            put_value(out, value);
        }
    }
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Vec<Value>>, RowsError> {
    let mut input = Input { bytes };
    let count = input.len()?;
    // Every row takes at least its 4-byte length, so a count beyond that is a lie.
    let mut rows = Vec::with_capacity(count.min(input.bytes.len() / 4));
    for _ in 0..count {
        let cells = input.len()?;
        let mut row = Vec::with_capacity(cells.min(input.bytes.len()));
        for _ in 0..cells {
            row.push(input.value(0)?);
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

    fn string(&mut self) -> Result<String, RowsError> {
        let len = self.len()?;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| RowsError("text is not UTF-8"))
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

    fn value(&mut self, depth: usize) -> Result<Value, RowsError> {
        let [tag] = self.array()?;
        Ok(match tag {
            NULL => Value::Null,
            BOOL => Value::Bool(self.array::<1>()?[0] != 0),
            INT => Value::Int(i64::from_le_bytes(self.array()?)),
            UINT => Value::UInt(u64::from_le_bytes(self.array()?)),
            FLOAT => Value::Float(f64::from_bits(u64::from_le_bytes(self.array()?))),
            DECIMAL => Value::Decimal(self.string()?),
            TEXT => Value::Text(self.string()?),
            BYTES => {
                let len = self.len()?;
                Value::Bytes(self.take(len)?.to_vec())
            }
            DATE => Value::Date(self.date()?),
            TIME => Value::Time(self.time()?),
            TIMESTAMP => Value::Timestamp(NaiveDateTime::new(self.date()?, self.time()?)),
            TIMESTAMP_TZ => {
                let utc = NaiveDateTime::new(self.date()?, self.time()?);
                let offset = FixedOffset::east_opt(i32::from_le_bytes(self.array()?))
                    .ok_or(RowsError("time zone offset out of range"))?;
                Value::TimestampTz(DateTime::from_naive_utc_and_offset(utc, offset))
            }
            UUID => Value::Uuid(uuid::Uuid::from_bytes(self.array()?)),
            JSON => Value::Json(self.string()?),
            ARRAY => {
                if depth >= MAX_DEPTH {
                    return Err(RowsError("arrays nested too deep"));
                }
                let count = self.len()?;
                let mut items = Vec::with_capacity(count.min(self.bytes.len()));
                for _ in 0..count {
                    items.push(self.value(depth + 1)?);
                }
                Value::Array(items)
            }
            OTHER => Value::Other {
                type_name: self.string()?,
                text: self.string()?,
            },
            _ => return Err(RowsError("unknown value kind")),
        })
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
