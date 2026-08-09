//! Populate [`SpreadsheetInfo`]: sheet list (cheap, container metadata)
//! plus core document properties.

use peek_io::InputSource;

use super::SpreadsheetFormat;
use super::info::SpreadsheetInfo;
use super::workbook::Workbook;
use super::xml_props;
use crate::info::Extras;

pub fn gather_extras(source: &InputSource, fmt: SpreadsheetFormat) -> Extras {
    let (sheets, error) = match Workbook::open(source, fmt) {
        Ok(wb) => (wb.sheet_names(), None),
        Err(e) => (Vec::new(), Some(format!("{e:#}"))),
    };
    let metadata = xml_props::read_metadata(source, fmt).unwrap_or_default();
    Box::new(SpreadsheetInfo {
        format: fmt,
        sheets,
        metadata,
        error,
    })
}
