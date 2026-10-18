//! SQL text helpers: [`split`] (script into statements), [`statement_at`]
//! (statement under the cursor) and [`classify`] (for safe mode).

use std::ops::Range;

/// Split a script into statements on `;`, ignoring `;` inside strings, quoted identifiers
/// and comments. Returns trimmed byte ranges; empty statements are skipped.
///
/// Handles common MySQL/PostgreSQL/SQLite syntax: `'..'` (with `\'` and `''`), `".."`,
/// `` `..` ``, `-- `, `#`, `/* */`. `DELIMITER` and `$$` are not supported yet.
pub fn split(sql: &str) -> Vec<Range<usize>> {
    segments(sql)
        .into_iter()
        .filter_map(|seg| trim(sql, seg))
        .collect()
}

/// The statement containing byte `offset`. When the cursor sits in blank space after the
/// last `;`, returns the statement just before it.
pub fn statement_at(sql: &str, offset: usize) -> Option<Range<usize>> {
    let offset = offset.min(sql.len());
    let mut previous = None;
    for seg in segments(sql) {
        let trimmed = trim(sql, seg.clone());
        // seg.end is just past the `;`, so a cursor right after `;` still belongs to this statement.
        if offset <= seg.end {
            return trimmed.or(previous);
        }
        if trimmed.is_some() {
            previous = trimmed;
        }
    }
    previous
}

/// How much a statement can change, for safe mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatementKind {
    /// SELECT, SHOW, EXPLAIN, … — changes nothing.
    Read,
    /// INSERT, UPDATE/DELETE with WHERE, CALL, …
    Write,
    /// CREATE, ALTER, GRANT, …
    Schema,
    /// DROP, TRUNCATE, UPDATE/DELETE without WHERE.
    Destructive,
}

/// Classify a statement by its first keyword (comments and parentheses are skipped).
/// Unknown keywords count as `Write`, to stay on the safe side.
pub fn classify(statement: &str) -> StatementKind {
    classify_words(&keywords(statement))
}

fn classify_words(words: &[String]) -> StatementKind {
    let Some(first) = words.first() else {
        return StatementKind::Read;
    };
    match first.as_str() {
        // `EXPLAIN ANALYZE` runs the statement it explains, so it is as risky as that statement.
        "EXPLAIN" | "DESCRIBE" | "DESC" if words.get(1).is_some_and(|w| w == "ANALYZE") => {
            let rest = &words[2..];
            let rest = if rest.first().is_some_and(|w| w == "FORMAT") {
                rest.get(2..).unwrap_or_default()
            } else {
                rest
            };
            classify_words(rest)
        }
        "SELECT" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "USE" | "HELP" | "VALUES"
        | "TABLE" => StatementKind::Read,
        // CTE: look at the main statement that follows.
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

/// The first keyword of a statement, upper-cased, skipping comments.
pub fn first_keyword(statement: &str) -> Option<String> {
    keywords(statement).into_iter().next()
}

/// How a statement changes the session's transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionEffect {
    /// `BEGIN`, `START TRANSACTION`.
    Begin,
    /// `COMMIT`, `ROLLBACK`, and statements that commit implicitly (`CREATE`, `ALTER`, `DROP`,
    /// `TRUNCATE`, `RENAME`, `GRANT`, `REVOKE`).
    End,
}

/// The effect of `statement` on an open transaction, or `None` if it does not start or end one.
/// `ROLLBACK TO SAVEPOINT` keeps the transaction open.
pub fn transaction_effect(statement: &str) -> Option<TransactionEffect> {
    let words = keywords(statement);
    let second = words.get(1).map(String::as_str);
    match words.first()?.as_str() {
        // MariaDB's `BEGIN NOT ATOMIC` starts a compound statement, not a transaction.
        "BEGIN" if second != Some("NOT") => Some(TransactionEffect::Begin),
        "START" if second == Some("TRANSACTION") => Some(TransactionEffect::Begin),
        "COMMIT" => Some(TransactionEffect::End),
        "ROLLBACK" if !words.iter().any(|w| w == "TO") => Some(TransactionEffect::End),
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" | "GRANT" | "REVOKE" => {
            Some(TransactionEffect::End)
        }
        _ => None,
    }
}

/// Upper-cased words outside strings and comments.
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

/// Raw segments between `;` (including the `;`).
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
                // `''` / `""` / ``` `` ``` is an escaped quote
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

/// Trim whitespace and the trailing `;`; `None` when only comments/whitespace remain.
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

/// Length of the leading whitespace and line comments (`--`, `#`).
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
            ("EXPLAIN DELETE FROM t", Read),
            ("explain select 1", Read),
            ("EXPLAIN ANALYZE SELECT * FROM t", Read),
            ("explain analyze delete from t", Destructive),
            (
                "EXPLAIN ANALYZE FORMAT=TREE UPDATE t SET a = 1 WHERE id = 2",
                Write,
            ),
            (
                "DESC ANALYZE DELETE a FROM a JOIN b ON a.id = b.id",
                Destructive,
            ),
            ("/* EXPLAIN ANALYZE */ SELECT 1", Read),
            ("EXPLAIN 'analyze'", Read),
            ("EXPLAIN ANALYZE", Read),
        ];
        for (sql, kind) in cases {
            assert_eq!(classify(sql), kind, "{sql}");
        }
    }

    #[test]
    fn transaction_effects() {
        use TransactionEffect::*;
        let cases = [
            ("BEGIN", Some(Begin)),
            ("begin work", Some(Begin)),
            ("START TRANSACTION READ ONLY", Some(Begin)),
            ("BEGIN NOT ATOMIC SELECT 1; END", None),
            ("commit", Some(End)),
            ("ROLLBACK", Some(End)),
            ("ROLLBACK TO SAVEPOINT a", None),
            ("rollback work to a", None),
            ("CREATE TABLE t (id INT)", Some(End)),
            ("truncate t", Some(End)),
            ("DELETE FROM t", None),
            ("SELECT 'commit'", None),
            ("-- commit\nSELECT 1", None),
            ("START SLAVE", None),
            ("", None),
        ];
        for (sql, effect) in cases {
            assert_eq!(transaction_effect(sql), effect, "{sql}");
        }
        assert_eq!(
            first_keyword("/* x */ explain select 1").as_deref(),
            Some("EXPLAIN")
        );
        assert_eq!(first_keyword("  -- only a comment"), None);
    }

    #[test]
    fn statement_at_cursor() {
        let sql = "SELECT 1;\nSELECT 2;\n\n";
        let at = |o| statement_at(sql, o).map(|r| &sql[r]);
        assert_eq!(at(0), Some("SELECT 1"));
        assert_eq!(at(8), Some("SELECT 1")); // just before `;`
        assert_eq!(at(9), Some("SELECT 1")); // ngay sau `;`
        assert_eq!(at(12), Some("SELECT 2"));
        assert_eq!(at(sql.len()), Some("SELECT 2")); // trailing blank line
        assert_eq!(statement_at("", 0), None);
    }
}
