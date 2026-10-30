//! SQL Server values (`tiberius::ColumnData`) as [`Value`]s, and parameters back.
//!
//! Dates and times arrive as SQL Server's own counts (days since 1900 or year 1, ticks at the
//! column's scale) and are turned into chrono values here, exactly.

use std::borrow::Cow;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use quarry_model::Value;
use tiberius::numeric::Numeric;
use tiberius::time::{Date, DateTime2, Time};
use tiberius::{ColumnData, ColumnType, ToSql};

pub fn value(data: ColumnData<'static>, column: ColumnType) -> Value {
    match data {
        ColumnData::U8(v) => v.map_or(Value::Null, |v| Value::Int(v.into())),
        ColumnData::I16(v) => v.map_or(Value::Null, |v| Value::Int(v.into())),
        ColumnData::I32(v) => v.map_or(Value::Null, |v| Value::Int(v.into())),
        ColumnData::I64(v) => v.map_or(Value::Null, Value::Int),
        ColumnData::F32(v) => v.map_or(Value::Null, |v| float(v.into(), column)),
        ColumnData::F64(v) => v.map_or(Value::Null, |v| float(v, column)),
        ColumnData::Bit(v) => v.map_or(Value::Null, Value::Bool),
        ColumnData::String(v) => v.map_or(Value::Null, |v| Value::Text(v.into_owned())),
        ColumnData::Guid(v) => v.map_or(Value::Null, Value::Uuid),
        ColumnData::Binary(v) => v.map_or(Value::Null, |v| Value::Bytes(v.into_owned())),
        ColumnData::Numeric(v) => v.map_or(Value::Null, |v| Value::Decimal(numeric(v))),
        ColumnData::Xml(v) => v.map_or(Value::Null, |v| Value::Text(v.into_owned().into_string())),
        ColumnData::DateTime(v) => v.map_or(Value::Null, |v| {
            // Days since 1900-01-01, then 1/300 s since midnight.
            let nanos = i64::from(v.seconds_fragments()) * 10_000_000 / 3;
            timestamp(epoch_1900(), v.days().into(), nanos)
        }),
        ColumnData::SmallDateTime(v) => v.map_or(Value::Null, |v| {
            let nanos = i64::from(v.seconds_fragments()) * 60_000_000_000;
            timestamp(epoch_1900(), v.days().into(), nanos)
        }),
        ColumnData::Date(v) => v.map_or(Value::Null, |v| {
            date(v).map_or_else(|| other(column, "date out of range"), Value::Date)
        }),
        ColumnData::Time(v) => v.map_or(Value::Null, |v| {
            time(v).map_or_else(|| other(column, "time out of range"), Value::Time)
        }),
        ColumnData::DateTime2(v) => v.map_or(Value::Null, |v| {
            datetime2(v).map_or_else(|| other(column, "date out of range"), Value::Timestamp)
        }),
        ColumnData::DateTimeOffset(v) => v.map_or(Value::Null, |v| {
            // The date and time are UTC; the offset is minutes east.
            let at = datetime2(v.datetime2());
            let offset = FixedOffset::east_opt(i32::from(v.offset()) * 60);
            match (at, offset) {
                (Some(at), Some(offset)) => {
                    Value::TimestampTz(DateTime::from_naive_utc_and_offset(at, offset))
                }
                _ => other(column, "date out of range"),
            }
        }),
    }
}

/// `money` arrives as a float; it has four decimal places.
fn float(v: f64, column: ColumnType) -> Value {
    match column {
        ColumnType::Money | ColumnType::Money4 => Value::Decimal(format!("{v:.4}")),
        _ => Value::Float(v),
    }
}

fn other(column: ColumnType, text: &str) -> Value {
    Value::Other {
        type_name: type_name(column).to_owned(),
        text: text.to_owned(),
    }
}

fn numeric(n: Numeric) -> String {
    let scale = usize::from(n.scale());
    let value = n.value();
    let digits = value.unsigned_abs().to_string();
    let sign = if value < 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let digits = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = digits.split_at(digits.len() - scale);
    format!("{sign}{int}.{frac}")
}

fn epoch_1900() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(1900, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .unwrap_or_default()
}

fn timestamp(epoch: NaiveDateTime, days: i64, nanos: i64) -> Value {
    epoch
        .checked_add_signed(Duration::days(days))
        .and_then(|at| at.checked_add_signed(Duration::nanoseconds(nanos)))
        .map_or(Value::Null, Value::Timestamp)
}

fn date(d: Date) -> Option<NaiveDate> {
    // Days since 0001-01-01, which chrono counts as day 1.
    NaiveDate::from_num_days_from_ce_opt(i32::try_from(d.days()).ok()?.checked_add(1)?)
}

