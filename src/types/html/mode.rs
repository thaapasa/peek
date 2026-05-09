//! Rendered HTML view backed by `html2text`.
//!
//! Renders the whole document into wrapped, ANSI-styled lines on first
//! use and on terminal width changes. Rendering is whole-document
//! (html2text has no streaming API), so very large HTML may pause on
//! first render — typical pages are well under 1 MB and render
//! instantly.

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::StyleMode;
use crate::viewer::modes::{Mode, ModeId, RenderCtx, Window, slice_window};

use super::render;

pub(crate) struct RenderedMode {
    source: InputSource,
    style_mode: StyleMode,
    /// Cached render keyed by the width it was produced for. `None`
    /// before the first render; replaced when `term_cols` changes.
    cache: Option<Cached>,
}

struct Cached {
    width: usize,
    lines: Vec<String>,
}

impl RenderedMode {
    pub(crate) fn new(source: InputSource, style_mode: StyleMode) -> Self {
        Self {
            source,
            style_mode,
            cache: None,
        }
    }

    fn ensure_rendered(&mut self, width: usize) -> Result<&[String]> {
        let needs_render = self
            .cache
            .as_ref()
            .map(|c| c.width != width)
            .unwrap_or(true);
        if needs_render {
            let bytes = self.source.read_bytes()?;
            let lines = render::render(&bytes, width.max(20), self.style_mode)?;
            self.cache = Some(Cached { width, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

impl Mode for RenderedMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Rendered"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines = self.ensure_rendered(ctx.term_cols)?;
        let total = lines.len();
        let win = slice_window(lines, scroll, rows);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.lines.len())
    }
}
