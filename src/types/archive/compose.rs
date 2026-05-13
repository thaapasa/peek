//! Per-type compose: archive containers (zip / tar / 7z / cpio / ar
//! and their compressed variants) — listing-only TOC view.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, Detected};
use crate::types::archive;
use crate::viewer::ComposeCtx;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::Mode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &Args,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: ArchiveFormat,
) -> Result<()> {
    let (entries, warnings) = match archive::reader::list_entries(source, fmt) {
        Ok(e) => (e, Vec::new()),
        Err(e) => (Vec::new(), vec![format!("Failed to list archive: {e:#}")]),
    };
    modes.push(Box::new(ListingMode::new(
        fmt.label(),
        "TOC",
        entries,
        warnings,
    )));
    Ok(())
}
