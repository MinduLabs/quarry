//! PostgreSQL values in their binary form, read into [`Value`]s.
//!
//! tokio-postgres reads results in the binary format, so each type is decoded here from its
//! wire layout (`src/backend/utils/adt/*send` in PostgreSQL). A type this file does not know
//! becomes `Value::Other` with its text when the bytes are UTF-8 (`citext`, an enum label) or
//! its hex otherwise, never a failure.

use std::fmt::Write as _;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Offset, Utc};
use quarry_model::Value;
use tokio_postgres::types::{FromSql, Kind, Type};

/// A column's bytes, whatever its type: decoded by [`value`].
pub struct Raw<'a>(pub &'a [u8]);

impl<'a> FromSql<'a> for Raw<'a> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(raw))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

/// Arrays nest at most this deep, so a malformed value cannot overflow the stack.
const MAX_DEPTH: usize = 8;

/// `bytes` of type `ty` as a value; `None` is SQL `NULL`.
pub fn value(ty: &Type, bytes: Option<&[u8]>) -> Value {
    match bytes {
        None => Value::Null,
        Some(bytes) => decode(ty, bytes, 0).unwrap_or_else(|| other(ty, bytes)),
    }
}

fn decode(ty: &Type, b: &[u8], depth: usize) -> Option<Value> {
    match ty.kind() {
        Kind::Array(element) => return array(element, b, depth),
        Kind::Domain(base) => return decode(base, b, depth),
        Kind::Enum(_) => return text(b).map(Value::Text),
        _ => {}
    }
    Some(match *ty {
        Type::BOOL => Value::Bool(*b.first()? != 0),
        Type::INT2 => Value::Int(i16::from_be_bytes(b.try_into().ok()?).into()),
        Type::INT4 => Value::Int(i32::from_be_bytes(b.try_into().ok()?).into()),
        Type::INT8 => Value::Int(i64::from_be_bytes(b.try_into().ok()?)),
        Type::OID | Type::XID | Type::CID | Type::REGPROC | Type::REGCLASS | Type::REGTYPE => {
            Value::UInt(u32::from_be_bytes(b.try_into().ok()?).into())
        }
        Type::FLOAT4 => Value::Float(f32::from_be_bytes(b.try_into().ok()?).into()),
        Type::FLOAT8 => Value::Float(f64::from_be_bytes(b.try_into().ok()?)),
        Type::NUMERIC => Value::Decimal(numeric(b)?),
        Type::MONEY => Value::Decimal(money(i64::from_be_bytes(b.try_into().ok()?))),
        Type::TEXT
        | Type::VARCHAR
        | Type::BPCHAR
        | Type::NAME
        | Type::CHAR
        | Type::UNKNOWN
        | Type::XML => Value::Text(text(b)?),
        Type::JSON => Value::Json(text(b)?),
        // jsonb starts with a format version, 1.
        Type::JSONB => Value::Json(text(b.strip_prefix(&[1])?)?),
        Type::BYTEA => Value::Bytes(b.to_vec()),
        Type::UUID => Value::Uuid(uuid::Uuid::from_bytes(b.try_into().ok()?)),
        Type::DATE => date(i32::from_be_bytes(b.try_into().ok()?))
            .map_or_else(|| infinite(ty, b), Value::Date),
        Type::TIME => Value::Time(time(i64::from_be_bytes(b.try_into().ok()?))?),
        Type::TIMESTAMP => timestamp(i64::from_be_bytes(b.try_into().ok()?))
            .map_or_else(|| infinite(ty, b), Value::Timestamp),
        // Stored in UTC; the session's time zone is not applied.
        Type::TIMESTAMPTZ => timestamp(i64::from_be_bytes(b.try_into().ok()?)).map_or_else(
            || infinite(ty, b),
            |at| Value::TimestampTz(DateTime::from_naive_utc_and_offset(at, utc())),
        ),
        Type::TIMETZ => other_text(ty, timetz(b)?),
        Type::INTERVAL => other_text(ty, interval(b)?),
        Type::INET | Type::CIDR => other_text(ty, inet(b, *ty == Type::CIDR)?),
        Type::MACADDR | Type::MACADDR8 => other_text(ty, mac(b)),
        Type::BIT | Type::VARBIT => other_text(ty, bits(b)?),
        _ => return None,
    })
}

