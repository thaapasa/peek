//! PDF page renderer: rasterizes one page via Pdfium and ASCII-renders
//! the visible viewport through the shared image pipeline. Plugged
//! into the generic [`crate::viewer::paged::PagedImageMode`].
//!
//! Same shape as [`crate::types::comic::cbz::CbzPageRenderer`]: the
//! decoded native-resolution-ish bitmap is cached per page, and each
//! render crops only the viewport's pixel ROI from it. The Pdfium
//! rasterisation runs at a fixed high source DPI (capped at Pdfium's
//! 4096-pixel render ceiling) so ROI crops at zoom > 1 stay sharp
//! through the typical zoom range. Beyond the source DPI the viewport
//! upscales pixels rather than re-rasterising; pushing the source
//! higher than the cap would need either a smaller cache footprint
//! per page or Pdfium's clip-rect render API to skip the rest of the
//! page entirely. Single-slot caches (the bitmap, and a matching slot
//! for the text layer's positioned words) mean stepping `n` / `p`
//! evicts the prior page — bounded memory regardless of document
//! length. Also hosts the lazily-probed "has a text layer" flag that
//! gates the overlay toggle, and the `render_page` step that splices
//! the reconstructed-text overlay over the rendered lines when it's
//! on.

use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

use anyhow::Result;
use image::DynamicImage;

use crate::types::image::paged_render::{render_image_window, render_image_window_mapped};
use crate::types::image::pipeline::ImageConfig;
use crate::viewer::paged::{PageRenderer, PagedRender, RenderArgs, image_placeholder};

use super::package::Doc;
use super::text_overlay::{self, PageWords};

/// Pdfium's hard ceiling on render bitmap width / height. Pushing the
/// source higher than this is rejected; staying at the cap is the
/// sharpest viable render.
const PDFIUM_RENDER_CAP_PX: u32 = 4096;

pub(crate) struct PdfPageRenderer {
    doc: Doc,
    /// Single-slot cache: the most recently rendered page's bitmap at
    /// the fixed source DPI. Stepping pages evicts the prior slot.
    /// Holding only one bitmap keeps memory bounded regardless of
    /// document length — a 1000-page PDF doesn't accumulate per-page
    /// rasterisations.
    cached: RefCell<Option<CachedPage>>,
    /// Single-slot positioned-words cache for the text overlay, keyed
    /// like the bitmap slot. Extraction walks every char's bounds via
    /// FFI — too costly per redraw, cheap once per page.
    cached_words: RefCell<Option<(usize, Arc<PageWords>)>>,
    /// Lazily probed "document has a text layer" flag gating the
    /// overlay toggle. Probes a few leading pages once on first ask.
    has_text: OnceCell<bool>,
}

struct CachedPage {
    idx: usize,
    bitmap: Arc<DynamicImage>,
}

impl PdfPageRenderer {
    pub(crate) fn new(doc: Doc) -> Self {
        Self {
            doc,
            cached: RefCell::new(None),
            cached_words: RefCell::new(None),
            has_text: OnceCell::new(),
        }
    }

    /// Get the rasterised bitmap for page `idx`, calling Pdfium on a
    /// single-slot cache miss. The bitmap is rendered at the fixed
    /// source DPI cap so subsequent ROI crops at any zoom share the
    /// same source.
    fn page_bitmap(&self, idx: usize) -> Result<Arc<DynamicImage>> {
        if let Some(c) = self.cached.borrow().as_ref()
            && c.idx == idx
        {
            return Ok(Arc::clone(&c.bitmap));
        }
        let img = self.doc.render_page(idx, PDFIUM_RENDER_CAP_PX)?;
        let img = Arc::new(img);
        *self.cached.borrow_mut() = Some(CachedPage {
            idx,
            bitmap: Arc::clone(&img),
        });
        Ok(img)
    }

    /// Get page `idx`'s positioned words, extracting on a single-slot
    /// cache miss.
    fn page_words(&self, idx: usize) -> Result<Arc<PageWords>> {
        if let Some((i, w)) = self.cached_words.borrow().as_ref()
            && *i == idx
        {
            return Ok(Arc::clone(w));
        }
        let words = Arc::new(self.doc.page_words(idx)?);
        *self.cached_words.borrow_mut() = Some((idx, Arc::clone(&words)));
        Ok(words)
    }
}

impl PageRenderer for PdfPageRenderer {
    fn page_count(&self) -> usize {
        self.doc.page_count()
    }

    fn supports_text_overlay(&self) -> bool {
        *self
            .has_text
            .get_or_init(|| self.doc.has_extractable_text())
    }