fn time(t: Time) -> Option<NaiveTime> {
    // Ticks of 10^-scale seconds since midnight.
    let per_second = 10u64.checked_pow(u32::from(t.scale()))?;
    let seconds = u32::try_from(t.increments() / per_second).ok()?;
    let nanos = (t.increments() % per_second) * (1_000_000_000 / per_second);
    NaiveTime::from_num_seconds_from_midnight_opt(seconds, u32::try_from(nanos).ok()?)
}

fn datetime2(dt: DateTime2) -> Option<NaiveDateTime> {
    Some(NaiveDateTime::new(date(dt.date())?, time(dt.time())?))
}

/// The type's name as SQL Server writes it, for the grid's header.
pub fn type_name(column: ColumnType) -> &'static str {
    match column {
        ColumnType::Null => "null",
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 | ColumnType::Intn => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Float4 => "real",
        ColumnType::Float8 | ColumnType::Floatn => "float",
        ColumnType::Money => "money",
        ColumnType::Money4 => "smallmoney",
        ColumnType::Datetime | ColumnType::Datetimen => "datetime",
        ColumnType::Datetime4 => "smalldatetime",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::Decimaln => "decimal",
        ColumnType::Numericn => "numeric",
        ColumnType::BigVarBin => "varbinary",
        ColumnType::BigBinary => "binary",
        ColumnType::BigVarChar => "varchar",
        ColumnType::BigChar => "char",
        ColumnType::NVarchar => "nvarchar",
        ColumnType::NChar => "nchar",
        ColumnType::Xml => "xml",
        ColumnType::Udt => "udt",
        ColumnType::Text => "text",
        ColumnType::Image => "image",
        ColumnType::NText => "ntext",
        ColumnType::SSVariant => "sql_variant",
    }
}

/// A parameter as text, which the server converts to the type the statement needs: a grid edit
/// holds `Value::Text("42")` for an `int` column. Bytes go as binary, which text cannot carry.
#[derive(Debug)]
pub enum Param {
    Null,
    Bytes(Vec<u8>),
    Text(String),
}

impl Param {
    pub fn new(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bytes(bytes) => Self::Bytes(bytes.clone()),
            other => Self::Text(text(other)),
        }
    }
}

impl ToSql for Param {
    fn to_sql(&self) -> ColumnData<'_> {
        match self {
            Self::Null => ColumnData::String(None),
            Self::Bytes(bytes) => ColumnData::Binary(Some(Cow::Borrowed(bytes))),
            Self::Text(text) => ColumnData::String(Some(Cow::Borrowed(text))),
        }
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "1" } else { "0" }.to_owned(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::Decimal(text) | Value::Text(text) | Value::Json(text) => text.clone(),
        Value::Other { text, .. } => text.clone(),
        Value::Bytes(bytes) => bytes.iter().map(|b| format!("{b:02X}")).collect(),
        Value::Date(date) => date.format("%Y-%m-%d").to_string(),
        Value::Time(time) => time.format("%H:%M:%S%.f").to_string(),
        Value::Timestamp(at) => at.format("%Y-%m-%dT%H:%M:%S%.f").to_string(),
        Value::TimestampTz(at) => at.format("%Y-%m-%dT%H:%M:%S%.f%:z").to_string(),
        Value::Uuid(uuid) => uuid.to_string(),
        Value::Array(items) => items.iter().map(text).collect::<Vec<_>>().join(","),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_keep_their_scale() {
        assert_eq!(numeric(Numeric::new_with_scale(123_456, 2)), "1234.56");
        assert_eq!(numeric(Numeric::new_with_scale(-5, 4)), "-0.0005");
        assert_eq!(numeric(Numeric::new_with_scale(42, 0)), "42");
    }

    #[test]
    fn times_count_ticks_at_their_scale() {
        assert_eq!(
            time(Time::new(863_999_999_999, 7)),
            NaiveTime::from_hms_nano_opt(23, 59, 59, 999_999_900)
        );
        assert_eq!(date(Date::new(0)), NaiveDate::from_ymd_opt(1, 1, 1));
    }

    #[test]
    fn legacy_datetime_counts_three_hundredths() {
        let v = value(
            ColumnData::DateTime(Some(tiberius::time::DateTime::new(36524, 450))),
            ColumnType::Datetimen,
        );
        assert_eq!(v.display(64), "2000-01-01 00:00:01.500");
    }

    #[test]
    fn params_are_text_except_bytes_and_null() {
        assert!(matches!(
            Param::new(&Value::Null).to_sql(),
            ColumnData::String(None)
        ));
        assert!(matches!(
            Param::new(&Value::Bytes(vec![1])).to_sql(),
            ColumnData::Binary(Some(_))
        ));
        assert!(
            matches!(Param::new(&Value::Bool(true)).to_sql(), ColumnData::String(Some(t)) if t == "1")
        );
    }
}
