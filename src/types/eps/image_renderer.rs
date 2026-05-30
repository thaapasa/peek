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
    /// Single-slot cache of the render *outcome* — success or failure.
    /// The Ghostscript render (a subprocess) or preview decode runs
    /// exactly once; a failure is cached too so a broken Render tab
    /// doesn't re-spawn `gs` on every pan / zoom / resize redraw.
    cached: RefCell<Option<Result<Arc<DynamicImage>, String>>>,
}

impl EpsImageRenderer {
    pub(crate) fn new(source: EpsImageSource) -> Self {
        Self {
            source,
            cached: RefCell::new(None),
        }
    }

    fn bitmap(&self) -> Result<Arc<DynamicImage>, String> {
        if let Some(cached) = self.cached.borrow().as_ref() {
            return cached.clone();
        }
        let result = match &self.source {
            EpsImageSource::Preview(img) => Ok(Arc::clone(img)),
            EpsImageSource::Ghostscript {
                exe,
                postscript,
                crop_to_bbox,
            } => super::gs::render(exe, postscript, *crop_to_bbox)
                .map(Arc::new)
                .map_err(|e| format!("{e:#}")),
        };
        *self.cached.borrow_mut() = Some(result.clone());
        result
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
                warnings.push(format!("render failed: {e}"));
                Ok(image_placeholder("[render unavailable]", args.term))
            }
        }
    }
}
