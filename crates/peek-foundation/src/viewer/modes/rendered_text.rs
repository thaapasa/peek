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
use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::viewer::modes::{
    Handled, Mode, ModeId, NEXT_PREV_MATCH_HELP, RenderCtx, Window, slice_window, step_search,
};
use crate::viewer::search::{self, SearchState, SearchTarget};
use crate::viewer::ui::{Action, HelpEntry};

const EXTRA_ACTIONS: &[HelpEntry] = &[(&[Action::OpenSearch], "Search"), NEXT_PREV_MATCH_HELP];

/// Rendered whole-document views (HTML, DOCX/ODT, RTF) build the entire
/// document in memory — none of the renderers stream. Above this size the
/// rendered view is refused and the raw source / hex view takes over, so a
/// pathological multi-hundred-MB document (or a zip-bomb `content.xml`
/// inside a small DOCX) stays openable. Mirrors `PRETTY_MAX_BYTES`.
pub const RENDER_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Cap-violation message — `None` when `len` fits under
/// [`RENDER_MAX_BYTES`]. Split from [`ensure_under_render_cap`] for
/// callers that report the refusal as a warning line instead of an `Err`
/// (the HTML renderer falls back to raw source).
pub fn render_cap_exceeded(len: u64, what: &str) -> Option<String> {
    (len > RENDER_MAX_BYTES).then(|| {
        format!(
            "{what} is {} MB (> {} MB render cap)",
            len / (1024 * 1024),
            RENDER_MAX_BYTES / (1024 * 1024)
        )
    })
}

/// Refuse a whole-document read when `len` exceeds [`RENDER_MAX_BYTES`].
pub fn ensure_under_render_cap(len: u64, what: &str) -> Result<()> {
    match render_cap_exceeded(len, what) {
        Some(msg) => Err(anyhow::anyhow!(msg)),
        None => Ok(()),
    }
}

/// Turns a parsed document into width-wrapped, ANSI-styled lines.
///
/// Implementors own the parsed document (the AST, the PDF handle, the
/// raw HTML bytes). `render` takes `&mut self` so a renderer can record
/// per-render warnings — PDF text extraction degrades page-by-page.
pub trait TextRenderer {
    /// Status-line label for the wrapping mode.
    fn label(&self) -> &'static str;

    /// `ModeId` the wrapping mode reports. Usually `ModeId::Rendered`;
    /// PDF text uses `ModeId::Content`.
    fn mode_id(&self) -> ModeId;

    /// Render the whole document, wrapped at `width`. `theme_name` is
    /// supplied fresh on every call so renderers that reach into the
    /// syntect theme registry (Markdown fenced code) pick up theme
    /// cycles instead of capturing the name at construction time.
    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> Result<Vec<String>>;

    /// Drain warnings accumulated during recent renders. Default: none.
    fn take_warnings(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// The render is invalidated whenever any input to the wrap changes.
/// `theme_name` is on the key so a theme cycle drops the cache and
/// re-renders with the new palette — without it, cached lines would
/// stay painted in the previous theme until a resize or color cycle
/// happened to change another key field.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    style_mode: StyleMode,
    theme_name: PeekThemeName,
}

struct Cached {
    key: CacheKey,
    lines: Vec<String>,
}

/// Whole-document read mode generic over its [`TextRenderer`].
pub struct RenderedTextMode<R: TextRenderer> {
    renderer: R,
    cache: Option<Cached>,
    /// Active text search over the rendered lines. Indices are the
    /// wrapped-line domain, so a resize clears it.
    search: Option<SearchState>,
}

impl<R: TextRenderer> RenderedTextMode<R> {
    pub fn new(renderer: R) -> Self {
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
        theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> Result<&[String]> {
        let key = CacheKey {
            width,
            style_mode,
            theme_name,
        };
        let needs = self.cache.as_ref().map(|c| c.key != key).unwrap_or(true);
        if needs {
            let lines = self.renderer.render(width, theme, theme_name, style_mode)?;
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
        let lines = self.ensure_rendered(
            ctx.term_cols,
            ctx.peek_theme,
            ctx.theme_name,
            ctx.peek_theme.style_mode,
        )?;
        let total = lines.len();
        let mut win = slice_window(lines, scroll, rows);
        search::overlay_window(&mut win, scroll, self.search.as_ref(), ctx.peek_theme);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.lines.len())
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let lines = self.ensure_rendered(
            ctx.term_cols,
            ctx.peek_theme,
            ctx.theme_name,
            ctx.peek_theme.style_mode,
        )?;
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
            Action::Next => step_search(&mut self.search, 1),
            Action::Prev => step_search(&mut self.search, -1),
            _ => Handled::No,
        }
    }

    fn set_search(&mut self, query: Option<&str>) -> SearchTarget {
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
                first.map_or(SearchTarget::Owned, SearchTarget::ScrollTo)
            }
            _ => {
                self.search = None;
                SearchTarget::Owned
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
