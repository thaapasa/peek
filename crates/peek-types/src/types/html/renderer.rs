//! `TextRenderer` for HTML, backed by `html2text`.
//!
//! Rendering is whole-document (html2text has no streaming API), so
//! very large HTML may pause on first render — typical pages are well
//! under 1 MB and render instantly. The generic [`RenderedTextMode`]
//! caches the result per `(width, style_mode)`, so a color cycle or
//! resize re-renders and everything else is a cache hit.

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::viewer::modes::{ModeId, TextRenderer};

use super::render;

pub(crate) struct HtmlRenderer {
    source: InputSource,
}

impl HtmlRenderer {
    pub(crate) fn new(source: InputSource) -> Self {
        Self { source }
    }
}

impl TextRenderer for HtmlRenderer {
    fn label(&self) -> &'static str {
        "Rendered"
    }

    fn mode_id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn render(
        &mut self,
        width: usize,
        _theme: &PeekTheme,
        _theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        let bytes = self.source.read_bytes()?;
        render::render(&bytes, width.max(20), style_mode)
    }
}
