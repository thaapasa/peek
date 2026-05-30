//! Spreadsheet workbook info shape.

use crate::types::document::DocumentMetadata;

use super::format::SpreadsheetFormat;

#[derive(Debug, Clone)]
pub struct SpreadsheetInfo {
    pub format: SpreadsheetFormat,
    /// Sheet names in workbook order.
    pub sheets: Vec<String>,
    /// Core document properties (title / creator / dates) from
    /// `docProps/core.xml` (OOXML) or `meta.xml` (ODS). Empty fields
    /// drop to `None`.
    pub metadata: DocumentMetadata,
    /// Reason the workbook couldn't be opened, if any — rendered in
    /// place of the stats.
    pub error: Option<String>,
}
