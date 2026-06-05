//! Extract handler for spreadsheet listing rows.
//!
//! Two listings feed this: the Sheets listing (rows `<sheet>.csv`) and
//! the raw ZIP-entry listing. A `.csv` key naming a real sheet streams
//! that sheet to a CSV tempfile; anything else is a container zip path,
//! delegated to the archive extractor (so the Files listing browses /
//! extracts the workbook's internals like any other zip).

use anyhow::anyhow;

use crate::extract::{ExtractError, ExtractOptions, Extracted};
use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, SpreadsheetFormat};

use super::compose::SHEET_SUFFIX;
use super::workbook::Workbook;

pub fn extract(
    source: &InputSource,
    key: &str,
    fmt: SpreadsheetFormat,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    if let Some(sheet) = key.strip_suffix(SHEET_SUFFIX) {
        let mut wb = Workbook::open(source, fmt).map_err(ExtractError::Other)?;
        if wb.sheet_names().iter().any(|n| n == sheet) {
            return extract_sheet_csv(&mut wb, sheet);
        }
    }
    // Not a sheet row → a raw zip-container entry.
    crate::types::archive::extract::extract(source, ArchiveFormat::Zip, key, opts)
}

fn extract_sheet_csv(wb: &mut Workbook, sheet: &str) -> Result<Extracted, ExtractError> {
    let data = wb
        .materialize(sheet)
        .map_err(|e| ExtractError::Other(anyhow!("reading sheet {sheet}: {e:#}")))?;

    let tmp = tempfile::NamedTempFile::new()
        .map_err(|e| ExtractError::Other(anyhow!("creating CSV temp spool: {e}")))?;
    {
        let mut wtr = csv::Writer::from_writer(tmp.as_file());
        for row in data.csv_rows() {
            // NULL / empty cell → empty CSV field (lossless for text).
            wtr.write_record(row.iter().map(|c| c.as_deref().unwrap_or("")))
                .map_err(|e| ExtractError::Other(anyhow!("writing CSV: {e}")))?;
        }
        wtr.flush()
            .map_err(|e| ExtractError::Other(anyhow!("flushing CSV: {e}")))?;
    }

    let suggested_name = format!("{sheet}{SHEET_SUFFIX}");
    Ok(Extracted {
        source: InputSource::temp_file(tmp, suggested_name.clone()),
        suggested_name,
    })
}
