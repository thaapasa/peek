//! [`RowSource`] implementation over a SQLite table or view.
//!
//! Holds a sliding window of body rows ([`WINDOW_SIZE`] entries) plus
//! a synthetic header row (the column names). When the table-view mode
//! asks for a row outside the window, the buffer reloads via
//! `SELECT * FROM "<entity>" LIMIT ? OFFSET ?`, centred on the
//! requested row so a small viewport movement doesn't immediately
//! evict everything that surrounds it.
//!
//! Row count comes from a one-shot `COUNT(*)` at construction so
//! [`RowSource::total`] is always definite and scrollbar / max-top
//! math works without driving the cursor to EOF.
//!
//! Cell stringification: NULL → `None`, INTEGER / REAL / TEXT → their
//! natural string form, BLOB → `<blob: N bytes>` placeholder (hex
//! preview deferred). The `Option<String>` shape matches what
//! [`crate::viewer::table::row_source::RowSource`] requires; the table
//! view paints `None` distinctly from an empty string.

use std::cmp::min;

use anyhow::{Context, Result, anyhow};
use rusqlite::types::ValueRef;

use crate::input::InputSource;
use crate::types::sqlite::reader::SqliteReader;
use crate::types::sqlite::sql::quote_ident;
use crate::viewer::table::row_source::RowSource;
use crate::viewer::table::rows_mode::Alignment;

/// Sliding-window size in body rows. 1000 fits comfortably in memory
/// for any realistic column count and keeps OFFSET re-queries
/// infrequent under normal scrolling (≈ 25 viewports of typical
/// terminal height between refills).
pub const WINDOW_SIZE: usize = 1000;

pub struct SqliteRowSet {
    reader: SqliteReader,
    /// Quoted entity name ready to splice into a `FROM` clause.
    entity_sql: String,
    /// Column names — also rendered as the synthetic header row.
    columns: Vec<String>,
    header_cells: Vec<Option<String>>,
    /// Declared per-column types from `PRAGMA table_info`. Drives
    /// alignment inference and stays around for any future type-aware
    /// rendering (e.g. dimmed nulls in numeric columns).
    column_types: Vec<String>,
    /// Total body rows (excludes the synthetic header). Cached at
    /// construction via `COUNT(*)`.
    total_body_rows: usize,
    /// Lowest body-row index currently in `window`.
    window_start: usize,
    /// Materialised body rows. `window[i]` is body row `window_start + i`.
    window: Vec<Vec<Option<String>>>,
    /// Empty cells vector cached so `row()` can return a stable empty
    /// slice when the underlying source is empty.
    empty: Vec<Option<String>>,
}

impl SqliteRowSet {
    pub fn open(source: &InputSource, entity: &str) -> Result<Self> {
        let reader = SqliteReader::open(source)?;
        let entity_sql = quote_ident(entity);
        let cols = pragma_table_info(&reader.conn, entity)
            .with_context(|| format!("listing columns for {entity} via PRAGMA table_info"))?;
        if cols.is_empty() {
            return Err(anyhow!(
                "no columns reported for {entity}; entity may not exist or have a queryable shape"
            ));
        }
        let columns: Vec<String> = cols.iter().map(|c| c.name.clone()).collect();
        let column_types: Vec<String> = cols.iter().map(|c| c.declared_type.clone()).collect();
        let header_cells: Vec<Option<String>> = columns.iter().map(|c| Some(c.clone())).collect();
        let total_body_rows = count_rows(&reader.conn, &entity_sql)?;

        let mut row_set = Self {
            reader,
            entity_sql,
            columns,
            header_cells,
            column_types,
            total_body_rows,
            window_start: 0,
            window: Vec::new(),
            empty: Vec::new(),
        };
        // Seed the window so the first render finds rows without
        // waiting for the mode's explicit ensure_row pull.
        row_set.refill_around(0)?;
        Ok(row_set)
    }

    /// Per-column alignment derived from declared types. Numeric
    /// columns right-align so digit grids line up; everything else
    /// (TEXT / BLOB / unknown) stays left. Matches the CSV viewer's
    /// inference rule for visual consistency.
    pub fn alignments(&self) -> Vec<Alignment> {
        self.column_types
            .iter()
            .map(|t| {
                if is_numeric_type(t) {
                    Alignment::Right
                } else {
                    Alignment::Left
                }
            })
            .collect()
    }

