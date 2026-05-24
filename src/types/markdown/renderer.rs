//! `TextRenderer` for markdown, backed by the in-tree `render` module
//! (pulldown-cmark event stream → ANSI-styled wrapped lines).
//!
//! Whole-document render: pulldown-cmark has no streaming public API
//! that would help here, and the typical markdown file is well under
//! 1 MB. The generic `RenderedTextMode` caches per
//! `(width, style_mode, theme_name)`, so resize, color cycle, and
//! theme cycle are the re-render triggers.

use std::rc::Rc;

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode, ThemeManager};
use crate::viewer::modes::{ModeId, TextRenderer};

use super::render;

pub(crate) struct MarkdownRenderer {
    source: InputSource,
    theme_manager: Rc<ThemeManager>,
}

impl MarkdownRenderer {
    pub(crate) fn new(source: InputSource, theme_manager: Rc<ThemeManager>) -> Self {
        Self {
            source,
            theme_manager,
        }
    }
}

impl TextRenderer for MarkdownRenderer {
    fn label(&self) -> &'static str {
        "Rendered"
    }

    fn mode_id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        let text = self.source.read_text()?;
        render::render(
            &text,
            width,
            theme,
            style_mode,
            &self.theme_manager,
            theme_name,
        )
    }
}