fn other(ty: &Type, b: &[u8]) -> Value {
    let text = match std::str::from_utf8(b) {
        Ok(text) => text.to_owned(),
        Err(_) => {
            let mut hex = String::with_capacity(2 + b.len() * 2);
            hex.push_str("\\x");
            for byte in b {
                let _ = write!(hex, "{byte:02x}");
            }
            hex
        }
    };
    other_text(ty, text)
}

fn other_text(ty: &Type, text: String) -> Value {
    Value::Other {
        type_name: ty.name().to_owned(),
        text,
    }
}

fn text(b: &[u8]) -> Option<String> {
    std::str::from_utf8(b).ok().map(str::to_owned)
}

fn utc() -> FixedOffset {
    Utc.fix()
}

/// `infinity` / `-infinity`, which chrono cannot hold.
fn infinite(ty: &Type, b: &[u8]) -> Value {
    let negative = b.first().is_some_and(|first| first & 0x80 != 0);
    other_text(
        ty,
        if negative { "-infinity" } else { "infinity" }.to_owned(),
    )
}

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(2000, 1, 1).unwrap_or(NaiveDate::MIN)
}

fn date(days: i32) -> Option<NaiveDate> {
    if days == i32::MAX || days == i32::MIN {
        return None;
    }
    epoch().checked_add_signed(Duration::days(days.into()))
}

fn time(micros: i64) -> Option<NaiveTime> {
    let seconds = u32::try_from(micros.div_euclid(1_000_000)).ok()?;
    let nanos = u32::try_from(micros.rem_euclid(1_000_000) * 1000).ok()?;
    NaiveTime::from_num_seconds_from_midnight_opt(seconds, nanos)
}

fn timestamp(micros: i64) -> Option<NaiveDateTime> {
    if micros == i64::MAX || micros == i64::MIN {
        return None;
    }
    epoch()
        .and_hms_opt(0, 0, 0)?
        .checked_add_signed(Duration::microseconds(micros))
}

/// `numeric`: digit count, weight, sign, display scale, then base-10000 digits.
fn numeric(b: &[u8]) -> Option<String> {
    let word = |i: usize| -> Option<u16> { Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?])) };
    let count = usize::from(word(0)?);
    let weight = i32::from(word(2)? as i16);
    let sign = word(4)?;
    let scale = usize::from(word(6)?);
    match sign {
        0xC000 => return Some("NaN".to_owned()),
        0xD000 => return Some("Infinity".to_owned()),
        0xF000 => return Some("-Infinity".to_owned()),
        _ => {}
    }
    let digits: Vec<u16> = (0..count).map(|i| word(8 + i * 2)).collect::<Option<_>>()?;
    let digit = |index: i32| -> u16 {
        usize::try_from(index)
            .ok()
            .and_then(|i| digits.get(i).copied())
            .unwrap_or(0)
    };
    let mut out = String::new();
    if sign == 0x4000 {
        out.push('-');
    }
    if weight < 0 {
        out.push('0');
    } else {
        for i in 0..=weight {
            if i == 0 {
                let _ = write!(out, "{}", digit(i));
            } else {
                let _ = write!(out, "{:04}", digit(i));
            }
        }
    }
    if scale > 0 {
        out.push('.');
        let mut fraction = String::with_capacity(scale + 4);
        let mut group = weight + 1;
        while fraction.len() < scale {
            let _ = write!(fraction, "{:04}", digit(group));
            group += 1;
        }
        fraction.truncate(scale);
        out.push_str(&fraction);
    }
    Some(out)
}

fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

fn clock(micros: i64) -> String {
    let micros = micros.unsigned_abs();
    let (seconds, fraction) = (micros / 1_000_000, micros % 1_000_000);
    let mut out = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if fraction > 0 {
        let digits = format!("{fraction:06}");
        out.push('.');
        out.push_str(digits.trim_end_matches('0'));
    }
    out
}

/// `timetz`: microseconds since midnight, then the zone's offset in seconds west of UTC.
fn timetz(b: &[u8]) -> Option<String> {
    let micros = i64::from_be_bytes(b.get(..8)?.try_into().ok()?);
    let west = i32::from_be_bytes(b.get(8..12)?.try_into().ok()?);
    let east = -west;
    let sign = if east < 0 { '-' } else { '+' };
    let east = east.unsigned_abs();
    let mut out = clock(micros);
    let _ = write!(out, "{sign}{:02}", east / 3600);
    if east % 3600 != 0 {
        let _ = write!(out, ":{:02}", east / 60 % 60);
    }
    Some(out)
}

