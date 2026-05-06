//! Build [`SqlStats`] by single-pass scan.
//!
//! The scanner tracks string/comment/dollar-quoted state to find real
//! statement boundaries (`;` outside any quoted/comment region). Each
//! statement's first keyword classifies it (DDL / DML / DQL / TCL); for
//! `CREATE` statements the object kind + name are recorded. Dialect is
//! a heuristic: weighted feature votes pick a winner over Generic.

use crate::info::{SqlDialect, SqlStats};

pub fn gather(text: &str) -> SqlStats {
    let mut stats = SqlStats {
        statement_count: 0,
        ddl_count: 0,
        dml_count: 0,
        dql_count: 0,
        tcl_count: 0,
        other_count: 0,
        created_tables: Vec::new(),
        created_views: Vec::new(),
        created_indexes: Vec::new(),
        created_functions: Vec::new(),
        created_triggers: Vec::new(),
        comment_lines: 0,
        dialect: SqlDialect::Generic,
        has_dollar_quoted: false,
    };

    stats.comment_lines = count_comment_lines(text);
    stats.dialect = guess_dialect(text);
    stats.has_dollar_quoted = text.contains("$$") || has_tagged_dollar_quote(text);

    for stmt in split_statements(text) {
        let cleaned = strip_comments(stmt);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }
        stats.statement_count += 1;
        classify(trimmed, &mut stats);
    }

    stats
}

fn classify(stmt: &str, stats: &mut SqlStats) {
    let upper = upper_prefix(stmt, 32);
    let kind = first_keyword(&upper);
    match kind.as_deref() {
        Some("SELECT") | Some("WITH") | Some("VALUES") | Some("TABLE") | Some("SHOW")
        | Some("EXPLAIN") => {
            stats.dql_count += 1;
        }
        Some("INSERT") | Some("UPDATE") | Some("DELETE") | Some("MERGE") | Some("UPSERT")
        | Some("REPLACE") | Some("TRUNCATE") | Some("CALL") | Some("COPY") => {
            stats.dml_count += 1;
        }
        Some("CREATE") | Some("ALTER") | Some("DROP") | Some("RENAME") | Some("COMMENT")
        | Some("GRANT") | Some("REVOKE") => {
            stats.ddl_count += 1;
            if matches!(kind.as_deref(), Some("CREATE")) {
                record_created(stmt, stats);
            }
        }
        Some("BEGIN") | Some("START") | Some("COMMIT") | Some("ROLLBACK") | Some("SAVEPOINT")
        | Some("END") => {
            stats.tcl_count += 1;
        }
        _ => {
            stats.other_count += 1;
        }
    }
}

/// `CREATE [OR REPLACE] [TEMP|TEMPORARY] [UNIQUE] {TABLE|VIEW|INDEX|...} [IF NOT EXISTS] name`
fn record_created(stmt: &str, stats: &mut SqlStats) {
    let mut tokens = tokenize(stmt);
    // Drop `CREATE`
    if tokens.first().map(|t| t.eq_ignore_ascii_case("CREATE")) != Some(true) {
        return;
    }
    tokens.remove(0);

    // Skip optional modifiers
    let modifiers = [
        "OR",
        "REPLACE",
        "TEMP",
        "TEMPORARY",
        "GLOBAL",
        "LOCAL",
        "UNIQUE",
        "MATERIALIZED",
    ];
    while let Some(first) = tokens.first()
        && modifiers.iter().any(|m| first.eq_ignore_ascii_case(m))
    {
        tokens.remove(0);
    }

    let kind_token = tokens.first().cloned();
    let kind = match kind_token.as_deref() {
        Some(k) if k.eq_ignore_ascii_case("TABLE") => "table",
        Some(k) if k.eq_ignore_ascii_case("VIEW") => "view",
        Some(k) if k.eq_ignore_ascii_case("INDEX") => "index",
        Some(k) if k.eq_ignore_ascii_case("FUNCTION") || k.eq_ignore_ascii_case("PROCEDURE") => {
            "function"
        }
        Some(k) if k.eq_ignore_ascii_case("TRIGGER") => "trigger",
        _ => return,
    };
    tokens.remove(0);

    // `IF NOT EXISTS`
    if tokens
        .first()
        .map(|t| t.eq_ignore_ascii_case("IF"))
        .unwrap_or(false)
    {
        tokens.remove(0);
        if tokens
            .first()
            .map(|t| t.eq_ignore_ascii_case("NOT"))
            .unwrap_or(false)
        {
            tokens.remove(0);
        }
        if tokens
            .first()
            .map(|t| t.eq_ignore_ascii_case("EXISTS"))
            .unwrap_or(false)
        {
            tokens.remove(0);
        }
    }

    let Some(name_token) = tokens.first() else {
        return;
    };
    let name = unquote_ident(name_token);
    if name.is_empty() {
        return;
    }

    let bucket = match kind {
        "table" => &mut stats.created_tables,
        "view" => &mut stats.created_views,
        "index" => &mut stats.created_indexes,
        "function" => &mut stats.created_functions,
        "trigger" => &mut stats.created_triggers,
        _ => return,
    };
    if !bucket.contains(&name) {
        bucket.push(name);
    }
}

