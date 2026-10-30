//! SQL text helpers: [`split`] (script into statements), [`script`] (the statements a run
//! sends), [`statement_at`] (statement under the cursor), [`classify`] (for safe mode) and
//! [`session_state`] (what a statement needs from its own session).

use std::ops::Range;

mod completion;
mod session;

pub use completion::completion_context;
pub use quarry_model::{CompletionContext, CompletionNames, StatementTable};
pub use session::{SessionState, session_state};

/// Split a script into statements on `;`, ignoring `;` inside strings, quoted identifiers
/// and comments. Returns trimmed byte ranges; empty statements are skipped.
///
/// Handles common MySQL/PostgreSQL/SQLite syntax: `'..'` (with `\'` and `''`), `".."`,
/// `` `..` ``, `-- `, `#`, `/* */`, and `DELIMITER` lines: a line
/// `DELIMITER //` makes `//` end the statements that follow, and is not a statement itself.
/// `$$` quoting is not supported yet.
pub fn split(sql: &str) -> Vec<Range<usize>> {
    scan(sql)
        .segments
        .into_iter()
        .filter_map(|seg| trim(sql, seg))
        .collect()
}

/// The statements a run sends one by one, in order: [`split`], or the whole script as one
/// statement when it holds a compound statement (a routine, trigger or event with a
/// `BEGIN … END` body, or `BEGIN NOT ATOMIC`). The server reads the `;` inside such a body as
/// part of it; `split` cannot without `DELIMITER`, so the script goes as it was written.
/// A script with `DELIMITER` lines is always split: the server does not know that command.
pub fn script(sql: &str) -> Vec<Range<usize>> {
    if scan(sql).has_delimiter {
        return split(sql);
    }
    let statements = split(sql);
    let is_compound = |range: &Range<usize>| {
        let words = keywords(&sql[range.clone()]);
        match words.first().map(String::as_str) {
            Some("CREATE") => words.iter().any(|w| w == "BEGIN"),
            Some("BEGIN") => words.get(1).is_some_and(|w| w == "NOT"),
            _ => false,
        }
    };
    if statements.len() > 1 && statements.iter().any(is_compound) {
        let whole = Segment {
            range: 0..sql.len(),
            terminator: if sql.trim_end().ends_with(';') { 1 } else { 0 },
        };
        return trim(sql, whole).into_iter().collect();
    }
    statements
}

