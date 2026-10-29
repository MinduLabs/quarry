use std::collections::HashSet;
use std::io::Write;

use quarry_model::{ColumnMeta, Value};
use serde::de::IgnoredAny;

use crate::{Result, cell_text, float_text, push_json_string};

/// Integers beyond this lose precision as a JavaScript number, so they travel as strings.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// One JSON array, one object per line.
    Array,
    /// One object per line (NDJSON).
    Lines,
}

#[derive(Debug)]
pub struct Writer<W: Write> {
    sink: W,
    kind: Kind,
    /// Each column as `"name":`, already escaped. Repeated names get `_2`, `_3`, so no key is
    /// lost when a join returns two columns with the same name.
    keys: Vec<String>,
    rows: u64,
    buf: String,
}

impl<W: Write> Writer<W> {
    pub(crate) fn start(kind: Kind, sink: W, columns: &[ColumnMeta]) -> Result<Self> {
        let mut used = HashSet::new();
        let keys = columns
            .iter()
            .map(|column| {
                let mut name = column.name.clone();
                let mut copy = 2;
                while !used.insert(name.clone()) {
                    name = format!("{}_{copy}", column.name);
                    copy += 1;
                }
                let mut key = String::new();
                push_json_string(&mut key, &name);
                key.push(':');
                key
            })
            .collect();
        Ok(Self {
            sink,
            kind,
            keys,
            rows: 0,
            buf: String::new(),
        })
    }

    pub(crate) fn write_batch(&mut self, rows: &[Vec<Value>]) -> Result<()> {
        for row in rows {
            match (self.kind, self.rows) {
                (Kind::Array, 0) => self.buf.push_str("[\n  "),
                (Kind::Array, _) => self.buf.push_str(",\n  "),
                (Kind::Lines, _) => {}
            }
            self.buf.push('{');
            for (index, key) in self.keys.iter().enumerate() {
                if index > 0 {
                    self.buf.push(',');
                }
                self.buf.push_str(key);
                match row.get(index) {
                    Some(value) => push_json_value(&mut self.buf, value),
                    None => self.buf.push_str("null"),
                }
            }
            self.buf.push('}');
            if self.kind == Kind::Lines {
                self.buf.push('\n');
            }
            self.rows += 1;
        }
        self.sink.write_all(self.buf.as_bytes())?;
        self.buf.clear();
        Ok(())
    }

    pub(crate) fn rows(&self) -> u64 {
        self.rows
    }

    pub(crate) fn finish(mut self) -> Result<W> {
        if self.kind == Kind::Array {
            let end = if self.rows == 0 { "[]\n" } else { "\n]\n" };
            self.sink.write_all(end.as_bytes())?;
        }
        self.sink.flush()?;
        Ok(self.sink)
    }
}

/// A value as JSON. Numbers that fit a JavaScript number stay numbers, the rest are strings, so
/// nothing is rounded by whatever reads the file.
fn push_json_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(v) if v.unsigned_abs() <= MAX_SAFE_INTEGER => out.push_str(&v.to_string()),
        Value::UInt(v) if *v <= MAX_SAFE_INTEGER => out.push_str(&v.to_string()),
        Value::Float(v) if v.is_finite() => out.push_str(&float_text(*v)),
        Value::Json(text) if is_valid_json(text) => out.push_str(&minify(text)),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_json_value(out, item);
            }
            out.push(']');
        }
        other => push_json_string(out, &cell_text(other)),
    }
}

fn is_valid_json(text: &str) -> bool {
    serde_json::from_str::<IgnoredAny>(text).is_ok()
}

