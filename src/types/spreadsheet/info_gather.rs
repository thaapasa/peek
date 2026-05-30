//! Populate [`SpreadsheetInfo`]: sheet list (cheap, container metadata)
//! plus core document properties.

use crate::info::FileExtras;
use crate::input::InputSource;

use super::format::SpreadsheetFormat;
use super::info::SpreadsheetInfo;
use super::workbook::Workbook;
use super::xml_props;

pub fn gather_extras(source: &InputSource, fmt: SpreadsheetFormat) -> FileExtras {
    let (sheets, error) = match Workbook::open(source, fmt) {
        Ok(wb) => (wb.sheet_names(), None),
        Err(e) => (Vec::new(), Some(format!("{e:#}"))),
    };
    let metadata = xml_props::read_metadata(source, fmt).unwrap_or_default();
    FileExtras::Spreadsheet(SpreadsheetInfo {
        format: fmt,
        sheets,
        metadata,
        error,
    })
}
