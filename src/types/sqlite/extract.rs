//! Extract handler for SQLite schema-row drilldowns.
//!
//! Inner-path scheme matches what [`super::compose`] wrote into the
//! listing: `<kind>/<name>.sql` where `<kind>` is one of
//! `tables` / `views` / `indexes` / `triggers`. The handler queries
//! `sqlite_master.sql` for the entity's `CREATE …` statement, prepends
//! a `-- <name> from <db>` header comment, and hands the result back
//! as an in-memory [`InputSource`]. peek's outer pipeline then
//! re-detects (`.sql` → `FileType::SourceCode`) and opens it through
//! the existing SQL syntax view — no new mode needed.
//!
//! Contents rows (`<kind>/<name>.csv`) land in the next patch with a
//! dedicated streaming table mode; they don't go through this
//! extractor.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};

use crate::extract::{ExtractError, Extracted};
use crate::input::InputSource;
use crate::types::sqlite::compose::{
    KIND_INDEXES, KIND_TABLES, KIND_TRIGGERS, KIND_VIEWS, SCHEMA_SUFFIX,
};
use crate::types::sqlite::reader::SqliteReader;

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let parsed = parse_key(key).ok_or_else(|| ExtractError::InvalidKey(key.to_string()))?;

    let reader = SqliteReader::open(source).map_err(ExtractError::Other)?;
    let sql = lookup_sql(&reader.conn, parsed.master_kind, parsed.name)
        .map_err(ExtractError::Other)?
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;

    let body = format_dump(parsed.name, source.name(), &sql);
    let suggested_name = format!("{}{}", parsed.name, SCHEMA_SUFFIX);
    Ok(Extracted {
        source: InputSource::memory(body.into_bytes(), suggested_name.clone()),
        suggested_name,
    })
}

struct ParsedKey<'a> {
    /// `sqlite_master.type` literal: `table` / `view` / `index` / `trigger`.
    master_kind: &'a str,
    /// Bare entity name (no kind prefix, no `.sql` suffix).
    name: &'a str,
}

fn parse_key(key: &str) -> Option<ParsedKey<'_>> {
    let (kind_dir, rest) = key.split_once('/')?;
    let name = rest.strip_suffix(SCHEMA_SUFFIX)?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let master_kind = match kind_dir {
        KIND_TABLES => "table",
        KIND_VIEWS => "view",
        KIND_INDEXES => "index",
        KIND_TRIGGERS => "trigger",
        _ => return None,
    };
    Some(ParsedKey { master_kind, name })
}

fn lookup_sql(conn: &Connection, kind: &str, name: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
        rusqlite::params![kind, name],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .context("looking up sqlite_master.sql")
    .map(|opt| opt.flatten())
}

/// Build the dumped SQL text. SQLite stores the original `CREATE …`
/// without a trailing semicolon; add one so the file is a valid
/// standalone SQL script that a downstream `psql` / `sqlite3` can
/// execute. The leading comment is a hint, not metadata — peek's
/// SQL syntax highlighter renders it as a comment line.
fn format_dump(name: &str, db_name: &str, sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 64);
    out.push_str("-- ");
    out.push_str(name);
    out.push_str(" from ");
    out.push_str(db_name);
    out.push('\n');
    out.push_str(sql.trim_end());
    if !out.ends_with(';') {
        out.push(';');
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key_accepts_each_kind() {
        let cases = [
            ("tables/books.sql", "table", "books"),
            ("views/popular_authors.sql", "view", "popular_authors"),
            (
                "indexes/idx_books_language.sql",
                "index",
                "idx_books_language",
            ),
            ("triggers/audit_books.sql", "trigger", "audit_books"),
        ];
        for (key, kind, name) in cases {
            let p = parse_key(key).unwrap_or_else(|| panic!("parse {key}"));
            assert_eq!(p.master_kind, kind, "kind for {key}");
            assert_eq!(p.name, name, "name for {key}");
        }
    }

    #[test]
    fn parse_key_rejects_bad_shapes() {
        assert!(parse_key("books.sql").is_none(), "missing kind dir");
        assert!(parse_key("tables/books").is_none(), "missing suffix");
        assert!(parse_key("tables/.sql").is_none(), "empty name");
        assert!(parse_key("other/x.sql").is_none(), "unknown kind");
        assert!(parse_key("tables/a/b.sql").is_none(), "nested path");
    }

    #[test]
    fn format_dump_appends_semicolon_and_header() {
        let out = format_dump("books", "library.sqlite", "CREATE TABLE books (id INT)");
        assert!(out.starts_with("-- books from library.sqlite\n"));
        assert!(out.contains("CREATE TABLE books (id INT);"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn format_dump_does_not_double_semicolon() {
        let out = format_dump("v", "db", "CREATE VIEW v AS SELECT 1;");
        let semis = out.matches(';').count();
        assert_eq!(semis, 1, "one ';' kept, no duplicate appended");
    }
}