/// The statement containing byte `offset`. When the cursor sits in blank space after the
/// last `;`, returns the statement just before it.
pub fn statement_at(sql: &str, offset: usize) -> Option<Range<usize>> {
    let offset = offset.min(sql.len());
    let mut previous = None;
    for seg in scan(sql).segments {
        let end = seg.range.end;
        let trimmed = trim(sql, seg);
        // seg.end is just past the `;`, so a cursor right after `;` still belongs to this statement.
        if offset <= end {
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
        if let Some(next) = skip_literal(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
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

/// A statement's raw text, up to and including its terminator.
pub(crate) struct Segment {
    pub(crate) range: Range<usize>,
    /// Bytes of the terminator at the end of `range` (0 for the last, unterminated statement).
    pub(crate) terminator: usize,
}

pub(crate) struct Scan {
    pub(crate) segments: Vec<Segment>,
    pub(crate) has_delimiter: bool,
}

/// Raw segments between terminators, following `DELIMITER` lines.
pub(crate) fn scan(sql: &str) -> Scan {
    let bytes = sql.as_bytes();
    let mut segments = Vec::new();
    let mut has_delimiter = false;
    let mut delimiter: &[u8] = b";";
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if (i == 0 || bytes[i - 1] == b'\n')
            && let Some((token, line_end)) = delimiter_line(sql, i)
        {
            // The line ends the statement before it, if any.
            if start < i {
                segments.push(Segment {
                    range: start..i,
                    terminator: 0,
                });
            }
            has_delimiter = true;
            if !token.is_empty() {
                delimiter = token.as_bytes();
            }
            i = line_end;
            start = i;
            continue;
        }
        if let Some(next) = skip_literal(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            _ if bytes[i..].starts_with(delimiter) => {
                i += delimiter.len();
                segments.push(Segment {
                    range: start..i,
                    terminator: delimiter.len(),
                });
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        segments.push(Segment {
            range: start..bytes.len(),
            terminator: 0,
        });
    }
    Scan {
        segments,
        has_delimiter,
    }
}

/// A `DELIMITER <token>` line starting at byte `line`: the token and where the next line starts.
fn delimiter_line(sql: &str, line: usize) -> Option<(&str, usize)> {
    const WORD: &str = "DELIMITER";
    let line_end = sql[line..].find('\n').map_or(sql.len(), |nl| line + nl + 1);
    let text = sql[line..line_end].trim_start_matches([' ', '\t']);
    let rest = text.get(WORD.len()..)?;
    if !text[..WORD.len()].eq_ignore_ascii_case(WORD) || !rest.starts_with([' ', '\t']) {
        return None;
    }
    let token = rest.split_whitespace().next().unwrap_or_default();
    Some((token, line_end))
}

/// Where a string, a quoted name or a comment starting at `i` ends; `None` when none starts
/// there. Every scan that must not look inside them steps over them with this.
pub(crate) fn skip_literal(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes.get(i)? {
        quote @ (b'\'' | b'"' | b'`') => Some(skip_quoted(bytes, i, *quote)),
        b'-' if bytes.get(i + 1) == Some(&b'-') => Some(skip_line(bytes, i)),
        b'#' => Some(skip_line(bytes, i)),
        b'/' if bytes.get(i + 1) == Some(&b'*') => {
            Some(find(bytes, i + 2, b"*/").map_or(bytes.len(), |end| end + 2))
        }
        _ => None,
    }
}

fn skip_quoted(bytes: &[u8], open: usize, quote: u8) -> usize {
    quoted_end(bytes, open, quote).0
}

/// The end of the quoted text opening at `open`, and whether it closed. A doubled quote
/// (`''`, `""`, ``` `` ```) or a backslash escape stays inside.
pub(crate) fn quoted_end(bytes: &[u8], open: usize, quote: u8) -> (usize, bool) {
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote != b'`' => i += 2,
            b if b == quote => {
                if bytes.get(i + 1) == Some(&quote) {
                    i += 2;
                } else {
                    return (i + 1, true);
                }
            }
            _ => i += 1,
        }
    }
    (bytes.len(), false)
}

fn skip_line(bytes: &[u8], from: usize) -> usize {
    find(bytes, from, b"\n").map_or(bytes.len(), |nl| nl + 1)
}

pub(crate) fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Trim whitespace and the terminator; `None` when only comments/whitespace remain.
fn trim(sql: &str, seg: Segment) -> Option<Range<usize>> {
    let seg = seg.range.start..seg.range.end - seg.terminator;
    let text = &sql[seg.clone()];
    let text_no_semi = text.trim_end();
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
    fn delimiter_lines_change_the_terminator() {
        let sql = "DELIMITER //\nCREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\nEND //\n\
                   delimiter ;\nCALL p(); SELECT ';//'";
        let body = "CREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\nEND";
        assert_eq!(texts(sql), [body, "CALL p()", "SELECT ';//'"]);
        // The server does not know `DELIMITER`: such a script always runs statement by statement.
        let run: Vec<&str> = script(sql).into_iter().map(|r| &sql[r]).collect();
        assert_eq!(run, [body, "CALL p()", "SELECT ';//'"]);
        let inside = sql.find("SELECT 1").unwrap();
        assert_eq!(statement_at(sql, inside).map(|r| &sql[r]), Some(body));
        // A delimiter of several characters, and a line that only mentions the word.
        assert_eq!(
            texts("DELIMITER $$\nSELECT 1$$ SELECT 2 $$"),
            ["SELECT 1", "SELECT 2"]
        );
        assert_eq!(
            texts("SELECT 'DELIMITER x';\nSELECT delimiter_x"),
            ["SELECT 'DELIMITER x'", "SELECT delimiter_x"]
        );
        // Malformed lines never panic.
        for odd in [
            "DELIMITER",
            "DELIMITERé x",
            "DELIMITER \n",
            "ĐELIMITER x\nSELECT 1",
            "x\nDELIMITER 👋\nSELECT 1👋",
        ] {
            let _ = (split(odd), script(odd), statement_at(odd, odd.len()));
        }
        // The line ends an unterminated statement before it.
        assert_eq!(
            texts("SELECT 1\nDELIMITER //\nSELECT 2//"),
            ["SELECT 1", "SELECT 2"]
        );
    }

    #[test]
    fn script_keeps_compound_statements_whole() {
        let run = |sql| script(sql).into_iter().map(|r| &sql[r]).collect::<Vec<_>>();
        assert_eq!(run("SELECT 1; SELECT 2;"), ["SELECT 1", "SELECT 2"]);
        let routine = "CREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\n  SELECT 2;\nEND;\nCALL p()";
        assert_eq!(run(routine), [routine]);
        let block = "BEGIN NOT ATOMIC SELECT 1; END";
        assert_eq!(run(block), [block]);
        assert_eq!(
            run("BEGIN; SELECT 1; COMMIT"),
            ["BEGIN", "SELECT 1", "COMMIT"]
        );
        assert_eq!(run("CREATE TABLE t (begin_at INT); SELECT 1").len(), 2);
        assert_eq!(run("-- nothing"), Vec::<&str>::new());
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
