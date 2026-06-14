//! Thin SQLite-side constructor for the shared streaming table view.
//!
//! Opens a [`SqliteRowSet`] over `<entity>` (a table or view in the
//! database), derives per-column alignment from the SQLite type
//! affinities, and wraps the lot in a generic [`RowsTableMode`] with
//! `has_header = true` so the synthetic header row from
//! `SqliteRowSet::row(0)` renders as the table heading.

use anyhow::Result;

use crate::types::sqlite::row_set::SqliteRowSet;
use crate::viewer::table::rows_mode::RowsTableMode;
use peek_io::InputSource;

/// Fixed mode label for SQLite contents views. Matches what the
/// generic table mode shows in the status bar — kept stable so users
/// can quickly tell they're looking at row data, not the schema
/// listing.
const TABLE_MODE_LABEL: &str = "Rows";

pub fn build(source: &InputSource, entity: &str) -> Result<RowsTableMode> {
    let row_set = SqliteRowSet::open(source, entity)?;
    let aligns = row_set.alignments();
    Ok(RowsTableMode::new(
        Box::new(row_set),
        aligns,
        true,
        TABLE_MODE_LABEL,
    ))
}
