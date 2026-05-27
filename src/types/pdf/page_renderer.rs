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
//! page entirely. Single-slot cache means stepping `n` / `p` evicts
//! the prior page — bounded memory regardless of document length.

use std::cell::RefCell;
use std::sync::Arc;

use anyhow::Result;
use image::DynamicImage;

use crate::types::image::pipeline::ImageConfig;
use crate::types::image::pipeline::render::{
    self as image_render, GridWindow, TermSize, prepare_decoded,
};
use crate::viewer::paged::{PageRenderer, PagedRender, RenderArgs};

use super::package::Doc;

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
}

impl PageRenderer for PdfPageRenderer {
    fn page_count(&self) -> usize {
        self.doc.page_count()
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
                return Ok(placeholder(
                    &format!("[page {} render failed]", idx + 1),
                    args.term,
                ));
            }
        };
        let mut config = config;
        config.style_mode = args.style_mode;
        let prep = prepare_decoded((*img).clone(), &config, args.term);
        if args.zoom.is_one() {
            // Fast path: prep already at base grid; crop visible window
            // for the current pan.
            let viewport_cols = prep.cols.min(args.term.cols).max(1);
            let viewport_rows = prep.rows.min(args.term.rows).max(1);
            let max_x = prep.cols.saturating_sub(viewport_cols);
            let max_y = prep.rows.saturating_sub(viewport_rows);
            let sx = args.scroll_x.min(max_x);
            let sy = args.scroll_y.min(max_y);
            let window = GridWindow {
                col_start: sx,
                col_end: sx + viewport_cols,
                row_start: sy,
                row_end: sy + viewport_rows,
            };
            let lines = image_render::render_prepared(&prep, &config, window);
            return Ok(PagedRender {
                lines,
                effective_cols: prep.cols,
                effective_rows: prep.rows,
                viewport_cols,
                viewport_rows,
            });
        }
        let result = image_render::render_prepared_zoomed(
            &prep,
            &config,
            args.term,
            args.zoom.factor(),
            args.scroll_x,
            args.scroll_y,
        );
        Ok(PagedRender {
            lines: result.lines,
            effective_cols: result.effective_cols,
            effective_rows: result.effective_rows,
            viewport_cols: result.viewport_cols,
            viewport_rows: result.viewport_rows,
        })
    }
}

fn placeholder(text: &str, term: TermSize) -> PagedRender {
    let cols = (text.len() as u32).max(1).min(term.cols);
    PagedRender {
        lines: vec![text.to_string()],
        effective_cols: cols,
        effective_rows: 1,
        viewport_cols: cols,
        viewport_rows: 1,
    }
}