fn unquote_ident(s: &str) -> String {
    let trimmed = s.trim_matches(|c: char| c == ';' || c == ',' || c == '(');
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'"' && last == b'"')
            || (first == b'`' && last == b'`')
            || (first == b'[' && last == b']')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

/// Split text on `;` that lies outside any string/comment/dollar-quoted
/// region. Returned slices borrow from the input.
fn split_statements(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut state = ScanState::Code;

    while i < bytes.len() {
        match state {
            ScanState::Code => match bytes[i] {
                b'\'' => {
                    state = ScanState::SingleQuote;
                    i += 1;
                }
                b'"' => {
                    state = ScanState::DoubleQuote;
                    i += 1;
                }
                b'`' => {
                    state = ScanState::Backtick;
                    i += 1;
                }
                b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                    state = ScanState::LineComment;
                    i += 2;
                }
                b'#' => {
                    state = ScanState::LineComment;
                    i += 1;
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    state = ScanState::BlockComment;
                    i += 2;
                }
                b'$' => {
                    if let Some((tag_end, tag)) = parse_dollar_tag(bytes, i) {
                        state = ScanState::DollarQuote(tag.to_string());
                        i = tag_end;
                    } else {
                        i += 1;
                    }
                }
                b';' => {
                    statements.push(&text[start..i]);
                    start = i + 1;
                    i += 1;
                }
                _ => i += 1,
            },
            ScanState::SingleQuote => match bytes[i] {
                b'\\' if i + 1 < bytes.len() => i += 2,
                b'\'' if i + 1 < bytes.len() && bytes[i + 1] == b'\'' => i += 2, // SQL '' escape
                b'\'' => {
                    state = ScanState::Code;
                    i += 1;
                }
                _ => i += 1,
            },
            ScanState::DoubleQuote => match bytes[i] {
                b'\\' if i + 1 < bytes.len() => i += 2,
                b'"' => {
                    state = ScanState::Code;
                    i += 1;
                }
                _ => i += 1,
            },
            ScanState::Backtick => match bytes[i] {
                b'`' => {
                    state = ScanState::Code;
                    i += 1;
                }
                _ => i += 1,
            },
            ScanState::LineComment => match bytes[i] {
                b'\n' => {
                    state = ScanState::Code;
                    i += 1;
                }
                _ => i += 1,
            },
            ScanState::BlockComment => {
                if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    state = ScanState::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            ScanState::DollarQuote(ref tag) => {
                let needle = format!("${tag}$");
                let nb = needle.as_bytes();
                if i + nb.len() <= bytes.len() && &bytes[i..i + nb.len()] == nb {
                    i += nb.len();
                    state = ScanState::Code;
                } else {
                    i += 1;
                }
            }
        }
    }
    if start < bytes.len() {
        statements.push(&text[start..]);
    }
    statements
}

enum ScanState {
    Code,
    SingleQuote,
    DoubleQuote,
    Backtick,
    LineComment,
    BlockComment,
    DollarQuote(String),
}

/// Return `(byte index just past the closing `$`, tag string)` for a
/// dollar-quote opener at `start`. `start` must point at the first `$`.
fn parse_dollar_tag(bytes: &[u8], start: usize) -> Option<(usize, &str)> {
    debug_assert_eq!(bytes[start], b'$');
    let mut j = start + 1;
    while j < bytes.len() {
        let b = bytes[j];
        if b == b'$' {
            // Tag is everything between the two `$`.
            let tag = std::str::from_utf8(&bytes[start + 1..j]).ok()?;
            // Tag must be empty or a valid identifier (letters/digits/_).
            if !tag.is_empty() && !tag.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                return None;
            }
            return Some((j + 1, tag));
        }
        if !(b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
        j += 1;
    }
    None
}

