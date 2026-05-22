//! CBZ page renderer: decodes one image page out of the ZIP container
//! and ASCII-renders it through the shared image pipeline. Plugged into
//! the generic [`crate::viewer::paged::PagedImageMode`] — `n` / `p`
//! step pages, the per-page render cache is keyed by `(page, cols,
//! rows, style, image config)`.

use anyhow::Result;

use crate::input::InputSource;
use crate::types::image::pipeline::ImageConfig;
use crate::types::image::pipeline::render::{
    self as image_render, GridWindow, TermSize, prepare_decoded,
};
use crate::viewer::cell_size::cell_aspect_h_over_w;
use crate::viewer::paged::{self, PageCacheKey, PageRenderer};

use super::package::{self, Page};

pub(crate) struct CbzPageRenderer {
    source: InputSource,
    pages: Vec<Page>,
}

impl CbzPageRenderer {
    pub(crate) fn new(source: InputSource, pages: Vec<Page>) -> Self {
        Self { source, pages }
    }
}

impl PageRenderer for CbzPageRenderer {
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn render_page(
        &self,
        idx: usize,
        config: ImageConfig,
        key: &PageCacheKey,
        warnings: &mut Vec<String>,
    ) -> Result<Vec<String>> {
        let page = &self.pages[idx];
        let mut zip = match package::open_zip(&self.source) {
            Ok(z) => z,
            Err(e) => {
                warnings.push(format!("page {}: {e:#}", idx + 1));
                return Ok(vec![format!("[page {} unavailable]", idx + 1)]);
            }
        };
        let bytes = match package::read_page(&mut zip, &page.full_path) {
            Ok(b) => b,
            Err(e) => {
                warnings.push(format!("page {}: {e:#}", idx + 1));
                return Ok(vec![format!("[page {} unavailable]", idx + 1)]);
            }
        };
        let img = match image::load_from_memory(&bytes) {
            Ok(i) => i,
            Err(e) => {
                warnings.push(format!("page {}: decode failed: {e:#}", idx + 1));
                return Ok(vec![format!("[page {} decode failed]", idx + 1)]);
            }
        };
        let mut config = config;
        config.style_mode = key.style_mode;
        let term = TermSize {
            cols: key.width as u32,
            rows: paged::pipe_rows(key.rows),
            cell_h_over_w: cell_aspect_h_over_w(),
        };
        let prep = prepare_decoded(img, &config, term);
        let window = GridWindow::full(prep.cols, prep.rows);
        Ok(image_render::render_prepared(&prep, &config, window))
    }
}
