//! Spreadsheet workbook format. All three route through `calamine` —
//! the format only labels the Info section and the extension allow-list,
//! and picks which `calamine` reader to construct.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadsheetFormat {
    /// Excel OOXML workbook.
    Xlsx,
    /// Macro-enabled Excel OOXML workbook (same on-disk format).
    Xlsm,
    /// OpenDocument Spreadsheet.
    Ods,
}

impl SpreadsheetFormat {
    /// Human label for the Info section header.
    pub fn label(self) -> &'static str {
        match self {
            SpreadsheetFormat::Xlsx => "Excel",
            SpreadsheetFormat::Xlsm => "Excel (macro-enabled)",
            SpreadsheetFormat::Ods => "OpenDocument Spreadsheet",
        }
    }

    /// Whether this is an OOXML (Excel) container — `docProps/core.xml`
    /// metadata vs ODS's `meta.xml`.
    pub fn is_ooxml(self) -> bool {
        matches!(self, SpreadsheetFormat::Xlsx | SpreadsheetFormat::Xlsm)
    }
}
