//! `TextRenderer` for HTML, backed by `html2text`.
//!
//! Rendering is whole-document (html2text has no streaming API), so
//! very large HTML may pause on first render — typical pages are well
//! under 1 MB and render instantly. Above
//! [`RENDER_MAX_BYTES`](crate::viewer::modes::RENDER_MAX_BYTES) the render
//! is refused (one warning line) so a multi-hundred-MB page can't blow up
//! memory or freeze the UI; the raw Source view (always pushed alongside)
//! stands in. The generic [`RenderedTextMode`] caches the result per
//! `(width, style_mode)`, so a color cycle or resize re-renders and
//! everything else is a cache hit.

use anyhow::Result;
use peek_io::InputSource;
use peek_theme::{PeekTheme, PeekThemeName, StyleMode};

use super::render;
use crate::viewer::modes::{ModeId, TextRenderer, render_cap_placeholder};

pub(crate) struct HtmlRenderer {
    source: InputSource,
    /// Set when the last render refused (over cap / read error); drained
    /// through `take_warnings` and surfaced in Info.
    warning: Option<String>,
}

impl HtmlRenderer {
    pub(crate) fn new(source: InputSource) -> Self {
        Self {
            source,
            warning: None,
        }
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
        let len = self.source.byte_len()?;
        if let Some(lines) = render_cap_placeholder(len, "HTML", &mut self.warning) {
            return Ok(lines);
        }
        let bytes = self.source.read_bytes(peek_io::limits::Budget::Unbounded(
            "gated by render cap above",
        ))?;
        render::render(&bytes, width.max(20), style_mode)
    }

    fn take_warnings(&mut self) -> Vec<String> {
        self.warning.take().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use peek_theme::ThemeManager;

    use super::*;
    use crate::viewer::modes::RENDER_MAX_BYTES;

    fn theme() -> PeekTheme {
        ThemeManager::new(PeekThemeName::default(), StyleMode::Plain)
            .peek_theme()
            .clone()
    }

    #[test]
    fn over_cap_refuses_with_warning_not_a_full_render() {
        // A blob past the cap must not be read/parsed: one placeholder line,
        // one drained warning, no html2text pass.
        let big = vec![b' '; (RENDER_MAX_BYTES + 1) as usize];
        let mut r = HtmlRenderer::new(InputSource::memory(big, "huge.html"));
        let lines = r
            .render(80, &theme(), PeekThemeName::default(), StyleMode::Plain)
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("render cap"), "got: {:?}", lines[0]);
        assert_eq!(r.take_warnings().len(), 1);
        assert!(r.take_warnings().is_empty(), "warning drains once");
    }

    #[test]
    fn under_cap_renders_normally() {
        let mut r = HtmlRenderer::new(InputSource::memory(b"<p>hi</p>".to_vec(), "small.html"));
        let lines = r
            .render(80, &theme(), PeekThemeName::default(), StyleMode::Plain)
            .unwrap();
        assert!(lines.iter().any(|l| l.contains("hi")));
        assert!(r.take_warnings().is_empty());
    }
}
