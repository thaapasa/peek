//! Generic whole-document read mode.
//!
//! Several file types — DOCX/ODT, RTF, HTML, PDF text — present the
//! same flat view: render the whole parsed document into width-wrapped,
//! ANSI-styled lines, cache the result per `(width, style_mode)`, and
//! offer text search over the wrapped lines. The cache invalidates on
//! resize and color cycle.
//!
//! The only thing that varies between them is *how the parsed document
//! becomes `Vec<String>`* — that one function is the [`TextRenderer`]
//! trait. `RenderedTextMode<R>` supplies everything else: caching,
//! windowing, search, and the whole `Mode` impl.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::modes::{
    Handled, Mode, ModeId, NEXT_PREV_MATCH_HELP, RenderCtx, Window, slice_window, step_search,
};
use crate::viewer::search::{self, SearchState};
use crate::viewer::ui::{Action, HelpEntry};

const EXTRA_ACTIONS: &[HelpEntry] = &[(&[Action::OpenSearch], "Search"), NEXT_PREV_MATCH_HELP];

/// Turns a parsed document into width-wrapped, ANSI-styled lines.
///
/// Implementors own the parsed document (the AST, the PDF handle, the
/// raw HTML bytes). `render` takes `&mut self` so a renderer can record
/// per-render warnings — PDF text extraction degrades page-by-page.
pub(crate) trait TextRenderer {
    /// Status-line label for the wrapping mode.
    fn label(&self) -> &'static str;

    /// `ModeId` the wrapping mode reports. Usually `ModeId::Rendered`;
    /// PDF text uses `ModeId::Content`.
    fn mode_id(&self) -> ModeId;

    /// Render the whole document, wrapped at `width`.
    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        style_mode: StyleMode,
    ) -> Result<Vec<String>>;

    /// Drain warnings accumulated during recent renders. Default: none.
    fn take_warnings(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// The render is invalidated whenever either input to the wrap changes.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    style_mode: StyleMode,
}

struct Cached {
    key: CacheKey,
    lines: Vec<String>,
}

/// Whole-document read mode generic over its [`TextRenderer`].
pub(crate) struct RenderedTextMode<R: TextRenderer> {
    renderer: R,
    cache: Option<Cached>,
    /// Active text search over the rendered lines. Indices are the
    /// wrapped-line domain, so a resize clears it.
    search: Option<SearchState>,
}

impl<R: TextRenderer> RenderedTextMode<R> {
    pub(crate) fn new(renderer: R) -> Self {
        Self {
            renderer,
            cache: None,
            search: None,
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
            let lines = self.renderer.render(width, theme, style_mode)?;
            self.cache = Some(Cached { key, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

impl<R: TextRenderer> Mode for RenderedTextMode<R> {
    fn id(&self) -> ModeId {
        self.renderer.mode_id()
    }

    fn label(&self) -> &str {
        self.renderer.label()
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.peek_theme, ctx.peek_theme.style_mode)?;
        let total = lines.len();
        let mut win = slice_window(lines, scroll, rows);
        search::overlay_window(&mut win, scroll, self.search.as_ref(), ctx.peek_theme);
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

    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {
        // A width change re-wraps the document — match line indices no
        // longer line up, so drop the search.
        self.search = None;
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::Back if self.search.is_some() => {
                self.search = None;
                Handled::Yes
            }
            Action::NextMatch => step_search(&mut self.search, 1),
            Action::PrevMatch => step_search(&mut self.search, -1),
            _ => Handled::No,
        }
    }

    fn set_search(&mut self, query: Option<&str>) -> Option<usize> {
        match query {
            Some(q) if !q.is_empty() => {
                let lines = self
                    .cache
                    .as_ref()
                    .map(|c| c.lines.as_slice())
                    .unwrap_or(&[]);
                let state = SearchState::scan(lines.iter(), q);
                let first = state.first_line();
                self.search = Some(state);
                first
            }
            _ => {
                self.search = None;
                None
            }
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        self.search
            .as_ref()
            .map(|s| vec![s.status_segment(theme)])
            .unwrap_or_default()
    }

    fn take_warnings(&mut self) -> Vec<String> {
        self.renderer.take_warnings()
    }
}
