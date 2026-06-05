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

// Spreadsheet detection: extension + MIME.
//
// Like the other OOXML / ODF containers (docx, odt, epub), workbooks
// magic-detect as `application/zip`, so the extension is what routes
// them here; a magic-only zip with no spreadsheet extension falls
// through to the archive viewer.

/// Map a lowercase extension to a spreadsheet format.
pub fn format_from_ext(ext: &str) -> Option<SpreadsheetFormat> {
    match ext {
        "xlsx" => Some(SpreadsheetFormat::Xlsx),
        "xlsm" => Some(SpreadsheetFormat::Xlsm),
        "ods" => Some(SpreadsheetFormat::Ods),
        _ => None,
    }
}

/// Map an IANA MIME to a spreadsheet format. Used for the
/// extension-mismatch allow-list and any future content sniff.
pub fn format_from_mime(mime: &str) -> Option<SpreadsheetFormat> {
    match mime {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            Some(SpreadsheetFormat::Xlsx)
        }
        "application/vnd.ms-excel.sheet.macroenabled.12" => Some(SpreadsheetFormat::Xlsm),
        "application/vnd.oasis.opendocument.spreadsheet" => Some(SpreadsheetFormat::Ods),
        _ => None,
    }
}
