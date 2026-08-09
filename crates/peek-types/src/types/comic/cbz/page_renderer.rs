//! CBZ page renderer: decodes one image page out of the ZIP container
//! and ASCII-renders the visible viewport through the shared image
//! pipeline. Plugged into the generic
//! [`crate::viewer::paged::PagedImageMode`]. The native-resolution
//! decoded bitmap is cached per page; each render crops only the
//! viewport's pixel ROI from it (matching the raster-image happy
//! path), so memory stays bounded by viewport rather than effective
//! grid at high zoom. The decoded-page cache is a small most-recently-
//! used ring ([`MAX_CACHED_PAGES`]) so scrolling a long comic doesn't
//! accumulate every full-resolution page in memory.

use std::cell::RefCell;
use std::sync::Arc;

use anyhow::Result;
use image::DynamicImage;
use peek_io::InputSource;

use super::package::{self, Page};
use crate::types::image::paged_render::render_image_window;
use crate::types::image::pipeline::ImageConfig;
use crate::viewer::paged::{PageRenderer, PagedRender, RenderArgs, image_placeholder};

/// How many decoded full-resolution pages to retain. A comic page is the
/// only thing the cache holds, and the access pattern is sequential with
/// occasional back-flips, so a handful of MRU slots keeps adjacent
/// navigation re-decode-free while bounding memory regardless of comic
/// length. Each slot is one decoded bitmap; pan / zoom on the current
/// page is always a hit.
const MAX_CACHED_PAGES: usize = 4;

pub(crate) struct CbzPageRenderer {
    source: InputSource,
    pages: Vec<Page>,
    /// Native-resolution decoded source bitmaps, most-recently-used last.
    /// Populated lazily on first render of each page; re-decoding on
    /// every pan / zoom would be wasted I/O + Lanczos cost. Capped at
    /// [`MAX_CACHED_PAGES`] — the least-recently-used page is evicted
    /// once the ring is full. `Arc<DynamicImage>` keeps the cache cheap
    /// to clone out for the prep pipeline without a pixel copy.
    decoded: RefCell<Vec<(usize, Arc<DynamicImage>)>>,
}

impl CbzPageRenderer {
    pub(crate) fn new(source: InputSource, pages: Vec<Page>) -> Self {
        Self {
            source,
            pages,
            decoded: RefCell::new(Vec::new()),
        }
    }

    /// Get the decoded native-resolution bitmap for page `idx`,
    /// reading + decoding from the ZIP on a cache miss. Returns a
    /// `Result<Arc<_>>` so the caller can render a placeholder line
    /// on failure without poisoning the cache.
    fn decoded_source(&self, idx: usize) -> Result<Arc<DynamicImage>> {
        // Hit: promote to most-recently-used and clone out.
        if let Some(pos) = self.decoded.borrow().iter().position(|(i, _)| *i == idx) {
            let mut cache = self.decoded.borrow_mut();
            let entry = cache.remove(pos);
            let img = Arc::clone(&entry.1);
            cache.push(entry);
            return Ok(img);
        }
        let page = &self.pages[idx];
        let mut zip = package::open_zip(&self.source)?;
        let bytes = package::read_page(&mut zip, &page.full_path)?;
        let img = Arc::new(image::load_from_memory(&bytes)?);
        let mut cache = self.decoded.borrow_mut();
        if cache.len() >= MAX_CACHED_PAGES {
            cache.remove(0);
        }
        cache.push((idx, Arc::clone(&img)));
        Ok(img)
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
        args: RenderArgs,
        warnings: &mut Vec<String>,
    ) -> Result<PagedRender> {
        let img = match self.decoded_source(idx) {
            Ok(i) => i,
            Err(e) => {
                warnings.push(format!("page {}: {e:#}", idx + 1));
                return Ok(image_placeholder(
                    &format!("[page {} unavailable]", idx + 1),
                    args.term,
                ));
            }
        };
        Ok(render_image_window(&img, config, args))
    }
}

#[cfg(test)]
mod tests {
    use peek_theme::StyleMode;

    use super::*;
    use crate::viewer::image_render::{Background, FitMode, ImageMode, TermSize, ZoomLevel};

    fn cbz_fixture() -> InputSource {
        let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("test-books/sample-pages.cbz");
        InputSource::File(path)
    }

    fn config(fit: FitMode) -> ImageConfig {
        ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit,
        }
    }

    fn args(zoom: ZoomLevel) -> RenderArgs {
        RenderArgs {
            term: TermSize {
                cols: 80,
                rows: 40,
                cell_h_over_w: 2.0,
            },
            zoom,
            scroll_x: 0,
            scroll_y: 0,
            style_mode: StyleMode::Plain,
            text_overlay: false,
        }
    }

    /// The real CBZ renderer must produce an effective grid wider than the
    /// 80-col viewport for a landscape page at zoom 2× under fit=Contain —
    /// this is the overflow signal `PagedImageMode` keys horizontal pan off.
    #[test]
    fn landscape_page_overflows_viewport_at_zoom_2x() {
        let source = cbz_fixture();
        let pages = package::list_pages(&source).expect("list pages");
        assert!(pages.len() >= 2, "fixture needs a landscape page 2");
        let renderer = CbzPageRenderer::new(source, pages);

        let mut warnings = Vec::new();
        // Page index 1 is the landscape (1500×1000) page.
        let render = renderer
            .render_page(
                1,
                config(FitMode::Contain),
                args(ZoomLevel::preset(2)),
                &mut warnings,
            )
            .expect("render");
        assert!(
            render.effective_cols > render.viewport_cols,
            "expected effective grid ({}) wider than viewport ({}) at zoom 2×",
            render.effective_cols,
            render.viewport_cols,
        );
    }

    /// Same overflow without zoom: a landscape page under fit=FitHeight at
    /// zoom 1 still produces a grid wider than the viewport.
    #[test]
    fn landscape_page_overflows_viewport_fit_height_zoom_one() {
        let source = cbz_fixture();
        let pages = package::list_pages(&source).expect("list pages");
        let renderer = CbzPageRenderer::new(source, pages);

        let mut warnings = Vec::new();
        let render = renderer
            .render_page(
                1,
                config(FitMode::FitHeight),
                args(ZoomLevel::preset(1)),
                &mut warnings,
            )
            .expect("render");
        assert!(
            render.effective_cols > 80,
            "expected horizontal overflow at fit=FitHeight, got {}",
            render.effective_cols,
        );
    }
}
