//! Per-type compose for font files. Pushes a [`SpecimenMode`] as the
//! primary view (rasterised hard-coded sample sentence routed through
//! the ASCII image pipeline). No source view — fonts are binary
//! containers, so the universal Hex aux mode handles raw byte
//! inspection.
//!
//! Specimen rasterisation is best-effort: a face that fontdue can't
//! parse falls through silently, leaving the user with the Info +
//! Hex tail from the universal append (still useful).

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::font::specimen;
use crate::types::font::specimen_mode::SpecimenMode;
use crate::viewer::ComposeCtx;
use crate::viewer::modes::Mode;

/// Vertical pixel budget for the rendered specimen canvas. Chosen so
/// the image pipeline downsamples to a reasonable terminal height
/// (~30–40 rows) at common cell aspects. Smaller would force fontdue
/// to rasterise tiny glyphs (poor coverage maps); larger wastes RAM
/// for an output that's already being downsampled.
const SPECIMEN_TARGET_HEIGHT_PX: u32 = 320;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    // Best-effort: a malformed font (or one fontdue rejects) skips the
    // specimen and falls through to the universal Info + Hex tail.
    if let Ok(bytes) = source.read_bytes()
        && let Ok(image) = specimen::rasterise(&bytes, 0, SPECIMEN_TARGET_HEIGHT_PX)
    {
        let config = crate::viewer::image_config(args);
        modes.push(Box::new(SpecimenMode::new(image, config)));
    }
    Ok(())
}
