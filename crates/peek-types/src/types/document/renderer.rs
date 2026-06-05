//! `TextRenderer` for the shared word-processing AST.
//!
//! Walks an owned [`super::ast::Doc`] through the shared renderer. The
//! mode mechanics (caching, search, windowing) live in the generic
//! [`RenderedTextMode`]; this is only the per-format render body.
//!
//! Format-agnostic — DOCX and ODT both parse to `Doc` and reuse this.

use anyhow::Result;

use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::types::document::ast::Doc;
use crate::types::document::render;
use crate::viewer::modes::{ModeId, TextRenderer};

pub(crate) struct DocRenderer {
    doc: Doc,
}

impl DocRenderer {
    pub(crate) fn new(doc: Doc) -> Self {
        Self { doc }
    }
}

impl TextRenderer for DocRenderer {
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
        render::render(&self.doc, width, theme, style_mode)
    }
}
