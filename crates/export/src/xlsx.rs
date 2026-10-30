use std::fmt;
use std::io::Write;

use chrono::Datelike;
use quarry_model::{ColumnMeta, Value};
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};

use crate::{ExportError, Options, Result, Summary, cell_text};

/// Rows on one sheet, header included.
pub(crate) const MAX_ROWS_PER_SHEET: u64 = 1_048_576;
pub(crate) const MAX_COLUMNS: usize = 16_384;
/// Characters in one cell, counted in UTF-16 units as Excel counts them.
const MAX_CELL_CHARS: usize = 32_767;
/// Beyond this a whole number is not exact as an Excel number (a 64-bit float).
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Excel keeps 15 significant digits of a number.
const MAX_EXACT_DIGITS: usize = 15;
/// Excel's calendar starts here.
const FIRST_YEAR: i32 = 1900;

static NULL: Value = Value::Null;

impl From<XlsxError> for ExportError {
    fn from(err: XlsxError) -> Self {
        match err {
            XlsxError::IoError(err) => err.into(),
            other => Self::Spreadsheet(other.to_string()),
        }
    }
}

struct Formats {
    header: Format,
    date: Format,
    datetime: Format,
    time: Format,
}

/// Excel (.xlsx). Each sheet is streamed to a temporary file as rows arrive, so memory does not
/// grow with the row count; the file is zipped into the sink at [`Writer::finish`]. Past the
/// rows a sheet holds, the writer starts a new sheet in the same file.
pub struct Writer<W: Write + Send> {
    sink: W,
    workbook: Workbook,
    names: Vec<String>,
    width: usize,
    has_header: bool,
    /// Data rows per sheet.
    rows_per_sheet: u64,
    formats: Formats,
    sheets: u32,
    /// Row number of the next row on the current sheet.
    next_row: u32,
    rows_in_sheet: u64,
    rows: u64,
    cut_cells: u64,
}

impl<W: Write + Send> fmt::Debug for Writer<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("rows", &self.rows)
            .field("sheets", &self.sheets)
            .finish_non_exhaustive()
    }
}

impl<W: Write + Send> Writer<W> {
    pub(crate) fn start(options: &Options, sink: W, columns: &[ColumnMeta]) -> Result<Self> {
        if columns.len() > MAX_COLUMNS {
            return Err(ExportError::TooManyColumns(columns.len()));
        }
        let capacity = MAX_ROWS_PER_SHEET - u64::from(options.has_header);
        let rows_per_sheet = options
            .max_rows_per_sheet
            .unwrap_or(capacity)
            .clamp(1, capacity);
        let mut writer = Self {
            sink,
            workbook: Workbook::new(),
            names: columns.iter().map(|column| column.name.clone()).collect(),
            width: columns.len(),
            has_header: options.has_header,
            rows_per_sheet,
            formats: Formats {
                header: Format::new().set_bold(),
                date: Format::new().set_num_format("yyyy-mm-dd"),
                datetime: Format::new().set_num_format("yyyy-mm-dd hh:mm:ss"),
                time: Format::new().set_num_format("hh:mm:ss"),
            },
            sheets: 0,
            next_row: 0,
            rows_in_sheet: 0,
            rows: 0,
            cut_cells: 0,
        };
        writer.begin_sheet()?;
        Ok(writer)
    }

    pub(crate) fn write_batch(&mut self, rows: &[Vec<Value>]) -> Result<()> {
        for row in rows {
            if self.rows_in_sheet >= self.rows_per_sheet {
                self.begin_sheet()?;
            }
            let sheet = self
                .workbook
                .worksheet_from_index(self.sheets as usize - 1)?;
            for index in 0..self.width {
                let value = row.get(index).unwrap_or(&NULL);
                // `start` checked that every column index fits.
                write_cell(
                    sheet,
                    self.next_row,
                    index as u16,
                    value,
                    &self.formats,
                    &mut self.cut_cells,
                )?;
            }
            self.next_row += 1;
            self.rows_in_sheet += 1;
            self.rows += 1;
        }
        Ok(())
    }

    pub(crate) fn summary(&self) -> Summary {
        Summary {
            rows: self.rows,
            sheets: self.sheets,
            cut_cells: self.cut_cells,
        }
    }

    pub(crate) fn finish(mut self) -> Result<W> {
        self.workbook.save_to_writer(&mut self.sink)?;
        self.sink.flush()?;
        Ok(self.sink)
    }

