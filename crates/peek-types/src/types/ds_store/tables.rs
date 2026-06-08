//! The `.DS_Store` records table — one row per stored Finder property.
//! Layout and painting live in the shared `viewer::table::TableMode`;
//! this builds the structured data from a parsed [`DsStore`].

use super::format::{format_value, property_label};
use super::reader::DsStore;
use crate::viewer::table::{Align, Cell, CellRole, Table, cell, fit_columns};

/// Build the records table: `File │ Property │ Code │ Value`. Rows arrive
/// in B-tree key order (by filename), which is already the natural
/// grouping.
pub fn build(store: &DsStore) -> Table {
    let rows: Vec<Vec<Cell>> = store
        .records
        .iter()
        .map(|r| {
            let property = match property_label(&r.code) {
                Some(label) => cell(label.to_string(), CellRole::Primary),
                None => cell("—".to_string(), CellRole::Muted),
            };
            vec![
                cell(r.name.clone(), CellRole::Name),
                property,
                cell(r.code.clone(), CellRole::Tag),
                cell(format_value(r), CellRole::Name),
            ]
        })
        .collect();
    let columns = fit_columns(
        &[
            ("File", Align::Left),
            ("Property", Align::Left),
            ("Code", Align::Left),
            ("Value", Align::Left),
        ],
        &rows,
    );
    let notice = if store.truncated {
        Some("(parse stopped early — some records may be missing)".to_string())
    } else if rows.is_empty() {
        Some("(no records)".to_string())
    } else {
        None
    };
    Table {
        columns,
        rows,
        notice,
    }
}
