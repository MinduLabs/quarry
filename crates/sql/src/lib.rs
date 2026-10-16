//! Tiện ích xử lý văn bản SQL.
//!
//! Đã có: [`split`] (tách script thành statement), [`statement_at`]
//! (statement tại vị trí con trỏ) và [`classify`] (cho safe mode).
//! Sẽ có: ngữ cảnh autocomplete, format. Xem `docs/architecture/ARCHITECTURE.md` §2.

use std::ops::Range;

/// Tách script thành các statement (theo `;`), bỏ qua `;` nằm trong chuỗi,
/// identifier có quote và comment. Trả về byte range đã trim khoảng trắng;
/// statement rỗng bị bỏ qua.
///
/// Hỗ trợ cú pháp MySQL/PostgreSQL/SQLite phổ biến: `'..'` (cả `\'` và `''`),
/// `".."`, `` `..` ``, `-- `, `#`, `/* */`. Chưa hỗ trợ `DELIMITER` và `$$`.
pub fn split(sql: &str) -> Vec<Range<usize>> {
    segments(sql)
        .into_iter()
        .filter_map(|seg| trim(sql, seg))
        .collect()
}

/// Statement chứa vị trí `offset` (byte). Nếu con trỏ nằm sau dấu `;` cuối
/// trên phần trống, trả về statement ngay trước đó.
pub fn statement_at(sql: &str, offset: usize) -> Option<Range<usize>> {
    let offset = offset.min(sql.len());
    let mut previous = None;
    for seg in segments(sql) {
        let trimmed = trim(sql, seg.clone());
        // seg.end trỏ ngay sau `;` (nếu có) nên con trỏ đứng sát `;` vẫn thuộc statement này.
        if offset <= seg.end {
            return trimmed.or(previous);
        }
        if trimmed.is_some() {
            previous = trimmed;
        }
    }
    previous
}

/// Mức độ ảnh hưởng của một statement, dùng cho safe mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatementKind {
    /// SELECT, SHOW, EXPLAIN, … — không thay đổi dữ liệu.
    Read,
    /// INSERT, UPDATE/DELETE có WHERE, CALL, …
    Write,
    /// CREATE, ALTER, GRANT, …
    Schema,
    /// DROP, TRUNCATE, UPDATE/DELETE không có WHERE.
    Destructive,
}

/// Phân loại một statement theo từ khoá đầu tiên (bỏ qua comment và dấu ngoặc).
/// Từ khoá lạ được xếp vào `Write` để an toàn.
pub fn classify(statement: &str) -> StatementKind {
    let words = keywords(statement);
    let Some(first) = words.first() else {
        return StatementKind::Read;
    };
    match first.as_str() {
        "SELECT" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "USE" | "HELP" | "VALUES"
        | "TABLE" => StatementKind::Read,
        // CTE: xét câu lệnh chính phía sau.
        "WITH" => {
            if words
                .iter()
                .any(|w| matches!(w.as_str(), "UPDATE" | "DELETE" | "INSERT" | "REPLACE"))
            {
                StatementKind::Write
            } else {
                StatementKind::Read
            }
        }
        "DROP" | "TRUNCATE" => StatementKind::Destructive,
        "UPDATE" | "DELETE" if !words.iter().any(|w| w == "WHERE") => StatementKind::Destructive,
        "CREATE" | "ALTER" | "RENAME" | "GRANT" | "REVOKE" => StatementKind::Schema,
        _ => StatementKind::Write,
    }
}

/// Các từ (viết hoa) ngoài chuỗi và comment.
fn keywords(statement: &str) -> Vec<String> {
    let bytes = statement.as_bytes();
    let mut words = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"' | b'`') => i = skip_quoted(bytes, i, quote),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_line(bytes, i),
            b'#' => i = skip_line(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = find(bytes, i + 2, b"*/").map_or(bytes.len(), |end| end + 2);
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                words.push(statement[start..i].to_ascii_uppercase());
            }
            _ => i += 1,
        }
    }
    words
}

/// Các đoạn thô giữa các `;` (bao gồm cả `;`).
fn segments(sql: &str) -> Vec<Range<usize>> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"' | b'`') => i = skip_quoted(bytes, i, quote),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_line(bytes, i),
            b'#' => i = skip_line(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = find(bytes, i + 2, b"*/").map_or(bytes.len(), |end| end + 2);
            }
            b';' => {
                i += 1;
                out.push(start..i);
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        out.push(start..bytes.len());
    }
    out
}

