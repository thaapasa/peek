//! Per-type compose for font files. Pushes a [`SpecimenMode`] as the
//! primary view (rasterised hard-coded sample sentence routed through
//! the ASCII image pipeline). No source view — fonts are binary
//! containers, so the universal Hex aux mode handles raw byte
//! inspection.
//!
//! Specimen rasterisation is best-effort: a face that fontdue can't
//! parse falls through silently, leaving the user with the Info +
//! Hex tail from the universal append (still useful).

use std::borrow::Cow;

use anyhow::Result;
use bytes::Bytes;

use crate::input::InputSource;
use crate::input::detect::{Detected, FileType};
use crate::types::font::info_gather;
use crate::types::font::specimen;
use crate::types::font::specimen_mode::SpecimenMode;
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::modes::Mode;

/// Vertical pixel budget for the rendered specimen canvas. Chosen so
/// the image pipeline downsamples to a reasonable terminal height
/// (~30–40 rows) at common cell aspects. Smaller would force fontdue
/// to rasterise tiny glyphs (poor coverage maps); larger wastes RAM
/// for an output that's already being downsampled.
const SPECIMEN_TARGET_HEIGHT_PX: u32 = 320;

pub fn compose(
    source: &InputSource,
    detected: &Detected,
    args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let FileType::Font(fmt) = detected.file_type else {
        return Ok(());
    };

    // Best-effort: a malformed font (or one fontdue rejects) skips the
    // specimen and falls through to the universal Info + Hex tail.
    if let Ok(bytes) = source.read_bytes() {
        // Unwrap WOFF to its inner sfnt before fontdue sees it. Bare
        // sfnt borrows through, so clone the refcounted handle rather
        // than copying; WOFF produces an owned buffer. SpecimenMode
        // keeps these bytes to re-rasterise on face cycle / zoom, so it
        // must hold the decoded sfnt, not the wrapper.
        let sfnt: Bytes = match crate::types::font::sfnt::decode(&bytes, fmt) {
            Ok(Cow::Borrowed(_)) => bytes.clone(),
            Ok(Cow::Owned(v)) => Bytes::from(v),
            Err(_) => return Ok(()),
        };
        if let Ok(image) = specimen::rasterise(&sfnt, 0, SPECIMEN_TARGET_HEIGHT_PX) {
            let config = crate::viewer::image_config(args);
            let face_count = info_gather::face_count(&sfnt);
            modes.push(Box::new(SpecimenMode::new(
                sfnt,
                face_count,
                image,
                SPECIMEN_TARGET_HEIGHT_PX,
                config,
            )));
        }
    }
    Ok(())
}
