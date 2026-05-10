//! RTF read mode. Renders the parsed AST per (width, style_mode) and
//! caches the result. Cache invalidates on resize / color cycle.

use anyhow::Result;

use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::modes::{Mode, ModeId, RenderCtx, Window, slice_window};

use super::parse::Parsed;
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

pub(crate) struct RtfReadMode {
    #[allow(dead_code)]
    source: InputSource,
    parsed: Parsed,
    cache: Option<Cached>,
}

impl RtfReadMode {
    pub(crate) fn new(source: InputSource, parsed: Parsed) -> Self {
        Self {
            source,
            parsed,
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
            let lines = render::render(&self.parsed, width, theme, style_mode)?;
            self.cache = Some(Cached { key, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

impl Mode for RtfReadMode {
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
}