fn skip_quoted(bytes: &[u8], open: usize, quote: u8) -> usize {
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote != b'`' => i += 2,
            b if b == quote => {
                // `''` / `""` / ``` `` ``` là quote được escape
                if bytes.get(i + 1) == Some(&quote) {
                    i += 2;
                } else {
                    return i + 1;
                }
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_line(bytes: &[u8], from: usize) -> usize {
    find(bytes, from, b"\n").map_or(bytes.len(), |nl| nl + 1)
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Bỏ khoảng trắng và dấu `;` ở cuối; `None` nếu không còn gì ngoài comment/khoảng trắng.
fn trim(sql: &str, seg: Range<usize>) -> Option<Range<usize>> {
    let text = &sql[seg.clone()];
    let text_no_semi = text.trim_end().strip_suffix(';').unwrap_or(text);
    let start = seg.start + leading_noise(text_no_semi);
    let end = seg.start + text_no_semi.trim_end().len();
    if start >= end || only_comments(&sql[start..end]) {
        return None;
    }
    Some(start..end)
}

/// Độ dài phần khoảng trắng và comment dòng (`--`, `#`) ở đầu đoạn.
fn leading_noise(text: &str) -> usize {
    let mut rest = text;
    loop {
        let trimmed = rest.trim_start();
        if trimmed.starts_with("--") || trimmed.starts_with('#') {
            rest = trimmed.find('\n').map_or("", |nl| &trimmed[nl + 1..]);
        } else {
            return text.len() - trimmed.len();
        }
    }
}

fn only_comments(text: &str) -> bool {
    text.lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with("--") || line.starts_with('#')
    }) && !text.contains("/*")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(sql: &str) -> Vec<&str> {
        split(sql).into_iter().map(|r| &sql[r]).collect()
    }

    #[test]
    fn splits_simple_script() {
        assert_eq!(
            texts("SELECT 1; SELECT 2;\n\nSELECT 3"),
            ["SELECT 1", "SELECT 2", "SELECT 3"]
        );
    }

    #[test]
    fn ignores_semicolons_in_strings_identifiers_and_comments() {
        let sql = "SELECT 'a;b', \"c;d\", `e;f` -- x;y\n FROM t /* ; */; # z;\nSELECT 'it''s;' ;";
        assert_eq!(
            texts(sql),
            [
                "SELECT 'a;b', \"c;d\", `e;f` -- x;y\n FROM t /* ; */",
                "SELECT 'it''s;'"
            ]
        );
    }

    #[test]
    fn handles_backslash_escape() {
        assert_eq!(
            texts(r"SELECT 'a\';b'; SELECT 2"),
            [r"SELECT 'a\';b'", "SELECT 2"]
        );
    }

    #[test]
    fn skips_empty_and_comment_only_statements() {
        assert_eq!(texts(";;  -- just a comment\n;SELECT 1"), ["SELECT 1"]);
    }

    #[test]
    fn classifies_statements() {
        use StatementKind::*;
        let cases = [
            ("select * from t", Read),
            ("/* x */ (SELECT 1)", Read),
            ("SHOW TABLES", Read),
            ("WITH a AS (SELECT 1) SELECT * FROM a", Read),
            ("insert into t values (1)", Write),
            ("UPDATE t SET a = 1 WHERE id = 2", Write),
            ("update t set a = 'where'", Destructive),
            ("DELETE FROM t", Destructive),
            ("delete from t -- where id = 1", Destructive),
            ("DROP TABLE t", Destructive),
            ("truncate t", Destructive),
            ("ALTER TABLE t ADD c INT", Schema),
            ("OPTIMIZE TABLE t", Write),
            ("", Read),
        ];
        for (sql, kind) in cases {
            assert_eq!(classify(sql), kind, "{sql}");
        }
    }

    #[test]
    fn statement_at_cursor() {
        let sql = "SELECT 1;\nSELECT 2;\n\n";
        let at = |o| statement_at(sql, o).map(|r| &sql[r]);
        assert_eq!(at(0), Some("SELECT 1"));
        assert_eq!(at(8), Some("SELECT 1")); // ngay trước `;`
        assert_eq!(at(9), Some("SELECT 1")); // ngay sau `;`
        assert_eq!(at(12), Some("SELECT 2"));
        assert_eq!(at(sql.len()), Some("SELECT 2")); // dòng trống cuối
        assert_eq!(statement_at("", 0), None);
    }
}
