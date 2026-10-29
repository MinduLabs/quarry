use std::io::Write;

use quarry_model::{ColumnMeta, Value};

use crate::{ExportError, Options, Result, cell_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Csv,
    Tsv,
}

/// CSV (RFC 4180: CRLF lines, quotes only where needed) and TSV (LF lines, no quoting; a tab or
/// line break inside a value becomes a space, since it would split the cell).
#[derive(Debug)]
pub struct Writer<W: Write> {
    sink: W,
    kind: Kind,
    delimiter: char,
    width: usize,
    rows: u64,
    /// One batch of text, reused so a batch costs no allocation after the first.
    buf: String,
}

impl<W: Write> Writer<W> {
    pub(crate) fn start(
        kind: Kind,
        options: &Options,
        sink: W,
        columns: &[ColumnMeta],
    ) -> Result<Self> {
        let delimiter = match kind {
            Kind::Csv => options.delimiter,
            Kind::Tsv => '\t',
        };
        if kind == Kind::Csv && matches!(delimiter, '"' | '\r' | '\n') {
            return Err(ExportError::InvalidDelimiter);
        }
        let mut writer = Self {
            sink,
            kind,
            delimiter,
            width: columns.len(),
            rows: 0,
            buf: String::new(),
        };
        if options.has_bom {
            writer.sink.write_all(&[0xEF, 0xBB, 0xBF])?;
        }
        if options.has_header {
            for (index, column) in columns.iter().enumerate() {
                writer.push_cell(index, &column.name);
            }
            writer.push_line_end();
            writer.flush_buf()?;
        }
        Ok(writer)
    }

    pub(crate) fn write_batch(&mut self, rows: &[Vec<Value>]) -> Result<()> {
        for row in rows {
            for index in 0..self.width {
                match row.get(index) {
                    Some(value) => self.push_cell(index, &cell_text(value)),
                    None => self.push_cell(index, ""),
                }
            }
            self.push_line_end();
        }
        self.rows += rows.len() as u64;
        self.flush_buf()
    }

    pub(crate) fn rows(&self) -> u64 {
        self.rows
    }

    pub(crate) fn finish(mut self) -> Result<W> {
        self.sink.flush()?;
        Ok(self.sink)
    }

    fn push_cell(&mut self, index: usize, text: &str) {
        if index > 0 {
            self.buf.push(self.delimiter);
        }
        match self.kind {
            Kind::Csv => push_csv_field(&mut self.buf, text, self.delimiter),
            Kind::Tsv => push_tsv_field(&mut self.buf, text),
        }
    }

    fn push_line_end(&mut self) {
        self.buf.push_str(match self.kind {
            Kind::Csv => "\r\n",
            Kind::Tsv => "\n",
        });
    }

    fn flush_buf(&mut self) -> Result<()> {
        self.sink.write_all(self.buf.as_bytes())?;
        self.buf.clear();
        Ok(())
    }
}

fn push_csv_field(out: &mut String, text: &str, delimiter: char) {
    let is_quoted = text.contains([delimiter, '"', '\r', '\n']) || text.trim().len() != text.len();
    if !is_quoted {
        out.push_str(text);
        return;
    }
    out.push('"');
    for ch in text.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
}

