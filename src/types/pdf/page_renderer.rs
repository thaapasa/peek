//! PDF page renderer: rasterizes one page via Pdfium and ASCII-renders
//! it through the shared image pipeline. Plugged into the generic
//! [`crate::viewer::paged::PagedImageMode`] — same caching shape and
//! navigation as [`crate::types::comic::cbz::CbzPageRenderer`].

use anyhow::Result;

use crate::types::image::pipeline::ImageConfig;
use crate::types::image::pipeline::render::{
    self as image_render, GridWindow, TermSize, prepare_decoded,
};
use crate::viewer::cell_size::cell_aspect_h_over_w;
use crate::viewer::paged::{self, PageCacheKey, PageRenderer};

use super::package::Doc;

pub(crate) struct PdfPageRenderer {
    doc: Doc,
}

impl PdfPageRenderer {
    pub(crate) fn new(doc: Doc) -> Self {
        Self { doc }
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
        key: &PageCacheKey,
        warnings: &mut Vec<String>,
    ) -> Result<Vec<String>> {
        let mut config = config;
        config.style_mode = key.style_mode;
        let term = TermSize {
            cols: key.width as u32,
            rows: paged::pipe_rows(key.rows),
            cell_h_over_w: cell_aspect_h_over_w(),
        };
        // Rasterize at ~16 px per terminal column. Pdfium auto-scales
        // height to preserve native aspect ratio, so the downstream
        // image pipeline receives a correctly-proportioned bitmap and
        // FitWidth can size it to the terminal grid without squashing.
        let px_w = (key.width as u32 * 16).clamp(64, 4096);
        let img = match self.doc.render_page(idx, px_w) {
            Ok(i) => i,
            Err(e) => {
                warnings.push(format!("page {}: render failed: {e:#}", idx + 1));
                return Ok(vec![format!("[page {} render failed]", idx + 1)]);
            }
        };
        let prep = prepare_decoded(img, &config, term);
        let window = GridWindow::full(prep.cols, prep.rows);
        Ok(image_render::render_prepared(&prep, &config, window))
    }
}
