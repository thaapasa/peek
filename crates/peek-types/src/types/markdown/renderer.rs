//! `TextRenderer` for markdown, backed by the in-tree `render` module
//! (pulldown-cmark event stream → ANSI-styled wrapped lines).
//!
//! Whole-document render: pulldown-cmark has no streaming public API
//! that would help here, and the typical markdown file is well under
//! 1 MB. Over the render cap the view degrades to a placeholder line
//! plus a warning (the HTML renderer's pattern) — the source view
//! still streams the full file. The generic `RenderedTextMode` caches
//! per `(width, style_mode, theme_name)`, so resize, color cycle, and
//! theme cycle are the re-render triggers.

use std::rc::Rc;

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode, ThemeManager};
use crate::viewer::modes::{ModeId, TextRenderer, render_cap_exceeded};

use super::render;

pub(crate) struct MarkdownRenderer {
    source: InputSource,
    theme_manager: Rc<ThemeManager>,
    warning: Option<String>,
}

impl MarkdownRenderer {
    pub(crate) fn new(source: InputSource, theme_manager: Rc<ThemeManager>) -> Self {
        Self {
            source,
            theme_manager,
            warning: None,
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
        self.warning = None;
        let len = self.source.byte_len()?;
        if let Some(mut msg) = render_cap_exceeded(len, "markdown") {
            msg.push_str("; see the source view");
            self.warning = Some(msg.clone());
            return Ok(vec![msg]);
        }
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

    fn take_warnings(&mut self) -> Vec<String> {
        self.warning.take().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::modes::RENDER_MAX_BYTES;

    fn renderer(bytes: Vec<u8>) -> MarkdownRenderer {
        MarkdownRenderer::new(
            InputSource::memory(bytes, "test.md"),
            Rc::new(ThemeManager::new(
                PeekThemeName::default(),
                StyleMode::Plain,
            )),
        )
    }

    #[test]
    fn over_cap_refuses_with_warning_not_a_full_render() {
        let mut r = renderer(vec![b' '; (RENDER_MAX_BYTES + 1) as usize]);
        let tm = ThemeManager::new(PeekThemeName::default(), StyleMode::Plain);
        let lines = r
            .render(
                80,
                tm.peek_theme(),
                PeekThemeName::default(),
                StyleMode::Plain,
            )
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("render cap"), "got: {:?}", lines[0]);
        assert_eq!(r.take_warnings().len(), 1);
        assert!(r.take_warnings().is_empty(), "warning drains once");
    }

    #[test]
    fn under_cap_renders_normally() {
        let mut r = renderer(b"# Title\n\nbody\n".to_vec());
        let tm = ThemeManager::new(PeekThemeName::default(), StyleMode::Plain);
        let lines = r
            .render(
                80,
                tm.peek_theme(),
                PeekThemeName::default(),
                StyleMode::Plain,
            )
            .unwrap();
        assert!(lines.iter().any(|l| l.contains("Title")));
        assert!(r.take_warnings().is_empty());
    }
}
