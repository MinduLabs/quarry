//! Does a statement lean on state that only its own session holds?

use super::{keywords, skip_literal};

/// What a statement relies on from the session it runs in. Run on another connection, such a
/// statement fails or answers differently. Only the text is read, so a statement that merely
/// reads a temporary table created earlier cannot be told apart from a normal table: this is a
/// warning for the user, never a decision about safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionState {
    /// `@name`, `@'name'`: user variables, which belong to one connection. System variables
    /// (`@@name`) are not counted: every connection has them.
    pub has_user_variables: bool,
    /// `CREATE TEMPORARY TABLE`, `DROP TEMPORARY TABLE`: a temporary table lives and dies with
    /// one connection.
    pub has_temporary_tables: bool,
}

impl SessionState {
    /// The statement needs nothing from its session, as far as its text shows.
    pub fn is_stateless(self) -> bool {
        self == Self::default()
    }
}

/// The session state `statement` (or a whole script) relies on. Never panics: malformed text,
/// unterminated strings and comments, and huge input give a plain answer.
pub fn session_state(statement: &str) -> SessionState {
    SessionState {
        has_user_variables: has_user_variable(statement),
        has_temporary_tables: has_temporary_table(statement),
    }
}

/// `@name` outside strings, quoted identifiers and comments.
fn has_user_variable(statement: &str) -> bool {
    let bytes = statement.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(next) = skip_literal(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'@' if bytes.get(i + 1) == Some(&b'@') => {
                // `@@version`, `@@session.sql_mode`: a system variable.
                i += 2;
                while bytes.get(i).is_some_and(|b| is_name_byte(*b)) {
                    i += 1;
                }
            }
            b'@' => match bytes.get(i + 1) {
                Some(b) if is_name_byte(*b) || matches!(b, b'\'' | b'"' | b'`') => return true,
                // `@>` and `a @ b` are operators in other databases.
                _ => i += 1,
            },
            _ => i += 1,
        }
    }
    false
}

/// A byte of a variable name. Bytes of a multi-byte character count: `@tên` is a valid name.
fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$') || byte >= 0x80
}

fn has_temporary_table(statement: &str) -> bool {
    let words = keywords(statement);
    matches!(words.first().map(String::as_str), Some("CREATE" | "DROP"))
        && words
            .windows(2)
            .any(|pair| matches!(pair[0].as_str(), "TEMPORARY" | "TEMP") && pair[1] == "TABLE")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(sql: &str) -> bool {
        session_state(sql).has_user_variables
    }

    fn temp(sql: &str) -> bool {
        session_state(sql).has_temporary_tables
    }

    #[test]
    fn user_variables_are_found() {
        assert!(vars("SELECT * FROM t WHERE id = @x"));
        assert!(vars("SET @a := 1, @b = 2"));
        assert!(vars("SELECT COUNT(*) INTO @n FROM t"));
        assert!(vars("SELECT @row := @row + 1 FROM t"));
        assert!(vars("SELECT @tên"), "non-ASCII names are valid");
        assert!(vars("SELECT @a.b$c_1"));
    }

    #[test]
    fn quoted_user_variable_names_are_found() {
        assert!(vars("SELECT @'my var'"));
        assert!(vars("SELECT @\"my var\""));
        assert!(vars("SELECT @`my var`"));
    }

    #[test]
    fn system_variables_are_not_user_variables() {
        assert!(!vars("SELECT @@version"));
        assert!(!vars("SELECT @@session.sql_mode, @@global.max_connections"));
        assert!(!vars("SET @@autocommit = 0"));
        assert!(
            vars("SELECT @@version, @x"),
            "a user variable after a system one still counts"
        );
    }

    #[test]
    fn lookalikes_in_strings_identifiers_and_comments_are_ignored() {
        for sql in [
            "SELECT '@x'",
            "SELECT \"@x\"",
            "SELECT `@x`",
            "SELECT 1 -- @x",
            "SELECT 1 # @x",
            "SELECT 1 /* @x */",
            "SELECT 'it''s @x'",
            "SELECT 'a\\'@x'",
            "SELECT 'Nẵng @x'",
        ] {
            assert!(!vars(sql), "{sql}");
        }
        assert!(vars("SELECT 'a' = @x"));
        assert!(vars("SELECT '@a', @b"));
        assert!(vars("SELECT 1 /* c */ , @x"));
    }

    #[test]
    fn operators_of_other_databases_are_not_variables() {
        assert!(!vars("SELECT a @> b FROM t"));
        assert!(!vars("SELECT @ -5"));
        assert!(!vars("SELECT x @ y"));
    }

    #[test]
    fn temporary_tables_are_found() {
        assert!(temp("CREATE TEMPORARY TABLE t (id INT)"));
        assert!(temp("create temporary table t as select 1"));
        assert!(temp("DROP TEMPORARY TABLE IF EXISTS t"));
        assert!(temp("CREATE GLOBAL TEMPORARY TABLE t (id INT)"));
        assert!(temp("CREATE TEMP TABLE t (id INT)"));
        assert!(temp("-- note\nCREATE /* c */ TEMPORARY TABLE t (id INT)"));
    }

    #[test]
    fn temporary_lookalikes_are_ignored() {
        for sql in [
            "SELECT 'CREATE TEMPORARY TABLE t'",
            "-- CREATE TEMPORARY TABLE t\nSELECT 1",
            "/* DROP TEMPORARY TABLE t */ SELECT 1",
            "CREATE TABLE temporary_x (id INT)",
            "CREATE TABLE t (temporary INT, other INT)",
            "SELECT temporary FROM table_a",
            "DROP TABLE t",
        ] {
            assert!(!temp(sql), "{sql}");
        }
    }

    #[test]
    fn both_kinds_are_reported_separately() {
        let both = session_state("CREATE TEMPORARY TABLE t AS SELECT @x");
        assert!(both.has_user_variables && both.has_temporary_tables);
        assert!(!both.is_stateless());
        let only_vars = session_state("SELECT @x");
        assert!(only_vars.has_user_variables && !only_vars.has_temporary_tables);
        assert!(session_state("SELECT * FROM orders WHERE id = 1").is_stateless());
    }

    #[test]
    fn empty_and_malformed_text_gives_a_plain_answer() {
        for sql in [
            "",
            "   \n\t",
            "@",
            "@@",
            "SELECT @",
            "SELECT @@",
            "SELECT '@x",
            "SELECT \"@x",
            "SELECT `@x",
            "SELECT /* @x",
            "SELECT 'a\\",
            "SELECT -- @x",
        ] {
            assert!(!vars(sql), "{sql:?}");
        }
        assert!(session_state("").is_stateless());
    }

    #[test]
    fn huge_input_is_read_in_one_pass() {
        let long = "@".repeat(1_000_000);
        assert!(!vars(&long), "a run of `@@` pairs is system variables");
        assert!(!vars(&"'".repeat(1_000_001)));
        assert!(!vars(&format!("/*{}", "@x ".repeat(300_000))));
        assert!(vars(&format!("{} @x", "SELECT 1; ".repeat(100_000))));
    }
}