    fn begin_sheet(&mut self) -> Result<()> {
        let sheet = self.workbook.add_worksheet_with_constant_memory()?;
        self.sheets += 1;
        self.next_row = 0;
        self.rows_in_sheet = 0;
        if self.has_header {
            sheet.set_freeze_panes(1, 0)?;
            for (index, name) in self.names.iter().enumerate() {
                let name = fit(name, &mut self.cut_cells);
                sheet.write_string_with_format(0, index as u16, name, &self.formats.header)?;
            }
            self.next_row = 1;
        }
        Ok(())
    }
}

/// Numbers that Excel holds exactly become numbers, dates become real dates, everything else is
/// text. Anything Excel would round or reject stays text, so no digit is lost.
fn write_cell(
    sheet: &mut Worksheet,
    row: u32,
    col: u16,
    value: &Value,
    formats: &Formats,
    cut_cells: &mut u64,
) -> std::result::Result<(), XlsxError> {
    match value {
        Value::Null => {}
        Value::Bool(b) => {
            sheet.write_boolean(row, col, *b)?;
        }
        Value::Int(v) if v.unsigned_abs() <= MAX_SAFE_INTEGER => {
            sheet.write_number(row, col, *v as f64)?;
        }
        Value::UInt(v) if *v <= MAX_SAFE_INTEGER => {
            sheet.write_number(row, col, *v as f64)?;
        }
        Value::Float(v) if v.is_finite() => {
            sheet.write_number(row, col, *v)?;
        }
        Value::Decimal(text) => match exact_decimal(text) {
            Some(number) => {
                sheet.write_number(row, col, number)?;
            }
            None => {
                sheet.write_string(row, col, fit(text, cut_cells))?;
            }
        },
        Value::Date(date) if date.year() >= FIRST_YEAR => {
            sheet.write_datetime_with_format(row, col, date, &formats.date)?;
        }
        Value::Timestamp(timestamp) if timestamp.year() >= FIRST_YEAR => {
            sheet.write_datetime_with_format(row, col, timestamp, &formats.datetime)?;
        }
        Value::Time(time) => {
            sheet.write_datetime_with_format(row, col, time, &formats.time)?;
        }
        other => {
            sheet.write_string(row, col, fit(&cell_text(other), cut_cells))?;
        }
    }
    Ok(())
}

/// A decimal with at most 15 significant digits is exact as an Excel number.
fn exact_decimal(text: &str) -> Option<f64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let is_plain = !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && digits.matches('.').count() <= 1;
    if !is_plain {
        return None;
    }
    let significant = digits
        .chars()
        .filter(char::is_ascii_digit)
        .skip_while(|c| *c == '0')
        .count();
    if significant > MAX_EXACT_DIGITS {
        return None;
    }
    text.parse().ok()
}

