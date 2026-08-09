//! Per-type compose: PDF page-render + text-extraction + embedded
//! files listing.

use anyhow::Result;
use peek_detect::Detected;
use peek_io::InputSource;

use crate::types::image::pipeline::FitMode;
use crate::types::pdf::{self, PdfPageRenderer, PdfTextRenderer};
use crate::viewer::ComposeOpts;
use crate::viewer::listing::{ListingMode, from_flat_paths};
use crate::viewer::modes::{Mode, RenderedTextMode};
use crate::viewer::paged::PagedImageMode;
use crate::viewer::{ComposeCtx, image_config};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    // Page-render + text-extraction + /EmbeddedFiles listing. If
    // pdfium can't open the file the open error rides through
    // FileInfo warnings via the universal tail, so the user lands on
    // Info with the reason instead of a silent fall-through.
    if let Ok(doc) = pdf::package::open_doc(source) {
        if doc.page_count() > 0 {
            // PDF pages are usually portrait + dense — fitting both
            // axes into the viewport (Contain) crushes a full A4 page
            // into ~30 illegible rows. FitWidth fills the terminal
            // width at correct aspect ratio; vertical scroll covers the
            // overflow. The user can cycle back to Contain via `f`.
            let mut cfg = image_config(args);
            cfg.fit = FitMode::FitWidth;
            modes.push(Box::new(PagedImageMode::new(
                PdfPageRenderer::new(doc.clone()),
                cfg,
            )));
        }
        // Only offer the text view when the document actually carries a
        // text layer. Image-only scans and outlined-vector artwork
        // (`.ai`) extract nothing, so the tab would render a wall of
        // "[text unavailable]" — skip it instead.
        if doc.has_extractable_text() {
            modes.push(Box::new(RenderedTextMode::new(PdfTextRenderer::new(
                doc.clone(),
            ))));
        }
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
