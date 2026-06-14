//! SQLite info gathering: open the database read-only, scrape file-
//! and catalogue-level metadata, package it into [`SqliteInfo`].
//!
//! Parse / open failures land in `SqliteInfo::err` rather than
//! bubbling up, so the Info view always renders (matches the
//! [`crate::types::objfile`] convention).

use rusqlite::Connection;

use super::catalog;
use super::info::{SqliteInfo, SqliteStats};
use super::reader::SqliteReader;
use crate::info::Extras;
use peek_io::InputSource;

/// Cap on the number of biggest tables surfaced in the Info section.
/// Keep small — the Info view is for at-a-glance scanning, not
/// browsing the whole schema.
const TOP_TABLES_CAP: usize = 5;

pub fn gather_extras(source: &InputSource) -> Extras {
    Box::new(gather(source))
}

fn gather(source: &InputSource) -> SqliteInfo {
    let reader = match SqliteReader::open(source) {
        Ok(r) => r,
        Err(e) => return SqliteInfo::err(format!("open failed: {e:#}")),
    };
    match collect(&reader.conn) {
        Ok(stats) => SqliteInfo::ok(stats),
        Err(e) => SqliteInfo::err(format!("scrape failed: {e:#}")),
    }
}

fn collect(conn: &Connection) -> anyhow::Result<SqliteStats> {
    let page_size: u32 = pragma_value(conn, "page_size").unwrap_or(0);
    let page_count: u32 = pragma_value(conn, "page_count").unwrap_or(0);
    let encoding: String = pragma_value(conn, "encoding").unwrap_or_else(|| "?".to_string());
    let schema_version: i64 = pragma_value(conn, "schema_version").unwrap_or(0);
    let user_version: i64 = pragma_value(conn, "user_version").unwrap_or(0);
    let application_id: i64 = pragma_value(conn, "application_id").unwrap_or(0);
    let journal_mode: String =
        pragma_value(conn, "journal_mode").unwrap_or_else(|| "?".to_string());
    let integrity_ok = integrity_check(conn);

    let catalog = catalog::load(conn)?;
    let total_rows: u64 = catalog.tables.iter().map(|t| t.row_count).sum();
    let mut by_size = catalog.tables.clone();
    by_size.sort_by_key(|t| std::cmp::Reverse(t.row_count));
    let top_tables: Vec<(String, u64)> = by_size
        .into_iter()
        .take(TOP_TABLES_CAP)
        .map(|t| (t.name, t.row_count))
        .collect();

    Ok(SqliteStats {
        page_size,
        page_count,
        encoding,
        schema_version,
        user_version,
        application_id,
        journal_mode,
        integrity_ok,
        table_count: catalog.tables.len(),
        view_count: catalog.views.len(),
        index_count: catalog.indexes.len(),
        trigger_count: catalog.triggers.len(),
        total_rows,
        top_tables,
    })
}

/// `PRAGMA <name>` → single value, typed by the caller.
fn pragma_value<T>(conn: &Connection, name: &str) -> Option<T>
where
    T: rusqlite::types::FromSql,
{
    conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .ok()
}

/// Run `PRAGMA integrity_check(1)` — capped at one error so a broken
/// DB doesn't spool a long error list into the Info path. Returns
/// `true` when SQLite reports the single literal "ok".
fn integrity_check(conn: &Connection) -> bool {
    conn.query_row("PRAGMA integrity_check(1)", [], |row| {
        row.get::<_, String>(0)
    })
    .map(|s| s == "ok")
    .unwrap_or(false)
}
