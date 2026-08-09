//! Spreadsheet support: `.xlsx` / `.xlsm` / `.ods` workbooks.
//!
//! Structurally a spreadsheet is "several named sheets, each a table" —
//! the same shape as the SQLite viewer (entities → listing → drill into
//! one → streaming table view), so this reuses that compose pattern plus
//! the shared `RowsTableMode` / `RowSource` table stack. `calamine` does
//! the parsing (`workbook`); a materialised sheet is wrapped as a
//! `RowSource` whose alignment comes from calamine's native cell types.

pub mod compose;
pub mod extract;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod sheet_list;
pub mod workbook;
pub mod xml_props;

pub use info::SpreadsheetInfo;
/// Format enum, re-exported from `peek_detect` at the module root so
/// reader code keeps a local `crate::types::spreadsheet::SpreadsheetFormat` path.
pub use peek_detect::types::spreadsheet::SpreadsheetFormat;
