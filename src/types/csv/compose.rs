//! Per-type compose: aligned table view + paired Source ContentMode for
//! CSV / TSV. Info / Hex / Help / About are appended by the central
//! `Registry::compose_modes` tail.

use std::rc::Rc;

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::csv::format::CsvFormat;
use crate::types::csv::parse::CsvData;
use crate::types::csv::table_mode::CsvTableMode;
use crate::viewer::ComposeCtx;
use crate::viewer::modes::{ContentMode, ContentModeConfig, Mode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: CsvFormat,
) -> Result<()> {
    let data = CsvData::open(source, fmt)?;
    modes.push(Box::new(CsvTableMode::new(data)));
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
