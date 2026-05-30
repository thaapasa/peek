//! CBZ page renderer: decodes one image page out of the ZIP container
//! and ASCII-renders the visible viewport through the shared image
//! pipeline. Plugged into the generic
//! [`crate::viewer::paged::PagedImageMode`]. The native-resolution
//! decoded bitmap is cached per page; each render crops only the
//! viewport's pixel ROI from it (matching the raster-image happy
//! path), so memory stays bounded by viewport rather than effective
//! grid at high zoom.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use image::DynamicImage;

use crate::input::InputSource;
use crate::types::image::pipeline::ImageConfig;
use crate::viewer::paged::{
    PageRenderer, PagedRender, RenderArgs, image_placeholder, render_image_window,
};

use super::package::{self, Page};

pub(crate) struct CbzPageRenderer {
    source: InputSource,
    pages: Vec<Page>,
    /// Native-resolution decoded source bitmaps keyed by page index.
    /// Populated lazily on first render of each page and held for the
    /// lifetime of the renderer — re-decoding on every pan / zoom
    /// would be wasted I/O + Lanczos cost. `Arc<DynamicImage>` keeps
    /// the cache cheap to clone out for the prep pipeline without a
    /// pixel copy.
    decoded: RefCell<HashMap<usize, Arc<DynamicImage>>>,
}

impl CbzPageRenderer {
    pub(crate) fn new(source: InputSource, pages: Vec<Page>) -> Self {
        Self {
            source,
            pages,
            decoded: RefCell::new(HashMap::new()),
        }
    }

    /// Get the decoded native-resolution bitmap for page `idx`,
    /// reading + decoding from the ZIP on a cache miss. Returns a
    /// `Result<Arc<_>>` so the caller can render a placeholder line
    /// on failure without poisoning the cache.
    fn decoded_source(&self, idx: usize) -> Result<Arc<DynamicImage>> {
        if let Some(img) = self.decoded.borrow().get(&idx) {
            return Ok(Arc::clone(img));
        }
        let page = &self.pages[idx];
        let mut zip = package::open_zip(&self.source)?;
        let bytes = package::read_page(&mut zip, &page.full_path)?;
        let img = image::load_from_memory(&bytes)?;
        let img = Arc::new(img);
        self.decoded.borrow_mut().insert(idx, Arc::clone(&img));
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