/// `text` (valid JSON) without the whitespace between tokens, so it stays on one line. Keys keep
/// their order and numbers their exact digits.
fn minify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut is_in_string = false;
    let mut is_escaped = false;
    for ch in text.chars() {
        if is_in_string {
            out.push(ch);
            if is_escaped {
                is_escaped = false;
            } else if ch == '\\' {
                is_escaped = true;
            } else if ch == '"' {
                is_in_string = false;
            }
        } else if ch == '"' {
            is_in_string = true;
            out.push(ch);
        } else if !ch.is_whitespace() {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{Ansi, columns};
    use crate::{Format, Options, Writer as AnyWriter};

    fn write(format: Format, names: &[&str], batches: &[Vec<Vec<Value>>]) -> String {
        let mut writer = AnyWriter::start(
            format,
            &Options::default(),
            &Ansi,
            Vec::new(),
            &columns(names),
        )
        .unwrap();
        for batch in batches {
            writer.write_batch(batch).unwrap();
        }
        String::from_utf8(writer.finish().unwrap()).unwrap()
    }

    fn text(value: &str) -> Value {
        Value::Text(value.into())
    }

    #[test]
    fn ndjson_is_one_object_per_line_in_column_order() {
        let batches = vec![
            vec![vec![Value::Int(1), text("a")]],
            vec![vec![Value::Int(2), Value::Null]],
        ];
        let out = write(Format::Ndjson, &["id", "name"], &batches);
        assert_eq!(out, "{\"id\":1,\"name\":\"a\"}\n{\"id\":2,\"name\":null}\n");
    }

    #[test]
    fn json_is_one_array_across_batches() {
        let batches = vec![
            vec![vec![Value::Int(1)]],
            vec![vec![Value::Int(2)], vec![Value::Int(3)]],
        ];
        let out = write(Format::Json, &["id"], &batches);
        assert_eq!(out, "[\n  {\"id\":1},\n  {\"id\":2},\n  {\"id\":3}\n]\n");
    }

    #[test]
    fn zero_rows_give_an_empty_array_and_an_empty_file() {
        assert_eq!(write(Format::Json, &["id"], &[]), "[]\n");
        assert_eq!(write(Format::Ndjson, &["id"], &[]), "");
    }

    #[test]
    fn numbers_beyond_what_javascript_holds_become_strings() {
        let batches = vec![vec![vec![
            Value::Int(9_007_199_254_740_991),
            Value::Int(9_007_199_254_740_992),
            Value::Int(i64::MIN),
            Value::UInt(u64::MAX),
            Value::Decimal("12345678901234567890.123".into()),
        ]]];
        let out = write(Format::Ndjson, &["a", "b", "c", "d", "e"], &batches);
        assert_eq!(
            out,
            "{\"a\":9007199254740991,\"b\":\"9007199254740992\",\"c\":\"-9223372036854775808\",\
             \"d\":\"18446744073709551615\",\"e\":\"12345678901234567890.123\"}\n"
        );
    }

    #[test]
    fn floats_that_json_cannot_hold_become_strings() {
        let batches = vec![vec![vec![
            Value::Float(1.5),
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
        ]]];
        let out = write(Format::Ndjson, &["a", "b", "c"], &batches);
        assert_eq!(out, "{\"a\":1.5,\"b\":\"NaN\",\"c\":\"Infinity\"}\n");
    }

    #[test]
    fn json_columns_stay_json_on_one_line_with_key_order_and_digits_intact() {
        let doc = "{\n  \"z\": 1,\n  \"a\": [1.10, \"x y\", {\"k\": null}]\n}";
        let out = write(
            Format::Ndjson,
            &["doc"],
            &[vec![vec![Value::Json(doc.into())]]],
        );
        assert_eq!(
            out,
            "{\"doc\":{\"z\":1,\"a\":[1.10,\"x y\",{\"k\":null}]}}\n"
        );
    }

    #[test]
    fn a_json_column_that_is_not_valid_json_becomes_a_string() {
        let out = write(
            Format::Ndjson,
            &["doc"],
            &[vec![vec![Value::Json("{oops".into())]]],
        );
        assert_eq!(out, "{\"doc\":\"{oops\"}\n");
    }

    #[test]
    fn repeated_column_names_keep_every_value() {
        let batches = vec![vec![vec![Value::Int(1), Value::Int(2), Value::Int(3)]]];
        let out = write(Format::Ndjson, &["id", "id", "id"], &batches);
        assert_eq!(out, "{\"id\":1,\"id_2\":2,\"id_3\":3}\n");
    }

    #[test]
    fn a_short_row_is_padded_with_null() {
        let out = write(Format::Ndjson, &["a", "b"], &[vec![vec![Value::Int(1)]]]);
        assert_eq!(out, "{\"a\":1,\"b\":null}\n");
    }

    #[test]
    fn text_and_names_are_escaped_and_non_ascii_survives() {
        let batches = vec![vec![vec![text("Nguyễn \"Ánh\"\n")]]];
        let out = write(Format::Ndjson, &["tên"], &batches);
        assert_eq!(out, "{\"tên\":\"Nguyễn \\\"Ánh\\\"\\n\"}\n");
    }

    #[test]
    fn arrays_nest_and_bytes_are_hex_strings() {
        let batches = vec![vec![vec![
            Value::Array(vec![Value::Int(1), text("x"), Value::Null]),
            Value::Bytes(vec![0xde, 0xad]),
        ]]];
        let out = write(Format::Ndjson, &["a", "b"], &batches);
        assert_eq!(out, "{\"a\":[1,\"x\",null],\"b\":\"dead\"}\n");
    }
}
