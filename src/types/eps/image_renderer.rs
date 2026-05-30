//! Single-image `PageRenderer` for the EPS image views.
//!
//! Two sources share one renderer: the embedded raster preview baked
//! into a binary DOS-EPS, and a Ghostscript render of the PostScript.
//! Both resolve to one `DynamicImage`, decoded / rendered lazily on
//! first draw and cached for the renderer's lifetime — so the "Render"
//! tab only spawns Ghostscript if the user actually opens it. The
//! viewport crop / zoom / pan is the shared `render_image_window`.

use std::cell::RefCell;
use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use image::DynamicImage;

use crate::types::image::pipeline::ImageConfig;
use crate::viewer::paged::{
    PageRenderer, PagedRender, RenderArgs, image_placeholder, render_image_window,
};

/// Where the displayed bitmap comes from.
pub(crate) enum EpsImageSource {
    /// Embedded DOS-EPS preview, already decoded at compose time (so a
    /// preview the image crate can't handle never produces a dead tab).
    Preview(Arc<DynamicImage>),
    /// Render the PostScript via Ghostscript on first draw.
    Ghostscript {
        exe: &'static str,
        postscript: Bytes,
        crop_to_bbox: bool,
    },
}

pub(crate) struct EpsImageRenderer {
    source: EpsImageSource,
    /// Single-slot cache of the produced bitmap. The preview decode or
    /// Ghostscript render runs once; pan / zoom / resize reuse it.
    cached: RefCell<Option<Arc<DynamicImage>>>,
}

impl EpsImageRenderer {
    pub(crate) fn new(source: EpsImageSource) -> Self {
        Self {
            source,
            cached: RefCell::new(None),
        }
    }

    fn bitmap(&self) -> Result<Arc<DynamicImage>> {
        if let Some(img) = self.cached.borrow().as_ref() {
            return Ok(Arc::clone(img));
        }
        let img = match &self.source {
            EpsImageSource::Preview(img) => Arc::clone(img),
            EpsImageSource::Ghostscript {
                exe,
                postscript,
                crop_to_bbox,
            } => Arc::new(super::gs::render(exe, postscript, *crop_to_bbox)?),
        };
        *self.cached.borrow_mut() = Some(Arc::clone(&img));
        Ok(img)
    }
}

impl PageRenderer for EpsImageRenderer {
    fn page_count(&self) -> usize {
        1
    }

    fn render_page(
        &self,
        _idx: usize,
        config: ImageConfig,
        args: RenderArgs,
        warnings: &mut Vec<String>,
    ) -> Result<PagedRender> {
        match self.bitmap() {
            Ok(img) => Ok(render_image_window(&img, config, args)),
            Err(e) => {
                warnings.push(format!("render failed: {e:#}"));
                Ok(image_placeholder("[render unavailable]", args.term))
            }
        }
    }
}
