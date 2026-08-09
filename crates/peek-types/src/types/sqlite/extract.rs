//! Extract handler for SQLite listing rows.
//!
//! Inner-path scheme matches what [`super::compose`] wrote into the
//! listing: `<kind>/<name>.sql` for the entity's `CREATE …` DDL, and
//! `<kind>/<name>.csv` for the contents of a table or view.
//!
//! * `.sql` rows return an in-memory source with a `-- <name> from
//!   <db>` header comment + the `CREATE …` statement pulled from
//!   `sqlite_master.sql`. peek's outer re-detect picks it up as
//!   `FileType::SourceCode { syntax: "sql" }` and opens the SQL syntax
//!   view — no new mode needed.
//! * `.csv` rows stream `SELECT * FROM "<entity>"` through `csv::Writer`
//!   into a `NamedTempFile`. Tempfile-spooled so the rows never have to
//!   fit in memory all at once — peek's existing `TempFile`
//!   `InputSource` arm holds the file alive until every clone drops.
//!   Cell stringification: NULL → empty (CSV convention), INTEGER /
//!   REAL / TEXT → display form, BLOB → SQL hex literal `X'…'`
//!   (round-trippable into an `INSERT` and lossless on the wire).

use std::fmt::Write as _;

use anyhow::{Context, Result, anyhow};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension};
use tempfile::NamedTempFile;

use crate::extract::{ExtractError, Extracted};
use crate::types::sqlite::compose::{
    CONTENTS_SUFFIX, KIND_INDEXES, KIND_TABLES, KIND_TRIGGERS, KIND_VIEWS, SCHEMA_SUFFIX,
};
use crate::types::sqlite::reader::SqliteReader;
use crate::types::sqlite::sql::quote_ident;
use peek_io::InputSource;

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let parsed = parse_key(key).ok_or_else(|| ExtractError::InvalidKey(key.to_string()))?;
    match parsed.leaf {
        LeafKind::Schema => extract_schema(source, key, &parsed),
        LeafKind::Contents => extract_contents(source, &parsed),
    }
}

fn extract_schema(
    source: &InputSource,
    raw_key: &str,
    parsed: &ParsedKey<'_>,
) -> Result<Extracted, ExtractError> {
    let reader = SqliteReader::open(source).map_err(ExtractError::Other)?;
    let sql = lookup_sql(&reader.conn, parsed.master_kind, parsed.name)
        .map_err(ExtractError::Other)?
        .ok_or_else(|| ExtractError::NotFound(raw_key.to_string()))?;

    let body = format_ddl_dump(parsed.name, source.name(), &sql);
    let suggested_name = format!("{}{}", parsed.name, SCHEMA_SUFFIX);
    Ok(Extracted {
        source: InputSource::memory(body.into_bytes(), suggested_name.clone()),
        suggested_name,
    })
}

fn extract_contents(
    source: &InputSource,
    parsed: &ParsedKey<'_>,
) -> Result<Extracted, ExtractError> {
    let reader = SqliteReader::open(source).map_err(ExtractError::Other)?;
    let entity_sql = quote_ident(parsed.name);

    let mut tmp = NamedTempFile::new()
        .map_err(|e| ExtractError::Other(anyhow!("creating CSV temp spool: {e}")))?;
    write_csv_dump(&reader.conn, parsed.name, &entity_sql, tmp.as_file_mut())
        .map_err(ExtractError::Other)?;

    let suggested_name = format!("{}{}", parsed.name, CONTENTS_SUFFIX);
    Ok(Extracted {
        source: InputSource::temp_file(tmp, suggested_name.clone()),
        suggested_name,
    })
}

/// Stream every row of `<entity>` through `csv::Writer` into `out`.
/// Header row carries the column names from `PRAGMA table_info`; if
/// the PRAGMA reports nothing (rare, e.g. a malformed view), fall back
/// to whatever column count the first `SELECT *` row exposes and emit
/// `col0` / `col1` / … placeholders.
fn write_csv_dump<W: std::io::Write>(
    conn: &Connection,
    entity: &str,
    entity_sql: &str,
    out: W,
) -> Result<()> {
    let mut wtr = csv::Writer::from_writer(out);

    let columns = list_columns(conn, entity).context("listing columns for CSV extract")?;
    if !columns.is_empty() {
        wtr.write_record(&columns)
            .context("writing CSV header row")?;
    }

    let sql = format!("SELECT * FROM {entity_sql}");
    let mut stmt = conn
        .prepare(&sql)
        .with_context(|| format!("preparing {sql}"))?;
    let col_count = stmt.column_count();
    let mut rows = stmt.query([]).with_context(|| format!("executing {sql}"))?;
    let mut buf = Vec::with_capacity(col_count);
    while let Some(row) = rows.next().context("pulling SQLite row")? {
        buf.clear();
        for i in 0..col_count {
            let val = row.get_ref(i).unwrap_or(ValueRef::Null);
            buf.push(value_to_csv_cell(val));
        }
        wtr.write_record(&buf).context("writing CSV row")?;
    }
    wtr.flush().context("flushing CSV writer")?;
    Ok(())
}