fn has_tagged_dollar_quote(text: &str) -> bool {
    // Find `$tag$` for non-empty tag.
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$'
            && let Some((_, tag)) = parse_dollar_tag(bytes, i)
            && !tag.is_empty()
        {
            return true;
        }
        i += 1;
    }
    false
}

fn strip_comments(stmt: &str) -> String {
    let bytes = stmt.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    let mut state = ScanState::Code;
    while i < bytes.len() {
        match state {
            ScanState::Code => match bytes[i] {
                b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                    state = ScanState::LineComment;
                    i += 2;
                }
                b'#' => {
                    state = ScanState::LineComment;
                    i += 1;
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    state = ScanState::BlockComment;
                    i += 2;
                }
                b'\'' => {
                    out.push('\'');
                    state = ScanState::SingleQuote;
                    i += 1;
                }
                b'"' => {
                    out.push('"');
                    state = ScanState::DoubleQuote;
                    i += 1;
                }
                _ => {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            },
            ScanState::SingleQuote => {
                out.push(bytes[i] as char);
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(bytes[i + 1] as char);
                    i += 2;
                } else if bytes[i] == b'\'' {
                    state = ScanState::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            ScanState::DoubleQuote => {
                out.push(bytes[i] as char);
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(bytes[i + 1] as char);
                    i += 2;
                } else if bytes[i] == b'"' {
                    state = ScanState::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            ScanState::LineComment => {
                if bytes[i] == b'\n' {
                    out.push('\n');
                    state = ScanState::Code;
                }
                i += 1;
            }
            ScanState::BlockComment => {
                if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    state = ScanState::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

fn count_comment_lines(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0usize;
    let mut i = 0;
    let mut state = ScanState::Code;
    let mut line_has_comment = false;

    while i < bytes.len() {
        let b = bytes[i];
        match state {
            ScanState::Code => match b {
                b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                    line_has_comment = true;
                    state = ScanState::LineComment;
                    i += 2;
                }
                b'#' => {
                    line_has_comment = true;
                    state = ScanState::LineComment;
                    i += 1;
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    line_has_comment = true;
                    state = ScanState::BlockComment;
                    i += 2;
                }
                b'\'' => {
                    state = ScanState::SingleQuote;
                    i += 1;
                }
                b'"' => {
                    state = ScanState::DoubleQuote;
                    i += 1;
                }
                b'\n' => {
                    if line_has_comment {
                        count += 1;
                    }
                    line_has_comment = false;
                    i += 1;
                }
                _ => i += 1,
            },
            ScanState::LineComment => {
                if b == b'\n' {
                    count += 1;
                    line_has_comment = false;
                    state = ScanState::Code;
                }
                i += 1;
            }
            ScanState::BlockComment => {
                if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    state = ScanState::Code;
                    i += 2;
                } else {
                    if b == b'\n' {
                        count += 1;
                        line_has_comment = false;
                    } else {
                        line_has_comment = true;
                    }
                    i += 1;
                }
            }
            ScanState::SingleQuote => {
                if b == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else if b == b'\'' {
                    state = ScanState::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            ScanState::DoubleQuote => {
                if b == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else if b == b'"' {
                    state = ScanState::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    if line_has_comment {
        count += 1;
    }
    count
}

fn upper_prefix(s: &str, max: usize) -> String {
    s.chars().take(max).flat_map(|c| c.to_uppercase()).collect()
}

fn first_keyword(upper: &str) -> Option<String> {
    upper
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|w| !w.is_empty())
        .map(|w| w.to_string())
}

fn tokenize(stmt: &str) -> Vec<String> {
    // Tokens: contiguous non-whitespace runs. Quoted/bracketed identifiers
    // are kept together so we can unquote them later.
    let mut tokens = Vec::new();
    let bytes = stmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            c if (c as char).is_whitespace() => {
                i += 1;
            }
            b'"' | b'`' | b'[' => {
                let close = match bytes[i] {
                    b'[' => b']',
                    other => other,
                };
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != close {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                tokens.push(String::from_utf8_lossy(&bytes[start..i]).into_owned());
            }
            _ => {
                let start = i;
                while i < bytes.len() && !(bytes[i] as char).is_whitespace() {
                    let b = bytes[i];
                    if b == b'(' || b == b';' || b == b',' {
                        if start == i {
                            i += 1;
                        }
                        break;
                    }
                    i += 1;
                }
                if i > start {
                    tokens.push(String::from_utf8_lossy(&bytes[start..i]).into_owned());
                }
            }
        }
    }
    tokens
}

fn guess_dialect(text: &str) -> SqlDialect {
    let upper = text.to_ascii_uppercase();
    let mut pg = 0i32;
    let mut my = 0i32;
    let mut sq = 0i32;
    let mut ts = 0i32;

    if text.contains("$$") {
        pg += 3;
    }
    if upper.contains("RETURNING ") {
        pg += 2;
    }
    if upper.contains("SERIAL") || upper.contains("BIGSERIAL") || upper.contains("LANGUAGE PLPGSQL")
    {
        pg += 2;
    }
    if upper.contains("::TEXT")
        || upper.contains("::INT")
        || upper.contains("::BIGINT")
        || upper.contains("::JSONB")
    {
        pg += 1;
    }
    if upper.contains("ON CONFLICT") {
        pg += 1;
    }
    if upper.contains("TIMESTAMPTZ") {
        pg += 1;
    }

    if upper.contains("AUTO_INCREMENT") {
        my += 3;
    }
    if upper.contains("ENGINE=") {
        my += 2;
    }
    if text.contains('`') {
        my += 1;
    }
    if upper.contains("UNSIGNED") {
        my += 1;
    }

    if upper.contains("AUTOINCREMENT") {
        sq += 3;
    }
    if upper.contains("PRAGMA ") {
        sq += 2;
    }

    if upper.contains("IDENTITY(") {
        ts += 2;
    }
    if upper.contains("\nGO\n") || upper.starts_with("GO\n") || upper.ends_with("\nGO") {
        ts += 2;
    }
    if upper.contains("NVARCHAR(") {
        ts += 1;
    }

    let scores = [
        (SqlDialect::PostgreSql, pg),
        (SqlDialect::MySql, my),
        (SqlDialect::Sqlite, sq),
        (SqlDialect::TSql, ts),
    ];
    let (best, score) = scores.iter().max_by_key(|(_, s)| *s).copied().unwrap();
    if score >= 2 {
        best
    } else {
        SqlDialect::Generic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_basic_statements() {
        let s = "
            CREATE TABLE t (id INT);
            INSERT INTO t VALUES (1);
            SELECT * FROM t;
            BEGIN;
            COMMIT;
        ";
        let stats = gather(s);
        assert_eq!(stats.statement_count, 5);
        assert_eq!(stats.ddl_count, 1);
        assert_eq!(stats.dml_count, 1);
        assert_eq!(stats.dql_count, 1);
        assert_eq!(stats.tcl_count, 2);
    }

    #[test]
    fn ignores_semicolons_inside_strings_and_dollar_quotes() {
        let s = r#"
            SELECT 'a;b;c';
            DO $$ BEGIN RAISE NOTICE 'x;y'; END $$;
            SELECT 1;
        "#;
        let stats = gather(s);
        assert_eq!(stats.statement_count, 3);
        assert!(stats.has_dollar_quoted);
    }

    #[test]
    fn records_created_objects() {
        let s = "
            CREATE TABLE customers (id INT);
            CREATE OR REPLACE VIEW v AS SELECT 1;
            CREATE INDEX idx_a ON customers(id);
            CREATE FUNCTION f() RETURNS INT AS $$ SELECT 1 $$ LANGUAGE sql;
            CREATE TRIGGER trg BEFORE UPDATE ON customers EXECUTE FUNCTION f();
        ";
        let stats = gather(s);
        assert_eq!(stats.created_tables, vec!["customers"]);
        assert_eq!(stats.created_views, vec!["v"]);
        assert_eq!(stats.created_indexes, vec!["idx_a"]);
        assert_eq!(stats.created_functions, vec!["f"]);
        assert_eq!(stats.created_triggers, vec!["trg"]);
    }

    #[test]
    fn dialect_postgres() {
        let s = "CREATE TABLE t (id BIGSERIAL); INSERT INTO t DEFAULT VALUES RETURNING id;";
        assert_eq!(gather(s).dialect, SqlDialect::PostgreSql);
    }

    #[test]
    fn dialect_mysql() {
        let s = "CREATE TABLE `t` (id INT AUTO_INCREMENT) ENGINE=InnoDB;";
        assert_eq!(gather(s).dialect, SqlDialect::MySql);
    }

    #[test]
    fn comment_lines_counted() {
        let s = "-- one\n-- two\nSELECT 1;\n/* three\n   four */\n";
        let stats = gather(s);
        assert_eq!(stats.comment_lines, 4);
    }

    #[test]
    fn if_not_exists_skipped() {
        let s = "CREATE TABLE IF NOT EXISTS users (id INT);";
        let stats = gather(s);
        assert_eq!(stats.created_tables, vec!["users"]);
    }
}
