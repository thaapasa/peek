//! Per-type compose: PDF page-render + text-extraction + embedded
//! files listing.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::pdf::{self, PdfPageMode, PdfTextRenderer};
use crate::viewer::listing::{ListingMode, from_flat_paths};
use crate::viewer::modes::{Mode, RenderedTextMode};
use crate::viewer::{ComposeCtx, image_config};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    // Page-render + text-extraction + /EmbeddedFiles listing. If
    // pdfium can't open the file the open error rides through
    // FileInfo warnings via the universal tail, so the user lands on
    // Info with the reason instead of a silent fall-through.
    if let Ok(doc) = pdf::package::open_doc(source) {
        if doc.page_count() > 0 {
            modes.push(Box::new(PdfPageMode::new(doc.clone(), image_config(args))));
        }
        modes.push(Box::new(RenderedTextMode::new(PdfTextRenderer::new(
            doc.clone(),
        ))));
        let embeds = doc.list_embeds();
        if !embeds.is_empty() {
            let entries = from_flat_paths(embeds);
            modes.push(Box::new(ListingMode::new(
                "PDF",
                "Embeds",
                entries,
                Vec::new(),
            )));
        }
    }
    Ok(())
}
