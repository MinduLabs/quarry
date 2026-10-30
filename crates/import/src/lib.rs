//! Reads CSV and TSV one record at a time, so a file of any size costs one record of memory.
//! Nothing here touches a database: the engine decides what a record means.

use std::io::{self, BufRead};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ImportError>;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("Couldn't read the file: {0}. Check that it still exists and try again.")]
    Io(#[from] io::Error),
    #[error("The delimiter must be one ASCII character that is not a quote or a line break.")]
    InvalidDelimiter,
    #[error("Line {line}: a quoted value never closes. Check for a stray quote.")]
    UnclosedQuote { line: u64 },
    #[error("Line {line}: the file is not valid UTF-8. Save it as UTF-8 and try again.")]
    NotUtf8 { line: u64 },
    #[error("Line {line}: found {found} values but expected {expected}. Check the delimiter.")]
    RowWidth {
        line: u64,
        expected: usize,
        found: usize,
    },
}

/// How a file is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// A tab for TSV.
    pub delimiter: char,
    /// The first record names the columns and is not data.
    pub has_header: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            delimiter: ',',
            has_header: true,
        }
    }
}

/// One record per call. Quotes follow RFC 4180: `""` is a quote inside a quoted value, and a
/// quoted value may hold the delimiter and line breaks. A blank line is skipped.
#[derive(Debug)]
pub struct Reader<R: BufRead> {
    source: R,
    delimiter: u8,
    header: Option<Vec<String>>,
    width: Option<usize>,
    /// Line the next record starts on, 1-based.
    line: u64,
}

impl<R: BufRead> Reader<R> {
    /// Reads the header too, so `header` and `width` are known before the first record.
    pub fn new(mut source: R, options: Options) -> Result<Self> {
        let delimiter = u8::try_from(options.delimiter)
            .ok()
            .filter(|byte| byte.is_ascii() && !matches!(byte, b'"' | b'\r' | b'\n'))
            .ok_or(ImportError::InvalidDelimiter)?;
        // Excel writes a byte order mark; it is not part of the first name.
        if source.fill_buf()?.starts_with(&[0xEF, 0xBB, 0xBF]) {
            source.consume(3);
        }
        let mut reader = Self {
            source,
            delimiter,
            header: None,
            width: None,
            line: 1,
        };
        if options.has_header {
            let header = reader.read_record()?;
            reader.width = header.as_ref().map(Vec::len);
            reader.header = header;
        }
        Ok(reader)
    }

    pub fn header(&self) -> Option<&[String]> {
        self.header.as_deref()
    }

    /// Values per record: the header's, or the first record's when there is no header.
    pub fn width(&self) -> Option<usize> {
        self.width
    }

    /// The next data record, or `None` at the end. A record of another width is an error: a
    /// shifted column would write wrong data into the table.
    pub fn next_record(&mut self) -> Result<Option<Vec<String>>> {
        let line = self.line;
        let Some(record) = self.read_record()? else {
            return Ok(None);
        };
        let expected = *self.width.get_or_insert(record.len());
        if record.len() != expected {
            return Err(ImportError::RowWidth {
                line,
                expected,
                found: record.len(),
            });
        }
        Ok(Some(record))
    }

    fn peek(&mut self) -> Result<Option<u8>> {
        Ok(self.source.fill_buf()?.first().copied())
    }

    fn read_record(&mut self) -> Result<Option<Vec<String>>> {
        loop {
            let start = self.line;
            let Some((fields, has_quote)) = self.read_fields(start)? else {
                return Ok(None);
            };
            let is_blank = fields.len() == 1 && fields[0].is_empty() && !has_quote;
            if !is_blank {
                return Ok(Some(fields));
            }
        }
    }