/// Map a SQLite value to its CSV cell form. NULL → empty (CSV
/// convention — every reader treats an empty unquoted field that way),
/// numbers and text → display form, BLOB → `X'…'` hex literal so the
/// dump is lossless and a downstream `INSERT` round-trips. Hex digits
/// are lowercase to match SQLite's own dump format.
fn value_to_csv_cell(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(r) => format!("{r}"),
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        ValueRef::Blob(bytes) => {
            let mut s = String::with_capacity(bytes.len() * 2 + 3);
            s.push_str("X'");
            for byte in bytes {
                let _ = write!(s, "{byte:02x}");
            }
            s.push('\'');
            s
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LeafKind {
    Schema,
    Contents,
}

struct ParsedKey<'a> {
    /// `sqlite_master.type` literal: `table` / `view` / `index` / `trigger`.
    master_kind: &'a str,
    /// Bare entity name (no kind prefix, no suffix).
    name: &'a str,
    /// Which leaf is being addressed — `.sql` (DDL dump) or `.csv`
    /// (row contents).
    leaf: LeafKind,
}

fn parse_key(key: &str) -> Option<ParsedKey<'_>> {
    let (kind_dir, rest) = key.split_once('/')?;
    let (leaf, name) = match rest.strip_suffix(SCHEMA_SUFFIX) {
        Some(n) => (LeafKind::Schema, n),
        None => (LeafKind::Contents, rest.strip_suffix(CONTENTS_SUFFIX)?),
    };
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
    // Contents only makes sense for row-bearing kinds; indexes /
    // triggers don't get a `.csv` leaf written by compose, but a
    // defensive reject here keeps a future bug from running
    // `SELECT * FROM <index>` (a SQLite syntax error).
    if matches!(leaf, LeafKind::Contents) && !matches!(master_kind, "table" | "view") {
        return None;
    }
    Some(ParsedKey {
        master_kind,
        name,
        leaf,
    })
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

/// Column names of `<entity>` via `PRAGMA table_info`. Works for both
/// real tables and views. Returns `Vec::new()` when SQLite reports
/// nothing — the caller falls back to a header-less CSV dump.
fn list_columns(conn: &Connection, entity: &str) -> Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({})", quote_ident(entity));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Build the dumped SQL text. SQLite stores the original `CREATE …`
/// without a trailing semicolon; add one so the file is a valid
/// standalone SQL script that a downstream `psql` / `sqlite3` can
/// execute. The leading comment is a hint, not metadata — peek's
/// SQL syntax highlighter renders it as a comment line.
fn format_ddl_dump(name: &str, db_name: &str, sql: &str) -> String {
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
    fn parse_key_accepts_each_kind_for_sql() {
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
            assert_eq!(p.leaf, LeafKind::Schema, "leaf for {key}");
        }
    }

    #[test]
    fn parse_key_accepts_csv_for_tables_and_views() {
        let t = parse_key("tables/books.csv").expect("tables/books.csv");
        assert_eq!(t.master_kind, "table");
        assert_eq!(t.name, "books");
        assert_eq!(t.leaf, LeafKind::Contents);
        let v = parse_key("views/popular_authors.csv").expect("views/popular_authors.csv");
        assert_eq!(v.master_kind, "view");
        assert_eq!(v.leaf, LeafKind::Contents);
    }

    #[test]
    fn parse_key_rejects_csv_for_non_row_bearing_kinds() {
        // Defensive — compose never writes these. A future bug would
        // be caught here rather than at `SELECT * FROM <index>` time.
        assert!(parse_key("indexes/idx_x.csv").is_none());
        assert!(parse_key("triggers/trg.csv").is_none());
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
    fn format_ddl_dump_appends_semicolon_and_header() {
        let out = format_ddl_dump("books", "library.sqlite", "CREATE TABLE books (id INT)");
        assert!(out.starts_with("-- books from library.sqlite\n"));
        assert!(out.contains("CREATE TABLE books (id INT);"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn format_ddl_dump_does_not_double_semicolon() {
        let out = format_ddl_dump("v", "db", "CREATE VIEW v AS SELECT 1;");
        let semis = out.matches(';').count();
        assert_eq!(semis, 1, "one ';' kept, no duplicate appended");
    }

    #[test]
    fn value_to_csv_cell_renders_each_variant() {
        assert_eq!(value_to_csv_cell(ValueRef::Null), "");
        assert_eq!(value_to_csv_cell(ValueRef::Integer(42)), "42");
        assert_eq!(value_to_csv_cell(ValueRef::Real(1.5)), "1.5");
        assert_eq!(value_to_csv_cell(ValueRef::Text(b"hi")), "hi");
        assert_eq!(
            value_to_csv_cell(ValueRef::Blob(&[0xde, 0xad, 0xbe, 0xef])),
            "X'deadbeef'"
        );
        assert_eq!(value_to_csv_cell(ValueRef::Blob(&[])), "X''");
    }
}
