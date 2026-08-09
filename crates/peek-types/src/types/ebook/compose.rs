//! Per-type compose: EPUB read mode + ZIP listing TOC.

use anyhow::Result;
use peek_detect::Detected;
use peek_io::InputSource;

use crate::types::archive;
use crate::types::ebook::epub::{self, EpubReader};
use crate::viewer::modes::Mode;
use crate::viewer::{ComposeCtx, ComposeOpts, image_config};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    match epub::package::open(source) {
        Ok(pkg) => modes.push(Box::new(EpubReader::into_mode(
            source.clone(),
            image_config(args),
            pkg,
        ))),
        Err(e) => warnings.push(format!("EPUB metadata unreadable: {e:#}")),
    }
    archive::reader::push_zip_toc(source, "EPUB", warnings, modes);
    Ok(())
}