    /// Refill the window so it covers `target_body` body-row index.
    /// Centred on the target — gives equal scroll room in both
    /// directions before the next refill.
    fn refill_around(&mut self, target_body: usize) -> Result<()> {
        if self.total_body_rows == 0 {
            self.window_start = 0;
            self.window.clear();
            return Ok(());
        }
        let half = WINDOW_SIZE / 2;
        let raw_start = target_body.saturating_sub(half);
        let max_start = self.total_body_rows.saturating_sub(WINDOW_SIZE);
        let start = min(raw_start, max_start);
        let limit = min(WINDOW_SIZE, self.total_body_rows - start);
        let rows = fetch_window(
            &self.reader.conn,
            &self.entity_sql,
            &self.columns,
            start,
            limit,
        )?;
        self.window_start = start;
        self.window = rows;
        Ok(())
    }

    /// Whether body row `body_idx` is currently buffered.
    fn body_in_window(&self, body_idx: usize) -> bool {
        body_idx >= self.window_start && body_idx < self.window_start + self.window.len()
    }
}

impl RowSource for SqliteRowSet {
    fn ensure_row(&mut self, idx: usize) -> Result<usize> {
        // idx 0 is the synthetic header — always available.
        if idx == 0 || self.total_body_rows == 0 {
            return Ok(self.loaded());
        }
        let body_idx = idx - 1;
        let target_body = min(body_idx, self.total_body_rows.saturating_sub(1));
        if !self.body_in_window(target_body) {
            self.refill_around(target_body)?;
        }
        Ok(self.loaded())
    }

    fn ensure_all(&mut self) -> Result<()> {
        // Total is known up front; loaded() already reports it. No
        // need to drive a cursor — cell-scoped search will only see
        // the current window (a known step-4 limitation; full-scan
        // search is deferred).
        Ok(())
    }

    fn row(&self, idx: usize) -> Option<&[Option<String>]> {
        if idx == 0 {
            return Some(if self.columns.is_empty() {
                &self.empty
            } else {
                &self.header_cells
            });
        }
        let body_idx = idx - 1;
        if !self.body_in_window(body_idx) {
            return None;
        }
        let local = body_idx - self.window_start;
        self.window.get(local).map(Vec::as_slice)
    }

    fn loaded(&self) -> usize {
        // Header + all data rows are addressable: data rows outside
        // the window resolve via `ensure_row` refill.
        1 + self.total_body_rows
    }

    fn total(&self) -> Option<usize> {
        Some(1 + self.total_body_rows)
    }

    fn column_count(&self) -> usize {
        self.columns.len()
    }
}

struct ColumnInfo {
    name: String,
    declared_type: String,
}

