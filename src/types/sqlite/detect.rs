//! Detection helpers for SQLite databases.
//!
//! Magic-byte detection (the `"SQLite format 3\0"` prefix → MIME
//! `application/vnd.sqlite3` / `application/x-sqlite3`) flows through
//! the `infer` crate in `input::detect`; this module covers the
//! extension and MIME → format mapping invoked from there.

use super::format::SqliteFormat;

pub fn format_from_ext(ext: &str) -> Option<SqliteFormat> {
    match ext {
        "sqlite" | "sqlite3" | "db" | "db3" => Some(SqliteFormat::Sqlite),
        _ => None,
    }
}

pub fn format_from_mime(mime: &str) -> Option<SqliteFormat> {
    match mime {
        "application/vnd.sqlite3" | "application/x-sqlite3" => Some(SqliteFormat::Sqlite),
        _ => None,
    }
}
