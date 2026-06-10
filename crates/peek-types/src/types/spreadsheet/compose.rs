//! Spreadsheet compose: a `Sheets` listing whose rows drill into a
//! streaming table view (mirrors the SQLite entities → contents flow),
//! plus a secondary raw ZIP-entry listing (workbooks are zip
//! containers) and the universal Info tail.

use anyhow::{Result, anyhow};

use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, Detected, SpreadsheetFormat};
use crate::types::archive;
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::{DescendFrame, ExtractTarget, Mode};
use crate::viewer::table::rows_mode::RowsTableMode;

use super::sheet_list::SheetListSource;
use super::workbook::Workbook;

/// Suffix on a sheet's listing row. Mirrors SQLite's contents rows:
/// Enter drills into the table view, `e` extracts the sheet to a CSV
/// file. Also the marker `extract` uses to tell a sheet key apart from
/// a raw zip-entry path.
pub(crate) const SHEET_SUFFIX: &str = ".csv";

pub fn compose(
    source: &InputSource,
    detected: &Detected,
    _args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: SpreadsheetFormat,
) -> Result<()> {
    let (names, warnings) = match Workbook::open(source, fmt) {
        Ok(wb) => (wb.sheet_names(), Vec::new()),
        Err(e) => (Vec::new(), vec![format!("Failed to open workbook: {e:#}")]),
    };

    let descend_source = source.clone();
    let descend_detected = detected.clone();
    let sheets = SheetListSource::new(&names, fmt.label());
    let listing = ListingMode::from_source(Box::new(sheets), "Sheets", warnings)
        .with_descend_handler(move |target| {
            let ExtractTarget::EntryPath(key) = target else {
                return None;
            };
            let name = key.strip_suffix(SHEET_SUFFIX)?;
            Some(build_sheet_frame(
                &descend_source,
                &descend_detected,
                fmt,
                name,
            ))
        });
    modes.push(Box::new(listing));

    // Secondary view: the workbook's raw zip entries. Browse / extract
    // through the standard archive path (extract delegates non-sheet
    // keys to the zip extractor).
    if let Ok(zip_entries) = archive::reader::list_entries(source, ArchiveFormat::Zip)
        && !zip_entries.is_empty()
    {
        modes.push(Box::new(ListingMode::new(
            "ZIP",
            "Files",
            zip_entries,
            Vec::new(),
        )));
    }
    Ok(())
}

fn build_sheet_frame(
    source: &InputSource,
    detected: &Detected,
    fmt: SpreadsheetFormat,
    sheet: &str,
) -> Result<DescendFrame> {
    let mut wb = Workbook::open(source, fmt)?;
    let data = wb
        .materialize(sheet)
        .map_err(|e| anyhow!("opening sheet {sheet}: {e:#}"))?;
    let aligns = data.alignments();
    let has_header = data.header_detected();
    let table = RowsTableMode::new(Box::new(data), aligns, has_header, "Sheet");
    let mut modes: Vec<Box<dyn Mode>> = vec![Box::new(table)];
    // No Hex: the frame reuses the whole-workbook source, so a hex dump
    // would show the container, not this sheet.
    crate::viewer::append_universal_modes(&mut modes, None)?;
    Ok(DescendFrame {
        source: source.clone(),
        detected: detected.clone(),
        modes,
        breadcrumb_label: Some(sheet.to_string()),
    })
}