fn pragma_table_info(conn: &rusqlite::Connection, entity: &str) -> Result<Vec<ColumnInfo>> {
    let sql = format!("PRAGMA table_info({})", quote_ident(entity));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(ColumnInfo {
            name: row.get::<_, String>(1)?,
            declared_type: row.get::<_, String>(2).unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn count_rows(conn: &rusqlite::Connection, entity_sql: &str) -> Result<usize> {
    let sql = format!("SELECT COUNT(*) FROM {entity_sql}");
    let count: i64 = conn
        .query_row(&sql, [], |row| row.get(0))
        .with_context(|| format!("COUNT(*) on {entity_sql}"))?;
    Ok(count.max(0) as usize)
}

fn fetch_window(
    conn: &rusqlite::Connection,
    entity_sql: &str,
    columns: &[String],
    offset: usize,
    limit: usize,
) -> Result<Vec<Vec<Option<String>>>> {
    let sql = format!("SELECT * FROM {entity_sql} LIMIT ?1 OFFSET ?2");
    let mut stmt = conn.prepare(&sql)?;
    let col_count = columns.len();
    let rows = stmt
        .query_map(rusqlite::params![limit as i64, offset as i64], |row| {
            let mut cells = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let raw = row.get_ref(i).unwrap_or(ValueRef::Null);
                cells.push(value_to_cell(raw));
            }
            Ok(cells)
        })
        .with_context(|| format!("paging {entity_sql} LIMIT {limit} OFFSET {offset}"))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// SQLite value → display cell. NULL stays `None`; everything else
/// stringifies. BLOBs get a `<blob: N bytes>` placeholder so the
/// table view doesn't try to render binary as text. Hex preview for
/// small BLOBs is deferred — too noisy by default for the common
/// "large opaque payload" case.
fn value_to_cell(v: ValueRef<'_>) -> Option<String> {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(r) => Some(format!("{r}")),
        ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => Some(format!("<blob: {} bytes>", bytes.len())),
    }
}

/// Whether a declared column type should render right-aligned (i.e.
/// it stores numbers users want to scan as digit columns). Looser
/// than SQLite's storage affinity — we look at *display* shape:
/// `DATE`, `DATETIME`, `BOOLEAN` get NUMERIC storage affinity per
/// SQLite, but the values are text-shaped and left-align reads
/// better. Substring match (not prefix) so `BIGINT`,
/// `DOUBLE PRECISION`, `MEDIUMINT` classify correctly without
/// enumerating every spelling.
fn is_numeric_type(declared: &str) -> bool {
    let upper = declared.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return false;
    }
    // Text markers win first — a column declared `INT` would never
    // ship inside a CHAR type, but defensive ordering keeps mixed
    // spellings honest.
    if upper.contains("CHAR")
        || upper.contains("CLOB")
        || upper.contains("TEXT")
        || upper.contains("BLOB")
        || upper.contains("DATE")
        || upper.contains("TIME")
        || upper.contains("BOOL")
    {
        return false;
    }
    upper.contains("INT")
        || upper.contains("REAL")
        || upper.contains("FLOA")
        || upper.contains("DOUB")
        || upper.contains("NUMERIC")
        || upper.contains("DECIMAL")
        || upper.contains("NUM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_type_classification() {
        assert!(is_numeric_type("INTEGER"));
        assert!(is_numeric_type("int"));
        assert!(is_numeric_type("BIGINT"));
        assert!(is_numeric_type("REAL"));
        assert!(is_numeric_type("DOUBLE PRECISION"));
        assert!(is_numeric_type("NUMERIC(10,2)"));
        assert!(!is_numeric_type("TEXT"));
        assert!(!is_numeric_type("BLOB"));
        assert!(!is_numeric_type(""));
        assert!(!is_numeric_type("DATETIME"));
    }

    #[test]
    fn library_fixture_pages_through_books_table() {
        use std::path::PathBuf;
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-data/library.sqlite");
        let src = InputSource::File(p);
        let mut rs = SqliteRowSet::open(&src, "books").expect("open books");

        // Synthetic header carries the column names.
        let header = rs.row(0).expect("header row");
        let names: Vec<&str> = header.iter().map(|c| c.as_deref().unwrap_or("")).collect();
        assert!(names.contains(&"id"));
        assert!(names.contains(&"title"));

        // Total = 1 + body rows (2500 in the fixture).
        assert_eq!(rs.loaded(), 2501);
        assert_eq!(rs.total(), Some(2501));

        // Row 1 (first body row) is in the seeded window.
        let r1 = rs.row(1).expect("first body row");
        assert_eq!(r1.len(), names.len());
        // PG #1 is "The Declaration of Independence..."
        let title = r1[1].as_deref().expect("title not null");
        assert!(title.contains("Declaration"));

        // Force a window slide: ask for a row past the initial window.
        let far = 2400usize;
        rs.ensure_row(far + 1).expect("ensure far row");
        let r_far = rs.row(far + 1).expect("row after slide");
        assert!(!r_far.is_empty());

        // Header still available after a slide.
        assert!(rs.row(0).is_some());
    }

    #[test]
    fn value_to_cell_maps_each_variant() {
        assert_eq!(value_to_cell(ValueRef::Null), None);
        assert_eq!(value_to_cell(ValueRef::Integer(42)), Some("42".to_string()));
        assert_eq!(value_to_cell(ValueRef::Real(1.5)), Some("1.5".to_string()));
        assert_eq!(value_to_cell(ValueRef::Text(b"hi")), Some("hi".to_string()));
        assert_eq!(
            value_to_cell(ValueRef::Blob(&[0u8; 8])),
            Some("<blob: 8 bytes>".to_string())
        );
    }
}
