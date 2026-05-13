//! Per-type compose: CBZ read mode (paged image reader) + ZIP TOC.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, Detected};
use crate::types::archive;
use crate::types::comic::{CbzReadMode, cbz};
use crate::viewer::ComposeCtx;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::Mode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    match cbz::package::list_pages(source) {
        Ok(pages) if !pages.is_empty() => {
            modes.push(Box::new(CbzReadMode::new(
                source.clone(),
                ctx.image_config(args),
                pages,
            )));
        }
        Ok(_) => warnings.push("CBZ contains no image pages".to_string()),
        Err(e) => warnings.push(format!("CBZ unreadable: {e:#}")),
    }
    let (entries, mut listing_warnings) =
        match archive::reader::list_entries(source, ArchiveFormat::Zip) {
            Ok(e) => (e, Vec::new()),
            Err(e) => (Vec::new(), vec![format!("Failed to list CBZ: {e:#}")]),
        };
    warnings.append(&mut listing_warnings);
    modes.push(Box::new(ListingMode::new("CBZ", "TOC", entries, warnings)));
    Ok(())
}
