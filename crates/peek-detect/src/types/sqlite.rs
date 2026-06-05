//! SQLite database flavours. Single variant today; SQLCipher / future
//! on-disk flavours would slot in here.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteFormat {
    /// Plain SQLite 3 database (`.sqlite` / `.sqlite3` / `.db` / `.db3`).
    Sqlite,
}

impl SqliteFormat {
    /// Display label for the Format info row. Consumed by later patches
    /// once SQLite has its own `Format` row; the info section already
    /// renders the SQLite header without one.
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "SQLite 3",
        }
    }
}

// Detection helpers for SQLite databases.
//
// Magic-byte detection (the `"SQLite format 3\0"` prefix → MIME
// `application/vnd.sqlite3` / `application/x-sqlite3`) flows through
// the `infer` crate in `input::detect`; this module covers the
// extension and MIME → format mapping invoked from there.

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
