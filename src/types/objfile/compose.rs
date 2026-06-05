//! Per-type compose: object files. InfoMode is the landing view (the
//! header summary — mirrors `file` / `readelf -h`); two table views,
//! Sections and Symbols, follow as `ObjectTableMode`s the Tab cycle
//! steps through. No extract path.

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::modes::{InfoMode, Mode};
use crate::viewer::table::TableMode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    // Info first — the most useful single view for an object file. The
    // universal tail dedupes by ModeId, so the later Info append is a
    // no-op.
    modes.push(Box::new(InfoMode::new()));

    // Two table views. A parse failure leaves the stack Info-only; the
    // Info section surfaces the same error.
    if let Ok(tables) = super::tables::build(source) {
        modes.push(Box::new(TableMode::new("Sections", tables.sections)));
        modes.push(Box::new(TableMode::new("Symbols", tables.symbols)));
    }
    Ok(())
}
