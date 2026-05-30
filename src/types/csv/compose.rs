//! Per-type compose: aligned table view + paired Source ContentMode for
//! CSV / TSV. Info / Hex / Help / About are appended by the central
//! `Registry::compose_modes` tail.

use std::rc::Rc;

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::csv::format::CsvFormat;
use crate::types::csv::parse::{CellKind, CsvData, classify_cell};
use crate::viewer::ComposeCtx;
use crate::viewer::modes::{ContentMode, ContentModeConfig, Mode};
use crate::viewer::table::rows_mode::{Alignment, RowsTableMode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: CsvFormat,
) -> Result<()> {
    let data = CsvData::open(source, fmt)?;
    modes.push(Box::new(build_csv_mode(data)));
    // Paired Source view: raw CSV bytes, no syntax token (no robust CSV
    // syntax shipped with two-face).
    let line_source = source.open_line_source()?;
    modes.push(Box::new(ContentMode::new(
        source.clone(),
        line_source,
        Rc::clone(&ctx.theme_manager),
        ctx.theme_name,
        ContentModeConfig {
            label: "Source",
            line_numbers: args.line_numbers,
            ..Default::default()
        },
    )));
    Ok(())
}

/// Wrap a parsed `CsvData` in a `RowsTableMode`. The CSV-specific bits
/// (alignment inference via `classify_cell`, header-heuristic seed)
/// stay here so the shared mode keeps no per-source logic. Exposed
/// `pub` so tests in the mode itself can build a CSV-backed instance.
pub fn build_csv_mode(data: CsvData) -> RowsTableMode {
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
