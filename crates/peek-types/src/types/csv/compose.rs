//! Per-type compose: aligned table view + paired Source ContentMode for
//! CSV / TSV. Info / Hex / Help / About are appended by the central
//! `Registry::compose_modes` tail.
//!
//! A source that won't open as CSV (e.g. an over-cap UTF-16 file whose
//! transcode the memory budget refuses) degrades to the Source view
//! alone — the streaming raw view works on any bytes — with the reason
//! carried as a warning. It must not fail the whole open: every sibling
//! budget gate degrades, and `main.rs` turns a compose error into a
//! process-level failure.

use std::rc::Rc;

use anyhow::Result;
use peek_detect::Detected;
use peek_io::InputSource;

use crate::types::csv::CsvFormat;
use crate::types::csv::parse::{CellKind, CsvData, classify_cell};
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::modes::{ContentMode, ContentModeConfig, Mode};
use crate::viewer::table::rows_mode::{Alignment, RowsTableMode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: CsvFormat,
) -> Result<()> {
    let table_refused = match CsvData::open(source, fmt) {
        Ok(data) => {
            modes.push(Box::new(build_csv_mode(data)));
            None
        }
        Err(e) => Some(format!("table view unavailable: {e:#}")),
    };
    // Paired Source view: raw CSV bytes, no syntax token (no robust CSV
    // syntax shipped with two-face).
    let line_source = source.open_line_source()?;
    let mut content = ContentMode::new(
        source.clone(),
        line_source,
        Rc::clone(&ctx.theme_manager),
        ctx.theme_manager.theme_name,
        ContentModeConfig {
            label: "Source",
            line_numbers: args.line_numbers,
            ..Default::default()
        },
    );
    if let Some(warning) = table_refused {
        content.push_warning(warning);
    }
    modes.push(Box::new(content));
    Ok(())
}

/// Wrap a parsed `CsvData` in a `RowsTableMode`. The CSV-specific bits
/// (alignment inference via `classify_cell`, header-heuristic seed)
/// stay here so the shared mode keeps no per-source logic. Exposed
/// `pub(crate)` so tests in the mode itself can build a CSV-backed instance.
pub(crate) fn build_csv_mode(data: CsvData) -> RowsTableMode {
    let has_header = data.header_heuristic;
    let body_start = if has_header { 1 } else { 0 };
    let align = infer_alignments(&data, body_start);
    RowsTableMode::new(Box::new(data), align, has_header, "Table")
}

/// Infer per-column alignment from the seed body. Right-align when
/// every non-empty body cell classifies as Int or Float and at least
/// one such cell exists; otherwise left.
pub fn infer_alignments(data: &CsvData, body_start: usize) -> Vec<Alignment> {
    let cols = data.column_count();
    let mut numeric = vec![true; cols];
    let mut any_typed = vec![false; cols];
    for rec in data.seed.iter().skip(body_start) {
        if rec.malformed {
            continue;
        }
        for (i, cell) in rec.cells.iter().enumerate().take(cols) {
            match classify_cell(cell.as_deref().unwrap_or("")) {
                CellKind::Empty => {}
                CellKind::Int | CellKind::Float => any_typed[i] = true,
                _ => numeric[i] = false,
            }
        }
    }
    (0..cols)
        .map(|i| {
            if numeric[i] && any_typed[i] {
                Alignment::Right
            } else {
                Alignment::Left
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use peek_detect::FileType;
    use peek_theme::{PeekThemeName, StyleMode, ThemeManager};

    use super::*;

    /// A CSV whose table view refuses to open (here: an over-cap UTF-16
    /// file whose transcode the memory budget rejects) must degrade to
    /// the Source view with a warning — not propagate the error.
    /// `main.rs` turns a compose error into a process-level failure, so
    /// a propagated refusal would lose Source / Hex / Info entirely.
    #[test]
    fn over_cap_utf16_degrades_to_source_view() {
        let cap = peek_io::limits::WHOLE_DOC_BYTES as usize;
        let mut buf = vec![0xFF, 0xFE];
        buf.resize(cap + 2, b' ');
        let source = InputSource::stdin(Bytes::from(buf));
        let detected = Detected {
            file_type: FileType::Csv(CsvFormat::Csv),
            magic_mime: None,
            decompressed_from: None,
        };
        let args = ComposeOpts {
            theme: PeekThemeName::IdeaDark,
            color: StyleMode::Plain,
            plain: true,
            raw: false,
            line_numbers: false,
            no_svg_anim: false,
            language: None,
            width: 0,
            margin: 0,
            image_mode: String::new(),
            background: String::new(),
            edge_density: 0.0,
        };
        let ctx = ComposeCtx {
            theme_manager: Rc::new(ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain)),
        };
        let mut modes: Vec<Box<dyn Mode>> = Vec::new();
        compose(&source, &detected, &args, &ctx, &mut modes, CsvFormat::Csv)
            .expect("over-cap UTF-16 must degrade, not fail the open");
        assert_eq!(modes.len(), 1, "only the Source view composes");
        assert_eq!(modes[0].label(), "Source");
        let warnings = modes[0].take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("table view unavailable")),
            "the refusal reason must surface as a warning: {warnings:?}"
        );
    }
}
