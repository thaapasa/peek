//! `sqlite_master` walker. Yields the user-facing schema entities
//! grouped by kind, with a row count attached to each table/view.
//!
//! Internal `sqlite_*` shadow tables are filtered out — they're SQLite
//! bookkeeping (autoindex, stat tables) and have no place in a
//! user-facing listing.

use anyhow::{Context, Result};
use rusqlite::Connection;

use super::sql::quote_ident;

/// One schema entity (table, view, index, or trigger).
#[derive(Debug, Clone)]
pub struct Entity {
    pub name: String,
    /// Table this entity belongs to. For tables/views it equals `name`;
    /// for indexes / triggers it's the parent table. Captured from
    /// `sqlite_master` but not yet surfaced — reserved for showing an
    /// index/trigger's parent table in the listing.
    #[allow(dead_code)]
    pub tbl_name: String,
    /// Original `CREATE …` DDL as stored in `sqlite_master.sql`. `None`
    /// for entities SQLite synthesised internally without a SQL form.
    /// Drives the listing's `.sql`-leaf size column.
    pub sql: Option<String>,
    /// Row count from `SELECT COUNT(*) FROM <entity>`. Always 0 for
    /// indexes and triggers — they're not row-bearing on their own.
    pub row_count: u64,
}

#[derive(Debug, Default, Clone)]
pub struct SqliteCatalog {
    pub tables: Vec<Entity>,
    pub views: Vec<Entity>,
    pub indexes: Vec<Entity>,
    pub triggers: Vec<Entity>,
}

pub fn load(conn: &Connection) -> Result<SqliteCatalog> {
    let mut stmt = conn
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' \
             ORDER BY type, name",
        )
        .context("preparing sqlite_master scan")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SqliteMasterRow {
                kind: row.get::<_, String>(0)?,
                name: row.get::<_, String>(1)?,
                tbl_name: row.get::<_, String>(2)?,
                sql: row.get::<_, Option<String>>(3)?,
            })
        })
        .context("scanning sqlite_master")?;

    let mut catalog = SqliteCatalog::default();
    for r in rows {
        let r = r.context("reading sqlite_master row")?;
        let row_count = match r.kind.as_str() {
            "table" | "view" => count_rows(conn, &r.name).unwrap_or(0),
            _ => 0,
        };
        let entity = Entity {
            name: r.name,
            tbl_name: r.tbl_name,
            sql: r.sql,
            row_count,
        };
        match r.kind.as_str() {
            "table" => catalog.tables.push(entity),
            "view" => catalog.views.push(entity),
            "index" => catalog.indexes.push(entity),
            "trigger" => catalog.triggers.push(entity),
            // SQLite has no other kinds today; skip silently.
            _ => {}
        }
    }
    Ok(catalog)
}

struct SqliteMasterRow {
    kind: String,
    name: String,
    tbl_name: String,
    sql: Option<String>,
}

/// `SELECT COUNT(*) FROM "<name>"` with the identifier quoted per
/// SQL spec. Errors fold into `None` at the call site so a single
/// broken table doesn't fail the catalogue scan.
fn count_rows(conn: &Connection, name: &str) -> Result<u64> {
    let sql = format!("SELECT COUNT(*) FROM {}", quote_ident(name));
    let count: i64 = conn
        .query_row(&sql, [], |row| row.get(0))
        .with_context(|| format!("COUNT(*) on {name}"))?;
    Ok(count.max(0) as u64)
}
