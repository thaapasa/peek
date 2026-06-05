//! SQLite info shape: file- and schema-level scrape, or a parse-error
//! surface. Mirrors the [`crate::types::objfile::info::ObjectInfo`]
//! shape — `stats` is `Some` exactly when `error` is `None`.

/// File-level + catalogue-level metadata for a SQLite database.
pub struct SqliteStats {
    /// `PRAGMA page_size` — physical page size in bytes (powers of 2,
    /// 512..65536).
    pub page_size: u32,
    /// `PRAGMA page_count` — total pages in the database.
    pub page_count: u32,
    /// `PRAGMA encoding` — "UTF-8" / "UTF-16le" / "UTF-16be".
    pub encoding: String,
    /// `PRAGMA schema_version` — bumped on every schema change.
    pub schema_version: i64,
    /// `PRAGMA user_version` — application-managed schema version.
    /// Zero is the default and rendered only when non-zero.
    pub user_version: i64,
    /// `PRAGMA application_id` — 32-bit magic identifying the
    /// application that owns the schema (e.g. Fossil SCM uses this).
    /// Zero by default; rendered as hex when non-zero.
    pub application_id: i64,
    /// `PRAGMA journal_mode` — "delete" / "wal" / "memory" / etc.
    pub journal_mode: String,
    /// `PRAGMA integrity_check(1)` returned the string `"ok"`.
    pub integrity_ok: bool,
    pub table_count: usize,
    pub view_count: usize,
    pub index_count: usize,
    pub trigger_count: usize,
    /// Sum of `COUNT(*)` across user tables.
    pub total_rows: u64,
    /// Up to a handful of the biggest tables by row count, for the
    /// info panel's quick-scan section.
    pub top_tables: Vec<(String, u64)>,
}

/// SQLite metadata, or the reason scraping it failed.
pub struct SqliteInfo {
    pub stats: Option<SqliteStats>,
    pub error: Option<String>,
}

impl SqliteInfo {
    pub fn ok(stats: SqliteStats) -> Self {
        Self {
            stats: Some(stats),
            error: None,
        }
    }

    pub fn err(msg: String) -> Self {
        Self {
            stats: None,
            error: Some(msg),
        }
    }
}