fn push_tsv_field(out: &mut String, text: &str) {
    let mut is_in_run = false;
    for ch in text.chars() {
        if matches!(ch, '\t' | '\r' | '\n') {
            if !is_in_run {
                out.push(' ');
            }
            is_in_run = true;
        } else {
            out.push(ch);
            is_in_run = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{Ansi, columns};
    use crate::{Format, Writer as AnyWriter};

    fn write(
        format: Format,
        options: &Options,
        names: &[&str],
        batches: &[Vec<Vec<Value>>],
    ) -> Vec<u8> {
        let mut writer =
            AnyWriter::start(format, options, &Ansi, Vec::new(), &columns(names)).unwrap();
        for batch in batches {
            writer.write_batch(batch).unwrap();
        }
        writer.finish().unwrap()
    }

    fn text(value: &str) -> Value {
        Value::Text(value.into())
    }

    #[test]
    fn csv_has_a_header_crlf_lines_and_quotes_only_where_needed() {
        let batches = vec![
            vec![vec![Value::Int(1), text("plain"), text("a,b")]],
            vec![
                vec![Value::Int(2), text("say \"hi\""), text("line\nbreak")],
                vec![Value::Int(3), text(" padded "), Value::Null],
            ],
        ];
        let out = write(
            Format::Csv,
            &Options::default(),
            &["id", "note", "extra"],
            &batches,
        );
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "id,note,extra\r\n\
             1,plain,\"a,b\"\r\n\
             2,\"say \"\"hi\"\"\",\"line\nbreak\"\r\n\
             3,\" padded \",\r\n"
        );
    }

    #[test]
    fn csv_honors_a_custom_delimiter() {
        let batches = vec![vec![vec![text("a;b"), text("c,d")]]];
        let options = Options {
            delimiter: ';',
            ..Options::default()
        };
        let out = write(Format::Csv, &options, &["x", "y"], &batches);
        assert_eq!(String::from_utf8(out).unwrap(), "x;y\r\n\"a;b\";c,d\r\n");
    }

    #[test]
    fn csv_refuses_a_delimiter_that_breaks_the_format() {
        for delimiter in ['"', '\n', '\r'] {
            let options = Options {
                delimiter,
                ..Options::default()
            };
            let err = AnyWriter::start(Format::Csv, &options, &Ansi, Vec::new(), &columns(&["a"]))
                .unwrap_err();
            assert!(matches!(err, ExportError::InvalidDelimiter));
        }
    }

    #[test]
    fn bom_comes_first_only_when_asked() {
        let with_bom = Options {
            has_bom: true,
            ..Options::default()
        };
        let out = write(Format::Csv, &with_bom, &["a"], &[]);
        assert_eq!(out, b"\xEF\xBB\xBFa\r\n");
        let out = write(Format::Csv, &Options::default(), &["a"], &[]);
        assert_eq!(out, b"a\r\n");
    }

    #[test]
    fn no_header_means_rows_only() {
        let options = Options {
            has_header: false,
            ..Options::default()
        };
        let out = write(Format::Csv, &options, &["a"], &[vec![vec![Value::Int(1)]]]);
        assert_eq!(out, b"1\r\n");
    }

    #[test]
    fn zero_rows_give_a_header_only_file() {
        let out = write(Format::Csv, &Options::default(), &["id", "name"], &[]);
        assert_eq!(out, b"id,name\r\n");
    }

    #[test]
    fn big_numbers_and_decimals_keep_every_digit() {
        let batches = vec![vec![vec![
            Value::Int(i64::MAX),
            Value::UInt(u64::MAX),
            Value::Decimal("12345678901234567890.123".into()),
        ]]];
        let out = write(Format::Csv, &Options::default(), &["a", "b", "c"], &batches);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "a,b,c\r\n9223372036854775807,18446744073709551615,12345678901234567890.123\r\n"
        );
    }

    #[test]
    fn non_ascii_text_round_trips_byte_for_byte() {
        let value = "Nguyễn Văn Ánh - Đà Nẵng";
        let batches = vec![vec![vec![text(value)]]];
        let out = write(Format::Csv, &Options::default(), &["name"], &batches);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("name\r\n{value}\r\n")
        );
    }

    #[test]
    fn a_short_row_is_padded_with_empty_cells() {
        let out = write(
            Format::Csv,
            &Options::default(),
            &["a", "b", "c"],
            &[vec![vec![Value::Int(1)]]],
        );
        assert_eq!(out, b"a,b,c\r\n1,,\r\n");
    }

    #[test]
    fn tsv_uses_lf_and_turns_tabs_and_line_breaks_into_one_space() {
        let batches = vec![vec![vec![text("a\tb"), text("x\r\n\ny"), Value::Null]]];
        let out = write(Format::Tsv, &Options::default(), &["p", "q", "r"], &batches);
        assert_eq!(String::from_utf8(out).unwrap(), "p\tq\tr\na b\tx y\t\n");
    }

    #[test]
    fn rows_counts_what_was_written() {
        let mut writer = AnyWriter::start(
            Format::Csv,
            &Options::default(),
            &Ansi,
            Vec::new(),
            &columns(&["a"]),
        )
        .unwrap();
        writer
            .write_batch(&[vec![Value::Int(1)], vec![Value::Int(2)]])
            .unwrap();
        writer.write_batch(&[vec![Value::Int(3)]]).unwrap();
        assert_eq!(writer.rows(), 3);
    }
}
