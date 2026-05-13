//! Per-type compose: disk images. ISO gets a directory-tree TOC;
//! DMG / Raw skip the TOC (no filesystem walker available) and the
//! universal Info tail carries the metadata.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{Detected, DiskImageFormat};
use crate::viewer::ComposeCtx;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::{InfoMode, Mode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &Args,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: DiskImageFormat,
) -> Result<()> {
    match fmt {
        DiskImageFormat::Iso => {
            let (entries, warnings) = match crate::types::disk_image::iso_listing::list_iso(source)
            {
                Ok(e) => (e, Vec::new()),
                Err(e) => (Vec::new(), vec![format!("Failed to list ISO: {e:#}")]),
            };
            modes.push(Box::new(ListingMode::new(
                fmt.label(),
                "TOC",
                entries,
                warnings,
            )));
        }
        DiskImageFormat::Dmg | DiskImageFormat::Raw => {
            // No content / TOC view — push Info as the primary so
            // disk-image metadata is what the user lands on. The
            // universal tail dedupes by ModeId, so the later Info
            // append becomes a no-op.
            modes.push(Box::new(InfoMode::new()));
        }
    }
    Ok(())
}