/// `text` cut to what one cell holds. A cut is counted so the user is told.
fn fit<'a>(text: &'a str, cut_cells: &mut u64) -> &'a str {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        units += ch.len_utf16();
        if units > MAX_CELL_CHARS {
            *cut_cells += 1;
            return &text[..index];
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::{Cursor, Read};

    use super::*;
    use crate::test_util::{Ansi, columns};
    use crate::{Format as ExportFormat, Writer as AnyWriter, format_info};

    /// The files of the .xlsx that hold sheets, by name, as text.
    fn sheets(bytes: Vec<u8>) -> BTreeMap<String, String> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut found = BTreeMap::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().unwrap().into_owned();
            if name.starts_with("xl/worksheets/sheet") {
                let mut text = String::new();
                file.read_to_string(&mut text).unwrap();
                found.insert(name, text);
            }
        }
        found
    }

    fn write(options: &Options, names: &[&str], batches: &[Vec<Vec<Value>>]) -> (Summary, Vec<u8>) {
        let mut writer = AnyWriter::start(
            ExportFormat::Xlsx,
            options,
            &Ansi,
            Vec::new(),
            &columns(names),
        )
        .unwrap();
        for batch in batches {
            writer.write_batch(batch).unwrap();
        }
        let summary = writer.summary();
        (summary, writer.finish().unwrap())
    }

    fn row_count(sheet: &str) -> usize {
        sheet.matches("<row ").count()
    }

    /// The XML of one cell, e.g. `A2`, or `None` when the sheet has no such cell.
    fn cell<'a>(sheet: &'a str, reference: &str) -> Option<&'a str> {
        let start = sheet.find(&format!("<c r=\"{reference}\""))?;
        let end = sheet[start..].find("</c>")? + start + "</c>".len();
        Some(&sheet[start..end])
    }

    /// The number in a cell's `<v>`.
    fn value_of(cell: &str) -> f64 {
        let after = cell.split("<v>").nth(1).unwrap();
        after.split("</v>").next().unwrap().parse().unwrap()
    }

    fn only_sheet(bytes: Vec<u8>) -> String {
        let mut found = sheets(bytes);
        assert_eq!(found.len(), 1);
        found.pop_first().unwrap().1
    }

    fn row_of(values: Vec<Value>) -> Vec<Vec<Value>> {
        vec![values]
    }

    fn text(value: &str) -> Value {
        Value::Text(value.into())
    }

    #[test]
    fn cells_are_typed_and_null_leaves_the_cell_empty() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let batches = vec![row_of(vec![
            Value::Int(1),
            text("hi"),
            Value::Bool(true),
            Value::Decimal("0.1".into()),
            Value::Date(date),
            Value::Null,
        ])];
        let (_, bytes) = write(
            &Options::default(),
            &["a", "b", "c", "d", "e", "f"],
            &batches,
        );
        let sheet = only_sheet(bytes);
        assert!(cell(&sheet, "A1").unwrap().contains("<t>a</t>"));
        assert!(
            cell(&sheet, "A1").unwrap().contains(" s=\"1\""),
            "header is bold"
        );
        assert_eq!(cell(&sheet, "A2").unwrap(), "<c r=\"A2\"><v>1</v></c>");
        assert!(cell(&sheet, "B2").unwrap().contains("<t>hi</t>"));
        assert!(cell(&sheet, "C2").unwrap().contains("t=\"b\""));
        assert_eq!(cell(&sheet, "D2").unwrap(), "<c r=\"D2\"><v>0.1</v></c>");
        assert!(
            cell(&sheet, "E2").unwrap().contains("<v>46316</v>"),
            "a real date"
        );
        assert!(cell(&sheet, "F2").is_none());
        assert!(
            sheet.contains("state=\"frozen\""),
            "the header row stays in view"
        );
    }

    #[test]
    fn covers_ae7_a_long_result_continues_on_new_sheets_with_the_header_repeated() {
        let rows: Vec<Vec<Value>> = (1..=7).map(|n| vec![Value::Int(n)]).collect();
        let options = Options {
            max_rows_per_sheet: Some(3),
            ..Options::default()
        };
        let (summary, bytes) = write(&options, &["n"], &[rows]);
        assert_eq!(
            summary,
            Summary {
                rows: 7,
                sheets: 3,
                cut_cells: 0
            }
        );
        let found = sheets(bytes);
        let counts: Vec<usize> = found.values().map(|sheet| row_count(sheet)).collect();
        assert_eq!(counts, vec![4, 4, 2], "header plus 3, 3 and 1 rows");
        for sheet in found.values() {
            assert!(cell(sheet, "A1").unwrap().contains("<t>n</t>"));
        }
        assert!(
            cell(&found["xl/worksheets/sheet3.xml"], "A2")
                .unwrap()
                .contains("<v>7</v>")
        );
    }

    #[test]
    fn a_sheet_that_fills_exactly_does_not_start_an_empty_one() {
        let rows: Vec<Vec<Value>> = (1..=6).map(|n| vec![Value::Int(n)]).collect();
        let options = Options {
            max_rows_per_sheet: Some(3),
            ..Options::default()
        };
        let (summary, _) = write(&options, &["n"], &[rows]);
        assert_eq!(summary.sheets, 2);
    }

    #[test]
    fn a_result_with_no_rows_is_one_sheet_with_the_header() {
        let (summary, bytes) = write(&Options::default(), &["id", "name"], &[]);
        assert_eq!(
            summary,
            Summary {
                rows: 0,
                sheets: 1,
                cut_cells: 0
            }
        );
        let sheet = only_sheet(bytes);
        assert_eq!(row_count(&sheet), 1);
    }

    #[test]
    fn a_sheet_holds_what_excel_holds_unless_asked_for_less() {
        let names = columns(&["a"]);
        let rows_per_sheet = |options: &Options| {
            Writer::start(options, Vec::new(), &names)
                .unwrap()
                .rows_per_sheet
        };
        assert_eq!(rows_per_sheet(&Options::default()), 1_048_575);
        let no_header = Options {
            has_header: false,
            ..Options::default()
        };
        assert_eq!(rows_per_sheet(&no_header), 1_048_576);
        let too_many = Options {
            max_rows_per_sheet: Some(5_000_000),
            ..Options::default()
        };
        assert_eq!(rows_per_sheet(&too_many), 1_048_575);
        let none = Options {
            max_rows_per_sheet: Some(0),
            ..Options::default()
        };
        assert_eq!(rows_per_sheet(&none), 1);
    }

    #[test]
    fn decimals_are_numbers_only_when_excel_keeps_every_digit() {
        let decimals = [
            ("0.1", true),
            ("-1.50", true),
            ("123456789012345", true),
            ("0.000000000000001", true),
            ("1234567890123456", false),
            ("12345678901234567.89", false),
            ("1e5", false),
            ("+5", false),
            ("1.2.3", false),
        ];
        let row: Vec<Value> = decimals
            .iter()
            .map(|(d, _)| Value::Decimal((*d).into()))
            .collect();
        let names: Vec<String> = (0..decimals.len()).map(|n| format!("c{n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let (_, bytes) = write(&Options::default(), &names, &[row_of(row)]);
        let sheet = only_sheet(bytes);
        for (index, (decimal, is_number)) in decimals.iter().enumerate() {
            let reference = format!("{}2", (b'A' + index as u8) as char);
            let xml = cell(&sheet, &reference).unwrap();
            if *is_number {
                assert!(
                    !xml.contains("inlineStr"),
                    "{decimal} should be a number: {xml}"
                );
            } else {
                assert!(
                    xml.contains(&format!("<t>{decimal}</t>")),
                    "{decimal} kept as text: {xml}"
                );
            }
        }
    }

    #[test]
    fn integers_beyond_what_excel_holds_exactly_stay_text_with_every_digit() {
        let row = vec![
            Value::Int(9_007_199_254_740_991),
            Value::Int(9_007_199_254_740_992),
            Value::Int(i64::MIN),
            Value::UInt(u64::MAX),
        ];
        let (_, bytes) = write(&Options::default(), &["a", "b", "c", "d"], &[row_of(row)]);
        let sheet = only_sheet(bytes);
        assert_eq!(
            cell(&sheet, "A2").unwrap(),
            "<c r=\"A2\"><v>9007199254740991</v></c>"
        );
        assert!(
            cell(&sheet, "B2")
                .unwrap()
                .contains("<t>9007199254740992</t>")
        );
        assert!(
            cell(&sheet, "C2")
                .unwrap()
                .contains("<t>-9223372036854775808</t>")
        );
        assert!(
            cell(&sheet, "D2")
                .unwrap()
                .contains("<t>18446744073709551615</t>")
        );
    }

    #[test]
    fn floats_excel_cannot_hold_become_text() {
        let row = vec![
            Value::Float(1.5),
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
        ];
        let (_, bytes) = write(&Options::default(), &["a", "b", "c"], &[row_of(row)]);
        let sheet = only_sheet(bytes);
        assert_eq!(cell(&sheet, "A2").unwrap(), "<c r=\"A2\"><v>1.5</v></c>");
        assert!(cell(&sheet, "B2").unwrap().contains("<t>NaN</t>"));
        assert!(cell(&sheet, "C2").unwrap().contains("<t>Infinity</t>"));
    }

    #[test]
    fn dates_before_excels_calendar_stay_text() {
        let old = chrono::NaiveDate::from_ymd_opt(1850, 1, 1).unwrap();
        let time = chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        let stamp = chrono::NaiveDate::from_ymd_opt(2026, 10, 21)
            .unwrap()
            .and_hms_opt(18, 30, 0)
            .unwrap();
        let row = vec![Value::Date(old), Value::Time(time), Value::Timestamp(stamp)];
        let (_, bytes) = write(&Options::default(), &["d", "t", "s"], &[row_of(row)]);
        let sheet = only_sheet(bytes);
        assert!(cell(&sheet, "A2").unwrap().contains("<t>1850-01-01</t>"));
        assert!(cell(&sheet, "B2").unwrap().contains("<v>0.5</v>"));
        let serial = value_of(cell(&sheet, "C2").unwrap());
        assert!((serial - 46316.770833333).abs() < 1e-6, "{serial}");
    }

    #[test]
    fn a_formula_looking_text_stays_text() {
        let (_, bytes) = write(&Options::default(), &["a"], &[row_of(vec![text("=1+1")])]);
        let sheet = only_sheet(bytes);
        let xml = cell(&sheet, "A2").unwrap();
        assert!(xml.contains("<t>=1+1</t>") && !xml.contains("<f>"), "{xml}");
    }

    #[test]
    fn text_past_excels_cell_limit_is_cut_and_counted() {
        let long = "a".repeat(40_000);
        let emoji = "😀".repeat(20_000);
        let row = vec![text(&long), text(&emoji), text("short")];
        let (summary, bytes) = write(&Options::default(), &["a", "b", "c"], &[row_of(row)]);
        assert_eq!(summary.cut_cells, 2);
        let sheet = only_sheet(bytes);
        let inner = |xml: &str| {
            let after = xml.split("<t>").nth(1).unwrap();
            after.split("</t>").next().unwrap().to_owned()
        };
        assert_eq!(inner(cell(&sheet, "A2").unwrap()).len(), 32_767);
        assert_eq!(
            inner(cell(&sheet, "B2").unwrap()).chars().count(),
            16_383,
            "two UTF-16 units each"
        );
        assert_eq!(inner(cell(&sheet, "C2").unwrap()), "short");
    }

    #[test]
    fn no_header_means_data_from_the_first_row() {
        let options = Options {
            has_header: false,
            ..Options::default()
        };
        let (_, bytes) = write(&options, &["a"], &[row_of(vec![Value::Int(7)])]);
        let sheet = only_sheet(bytes);
        assert_eq!(cell(&sheet, "A1").unwrap(), "<c r=\"A1\"><v>7</v></c>");
        assert!(!sheet.contains("state=\"frozen\""));
    }

    #[test]
    fn more_columns_than_excel_holds_is_refused_with_a_reason() {
        let names: Vec<String> = (0..=MAX_COLUMNS).map(|n| format!("c{n}")).collect();
        let columns: Vec<_> = names.iter().map(|n| crate::test_util::column(n)).collect();
        let err = Writer::start(&Options::default(), Vec::new(), &columns).unwrap_err();
        assert!(matches!(err, ExportError::TooManyColumns(16_385)));
        assert!(err.to_string().contains("Select fewer columns"));
    }

    #[test]
    fn excel_has_no_preview_and_declares_its_sheet_limit() {
        let info = format_info(ExportFormat::Xlsx);
        assert!(!info.has_preview);
        assert_eq!(info.max_rows_per_sheet, Some(1_048_576));
        assert_eq!(info.extension, "xlsx");
    }

    /// Run by hand to see memory stay flat:
    /// `/usr/bin/time -l cargo test --release -p quarry-export -- --ignored million_rows`
    #[test]
    #[ignore = "writes a million rows"]
    fn million_rows_fill_a_sheet_then_continue_on_the_next() {
        let names = columns(&["id", "name", "amount"]);
        let mut writer = AnyWriter::start(
            ExportFormat::Xlsx,
            &Options::default(),
            &Ansi,
            Vec::new(),
            &names,
        )
        .unwrap();
        for start in (0..1_200_000i64).step_by(10_000) {
            let batch: Vec<Vec<Value>> = (start..start + 10_000)
                .map(|n| {
                    vec![
                        Value::Int(n),
                        text("some text for the row"),
                        Value::Decimal("12.34".into()),
                    ]
                })
                .collect();
            writer.write_batch(&batch).unwrap();
        }
        assert_eq!(writer.summary().sheets, 2);
        let found = sheets(writer.finish().unwrap());
        let counts: Vec<usize> = found.values().map(|sheet| row_count(sheet)).collect();
        assert_eq!(counts, vec![1_048_576, 1 + 1_200_000 - 1_048_575]);
    }

    /// Run by hand with different `QUARRY_EXPORT_ROWS` to see that peak memory does not follow
    /// the row count: `QUARRY_EXPORT_ROWS=4000000 /usr/bin/time -l <test binary> --ignored
    /// rows_to_a_sink`.
    #[test]
    #[ignore = "writes many rows"]
    fn rows_to_a_sink_leave_memory_flat() {
        let total: i64 = std::env::var("QUARRY_EXPORT_ROWS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1_200_000);
        let names = columns(&["id", "name", "amount"]);
        let mut writer = AnyWriter::start(
            ExportFormat::Xlsx,
            &Options::default(),
            &Ansi,
            std::io::sink(),
            &names,
        )
        .unwrap();
        for start in (0..total).step_by(10_000) {
            let batch: Vec<Vec<Value>> = (start..(start + 10_000).min(total))
                .map(|n| {
                    vec![
                        Value::Int(n),
                        text("some text for the row"),
                        Value::Decimal("12.34".into()),
                    ]
                })
                .collect();
            writer.write_batch(&batch).unwrap();
        }
        assert_eq!(writer.rows(), total as u64);
        writer.finish().unwrap();
    }
}
