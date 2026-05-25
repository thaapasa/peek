//! Per-type compose for font files. Phase 1: no source view — fonts
//! are binary containers, so the source mode would be a wall of
//! mojibake. The rich Info section (from `FileExtras::Font`) plus the
//! universal Hex / About / Help tail is the whole viewer for now.
//! Phase 2 will push a specimen render mode here.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::viewer::ComposeCtx;
use crate::viewer::modes::Mode;

pub fn compose(
    _source: &InputSource,
    _detected: &Detected,
    _args: &Args,
    _ctx: &ComposeCtx,
    _modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    // No type-specific modes yet. Info + Hex from the universal tail
    // give the user metadata + raw bytes; Phase 2 inserts a specimen
    // render here above the Info aux mode so the default open lands on
    // the rendered preview.
    Ok(())
}