    /// One record's values, and whether any was quoted (`""` is an empty value, not a blank line).
    fn read_fields(&mut self, start: u64) -> Result<Option<(Vec<String>, bool)>> {
        if self.peek()?.is_none() {
            return Ok(None);
        }
        let mut fields = Vec::new();
        let mut value: Vec<u8> = Vec::new();
        let mut has_quote = false;
        let mut is_quoted = false;
        loop {
            let Some(byte) = self.peek()? else {
                if is_quoted {
                    return Err(ImportError::UnclosedQuote { line: start });
                }
                break;
            };
            self.source.consume(1);
            if is_quoted {
                if byte == b'"' {
                    if self.peek()? == Some(b'"') {
                        self.source.consume(1);
                        value.push(b'"');
                    } else {
                        is_quoted = false;
                    }
                } else {
                    if byte == b'\n' {
                        self.line += 1;
                    }
                    value.push(byte);
                }
                continue;
            }
            match byte {
                b'"' if value.is_empty() => {
                    is_quoted = true;
                    has_quote = true;
                }
                b'\r' | b'\n' => {
                    if byte == b'\r' && self.peek()? == Some(b'\n') {
                        self.source.consume(1);
                    }
                    self.line += 1;
                    break;
                }
                _ if byte == self.delimiter => fields.push(take_text(&mut value, start)?),
                _ => value.push(byte),
            }
        }
        fields.push(take_text(&mut value, start)?);
        Ok(Some((fields, has_quote)))
    }
}

fn take_text(value: &mut Vec<u8>, line: u64) -> Result<String> {
    String::from_utf8(std::mem::take(value)).map_err(|_| ImportError::NotUtf8 { line })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(text: &str, options: Options) -> Result<Vec<Vec<String>>> {
        let mut reader = Reader::new(text.as_bytes(), options)?;
        let mut rows = Vec::new();
        while let Some(row) = reader.next_record()? {
            rows.push(row);
        }
        Ok(rows)
    }

    fn row(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn reads_header_and_rows() {
        let mut reader =
            Reader::new("id,name\r\n1,Ann\n2,Bob".as_bytes(), Options::default()).expect("reader");
        assert_eq!(reader.header(), Some(row(&["id", "name"]).as_slice()));
        assert_eq!(reader.width(), Some(2));
        assert_eq!(reader.next_record().expect("row"), Some(row(&["1", "Ann"])));
        assert_eq!(reader.next_record().expect("row"), Some(row(&["2", "Bob"])));
        assert_eq!(reader.next_record().expect("end"), None);
    }

    #[test]
    fn quoted_values_hold_delimiters_quotes_and_line_breaks() {
        let rows =
            read_all("a,b\n\"x,y\",\"say \"\"hi\"\"\nnow\"\n", Options::default()).expect("rows");
        assert_eq!(rows, vec![row(&["x,y", "say \"hi\"\nnow"])]);
    }

    #[test]
    fn blank_lines_are_skipped_but_a_quoted_empty_value_is_data() {
        let rows = read_all("a\n\n\"\"\n\nb\n", Options::default()).expect("rows");
        assert_eq!(rows, vec![row(&[""]), row(&["b"])]);
    }

    #[test]
    fn without_header_the_first_record_sets_the_width() {
        let options = Options {
            delimiter: '\t',
            has_header: false,
        };
        let err = read_all("1\t2\n3\n", options).expect_err("ragged");
        assert!(matches!(
            err,
            ImportError::RowWidth {
                line: 2,
                expected: 2,
                found: 1
            }
        ));
    }

    #[test]
    fn byte_order_mark_is_not_part_of_the_first_name() {
        let reader =
            Reader::new("\u{feff}id,name\n".as_bytes(), Options::default()).expect("reader");
        assert_eq!(reader.header(), Some(row(&["id", "name"]).as_slice()));
    }

    #[test]
    fn unclosed_quote_names_the_line_it_opened_on() {
        let err = read_all("a\n1\n\"oops\n2\n", Options::default()).expect_err("unclosed");
        assert!(matches!(err, ImportError::UnclosedQuote { line: 3 }));
    }

    #[test]
    fn invalid_utf8_is_an_error_not_a_lossy_value() {
        let mut reader = Reader::new(&b"a\n\xff\n"[..], Options::default()).expect("reader");
        assert!(matches!(
            reader.next_record(),
            Err(ImportError::NotUtf8 { line: 2 })
        ));
    }

    #[test]
    fn rejects_a_delimiter_that_breaks_the_format() {
        for delimiter in ['"', '\n', 'é'] {
            let options = Options {
                delimiter,
                has_header: true,
            };
            assert!(matches!(
                Reader::new("a".as_bytes(), options),
                Err(ImportError::InvalidDelimiter)
            ));
        }
    }
}
