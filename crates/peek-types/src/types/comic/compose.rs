//! Per-type compose: CBZ read mode (paged image reader) + ZIP TOC.

use anyhow::Result;
use peek_detect::Detected;
use peek_io::InputSource;

use crate::types::archive;
use crate::types::comic::{CbzPageRenderer, cbz};
use crate::viewer::modes::Mode;
use crate::viewer::paged::PagedImageMode;
use crate::viewer::{ComposeCtx, ComposeOpts, image_config};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    match cbz::package::list_pages(source) {
        Ok(pages) if !pages.is_empty() => {
            modes.push(Box::new(PagedImageMode::new(
                CbzPageRenderer::new(source.clone(), pages),
                image_config(args),
            )));
        }
        Ok(_) => warnings.push("CBZ contains no image pages".to_string()),
        Err(e) => warnings.push(format!("CBZ unreadable: {e:#}")),
    }
    archive::reader::push_zip_toc(source, "CBZ", warnings, modes);
    Ok(())
}
