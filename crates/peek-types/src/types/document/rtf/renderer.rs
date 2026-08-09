//! `TextRenderer` for the parsed RTF stream.
//!
//! RTF stays outside the shared word-processing AST — its on-the-wire
//! shape is a flat painter-tagged text stream — so it has its own
//! `render`. The mode mechanics come from the generic
//! [`RenderedTextMode`].

use anyhow::Result;
use peek_theme::{PeekTheme, PeekThemeName, StyleMode};

use super::parse::Parsed;
use super::render;
use crate::viewer::modes::{ModeId, TextRenderer};

pub(crate) struct RtfRenderer {
    parsed: Parsed,
}

impl RtfRenderer {
    pub(crate) fn new(parsed: Parsed) -> Self {
        Self { parsed }
    }
}

impl TextRenderer for RtfRenderer {
    fn label(&self) -> &'static str {
        "Read"
    }

    fn mode_id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        _theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        render::render(&self.parsed, width, theme, style_mode)
    }
}