/// `interval`: microseconds, days, months, printed the way PostgreSQL does by default.
fn interval(b: &[u8]) -> Option<String> {
    let micros = i64::from_be_bytes(b.get(..8)?.try_into().ok()?);
    let days = i32::from_be_bytes(b.get(8..12)?.try_into().ok()?);
    let months = i32::from_be_bytes(b.get(12..16)?.try_into().ok()?);
    let mut parts = Vec::new();
    let unit =
        |n: i32, one: &str, many: &str| format!("{n} {}", if n.abs() == 1 { one } else { many });
    if months / 12 != 0 {
        parts.push(unit(months / 12, "year", "years"));
    }
    if months % 12 != 0 {
        parts.push(unit(months % 12, "mon", "mons"));
    }
    if days != 0 {
        parts.push(unit(days, "day", "days"));
    }
    if micros != 0 || parts.is_empty() {
        let sign = if micros < 0 { "-" } else { "" };
        parts.push(format!("{sign}{}", clock(micros)));
    }
    Some(parts.join(" "))
}

/// `inet` / `cidr`: family, prefix bits, a cidr flag, address length, address.
fn inet(b: &[u8], is_cidr: bool) -> Option<String> {
    let (family, bits, len) = (*b.first()?, *b.get(1)?, usize::from(*b.get(3)?));
    let address = b.get(4..4 + len)?;
    let (text, full) = match family {
        2 => {
            let octets: [u8; 4] = address.try_into().ok()?;
            (std::net::Ipv4Addr::from(octets).to_string(), 32)
        }
        3 => {
            let octets: [u8; 16] = address.try_into().ok()?;
            (std::net::Ipv6Addr::from(octets).to_string(), 128)
        }
        _ => return None,
    };
    Some(if is_cidr || bits != full {
        format!("{text}/{bits}")
    } else {
        text
    })
}

