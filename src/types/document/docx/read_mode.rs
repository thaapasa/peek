//! DOCX read mode: walks the owned [`super::package::Doc`] AST through
//! the renderer per (width, style_mode). Cache invalidates when the
//! terminal resizes or the user cycles color, so a single open + walk
//! covers every redraw of a typical session.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::ui::Action;

use super::package::Doc;
use super::render;

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    style_mode: StyleMode,
}

struct Cached {
    key: CacheKey,
    lines: Vec<String>,
}

pub(crate) struct DocxReadMode {
    #[allow(dead_code)]
    source: InputSource,
    doc: Doc,
    cache: Option<Cached>,
}

impl DocxReadMode {
    pub(crate) fn new(source: InputSource, doc: Doc) -> Self {
        Self {
            source,
            doc,
            cache: None,
        }
    }

    fn ensure_rendered(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        style_mode: StyleMode,
    ) -> Result<&[String]> {
        let key = CacheKey { width, style_mode };
        let needs = self.cache.as_ref().map(|c| c.key != key).unwrap_or(true);
        if needs {
            let lines = render::render(&self.doc, width, theme, style_mode)?;
            self.cache = Some(Cached { key, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

impl Mode for DocxReadMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Read"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.peek_theme, ctx.peek_theme.style_mode)?;
        let total = lines.len();
        let win = slice_window(lines, scroll, rows);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.lines.len())
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.peek_theme, ctx.peek_theme.style_mode)?;
        for line in lines {
            out.write_line(line)?;
        }
        Ok(())
    }

    fn handle(&mut self, _action: Action) -> Handled {
        Handled::No
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let _ = theme;
        Vec::new()
    }
}
