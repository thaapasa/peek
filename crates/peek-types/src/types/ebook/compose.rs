//! Per-type compose: EPUB read mode + ZIP listing TOC.

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, Detected};
use crate::types::archive;
use crate::types::ebook::epub::{self, EpubReadMode};
use crate::viewer::ComposeOpts;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::Mode;
use crate::viewer::{ComposeCtx, image_config};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    match epub::package::open(source) {
        Ok(pkg) => modes.push(Box::new(EpubReadMode::new(
            source.clone(),
            image_config(args),
            pkg,
        ))),
        Err(e) => warnings.push(format!("EPUB metadata unreadable: {e:#}")),
    }
    let (entries, mut listing_warnings) =
        match archive::reader::list_entries(source, ArchiveFormat::Zip) {
            Ok((e, _)) => (e, Vec::new()),
            Err(e) => (Vec::new(), vec![format!("Failed to list EPUB: {e:#}")]),
        };
    warnings.append(&mut listing_warnings);
    modes.push(Box::new(ListingMode::new("EPUB", "TOC", entries, warnings)));
    Ok(())
}