fn mac(b: &[u8]) -> String {
    b.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// `bit` / `varbit`: the length in bits, then the bits, high first.
fn bits(b: &[u8]) -> Option<String> {
    let len = usize::try_from(i32::from_be_bytes(b.get(..4)?.try_into().ok()?)).ok()?;
    let data = b.get(4..)?;
    (0..len)
        .map(|i| {
            let byte = data.get(i / 8)?;
            Some(if byte & (0x80 >> (i % 8)) != 0 {
                '1'
            } else {
                '0'
            })
        })
        .collect()
}

/// An array: dimensions, a null flag, the element type, each dimension's length and lower
/// bound, then each element as a length (-1 for `NULL`) and its bytes. Nested dimensions
/// become nested arrays.
fn array(element: &Type, b: &[u8], depth: usize) -> Option<Value> {
    if depth >= MAX_DEPTH {
        return None;
    }
    let int = |at: usize| -> Option<i32> {
        Some(i32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
    };
    let dims = usize::try_from(int(0)?).ok()?;
    if dims == 0 {
        return Some(Value::Array(Vec::new()));
    }
    let lengths: Vec<usize> = (0..dims)
        .map(|d| usize::try_from(int(12 + d * 8)?).ok())
        .collect::<Option<_>>()?;
    let mut at = 12 + dims * 8;
    let total = lengths
        .iter()
        .try_fold(1usize, |n, len| n.checked_mul(*len))?;
    // Each element takes at least its 4-byte length.
    if total > b.len() / 4 + 1 {
        return None;
    }
    let mut items = Vec::with_capacity(total);
    for _ in 0..total {
        let len = int(at)?;
        at += 4;
        if len < 0 {
            items.push(Value::Null);
            continue;
        }
        let len = usize::try_from(len).ok()?;
        let bytes = b.get(at..at + len)?;
        at += len;
        items.push(decode(element, bytes, depth + 1).unwrap_or_else(|| other(element, bytes)));
    }
    Some(nest(items, &lengths))
}

/// Splits `items` into nested arrays, the last dimension innermost.
fn nest(items: Vec<Value>, lengths: &[usize]) -> Value {
    let Some((first, rest)) = lengths.split_first() else {
        return Value::Array(items);
    };
    if rest.is_empty() {
        return Value::Array(items);
    }
    let inner: usize = rest.iter().product();
    let mut items = items.into_iter();
    Value::Array(
        (0..*first)
            .map(|_| nest(items.by_ref().take(inner).collect(), rest))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric_bytes(weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(digits.len() as u16).to_be_bytes());
        b.extend_from_slice(&weight.to_be_bytes());
        b.extend_from_slice(&sign.to_be_bytes());
        b.extend_from_slice(&scale.to_be_bytes());
        for d in digits {
            b.extend_from_slice(&d.to_be_bytes());
        }
        b
    }

    #[test]
    fn numerics_keep_every_digit() {
        // 12345678.9012
        assert_eq!(
            numeric(&numeric_bytes(1, 0, 4, &[1234, 5678, 9012])).as_deref(),
            Some("12345678.9012")
        );
        // -0.0005
        assert_eq!(
            numeric(&numeric_bytes(-1, 0x4000, 4, &[5])).as_deref(),
            Some("-0.0005")
        );
        // 100000000 (trailing zero groups are not sent)
        assert_eq!(
            numeric(&numeric_bytes(2, 0, 0, &[1])).as_deref(),
            Some("100000000")
        );
        // 1.50 keeps its scale
        assert_eq!(
            numeric(&numeric_bytes(0, 0, 2, &[1, 5000])).as_deref(),
            Some("1.50")
        );
        assert_eq!(
            numeric(&numeric_bytes(0, 0xC000, 0, &[])).as_deref(),
            Some("NaN")
        );
        assert_eq!(numeric(&[0, 1]), None);
    }

    #[test]
    fn dates_and_times_count_from_2000() {
        assert_eq!(
            value(&Type::DATE, Some(&(-1i32).to_be_bytes())),
            Value::Date(NaiveDate::from_ymd_opt(1999, 12, 31).unwrap())
        );
        assert_eq!(
            value(&Type::TIMESTAMP, Some(&1_500_000i64.to_be_bytes())),
            Value::Timestamp(
                NaiveDate::from_ymd_opt(2000, 1, 1)
                    .unwrap()
                    .and_hms_milli_opt(0, 0, 1, 500)
                    .unwrap()
            )
        );
        assert_eq!(
            value(&Type::DATE, Some(&i32::MAX.to_be_bytes())),
            Value::Other {
                type_name: "date".into(),
                text: "infinity".into()
            }
        );
    }

    #[test]
    fn intervals_read_like_psql() {
        let mut b = Vec::new();
        b.extend_from_slice(&(4 * 3_600_000_000i64 + 5_000_000).to_be_bytes());
        b.extend_from_slice(&3i32.to_be_bytes());
        b.extend_from_slice(&14i32.to_be_bytes());
        assert_eq!(
            interval(&b).as_deref(),
            Some("1 year 2 mons 3 days 04:00:05")
        );
    }

    #[test]
    fn network_and_bit_types_read_as_text() {
        assert_eq!(
            inet(&[2, 32, 0, 4, 10, 0, 0, 1], false).as_deref(),
            Some("10.0.0.1")
        );
        assert_eq!(
            inet(&[2, 24, 1, 4, 10, 0, 0, 0], true).as_deref(),
            Some("10.0.0.0/24")
        );
        assert_eq!(bits(&[0, 0, 0, 3, 0b1010_0000]).as_deref(), Some("101"));
    }

    #[test]
    fn arrays_nest_by_dimension() {
        // int4[][] {{1,2},{3,NULL}}
        let mut b = Vec::new();
        for n in [2i32, 1, 23, 2, 1, 2, 1] {
            b.extend_from_slice(&n.to_be_bytes());
        }
        for v in [Some(1i32), Some(2), Some(3), None] {
            match v {
                Some(v) => {
                    b.extend_from_slice(&4i32.to_be_bytes());
                    b.extend_from_slice(&v.to_be_bytes());
                }
                None => b.extend_from_slice(&(-1i32).to_be_bytes()),
            }
        }
        assert_eq!(
            value(&Type::INT4_ARRAY, Some(&b)),
            Value::Array(vec![
                Value::Array(vec![Value::Int(1), Value::Int(2)]),
                Value::Array(vec![Value::Int(3), Value::Null]),
            ])
        );
    }

    #[test]
    fn unknown_types_never_fail() {
        assert!(matches!(
            value(&Type::INT4, Some(&[1])),
            Value::Other { .. }
        ));
        assert_eq!(
            value(&Type::POINT, Some(&[0xff])),
            Value::Other {
                type_name: "point".into(),
                text: "\\xff".into()
            }
        );
    }
}
