//! Presentation read mode: one slide at a time.
//!
//! Renders the slide at `current` through the shared document prose
//! renderer (`crate::types::document::render`). `n` / `p` step
//! forward / back through the deck, resetting scroll for the new slide;
//! while a search is active they step matches instead. The per-slide
//! render cache is keyed by `(width, style_mode, theme_name)` so a
//! resize or theme cycle re-renders only the visible slide and stepping
//! back reuses prior renders.
//!
//! Mirrors `EpubReadMode` rather than the generic `PagedImageMode<R>`:
//! slides are text-rich and want per-slide search — the same reasoning
//! the EPUB read mode documents for staying separate from the
//! paged-image shell. The slides are parsed up front into `Vec<Doc>`, so
//! unlike EPUB (which re-reads chapter HTML lazily) rendering touches no
//! I/O — it just walks the cached AST.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::output::PrintOutput;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::types::document::ast::Doc;
use crate::types::document::render;
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window, step_search};
use crate::viewer::paged::pipe_walk_pages;
use crate::viewer::search::{self, SearchState, SearchTarget};
use crate::viewer::ui::{Action, HelpEntry};

const EXTRA_ACTIONS: &[HelpEntry] = &[
    (
        // With a search active these step matches instead of slides.
        &[Action::Next, Action::Prev],
        "Next / previous slide",
    ),
    (&[Action::OpenSearch], "Search"),
];

/// Per-slide render invalidated whenever any wrap input changes. Theme
/// name is on the key so a theme cycle drops the cache and re-paints —
/// same shape as the generic `RenderedTextMode` cache key.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    style_mode: StyleMode,
    theme_name: PeekThemeName,
}

struct SlideCache {
    key: CacheKey,
    lines: Vec<String>,
}

pub(crate) struct PresentationReadMode {
    slides: Vec<Doc>,
    current: usize,
    cache: Vec<Option<SlideCache>>,
    /// Active text search over the current slide's rendered lines.
    /// Cleared on a slide step or resize — both change the line set the
    /// match indices point into.
    search: Option<SearchState>,
}

impl PresentationReadMode {
    pub(crate) fn new(slides: Vec<Doc>) -> Self {
        let mut cache = Vec::with_capacity(slides.len());
        cache.resize_with(slides.len(), || None);
        Self {
            slides,
            current: 0,
            cache,
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
        if self.slides.is_empty() {
            return Ok(&[]);
        }
        let idx = self.current;
        let key = CacheKey {
            width,
            style_mode,
            theme_name,
        };
        let needs = self.cache[idx]
            .as_ref()
            .map(|c| c.key != key)
            .unwrap_or(true);
        if needs {
            let lines = render::render(&self.slides[idx], width, theme, style_mode)?;
            self.cache[idx] = Some(SlideCache { key, lines });
        }
        Ok(&self.cache[idx].as_ref().expect("cache populated").lines)
    }
}

impl Mode for PresentationReadMode {
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
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.len())
    }

    /// Print mode walks every slide in order, separating each with a
    /// blank line — the whole deck materialises only on the pipe path;
    /// the interactive view stays single-slide.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.slides.len();
        let saved = self.current;
        let res = pipe_walk_pages(out, total, |i, out| {
            self.current = i;
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
        });
        self.current = saved;
        res
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        // Esc clears an active search before falling through to global back.
        if action == Action::Back && self.search.is_some() {
            self.search = None;
            return Handled::Yes;
        }
        match action {
            Action::Next => {
                if self.search.is_some() {
                    step_search(&mut self.search, 1)
                } else {
                    crate::viewer::paged::step_paged(&mut self.current, self.slides.len(), 1)
                }
            }
            Action::Prev => {
                if self.search.is_some() {
                    step_search(&mut self.search, -1)
                } else {
                    crate::viewer::paged::step_paged(&mut self.current, self.slides.len(), -1)
                }
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        if self.slides.is_empty() {
            return Vec::new();
        }
        let mut segs = vec![(
            format!("slide {}/{}", self.current + 1, self.slides.len()),
            theme.muted,
        )];
        if let Some(search) = &self.search {
            segs.push(search.status_segment(theme));
        }
        segs
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.slides.len() <= 1 {
            return Vec::new();
        }
        vec!["n/p:slide"]
    }

    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {
        // A width change re-wraps every slide — match line indices no
        // longer line up, so drop the search.
        self.search = None;
    }

    fn set_search(&mut self, query: Option<&str>) -> SearchTarget {
        match query {
            Some(q) if !q.is_empty() => {
                let lines = self
                    .cache
                    .get(self.current)
                    .and_then(|c| c.as_ref())
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
}
