//! Single-image [`PageRenderer`] for the Keynote deck preview.
//!
//! Wraps the decoded `preview.jpg` thumbnail and defers to the shared
//! single-bitmap window renderer, so the preview gets zoom / pan / fit /
//! background cycling for free — same shape as the EPS preview renderer.

use std::sync::Arc;

use anyhow::Result;
use image::DynamicImage;

use crate::types::image::paged_render::render_image_window;
use crate::types::image::pipeline::ImageConfig;
use crate::viewer::paged::{PageRenderer, PagedRender, RenderArgs};

pub(crate) struct PreviewRenderer {
    img: Arc<DynamicImage>,
}

impl PreviewRenderer {
    pub(crate) fn new(img: Arc<DynamicImage>) -> Self {
        Self { img }
    }
}

impl PageRenderer for PreviewRenderer {
    fn page_count(&self) -> usize {
        1
    }

    fn render_page(
        &self,
        _idx: usize,
        config: ImageConfig,
        args: RenderArgs,
        _warnings: &mut Vec<String>,
    ) -> Result<PagedRender> {
        Ok(render_image_window(&self.img, config, args))
    }
}
