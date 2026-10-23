//! What fits at the cursor of a SQL editor: the names (tables, columns, databases) and the
//! keywords the statement around it allows there. Read from tokens, linear in the statement's
//! length, so it can run on every keystroke.

use std::ops::Range;

use quarry_model::{CompletionContext, CompletionNames, StatementTable};

/// The completion context at byte `cursor` of `sql`, or `None` where nothing completes: inside a
/// string, a quoted name or a comment, on a `DELIMITER` line, or after a dot that follows no name.
pub fn completion_context(sql: &str, cursor: usize) -> Option<CompletionContext> {
    let cursor = floor_char_boundary(sql, cursor);
    let statement = statement_around(sql, cursor)?;
    let text = &sql[statement.clone()];
    let cursor_in = cursor - statement.start;
    let tokens = lex(text);
    if tokens.iter().any(|t| t.encloses(cursor_in)) {
        return None;
    }
    let word_start = word_start(text, cursor_in);
    let code: Vec<Lexeme<'_>> = tokens
        .iter()
        .filter(|t| !t.kind.is_comment())
        .map(|t| Lexeme::new(text, t))
        .collect();
    let before: Vec<&Lexeme<'_>> = code.iter().filter(|l| l.end <= word_start).collect();
    let tables = collect_tables(&code);

    let after_dot = text[..word_start].ends_with('.');
    let names = if after_dot {
        after_dot_names(&before, &tables)?
    } else if is_table_slot(&before) {
        CompletionNames::Objects { database: None }
    } else if before.is_empty() {
        CompletionNames::Keywords
    } else {
        CompletionNames::Expression
    };

    let clause = clause_at(&before);
    let expects_name = !matches!(
        names,
        CompletionNames::Keywords | CompletionNames::Expression
    ) || (matches!(names, CompletionNames::Expression)
        && is_operand_slot(&before));
    let (keywords, functions) = if after_dot {
        (Vec::new(), Vec::new())
    } else {
        keywords_at(&before, clause, &names, expects_name)
    };

    Some(CompletionContext {
        word_start: u32::try_from(statement.start + word_start).unwrap_or(u32::MAX),
        names,
        tables,
        keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
        functions: functions.iter().map(|f| (*f).to_owned()).collect(),
        expects_name,
    })
}

fn floor_char_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// The statement holding `cursor`, without its terminator. After a terminator a new, empty
/// statement starts; a `DELIMITER` line belongs to no statement.
fn statement_around(sql: &str, cursor: usize) -> Option<Range<usize>> {
    let segments = crate::scan(sql).segments;
    let Some(segment) = segments
        .iter()
        .find(|s| s.range.start <= cursor && cursor < s.range.end)
        .or_else(|| {
            segments
                .last()
                .filter(|s| cursor == s.range.end && s.terminator == 0)
        })
    else {
        // Past the last terminator, or an empty text: a statement not begun yet.
        let line_start = sql[..cursor].rfind('\n').map_or(0, |nl| nl + 1);
        let is_directive = sql[line_start..].trim_start().len() >= 9
            && sql[line_start..].trim_start()[..9].eq_ignore_ascii_case("DELIMITER");
        return (!is_directive).then_some(cursor..cursor);
    };
    let end = segment.range.end - segment.terminator;
    if cursor > end || (segment.terminator > 0 && cursor == segment.range.end) {
        return Some(cursor..cursor);
    }
    Some(segment.range.start..end)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Where the word ending at `cursor` starts.
fn word_start(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map_or(cursor, |(i, _)| i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    /// `` `name` ``, with `closed` false when the text ends inside it.
    Quoted {
        closed: bool,
    },
    String {
        closed: bool,
    },
    Number,
    Punct,
    LineComment,
    BlockComment {
        closed: bool,
    },
}

impl Kind {
    fn is_comment(self) -> bool {
        matches!(self, Kind::LineComment | Kind::BlockComment { .. })
    }
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    range: Range<usize>,
}

impl Token {
    /// Whether the cursor is inside a token nothing completes in. An open token and a line
    /// comment run to their end, so the end counts as inside.
    fn encloses(&self, cursor: usize) -> bool {
        let inside = self.range.start < cursor && cursor < self.range.end;
        let at_end = self.range.start < cursor && cursor == self.range.end;
        match self.kind {
            Kind::LineComment
            | Kind::Quoted { closed: false }
            | Kind::String { closed: false }
            | Kind::BlockComment { closed: false } => inside || at_end,
            Kind::Quoted { .. } | Kind::String { .. } | Kind::BlockComment { .. } => inside,
            Kind::Word | Kind::Number | Kind::Punct => false,
        }
    }
}

/// MySQL tokens: `'…'` and `"…"` strings (with `\` escapes and doubled quotes), `` `…` `` names,
/// `-- ` and `#` line comments, `/* */` block comments, numbers, words and punctuation.
fn lex(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let b = bytes[i];
        let kind = match b {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            b'#' => {
                i = line_end(bytes, i);
                Kind::LineComment
            }
            b'-' if bytes.get(i + 1) == Some(&b'-')
                && bytes.get(i + 2).is_none_or(|c| c.is_ascii_whitespace()) =>
            {
                i = line_end(bytes, i);
                Kind::LineComment
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => match find(bytes, i + 2, b"*/") {
                Some(end) => {
                    i = end + 2;
                    Kind::BlockComment { closed: true }
                }
                None => {
                    i = bytes.len();
                    Kind::BlockComment { closed: false }
                }
            },
            quote @ (b'\'' | b'"' | b'`') => {
                let (end, closed) = quoted_end(bytes, i, quote);
                i = end;
                if quote == b'`' {
                    Kind::Quoted { closed }
                } else {
                    Kind::String { closed }
                }
            }
            b'0'..=b'9' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                    i += 1;
                }
                Kind::Number
            }
            _ if b.is_ascii_alphabetic() || b == b'_' || b == b'$' || b >= 0x80 => {
                while i < bytes.len() {
                    let c = bytes[i];
                    if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80 {
                        i += 1;
                    } else {
                        break;
                    }
                }
                Kind::Word
            }
            _ => {
                let rest = &text[i..];
                let len = ["<=>", "<=", ">=", "<>", "!=", ":=", "||", "&&"]
                    .iter()
                    .find(|op| rest.starts_with(**op))
                    .map_or_else(
                        || rest.chars().next().map_or(1, char::len_utf8),
                        |op| op.len(),
                    );
                i += len;
                Kind::Punct
            }
        };
        tokens.push(Token {
            kind,
            range: start..i,
        });
    }
    tokens
}