    fn render_page(
        &self,
        idx: usize,
        config: ImageConfig,
        args: RenderArgs,
        warnings: &mut Vec<String>,
    ) -> Result<PagedRender> {
        let img = match self.page_bitmap(idx) {
            Ok(i) => i,
            Err(e) => {
                warnings.push(format!("page {}: render failed: {e:#}", idx + 1));
                return Ok(image_placeholder(
                    &format!("[page {} render failed]", idx + 1),
                    args.term,
                ));
            }
        };
        if !args.text_overlay {
            return Ok(render_image_window(&img, config, args));
        }
        let (mut render, map) = render_image_window_mapped(&img, config, args);
        match self.page_words(idx) {
            Ok(words) => {
                let cells = text_overlay::layout(&words, &map, config.margin);
                for (row, line) in render.lines.iter_mut().enumerate() {
                    if let Some(row_cells) = cells.get(&(row as u32)) {
                        *line = text_overlay::splice(line, row_cells);
                    }
                }
            }
            Err(e) => {
                warnings.push(format!("page {}: text overlay failed: {e:#}", idx + 1));
            }
        }
        Ok(render)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::pdf::package;
    use crate::viewer::image_render::{Background, FitMode, ImageMode, TermSize, ZoomLevel};
    use peek_io::InputSource;
    use peek_theme::StyleMode;

    /// Open the text-heavy fixture, or `None` when Pdfium isn't
    /// available in this environment (CI runs without the dylib) —
    /// callers skip in that case rather than fail.
    fn fixture_doc() -> Option<Doc> {
        let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("test-books/frankenstein.pdf");
        match package::open_doc(&InputSource::File(path)) {
            Ok(doc) => Some(doc),
            Err(e) => {
                eprintln!("skipping: pdfium unavailable ({e:#})");
                None
            }
        }
    }

    fn config() -> ImageConfig {
        ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit: FitMode::FitWidth,
        }
    }

    fn args(zoom: f32, text_overlay: bool) -> RenderArgs {
        RenderArgs {
            term: TermSize {
                cols: 100,
                rows: 50,
                cell_h_over_w: 2.0,
            },
            zoom: ZoomLevel::new(zoom),
            scroll_x: 0,
            scroll_y: 0,
            style_mode: StyleMode::Plain,
            text_overlay,
        }
    }

    /// One test, three phases: word extraction geometry, the overlay
    /// splicing real words into the render, and the toggle gating it.
    /// Single `#[test]` on purpose — Pdfium has process-wide C++ state
    /// and is not thread-safe, so concurrent test threads touching the
    /// same library segfault.
    #[test]
    fn text_overlay_end_to_end() {
        let Some(doc) = fixture_doc() else { return };

        // Page 1 (the first text page — page 0 is a cover image):
        // extraction yields words with sane in-page geometry.
        let words = doc.page_words(1).expect("page words");
        assert!(words.page_w > 0.0 && words.page_h > 0.0);
        assert!(
            words.words.len() >= 5,
            "expected a handful of words on the first text page, got {}",
            words.words.len()
        );
        for w in &words.words {
            assert!(!w.text.is_empty());
            assert!(w.width > 0.0 && w.height > 0.0, "{}: empty box", w.text);
            assert!(
                w.left >= -1.0 && w.left + w.width <= words.page_w + 1.0,
                "{}: x range outside page",
                w.text
            );
            assert!(
                w.top >= -1.0 && w.top + w.height <= words.page_h + 1.0,
                "{}: y range outside page",
                w.text
            );
        }

        // At a zoom where a text line spans ≳1 cell row, the overlay
        // must put real words from the text layer into the rendered
        // lines.
        let renderer = PdfPageRenderer::new(doc);
        assert!(renderer.supports_text_overlay());
        let mut warnings = Vec::new();
        let render = renderer
            .render_page(1, config(), args(4.0, true), &mut warnings)
            .expect("render");
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        let joined = render.lines.join("\n");
        let hit = words
            .words
            .iter()
            .filter(|w| w.text.len() >= 4)
            .any(|w| joined.contains(&w.text));
        assert!(hit, "no extracted word found in overlaid render:\n{joined}");

        // Overlay off → no recognizable words (the toggle actually
        // gates the splice).
        let render = renderer
            .render_page(1, config(), args(4.0, false), &mut warnings)
            .expect("render");
        let joined = render.lines.join("\n");
        let hit = words
            .words
            .iter()
            .filter(|w| w.text.len() >= 6)
            .any(|w| joined.contains(&w.text));
        assert!(!hit, "plain render unexpectedly contains text-layer words");
    }
}
