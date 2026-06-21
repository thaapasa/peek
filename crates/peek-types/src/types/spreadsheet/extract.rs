//! Extract handler for spreadsheet listing rows.
//!
//! Two listings feed this: the Sheets listing (rows `<sheet>.csv`) and
//! the raw ZIP-entry listing. A `.csv` key naming a real sheet streams
//! that sheet to a CSV tempfile; anything else is a container zip path,
//! delegated to the archive extractor (so the Files listing browses /
//! extracts the workbook's internals like any other zip).

use anyhow::anyhow;

use crate::extract::{ExtractError, ExtractOptions, Extracted, sanitize_entry_path};
use peek_detect::{ArchiveFormat, SpreadsheetFormat};
use peek_io::InputSource;

use super::compose::SHEET_SUFFIX;
use super::workbook::Workbook;

pub fn extract(
    source: &InputSource,
    key: &str,
    fmt: SpreadsheetFormat,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    if let Some(stripped) = key.strip_suffix(SHEET_SUFFIX) {
        let mut wb = Workbook::open(source, fmt).map_err(ExtractError::Other)?;
        // Two callers key this differently: the `--list` pipe carries the
        // *sanitized* sheet name (`flat_line` sanitises before painting),
        // while the interactive descend/extract path carries the raw name.
        // Accept either form, then materialise with the raw name calamine
        // indexes by — keeps the list-key → extract-key round-trip intact
        // for control-char sheet names.
        let raw = wb
            .sheet_names()
            .into_iter()
            .find(|n| n == stripped || peek_io::sanitize_terminal_controls(n) == stripped);
        if let Some(raw) = raw {
            return extract_sheet_csv(&mut wb, &raw);
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

    let suggested_name = format!("{}{SHEET_SUFFIX}", safe_sheet_filename(sheet));
    Ok(Extracted {
        source: InputSource::temp_file(tmp, suggested_name.clone()),
        suggested_name,
    })
}

/// Reduce a workbook-declared sheet name to a safe download basename.
///
/// Sheet names are unsanitized — a hand-built file can put `../` or an
/// absolute path in one. The TOC lookup keys on the raw name (calamine
/// needs the exact string), but the suggested *filename* must carry no
/// path separators, or a no-`-o` extract would let the name steer the
/// write outside the cwd (`write::Output::resolve` uses it verbatim).
/// Mirrors the email / pdf extractors. Falls back to `sheet` for names
/// that don't survive sanitisation (pure traversal, empty).
fn safe_sheet_filename(sheet: &str) -> String {
    sanitize_entry_path(sheet)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "sheet".to_string())
}

#[cfg(test)]
mod tests {
    use super::safe_sheet_filename;

    #[test]
    fn traversal_names_reduce_to_safe_basename() {
        // No result may carry a path separator or escape the cwd.
        for evil in [
            "../../../../tmp/PWNED",
            "/etc/cron.d/x",
            "..",
            "../..",
            "a/b/../../c",
        ] {
            let out = safe_sheet_filename(evil);
            assert!(!out.contains('/'), "{evil:?} → {out:?} leaked a separator");
            assert_ne!(out, "..", "{evil:?} → {out:?}");
        }
    }

    #[test]
    fn nested_name_keeps_only_basename() {
        assert_eq!(safe_sheet_filename("dir/Sheet1"), "Sheet1");
    }

    #[test]
    fn ordinary_name_passes_through() {
        assert_eq!(safe_sheet_filename("Sheet1"), "Sheet1");
    }
}