fn line_end(bytes: &[u8], from: usize) -> usize {
    find(bytes, from, b"\n").unwrap_or(bytes.len())
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// The end of the quoted text opening at `open`, and whether it closed.
fn quoted_end(bytes: &[u8], open: usize, quote: u8) -> (usize, bool) {
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

#[derive(Debug)]
struct Lexeme<'a> {
    kind: Kind,
    text: &'a str,
    end: usize,
}

impl<'a> Lexeme<'a> {
    fn new(source: &'a str, token: &Token) -> Self {
        Self {
            kind: token.kind,
            text: &source[token.range.clone()],
            end: token.range.end,
        }
    }

    fn is_word(&self, word: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(word)
    }

    fn is_punct(&self, punct: &str) -> bool {
        self.kind == Kind::Punct && self.text == punct
    }

    fn is_keyword(&self) -> bool {
        self.kind == Kind::Word && RESERVED.iter().any(|k| self.text.eq_ignore_ascii_case(k))
    }

    /// The name a word or a quoted name spells; a reserved word is no name.
    fn name(&self) -> Option<String> {
        match self.kind {
            Kind::Word if !self.is_keyword() => Some(self.text.to_owned()),
            Kind::Quoted { closed: true } => {
                Some(self.text[1..self.text.len() - 1].replace("``", "`"))
            }
            _ => None,
        }
    }

    /// Whether it ends a value: a name, number, string, `)`, `*` or a literal word.
    fn ends_value(&self) -> bool {
        match self.kind {
            Kind::Number | Kind::String { .. } | Kind::Quoted { .. } => true,
            Kind::Punct => self.text == ")" || self.text == "*",
            Kind::Word => {
                !self.is_keyword()
                    || ["NULL", "TRUE", "FALSE", "ASC", "DESC", "END"]
                        .iter()
                        .any(|w| self.is_word(w))
            }
            _ => false,
        }
    }
}

/// Words that never name a table, column or alias here (MySQL reserved words that start or join
/// clauses, plus those that end a table reference).
const RESERVED: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "AS",
    "ASC",
    "BETWEEN",
    "BY",
    "CALL",
    "CASE",
    "CREATE",
    "CROSS",
    "DELETE",
    "DESC",
    "DESCRIBE",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "EXPLAIN",
    "FALSE",
    "FOR",
    "FORCE",
    "FROM",
    "FULL",
    "GROUP",
    "HAVING",
    "IGNORE",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "LOCK",
    "NATURAL",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "REGEXP",
    "REPLACE",
    "RIGHT",
    "RLIKE",
    "SELECT",
    "SET",
    "SHOW",
    "STRAIGHT_JOIN",
    "TABLE",
    "THEN",
    "TRUE",
    "TRUNCATE",
    "UNION",
    "UPDATE",
    "USE",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

/// Words after which a table is named.
const RELATION_WORDS: &[&str] = &["FROM", "JOIN", "INTO", "UPDATE", "TABLE"];
/// Words between UPDATE / DELETE / INSERT and the table.
const MODIFIERS: &[&str] = &[
    "LOW_PRIORITY",
    "IGNORE",
    "QUICK",
    "DELAYED",
    "HIGH_PRIORITY",
    "ONLY",
];

fn relation_word_at(lexemes: &[&Lexeme<'_>], index: usize) -> bool {
    let Some(lexeme) = lexemes.get(index) else {
        return false;
    };
    if !RELATION_WORDS.iter().any(|w| lexeme.is_word(w)) {
        return false;
    }
    !lexeme.is_word("FROM") || from_names_tables(lexemes, index)
}

/// Whether the FROM at `index` starts a table list, rather than ending `IS DISTINCT FROM` or
/// sitting in a call such as `EXTRACT(YEAR FROM created_at)`.
fn from_names_tables(lexemes: &[&Lexeme<'_>], index: usize) -> bool {
    if index > 0 && lexemes[index - 1].is_word("DISTINCT") {
        return false;
    }
    match enclosing_group(lexemes, index) {
        None => true,
        Some(open) => lexemes
            .get(open + 1)
            .is_some_and(|first| ["SELECT", "WITH"].iter().any(|w| first.is_word(w))),
    }
}

/// The index of the `(` that opens the innermost group around `index`.
fn enclosing_group(lexemes: &[&Lexeme<'_>], index: usize) -> Option<usize> {
    let mut depth = 0usize;
    for at in (0..index).rev() {
        if lexemes[at].is_punct(")") {
            depth += 1;
        } else if lexemes[at].is_punct("(") {
            if depth == 0 {
                return Some(at);
            }
            depth -= 1;
        }
    }
    None
}

/// The cursor is where a table goes: after a relation word, or after a comma in a FROM list.
fn is_table_slot(before: &[&Lexeme<'_>]) -> bool {
    let Some(last) = before.len().checked_sub(1) else {
        return false;
    };
    let mut at = last;
    while at > 0 && MODIFIERS.iter().any(|m| before[at].is_word(m)) {
        at -= 1;
    }
    if relation_word_at(before, at) {
        return true;
    }
    before[last].is_punct(",") && in_from_list(&before[..last])
}

/// Whether a comma continues the table list of a FROM (or a multi-table UPDATE).
fn in_from_list(rest: &[&Lexeme<'_>]) -> bool {
    for index in (0..rest.len()).rev() {
        let lexeme = rest[index];
        if lexeme.is_word("FROM") || lexeme.is_word("UPDATE") {
            return lexeme.is_word("UPDATE") || from_names_tables(rest, index);
        }
        let in_list = lexeme.is_punct(",")
            || lexeme.is_punct(".")
            || lexeme.is_word("AS")
            || lexeme.name().is_some();
        if !in_list {
            return false;
        }
    }
    false
}

/// The tables the statement names, after FROM / JOIN / INTO / UPDATE / TABLE, with their aliases.
fn collect_tables(code: &[Lexeme<'_>]) -> Vec<StatementTable> {
    let lexemes: Vec<&Lexeme<'_>> = code.iter().collect();
    let mut tables: Vec<StatementTable> = Vec::new();
    for index in 0..lexemes.len() {
        if !relation_word_at(&lexemes, index) {
            continue;
        }
        let is_list = lexemes[index].is_word("FROM") || lexemes[index].is_word("UPDATE");
        let mut at = index + 1;
        loop {
            while lexemes
                .get(at)
                .is_some_and(|l| MODIFIERS.iter().any(|m| l.is_word(m)))
            {
                at += 1;
            }
            if lexemes.get(at).is_some_and(|l| l.is_punct("(")) {
                // A derived table: its alias names no table of the schema.
                at = after_group(&lexemes, at);
                at = after_alias(&lexemes, at).1;
            } else {
                let Some((database, name, next)) = table_at(&lexemes, at) else {
                    break;
                };
                at = next;
                if lexemes.get(at).is_some_and(|l| l.is_punct("(")) {
                    // A call such as `JSON_TABLE(...)` or `t (a, b)` after INTO: no alias follows.
                    if !lexemes[index].is_word("INTO") {
                        break;
                    }
                }
                let (alias, next) = after_alias(&lexemes, at);
                at = next;
                let table = StatementTable {
                    database,
                    name,
                    alias,
                };
                if !tables.contains(&table) {
                    tables.push(table);
                }
            }
            if is_list && lexemes.get(at).is_some_and(|l| l.is_punct(",")) {
                at += 1;
                continue;
            }
            break;
        }
    }
    tables
}

/// `name` or `database.name` at `index`, and the index after it.
fn table_at(lexemes: &[&Lexeme<'_>], index: usize) -> Option<(Option<String>, String, usize)> {
    let first = lexemes.get(index)?.name()?;
    if lexemes.get(index + 1).is_some_and(|l| l.is_punct(".")) {
        // `db.` with nothing after it yet names no table.
        let second = lexemes.get(index + 2).and_then(|l| l.name())?;
        return Some((Some(first), second, index + 3));
    }
    Some((None, first, index + 1))
}

fn after_alias(lexemes: &[&Lexeme<'_>], index: usize) -> (Option<String>, usize) {
    match lexemes.get(index) {
        Some(l) if l.is_word("AS") => (lexemes.get(index + 1).and_then(|l| l.name()), index + 2),
        Some(l) => match l.name() {
            Some(alias) => (Some(alias), index + 1),
            None => (None, index),
        },
        None => (None, index),
    }
}

/// The index after the parenthesised group that opens at `index`.
fn after_group(lexemes: &[&Lexeme<'_>], index: usize) -> usize {
    let mut depth = 0usize;
    for (at, lexeme) in lexemes.iter().enumerate().skip(index) {
        if lexeme.is_punct("(") {
            depth += 1;
        } else if lexeme.is_punct(")") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return at + 1;
            }
        }
    }
    lexemes.len()
}

/// After `name.`: the columns of the table `name` stands for, or the tables of database `name`;
/// after `db.table.`: that table's columns. `None` after a dot that follows no name.
fn after_dot_names(before: &[&Lexeme<'_>], tables: &[StatementTable]) -> Option<CompletionNames> {
    let [rest @ .., name, dot] = before else {
        return None;
    };
    if !dot.is_punct(".") {
        return None;
    }
    let name = name.name()?;
    if let [.., database, dot] = rest
        && dot.is_punct(".")
        && let Some(database) = database.name()
    {
        return Some(CompletionNames::Columns {
            tables: vec![StatementTable {
                database: Some(database),
                name,
                alias: None,
            }],
        });
    }
    let found = lookup(tables, &name);
    Some(match found {
        Some(table) => CompletionNames::Columns {
            tables: vec![table.clone()],
        },
        None => CompletionNames::Objects {
            database: Some(name),
        },
    })
}

/// The table `name` stands for: an alias first, then a table name, as written and then without
/// case, the way MySQL reads an unquoted name.
fn lookup<'t>(tables: &'t [StatementTable], name: &str) -> Option<&'t StatementTable> {
    let by = |exact: bool| {
        let same = |candidate: &str| {
            if exact {
                candidate == name
            } else {
                candidate.eq_ignore_ascii_case(name)
            }
        };
        tables
            .iter()
            .find(|t| t.alias.as_deref().is_some_and(same))
            .or_else(|| tables.iter().find(|t| same(&t.name)))
    };
    by(true).or_else(|| by(false))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Clause {
    Select,
    From,
    Join,
    On,
    Where,
    GroupBy,
    OrderBy,
    Having,
    Set,
    Into,
    Values,
    Limit,
    Union,
    Case,
    Call,
}

/// The clause the cursor is in: the nearest clause keyword before it at the same depth,
/// skipping closed groups and balanced `CASE … END`; a call when the cursor is in a call.
fn clause_at(before: &[&Lexeme<'_>]) -> Option<Clause> {
    let mut depth = 0usize;
    let mut pending_end = 0usize;
    for index in (0..before.len()).rev() {
        let lexeme = before[index];
        if lexeme.is_punct(")") {
            depth += 1;
            continue;
        }
        if lexeme.is_punct("(") {
            if depth == 0 {
                // Inside a group: a call, or a subquery that starts with its own clause.
                let is_call = index > 0 && before[index - 1].kind == Kind::Word;
                let is_subquery = before
                    .get(index + 1)
                    .is_some_and(|l| l.is_word("SELECT") || l.is_word("WITH"));
                if is_call && !is_subquery && !before[index - 1].is_word("IN") {
                    return Some(Clause::Call);
                }
                continue;
            }
            depth -= 1;
            continue;
        }
        if depth > 0 || lexeme.kind != Kind::Word {
            continue;
        }
        if lexeme.is_word("END") {
            pending_end += 1;
            continue;
        }
        if lexeme.is_word("CASE") {
            if pending_end > 0 {
                pending_end -= 1;
                continue;
            }
            return Some(Clause::Case);
        }
        if pending_end > 0 {
            continue;
        }
        let previous = index.checked_sub(1).map(|i| before[i]);
        let clause = match lexeme.text.to_ascii_uppercase().as_str() {
            "WHEN" | "THEN" | "ELSE" => Clause::Case,
            "SELECT" => Clause::Select,
            "FROM" if from_names_tables(before, index) => Clause::From,
            "JOIN" => Clause::Join,
            "ON" if !previous.is_some_and(|p| p.is_word("DUPLICATE")) => Clause::On,
            "WHERE" => Clause::Where,
            "HAVING" => Clause::Having,
            "SET" => Clause::Set,
            "INTO" => Clause::Into,
            "VALUES" | "VALUE" => Clause::Values,
            "LIMIT" | "OFFSET" => Clause::Limit,
            "UNION" | "INTERSECT" | "EXCEPT" => Clause::Union,
            "UPDATE" if previous.is_some_and(|p| p.is_word("KEY")) => Clause::Set,
            "BY" => match previous.map(|p| p.text.to_ascii_uppercase()).as_deref() {
                Some("GROUP") => Clause::GroupBy,
                Some("ORDER" | "PARTITION") => Clause::OrderBy,
                _ => continue,
            },
            _ => continue,
        };
        return Some(clause);
    }
    None
}

/// A value or a name goes at the cursor (after a clause keyword, an operator, `(` or `,`), not
/// the next keyword (after a finished value).
fn is_operand_slot(before: &[&Lexeme<'_>]) -> bool {
    let Some(last) = before.last() else {
        return false;
    };
    if last.is_punct("*") {
        // `SELECT *` and `t.*` end a value; `a * |` does not.
        let previous = before.len().checked_sub(2).map(|i| before[i]);
        return !previous.is_none_or(|p| p.is_word("SELECT") || p.is_punct(",") || p.is_punct("."));
    }
    !last.ends_value()
}

const COMPARISONS: &[&str] = &["=", "<>", "!=", "<", ">", "<=", ">=", "<=>"];

/// Whether the value at the end of `before` completes a comparison (`a = 1`), so the next word
/// joins conditions, rather than being the left side still waiting for its operator (`a`).
fn ends_comparison(before: &[&Lexeme<'_>]) -> bool {
    let mut at = before.len();
    // Step over the last value: a name path (`a.b`) or a literal.
    while at > 0 {
        at -= 1;
        if at >= 2 && before[at - 1].is_punct(".") {
            at -= 1;
            continue;
        }
        break;
    }
    let Some(previous) = at.checked_sub(1).map(|i| before[i]) else {
        return false;
    };
    COMPARISONS.iter().any(|op| previous.is_punct(op))
        || ["LIKE", "REGEXP", "RLIKE", "IS", "NOT", "AND"]
            .iter()
            .any(|w| previous.is_word(w))
        || previous.is_punct(")")
}

const STATEMENT_START: &[&str] = &[
    "SELECT",
    "INSERT INTO",
    "UPDATE",
    "DELETE FROM",
    "WITH",
    "REPLACE INTO",
    "CREATE TABLE",
    "ALTER TABLE",
    "DROP TABLE",
    "TRUNCATE TABLE",
    "SHOW",
    "DESCRIBE",
    "EXPLAIN",
    "USE",
    "SET",
    "BEGIN",
    "START TRANSACTION",
    "COMMIT",
    "ROLLBACK",
    "CALL",
];

const FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "COALESCE",
    "IFNULL",
    "NULLIF",
    "IF",
    "CONCAT",
    "CONCAT_WS",
    "GROUP_CONCAT",
    "LOWER",
    "UPPER",
    "LENGTH",
    "CHAR_LENGTH",
    "SUBSTRING",
    "TRIM",
    "REPLACE",
    "ROUND",
    "FLOOR",
    "CEIL",
    "ABS",
    "NOW",
    "CURDATE",
    "CURRENT_TIMESTAMP",
    "DATE",
    "DATE_FORMAT",
    "DATE_ADD",
    "DATE_SUB",
    "DATEDIFF",
    "TIMESTAMPDIFF",
    "YEAR",
    "MONTH",
    "DAY",
    "CAST",
    "CONVERT",
    "JSON_EXTRACT",
    "JSON_UNQUOTE",
    "JSON_OBJECT",
    "JSON_ARRAY",
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
    "LAG",
    "LEAD",
    "UUID",
];

const JOINS: &[&str] = &[
    "JOIN",
    "INNER JOIN",
    "LEFT JOIN",
    "RIGHT JOIN",
    "CROSS JOIN",
];

/// The keywords (and functions) that fit at the cursor, likeliest first.
fn keywords_at(
    before: &[&Lexeme<'_>],
    clause: Option<Clause>,
    names: &CompletionNames,
    expects_name: bool,
) -> (Vec<&'static str>, Vec<&'static str>) {
    let last = before.last();
    let last_is = |word: &str| last.is_some_and(|l| l.is_word(word));
    let previous_is = |word: &str| {
        before
            .len()
            .checked_sub(2)
            .is_some_and(|i| before[i].is_word(word))
    };

    // The word that always follows another.
    let follow: Option<&[&str]> = match () {
        _ if last.is_none() => Some(STATEMENT_START),
        _ if last_is("IS") => Some(&["NULL", "NOT NULL", "TRUE", "FALSE"]),
        _ if last_is("NOT") && previous_is("IS") => Some(&["NULL", "TRUE", "FALSE"]),
        _ if last_is("ORDER") || last_is("GROUP") || last_is("PARTITION") => Some(&["BY"]),
        _ if last_is("INSERT") || last_is("REPLACE") => Some(&["INTO"]),
        _ if last_is("DELETE") => Some(&["FROM"]),
        _ if last_is("LEFT") || last_is("RIGHT") => Some(&["JOIN", "OUTER JOIN"]),
        _ if last_is("INNER") || last_is("CROSS") || last_is("NATURAL") || last_is("OUTER") => {
            Some(&["JOIN"])
        }
        _ if last_is("UNION") => Some(&["SELECT", "ALL"]),
        _ if last_is("DUPLICATE") => Some(&["KEY UPDATE"]),
        _ if last_is("CREATE") => Some(&[
            "TABLE",
            "VIEW",
            "INDEX",
            "UNIQUE INDEX",
            "DATABASE",
            "PROCEDURE",
            "FUNCTION",
            "TRIGGER",
            "EVENT",
        ]),
        _ if last_is("ALTER") => Some(&["TABLE", "VIEW", "DATABASE"]),
        _ if last_is("DROP") => Some(&[
            "TABLE",
            "VIEW",
            "INDEX",
            "DATABASE",
            "PROCEDURE",
            "FUNCTION",
            "TRIGGER",
        ]),
        _ if last_is("SHOW") => Some(&[
            "TABLES",
            "DATABASES",
            "COLUMNS FROM",
            "INDEX FROM",
            "CREATE TABLE",
            "PROCESSLIST",
            "VARIABLES",
            "STATUS",
            "GRANTS",
            "WARNINGS",
        ]),
        _ => None,
    };
    if let Some(words) = follow {
        return (words.to_vec(), Vec::new());
    }
    if matches!(names, CompletionNames::Objects { .. }) {
        return (Vec::new(), Vec::new());
    }

    let expression = |extra: &[&'static str]| -> (Vec<&'static str>, Vec<&'static str>) {
        let mut words = vec!["NOT", "EXISTS", "NULL", "TRUE", "FALSE", "CASE"];
        words.extend_from_slice(extra);
        (words, FUNCTIONS.to_vec())
    };
    let clause_ends: &[&str] = &["GROUP BY", "ORDER BY", "HAVING", "LIMIT", "UNION"];
    let joined = |head: &[&'static str], tail: &[&'static str]| {
        let mut words = head.to_vec();
        words.extend_from_slice(tail);
        words
    };

    match clause {
        Some(Clause::Select) if expects_name => expression(&["DISTINCT"]),
        Some(Clause::Select) => (vec!["FROM", "AS", "UNION"], Vec::new()),
        Some(Clause::From) => (
            joined(&["WHERE"], &joined(JOINS, &joined(clause_ends, &["AS"]))),
            Vec::new(),
        ),
        Some(Clause::Join) => (
            joined(&["ON", "USING", "AS"], &joined(JOINS, &["WHERE"])),
            Vec::new(),
        ),
        Some(Clause::On | Clause::Where | Clause::Having) if expects_name => expression(&[]),
        Some(clause @ (Clause::On | Clause::Where | Clause::Having)) => {
            let next: &[&str] = match clause {
                Clause::On => &[
                    "WHERE",
                    "JOIN",
                    "LEFT JOIN",
                    "INNER JOIN",
                    "GROUP BY",
                    "ORDER BY",
                ],
                Clause::Where => &["GROUP BY", "ORDER BY", "LIMIT", "UNION"],
                _ => &["ORDER BY", "LIMIT"],
            };
            let words = if ends_comparison(before) {
                joined(&["AND", "OR"], next)
            } else {
                joined(
                    &[
                        "=",
                        "<>",
                        "IS NULL",
                        "IS NOT NULL",
                        "IN",
                        "NOT IN",
                        "LIKE",
                        "NOT LIKE",
                        "BETWEEN",
                        "REGEXP",
                        "AND",
                        "OR",
                    ],
                    next,
                )
            };
            (words, Vec::new())
        }
        Some(Clause::GroupBy) if expects_name => (Vec::new(), FUNCTIONS.to_vec()),
        Some(Clause::GroupBy) => (
            vec!["HAVING", "ORDER BY", "LIMIT", "WITH ROLLUP"],
            Vec::new(),
        ),
        Some(Clause::OrderBy) if expects_name => (Vec::new(), FUNCTIONS.to_vec()),
        Some(Clause::OrderBy) if last_is("ASC") || last_is("DESC") => (vec!["LIMIT"], Vec::new()),
        Some(Clause::OrderBy) => (vec!["ASC", "DESC", "LIMIT"], Vec::new()),
        Some(Clause::Set) if expects_name => expression(&["DEFAULT"]),
        Some(Clause::Set) => (vec!["WHERE"], Vec::new()),
        Some(Clause::Into) => (vec!["VALUES", "SELECT", "SET"], Vec::new()),
        Some(Clause::Values) if expects_name => expression(&["DEFAULT"]),
        Some(Clause::Values) => (vec!["ON DUPLICATE KEY UPDATE"], Vec::new()),
        Some(Clause::Limit) => (vec!["OFFSET"], Vec::new()),
        Some(Clause::Union) => (vec!["SELECT", "ALL"], Vec::new()),
        Some(Clause::Case) if expects_name => expression(&[]),
        Some(Clause::Case) => (vec!["WHEN", "THEN", "ELSE", "END", "AND", "OR"], Vec::new()),
        Some(Clause::Call) => expression(&["DISTINCT"]),
        None => (Vec::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The context at the `|` in `marked`.
    fn at(marked: &str) -> Option<CompletionContext> {
        let cursor = marked.find('|').expect("a cursor mark");
        completion_context(&marked.replacen('|', "", 1), cursor)
    }

    fn table(database: Option<&str>, name: &str, alias: Option<&str>) -> StatementTable {
        StatementTable {
            database: database.map(str::to_owned),
            name: name.to_owned(),
            alias: alias.map(str::to_owned),
        }
    }

    fn names(marked: &str) -> Option<CompletionNames> {
        at(marked).map(|c| c.names)
    }

    fn keywords(marked: &str) -> Vec<String> {
        at(marked).map(|c| c.keywords).unwrap_or_default()
    }

    const OBJECTS: Option<CompletionNames> = Some(CompletionNames::Objects { database: None });

    #[test]
    fn a_table_goes_after_relation_words() {
        assert_eq!(names("SELECT * FROM |"), OBJECTS);
        assert_eq!(names("SELECT * FROM us|"), OBJECTS);
        assert_eq!(names("select * from users u join |"), OBJECTS);
        assert_eq!(names("SELECT * FROM users u, |"), OBJECTS);
        assert_eq!(names("INSERT INTO |"), OBJECTS);
        assert_eq!(names("UPDATE |"), OBJECTS);
        assert_eq!(names("UPDATE LOW_PRIORITY |"), OBJECTS);
        assert_eq!(names("TRUNCATE TABLE |"), OBJECTS);
    }

    #[test]
    fn a_dot_lists_that_tables_columns_or_that_databases_tables() {
        let users = || {
            Some(CompletionNames::Columns {
                tables: vec![table(None, "users", Some("u"))],
            })
        };
        assert_eq!(names("SELECT u.| FROM users u"), users());
        assert_eq!(names("SELECT u.na| FROM users AS u"), users());
        assert_eq!(names("SELECT * FROM users U WHERE u.|"), {
            Some(CompletionNames::Columns {
                tables: vec![table(None, "users", Some("U"))],
            })
        });
        assert_eq!(
            names("SELECT users.| FROM users"),
            Some(CompletionNames::Columns {
                tables: vec![table(None, "users", None)]
            })
        );
        assert_eq!(
            names("SELECT * FROM shop.|"),
            Some(CompletionNames::Objects {
                database: Some("shop".into())
            })
        );
        assert_eq!(
            names("SELECT shop.orders.| FROM x"),
            Some(CompletionNames::Columns {
                tables: vec![table(Some("shop"), "orders", None)]
            })
        );
        assert_eq!(
            names("SELECT o.| FROM `shop`.`order items` o"),
            Some(CompletionNames::Columns {
                tables: vec![table(Some("shop"), "order items", Some("o"))]
            })
        );
    }

    #[test]
    fn a_dot_after_no_name_completes_nothing() {
        assert_eq!(names("SELECT * FROM items WHERE price > 0.|"), None);
        assert_eq!(names("SELECT 'a'.|"), None);
        assert_eq!(names("SELECT (1).|"), None);
    }

    #[test]
    fn joins_and_lists_collect_their_tables() {
        let context = at(
            "SELECT | FROM users u JOIN shop.orders AS o ON o.user_id = u.id LEFT JOIN items ON 1",
        )
        .unwrap();
        assert_eq!(
            context.tables,
            vec![
                table(None, "users", Some("u")),
                table(Some("shop"), "orders", Some("o")),
                table(None, "items", None),
            ]
        );
        assert_eq!(context.names, CompletionNames::Expression);
        let listed = at("SELECT | FROM a x, b, c AS z WHERE 1").unwrap();
        assert_eq!(
            listed.tables,
            vec![
                table(None, "a", Some("x")),
                table(None, "b", None),
                table(None, "c", Some("z"))
            ]
        );
        let update = at("UPDATE orders o SET o.| = 1").unwrap();
        assert_eq!(update.tables, vec![table(None, "orders", Some("o"))]);
    }

    #[test]
    fn a_from_inside_a_call_or_after_distinct_names_no_table() {
        let orders = vec![table(None, "orders", None)];
        assert_eq!(
            at("SELECT EXTRACT(YEAR FROM |) FROM orders")
                .unwrap()
                .tables,
            orders
        );
        assert_eq!(
            names("SELECT EXTRACT(YEAR FROM |) FROM orders"),
            Some(CompletionNames::Expression)
        );
        assert_eq!(
            names("SELECT * FROM orders WHERE a IS DISTINCT FROM |"),
            Some(CompletionNames::Expression)
        );
        assert_eq!(
            names("SELECT * FROM orders WHERE id IN (SELECT id FROM |"),
            OBJECTS
        );
    }

    #[test]
    fn strings_and_comments_complete_nothing() {
        assert_eq!(at("SELECT 'FROM |'"), None);
        assert_eq!(at("SELECT 1 -- FROM |"), None);
        assert_eq!(at("SELECT 1 # FROM |"), None);
        assert_eq!(at("SELECT /* FROM | */ 1"), None);
        assert_eq!(at("SELECT 'open |"), None);
        assert_eq!(at("SELECT `us|"), None);
        assert!(at("SELECT 'a' |").is_some());
    }

    #[test]
    fn a_new_statement_starts_after_the_terminator() {
        assert_eq!(keywords("SELECT * FROM users;\n|")[0], "SELECT");
        assert_eq!(keywords("SELECT * FROM users;\nsel|")[0], "SELECT");
        assert_eq!(keywords("|")[0], "SELECT");
        let context = at("SELECT * FROM users u WHERE |;\nSELECT 2").unwrap();
        assert_eq!(context.tables, vec![table(None, "users", Some("u"))]);
        let delimited = at("DELIMITER //\nSELECT * FROM t x WHERE x.| //").unwrap();
        assert_eq!(
            delimited.names,
            CompletionNames::Columns {
                tables: vec![table(None, "t", Some("x"))]
            }
        );
        assert_eq!(at("DELIMITER |"), None);
    }

    #[test]
    fn the_keywords_that_come_next() {
        assert_eq!(
            keywords("SELECT * FROM orders ORDER BY total |")[..2],
            ["ASC", "DESC"]
        );
        assert_eq!(
            keywords("SELECT * FROM orders ORDER BY o.total |")[..2],
            ["ASC", "DESC"]
        );
        assert_eq!(
            keywords("SELECT * FROM orders ORDER BY total DESC |"),
            ["LIMIT"]
        );
        assert_eq!(keywords("SELECT * FROM o JOIN c x |")[..2], ["ON", "USING"]);
        assert_eq!(
            keywords("SELECT * FROM o WHERE a IS |")[..2],
            ["NULL", "NOT NULL"]
        );
        assert_eq!(keywords("SELECT * FROM o WHERE a IS NOT |")[0], "NULL");
        assert_eq!(
            keywords("SELECT * FROM o WHERE a = 1 |")[..2],
            ["AND", "OR"]
        );
        assert_eq!(keywords("SELECT * FROM o WHERE a |")[0], "=");
        assert_eq!(
            keywords("SELECT * FROM o x JOIN c y ON y.id = x.c_id |")[..2],
            ["AND", "OR"]
        );
        assert_eq!(
            keywords("SELECT a, COUNT(*) FROM o GROUP BY a |")[0],
            "HAVING"
        );
        assert_eq!(keywords("SELECT * FROM orders o |")[0], "WHERE");
        assert_eq!(keywords("SELECT a |")[0], "FROM");
        assert_eq!(keywords("SELECT * |")[0], "FROM");
        assert_eq!(keywords("INSERT |"), ["INTO"]);
        assert_eq!(keywords("SELECT * FROM o ORDER |"), ["BY"]);
        assert_eq!(
            keywords("INSERT INTO t (a) VALUES (1) ON DUPLICATE |"),
            ["KEY UPDATE"]
        );
        assert_eq!(
            keywords("SELECT CASE WHEN a > 1 THEN 'x' |")[..2],
            ["WHEN", "THEN"]
        );
    }

    #[test]
    fn names_lead_in_an_operand_slot_and_keywords_after_a_value() {
        assert!(at("SELECT * FROM o WHERE |").unwrap().expects_name);
        assert!(at("SELECT * FROM o ORDER BY |").unwrap().expects_name);
        assert!(at("SELECT |").unwrap().expects_name);
        assert!(at("SELECT a, |").unwrap().expects_name);
        assert!(at("SELECT * FROM o WHERE a = |").unwrap().expects_name);
        assert!(at("SELECT * FROM |").unwrap().expects_name);
        assert!(!at("SELECT * FROM o ORDER BY total |").unwrap().expects_name);
        assert!(!at("SELECT * FROM o WHERE a = 1 |").unwrap().expects_name);
        assert!(!at("SELECT * |").unwrap().expects_name);
        assert!(!at("|").unwrap().expects_name);
        assert!(!at("SELECT a, COUNT(*) |").unwrap().expects_name);
    }

    #[test]
    fn functions_go_where_an_expression_goes() {
        assert!(
            at("SELECT |")
                .unwrap()
                .functions
                .contains(&"COUNT".to_owned())
        );
        assert!(
            at("SELECT * FROM o WHERE |")
                .unwrap()
                .functions
                .contains(&"NOW".to_owned())
        );
        assert!(
            at("SELECT COUNT(|")
                .unwrap()
                .functions
                .contains(&"DISTINCT".to_owned())
                || at("SELECT COUNT(|")
                    .unwrap()
                    .keywords
                    .contains(&"DISTINCT".to_owned())
        );
        assert!(
            at("SELECT * FROM o ORDER BY total |")
                .unwrap()
                .functions
                .is_empty()
        );
    }

    #[test]
    fn the_word_start_is_where_the_typed_word_begins() {
        assert_eq!(at("SELECT na|").unwrap().word_start, 7);
        assert_eq!(at("SELECT u.na| FROM users u").unwrap().word_start, 9);
        assert_eq!(at("SELECT |").unwrap().word_start, 7);
        let text = "SELECT * FROM đơn";
        let ctx = completion_context(text, text.len() - 2).unwrap();
        assert_eq!(ctx.names, CompletionNames::Objects { database: None });
    }

    #[test]
    fn odd_input_never_panics() {
        for text in [
            "",
            "'",
            "`",
            "/*",
            "--",
            "#",
            ".",
            "..",
            "(",
            ")))",
            "SELECT ((",
            "a.b.c.d.",
            "SELECT * FROM `",
            "ĐƠN.",
            "SELECT 'x' FROM t WHERE \\",
            "DELIMITER",
            "DELIMITER //\n",
            "INSERT INTO t (",
            "CASE END END",
        ] {
            for cursor in 0..=text.len() {
                let _ = completion_context(text, cursor);
            }
        }
    }
}
